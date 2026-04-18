use halcyon_quote::autocall_hedged::{
    price_hedged_autocall, AutocallTerms, CouponVaultMode, HedgeFundingMode, HedgeMode,
    HedgePolicy, PathPoint, PricingModel,
};
use halcyon_quote::insurance::european_nig::nig_european_il_premium;
use halcyon_quote::insurance::settlement::compute_settlement_from_prices;

const SCALE_6_F64: f64 = 1_000_000.0;
const SCALE_12_U64: u64 = 1_000_000_000_000;
const SCALE_12_U128: u128 = 1_000_000_000_000;

fn s6(value: f64) -> i64 {
    (value * SCALE_6_F64).round() as i64
}

#[test]
fn il_product_smoke() {
    let premium_frac =
        nig_european_il_premium(s6(1.0), 30, s6(0.01), s6(0.07), s6(3.1401), s6(1.2139))
            .expect("nig premium") as f64
            / SCALE_6_F64;
    assert!(premium_frac > 0.0);

    let (terminal_il, payout_raw) = compute_settlement_from_prices(
        SCALE_12_U64 / 2,
        300 * SCALE_12_U128,
        SCALE_12_U128,
        150 * SCALE_12_U128,
        SCALE_12_U128,
        10_000_000_000,
        (0.01 * SCALE_12_U64 as f64).round() as u64,
        (0.07 * SCALE_12_U64 as f64).round() as u64,
    )
    .expect("settlement");

    assert!(terminal_il > 0);
    assert_eq!(payout_raw, 600_000_000);
}

fn default_policy() -> HedgePolicy {
    HedgePolicy {
        hedge_mode: HedgeMode::DeltaDaily,
        initial_hedge_fraction: 0.25,
        delta_clip: 0.75,
        rebalance_band: 0.10,
        min_trade_notional_pct: 0.01,
        max_trade_notional_pct: 1.0,
        slippage_bps: 10.0,
        slippage_coeff: 25.0,
        liquidity_proxy_usdc: 250_000.0,
        slippage_stress_multiplier: 1.0,
        keeper_bounty_usdc: 0.10,
        cooldown_days: 0,
        max_rebalances_per_day: 8,
        force_observation_review: true,
        allow_intraperiod_checks: true,
        hedge_funding_mode: HedgeFundingMode::SeparateHedgeSleeve,
        coupon_vault_mode: CouponVaultMode::SeparateCouponVault,
        ..HedgePolicy::default()
    }
}

#[test]
fn sol_autocall_product_smoke() {
    let priced = price_hedged_autocall(&AutocallTerms::current_v1(1.0), &PricingModel::default())
        .expect("price current_v1");
    assert!(priced.pricing.fair_coupon_per_observation > 0.0);

    let path = (0..=16)
        .map(|day| {
            let close = if day == 2 { 1.03 } else { 1.0 };
            PathPoint {
                day,
                close,
                low: close,
            }
        })
        .collect::<Vec<_>>();

    let replay = priced
        .simulate_path(&default_policy(), &path)
        .expect("replay current_v1");

    assert!(replay.autocalled);
    assert!(!replay.knock_in_triggered);
    assert!(replay.buyer_total_return > 0.0);
}
