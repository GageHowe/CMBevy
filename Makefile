# feature-unification = "package" in .cargo/config.toml means each binary gets its own
# feature set — no unification across workspace members. Use -p <package> to be explicit.

# .PHONY: dev test-network build s c emulator build-release build-testing runs-release runs-testing runc-release runc-testing

PERF_BUILDID_DIR := $(CURDIR)/target/perf-buildid
PROFILING_RUSTFLAGS := -C force-frame-pointers=yes

define CLIPPY_COMMANDS
	cargo clippy -p client --bin client --no-deps -q
	cargo clippy -p gameserver --bin gameserver --no-deps -q
	# cargo clippy -p network_emulator --lib --bin network_emulator --no-deps -q
endef

build:
	cargo build -p client
	cargo build -p gameserver
	# cargo build -p network_emulator

dummy:
	# $(CLIPPY_COMMANDS)

run:
	cargo build -p gameserver && cargo run -p client

runs:
	cargo run -p gameserver
runs-release:
	cargo run -p gameserver --release
runs-testing:
	cargo run -p gameserver --profile profiling

runc:
	cargo run -p client
runc-release:
	cargo run -p client --release
runc-testing:
	cargo run -p client --profile profiling

emulator:
	cargo run -p network_emulator --release

build-release:
	cargo build -p gameserver --release
	cargo build -p client --release
	cargo build -p network_emulator --release

build-testing: # optimized, contains debug info
	cargo build -p gameserver --profile profiling
	cargo build -p client --profile profiling
	cargo build -p network_emulator --profile profiling

check:
	cargo check -p gameserver
	cargo check -p client
	cargo check -p network_emulator

clippy:
	$(CLIPPY_COMMANDS)

clean:
	cargo clean

# build-profiling:
# 	RUSTFLAGS="$(PROFILING_RUSTFLAGS)" cargo build -p gameserver --profile profiling
# 	RUSTFLAGS="$(PROFILING_RUSTFLAGS)" cargo build -p client --profile profiling

# perf-client: build-profiling
# 	mkdir -p $(PERF_BUILDID_DIR)
# 	DEBUGINFOD_URLS= PERF_BUILDID_DIR="$(PERF_BUILDID_DIR)" LD_LIBRARY_PATH="$(CURDIR)/target/profiling:$$LD_LIBRARY_PATH" perf record -F 99 --call-graph fp -- target/profiling/client --server 127.0.0.1:42070

# perf-server: build-profiling
# 	mkdir -p $(PERF_BUILDID_DIR)
# 	DEBUGINFOD_URLS= PERF_BUILDID_DIR="$(PERF_BUILDID_DIR)" perf record -F 99 --call-graph fp -- target/profiling/gameserver --port 42070

# perf-report:
# 	mkdir -p $(PERF_BUILDID_DIR)
# 	DEBUGINFOD_URLS= PERF_BUILDID_DIR="$(PERF_BUILDID_DIR)" perf report

# perf-top-client:
# 	mkdir -p $(PERF_BUILDID_DIR)
# 	DEBUGINFOD_URLS= PERF_BUILDID_DIR="$(PERF_BUILDID_DIR)" LD_LIBRARY_PATH="$(CURDIR)/target/profiling:$$LD_LIBRARY_PATH" perf top -- target/profiling/client --server 127.0.0.1:42070
