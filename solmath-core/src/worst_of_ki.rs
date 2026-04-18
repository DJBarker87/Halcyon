use crate::arithmetic::{fp_div_i, fp_mul_i, fp_sqrt};
use crate::error::SolMathError;
use crate::gauss_hermite::{gh_rule, INV_SQRT_PI};
use crate::transcendental::exp_fixed_i;
use crate::SCALE_I;

const SQRT_2: f64 = core::f64::consts::SQRT_2;

/// One affine log-return coordinate in `(u, v)` conditional space.
///
/// The log return is:
///
/// `x = constant + u_coeff * u + v_coeff * v`
///
/// All fields are signed fixed-point values at `SCALE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AffineLogCoordinate {
    pub constant: i128,
    pub u_coeff: i128,
    pub v_coeff: i128,
}

/// KI-region moment outputs at `SCALE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorstOfKiMoment {
    /// `P(min(x_i) <= barrier_log)` at `SCALE`.
    pub ki_probability: i128,
    /// `E[min(exp(x_i)) * 1_{KI}]` at `SCALE`.
    pub worst_indicator_expectation: i128,
}

#[inline]
fn clamp_prob(value: i128) -> i128 {
    value.clamp(0, SCALE_I)
}

#[inline]
fn clamp_unit_interval(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

#[inline]
fn to_f64(value: i128) -> f64 {
    value as f64 / SCALE_I as f64
}

#[inline]
fn from_unit_scale(value: f64) -> Result<i128, SolMathError> {
    if !value.is_finite() {
        return Err(SolMathError::DomainError);
    }
    let scaled = value * SCALE_I as f64;
    if !scaled.is_finite() || scaled < i128::MIN as f64 || scaled > i128::MAX as f64 {
        return Err(SolMathError::Overflow);
    }
    let rounded = if scaled >= 0.0 {
        scaled + 0.5
    } else {
        scaled - 0.5
    };
    Ok(rounded as i128)
}

#[inline]
fn sqrt_fixed_to_f64(value: i128) -> Result<f64, SolMathError> {
    if value <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    Ok(fp_sqrt(value as u128)? as f64 / SCALE_I as f64)
}

/// Deterministic KI-region moment for the 3-name worst-of spread plane.
///
/// Inputs:
/// - `mean_u`, `mean_v`: conditional Gaussian means for `(u, v)` at `SCALE`.
/// - `var_uu`, `cov_uv`, `var_vv`: conditional covariance entries at `SCALE`.
/// - `barrier_log`: knock-in barrier in log space at `SCALE`.
/// - `coords`: affine maps from `(u, v)` to each asset log return.
///
/// Returns:
/// - `ki_probability = P(any x_i <= barrier_log)` at `SCALE`.
/// - `worst_indicator_expectation = E[min(exp(x_i)) * 1_{KI}]` at `SCALE`.
///
/// Error conditions:
/// - `DegenerateVariance` if the covariance is not positive definite.
/// - `DomainError` if the quadrature state is non-finite.
/// - `Overflow` if the final moment cannot be represented at `SCALE`.
///
/// Accuracy:
/// - Uses 13x13 Gauss-Hermite quadrature on the 2D conditional Gaussian.
/// - Intended as a deterministic host/on-chain helper for the KI leg.
pub fn worst_of_ki_moment(
    mean_u: i128,
    mean_v: i128,
    var_uu: i128,
    cov_uv: i128,
    var_vv: i128,
    barrier_log: i128,
    coords: [AffineLogCoordinate; 3],
) -> Result<WorstOfKiMoment, SolMathError> {
    worst_of_ki_moment_with_order(
        mean_u,
        mean_v,
        var_uu,
        cov_uv,
        var_vv,
        barrier_log,
        coords,
        13,
    )
}

/// Deterministic KI-region moment with a selectable Gauss-Hermite rule.
///
/// Supported `quadrature_order` values are `3`, `4`, `5`, `6`, `7`, `8`, `9`,
/// `10`, and `13`.
///
/// Error conditions:
/// - `DegenerateVariance` if the covariance is not positive definite.
/// - `DomainError` if the quadrature order is unsupported or the state is non-finite.
/// - `Overflow` if the final moment cannot be represented at `SCALE`.
pub fn worst_of_ki_moment_with_order(
    mean_u: i128,
    mean_v: i128,
    var_uu: i128,
    cov_uv: i128,
    var_vv: i128,
    barrier_log: i128,
    coords: [AffineLogCoordinate; 3],
    quadrature_order: usize,
) -> Result<WorstOfKiMoment, SolMathError> {
    if var_uu <= 0 || var_vv <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }
    let (gh_nodes, gh_weights) = gh_rule(quadrature_order).ok_or(SolMathError::DomainError)?;

    let mean_u_f = to_f64(mean_u);
    let mean_v_f = to_f64(mean_v);
    let var_u_f = to_f64(var_uu);
    let cov_uv_f = to_f64(cov_uv);
    let var_v_f = to_f64(var_vv);
    let barrier_f = to_f64(barrier_log);
    if !mean_u_f.is_finite()
        || !mean_v_f.is_finite()
        || !var_u_f.is_finite()
        || !cov_uv_f.is_finite()
        || !var_v_f.is_finite()
        || !barrier_f.is_finite()
    {
        return Err(SolMathError::DomainError);
    }

    let cov_sq = fp_mul_i(cov_uv, cov_uv)?;
    let cond_var_fixed = var_vv
        .checked_sub(fp_div_i(cov_sq, var_uu)?)
        .ok_or(SolMathError::Overflow)?;
    if cond_var_fixed <= 0 {
        return Err(SolMathError::DegenerateVariance);
    }

    let l11 = sqrt_fixed_to_f64(var_uu)?;
    let l21 = cov_uv_f / l11;
    let cond_var = to_f64(cond_var_fixed);
    if cond_var <= 0.0 || !cond_var.is_finite() {
        return Err(SolMathError::DegenerateVariance);
    }
    let l22 = sqrt_fixed_to_f64(cond_var_fixed)?;
    let inv_sqrt_pi = INV_SQRT_PI as f64 / SCALE_I as f64;

    let mut ki_probability = 0.0_f64;
    let mut worst_indicator_expectation = 0.0_f64;

    for i in 0..gh_nodes.len() {
        let z1 = gh_nodes[i] as f64 / SCALE_I as f64;
        let w1 = gh_weights[i] as f64 / SCALE_I as f64;
        for j in 0..gh_nodes.len() {
            let z2 = gh_nodes[j] as f64 / SCALE_I as f64;
            let w2 = gh_weights[j] as f64 / SCALE_I as f64;
            let u = mean_u_f + SQRT_2 * l11 * z1;
            let v = mean_v_f + SQRT_2 * (l21 * z1 + l22 * z2);
            if !u.is_finite() || !v.is_finite() {
                return Err(SolMathError::DomainError);
            }
            let mut x = [0.0_f64; 3];
            for idx in 0..3 {
                x[idx] = to_f64(coords[idx].constant)
                    + to_f64(coords[idx].u_coeff) * u
                    + to_f64(coords[idx].v_coeff) * v;
                if !x[idx].is_finite() {
                    return Err(SolMathError::DomainError);
                }
            }

            let worst_log = x[0].min(x[1]).min(x[2]);
            let is_ki = x[0] <= barrier_f || x[1] <= barrier_f || x[2] <= barrier_f;
            let weight = inv_sqrt_pi * w1 * inv_sqrt_pi * w2;
            if !weight.is_finite() || weight <= 0.0 {
                continue;
            }
            if is_ki {
                ki_probability += weight;
                let worst_log_fixed = from_unit_scale(worst_log)?;
                let worst_level = exp_fixed_i(worst_log_fixed)? as f64 / SCALE_I as f64;
                worst_indicator_expectation += weight * worst_level;
            }
        }
    }

    Ok(WorstOfKiMoment {
        ki_probability: clamp_prob(from_unit_scale(clamp_unit_interval(ki_probability))?),
        worst_indicator_expectation: from_unit_scale(worst_indicator_expectation.max(0.0))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ref_moment(
        mean_u: f64,
        mean_v: f64,
        var_u: f64,
        cov_uv: f64,
        var_v: f64,
        barrier: f64,
        coords: [(f64, f64, f64); 3],
    ) -> (f64, f64) {
        const GH9_NODES: [f64; 9] = [
            -3.190_993_201_782,
            -2.266_580_584_532,
            -1.468_553_289_217,
            -0.723_551_018_753,
            0.0,
            0.723_551_018_753,
            1.468_553_289_217,
            2.266_580_584_532,
            3.190_993_201_782,
        ];
        const GH9_WEIGHTS: [f64; 9] = [
            0.000_039_606_977,
            0.004_943_624_276,
            0.088_474_527_394,
            0.432_651_559_003,
            0.720_235_215_606,
            0.432_651_559_003,
            0.088_474_527_394,
            0.004_943_624_276,
            0.000_039_606_977,
        ];
        let l11 = var_u.sqrt();
        let l21 = cov_uv / l11;
        let l22 = (var_v - l21 * l21).sqrt();
        let inv_sqrt_pi = 1.0 / core::f64::consts::PI.sqrt();
        let mut probability = 0.0_f64;
        let mut expectation = 0.0_f64;
        for i in 0..GH9_NODES.len() {
            for j in 0..GH9_NODES.len() {
                let z1 = GH9_NODES[i];
                let z2 = GH9_NODES[j];
                let u = mean_u + SQRT_2 * l11 * z1;
                let v = mean_v + SQRT_2 * (l21 * z1 + l22 * z2);
                let mut x = [0.0_f64; 3];
                for idx in 0..3 {
                    x[idx] = coords[idx].0 + coords[idx].1 * u + coords[idx].2 * v;
                }
                let weight = inv_sqrt_pi * GH9_WEIGHTS[i] * inv_sqrt_pi * GH9_WEIGHTS[j];
                if x[0] <= barrier || x[1] <= barrier || x[2] <= barrier {
                    probability += weight;
                    expectation += weight * x[0].min(x[1]).min(x[2]).exp();
                }
            }
        }
        (probability, expectation)
    }

    #[test]
    fn worst_of_ki_moment_matches_high_order_reference() {
        let cases = [
            (
                0.03,
                0.01,
                0.12,
                -0.03,
                0.18,
                -0.223_143_551_314_209_7,
                [
                    (0.02, -0.65, -0.40),
                    (0.01, 0.35, -0.40),
                    (-0.01, -0.65, 0.60),
                ],
            ),
            (
                -0.02,
                0.04,
                0.09,
                0.01,
                0.14,
                -0.223_143_551_314_209_7,
                [
                    (-0.01, -0.55, -0.45),
                    (0.00, 0.45, -0.45),
                    (0.02, -0.55, 0.55),
                ],
            ),
        ];

        for case in cases {
            let (mean_u, mean_v, var_u, cov_uv, var_v, barrier, coords_f) = case;
            let got = worst_of_ki_moment(
                (mean_u * SCALE_I as f64).round() as i128,
                (mean_v * SCALE_I as f64).round() as i128,
                (var_u * SCALE_I as f64).round() as i128,
                (cov_uv * SCALE_I as f64).round() as i128,
                (var_v * SCALE_I as f64).round() as i128,
                (barrier * SCALE_I as f64).round() as i128,
                coords_f.map(|(constant, u_coeff, v_coeff)| AffineLogCoordinate {
                    constant: (constant * SCALE_I as f64).round() as i128,
                    u_coeff: (u_coeff * SCALE_I as f64).round() as i128,
                    v_coeff: (v_coeff * SCALE_I as f64).round() as i128,
                }),
            )
            .expect("moment should succeed");

            let (want_prob, want_exp) =
                ref_moment(mean_u, mean_v, var_u, cov_uv, var_v, barrier, coords_f);
            let got_prob = got.ki_probability as f64 / SCALE_I as f64;
            let got_exp = got.worst_indicator_expectation as f64 / SCALE_I as f64;
            assert!(
                (got_prob - want_prob).abs() < 1.5e-1,
                "probability mismatch: got={got_prob} want={want_prob}"
            );
            assert!(
                (got_exp - want_exp).abs() < 1.5e-1,
                "expectation mismatch: got={got_exp} want={want_exp}"
            );
        }
    }
}
