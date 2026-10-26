use std::path::Path;

use quil_core::AudioBuffer;
use zeroize::Zeroizing;

use crate::{AudioError, resample_to_16k};

pub fn read_wav(path: &Path) -> Result<AudioBuffer, AudioError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    if channels == 0 || spec.sample_rate == 0 {
        return Err(AudioError::Invalid(
            "WAV needs at least one channel and a positive sample rate".into(),
        ));
    }

    let mut interleaved = Zeroizing::new(Vec::new());
    match spec.sample_format {
        hound::SampleFormat::Float if spec.bits_per_sample == 32 => {
            for sample in reader.samples::<f32>() {
                interleaved.push(sample?);
            }
        }
        hound::SampleFormat::Int if spec.bits_per_sample == 16 => {
            for sample in reader.samples::<i16>() {
                interleaved.push(f32::from(sample?) / 32_768.0);
            }
        }
        hound::SampleFormat::Int if spec.bits_per_sample == 24 || spec.bits_per_sample == 32 => {
            let scale = 2_f64.powi(i32::from(spec.bits_per_sample) - 1);
            for sample in reader.samples::<i32>() {
                interleaved.push((f64::from(sample?) / scale) as f32);
            }
        }
        _ => {
            return Err(AudioError::Invalid(format!(
                "unsupported WAV format: {:?}, {} bits",
                spec.sample_format, spec.bits_per_sample
            )));
        }
    }

    if interleaved.is_empty() || interleaved.len() % channels != 0 {
        return Err(AudioError::Invalid(
            "WAV contains an empty or incomplete interleaved frame".into(),
        ));
    }
    let mut mono = Vec::with_capacity(interleaved.len() / channels);
    let scale = 1.0 / channels as f32;
    for frame in interleaved.chunks_exact(channels) {
        mono.push(frame.iter().sum::<f32>() * scale);
    }
    resample_to_16k(AudioBuffer::new(spec.sample_rate, mono))
}
