# Models and licenses

Quil treats model weights as third-party artifacts, not as relicensed Quil code. Every shipped artifact must permit redistribution alongside the MIT application, have its upstream license recorded, and have every downloaded file pinned by SHA-256 in `models/catalog.json`.

## Milestone 1 candidates

| Model | Weights license | Runtime/format | Language | Status |
| --- | --- | --- | --- | --- |
| [OpenAI Whisper large-v3-turbo](https://huggingface.co/openai/whisper-large-v3-turbo) | MIT | whisper.cpp / GGML, Metal with Core ML variant | Multilingual | Accuracy option; measured |
| [Distil-Whisper large-v3](https://huggingface.co/distil-whisper/distil-large-v3-ggml) | MIT | whisper.cpp / GGML, Metal | English | Measured |
| [Moonshine Tiny](https://huggingface.co/UsefulSensors/moonshine-tiny) | MIT | ONNX / Core ML and CPU | English | **Default (CPU)** |
| [Moonshine Base](https://huggingface.co/UsefulSensors/moonshine-base) | MIT | ONNX / Core ML and CPU | English | Balanced option; measured |
| [Silero VAD](https://github.com/snakers4/silero-vad) | MIT | ONNX / CPU | Multilingual VAD | Planned for milestone 2 |

Upstream license references:

- OpenAI states that Whisper code and model weights are MIT-licensed: <https://github.com/openai/whisper>.
- Distil-Whisper explicitly inherits the MIT license: <https://github.com/huggingface/distil-whisper/blob/main/LICENSE>.
- Useful Sensors publishes Moonshine checkpoints with MIT license metadata: <https://huggingface.co/UsefulSensors/moonshine>.
- Silero VAD is MIT: <https://github.com/snakers4/silero-vad/blob/master/LICENSE>.

## Default and execution-provider selection

Milestone 1 selects **Moonshine Tiny on ONNX Runtime CPU** as the default, with `default: true` in the catalog. This favors latency and memory while keeping model accuracy acceptable; see the actual measurements in `BENCHMARKS.md`. Moonshine Base offers a balanced alternative, Distil-Whisper a Metal alternative, and Whisper large-v3-turbo the best measured accuracy on the smoke corpus.

`Execution::Auto` uses Metal for Whisper and CPU for Moonshine. Explicit `--execution coreml` remains available for Moonshine experiments, but dynamic-shape conversion currently falls back to CPU for partitions and was slower in local measurements. Default selection is revisited after streaming benchmarks.

## Excluded: Parakeet

NVIDIA Parakeet TDT 0.6B v2/v3 weights are currently labeled **CC-BY-4.0**, not MIT/Apache. They are not in Quil's redistributable model catalog under the current licensing policy. This is a license decision, not a performance judgment.

## Runtime backends

- [whisper.cpp](https://github.com/ggml-org/whisper.cpp): MIT.
- [whisper-rs](https://codeberg.org/tazz4843/whisper-rs): Unlicense (OSI-approved public-domain dedication); the full transitive license set is enforced by `cargo deny`.
- [ONNX Runtime](https://github.com/microsoft/onnxruntime): MIT.
- `ort` / `ort-sys`: MIT OR Apache-2.0; the exact pinned version and transitive licenses are enforced by `cargo deny`.

Other direct Rust libraries use permissive MIT, Apache-2.0, or MIT OR Apache-2.0 licenses (`cpal`, `rtrb`, `rubato`, `clap`, `serde`, `serde_json`, `thiserror`, `zeroize`, `sha2`, `hound`, `ndarray`, and `reqwest`). `libc` is MIT OR Apache-2.0. Hotkey and insertion uses MIT-licensed `rdev` and `enigo`, Apache-2.0 `macos-accessibility-client`, and `objc2-application-services` / `objc2-core-foundation` (MIT OR Apache-2.0 OR Zlib). Build and test tooling uses the same permissive policy; `cargo deny` checks the complete transitive graph.

ONNX Runtime is linked from the macOS arm64/Core ML 1.31.0 artifact published by the `ort` project. `scripts/fetch_onnxruntime.py` performs an explicit build-time fetch and verifies both the pinned archive SHA-256 (`4033efca…68c`) and extracted static-library SHA-256 (`8a9d7b9c…641`) before installation under `.tools/`. It is not a runtime download and does not add an HTTP client to Quil.

## Installation and verification

Models are not bundled into the source repository. Installation is always explicit:

```bash
quil-model-fetch download <model-id>
quil-model-fetch verify <model-id>
```

The downloader uses checked-in expected hashes, downloads to a temporary file, verifies length and SHA-256, and atomically renames only after verification. A hash calculated from the just-downloaded bytes is never accepted as the expected hash. Runtime transcription never downloads or updates a model.

The catalog records the upstream revision/URL, artifact size, SHA-256, and license source. Converted artifacts retain their upstream attribution; conversion does not change the weights' license.

## Benchmark corpus

Benchmark microphone recordings are never created or retained. Accuracy tests use separately downloaded, documented speech corpus files. Corpus licenses apply to the corpus, not to model weights or Quil code; corpus audio is gitignored and is not redistributed in Quil releases.
