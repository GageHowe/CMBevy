# feature-unification = "package" in .cargo/config.toml means each binary gets its own
# feature set — no unification across workspace members. Use -p <package> to be explicit.

.PHONY: default server client server-release client-release build build-release check

build:
	cargo build -p gameserver
	cargo build -p client

server:
	cargo run -p gameserver

client:
	cargo run -p client

server-release:
	cargo run -p gameserver --release

client-release:
	cargo run -p client --release

build-release:
	cargo build -p gameserver --release
	cargo build -p client --release

check:
	cargo check -p gameserver
	cargo check -p client

clean:
	cargo clean
