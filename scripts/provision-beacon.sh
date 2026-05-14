#!/usr/bin/env bash
set -euo pipefail

# ssh:
# ssh root@66.228.55.104
# or
# ssh -t GageHowe@lish-us-central.linode.com beacon-us-central

sudo apt update
sudo apt install gh rustup make build-essential caddy -y

gh auth login
git clone --filter=blob:none --no-checkout https://github.com/GageHowe/CMBevy && cd CMBevy && git sparse-checkout init --cone && git sparse-checkout set beacon crates client gameserver tools scripts && git checkout main

make beacon-release
sudo install -d -m 755 /var/lib/beacon/asset_blobs
sudo cp scripts/beacon.service /etc/systemd/system/beacon.service
sudo cp beacon/Caddyfile /etc/caddy/Caddyfile
sudo systemctl daemon-reload
sudo systemctl enable beacon --now
sudo systemctl restart caddy

# confirmed working 5/14/26
