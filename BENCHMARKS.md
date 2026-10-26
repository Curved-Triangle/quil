# Benchmarks

## Milestone 1 measured results

Measurements were taken locally on **2026-10-25** from release binaries. They are not vendor-published numbers. The default is **Moonshine Tiny with the ONNX Runtime CPU provider** because latency and memory are the top-priority constraints; Moonshine Base is the balanced accuracy option and Whisper large-v3-turbo is the accuracy option.

### Method

- Corpus: Mini LibriSpeech `dev-clean-2`, eight utterances from four speakers, **74.655 seconds / 183 reference words**.
- Audio: identical 16 kHz mono signed 16-bit PCM converted from the source FLAC; no Quil microphone recordings.
- Runs: one cold transcription followed by three warm corpus transcriptions per configuration; each configuration ran in a fresh process.
- `First`: time to transcribe the first corpus clip immediately after model load. **This is not a streaming-first-partial measurement.**
- `Warm`: median total time for all eight clips across three warm runs.
- `RTF`: warm median / total audio duration; lower is better.
- `Peak RSS`: process lifetime peak including load, corpus, model, and allocator overhead; it is not idle RAM.
- `WER`: aggregate edit errors / reference words after identical lowercase/punctuation normalization. Small clean corpora are not statistically representative of all dictation.

| Model / execution | Load (ms) | First (ms) | Warm (ms) | RTF | Peak RSS (MiB) | WER |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Moonshine Tiny / Core ML + CPU fallback | 1547.6 | 536.6 | 1510.7 | 0.0202 | 1104.4 | 3.28% |
| Moonshine Tiny / CPU | 552.7 | 160.2 | 822.6 | 0.0110 | 1207.8 | 3.28% |
| Moonshine Base / Core ML + CPU fallback | 2065.6 | 779.0 | 2300.9 | 0.0308 | 1704.2 | 2.73% |
| Moonshine Base / CPU | 753.1 | 247.3 | 1382.8 | 0.0185 | 1678.4 | 2.73% |
| Distil-Whisper large-v3 / Metal | 412.4 | 345.8 | 2146.9 | 0.0288 | 1723.1 | 1.09% |
| Whisper large-v3-turbo / Metal | 444.5 | 519.0 | 4091.6 | 0.0548 | 1835.1 | 0.00% |

Whisper large-v3-turbo made zero edit errors on this particular 183-word corpus; that does not imply zero WER on general speech. Distil-Whisper's two substitutions were the only errors in its run.

### Resident memory detail

`After load` is sampled immediately after session creation. `After inference` is sampled after all warm runs while the model remains loaded. Allocator caching and KV workspaces explain why these values differ from artifact size and peak RSS.

| Model / execution | After load (MiB) | After inference (MiB) |
| --- | ---: | ---: |
| Moonshine Tiny / Core ML + CPU fallback | 397.1 | 1055.0 |
| Moonshine Tiny / CPU | 419.7 | 1127.7 |
| Moonshine Base / Core ML + CPU fallback | 662.8 | 1665.8 |
| Moonshine Base / CPU | 728.9 | 1611.5 |
| Distil-Whisper large-v3 / Metal | 1649.2 | 1527.9 |
| Whisper large-v3-turbo / Metal | 1743.2 | 1784.3 |

### Acceleration findings

- **Moonshine CPU wins over the current Core ML path.** Dynamic-shape MLProgram conversion logs unsupported-shape errors and falls back to CPU for partitions. Explicit Core ML runs are included for transparency; `Execution::Auto` therefore selects CPU for Moonshine. A Core ML result is not presented as an ANE-only result.
- **Whisper Metal works.** `whisper-rs` reports whisper.cpp 1.8.3 with Metal + Accelerate. A separate first-ever Metal smoke initialization spent 5.243 s compiling embedded shaders; the load values above were measured with the shader cache warm.
- Whisper Core ML encoder artifacts are not yet in the verified catalog, so no Whisper Core ML benchmark is claimed.

### Short repeated-inference check

Moonshine Tiny CPU completed 20 warm iterations over the eight-clip corpus (160 utterances) after its cold pass. RSS was 1150.0 MiB after run 1 and 1123.5 MiB after run 20; the final 12 samples were flat at 1123.5 MiB. Warm-corpus medians stayed near 0.83 s. This short check showed no upward memory trend, but it is not the required one-hour soak.

Per-run RSS (MiB): `1150.0, 1137.8, 1124.6, 1115.4, 1113.4, 1119.5, 1120.0, 1123.5, 1123.5, 1123.5, 1123.5, 1123.5, 1123.5, 1123.5, 1123.5, 1123.5, 1123.5, 1123.5, 1123.5, 1123.5`

### Environment

| Field | Value |
| --- | --- |
| Hardware | Apple M5 Max |
| Architecture | aarch64 |
| macOS | 26.4 |
| Power | 100% battery, discharging |
| Rust | 1.94.0 |
| CMake | 4.4.4 |
| Deployment target | macOS 13.4 |
| whisper.cpp | 1.8.3 via whisper-rs 0.16.0 |
| ONNX Runtime | 1.31.0 via ort 2.0.0-rc.15 |
| Model revisions | Pinned in `models/catalog.json` |

## Targets and current status

| Target | Status |
| --- | --- |
| First partial < 300 ms after speech starts | **Milestone 2 measurement.** Offline first-transcription times above must not be substituted for this target. |
| Idle RAM < 50 MB with model unloaded | Menu-bar app target; not measured by this CLI spike. |
| Idle CPU ~0% | Menu-bar app target; not measured by this CLI spike. |
| No memory growth over one hour | Milestone 5 soak test; not claimed yet. |

## Reproduce

```bash
python3 scripts/fetch_onnxruntime.py
python3 scripts/prepare_benchmark_corpus.py
scripts/cargo.sh build --release --workspace
scripts/run-benchmarks.sh benchmark-data/corpus/benchmark-corpus.json
python3 scripts/render_benchmarks.py
```

Raw reports and logs are written to `benchmark-data/results/` and are gitignored. `tests/fixtures/benchmark-corpus.json` is the smaller CI smoke corpus. See `benchmarks/NOTICE.md` and `tests/fixtures/NOTICE.md` for corpus attribution. CI applies pinned smoke-model thresholds rather than treating a missing model or corpus as a pass.
