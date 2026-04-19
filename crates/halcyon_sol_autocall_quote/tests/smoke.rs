use halcyon_sol_autocall_quote::autocall_hedged::{
    price_hedged_autocall, AutocallTerms, CouponVaultMode, HedgeFundingMode, HedgeMode,
    HedgePolicy, PathPoint, PricingModel,
};

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
