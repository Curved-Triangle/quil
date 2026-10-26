#![forbid(unsafe_code)]

mod capture;
mod pcm;
mod ring;
mod wav;

pub use capture::{capture_microphone, input_device_names};
pub use pcm::resample_to_16k;
pub use ring::{MonoPcmReader, MonoPcmWriter, PcmSample, mono_ring};
pub use wav::read_wav;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum AudioError {
    #[error("audio input error: {0}")]
    Input(String),
    #[error("audio input disconnected or unavailable")]
    Stream,
    #[error("audio ring buffer overflowed; capture was stopped rather than dropping speech")]
    Overflow,
    #[error("invalid audio: {0}")]
    Invalid(String),
    #[error("failed to read WAV audio: {0}")]
    Wav(#[from] hound::Error),
    #[error("failed to resample audio: {0}")]
    Resample(String),
    #[error("audio consumer thread failed")]
    Consumer,
}
