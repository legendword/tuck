#!/bin/bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
cd "$SCRIPT_DIR"

OUT_DIR="$SCRIPT_DIR/TuckApp/Generated"
UNIVERSAL_DIR="$SCRIPT_DIR/target/universal"

# Ensure both targets are installed
rustup target add aarch64-apple-darwin x86_64-apple-darwin 2>/dev/null || true

echo "==> Building tuck-ffi for aarch64-apple-darwin..."
cargo build --release --package tuck-ffi --target aarch64-apple-darwin

echo "==> Building tuck-ffi for x86_64-apple-darwin..."
cargo build --release --package tuck-ffi --target x86_64-apple-darwin

echo "==> Creating universal binary..."
mkdir -p "$UNIVERSAL_DIR"
lipo -create \
    target/aarch64-apple-darwin/release/libtuck_ffi.a \
    target/x86_64-apple-darwin/release/libtuck_ffi.a \
    -output "$UNIVERSAL_DIR/libtuck_ffi.a"

echo "==> Generating Swift bindings..."
mkdir -p "$OUT_DIR"
cargo run --release --package tuck-ffi --bin uniffi-bindgen -- \
    generate --library target/aarch64-apple-darwin/release/libtuck_ffi.a \
    --language swift \
    --out-dir "$OUT_DIR"

# Rename modulemap for Xcode compatibility
if [ -f "$OUT_DIR/tuck_ffiFFI.modulemap" ]; then
    mv "$OUT_DIR/tuck_ffiFFI.modulemap" "$OUT_DIR/module.modulemap"
fi

echo "==> Done!"
echo "  Static library: $UNIVERSAL_DIR/libtuck_ffi.a"
echo "  Swift bindings: $OUT_DIR/"
ls -la "$OUT_DIR/"
