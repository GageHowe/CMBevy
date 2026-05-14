#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

PID_FILE="build/beacon.pid"
LOG_FILE="build/beacon.log"

if [ -f "$PID_FILE" ]; then
    pid="$(cat "$PID_FILE")"
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid"
        while kill -0 "$pid" 2>/dev/null; do
            sleep 1
        done
    fi
    rm -f "$PID_FILE"
fi

git pull --ff-only origin main
make beacon-release

mkdir -p build
nohup ./build/beacon >"$LOG_FILE" 2>&1 &
echo $! >"$PID_FILE"

echo "Beacon restarted with pid $(cat "$PID_FILE")"
echo "Log: $LOG_FILE"
