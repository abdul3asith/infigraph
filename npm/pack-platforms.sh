#!/bin/bash
# Pack platform-specific npm packages from the local bin/ directory.
# Usage: ./pack-platforms.sh
# Assumes bin/ contains: infigraph, infigraph-mcp, infigraph-driver.jar, grammars/, models/
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
BIN_DIR="$SCRIPT_DIR/bin"
PLATFORMS_DIR="$SCRIPT_DIR/platforms"

if [ ! -f "$BIN_DIR/infigraph" ] && [ ! -f "$BIN_DIR/infigraph.exe" ]; then
  echo "No binaries in $BIN_DIR — nothing to pack"
  exit 1
fi

# Detect platform from binary, not from shell (Rosetta reports x86_64 for arm64 binaries)
OS="$(uname -s)"
if [ -f "$BIN_DIR/infigraph.exe" ]; then
  PLATFORM="win32-x64"
elif [ "$OS" = "Darwin" ]; then
  BINARY_ARCH="$(file "$BIN_DIR/infigraph" | grep -o 'arm64\|x86_64')"
  case "$BINARY_ARCH" in
    arm64)   PLATFORM="darwin-arm64" ;;
    x86_64)  PLATFORM="darwin-x64" ;;
    *)       echo "Unknown binary arch: $BINARY_ARCH"; exit 1 ;;
  esac
elif [ "$OS" = "Linux" ]; then
  PLATFORM="linux-x64"
else
  echo "Unknown OS: $OS"; exit 1
fi

echo "Packing platform package for $PLATFORM..."

DEST="$PLATFORMS_DIR/$PLATFORM/bin"
rm -rf "$DEST"
mkdir -p "$DEST"

# Copy binaries and shared assets
cp "$BIN_DIR/infigraph"* "$DEST/" 2>/dev/null || true
cp "$BIN_DIR/infigraph-mcp"* "$DEST/" 2>/dev/null || true
[ -f "$BIN_DIR/infigraph-driver.jar" ] && cp "$BIN_DIR/infigraph-driver.jar" "$DEST/"
[ -d "$BIN_DIR/grammars" ] && cp -r "$BIN_DIR/grammars" "$DEST/"
[ -d "$BIN_DIR/models" ] && cp -r "$BIN_DIR/models" "$DEST/"

# Don't copy JS stubs into platform package
rm -f "$DEST/infigraph.js" "$DEST/infigraph-mcp.js"

# Pack
cd "$PLATFORMS_DIR/$PLATFORM"
npm pack
echo "Platform package: $PLATFORMS_DIR/$PLATFORM/$(ls *.tgz)"

# Also pack main package
cd "$SCRIPT_DIR"
npm pack
echo "Main package: $SCRIPT_DIR/$(ls *.tgz | head -1)"
