use quil_core::{AudioBuffer, SAMPLE_RATE};
use rubato::{Fft, FixedSync, Resampler, audioadapter_buffers::direct::InterleavedSlice};

use crate::AudioError;

/// Convert an owned clip to the ASR input rate. All construction and allocation
/// happens on the caller/worker thread, never in the capture callback.
pub fn resample_to_16k(audio: AudioBuffer) -> Result<AudioBuffer, AudioError> {
    let input_rate = audio.sample_rate();
    if input_rate == SAMPLE_RATE {
        return Ok(audio);
    }

    let input_len = audio.samples().len();
    if input_len == 0 {
        return Ok(AudioBuffer::new(SAMPLE_RATE, Vec::new()));
    }
    let mut resampler = Fft::<f32>::new(
        input_rate as usize,
        SAMPLE_RATE as usize,
        1024,
        1,
        FixedSync::Both,
    )
    .map_err(|error| AudioError::Resample(error.to_string()))?;
    let output_capacity = resampler.process_all_needed_output_len(input_len);
    let mut output = AudioBuffer::new(SAMPLE_RATE, vec![0.0_f32; output_capacity]);

    let input_adapter = InterleavedSlice::new(audio.samples(), 1, input_len)
        .map_err(|error| AudioError::Resample(error.to_string()))?;
    let mut output_adapter = InterleavedSlice::new_mut(output.samples_mut(), 1, output_capacity)
        .map_err(|error| AudioError::Resample(error.to_string()))?;
    let (_, produced) = resampler
        .process_all_into_buffer(&input_adapter, &mut output_adapter, input_len, None)
        .map_err(|error| AudioError::Resample(error.to_string()))?;
    output.truncate(produced);
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_identity_rate() {
        let audio = AudioBuffer::new(SAMPLE_RATE, vec![0.1, 0.2]);
        let converted = resample_to_16k(audio).expect("identity conversion");
        assert_eq!(converted.samples(), [0.1, 0.2]);
    }

    #[test]
    fn downsamples_to_expected_duration() {
        let audio = AudioBuffer::new(48_000, vec![0.0; 48_000]);
        let converted = resample_to_16k(audio).expect("downsample");
        assert_eq!(converted.sample_rate(), SAMPLE_RATE);
        assert!((converted.duration_seconds() - 1.0).abs() < 0.002);
        assert!(converted.samples().iter().all(|sample| sample.is_finite()));
    }
}
