//! Frozen factor-model skeleton for the SPY/QQQ/IWM worst-of note.
//!
//! This module does not yet contain the final on-chain quadrature engine.
//! It provides the deterministic building blocks that both the offline
//! calibration path and the future on-chain pricer share:
//!
//! - calibrated common-factor loadings
//! - residual covariance in asset and `(u, v)` coordinates
//! - common-factor delta scaling
//! - risk-neutral drift adjustment
//! - barrier half-plane shifts for the spread-plane triangle geometry
//! - exact spread-plane triangle probabilities via `solmath-core`

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use solmath_core::gauss_hermite::{
    GH13_NODES, GH13_WEIGHTS, GH5_NODES, GH5_WEIGHTS, GH7_NODES, GH7_WEIGHTS, GH9_NODES,
    GH9_WEIGHTS, GL5_NODES, GL5_WEIGHTS, GL7_NODES, GL7_WEIGHTS,
};
use solmath_core::nig_weights_table::{nig_importance_weights_9, GH9_NODES_S6};
use solmath_core::worst_of_ki_i64::{cholesky6, ki_moment_i64_gh3, AffineCoord6};
use solmath_core::{
    triangle_probability as triangle_probability_fp,
    triangle_probability_i64 as triangle_probability_i64_fp,
    triangle_probability_phi2 as triangle_probability_phi2_fp,
    triangle_probability_precomputed as triangle_probability_precomputed_fp,
    triangle_probability_with_order as triangle_probability_fp_with_order,
    triangle_region_moment as triangle_region_moment_fp,
    triangle_region_moment_with_order as triangle_region_moment_fp_with_order, worst_of_ki_moment,
    worst_of_ki_moment_with_order, AffineLogCoordinate, HalfPlane as FixedHalfPlane, SolMathError,
    TrianglePre64, TrianglePrecomputed, TriangleRegionMoment as FixedTriangleRegionMoment,
    WorstOfKiMoment, INV_SQRT_PI, PHI2_RESID_QQQ_IWM, PHI2_RESID_SPY_IWM, PHI2_RESID_SPY_QQQ,
    SCALE_I,
};

const SQRT_2: f64 = core::f64::consts::SQRT_2;

/// Precomputed triangle geometry for step_days=63, frozen loadings + residual covariance.
const TRIANGLE_PRE_63: TrianglePrecomputed = TrianglePrecomputed {
    inv_std: [25_468_813_827_413, 16_386_565_592_036, 15_922_933_747_843],
    au: [567_972_444_067, -1_157_158_208_641, 567_972_444_067],
    av: [641_427_106_944, 641_427_106_944, -1_083_703_545_764],
    phi2_neg: [false, true, true],
};

const PHI2_TABLES: [&[[i32; 64]; 64]; 3] = [
    &PHI2_RESID_SPY_QQQ,
    &PHI2_RESID_SPY_IWM,
    &PHI2_RESID_QQQ_IWM,
];

/// Cov(u, w_k)/σ_wk and Cov(v, w_k)/σ_wk for each half-plane, step=63, at SCALE_6.
const COV_PROJ_63: [[i64; 2]; 3] = [[22_475, 41_312], [-35_191, 31_655], [18_982, -48_003]];
const TRIANGLE_PAIR_RHO_63: [i64; 3] = [8_070, -509_610, -864_483];
const TRIANGLE_PAIR_INV_SQRT_1MRHO2_63: [i64; 3] = [1_000_033, 1_162_243, 1_989_410];

const TRIANGLE_PRE64_63: TrianglePre64 = TrianglePre64 {
    au: [567_972, -1_157_159, 567_972],
    av: [641_427, 641_427, -1_083_704],
    inv_std: [25_468_813, 16_386_565, 15_922_933],
    phi2_neg: [false, true, true],
};
const SQRT_2PI: f64 = 2.506_628_274_631_000_2;
const PATH_WEIGHT_CUTOFF: f64 = 1.0e-14;
const STATE_MASS_EPS: f64 = 1.0e-12;
const SPY_LOG_BUCKET: f64 = 0.020;
const SPREAD_LOG_BUCKET: f64 = 0.015;
const MAX_LIVE_STATES: usize = 12_000;
const GL20_NODES_F64: [f64; 20] = [
    -0.993_128_599_185,
    -0.963_971_927_278,
    -0.912_234_428_251,
    -0.839_116_971_822,
    -0.746_331_906_460,
    -0.636_053_680_727,
    -0.510_867_001_951,
    -0.373_706_088_715,
    -0.227_785_851_142,
    -0.076_526_521_133,
    0.076_526_521_133,
    0.227_785_851_142,
    0.373_706_088_715,
    0.510_867_001_951,
    0.636_053_680_727,
    0.746_331_906_460,
    0.839_116_971_822,
    0.912_234_428_251,
    0.963_971_927_278,
    0.993_128_599_185,
];
const GL20_WEIGHTS_F64: [f64; 20] = [
    0.017_614_007_139,
    0.040_601_429_800,
    0.062_672_048_334,
    0.083_276_741_577,
    0.101_930_119_817,
    0.118_194_531_962,
    0.131_688_638_449,
    0.142_096_109_318,
    0.149_172_986_473,
    0.152_753_387_131,
    0.152_753_387_131,
    0.149_172_986_473,
    0.142_096_109_318,
    0.131_688_638_449,
    0.118_194_531_962,
    0.101_930_119_817,
    0.083_276_741_577,
    0.062_672_048_334,
    0.040_601_429_800,
    0.017_614_007_139,
];

fn erf_approx(x: f64) -> f64 {
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    let ax = x.abs();
    let t = 1.0 / (1.0 + 0.327_591_1 * ax);
    let y = 1.0
        - (((((1.061_405_429 * t - 1.453_152_027) * t) + 1.421_413_741) * t - 0.284_496_736) * t
            + 0.254_829_592)
            * t
            * (-ax * ax).exp();
    sign * y
}

fn bessel_k1_f64(z: f64) -> f64 {
    if !z.is_finite() || z <= 0.0 {
        return 0.0;
    }
    if z <= 2.0 {
        let t = z / 3.75;
        let t_sq = t * t;
        let i1_over_z = 0.5
            + t_sq
                * (0.878_905_94
                    + t_sq
                        * (0.514_988_69
                            + t_sq
                                * (0.150_849_34
                                    + t_sq
                                        * (0.026_587_33
                                            + t_sq * (0.003_015_32 + t_sq * 0.000_324_11)))));
        let i1 = z * i1_over_z;
        let u = z / 2.0;
        let u_sq = u * u;
        let q = 1.0
            + u_sq
                * (0.154_431_44
                    + u_sq
                        * (-0.672_785_79
                            + u_sq
                                * (-0.181_568_97
                                    + u_sq
                                        * (-0.019_194_02
                                            + u_sq * (-0.001_104_04 - 0.000_046_86 * u_sq)))));
        return u.ln() * i1 + q / z;
    }

    let t = 2.0 / z;
    let poly = 1.253_314_14
        + t * (0.234_986_19
            + t * (-0.036_556_20
                + t * (0.015_042_68
                    + t * (-0.007_803_53 + t * (0.003_256_14 - 0.000_682_45 * t)))));
    poly * (-z).exp() / z.sqrt()
}

/// One observation-date marginal used by the deterministic quote recursion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ObservationMarginal {
    pub observation_day: u32,
    pub autocall_probability: f64,
    pub coupon_probability: f64,
    pub knock_in_safe_probability: f64,
    pub ki_probability: f64,
    pub ki_worst_indicator_expectation: f64,
    pub knocked_redemption_expectation: f64,
    pub survival_probability: f64,
    pub autocall_first_hit_probability: f64,
    pub first_knock_in_probability: f64,
    pub coupon_annuity_contribution: f64,
    pub autocall_redemption_pv_contribution: f64,
}

/// Deterministic factor-model quote output.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FactoredWorstOfLegDecomposition {
    /// `V0`: redemption / principal leg present value at zero coupon.
    pub redemption_leg_pv: f64,
    /// `U0`: coupon annuity present value, equal to `V1 - V0`.
    pub coupon_annuity_pv: f64,
    /// `1 - V0` in normalized notation, scaled here by product notional.
    pub loss_leg_pv: f64,
    /// Portion of `V0` paid through early autocalls before maturity.
    pub early_autocall_redemption_pv: f64,
    /// Portion of `V0` paid at maturity.
    pub maturity_redemption_pv: f64,
    /// Knocked-in subset of the maturity redemption leg.
    pub maturity_knock_in_redemption_pv: f64,
}

/// Deterministic factor-model quote output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactoredWorstOfQuote {
    pub sigma_common: f64,
    pub fair_coupon_per_observation: f64,
    pub fair_coupon_bps: f64,
    pub quoted_coupon_bps: f64,
    pub leg_decomposition: FactoredWorstOfLegDecomposition,
    pub zero_coupon_pv: f64,
    pub unit_coupon_pv: f64,
    pub unit_coupon_sensitivity: f64,
    pub expected_redemption: f64,
    pub expected_coupon_count: f64,
    pub expected_life_days: f64,
    pub knock_in_rate: f64,
    pub autocall_rate: f64,
    pub approximate_no_autocall_probability: f64,
    pub approximate_no_knock_in_probability: f64,
    pub observation_marginals: [ObservationMarginal; 6],
}

/// Exact continuation survivor means for the 9-node c1 lookup tables.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OnchainV1SurvivorMomentTable {
    pub observation_days: [u32; 6],
    pub expectation_u_safe: [[f64; 9]; 6],
    pub expectation_v_safe: [[f64; 9]; 6],
    pub expectation_u_knocked: [[f64; 9]; 6],
    pub expectation_v_knocked: [[f64; 9]; 6],
    pub common_factor_safe: [f64; 6],
    pub common_factor_knocked: [f64; 6],
}

/// Exact per-node continuation mass diagnostics for the 9-node c1 lookup replay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OnchainV1ReplayDiagnostic {
    pub observation_days: [u32; 6],
    pub safe_node_input_mass: [[f64; 9]; 6],
    pub knocked_node_input_mass: [[f64; 9]; 6],
    pub node_autocall_first_hit_mass: [[f64; 9]; 6],
    pub node_first_knock_in_mass: [[f64; 9]; 6],
    pub safe_node_continue_mass: [[f64; 9]; 6],
    pub knocked_node_continue_mass: [[f64; 9]; 6],
}

/// Partial profile of the deterministic survivor recursion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FactoredWorstOfTraceProfile {
    pub sigma_common: f64,
    pub completed_observations: u8,
    pub terminalized: bool,
    pub live_state_count: u32,
    pub peak_live_state_count: u32,
    pub live_probability_mass: f64,
    pub redemption_leg_pv: f64,
    pub coupon_annuity_pv: f64,
    pub expected_life_days: f64,
    pub knock_in_rate: f64,
    pub autocall_rate: f64,
}

/// Host-only mid-life NAV diagnostics for the flagship autocall.
#[cfg(not(target_os = "solana"))]
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MidlifeReferenceTrace {
    pub nav_per_notional: f64,
    pub remaining_coupon_pv_per_notional: f64,
    pub par_recovery_probability: f64,
}

/// One resumable live state in the exact survivor recursion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FactoredWorstOfCheckpointState {
    pub weight: f64,
    pub logs: [f64; 3],
    pub knocked: bool,
    pub missed_coupons: u8,
}

/// Serializable checkpoint for split-transaction exact recursion.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactoredWorstOfCheckpoint {
    pub sigma_common: f64,
    pub completed_observations: u8,
    pub peak_live_state_count: u32,
    pub live_states: Vec<FactoredWorstOfCheckpointState>,
    pub redemption_leg_pv: f64,
    pub coupon_annuity_pv: f64,
    pub expected_life_days: f64,
    pub observation_survival_probability: [f64; 6],
    pub observation_autocall_first_hit_probability: [f64; 6],
    pub observation_first_knock_in_probability: [f64; 6],
    pub observation_coupon_annuity_contribution: [f64; 6],
    pub observation_autocall_redemption_pv_contribution: [f64; 6],
    pub maturity_redemption_pv: f64,
    pub maturity_knock_in_redemption_pv: f64,
}

/// Diagnostic profile of one exact step kernel before survivor-state branching.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FactoredWorstOfStepKernelProfile {
    pub sigma_common: f64,
    pub step_days: u32,
    pub factor_node_count: u32,
    pub outcome_count: u32,
    pub total_weight: f64,
    pub max_outcome_weight: f64,
    pub min_outcome_weight: f64,
}

#[derive(Debug, Clone, Copy)]
struct WeightedFactorNode {
    value: f64,
    weight: f64,
}

#[derive(Debug, Clone, Copy)]
struct ConditionalFactorNode {
    value: f64,
    weight: f64,
    mean: [f64; 2],
    covariance: [[f64; 2]; 2],
}

#[derive(Debug, Clone, Copy)]
struct StepOutcome {
    weight: f64,
    log_return_increments: [f64; 3],
}

#[derive(Debug, Clone, Copy, Default)]
struct RegionLogMoment {
    probability: f64,
    log_indicator_expectation: [f64; 3],
}

#[derive(Debug, Clone, Copy, Default)]
struct UvRegionMoment {
    probability: f64,
    expectation_u: f64,
    expectation_v: f64,
    expectation_uu: f64,
    expectation_uv: f64,
    expectation_vv: f64,
}

#[derive(Debug, Clone, Copy)]
struct GaussianUvState {
    weight: f64,
    common_factor: f64,
    uv_mean: [f64; 2],
    uv_covariance: [[f64; 2]; 2],
    knocked: bool,
}

impl UvRegionMoment {
    fn from_gaussian(mean: [f64; 2], covariance: [[f64; 2]; 2]) -> Self {
        Self {
            probability: 1.0,
            expectation_u: mean[0],
            expectation_v: mean[1],
            expectation_uu: covariance[0][0] + mean[0] * mean[0],
            expectation_uv: covariance[0][1] + mean[0] * mean[1],
            expectation_vv: covariance[1][1] + mean[1] * mean[1],
        }
    }

    fn subtract(self, other: Self) -> Self {
        Self {
            probability: (self.probability - other.probability).max(0.0),
            expectation_u: self.expectation_u - other.expectation_u,
            expectation_v: self.expectation_v - other.expectation_v,
            expectation_uu: self.expectation_uu - other.expectation_uu,
            expectation_uv: self.expectation_uv - other.expectation_uv,
            expectation_vv: self.expectation_vv - other.expectation_vv,
        }
    }

    fn conditional_distribution(self) -> Option<([f64; 2], [[f64; 2]; 2])> {
        if !self.probability.is_finite() || self.probability <= STATE_MASS_EPS {
            return None;
        }
        let probability = self.probability;
        let mean_u = self.expectation_u / probability;
        let mean_v = self.expectation_v / probability;
        if !mean_u.is_finite() || !mean_v.is_finite() {
            return None;
        }
        let mut var_u = self.expectation_uu / probability - mean_u * mean_u;
        let mut cov_uv = self.expectation_uv / probability - mean_u * mean_v;
        let mut var_v = self.expectation_vv / probability - mean_v * mean_v;
        if !var_u.is_finite() || !cov_uv.is_finite() || !var_v.is_finite() {
            return None;
        }
        // Floor must survive f64 → i64 SCALE_6 conversion: 1e-5 → 10 at S6.
        // Prior floor (1e-12) rounded to 0, causing DegenerateVariance in Cholesky.
        var_u = var_u.max(1.0e-5);
        var_v = var_v.max(1.0e-5);
        let cov_bound = (var_u * var_v).sqrt();
        cov_uv = cov_uv.clamp(-cov_bound, cov_bound);
        Some(([mean_u, mean_v], [[var_u, cov_uv], [cov_uv, var_v]]))
    }
}

#[derive(Debug, Clone, Copy)]
struct LegTrace {
    redemption_leg_pv: f64,
    coupon_annuity_pv: f64,
    expected_life_days: f64,
    knock_in_rate: f64,
    autocall_rate: f64,
    observation_survival_probability: [f64; 6],
    observation_autocall_first_hit_probability: [f64; 6],
    observation_first_knock_in_probability: [f64; 6],
    observation_coupon_annuity_contribution: [f64; 6],
    observation_autocall_redemption_pv_contribution: [f64; 6],
    maturity_redemption_pv: f64,
    maturity_knock_in_redemption_pv: f64,
}

#[derive(Debug, Clone, Copy, Default)]
struct OnchainV1SurvivorMomentCapture {
    expectation_u_safe: [[f64; 9]; 6],
    expectation_v_safe: [[f64; 9]; 6],
    expectation_u_knocked: [[f64; 9]; 6],
    expectation_v_knocked: [[f64; 9]; 6],
    common_factor_safe: [f64; 6],
    common_factor_knocked: [f64; 6],
}

#[derive(Debug, Clone, Copy, Default)]
struct OnchainV1ReplayCapture {
    safe_node_input_mass: [[f64; 9]; 6],
    knocked_node_input_mass: [[f64; 9]; 6],
    node_autocall_first_hit_mass: [[f64; 9]; 6],
    node_first_knock_in_mass: [[f64; 9]; 6],
    safe_node_continue_mass: [[f64; 9]; 6],
    knocked_node_continue_mass: [[f64; 9]; 6],
}

#[derive(Debug, Default, Clone, Copy)]
struct LegTraceAccumulator {
    redemption_leg_pv: f64,
    coupon_annuity_pv: f64,
    expected_life_days: f64,
    observation_survival_probability: [f64; 6],
    observation_autocall_first_hit_probability: [f64; 6],
    observation_first_knock_in_probability: [f64; 6],
    observation_coupon_annuity_contribution: [f64; 6],
    observation_autocall_redemption_pv_contribution: [f64; 6],
    maturity_redemption_pv: f64,
    maturity_knock_in_redemption_pv: f64,
}

#[derive(Debug, Clone, Copy)]
struct LiveState {
    weight: f64,
    logs: [f64; 3],
    knocked: bool,
    missed_coupons: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct StateKey {
    spy_bucket: i32,
    spread_u_bucket: i32,
    spread_v_bucket: i32,
    knocked: bool,
    missed_coupons: u8,
}

#[derive(Debug, Clone, Copy)]
struct BucketAccumulator {
    weight: f64,
    weighted_logs: [f64; 3],
}

#[derive(Debug, Clone, Copy)]
struct TraceBuildResult {
    trace: LegTrace,
    completed_observations: usize,
    terminalized: bool,
    live_state_count: usize,
    peak_live_state_count: usize,
    live_probability_mass: f64,
}

#[derive(Debug, Clone, Copy)]
struct PreMaturityStateProbabilities {
    autocall_probability: f64,
    autocall_coupon_probability: f64,
    safe_survival_probability: f64,
    knocked_survival_probability: f64,
    first_knock_in_probability: f64,
}

/// Three-name worst-of observation schedule for the 18-month shelf.
pub const OBSERVATION_DAYS_18M: [u32; 6] = [63, 126, 189, 252, 315, 378];

/// Errors returned by the factor-model helpers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FactoredWorstOfError {
    InvalidSigmaCommon,
    InvalidStepDays,
    InvalidQuadratureOrder,
    InvalidCheckpoint,
    InvalidShape,
    InvalidCovariance,
    InvalidBarrierShift,
    DegenerateDensity,
    SolMath(SolMathError),
}

impl From<SolMathError> for FactoredWorstOfError {
    fn from(value: SolMathError) -> Self {
        Self::SolMath(value)
    }
}

/// One half-plane of the spread triangle: `a_u * u + a_v * v <= rhs`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BarrierHalfPlane {
    pub a_u: f64,
    pub a_v: f64,
    pub rhs: f64,
}

/// Common-factor NIG shape with live delta scaling.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CommonFactorNig {
    pub alpha: f64,
    pub beta: f64,
    pub gamma: f64,
    pub delta_scale_daily: f64,
    pub delta_scale_annual_trading_day: f64,
}

/// Product shell carried by the frozen factor model.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WorstOfFactoredShell {
    pub tenor_trading_days: u32,
    pub observation_days: [u32; 6],
    pub autocall_barrier: f64,
    pub coupon_barrier: f64,
    pub knock_in_barrier: f64,
    pub quote_share: f64,
    pub issuer_margin_bps: f64,
    pub fair_coupon_floor_bps: u32,
    pub fair_coupon_ceiling_bps: u32,
    pub notional: f64,
}

/// Low-CU quadrature settings for the on-chain v1 approximation path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactoredWorstOfOnchainConfig {
    pub factor_order: u8,
    pub triangle_gl_order: u8,
    pub ki_order: u8,
    pub components_per_class: u8,
}

impl Default for FactoredWorstOfOnchainConfig {
    fn default() -> Self {
        Self {
            factor_order: 13,
            triangle_gl_order: 20,
            ki_order: 13,
            components_per_class: 5,
        }
    }
}

/// Frozen SPY/QQQ/IWM factor skeleton.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FactoredWorstOfModel {
    pub names: [String; 3],
    pub common_factor_loadings: [f64; 3],
    pub uv_factor_slope: [f64; 2],
    pub common_factor: CommonFactorNig,
    pub residual_covariance_daily: [[f64; 3]; 3],
    pub residual_covariance_uv_daily: [[f64; 2]; 2],
    pub autocall_halfplanes: [BarrierHalfPlane; 3],
    pub knock_in_safe_halfplanes: [BarrierHalfPlane; 3],
    pub shell: WorstOfFactoredShell,
}

impl FactoredWorstOfModel {
    fn leg_decomposition_from_trace(&self, trace: LegTrace) -> FactoredWorstOfLegDecomposition {
        let redemption_leg_pv = trace.redemption_leg_pv;
        let coupon_annuity_pv = trace.coupon_annuity_pv.max(0.0);
        let loss_leg_pv = (self.shell.notional - redemption_leg_pv).max(0.0);
        let early_autocall_redemption_pv = trace
            .observation_autocall_redemption_pv_contribution
            .into_iter()
            .sum::<f64>();
        let maturity_redemption_pv = trace.maturity_redemption_pv;
        let maturity_knock_in_redemption_pv = trace.maturity_knock_in_redemption_pv;
        FactoredWorstOfLegDecomposition {
            redemption_leg_pv,
            coupon_annuity_pv,
            loss_leg_pv,
            early_autocall_redemption_pv,
            maturity_redemption_pv,
            maturity_knock_in_redemption_pv,
        }
    }

    fn initial_checkpoint(
        &self,
        sigma_common: f64,
    ) -> Result<FactoredWorstOfCheckpoint, FactoredWorstOfError> {
        if !sigma_common.is_finite() || sigma_common <= 0.0 {
            return Err(FactoredWorstOfError::InvalidSigmaCommon);
        }
        Ok(FactoredWorstOfCheckpoint {
            sigma_common,
            completed_observations: 0,
            peak_live_state_count: 1,
            live_states: vec![FactoredWorstOfCheckpointState {
                weight: 1.0,
                logs: [0.0; 3],
                knocked: false,
                missed_coupons: 0,
            }],
            redemption_leg_pv: 0.0,
            coupon_annuity_pv: 0.0,
            expected_life_days: 0.0,
            observation_survival_probability: [0.0; 6],
            observation_autocall_first_hit_probability: [0.0; 6],
            observation_first_knock_in_probability: [0.0; 6],
            observation_coupon_annuity_contribution: [0.0; 6],
            observation_autocall_redemption_pv_contribution: [0.0; 6],
            maturity_redemption_pv: 0.0,
            maturity_knock_in_redemption_pv: 0.0,
        })
    }

    fn accumulator_from_checkpoint(
        checkpoint: &FactoredWorstOfCheckpoint,
    ) -> Result<LegTraceAccumulator, FactoredWorstOfError> {
        let completed_observations = checkpoint.completed_observations as usize;
        if completed_observations > OBSERVATION_DAYS_18M.len() {
            return Err(FactoredWorstOfError::InvalidCheckpoint);
        }
        if checkpoint.peak_live_state_count < checkpoint.live_states.len() as u32 {
            return Err(FactoredWorstOfError::InvalidCheckpoint);
        }
        for value in [
            checkpoint.redemption_leg_pv,
            checkpoint.coupon_annuity_pv,
            checkpoint.expected_life_days,
            checkpoint.maturity_redemption_pv,
            checkpoint.maturity_knock_in_redemption_pv,
        ] {
            if !value.is_finite() {
                return Err(FactoredWorstOfError::InvalidCheckpoint);
            }
        }
        for array in [
            checkpoint.observation_survival_probability,
            checkpoint.observation_autocall_first_hit_probability,
            checkpoint.observation_first_knock_in_probability,
            checkpoint.observation_coupon_annuity_contribution,
            checkpoint.observation_autocall_redemption_pv_contribution,
        ] {
            if array.iter().any(|value| !value.is_finite()) {
                return Err(FactoredWorstOfError::InvalidCheckpoint);
            }
        }
        Ok(LegTraceAccumulator {
            redemption_leg_pv: checkpoint.redemption_leg_pv,
            coupon_annuity_pv: checkpoint.coupon_annuity_pv,
            expected_life_days: checkpoint.expected_life_days,
            observation_survival_probability: checkpoint.observation_survival_probability,
            observation_autocall_first_hit_probability: checkpoint
                .observation_autocall_first_hit_probability,
            observation_first_knock_in_probability: checkpoint
                .observation_first_knock_in_probability,
            observation_coupon_annuity_contribution: checkpoint
                .observation_coupon_annuity_contribution,
            observation_autocall_redemption_pv_contribution: checkpoint
                .observation_autocall_redemption_pv_contribution,
            maturity_redemption_pv: checkpoint.maturity_redemption_pv,
            maturity_knock_in_redemption_pv: checkpoint.maturity_knock_in_redemption_pv,
        })
    }

    fn live_states_from_checkpoint(
        checkpoint: &FactoredWorstOfCheckpoint,
    ) -> Result<Vec<LiveState>, FactoredWorstOfError> {
        let mut live_states = Vec::with_capacity(checkpoint.live_states.len());
        for state in &checkpoint.live_states {
            if !state.weight.is_finite()
                || state.weight < 0.0
                || state.logs.iter().any(|value| !value.is_finite())
            {
                return Err(FactoredWorstOfError::InvalidCheckpoint);
            }
            live_states.push(LiveState {
                weight: state.weight,
                logs: state.logs,
                knocked: state.knocked,
                missed_coupons: state.missed_coupons,
            });
        }
        Ok(live_states)
    }

    fn checkpoint_from_internal(
        &self,
        sigma_common: f64,
        completed_observations: usize,
        peak_live_state_count: usize,
        accumulator: LegTraceAccumulator,
        live_states: &[LiveState],
    ) -> FactoredWorstOfCheckpoint {
        FactoredWorstOfCheckpoint {
            sigma_common,
            completed_observations: completed_observations as u8,
            peak_live_state_count: peak_live_state_count as u32,
            live_states: live_states
                .iter()
                .map(|state| FactoredWorstOfCheckpointState {
                    weight: state.weight,
                    logs: state.logs,
                    knocked: state.knocked,
                    missed_coupons: state.missed_coupons,
                })
                .collect(),
            redemption_leg_pv: accumulator.redemption_leg_pv,
            coupon_annuity_pv: accumulator.coupon_annuity_pv,
            expected_life_days: accumulator.expected_life_days,
            observation_survival_probability: accumulator.observation_survival_probability,
            observation_autocall_first_hit_probability: accumulator
                .observation_autocall_first_hit_probability,
            observation_first_knock_in_probability: accumulator
                .observation_first_knock_in_probability,
            observation_coupon_annuity_contribution: accumulator
                .observation_coupon_annuity_contribution,
            observation_autocall_redemption_pv_contribution: accumulator
                .observation_autocall_redemption_pv_contribution,
            maturity_redemption_pv: accumulator.maturity_redemption_pv,
            maturity_knock_in_redemption_pv: accumulator.maturity_knock_in_redemption_pv,
        }
    }

    fn trace_build_result_from_state(
        &self,
        completed_observations: usize,
        accumulator: LegTraceAccumulator,
        live_states: &[LiveState],
        peak_live_state_count: usize,
    ) -> TraceBuildResult {
        let knock_in_rate = accumulator
            .observation_first_knock_in_probability
            .into_iter()
            .sum::<f64>()
            .clamp(0.0, 1.0);
        let autocall_rate = accumulator
            .observation_autocall_first_hit_probability
            .into_iter()
            .sum::<f64>()
            .clamp(0.0, 1.0);
        let terminalized = completed_observations == self.shell.observation_days.len();
        let live_probability_mass = if terminalized {
            0.0
        } else {
            live_states.iter().map(|state| state.weight).sum::<f64>()
        };

        TraceBuildResult {
            trace: LegTrace {
                redemption_leg_pv: accumulator.redemption_leg_pv,
                coupon_annuity_pv: accumulator.coupon_annuity_pv,
                expected_life_days: accumulator.expected_life_days,
                knock_in_rate,
                autocall_rate,
                observation_survival_probability: accumulator.observation_survival_probability,
                observation_autocall_first_hit_probability: accumulator
                    .observation_autocall_first_hit_probability,
                observation_first_knock_in_probability: accumulator
                    .observation_first_knock_in_probability,
                observation_coupon_annuity_contribution: accumulator
                    .observation_coupon_annuity_contribution,
                observation_autocall_redemption_pv_contribution: accumulator
                    .observation_autocall_redemption_pv_contribution,
                maturity_redemption_pv: accumulator.maturity_redemption_pv,
                maturity_knock_in_redemption_pv: accumulator.maturity_knock_in_redemption_pv,
            },
            completed_observations,
            terminalized,
            live_state_count: live_states.len(),
            peak_live_state_count,
            live_probability_mass,
        }
    }

    fn trace_build_result_from_checkpoint(
        &self,
        checkpoint: &FactoredWorstOfCheckpoint,
    ) -> Result<TraceBuildResult, FactoredWorstOfError> {
        let accumulator = Self::accumulator_from_checkpoint(checkpoint)?;
        let live_states = Self::live_states_from_checkpoint(checkpoint)?;
        Ok(self.trace_build_result_from_state(
            checkpoint.completed_observations as usize,
            accumulator,
            &live_states,
            checkpoint.peak_live_state_count as usize,
        ))
    }

    /// Current offline-calibrated SPY/QQQ/IWM factor skeleton.
    pub fn spy_qqq_iwm_current() -> Self {
        Self {
            names: ["SPY".to_string(), "QQQ".to_string(), "IWM".to_string()],
            common_factor_loadings: [
                0.515_731_101_696_962,
                0.567_972_444_067_054_3,
                0.641_427_106_944_300_2,
            ],
            uv_factor_slope: [0.052_241_342_370_092_26, 0.125_696_005_247_338_15],
            common_factor: CommonFactorNig {
                alpha: 30.369_787_684_188_967,
                beta: -4.253_775_293_079_228,
                gamma: 30.070_407_375_669_266,
                delta_scale_daily: 29.480_471_389_432_99,
                delta_scale_annual_trading_day: 0.116_985_997_577_115_03,
            },
            residual_covariance_daily: [
                [
                    0.000_008_222_389_184_226_306,
                    0.000_000_103_059_302_629_756_96,
                    -0.000_006_702_362_014_354_354,
                ],
                [
                    0.000_000_103_059_302_629_756_96,
                    0.000_019_862_759_550_171_35,
                    -0.000_017_670_988_414_115_595,
                ],
                [
                    -0.000_006_702_362_014_354_354,
                    -0.000_017_670_988_414_115_595,
                    0.000_021_036_296_842_148_264,
                ],
            ],
            residual_covariance_uv_daily: [
                [
                    0.000_027_879_030_129_138_14,
                    -0.000_002_849_296_518_164_692_7,
                ],
                [
                    -0.000_002_849_296_518_164_692_7,
                    0.000_042_663_410_055_083_27,
                ],
            ],
            autocall_halfplanes: [
                BarrierHalfPlane {
                    a_u: 0.567_972_444_067_054_3,
                    a_v: 0.641_427_106_944_300_2,
                    rhs: 0.0,
                },
                BarrierHalfPlane {
                    a_u: -1.157_158_208_641_262_3,
                    a_v: 0.641_427_106_944_300_2,
                    rhs: 0.0,
                },
                BarrierHalfPlane {
                    a_u: 0.567_972_444_067_054_3,
                    a_v: -1.083_703_545_764_016_3,
                    rhs: 0.0,
                },
            ],
            knock_in_safe_halfplanes: [
                BarrierHalfPlane {
                    a_u: 0.567_972_444_067_054_3,
                    a_v: 0.641_427_106_944_300_2,
                    rhs: 0.384_951_780_326_334_26,
                },
                BarrierHalfPlane {
                    a_u: -1.157_158_208_641_262_3,
                    a_v: 0.641_427_106_944_300_2,
                    rhs: 0.384_951_780_326_334_26,
                },
                BarrierHalfPlane {
                    a_u: 0.567_972_444_067_054_3,
                    a_v: -1.083_703_545_764_016_3,
                    rhs: 0.384_951_780_326_334_26,
                },
            ],
            shell: WorstOfFactoredShell {
                tenor_trading_days: 378,
                observation_days: OBSERVATION_DAYS_18M,
                autocall_barrier: 1.0,
                coupon_barrier: 1.0,
                knock_in_barrier: 0.80,
                quote_share: 0.60,
                issuer_margin_bps: 100.0,
                fair_coupon_floor_bps: 150,
                fair_coupon_ceiling_bps: 1400,
                notional: 100.0,
            },
        }
    }

    /// Host-side mid-life NAV reference over an arbitrary remaining monthly schedule.
    ///
    /// This keeps the existing frozen SPY/QQQ/IWM factor skeleton and walks the
    /// remaining observation dates from the live spot state. Coupon memory is
    /// tracked exactly in the discrete branch state.
    #[cfg(not(target_os = "solana"))]
    #[allow(clippy::too_many_arguments)]
    pub fn midlife_reference_trace(
        &self,
        sigma_common: f64,
        current_logs: [f64; 3],
        now_trading_day: u32,
        remaining_coupon_days: &[u32],
        remaining_autocall_days: &[u32],
        ki_latched: bool,
        missed_coupon_observations: u8,
        coupon_per_observation: f64,
        max_live_states: usize,
    ) -> Result<MidlifeReferenceTrace, FactoredWorstOfError> {
        self.validate()?;
        if !sigma_common.is_finite() || sigma_common <= 0.0 {
            return Err(FactoredWorstOfError::InvalidSigmaCommon);
        }
        if current_logs.iter().any(|value| !value.is_finite()) {
            return Err(FactoredWorstOfError::InvalidBarrierShift);
        }
        if !coupon_per_observation.is_finite() || coupon_per_observation < 0.0 {
            return Err(FactoredWorstOfError::InvalidShape);
        }

        let coupon_barrier = self.shell.coupon_barrier;
        let autocall_barrier = self.shell.autocall_barrier;
        let knock_in_barrier = self.shell.knock_in_barrier;
        let max_live_states = max_live_states.max(1);

        let mut previous_day = now_trading_day;
        let mut prev_coupon_day = None;
        for &day in remaining_coupon_days {
            if day < now_trading_day {
                return Err(FactoredWorstOfError::InvalidStepDays);
            }
            if let Some(prev) = prev_coupon_day {
                if day <= prev {
                    return Err(FactoredWorstOfError::InvalidStepDays);
                }
            }
            prev_coupon_day = Some(day);
        }

        let mut prev_autocall_day = None;
        for &day in remaining_autocall_days {
            if day < now_trading_day {
                return Err(FactoredWorstOfError::InvalidStepDays);
            }
            if remaining_coupon_days.binary_search(&day).is_err() {
                return Err(FactoredWorstOfError::InvalidStepDays);
            }
            if let Some(prev) = prev_autocall_day {
                if day <= prev {
                    return Err(FactoredWorstOfError::InvalidStepDays);
                }
            }
            prev_autocall_day = Some(day);
        }

        if remaining_coupon_days.is_empty() {
            let worst = current_logs
                .iter()
                .map(|value| value.exp())
                .fold(f64::INFINITY, f64::min);
            if !worst.is_finite() || worst <= 0.0 {
                return Err(FactoredWorstOfError::InvalidShape);
            }
            let redemption = if ki_latched && worst < 1.0 {
                worst
            } else {
                1.0
            };
            return Ok(MidlifeReferenceTrace {
                nav_per_notional: redemption.max(0.0),
                remaining_coupon_pv_per_notional: 0.0,
                par_recovery_probability: if redemption >= 1.0 - 1.0e-12 {
                    1.0
                } else {
                    0.0
                },
            });
        }

        let mut live_states = vec![LiveState {
            weight: 1.0,
            logs: current_logs,
            knocked: ki_latched,
            missed_coupons: missed_coupon_observations,
        }];
        let mut redemption_pv = 0.0_f64;
        let mut coupon_pv = 0.0_f64;
        let mut par_recovery_probability = 0.0_f64;

        for (observation_index, &observation_day) in remaining_coupon_days.iter().enumerate() {
            let step_days = observation_day
                .checked_sub(previous_day)
                .ok_or(FactoredWorstOfError::InvalidStepDays)?;
            previous_day = observation_day;
            let is_maturity = observation_index + 1 == remaining_coupon_days.len();
            let is_autocall_day = remaining_autocall_days
                .binary_search(&observation_day)
                .is_ok();
            let outcomes = if step_days == 0 {
                vec![StepOutcome {
                    weight: 1.0,
                    log_return_increments: [0.0; 3],
                }]
            } else {
                self.step_outcomes(sigma_common, step_days)?
            };
            let mut next_buckets = HashMap::<StateKey, BucketAccumulator>::new();

            for state in &live_states {
                for outcome in &outcomes {
                    if !outcome.weight.is_finite() || outcome.weight <= 0.0 {
                        continue;
                    }
                    let branch_weight = state.weight * outcome.weight;
                    if !branch_weight.is_finite() || branch_weight <= 0.0 {
                        continue;
                    }
                    if branch_weight < PATH_WEIGHT_CUTOFF {
                        break;
                    }

                    let next_logs = [
                        state.logs[0] + outcome.log_return_increments[0],
                        state.logs[1] + outcome.log_return_increments[1],
                        state.logs[2] + outcome.log_return_increments[2],
                    ];
                    let levels = [next_logs[0].exp(), next_logs[1].exp(), next_logs[2].exp()];
                    if levels
                        .iter()
                        .any(|value| !value.is_finite() || *value <= 0.0)
                    {
                        return Err(FactoredWorstOfError::InvalidShape);
                    }
                    let worst = levels[0].min(levels[1]).min(levels[2]);
                    let coupon_due = worst >= coupon_barrier;
                    let coupon_multiplier = if coupon_due {
                        f64::from(state.missed_coupons) + 1.0
                    } else {
                        0.0
                    };
                    let coupon_payment = coupon_per_observation * coupon_multiplier;
                    coupon_pv += branch_weight * coupon_payment;

                    if is_autocall_day
                        && !is_maturity
                        && levels.iter().all(|level| *level >= autocall_barrier)
                    {
                        redemption_pv += branch_weight;
                        par_recovery_probability += branch_weight;
                        continue;
                    }

                    let knocked_next = state.knocked || worst <= knock_in_barrier;
                    if is_maturity {
                        let redemption = if knocked_next && worst < 1.0 {
                            worst
                        } else {
                            1.0
                        };
                        redemption_pv += branch_weight * redemption;
                        if redemption >= 1.0 - 1.0e-12 {
                            par_recovery_probability += branch_weight;
                        }
                        continue;
                    }

                    let missed_next = if coupon_due {
                        0
                    } else {
                        state
                            .missed_coupons
                            .checked_add(1)
                            .ok_or(FactoredWorstOfError::InvalidCheckpoint)?
                    };
                    let key = Self::state_key(next_logs, knocked_next, missed_next);
                    let bucket = next_buckets.entry(key).or_insert(BucketAccumulator {
                        weight: 0.0,
                        weighted_logs: [0.0; 3],
                    });
                    bucket.weight += branch_weight;
                    bucket.weighted_logs[0] += branch_weight * next_logs[0];
                    bucket.weighted_logs[1] += branch_weight * next_logs[1];
                    bucket.weighted_logs[2] += branch_weight * next_logs[2];
                }
            }

            if is_maturity {
                live_states.clear();
                continue;
            }

            let mut ordered_buckets = next_buckets.into_iter().collect::<Vec<_>>();
            ordered_buckets.sort_by(|(left_key, left_bucket), (right_key, right_bucket)| {
                right_bucket
                    .weight
                    .total_cmp(&left_bucket.weight)
                    .then_with(|| left_key.cmp(right_key))
            });
            live_states = ordered_buckets
                .into_iter()
                .filter_map(|(key, bucket)| {
                    if !bucket.weight.is_finite() || bucket.weight <= STATE_MASS_EPS {
                        return None;
                    }
                    Some(LiveState {
                        weight: bucket.weight,
                        logs: [
                            bucket.weighted_logs[0] / bucket.weight,
                            bucket.weighted_logs[1] / bucket.weight,
                            bucket.weighted_logs[2] / bucket.weight,
                        ],
                        knocked: key.knocked,
                        missed_coupons: key.missed_coupons,
                    })
                })
                .collect();
            if live_states.len() > max_live_states {
                live_states.truncate(max_live_states);
            }
        }

        Ok(MidlifeReferenceTrace {
            nav_per_notional: (redemption_pv + coupon_pv).max(0.0),
            remaining_coupon_pv_per_notional: coupon_pv.max(0.0),
            par_recovery_probability: par_recovery_probability.clamp(0.0, 1.0),
        })
    }

    /// Basic self-consistency checks for the frozen factor skeleton.
    pub fn validate(&self) -> Result<(), FactoredWorstOfError> {
        if self.common_factor.alpha <= 0.0
            || self.common_factor.gamma <= 0.0
            || self.common_factor.beta.abs() >= self.common_factor.alpha
        {
            return Err(FactoredWorstOfError::InvalidShape);
        }
        if self.shell.tenor_trading_days == 0 || self.shell.observation_days[0] == 0 {
            return Err(FactoredWorstOfError::InvalidStepDays);
        }
        if self.residual_covariance_uv_daily[0][0] <= 0.0
            || self.residual_covariance_uv_daily[1][1] <= 0.0
        {
            return Err(FactoredWorstOfError::InvalidCovariance);
        }
        Ok(())
    }

    fn validate_onchain_config(
        config: FactoredWorstOfOnchainConfig,
    ) -> Result<(), FactoredWorstOfError> {
        let factor_ok = matches!(config.factor_order, 5 | 7 | 9 | 13);
        let triangle_ok = matches!(config.triangle_gl_order, 0 | 5 | 7 | 20);
        let ki_ok = matches!(config.ki_order, 0 | 3 | 4 | 5 | 6 | 7 | 8 | 9 | 10 | 13);
        let components_ok = (1..=8).contains(&config.components_per_class);
        if factor_ok && triangle_ok && ki_ok && components_ok {
            Ok(())
        } else {
            Err(FactoredWorstOfError::InvalidQuadratureOrder)
        }
    }

    fn factor_rule(order: u8) -> Result<(&'static [i128], &'static [i128]), FactoredWorstOfError> {
        match order {
            5 => Ok((&GH5_NODES, &GH5_WEIGHTS)),
            7 => Ok((&GH7_NODES, &GH7_WEIGHTS)),
            9 => Ok((&GH9_NODES, &GH9_WEIGHTS)),
            13 => Ok((&GH13_NODES, &GH13_WEIGHTS)),
            _ => Err(FactoredWorstOfError::InvalidQuadratureOrder),
        }
    }

    fn ki_triangle_order(order: u8) -> Result<u8, FactoredWorstOfError> {
        match order {
            3 | 4 | 5 | 6 => Ok(5),
            7 | 8 | 9 | 10 | 13 => Ok(7),
            _ => Err(FactoredWorstOfError::InvalidQuadratureOrder),
        }
    }

    /// Live common-factor delta for one observation step.
    pub fn delta_step(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<f64, FactoredWorstOfError> {
        if !sigma_common.is_finite() || sigma_common <= 0.0 {
            return Err(FactoredWorstOfError::InvalidSigmaCommon);
        }
        if step_days == 0 {
            return Err(FactoredWorstOfError::InvalidStepDays);
        }
        Ok(sigma_common
            * sigma_common
            * self.common_factor.delta_scale_annual_trading_day
            * step_days as f64)
    }

    /// Location parameter that keeps the common factor mean-zero.
    pub fn zero_mean_common_factor_location(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<f64, FactoredWorstOfError> {
        let delta = self.delta_step(sigma_common, step_days)?;
        Ok(-delta * self.common_factor.beta / self.common_factor.gamma)
    }

    /// Risk-neutral log drifts that enforce `E[e^{dx_i}] = 1` on the step.
    pub fn risk_neutral_step_drifts(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<[f64; 3], FactoredWorstOfError> {
        let delta = self.delta_step(sigma_common, step_days)?;
        let location = self.zero_mean_common_factor_location(sigma_common, step_days)?;
        let mut drifts = [0.0_f64; 3];
        let residual_scale = step_days as f64;
        for (index, drift) in drifts.iter_mut().enumerate() {
            let loading = self.common_factor_loadings[index];
            let shifted =
                self.common_factor.alpha.powi(2) - (self.common_factor.beta + loading).powi(2);
            if shifted <= 0.0 {
                return Err(FactoredWorstOfError::InvalidShape);
            }
            let common_term =
                location * loading + delta * (self.common_factor.gamma - shifted.sqrt());
            let gaussian_term = 0.5 * self.residual_covariance_daily[index][index] * residual_scale;
            *drift = -(common_term + gaussian_term);
        }
        Ok(drifts)
    }

    /// Shifted barrier half-planes for one factor level and observation step.
    pub fn shifted_halfplanes(
        &self,
        base: [BarrierHalfPlane; 3],
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
    ) -> Result<[BarrierHalfPlane; 3], FactoredWorstOfError> {
        if !factor_value.is_finite() {
            return Err(FactoredWorstOfError::InvalidBarrierShift);
        }
        let drifts = self.risk_neutral_step_drifts(sigma_common, step_days)?;
        let drift_shift = self.common_factor_loadings[0] * drifts[0]
            + self.common_factor_loadings[1] * drifts[1]
            + self.common_factor_loadings[2] * drifts[2];
        Ok(base.map(|plane| BarrierHalfPlane {
            a_u: plane.a_u,
            a_v: plane.a_v,
            rhs: plane.rhs + factor_value + drift_shift,
        }))
    }

    fn total_loading(&self) -> Result<f64, FactoredWorstOfError> {
        let total_loading = self.common_factor_loadings[0]
            + self.common_factor_loadings[1]
            + self.common_factor_loadings[2];
        if total_loading.abs() <= 1.0e-12 {
            return Err(FactoredWorstOfError::InvalidShape);
        }
        Ok(total_loading)
    }

    fn uniform_barrier_halfplanes(
        &self,
        barrier_level: f64,
    ) -> Result<[BarrierHalfPlane; 3], FactoredWorstOfError> {
        if !barrier_level.is_finite() || barrier_level <= 0.0 {
            return Err(FactoredWorstOfError::InvalidBarrierShift);
        }
        let total_loading = self.total_loading()?;
        let rhs = -total_loading * barrier_level.ln();
        Ok([
            BarrierHalfPlane {
                a_u: self.common_factor_loadings[1],
                a_v: self.common_factor_loadings[2],
                rhs,
            },
            BarrierHalfPlane {
                a_u: self.common_factor_loadings[1] - total_loading,
                a_v: self.common_factor_loadings[2],
                rhs,
            },
            BarrierHalfPlane {
                a_u: self.common_factor_loadings[1],
                a_v: self.common_factor_loadings[2] - total_loading,
                rhs,
            },
        ])
    }

    fn state_shifted_halfplanes(
        &self,
        base: [BarrierHalfPlane; 3],
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
    ) -> Result<[BarrierHalfPlane; 3], FactoredWorstOfError> {
        if current_logs.iter().any(|value| !value.is_finite()) {
            return Err(FactoredWorstOfError::InvalidBarrierShift);
        }
        let total_loading = self.total_loading()?;
        let shifted = self.shifted_halfplanes(base, sigma_common, step_days, factor_value)?;
        Ok([
            BarrierHalfPlane {
                a_u: shifted[0].a_u,
                a_v: shifted[0].a_v,
                rhs: shifted[0].rhs + total_loading * current_logs[0],
            },
            BarrierHalfPlane {
                a_u: shifted[1].a_u,
                a_v: shifted[1].a_v,
                rhs: shifted[1].rhs + total_loading * current_logs[1],
            },
            BarrierHalfPlane {
                a_u: shifted[2].a_u,
                a_v: shifted[2].a_v,
                rhs: shifted[2].rhs + total_loading * current_logs[2],
            },
        ])
    }

    /// Conditional `(u, v)` Gaussian mean and covariance at one observation date.
    pub fn conditional_uv_distribution(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
    ) -> Result<([f64; 2], [[f64; 2]; 2]), FactoredWorstOfError> {
        let drifts = self.risk_neutral_step_drifts(sigma_common, step_days)?;
        let mean = [
            (drifts[1] - drifts[0]) + self.uv_factor_slope[0] * factor_value,
            (drifts[2] - drifts[0]) + self.uv_factor_slope[1] * factor_value,
        ];
        let scale = step_days as f64;
        let covariance = [
            [
                self.residual_covariance_uv_daily[0][0] * scale,
                self.residual_covariance_uv_daily[0][1] * scale,
            ],
            [
                self.residual_covariance_uv_daily[1][0] * scale,
                self.residual_covariance_uv_daily[1][1] * scale,
            ],
        ];
        if covariance[0][0] <= 0.0 || covariance[1][1] <= 0.0 {
            return Err(FactoredWorstOfError::InvalidCovariance);
        }
        Ok((mean, covariance))
    }

    fn to_fixed(value: f64) -> Result<i128, FactoredWorstOfError> {
        if !value.is_finite() {
            return Err(FactoredWorstOfError::InvalidBarrierShift);
        }
        let scaled = value * SCALE_I as f64;
        if !scaled.is_finite() || scaled < i128::MIN as f64 || scaled > i128::MAX as f64 {
            return Err(FactoredWorstOfError::InvalidBarrierShift);
        }
        Ok(scaled.round() as i128)
    }

    fn solve_halfplane_intersection(
        a: BarrierHalfPlane,
        b: BarrierHalfPlane,
    ) -> Option<(f64, f64)> {
        let det = a.a_u * b.a_v - b.a_u * a.a_v;
        if det.abs() < 1.0e-14 {
            return None;
        }
        let u = (a.rhs * b.a_v - b.rhs * a.a_v) / det;
        let v = (a.a_u * b.rhs - b.a_u * a.rhs) / det;
        Some((u, v))
    }

    fn triangle_vertices_f64(planes: [BarrierHalfPlane; 3]) -> Vec<(f64, f64)> {
        let mut vertices = Vec::new();
        for i in 0..3 {
            for j in (i + 1)..3 {
                let Some(point) = Self::solve_halfplane_intersection(planes[i], planes[j]) else {
                    continue;
                };
                let feasible = planes
                    .iter()
                    .all(|plane| plane.a_u * point.0 + plane.a_v * point.1 <= plane.rhs + 1.0e-10);
                if feasible
                    && !vertices.iter().any(|existing: &(f64, f64)| {
                        (existing.0 - point.0).abs() <= 1.0e-10
                            && (existing.1 - point.1).abs() <= 1.0e-10
                    })
                {
                    vertices.push(point);
                }
            }
        }
        vertices.sort_by(|left, right| left.0.total_cmp(&right.0));
        vertices
    }

    fn vertical_section_f64(vertices: &[(f64, f64)], u_value: f64) -> Option<(f64, f64)> {
        let mut hits = Vec::new();
        for index in 0..vertices.len() {
            let (x1, y1) = vertices[index];
            let (x2, y2) = vertices[(index + 1) % vertices.len()];
            let u_min = x1.min(x2);
            let u_max = x1.max(x2);
            if u_value < u_min - 1.0e-12 || u_value > u_max + 1.0e-12 {
                continue;
            }
            if (x2 - x1).abs() < 1.0e-12 {
                hits.push(y1);
                hits.push(y2);
                continue;
            }
            let t = (u_value - x1) / (x2 - x1);
            if (-1.0e-12..=1.0 + 1.0e-12).contains(&t) {
                hits.push(y1 + t * (y2 - y1));
            }
        }
        if hits.is_empty() {
            return None;
        }
        hits.sort_by(|left, right| left.total_cmp(right));
        Some((hits[0], hits[hits.len() - 1]))
    }

    fn triangle_probability_f64_fallback_explicit(
        &self,
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
        planes: [BarrierHalfPlane; 3],
    ) -> Result<f64, FactoredWorstOfError> {
        let vertices = Self::triangle_vertices_f64(planes);
        if vertices.len() < 3 {
            return Ok(0.0);
        }
        let var_u = covariance[0][0];
        let cov_uv = covariance[0][1];
        let var_v = covariance[1][1];
        if var_u <= 0.0 || var_v <= 0.0 {
            return Err(FactoredWorstOfError::InvalidCovariance);
        }
        let cond_var = var_v - cov_uv * cov_uv / var_u;
        if cond_var <= 0.0 {
            return Err(FactoredWorstOfError::InvalidCovariance);
        }
        let sigma_u = var_u.sqrt();
        let sigma_v_cond = cond_var.sqrt();
        let mut total = 0.0_f64;
        let mut x_coords: Vec<f64> = vertices.iter().map(|point| point.0).collect();
        x_coords.sort_by(|left, right| left.total_cmp(right));

        const GL20_NODES: [f64; 20] = [
            -0.993_128_599_185,
            -0.963_971_927_278,
            -0.912_234_428_251,
            -0.839_116_971_822,
            -0.746_331_906_460,
            -0.636_053_680_727,
            -0.510_867_001_951,
            -0.373_706_088_715,
            -0.227_785_851_142,
            -0.076_526_521_133,
            0.076_526_521_133,
            0.227_785_851_142,
            0.373_706_088_715,
            0.510_867_001_951,
            0.636_053_680_727,
            0.746_331_906_460,
            0.839_116_971_822,
            0.912_234_428_251,
            0.963_971_927_278,
            0.993_128_599_185,
        ];
        const GL20_WEIGHTS: [f64; 20] = [
            0.017_614_007_139,
            0.040_601_429_800,
            0.062_672_048_334,
            0.083_276_741_577,
            0.101_930_119_817,
            0.118_194_531_962,
            0.131_688_638_449,
            0.142_096_109_318,
            0.149_172_986_473,
            0.152_753_387_131,
            0.152_753_387_131,
            0.149_172_986_473,
            0.142_096_109_318,
            0.131_688_638_449,
            0.118_194_531_962,
            0.101_930_119_817,
            0.083_276_741_577,
            0.062_672_048_334,
            0.040_601_429_800,
            0.017_614_007_139,
        ];

        for interval_index in 0..(x_coords.len() - 1) {
            let left = x_coords[interval_index];
            let right = x_coords[interval_index + 1];
            if right - left <= 1.0e-12 {
                continue;
            }
            let half = 0.5 * (right - left);
            let mid = 0.5 * (left + right);
            for node_index in 0..GL20_NODES.len() {
                let u_value = mid + half * GL20_NODES[node_index];
                let Some((v_lo, v_hi)) = Self::vertical_section_f64(&vertices, u_value) else {
                    continue;
                };
                let z_u = (u_value - mean[0]) / sigma_u;
                let pdf_u = (-0.5 * z_u * z_u).exp() / (sigma_u * SQRT_2PI);
                let cond_mean = mean[1] + (cov_uv / var_u) * (u_value - mean[0]);
                let z_hi = (v_hi - cond_mean) / sigma_v_cond;
                let z_lo = (v_lo - cond_mean) / sigma_v_cond;
                let cdf_hi = 0.5 * (1.0 + erf_approx(z_hi / SQRT_2));
                let cdf_lo = 0.5 * (1.0 + erf_approx(z_lo / SQRT_2));
                total += GL20_WEIGHTS[node_index] * pdf_u * (cdf_hi - cdf_lo).max(0.0) * half;
            }
        }

        Ok(total.clamp(0.0, 1.0))
    }

    fn halfplanes_to_fixed(
        planes: [BarrierHalfPlane; 3],
    ) -> Result<[FixedHalfPlane; 3], FactoredWorstOfError> {
        Ok([
            FixedHalfPlane {
                a_u: Self::to_fixed(planes[0].a_u)?,
                a_v: Self::to_fixed(planes[0].a_v)?,
                rhs: Self::to_fixed(planes[0].rhs)?,
            },
            FixedHalfPlane {
                a_u: Self::to_fixed(planes[1].a_u)?,
                a_v: Self::to_fixed(planes[1].a_v)?,
                rhs: Self::to_fixed(planes[1].rhs)?,
            },
            FixedHalfPlane {
                a_u: Self::to_fixed(planes[2].a_u)?,
                a_v: Self::to_fixed(planes[2].a_v)?,
                rhs: Self::to_fixed(planes[2].rhs)?,
            },
        ])
    }

    fn triangle_probability_explicit(
        &self,
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
        planes: [BarrierHalfPlane; 3],
    ) -> Result<f64, FactoredWorstOfError> {
        self.triangle_probability_explicit_with_order(mean, covariance, planes, 20, None)
    }

    fn triangle_probability_explicit_with_order(
        &self,
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
        planes: [BarrierHalfPlane; 3],
        triangle_gl_order: u8,
        triple_pre: Option<&crate::worst_of_c1_fast::TripleCorrectionPre>,
    ) -> Result<f64, FactoredWorstOfError> {
        let mean_u = Self::to_fixed(mean[0])?;
        let mean_v = Self::to_fixed(mean[1])?;
        let var_uu = Self::to_fixed(covariance[0][0])?;
        let cov_uv = Self::to_fixed(covariance[0][1])?;
        let var_vv = Self::to_fixed(covariance[1][1])?;
        let fixed_planes = Self::halfplanes_to_fixed(planes)?;
        let probability = if triangle_gl_order == 0 {
            // Flattened i64 path with GH3 triple correction
            use crate::worst_of_c1_fast::triple_complement_gh3;
            let rhs = [
                fixed_planes[0].rhs,
                fixed_planes[1].rhs,
                fixed_planes[2].rhs,
            ];
            let p_ie =
                triangle_probability_i64_fp(mean_u, mean_v, rhs, &TRIANGLE_PRE64_63, PHI2_TABLES);
            if let Some(tp) = triple_pre {
                const S6: i64 = 1_000_000;
                let mu6 = (mean_u / S6 as i128) as i64;
                let mv6 = (mean_v / S6 as i128) as i64;
                let pre = &TRIANGLE_PRE64_63;
                let mut num6 = [0i64; 3];
                for k in 0..3 {
                    let rhs6 = (rhs[k] / S6 as i128) as i64;
                    let ew6 = (pre.au[k] as i64 * mu6 + pre.av[k] as i64 * mv6) / S6;
                    num6[k] = rhs6 - ew6;
                }
                let phi3 = triple_complement_gh3(tp, num6) as i128 * S6 as i128;
                Ok((p_ie - phi3).max(0))
            } else {
                Ok(p_ie)
            }
        } else if triangle_gl_order == 20 {
            triangle_probability_fp(mean_u, mean_v, var_uu, cov_uv, var_vv, fixed_planes)
        } else {
            triangle_probability_fp_with_order(
                mean_u,
                mean_v,
                var_uu,
                cov_uv,
                var_vv,
                fixed_planes,
                triangle_gl_order as usize,
            )
        };
        match probability {
            Ok(value) => Ok((value as f64 / SCALE_I as f64).clamp(0.0, 1.0)),
            Err(SolMathError::DomainError) => {
                self.triangle_probability_f64_fallback_explicit(mean, covariance, planes)
            }
            Err(error) => Err(FactoredWorstOfError::SolMath(error)),
        }
    }

    fn triangle_uv_region_moment_explicit_with_order(
        &self,
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
        planes: [BarrierHalfPlane; 3],
        triangle_gl_order: u8,
        triple_pre: Option<&crate::worst_of_c1_fast::TripleCorrectionPre>,
    ) -> Result<UvRegionMoment, FactoredWorstOfError> {
        let mean_u = Self::to_fixed(mean[0])?;
        let mean_v = Self::to_fixed(mean[1])?;
        let var_uu = Self::to_fixed(covariance[0][0])?;
        let cov_uv = Self::to_fixed(covariance[0][1])?;
        let var_vv = Self::to_fixed(covariance[1][1])?;
        let fixed_planes = Self::halfplanes_to_fixed(planes)?;
        if triangle_gl_order == 0 {
            // Fused probability + moments via first-order truncation corrections.
            // Uses crate-local copy to avoid BPF cross-crate register-spill overhead.
            use crate::worst_of_c1_fast::triangle_probability_and_moments_local;
            const S6F: f64 = 1_000_000.0;
            let rhs = [
                fixed_planes[0].rhs,
                fixed_planes[1].rhs,
                fixed_planes[2].rhs,
            ];
            let uncond_cov_s6 = [
                (covariance[0][0] * S6F) as i64,
                (covariance[0][1] * S6F) as i64,
                (covariance[1][1] * S6F) as i64,
            ];
            let m6 = triangle_probability_and_moments_local(
                mean_u,
                mean_v,
                rhs,
                &TRIANGLE_PRE64_63,
                PHI2_TABLES,
                &COV_PROJ_63,
                &TRIANGLE_PAIR_RHO_63,
                &TRIANGLE_PAIR_INV_SQRT_1MRHO2_63,
                uncond_cov_s6,
                triple_pre,
            );
            return Ok(UvRegionMoment {
                probability: (m6.probability as f64 / S6F).clamp(0.0, 1.0),
                expectation_u: m6.expectation_u as f64 / S6F,
                expectation_v: m6.expectation_v as f64 / S6F,
                expectation_uu: m6.expectation_uu as f64 / S6F,
                expectation_uv: m6.expectation_uv as f64 / S6F,
                expectation_vv: m6.expectation_vv as f64 / S6F,
            });
        }
        let moment_order = triangle_gl_order;
        let fixed_moment = if moment_order == 20 {
            triangle_region_moment_fp(mean_u, mean_v, var_uu, cov_uv, var_vv, fixed_planes)
        } else {
            triangle_region_moment_fp_with_order(
                mean_u,
                mean_v,
                var_uu,
                cov_uv,
                var_vv,
                fixed_planes,
                moment_order as usize,
            )
        };
        match fixed_moment {
            Ok(moment) => Ok(Self::triangle_region_moment_from_fixed(moment)),
            Err(SolMathError::DomainError) => self
                .triangle_uv_region_moment_f64_fallback_with_order(
                    mean,
                    covariance,
                    planes,
                    triangle_gl_order,
                ),
            Err(error) => Err(FactoredWorstOfError::SolMath(error)),
        }
    }

    fn triangle_region_moment_from_fixed(moment: FixedTriangleRegionMoment) -> UvRegionMoment {
        UvRegionMoment {
            probability: (moment.probability as f64 / SCALE_I as f64).clamp(0.0, 1.0),
            expectation_u: moment.expectation_u as f64 / SCALE_I as f64,
            expectation_v: moment.expectation_v as f64 / SCALE_I as f64,
            expectation_uu: moment.expectation_uu as f64 / SCALE_I as f64,
            expectation_uv: moment.expectation_uv as f64 / SCALE_I as f64,
            expectation_vv: moment.expectation_vv as f64 / SCALE_I as f64,
        }
    }

    fn triangle_uv_region_moment_f64_fallback_with_order(
        &self,
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
        planes: [BarrierHalfPlane; 3],
        triangle_gl_order: u8,
    ) -> Result<UvRegionMoment, FactoredWorstOfError> {
        let vertices = Self::triangle_vertices_f64(planes);
        if vertices.len() < 3 {
            return Ok(UvRegionMoment::default());
        }

        let var_u = covariance[0][0];
        let cov_uv = covariance[0][1];
        let var_v = covariance[1][1];
        if var_u <= 0.0 || var_v <= 0.0 {
            return Err(FactoredWorstOfError::InvalidCovariance);
        }
        let cond_var = var_v - cov_uv * cov_uv / var_u;
        if cond_var <= 0.0 {
            return Err(FactoredWorstOfError::InvalidCovariance);
        }
        let sigma_u = var_u.sqrt();
        let sigma_v_cond = cond_var.sqrt();
        let mut x_coords: Vec<f64> = vertices.iter().map(|point| point.0).collect();
        x_coords.sort_by(|left, right| left.total_cmp(right));

        let gl_order = if triangle_gl_order == 0 {
            5
        } else {
            triangle_gl_order
        };
        let (nodes_i128, weights_i128) = match gl_order {
            5 => (Some(&GL5_NODES[..]), Some(&GL5_WEIGHTS[..])),
            7 => (Some(&GL7_NODES[..]), Some(&GL7_WEIGHTS[..])),
            20 => (None, None),
            _ => return Err(FactoredWorstOfError::InvalidQuadratureOrder),
        };

        let mut moment = UvRegionMoment::default();

        for interval_index in 0..(x_coords.len() - 1) {
            let left = x_coords[interval_index];
            let right = x_coords[interval_index + 1];
            if right - left <= 1.0e-12 {
                continue;
            }
            let half = 0.5 * (right - left);
            let mid = 0.5 * (left + right);

            match (nodes_i128, weights_i128) {
                (Some(nodes), Some(weights)) => {
                    for node_index in 0..nodes.len() {
                        let node = nodes[node_index] as f64 / SCALE_I as f64;
                        let weight = weights[node_index] as f64 / SCALE_I as f64;
                        let u_value = mid + half * node;
                        let Some((v_lo, v_hi)) = Self::vertical_section_f64(&vertices, u_value)
                        else {
                            continue;
                        };
                        let z_u = (u_value - mean[0]) / sigma_u;
                        let pdf_u = (-0.5 * z_u * z_u).exp() / (sigma_u * SQRT_2PI);
                        let cond_mean = mean[1] + (cov_uv / var_u) * (u_value - mean[0]);
                        let z_hi = (v_hi - cond_mean) / sigma_v_cond;
                        let z_lo = (v_lo - cond_mean) / sigma_v_cond;
                        let cdf_hi = 0.5 * (1.0 + erf_approx(z_hi / SQRT_2));
                        let cdf_lo = 0.5 * (1.0 + erf_approx(z_lo / SQRT_2));
                        let prob_v = (cdf_hi - cdf_lo).max(0.0);
                        let pdf_hi = (-0.5 * z_hi * z_hi).exp() / SQRT_2PI;
                        let pdf_lo = (-0.5 * z_lo * z_lo).exp() / SQRT_2PI;
                        let v_truncated = cond_mean * prob_v + sigma_v_cond * (pdf_lo - pdf_hi);
                        let second_moment_z = prob_v - z_hi * pdf_hi + z_lo * pdf_lo;
                        let v_second = cond_mean * cond_mean * prob_v
                            + 2.0 * cond_mean * sigma_v_cond * (pdf_lo - pdf_hi)
                            + sigma_v_cond * sigma_v_cond * second_moment_z;
                        let shell_weight = weight * pdf_u * half;
                        moment.probability += shell_weight * prob_v;
                        moment.expectation_u += shell_weight * u_value * prob_v;
                        moment.expectation_v += shell_weight * v_truncated;
                        moment.expectation_uu += shell_weight * u_value * u_value * prob_v;
                        moment.expectation_uv += shell_weight * u_value * v_truncated;
                        moment.expectation_vv += shell_weight * v_second;
                    }
                }
                (None, None) => {
                    for node_index in 0..GL20_NODES_F64.len() {
                        let u_value = mid + half * GL20_NODES_F64[node_index];
                        let Some((v_lo, v_hi)) = Self::vertical_section_f64(&vertices, u_value)
                        else {
                            continue;
                        };
                        let z_u = (u_value - mean[0]) / sigma_u;
                        let pdf_u = (-0.5 * z_u * z_u).exp() / (sigma_u * SQRT_2PI);
                        let cond_mean = mean[1] + (cov_uv / var_u) * (u_value - mean[0]);
                        let z_hi = (v_hi - cond_mean) / sigma_v_cond;
                        let z_lo = (v_lo - cond_mean) / sigma_v_cond;
                        let cdf_hi = 0.5 * (1.0 + erf_approx(z_hi / SQRT_2));
                        let cdf_lo = 0.5 * (1.0 + erf_approx(z_lo / SQRT_2));
                        let prob_v = (cdf_hi - cdf_lo).max(0.0);
                        let pdf_hi = (-0.5 * z_hi * z_hi).exp() / SQRT_2PI;
                        let pdf_lo = (-0.5 * z_lo * z_lo).exp() / SQRT_2PI;
                        let v_truncated = cond_mean * prob_v + sigma_v_cond * (pdf_lo - pdf_hi);
                        let second_moment_z = prob_v - z_hi * pdf_hi + z_lo * pdf_lo;
                        let v_second = cond_mean * cond_mean * prob_v
                            + 2.0 * cond_mean * sigma_v_cond * (pdf_lo - pdf_hi)
                            + sigma_v_cond * sigma_v_cond * second_moment_z;
                        let shell_weight = GL20_WEIGHTS_F64[node_index] * pdf_u * half;
                        moment.probability += shell_weight * prob_v;
                        moment.expectation_u += shell_weight * u_value * prob_v;
                        moment.expectation_v += shell_weight * v_truncated;
                        moment.expectation_uu += shell_weight * u_value * u_value * prob_v;
                        moment.expectation_uv += shell_weight * u_value * v_truncated;
                        moment.expectation_vv += shell_weight * v_second;
                    }
                }
                _ => return Err(FactoredWorstOfError::InvalidQuadratureOrder),
            }
        }

        moment.probability = moment.probability.clamp(0.0, 1.0);
        Ok(moment)
    }

    fn triangle_uv_moment_explicit_with_order(
        &self,
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
        planes: [BarrierHalfPlane; 3],
        triangle_gl_order: u8,
    ) -> Result<(f64, f64, f64), FactoredWorstOfError> {
        let moment = self.triangle_uv_region_moment_explicit_with_order(
            mean,
            covariance,
            planes,
            triangle_gl_order,
            None,
        )?;
        Ok((
            moment.probability,
            moment.expectation_u,
            moment.expectation_v,
        ))
    }

    /// Exact Gaussian triangle probability for one factor value and observation step.
    pub fn triangle_probability(
        &self,
        base: [BarrierHalfPlane; 3],
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
    ) -> Result<f64, FactoredWorstOfError> {
        let (mean, covariance) =
            self.conditional_uv_distribution(sigma_common, step_days, factor_value)?;
        let planes = self.shifted_halfplanes(base, sigma_common, step_days, factor_value)?;
        self.triangle_probability_explicit(mean, covariance, planes)
    }

    /// Conditional autocall-region probability in the Gaussian spread plane.
    pub fn autocall_probability(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
    ) -> Result<f64, FactoredWorstOfError> {
        self.triangle_probability(
            self.autocall_halfplanes,
            sigma_common,
            step_days,
            factor_value,
        )
    }

    /// Conditional knock-in-safe probability in the Gaussian spread plane.
    pub fn knock_in_safe_probability(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
    ) -> Result<f64, FactoredWorstOfError> {
        self.triangle_probability(
            self.knock_in_safe_halfplanes,
            sigma_common,
            step_days,
            factor_value,
        )
    }

    fn drift_shift(&self, sigma_common: f64, step_days: u32) -> Result<f64, FactoredWorstOfError> {
        let drifts = self.risk_neutral_step_drifts(sigma_common, step_days)?;
        Ok(self.common_factor_loadings[0] * drifts[0]
            + self.common_factor_loadings[1] * drifts[1]
            + self.common_factor_loadings[2] * drifts[2])
    }

    fn common_factor_std(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<f64, FactoredWorstOfError> {
        if !sigma_common.is_finite() || sigma_common <= 0.0 {
            return Err(FactoredWorstOfError::InvalidSigmaCommon);
        }
        if step_days == 0 {
            return Err(FactoredWorstOfError::InvalidStepDays);
        }
        Ok(sigma_common * (step_days as f64 / 252.0).sqrt())
    }

    fn common_factor_pdf(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
    ) -> Result<f64, FactoredWorstOfError> {
        let delta = self.delta_step(sigma_common, step_days)?;
        let drift = self.zero_mean_common_factor_location(sigma_common, step_days)?;
        let centered = factor_value - drift;
        let radius = (delta * delta + centered * centered).sqrt();
        if !radius.is_finite() || radius <= 0.0 {
            return Ok(0.0);
        }
        let ar = self.common_factor.alpha * radius;
        let k1 = bessel_k1_f64(ar);
        if k1 <= 0.0 {
            return Ok(0.0);
        }
        let exponent = delta * self.common_factor.gamma + self.common_factor.beta * centered;
        let density = (self.common_factor.alpha * delta / core::f64::consts::PI)
            * (k1 / radius)
            * exponent.exp();
        Ok(density.max(0.0))
    }

    fn common_factor_weighted_nodes_with_rule(
        &self,
        sigma_common: f64,
        step_days: u32,
        gh_nodes: &[i128],
        gh_weights: &[i128],
    ) -> Result<Vec<WeightedFactorNode>, FactoredWorstOfError> {
        let std = self.common_factor_std(sigma_common, step_days)?;
        if std <= 0.0 {
            return Err(FactoredWorstOfError::DegenerateDensity);
        }
        let inv_sqrt_pi = INV_SQRT_PI as f64 / SCALE_I as f64;
        let mut normalizer = 0.0_f64;
        let mut nodes = Vec::with_capacity(gh_nodes.len());

        for index in 0..gh_nodes.len() {
            let node = gh_nodes[index] as f64 / SCALE_I as f64;
            let gh_weight = gh_weights[index] as f64 / SCALE_I as f64;
            let factor_value = SQRT_2 * std * node;
            let normal_pdf = (-0.5 * (factor_value / std).powi(2)).exp() / (std * SQRT_2PI);
            if !normal_pdf.is_finite() || normal_pdf <= 0.0 {
                return Err(FactoredWorstOfError::DegenerateDensity);
            }
            let nig_pdf = self.common_factor_pdf(sigma_common, step_days, factor_value)?;
            let weight = inv_sqrt_pi * gh_weight * nig_pdf / normal_pdf;
            nodes.push(WeightedFactorNode {
                value: factor_value,
                weight,
            });
            normalizer += weight;
        }

        if !normalizer.is_finite() || normalizer <= 0.0 {
            return Err(FactoredWorstOfError::DegenerateDensity);
        }
        for node in &mut nodes {
            node.weight /= normalizer;
        }
        Ok(nodes)
    }

    fn common_factor_weighted_nodes(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<Vec<WeightedFactorNode>, FactoredWorstOfError> {
        self.common_factor_weighted_nodes_with_rule(
            sigma_common,
            step_days,
            &GH13_NODES,
            &GH13_WEIGHTS,
        )
    }

    fn common_factor_weighted_nodes_with_order(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_order: u8,
    ) -> Result<Vec<WeightedFactorNode>, FactoredWorstOfError> {
        let (gh_nodes, gh_weights) = Self::factor_rule(factor_order)?;
        self.common_factor_weighted_nodes_with_rule(sigma_common, step_days, gh_nodes, gh_weights)
    }

    fn conditional_factor_nodes(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<Vec<ConditionalFactorNode>, FactoredWorstOfError> {
        let weighted_nodes = self.common_factor_weighted_nodes(sigma_common, step_days)?;
        let mut nodes = Vec::with_capacity(weighted_nodes.len());
        for node in weighted_nodes {
            let (mean, covariance) =
                self.conditional_uv_distribution(sigma_common, step_days, node.value)?;
            nodes.push(ConditionalFactorNode {
                value: node.value,
                weight: node.weight,
                mean,
                covariance,
            });
        }
        Ok(nodes)
    }

    /// Fast factor nodes using precomputed NIG importance weight tables.
    /// ~900 CU for all 9 weights (vs >1M for the Bessel K₁ path).
    /// Returns the count (for CU benchmarking without exposing private types).
    pub fn conditional_factor_nodes_fast_count(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<usize, FactoredWorstOfError> {
        Ok(self
            .conditional_factor_nodes_fast(sigma_common, step_days)?
            .len())
    }

    pub fn conditional_factor_nodes_fast(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<Vec<ConditionalFactorNode>, FactoredWorstOfError> {
        const S6: f64 = 1_000_000.0;
        let sigma_s6 = (sigma_common * S6) as i64;
        let weights = nig_importance_weights_9(sigma_s6);
        let std = self.common_factor_std(sigma_common, step_days)?;

        // Hoist: drifts and covariance are the same for all 9 nodes.
        let drifts = self.risk_neutral_step_drifts(sigma_common, step_days)?;
        let scale = step_days as f64;
        let covariance = [
            [
                self.residual_covariance_uv_daily[0][0] * scale,
                self.residual_covariance_uv_daily[0][1] * scale,
            ],
            [
                self.residual_covariance_uv_daily[1][0] * scale,
                self.residual_covariance_uv_daily[1][1] * scale,
            ],
        ];

        let mut nodes = Vec::with_capacity(9);
        for k in 0..9 {
            let z_k = GH9_NODES_S6[k] as f64 / S6;
            let factor_value = SQRT_2 * std * z_k;
            let weight = weights[k] as f64 / S6;
            // Only the mean depends on factor_value.
            let mean = [
                (drifts[1] - drifts[0]) + self.uv_factor_slope[0] * factor_value,
                (drifts[2] - drifts[0]) + self.uv_factor_slope[1] * factor_value,
            ];
            nodes.push(ConditionalFactorNode {
                value: factor_value,
                weight,
                mean,
                covariance,
            });
        }
        Ok(nodes)
    }

    fn conditional_factor_nodes_with_order(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_order: u8,
    ) -> Result<Vec<ConditionalFactorNode>, FactoredWorstOfError> {
        let weighted_nodes =
            self.common_factor_weighted_nodes_with_order(sigma_common, step_days, factor_order)?;
        let mut nodes = Vec::with_capacity(weighted_nodes.len());
        for node in weighted_nodes {
            let (mean, covariance) =
                self.conditional_uv_distribution(sigma_common, step_days, node.value)?;
            nodes.push(ConditionalFactorNode {
                value: node.value,
                weight: node.weight,
                mean,
                covariance,
            });
        }
        Ok(nodes)
    }

    fn common_factor_quadrature<F>(
        &self,
        sigma_common: f64,
        step_days: u32,
        mut integrand: F,
    ) -> Result<f64, FactoredWorstOfError>
    where
        F: FnMut(f64) -> Result<f64, FactoredWorstOfError>,
    {
        let nodes = self.common_factor_weighted_nodes(sigma_common, step_days)?;
        let mut total = 0.0_f64;
        for node in nodes {
            total += node.weight * integrand(node.value)?;
        }
        Ok(total)
    }

    fn marginal_autocall_probability(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<f64, FactoredWorstOfError> {
        self.common_factor_quadrature(sigma_common, step_days, |factor_value| {
            self.autocall_probability(sigma_common, step_days, factor_value)
        })
    }

    fn marginal_knock_in_safe_probability(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<f64, FactoredWorstOfError> {
        self.common_factor_quadrature(sigma_common, step_days, |factor_value| {
            self.knock_in_safe_probability(sigma_common, step_days, factor_value)
        })
    }

    fn marginal_coupon_probability(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<f64, FactoredWorstOfError> {
        let coupon_halfplanes = self.uniform_barrier_halfplanes(self.shell.coupon_barrier)?;
        self.common_factor_quadrature(sigma_common, step_days, |factor_value| {
            self.state_triangle_probability(
                coupon_halfplanes,
                sigma_common,
                step_days,
                factor_value,
                [0.0; 3],
            )
        })
    }

    fn marginal_ki_moment(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<(f64, f64), FactoredWorstOfError> {
        let nodes = self.common_factor_weighted_nodes(sigma_common, step_days)?;
        let mut ki_probability = 0.0_f64;
        let mut worst_indicator_expectation = 0.0_f64;
        for node in nodes {
            let (node_ki_probability, node_worst_indicator_expectation) =
                self.conditional_ki_moment(sigma_common, step_days, node.value)?;
            ki_probability += node.weight * node_ki_probability;
            worst_indicator_expectation += node.weight * node_worst_indicator_expectation;
        }
        Ok((
            ki_probability.clamp(0.0, 1.0),
            worst_indicator_expectation.max(0.0),
        ))
    }

    fn marginal_knocked_redemption_expectation_with_order(
        &self,
        sigma_common: f64,
        step_days: u32,
        ki_order: u8,
    ) -> Result<f64, FactoredWorstOfError> {
        let factor_nodes = self.conditional_factor_nodes_with_order(sigma_common, step_days, 13)?;
        self.state_marginal_knocked_redemption_expectation_with_order(
            sigma_common,
            step_days,
            &factor_nodes,
            [0.0; 3],
            ki_order,
        )
    }

    fn conditional_log_returns(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        u: f64,
        v: f64,
    ) -> Result<[f64; 3], FactoredWorstOfError> {
        let drifts = self.risk_neutral_step_drifts(sigma_common, step_days)?;
        let drift_shift = self.drift_shift(sigma_common, step_days)?;
        let total_loading = self.common_factor_loadings[0]
            + self.common_factor_loadings[1]
            + self.common_factor_loadings[2];
        if total_loading.abs() <= 1.0e-12 {
            return Err(FactoredWorstOfError::InvalidShape);
        }
        let x_spy = (factor_value
            - self.common_factor_loadings[1] * u
            - self.common_factor_loadings[2] * v
            + drift_shift)
            / total_loading;
        let x_qqq = x_spy + u;
        let x_iwm = x_spy + v;

        // Keep the explicit drift calculation in the path so the solver fails
        // fast if the risk-neutral step becomes ill-posed.
        let _ = drifts;
        Ok([x_spy, x_qqq, x_iwm])
    }

    fn step_outcomes(
        &self,
        sigma_common: f64,
        step_days: u32,
    ) -> Result<Vec<StepOutcome>, FactoredWorstOfError> {
        let factor_nodes = self.conditional_factor_nodes(sigma_common, step_days)?;
        let inv_sqrt_pi = INV_SQRT_PI as f64 / SCALE_I as f64;
        let mut outcomes = Vec::<StepOutcome>::with_capacity(
            factor_nodes.len() * GH5_NODES.len() * GH5_NODES.len(),
        );

        for factor_node in factor_nodes {
            let var_u = factor_node.covariance[0][0];
            let cov_uv = factor_node.covariance[0][1];
            let var_v = factor_node.covariance[1][1];
            if var_u <= 0.0 || var_v <= 0.0 {
                return Err(FactoredWorstOfError::InvalidCovariance);
            }
            let l11 = var_u.sqrt();
            let l21 = cov_uv / l11;
            let cond_var = var_v - l21 * l21;
            if cond_var <= 0.0 {
                return Err(FactoredWorstOfError::InvalidCovariance);
            }
            let l22 = cond_var.sqrt();

            for i in 0..GH5_NODES.len() {
                let z1 = GH5_NODES[i] as f64 / SCALE_I as f64;
                let w1 = GH5_WEIGHTS[i] as f64 / SCALE_I as f64;
                for j in 0..GH5_NODES.len() {
                    let z2 = GH5_NODES[j] as f64 / SCALE_I as f64;
                    let w2 = GH5_WEIGHTS[j] as f64 / SCALE_I as f64;
                    let u = factor_node.mean[0] + SQRT_2 * l11 * z1;
                    let v = factor_node.mean[1] + SQRT_2 * (l21 * z1 + l22 * z2);
                    let spread_weight = inv_sqrt_pi * w1 * inv_sqrt_pi * w2;
                    outcomes.push(StepOutcome {
                        weight: factor_node.weight * spread_weight,
                        log_return_increments: self.conditional_log_returns(
                            sigma_common,
                            step_days,
                            factor_node.value,
                            u,
                            v,
                        )?,
                    });
                }
            }
        }

        let total_weight = outcomes.iter().map(|outcome| outcome.weight).sum::<f64>();
        if !total_weight.is_finite() || total_weight <= 0.0 {
            return Err(FactoredWorstOfError::DegenerateDensity);
        }
        for outcome in &mut outcomes {
            outcome.weight /= total_weight;
        }
        outcomes.sort_by(|left, right| right.weight.total_cmp(&left.weight));
        Ok(outcomes)
    }

    fn state_triangle_probability(
        &self,
        base: [BarrierHalfPlane; 3],
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
    ) -> Result<f64, FactoredWorstOfError> {
        let (mean, covariance) =
            self.conditional_uv_distribution(sigma_common, step_days, factor_value)?;
        self.state_triangle_probability_with_distribution(
            base,
            sigma_common,
            step_days,
            factor_value,
            current_logs,
            mean,
            covariance,
        )
    }

    fn state_triangle_probability_with_distribution(
        &self,
        base: [BarrierHalfPlane; 3],
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
    ) -> Result<f64, FactoredWorstOfError> {
        let planes = self.state_shifted_halfplanes(
            base,
            sigma_common,
            step_days,
            factor_value,
            current_logs,
        )?;
        self.triangle_probability_explicit(mean, covariance, planes)
    }

    fn state_marginal_triangle_probability(
        &self,
        base: [BarrierHalfPlane; 3],
        sigma_common: f64,
        step_days: u32,
        factor_nodes: &[ConditionalFactorNode],
        current_logs: [f64; 3],
    ) -> Result<f64, FactoredWorstOfError> {
        let mut probability = 0.0_f64;
        for node in factor_nodes {
            probability += node.weight
                * self.state_triangle_probability_with_distribution(
                    base,
                    sigma_common,
                    step_days,
                    node.value,
                    current_logs,
                    node.mean,
                    node.covariance,
                )?;
        }
        Ok(probability.clamp(0.0, 1.0))
    }

    fn state_marginal_triangle_log_moment_with_order(
        &self,
        base: [BarrierHalfPlane; 3],
        sigma_common: f64,
        step_days: u32,
        factor_nodes: &[ConditionalFactorNode],
        current_logs: [f64; 3],
        triangle_gl_order: u8,
    ) -> Result<RegionLogMoment, FactoredWorstOfError> {
        let mut moment = RegionLogMoment::default();
        for node in factor_nodes {
            let planes = self.state_shifted_halfplanes(
                base,
                sigma_common,
                step_days,
                node.value,
                current_logs,
            )?;
            let (probability, expectation_u, expectation_v) = self
                .triangle_uv_moment_explicit_with_order(
                    node.mean,
                    node.covariance,
                    planes,
                    triangle_gl_order,
                )?;
            if probability <= STATE_MASS_EPS {
                continue;
            }
            let params = self.conditional_log_coordinate_params(
                sigma_common,
                step_days,
                node.value,
                current_logs,
            )?;
            moment.probability += node.weight * probability;
            for index in 0..3 {
                moment.log_indicator_expectation[index] += node.weight
                    * (params[index].0 * probability
                        + params[index].1 * expectation_u
                        + params[index].2 * expectation_v);
            }
        }
        moment.probability = moment.probability.clamp(0.0, 1.0);
        Ok(moment)
    }

    fn state_triangle_log_moment_for_node_with_order(
        &self,
        base: [BarrierHalfPlane; 3],
        sigma_common: f64,
        step_days: u32,
        node: ConditionalFactorNode,
        current_logs: [f64; 3],
        triangle_gl_order: u8,
    ) -> Result<RegionLogMoment, FactoredWorstOfError> {
        let planes =
            self.state_shifted_halfplanes(base, sigma_common, step_days, node.value, current_logs)?;
        let (probability, expectation_u, expectation_v) = self
            .triangle_uv_moment_explicit_with_order(
                node.mean,
                node.covariance,
                planes,
                triangle_gl_order,
            )?;
        if probability <= STATE_MASS_EPS {
            return Ok(RegionLogMoment::default());
        }
        let params = self.conditional_log_coordinate_params(
            sigma_common,
            step_days,
            node.value,
            current_logs,
        )?;
        let mut log_indicator_expectation = [0.0; 3];
        for index in 0..3 {
            log_indicator_expectation[index] = params[index].0 * probability
                + params[index].1 * expectation_u
                + params[index].2 * expectation_v;
        }
        Ok(RegionLogMoment {
            probability: probability.clamp(0.0, 1.0),
            log_indicator_expectation,
        })
    }

    fn unconditional_next_logs(
        &self,
        sigma_common: f64,
        step_days: u32,
        current_logs: [f64; 3],
    ) -> Result<[f64; 3], FactoredWorstOfError> {
        let drifts = self.risk_neutral_step_drifts(sigma_common, step_days)?;
        Ok([
            current_logs[0] + drifts[0],
            current_logs[1] + drifts[1],
            current_logs[2] + drifts[2],
        ])
    }

    fn conditional_expected_logs_for_node(
        &self,
        sigma_common: f64,
        step_days: u32,
        node: ConditionalFactorNode,
        current_logs: [f64; 3],
    ) -> Result<[f64; 3], FactoredWorstOfError> {
        let params = self.conditional_log_coordinate_params(
            sigma_common,
            step_days,
            node.value,
            current_logs,
        )?;
        Ok([
            params[0].0 + params[0].1 * node.mean[0] + params[0].2 * node.mean[1],
            params[1].0 + params[1].1 * node.mean[0] + params[1].2 * node.mean[1],
            params[2].0 + params[2].1 * node.mean[0] + params[2].2 * node.mean[1],
        ])
    }

    fn state_marginal_ki_moment_with_order(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_nodes: &[ConditionalFactorNode],
        current_logs: [f64; 3],
        ki_order: u8,
    ) -> Result<(f64, f64), FactoredWorstOfError> {
        let mut ki_probability = 0.0_f64;
        let mut worst_indicator_expectation = 0.0_f64;
        for node in factor_nodes {
            let (node_ki_probability, node_worst_indicator_expectation) = self
                .conditional_ki_moment_from_distribution_with_order(
                    sigma_common,
                    step_days,
                    node.value,
                    current_logs,
                    node.mean,
                    node.covariance,
                    ki_order,
                )?;
            ki_probability += node.weight * node_ki_probability;
            worst_indicator_expectation += node.weight * node_worst_indicator_expectation;
        }
        Ok((
            ki_probability.clamp(0.0, 1.0),
            worst_indicator_expectation.max(0.0),
        ))
    }

    fn conditional_knocked_redemption_expectation_from_distribution_with_order(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
        ki_order: u8,
    ) -> Result<f64, FactoredWorstOfError> {
        if ki_order == 0 {
            let (below_initial_probability, below_initial_redemption) = self
                .conditional_ki_moment_i64_gh3(
                    sigma_common,
                    step_days,
                    factor_value,
                    current_logs,
                    mean,
                    covariance,
                )?;
            return Ok((1.0 - below_initial_probability + below_initial_redemption).clamp(0.0, 1.0));
        }
        let barrier_log = Self::to_fixed(0.0)?;
        let moment = worst_of_ki_moment_with_order(
            Self::to_fixed(mean[0])?,
            Self::to_fixed(mean[1])?,
            Self::to_fixed(covariance[0][0])?,
            Self::to_fixed(covariance[0][1])?,
            Self::to_fixed(covariance[1][1])?,
            barrier_log,
            self.conditional_ki_affine_coordinates_from_state(
                sigma_common,
                step_days,
                factor_value,
                current_logs,
            )?,
            ki_order as usize,
        )?;
        let below_initial_probability =
            (moment.ki_probability as f64 / SCALE_I as f64).clamp(0.0, 1.0);
        let below_initial_redemption =
            (moment.worst_indicator_expectation as f64 / SCALE_I as f64).max(0.0);
        Ok((1.0_f64 - below_initial_probability + below_initial_redemption).clamp(0.0, 1.0))
    }

    fn state_marginal_knocked_redemption_expectation_with_order(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_nodes: &[ConditionalFactorNode],
        current_logs: [f64; 3],
        ki_order: u8,
    ) -> Result<f64, FactoredWorstOfError> {
        let mut redemption_expectation = 0.0_f64;
        for node in factor_nodes {
            redemption_expectation += node.weight
                * self.conditional_knocked_redemption_expectation_from_distribution_with_order(
                    sigma_common,
                    step_days,
                    node.value,
                    current_logs,
                    node.mean,
                    node.covariance,
                    ki_order,
                )?;
        }
        Ok(redemption_expectation.clamp(0.0, 1.0))
    }

    fn conditional_log_coordinate_params(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
    ) -> Result<[(f64, f64, f64); 3], FactoredWorstOfError> {
        if current_logs.iter().any(|value| !value.is_finite()) {
            return Err(FactoredWorstOfError::InvalidBarrierShift);
        }
        let drift_shift = self.drift_shift(sigma_common, step_days)?;
        let total_loading = self.total_loading()?;
        let common_constant = (factor_value + drift_shift) / total_loading;
        let spy_u = -self.common_factor_loadings[1] / total_loading;
        let spy_v = -self.common_factor_loadings[2] / total_loading;

        Ok([
            (current_logs[0] + common_constant, spy_u, spy_v),
            (current_logs[1] + common_constant, 1.0 + spy_u, spy_v),
            (current_logs[2] + common_constant, spy_u, 1.0 + spy_v),
        ])
    }

    fn conditional_ki_affine_coordinates_from_state(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
    ) -> Result<[AffineLogCoordinate; 3], FactoredWorstOfError> {
        let params = self.conditional_log_coordinate_params(
            sigma_common,
            step_days,
            factor_value,
            current_logs,
        )?;
        Ok([
            AffineLogCoordinate {
                constant: Self::to_fixed(params[0].0)?,
                u_coeff: Self::to_fixed(params[0].1)?,
                v_coeff: Self::to_fixed(params[0].2)?,
            },
            AffineLogCoordinate {
                constant: Self::to_fixed(params[1].0)?,
                u_coeff: Self::to_fixed(params[1].1)?,
                v_coeff: Self::to_fixed(params[1].2)?,
            },
            AffineLogCoordinate {
                constant: Self::to_fixed(params[2].0)?,
                u_coeff: Self::to_fixed(params[2].1)?,
                v_coeff: Self::to_fixed(params[2].2)?,
            },
        ])
    }

    /// Conditional KI moment in the Gaussian spread plane for one factor node.
    ///
    /// Returns:
    /// - `ki_probability = P(min(S_i/S_0) <= 0.80 | factor_value)`
    /// - `worst_indicator_expectation = E[min(S_i/S_0) * 1_{KI} | factor_value]`
    ///
    /// Error conditions:
    /// - invalid `sigma_common`
    /// - invalid factor-shape or covariance parameters
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn conditional_ki_moment(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
    ) -> Result<(f64, f64), FactoredWorstOfError> {
        self.conditional_ki_moment_from_state(sigma_common, step_days, factor_value, [0.0; 3])
    }

    fn conditional_ki_moment_from_state(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
    ) -> Result<(f64, f64), FactoredWorstOfError> {
        let (mean, covariance) =
            self.conditional_uv_distribution(sigma_common, step_days, factor_value)?;
        self.conditional_ki_moment_from_distribution(
            sigma_common,
            step_days,
            factor_value,
            current_logs,
            mean,
            covariance,
        )
    }

    fn conditional_ki_moment_from_distribution(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
    ) -> Result<(f64, f64), FactoredWorstOfError> {
        self.conditional_ki_moment_from_distribution_with_order(
            sigma_common,
            step_days,
            factor_value,
            current_logs,
            mean,
            covariance,
            13,
        )
    }

    fn conditional_ki_moment_i64_gh3(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
    ) -> Result<(f64, f64), FactoredWorstOfError> {
        const S6: f64 = 1_000_000.0;
        let params = self.conditional_log_coordinate_params(
            sigma_common,
            step_days,
            factor_value,
            current_logs,
        )?;
        let coords = [
            AffineCoord6 {
                constant: (params[0].0 * S6) as i64,
                u_coeff: (params[0].1 * S6) as i64,
                v_coeff: (params[0].2 * S6) as i64,
            },
            AffineCoord6 {
                constant: (params[1].0 * S6) as i64,
                u_coeff: (params[1].1 * S6) as i64,
                v_coeff: (params[1].2 * S6) as i64,
            },
            AffineCoord6 {
                constant: (params[2].0 * S6) as i64,
                u_coeff: (params[2].1 * S6) as i64,
                v_coeff: (params[2].2 * S6) as i64,
            },
        ];
        // Clamp eigenvalues: ensure positive-definite at SCALE_6 after truncation.
        // Conditional states can have near-zero variance from cancellation;
        // floor at 1e-5 (= 10 at S6) keeps Cholesky stable.
        let var_uu = covariance[0][0].max(1.0e-5);
        let var_vv = covariance[1][1].max(1.0e-5);
        let cov_bound = (var_uu * var_vv).sqrt() * 0.999;
        let cov_uv = covariance[0][1].clamp(-cov_bound, cov_bound);
        let (l11, l21, l22) = cholesky6(
            (var_uu * S6) as i64,
            (cov_uv * S6) as i64,
            (var_vv * S6) as i64,
        )
        .map_err(|e| FactoredWorstOfError::SolMath(e))?;
        let barrier = (self.shell.knock_in_barrier.ln() * S6) as i64;
        let m = ki_moment_i64_gh3(
            (mean[0] * S6) as i64,
            (mean[1] * S6) as i64,
            l11,
            l21,
            l22,
            barrier,
            coords,
        );
        Ok((
            (m.ki_probability as f64 / S6).clamp(0.0, 1.0),
            (m.worst_indicator as f64 / S6).max(0.0),
        ))
    }

    fn conditional_ki_moment_from_distribution_with_order(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
        ki_order: u8,
    ) -> Result<(f64, f64), FactoredWorstOfError> {
        if ki_order == 0 {
            return self.conditional_ki_moment_i64_gh3(
                sigma_common,
                step_days,
                factor_value,
                current_logs,
                mean,
                covariance,
            );
        }
        let ki_barrier_log = self.shell.knock_in_barrier.ln();
        let WorstOfKiMoment {
            ki_probability,
            worst_indicator_expectation,
        } = if ki_order == 13 {
            worst_of_ki_moment(
                Self::to_fixed(mean[0])?,
                Self::to_fixed(mean[1])?,
                Self::to_fixed(covariance[0][0])?,
                Self::to_fixed(covariance[0][1])?,
                Self::to_fixed(covariance[1][1])?,
                Self::to_fixed(ki_barrier_log)?,
                self.conditional_ki_affine_coordinates_from_state(
                    sigma_common,
                    step_days,
                    factor_value,
                    current_logs,
                )?,
            )?
        } else {
            worst_of_ki_moment_with_order(
                Self::to_fixed(mean[0])?,
                Self::to_fixed(mean[1])?,
                Self::to_fixed(covariance[0][0])?,
                Self::to_fixed(covariance[0][1])?,
                Self::to_fixed(covariance[1][1])?,
                Self::to_fixed(ki_barrier_log)?,
                self.conditional_ki_affine_coordinates_from_state(
                    sigma_common,
                    step_days,
                    factor_value,
                    current_logs,
                )?,
                ki_order as usize,
            )?
        };

        Ok((
            (ki_probability as f64 / SCALE_I as f64).clamp(0.0, 1.0),
            worst_indicator_expectation as f64 / SCALE_I as f64,
        ))
    }

    fn ki_min_region_halfplanes(
        params: &[(f64, f64, f64); 3],
        name_index: usize,
        barrier_log: f64,
    ) -> [BarrierHalfPlane; 3] {
        let (constant_i, u_i, v_i) = params[name_index];
        let mut planes = [BarrierHalfPlane {
            a_u: 0.0,
            a_v: 0.0,
            rhs: 0.0,
        }; 3];
        planes[0] = BarrierHalfPlane {
            a_u: u_i,
            a_v: v_i,
            rhs: barrier_log - constant_i,
        };
        let mut plane_index = 1usize;
        for other_index in 0..3 {
            if other_index == name_index {
                continue;
            }
            let (constant_j, u_j, v_j) = params[other_index];
            planes[plane_index] = BarrierHalfPlane {
                a_u: u_i - u_j,
                a_v: v_i - v_j,
                rhs: constant_j - constant_i,
            };
            plane_index += 1;
        }
        planes
    }

    fn conditional_ki_worst_indicator_via_partition_with_order(
        &self,
        sigma_common: f64,
        step_days: u32,
        factor_value: f64,
        current_logs: [f64; 3],
        mean: [f64; 2],
        covariance: [[f64; 2]; 2],
        ki_order: u8,
    ) -> Result<f64, FactoredWorstOfError> {
        let barrier_log = self.shell.knock_in_barrier.ln();
        let params = self.conditional_log_coordinate_params(
            sigma_common,
            step_days,
            factor_value,
            current_logs,
        )?;
        let triangle_gl_order = Self::ki_triangle_order(ki_order)?;
        let mut worst_indicator_expectation = 0.0_f64;

        for name_index in 0..3 {
            let (constant, u_coeff, v_coeff) = params[name_index];
            let shifted_mean = [
                mean[0] + covariance[0][0] * u_coeff + covariance[0][1] * v_coeff,
                mean[1] + covariance[0][1] * u_coeff + covariance[1][1] * v_coeff,
            ];
            let tilted_exponent = constant
                + u_coeff * mean[0]
                + v_coeff * mean[1]
                + 0.5
                    * (u_coeff * u_coeff * covariance[0][0]
                        + 2.0 * u_coeff * v_coeff * covariance[0][1]
                        + v_coeff * v_coeff * covariance[1][1]);
            let tilted_weight = tilted_exponent.exp();
            if !tilted_weight.is_finite() || tilted_weight < 0.0 {
                return Err(FactoredWorstOfError::InvalidShape);
            }

            let region_probability = self.triangle_probability_explicit_with_order(
                shifted_mean,
                covariance,
                Self::ki_min_region_halfplanes(&params, name_index, barrier_log),
                triangle_gl_order,
                None,
            )?;
            worst_indicator_expectation += tilted_weight * region_probability;
        }

        if !worst_indicator_expectation.is_finite() || worst_indicator_expectation < 0.0 {
            return Err(FactoredWorstOfError::InvalidShape);
        }
        Ok(worst_indicator_expectation.max(0.0))
    }

    fn quantize(value: f64, step: f64) -> i32 {
        (value / step).round() as i32
    }

    fn logs_feature_vector(&self, logs: [f64; 3]) -> [f64; 3] {
        [
            self.common_factor_loadings[0] * logs[0]
                + self.common_factor_loadings[1] * logs[1]
                + self.common_factor_loadings[2] * logs[2],
            logs[1] - logs[0],
            logs[2] - logs[0],
        ]
    }

    fn compress_live_state_class(
        &self,
        states: Vec<LiveState>,
        knocked: bool,
        max_components: usize,
    ) -> Vec<LiveState> {
        let mut filtered = states
            .into_iter()
            .filter(|state| {
                state.weight.is_finite()
                    && state.weight > STATE_MASS_EPS
                    && state.logs.iter().all(|value| value.is_finite())
            })
            .collect::<Vec<_>>();
        if max_components == 0 {
            return Vec::new();
        }
        if filtered.len() <= max_components {
            filtered.sort_by(|left, right| right.weight.total_cmp(&left.weight));
            return filtered;
        }

        let total_weight = filtered.iter().map(|state| state.weight).sum::<f64>();
        if !total_weight.is_finite() || total_weight <= STATE_MASS_EPS {
            return Vec::new();
        }

        let features = filtered
            .iter()
            .map(|state| self.logs_feature_vector(state.logs))
            .collect::<Vec<_>>();
        let mut order = (0..filtered.len()).collect::<Vec<_>>();
        order.sort_by(|left, right| features[*left][0].total_cmp(&features[*right][0]));

        let mut compressed = Vec::with_capacity(max_components);
        let mut start = 0usize;
        let mut cumulative = 0.0_f64;
        for cluster_index in 0..max_components {
            let target = if cluster_index + 1 == max_components {
                total_weight
            } else {
                total_weight * (cluster_index as f64 + 1.0) / max_components as f64
            };
            let mut end = start;
            while end < order.len() && cumulative < target - STATE_MASS_EPS {
                cumulative += filtered[order[end]].weight;
                end += 1;
            }
            if end <= start {
                continue;
            }

            let mut weight = 0.0_f64;
            let mut mean_logs = [0.0_f64; 3];
            for ordered_index in &order[start..end] {
                let state = filtered[*ordered_index];
                weight += state.weight;
                for axis in 0..3 {
                    mean_logs[axis] += state.weight * state.logs[axis];
                }
            }
            if weight <= STATE_MASS_EPS {
                start = end;
                continue;
            }
            for axis in 0..3 {
                mean_logs[axis] /= weight;
            }

            compressed.push(LiveState {
                weight,
                logs: mean_logs,
                knocked,
                missed_coupons: 0,
            });
            start = end;
            if start >= order.len() {
                break;
            }
        }
        compressed.sort_by(|left, right| right.weight.total_cmp(&left.weight));
        compressed
    }

    fn compress_gaussian_uv_state_class(
        &self,
        states: Vec<GaussianUvState>,
        knocked: bool,
        max_components: usize,
    ) -> Vec<GaussianUvState> {
        let mut filtered = states
            .into_iter()
            .filter(|state| {
                state.weight.is_finite()
                    && state.weight > STATE_MASS_EPS
                    && state.common_factor.is_finite()
                    && state.uv_mean.iter().all(|value| value.is_finite())
                    && state
                        .uv_covariance
                        .iter()
                        .flat_map(|row| row.iter())
                        .all(|value| value.is_finite())
            })
            .collect::<Vec<_>>();
        if max_components == 0 {
            return Vec::new();
        }
        if filtered.len() <= max_components {
            filtered.sort_by(|left, right| right.weight.total_cmp(&left.weight));
            return filtered;
        }

        let total_weight = filtered.iter().map(|state| state.weight).sum::<f64>();
        if !total_weight.is_finite() || total_weight <= STATE_MASS_EPS {
            return Vec::new();
        }

        let mut order = (0..filtered.len()).collect::<Vec<_>>();
        order.sort_by(|left, right| {
            filtered[*left]
                .common_factor
                .total_cmp(&filtered[*right].common_factor)
        });

        let mut compressed = Vec::with_capacity(max_components);
        let mut start = 0usize;
        let mut cumulative = 0.0_f64;
        for cluster_index in 0..max_components {
            let target = if cluster_index + 1 == max_components {
                total_weight
            } else {
                total_weight * (cluster_index as f64 + 1.0) / max_components as f64
            };
            let mut end = start;
            while end < order.len() && cumulative < target - STATE_MASS_EPS {
                cumulative += filtered[order[end]].weight;
                end += 1;
            }
            if end <= start {
                continue;
            }

            let mut weight = 0.0_f64;
            let mut common_factor = 0.0_f64;
            let mut uv_mean = [0.0_f64; 2];
            for ordered_index in &order[start..end] {
                let state = filtered[*ordered_index];
                weight += state.weight;
                common_factor += state.weight * state.common_factor;
                uv_mean[0] += state.weight * state.uv_mean[0];
                uv_mean[1] += state.weight * state.uv_mean[1];
            }
            if weight <= STATE_MASS_EPS {
                start = end;
                continue;
            }
            common_factor /= weight;
            uv_mean[0] /= weight;
            uv_mean[1] /= weight;

            let mut uv_covariance = [[0.0_f64; 2]; 2];
            for ordered_index in &order[start..end] {
                let state = filtered[*ordered_index];
                let delta_u = state.uv_mean[0] - uv_mean[0];
                let delta_v = state.uv_mean[1] - uv_mean[1];
                uv_covariance[0][0] +=
                    state.weight * (state.uv_covariance[0][0] + delta_u * delta_u);
                uv_covariance[0][1] +=
                    state.weight * (state.uv_covariance[0][1] + delta_u * delta_v);
                uv_covariance[1][0] +=
                    state.weight * (state.uv_covariance[1][0] + delta_v * delta_u);
                uv_covariance[1][1] +=
                    state.weight * (state.uv_covariance[1][1] + delta_v * delta_v);
            }
            for row in 0..2 {
                for col in 0..2 {
                    uv_covariance[row][col] /= weight;
                }
            }
            // Floor must survive f64 → i64 SCALE_6 conversion (see conditional_distribution).
            uv_covariance[0][0] = uv_covariance[0][0].max(1.0e-5);
            uv_covariance[1][1] = uv_covariance[1][1].max(1.0e-5);
            let cov_bound = (uv_covariance[0][0] * uv_covariance[1][1]).sqrt();
            let clipped_cov = uv_covariance[0][1].clamp(-cov_bound, cov_bound);
            uv_covariance[0][1] = clipped_cov;
            uv_covariance[1][0] = clipped_cov;

            compressed.push(GaussianUvState {
                weight,
                common_factor,
                uv_mean,
                uv_covariance,
                knocked,
            });
            start = end;
            if start >= order.len() {
                break;
            }
        }

        compressed.sort_by(|left, right| right.weight.total_cmp(&left.weight));
        compressed
    }

    fn record_onchain_v1_survivor_moments(
        capture: &mut OnchainV1SurvivorMomentCapture,
        observation_index: usize,
        factor_nodes: &[ConditionalFactorNode],
        safe_states: &[GaussianUvState],
        knocked_states: &[GaussianUvState],
    ) {
        if observation_index >= 6 || factor_nodes.len() != 9 {
            return;
        }
        let safe_mass = safe_states.iter().map(|state| state.weight).sum::<f64>();
        let knocked_mass = knocked_states.iter().map(|state| state.weight).sum::<f64>();

        if safe_mass.is_finite() && safe_mass > STATE_MASS_EPS {
            capture.common_factor_safe[observation_index] = safe_states
                .iter()
                .map(|state| state.weight * state.common_factor)
                .sum::<f64>()
                / safe_mass;
            for (node_index, node) in factor_nodes.iter().enumerate() {
                let mut eu = 0.0_f64;
                let mut ev = 0.0_f64;
                for state in safe_states {
                    eu += state.weight * (state.uv_mean[0] + node.mean[0]);
                    ev += state.weight * (state.uv_mean[1] + node.mean[1]);
                }
                capture.expectation_u_safe[observation_index][node_index] = eu / safe_mass;
                capture.expectation_v_safe[observation_index][node_index] = ev / safe_mass;
            }
        }

        if knocked_mass.is_finite() && knocked_mass > STATE_MASS_EPS {
            capture.common_factor_knocked[observation_index] = knocked_states
                .iter()
                .map(|state| state.weight * state.common_factor)
                .sum::<f64>()
                / knocked_mass;
            for (node_index, node) in factor_nodes.iter().enumerate() {
                let mut eu = 0.0_f64;
                let mut ev = 0.0_f64;
                for state in knocked_states {
                    eu += state.weight * (state.uv_mean[0] + node.mean[0]);
                    ev += state.weight * (state.uv_mean[1] + node.mean[1]);
                }
                capture.expectation_u_knocked[observation_index][node_index] = eu / knocked_mass;
                capture.expectation_v_knocked[observation_index][node_index] = ev / knocked_mass;
            }
        }
    }

    fn state_key(logs: [f64; 3], knocked: bool, missed_coupons: u8) -> StateKey {
        StateKey {
            spy_bucket: Self::quantize(logs[0], SPY_LOG_BUCKET),
            spread_u_bucket: Self::quantize(logs[1] - logs[0], SPREAD_LOG_BUCKET),
            spread_v_bucket: Self::quantize(logs[2] - logs[0], SPREAD_LOG_BUCKET),
            knocked,
            missed_coupons,
        }
    }

    fn pre_maturity_state_probabilities(
        &self,
        sigma_common: f64,
        step_days: u32,
        autocall_coupon_halfplanes: [BarrierHalfPlane; 3],
        factor_nodes: &[ConditionalFactorNode],
        current_logs: [f64; 3],
        knocked: bool,
    ) -> Result<PreMaturityStateProbabilities, FactoredWorstOfError> {
        let autocall_probability = self.state_marginal_triangle_probability(
            self.autocall_halfplanes,
            sigma_common,
            step_days,
            factor_nodes,
            current_logs,
        )?;
        let autocall_coupon_probability = self
            .state_marginal_triangle_probability(
                autocall_coupon_halfplanes,
                sigma_common,
                step_days,
                factor_nodes,
                current_logs,
            )?
            .clamp(0.0, autocall_probability);

        if knocked {
            return Ok(PreMaturityStateProbabilities {
                autocall_probability,
                autocall_coupon_probability,
                safe_survival_probability: 0.0,
                knocked_survival_probability: (1.0 - autocall_probability).clamp(0.0, 1.0),
                first_knock_in_probability: 0.0,
            });
        }

        let knock_in_safe_probability = self.state_marginal_triangle_probability(
            self.knock_in_safe_halfplanes,
            sigma_common,
            step_days,
            factor_nodes,
            current_logs,
        )?;
        let safe_survival_probability =
            (knock_in_safe_probability - autocall_probability).clamp(0.0, 1.0);
        let knocked_survival_probability = (1.0 - knock_in_safe_probability).clamp(0.0, 1.0);

        Ok(PreMaturityStateProbabilities {
            autocall_probability,
            autocall_coupon_probability,
            safe_survival_probability,
            knocked_survival_probability,
            first_knock_in_probability: knocked_survival_probability,
        })
    }

    fn advance_deterministic_checkpoint_internal(
        &self,
        checkpoint: FactoredWorstOfCheckpoint,
        max_observations: usize,
    ) -> Result<(TraceBuildResult, FactoredWorstOfCheckpoint), FactoredWorstOfError> {
        let completed_observations = checkpoint.completed_observations as usize;
        if completed_observations > self.shell.observation_days.len() {
            return Err(FactoredWorstOfError::InvalidCheckpoint);
        }
        let sigma_common = checkpoint.sigma_common;
        let max_observations = max_observations.min(self.shell.observation_days.len());
        if completed_observations > max_observations {
            return Err(FactoredWorstOfError::InvalidCheckpoint);
        }

        let mut accumulator = Self::accumulator_from_checkpoint(&checkpoint)?;
        let mut live_states = Self::live_states_from_checkpoint(&checkpoint)?;
        let mut peak_live_state_count =
            usize::max(checkpoint.peak_live_state_count as usize, live_states.len());
        let mut previous_day = if completed_observations == 0 {
            0_u32
        } else {
            self.shell.observation_days[completed_observations - 1]
        };

        for observation_index in completed_observations..max_observations {
            let observation_day = self.shell.observation_days[observation_index];
            let step_days = observation_day - previous_day;
            previous_day = observation_day;
            let outcomes = self.step_outcomes(sigma_common, step_days)?;
            let is_maturity = observation_index + 1 == self.shell.observation_days.len();
            let mut next_buckets = HashMap::<StateKey, BucketAccumulator>::new();
            accumulator.observation_survival_probability[observation_index] =
                live_states.iter().map(|state| state.weight).sum::<f64>();

            for state in &live_states {
                for outcome in &outcomes {
                    if !outcome.weight.is_finite() || outcome.weight <= 0.0 {
                        continue;
                    }
                    let branch_weight = state.weight * outcome.weight;
                    if !branch_weight.is_finite() || branch_weight <= 0.0 {
                        continue;
                    }
                    if branch_weight < PATH_WEIGHT_CUTOFF {
                        break;
                    }

                    let next_logs = [
                        state.logs[0] + outcome.log_return_increments[0],
                        state.logs[1] + outcome.log_return_increments[1],
                        state.logs[2] + outcome.log_return_increments[2],
                    ];
                    let levels = [next_logs[0].exp(), next_logs[1].exp(), next_logs[2].exp()];
                    let worst = levels[0].min(levels[1]).min(levels[2]);
                    let knocked_next = state.knocked || worst <= self.shell.knock_in_barrier;
                    let coupon_due = worst >= self.shell.coupon_barrier;
                    if !state.knocked && knocked_next {
                        accumulator.observation_first_knock_in_probability[observation_index] +=
                            branch_weight;
                    }
                    let coupon_multiplier = if coupon_due {
                        f64::from(state.missed_coupons + 1)
                    } else {
                        0.0
                    };
                    let all_above_autocall = levels
                        .iter()
                        .all(|level| *level >= self.shell.autocall_barrier);

                    if !is_maturity && all_above_autocall {
                        accumulator.redemption_leg_pv += branch_weight * self.shell.notional;
                        accumulator.coupon_annuity_pv += branch_weight * coupon_multiplier;
                        accumulator.observation_autocall_first_hit_probability
                            [observation_index] += branch_weight;
                        accumulator.observation_coupon_annuity_contribution[observation_index] +=
                            branch_weight * coupon_multiplier;
                        accumulator.observation_autocall_redemption_pv_contribution
                            [observation_index] += branch_weight * self.shell.notional;
                        accumulator.expected_life_days += branch_weight * observation_day as f64;
                        continue;
                    }

                    if is_maturity {
                        let redemption = if knocked_next {
                            self.shell.notional * worst
                        } else {
                            self.shell.notional
                        };
                        accumulator.redemption_leg_pv += branch_weight * redemption;
                        accumulator.coupon_annuity_pv += branch_weight * coupon_multiplier;
                        accumulator.observation_coupon_annuity_contribution[observation_index] +=
                            branch_weight * coupon_multiplier;
                        accumulator.maturity_redemption_pv += branch_weight * redemption;
                        if knocked_next {
                            accumulator.maturity_knock_in_redemption_pv +=
                                branch_weight * redemption;
                        }
                        accumulator.expected_life_days += branch_weight * observation_day as f64;
                        continue;
                    }

                    let missed_next = if coupon_due {
                        0
                    } else {
                        state.missed_coupons + 1
                    };
                    let key = Self::state_key(next_logs, knocked_next, missed_next);
                    let next_bucket = next_buckets.entry(key).or_insert(BucketAccumulator {
                        weight: 0.0,
                        weighted_logs: [0.0; 3],
                    });
                    next_bucket.weight += branch_weight;
                    next_bucket.weighted_logs[0] += branch_weight * next_logs[0];
                    next_bucket.weighted_logs[1] += branch_weight * next_logs[1];
                    next_bucket.weighted_logs[2] += branch_weight * next_logs[2];
                }
            }

            let mut ordered_buckets = next_buckets
                .into_iter()
                .map(|(key, bucket)| (key, bucket))
                .collect::<Vec<_>>();
            ordered_buckets.sort_by(|(left_key, left_bucket), (right_key, right_bucket)| {
                right_bucket
                    .weight
                    .total_cmp(&left_bucket.weight)
                    .then_with(|| left_key.cmp(right_key))
            });
            live_states = ordered_buckets
                .into_iter()
                .map(|(key, bucket)| LiveState {
                    weight: bucket.weight,
                    logs: [
                        bucket.weighted_logs[0] / bucket.weight,
                        bucket.weighted_logs[1] / bucket.weight,
                        bucket.weighted_logs[2] / bucket.weight,
                    ],
                    knocked: key.knocked,
                    missed_coupons: key.missed_coupons,
                })
                .collect();
            if live_states.len() > MAX_LIVE_STATES {
                live_states.truncate(MAX_LIVE_STATES);
            }
            peak_live_state_count = peak_live_state_count.max(live_states.len());
        }

        let result = self.trace_build_result_from_state(
            max_observations,
            accumulator,
            &live_states,
            peak_live_state_count,
        );
        let checkpoint = self.checkpoint_from_internal(
            sigma_common,
            max_observations,
            peak_live_state_count,
            LegTraceAccumulator {
                redemption_leg_pv: result.trace.redemption_leg_pv,
                coupon_annuity_pv: result.trace.coupon_annuity_pv,
                expected_life_days: result.trace.expected_life_days,
                observation_survival_probability: result.trace.observation_survival_probability,
                observation_autocall_first_hit_probability: result
                    .trace
                    .observation_autocall_first_hit_probability,
                observation_first_knock_in_probability: result
                    .trace
                    .observation_first_knock_in_probability,
                observation_coupon_annuity_contribution: result
                    .trace
                    .observation_coupon_annuity_contribution,
                observation_autocall_redemption_pv_contribution: result
                    .trace
                    .observation_autocall_redemption_pv_contribution,
                maturity_redemption_pv: result.trace.maturity_redemption_pv,
                maturity_knock_in_redemption_pv: result.trace.maturity_knock_in_redemption_pv,
            },
            &live_states,
        );
        Ok((result, checkpoint))
    }

    fn build_deterministic_leg_trace(
        &self,
        sigma_common: f64,
        max_observations: usize,
    ) -> Result<TraceBuildResult, FactoredWorstOfError> {
        let checkpoint = self.initial_checkpoint(sigma_common)?;
        Ok(self
            .advance_deterministic_checkpoint_internal(checkpoint, max_observations)?
            .0)
    }

    fn deterministic_leg_trace(&self, sigma_common: f64) -> Result<LegTrace, FactoredWorstOfError> {
        Ok(self
            .build_deterministic_leg_trace(sigma_common, self.shell.observation_days.len())?
            .trace)
    }

    fn build_observation_marginals(
        &self,
        sigma_common: f64,
    ) -> Result<[ObservationMarginal; 6], FactoredWorstOfError> {
        let mut observation_marginals = [ObservationMarginal {
            observation_day: 0,
            autocall_probability: 0.0,
            coupon_probability: 0.0,
            knock_in_safe_probability: 0.0,
            ki_probability: 0.0,
            ki_worst_indicator_expectation: 0.0,
            knocked_redemption_expectation: 0.0,
            survival_probability: 0.0,
            autocall_first_hit_probability: 0.0,
            first_knock_in_probability: 0.0,
            coupon_annuity_contribution: 0.0,
            autocall_redemption_pv_contribution: 0.0,
        }; 6];

        for (index, observation_day) in self.shell.observation_days.iter().copied().enumerate() {
            let autocall_probability =
                self.marginal_autocall_probability(sigma_common, observation_day)?;
            let coupon_probability =
                self.marginal_coupon_probability(sigma_common, observation_day)?;
            let knock_in_safe_probability =
                self.marginal_knock_in_safe_probability(sigma_common, observation_day)?;
            let (ki_probability, ki_worst_indicator_expectation) =
                self.marginal_ki_moment(sigma_common, observation_day)?;
            let knocked_redemption_expectation = self
                .marginal_knocked_redemption_expectation_with_order(
                    sigma_common,
                    observation_day,
                    13,
                )?;

            observation_marginals[index] = ObservationMarginal {
                observation_day,
                autocall_probability,
                coupon_probability,
                knock_in_safe_probability,
                ki_probability,
                ki_worst_indicator_expectation,
                knocked_redemption_expectation,
                survival_probability: 0.0,
                autocall_first_hit_probability: 0.0,
                first_knock_in_probability: 0.0,
                coupon_annuity_contribution: 0.0,
                autocall_redemption_pv_contribution: 0.0,
            };
        }

        Ok(observation_marginals)
    }

    fn build_observation_marginals_onchain_v1(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
    ) -> Result<[ObservationMarginal; 6], FactoredWorstOfError> {
        Self::validate_onchain_config(config)?;
        let mut observation_marginals = [ObservationMarginal {
            observation_day: 0,
            autocall_probability: 0.0,
            coupon_probability: 0.0,
            knock_in_safe_probability: 0.0,
            ki_probability: 0.0,
            ki_worst_indicator_expectation: 0.0,
            knocked_redemption_expectation: 0.0,
            survival_probability: 0.0,
            autocall_first_hit_probability: 0.0,
            first_knock_in_probability: 0.0,
            coupon_annuity_contribution: 0.0,
            autocall_redemption_pv_contribution: 0.0,
        }; 6];
        for (index, observation_day) in self.shell.observation_days.iter().copied().enumerate() {
            observation_marginals[index] = self.build_observation_marginal_onchain_v1_for_day(
                sigma_common,
                observation_day,
                config,
            )?;
        }

        Ok(observation_marginals)
    }

    fn build_observation_marginal_onchain_v1_for_day(
        &self,
        sigma_common: f64,
        observation_day: u32,
        config: FactoredWorstOfOnchainConfig,
    ) -> Result<ObservationMarginal, FactoredWorstOfError> {
        let weighted_nodes = self.common_factor_weighted_nodes_with_order(
            sigma_common,
            observation_day,
            config.factor_order,
        )?;
        let coupon_halfplanes = self.uniform_barrier_halfplanes(self.shell.coupon_barrier)?;
        let mut autocall_probability = 0.0_f64;
        let mut coupon_probability = 0.0_f64;
        let mut knock_in_safe_probability = 0.0_f64;
        let mut ki_probability = 0.0_f64;
        let mut ki_worst_indicator_expectation = 0.0_f64;
        let mut knocked_redemption_expectation = 0.0_f64;

        for node in weighted_nodes {
            let (mean, covariance) =
                self.conditional_uv_distribution(sigma_common, observation_day, node.value)?;
            let shifted_autocall = self.shifted_halfplanes(
                self.autocall_halfplanes,
                sigma_common,
                observation_day,
                node.value,
            )?;
            autocall_probability += node.weight
                * self.triangle_probability_explicit_with_order(
                    mean,
                    covariance,
                    shifted_autocall,
                    config.triangle_gl_order,
                    None,
                )?;
            let shifted_coupon = self.shifted_halfplanes(
                coupon_halfplanes,
                sigma_common,
                observation_day,
                node.value,
            )?;
            coupon_probability += node.weight
                * self.triangle_probability_explicit_with_order(
                    mean,
                    covariance,
                    shifted_coupon,
                    config.triangle_gl_order,
                    None,
                )?;

            let shifted_ki_safe = self.shifted_halfplanes(
                self.knock_in_safe_halfplanes,
                sigma_common,
                observation_day,
                node.value,
            )?;
            let node_knock_in_safe_probability = self.triangle_probability_explicit_with_order(
                mean,
                covariance,
                shifted_ki_safe,
                config.triangle_gl_order,
                None,
            )?;
            knock_in_safe_probability += node.weight * node_knock_in_safe_probability;
            let node_ki_probability = (1.0 - node_knock_in_safe_probability).clamp(0.0, 1.0);

            ki_probability += node.weight * node_ki_probability;
            ki_worst_indicator_expectation += node.weight
                * self.conditional_ki_worst_indicator_via_partition_with_order(
                    sigma_common,
                    observation_day,
                    node.value,
                    [0.0; 3],
                    mean,
                    covariance,
                    config.ki_order,
                )?;
            knocked_redemption_expectation += node.weight
                * self.conditional_knocked_redemption_expectation_from_distribution_with_order(
                    sigma_common,
                    observation_day,
                    node.value,
                    [0.0; 3],
                    mean,
                    covariance,
                    config.ki_order,
                )?;
        }

        Ok(ObservationMarginal {
            observation_day,
            autocall_probability: autocall_probability.clamp(0.0, 1.0),
            coupon_probability: coupon_probability.clamp(0.0, 1.0),
            knock_in_safe_probability: knock_in_safe_probability.clamp(0.0, 1.0),
            ki_probability: ki_probability.clamp(0.0, 1.0),
            ki_worst_indicator_expectation: ki_worst_indicator_expectation.max(0.0),
            knocked_redemption_expectation: knocked_redemption_expectation.clamp(0.0, 1.0),
            survival_probability: 0.0,
            autocall_first_hit_probability: 0.0,
            first_knock_in_probability: 0.0,
            coupon_annuity_contribution: 0.0,
            autocall_redemption_pv_contribution: 0.0,
        })
    }

    fn build_onchain_v1_trace_from_marginals(
        &self,
        observation_marginals: &mut [ObservationMarginal; 6],
        max_observations: usize,
    ) -> TraceBuildResult {
        let max_observations = max_observations.min(self.shell.observation_days.len());
        let mut accumulator = LegTraceAccumulator::default();
        let mut safe_survival = 1.0_f64;
        let mut knocked_survival = 0.0_f64;
        let mut peak_live_state_count = 1usize;

        for observation_index in 0..max_observations {
            let is_maturity = observation_index + 1 == self.shell.observation_days.len();
            let observation_day = self.shell.observation_days[observation_index] as f64;
            let marginal = &mut observation_marginals[observation_index];
            let survival_mass = (safe_survival + knocked_survival).clamp(0.0, 1.0);
            accumulator.observation_survival_probability[observation_index] = survival_mass;
            marginal.survival_probability = survival_mass;

            let autocall_probability = marginal.autocall_probability.clamp(0.0, 1.0);
            let coupon_probability = marginal.coupon_probability.clamp(0.0, 1.0);
            let knock_in_safe_probability = marginal
                .knock_in_safe_probability
                .clamp(autocall_probability, 1.0);
            let ki_probability = marginal.ki_probability.clamp(0.0, 1.0);
            let first_knock_in_probability = ki_probability;
            let knocked_redemption_expectation =
                marginal.knocked_redemption_expectation.clamp(0.0, 1.0);

            let safe_autocall = safe_survival * autocall_probability;
            let knocked_autocall = knocked_survival * autocall_probability;
            let first_hit = (safe_autocall + knocked_autocall).clamp(0.0, survival_mass);
            let safe_continue =
                safe_survival * (knock_in_safe_probability - autocall_probability).max(0.0);
            let first_knock_in = safe_survival * first_knock_in_probability;
            let knocked_continue = knocked_survival * (1.0 - autocall_probability).max(0.0);

            if is_maturity {
                let maturity_coupon_hit =
                    (safe_survival + knocked_survival) * coupon_probability.clamp(0.0, 1.0);
                let maturity_safe_principal = (safe_survival * knock_in_safe_probability).max(0.0);
                let maturity_knock_in_redemption = safe_survival
                    * marginal.ki_worst_indicator_expectation
                    + knocked_survival * knocked_redemption_expectation;
                let coupon_count = (observation_index + 1) as f64;

                accumulator.redemption_leg_pv +=
                    self.shell.notional * (maturity_safe_principal + maturity_knock_in_redemption);
                accumulator.coupon_annuity_pv += coupon_count * maturity_coupon_hit;
                accumulator.maturity_redemption_pv +=
                    self.shell.notional * (maturity_safe_principal + maturity_knock_in_redemption);
                accumulator.maturity_knock_in_redemption_pv +=
                    self.shell.notional * maturity_knock_in_redemption;
                accumulator.observation_coupon_annuity_contribution[observation_index] =
                    coupon_count * maturity_coupon_hit;
                accumulator.observation_first_knock_in_probability[observation_index] =
                    first_knock_in;
                accumulator.expected_life_days += observation_day * survival_mass;

                marginal.first_knock_in_probability = first_knock_in;
                marginal.coupon_annuity_contribution = coupon_count * maturity_coupon_hit;

                safe_survival = 0.0;
                knocked_survival = 0.0;
                peak_live_state_count = peak_live_state_count.max(0);
                continue;
            }

            let coupon_count = (observation_index + 1) as f64;
            accumulator.redemption_leg_pv += self.shell.notional * first_hit;
            accumulator.coupon_annuity_pv += coupon_count * first_hit;
            accumulator.observation_autocall_first_hit_probability[observation_index] = first_hit;
            accumulator.observation_first_knock_in_probability[observation_index] = first_knock_in;
            accumulator.observation_coupon_annuity_contribution[observation_index] =
                coupon_count * first_hit;
            accumulator.observation_autocall_redemption_pv_contribution[observation_index] =
                self.shell.notional * first_hit;
            accumulator.expected_life_days += observation_day * first_hit;

            marginal.autocall_first_hit_probability = first_hit;
            marginal.first_knock_in_probability = first_knock_in;
            marginal.coupon_annuity_contribution = coupon_count * first_hit;
            marginal.autocall_redemption_pv_contribution = self.shell.notional * first_hit;

            safe_survival = safe_continue.max(0.0);
            knocked_survival = (knocked_continue + first_knock_in).max(0.0);
            let live_state_count = usize::from(safe_survival > STATE_MASS_EPS)
                + usize::from(knocked_survival > STATE_MASS_EPS);
            peak_live_state_count = peak_live_state_count.max(live_state_count);
        }

        let knock_in_rate = accumulator
            .observation_first_knock_in_probability
            .into_iter()
            .sum::<f64>()
            .clamp(0.0, 1.0);
        let autocall_rate = accumulator
            .observation_autocall_first_hit_probability
            .into_iter()
            .sum::<f64>()
            .clamp(0.0, 1.0);
        let live_state_count = usize::from(safe_survival > STATE_MASS_EPS)
            + usize::from(knocked_survival > STATE_MASS_EPS);
        let terminalized = max_observations == self.shell.observation_days.len();
        let live_probability_mass = if terminalized {
            0.0
        } else {
            (safe_survival + knocked_survival).clamp(0.0, 1.0)
        };

        TraceBuildResult {
            trace: LegTrace {
                redemption_leg_pv: accumulator.redemption_leg_pv,
                coupon_annuity_pv: accumulator.coupon_annuity_pv,
                expected_life_days: accumulator.expected_life_days,
                knock_in_rate,
                autocall_rate,
                observation_survival_probability: accumulator.observation_survival_probability,
                observation_autocall_first_hit_probability: accumulator
                    .observation_autocall_first_hit_probability,
                observation_first_knock_in_probability: accumulator
                    .observation_first_knock_in_probability,
                observation_coupon_annuity_contribution: accumulator
                    .observation_coupon_annuity_contribution,
                observation_autocall_redemption_pv_contribution: accumulator
                    .observation_autocall_redemption_pv_contribution,
                maturity_redemption_pv: accumulator.maturity_redemption_pv,
                maturity_knock_in_redemption_pv: accumulator.maturity_knock_in_redemption_pv,
            },
            completed_observations: max_observations,
            terminalized,
            live_state_count,
            peak_live_state_count,
            live_probability_mass,
        }
    }

    fn build_onchain_v1_scalar_trace(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
        max_observations: usize,
    ) -> Result<([ObservationMarginal; 6], TraceBuildResult), FactoredWorstOfError> {
        Self::validate_onchain_config(config)?;
        let max_observations = max_observations.min(self.shell.observation_days.len());
        let mut observation_marginals = [ObservationMarginal {
            observation_day: 0,
            autocall_probability: 0.0,
            coupon_probability: 0.0,
            knock_in_safe_probability: 0.0,
            ki_probability: 0.0,
            ki_worst_indicator_expectation: 0.0,
            knocked_redemption_expectation: 0.0,
            survival_probability: 0.0,
            autocall_first_hit_probability: 0.0,
            first_knock_in_probability: 0.0,
            coupon_annuity_contribution: 0.0,
            autocall_redemption_pv_contribution: 0.0,
        }; 6];
        for (index, observation_day) in self
            .shell
            .observation_days
            .iter()
            .copied()
            .take(max_observations)
            .enumerate()
        {
            observation_marginals[index] = self.build_observation_marginal_onchain_v1_for_day(
                sigma_common,
                observation_day,
                config,
            )?;
        }
        let result = self
            .build_onchain_v1_trace_from_marginals(&mut observation_marginals, max_observations);
        Ok((observation_marginals, result))
    }

    fn build_onchain_v1_trace_with_continuation_internal(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
        max_observations: usize,
        mut moment_capture: Option<&mut OnchainV1SurvivorMomentCapture>,
        mut replay_capture: Option<&mut OnchainV1ReplayCapture>,
    ) -> Result<([ObservationMarginal; 6], TraceBuildResult), FactoredWorstOfError> {
        let max_observations = max_observations.min(self.shell.observation_days.len());
        if max_observations == 0 {
            return Ok((
                [ObservationMarginal {
                    observation_day: 0,
                    autocall_probability: 0.0,
                    coupon_probability: 0.0,
                    knock_in_safe_probability: 0.0,
                    ki_probability: 0.0,
                    ki_worst_indicator_expectation: 0.0,
                    knocked_redemption_expectation: 0.0,
                    survival_probability: 0.0,
                    autocall_first_hit_probability: 0.0,
                    first_knock_in_probability: 0.0,
                    coupon_annuity_contribution: 0.0,
                    autocall_redemption_pv_contribution: 0.0,
                }; 6],
                TraceBuildResult {
                    trace: LegTrace {
                        redemption_leg_pv: 0.0,
                        coupon_annuity_pv: 0.0,
                        expected_life_days: 0.0,
                        knock_in_rate: 0.0,
                        autocall_rate: 0.0,
                        observation_survival_probability: [0.0; 6],
                        observation_autocall_first_hit_probability: [0.0; 6],
                        observation_first_knock_in_probability: [0.0; 6],
                        observation_coupon_annuity_contribution: [0.0; 6],
                        observation_autocall_redemption_pv_contribution: [0.0; 6],
                        maturity_redemption_pv: 0.0,
                        maturity_knock_in_redemption_pv: 0.0,
                    },
                    completed_observations: 0,
                    terminalized: false,
                    live_state_count: 1,
                    peak_live_state_count: 1,
                    live_probability_mass: 1.0,
                },
            ));
        }
        let mut observation_marginals = [ObservationMarginal {
            observation_day: 0,
            autocall_probability: 0.0,
            coupon_probability: 0.0,
            knock_in_safe_probability: 0.0,
            ki_probability: 0.0,
            ki_worst_indicator_expectation: 0.0,
            knocked_redemption_expectation: 0.0,
            survival_probability: 0.0,
            autocall_first_hit_probability: 0.0,
            first_knock_in_probability: 0.0,
            coupon_annuity_contribution: 0.0,
            autocall_redemption_pv_contribution: 0.0,
        }; 6];
        let coupon_halfplanes = self.uniform_barrier_halfplanes(self.shell.coupon_barrier)?;
        let mut accumulator = LegTraceAccumulator::default();
        let mut safe_states = vec![GaussianUvState {
            weight: 1.0,
            common_factor: 0.0,
            uv_mean: [0.0; 2],
            uv_covariance: [[1.0e-12, 0.0], [0.0, 1.0e-12]],
            knocked: false,
        }];
        let mut knocked_states = Vec::<GaussianUvState>::new();
        let mut previous_day = 0_u32;
        let mut peak_live_state_count = 1usize;

        // Hoist factor node computation: step_days is constant (63) for all observations.
        // NIG density + conditional UV distribution computed once, reused 6×.
        let first_step = self.shell.observation_days[0] - 0;
        let cached_factor_nodes = if config.triangle_gl_order == 0 {
            self.conditional_factor_nodes_fast(sigma_common, first_step)?
        } else {
            self.conditional_factor_nodes_with_order(sigma_common, first_step, config.factor_order)?
        };

        // Build the GH3 triple-complement correction geometry for the t0 path.
        // Uses the frozen step covariance (same Cholesky as TRIANGLE_PRE64_63).
        use crate::worst_of_c1_fast::{build_triple_correction_pre, TripleCorrectionPre};
        let triple_pre: Option<TripleCorrectionPre> = if config.triangle_gl_order == 0 {
            let step_cov = &cached_factor_nodes[0].covariance;
            let cov_s6 = [
                (step_cov[0][0] * 1e6) as i64,
                (step_cov[0][1] * 1e6) as i64,
                (step_cov[1][1] * 1e6) as i64,
            ];
            match cholesky6(cov_s6[0], cov_s6[1], cov_s6[2]) {
                Ok((l11, l21, l22)) => Some(build_triple_correction_pre(
                    l11,
                    l21,
                    l22,
                    &TRIANGLE_PRE64_63.au,
                    &TRIANGLE_PRE64_63.av,
                )),
                Err(_) => None,
            }
        } else {
            None
        };
        let triple_ref = triple_pre.as_ref();

        for observation_index in 0..max_observations {
            let observation_day = self.shell.observation_days[observation_index];
            let step_days = observation_day - previous_day;
            previous_day = observation_day;
            let factor_nodes = if step_days == first_step {
                cached_factor_nodes.clone()
            } else {
                self.conditional_factor_nodes_with_order(
                    sigma_common,
                    step_days,
                    config.factor_order,
                )?
            };
            if let Some(capture) = moment_capture.as_deref_mut() {
                Self::record_onchain_v1_survivor_moments(
                    capture,
                    observation_index,
                    &factor_nodes,
                    &safe_states,
                    &knocked_states,
                );
            }
            if let Some(capture) = replay_capture.as_deref_mut() {
                if factor_nodes.len() == 9 {
                    let safe_weight = safe_states.iter().map(|state| state.weight).sum::<f64>();
                    let knocked_weight =
                        knocked_states.iter().map(|state| state.weight).sum::<f64>();
                    for (node_index, node) in factor_nodes.iter().enumerate() {
                        capture.safe_node_input_mass[observation_index][node_index] =
                            safe_weight * node.weight;
                        capture.knocked_node_input_mass[observation_index][node_index] =
                            knocked_weight * node.weight;
                    }
                }
            }
            let is_maturity = observation_index + 1 == self.shell.observation_days.len();
            let marginal = &mut observation_marginals[observation_index];
            marginal.observation_day = observation_day;

            let safe_weight = safe_states.iter().map(|state| state.weight).sum::<f64>();
            let knocked_weight = knocked_states.iter().map(|state| state.weight).sum::<f64>();
            let survival_mass = (safe_weight + knocked_weight).clamp(0.0, 1.0);
            accumulator.observation_survival_probability[observation_index] = survival_mass;
            marginal.survival_probability = survival_mass;

            if is_maturity {
                let coupon_count = (observation_index + 1) as f64;
                let mut maturity_coupon_hit = 0.0_f64;
                let mut maturity_safe_principal = 0.0_f64;
                let mut maturity_first_knock_in = 0.0_f64;
                let mut maturity_knock_in_redemption = 0.0_f64;

                for state in &safe_states {
                    for node in &factor_nodes {
                        let total_common_factor = state.common_factor + node.value;
                        let total_mean = [
                            state.uv_mean[0] + node.mean[0],
                            state.uv_mean[1] + node.mean[1],
                        ];
                        let total_covariance = [
                            [
                                state.uv_covariance[0][0] + node.covariance[0][0],
                                state.uv_covariance[0][1] + node.covariance[0][1],
                            ],
                            [
                                state.uv_covariance[1][0] + node.covariance[1][0],
                                state.uv_covariance[1][1] + node.covariance[1][1],
                            ],
                        ];
                        let shifted_coupon = self.shifted_halfplanes(
                            coupon_halfplanes,
                            sigma_common,
                            observation_day,
                            total_common_factor,
                        )?;
                        let shifted_ki_safe = self.shifted_halfplanes(
                            self.knock_in_safe_halfplanes,
                            sigma_common,
                            observation_day,
                            total_common_factor,
                        )?;
                        let coupon_probability = self.triangle_probability_explicit_with_order(
                            total_mean,
                            total_covariance,
                            shifted_coupon,
                            config.triangle_gl_order,
                            triple_ref,
                        )?;
                        let ki_safe_probability = self.triangle_probability_explicit_with_order(
                            total_mean,
                            total_covariance,
                            shifted_ki_safe,
                            config.triangle_gl_order,
                            triple_ref,
                        )?;
                        let (ki_probability, worst_indicator_expectation) = self
                            .conditional_ki_moment_from_distribution_with_order(
                                sigma_common,
                                observation_day,
                                total_common_factor,
                                [0.0; 3],
                                total_mean,
                                total_covariance,
                                config.ki_order,
                            )?;
                        let node_scale = state.weight * node.weight;
                        maturity_coupon_hit += node_scale * coupon_probability.clamp(0.0, 1.0);
                        maturity_safe_principal +=
                            node_scale * ki_safe_probability.clamp(coupon_probability, 1.0);
                        maturity_first_knock_in += node_scale
                            * if ki_probability > STATE_MASS_EPS {
                                ki_probability
                            } else {
                                (1.0 - ki_safe_probability).max(0.0)
                            };
                        maturity_knock_in_redemption +=
                            node_scale * worst_indicator_expectation.max(0.0);
                    }
                }

                for state in &knocked_states {
                    for node in &factor_nodes {
                        let total_common_factor = state.common_factor + node.value;
                        let total_mean = [
                            state.uv_mean[0] + node.mean[0],
                            state.uv_mean[1] + node.mean[1],
                        ];
                        let total_covariance = [
                            [
                                state.uv_covariance[0][0] + node.covariance[0][0],
                                state.uv_covariance[0][1] + node.covariance[0][1],
                            ],
                            [
                                state.uv_covariance[1][0] + node.covariance[1][0],
                                state.uv_covariance[1][1] + node.covariance[1][1],
                            ],
                        ];
                        let shifted_coupon = self.shifted_halfplanes(
                            coupon_halfplanes,
                            sigma_common,
                            observation_day,
                            total_common_factor,
                        )?;
                        let coupon_probability = self.triangle_probability_explicit_with_order(
                            total_mean,
                            total_covariance,
                            shifted_coupon,
                            config.triangle_gl_order,
                            None,
                        )?;
                        let knocked_redemption = self
                            .conditional_knocked_redemption_expectation_from_distribution_with_order(
                                sigma_common,
                                observation_day,
                                total_common_factor,
                                [0.0; 3],
                                total_mean,
                                total_covariance,
                                config.ki_order,
                            )?;
                        let node_scale = state.weight * node.weight;
                        maturity_coupon_hit += node_scale * coupon_probability.clamp(0.0, 1.0);
                        maturity_knock_in_redemption += node_scale * knocked_redemption;
                    }
                }

                accumulator.redemption_leg_pv +=
                    self.shell.notional * (maturity_safe_principal + maturity_knock_in_redemption);
                accumulator.coupon_annuity_pv += coupon_count * maturity_coupon_hit;
                accumulator.maturity_redemption_pv +=
                    self.shell.notional * (maturity_safe_principal + maturity_knock_in_redemption);
                accumulator.maturity_knock_in_redemption_pv +=
                    self.shell.notional * maturity_knock_in_redemption;
                accumulator.observation_coupon_annuity_contribution[observation_index] =
                    coupon_count * maturity_coupon_hit;
                accumulator.observation_first_knock_in_probability[observation_index] =
                    maturity_first_knock_in;
                accumulator.expected_life_days += observation_day as f64 * survival_mass;

                marginal.first_knock_in_probability = maturity_first_knock_in;
                marginal.coupon_annuity_contribution = coupon_count * maturity_coupon_hit;

                safe_states.clear();
                knocked_states.clear();
                peak_live_state_count = peak_live_state_count.max(0);
                continue;
            }

            let mut first_hit = 0.0_f64;
            let mut first_knock_in = 0.0_f64;
            let mut next_safe_children = Vec::<GaussianUvState>::new();
            let mut next_knocked_children = Vec::<GaussianUvState>::new();

            for state in &safe_states {
                for (node_index, node) in factor_nodes.iter().enumerate() {
                    let total_common_factor = state.common_factor + node.value;
                    let total_mean = [
                        state.uv_mean[0] + node.mean[0],
                        state.uv_mean[1] + node.mean[1],
                    ];
                    let total_covariance = [
                        [
                            state.uv_covariance[0][0] + node.covariance[0][0],
                            state.uv_covariance[0][1] + node.covariance[0][1],
                        ],
                        [
                            state.uv_covariance[1][0] + node.covariance[1][0],
                            state.uv_covariance[1][1] + node.covariance[1][1],
                        ],
                    ];
                    let shifted_autocall = self.shifted_halfplanes(
                        self.autocall_halfplanes,
                        sigma_common,
                        observation_day,
                        total_common_factor,
                    )?;
                    let shifted_ki_safe = self.shifted_halfplanes(
                        self.knock_in_safe_halfplanes,
                        sigma_common,
                        observation_day,
                        total_common_factor,
                    )?;
                    let node_autocall = self.triangle_uv_region_moment_explicit_with_order(
                        total_mean,
                        total_covariance,
                        shifted_autocall,
                        config.triangle_gl_order,
                        triple_ref,
                    )?;
                    let node_ki_safe = self.triangle_uv_region_moment_explicit_with_order(
                        total_mean,
                        total_covariance,
                        shifted_ki_safe,
                        config.triangle_gl_order,
                        triple_ref,
                    )?;
                    let node_scale = state.weight * node.weight;
                    first_hit += node_scale * node_autocall.probability;
                    if let Some(capture) = replay_capture.as_deref_mut() {
                        if node_index < 9 {
                            capture.node_autocall_first_hit_mass[observation_index][node_index] +=
                                node_scale * node_autocall.probability;
                        }
                    }

                    let safe_continue_moment = node_ki_safe.subtract(node_autocall);
                    let safe_continue_weight = node_scale * safe_continue_moment.probability;
                    if let Some(capture) = replay_capture.as_deref_mut() {
                        if node_index < 9 {
                            capture.safe_node_continue_mass[observation_index][node_index] +=
                                safe_continue_weight.max(0.0);
                        }
                    }
                    if safe_continue_weight > STATE_MASS_EPS {
                        if let Some((uv_mean, uv_covariance)) =
                            safe_continue_moment.conditional_distribution()
                        {
                            next_safe_children.push(GaussianUvState {
                                weight: safe_continue_weight,
                                common_factor: total_common_factor,
                                uv_mean,
                                uv_covariance,
                                knocked: false,
                            });
                        }
                    }

                    let knocked_moment =
                        UvRegionMoment::from_gaussian(total_mean, total_covariance)
                            .subtract(node_ki_safe);
                    let knocked_weight = node_scale * knocked_moment.probability;
                    first_knock_in += knocked_weight;
                    if let Some(capture) = replay_capture.as_deref_mut() {
                        if node_index < 9 {
                            capture.node_first_knock_in_mass[observation_index][node_index] +=
                                knocked_weight.max(0.0);
                            capture.knocked_node_continue_mass[observation_index][node_index] +=
                                knocked_weight.max(0.0);
                        }
                    }
                    if knocked_weight > STATE_MASS_EPS {
                        if let Some((uv_mean, uv_covariance)) =
                            knocked_moment.conditional_distribution()
                        {
                            next_knocked_children.push(GaussianUvState {
                                weight: knocked_weight,
                                common_factor: total_common_factor,
                                uv_mean,
                                uv_covariance,
                                knocked: true,
                            });
                        }
                    }
                }
            }

            for state in &knocked_states {
                for (node_index, node) in factor_nodes.iter().enumerate() {
                    let total_common_factor = state.common_factor + node.value;
                    let total_mean = [
                        state.uv_mean[0] + node.mean[0],
                        state.uv_mean[1] + node.mean[1],
                    ];
                    let total_covariance = [
                        [
                            state.uv_covariance[0][0] + node.covariance[0][0],
                            state.uv_covariance[0][1] + node.covariance[0][1],
                        ],
                        [
                            state.uv_covariance[1][0] + node.covariance[1][0],
                            state.uv_covariance[1][1] + node.covariance[1][1],
                        ],
                    ];
                    let shifted_autocall = self.shifted_halfplanes(
                        self.autocall_halfplanes,
                        sigma_common,
                        observation_day,
                        total_common_factor,
                    )?;
                    let node_autocall = self.triangle_uv_region_moment_explicit_with_order(
                        total_mean,
                        total_covariance,
                        shifted_autocall,
                        config.triangle_gl_order,
                        triple_ref,
                    )?;
                    let node_scale = state.weight * node.weight;
                    first_hit += node_scale * node_autocall.probability;
                    if let Some(capture) = replay_capture.as_deref_mut() {
                        if node_index < 9 {
                            capture.node_autocall_first_hit_mass[observation_index][node_index] +=
                                node_scale * node_autocall.probability;
                        }
                    }

                    let continue_moment =
                        UvRegionMoment::from_gaussian(total_mean, total_covariance)
                            .subtract(node_autocall);
                    let continue_weight = node_scale * continue_moment.probability;
                    if let Some(capture) = replay_capture.as_deref_mut() {
                        if node_index < 9 {
                            capture.knocked_node_continue_mass[observation_index][node_index] +=
                                continue_weight.max(0.0);
                        }
                    }
                    if continue_weight > STATE_MASS_EPS {
                        if let Some((uv_mean, uv_covariance)) =
                            continue_moment.conditional_distribution()
                        {
                            next_knocked_children.push(GaussianUvState {
                                weight: continue_weight,
                                common_factor: total_common_factor,
                                uv_mean,
                                uv_covariance,
                                knocked: true,
                            });
                        }
                    }
                }
            }

            let coupon_count = (observation_index + 1) as f64;
            accumulator.redemption_leg_pv += self.shell.notional * first_hit;
            accumulator.coupon_annuity_pv += coupon_count * first_hit;
            accumulator.observation_autocall_first_hit_probability[observation_index] = first_hit;
            accumulator.observation_first_knock_in_probability[observation_index] = first_knock_in;
            accumulator.observation_coupon_annuity_contribution[observation_index] =
                coupon_count * first_hit;
            accumulator.observation_autocall_redemption_pv_contribution[observation_index] =
                self.shell.notional * first_hit;
            accumulator.expected_life_days += observation_day as f64 * first_hit;

            marginal.autocall_first_hit_probability = first_hit;
            marginal.first_knock_in_probability = first_knock_in;
            marginal.coupon_annuity_contribution = coupon_count * first_hit;
            marginal.autocall_redemption_pv_contribution = self.shell.notional * first_hit;

            safe_states = self.compress_gaussian_uv_state_class(
                next_safe_children,
                false,
                config.components_per_class as usize,
            );
            knocked_states = self.compress_gaussian_uv_state_class(
                next_knocked_children,
                true,
                config.components_per_class as usize,
            );

            let live_state_count = safe_states.len() + knocked_states.len();
            peak_live_state_count = peak_live_state_count.max(live_state_count);
        }

        let knock_in_rate = accumulator
            .observation_first_knock_in_probability
            .into_iter()
            .sum::<f64>()
            .clamp(0.0, 1.0);
        let autocall_rate = accumulator
            .observation_autocall_first_hit_probability
            .into_iter()
            .sum::<f64>()
            .clamp(0.0, 1.0);
        let live_state_count = safe_states.len() + knocked_states.len();
        let terminalized = max_observations == self.shell.observation_days.len();
        let live_probability_mass = if terminalized {
            0.0
        } else {
            (safe_states.iter().map(|state| state.weight).sum::<f64>()
                + knocked_states.iter().map(|state| state.weight).sum::<f64>())
            .clamp(0.0, 1.0)
        };
        let result = TraceBuildResult {
            trace: LegTrace {
                redemption_leg_pv: accumulator.redemption_leg_pv,
                coupon_annuity_pv: accumulator.coupon_annuity_pv,
                expected_life_days: accumulator.expected_life_days,
                knock_in_rate,
                autocall_rate,
                observation_survival_probability: accumulator.observation_survival_probability,
                observation_autocall_first_hit_probability: accumulator
                    .observation_autocall_first_hit_probability,
                observation_first_knock_in_probability: accumulator
                    .observation_first_knock_in_probability,
                observation_coupon_annuity_contribution: accumulator
                    .observation_coupon_annuity_contribution,
                observation_autocall_redemption_pv_contribution: accumulator
                    .observation_autocall_redemption_pv_contribution,
                maturity_redemption_pv: accumulator.maturity_redemption_pv,
                maturity_knock_in_redemption_pv: accumulator.maturity_knock_in_redemption_pv,
            },
            completed_observations: max_observations,
            terminalized,
            live_state_count,
            peak_live_state_count,
            live_probability_mass,
        };

        Ok((observation_marginals, result))
    }

    fn build_onchain_v1_trace_with_continuation(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
        max_observations: usize,
    ) -> Result<([ObservationMarginal; 6], TraceBuildResult), FactoredWorstOfError> {
        self.build_onchain_v1_trace_with_continuation_internal(
            sigma_common,
            config,
            max_observations,
            None,
            None,
        )
    }

    /// Deterministic leg decomposition from the frozen factor skeleton.
    ///
    /// Returns the same `V0` / `U0` split used by the 1D engine:
    /// - redemption leg `V0`
    /// - coupon annuity `U0 = V1 - V0`
    /// - loss leg `notional - V0`
    ///
    /// Error conditions:
    /// - invalid `sigma_common`
    /// - invalid factor-shape or covariance parameters
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn leg_decomposition(
        &self,
        sigma_common: f64,
    ) -> Result<FactoredWorstOfLegDecomposition, FactoredWorstOfError> {
        self.validate()?;
        let trace = self.deterministic_leg_trace(sigma_common)?;
        Ok(self.leg_decomposition_from_trace(trace))
    }

    /// Deterministic coupon quote from the frozen factor skeleton.
    ///
    /// The transition law comes from the calibrated common NIG factor plus
    /// Gaussian spread residuals. The payoff recursion itself matches the
    /// Python reference pricer's path-state logic: quarterly monitoring,
    /// memory coupons, observation-date autocall, and discrete KI tracked
    /// through the life of the note. The expectation is taken by propagating
    /// weighted live states across the full step kernel and merging nearby
    /// states in `(x_spy, u, v)` buckets between observations.
    ///
    /// Error conditions:
    /// - invalid `sigma_common`
    /// - invalid factor-shape or covariance parameters
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn observation_marginals(
        &self,
        sigma_common: f64,
    ) -> Result<[ObservationMarginal; 6], FactoredWorstOfError> {
        self.validate()?;
        self.build_observation_marginals(sigma_common)
    }

    /// Low-CU observation marginals for the on-chain v1 approximation path.
    ///
    /// Error conditions:
    /// - invalid `sigma_common`
    /// - invalid factor-shape or covariance parameters
    /// - unsupported quadrature orders in `config`
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn observation_marginals_onchain_v1(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
    ) -> Result<[ObservationMarginal; 6], FactoredWorstOfError> {
        self.validate()?;
        self.build_observation_marginals_onchain_v1(sigma_common, config)
    }

    /// Exact survivor-recursion checkpoint after the first `max_observations`
    /// monitoring dates. `max_observations` is clamped to the product schedule.
    ///
    /// Error conditions:
    /// - invalid `sigma_common`
    /// - invalid factor-shape or covariance parameters
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn deterministic_checkpoint(
        &self,
        sigma_common: f64,
        max_observations: usize,
    ) -> Result<FactoredWorstOfCheckpoint, FactoredWorstOfError> {
        self.validate()?;
        let checkpoint = self.initial_checkpoint(sigma_common)?;
        Ok(self
            .advance_deterministic_checkpoint_internal(checkpoint, max_observations)?
            .1)
    }

    /// Resumes the exact survivor recursion from a previously saved checkpoint
    /// through `max_observations` monitoring dates in total.
    ///
    /// Error conditions:
    /// - invalid checkpoint contents or schedule index
    /// - invalid factor-shape or covariance parameters
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn resume_deterministic_checkpoint(
        &self,
        checkpoint: &FactoredWorstOfCheckpoint,
        max_observations: usize,
    ) -> Result<FactoredWorstOfCheckpoint, FactoredWorstOfError> {
        self.validate()?;
        Ok(self
            .advance_deterministic_checkpoint_internal(checkpoint.clone(), max_observations)?
            .1)
    }

    /// Profiles a saved exact survivor-recursion checkpoint without advancing it.
    ///
    /// Error conditions:
    /// - invalid checkpoint contents or schedule index
    /// - invalid factor-shape or covariance parameters
    pub fn profile_deterministic_checkpoint(
        &self,
        checkpoint: &FactoredWorstOfCheckpoint,
    ) -> Result<FactoredWorstOfTraceProfile, FactoredWorstOfError> {
        self.validate()?;
        let result = self.trace_build_result_from_checkpoint(checkpoint)?;
        Ok(FactoredWorstOfTraceProfile {
            sigma_common: checkpoint.sigma_common,
            completed_observations: result.completed_observations as u8,
            terminalized: result.terminalized,
            live_state_count: result.live_state_count as u32,
            peak_live_state_count: result.peak_live_state_count as u32,
            live_probability_mass: result.live_probability_mass,
            redemption_leg_pv: result.trace.redemption_leg_pv,
            coupon_annuity_pv: result.trace.coupon_annuity_pv,
            expected_life_days: result.trace.expected_life_days,
            knock_in_rate: result.trace.knock_in_rate,
            autocall_rate: result.trace.autocall_rate,
        })
    }

    /// Profiles the survivor recursion through the first `max_observations`
    /// monitoring dates. `max_observations` is clamped to the product schedule.
    ///
    /// Error conditions:
    /// - invalid `sigma_common`
    /// - invalid factor-shape or covariance parameters
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn profile_deterministic_trace(
        &self,
        sigma_common: f64,
        max_observations: usize,
    ) -> Result<FactoredWorstOfTraceProfile, FactoredWorstOfError> {
        self.validate()?;
        let result = self.build_deterministic_leg_trace(sigma_common, max_observations)?;
        Ok(FactoredWorstOfTraceProfile {
            sigma_common,
            completed_observations: result.completed_observations as u8,
            terminalized: result.terminalized,
            live_state_count: result.live_state_count as u32,
            peak_live_state_count: result.peak_live_state_count as u32,
            live_probability_mass: result.live_probability_mass,
            redemption_leg_pv: result.trace.redemption_leg_pv,
            coupon_annuity_pv: result.trace.coupon_annuity_pv,
            expected_life_days: result.trace.expected_life_days,
            knock_in_rate: result.trace.knock_in_rate,
            autocall_rate: result.trace.autocall_rate,
        })
    }

    fn quote_from_trace_result(
        &self,
        sigma_common: f64,
        observation_marginals: [ObservationMarginal; 6],
        trace: LegTrace,
    ) -> FactoredWorstOfQuote {
        let leg_decomposition = self.leg_decomposition_from_trace(trace);
        let expected_redemption = leg_decomposition.redemption_leg_pv;
        let unit_coupon_sensitivity = leg_decomposition.coupon_annuity_pv;
        let expected_coupon_count = unit_coupon_sensitivity;
        let fair_coupon_per_observation = if expected_coupon_count > 1.0e-12 {
            (leg_decomposition.loss_leg_pv / expected_coupon_count).max(0.0)
        } else {
            0.0
        };
        let fair_coupon_bps = fair_coupon_per_observation * 10_000.0 / self.shell.notional;
        let quoted_coupon_bps =
            (self.shell.quote_share * fair_coupon_bps - self.shell.issuer_margin_bps).max(0.0);

        FactoredWorstOfQuote {
            sigma_common,
            fair_coupon_per_observation,
            fair_coupon_bps,
            quoted_coupon_bps,
            leg_decomposition,
            zero_coupon_pv: leg_decomposition.redemption_leg_pv,
            unit_coupon_pv: leg_decomposition.redemption_leg_pv
                + leg_decomposition.coupon_annuity_pv,
            unit_coupon_sensitivity,
            expected_redemption,
            expected_coupon_count,
            expected_life_days: trace.expected_life_days,
            knock_in_rate: trace.knock_in_rate,
            autocall_rate: trace.autocall_rate,
            approximate_no_autocall_probability: (1.0 - trace.autocall_rate).clamp(0.0, 1.0),
            approximate_no_knock_in_probability: (1.0 - trace.knock_in_rate).clamp(0.0, 1.0),
            observation_marginals,
        }
    }

    /// Low-CU profile of the reduced-state on-chain v1 continuation through
    /// the first `max_observations` monitoring dates.
    ///
    /// Error conditions:
    /// - invalid `sigma_common`
    /// - invalid factor-shape or covariance parameters
    /// - unsupported quadrature orders in `config`
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn profile_onchain_v1_trace(
        &self,
        sigma_common: f64,
        max_observations: usize,
        config: FactoredWorstOfOnchainConfig,
    ) -> Result<FactoredWorstOfTraceProfile, FactoredWorstOfError> {
        self.validate()?;
        let (_, result) =
            self.build_onchain_v1_trace_with_continuation(sigma_common, config, max_observations)?;
        Ok(FactoredWorstOfTraceProfile {
            sigma_common,
            completed_observations: result.completed_observations as u8,
            terminalized: result.terminalized,
            live_state_count: result.live_state_count as u32,
            peak_live_state_count: result.peak_live_state_count as u32,
            live_probability_mass: result.live_probability_mass,
            redemption_leg_pv: result.trace.redemption_leg_pv,
            coupon_annuity_pv: result.trace.coupon_annuity_pv,
            expected_life_days: result.trace.expected_life_days,
            knock_in_rate: result.trace.knock_in_rate,
            autocall_rate: result.trace.autocall_rate,
        })
    }

    /// Exact continuation survivor means for the 9-node c1 lookup tables.
    ///
    /// The returned `(u, v)` means are recorded at the start of each
    /// observation, conditional on survival through all prior observations,
    /// and conditioned on the current common-factor node.
    pub fn onchain_v1_survivor_moment_table(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
    ) -> Result<OnchainV1SurvivorMomentTable, FactoredWorstOfError> {
        self.validate()?;
        Self::validate_onchain_config(config)?;
        if config.factor_order != 9 {
            return Err(FactoredWorstOfError::InvalidQuadratureOrder);
        }

        let mut capture = OnchainV1SurvivorMomentCapture::default();
        self.build_onchain_v1_trace_with_continuation_internal(
            sigma_common,
            config,
            self.shell.observation_days.len(),
            Some(&mut capture),
            None,
        )?;
        Ok(OnchainV1SurvivorMomentTable {
            observation_days: self.shell.observation_days,
            expectation_u_safe: capture.expectation_u_safe,
            expectation_v_safe: capture.expectation_v_safe,
            expectation_u_knocked: capture.expectation_u_knocked,
            expectation_v_knocked: capture.expectation_v_knocked,
            common_factor_safe: capture.common_factor_safe,
            common_factor_knocked: capture.common_factor_knocked,
        })
    }

    #[cfg(not(target_os = "solana"))]
    pub fn onchain_v1_replay_diagnostic(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
    ) -> Result<OnchainV1ReplayDiagnostic, FactoredWorstOfError> {
        self.validate()?;
        Self::validate_onchain_config(config)?;
        if config.factor_order != 9 {
            return Err(FactoredWorstOfError::InvalidQuadratureOrder);
        }

        let mut capture = OnchainV1ReplayCapture::default();
        self.build_onchain_v1_trace_with_continuation_internal(
            sigma_common,
            config,
            self.shell.observation_days.len(),
            None,
            Some(&mut capture),
        )?;
        Ok(OnchainV1ReplayDiagnostic {
            observation_days: self.shell.observation_days,
            safe_node_input_mass: capture.safe_node_input_mass,
            knocked_node_input_mass: capture.knocked_node_input_mass,
            node_autocall_first_hit_mass: capture.node_autocall_first_hit_mass,
            node_first_knock_in_mass: capture.node_first_knock_in_mass,
            safe_node_continue_mass: capture.safe_node_continue_mass,
            knocked_node_continue_mass: capture.knocked_node_continue_mass,
        })
    }

    fn first_observation_region_probability_onchain_v1_internal(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
        base: [BarrierHalfPlane; 3],
    ) -> Result<f64, FactoredWorstOfError> {
        self.validate()?;
        Self::validate_onchain_config(config)?;
        let Some(&observation_day) = self.shell.observation_days.first() else {
            return Ok(0.0);
        };
        let factor_nodes = self.conditional_factor_nodes_with_order(
            sigma_common,
            observation_day,
            config.factor_order,
        )?;
        let mut probability = 0.0_f64;
        for node in &factor_nodes {
            let planes =
                self.shifted_halfplanes(base, sigma_common, observation_day, node.value)?;
            let moment = self.triangle_uv_region_moment_explicit_with_order(
                node.mean,
                node.covariance,
                planes,
                config.triangle_gl_order,
                None,
            )?;
            probability += node.weight * moment.probability;
        }
        Ok(probability.clamp(0.0, 1.0))
    }

    /// Profiles the first-observation autocall region probability under the
    /// reduced-state on-chain v1 approximation.
    ///
    /// Error conditions:
    /// - invalid factor skeleton or `sigma_common`
    /// - unsupported quadrature orders in `config`
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn first_observation_autocall_probability_onchain_v1(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
    ) -> Result<f64, FactoredWorstOfError> {
        self.first_observation_region_probability_onchain_v1_internal(
            sigma_common,
            config,
            self.autocall_halfplanes,
        )
    }

    /// Profiles the first-observation KI-safe region probability under the
    /// reduced-state on-chain v1 approximation.
    ///
    /// Error conditions:
    /// - invalid factor skeleton or `sigma_common`
    /// - unsupported quadrature orders in `config`
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn first_observation_ki_safe_probability_onchain_v1(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
    ) -> Result<f64, FactoredWorstOfError> {
        self.first_observation_region_probability_onchain_v1_internal(
            sigma_common,
            config,
            self.knock_in_safe_halfplanes,
        )
    }

    /// Accuracy-first fallback for the public v1 quote entrypoint.
    ///
    /// The on-chain v1 path uses the moment-enriched survivor continuation:
    /// a point-mass mixture in the common factor with conditional Gaussian
    /// spread states carrying truncated triangle moments. The exact
    /// deterministic reference remains available via [`Self::quote_coupon`].
    ///
    /// Error conditions:
    /// - invalid `sigma_common`
    /// - invalid factor-shape or covariance parameters
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn quote_coupon_onchain_v1(
        &self,
        sigma_common: f64,
        config: FactoredWorstOfOnchainConfig,
    ) -> Result<FactoredWorstOfQuote, FactoredWorstOfError> {
        let (observation_marginals, result) = self.build_onchain_v1_trace_with_continuation(
            sigma_common,
            config,
            self.shell.observation_days.len(),
        )?;
        Ok(self.quote_from_trace_result(sigma_common, observation_marginals, result.trace))
    }

    /// Deterministic coupon quote from the frozen factor skeleton.
    ///
    /// The transition law comes from the calibrated common NIG factor plus
    /// Gaussian spread residuals. The payoff recursion itself matches the
    /// Python reference pricer's path-state logic: quarterly monitoring,
    /// memory coupons, observation-date autocall, and discrete KI tracked
    /// through the life of the note. The expectation is taken by propagating
    /// weighted live states across the full step kernel and merging nearby
    /// states in `(x_spy, u, v)` buckets between observations.
    ///
    /// Error conditions:
    /// - invalid `sigma_common`
    /// - invalid factor-shape or covariance parameters
    /// - failures returned by the underlying fixed-point SolMath primitives
    pub fn quote_coupon(
        &self,
        sigma_common: f64,
    ) -> Result<FactoredWorstOfQuote, FactoredWorstOfError> {
        self.validate()?;
        let mut observation_marginals = self.build_observation_marginals(sigma_common)?;

        let trace = self.deterministic_leg_trace(sigma_common)?;
        for (index, marginal) in observation_marginals.iter_mut().enumerate() {
            marginal.survival_probability = trace.observation_survival_probability[index];
            marginal.autocall_first_hit_probability =
                trace.observation_autocall_first_hit_probability[index];
            marginal.first_knock_in_probability =
                trace.observation_first_knock_in_probability[index];
            marginal.coupon_annuity_contribution =
                trace.observation_coupon_annuity_contribution[index];
            marginal.autocall_redemption_pv_contribution =
                trace.observation_autocall_redemption_pv_contribution[index];
        }
        Ok(self.quote_from_trace_result(sigma_common, observation_marginals, trace))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solve_intersection(a: BarrierHalfPlane, b: BarrierHalfPlane) -> Option<(f64, f64)> {
        let det = a.a_u * b.a_v - b.a_u * a.a_v;
        if det.abs() < 1e-12 {
            return None;
        }
        let u = (a.rhs * b.a_v - b.rhs * a.a_v) / det;
        let v = (a.a_u * b.rhs - b.a_u * a.rhs) / det;
        Some((u, v))
    }

    #[test]
    fn frozen_model_matches_current_shell() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        assert_eq!(model.shell.observation_days, OBSERVATION_DAYS_18M);
        assert_eq!(model.shell.knock_in_barrier, 0.80);
        assert_eq!(model.shell.quote_share, 0.60);
        assert_eq!(model.shell.issuer_margin_bps, 100.0);
        assert!(model.validate().is_ok());
    }

    #[test]
    fn delta_step_and_zero_mean_location_are_finite() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma_common = 0.364_352_876_062_913_67;
        let delta = model.delta_step(sigma_common, 63).unwrap();
        let location = model
            .zero_mean_common_factor_location(sigma_common, 63)
            .unwrap();
        assert!(delta > 0.0);
        assert!(location.is_finite());
    }

    #[test]
    fn risk_neutral_drifts_are_finite() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let drifts = model
            .risk_neutral_step_drifts(0.364_352_876_062_913_67, 63)
            .unwrap();
        assert!(drifts.into_iter().all(f64::is_finite));
    }

    #[test]
    fn positive_factor_opens_the_autocall_triangle() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let shifted = model
            .shifted_halfplanes(
                model.autocall_halfplanes,
                0.364_352_876_062_913_67,
                63,
                0.05,
            )
            .unwrap();
        let p01 = solve_intersection(shifted[0], shifted[1]).unwrap();
        let p02 = solve_intersection(shifted[0], shifted[2]).unwrap();
        let p12 = solve_intersection(shifted[1], shifted[2]).unwrap();
        for point in [p01, p02, p12] {
            assert!(shifted
                .iter()
                .all(|plane| plane.a_u * point.0 + plane.a_v * point.1 <= plane.rhs + 1e-10));
        }
    }

    #[test]
    fn uniform_barrier_halfplanes_reproduce_stored_shell_geometry() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let autocall = model
            .uniform_barrier_halfplanes(model.shell.autocall_barrier)
            .unwrap();
        let knock_in_safe = model
            .uniform_barrier_halfplanes(model.shell.knock_in_barrier)
            .unwrap();

        for index in 0..3 {
            assert!((autocall[index].a_u - model.autocall_halfplanes[index].a_u).abs() < 1.0e-12);
            assert!((autocall[index].a_v - model.autocall_halfplanes[index].a_v).abs() < 1.0e-12);
            assert!((autocall[index].rhs - model.autocall_halfplanes[index].rhs).abs() < 1.0e-12);
            assert!(
                (knock_in_safe[index].a_u - model.knock_in_safe_halfplanes[index].a_u).abs()
                    < 1.0e-12
            );
            assert!(
                (knock_in_safe[index].a_v - model.knock_in_safe_halfplanes[index].a_v).abs()
                    < 1.0e-12
            );
            assert!(
                (knock_in_safe[index].rhs - model.knock_in_safe_halfplanes[index].rhs).abs()
                    < 1.0e-12
            );
        }
    }

    #[test]
    fn zero_state_helpers_match_base_conditionals() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma_common = 0.364_352_876_062_913_67;
        let step_days = 63;
        let factor_value = 0.05;
        let zero_state = [0.0; 3];

        let base_autocall = model
            .autocall_probability(sigma_common, step_days, factor_value)
            .unwrap();
        let shifted_autocall = model
            .state_triangle_probability(
                model.autocall_halfplanes,
                sigma_common,
                step_days,
                factor_value,
                zero_state,
            )
            .unwrap();
        assert!((base_autocall - shifted_autocall).abs() < 1.0e-12);

        let base_ki = model
            .conditional_ki_moment(sigma_common, step_days, factor_value)
            .unwrap();
        let shifted_ki = model
            .conditional_ki_moment_from_state(sigma_common, step_days, factor_value, zero_state)
            .unwrap();
        assert!((base_ki.0 - shifted_ki.0).abs() < 1.0e-12);
        assert!((base_ki.1 - shifted_ki.1).abs() < 1.0e-12);
    }

    #[test]
    fn zero_state_pre_maturity_partition_matches_marginals() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma_common = 0.364_352_876_062_913_67;
        let step_days = 63;
        let factor_nodes = model
            .conditional_factor_nodes(sigma_common, step_days)
            .unwrap();
        let exact = model
            .pre_maturity_state_probabilities(
                sigma_common,
                step_days,
                model
                    .uniform_barrier_halfplanes(
                        model.shell.autocall_barrier.max(model.shell.coupon_barrier),
                    )
                    .unwrap(),
                &factor_nodes,
                [0.0; 3],
                false,
            )
            .unwrap();
        let autocall = model
            .marginal_autocall_probability(sigma_common, step_days)
            .unwrap();
        let knock_in_safe = model
            .marginal_knock_in_safe_probability(sigma_common, step_days)
            .unwrap();
        assert!((exact.autocall_probability - autocall).abs() < 1.0e-12);
        assert!((exact.autocall_coupon_probability - autocall).abs() < 1.0e-12);
        assert!((exact.safe_survival_probability - (knock_in_safe - autocall)).abs() < 1.0e-12);
        assert!((exact.first_knock_in_probability - (1.0 - knock_in_safe)).abs() < 1.0e-12);
        assert!(
            (exact.autocall_probability
                + exact.safe_survival_probability
                + exact.knocked_survival_probability
                - 1.0)
                .abs()
                < 1.0e-12
        );
    }

    #[test]
    fn fixed_point_triangle_probabilities_match_python_reference() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma = 0.364_352_876_062_913_67;

        let autocall_cases = [
            (63_u32, 0.02_f64, 0.029_209_342_806_f64),
            (63_u32, 0.05_f64, 0.367_914_583_526_f64),
            (126_u32, 0.08_f64, 0.396_905_185_092_f64),
            (189_u32, 0.10_f64, 0.368_185_144_278_f64),
        ];
        for (step_days, factor, expected) in autocall_cases {
            let got = model
                .autocall_probability(sigma, step_days, factor)
                .unwrap();
            assert!(
                (got - expected).abs() < 5.0e-6,
                "autocall step={step_days} factor={factor} got={got} expected={expected}"
            );
        }

        let knock_in_cases = [
            (126_u32, -0.20_f64, 0.913_909_895_272_f64),
            (189_u32, -0.10_f64, 0.977_122_500_598_f64),
            (378_u32, -0.05_f64, 0.915_066_381_491_f64),
            (378_u32, 0.00_f64, 0.961_706_766_610_f64),
        ];
        for (step_days, factor, expected) in knock_in_cases {
            let got = model
                .knock_in_safe_probability(sigma, step_days, factor)
                .unwrap();
            assert!(
                (got - expected).abs() < 5.0e-6,
                "ki-safe step={step_days} factor={factor} got={got} expected={expected}"
            );
        }
    }

    #[test]
    fn conditional_ki_moment_matches_safe_triangle_complement() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma = 0.364_352_876_062_913_67;
        let cases = [
            (63_u32, -0.10_f64),
            (126_u32, -0.05_f64),
            (189_u32, 0.00_f64),
            (378_u32, 0.05_f64),
        ];

        for (step_days, factor_value) in cases {
            let safe_probability = model
                .knock_in_safe_probability(sigma, step_days, factor_value)
                .unwrap();
            let (ki_probability, worst_indicator_expectation) = model
                .conditional_ki_moment(sigma, step_days, factor_value)
                .unwrap();
            assert!(
                ((1.0 - safe_probability) - ki_probability).abs() < 2.0e-2,
                "step={step_days} factor={factor_value} safe={safe_probability} ki={ki_probability}"
            );
            assert!(worst_indicator_expectation >= 0.0);
            assert!(
                worst_indicator_expectation
                    <= ki_probability * model.shell.knock_in_barrier + 1.0e-6
            );
        }
    }

    #[test]
    fn observation_marginal_ki_probability_matches_safe_complement() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let quote = model.quote_coupon(0.364_352_876_062_913_67).unwrap();
        for marginal in quote.observation_marginals {
            assert!(
                ((1.0 - marginal.knock_in_safe_probability) - marginal.ki_probability).abs()
                    < 2.0e-2,
                "day={} safe={} ki={}",
                marginal.observation_day,
                marginal.knock_in_safe_probability,
                marginal.ki_probability
            );
            assert!(marginal.ki_worst_indicator_expectation >= 0.0);
            assert!(
                marginal.ki_worst_indicator_expectation
                    <= marginal.ki_probability * model.shell.knock_in_barrier + 1.0e-6
            );
        }
    }

    #[test]
    fn common_factor_density_is_positive_at_center() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let day_63 = model
            .common_factor_pdf(0.364_352_876_062_913_67, 63, 0.0)
            .unwrap();
        let day_126 = model
            .common_factor_pdf(0.364_352_876_062_913_67, 126, 0.0)
            .unwrap();
        assert!(day_63 > 0.0);
        assert!(day_126 > 0.0);
    }

    #[test]
    fn leg_decomposition_matches_quote_identity() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let legs = model.leg_decomposition(0.364_352_876_062_913_67).unwrap();
        let quote = model.quote_coupon(0.364_352_876_062_913_67).unwrap();
        assert!((legs.redemption_leg_pv - quote.zero_coupon_pv).abs() < 1.0e-12);
        assert!((legs.coupon_annuity_pv - quote.unit_coupon_sensitivity).abs() < 1.0e-12);
        assert!(
            (legs.loss_leg_pv - (model.shell.notional - legs.redemption_leg_pv)).abs() < 1.0e-12
        );
        assert!(
            (quote.fair_coupon_per_observation - legs.loss_leg_pv / legs.coupon_annuity_pv).abs()
                < 1.0e-12
        );
        assert!(
            (quote.unit_coupon_pv - (legs.redemption_leg_pv + legs.coupon_annuity_pv)).abs()
                < 1.0e-9
        );
        assert!(
            (legs.early_autocall_redemption_pv + legs.maturity_redemption_pv
                - legs.redemption_leg_pv)
                .abs()
                < 1.0e-9
        );
        assert!(legs.maturity_knock_in_redemption_pv <= legs.maturity_redemption_pv + 1.0e-12);
    }

    #[test]
    fn observation_trace_matches_leg_totals() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let quote = model.quote_coupon(0.364_352_876_062_913_67).unwrap();
        let total_autocall = quote
            .observation_marginals
            .iter()
            .map(|marginal| marginal.autocall_first_hit_probability)
            .sum::<f64>();
        let total_first_knock_in = quote
            .observation_marginals
            .iter()
            .map(|marginal| marginal.first_knock_in_probability)
            .sum::<f64>();
        let total_coupon_annuity = quote
            .observation_marginals
            .iter()
            .map(|marginal| marginal.coupon_annuity_contribution)
            .sum::<f64>();
        let total_autocall_redemption = quote
            .observation_marginals
            .iter()
            .map(|marginal| marginal.autocall_redemption_pv_contribution)
            .sum::<f64>();

        assert!((total_autocall - quote.autocall_rate).abs() < 1.0e-9);
        assert!((total_first_knock_in - quote.knock_in_rate).abs() < 1.0e-9);
        assert!((total_coupon_annuity - quote.leg_decomposition.coupon_annuity_pv).abs() < 1.0e-9);
        assert!(
            (total_autocall_redemption - quote.leg_decomposition.early_autocall_redemption_pv)
                .abs()
                < 1.0e-9
        );
        assert!(
            (quote.leg_decomposition.early_autocall_redemption_pv
                + quote.leg_decomposition.maturity_redemption_pv
                - quote.leg_decomposition.redemption_leg_pv)
                .abs()
                < 1.0e-9
        );

        let mut previous_survival = 1.0 + 1.0e-12;
        for marginal in quote.observation_marginals {
            assert!(marginal.survival_probability <= previous_survival + 1.0e-12);
            assert!(
                marginal.autocall_first_hit_probability <= marginal.survival_probability + 1.0e-12
            );
            assert!(marginal.first_knock_in_probability <= marginal.survival_probability + 1.0e-12);
            previous_survival = marginal.survival_probability;
        }
    }

    #[test]
    fn deterministic_quote_recursion_matches_reference_band() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let quote = model.quote_coupon(0.364_352_876_062_913_67).unwrap();
        assert!(quote.fair_coupon_bps.is_finite());
        assert!((quote.fair_coupon_bps - 778.56).abs() < 20.0);
        assert!((quote.quoted_coupon_bps - 367.13).abs() < 15.0);
        assert!(quote.quoted_coupon_bps >= 0.0);
        assert!(quote.expected_coupon_count > 1.0);
        assert!(quote.expected_coupon_count < 1.4);
        assert!((quote.zero_coupon_pv - 90.17).abs() < 1.0);
        assert!((quote.unit_coupon_pv - 91.43).abs() < 1.0);
        assert!((quote.expected_life_days - 230.69).abs() < 10.0);
        assert!((quote.knock_in_rate - 0.358).abs() < 0.05);
        assert!((quote.autocall_rate - 0.575).abs() < 0.05);
    }

    #[test]
    fn deterministic_quote_recursion_moves_with_sigma() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let low = model.quote_coupon(0.291_482_300_850_330_96).unwrap();
        let mid = model.quote_coupon(0.364_352_876_062_913_67).unwrap();
        let high = model.quote_coupon(0.437_223_451_275_496_4).unwrap();
        assert!(low.fair_coupon_bps.is_finite());
        assert!(mid.fair_coupon_bps.is_finite());
        assert!(high.fair_coupon_bps.is_finite());
        assert!(
            (low.fair_coupon_bps - 647.612_476_618_249_6).abs() < 0.05,
            "low={} mid={} high={}",
            low.fair_coupon_bps,
            mid.fair_coupon_bps,
            high.fair_coupon_bps
        );
        assert!(
            (mid.fair_coupon_bps - 778.455_734_604_584_9).abs() < 0.05,
            "low={} mid={} high={}",
            low.fair_coupon_bps,
            mid.fair_coupon_bps,
            high.fair_coupon_bps
        );
        assert!(
            (high.fair_coupon_bps - 882.538_556_029_717).abs() < 0.05,
            "low={} mid={} high={}",
            low.fair_coupon_bps,
            mid.fair_coupon_bps,
            high.fair_coupon_bps
        );
        assert!((low.zero_coupon_pv - 92.190_067_334_876_4).abs() < 0.005);
        assert!((mid.zero_coupon_pv - 90.376_424_088_588_93).abs() < 0.005);
        assert!((high.zero_coupon_pv - 88.942_475_636_164_45).abs() < 0.005);
        assert!((low.unit_coupon_pv - 93.396_025_056_059_61).abs() < 0.005);
        assert!((mid.unit_coupon_pv - 91.612_663_412_626_97).abs() < 0.005);
        assert!((high.unit_coupon_pv - 90.195_398_161_564_37).abs() < 0.005);
        assert!((low.knock_in_rate - 0.301_119_118_375_250_35).abs() < 0.0005);
        assert!((mid.knock_in_rate - 0.356_401_735_005_844_27).abs() < 0.0005);
        assert!((high.knock_in_rate - 0.383_296_046_816_854_5).abs() < 0.0005);
        assert!((low.autocall_rate - 0.575_156_347_278_624_9).abs() < 0.0005);
        assert!((mid.autocall_rate - 0.590_816_400_486_198_4).abs() < 0.0005);
        assert!((high.autocall_rate - 0.602_120_699_946_281).abs() < 0.0005);
        assert!((low.expected_life_days - 227.625_913_452_839_9).abs() < 0.005);
        assert!((mid.expected_life_days - 223.787_621_392_430_45).abs() < 0.005);
        assert!((high.expected_life_days - 220.989_315_042_210_88).abs() < 0.005);
        assert!(
            low.fair_coupon_bps < mid.fair_coupon_bps,
            "low={} mid={} high={}",
            low.fair_coupon_bps,
            mid.fair_coupon_bps,
            high.fair_coupon_bps
        );
        assert!(
            mid.fair_coupon_bps < high.fair_coupon_bps,
            "low={} mid={} high={}",
            low.fair_coupon_bps,
            mid.fair_coupon_bps,
            high.fair_coupon_bps
        );
        assert!(low.leg_decomposition.redemption_leg_pv > high.leg_decomposition.redemption_leg_pv);
        assert!(low.leg_decomposition.loss_leg_pv < high.leg_decomposition.loss_leg_pv);
        assert!(low.leg_decomposition.coupon_annuity_pv < high.leg_decomposition.coupon_annuity_pv);
        assert!(low.expected_life_days > high.expected_life_days);
    }

    #[test]
    fn deterministic_checkpoint_resume_matches_one_shot_trace() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma_common = 0.364_352_876_062_913_67;

        let checkpoint_obs3 = model.deterministic_checkpoint(sigma_common, 3).unwrap();
        let prefix_profile = model
            .profile_deterministic_checkpoint(&checkpoint_obs3)
            .unwrap();
        let direct_prefix = model.profile_deterministic_trace(sigma_common, 3).unwrap();

        assert_eq!(checkpoint_obs3.completed_observations, 3);
        assert_eq!(prefix_profile.completed_observations, 3);
        assert!(!prefix_profile.terminalized);
        assert!(
            (prefix_profile.redemption_leg_pv - direct_prefix.redemption_leg_pv).abs() < 1.0e-9
        );
        assert!(
            (prefix_profile.coupon_annuity_pv - direct_prefix.coupon_annuity_pv).abs() < 1.0e-9
        );
        assert!(
            (prefix_profile.expected_life_days - direct_prefix.expected_life_days).abs() < 1.0e-9
        );
        assert!((prefix_profile.knock_in_rate - direct_prefix.knock_in_rate).abs() < 1.0e-9);
        assert!((prefix_profile.autocall_rate - direct_prefix.autocall_rate).abs() < 1.0e-9);
        assert_eq!(
            prefix_profile.live_state_count,
            direct_prefix.live_state_count
        );
        assert_eq!(
            prefix_profile.peak_live_state_count,
            direct_prefix.peak_live_state_count
        );

        let terminal_checkpoint = model
            .resume_deterministic_checkpoint(&checkpoint_obs3, 6)
            .unwrap();
        let resumed_profile = model
            .profile_deterministic_checkpoint(&terminal_checkpoint)
            .unwrap();
        let direct_quote = model.quote_coupon(sigma_common).unwrap();

        assert_eq!(terminal_checkpoint.completed_observations, 6);
        assert!(resumed_profile.terminalized);
        assert_eq!(resumed_profile.live_state_count, 0);
        assert!((resumed_profile.redemption_leg_pv - direct_quote.zero_coupon_pv).abs() < 1.0e-9);
        assert!(
            (resumed_profile.coupon_annuity_pv - direct_quote.unit_coupon_sensitivity).abs()
                < 1.0e-9
        );
        assert!(
            (resumed_profile.expected_life_days - direct_quote.expected_life_days).abs() < 1.0e-9
        );
        assert!((resumed_profile.knock_in_rate - direct_quote.knock_in_rate).abs() < 1.0e-9);
        assert!((resumed_profile.autocall_rate - direct_quote.autocall_rate).abs() < 1.0e-9);
    }

    #[test]
    #[ignore]
    fn deterministic_checkpoint_growth_snapshot() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma_common = 0.291_482_300_850_330_96;

        for max_observations in 1..=6 {
            let checkpoint = model
                .deterministic_checkpoint(sigma_common, max_observations)
                .unwrap();
            let profile = model.profile_deterministic_checkpoint(&checkpoint).unwrap();
            let live_probability_mass = checkpoint
                .live_states
                .iter()
                .map(|state| state.weight)
                .sum::<f64>();

            println!(
                "obs={} live_states={} peak_states={} live_mass={:.9} v0={:.9} u0={:.9}",
                max_observations,
                checkpoint.live_states.len(),
                checkpoint.peak_live_state_count,
                live_probability_mass,
                profile.redemption_leg_pv,
                profile.coupon_annuity_pv,
            );
        }
    }

    #[test]
    fn onchain_v1_quote_is_finite_and_monotone() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let config = FactoredWorstOfOnchainConfig::default();
        let low = model
            .quote_coupon_onchain_v1(0.291_482_300_850_330_96, config)
            .unwrap();
        let mid = model
            .quote_coupon_onchain_v1(0.364_352_876_062_913_67, config)
            .unwrap();
        let high = model
            .quote_coupon_onchain_v1(0.437_223_451_275_496_4, config)
            .unwrap();

        assert!(low.fair_coupon_bps.is_finite());
        assert!(mid.fair_coupon_bps.is_finite());
        assert!(high.fair_coupon_bps.is_finite());
        assert!(low.zero_coupon_pv.is_finite());
        assert!(mid.zero_coupon_pv.is_finite());
        assert!(high.zero_coupon_pv.is_finite());
        assert!(low.unit_coupon_pv.is_finite());
        assert!(mid.unit_coupon_pv.is_finite());
        assert!(high.unit_coupon_pv.is_finite());
        assert!(low.knock_in_rate.is_finite());
        assert!(mid.knock_in_rate.is_finite());
        assert!(high.knock_in_rate.is_finite());
        assert!(low.autocall_rate.is_finite());
        assert!(mid.autocall_rate.is_finite());
        assert!(high.autocall_rate.is_finite());

        assert!(low.fair_coupon_bps < mid.fair_coupon_bps);
        assert!(mid.fair_coupon_bps < high.fair_coupon_bps);
        assert!(low.zero_coupon_pv > high.zero_coupon_pv);
        assert!(low.expected_life_days > high.expected_life_days);
        assert!(low.zero_coupon_pv <= model.shell.notional + 1.0e-9);
        assert!(mid.zero_coupon_pv <= model.shell.notional + 1.0e-9);
        assert!(high.zero_coupon_pv <= model.shell.notional + 1.0e-9);
        assert!(low.unit_coupon_pv >= low.zero_coupon_pv);
        assert!(mid.unit_coupon_pv >= mid.zero_coupon_pv);
        assert!(high.unit_coupon_pv >= high.zero_coupon_pv);
    }

    #[test]
    #[ignore]
    fn onchain_v1_anchor_snapshot() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let config = FactoredWorstOfOnchainConfig::default();

        for sigma_common in [
            0.291_482_300_850_330_96,
            0.364_352_876_062_913_67,
            0.437_223_451_275_496_4,
        ] {
            let quote = model.quote_coupon_onchain_v1(sigma_common, config).unwrap();
            println!(
                "sigma={:.15} fair_bps={:.6} v0={:.9} u0={:.9} ki={:.9} ac={:.9} life={:.9}",
                sigma_common,
                quote.fair_coupon_bps,
                quote.zero_coupon_pv,
                quote.unit_coupon_sensitivity,
                quote.knock_in_rate,
                quote.autocall_rate,
                quote.expected_life_days,
            );
        }
    }

    #[test]
    #[ignore]
    fn onchain_v1_c1_anchor_snapshot() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        for sigma_common in [
            0.291_482_300_850_330_96,
            0.364_352_876_062_913_67,
            0.437_223_451_275_496_4,
        ] {
            let exact = model.quote_coupon(sigma_common).unwrap();
            println!(
                "exact sigma={:.15} fair_bps={:.6} v0={:.9} u0={:.9}",
                sigma_common,
                exact.fair_coupon_bps,
                exact.zero_coupon_pv,
                exact.unit_coupon_sensitivity,
            );
            for (label, config) in [
                (
                    "c1_t0",
                    FactoredWorstOfOnchainConfig {
                        factor_order: 9,
                        triangle_gl_order: 0,
                        ki_order: 0,
                        components_per_class: 1,
                    },
                ),
                (
                    "c1_t5",
                    FactoredWorstOfOnchainConfig {
                        factor_order: 7,
                        triangle_gl_order: 5,
                        ki_order: 5,
                        components_per_class: 1,
                    },
                ),
                (
                    "c1_t7",
                    FactoredWorstOfOnchainConfig {
                        factor_order: 9,
                        triangle_gl_order: 7,
                        ki_order: 7,
                        components_per_class: 1,
                    },
                ),
            ] {
                let approx = model.quote_coupon_onchain_v1(sigma_common, config).unwrap();
                println!(
                    "{label} sigma={:.15} fair_bps={:.6} err_bps={:.6} v0={:.9} u0={:.9} ki={:.9} ac={:.9}",
                    sigma_common,
                    approx.fair_coupon_bps,
                    approx.fair_coupon_bps - exact.fair_coupon_bps,
                    approx.zero_coupon_pv,
                    approx.unit_coupon_sensitivity,
                    approx.knock_in_rate,
                    approx.autocall_rate,
                );
            }
        }
    }

    #[test]
    fn onchain_v1_c1_t0_low_anchor_regression() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let quote = model
            .quote_coupon_onchain_v1(
                0.291_482_300_850_330_96,
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 0,
                    ki_order: 0,
                    components_per_class: 1,
                },
            )
            .unwrap();
        assert!(quote.fair_coupon_bps.is_finite());
        assert!(quote.zero_coupon_pv.is_finite());
        assert!(quote.unit_coupon_sensitivity.is_finite());
    }

    #[test]
    #[ignore]
    fn onchain_v1_c1_t0_anchor_obs_snapshot() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let config = FactoredWorstOfOnchainConfig {
            factor_order: 9,
            triangle_gl_order: 0,
            ki_order: 0,
            components_per_class: 1,
        };

        for sigma_common in [
            0.291_482_300_850_330_96,
            0.364_352_876_062_913_67,
            0.437_223_451_275_496_4,
        ] {
            let exact = model.quote_coupon(sigma_common).unwrap();
            let approx = model.quote_coupon_onchain_v1(sigma_common, config).unwrap();
            println!(
                "sigma={sigma_common:.15} exact_bps={:.6} c1_t0_bps={:.6}",
                exact.fair_coupon_bps, approx.fair_coupon_bps,
            );

            for index in 0..model.shell.observation_days.len() {
                let exact_marginal = exact.observation_marginals[index];
                let approx_marginal = approx.observation_marginals[index];
                println!(
                    "obs={} exact[surv={:.9} ac={:.9} ki1={:.9}] c1_t0[surv={:.9} ac={:.9} ki1={:.9}]",
                    model.shell.observation_days[index],
                    exact_marginal.survival_probability,
                    exact_marginal.autocall_first_hit_probability,
                    exact_marginal.first_knock_in_probability,
                    approx_marginal.survival_probability,
                    approx_marginal.autocall_first_hit_probability,
                    approx_marginal.first_knock_in_probability,
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn onchain_v1_vs_exact_component_snapshot() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma_common = 0.364_352_876_062_913_67;

        let exact = model.quote_coupon(sigma_common).unwrap();
        println!(
            "exact fair_bps={:.6} v0={:.9} u0={:.9} ki={:.9} ac={:.9} life={:.9}",
            exact.fair_coupon_bps,
            exact.zero_coupon_pv,
            exact.unit_coupon_sensitivity,
            exact.knock_in_rate,
            exact.autocall_rate,
            exact.expected_life_days,
        );

        for (label, config) in [
            ("default", FactoredWorstOfOnchainConfig::default()),
            (
                "c6_hi",
                FactoredWorstOfOnchainConfig {
                    factor_order: 13,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 6,
                },
            ),
        ] {
            let approx = model.quote_coupon_onchain_v1(sigma_common, config).unwrap();
            println!(
                "{label} fair_bps={:.6} v0={:.9} u0={:.9} ki={:.9} ac={:.9} life={:.9}",
                approx.fair_coupon_bps,
                approx.zero_coupon_pv,
                approx.unit_coupon_sensitivity,
                approx.knock_in_rate,
                approx.autocall_rate,
                approx.expected_life_days,
            );

            for index in 0..model.shell.observation_days.len() {
                let exact_marginal = exact.observation_marginals[index];
                let approx_marginal = approx.observation_marginals[index];
                println!(
                    "{label} obs={} exact[surv={:.9} ac={:.9} ki1={:.9} coup={:.9}] approx[surv={:.9} ac={:.9} ki1={:.9} coup={:.9}]",
                    model.shell.observation_days[index],
                    exact_marginal.survival_probability,
                    exact_marginal.autocall_first_hit_probability,
                    exact_marginal.first_knock_in_probability,
                    exact_marginal.coupon_annuity_contribution,
                    approx_marginal.survival_probability,
                    approx_marginal.autocall_first_hit_probability,
                    approx_marginal.first_knock_in_probability,
                    approx_marginal.coupon_annuity_contribution,
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn onchain_v1_continuation_component_snapshot() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma_common = 0.364_352_876_062_913_67;
        let exact = model.quote_coupon(sigma_common).unwrap();
        println!(
            "exact fair_bps={:.6} v0={:.9} u0={:.9} ki={:.9} ac={:.9} life={:.9}",
            exact.fair_coupon_bps,
            exact.zero_coupon_pv,
            exact.unit_coupon_sensitivity,
            exact.knock_in_rate,
            exact.autocall_rate,
            exact.expected_life_days,
        );

        for (label, config) in [
            (
                "c2",
                FactoredWorstOfOnchainConfig {
                    components_per_class: 2,
                    ..FactoredWorstOfOnchainConfig::default()
                },
            ),
            (
                "c3",
                FactoredWorstOfOnchainConfig {
                    components_per_class: 3,
                    ..FactoredWorstOfOnchainConfig::default()
                },
            ),
            (
                "c3_hi",
                FactoredWorstOfOnchainConfig {
                    factor_order: 13,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 3,
                },
            ),
            (
                "c4",
                FactoredWorstOfOnchainConfig {
                    components_per_class: 4,
                    ..FactoredWorstOfOnchainConfig::default()
                },
            ),
            (
                "c6",
                FactoredWorstOfOnchainConfig {
                    components_per_class: 6,
                    ..FactoredWorstOfOnchainConfig::default()
                },
            ),
        ] {
            let (observation_marginals, result) = model
                .build_onchain_v1_trace_with_continuation(
                    sigma_common,
                    config,
                    model.shell.observation_days.len(),
                )
                .unwrap();
            let approx =
                model.quote_from_trace_result(sigma_common, observation_marginals, result.trace);
            println!(
                "{label} fair_bps={:.6} v0={:.9} u0={:.9} ki={:.9} ac={:.9} life={:.9}",
                approx.fair_coupon_bps,
                approx.zero_coupon_pv,
                approx.unit_coupon_sensitivity,
                approx.knock_in_rate,
                approx.autocall_rate,
                approx.expected_life_days,
            );
        }
    }

    #[test]
    #[ignore]
    fn onchain_v1_enriched_anchor_snapshot() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        for (label, config) in [
            (
                "c3_hi",
                FactoredWorstOfOnchainConfig {
                    factor_order: 13,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 3,
                },
            ),
            (
                "c5_hi",
                FactoredWorstOfOnchainConfig {
                    factor_order: 13,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 5,
                },
            ),
            (
                "c6_hi",
                FactoredWorstOfOnchainConfig {
                    factor_order: 13,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 6,
                },
            ),
        ] {
            for sigma_common in [
                0.291_482_300_850_330_96,
                0.364_352_876_062_913_67,
                0.437_223_451_275_496_4,
            ] {
                let exact = model.quote_coupon(sigma_common).unwrap();
                let (observation_marginals, result) = model
                    .build_onchain_v1_trace_with_continuation(
                        sigma_common,
                        config,
                        model.shell.observation_days.len(),
                    )
                    .unwrap();
                let approx = model.quote_from_trace_result(
                    sigma_common,
                    observation_marginals,
                    result.trace,
                );
                println!(
                    "{label} sigma={:.15} exact_bps={:.6} approx_bps={:.6} err_bps={:.6} exact_v0={:.9} approx_v0={:.9} exact_u0={:.9} approx_u0={:.9}",
                    sigma_common,
                    exact.fair_coupon_bps,
                    approx.fair_coupon_bps,
                    approx.fair_coupon_bps - exact.fair_coupon_bps,
                    exact.zero_coupon_pv,
                    approx.zero_coupon_pv,
                    exact.unit_coupon_sensitivity,
                    approx.unit_coupon_sensitivity,
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn onchain_v1_enriched_high_sigma_component_sweep() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma_common = 0.437_223_451_275_496_4;
        let exact = model.quote_coupon(sigma_common).unwrap();
        println!(
            "exact fair_bps={:.6} v0={:.9} u0={:.9} ki={:.9} ac={:.9} life={:.9}",
            exact.fair_coupon_bps,
            exact.zero_coupon_pv,
            exact.unit_coupon_sensitivity,
            exact.knock_in_rate,
            exact.autocall_rate,
            exact.expected_life_days,
        );

        for components_per_class in [3u8, 4, 5, 6] {
            let config = FactoredWorstOfOnchainConfig {
                factor_order: 13,
                triangle_gl_order: 20,
                ki_order: 13,
                components_per_class,
            };
            let (observation_marginals, result) = model
                .build_onchain_v1_trace_with_continuation(
                    sigma_common,
                    config,
                    model.shell.observation_days.len(),
                )
                .unwrap();
            let approx =
                model.quote_from_trace_result(sigma_common, observation_marginals, result.trace);
            println!(
                "c{components_per_class}_hi fair_bps={:.6} err_bps={:.6} v0={:.9} u0={:.9} ki={:.9} ac={:.9} life={:.9}",
                approx.fair_coupon_bps,
                approx.fair_coupon_bps - exact.fair_coupon_bps,
                approx.zero_coupon_pv,
                approx.unit_coupon_sensitivity,
                approx.knock_in_rate,
                approx.autocall_rate,
                approx.expected_life_days,
            );
        }
    }

    #[test]
    #[ignore]
    fn onchain_v1_lean_config_anchor_sweep() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        for (label, config) in [
            (
                "c2_hi",
                FactoredWorstOfOnchainConfig {
                    factor_order: 13,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 2,
                },
            ),
            (
                "c3_hi",
                FactoredWorstOfOnchainConfig {
                    factor_order: 13,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 3,
                },
            ),
            (
                "c4_hi",
                FactoredWorstOfOnchainConfig {
                    factor_order: 13,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 4,
                },
            ),
            (
                "c4_t7",
                FactoredWorstOfOnchainConfig {
                    factor_order: 13,
                    triangle_gl_order: 7,
                    ki_order: 13,
                    components_per_class: 4,
                },
            ),
            (
                "c4_f9",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 4,
                },
            ),
            (
                "c4_f9_k9",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 20,
                    ki_order: 9,
                    components_per_class: 4,
                },
            ),
            (
                "c4_f9_t7",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 7,
                    ki_order: 13,
                    components_per_class: 4,
                },
            ),
            (
                "c4_f9_t7_k9",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 7,
                    ki_order: 9,
                    components_per_class: 4,
                },
            ),
            (
                "c2_med",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 7,
                    ki_order: 7,
                    components_per_class: 2,
                },
            ),
            (
                "c2_lo",
                FactoredWorstOfOnchainConfig {
                    factor_order: 7,
                    triangle_gl_order: 5,
                    ki_order: 5,
                    components_per_class: 2,
                },
            ),
            (
                "c3_med",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 7,
                    ki_order: 7,
                    components_per_class: 3,
                },
            ),
            (
                "c3_lo",
                FactoredWorstOfOnchainConfig {
                    factor_order: 7,
                    triangle_gl_order: 5,
                    ki_order: 5,
                    components_per_class: 3,
                },
            ),
        ] {
            for sigma_common in [
                0.291_482_300_850_330_96,
                0.364_352_876_062_913_67,
                0.437_223_451_275_496_4,
            ] {
                let exact = model.quote_coupon(sigma_common).unwrap();
                let (observation_marginals, result) = model
                    .build_onchain_v1_trace_with_continuation(
                        sigma_common,
                        config,
                        model.shell.observation_days.len(),
                    )
                    .unwrap();
                let approx = model.quote_from_trace_result(
                    sigma_common,
                    observation_marginals,
                    result.trace,
                );
                println!(
                    "{label} sigma={:.15} exact_bps={:.6} approx_bps={:.6} err_bps={:.6} exact_v0={:.9} approx_v0={:.9} exact_u0={:.9} approx_u0={:.9}",
                    sigma_common,
                    exact.fair_coupon_bps,
                    approx.fair_coupon_bps,
                    approx.fair_coupon_bps - exact.fair_coupon_bps,
                    exact.zero_coupon_pv,
                    approx.zero_coupon_pv,
                    exact.unit_coupon_sensitivity,
                    approx.unit_coupon_sensitivity,
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn t0_mills_ratio_vs_gl_accuracy() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigmas = [
            ("low", 0.291_482_300_850_330_96_f64),
            ("mid", 0.364_352_876_062_913_67_f64),
            ("high", 0.437_223_451_275_496_4_f64),
        ];

        let configs: Vec<(&str, FactoredWorstOfOnchainConfig)> = vec![
            (
                "c4_f9_t20_k9",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 20,
                    ki_order: 9,
                    components_per_class: 4,
                },
            ),
            (
                "c4_f9_t0_k0",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 0,
                    ki_order: 0,
                    components_per_class: 4,
                },
            ),
            (
                "c3_f9_t20_k9",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 20,
                    ki_order: 9,
                    components_per_class: 3,
                },
            ),
            (
                "c3_f9_t0_k0",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 0,
                    ki_order: 0,
                    components_per_class: 3,
                },
            ),
            (
                "c2_f9_t20_k9",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 20,
                    ki_order: 9,
                    components_per_class: 2,
                },
            ),
            (
                "c2_f9_t0_k0",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 0,
                    ki_order: 0,
                    components_per_class: 2,
                },
            ),
            (
                "c1_f9_t0_k0",
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 0,
                    ki_order: 0,
                    components_per_class: 1,
                },
            ),
        ];

        for &(sigma_label, sigma) in &sigmas {
            let exact = model.quote_coupon(sigma).unwrap();
            println!(
                "\nsigma={sigma_label} ({sigma:.4}) exact_bps={:.2}",
                exact.fair_coupon_bps
            );

            for (label, config) in &configs {
                match model.build_onchain_v1_trace_with_continuation(
                    sigma,
                    *config,
                    model.shell.observation_days.len(),
                ) {
                    Ok((obs, result)) => {
                        let approx = model.quote_from_trace_result(sigma, obs, result.trace);
                        let err = approx.fair_coupon_bps - exact.fair_coupon_bps;
                        println!(
                            "  {label:>16}  fc={:.2}  err={:+.1} bps  ki={:.4}  ac={:.4}",
                            approx.fair_coupon_bps, err, approx.knock_in_rate, approx.autocall_rate
                        );
                    }
                    Err(e) => {
                        println!("  {label:>16}  FAILED: {e:?}");
                    }
                }
            }
        }
    }

    #[test]
    #[ignore]
    fn t0_vs_gl20_obs1_diagnostic() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma = 0.291_482_300_850_330_96_f64;

        for max_obs in 1..=6 {
            for &(label, config) in &[
                (
                    "t20",
                    FactoredWorstOfOnchainConfig {
                        factor_order: 9,
                        triangle_gl_order: 20,
                        ki_order: 9,
                        components_per_class: 2,
                    },
                ),
                (
                    "t0 ",
                    FactoredWorstOfOnchainConfig {
                        factor_order: 9,
                        triangle_gl_order: 0,
                        ki_order: 0,
                        components_per_class: 2,
                    },
                ),
            ] {
                match model.build_onchain_v1_trace_with_continuation(sigma, config, max_obs) {
                    Ok((obs, result)) => {
                        let m = &obs[max_obs - 1];
                        println!(
                            "{label} obs={max_obs} surv={:.6} ac_hit={:.6} ki_1st={:.6} ac_p={:.6} ki_safe_p={:.6}",
                            m.survival_probability,
                            m.autocall_first_hit_probability,
                            m.first_knock_in_probability,
                            m.autocall_probability,
                            m.knock_in_safe_probability,
                        );
                    }
                    Err(e) => println!("{label} obs={max_obs} FAILED: {e:?}"),
                }
            }
        }
    }

    #[test]
    #[ignore]
    fn t0_vs_gl20_probability_node0() {
        use solmath_core::SCALE_I;
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma = 0.291_482_300_850_330_96_f64;
        let step = model.shell.observation_days[0];
        let nodes_fast = model.conditional_factor_nodes_fast(sigma, step).unwrap();
        let nodes_gl = model
            .conditional_factor_nodes_with_order(sigma, step, 9)
            .unwrap();

        println!(
            "fast node count={}, gl node count={}",
            nodes_fast.len(),
            nodes_gl.len()
        );

        // Compare node properties
        for k in 0..nodes_fast.len().min(nodes_gl.len()) {
            let nf = &nodes_fast[k];
            let ng = &nodes_gl[k];
            println!(
                "node {k}: fast w={:.9} val={:.6} mean=({:.6},{:.6}) cov=({:.6},{:.6},{:.6})",
                nf.weight,
                nf.value,
                nf.mean[0],
                nf.mean[1],
                nf.covariance[0][0],
                nf.covariance[0][1],
                nf.covariance[1][1],
            );
            println!(
                "         gl   w={:.9} val={:.6} mean=({:.6},{:.6}) cov=({:.6},{:.6},{:.6})",
                ng.weight,
                ng.value,
                ng.mean[0],
                ng.mean[1],
                ng.covariance[0][0],
                ng.covariance[0][1],
                ng.covariance[1][1],
            );
        }

        // Compare triangle probabilities at a single node
        let node = &nodes_fast[4]; // center node
        let mean = node.mean;
        let covariance = node.covariance;
        let autocall_planes = model
            .shifted_halfplanes(model.autocall_halfplanes, sigma, step, node.value)
            .unwrap();
        let ki_safe_planes = model
            .shifted_halfplanes(model.knock_in_safe_halfplanes, sigma, step, node.value)
            .unwrap();

        // t0 probability
        let p_ac_t0 = model
            .triangle_probability_explicit_with_order(mean, covariance, autocall_planes, 0, None)
            .unwrap();
        let p_ac_gl20 = model
            .triangle_probability_explicit_with_order(mean, covariance, autocall_planes, 20, None)
            .unwrap();
        let p_ki_t0 = model
            .triangle_probability_explicit_with_order(mean, covariance, ki_safe_planes, 0, None)
            .unwrap();
        let p_ki_gl20 = model
            .triangle_probability_explicit_with_order(mean, covariance, ki_safe_planes, 20, None)
            .unwrap();

        println!("\nCenter node (k=4):");
        println!(
            "  autocall: t0={:.9} gl20={:.9} ratio={:.4}",
            p_ac_t0,
            p_ac_gl20,
            p_ac_t0 / p_ac_gl20
        );
        println!(
            "  ki_safe:  t0={:.9} gl20={:.9} ratio={:.4}",
            p_ki_t0,
            p_ki_gl20,
            p_ki_t0 / p_ki_gl20
        );

        // Per-node comparison with GH3 correction
        println!("\n  Per-node autocall: ie vs gl20 vs GH3-corrected:");
        {
            use crate::worst_of_c1_fast::{build_triple_correction_pre, triple_complement_gh3};
            use solmath_core::worst_of_ki_i64::cholesky6;
            let pre = &TRIANGLE_PRE64_63;
            let cov_s6 = [
                (covariance[0][0] * 1e6) as i64,
                (covariance[0][1] * 1e6) as i64,
                (covariance[1][1] * 1e6) as i64,
            ];
            let (l11, l21, l22) = cholesky6(cov_s6[0], cov_s6[1], cov_s6[2]).unwrap();
            let tcp = build_triple_correction_pre(l11, l21, l22, &pre.au, &pre.av);
            let s6: i64 = 1_000_000;
            let mut total_ie = 0.0_f64;
            let mut total_gh3 = 0.0_f64;
            let mut total_gl20 = 0.0_f64;
            for k in 0..nodes_fast.len() {
                let n = &nodes_fast[k];
                let ac_k = model
                    .shifted_halfplanes(model.autocall_halfplanes, sigma, step, n.value)
                    .unwrap();
                let p_ie = model
                    .triangle_probability_explicit_with_order(n.mean, n.covariance, ac_k, 0, None)
                    .unwrap();
                let p_gl20 = model
                    .triangle_probability_explicit_with_order(n.mean, n.covariance, ac_k, 20, None)
                    .unwrap();
                let mu6 = (n.mean[0] * 1e6) as i64;
                let mv6 = (n.mean[1] * 1e6) as i64;
                let mut num6 = [0i64; 3];
                for j in 0..3 {
                    let rhs6 = (ac_k[j].rhs * 1e6) as i64;
                    let ew6 = (pre.au[j] as i64 * mu6 + pre.av[j] as i64 * mv6) / s6;
                    num6[j] = rhs6 - ew6;
                }
                let phi3 = triple_complement_gh3(&tcp, num6) as f64 / 1e6;
                let p_gh3 = (p_ie - phi3).max(0.0);
                total_ie += n.weight * p_ie;
                total_gh3 += n.weight * p_gh3;
                total_gl20 += n.weight * p_gl20;
                if n.weight > 0.01 || (p_ie - p_gl20).abs() > 0.001 {
                    println!(
                        "    k={k} w={:.4} fv={:+.3} gl20={:.6} ie={:.6} gh3={:.6} phi3={:.6}",
                        n.weight, n.value, p_gl20, p_ie, p_gh3, phi3
                    );
                }
            }
            println!(
                "    TOTAL: gl20={:.6} ie={:.6} gh3={:.6}",
                total_gl20, total_ie, total_gh3
            );
        }

        // Trace: compare z-values from TrianglePre64 path vs GL20 raw half-planes
        println!("\n  Autocall planes at center node:");
        {
            use solmath_core::{bvn_cdf_i64, norm_cdf_i64};
            let pre = &TRIANGLE_PRE64_63;
            let s6: i64 = 1_000_000;
            let shift: i64 = 1_000_000;
            let mu6 = (mean[0] * s6 as f64) as i64;
            let mv6 = (mean[1] * s6 as f64) as i64;

            for k in 0..3 {
                // f64 z from raw half-plane geometry and actual covariance
                let a_u = autocall_planes[k].a_u;
                let a_v = autocall_planes[k].a_v;
                let rhs = autocall_planes[k].rhs;
                let ew = a_u * mean[0] + a_v * mean[1];
                let var_w = a_u * a_u * covariance[0][0]
                    + 2.0 * a_u * a_v * covariance[0][1]
                    + a_v * a_v * covariance[1][1];
                let std_w = var_w.sqrt();
                let z_f64 = (rhs - ew) / std_w;

                // i64 z from TrianglePre64 path
                let rhs6 = (rhs * s6 as f64) as i64;
                let ew6 = (pre.au[k] as i64 * mu6 + pre.av[k] as i64 * mv6) / s6;
                let num6 = rhs6 - ew6;
                let z_i64 = num6 as i64 * pre.inv_std[k] as i64 / s6;

                println!(
                    "    plane {k}: rhs={rhs:.6} ew_f64={ew:.6} std_w={std_w:.6} z_f64={z_f64:.6} | z_i64={} ({:.6})",
                    z_i64, z_i64 as f64 / s6 as f64,
                );
                println!(
                    "      pre.au={} pre.av={} pre.inv_std={} | plane.a_u={a_u:.6} plane.a_v={a_v:.6}",
                    pre.au[k], pre.av[k], pre.inv_std[k],
                );
            }

            // Now trace the full I-E
            let phi2_tables: [&[[i32; 64]; 64]; 3] = [
                &PHI2_RESID_SPY_QQQ,
                &PHI2_RESID_SPY_IWM,
                &PHI2_RESID_QQQ_IWM,
            ];
            let mut z_scale = [0i64; 3];
            let mut phi_z = [0i64; 3];
            for k in 0..3 {
                let rhs6 = (autocall_planes[k].rhs * s6 as f64) as i64;
                let ew6 = (pre.au[k] as i64 * mu6 + pre.av[k] as i64 * mv6) / s6;
                let z6 = (rhs6 - ew6) as i64 * pre.inv_std[k] as i64 / s6;
                z_scale[k] = z6 * shift;
                phi_z[k] = norm_cdf_i64(z_scale[k]);
            }
            let sum_c = (s6 - phi_z[0]) + (s6 - phi_z[1]) + (s6 - phi_z[2]);
            println!(
                "    phi_z=[{},{},{}] sum_compl={sum_c}",
                phi_z[0], phi_z[1], phi_z[2]
            );

            let pairs: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];
            let mut sp: i64 = 0;
            for (pidx, &(i, j)) in pairs.iter().enumerate() {
                let phi2 = if pre.phi2_neg[pidx] {
                    let a = norm_cdf_i64(-z_scale[i]);
                    let b = bvn_cdf_i64(-z_scale[i], z_scale[j], phi2_tables[pidx]);
                    let v = (a - b).max(0);
                    println!("    pair({i},{j}) neg: Phi(-z_i)={a} Phi2(-z_i,z_j)={b} -> {v}");
                    v
                } else {
                    let v = bvn_cdf_i64(-z_scale[i], -z_scale[j], phi2_tables[pidx]);
                    println!("    pair({i},{j}) pos: Phi2(-z_i,-z_j)={v}");
                    v
                };
                sp += phi2;
            }
            let prob = (s6 - sum_c + sp).clamp(0, s6);
            println!(
                "    sum_pair={sp} prob={prob} = {:.9}",
                prob as f64 / s6 as f64
            );

            // GH3 triple correction
            {
                use crate::worst_of_c1_fast::{build_triple_correction_pre, triple_complement_gh3};
                use solmath_core::worst_of_ki_i64::cholesky6;
                let cov_uu_s6 = (covariance[0][0] * s6 as f64) as i64;
                let cov_uv_s6 = (covariance[0][1] * s6 as f64) as i64;
                let cov_vv_s6 = (covariance[1][1] * s6 as f64) as i64;
                let (l11, l21, l22) = cholesky6(cov_uu_s6, cov_uv_s6, cov_vv_s6).unwrap();
                println!("    Cholesky: l11={l11} l21={l21} l22={l22}");
                let tcp = build_triple_correction_pre(l11, l21, l22, &pre.au, &pre.av);
                println!(
                    "    Triple pre: slopes=[{},{},{}] is_upper=[{},{},{}]",
                    tcp.slope[0],
                    tcp.slope[1],
                    tcp.slope[2],
                    tcp.is_upper[0],
                    tcp.is_upper[1],
                    tcp.is_upper[2]
                );
                let mut num6_arr = [0i64; 3];
                for k in 0..3 {
                    let rhs6 = (autocall_planes[k].rhs * s6 as f64) as i64;
                    let ew6 = (pre.au[k] as i64 * mu6 + pre.av[k] as i64 * mv6) / s6;
                    num6_arr[k] = rhs6 - ew6;
                }
                let phi3 = triple_complement_gh3(&tcp, num6_arr);
                let corrected = (prob - phi3).max(0);
                println!(
                    "    GH3 Phi3={phi3} ({:.9}) corrected={corrected} ({:.9})",
                    phi3 as f64 / s6 as f64,
                    corrected as f64 / s6 as f64
                );
            }

            // Inner triangle (triple complement): use POSITIVE z values
            // Φ₂(z_i, z_j; ρ) for the inner I-E
            let sum_c_inner = phi_z[0] + phi_z[1] + phi_z[2]; // Σ Φ(z_k)
            let mut sp_inner: i64 = 0;
            for (pidx, &(i, j)) in pairs.iter().enumerate() {
                let phi2 = if pre.phi2_neg[pidx] {
                    // Φ₂(z_i, z_j; -|ρ|) = Φ(z_i) - Φ₂(z_i, -z_j; |ρ|)
                    let phi_a = norm_cdf_i64(z_scale[i]);
                    let b = bvn_cdf_i64(z_scale[i], -z_scale[j], phi2_tables[pidx]);
                    let v = (phi_a - b).max(0);
                    println!(
                        "    inner pair({i},{j}) neg: Phi(z_i)={phi_a} Phi2(z_i,-z_j)={b} -> {v}"
                    );
                    v
                } else {
                    let v = bvn_cdf_i64(z_scale[i], z_scale[j], phi2_tables[pidx]);
                    println!("    inner pair({i},{j}) pos: Phi2(z_i,z_j)={v}");
                    v
                };
                sp_inner += phi2;
            }
            let inner = (s6 - sum_c_inner + sp_inner).clamp(0, s6);
            println!(
                "    inner: sum_compl={sum_c_inner} sum_pair={sp_inner} inner_prob={inner} ({:.9})",
                inner as f64 / s6 as f64
            );
            println!(
                "    corrected: outer - inner = {} ({:.9})",
                (prob - inner).max(0),
                (prob - inner).max(0) as f64 / s6 as f64
            );

            // Also call the actual function for the inner triangle
            // Negate mean and rhs to get -z values
            let inner_rhs: [i128; 3] = [
                -(autocall_planes[0].rhs * 1e12) as i128,
                -(autocall_planes[1].rhs * 1e12) as i128,
                -(autocall_planes[2].rhs * 1e12) as i128,
            ];
            let inner_mean_u = -(mean[0] * 1e12) as i128;
            let inner_mean_v = -(mean[1] * 1e12) as i128;
            let inner_fn = triangle_probability_i64_fp(
                inner_mean_u,
                inner_mean_v,
                inner_rhs,
                &TRIANGLE_PRE64_63,
                PHI2_TABLES,
            );
            let inner_fn_s6 = (inner_fn / 1_000_000) as i64;
            println!(
                "    inner via function: {} ({:.9})",
                inner_fn_s6,
                inner_fn_s6 as f64 / s6 as f64
            );
            println!(
                "    corrected via fn: {} ({:.9})",
                (prob - inner_fn_s6).max(0),
                (prob - inner_fn_s6).max(0) as f64 / s6 as f64
            );
        }
    }
}
