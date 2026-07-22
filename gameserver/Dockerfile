# Build from workspace root:
# docker build -f gameserver/Dockerfile -t cmbevy-gameserver:testing .

# Build stage
FROM rustlang/rust:nightly-slim AS builder

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

RUN sed -i '/codegen-backend/d' .cargo/config.toml
RUN cargo build --release -p gameserver --bin gameserver

# Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --create-home --uid 10001 gameserver

WORKDIR /app
COPY --from=builder /app/target/release/gameserver /usr/local/bin/gameserver
COPY --from=builder --chown=gameserver:gameserver /app/assets/maps ./assets/maps
COPY --from=builder --chown=gameserver:gameserver /app/assets/gametypes ./assets/gametypes
COPY --from=builder --chown=gameserver:gameserver /app/assets/collision ./assets/collision

USER gameserver

EXPOSE 42070/udp
EXPOSE 42071/udp
ENTRYPOINT ["gameserver"]
CMD ["--port", "42070"]
