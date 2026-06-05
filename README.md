# Olympus V5 — Distributed Cognitive Cluster

> **Version:** 5.0.0 | **Status:** Active Build | **Org:** [ultron-v5](https://huggingface.co/ultron-v5)

## Architecture

```
LOCAL WORKSPACE (Antigravity Cloud)
  orchestrator/ (Python 4-Tier Swarm CLI)
       ↕ HTTPS
SPACE 1: olympus-gateway (Rust · Axum · Moka · Tantivy)
       ↕ Webdis REST + Bearer Auth
SPACE 2: olympus-memory-l1  (Redis + Webdis — Working Memory)
SPACE 3: olympus-memory-l3  (Redis + Webdis — Semantic Core)
SPACE 4: olympus-memory-l4  (Redis + Webdis — Skillbook)
SPACE 5: olympus-memory-rnd (Redis + Webdis — R&D Queue)
```

## Quick Start

### 1. Set secrets in environment
```bash
export HF_TOKEN="hf_..."
export ADMIN_TOKEN="your-secure-admin-token"
export GEMINI_API_KEY="your-gemini-key"
export GATEWAY_URL="https://ultron-v5-olympus-gateway.hf.space"
```

### 2. Inject your first API key
```bash
python -m orchestrator.main inject-key \
  --provider groq --model llama3-70b-8192 --key gsk_xxx
```

### 3. Check cluster health
```bash
python -m orchestrator.main health
```

### 4. Run the swarm on a project
```bash
python -m orchestrator.main run "Build a FastAPI REST service for user authentication"
```

## HF Spaces

| Space | URL | Role |
|-------|-----|------|
| olympus-gateway | `ultron-v5-olympus-gateway.hf.space` | Rust Axum Brain Stem |
| olympus-memory-l1 | `ultron-v5-olympus-memory-l1.hf.space` | Working Memory |
| olympus-memory-l3 | `ultron-v5-olympus-memory-l3.hf.space` | Semantic Rules |
| olympus-memory-l4 | `ultron-v5-olympus-memory-l4.hf.space` | Procedural Skills |
| olympus-memory-rnd | `ultron-v5-olympus-memory-rnd.hf.space` | R&D Queue |

## Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `POST` | `/v1/chat/completions` | JIT-compiled LLM chat |
| `POST` | `/v1/admin/keys` | Inject API key at runtime |
| `GET`  | `/v1/health` | Cluster health (all 5 spaces) |
| `GET`  | `/v1/admin/status` | KeyPool stats + Tantivy metrics |
| `POST` | `/v1/memory/error` | Push error to R&D queue |
| `GET`  | `/dashboard` | Ultron Control Panel UI |

## Local Development

```bash
# Create .env file
cp .env.example .env
# Edit .env with your secrets

# Run locally (cloud terminal only)
make dev
```

## Deploying to HF Spaces

```bash
# Deploy memory shards first (they need to be running before gateway warm-up)
make push-memory

# Deploy gateway
make push-gateway
```

## Zero Local Binaries Rule
All `cargo`, `python`, and `git` commands must be run inside the **Antigravity cloud terminal**. Never install Rust or Python directly on Windows.
