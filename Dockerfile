FROM rust:1.95-bookworm AS builder
WORKDIR /app
COPY src/server ./src/server
RUN cargo build --release --manifest-path src/server/Cargo.toml

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /app/src/server/target/release/parquet-viewer-server /usr/local/bin/parquet-viewer-server
ENV PV_HOST=0.0.0.0 PV_PORT=8080
EXPOSE 8080
ENTRYPOINT ["parquet-viewer-server"]
