use std::fmt;

use zeroize::{Zeroize, Zeroizing};

pub const SAMPLE_RATE: u32 = 16_000;

/// Owned mono PCM. `Zeroizing` wipes the allocation when this value is dropped;
/// `clear` also lets a long-running owner wipe samples before deallocation.
pub struct AudioBuffer {
    sample_rate: u32,
    samples: Zeroizing<Vec<f32>>,
}

impl fmt::Debug for AudioBuffer {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never expose PCM through a routine debug/log formatting call.
        formatter
            .debug_struct("AudioBuffer")
            .field("sample_rate", &self.sample_rate)
            .field("sample_count", &self.samples.len())
            .finish()
    }
}

impl AudioBuffer {
    pub fn new(sample_rate: u32, samples: Vec<f32>) -> Self {
        assert!(sample_rate > 0, "PCM sample rate must be positive");
        Self {
            sample_rate,
            samples: Zeroizing::new(samples),
        }
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }

    pub fn samples_mut(&mut self) -> &mut [f32] {
        &mut self.samples
    }

    pub fn truncate(&mut self, length: usize) {
        self.samples.truncate(length);
    }

    pub fn duration_seconds(&self) -> f64 {
        self.samples.len() as f64 / f64::from(self.sample_rate)
    }

    pub fn clear(&mut self) {
        self.samples.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clear_overwrites_samples() {
        let mut audio = AudioBuffer::new(SAMPLE_RATE, vec![0.25, -0.5, 1.0]);
        audio.clear();
        assert!(audio.samples().iter().all(|sample| *sample == 0.0));
    }

    #[test]
    fn debug_output_never_contains_pcm() {
        let audio = AudioBuffer::new(SAMPLE_RATE, vec![0.25, -0.5]);
        let debug = format!("{audio:?}");
        assert!(debug.contains("sample_count: 2"));
        assert!(!debug.contains("0.25"));
        assert!(!debug.contains("-0.5"));
    }

    #[test]
    fn duration_uses_the_original_sample_rate() {
        let audio = AudioBuffer::new(48_000, vec![0.0; 24_000]);
        assert_eq!(audio.duration_seconds(), 0.5);
    }
}
