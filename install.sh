#!/bin/sh
set -eu

REPO="legendword/tuck"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="tuck"
ASSET_NAME="tuck-macos-universal"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "Error: tuck is macOS only." >&2
  exit 1
fi

echo "Downloading latest tuck release..."
TMPFILE=$(mktemp)
trap 'rm -f "$TMPFILE"' EXIT

curl -fSL "https://github.com/${REPO}/releases/latest/download/${ASSET_NAME}" -o "$TMPFILE"
chmod +x "$TMPFILE"
xattr -d com.apple.quarantine "$TMPFILE" 2>/dev/null || true

if [ -w "$INSTALL_DIR" ]; then
  mv "$TMPFILE" "${INSTALL_DIR}/${BINARY_NAME}"
else
  echo "Need sudo to install to ${INSTALL_DIR}"
  sudo mv "$TMPFILE" "${INSTALL_DIR}/${BINARY_NAME}"
fi

echo "Installed: $(tuck --version)"
