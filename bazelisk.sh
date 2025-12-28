#!/bin/bash
# Downloads and caches bazelisk, then executes it with any arguments passed

set -euo pipefail

BAZELISK_VERSION="v1.25.0"
CACHE_DIR="${XDG_CACHE_HOME:-$HOME/.cache}/protocrap"
BAZELISK_BIN="$CACHE_DIR/bazelisk-$BAZELISK_VERSION"

# Detect platform
OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"

case "$OS" in
    linux)  OS_NAME="linux" ;;
    darwin) OS_NAME="darwin" ;;
    *)      echo "Unsupported OS: $OS" >&2; exit 1 ;;
esac

case "$ARCH" in
    x86_64)  ARCH_NAME="amd64" ;;
    aarch64) ARCH_NAME="arm64" ;;
    arm64)   ARCH_NAME="arm64" ;;
    *)       echo "Unsupported architecture: $ARCH" >&2; exit 1 ;;
esac

# Download if not cached
if [ ! -x "$BAZELISK_BIN" ]; then
    mkdir -p "$CACHE_DIR"
    URL="https://github.com/bazelbuild/bazelisk/releases/download/$BAZELISK_VERSION/bazelisk-$OS_NAME-$ARCH_NAME"
    echo "Downloading bazelisk $BAZELISK_VERSION for $OS_NAME-$ARCH_NAME..." >&2
    curl -fsSL "$URL" -o "$BAZELISK_BIN"
    chmod +x "$BAZELISK_BIN"
fi

exec "$BAZELISK_BIN" "$@"
