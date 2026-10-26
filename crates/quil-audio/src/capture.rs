use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

use cpal::{
    SampleFormat, SizedSample, StreamConfig,
    traits::{DeviceTrait, HostTrait, StreamTrait},
};
use quil_core::AudioBuffer;

use crate::{AudioError, MonoPcmWriter, PcmSample, mono_ring, resample_to_16k};

pub fn input_device_names() -> Result<Vec<String>, AudioError> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|error| AudioError::Input(error.to_string()))?;
    Ok(devices.map(|device| device.to_string()).collect())
}

/// A running bounded microphone stream. The Core Audio callback only writes to
/// the lock-free ring; the owner drains it on any non-real-time thread.
pub struct MicrophoneCapture {
    stream: cpal::Stream,
    reader: crate::MonoPcmReader,
    overflowed: Arc<AtomicBool>,
    stream_failed: Arc<AtomicBool>,
    input_rate: u32,
    read_buffer: AudioBuffer,
    max_chunk_samples: usize,
}

impl MicrophoneCapture {
    pub fn start(requested_device: Option<&str>, buffer_seconds: u32) -> Result<Self, AudioError> {
        if buffer_seconds == 0 || buffer_seconds > 300 {
            return Err(AudioError::Invalid(
                "capture ring duration must be between 1 and 300 seconds".into(),
            ));
        }
        let host = cpal::default_host();
        let device = match requested_device {
            Some(name) => host
                .input_devices()
                .map_err(|error| AudioError::Input(error.to_string()))?
                .find(|device| device.to_string() == name)
                .ok_or_else(|| AudioError::Input(format!("microphone `{name}` was not found")))?,
            None => host.default_input_device().ok_or_else(|| {
                AudioError::Input("no default microphone; check macOS microphone permission".into())
            })?,
        };
        let supported = device
            .default_input_config()
            .map_err(|error| AudioError::Input(error.to_string()))?;
        let sample_format = supported.sample_format();
        let config: StreamConfig = supported.into();
        let input_rate = config.sample_rate;
        let channels = usize::from(config.channels);
        if input_rate == 0 || channels == 0 {
            return Err(AudioError::Invalid(
                "microphone reported an empty stream configuration".into(),
            ));
        }

        let ring_capacity = (input_rate as usize).saturating_mul(buffer_seconds as usize);
        let (writer, reader) = mono_ring(ring_capacity, channels)?;
        let overflowed = writer.overflowed_handle();
        let stream_failed = Arc::new(AtomicBool::new(false));
        let stream = build_stream(
            &device,
            &config,
            sample_format,
            writer,
            Arc::clone(&stream_failed),
        )?;
        stream
            .play()
            .map_err(|error| AudioError::Input(error.to_string()))?;
        Ok(Self {
            stream,
            reader,
            overflowed,
            stream_failed,
            input_rate,
            read_buffer: AudioBuffer::with_capacity(input_rate, ring_capacity),
            max_chunk_samples: ring_capacity,
        })
    }

    pub fn input_rate(&self) -> u32 {
        self.input_rate
    }

    pub fn check_status(&self) -> Result<(), AudioError> {
        if self.overflowed.load(Ordering::Acquire) {
            return Err(AudioError::Overflow);
        }
        if self.stream_failed.load(Ordering::Acquire) {
            return Err(AudioError::Stream);
        }
        Ok(())
    }

    /// Drain currently available native-rate samples into a reusable,
    /// zero-on-drop buffer. The hard preallocated bound prevents reallocation
    /// from ever freeing an unwiped audio copy.
    pub fn read_available(&mut self) -> Result<&mut AudioBuffer, AudioError> {
        self.read_buffer.truncate(0);
        while let Some(sample) = self.reader.pop() {
            self.read_buffer
                .try_push(sample, self.max_chunk_samples)
                .map_err(|_| AudioError::Overflow)?;
        }
        self.check_status()?;
        Ok(&mut self.read_buffer)
    }

    /// Stop the stream and drain the final samples. The stream is also paused
    /// on drop, so error and Ctrl-C paths cannot leave capture running.
    pub fn stop(&mut self) -> Result<&mut AudioBuffer, AudioError> {
        let pause_result = self
            .stream
            .pause()
            .map_err(|error| AudioError::Input(error.to_string()));
        let remaining = self.read_available();
        pause_result?;
        remaining
    }
}

impl Drop for MicrophoneCapture {
    fn drop(&mut self) {
        let _ = self.stream.pause();
    }
}

/// Capture for a fixed duration directly into a bounded ring and an owned
/// in-memory buffer. No file or logging operation exists on this path.
pub fn capture_microphone(
    duration: Duration,
    requested_device: Option<&str>,
) -> Result<AudioBuffer, AudioError> {
    if duration.is_zero() || duration.as_secs() > 300 {
        return Err(AudioError::Invalid(
            "capture duration must be greater than zero and at most 300 seconds".into(),
        ));
    }
    let buffer_seconds = duration.as_secs().saturating_add(2).max(1) as u32;
    let mut capture = MicrophoneCapture::start(requested_device, buffer_seconds)?;
    let input_rate = capture.input_rate();
    let expected_samples = (duration.as_secs_f64() * f64::from(input_rate)).ceil() as usize;
    let max_samples = expected_samples.saturating_add(input_rate as usize);
    let mut audio = AudioBuffer::with_capacity(input_rate, max_samples);
    let started = Instant::now();
    let mut capture_error = None;
    while started.elapsed() < duration {
        match capture.read_available() {
            Ok(chunk) => audio
                .try_extend_from_slice(chunk.samples(), max_samples)
                .map_err(|_| AudioError::Overflow)?,
            Err(error) => {
                capture_error = Some(error);
                break;
            }
        }
        if capture.overflowed.load(Ordering::Acquire)
            || capture.stream_failed.load(Ordering::Acquire)
        {
            break;
        }
        thread::sleep(Duration::from_millis(5));
    }
    let remaining = capture.stop()?;
    audio
        .try_extend_from_slice(remaining.samples(), max_samples)
        .map_err(|_| AudioError::Overflow)?;
    if let Some(error) = capture_error {
        return Err(error);
    }
    resample_to_16k(audio)
}

fn build_stream(
    device: &cpal::Device,
    config: &StreamConfig,
    format: SampleFormat,
    writer: MonoPcmWriter,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, AudioError> {
    macro_rules! typed_stream {
        ($sample:ty) => {
            build_typed_stream::<$sample>(device, config, writer, failed)
        };
    }
    match format {
        SampleFormat::F32 => typed_stream!(f32),
        SampleFormat::F64 => typed_stream!(f64),
        SampleFormat::I8 => typed_stream!(i8),
        SampleFormat::I16 => typed_stream!(i16),
        SampleFormat::I32 => typed_stream!(i32),
        SampleFormat::U8 => typed_stream!(u8),
        SampleFormat::U16 => typed_stream!(u16),
        SampleFormat::U32 => typed_stream!(u32),
        other => Err(AudioError::Input(format!(
            "unsupported microphone sample format {other:?}"
        ))),
    }
}

fn build_typed_stream<T>(
    device: &cpal::Device,
    config: &StreamConfig,
    mut writer: MonoPcmWriter,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, AudioError>
where
    T: SizedSample + PcmSample,
{
    let error_flag = Arc::clone(&failed);
    device
        .build_input_stream::<T, _, _>(
            *config,
            move |samples, _| writer.write(samples),
            move |_| {
                // No formatting or logging here: the error callback only uses
                // an atomic and the main thread reports a sanitized error.
                error_flag.store(true, Ordering::Release);
            },
            None,
        )
        .map_err(|error| AudioError::Input(error.to_string()))
}
