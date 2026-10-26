# Quil Agent Guide

Quil is a fully local macOS dictation application. This repository currently targets milestone 1: an in-memory microphone CLI, explicit model management, and reproducible Apple Silicon benchmarks. Later milestones add streaming, the native menu-bar app, settings, and release hardening.

## Commands

```bash
python3 scripts/fetch_onnxruntime.py  # once; explicit, verified build-time fetch
scripts/cargo.sh fmt --all -- --check
scripts/cargo.sh clippy --workspace --all-targets --locked -- -D warnings
scripts/cargo.sh test --workspace --locked
cargo audit --deny warnings
cargo deny check
```

Use the pinned Rust toolchain. `scripts/cargo.sh` sets `ORT_LIB_PATH` to the verified static ONNX Runtime and adds a repo-local CMake to PATH when available.

For user-facing install/run flows, use `make install`, `make models`, and `make run`. Installation is user-local under `~/.local` and never uses Docker. `scripts/install.sh` copies the binaries and original chime assets. Do not add ElevenLabs, online TTS, or any other network-dependent notification service; `--no-chime` disables sounds. Do not claim a command or benchmark passed unless it actually ran. Never invent benchmark numbers.

## Architecture

- `quil-core`: model catalog, audio ownership, shared types, and text metrics.
- `quil-audio`: Core Audio capture and bounded, real-time-safe sample transport.
- `quil-inference`: local ASR backends behind a common interface.
- `quil`: recording, file transcription, benchmarks, and early hotkey dictation. No HTTP dependency.
- `quil-model-fetch`: the only component permitted to contain an HTTP client.

ONNX Runtime permits exactly one environment per process. `moonshine.rs` keeps it in a process-wide static so the idle model unload and reload cycle in dictation reuses it instead of failing; execution providers are registered per session.

`quil/src/dictate.rs` owns hotkey/session lifecycle and the lost-release watchdog: a session that outlives its mode-dependent cap is stopped and transcribed, and the pressed-gate is cleared, so a swallowed key event cannot leave the microphone capturing indefinitely. `streaming.rs` owns the energy VAD and append-only stable-prefix committer. `insert.rs` uses direct Core Foundation/AXUIElement bindings (`AXSelectedText`) with native keyboard-event fallback; do not reintroduce a Swift runtime dependency. Function keys are the only supported hotkeys so shortcuts cannot insert printable characters. Keep Accessibility and microphone explanations precise, and never log other keys or dictated text.

Keep the runtime dependency graph free of HTTP clients, telemetry, automatic updates, and network-based model hubs. Model files are installed only by an explicit `quil-model-fetch` command or an equivalent deliberate UI action later.

## Non-negotiable rules

- Never write captured microphone audio to disk, logs, crash reports, or test fixtures. Benchmark WAV files must come from documented external datasets or synthetic sources.
- Zero captured audio buffers after use. Keep queues bounded; the audio callback must not allocate, lock, block, log, or perform I/O.
- Every downloadable file needs an independently pinned SHA-256 in `models/catalog.json`. Verify before atomic installation, re-verify the installed destination after the rename, and verify byte length plus SHA-256 again in the runtime before every model load. Never treat a checksum calculated from the same unverified download as the expected value, and never trust file size alone.
- Verify model and dependency licenses before adding them. No GPL/AGPL in the core. Update `MODELS.md` and `deny.toml` together with model/backend changes.
- Runtime app targets must not include network entitlements. Only microphone and accessibility permissions may be requested, with purpose text.
- Keep `unsafe` out of pure-Rust crates. Isolate unavoidable platform FFI and justify every unsafe block with a comment explaining its invariants.
- Keep direct dependencies and GitHub Actions pinned. Review Cargo.lock changes.
- The repository is public as an early-development project; do not tag a stable release without an explicit user request.
- Do not commit or push without separate, current-turn user confirmation.

## Documentation

Update `README.md`, `MODELS.md`, `BENCHMARKS.md`, and this file when commands, architecture, model support, or measurements change. `BENCHMARKS.md` distinguishes measured results from future targets. The source logo is `assets/quil-logo.jpg`; do not replace it with unrelated branding.
