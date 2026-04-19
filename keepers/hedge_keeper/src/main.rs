use anchor_lang::AccountDeserialize;
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use clap::Parser;
use halcyon_client_sdk::{
    decode::{fetch_anchor_account, fetch_anchor_account_opt, list_policy_headers_for_product},
    pda,
    tx::send_instructions,
};
use halcyon_sol_autocall_quote::autocall_hedged::{
    price_hedged_autocall, AutocallTerms, CouponQuoteMode, PricingModel,
};
use pyth_solana_receiver_sdk::price_update::{PriceUpdateV2, VerificationLevel};
use serde::Deserialize;
use solana_rpc_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{
    account::Account,
    commitment_config::CommitmentConfig,
    instruction::AccountMeta,
    program_pack::Pack,
    pubkey::Pubkey,
    signature::{Keypair, Signature},
    signer::Signer,
};
use spl_token::state::Account as SplTokenAccount;
use std::collections::BTreeSet;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::sleep;
use tracing::{error, info, warn};

const JUPITER_SLIPPAGE_BPS: u16 = 50;
const JUPITER_PRICE_SANITY_BPS: u64 = 100;
const JUPITER_MAX_ACCOUNTS: u16 = 64;
const ASSET_TAG_SOL_SPOT: [u8; 8] = *b"SOL.SPOT";

#[derive(Debug, Deserialize)]
struct KeeperConfig {
    rpc_endpoint: String,
    keypair_path: String,
    kernel_program_id: String,
    sol_autocall_program_id: String,
    usdc_mint: String,
    pyth_sol: String,
    #[serde(default = "default_jupiter_base_url")]
    jupiter_base_url: String,
    #[serde(default)]
    jupiter_api_key: Option<String>,
    #[serde(default = "default_true")]
    dry_run: bool,
    #[serde(default = "default_allowed_jupiter_program_ids")]
    allowed_jupiter_program_ids: Vec<String>,
    #[serde(default)]
    allow_intraperiod_checks: bool,
    #[serde(default = "default_scan_interval_secs")]
    scan_interval_secs: u64,
    #[serde(default = "default_backoff_cap_secs")]
    backoff_cap_secs: u64,
    #[serde(default = "default_failure_budget")]
    failure_budget: u32,
}

fn default_true() -> bool {
    true
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

fn default_jupiter_base_url() -> String {
    "https://api.jup.ag/swap/v2".to_string()
}

fn default_allowed_jupiter_program_ids() -> Vec<String> {
    vec!["JUP6LkbZbjS1jKKwapdHNy74zcZ3tLUZoi5QNyVTaV4".to_string()]
}

impl KeeperConfig {
    fn load(path: &str) -> Result<Self> {
        let raw = std::fs::read_to_string(Path::new(path))
            .with_context(|| format!("reading hedge-keeper config at {path}"))?;
        serde_json::from_str(&raw).with_context(|| format!("parsing hedge-keeper config at {path}"))
    }

    fn load_keypair(&self) -> Result<Keypair> {
        solana_sdk::signer::keypair::read_keypair_file(&self.keypair_path)
            .map_err(|e| anyhow!("reading keypair at {}: {}", self.keypair_path, e))
    }

    fn jupiter_api_key(&self) -> Result<String> {
        self.jupiter_api_key
            .clone()
            .or_else(|| std::env::var("JUPITER_API_KEY").ok())
            .filter(|key| !key.trim().is_empty())
            .context(
                "missing Jupiter API key; set `jupiter_api_key` in hedge keeper config or `JUPITER_API_KEY` in the environment",
            )
    }

    fn allowed_jupiter_programs(&self) -> Result<BTreeSet<Pubkey>> {
        self.allowed_jupiter_program_ids
            .iter()
            .map(|program_id| {
                Pubkey::from_str(program_id)
                    .with_context(|| format!("parsing allowed Jupiter program id {program_id}"))
            })
            .collect()
    }
}

#[derive(Parser, Debug)]
#[command(name = "hedge_keeper", about = "Halcyon SOL Autocall hedge keeper")]
struct Args {
    #[arg(long, default_value = "config/hedge_keeper.json")]
    config: String,

    #[arg(long)]
    once: bool,
}

struct KeeperClient {
    rpc: Arc<RpcClient>,
    http: reqwest::Client,
    signer: Keypair,
    kernel_program: Pubkey,
    sol_autocall_program: Pubkey,
    usdc_mint: Pubkey,
    wsol_mint: Pubkey,
    pyth_sol: Pubkey,
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
            http: reqwest::Client::builder()
                .build()
                .context("building Jupiter HTTP client")?,
            signer: cfg.load_keypair()?,
            kernel_program: Pubkey::from_str(&cfg.kernel_program_id)
                .with_context(|| format!("parsing kernel_program_id {}", cfg.kernel_program_id))?,
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
            wsol_mint: spl_token::native_mint::ID,
            pyth_sol: Pubkey::from_str(&cfg.pyth_sol)
                .with_context(|| format!("parsing pyth_sol {}", cfg.pyth_sol))?,
        })
    }
}

#[derive(Debug, Clone)]
struct WalletCustody {
    usdc_ata: Pubkey,
    wsol_ata: Pubkey,
    usdc_balance_raw: u64,
    wsol_balance_raw: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupiterInstructionAccount {
    pubkey: String,
    is_signer: bool,
    is_writable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupiterInstructionPayload {
    program_id: String,
    accounts: Vec<JupiterInstructionAccount>,
    data: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct JupiterBuildResponse {
    input_mint: String,
    output_mint: String,
    in_amount: String,
    out_amount: String,
    swap_instruction: JupiterInstructionPayload,
}

#[derive(Debug, Clone, Copy)]
enum TradeDirection {
    BuyWsol,
    SellWsol,
}

#[derive(Debug)]
struct PlannedSwap {
    direction: TradeDirection,
    build: JupiterBuildResponse,
    quoted_price_s6: i64,
}

#[derive(Debug)]
struct ExecutedSwap {
    signature: Signature,
    post_custody: WalletCustody,
    quoted_price_s6: i64,
    effective_price_s6: i64,
    actual_position_delta_raw: i64,
    actual_usdc_delta_raw: i64,
    execution_cost_raw: u64,
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

fn pow10_i64(n: u32) -> Result<i64> {
    let mut out = 1i64;
    for _ in 0..n {
        out = out.checked_mul(10).context("pow10 overflow")?;
    }
    Ok(out)
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

fn interpolate(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    if xs.is_empty() || ys.is_empty() {
        return 0.0;
    }
    if x <= xs[0] {
        return ys[0];
    }
    if let Some(last) = xs.len().checked_sub(1) {
        if x >= xs[last] {
            return ys[last];
        }
    }
    for i in 1..xs.len() {
        if x <= xs[i] {
            let x0 = xs[i - 1];
            let x1 = xs[i];
            let y0 = ys[i - 1];
            let y1 = ys[i];
            let weight = if x1 == x0 { 0.0 } else { (x - x0) / (x1 - x0) };
            return y0 + weight * (y1 - y0);
        }
    }
    *ys.last().unwrap_or(&0.0)
}

fn compose_sigma_ann(
    sigma: &halcyon_kernel::state::VaultSigma,
    regime: &halcyon_kernel::state::RegimeSignal,
    sigma_floor_annualised_s6: i64,
) -> Result<f64> {
    let sigma_s6 = halcyon_sol_autocall::pricing::compose_pricing_sigma(
        sigma,
        regime,
        sigma_floor_annualised_s6,
    )?;
    Ok(sigma_s6 as f64 / 1_000_000.0)
}

fn note_target_quantity_sol(
    header: &halcyon_kernel::state::PolicyHeader,
    terms: &halcyon_sol_autocall::state::SolAutocallTerms,
    sigma_ann: f64,
    spot_price_s6: i64,
    now: i64,
) -> Result<f64> {
    if header.expiry_ts <= now {
        return Ok(0.0);
    }

    let observation_index = terms.current_observation_index as usize;
    if observation_index < halcyon_sol_autocall::state::OBSERVATION_COUNT
        && now >= terms.observation_schedule[observation_index]
        && observation_index >= terms.no_autocall_first_n_obs as usize
        && spot_price_s6 >= terms.autocall_barrier_s6
    {
        return Ok(0.0);
    }

    let notional = header.notional as f64;
    let entry_level = terms.entry_price_s6 as f64 / 1_000_000.0;
    let coupon_per_observation = halcyon_sol_autocall::pricing::coupon_per_observation_usdc(
        header.notional,
        terms.offered_coupon_bps_s6,
    )? as f64;
    let contract_terms = AutocallTerms {
        notional,
        entry_level,
        maturity_days: halcyon_sol_autocall::state::MATURITY_DAYS as usize,
        observation_interval_days: halcyon_sol_autocall::state::OBSERVATION_INTERVAL_DAYS as usize,
        autocall_barrier: terms.autocall_barrier_s6 as f64 / terms.entry_price_s6 as f64,
        coupon_barrier: terms.coupon_barrier_s6 as f64 / terms.entry_price_s6 as f64,
        knock_in_barrier: terms.ki_barrier_s6 as f64 / terms.entry_price_s6 as f64,
        coupon_quote_mode: CouponQuoteMode::FixedPerObservation(coupon_per_observation),
        issuer_margin_bps: terms.issuer_margin_bps as f64,
        quote_share_of_fair_coupon: terms.quote_share_bps as f64 / 10_000.0,
        note_id: header.policy_id.to_string(),
        engine_version: header.engine_version.to_string(),
        no_autocall_first_n_obs: terms.no_autocall_first_n_obs as usize,
    };
    let model = PricingModel {
        sigma_ann,
        ..Default::default()
    };
    let priced = price_hedged_autocall(&contract_terms, &model)?;
    let elapsed_days = ((now.saturating_sub(header.issued_at)).max(0)
        / halcyon_sol_autocall::state::SECONDS_PER_DAY) as usize;
    let day = elapsed_days.min(priced.surfaces.len().saturating_sub(1));
    let spot_ratio = (spot_price_s6 as f64 / 1_000_000.0) / entry_level;
    let surface = &priced.surfaces[day];
    let raw_delta = if terms.ki_triggered {
        interpolate(&surface.spot_ratios, &surface.touched_deltas, spot_ratio)
    } else {
        interpolate(&surface.spot_ratios, &surface.untouched_deltas, spot_ratio)
    };
    let capped_target_delta = (raw_delta.max(0.0) * 0.5).min(0.75);
    Ok(capped_target_delta * notional)
}

fn qty_to_raw(quantity_sol: f64) -> Result<i64> {
    let raw = (quantity_sol * 1_000_000_000.0).round();
    anyhow::ensure!(raw.is_finite(), "invalid hedge quantity");
    Ok(raw as i64)
}

fn raw_to_qty(raw: i64) -> f64 {
    raw as f64 / 1_000_000_000.0
}

fn parse_u64_field(label: &str, value: &str) -> Result<u64> {
    value
        .parse::<u64>()
        .with_context(|| format!("parsing Jupiter field `{label}` from `{value}`"))
}

fn i64_from_u64(value: u64, label: &str) -> Result<i64> {
    i64::try_from(value).with_context(|| format!("converting `{label}` to i64"))
}

fn abs_i64_to_u64(value: i64, label: &str) -> Result<u64> {
    let as_i128 = i128::from(value).abs();
    u64::try_from(as_i128).with_context(|| format!("converting abs({label}) to u64"))
}

fn estimate_usdc_input_raw(sol_raw: u64, spot_price_s6: i64) -> Result<u64> {
    anyhow::ensure!(spot_price_s6 > 0, "spot price must be positive");
    let numerator = u128::from(sol_raw)
        .checked_mul(u128::try_from(spot_price_s6).context("spot price is negative")?)
        .context("USDC estimate overflow")?
        .checked_add(1_000_000_000u128 - 1)
        .context("USDC estimate overflow")?;
    let raw = numerator / 1_000_000_000u128;
    u64::try_from(raw).context("USDC estimate overflow")
}

fn reference_notional_usdc_raw(sol_raw: u64, spot_price_s6: i64) -> Result<u64> {
    anyhow::ensure!(spot_price_s6 > 0, "spot price must be positive");
    let raw = u128::from(sol_raw)
        .checked_mul(u128::try_from(spot_price_s6).context("spot price is negative")?)
        .context("reference notional overflow")?
        / 1_000_000_000u128;
    u64::try_from(raw).context("reference notional overflow")
}

fn effective_price_s6(usdc_raw: u64, sol_raw: u64) -> Result<i64> {
    anyhow::ensure!(usdc_raw > 0, "USDC flow must be positive");
    anyhow::ensure!(sol_raw > 0, "SOL flow must be positive");
    let raw = u128::from(usdc_raw)
        .checked_mul(1_000_000_000u128)
        .context("effective price overflow")?
        / u128::from(sol_raw);
    i64::try_from(raw).context("effective price overflow")
}

fn price_deviation_bps(reference_price_s6: i64, observed_price_s6: i64) -> Result<u64> {
    anyhow::ensure!(reference_price_s6 > 0, "reference price must be positive");
    anyhow::ensure!(observed_price_s6 > 0, "observed price must be positive");
    let diff = (i128::from(reference_price_s6) - i128::from(observed_price_s6)).abs() as u128;
    let bps = diff
        .checked_mul(10_000u128)
        .context("price deviation overflow")?
        / u128::try_from(reference_price_s6).context("reference price is negative")?;
    u64::try_from(bps).context("price deviation overflow")
}

async fn read_pyth_price_s6(
    rpc: &RpcClient,
    address: &Pubkey,
    staleness_cap_secs: i64,
) -> Result<i64> {
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
        update.price_message.feed_id == halcyon_oracles::feed_ids::SOL_USD,
        "unexpected feed id"
    );
    anyhow::ensure!(
        matches!(update.verification_level, VerificationLevel::Full),
        "Pyth verification level is not Full"
    );
    let now = unix_now();
    anyhow::ensure!(
        now.saturating_sub(update.price_message.publish_time) <= staleness_cap_secs,
        "Pyth SOL price is stale"
    );
    rescale_to_s6(update.price_message.price, update.price_message.exponent)
}

fn parse_spl_token_balance(account: Option<&Account>, address: &Pubkey) -> Result<u64> {
    let Some(account) = account else {
        return Ok(0);
    };
    anyhow::ensure!(
        account.owner == spl_token::ID,
        "token account {address} has unexpected owner {}",
        account.owner
    );
    let token_account = SplTokenAccount::unpack(&account.data)
        .with_context(|| format!("decoding token account {address}"))?;
    Ok(token_account.amount)
}

async fn read_wallet_custody(
    rpc: &RpcClient,
    owner: &Pubkey,
    usdc_mint: &Pubkey,
    wsol_mint: &Pubkey,
) -> Result<WalletCustody> {
    let usdc_ata = pda::associated_token_account(owner, usdc_mint);
    let wsol_ata = pda::associated_token_account(owner, wsol_mint);
    let accounts = rpc
        .get_multiple_accounts(&[usdc_ata, wsol_ata])
        .await
        .context("fetching hedge sleeve custody accounts")?;
    anyhow::ensure!(
        accounts.len() == 2,
        "expected exactly two custody accounts, got {}",
        accounts.len()
    );
    let usdc_account = accounts[0].as_ref();
    let wsol_account = accounts[1].as_ref();
    Ok(WalletCustody {
        usdc_ata,
        wsol_ata,
        usdc_balance_raw: parse_spl_token_balance(usdc_account, &usdc_ata)?,
        wsol_balance_raw: parse_spl_token_balance(wsol_account, &wsol_ata)?,
    })
}

fn deserialize_jupiter_swap_payload(
    payload: &JupiterInstructionPayload,
    allowed_programs: &BTreeSet<Pubkey>,
    sleeve_authority: &Pubkey,
    tx_signer: &Pubkey,
) -> Result<(Pubkey, Vec<AccountMeta>, Vec<u8>)> {
    let program_id = Pubkey::from_str(&payload.program_id)
        .with_context(|| format!("parsing Jupiter program id {}", payload.program_id))?;
    anyhow::ensure!(
        allowed_programs.contains(&program_id),
        "Jupiter instruction uses non-allowlisted program {}",
        program_id
    );
    let accounts = payload
        .accounts
        .iter()
        .map(|account| {
            let pubkey = Pubkey::from_str(&account.pubkey)
                .with_context(|| format!("parsing Jupiter account {}", account.pubkey))?;
            anyhow::ensure!(
                !account.is_signer || pubkey == *sleeve_authority || pubkey == *tx_signer,
                "Jupiter instruction requested unexpected signer {} for program {}",
                pubkey,
                program_id
            );
            Ok(AccountMeta {
                pubkey,
                is_signer: account.is_signer,
                is_writable: account.is_writable,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let data = base64::engine::general_purpose::STANDARD
        .decode(&payload.data)
        .context("base64-decoding Jupiter instruction data")?;
    Ok((program_id, accounts, data))
}

async fn jupiter_build_swap(
    client: &KeeperClient,
    cfg: &KeeperConfig,
    input_mint: &Pubkey,
    output_mint: &Pubkey,
    amount: u64,
    destination_token_account: &Pubkey,
    taker: &Pubkey,
) -> Result<JupiterBuildResponse> {
    let api_key = cfg.jupiter_api_key()?;
    let endpoint = format!("{}/build", cfg.jupiter_base_url.trim_end_matches('/'));
    let query = vec![
        ("inputMint", input_mint.to_string()),
        ("outputMint", output_mint.to_string()),
        ("amount", amount.to_string()),
        ("taker", taker.to_string()),
        ("payer", client.signer.pubkey().to_string()),
        ("slippageBps", JUPITER_SLIPPAGE_BPS.to_string()),
        ("wrapAndUnwrapSol", "false".to_string()),
        (
            "destinationTokenAccount",
            destination_token_account.to_string(),
        ),
        ("maxAccounts", JUPITER_MAX_ACCOUNTS.to_string()),
    ];

    let response = client
        .http
        .get(&endpoint)
        .header("x-api-key", api_key)
        .query(&query)
        .send()
        .await
        .with_context(|| format!("calling Jupiter build endpoint {endpoint}"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("reading Jupiter response body")?;
    anyhow::ensure!(
        status.is_success(),
        "Jupiter build request failed ({status}): {body}"
    );
    serde_json::from_str(&body).context("parsing Jupiter build response")
}

fn quote_price_s6(build: &JupiterBuildResponse, direction: TradeDirection) -> Result<i64> {
    let in_amount = parse_u64_field("inAmount", &build.in_amount)?;
    let out_amount = parse_u64_field("outAmount", &build.out_amount)?;
    match direction {
        TradeDirection::BuyWsol => effective_price_s6(in_amount, out_amount),
        TradeDirection::SellWsol => effective_price_s6(out_amount, in_amount),
    }
}

async fn plan_jupiter_swap(
    client: &KeeperClient,
    cfg: &KeeperConfig,
    pre_custody: &WalletCustody,
    sleeve_authority: &Pubkey,
    desired_trade_raw: i64,
    spot_price_s6: i64,
) -> Result<PlannedSwap> {
    anyhow::ensure!(desired_trade_raw != 0, "desired hedge trade is zero");

    let direction = if desired_trade_raw > 0 {
        TradeDirection::BuyWsol
    } else {
        TradeDirection::SellWsol
    };

    let build = match direction {
        TradeDirection::BuyWsol => {
            let desired_out_raw =
                u64::try_from(desired_trade_raw).context("buy target overflow")?;
            let mut requested_input_raw =
                estimate_usdc_input_raw(desired_out_raw, spot_price_s6)?.max(1);
            if pre_custody.usdc_balance_raw < requested_input_raw {
                warn!(
                    target = "hedge_keeper",
                    requested_input_raw,
                    available_usdc_raw = pre_custody.usdc_balance_raw,
                    "hedge sleeve USDC balance is below the spot-estimated buy amount; proceeding with available balance",
                );
                requested_input_raw = pre_custody.usdc_balance_raw;
            }
            anyhow::ensure!(
                requested_input_raw > 0,
                "hedge sleeve has no USDC available for a required buy hedge"
            );

            let mut build = jupiter_build_swap(
                client,
                cfg,
                &client.usdc_mint,
                &client.wsol_mint,
                requested_input_raw,
                &pre_custody.wsol_ata,
                sleeve_authority,
            )
            .await?;
            anyhow::ensure!(
                build.input_mint == client.usdc_mint.to_string()
                    && build.output_mint == client.wsol_mint.to_string(),
                "unexpected Jupiter route pair {} -> {} for buy hedge",
                build.input_mint,
                build.output_mint
            );
            let quoted_out_raw = parse_u64_field("outAmount", &build.out_amount)?;
            anyhow::ensure!(quoted_out_raw > 0, "Jupiter quoted zero WSOL output");

            let adjusted_input_raw = ((u128::from(requested_input_raw)
                * u128::from(desired_out_raw))
            .checked_add(u128::from(quoted_out_raw) - 1)
            .context("input adjustment overflow")?
                / u128::from(quoted_out_raw))
            .min(u128::from(pre_custody.usdc_balance_raw));

            if adjusted_input_raw > 0 && adjusted_input_raw != u128::from(requested_input_raw) {
                build = jupiter_build_swap(
                    client,
                    cfg,
                    &client.usdc_mint,
                    &client.wsol_mint,
                    u64::try_from(adjusted_input_raw).context("adjusted buy amount overflow")?,
                    &pre_custody.wsol_ata,
                    sleeve_authority,
                )
                .await?;
                anyhow::ensure!(
                    build.input_mint == client.usdc_mint.to_string()
                        && build.output_mint == client.wsol_mint.to_string(),
                    "unexpected Jupiter route pair {} -> {} for adjusted buy hedge",
                    build.input_mint,
                    build.output_mint
                );
            }
            build
        }
        TradeDirection::SellWsol => {
            let requested_input_raw = abs_i64_to_u64(desired_trade_raw, "desired_trade_raw")?
                .min(pre_custody.wsol_balance_raw);
            anyhow::ensure!(
                requested_input_raw > 0,
                "hedge sleeve has no WSOL inventory available for a required sell hedge"
            );
            let build = jupiter_build_swap(
                client,
                cfg,
                &client.wsol_mint,
                &client.usdc_mint,
                requested_input_raw,
                &pre_custody.usdc_ata,
                sleeve_authority,
            )
            .await?;
            anyhow::ensure!(
                build.input_mint == client.wsol_mint.to_string()
                    && build.output_mint == client.usdc_mint.to_string(),
                "unexpected Jupiter route pair {} -> {} for sell hedge",
                build.input_mint,
                build.output_mint
            );
            build
        }
    };

    let quoted_price_s6 = quote_price_s6(&build, direction)?;
    let quote_deviation_bps = price_deviation_bps(spot_price_s6, quoted_price_s6)?;
    anyhow::ensure!(
        quote_deviation_bps <= JUPITER_PRICE_SANITY_BPS,
        "Jupiter quote price deviates {} bps from Pyth spot (quote={} spot={})",
        quote_deviation_bps,
        quoted_price_s6,
        spot_price_s6
    );

    Ok(PlannedSwap {
        direction,
        build,
        quoted_price_s6,
    })
}

async fn execute_target_swap(
    client: &KeeperClient,
    cfg: &KeeperConfig,
    pre_custody: WalletCustody,
    old_position_raw: i64,
    target_position_raw: i64,
    desired_trade_raw: i64,
    spot_price_s6: i64,
    sequence: u64,
) -> Result<ExecutedSwap> {
    let hedge_sleeve = pda::hedge_sleeve(&client.sol_autocall_program).0;
    let planned = plan_jupiter_swap(
        client,
        cfg,
        &pre_custody,
        &hedge_sleeve,
        desired_trade_raw,
        spot_price_s6,
    )
    .await?;

    let allowed_programs = cfg.allowed_jupiter_programs()?;
    let (jupiter_program, jupiter_accounts, jupiter_instruction_data) =
        deserialize_jupiter_swap_payload(
            &planned.build.swap_instruction,
            &allowed_programs,
            &hedge_sleeve,
            &client.signer.pubkey(),
        )?;
    let ix = halcyon_client_sdk::kernel::execute_hedge_swap_ix(
        &client.signer.pubkey(),
        &client.signer.pubkey(),
        &client.usdc_mint,
        &client.pyth_sol,
        &jupiter_program,
        halcyon_kernel::ExecuteHedgeSwapArgs {
            product_program_id: client.sol_autocall_program,
            asset_tag: ASSET_TAG_SOL_SPOT,
            leg_index: 0,
            old_position_raw,
            target_position_raw,
            max_slippage_bps: JUPITER_PRICE_SANITY_BPS as u16,
            jupiter_instruction_data,
            sequence,
        },
        jupiter_accounts,
    );
    let signature = send_instructions(&client.rpc, &client.signer, vec![ix]).await?;
    let post_custody = read_wallet_custody(
        &client.rpc,
        &hedge_sleeve,
        &client.usdc_mint,
        &client.wsol_mint,
    )
    .await?;

    let pre_position_raw = i64_from_u64(pre_custody.wsol_balance_raw, "pre WSOL balance")?;
    let post_position_raw = i64_from_u64(post_custody.wsol_balance_raw, "post WSOL balance")?;
    let pre_usdc_raw = i64_from_u64(pre_custody.usdc_balance_raw, "pre USDC balance")?;
    let post_usdc_raw = i64_from_u64(post_custody.usdc_balance_raw, "post USDC balance")?;

    let actual_position_delta_raw = post_position_raw
        .checked_sub(pre_position_raw)
        .context("actual position delta overflow")?;
    let actual_usdc_delta_raw = post_usdc_raw
        .checked_sub(pre_usdc_raw)
        .context("actual USDC delta overflow")?;

    match planned.direction {
        TradeDirection::BuyWsol => {
            anyhow::ensure!(
                actual_position_delta_raw > 0 && actual_usdc_delta_raw < 0,
                "kernel-executed buy hedge produced unexpected balance deltas: position_delta_raw={} usdc_delta_raw={}",
                actual_position_delta_raw,
                actual_usdc_delta_raw
            );
        }
        TradeDirection::SellWsol => {
            anyhow::ensure!(
                actual_position_delta_raw < 0 && actual_usdc_delta_raw > 0,
                "kernel-executed sell hedge produced unexpected balance deltas: position_delta_raw={} usdc_delta_raw={}",
                actual_position_delta_raw,
                actual_usdc_delta_raw
            );
        }
    }

    let actual_position_delta_abs_raw =
        abs_i64_to_u64(actual_position_delta_raw, "actual_position_delta_raw")?;
    let actual_usdc_flow_abs_raw = abs_i64_to_u64(actual_usdc_delta_raw, "actual_usdc_delta_raw")?;
    let effective_price_s6 =
        effective_price_s6(actual_usdc_flow_abs_raw, actual_position_delta_abs_raw)?;
    let reference_notional_raw =
        reference_notional_usdc_raw(actual_position_delta_abs_raw, spot_price_s6)?;
    let execution_cost_raw = actual_usdc_flow_abs_raw.saturating_sub(reference_notional_raw);

    Ok(ExecutedSwap {
        signature,
        post_custody,
        quoted_price_s6: planned.quoted_price_s6,
        effective_price_s6,
        actual_position_delta_raw,
        actual_usdc_delta_raw,
        execution_cost_raw,
    })
}

async fn run_once(client: &KeeperClient, cfg: &KeeperConfig) -> Result<()> {
    let protocol = fetch_anchor_account::<halcyon_kernel::state::ProtocolConfig>(
        &client.rpc,
        &pda::protocol_config().0,
    )
    .await?;
    let sigma = fetch_anchor_account::<halcyon_kernel::state::VaultSigma>(
        &client.rpc,
        &pda::vault_sigma(&client.sol_autocall_program).0,
    )
    .await?;
    let regime = fetch_anchor_account::<halcyon_kernel::state::RegimeSignal>(
        &client.rpc,
        &pda::regime_signal(&client.sol_autocall_program).0,
    )
    .await?;
    let hedge_book = fetch_anchor_account_opt::<halcyon_kernel::state::HedgeBookState>(
        &client.rpc,
        &pda::hedge_book(&client.sol_autocall_program).0,
    )
    .await?;
    let last_rebalance_ts = hedge_book
        .as_ref()
        .map(|book| book.last_rebalance_ts)
        .unwrap_or(0);
    let now = unix_now();
    anyhow::ensure!(
        now.saturating_sub(sigma.ewma_last_timestamp) <= protocol.sigma_staleness_cap_secs,
        "vault sigma is stale"
    );
    anyhow::ensure!(
        now.saturating_sub(regime.last_update_ts) <= protocol.regime_staleness_cap_secs,
        "regime signal is stale"
    );
    let sigma_ann = compose_sigma_ann(&sigma, &regime, protocol.sigma_floor_annualised_s6)?;
    let spot_price_s6 = read_pyth_price_s6(
        &client.rpc,
        &client.pyth_sol,
        protocol.pyth_quote_staleness_cap_secs,
    )
    .await?;
    let headers =
        list_policy_headers_for_product(&client.rpc, &client.sol_autocall_program).await?;
    let new_issuance_since_last_rebalance = headers.iter().any(|(_, header)| {
        header.status == halcyon_kernel::state::PolicyStatus::Active
            && header.issued_at > last_rebalance_ts
    });
    let hedge_sleeve = pda::hedge_sleeve(&client.sol_autocall_program).0;
    let pre_custody = read_wallet_custody(
        &client.rpc,
        &hedge_sleeve,
        &client.usdc_mint,
        &client.wsol_mint,
    )
    .await?;

    let mut total_notional = 0.0f64;
    let mut aggregate_target_qty_sol = 0.0f64;
    let mut active_policies = 0usize;
    let mut due_review_policies = 0usize;
    for (_, header) in headers {
        if header.status != halcyon_kernel::state::PolicyStatus::Active
            || header.product_terms == Pubkey::default()
        {
            continue;
        }
        let terms = fetch_anchor_account::<halcyon_sol_autocall::state::SolAutocallTerms>(
            &client.rpc,
            &header.product_terms,
        )
        .await?;
        if terms.status != halcyon_sol_autocall::state::ProductStatus::Active {
            continue;
        }
        if (terms.current_observation_index as usize)
            < halcyon_sol_autocall::state::OBSERVATION_COUNT
            && now >= terms.observation_schedule[terms.current_observation_index as usize]
        {
            due_review_policies += 1;
        }
        total_notional += header.notional as f64;
        aggregate_target_qty_sol +=
            note_target_quantity_sol(&header, &terms, sigma_ann, spot_price_s6, now)?;
        active_policies += 1;
    }

    let old_position_raw = hedge_book
        .as_ref()
        .map(|book| book.legs[0].current_position_raw)
        .unwrap_or(0);
    let current_position_raw = i64_from_u64(
        pre_custody.wsol_balance_raw,
        "current hedge sleeve WSOL balance",
    )?;
    let current_qty_sol = raw_to_qty(current_position_raw);
    let target_position_raw = if active_policies > 0 && total_notional > 0.0 {
        qty_to_raw(aggregate_target_qty_sol)?
    } else {
        0
    };
    let trade_target_raw = target_position_raw
        .checked_sub(current_position_raw)
        .context("target trade overflow")?;
    let trade_qty_sol = raw_to_qty(trade_target_raw);
    let sleeve_vs_book_delta_raw = current_position_raw
        .checked_sub(old_position_raw)
        .context("sleeve/book delta overflow")?;

    anyhow::ensure!(
        sleeve_vs_book_delta_raw == 0,
        "hedge sleeve WSOL balance diverged from hedge book: sleeve={} hedge_book={}",
        current_position_raw,
        old_position_raw
    );

    if active_policies == 0 && current_position_raw == 0 {
        info!(
            target = "hedge_keeper",
            "no active SOL Autocall policies and no hedge inventory"
        );
        return Ok(());
    }

    let (
        target_delta,
        current_delta,
        trade_delta,
        band_breached,
        min_trade_passed,
        rebalance_window_open,
    ) = if active_policies > 0 && total_notional > 0.0 {
        let target_delta = aggregate_target_qty_sol / total_notional;
        let current_delta = current_qty_sol / total_notional;
        let trade_delta = target_delta - current_delta;
        let band_breached = trade_delta.abs() > 0.10;
        let min_trade_passed = (trade_qty_sol.abs() / total_notional) >= 0.01;
        let rebalance_window_open = cfg.allow_intraperiod_checks
            || due_review_policies > 0
            || new_issuance_since_last_rebalance;
        (
            target_delta,
            current_delta,
            trade_delta,
            band_breached,
            min_trade_passed,
            rebalance_window_open,
        )
    } else {
        (
            0.0,
            0.0,
            0.0,
            current_position_raw != 0,
            current_position_raw != 0,
            current_position_raw != 0,
        )
    };

    info!(
        target = "hedge_keeper",
        active_policies,
        due_review_policies,
        sigma_ann,
        spot_price_s6,
        total_notional,
        current_qty_sol,
        current_position_raw,
        hedge_book_position_raw = old_position_raw,
        sleeve_vs_book_delta_raw,
        hedge_sleeve_usdc_balance_raw = pre_custody.usdc_balance_raw,
        hedge_sleeve_wsol_balance_raw = pre_custody.wsol_balance_raw,
        aggregate_target_qty_sol,
        target_position_raw,
        target_delta,
        current_delta,
        trade_delta,
        trade_qty_sol,
        trade_target_raw,
        rebalance_window_open,
        allow_intraperiod_checks = cfg.allow_intraperiod_checks,
        new_issuance_since_last_rebalance,
        band_breached,
        min_trade_passed,
        dry_run = cfg.dry_run,
        "evaluated hedge pass",
    );

    if !rebalance_window_open {
        return Ok(());
    }

    if !band_breached || !min_trade_passed || trade_target_raw == 0 {
        return Ok(());
    }

    if cfg.dry_run {
        warn!(
            target = "hedge_keeper",
            target_position_raw,
            current_position_raw,
            trade_target_raw,
            "dry-run: kernel execute_hedge_swap not invoked",
        );
        return Ok(());
    }

    let sequence = hedge_book
        .as_ref()
        .map(|book| book.sequence + 1)
        .unwrap_or(1);
    let executed = execute_target_swap(
        client,
        cfg,
        pre_custody,
        old_position_raw,
        target_position_raw,
        trade_target_raw,
        spot_price_s6,
        sequence,
    )
    .await?;
    let new_position_raw = i64_from_u64(
        executed.post_custody.wsol_balance_raw,
        "post-trade hedge sleeve WSOL balance",
    )?;
    let trade_delta_raw = new_position_raw
        .checked_sub(old_position_raw)
        .context("recorded trade delta overflow")?;
    info!(
        target = "hedge_keeper",
        execute_signature = %executed.signature,
        quoted_price_s6 = executed.quoted_price_s6,
        effective_price_s6 = executed.effective_price_s6,
        execution_cost_raw = executed.execution_cost_raw,
        actual_position_delta_raw = executed.actual_position_delta_raw,
        actual_usdc_delta_raw = executed.actual_usdc_delta_raw,
        new_position_raw,
        trade_delta_raw,
        sequence,
        "executed kernel-backed hedge swap",
    );
    Ok(())
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
                    target = "hedge_keeper",
                    %err,
                    consecutive_failures,
                    "hedge pass failed",
                );
                if consecutive_failures >= cfg.failure_budget {
                    return Err(err);
                }
                sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(cfg.backoff_cap_secs);
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();
    let cfg = KeeperConfig::load(&args.config)?;
    let client = KeeperClient::connect(&cfg).await?;
    info!(
        target = "hedge_keeper",
        endpoint = %cfg.rpc_endpoint,
        dry_run = cfg.dry_run,
        kernel_program = %client.kernel_program,
        sol_autocall_program = %client.sol_autocall_program,
        usdc_mint = %client.usdc_mint,
        wsol_mint = %client.wsol_mint,
        jupiter_base_url = %cfg.jupiter_base_url,
        "hedge keeper starting",
    );

    if args.once {
        run_once(&client, &cfg).await?;
        return Ok(());
    }

    let shutdown = tokio::signal::ctrl_c();
    tokio::select! {
        result = run_forever(&client, &cfg) => result,
        _ = shutdown => {
            info!(target = "hedge_keeper", "SIGINT received; shutting down");
            Ok(())
        }
    }
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
