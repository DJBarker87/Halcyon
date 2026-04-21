use anyhow::Result;
use clap::{Args as ClapArgs, Subcommand};
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{decode::fetch_anchor_account, kernel, sol_autocall, tx};

use crate::client::CliContext;

#[derive(Debug, Subcommand)]
pub enum KeeperCmd {
    FireObservation(FireObservationArgs),
    FireHedge(FireHedgeArgs),
    FireRegime(FireRegimeArgs),
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
}

pub async fn run(ctx: &CliContext, cmd: KeeperCmd) -> Result<()> {
    match cmd {
        KeeperCmd::FireObservation(a) => fire_observation(ctx, a).await,
        KeeperCmd::FireHedge(a) => fire_hedge(ctx, a).await,
        KeeperCmd::FireRegime(a) => fire_regime(ctx, a).await,
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

async fn fire_regime(ctx: &CliContext, args: FireRegimeArgs) -> Result<()> {
    let keeper = ctx.signer()?;
    let fvol_s6 = match args.fvol_s6 {
        Some(value) => value,
        None => fetch_fvol_s6(&args.history_url).await?,
    };
    let regime = halcyon_il_quote::classify_regime_from_fvol_s6(fvol_s6);
    let ix = kernel::write_regime_signal_ix(
        &keeper.pubkey(),
        &keeper.pubkey(),
        &halcyon_il_protection::ID,
        halcyon_kernel::WriteRegimeSignalArgs {
            product_program_id: halcyon_il_protection::ID,
            fvol_s6,
        },
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), keeper, vec![ix]).await?;
    println!(
        "keepers fire-regime: signature={signature} fvol_s6={fvol_s6} regime={:?} sigma_multiplier_s6={}",
        regime.regime, regime.sigma_multiplier_s6
    );
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
