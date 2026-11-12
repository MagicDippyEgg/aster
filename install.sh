#!/bin/sh
set -e

# Aster Package Manager Installation Script
# Usage: curl -fsSL https://raw.githubusercontent.com/MagicDippyEgg/aster/refs/heads/main/install.sh | bash

ASTER_HOME="${ASTER_HOME:-$HOME/.bin/aster}"
mkdir -p "$ASTER_HOME"

echo "Installing Aster Package Manager..."

TAG=""
if command -v curl >/dev/null 2>&1; then
    TAG=$(curl -sSL https://api.github.com/repos/MagicDippyEgg/aster/releases/latest 2>/dev/null | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)
elif command -v wget >/dev/null 2>&1; then
    TAG=$(wget -qO- https://api.github.com/repos/MagicDippyEgg/aster/releases/latest 2>/dev/null | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/' || true)
fi

if [ -z "$TAG" ]; then
    TAG="0.2.7"
fi

DOWNLOAD_URL="https://github.com/MagicDippyEgg/aster/releases/download/${TAG}/aster"
TMP_BIN="$(mktemp)"

cleanup() {
    rm -f "$TMP_BIN"
}
trap cleanup EXIT INT TERM

echo "Downloading Aster ${TAG}..."

if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$DOWNLOAD_URL" -o "$TMP_BIN"
elif command -v wget >/dev/null 2>&1; then
    wget -qO "$TMP_BIN" "$DOWNLOAD_URL"
else
    echo "Error: curl or wget is required to install Aster." >&2
    exit 1
fi

chmod +x "$TMP_BIN"
mv "$TMP_BIN" "$ASTER_HOME/aster"

echo "Successfully installed Aster (${TAG}) to ${ASTER_HOME}/aster"

NEEDS_ASTER_HOME=
NEEDS_BIN=
case ":$PATH:" in
    *":$ASTER_HOME:"*) ;;
    *) NEEDS_ASTER_HOME=1 ;;
esac
case ":$PATH:" in
    *":$ASTER_HOME/bin:"*) ;;
    *) NEEDS_BIN=1 ;;
esac

if [ -n "$NEEDS_ASTER_HOME" ] || [ -n "$NEEDS_BIN" ]; then
    echo ""
    echo "Note: To run 'aster' and the packages it installs from anywhere, add to your PATH:"
    echo "  export PATH=\"\$PATH:$ASTER_HOME:$ASTER_HOME/bin\""
    echo ""
    echo "You can add this line to your shell configuration file (e.g. ~/.bashrc or ~/.zshrc)."
fi
