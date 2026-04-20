use anchor_lang::AccountDeserialize;
use anyhow::{Context, Result};
use clap::Parser;
use halcyon_client_sdk::{
    decode::{
        decode_anchor_account, fetch_anchor_account, fetch_multiple_accounts,
        list_policy_headers_for_product,
    },
    kernel, pda,
    tx::send_instructions,
};
use pyth_solana_receiver_sdk::price_update::{PriceUpdateV2, VerificationLevel};
use serde::{Deserialize, Serialize};
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    commitment_config::CommitmentConfig,
    hash::hashv,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{error, info, warn};

const MAX_PYTH_CLOCK_SKEW_SECS: i64 = 5;

#[derive(Parser, Debug)]
#[command(
    name = "delta_keeper",
    about = "Halcyon flagship aggregate delta keeper"
)]
struct Args {
    #[arg(long, default_value = "config/delta_keeper.json")]
    config: String,

    #[arg(long)]
    once: bool,
}

#[derive(Debug, Deserialize)]
struct KeeperConfig {
    rpc_endpoint: String,
    keypair_path: String,
    pyth_spy: String,
    pyth_qqq: String,
    pyth_iwm: String,
    #[serde(default = "default_merkle_output_path")]
    merkle_output_path: String,
    #[serde(default = "default_scan_interval_secs")]
    scan_interval_secs: u64,
    #[serde(default = "default_backoff_cap_secs")]
    backoff_cap_secs: u64,
    #[serde(default = "default_failure_budget")]
    failure_budget: u32,
}

fn default_merkle_output_path() -> String {
    "/tmp/halcyon_flagship_delta.json".to_string()
}

fn default_scan_interval_secs() -> u64 {
    30
}

fn default_backoff_cap_secs() -> u64 {
    60
}

fn default_failure_budget() -> u32 {
    5
}

impl KeeperConfig {
    fn load(path: &str) -> Result<Self> {
        let raw = std::fs::read_to_string(Path::new(path))
            .with_context(|| format!("reading delta-keeper config at {path}"))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing delta-keeper config at {path}"))
    }

    fn load_keypair(&self) -> Result<Keypair> {
        solana_sdk::signer::keypair::read_keypair_file(&self.keypair_path)
            .map_err(|e| anyhow::anyhow!("reading keypair at {}: {}", self.keypair_path, e))
    }
}

struct KeeperClient {
    rpc: Arc<RpcClient>,
    signer: Keypair,
    pyth_spy: Pubkey,
    pyth_qqq: Pubkey,
    pyth_iwm: Pubkey,
}

impl KeeperClient {
    async fn connect(cfg: &KeeperConfig) -> Result<Self> {
        let rpc = Arc::new(RpcClient::new_with_commitment(
            cfg.rpc_endpoint.clone(),
            CommitmentConfig::confirmed(),
        ));
        rpc.get_slot()
            .await
            .with_context(|| format!("pinging RPC at {}", cfg.rpc_endpoint))?;
        Ok(Self {
            rpc,
            signer: cfg.load_keypair()?,
            pyth_spy: Pubkey::from_str(&cfg.pyth_spy)
                .with_context(|| format!("parsing pyth_spy {}", cfg.pyth_spy))?,
            pyth_qqq: Pubkey::from_str(&cfg.pyth_qqq)
                .with_context(|| format!("parsing pyth_qqq {}", cfg.pyth_qqq))?,
            pyth_iwm: Pubkey::from_str(&cfg.pyth_iwm)
                .with_context(|| format!("parsing pyth_iwm {}", cfg.pyth_iwm))?,
        })
    }

    async fn send_write_aggregate_delta(
        &self,
        args: halcyon_kernel::WriteAggregateDeltaArgs,
    ) -> Result<Signature> {
        let ix =
            kernel::write_aggregate_delta_ix(&self.signer.pubkey(), &self.signer.pubkey(), args);
        send_instructions(&self.rpc, &self.signer, vec![ix]).await
    }
}

#[derive(Debug, Serialize)]
struct DeltaLeafRecord {
    policy: String,
    terms: String,
    ki_latched: bool,
    next_coupon_index: u8,
    next_autocall_index: u8,
    delta_spy_s6: i64,
    delta_qqq_s6: i64,
    delta_iwm_s6: i64,
}

#[derive(Debug, Serialize)]
struct DeltaArtifact {
    product_program_id: String,
    generated_at_ts: i64,
    spot_spy_s6: i64,
    spot_qqq_s6: i64,
    spot_iwm_s6: i64,
    spy_publish_time: i64,
    qqq_publish_time: i64,
    iwm_publish_time: i64,
    delta_spy_s6: i64,
    delta_qqq_s6: i64,
    delta_iwm_s6: i64,
    merkle_root_hex: String,
    note_count: usize,
    notes: Vec<DeltaLeafRecord>,
}

#[derive(Debug, Serialize)]
struct SignedDeltaArtifact {
    artifact: DeltaArtifact,
    artifact_sha256_hex: String,
    signer_pubkey: String,
    artifact_signature_base58: String,
}

#[derive(Debug, Clone, Copy)]
struct PythSnapshot {
    price_s6: i64,
    publish_time: i64,
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    let cfg = KeeperConfig::load(&args.config)?;
    let client = KeeperClient::connect(&cfg).await?;

    info!(
        target = "delta_keeper",
        endpoint = %cfg.rpc_endpoint,
        product = %halcyon_flagship_autocall::ID,
        "delta keeper starting",
    );

    if args.once {
        run_once(&client, &cfg).await?;
        return Ok(());
    }

    let shutdown = tokio::signal::ctrl_c();
    tokio::select! {
        result = run_forever(&client, &cfg) => {
            warn!(target = "delta_keeper", ?result, "scheduler exited");
            result
        }
        _ = shutdown => {
            info!(target = "delta_keeper", "SIGINT received; shutting down");
            Ok(())
        }
    }
}

async fn run_forever(client: &KeeperClient, cfg: &KeeperConfig) -> Result<()> {
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
                    target = "delta_keeper",
                    %err,
                    consecutive_failures,
                    "delta pass failed",
                );
                if consecutive_failures >= cfg.failure_budget {
                    warn!(
                        target = "delta_keeper",
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

async fn run_once(client: &KeeperClient, cfg: &KeeperConfig) -> Result<()> {
    let generated_at_ts = unix_now();
    let protocol_config = fetch_anchor_account::<halcyon_kernel::state::ProtocolConfig>(
        &client.rpc,
        &pda::protocol_config().0,
    )
    .await?;
    let spot_spy = read_pyth_price_s6(
        &client.rpc,
        &client.pyth_spy,
        &halcyon_oracles::feed_ids::SPY_USD,
        generated_at_ts,
        protocol_config.pyth_quote_staleness_cap_secs,
    )
    .await?;
    let spot_qqq = read_pyth_price_s6(
        &client.rpc,
        &client.pyth_qqq,
        &halcyon_oracles::feed_ids::QQQ_USD,
        generated_at_ts,
        protocol_config.pyth_quote_staleness_cap_secs,
    )
    .await?;
    let spot_iwm = read_pyth_price_s6(
        &client.rpc,
        &client.pyth_iwm,
        &halcyon_oracles::feed_ids::IWM_USD,
        generated_at_ts,
        protocol_config.pyth_quote_staleness_cap_secs,
    )
    .await?;
    let vault_sigma = fetch_anchor_account::<halcyon_kernel::state::VaultSigma>(
        &client.rpc,
        &pda::vault_sigma(&halcyon_flagship_autocall::ID).0,
    )
    .await?;
    let sigma_pricing_s6 = halcyon_flagship_autocall::pricing::compose_pricing_sigma(
        &vault_sigma,
        protocol_config.sigma_floor_annualised_s6,
    )?;

    let mut policies = list_policy_headers_for_product(&client.rpc, &halcyon_flagship_autocall::ID)
        .await?
        .into_iter()
        .filter(|(_, header)| header.status == halcyon_kernel::state::PolicyStatus::Active)
        .collect::<Vec<_>>();
    policies.sort_by_key(|(address, _)| address.to_bytes());

    let term_addresses = policies
        .iter()
        .map(|(_, header)| header.product_terms)
        .collect::<Vec<_>>();
    let term_accounts = fetch_multiple_accounts(&client.rpc, &term_addresses).await?;

    let mut notes = Vec::new();
    let mut leaf_hashes = Vec::new();
    let mut agg_spy_s6 = 0i64;
    let mut agg_qqq_s6 = 0i64;
    let mut agg_iwm_s6 = 0i64;

    for ((policy_address, header), term_account) in
        policies.into_iter().zip(term_accounts.into_iter())
    {
        let Some(term_account) = term_account else {
            continue;
        };
        let terms: halcyon_flagship_autocall::state::FlagshipAutocallTerms =
            decode_anchor_account(&term_account.data)?;
        if terms.status != halcyon_flagship_autocall::state::ProductStatus::Active {
            continue;
        }

        let delta = halcyon_flagship_autocall::pricing::compute_live_delta_s6(
            &terms,
            sigma_pricing_s6,
            header.notional,
            spot_spy.price_s6,
            spot_qqq.price_s6,
            spot_iwm.price_s6,
        )?;
        agg_spy_s6 = agg_spy_s6
            .checked_add(delta.delta_spy_s6)
            .context("aggregate SPY delta overflow")?;
        agg_qqq_s6 = agg_qqq_s6
            .checked_add(delta.delta_qqq_s6)
            .context("aggregate QQQ delta overflow")?;
        agg_iwm_s6 = agg_iwm_s6
            .checked_add(delta.delta_iwm_s6)
            .context("aggregate IWM delta overflow")?;

        notes.push(DeltaLeafRecord {
            policy: policy_address.to_string(),
            terms: header.product_terms.to_string(),
            ki_latched: terms.ki_latched,
            next_coupon_index: terms.next_coupon_index,
            next_autocall_index: terms.next_autocall_index,
            delta_spy_s6: delta.delta_spy_s6,
            delta_qqq_s6: delta.delta_qqq_s6,
            delta_iwm_s6: delta.delta_iwm_s6,
        });
        leaf_hashes.push(leaf_hash(
            &policy_address,
            delta.delta_spy_s6,
            delta.delta_qqq_s6,
            delta.delta_iwm_s6,
        ));
    }

    let merkle_root = merkle_root(&leaf_hashes);
    let artifact = DeltaArtifact {
        product_program_id: halcyon_flagship_autocall::ID.to_string(),
        generated_at_ts,
        spot_spy_s6: spot_spy.price_s6,
        spot_qqq_s6: spot_qqq.price_s6,
        spot_iwm_s6: spot_iwm.price_s6,
        spy_publish_time: spot_spy.publish_time,
        qqq_publish_time: spot_qqq.publish_time,
        iwm_publish_time: spot_iwm.publish_time,
        delta_spy_s6: agg_spy_s6,
        delta_qqq_s6: agg_qqq_s6,
        delta_iwm_s6: agg_iwm_s6,
        merkle_root_hex: hex_string(&merkle_root),
        note_count: notes.len(),
        notes,
    };
    let artifact_bytes =
        serde_json::to_vec_pretty(&artifact).context("serializing delta artifact payload")?;
    let signed_artifact = SignedDeltaArtifact {
        artifact,
        artifact_sha256_hex: hex_string(&hashv(&[&artifact_bytes]).to_bytes()),
        signer_pubkey: client.signer.pubkey().to_string(),
        artifact_signature_base58: client.signer.sign_message(&artifact_bytes).to_string(),
    };
    std::fs::write(
        &cfg.merkle_output_path,
        serde_json::to_vec_pretty(&signed_artifact).context("serializing signed delta artifact")?,
    )
    .with_context(|| format!("writing delta artifact to {}", cfg.merkle_output_path))?;

    let sig = client
        .send_write_aggregate_delta(halcyon_kernel::WriteAggregateDeltaArgs {
            product_program_id: halcyon_flagship_autocall::ID,
            delta_spy_s6: agg_spy_s6,
            delta_qqq_s6: agg_qqq_s6,
            delta_iwm_s6: agg_iwm_s6,
            merkle_root,
            spot_spy_s6: spot_spy.price_s6,
            spot_qqq_s6: spot_qqq.price_s6,
            spot_iwm_s6: spot_iwm.price_s6,
            live_note_count: u32::try_from(signed_artifact.artifact.note_count)
                .context("note_count overflow")?,
        })
        .await?;

    info!(
        target = "delta_keeper",
        notes = signed_artifact.artifact.note_count,
        delta_spy_s6 = agg_spy_s6,
        delta_qqq_s6 = agg_qqq_s6,
        delta_iwm_s6 = agg_iwm_s6,
        merkle_root = %signed_artifact.artifact.merkle_root_hex,
        %sig,
        "wrote flagship aggregate delta",
    );
    Ok(())
}

async fn read_pyth_price_s6(
    rpc: &RpcClient,
    address: &Pubkey,
    feed_id: &[u8; 32],
    now: i64,
    staleness_cap_secs: i64,
) -> Result<PythSnapshot> {
    let account = rpc
        .get_account(address)
        .await
        .with_context(|| format!("fetching Pyth account {address}"))?;
    anyhow::ensure!(
        account.owner == pyth_solana_receiver_sdk::ID,
        "unexpected Pyth owner {}",
        account.owner
    );
    let mut slice: &[u8] = &account.data;
    let update = PriceUpdateV2::try_deserialize(&mut slice)?;
    anyhow::ensure!(
        update.price_message.feed_id == *feed_id,
        "unexpected feed id"
    );
    anyhow::ensure!(
        matches!(update.verification_level, VerificationLevel::Full),
        "Pyth verification level is not Full"
    );
    validate_publish_time(now, update.price_message.publish_time, staleness_cap_secs)?;
    Ok(PythSnapshot {
        price_s6: rescale_to_s6(update.price_message.price, update.price_message.exponent)?,
        publish_time: update.price_message.publish_time,
    })
}

fn validate_publish_time(now: i64, publish_time: i64, staleness_cap_secs: i64) -> Result<()> {
    anyhow::ensure!(staleness_cap_secs > 0, "invalid Pyth staleness cap");
    anyhow::ensure!(publish_time > 0, "invalid Pyth publish time");
    anyhow::ensure!(
        publish_time <= now.saturating_add(MAX_PYTH_CLOCK_SKEW_SECS),
        "Pyth publish time is in the future"
    );
    anyhow::ensure!(
        now.saturating_sub(publish_time) <= staleness_cap_secs,
        "Pyth price is stale"
    );
    Ok(())
}

fn rescale_to_s6(value: i64, expo: i32) -> Result<i64> {
    let shift = expo.checked_add(6).context("expo shift overflow")?;
    if shift == 0 {
        return Ok(value);
    }
    if shift > 0 {
        return value
            .checked_mul(pow10_i64(shift as u32)?)
            .context("rescale overflow");
    }
    Ok(value / pow10_i64((-shift) as u32)?)
}

fn pow10_i64(n: u32) -> Result<i64> {
    let mut out = 1i64;
    for _ in 0..n {
        out = out.checked_mul(10).context("pow10 overflow")?;
    }
    Ok(out)
}

fn leaf_hash(policy: &Pubkey, delta_spy_s6: i64, delta_qqq_s6: i64, delta_iwm_s6: i64) -> [u8; 32] {
    hashv(&[
        b"flagship-delta-leaf",
        &policy.to_bytes(),
        &delta_spy_s6.to_le_bytes(),
        &delta_qqq_s6.to_le_bytes(),
        &delta_iwm_s6.to_le_bytes(),
    ])
    .to_bytes()
}

fn merkle_root(leaves: &[[u8; 32]]) -> [u8; 32] {
    if leaves.is_empty() {
        return [0u8; 32];
    }
    let mut level = leaves.to_vec();
    while level.len() > 1 {
        let mut next = Vec::with_capacity((level.len() + 1) / 2);
        let mut index = 0usize;
        while index < level.len() {
            let left = level[index];
            let right = if index + 1 < level.len() {
                level[index + 1]
            } else {
                left
            };
            next.push(hashv(&[b"flagship-delta-node", &left, &right]).to_bytes());
            index += 2;
        }
        level = next;
    }
    level[0]
}

fn hex_string(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn init_tracing() {
    use tracing_subscriber::{fmt, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    fmt()
        .with_env_filter(filter)
        .json()
        .with_current_span(false)
        .init();
}

fn unix_now() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::validate_publish_time;

    #[test]
    fn rejects_stale_publish_time() {
        let err = validate_publish_time(1_000, 900, 30).unwrap_err();
        assert!(err.to_string().contains("stale"));
    }

    #[test]
    fn accepts_recent_publish_time_with_small_clock_skew() {
        validate_publish_time(1_000, 1_003, 30).unwrap();
    }
}
