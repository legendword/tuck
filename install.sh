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
  curl --proto '=https' --proto-redir '=https' --tlsv1.2 -SL "$@"
}

echo "Downloading latest tuck release..."
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT
BINARY="${WORKDIR}/${ASSET_NAME}"
CHECKSUM="${WORKDIR}/${ASSET_NAME}.sha256"

fetch -f "${BASE_URL}/${ASSET_NAME}" -o "$BINARY"

# Verify the download against the SHA-256 published with the release.
# Releases made before checksums were published do not have one (HTTP 404);
# only then is the check skipped, with a warning. Any other failure to get
# the checksum stops the install.
if ! status=$(fetch -s -o "$CHECKSUM" -w '%{http_code}' "${BASE_URL}/${ASSET_NAME}.sha256"); then
  echo "Error: could not download the release checksum; not installing." >&2
  exit 1
fi
case "$status" in
  200)
    expected=$(awk 'NR == 1 { print $1 }' "$CHECKSUM")
    actual=$(shasum -a 256 "$BINARY" | awk '{ print $1 }')
    if [ "${#expected}" -ne 64 ] || [ "$expected" != "$actual" ]; then
      echo "Error: checksum verification failed; not installing." >&2
      exit 1
    fi
    echo "Checksum verified."
    ;;
  404)
    echo "Warning: this release does not publish a checksum; skipping verification." >&2
    ;;
  *)
    echo "Error: unexpected response (HTTP ${status}) for the release checksum; not installing." >&2
    exit 1
    ;;
esac

xattr -d com.apple.quarantine "$BINARY" 2>/dev/null || true

# `install` sets the mode explicitly, and with sudo makes the binary
# root-owned, so the copy in a system directory is not left writable by the
# account that ran the installer.
if [ -w "$INSTALL_DIR" ]; then
  install -m 755 "$BINARY" "${INSTALL_DIR}/${BINARY_NAME}"
else
  echo "Need sudo to install to ${INSTALL_DIR}"
  sudo install -m 755 -o root -g wheel "$BINARY" "${INSTALL_DIR}/${BINARY_NAME}"
fi

echo "Installed: $(tuck --version)"
