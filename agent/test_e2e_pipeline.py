"""
Project Olympus — End-to-End Pipeline Health Check
===================================================
Validates the complete 4-tier chain is wired correctly before dispatching any task.

Run locally with:
    python agent/test_e2e_pipeline.py --nancy-url https://free-llm-router.hf.space --nancy-key <key>

Or with env vars:
    NANCY_URL=https://... NANCY_KEY=ny_... python agent/test_e2e_pipeline.py
"""
from __future__ import annotations

import argparse
import asyncio
import json
import os
import sys

import httpx

GREEN = "\033[92m"
RED = "\033[91m"
YELLOW = "\033[93m"
CYAN = "\033[96m"
BOLD = "\033[1m"
RESET = "\033[0m"

PASS = f"{GREEN}✅ PASS{RESET}"
FAIL = f"{RED}❌ FAIL{RESET}"
WARN = f"{YELLOW}⚠️  WARN{RESET}"


def h1(msg: str) -> None:
    print(f"\n{BOLD}{CYAN}{'='*60}{RESET}")
    print(f"{BOLD}{CYAN}  {msg}{RESET}")
    print(f"{BOLD}{CYAN}{'='*60}{RESET}")


def check(name: str, passed: bool, detail: str = "") -> bool:
    status = PASS if passed else FAIL
    print(f"  {status}  {name}")
    if detail:
        print(f"         {YELLOW}{detail}{RESET}")
    return passed


async def run_tests(nancy_url: str, nancy_key: str, ultron_url: str = "") -> int:
    """Run all E2E health checks. Returns number of failed checks."""
    failures = 0

    async with httpx.AsyncClient(timeout=15.0) as client:
        # ─── CHECK 1: Nancy /health ─────────────────────────────────────────
        h1("Check 1: Nancy Gateway Health")
        try:
            r = await client.get(f"{nancy_url}/health")
            data = r.json()
            ok = r.status_code == 200 and data.get("status") == "healthy"
            failures += 0 if check("Nancy /health responds 200 with status=healthy", ok,
                                    f"HTTP {r.status_code} | body: {json.dumps(data)[:120]}") else 1
            ext_count = data.get("active_extensions", -1)
            check(f"Nancy reports extension count (got: {ext_count})", ext_count >= 0,
                  "active_extensions missing from /health response")
        except Exception as e:
            failures += 1
            check("Nancy /health reachable", False, str(e))

        # ─── CHECK 2: Nancy /health/redis ───────────────────────────────────
        h1("Check 2: Redis/Webdis Connectivity")
        try:
            r = await client.get(f"{nancy_url}/health/redis")
            data = r.json()
            redis_ok = data.get("redis") == "ok"
            failures += 0 if check("Redis PING returns ok", redis_ok,
                                    f"Redis state: {data.get('redis')} | detail: {data.get('detail', '')[:100]}") else 1
            if redis_ok:
                check(f"Redis latency acceptable (<200ms, got {data.get('latency_ms')}ms)",
                      (data.get("latency_ms") or 999) < 200)
        except Exception as e:
            failures += 1
            check("Nancy /health/redis reachable", False, str(e))

        # ─── CHECK 3: Nancy /ext/status ─────────────────────────────────────
        h1("Check 3: Extension SSE Connection")
        try:
            r = await client.get(f"{nancy_url}/ext/status")
            data = r.json()
            ext_count = data.get("active_extension_count", 0)
            ext_ok = ext_count > 0
            failures += 0 if check(f"At least 1 extension SSE connected (got: {ext_count})", ext_ok,
                                    "→ ACTION: Open Brave, load Nancy extension, click Connect in side panel") else 1
            queue = data.get("queue", {})
            print(f"         {CYAN}Queue: {queue.get('pending', '?')} pending | {queue.get('active', '?')} active{RESET}")
        except Exception as e:
            failures += 1
            check("Nancy /ext/status reachable", False, str(e))

        # ─── CHECK 4: Nancy /v1/chat/completions smoke test ─────────────────
        h1("Check 4: Nancy Chat Completions API")
        try:
            payload = {
                "model": "deepseek-chat",
                "messages": [{"role": "user", "content": "Reply with exactly: OLYMPUS_OK"}],
                "stream": False,
                "max_tokens": 20,
            }
            r = await client.post(
                f"{nancy_url}/v1/chat/completions",
                json=payload,
                headers={"Authorization": f"Bearer {nancy_key}"},
                timeout=90.0,
            )
            ok = r.status_code == 200
            failures += 0 if check("POST /v1/chat/completions returns 200", ok,
                                    f"HTTP {r.status_code} | body: {r.text[:200]}") else 1
            if ok:
                resp_data = r.json()
                content = resp_data.get("choices", [{}])[0].get("message", {}).get("content", "")
                check("Response contains expected echo", "OLYMPUS" in content.upper(),
                      f"Got: {content[:80]}")
        except Exception as e:
            failures += 1
            check("Nancy /v1/chat/completions reachable", False, str(e))

        # ─── CHECK 5: Ultron dashboard (optional) ───────────────────────────
        if ultron_url:
            h1("Check 5: Ultron Dashboard")
            try:
                r = await client.get(f"{ultron_url}/")
                ok = r.status_code == 200
                failures += 0 if check("Ultron root / responds 200", ok,
                                        f"HTTP {r.status_code}") else 1

                r2 = await client.get(f"{ultron_url}/admin/status")
                ok2 = r2.status_code == 200
                check("Ultron /admin/status responds 200", ok2, f"HTTP {r2.status_code}")

                r3 = await client.get(f"{ultron_url}/admin/validate-nancy")
                if r3.status_code == 200:
                    vdata = r3.json()
                    check("Ultron /admin/validate-nancy: Nancy connected",
                          vdata.get("connected", False),
                          f"Result: {json.dumps(vdata)[:200]}")
            except Exception as e:
                failures += 1
                check("Ultron reachable", False, str(e))

    # ─── FINAL SUMMARY ──────────────────────────────────────────────────────
    h1("Final Summary")
    total_checks = 9 if ultron_url else 7
    passed = total_checks - failures
    color = GREEN if failures == 0 else RED
    print(f"\n  {color}{BOLD}{passed}/{total_checks} checks passed{RESET}")
    if failures == 0:
        print(f"\n  {GREEN}{BOLD}🎉 ALL SYSTEMS GO — Project Olympus E2E pipeline is healthy!{RESET}")
    else:
        print(f"\n  {RED}{BOLD}⚠️  {failures} check(s) failed — fix before dispatching tasks.{RESET}")
    print()
    return failures


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Project Olympus E2E Health Check")
    parser.add_argument("--nancy-url", default=os.getenv("NANCY_URL", "http://127.0.0.1:7860"))
    parser.add_argument("--nancy-key", default=os.getenv("NANCY_KEY", ""))
    parser.add_argument("--ultron-url", default=os.getenv("ULTRON_URL", ""))
    return parser.parse_args()


if __name__ == "__main__":
    if sys.platform == 'win32':
        import io
        sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding='utf-8')

    args = parse_args()
    if not args.nancy_key:
        print(f"{RED}ERROR: --nancy-key or NANCY_KEY env var is required{RESET}")
        sys.exit(1)

    print(f"{BOLD}Project Olympus E2E Health Check{RESET}")
    print(f"Nancy URL : {CYAN}{args.nancy_url}{RESET}")
    print(f"Ultron URL: {CYAN}{args.ultron_url or '(not configured)'}{RESET}")

    failures = asyncio.run(run_tests(
        nancy_url=args.nancy_url.rstrip("/"),
        nancy_key=args.nancy_key,
        ultron_url=args.ultron_url.rstrip("/") if args.ultron_url else "",
    ))
    sys.exit(1 if failures > 0 else 0)

