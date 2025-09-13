# Multi-stage build for optimized Docker image
# Stage 1: Build dependencies cache
FROM rust:1.75-alpine AS dependencies
RUN apk add --no-cache musl-dev

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src

# Stage 2: Build WebUI
FROM node:20-alpine AS webui-builder
RUN apk add --no-cache curl bash

# Install Rust for WASM compilation
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --target wasm32-unknown-unknown
ENV PATH="/root/.cargo/bin:${PATH}"
RUN cargo install trunk --locked

WORKDIR /app/webui
COPY webui/Cargo.toml webui/Cargo.lock ./
COPY webui/src ./src
COPY webui/index.html webui/style.css webui/Trunk.toml ./
RUN trunk build --release

# Stage 3: Build application
FROM rust:1.75-alpine AS builder
RUN apk add --no-cache musl-dev

WORKDIR /app
COPY --from=dependencies /app/target /app/target
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY config ./config

# Rebuild with actual source
RUN touch src/main.rs
RUN cargo build --release

# Stage 4: Minimal runtime image
FROM alpine:3.19
RUN apk add --no-cache ca-certificates wget

WORKDIR /app

# Copy binary and assets
COPY --from=builder /app/target/release/speicherwald /app/speicherwald
COPY --from=webui-builder /app/ui /app/ui
COPY config /app/config

# Create non-root user and data directory
RUN mkdir -p /app/data && \
    adduser -D -H -u 1000 speicherwald && \
    chown -R speicherwald:speicherwald /app

USER speicherwald

ENV SPEICHERWALD__DATABASE__URL=sqlite:///app/data/speicherwald.db
ENV SPEICHERWALD__SERVER__HOST=0.0.0.0

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=3s --start-period=5s --retries=3 \
  CMD wget --no-verbose --tries=1 --spider http://localhost:8080/healthz || exit 1

CMD ["/app/speicherwald"]
