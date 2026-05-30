"""
Nancy HF Space — Health & Status Router.

Provides health checks and detailed status monitoring endpoints for system observability.
"""

from __future__ import annotations

import time
import logging

from fastapi import APIRouter, Depends

from core.auth import require_api_key
from core.queue import task_queue
from core.router import provider_router

logger = logging.getLogger("nancy.health")

router = APIRouter(tags=["Health & Monitoring"])


@router.get("/health")
async def health_check():
    """
    Simple Liveness/Readiness Probe.
    Used by keep-alive cron pings to prevent HF Space sleeping.
    """
    # Include extension connection count so UptimeRobot and monitoring can see it
    ext_count = 0
    try:
        from routers.extension import active_extensions
        now = time.time()
        ext_count = sum(1 for ts in active_extensions.values() if (now - ts) < 45.0)
    except Exception:
        pass

    return {
        "status": "healthy",
        "timestamp": time.time(),
        "active_extensions": ext_count,
    }


@router.get("/health/redis")
async def redis_health_check():
    """
    Redis / Webdis Connectivity Health Check.
    No authentication required so Ultron and monitoring tools can poll freely.
    Differentiates: disabled | offline | auth_error | ok
    """
    from core.redis_client import redis_client

    if not redis_client.is_enabled:
        return {
            "redis": "disabled",
            "latency_ms": None,
            "detail": "Redis is not configured (UPSTASH_REDIS_REST_URL or self-hosted Webdis URL missing).",
        }

    start = time.time()
    try:
        result = await redis_client._execute("PING")
        latency_ms = round((time.time() - start) * 1000, 2)

        if result in ("PONG", "pong", True, "OK"):
            return {
                "redis": "ok",
                "latency_ms": latency_ms,
                "detail": "Redis PING returned PONG successfully.",
            }
        else:
            return {
                "redis": "unexpected_response",
                "latency_ms": latency_ms,
                "detail": f"Redis PING returned unexpected value: {result!r}",
            }
    except Exception as exc:
        latency_ms = round((time.time() - start) * 1000, 2)
        logger.error("Redis health check failed: %s", exc)
        return {
            "redis": "offline",
            "latency_ms": latency_ms,
            "detail": str(exc)[:200],
        }


@router.get("/health/extensions")
async def extension_health_check():
    """
    Extension SSE Connection Health.
    No authentication required so Ultron dashboard can poll without an API key.
    Returns count of active extension SSE connections in the last 45 seconds.
    """
    try:
        from routers.extension import active_extensions
        now = time.time()
        active = {
            eid: round(now - ts, 1)
            for eid, ts in active_extensions.items()
            if (now - ts) < 45.0
        }
        return {
            "active_extension_count": len(active),
            "extensions": active,
            "queue": task_queue.get_status(),
        }
    except Exception as exc:
        return {"active_extension_count": 0, "extensions": {}, "error": str(exc)}


@router.get("/status")
async def detailed_status(api_key: str = Depends(require_api_key)):
    """
    Detailed System Status.
    Requires Nancy API key authentication. Returns task queue size,
    provider circuit breaker states, and connected extension sessions.
    """
    # Import active_extensions dynamically to avoid circular import
    active_exts = {}
    try:
        from routers.extension import active_extensions
        now = time.time()
        for ext_id, last_seen in list(active_extensions.items()):
            active_exts[ext_id] = {
                "last_seen_ago": round(now - last_seen, 1),
                "online": (now - last_seen) < 30.0,  # 30 seconds threshold
            }
    except Exception:
        pass

    return {
        "status": "running",
        "timestamp": time.time(),
        "queue": task_queue.get_status(),
        "router": {
            "providers": provider_router.get_provider_states(),
            "available_models": provider_router.get_available_models(),
        },
        "active_tasks": task_queue.get_active_tasks(),
        "recent_history": task_queue.get_history(20),
        "connected_extensions": active_exts,
    }
