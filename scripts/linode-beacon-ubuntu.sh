#!/usr/bin/env bash

# ssh:
# ssh root@66.228.55.104
# or
# ssh -t GageHowe@lish-us-central.linode.com beacon-us-central

# a log of commands I ran to get the beacon up and running

sudo apt update
sudo apt install gh rustup make build-essential caddy -y

gh auth login
git clone --filter=blob:none --no-checkout https://github.com/GageHowe/CMBevy
cd CMBevy
git sparse-checkout init --cone

# cargo still loads the root workspace, so all declared workspace member paths
# need to exist even when we only run beacon.
git sparse-checkout set beacon crates client gameserver tools
git checkout main

make beacon-release
sudo cp beacon/Caddyfile /etc/caddy/Caddyfile
sudo systemctl reload caddy
./build/beacon
