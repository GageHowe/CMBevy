#!/usr/bin/env bash

# ssh:
# ssh root@66.228.55.104
# or
# ssh -t GageHowe@lish-us-central.linode.com beacon-us-central

# a log of commands I ran to get the beacon up and running

sudo apt update
apt upgrade
apt install gh rustup make build-essential certbot openssl -y

DOMAIN="${1:-}"
EMAIL="${2:-}"

gh auth login
git clone --filter=blob:none --no-checkout https://github.com/GageHowe/CMBevy
cd CMBevy
git sparse-checkout init --cone

# cargo still loads the root workspace, so all declared workspace member paths
# need to exist even when we only run beacon.
git sparse-checkout set beacon crates client gameserver tools
git checkout main

make beacon-release
if [ -n "$DOMAIN" ] && [ -n "$EMAIL" ]; then
    sudo certbot certonly --standalone -d "$DOMAIN" --non-interactive --agree-tos -m "$EMAIL"
    ln -sf "/etc/letsencrypt/live/$DOMAIN/fullchain.pem" build/fullchain.pem
    ln -sf "/etc/letsencrypt/live/$DOMAIN/privkey.pem" build/privkey.pem
else
    openssl req -x509 -nodes -newkey rsa:2048 \
        -keyout build/privkey.pem \
        -out build/fullchain.pem \
        -days 365 \
        -subj "/CN=$(hostname)"
fi

./build/beacon
