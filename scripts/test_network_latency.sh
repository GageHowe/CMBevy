#!/bin/bash
set -e

# Network Latency Test Script - Tests server/client with tc delay

# Defaults
DELAY_MS=50
JITTER_MS=10
PACKET_LOSS=0
SERVER_PORT=42069
SERVER_IP="127.0.0.1"
NET_INTERFACE="lo"
PROJECT_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

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
    sudo tc qdisc del dev $NET_INTERFACE root 2>/dev/null || true
}

trap cleanup EXIT

if [[ "$CLEANUP_ONLY" == "true" ]]; then
    cleanup && echo "Cleaned up." && exit 0
fi

echo "Delay: ${DELAY_MS}ms ±${JITTER_MS}ms, Loss: ${PACKET_LOSS}%, Server: ${SERVER_IP}:${SERVER_PORT}"

# Egress delay on loopback (affects outgoing packets)
sudo tc qdisc del dev $NET_INTERFACE root 2>/dev/null || true
sudo tc qdisc add dev $NET_INTERFACE root netem delay ${DELAY_MS}ms ${JITTER_MS}ms ${PACKET_LOSS:+loss ${PACKET_LOSS}%}

echo "tc qdisc show:"
sudo tc qdisc show dev $NET_INTERFACE

# Start server and client
(cd "$PROJECT_ROOT" && cargo run -p gameserver) &
sleep 2
(cd "$PROJECT_ROOT" && cargo run -p client -- --server ${SERVER_IP}:${SERVER_PORT})

echo "Done. Effective RTT: ~$((DELAY_MS * 2))-$(((DELAY_MS + JITTER_MS) * 2))ms"
