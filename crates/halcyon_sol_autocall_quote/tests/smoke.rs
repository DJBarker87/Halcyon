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
            let close = if day == 2 || day == 4 { 1.03 } else { 1.0 };
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

    let first_obs = replay
        .steps
        .iter()
        .find(|step| step.day == 2)
        .expect("day-2 observation step");
    assert!(first_obs.observation_day);
    assert!(!first_obs.autocalled);
    assert!(first_obs.coupon_paid > 0.0);

    let second_obs = replay
        .steps
        .iter()
        .find(|step| step.day == 4)
        .expect("day-4 observation step");
    assert!(second_obs.observation_day);
    assert!(second_obs.autocalled);

    assert!(replay.autocalled);
    assert!(!replay.knock_in_triggered);
    assert!(replay.buyer_total_return > 0.0);
}
