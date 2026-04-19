use anyhow::Result;
use clap::Args as ClapArgs;
use solana_sdk::signer::Signer;

use halcyon_client_sdk::{
    decode::fetch_anchor_account_opt,
    kernel::{self, InitializeProtocolArgs},
    pda, tx,
};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    #[arg(long)]
    pub usdc_mint: String,
    #[arg(long, default_value_t = 7_500)]
    pub utilization_cap_bps: u64,
    #[arg(long, default_value_t = 9_000)]
    pub senior_share_bps: u16,
    #[arg(long, default_value_t = 300)]
    pub junior_share_bps: u16,
    #[arg(long, default_value_t = 700)]
    pub treasury_share_bps: u16,
    #[arg(long, default_value_t = 86_400)]
    pub senior_cooldown_secs: i64,
    #[arg(long, default_value_t = 300)]
    pub ewma_rate_limit_secs: i64,
    #[arg(long, default_value_t = 21_600)]
    pub sigma_staleness_cap_secs: i64,
    #[arg(long, default_value_t = 86_400)]
    pub regime_staleness_cap_secs: i64,
    #[arg(long, default_value_t = 86_400)]
    pub regression_staleness_cap_secs: i64,
    #[arg(long, default_value_t = 30)]
    pub pyth_quote_staleness_cap_secs: i64,
    #[arg(long, default_value_t = 60)]
    pub pyth_settle_staleness_cap_secs: i64,
    #[arg(long, default_value_t = 300)]
    pub quote_ttl_secs: i64,
    #[arg(long, default_value_t = 600_000)]
    pub sigma_floor_annualised_s6: i64,
    #[arg(long, default_value_t = 7_500)]
    pub sol_autocall_quote_share_bps: u16,
    #[arg(long, default_value_t = 50)]
    pub sol_autocall_issuer_margin_bps: u16,
    #[arg(long)]
    pub treasury_destination: Option<String>,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let admin = ctx.signer()?;
    let usdc_mint = CliContext::parse_pubkey("usdc_mint", &args.usdc_mint)?;
    let (protocol_config, _) = pda::protocol_config();
    if fetch_anchor_account_opt::<halcyon_kernel::state::ProtocolConfig>(
        ctx.rpc.as_ref(),
        &protocol_config,
    )
    .await?
    .is_some()
    {
        println!("init-protocol: ProtocolConfig already exists at {protocol_config}");
        return Ok(());
    }

    let treasury_destination = match args.treasury_destination.as_deref() {
        Some(dest) => CliContext::parse_pubkey("treasury_destination", dest)?,
        None => pda::associated_token_account(&admin.pubkey(), &usdc_mint),
    };
    let ix = kernel::initialize_protocol_ix(
        &admin.pubkey(),
        &usdc_mint,
        InitializeProtocolArgs {
            utilization_cap_bps: args.utilization_cap_bps,
            senior_share_bps: args.senior_share_bps,
            junior_share_bps: args.junior_share_bps,
            treasury_share_bps: args.treasury_share_bps,
            senior_cooldown_secs: args.senior_cooldown_secs,
            ewma_rate_limit_secs: args.ewma_rate_limit_secs,
            sigma_staleness_cap_secs: args.sigma_staleness_cap_secs,
            regime_staleness_cap_secs: args.regime_staleness_cap_secs,
            regression_staleness_cap_secs: args.regression_staleness_cap_secs,
            pyth_quote_staleness_cap_secs: args.pyth_quote_staleness_cap_secs,
            pyth_settle_staleness_cap_secs: args.pyth_settle_staleness_cap_secs,
            quote_ttl_secs: args.quote_ttl_secs,
            sigma_floor_annualised_s6: args.sigma_floor_annualised_s6,
            sol_autocall_quote_share_bps: args.sol_autocall_quote_share_bps,
            sol_autocall_issuer_margin_bps: args.sol_autocall_issuer_margin_bps,
            treasury_destination,
        },
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), admin, vec![ix]).await?;
    let (vault_usdc, _) = pda::vault_usdc(&usdc_mint);
    let (treasury_usdc, _) = pda::treasury_usdc(&usdc_mint);
    println!("init-protocol: signature={signature}");
    println!("  protocol_config={protocol_config}");
    println!("  vault_usdc={vault_usdc}");
    println!("  treasury_usdc={treasury_usdc}");
    println!("  treasury_destination={treasury_destination}");
    Ok(())
}
