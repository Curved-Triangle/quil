<img src="assets/quil-logo.jpg" alt="Quil" width="640">

# Quil

Quil is an open-source, fully local speech-to-text dictation app for macOS on Apple Silicon. The goal is simple: hold a hotkey in any app, speak, and have text appear at the cursor—without audio or transcription leaving the machine.

> **Status:** private development. The milestone 1 spike is implemented: local transcription, explicit model management, and a four-model benchmark. Streaming partials, global hotkeys, cursor insertion, and the menu-bar app remain for later milestones.

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

## Benchmarks

See [BENCHMARKS.md](BENCHMARKS.md) for the test matrix, measurements, and current default-model decision. Each benchmark run records hardware, model revision, latency, real-time factor, peak RSS, and word error rate. No performance number is copied from another machine or presented as a local measurement.

## Roadmap

1. **Spike:** local mic transcription, four-model benchmark, default selection.
2. **Streaming:** VAD, chunked inference, real-time stable partials.
3. **Dictation UI:** native menu bar, global hotkey, Accessibility insertion and clipboard fallback.
4. **Polish:** settings, model manager, code mode, launch at login.
5. **Hardening:** security review, signing/notarization, one-hour soak, release documentation.

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md), [SECURITY.md](SECURITY.md), and [MODELS.md](MODELS.md). Quil is available under the [MIT License](LICENSE). The repository stays private until a deliberate open-source release.
