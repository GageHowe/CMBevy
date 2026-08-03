# Build from workspace root: docker build -f beacon/Dockerfile .

# Build stage
FROM rustlang/rust:nightly-slim AS builder

RUN apt-get update && apt-get install -y build-essential && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .
RUN make beacon-r

FROM debian:bookworm-slim
WORKDIR /app
COPY --from=builder /app/build/beacon .

EXPOSE 8000
CMD ["./beacon"]
