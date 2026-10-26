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

    let expected_samples = (duration.as_secs_f64() * f64::from(input_rate)).ceil() as usize;
    let ring_capacity = expected_samples
        .saturating_add(input_rate as usize)
        .max(input_rate as usize);
    let (writer, mut reader) = mono_ring(ring_capacity, channels)?;
    let overflowed = writer.overflowed_handle();
    let stream_failed = Arc::new(AtomicBool::new(false));
    let stop = Arc::new(AtomicBool::new(false));

    // Build before starting the consumer so a construction failure cannot
    // strand a thread. The stream remains stopped until the consumer is ready.
    let stream = build_stream(
        &device,
        &config,
        sample_format,
        writer,
        Arc::clone(&stream_failed),
    )?;

    let consumer_stop = Arc::clone(&stop);
    let consumer = thread::Builder::new()
        .name("quil-pcm-consumer".into())
        .spawn(move || {
            let mut samples = Vec::with_capacity(expected_samples);
            while !consumer_stop.load(Ordering::Acquire) || reader.pending() > 0 {
                match reader.pop() {
                    Some(sample) => samples.push(sample),
                    None => thread::park_timeout(Duration::from_millis(2)),
                }
            }
            (AudioBuffer::new(input_rate, samples), reader.overflowed())
        })
        .map_err(|error| AudioError::Input(error.to_string()))?;

    let stream_result = (|| -> Result<(), AudioError> {
        stream
            .play()
            .map_err(|error| AudioError::Input(error.to_string()))?;
        let started = Instant::now();
        while started.elapsed() < duration
            && !overflowed.load(Ordering::Acquire)
            && !stream_failed.load(Ordering::Acquire)
        {
            thread::sleep(Duration::from_millis(5));
        }
        stream
            .pause()
            .map_err(|error| AudioError::Input(error.to_string()))?;
        Ok(())
    })();

    drop(stream);
    stop.store(true, Ordering::Release);
    let (audio, ring_overflowed) = consumer.join().map_err(|_| AudioError::Consumer)?;
    stream_result?;
    if overflowed.load(Ordering::Acquire) || ring_overflowed {
        return Err(AudioError::Overflow);
    }
    if stream_failed.load(Ordering::Acquire) {
        return Err(AudioError::Stream);
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
