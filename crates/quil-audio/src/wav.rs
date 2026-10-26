use std::{fs, path::Path};

use quil_core::AudioBuffer;
use zeroize::Zeroizing;

use crate::{AudioError, resample_to_16k};

/// Supported input rates for file transcription: every common speech and
/// music rate (8 kHz telephony through 384 kHz high-resolution) without
/// letting a header pick a pathological ratio.
const MIN_INPUT_RATE: u32 = 8_000;
const MAX_INPUT_RATE: u32 = 384_000;

pub fn read_wav(path: &Path) -> Result<AudioBuffer, AudioError> {
    let mut reader = hound::WavReader::open(path)?;
    let spec = reader.spec();
    let channels = usize::from(spec.channels);
    if channels == 0 {
        return Err(AudioError::Invalid("WAV needs at least one channel".into()));
    }
    // Rates below 8 kHz are rejected before any allocation. The 16 kHz
    // resampler's output grows inversely with the claimed input rate, so a
    // tiny file claiming 1 Hz would otherwise demand ~16,000x its size in
    // resident memory before the duration caps further down the pipeline
    // ever run.
    if !(MIN_INPUT_RATE..=MAX_INPUT_RATE).contains(&spec.sample_rate) {
        return Err(AudioError::Invalid(format!(
            "WAV sample rate {} Hz is outside the supported {}..={} Hz range",
            spec.sample_rate, MIN_INPUT_RATE, MAX_INPUT_RATE
        )));
    }

    // Pre-size the buffer so Vec growth never reallocates away an unwiped
    // audio copy: `Zeroizing` wipes only the final allocation, so the old
    // allocation freed by growth would remain on the heap. Both the declared
    // sample count and the on-disk byte length are untrusted file input, so
    // take the smaller bound; every supported format stores at least 2 bytes
    // per sample, which keeps resident memory within a small multiple of the
    // real file size even when a header lies about its chunk.
    let file_bytes = fs::metadata(path)
        .map_err(|error| AudioError::Input(error.to_string()))?
        .len();
    let disk_bound_samples = u32::try_from(file_bytes / 2).unwrap_or(u32::MAX);
    let declared_samples = reader.len();
    let mut interleaved = Zeroizing::new(Vec::with_capacity(
        declared_samples.min(disk_bound_samples) as usize,
    ));
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn temp_wav(name: &str, sample_rate: u32, samples: &[i16]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("quil-wav-test-{name}"));
        let spec = hound::WavSpec {
            channels: 1,
            sample_rate,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&path, spec).expect("create test wav");
        for sample in samples {
            writer.write_sample(*sample).expect("write test sample");
        }
        writer.finalize().expect("finalize test wav");
        path
    }

    #[test]
    fn reads_a_mono_16k_wav_without_resampling() {
        let path = temp_wav("identity", 16_000, &[8_192, -8_192, 16_384]);
        let audio = read_wav(&path).expect("16 kHz mono wav must be readable");
        assert_eq!(audio.sample_rate(), 16_000);
        assert_eq!(audio.samples().len(), 3);
        assert!((audio.samples()[0] - 8_192.0 / 32_768.0).abs() < 1e-6);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn downsamples_a_48k_wav_on_read() {
        let samples: Vec<i16> = (0..256).map(|index| index as i16 * 32).collect();
        let path = temp_wav("downsample", 48_000, &samples);
        let audio = read_wav(&path).expect("48 kHz wav must be readable");
        assert_eq!(audio.sample_rate(), 16_000);
        assert!(!audio.samples().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_a_rate_below_the_supported_floor() {
        // A tiny file claiming 1 Hz would otherwise drive a ~16,000x
        // resampler allocation; the floor check must fire before any read.
        let path = temp_wav("low-rate", 1, &[0; 16]);
        let error = read_wav(&path).expect_err("sub-8 kHz rate must be rejected");
        assert!(format!("{error}").contains("sample rate"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn rejects_a_rate_above_the_supported_ceiling() {
        let path = temp_wav("high-rate", 1_000_000, &[0; 16]);
        let error = read_wav(&path).expect_err("absurd rate must be rejected");
        assert!(format!("{error}").contains("sample rate"));
        let _ = std::fs::remove_file(&path);
    }
}
