#!/bin/bash
set -e

# Network Latency Test Script - Tests server/client with tc delay

# Defaults
DELAY_MS=50
JITTER_MS=10
PACKET_LOSS=0
SERVER_PORT=42070
LAN_DISCOVERY_PORT=42071
SERVER_IP="127.0.0.1"
NET_INTERFACE="lo"
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVER_PID=""
CLIENT_PID=""
EMULATOR_PID=""
_cleaned_up=""

usage() {
    cat << EOF
Usage: $0 [OPTIONS]

OPTIONS:
    -d, --delay MS     One-way delay in ms (default: $DELAY_MS)
    -j, --jitter MS    Jitter variation in ms (default: $JITTER_MS)
    -l, --loss PERCENT Packet loss percentage (default: $PACKET_LOSS)
    -p, --port PORT    Server port (default: $SERVER_PORT)
    -c, --cleanup      Only cleanup tc rules
    -h, --help         Show this help

EXAMPLES:
    $0 -d 100 -j 20
    $0 -d 500 -l 5
    $0 -c  # cleanup only

EOF
    exit 0
}

while [[ $# -gt 0 ]]; do
    case $1 in
        -d|--delay) DELAY_MS="$2"; shift 2 ;;
        -j|--jitter) JITTER_MS="$2"; shift 2 ;;
        -l|--loss) PACKET_LOSS="$2"; shift 2 ;;
        -p|--port) SERVER_PORT="$2"; shift 2 ;;
        -c|--cleanup) CLEANUP_ONLY=true; shift ;;
        -h|--help) usage ;;
        *) echo "Unknown: $1"; exit 1 ;;
    esac
done

cleanup() {
    [[ -n "$_cleaned_up" ]] && return
    _cleaned_up=1
    if [[ -n "$CLIENT_PID" ]] && kill -0 "$CLIENT_PID" 2>/dev/null; then
        kill "$CLIENT_PID" 2>/dev/null || true
        wait "$CLIENT_PID" 2>/dev/null || true
    fi
    if [[ -n "$EMULATOR_PID" ]] && kill -0 "$EMULATOR_PID" 2>/dev/null; then
        kill "$EMULATOR_PID" 2>/dev/null || true
        wait "$EMULATOR_PID" 2>/dev/null || true
    fi
    if [[ -n "$SERVER_PID" ]] && kill -0 "$SERVER_PID" 2>/dev/null; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    sudo tc qdisc del dev $NET_INTERFACE root 2>/dev/null || true
}

handle_interrupt() {
    cleanup
    exit 130
}

trap cleanup EXIT
trap handle_interrupt INT TERM

port_pids() {
    local proto="$1"
    local port="$2"
    if command -v fuser >/dev/null 2>&1; then
        fuser -n "$proto" "$port" 2>/dev/null | tr ' ' '\n' | sed '/^$/d' | sort -u
        return
    fi
    if command -v lsof >/dev/null 2>&1; then
        if [[ "$proto" == "tcp" ]]; then
            lsof -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null | sort -u
        else
            lsof -tiUDP:"$port" 2>/dev/null | sort -u
        fi
        return
    fi
    ss -H -lpn "$proto" "sport = :$port" 2>/dev/null | sed -n 's/.*pid=\([0-9]\+\).*/\1/p' | sort -u
}

kill_port_users() {
    local port="$1"
    local pids
    pids="$({
        port_pids tcp "$port"
        port_pids udp "$port"
    } | sort -u)"
    [[ -z "$pids" ]] && return
    echo "Killing processes on port $port: $pids"
    while read -r pid; do
        [[ -z "$pid" ]] && continue
        kill "$pid" 2>/dev/null || true
    done <<< "$pids"
}

if [[ "$CLEANUP_ONLY" == "true" ]]; then
    cleanup && echo "Cleaned up." && exit 0
fi

echo "Delay: ${DELAY_MS}ms ±${JITTER_MS}ms, Loss: ${PACKET_LOSS}%, Server: ${SERVER_IP}:${SERVER_PORT}"

# Egress delay on loopback (affects outgoing packets)
sudo tc qdisc del dev $NET_INTERFACE root 2>/dev/null || true
sudo tc qdisc add dev $NET_INTERFACE root netem delay ${DELAY_MS}ms ${JITTER_MS}ms ${PACKET_LOSS:+loss ${PACKET_LOSS}%}

echo "tc qdisc show:"
sudo tc qdisc show dev $NET_INTERFACE

kill_port_users "$SERVER_PORT"
kill_port_users "$LAN_DISCOVERY_PORT"

# Start server and client
(cd "$PROJECT_ROOT" && cargo run -p gameserver -- --port ${SERVER_PORT}) &
SERVER_PID=$!
sleep 2
(cd "$PROJECT_ROOT" && cargo run -p client -- --server ${SERVER_IP}:${SERVER_PORT}) &
CLIENT_PID=$!
wait "$CLIENT_PID"

echo "Done. Effective RTT: ~$((DELAY_MS * 2))-$(((DELAY_MS + JITTER_MS) * 2))ms"
