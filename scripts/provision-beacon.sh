#!/usr/bin/env bash

# ssh:
# ssh root@66.228.55.104
# or
# ssh -t GageHowe@lish-us-central.linode.com beacon-us-central

# a log of commands I ran to get the beacon up and running

sudo apt update
sudo apt install gh rustup make build-essential caddy -y

gh auth login
git clone --filter=blob:none --no-checkout https://github.com/GageHowe/CMBevy && cd CMBevy && git sparse-checkout init --cone && git sparse-checkout set beacon crates client gameserver tools && git checkout main

# make and run through caddy
sudo fuser -k 80/tcp 443/tcp 8000/tcp
make beacon-release && sudo cp beacon/Caddyfile /etc/caddy/Caddyfile && sudo systemctl restart caddy && ./build/beacon
