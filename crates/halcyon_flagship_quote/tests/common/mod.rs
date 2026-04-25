use halcyon_flagship_quote::midlife_pricer::{MidlifeInputs, MidlifeNav};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const SCHEMA_VERSION: u32 = 1;
pub const MONTHLY_SCHEDULE: [i64; 18] = [
    21, 42, 63, 84, 105, 126, 147, 168, 189, 210, 231, 252, 273, 294, 315, 336, 357, 378,
];
pub const QUARTERLY_SCHEDULE: [i64; 6] = [63, 126, 189, 252, 315, 378];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MidlifeFixture {
    pub label: String,
    pub inputs: MidlifeInputs,
    pub expected_nav_s6: i64,
    pub expected_ki_level_usd_s6: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MidlifeFixtureFile {
    pub schema_version: u32,
    pub reference_fn: String,
    pub quadrature: String,
    pub vectors: Vec<MidlifeFixture>,
}

pub fn fixtures_path() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests");
    p.push("fixtures");
    p.push("midlife_nav_vectors.json");
    p
}

pub fn base_inputs() -> MidlifeInputs {
    MidlifeInputs {
        current_spy_s6: 100_000_000,
        current_qqq_s6: 100_000_000,
        current_iwm_s6: 100_000_000,
        sigma_common_s6: 180_000,
        entry_spy_s6: 100_000_000,
        entry_qqq_s6: 100_000_000,
        entry_iwm_s6: 100_000_000,
        beta_spy_s12: 900_000_000_000,
        beta_qqq_s12: 400_000_000_000,
        alpha_s12: 50_000_000,
        regression_residual_vol_s6: 220_000,
        monthly_coupon_schedule: MONTHLY_SCHEDULE,
        quarterly_autocall_schedule: QUARTERLY_SCHEDULE,
        next_coupon_index: 0,
        next_autocall_index: 0,
        offered_coupon_bps_s6: 500_000_000,
        coupon_barrier_bps: 10_000,
        autocall_barrier_bps: 10_000,
        ki_barrier_bps: 8_000,
        ki_latched: false,
        missed_coupon_observations: 0,
        coupons_paid_usdc: 0,
        notional_usdc: 100_000_000,
        now_trading_day: 0,
    }
}

fn next_autocall_index(next_coupon_index: u8) -> u8 {
    const QUARTERLY_COUPON_INDICES: [u8; 6] = [2, 5, 8, 11, 14, 17];
    QUARTERLY_COUPON_INDICES
        .iter()
        .position(|&idx| idx >= next_coupon_index)
        .unwrap_or(QUARTERLY_COUPON_INDICES.len()) as u8
}

pub fn inputs_for_state(
    current_ratios_s6: [i64; 3],
    sigma_common_s6: i64,
    next_coupon_index: u8,
    ki_latched: bool,
    missed_coupon_observations: u8,
    coupons_paid_usdc: u64,
    now_trading_day: u16,
) -> MidlifeInputs {
    let mut inputs = base_inputs();
    inputs.current_spy_s6 = current_ratios_s6[0] * 100;
    inputs.current_qqq_s6 = current_ratios_s6[1] * 100;
    inputs.current_iwm_s6 = current_ratios_s6[2] * 100;
    inputs.sigma_common_s6 = sigma_common_s6;
    inputs.next_coupon_index = next_coupon_index;
    inputs.next_autocall_index = next_autocall_index(next_coupon_index);
    inputs.ki_latched = ki_latched;
    inputs.missed_coupon_observations = missed_coupon_observations;
    inputs.coupons_paid_usdc = coupons_paid_usdc;
    inputs.now_trading_day = now_trading_day;
    inputs
}

pub fn final_interval_start(next_coupon_index: u8) -> u16 {
    if next_coupon_index == 0 {
        0
    } else {
        MONTHLY_SCHEDULE[next_coupon_index as usize - 1] as u16
    }
}

pub fn edge_day_before_maturity() -> u16 {
    (MONTHLY_SCHEDULE[17] - 1) as u16
}

pub fn snapshot_fixture(label: String, inputs: MidlifeInputs, nav: MidlifeNav) -> MidlifeFixture {
    MidlifeFixture {
        label,
        inputs,
        expected_nav_s6: nav.nav_s6,
        expected_ki_level_usd_s6: nav.ki_level_usd_s6,
    }
}
