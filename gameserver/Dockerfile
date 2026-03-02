# Build from workspace root: docker build -f gameserver/Dockerfile .

# Build stage
FROM rustlang/rust:nightly-slim AS builder

RUN apt-get update && apt-get install -y build-essential && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

RUN sed -i '/codegen-backend/d' .cargo/config.toml
RUN cargo build --release --bin gameserver

# Runtime
FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/target/release/gameserver .
EXPOSE 8000
CMD ["./gameserver"]
