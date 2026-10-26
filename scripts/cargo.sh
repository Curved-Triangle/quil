#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
export ORT_LIB_PATH="${ORT_LIB_PATH:-$ROOT/.tools/onnxruntime/lib}"
if [ ! -f "$ORT_LIB_PATH/libonnxruntime.a" ]; then
    echo "ONNX Runtime is missing; run python3 scripts/fetch_onnxruntime.py" >&2
    exit 1
fi
if [ -d "$ROOT/.tools/venv/bin" ]; then
    PATH="$ROOT/.tools/venv/bin:$PATH"
    export PATH
fi
exec cargo "$@"
