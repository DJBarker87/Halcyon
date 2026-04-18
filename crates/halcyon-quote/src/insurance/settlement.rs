//! IL settlement computation.
//!
//! Computes actual IL from entry and current prices, applies deductible and cap,
//! determines USDC payout. Used by cancel, reset, expire, and trigger_cap.

use solmath_core::{fp_div, fp_mul, pow_fixed, SolMathError, SCALE};

/// Compute settlement payout for a 2-token insured position.
///
/// IL(x) = w·x + (1-w) - x^w
///
/// where x = current_price / entry_price (the price ratio).
///
/// Payout = min(max(IL - d, 0), c - d) × position_value
///
/// # Arguments
/// * `weight` - Weight of the volatile token (at SCALE)
/// * `entry_price_ratio` - P_current / P_entry at opt-in (at SCALE; always SCALE at entry)
/// * `current_price_ratio` - P_current / P_entry now (at SCALE)
/// * `position_value_usdc` - Position value in USDC (raw, 6 decimals)
/// * `deductible` - Deductible fraction (at SCALE)
/// * `cap` - Cap fraction (at SCALE)
///
/// # Returns
/// `(il_fraction, payout_usdc)` — IL as fraction at SCALE, and USDC payout (raw, 6 decimals).
/// If IL is below deductible, payout is 0. IL is always non-negative (IL formula has IL≥0 for any x).
pub fn compute_settlement(
    weight: u64,
    entry_price_ratio: u128,
    current_price_ratio: u128,
    position_value_usdc: u64,
    deductible: u64,
    cap: u64,
) -> Result<(u128, u64), SolMathError> {
    if deductible >= cap {
        return Err(SolMathError::DomainError);
    }

    // x = current / entry (relative price move since opt-in)
    let x = if entry_price_ratio == 0 {
        return Err(SolMathError::DivisionByZero);
    } else if entry_price_ratio == SCALE {
        current_price_ratio
    } else {
        // x = current_price_ratio / entry_price_ratio (but both are already ratios)
        // In practice, entry_price_ratio is SCALE at opt-in, but after reset it
        // could be different if we re-anchor.
        (current_price_ratio * SCALE) / entry_price_ratio
    };

    // IL(x) = w·x + (1-w) - x^w
    let w = weight as u128;
    let w_stable = SCALE - w;

    // x^w (pow_fixed works at SCALE)
    let x_pow_w = pow_fixed(x, w)?;

    // w·x at SCALE
    let w_x = fp_mul(w, x)?;

    // IL = w·x + (1-w) - x^w
    // This can be negative for very small x, but the insurance formula clamps at 0.
    let hold = w_x + w_stable; // w·x + (1-w), at SCALE
    let il_signed = hold as i128 - x_pow_w as i128;
    let il = if il_signed < 0 {
        0u128
    } else {
        il_signed as u128
    };

    // Capped payout fraction: min(max(IL - d, 0), c - d)
    let d = deductible as u128;
    let c = cap as u128;
    let payout_fraction = if il <= d {
        0u128
    } else {
        let above_d = il - d;
        let spread = c - d;
        above_d.min(spread)
    };

    // USDC payout = payout_fraction × position_value / SCALE
    // Use u128 intermediate to avoid overflow
    let payout_usdc = (payout_fraction * (position_value_usdc as u128)) / SCALE;

    Ok((il, payout_usdc as u64))
}

/// Compute settlement from raw oracle prices (convenience wrapper).
///
/// Derives the price ratio x from current and entry prices for both tokens,
/// then delegates to [`compute_settlement`].
///
/// # Arguments
/// * `weight` — Weight of the volatile token (token A, index 0) at SCALE.
/// * `price_a_now`, `price_b_now` — Current oracle prices at SCALE.
/// * `price_a_entry`, `price_b_entry` — Entry oracle prices at SCALE.
/// * `position_value_usdc` — Position value in USDC (raw, 6 decimals).
/// * `deductible`, `cap` — At SCALE.
pub fn compute_settlement_from_prices(
    weight: u64,
    price_a_now: u128,
    price_b_now: u128,
    price_a_entry: u128,
    price_b_entry: u128,
    position_value_usdc: u64,
    deductible: u64,
    cap: u64,
) -> Result<(u128, u64), SolMathError> {
    if price_a_now == 0 || price_b_now == 0 || price_a_entry == 0 || price_b_entry == 0 {
        return Err(SolMathError::DivisionByZero);
    }

    // x = (P_a_now × P_b_entry) / (P_a_entry × P_b_now)
    let num = fp_mul(price_a_now, price_b_entry)?;
    let den = fp_mul(price_a_entry, price_b_now)?;
    if den == 0 {
        return Err(SolMathError::DivisionByZero);
    }
    let x = fp_div(num, den)?;

    compute_settlement(weight, SCALE, x, position_value_usdc, deductible, cap)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE_U64: u64 = SCALE as u64;

    #[test]
    fn no_price_change_no_il() {
        let (il, payout) = compute_settlement(
            SCALE_U64 / 2,   // 50% weight
            SCALE,           // entry = 1.0
            SCALE,           // current = 1.0 (no change)
            10_000_000_000,  // $10,000 USDC
            20_000_000_000,  // 2% deductible
            250_000_000_000, // 25% cap
        )
        .unwrap();

        assert_eq!(il, 0, "no price change → no IL");
        assert_eq!(payout, 0, "no IL → no payout");
    }

    #[test]
    fn price_doubles_50_50() {
        // x = 2.0: IL = 0.5*2 + 0.5 - 2^0.5 = 1.5 - 1.4142... ≈ 0.0858
        let x = 2 * SCALE;
        let (il, payout) = compute_settlement(
            SCALE_U64 / 2,
            SCALE,
            x,
            10_000_000_000,  // $10,000
            20_000_000_000,  // 2%
            250_000_000_000, // 25%
        )
        .unwrap();

        // IL ≈ 8.58%
        let il_pct = il * 100 / SCALE;
        assert!(
            il_pct >= 8 && il_pct <= 9,
            "IL should be ~8.58%, got {}%",
            il_pct
        );

        // Payout = (0.0858 - 0.02) × $10,000 = ~$658
        assert!(
            payout > 600_000_000 && payout < 700_000_000,
            "payout={payout}"
        );
    }

    #[test]
    fn il_below_deductible_no_payout() {
        // Small price move: x = 1.05 → IL ≈ 0.06% (below 2% deductible)
        let x = SCALE + SCALE / 20; // 1.05
        let (il, payout) = compute_settlement(
            SCALE_U64 / 2,
            SCALE,
            x,
            10_000_000_000,
            20_000_000_000,
            250_000_000_000,
        )
        .unwrap();

        assert!(il < 20_000_000_000, "IL should be below deductible");
        assert_eq!(payout, 0, "below deductible → zero payout");
    }

    #[test]
    fn il_at_cap_clamped() {
        // x = 10.0: IL = 0.5*10 + 0.5 - 10^0.5 = 5.5 - 3.162 = 2.338 (233.8%)
        // But capped at (25% - 2%) = 23%
        let x = 10 * SCALE;
        let (il, payout) = compute_settlement(
            SCALE_U64 / 2,
            SCALE,
            x,
            10_000_000_000,
            20_000_000_000,
            250_000_000_000,
        )
        .unwrap();

        // IL is way above cap
        assert!(il > 250_000_000_000);
        // Payout capped at (25% - 2%) × $10,000 = $2,300
        assert_eq!(payout, 2_300_000_000, "payout should be capped at $2,300");
    }

    #[test]
    fn deductible_gte_cap_error() {
        let r = compute_settlement(
            SCALE_U64 / 2,
            SCALE,
            SCALE,
            10_000_000_000,
            250_000_000_000,
            250_000_000_000,
        );
        assert!(r.is_err());
    }

    #[test]
    fn asymmetric_weight_70_30() {
        // 70/30 pool, price drops to 0.5: IL = 0.7*0.5 + 0.3 - 0.5^0.7
        let x = SCALE / 2; // 0.5
        let (il, _) = compute_settlement(
            700_000_000_000, // 70%
            SCALE,
            x,
            10_000_000_000,
            20_000_000_000,
            250_000_000_000,
        )
        .unwrap();

        // IL should be positive
        assert!(il > 0, "IL should be positive for 70/30 at x=0.5");
    }

    // ── compute_settlement_from_prices ──

    const SOL_150: u128 = 150 * SCALE;
    const USDC_1: u128 = SCALE;

    #[test]
    fn from_prices_no_change() {
        let (il, payout) = compute_settlement_from_prices(
            SCALE_U64 / 2,
            SOL_150,
            USDC_1,
            SOL_150,
            USDC_1,
            10_000_000_000,
            20_000_000_000,
            250_000_000_000,
        )
        .unwrap();
        assert_eq!(il, 0);
        assert_eq!(payout, 0);
    }

    #[test]
    fn from_prices_sol_doubles() {
        let (il, payout) = compute_settlement_from_prices(
            SCALE_U64 / 2,
            2 * SOL_150,
            USDC_1,
            SOL_150,
            USDC_1,
            10_000_000_000,
            20_000_000_000,
            250_000_000_000,
        )
        .unwrap();
        let il_pct = il * 100 / SCALE;
        assert!(il_pct >= 8 && il_pct <= 9, "IL ~8.58%, got {}%", il_pct);
        assert!(payout > 600_000_000 && payout < 700_000_000);
    }

    #[test]
    fn from_prices_both_move() {
        // SOL $150→$300, USDC $1→$1.02.
        // x = (300 × 1) / (150 × 1.02) = 300/153 ≈ 1.9608
        // Very close to x=2, IL should be close to 8.58%.
        let usdc_now = SCALE + SCALE / 50; // 1.02
        let (il, _) = compute_settlement_from_prices(
            SCALE_U64 / 2,
            2 * SOL_150,
            usdc_now,
            SOL_150,
            USDC_1,
            10_000_000_000,
            20_000_000_000,
            250_000_000_000,
        )
        .unwrap();
        // x ≈ 1.96 → IL slightly less than x=2
        let il_pct = il * 100 / SCALE;
        assert!(il_pct >= 7 && il_pct <= 9, "IL ~8%, got {}%", il_pct);
    }

    #[test]
    fn from_prices_zero_rejected() {
        assert!(compute_settlement_from_prices(
            SCALE_U64 / 2,
            0,
            USDC_1,
            SOL_150,
            USDC_1,
            10_000_000_000,
            20_000_000_000,
            250_000_000_000,
        )
        .is_err());
    }

    #[test]
    fn from_prices_matches_ratio_version() {
        // Verify from_prices gives same result as manual ratio computation.
        let (il_direct, pay_direct) = compute_settlement(
            SCALE_U64 / 2,
            SCALE,
            2 * SCALE,
            10_000_000_000,
            20_000_000_000,
            250_000_000_000,
        )
        .unwrap();
        let (il_prices, pay_prices) = compute_settlement_from_prices(
            SCALE_U64 / 2,
            2 * SOL_150,
            USDC_1,
            SOL_150,
            USDC_1,
            10_000_000_000,
            20_000_000_000,
            250_000_000_000,
        )
        .unwrap();

        assert_eq!(il_direct, il_prices, "IL should match");
        assert_eq!(pay_direct, pay_prices, "payout should match");
    }
}
