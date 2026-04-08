#!/usr/bin/env bash
# Build and package CriticalMass for Linux (itch.io release)
# Run from repo root: ./scripts/build_release_linux.sh

set -euo pipefail

VERSION="${1:-0.1.0}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DIST_DIR="$REPO_ROOT/dist/linux"

echo "Building release binaries..."
cd "$REPO_ROOT"
make build-release

rm -rf "$DIST_DIR"
mkdir -p "$DIST_DIR/assets"

echo "Copying binaries..."
cp target/release/client     "$DIST_DIR/client"
cp target/release/gameserver "$DIST_DIR/gameserver"
chmod +x "$DIST_DIR/client" "$DIST_DIR/gameserver"

echo "Copying runtime libraries..."
for lib in libsteam_api.so libfmod.so.14 libfmodstudio.so.14; do
    if [ ! -f "target/release/$lib" ]; then
        echo "ERROR: target/release/$lib not found — was the client built?" >&2
        exit 1
    fi
    cp "target/release/$lib" "$DIST_DIR/$lib"
done

echo "Copying assets..."
cp -r assets/. "$DIST_DIR/assets/"

echo "Zipping..."
ZIP_PATH="$REPO_ROOT/dist/criticalmass-linux-$VERSION.zip"
rm -f "$ZIP_PATH"
cd "$DIST_DIR"
zip -r "$ZIP_PATH" .

echo "Done: $ZIP_PATH"
