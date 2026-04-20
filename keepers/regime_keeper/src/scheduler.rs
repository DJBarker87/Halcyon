use anyhow::Result;
use halcyon_client_sdk::{decode::fetch_anchor_account_opt, pda};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tracing::{error, info, warn};

use crate::config::KeeperConfig;
use crate::rpc::KeeperClient;

const REGIME_WRITE_MIN_GAP_SECS: i64 = 18 * 60 * 60;

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

pub async fn run_once(client: &KeeperClient, cfg: &KeeperConfig) -> Result<()> {
    let now = unix_now();
    if let Some(signal) = fetch_anchor_account_opt::<halcyon_kernel::state::RegimeSignal>(
        &client.rpc,
        &pda::regime_signal(&halcyon_il_protection::ID).0,
    )
    .await?
    {
        let age = now.saturating_sub(signal.last_update_ts);
        if age < REGIME_WRITE_MIN_GAP_SECS {
            info!(
                target = "regime_keeper",
                age_secs = age,
                min_gap_secs = REGIME_WRITE_MIN_GAP_SECS,
                "skipping regime write; previous signal is still within the minimum gap",
            );
            return Ok(());
        }
    }

    let fvol_s6 = fetch_fvol_s6(&cfg.history_url).await?;
    let regime = halcyon_il_quote::classify_regime_from_fvol_s6(fvol_s6);
    let sig = client
        .send_write_regime_signal(halcyon_kernel::WriteRegimeSignalArgs {
            product_program_id: halcyon_il_protection::ID,
            fvol_s6,
            regime: regime.regime as u8,
            sigma_multiplier_s6: regime.sigma_multiplier_s6,
            sigma_floor_annualised_s6: regime.sigma_floor_annualised_s6,
        })
        .await?;
    info!(
        target = "regime_keeper",
        fvol_s6,
        regime = ?regime.regime,
        sigma_multiplier_s6 = regime.sigma_multiplier_s6,
        %sig,
        "wrote IL Protection regime signal",
    );
    Ok(())
}

pub async fn run_forever(client: &KeeperClient, cfg: &KeeperConfig) -> Result<()> {
    let mut consecutive_failures: u32 = 0;
    let mut backoff_secs: u64 = 1;

    loop {
        match run_once(client, cfg).await {
            Ok(()) => {
                consecutive_failures = 0;
                backoff_secs = 1;
                sleep(Duration::from_secs(cfg.scan_interval_secs)).await;
            }
            Err(err) => {
                consecutive_failures += 1;
                error!(
                    target = "regime_keeper",
                    %err,
                    consecutive_failures,
                    "regime pass failed",
                );
                if consecutive_failures >= cfg.failure_budget {
                    warn!(
                        target = "regime_keeper",
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

#[derive(serde::Deserialize)]
struct CoinGeckoMarketChart {
    prices: Vec<(f64, f64)>,
}

async fn fetch_fvol_s6(history_url: &str) -> Result<i64> {
    let response = reqwest::get(history_url).await?.error_for_status()?;
    let chart: CoinGeckoMarketChart = response.json().await?;
    let closes = chart
        .prices
        .into_iter()
        .map(|(_, price)| price)
        .collect::<Vec<_>>();
    let fvol = halcyon_il_quote::compute_fvol_from_daily_closes(&closes)
        .ok_or_else(|| anyhow::anyhow!("insufficient or invalid price history for fvol"))?;
    Ok((fvol * 1_000_000.0).round() as i64)
}
