use anyhow::{anyhow, Result};
use clap::{Args as ClapArgs, Subcommand};
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{decode::fetch_anchor_account, kernel, sol_autocall, tx};
use halcyon_sol_autocall_quote::{
    autocall_v2::AutocallParams,
    autocall_v2_e11::precompute_reduced_operators_from_const,
    generated::pod_deim_table::{
        TRAINING_ALPHA_S6, TRAINING_AUTOCALL_LOG_6, TRAINING_BETA_S6,
        TRAINING_KNOCK_IN_LOG_6, TRAINING_NO_AUTOCALL_FIRST_N_OBS, TRAINING_N_OBS,
        TRAINING_REFERENCE_STEP_DAYS,
    },
};
use solana_sdk::pubkey::Pubkey;

use crate::client::CliContext;

const REDUCED_OPERATOR_CHUNK_LEN: usize = 48;

#[derive(Debug, Subcommand)]
pub enum KeeperCmd {
    FireObservation(FireObservationArgs),
    FireHedge(FireHedgeArgs),
    FireRegime(FireRegimeArgs),
    FireReducedOps(FireReducedOpsArgs),
}

#[derive(Debug, ClapArgs)]
pub struct FireObservationArgs {
    pub policy: String,
    #[arg(long)]
    pub usdc_mint: String,
    #[arg(long)]
    pub pyth_sol: String,
}

#[derive(Debug, ClapArgs)]
pub struct FireHedgeArgs {
    #[arg(long, default_value = "SOL")]
    pub asset_tag: String,
    #[arg(long, default_value_t = 0)]
    pub leg_index: u8,
    #[arg(long)]
    pub new_position_raw: i64,
    #[arg(long)]
    pub executed_price_s6: i64,
    #[arg(long, default_value_t = 0)]
    pub execution_cost: u64,
    #[arg(long)]
    pub trade_delta_raw: Option<i64>,
    #[arg(long)]
    pub sequence: Option<u64>,
}

#[derive(Debug, ClapArgs)]
pub struct FireRegimeArgs {
    #[arg(
        long,
        default_value = "https://api.coingecko.com/api/v3/coins/solana/market_chart?vs_currency=usd&days=120&interval=daily"
    )]
    pub history_url: String,
    #[arg(long)]
    pub fvol_s6: Option<i64>,
    /// Product that owns the regime_signal PDA to seed. One of `il`, `sol`, `flagship`.
    #[arg(long, default_value = "il")]
    pub product: String,
}

#[derive(Debug, ClapArgs)]
pub struct FireReducedOpsArgs {}

pub async fn run(ctx: &CliContext, cmd: KeeperCmd) -> Result<()> {
    match cmd {
        KeeperCmd::FireObservation(a) => fire_observation(ctx, a).await,
        KeeperCmd::FireHedge(a) => fire_hedge(ctx, a).await,
        KeeperCmd::FireRegime(a) => fire_regime(ctx, a).await,
        KeeperCmd::FireReducedOps(a) => fire_reduced_ops(ctx, a).await,
    }
}

async fn fire_observation(ctx: &CliContext, args: FireObservationArgs) -> Result<()> {
    let keeper = ctx.signer()?;
    let policy = CliContext::parse_pubkey("policy", &args.policy)?;
    let usdc_mint = CliContext::parse_pubkey("usdc_mint", &args.usdc_mint)?;
    let pyth_sol = CliContext::parse_pubkey("pyth_sol", &args.pyth_sol)?;
    let header =
        fetch_anchor_account::<halcyon_kernel::state::PolicyHeader>(ctx.rpc.as_ref(), &policy)
            .await?;
    let terms = fetch_anchor_account::<halcyon_sol_autocall::state::SolAutocallTerms>(
        ctx.rpc.as_ref(),
        &header.product_terms,
    )
    .await?;
    let ix = sol_autocall::record_observation_ix(
        &keeper.pubkey(),
        &usdc_mint,
        pyth_sol,
        &header,
        policy,
        terms.current_observation_index,
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), keeper, vec![ix]).await?;
    println!(
        "keepers fire-observation: signature={signature} policy={policy} expected_index={}",
        terms.current_observation_index
    );
    Ok(())
}

async fn fire_hedge(ctx: &CliContext, args: FireHedgeArgs) -> Result<()> {
    let _ = ctx;
    let _ = args;
    anyhow::bail!(
        "keepers fire-hedge is disabled; manual hedge recording was retired. Use the hedge keeper prepare_hedge_swap -> Jupiter swap -> record_hedge_trade flow instead."
    )
}

fn resolve_regime_product(alias: &str) -> Result<Pubkey> {
    match alias.to_ascii_lowercase().as_str() {
        "il" | "il_protection" | "il-protection" => Ok(halcyon_il_protection::ID),
        "sol" | "sol_autocall" | "sol-autocall" => Ok(halcyon_sol_autocall::ID),
        "flagship" | "flagship_autocall" | "flagship-autocall" => {
            Ok(halcyon_flagship_autocall::ID)
        }
        other => anyhow::bail!(
            "unknown --product '{other}' (expected one of: il, sol, flagship)"
        ),
    }
}

async fn fire_regime(ctx: &CliContext, args: FireRegimeArgs) -> Result<()> {
    let keeper = ctx.signer()?;
    let product_program_id = resolve_regime_product(&args.product)?;
    let fvol_s6 = match args.fvol_s6 {
        Some(value) => value,
        None => fetch_fvol_s6(&args.history_url).await?,
    };
    let regime = halcyon_il_quote::classify_regime_from_fvol_s6(fvol_s6);
    let ix = kernel::write_regime_signal_ix(
        &keeper.pubkey(),
        &keeper.pubkey(),
        &product_program_id,
        halcyon_kernel::WriteRegimeSignalArgs {
            product_program_id,
            fvol_s6,
        },
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), keeper, vec![ix]).await?;
    println!(
        "keepers fire-regime: signature={signature} product={product_program_id} fvol_s6={fvol_s6} regime={:?} sigma_multiplier_s6={}",
        regime.regime, regime.sigma_multiplier_s6
    );
    Ok(())
}

async fn fire_reduced_ops(ctx: &CliContext, _args: FireReducedOpsArgs) -> Result<()> {
    let keeper = ctx.signer()?;
    let (protocol_config, _) = halcyon_client_sdk::pda::protocol_config();
    let (vault_sigma, _) = halcyon_client_sdk::pda::vault_sigma(&halcyon_sol_autocall::ID);
    let (regime_signal, _) = halcyon_client_sdk::pda::regime_signal(&halcyon_sol_autocall::ID);

    let protocol = fetch_anchor_account::<halcyon_kernel::state::ProtocolConfig>(
        ctx.rpc.as_ref(),
        &protocol_config,
    )
    .await?;
    let sigma = fetch_anchor_account::<halcyon_kernel::state::VaultSigma>(ctx.rpc.as_ref(), &vault_sigma)
        .await?;
    let regime =
        fetch_anchor_account::<halcyon_kernel::state::RegimeSignal>(ctx.rpc.as_ref(), &regime_signal)
            .await?;

    let sigma_ann_s6 = halcyon_sol_autocall::pricing::compose_pricing_sigma(
        &sigma,
        &regime,
        protocol.sigma_floor_annualised_s6,
    )?;

    let contract = AutocallParams {
        n_obs: TRAINING_N_OBS,
        knock_in_log_6: TRAINING_KNOCK_IN_LOG_6,
        autocall_log_6: TRAINING_AUTOCALL_LOG_6,
        no_autocall_first_n_obs: TRAINING_NO_AUTOCALL_FIRST_N_OBS,
    };
    let reduced = precompute_reduced_operators_from_const(
        sigma_ann_s6,
        TRAINING_ALPHA_S6,
        TRAINING_BETA_S6,
        TRAINING_REFERENCE_STEP_DAYS,
        &contract,
    )
    .map_err(|err| anyhow!("failed to precompute reduced operators: {err:?}"))?;

    let p_red_v = reduced.p_red_v;
    let p_red_u = reduced.p_red_u;
    for (side, values) in [
        (halcyon_sol_autocall::ReducedOperatorSide::V, p_red_v.as_slice()),
        (halcyon_sol_autocall::ReducedOperatorSide::U, p_red_u.as_slice()),
    ] {
        for start in (0..values.len()).step_by(REDUCED_OPERATOR_CHUNK_LEN) {
            let end = (start + REDUCED_OPERATOR_CHUNK_LEN).min(values.len());
            let ix = sol_autocall::write_reduced_operators_ix(
                &keeper.pubkey(),
                halcyon_sol_autocall::WriteReducedOperatorsArgs {
                    begin_upload: matches!(side, halcyon_sol_autocall::ReducedOperatorSide::V)
                        && start == 0,
                    side,
                    start: start as u16,
                    values: values[start..end].to_vec(),
                },
            );
            let signature = tx::send_instructions(ctx.rpc.as_ref(), keeper, vec![ix]).await?;
            println!(
                "keepers fire-reduced-ops: signature={signature} side={side:?} range={start}..{end} sigma_ann_s6={sigma_ann_s6} vault_sigma_slot={} regime_signal_slot={}",
                sigma.last_update_slot, regime.last_update_slot,
            );
        }
    }
    Ok(())
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
