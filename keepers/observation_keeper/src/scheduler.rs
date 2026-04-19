use anyhow::Result;
use solana_sdk::pubkey::Pubkey;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::config::KeeperConfig;
use crate::rpc::KeeperClient;

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub async fn run_once(
    client: &KeeperClient,
    cfg: &KeeperConfig,
    policy_filter: Option<Pubkey>,
) -> Result<()> {
    let policies = client.policy_headers(policy_filter).await?;
    let mut fired = 0usize;

    for (policy_address, mut header) in policies {
        if header.product_program_id != client.sol_autocall_program
            || header.status != halcyon_kernel::state::PolicyStatus::Active
            || header.product_terms == Pubkey::default()
        {
            continue;
        }

        loop {
            let terms = client.product_terms(&header.product_terms).await?;
            if terms.status != halcyon_sol_autocall::state::ProductStatus::Active
                || terms.current_observation_index as usize
                    >= halcyon_sol_autocall::state::OBSERVATION_COUNT
            {
                break;
            }

            let expected_index = terms.current_observation_index;
            let due_at = terms.observation_schedule[expected_index as usize];
            if unix_now() < due_at {
                break;
            }

            let sig = client
                .send_record_observation(policy_address, &header, expected_index)
                .await?;
            fired += 1;
            info!(
                target = "scheduler",
                policy = %policy_address,
                expected_index,
                %sig,
                "recorded observation",
            );

            header = halcyon_client_sdk::decode::fetch_anchor_account::<
                halcyon_kernel::state::PolicyHeader,
            >(&client.rpc, &policy_address)
            .await?;
            if header.status != halcyon_kernel::state::PolicyStatus::Active {
                break;
            }
        }
    }

    info!(
        target = "scheduler",
        fired,
        scan_interval_secs = cfg.scan_interval_secs,
        "observation pass complete",
    );
    Ok(())
}

pub async fn run_forever(client: &KeeperClient, cfg: &KeeperConfig) -> Result<()> {
    let mut consecutive_failures: u32 = 0;
    let mut backoff_secs: u64 = 1;

    loop {
        match run_once(client, cfg, None).await {
            Ok(()) => {
                consecutive_failures = 0;
                backoff_secs = 1;
                sleep(Duration::from_secs(cfg.scan_interval_secs)).await;
            }
            Err(err) => {
                consecutive_failures += 1;
                error!(
                    target = "scheduler",
                    %err,
                    consecutive_failures,
                    "scheduler pass failed",
                );
                if consecutive_failures >= cfg.failure_budget {
                    warn!(
                        target = "scheduler",
                        failure_budget = cfg.failure_budget,
                        "failure budget exhausted; exiting for ops alert",
                    );
                    return Err(err);
                }
                sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(cfg.backoff_cap_secs);
            }
        }
    }
}
