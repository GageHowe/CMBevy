FROM rust:1.76-slim

RUN apt-get update && apt-get install -y \
    clang \
    build-essential \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN cargo build --release --bin server

CMD ["./target/release/server"]
