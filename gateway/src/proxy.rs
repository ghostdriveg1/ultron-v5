// gateway/src/proxy.rs
//
// Proxy abstraction layer. Routes reqwest HTTP clients through a rotating list
// of HTTP/SOCKS5 proxies. Falls back to direct connection if list is empty.
// Each call to `get_client()` selects the next proxy in round-robin fashion.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use anyhow::Result;
use reqwest::Client;
use tracing::{info, warn};

pub struct ProxyRotator {
    proxies: Vec<String>,
    counter: AtomicUsize,
    /// Direct client (no proxy) as fallback
    direct_client: Client,
    /// Per-proxy clients (lazy-built)
    proxy_clients: Vec<Client>,
}

impl ProxyRotator {
    pub fn new(proxy_list: Vec<String>) -> Result<Self> {
        let direct_client = Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .use_rustls_tls()
            .user_agent("Olympus-Gateway/5.0")
            .build()?;

        let mut proxy_clients = Vec::new();
        for proxy_url in &proxy_list {
            match Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .use_rustls_tls()
                .user_agent("Olympus-Gateway/5.0")
                .proxy(reqwest::Proxy::all(proxy_url)?)
                .build()
            {
                Ok(client) => {
                    info!(proxy = %proxy_url, "Proxy client initialized");
                    proxy_clients.push(client);
                }
                Err(e) => {
                    warn!(proxy = %proxy_url, error = %e, "Failed to build proxy client — skipping");
                }
            }
        }

        if proxy_list.is_empty() {
            info!("No proxies configured — using direct connection");
        } else {
            info!(count = proxy_clients.len(), "Proxy rotator initialized");
        }

        Ok(ProxyRotator {
            proxies: proxy_list,
            counter: AtomicUsize::new(0),
            direct_client,
            proxy_clients,
        })
    }

    /// Get the next client in rotation
    pub fn get_client(&self) -> &Client {
        if self.proxy_clients.is_empty() {
            return &self.direct_client;
        }
        let idx = self.counter.fetch_add(1, Ordering::Relaxed) % self.proxy_clients.len();
        &self.proxy_clients[idx]
    }

    /// Get a plain direct client (used for internal Webdis calls that must not be proxied)
    pub fn direct_client(&self) -> &Client {
        &self.direct_client
    }

    pub fn proxy_count(&self) -> usize {
        self.proxy_clients.len()
    }
}

pub type SharedProxyRotator = Arc<ProxyRotator>;
