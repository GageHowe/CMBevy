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

echo "Copying libsteam_api.so from build output..."
STEAM_SO="$(find target/release/build -name "libsteam_api.so" | head -1)"
if [ -n "$STEAM_SO" ]; then
    cp "$STEAM_SO" "$DIST_DIR/libsteam_api.so"
else
    echo "ERROR: libsteam_api.so not found in build output — was the client built?" >&2
    exit 1
fi

echo "Copying assets (excluding blender sources)..."
for dir in assets/*/; do
    name="$(basename "$dir")"
    if [ "$name" != "blender" ]; then
        cp -r "$dir" "$DIST_DIR/assets/$name"
    fi
done

echo "Zipping..."
ZIP_PATH="$REPO_ROOT/dist/criticalmass-linux-$VERSION.zip"
rm -f "$ZIP_PATH"
cd "$DIST_DIR"
zip -r "$ZIP_PATH" .

echo "Done: $ZIP_PATH"
