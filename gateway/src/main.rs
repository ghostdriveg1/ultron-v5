// gateway/src/main.rs
//
// Olympus V5 Gateway — Entry Point
//
// Boot sequence:
//   1. Load config from env vars
//   2. Initialize ProxyRotator, KeyPool, OmniMem, TantivyEngine
//   3. Run warm-up sequence (pre-populate Moka L0 cache)
//   4. Spawn detached ACE-2026 R&D Self-Healing Loop
//   5. Bind Axum HTTP server on 0.0.0.0:7860

mod config;
mod keypool;
mod memory;
mod proxy;
mod rnd_loop;
mod router;
mod tantivy_engine;

use std::sync::Arc;
use anyhow::Result;
use tracing::info;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

#[tokio::main]
async fn main() -> Result<()> {
    // ── Initialize tracing ────────────────────────────────────────────────
    tracing_subscriber::registry()
        .with(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with(fmt::layer().with_target(false))
        .init();

    info!("╔══════════════════════════════════════════════════╗");
    info!("║   OLYMPUS V5 — Distributed Cognitive Gateway     ║");
    info!("║   5-Space Cluster | Axum | Moka | Tantivy        ║");
    info!("╚══════════════════════════════════════════════════╝");

    // ── Load config ───────────────────────────────────────────────────────
    // Load .env file if present (local dev convenience)
    let _ = dotenvy::dotenv();
    let config = Arc::new(config::Config::from_env());
    info!(port = config.port, "Config loaded");

    // ── Initialize subsystems ─────────────────────────────────────────────
    info!("==> Initializing ProxyRotator...");
    let proxy = Arc::new(proxy::ProxyRotator::new(config.proxy_list.clone())?);
    info!(proxy_count = proxy.proxy_count(), "ProxyRotator ready");

    info!("==> Initializing KeyPool...");
    let keypool = keypool::new_shared_keypool();
    info!("KeyPool ready (empty — inject keys via POST /v1/admin/keys)");

    info!("==> Initializing OmniMem (4-shard cluster)...");
    let memory = Arc::new(memory::OmniMem::new(&config)?);
    info!("OmniMem initialized");

    info!("==> Initializing Tantivy BM25 RAM index...");
    let tantivy = tantivy_engine::new_tantivy_engine()?;
    info!("Tantivy engine ready");

    // ── Warm-up sequence ──────────────────────────────────────────────────
    info!("==> Running warm-up sequence (pre-loading L0 Moka cache)...");
    if let Err(e) = memory.warmup(config.warmup_skill_count).await {
        tracing::warn!(error = %e, "Warm-up had errors (non-fatal — shards may still be starting)");
    }
    info!("==> Warm-up complete");

    // ── Spawn R&D Self-Healing Loop ───────────────────────────────────────
    {
        let config_clone = config.clone();
        let memory_clone = memory.clone();
        let keypool_clone = keypool.clone();
        let tantivy_clone = tantivy.clone();
        tokio::spawn(async move {
            rnd_loop::run_rnd_loop(config_clone, memory_clone, keypool_clone, tantivy_clone).await;
        });
    }
    info!("==> ACE-2026 R&D loop spawned (detached Tokio task)");

    // ── Build and bind Axum router ────────────────────────────────────────
    let app_state = router::AppState {
        config: config.clone(),
        keypool,
        memory,
        tantivy,
        proxy,
    };

    let app = router::build_router(app_state);

    let addr = format!("0.0.0.0:{}", config.port);
    info!("==> Gateway online at http://{}", addr);
    info!("==> Dashboard: http://{}/dashboard", addr);
    info!("==> Health:    http://{}/v1/health", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
