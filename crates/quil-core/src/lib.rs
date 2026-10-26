#![forbid(unsafe_code)]

mod audio;
mod catalog;
mod paths;
mod wer;

pub use audio::{AudioBuffer, SAMPLE_RATE};
pub use catalog::{Backend, Catalog, CatalogError, ModelFile, ModelSpec, embedded_catalog};
pub use paths::default_model_dir;
pub use wer::{WerStats, normalized_words, word_error_rate};

use thiserror::Error;

#[derive(Debug, Clone, Default)]
pub struct TranscribeOptions {
    /// BCP-47-ish language tag understood by the selected backend. `None` means
    /// the backend default (English for Moonshine, auto-detection for Whisper).
    pub language: Option<String>,
}

#[derive(Debug, Error)]
pub enum TranscriptionError {
    #[error("audio is too short to transcribe")]
    EmptyAudio,
    #[error("audio must contain only finite PCM samples")]
    NonFiniteAudio,
    #[error("{backend} inference failed: {message}")]
    Backend {
        backend: &'static str,
        message: String,
    },
}

/// A stateful local transcriber. Implementations may reuse loaded weights and
/// decoder state, but must reset utterance-specific state between calls.
pub trait Transcriber {
    fn transcribe(
        &mut self,
        samples: &[f32],
        options: &TranscribeOptions,
    ) -> Result<String, TranscriptionError>;
}

pub fn validate_pcm(samples: &[f32]) -> Result<(), TranscriptionError> {
    if samples.len() < SAMPLE_RATE as usize / 10 {
        return Err(TranscriptionError::EmptyAudio);
    }
    if !samples.iter().all(|sample| sample.is_finite()) {
        return Err(TranscriptionError::NonFiniteAudio);
    }
    Ok(())
}
