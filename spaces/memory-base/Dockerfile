FROM debian:bookworm-slim

LABEL maintainer="ultron-v5"
LABEL description="OMNIMEM Shard -- Redis + Webdis for Olympus V5"

# Install build deps + runtime deps in one layer
RUN apt-get update && apt-get install -y --no-install-recommends \
    redis-server \
    build-essential \
    libevent-dev \
    libssl-dev \
    git \
    curl \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Build Webdis from source (official repo: nicolasff/webdis)
RUN git clone --depth 1 https://github.com/nicolasff/webdis.git /tmp/webdis \
    && cd /tmp/webdis \
    && make \
    && cp webdis /usr/local/bin/webdis \
    && chmod +x /usr/local/bin/webdis \
    && rm -rf /tmp/webdis

# Create app directory and user
RUN useradd -m -u 1000 user && \
    mkdir -p /app/data /etc/webdis && \
    chown -R user:user /app /etc/webdis /etc/redis /usr/local/bin/webdis

WORKDIR /app

# Copy configs
COPY --chown=user:user redis.conf /etc/redis/redis.conf
COPY --chown=user:user webdis.json /etc/webdis/webdis.json
COPY --chown=user:user start.sh /start.sh
RUN chmod +x /start.sh

USER user

# HF Spaces uses port 7860
EXPOSE 7860

# Real health check -- Webdis responds to /PING with {"PING":"PONG"}
HEALTHCHECK --interval=30s --timeout=10s --start-period=60s --retries=5 \
    CMD curl -sf http://localhost:7860/PING || exit 1

CMD ["/start.sh"]
