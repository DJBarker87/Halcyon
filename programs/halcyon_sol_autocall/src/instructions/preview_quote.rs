//! `preview_quote` — read-only, driven by `simulateTransaction`.
//!
//! Returns `QuotePreview` the CLI can apply slippage against when the user
//! confirms a `buy`. Does not mutate state and does not CPI; handler returns
//! zeroes for no-quote states so a caller UI can render "no quote right now"
//! without having to inspect a tx-level abort.

use anchor_lang::prelude::*;
use halcyon_common::seeds;
use halcyon_kernel::state::{ProductRegistryEntry, ProtocolConfig, RegimeSignal, VaultSigma};

use crate::pricing::{
    self, compose_pricing_sigma, require_pod_deim_table_match, require_protocol_unpaused,
    require_regime_fresh, require_sigma_fresh, solve_quote, ConfidenceGate,
};
use crate::state::{SolAutocallReducedOperators, CURRENT_ENGINE_VERSION};

#[derive(AnchorSerialize, AnchorDeserialize, Clone, Debug)]
pub struct QuotePreview {
    /// Upfront premium charged at issuance. SOL Autocall v1 follows the
    /// economics docs and escrows principal instead, so this is currently 0.
    pub premium: u64,
    pub max_liability: u64,
    /// Fair coupon per observation at SCALE_6 bps. Zero when the preview is in
    /// a no-quote state, currently either low pricing confidence or fair
    /// coupon below the 50 bps issuance floor from the economics docs.
    pub fair_coupon_bps_s6: i64,
    pub offered_coupon_bps_s6: i64,
    pub quote_slot: u64,
    pub engine_version: u16,
    pub entry_price_s6: i64,
    pub expiry_ts: i64,
}

#[derive(Accounts)]
pub struct PreviewQuote<'info> {
    #[account(seeds = [seeds::PROTOCOL_CONFIG], seeds::program = halcyon_kernel::ID, bump)]
    pub protocol_config: Account<'info, ProtocolConfig>,
    #[account(
        seeds = [seeds::PRODUCT_REGISTRY, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = product_registry_entry.product_program_id == crate::ID,
        constraint = product_registry_entry.active @ halcyon_common::HalcyonError::ProductNotRegistered,
    )]
    pub product_registry_entry: Account<'info, ProductRegistryEntry>,
    #[account(
        seeds = [seeds::VAULT_SIGMA, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = vault_sigma.product_program_id == crate::ID,
    )]
    pub vault_sigma: Account<'info, VaultSigma>,
    #[account(
        seeds = [seeds::REGIME_SIGNAL, crate::ID.as_ref()],
        seeds::program = halcyon_kernel::ID,
        bump,
        constraint = regime_signal.product_program_id == crate::ID,
    )]
    pub regime_signal: Account<'info, RegimeSignal>,
    #[account(seeds = [seeds::REDUCED_OPERATORS], bump)]
    pub reduced_operators: Account<'info, SolAutocallReducedOperators>,
    /// CHECK: Pyth price account, validated by halcyon_oracles.
    pub pyth_sol: UncheckedAccount<'info>,
    pub clock: Sysvar<'info, Clock>,
}

pub fn handler(ctx: Context<PreviewQuote>, notional_usdc: u64) -> Result<QuotePreview> {
    let now = ctx.accounts.clock.unix_timestamp;
    pricing::cu_trace("preview_quote:start");

    require_protocol_unpaused(&ctx.accounts.protocol_config)?;
    require_pod_deim_table_match(&ctx.accounts.protocol_config)?;
    require!(
        !ctx.accounts.product_registry_entry.paused,
        halcyon_common::HalcyonError::IssuancePausedPerProduct
    );
    require_sigma_fresh(
        &ctx.accounts.vault_sigma,
        now,
        ctx.accounts.protocol_config.sigma_staleness_cap_secs,
    )?;
    require_regime_fresh(
        &ctx.accounts.regime_signal,
        now,
        ctx.accounts.protocol_config.regime_staleness_cap_secs,
    )?;
    pricing::cu_trace("preview_quote:after_state_checks");

    let pyth = halcyon_oracles::read_pyth_price(
        &ctx.accounts.pyth_sol.to_account_info(),
        &halcyon_oracles::feed_ids::SOL_USD,
        &crate::ID,
        &ctx.accounts.clock,
        ctx.accounts.protocol_config.pyth_quote_staleness_cap_secs,
    )?;
    pricing::cu_trace("preview_quote:after_oracle_read");

    // Sigma composition follows the math stack: annualise daily EWMA variance
    // on a 365-day basis, apply the regime multiplier, then enforce the floor.
    let sigma_pricing_s6 = compose_pricing_sigma(
        &ctx.accounts.vault_sigma,
        &ctx.accounts.regime_signal,
        crate::pricing::protocol_sigma_floor_annualised_s6(&ctx.accounts.protocol_config),
    )?;
    pricing::cu_trace("preview_quote:after_sigma_compose");

    // Preview uses `SignalOnly`: no-quote conditions return zeros instead of
    // aborting, so `simulateTransaction` still yields a parseable result.
    let quote = solve_quote(
        sigma_pricing_s6,
        &ctx.accounts.reduced_operators,
        ctx.accounts.vault_sigma.last_update_slot,
        ctx.accounts.regime_signal.last_update_slot,
        notional_usdc,
        protocol_share_bps(&ctx.accounts.protocol_config),
        protocol_margin_bps(&ctx.accounts.protocol_config),
        now,
        ConfidenceGate::SignalOnly,
    )?;
    pricing::cu_trace("preview_quote:after_solve_quote");

    let preview = QuotePreview {
        premium: quote.premium,
        max_liability: quote.max_liability,
        fair_coupon_bps_s6: quote.fair_coupon_bps_s6,
        offered_coupon_bps_s6: quote.offered_coupon_bps_s6,
        quote_slot: quote.quote_slot,
        engine_version: CURRENT_ENGINE_VERSION,
        entry_price_s6: pyth.price_s6,
        expiry_ts: quote.expiry_ts,
    };
    pricing::cu_trace("preview_quote:end");
    Ok(preview)
}

fn protocol_share_bps(cfg: &ProtocolConfig) -> u16 {
    cfg.sol_autocall_quote_share_bps
}

fn protocol_margin_bps(cfg: &ProtocolConfig) -> u16 {
    cfg.sol_autocall_issuer_margin_bps
}
