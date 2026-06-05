use axum::{
    routing::{get, post},
    Router,
    Json,
    extract::State,
};
use moka::future::Cache;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

mod keypool;
mod memory;

use keypool::KeyPool;
use memory::MemoryManager;

// The global application state shared across all Axum threads
pub struct AppState {
    pub keypool: Arc<KeyPool>,
    pub memory: Arc<MemoryManager>,
    pub l0_cache: Cache<String, String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    info!("Booting Olympus Gateway V4 - Distributed State Manager...");

    // Initialize the L0 Local Cache (Moka) with a 5-minute TTL
    let l0_cache = Cache::builder()
        .max_capacity(10_000)
        .time_to_live(Duration::from_secs(300))
        .build();

    let keypool = Arc::new(KeyPool::new());
    let memory = Arc::new(MemoryManager::new());

    // --- The "Cold-Start" Storm Preventer ---
    info!("Initiating Moka L0 Cache Warm-up Sequence...");
    let critical_keys = vec![
        "ultron:l3:core_architecture:v1",
        "ultron:l3:security_protocols:v1",
    ];
    
    for key in critical_keys {
        info!("Warming up critical rule: {}", key);
        match memory.fetch_from_webdis(key).await {
            Ok(content) => {
                l0_cache.insert(key.to_string(), content).await;
                info!("Successfully cached {}", key);
            }
            Err(_) => warn!("Failed to fetch {} during warm-up. Is Shard A online?", key),
        }
    }
    info!("Cache Warm-up Complete.");

    let state = Arc::new(AppState {
        keypool,
        memory,
        l0_cache,
    });

    let app = Router::new()
        .route("/health", get(health_check))
        .route("/v1/chat/completions", post(handle_chat))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], 7860));
    info!("Olympus Gateway listening strictly on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn health_check() -> &'static str {
    "Olympus Gateway V4 is Online and Fully Armed."
}

#[derive(Deserialize)]
struct ChatRequest {
    model: String,
    messages: Vec<serde_json::Value>,
}

async fn handle_chat(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChatRequest>,
) -> Json<serde_json::Value> {
    info!("Received execution task for model: {}", payload.model);
    
    // The actual routing logic will be implemented here
    // 1. Check Tantivy Index for matching skills
    // 2. Fetch skill content from L0 Cache (or Webdis if missing)
    // 3. Compile JIT CLAUDE.md
    // 4. Hit KeyPool

    Json(serde_json::json!({
        "status": "acknowledged",
        "model": payload.model
    }))
}
