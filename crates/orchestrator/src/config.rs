// crates/orchestrator/src/config.rs
// Runtime config loaded from env vars — all secrets injected, nothing hardcoded.

use anyhow::{anyhow, Result};

#[derive(Debug, Clone)]
pub struct OrchestratorConfig {
    // Gateway
    pub gateway_url: String,
    pub hf_token: String,
    pub admin_token: String,

    // Tier 1 & 2 — direct provider call (bypasses Gateway, uses its own key)
    pub tier12_provider: String,      // e.g. "gemini", "openrouter"
    pub tier12_api_key: String,       // provider key for Senator + Board
    pub tier12_model: String,         // e.g. "gemini-2.0-flash", "anthropic/claude-opus-4"

    // Tier 3 & 4 — routed through Gateway KeyPool
    pub tier34_provider: String,      // e.g. "groq"
    pub tier34_model: String,         // e.g. "llama3-70b-8192"

    // Workspace
    pub workspace_path: String,
}

impl OrchestratorConfig {
    pub fn from_env() -> Result<Self> {
        Ok(OrchestratorConfig {
            gateway_url: std::env::var("GATEWAY_URL")
                .unwrap_or_else(|_| "https://ultron-v5-olympus-gateway.hf.space".into()),

            hf_token: std::env::var("HF_TOKEN")
                .map_err(|_| anyhow!("HF_TOKEN is required"))?,

            admin_token: std::env::var("ADMIN_TOKEN")
                .unwrap_or_default(),

            // Tier 1/2 default: Gemini Flash (fast, free tier available)
            // You can switch to any provider just by changing these env vars
            tier12_provider: std::env::var("TIER12_PROVIDER")
                .unwrap_or_else(|_| "gemini".into()),

            tier12_api_key: std::env::var("TIER12_API_KEY")
                .map_err(|_| anyhow!("TIER12_API_KEY is required (your Gemini/OpenRouter/etc key)"))?,

            tier12_model: std::env::var("TIER12_MODEL")
                .unwrap_or_else(|_| "gemini-2.0-flash".into()),

            // Tier 3/4 routed through Gateway (uses pooled free-tier keys)
            tier34_provider: std::env::var("TIER34_PROVIDER")
                .unwrap_or_else(|_| "groq".into()),

            tier34_model: std::env::var("TIER34_MODEL")
                .unwrap_or_else(|_| "llama-3.3-70b-versatile".into()),

            workspace_path: std::env::var("WORKSPACE_PATH")
                .unwrap_or_else(|_| ".".into()),
        })
    }
}
