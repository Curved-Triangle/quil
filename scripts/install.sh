#!/bin/sh
set -eu

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cd "$ROOT"
if [ "$(uname -s)" != "Darwin" ] || [ "$(uname -m)" != "arm64" ]; then
    echo "Quil currently supports only Apple Silicon macOS." >&2
    exit 1
fi

python3 scripts/fetch_onnxruntime.py
scripts/cargo.sh build --release --workspace

BIN_DIR="${QUIL_INSTALL_DIR:-$HOME/.local/bin}"
ASSET_DIR="$HOME/.local/share/quil/assets"
mkdir -p "$BIN_DIR" "$ASSET_DIR"

# Never install through a pre-existing symlink: `install` would follow it and
# clobber the symlink's target with a binary marked executable.
for target in "$BIN_DIR/quil" "$BIN_DIR/quil-model-fetch" \
    "$ASSET_DIR/chime-start.wav" "$ASSET_DIR/chime-stop.wav"; do
    if [ -L "$target" ]; then
        echo "Refusing to install over symlink: $target" >&2
        exit 1
    fi
done

install -m 755 target/release/quil "$BIN_DIR/quil"
install -m 755 target/release/quil-model-fetch "$BIN_DIR/quil-model-fetch"
install -m 644 assets/chime-start.wav assets/chime-stop.wav "$ASSET_DIR/"

printf '\nInstalled %s/quil and %s/quil-model-fetch\n' "$BIN_DIR" "$BIN_DIR"
printf 'Next: %s/quil-model-fetch download moonshine-tiny\n' "$BIN_DIR"
printf 'Then: %s/quil dictate\n' "$BIN_DIR"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) printf 'Add this to your shell PATH: export PATH="%s:$PATH"\n' "$BIN_DIR" ;;
esac
