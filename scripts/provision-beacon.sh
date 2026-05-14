#!/usr/bin/env bash
set -euo pipefail

# ssh root@66.228.55.104
# ssh -t GageHowe@lish-us-central.linode.com beacon-us-central

apt update
apt install gh rustup make build-essential caddy -y

gh auth login
git clone --filter=blob:none --no-checkout https://github.com/GageHowe/CMBevy && cd CMBevy && git sparse-checkout init --cone && git sparse-checkout set beacon crates client gameserver tools scripts && git checkout main

make beacon-release
install -d -m 755 /var/lib/beacon/asset_blobs
cp scripts/beacon.service /etc/systemd/system/beacon.service
cp beacon/Caddyfile /etc/caddy/Caddyfile
systemctl daemon-reload
systemctl enable beacon --now
systemctl restart caddy

# confirmed working 5/14/26
