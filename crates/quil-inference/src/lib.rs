#![forbid(unsafe_code)]

mod moonshine;
mod moonshine_tokenizer;
mod whisper;

use std::path::{Path, PathBuf};

use quil_core::{Backend, ModelSpec, Transcriber};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Execution {
    Auto,
    Metal,
    CoreMl,
    Cpu,
}

#[derive(Debug, Error)]
pub enum InferenceError {
    #[error("model artifact {path} is invalid: {message}; run quil-model-fetch download/verify")]
    Artifact { path: PathBuf, message: String },
    #[error("{backend} backend is unavailable: {message}")]
    Unavailable {
        backend: &'static str,
        message: String,
    },
    #[error("failed to initialize {backend}: {message}")]
    Initialization {
        backend: &'static str,
        message: String,
    },
}

pub struct LoadedModel {
    pub transcriber: Box<dyn Transcriber>,
    pub execution: String,
}

pub fn load(
    model: &ModelSpec,
    model_dir: &Path,
    execution: Execution,
) -> Result<LoadedModel, InferenceError> {
    verify_artifacts(model, model_dir)?;
    match model.backend {
        Backend::Whisper => whisper::load(model, model_dir, execution),
        Backend::Moonshine => moonshine::load(model, model_dir, execution),
    }
}

fn verify_artifacts(model: &ModelSpec, model_dir: &Path) -> Result<(), InferenceError> {
    for artifact in &model.files {
        let path = model_dir.join(&artifact.path);
        let metadata = std::fs::metadata(&path).map_err(|error| InferenceError::Artifact {
            path: path.clone(),
            message: error.to_string(),
        })?;
        if metadata.len() != artifact.bytes {
            return Err(InferenceError::Artifact {
                path,
                message: format!(
                    "expected {} bytes, found {}",
                    artifact.bytes,
                    metadata.len()
                ),
            });
        }
    }
    Ok(())
}

pub(crate) fn initialization_error(
    backend: &'static str,
    error: impl std::fmt::Display,
) -> InferenceError {
    InferenceError::Initialization {
        backend,
        message: error.to_string(),
    }
}
