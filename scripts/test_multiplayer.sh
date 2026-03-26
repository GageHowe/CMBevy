sudo fuser -k 42069/udp
sudo fuser -k 42070/udp

cargo run -p network_emulator -- --listen 127.0.0.1:42069 --server 127.0.0.1:42070 &
cargo run -p gameserver &

sleep 1

cargo run --bin client &

wait
