use anyhow::{Context, Result};
use halcyon_client_sdk::{
    decode::{fetch_anchor_account, list_policy_headers_for_product},
    sol_autocall::record_observation_ix,
    tx::send_instructions,
};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::commitment_config::CommitmentConfig;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature};
use solana_sdk::signer::Signer;
use std::str::FromStr;
use std::sync::Arc;

use crate::config::KeeperConfig;

pub struct KeeperClient {
    pub rpc: Arc<RpcClient>,
    pub keypair: Keypair,
    pub sol_autocall_program: Pubkey,
    pub usdc_mint: Pubkey,
    pub pyth_sol: Pubkey,
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
            sol_autocall_program: Pubkey::from_str(&cfg.sol_autocall_program_id).with_context(
                || {
                    format!(
                        "parsing sol_autocall_program_id {}",
                        cfg.sol_autocall_program_id
                    )
                },
            )?,
            usdc_mint: Pubkey::from_str(&cfg.usdc_mint)
                .with_context(|| format!("parsing usdc_mint {}", cfg.usdc_mint))?,
            pyth_sol: Pubkey::from_str(&cfg.pyth_sol)
                .with_context(|| format!("parsing pyth_sol {}", cfg.pyth_sol))?,
        })
    }

    pub async fn policy_headers(
        &self,
        filter: Option<Pubkey>,
    ) -> Result<Vec<(Pubkey, halcyon_kernel::state::PolicyHeader)>> {
        match filter {
            Some(policy) => Ok(vec![(
                policy,
                fetch_anchor_account::<halcyon_kernel::state::PolicyHeader>(&self.rpc, &policy)
                    .await?,
            )]),
            None => list_policy_headers_for_product(&self.rpc, &self.sol_autocall_program).await,
        }
    }

    pub async fn product_terms(
        &self,
        address: &Pubkey,
    ) -> Result<halcyon_sol_autocall::state::SolAutocallTerms> {
        fetch_anchor_account(&self.rpc, address).await
    }

    pub async fn send_record_observation(
        &self,
        policy_address: Pubkey,
        policy: &halcyon_kernel::state::PolicyHeader,
        expected_index: u8,
    ) -> Result<Signature> {
        let ix = record_observation_ix(
            &self.keypair.pubkey(),
            &self.usdc_mint,
            self.pyth_sol,
            policy,
            policy_address,
            expected_index,
        );
        send_instructions(&self.rpc, &self.keypair, vec![ix]).await
    }
}
