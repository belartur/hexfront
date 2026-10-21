#!/usr/bin/env bash
# Build the browser version of Hexfront into dist_wasm/.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TARGET="wasm32-unknown-unknown"
OUT="$ROOT/dist_wasm"
WASM="$ROOT/rust/target/$TARGET/release/hexfront.wasm"

if ! command -v cargo >/dev/null 2>&1; then
    echo "error: cargo is not installed" >&2
    exit 1
fi

if command -v rustup >/dev/null 2>&1; then
    if ! rustup target list --installed | grep -qx "$TARGET"; then
        echo "installing Rust target $TARGET"
        rustup target add "$TARGET"
    fi
fi

echo "building release wasm"
(
    cd "$ROOT/rust"
    cargo build --release --locked --target "$TARGET"
)

if [[ ! -f "$WASM" ]]; then
    echo "error: build did not produce $WASM" >&2
    exit 1
fi

rm -rf "$OUT"
mkdir -p "$OUT/maps" "$OUT/sounds"
cp "$WASM" "$OUT/hexfront.wasm"
cp "$ROOT/rust/wasm/index.html" "$OUT/index.html"
cp "$ROOT/rust/wasm/mq_js_bundle.js" "$OUT/mq_js_bundle.js"
cp "$ROOT"/maps/*.map "$OUT/maps/"
cp "$ROOT"/sounds/*.ogg "$OUT/sounds/"

echo
echo "built $OUT"
du -sh "$OUT"
echo
echo "serve it over HTTP (file:// does not work because of XHR):"
echo "  python3 -m http.server 8080 --directory \"$OUT\""
echo "then open http://localhost:8080/"
