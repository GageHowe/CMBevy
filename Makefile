CLIENT_FEATURES := --features game_objects/client

.PHONY: default server client server-release client-release build build-release check

build:
	cargo build --bin gameserver
	cargo build --bin client $(CLIENT_FEATURES)

server:
	cargo run --bin gameserver

client:
	cargo run --bin client $(CLIENT_FEATURES)

server-release:
	cargo run --bin gameserver --release

client-release:
	cargo run --bin client $(CLIENT_FEATURES) --release

build-release:
	cargo build --bin gameserver --release
	cargo build --bin client $(CLIENT_FEATURES) --release

check:
	cargo check --bin gameserver
	cargo check --bin client $(CLIENT_FEATURES)

clean:
	cargo clean
