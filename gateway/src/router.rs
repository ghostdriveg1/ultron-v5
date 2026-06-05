// gateway/src/router.rs
//
// Axum HTTP router — all API routes for the Olympus Gateway.
//
// Routes:
//   POST /v1/chat/completions     — JIT compiler → KeyPool → proxy → LLM
//   POST /v1/admin/keys           — Dynamic key injection
//   POST /v1/admin/providers      — Register any OpenAI-compatible provider at runtime
//   GET  /v1/admin/status         — KeyPool stats, Tantivy, cluster
//   GET  /v1/health               — Cluster health (all 5 shards)
//   POST /v1/memory/error         — Push error to R&D queue
//   GET  /dashboard               — Ultron Control Panel

use std::sync::Arc;
use axum::{
    extract::{Extension, Json},
    http::{HeaderMap, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::{
    config::Config,
    keypool::{AddKeyRequest, RegisterProviderRequest, SharedKeyPool},
    memory::SharedOmniMem,
    proxy::SharedProxyRotator,
    rnd_loop::{compute_error_hash, RndQueueItem},
    tantivy_engine::SharedTantivyEngine,
};

// ─────────────────────────────────────────────────────────────────────────────
// App State
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub keypool: SharedKeyPool,
    pub memory: SharedOmniMem,
    pub tantivy: SharedTantivyEngine,
    pub proxy: SharedProxyRotator,
}

// ─────────────────────────────────────────────────────────────────────────────
// Router construction
// ─────────────────────────────────────────────────────────────────────────────

pub fn build_router(state: AppState) -> Router {
    Router::new()
        // LLM proxy
        .route("/v1/chat/completions", post(chat_completions_handler))
        // Admin
        .route("/v1/admin/keys", post(add_keys_handler))
        .route("/v1/admin/providers", post(register_provider_handler))
        .route("/v1/admin/status", get(admin_status_handler))
        // Health
        .route("/v1/health", get(health_handler))
        // Memory operations
        .route("/v1/memory/error", post(push_error_handler))
        // Control Panel UI
        .route("/dashboard", get(dashboard_handler))
        .route("/", get(dashboard_handler))
        // Fallback
        .fallback(fallback_handler)
        .layer(Extension(Arc::new(state)))
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http())
}

// ─────────────────────────────────────────────────────────────────────────────
// Middleware: Admin auth
// ─────────────────────────────────────────────────────────────────────────────

fn check_admin_auth(headers: &HeaderMap, config: &Config) -> bool {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .map(|v| v == format!("Bearer {}", config.admin_token))
        .unwrap_or(false)
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/chat/completions
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub model: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stream: Option<bool>,
    pub provider: Option<String>,  // optional provider override
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

pub async fn chat_completions_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(mut req): Json<ChatRequest>,
) -> impl IntoResponse {
    let model = req.model.clone().unwrap_or_else(|| "llama-3.3-70b-versatile".to_string());
    let provider_name = req.provider.clone().unwrap_or_else(|| "groq".to_string());

    info!(model = %model, provider = %provider_name, "Chat completion request");

    // ── 1. JIT Karpathy Compiler — inject known strategies ───────────────
    if let Some(last_msg) = req.messages.last() {
        if last_msg.role == "user" {
            let error_hash = compute_error_hash(&last_msg.content);
            if let Ok(Some(strategy)) = state.tantivy.lookup_by_hash(&error_hash) {
                req.messages.insert(0, ChatMessage {
                    role: "system".to_string(),
                    content: format!(
                        "## Olympus JIT Strategy\nApply this verified fix:\n\n{}\n---",
                        strategy
                    ),
                });
                info!(hash = %error_hash, "JIT strategy injected from Tantivy");
            }
        }
    }

    // ── 2. Get next available key + resolve provider URL ─────────────────
    let (api_key, api_url) = {
        let mut pool = state.keypool.write().await;

        let mut key_result = pool.get_next_available(&provider_name, &model);
        if key_result.is_none() {
            key_result = pool.get_any_key_for_provider(&provider_name);
        }

        match key_result {
            Some((key, _)) => {
                // Look up the provider's base_url from the dynamic registry
                let url = pool.get_provider(&provider_name)
                    .map(|p| p.chat_url())
                    .unwrap_or_else(|| {
                        // Fallback: treat provider_name as a base_url directly
                        format!("{}/chat/completions", provider_name)
                    });
                (key, url)
            }
            None => {
                warn!(provider = %provider_name, model = %model, "No available keys");
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(json!({
                        "error": "No available API keys for this provider/model",
                        "hint": "Inject keys via POST /v1/admin/keys or add a provider via POST /v1/admin/providers",
                        "code": "NO_KEYS_AVAILABLE"
                    })),
                ).into_response();
            }
        }
    };

    // ── 3. Build extra headers for this provider ─────────────────────────
    let extra_headers: Vec<(String, String)> = {
        let pool = state.keypool.read().await;
        pool.get_provider(&provider_name)
            .map(|p| p.extra_headers.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    };

    // ── 4. Forward to LLM provider via proxy ─────────────────────────────
    let client = state.proxy.get_client();
    let payload = json!({
        "model": model,
        "messages": req.messages,
        "max_tokens": req.max_tokens.unwrap_or(4096),
        "temperature": req.temperature.unwrap_or(0.7),
    });

    let mut request_builder = client
        .post(&api_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json");

    for (k, v) in extra_headers {
        request_builder = request_builder.header(k, v);
    }

    match request_builder.json(&payload).send().await {
        Ok(r) => {
            let status = r.status();
            if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
                let mut pool = state.keypool.write().await;
                pool.mark_exhausted(&provider_name, &model, &api_key[..8.min(api_key.len())]);
                warn!(model = %model, "Key rate-limited — 15min jitter timeout applied");
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(json!({"error": "Rate limited — key rotated, retry request"})),
                ).into_response();
            }
            if status == reqwest::StatusCode::UNAUTHORIZED {
                let mut pool = state.keypool.write().await;
                pool.mark_invalid(&provider_name, &model, &api_key[..8.min(api_key.len())]);
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": "API key rejected by provider — key disabled"})),
                ).into_response();
            }
            match r.json::<Value>().await {
                Ok(json_resp) => (StatusCode::OK, Json(json_resp)).into_response(),
                Err(e) => (
                    StatusCode::BAD_GATEWAY,
                    Json(json!({"error": format!("LLM response parse error: {}", e)})),
                ).into_response(),
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": format!("LLM provider unreachable: {}", e)})),
        ).into_response(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/admin/keys
// ─────────────────────────────────────────────────────────────────────────────

pub async fn add_keys_handler(
    headers: HeaderMap,
    Extension(state): Extension<Arc<AppState>>,
    Json(req): Json<AddKeyRequest>,
) -> impl IntoResponse {
    if !check_admin_auth(&headers, &state.config) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid admin token"}))).into_response();
    }

    let key_prefix = if req.key.len() >= 8 { &req.key[..8] } else { &req.key };
    info!(provider = %req.provider, model = %req.model, key_prefix = %key_prefix, "Admin: injecting key");

    let result = {
        let mut pool = state.keypool.write().await;
        pool.add_key(req.key, req.provider.clone(), req.model.clone())
    };

    match result {
        Ok(()) => {
            let stats = state.keypool.read().await.stats();
            (StatusCode::OK, Json(json!({
                "status": "key_added",
                "provider": req.provider,
                "model": req.model,
                "pool_stats": stats,
            }))).into_response()
        }
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": e,
                "hint": "Register the provider first via POST /v1/admin/providers"
            })),
        ).into_response(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/admin/providers  — NEW: Register any OpenAI-compatible provider
// ─────────────────────────────────────────────────────────────────────────────

pub async fn register_provider_handler(
    headers: HeaderMap,
    Extension(state): Extension<Arc<AppState>>,
    Json(req): Json<RegisterProviderRequest>,
) -> impl IntoResponse {
    if !check_admin_auth(&headers, &state.config) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Invalid admin token"}))).into_response();
    }

    info!(name = %req.name, base_url = %req.base_url, "Admin: registering new provider");

    let config = crate::keypool::ProviderConfig {
        name: req.name.clone(),
        base_url: req.base_url.clone(),
        extra_headers: req.extra_headers,
    };

    {
        let mut pool = state.keypool.write().await;
        pool.register_provider(config);
    }

    let providers: Vec<String> = {
        let pool = state.keypool.read().await;
        pool.list_providers().iter().map(|p| p.name.clone()).collect()
    };

    (StatusCode::OK, Json(json!({
        "status": "provider_registered",
        "name": req.name,
        "base_url": req.base_url,
        "chat_endpoint": format!("{}/chat/completions", req.base_url.trim_end_matches('/')),
        "all_providers": providers,
    }))).into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/health
// ─────────────────────────────────────────────────────────────────────────────

pub async fn health_handler(
    Extension(state): Extension<Arc<AppState>>,
) -> impl IntoResponse {
    let cluster = state.memory.health().await;
    let all_healthy = cluster.all_healthy();
    let status = if all_healthy { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };

    let providers: Vec<String> = {
        let pool = state.keypool.read().await;
        pool.list_providers().iter().map(|p| p.name.clone()).collect()
    };

    (status, Json(json!({
        "status": if all_healthy { "healthy" } else { "degraded" },
        "gateway": "online",
        "version": "5.0.0",
        "registered_providers": providers,
        "cluster": {
            "space_2_l1": cluster.l1,
            "space_3_l3": cluster.l3,
            "space_4_l4": cluster.l4,
            "space_5_rnd": cluster.rnd,
        },
    }))).into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /v1/admin/status
// ─────────────────────────────────────────────────────────────────────────────

pub async fn admin_status_handler(
    headers: HeaderMap,
    Extension(state): Extension<Arc<AppState>>,
) -> impl IntoResponse {
    if !check_admin_auth(&headers, &state.config) {
        return (StatusCode::UNAUTHORIZED, Json(json!({"error": "Unauthorized"}))).into_response();
    }

    let keypool_stats = state.keypool.read().await.stats();
    let tantivy_docs = state.tantivy.doc_count();
    let cluster_health = state.memory.health().await;

    (StatusCode::OK, Json(json!({
        "keypool": keypool_stats,
        "tantivy": { "indexed_skills": tantivy_docs },
        "cluster": cluster_health,
        "proxy_count": state.proxy.proxy_count(),
    }))).into_response()
}

// ─────────────────────────────────────────────────────────────────────────────
// POST /v1/memory/error
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct PushErrorRequest {
    pub error: String,
    pub context: String,
    pub exit_code: Option<i32>,
    pub file: Option<String>,
    pub line: Option<u32>,
}

pub async fn push_error_handler(
    Extension(state): Extension<Arc<AppState>>,
    Json(req): Json<PushErrorRequest>,
) -> impl IntoResponse {
    let item = RndQueueItem {
        error: req.error.clone(),
        context: req.context,
        exit_code: req.exit_code.unwrap_or(1),
        file: req.file,
        line: req.line,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };

    let payload = serde_json::to_string(&item).unwrap_or_default();
    let error_hash = compute_error_hash(&req.error);

    match state.memory.push_rnd_error(&payload).await {
        Ok(depth) => (StatusCode::OK, Json(json!({
            "status": "queued",
            "error_hash": error_hash,
            "queue_depth": depth,
        }))).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({
            "error": format!("Failed to queue error: {}", e)
        }))).into_response(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// GET /dashboard
// ─────────────────────────────────────────────────────────────────────────────

pub async fn dashboard_handler() -> impl IntoResponse {
    Html(include_str!("../../dashboard/index.html"))
}

// ─────────────────────────────────────────────────────────────────────────────
// 404 fallback
// ─────────────────────────────────────────────────────────────────────────────

pub async fn fallback_handler() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(json!({"error": "Route not found", "version": "olympus-v5"})))
}
