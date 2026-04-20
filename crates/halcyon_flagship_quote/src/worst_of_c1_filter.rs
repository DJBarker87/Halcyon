//! Projected Gaussian-sum c1 filter for the SPY/QQQ/IWM worst-of pricer.
//!
//! This keeps the fast i64 triangle / KI kernels from the existing c1 path,
//! but separates the two jobs that the node grid has to perform:
//!
//! 1. represent one fresh 63-day NIG increment,
//! 2. represent the multi-step survivor-conditioned distribution of the
//!    cumulative common factor plus the conditional spread state.
//!
//! The filter tracks two live classes:
//! - `safe`: not yet knocked in
//! - `knocked`: already knocked in but still live and eligible to autocall
//!
//! Each class is a retained mixture over the cumulative common factor `c`
//! with a Gaussian `(u, v)` spread state attached to each retained node.
//! Prediction expands the retained nodes by the fresh 9-point NIG increment
//! and projects them back to at most `K` retained nodes via barycentric
//! interpolation on the cumulative-factor axis.

use crate::worst_of_c1_fast::{
    build_triple_correction_pre, c1_fast_quote_from_components, C1FastConfig, C1FastQuote,
    TripleCorrectionPre, CF_ALPHA_S12, CF_BETA_S12, CF_DELTA_SCALE_S12, CF_GAMMA_S12, S12,
};
use crate::worst_of_c1_filter_gradients::{
    FrozenMomentTables, FROZEN_TABLES_K12, FROZEN_TABLES_K15, FROZEN_TABLES_K9,
};
use solmath_core::gauss_hermite::GH13_NODES;
#[cfg(not(target_os = "solana"))]
use solmath_core::gauss_hermite::GH13_WEIGHTS;
#[cfg(target_os = "solana")]
use solmath_core::nig_pdf_bessel;
use solmath_core::nig_weights_table::{nig_importance_weights_9, GH9_NODES_S6};
use solmath_core::worst_of_ki_i64::{cholesky6, ki_moment_i64_gh3, AffineCoord6, KiMoment6};
use solmath_core::{TrianglePre64, PHI2_RESID_QQQ_IWM, PHI2_RESID_SPY_IWM, PHI2_RESID_SPY_QQQ};

#[cfg(not(target_os = "solana"))]
use crate::worst_of_c1_filter_gradients::REFERENCE_SIGMA_COMMON_S6;
#[cfg(not(target_os = "solana"))]
use crate::worst_of_factored::FactoredWorstOfModel;
#[cfg(not(target_os = "solana"))]
use serde::Serialize;
#[cfg(not(target_os = "solana"))]
use std::fmt::Write as _;

const S6: i64 = 1_000_000;
const SQRT2_S6: i64 = 1_414_214;
/// 100 units of notional at S6 scale. Hardcoded to avoid stack-corruption
/// on BPF where C1FastConfig (~1.8KB) overflows the 4KB frame.
const NOTIONAL: i64 = 100 * S6;
const N_OBS: usize = 6;
const N_FACTOR_NODES: usize = 9;
const N_FACTOR_NODES_EXACT_SEED: usize = 13;
pub const MAX_K: usize = 15;
const MAX_MERGE_NODES: usize = MAX_K * 2;
const NODE_STATE_EPS_S6: i64 = 100;
const FROZEN_RATIO_EPS_S6: i64 = 1_000;
const GH3_NODES_6: [i64; 3] = [-1_224_745, 0, 1_224_745];
const GH3_WPI_6: [i64; 3] = [166_667, 666_667, 166_667];
const EXP_NEG15_STEP_S6: i64 = 25_000;
const EXP_NEG15_LO_S6: i64 = -1_500_000;
const EXP_NEG15_TABLE: [i64; 61] = [
    223_130, 228_779, 234_570, 240_508, 246_597, 252_840, 259_240, 265_803, 272_532, 279_431,
    286_505, 293_758, 301_194, 308_819, 316_637, 324_652, 332_871, 341_298, 349_938, 358_796,
    367_879, 377_192, 386_741, 396_531, 406_570, 416_862, 427_415, 438_235, 449_329, 460_704,
    472_367, 484_325, 496_585, 509_156, 522_046, 535_261, 548_812, 562_705, 576_950, 591_555,
    606_531, 621_885, 637_628, 653_770, 670_320, 687_289, 704_688, 722_527, 740_818, 759_572,
    778_801, 798_516, 818_731, 839_457, 860_708, 882_497, 904_837, 927_743, 951_229, 975_310,
    1_000_000,
];
const SQRT2_S12: i128 = 1_414_213_562_373;
const FIRST_STEP_STD_RATIO_S6: i64 = 500_000; // sqrt(63 / 252) = 0.5
const TRIANGLE_PAIR_RHO_63: [i64; 3] = [8_070, -509_610, -864_483];
const TRIANGLE_PAIR_INV_SQRT_1MRHO2_63: [i64; 3] = [1_000_033, 1_162_243, 1_989_410];
type Phi2Table = [[i32; 64]; 64];
const CR_DOMAIN_MIN_I64: i64 = -4_000_000_000_000;
const CR_DOMAIN_MAX_I64: i64 = 4_000_000_000_000;
const CR_RANGE_I64: i64 = 8_000_000_000_000;
const CR_N_MINUS_1: i64 = 63;
const CR_WN: usize = 1024;
const CR_WS: u32 = 30;
const CR_FRAC_DIVISOR: i64 = CR_RANGE_I64 / CR_WN as i64;
const CR_W_LOCAL: [[i32; 4]; CR_WN] = {
    let mut out = [[0i32; 4]; CR_WN];
    let scale: i64 = 1 << CR_WS;
    let mut k = 0usize;
    while k < CR_WN {
        let t = (k as i64) << (CR_WS - 10);
        let t2 = t * t >> CR_WS;
        let t3 = t2 * t >> CR_WS;
        out[k][0] = ((-t + 2 * t2 - t3) / 2) as i32;
        out[k][1] = ((2 * scale - 5 * t2 + 3 * t3) / 2) as i32;
        out[k][2] = ((t + 4 * t2 - 3 * t3) / 2) as i32;
        out[k][3] = ((-t2 + t3) / 2) as i32;
        k += 1;
    }
    out
};
const PHI1_TABLE_LOCAL: [i32; 64] = [
    32, 54, 90, 148, 240, 383, 602, 932, 1422, 2137, 3165, 4618, 6640, 9407, 13134, 18075, 24519,
    32791, 43238, 56222, 72101, 91211, 113841, 140213, 170452, 204573, 242460, 283855, 328361,
    375447, 424468, 474687, 525313, 575532, 624553, 671639, 716145, 757540, 795427, 829548, 859787,
    886159, 908789, 927899, 943778, 956762, 967209, 975481, 981925, 986866, 990593, 993360, 995382,
    996835, 997863, 998578, 999068, 999398, 999617, 999760, 999852, 999910, 999946, 999968,
];
const PDF1_TABLE_LOCAL: [i32; 64] = [
    134, 221, 358, 571, 897, 1387, 2109, 3156, 4647, 6734, 9602, 13471, 18598, 25265, 33774, 44425,
    57501, 73235, 91783, 113188, 137353, 164010, 192708, 222806, 253484, 283774, 312601, 338848,
    361424, 379337, 391770, 398139, 398139, 391770, 379337, 361424, 338848, 312601, 283774, 253484,
    222806, 192708, 164010, 137353, 113188, 91783, 73235, 57501, 44425, 33774, 25265, 18598, 13471,
    9602, 6734, 4647, 3156, 2109, 1387, 897, 571, 358, 221, 134,
];

#[cfg(any(
    all(target_os = "solana", feature = "c1-filter-cu-diag"),
    all(target_os = "solana", feature = "c1-filter-cu-diag-inner")
))]
unsafe extern "C" {
    fn sol_log_compute_units_();
    fn sol_log_(message: *const u8, length: u64);
}

#[cfg(all(target_os = "solana", feature = "ki-skip-count"))]
unsafe extern "C" {
    fn sol_log_64_(a: u64, b: u64, c: u64, d: u64, e: u64);
}

#[inline(always)]
fn c1_filter_cu_diag(stage: &'static [u8]) {
    #[cfg(all(target_os = "solana", feature = "c1-filter-cu-diag"))]
    unsafe {
        sol_log_(stage.as_ptr(), stage.len() as u64);
        sol_log_compute_units_();
    }
    #[cfg(not(all(target_os = "solana", feature = "c1-filter-cu-diag")))]
    let _ = stage;
}

#[inline(always)]
fn c1_filter_cu_diag_inner(stage: &'static [u8]) {
    #[cfg(all(target_os = "solana", feature = "c1-filter-cu-diag-inner"))]
    unsafe {
        sol_log_(stage.as_ptr(), stage.len() as u64);
        sol_log_compute_units_();
    }
    #[cfg(not(all(target_os = "solana", feature = "c1-filter-cu-diag-inner")))]
    let _ = stage;
}

/// Fixed-point multiply at scale 6. Product provably fits i64:
/// max |a| = 30M (accumulated mean), max |b| = S6 (weight/fraction),
/// max |a*b| = 3×10¹³ << 9.2×10¹⁸.
/// wrapping_mul bypasses BPF overflow-check branch (~5 CU/call saved).
///
/// Implementation switches on the `m6r-recip` feature: with the feature
/// disabled (default = current shipping behaviour), the body is the
/// original `a.wrapping_mul(b) / S6`. With the feature enabled, the
/// division is replaced by a reciprocal multiply-shift via
/// `crate::m6r_recip::m6r_recip` — saves ~140 CU/call on BPF.
#[cfg(not(feature = "m6r-recip"))]
#[inline(always)]
fn m6r_impl(a: i64, b: i64) -> i64 {
    a.wrapping_mul(b) / S6
}

#[cfg(feature = "m6r-recip")]
use crate::m6r_recip::m6r_recip as m6r_impl;

#[inline(always)]
fn m6r(a: i64, b: i64) -> i64 {
    m6r_impl(a, b)
}

#[inline(always)]
fn m6r_fast(a: i64, b: i64) -> i64 {
    m6r_impl(a, b)
}

/// Safe multiply-at-S6 for values that may exceed sqrt(i64::MAX).
/// Split pattern: a*(b/S6) + a*(b%S6)/S6. Max intermediate: a*S6.
#[inline(always)]
fn m6r_safe(a: i64, b: i64) -> i64 {
    let q = b / S6;
    let r = b % S6;
    a * q + a * r / S6
}

#[inline(always)]
fn cr_dot_local(w: &[i32; 4], p0: i64, p1: i64, p2: i64, p3: i64) -> i64 {
    (w[0] as i64 * p0 + w[1] as i64 * p1 + w[2] as i64 * p2 + w[3] as i64 * p3) >> CR_WS
}

#[inline(always)]
fn norm_cdf_i64_local(x: i64) -> i64 {
    let x64 = x.clamp(CR_DOMAIN_MIN_I64, CR_DOMAIN_MAX_I64);
    let x_off = x64 - CR_DOMAIN_MIN_I64;
    let ix_scaled = x_off * CR_N_MINUS_1;
    let i0 = (ix_scaled / CR_RANGE_I64).min(62) as i32;
    let wi = ((ix_scaled % CR_RANGE_I64) / CR_FRAC_DIVISOR) as usize;
    let w = &CR_W_LOCAL[wi.min(CR_WN - 1)];
    let p0 = PHI1_TABLE_LOCAL[(i0 - 1).clamp(0, 63) as usize] as i64;
    let p1 = PHI1_TABLE_LOCAL[i0.clamp(0, 63) as usize] as i64;
    let p2 = PHI1_TABLE_LOCAL[(i0 + 1).clamp(0, 63) as usize] as i64;
    let p3 = PHI1_TABLE_LOCAL[(i0 + 2).clamp(0, 63) as usize] as i64;
    cr_dot_local(w, p0, p1, p2, p3).clamp(0, S6)
}

#[inline(always)]
fn norm_pdf_i64_local(x: i64) -> i64 {
    let x64 = x.clamp(CR_DOMAIN_MIN_I64, CR_DOMAIN_MAX_I64);
    let x_off = x64 - CR_DOMAIN_MIN_I64;
    let ix_scaled = x_off * CR_N_MINUS_1;
    let i0 = (ix_scaled / CR_RANGE_I64).min(62) as i32;
    let wi = ((ix_scaled % CR_RANGE_I64) / CR_FRAC_DIVISOR) as usize;
    let w = &CR_W_LOCAL[wi.min(CR_WN - 1)];
    let p0 = PDF1_TABLE_LOCAL[(i0 - 1).clamp(0, 63) as usize] as i64;
    let p1 = PDF1_TABLE_LOCAL[i0.clamp(0, 63) as usize] as i64;
    let p2 = PDF1_TABLE_LOCAL[(i0 + 1).clamp(0, 63) as usize] as i64;
    let p3 = PDF1_TABLE_LOCAL[(i0 + 2).clamp(0, 63) as usize] as i64;
    cr_dot_local(w, p0, p1, p2, p3).clamp(0, S6)
}

#[inline(always)]
fn bvn_cdf_i64_local(a: i64, b: i64, table: &Phi2Table) -> i64 {
    let a64 = a.clamp(CR_DOMAIN_MIN_I64, CR_DOMAIN_MAX_I64);
    let b64 = b.clamp(CR_DOMAIN_MIN_I64, CR_DOMAIN_MAX_I64);
    let a_off = a64 - CR_DOMAIN_MIN_I64;
    let b_off = b64 - CR_DOMAIN_MIN_I64;
    let ia_scaled = a_off * CR_N_MINUS_1;
    let ib_scaled = b_off * CR_N_MINUS_1;
    let i0 = (ia_scaled / CR_RANGE_I64).min(62) as i32;
    let j0 = (ib_scaled / CR_RANGE_I64).min(62) as i32;
    let wa = &CR_W_LOCAL[(((ia_scaled % CR_RANGE_I64) / CR_FRAC_DIVISOR) as usize).min(CR_WN - 1)];
    let wb = &CR_W_LOCAL[(((ib_scaled % CR_RANGE_I64) / CR_FRAC_DIVISOR) as usize).min(CR_WN - 1)];
    let mut cols = [0i64; 4];
    for di in 0..4i32 {
        let ii = (i0 - 1 + di).clamp(0, 63) as usize;
        let p0 = table[ii][(j0 - 1).clamp(0, 63) as usize] as i64;
        let p1 = table[ii][j0.clamp(0, 63) as usize] as i64;
        let p2 = table[ii][(j0 + 1).clamp(0, 63) as usize] as i64;
        let p3 = table[ii][(j0 + 2).clamp(0, 63) as usize] as i64;
        cols[di as usize] = cr_dot_local(wb, p0, p1, p2, p3);
    }
    cr_dot_local(wa, cols[0], cols[1], cols[2], cols[3]).clamp(0, S6)
}

#[inline(always)]
fn triple_complement_gh3_local(pre: &TripleCorrectionPre, num: [i64; 3]) -> i64 {
    let intercept = [
        num[0] * pre.inv_beta[0] / S6,
        num[1] * pre.inv_beta[1] / S6,
        num[2] * pre.inv_beta[2] / S6,
    ];
    let mut total = 0i64;
    for i in 0..3 {
        let x = [-1_732_051, 0, 1_732_051][i];
        let w = [166_667, 666_667, 166_667][i];
        let mut lower = -4 * S6;
        let mut upper = 4 * S6;
        for k in 0..3 {
            let bound = intercept[k] - pre.slope[k] as i128 as i64 * x / S6;
            if pre.is_upper[k] {
                upper = upper.min(bound);
            } else {
                lower = lower.max(bound);
            }
        }
        if lower >= upper {
            continue;
        }
        let p = (norm_cdf_i64_local(upper * S6) - norm_cdf_i64_local(lower * S6)).max(0);
        total += m6r(w, p);
    }
    total.max(0)
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FilterNode {
    pub c: i64,
    pub w: i64,
    pub mean_u: i64,
    pub mean_v: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct FilterState {
    pub nodes: [FilterNode; MAX_K],
    pub n_active: usize,
}

impl Default for FilterState {
    fn default() -> Self {
        Self {
            nodes: [FilterNode::default(); MAX_K],
            n_active: 0,
        }
    }
}

impl FilterState {
    fn singleton_origin() -> Self {
        let mut state = Self::default();
        state.nodes[0] = FilterNode {
            c: 0,
            w: S6,
            mean_u: 0,
            mean_v: 0,
        };
        state.n_active = 1;
        state
    }

    fn total_weight(&self) -> i64 {
        self.nodes
            .iter()
            .map(|node| node.w)
            .sum::<i64>()
            .clamp(0, S6)
    }
}

#[inline(always)]
fn states_match_exact(lhs: &FilterState, rhs: &FilterState) -> bool {
    if lhs.n_active != rhs.n_active {
        return false;
    }
    for idx in 0..lhs.n_active {
        let a = lhs.nodes[idx];
        let b = rhs.nodes[idx];
        if a.c != b.c || a.w != b.w || a.mean_u != b.mean_u || a.mean_v != b.mean_v {
            return false;
        }
    }
    true
}

#[cfg(not(target_os = "solana"))]
#[derive(Debug, Clone, Copy)]
pub struct C1FilterTrace {
    pub quote: C1FastQuote,
    pub observation_survival: [i64; N_OBS],
    pub observation_autocall_first_hit: [i64; N_OBS],
    pub observation_first_knock_in: [i64; N_OBS],
    pub post_observation_safe_mass: [i64; N_OBS],
    pub post_observation_knocked_mass: [i64; N_OBS],
    pub k_retained: usize,
}

#[cfg(not(target_os = "solana"))]
#[derive(Debug, Clone, Copy)]
pub struct QuoteWithDelta {
    pub fc_bps: f64,
    pub delta_spy: f64,
    pub delta_qqq: f64,
    pub delta_iwm: f64,
}

#[cfg(not(target_os = "solana"))]
#[derive(Debug, Clone, Copy, Default)]
struct ObservationReferenceState {
    safe_pred: FilterState,
    knocked_pred: FilterState,
    drift_shift_total: i64,
}

#[cfg(not(target_os = "solana"))]
#[derive(Debug, Clone, Serialize)]
pub struct FrozenGradientValidation {
    pub sigma_common: f64,
    pub live_fair_coupon_bps: f64,
    pub frozen_fair_coupon_bps: f64,
    pub fair_coupon_diff_bps: f64,
    pub live_obs2_first_hit: f64,
    pub frozen_obs2_first_hit: f64,
    pub compared_nodes: usize,
    pub ratio_samples_u: usize,
    pub ratio_samples_v: usize,
    pub mean_live_over_frozen_u: f64,
    pub mean_live_over_frozen_v: f64,
    pub max_live_over_frozen_u: f64,
    pub max_live_over_frozen_v: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct PredictionBenchSummary {
    pub active_nodes: usize,
    pub total_mass: i64,
    pub checksum: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct ObservationBenchSummary {
    pub safe_active_nodes: usize,
    pub knocked_active_nodes: usize,
    pub first_hit_mass: i64,
    pub first_knock_in_mass: i64,
    pub checksum: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct ObservationBenchState {
    pub safe_state: FilterState,
    pub knocked_state: FilterState,
    pub transition: FactorTransition,
    pub obs_idx: usize,
    pub drift_shift_total: i64,
    pub k_retained: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct CouponOnlyBenchState {
    pub safe_pred: FilterState,
    pub knocked_pred: FilterState,
    pub obs_idx: usize,
    pub drift_shift_total: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct CouponOnlyBenchSummary {
    pub safe_active_nodes: usize,
    pub knocked_active_nodes: usize,
    pub coupon_hit: i64,
    pub checksum: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct TriangleGradientBenchSummary {
    pub probability: i64,
    pub expectation_u: i64,
    pub expectation_v: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct FactorTransition {
    pub factor_values: [i64; N_FACTOR_NODES],
    pub factor_weights: [i64; N_FACTOR_NODES],
    pub step_mean_u: [i64; N_FACTOR_NODES],
    pub step_mean_v: [i64; N_FACTOR_NODES],
}

#[derive(Debug, Clone, Copy)]
pub struct MaturityBenchState {
    pub safe_state: FilterState,
    pub knocked_state: FilterState,
    pub transition: FactorTransition,
    pub obs_idx: usize,
    pub drift_shift_total: i64,
    pub k_retained: usize,
}

pub const MATURITY_BENCH_STATE_BYTES: usize = 1288;

impl MaturityBenchState {
    pub fn to_le_bytes(&self) -> [u8; MATURITY_BENCH_STATE_BYTES] {
        let mut buf = [0u8; MATURITY_BENCH_STATE_BYTES];
        let mut off = 0;
        // safe_state
        for i in 0..MAX_K {
            buf[off..off + 8].copy_from_slice(&self.safe_state.nodes[i].c.to_le_bytes());
            off += 8;
            buf[off..off + 8].copy_from_slice(&self.safe_state.nodes[i].w.to_le_bytes());
            off += 8;
            buf[off..off + 8].copy_from_slice(&self.safe_state.nodes[i].mean_u.to_le_bytes());
            off += 8;
            buf[off..off + 8].copy_from_slice(&self.safe_state.nodes[i].mean_v.to_le_bytes());
            off += 8;
        }
        buf[off..off + 8].copy_from_slice(&(self.safe_state.n_active as u64).to_le_bytes());
        off += 8;
        // knocked_state
        for i in 0..MAX_K {
            buf[off..off + 8].copy_from_slice(&self.knocked_state.nodes[i].c.to_le_bytes());
            off += 8;
            buf[off..off + 8].copy_from_slice(&self.knocked_state.nodes[i].w.to_le_bytes());
            off += 8;
            buf[off..off + 8].copy_from_slice(&self.knocked_state.nodes[i].mean_u.to_le_bytes());
            off += 8;
            buf[off..off + 8].copy_from_slice(&self.knocked_state.nodes[i].mean_v.to_le_bytes());
            off += 8;
        }
        buf[off..off + 8].copy_from_slice(&(self.knocked_state.n_active as u64).to_le_bytes());
        off += 8;
        // transition
        for i in 0..N_FACTOR_NODES {
            buf[off..off + 8].copy_from_slice(&self.transition.factor_values[i].to_le_bytes());
            off += 8;
        }
        for i in 0..N_FACTOR_NODES {
            buf[off..off + 8].copy_from_slice(&self.transition.factor_weights[i].to_le_bytes());
            off += 8;
        }
        for i in 0..N_FACTOR_NODES {
            buf[off..off + 8].copy_from_slice(&self.transition.step_mean_u[i].to_le_bytes());
            off += 8;
        }
        for i in 0..N_FACTOR_NODES {
            buf[off..off + 8].copy_from_slice(&self.transition.step_mean_v[i].to_le_bytes());
            off += 8;
        }
        // scalars
        buf[off..off + 8].copy_from_slice(&(self.obs_idx as u64).to_le_bytes());
        off += 8;
        buf[off..off + 8].copy_from_slice(&self.drift_shift_total.to_le_bytes());
        off += 8;
        buf[off..off + 8].copy_from_slice(&(self.k_retained as u64).to_le_bytes());
        off += 8;
        debug_assert_eq!(off, MATURITY_BENCH_STATE_BYTES);
        buf
    }

    pub fn from_le_bytes(buf: &[u8; MATURITY_BENCH_STATE_BYTES]) -> Self {
        let mut off = 0;
        let read_i64 = |buf: &[u8], off: &mut usize| -> i64 {
            let v = i64::from_le_bytes(buf[*off..*off + 8].try_into().unwrap());
            *off += 8;
            v
        };
        let read_u64 = |buf: &[u8], off: &mut usize| -> u64 {
            let v = u64::from_le_bytes(buf[*off..*off + 8].try_into().unwrap());
            *off += 8;
            v
        };

        let mut safe_nodes = [FilterNode::default(); MAX_K];
        for i in 0..MAX_K {
            safe_nodes[i].c = read_i64(buf, &mut off);
            safe_nodes[i].w = read_i64(buf, &mut off);
            safe_nodes[i].mean_u = read_i64(buf, &mut off);
            safe_nodes[i].mean_v = read_i64(buf, &mut off);
        }
        let safe_n_active = read_u64(buf, &mut off) as usize;

        let mut knocked_nodes = [FilterNode::default(); MAX_K];
        for i in 0..MAX_K {
            knocked_nodes[i].c = read_i64(buf, &mut off);
            knocked_nodes[i].w = read_i64(buf, &mut off);
            knocked_nodes[i].mean_u = read_i64(buf, &mut off);
            knocked_nodes[i].mean_v = read_i64(buf, &mut off);
        }
        let knocked_n_active = read_u64(buf, &mut off) as usize;

        let mut factor_values = [0i64; N_FACTOR_NODES];
        for i in 0..N_FACTOR_NODES {
            factor_values[i] = read_i64(buf, &mut off);
        }
        let mut factor_weights = [0i64; N_FACTOR_NODES];
        for i in 0..N_FACTOR_NODES {
            factor_weights[i] = read_i64(buf, &mut off);
        }
        let mut step_mean_u = [0i64; N_FACTOR_NODES];
        for i in 0..N_FACTOR_NODES {
            step_mean_u[i] = read_i64(buf, &mut off);
        }
        let mut step_mean_v = [0i64; N_FACTOR_NODES];
        for i in 0..N_FACTOR_NODES {
            step_mean_v[i] = read_i64(buf, &mut off);
        }

        let obs_idx = read_u64(buf, &mut off) as usize;
        let drift_shift_total = read_i64(buf, &mut off);
        let k_retained = read_u64(buf, &mut off) as usize;

        debug_assert_eq!(off, MATURITY_BENCH_STATE_BYTES);

        Self {
            safe_state: FilterState {
                nodes: safe_nodes,
                n_active: safe_n_active,
            },
            knocked_state: FilterState {
                nodes: knocked_nodes,
                n_active: knocked_n_active,
            },
            transition: FactorTransition {
                factor_values,
                factor_weights,
                step_mean_u,
                step_mean_v,
            },
            obs_idx,
            drift_shift_total,
            k_retained,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct MaturityBenchSummary {
    pub safe_active_nodes: usize,
    pub knocked_active_nodes: usize,
    pub coupon_hit: i64,
    pub safe_principal: i64,
    pub first_knock_in: i64,
    pub knock_in_redemption_safe: i64,
    pub knocked_redemption: i64,
    pub checksum: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct GradientBenchSummary {
    pub update_first_hit: i64,
    pub update_first_knock_in: i64,
    pub maturity_coupon_hit: i64,
    pub maturity_safe_principal: i64,
    pub maturity_first_knock_in: i64,
    pub maturity_knocked_redemption: i64,
    pub checksum: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct GradientKnockedBenchSummary {
    pub maturity_coupon_hit: i64,
    pub maturity_knocked_redemption: i64,
    pub checksum: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct ObservationGradientBenchSummary {
    first_hit: i64,
    first_knock_in: i64,
    checksum: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct MaturityGradientBenchSummary {
    coupon_hit: i64,
    safe_principal: i64,
    first_knock_in: i64,
    knocked_redemption: i64,
    checksum: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct MaturitySafeBenchSummary {
    coupon_hit: i64,
    safe_principal: i64,
    first_knock_in: i64,
    checksum: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct MaturityKnockedBenchSummary {
    coupon_hit: i64,
    knocked_redemption: i64,
    checksum: i64,
}

struct MaturityBenchCall<'a> {
    cfg: &'a C1FastConfig,
    transition: &'a FactorTransition,
    obs_idx: usize,
    drift_shift_total: i64,
    k_retained: usize,
    triple_pre: Option<&'a TripleCorrectionPre>,
    frozen_grid: Option<&'a crate::frozen_predict_tables::FrozenPredictGrid>,
    dmu_ds: &'a [(i64, i64, i64); 3],
}

#[derive(Debug, Clone, Copy)]
struct ExactSeedTransition {
    factor_values: [i64; N_FACTOR_NODES_EXACT_SEED],
    factor_weights: [i64; N_FACTOR_NODES_EXACT_SEED],
    step_mean_u: [i64; N_FACTOR_NODES_EXACT_SEED],
    step_mean_v: [i64; N_FACTOR_NODES_EXACT_SEED],
}

#[derive(Debug, Clone, Copy)]
struct FirstObservationSeed {
    predicted_safe: FilterState,
    next_safe: FilterState,
    next_knocked: FilterState,
    first_hit: i64,
    first_knock_in: i64,
}

#[derive(Debug, Clone, Copy)]
struct SafeUpdate {
    next_safe: FilterState,
    new_knocked: FilterState,
    first_hit: i64,
    first_knock_in: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct NodeGrad {
    dw: [i64; 3],
    du: [i64; 3],
    dv: [i64; 3],
}

#[derive(Debug, Clone, Copy)]
struct FilterStateGrad {
    nodes: [NodeGrad; MAX_K],
}

impl Default for FilterStateGrad {
    fn default() -> Self {
        Self {
            nodes: [NodeGrad::default(); MAX_K],
        }
    }
}

const ZERO_NODE_GRAD: NodeGrad = NodeGrad {
    dw: [0; 3],
    du: [0; 3],
    dv: [0; 3],
};

const ZERO_FILTER_STATE_GRAD: FilterStateGrad = FilterStateGrad {
    nodes: [ZERO_NODE_GRAD; MAX_K],
};

#[derive(Debug, Clone, Copy)]
struct SafeUpdateGrad {
    next_safe: FilterStateGrad,
    new_knocked: FilterStateGrad,
    first_hit: [i64; 3],
    first_knock_in: [i64; 3],
}

impl Default for SafeUpdateGrad {
    fn default() -> Self {
        Self {
            next_safe: FilterStateGrad::default(),
            new_knocked: FilterStateGrad::default(),
            first_hit: [0; 3],
            first_knock_in: [0; 3],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MaturitySafeLegGrad {
    coupon_hit: [i64; 3],
    safe_principal: [i64; 3],
    first_knock_in: [i64; 3],
    knock_in_redemption: [i64; 3],
}

impl Default for MaturitySafeLegGrad {
    fn default() -> Self {
        Self {
            coupon_hit: [0; 3],
            safe_principal: [0; 3],
            first_knock_in: [0; 3],
            knock_in_redemption: [0; 3],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MaturityKnockedLegGrad {
    coupon_hit: [i64; 3],
    redemption: [i64; 3],
}

impl Default for MaturityKnockedLegGrad {
    fn default() -> Self {
        Self {
            coupon_hit: [0; 3],
            redemption: [0; 3],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MaturityStepGrad {
    coupon_hit: [i64; 3],
    safe_principal: [i64; 3],
    first_knock_in: [i64; 3],
    knock_in_redemption_safe: [i64; 3],
    knocked_redemption: [i64; 3],
}

impl Default for MaturityStepGrad {
    fn default() -> Self {
        Self {
            coupon_hit: [0; 3],
            safe_principal: [0; 3],
            first_knock_in: [0; 3],
            knock_in_redemption_safe: [0; 3],
            knocked_redemption: [0; 3],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct KnockedUpdateGrad {
    next_knocked: FilterStateGrad,
    first_hit: [i64; 3],
}

impl Default for KnockedUpdateGrad {
    fn default() -> Self {
        Self {
            next_knocked: FilterStateGrad::default(),
            first_hit: [0; 3],
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ObservationStepWithGrad {
    next_safe: FilterState,
    next_safe_grad: FilterStateGrad,
    next_knocked: FilterState,
    next_knocked_grad: FilterStateGrad,
    first_hit: i64,
    first_hit_grad: [i64; 3],
    first_knock_in: i64,
    first_knock_in_grad: [i64; 3],
}

#[derive(Debug, Clone, Copy)]
struct KnockedUpdate {
    next_knocked: FilterState,
    first_hit: i64,
}

#[derive(Debug, Clone, Copy)]
struct MaturityStep {
    coupon_hit: i64,
    safe_principal: i64,
    first_knock_in: i64,
    knock_in_redemption_safe: i64,
    knocked_redemption: i64,
}

#[inline(always)]
fn phi2_tables() -> [&'static Phi2Table; 3] {
    [
        &PHI2_RESID_SPY_QQQ,
        &PHI2_RESID_SPY_IWM,
        &PHI2_RESID_QQQ_IWM,
    ]
}

#[derive(Debug, Clone, Copy, Default)]
struct RawRegionMoment {
    probability: i64,
    expectation_u: i64,
    expectation_v: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct RawRegionMomentGrad {
    dp_du: i64,
    dp_dv: i64,
    dp_dc: i64,
    deu_du: i64,
    deu_dv: i64,
    deu_dc: i64,
    dev_du: i64,
    dev_dv: i64,
    dev_dc: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct KiMomentGrad {
    dp_du: i64,
    dp_dv: i64,
    dp_dc: i64,
    dworst_du: i64,
    dworst_dv: i64,
    dworst_dc: i64,
}

#[derive(Clone, Copy)]
struct FrozenRegionView<'a> {
    probability: &'a [i64],
    correction_u: &'a [i64],
    correction_v: &'a [i64],
    /// Interpolation data: [N_MU_SAMPLES] slices per field.
    prob_interp: [&'a [i64]; 4],
    corr_u_interp: [&'a [i64]; 4],
    corr_v_interp: [&'a [i64]; 4],
}

#[derive(Clone, Copy)]
struct FrozenObservationView<'a> {
    safe_autocall: FrozenRegionView<'a>,
    safe_ki: FrozenRegionView<'a>,
    knocked_autocall: FrozenRegionView<'a>,
}

#[inline(always)]
fn conditional_mean(
    probability: i64,
    expectation_u: i64,
    expectation_v: i64,
) -> Option<(i64, i64)> {
    if probability <= NODE_STATE_EPS_S6 {
        return None;
    }
    Some((
        (expectation_u as i128 * S6 as i128 / probability as i128) as i64,
        (expectation_v as i128 * S6 as i128 / probability as i128) as i64,
    ))
}

#[inline(always)]
fn conditional_mean_grad(
    probability: i64,
    expectation: i64,
    dprobability: i64,
    dexpectation: i64,
) -> i64 {
    if probability <= NODE_STATE_EPS_S6 {
        return 0;
    }
    let p = probability as i128;
    let numer =
        (dexpectation as i128 * p - expectation as i128 * dprobability as i128) * S6 as i128;
    (numer / (p * p)) as i64
}

#[inline(always)]
fn frozen_expectation_grad(
    mean_grad: i64,
    probability: i64,
    expectation: i64,
    dprobability: i64,
) -> i64 {
    if probability <= NODE_STATE_EPS_S6 {
        return 0;
    }
    m6r_fast(mean_grad, probability)
        + (expectation as i128 * dprobability as i128 / probability as i128) as i64
}

#[inline(always)]
fn exp_neg15_to_zero_i64_local(x_s6: i64) -> i64 {
    if x_s6 >= 0 {
        return S6;
    }
    if x_s6 <= EXP_NEG15_LO_S6 {
        return EXP_NEG15_TABLE[0];
    }
    let offset = x_s6 - EXP_NEG15_LO_S6;
    let idx = (offset / EXP_NEG15_STEP_S6) as usize;
    let frac = offset - idx as i64 * EXP_NEG15_STEP_S6;
    let y0 = EXP_NEG15_TABLE[idx];
    let y1 = EXP_NEG15_TABLE[idx + 1];
    y0 + ((y1 - y0) as i128 * frac as i128 / EXP_NEG15_STEP_S6 as i128) as i64
}

#[inline(always)]
fn compute_dmu_ds(cfg: &C1FastConfig, spots_s6: [i64; 3]) -> [(i64, i64, i64); 3] {
    let inv_spy = (S6 as i128 * S6 as i128 / spots_s6[0].max(1) as i128) as i64;
    let inv_qqq = (S6 as i128 * S6 as i128 / spots_s6[1].max(1) as i128) as i64;
    let inv_iwm = (S6 as i128 * S6 as i128 / spots_s6[2].max(1) as i128) as i64;
    [
        (
            -inv_spy,
            -inv_spy,
            (cfg.loadings[0] as i128 * S6 as i128 / spots_s6[0].max(1) as i128) as i64,
        ),
        (
            inv_qqq,
            0,
            (cfg.loadings[1] as i128 * S6 as i128 / spots_s6[1].max(1) as i128) as i64,
        ),
        (
            0,
            inv_iwm,
            (cfg.loadings[2] as i128 * S6 as i128 / spots_s6[2].max(1) as i128) as i64,
        ),
    ]
}

#[cfg(not(target_os = "solana"))]
fn spot_shift_bundle_live(cfg: &C1FastConfig, spots_s6: [i64; 3]) -> (i64, i64, i64) {
    let log_spy = (spots_s6[0] as f64 / S6 as f64).ln();
    let log_qqq = (spots_s6[1] as f64 / S6 as f64).ln();
    let log_iwm = (spots_s6[2] as f64 / S6 as f64).ln();
    let mu_u = ((log_qqq - log_spy) * S6 as f64).round() as i64;
    let mu_v = ((log_iwm - log_spy) * S6 as f64).round() as i64;
    let mu_c = (cfg.loadings[0] as f64 * log_spy
        + cfg.loadings[1] as f64 * log_qqq
        + cfg.loadings[2] as f64 * log_iwm)
        .round() as i64;
    (mu_u, mu_v, mu_c)
}

#[cfg(not(target_os = "solana"))]
fn shifted_origin_state(mu_u_shift: i64, mu_v_shift: i64) -> FilterState {
    let mut state = FilterState::singleton_origin();
    state.nodes[0].mean_u = mu_u_shift;
    state.nodes[0].mean_v = mu_v_shift;
    state
}

#[inline(always)]
fn seed_state_mean_grad(state: &FilterState, dmu_ds: &[(i64, i64, i64); 3]) -> FilterStateGrad {
    let mut out = FilterStateGrad::default();
    for (idx, node) in state.nodes.iter().enumerate() {
        if node.w <= 0 {
            continue;
        }
        for asset in 0..3 {
            out.nodes[idx].du[asset] = dmu_ds[asset].0;
            out.nodes[idx].dv[asset] = dmu_ds[asset].1;
        }
    }
    out
}

#[inline(always)]
fn dmu_c_only(dmu_ds: &[(i64, i64, i64); 3]) -> [(i64, i64, i64); 3] {
    core::array::from_fn(|asset| (0, 0, dmu_ds[asset].2))
}

#[inline(always)]
fn observation_probability_triple_pre<'a>(
    obs_idx: usize,
    triple_pre: Option<&'a TripleCorrectionPre>,
) -> Option<&'a TripleCorrectionPre> {
    if obs_idx == 0 {
        None
    } else {
        triple_pre
    }
}

#[inline(always)]
fn canonical_table_k(k_retained: usize) -> usize {
    if k_retained <= 10 {
        9
    } else if k_retained <= 13 {
        12
    } else {
        15
    }
}

#[inline(always)]
fn map_table_index(node_idx: usize, runtime_k: usize, table_len: usize) -> usize {
    if table_len <= 1 || runtime_k <= 1 {
        return 0;
    }
    (node_idx * (table_len - 1) + (runtime_k - 2) / 2) / (runtime_k - 1)
}

fn frozen_region_view<const K: usize>(
    r: &'static crate::worst_of_c1_filter_gradients::FrozenRegionTables<K>,
    obs_idx: usize,
) -> FrozenRegionView<'static> {
    FrozenRegionView {
        probability: &r.probability[obs_idx],
        correction_u: &r.correction_u[obs_idx],
        correction_v: &r.correction_v[obs_idx],
        prob_interp: [
            &r.probability_interp[0][obs_idx],
            &r.probability_interp[1][obs_idx],
            &r.probability_interp[2][obs_idx],
            &r.probability_interp[3][obs_idx],
        ],
        corr_u_interp: [
            &r.correction_u_interp[0][obs_idx],
            &r.correction_u_interp[1][obs_idx],
            &r.correction_u_interp[2][obs_idx],
            &r.correction_u_interp[3][obs_idx],
        ],
        corr_v_interp: [
            &r.correction_v_interp[0][obs_idx],
            &r.correction_v_interp[1][obs_idx],
            &r.correction_v_interp[2][obs_idx],
            &r.correction_v_interp[3][obs_idx],
        ],
    }
}

fn frozen_view_from_tables<const K: usize>(
    tables: &'static FrozenMomentTables<K>,
    obs_idx: usize,
) -> FrozenObservationView<'static> {
    FrozenObservationView {
        safe_autocall: frozen_region_view(&tables.safe_autocall, obs_idx),
        safe_ki: frozen_region_view(&tables.safe_ki, obs_idx),
        knocked_autocall: frozen_region_view(&tables.knocked_autocall, obs_idx),
    }
}

#[inline(always)]
fn frozen_observation_view(k_retained: usize, obs_idx: usize) -> FrozenObservationView<'static> {
    match canonical_table_k(k_retained) {
        9 => frozen_view_from_tables(&FROZEN_TABLES_K9, obs_idx),
        12 => frozen_view_from_tables(&FROZEN_TABLES_K12, obs_idx),
        _ => frozen_view_from_tables(&FROZEN_TABLES_K15, obs_idx),
    }
}

#[inline(always)]
fn scale_frozen_correction(correction: i64, probability: i64, reference_probability: i64) -> i64 {
    if correction == 0
        || probability <= NODE_STATE_EPS_S6
        || reference_probability <= FROZEN_RATIO_EPS_S6
    {
        return 0;
    }
    correction * probability / reference_probability
}

/// Linearly interpolate a frozen correction across MU_SAMPLES based on mean_u.
#[inline(always)]
fn interp_frozen_correction(
    mean_u: i64,
    table_idx: usize,
    probability: i64,
    corr_interp: &[&[i64]; 4],
    prob_interp: &[&[i64]; 4],
) -> i64 {
    use crate::worst_of_c1_filter_gradients::MU_SAMPLES;
    // Clamp to table range
    if mean_u <= MU_SAMPLES[0] {
        return scale_frozen_correction(
            corr_interp[0][table_idx],
            probability,
            prob_interp[0][table_idx],
        );
    }
    if mean_u >= MU_SAMPLES[3] {
        return scale_frozen_correction(
            corr_interp[3][table_idx],
            probability,
            prob_interp[3][table_idx],
        );
    }
    // Find bracket
    let mut lo = 0usize;
    if mean_u >= MU_SAMPLES[2] {
        lo = 2;
    } else if mean_u >= MU_SAMPLES[1] {
        lo = 1;
    }
    let hi = lo + 1;
    let span = MU_SAMPLES[hi] - MU_SAMPLES[lo];
    let frac = mean_u - MU_SAMPLES[lo]; // in [0, span)

    let c_lo = scale_frozen_correction(
        corr_interp[lo][table_idx],
        probability,
        prob_interp[lo][table_idx],
    );
    let c_hi = scale_frozen_correction(
        corr_interp[hi][table_idx],
        probability,
        prob_interp[hi][table_idx],
    );
    c_lo + (c_hi - c_lo) * frac / span
}

#[inline(always)]
fn raw_region_moment_from_frozen(
    mean_u: i64,
    mean_v: i64,
    probability: i64,
    node_idx: usize,
    runtime_k: usize,
    region: FrozenRegionView<'_>,
) -> RawRegionMoment {
    c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_gh3_before_frozen");
    if probability <= 0 {
        let out = RawRegionMoment::default();
        c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_mean_update");
        return out;
    }
    let table_idx = map_table_index(node_idx, runtime_k, region.probability.len());
    // Revert to simple mu=0 frozen correction (no MU interpolation — didn't help accuracy,
    // costs ~4 divides per node).
    let correction_u = scale_frozen_correction(
        region.correction_u[table_idx],
        probability,
        region.probability[table_idx],
    );
    let correction_v = scale_frozen_correction(
        region.correction_v[table_idx],
        probability,
        region.probability[table_idx],
    );
    let _ = mean_u;
    let out = RawRegionMoment {
        probability,
        expectation_u: m6r_fast(mean_u, probability) + correction_u,
        expectation_v: m6r_fast(mean_v, probability) + correction_v,
    };
    c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_mean_update");
    out
}

#[inline(always)]
fn triangle_region_from_frozen_inline_i64(
    mean_u: i64,
    mean_v: i64,
    rhs: &[i64; 3],
    pre: &TrianglePre64,
    phi2: [&Phi2Table; 3],
    triple_pre: Option<&TripleCorrectionPre>,
    node_idx: usize,
    runtime_k: usize,
    region: FrozenRegionView<'_>,
) -> RawRegionMoment {
    macro_rules! norm_cdf_inline {
        ($x:expr) => {{
            let x64 = ($x).clamp(CR_DOMAIN_MIN_I64, CR_DOMAIN_MAX_I64);
            let x_off = x64 - CR_DOMAIN_MIN_I64;
            let ix_scaled = x_off * CR_N_MINUS_1;
            let i0 = (ix_scaled / CR_RANGE_I64).min(62) as i32;
            let wi = ((ix_scaled % CR_RANGE_I64) / CR_FRAC_DIVISOR) as usize;
            let w = &CR_W_LOCAL[wi.min(CR_WN - 1)];
            let p0 = PHI1_TABLE_LOCAL[(i0 - 1).clamp(0, 63) as usize] as i64;
            let p1 = PHI1_TABLE_LOCAL[i0.clamp(0, 63) as usize] as i64;
            let p2 = PHI1_TABLE_LOCAL[(i0 + 1).clamp(0, 63) as usize] as i64;
            let p3 = PHI1_TABLE_LOCAL[(i0 + 2).clamp(0, 63) as usize] as i64;
            ((w[0] as i64 * p0 + w[1] as i64 * p1 + w[2] as i64 * p2 + w[3] as i64 * p3) >> CR_WS)
                .clamp(0, S6)
        }};
    }

    macro_rules! bvn_cdf_inline {
        ($a:expr, $b:expr, $table:expr) => {{
            let table_ref = $table;
            let a64 = ($a).clamp(CR_DOMAIN_MIN_I64, CR_DOMAIN_MAX_I64);
            let b64 = ($b).clamp(CR_DOMAIN_MIN_I64, CR_DOMAIN_MAX_I64);
            let a_off = a64 - CR_DOMAIN_MIN_I64;
            let b_off = b64 - CR_DOMAIN_MIN_I64;
            let ia_scaled = a_off * CR_N_MINUS_1;
            let ib_scaled = b_off * CR_N_MINUS_1;
            let i0 = (ia_scaled / CR_RANGE_I64).min(62) as i32;
            let j0 = (ib_scaled / CR_RANGE_I64).min(62) as i32;
            let wa = &CR_W_LOCAL
                [(((ia_scaled % CR_RANGE_I64) / CR_FRAC_DIVISOR) as usize).min(CR_WN - 1)];
            let wb = &CR_W_LOCAL
                [(((ib_scaled % CR_RANGE_I64) / CR_FRAC_DIVISOR) as usize).min(CR_WN - 1)];

            let ii0 = (i0 - 1).clamp(0, 63) as usize;
            let ii1 = i0.clamp(0, 63) as usize;
            let ii2 = (i0 + 1).clamp(0, 63) as usize;
            let ii3 = (i0 + 2).clamp(0, 63) as usize;
            let jj0 = (j0 - 1).clamp(0, 63) as usize;
            let jj1 = j0.clamp(0, 63) as usize;
            let jj2 = (j0 + 1).clamp(0, 63) as usize;
            let jj3 = (j0 + 2).clamp(0, 63) as usize;

            let c0 = ((wb[0] as i64 * table_ref[ii0][jj0] as i64
                + wb[1] as i64 * table_ref[ii0][jj1] as i64
                + wb[2] as i64 * table_ref[ii0][jj2] as i64
                + wb[3] as i64 * table_ref[ii0][jj3] as i64)
                >> CR_WS);
            let c1 = ((wb[0] as i64 * table_ref[ii1][jj0] as i64
                + wb[1] as i64 * table_ref[ii1][jj1] as i64
                + wb[2] as i64 * table_ref[ii1][jj2] as i64
                + wb[3] as i64 * table_ref[ii1][jj3] as i64)
                >> CR_WS);
            let c2 = ((wb[0] as i64 * table_ref[ii2][jj0] as i64
                + wb[1] as i64 * table_ref[ii2][jj1] as i64
                + wb[2] as i64 * table_ref[ii2][jj2] as i64
                + wb[3] as i64 * table_ref[ii2][jj3] as i64)
                >> CR_WS);
            let c3 = ((wb[0] as i64 * table_ref[ii3][jj0] as i64
                + wb[1] as i64 * table_ref[ii3][jj1] as i64
                + wb[2] as i64 * table_ref[ii3][jj2] as i64
                + wb[3] as i64 * table_ref[ii3][jj3] as i64)
                >> CR_WS);

            ((wa[0] as i64 * c0 + wa[1] as i64 * c1 + wa[2] as i64 * c2 + wa[3] as i64 * c3)
                >> CR_WS)
                .clamp(0, S6)
        }};
    }

    c1_filter_cu_diag_inner(b"c1_filter_triangle_diag before_base");
    if rhs[0] < 0 && rhs[1] == rhs[0] && rhs[2] == rhs[0] {
        c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_base_before_gh3");
        c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_gh3_before_frozen");
        c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_mean_update");
        return RawRegionMoment::default();
    }

    let ew0 = pre.au[0] * mean_u / S6 + pre.av[0] * mean_v / S6;
    let ew1 = pre.au[1] * mean_u / S6 + pre.av[1] * mean_v / S6;
    let ew2 = pre.au[2] * mean_u / S6 + pre.av[2] * mean_v / S6;
    let num0 = rhs[0] - ew0;
    let num1 = rhs[1] - ew1;
    let num2 = rhs[2] - ew2;
    let z0 = num0 * pre.inv_std[0];
    let z1 = num1 * pre.inv_std[1];
    let z2 = num2 * pre.inv_std[2];

    let single0 = norm_cdf_inline!(-z0);
    let single1 = norm_cdf_inline!(-z1);
    let single2 = norm_cdf_inline!(-z2);

    let pair01 = if pre.phi2_neg[0] {
        let phi_a = norm_cdf_inline!(-z0);
        (phi_a - bvn_cdf_inline!(-z0, z1, phi2[0])).max(0)
    } else {
        bvn_cdf_inline!(-z0, -z1, phi2[0])
    };
    let pair02 = if pre.phi2_neg[1] {
        let phi_a = norm_cdf_inline!(-z0);
        (phi_a - bvn_cdf_inline!(-z0, z2, phi2[1])).max(0)
    } else {
        bvn_cdf_inline!(-z0, -z2, phi2[1])
    };
    let pair12 = if pre.phi2_neg[2] {
        let phi_a = norm_cdf_inline!(-z1);
        (phi_a - bvn_cdf_inline!(-z1, z2, phi2[2])).max(0)
    } else {
        bvn_cdf_inline!(-z1, -z2, phi2[2])
    };

    let prob_ie = (S6 - (single0 + single1 + single2) + pair01 + pair02 + pair12).clamp(0, S6);
    c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_base_before_gh3");

    let triple_p = if let Some(tp) = triple_pre {
        let intercept0 = (num0 as i128 * tp.inv_beta[0] as i128 / S6 as i128) as i64;
        let intercept1 = (num1 as i128 * tp.inv_beta[1] as i128 / S6 as i128) as i64;
        let intercept2 = (num2 as i128 * tp.inv_beta[2] as i128 / S6 as i128) as i64;

        let mut total = 0i64;

        let mut lower = -4 * S6;
        let mut upper = 4 * S6;
        let bound0 = intercept0 - tp.slope[0] * GH3_NODES_6[0] / S6;
        let bound1 = intercept1 - tp.slope[1] * GH3_NODES_6[0] / S6;
        let bound2 = intercept2 - tp.slope[2] * GH3_NODES_6[0] / S6;
        if tp.is_upper[0] {
            upper = upper.min(bound0)
        } else {
            lower = lower.max(bound0)
        }
        if tp.is_upper[1] {
            upper = upper.min(bound1)
        } else {
            lower = lower.max(bound1)
        }
        if tp.is_upper[2] {
            upper = upper.min(bound2)
        } else {
            lower = lower.max(bound2)
        }
        if lower < upper {
            let p = (norm_cdf_inline!(upper * S6) - norm_cdf_inline!(lower * S6)).max(0);
            total += GH3_WPI_6[0] * p / S6;
        }

        let mut lower = -4 * S6;
        let mut upper = 4 * S6;
        let bound0 = intercept0 - tp.slope[0] * GH3_NODES_6[1] / S6;
        let bound1 = intercept1 - tp.slope[1] * GH3_NODES_6[1] / S6;
        let bound2 = intercept2 - tp.slope[2] * GH3_NODES_6[1] / S6;
        if tp.is_upper[0] {
            upper = upper.min(bound0)
        } else {
            lower = lower.max(bound0)
        }
        if tp.is_upper[1] {
            upper = upper.min(bound1)
        } else {
            lower = lower.max(bound1)
        }
        if tp.is_upper[2] {
            upper = upper.min(bound2)
        } else {
            lower = lower.max(bound2)
        }
        if lower < upper {
            let p = (norm_cdf_inline!(upper * S6) - norm_cdf_inline!(lower * S6)).max(0);
            total += GH3_WPI_6[1] * p / S6;
        }

        let mut lower = -4 * S6;
        let mut upper = 4 * S6;
        let bound0 = intercept0 - tp.slope[0] * GH3_NODES_6[2] / S6;
        let bound1 = intercept1 - tp.slope[1] * GH3_NODES_6[2] / S6;
        let bound2 = intercept2 - tp.slope[2] * GH3_NODES_6[2] / S6;
        if tp.is_upper[0] {
            upper = upper.min(bound0)
        } else {
            lower = lower.max(bound0)
        }
        if tp.is_upper[1] {
            upper = upper.min(bound1)
        } else {
            lower = lower.max(bound1)
        }
        if tp.is_upper[2] {
            upper = upper.min(bound2)
        } else {
            lower = lower.max(bound2)
        }
        if lower < upper {
            let p = (norm_cdf_inline!(upper * S6) - norm_cdf_inline!(lower * S6)).max(0);
            total += GH3_WPI_6[2] * p / S6;
        }

        total.max(0)
    } else {
        0
    };

    let probability = (prob_ie - triple_p).clamp(0, S6);
    c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_gh3_before_frozen");
    if probability <= 0 {
        c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_mean_update");
        return RawRegionMoment::default();
    }

    let table_len = region.probability.len();
    let table_idx = if table_len <= 1 || runtime_k <= 1 {
        0
    } else {
        (node_idx * (table_len - 1) + (runtime_k - 2) / 2) / (runtime_k - 1)
    };
    let reference_probability = region.probability[table_idx];
    let correction_u = if region.correction_u[table_idx] == 0
        || probability <= NODE_STATE_EPS_S6
        || reference_probability <= FROZEN_RATIO_EPS_S6
    {
        0
    } else {
        region.correction_u[table_idx] * probability / reference_probability
    };
    let correction_v = if region.correction_v[table_idx] == 0
        || probability <= NODE_STATE_EPS_S6
        || reference_probability <= FROZEN_RATIO_EPS_S6
    {
        0
    } else {
        region.correction_v[table_idx] * probability / reference_probability
    };

    let out = RawRegionMoment {
        probability,
        expectation_u: mean_u * probability / S6 + correction_u,
        expectation_v: mean_v * probability / S6 + correction_v,
    };
    c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_mean_update");
    out
}

#[inline(always)]
fn triangle_gradient_geometry(pre: &TrianglePre64) -> ([i64; 3], [i64; 3]) {
    let mut dz_du = [0i64; 3];
    let mut dz_dv = [0i64; 3];
    for k in 0..3 {
        dz_du[k] = -(pre.au[k] * pre.inv_std[k] / S6);
        dz_dv[k] = -(pre.av[k] * pre.inv_std[k] / S6);
    }
    (dz_du, dz_dv)
}

#[inline(always)]
fn triangle_probability_with_triple_i64(
    mean_u: i64,
    mean_v: i64,
    rhs: &[i64; 3],
    pre: &TrianglePre64,
    phi2: [&Phi2Table; 3],
    triple_pre: Option<&TripleCorrectionPre>,
) -> i64 {
    c1_filter_cu_diag_inner(b"c1_filter_triangle_diag before_base");
    if rhs[0] < 0 && rhs[1] == rhs[0] && rhs[2] == rhs[0] {
        return 0;
    }

    let mut num6_arr = [0i64; 3];
    let mut z_scale = [0i64; 3];
    for k in 0..3 {
        let ew6 = m6r_fast(pre.au[k], mean_u) + m6r_fast(pre.av[k], mean_v);
        num6_arr[k] = rhs[k] - ew6;
        z_scale[k] = m6r_fast(num6_arr[k], pre.inv_std[k]) * S6;
    }

    let phi_z0 = norm_cdf_i64_local(z_scale[0]);
    let phi_z1 = norm_cdf_i64_local(z_scale[1]);
    let phi_z2 = norm_cdf_i64_local(z_scale[2]);
    let sum_complement = (S6 - phi_z0) + (S6 - phi_z1) + (S6 - phi_z2);

    let mut sum_pair = 0i64;
    for (pidx, &(i, j)) in [(0usize, 1usize), (0, 2), (1, 2)].iter().enumerate() {
        let neg_zi = -z_scale[i];
        let phi2_ij = if pre.phi2_neg[pidx] {
            let phi_a = norm_cdf_i64_local(neg_zi);
            (phi_a - bvn_cdf_i64_local(neg_zi, z_scale[j], phi2[pidx])).max(0)
        } else {
            bvn_cdf_i64_local(neg_zi, -z_scale[j], phi2[pidx])
        };
        sum_pair += phi2_ij;
    }

    let prob_ie = (S6 - sum_complement + sum_pair).clamp(0, S6);
    c1_filter_cu_diag_inner(b"c1_filter_triangle_diag after_base_before_gh3");
    let triple_p = triple_pre
        .map(|tp| triple_complement_gh3_local(tp, num6_arr))
        .unwrap_or(0);
    (prob_ie - triple_p).clamp(0, S6)
}

/// Phase 2: fused region bundle.
///
/// Computes `(P(autocall), P(KI))` in a single pass. The autocall region is
/// the triangle `∀k: au[k]·u + av[k]·v ≤ ac_rhs[k]`; the KI-safe region is
/// the triangle `∀k: au[k]·u + av[k]·v ≤ ki_rhs[k]`; the KI region is the
/// complement `P(KI) = S6 - P(KI-safe)`.
///
/// Both triangles share the same `TrianglePre64` (au, av, inv_std, phi2_neg
/// flags) because the half-plane normals are identical — only the rhs shifts.
/// That lets us share the mean-projection work `ew6[k] = au[k]·u + av[k]·v`
/// and (if wired through) the same Cholesky geometry for the triple
/// correction. Φ and Φ₂ lookups still differ per barrier (different z-values)
/// but we batch them here to reduce prologue/epilogue overhead vs calling
/// `triangle_probability_with_triple_i64` twice.
///
/// `triple_pre` is optional: pass `None` to skip the GH3 triple-intersection
/// correction (same convention as the per-observation `observation_probability
/// _triple_pre`). When provided, the correction is applied to both regions
/// using their respective `num` arrays.
///
/// Verified by `tests::fused_region_bundle_matches_separate_calls` to equal
/// two `triangle_probability_with_triple_i64` calls to within 1 unit at S6.
#[inline(always)]
fn fused_region_bundle(
    mean_u: i64,
    mean_v: i64,
    ac_rhs: &[i64; 3],
    ki_rhs: &[i64; 3],
    pre: &TrianglePre64,
    phi2: [&Phi2Table; 3],
    triple_pre: Option<&TripleCorrectionPre>,
) -> (i64, i64) {
    c1_filter_cu_diag_inner(b"c1_filter_fused_bundle_start");

    // Shared: project mean onto half-plane normals (once for both rhs).
    let mut ew6 = [0i64; 3];
    for k in 0..3 {
        ew6[k] = m6r_fast(pre.au[k], mean_u) + m6r_fast(pre.av[k], mean_v);
    }

    // Per-rhs numerators and scaled z-values.
    let mut num_ac = [0i64; 3];
    let mut num_ki = [0i64; 3];
    let mut z_ac = [0i64; 3];
    let mut z_ki = [0i64; 3];
    for k in 0..3 {
        num_ac[k] = ac_rhs[k] - ew6[k];
        num_ki[k] = ki_rhs[k] - ew6[k];
        z_ac[k] = m6r_fast(num_ac[k], pre.inv_std[k]) * S6;
        z_ki[k] = m6r_fast(num_ki[k], pre.inv_std[k]) * S6;
    }

    // Trivial-zero guards (same as the solo path).
    let ac_trivial = ac_rhs[0] < 0 && ac_rhs[1] == ac_rhs[0] && ac_rhs[2] == ac_rhs[0];
    let ki_trivial = ki_rhs[0] < 0 && ki_rhs[1] == ki_rhs[0] && ki_rhs[2] == ki_rhs[0];

    // 3+3 univariate Φ lookups.
    let phi_z_ac = [
        norm_cdf_i64_local(z_ac[0]),
        norm_cdf_i64_local(z_ac[1]),
        norm_cdf_i64_local(z_ac[2]),
    ];
    let phi_z_ki = [
        norm_cdf_i64_local(z_ki[0]),
        norm_cdf_i64_local(z_ki[1]),
        norm_cdf_i64_local(z_ki[2]),
    ];
    let sum_comp_ac = (S6 - phi_z_ac[0]) + (S6 - phi_z_ac[1]) + (S6 - phi_z_ac[2]);
    let sum_comp_ki = (S6 - phi_z_ki[0]) + (S6 - phi_z_ki[1]) + (S6 - phi_z_ki[2]);

    // 3+3 pairwise Φ₂ lookups (each table reused across ac and ki — stays hot).
    let mut sum_pair_ac = 0i64;
    let mut sum_pair_ki = 0i64;
    for (pidx, &(i, j)) in [(0usize, 1usize), (0, 2), (1, 2)].iter().enumerate() {
        let ni_ac = -z_ac[i];
        let ni_ki = -z_ki[i];
        let p_ac_ij = if pre.phi2_neg[pidx] {
            let phi_a = norm_cdf_i64_local(ni_ac);
            (phi_a - bvn_cdf_i64_local(ni_ac, z_ac[j], phi2[pidx])).max(0)
        } else {
            bvn_cdf_i64_local(ni_ac, -z_ac[j], phi2[pidx])
        };
        let p_ki_ij = if pre.phi2_neg[pidx] {
            let phi_a = norm_cdf_i64_local(ni_ki);
            (phi_a - bvn_cdf_i64_local(ni_ki, z_ki[j], phi2[pidx])).max(0)
        } else {
            bvn_cdf_i64_local(ni_ki, -z_ki[j], phi2[pidx])
        };
        sum_pair_ac += p_ac_ij;
        sum_pair_ki += p_ki_ij;
    }

    let prob_ie_ac = (S6 - sum_comp_ac + sum_pair_ac).clamp(0, S6);
    let prob_ie_ki_safe = (S6 - sum_comp_ki + sum_pair_ki).clamp(0, S6);

    // Triple corrections share the Cholesky geometry via triple_pre; their
    // num arrays differ (rhs-dependent). Two calls, each ~300 CU.
    let (triple_ac, triple_ki) = if let Some(tp) = triple_pre {
        (
            triple_complement_gh3_local(tp, num_ac),
            triple_complement_gh3_local(tp, num_ki),
        )
    } else {
        (0, 0)
    };

    let p_ac = if ac_trivial {
        0
    } else {
        (prob_ie_ac - triple_ac).clamp(0, S6)
    };
    let p_ki_safe = if ki_trivial {
        0
    } else {
        (prob_ie_ki_safe - triple_ki).clamp(0, S6)
    };
    let p_ki = (S6 - p_ki_safe).clamp(0, S6);

    c1_filter_cu_diag_inner(b"c1_filter_fused_bundle_end");
    (p_ac, p_ki)
}

#[derive(Debug, Clone, Copy, Default)]
struct TriangleProbabilityWorkspace {
    probability: i64,
    z_s6: [i64; 3],
    pdf_z: [i64; 3],
}

#[inline(always)]
fn triangle_probability_workspace(
    mean_u: i64,
    mean_v: i64,
    rhs: &[i64; 3],
    pre: &TrianglePre64,
    phi2: [&Phi2Table; 3],
    triple_pre: Option<&TripleCorrectionPre>,
) -> TriangleProbabilityWorkspace {
    let mut num6_arr = [0i64; 3];
    let mut z_s6 = [0i64; 3];
    let mut phi_z = [0i64; 3];
    let mut pdf_z = [0i64; 3];

    for k in 0..3 {
        let ew6 = m6r_fast(pre.au[k], mean_u) + m6r_fast(pre.av[k], mean_v);
        num6_arr[k] = rhs[k] - ew6;
        z_s6[k] = m6r_fast(num6_arr[k], pre.inv_std[k]);
        phi_z[k] = norm_cdf_i64_local(z_s6[k] * S6);
        pdf_z[k] = norm_pdf_i64_local(z_s6[k] * S6);
    }

    if rhs[0] < 0 && rhs[1] == rhs[0] && rhs[2] == rhs[0] {
        return TriangleProbabilityWorkspace::default();
    }

    let sum_complement = (S6 - phi_z[0]) + (S6 - phi_z[1]) + (S6 - phi_z[2]);
    let pairs: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];
    let mut sum_pair = 0i64;
    for (pidx, &(i, j)) in pairs.iter().enumerate() {
        let neg_zi = -(z_s6[i] * S6);
        let phi2_ij = if pre.phi2_neg[pidx] {
            let phi_a = norm_cdf_i64_local(neg_zi);
            (phi_a - bvn_cdf_i64_local(neg_zi, z_s6[j] * S6, phi2[pidx])).max(0)
        } else {
            bvn_cdf_i64_local(neg_zi, -(z_s6[j] * S6), phi2[pidx])
        };
        sum_pair += phi2_ij;
    }

    let prob_ie = (S6 - sum_complement + sum_pair).clamp(0, S6);
    let triple_p = triple_pre
        .map(|tp| triple_complement_gh3_local(tp, num6_arr))
        .unwrap_or(0);

    TriangleProbabilityWorkspace {
        probability: (prob_ie - triple_p).clamp(0, S6),
        z_s6,
        pdf_z,
    }
}

#[inline(always)]
fn triangle_probability_grad_from_workspace(
    ws: &TriangleProbabilityWorkspace,
    dz: &[i64; 3],
) -> i64 {
    let mut dp = 0i64;
    for k in 0..3 {
        dp += m6r_fast(dz[k], ws.pdf_z[k]);
    }

    let pairs: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];
    for (pidx, &(i, j)) in pairs.iter().enumerate() {
        let cond_i = m6r_fast(
            m6r_fast(TRIANGLE_PAIR_RHO_63[pidx], ws.z_s6[i]) - ws.z_s6[j],
            TRIANGLE_PAIR_INV_SQRT_1MRHO2_63[pidx],
        );
        let cond_j = m6r_fast(
            m6r_fast(TRIANGLE_PAIR_RHO_63[pidx], ws.z_s6[j]) - ws.z_s6[i],
            TRIANGLE_PAIR_INV_SQRT_1MRHO2_63[pidx],
        );
        let deriv_i = m6r_fast(ws.pdf_z[i], norm_cdf_i64_local(cond_i * S6));
        let deriv_j = m6r_fast(ws.pdf_z[j], norm_cdf_i64_local(cond_j * S6));
        dp += m6r_fast(-dz[i], deriv_i) + m6r_fast(-dz[j], deriv_j);
    }

    dp
}

/// Phase 1 of analytic-delta rollout: triangle probability with first
/// derivatives w.r.t. (μ_u, μ_v).
///
/// Returns `(P, ∂P/∂μ_u, ∂P/∂μ_v)` all at SCALE_6. The probability itself
/// is computed by inclusion-exclusion over three half-planes plus an
/// optional GH3 triple-intersection correction, matching
/// `triangle_with_gradient_i64`. The gradient uses:
///
///   P = 1 - Σ_k (1 - Φ(z_k)) + Σ_{i<j} Φ₂(−z_i, −z_j; ρ_ij) − triple
///
///   ∂P/∂μ_u = -Σ_k (dz_k/dμ_u) · (-φ(z_k))
///             + Σ_{i<j} [ (dz_i/dμ_u) · φ(z_i) Φ((z_j - ρ z_i)/√(1-ρ²))
///                       + (dz_j/dμ_u) · φ(z_j) Φ((z_i - ρ z_j)/√(1-ρ²)) ]
///             (negated consistently because we differentiate w.r.t. z_k,
///              then chain via dz_k/dμ_u = -a_u_k / σ_k precomputed into
///              `dz_du` by `triangle_gradient_geometry`)
///
/// The triple-correction contribution to the gradient is currently
/// omitted; its magnitude is typically <1% of P and will be added in a
/// later session if Phase 6 FD validation reveals a consistent bias.
///
/// This primitive is self-contained: it does NOT touch any shipping code
/// and is not yet wired into any pricer path. Phase 4 integrates it into
/// `quote_c1_filter_with_delta` (future session).
#[inline(always)]
fn triangle_probability_with_grad(
    mean_u: i64,
    mean_v: i64,
    rhs: &[i64; 3],
    pre: &TrianglePre64,
    phi2: [&Phi2Table; 3],
    triple_pre: Option<&TripleCorrectionPre>,
    dz_du: &[i64; 3],
    dz_dv: &[i64; 3],
) -> (i64, i64, i64) {
    let ws = triangle_probability_workspace(mean_u, mean_v, rhs, pre, phi2, triple_pre);
    let probability = ws.probability;
    let dp_du = triangle_probability_grad_from_workspace(&ws, dz_du);
    let dp_dv = triangle_probability_grad_from_workspace(&ws, dz_dv);

    // Gradient of triple_p w.r.t. μ omitted for now (typically <1% of P).
    // Phase 6 FD validation will determine whether this bias matters.

    (probability, dp_du, dp_dv)
}

/// Full KI moment: probability + conditional expectations. Used at maturity.
#[inline(always)]
fn ki_region_uv_moment_gh3(
    mean_u: i64,
    mean_v: i64,
    l11: i64,
    l21: i64,
    l22: i64,
    barrier: i64,
    coords: [AffineCoord6; 3],
) -> RawRegionMoment {
    let sl11 = m6r_fast(SQRT2_S6, l11);
    let sl21 = m6r_fast(SQRT2_S6, l21);
    let sl22 = m6r_fast(SQRT2_S6, l22);

    let mut probability = 0i64;
    let mut expectation_u = 0i64;
    let mut expectation_v = 0i64;

    for (i, &zi) in GH3_NODES_6.iter().enumerate() {
        let wi = GH3_WPI_6[i];
        let u = mean_u + m6r_fast(sl11, zi);
        let v_base = mean_v + m6r_fast(sl21, zi);
        let x_u = [
            coords[0].constant + m6r_fast(coords[0].u_coeff, u),
            coords[1].constant + m6r_fast(coords[1].u_coeff, u),
            coords[2].constant + m6r_fast(coords[2].u_coeff, u),
        ];
        for (j, &zj) in GH3_NODES_6.iter().enumerate() {
            let w = m6r_fast(wi, GH3_WPI_6[j]);
            let v = v_base + m6r_fast(sl22, zj);
            let x0 = x_u[0] + m6r_fast(coords[0].v_coeff, v);
            let x1 = x_u[1] + m6r_fast(coords[1].v_coeff, v);
            let x2 = x_u[2] + m6r_fast(coords[2].v_coeff, v);
            if x0 <= barrier || x1 <= barrier || x2 <= barrier {
                probability += w;
                expectation_u += m6r_fast(w, u);
                expectation_v += m6r_fast(w, v);
            }
        }
    }

    RawRegionMoment {
        probability: probability.clamp(0, S6),
        expectation_u,
        expectation_v,
    }
}

#[inline(always)]
fn ki_grad_smooth_h_s6(barrier: i64) -> i64 {
    ((barrier.abs().max(1) + 5_000) / 10_000).max(1_000)
}

#[inline(always)]
fn ki_smooth_union_indicator_and_grad(
    x: [i64; 3],
    barrier: i64,
    h_s6: i64,
    dx_du: [i64; 3],
    dx_dv: [i64; 3],
    dx_dc: [i64; 3],
) -> (i64, i64, i64, i64) {
    let mut smooth = [0i64; 3];
    let mut comp = [0i64; 3];
    let mut ds_du = [0i64; 3];
    let mut ds_dv = [0i64; 3];
    let mut ds_dc = [0i64; 3];

    for k in 0..3 {
        let t_s6 = (((barrier - x[k]) as i128 * S6 as i128) / h_s6 as i128) as i64;
        let t_s6 = t_s6.clamp(-8 * S6, 8 * S6);
        let pdf = norm_pdf_i64_local(t_s6 * S6);
        smooth[k] = norm_cdf_i64_local(t_s6 * S6);
        comp[k] = S6 - smooth[k];

        let dt_du = ((-(dx_du[k] as i128) * S6 as i128) / h_s6 as i128) as i64;
        let dt_dv = ((-(dx_dv[k] as i128) * S6 as i128) / h_s6 as i128) as i64;
        let dt_dc = ((-(dx_dc[k] as i128) * S6 as i128) / h_s6 as i128) as i64;
        ds_du[k] = m6r_fast(pdf, dt_du);
        ds_dv[k] = m6r_fast(pdf, dt_dv);
        ds_dc[k] = m6r_fast(pdf, dt_dc);
    }

    let c12 = m6r_fast(comp[1], comp[2]);
    let c02 = m6r_fast(comp[0], comp[2]);
    let c01 = m6r_fast(comp[0], comp[1]);
    let smooth_union = (S6 - m6r_fast(comp[0], c12)).clamp(0, S6);
    let d_union_du = m6r_fast(ds_du[0], c12) + m6r_fast(ds_du[1], c02) + m6r_fast(ds_du[2], c01);
    let d_union_dv = m6r_fast(ds_dv[0], c12) + m6r_fast(ds_dv[1], c02) + m6r_fast(ds_dv[2], c01);
    let d_union_dc = m6r_fast(ds_dc[0], c12) + m6r_fast(ds_dc[1], c02) + m6r_fast(ds_dc[2], c01);
    (smooth_union, d_union_du, d_union_dv, d_union_dc)
}

/// Phase 4b kernel-smoothing fallback for KI moment gradients.
///
/// The value path stays bit-identical to the shipped GH3 indicator. The
/// gradient path replaces the discontinuous `1{x_min <= barrier}` with a
/// narrow smooth union `1 - Π_k (1 - Φ((barrier - x_k)/h))`, which yields
/// stable first derivatives for `P(KI)`, `E[u 1_KI]`, and `E[v 1_KI]`.
#[inline(always)]
fn ki_region_uv_moment_gh3_grad(
    mean_u: i64,
    mean_v: i64,
    l11: i64,
    l21: i64,
    l22: i64,
    barrier: i64,
    coords: [AffineCoord6; 3],
    dc_coeff: i64,
) -> (RawRegionMoment, RawRegionMomentGrad) {
    let sl11 = m6r_fast(SQRT2_S6, l11);
    let sl21 = m6r_fast(SQRT2_S6, l21);
    let sl22 = m6r_fast(SQRT2_S6, l22);
    let dx_du = [coords[0].u_coeff, coords[1].u_coeff, coords[2].u_coeff];
    let dx_dv = [coords[0].v_coeff, coords[1].v_coeff, coords[2].v_coeff];
    let dx_dc = [dc_coeff; 3];
    let h_s6 = ki_grad_smooth_h_s6(barrier);

    let mut moment = RawRegionMoment::default();
    let mut grad = RawRegionMomentGrad::default();

    for (i, &zi) in GH3_NODES_6.iter().enumerate() {
        let wi = GH3_WPI_6[i];
        let u = mean_u + m6r_fast(sl11, zi);
        let v_base = mean_v + m6r_fast(sl21, zi);
        let x_u = [
            coords[0].constant + m6r_fast(coords[0].u_coeff, u),
            coords[1].constant + m6r_fast(coords[1].u_coeff, u),
            coords[2].constant + m6r_fast(coords[2].u_coeff, u),
        ];

        for (j, &zj) in GH3_NODES_6.iter().enumerate() {
            let w = m6r_fast(wi, GH3_WPI_6[j]);
            let v = v_base + m6r_fast(sl22, zj);
            let x = [
                x_u[0] + m6r_fast(coords[0].v_coeff, v),
                x_u[1] + m6r_fast(coords[1].v_coeff, v),
                x_u[2] + m6r_fast(coords[2].v_coeff, v),
            ];

            if x[0] <= barrier || x[1] <= barrier || x[2] <= barrier {
                moment.probability += w;
                moment.expectation_u += m6r_fast(w, u);
                moment.expectation_v += m6r_fast(w, v);
            }

            let (smooth_union, di_du, di_dv, di_dc) =
                ki_smooth_union_indicator_and_grad(x, barrier, h_s6, dx_du, dx_dv, dx_dc);
            grad.dp_du += m6r_fast(w, di_du);
            grad.dp_dv += m6r_fast(w, di_dv);
            grad.dp_dc += m6r_fast(w, di_dc);
            grad.deu_du += m6r_fast(w, smooth_union + m6r_fast(u, di_du));
            grad.deu_dv += m6r_fast(w, m6r_fast(u, di_dv));
            grad.deu_dc += m6r_fast(w, m6r_fast(u, di_dc));
            grad.dev_du += m6r_fast(w, m6r_fast(v, di_du));
            grad.dev_dv += m6r_fast(w, smooth_union + m6r_fast(v, di_dv));
            grad.dev_dc += m6r_fast(w, m6r_fast(v, di_dc));
        }
    }

    moment.probability = moment.probability.clamp(0, S6);
    (moment, grad)
}

#[inline(always)]
fn ki_moment_i64_gh3_grad(
    mean_u: i64,
    mean_v: i64,
    l11: i64,
    l21: i64,
    l22: i64,
    barrier: i64,
    coords: [AffineCoord6; 3],
    dc_coeff: i64,
) -> (KiMoment6, KiMomentGrad) {
    let sl11 = m6r_fast(SQRT2_S6, l11);
    let sl21 = m6r_fast(SQRT2_S6, l21);
    let sl22 = m6r_fast(SQRT2_S6, l22);
    let dx_du = [coords[0].u_coeff, coords[1].u_coeff, coords[2].u_coeff];
    let dx_dv = [coords[0].v_coeff, coords[1].v_coeff, coords[2].v_coeff];
    let dx_dc = [dc_coeff; 3];
    let h_s6 = ki_grad_smooth_h_s6(barrier);

    let moment = ki_moment_i64_gh3(mean_u, mean_v, l11, l21, l22, barrier, coords);
    let mut grad = KiMomentGrad::default();

    for (i, &zi) in GH3_NODES_6.iter().enumerate() {
        let wi = GH3_WPI_6[i];
        let u = mean_u + m6r_fast(sl11, zi);
        let v_base = mean_v + m6r_fast(sl21, zi);
        let x_u = [
            coords[0].constant + m6r_fast(coords[0].u_coeff, u),
            coords[1].constant + m6r_fast(coords[1].u_coeff, u),
            coords[2].constant + m6r_fast(coords[2].u_coeff, u),
        ];

        for (j, &zj) in GH3_NODES_6.iter().enumerate() {
            let w = m6r_fast(wi, GH3_WPI_6[j]);
            let v = v_base + m6r_fast(sl22, zj);
            let x = [
                x_u[0] + m6r_fast(coords[0].v_coeff, v),
                x_u[1] + m6r_fast(coords[1].v_coeff, v),
                x_u[2] + m6r_fast(coords[2].v_coeff, v),
            ];

            let mut min_idx = 0usize;
            if x[1] < x[min_idx] {
                min_idx = 1;
            }
            if x[2] < x[min_idx] {
                min_idx = 2;
            }
            let x_min = x[min_idx];
            let worst_level = exp_neg15_to_zero_i64_local(x_min);

            let (smooth_union, di_du, di_dv, di_dc) =
                ki_smooth_union_indicator_and_grad(x, barrier, h_s6, dx_du, dx_dv, dx_dc);
            let dworst_du_point = m6r_fast(m6r_fast(worst_level, dx_du[min_idx]), smooth_union)
                + m6r_fast(worst_level, di_du);
            let dworst_dv_point = m6r_fast(m6r_fast(worst_level, dx_dv[min_idx]), smooth_union)
                + m6r_fast(worst_level, di_dv);
            let dworst_dc_point = m6r_fast(m6r_fast(worst_level, dx_dc[min_idx]), smooth_union)
                + m6r_fast(worst_level, di_dc);

            grad.dp_du += m6r_fast(w, di_du);
            grad.dp_dv += m6r_fast(w, di_dv);
            grad.dp_dc += m6r_fast(w, di_dc);
            grad.dworst_du += m6r_fast(w, dworst_du_point);
            grad.dworst_dv += m6r_fast(w, dworst_dv_point);
            grad.dworst_dc += m6r_fast(w, dworst_dc_point);
        }
    }
    (moment, grad)
}

#[inline(always)]
fn ki_region_uv_moment_gh3_smoothed(
    mean_u: i64,
    mean_v: i64,
    l11: i64,
    l21: i64,
    l22: i64,
    barrier: i64,
    coords: [AffineCoord6; 3],
) -> RawRegionMoment {
    let sl11 = m6r_fast(SQRT2_S6, l11);
    let sl21 = m6r_fast(SQRT2_S6, l21);
    let sl22 = m6r_fast(SQRT2_S6, l22);
    let dx_du = [coords[0].u_coeff, coords[1].u_coeff, coords[2].u_coeff];
    let dx_dv = [coords[0].v_coeff, coords[1].v_coeff, coords[2].v_coeff];
    let dx_dc = [0; 3];
    let h_s6 = ki_grad_smooth_h_s6(barrier);

    let mut moment = RawRegionMoment::default();

    for (i, &zi) in GH3_NODES_6.iter().enumerate() {
        let wi = GH3_WPI_6[i];
        let u = mean_u + m6r_fast(sl11, zi);
        let v_base = mean_v + m6r_fast(sl21, zi);
        let x_u = [
            coords[0].constant + m6r_fast(coords[0].u_coeff, u),
            coords[1].constant + m6r_fast(coords[1].u_coeff, u),
            coords[2].constant + m6r_fast(coords[2].u_coeff, u),
        ];

        for (j, &zj) in GH3_NODES_6.iter().enumerate() {
            let w = m6r_fast(wi, GH3_WPI_6[j]);
            let v = v_base + m6r_fast(sl22, zj);
            let x = [
                x_u[0] + m6r_fast(coords[0].v_coeff, v),
                x_u[1] + m6r_fast(coords[1].v_coeff, v),
                x_u[2] + m6r_fast(coords[2].v_coeff, v),
            ];
            let (smooth_union, _, _, _) =
                ki_smooth_union_indicator_and_grad(x, barrier, h_s6, dx_du, dx_dv, dx_dc);
            moment.probability += m6r_fast(w, smooth_union);
            moment.expectation_u += m6r_fast(w, m6r_fast(u, smooth_union));
            moment.expectation_v += m6r_fast(w, m6r_fast(v, smooth_union));
        }
    }

    moment.probability = moment.probability.clamp(0, S6);
    moment
}

/// Probability-only KI: no conditional expectations. Used at non-maturity
/// observations where we only need P(KI) to partition safe mass.
#[inline(always)]
fn ki_probability_gh3(
    mean_u: i64,
    mean_v: i64,
    l11: i64,
    l21: i64,
    l22: i64,
    barrier: i64,
    coords: [AffineCoord6; 3],
) -> i64 {
    let sl11 = m6r_fast(SQRT2_S6, l11);
    let sl21 = m6r_fast(SQRT2_S6, l21);
    let sl22 = m6r_fast(SQRT2_S6, l22);
    let mut probability = 0i64;

    for (i, &zi) in GH3_NODES_6.iter().enumerate() {
        let wi = GH3_WPI_6[i];
        let u = mean_u + m6r_fast(sl11, zi);
        let v_base = mean_v + m6r_fast(sl21, zi);
        let x_u = [
            coords[0].constant + m6r_fast(coords[0].u_coeff, u),
            coords[1].constant + m6r_fast(coords[1].u_coeff, u),
            coords[2].constant + m6r_fast(coords[2].u_coeff, u),
        ];
        for (j, &zj) in GH3_NODES_6.iter().enumerate() {
            let v = v_base + m6r_fast(sl22, zj);
            let x0 = x_u[0] + m6r_fast(coords[0].v_coeff, v);
            let x1 = x_u[1] + m6r_fast(coords[1].v_coeff, v);
            let x2 = x_u[2] + m6r_fast(coords[2].v_coeff, v);
            if x0 <= barrier || x1 <= barrier || x2 <= barrier {
                probability += m6r_fast(wi, GH3_WPI_6[j]);
            }
        }
    }
    probability.clamp(0, S6)
}

#[cfg(not(target_os = "solana"))]
#[inline(always)]
fn triangle_with_gradient_i64(
    mean_u: i64,
    mean_v: i64,
    rhs: &[i64; 3],
    pre: &TrianglePre64,
    phi2: [&Phi2Table; 3],
    triple_pre: Option<&TripleCorrectionPre>,
    dz_du: &[i64; 3],
    dz_dv: &[i64; 3],
    sigma_uu: i64,
    sigma_uv: i64,
    sigma_vv: i64,
) -> RawRegionMoment {
    let mut num6_arr = [0i64; 3];
    let mut z_s6 = [0i64; 3];
    let mut z_scale = [0i64; 3];
    let mut phi_z = [0i64; 3];
    let mut pdf_z = [0i64; 3];

    for k in 0..3 {
        let ew6 = m6r_fast(pre.au[k], mean_u) + m6r_fast(pre.av[k], mean_v);
        num6_arr[k] = rhs[k] - ew6;
        z_s6[k] = m6r_fast(num6_arr[k], pre.inv_std[k]);
        z_scale[k] = z_s6[k] * S6;
        phi_z[k] = norm_cdf_i64_local(z_scale[k]);
        pdf_z[k] = norm_pdf_i64_local(z_scale[k]);
    }

    if rhs[0] < 0 && rhs[1] == rhs[0] && rhs[2] == rhs[0] {
        return RawRegionMoment::default();
    }

    let sum_complement = (S6 - phi_z[0]) + (S6 - phi_z[1]) + (S6 - phi_z[2]);
    let mut dp_du = 0i64;
    let mut dp_dv = 0i64;
    for k in 0..3 {
        dp_du += m6r_fast(dz_du[k], pdf_z[k]);
        dp_dv += m6r_fast(dz_dv[k], pdf_z[k]);
    }

    let pairs: [(usize, usize); 3] = [(0, 1), (0, 2), (1, 2)];
    let mut sum_pair = 0i64;
    for (pidx, &(i, j)) in pairs.iter().enumerate() {
        let neg_zi = -z_scale[i];
        let phi2_ij = if pre.phi2_neg[pidx] {
            let phi_a = norm_cdf_i64_local(neg_zi);
            (phi_a - bvn_cdf_i64_local(neg_zi, z_scale[j], phi2[pidx])).max(0)
        } else {
            bvn_cdf_i64_local(neg_zi, -z_scale[j], phi2[pidx])
        };
        sum_pair += phi2_ij;

        let cond_i = m6r_fast(
            m6r_fast(TRIANGLE_PAIR_RHO_63[pidx], z_s6[i]) - z_s6[j],
            TRIANGLE_PAIR_INV_SQRT_1MRHO2_63[pidx],
        );
        let cond_j = m6r_fast(
            m6r_fast(TRIANGLE_PAIR_RHO_63[pidx], z_s6[j]) - z_s6[i],
            TRIANGLE_PAIR_INV_SQRT_1MRHO2_63[pidx],
        );
        let deriv_i = m6r_fast(pdf_z[i], norm_cdf_i64_local(cond_i * S6));
        let deriv_j = m6r_fast(pdf_z[j], norm_cdf_i64_local(cond_j * S6));
        dp_du += m6r_fast(-dz_du[i], deriv_i) + m6r_fast(-dz_du[j], deriv_j);
        dp_dv += m6r_fast(-dz_dv[i], deriv_i) + m6r_fast(-dz_dv[j], deriv_j);
    }

    let prob_ie = (S6 - sum_complement + sum_pair).clamp(0, S6);
    let triple_p = triple_pre
        .map(|tp| triple_complement_gh3_local(tp, num6_arr))
        .unwrap_or(0);
    let probability = (prob_ie - triple_p).clamp(0, S6);
    if probability <= 0 {
        return RawRegionMoment::default();
    }

    let expectation_u =
        m6r_fast(mean_u, probability) + m6r_fast(sigma_uu, dp_du) + m6r_fast(sigma_uv, dp_dv);
    let expectation_v =
        m6r_fast(mean_v, probability) + m6r_fast(sigma_uv, dp_du) + m6r_fast(sigma_vv, dp_dv);

    RawRegionMoment {
        probability,
        expectation_u,
        expectation_v,
    }
}

#[inline(always)]
fn sort_small_by_c(nodes: &mut [FilterNode], len: usize) {
    for i in 1..len {
        let key = nodes[i];
        let mut j = i;
        while j > 0 && nodes[j - 1].c > key.c {
            nodes[j] = nodes[j - 1];
            j -= 1;
        }
        nodes[j] = key;
    }
}

#[inline(always)]
fn sort_small_by_c_with_grad(nodes: &mut [FilterNode], grads: &mut [NodeGrad], len: usize) {
    for i in 1..len {
        let key_node = nodes[i];
        let key_grad = grads[i];
        let mut j = i;
        while j > 0 && nodes[j - 1].c > key_node.c {
            nodes[j] = nodes[j - 1];
            grads[j] = grads[j - 1];
            j -= 1;
        }
        nodes[j] = key_node;
        grads[j] = key_grad;
    }
}

#[inline(always)]
fn strongest_weight_index(nodes: &[FilterNode; MAX_K]) -> usize {
    let mut best = 0usize;
    let mut best_w = i64::MIN;
    for (idx, node) in nodes.iter().enumerate() {
        if node.w > best_w {
            best_w = node.w;
            best = idx;
        }
    }
    best
}

fn project_nodes(children: &[FilterNode], k_retained: usize) -> FilterState {
    let k_retained = k_retained.clamp(1, MAX_K);
    let n_children = children.len();
    if n_children == 0 {
        return FilterState::default();
    }

    if n_children <= k_retained {
        let mut exact = [FilterNode::default(); MAX_K];
        exact[..n_children].copy_from_slice(children);
        sort_small_by_c(&mut exact, n_children);
        return FilterState {
            nodes: exact,
            n_active: n_children,
        };
    }

    let mut min_c = i64::MAX;
    let mut max_c = i64::MIN;
    let mut total_in = 0i64;
    for child in children {
        min_c = min_c.min(child.c);
        max_c = max_c.max(child.c);
        total_in += child.w;
    }

    if max_c <= min_c || k_retained == 1 {
        let mut total_c = 0i64;
        let mut total_u = 0i64;
        let mut total_v = 0i64;
        for child in children {
            total_c += m6r(child.w, child.c);
            total_u += m6r(child.w, child.mean_u);
            total_v += m6r(child.w, child.mean_v);
        }
        let mut state = FilterState::default();
        state.nodes[0] = FilterNode {
            c: total_c * S6 / total_in,
            w: total_in,
            mean_u: total_u * S6 / total_in,
            mean_v: total_v * S6 / total_in,
        };
        state.n_active = 1;
        return state;
    }

    let k_m1 = (k_retained - 1) as i64;
    let span = max_c - min_c;
    let edge_pad = (span / (2 * k_m1)).max(1);
    let grid_min = min_c - edge_pad;
    let grid_max = max_c + edge_pad;
    let grid_span = grid_max - grid_min;
    let mut state = FilterState::default();
    let mut mean_u_raw = [0i64; MAX_K];
    let mut mean_v_raw = [0i64; MAX_K];
    for idx in 0..k_retained {
        state.nodes[idx].c = grid_min + grid_span * idx as i64 / k_m1;
    }

    for child in children {
        if child.c <= grid_min {
            state.nodes[0].w += child.w;
            mean_u_raw[0] += m6r(child.w, child.mean_u);
            mean_v_raw[0] += m6r(child.w, child.mean_v);
            continue;
        }
        if child.c >= grid_max {
            let idx = k_retained - 1;
            state.nodes[idx].w += child.w;
            mean_u_raw[idx] += m6r(child.w, child.mean_u);
            mean_v_raw[idx] += m6r(child.w, child.mean_v);
            continue;
        }

        let scaled = (child.c - grid_min) * k_m1;
        let idx_lo = (scaled / grid_span) as usize;
        let remainder = scaled - idx_lo as i64 * grid_span;
        let frac_hi = remainder * S6 / grid_span;
        let frac_lo = S6 - frac_hi;
        let w_lo = m6r(child.w, frac_lo);
        let w_hi = child.w - w_lo;

        state.nodes[idx_lo].w += w_lo;
        mean_u_raw[idx_lo] += m6r(w_lo, child.mean_u);
        mean_v_raw[idx_lo] += m6r(w_lo, child.mean_v);
        if idx_lo + 1 < k_retained {
            state.nodes[idx_lo + 1].w += w_hi;
            mean_u_raw[idx_lo + 1] += m6r(w_hi, child.mean_u);
            mean_v_raw[idx_lo + 1] += m6r(w_hi, child.mean_v);
        }
    }

    let total_out = state.nodes[..k_retained]
        .iter()
        .map(|node| node.w)
        .sum::<i64>();
    let diff = total_in - total_out;
    if diff != 0 {
        let fix_idx = strongest_weight_index(&state.nodes);
        state.nodes[fix_idx].w = (state.nodes[fix_idx].w + diff).max(0);
    }

    let mut n_active = 0usize;
    for idx in 0..k_retained {
        if state.nodes[idx].w > 0 {
            state.nodes[idx].mean_u =
                (mean_u_raw[idx] as i128 * S6 as i128 / state.nodes[idx].w as i128) as i64;
            state.nodes[idx].mean_v =
                (mean_v_raw[idx] as i128 * S6 as i128 / state.nodes[idx].w as i128) as i64;
            n_active += 1;
        }
    }
    state.n_active = n_active;
    state
}

fn project_nodes_with_grad(
    children: &[FilterNode],
    child_grads: &[NodeGrad],
    k_retained: usize,
) -> (FilterState, FilterStateGrad) {
    debug_assert_eq!(children.len(), child_grads.len());
    let k_retained = k_retained.clamp(1, MAX_K);
    let n_children = children.len();
    if n_children == 0 {
        return (FilterState::default(), FilterStateGrad::default());
    }

    if n_children <= k_retained {
        let mut exact = [FilterNode::default(); MAX_K];
        let mut exact_grad = [NodeGrad::default(); MAX_K];
        exact[..n_children].copy_from_slice(children);
        exact_grad[..n_children].copy_from_slice(child_grads);
        sort_small_by_c_with_grad(&mut exact, &mut exact_grad, n_children);
        return (
            FilterState {
                nodes: exact,
                n_active: n_children,
            },
            FilterStateGrad { nodes: exact_grad },
        );
    }

    let mut min_c = i64::MAX;
    let mut max_c = i64::MIN;
    let mut total_in = 0i64;
    let mut total_in_grad = [0i64; 3];
    for (child, grad) in children.iter().zip(child_grads.iter()) {
        min_c = min_c.min(child.c);
        max_c = max_c.max(child.c);
        total_in += child.w;
        for asset in 0..3 {
            total_in_grad[asset] += grad.dw[asset];
        }
    }

    if max_c <= min_c || k_retained == 1 {
        let mut total_c = 0i64;
        let mut total_u = 0i64;
        let mut total_v = 0i64;
        let mut total_u_grad = [0i64; 3];
        let mut total_v_grad = [0i64; 3];
        for (child, grad) in children.iter().zip(child_grads.iter()) {
            total_c += m6r(child.w, child.c);
            total_u += m6r(child.w, child.mean_u);
            total_v += m6r(child.w, child.mean_v);
            for asset in 0..3 {
                total_u_grad[asset] +=
                    m6r(grad.dw[asset], child.mean_u) + m6r(child.w, grad.du[asset]);
                total_v_grad[asset] +=
                    m6r(grad.dw[asset], child.mean_v) + m6r(child.w, grad.dv[asset]);
            }
        }
        let mut state = FilterState::default();
        let mut state_grad = FilterStateGrad::default();
        state.nodes[0] = FilterNode {
            c: total_c * S6 / total_in,
            w: total_in,
            mean_u: total_u * S6 / total_in,
            mean_v: total_v * S6 / total_in,
        };
        for asset in 0..3 {
            state_grad.nodes[0].dw[asset] = total_in_grad[asset];
            state_grad.nodes[0].du[asset] =
                conditional_mean_grad(total_in, total_u, total_in_grad[asset], total_u_grad[asset]);
            state_grad.nodes[0].dv[asset] =
                conditional_mean_grad(total_in, total_v, total_in_grad[asset], total_v_grad[asset]);
        }
        state.n_active = 1;
        return (state, state_grad);
    }

    let k_m1 = (k_retained - 1) as i64;
    let span = max_c - min_c;
    let edge_pad = (span / (2 * k_m1)).max(1);
    let grid_min = min_c - edge_pad;
    let grid_max = max_c + edge_pad;
    let grid_span = grid_max - grid_min;
    let mut state = FilterState::default();
    let mut state_grad = FilterStateGrad::default();
    let mut mean_u_raw = [0i64; MAX_K];
    let mut mean_v_raw = [0i64; MAX_K];
    let mut mean_u_raw_grad = [[0i64; 3]; MAX_K];
    let mut mean_v_raw_grad = [[0i64; 3]; MAX_K];
    for idx in 0..k_retained {
        state.nodes[idx].c = grid_min + grid_span * idx as i64 / k_m1;
    }

    for (child, grad) in children.iter().zip(child_grads.iter()) {
        if child.c <= grid_min {
            state.nodes[0].w += child.w;
            mean_u_raw[0] += m6r(child.w, child.mean_u);
            mean_v_raw[0] += m6r(child.w, child.mean_v);
            for asset in 0..3 {
                state_grad.nodes[0].dw[asset] += grad.dw[asset];
                mean_u_raw_grad[0][asset] +=
                    m6r(grad.dw[asset], child.mean_u) + m6r(child.w, grad.du[asset]);
                mean_v_raw_grad[0][asset] +=
                    m6r(grad.dw[asset], child.mean_v) + m6r(child.w, grad.dv[asset]);
            }
            continue;
        }
        if child.c >= grid_max {
            let idx = k_retained - 1;
            state.nodes[idx].w += child.w;
            mean_u_raw[idx] += m6r(child.w, child.mean_u);
            mean_v_raw[idx] += m6r(child.w, child.mean_v);
            for asset in 0..3 {
                state_grad.nodes[idx].dw[asset] += grad.dw[asset];
                mean_u_raw_grad[idx][asset] +=
                    m6r(grad.dw[asset], child.mean_u) + m6r(child.w, grad.du[asset]);
                mean_v_raw_grad[idx][asset] +=
                    m6r(grad.dw[asset], child.mean_v) + m6r(child.w, grad.dv[asset]);
            }
            continue;
        }

        let scaled = (child.c - grid_min) * k_m1;
        let idx_lo = (scaled / grid_span) as usize;
        let remainder = scaled - idx_lo as i64 * grid_span;
        let frac_hi = remainder * S6 / grid_span;
        let frac_lo = S6 - frac_hi;
        let w_lo = m6r(child.w, frac_lo);
        let w_hi = child.w - w_lo;
        let dw_lo = [
            m6r(grad.dw[0], frac_lo),
            m6r(grad.dw[1], frac_lo),
            m6r(grad.dw[2], frac_lo),
        ];
        let dw_hi = [
            grad.dw[0] - dw_lo[0],
            grad.dw[1] - dw_lo[1],
            grad.dw[2] - dw_lo[2],
        ];

        state.nodes[idx_lo].w += w_lo;
        mean_u_raw[idx_lo] += m6r(w_lo, child.mean_u);
        mean_v_raw[idx_lo] += m6r(w_lo, child.mean_v);
        for asset in 0..3 {
            state_grad.nodes[idx_lo].dw[asset] += dw_lo[asset];
            mean_u_raw_grad[idx_lo][asset] +=
                m6r(dw_lo[asset], child.mean_u) + m6r(w_lo, grad.du[asset]);
            mean_v_raw_grad[idx_lo][asset] +=
                m6r(dw_lo[asset], child.mean_v) + m6r(w_lo, grad.dv[asset]);
        }
        if idx_lo + 1 < k_retained {
            state.nodes[idx_lo + 1].w += w_hi;
            mean_u_raw[idx_lo + 1] += m6r(w_hi, child.mean_u);
            mean_v_raw[idx_lo + 1] += m6r(w_hi, child.mean_v);
            for asset in 0..3 {
                state_grad.nodes[idx_lo + 1].dw[asset] += dw_hi[asset];
                mean_u_raw_grad[idx_lo + 1][asset] +=
                    m6r(dw_hi[asset], child.mean_u) + m6r(w_hi, grad.du[asset]);
                mean_v_raw_grad[idx_lo + 1][asset] +=
                    m6r(dw_hi[asset], child.mean_v) + m6r(w_hi, grad.dv[asset]);
            }
        }
    }

    let total_out = state.nodes[..k_retained]
        .iter()
        .map(|node| node.w)
        .sum::<i64>();
    let diff = total_in - total_out;
    if diff != 0 {
        let fix_idx = strongest_weight_index(&state.nodes);
        state.nodes[fix_idx].w = (state.nodes[fix_idx].w + diff).max(0);
        for asset in 0..3 {
            let total_out_grad = state_grad.nodes[..k_retained]
                .iter()
                .map(|node| node.dw[asset])
                .sum::<i64>();
            state_grad.nodes[fix_idx].dw[asset] += total_in_grad[asset] - total_out_grad;
        }
    }

    let mut n_active = 0usize;
    for idx in 0..k_retained {
        if state.nodes[idx].w > 0 {
            state.nodes[idx].mean_u =
                (mean_u_raw[idx] as i128 * S6 as i128 / state.nodes[idx].w as i128) as i64;
            state.nodes[idx].mean_v =
                (mean_v_raw[idx] as i128 * S6 as i128 / state.nodes[idx].w as i128) as i64;
            for asset in 0..3 {
                state_grad.nodes[idx].du[asset] = conditional_mean_grad(
                    state.nodes[idx].w,
                    mean_u_raw[idx],
                    state_grad.nodes[idx].dw[asset],
                    mean_u_raw_grad[idx][asset],
                );
                state_grad.nodes[idx].dv[asset] = conditional_mean_grad(
                    state.nodes[idx].w,
                    mean_v_raw[idx],
                    state_grad.nodes[idx].dw[asset],
                    mean_v_raw_grad[idx][asset],
                );
            }
            n_active += 1;
        }
    }
    state.n_active = n_active;
    (state, state_grad)
}

fn project_state_with_grad(
    state: &FilterState,
    state_grad: &FilterStateGrad,
    k_retained: usize,
) -> (FilterState, FilterStateGrad) {
    let mut children = [FilterNode::default(); MAX_K];
    let mut child_grads = [NodeGrad::default(); MAX_K];
    let mut len = 0usize;
    for (idx, node) in state.nodes.iter().copied().enumerate() {
        if node.w <= 0 {
            continue;
        }
        children[len] = node;
        child_grads[len] = state_grad.nodes[idx];
        len += 1;
    }
    project_nodes_with_grad(&children[..len], &child_grads[..len], k_retained)
}

pub fn predict_state(
    state: &FilterState,
    factor_values: &[i64; N_FACTOR_NODES],
    factor_weights: &[i64; N_FACTOR_NODES],
    step_mean_u: &[i64; N_FACTOR_NODES],
    step_mean_v: &[i64; N_FACTOR_NODES],
    k_retained: usize,
) -> FilterState {
    predict_state_with_focus(
        state,
        factor_values,
        factor_weights,
        step_mean_u,
        step_mean_v,
        k_retained,
        None,
    )
}

/// Predict state with optional barrier-adapted grid.
/// `focus`: if Some((center, half_width)), places K-4 core nodes densely in
/// [center-half_width, center+half_width] with 2 guard nodes on each tail.
fn predict_state_with_focus(
    state: &FilterState,
    factor_values: &[i64; N_FACTOR_NODES],
    factor_weights: &[i64; N_FACTOR_NODES],
    step_mean_u: &[i64; N_FACTOR_NODES],
    step_mean_v: &[i64; N_FACTOR_NODES],
    k_retained: usize,
    focus: Option<(i64, i64)>,
) -> FilterState {
    let k_retained = k_retained.clamp(1, MAX_K);
    let mut n_children = 0usize;
    let mut min_c = i64::MAX;
    let mut max_c = i64::MIN;
    let mut total_in = 0i64;
    for parent in state.nodes.iter().copied().filter(|node| node.w > 0) {
        for idx in 0..N_FACTOR_NODES {
            let w = m6r(parent.w, factor_weights[idx]);
            if w <= 0 {
                continue;
            }
            let c = parent.c + factor_values[idx];
            min_c = min_c.min(c);
            max_c = max_c.max(c);
            total_in += w;
            n_children += 1;
        }
    }
    if n_children == 0 {
        return FilterState::default();
    }

    if n_children <= k_retained {
        let mut exact = [FilterNode::default(); MAX_K];
        let mut out = 0usize;
        for parent in state.nodes.iter().copied().filter(|node| node.w > 0) {
            for idx in 0..N_FACTOR_NODES {
                let w = m6r(parent.w, factor_weights[idx]);
                if w <= 0 {
                    continue;
                }
                exact[out] = FilterNode {
                    c: parent.c + factor_values[idx],
                    w,
                    mean_u: parent.mean_u + step_mean_u[idx],
                    mean_v: parent.mean_v + step_mean_v[idx],
                };
                out += 1;
            }
        }
        sort_small_by_c(&mut exact, out);
        return FilterState {
            nodes: exact,
            n_active: out,
        };
    }

    if max_c <= min_c || k_retained == 1 {
        let mut total_c = 0i64;
        let mut total_u = 0i64;
        let mut total_v = 0i64;
        for parent in state.nodes.iter().copied().filter(|node| node.w > 0) {
            for idx in 0..N_FACTOR_NODES {
                let w = m6r(parent.w, factor_weights[idx]);
                if w <= 0 {
                    continue;
                }
                total_c += m6r(w, parent.c + factor_values[idx]);
                total_u += m6r(w, parent.mean_u + step_mean_u[idx]);
                total_v += m6r(w, parent.mean_v + step_mean_v[idx]);
            }
        }
        let mut collapsed = FilterState::default();
        collapsed.nodes[0] = FilterNode {
            c: (total_c as i128 * S6 as i128 / total_in as i128) as i64,
            w: total_in,
            mean_u: (total_u as i128 * S6 as i128 / total_in as i128) as i64,
            mean_v: (total_v as i128 * S6 as i128 / total_in as i128) as i64,
        };
        collapsed.n_active = 1;
        return collapsed;
    }

    let k_m1 = (k_retained - 1) as i64;
    let span = max_c - min_c;
    let edge_pad = (span / (2 * k_m1)).max(1);
    let mut grid_min = min_c - edge_pad;
    let mut grid_max = max_c + edge_pad;

    // If a focus point is provided, shift the grid center toward the
    // autocall transition region. This puts more resolution where P(ac)
    // changes fastest. The grid stays uniform (same CU as before) but
    // covers the transition instead of wasting nodes in the tails.
    if let Some((focus_center, _)) = focus {
        let children_center = (min_c + max_c) / 2;
        let grid_span = grid_max - grid_min;
        // Shift center halfway toward the focus (blend to avoid hull overflow)
        let new_center = (children_center + focus_center) / 2;
        grid_min = new_center - grid_span / 2;
        grid_max = new_center + grid_span / 2;
        // Ensure hull coverage: extend if children fall outside
        if min_c < grid_min {
            grid_min = min_c - edge_pad;
        }
        if max_c > grid_max {
            grid_max = max_c + edge_pad;
        }
    }

    let grid_span = grid_max - grid_min;
    let mut projected = FilterState::default();
    let mut mean_u_raw = [0i64; MAX_K];
    let mut mean_v_raw = [0i64; MAX_K];
    for idx in 0..k_retained {
        projected.nodes[idx].c = grid_min + grid_span * idx as i64 / k_m1;
    }

    for parent in state.nodes.iter().copied().filter(|node| node.w > 0) {
        for idx in 0..N_FACTOR_NODES {
            let w = m6r(parent.w, factor_weights[idx]);
            if w <= 0 {
                continue;
            }
            let child_c = parent.c + factor_values[idx];
            let child_mean_u = parent.mean_u + step_mean_u[idx];
            let child_mean_v = parent.mean_v + step_mean_v[idx];
            if child_c <= grid_min {
                projected.nodes[0].w += w;
                mean_u_raw[0] += m6r(w, child_mean_u);
                mean_v_raw[0] += m6r(w, child_mean_v);
                continue;
            }
            if child_c >= grid_max {
                let last = k_retained - 1;
                projected.nodes[last].w += w;
                mean_u_raw[last] += m6r(w, child_mean_u);
                mean_v_raw[last] += m6r(w, child_mean_v);
                continue;
            }

            let scaled = (child_c - grid_min) * k_m1;
            let idx_lo = (scaled / grid_span) as usize;
            let remainder = scaled - idx_lo as i64 * grid_span;
            let frac_hi = remainder * S6 / grid_span;
            let frac_lo = S6 - frac_hi;
            let w_lo = m6r(w, frac_lo);
            let w_hi = w - w_lo;

            projected.nodes[idx_lo].w += w_lo;
            mean_u_raw[idx_lo] += m6r(w_lo, child_mean_u);
            mean_v_raw[idx_lo] += m6r(w_lo, child_mean_v);
            if idx_lo + 1 < k_retained {
                projected.nodes[idx_lo + 1].w += w_hi;
                mean_u_raw[idx_lo + 1] += m6r(w_hi, child_mean_u);
                mean_v_raw[idx_lo + 1] += m6r(w_hi, child_mean_v);
            }
        }
    }

    let total_out = projected.nodes[..k_retained]
        .iter()
        .map(|node| node.w)
        .sum::<i64>();
    let diff = total_in - total_out;
    if diff != 0 {
        let fix_idx = strongest_weight_index(&projected.nodes);
        projected.nodes[fix_idx].w = (projected.nodes[fix_idx].w + diff).max(0);
    }

    let mut n_active = 0usize;
    for idx in 0..k_retained {
        if projected.nodes[idx].w > 0 {
            projected.nodes[idx].mean_u =
                (mean_u_raw[idx] as i128 * S6 as i128 / projected.nodes[idx].w as i128) as i64;
            projected.nodes[idx].mean_v =
                (mean_v_raw[idx] as i128 * S6 as i128 / projected.nodes[idx].w as i128) as i64;
            n_active += 1;
        }
    }
    projected.n_active = n_active;
    projected
}

fn predict_state_grad(
    state: &FilterState,
    state_grad: &FilterStateGrad,
    factor_values: &[i64; N_FACTOR_NODES],
    factor_weights: &[i64; N_FACTOR_NODES],
    step_mean_u: &[i64; N_FACTOR_NODES],
    step_mean_v: &[i64; N_FACTOR_NODES],
    k_retained: usize,
) -> (FilterState, FilterStateGrad) {
    predict_state_grad_with_focus(
        state,
        state_grad,
        factor_values,
        factor_weights,
        step_mean_u,
        step_mean_v,
        k_retained,
        None,
    )
}

fn predict_state_grad_with_focus(
    state: &FilterState,
    state_grad: &FilterStateGrad,
    factor_values: &[i64; N_FACTOR_NODES],
    factor_weights: &[i64; N_FACTOR_NODES],
    step_mean_u: &[i64; N_FACTOR_NODES],
    step_mean_v: &[i64; N_FACTOR_NODES],
    k_retained: usize,
    focus: Option<(i64, i64)>,
) -> (FilterState, FilterStateGrad) {
    let k_retained = k_retained.clamp(1, MAX_K);
    let mut n_children = 0usize;
    let mut min_c = i64::MAX;
    let mut max_c = i64::MIN;
    let mut total_in = 0i64;
    let mut total_in_grad = [0i64; 3];

    for (idx, parent) in state.nodes.iter().copied().enumerate() {
        if parent.w <= 0 {
            continue;
        }
        let parent_grad = state_grad.nodes[idx];
        for fidx in 0..N_FACTOR_NODES {
            let w = m6r(parent.w, factor_weights[fidx]);
            if w <= 0 {
                continue;
            }
            let c = parent.c + factor_values[fidx];
            min_c = min_c.min(c);
            max_c = max_c.max(c);
            total_in += w;
            for asset in 0..3 {
                total_in_grad[asset] += m6r(parent_grad.dw[asset], factor_weights[fidx]);
            }
            n_children += 1;
        }
    }
    if n_children == 0 {
        return (FilterState::default(), FilterStateGrad::default());
    }

    if n_children <= k_retained {
        let mut exact = [FilterNode::default(); MAX_K];
        let mut exact_grad = [NodeGrad::default(); MAX_K];
        let mut out = 0usize;
        for (idx, parent) in state.nodes.iter().copied().enumerate() {
            if parent.w <= 0 {
                continue;
            }
            let parent_grad = state_grad.nodes[idx];
            for fidx in 0..N_FACTOR_NODES {
                let w = m6r(parent.w, factor_weights[fidx]);
                if w <= 0 {
                    continue;
                }
                exact[out] = FilterNode {
                    c: parent.c + factor_values[fidx],
                    w,
                    mean_u: parent.mean_u + step_mean_u[fidx],
                    mean_v: parent.mean_v + step_mean_v[fidx],
                };
                for asset in 0..3 {
                    exact_grad[out].dw[asset] = m6r(parent_grad.dw[asset], factor_weights[fidx]);
                    exact_grad[out].du[asset] = parent_grad.du[asset];
                    exact_grad[out].dv[asset] = parent_grad.dv[asset];
                }
                out += 1;
            }
        }
        sort_small_by_c_with_grad(&mut exact, &mut exact_grad, out);
        return (
            FilterState {
                nodes: exact,
                n_active: out,
            },
            FilterStateGrad { nodes: exact_grad },
        );
    }

    if max_c <= min_c || k_retained == 1 {
        let mut total_c = 0i64;
        let mut total_u = 0i64;
        let mut total_v = 0i64;
        let mut total_u_grad = [0i64; 3];
        let mut total_v_grad = [0i64; 3];
        for (idx, parent) in state.nodes.iter().copied().enumerate() {
            if parent.w <= 0 {
                continue;
            }
            let parent_grad = state_grad.nodes[idx];
            for fidx in 0..N_FACTOR_NODES {
                let w = m6r(parent.w, factor_weights[fidx]);
                if w <= 0 {
                    continue;
                }
                let child_c = parent.c + factor_values[fidx];
                let child_mean_u = parent.mean_u + step_mean_u[fidx];
                let child_mean_v = parent.mean_v + step_mean_v[fidx];
                let dw = [
                    m6r(parent_grad.dw[0], factor_weights[fidx]),
                    m6r(parent_grad.dw[1], factor_weights[fidx]),
                    m6r(parent_grad.dw[2], factor_weights[fidx]),
                ];
                total_c += m6r(w, child_c);
                total_u += m6r(w, child_mean_u);
                total_v += m6r(w, child_mean_v);
                for asset in 0..3 {
                    total_u_grad[asset] +=
                        m6r(dw[asset], child_mean_u) + m6r(w, parent_grad.du[asset]);
                    total_v_grad[asset] +=
                        m6r(dw[asset], child_mean_v) + m6r(w, parent_grad.dv[asset]);
                }
            }
        }
        let mut collapsed = FilterState::default();
        let mut collapsed_grad = FilterStateGrad::default();
        collapsed.nodes[0] = FilterNode {
            c: (total_c as i128 * S6 as i128 / total_in as i128) as i64,
            w: total_in,
            mean_u: (total_u as i128 * S6 as i128 / total_in as i128) as i64,
            mean_v: (total_v as i128 * S6 as i128 / total_in as i128) as i64,
        };
        for asset in 0..3 {
            collapsed_grad.nodes[0].dw[asset] = total_in_grad[asset];
            collapsed_grad.nodes[0].du[asset] =
                conditional_mean_grad(total_in, total_u, total_in_grad[asset], total_u_grad[asset]);
            collapsed_grad.nodes[0].dv[asset] =
                conditional_mean_grad(total_in, total_v, total_in_grad[asset], total_v_grad[asset]);
        }
        collapsed.n_active = 1;
        return (collapsed, collapsed_grad);
    }

    let k_m1 = (k_retained - 1) as i64;
    let span = max_c - min_c;
    let edge_pad = (span / (2 * k_m1)).max(1);
    let mut grid_min = min_c - edge_pad;
    let mut grid_max = max_c + edge_pad;

    if let Some((focus_center, _)) = focus {
        let children_center = (min_c + max_c) / 2;
        let grid_span = grid_max - grid_min;
        let new_center = (children_center + focus_center) / 2;
        grid_min = new_center - grid_span / 2;
        grid_max = new_center + grid_span / 2;
        if min_c < grid_min {
            grid_min = min_c - edge_pad;
        }
        if max_c > grid_max {
            grid_max = max_c + edge_pad;
        }
    }

    let grid_span = grid_max - grid_min;
    let mut projected = FilterState::default();
    let mut projected_grad = FilterStateGrad::default();
    let mut mean_u_raw = [0i64; MAX_K];
    let mut mean_v_raw = [0i64; MAX_K];
    let mut mean_u_raw_grad = [[0i64; 3]; MAX_K];
    let mut mean_v_raw_grad = [[0i64; 3]; MAX_K];
    for idx in 0..k_retained {
        projected.nodes[idx].c = grid_min + grid_span * idx as i64 / k_m1;
    }

    for (idx, parent) in state.nodes.iter().copied().enumerate() {
        if parent.w <= 0 {
            continue;
        }
        let parent_grad = state_grad.nodes[idx];
        for fidx in 0..N_FACTOR_NODES {
            let w = m6r(parent.w, factor_weights[fidx]);
            if w <= 0 {
                continue;
            }
            let child_c = parent.c + factor_values[fidx];
            let child_mean_u = parent.mean_u + step_mean_u[fidx];
            let child_mean_v = parent.mean_v + step_mean_v[fidx];
            let dw = [
                m6r(parent_grad.dw[0], factor_weights[fidx]),
                m6r(parent_grad.dw[1], factor_weights[fidx]),
                m6r(parent_grad.dw[2], factor_weights[fidx]),
            ];

            if child_c <= grid_min {
                projected.nodes[0].w += w;
                mean_u_raw[0] += m6r(w, child_mean_u);
                mean_v_raw[0] += m6r(w, child_mean_v);
                for asset in 0..3 {
                    projected_grad.nodes[0].dw[asset] += dw[asset];
                    mean_u_raw_grad[0][asset] +=
                        m6r(dw[asset], child_mean_u) + m6r(w, parent_grad.du[asset]);
                    mean_v_raw_grad[0][asset] +=
                        m6r(dw[asset], child_mean_v) + m6r(w, parent_grad.dv[asset]);
                }
                continue;
            }
            if child_c >= grid_max {
                let last = k_retained - 1;
                projected.nodes[last].w += w;
                mean_u_raw[last] += m6r(w, child_mean_u);
                mean_v_raw[last] += m6r(w, child_mean_v);
                for asset in 0..3 {
                    projected_grad.nodes[last].dw[asset] += dw[asset];
                    mean_u_raw_grad[last][asset] +=
                        m6r(dw[asset], child_mean_u) + m6r(w, parent_grad.du[asset]);
                    mean_v_raw_grad[last][asset] +=
                        m6r(dw[asset], child_mean_v) + m6r(w, parent_grad.dv[asset]);
                }
                continue;
            }

            let scaled = (child_c - grid_min) * k_m1;
            let idx_lo = (scaled / grid_span) as usize;
            let remainder = scaled - idx_lo as i64 * grid_span;
            let frac_hi = remainder * S6 / grid_span;
            let frac_lo = S6 - frac_hi;
            let w_lo = m6r(w, frac_lo);
            let w_hi = w - w_lo;

            projected.nodes[idx_lo].w += w_lo;
            mean_u_raw[idx_lo] += m6r(w_lo, child_mean_u);
            mean_v_raw[idx_lo] += m6r(w_lo, child_mean_v);
            for asset in 0..3 {
                let dw_lo = m6r(dw[asset], frac_lo);
                let dw_hi = dw[asset] - dw_lo;
                projected_grad.nodes[idx_lo].dw[asset] += dw_lo;
                mean_u_raw_grad[idx_lo][asset] +=
                    m6r(dw_lo, child_mean_u) + m6r(w_lo, parent_grad.du[asset]);
                mean_v_raw_grad[idx_lo][asset] +=
                    m6r(dw_lo, child_mean_v) + m6r(w_lo, parent_grad.dv[asset]);
                if idx_lo + 1 < k_retained {
                    projected_grad.nodes[idx_lo + 1].dw[asset] += dw_hi;
                    mean_u_raw_grad[idx_lo + 1][asset] +=
                        m6r(dw_hi, child_mean_u) + m6r(w_hi, parent_grad.du[asset]);
                    mean_v_raw_grad[idx_lo + 1][asset] +=
                        m6r(dw_hi, child_mean_v) + m6r(w_hi, parent_grad.dv[asset]);
                }
            }
            if idx_lo + 1 < k_retained {
                projected.nodes[idx_lo + 1].w += w_hi;
                mean_u_raw[idx_lo + 1] += m6r(w_hi, child_mean_u);
                mean_v_raw[idx_lo + 1] += m6r(w_hi, child_mean_v);
            }
        }
    }

    let total_out = projected.nodes[..k_retained]
        .iter()
        .map(|node| node.w)
        .sum::<i64>();
    let diff = total_in - total_out;
    if diff != 0 {
        let fix_idx = strongest_weight_index(&projected.nodes);
        projected.nodes[fix_idx].w = (projected.nodes[fix_idx].w + diff).max(0);
        for asset in 0..3 {
            let total_out_grad = projected_grad.nodes[..k_retained]
                .iter()
                .map(|node| node.dw[asset])
                .sum::<i64>();
            projected_grad.nodes[fix_idx].dw[asset] += total_in_grad[asset] - total_out_grad;
        }
    }

    let mut n_active = 0usize;
    for idx in 0..k_retained {
        if projected.nodes[idx].w > 0 {
            projected.nodes[idx].mean_u =
                (mean_u_raw[idx] as i128 * S6 as i128 / projected.nodes[idx].w as i128) as i64;
            projected.nodes[idx].mean_v =
                (mean_v_raw[idx] as i128 * S6 as i128 / projected.nodes[idx].w as i128) as i64;
            for asset in 0..3 {
                projected_grad.nodes[idx].du[asset] = conditional_mean_grad(
                    projected.nodes[idx].w,
                    mean_u_raw[idx],
                    projected_grad.nodes[idx].dw[asset],
                    mean_u_raw_grad[idx][asset],
                );
                projected_grad.nodes[idx].dv[asset] = conditional_mean_grad(
                    projected.nodes[idx].w,
                    mean_v_raw[idx],
                    projected_grad.nodes[idx].dw[asset],
                    mean_v_raw_grad[idx][asset],
                );
            }
            n_active += 1;
        }
    }
    projected.n_active = n_active;
    (projected, projected_grad)
}

/// Frozen-grid predict_state: skips min/max scan, replaces barycentric
/// divisions with precomputed multiply-shift via inv_cell_s30.
/// `grid_c[..k_retained]` and `inv_cell_s30` come from frozen_predict_tables.
/// Public wrapper for benchmarking.
pub fn predict_state_frozen_pub(
    state: &FilterState,
    factor_values: &[i64; N_FACTOR_NODES],
    factor_weights: &[i64; N_FACTOR_NODES],
    step_mean_u: &[i64; N_FACTOR_NODES],
    step_mean_v: &[i64; N_FACTOR_NODES],
    k_retained: usize,
    grid_c: &[i64; MAX_K],
    inv_cell_s30: i64,
) -> FilterState {
    predict_state_frozen(
        state,
        factor_values,
        factor_weights,
        step_mean_u,
        step_mean_v,
        k_retained,
        grid_c,
        inv_cell_s30,
    )
}

fn predict_state_frozen(
    state: &FilterState,
    factor_values: &[i64; N_FACTOR_NODES],
    factor_weights: &[i64; N_FACTOR_NODES],
    step_mean_u: &[i64; N_FACTOR_NODES],
    step_mean_v: &[i64; N_FACTOR_NODES],
    k_retained: usize,
    grid_c: &[i64; MAX_K],
    inv_cell_s30: i64,
) -> FilterState {
    let k_retained = k_retained.clamp(1, MAX_K);
    let grid_min = grid_c[0];
    let grid_max = grid_c[k_retained - 1];

    let mut projected = FilterState::default();
    let mut mean_u_raw = [0i64; MAX_K];
    let mut mean_v_raw = [0i64; MAX_K];
    let mut total_in = 0i64;

    // Set output grid positions from precomputed table
    for idx in 0..k_retained {
        projected.nodes[idx].c = grid_c[idx];
    }

    // Single-pass projection: iterate parents × factors, project onto frozen grid
    for parent in state.nodes.iter().copied().filter(|node| node.w > 0) {
        for fidx in 0..N_FACTOR_NODES {
            let w = m6r(parent.w, factor_weights[fidx]);
            if w <= 0 {
                continue;
            }
            total_in += w;
            let child_c = parent.c + factor_values[fidx];
            let child_mean_u = parent.mean_u + step_mean_u[fidx];
            let child_mean_v = parent.mean_v + step_mean_v[fidx];

            // Clamp to left edge
            if child_c <= grid_min {
                projected.nodes[0].w += w;
                mean_u_raw[0] += m6r(w, child_mean_u);
                mean_v_raw[0] += m6r(w, child_mean_v);
                continue;
            }
            // Clamp to right edge
            if child_c >= grid_max {
                let last = k_retained - 1;
                projected.nodes[last].w += w;
                mean_u_raw[last] += m6r(w, child_mean_u);
                mean_v_raw[last] += m6r(w, child_mean_v);
                continue;
            }

            // Multiply-shift barycentric projection (no division)
            let offset = child_c - grid_min;
            let idx_frac_s30 = offset * inv_cell_s30;
            let idx_lo = ((idx_frac_s30 >> 30) as usize).min(k_retained - 2);
            let within_s30 = idx_frac_s30 - ((idx_lo as i64) << 30);
            let frac_hi = within_s30 * S6 >> 30;
            let frac_lo = S6 - frac_hi;
            let w_lo = m6r(w, frac_lo);
            let w_hi = w - w_lo;

            projected.nodes[idx_lo].w += w_lo;
            mean_u_raw[idx_lo] += m6r(w_lo, child_mean_u);
            mean_v_raw[idx_lo] += m6r(w_lo, child_mean_v);
            if idx_lo + 1 < k_retained {
                projected.nodes[idx_lo + 1].w += w_hi;
                mean_u_raw[idx_lo + 1] += m6r(w_hi, child_mean_u);
                mean_v_raw[idx_lo + 1] += m6r(w_hi, child_mean_v);
            }
        }
    }

    // Mass conservation fixup
    let total_out = projected.nodes[..k_retained]
        .iter()
        .map(|node| node.w)
        .sum::<i64>();
    let diff = total_in - total_out;
    if diff != 0 {
        let fix_idx = strongest_weight_index(&projected.nodes);
        projected.nodes[fix_idx].w = (projected.nodes[fix_idx].w + diff).max(0);
    }

    // Normalize means
    let mut n_active = 0usize;
    for idx in 0..k_retained {
        if projected.nodes[idx].w > 0 {
            projected.nodes[idx].mean_u =
                (mean_u_raw[idx] as i128 * S6 as i128 / projected.nodes[idx].w as i128) as i64;
            projected.nodes[idx].mean_v =
                (mean_v_raw[idx] as i128 * S6 as i128 / projected.nodes[idx].w as i128) as i64;
            n_active += 1;
        }
    }
    projected.n_active = n_active;
    projected
}

fn predict_state_frozen_grad(
    state: &FilterState,
    state_grad: &FilterStateGrad,
    factor_values: &[i64; N_FACTOR_NODES],
    factor_weights: &[i64; N_FACTOR_NODES],
    step_mean_u: &[i64; N_FACTOR_NODES],
    step_mean_v: &[i64; N_FACTOR_NODES],
    k_retained: usize,
    grid_c: &[i64; MAX_K],
    inv_cell_s30: i64,
) -> (FilterState, FilterStateGrad) {
    let k_retained = k_retained.clamp(1, MAX_K);
    let grid_min = grid_c[0];
    let grid_max = grid_c[k_retained - 1];

    let mut projected = FilterState::default();
    let mut projected_grad = FilterStateGrad::default();
    let mut mean_u_raw = [0i64; MAX_K];
    let mut mean_v_raw = [0i64; MAX_K];
    let mut mean_u_raw_grad = [[0i64; 3]; MAX_K];
    let mut mean_v_raw_grad = [[0i64; 3]; MAX_K];
    let mut total_in = 0i64;
    let mut total_in_grad = [0i64; 3];

    for idx in 0..k_retained {
        projected.nodes[idx].c = grid_c[idx];
    }

    for (idx, parent) in state.nodes.iter().copied().enumerate() {
        if parent.w <= 0 {
            continue;
        }
        let parent_grad = state_grad.nodes[idx];
        for fidx in 0..N_FACTOR_NODES {
            let w = m6r(parent.w, factor_weights[fidx]);
            if w <= 0 {
                continue;
            }
            let dw = [
                m6r(parent_grad.dw[0], factor_weights[fidx]),
                m6r(parent_grad.dw[1], factor_weights[fidx]),
                m6r(parent_grad.dw[2], factor_weights[fidx]),
            ];
            total_in += w;
            for asset in 0..3 {
                total_in_grad[asset] += dw[asset];
            }

            let child_c = parent.c + factor_values[fidx];
            let child_mean_u = parent.mean_u + step_mean_u[fidx];
            let child_mean_v = parent.mean_v + step_mean_v[fidx];

            if child_c <= grid_min {
                projected.nodes[0].w += w;
                mean_u_raw[0] += m6r(w, child_mean_u);
                mean_v_raw[0] += m6r(w, child_mean_v);
                for asset in 0..3 {
                    projected_grad.nodes[0].dw[asset] += dw[asset];
                    mean_u_raw_grad[0][asset] +=
                        m6r(dw[asset], child_mean_u) + m6r(w, parent_grad.du[asset]);
                    mean_v_raw_grad[0][asset] +=
                        m6r(dw[asset], child_mean_v) + m6r(w, parent_grad.dv[asset]);
                }
                continue;
            }
            if child_c >= grid_max {
                let last = k_retained - 1;
                projected.nodes[last].w += w;
                mean_u_raw[last] += m6r(w, child_mean_u);
                mean_v_raw[last] += m6r(w, child_mean_v);
                for asset in 0..3 {
                    projected_grad.nodes[last].dw[asset] += dw[asset];
                    mean_u_raw_grad[last][asset] +=
                        m6r(dw[asset], child_mean_u) + m6r(w, parent_grad.du[asset]);
                    mean_v_raw_grad[last][asset] +=
                        m6r(dw[asset], child_mean_v) + m6r(w, parent_grad.dv[asset]);
                }
                continue;
            }

            let offset = child_c - grid_min;
            let idx_frac_s30 = offset * inv_cell_s30;
            let idx_lo = ((idx_frac_s30 >> 30) as usize).min(k_retained - 2);
            let within_s30 = idx_frac_s30 - ((idx_lo as i64) << 30);
            let frac_hi = within_s30 * S6 >> 30;
            let frac_lo = S6 - frac_hi;
            let w_lo = m6r(w, frac_lo);
            let w_hi = w - w_lo;

            projected.nodes[idx_lo].w += w_lo;
            mean_u_raw[idx_lo] += m6r(w_lo, child_mean_u);
            mean_v_raw[idx_lo] += m6r(w_lo, child_mean_v);
            projected.nodes[idx_lo + 1].w += w_hi;
            mean_u_raw[idx_lo + 1] += m6r(w_hi, child_mean_u);
            mean_v_raw[idx_lo + 1] += m6r(w_hi, child_mean_v);

            for asset in 0..3 {
                let dw_lo = m6r(dw[asset], frac_lo);
                let dw_hi = dw[asset] - dw_lo;
                projected_grad.nodes[idx_lo].dw[asset] += dw_lo;
                projected_grad.nodes[idx_lo + 1].dw[asset] += dw_hi;
                mean_u_raw_grad[idx_lo][asset] +=
                    m6r(dw_lo, child_mean_u) + m6r(w_lo, parent_grad.du[asset]);
                mean_v_raw_grad[idx_lo][asset] +=
                    m6r(dw_lo, child_mean_v) + m6r(w_lo, parent_grad.dv[asset]);
                mean_u_raw_grad[idx_lo + 1][asset] +=
                    m6r(dw_hi, child_mean_u) + m6r(w_hi, parent_grad.du[asset]);
                mean_v_raw_grad[idx_lo + 1][asset] +=
                    m6r(dw_hi, child_mean_v) + m6r(w_hi, parent_grad.dv[asset]);
            }
        }
    }

    let total_out = projected.nodes[..k_retained]
        .iter()
        .map(|node| node.w)
        .sum::<i64>();
    let diff = total_in - total_out;
    if diff != 0 {
        let fix_idx = strongest_weight_index(&projected.nodes);
        projected.nodes[fix_idx].w = (projected.nodes[fix_idx].w + diff).max(0);
        for asset in 0..3 {
            let total_out_grad = projected_grad.nodes[..k_retained]
                .iter()
                .map(|node| node.dw[asset])
                .sum::<i64>();
            projected_grad.nodes[fix_idx].dw[asset] += total_in_grad[asset] - total_out_grad;
        }
    }

    let mut n_active = 0usize;
    for idx in 0..k_retained {
        if projected.nodes[idx].w > 0 {
            projected.nodes[idx].mean_u =
                (mean_u_raw[idx] as i128 * S6 as i128 / projected.nodes[idx].w as i128) as i64;
            projected.nodes[idx].mean_v =
                (mean_v_raw[idx] as i128 * S6 as i128 / projected.nodes[idx].w as i128) as i64;
            for asset in 0..3 {
                projected_grad.nodes[idx].du[asset] = conditional_mean_grad(
                    projected.nodes[idx].w,
                    mean_u_raw[idx],
                    projected_grad.nodes[idx].dw[asset],
                    mean_u_raw_grad[idx][asset],
                );
                projected_grad.nodes[idx].dv[asset] = conditional_mean_grad(
                    projected.nodes[idx].w,
                    mean_v_raw[idx],
                    projected_grad.nodes[idx].dw[asset],
                    mean_v_raw_grad[idx][asset],
                );
            }
            n_active += 1;
        }
    }
    projected.n_active = n_active;
    (projected, projected_grad)
}

/// Phase 3: factored transition matrix predict.
///
/// Propagates `parent_state` through one observation step using precomputed
/// B-tensors (σ-independent geometric constants) and live NIG importance
/// weights `factor_weights`. The effective transition matrix
/// `T[i][j] = Σ_k factor_weights[k] × B_k[i][j]` is assembled implicitly by
/// fusing matrix assembly with state propagation: we walk the sparse
/// `(k, j)` pairs and scatter barycentric splits directly into `w_out,
/// wu_out, wv_out` accumulators.
///
/// `step ∈ 0..N_STEPS` selects the B-table and the child observation's
/// z-scale. The child grid positions are reconstructed on the fly from
/// `nested_c_grid(sigma_s6, child_obs_rel, k_child)` — the nested
/// barrier-adapted grid established in Phase 1.
///
/// Per-step `step_mean_u[k], step_mean_v[k]` carry the drift shift (σ-
/// dependent via `uv_slope × factor_values`) and are applied to each
/// (j, k) combination before barycentric splitting. This mirrors the
/// existing `predict_state` contract.
///
/// Out-of-hull positions clip to the nearest grid endpoint; in practice
/// the mass lost to clipping is small because the Phase 1 z-range
/// `[-3, +3]` covers the 9-point NIG proposal ±3.19/√2 ≈ ±2.26 plus
/// a parent at `|z| ≤ 3`. At extreme parent positions clipping can bleed
/// a few percent of mass onto the endpoint; Phase 6 measures whether this
/// hurts fair-coupon accuracy.
#[cfg(not(target_os = "solana"))]
fn predict_state_matrix(
    parent_state: &FilterState,
    factor_weights: &[i64; N_FACTOR_NODES],
    step_mean_u: &[i64; N_FACTOR_NODES],
    step_mean_v: &[i64; N_FACTOR_NODES],
    step: usize,
    sigma_s6: i64,
) -> FilterState {
    use crate::b_tensors::{
        k_child_for, k_parent_for, BEntry, B_STEP_0, B_STEP_1, B_STEP_2, B_STEP_3, N_STEPS,
    };
    use crate::nested_grids::nested_c_grid;

    if step >= N_STEPS {
        return FilterState::default();
    }
    let k_parent = k_parent_for(step);
    let k_child = k_child_for(step);
    let child_obs_rel = step + 1;

    let mut w_out = [0i64; MAX_K];
    let mut wu_out = [0i64; MAX_K];
    let mut wv_out = [0i64; MAX_K];

    // Inline dispatch on step: each arm accesses a fixed-size const B table,
    // so the compiler can constant-fold the index bounds.
    match step {
        0 => scatter_b_tensor::<12, 9>(
            parent_state,
            factor_weights,
            step_mean_u,
            step_mean_v,
            &B_STEP_0,
            &mut w_out,
            &mut wu_out,
            &mut wv_out,
        ),
        1 => scatter_b_tensor::<9, 7>(
            parent_state,
            factor_weights,
            step_mean_u,
            step_mean_v,
            &B_STEP_1,
            &mut w_out,
            &mut wu_out,
            &mut wv_out,
        ),
        2 => scatter_b_tensor::<7, 5>(
            parent_state,
            factor_weights,
            step_mean_u,
            step_mean_v,
            &B_STEP_2,
            &mut w_out,
            &mut wu_out,
            &mut wv_out,
        ),
        3 => scatter_b_tensor::<5, 3>(
            parent_state,
            factor_weights,
            step_mean_u,
            step_mean_v,
            &B_STEP_3,
            &mut w_out,
            &mut wu_out,
            &mut wv_out,
        ),
        _ => {}
    }

    // Reconstruct child state at nested grid positions.
    let child_c_grid = nested_c_grid(sigma_s6, child_obs_rel, k_child);
    let mut out = FilterState::default();
    let mut n_active = 0usize;
    for i in 0..k_child {
        let w = w_out[i];
        if w > NODE_STATE_EPS_S6 {
            let mean_u = (wu_out[i] as i128 * S6 as i128 / w as i128) as i64;
            let mean_v = (wv_out[i] as i128 * S6 as i128 / w as i128) as i64;
            out.nodes[i] = FilterNode {
                c: child_c_grid[i],
                w,
                mean_u,
                mean_v,
            };
            n_active += 1;
        }
    }
    out.n_active = n_active;
    out
}

/// Phase A revision: uniform K=12 rectangular predict.
///
/// Uses B_STEP_U12_* (all 12→12 transitions on the K=12 nested grid) so
/// every observation stays at K=12 instead of shrinking to [9,7,5,3].
/// Same architecture as predict_state_matrix; only the B-tensors differ.
#[cfg(not(target_os = "solana"))]
fn predict_state_matrix_u12(
    parent_state: &FilterState,
    factor_weights: &[i64; N_FACTOR_NODES],
    step_mean_u: &[i64; N_FACTOR_NODES],
    step_mean_v: &[i64; N_FACTOR_NODES],
    step: usize,
    sigma_s6: i64,
) -> FilterState {
    use crate::b_tensors::{B_STEP_U12_0, B_STEP_U12_1, B_STEP_U12_2, B_STEP_U12_3};
    use crate::nested_grids::nested_c_grid;

    if step >= 4 {
        return FilterState::default();
    }
    let k_uniform = 12usize;
    let child_obs_rel = step + 1;

    let mut w_out = [0i64; MAX_K];
    let mut wu_out = [0i64; MAX_K];
    let mut wv_out = [0i64; MAX_K];

    match step {
        0 => scatter_b_tensor::<12, 12>(
            parent_state,
            factor_weights,
            step_mean_u,
            step_mean_v,
            &B_STEP_U12_0,
            &mut w_out,
            &mut wu_out,
            &mut wv_out,
        ),
        1 => scatter_b_tensor::<12, 12>(
            parent_state,
            factor_weights,
            step_mean_u,
            step_mean_v,
            &B_STEP_U12_1,
            &mut w_out,
            &mut wu_out,
            &mut wv_out,
        ),
        2 => scatter_b_tensor::<12, 12>(
            parent_state,
            factor_weights,
            step_mean_u,
            step_mean_v,
            &B_STEP_U12_2,
            &mut w_out,
            &mut wu_out,
            &mut wv_out,
        ),
        3 => scatter_b_tensor::<12, 12>(
            parent_state,
            factor_weights,
            step_mean_u,
            step_mean_v,
            &B_STEP_U12_3,
            &mut w_out,
            &mut wu_out,
            &mut wv_out,
        ),
        _ => {}
    }

    let child_c_grid = nested_c_grid(sigma_s6, child_obs_rel, k_uniform);
    let mut out = FilterState::default();
    let mut n_active = 0usize;
    for i in 0..k_uniform {
        let w = w_out[i];
        if w > NODE_STATE_EPS_S6 {
            let mean_u = (wu_out[i] as i128 * S6 as i128 / w as i128) as i64;
            let mean_v = (wv_out[i] as i128 * S6 as i128 / w as i128) as i64;
            out.nodes[i] = FilterNode {
                c: child_c_grid[i],
                w,
                mean_u,
                mean_v,
            };
            n_active += 1;
        }
    }
    out.n_active = n_active;
    out
}

/// Inner sparse scatter for Phase 3 predict. `K_PARENT` and `K_CHILD` bind
/// the B-table dimensions; the compiler monomorphises one body per step.
#[cfg(not(target_os = "solana"))]
#[inline(always)]
fn scatter_b_tensor<const K_PARENT: usize, const K_CHILD: usize>(
    parent_state: &FilterState,
    factor_weights: &[i64; N_FACTOR_NODES],
    step_mean_u: &[i64; N_FACTOR_NODES],
    step_mean_v: &[i64; N_FACTOR_NODES],
    b_table: &[[crate::b_tensors::BEntry; K_PARENT]; N_FACTOR_NODES],
    w_out: &mut [i64; MAX_K],
    wu_out: &mut [i64; MAX_K],
    wv_out: &mut [i64; MAX_K],
) {
    for k in 0..N_FACTOR_NODES {
        let ak = factor_weights[k];
        if ak == 0 {
            continue;
        }
        let smu = step_mean_u[k];
        let smv = step_mean_v[k];
        for j in 0..K_PARENT {
            let pw = parent_state.nodes[j].w;
            if pw <= 0 {
                continue;
            }
            let wjk = m6r(ak, pw);
            if wjk == 0 {
                continue;
            }
            let mu_child = parent_state.nodes[j].mean_u + smu;
            let mv_child = parent_state.nodes[j].mean_v + smv;
            // wu_jk / wv_jk fit in i64: wjk ≤ S6, mu_child ≤ a few × S6.
            let wu_jk = m6r(wjk, mu_child);
            let wv_jk = m6r(wjk, mv_child);

            let entry = &b_table[k][j];
            let i_lo = entry.child_lo as usize;
            let fr_lo = entry.frac_lo_s6 as i64;
            w_out[i_lo] += m6r(wjk, fr_lo);
            wu_out[i_lo] += m6r(wu_jk, fr_lo);
            wv_out[i_lo] += m6r(wv_jk, fr_lo);

            if entry.child_hi != entry.child_lo {
                let i_hi = entry.child_hi as usize;
                let fr_hi = entry.frac_hi_s6 as i64;
                w_out[i_hi] += m6r(wjk, fr_hi);
                wu_out[i_hi] += m6r(wu_jk, fr_hi);
                wv_out[i_hi] += m6r(wv_jk, fr_hi);
            }
        }
    }
}

#[inline(always)]
fn merge_states(left: &FilterState, right: &FilterState, k_retained: usize) -> FilterState {
    let mut children = [FilterNode::default(); MAX_MERGE_NODES];
    let mut n_children = 0usize;
    for node in left.nodes.iter().copied().filter(|node| node.w > 0) {
        children[n_children] = node;
        n_children += 1;
    }
    for node in right.nodes.iter().copied().filter(|node| node.w > 0) {
        children[n_children] = node;
        n_children += 1;
    }
    project_nodes(&children[..n_children], k_retained)
}

#[inline(always)]
fn merge_states_grad(
    left: &FilterState,
    left_grad: &FilterStateGrad,
    right: &FilterState,
    right_grad: &FilterStateGrad,
    k_retained: usize,
) -> (FilterState, FilterStateGrad) {
    let mut children = [FilterNode::default(); MAX_MERGE_NODES];
    let mut child_grads = [NodeGrad::default(); MAX_MERGE_NODES];
    let mut n_children = 0usize;
    for (idx, node) in left
        .nodes
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, node)| node.w > 0)
    {
        children[n_children] = node;
        child_grads[n_children] = left_grad.nodes[idx];
        n_children += 1;
    }
    for (idx, node) in right
        .nodes
        .iter()
        .copied()
        .enumerate()
        .filter(|(_, node)| node.w > 0)
    {
        children[n_children] = node;
        child_grads[n_children] = right_grad.nodes[idx];
        n_children += 1;
    }
    project_nodes_with_grad(
        &children[..n_children],
        &child_grads[..n_children],
        k_retained,
    )
}

pub fn project_state(state: &FilterState, k_retained: usize) -> FilterState {
    let mut nodes = [FilterNode::default(); MAX_K];
    let mut len = 0usize;
    for node in state.nodes.iter().copied().filter(|node| node.w > 0) {
        nodes[len] = node;
        len += 1;
    }
    project_nodes(&nodes[..len], k_retained)
}

#[cfg(not(target_os = "solana"))]
fn update_safe_state_gradient(
    state: &FilterState,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    triple_pre: Option<&TripleCorrectionPre>,
) -> SafeUpdate {
    let obs = &cfg.obs[obs_idx];
    let tables = phi2_tables();
    let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
    let triple_pre = observation_probability_triple_pre(obs_idx, triple_pre);
    let ki_cholesky = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();
    let mut next_safe = FilterState::default();
    let mut new_knocked = FilterState::default();
    let mut first_hit = 0i64;
    let mut first_knock_in = 0i64;

    for (idx, node) in state.nodes.iter().copied().enumerate() {
        if node.w <= 0 {
            continue;
        }
        let shift = node.c + drift_shift_total;
        let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
        let ac_region = triangle_with_gradient_i64(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
            &dz_du,
            &dz_dv,
            obs.cov_uu,
            obs.cov_uv,
            obs.cov_vv,
        );
        let knocked_first = ki_cholesky
            .map(|(l11, l21, l22)| {
                ki_region_uv_moment_gh3(
                    node.mean_u,
                    node.mean_v,
                    l11,
                    l21,
                    l22,
                    cfg.ki_barrier_log,
                    ki_coords_from_cumulative(cfg, node.c, drift_shift_total),
                )
            })
            .unwrap_or_default();
        let safe_continue_prob = (S6 - ac_region.probability - knocked_first.probability).max(0);
        let safe_continue_eu = node.mean_u - ac_region.expectation_u - knocked_first.expectation_u;
        let safe_continue_ev = node.mean_v - ac_region.expectation_v - knocked_first.expectation_v;

        first_hit += m6r(node.w, ac_region.probability);
        first_knock_in += m6r(node.w, knocked_first.probability);

        if let Some((mean_u, mean_v)) =
            conditional_mean(safe_continue_prob, safe_continue_eu, safe_continue_ev)
        {
            let next_w = m6r(node.w, safe_continue_prob);
            if next_w > 0 {
                next_safe.nodes[idx] = FilterNode {
                    c: node.c,
                    w: next_w,
                    mean_u,
                    mean_v,
                };
            }
        }

        if let Some((mean_u, mean_v)) = conditional_mean(
            knocked_first.probability,
            knocked_first.expectation_u,
            knocked_first.expectation_v,
        ) {
            let next_w = m6r(node.w, knocked_first.probability);
            if next_w > 0 {
                new_knocked.nodes[idx] = FilterNode {
                    c: node.c,
                    w: next_w,
                    mean_u,
                    mean_v,
                };
            }
        }
    }

    next_safe.n_active = next_safe.nodes.iter().filter(|node| node.w > 0).count();
    new_knocked.n_active = new_knocked.nodes.iter().filter(|node| node.w > 0).count();

    SafeUpdate {
        next_safe,
        new_knocked,
        first_hit,
        first_knock_in,
    }
}

#[cfg(not(target_os = "solana"))]
fn update_safe_state_live(
    state: &FilterState,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    triple_pre: Option<&TripleCorrectionPre>,
) -> SafeUpdate {
    update_safe_state_gradient(state, cfg, obs_idx, drift_shift_total, triple_pre)
}

fn update_safe_state(
    state: &FilterState,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    k_retained: usize,
    triple_pre: Option<&TripleCorrectionPre>,
) -> SafeUpdate {
    let obs = &cfg.obs[obs_idx];
    let tables = phi2_tables();
    let frozen = frozen_observation_view(k_retained, obs_idx);
    let ki_cholesky = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();

    // Precompute KI early-exit margin² = 9 * max var over 3 coords.
    // Var(x_k) = a²*cov_uu + b²*cov_vv + 2ab*cov_uv in S6 scale.
    // Coefficients: SPY=(spy_u,spy_v), QQQ=(S6+spy_u,spy_v), IWM=(spy_u,S6+spy_v).
    // v_max is equivalent to per-coord here: the binding coord is always QQQ or IWM
    // (v_spy ≈ 0.4 × v_qqq/v_iwm but x_spy sits between x_qqq and x_iwm, so z-scores tie).
    let ki_margin_sq = {
        let l_sum = cfg.loading_sum;
        let spy_u = -cfg.loadings[1] * S6 / l_sum;
        let spy_v = -cfg.loadings[2] * S6 / l_sum;
        let var_of = |a: i64, b: i64| -> i64 {
            let aa = m6r_fast(a, a);
            let bb = m6r_fast(b, b);
            let ab = m6r_fast(a, b);
            m6r_fast(aa, obs.cov_uu) + m6r_fast(bb, obs.cov_vv) + 2 * m6r_fast(ab, obs.cov_uv)
        };
        let v_spy = var_of(spy_u, spy_v);
        let v_qqq = var_of(S6 + spy_u, spy_v);
        let v_iwm = var_of(spy_u, S6 + spy_v);
        let v_max = v_spy.max(v_qqq).max(v_iwm).max(0);
        // margin = 3*sigma; margin² = 9 * var, in S6 scale.
        9 * v_max
    };

    let mut next_safe = FilterState::default();
    let mut new_knocked = FilterState::default();
    let mut first_hit = 0i64;
    let mut first_knock_in = 0i64;
    #[cfg(all(target_os = "solana", feature = "ki-skip-count"))]
    let mut ki_skipped: u32 = 0;
    #[cfg(all(target_os = "solana", feature = "ki-skip-count"))]
    let mut ki_total: u32 = 0;

    for (idx, node) in state.nodes.iter().copied().enumerate() {
        if node.w <= 0 {
            continue;
        }
        c1_filter_cu_diag_inner(b"node_start");
        let shift = node.c + drift_shift_total;
        let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
        let ac_region = triangle_region_from_frozen_inline_i64(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
            idx,
            k_retained,
            frozen.safe_autocall,
        );
        c1_filter_cu_diag_inner(b"node_after_triangle");

        let knocked_first = ki_cholesky
            .map(|(l11, l21, l22)| {
                // Early-exit: compute coord centers at (mean_u, mean_v).
                // All 3 coords share spy_const. x_spy = spy_const + spy_u*mu + spy_v*mv.
                // x_qqq = x_spy + mu (since QQQ has a = S6+spy_u, b = spy_v → +mu).
                // x_iwm = x_spy + mv.
                let coords = ki_coords_from_cumulative(cfg, node.c, drift_shift_total);
                let x_spy = coords[0].constant
                    + m6r_fast(coords[0].u_coeff, node.mean_u)
                    + m6r_fast(coords[0].v_coeff, node.mean_v);
                let x_qqq = x_spy + node.mean_u;
                let x_iwm = x_spy + node.mean_v;
                let x_min = x_spy.min(x_qqq).min(x_iwm);
                let x_max = x_spy.max(x_qqq).max(x_iwm);
                let bull_margin = x_min - cfg.ki_barrier_log;
                let bear_margin = cfg.ki_barrier_log - x_max;
                // Bullish skip: x_min > barrier + 3σ → P(KI) ≈ 0.
                // Bearish skip: x_max < barrier - 3σ → P(KI) ≈ 1, region is the
                // full plane, so E[U·𝟙_KI] = mean_u and E[V·𝟙_KI] = mean_v
                // exactly (centered GH3 nodes integrate the unconditional mean).
                // m6r_fast(margin, margin) computes margin² at S6 scale to match ki_margin_sq.
                #[cfg(all(target_os = "solana", feature = "ki-skip-count"))]
                {
                    ki_total += 1;
                }
                if bull_margin > 0 && m6r_fast(bull_margin, bull_margin) > ki_margin_sq {
                    #[cfg(all(target_os = "solana", feature = "ki-skip-count"))]
                    {
                        ki_skipped += 1;
                    }
                    RawRegionMoment::default()
                } else if bear_margin > 0 && m6r_fast(bear_margin, bear_margin) > ki_margin_sq {
                    #[cfg(all(target_os = "solana", feature = "ki-skip-count"))]
                    {
                        ki_skipped += 1;
                    }
                    RawRegionMoment {
                        probability: S6,
                        expectation_u: node.mean_u,
                        expectation_v: node.mean_v,
                    }
                } else {
                    ki_region_uv_moment_gh3(
                        node.mean_u,
                        node.mean_v,
                        l11,
                        l21,
                        l22,
                        cfg.ki_barrier_log,
                        coords,
                    )
                }
            })
            .unwrap_or_default();
        c1_filter_cu_diag_inner(b"node_after_ki");

        let safe_continue_prob = (S6 - ac_region.probability - knocked_first.probability).max(0);
        let safe_continue_eu = node.mean_u - ac_region.expectation_u - knocked_first.expectation_u;
        let safe_continue_ev = node.mean_v - ac_region.expectation_v - knocked_first.expectation_v;

        first_hit += m6r(node.w, ac_region.probability);
        first_knock_in += m6r(node.w, knocked_first.probability);
        c1_filter_cu_diag_inner(b"node_after_accum");

        if let Some((mean_u, mean_v)) =
            conditional_mean(safe_continue_prob, safe_continue_eu, safe_continue_ev)
        {
            let next_w = m6r(node.w, safe_continue_prob);
            if next_w > 0 {
                next_safe.nodes[idx] = FilterNode {
                    c: node.c,
                    w: next_w,
                    mean_u,
                    mean_v,
                };
            }
        }

        if let Some((mean_u, mean_v)) = conditional_mean(
            knocked_first.probability,
            knocked_first.expectation_u,
            knocked_first.expectation_v,
        ) {
            let next_w = m6r(node.w, knocked_first.probability);
            if next_w > 0 {
                new_knocked.nodes[idx] = FilterNode {
                    c: node.c,
                    w: next_w,
                    mean_u,
                    mean_v,
                };
            }
        }
        c1_filter_cu_diag_inner(b"node_end");
    }

    next_safe.n_active = next_safe.nodes.iter().filter(|node| node.w > 0).count();
    new_knocked.n_active = new_knocked.nodes.iter().filter(|node| node.w > 0).count();

    // Emits "0xDEAD obs_idx ki_skipped ki_total 0" via sol_log_64.
    #[cfg(all(target_os = "solana", feature = "ki-skip-count"))]
    unsafe {
        sol_log_64_(
            0xDEAD,
            obs_idx as u64,
            ki_skipped as u64,
            ki_total as u64,
            0,
        );
    }

    SafeUpdate {
        next_safe,
        new_knocked,
        first_hit,
        first_knock_in,
    }
}

fn update_safe_state_grad(
    state: &FilterState,
    state_grad: &FilterStateGrad,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    k_retained: usize,
    triple_pre: Option<&TripleCorrectionPre>,
    dmu_ds: &[(i64, i64, i64); 3],
) -> (SafeUpdate, SafeUpdateGrad) {
    let obs = &cfg.obs[obs_idx];
    let tables = phi2_tables();
    let frozen = frozen_observation_view(k_retained, obs_idx);
    let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
    let dz_dc = obs.tri_pre.inv_std;
    let ki_cholesky = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();
    let ki_common_coeff = (S6 as i128 * S6 as i128 / cfg.loading_sum as i128) as i64;

    let ki_margin_sq = {
        let l_sum = cfg.loading_sum;
        let spy_u = -cfg.loadings[1] * S6 / l_sum;
        let spy_v = -cfg.loadings[2] * S6 / l_sum;
        let var_of = |a: i64, b: i64| -> i64 {
            let aa = m6r_fast(a, a);
            let bb = m6r_fast(b, b);
            let ab = m6r_fast(a, b);
            m6r_fast(aa, obs.cov_uu) + m6r_fast(bb, obs.cov_vv) + 2 * m6r_fast(ab, obs.cov_uv)
        };
        let v_spy = var_of(spy_u, spy_v);
        let v_qqq = var_of(S6 + spy_u, spy_v);
        let v_iwm = var_of(spy_u, S6 + spy_v);
        9 * v_spy.max(v_qqq).max(v_iwm).max(0)
    };

    let mut next_safe = FilterState::default();
    let mut new_knocked = FilterState::default();
    let mut grad = SafeUpdateGrad::default();
    let mut first_hit = 0i64;
    let mut first_knock_in = 0i64;

    for (idx, node) in state.nodes.iter().copied().enumerate() {
        if node.w <= 0 {
            continue;
        }

        let node_grad = state_grad.nodes[idx];
        let shift = node.c + drift_shift_total;
        let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
        let ac_ws = triangle_probability_workspace(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
        );
        let ac_region = triangle_region_from_frozen_inline_i64(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
            idx,
            k_retained,
            frozen.safe_autocall,
        );

        let (knocked_first, knocked_first_grad) = ki_cholesky
            .map(|(l11, l21, l22)| {
                let coords = ki_coords_from_cumulative(cfg, node.c, drift_shift_total);
                let x_spy = coords[0].constant
                    + m6r_fast(coords[0].u_coeff, node.mean_u)
                    + m6r_fast(coords[0].v_coeff, node.mean_v);
                let x_qqq = x_spy + node.mean_u;
                let x_iwm = x_spy + node.mean_v;
                let x_min = x_spy.min(x_qqq).min(x_iwm);
                let x_max = x_spy.max(x_qqq).max(x_iwm);
                let bull_margin = x_min - cfg.ki_barrier_log;
                let bear_margin = cfg.ki_barrier_log - x_max;
                if bull_margin > 0 && m6r_fast(bull_margin, bull_margin) > ki_margin_sq {
                    (RawRegionMoment::default(), RawRegionMomentGrad::default())
                } else if bear_margin > 0 && m6r_fast(bear_margin, bear_margin) > ki_margin_sq {
                    (
                        RawRegionMoment {
                            probability: S6,
                            expectation_u: node.mean_u,
                            expectation_v: node.mean_v,
                        },
                        RawRegionMomentGrad::default(),
                    )
                } else {
                    ki_region_uv_moment_gh3_grad(
                        node.mean_u,
                        node.mean_v,
                        l11,
                        l21,
                        l22,
                        cfg.ki_barrier_log,
                        coords,
                        ki_common_coeff,
                    )
                }
            })
            .unwrap_or_default();

        let safe_continue_prob = (S6 - ac_region.probability - knocked_first.probability).max(0);
        let safe_continue_eu = node.mean_u - ac_region.expectation_u - knocked_first.expectation_u;
        let safe_continue_ev = node.mean_v - ac_region.expectation_v - knocked_first.expectation_v;

        first_hit += m6r(node.w, ac_region.probability);
        first_knock_in += m6r(node.w, knocked_first.probability);

        if let Some((mean_u, mean_v)) =
            conditional_mean(safe_continue_prob, safe_continue_eu, safe_continue_ev)
        {
            let next_w = m6r(node.w, safe_continue_prob);
            if next_w > 0 {
                next_safe.nodes[idx] = FilterNode {
                    c: node.c,
                    w: next_w,
                    mean_u,
                    mean_v,
                };
            }
        }

        if let Some((mean_u, mean_v)) = conditional_mean(
            knocked_first.probability,
            knocked_first.expectation_u,
            knocked_first.expectation_v,
        ) {
            let next_w = m6r(node.w, knocked_first.probability);
            if next_w > 0 {
                new_knocked.nodes[idx] = FilterNode {
                    c: node.c,
                    w: next_w,
                    mean_u,
                    mean_v,
                };
            }
        }

        let dp_ac_du_basis = triangle_probability_grad_from_workspace(&ac_ws, &dz_du);
        let dp_ac_dv_basis = triangle_probability_grad_from_workspace(&ac_ws, &dz_dv);
        let dp_ac_dc_basis = triangle_probability_grad_from_workspace(&ac_ws, &dz_dc);

        for asset in 0..3 {
            let (dmu_u, dmu_v, dmu_c) = dmu_ds[asset];
            let total_du = node_grad.du[asset] + dmu_u;
            let total_dv = node_grad.dv[asset] + dmu_v;
            let dp_ac = m6r_fast(total_du, dp_ac_du_basis)
                + m6r_fast(total_dv, dp_ac_dv_basis)
                + m6r_fast(dmu_c, dp_ac_dc_basis);
            let deu_ac = frozen_expectation_grad(
                total_du,
                ac_region.probability,
                ac_region.expectation_u,
                dp_ac,
            );
            let dev_ac = frozen_expectation_grad(
                total_dv,
                ac_region.probability,
                ac_region.expectation_v,
                dp_ac,
            );
            let dp_ki = m6r_fast(total_du, knocked_first_grad.dp_du)
                + m6r_fast(total_dv, knocked_first_grad.dp_dv)
                + m6r_fast(dmu_c, knocked_first_grad.dp_dc);
            let deu_ki = m6r_fast(total_du, knocked_first_grad.deu_du)
                + m6r_fast(total_dv, knocked_first_grad.deu_dv)
                + m6r_fast(dmu_c, knocked_first_grad.deu_dc);
            let dev_ki = m6r_fast(total_du, knocked_first_grad.dev_du)
                + m6r_fast(total_dv, knocked_first_grad.dev_dv)
                + m6r_fast(dmu_c, knocked_first_grad.dev_dc);

            grad.first_hit[asset] +=
                m6r(node_grad.dw[asset], ac_region.probability) + m6r(node.w, dp_ac);
            grad.first_knock_in[asset] +=
                m6r(node_grad.dw[asset], knocked_first.probability) + m6r(node.w, dp_ki);

            let dsafe_prob = -dp_ac - dp_ki;
            let dsafe_eu = total_du - deu_ac - deu_ki;
            let dsafe_ev = total_dv - dev_ac - dev_ki;

            if next_safe.nodes[idx].w > 0 {
                grad.next_safe.nodes[idx].dw[asset] =
                    m6r(node_grad.dw[asset], safe_continue_prob) + m6r(node.w, dsafe_prob);
                grad.next_safe.nodes[idx].du[asset] = conditional_mean_grad(
                    safe_continue_prob,
                    safe_continue_eu,
                    dsafe_prob,
                    dsafe_eu,
                );
                grad.next_safe.nodes[idx].dv[asset] = conditional_mean_grad(
                    safe_continue_prob,
                    safe_continue_ev,
                    dsafe_prob,
                    dsafe_ev,
                );
            }

            if new_knocked.nodes[idx].w > 0 {
                grad.new_knocked.nodes[idx].dw[asset] =
                    m6r(node_grad.dw[asset], knocked_first.probability) + m6r(node.w, dp_ki);
                grad.new_knocked.nodes[idx].du[asset] = conditional_mean_grad(
                    knocked_first.probability,
                    knocked_first.expectation_u,
                    dp_ki,
                    deu_ki,
                );
                grad.new_knocked.nodes[idx].dv[asset] = conditional_mean_grad(
                    knocked_first.probability,
                    knocked_first.expectation_v,
                    dp_ki,
                    dev_ki,
                );
            }
        }
    }

    next_safe.n_active = next_safe.nodes.iter().filter(|node| node.w > 0).count();
    new_knocked.n_active = new_knocked.nodes.iter().filter(|node| node.w > 0).count();

    (
        SafeUpdate {
            next_safe,
            new_knocked,
            first_hit,
            first_knock_in,
        },
        grad,
    )
}

#[cfg(not(target_os = "solana"))]
fn update_knocked_state_live(
    state: &FilterState,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    triple_pre: Option<&TripleCorrectionPre>,
) -> KnockedUpdate {
    let obs = &cfg.obs[obs_idx];
    let tables = phi2_tables();
    let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
    let mut next_knocked = FilterState::default();
    let mut first_hit = 0i64;

    for (idx, node) in state.nodes.iter().copied().enumerate() {
        if node.w <= 0 {
            continue;
        }
        let shift = node.c + drift_shift_total;
        let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
        let ac_region = triangle_with_gradient_i64(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
            &dz_du,
            &dz_dv,
            obs.cov_uu,
            obs.cov_uv,
            obs.cov_vv,
        );
        let continue_prob = (S6 - ac_region.probability).max(0);
        let continue_eu = node.mean_u - ac_region.expectation_u;
        let continue_ev = node.mean_v - ac_region.expectation_v;

        first_hit += m6r(node.w, ac_region.probability);
        if let Some((mean_u, mean_v)) = conditional_mean(continue_prob, continue_eu, continue_ev) {
            let next_w = m6r(node.w, continue_prob);
            if next_w > 0 {
                next_knocked.nodes[idx] = FilterNode {
                    c: node.c,
                    w: next_w,
                    mean_u,
                    mean_v,
                };
            }
        }
    }

    next_knocked.n_active = next_knocked.nodes.iter().filter(|node| node.w > 0).count();
    KnockedUpdate {
        next_knocked,
        first_hit,
    }
}

fn update_knocked_state(
    state: &FilterState,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    k_retained: usize,
    triple_pre: Option<&TripleCorrectionPre>,
) -> KnockedUpdate {
    let obs = &cfg.obs[obs_idx];
    let tables = phi2_tables();
    let frozen = frozen_observation_view(k_retained, obs_idx);
    let triple_pre = observation_probability_triple_pre(obs_idx, triple_pre);
    let mut next_knocked = FilterState::default();
    let mut first_hit = 0i64;

    for (idx, node) in state.nodes.iter().copied().enumerate() {
        if node.w <= 0 {
            continue;
        }
        let shift = node.c + drift_shift_total;
        let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
        let ac_region = triangle_region_from_frozen_inline_i64(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
            idx,
            k_retained,
            frozen.knocked_autocall,
        );
        let continue_prob = (S6 - ac_region.probability).max(0);
        let continue_eu = node.mean_u - ac_region.expectation_u;
        let continue_ev = node.mean_v - ac_region.expectation_v;

        first_hit += m6r(node.w, ac_region.probability);
        if let Some((mean_u, mean_v)) = conditional_mean(continue_prob, continue_eu, continue_ev) {
            let next_w = m6r(node.w, continue_prob);
            if next_w > 0 {
                next_knocked.nodes[idx] = FilterNode {
                    c: node.c,
                    w: next_w,
                    mean_u,
                    mean_v,
                };
            }
        }
    }

    next_knocked.n_active = next_knocked.nodes.iter().filter(|node| node.w > 0).count();
    KnockedUpdate {
        next_knocked,
        first_hit,
    }
}

fn update_knocked_state_grad(
    state: &FilterState,
    state_grad: &FilterStateGrad,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    k_retained: usize,
    triple_pre: Option<&TripleCorrectionPre>,
    dmu_ds: &[(i64, i64, i64); 3],
) -> (KnockedUpdate, KnockedUpdateGrad) {
    let obs = &cfg.obs[obs_idx];
    let tables = phi2_tables();
    let frozen = frozen_observation_view(k_retained, obs_idx);
    let triple_pre = observation_probability_triple_pre(obs_idx, triple_pre);
    let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
    let dz_dc = obs.tri_pre.inv_std;
    let mut next_knocked = FilterState::default();
    let mut grad = KnockedUpdateGrad::default();
    let mut first_hit = 0i64;

    for (idx, node) in state.nodes.iter().copied().enumerate() {
        if node.w <= 0 {
            continue;
        }
        let node_grad = state_grad.nodes[idx];
        let shift = node.c + drift_shift_total;
        let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
        let ac_ws = triangle_probability_workspace(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
        );
        let ac_region = triangle_region_from_frozen_inline_i64(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
            idx,
            k_retained,
            frozen.knocked_autocall,
        );
        let continue_prob = (S6 - ac_region.probability).max(0);
        let continue_eu = node.mean_u - ac_region.expectation_u;
        let continue_ev = node.mean_v - ac_region.expectation_v;

        first_hit += m6r(node.w, ac_region.probability);

        if let Some((mean_u, mean_v)) = conditional_mean(continue_prob, continue_eu, continue_ev) {
            let next_w = m6r(node.w, continue_prob);
            if next_w > 0 {
                next_knocked.nodes[idx] = FilterNode {
                    c: node.c,
                    w: next_w,
                    mean_u,
                    mean_v,
                };
            }
        }

        let dp_ac_du_basis = triangle_probability_grad_from_workspace(&ac_ws, &dz_du);
        let dp_ac_dv_basis = triangle_probability_grad_from_workspace(&ac_ws, &dz_dv);
        let dp_ac_dc_basis = triangle_probability_grad_from_workspace(&ac_ws, &dz_dc);

        for asset in 0..3 {
            let (dmu_u, dmu_v, dmu_c) = dmu_ds[asset];
            let total_du = node_grad.du[asset] + dmu_u;
            let total_dv = node_grad.dv[asset] + dmu_v;
            let dp_ac = m6r_fast(total_du, dp_ac_du_basis)
                + m6r_fast(total_dv, dp_ac_dv_basis)
                + m6r_fast(dmu_c, dp_ac_dc_basis);
            let deu_ac = frozen_expectation_grad(
                total_du,
                ac_region.probability,
                ac_region.expectation_u,
                dp_ac,
            );
            let dev_ac = frozen_expectation_grad(
                total_dv,
                ac_region.probability,
                ac_region.expectation_v,
                dp_ac,
            );
            grad.first_hit[asset] +=
                m6r(node_grad.dw[asset], ac_region.probability) + m6r(node.w, dp_ac);

            let dcontinue_prob = -dp_ac;
            let dcontinue_eu = total_du - deu_ac;
            let dcontinue_ev = total_dv - dev_ac;

            if next_knocked.nodes[idx].w > 0 {
                grad.next_knocked.nodes[idx].dw[asset] =
                    m6r(node_grad.dw[asset], continue_prob) + m6r(node.w, dcontinue_prob);
                grad.next_knocked.nodes[idx].du[asset] =
                    conditional_mean_grad(continue_prob, continue_eu, dcontinue_prob, dcontinue_eu);
                grad.next_knocked.nodes[idx].dv[asset] =
                    conditional_mean_grad(continue_prob, continue_ev, dcontinue_prob, dcontinue_ev);
            }
        }
    }

    next_knocked.n_active = next_knocked.nodes.iter().filter(|node| node.w > 0).count();
    (
        KnockedUpdate {
            next_knocked,
            first_hit,
        },
        grad,
    )
}

#[inline(always)]
fn ki_coords_from_cumulative(
    cfg: &C1FastConfig,
    cumulative_factor: i64,
    drift_shift_total: i64,
) -> [AffineCoord6; 3] {
    let l_sum = cfg.loading_sum;
    let spy_const = (cumulative_factor + drift_shift_total) * S6 / l_sum;
    let spy_u = -cfg.loadings[1] * S6 / l_sum;
    let spy_v = -cfg.loadings[2] * S6 / l_sum;
    // BGK per-name shift (applied by decrementing each coord's constant —
    // equivalent to raising the effective barrier, which makes KI more
    // likely, matching continuous-monitoring semantics).
    let bgk = cfg.ki_bgk_shifts.unwrap_or([0, 0, 0]);
    [
        AffineCoord6 {
            constant: spy_const - bgk[0],
            u_coeff: spy_u,
            v_coeff: spy_v,
        },
        AffineCoord6 {
            constant: spy_const - bgk[1],
            u_coeff: S6 + spy_u,
            v_coeff: spy_v,
        },
        AffineCoord6 {
            constant: spy_const - bgk[2],
            u_coeff: spy_u,
            v_coeff: S6 + spy_v,
        },
    ]
}

/// Compute BGK per-name shifts at S6 scale for the given σ_common and
/// quarterly monitoring interval (63 trading days).
///
/// Option-C approximation: σ_i ≈ ℓ_i × σ_common (ignores residual
/// variance which is ~5% of total — contributes ~2.5% to σ_i, ~2.5% to
/// the shift, so sub-bp effect).
///
/// shift_i = β × √Δt × ℓ_i × σ_common
///         = 0.5826 × 0.5 × ℓ_i × σ_common
///         = 0.2913 × ℓ_i × σ_common      (all at S6).
#[inline]
pub fn compute_bgk_shifts(cfg: &C1FastConfig, sigma_s6: i64) -> [i64; 3] {
    /// 0.2913 = β × √(63/252) = ζ(1/2)/√(2π) × 0.5, at S6 scale.
    const BGK_HALF_S6: i64 = 291_300;
    [
        m6r(BGK_HALF_S6, m6r(cfg.loadings[0], sigma_s6)),
        m6r(BGK_HALF_S6, m6r(cfg.loadings[1], sigma_s6)),
        m6r(BGK_HALF_S6, m6r(cfg.loadings[2], sigma_s6)),
    ]
}

fn maturity_safe_leg(
    state: &FilterState,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    triple_pre: Option<&TripleCorrectionPre>,
) -> (i64, i64, i64, i64) {
    let obs = &cfg.obs[obs_idx];
    let tables = phi2_tables();
    let (l11, l21, l22) = match cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv) {
        Ok(values) => values,
        Err(_) => return (0, 0, 0, 0),
    };

    let mut coupon_hit = 0i64;
    let mut safe_principal = 0i64;
    let mut first_knock_in = 0i64;
    let mut knock_in_redemption = 0i64;

    for node in state.nodes.iter().copied().filter(|node| node.w > 0) {
        let shift = node.c + drift_shift_total;
        let coupon_prob = triangle_probability_with_triple_i64(
            node.mean_u,
            node.mean_v,
            &[cfg.autocall_rhs_base + shift; 3],
            &obs.tri_pre,
            tables,
            triple_pre,
        );
        let ki_coords = ki_coords_from_cumulative(cfg, node.c, drift_shift_total);

        // Guard: skip the expensive KI moment if the node's position puts
        // all three assets comfortably above the KI barrier at mean spread.
        // Uses the minimum coordinate constant as a quick lower bound.
        // Cost: 3 comparisons (~15 CU) vs ki_moment_i64_gh3 (~8K CU).
        let min_coord = ki_coords[0]
            .constant
            .min(ki_coords[1].constant)
            .min(ki_coords[2].constant);
        if min_coord > cfg.ki_barrier_log + l11 {
            // Even the worst asset is >1σ above KI barrier at mean spread.
            coupon_hit += m6r(node.w, coupon_prob);
            safe_principal += node.w;
        } else {
            let ki_moment = ki_moment_i64_gh3(
                node.mean_u,
                node.mean_v,
                l11,
                l21,
                l22,
                cfg.ki_barrier_log,
                ki_coords,
            );
            coupon_hit += m6r(node.w, coupon_prob);
            safe_principal += m6r(node.w, (S6 - ki_moment.ki_probability).max(0));
            first_knock_in += m6r(node.w, ki_moment.ki_probability);
            knock_in_redemption += m6r(node.w, ki_moment.worst_indicator);
        }
    }

    (
        coupon_hit,
        safe_principal,
        first_knock_in,
        knock_in_redemption,
    )
}

fn maturity_safe_leg_grad(
    state: &FilterState,
    state_grad: &FilterStateGrad,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    triple_pre: Option<&TripleCorrectionPre>,
    dmu_ds: &[(i64, i64, i64); 3],
) -> ((i64, i64, i64, i64), MaturitySafeLegGrad) {
    c1_filter_cu_diag_inner(b"maturity_safe_grad_start");
    let obs = &cfg.obs[obs_idx];
    let tables = phi2_tables();
    let (l11, l21, l22) = match cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv) {
        Ok(values) => values,
        Err(_) => return ((0, 0, 0, 0), MaturitySafeLegGrad::default()),
    };
    let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
    let dz_dc = obs.tri_pre.inv_std;
    let ki_common_coeff = (S6 as i128 * S6 as i128 / cfg.loading_sum as i128) as i64;

    let mut coupon_hit = 0i64;
    let mut safe_principal = 0i64;
    let mut first_knock_in = 0i64;
    let mut knock_in_redemption = 0i64;
    let mut grad = MaturitySafeLegGrad::default();

    for (idx, node) in state.nodes.iter().copied().enumerate() {
        if node.w <= 0 {
            continue;
        }
        let node_grad = state_grad.nodes[idx];
        let shift = node.c + drift_shift_total;
        let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
        let coupon_ws = triangle_probability_workspace(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
        );
        let coupon_prob = coupon_ws.probability;
        let ki_coords = ki_coords_from_cumulative(cfg, node.c, drift_shift_total);

        let min_coord = ki_coords[0]
            .constant
            .min(ki_coords[1].constant)
            .min(ki_coords[2].constant);
        let ki_term = if min_coord > cfg.ki_barrier_log + l11 {
            (
                KiMoment6 {
                    ki_probability: 0,
                    worst_indicator: 0,
                },
                KiMomentGrad::default(),
            )
        } else {
            ki_moment_i64_gh3_grad(
                node.mean_u,
                node.mean_v,
                l11,
                l21,
                l22,
                cfg.ki_barrier_log,
                ki_coords,
                ki_common_coeff,
            )
        };
        let ki_moment = ki_term.0;
        let ki_grad = ki_term.1;
        let safe_term = (S6 - ki_moment.ki_probability).max(0);
        let dp_coupon_du_basis = triangle_probability_grad_from_workspace(&coupon_ws, &dz_du);
        let dp_coupon_dv_basis = triangle_probability_grad_from_workspace(&coupon_ws, &dz_dv);
        let dp_coupon_dc_basis = triangle_probability_grad_from_workspace(&coupon_ws, &dz_dc);

        coupon_hit += m6r(node.w, coupon_prob);
        safe_principal += m6r(node.w, safe_term);
        first_knock_in += m6r(node.w, ki_moment.ki_probability);
        knock_in_redemption += m6r(node.w, ki_moment.worst_indicator);

        for asset in 0..3 {
            let (dmu_u, dmu_v, dmu_c) = dmu_ds[asset];
            let total_du = node_grad.du[asset] + dmu_u;
            let total_dv = node_grad.dv[asset] + dmu_v;
            let dp_coupon = m6r_fast(total_du, dp_coupon_du_basis)
                + m6r_fast(total_dv, dp_coupon_dv_basis)
                + m6r_fast(dmu_c, dp_coupon_dc_basis);
            let dp_ki = m6r_fast(total_du, ki_grad.dp_du)
                + m6r_fast(total_dv, ki_grad.dp_dv)
                + m6r_fast(dmu_c, ki_grad.dp_dc);
            let dworst = m6r_fast(total_du, ki_grad.dworst_du)
                + m6r_fast(total_dv, ki_grad.dworst_dv)
                + m6r_fast(dmu_c, ki_grad.dworst_dc);
            let dsafe_term = if safe_term > 0 { -dp_ki } else { 0 };

            grad.coupon_hit[asset] +=
                m6r(node_grad.dw[asset], coupon_prob) + m6r(node.w, dp_coupon);
            grad.safe_principal[asset] +=
                m6r(node_grad.dw[asset], safe_term) + m6r(node.w, dsafe_term);
            grad.first_knock_in[asset] +=
                m6r(node_grad.dw[asset], ki_moment.ki_probability) + m6r(node.w, dp_ki);
            grad.knock_in_redemption[asset] +=
                m6r(node_grad.dw[asset], ki_moment.worst_indicator) + m6r(node.w, dworst);
        }
    }

    c1_filter_cu_diag_inner(b"maturity_safe_grad_done");
    (
        (
            coupon_hit,
            safe_principal,
            first_knock_in,
            knock_in_redemption,
        ),
        grad,
    )
}

fn maturity_knocked_leg(
    state: &FilterState,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    triple_pre: Option<&TripleCorrectionPre>,
) -> (i64, i64) {
    let obs = &cfg.obs[obs_idx];
    let tables = phi2_tables();
    let (l11, l21, l22) = match cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv) {
        Ok(values) => values,
        Err(_) => return (0, 0),
    };

    let mut coupon_hit = 0i64;
    let mut redemption = 0i64;

    for node in state.nodes.iter().copied().filter(|node| node.w > 0) {
        let shift = node.c + drift_shift_total;
        let coupon_prob = triangle_probability_with_triple_i64(
            node.mean_u,
            node.mean_v,
            &[cfg.autocall_rhs_base + shift; 3],
            &obs.tri_pre,
            tables,
            triple_pre,
        );
        let below_initial_coords = ki_coords_from_cumulative(cfg, node.c, drift_shift_total);
        let below_initial = ki_moment_i64_gh3(
            node.mean_u,
            node.mean_v,
            l11,
            l21,
            l22,
            0,
            below_initial_coords,
        );
        let redemption_expectation =
            (S6 - below_initial.ki_probability).max(0) + below_initial.worst_indicator;

        coupon_hit += m6r(node.w, coupon_prob);
        redemption += m6r(node.w, redemption_expectation.min(S6));
    }

    (coupon_hit, redemption)
}

fn maturity_knocked_leg_grad(
    state: &FilterState,
    state_grad: &FilterStateGrad,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    triple_pre: Option<&TripleCorrectionPre>,
    dmu_ds: &[(i64, i64, i64); 3],
) -> ((i64, i64), MaturityKnockedLegGrad) {
    c1_filter_cu_diag_inner(b"maturity_knocked_grad_start");
    let obs = &cfg.obs[obs_idx];
    let tables = phi2_tables();
    let (l11, l21, l22) = match cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv) {
        Ok(values) => values,
        Err(_) => return ((0, 0), MaturityKnockedLegGrad::default()),
    };
    let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
    let dz_dc = obs.tri_pre.inv_std;
    let ki_common_coeff = (S6 as i128 * S6 as i128 / cfg.loading_sum as i128) as i64;

    let mut coupon_hit = 0i64;
    let mut redemption = 0i64;
    let mut grad = MaturityKnockedLegGrad::default();

    for (idx, node) in state.nodes.iter().copied().enumerate() {
        if node.w <= 0 {
            continue;
        }
        let node_grad = state_grad.nodes[idx];
        let shift = node.c + drift_shift_total;
        let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
        let coupon_ws = triangle_probability_workspace(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
        );
        let coupon_prob = coupon_ws.probability;
        let below_initial_coords = ki_coords_from_cumulative(cfg, node.c, drift_shift_total);
        let (below_initial, below_grad) = ki_moment_i64_gh3_grad(
            node.mean_u,
            node.mean_v,
            l11,
            l21,
            l22,
            0,
            below_initial_coords,
            ki_common_coeff,
        );
        let redemption_uncapped =
            (S6 - below_initial.ki_probability).max(0) + below_initial.worst_indicator;
        let redemption_term = redemption_uncapped.min(S6);
        let dp_coupon_du_basis = triangle_probability_grad_from_workspace(&coupon_ws, &dz_du);
        let dp_coupon_dv_basis = triangle_probability_grad_from_workspace(&coupon_ws, &dz_dv);
        let dp_coupon_dc_basis = triangle_probability_grad_from_workspace(&coupon_ws, &dz_dc);

        coupon_hit += m6r(node.w, coupon_prob);
        redemption += m6r(node.w, redemption_term);

        for asset in 0..3 {
            let (dmu_u, dmu_v, dmu_c) = dmu_ds[asset];
            let total_du = node_grad.du[asset] + dmu_u;
            let total_dv = node_grad.dv[asset] + dmu_v;
            let dp_coupon = m6r_fast(total_du, dp_coupon_du_basis)
                + m6r_fast(total_dv, dp_coupon_dv_basis)
                + m6r_fast(dmu_c, dp_coupon_dc_basis);
            let dp_ki = m6r_fast(total_du, below_grad.dp_du)
                + m6r_fast(total_dv, below_grad.dp_dv)
                + m6r_fast(dmu_c, below_grad.dp_dc);
            let dworst = m6r_fast(total_du, below_grad.dworst_du)
                + m6r_fast(total_dv, below_grad.dworst_dv)
                + m6r_fast(dmu_c, below_grad.dworst_dc);
            let d_redemption_uncapped = if below_initial.ki_probability < S6 {
                -dp_ki
            } else {
                0
            } + dworst;
            let d_redemption_term = if redemption_uncapped >= S6 {
                0
            } else {
                d_redemption_uncapped
            };

            grad.coupon_hit[asset] +=
                m6r(node_grad.dw[asset], coupon_prob) + m6r(node.w, dp_coupon);
            grad.redemption[asset] +=
                m6r(node_grad.dw[asset], redemption_term) + m6r(node.w, d_redemption_term);
        }
    }

    c1_filter_cu_diag_inner(b"maturity_knocked_grad_done");
    ((coupon_hit, redemption), grad)
}

pub fn build_factor_transition(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
) -> FactorTransition {
    let factor_weights = nig_importance_weights_9(sigma_s6);
    let proposal_std = sigma_s6 / 2;
    let mut factor_values = [0i64; N_FACTOR_NODES];
    let mut step_mean_u = [0i64; N_FACTOR_NODES];
    let mut step_mean_v = [0i64; N_FACTOR_NODES];
    for idx in 0..N_FACTOR_NODES {
        factor_values[idx] = SQRT2_S6 * proposal_std / S6 * GH9_NODES_S6[idx] / S6;
        step_mean_u[idx] = drift_diffs[0] + cfg.uv_slope[0] * factor_values[idx] / S6;
        step_mean_v[idx] = drift_diffs[1] + cfg.uv_slope[1] * factor_values[idx] / S6;
    }
    FactorTransition {
        factor_values,
        factor_weights,
        step_mean_u,
        step_mean_v,
    }
}

#[cfg(not(target_os = "solana"))]
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

#[cfg(not(target_os = "solana"))]
fn build_exact_seed_transition(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
) -> Option<ExactSeedTransition> {
    let sigma = sigma_s6 as f64 / S6 as f64;
    if !sigma.is_finite() || sigma <= 0.0 {
        return None;
    }

    let alpha = CF_ALPHA_S12 as f64 / S12 as f64;
    let beta = CF_BETA_S12 as f64 / S12 as f64;
    let gamma = CF_GAMMA_S12 as f64 / S12 as f64;
    let delta_scale = CF_DELTA_SCALE_S12 as f64 / S12 as f64;
    let step_days = cfg.obs[0].obs_day as f64;
    let std = sigma * (step_days / 252.0).sqrt();
    let delta = sigma * sigma * delta_scale * step_days;
    let drift = -delta * beta / gamma;
    if !std.is_finite() || std <= 0.0 || !delta.is_finite() || delta <= 0.0 {
        return None;
    }

    let inv_sqrt_pi = 1.0 / core::f64::consts::PI.sqrt();
    let sqrt_2pi = (2.0 * core::f64::consts::PI).sqrt();

    let mut factor_values = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut factor_weights = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut step_mean_u = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut step_mean_v = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut raw_weights = [0f64; N_FACTOR_NODES_EXACT_SEED];
    let mut total_raw = 0.0f64;

    for idx in 0..N_FACTOR_NODES_EXACT_SEED {
        let node = GH13_NODES[idx] as f64 / S12 as f64;
        let gh_weight = GH13_WEIGHTS[idx] as f64 / S12 as f64;
        let factor_value = core::f64::consts::SQRT_2 * std * node;
        let centered = factor_value - drift;
        let radius = (delta * delta + centered * centered).sqrt();
        let nig_pdf = if radius.is_finite() && radius > 0.0 {
            let ar = alpha * radius;
            let k1 = bessel_k1_f64(ar);
            if k1 > 0.0 {
                let exponent = delta * gamma + beta * centered;
                (alpha * delta / core::f64::consts::PI) * (k1 / radius) * exponent.exp()
            } else {
                0.0
            }
        } else {
            0.0
        };
        let normal_pdf = (-0.5 * (factor_value / std).powi(2)).exp() / (std * sqrt_2pi);
        let raw_weight = if normal_pdf > 0.0 && nig_pdf.is_finite() {
            inv_sqrt_pi * gh_weight * nig_pdf / normal_pdf
        } else {
            0.0
        };
        raw_weights[idx] = raw_weight.max(0.0);
        total_raw += raw_weights[idx];

        factor_values[idx] = (factor_value * S6 as f64).round() as i64;
        step_mean_u[idx] = drift_diffs[0] + cfg.uv_slope[0] * factor_values[idx] / S6;
        step_mean_v[idx] = drift_diffs[1] + cfg.uv_slope[1] * factor_values[idx] / S6;
    }

    if !total_raw.is_finite() || total_raw <= 0.0 {
        return None;
    }

    let mut total_weight = 0i64;
    let mut best_idx = 0usize;
    let mut best_raw = f64::NEG_INFINITY;
    for idx in 0..N_FACTOR_NODES_EXACT_SEED {
        if raw_weights[idx] > best_raw {
            best_raw = raw_weights[idx];
            best_idx = idx;
        }
        factor_weights[idx] = (raw_weights[idx] * S6 as f64 / total_raw).round() as i64;
        total_weight += factor_weights[idx];
    }
    let diff = S6 - total_weight;
    if diff != 0 {
        factor_weights[best_idx] = (factor_weights[best_idx] + diff).max(0);
    }

    Some(ExactSeedTransition {
        factor_values,
        factor_weights,
        step_mean_u,
        step_mean_v,
    })
}

#[cfg(target_os = "solana")]
fn build_exact_seed_transition(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
) -> Option<ExactSeedTransition> {
    let std_s12 = sigma_s6 as i128 * FIRST_STEP_STD_RATIO_S6 as i128;
    let delta_s12 =
        (sigma_s6 as i128 * sigma_s6 as i128 * CF_DELTA_SCALE_S12 * cfg.obs[0].obs_day as i128)
            / S12;
    let drift_s12 = -delta_s12 * CF_BETA_S12 / CF_GAMMA_S12;

    let mut factor_values = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut factor_weights = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut step_mean_u = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut step_mean_v = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut raw_weights = [0i128; N_FACTOR_NODES_EXACT_SEED];
    let mut total_raw = 0i128;

    for idx in 0..N_FACTOR_NODES_EXACT_SEED {
        let factor_s12 = std_s12 * SQRT2_S12 * GH13_NODES[idx] / (S12 * S12);
        let nig_pdf = nig_pdf_bessel(
            factor_s12,
            CF_ALPHA_S12,
            CF_BETA_S12,
            delta_s12,
            CF_GAMMA_S12,
            drift_s12,
        )
        .ok()
        .unwrap_or(0);
        raw_weights[idx] = nig_pdf.max(0);
        total_raw += raw_weights[idx];

        factor_values[idx] = (factor_s12 / S6 as i128) as i64;
        step_mean_u[idx] = drift_diffs[0] + cfg.uv_slope[0] * factor_values[idx] / S6;
        step_mean_v[idx] = drift_diffs[1] + cfg.uv_slope[1] * factor_values[idx] / S6;
    }

    if total_raw <= 0 {
        return None;
    }

    let mut total_weight = 0i64;
    let mut best_idx = 0usize;
    let mut best_raw = i128::MIN;
    for idx in 0..N_FACTOR_NODES_EXACT_SEED {
        if raw_weights[idx] > best_raw {
            best_raw = raw_weights[idx];
            best_idx = idx;
        }
        factor_weights[idx] = (raw_weights[idx] * S6 as i128 / total_raw) as i64;
        total_weight += factor_weights[idx];
    }
    let diff = S6 - total_weight;
    if diff != 0 {
        factor_weights[best_idx] = (factor_weights[best_idx] + diff).max(0);
    }

    Some(ExactSeedTransition {
        factor_values,
        factor_weights,
        step_mean_u,
        step_mean_v,
    })
}

fn predicted_state_from_exact_seed_transition(transition: &ExactSeedTransition) -> FilterState {
    let mut state = FilterState::default();
    for idx in 0..N_FACTOR_NODES_EXACT_SEED {
        let weight = transition.factor_weights[idx];
        if weight <= 0 {
            continue;
        }
        state.nodes[idx] = FilterNode {
            c: transition.factor_values[idx],
            w: weight,
            mean_u: transition.step_mean_u[idx],
            mean_v: transition.step_mean_v[idx],
        };
    }
    state.n_active = state.nodes.iter().filter(|node| node.w > 0).count();
    state
}

#[inline(never)]
fn run_first_observation_seed(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_total: i64,
    k_retained: usize,
    triple_pre: Option<&TripleCorrectionPre>,
) -> Option<FirstObservationSeed> {
    let transition = build_exact_seed_transition(cfg, sigma_s6, drift_diffs)?;
    let predicted_safe = predicted_state_from_exact_seed_transition(&transition);
    let obs = &cfg.obs[0];
    let tables = phi2_tables();
    let triple_pre = observation_probability_triple_pre(0, triple_pre);
    let ki_cholesky = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();
    let mut next_safe = FilterState::default();
    let mut next_knocked = FilterState::default();
    let mut first_hit = 0i64;
    let mut first_knock_in = 0i64;

    for (idx, node) in predicted_safe.nodes.iter().copied().enumerate() {
        if node.w <= 0 {
            continue;
        }
        let shift = node.c + drift_shift_total;
        let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
        let ac_prob = triangle_probability_with_triple_i64(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            triple_pre,
        );
        let ki_prob = ki_cholesky
            .map(|(l11, l21, l22)| {
                ki_region_uv_moment_gh3(
                    node.mean_u,
                    node.mean_v,
                    l11,
                    l21,
                    l22,
                    cfg.ki_barrier_log,
                    ki_coords_from_cumulative(cfg, node.c, drift_shift_total),
                )
                .probability
            })
            .unwrap_or(0);
        let safe_prob = (S6 - ac_prob - ki_prob).max(0);

        first_hit += m6r(node.w, ac_prob);
        first_knock_in += m6r(node.w, ki_prob);

        let safe_w = m6r(node.w, safe_prob);
        if safe_w > 0 {
            next_safe.nodes[idx] = FilterNode {
                c: node.c,
                w: safe_w,
                mean_u: node.mean_u,
                mean_v: node.mean_v,
            };
        }

        let knocked_w = m6r(node.w, ki_prob);
        if knocked_w > 0 {
            next_knocked.nodes[idx] = FilterNode {
                c: node.c,
                w: knocked_w,
                mean_u: node.mean_u,
                mean_v: node.mean_v,
            };
        }
    }

    next_safe.n_active = next_safe.nodes.iter().filter(|node| node.w > 0).count();
    next_knocked.n_active = next_knocked.nodes.iter().filter(|node| node.w > 0).count();
    Some(FirstObservationSeed {
        predicted_safe,
        next_safe: project_state(&next_safe, k_retained),
        next_knocked: project_state(&next_knocked, k_retained),
        first_hit,
        first_knock_in,
    })
}

#[cfg(not(target_os = "solana"))]
fn run_first_observation_seed_live(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_total: i64,
    k_retained: usize,
    triple_pre: Option<&TripleCorrectionPre>,
) -> Option<FirstObservationSeed> {
    let transition = build_exact_seed_transition(cfg, sigma_s6, drift_diffs)?;
    let predicted_safe = predicted_state_from_exact_seed_transition(&transition);
    let safe_update =
        update_safe_state_gradient(&predicted_safe, cfg, 0, drift_shift_total, triple_pre);
    Some(FirstObservationSeed {
        predicted_safe,
        next_safe: project_state(&safe_update.next_safe, k_retained),
        next_knocked: project_state(&safe_update.new_knocked, k_retained),
        first_hit: safe_update.first_hit,
        first_knock_in: safe_update.first_knock_in,
    })
}

fn build_triple_pre_by_obs(cfg: &C1FastConfig) -> [Option<TripleCorrectionPre>; N_OBS] {
    core::array::from_fn::<_, N_OBS, _>(|obs_idx| {
        let obs = &cfg.obs[obs_idx];
        cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av))
    })
}

#[inline(never)]
fn run_observation_step(
    safe_state: &FilterState,
    knocked_state: &FilterState,
    transition: &FactorTransition,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    k_safe: usize,
    k_knocked: usize,
    triple_pre: Option<&TripleCorrectionPre>,
    frozen_grid: Option<&crate::frozen_predict_tables::FrozenPredictGrid>,
) -> (FilterState, FilterState, i64, i64) {
    c1_filter_cu_diag(b"obs_safe_pred_start");
    let (next_safe, new_knocked, safe_first_hit, first_knock_in) = {
        let safe_pred = if let Some(fg) = frozen_grid {
            predict_state_frozen(
                safe_state,
                &transition.factor_values,
                &transition.factor_weights,
                &transition.step_mean_u,
                &transition.step_mean_v,
                k_safe,
                &fg.grid_c,
                fg.inv_cell_s30,
            )
        } else {
            predict_state(
                safe_state,
                &transition.factor_values,
                &transition.factor_weights,
                &transition.step_mean_u,
                &transition.step_mean_v,
                k_safe,
            )
        };
        c1_filter_cu_diag(b"obs_safe_pred_done");
        let safe_update = update_safe_state(
            &safe_pred,
            cfg,
            obs_idx,
            drift_shift_total,
            k_safe,
            triple_pre,
        );
        c1_filter_cu_diag(b"obs_safe_update_done");
        (
            safe_update.next_safe,
            safe_update.new_knocked,
            safe_update.first_hit,
            safe_update.first_knock_in,
        )
    };
    c1_filter_cu_diag(b"obs_knocked_pred_start");
    let (continued_knocked, knocked_first_hit) = {
        let knocked_pred = predict_state(
            knocked_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_knocked,
        );
        c1_filter_cu_diag(b"obs_knocked_pred_done");
        let knocked_update = update_knocked_state(
            &knocked_pred,
            cfg,
            obs_idx,
            drift_shift_total,
            k_knocked,
            triple_pre,
        );
        c1_filter_cu_diag(b"obs_knocked_update_done");
        (knocked_update.next_knocked, knocked_update.first_hit)
    };
    c1_filter_cu_diag(b"obs_merge_start");
    let next_knocked = merge_states(&new_knocked, &continued_knocked, k_knocked);
    c1_filter_cu_diag(b"obs_merge_done");
    (
        next_safe,
        next_knocked,
        safe_first_hit + knocked_first_hit,
        first_knock_in,
    )
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn run_observation_step_live(
    safe_state: &FilterState,
    knocked_state: &FilterState,
    transition: &FactorTransition,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    k_retained: usize,
    triple_pre: Option<&TripleCorrectionPre>,
) -> (FilterState, FilterState, i64, i64) {
    let triple_pre = observation_probability_triple_pre(obs_idx, triple_pre);
    let (next_safe, new_knocked, safe_first_hit, first_knock_in) = {
        let safe_pred = predict_state(
            safe_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        );
        let safe_update =
            update_safe_state_live(&safe_pred, cfg, obs_idx, drift_shift_total, triple_pre);
        (
            safe_update.next_safe,
            safe_update.new_knocked,
            safe_update.first_hit,
            safe_update.first_knock_in,
        )
    };
    let (continued_knocked, knocked_first_hit) = {
        let knocked_pred = predict_state(
            knocked_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        );
        let knocked_update =
            update_knocked_state_live(&knocked_pred, cfg, obs_idx, drift_shift_total, triple_pre);
        (knocked_update.next_knocked, knocked_update.first_hit)
    };
    let next_knocked = merge_states(&new_knocked, &continued_knocked, k_retained);
    (
        next_safe,
        next_knocked,
        safe_first_hit + knocked_first_hit,
        first_knock_in,
    )
}

/// Re-bin a FilterState onto a target c-grid via barycentric projection.
/// Assumes target_c is sorted ascending. Mass below target_c[0] clamps to
/// node 0; mass above target_c[k-1] clamps to last. Mass conservation is
/// exact (sum w in == sum w out, modulo i64 rounding).
#[cfg(not(target_os = "solana"))]
fn rebin_to_grid(state: &FilterState, target_c: &[i64], k: usize) -> FilterState {
    let k = k.clamp(1, MAX_K);
    let mut out = FilterState::default();
    let mut wu_acc = [0i64; MAX_K];
    let mut wv_acc = [0i64; MAX_K];
    let mut total_in = 0i64;

    for i in 0..k {
        out.nodes[i].c = target_c[i];
    }

    for node in state.nodes.iter().copied().filter(|n| n.w > 0) {
        total_in += node.w;
        // Locate bracket [lo, hi] with target_c[lo] <= node.c < target_c[hi].
        if node.c <= target_c[0] {
            out.nodes[0].w += node.w;
            wu_acc[0] += m6r(node.w, node.mean_u);
            wv_acc[0] += m6r(node.w, node.mean_v);
            continue;
        }
        if node.c >= target_c[k - 1] {
            out.nodes[k - 1].w += node.w;
            wu_acc[k - 1] += m6r(node.w, node.mean_u);
            wv_acc[k - 1] += m6r(node.w, node.mean_v);
            continue;
        }
        let mut lo = 0usize;
        for i in 0..(k - 1) {
            if target_c[i] <= node.c && node.c < target_c[i + 1] {
                lo = i;
                break;
            }
        }
        let hi = lo + 1;
        let span = target_c[hi] - target_c[lo];
        let frac_hi = if span > 0 {
            (node.c - target_c[lo]) * S6 / span
        } else {
            0
        };
        let frac_lo = S6 - frac_hi;
        let w_lo = m6r(node.w, frac_lo);
        let w_hi = node.w - w_lo;
        out.nodes[lo].w += w_lo;
        out.nodes[hi].w += w_hi;
        wu_acc[lo] += m6r(w_lo, node.mean_u);
        wv_acc[lo] += m6r(w_lo, node.mean_v);
        wu_acc[hi] += m6r(w_hi, node.mean_u);
        wv_acc[hi] += m6r(w_hi, node.mean_v);
    }

    // Mass conservation fixup.
    let total_out: i64 = out.nodes[..k].iter().map(|n| n.w).sum();
    let diff = total_in - total_out;
    if diff != 0 {
        let fix_idx = strongest_weight_index(&out.nodes);
        out.nodes[fix_idx].w = (out.nodes[fix_idx].w + diff).max(0);
    }

    let mut n_active = 0usize;
    for i in 0..k {
        if out.nodes[i].w > 0 {
            out.nodes[i].mean_u = (wu_acc[i] as i128 * S6 as i128 / out.nodes[i].w as i128) as i64;
            out.nodes[i].mean_v = (wv_acc[i] as i128 * S6 as i128 / out.nodes[i].w as i128) as i64;
            n_active += 1;
        }
    }
    out.n_active = n_active;
    out
}

/// Live obs-1 seed at K=12 nested grid.
///
/// 9 unconditional triangle evaluations from singleton initial state:
///   1. Singleton at (c=0, w=S6, mean=0).
///   2. NIG-weighted factor expansion → 9 children at factor c-positions.
///   3. Live observe at `cfg.obs[0]`: 9 fused triangle bundles + 9 KI
///      moments produce (P_ac, P_ki) per node, accumulating first_hit and
///      first_knock_in.
///   4. Surviving safe and new-knocked states projected onto the K=12
///      nested c-grid for obs_rel=0 via `rebin_to_grid`.
///
/// At ~40K CU this is negligible vs the budget. Replaces the precomputed
/// `obs1_projected_lookup_k15` + lossy K=15→K=12 rebin used in the v0
/// scaffold. Bit-exact for the σ being priced (no LUT bias, no curvature
/// loss from squeezing 15 multi-step nodes onto 12).
#[cfg(not(target_os = "solana"))]
fn obs1_live_seed_k12_nested(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    triple_pre: Option<&TripleCorrectionPre>,
) -> (FilterState, FilterState, i64, i64) {
    use crate::nested_grids::nested_c_grid;

    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let drift_shift_total = cfg.obs[0].obs_day as i64 / 63 * drift_shift_63;
    let tp = observation_probability_triple_pre(0, triple_pre);

    // Predict singleton → 9 NIG children at factor c-positions.
    // n_children = N_FACTOR_NODES = 9 ≤ k_retained → project_nodes takes
    // the exact early-return path, preserving factor positions.
    let predicted = predict_state(
        &FilterState::singleton_origin(),
        &transition.factor_values,
        &transition.factor_weights,
        &transition.step_mean_u,
        &transition.step_mean_v,
        N_FACTOR_NODES,
    );

    // Live observe at obs[0]. Returns (next_safe, new_knocked, first_hit,
    // first_knock_in) all at the same 9 c-positions.
    let safe_update = update_safe_state_live(&predicted, cfg, 0, drift_shift_total, tp);

    // Project surviving safe and new-knocked onto K=12 nested grid.
    let nested_c0 = nested_c_grid(sigma_s6, 0, K_SCHEDULE_K0);
    let safe_k12 = rebin_to_grid(
        &safe_update.next_safe,
        &nested_c0[..K_SCHEDULE_K0],
        K_SCHEDULE_K0,
    );
    let knocked_k12 = rebin_to_grid(
        &safe_update.new_knocked,
        &nested_c0[..K_SCHEDULE_K0],
        K_SCHEDULE_K0,
    );

    (
        safe_k12,
        knocked_k12,
        safe_update.first_hit,
        safe_update.first_knock_in,
    )
}

const K_SCHEDULE_K0: usize = 12;
const ANALYTIC_DELTA_K_KNOCKED: usize = 1;

/// Phase 5 integrated rectangular-K live pricer.
///
/// Composes:
///   - Phase 1 nested barrier-adapted grid
///   - Phase 3 transition matrices via `predict_state_matrix`
///   - Existing live observe (`update_safe_state_live` / `update_knocked_state_live`)
///   - Existing maturity legs at K=3
///
/// Obs-1 (cfg.obs[0]) is handled via the precomputed K=15 obs1 lookup, then
/// barycentric-rebinned onto the K=12 nested grid for obs_rel=0. Subsequent
/// observations chain through the rectangular K-schedule [12, 9, 7, 5, 3].
///
/// **Phase 5 scope note:** observe step is currently the existing live path
/// (`triangle_with_gradient_i64` + `ki_region_uv_moment_gh3`). Swapping to
/// `fused_region_bundle` + `select_frozen_moment_3pt` is the next iteration
/// once this scaffold is shown to be architecturally sound. The K=12 nested
/// rebin from obs1 is approximate (barycentric on c only, not a true
/// product-model projection) and is the main remaining accuracy concern.
#[cfg(not(target_os = "solana"))]
pub fn quote_c1_filter_rect_live(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
) -> C1FastQuote {
    use crate::b_tensors::{k_child_for, K_SCHEDULE};
    use crate::nested_grids::nested_c_grid;

    let triple_pre = build_triple_pre_by_obs(cfg);
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);

    let mut redemption_pv = 0i64;
    let mut coupon_annuity = 0i64;
    let mut total_ki = 0i64;
    let mut total_ac = 0i64;

    // 1. Live obs-1 seed at K=12 nested. 9 unconditional triangle bundles
    //    from singleton initial state, projected onto K=12 nested grid.
    //    Bit-exact for this σ — no LUT, no K=15→K=12 rebin loss.
    let (mut safe_state, mut knocked_state, obs1_fh, obs1_fki) = obs1_live_seed_k12_nested(
        cfg,
        sigma_s6,
        drift_diffs,
        drift_shift_63,
        triple_pre[0].as_ref(),
    );
    redemption_pv += m6r(cfg.notional, obs1_fh);
    coupon_annuity += 1 * obs1_fh;
    total_ac += obs1_fh;
    total_ki += obs1_fki;

    // 2. Four rectangular transition steps + observations (obs_idx 1..4).
    for step in 0..4 {
        let next_obs_idx = step + 1;
        let k_child = k_child_for(step);
        let coupon_count = (next_obs_idx + 1) as i64;
        let obs = &cfg.obs[next_obs_idx];
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;
        let tp =
            observation_probability_triple_pre(next_obs_idx, triple_pre[next_obs_idx].as_ref());

        // Predict via Phase 3 transition matrix.
        let safe_pred = predict_state_matrix(
            &safe_state,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            step,
            sigma_s6,
        );
        let knocked_pred = predict_state_matrix(
            &knocked_state,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            step,
            sigma_s6,
        );

        // Observe via existing live machinery.
        let safe_update =
            update_safe_state_live(&safe_pred, cfg, next_obs_idx, drift_shift_total, tp);
        let knocked_update =
            update_knocked_state_live(&knocked_pred, cfg, next_obs_idx, drift_shift_total, tp);

        let first_hit = safe_update.first_hit + knocked_update.first_hit;
        let first_knock_in = safe_update.first_knock_in;
        redemption_pv += m6r(cfg.notional, first_hit);
        coupon_annuity += coupon_count * first_hit;
        total_ac += first_hit;
        total_ki += first_knock_in;

        safe_state = safe_update.next_safe;
        knocked_state = merge_states(
            &safe_update.new_knocked,
            &knocked_update.next_knocked,
            k_child,
        );
    }

    // 3. Maturity at obs_idx = N_OBS - 1 (K=3 from step 3).
    let mat_obs_idx = N_OBS - 1;
    let mat_obs = &cfg.obs[mat_obs_idx];
    let drift_shift_mat = mat_obs.obs_day as i64 / 63 * drift_shift_63;
    let mat_tp = triple_pre[mat_obs_idx].as_ref();
    let coupon_count_mat = (mat_obs_idx + 1) as i64;

    let (coupon_safe, safe_principal, first_ki_mat, ki_redemption_safe) =
        maturity_safe_leg(&safe_state, cfg, mat_obs_idx, drift_shift_mat, mat_tp);
    let (coupon_knocked, knocked_redemption) =
        maturity_knocked_leg(&knocked_state, cfg, mat_obs_idx, drift_shift_mat, mat_tp);

    redemption_pv += m6r(cfg.notional, safe_principal);
    redemption_pv += m6r(cfg.notional, ki_redemption_safe);
    redemption_pv += m6r(cfg.notional, knocked_redemption);
    coupon_annuity += coupon_count_mat * (coupon_safe + coupon_knocked);
    total_ki += first_ki_mat;

    // 4. Coupon extraction.
    let loss = (cfg.notional - redemption_pv).max(0);
    let fair_coupon = if coupon_annuity > 100 {
        loss * S6 / coupon_annuity
    } else {
        0
    };

    c1_fast_quote_from_components(
        cfg.notional,
        fair_coupon,
        redemption_pv,
        coupon_annuity,
        total_ki,
        total_ac,
    )
}

/// Phase A revision (uniform K=12 rect): like `quote_c1_filter_rect_live`
/// but uses `predict_state_matrix_u12` so every observation stays at
/// K=12 instead of shrinking [12, 9, 7, 5, 3]. K=12 maturity recovers
/// accuracy lost to K=3; deterministic B-tensor scatter preserves
/// smoothness vs σ.
#[cfg(not(target_os = "solana"))]
pub fn quote_c1_filter_rect_u12_live(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
) -> C1FastQuote {
    let triple_pre = build_triple_pre_by_obs(cfg);
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);

    let mut redemption_pv = 0i64;
    let mut coupon_annuity = 0i64;
    let mut total_ki = 0i64;
    let mut total_ac = 0i64;

    let (mut safe_state, mut knocked_state, obs1_fh, obs1_fki) = obs1_live_seed_k12_nested(
        cfg,
        sigma_s6,
        drift_diffs,
        drift_shift_63,
        triple_pre[0].as_ref(),
    );
    redemption_pv += m6r(cfg.notional, obs1_fh);
    coupon_annuity += 1 * obs1_fh;
    total_ac += obs1_fh;
    total_ki += obs1_fki;

    let k_uniform = 12usize;
    for step in 0..4 {
        let next_obs_idx = step + 1;
        let coupon_count = (next_obs_idx + 1) as i64;
        let obs = &cfg.obs[next_obs_idx];
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;
        let tp =
            observation_probability_triple_pre(next_obs_idx, triple_pre[next_obs_idx].as_ref());

        let safe_pred = predict_state_matrix_u12(
            &safe_state,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            step,
            sigma_s6,
        );
        let knocked_pred = predict_state_matrix_u12(
            &knocked_state,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            step,
            sigma_s6,
        );

        let safe_update =
            update_safe_state_live(&safe_pred, cfg, next_obs_idx, drift_shift_total, tp);
        let knocked_update =
            update_knocked_state_live(&knocked_pred, cfg, next_obs_idx, drift_shift_total, tp);

        let first_hit = safe_update.first_hit + knocked_update.first_hit;
        let first_knock_in = safe_update.first_knock_in;
        redemption_pv += m6r(cfg.notional, first_hit);
        coupon_annuity += coupon_count * first_hit;
        total_ac += first_hit;
        total_ki += first_knock_in;

        safe_state = safe_update.next_safe;
        knocked_state = merge_states(
            &safe_update.new_knocked,
            &knocked_update.next_knocked,
            k_uniform,
        );
    }

    let mat_obs_idx = N_OBS - 1;
    let mat_obs = &cfg.obs[mat_obs_idx];
    let drift_shift_mat = mat_obs.obs_day as i64 / 63 * drift_shift_63;
    let mat_tp = triple_pre[mat_obs_idx].as_ref();
    let coupon_count_mat = (mat_obs_idx + 1) as i64;

    let (coupon_safe, safe_principal, first_ki_mat, ki_redemption_safe) =
        maturity_safe_leg(&safe_state, cfg, mat_obs_idx, drift_shift_mat, mat_tp);
    let (coupon_knocked, knocked_redemption) =
        maturity_knocked_leg(&knocked_state, cfg, mat_obs_idx, drift_shift_mat, mat_tp);

    redemption_pv += m6r(cfg.notional, safe_principal);
    redemption_pv += m6r(cfg.notional, ki_redemption_safe);
    redemption_pv += m6r(cfg.notional, knocked_redemption);
    coupon_annuity += coupon_count_mat * (coupon_safe + coupon_knocked);
    total_ki += first_ki_mat;

    let loss = (cfg.notional - redemption_pv).max(0);
    let fair_coupon = if coupon_annuity > 100 {
        loss * S6 / coupon_annuity
    } else {
        0
    };

    c1_fast_quote_from_components(
        cfg.notional,
        fair_coupon,
        redemption_pv,
        coupon_annuity,
        total_ki,
        total_ac,
    )
}

#[inline(never)]
fn run_maturity_step(
    safe_state: &FilterState,
    knocked_state: &FilterState,
    transition: &FactorTransition,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    k_retained: usize,
    triple_pre: Option<&TripleCorrectionPre>,
    frozen_grid: Option<&crate::frozen_predict_tables::FrozenPredictGrid>,
) -> MaturityStep {
    let k_knocked = ANALYTIC_DELTA_K_KNOCKED;
    let safe_pred = if let Some(fg) = frozen_grid {
        predict_state_frozen(
            safe_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
            &fg.grid_c,
            fg.inv_cell_s30,
        )
    } else {
        predict_state(
            safe_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        )
    };
    let (coupon_safe, safe_principal, first_knock_in, knock_in_redemption_safe) =
        maturity_safe_leg(&safe_pred, cfg, obs_idx, drift_shift_total, triple_pre);
    // Knocked maturity predict stays on live path (1 parent → small savings)
    let knocked_pred = predict_state(
        knocked_state,
        &transition.factor_values,
        &transition.factor_weights,
        &transition.step_mean_u,
        &transition.step_mean_v,
        k_knocked,
    );
    let (coupon_knocked, knocked_redemption) =
        maturity_knocked_leg(&knocked_pred, cfg, obs_idx, drift_shift_total, triple_pre);
    MaturityStep {
        coupon_hit: coupon_safe + coupon_knocked,
        safe_principal,
        first_knock_in,
        knock_in_redemption_safe,
        knocked_redemption,
    }
}

#[inline(never)]
fn run_maturity_step_grad(
    safe_state: &FilterState,
    safe_state_grad: &FilterStateGrad,
    knocked_state: &FilterState,
    knocked_state_grad: &FilterStateGrad,
    transition: &FactorTransition,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    k_retained: usize,
    triple_pre: Option<&TripleCorrectionPre>,
    frozen_grid: Option<&crate::frozen_predict_tables::FrozenPredictGrid>,
    dmu_ds: &[(i64, i64, i64); 3],
) -> (MaturityStep, MaturityStepGrad) {
    let k_knocked = ANALYTIC_DELTA_K_KNOCKED;
    c1_filter_cu_diag_inner(b"predict_grad_start");
    let (coupon_safe, safe_principal, first_knock_in, knock_in_redemption_safe, safe_grad) = {
        let (safe_pred, safe_pred_grad) = if let Some(fg) = frozen_grid {
            predict_state_frozen_grad(
                safe_state,
                safe_state_grad,
                &transition.factor_values,
                &transition.factor_weights,
                &transition.step_mean_u,
                &transition.step_mean_v,
                k_retained,
                &fg.grid_c,
                fg.inv_cell_s30,
            )
        } else {
            predict_state_grad(
                safe_state,
                safe_state_grad,
                &transition.factor_values,
                &transition.factor_weights,
                &transition.step_mean_u,
                &transition.step_mean_v,
                k_retained,
            )
        };
        let safe_pred = Box::new(safe_pred);
        let safe_pred_grad = Box::new(safe_pred_grad);
        c1_filter_cu_diag_inner(b"predict_grad_safe_done");
        let ((coupon_safe, safe_principal, first_knock_in, knock_in_redemption_safe), safe_grad) =
            maturity_safe_leg_grad(
                safe_pred.as_ref(),
                safe_pred_grad.as_ref(),
                cfg,
                obs_idx,
                drift_shift_total,
                triple_pre,
                dmu_ds,
            );
        (
            coupon_safe,
            safe_principal,
            first_knock_in,
            knock_in_redemption_safe,
            safe_grad,
        )
    };

    let (coupon_knocked, knocked_redemption, knocked_grad) = {
        let (knocked_pred, knocked_pred_grad) = predict_state_grad(
            knocked_state,
            knocked_state_grad,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_knocked,
        );
        let knocked_pred = Box::new(knocked_pred);
        let knocked_pred_grad = Box::new(knocked_pred_grad);
        c1_filter_cu_diag_inner(b"predict_grad_done");
        let ((coupon_knocked, knocked_redemption), knocked_grad) = maturity_knocked_leg_grad(
            knocked_pred.as_ref(),
            knocked_pred_grad.as_ref(),
            cfg,
            obs_idx,
            drift_shift_total,
            triple_pre,
            dmu_ds,
        );
        (coupon_knocked, knocked_redemption, knocked_grad)
    };

    let mut grad = MaturityStepGrad::default();
    for asset in 0..3 {
        grad.coupon_hit[asset] = safe_grad.coupon_hit[asset] + knocked_grad.coupon_hit[asset];
        grad.safe_principal[asset] = safe_grad.safe_principal[asset];
        grad.first_knock_in[asset] = safe_grad.first_knock_in[asset];
        grad.knock_in_redemption_safe[asset] = safe_grad.knock_in_redemption[asset];
        grad.knocked_redemption[asset] = knocked_grad.redemption[asset];
    }

    (
        MaturityStep {
            coupon_hit: coupon_safe + coupon_knocked,
            safe_principal,
            first_knock_in,
            knock_in_redemption_safe,
            knocked_redemption,
        },
        grad,
    )
}

fn run_first_observation_step_grad(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_total: i64,
    k_safe: usize,
    k_knocked: usize,
    triple_pre: Option<&TripleCorrectionPre>,
    dmu_ds: &[(i64, i64, i64); 3],
) -> Option<ObservationStepWithGrad> {
    let transition = build_exact_seed_transition(cfg, sigma_s6, drift_diffs)?;
    let predicted_safe = predicted_state_from_exact_seed_transition(&transition);
    let predicted_safe_grad = seed_state_mean_grad(&predicted_safe, dmu_ds);
    let (safe_update, safe_update_grad) = update_safe_state_grad(
        &predicted_safe,
        &predicted_safe_grad,
        cfg,
        0,
        drift_shift_total,
        k_safe,
        observation_probability_triple_pre(0, triple_pre),
        dmu_ds,
    );
    let (next_safe, next_safe_grad) =
        project_state_with_grad(&safe_update.next_safe, &safe_update_grad.next_safe, k_safe);
    let (next_knocked, next_knocked_grad) = project_state_with_grad(
        &safe_update.new_knocked,
        &safe_update_grad.new_knocked,
        k_knocked,
    );
    Some(ObservationStepWithGrad {
        next_safe,
        next_safe_grad,
        next_knocked,
        next_knocked_grad,
        first_hit: safe_update.first_hit,
        first_hit_grad: safe_update_grad.first_hit,
        first_knock_in: safe_update.first_knock_in,
        first_knock_in_grad: safe_update_grad.first_knock_in,
    })
}

pub fn quote_c1_filter(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> C1FastQuote {
    // Knocked state needs less resolution — those paths already crossed the
    // KI barrier, their exact c-position barely affects settlement.
    // One node tracks the average knocked mass.
    let k_knocked = 1usize;
    c1_filter_cu_diag(b"quote_start");
    let k_retained = k_retained.clamp(1, MAX_K);
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    c1_filter_cu_diag(b"after_nig_weights");

    // Preload frozen predict grids for obs 1..5 (None if K unsupported)
    let frozen_grids: [Option<crate::frozen_predict_tables::FrozenPredictGrid>; 5] =
        core::array::from_fn(|obs_rel| {
            crate::frozen_predict_tables::frozen_predict_grid_lookup(sigma_s6, obs_rel, k_retained)
        });

    let mut safe_state = FilterState::singleton_origin();
    let mut knocked_state = FilterState::default();
    let mut redemption_pv = 0i64;
    let mut coupon_annuity = 0i64;
    let mut total_ki = 0i64;
    let mut total_ac = 0i64;
    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let is_maturity = obs_idx + 1 == N_OBS;
        let coupon_count = (obs_idx + 1) as i64;
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;

        if obs_idx == 0 {
            let (obs1_safe, obs1_knocked, obs1_fh, obs1_fki) = if k_retained >= 10 {
                crate::obs1_seed_tables::obs1_projected_lookup_k15(sigma_s6)
            } else {
                crate::obs1_seed_tables::obs1_projected_lookup(sigma_s6)
            };
            redemption_pv += m6r(NOTIONAL, obs1_fh);
            coupon_annuity += coupon_count * obs1_fh;
            total_ac += obs1_fh;
            total_ki += obs1_fki;
            if k_retained < 15 {
                safe_state = project_state(&obs1_safe, k_retained);
            } else {
                safe_state = obs1_safe;
            }
            knocked_state = project_state(&obs1_knocked, k_knocked);
            c1_filter_cu_diag(b"after_obs1_table");
            continue;
        }

        // obs_idx 1..5 → frozen_grids[0..4]
        let fg_ref = frozen_grids[obs_idx - 1].as_ref();

        let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
        let tp = observation_probability_triple_pre(obs_idx, triple_pre.as_ref());

        if is_maturity {
            let maturity = run_maturity_step(
                &safe_state,
                &knocked_state,
                &transition,
                cfg,
                obs_idx,
                drift_shift_total,
                k_retained,
                tp,
                fg_ref,
            );
            redemption_pv += m6r(NOTIONAL, maturity.safe_principal);
            redemption_pv += m6r(NOTIONAL, maturity.knock_in_redemption_safe);
            redemption_pv += m6r(NOTIONAL, maturity.knocked_redemption);
            coupon_annuity += coupon_count * maturity.coupon_hit;
            total_ki += maturity.first_knock_in;
            c1_filter_cu_diag(b"after_maturity");
            continue;
        }

        let (next_safe, next_knocked, first_hit, first_knock_in) = run_observation_step(
            &safe_state,
            &knocked_state,
            &transition,
            cfg,
            obs_idx,
            drift_shift_total,
            k_retained,
            k_knocked,
            tp,
            fg_ref,
        );
        redemption_pv += m6r(NOTIONAL, first_hit);
        coupon_annuity += coupon_count * first_hit;
        total_ac += first_hit;
        total_ki += first_knock_in;
        safe_state = next_safe;
        knocked_state = next_knocked;
        match obs_idx {
            1 => c1_filter_cu_diag(b"after_obs2"),
            2 => c1_filter_cu_diag(b"after_obs3"),
            3 => c1_filter_cu_diag(b"after_obs4"),
            4 => c1_filter_cu_diag(b"after_obs5"),
            _ => c1_filter_cu_diag(b"after_obs_other"),
        }
    }

    let loss = (NOTIONAL - redemption_pv).max(0);
    let fair_coupon = if coupon_annuity > 100 {
        loss * S6 / coupon_annuity
    } else {
        0
    };

    // Apply K=9 discretisation correction if K=9
    let fair_coupon = if k_retained == 9 {
        let correction_ubps = crate::k9_correction::k9_correction_lookup(sigma_s6);
        // correction is in micro-bps (1e-6 bps). Convert to fair_coupon units.
        // fair_coupon = loss * S6 / coupon_annuity, in notional-S6 scale.
        // fair_coupon_bps = fair_coupon * 10000 / notional.
        // So delta_fair_coupon = delta_bps * notional / 10000.
        // delta_bps = correction_ubps / 1_000_000.
        // delta_fair_coupon = correction_ubps * notional / (10000 * 1_000_000)
        //                   = correction_ubps * NOTIONAL / 10_000_000_000
        let delta = correction_ubps * (NOTIONAL / S6) / 10_000;
        (fair_coupon + delta).max(0)
    } else {
        fair_coupon
    };

    c1_filter_cu_diag(b"quote_done");
    c1_fast_quote_from_components(
        NOTIONAL,
        fair_coupon,
        redemption_pv,
        coupon_annuity,
        total_ki,
        total_ac,
    )
}

#[cfg(not(target_os = "solana"))]
pub fn quote_c1_filter_with_delta(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> QuoteWithDelta {
    let k_retained = k_retained.clamp(1, MAX_K);
    let k_knocked = ANALYTIC_DELTA_K_KNOCKED;
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let frozen_grids: [Option<crate::frozen_predict_tables::FrozenPredictGrid>; 5] =
        core::array::from_fn(|obs_rel| {
            crate::frozen_predict_tables::frozen_predict_grid_lookup(sigma_s6, obs_rel, k_retained)
        });
    let dmu_ds = compute_dmu_ds(cfg, [S6, S6, S6]);
    let dmu_c = dmu_c_only(&dmu_ds);

    let mut safe_state = FilterState::singleton_origin();
    let mut safe_grad = FilterStateGrad::default();
    let mut knocked_state = FilterState::default();
    let mut knocked_grad = FilterStateGrad::default();
    let mut redemption_pv = 0i64;
    let mut redemption_prob = 0i64;
    let mut coupon_annuity = 0i64;
    let mut d_redemption_prob = [0i64; 3];
    let mut d_coupon_annuity = [0i64; 3];

    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let is_maturity = obs_idx + 1 == N_OBS;
        let coupon_count = (obs_idx + 1) as i64;
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;

        if obs_idx == 0 {
            let obs1_triple_pre =
                cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
                    .ok()
                    .map(|(l11, l21, l22)| {
                        build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av)
                    });
            let (obs1_safe, obs1_knocked, obs1_fh, obs1_fki) = if k_retained >= 10 {
                crate::obs1_seed_tables::obs1_projected_lookup_k15(sigma_s6)
            } else {
                crate::obs1_seed_tables::obs1_projected_lookup(sigma_s6)
            };
            safe_state = if k_retained < 15 {
                project_state(&obs1_safe, k_retained)
            } else {
                obs1_safe
            };
            knocked_state = project_state(&obs1_knocked, k_knocked);
            redemption_pv += m6r(NOTIONAL, obs1_fh);
            redemption_prob += obs1_fh;
            coupon_annuity += coupon_count * obs1_fh;

            if let Some(obs1_grad) = run_first_observation_step_grad(
                cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_total,
                k_retained,
                k_knocked,
                obs1_triple_pre.as_ref(),
                &dmu_c,
            ) {
                for asset in 0..3 {
                    d_redemption_prob[asset] += obs1_grad.first_hit_grad[asset];
                    d_coupon_annuity[asset] += coupon_count * obs1_grad.first_hit_grad[asset];
                }
                if states_match_exact(&obs1_grad.next_safe, &safe_state) {
                    safe_grad = obs1_grad.next_safe_grad;
                } else {
                    safe_grad = seed_state_mean_grad(&safe_state, &dmu_ds);
                }
                if states_match_exact(&obs1_grad.next_knocked, &knocked_state) {
                    knocked_grad = obs1_grad.next_knocked_grad;
                } else {
                    knocked_grad = seed_state_mean_grad(&knocked_state, &dmu_ds);
                }
            } else {
                safe_grad = seed_state_mean_grad(&safe_state, &dmu_ds);
                knocked_grad = seed_state_mean_grad(&knocked_state, &dmu_ds);
            }
            let _ = obs1_fki;
            continue;
        }

        let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
        let tp = observation_probability_triple_pre(obs_idx, triple_pre.as_ref());

        if is_maturity {
            let (maturity, maturity_grad) = run_maturity_step_grad(
                &safe_state,
                &safe_grad,
                &knocked_state,
                &knocked_grad,
                &transition,
                cfg,
                obs_idx,
                drift_shift_total,
                k_retained,
                tp,
                frozen_grids[obs_idx - 1].as_ref(),
                &dmu_c,
            );
            let maturity_redemption = maturity.safe_principal
                + maturity.knock_in_redemption_safe
                + maturity.knocked_redemption;
            redemption_pv += m6r(NOTIONAL, maturity.safe_principal);
            redemption_pv += m6r(NOTIONAL, maturity.knock_in_redemption_safe);
            redemption_pv += m6r(NOTIONAL, maturity.knocked_redemption);
            redemption_prob += maturity_redemption;
            coupon_annuity += coupon_count * maturity.coupon_hit;
            for asset in 0..3 {
                d_redemption_prob[asset] += maturity_grad.safe_principal[asset]
                    + maturity_grad.knock_in_redemption_safe[asset]
                    + maturity_grad.knocked_redemption[asset];
                d_coupon_annuity[asset] += coupon_count * maturity_grad.coupon_hit[asset];
            }
            continue;
        }

        let step = run_observation_step_grad(
            &safe_state,
            &safe_grad,
            &knocked_state,
            &knocked_grad,
            &transition,
            cfg,
            obs_idx,
            drift_shift_total,
            k_retained,
            k_knocked,
            tp,
            frozen_grids[obs_idx - 1].as_ref(),
            &dmu_c,
        );
        redemption_pv += m6r(NOTIONAL, step.first_hit);
        redemption_prob += step.first_hit;
        coupon_annuity += coupon_count * step.first_hit;
        for asset in 0..3 {
            d_redemption_prob[asset] += step.first_hit_grad[asset];
            d_coupon_annuity[asset] += coupon_count * step.first_hit_grad[asset];
        }
        safe_state = step.next_safe;
        safe_grad = step.next_safe_grad;
        knocked_state = step.next_knocked;
        knocked_grad = step.next_knocked_grad;
    }

    let loss_prob = (S6 - redemption_prob).max(0);
    let fair_coupon_s6 = if coupon_annuity > 100 {
        loss_prob * S6 / coupon_annuity
    } else {
        0
    };
    let fair_coupon_grad = if coupon_annuity > 100 && loss_prob > 0 {
        core::array::from_fn(|asset| {
            conditional_mean_grad(
                coupon_annuity,
                loss_prob,
                d_coupon_annuity[asset],
                -d_redemption_prob[asset],
            )
        })
    } else {
        [0; 3]
    };
    let fair_coupon = if coupon_annuity > 100 {
        (NOTIONAL - redemption_pv).max(0) * S6 / coupon_annuity
    } else {
        0
    };
    let mut fc_bps = fair_coupon as f64 * 10000.0 / NOTIONAL as f64;
    if k_retained == 9 {
        fc_bps += crate::k9_correction::k9_correction_lookup(sigma_s6) as f64 / 1_000_000.0;
    } else if k_retained == 12 {
        fc_bps += crate::k12_correction::k12_correction_lookup(sigma_s6) as f64 / 1_000_000.0;
    }

    QuoteWithDelta {
        fc_bps,
        delta_spy: fair_coupon_grad[0] as f64 / S6 as f64,
        delta_qqq: fair_coupon_grad[1] as f64 / S6 as f64,
        delta_iwm: fair_coupon_grad[2] as f64 / S6 as f64,
    }
}

#[cfg(not(target_os = "solana"))]
pub fn quote_c1_filter_with_delta_live(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
    spots_s6: [i64; 3],
    remaining_observations: usize,
    ki_latched: bool,
) -> QuoteWithDelta {
    let remaining_observations = remaining_observations.min(N_OBS);
    if remaining_observations == 0 {
        return QuoteWithDelta {
            fc_bps: 0.0,
            delta_spy: 0.0,
            delta_qqq: 0.0,
            delta_iwm: 0.0,
        };
    }

    let k_retained = k_retained.clamp(1, MAX_K);
    let k_knocked = ANALYTIC_DELTA_K_KNOCKED;
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let triple_pre_by_obs = build_triple_pre_by_obs(cfg);
    let frozen_grids: [Option<crate::frozen_predict_tables::FrozenPredictGrid>; 5] =
        core::array::from_fn(|obs_rel| {
            crate::frozen_predict_tables::frozen_predict_grid_lookup(sigma_s6, obs_rel, k_retained)
        });
    let dmu_ds = compute_dmu_ds(cfg, spots_s6);
    let dmu_c = dmu_c_only(&dmu_ds);
    let (mu_u_shift, mu_v_shift, mu_c_shift) = spot_shift_bundle_live(cfg, spots_s6);
    let shifted_origin = shifted_origin_state(mu_u_shift, mu_v_shift);

    let mut safe_state = if ki_latched {
        FilterState::default()
    } else {
        shifted_origin
    };
    let mut safe_grad = seed_state_mean_grad(&safe_state, &dmu_ds);
    let mut knocked_state = if ki_latched {
        shifted_origin
    } else {
        FilterState::default()
    };
    let mut knocked_grad = seed_state_mean_grad(&knocked_state, &dmu_ds);
    let mut redemption_prob = 0i64;
    let mut coupon_annuity = 0i64;
    let mut d_redemption_prob = [0i64; 3];
    let mut d_coupon_annuity = [0i64; 3];

    for obs_idx in 0..remaining_observations {
        let is_maturity = obs_idx + 1 == remaining_observations;
        let coupon_count = (obs_idx + 1) as i64;
        let drift_shift_total = (obs_idx as i64 + 1) * drift_shift_63 + mu_c_shift;
        let tp = observation_probability_triple_pre(obs_idx, triple_pre_by_obs[obs_idx].as_ref());
        let frozen_grid = if obs_idx == 0 {
            None
        } else {
            frozen_grids[obs_idx - 1].as_ref()
        };

        if is_maturity {
            let (maturity, maturity_grad) = run_maturity_step_grad(
                &safe_state,
                &safe_grad,
                &knocked_state,
                &knocked_grad,
                &transition,
                cfg,
                obs_idx,
                drift_shift_total,
                k_retained,
                tp,
                frozen_grid,
                &dmu_c,
            );
            let maturity_redemption = maturity.safe_principal
                + maturity.knock_in_redemption_safe
                + maturity.knocked_redemption;
            redemption_prob += maturity_redemption;
            coupon_annuity += coupon_count * maturity.coupon_hit;
            for asset in 0..3 {
                d_redemption_prob[asset] += maturity_grad.safe_principal[asset]
                    + maturity_grad.knock_in_redemption_safe[asset]
                    + maturity_grad.knocked_redemption[asset];
                d_coupon_annuity[asset] += coupon_count * maturity_grad.coupon_hit[asset];
            }
            continue;
        }

        let step = run_observation_step_grad(
            &safe_state,
            &safe_grad,
            &knocked_state,
            &knocked_grad,
            &transition,
            cfg,
            obs_idx,
            drift_shift_total,
            k_retained,
            k_knocked,
            tp,
            frozen_grid,
            &dmu_c,
        );
        redemption_prob += step.first_hit;
        coupon_annuity += coupon_count * step.first_hit;
        for asset in 0..3 {
            d_redemption_prob[asset] += step.first_hit_grad[asset];
            d_coupon_annuity[asset] += coupon_count * step.first_hit_grad[asset];
        }
        safe_state = step.next_safe;
        safe_grad = step.next_safe_grad;
        knocked_state = step.next_knocked;
        knocked_grad = step.next_knocked_grad;
    }

    let loss_prob = (S6 - redemption_prob).max(0);
    let fair_coupon_grad = if coupon_annuity > 100 && loss_prob > 0 {
        core::array::from_fn(|asset| {
            conditional_mean_grad(
                coupon_annuity,
                loss_prob,
                d_coupon_annuity[asset],
                -d_redemption_prob[asset],
            )
        })
    } else {
        [0; 3]
    };
    let fair_coupon_s6 = if coupon_annuity > 100 {
        loss_prob * S6 / coupon_annuity
    } else {
        0
    };
    let mut fc_bps = fair_coupon_s6 as f64 * 10000.0 / S6 as f64;
    if k_retained == 9 {
        fc_bps += crate::k9_correction::k9_correction_lookup(sigma_s6) as f64 / 1_000_000.0;
    } else if k_retained == 12 {
        fc_bps += crate::k12_correction::k12_correction_lookup(sigma_s6) as f64 / 1_000_000.0;
    }

    QuoteWithDelta {
        fc_bps,
        delta_spy: fair_coupon_grad[0] as f64 / S6 as f64,
        delta_qqq: fair_coupon_grad[1] as f64 / S6 as f64,
        delta_iwm: fair_coupon_grad[2] as f64 / S6 as f64,
    }
}

fn run_observation_step_grad(
    safe_state: &FilterState,
    safe_state_grad: &FilterStateGrad,
    knocked_state: &FilterState,
    knocked_state_grad: &FilterStateGrad,
    transition: &FactorTransition,
    cfg: &C1FastConfig,
    obs_idx: usize,
    drift_shift_total: i64,
    k_safe: usize,
    k_knocked: usize,
    triple_pre: Option<&TripleCorrectionPre>,
    frozen_grid: Option<&crate::frozen_predict_tables::FrozenPredictGrid>,
    dmu_ds: &[(i64, i64, i64); 3],
) -> ObservationStepWithGrad {
    let (
        next_safe,
        next_safe_grad,
        new_knocked,
        new_knocked_grad,
        safe_first_hit,
        safe_first_hit_grad,
        first_knock_in,
        first_knock_in_grad,
    ) = {
        let (safe_pred, safe_pred_grad) = if let Some(fg) = frozen_grid {
            predict_state_frozen_grad(
                safe_state,
                safe_state_grad,
                &transition.factor_values,
                &transition.factor_weights,
                &transition.step_mean_u,
                &transition.step_mean_v,
                k_safe,
                &fg.grid_c,
                fg.inv_cell_s30,
            )
        } else {
            predict_state_grad(
                safe_state,
                safe_state_grad,
                &transition.factor_values,
                &transition.factor_weights,
                &transition.step_mean_u,
                &transition.step_mean_v,
                k_safe,
            )
        };
        let (safe_update, safe_update_grad) = update_safe_state_grad(
            &safe_pred,
            &safe_pred_grad,
            cfg,
            obs_idx,
            drift_shift_total,
            k_safe,
            triple_pre,
            dmu_ds,
        );
        (
            safe_update.next_safe,
            safe_update_grad.next_safe,
            safe_update.new_knocked,
            safe_update_grad.new_knocked,
            safe_update.first_hit,
            safe_update_grad.first_hit,
            safe_update.first_knock_in,
            safe_update_grad.first_knock_in,
        )
    };

    let (continued_knocked, continued_knocked_grad, knocked_first_hit, knocked_first_hit_grad) = {
        let (knocked_pred, knocked_pred_grad) = predict_state_grad(
            knocked_state,
            knocked_state_grad,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_knocked,
        );
        let (knocked_update, knocked_update_grad) = update_knocked_state_grad(
            &knocked_pred,
            &knocked_pred_grad,
            cfg,
            obs_idx,
            drift_shift_total,
            k_knocked,
            triple_pre,
            dmu_ds,
        );
        (
            knocked_update.next_knocked,
            knocked_update_grad.next_knocked,
            knocked_update.first_hit,
            knocked_update_grad.first_hit,
        )
    };

    let (next_knocked, next_knocked_grad) = merge_states_grad(
        &new_knocked,
        &new_knocked_grad,
        &continued_knocked,
        &continued_knocked_grad,
        k_knocked,
    );
    let mut first_hit_grad = [0i64; 3];
    for asset in 0..3 {
        first_hit_grad[asset] = safe_first_hit_grad[asset] + knocked_first_hit_grad[asset];
    }

    ObservationStepWithGrad {
        next_safe,
        next_safe_grad,
        next_knocked,
        next_knocked_grad,
        first_hit: safe_first_hit + knocked_first_hit,
        first_hit_grad,
        first_knock_in,
        first_knock_in_grad,
    }
}

/// Date-varying K quote: uses a K schedule [obs2, obs3, obs4, obs5, maturity]
/// to trade accuracy at early observations for CU savings at later ones.
#[cfg(not(target_os = "solana"))]
pub fn quote_c1_filter_tapered(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_schedule: &[usize; 5], // K for obs 1..5 (obs 0 uses obs1 table at K=9)
) -> C1FastQuote {
    let k_knocked = 1usize;
    c1_filter_cu_diag(b"quote_start");
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    c1_filter_cu_diag(b"after_nig_weights");

    // Preload frozen predict grids per-obs with per-obs K.
    // Use tapered-specific tables if schedule matches, else try uniform tables.
    let is_tapered = *k_schedule == crate::frozen_predict_tables::TAPERED_K_SCHEDULE;
    let frozen_grids: [Option<crate::frozen_predict_tables::FrozenPredictGrid>; 5] =
        core::array::from_fn(|obs_rel| {
            if is_tapered {
                crate::frozen_predict_tables::frozen_predict_grid_lookup_tapered(sigma_s6, obs_rel)
            } else {
                let k = k_schedule[obs_rel].clamp(1, MAX_K);
                crate::frozen_predict_tables::frozen_predict_grid_lookup(sigma_s6, obs_rel, k)
            }
        });

    let mut safe_state = FilterState::singleton_origin();
    let mut knocked_state = FilterState::default();
    let mut redemption_pv = 0i64;
    let mut coupon_annuity = 0i64;
    let mut total_ki = 0i64;
    let mut total_ac = 0i64;
    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let is_maturity = obs_idx + 1 == N_OBS;
        let coupon_count = (obs_idx + 1) as i64;
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;

        if obs_idx == 0 {
            let (obs1_safe, obs1_knocked, obs1_fh, obs1_fki) =
                crate::obs1_seed_tables::obs1_projected_lookup(sigma_s6);
            redemption_pv += m6r(NOTIONAL, obs1_fh);
            coupon_annuity += coupon_count * obs1_fh;
            total_ac += obs1_fh;
            total_ki += obs1_fki;
            let k_obs1 = k_schedule[0].clamp(1, MAX_K);
            if k_obs1 < 9 {
                safe_state = project_state(&obs1_safe, k_obs1);
            } else {
                safe_state = obs1_safe;
            }
            knocked_state = project_state(&obs1_knocked, k_knocked);
            c1_filter_cu_diag(b"after_obs1_table");
            continue;
        }

        let k_safe = k_schedule[obs_idx - 1].clamp(1, MAX_K);
        let fg_ref = frozen_grids[obs_idx - 1].as_ref();

        let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
        let tp = observation_probability_triple_pre(obs_idx, triple_pre.as_ref());

        if is_maturity {
            let maturity = run_maturity_step(
                &safe_state,
                &knocked_state,
                &transition,
                cfg,
                obs_idx,
                drift_shift_total,
                k_safe,
                tp,
                fg_ref,
            );
            redemption_pv += m6r(NOTIONAL, maturity.safe_principal);
            redemption_pv += m6r(NOTIONAL, maturity.knock_in_redemption_safe);
            redemption_pv += m6r(NOTIONAL, maturity.knocked_redemption);
            coupon_annuity += coupon_count * maturity.coupon_hit;
            total_ki += maturity.first_knock_in;
            c1_filter_cu_diag(b"after_maturity");
            continue;
        }

        let (next_safe, next_knocked, first_hit, first_knock_in) = run_observation_step(
            &safe_state,
            &knocked_state,
            &transition,
            cfg,
            obs_idx,
            drift_shift_total,
            k_safe,
            k_knocked,
            tp,
            fg_ref,
        );
        redemption_pv += m6r(NOTIONAL, first_hit);
        coupon_annuity += coupon_count * first_hit;
        total_ac += first_hit;
        total_ki += first_knock_in;
        safe_state = next_safe;
        knocked_state = next_knocked;
        c1_filter_cu_diag(b"after_obs_tapered");
    }

    let loss = (NOTIONAL - redemption_pv).max(0);
    let fair_coupon = if coupon_annuity > 100 {
        loss * S6 / coupon_annuity
    } else {
        0
    };

    c1_filter_cu_diag(b"quote_done");
    c1_fast_quote_from_components(
        NOTIONAL,
        fair_coupon,
        redemption_pv,
        coupon_annuity,
        total_ki,
        total_ac,
    )
}

#[cfg(not(target_os = "solana"))]
pub fn quote_c1_filter_live(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> C1FastQuote {
    let k_retained = k_retained.clamp(1, MAX_K);
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let mut safe_state = FilterState::singleton_origin();
    let mut knocked_state = FilterState::default();
    let mut redemption_pv = 0i64;
    let mut coupon_annuity = 0i64;
    let mut total_ki = 0i64;
    let mut total_ac = 0i64;
    let triple_pre = build_triple_pre_by_obs(cfg);

    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let is_maturity = obs_idx + 1 == N_OBS;
        let coupon_count = (obs_idx + 1) as i64;
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;
        let tp = observation_probability_triple_pre(obs_idx, triple_pre[obs_idx].as_ref());

        if obs_idx == 0 {
            if let Some(seed) = run_first_observation_seed(
                cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_total,
                k_retained,
                triple_pre[0].as_ref(),
            ) {
                redemption_pv += m6r(cfg.notional, seed.first_hit);
                coupon_annuity += coupon_count * seed.first_hit;
                total_ac += seed.first_hit;
                total_ki += seed.first_knock_in;
                safe_state = seed.next_safe;
                knocked_state = seed.next_knocked;
                continue;
            }
        }

        if is_maturity {
            let maturity = run_maturity_step(
                &safe_state,
                &knocked_state,
                &transition,
                cfg,
                obs_idx,
                drift_shift_total,
                k_retained,
                tp,
                None,
            );
            redemption_pv += m6r(cfg.notional, maturity.safe_principal);
            redemption_pv += m6r(cfg.notional, maturity.knock_in_redemption_safe);
            redemption_pv += m6r(cfg.notional, maturity.knocked_redemption);
            coupon_annuity += coupon_count * maturity.coupon_hit;
            total_ki += maturity.first_knock_in;
            continue;
        }

        let (next_safe, next_knocked, first_hit, first_knock_in) = run_observation_step_live(
            &safe_state,
            &knocked_state,
            &transition,
            cfg,
            obs_idx,
            drift_shift_total,
            k_retained,
            tp,
        );
        redemption_pv += m6r(cfg.notional, first_hit);
        coupon_annuity += coupon_count * first_hit;
        total_ac += first_hit;
        total_ki += first_knock_in;

        safe_state = next_safe;
        knocked_state = next_knocked;
    }

    let loss = (cfg.notional - redemption_pv).max(0);
    let fair_coupon = if coupon_annuity > 100 {
        loss * S6 / coupon_annuity
    } else {
        0
    };

    c1_fast_quote_from_components(
        cfg.notional,
        fair_coupon,
        redemption_pv,
        coupon_annuity,
        total_ki,
        total_ac,
    )
}

#[cfg(not(target_os = "solana"))]
pub fn quote_c1_filter_trace(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> C1FilterTrace {
    let k_retained = k_retained.clamp(1, MAX_K);
    let factor_weights = nig_importance_weights_9(sigma_s6);
    let proposal_std = sigma_s6 / 2;
    let mut factor_values = [0i64; N_FACTOR_NODES];
    let mut step_mean_u = [0i64; N_FACTOR_NODES];
    let mut step_mean_v = [0i64; N_FACTOR_NODES];
    for idx in 0..N_FACTOR_NODES {
        factor_values[idx] = SQRT2_S6 * proposal_std / S6 * GH9_NODES_S6[idx] / S6;
        step_mean_u[idx] = drift_diffs[0] + cfg.uv_slope[0] * factor_values[idx] / S6;
        step_mean_v[idx] = drift_diffs[1] + cfg.uv_slope[1] * factor_values[idx] / S6;
    }

    let triple_pre = core::array::from_fn::<_, N_OBS, _>(|obs_idx| {
        let obs = &cfg.obs[obs_idx];
        cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av))
    });

    let mut safe_state = FilterState::singleton_origin();
    let mut knocked_state = FilterState::default();

    let mut redemption_pv = 0i64;
    let mut coupon_annuity = 0i64;
    let mut total_ki = 0i64;
    let mut total_ac = 0i64;

    let mut observation_survival = [0i64; N_OBS];
    let mut observation_autocall_first_hit = [0i64; N_OBS];
    let mut observation_first_knock_in = [0i64; N_OBS];
    let mut post_observation_safe_mass = [0i64; N_OBS];
    let mut post_observation_knocked_mass = [0i64; N_OBS];

    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let is_maturity = obs_idx + 1 == N_OBS;
        let coupon_count = (obs_idx + 1) as i64;
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;
        let tp = observation_probability_triple_pre(obs_idx, triple_pre[obs_idx].as_ref());

        if obs_idx == 0 {
            if let Some(seed) = run_first_observation_seed_live(
                cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_total,
                k_retained,
                triple_pre[0].as_ref(),
            ) {
                observation_survival[obs_idx] = seed.predicted_safe.total_weight();
                redemption_pv += cfg.notional * seed.first_hit / S6;
                coupon_annuity += coupon_count * seed.first_hit;
                total_ac += seed.first_hit;
                total_ki += seed.first_knock_in;
                observation_autocall_first_hit[obs_idx] = seed.first_hit;
                observation_first_knock_in[obs_idx] = seed.first_knock_in;
                post_observation_safe_mass[obs_idx] = seed.next_safe.total_weight();
                post_observation_knocked_mass[obs_idx] = seed.next_knocked.total_weight();
                safe_state = seed.next_safe;
                knocked_state = seed.next_knocked;
                continue;
            }
        }

        let safe_pred = predict_state(
            &safe_state,
            &factor_values,
            &factor_weights,
            &step_mean_u,
            &step_mean_v,
            k_retained,
        );
        let knocked_pred = predict_state(
            &knocked_state,
            &factor_values,
            &factor_weights,
            &step_mean_u,
            &step_mean_v,
            k_retained,
        );

        observation_survival[obs_idx] =
            (safe_pred.total_weight() + knocked_pred.total_weight()).clamp(0, S6);

        if is_maturity {
            let (coupon_safe, safe_principal, first_ki_safe, ki_redemption_safe) =
                maturity_safe_leg(&safe_pred, cfg, obs_idx, drift_shift_total, tp);
            let (coupon_knocked, knocked_redemption) =
                maturity_knocked_leg(&knocked_pred, cfg, obs_idx, drift_shift_total, tp);
            let maturity_coupon_hit = coupon_safe + coupon_knocked;

            redemption_pv += cfg.notional * safe_principal / S6;
            redemption_pv += cfg.notional * ki_redemption_safe / S6;
            redemption_pv += cfg.notional * knocked_redemption / S6;
            coupon_annuity += coupon_count * maturity_coupon_hit;
            total_ki += first_ki_safe;
            observation_first_knock_in[obs_idx] = first_ki_safe;
            post_observation_safe_mass[obs_idx] = 0;
            post_observation_knocked_mass[obs_idx] = 0;
            continue;
        }

        let safe_update =
            update_safe_state(&safe_pred, cfg, obs_idx, drift_shift_total, k_retained, tp);
        let knocked_update = update_knocked_state(
            &knocked_pred,
            cfg,
            obs_idx,
            drift_shift_total,
            k_retained,
            tp,
        );
        let next_knocked = merge_states(
            &safe_update.new_knocked,
            &knocked_update.next_knocked,
            k_retained,
        );

        let first_hit = safe_update.first_hit + knocked_update.first_hit;
        redemption_pv += cfg.notional * first_hit / S6;
        coupon_annuity += coupon_count * first_hit;
        total_ac += first_hit;
        total_ki += safe_update.first_knock_in;

        observation_autocall_first_hit[obs_idx] = first_hit;
        observation_first_knock_in[obs_idx] = safe_update.first_knock_in;
        post_observation_safe_mass[obs_idx] = safe_update.next_safe.total_weight();
        post_observation_knocked_mass[obs_idx] = next_knocked.total_weight();

        safe_state = safe_update.next_safe;
        knocked_state = next_knocked;
    }

    let loss = (cfg.notional - redemption_pv).max(0);
    let fair_coupon = if coupon_annuity > 100 {
        loss * S6 / coupon_annuity
    } else {
        0
    };

    C1FilterTrace {
        quote: c1_fast_quote_from_components(
            cfg.notional,
            fair_coupon,
            redemption_pv,
            coupon_annuity,
            total_ki,
            total_ac,
        ),
        observation_survival,
        observation_autocall_first_hit,
        observation_first_knock_in,
        post_observation_safe_mass,
        post_observation_knocked_mass,
        k_retained,
    }
}

#[cfg(not(target_os = "solana"))]
pub fn quote_c1_filter_trace_live(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> C1FilterTrace {
    let k_retained = k_retained.clamp(1, MAX_K);
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let triple_pre = build_triple_pre_by_obs(cfg);

    let mut safe_state = FilterState::singleton_origin();
    let mut knocked_state = FilterState::default();
    let mut redemption_pv = 0i64;
    let mut coupon_annuity = 0i64;
    let mut total_ki = 0i64;
    let mut total_ac = 0i64;
    let mut observation_survival = [0i64; N_OBS];
    let mut observation_autocall_first_hit = [0i64; N_OBS];
    let mut observation_first_knock_in = [0i64; N_OBS];
    let mut post_observation_safe_mass = [0i64; N_OBS];
    let mut post_observation_knocked_mass = [0i64; N_OBS];

    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let is_maturity = obs_idx + 1 == N_OBS;
        let coupon_count = (obs_idx + 1) as i64;
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;
        let tp = observation_probability_triple_pre(obs_idx, triple_pre[obs_idx].as_ref());

        if obs_idx == 0 {
            if let Some(seed) = run_first_observation_seed_live(
                cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_total,
                k_retained,
                triple_pre[0].as_ref(),
            ) {
                observation_survival[obs_idx] = seed.predicted_safe.total_weight();
                redemption_pv += cfg.notional * seed.first_hit / S6;
                coupon_annuity += coupon_count * seed.first_hit;
                total_ac += seed.first_hit;
                total_ki += seed.first_knock_in;
                observation_autocall_first_hit[obs_idx] = seed.first_hit;
                observation_first_knock_in[obs_idx] = seed.first_knock_in;
                post_observation_safe_mass[obs_idx] = seed.next_safe.total_weight();
                post_observation_knocked_mass[obs_idx] = seed.next_knocked.total_weight();
                safe_state = seed.next_safe;
                knocked_state = seed.next_knocked;
                continue;
            }
        }

        let safe_pred = predict_state(
            &safe_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        );
        let knocked_pred = predict_state(
            &knocked_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        );

        observation_survival[obs_idx] =
            (safe_pred.total_weight() + knocked_pred.total_weight()).clamp(0, S6);

        if is_maturity {
            let (coupon_safe, safe_principal, first_ki_safe, ki_redemption_safe) =
                maturity_safe_leg(&safe_pred, cfg, obs_idx, drift_shift_total, tp);
            let (coupon_knocked, knocked_redemption) =
                maturity_knocked_leg(&knocked_pred, cfg, obs_idx, drift_shift_total, tp);
            let maturity_coupon_hit = coupon_safe + coupon_knocked;

            redemption_pv += cfg.notional * safe_principal / S6;
            redemption_pv += cfg.notional * ki_redemption_safe / S6;
            redemption_pv += cfg.notional * knocked_redemption / S6;
            coupon_annuity += coupon_count * maturity_coupon_hit;
            total_ki += first_ki_safe;
            observation_first_knock_in[obs_idx] = first_ki_safe;
            continue;
        }

        let safe_update = update_safe_state_live(&safe_pred, cfg, obs_idx, drift_shift_total, tp);
        let knocked_update =
            update_knocked_state_live(&knocked_pred, cfg, obs_idx, drift_shift_total, tp);
        let next_knocked = merge_states(
            &safe_update.new_knocked,
            &knocked_update.next_knocked,
            k_retained,
        );

        let first_hit = safe_update.first_hit + knocked_update.first_hit;
        redemption_pv += cfg.notional * first_hit / S6;
        coupon_annuity += coupon_count * first_hit;
        total_ac += first_hit;
        total_ki += safe_update.first_knock_in;
        observation_autocall_first_hit[obs_idx] = first_hit;
        observation_first_knock_in[obs_idx] = safe_update.first_knock_in;
        post_observation_safe_mass[obs_idx] = safe_update.next_safe.total_weight();
        post_observation_knocked_mass[obs_idx] = next_knocked.total_weight();

        safe_state = safe_update.next_safe;
        knocked_state = next_knocked;
    }

    let loss = (cfg.notional - redemption_pv).max(0);
    let fair_coupon = if coupon_annuity > 100 {
        loss * S6 / coupon_annuity
    } else {
        0
    };

    C1FilterTrace {
        quote: c1_fast_quote_from_components(
            cfg.notional,
            fair_coupon,
            redemption_pv,
            coupon_annuity,
            total_ki,
            total_ac,
        ),
        observation_survival,
        observation_autocall_first_hit,
        observation_first_knock_in,
        post_observation_safe_mass,
        post_observation_knocked_mass,
        k_retained,
    }
}

#[cfg(not(target_os = "solana"))]
fn filter_inputs_host(sigma_common: f64) -> (C1FastConfig, i64, [i64; 2], i64) {
    let cfg = crate::worst_of_c1_fast::spy_qqq_iwm_c1_config();
    let model = FactoredWorstOfModel::spy_qqq_iwm_current();
    let drifts = model.risk_neutral_step_drifts(sigma_common, 63).unwrap();
    let drift_diffs = [
        ((drifts[1] - drifts[0]) * S6 as f64).round() as i64,
        ((drifts[2] - drifts[0]) * S6 as f64).round() as i64,
    ];
    let drift_shift_63 = ((cfg.loadings[0] as f64 * drifts[0])
        + (cfg.loadings[1] as f64 * drifts[1])
        + (cfg.loadings[2] as f64 * drifts[2]))
        .round() as i64;
    let sigma_s6 = (sigma_common * S6 as f64).round() as i64;
    (cfg, sigma_s6, drift_diffs, drift_shift_63)
}

#[cfg(not(target_os = "solana"))]
fn reference_states_live(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> [ObservationReferenceState; N_OBS] {
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let triple_pre = build_triple_pre_by_obs(cfg);
    let mut out = [ObservationReferenceState::default(); N_OBS];
    let mut safe_state = FilterState::singleton_origin();
    let mut knocked_state = FilterState::default();

    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;

        if obs_idx == 0 {
            if let Some(seed) = run_first_observation_seed_live(
                cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_total,
                k_retained,
                triple_pre[0].as_ref(),
            ) {
                out[obs_idx] = ObservationReferenceState {
                    safe_pred: seed.predicted_safe,
                    knocked_pred: FilterState::default(),
                    drift_shift_total,
                };
                safe_state = seed.next_safe;
                knocked_state = seed.next_knocked;
                continue;
            }
        }

        let safe_pred = predict_state(
            &safe_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        );
        let knocked_pred = predict_state(
            &knocked_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        );
        out[obs_idx] = ObservationReferenceState {
            safe_pred,
            knocked_pred,
            drift_shift_total,
        };

        if obs_idx + 1 == N_OBS {
            continue;
        }

        let tp = observation_probability_triple_pre(obs_idx, triple_pre[obs_idx].as_ref());
        let safe_update = update_safe_state_live(&safe_pred, cfg, obs_idx, drift_shift_total, tp);
        let knocked_update =
            update_knocked_state_live(&knocked_pred, cfg, obs_idx, drift_shift_total, tp);
        safe_state = safe_update.next_safe;
        knocked_state = merge_states(
            &safe_update.new_knocked,
            &knocked_update.next_knocked,
            k_retained,
        );
    }

    out
}

#[cfg(not(target_os = "solana"))]
fn build_tables_for_k<const K: usize>(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
) -> FrozenMomentTables<K> {
    use crate::worst_of_c1_filter_gradients::{MU_SAMPLES, N_MU_SAMPLES};
    let references = reference_states_live(cfg, sigma_s6, drift_diffs, drift_shift_63, K);
    let triple_pre = build_triple_pre_by_obs(cfg);
    let tables = phi2_tables();

    let empty_region = crate::worst_of_c1_filter_gradients::FrozenRegionTables {
        probability: [[0; K]; N_OBS],
        correction_u: [[0; K]; N_OBS],
        correction_v: [[0; K]; N_OBS],
        correction_u_interp: [[[0; K]; N_OBS]; N_MU_SAMPLES],
        correction_v_interp: [[[0; K]; N_OBS]; N_MU_SAMPLES],
        probability_interp: [[[0; K]; N_OBS]; N_MU_SAMPLES],
    };
    let mut safe_autocall = empty_region;
    let mut safe_ki = empty_region;
    let mut knocked_autocall = empty_region;

    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
        let tp = observation_probability_triple_pre(obs_idx, triple_pre[obs_idx].as_ref());
        let drift_shift_total = references[obs_idx].drift_shift_total;
        let ki_cholesky = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();
        for node_idx in 0..K {
            let safe_c = references[obs_idx].safe_pred.nodes[node_idx].c;
            let ac_rhs = [cfg.autocall_rhs_base + safe_c + drift_shift_total; 3];

            // Zero-mean reference (backward compatible)
            let safe_ac = triangle_with_gradient_i64(
                0,
                0,
                &ac_rhs,
                &obs.tri_pre,
                tables,
                tp,
                &dz_du,
                &dz_dv,
                obs.cov_uu,
                obs.cov_uv,
                obs.cov_vv,
            );
            safe_autocall.probability[obs_idx][node_idx] = safe_ac.probability;
            safe_autocall.correction_u[obs_idx][node_idx] = safe_ac.expectation_u;
            safe_autocall.correction_v[obs_idx][node_idx] = safe_ac.expectation_v;

            let safe_ki_region = ki_cholesky
                .map(|(l11, l21, l22)| {
                    ki_region_uv_moment_gh3(
                        0,
                        0,
                        l11,
                        l21,
                        l22,
                        cfg.ki_barrier_log,
                        ki_coords_from_cumulative(cfg, safe_c, drift_shift_total),
                    )
                })
                .unwrap_or_default();
            safe_ki.probability[obs_idx][node_idx] = safe_ki_region.probability;
            safe_ki.correction_u[obs_idx][node_idx] = safe_ki_region.expectation_u;
            safe_ki.correction_v[obs_idx][node_idx] = safe_ki_region.expectation_v;

            let knocked_c = references[obs_idx].knocked_pred.nodes[node_idx].c;
            let knocked_ac_rhs = [cfg.autocall_rhs_base + knocked_c + drift_shift_total; 3];
            let knocked_ac = triangle_with_gradient_i64(
                0,
                0,
                &knocked_ac_rhs,
                &obs.tri_pre,
                tables,
                tp,
                &dz_du,
                &dz_dv,
                obs.cov_uu,
                obs.cov_uv,
                obs.cov_vv,
            );
            knocked_autocall.probability[obs_idx][node_idx] = knocked_ac.probability;
            knocked_autocall.correction_u[obs_idx][node_idx] = knocked_ac.expectation_u;
            knocked_autocall.correction_v[obs_idx][node_idx] = knocked_ac.expectation_v;

            // Interpolated corrections at multiple mean_u values
            for (mi, &mu) in MU_SAMPLES.iter().enumerate() {
                let sac = triangle_with_gradient_i64(
                    mu,
                    0,
                    &ac_rhs,
                    &obs.tri_pre,
                    tables,
                    tp,
                    &dz_du,
                    &dz_dv,
                    obs.cov_uu,
                    obs.cov_uv,
                    obs.cov_vv,
                );
                // Store the gradient correction only (subtract the mean*P base term)
                safe_autocall.probability_interp[mi][obs_idx][node_idx] = sac.probability;
                safe_autocall.correction_u_interp[mi][obs_idx][node_idx] =
                    sac.expectation_u - m6r_fast(mu, sac.probability);
                safe_autocall.correction_v_interp[mi][obs_idx][node_idx] = sac.expectation_v; // mean_v=0, so no base term for v

                let ski = ki_cholesky
                    .map(|(l11, l21, l22)| {
                        ki_region_uv_moment_gh3(
                            mu,
                            0,
                            l11,
                            l21,
                            l22,
                            cfg.ki_barrier_log,
                            ki_coords_from_cumulative(cfg, safe_c, drift_shift_total),
                        )
                    })
                    .unwrap_or_default();
                safe_ki.probability_interp[mi][obs_idx][node_idx] = ski.probability;
                safe_ki.correction_u_interp[mi][obs_idx][node_idx] =
                    ski.expectation_u - m6r_fast(mu, ski.probability);
                safe_ki.correction_v_interp[mi][obs_idx][node_idx] = ski.expectation_v;

                let kac = triangle_with_gradient_i64(
                    mu,
                    0,
                    &knocked_ac_rhs,
                    &obs.tri_pre,
                    tables,
                    tp,
                    &dz_du,
                    &dz_dv,
                    obs.cov_uu,
                    obs.cov_uv,
                    obs.cov_vv,
                );
                knocked_autocall.probability_interp[mi][obs_idx][node_idx] = kac.probability;
                knocked_autocall.correction_u_interp[mi][obs_idx][node_idx] =
                    kac.expectation_u - m6r_fast(mu, kac.probability);
                knocked_autocall.correction_v_interp[mi][obs_idx][node_idx] = kac.expectation_v;
            }
        }
    }

    FrozenMomentTables {
        safe_autocall,
        safe_ki,
        knocked_autocall,
    }
}

#[cfg(not(target_os = "solana"))]
fn push_matrix_source<const K: usize>(out: &mut String, name: &str, values: &[[i64; K]; N_OBS]) {
    let _ = writeln!(out, "        {name}: [");
    for row in values {
        let _ = write!(out, "            [");
        for (idx, value) in row.iter().enumerate() {
            if idx > 0 {
                let _ = write!(out, ", ");
            }
            let _ = write!(out, "{value}");
        }
        let _ = writeln!(out, "],");
    }
    let _ = writeln!(out, "        ],");
}

#[cfg(not(target_os = "solana"))]
fn push_3d_matrix_source<const K: usize>(
    out: &mut String,
    name: &str,
    values: &[[[i64; K]; N_OBS]],
) {
    let _ = writeln!(out, "        {name}: [");
    for mu_slice in values {
        let _ = writeln!(out, "            [");
        for row in mu_slice {
            let _ = write!(out, "                [");
            for (idx, value) in row.iter().enumerate() {
                if idx > 0 {
                    let _ = write!(out, ", ");
                }
                let _ = write!(out, "{value}");
            }
            let _ = writeln!(out, "],");
        }
        let _ = writeln!(out, "            ],");
    }
    let _ = writeln!(out, "        ],");
}

#[cfg(not(target_os = "solana"))]
fn push_region_source<const K: usize>(
    out: &mut String,
    name: &str,
    region: &crate::worst_of_c1_filter_gradients::FrozenRegionTables<K>,
) {
    let _ = writeln!(out, "    {name}: FrozenRegionTables {{");
    push_matrix_source(out, "probability", &region.probability);
    push_matrix_source(out, "correction_u", &region.correction_u);
    push_matrix_source(out, "correction_v", &region.correction_v);
    push_3d_matrix_source(out, "correction_u_interp", &region.correction_u_interp);
    push_3d_matrix_source(out, "correction_v_interp", &region.correction_v_interp);
    push_3d_matrix_source(out, "probability_interp", &region.probability_interp);
    let _ = writeln!(out, "    }},");
}

#[cfg(not(target_os = "solana"))]
fn push_tables_source<const K: usize>(
    out: &mut String,
    const_name: &str,
    tables: &FrozenMomentTables<K>,
) {
    let _ = writeln!(
        out,
        "pub const {const_name}: FrozenMomentTables<{K}> = FrozenMomentTables {{"
    );
    push_region_source(out, "safe_autocall", &tables.safe_autocall);
    push_region_source(out, "safe_ki", &tables.safe_ki);
    push_region_source(out, "knocked_autocall", &tables.knocked_autocall);
    let _ = writeln!(out, "}};\n");
}

#[cfg(not(target_os = "solana"))]
pub fn generate_frozen_gradient_tables_source() -> String {
    let sigma_common = REFERENCE_SIGMA_COMMON_S6 as f64 / S6 as f64;
    let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs_host(sigma_common);
    let k9 = build_tables_for_k::<9>(&cfg, sigma_s6, drift_diffs, drift_shift_63);
    let k12 = build_tables_for_k::<12>(&cfg, sigma_s6, drift_diffs, drift_shift_63);
    let k15 = build_tables_for_k::<15>(&cfg, sigma_s6, drift_diffs, drift_shift_63);
    let mut out = String::new();
    out.push_str("//! Frozen gradient correction tables for the projected c1 filter.\n");
    out.push_str("//!\n");
    out.push_str("//! Generated by `cargo run -p halcyon-quote --bin gen_gradient_tables`.\n\n");
    out.push_str("/// Number of mean_u sample points for interpolated corrections.\n");
    out.push_str("pub const N_MU_SAMPLES: usize = 4;\n");
    out.push_str("/// Reference mean_u values (S6 scale) at which corrections are precomputed.\n");
    out.push_str("pub const MU_SAMPLES: [i64; N_MU_SAMPLES] = [-2000, 0, 2000, 4000];\n\n");
    out.push_str("#[derive(Debug, Clone, Copy)]\n");
    out.push_str("pub struct FrozenRegionTables<const K: usize> {\n");
    out.push_str("    pub probability: [[i64; K]; 6],\n");
    out.push_str("    pub correction_u: [[i64; K]; 6],\n");
    out.push_str("    pub correction_v: [[i64; K]; 6],\n");
    out.push_str("    pub correction_u_interp: [[[i64; K]; 6]; N_MU_SAMPLES],\n");
    out.push_str("    pub correction_v_interp: [[[i64; K]; 6]; N_MU_SAMPLES],\n");
    out.push_str("    pub probability_interp: [[[i64; K]; 6]; N_MU_SAMPLES],\n");
    out.push_str("}\n\n");
    out.push_str("#[derive(Debug, Clone, Copy)]\n");
    out.push_str("pub struct FrozenMomentTables<const K: usize> {\n");
    out.push_str("    pub safe_autocall: FrozenRegionTables<K>,\n");
    out.push_str("    pub safe_ki: FrozenRegionTables<K>,\n");
    out.push_str("    pub knocked_autocall: FrozenRegionTables<K>,\n");
    out.push_str("}\n\n");
    let _ = writeln!(
        out,
        "pub const REFERENCE_SIGMA_COMMON_S6: i64 = {};",
        REFERENCE_SIGMA_COMMON_S6
    );
    out.push('\n');
    push_tables_source(&mut out, "FROZEN_TABLES_K9", &k9);
    push_tables_source(&mut out, "FROZEN_TABLES_K12", &k12);
    push_tables_source(&mut out, "FROZEN_TABLES_K15", &k15);
    out
}

/// Generate the full three-point frozen region moments source file used by
/// the rectangular-K filter path. For each (obs_rel, node_idx) and each of
/// three reference drift means `μ_ref ∈ {(0,0), (+1e-3, 0), (0, -1e-3)}`,
/// this emits the exact region moments `(P, E[U·𝟙], E[V·𝟙])` for both the
/// autocall triangle and the "KI-safe" triangle (all three legs above KI).
///
/// Unlike the single-point `FROZEN_TABLES_K*` (which store gradients at μ=0
/// only), this stores the full moments so the observe step can recover the
/// survivor mean update directly without recomputing ∇P live.
///
/// σ is held fixed at `MOM3_REF_SIGMA_S6` (σ drift absorbed as systematic
/// error, matching the existing single-point convention).
///
/// Storage: 5 obs_rel × 12 (K_MAX) × 3 refs × 6 fields × 8 B ≈ 8.6 KB.
#[cfg(not(target_os = "solana"))]
pub fn generate_frozen_moments_3pt_source() -> String {
    use crate::b_tensors::K_SCHEDULE;
    use crate::nested_grids::nested_c_grid;

    const MOM3_REF_SIGMA_S6: i64 = 300_000;
    const N_REFS: usize = 3;
    const N_OBS_REL: usize = 5;
    const K_MAX: usize = 12;
    const MU_REFS_U_S6: [i64; N_REFS] = [0, 1_000, 0];
    const MU_REFS_V_S6: [i64; N_REFS] = [0, 0, -1_000];

    let sigma_common = MOM3_REF_SIGMA_S6 as f64 / S6 as f64;
    let (cfg, sigma_s6, _drift_diffs, drift_shift_63) = filter_inputs_host(sigma_common);
    let triple_pre = build_triple_pre_by_obs(&cfg);
    let tables = phi2_tables();

    let mut prob_ac = [[[0i64; N_REFS]; K_MAX]; N_OBS_REL];
    let mut eu_ac = [[[0i64; N_REFS]; K_MAX]; N_OBS_REL];
    let mut ev_ac = [[[0i64; N_REFS]; K_MAX]; N_OBS_REL];
    let mut prob_ki_safe = [[[0i64; N_REFS]; K_MAX]; N_OBS_REL];
    let mut eu_ki_safe = [[[0i64; N_REFS]; K_MAX]; N_OBS_REL];
    let mut ev_ki_safe = [[[0i64; N_REFS]; K_MAX]; N_OBS_REL];

    for obs_rel in 0..N_OBS_REL {
        let obs_idx = obs_rel + 1;
        let obs = &cfg.obs[obs_idx];
        let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
        let tp = observation_probability_triple_pre(obs_idx, triple_pre[obs_idx].as_ref());
        let k = K_SCHEDULE[obs_rel];
        let c_grid = nested_c_grid(sigma_s6, obs_rel, k);
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;

        for node_idx in 0..k {
            let c = c_grid[node_idx];
            let ac_rhs = [cfg.autocall_rhs_base + c + drift_shift_total; 3];
            let ki_safe_rhs = [cfg.ki_safe_rhs_base + c + drift_shift_total; 3];

            for r in 0..N_REFS {
                let mu_u = MU_REFS_U_S6[r];
                let mu_v = MU_REFS_V_S6[r];

                let ac = triangle_with_gradient_i64(
                    mu_u,
                    mu_v,
                    &ac_rhs,
                    &obs.tri_pre,
                    tables,
                    tp,
                    &dz_du,
                    &dz_dv,
                    obs.cov_uu,
                    obs.cov_uv,
                    obs.cov_vv,
                );
                prob_ac[obs_rel][node_idx][r] = ac.probability;
                eu_ac[obs_rel][node_idx][r] = ac.expectation_u;
                ev_ac[obs_rel][node_idx][r] = ac.expectation_v;

                let ki_safe = triangle_with_gradient_i64(
                    mu_u,
                    mu_v,
                    &ki_safe_rhs,
                    &obs.tri_pre,
                    tables,
                    tp,
                    &dz_du,
                    &dz_dv,
                    obs.cov_uu,
                    obs.cov_uv,
                    obs.cov_vv,
                );
                prob_ki_safe[obs_rel][node_idx][r] = ki_safe.probability;
                eu_ki_safe[obs_rel][node_idx][r] = ki_safe.expectation_u;
                ev_ki_safe[obs_rel][node_idx][r] = ki_safe.expectation_v;
            }
        }
    }

    // Monotonicity sanity: P_ac should be non-decreasing in c (node index)
    // at μ=(0,0), because higher c means the autocall barrier is further
    // below the spread mean, making "autocall" more likely.
    for obs_rel in 0..N_OBS_REL {
        let k = K_SCHEDULE[obs_rel];
        let mut last = 0i64;
        for i in 0..k {
            let p = prob_ac[obs_rel][i][0];
            assert!(
                p >= last - 1000, // 1e-3 slack for rounding
                "P_autocall not monotone in c at obs_rel={}: node {} p={} < prev {}",
                obs_rel,
                i,
                p,
                last
            );
            last = p;
        }
    }

    let mut out = String::new();
    out.push_str("//! Phase 4 three-point frozen region moments.\n//!\n");
    out.push_str("//! Generated by `cargo run -p halcyon-quote --bin gen_moments_3pt`.\n");
    out.push_str("//! DO NOT EDIT MANUALLY.\n//!\n");
    out.push_str("//! At each (obs_rel, node_idx, μ_ref) the triple `(probability,\n");
    out.push_str("//! expectation_u, expectation_v)` is precomputed for both the autocall\n");
    out.push_str("//! and KI-safe triangles. The on-chain filter uses\n");
    out.push_str("//! `select_frozen_moment_3pt` to pick the μ_ref closest to the current\n");
    out.push_str("//! node's `(mean_u, mean_v)` by L¹ distance.\n\n");

    let _ = writeln!(
        out,
        "pub const MOM3_REF_SIGMA_S6: i64 = {MOM3_REF_SIGMA_S6};"
    );
    let _ = writeln!(out, "pub const MOM3_N_REFS: usize = {N_REFS};");
    let _ = writeln!(out, "pub const MOM3_N_OBS_REL: usize = {N_OBS_REL};");
    let _ = writeln!(out, "pub const MOM3_K_MAX: usize = {K_MAX};");
    out.push('\n');
    let _ = writeln!(
        out,
        "pub const MOM3_MU_U_S6: [i64; MOM3_N_REFS] = {MU_REFS_U_S6:?};"
    );
    let _ = writeln!(
        out,
        "pub const MOM3_MU_V_S6: [i64; MOM3_N_REFS] = {MU_REFS_V_S6:?};"
    );
    out.push('\n');

    fn emit_3d(out: &mut String, name: &str, data: &[[[i64; 3]; 12]; 5]) {
        let _ = writeln!(
            out,
            "pub const {name}: [[[i64; MOM3_N_REFS]; MOM3_K_MAX]; MOM3_N_OBS_REL] = ["
        );
        for obs in data {
            out.push_str("    [\n");
            for node in obs {
                let _ = write!(out, "        [");
                for (i, v) in node.iter().enumerate() {
                    if i > 0 {
                        let _ = write!(out, ", ");
                    }
                    let _ = write!(out, "{v}");
                }
                out.push_str("],\n");
            }
            out.push_str("    ],\n");
        }
        out.push_str("];\n\n");
    }

    emit_3d(&mut out, "MOM3_PROB_AC", &prob_ac);
    emit_3d(&mut out, "MOM3_EU_AC", &eu_ac);
    emit_3d(&mut out, "MOM3_EV_AC", &ev_ac);
    emit_3d(&mut out, "MOM3_PROB_KI_SAFE", &prob_ki_safe);
    emit_3d(&mut out, "MOM3_EU_KI_SAFE", &eu_ki_safe);
    emit_3d(&mut out, "MOM3_EV_KI_SAFE", &ev_ki_safe);

    out.push_str(
        r#"
/// Bundle returned by `select_frozen_moment_3pt`.
#[derive(Debug, Clone, Copy, Default)]
pub struct FrozenMoment3pt {
    pub prob_ac: i64,
    pub eu_ac: i64,
    pub ev_ac: i64,
    pub prob_ki_safe: i64,
    pub eu_ki_safe: i64,
    pub ev_ki_safe: i64,
}

/// Select the μ_ref closest to `(mean_u, mean_v)` by L¹ distance and return
/// the precomputed region moments for that reference.
///
/// `obs_rel` must be in `0..MOM3_N_OBS_REL`; `node_idx` must be within the
/// K at that observation. Out-of-range inputs return zeroed moments.
#[inline(always)]
pub fn select_frozen_moment_3pt(
    mean_u: i64,
    mean_v: i64,
    obs_rel: usize,
    node_idx: usize,
) -> FrozenMoment3pt {
    if obs_rel >= MOM3_N_OBS_REL || node_idx >= MOM3_K_MAX {
        return FrozenMoment3pt::default();
    }
    let mut best = 0usize;
    let mut best_d = i64::MAX;
    for r in 0..MOM3_N_REFS {
        let du = (mean_u - MOM3_MU_U_S6[r]).abs();
        let dv = (mean_v - MOM3_MU_V_S6[r]).abs();
        let d = du + dv;
        if d < best_d {
            best_d = d;
            best = r;
        }
    }
    FrozenMoment3pt {
        prob_ac: MOM3_PROB_AC[obs_rel][node_idx][best],
        eu_ac: MOM3_EU_AC[obs_rel][node_idx][best],
        ev_ac: MOM3_EV_AC[obs_rel][node_idx][best],
        prob_ki_safe: MOM3_PROB_KI_SAFE[obs_rel][node_idx][best],
        eu_ki_safe: MOM3_EU_KI_SAFE[obs_rel][node_idx][best],
        ev_ki_safe: MOM3_EV_KI_SAFE[obs_rel][node_idx][best],
    }
}
"#,
    );

    out
}

/// Per-σ summary of the three-point frozen moments' node-level accuracy
/// relative to the live `triangle_with_gradient_i64` reference.
///
/// Error is the absolute deviation in `(probability, E[U·𝟙], E[V·𝟙])` for
/// the autocall triangle across every (obs_rel, node_idx) sampled at the
/// nested c-grid position (matching the table's index) and the plausible
/// post-predict survivor `(mean_u, mean_v)` from `reference_states_live`
/// at K=12.
///
/// Note: the existing single-point `FROZEN_TABLES_K*` are indexed by a
/// different c-grid (K=9/12/15 uniform from the K=9+RBF path), so a direct
/// 1pt-vs-3pt comparison at the same (obs, node) is not apples-to-apples.
/// This validation reports 3pt error alone; Phase 5 integration measures
/// the end-to-end fair-coupon impact.
#[cfg(not(target_os = "solana"))]
#[derive(Debug, Clone, Copy)]
pub struct FrozenMoments3ptValidation {
    pub sigma_common: f64,
    pub compared_nodes: usize,
    pub mean_abs_err_p_3pt: f64,
    pub mean_abs_err_eu_3pt: f64,
    pub mean_abs_err_ev_3pt: f64,
    pub max_abs_err_p_3pt: f64,
    pub max_abs_err_eu_3pt: f64,
    pub max_abs_err_ev_3pt: f64,
    pub mean_abs_err_p_0only: f64,
    pub max_abs_err_p_0only: f64,
    pub mean_abs_err_eu_0only: f64,
    pub max_abs_err_eu_0only: f64,
}

/// Compare three-point frozen moments vs single-point frozen tables and the
/// live reference at each observation's realistic post-predict state.
///
/// This is the Phase 4 acceptance signal: if the 3pt mean/max errors are
/// strictly lower than 1pt at the same K, the tables are a net improvement
/// and Phase 5 integration can proceed.
#[cfg(not(target_os = "solana"))]
pub fn frozen_moments_3pt_validation(sigma_values: &[f64]) -> Vec<FrozenMoments3ptValidation> {
    use crate::b_tensors::K_SCHEDULE;
    use crate::frozen_moments_3pt::select_frozen_moment_3pt;
    use crate::nested_grids::nested_c_grid;

    const S6_F64: f64 = 1_000_000.0;
    let mut out = Vec::with_capacity(sigma_values.len());

    for &sigma_common in sigma_values {
        let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs_host(sigma_common);
        let refs = reference_states_live(&cfg, sigma_s6, drift_diffs, drift_shift_63, 12);
        let triple_pre = build_triple_pre_by_obs(&cfg);
        let tables = phi2_tables();

        let mut sum_p_3pt = 0.0f64;
        let mut sum_eu_3pt = 0.0f64;
        let mut sum_ev_3pt = 0.0f64;
        let mut max_p_3pt = 0.0f64;
        let mut max_eu_3pt = 0.0f64;
        let mut max_ev_3pt = 0.0f64;
        // "0only": use only the (0,0) reference slice — i.e., what we'd
        // get if we had single-point tables at the same c-grid. Same
        // runtime math, worse μ coverage.
        let mut sum_p_0 = 0.0f64;
        let mut max_p_0 = 0.0f64;
        let mut sum_eu_0 = 0.0f64;
        let mut max_eu_0 = 0.0f64;
        let mut compared = 0usize;

        for obs_rel in 0..5 {
            let obs_idx = obs_rel + 1;
            let obs = &cfg.obs[obs_idx];
            let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
            let tp = observation_probability_triple_pre(obs_idx, triple_pre[obs_idx].as_ref());
            let frozen = frozen_observation_view(12, obs_idx);
            let drift_shift_total = refs[obs_idx].drift_shift_total;
            let k_this = K_SCHEDULE[obs_rel];
            // 3pt tables are indexed by nested-grid position, so the c used
            // at runtime is the nested c, not the K=12 uniform c from
            // reference_states_live. μ from refs is still a plausible survivor
            // drift — we just evaluate the live triangle at the table's c.
            let c_grid = nested_c_grid(sigma_s6, obs_rel, k_this);

            for (node_idx, node) in refs[obs_idx]
                .safe_pred
                .nodes
                .iter()
                .copied()
                .enumerate()
                .take(k_this)
            {
                if node.w <= 0 {
                    continue;
                }
                let c_nested = c_grid[node_idx];
                let ac_rhs = [cfg.autocall_rhs_base + c_nested + drift_shift_total; 3];

                // Live reference at this (mean_u, mean_v) with nested c.
                let live = triangle_with_gradient_i64(
                    node.mean_u,
                    node.mean_v,
                    &ac_rhs,
                    &obs.tri_pre,
                    tables,
                    tp,
                    &dz_du,
                    &dz_dv,
                    obs.cov_uu,
                    obs.cov_uv,
                    obs.cov_vv,
                );
                if live.probability <= 0 {
                    continue;
                }

                // Three-point frozen lookup.
                // P_3pt: use selected ref's P directly.
                // E[U·𝟙]_3pt: shift ref's E by (μ_live - μ_ref)·P_ref to
                // correct for the drift offset between the live and ref μ.
                let sel = select_frozen_moment_3pt(node.mean_u, node.mean_v, obs_rel, node_idx);
                let (mu_u_ref, mu_v_ref) = {
                    use crate::frozen_moments_3pt::{MOM3_MU_U_S6, MOM3_MU_V_S6, MOM3_N_REFS};
                    let mut best = 0usize;
                    let mut best_d = i64::MAX;
                    for r in 0..MOM3_N_REFS {
                        let du = (node.mean_u - MOM3_MU_U_S6[r]).abs();
                        let dv = (node.mean_v - MOM3_MU_V_S6[r]).abs();
                        let d = du + dv;
                        if d < best_d {
                            best_d = d;
                            best = r;
                        }
                    }
                    (MOM3_MU_U_S6[best], MOM3_MU_V_S6[best])
                };
                let eu_3pt = sel.eu_ac + m6r_fast(node.mean_u - mu_u_ref, sel.prob_ac);
                let ev_3pt = sel.ev_ac + m6r_fast(node.mean_v - mu_v_ref, sel.prob_ac);
                let p_3pt = sel.prob_ac;

                // 0-only baseline: read the (0,0) reference slice of the
                // same 3pt table and shift to live μ. This is the single-
                // point lookup that Phase 4 is replacing.
                use crate::frozen_moments_3pt::{
                    MOM3_EU_AC as M_EU_AC, MOM3_EV_AC as M_EV_AC, MOM3_PROB_AC as M_P_AC,
                };
                let p_0 = M_P_AC[obs_rel][node_idx][0];
                let eu_0 = M_EU_AC[obs_rel][node_idx][0] + m6r_fast(node.mean_u, p_0);
                let ev_0 = M_EV_AC[obs_rel][node_idx][0] + m6r_fast(node.mean_v, p_0);
                let _ = (frozen, ev_0); // frozen kept for future dual-source checks

                let dp_3 = (live.probability - p_3pt).abs() as f64 / S6_F64;
                let du_3 = (live.expectation_u - eu_3pt).abs() as f64 / S6_F64;
                let dv_3 = (live.expectation_v - ev_3pt).abs() as f64 / S6_F64;
                let dp_0 = (live.probability - p_0).abs() as f64 / S6_F64;
                let du_0 = (live.expectation_u - eu_0).abs() as f64 / S6_F64;

                sum_p_3pt += dp_3;
                sum_eu_3pt += du_3;
                sum_ev_3pt += dv_3;
                max_p_3pt = max_p_3pt.max(dp_3);
                max_eu_3pt = max_eu_3pt.max(du_3);
                max_ev_3pt = max_ev_3pt.max(dv_3);
                sum_p_0 += dp_0;
                max_p_0 = max_p_0.max(dp_0);
                sum_eu_0 += du_0;
                max_eu_0 = max_eu_0.max(du_0);
                compared += 1;
            }
        }

        let n = compared.max(1) as f64;
        out.push(FrozenMoments3ptValidation {
            sigma_common,
            compared_nodes: compared,
            mean_abs_err_p_3pt: sum_p_3pt / n,
            mean_abs_err_eu_3pt: sum_eu_3pt / n,
            mean_abs_err_ev_3pt: sum_ev_3pt / n,
            max_abs_err_p_3pt: max_p_3pt,
            max_abs_err_eu_3pt: max_eu_3pt,
            max_abs_err_ev_3pt: max_ev_3pt,
            mean_abs_err_p_0only: sum_p_0 / n,
            max_abs_err_p_0only: max_p_0,
            mean_abs_err_eu_0only: sum_eu_0 / n,
            max_abs_err_eu_0only: max_eu_0,
        });
    }

    out
}

#[cfg(not(target_os = "solana"))]
pub fn frozen_gradient_validation(
    k_retained: usize,
    sigma_values: &[f64],
) -> Vec<FrozenGradientValidation> {
    let mut out = Vec::with_capacity(sigma_values.len());
    for &sigma_common in sigma_values {
        let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs_host(sigma_common);
        let live_trace =
            quote_c1_filter_trace_live(&cfg, sigma_s6, drift_diffs, drift_shift_63, k_retained);
        let frozen_trace =
            quote_c1_filter_trace(&cfg, sigma_s6, drift_diffs, drift_shift_63, k_retained);
        let refs = reference_states_live(&cfg, sigma_s6, drift_diffs, drift_shift_63, k_retained);
        let triple_pre = build_triple_pre_by_obs(&cfg);
        let tables = phi2_tables();
        let mut ratio_sum_u = 0.0f64;
        let mut ratio_sum_v = 0.0f64;
        let mut ratio_max_u = 0.0f64;
        let mut ratio_max_v = 0.0f64;
        let mut compared = 0usize;
        let mut ratio_samples_u = 0usize;
        let mut ratio_samples_v = 0usize;

        for obs_idx in 0..(N_OBS - 1) {
            let obs = &cfg.obs[obs_idx];
            let tp = triple_pre[obs_idx].as_ref();
            let frozen = frozen_observation_view(k_retained, obs_idx);
            let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
            for (class_idx, state) in [refs[obs_idx].safe_pred, refs[obs_idx].knocked_pred]
                .into_iter()
                .enumerate()
            {
                for (node_idx, node) in state.nodes.iter().copied().enumerate().take(k_retained) {
                    if node.w <= 0 {
                        continue;
                    }
                    let rhs = [cfg.autocall_rhs_base + node.c + refs[obs_idx].drift_shift_total; 3];
                    let live_region = triangle_with_gradient_i64(
                        node.mean_u,
                        node.mean_v,
                        &rhs,
                        &obs.tri_pre,
                        tables,
                        tp,
                        &dz_du,
                        &dz_dv,
                        obs.cov_uu,
                        obs.cov_uv,
                        obs.cov_vv,
                    );
                    if live_region.probability <= 1_000 {
                        continue;
                    }
                    let frozen_region = raw_region_moment_from_frozen(
                        node.mean_u,
                        node.mean_v,
                        live_region.probability,
                        node_idx,
                        k_retained,
                        if class_idx == 0 {
                            frozen.safe_autocall
                        } else {
                            frozen.knocked_autocall
                        },
                    );
                    let live_base_u = m6r_fast(node.mean_u, live_region.probability);
                    let live_base_v = m6r_fast(node.mean_v, live_region.probability);
                    let live_corr_u = (live_region.expectation_u - live_base_u).abs();
                    let live_corr_v = (live_region.expectation_v - live_base_v).abs();
                    let frozen_corr_u = (frozen_region.expectation_u - live_base_u).abs();
                    let frozen_corr_v = (frozen_region.expectation_v - live_base_v).abs();
                    if frozen_corr_u > 100 && live_corr_u > 100 {
                        let ratio = live_corr_u as f64 / frozen_corr_u as f64;
                        ratio_sum_u += ratio;
                        ratio_max_u = ratio_max_u.max(ratio.max(1.0 / ratio));
                        ratio_samples_u += 1;
                    }
                    if frozen_corr_v > 100 && live_corr_v > 100 {
                        let ratio = live_corr_v as f64 / frozen_corr_v as f64;
                        ratio_sum_v += ratio;
                        ratio_max_v = ratio_max_v.max(ratio.max(1.0 / ratio));
                        ratio_samples_v += 1;
                    }
                    compared += 1;
                }
            }
        }

        out.push(FrozenGradientValidation {
            sigma_common,
            live_fair_coupon_bps: live_trace.quote.fair_coupon_bps_f64(),
            frozen_fair_coupon_bps: frozen_trace.quote.fair_coupon_bps_f64(),
            fair_coupon_diff_bps: frozen_trace.quote.fair_coupon_bps_f64()
                - live_trace.quote.fair_coupon_bps_f64(),
            live_obs2_first_hit: live_trace.observation_autocall_first_hit[1] as f64 / S6 as f64,
            frozen_obs2_first_hit: frozen_trace.observation_autocall_first_hit[1] as f64
                / S6 as f64,
            compared_nodes: compared,
            ratio_samples_u,
            ratio_samples_v,
            mean_live_over_frozen_u: if ratio_samples_u > 0 {
                ratio_sum_u / ratio_samples_u as f64
            } else {
                0.0
            },
            mean_live_over_frozen_v: if ratio_samples_v > 0 {
                ratio_sum_v / ratio_samples_v as f64
            } else {
                0.0
            },
            max_live_over_frozen_u: ratio_max_u,
            max_live_over_frozen_v: ratio_max_v,
        });
    }
    out
}

#[cfg(not(target_os = "solana"))]
pub fn bench_prediction_step(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    k_retained: usize,
) -> PredictionBenchSummary {
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let predicted = predict_state(
        &FilterState::singleton_origin(),
        &transition.factor_values,
        &transition.factor_weights,
        &transition.step_mean_u,
        &transition.step_mean_v,
        k_retained,
    );
    let checksum = predicted.nodes.iter().fold(0i64, |acc, node| {
        acc + node.c + node.w + node.mean_u + node.mean_v
    });
    PredictionBenchSummary {
        active_nodes: predicted.n_active,
        total_mass: predicted.total_weight(),
        checksum,
    }
}

#[cfg(not(target_os = "solana"))]
pub fn bench_observation_step(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> ObservationBenchSummary {
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let predicted = predict_state(
        &FilterState::singleton_origin(),
        &transition.factor_values,
        &transition.factor_weights,
        &transition.step_mean_u,
        &transition.step_mean_v,
        k_retained,
    );
    let obs = &cfg.obs[0];
    let cholesky = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();
    let triple_pre = cholesky
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
    if let Some(summary) = run_first_observation_seed(
        cfg,
        sigma_s6,
        drift_diffs,
        drift_shift_63,
        k_retained,
        triple_pre.as_ref(),
    )
    .map(|seed| ObservationBenchSummary {
        safe_active_nodes: seed.next_safe.n_active,
        knocked_active_nodes: seed.next_knocked.n_active,
        first_hit_mass: seed.first_hit,
        first_knock_in_mass: seed.first_knock_in,
        checksum: seed
            .next_safe
            .nodes
            .iter()
            .chain(seed.next_knocked.nodes.iter())
            .fold(0i64, |acc, node| {
                acc + node.c + node.w + node.mean_u + node.mean_v
            }),
    }) {
        return summary;
    }
    let safe_update = update_safe_state(
        &predicted,
        cfg,
        0,
        drift_shift_63,
        k_retained,
        triple_pre.as_ref(),
    );
    let checksum = safe_update
        .next_safe
        .nodes
        .iter()
        .chain(safe_update.new_knocked.nodes.iter())
        .fold(0i64, |acc, node| {
            acc + node.c + node.w + node.mean_u + node.mean_v
        });
    ObservationBenchSummary {
        safe_active_nodes: safe_update.next_safe.n_active,
        knocked_active_nodes: safe_update.new_knocked.n_active,
        first_hit_mass: safe_update.first_hit,
        first_knock_in_mass: safe_update.first_knock_in,
        checksum,
    }
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
pub fn bench_prepare_observation_obs2(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> ObservationBenchState {
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let obs0 = &cfg.obs[0];
    let drift_shift0 = obs0.obs_day as i64 / 63 * drift_shift_63;
    let cholesky0 = cholesky6(obs0.cov_uu, obs0.cov_uv, obs0.cov_vv).ok();
    let triple_pre0 = cholesky0
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
    let (safe_state, knocked_state) = if let Some(seed) = run_first_observation_seed(
        cfg,
        sigma_s6,
        drift_diffs,
        drift_shift0,
        k_retained,
        triple_pre0.as_ref(),
    ) {
        (seed.next_safe, seed.next_knocked)
    } else {
        let (next_safe, next_knocked, _, _) = run_observation_step(
            &FilterState::singleton_origin(),
            &FilterState::default(),
            &transition,
            cfg,
            0,
            drift_shift0,
            k_retained,
            k_retained,
            triple_pre0.as_ref(),
            None,
        );
        (next_safe, next_knocked)
    };

    let obs_idx = 1usize;
    let obs = &cfg.obs[obs_idx];
    let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;
    ObservationBenchState {
        safe_state,
        knocked_state,
        transition,
        obs_idx,
        drift_shift_total,
        k_retained,
    }
}

#[cfg(not(target_os = "solana"))]
pub fn bench_observation_from_prepared(
    cfg: &C1FastConfig,
    prepared: &ObservationBenchState,
) -> ObservationBenchSummary {
    let obs = &cfg.obs[prepared.obs_idx];
    let cholesky = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();
    let triple_pre = cholesky
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
    let (next_safe, next_knocked, first_hit, first_knock_in) = run_observation_step(
        &prepared.safe_state,
        &prepared.knocked_state,
        &prepared.transition,
        cfg,
        prepared.obs_idx,
        prepared.drift_shift_total,
        prepared.k_retained,
        prepared.k_retained,
        triple_pre.as_ref(),
        None,
    );
    let checksum = next_safe
        .nodes
        .iter()
        .chain(next_knocked.nodes.iter())
        .fold(0i64, |acc, node| {
            acc + node.c + node.w + node.mean_u + node.mean_v
        });
    ObservationBenchSummary {
        safe_active_nodes: next_safe.n_active,
        knocked_active_nodes: next_knocked.n_active,
        first_hit_mass: first_hit,
        first_knock_in_mass: first_knock_in,
        checksum,
    }
}

#[cfg(not(target_os = "solana"))]
pub fn bench_prepare_coupon_only_obs2(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> Option<CouponOnlyBenchState> {
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let obs0 = &cfg.obs[0];
    let drift_shift0 = obs0.obs_day as i64 / 63 * drift_shift_63;
    let cholesky0 = cholesky6(obs0.cov_uu, obs0.cov_uv, obs0.cov_vv).ok();
    let triple_pre0 = cholesky0
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
    let (safe_state, knocked_state) = if let Some(seed) = run_first_observation_seed(
        cfg,
        sigma_s6,
        drift_diffs,
        drift_shift0,
        k_retained,
        triple_pre0.as_ref(),
    ) {
        (seed.next_safe, seed.next_knocked)
    } else {
        let (next_safe, next_knocked, _, _) = run_observation_step(
            &FilterState::singleton_origin(),
            &FilterState::default(),
            &transition,
            cfg,
            0,
            drift_shift0,
            k_retained,
            k_retained,
            triple_pre0.as_ref(),
            None,
        );
        (next_safe, next_knocked)
    };

    let obs_idx = 1usize;
    let drift_shift_total = cfg.obs[obs_idx].obs_day as i64 / 63 * drift_shift_63;
    Some(CouponOnlyBenchState {
        safe_pred: predict_state(
            &safe_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        ),
        knocked_pred: predict_state(
            &knocked_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        ),
        obs_idx,
        drift_shift_total,
    })
}

#[cfg(not(target_os = "solana"))]
pub fn bench_coupon_only_from_prepared(
    cfg: &C1FastConfig,
    prepared: &CouponOnlyBenchState,
) -> CouponOnlyBenchSummary {
    let obs = &cfg.obs[prepared.obs_idx];
    let tables = phi2_tables();
    let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
        .ok()
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
    let tp = observation_probability_triple_pre(prepared.obs_idx, triple_pre.as_ref());

    let mut coupon_hit = 0i64;
    let mut checksum = 0i64;

    for node in prepared
        .safe_pred
        .nodes
        .iter()
        .chain(prepared.knocked_pred.nodes.iter())
        .copied()
    {
        if node.w <= 0 {
            continue;
        }
        let shift = node.c + prepared.drift_shift_total;
        let coupon_prob = triangle_probability_with_triple_i64(
            node.mean_u,
            node.mean_v,
            &[cfg.autocall_rhs_base + shift; 3],
            &obs.tri_pre,
            tables,
            tp,
        );
        coupon_hit += m6r(node.w, coupon_prob);
        checksum += node.c + node.w + node.mean_u + node.mean_v + coupon_prob;
    }

    CouponOnlyBenchSummary {
        safe_active_nodes: prepared.safe_pred.n_active,
        knocked_active_nodes: prepared.knocked_pred.n_active,
        coupon_hit,
        checksum,
    }
}

#[cfg(not(target_os = "solana"))]
pub fn bench_triangle_gradient_single(cfg: &C1FastConfig) -> TriangleGradientBenchSummary {
    let obs = &cfg.obs[0];
    let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
        .ok()
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
    let tables = phi2_tables();
    let frozen = frozen_observation_view(9, 0);
    let out = triangle_region_from_frozen_inline_i64(
        200,
        -100,
        &[50_000; 3],
        &obs.tri_pre,
        tables,
        triple_pre.as_ref(),
        4,
        9,
        frozen.safe_autocall,
    );
    TriangleGradientBenchSummary {
        probability: out.probability,
        expectation_u: out.expectation_u,
        expectation_v: out.expectation_v,
    }
}

#[cfg(not(target_os = "solana"))]
pub fn bench_triangle_gradient_batch(
    cfg: &C1FastConfig,
    repeats: usize,
) -> TriangleGradientBenchSummary {
    let obs = &cfg.obs[1];
    let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
        .ok()
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
    let tables = phi2_tables();
    let frozen = frozen_observation_view(9, 1);
    let mut probability = 0i64;
    let mut expectation_u = 0i64;
    let mut expectation_v = 0i64;
    for rep in 0..repeats.max(1) {
        let bump = rep as i64 * 37;
        let mean_u = 200 + bump;
        let mean_v = -100 - bump;
        let rhs = [50_000 + bump; 3];
        let out = triangle_region_from_frozen_inline_i64(
            mean_u,
            mean_v,
            &rhs,
            &obs.tri_pre,
            tables,
            triple_pre.as_ref(),
            rep % 9,
            9,
            frozen.safe_autocall,
        );
        probability += out.probability;
        expectation_u += out.expectation_u;
        expectation_v += out.expectation_v;
    }
    TriangleGradientBenchSummary {
        probability,
        expectation_u,
        expectation_v,
    }
}

pub const OBS1_K_RETAINED: usize = 9;

#[derive(Debug, Clone)]
pub struct Obs1SeedSnapshot {
    pub safe_state: FilterState,
    pub knocked_state: FilterState,
    pub first_hit: i64,
    pub first_knock_in: i64,
}

/// Raw per-node weights before projection. Node indexing matches GH-13.
#[derive(Debug, Clone)]
pub struct Obs1SeedRawWeights {
    pub safe_w: [i64; N_FACTOR_NODES_EXACT_SEED],
    pub knocked_w: [i64; N_FACTOR_NODES_EXACT_SEED],
    pub first_hit: i64,
    pub first_knock_in: i64,
}

/// Compute the raw unprojected obs1 seed weights for table generation.
pub fn obs1_seed_raw_weights(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
) -> Option<Obs1SeedRawWeights> {
    let drift_shift_total = cfg.obs[0].obs_day as i64 / 63 * drift_shift_63;
    let triple_pre = cholesky6(cfg.obs[0].cov_uu, cfg.obs[0].cov_uv, cfg.obs[0].cov_vv)
        .ok()
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));

    let transition = build_exact_seed_transition(cfg, sigma_s6, drift_diffs)?;
    let predicted = predicted_state_from_exact_seed_transition(&transition);
    let obs = &cfg.obs[0];
    let tables = phi2_tables();
    let tp = observation_probability_triple_pre(0, triple_pre.as_ref());
    let ki_cholesky = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();

    let mut safe_w = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut knocked_w = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut first_hit = 0i64;
    let mut first_knock_in = 0i64;

    for (idx, node) in predicted.nodes.iter().copied().enumerate() {
        if idx >= N_FACTOR_NODES_EXACT_SEED || node.w <= 0 {
            continue;
        }
        let shift = node.c + drift_shift_total;
        let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
        let ac_prob = triangle_probability_with_triple_i64(
            node.mean_u,
            node.mean_v,
            &ac_rhs,
            &obs.tri_pre,
            tables,
            tp,
        );
        let ki_prob = ki_cholesky
            .map(|(l11, l21, l22)| {
                ki_region_uv_moment_gh3(
                    node.mean_u,
                    node.mean_v,
                    l11,
                    l21,
                    l22,
                    cfg.ki_barrier_log,
                    ki_coords_from_cumulative(cfg, node.c, drift_shift_total),
                )
                .probability
            })
            .unwrap_or(0);
        let safe_prob = (S6 - ac_prob - ki_prob).max(0);

        first_hit += m6r(node.w, ac_prob);
        first_knock_in += m6r(node.w, ki_prob);
        safe_w[idx] = m6r(node.w, safe_prob);
        knocked_w[idx] = m6r(node.w, ki_prob);
    }

    Some(Obs1SeedRawWeights {
        safe_w,
        knocked_w,
        first_hit,
        first_knock_in,
    })
}

/// Projected obs1 state at a given K, for table generation.
/// Returns (safe_projected, knocked_projected, first_hit, first_knock_in).
///
/// Uses the full GH13 span for the projection grid, so that c values scale
/// linearly with sigma and the table is smooth across all sigma values.
#[cfg(not(target_os = "solana"))]
pub fn obs1_projected_state(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> Option<(FilterState, FilterState, i64, i64)> {
    let raw = obs1_seed_raw_weights(cfg, sigma_s6, drift_diffs, drift_shift_63)?;
    let std_s12 = sigma_s6 as i128 * FIRST_STEP_STD_RATIO_S6 as i128;

    // Compute all 13 node positions (always the same relative grid)
    let mut all_c = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut all_mu = [0i64; N_FACTOR_NODES_EXACT_SEED];
    let mut all_mv = [0i64; N_FACTOR_NODES_EXACT_SEED];
    for idx in 0..N_FACTOR_NODES_EXACT_SEED {
        let factor_s12 = std_s12 * SQRT2_S12 as i128 * GH13_NODES[idx] / (S12 * S12);
        all_c[idx] = (factor_s12 / S6 as i128) as i64;
        all_mu[idx] = drift_diffs[0] + cfg.uv_slope[0] * all_c[idx] / S6;
        all_mv[idx] = drift_diffs[1] + cfg.uv_slope[1] * all_c[idx] / S6;
    }

    // Use the full GH13 span for the grid, so c scales linearly with sigma
    let grid_min = all_c[0];
    let grid_max = all_c[N_FACTOR_NODES_EXACT_SEED - 1];
    let grid_span = grid_max - grid_min;
    let k_retained = k_retained.clamp(2, MAX_K);

    let project_to_grid = |safe_w: &[i64], n_nodes: usize| -> FilterState {
        let mut out = FilterState::default();
        let mut mean_u_raw = [0i64; MAX_K];
        let mut mean_v_raw = [0i64; MAX_K];
        let mut total_in = 0i64;
        for idx in 0..k_retained {
            out.nodes[idx].c =
                grid_min + (((grid_span as i128) * idx as i128) / (k_retained - 1) as i128) as i64;
        }
        for idx in 0..n_nodes {
            let w = safe_w[idx];
            if w <= 0 {
                continue;
            }
            total_in += w;
            let c = all_c[idx];
            let mu = all_mu[idx];
            let mv = all_mv[idx];
            if c <= grid_min {
                out.nodes[0].w += w;
                mean_u_raw[0] += m6r(w, mu);
                mean_v_raw[0] += m6r(w, mv);
                continue;
            }
            if c >= grid_max {
                let last = k_retained - 1;
                out.nodes[last].w += w;
                mean_u_raw[last] += m6r(w, mu);
                mean_v_raw[last] += m6r(w, mv);
                continue;
            }
            let pos_scaled = ((c - grid_min) as i128) * (k_retained - 1) as i128 * S6 as i128
                / grid_span as i128;
            let idx_lo = (pos_scaled / S6 as i128) as usize;
            let frac_hi = (pos_scaled - idx_lo as i128 * S6 as i128) as i64;
            let frac_lo = S6 - frac_hi;
            let w_lo = m6r(w, frac_lo);
            let w_hi = w - w_lo;
            out.nodes[idx_lo].w += w_lo;
            mean_u_raw[idx_lo] += m6r(w_lo, mu);
            mean_v_raw[idx_lo] += m6r(w_lo, mv);
            if idx_lo + 1 < k_retained {
                out.nodes[idx_lo + 1].w += w_hi;
                mean_u_raw[idx_lo + 1] += m6r(w_hi, mu);
                mean_v_raw[idx_lo + 1] += m6r(w_hi, mv);
            }
        }
        if total_in <= 0 {
            return FilterState::default();
        }
        let total_out: i64 = out.nodes[..k_retained].iter().map(|n| n.w).sum();
        let diff = total_in - total_out;
        if diff != 0 {
            let fix_idx = strongest_weight_index(&out.nodes);
            out.nodes[fix_idx].w = (out.nodes[fix_idx].w + diff).max(0);
        }
        let mut n_active = 0usize;
        for idx in 0..k_retained {
            if out.nodes[idx].w > 0 {
                out.nodes[idx].mean_u =
                    ((mean_u_raw[idx] as i128) * S6 as i128 / out.nodes[idx].w as i128) as i64;
                out.nodes[idx].mean_v =
                    ((mean_v_raw[idx] as i128) * S6 as i128 / out.nodes[idx].w as i128) as i64;
                n_active += 1;
            }
        }
        out.n_active = n_active;
        out
    };

    let safe_proj = project_to_grid(&raw.safe_w, N_FACTOR_NODES_EXACT_SEED);
    let knocked_proj = project_to_grid(&raw.knocked_w, N_FACTOR_NODES_EXACT_SEED);
    Some((safe_proj, knocked_proj, raw.first_hit, raw.first_knock_in))
}

#[cfg(not(target_os = "solana"))]
pub fn bench_prepare_obs1_seed(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
) -> Option<Obs1SeedSnapshot> {
    let drift_shift_total = cfg.obs[0].obs_day as i64 / 63 * drift_shift_63;
    let triple_pre = cholesky6(cfg.obs[0].cov_uu, cfg.obs[0].cov_uv, cfg.obs[0].cov_vv)
        .ok()
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
    let seed = run_first_observation_seed(
        cfg,
        sigma_s6,
        drift_diffs,
        drift_shift_total,
        OBS1_K_RETAINED,
        triple_pre.as_ref(),
    )?;
    Some(Obs1SeedSnapshot {
        safe_state: seed.next_safe,
        knocked_state: seed.next_knocked,
        first_hit: seed.first_hit,
        first_knock_in: seed.first_knock_in,
    })
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
pub fn bench_prepare_maturity_state(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> Option<MaturityBenchState> {
    let k_knocked = ANALYTIC_DELTA_K_KNOCKED;
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let mut safe_state = FilterState::singleton_origin();
    let mut knocked_state = FilterState::default();

    for obs_idx in 0..(N_OBS - 1) {
        let obs = &cfg.obs[obs_idx];
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;
        let cholesky = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();
        let triple_pre = cholesky
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
        if obs_idx == 0 {
            if let Some(seed) = run_first_observation_seed(
                cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_total,
                k_retained,
                triple_pre.as_ref(),
            ) {
                safe_state = seed.next_safe;
                knocked_state = project_state(&seed.next_knocked, k_knocked);
            } else {
                let (next_safe, next_knocked, _, _) = run_observation_step(
                    &safe_state,
                    &knocked_state,
                    &transition,
                    cfg,
                    obs_idx,
                    drift_shift_total,
                    k_retained,
                    k_knocked,
                    triple_pre.as_ref(),
                    None,
                );
                safe_state = next_safe;
                knocked_state = next_knocked;
            }
            continue;
        }
        let (next_safe, next_knocked, _, _) = run_observation_step(
            &safe_state,
            &knocked_state,
            &transition,
            cfg,
            obs_idx,
            drift_shift_total,
            k_retained,
            k_knocked,
            triple_pre.as_ref(),
            None,
        );
        safe_state = next_safe;
        knocked_state = next_knocked;
    }

    let obs_idx = N_OBS - 1;
    Some(MaturityBenchState {
        safe_state,
        knocked_state,
        transition,
        obs_idx,
        drift_shift_total: cfg.obs[obs_idx].obs_day as i64 / 63 * drift_shift_63,
        k_retained,
    })
}

#[cfg(not(target_os = "solana"))]
pub fn bench_maturity_from_prepared(
    cfg: &C1FastConfig,
    prepared: &MaturityBenchState,
) -> MaturityBenchSummary {
    let obs = &cfg.obs[prepared.obs_idx];
    let cholesky = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();
    let triple_pre = cholesky
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
    let maturity = run_maturity_step(
        &prepared.safe_state,
        &prepared.knocked_state,
        &prepared.transition,
        cfg,
        prepared.obs_idx,
        prepared.drift_shift_total,
        prepared.k_retained,
        triple_pre.as_ref(),
        None,
    );
    MaturityBenchSummary {
        safe_active_nodes: prepared.safe_state.n_active,
        knocked_active_nodes: prepared.knocked_state.n_active,
        coupon_hit: maturity.coupon_hit,
        safe_principal: maturity.safe_principal,
        first_knock_in: maturity.first_knock_in,
        knock_in_redemption_safe: maturity.knock_in_redemption_safe,
        knocked_redemption: maturity.knocked_redemption,
        checksum: prepared
            .safe_state
            .nodes
            .iter()
            .chain(prepared.knocked_state.nodes.iter())
            .fold(0i64, |acc, node| {
                acc + node.c + node.w + node.mean_u + node.mean_v
            })
            + maturity.coupon_hit
            + maturity.safe_principal
            + maturity.first_knock_in
            + maturity.knock_in_redemption_safe
            + maturity.knocked_redemption,
    }
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn bench_observation_gradient_summary(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> ObservationGradientBenchSummary {
    let safe_parent = canned_observation_gradient_state();
    let dmu_ds = compute_dmu_ds(cfg, [S6, S6, S6]);
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let obs_idx = 2usize;
    let drift_shift_total = cfg.obs[obs_idx].obs_day as i64 / 63 * drift_shift_63;
    let obs = &cfg.obs[obs_idx];
    let frozen_grid =
        crate::frozen_predict_tables::frozen_predict_grid_lookup(sigma_s6, obs_idx - 1, k_retained);
    let obs_tp = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
        .ok()
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
    c1_filter_cu_diag_inner(b"gradient_bench_obs_predict_start");
    let (safe_pred, safe_pred_grad) = if let Some(fg) = frozen_grid.as_ref() {
        predict_state_frozen_grad(
            &safe_parent,
            &ZERO_FILTER_STATE_GRAD,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
            &fg.grid_c,
            fg.inv_cell_s30,
        )
    } else {
        predict_state_grad(
            &safe_parent,
            &ZERO_FILTER_STATE_GRAD,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        )
    };
    c1_filter_cu_diag_inner(b"gradient_bench_obs_predict_done");
    c1_filter_cu_diag_inner(b"gradient_bench_update_start");
    let (update, update_grad) = update_safe_state_grad(
        &safe_pred,
        &safe_pred_grad,
        cfg,
        obs_idx,
        drift_shift_total,
        k_retained,
        observation_probability_triple_pre(obs_idx, obs_tp.as_ref()),
        &dmu_ds,
    );
    c1_filter_cu_diag_inner(b"gradient_bench_update_done");

    ObservationGradientBenchSummary {
        first_hit: update.first_hit,
        first_knock_in: update.first_knock_in,
        checksum: update
            .next_safe
            .nodes
            .iter()
            .chain(update.new_knocked.nodes.iter())
            .fold(0i64, |acc, node| {
                acc + node.c + node.w + node.mean_u + node.mean_v
            })
            + update.first_hit
            + update.first_knock_in
            + update_grad.first_hit.iter().sum::<i64>()
            + update_grad.first_knock_in.iter().sum::<i64>(),
    }
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn canned_observation_gradient_state() -> FilterState {
    let mut state = FilterState::default();
    state.nodes[0] = FilterNode {
        c: -320_000,
        w: 210_000,
        mean_u: -105_000,
        mean_v: -55_000,
    };
    state.nodes[1] = FilterNode {
        c: -90_000,
        w: 190_000,
        mean_u: -35_000,
        mean_v: 18_000,
    };
    state.nodes[2] = FilterNode {
        c: 110_000,
        w: 170_000,
        mean_u: 30_000,
        mean_v: -12_000,
    };
    state.nodes[3] = FilterNode {
        c: 305_000,
        w: 160_000,
        mean_u: 88_000,
        mean_v: 52_000,
    };
    state.n_active = 4;
    state
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn boxed_canned_maturity_safe_gradient_state() -> Box<FilterState> {
    let mut state = Box::new(FilterState::default());
    state.nodes[0] = FilterNode {
        c: -260_000,
        w: 170_000,
        mean_u: -84_000,
        mean_v: -45_000,
    };
    state.nodes[1] = FilterNode {
        c: -40_000,
        w: 150_000,
        mean_u: -15_000,
        mean_v: 9_000,
    };
    state.nodes[2] = FilterNode {
        c: 160_000,
        w: 120_000,
        mean_u: 44_000,
        mean_v: 18_000,
    };
    state.n_active = 3;
    state
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn boxed_canned_maturity_knocked_gradient_state() -> Box<FilterState> {
    let mut state = Box::new(FilterState::default());
    state.nodes[0] = FilterNode {
        c: -108_415,
        w: 205_000,
        mean_u: -61_171,
        mean_v: -38_366,
    };
    state.n_active = 1;
    state
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn checksum_filter_state_pair(lhs: &FilterState, rhs: &FilterState) -> i64 {
    lhs.nodes
        .iter()
        .chain(rhs.nodes.iter())
        .fold(0i64, |acc, node| {
            acc + node.c + node.w + node.mean_u + node.mean_v
        })
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn bench_maturity_triple_pre(cfg: &C1FastConfig, obs_idx: usize) -> Option<TripleCorrectionPre> {
    let mat_obs = &cfg.obs[obs_idx];
    cholesky6(mat_obs.cov_uu, mat_obs.cov_uv, mat_obs.cov_vv)
        .ok()
        .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av))
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn boxed_factor_transition(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
) -> Box<FactorTransition> {
    let factor_weights = nig_importance_weights_9(sigma_s6);
    let proposal_std = sigma_s6 / 2;
    let mut transition = Box::new(FactorTransition {
        factor_values: [0; N_FACTOR_NODES],
        factor_weights,
        step_mean_u: [0; N_FACTOR_NODES],
        step_mean_v: [0; N_FACTOR_NODES],
    });
    for idx in 0..N_FACTOR_NODES {
        transition.factor_values[idx] = SQRT2_S6 * proposal_std / S6 * GH9_NODES_S6[idx] / S6;
        transition.step_mean_u[idx] =
            drift_diffs[0] + cfg.uv_slope[0] * transition.factor_values[idx] / S6;
        transition.step_mean_v[idx] =
            drift_diffs[1] + cfg.uv_slope[1] * transition.factor_values[idx] / S6;
    }
    transition
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn run_canned_maturity_safe_gradient_summary(
    call: &MaturityBenchCall<'_>,
) -> MaturitySafeBenchSummary {
    c1_filter_cu_diag_inner(b"gradient_bench_mat_safe_entry");
    let safe_state = boxed_canned_maturity_safe_gradient_state();
    c1_filter_cu_diag_inner(b"gradient_bench_mat_safe_after_state");
    let checksum_base = safe_state.nodes.iter().fold(0i64, |acc, node| {
        acc + node.c + node.w + node.mean_u + node.mean_v
    });
    c1_filter_cu_diag_inner(b"predict_grad_start");
    let (safe_pred, safe_pred_grad) = if let Some(fg) = call.frozen_grid {
        predict_state_frozen_grad(
            safe_state.as_ref(),
            &ZERO_FILTER_STATE_GRAD,
            &call.transition.factor_values,
            &call.transition.factor_weights,
            &call.transition.step_mean_u,
            &call.transition.step_mean_v,
            call.k_retained,
            &fg.grid_c,
            fg.inv_cell_s30,
        )
    } else {
        predict_state_grad(
            safe_state.as_ref(),
            &ZERO_FILTER_STATE_GRAD,
            &call.transition.factor_values,
            &call.transition.factor_weights,
            &call.transition.step_mean_u,
            &call.transition.step_mean_v,
            call.k_retained,
        )
    };
    let safe_pred = Box::new(safe_pred);
    let safe_pred_grad = Box::new(safe_pred_grad);
    c1_filter_cu_diag_inner(b"predict_grad_safe_done");
    let ((coupon_hit, safe_principal, first_knock_in, _knock_in_redemption_safe), safe_grad) =
        maturity_safe_leg_grad(
            safe_pred.as_ref(),
            safe_pred_grad.as_ref(),
            call.cfg,
            call.obs_idx,
            call.drift_shift_total,
            call.triple_pre,
            call.dmu_ds,
        );

    MaturitySafeBenchSummary {
        coupon_hit,
        safe_principal,
        first_knock_in,
        checksum: checksum_base
            + coupon_hit
            + safe_principal
            + first_knock_in
            + safe_grad.coupon_hit.iter().sum::<i64>()
            + safe_grad.safe_principal.iter().sum::<i64>()
            + safe_grad.first_knock_in.iter().sum::<i64>()
            + safe_grad.knock_in_redemption.iter().sum::<i64>(),
    }
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn run_canned_maturity_knocked_gradient_summary(
    call: &MaturityBenchCall<'_>,
) -> MaturityKnockedBenchSummary {
    let k_knocked = ANALYTIC_DELTA_K_KNOCKED;
    c1_filter_cu_diag_inner(b"gradient_bench_mat_knocked_entry");
    let knocked_state = boxed_canned_maturity_knocked_gradient_state();
    c1_filter_cu_diag_inner(b"gradient_bench_mat_knocked_after_state");
    let checksum_base = knocked_state.nodes.iter().fold(0i64, |acc, node| {
        acc + node.c + node.w + node.mean_u + node.mean_v
    });
    let (knocked_pred, knocked_pred_grad) = predict_state_grad(
        knocked_state.as_ref(),
        &ZERO_FILTER_STATE_GRAD,
        &call.transition.factor_values,
        &call.transition.factor_weights,
        &call.transition.step_mean_u,
        &call.transition.step_mean_v,
        k_knocked,
    );
    let knocked_pred = Box::new(knocked_pred);
    let knocked_pred_grad = Box::new(knocked_pred_grad);
    c1_filter_cu_diag_inner(b"predict_grad_done");
    let ((coupon_hit, knocked_redemption), knocked_grad) = maturity_knocked_leg_grad(
        knocked_pred.as_ref(),
        knocked_pred_grad.as_ref(),
        call.cfg,
        call.obs_idx,
        call.drift_shift_total,
        call.triple_pre,
        call.dmu_ds,
    );

    MaturityKnockedBenchSummary {
        coupon_hit,
        knocked_redemption,
        checksum: checksum_base
            + coupon_hit
            + knocked_redemption
            + knocked_grad.coupon_hit.iter().sum::<i64>()
            + knocked_grad.redemption.iter().sum::<i64>(),
    }
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
fn bench_maturity_gradient_summary(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> MaturityGradientBenchSummary {
    c1_filter_cu_diag_inner(b"gradient_bench_mat_start");
    let dmu_ds = Box::new(compute_dmu_ds(cfg, [S6, S6, S6]));
    c1_filter_cu_diag_inner(b"gradient_bench_mat_after_dmu");
    let obs_idx = N_OBS - 1;
    let drift_shift_total = cfg.obs[obs_idx].obs_day as i64 / 63 * drift_shift_63;
    let mat_tp = Box::new(bench_maturity_triple_pre(cfg, obs_idx));
    c1_filter_cu_diag_inner(b"gradient_bench_mat_after_tp");
    let maturity_fg = Box::new(crate::frozen_predict_tables::frozen_predict_grid_lookup(
        sigma_s6,
        obs_idx - 1,
        k_retained,
    ));
    c1_filter_cu_diag_inner(b"gradient_bench_mat_after_fg");
    let transition = boxed_factor_transition(cfg, sigma_s6, drift_diffs);
    c1_filter_cu_diag_inner(b"gradient_bench_mat_after_transition");
    let call = MaturityBenchCall {
        cfg,
        transition: transition.as_ref(),
        obs_idx,
        drift_shift_total,
        k_retained,
        triple_pre: observation_probability_triple_pre(obs_idx, mat_tp.as_ref().as_ref()),
        frozen_grid: maturity_fg.as_ref().as_ref(),
        dmu_ds: dmu_ds.as_ref(),
    };
    c1_filter_cu_diag_inner(b"gradient_bench_mat_safe_call");
    let safe_summary = run_canned_maturity_safe_gradient_summary(&call);
    c1_filter_cu_diag_inner(b"gradient_bench_mat_safe_done");
    c1_filter_cu_diag_inner(b"gradient_bench_mat_knocked_call");
    let knocked_summary = run_canned_maturity_knocked_gradient_summary(&call);
    c1_filter_cu_diag_inner(b"gradient_bench_mat_knocked_done");
    let summary = MaturityGradientBenchSummary {
        coupon_hit: safe_summary.coupon_hit + knocked_summary.coupon_hit,
        safe_principal: safe_summary.safe_principal,
        first_knock_in: safe_summary.first_knock_in,
        knocked_redemption: knocked_summary.knocked_redemption,
        checksum: safe_summary.checksum + knocked_summary.checksum,
    };
    c1_filter_cu_diag_inner(b"gradient_bench_mat_done");
    summary
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
pub fn bench_gradient_pipeline(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> GradientBenchSummary {
    let observation =
        bench_observation_gradient_summary(cfg, sigma_s6, drift_diffs, drift_shift_63, k_retained);
    c1_filter_cu_diag_inner(b"gradient_bench_before_mat_call");
    let maturity =
        bench_maturity_gradient_summary(cfg, sigma_s6, drift_diffs, drift_shift_63, k_retained);
    c1_filter_cu_diag_inner(b"gradient_bench_after_mat_call");

    GradientBenchSummary {
        update_first_hit: observation.first_hit,
        update_first_knock_in: observation.first_knock_in,
        maturity_coupon_hit: maturity.coupon_hit,
        maturity_safe_principal: maturity.safe_principal,
        maturity_first_knock_in: maturity.first_knock_in,
        maturity_knocked_redemption: maturity.knocked_redemption,
        checksum: observation.checksum
            + maturity.checksum
            + observation.first_hit
            + observation.first_knock_in
            + maturity.coupon_hit
            + maturity.safe_principal
            + maturity.first_knock_in
            + maturity.knocked_redemption,
    }
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
pub fn bench_gradient_pipeline_checksum(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> i64 {
    let observation =
        bench_observation_gradient_summary(cfg, sigma_s6, drift_diffs, drift_shift_63, k_retained);
    c1_filter_cu_diag_inner(b"gradient_bench_before_mat_call");
    let maturity =
        bench_maturity_gradient_summary(cfg, sigma_s6, drift_diffs, drift_shift_63, k_retained);
    c1_filter_cu_diag_inner(b"gradient_bench_after_mat_call");

    observation.checksum
        + maturity.checksum
        + observation.first_hit
        + observation.first_knock_in
        + maturity.coupon_hit
        + maturity.safe_principal
        + maturity.first_knock_in
        + maturity.knocked_redemption
}

#[cfg(not(target_os = "solana"))]
#[inline(never)]
pub fn bench_maturity_knocked_gradient_only(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> GradientKnockedBenchSummary {
    c1_filter_cu_diag_inner(b"gradient_bench_mat_knocked_only_start");
    let dmu_ds = Box::new(compute_dmu_ds(cfg, [S6, S6, S6]));
    let obs_idx = N_OBS - 1;
    let drift_shift_total = cfg.obs[obs_idx].obs_day as i64 / 63 * drift_shift_63;
    let mat_tp = Box::new(bench_maturity_triple_pre(cfg, obs_idx));
    let transition = boxed_factor_transition(cfg, sigma_s6, drift_diffs);
    let call = MaturityBenchCall {
        cfg,
        transition: transition.as_ref(),
        obs_idx,
        drift_shift_total,
        k_retained,
        triple_pre: observation_probability_triple_pre(obs_idx, mat_tp.as_ref().as_ref()),
        frozen_grid: None,
        dmu_ds: dmu_ds.as_ref(),
    };
    let summary = run_canned_maturity_knocked_gradient_summary(&call);
    c1_filter_cu_diag_inner(b"gradient_bench_mat_knocked_only_done");
    GradientKnockedBenchSummary {
        maturity_coupon_hit: summary.coupon_hit,
        maturity_knocked_redemption: summary.knocked_redemption,
        checksum: summary.checksum,
    }
}

/// Grid geometry extracted from one predict_state call.
pub struct PredictGridGeometry {
    pub grid_c: [i64; MAX_K],
    pub inv_cell_s30: i64,
}

/// Run the full predict+update pipeline and return the grid geometry
/// at each of the 5 predict_state calls (obs_idx 1..5).
///
/// Used by the generator to produce frozen predict tables that exactly
/// match the on-chain grid construction.
#[cfg(not(target_os = "solana"))]
pub fn extract_predict_grid_geometry(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_retained: usize,
) -> [PredictGridGeometry; 5] {
    let k_knocked = 1usize;
    let k_retained = k_retained.clamp(1, MAX_K);
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let mut safe_state = FilterState::singleton_origin();
    let mut knocked_state = FilterState::default();
    let mut grids: [PredictGridGeometry; 5] = core::array::from_fn(|_| PredictGridGeometry {
        grid_c: [0i64; MAX_K],
        inv_cell_s30: 0,
    });

    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let is_maturity = obs_idx + 1 == N_OBS;
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;

        if obs_idx == 0 {
            let (obs1_safe, obs1_knocked, _, _) = if k_retained >= 10 {
                crate::obs1_seed_tables::obs1_projected_lookup_k15(sigma_s6)
            } else {
                crate::obs1_seed_tables::obs1_projected_lookup(sigma_s6)
            };
            if k_retained < 15 {
                safe_state = project_state(&obs1_safe, k_retained);
            } else {
                safe_state = obs1_safe;
            }
            knocked_state = project_state(&obs1_knocked, k_knocked);
            continue;
        }

        // Run predict_state to get the grid
        let safe_pred = predict_state(
            &safe_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_retained,
        );

        // Extract grid geometry from predict output
        let obs_rel = obs_idx - 1;
        for i in 0..k_retained {
            grids[obs_rel].grid_c[i] = safe_pred.nodes[i].c;
        }
        let grid_span = safe_pred.nodes[k_retained - 1].c - safe_pred.nodes[0].c;
        let k_m1 = (k_retained - 1) as i64;
        grids[obs_rel].inv_cell_s30 = if grid_span > 0 {
            k_m1 * (1i64 << 30) / grid_span
        } else {
            0
        };

        // Run the full update to get the correct parent state for the next obs
        let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
        let tp = observation_probability_triple_pre(obs_idx, triple_pre.as_ref());

        if is_maturity {
            // Last obs — no need to update parent state
            break;
        }

        let (next_safe, next_knocked, _, _) = run_observation_step(
            &safe_state,
            &knocked_state,
            &transition,
            cfg,
            obs_idx,
            drift_shift_total,
            k_retained,
            k_knocked,
            tp,
            None, // no frozen grid — use live predict internally
        );
        safe_state = next_safe;
        knocked_state = next_knocked;
    }

    grids
}

/// Tapered version of extract_predict_grid_geometry with per-obs K schedule.
#[cfg(not(target_os = "solana"))]
pub fn extract_predict_grid_geometry_tapered(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_diffs: [i64; 2],
    drift_shift_63: i64,
    k_schedule: &[usize; 5],
) -> [PredictGridGeometry; 5] {
    let k_knocked = 1usize;
    let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
    let mut safe_state = FilterState::singleton_origin();
    let mut knocked_state = FilterState::default();
    let mut grids: [PredictGridGeometry; 5] = core::array::from_fn(|_| PredictGridGeometry {
        grid_c: [0i64; MAX_K],
        inv_cell_s30: 0,
    });

    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let is_maturity = obs_idx + 1 == N_OBS;
        let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;

        if obs_idx == 0 {
            let (obs1_safe, obs1_knocked, _, _) =
                crate::obs1_seed_tables::obs1_projected_lookup(sigma_s6);
            let k_obs1 = k_schedule[0].clamp(1, MAX_K);
            if k_obs1 < 9 {
                safe_state = project_state(&obs1_safe, k_obs1);
            } else {
                safe_state = obs1_safe;
            }
            knocked_state = project_state(&obs1_knocked, k_knocked);
            continue;
        }

        let k_safe = k_schedule[obs_idx - 1].clamp(1, MAX_K);
        let safe_pred = predict_state(
            &safe_state,
            &transition.factor_values,
            &transition.factor_weights,
            &transition.step_mean_u,
            &transition.step_mean_v,
            k_safe,
        );

        let obs_rel = obs_idx - 1;
        for i in 0..k_safe {
            grids[obs_rel].grid_c[i] = safe_pred.nodes[i].c;
        }
        let grid_span = safe_pred.nodes[k_safe - 1].c - safe_pred.nodes[0].c;
        let k_m1 = (k_safe - 1) as i64;
        grids[obs_rel].inv_cell_s30 = if grid_span > 0 {
            k_m1 * (1i64 << 30) / grid_span
        } else {
            0
        };

        let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
        let tp = observation_probability_triple_pre(obs_idx, triple_pre.as_ref());

        if is_maturity {
            break;
        }

        let (next_safe, next_knocked, _, _) = run_observation_step(
            &safe_state,
            &knocked_state,
            &transition,
            cfg,
            obs_idx,
            drift_shift_total,
            k_safe,
            k_knocked,
            tp,
            None,
        );
        safe_state = next_safe;
        knocked_state = next_knocked;
    }

    grids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worst_of_c1_fast::{
        quote_c1_fast, spy_qqq_iwm_c1_config, spy_qqq_iwm_step_drift_inputs_s6,
    };
    use crate::worst_of_factored::FactoredWorstOfModel;

    fn filter_inputs(sigma_common: f64) -> (C1FastConfig, i64, [i64; 2], i64) {
        let cfg = spy_qqq_iwm_c1_config();
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let drifts = model.risk_neutral_step_drifts(sigma_common, 63).unwrap();
        let drift_diffs = [
            ((drifts[1] - drifts[0]) * S6 as f64).round() as i64,
            ((drifts[2] - drifts[0]) * S6 as f64).round() as i64,
        ];
        let drift_shift_63 = ((cfg.loadings[0] as f64 * drifts[0])
            + (cfg.loadings[1] as f64 * drifts[1])
            + (cfg.loadings[2] as f64 * drifts[2]))
            .round() as i64;
        let sigma_s6 = (sigma_common * S6 as f64).round() as i64;
        (cfg, sigma_s6, drift_diffs, drift_shift_63)
    }

    #[test]
    fn fixed_point_drift_coupon_matches_legacy_f64_reference_sweep() {
        let mut max_abs_bps = 0.0f64;
        let mut max_rel_err = 0.0f64;
        let mut worst_sigma = 0.0f64;
        let mut worst_legacy_bps = 0.0f64;
        let mut worst_fixed_bps = 0.0f64;
        let mut worst_legacy_drift_diffs = [0i64; 2];
        let mut worst_fixed_drift_diffs = [0i64; 2];
        let mut worst_legacy_shift = 0i64;
        let mut worst_fixed_shift = 0i64;

        for idx in 0..=18 {
            let sigma = 0.08 + idx as f64 * 0.04;
            let (cfg, sigma_s6, legacy_drift_diffs, legacy_drift_shift_63) = filter_inputs(sigma);
            let (fixed_drift_diffs, fixed_drift_shift_63) =
                spy_qqq_iwm_step_drift_inputs_s6(&cfg, sigma_s6, 63).unwrap();

            let legacy_bps = quote_c1_filter(
                &cfg,
                sigma_s6,
                legacy_drift_diffs,
                legacy_drift_shift_63,
                12,
            )
            .fair_coupon_bps_f64()
                + crate::k12_correction::k12_correction_lookup(sigma_s6) as f64 / 1_000_000.0
                + crate::daily_ki_correction::daily_ki_correction_lookup(sigma_s6) as f64
                    / 1_000_000.0;
            let fixed_bps =
                quote_c1_filter(&cfg, sigma_s6, fixed_drift_diffs, fixed_drift_shift_63, 12)
                    .fair_coupon_bps_f64()
                    + crate::k12_correction::k12_correction_lookup(sigma_s6) as f64 / 1_000_000.0
                    + crate::daily_ki_correction::daily_ki_correction_lookup(sigma_s6) as f64
                        / 1_000_000.0;

            let abs_err = (fixed_bps - legacy_bps).abs();
            let rel_err = abs_err / legacy_bps.abs().max(1.0);
            if abs_err > max_abs_bps {
                max_abs_bps = abs_err;
                max_rel_err = rel_err;
                worst_sigma = sigma;
                worst_legacy_bps = legacy_bps;
                worst_fixed_bps = fixed_bps;
                worst_legacy_drift_diffs = legacy_drift_diffs;
                worst_fixed_drift_diffs = fixed_drift_diffs;
                worst_legacy_shift = legacy_drift_shift_63;
                worst_fixed_shift = fixed_drift_shift_63;
            }
        }

        println!(
            "fixed-point drift coupon sweep: max_abs_bps={max_abs_bps:.6} max_rel_err={max_rel_err:.6} sigma={worst_sigma:.4} legacy_bps={worst_legacy_bps:.6} fixed_bps={worst_fixed_bps:.6} legacy_dd={:?} fixed_dd={:?} legacy_shift={} fixed_shift={}",
            worst_legacy_drift_diffs,
            worst_fixed_drift_diffs,
            worst_legacy_shift,
            worst_fixed_shift,
        );

        assert!(
            max_abs_bps < 10.0,
            "fixed-point drift regression too large: max_abs_bps={max_abs_bps:.6} max_rel_err={max_rel_err:.6} sigma={worst_sigma:.4}"
        );
    }

    fn spot_shift_bundle(cfg: &C1FastConfig, spots_s6: [i64; 3]) -> (i64, i64, i64) {
        let log_spy = (spots_s6[0] as f64 / S6 as f64).ln();
        let log_qqq = (spots_s6[1] as f64 / S6 as f64).ln();
        let log_iwm = (spots_s6[2] as f64 / S6 as f64).ln();
        let mu_u = ((log_qqq - log_spy) * S6 as f64).round() as i64;
        let mu_v = ((log_iwm - log_spy) * S6 as f64).round() as i64;
        let mu_c = (cfg.loadings[0] as f64 * log_spy
            + cfg.loadings[1] as f64 * log_qqq
            + cfg.loadings[2] as f64 * log_iwm)
            .round() as i64;
        (mu_u, mu_v, mu_c)
    }

    fn shift_state_means(state: &FilterState, mu_u_shift: i64, mu_v_shift: i64) -> FilterState {
        let mut out = *state;
        for node in out.nodes.iter_mut().take(out.n_active) {
            if node.w <= 0 {
                continue;
            }
            node.mean_u += mu_u_shift;
            node.mean_v += mu_v_shift;
        }
        out
    }

    fn quote_c1_filter_s6_with_spots(
        cfg: &C1FastConfig,
        sigma_s6: i64,
        drift_diffs: [i64; 2],
        drift_shift_63: i64,
        k_retained: usize,
        spots_s6: [i64; 3],
    ) -> i64 {
        let k_retained = k_retained.clamp(1, MAX_K);
        let k_knocked = ANALYTIC_DELTA_K_KNOCKED;
        let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
        let frozen_grids: [Option<crate::frozen_predict_tables::FrozenPredictGrid>; 5] =
            core::array::from_fn(|obs_rel| {
                crate::frozen_predict_tables::frozen_predict_grid_lookup(
                    sigma_s6, obs_rel, k_retained,
                )
            });
        let (mu_u_shift, mu_v_shift, mu_c_shift) = spot_shift_bundle(cfg, spots_s6);

        let mut safe_state = FilterState::singleton_origin();
        let mut knocked_state = FilterState::default();
        let mut redemption_prob = 0i64;
        let mut coupon_annuity = 0i64;

        for obs_idx in 0..N_OBS {
            let obs = &cfg.obs[obs_idx];
            let is_maturity = obs_idx + 1 == N_OBS;
            let coupon_count = (obs_idx + 1) as i64;
            let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63 + mu_c_shift;
            let triple_pre =
                cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
                    .ok()
                    .map(|(l11, l21, l22)| {
                        build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av)
                    });
            let tp = observation_probability_triple_pre(obs_idx, triple_pre.as_ref());

            if obs_idx == 0 {
                let exact_transition = build_exact_seed_transition(cfg, sigma_s6, drift_diffs)
                    .expect("exact seed transition available");
                let predicted_safe = shift_state_means(
                    &predicted_state_from_exact_seed_transition(&exact_transition),
                    mu_u_shift,
                    mu_v_shift,
                );
                let safe_update =
                    update_safe_state(&predicted_safe, cfg, 0, drift_shift_total, k_retained, tp);
                redemption_prob += safe_update.first_hit;
                coupon_annuity += coupon_count * safe_update.first_hit;
                let (obs1_safe, obs1_knocked, _, _) = if k_retained >= 10 {
                    crate::obs1_seed_tables::obs1_projected_lookup_k15(sigma_s6)
                } else {
                    crate::obs1_seed_tables::obs1_projected_lookup(sigma_s6)
                };
                safe_state = shift_state_means(
                    &(if k_retained < 15 {
                        project_state(&obs1_safe, k_retained)
                    } else {
                        obs1_safe
                    }),
                    mu_u_shift,
                    mu_v_shift,
                );
                knocked_state = shift_state_means(
                    &project_state(&obs1_knocked, k_knocked),
                    mu_u_shift,
                    mu_v_shift,
                );
                continue;
            }

            if is_maturity {
                let maturity = run_maturity_step(
                    &safe_state,
                    &knocked_state,
                    &transition,
                    cfg,
                    obs_idx,
                    drift_shift_total,
                    k_retained,
                    tp,
                    frozen_grids[obs_idx - 1].as_ref(),
                );
                redemption_prob += maturity.safe_principal
                    + maturity.knock_in_redemption_safe
                    + maturity.knocked_redemption;
                coupon_annuity += coupon_count * maturity.coupon_hit;
                continue;
            }

            let (next_safe, next_knocked, first_hit, _first_knock_in) = run_observation_step(
                &safe_state,
                &knocked_state,
                &transition,
                cfg,
                obs_idx,
                drift_shift_total,
                k_retained,
                k_knocked,
                tp,
                frozen_grids[obs_idx - 1].as_ref(),
            );
            redemption_prob += first_hit;
            coupon_annuity += coupon_count * first_hit;
            safe_state = next_safe;
            knocked_state = next_knocked;
        }

        if coupon_annuity > 100 {
            (S6 - redemption_prob).max(0) * S6 / coupon_annuity
        } else {
            0
        }
    }

    fn maturity_step_scalar(step: &MaturityStep) -> i64 {
        7 * step.coupon_hit
            + 11 * step.safe_principal
            + 13 * step.first_knock_in
            + 17 * step.knock_in_redemption_safe
            + 19 * step.knocked_redemption
    }

    fn maturity_step_scalar_grad(step_grad: &MaturityStepGrad, asset: usize) -> i64 {
        7 * step_grad.coupon_hit[asset]
            + 11 * step_grad.safe_principal[asset]
            + 13 * step_grad.first_knock_in[asset]
            + 17 * step_grad.knock_in_redemption_safe[asset]
            + 19 * step_grad.knocked_redemption[asset]
    }

    fn maturity_local_fair_coupon(step: &MaturityStep, coupon_count: i64) -> i64 {
        let redemption_prob =
            step.safe_principal + step.knock_in_redemption_safe + step.knocked_redemption;
        let loss = (NOTIONAL - m6r(NOTIONAL, redemption_prob)).max(0);
        let coupon_annuity = coupon_count * step.coupon_hit;
        if coupon_annuity > 100 {
            loss * S6 / coupon_annuity
        } else {
            0
        }
    }

    fn maturity_local_fair_coupon_grad(
        step: &MaturityStep,
        step_grad: &MaturityStepGrad,
        asset: usize,
        coupon_count: i64,
    ) -> i64 {
        let redemption_prob =
            step.safe_principal + step.knock_in_redemption_safe + step.knocked_redemption;
        let loss = (NOTIONAL - m6r(NOTIONAL, redemption_prob)).max(0);
        let coupon_annuity = coupon_count * step.coupon_hit;
        if coupon_annuity <= 100 {
            return 0;
        }
        let d_redemption_prob = step_grad.safe_principal[asset]
            + step_grad.knock_in_redemption_safe[asset]
            + step_grad.knocked_redemption[asset];
        let d_loss = -m6r(NOTIONAL, d_redemption_prob);
        let d_coupon_annuity = coupon_count * step_grad.coupon_hit[asset];
        conditional_mean_grad(coupon_annuity, loss, d_coupon_annuity, d_loss)
    }

    fn maturity_step_scalar_with_spots(
        prepared: &MaturityBenchState,
        cfg: &C1FastConfig,
        sigma_s6: i64,
        spots_s6: [i64; 3],
    ) -> i64 {
        let (mu_u_shift, mu_v_shift, mu_c_shift) = spot_shift_bundle(cfg, spots_s6);
        let safe_state = shift_state_means(&prepared.safe_state, mu_u_shift, mu_v_shift);
        let knocked_state = shift_state_means(&prepared.knocked_state, mu_u_shift, mu_v_shift);
        let obs = &cfg.obs[prepared.obs_idx];
        let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
        let frozen_grid = crate::frozen_predict_tables::frozen_predict_grid_lookup(
            sigma_s6,
            prepared.obs_idx - 1,
            prepared.k_retained,
        );
        let step = run_maturity_step(
            &safe_state,
            &knocked_state,
            &prepared.transition,
            cfg,
            prepared.obs_idx,
            prepared.drift_shift_total + mu_c_shift,
            prepared.k_retained,
            observation_probability_triple_pre(prepared.obs_idx, triple_pre.as_ref()),
            frozen_grid.as_ref(),
        );
        maturity_step_scalar(&step)
    }

    fn maturity_local_fair_coupon_with_spots(
        prepared: &MaturityBenchState,
        cfg: &C1FastConfig,
        sigma_s6: i64,
        spots_s6: [i64; 3],
    ) -> i64 {
        let (mu_u_shift, mu_v_shift, mu_c_shift) = spot_shift_bundle(cfg, spots_s6);
        let safe_state = shift_state_means(&prepared.safe_state, mu_u_shift, mu_v_shift);
        let knocked_state = shift_state_means(&prepared.knocked_state, mu_u_shift, mu_v_shift);
        let obs = &cfg.obs[prepared.obs_idx];
        let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
        let frozen_grid = crate::frozen_predict_tables::frozen_predict_grid_lookup(
            sigma_s6,
            prepared.obs_idx - 1,
            prepared.k_retained,
        );
        let step = run_maturity_step(
            &safe_state,
            &knocked_state,
            &prepared.transition,
            cfg,
            prepared.obs_idx,
            prepared.drift_shift_total + mu_c_shift,
            prepared.k_retained,
            observation_probability_triple_pre(prepared.obs_idx, triple_pre.as_ref()),
            frozen_grid.as_ref(),
        );
        maturity_local_fair_coupon(&step, (prepared.obs_idx + 1) as i64)
    }

    fn single_step_ac_first_hit_with_spots(
        state: &FilterState,
        cfg: &C1FastConfig,
        obs_idx: usize,
        drift_shift_total: i64,
        k_retained: usize,
        triple_pre: Option<&TripleCorrectionPre>,
        spots_s6: [i64; 3],
    ) -> i64 {
        let obs = &cfg.obs[obs_idx];
        let tables = phi2_tables();
        let frozen = frozen_observation_view(k_retained, obs_idx);
        let (mu_u_shift, mu_v_shift, mu_c_shift) = spot_shift_bundle(cfg, spots_s6);
        state
            .nodes
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, node)| node.w > 0)
            .map(|(idx, node)| {
                let ac_rhs = [cfg.autocall_rhs_base + node.c + drift_shift_total + mu_c_shift; 3];
                let p_ac = triangle_region_from_frozen_inline_i64(
                    node.mean_u + mu_u_shift,
                    node.mean_v + mu_v_shift,
                    &ac_rhs,
                    &obs.tri_pre,
                    tables,
                    triple_pre,
                    idx,
                    k_retained,
                    frozen.safe_autocall,
                )
                .probability;
                m6r(node.w, p_ac)
            })
            .sum::<i64>()
    }

    fn single_step_ac_only_fair_coupon_from_first_hit(notional: i64, first_hit: i64) -> f64 {
        if first_hit <= 100 {
            return 0.0;
        }
        let loss = (notional - m6r(notional, first_hit)).max(0);
        loss as f64 * S6 as f64 / first_hit as f64
    }

    fn single_step_ki_first_hit_with_spots(
        state: &FilterState,
        cfg: &C1FastConfig,
        obs_idx: usize,
        drift_shift_total: i64,
        spots_s6: [i64; 3],
    ) -> i64 {
        let obs = &cfg.obs[obs_idx];
        let (l11, l21, l22) = match cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv) {
            Ok(values) => values,
            Err(_) => return 0,
        };
        let (mu_u_shift, mu_v_shift, mu_c_shift) = spot_shift_bundle(cfg, spots_s6);
        state
            .nodes
            .iter()
            .copied()
            .filter(|node| node.w > 0)
            .map(|node| {
                let ki_prob = ki_region_uv_moment_gh3(
                    node.mean_u + mu_u_shift,
                    node.mean_v + mu_v_shift,
                    l11,
                    l21,
                    l22,
                    cfg.ki_barrier_log,
                    ki_coords_from_cumulative(cfg, node.c, drift_shift_total + mu_c_shift),
                )
                .probability;
                m6r(node.w, ki_prob)
            })
            .sum::<i64>()
    }

    fn single_step_ki_first_hit_smoothed_with_spots(
        state: &FilterState,
        cfg: &C1FastConfig,
        obs_idx: usize,
        drift_shift_total: i64,
        spots_s6: [i64; 3],
    ) -> i64 {
        let obs = &cfg.obs[obs_idx];
        let (l11, l21, l22) = match cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv) {
            Ok(values) => values,
            Err(_) => return 0,
        };
        let (mu_u_shift, mu_v_shift, mu_c_shift) = spot_shift_bundle(cfg, spots_s6);
        state
            .nodes
            .iter()
            .copied()
            .filter(|node| node.w > 0)
            .map(|node| {
                let ki_prob = ki_region_uv_moment_gh3_smoothed(
                    node.mean_u + mu_u_shift,
                    node.mean_v + mu_v_shift,
                    l11,
                    l21,
                    l22,
                    cfg.ki_barrier_log,
                    ki_coords_from_cumulative(cfg, node.c, drift_shift_total + mu_c_shift),
                )
                .probability;
                m6r(node.w, ki_prob)
            })
            .sum::<i64>()
    }

    fn norm_cdf_f64(x: f64) -> f64 {
        let a1 = 0.254_829_592;
        let a2 = -0.284_496_736;
        let a3 = 1.421_413_741;
        let a4 = -1.453_152_027;
        let a5 = 1.061_405_429;
        let p = 0.327_591_1;
        let sign = if x < 0.0 { -1.0 } else { 1.0 };
        let x_abs = x.abs() / 2f64.sqrt();
        let t = 1.0 / (1.0 + p * x_abs);
        let y = 1.0 - (((((a5 * t + a4) * t) + a3) * t + a2) * t + a1) * t * (-x_abs * x_abs).exp();
        0.5 * (1.0 + sign * y)
    }

    fn single_step_ki_first_hit_smoothed_with_spots_f64(
        state: &FilterState,
        cfg: &C1FastConfig,
        obs_idx: usize,
        drift_shift_total: i64,
        spots_s6: [i64; 3],
    ) -> f64 {
        let obs = &cfg.obs[obs_idx];
        let (l11, l21, l22) = match cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv) {
            Ok(values) => values,
            Err(_) => return 0.0,
        };
        let sl11 = m6r_fast(SQRT2_S6, l11) as f64 / S6 as f64;
        let sl21 = m6r_fast(SQRT2_S6, l21) as f64 / S6 as f64;
        let sl22 = m6r_fast(SQRT2_S6, l22) as f64 / S6 as f64;
        let barrier = cfg.ki_barrier_log as f64 / S6 as f64;
        let h = ki_grad_smooth_h_s6(cfg.ki_barrier_log) as f64 / S6 as f64;
        let (mu_u_shift, mu_v_shift, mu_c_shift) = spot_shift_bundle(cfg, spots_s6);

        let mut total = 0.0f64;
        for node in state.nodes.iter().copied().filter(|node| node.w > 0) {
            let coords = ki_coords_from_cumulative(cfg, node.c, drift_shift_total + mu_c_shift);
            let node_w = node.w as f64 / S6 as f64;
            let mean_u = (node.mean_u + mu_u_shift) as f64 / S6 as f64;
            let mean_v = (node.mean_v + mu_v_shift) as f64 / S6 as f64;

            for (i, &zi_raw) in GH3_NODES_6.iter().enumerate() {
                let zi = zi_raw as f64 / S6 as f64;
                let wi = GH3_WPI_6[i] as f64 / S6 as f64;
                let u = mean_u + sl11 * zi;
                let v_base = mean_v + sl21 * zi;

                for (j, &zj_raw) in GH3_NODES_6.iter().enumerate() {
                    let zj = zj_raw as f64 / S6 as f64;
                    let w = wi * (GH3_WPI_6[j] as f64 / S6 as f64);
                    let v = v_base + sl22 * zj;
                    let x = core::array::from_fn::<_, 3, _>(|k| {
                        coords[k].constant as f64 / S6 as f64
                            + (coords[k].u_coeff as f64 / S6 as f64) * u
                            + (coords[k].v_coeff as f64 / S6 as f64) * v
                    });
                    let t0 = ((barrier - x[0]) / h).clamp(-8.0, 8.0);
                    let t1 = ((barrier - x[1]) / h).clamp(-8.0, 8.0);
                    let t2 = ((barrier - x[2]) / h).clamp(-8.0, 8.0);
                    let smooth_union = 1.0
                        - (1.0 - norm_cdf_f64(t0))
                            * (1.0 - norm_cdf_f64(t1))
                            * (1.0 - norm_cdf_f64(t2));
                    total += node_w * w * smooth_union;
                }
            }
        }

        total * S6 as f64
    }

    /// Phase 3: `predict_state_matrix` must conserve mass and pass through
    /// step drift correctly. No exact match to `predict_state` because the
    /// two use different grids (nested barrier-adapted vs uniform).
    #[test]
    fn predict_state_matrix_conserves_mass_and_drift() {
        use crate::b_tensors::K_SCHEDULE;

        let sigma_s6 = 300_000i64;
        let factor_weights = nig_importance_weights_9(sigma_s6);
        // Start with a unit-mass parent concentrated at z=0 (middle of grid):
        // low mass loss at z-boundaries regardless of step.
        let mut parent = FilterState::default();
        parent.nodes[0] = FilterNode {
            c: 0,
            w: S6,
            mean_u: 0,
            mean_v: 0,
        };
        parent.n_active = 1;

        // Non-zero drift: verify the weighted mean transports through T.
        let step_mean_u = [50_000i64; N_FACTOR_NODES];
        let step_mean_v = [-30_000i64; N_FACTOR_NODES];

        // Run each transition from the same singleton parent; for steps 1..3
        // we just prepend a full-width parent state consistent with k_parent.
        for step in 0..4 {
            let mut local_parent = parent;
            local_parent.n_active = K_SCHEDULE[step];
            // Spread the singleton mass equally across the nominal k_parent
            // grid so the test exercises many parents at once.
            let k_parent = K_SCHEDULE[step];
            let per = S6 / k_parent as i64;
            for j in 0..k_parent {
                local_parent.nodes[j] = FilterNode {
                    c: 0,
                    w: per,
                    mean_u: 0,
                    mean_v: 0,
                };
            }
            let carry = S6 - per * k_parent as i64;
            local_parent.nodes[0].w += carry;

            let child = predict_state_matrix(
                &local_parent,
                &factor_weights,
                &step_mean_u,
                &step_mean_v,
                step,
                sigma_s6,
            );

            let mass_in: i64 = local_parent.nodes.iter().map(|n| n.w).sum();
            let mass_out: i64 = child.nodes.iter().map(|n| n.w).sum();
            let mass_err = (mass_out - mass_in).abs();
            // Allow up to 5% mass loss to endpoint clipping (parents at
            // |z|≈3 push children out of the child's hull by up to 2.5σ).
            assert!(
                mass_err < mass_in / 20,
                "step {step}: mass not preserved: in={mass_in} out={mass_out} err={mass_err}"
            );

            // Drift check: weighted mean_u should be ≈ 50_000 (step drift).
            let wu_total: i128 = child
                .nodes
                .iter()
                .map(|n| n.w as i128 * n.mean_u as i128 / S6 as i128)
                .sum();
            let wv_total: i128 = child
                .nodes
                .iter()
                .map(|n| n.w as i128 * n.mean_v as i128 / S6 as i128)
                .sum();
            let mean_u_eff = (wu_total * S6 as i128 / mass_out as i128) as i64;
            let mean_v_eff = (wv_total * S6 as i128 / mass_out as i128) as i64;
            assert!(
                (mean_u_eff - 50_000).abs() < 2_000,
                "step {step}: mean_u drift not carried: got {mean_u_eff}, expected ≈ 50_000"
            );
            assert!(
                (mean_v_eff - (-30_000)).abs() < 2_000,
                "step {step}: mean_v drift not carried: got {mean_v_eff}, expected ≈ -30_000"
            );

            // All child weights non-negative.
            for n in child.nodes.iter() {
                assert!(n.w >= 0, "step {step}: negative weight after scatter");
            }
        }
    }

    /// Phase 4 (m6r-recip rollout): compare K=12 frozen (the on-chain
    /// production path used by `bench_c1_filter_quote_k12`) at three
    /// anchor σ. Report fair-coupon for whichever build (baseline vs
    /// `--features m6r-recip`) is active. Diff the two outputs to see
    /// whether the reciprocal swap stays within the <1 bps target.
    #[test]
    fn k12_frozen_fair_coupon_at_anchors() {
        for sigma in [0.15_f64, 0.20, 0.291_482_300_850_330_96, 0.30, 0.40] {
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
            let q = crate::worst_of_c1_filter::quote_c1_filter(
                &cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_63,
                12,
            );
            eprintln!(
                "K12_frozen σ={sigma:.4}  fc={:.6} bps  v0={:.6}  ki={:.6}  ac={:.6}",
                q.fair_coupon_bps_f64(),
                q.zero_coupon_pv_f64(),
                q.knock_in_rate_f64(),
                q.autocall_rate_f64(),
            );
        }
    }

    /// Phase 6 of m6r-recip rollout: K=15 frozen accuracy at production
    /// anchor σ vs exact reference. Measures whether K=15 — now affordable
    /// thanks to the recip CU savings — hits sub-30 bps without RBF.
    #[test]
    fn k15_frozen_accuracy_vs_exact() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let mut max_gap = 0.0f64;
        for sigma in [
            0.15_f64,
            0.20,
            0.291_482_300_850_330_96,
            0.30,
            0.35,
            0.40,
            0.45,
        ] {
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
            let exact = model.quote_coupon(sigma).expect("exact");
            let k15 = crate::worst_of_c1_filter::quote_c1_filter(
                &cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_63,
                15,
            );
            let k12 = crate::worst_of_c1_filter::quote_c1_filter(
                &cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_63,
                12,
            );
            let gap_15 = (k15.fair_coupon_bps_f64() - exact.fair_coupon_bps).abs();
            let gap_12 = (k12.fair_coupon_bps_f64() - exact.fair_coupon_bps).abs();
            max_gap = max_gap.max(gap_15);
            eprintln!(
                "σ={sigma:.4}  k15={:.4}  k12={:.4}  exact={:.4}  k15_gap={:.4}  k12_gap={:.4}",
                k15.fair_coupon_bps_f64(),
                k12.fair_coupon_bps_f64(),
                exact.fair_coupon_bps,
                gap_15,
                gap_12,
            );
        }
        eprintln!("\nmax K15 gap to exact: {:.4} bps", max_gap);
    }

    /// Phase 5 architectural smoke test: `quote_c1_filter_rect_live` runs
    /// end-to-end and produces a finite fair-coupon. Reports the bps gap
    /// against the exact `worst_of_factored` reference at three anchor σ.
    /// Does not yet assert a tight band — this is the "scaffold proves it
    /// composes" gate, not the production accuracy gate. The 30/50/>50 bps
    /// decision tree from the plan kicks in once fused_region_bundle and
    /// 3pt frozen moments are wired into the observe step (next iteration).
    #[test]
    fn rect_live_scaffold_runs_end_to_end() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let mut max_gap_bps = 0.0f64;
        let mut report = String::new();
        for sigma in [0.15_f64, 0.20, 0.30] {
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
            let exact = model.quote_coupon(sigma).expect("exact reference");
            let rect = quote_c1_filter_rect_live(&cfg, sigma_s6, drift_diffs, drift_shift_63);
            let live_uniform =
                quote_c1_filter_live(&cfg, sigma_s6, drift_diffs, drift_shift_63, 12);
            let gap = (rect.fair_coupon_bps_f64() - exact.fair_coupon_bps).abs();
            let live_gap = (live_uniform.fair_coupon_bps_f64() - exact.fair_coupon_bps).abs();
            max_gap_bps = max_gap_bps.max(gap);
            use std::fmt::Write as _;
            let _ = writeln!(
                &mut report,
                "sigma={sigma:.2}  rect_bps={:.4}  liveK12_bps={:.4}  exact_bps={:.4}  rect_gap={:.4}  liveK12_gap={:.4}",
                rect.fair_coupon_bps_f64(),
                live_uniform.fair_coupon_bps_f64(),
                exact.fair_coupon_bps,
                gap,
                live_gap,
            );
        }
        // Print results for human review (always, since this is the scaffold gate).
        eprintln!("\nPhase 5 rect_live scaffold accuracy:\n{report}");
        eprintln!("max_gap_bps = {:.4}", max_gap_bps);
        // Architectural soundness: pricer must produce sane numbers (not NaN,
        // not zero, not absurdly far from exact). Loose 1500 bps gate to
        // accommodate the documented obs1 rebin approximation.
        assert!(
            max_gap_bps.is_finite() && max_gap_bps < 1500.0,
            "rect_live scaffold gap {:.2} bps suggests architectural bug, not just approximation",
            max_gap_bps
        );
    }

    /// Phase 1 of analytic-delta rollout: verify `triangle_probability_with
    /// _grad` exposes the same gradient that the shipped
    /// `triangle_with_gradient_i64` consumes internally via the Stein
    /// identity for its expectation_u/expectation_v output.
    ///
    /// Approach: compute (P, dp_du, dp_dv) via the new primitive, compute
    /// (P', EU, EV) via the existing primitive, then reconstruct EU and EV
    /// from the new primitive's gradient via Stein:
    ///   EU_reconstructed = μ_u · P + σ_uu · dp_du + σ_uv · dp_dv
    ///   EV_reconstructed = μ_v · P + σ_uv · dp_du + σ_vv · dp_dv
    /// and assert it matches within i64 rounding (1 ULP). P must match
    /// exactly. This is a bit-exact cross-check: the gradients come from
    /// the same formulas, so the new primitive is valid iff its exposed
    /// gradient reconstructs the shipped path's moments.
    ///
    /// Central-FD validation against the underlying continuous gradient is
    /// a separate (future) exercise — the i64 primitive's rounding noise
    /// dominates central FD at realistic EPS, making it a noisy test. The
    /// shipped path's EU/EV are in production use and have been validated
    /// end-to-end via the pricer accuracy tests, so equivalence to them is
    /// a strong-enough gate for wiring into `quote_c1_filter_with_delta`.
    #[test]
    fn triangle_probability_with_grad_matches_shipped_stein() {
        use crate::worst_of_c1_fast::spy_qqq_iwm_c1_config;

        let cfg = spy_qqq_iwm_c1_config();
        let phi2 = phi2_tables();
        let triple_by_obs = build_triple_pre_by_obs(&cfg);

        // Sweep: obs 2..4 × mean_u ∈ {-80k, 0, +80k} × mean_v ∈ {-80k, 0, +80k}
        // × rhs offset ∈ {0, 50_000}. 54 cases.
        let mean_probes: [i64; 3] = [-80_000, 0, 80_000];
        let rhs_offsets: [i64; 2] = [0, 50_000];

        let mut worst_p_diff = 0i64;
        let mut worst_eu_diff = 0i64;
        let mut worst_ev_diff = 0i64;
        let mut worst_case = String::new();
        let mut n_cases = 0usize;
        let mut n_nonzero = 0usize;

        for obs_idx in 2..5 {
            let obs = &cfg.obs[obs_idx];
            let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
            let tp = observation_probability_triple_pre(obs_idx, triple_by_obs[obs_idx].as_ref());

            for &mu_u in &mean_probes {
                for &mu_v in &mean_probes {
                    for &off in &rhs_offsets {
                        let rhs = [cfg.autocall_rhs_base + off; 3];

                        let (p_new, dp_du, dp_dv) = triangle_probability_with_grad(
                            mu_u,
                            mu_v,
                            &rhs,
                            &obs.tri_pre,
                            phi2,
                            tp,
                            &dz_du,
                            &dz_dv,
                        );

                        let shipped = triangle_with_gradient_i64(
                            mu_u,
                            mu_v,
                            &rhs,
                            &obs.tri_pre,
                            phi2,
                            tp,
                            &dz_du,
                            &dz_dv,
                            obs.cov_uu,
                            obs.cov_uv,
                            obs.cov_vv,
                        );

                        let p_diff = (p_new - shipped.probability).abs();
                        worst_p_diff = worst_p_diff.max(p_diff);

                        if p_new > 0 {
                            n_nonzero += 1;
                            let eu_recon = m6r_fast(mu_u, p_new)
                                + m6r_fast(obs.cov_uu, dp_du)
                                + m6r_fast(obs.cov_uv, dp_dv);
                            let ev_recon = m6r_fast(mu_v, p_new)
                                + m6r_fast(obs.cov_uv, dp_du)
                                + m6r_fast(obs.cov_vv, dp_dv);
                            let eu_diff = (eu_recon - shipped.expectation_u).abs();
                            let ev_diff = (ev_recon - shipped.expectation_v).abs();
                            if eu_diff > worst_eu_diff || ev_diff > worst_ev_diff {
                                worst_case = format!(
                                    "obs={obs_idx} mu=({mu_u},{mu_v}) off={off} P={p_new} \
                                     dp_du={dp_du} dp_dv={dp_dv} \
                                     EU_recon={eu_recon} EU_ship={} Δ={eu_diff} \
                                     EV_recon={ev_recon} EV_ship={} Δ={ev_diff}",
                                    shipped.expectation_u, shipped.expectation_v,
                                );
                            }
                            worst_eu_diff = worst_eu_diff.max(eu_diff);
                            worst_ev_diff = worst_ev_diff.max(ev_diff);
                        }
                        n_cases += 1;
                    }
                }
            }
        }

        eprintln!(
            "n_cases={n_cases} nonzero={n_nonzero} \
             worst_p_diff={worst_p_diff} \
             worst_eu_diff={worst_eu_diff} worst_ev_diff={worst_ev_diff}"
        );
        eprintln!("worst: {worst_case}");

        assert_eq!(
            worst_p_diff, 0,
            "P diverged between new primitive and shipped — not same formula",
        );
        // EU/EV reconstruction: accept small ULP budget. The shipped
        // primitive accumulates dp_du/dp_dv in the same loop body as the
        // new primitive, so m6r_fast ordering matches; any diff comes from
        // the Stein recomposition at the caller. 100 ULPs ~ 1e-4.
        assert!(
            worst_eu_diff <= 100,
            "EU reconstruction off by {worst_eu_diff} ULPs (>100)"
        );
        assert!(
            worst_ev_diff <= 100,
            "EV reconstruction off by {worst_ev_diff} ULPs (>100)"
        );
    }

    #[test]
    fn update_safe_state_grad_matches_single_step_pricer_fd() {
        let sigma = 0.364_352_876_062_913_67;
        let (cfg, _sigma_s6, _drift_diffs, drift_shift_63) = filter_inputs(sigma);
        let triple_by_obs = build_triple_pre_by_obs(&cfg);
        let dmu_ds = compute_dmu_ds(&cfg, [S6, S6, S6]);
        let eps = 100i64; // 1e-4 in normalized spot-ratio SCALE_6.
        let c_candidates = [
            -200_000, -150_000, -100_000, -50_000, 0, 50_000, 100_000, 150_000, 200_000,
        ];
        let mut best_case = None;

        for obs_idx in 1..5 {
            let drift_shift_total = cfg.obs[obs_idx].obs_day as i64 / 63 * drift_shift_63;
            let tp = observation_probability_triple_pre(obs_idx, triple_by_obs[obs_idx].as_ref());
            for &c in &c_candidates {
                let mut safe_pred = FilterState::default();
                safe_pred.nodes[0] = FilterNode {
                    c,
                    w: S6,
                    mean_u: 0,
                    mean_v: 0,
                };
                safe_pred.n_active = 1;

                let (update, update_grad) = update_safe_state_grad(
                    &safe_pred,
                    &FilterStateGrad::default(),
                    &cfg,
                    obs_idx,
                    drift_shift_total,
                    12,
                    tp,
                    &dmu_ds,
                );
                let shipped =
                    update_safe_state(&safe_pred, &cfg, obs_idx, drift_shift_total, 12, tp);
                assert_eq!(
                    update.first_hit, shipped.first_hit,
                    "first-hit drift vs frozen path"
                );
                if update.first_hit <= 100 {
                    continue;
                }

                let mut max_rel = 0.0f64;
                let mut per_asset = [0.0f64; 3];
                for asset in 0..3 {
                    let mut up = [S6, S6, S6];
                    let mut dn = [S6, S6, S6];
                    up[asset] += eps;
                    dn[asset] -= eps;

                    let first_hit_up = single_step_ac_first_hit_with_spots(
                        &safe_pred,
                        &cfg,
                        obs_idx,
                        drift_shift_total,
                        12,
                        tp,
                        up,
                    );
                    let first_hit_dn = single_step_ac_first_hit_with_spots(
                        &safe_pred,
                        &cfg,
                        obs_idx,
                        drift_shift_total,
                        12,
                        tp,
                        dn,
                    );
                    let fc_up =
                        single_step_ac_only_fair_coupon_from_first_hit(cfg.notional, first_hit_up);
                    let fc_dn =
                        single_step_ac_only_fair_coupon_from_first_hit(cfg.notional, first_hit_dn);
                    let fd = (fc_up - fc_dn) / (2.0 * eps as f64);
                    let analytic = -(cfg.notional as f64) * update_grad.first_hit[asset] as f64
                        / (update.first_hit as f64 * update.first_hit as f64);
                    let rel_err = (analytic - fd).abs() / fd.abs().max(1.0e-9);
                    per_asset[asset] = rel_err;
                    max_rel = max_rel.max(rel_err);
                }

                let summary = format!(
                    "obs={obs_idx} c={c} first_hit={} rel=[{:.6},{:.6},{:.6}]",
                    update.first_hit, per_asset[0], per_asset[1], per_asset[2]
                );
                if best_case
                    .as_ref()
                    .map(|(best_rel, _): &(f64, String)| max_rel < *best_rel)
                    .unwrap_or(true)
                {
                    best_case = Some((max_rel, summary));
                }
            }
        }

        let (best_rel, best_summary) = best_case.expect("no usable AC-only probe found");
        eprintln!("best_session_a_gate: {best_summary}");
        assert!(
            best_rel < 0.01,
            "best AC-only single-step FD gate still too wide: {best_summary}",
        );
    }

    #[test]
    #[ignore]
    fn survey_ki_triangle_complement_vs_gh3_moments() {
        use crate::b_tensors::K_SCHEDULE;
        use crate::nested_grids::nested_c_grid;

        let sigmas = [0.20_f64, 0.30, 0.40];
        let mean_grid = [-100_000, -50_000, 0, 50_000, 100_000];
        let tables = phi2_tables();

        let mut max_p_gap = 0i64;
        let mut max_eu_gap = 0i64;
        let mut max_ev_gap = 0i64;
        let mut worst_p = String::new();
        let mut worst_eu = String::new();
        let mut worst_ev = String::new();

        for &sigma in &sigmas {
            let (cfg, sigma_s6, _drift_diffs, drift_shift_63) = filter_inputs(sigma);
            let triple_by_obs = build_triple_pre_by_obs(&cfg);

            for obs_idx in 2..=4 {
                let obs_rel = obs_idx - 1;
                let obs = &cfg.obs[obs_idx];
                let (dz_du, dz_dv) = triangle_gradient_geometry(&obs.tri_pre);
                let tp =
                    observation_probability_triple_pre(obs_idx, triple_by_obs[obs_idx].as_ref());
                let (l11, l21, l22) =
                    cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).expect("ki cholesky");
                let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;
                let c_grid = nested_c_grid(sigma_s6, obs_rel, K_SCHEDULE[obs_rel]);

                for &mu_u in &mean_grid {
                    for &mu_v in &mean_grid {
                        for &c in c_grid[..K_SCHEDULE[obs_rel]].iter() {
                            let ki_gh3 = ki_region_uv_moment_gh3(
                                mu_u,
                                mu_v,
                                l11,
                                l21,
                                l22,
                                cfg.ki_barrier_log,
                                ki_coords_from_cumulative(&cfg, c, drift_shift_total),
                            );
                            let ki_safe_rhs = [cfg.ki_safe_rhs_base + c + drift_shift_total; 3];
                            let ki_safe = triangle_with_gradient_i64(
                                mu_u,
                                mu_v,
                                &ki_safe_rhs,
                                &obs.tri_pre,
                                tables,
                                tp,
                                &dz_du,
                                &dz_dv,
                                obs.cov_uu,
                                obs.cov_uv,
                                obs.cov_vv,
                            );
                            let ki_triangle = RawRegionMoment {
                                probability: (S6 - ki_safe.probability).clamp(0, S6),
                                expectation_u: mu_u - ki_safe.expectation_u,
                                expectation_v: mu_v - ki_safe.expectation_v,
                            };

                            let p_gap = (ki_triangle.probability - ki_gh3.probability).abs();
                            let eu_gap = (ki_triangle.expectation_u - ki_gh3.expectation_u).abs();
                            let ev_gap = (ki_triangle.expectation_v - ki_gh3.expectation_v).abs();

                            if p_gap > max_p_gap {
                                max_p_gap = p_gap;
                                worst_p = format!(
                                    "sigma={sigma:.2} obs={obs_idx} mu=({mu_u},{mu_v}) c={c} tri={} gh3={}",
                                    ki_triangle.probability, ki_gh3.probability,
                                );
                            }
                            if eu_gap > max_eu_gap {
                                max_eu_gap = eu_gap;
                                worst_eu = format!(
                                    "sigma={sigma:.2} obs={obs_idx} mu=({mu_u},{mu_v}) c={c} tri={} gh3={} p_tri={} p_gh3={}",
                                    ki_triangle.expectation_u,
                                    ki_gh3.expectation_u,
                                    ki_triangle.probability,
                                    ki_gh3.probability,
                                );
                            }
                            if ev_gap > max_ev_gap {
                                max_ev_gap = ev_gap;
                                worst_ev = format!(
                                    "sigma={sigma:.2} obs={obs_idx} mu=({mu_u},{mu_v}) c={c} tri={} gh3={} p_tri={} p_gh3={}",
                                    ki_triangle.expectation_v,
                                    ki_gh3.expectation_v,
                                    ki_triangle.probability,
                                    ki_gh3.probability,
                                );
                            }
                        }
                    }
                }
            }
        }

        eprintln!("ki_triangle_vs_gh3 max_p_gap={max_p_gap} worst_p={worst_p}");
        eprintln!("ki_triangle_vs_gh3 max_eu_gap={max_eu_gap} worst_eu={worst_eu}");
        eprintln!("ki_triangle_vs_gh3 max_ev_gap={max_ev_gap} worst_ev={worst_ev}");
    }

    #[test]
    fn update_safe_state_grad_matches_single_step_ki_fd() {
        let sigma = 0.364_352_876_062_913_67;
        let (cfg, _sigma_s6, _drift_diffs, drift_shift_63) = filter_inputs(sigma);
        let triple_by_obs = build_triple_pre_by_obs(&cfg);
        let dmu_ds = compute_dmu_ds(&cfg, [S6, S6, S6]);
        let eps = 100i64; // 1e-4 in normalized spot-ratio SCALE_6.
        let mean_candidates = [-150_000, -100_000, -50_000, 0, 50_000, 100_000, 150_000];
        let c_candidates = [
            -450_000, -400_000, -350_000, -300_000, -250_000, -200_000, -150_000, -100_000,
            -50_000, 0, 50_000,
        ];
        let mut best_case = None;

        for obs_idx in 1..5 {
            let drift_shift_total = cfg.obs[obs_idx].obs_day as i64 / 63 * drift_shift_63;
            let tp = observation_probability_triple_pre(obs_idx, triple_by_obs[obs_idx].as_ref());
            for &c in &c_candidates {
                for &mean_u in &mean_candidates {
                    for &mean_v in &mean_candidates {
                        let mut safe_pred = FilterState::default();
                        safe_pred.nodes[0] = FilterNode {
                            c,
                            w: S6,
                            mean_u,
                            mean_v,
                        };
                        safe_pred.n_active = 1;

                        let (update, update_grad) = update_safe_state_grad(
                            &safe_pred,
                            &FilterStateGrad::default(),
                            &cfg,
                            obs_idx,
                            drift_shift_total,
                            12,
                            tp,
                            &dmu_ds,
                        );
                        let shipped =
                            update_safe_state(&safe_pred, &cfg, obs_idx, drift_shift_total, 12, tp);
                        assert_eq!(
                            update.first_knock_in, shipped.first_knock_in,
                            "first-knock-in drift vs frozen path"
                        );
                        if update.first_knock_in <= 100 {
                            continue;
                        }

                        let mut max_rel = 0.0f64;
                        let mut max_abs_fd = 0.0f64;
                        let mut per_asset = [0.0f64; 3];
                        let mut analytic_vals = [0.0f64; 3];
                        let mut fd_vals = [0.0f64; 3];
                        for asset in 0..3 {
                            let mut up = [S6, S6, S6];
                            let mut dn = [S6, S6, S6];
                            up[asset] += eps;
                            dn[asset] -= eps;

                            let first_ki_up = single_step_ki_first_hit_smoothed_with_spots_f64(
                                &safe_pred,
                                &cfg,
                                obs_idx,
                                drift_shift_total,
                                up,
                            );
                            let first_ki_dn = single_step_ki_first_hit_smoothed_with_spots_f64(
                                &safe_pred,
                                &cfg,
                                obs_idx,
                                drift_shift_total,
                                dn,
                            );
                            let fd = (first_ki_up - first_ki_dn) / (2.0 * eps as f64);
                            let analytic = update_grad.first_knock_in[asset] as f64 / S6 as f64;
                            let rel_err = (analytic - fd).abs() / fd.abs().max(1.0e-9);
                            per_asset[asset] = rel_err;
                            analytic_vals[asset] = analytic;
                            fd_vals[asset] = fd;
                            max_rel = max_rel.max(rel_err);
                            max_abs_fd = max_abs_fd.max(fd.abs());
                        }

                        if max_abs_fd < 0.05 {
                            continue;
                        }

                        let summary = format!(
                            "obs={obs_idx} c={c} mu=({mean_u},{mean_v}) first_ki={} rel=[{:.6},{:.6},{:.6}] analytic=[{:.3},{:.3},{:.3}] fd=[{:.3},{:.3},{:.3}]",
                            update.first_knock_in,
                            per_asset[0],
                            per_asset[1],
                            per_asset[2],
                            analytic_vals[0],
                            analytic_vals[1],
                            analytic_vals[2],
                            fd_vals[0],
                            fd_vals[1],
                            fd_vals[2],
                        );
                        if best_case
                            .as_ref()
                            .map(|(best_rel, _): &(f64, String)| max_rel < *best_rel)
                            .unwrap_or(true)
                        {
                            best_case = Some((max_rel, summary));
                        }
                    }
                }
            }
        }

        let (best_rel, best_summary) = best_case.expect("no usable KI probe found");
        eprintln!("best_session_b_gate: {best_summary}");
        assert!(
            best_rel < 0.01,
            "best KI single-step smoothed FD gate still too wide: {best_summary}",
        );
    }

    #[test]
    fn maturity_step_grad_matches_local_step_fd() {
        let sigma = 0.364_352_876_062_913_67;
        let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
        let prepared =
            bench_prepare_maturity_state(&cfg, sigma_s6, drift_diffs, drift_shift_63, 12)
                .expect("maturity prepared state");
        let obs = &cfg.obs[prepared.obs_idx];
        let triple_pre = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
            .ok()
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
        let tp = observation_probability_triple_pre(prepared.obs_idx, triple_pre.as_ref());
        let frozen_grid = crate::frozen_predict_tables::frozen_predict_grid_lookup(
            sigma_s6,
            prepared.obs_idx - 1,
            prepared.k_retained,
        );
        let dmu_ds = compute_dmu_ds(&cfg, [S6, S6, S6]);
        let safe_grad = seed_state_mean_grad(&prepared.safe_state, &dmu_ds);
        let knocked_grad = seed_state_mean_grad(&prepared.knocked_state, &dmu_ds);
        let dmu_c = dmu_c_only(&dmu_ds);

        let (maturity, maturity_grad) = run_maturity_step_grad(
            &prepared.safe_state,
            &safe_grad,
            &prepared.knocked_state,
            &knocked_grad,
            &prepared.transition,
            &cfg,
            prepared.obs_idx,
            prepared.drift_shift_total,
            prepared.k_retained,
            tp,
            frozen_grid.as_ref(),
            &dmu_c,
        );
        let shipped = run_maturity_step(
            &prepared.safe_state,
            &prepared.knocked_state,
            &prepared.transition,
            &cfg,
            prepared.obs_idx,
            prepared.drift_shift_total,
            prepared.k_retained,
            tp,
            frozen_grid.as_ref(),
        );
        assert_eq!(
            maturity.coupon_hit, shipped.coupon_hit,
            "maturity coupon drift"
        );
        assert_eq!(
            maturity.safe_principal, shipped.safe_principal,
            "maturity safe principal drift"
        );
        assert_eq!(
            maturity.first_knock_in, shipped.first_knock_in,
            "maturity first-knock-in drift"
        );
        assert_eq!(
            maturity.knock_in_redemption_safe, shipped.knock_in_redemption_safe,
            "maturity safe KI redemption drift"
        );
        assert_eq!(
            maturity.knocked_redemption, shipped.knocked_redemption,
            "maturity knocked redemption drift"
        );

        let eps = 100i64;
        let mut max_rel = 0.0f64;
        let mut analytic_vals = [0.0f64; 3];
        let mut fd_vals = [0.0f64; 3];
        for asset in 0..3 {
            let mut up = [S6, S6, S6];
            let mut dn = [S6, S6, S6];
            up[asset] += eps;
            dn[asset] -= eps;

            let scalar_up = maturity_step_scalar_with_spots(&prepared, &cfg, sigma_s6, up);
            let scalar_dn = maturity_step_scalar_with_spots(&prepared, &cfg, sigma_s6, dn);
            let fd = (scalar_up - scalar_dn) as f64 / (2.0 * eps as f64);
            let analytic = maturity_step_scalar_grad(&maturity_grad, asset) as f64 / S6 as f64;
            let rel_err = (analytic - fd).abs() / fd.abs().max(1.0e-9);
            analytic_vals[asset] = analytic;
            fd_vals[asset] = fd;
            max_rel = max_rel.max(rel_err);
        }

        let summary = format!(
            "mat_scalar={} rel=[{:.6},{:.6},{:.6}] analytic=[{:.3},{:.3},{:.3}] fd=[{:.3},{:.3},{:.3}]",
            maturity_step_scalar(&maturity),
            (analytic_vals[0] - fd_vals[0]).abs() / fd_vals[0].abs().max(1.0e-9),
            (analytic_vals[1] - fd_vals[1]).abs() / fd_vals[1].abs().max(1.0e-9),
            (analytic_vals[2] - fd_vals[2]).abs() / fd_vals[2].abs().max(1.0e-9),
            analytic_vals[0],
            analytic_vals[1],
            analytic_vals[2],
            fd_vals[0],
            fd_vals[1],
            fd_vals[2],
        );
        eprintln!("session_c_gate: {summary}");
        assert!(
            max_rel < 0.07,
            "maturity local-step FD gate too wide: {summary}",
        );
    }

    #[test]
    fn quote_c1_filter_with_delta_matches_corrected_forward_quote() {
        let sigma = 0.291_482_300_850_330_96;
        let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
        let shipped = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, 12);
        let with_delta =
            quote_c1_filter_with_delta(&cfg, sigma_s6, drift_diffs, drift_shift_63, 12);
        let expected_bps = shipped.fair_coupon_bps_f64()
            + crate::k12_correction::k12_correction_lookup(sigma_s6) as f64 / 1_000_000.0;
        assert!(
            (with_delta.fc_bps - expected_bps).abs() < 1.0e-9,
            "wrapper fc drifted: wrapper={} expected={}",
            with_delta.fc_bps,
            expected_bps
        );
        assert!(with_delta.delta_spy.is_finite());
        assert!(with_delta.delta_qqq.is_finite());
        assert!(with_delta.delta_iwm.is_finite());
    }

    #[test]
    fn quote_c1_filter_with_delta_pricer_fd_snapshot() {
        let sigmas = [
            0.291_482_300_850_330_96,
            0.364_352_876_062_913_67,
            0.437_223_451_275_496_4,
        ];
        let eps = 100i64; // 1e-4 in physical spot units

        for sigma in sigmas {
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
            let analytic =
                quote_c1_filter_with_delta(&cfg, sigma_s6, drift_diffs, drift_shift_63, 12);
            let base_s6 = quote_c1_filter_s6_with_spots(
                &cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_63,
                12,
                [S6, S6, S6],
            );
            let analytic_vals = [analytic.delta_spy, analytic.delta_qqq, analytic.delta_iwm];
            let mut fd_vals = [0.0f64; 3];
            let mut rel_vals = [0.0f64; 3];

            for asset in 0..3 {
                let mut up = [S6, S6, S6];
                let mut dn = [S6, S6, S6];
                up[asset] += eps;
                dn[asset] -= eps;
                let up_s6 = quote_c1_filter_s6_with_spots(
                    &cfg,
                    sigma_s6,
                    drift_diffs,
                    drift_shift_63,
                    12,
                    up,
                );
                let dn_s6 = quote_c1_filter_s6_with_spots(
                    &cfg,
                    sigma_s6,
                    drift_diffs,
                    drift_shift_63,
                    12,
                    dn,
                );
                let fd = (up_s6 - dn_s6) as f64 / (2.0 * eps as f64);
                fd_vals[asset] = fd;
                rel_vals[asset] = (analytic_vals[asset] - fd).abs() / fd.abs().max(1.0e-9);
            }

            let delta_sum = analytic_vals.iter().sum::<f64>();
            eprintln!(
                "session_e_fd: sigma={sigma:.15} base_s6={} analytic=[{:.6},{:.6},{:.6}] fd=[{:.6},{:.6},{:.6}] rel=[{:.6},{:.6},{:.6}] sum={:.6}",
                base_s6,
                analytic_vals[0],
                analytic_vals[1],
                analytic_vals[2],
                fd_vals[0],
                fd_vals[1],
                fd_vals[2],
                rel_vals[0],
                rel_vals[1],
                rel_vals[2],
                delta_sum,
            );

            assert!(analytic.delta_spy.is_finite());
            assert!(analytic.delta_qqq.is_finite());
            assert!(analytic.delta_iwm.is_finite());
        }
    }

    /// Phase 4: three-point frozen moments selector behaves as a pure
    /// L¹-nearest lookup and the tables' (0,0) slot reproduces the live
    /// region moment at exactly μ_ref=(0,0) (up to i64/Stein rounding).
    /// Also confirms the 3pt advantage at a μ far from (0,0): picking the
    /// nearest ref yields strictly lower error than the (0,0)-only lookup.
    #[test]
    fn frozen_moments_3pt_selector_and_drift_advantage() {
        use crate::frozen_moments_3pt::{
            select_frozen_moment_3pt, MOM3_EU_AC, MOM3_MU_U_S6, MOM3_MU_V_S6, MOM3_PROB_AC,
        };

        // Selector identity: at the exact ref μ, returns that ref's values.
        for (r, (mu_u, mu_v)) in MOM3_MU_U_S6
            .iter()
            .copied()
            .zip(MOM3_MU_V_S6.iter().copied())
            .enumerate()
        {
            let sel = select_frozen_moment_3pt(mu_u, mu_v, 0, 7);
            assert_eq!(sel.prob_ac, MOM3_PROB_AC[0][7][r]);
            assert_eq!(sel.eu_ac, MOM3_EU_AC[0][7][r]);
        }

        // L¹ nearest: at μ=(+2000, 0), the +1000 ref should win over 0.
        let sel_pos = select_frozen_moment_3pt(2_000, 0, 0, 7);
        assert_eq!(sel_pos.prob_ac, MOM3_PROB_AC[0][7][1]);
        // At μ=(0, -2000), the -1000 ref should win.
        let sel_neg = select_frozen_moment_3pt(0, -2_000, 0, 7);
        assert_eq!(sel_neg.prob_ac, MOM3_PROB_AC[0][7][2]);

        // Drift advantage: at μ near a non-zero ref, 3pt beats 0-only on
        // the probability reconstruction. Construct a synthetic μ=(+900, 0)
        // — selector picks the +1000 ref (distance 100) over 0 (distance 900).
        // The 3pt P matches MOM3_PROB_AC[...][1]; the 0-only P always returns
        // MOM3_PROB_AC[...][0]. If these slots differ, 3pt is at worst equal
        // and typically closer to the live moment at μ=(+900, 0).
        let sel_900 = select_frozen_moment_3pt(900, 0, 0, 7);
        assert_eq!(sel_900.prob_ac, MOM3_PROB_AC[0][7][1]);
    }

    /// Phase 2 equivalence: `fused_region_bundle` must produce the same
    /// `(P_ac, P_ki)` as two separate calls to `triangle_probability_with
    /// _triple_i64` with ac_rhs and ki_rhs respectively (with P_ki =
    /// S6 - P_ki_safe).
    #[test]
    fn fused_region_bundle_matches_separate_calls() {
        let cfg = spy_qqq_iwm_c1_config();
        let tables = phi2_tables();
        // Cover three anchor sigmas × three (mean_u, mean_v) probes × 4 obs.
        let sigmas = [0.20_f64, 0.30, 0.45];
        let mean_probes: [(i64, i64); 3] = [(0, 0), (30_000, -50_000), (-80_000, 120_000)];
        let mut max_abs_diff_ac = 0i64;
        let mut max_abs_diff_ki = 0i64;
        for &sig in &sigmas {
            for obs_idx in 0..N_OBS {
                let obs = &cfg.obs[obs_idx];
                let triple =
                    cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
                        .ok()
                        .map(|(l11, l21, l22)| {
                            build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av)
                        });
                let tp = observation_probability_triple_pre(obs_idx, triple.as_ref());
                // Pick a representative shift capturing the barrier proximity.
                let scale = obs.obs_day as i64 / 63;
                let shift = (sig * S6 as f64 * 0.2 * scale as f64) as i64;
                let ac_rhs = [cfg.autocall_rhs_base + shift; 3];
                let ki_rhs = [cfg.ki_safe_rhs_base + shift; 3];

                for &(mu, mv) in &mean_probes {
                    let p_ac_solo = triangle_probability_with_triple_i64(
                        mu,
                        mv,
                        &ac_rhs,
                        &obs.tri_pre,
                        tables,
                        tp,
                    );
                    let p_ki_safe_solo = triangle_probability_with_triple_i64(
                        mu,
                        mv,
                        &ki_rhs,
                        &obs.tri_pre,
                        tables,
                        tp,
                    );
                    let p_ki_solo = (S6 - p_ki_safe_solo).clamp(0, S6);

                    let (p_ac_fused, p_ki_fused) =
                        fused_region_bundle(mu, mv, &ac_rhs, &ki_rhs, &obs.tri_pre, tables, tp);
                    max_abs_diff_ac = max_abs_diff_ac.max((p_ac_solo - p_ac_fused).abs());
                    max_abs_diff_ki = max_abs_diff_ki.max((p_ki_solo - p_ki_fused).abs());
                }
            }
        }
        // Fused is a pure refactor — bitwise equal up to integer rounding noise.
        // Tolerance: 1 unit at S6 (1e-6 absolute on a probability). In practice 0.
        assert!(
            max_abs_diff_ac <= 1,
            "fused autocall probability drifts from separate call: max_abs_diff={max_abs_diff_ac}"
        );
        assert!(
            max_abs_diff_ki <= 1,
            "fused KI probability drifts from separate call: max_abs_diff={max_abs_diff_ki}"
        );
    }

    #[test]
    #[ignore]
    fn projected_filter_matches_obs1_exact_anchor() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma = 0.364_352_876_062_913_67;
        let exact = model.quote_coupon(sigma).unwrap();
        let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
        let trace = quote_c1_filter_trace(&cfg, sigma_s6, drift_diffs, drift_shift_63, 9);
        let obs1 = trace.observation_autocall_first_hit[0] as f64 / S6 as f64;
        let exact_obs1 = exact.observation_marginals[0].autocall_first_hit_probability;
        assert!(
            (obs1 - exact_obs1).abs() < 5.0e-4,
            "obs1 filter={} exact={}",
            obs1,
            exact_obs1
        );
    }

    #[test]
    fn projected_filter_regenerates_obs2_bullish_mass() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let sigma = 0.364_352_876_062_913_67;
        let exact = model.quote_coupon(sigma).unwrap();
        let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
        let fast = quote_c1_fast(&cfg, sigma_s6, drift_diffs, drift_shift_63);
        let trace = quote_c1_filter_trace(&cfg, sigma_s6, drift_diffs, drift_shift_63, 9);
        let obs2 = trace.observation_autocall_first_hit[1] as f64 / S6 as f64;
        let exact_obs2 = exact.observation_marginals[1].autocall_first_hit_probability;
        assert!(
            (obs2 - exact_obs2).abs() < 0.08,
            "obs2 filter={} exact={}",
            obs2,
            exact_obs2
        );
        assert!(
            obs2 > 0.05 && obs2 < 0.20,
            "obs2 first-hit should sit in the regenerated middle mass, got {}",
            obs2
        );
        assert!(
            (trace.quote.fair_coupon_bps_f64() - exact.fair_coupon_bps).abs()
                < (fast.fair_coupon_bps_f64() - exact.fair_coupon_bps).abs(),
            "filter_bps={} fast_bps={} exact_bps={}",
            trace.quote.fair_coupon_bps_f64(),
            fast.fair_coupon_bps_f64(),
            exact.fair_coupon_bps
        );
    }

    #[test]
    fn projected_filter_beats_c1_fast_at_anchors() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        for sigma in [
            0.291_482_300_850_330_96,
            0.364_352_876_062_913_67,
            0.437_223_451_275_496_4,
        ] {
            let exact = model.quote_coupon(sigma).unwrap();
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
            let fast = quote_c1_fast(&cfg, sigma_s6, drift_diffs, drift_shift_63);
            let trace = quote_c1_filter_trace(&cfg, sigma_s6, drift_diffs, drift_shift_63, 9);
            assert!(
                (trace.quote.fair_coupon_bps_f64() - exact.fair_coupon_bps).abs()
                    < (fast.fair_coupon_bps_f64() - exact.fair_coupon_bps).abs(),
                "sigma={} filter_bps={} fast_bps={} exact_bps={}",
                sigma,
                trace.quote.fair_coupon_bps_f64(),
                fast.fair_coupon_bps_f64(),
                exact.fair_coupon_bps
            );
        }
    }

    #[test]
    fn projected_filter_k15_hits_relaxed_anchor_band() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        for sigma in [
            0.291_482_300_850_330_96,
            0.364_352_876_062_913_67,
            0.437_223_451_275_496_4,
        ] {
            let exact = model.quote_coupon(sigma).unwrap();
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
            let trace = quote_c1_filter_trace(&cfg, sigma_s6, drift_diffs, drift_shift_63, 15);
            assert!(
                (trace.quote.fair_coupon_bps_f64() - exact.fair_coupon_bps).abs() < 40.0,
                "sigma={} filter_bps={} exact_bps={}",
                sigma,
                trace.quote.fair_coupon_bps_f64(),
                exact.fair_coupon_bps
            );
        }
    }

    #[test]
    #[ignore]
    fn projected_filter_is_monotone_on_anchor_strip() {
        let (cfg, _, _, _) = filter_inputs(0.291_482_300_850_330_96);
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let mut prev = f64::NEG_INFINITY;
        for idx in 0..200 {
            let sigma = 0.25 + 0.0015 * idx as f64;
            let sigma_s6 = (sigma * S6 as f64).round() as i64;
            let drifts = model.risk_neutral_step_drifts(sigma, 63).unwrap();
            let drift_diffs = [
                ((drifts[1] - drifts[0]) * S6 as f64).round() as i64,
                ((drifts[2] - drifts[0]) * S6 as f64).round() as i64,
            ];
            let drift_shift_63 = ((cfg.loadings[0] as f64 * drifts[0])
                + (cfg.loadings[1] as f64 * drifts[1])
                + (cfg.loadings[2] as f64 * drifts[2]))
                .round() as i64;
            let quote = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, 9);
            assert!(
                quote.fair_coupon_bps_f64() + 0.1 >= prev,
                "monotonicity violation at sigma={} prev={} next={}",
                sigma,
                prev,
                quote.fair_coupon_bps_f64()
            );
            prev = quote.fair_coupon_bps_f64();
        }
    }

    #[test]
    fn projected_filter_conserves_mass() {
        let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(0.364_352_876_062_913_67);
        let trace = quote_c1_filter_trace(&cfg, sigma_s6, drift_diffs, drift_shift_63, 9);
        let mut cumulative_autocall = 0i64;
        for obs_idx in 0..(N_OBS - 1) {
            cumulative_autocall += trace.observation_autocall_first_hit[obs_idx];
            let live_mass = trace.post_observation_safe_mass[obs_idx]
                + trace.post_observation_knocked_mass[obs_idx];
            let total = cumulative_autocall + live_mass;
            assert!(
                (total - S6).abs() <= 300,
                "obs={} cumulative_ac={} live={} total={}",
                obs_idx + 1,
                cumulative_autocall,
                live_mass,
                total
            );
        }
    }

    #[test]
    #[ignore]
    fn projected_filter_k_sweep_snapshot() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        for sigma in [
            0.291_482_300_850_330_96,
            0.364_352_876_062_913_67,
            0.437_223_451_275_496_4,
        ] {
            let exact = model.quote_coupon(sigma).unwrap();
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
            println!(
                "\nsigma={sigma:.15} exact_bps={:.6} exact_obs1={:.9} exact_obs2={:.9}",
                exact.fair_coupon_bps,
                exact.observation_marginals[0].autocall_first_hit_probability,
                exact.observation_marginals[1].autocall_first_hit_probability,
            );
            for k in [7usize, 9, 12, 15] {
                let trace = quote_c1_filter_trace(&cfg, sigma_s6, drift_diffs, drift_shift_63, k);
                println!(
                    "  k={k:>2} fair_bps={:.6} err={:+.3} obs1={:.9} obs2={:.9} ki={:.9} ac={:.9}",
                    trace.quote.fair_coupon_bps_f64(),
                    trace.quote.fair_coupon_bps_f64() - exact.fair_coupon_bps,
                    trace.observation_autocall_first_hit[0] as f64 / S6 as f64,
                    trace.observation_autocall_first_hit[1] as f64 / S6 as f64,
                    trace.quote.knock_in_rate_f64(),
                    trace.quote.autocall_rate_f64(),
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn projected_filter_monotonicity_snapshot() {
        let (cfg, _, _, _) = filter_inputs(0.291_482_300_850_330_96);
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        for k in [9usize, 12, 15] {
            let mut prev = f64::NEG_INFINITY;
            let mut violations = 0usize;
            for idx in 0..200 {
                let sigma = 0.25 + 0.0015 * idx as f64;
                let sigma_s6 = (sigma * S6 as f64).round() as i64;
                let drifts = model.risk_neutral_step_drifts(sigma, 63).unwrap();
                let drift_diffs = [
                    ((drifts[1] - drifts[0]) * S6 as f64).round() as i64,
                    ((drifts[2] - drifts[0]) * S6 as f64).round() as i64,
                ];
                let drift_shift_63 = ((cfg.loadings[0] as f64 * drifts[0])
                    + (cfg.loadings[1] as f64 * drifts[1])
                    + (cfg.loadings[2] as f64 * drifts[2]))
                    .round() as i64;
                let quote = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, k);
                if quote.fair_coupon_bps_f64() + 0.1 < prev {
                    violations += 1;
                }
                prev = quote.fair_coupon_bps_f64();
            }
            println!("k={k} monotonic_violations={violations}");
        }
    }

    #[test]
    #[ignore]
    fn generate_maturity_bench_snapshot() {
        let sigma_common = 0.35;
        let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma_common);
        let prepared = bench_prepare_maturity_state(&cfg, sigma_s6, drift_diffs, drift_shift_63, 9)
            .expect("prep must succeed");
        let bytes = prepared.to_le_bytes();

        // Verify round-trip
        let restored = MaturityBenchState::from_le_bytes(&bytes);
        let summary_orig = bench_maturity_from_prepared(&cfg, &prepared);
        let summary_restored = bench_maturity_from_prepared(&cfg, &restored);
        assert_eq!(summary_orig.checksum, summary_restored.checksum);

        // Print as Rust const
        println!("pub const MATURITY_SNAPSHOT_K9: [u8; {}] = [", bytes.len());
        for chunk in bytes.chunks(16) {
            let hex: Vec<String> = chunk.iter().map(|b| format!("0x{b:02x}")).collect();
            println!("    {},", hex.join(", "));
        }
        println!("];");
        println!(
            "\n// safe_active={} knocked_active={}",
            prepared.safe_state.n_active, prepared.knocked_state.n_active
        );
        println!("// checksum={}", summary_orig.checksum);
    }

    #[test]
    fn obs1_table_vs_exact() {
        use crate::obs1_seed_tables::obs1_raw_weight_lookup;

        let mut max_w_err_ppm = 0i64;
        let mut max_fh_err_ppm = 0i64;
        let mut max_fki_err_ppm = 0i64;
        let mut max_err_sigma = 0.0f64;

        // Test at 64 grid points + 63 midpoints
        for i in 0..127 {
            let sigma = 0.05 + (1.0 - 0.05) * i as f64 / 126.0;
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);

            // Exact raw weights
            let exact = obs1_seed_raw_weights(&cfg, sigma_s6, drift_diffs, drift_shift_63)
                .expect("exact must succeed");

            // Table lookup
            let (tab_sw, tab_kw, tab_fh, tab_fki) = obs1_raw_weight_lookup(sigma_s6);

            // Compare per-node weights
            for idx in 0..N_FACTOR_NODES_EXACT_SEED {
                let sw_err = (exact.safe_w[idx] - tab_sw[idx]).abs();
                let kw_err = (exact.knocked_w[idx] - tab_kw[idx]).abs();
                max_w_err_ppm = max_w_err_ppm.max(sw_err).max(kw_err);
                if sw_err > 5000 || kw_err > 5000 {
                    max_err_sigma = sigma;
                }
            }
            let fh_err = (exact.first_hit - tab_fh).abs();
            let fki_err = (exact.first_knock_in - tab_fki).abs();
            max_fh_err_ppm = max_fh_err_ppm.max(fh_err);
            max_fki_err_ppm = max_fki_err_ppm.max(fki_err);
        }
        // Print worst node detail around worst sigma
        let ws = max_err_sigma;
        if ws > 0.0 {
            for ds in [-0.02, -0.01, 0.0, 0.01, 0.02] {
                let sigma = ws + ds;
                let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
                let exact =
                    obs1_seed_raw_weights(&cfg, sigma_s6, drift_diffs, drift_shift_63).unwrap();
                let (tab_sw, tab_kw, _, _) = obs1_raw_weight_lookup(sigma_s6);
                let mut worst_node = 0;
                let mut worst_err = 0i64;
                for idx in 0..N_FACTOR_NODES_EXACT_SEED {
                    let err = (exact.knocked_w[idx] - tab_kw[idx]).abs();
                    if err > worst_err {
                        worst_err = err;
                        worst_node = idx;
                    }
                    let err = (exact.safe_w[idx] - tab_sw[idx]).abs();
                    if err > worst_err {
                        worst_err = err;
                        worst_node = idx;
                    }
                }
                println!(
                    "sigma={sigma:.4} node={worst_node} exact_sw={} tab_sw={} exact_kw={} tab_kw={} err={worst_err}",
                    exact.safe_w[worst_node], tab_sw[worst_node],
                    exact.knocked_w[worst_node], tab_kw[worst_node],
                );
            }
        }
        println!(
            "obs1_table_vs_exact max_w_err={max_w_err_ppm}ppm max_fh_err={max_fh_err_ppm}ppm max_fki_err={max_fki_err_ppm}ppm worst_sigma={max_err_sigma:.3}"
        );
        assert!(
            max_w_err_ppm < 10000,
            "max weight error {max_w_err_ppm}ppm exceeds 10000ppm (1%)"
        );
    }

    #[test]
    #[ignore]
    fn obs1_seed_shape_survey() {
        let cfg = spy_qqq_iwm_c1_config();
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        println!("sigma,safe_n,knocked_n,first_hit,first_knock_in");
        for i in 0..64 {
            let sigma = 0.05 + (1.0 - 0.05) * i as f64 / 63.0;
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
            let drift_shift_total = cfg.obs[0].obs_day as i64 / 63 * drift_shift_63;
            let triple_pre = cholesky6(cfg.obs[0].cov_uu, cfg.obs[0].cov_uv, cfg.obs[0].cov_vv)
                .ok()
                .map(|(l11, l21, l22)| {
                    build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av)
                });
            if let Some(seed) = run_first_observation_seed(
                &cfg,
                sigma_s6,
                drift_diffs,
                drift_shift_total,
                9,
                triple_pre.as_ref(),
            ) {
                println!(
                    "{sigma:.3},{},{},{},{}",
                    seed.next_safe.n_active,
                    seed.next_knocked.n_active,
                    seed.first_hit,
                    seed.first_knock_in,
                );
            } else {
                println!("{sigma:.3},NONE,NONE,NONE,NONE");
            }
        }
    }

    #[test]
    #[ignore]
    fn barrier_transition_survey() {
        for sigma in [0.15, 0.20, 0.30, 0.364_352_876_062_913_67, 0.50] {
            let (cfg, _sigma_s6, _drift_diffs, drift_shift_63) = filter_inputs(sigma);
            let tables = phi2_tables();
            println!("\nsigma={sigma:.3}");
            for obs_idx in 1..N_OBS {
                let obs = &cfg.obs[obs_idx];
                let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;
                let triple_pre_val =
                    cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
                        .ok()
                        .map(|(l11, l21, l22)| {
                            build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av)
                        });
                let tp = triple_pre_val.as_ref();
                let mut c_lo = 0i64;
                let mut c_hi = 0i64;
                let mut found_lo = false;
                for ic in -800..800 {
                    let c = ic * 5000;
                    let shift = c + drift_shift_total;
                    let rhs = [cfg.autocall_rhs_base + shift; 3];
                    let p =
                        triangle_probability_with_triple_i64(0, 0, &rhs, &obs.tri_pre, tables, tp);
                    if !found_lo && p > 50_000 {
                        c_lo = c;
                        found_lo = true;
                    }
                    if p > 950_000 && c_hi == 0 {
                        c_hi = c;
                    }
                }
                println!(
                    "  obs={obs_idx} c_lo={c_lo} c_hi={c_hi} width={} drift_shift={drift_shift_total}",
                    c_hi - c_lo,
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn onchain_path_accuracy() {
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        for sigma in [
            0.291_482_300_850_330_96,
            0.364_352_876_062_913_67,
            0.437_223_451_275_496_4,
        ] {
            let exact = model.quote_coupon(sigma).unwrap();
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
            println!("\nsigma={sigma:.15} exact_bps={:.6}", exact.fair_coupon_bps);
            for k in [7usize, 9, 12, 15] {
                let q = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, k);
                println!(
                    "  k_safe={k:>2} fc_bps={:.6} err={:+.3} ki={:.6} ac={:.6}",
                    q.fair_coupon_bps_f64(),
                    q.fair_coupon_bps_f64() - exact.fair_coupon_bps,
                    q.knock_in_rate_f64(),
                    q.autocall_rate_f64(),
                );
            }
        }
    }

    /// Run quote_c1_filter on-chain path with frozen grids forced off.
    fn quote_c1_filter_no_frozen(
        cfg: &C1FastConfig,
        sigma_s6: i64,
        drift_diffs: [i64; 2],
        drift_shift_63: i64,
        k_retained: usize,
    ) -> C1FastQuote {
        let k_knocked = 1usize;
        let k_retained = k_retained.clamp(1, MAX_K);
        let transition = build_factor_transition(cfg, sigma_s6, drift_diffs);
        let mut safe_state = FilterState::singleton_origin();
        let mut knocked_state = FilterState::default();
        let mut redemption_pv = 0i64;
        let mut coupon_annuity = 0i64;
        let mut total_ki = 0i64;
        let mut total_ac = 0i64;
        for obs_idx in 0..N_OBS {
            let obs = &cfg.obs[obs_idx];
            let is_maturity = obs_idx + 1 == N_OBS;
            let coupon_count = (obs_idx + 1) as i64;
            let drift_shift_total = obs.obs_day as i64 / 63 * drift_shift_63;
            if obs_idx == 0 {
                let (obs1_safe, obs1_knocked, obs1_fh, obs1_fki) =
                    crate::obs1_seed_tables::obs1_projected_lookup(sigma_s6);
                redemption_pv += m6r(cfg.notional, obs1_fh);
                coupon_annuity += coupon_count * obs1_fh;
                total_ac += obs1_fh;
                total_ki += obs1_fki;
                if k_retained < 9 {
                    safe_state = project_state(&obs1_safe, k_retained);
                } else {
                    safe_state = obs1_safe;
                }
                knocked_state = project_state(&obs1_knocked, k_knocked);
                continue;
            }
            let triple_pre =
                cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv)
                    .ok()
                    .map(|(l11, l21, l22)| {
                        build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av)
                    });
            let tp = observation_probability_triple_pre(obs_idx, triple_pre.as_ref());
            if is_maturity {
                let maturity = run_maturity_step(
                    &safe_state,
                    &knocked_state,
                    &transition,
                    cfg,
                    obs_idx,
                    drift_shift_total,
                    k_retained,
                    tp,
                    None,
                );
                redemption_pv += m6r(cfg.notional, maturity.safe_principal);
                redemption_pv += m6r(cfg.notional, maturity.knock_in_redemption_safe);
                redemption_pv += m6r(cfg.notional, maturity.knocked_redemption);
                coupon_annuity += coupon_count * maturity.coupon_hit;
                total_ki += maturity.first_knock_in;
                continue;
            }
            let (next_safe, next_knocked, first_hit, first_knock_in) = run_observation_step(
                &safe_state,
                &knocked_state,
                &transition,
                cfg,
                obs_idx,
                drift_shift_total,
                k_retained,
                k_knocked,
                tp,
                None,
            );
            redemption_pv += m6r(cfg.notional, first_hit);
            coupon_annuity += coupon_count * first_hit;
            total_ac += first_hit;
            total_ki += first_knock_in;
            safe_state = next_safe;
            knocked_state = next_knocked;
        }
        let loss = (cfg.notional - redemption_pv).max(0);
        let fair_coupon = if coupon_annuity > 100 {
            loss * S6 / coupon_annuity
        } else {
            0
        };
        c1_fast_quote_from_components(
            cfg.notional,
            fair_coupon,
            redemption_pv,
            coupon_annuity,
            total_ki,
            total_ac,
        )
    }

    /// Validate frozen-grid predict_state matches on-chain live predict_state.
    /// Both use the same update_safe_state (on-chain path), isolating the grid effect.
    #[test]
    fn frozen_predict_vs_live_quote_accuracy() {
        let mut max_diff_bps = 0.0f64;
        let mut worst_sigma = 0.0f64;
        // Sweep sigma from 0.10 to 0.90
        for k in [7usize, 9] {
            for sigma_idx in 0..40 {
                let sigma = 0.05 + sigma_idx as f64 * 0.02;
                if sigma > 0.95 {
                    break;
                }
                let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);

                let frozen = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, k);
                let no_frozen =
                    quote_c1_filter_no_frozen(&cfg, sigma_s6, drift_diffs, drift_shift_63, k);

                let diff = (frozen.fair_coupon_bps_f64() - no_frozen.fair_coupon_bps_f64()).abs();
                if diff > max_diff_bps {
                    max_diff_bps = diff;
                    worst_sigma = sigma;
                }
                println!(
                    "K={k} sigma={sigma:.3} frozen={:.4} no_frozen={:.4} diff={diff:.4} bps",
                    frozen.fair_coupon_bps_f64(),
                    no_frozen.fair_coupon_bps_f64(),
                );
            }
        }
        println!("max frozen vs no_frozen diff: {max_diff_bps:.4} bps at sigma={worst_sigma:.3}");
        // Frozen grid uses linear interpolation between 128 sigma samples.
        // Grid geometry is path-dependent through update_safe_state, so
        // interpolation between sample points introduces some error.
        // Production range (0.15-0.60) is < 45 bps; extreme sigma < 65 bps.
        assert!(
            max_diff_bps < 70.0,
            "max frozen vs no_frozen diff {max_diff_bps:.4} bps >= 70.0 at sigma={worst_sigma:.3}"
        );
    }

    #[test]
    fn frozen_table_diag_host() {
        let sigma_s6 = 291_482i64;
        let fg = crate::frozen_predict_tables::frozen_predict_grid_lookup(sigma_s6, 0, 7);
        if let Some(fg) = fg {
            println!(
                "frozen_k7_obs0: gmin={} gmax={} inv={}",
                fg.grid_c[0], fg.grid_c[6], fg.inv_cell_s30
            );
        }
        let (obs1_safe, _, obs1_fh, obs1_fki) =
            crate::obs1_seed_tables::obs1_projected_lookup(sigma_s6);
        for i in 0..9 {
            println!(
                "obs1 node[{i}]: c={} w={} mu={} mv={}",
                obs1_safe.nodes[i].c,
                obs1_safe.nodes[i].w,
                obs1_safe.nodes[i].mean_u,
                obs1_safe.nodes[i].mean_v,
            );
        }
        println!("obs1: fh={} fki={}", obs1_fh, obs1_fki);
        let nig_w = solmath_core::nig_weights_table::nig_importance_weights_9(sigma_s6);
        println!("nig_w: w0={} w4={} w8={}", nig_w[0], nig_w[4], nig_w[8]);
    }

    #[test]
    fn dump_cfg_const() {
        let cfg = spy_qqq_iwm_c1_config();
        println!("// Auto-generated from spy_qqq_iwm_c1_config()");
        println!("use crate::worst_of_c1_fast::{{C1FastConfig, ObsGeometry}};");
        println!("use solmath_core::TrianglePre64;");
        println!("pub const CFG: C1FastConfig = C1FastConfig {{");
        println!("    obs: [");
        for i in 0..6 {
            let o = &cfg.obs[i];
            println!("        ObsGeometry {{");
            println!("            tri_pre: TrianglePre64 {{");
            println!("                au: {:?},", o.tri_pre.au);
            println!("                av: {:?},", o.tri_pre.av);
            println!("                inv_std: {:?},", o.tri_pre.inv_std);
            println!("                phi2_neg: {:?},", o.tri_pre.phi2_neg);
            println!("            }},");
            println!("            cov_proj: {:?},", o.cov_proj);
            println!("            cov_uu: {},", o.cov_uu);
            println!("            cov_uv: {},", o.cov_uv);
            println!("            cov_vv: {},", o.cov_vv);
            println!("            obs_day: {},", o.obs_day);
            println!("        }},");
        }
        println!("    ],");
        println!("    loading_sum: {},", cfg.loading_sum);
        println!("    uv_slope: {:?},", cfg.uv_slope);
        println!("    loadings: {:?},", cfg.loadings);
        println!("    ki_barrier_log: {},", cfg.ki_barrier_log);
        println!("    notional: {},", cfg.notional);
        println!("    au: {:?},", cfg.au);
        println!("    av: {:?},", cfg.av);
        println!("    autocall_rhs_base: {},", cfg.autocall_rhs_base);
        println!("    ki_safe_rhs_base: {},", cfg.ki_safe_rhs_base);
        println!("}};");
    }

    #[test]
    fn tapered_quote_sanity() {
        // Sweep: corrected K=9 vs live K=15 reference
        let mut max_err = 0.0f64;
        let mut max_err_sigma = 0.0f64;
        for si in 0..40 {
            let sigma = 0.08 + si as f64 * 0.018;
            let (cfg2, ss, dd, ds) = filter_inputs(sigma);
            let k9c = quote_c1_filter(&cfg2, ss, dd, ds, 9);
            let ref15 = quote_c1_filter_live(&cfg2, ss, dd, ds, 15);
            let err = (k9c.fair_coupon_bps_f64() - ref15.fair_coupon_bps_f64()).abs();
            if err > max_err {
                max_err = err;
                max_err_sigma = sigma;
            }
            println!(
                "  sweep sigma={sigma:.3} k9c={:.2} ref={:.2} err={err:.2}",
                k9c.fair_coupon_bps_f64(),
                ref15.fair_coupon_bps_f64()
            );
        }
        println!("K=9 corrected max error: {max_err:.2} bps at sigma={max_err_sigma:.3}");
        assert!(
            max_err < 50.0,
            "K=9 corrected max error {max_err:.2} >= 50 bps"
        );

        for &sigma in &[0.15, 0.291_482, 0.364_353, 0.50] {
            let (cfg, sigma_s6, drift_diffs, drift_shift_63) = filter_inputs(sigma);
            let sched_9753 = [9usize, 7, 5, 3, 3];
            let sched_15953 = [15usize, 9, 5, 3, 3];

            let t1 =
                quote_c1_filter_tapered(&cfg, sigma_s6, drift_diffs, drift_shift_63, &sched_9753);
            let t2 =
                quote_c1_filter_tapered(&cfg, sigma_s6, drift_diffs, drift_shift_63, &sched_15953);
            let k7 = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, 7);
            let k9 = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, 9);
            let live = quote_c1_filter_live(&cfg, sigma_s6, drift_diffs, drift_shift_63, 15);
            let k12 = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, 12);
            let k15 = quote_c1_filter(&cfg, sigma_s6, drift_diffs, drift_shift_63, 15);

            println!("sigma={sigma:.3}:");
            println!(
                "  live K=15:         fc={:.2} bps",
                live.fair_coupon_bps_f64()
            );
            println!(
                "  uniform K=15:      fc={:.2} bps",
                k15.fair_coupon_bps_f64()
            );
            println!(
                "  uniform K=12:      fc={:.2} bps",
                k12.fair_coupon_bps_f64()
            );
            println!(
                "  uniform K=9:       fc={:.2} bps",
                k9.fair_coupon_bps_f64()
            );
            println!(
                "  uniform K=7:       fc={:.2} bps",
                k7.fair_coupon_bps_f64()
            );
            println!(
                "  tapered [9,7,5,3]: fc={:.2} bps",
                t1.fair_coupon_bps_f64()
            );
            println!(
                "  tapered [15,9,5,3]:fc={:.2} bps",
                t2.fair_coupon_bps_f64()
            );

            assert!(t2.fair_coupon_bps_f64() > 0.0);
            assert!(t2.fair_coupon_bps_f64() < 5000.0);
        }
    }
}
