use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use rtrb::{Consumer, Producer, RingBuffer};
use zeroize::Zeroize;

use crate::AudioError;

/// A slot with drop-time wiping, including slots abandoned in the ring on an
/// error path. Volatile zeroization inside `zeroize` prevents dead-store
/// elimination; this is why the ring does not store bare `f32` values.
struct AudioSlot(f32);

impl Default for AudioSlot {
    fn default() -> Self {
        Self(0.0)
    }
}

impl Drop for AudioSlot {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

pub trait PcmSample: Copy {
    fn to_f32(self) -> f32;
}

impl PcmSample for f32 {
    fn to_f32(self) -> f32 {
        self
    }
}

impl PcmSample for f64 {
    fn to_f32(self) -> f32 {
        self as f32
    }
}

impl PcmSample for i8 {
    fn to_f32(self) -> f32 {
        f32::from(self) / 128.0
    }
}

impl PcmSample for u8 {
    fn to_f32(self) -> f32 {
        (f32::from(self) - 128.0) / 128.0
    }
}

impl PcmSample for i16 {
    fn to_f32(self) -> f32 {
        f32::from(self) / 32_768.0
    }
}

impl PcmSample for u16 {
    fn to_f32(self) -> f32 {
        (f32::from(self) - 32_768.0) / 32_768.0
    }
}

impl PcmSample for i32 {
    fn to_f32(self) -> f32 {
        (self as f64 / 2_147_483_648.0) as f32
    }
}

impl PcmSample for u32 {
    fn to_f32(self) -> f32 {
        ((f64::from(self) - 2_147_483_648.0) / 2_147_483_648.0) as f32
    }
}

/// Single-producer side used by the Core Audio callback. `write` performs only
/// arithmetic, atomic operations, and lock-free ring writes.
pub struct MonoPcmWriter {
    producer: Producer<AudioSlot>,
    channels: usize,
    overflowed: Arc<AtomicBool>,
}

impl MonoPcmWriter {
    pub fn write<T: PcmSample>(&mut self, interleaved: &[T]) {
        if self.overflowed.load(Ordering::Relaxed) {
            return;
        }
        let channel_count = self.channels;
        let scale = 1.0 / channel_count as f32;
        for frame in interleaved.chunks_exact(channel_count) {
            let mut sum = 0.0;
            for sample in frame {
                sum += sample.to_f32();
            }
            if self.producer.push(AudioSlot(sum * scale)).is_err() {
                self.overflowed.store(true, Ordering::Release);
                return;
            }
        }
    }

    pub fn overflowed(&self) -> bool {
        self.overflowed.load(Ordering::Acquire)
    }

    pub(crate) fn overflowed_handle(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.overflowed)
    }
}

pub struct MonoPcmReader {
    consumer: Consumer<AudioSlot>,
    overflowed: Arc<AtomicBool>,
}

impl MonoPcmReader {
    pub fn pop(&mut self) -> Option<f32> {
        let slot = self.consumer.pop().ok()?;
        let sample = slot.0;
        // The popped slot is dropped here, wiping its ring-local copy. The
        // returned `f32` belongs to the consumer-owned output buffer.
        Some(sample)
    }

    pub fn pending(&self) -> usize {
        self.consumer.slots()
    }

    pub fn overflowed(&self) -> bool {
        self.overflowed.load(Ordering::Acquire)
    }
}

pub fn mono_ring(
    capacity: usize,
    channels: usize,
) -> Result<(MonoPcmWriter, MonoPcmReader), AudioError> {
    if capacity == 0 || channels == 0 {
        return Err(AudioError::Invalid(
            "ring capacity and channel count must be positive".into(),
        ));
    }
    let (producer, consumer) = RingBuffer::new(capacity);
    let overflowed = Arc::new(AtomicBool::new(false));
    Ok((
        MonoPcmWriter {
            producer,
            channels,
            overflowed: Arc::clone(&overflowed),
        },
        MonoPcmReader {
            consumer,
            overflowed,
        },
    ))
}
