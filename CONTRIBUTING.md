# Contributing to Quil

Thanks for helping build private, local dictation. By contributing, you agree that your contribution is licensed under Quil's MIT License.

## Development setup

Use macOS on Apple Silicon for inference and audio testing. Install `rustup`, Python 3, and CMake; the repository's `rust-toolchain.toml` selects the supported Rust version. Run `python3 scripts/fetch_onnxruntime.py` once to install the verified static runtime under `.tools/`, then use `scripts/cargo.sh` or export `ORT_LIB_PATH` as shown in the README.

```bash
python3 scripts/fetch_onnxruntime.py
cargo build
cargo test --workspace
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo audit --deny warnings
cargo deny check
```

Run the full locked versions of these commands before submitting a change. Do not add a dependency without checking its license, maintenance, binary-size impact, and runtime network behavior.

## Changes and tests

- Keep changes focused and document user-facing command changes.
- Add tests for bug fixes and new behavior.
- Do not allocate, lock, block, log, or perform I/O in the audio callback.
- Never add captured microphone audio or private dictated text as a fixture.
- Every new unsafe block needs a concrete safety justification.
- Update `MODELS.md` and the checksum catalog for model changes.
- Update `BENCHMARKS.md` only with reproducible measurements, including hardware and backend details.

## Privacy and licensing review

New runtime HTTP clients, telemetry, automatic updates, GPL/AGPL dependencies, and unverified model downloads are not accepted. Network code belongs only in the explicit model-fetch component and requires strong justification.

## Repository workflow

The repository is private during initial development. Do not make it public, publish releases, or publish model artifacts without maintainer approval. Code signing identities, notarization credentials, and secrets must never be committed.
