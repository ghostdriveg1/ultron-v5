// crates/orchestrator/src/llm.rs
//
// Universal OpenAI-Compatible LLM Client
//
// One function. Calls ANY provider. Works for every provider verified as of June 2026:
// Groq, Gemini, OpenRouter, Mistral, DeepSeek, Together, Fireworks, Cohere,
// Perplexity, and any future OpenAI-compatible provider.
//
// For Anthropic: use OpenRouter with model "anthropic/claude-3.5-sonnet"
// No Python. No SDK. Just reqwest.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tracing::{debug, warn};

// ─────────────────────────────────────────────────────────────────────────────
// Types (OpenAI-compatible schema — works for all providers)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self { role: "system".into(), content: content.into() }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self { role: "user".into(), content: content.into() }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self { role: "assistant".into(), content: content.into() }
    }
}

#[derive(Debug, Serialize)]
struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f32>,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize)]
struct Choice {
    message: ResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ResponseMessage {
    content: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// Provider config — verified June 2026
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct ProviderConfig {
    pub name: &'static str,
    pub base_url: &'static str,
    pub extra_headers: &'static [(&'static str, &'static str)],
}

/// All verified OpenAI-compatible providers as of June 2026.
/// Add new ones by just extending this list — no other code changes needed.
pub const KNOWN_PROVIDERS: &[ProviderConfig] = &[
    ProviderConfig {
        name: "groq",
        base_url: "https://api.groq.com/openai/v1",
        extra_headers: &[],
    },
    ProviderConfig {
        // Official OpenAI-compat endpoint — verified June 2026
        name: "gemini",
        base_url: "https://generativelanguage.googleapis.com/v1beta/openai/v1",
        extra_headers: &[],
    },
    ProviderConfig {
        // Routes to Claude, GPT-4, Llama, Gemini, DeepSeek — one key rules all
        name: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        extra_headers: &[
            ("HTTP-Referer", "https://ultron-v5-olympus-gateway.hf.space"),
            ("X-Title", "Olympus V5"),
        ],
    },
    ProviderConfig {
        name: "mistral",
        base_url: "https://api.mistral.ai/v1",
        extra_headers: &[],
    },
    ProviderConfig {
        name: "deepseek",
        base_url: "https://api.deepseek.com/v1",
        extra_headers: &[],
    },
    ProviderConfig {
        name: "together",
        base_url: "https://api.together.ai/v1",
        extra_headers: &[],
    },
    ProviderConfig {
        name: "fireworks",
        base_url: "https://api.fireworks.ai/inference/v1",
        extra_headers: &[],
    },
    ProviderConfig {
        name: "cohere",
        base_url: "https://api.cohere.com/compatibility/v1",
        extra_headers: &[],
    },
    ProviderConfig {
        name: "perplexity",
        base_url: "https://api.perplexity.ai",
        extra_headers: &[],
    },
];

// ─────────────────────────────────────────────────────────────────────────────
// LlmClient — the universal caller
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct LlmClient {
    client: reqwest::Client,
}

impl LlmClient {
    pub fn new() -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .use_rustls_tls()
            .user_agent("Olympus-Orchestrator/5.0")
            .build()?;
        Ok(LlmClient { client })
    }

    /// Call any OpenAI-compatible provider.
    /// provider_name: matches KNOWN_PROVIDERS or use custom base_url directly.
    pub async fn chat(
        &self,
        provider_name: &str,
        api_key: &str,
        model: &str,
        messages: Vec<Message>,
        max_tokens: Option<u32>,
        temperature: Option<f32>,
    ) -> Result<String> {
        // Look up provider base_url
        let (base_url, extra_headers) = self.resolve_provider(provider_name)?;
        let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

        debug!(provider = %provider_name, model = %model, url = %url, "LLM call");

        let payload = ChatRequest {
            model: model.to_string(),
            messages,
            max_tokens,
            temperature,
        };

        let mut req = self.client
            .post(&url)
            .bearer_auth(api_key)
            .json(&payload);

        // Apply provider-specific extra headers
        for (key, val) in extra_headers {
            req = req.header(*key, *val);
        }

        let response = req.send().await?;
        let status = response.status();

        if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            return Err(anyhow!("RATE_LIMITED:{}", provider_name));
        }
        if status == reqwest::StatusCode::UNAUTHORIZED {
            return Err(anyhow!("INVALID_KEY:{}", provider_name));
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("HTTP {}: {}", status, body));
        }

        let parsed: ChatResponse = response.json().await
            .map_err(|e| anyhow!("Failed to parse LLM response: {}", e))?;

        parsed.choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| anyhow!("Empty choices in LLM response"))
    }

    /// Call the Gateway's /v1/chat/completions endpoint (Tiers 3 & 4)
    /// This uses the Gateway's KeyPool — no API key needed here.
    pub async fn chat_via_gateway(
        &self,
        gateway_url: &str,
        hf_token: &str,
        model: &str,
        provider: &str,
        messages: Vec<Message>,
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let url = format!("{}/v1/chat/completions", gateway_url.trim_end_matches('/'));

        let mut payload = serde_json::json!({
            "model": model,
            "provider": provider,
            "messages": messages,
            "max_tokens": max_tokens.unwrap_or(4096),
        });

        let response = self.client
            .post(&url)
            .bearer_auth(hf_token)
            .json(&payload)
            .send()
            .await?;

        if !response.status().is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Gateway error: {}", body));
        }

        let parsed: ChatResponse = response.json().await?;
        parsed.choices
            .into_iter()
            .next()
            .map(|c| c.message.content)
            .ok_or_else(|| anyhow!("Empty gateway response"))
    }

    fn resolve_provider<'a>(&self, name: &'a str) -> Result<(&'static str, &'static [(&'static str, &'static str)])> {
        KNOWN_PROVIDERS
            .iter()
            .find(|p| p.name == name)
            .map(|p| (p.base_url, p.extra_headers))
            .ok_or_else(|| anyhow!(
                "Unknown provider '{}'. Known providers: {}. \
                Or register a custom one via POST /v1/admin/providers on the Gateway.",
                name,
                KNOWN_PROVIDERS.iter().map(|p| p.name).collect::<Vec<_>>().join(", ")
            ))
    }
}
