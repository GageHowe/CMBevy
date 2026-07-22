#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."

docker build -f gameserver/Dockerfile -t cmbevy-gameserver:testing .
docker rm -f cmbevy-gameserver >/dev/null 2>&1 || true
docker run -d \
  --name cmbevy-gameserver \
  --restart unless-stopped \
  -p 42070:42070/udp \
  cmbevy-gameserver:testing \
  --port 42070 \
  --map maps/_default.ron \
  --gametype /app/assets/gametypes/ctf.lua \
  "$@"
docker ps --filter name=cmbevy-gameserver
docker logs --tail 30 cmbevy-gameserver
