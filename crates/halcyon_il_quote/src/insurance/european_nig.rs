//! NIG European IL premium via Gauss-Legendre 5-point quadrature.
//!
//! Build plan v3.2 § B1 — the only Hedge pricing engine. Both pairs (SOL/USDC
//! and WETH/SOL) price on-chain via this function with tenor-fitted NIG params
//! from `data/nig_calibration_all_pairs_tenors.json`.
//!
//! # Why i64 / SCALE_6
//!
//! The whole inner loop runs at `i64` fixed-point with `SCALE_6 = 1e6`, using
//! solmath's `mul6 / div6 / sqrt6 / exp6` helpers. These cost ≈5× less BPF CU
//! per op than the i128 / SCALE_12 equivalents because they fit native 64-bit
//! arithmetic. `nig_call_64` uses the same trick to hit ~120K CU; we apply it
//! here so the GL5 quadrature lands inside the build plan B1 budget.
//!
//! # Formula
//!
//! ```text
//! V_unit = ∫ min(max(IL(x) - d, 0), c - d) · f_NIG(x; α, β, δ_T, μ) dx
//!
//! IL(x) = ½ (e^{x/2} - 1)²              entry-normalised IL (B3)
//! γ     = √(α² - β²)
//! γ_s   = √(α² - (β+1)²)                MGF-shifted γ
//! δ_T   = σ² · γ³ / α² · T/365          LINEAR in T (NOT √T)
//! μ_T   = δ_T · (γ_s - γ)               martingale drift, negative for β>0
//! ```
//!
//! Variance check: `Var(X) = δ_T · α²/γ³ = σ² · T/365` (matches Gaussian Var).
//!
//! # Density
//!
//! ```text
//! f_NIG(x) = (α δ_T K₁(αR)) / (π R) · exp(δ_T γ + β(x-μ))
//! R(x)     = √(δ_T² + (x-μ)²)
//! ```
//!
//! `K₁(z)` is computed via the Abramowitz & Stegun 9.8.8 polynomial form,
//! valid for `z ≥ 2` with `|ε| < 2.2 × 10⁻⁷`.
//!
//! # Quadrature regions
//!
//! Four regions (5-point Gauss-Legendre each, 20 NIG density evals total):
//! - **Linear right**  `[x_d_up, x_c_up]`              payoff = `IL(x) − d`
//! - **Linear left**   `[x_c_dn, x_d_dn]`              payoff = `IL(x) − d`
//! - **Cap right**     `[x_c_up, x_c_up + L]`          payoff = `c − d`
//! - **Cap left**      `[x_c_dn − L, x_c_dn]`          payoff = `c − d`
//!
//! where `L = 10 · σ √(T/365)` truncates the tail at 10 NIG std-devs.
//!
//! # API
//!
//! Returns the premium as a fraction of insured value at SCALE_6 = 1e6.
//! Caller multiplies by `insured_value` (raw token units) for the
//! USDC-denominated premium.

use solmath_core::{div6, exp6, mul6, sqrt6, SolMathError, PI6, SCALE_6};

// ─────────────────────────────────────────────────────────────────────────
// 5-point Gauss-Legendre on [-1, 1] at SCALE_6
// (truncated from the SCALE_12 constants in solmath_core::gauss_hermite)
// ─────────────────────────────────────────────────────────────────────────

const GL5_NODES: [i64; 5] = [
    -906_180, // -0.906179845938663964
    -538_469, // -0.538469310105683108
    0, 538_469, 906_180,
];
const GL5_WEIGHTS: [i64; 5] = [
    236_927, // 0.236926885056189390
    478_629, // 0.478628670499366249
    568_889, // 0.568888888888888555
    478_629, 236_927,
];

// ─────────────────────────────────────────────────────────────────────────
// Abramowitz & Stegun 9.8.8 — K₁(z) for z ≥ 2
//   √z · e^z · K₁(z) = c0 + c1·t + c2·t² + ... + c6·t⁶,   t = 2/z
//   |ε| < 2.2 × 10⁻⁷
// ─────────────────────────────────────────────────────────────────────────

const K1_C0: i64 = 1_253_314; //  1.25331414
const K1_C1: i64 = 234_986; //  0.23498619
const K1_C2: i64 = -36_556; // -0.03655620
const K1_C3: i64 = 15_043; //  0.01504268
const K1_C4: i64 = -7_804; // -0.00780353
const K1_C5: i64 = 3_256; //  0.00325614
const K1_C6: i64 = -682; // -0.00068245

// ─────────────────────────────────────────────────────────────────────────
// Abramowitz & Stegun 9.8.7 — K₁(z) for 0 < z ≤ 2
//   z·K₁(z) = z·ln(z/2)·I₁(z) + 1 + Σ aₙ·(z/2)^(2n)
//   |ε| < 8 × 10⁻⁹
// ─────────────────────────────────────────────────────────────────────────

const K1S_A0: i64 = 1_000_000; //  1.0
const K1S_A1: i64 = 154_431; //  0.15443144
const K1S_A2: i64 = -672_786; // -0.67278579
const K1S_A3: i64 = -181_569; // -0.18156897
const K1S_A4: i64 = -19_194; // -0.01919402
const K1S_A5: i64 = -1_104; // -0.00110404
const K1S_A6: i64 = -47; // -0.00004686

// ─────────────────────────────────────────────────────────────────────────
// Abramowitz & Stegun 9.8.1 — I₁(z) for |z| ≤ 3.75
//   I₁(z)/z = c0 + c1·t² + c2·t⁴ + ... + c6·t¹²,   t = z/3.75
//   |ε| < 8 × 10⁻⁹
// ─────────────────────────────────────────────────────────────────────────

const I1_C0: i64 = 500_000; //  0.5
const I1_C1: i64 = 878_906; //  0.87890594
const I1_C2: i64 = 514_989; //  0.51498869
const I1_C3: i64 = 150_849; //  0.15084934
const I1_C4: i64 = 26_587; //  0.02658733
const I1_C5: i64 = 3_015; //  0.00301532
const I1_C6: i64 = 324; //  0.00032411

const INV_3_75: i64 = 266_667; // 1/3.75 ≈ 0.266667 at SCALE_6

/// K₁(z) at SCALE_6 for `z` ∈ (0, 2·SCALE_6] via A&S 9.8.7 + 9.8.1.
/// Returns the **direct** Bessel value (not √z·eᶻ·K₁(z)).
///
/// Caller is responsible for the `z > 0` precondition; `z = 0` would be a
/// log-singularity.
fn bessel_k1_small(z: i64) -> Result<i64, SolMathError> {
    // ── I₁(z) via A&S 9.8.1 ──
    // I₁(z)/z polynomial in t² where t = z/3.75
    let t = mul6(z, INV_3_75)?;
    let t_sq = mul6(t, t)?;
    let mut p = I1_C6;
    p = mul6(p, t_sq)? + I1_C5;
    p = mul6(p, t_sq)? + I1_C4;
    p = mul6(p, t_sq)? + I1_C3;
    p = mul6(p, t_sq)? + I1_C2;
    p = mul6(p, t_sq)? + I1_C1;
    p = mul6(p, t_sq)? + I1_C0;
    let i1_z = mul6(p, z)?; // I₁(z) = (poly) · z

    // ── q(z) = 1 + Σ aₙ·(z/2)^(2n) ──
    let u = z / 2;
    let u_sq = mul6(u, u)?;
    let mut q = K1S_A6;
    q = mul6(q, u_sq)? + K1S_A5;
    q = mul6(q, u_sq)? + K1S_A4;
    q = mul6(q, u_sq)? + K1S_A3;
    q = mul6(q, u_sq)? + K1S_A2;
    q = mul6(q, u_sq)? + K1S_A1;
    q = mul6(q, u_sq)? + K1S_A0;

    // K₁(z) = ln(z/2)·I₁(z) + q/z
    // Note: ln(z/2) is negative for z < 2 (since z/2 < 1), and ln6 returns signed.
    let ln_u = solmath_core::ln6(u)?;
    let ln_i1 = mul6(ln_u, i1_z)?;
    let q_over_z = div6(q, z)?;
    Ok(ln_i1 + q_over_z)
}

/// Tail truncation distance in NIG standard deviations.
const TAIL_STD_DEVIATIONS: i64 = 10 * SCALE_6;

/// Compute IL(x) = ½(e^{x/2} - 1)² at SCALE_6 for log-return `x` at SCALE_6.
#[inline]
fn il_entry(x: i64) -> Result<i64, SolMathError> {
    let half_x = x / 2;
    let exp_half = exp6(half_x)?;
    let diff = exp_half - SCALE_6;
    let sq = mul6(diff, diff)?;
    Ok(sq / 2)
}

/// Solve `IL(x) = h` for the positive root: `x = 2 · ln(1 + √(2h))`.
/// Uses solmath's i64 ln via series; cheap relative to the inner quadrature.
///
/// `pub(super)` so the sibling `cos_first_passage` module can reuse it.
#[inline]
pub(super) fn il_root_up(h: i64) -> Result<i64, SolMathError> {
    if h <= 0 {
        return Ok(0);
    }
    let two_h = 2 * h;
    let sqrt_2h = sqrt6(two_h)?;
    let arg = SCALE_6 + sqrt_2h;
    let ln_arg = solmath_core::ln6(arg)?;
    Ok(2 * ln_arg)
}

/// Solve `IL(x) = h` for the negative root: `x = 2 · ln(1 - √(2h))`.
/// Requires `h < 0.5` so that `1 - √(2h) > 0`.
///
/// `pub(super)` so the sibling `cos_first_passage` module can reuse it.
#[inline]
pub(super) fn il_root_dn(h: i64) -> Result<i64, SolMathError> {
    if h <= 0 {
        return Ok(0);
    }
    let two_h = 2 * h;
    let sqrt_2h = sqrt6(two_h)?;
    if sqrt_2h >= SCALE_6 {
        return Err(SolMathError::DomainError);
    }
    let arg = SCALE_6 - sqrt_2h;
    let ln_arg = solmath_core::ln6(arg)?;
    Ok(2 * ln_arg)
}

/// Internal context bundling NIG params and precomputed loop-invariant
/// quantities for the inner GL5 loop.
///
/// `pub(super)` so the sibling `cos_first_passage` module can reuse it.
pub(super) struct NigCtx {
    pub(super) alpha: i64,
    pub(super) beta: i64,
    pub(super) mu: i64,
    /// `δ_T²`
    pub(super) delta_t_sq: i64,
    /// `δ_T · γ`
    pub(super) delta_gamma: i64,
    /// `δ_T · √α / π` — large-arg path prefactor (αR ≥ 2). Per-node
    /// denominator becomes `R^(3/2)` (one sqrt + one mul) instead of
    /// `R · √(αR)` (two sqrts).
    pub(super) prefactor: i64,
    /// `α · δ_T / π` — small-arg path prefactor (αR < 2). Equals
    /// `prefactor · √α`. Used when we evaluate K₁ directly via A&S 9.8.7.
    pub(super) prefactor_small: i64,
}

/// Evaluate NIG density `f(x)` at a single point.
///
/// Dispatches to one of two K₁ approximations depending on αR:
///   - **αR ≥ 2**: A&S 9.8.8 large-arg polynomial in `t = 2/(αR)` (|ε| < 2.2e-7),
///     combined with the `√(αR)·exp(αR)` factor for numerical stability of
///     `exp(δ_T·γ + β·(x−μ) − αR)` (avoids overflow when αR is large).
///   - **0 < αR < 2**: A&S 9.8.7 small-arg form using A&S 9.8.1 for I₁(αR)
///     (|ε| < 8e-9). Required for production 30-day calibrations where the
///     leftmost GL5 nodes give αR ∈ [1, 2] at typical σ. Without this branch,
///     SOL/USDC 30d (α=3.14) reverts on 58% of grid points and WETH/SOL 30d
///     (α=3.46) on 64% — see `data/validation/rust_european_grid.csv`.
pub(super) fn nig_pdf_at(x_minus_mu: i64, ctx: &NigCtx) -> Result<i64, SolMathError> {
    // R² = δ_T² + (x-μ)²
    let xm_sq = mul6(x_minus_mu, x_minus_mu)?;
    let r_sq = ctx.delta_t_sq + xm_sq;
    let r = sqrt6(r_sq)?;
    if r == 0 {
        return Err(SolMathError::DomainError);
    }

    // αR
    let ar = mul6(ctx.alpha, r)?;
    if ar <= 0 {
        return Err(SolMathError::DomainError);
    }

    if ar >= 2 * SCALE_6 {
        // ── Large-arg path (A&S 9.8.8) ──
        // K₁(αR) via polynomial in t = 2/(αR), wrapped in √(αR)·exp(αR).
        let t = div6(2 * SCALE_6, ar)?;
        let mut p = K1_C6;
        p = mul6(p, t)? + K1_C5;
        p = mul6(p, t)? + K1_C4;
        p = mul6(p, t)? + K1_C3;
        p = mul6(p, t)? + K1_C2;
        p = mul6(p, t)? + K1_C1;
        p = mul6(p, t)? + K1_C0;

        // R^(3/2) = R · √R   (single sqrt + one mul)
        let sqrt_r = sqrt6(r)?;
        let r_three_halves = mul6(r, sqrt_r)?;

        // Combined exponent: -αR + δ_T·γ + β·(x - μ)
        let exponent = -ar + ctx.delta_gamma + mul6(ctx.beta, x_minus_mu)?;
        let exp_val = exp6(exponent)?;

        // density = (δ_T · √α / π) · poly · exp_val / R^(3/2)
        let num = mul6(mul6(ctx.prefactor, p)?, exp_val)?;
        div6(num, r_three_halves)
    } else {
        // ── Small-arg path (A&S 9.8.7 + 9.8.1) ──
        // Compute K₁(αR) directly. αR ∈ (0, 2), so the +δ_T·γ + β·(x−μ)
        // exponent is bounded (no need for the −αR cancellation trick).
        let k1 = bessel_k1_small(ar)?;
        let exponent = ctx.delta_gamma + mul6(ctx.beta, x_minus_mu)?;
        let exp_val = exp6(exponent)?;
        // density = (α · δ_T / π) · K₁(αR) · exp(δ_T·γ + β·(x−μ)) / R
        let num = mul6(mul6(ctx.prefactor_small, k1)?, exp_val)?;
        div6(num, r)
    }
}

/// Gauss-Legendre integration over `[a, b]` with caller-supplied
/// node/weight arrays. `payoff(x)` returns the payoff at a given `x`.
///
/// **Precision-critical:** the inner product `pay · dens · weight` is
/// accumulated at full i128 width (effective scale `1e18`) and only
/// reduced back to SCALE_6 at the very end. Doing per-step `mul6` would
/// truncate ~1 tick per multiply; with 4 muls per node × 5 nodes = 20
/// truncations, the rounding floor swallows low-σ premiums (which are
/// only ~40 ticks at SCALE_6). The wide accumulator restores the
/// missing precision while keeping every BPF op a native 64-bit mul.
fn gl_integrate<F>(
    a: i64,
    b: i64,
    nodes: &[i64],
    weights: &[i64],
    ctx: &NigCtx,
    mut payoff: F,
) -> Result<i64, SolMathError>
where
    F: FnMut(i64) -> Result<i64, SolMathError>,
{
    if b <= a {
        return Ok(0);
    }
    let half_width = (b - a) / 2;
    let mid = (a + b) / 2;

    // sum_wide accumulates `pay × dens × weight` at i128 SCALE_18
    // (no division until the very end).
    let mut sum_wide: i128 = 0;
    for k in 0..nodes.len() {
        let u_k = nodes[k];
        let w_k = weights[k];
        let x_k = mul6(half_width, u_k)? + mid;
        let pay = payoff(x_k)?;
        if pay == 0 {
            continue;
        }
        let dens = nig_pdf_at(x_k - ctx.mu, ctx)?;
        // pay × dens × weight at SCALE_18 in i128, no truncation.
        let prod = (pay as i128) * (dens as i128) * (w_k as i128);
        sum_wide += prod;
    }
    // ∫ ≈ half_width × sum_wide   at i128 SCALE_24
    // Reduce to SCALE_6 by dividing by 10^18.
    let result = ((half_width as i128) * sum_wide) / 1_000_000_000_000_000_000i128;
    if result > i64::MAX as i128 || result < i64::MIN as i128 {
        return Err(SolMathError::Overflow);
    }
    Ok(result as i64)
}

/// Build a `NigCtx` from raw NIG params + tenor.
///
/// Returns `(ctx, gamma, t_years_scale6)` so callers can reuse derived
/// quantities (e.g. for `nig_std`). Both `cos_first_passage` and
/// `nig_european_il_premium` use this.
///
/// `pub(super)` — only sibling modules in `insurance/` can call this.
pub(super) fn build_nig_ctx(
    sigma: i64,
    days: u32,
    alpha: i64,
    beta: i64,
) -> Result<(NigCtx, i64, i64), SolMathError> {
    if alpha <= 0 || sigma <= 0 || days == 0 {
        return Err(SolMathError::DomainError);
    }

    // ── NIG validation: α > |β| AND α² > (β+1)² ──
    let alpha_sq = mul6(alpha, alpha)?;
    let beta_sq = mul6(beta, beta)?;
    if alpha_sq <= beta_sq {
        return Err(SolMathError::DomainError);
    }
    let beta_plus_1 = beta + SCALE_6;
    let bp1_sq = mul6(beta_plus_1, beta_plus_1)?;
    if alpha_sq <= bp1_sq {
        return Err(SolMathError::DomainError);
    }

    // ── γ = √(α² − β²),  γ_s = √(α² − (β+1)²) ──
    let gamma = sqrt6(alpha_sq - beta_sq)?;
    let gamma_s = sqrt6(alpha_sq - bp1_sq)?;

    // ── δ_T = σ² · γ³ / α² · T/365  (LINEAR in T) ──
    let sigma_sq = mul6(sigma, sigma)?;
    let gamma_sq = mul6(gamma, gamma)?;
    let gamma_cu = mul6(gamma_sq, gamma)?;
    let delta_eff = div6(mul6(sigma_sq, gamma_cu)?, alpha_sq)?;
    let t_years = ((days as i64) * SCALE_6) / 365;
    let delta_t = mul6(delta_eff, t_years)?;
    if delta_t <= 0 {
        return Err(SolMathError::DomainError);
    }

    // ── μ_T = δ_T · (γ_s − γ) ──
    let mu = mul6(delta_t, gamma_s - gamma)?;

    // ── Hoisted loop invariants ──
    let delta_t_sq = mul6(delta_t, delta_t)?;
    let delta_gamma = mul6(delta_t, gamma)?;
    let sqrt_alpha = sqrt6(alpha)?;
    let prefactor = div6(mul6(delta_t, sqrt_alpha)?, PI6)?;
    let prefactor_small = mul6(prefactor, sqrt_alpha)?;

    let ctx = NigCtx {
        alpha,
        beta,
        mu,
        delta_t_sq,
        delta_gamma,
        prefactor,
        prefactor_small,
    };
    Ok((ctx, gamma, t_years))
}

/// Compute the NIG European IL Hedge premium per unit of insured value.
///
/// All inputs at SCALE_6 (1e6) unless stated.
///
/// # Inputs
/// - `sigma`: pipeline σ, SCALE_6
/// - `days`: tenor in days
/// - `deductible`: `d`, SCALE_6 (e.g. 0.02 → `20_000`)
/// - `cap`: `c`, SCALE_6 (e.g. 0.15 → `150_000`)
/// - `alpha`: tenor-fitted NIG `α`, SCALE_6 (e.g. 8.84 → `8_840_000`)
/// - `beta`: tenor-fitted NIG `β`, SCALE_6 (signed; positive for SOL pump-skew)
///
/// # Returns
/// Premium as fraction of insured value at SCALE_6. Caller multiplies by
/// `insured_value` to get the USDC amount.
///
/// # Errors
/// - `DomainError` if `deductible >= cap`, `cap >= 0.5`, `α ≤ |β|`,
///   or `α² ≤ (β+1)²` (NIG MGF requirement).
pub fn nig_european_il_premium(
    sigma: i64,
    days: u32,
    deductible: i64,
    cap: i64,
    alpha: i64,
    beta: i64,
) -> Result<i64, SolMathError> {
    if days == 0 || sigma <= 0 {
        return Ok(0);
    }
    if deductible >= cap {
        return Err(SolMathError::DomainError);
    }
    // c < 0.5 required so that x_c_dn = 2 ln(1 - √(2c)) is real.
    if cap >= SCALE_6 / 2 {
        return Err(SolMathError::DomainError);
    }
    if alpha <= 0 {
        return Err(SolMathError::DomainError);
    }

    // ── NIG parameter validation: α > |β| AND α² > (β+1)² ──
    let alpha_sq = mul6(alpha, alpha)?;
    let beta_sq = mul6(beta, beta)?;
    if alpha_sq <= beta_sq {
        return Err(SolMathError::DomainError);
    }
    let beta_plus_1 = beta + SCALE_6;
    let bp1_sq = mul6(beta_plus_1, beta_plus_1)?;
    if alpha_sq <= bp1_sq {
        return Err(SolMathError::DomainError);
    }

    // ── γ = √(α² − β²),  γ_s = √(α² − (β+1)²) ──
    let gamma = sqrt6(alpha_sq - beta_sq)?;
    let gamma_s = sqrt6(alpha_sq - bp1_sq)?;

    // ── δ_T = σ² · γ³ / α² · T/365  (LINEAR in T) ──
    let sigma_sq = mul6(sigma, sigma)?;
    let gamma_sq = mul6(gamma, gamma)?;
    let gamma_cu = mul6(gamma_sq, gamma)?;
    let delta_eff = div6(mul6(sigma_sq, gamma_cu)?, alpha_sq)?;
    let t_years = ((days as i64) * SCALE_6) / 365;
    let delta_t = mul6(delta_eff, t_years)?;
    if delta_t <= 0 {
        return Ok(0);
    }

    // ── μ_T = δ_T · (γ_s − γ)  (negative for β > 0) ──
    let mu = mul6(delta_t, gamma_s - gamma)?;

    // ── Hoisted loop invariants ──
    let delta_t_sq = mul6(delta_t, delta_t)?;
    let delta_gamma = mul6(delta_t, gamma)?;
    let sqrt_alpha = sqrt6(alpha)?;
    let prefactor = div6(mul6(delta_t, sqrt_alpha)?, PI6)?;
    // prefactor_small = α · δ_T / π = prefactor · √α
    let prefactor_small = mul6(prefactor, sqrt_alpha)?;

    let ctx = NigCtx {
        alpha,
        beta,
        mu,
        delta_t_sq,
        delta_gamma,
        prefactor,
        prefactor_small,
    };

    // ── IL roots ──
    let x_d_up = il_root_up(deductible)?;
    let x_d_dn = il_root_dn(deductible)?;
    let x_c_up = il_root_up(cap)?;
    let x_c_dn = il_root_dn(cap)?;

    // ── nig_std = σ · √(T/365)  (10·σ tail truncation) ──
    let nig_std = sqrt6(mul6(sigma_sq, t_years)?)?;
    if nig_std <= 0 {
        return Ok(0);
    }
    let tail_distance = mul6(TAIL_STD_DEVIATIONS, nig_std)?;

    // ── Region 1: linear right [x_d_up, x_c_up] ──
    let payoff_lin = |x: i64| -> Result<i64, SolMathError> {
        let il = il_entry(x)?;
        let p = il - deductible;
        Ok(if p > 0 { p } else { 0 })
    };
    let region_lr = gl_integrate(x_d_up, x_c_up, &GL5_NODES, &GL5_WEIGHTS, &ctx, payoff_lin)?;

    // ── Region 2: linear left [x_c_dn, x_d_dn] ──
    let region_ll = gl_integrate(x_c_dn, x_d_dn, &GL5_NODES, &GL5_WEIGHTS, &ctx, payoff_lin)?;

    // ── Region 3: cap right [x_c_up, x_c_up + L] ──
    let cap_payoff = cap - deductible;
    let payoff_cap = |_x: i64| -> Result<i64, SolMathError> { Ok(cap_payoff) };

    let region_cr = gl_integrate(
        x_c_up,
        x_c_up + tail_distance,
        &GL5_NODES,
        &GL5_WEIGHTS,
        &ctx,
        payoff_cap,
    )?;
    // ── Region 4: cap left [x_c_dn − L, x_c_dn] ──
    let region_cl = gl_integrate(
        x_c_dn - tail_distance,
        x_c_dn,
        &GL5_NODES,
        &GL5_WEIGHTS,
        &ctx,
        payoff_cap,
    )?;

    let total = region_lr + region_ll + region_cr + region_cl;
    Ok(if total > 0 { total } else { 0 })
}

/// Build plan §B2 — concentrated-liquidity (CLMM) variant of the NIG
/// European IL premium.
///
/// For a CLMM position with concentration factor `C ≥ 1` (where `C = 1` is
/// full-range and matches `nig_european_il_premium` exactly), the IL is
/// amplified by `C` and the linear payoff zone of the option shifts to
/// `[d/C, c/C]`. The fair-value identity is:
///
/// ```text
/// Premium_CLMM(σ, T, d, c, C) = C · Premium_fullrange(σ, T, d/C, c/C)
/// ```
///
/// # Exit criterion (build plan §B2)
/// - `C == 1` reduces to `nig_european_il_premium` exactly.
/// - `C  > 1` produces a strictly larger premium for any d, c, σ that
///   produces a non-zero base premium.
///
/// # Inputs
/// All scalar params at SCALE_6, plus `concentration` at SCALE_6
/// (e.g. `C = 2.5x` → `2_500_000`).
pub fn nig_european_il_premium_clmm(
    sigma: i64,
    days: u32,
    deductible: i64,
    cap: i64,
    alpha: i64,
    beta: i64,
    concentration: i64,
) -> Result<i64, SolMathError> {
    if concentration < SCALE_6 {
        // C < 1 makes no physical sense (a position cannot be less
        // concentrated than full-range).
        return Err(SolMathError::DomainError);
    }
    // Scale d and c by 1/C. mul6/div6 keep us in SCALE_6.
    let d_scaled = div6(deductible, concentration)?;
    let c_scaled = div6(cap, concentration)?;
    let base = nig_european_il_premium(sigma, days, d_scaled, c_scaled, alpha, beta)?;
    // Multiply by C.
    mul6(base, concentration)
}

// ═════════════════════════════════════════════════════════════════════════
// Tests
// ═════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    const SCALE_F: f64 = 1e6;

    fn s(v: f64) -> i64 {
        (v * SCALE_F) as i64
    }

    fn frac(p: i64) -> f64 {
        (p as f64) / SCALE_F
    }

    // SOL/USDC 3d tenor-fitted (data/nig_calibration_all_pairs_tenors.json)
    const ALPHA_SOL: f64 = 8.838560453503158;
    const BETA_SOL: f64 = 1.149128582811099;

    // WETH/SOL 3d tenor-fitted
    const ALPHA_WETH: f64 = 7.907015676825188;
    const BETA_WETH: f64 = 2.465211024135256;

    /// Reference 10M-path MC fair values from
    /// data/definitive_pricing_tables/C15_euro_nig_sol_3d_d2_c15.npy
    const SOL_C15_REFERENCE: &[(f64, f64)] = &[
        (0.90, 0.00004316),
        (1.65, 0.00027771),
        (2.40, 0.00111754),
        (3.15, 0.00304860),
    ];

    /// Reference 10M-path MC fair values from
    /// data/definitive_pricing_tables/C15_euro_nig_weth_3d_d2_c15.npy
    const WETH_C15_REFERENCE: &[(f64, f64)] = &[
        (0.90, 0.00010230),
        (1.65, 0.00047399),
        (2.40, 0.00143434),
        (3.15, 0.00334794),
    ];

    /// Tolerance: 5% relative error vs 10M-path MC. The i64/SCALE_6 inner
    /// loop has ~7 sig fig precision (limited by SCALE_6), the A&S K₁ form
    /// is accurate to 2.2e-7, and GL5 is exact for polynomial degree ≤ 9.
    /// 5% leaves room for MC sampling noise (~1-2% at these fair values)
    /// plus the fixed-point precision floor.
    const REL_TOL: f64 = 0.05;

    fn assert_close(label: &str, sigma: f64, expected: f64, got: f64) {
        let rel = if expected > 0.0 {
            ((got - expected) / expected).abs()
        } else {
            got.abs()
        };
        assert!(
            rel < REL_TOL,
            "{} σ={:.3}: expected {:.10}, got {:.10}, rel_err={:.4}%",
            label,
            sigma,
            expected,
            got,
            rel * 100.0
        );
    }

    #[test]
    fn matches_mc_sol_usdc_3d_d2_c15() {
        let mut max_rel = 0.0_f64;
        for &(sigma, expected) in SOL_C15_REFERENCE {
            let p =
                nig_european_il_premium(s(sigma), 3, s(0.02), s(0.15), s(ALPHA_SOL), s(BETA_SOL))
                    .expect("price");
            let got = frac(p);
            let rel = ((got - expected) / expected).abs();
            if rel > max_rel {
                max_rel = rel;
            }
            assert_close("SOL/USDC", sigma, expected, got);
        }
        eprintln!("SOL/USDC max rel err vs MC: {:.4}%", max_rel * 100.0);
    }

    #[test]
    fn matches_mc_weth_sol_3d_d2_c15() {
        let mut max_rel = 0.0_f64;
        for &(sigma, expected) in WETH_C15_REFERENCE {
            let p =
                nig_european_il_premium(s(sigma), 3, s(0.02), s(0.15), s(ALPHA_WETH), s(BETA_WETH))
                    .expect("price");
            let got = frac(p);
            let rel = ((got - expected) / expected).abs();
            if rel > max_rel {
                max_rel = rel;
            }
            assert_close("WETH/SOL", sigma, expected, got);
        }
        eprintln!("WETH/SOL max rel err vs MC: {:.4}%", max_rel * 100.0);
    }

    #[test]
    fn delta_scales_linearly_in_t_not_sqrt_t() {
        // δ_T must be LINEAR in T. If we accidentally used √T, p12/p3 would
        // be ~2× instead of >>2. Empirically ~5–7×.
        let sigma = s(1.0);
        let p3 =
            nig_european_il_premium(sigma, 3, s(0.02), s(0.15), s(ALPHA_SOL), s(BETA_SOL)).unwrap();
        let p12 = nig_european_il_premium(sigma, 12, s(0.02), s(0.15), s(ALPHA_SOL), s(BETA_SOL))
            .unwrap();
        let ratio = (p12 as f64) / (p3 as f64);
        assert!(
            ratio > 3.5,
            "δ_T scaling looks wrong: p12/p3 = {:.2}",
            ratio
        );
    }

    #[test]
    fn monotone_in_sigma() {
        let mut prev: i64 = 0;
        for sigma in [0.5, 0.8, 1.2, 1.6, 2.0, 2.5, 3.0] {
            let p =
                nig_european_il_premium(s(sigma), 3, s(0.02), s(0.15), s(ALPHA_SOL), s(BETA_SOL))
                    .unwrap();
            assert!(p >= prev, "non-monotone at σ={}: {} < {}", sigma, p, prev);
            prev = p;
        }
    }

    #[test]
    fn monotone_in_t() {
        let mut prev: i64 = 0;
        for days in [1u32, 2, 3, 5, 7, 10, 14] {
            let p =
                nig_european_il_premium(s(1.0), days, s(0.02), s(0.15), s(ALPHA_SOL), s(BETA_SOL))
                    .unwrap();
            assert!(p >= prev, "non-monotone at T={}: {} < {}", days, p, prev);
            prev = p;
        }
    }

    #[test]
    fn bounded_by_cap_minus_deductible() {
        let bound = s(0.13); // 0.15 - 0.02
        for sigma in [0.5, 1.0, 2.0, 5.0, 10.0] {
            let p =
                nig_european_il_premium(s(sigma), 3, s(0.02), s(0.15), s(ALPHA_SOL), s(BETA_SOL))
                    .unwrap();
            assert!(
                p <= bound,
                "premium {} exceeds (c-d) bound {} at σ={}",
                p,
                bound,
                sigma
            );
        }
    }

    #[test]
    fn rejects_invalid_inputs() {
        // d >= c
        assert!(
            nig_european_il_premium(s(1.0), 3, s(0.15), s(0.15), s(ALPHA_SOL), s(BETA_SOL))
                .is_err()
        );
        // c >= 0.5
        assert!(
            nig_european_il_premium(s(1.0), 3, s(0.02), s(0.50), s(ALPHA_SOL), s(BETA_SOL))
                .is_err()
        );
        // alpha <= |beta|
        assert!(nig_european_il_premium(s(1.0), 3, s(0.02), s(0.15), s(1.0), s(2.0)).is_err());
    }

    #[test]
    fn clmm_c_one_matches_full_range() {
        // Build plan §B2 exit criterion: C=1 must reduce to B1 exactly.
        for sigma in [0.8, 1.5, 2.5] {
            let base =
                nig_european_il_premium(s(sigma), 3, s(0.02), s(0.15), s(ALPHA_SOL), s(BETA_SOL))
                    .unwrap();
            let clmm = nig_european_il_premium_clmm(
                s(sigma),
                3,
                s(0.02),
                s(0.15),
                s(ALPHA_SOL),
                s(BETA_SOL),
                s(1.0),
            )
            .unwrap();
            // Allow ≤2 ticks of round-trip noise from the d/C and c/C
            // divisions at SCALE_6.
            let diff = (base - clmm).abs();
            assert!(
                diff <= 2,
                "C=1 mismatch at σ={}: base={}, clmm={}, diff={}",
                sigma,
                base,
                clmm,
                diff
            );
        }
    }

    #[test]
    fn clmm_higher_concentration_gives_higher_premium() {
        // Build plan §B2 exit criterion: C > 1 must give higher premium.
        for sigma in [1.0, 2.0] {
            let base = nig_european_il_premium_clmm(
                s(sigma),
                3,
                s(0.02),
                s(0.15),
                s(ALPHA_SOL),
                s(BETA_SOL),
                s(1.0),
            )
            .unwrap();
            let amplified = nig_european_il_premium_clmm(
                s(sigma),
                3,
                s(0.02),
                s(0.15),
                s(ALPHA_SOL),
                s(BETA_SOL),
                s(2.0),
            )
            .unwrap();
            assert!(
                amplified > base,
                "CLMM C=2 not > C=1 at σ={}: base={}, amplified={}",
                sigma,
                base,
                amplified
            );
        }
    }

    #[test]
    fn clmm_rejects_c_below_one() {
        assert!(nig_european_il_premium_clmm(
            s(1.0),
            3,
            s(0.02),
            s(0.15),
            s(ALPHA_SOL),
            s(BETA_SOL),
            s(0.5),
        )
        .is_err());
    }

    #[test]
    fn zero_inputs_return_zero() {
        // sigma == 0 → premium = 0
        assert_eq!(
            nig_european_il_premium(0, 3, s(0.02), s(0.15), s(ALPHA_SOL), s(BETA_SOL)).unwrap(),
            0
        );
        // days == 0 → premium = 0
        assert_eq!(
            nig_european_il_premium(s(1.0), 0, s(0.02), s(0.15), s(ALPHA_SOL), s(BETA_SOL))
                .unwrap(),
            0
        );
    }
}
