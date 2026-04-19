use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

use halcyon_client_sdk::{pda, sol_autocall, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub notional: u64,
    #[arg(long)]
    pub usdc_mint: String,
    #[arg(long)]
    pub pyth_sol: String,
    #[arg(long, default_value_t = 50)]
    pub slippage_bps: u16,
    #[arg(long, default_value_t = 32)]
    pub max_quote_slot_delta: u64,
    #[arg(long, default_value_t = 30)]
    pub max_expiry_delta_secs: i64,
    #[arg(long)]
    pub policy_id: Option<String>,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let buyer = ctx.signer()?;
    let usdc_mint = CliContext::parse_pubkey("usdc_mint", &args.usdc_mint)?;
    let pyth_sol = CliContext::parse_pubkey("pyth_sol", &args.pyth_sol)?;
    let preview =
        sol_autocall::simulate_preview_quote(ctx.rpc.as_ref(), buyer, pyth_sol, args.notional)
            .await
            .context("previewing live quote before buy")?;
    if preview.max_liability == 0
        || preview.fair_coupon_bps_s6 == 0
        || preview.offered_coupon_bps_s6 == 0
    {
        bail!("no quote: live fair coupon is below the 50 bps issuance floor or pricing confidence is low");
    }

    let policy_id = match args.policy_id.as_deref() {
        Some(policy_id) => CliContext::parse_pubkey("policy_id", policy_id)?,
        None => Pubkey::new_unique(),
    };
    let max_premium = apply_bps_ceil(preview.premium, args.slippage_bps)?;
    let min_max_liability = apply_bps_floor(
        preview.max_liability,
        10_000u16.saturating_sub(args.slippage_bps),
    )?;
    let ix = sol_autocall::accept_quote_ix(
        &buyer.pubkey(),
        &usdc_mint,
        pyth_sol,
        sol_autocall::AcceptQuoteArgs {
            policy_id,
            notional_usdc: args.notional,
            max_premium,
            min_max_liability,
            min_offered_coupon_bps_s6: preview.offered_coupon_bps_s6,
            preview_quote_slot: preview.quote_slot,
            max_quote_slot_delta: args.max_quote_slot_delta,
            preview_entry_price_s6: preview.entry_price_s6,
            max_entry_price_deviation_bps: args.slippage_bps,
            preview_expiry_ts: preview.expiry_ts,
            max_expiry_delta_secs: args.max_expiry_delta_secs,
        },
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), buyer, vec![ix]).await?;
    let (policy, _) = pda::policy(&policy_id);
    let (terms, _) = pda::terms(&policy_id);
    println!("buy: signature={signature}");
    println!("  policy_id={policy_id}");
    println!("  policy={policy}");
    println!("  product_terms={terms}");
    println!("  notional_usdc={}", args.notional);
    println!("  principal_escrow_usdc={}", args.notional);
    println!("  premium_usdc={}", preview.premium);
    println!("  max_liability_usdc={}", preview.max_liability);
    println!("  offered_coupon_bps_s6={}", preview.offered_coupon_bps_s6);
    println!("  preview_entry_price_s6={}", preview.entry_price_s6);
    println!("  preview_expiry_ts={}", preview.expiry_ts);
    println!("  preview_quote_slot={}", preview.quote_slot);
    println!(
        "  liability_buffer_usdc={}",
        preview.max_liability.saturating_sub(args.notional)
    );
    println!("  max_premium_bound={max_premium}");
    println!("  min_max_liability_bound={min_max_liability}");
    println!(
        "  min_offered_coupon_bps_s6_bound={}",
        preview.offered_coupon_bps_s6
    );
    println!("  max_quote_slot_delta={}", args.max_quote_slot_delta);
    println!("  max_entry_price_deviation_bps={}", args.slippage_bps);
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
