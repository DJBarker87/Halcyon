use anyhow::{Context, Result};
use halcyon_client_sdk::{kernel, tx::send_instructions};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;
use std::sync::Arc;

use crate::config::KeeperConfig;

pub struct KeeperClient {
    pub rpc: Arc<RpcClient>,
    pub keypair: Keypair,
}

impl KeeperClient {
    pub async fn connect(cfg: &KeeperConfig) -> Result<Self> {
        let rpc = Arc::new(RpcClient::new_with_commitment(
            cfg.rpc_endpoint.clone(),
            CommitmentConfig::confirmed(),
        ));
        rpc.get_slot()
            .await
            .with_context(|| format!("pinging RPC at {}", cfg.rpc_endpoint))?;
        Ok(Self {
            rpc,
            keypair: cfg.load_keypair()?,
        })
    }

    pub async fn send_write_regime_signal(
        &self,
        args: halcyon_kernel::WriteRegimeSignalArgs,
    ) -> Result<Signature> {
        let ix = kernel::write_regime_signal_ix(
            &self.keypair.pubkey(),
            &self.keypair.pubkey(),
            &halcyon_il_protection::ID,
            args,
        );
        send_instructions(&self.rpc, &self.keypair, vec![ix]).await
    }
}
