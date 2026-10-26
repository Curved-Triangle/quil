use std::{
    path::{Path, PathBuf},
    process::{Command as ProcessCommand, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel},
    },
    thread,
    time::{Duration, Instant},
};

use global_hotkey::{
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
    hotkey::{Code, HotKey},
};
use quil_audio::{AudioError, MicrophoneCapture, resample_to_16k};
use quil_core::{AudioBuffer, ModelSpec, TranscribeOptions, TranscriptionError};
use quil_inference::{Execution, InferenceError, LoadedModel};
use thiserror::Error;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::WindowId,
};

use crate::{
    insert::{self, InsertError, TextInserter},
    streaming::{PartialCommitter, has_speech},
};

const TICK: Duration = Duration::from_millis(250);
const MAX_WINDOW_SECONDS: usize = 30;
/// Safety cap on a continuous dictation session. A push-to-talk hold rarely
/// exceeds a couple of minutes; if the key-release event is lost while apps
/// change focus, nothing else on screen would stop a session that keeps
/// capturing. Toggle sessions allow longer deliberate runs, so they use a
/// wider cap.
const MAX_HOLD_SESSION: Duration = Duration::from_secs(120);
const MAX_TOGGLE_SESSION: Duration = Duration::from_secs(300);
const CHIME_START: &str = "chime-start.wav";
const CHIME_STOP: &str = "chime-stop.wav";

#[derive(Debug)]
pub struct Options {
    pub mic: Option<String>,
    pub hotkey: String,
    pub mode: Mode,
    pub warm_seconds: u64,
    pub language: Option<String>,
    pub chime: bool,
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
    let mic_label = options.mic.clone();
    let mode = options.mode;
    let (sender, receiver) = sync_channel::<Command>(16);
    let (startup_sender, startup_receiver) = sync_channel(1);
    let hotkey_down = Arc::new(AtomicBool::new(false));
    let worker_gate = Arc::clone(&hotkey_down);
    let model = model.clone();
    let model_dir = model_dir.to_path_buf();
    let worker = thread::Builder::new()
        .name("quil-dictation".into())
        .spawn(move || {
            let mut worker = match Worker::new(model, model_dir, execution, options, worker_gate) {
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
    // Name the input up front: on machines with virtual audio bridges
    // (Pro Tools, Zoom, Teams), the system default can be a silent device.
    match mic_label.as_deref() {
        Some(mic) => eprintln!("Using input device: {mic}."),
        None => eprintln!(
            "Using the system default input device; `quil mics` lists inputs and --mic selects one."
        ),
    }
    let listener_sender = sender.clone();
    GlobalHotKeyEvent::set_event_handler(Some(move |event| {
        dispatch_hotkey(&listener_sender, &hotkey_down, event, mode);
    }));
    let manager =
        GlobalHotKeyManager::new().map_err(|error| DictationError::Hotkey(format!("{error:?}")))?;
    manager
        .register(HotKey::new(None, hotkey))
        .map_err(|error| DictationError::Hotkey(format!("{error:?}")))?;
    let listener_result = (|| -> Result<(), DictationError> {
        let event_loop =
            EventLoop::new().map_err(|error| DictationError::Hotkey(format!("{error:?}")))?;
        event_loop
            .run_app(&mut HotkeyApp)
            .map_err(|error| DictationError::Hotkey(format!("{error:?}")))
    })();
    let _ = sender.send(Command::Shutdown);
    let _ = worker.join();
    listener_result
}

struct HotkeyApp;

impl ApplicationHandler for HotkeyApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        event_loop.set_control_flow(ControlFlow::Wait);
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        _event: WindowEvent,
    ) {
    }
}

fn dispatch_hotkey(
    sender: &SyncSender<Command>,
    hotkey_down: &AtomicBool,
    event: GlobalHotKeyEvent,
    mode: Mode,
) {
    match event.state {
        HotKeyState::Pressed => {
            if hotkey_down.swap(true, Ordering::AcqRel) {
                return;
            }
            let command = match mode {
                Mode::Hold => Command::Start,
                Mode::Toggle => Command::Toggle,
            };
            let _ = sender.try_send(command);
        }
        HotKeyState::Released => {
            hotkey_down.store(false, Ordering::Release);
            if mode == Mode::Hold {
                let _ = sender.try_send(Command::Stop);
            }
        }
    }
}

fn session_cap(mode: Mode) -> Duration {
    match mode {
        Mode::Hold => MAX_HOLD_SESSION,
        Mode::Toggle => MAX_TOGGLE_SESSION,
    }
}

fn parse_hotkey(value: &str) -> Result<Code, DictationError> {
    match value.to_ascii_uppercase().as_str() {
        "F1" => Ok(Code::F1),
        "F2" => Ok(Code::F2),
        "F3" => Ok(Code::F3),
        "F4" => Ok(Code::F4),
        "F5" => Ok(Code::F5),
        "F6" => Ok(Code::F6),
        "F7" => Ok(Code::F7),
        "F8" => Ok(Code::F8),
        "F9" => Ok(Code::F9),
        "F10" => Ok(Code::F10),
        "F11" => Ok(Code::F11),
        "F12" => Ok(Code::F12),
        _ => Err(DictationError::Hotkey(format!(
            "unsupported hotkey `{value}`; choose F1-F12 so shortcuts never insert printable characters"
        ))),
    }
}

fn chime_path(filename: &str) -> PathBuf {
    if let Some(directory) = std::env::var_os("QUIL_ASSET_DIR") {
        return PathBuf::from(directory).join(filename);
    }
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets")
        .join(filename);
    if source.exists() {
        return source;
    }
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
        .join(".local")
        .join("share")
        .join("quil")
        .join("assets")
        .join(filename)
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
    hotkey_down: Arc<AtomicBool>,
    session_started: Option<Instant>,
    session_words: usize,
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
        hotkey_down: Arc<AtomicBool>,
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
            hotkey_down,
            session_started: None,
            session_words: 0,
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
                        if self.session_expired() {
                            self.recover_lost_release();
                            let result = self.stop_session();
                            self.report(result);
                        } else {
                            let result = self.tick();
                            self.report(result);
                        }
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
            if matches!(error, DictationError::Audio(AudioError::Stream)) {
                eprintln!(
                    "The microphone stream ended early: check microphone permission for this \
                     app in System Settings → Privacy & Security → Microphone, that no other \
                     app is holding the input, and that the input device still exists. \
                     `quil mics` lists inputs and --mic selects one."
                );
            }
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
        self.session_started = Some(Instant::now());
        self.speech_seen = false;
        self.silence_ticks = 0;
        self.last_inference = None;
        eprintln!("Quil listening…");
        self.play_chime(CHIME_START);
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
            self.play_chime(CHIME_STOP);
            Ok(())
        })();
        let words = self.session_words;
        self.reset_capture();
        if result.is_ok() {
            if words > 0 {
                eprintln!("Quil stopped listening; inserted {words} word(s).");
            } else {
                eprintln!(
                    "Quil stopped listening; no speech detected. If this repeats, the input \
                     device may be wrong or silent: `quil mics` lists inputs, --mic selects \
                     one, and your terminal app needs microphone permission."
                );
            }
        }
        result
    }

    fn reset_capture(&mut self) {
        self.capture = None;
        self.native_audio = None;
        self.committer.reset();
        self.active = false;
        self.session_started = None;
        self.session_words = 0;
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

    /// True when the active session ran past its mode's cap without a stop
    /// event; the worker polls this on every 25 ms tick.
    fn session_expired(&self) -> bool {
        self.session_started
            .is_some_and(|started| started.elapsed() >= session_cap(self.options.mode))
    }

    /// A session that reaches the cap means the stop never arrived: the
    /// key-release (push-to-talk) or stop press (toggle) can be swallowed
    /// while the frontmost app changes. Announce the stop, clear the
    /// listener's pressed-gate so the next press starts a fresh session
    /// instead of being swallowed, and let the caller transcribe what was
    /// captured — mic capture can never run unbounded.
    fn recover_lost_release(&mut self) {
        self.hotkey_down.store(false, Ordering::Release);
        eprintln!(
            "Quil stopped the session after the {}-second safety cap: no stop event arrived, \
             and hotkey events can be lost while apps change focus. Press {} again to dictate.",
            session_cap(self.options.mode).as_secs(),
            self.options.hotkey
        );
    }

    fn play_chime(&self, filename: &str) {
        if !self.options.chime {
            return;
        }
        let path = chime_path(filename);
        if path.exists() {
            // `afplay` is a local system utility, invoked by absolute path so a
            // poisoned PATH entry cannot redirect the spawn.
            let _ = ProcessCommand::new("/usr/bin/afplay")
                .arg(path)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn();
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
        // Length-check before copying. The short-window early return used to
        // leave an unwiped PCM `Vec` behind, contradicting the claim that
        // captured audio is zeroed after use. The single allocation below is
        // an `AudioBuffer`, which wipes itself on drop.
        let clip = {
            let native = self
                .native_audio
                .as_ref()
                .ok_or_else(|| AudioError::Invalid("capture buffer is not initialized".into()))?;
            let input_rate = native.sample_rate();
            let window_seconds = if final_chunk { 12 } else { 6 };
            let window = input_rate as usize * window_seconds;
            let start = native.len().saturating_sub(window);
            let selected = &native.samples()[start..];
            if selected.len() < input_rate as usize / 10 {
                return Ok(());
            }
            AudioBuffer::new(input_rate, selected.to_vec())
        };
        let mut audio = resample_to_16k(clip)?;
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
            self.session_words += delta.split_whitespace().count();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_has_a_finite_session_cap() {
        assert!(session_cap(Mode::Hold) > Duration::ZERO);
        assert!(session_cap(Mode::Toggle) > Duration::ZERO);
    }

    #[test]
    fn hold_sessions_cap_before_toggle_sessions() {
        // The watchdog primarily guards push-to-talk: a hold that outlives
        // its cap indicates a lost release, not a deliberate long dictation.
        assert!(session_cap(Mode::Hold) < session_cap(Mode::Toggle));
    }
}
