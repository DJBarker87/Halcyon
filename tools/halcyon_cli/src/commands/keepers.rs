use anyhow::Result;
use clap::{Args as ClapArgs, Subcommand};
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{decode::fetch_anchor_account, sol_autocall, tx};

use crate::client::CliContext;

#[derive(Debug, Subcommand)]
pub enum KeeperCmd {
    FireObservation(FireObservationArgs),
    FireHedge(FireHedgeArgs),
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

pub async fn run(ctx: &CliContext, cmd: KeeperCmd) -> Result<()> {
    match cmd {
        KeeperCmd::FireObservation(a) => fire_observation(ctx, a).await,
        KeeperCmd::FireHedge(a) => fire_hedge(ctx, a).await,
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
