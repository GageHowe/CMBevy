define CLIPPY_COMMANDS
	cargo clippy -p client --bin client --no-deps -q
	cargo clippy -p gameserver --bin gameserver --no-deps -q
	# cargo clippy -p network_emulator --lib --bin network_emulator --no-deps -q
endef

build:
	cargo build -p client
	cargo build -p gameserver

# beacon:
# 	cargo build -p beacon

beacon-r:
	cargo build -p beacon --release
	mkdir -p build
	cp target/release/beacon build/beacon.new
	mv -f build/beacon.new build/beacon

gameserver-image:
	DOCKER_BUILDKIT=1 docker build -f gameserver/Dockerfile -t cmbevy-gameserver:testing .

runb: beacon-r
	cargo run -p beacon --release

dummy:
	# $(CLIPPY_COMMANDS)

run:  # --features watch_assets
	cargo build -p gameserver && cargo run -p client

runs:
	cargo run -p gameserver
runs-r:
	cargo run -p gameserver --release
runs-testing:
	cargo run -p gameserver --profile profiling

runc:
	cargo run -p client
runc-r:
	cargo run -p client --release
runc-testing:
	cargo run -p client --profile profiling

emulator:
	cargo run -p network_emulator --release

buildc:
	cargo build -p client
builds:
	cargo build -p gameserver
build-r: builds-r buildc-r
	# cargo run -p packaging_tool --release
builds-r:
	cargo build -p gameserver --release
buildc-r:
	cargo build -p client --release

build-testing: # optimized, contains debug info
	cargo build -p gameserver --profile profiling
	cargo build -p client --profile profiling

check:
	cargo check -p gameserver
	cargo check -p client
	cargo check -p network_emulator

clippy:
	$(CLIPPY_COMMANDS)

clean:
	cargo clean
