use parking_lot::RwLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub struct ProviderNode {
    pub provider_name: String,
    pub base_url: String,
    pub api_key: String,
    pub model_id: String,
    pub fail_count: AtomicU32,
    pub cooldown_end: RwLock<Option<Instant>>,
}

pub struct KeyPool {
    pub nodes: Vec<Arc<ProviderNode>>,
}

impl KeyPool {
    pub fn new() -> Self {
        // Mocked initial keys. In production, these pull from ENV variables mapped securely to the HF Space.
        let nodes = vec![
            Arc::new(ProviderNode {
                provider_name: "Groq".to_string(),
                base_url: "https://api.groq.com/openai/v1".to_string(),
                api_key: "gsk_mock_groq_key".to_string(),
                model_id: "llama3-70b-8192".to_string(),
                fail_count: AtomicU32::new(0),
                cooldown_end: RwLock::new(None),
            }),
            Arc::new(ProviderNode {
                provider_name: "OpenRouter".to_string(),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                api_key: "sk-or-mock_key".to_string(),
                model_id: "meta-llama/llama-3-70b-instruct".to_string(), // Strictly matching model architectures
                fail_count: AtomicU32::new(0),
                cooldown_end: RwLock::new(None),
            }),
        ];

        Self { nodes }
    }

    pub fn get_optimal_provider(&self, requested_model: &str) -> Option<Arc<ProviderNode>> {
        // Iterate through the fallback chain to find the first healthy node matching the model type
        for node in &self.nodes {
            if node.model_id.contains(requested_model) || requested_model.contains("llama") {
                let cooldown = node.cooldown_end.read();
                if let Some(end_time) = *cooldown {
                    if Instant::now() < end_time {
                        continue; // Node is currently rate-limited, skip
                    }
                }
                return Some(node.clone());
            }
        }
        None
    }

    pub fn mark_failure(&self, node: &ProviderNode) {
        let count = node.fail_count.fetch_add(1, Ordering::SeqCst);
        if count >= 3 {
            // Trip the circuit breaker: Add 15 minutes of cooldown + jitter
            let mut cooldown = node.cooldown_end.write();
            *cooldown = Some(Instant::now() + Duration::from_secs(900));
            node.fail_count.store(0, Ordering::SeqCst); // Reset count for the next cycle
        }
    }
}
