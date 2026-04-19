use anyhow::{Context, Result};
use serde::Deserialize;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::keypair::read_keypair_file;
use std::path::Path;

#[derive(Debug, Deserialize)]
pub struct KeeperConfig {
    pub rpc_endpoint: String,
    pub keypair_path: String,
    pub sol_autocall_program_id: String,
    pub usdc_mint: String,
    pub pyth_sol: String,
    #[serde(default = "default_scan_interval_secs")]
    pub scan_interval_secs: u64,
    #[serde(default = "default_backoff_cap_secs")]
    pub backoff_cap_secs: u64,
    #[serde(default = "default_failure_budget")]
    pub failure_budget: u32,
}

fn default_scan_interval_secs() -> u64 {
    60
}

fn default_backoff_cap_secs() -> u64 {
    60
}

fn default_failure_budget() -> u32 {
    5
}

impl KeeperConfig {
    pub fn load(path: &str) -> Result<Self> {
        let raw = std::fs::read_to_string(Path::new(path))
            .with_context(|| format!("reading observation-keeper config at {path}"))?;
        serde_json::from_str(&raw)
            .with_context(|| format!("parsing observation-keeper config at {path}"))
    }

    pub fn load_keypair(&self) -> Result<Keypair> {
        read_keypair_file(&self.keypair_path)
            .map_err(|e| anyhow::anyhow!("reading keypair at {}: {}", self.keypair_path, e))
    }
}
