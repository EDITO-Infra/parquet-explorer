FROM rust:1.95-bookworm AS backend-builder
WORKDIR /app
COPY src/backend ./src/backend
RUN cargo build --release --locked --manifest-path src/backend/Cargo.toml

FROM node:22-alpine AS frontend-builder
WORKDIR /app
COPY src/frontend/package.json src/frontend/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY src/frontend ./
RUN npm run build

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates nginx curl \
    && rm -rf /var/lib/apt/lists/* \
    && rm -f /etc/nginx/sites-enabled/default
COPY --from=backend-builder /app/src/backend/target/release/parquet-explorer-backend /usr/local/bin/parquet-explorer-backend
COPY --from=frontend-builder /app/dist /usr/share/nginx/html
COPY deploy/nginx.conf /etc/nginx/conf.d/default.conf
COPY deploy/start.sh /usr/local/bin/start.sh
RUN chmod +x /usr/local/bin/start.sh

ENV PE_HOST=127.0.0.1 \
    PE_PORT=8080
EXPOSE 80
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s \
    CMD curl -fsS http://127.0.0.1/api/v1/health || exit 1
ENTRYPOINT ["start.sh"]
