use anyhow::{Context, Result};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use solana_sdk::signer::keypair::read_keypair_file;
use std::str::FromStr;
use std::sync::Arc;

/// Shared CLI context: RPC + optional signer. Reused by every subcommand.
pub struct CliContext {
    pub rpc: Arc<RpcClient>,
    pub signer: Option<Keypair>,
}

const HALCYON_USDC_MINT_ENV: &str = "HALCYON_USDC_MINT";
const USDC_MINT_ENV: &str = "USDC_MINT";

impl CliContext {
    pub async fn new(rpc_url: &str, keypair_path: Option<&str>) -> Result<Self> {
        let rpc = Arc::new(RpcClient::new_with_commitment(
            rpc_url.to_string(),
            CommitmentConfig::confirmed(),
        ));
        rpc.get_slot()
            .await
            .with_context(|| format!("pinging RPC at {rpc_url}"))?;
        let signer = match keypair_path {
            Some(p) => Some(
                read_keypair_file(p).map_err(|e| anyhow::anyhow!("reading keypair at {p}: {e}"))?,
            ),
            None => None,
        };
        Ok(Self { rpc, signer })
    }

    /// Return the signer or a descriptive error if `--keypair` wasn't supplied.
    pub fn signer(&self) -> Result<&Keypair> {
        self.signer
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("this subcommand requires --keypair"))
    }

    pub fn parse_pubkey(label: &str, input: &str) -> Result<Pubkey> {
        Pubkey::from_str(input).with_context(|| format!("parsing {label} pubkey {input}"))
    }

    pub fn resolve_usdc_mint(&self, input: Option<&str>) -> Result<Pubkey> {
        if let Some(value) = input.map(str::trim).filter(|value| !value.is_empty()) {
            return Self::parse_pubkey("usdc_mint", value);
        }
        for env_name in [HALCYON_USDC_MINT_ENV, USDC_MINT_ENV] {
            if let Ok(value) = std::env::var(env_name) {
                let value = value.trim();
                if !value.is_empty() {
                    return Self::parse_pubkey(env_name, value);
                }
            }
        }
        anyhow::bail!(
            "--usdc-mint is required; pass it explicitly or set {HALCYON_USDC_MINT_ENV}/{USDC_MINT_ENV}. Devnet demos should use the Halcyon mock-USDC mint created by the faucet bring-up."
        )
    }
}
