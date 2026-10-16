# ---- Build stage ----
FROM rust:1-slim-bookworm AS builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --bin lumen-pod-server && \
    cp /app/target/release/lumen-pod-server /usr/local/bin/lumen-pod-server

# ---- Runtime stage ----
FROM debian:bookworm-slim

RUN apt-get update && \
    apt-get install -y ca-certificates libssl3 curl && \
    rm -rf /var/lib/apt/lists/*

RUN useradd -m -s /bin/bash lumenpod && \
    mkdir -p /data/audio && chown lumenpod:lumenpod /data/audio

COPY --from=builder /usr/local/bin/lumen-pod-server /usr/local/bin/lumen-pod-server

USER lumenpod
EXPOSE 3210

ENV DATABASE_URL=postgres://lumenpod:lumenpod@db:5432/lumenpod
ENV PORT=3210
ENV RUST_LOG=lumen_pod_api=info,tower_http=info

ENTRYPOINT ["lumen-pod-server"]
CMD ["serve"]
