use std::{borrow::Cow, collections::HashMap, path::Path};

use crate::{
    Execution, InferenceError, LoadedModel, initialization_error,
    moonshine_tokenizer::MoonshineTokenizer,
};
use ndarray::{Array2, ArrayD, ArrayViewD, Axis, IxDyn};
use ort::{
    ep, inputs,
    session::{Session, builder::GraphOptimizationLevel},
    value::{DynValueRef, TensorRef},
};
use quil_core::{
    ModelSpec, SAMPLE_RATE, TranscribeOptions, Transcriber, TranscriptionError, validate_pcm,
};

const START_TOKEN: i64 = 1;
const EOS_TOKEN: i64 = 2;

pub fn load(
    model: &ModelSpec,
    model_dir: &Path,
    execution: Execution,
) -> Result<LoadedModel, InferenceError> {
    if execution == Execution::Metal {
        return Err(InferenceError::Unavailable {
            backend: "moonshine",
            message: "ONNX Runtime has no Metal execution provider; use coreml or cpu".into(),
        });
    }
    let variant = match model.id.as_str() {
        "moonshine-tiny" => Variant::Tiny,
        "moonshine-base" => Variant::Base,
        _ => {
            return Err(InferenceError::Unavailable {
                backend: "moonshine",
                message: format!("unknown variant `{}`", model.id),
            });
        }
    };

    let environment = ort::init()
        .with_execution_providers(execution_providers(execution))
        .build()
        .map_err(|error| initialization_error("moonshine", error))?;
    let encoder = Session::builder(&environment)
        .and_then(|builder| {
            builder
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .commit_from_file(model_dir.join("encoder.onnx"))
        })
        .map_err(|error| initialization_error("moonshine encoder", error))?;
    let decoder = Session::builder(&environment)
        .and_then(|builder| {
            builder
                .with_optimization_level(GraphOptimizationLevel::Level3)?
                .commit_from_file(model_dir.join("decoder.onnx"))
        })
        .map_err(|error| initialization_error("moonshine decoder", error))?;
    let encoder_input_names = encoder
        .inputs()
        .iter()
        .map(|input| input.name().to_owned())
        .collect();
    let decoder_input_names = decoder
        .inputs()
        .iter()
        .map(|input| input.name().to_owned())
        .collect();
    let tokenizer = MoonshineTokenizer::from_file(&model_dir.join("tokenizer.json"))
        .map_err(|error| initialization_error("moonshine tokenizer", error))?;
    let execution_name = if execution == Execution::CoreMl {
        "ONNX Runtime 1.31 CoreML MLProgram + CPU partition fallback".to_owned()
    } else {
        "ONNX Runtime 1.31 CPU".to_owned()
    };

    Ok(LoadedModel {
        transcriber: Box::new(MoonshineTranscriber {
            _environment: environment,
            encoder,
            decoder,
            tokenizer,
            variant,
            encoder_input_names,
            decoder_input_names,
        }),
        execution: execution_name,
    })
}

fn execution_providers(execution: Execution) -> Vec<ep::ExecutionProviderDispatch> {
    if execution != Execution::CoreMl {
        // Measured on Apple M5 Max: CPU is faster than the current dynamic-shape
        // Core ML partition path for both Moonshine variants (BENCHMARKS.md).
        return vec![ep::CPU::default().build()];
    }
    let coreml = ep::CoreML::default()
        .with_model_format(ep::coreml::ModelFormat::MLProgram)
        .with_subgraphs(true)
        .with_compute_units(ep::coreml::ComputeUnits::All)
        .build();
    let coreml = coreml.error_on_failure();
    vec![coreml, ep::CPU::default().build()]
}

struct MoonshineTranscriber {
    _environment: ort::environment::Environment,
    encoder: Session,
    decoder: Session,
    tokenizer: MoonshineTokenizer,
    variant: Variant,
    encoder_input_names: Vec<String>,
    decoder_input_names: Vec<String>,
}

impl Transcriber for MoonshineTranscriber {
    fn transcribe(
        &mut self,
        samples: &[f32],
        options: &TranscribeOptions,
    ) -> Result<String, TranscriptionError> {
        validate_pcm(samples)?;
        if options
            .language
            .as_deref()
            .is_some_and(|language| language != "en")
        {
            return Err(moonshine_error("Moonshine Tiny/Base supports English only"));
        }
        let seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
        if seconds > 64.0 {
            return Err(moonshine_error(format!(
                "Moonshine supports at most 64 seconds per chunk, received {seconds:.2}"
            )));
        }
        let max_tokens = ((seconds * 6.0).ceil() as usize).clamp(1, 512);
        let tokens = self.generate(samples, max_tokens)?;
        self.tokenizer.decode(&tokens).map_err(moonshine_error)
    }
}

impl MoonshineTranscriber {
    fn encode(&mut self, samples: &[f32]) -> Result<ArrayD<f32>, TranscriptionError> {
        // Borrow the caller's PCM directly; no second raw-audio allocation is
        // created on the ONNX path, so the owner's zero-on-drop policy remains
        // sufficient for microphone samples.
        let audio = ndarray::ArrayView::from_shape((1, samples.len()), samples)
            .map_err(|error| moonshine_error(error.to_string()))?
            .into_dyn();
        let outputs = if self
            .encoder_input_names
            .iter()
            .any(|name| name == "attention_mask")
        {
            let mask = Array2::<i64>::ones((1, samples.len())).into_dyn();
            self.encoder
                .run(inputs![
                    "input_values" => TensorRef::from_array_view(audio.view()).map_err(value_error)?,
                    "attention_mask" => TensorRef::from_array_view(mask.view()).map_err(value_error)?,
                ])
                .map_err(ort_error)?
        } else {
            self.encoder
                .run(inputs![
                    "input_values" => TensorRef::from_array_view(audio.view()).map_err(value_error)?,
                ])
                .map_err(ort_error)?
        };
        outputs
            .get("last_hidden_state")
            .ok_or_else(|| moonshine_error("encoder output last_hidden_state is missing"))?
            .try_extract_array::<f32>()
            .map(|array| array.to_owned())
            .map_err(ort_error)
    }

    fn generate(
        &mut self,
        samples: &[f32],
        max_tokens: usize,
    ) -> Result<Vec<i64>, TranscriptionError> {
        let encoder_hidden_states = self.encode(samples)?;
        let audio_attention_mask = Array2::<i64>::ones((1, samples.len())).into_dyn();
        let mut cache = KeyValueCache::new(self.variant);
        let mut tokens = vec![START_TOKEN];
        let mut input_ids = Array2::from_shape_vec((1, 1), vec![START_TOKEN])
            .map_err(|error| moonshine_error(error.to_string()))?
            .into_dyn();

        for iteration in 0..max_tokens {
            let use_cache = iteration > 0;
            let use_cache_array = ndarray::arr1(&[use_cache]).into_dyn();
            let mut run_inputs: Vec<(Cow<'_, str>, DynValueRef<'_>)> = vec![
                (Cow::Borrowed("input_ids"), borrowed_value(&input_ids)?),
                (
                    Cow::Borrowed("encoder_hidden_states"),
                    borrowed_value(&encoder_hidden_states)?,
                ),
                (
                    Cow::Borrowed("use_cache_branch"),
                    borrowed_value(&use_cache_array)?,
                ),
            ];
            if self
                .decoder_input_names
                .iter()
                .any(|name| name == "encoder_attention_mask")
            {
                run_inputs.push((
                    Cow::Borrowed("encoder_attention_mask"),
                    borrowed_value(&audio_attention_mask)?,
                ));
            }
            cache.append_inputs(&mut run_inputs)?;

            let outputs = self.decoder.run(run_inputs).map_err(ort_error)?;
            let next_token = {
                let logits = outputs
                    .get("logits")
                    .ok_or_else(|| moonshine_error("decoder output logits is missing"))?
                    .try_extract_array::<f32>()
                    .map_err(ort_error)?;
                argmax_last_token(&logits)?
            };
            tokens.push(next_token);
            if next_token == EOS_TOKEN {
                break;
            }
            input_ids = Array2::from_shape_vec((1, 1), vec![next_token])
                .map_err(|error| moonshine_error(error.to_string()))?
                .into_dyn();
            cache.update_from_outputs(&outputs, use_cache)?;
        }
        Ok(tokens)
    }
}

fn borrowed_value<'a, T>(array: &'a ArrayD<T>) -> Result<DynValueRef<'a>, TranscriptionError>
where
    T: ort::value::PrimitiveTensorElementType + std::fmt::Debug + Clone + 'static,
{
    TensorRef::from_array_view(array.view())
        .map(|value| value.into_dyn())
        .map_err(value_error)
}

fn argmax_last_token(logits: &ArrayViewD<'_, f32>) -> Result<i64, TranscriptionError> {
    let shape = logits.shape();
    if shape.len() != 3 || shape[0] != 1 || shape[1] == 0 || shape[2] == 0 {
        return Err(moonshine_error(format!(
            "unexpected logits shape {shape:?}"
        )));
    }
    let last_sequence = logits.index_axis(Axis(1), shape[1] - 1);
    let last = last_sequence.index_axis(Axis(0), 0);
    let mut best_token = None;
    let mut best_score = f32::NEG_INFINITY;
    for (index, score) in last.iter().copied().enumerate() {
        if score.is_finite() && score > best_score {
            best_score = score;
            best_token = Some(index as i64);
        }
    }
    best_token.ok_or_else(|| moonshine_error("decoder produced no finite logits"))
}

struct KeyValueCache {
    tensors: HashMap<String, ArrayD<f32>>,
    layers: usize,
}

impl KeyValueCache {
    fn new(variant: Variant) -> Self {
        let mut tensors = HashMap::new();
        for layer in 0..variant.layers() {
            for attention in ["decoder", "encoder"] {
                for part in ["key", "value"] {
                    tensors.insert(
                        cache_name(layer, attention, part),
                        ArrayD::zeros(IxDyn(&[1, 8, 1, variant.head_dim()])),
                    );
                }
            }
        }
        Self {
            tensors,
            layers: variant.layers(),
        }
    }

    fn append_inputs<'a>(
        &'a self,
        inputs: &mut Vec<(Cow<'a, str>, DynValueRef<'a>)>,
    ) -> Result<(), TranscriptionError> {
        for layer in 0..self.layers {
            for attention in ["decoder", "encoder"] {
                for part in ["key", "value"] {
                    let name = cache_name(layer, attention, part);
                    let tensor = self
                        .tensors
                        .get(&name)
                        .ok_or_else(|| moonshine_error("missing KV input"))?;
                    inputs.push((Cow::Owned(name), borrowed_value(tensor)?));
                }
            }
        }
        Ok(())
    }

    fn update_from_outputs(
        &mut self,
        outputs: &ort::session::SessionOutputs,
        use_cache: bool,
    ) -> Result<(), TranscriptionError> {
        for layer in 0..self.layers {
            for attention in ["decoder", "encoder"] {
                if use_cache && attention == "encoder" {
                    continue;
                }
                for part in ["key", "value"] {
                    let output_name = format!("present.{layer}.{attention}.{part}");
                    let output = outputs.get(&output_name).ok_or_else(|| {
                        moonshine_error(format!("decoder output {output_name} is missing"))
                    })?;
                    let tensor = output
                        .try_extract_array::<f32>()
                        .map_err(ort_error)?
                        .to_owned();
                    self.tensors
                        .insert(cache_name(layer, attention, part), tensor);
                }
            }
        }
        Ok(())
    }
}

fn cache_name(layer: usize, attention: &str, part: &str) -> String {
    format!("past_key_values.{layer}.{attention}.{part}")
}

#[derive(Clone, Copy)]
enum Variant {
    Tiny,
    Base,
}

impl Variant {
    fn layers(self) -> usize {
        match self {
            Self::Tiny => 6,
            Self::Base => 8,
        }
    }

    fn head_dim(self) -> usize {
        match self {
            Self::Tiny => 36,
            Self::Base => 52,
        }
    }
}

fn moonshine_error(error: impl std::fmt::Display) -> TranscriptionError {
    TranscriptionError::Backend {
        backend: "moonshine",
        message: error.to_string(),
    }
}

fn ort_error(error: ort::Error) -> TranscriptionError {
    moonshine_error(error)
}

fn value_error(error: ort::Error) -> TranscriptionError {
    moonshine_error(error)
}
