<img src="assets/quil-logo.jpg" alt="Quil" width="640">

# Quil

Quil is an open-source, fully local speech-to-text dictation app for macOS on Apple Silicon. The goal is simple: hold a hotkey in any app, speak, and have text appear at the cursor—without audio or transcription leaving the machine.

> **Status:** private development. Milestone 1 is complete, and an early CLI version of global-hotkey streaming dictation is available. The native menu-bar app, full settings/model manager, and release hardening remain.

## Principles

- **Local by construction:** transcription binaries have no HTTP client; model downloads live in a separate explicit tool.
- **Apple Silicon performance:** Metal, Core ML, and Accelerate paths are benchmarked rather than assumed.
- **Private audio:** microphone samples remain in bounded memory buffers, are never written to disk, and are zeroed after use.
- **Permissive licensing:** Quil is MIT. Models and third-party components are license-checked and documented.

## Build

Requirements: macOS on Apple Silicon, Rust via `rustup`, Python 3, and CMake for `whisper.cpp`. The repository pins Rust 1.94.0. Fetch the separately pinned and SHA-256-verified ONNX Runtime build artifact once, then point Cargo at it:

```bash
python3 scripts/fetch_onnxruntime.py
export ORT_LIB_PATH="$PWD/.tools/onnxruntime/lib"
cargo build --release
```

`scripts/cargo.sh` sets `ORT_LIB_PATH` (and a repo-local CMake, if present) for each command instead.

List available models (no network access):

```bash
cargo run -p quil -- models
```

The milestone 1 default is **Moonshine Tiny on CPU**, selected from the local latency/RAM measurements in `BENCHMARKS.md`. Omitting `--model` from `record` or `transcribe` uses that default.

Explicitly install a model, verifying every file against the checked-in SHA-256 manifest:

```bash
cargo run -p quil-model-fetch -- download moonshine-tiny
```

Record five seconds to memory and transcribe it (microphone permission required):

```bash
cargo run --release -p quil -- record --model moonshine-tiny --seconds 5
```

Transcribe an existing local WAV file:

```bash
cargo run --release -p quil -- transcribe --model moonshine-tiny --input speech.wav
```

Microphone capture is never saved. The `transcribe` command is for files you deliberately provide; benchmark audio is separate, documented corpus data.

## Hotkey dictation (early milestone 2/3)

The CLI now supports a global function-key listener, rolling local inference with an energy VAD, and stable-prefix text insertion into the focused app. It is still a CLI, not the final menu-bar application.

Install your model first, then run:

```bash
target/release/quil dictate
```

- **Hold F6** (push-to-talk), speak, and release. Toggle mode is available with `--mode toggle`.
- Choose another function key with `--hotkey F5` through `--hotkey F12`. Function keys are used so the shortcut never inserts a printable character. Depending on your keyboard settings, you may need to hold **Fn** as well.
- `--warm-seconds 300` keeps the model loaded for five idle minutes before unloading it. Set `--warm-seconds 0` to unload immediately after each session.
- Select a microphone with `quil mics` and `--mic "MacBook Pro Microphone"`, or another model with `--model moonshine-base` / `--model whisper-large-v3-turbo`.
- `--execution metal` is used for Whisper; Moonshine defaults to CPU because it was faster in local benchmarks.

Permissions are deliberately separate: macOS microphone access captures speech, while Accessibility is used only for the global key listener and text insertion. If you run Quil inside Terminal, enable **Terminal** in System Settings → Privacy & Security → Accessibility. If you use another host app, enable that app instead. You can disable the permission after this proof of concept; a signed Quil app will request it under its own identity later.

Test insertion without speaking:

```bash
target/release/quil insert-test --delay-ms 1000 "hello from Quil"
```

Focus the target field during the one-second delay. The inserter first tries `AXSelectedText`, then falls back to native keyboard events for controls such as terminal/editor fields. Dictated text is not logged. Audio remains in memory and is wiped as it is consumed.

## Benchmarks

See [BENCHMARKS.md](BENCHMARKS.md) for the test matrix, measurements, and current default-model decision. Each benchmark run records its timestamp, hardware, model revision, latency, real-time factor, peak RSS, and word error rate. No performance number is copied from another machine or presented as a local measurement.

## Roadmap

1. **Spike:** local mic transcription, four-model benchmark, default selection.
2. **Streaming:** early energy VAD, rolling inference, and stable-prefix partials are implemented; Silero VAD and latency validation remain.
3. **Dictation UI:** early global hotkey and Accessibility insertion are implemented; native menu bar and clipboard-preserving fallback remain.
4. **Polish:** settings, model manager, code mode, launch at login.
5. **Hardening:** security review, signing/notarization, one-hour soak, release documentation.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [MODELS.md](MODELS.md). Quil is available under the [MIT License](LICENSE). The repository stays private until a deliberate open-source release.
