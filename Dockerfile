# docker run --rm -p 42069:42069/udp imagename
# use with a service that supports udp

FROM rustlang/rust:nightly-slim

RUN apt-get update && apt-get install -y \
    clang \
    build-essential \
    pkg-config \
    libwayland-dev \
    libxkbcommon-dev \
    libudev-dev \
    libasound2-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN cargo build --release --bin server --features server

EXPOSE 
CMD ["./target/release/server"]
