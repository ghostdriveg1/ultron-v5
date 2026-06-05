// gateway/src/config.rs
// Runtime configuration loaded from environment variables.
// No secrets are hardcoded — all injected at runtime.

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct Config {
    // Server
    pub port: u16,
    pub admin_token: String,

    // OMNIMEM Shard URLs (Webdis REST endpoints)
    pub shard_l1_url: String, // Space 2 — Working Memory
    pub shard_l3_url: String, // Space 3 — Semantic Core
    pub shard_l4_url: String, // Space 4 — Skillbook
    pub shard_rnd_url: String, // Space 5 — R&D Queue

    // HF Bearer token for private Space auth
    pub hf_token: String,

    // Moka cache TTL
    pub cache_ttl: Duration,
    pub cache_max_capacity: u64,

    // Warm-up: how many top skills to pre-fetch on boot
    pub warmup_skill_count: usize,

    // Proxy list (optional, comma-separated)
    pub proxy_list: Vec<String>,

    // R&D Loop
    pub rnd_poll_timeout_secs: u64,
    pub rnd_verification_timeout_secs: u64,
}

impl Config {
    pub fn from_env() -> Self {
        let proxy_str = std::env::var("PROXY_LIST").unwrap_or_default();
        let proxy_list: Vec<String> = if proxy_str.is_empty() {
            vec![]
        } else {
            proxy_str.split(',').map(|s| s.trim().to_string()).collect()
        };

        Config {
            port: std::env::var("PORT")
                .unwrap_or_else(|_| "7860".to_string())
                .parse()
                .expect("PORT must be a number"),

            admin_token: std::env::var("ADMIN_TOKEN")
                .expect("ADMIN_TOKEN env var is required"),

            shard_l1_url: std::env::var("SHARD_L1_URL")
                .unwrap_or_else(|_| "https://ultron-v5-olympus-memory-l1.hf.space".to_string()),

            shard_l3_url: std::env::var("SHARD_L3_URL")
                .unwrap_or_else(|_| "https://ultron-v5-olympus-memory-l3.hf.space".to_string()),

            shard_l4_url: std::env::var("SHARD_L4_URL")
                .unwrap_or_else(|_| "https://ultron-v5-olympus-memory-l4.hf.space".to_string()),

            shard_rnd_url: std::env::var("SHARD_RND_URL")
                .unwrap_or_else(|_| "https://ultron-v5-olympus-memory-rnd.hf.space".to_string()),

            hf_token: std::env::var("HF_TOKEN")
                .expect("HF_TOKEN env var is required"),

            cache_ttl: Duration::from_secs(
                std::env::var("CACHE_TTL_SECS")
                    .unwrap_or_else(|_| "300".to_string())
                    .parse()
                    .unwrap_or(300),
            ),

            cache_max_capacity: std::env::var("CACHE_MAX_CAPACITY")
                .unwrap_or_else(|_| "10000".to_string())
                .parse()
                .unwrap_or(10_000),

            warmup_skill_count: std::env::var("WARMUP_SKILL_COUNT")
                .unwrap_or_else(|_| "100".to_string())
                .parse()
                .unwrap_or(100),

            proxy_list,

            rnd_poll_timeout_secs: 30,
            rnd_verification_timeout_secs: 60,
        }
    }
}
