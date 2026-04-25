# Build from workspace root: docker build -f beacon/Dockerfile .

# Build stage
FROM rustlang/rust:nightly-slim AS builder

RUN apt-get update && apt-get install -y build-essential && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
# Cranelift is a local-dev speedup; use LLVM for release builds in Docker
RUN sed -i '/codegen-backend/d' .cargo/config.toml
RUN sed -i '/^\[target.x86_64-unknown-linux-gnu\]/,+2d' .cargo/config.toml
RUN cargo build --release -p beacon

# Runtime
FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/beacon .
# COPY beacon/schema.sql .
EXPOSE 8000
CMD ["./beacon"]
