#!/bin/bash
set -e

echo "==> Starting Olympus OMNIMEM Shard..."

# Create required directories
mkdir -p /app/data
cd /app

# Start Redis in background
echo "==> Launching Redis..."
redis-server /etc/redis/redis.conf

# Wait for Redis to be ready
echo "==> Waiting for Redis to be ready..."
for i in $(seq 1 30); do
    if redis-cli ping | grep -q PONG; then
        echo "==> Redis is ready!"
        break
    fi
    echo "    Attempt $i/30 — waiting..."
    sleep 1
done

# Verify Redis is running
if ! redis-cli ping | grep -q PONG; then
    echo "ERROR: Redis failed to start!"
    exit 1
fi

echo "==> Redis maxmemory: $(redis-cli CONFIG GET maxmemory)"

# Start Webdis in foreground (this keeps the container alive)
echo "==> Launching Webdis on port 7860..."
exec webdis /etc/webdis/webdis.json
