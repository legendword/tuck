#!/bin/sh
set -eu

REPO="legendword/tuck"
INSTALL_DIR="/usr/local/bin"
BINARY_NAME="tuck"
ASSET_NAME="tuck-macos-universal"
BASE_URL="https://github.com/${REPO}/releases/latest/download"

if [ "$(uname -s)" != "Darwin" ]; then
  echo "Error: tuck is macOS only." >&2
  exit 1
fi

# HTTPS only, including on redirects.
fetch() {
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 -fSL "$@"
}

echo "Downloading latest tuck release..."
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT

fetch "${BASE_URL}/${ASSET_NAME}" -o "${WORKDIR}/${ASSET_NAME}"

# Verify the download against the SHA-256 published with the release.
# Releases made before checksums were published do not have one; for those
# the check is skipped with a warning rather than blocking the install.
if fetch -s "${BASE_URL}/${ASSET_NAME}.sha256" -o "${WORKDIR}/${ASSET_NAME}.sha256" 2>/dev/null; then
  if ! (cd "$WORKDIR" && shasum -a 256 -c "${ASSET_NAME}.sha256" >/dev/null); then
    echo "Error: checksum verification failed; not installing." >&2
    exit 1
  fi
  echo "Checksum verified."
else
  echo "Warning: this release does not publish a checksum; skipping verification." >&2
fi

xattr -d com.apple.quarantine "${WORKDIR}/${ASSET_NAME}" 2>/dev/null || true

# `install` sets the mode explicitly, and with sudo makes the binary
# root-owned, so the copy in a system directory is not left writable by the
# account that ran the installer.
if [ -w "$INSTALL_DIR" ]; then
  install -m 755 "${WORKDIR}/${ASSET_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
else
  echo "Need sudo to install to ${INSTALL_DIR}"
  sudo install -m 755 -o root -g wheel "${WORKDIR}/${ASSET_NAME}" "${INSTALL_DIR}/${BINARY_NAME}"
fi

echo "Installed: $(tuck --version)"
