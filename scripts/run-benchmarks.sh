#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
CORPUS="${1:-benchmark-data/corpus/benchmark-corpus.json}"
RUNS="${QUIL_BENCH_RUNS:-3}"
mkdir -p benchmark-data/results

run_one() {
    model=$1
    execution=$2
    suffix=$3
    echo "==> $model ($execution)"
    target/release/quil \
        --model-dir benchmark-data/models \
        --execution "$execution" \
        bench \
        --model "$model" \
        --corpus "$CORPUS" \
        --runs "$RUNS" \
        --output "benchmark-data/results/$suffix.json" \
        > "benchmark-data/results/$suffix.log" 2>&1
}

for model in moonshine-tiny moonshine-base; do
    run_one "$model" coreml "$model"
    run_one "$model" cpu "$model-cpu"
done
for model in distil-whisper-large-v3 whisper-large-v3-turbo; do
    run_one "$model" metal "$model"
done
