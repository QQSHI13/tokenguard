#!/usr/bin/env bash
# Install the Token Guard CLI from GitHub Releases.
# Usage: curl -fsSL https://raw.githubusercontent.com/QQSHI13/tokenguard/main/scripts/install-cli.sh | bash
#        curl ... | bash -s -- --version v0.1.8 --dest ~/.local/bin

set -euo pipefail

REPO="QQSHI13/tokenguard"
DEFAULT_DEST="${HOME}/.local/bin"
VERSION=""
DEST=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="${2:-}"
      shift 2
      ;;
    --dest)
      DEST="${2:-}"
      shift 2
      ;;
    -h|--help)
      echo "Usage: $0 [--version VERSION] [--dest DIR]"
      echo "  --version  Release tag to install (default: latest)"
      echo "  --dest     Install directory (default: ~/.local/bin)"
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

DEST="${DEST:-$DEFAULT_DEST}"

# Detect OS.
case "$(uname -s)" in
  Linux*)     OS=linux;;
  Darwin*)    OS=macos;;
  MINGW*|MSYS*|CYGWIN*) OS=windows;;
  *)
    echo "Unsupported OS: $(uname -s)" >&2
    exit 1
    ;;
esac

# Detect architecture.
ARCH="$(uname -m)"
case "$ARCH" in
  x86_64|amd64)  ARCH_SUFFIX=x86_64;;
  arm64|aarch64) ARCH_SUFFIX=aarch64;;
  *)
    echo "Unsupported architecture: $ARCH" >&2
    exit 1
    ;;
esac

if [[ "$OS" == "windows" ]]; then
  ASSET="tokenguard-windows-${ARCH_SUFFIX}.exe"
  BINARY="tokenguard.exe"
else
  ASSET="tokenguard-${OS}-${ARCH_SUFFIX}"
  BINARY="tokenguard"
fi

if [[ -z "$VERSION" ]]; then
  VERSION=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
  if [[ -z "$VERSION" ]]; then
    echo "Could not determine latest release" >&2
    exit 1
  fi
fi

URL="https://github.com/${REPO}/releases/download/${VERSION}/${ASSET}"

echo "Installing Token Guard CLI ${VERSION} for ${OS}/${ARCH_SUFFIX}..."
echo "  ${URL}"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

curl -fsSL "$URL" -o "${TMP_DIR}/${BINARY}"
chmod +x "${TMP_DIR}/${BINARY}"

mkdir -p "$DEST"
cp "${TMP_DIR}/${BINARY}" "${DEST}/${BINARY}"

echo "Installed to ${DEST}/${BINARY}"

if [[ ":${PATH}:" != *":${DEST}:"* ]]; then
  echo "Warning: ${DEST} is not on your PATH. Add it to your shell profile:"
  echo "  export PATH=\"${DEST}:\${PATH}\""
fi
