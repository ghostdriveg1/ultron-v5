# syntax=docker/dockerfile:1
FROM rust:1.79-slim-bookworm AS builder

# Install build deps
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config \
    libssl-dev \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Cache dependency build layer
COPY gateway/Cargo.toml gateway/Cargo.toml
COPY crates/orchestrator/Cargo.toml crates/orchestrator/Cargo.toml
COPY Cargo.toml Cargo.toml
# Create dummy main to compile deps first
RUN mkdir -p gateway/src && echo 'fn main(){}' > gateway/src/main.rs
RUN mkdir -p crates/orchestrator/src && echo 'fn main(){}' > crates/orchestrator/src/main.rs
RUN cargo build --release 2>/dev/null || true

# Now copy real source
COPY gateway/src gateway/src
COPY crates/orchestrator/src crates/orchestrator/src
COPY dashboard dashboard

# Force rebuild of our code
RUN touch gateway/src/main.rs
RUN touch crates/orchestrator/src/main.rs
RUN cargo build --release -p gateway

# ─── Runtime image ────────────────────────────────────────────────────────────
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    bash \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/gateway /app/gateway

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=20s --retries=3 \
    CMD bash -c 'exec 3<>/dev/tcp/localhost/7860 && echo -e "GET /v1/health HTTP/1.0\r\n\r\n" >&3 && cat <&3 | grep -q "healthy"'

EXPOSE 7860

CMD ["/app/gateway"]
