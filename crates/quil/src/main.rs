mod benchmark;
mod system;

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use clap::{Args, Parser, Subcommand, ValueEnum};
use quil_audio::{capture_microphone, input_device_names, read_wav};
use quil_core::{
    Catalog, CatalogError, ModelSpec, TranscribeOptions, TranscriptionError, default_model_dir,
    embedded_catalog,
};
use quil_inference::{Execution, InferenceError, LoadedModel};
use thiserror::Error;

#[derive(Debug, Parser)]
#[command(
    name = "quil",
    version,
    about = "Fully local speech transcription for macOS"
)]
struct Cli {
    #[command(flatten)]
    runtime: RuntimeOptions,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Args)]
struct RuntimeOptions {
    /// Override Quil's per-user model directory.
    #[arg(long, global = true, value_name = "PATH")]
    model_dir: Option<PathBuf>,
    /// Inference execution provider.
    #[arg(long, global = true, value_enum, default_value_t = ExecutionArg::Auto)]
    execution: ExecutionArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExecutionArg {
    Auto,
    Metal,
    Coreml,
    Cpu,
}

impl From<ExecutionArg> for Execution {
    fn from(value: ExecutionArg) -> Self {
        match value {
            ExecutionArg::Auto => Self::Auto,
            ExecutionArg::Metal => Self::Metal,
            ExecutionArg::Coreml => Self::CoreMl,
            ExecutionArg::Cpu => Self::Cpu,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List catalog entries without network access.
    Models,
    /// List available input devices.
    Mics,
    /// Record microphone audio to memory and transcribe it.
    Record {
        #[arg(long)]
        model: Option<String>,
        #[arg(long, default_value_t = 5)]
        seconds: u64,
        #[arg(long)]
        mic: Option<String>,
        #[arg(long)]
        language: Option<String>,
    },
    /// Transcribe an existing local WAV file.
    Transcribe {
        #[arg(long)]
        model: Option<String>,
        #[arg(long, value_name = "WAV")]
        input: PathBuf,
        #[arg(long)]
        language: Option<String>,
    },
    /// Benchmark one model against a local JSON corpus manifest.
    Bench {
        #[arg(long)]
        model: String,
        #[arg(long, value_name = "JSON")]
        corpus: PathBuf,
        #[arg(long, default_value_t = 3)]
        runs: usize,
        #[arg(long, value_name = "JSON")]
        output: Option<PathBuf>,
        /// Optional maximum acceptable warm real-time factor.
        #[arg(long)]
        max_rtf: Option<f64>,
        /// Optional maximum acceptable WER (0.05 means 5%).
        #[arg(long)]
        max_wer: Option<f64>,
        /// Optional maximum acceptable model load time.
        #[arg(long)]
        max_load_ms: Option<f64>,
    },
}

#[derive(Debug, Error)]
enum CliError {
    #[error(transparent)]
    Catalog(#[from] CatalogError),
    #[error(transparent)]
    Audio(#[from] quil_audio::AudioError),
    #[error(transparent)]
    Inference(#[from] InferenceError),
    #[error(transparent)]
    Transcription(#[from] TranscriptionError),
    #[error("I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON in {path}: {source}")]
    Json {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("{0}")]
    Invalid(String),
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), CliError> {
    let cli = Cli::parse();
    let catalog = embedded_catalog()?;
    let model_dir = match cli.runtime.model_dir {
        Some(path) => path,
        None => default_model_dir()
            .ok_or_else(|| CliError::Invalid("could not determine model directory".into()))?,
    };

    match cli.command {
        Command::Models => {
            for model in &catalog.models {
                println!(
                    "{}\t{}\t{:?}\t{}",
                    model.id, model.name, model.backend, model.license
                );
            }
        }
        Command::Mics => {
            for name in input_device_names()? {
                println!("{name}");
            }
        }
        Command::Record {
            model,
            seconds,
            mic,
            language,
        } => {
            let model = select_model(&catalog, model)?;
            let LoadedModel {
                mut transcriber,
                execution: _,
            } = load(model, &model_dir, cli.runtime.execution.into())?;
            eprintln!("Recording {seconds}s from the selected microphone; audio stays in memory…");
            let mut audio = capture_microphone(Duration::from_secs(seconds), mic.as_deref())?;
            let result = transcriber.transcribe(
                audio.samples(),
                &TranscribeOptions {
                    language: language.or_else(|| Some("en".into())),
                },
            );
            audio.clear();
            println!("{}", result?);
        }
        Command::Transcribe {
            model,
            input,
            language,
        } => {
            let model = select_model(&catalog, model)?;
            let LoadedModel {
                mut transcriber,
                execution: _,
            } = load(model, &model_dir, cli.runtime.execution.into())?;
            let mut audio = read_wav(&input)?;
            let result = transcriber.transcribe(audio.samples(), &TranscribeOptions { language });
            audio.clear();
            println!("{}", result?);
        }
        Command::Bench {
            model,
            corpus,
            runs,
            output,
            max_rtf,
            max_wer,
            max_load_ms,
        } => {
            let model = select_model(&catalog, Some(model))?;
            let report = benchmark::run(
                model,
                &model_dir,
                cli.runtime.execution.into(),
                &corpus,
                runs,
            )?;
            if let Some(max_rtf) = max_rtf
                && report.real_time_factor > max_rtf
            {
                return Err(CliError::Invalid(format!(
                    "RTF {:.4} exceeds regression budget {max_rtf:.4}",
                    report.real_time_factor
                )));
            }
            if let Some(max_wer) = max_wer
                && report.word_error_rate > max_wer
            {
                return Err(CliError::Invalid(format!(
                    "WER {:.4} exceeds regression budget {max_wer:.4}",
                    report.word_error_rate
                )));
            }
            if let Some(max_load_ms) = max_load_ms
                && report.load_ms > max_load_ms
            {
                return Err(CliError::Invalid(format!(
                    "load time {:.1} ms exceeds regression budget {max_load_ms:.1} ms",
                    report.load_ms
                )));
            }
            let json = serde_json::to_string_pretty(&report).map_err(|source| CliError::Json {
                path: output.clone().unwrap_or_else(|| PathBuf::from("<stdout>")),
                source,
            })?;
            if let Some(path) = output {
                std::fs::write(&path, format!("{json}\n"))
                    .map_err(|source| CliError::Io { path, source })?;
            } else {
                println!("{json}");
            }
        }
    }
    Ok(())
}

fn select_model(catalog: &Catalog, model: Option<String>) -> Result<&ModelSpec, CliError> {
    match model {
        Some(id) => Ok(catalog.model(&id)?),
        None => catalog.default_model().ok_or_else(|| {
            CliError::Invalid("no default model is configured; pass --model".into())
        }),
    }
}

fn load(
    model: &ModelSpec,
    model_dir: &Path,
    execution: Execution,
) -> Result<LoadedModel, InferenceError> {
    quil_inference::load(model, &model_dir.join(&model.id), execution)
}
