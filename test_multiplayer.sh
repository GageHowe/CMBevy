host=localhost
port=42069

sudo fuser -k $port/udp
go run emulate_network.go -a :42069 -b :42070 &
cargo run --bin server &
cargo run --bin client &

wait