// crates/orchestrator/src/gateway.rs
// Typed client for the Olympus V5 Gateway REST API.

use anyhow::Result;
use serde_json::Value;

#[derive(Clone)]
pub struct GatewayClient {
    base_url: String,
    hf_token: String,
    admin_token: String,
    client: reqwest::Client,
}

impl GatewayClient {
    pub fn new(base_url: String, hf_token: String, admin_token: String) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .use_rustls_tls()
            .build()?;
        Ok(GatewayClient { base_url, hf_token, admin_token, client })
    }

    pub async fn health(&self) -> Result<Value> {
        let r = self.client
            .get(format!("{}/v1/health", self.base_url))
            .bearer_auth(&self.hf_token)
            .send().await?;
        Ok(r.json().await?)
    }

    pub async fn status(&self) -> Result<Value> {
        let r = self.client
            .get(format!("{}/v1/admin/status", self.base_url))
            .bearer_auth(&self.admin_token)
            .send().await?;
        Ok(r.json().await?)
    }

    pub async fn inject_key(&self, provider: &str, model: &str, key: &str) -> Result<Value> {
        let r = self.client
            .post(format!("{}/v1/admin/keys", self.base_url))
            .bearer_auth(&self.admin_token)
            .json(&serde_json::json!({ "provider": provider, "model": model, "key": key }))
            .send().await?;
        Ok(r.json().await?)
    }

    pub async fn push_error(&self, error: &str, context: &str, exit_code: i32) -> Result<Value> {
        let r = self.client
            .post(format!("{}/v1/memory/error", self.base_url))
            .bearer_auth(&self.hf_token)
            .json(&serde_json::json!({ "error": error, "context": context, "exit_code": exit_code }))
            .send().await?;
        Ok(r.json().await?)
    }
}
