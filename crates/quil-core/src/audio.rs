use std::fmt;

use thiserror::Error;
use zeroize::{Zeroize, Zeroizing};

pub const SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, Error)]
#[error("audio buffer capacity of {limit} samples would be exceeded")]
pub struct AudioCapacityError {
    pub limit: usize,
}

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

    pub fn with_capacity(sample_rate: u32, capacity: usize) -> Self {
        Self::new(sample_rate, Vec::with_capacity(capacity))
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

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn try_push(&mut self, sample: f32, limit: usize) -> Result<(), AudioCapacityError> {
        if self.samples.len() >= limit || self.samples.len() >= self.samples.capacity() {
            return Err(AudioCapacityError { limit });
        }
        self.samples.push(sample);
        Ok(())
    }

    pub fn try_extend_from_slice(
        &mut self,
        samples: &[f32],
        limit: usize,
    ) -> Result<(), AudioCapacityError> {
        let new_len = self
            .samples
            .len()
            .checked_add(samples.len())
            .ok_or(AudioCapacityError { limit })?;
        if new_len > limit || new_len > self.samples.capacity() {
            return Err(AudioCapacityError { limit });
        }
        self.samples.extend_from_slice(samples);
        Ok(())
    }

    pub fn truncate(&mut self, length: usize) {
        if length < self.samples.len() {
            self.samples[length..].zeroize();
            self.samples.truncate(length);
        }
    }

    pub fn drop_prefix(&mut self, count: usize) {
        let old_len = self.samples.len();
        let count = count.min(old_len);
        self.samples[..count].zeroize();
        self.samples.copy_within(count..old_len, 0);
        // The move leaves duplicate PCM in the now-logically-removed tail.
        // Wipe it before shortening the vector so no stale samples remain.
        let new_len = old_len - count;
        self.samples[new_len..old_len].zeroize();
        self.samples.truncate(new_len);
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
    fn drop_prefix_preserves_only_the_live_suffix() {
        let mut audio = AudioBuffer::new(SAMPLE_RATE, vec![1.0, 2.0, 3.0, 4.0]);
        audio.drop_prefix(2);
        assert_eq!(audio.samples(), [3.0, 4.0]);
        audio.drop_prefix(99);
        assert!(audio.is_empty());
    }

    #[test]
    fn duration_uses_the_original_sample_rate() {
        let audio = AudioBuffer::new(48_000, vec![0.0; 24_000]);
        assert_eq!(audio.duration_seconds(), 0.5);
    }
}
