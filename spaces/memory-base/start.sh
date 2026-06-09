#!/bin/bash
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
    if redis-cli ping 2>/dev/null | grep -q PONG; then
        echo "==> Redis is ready!"
        break
    fi
    echo "    Attempt $i/30 -- waiting..."
    sleep 1
done

# Verify Redis is running
if ! redis-cli ping 2>/dev/null | grep -q PONG; then
    echo "ERROR: Redis failed to start!"
    redis-server /etc/redis/redis.conf --loglevel verbose 2>&1 | head -50
    echo "==> Retrying Redis with minimal config..."
    redis-server --bind 0.0.0.0 --port 6379 --daemonize no --maxmemory 128mb --maxmemory-policy allkeys-lru --protected-mode no --appendonly no --save "" &
    sleep 2
fi

echo "==> Redis info:"
redis-cli INFO server 2>/dev/null | head -5

# Start Webdis in foreground (this keeps the container alive)
echo "==> Launching Webdis on port 7860..."
exec webdis /etc/webdis/webdis.json
