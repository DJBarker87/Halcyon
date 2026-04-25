use anyhow::{Context, Result};
use clap::Args as ClapArgs;
use solana_sdk::{signature::Keypair, signer::Signer};

use halcyon_client_sdk::{il_protection, pda, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub insured_notional: u64,
    #[arg(long, value_name = "PUBKEY")]
    pub usdc_mint: Option<String>,
    #[arg(long)]
    pub pyth_sol: String,
    #[arg(long)]
    pub pyth_usdc: String,
    #[arg(long, default_value_t = 50)]
    pub premium_slippage_bps: u16,
    #[arg(long, default_value_t = 50)]
    pub max_liability_floor_bps: u16,
    #[arg(long, default_value_t = 25)]
    pub entry_drift_bps: u16,
    #[arg(long, default_value_t = 150)]
    pub max_quote_slot_delta: u64,
    #[arg(long, default_value_t = 60)]
    pub max_expiry_delta_secs: i64,
    #[arg(long)]
    pub policy_id: Option<String>,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let buyer = ctx.signer()?;
    let usdc_mint = ctx.resolve_usdc_mint(args.usdc_mint.as_deref())?;
    let pyth_sol = CliContext::parse_pubkey("pyth_sol", &args.pyth_sol)?;
    let pyth_usdc = CliContext::parse_pubkey("pyth_usdc", &args.pyth_usdc)?;
    let preview = il_protection::simulate_preview_quote(
        ctx.rpc.as_ref(),
        buyer,
        pyth_sol,
        pyth_usdc,
        args.insured_notional,
    )
    .await
    .context("previewing live IL quote before buy")?;

    let policy_id = match args.policy_id.as_deref() {
        Some(policy_id) => CliContext::parse_pubkey("policy_id", policy_id)?,
        None => Keypair::new().pubkey(),
    };
    let max_premium = apply_bps_ceil(preview.premium, args.premium_slippage_bps)?;
    let min_max_liability = apply_bps_floor(
        preview.max_liability,
        10_000u16.saturating_sub(args.max_liability_floor_bps),
    )?;
    let ix = il_protection::accept_quote_ix(
        &buyer.pubkey(),
        &usdc_mint,
        pyth_sol,
        pyth_usdc,
        il_protection::AcceptQuoteArgs {
            policy_id,
            insured_notional_usdc: args.insured_notional,
            max_premium,
            min_max_liability,
            preview_quote_slot: preview.quote_slot,
            max_quote_slot_delta: args.max_quote_slot_delta,
            preview_entry_sol_price_s6: preview.entry_sol_price_s6,
            preview_entry_usdc_price_s6: preview.entry_usdc_price_s6,
            max_entry_price_deviation_bps: args.entry_drift_bps,
            preview_expiry_ts: preview.expiry_ts,
            max_expiry_delta_secs: args.max_expiry_delta_secs,
        },
    );
    let signature =
        tx::send_compute_instructions_with_extra_signers(ctx.rpc.as_ref(), buyer, vec![ix], &[])
            .await?;
    let (policy, _) = pda::policy(&policy_id);
    let (terms, _) = pda::terms_for(&halcyon_il_protection::ID, &policy_id);
    println!("buy-il: signature={signature}");
    println!("  policy_id={policy_id}");
    println!("  policy={policy}");
    println!("  product_terms={terms}");
    println!("  insured_notional_usdc={}", args.insured_notional);
    println!("  premium_usdc={}", preview.premium);
    println!("  max_liability_usdc={}", preview.max_liability);
    println!("  sigma_pricing_s6={}", preview.sigma_pricing_s6);
    println!("  fvol_s6={}", preview.fvol_s6);
    println!("  regime={}", preview.regime);
    println!("  entry_sol_price_s6={}", preview.entry_sol_price_s6);
    println!("  entry_usdc_price_s6={}", preview.entry_usdc_price_s6);
    println!("  preview_quote_slot={}", preview.quote_slot);
    println!("  preview_expiry_ts={}", preview.expiry_ts);
    println!("  max_premium_bound={max_premium}");
    println!("  min_max_liability_bound={min_max_liability}");
    println!("  max_quote_slot_delta={}", args.max_quote_slot_delta);
    println!("  max_entry_price_deviation_bps={}", args.entry_drift_bps);
    println!("  premium_slippage_bps={}", args.premium_slippage_bps);
    println!("  max_liability_floor_bps={}", args.max_liability_floor_bps);
    println!("  max_expiry_delta_secs={}", args.max_expiry_delta_secs);
    Ok(())
}

fn apply_bps_ceil(value: u64, bps: u16) -> Result<u64> {
    let numerator = (value as u128)
        .checked_mul((10_000u128).saturating_add(bps as u128))
        .context("overflow applying slippage")?;
    Ok(((numerator + 9_999) / 10_000) as u64)
}

fn apply_bps_floor(value: u64, bps: u16) -> Result<u64> {
    let numerator = (value as u128)
        .checked_mul(bps as u128)
        .context("overflow applying slippage")?;
    Ok((numerator / 10_000) as u64)
}
