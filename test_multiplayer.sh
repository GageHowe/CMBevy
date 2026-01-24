# sudo fuser -k 8080/tcp

cargo run --bin server &
cargo run --bin client
