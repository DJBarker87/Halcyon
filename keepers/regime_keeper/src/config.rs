use anyhow::{Context, Result};
use serde::Deserialize;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::keypair::read_keypair_file;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct KeeperConfig {
    pub rpc_endpoint: String,
    pub keypair_path: String,
    #[serde(default = "default_history_url")]
    pub history_url: String,
    #[serde(default = "default_scan_interval_secs")]
    pub scan_interval_secs: u64,
    #[serde(default = "default_backoff_cap_secs")]
    pub backoff_cap_secs: u64,
    #[serde(default = "default_failure_budget")]
    pub failure_budget: u32,
}

fn default_history_url() -> String {
    "https://api.coingecko.com/api/v3/coins/solana/market_chart?vs_currency=usd&days=120&interval=daily".to_string()
}

fn default_scan_interval_secs() -> u64 {
    60 * 60
}

fn default_backoff_cap_secs() -> u64 {
    5 * 60
}

fn default_failure_budget() -> u32 {
    5
}

impl KeeperConfig {
    pub fn load(path: &str) -> Result<Self> {
        let raw = std::fs::read_to_string(Path::new(path))
            .with_context(|| format!("reading regime-keeper config at {path}"))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing regime-keeper config at {path}"))
    }

    pub fn load_keypair(&self) -> Result<Keypair> {
        read_keypair_file(&self.keypair_path)
            .map_err(|e| anyhow::anyhow!("reading keypair at {}: {}", self.keypair_path, e))
    }
}
