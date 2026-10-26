use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

use quil_audio::{AudioError, MicrophoneCapture, resample_to_16k};
use quil_core::{AudioBuffer, ModelSpec, TranscribeOptions, TranscriptionError};
use quil_inference::{Execution, InferenceError, LoadedModel};
use rdev::{Event, EventType, Key};
use thiserror::Error;

use crate::{
    insert::{self, InsertError, TextInserter},
    streaming::{PartialCommitter, has_speech},
};

const TICK: Duration = Duration::from_millis(250);
const MAX_WINDOW_SECONDS: usize = 30;

#[derive(Debug)]
pub struct Options {
    pub mic: Option<String>,
    pub hotkey: String,
    pub mode: Mode,
    pub warm_seconds: u64,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Hold,
    Toggle,
}

#[derive(Debug, Error)]
pub enum DictationError {
    #[error(transparent)]
    Audio(#[from] AudioError),
    #[error(transparent)]
    Inference(#[from] InferenceError),
    #[error(transparent)]
    Transcription(#[from] TranscriptionError),
    #[error(transparent)]
    Insert(#[from] InsertError),
    #[error("global hotkey error: {0}")]
    Hotkey(String),
    #[error("dictation worker failed: {0}")]
    Worker(String),
}

enum Command {
    Start,
    Stop,
    Toggle,
    Shutdown,
}

pub fn insert_test(text: &str, delay_ms: u64) -> Result<(), DictationError> {
    insert::require_accessibility()?;
    if delay_ms > 0 {
        eprintln!("Inserting test text in {delay_ms} ms; focus the target field now…");
        thread::sleep(Duration::from_millis(delay_ms));
    }
    TextInserter::new()?.insert(text)?;
    Ok(())
}

pub fn run(
    model: &ModelSpec,
    model_dir: &Path,
    execution: Execution,
    options: Options,
) -> Result<(), DictationError> {
    insert::require_accessibility()?;
    let hotkey = parse_hotkey(&options.hotkey)?;
    let hotkey_label = options.hotkey.clone();
    let mode = options.mode;
    let (sender, receiver) = sync_channel::<Command>(16);
    let (startup_sender, startup_receiver) = sync_channel(1);
    let model = model.clone();
    let model_dir = model_dir.to_path_buf();
    let worker = thread::Builder::new()
        .name("quil-dictation".into())
        .spawn(move || {
            let mut worker = match Worker::new(model, model_dir, execution, options) {
                Ok(worker) => {
                    let _ = startup_sender.send(Ok(()));
                    worker
                }
                Err(error) => {
                    let _ = startup_sender.send(Err(error));
                    return;
                }
            };
            worker.run(receiver);
        })
        .map_err(|error| DictationError::Worker(error.to_string()))?;

    match startup_receiver.recv() {
        Ok(Ok(())) => {}
        Ok(Err(error)) => return Err(error),
        Err(error) => return Err(DictationError::Worker(error.to_string())),
    }

    eprintln!(
        "Quil is listening for {hotkey_label} in {:?} mode. Press Ctrl-C to exit. No dictated text is logged.",
        match mode {
            Mode::Hold => "push-to-talk",
            Mode::Toggle => "toggle",
        }
    );
    let listener_sender = sender.clone();
    let hotkey_down = Arc::new(AtomicBool::new(false));
    let listener_result = rdev::listen(move |event| {
        dispatch_hotkey(&listener_sender, &hotkey_down, event, hotkey, mode);
    });
    let _ = sender.send(Command::Shutdown);
    let _ = worker.join();
    listener_result.map_err(|error| DictationError::Hotkey(format!("{error:?}")))
}

fn dispatch_hotkey(
    sender: &SyncSender<Command>,
    hotkey_down: &AtomicBool,
    event: Event,
    hotkey: Key,
    mode: Mode,
) {
    match event.event_type {
        EventType::KeyPress(key) if key == hotkey => {
            if hotkey_down.swap(true, Ordering::AcqRel) {
                return;
            }
            let command = match mode {
                Mode::Hold => Command::Start,
                Mode::Toggle => Command::Toggle,
            };
            let _ = sender.try_send(command);
        }
        EventType::KeyRelease(key) if key == hotkey => {
            hotkey_down.store(false, Ordering::Release);
            if mode == Mode::Hold {
                let _ = sender.try_send(Command::Stop);
            }
        }
        _ => {}
    }
}

fn parse_hotkey(value: &str) -> Result<Key, DictationError> {
    match value.to_ascii_uppercase().as_str() {
        "F1" => Ok(Key::F1),
        "F2" => Ok(Key::F2),
        "F3" => Ok(Key::F3),
        "F4" => Ok(Key::F4),
        "F5" => Ok(Key::F5),
        "F6" => Ok(Key::F6),
        "F7" => Ok(Key::F7),
        "F8" => Ok(Key::F8),
        "F9" => Ok(Key::F9),
        "F10" => Ok(Key::F10),
        "F11" => Ok(Key::F11),
        "F12" => Ok(Key::F12),
        _ => Err(DictationError::Hotkey(format!(
            "unsupported hotkey `{value}`; choose F1-F12 so shortcuts never insert printable characters"
        ))),
    }
}

struct Worker {
    model_spec: ModelSpec,
    model_dir: PathBuf,
    execution: Execution,
    options: Options,
    model: Option<LoadedModel>,
    inserter: TextInserter,
    capture: Option<MicrophoneCapture>,
    native_audio: Option<AudioBuffer>,
    committer: PartialCommitter,
    active: bool,
    speech_seen: bool,
    silence_ticks: u8,
    last_inference: Option<Instant>,
    last_used: Instant,
}

impl Worker {
    fn new(
        model_spec: ModelSpec,
        model_dir: PathBuf,
        execution: Execution,
        options: Options,
    ) -> Result<Self, DictationError> {
        Ok(Self {
            model_spec,
            model_dir,
            execution,
            options,
            model: None,
            inserter: TextInserter::new()?,
            capture: None,
            native_audio: None,
            committer: PartialCommitter::default(),
            active: false,
            speech_seen: false,
            silence_ticks: 0,
            last_inference: None,
            last_used: Instant::now(),
        })
    }

    fn run(&mut self, receiver: Receiver<Command>) {
        loop {
            let timeout = if self.active {
                Duration::from_millis(25)
            } else {
                Duration::from_millis(250)
            };
            match receiver.recv_timeout(timeout) {
                Ok(Command::Start) => {
                    if !self.active {
                        let result = self.start_session();
                        self.report(result);
                    }
                }
                Ok(Command::Stop) => {
                    if self.active {
                        let result = self.stop_session();
                        self.report(result);
                    }
                }
                Ok(Command::Toggle) => {
                    let result = if self.active {
                        self.stop_session()
                    } else {
                        self.start_session()
                    };
                    self.report(result);
                }
                Ok(Command::Shutdown) => {
                    if self.active {
                        let result = self.stop_session();
                        self.report(result);
                    }
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {
                    if self.active {
                        let result = self.tick();
                        self.report(result);
                    } else {
                        self.unload_if_cold();
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    if self.active {
                        let result = self.stop_session();
                        self.report(result);
                    }
                    return;
                }
            }
        }
    }

    fn report(&mut self, result: Result<(), DictationError>) {
        if let Err(error) = result {
            eprintln!("Quil dictation error: {error}");
            self.reset_capture();
        }
    }

    fn start_session(&mut self) -> Result<(), DictationError> {
        if self.capture.is_some() {
            return Ok(());
        }
        let capture =
            MicrophoneCapture::start(self.options.mic.as_deref(), MAX_WINDOW_SECONDS as u32)?;
        let input_rate = capture.input_rate();
        self.capture = Some(capture);
        self.native_audio = Some(AudioBuffer::with_capacity(
            input_rate,
            input_rate as usize * MAX_WINDOW_SECONDS,
        ));
        self.committer.reset();
        self.active = true;
        self.speech_seen = false;
        self.silence_ticks = 0;
        self.last_inference = None;
        eprintln!("Quil listening…");
        self.ensure_model()
    }

    fn stop_session(&mut self) -> Result<(), DictationError> {
        let result = (|| {
            if let Some(capture) = self.capture.as_mut() {
                let input_rate = capture.input_rate();
                let remaining = capture.stop()?;
                let remaining = AudioBuffer::new(input_rate, remaining.samples().to_vec());
                self.append_samples(remaining.samples(), input_rate)?;
            }
            if self.speech_seen && self.model.is_some() {
                self.transcribe_window(true)?;
            }
            self.last_used = Instant::now();
            Ok(())
        })();
        self.reset_capture();
        result
    }

    fn reset_capture(&mut self) {
        self.capture = None;
        self.native_audio = None;
        self.committer.reset();
        self.active = false;
        self.speech_seen = false;
        self.silence_ticks = 0;
        self.last_inference = None;
    }

    fn ensure_model(&mut self) -> Result<(), DictationError> {
        if self.model.is_none() {
            eprintln!("Loading {} locally…", self.model_spec.id);
            let model = quil_inference::load(
                &self.model_spec,
                &self.model_dir.join(&self.model_spec.id),
                self.execution,
            )?;
            eprintln!("Model ready: {}", model.execution);
            self.model = Some(model);
        }
        Ok(())
    }

    fn unload_if_cold(&mut self) {
        if self.model.is_some()
            && self.last_used.elapsed() >= Duration::from_secs(self.options.warm_seconds)
        {
            self.model = None;
            eprintln!("Model unloaded after the idle timeout.");
        }
    }

    fn tick(&mut self) -> Result<(), DictationError> {
        self.append_available()?;
        let input_rate = self
            .capture
            .as_ref()
            .ok_or_else(|| AudioError::Invalid("capture is not running".into()))?
            .input_rate();
        let recent_count = (input_rate as usize / 4).max(1);
        let speaking = {
            let native = self
                .native_audio
                .as_ref()
                .ok_or_else(|| AudioError::Invalid("capture buffer is not initialized".into()))?;
            let start = native.len().saturating_sub(recent_count);
            has_speech(&native.samples()[start..])
        };

        if speaking {
            self.speech_seen = true;
            self.silence_ticks = 0;
            let due = self
                .last_inference
                .is_none_or(|instant| instant.elapsed() >= TICK);
            if due && self.model.is_some() {
                self.transcribe_window(false)?;
                self.last_inference = Some(Instant::now());
            }
        } else if self.speech_seen {
            self.silence_ticks = self.silence_ticks.saturating_add(1);
            if self.silence_ticks >= 3 {
                self.finish_utterance()?;
            }
        } else {
            let native = self
                .native_audio
                .as_mut()
                .ok_or_else(|| AudioError::Invalid("capture buffer is not initialized".into()))?;
            let keep = input_rate as usize;
            if native.len() > keep {
                native.drop_prefix(native.len() - keep);
            }
        }
        Ok(())
    }

    fn finish_utterance(&mut self) -> Result<(), DictationError> {
        if self.model.is_some() {
            self.transcribe_window(true)?;
        }
        if let Some(native) = self.native_audio.as_mut() {
            native.truncate(0);
        }
        self.committer.reset();
        self.speech_seen = false;
        self.silence_ticks = 0;
        Ok(())
    }

    fn append_available(&mut self) -> Result<(), DictationError> {
        let capture = self
            .capture
            .as_mut()
            .ok_or_else(|| AudioError::Invalid("capture is not running".into()))?;
        let input_rate = capture.input_rate();
        let samples = AudioBuffer::new(input_rate, capture.read_available()?.samples().to_vec());
        self.append_samples(samples.samples(), input_rate)
    }

    fn append_samples(&mut self, samples: &[f32], input_rate: u32) -> Result<(), DictationError> {
        let native = self
            .native_audio
            .as_mut()
            .ok_or_else(|| AudioError::Invalid("capture buffer is not initialized".into()))?;
        let limit = input_rate as usize * MAX_WINDOW_SECONDS;
        let overflow = native
            .len()
            .saturating_add(samples.len())
            .saturating_sub(limit);
        if overflow > 0 {
            native.drop_prefix(overflow);
        }
        native
            .try_extend_from_slice(samples, limit)
            .map_err(|_| AudioError::Overflow)?;
        Ok(())
    }

    fn transcribe_window(&mut self, final_chunk: bool) -> Result<(), DictationError> {
        let (input_rate, samples) = {
            let native = self
                .native_audio
                .as_ref()
                .ok_or_else(|| AudioError::Invalid("capture buffer is not initialized".into()))?;
            let input_rate = native.sample_rate();
            let window_seconds = if final_chunk { 12 } else { 6 };
            let window = input_rate as usize * window_seconds;
            let start = native.len().saturating_sub(window);
            (input_rate, native.samples()[start..].to_vec())
        };
        if samples.len() < input_rate as usize / 10 {
            return Ok(());
        }
        let mut audio = resample_to_16k(AudioBuffer::new(input_rate, samples))?;
        let model = self
            .model
            .as_mut()
            .ok_or_else(|| AudioError::Invalid("model is not loaded".into()))?;
        let result = model.transcriber.transcribe(
            audio.samples(),
            &TranscribeOptions {
                language: self.options.language.clone(),
            },
        );
        audio.clear();
        let transcript = result?;
        self.last_used = Instant::now();
        let delta = if final_chunk {
            self.committer.finish(&transcript)
        } else {
            self.committer.update(&transcript)
        };
        if !delta.is_empty() {
            self.inserter.insert(&delta)?;
        }
        Ok(())
    }
}
