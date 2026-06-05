use reqwest::Client;
use std::time::Duration;

pub struct MemoryManager {
    http_client: Client,
    webdis_base_url_shard_a: String,
    webdis_base_url_shard_b: String,
    hf_bearer_token: String,
}

impl MemoryManager {
    pub fn new() -> Self {
        let http_client = Client::builder()
            .timeout(Duration::from_millis(500)) // Strict timeout to prevent blocking Gateway
            .build()
            .unwrap();

        // In production, these are loaded from secure environment variables.
        Self {
            http_client,
            webdis_base_url_shard_a: "https://mock-user-shard-a.hf.space".to_string(),
            webdis_base_url_shard_b: "https://mock-user-shard-b.hf.space".to_string(),
            hf_bearer_token: "hf_mock_token_strictly_private".to_string(),
        }
    }

    pub async fn fetch_from_webdis(&self, key: &str) -> Result<String, String> {
        // Decide routing based on key prefix (MVCC Versioned Reads)
        let target_url = if key.contains("ultron:l3") {
            format!("{}/GET/{}", self.webdis_base_url_shard_a, key)
        } else {
            format!("{}/GET/{}", self.webdis_base_url_shard_b, key)
        };

        let response = self.http_client
            .get(&target_url)
            .header("Authorization", format!("Bearer {}", self.hf_bearer_token))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if response.status().is_success() {
            // Webdis returns JSON like {"GET": "value"}
            let json: serde_json::Value = response.json().await.map_err(|e| e.to_string())?;
            if let Some(val) = json.get("GET") {
                return Ok(val.as_str().unwrap_or("").to_string());
            }
        }
        
        Err("Key not found or authentication failed".to_string())
    }
}
