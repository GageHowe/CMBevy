# GameServer

server executable for one multiplayer game instance.

## Container

Build from the workspace root:

```bash
make gameserver-image
```

The current CI gameserver build is gated/skipped; use this local image target for test server images.

Build and run on a Linux VPS:

```bash
./scripts/start_server.sh
```

Run a local test server:

```bash
docker run --rm -it -p 42070:42070/udp cmbevy-gameserver:testing
```

Override map/gametype/name with normal gameserver args:

```bash
docker run --rm -it -p 42070:42070/udp cmbevy-gameserver:testing \
  --port 42070 \
  --map maps/ring_poc.ron \
  --gametype /app/assets/gametypes/ffa.lua \
  --advertise-name "VPC Test" \
  --advertise-max-players 8
```

For a VPC host, open inbound UDP `42070` to clients. UDP `42071` is only for LAN discovery. If advertising through the beacon, allow outbound HTTPS `443` and UDP `42072`.
