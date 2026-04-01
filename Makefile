# feature-unification = "package" in .cargo/config.toml means each binary gets its own
# feature set — no unification across workspace members. Use -p <package> to be explicit.

# .PHONY: dev test-network build s c emulator build-release

FMOD_CORE_LIB := $(CURDIR)/assets/lib/fmodstudioapi20312linux/api/core/lib/x86_64
FMOD_STUDIO_LIB := $(CURDIR)/assets/lib/fmodstudioapi20312linux/api/studio/lib/x86_64
PERF_BUILDID_DIR := $(CURDIR)/target/perf-buildid
PROFILING_RUSTFLAGS := -C force-frame-pointers=yes

build:
	cargo build -p gameserver
	cargo build -p client
	cargo build -p network_emulator

cross-windows:
	cross build --target x86_64-pc-windows-gnu

dev:
	cargo build -p gameserver && cargo run -p client

test-network:
	powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test_multiplayer_windows.ps1

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

build-distribution:
	cargo build -p gameserver --profile distribution
	cargo build -p client --profile distribution
	cargo build -p network_emulator --profile distribution

build-profiling:
	RUSTFLAGS="$(PROFILING_RUSTFLAGS)" cargo build -p gameserver --profile profiling
	RUSTFLAGS="$(PROFILING_RUSTFLAGS)" cargo build -p client --profile profiling

flamegraph-client:
	mkdir -p target/profiling
	ln -sfn ../../assets target/profiling/assets
	mkdir -p $(PERF_BUILDID_DIR)
	DEBUGINFOD_URLS= PERF_BUILDID_DIR="$(PERF_BUILDID_DIR)" LD_LIBRARY_PATH="$(CURDIR)/target/profiling:$(FMOD_CORE_LIB):$(FMOD_STUDIO_LIB):$$LD_LIBRARY_PATH" RUSTFLAGS="$(PROFILING_RUSTFLAGS)" cargo flamegraph -F 99 -c "record --call-graph fp" --output flamegraph-client.svg --profile profiling -p client -- --server 127.0.0.1:42070

flamegraph-server:
	mkdir -p $(PERF_BUILDID_DIR)
	DEBUGINFOD_URLS= PERF_BUILDID_DIR="$(PERF_BUILDID_DIR)" RUSTFLAGS="$(PROFILING_RUSTFLAGS)" cargo flamegraph -F 99 -c "record --call-graph fp" --output flamegraph-server.svg --profile profiling -p gameserver -- --port 42070

perf-client: build-profiling
	mkdir -p target/profiling
	ln -sfn ../../assets target/profiling/assets
	mkdir -p $(PERF_BUILDID_DIR)
	DEBUGINFOD_URLS= PERF_BUILDID_DIR="$(PERF_BUILDID_DIR)" LD_LIBRARY_PATH="$(CURDIR)/target/profiling:$(FMOD_CORE_LIB):$(FMOD_STUDIO_LIB):$$LD_LIBRARY_PATH" perf record -F 99 --call-graph fp -- target/profiling/client --server 127.0.0.1:42070

perf-server: build-profiling
	mkdir -p $(PERF_BUILDID_DIR)
	DEBUGINFOD_URLS= PERF_BUILDID_DIR="$(PERF_BUILDID_DIR)" perf record -F 99 --call-graph fp -- target/profiling/gameserver --port 42070

perf-report:
	mkdir -p $(PERF_BUILDID_DIR)
	DEBUGINFOD_URLS= PERF_BUILDID_DIR="$(PERF_BUILDID_DIR)" perf report

perf-top-client:
	mkdir -p target/profiling
	ln -sfn ../../assets target/profiling/assets
	mkdir -p $(PERF_BUILDID_DIR)
	DEBUGINFOD_URLS= PERF_BUILDID_DIR="$(PERF_BUILDID_DIR)" LD_LIBRARY_PATH="$(CURDIR)/target/profiling:$(FMOD_CORE_LIB):$(FMOD_STUDIO_LIB):$$LD_LIBRARY_PATH" perf top -- target/profiling/client --server 127.0.0.1:42070

check:
	cargo check -p gameserver
	cargo check -p client
	cargo check -p network_emulator

clean:
	cargo clean
