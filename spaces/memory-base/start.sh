#!/bin/bash
echo "==> Starting Olympus OMNIMEM Shard..."

echo "==> Starting Olympus OMNIMEM Shard..."

# Create required directories
mkdir -p /app/data
cd /app

# Start Redis in background
echo "==> Launching Redis..."
redis-server /etc/redis/redis.conf &

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
    echo "ERROR: Redis failed to start! Continuing anyway to keep container alive for debugging..."
fi

echo "==> Redis maxmemory: $(redis-cli CONFIG GET maxmemory)"

# Start Webdis in foreground (this keeps the container alive)
echo "==> Launching Webdis on port 7860..."
webdis /etc/webdis/webdis.json || {
    echo "ERROR: Webdis crashed!"
    echo "==> Starting dummy HTTP server on 7860 to keep HF Space Healthy for debugging..."
    python3 -m http.server 7860
}

echo "==> Entering debug sleep loop. Container will stay alive."
while true; do sleep 60; done
