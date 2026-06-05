// gateway/src/keypool.rs
//
// Universal KeyPool — Dynamic Provider Registry
//
// DESIGN PHILOSOPHY:
// Since every LLM provider in existence uses the OpenAI-compatible API format
// (POST /v1/chat/completions), we don't hardcode ANY provider.
// A provider is just: { name, base_url }
// Users register new providers at runtime via POST /v1/admin/providers.
// Then inject keys via POST /v1/admin/keys.
// Any future OpenAI-compatible provider works automatically — forever.

use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::RwLock;
use rand::Rng;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Provider Config — dynamically registered at runtime
// ─────────────────────────────────────────────────────────────────────────────

/// A registered LLM provider. Just a name + OpenAI-compatible base URL.
/// Works for ANY provider: Groq, OpenRouter, Gemini, DeepSeek, Mistral,
/// Together AI, Perplexity, Anyscale, Fireworks, or any future provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// Short identifier (e.g. "groq", "openrouter", "my-new-provider")
    pub name: String,
    /// OpenAI-compatible base URL — e.g. "https://api.groq.com/openai/v1"
    pub base_url: String,
    /// Optional: extra headers some providers require (e.g. HTTP-Referer)
    pub extra_headers: HashMap<String, String>,
}

impl ProviderConfig {
    /// Builds the full chat completions endpoint URL
    pub fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.base_url.trim_end_matches('/'))
    }
}

/// Request body for POST /v1/admin/providers
#[derive(Debug, Deserialize)]
pub struct RegisterProviderRequest {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// ApiKey — single key with circuit-breaker state
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ApiKey {
    pub key: String,
    pub provider_name: String, // matches ProviderConfig.name
    pub model: String,
    pub timeout_until: Option<Instant>,
    pub requests_served: u64,
    pub requests_failed: u64,
}

impl ApiKey {
    pub fn new(key: String, provider_name: String, model: String) -> Self {
        ApiKey {
            key,
            provider_name,
            model,
            timeout_until: None,
            requests_served: 0,
            requests_failed: 0,
        }
    }

    pub fn is_available(&self) -> bool {
        self.timeout_until
            .map(|t| Instant::now() > t)
            .unwrap_or(true)
    }

    /// 15-minute jittered cooldown on 429
    pub fn mark_exhausted(&mut self) {
        let base = Duration::from_secs(15 * 60);
        let jitter = Duration::from_secs(rand::thread_rng().gen_range(0..=120));
        self.timeout_until = Some(Instant::now() + base + jitter);
        self.requests_failed += 1;
        warn!(
            key_prefix = &self.key[..8.min(self.key.len())],
            provider = %self.provider_name,
            model = %self.model,
            "Key rate-limited — ~15min jittered timeout applied"
        );
    }

    /// Permanent disable (401 Unauthorized — key is invalid/revoked)
    pub fn mark_invalid(&mut self) {
        self.timeout_until = Some(Instant::now() + Duration::from_secs(365 * 24 * 3600));
        warn!(
            key_prefix = &self.key[..8.min(self.key.len())],
            "Key permanently disabled (401 Unauthorized)"
        );
    }

    pub fn time_until_available_secs(&self) -> Option<u64> {
        self.timeout_until.and_then(|t| {
            t.checked_duration_since(Instant::now())
                .map(|d| d.as_secs())
        })
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Bucket key: (provider_name, model_id) — model-homogeneous
// ─────────────────────────────────────────────────────────────────────────────
type BucketKey = (String, String);

// ─────────────────────────────────────────────────────────────────────────────
// KeyPool — the core registry
// ─────────────────────────────────────────────────────────────────────────────

pub struct KeyPool {
    /// Registered providers — keyed by provider name
    pub providers: HashMap<String, ProviderConfig>,
    /// Model-homogeneous key buckets — (provider_name, model) → deque of keys
    buckets: HashMap<BucketKey, VecDeque<ApiKey>>,
    /// Lifetime stats
    total_added: u64,
    total_requests: u64,
}

impl KeyPool {
    pub fn new() -> Self {
        let mut pool = KeyPool {
            providers: HashMap::new(),
            buckets: HashMap::new(),
            total_added: 0,
            total_requests: 0,
        };

        // Pre-register the most common OpenAI-compatible providers
        // Users can add more at runtime — no restart needed
        pool.register_provider(ProviderConfig {
            name: "groq".into(),
            base_url: "https://api.groq.com/openai/v1".into(),
            extra_headers: HashMap::new(),
        });
        pool.register_provider(ProviderConfig {
            name: "openrouter".into(),
            base_url: "https://openrouter.ai/api/v1".into(),
            extra_headers: [
                ("HTTP-Referer".into(), "https://ultron-v5-olympus-gateway.hf.space".into()),
                ("X-Title".into(), "Olympus V5 Gateway".into()),
            ].into(),
        });
        pool.register_provider(ProviderConfig {
            name: "gemini".into(),
            base_url: "https://generativelanguage.googleapis.com/v1beta/openai/v1".into(),
            extra_headers: HashMap::new(),
        });
        pool.register_provider(ProviderConfig {
            name: "together".into(),
            base_url: "https://api.together.xyz/v1".into(),
            extra_headers: HashMap::new(),
        });
        pool.register_provider(ProviderConfig {
            name: "mistral".into(),
            base_url: "https://api.mistral.ai/v1".into(),
            extra_headers: HashMap::new(),
        });
        pool.register_provider(ProviderConfig {
            name: "deepseek".into(),
            base_url: "https://api.deepseek.com/v1".into(),
            extra_headers: HashMap::new(),
        });
        pool.register_provider(ProviderConfig {
            name: "fireworks".into(),
            base_url: "https://api.fireworks.ai/inference/v1".into(),
            extra_headers: HashMap::new(),
        });
        pool.register_provider(ProviderConfig {
            name: "perplexity".into(),
            base_url: "https://api.perplexity.ai".into(),
            extra_headers: HashMap::new(),
        });

        pool
    }

    // ── Provider registration ────────────────────────────────────────────────

    /// Register a new provider — works for ANY OpenAI-compatible endpoint
    pub fn register_provider(&mut self, config: ProviderConfig) {
        info!(
            name = %config.name,
            base_url = %config.base_url,
            "Provider registered"
        );
        self.providers.insert(config.name.clone(), config);
    }

    /// Get provider config by name
    pub fn get_provider(&self, name: &str) -> Option<&ProviderConfig> {
        self.providers.get(name)
    }

    /// List all registered providers
    pub fn list_providers(&self) -> Vec<&ProviderConfig> {
        self.providers.values().collect()
    }

    // ── Key management ───────────────────────────────────────────────────────

    /// Add a key for any registered provider
    pub fn add_key(&mut self, key: String, provider_name: String, model: String) -> Result<(), String> {
        if !self.providers.contains_key(&provider_name) {
            return Err(format!(
                "Provider '{}' is not registered. Register it first via POST /v1/admin/providers",
                provider_name
            ));
        }
        let bucket_key = (provider_name.clone(), model.clone());
        let api_key = ApiKey::new(key.clone(), provider_name.clone(), model.clone());
        self.buckets
            .entry(bucket_key)
            .or_insert_with(VecDeque::new)
            .push_back(api_key);
        self.total_added += 1;
        info!(
            key_prefix = &key[..8.min(key.len())],
            provider = %provider_name,
            model = %model,
            total = self.total_added,
            "Key injected into pool"
        );
        Ok(())
    }

    /// Get next available key for a (provider, model) pair — round-robin rotation
    pub fn get_next_available(&mut self, provider_name: &str, model: &str) -> Option<(String, String)> {
        let bucket_key = (provider_name.to_string(), model.to_string());
        let bucket = self.buckets.get_mut(&bucket_key)?;

        let len = bucket.len();
        for _ in 0..len {
            if let Some(mut key) = bucket.pop_front() {
                if key.is_available() {
                    let key_str = key.key.clone();
                    key.requests_served += 1;
                    bucket.push_back(key);
                    self.total_requests += 1;
                    return Some((key_str, provider_name.to_string()));
                } else {
                    // Still cooling down — put back
                    bucket.push_front(key);
                }
            }
        }

        // No keys for exact model — try fallback to any available key for this provider
        None
    }

    /// Try to find ANY available key for a given provider (any model)
    /// Used as fallback when the exact model has no keys
    pub fn get_any_key_for_provider(&mut self, provider_name: &str) -> Option<(String, String)> {
        let matching_keys: Vec<BucketKey> = self.buckets
            .keys()
            .filter(|(p, _)| p == provider_name)
            .cloned()
            .collect();

        for bucket_key in matching_keys {
            if let Some(key_info) = self.get_next_available(&bucket_key.0, &bucket_key.1) {
                return Some(key_info);
            }
        }
        None
    }

    /// Mark a specific key as rate-limited (429)
    pub fn mark_exhausted(&mut self, provider_name: &str, model: &str, key_prefix: &str) {
        let bucket_key = (provider_name.to_string(), model.to_string());
        if let Some(bucket) = self.buckets.get_mut(&bucket_key) {
            for k in bucket.iter_mut() {
                if k.key.starts_with(key_prefix) {
                    k.mark_exhausted();
                    return;
                }
            }
        }
    }

    /// Mark a key as invalid (401)
    pub fn mark_invalid(&mut self, provider_name: &str, model: &str, key_prefix: &str) {
        let bucket_key = (provider_name.to_string(), model.to_string());
        if let Some(bucket) = self.buckets.get_mut(&bucket_key) {
            for k in bucket.iter_mut() {
                if k.key.starts_with(key_prefix) {
                    k.mark_invalid();
                    return;
                }
            }
        }
    }

    // ── Stats ────────────────────────────────────────────────────────────────

    pub fn stats(&self) -> KeyPoolStats {
        let mut total_keys = 0;
        let mut available_keys = 0;
        let mut buckets_info = Vec::new();

        for ((provider, model), bucket) in &self.buckets {
            let avail: usize = bucket.iter().filter(|k| k.is_available()).count();
            let cooling: Vec<_> = bucket.iter()
                .filter(|k| !k.is_available())
                .map(|k| k.time_until_available_secs().unwrap_or(0))
                .collect();

            total_keys += bucket.len();
            available_keys += avail;
            buckets_info.push(BucketInfo {
                provider: provider.clone(),
                model: model.clone(),
                total: bucket.len(),
                available: avail,
                cooling_down: cooling,
                total_served: bucket.iter().map(|k| k.requests_served).sum(),
            });
        }

        KeyPoolStats {
            total_keys,
            available_keys,
            total_added_ever: self.total_added,
            total_requests_routed: self.total_requests,
            registered_providers: self.providers.len(),
            buckets: buckets_info,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Serializable stats types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct KeyPoolStats {
    pub total_keys: usize,
    pub available_keys: usize,
    pub total_added_ever: u64,
    pub total_requests_routed: u64,
    pub registered_providers: usize,
    pub buckets: Vec<BucketInfo>,
}

#[derive(Debug, Serialize, Clone)]
pub struct BucketInfo {
    pub provider: String,
    pub model: String,
    pub total: usize,
    pub available: usize,
    pub cooling_down: Vec<u64>, // seconds until each cooling key is available
    pub total_served: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// API request types
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AddKeyRequest {
    pub provider: String,   // matches a registered provider name
    pub model: String,      // exact model ID for model-homogeneous bucketing
    pub key: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Thread-safe shared handle
// ─────────────────────────────────────────────────────────────────────────────

pub type SharedKeyPool = Arc<RwLock<KeyPool>>;

pub fn new_shared_keypool() -> SharedKeyPool {
    Arc::new(RwLock::new(KeyPool::new()))
}
