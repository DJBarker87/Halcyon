//! Halcyon WASM shim.
//!
//! Exposes solmath-core primitives AND halcyon-quote product pricers to the
//! browser via the `wasm32-unknown-unknown` C ABI. The browser loader is in
//! `app/wasm_loader.js`.
//!
//! All boundary values are `f64`. Internally we convert to the fixed-point
//! scales each Rust function expects. On error (domain violation, overflow,
//! unknown enum code) we return NaN — the JS side treats NaN as "fall back
//! to cached whitepaper constant".
//!
//! This crate links against `std` so the `wasm32-unknown-unknown` build gets
//! an allocator and panic handler. solmath-core itself remains `no_std`.

use solmath_core::{
    barrier_hit_probability, barrier_option, black_scholes_price, compute_il, implied_vol,
    norm_cdf_poly, norm_pdf, BarrierType, SCALE,
};

use halcyon_quote::autocall_v2::{
    solve_fair_coupon_markov_richardson_gated, AutocallParams, AUTOCALL_LOG_6, KNOCK_IN_LOG_6,
};
use halcyon_quote::autocall_v2_e11::{live_quote_uses_e11, solve_fair_coupon_e11_cached};
use halcyon_quote::insurance::european_nig::nig_european_il_premium;
use halcyon_quote::k12_correction::k12_correction_lookup;
use halcyon_quote::worst_of_c1_fast::spy_qqq_iwm_c1_config;
use halcyon_quote::worst_of_c1_filter::quote_c1_filter;
use halcyon_quote::worst_of_factored::FactoredWorstOfModel;

// ============================================================================
// Boundary scale helpers
// ============================================================================

const SCALE_12_F: f64 = 1_000_000_000_000.0; // solmath_core 1e12
const SCALE_6_F:  f64 = 1_000_000.0;         // halcyon_quote pricers 1e6

fn to_fp12_u(x: f64) -> Option<u128> {
    if !x.is_finite() || x < 0.0 { return None; }
    let scaled = x * SCALE_12_F;
    if scaled > (i128::MAX as f64) { return None; }
    Some(scaled as u128)
}
fn to_fp12_i(x: f64) -> Option<i128> {
    if !x.is_finite() { return None; }
    let scaled = x * SCALE_12_F;
    if scaled.abs() > (i128::MAX as f64) { return None; }
    Some(scaled as i128)
}
fn to_fp6_i(x: f64) -> Option<i64> {
    if !x.is_finite() { return None; }
    let scaled = x * SCALE_6_F;
    if scaled.abs() > (i64::MAX as f64) { return None; }
    Some(scaled as i64)
}

// ============================================================================
// Primitives: solmath-core
// ============================================================================

#[no_mangle]
pub extern "C" fn sm_bs_call(s: f64, k: f64, r: f64, sigma: f64, t: f64) -> f64 {
    let (Some(sf), Some(kf), Some(rf), Some(sf2), Some(tf)) =
        (to_fp12_u(s), to_fp12_u(k), to_fp12_u(r), to_fp12_u(sigma), to_fp12_u(t))
    else { return f64::NAN; };
    match black_scholes_price(sf, kf, rf, sf2, tf) {
        Ok((call, _)) => call as f64 / SCALE_12_F,
        Err(_) => f64::NAN,
    }
}

#[no_mangle]
pub extern "C" fn sm_bs_put(s: f64, k: f64, r: f64, sigma: f64, t: f64) -> f64 {
    let (Some(sf), Some(kf), Some(rf), Some(sf2), Some(tf)) =
        (to_fp12_u(s), to_fp12_u(k), to_fp12_u(r), to_fp12_u(sigma), to_fp12_u(t))
    else { return f64::NAN; };
    match black_scholes_price(sf, kf, rf, sf2, tf) {
        Ok((_, put)) => put as f64 / SCALE_12_F,
        Err(_) => f64::NAN,
    }
}

#[no_mangle]
pub extern "C" fn sm_implied_vol(market_price: f64, s: f64, k: f64, r: f64, t: f64) -> f64 {
    let (Some(mpf), Some(sf), Some(kf), Some(rf), Some(tf)) =
        (to_fp12_u(market_price), to_fp12_u(s), to_fp12_u(k), to_fp12_u(r), to_fp12_u(t))
    else { return f64::NAN; };
    match implied_vol(mpf, sf, kf, rf, tf) {
        Ok(sigma) => sigma as f64 / SCALE_12_F,
        Err(_) => f64::NAN,
    }
}

#[no_mangle]
pub extern "C" fn sm_norm_cdf(x: f64) -> f64 {
    let Some(xf) = to_fp12_i(x) else { return f64::NAN; };
    match norm_cdf_poly(xf) { Ok(c) => c as f64 / SCALE_12_F, Err(_) => f64::NAN }
}

#[no_mangle]
pub extern "C" fn sm_norm_pdf(x: f64) -> f64 {
    let Some(xf) = to_fp12_i(x) else { return f64::NAN; };
    match norm_pdf(xf) { Ok(p) => p as f64 / SCALE_12_F, Err(_) => f64::NAN }
}

#[no_mangle]
pub extern "C" fn sm_erf(x: f64) -> f64 {
    let scaled = x * core::f64::consts::SQRT_2;
    let Some(xf) = to_fp12_i(scaled) else { return f64::NAN; };
    match norm_cdf_poly(xf) { Ok(c) => 2.0 * (c as f64 / SCALE_12_F) - 1.0, Err(_) => f64::NAN }
}

#[no_mangle]
pub extern "C" fn sm_compute_il(w: f64, x: f64) -> f64 {
    let (Some(wf), Some(xf)) = (to_fp12_u(w), to_fp12_u(x)) else { return f64::NAN; };
    match compute_il(wf, xf) { Ok(il) => il as f64 / SCALE_12_F, Err(_) => f64::NAN }
}

#[no_mangle]
pub extern "C" fn sm_barrier_hit_prob(spot: f64, barrier: f64, sigma: f64, t: f64, is_upper: i32) -> f64 {
    let (Some(sf), Some(bf), Some(sgf), Some(tf)) =
        (to_fp12_u(spot), to_fp12_u(barrier), to_fp12_u(sigma), to_fp12_u(t))
    else { return f64::NAN; };
    match barrier_hit_probability(sf, bf, sgf, tf, is_upper != 0) {
        Ok(p) => p as f64 / SCALE_12_F,
        Err(_) => f64::NAN,
    }
}

#[no_mangle]
pub extern "C" fn sm_barrier_option(
    s: f64, k: f64, h: f64, r: f64, sigma: f64, t: f64, is_call: i32, barrier_kind: i32,
) -> f64 {
    let (Some(sf), Some(kf), Some(hf), Some(rf), Some(sgf), Some(tf)) =
        (to_fp12_u(s), to_fp12_u(k), to_fp12_u(h), to_fp12_u(r), to_fp12_u(sigma), to_fp12_u(t))
    else { return f64::NAN; };
    let bt = match barrier_kind {
        0 => BarrierType::DownAndOut, 1 => BarrierType::DownAndIn,
        2 => BarrierType::UpAndOut,   3 => BarrierType::UpAndIn,
        _ => return f64::NAN,
    };
    match barrier_option(sf, kf, hf, rf, sgf, tf, is_call != 0, bt) {
        Ok(r) => r.price as f64 / SCALE_12_F,
        Err(_) => f64::NAN,
    }
}

#[no_mangle]
pub extern "C" fn sm_scale() -> f64 { SCALE as f64 }

// ============================================================================
// Product pricer: IL Protection
// ============================================================================
//
// The real halcyon-quote IL pricer. NIG European premium via 5-point
// Gauss-Legendre quadrature over 4 payoff regions (i64/SCALE_6).
// Production params per il_protection_math_stack.md §3, §7:
//   days=30, deductible=0.01, cap=0.07, alpha=3.14, beta=1.21
// Caller applies the ×1.10 underwriting load and quote-share separately.

/// IL fair premium as fraction of insured value.
/// Returns NaN on domain error.
#[no_mangle]
pub extern "C" fn sm_il_fair_premium(
    sigma_ann: f64, days: u32, deductible: f64, cap: f64, alpha: f64, beta: f64,
) -> f64 {
    let (Some(sigma6), Some(ded6), Some(cap6), Some(a6), Some(b6)) =
        (to_fp6_i(sigma_ann), to_fp6_i(deductible), to_fp6_i(cap), to_fp6_i(alpha), to_fp6_i(beta))
    else { return f64::NAN; };
    match nig_european_il_premium(sigma6, days, ded6, cap6, a6, b6) {
        Ok(prem6) => prem6 as f64 / SCALE_6_F,
        Err(_) => f64::NAN,
    }
}

// ============================================================================
// Product pricer: SOL Autocall
// ============================================================================
//
// The real halcyon-quote autocall pricer. POD-DEIM live operator pricer (E11)
// when sigma is in the [0.50, 2.50] training band, gated Richardson CTMC
// (N1=10, N2=15) fallback otherwise.
//
// Production constants per sol_autocall_math_stack.md §14:
//   alpha_6 = 13_040_000 (13.04), beta_6 = 1_520_000 (1.52)
//   reference_step_days = 2
//   AutocallParams { n_obs: 8, knock_in_log_6: ln(0.70), autocall_log_6: ln(1.025),
//                    no_autocall_first_n_obs: 1 }  // day-2 lockout
//
// First call triggers POD-DEIM offline training (SVD on snapshot matrix,
// DEIM greedy selection, atom pre-projection). Subsequent calls hit the
// cache keyed on (alpha, beta, step_days, contract).

const SOL_ALPHA_6: i64 = 13_040_000;
const SOL_BETA_6:  i64 = 1_520_000;
const SOL_STEP_DAYS: i64 = 2;

fn sol_contract() -> AutocallParams {
    AutocallParams {
        n_obs: 8,
        knock_in_log_6: KNOCK_IN_LOG_6,
        autocall_log_6: AUTOCALL_LOG_6,
        no_autocall_first_n_obs: 1, // lockout on obs 1 per math stack §14
    }
}

/// SOL autocall fair coupon per observation (decimal, e.g. 0.0175 = 175 bps/obs).
/// Uses POD-DEIM E11 when sigma ∈ [0.50, 2.50], Richardson CTMC fallback otherwise.
/// Returns NaN on any solver error.
#[no_mangle]
pub extern "C" fn sm_sol_fair_coupon(sigma_ann: f64) -> f64 {
    if !sigma_ann.is_finite() || sigma_ann <= 0.0 { return f64::NAN; }
    let Some(sigma6) = to_fp6_i(sigma_ann) else { return f64::NAN; };
    let contract = sol_contract();

    // Primary: POD-DEIM E11 (gated on sigma ∈ [0.50, 2.50] and n_obs == 8)
    if live_quote_uses_e11(sigma_ann, &contract) {
        if let Ok(r) = solve_fair_coupon_e11_cached(sigma6, SOL_ALPHA_6, SOL_BETA_6, SOL_STEP_DAYS, &contract) {
            return r.fair_coupon as f64 / SCALE_12_F; // bps at SCALE_6 → decimal
        }
    }

    // Fallback: gated Richardson CTMC with N1=10, N2=15
    use halcyon_quote::autocall_v2::NigParams6;
    let nig = match NigParams6::from_vol_with_step_days(sigma6, SOL_ALPHA_6, SOL_BETA_6, SOL_STEP_DAYS) {
        Ok(p) => p,
        Err(_) => return f64::NAN,
    };
    match solve_fair_coupon_markov_richardson_gated(&nig, 10, 15, &contract) {
        Ok(gated) => gated.result.fair_coupon as f64 / SCALE_12_F,
        Err(_) => f64::NAN,
    }
}

/// Returns which engine priced the most-recent call: 2 = E11 POD-DEIM in-band,
/// 1 = Richardson fallback, 0 = domain error.  Informational only.
///
/// Implementation detail: re-runs the gate check (cheap, no pricing work).
#[no_mangle]
pub extern "C" fn sm_sol_pricing_engine(sigma_ann: f64) -> i32 {
    if !sigma_ann.is_finite() || sigma_ann <= 0.0 { return 0; }
    let contract = sol_contract();
    if live_quote_uses_e11(sigma_ann, &contract) { 2 } else { 1 }
}

// ============================================================================
// Worst-of-3 (SPY/QQQ/IWM) 18-month autocallable — K=12 on-chain pricer
// ============================================================================

/// Worst-of-3 fair coupon (bps per quarterly observation) under the shipped
/// K=12 projected c1 filter + K=12 correction table.
///
/// This is the EXACT on-chain pricing path: `quote_c1_filter(K=12)` +
/// `k12_correction_lookup(σ)` — the same integer math the Solana program
/// runs. Callers should compose quote_share (0.60) and issuer margin (100 bps)
/// separately for display.
///
/// sigma_ann must be in [0.08, 0.80]; NaN returned outside that range (the
/// K=12 + daily-KI tables are calibrated on this σ envelope).
///
/// Future cos3d swap: once `daily_ki_correction` replaces the MC-absorbed
/// portion of `k12_correction`, this function grows a second lookup;
/// the signature and return units stay the same.
#[no_mangle]
pub extern "C" fn sm_worst_of_k12_coupon_bps(sigma_ann: f64) -> f64 {
    if !sigma_ann.is_finite() || sigma_ann < 0.08 || sigma_ann > 0.80 {
        return f64::NAN;
    }
    let cfg = spy_qqq_iwm_c1_config();
    let model = FactoredWorstOfModel::spy_qqq_iwm_current();
    let Some(sigma_s6) = to_fp6_i(sigma_ann) else { return f64::NAN; };

    let drifts = match model.risk_neutral_step_drifts(sigma_ann, 63) {
        Ok(d) => d,
        Err(_) => return f64::NAN,
    };
    let drift_diffs = [
        ((drifts[1] - drifts[0]) * SCALE_6_F).round() as i64,
        ((drifts[2] - drifts[0]) * SCALE_6_F).round() as i64,
    ];
    let drift_shift_63 = ((cfg.loadings[0] as f64 * drifts[0])
        + (cfg.loadings[1] as f64 * drifts[1])
        + (cfg.loadings[2] as f64 * drifts[2]))
        .round() as i64;

    let q = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, 12);
    q.fair_coupon_bps + (k12_correction_lookup(sigma_s6) as f64) / 1_000_000.0
}

/// Worst-of-3 knock-in rate (probability of any obs-date KI trigger over the
/// 18-month tenor) at σ, via the same K=12 filter pass.  Returned as a
/// decimal probability in [0, 1] or NaN on domain error.
#[no_mangle]
pub extern "C" fn sm_worst_of_k12_knock_in_rate(sigma_ann: f64) -> f64 {
    if !sigma_ann.is_finite() || sigma_ann < 0.08 || sigma_ann > 0.80 {
        return f64::NAN;
    }
    let cfg = spy_qqq_iwm_c1_config();
    let model = FactoredWorstOfModel::spy_qqq_iwm_current();
    let Some(sigma_s6) = to_fp6_i(sigma_ann) else { return f64::NAN; };

    let drifts = match model.risk_neutral_step_drifts(sigma_ann, 63) {
        Ok(d) => d,
        Err(_) => return f64::NAN,
    };
    let drift_diffs = [
        ((drifts[1] - drifts[0]) * SCALE_6_F).round() as i64,
        ((drifts[2] - drifts[0]) * SCALE_6_F).round() as i64,
    ];
    let drift_shift_63 = ((cfg.loadings[0] as f64 * drifts[0])
        + (cfg.loadings[1] as f64 * drifts[1])
        + (cfg.loadings[2] as f64 * drifts[2]))
        .round() as i64;

    let q = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, 12);
    q.knock_in_rate
}

/// Worst-of-3 autocall rate (probability that any quarterly observation
/// triggers early redemption) at σ, via the same K=12 filter pass.
#[no_mangle]
pub extern "C" fn sm_worst_of_k12_autocall_rate(sigma_ann: f64) -> f64 {
    if !sigma_ann.is_finite() || sigma_ann < 0.08 || sigma_ann > 0.80 {
        return f64::NAN;
    }
    let cfg = spy_qqq_iwm_c1_config();
    let model = FactoredWorstOfModel::spy_qqq_iwm_current();
    let Some(sigma_s6) = to_fp6_i(sigma_ann) else { return f64::NAN; };

    let drifts = match model.risk_neutral_step_drifts(sigma_ann, 63) {
        Ok(d) => d,
        Err(_) => return f64::NAN,
    };
    let drift_diffs = [
        ((drifts[1] - drifts[0]) * SCALE_6_F).round() as i64,
        ((drifts[2] - drifts[0]) * SCALE_6_F).round() as i64,
    ];
    let drift_shift_63 = ((cfg.loadings[0] as f64 * drifts[0])
        + (cfg.loadings[1] as f64 * drifts[1])
        + (cfg.loadings[2] as f64 * drifts[2]))
        .round() as i64;

    let q = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, 12);
    q.autocall_rate
}
