#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

SERVICE_NAME="${SERVICE_NAME:-beacon}"

git pull --ff-only origin main
make beacon-release
sudo systemctl restart "$SERVICE_NAME"

echo "Updated $SERVICE_NAME"
