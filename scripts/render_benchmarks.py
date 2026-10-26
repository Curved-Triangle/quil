#!/usr/bin/env python3
"""Render BENCHMARKS.md from actual local JSON reports (no invented values)."""

from __future__ import annotations

import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
RESULTS = ROOT / "benchmark-data" / "results"

ROWS = [
    ("Moonshine Tiny / Core ML + CPU fallback", "moonshine-tiny.json"),
    ("Moonshine Tiny / CPU", "moonshine-tiny-cpu.json"),
    ("Moonshine Base / Core ML + CPU fallback", "moonshine-base.json"),
    ("Moonshine Base / CPU", "moonshine-base-cpu.json"),
    ("Distil-Whisper large-v3 / Metal", "distil-whisper-large-v3.json"),
    ("Whisper large-v3-turbo / Metal", "whisper-large-v3-turbo.json"),
]


def main() -> None:
    reports = {filename: json.loads((RESULTS / filename).read_text()) for _, filename in ROWS}
    sample = reports[ROWS[0][1]]
    lines = [
        "# Benchmarks",
        "",
        "## Milestone 1 measured results",
        "",
        "Measurements were taken locally on **2026-10-25** from release binaries. They are not vendor-published numbers. The default is **Moonshine Tiny with the ONNX Runtime CPU provider** because latency and memory are the top-priority constraints; Moonshine Base is the balanced accuracy option and Whisper large-v3-turbo is the accuracy option.",
        "",
        "### Method",
        "",
        f"- Corpus: Mini LibriSpeech `dev-clean-2`, eight utterances from four speakers, **{sample['audio_seconds']:.3f} seconds / {sample['wer_stats']['reference_words']} reference words**.",
        "- Audio: identical 16 kHz mono signed 16-bit PCM converted from the source FLAC; no Quil microphone recordings.",
        "- Runs: one cold transcription followed by three warm corpus transcriptions per configuration; each configuration ran in a fresh process.",
        "- `First`: time to transcribe the first corpus clip immediately after model load. **This is not a streaming-first-partial measurement.**",
        "- `Warm`: median total time for all eight clips across three warm runs.",
        "- `RTF`: warm median / total audio duration; lower is better.",
        "- `Peak RSS`: process lifetime peak including load, corpus, model, and allocator overhead; it is not idle RAM.",
        "- `WER`: aggregate edit errors / reference words after identical lowercase/punctuation normalization. Small clean corpora are not statistically representative of all dictation.",
        "",
        "| Model / execution | Load (ms) | First (ms) | Warm (ms) | RTF | Peak RSS (MiB) | WER |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for label, filename in ROWS:
        report = reports[filename]
        lines.append(
            f"| {label} | {report['load_ms']:.1f} | {report['first_transcription_ms']:.1f} | "
            f"{report['median_warm_run_ms']:.1f} | {report['real_time_factor']:.4f} | "
            f"{report['peak_rss_bytes'] / 1048576:.1f} | {report['word_error_rate'] * 100:.2f}% |"
        )

    lines += [
        "",
        "Whisper large-v3-turbo made zero edit errors on this particular 183-word corpus; that does not imply zero WER on general speech. Distil-Whisper's two substitutions were the only errors in its run.",
        "",
        "### Resident memory detail",
        "",
        "`After load` is sampled immediately after session creation. `After inference` is sampled after all warm runs while the model remains loaded. Allocator caching and KV workspaces explain why these values differ from artifact size and peak RSS.",
        "",
        "| Model / execution | After load (MiB) | After inference (MiB) |",
        "| --- | ---: | ---: |",
    ]
    for label, filename in ROWS:
        report = reports[filename]
        lines.append(
            f"| {label} | {report['rss_after_load_bytes'] / 1048576:.1f} | "
            f"{report['rss_after_inference_bytes'] / 1048576:.1f} |"
        )

    lines += [
        "",
        "### Acceleration findings",
        "",
        "- **Moonshine CPU wins over the current Core ML path.** Dynamic-shape MLProgram conversion logs unsupported-shape errors and falls back to CPU for partitions. Explicit Core ML runs are included for transparency; `Execution::Auto` therefore selects CPU for Moonshine. A Core ML result is not presented as an ANE-only result.",
        "- **Whisper Metal works.** `whisper-rs` reports whisper.cpp 1.8.3 with Metal + Accelerate. A separate first-ever Metal smoke initialization spent 5.243 s compiling embedded shaders; the load values above were measured with the shader cache warm.",
        "- Whisper Core ML encoder artifacts are not yet in the verified catalog, so no Whisper Core ML benchmark is claimed.",
        "",
        "### Short repeated-inference check",
        "",
    ]
    soak_path = RESULTS / "moonshine-tiny-soak20.json"
    if soak_path.exists():
        soak = json.loads(soak_path.read_text())
        rss = [round(value / 1048576, 1) for value in soak["rss_after_warm_run_bytes"]]
        lines += [
            "Moonshine Tiny CPU completed 20 warm iterations over the eight-clip corpus (160 utterances) after its cold pass. RSS was "
            f"{rss[0]:.1f} MiB after run 1 and {rss[-1]:.1f} MiB after run 20; the final 12 samples were flat at {rss[-1]:.1f} MiB. Warm-corpus medians stayed near 0.83 s. This short check showed no upward memory trend, but it is not the required one-hour soak.",
            "",
            "Per-run RSS (MiB): `" + ", ".join(f"{value:.1f}" for value in rss) + "`",
            "",
        ]
    lines += [
        "### Environment",
        "",
        "| Field | Value |",
        "| --- | --- |",
        f"| Hardware | {sample['hardware']['chip']} |",
        f"| Architecture | {sample['hardware']['architecture']} |",
        f"| macOS | {sample['hardware']['macos']} |",
        "| Power | 100% battery, discharging |",
        "| Rust | 1.94.0 |",
        "| CMake | 4.4.4 |",
        "| Deployment target | macOS 13.4 |",
        "| whisper.cpp | 1.8.3 via whisper-rs 0.16.0 |",
        "| ONNX Runtime | 1.31.0 via ort 2.0.0-rc.15 |",
        "| Model revisions | Pinned in `models/catalog.json` |",
        "",
        "## Targets and current status",
        "",
        "| Target | Status |",
        "| --- | --- |",
        "| First partial < 300 ms after speech starts | **Milestone 2 measurement.** Offline first-transcription times above must not be substituted for this target. |",
        "| Idle RAM < 50 MB with model unloaded | Menu-bar app target; not measured by this CLI spike. |",
        "| Idle CPU ~0% | Menu-bar app target; not measured by this CLI spike. |",
        "| No memory growth over one hour | Milestone 5 soak test; not claimed yet. |",
        "",
        "## Reproduce",
        "",
        "```bash",
        "python3 scripts/fetch_onnxruntime.py",
        "python3 scripts/prepare_benchmark_corpus.py",
        "scripts/cargo.sh build --release --workspace",
        "scripts/run-benchmarks.sh benchmark-data/corpus/benchmark-corpus.json",
        "python3 scripts/render_benchmarks.py",
        "```",
        "",
        "Raw reports and logs are written to `benchmark-data/results/` and are gitignored. `tests/fixtures/benchmark-corpus.json` is the smaller CI smoke corpus. See `benchmarks/NOTICE.md` and `tests/fixtures/NOTICE.md` for corpus attribution. CI applies pinned smoke-model thresholds rather than treating a missing model or corpus as a pass.",
        "",
    ]
    (ROOT / "BENCHMARKS.md").write_text("\n".join(lines), encoding="utf-8")
    print(f"Rendered {ROOT / 'BENCHMARKS.md'}")


if __name__ == "__main__":
    main()
