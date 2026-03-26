# feature-unification = "package" in .cargo/config.toml means each binary gets its own
# feature set — no unification across workspace members. Use -p <package> to be explicit.

# .PHONY: dev test-network build s c emulator build-release

dev:
	cargo build -p gameserver && cargo run -p client

test-network:
	powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test_multiplayer_windows.ps1

build:
	cargo build -p gameserver
	cargo build -p client
	cargo build -p network_emulator

s:
	cargo run -p gameserver

c:
	cargo run -p client

emulator:
	cargo run -p network_emulator --release

build-release:
	cargo build -p gameserver --release
	cargo build -p client --release
	cargo build -p network_emulator --release

check:
	cargo check -p gameserver
	cargo check -p client
	cargo check -p network_emulator

clean:
	cargo clean
