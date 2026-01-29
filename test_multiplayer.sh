sudo fuser -k 42069/udp
sudo fuser -k 42070/udp

go run emulate_network.go -client 42069 -server 42070 &
cargo run --bin server --features server &

sleep 1

cargo run --bin client --features client &

wait