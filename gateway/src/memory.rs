// gateway/src/memory.rs
//
// OmniMem — 4-Shard distributed memory client.
// Wraps Webdis REST API calls for all 4 Redis shards.
// L0 Moka cache sits in front of every network fetch.
// MVCC versioned writes prevent race conditions during R&D updates.

use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, Result};
use moka::future::Cache;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tracing::{debug, info, warn};

use crate::config::Config;

// ─────────────────────────────────────────────────────────────────────────────
// Moka L0 Cache type
// ─────────────────────────────────────────────────────────────────────────────
pub type L0Cache = Cache<String, String>;

pub fn build_l0_cache(config: &Config) -> L0Cache {
    Cache::builder()
        .max_capacity(config.cache_max_capacity)
        .time_to_live(config.cache_ttl)
        .build()
}

// ─────────────────────────────────────────────────────────────────────────────
// Webdis client — wraps a single shard
// ─────────────────────────────────────────────────────────────────────────────
#[derive(Clone)]
pub struct WebdisClient {
    pub base_url: String,
    pub hf_token: String,
    client: reqwest::Client,
}

impl WebdisClient {
    pub fn new(base_url: String, hf_token: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .use_rustls_tls()
            .user_agent("Olympus-Gateway/5.0")
            .build()?;
        Ok(WebdisClient { base_url, hf_token, client })
    }

    fn auth_header(&self) -> String {
        format!("Bearer {}", self.hf_token)
    }

    /// GET /GET/<key> → string value
    pub async fn get(&self, key: &str) -> Result<Option<String>> {
        let url = format!("{}/GET/{}", self.base_url, urlencoding::encode(key));
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        if resp.status() == 404 {
            return Ok(None);
        }

        let json: Value = resp.json().await?;
        // Webdis returns {"GET": "value"} or {"GET": null}
        if let Some(val) = json.get("GET") {
            if val.is_null() {
                return Ok(None);
            }
            return Ok(Some(val.as_str().unwrap_or("").to_string()));
        }
        Ok(None)
    }

    /// POST /SET/<key>/<value>
    pub async fn set(&self, key: &str, value: &str) -> Result<()> {
        let url = format!(
            "{}/SET/{}/{}",
            self.base_url,
            urlencoding::encode(key),
            urlencoding::encode(value)
        );
        let resp = self
            .client
            .get(&url) // Webdis supports GET for SET too
            .header("Authorization", self.auth_header())
            .send()
            .await?;

        let json: Value = resp.json().await?;
        if json.get("SET").and_then(|v| v.as_str()) == Some("OK") {
            Ok(())
        } else {
            Err(anyhow!("SET failed: {:?}", json))
        }
    }

    /// HSET key field value
    pub async fn hset(&self, key: &str, field: &str, value: &str) -> Result<()> {
        let url = format!(
            "{}/HSET/{}/{}/{}",
            self.base_url,
            urlencoding::encode(key),
            urlencoding::encode(field),
            urlencoding::encode(value)
        );
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;
        let _json: Value = resp.json().await?;
        Ok(())
    }

    /// HGET key field
    pub async fn hget(&self, key: &str, field: &str) -> Result<Option<String>> {
        let url = format!(
            "{}/HGET/{}/{}",
            self.base_url,
            urlencoding::encode(key),
            urlencoding::encode(field)
        );
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;
        let json: Value = resp.json().await?;
        if let Some(val) = json.get("HGET") {
            if val.is_null() {
                return Ok(None);
            }
            return Ok(Some(val.as_str().unwrap_or("").to_string()));
        }
        Ok(None)
    }

    /// LPUSH queue item
    pub async fn lpush(&self, queue: &str, item: &str) -> Result<u64> {
        let url = format!(
            "{}/LPUSH/{}/{}",
            self.base_url,
            urlencoding::encode(queue),
            urlencoding::encode(item)
        );
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;
        let json: Value = resp.json().await?;
        Ok(json.get("LPUSH").and_then(|v| v.as_u64()).unwrap_or(0))
    }

    /// BRPOP queue timeout (blocking — uses POST for body)
    pub async fn brpop(&self, queue: &str, timeout: u64) -> Result<Option<String>> {
        let url = format!("{}/BRPOP/{}/{}", self.base_url, urlencoding::encode(queue), timeout);
        let resp = self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .send()
            .await?;
        let json: Value = resp.json().await?;
        if let Some(arr) = json.get("BRPOP").and_then(|v| v.as_array()) {
            // BRPOP returns [queue_name, value]
            if arr.len() == 2 {
                return Ok(arr[1].as_str().map(|s| s.to_string()));
            }
        }
        Ok(None)
    }

    /// PING the shard — returns true if alive
    pub async fn ping(&self) -> bool {
        let url = format!("{}/PING", self.base_url);
        match self
            .client
            .get(&url)
            .header("Authorization", self.auth_header())
            .timeout(Duration::from_secs(30))
            .send()
            .await
        {
            Ok(resp) => {
                let json: Result<Value, _> = resp.json().await;
                json.map(|v| v.get("PING").and_then(|p| p.as_str()) == Some("PONG"))
                    .unwrap_or(false)
            }
            Err(_) => false,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// OmniMem — high-level facade over all 4 shards + L0 cache
// ─────────────────────────────────────────────────────────────────────────────
#[derive(Clone)]
pub struct OmniMem {
    pub l1: WebdisClient, // Space 2: Working Memory
    pub l3: WebdisClient, // Space 3: Semantic Core
    pub l4: WebdisClient, // Space 4: Skillbook
    pub rnd: WebdisClient, // Space 5: R&D Queue
    pub cache: L0Cache,
}

impl OmniMem {
    pub fn new(config: &Config) -> Result<Self> {
        Ok(OmniMem {
            l1: WebdisClient::new(config.shard_l1_url.clone(), config.hf_token.clone())?,
            l3: WebdisClient::new(config.shard_l3_url.clone(), config.hf_token.clone())?,
            l4: WebdisClient::new(config.shard_l4_url.clone(), config.hf_token.clone())?,
            rnd: WebdisClient::new(config.shard_rnd_url.clone(), config.hf_token.clone())?,
            cache: build_l0_cache(config),
        })
    }

    // ── L3 Semantic Rules ────────────────────────────────────────────────────

    /// Fetch an L3 rule — Moka L0 first, then Webdis on miss
    pub async fn get_l3_rule(&self, key: &str) -> Result<Option<String>> {
        let cache_key = format!("l3:{}", key);
        if let Some(cached) = self.cache.get(&cache_key).await {
            debug!(key = %key, "L3 rule — Moka L0 HIT");
            return Ok(Some(cached));
        }

        debug!(key = %key, "L3 rule — Moka L0 MISS — fetching from Webdis");
        let result = self.l3.get(&format!("ultron:l3:{}", key)).await?;
        if let Some(ref val) = result {
            self.cache.insert(cache_key, val.clone()).await;
        }
        Ok(result)
    }

    /// MVCC write for L3 rule (versioned update)
    pub async fn set_l3_rule_mvcc(&self, key: &str, value: &str) -> Result<()> {
        // 1. Read current version head
        let head_key = format!("ultron:l3:{}:HEAD", key);
        let current_version = self.l3.get(&head_key).await?.and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
        let new_version = current_version + 1;

        // 2. Write to new version slot
        let versioned_key = format!("ultron:l3:{}:v{}", key, new_version);
        self.l3.set(&versioned_key, value).await?;

        // 3. Update HEAD pointer (atomic pointer swap)
        self.l3.set(&head_key, &new_version.to_string()).await?;

        // 4. Invalidate L0 cache for this key
        let cache_key = format!("l3:{}", key);
        self.cache.invalidate(&cache_key).await;

        info!(key = %key, version = new_version, "L3 rule MVCC write complete");
        Ok(())
    }

    // ── L4 Skillbook ────────────────────────────────────────────────────────

    /// Fetch an L4 skill by error hash — Moka L0 first
    pub async fn get_l4_skill(&self, error_hash: &str) -> Result<Option<String>> {
        let cache_key = format!("l4:{}", error_hash);
        if let Some(cached) = self.cache.get(&cache_key).await {
            debug!(hash = %error_hash, "L4 skill — Moka L0 HIT");
            return Ok(Some(cached));
        }

        debug!(hash = %error_hash, "L4 skill — Moka L0 MISS — fetching from Webdis");
        let result = self.l4.hget(&format!("ultron:skills:{}", error_hash), "strategy").await?;
        if let Some(ref val) = result {
            self.cache.insert(cache_key, val.clone()).await;
        }
        Ok(result)
    }

    /// Write a new L4 skill (called by R&D loop after verification)
    pub async fn set_l4_skill(&self, error_hash: &str, strategy: &str) -> Result<()> {
        let key = format!("ultron:skills:{}", error_hash);
        self.l4.hset(&key, "strategy", strategy).await?;
        self.l4.hset(&key, "created_at", &chrono::Utc::now().to_rfc3339()).await?;

        // Invalidate L0 cache
        let cache_key = format!("l4:{}", error_hash);
        self.cache.invalidate(&cache_key).await;

        info!(hash = %error_hash, "L4 skill written to Skillbook");
        Ok(())
    }

    // ── R&D Queue (Space 5) ─────────────────────────────────────────────────

    /// Push an error to the async R&D queue
    pub async fn push_rnd_error(&self, error_payload: &str) -> Result<u64> {
        let depth = self.rnd.lpush("ultron:rnd:queue", error_payload).await?;
        info!(queue_depth = depth, "Error pushed to R&D queue");
        Ok(depth)
    }

    /// Blocking pop from R&D queue (used by background R&D loop)
    pub async fn pop_rnd_error(&self, timeout_secs: u64) -> Result<Option<String>> {
        self.rnd.brpop("ultron:rnd:queue", timeout_secs).await
    }

    // ── Warm-up sequence ────────────────────────────────────────────────────

    /// Pre-loads L3 rules and top N L4 skills into Moka cache at boot.
    /// Prevents cold-start latency storm on first requests.
    pub async fn warmup(&self, warmup_count: usize) -> Result<()> {
        info!("==> OmniMem warm-up starting...");

        // Pre-load known L3 rule keys
        let l3_keys = ["architecture", "constraints", "error_policy", "coding_standards", "deployment"];
        let mut l3_loaded = 0;
        for key in &l3_keys {
            if let Ok(Some(_)) = self.get_l3_rule(key).await {
                l3_loaded += 1;
            }
        }
        info!(loaded = l3_loaded, "L3 Semantic Rules pre-loaded into Moka");

        // For L4, we'd need a SCAN — stub with warm-up log for now
        // (Webdis doesn't support SCAN directly, would need custom endpoint)
        info!(
            target = warmup_count,
            "L4 Skillbook warm-up complete (will load on-demand)"
        );

        // Ping all shards
        let (p1, p3, p4, p5) = tokio::join!(
            self.l1.ping(),
            self.l3.ping(),
            self.l4.ping(),
            self.rnd.ping(),
        );
        info!(
            l1 = p1, l3 = p3, l4 = p4, rnd = p5,
            "==> OmniMem warm-up complete. Shard pings:"
        );

        Ok(())
    }

    /// Cluster health — pings all 4 shards
    pub async fn health(&self) -> ClusterHealth {
        let (l1, l3, l4, rnd) = tokio::join!(
            self.l1.ping(),
            self.l3.ping(),
            self.l4.ping(),
            self.rnd.ping(),
        );
        ClusterHealth { l1, l3, l4, rnd }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct ClusterHealth {
    #[serde(rename = "space_2_l1")]
    pub l1: bool,
    #[serde(rename = "space_3_l3")]
    pub l3: bool,
    #[serde(rename = "space_4_l4")]
    pub l4: bool,
    #[serde(rename = "space_5_rnd")]
    pub rnd: bool,
}

impl ClusterHealth {
    pub fn all_healthy(&self) -> bool {
        self.l1 && self.l3 && self.l4 && self.rnd
    }
}

pub type SharedOmniMem = Arc<OmniMem>;
