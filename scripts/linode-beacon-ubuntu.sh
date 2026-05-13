#!/usr/bin/env bash

# ssh:
# ssh root@66.228.55.104
# or
# ssh -t GageHowe@lish-us-central.linode.com beacon-us-central

# a log of commands I ran to get the beacon up and running

sudo apt update
sudo apt upgrade
sudo apt install gh rustup make -y

gh auth login
git clone --filter=blob:none --no-checkout https://github.com/GageHowe/CMBevy
cd CMBevy
git sparse-checkout init --cone
git sparse-checkout set beacon crates/http_common Cargo.toml Cargo.lock rust-toolchain.toml
git checkout main

cargo run --release -p beacon
