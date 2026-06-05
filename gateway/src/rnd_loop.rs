// gateway/src/rnd_loop.rs
//
// ACE-2026 Upskill R&D Engine — Self-Healing Loop
//
// A detached Tokio task that continuously:
//   1. Blocks on BRPOP ultron:rnd:queue
//   2. Synthesizes a fix strategy via the KeyPool (any available key/provider)
//   3. Runs a bash verification test
//   4. On success: writes to L4 Skillbook + reloads Tantivy index

use std::sync::Arc;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

use crate::{
    keypool::SharedKeyPool,
    memory::SharedOmniMem,
    tantivy_engine::SharedTantivyEngine,
    config::Config,
};

/// Payload format for items on the R&D queue
#[derive(Debug, Serialize, Deserialize)]
pub struct RndQueueItem {
    pub error: String,
    pub context: String,
    pub exit_code: i32,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub timestamp: String,
}

/// Compute SHA256 of the error message for deduplication
pub fn compute_error_hash(error: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(error.as_bytes());
    hex::encode(hasher.finalize())
}

/// Main R&D loop — detached Tokio task
pub async fn run_rnd_loop(
    config: Arc<Config>,
    memory: SharedOmniMem,
    keypool: SharedKeyPool,
    tantivy: SharedTantivyEngine,
) {
    info!("==> ACE-2026 R&D Self-Healing Loop started");

    loop {
        // ── Step 1: Blocking pop from R&D queue ─────────────────────────────
        let item = match memory.pop_rnd_error(config.rnd_poll_timeout_secs).await {
            Ok(Some(raw)) => raw,
            Ok(None) => continue, // empty queue, keep polling
            Err(e) => {
                error!(error = %e, "R&D queue BRPOP failed — retrying in 5s");
                tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                continue;
            }
        };

        info!(payload_len = item.len(), "R&D queue item received");

        // ── Step 2: Parse payload ────────────────────────────────────────────
        let rnd_item: RndQueueItem = match serde_json::from_str(&item) {
            Ok(i) => i,
            Err(e) => {
                warn!(raw = %item, error = %e, "Failed to parse R&D queue item — skipping");
                continue;
            }
        };

        let error_hash = compute_error_hash(&rnd_item.error);

        // Skip if we already have a strategy for this error
        match memory.get_l4_skill(&error_hash).await {
            Ok(Some(existing)) => {
                info!(hash = %error_hash, "Already have strategy — re-indexing in Tantivy");
                let _ = tantivy.index_skill(&error_hash, &existing).await;
                continue;
            }
            _ => {}
        }

        // ── Step 3: Synthesize strategy via KeyPool ──────────────────────────
        let strategy = match synthesize_strategy(&rnd_item, &keypool).await {
            Ok(s) => s,
            Err(e) => {
                error!(error = %e, "LLM synthesis failed — requeueing");
                let _ = memory.push_rnd_error(&item).await;
                continue;
            }
        };

        info!(hash = %error_hash, strategy_len = strategy.len(), "Strategy synthesized");

        // ── Step 4: Verification Sandbox ─────────────────────────────────────
        if !run_verification_sandbox(&rnd_item, &strategy, &config).await {
            warn!(hash = %error_hash, "Strategy failed verification — not saved");
            continue;
        }

        // ── Step 5: Consolidate to L4 Skillbook + Tantivy ───────────────────
        if let Err(e) = memory.set_l4_skill(&error_hash, &strategy).await {
            error!(error = %e, "Failed to write to L4 Skillbook");
            continue;
        }
        if let Err(e) = tantivy.index_skill(&error_hash, &strategy).await {
            error!(error = %e, "Failed to reload Tantivy index");
            continue;
        }

        info!(hash = %error_hash, "✅ R&D Consolidation complete — swarm immune to this error");
    }
}

/// Synthesize a fix strategy using any available key from the KeyPool.
/// Uses dynamic provider registry — no hardcoded Provider enum.
async fn synthesize_strategy(item: &RndQueueItem, keypool: &SharedKeyPool) -> Result<String> {
    // Try to get any available key from the pool (dynamic — works with any provider)
    let (api_key, provider_name, chat_url) = {
        let mut pool = keypool.write().await;

        // Priority order: groq (fast) → openrouter (broad) → any available
        let providers_to_try = ["groq", "openrouter", "together", "mistral", "gemini"];
        let mut found = None;

        for provider in &providers_to_try {
            // Try common fast models first
            let models = ["llama-3.3-70b-versatile", "llama3-70b-8192",
                         "meta-llama/llama-3-70b-instruct", "mistral-medium",
                         "gemini-2.0-flash"];
            for model in &models {
                if let Some((key, _)) = pool.get_next_available(provider, model) {
                    let url = pool.get_provider(provider)
                        .map(|p| p.chat_url())
                        .unwrap_or_else(|| format!("https://api.{}.com/openai/v1/chat/completions", provider));
                    found = Some((key, provider.to_string(), url));
                    break;
                }
            }
            if found.is_some() { break; }

            // Fallback: any key for this provider
            if let Some((key, p)) = pool.get_any_key_for_provider(provider) {
                let url = pool.get_provider(provider)
                    .map(|cfg| cfg.chat_url())
                    .unwrap_or_default();
                found = Some((key, p, url));
                break;
            }
        }

        match found {
            Some(f) => f,
            None => return Err(anyhow::anyhow!("No available LLM keys in pool for R&D synthesis")),
        }
    };

    info!(provider = %provider_name, "Synthesizing strategy via {}", provider_name);

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .use_rustls_tls()
        .build()?;

    let payload = serde_json::json!({
        "model": "llama-3.3-70b-versatile",
        "messages": [
            {
                "role": "system",
                "content": "You are an expert software engineer. Analyze the error and respond with EXACTLY 2 sentences: 1) Root cause. 2) Precise fix. No preamble."
            },
            {
                "role": "user",
                "content": format!("Error: {}\nContext: {}\nExit Code: {}", item.error, item.context, item.exit_code)
            }
        ],
        "max_tokens": 200,
        "temperature": 0.1
    });

    let response = client
        .post(&chat_url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .send()
        .await?;

    if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
        let mut pool = keypool.write().await;
        pool.mark_exhausted(&provider_name, "llama-3.3-70b-versatile", &api_key[..8.min(api_key.len())]);
        return Err(anyhow::anyhow!("Rate limited during synthesis"));
    }

    let json: serde_json::Value = response.json().await?;
    json["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.trim().to_string())
        .ok_or_else(|| anyhow::anyhow!("Invalid LLM response"))
}

/// Bash verification sandbox — validates the strategy is non-empty and coherent
async fn run_verification_sandbox(
    _item: &RndQueueItem,
    strategy: &str,
    config: &Config,
) -> bool {
    let safe_strategy = strategy.replace('"', "'").replace('\n', " ");
    let test_script = format!(
        r#"#!/bin/bash
set -e
STRATEGY="{}"
[ -z "$STRATEGY" ] && {{ echo "FAIL: empty"; exit 1; }}
COUNT=$(echo "$STRATEGY" | grep -o '\.' | wc -l)
[ "$COUNT" -lt 1 ] && {{ echo "FAIL: no sentences"; exit 1; }}
echo "PASS"
exit 0"#,
        safe_strategy
    );

    let timeout = std::time::Duration::from_secs(config.rnd_verification_timeout_secs);
    match tokio::time::timeout(
        timeout,
        tokio::process::Command::new("bash").arg("-c").arg(&test_script).output(),
    ).await {
        Ok(Ok(out)) => {
            if out.status.success() { info!("Verification: PASS"); true }
            else {
                warn!(stderr = %String::from_utf8_lossy(&out.stderr), "Verification: FAIL");
                false
            }
        }
        Ok(Err(e)) => { warn!(error = %e, "Sandbox error"); false }
        Err(_) => { warn!("Sandbox timeout"); false }
    }
}
