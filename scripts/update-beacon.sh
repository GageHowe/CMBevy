#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

SERVICE_NAME="${SERVICE_NAME:-beacon}"

git pull --ff-only origin main
make beacon-r
sudo systemctl restart "$SERVICE_NAME"
sudo cp beacon/Caddyfile /etc/caddy/Caddyfile
sudo systemctl reload caddy

echo "Updated $SERVICE_NAME"
