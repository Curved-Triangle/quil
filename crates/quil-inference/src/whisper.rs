use std::path::Path;

use quil_core::{ModelSpec, TranscribeOptions, Transcriber, TranscriptionError, validate_pcm};
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::{Execution, InferenceError, LoadedModel, initialization_error};

pub fn load(
    model: &ModelSpec,
    model_dir: &Path,
    execution: Execution,
) -> Result<LoadedModel, InferenceError> {
    if execution == Execution::CoreMl {
        return Err(InferenceError::Unavailable {
            backend: "whisper",
            message:
                "Core ML encoder artifacts are not yet in the verified catalog; use metal or cpu"
                    .into(),
        });
    }
    let use_gpu = execution != Execution::Cpu;
    let mut context_params = WhisperContextParameters::default();
    context_params.use_gpu(use_gpu);
    let context = WhisperContext::new_with_params(model_dir.join("model.bin"), context_params)
        .map_err(|error| initialization_error("whisper", error))?;
    let execution_name = if use_gpu {
        format!(
            "whisper.cpp {} Metal + Accelerate",
            whisper_rs::WHISPER_CPP_VERSION
        )
    } else {
        format!(
            "whisper.cpp {} CPU + Accelerate",
            whisper_rs::WHISPER_CPP_VERSION
        )
    };
    let default_language =
        (model.languages.len() == 1 && model.languages[0] == "en").then(|| "en".to_owned());

    Ok(LoadedModel {
        transcriber: Box::new(WhisperTranscriber {
            context,
            default_language,
        }),
        execution: execution_name,
    })
}

struct WhisperTranscriber {
    context: WhisperContext,
    default_language: Option<String>,
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(
        &mut self,
        samples: &[f32],
        options: &TranscribeOptions,
    ) -> Result<String, TranscriptionError> {
        validate_pcm(samples)?;
        let mut state = self.context.create_state().map_err(whisper_error)?;
        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_language(
            options
                .language
                .as_deref()
                .or(self.default_language.as_deref()),
        );
        params.set_no_context(true);
        params.set_print_special(false);
        params.set_print_progress(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        let threads = std::thread::available_parallelism()
            .map(|count| count.get().min(8) as i32)
            .unwrap_or(4);
        params.set_n_threads(threads);

        state.full(params, samples).map_err(whisper_error)?;
        let text = state
            .as_iter()
            .map(|segment| segment.to_string())
            .collect::<Vec<_>>()
            .join("");
        Ok(text.trim().to_owned())
    }
}

fn whisper_error(error: impl std::fmt::Display) -> TranscriptionError {
    TranscriptionError::Backend {
        backend: "whisper",
        message: error.to_string(),
    }
}
