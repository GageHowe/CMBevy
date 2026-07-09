#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

dist="dist"
rm -rf "$dist"

key="$(openssl rand -hex 32)"
cargo run -p pack_assets --release -- --key "$key" --dist "$dist"
CM_ASSET_KEY="$key" cargo build -p client --release
cargo build -p gameserver --release

cp target/release/client "$dist/client"
cp target/release/gameserver "$dist/gameserver"
chmod +x "$dist/client" "$dist/gameserver"
for lib in "$@"; do
    cp "target/release/$lib" "$dist/"
done

echo "packaged $dist"
