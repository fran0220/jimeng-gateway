# Stage 1: Build Rust binary
FROM rust:1.92-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

# Stage 2: Build React frontend
FROM node:22-bookworm-slim AS frontend
WORKDIR /app/web
COPY web/package.json web/package-lock.json* ./
RUN npm install
COPY web/ .
RUN npm run build

# Stage 3: Runtime
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /app/target/release/jimeng-gateway ./
COPY --from=frontend /app/web/dist ./web/dist/

RUN mkdir -p /app/data

EXPOSE 5100

ENV PORT=5100 \
    JIMENG_UPSTREAM=http://jimeng-api:8000 \
    JIMENG_CONTAINER=jimeng-free-api-all \
    DATABASE_URL=sqlite:///app/data/gateway.db?mode=rwc

CMD ["./jimeng-gateway"]
