use anyhow::{bail, Context, Result};
use clap::Args as ClapArgs;
use solana_sdk::{pubkey::Pubkey, signer::Signer};

use halcyon_client_sdk::{flagship_autocall, pda, tx};

use crate::client::CliContext;

#[derive(Debug, ClapArgs)]
pub struct Args {
    pub notional: u64,
    #[arg(long, value_name = "PUBKEY")]
    pub usdc_mint: Option<String>,
    #[arg(long)]
    pub pyth_spy: String,
    #[arg(long)]
    pub pyth_qqq: String,
    #[arg(long)]
    pub pyth_iwm: String,
    #[arg(long, default_value_t = 50)]
    pub premium_slippage_bps: u16,
    #[arg(long, default_value_t = 50)]
    pub max_liability_floor_bps: u16,
    #[arg(long, default_value_t = 25)]
    pub entry_drift_bps: u16,
    #[arg(long, default_value_t = 150)]
    pub max_quote_slot_delta: u64,
    #[arg(long, default_value_t = 30)]
    pub max_expiry_delta_secs: i64,
    #[arg(long)]
    pub policy_id: Option<String>,
}

pub async fn run(ctx: &CliContext, args: Args) -> Result<()> {
    let buyer = ctx.signer()?;
    let usdc_mint = ctx.resolve_usdc_mint(args.usdc_mint.as_deref())?;
    let pyth_spy = CliContext::parse_pubkey("pyth_spy", &args.pyth_spy)?;
    let pyth_qqq = CliContext::parse_pubkey("pyth_qqq", &args.pyth_qqq)?;
    let pyth_iwm = CliContext::parse_pubkey("pyth_iwm", &args.pyth_iwm)?;
    let preview = flagship_autocall::simulate_preview_quote(
        ctx.rpc.as_ref(),
        buyer,
        pyth_spy,
        pyth_qqq,
        pyth_iwm,
        args.notional,
    )
    .await
    .context("previewing live Flagship quote before buy")?;
    if preview.max_liability == 0
        || preview.fair_coupon_bps_s6 == 0
        || preview.offered_coupon_bps_s6 == 0
    {
        bail!(
            "no quote: live fair coupon is below the issuance floor or pricing confidence is low"
        );
    }

    let policy_id = match args.policy_id.as_deref() {
        Some(policy_id) => CliContext::parse_pubkey("policy_id", policy_id)?,
        None => Pubkey::new_unique(),
    };
    let max_premium = apply_bps_ceil(preview.premium, args.premium_slippage_bps)?;
    let min_max_liability = apply_bps_floor(
        preview.max_liability,
        10_000u16.saturating_sub(args.max_liability_floor_bps),
    )?;
    let ix = flagship_autocall::accept_quote_ix(
        &buyer.pubkey(),
        &usdc_mint,
        pyth_spy,
        pyth_qqq,
        pyth_iwm,
        flagship_autocall::AcceptQuoteArgs {
            policy_id,
            notional_usdc: args.notional,
            max_premium,
            min_max_liability,
            min_offered_coupon_bps_s6: preview.offered_coupon_bps_s6,
            preview_quote_slot: preview.quote_slot,
            max_quote_slot_delta: args.max_quote_slot_delta,
            preview_entry_spy_price_s6: preview.entry_spy_price_s6,
            preview_entry_qqq_price_s6: preview.entry_qqq_price_s6,
            preview_entry_iwm_price_s6: preview.entry_iwm_price_s6,
            max_entry_price_deviation_bps: args.entry_drift_bps,
            preview_expiry_ts: preview.expiry_ts,
            max_expiry_delta_secs: args.max_expiry_delta_secs,
        },
    );
    let signature = tx::send_instructions(ctx.rpc.as_ref(), buyer, vec![ix]).await?;
    let (policy, _) = pda::policy(&policy_id);
    let (terms, _) = pda::terms_for(&halcyon_flagship_autocall::ID, &policy_id);
    println!("buy-flagship: signature={signature}");
    println!("  policy_id={policy_id}");
    println!("  policy={policy}");
    println!("  product_terms={terms}");
    println!("  notional_usdc={}", args.notional);
    println!("  premium_usdc={}", preview.premium);
    println!("  max_liability_usdc={}", preview.max_liability);
    println!("  offered_coupon_bps_s6={}", preview.offered_coupon_bps_s6);
    println!(
        "  preview_entry_spy_price_s6={}",
        preview.entry_spy_price_s6
    );
    println!(
        "  preview_entry_qqq_price_s6={}",
        preview.entry_qqq_price_s6
    );
    println!(
        "  preview_entry_iwm_price_s6={}",
        preview.entry_iwm_price_s6
    );
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
