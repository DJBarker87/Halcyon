//! Product-side quote/issuance helpers for `preview_quote` and `accept_quote`.
//!
//! The heavy pricing engines live in `halcyon_sol_autocall_quote`; scale-fixed
//! primitives live in `solmath_core`. This module is the product-side glue that:
//!   1. Composes live pricing sigma from kernel EWMA + regime state.
//!   2. Calls the current live quote engine.
//!   3. Applies the canonical offered-coupon formula
//!      (`fair_bps × quote_share − margin_bps`, additive margin).
//!   4. Packages `(upfront_premium, max_liability)` for the kernel.
//!   5. Computes the `terms_hash` that binds the policy.

use anchor_lang::prelude::*;
use halcyon_common::HalcyonError;
use halcyon_kernel::state::{ProtocolConfig, RegimeSignal, VaultSigma};
use halcyon_sol_autocall_quote::autocall_v2::{
    AutocallParams, AutocallPriceResult, AUTOCALL_LOG_6, KNOCK_IN_LOG_6,
};
use halcyon_sol_autocall_quote::autocall_v2_e11::solve_fair_coupon_deim_from_precomputed_const as solve_keeper_deim;
use halcyon_sol_autocall_quote::generated::pod_deim_table::POD_DEIM_TABLE_SHA256;
use solana_sha256_hasher::hash;
use solmath_core::{fp_mul, fp_sqrt, SCALE};

use crate::errors::SolAutocallError;
use crate::state::{
    CURRENT_ENGINE_VERSION, KI_BARRIER_BPS, MATURITY_DAYS, NO_AUTOCALL_FIRST_N_OBS,
    OBSERVATION_COUNT, OBSERVATION_INTERVAL_DAYS, SECONDS_PER_DAY, SolAutocallReducedOperators,
};

#[cfg(target_os = "solana")]
unsafe extern "C" {
    fn sol_log_compute_units_();
    fn sol_log_(message: *const u8, length: u64);
}

#[inline(always)]
pub(crate) fn cu_trace(label: &str) {
    #[cfg(target_os = "solana")]
    unsafe {
        sol_log_(label.as_ptr(), label.len() as u64);
        sol_log_compute_units_();
    }
    #[cfg(not(target_os = "solana"))]
    {
        let _ = label;
    }
}

pub const MIN_FAIR_COUPON_BPS: u64 = 50;
pub const KEEPER_DEIM_SIGMA_MIN_S6: i64 = 500_000;
pub const KEEPER_DEIM_SIGMA_MAX_S6: i64 = 2_500_000;

/// SCALE_6 constant used by the offered-coupon formula.
const SCALE_6_I128: i128 = 1_000_000;

pub struct QuoteOutputs {
    pub premium: u64,
    pub max_liability: u64,
    pub offered_coupon_bps_s6: i64,
    pub fair_coupon_bps_s6: i64,
    pub expiry_ts: i64,
    pub quote_slot: u64,
    pub engine_version: u16,
}

#[derive(Clone, Copy, Debug)]
pub enum ConfidenceGate {
    /// Preview path — return zeros on low confidence.
    SignalOnly,
    /// Issuance path — fail the instruction on low confidence.
    Abort,
}

fn zero_quote() -> QuoteOutputs {
    QuoteOutputs {
        premium: 0,
        max_liability: 0,
        offered_coupon_bps_s6: 0,
        fair_coupon_bps_s6: 0,
        expiry_ts: 0,
        quote_slot: 0,
        engine_version: CURRENT_ENGINE_VERSION,
    }
}

pub fn compose_pricing_sigma(
    vault_sigma: &VaultSigma,
    regime_signal: &RegimeSignal,
    sigma_floor_annualised_s6: i64,
) -> Result<i64> {
    let floor_s6 = sigma_floor_annualised_s6.max(regime_signal.sigma_floor_annualised_s6);
    require!(floor_s6 > 0, SolAutocallError::InvalidSigmaFloor);

    if vault_sigma.ewma_var_daily_s12 <= 0 {
        return Ok(floor_s6);
    }

    // SOL trades continuously; the math stack annualises on a 365-day basis.
    let annual_variance_s12 = fp_mul(
        vault_sigma.ewma_var_daily_s12 as u128,
        365u128.checked_mul(SCALE).ok_or(HalcyonError::Overflow)?,
    )
    .map_err(|_| error!(HalcyonError::Overflow))?;
    let sigma_annual_s12 =
        fp_sqrt(annual_variance_s12).map_err(|_| error!(HalcyonError::Overflow))? as i128;
    let sigma_regime_s12 = sigma_annual_s12
        .checked_mul(regime_signal.sigma_multiplier_s6.max(0) as i128)
        .and_then(|v| v.checked_div(1_000_000))
        .ok_or(HalcyonError::Overflow)?;
    let sigma_s6 = i64::try_from(
        sigma_regime_s12
            .checked_div(1_000_000)
            .ok_or(HalcyonError::Overflow)?,
    )
    .map_err(|_| error!(HalcyonError::Overflow))?;

    Ok(sigma_s6.max(floor_s6))
}

/// Call the keeper-fed fixed-product POD-DEIM pricer when the current reduced
/// operator matches the live sigma, otherwise return a no-quote / abort, then
/// apply Dom's offered-coupon formula and package the `(upfront_premium,
/// max_liability)` pair for issuance.
///
/// Per the economics docs, SOL Autocall escrows buyer principal on issue and
/// pays coupons from a separate coupon vault on each observation date. The
/// underwriting reserve therefore only carries principal / terminal-redemption
/// risk, not the coupon stream.
pub fn solve_quote(
    sigma_pricing_s6: i64,
    reduced_operators: &SolAutocallReducedOperators,
    vault_sigma_slot: u64,
    regime_signal_slot: u64,
    notional_usdc: u64,
    quote_share_bps: u16,
    issuer_margin_bps: u16,
    issued_at: i64,
    gate: ConfidenceGate,
) -> Result<QuoteOutputs> {
    cu_trace("solve_quote:start");

    let contract = AutocallParams {
        n_obs: OBSERVATION_COUNT,
        knock_in_log_6: KNOCK_IN_LOG_6,
        autocall_log_6: AUTOCALL_LOG_6,
        no_autocall_first_n_obs: NO_AUTOCALL_FIRST_N_OBS as usize,
    };

    let sigma_in_band =
        (KEEPER_DEIM_SIGMA_MIN_S6..=KEEPER_DEIM_SIGMA_MAX_S6).contains(&sigma_pricing_s6);
    let reduced_ops_current = sigma_in_band
        && contract.n_obs == OBSERVATION_COUNT
        && reduced_operators.version == SolAutocallReducedOperators::CURRENT_VERSION
        && reduced_operators.is_complete()
        && reduced_operators.matches_current_tables()
        && reduced_operators.sigma_ann_s6 == sigma_pricing_s6
        && reduced_operators.source_vault_sigma_slot == vault_sigma_slot
        && reduced_operators.source_regime_signal_slot == regime_signal_slot;
    cu_trace(if reduced_ops_current {
        "solve_quote:keeper_deim_ready=true"
    } else {
        "solve_quote:keeper_deim_ready=false"
    });

    if !sigma_in_band {
        return match gate {
            ConfidenceGate::Abort => err!(SolAutocallError::PricingSigmaOutOfBand),
            ConfidenceGate::SignalOnly => Ok(zero_quote()),
        };
    }

    if !reduced_ops_current {
        return match gate {
            ConfidenceGate::Abort => err!(SolAutocallError::ReducedOperatorsStale),
            ConfidenceGate::SignalOnly => Ok(zero_quote()),
        };
    }

    cu_trace("solve_quote:before_keeper_deim");
    let direct_quote: AutocallPriceResult = solve_keeper_deim(
        &reduced_operators.p_red_v,
        &reduced_operators.p_red_u,
        &contract,
    )
    .map_err(|_| error!(SolAutocallError::QuoteRecomputeMismatch))?;
    cu_trace("solve_quote:after_keeper_deim");

    match (gate, direct_quote.fair_coupon_bps < MIN_FAIR_COUPON_BPS) {
        (ConfidenceGate::Abort, true) => {
            return err!(SolAutocallError::FairCouponBelowIssuanceFloor);
        }
        (ConfidenceGate::SignalOnly, true) => {
            return Ok(zero_quote());
        }
        _ => {}
    }

    // Pricer output is `fair_coupon_bps: u64` — per-observation fair coupon
    // in integer bps. Widen to SCALE_6 bps (i.e. bps × 1e6) so the offered-
    // coupon formula operates in the shared SCALE_6 units.
    let fair_bps_s6: i64 = (direct_quote.fair_coupon_bps as i128)
        .checked_mul(SCALE_6_I128)
        .and_then(|v| i64::try_from(v).ok())
        .ok_or(HalcyonError::Overflow)?;
    let quote_share_s6: i64 = (quote_share_bps as i64)
        .checked_mul(100) // bps → SCALE_6 of a unit fraction: (bps / 10_000) × 1e6 = bps × 100
        .ok_or(HalcyonError::Overflow)?;
    let margin_bps_s6: i64 = (issuer_margin_bps as i64)
        .checked_mul(SCALE_6_I128 as i64)
        .ok_or(HalcyonError::Overflow)?;

    let offered_bps_s6 = offered_coupon_bps_s6(fair_bps_s6, quote_share_s6, margin_bps_s6);
    require!(offered_bps_s6 > 0, SolAutocallError::QuoteRecomputeMismatch);

    let expiry_ts = issued_at
        .checked_add(MATURITY_DAYS as i64 * SECONDS_PER_DAY)
        .ok_or(HalcyonError::Overflow)?;

    Ok(QuoteOutputs {
        premium: 0,
        max_liability: notional_usdc,
        offered_coupon_bps_s6: offered_bps_s6,
        fair_coupon_bps_s6: fair_bps_s6,
        expiry_ts,
        quote_slot: Clock::get()?.slot,
        engine_version: CURRENT_ENGINE_VERSION,
    })
}

/// Canonical offered-coupon formula per `worst_of_math_stack.md §6` and the
/// math-canon memory. Additive margin — NOT `× (1 − margin)`.
///
/// Inputs at SCALE_6:
///   fair_bps_s6          — fair coupon per observation in bps × 1e6
///   quote_share_s6       — quote share as a fraction × 1e6 (0.75 ≡ 750_000)
///   issuer_margin_bps_s6 — issuer margin in bps × 1e6
#[inline]
pub fn offered_coupon_bps_s6(
    fair_bps_s6: i64,
    quote_share_s6: i64,
    issuer_margin_bps_s6: i64,
) -> i64 {
    ((fair_bps_s6 as i128 * quote_share_s6 as i128 / SCALE_6_I128) as i64)
        .saturating_sub(issuer_margin_bps_s6)
}

pub fn coupon_per_observation_usdc(notional_usdc: u64, offered_coupon_bps_s6: i64) -> Result<u64> {
    require!(
        offered_coupon_bps_s6 >= 0,
        SolAutocallError::QuoteRecomputeMismatch
    );
    let coupon = (notional_usdc as u128)
        .checked_mul(offered_coupon_bps_s6 as u128)
        .and_then(|v| v.checked_div(10_000))
        .and_then(|v| v.checked_div(SCALE_6_I128 as u128))
        .ok_or(HalcyonError::Overflow)?;
    u64::try_from(coupon).map_err(|_| error!(HalcyonError::Overflow))
}

/// Guard a previewed buyer quote against drift before `accept_quote`.
///
/// Any product that previews a sigma-sensitive quote and accepts it later must
/// bind both the buyer's economic floor and the preview's freshness. When IL
/// Protection grows its L3 issuance path, it should mirror this pattern instead
/// of accepting a recomputed live quote with those fields unbounded.
pub fn require_quote_acceptance_bounds(
    quote: &QuoteOutputs,
    min_offered_coupon_bps_s6: i64,
    preview_quote_slot: u64,
    max_quote_slot_delta: u64,
    live_entry_price_s6: i64,
    preview_entry_price_s6: i64,
    max_entry_price_deviation_bps: u16,
    preview_expiry_ts: i64,
    max_expiry_delta_secs: i64,
) -> Result<()> {
    require!(
        quote.offered_coupon_bps_s6 >= min_offered_coupon_bps_s6,
        HalcyonError::SlippageExceeded
    );
    require!(
        quote.quote_slot >= preview_quote_slot,
        HalcyonError::SlippageExceeded
    );
    require!(
        quote.quote_slot - preview_quote_slot <= max_quote_slot_delta,
        HalcyonError::SlippageExceeded
    );
    require!(live_entry_price_s6 > 0, SolAutocallError::InvalidEntryPrice);
    require!(
        preview_entry_price_s6 > 0,
        SolAutocallError::InvalidEntryPrice
    );
    require!(max_expiry_delta_secs >= 0, HalcyonError::SlippageExceeded);
    let price_delta = i128::from(live_entry_price_s6)
        .checked_sub(i128::from(preview_entry_price_s6))
        .ok_or(HalcyonError::Overflow)?
        .abs();
    let price_delta_bps = price_delta
        .checked_mul(10_000)
        .and_then(|v| v.checked_div(i128::from(preview_entry_price_s6)))
        .ok_or(HalcyonError::Overflow)?;
    require!(
        price_delta_bps <= i128::from(max_entry_price_deviation_bps),
        HalcyonError::SlippageExceeded
    );
    require!(
        quote.expiry_ts >= preview_expiry_ts,
        HalcyonError::SlippageExceeded
    );
    let expiry_delta_secs = quote
        .expiry_ts
        .checked_sub(preview_expiry_ts)
        .ok_or(HalcyonError::Overflow)?;
    require!(
        expiry_delta_secs <= max_expiry_delta_secs,
        HalcyonError::SlippageExceeded
    );
    Ok(())
}

pub fn build_observation_schedule(issued_at: i64) -> Result<[i64; OBSERVATION_COUNT]> {
    let mut schedule = [0i64; OBSERVATION_COUNT];
    for (i, slot) in schedule.iter_mut().enumerate() {
        let day_offset = (i as i64 + 1) * OBSERVATION_INTERVAL_DAYS as i64;
        *slot = issued_at
            .checked_add(day_offset * SECONDS_PER_DAY)
            .ok_or(HalcyonError::Overflow)?;
    }
    Ok(schedule)
}

pub fn derive_barriers_from_entry(entry_price_s6: i64) -> Result<(i64, i64, i64)> {
    require!(entry_price_s6 > 0, SolAutocallError::InvalidEntryPrice);
    let autocall = bps_of(entry_price_s6, crate::state::AUTOCALL_BARRIER_BPS)?;
    let coupon = bps_of(entry_price_s6, crate::state::COUPON_BARRIER_BPS)?;
    let ki = bps_of(entry_price_s6, KI_BARRIER_BPS)?;
    Ok((autocall, coupon, ki))
}

fn bps_of(price_s6: i64, bps: u64) -> Result<i64> {
    let out = (price_s6 as i128)
        .checked_mul(bps as i128)
        .and_then(|v| v.checked_div(10_000))
        .ok_or(HalcyonError::Overflow)?;
    i64::try_from(out).map_err(|_| error!(HalcyonError::Overflow))
}

/// Hash the exact bytes that will populate the `ProductTerms` account on chain.
///
/// `finalize_policy` (kernel-side, K2) rehashes `product_terms.try_borrow_data()`
/// and compares the digest against `policy_header.terms_hash`. To make the two
/// hashes match, this helper serialises the full account shape the kernel will
/// see: 8-byte discriminator + borsh-encoded `SolAutocallTerms`. Accepts the
/// already-populated `SolAutocallTerms` struct and does zero pricing-side
/// decisions here.
pub fn hash_product_terms(terms: &crate::state::SolAutocallTerms) -> Result<[u8; 32]> {
    use anchor_lang::Discriminator;
    let mut buf = Vec::with_capacity(8 + crate::state::SolAutocallTerms::INIT_SPACE);
    buf.extend_from_slice(&crate::state::SolAutocallTerms::DISCRIMINATOR);
    terms
        .serialize(&mut buf)
        .map_err(|_| error!(HalcyonError::Overflow))?;
    Ok(hash(&buf).to_bytes())
}

pub fn require_sigma_fresh(vault_sigma: &VaultSigma, now: i64, cap_secs: i64) -> Result<()> {
    let age = now
        .checked_sub(vault_sigma.ewma_last_timestamp)
        .ok_or(HalcyonError::Overflow)?;
    require!(age <= cap_secs, HalcyonError::SigmaStale);
    Ok(())
}

pub fn require_regime_fresh(regime_signal: &RegimeSignal, now: i64, cap_secs: i64) -> Result<()> {
    let age = now
        .checked_sub(regime_signal.last_update_ts)
        .ok_or(HalcyonError::Overflow)?;
    require!(age <= cap_secs, HalcyonError::RegimeStale);
    Ok(())
}

pub fn require_protocol_unpaused(config: &ProtocolConfig) -> Result<()> {
    require!(!config.issuance_paused_global, HalcyonError::PausedGlobally);
    Ok(())
}

pub fn require_pod_deim_table_match(config: &ProtocolConfig) -> Result<()> {
    require!(
        config.pod_deim_table_sha256 == POD_DEIM_TABLE_SHA256,
        HalcyonError::CorrectionTableHashMismatch
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coupon_can_move_while_accept_bounds_stay_constant() {
        let notional_usdc = 1_000_000u64;
        let coupon_a = offered_coupon_bps_s6(100_000_000, 750_000, 50_000_000);
        let coupon_b = offered_coupon_bps_s6(250_000_000, 750_000, 50_000_000);
        assert_ne!(
            coupon_a, coupon_b,
            "different fair coupons should produce different offered coupons"
        );

        let quote_a = QuoteOutputs {
            premium: 0,
            max_liability: notional_usdc,
            offered_coupon_bps_s6: coupon_a,
            fair_coupon_bps_s6: 100_000_000,
            expiry_ts: 1,
            quote_slot: 1,
            engine_version: CURRENT_ENGINE_VERSION,
        };
        let quote_b = QuoteOutputs {
            premium: 0,
            max_liability: notional_usdc,
            offered_coupon_bps_s6: coupon_b,
            fair_coupon_bps_s6: 250_000_000,
            expiry_ts: 1,
            quote_slot: 2,
            engine_version: CURRENT_ENGINE_VERSION,
        };

        assert_eq!(
            quote_a.premium, quote_b.premium,
            "accept slippage keeps the same premium bound"
        );
        assert_eq!(
            quote_a.max_liability, quote_b.max_liability,
            "accept slippage keeps the same liability bound"
        );
    }

    #[test]
    fn quote_acceptance_bounds_reject_lower_coupon_and_stale_preview() {
        let quote = QuoteOutputs {
            premium: 0,
            max_liability: 1_000_000,
            offered_coupon_bps_s6: 120_000_000,
            fair_coupon_bps_s6: 150_000_000,
            expiry_ts: 1,
            quote_slot: 150,
            engine_version: CURRENT_ENGINE_VERSION,
        };

        assert!(
            require_quote_acceptance_bounds(
                &quote,
                120_000_000,
                125,
                25,
                100_000_000,
                100_500_000,
                50,
                1,
                0,
            )
            .is_ok(),
            "matching coupon floor and slot tolerance must pass"
        );
        assert!(
            require_quote_acceptance_bounds(
                &quote,
                130_000_000,
                125,
                25,
                100_000_000,
                100_500_000,
                50,
                1,
                0,
            )
            .is_err(),
            "buyer coupon floor must reject a worse live coupon"
        );
        assert!(
            require_quote_acceptance_bounds(
                &quote,
                120_000_000,
                100,
                25,
                100_000_000,
                100_500_000,
                50,
                1,
                0,
            )
            .is_err(),
            "preview slot tolerance must reject stale previews"
        );
    }

    #[test]
    fn quote_acceptance_bounds_reject_entry_and_expiry_drift() {
        let quote = QuoteOutputs {
            premium: 0,
            max_liability: 1_000_000,
            offered_coupon_bps_s6: 120_000_000,
            fair_coupon_bps_s6: 150_000_000,
            expiry_ts: 10,
            quote_slot: 150,
            engine_version: CURRENT_ENGINE_VERSION,
        };

        assert!(
            require_quote_acceptance_bounds(
                &quote,
                120_000_000,
                125,
                25,
                100_000_000,
                100_000_000,
                0,
                10,
                0,
            )
            .is_ok(),
            "identical previewed entry and expiry must pass"
        );
        assert!(
            require_quote_acceptance_bounds(
                &quote,
                120_000_000,
                125,
                25,
                102_000_000,
                100_000_000,
                100,
                10,
                0,
            )
            .is_err(),
            "entry-price drift above the caller tolerance must fail"
        );
        assert!(
            require_quote_acceptance_bounds(
                &quote,
                120_000_000,
                125,
                25,
                100_000_000,
                100_000_000,
                0,
                8,
                1,
            )
            .is_err(),
            "expiry drift above the caller tolerance must fail"
        );
    }
}
