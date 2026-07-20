# Network Emulator

Run the game server, then point clients at this UDP proxy:

```sh
cargo run -p network_emulator --release -- \
  --listen 0.0.0.0:42069 \
  --server 127.0.0.1:42070 \
  --mindelay 40 --maxdelay 80 --loss 0.05 --seed 123
```

From another machine, connect the client to `EMULATOR_IP:42069`. The emulator may run on
the server host or a third host; by default its upstream socket binds to a wildcard ephemeral
port. Use `--server-bind ADDR` when the route needs a specific local address.
