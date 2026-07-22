# feature-unification = "package" in .cargo/config.toml means each binary gets its own
# feature set — no unification across workspace members. Use -p <package> to be explicit.

.PHONY: build beacon-release gameserver-image runb dummy run runs runs-release runs-testing runc runc-release runc-testing emulator pack-assets build-release build-testing check clippy clean

define CLIPPY_COMMANDS
	cargo clippy -p client --bin client --features watch_assets --no-deps -q
	cargo clippy -p gameserver --bin gameserver --no-deps -q
	# cargo clippy -p network_emulator --lib --bin network_emulator --no-deps -q
endef

build:
	cargo build -p client --features watch_assets
	cargo build -p gameserver

# beacon:
# 	cargo build -p beacon

beacon-release:
	cargo build -p beacon --release
	mkdir -p build
	cp target/release/beacon build/beacon.new
	mv -f build/beacon.new build/beacon

gameserver-image:
	docker build -f gameserver/Dockerfile -t cmbevy-gameserver:testing .

runb: beacon-release
	cargo run -p beacon --release

dummy:
	# $(CLIPPY_COMMANDS)

run:
	cargo build -p gameserver && cargo run -p client --features watch_assets

runs:
	cargo run -p gameserver
runs-release:
	cargo run -p gameserver --release
runs-testing:
	cargo run -p gameserver --profile profiling

runc:
	cargo run -p client --features watch_assets
runc-release:
	cargo run -p client --release
runc-testing:
	cargo run -p client --profile profiling

emulator:
	cargo run -p network_emulator --release

build-release:
	cargo run -p packaging_tool --release

build-testing: # optimized, contains debug info
	cargo build -p gameserver --profile profiling
	cargo build -p client --profile profiling

check:
	cargo check -p gameserver
	cargo check -p client --features watch_assets
	cargo check -p network_emulator

clippy:
	$(CLIPPY_COMMANDS)

clean:
	cargo clean
