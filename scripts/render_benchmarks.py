#!/usr/bin/env python3
"""Render BENCHMARKS.md from consistent, timestamped local JSON reports."""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ROWS = [
    ("Moonshine Tiny / Core ML + CPU fallback", "moonshine-tiny.json"),
    ("Moonshine Tiny / CPU", "moonshine-tiny-cpu.json"),
    ("Moonshine Base / Core ML + CPU fallback", "moonshine-base.json"),
    ("Moonshine Base / CPU", "moonshine-base-cpu.json"),
    ("Distil-Whisper large-v3 / Metal", "distil-whisper-large-v3.json"),
    ("Whisper large-v3-turbo / Metal", "whisper-large-v3-turbo.json"),
]
SHARED_KEYS = [
    "corpus",
    "clips",
    "audio_seconds",
    "warm_runs",
    "tool_versions",
]
COMMON_KEYS = [
    "corpus",
    "clips",
    "audio_seconds",
    "tool_versions",
]
HARDWARE_KEYS = ["chip", "architecture", "macos"]


def require(condition: bool, message: str) -> None:
    if not condition:
        raise RuntimeError(message)


def load_reports(results: Path, manifest: Path, catalog: Path) -> tuple[dict, dict, list]:
    reports = {filename: json.loads((results / filename).read_text()) for _, filename in ROWS}
    for label, filename in ROWS:
        report = reports[filename]
        require(report.get("schema_version") == 2, f"{filename}: expected schema 2; rerun the benchmark")
        require(report.get("recorded_unix", 0) > 0, f"{filename}: missing recording timestamp")
        require(report.get("tool_versions", {}).get("rustc") not in (None, "", "unknown"), f"{filename}: missing rustc version")
        require(report.get("tool_versions", {}).get("cmake") not in (None, "", "unknown"), f"{filename}: missing CMake version")
        require(
            report["warm_runs"] == len(report["warm_run_ms"]) == len(report["rss_after_warm_run_bytes"]),
            f"{filename}: inconsistent warm-run vectors",
        )
        model_id = report["model_id"]
        require(model_id in catalog, f"{filename}: {model_id} is absent from the model catalog")
        require(
            report["model_revision"] == catalog[model_id]["revision"],
            f"{filename}: model revision differs from models/catalog.json",
        )

    sample = reports[ROWS[0][1]]
    for label, filename in ROWS[1:]:
        report = reports[filename]
        for key in SHARED_KEYS:
            require(report[key] == sample[key], f"{filename}: {key} differs from the shared benchmark")
        for key in HARDWARE_KEYS:
            require(
                report["hardware"][key] == sample["hardware"][key],
                f"{filename}: hardware {key} differs from the shared benchmark",
            )
        require(
            report["wer_stats"]["reference_words"] == sample["wer_stats"]["reference_words"],
            f"{filename}: reference-word count differs from the shared benchmark",
        )

    corpus = json.loads(manifest.read_text())
    require(sample["corpus"] == corpus["name"], "report corpus name differs from the manifest")
    require(sample["clips"] == len(corpus["files"]), "report clip count differs from the manifest")
    return reports, corpus, [sample]


def error_count(report: dict) -> int:
    stats = report["wer_stats"]
    return stats["substitutions"] + stats["deletions"] + stats["insertions"]


def markdown_cell(value: object) -> str:
    return str(value).replace("|", "\\|").replace("\n", " ")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--corpus",
        default="benchmark-data/corpus/benchmark-corpus.json",
        help="corpus manifest used by every result (no stale/mixed corpora allowed)",
    )
    parser.add_argument("--results", default="benchmark-data/results")
    args = parser.parse_args()
    results = Path(args.results)
    catalog_data = json.loads((ROOT / "models" / "catalog.json").read_text())
    catalog = {model["id"]: model for model in catalog_data["models"]}
    reports, corpus, _ = load_reports(results, Path(args.corpus), catalog)
    sample = reports[ROWS[0][1]]
    recorded = datetime.fromtimestamp(
        max(report["recorded_unix"] for report in reports.values()), timezone.utc
    ).date()
    speakers = {Path(entry["audio"]).stem.split("-")[0] for entry in corpus["files"]}
    lines = [
        "# Benchmarks",
        "",
        "## Milestone 1 measured results",
        "",
        f"Measurements were taken locally through **{recorded}** from release binaries. They are not vendor-published numbers. The default is **Moonshine Tiny with the ONNX Runtime CPU provider** because latency and memory are the top-priority constraints; Moonshine Base is the balanced accuracy option and Whisper large-v3-turbo is the accuracy option.",
        "",
        "### Method",
        "",
        f"- Corpus: `{sample['corpus']}`, {sample['clips']} utterances from {len(speakers)} speakers, **{sample['audio_seconds']:.3f} seconds / {sample['wer_stats']['reference_words']} reference words**.",
        "- Audio: identical 16 kHz mono signed 16-bit PCM converted from the source FLAC; no Quil microphone recordings.",
        f"- Runs: one cold transcription followed by {sample['warm_runs']} warm corpus transcriptions per configuration; each configuration ran in a fresh process.",
        "- `First`: time to transcribe the first corpus clip immediately after model load. **This is not a streaming-first-partial measurement.**",
        "- `Warm`: median total time for all clips across the warm runs.",
        "- `RTF`: warm median / total audio duration; lower is better.",
        "- `Peak RSS`: process lifetime peak including load, corpus, model, and allocator overhead; it is not idle RAM.",
        "- `WER`: aggregate edit errors / reference words after identical lowercase/punctuation normalization. Small clean corpora are not statistically representative of all dictation.",
        "- Renderer validation: all reports must share corpus metadata, hardware and toolchain metadata, run counts, and catalog revisions; otherwise rendering fails instead of publishing mixed claims. Power state is recorded but may vary between runs.",
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

    turbo = reports["whisper-large-v3-turbo.json"]
    distil = reports["distil-whisper-large-v3.json"]
    lines += [
        "",
        f"Whisper large-v3-turbo had {error_count(turbo)} total edit operations on this particular {sample['wer_stats']['reference_words']}-word corpus; that does not imply the same WER on general speech. Distil-Whisper had {error_count(distil)} total edit operations in its run.",
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
    ]

    soak_path = results / "moonshine-tiny-soak20.json"
    if soak_path.exists():
        soak = json.loads(soak_path.read_text())
        require(soak.get("schema_version") == 2, "soak report is stale; rerun it")
        for key in COMMON_KEYS:
            require(soak[key] == sample[key], f"soak report: {key} differs from the main benchmark")
        for key in HARDWARE_KEYS:
            require(
                soak["hardware"][key] == sample["hardware"][key],
                f"soak report: hardware {key} differs from the main benchmark",
            )
        require(soak["model_id"] == "moonshine-tiny", "unexpected soak model")
        require(
            soak["model_revision"] == catalog["moonshine-tiny"]["revision"],
            "soak report: model revision differs from models/catalog.json",
        )
        rss_raw = soak["rss_after_warm_run_bytes"]
        flat_count = 1
        for value in reversed(rss_raw[:-1]):
            if value != rss_raw[-1]:
                break
            flat_count += 1
        sample_label = "sample was" if flat_count == 1 else "samples were"
        lines += [
            "### Short repeated-inference check",
            "",
            f"Moonshine Tiny CPU completed {soak['warm_runs']} warm iterations over the {soak['clips']}-clip corpus ({soak['clips'] * soak['warm_runs']} utterances) after its cold pass. RSS was "
            f"{rss_raw[0] / 1048576:.1f} MiB after run 1 and {rss_raw[-1] / 1048576:.1f} MiB after run {len(rss_raw)}; the final {flat_count} {sample_label} flat at {rss_raw[-1] / 1048576:.1f} MiB. Median warm-corpus time was {soak['median_warm_run_ms'] / 1000:.2f} s. This short check showed no upward memory trend, but it is not the required one-hour soak.",
            "",
            "Per-run RSS (MiB): `" + ", ".join(f"{value / 1048576:.1f}" for value in rss_raw) + "`",
            "",
        ]

    lines += [
        "### Environment",
        "",
        "| Field | Value |",
        "| --- | --- |",
        f"| Hardware | {markdown_cell(sample['hardware']['chip'])} |",
        f"| Architecture | {markdown_cell(sample['hardware']['architecture'])} |",
        f"| macOS | {markdown_cell(sample['hardware']['macos'])} |",
        f"| Power | {markdown_cell(sample['hardware']['power'])} |",
        f"| Rust | {markdown_cell(sample['tool_versions']['rustc'])} |",
        f"| CMake | {markdown_cell(sample['tool_versions']['cmake'])} |",
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
        "python3 scripts/render_benchmarks.py --corpus benchmark-data/corpus/benchmark-corpus.json",
        "```",
        "",
        "Raw reports and logs are written to `benchmark-data/results/` and are gitignored. `tests/fixtures/benchmark-corpus.json` is the smaller CI smoke corpus. See `benchmarks/NOTICE.md` and `tests/fixtures/NOTICE.md` for corpus attribution. CI applies pinned smoke-model thresholds rather than treating a missing model or corpus as a pass.",
        "",
    ]
    (ROOT / "BENCHMARKS.md").write_text("\n".join(lines), encoding="utf-8")
    print(f"Rendered {ROOT / 'BENCHMARKS.md'}")


if __name__ == "__main__":
    main()
