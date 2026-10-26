# Benchmarks

## Milestone 1 measured results

Measurements were taken locally through **2026-10-26** from release binaries. They are not vendor-published numbers. The default is **Moonshine Tiny with the ONNX Runtime CPU provider** because latency and memory are the top-priority constraints; Moonshine Base is the balanced accuracy option and Whisper large-v3-turbo is the accuracy option.

### Method

- Corpus: `mini-librispeech-dev-clean-2-eight-clips`, 8 utterances from 4 speakers, **74.655 seconds / 183 reference words**.
- Audio: identical 16 kHz mono signed 16-bit PCM converted from the source FLAC; no Quil microphone recordings.
- Runs: one cold transcription followed by 3 warm corpus transcriptions per configuration; each configuration ran in a fresh process.
- `First`: time to transcribe the first corpus clip immediately after model load. **This is not a streaming-first-partial measurement.**
- `Warm`: median total time for all clips across the warm runs.
- `RTF`: warm median / total audio duration; lower is better.
- `Peak RSS`: process lifetime peak including load, corpus, model, and allocator overhead; it is not idle RAM.
- `WER`: aggregate edit errors / reference words after identical lowercase/punctuation normalization. Small clean corpora are not statistically representative of all dictation.
- Renderer validation: all reports must share corpus metadata, hardware and toolchain metadata, run counts, and catalog revisions; otherwise rendering fails instead of publishing mixed claims. Power state is recorded but may vary between runs.

| Model / execution | Load (ms) | First (ms) | Warm (ms) | RTF | Peak RSS (MiB) | WER |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Moonshine Tiny / Core ML + CPU fallback | 4343.5 | 1996.7 | 4955.0 | 0.0664 | 942.6 | 3.28% |
| Moonshine Tiny / CPU | 2602.2 | 634.3 | 2958.5 | 0.0396 | 1153.9 | 3.28% |
| Moonshine Base / Core ML + CPU fallback | 5932.5 | 4022.3 | 4607.8 | 0.0617 | 1817.7 | 2.73% |
| Moonshine Base / CPU | 1267.1 | 426.5 | 3222.3 | 0.0432 | 1710.5 | 2.73% |
| Distil-Whisper large-v3 / Metal | 788.7 | 333.5 | 4660.7 | 0.0624 | 1767.4 | 1.09% |
| Whisper large-v3-turbo / Metal | 811.6 | 560.4 | 8337.6 | 0.1117 | 1869.9 | 0.00% |

Whisper large-v3-turbo had 0 total edit operations on this particular 183-word corpus; that does not imply the same WER on general speech. Distil-Whisper had 2 total edit operations in its run.

### Resident memory detail

`After load` is sampled immediately after session creation. `After inference` is sampled after all warm runs while the model remains loaded. Allocator caching and KV workspaces explain why these values differ from artifact size and peak RSS.

| Model / execution | After load (MiB) | After inference (MiB) |
| --- | ---: | ---: |
| Moonshine Tiny / Core ML + CPU fallback | 387.5 | 570.1 |
| Moonshine Tiny / CPU | 405.6 | 693.1 |
| Moonshine Base / Core ML + CPU fallback | 653.8 | 1794.6 |
| Moonshine Base / CPU | 730.0 | 1639.6 |
| Distil-Whisper large-v3 / Metal | 1651.4 | 1736.5 |
| Whisper large-v3-turbo / Metal | 1744.4 | 1819.0 |

### Acceleration findings

- **Moonshine CPU wins over the current Core ML path.** Dynamic-shape MLProgram conversion logs unsupported-shape errors and falls back to CPU for partitions. Explicit Core ML runs are included for transparency; `Execution::Auto` therefore selects CPU for Moonshine. A Core ML result is not presented as an ANE-only result.
- **Whisper Metal works.** `whisper-rs` reports whisper.cpp 1.8.3 with Metal + Accelerate. A separate first-ever Metal smoke initialization spent 5.243 s compiling embedded shaders; the load values above were measured with the shader cache warm.
- Whisper Core ML encoder artifacts are not yet in the verified catalog, so no Whisper Core ML benchmark is claimed.

### Short repeated-inference check

Moonshine Tiny CPU completed 20 warm iterations over the 8-clip corpus (160 utterances) after its cold pass. RSS was 1351.9 MiB after run 1 and 1328.4 MiB after run 20; the final 4 samples were flat at 1328.4 MiB. Median warm-corpus time was 1.26 s. This short check showed no upward memory trend, but it is not the required one-hour soak.

Per-run RSS (MiB): `1351.9, 1339.2, 1329.5, 1320.3, 1318.3, 1324.4, 1324.4, 1324.4, 1324.4, 1324.4, 1324.4, 1324.4, 1324.9, 1324.9, 1324.9, 1324.9, 1328.4, 1328.4, 1328.4, 1328.4`

### Environment

| Field | Value |
| --- | --- |
| Hardware | Apple M5 Max |
| Architecture | aarch64 |
| macOS | 26.4 |
| Power | 100%; charged |
| Rust | rustc 1.94.0 (4a4ef493e 2026-03-02) |
| CMake | cmake version 4.4.4 |
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
python3 scripts/render_benchmarks.py --corpus benchmark-data/corpus/benchmark-corpus.json
```

Raw reports and logs are written to `benchmark-data/results/` and are gitignored. `tests/fixtures/benchmark-corpus.json` is the smaller CI smoke corpus. See `benchmarks/NOTICE.md` and `tests/fixtures/NOTICE.md` for corpus attribution. CI applies pinned smoke-model thresholds rather than treating a missing model or corpus as a pass.
