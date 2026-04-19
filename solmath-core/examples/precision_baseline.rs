use libm::{cos, erf, exp, log, sin, sqrt};
use serde_json::{json, Value};
use solmath_core::{
    bs_full, bs_full_hp, cos_fixed, exp6, exp_fixed_i, fp_div, fp_mul_i, fp_sqrt, implied_vol, ln6,
    ln_fixed_i, norm_cdf_fast, norm_cdf_poly, norm_pdf, pow_fixed, sin_fixed, sincos_fixed, sqrt6,
    SolMathError, SCALE, SCALE_6, SCALE_I,
};
use std::env;
use std::fs;
use std::path::PathBuf;

const SQRT_2: f64 = 1.414_213_562_373_095_1;
const SQRT_2PI: f64 = 2.506_628_274_631_000_7;

#[derive(Clone, Copy)]
struct BsRef {
    call: f64,
    put: f64,
    delta_call: f64,
    delta_put: f64,
    gamma: f64,
    vega: f64,
    theta_call: f64,
    theta_put: f64,
    rho_call: f64,
    rho_put: f64,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: cargo run -p solmath-core --features full --example precision_baseline -- <output-path>")?;
    let baseline_date = env::var("HALCYON_BASELINE_DATE").unwrap_or_else(|_| "unknown".to_string());
    let git_head = env::var("HALCYON_GIT_HEAD").unwrap_or_else(|_| "unknown".to_string());

    let boundary = run_boundary_checks();
    let scalar = run_scalar_transcendental_checks();
    let scalar_i64 = run_i64_scalar_checks();
    let trig_public = run_public_trig_checks();
    let norm_poly = run_norm_poly_checks();
    let norm_fast = run_norm_fast_checks();
    let bs_hp = run_bs_full_hp_checks();
    let iv = run_implied_vol_checks();

    let overall_pass = boundary["pass"].as_bool().unwrap_or(false)
        && scalar["pass"].as_bool().unwrap_or(false)
        && trig_public["pass"].as_bool().unwrap_or(false)
        && norm_poly["pass"].as_bool().unwrap_or(false)
        && norm_fast["pass"].as_bool().unwrap_or(false)
        && bs_hp["pass"].as_bool().unwrap_or(false)
        && iv["pass"].as_bool().unwrap_or(false);

    let report = json!({
        "baseline_date": baseline_date,
        "git_head": git_head,
        "crate": "solmath-core",
        "crate_version": env!("CARGO_PKG_VERSION"),
        "generator": "cargo run -p solmath-core --features full --example precision_baseline",
        "status": if overall_pass { "pass" } else { "fail" },
        "boundary": boundary,
        "scalar_transcendentals": scalar,
        "scalar_i64": scalar_i64,
        "trig_public": trig_public,
        "norm_poly": norm_poly,
        "norm_fast": norm_fast,
        "bs_full_hp": bs_hp,
        "implied_vol": iv
    });

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
    println!("{}", output.display());

    if overall_pass {
        Ok(())
    } else {
        Err("precision baseline checks failed".into())
    }
}

fn run_boundary_checks() -> Value {
    let mut cases = Vec::new();
    let mut pass_count = 0usize;

    macro_rules! push_case {
        ($name:expr, $pass:expr, $detail:expr) => {{
            let pass = $pass;
            if pass {
                pass_count += 1;
            }
            cases.push(json!({
                "name": $name,
                "pass": pass,
                "detail": $detail,
            }));
        }};
    }

    push_case!(
        "exp_fixed_i(40) -> overflow",
        exp_fixed_i(40 * SCALE_I) == Err(SolMathError::Overflow),
        format!("{:?}", exp_fixed_i(40 * SCALE_I))
    );
    push_case!(
        "exp_fixed_i(-40) -> zero",
        exp_fixed_i(-40 * SCALE_I) == Ok(0),
        format!("{:?}", exp_fixed_i(-40 * SCALE_I))
    );
    let exp_39 = exp_fixed_i(39 * SCALE_I).unwrap();
    let exp_39_ref = scale_i_from_f64(exp(39.0));
    push_case!(
        "exp_fixed_i(39) stays computable",
        relative_error(exp_39 as f64, exp_39_ref as f64) <= 3.0e-8,
        format!(
            "actual={exp_39} expected={exp_39_ref} rel_err={:.3e}",
            relative_error(exp_39 as f64, exp_39_ref as f64)
        )
    );
    let exp_10 = exp_fixed_i(10 * SCALE_I).unwrap();
    let exp_10_ref = scale_i_from_f64(exp(10.0));
    push_case!(
        "exp_fixed_i(10) matches host ref",
        relative_error(exp_10 as f64, exp_10_ref as f64) <= 3.0e-8,
        format!(
            "actual={exp_10} expected={exp_10_ref} rel_err={:.3e}",
            relative_error(exp_10 as f64, exp_10_ref as f64)
        )
    );
    push_case!(
        "fp_div(..., 0) -> division by zero",
        fp_div(SCALE, 0) == Err(SolMathError::DivisionByZero),
        format!("{:?}", fp_div(SCALE, 0))
    );
    push_case!(
        "pow_fixed(0, 0) -> domain error",
        pow_fixed(0, 0) == Err(SolMathError::DomainError),
        format!("{:?}", pow_fixed(0, 0))
    );
    push_case!(
        "exp6(20) -> overflow",
        exp6(20 * SCALE_6) == Err(SolMathError::Overflow),
        format!("{:?}", exp6(20 * SCALE_6))
    );
    push_case!(
        "exp6(-20) -> zero",
        exp6(-20 * SCALE_6) == Ok(0),
        format!("{:?}", exp6(-20 * SCALE_6))
    );

    json!({
        "pass": pass_count == cases.len(),
        "total": cases.len(),
        "passed": pass_count,
        "cases": cases,
    })
}

fn run_scalar_transcendental_checks() -> Value {
    let mut exp_ulps = Vec::new();
    let mut exp_rel = Vec::new();
    for i in 0..=4000 {
        let x = -10.0 + 20.0 * i as f64 / 4000.0;
        let x_fp = scale_i_from_f64(x);
        let actual = exp_fixed_i(x_fp).unwrap();
        let expected = scale_i_from_f64(exp(x));
        exp_ulps.push(abs_diff_i128(actual, expected));
        exp_rel.push(relative_error(actual as f64, expected as f64));
    }

    let mut ln_ulps = Vec::new();
    let mut ln_rel = Vec::new();
    for i in 1..=4000 {
        let x = 0.01 + 19.99 * i as f64 / 4000.0;
        let x_fp = scale_u_from_f64(x);
        let actual = ln_fixed_i(x_fp).unwrap();
        let expected = scale_i_from_f64(log(x));
        ln_ulps.push(abs_diff_i128(actual, expected));
        ln_rel.push(relative_error(actual as f64, expected as f64));
    }

    let mut sqrt_ulps = Vec::new();
    let mut sqrt_rel = Vec::new();
    for i in 0..=4000 {
        let x = 25.0 * i as f64 / 4000.0;
        let x_fp = scale_u_from_f64(x);
        let actual = fp_sqrt(x_fp).unwrap();
        let expected = scale_u_from_f64(sqrt(x));
        sqrt_ulps.push(abs_diff_u128(actual, expected));
        sqrt_rel.push(relative_error(actual as f64, expected as f64));
    }

    let mut pow_ulps = Vec::new();
    let mut pow_rel = Vec::new();
    let bases = [0.1, 0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 4.0, 10.0];
    let exponents = [0.25, 0.5, 1.0, 1.5, 2.0, 5.0];
    for &base in &bases {
        for &exponent in &exponents {
            let actual = pow_fixed(scale_u_from_f64(base), scale_u_from_f64(exponent)).unwrap();
            let expected = scale_u_from_f64(exp(log(base) * exponent));
            pow_ulps.push(abs_diff_u128(actual, expected));
            pow_rel.push(relative_error(actual as f64, expected as f64));
        }
    }

    let exp_ulp = summary_u64(&mut exp_ulps);
    let exp_relative = summary_f64(&mut exp_rel);
    let ln_ulp = summary_u64(&mut ln_ulps);
    let ln_relative = summary_f64(&mut ln_rel);
    let sqrt_ulp = summary_u64(&mut sqrt_ulps);
    let sqrt_relative = summary_f64(&mut sqrt_rel);
    let pow_ulp = summary_u64(&mut pow_ulps);
    let pow_relative = summary_f64(&mut pow_rel);
    let pass = exp_relative["max"].as_f64().unwrap_or(f64::INFINITY) <= 3.0e-8
        && ln_ulp["max"].as_u64().unwrap_or(u64::MAX) <= 2
        && ln_relative["max"].as_f64().unwrap_or(f64::INFINITY) <= 3.0e-10
        && sqrt_ulp["max"].as_u64().unwrap_or(u64::MAX) <= 1
        && sqrt_relative["max"].as_f64().unwrap_or(f64::INFINITY) <= 1.0e-10
        && pow_ulp["max"].as_u64().unwrap_or(u64::MAX) <= 128
        && pow_relative["max"].as_f64().unwrap_or(f64::INFINITY) <= 1.0e-10;

    json!({
        "pass": pass,
        "exp_fixed_i": {
            "ulp": exp_ulp,
            "relative": exp_relative,
        },
        "ln_fixed_i": {
            "ulp": ln_ulp,
            "relative": ln_relative,
        },
        "fp_sqrt": {
            "ulp": sqrt_ulp,
            "relative": sqrt_relative,
        },
        "pow_fixed": {
            "ulp": pow_ulp,
            "relative": pow_relative,
        }
    })
}

fn run_i64_scalar_checks() -> Value {
    let mut exp_ulps = Vec::new();
    let mut exp_rel = Vec::new();
    for i in 0..=3900 {
        let x = -19.5 + 39.0 * i as f64 / 3900.0;
        let x_fp = scale6_from_f64(x);
        let actual = exp6(x_fp).unwrap();
        let expected = scale6_from_f64(exp(x));
        exp_ulps.push(abs_diff_i64(actual, expected));
        exp_rel.push(relative_error(actual as f64, expected as f64));
    }

    let mut ln_ulps = Vec::new();
    let mut ln_rel = Vec::new();
    for i in 1..=4000 {
        let x = 0.01 + 19.99 * i as f64 / 4000.0;
        let x_fp = scale6_from_f64(x);
        let actual = ln6(x_fp).unwrap();
        let expected = scale6_from_f64(log(x));
        ln_ulps.push(abs_diff_i64(actual, expected));
        ln_rel.push(relative_error(actual as f64, expected as f64));
    }

    let mut sqrt_ulps = Vec::new();
    let mut sqrt_rel = Vec::new();
    for i in 0..=4000 {
        let x = 25.0 * i as f64 / 4000.0;
        let x_fp = scale6_from_f64(x);
        let actual = sqrt6(x_fp).unwrap();
        let expected = scale6_from_f64(sqrt(x));
        sqrt_ulps.push(abs_diff_i64(actual, expected));
        sqrt_rel.push(relative_error(actual as f64, expected as f64));
    }

    json!({
        "exp6": {
            "ulp": summary_u64(&mut exp_ulps),
            "relative": summary_f64(&mut exp_rel),
        },
        "ln6": {
            "ulp": summary_u64(&mut ln_ulps),
            "relative": summary_f64(&mut ln_rel),
        },
        "sqrt6": {
            "ulp": summary_u64(&mut sqrt_ulps),
            "relative": summary_f64(&mut sqrt_rel),
        }
    })
}

fn run_public_trig_checks() -> Value {
    let mut sin_ulps = Vec::new();
    let mut cos_ulps = Vec::new();
    let mut consistency_sin = Vec::new();
    let mut consistency_cos = Vec::new();
    let mut pythagorean = Vec::new();

    for i in 0..=20_000 {
        let x = -32.0 * std::f64::consts::PI + 64.0 * std::f64::consts::PI * i as f64 / 20_000.0;
        let x_fp = scale_i_from_f64(x);
        let sin_actual = sin_fixed(x_fp).unwrap();
        let cos_actual = cos_fixed(x_fp).unwrap();
        let (sin_pair, cos_pair) = sincos_fixed(x_fp).unwrap();
        let sin_expected = scale_i_from_f64(sin(x));
        let cos_expected = scale_i_from_f64(cos(x));
        sin_ulps.push(abs_diff_i128(sin_actual, sin_expected));
        cos_ulps.push(abs_diff_i128(cos_actual, cos_expected));
        consistency_sin.push(abs_diff_i128(sin_actual, sin_pair));
        consistency_cos.push(abs_diff_i128(cos_actual, cos_pair));
        let sum_sq =
            fp_mul_i(sin_actual, sin_actual).unwrap() + fp_mul_i(cos_actual, cos_actual).unwrap();
        pythagorean.push(abs_diff_i128(sum_sq, SCALE_I));
    }

    let sin_summary = summary_u64(&mut sin_ulps);
    let cos_summary = summary_u64(&mut cos_ulps);
    let pyth_summary = summary_u64(&mut pythagorean);
    let consistency_sin_summary = summary_u64(&mut consistency_sin);
    let consistency_cos_summary = summary_u64(&mut consistency_cos);
    let pass = sin_summary["max"].as_u64().unwrap_or(u64::MAX) <= 8
        && cos_summary["max"].as_u64().unwrap_or(u64::MAX) <= 8
        && pyth_summary["max"].as_u64().unwrap_or(u64::MAX) <= 4
        && consistency_sin_summary["max"].as_u64().unwrap_or(u64::MAX) == 0
        && consistency_cos_summary["max"].as_u64().unwrap_or(u64::MAX) == 0;

    json!({
        "pass": pass,
        "samples": 20_001,
        "sin": sin_summary,
        "cos": cos_summary,
        "sincos_consistency_sin": consistency_sin_summary,
        "sincos_consistency_cos": consistency_cos_summary,
        "pythagorean_identity": pyth_summary,
    })
}

fn run_norm_poly_checks() -> Value {
    let mut cdf_ulps = Vec::new();
    let mut pdf_ulps = Vec::new();
    for i in 0..=20_000 {
        let x = -8.0 + 16.0 * i as f64 / 20_000.0;
        let x_fp = scale_i_from_f64(x);
        let cdf_actual = norm_cdf_poly(x_fp).unwrap();
        let pdf_actual = norm_pdf(x_fp).unwrap();
        let cdf_expected = scale_i_from_f64(norm_cdf_ref(x));
        let pdf_expected = scale_i_from_f64(norm_pdf_ref(x));
        cdf_ulps.push(abs_diff_i128(cdf_actual, cdf_expected));
        pdf_ulps.push(abs_diff_i128(pdf_actual, pdf_expected));
    }

    let cdf_summary = summary_u64(&mut cdf_ulps);
    let pdf_summary = summary_u64(&mut pdf_ulps);
    let pass = cdf_summary["max"].as_u64().unwrap_or(u64::MAX) <= 4
        && pdf_summary["max"].as_u64().unwrap_or(u64::MAX) <= 2;

    json!({
        "pass": pass,
        "samples": 20_001,
        "cdf": cdf_summary,
        "pdf": pdf_summary,
    })
}

fn run_norm_fast_checks() -> Value {
    let mut overall = Vec::new();
    let mut interior = Vec::new();
    let mut mid = Vec::new();
    let mut tail = Vec::new();

    for i in 0..=20_000 {
        let x = -4.0 + 8.0 * i as f64 / 20_000.0;
        let x_fp = scale_i_from_f64(x);
        let actual = norm_cdf_fast(x_fp).unwrap() as f64 / SCALE_I as f64;
        let expected = norm_cdf_ref(x);
        let err = (actual - expected).abs();
        overall.push(err);
        let ax = x.abs();
        if ax <= 3.0 {
            interior.push(err);
        } else if ax <= 3.5 {
            mid.push(err);
        } else {
            tail.push(err);
        }
    }

    let overall_summary = summary_f64(&mut overall);
    let interior_summary = summary_f64(&mut interior);
    let mid_summary = summary_f64(&mut mid);
    let tail_summary = summary_f64(&mut tail);
    let pass = overall_summary["max"].as_f64().unwrap_or(f64::INFINITY) < 7.0e-5
        && interior_summary["max"].as_f64().unwrap_or(f64::INFINITY) < 7.0e-5
        && mid_summary["max"].as_f64().unwrap_or(f64::INFINITY) < 2.5e-6
        && tail_summary["max"].as_f64().unwrap_or(f64::INFINITY) < 2.0e-6;

    json!({
        "pass": pass,
        "samples": 20_001,
        "overall_abs_error": overall_summary,
        "interior_abs_error": interior_summary,
        "mid_abs_error": mid_summary,
        "tail_abs_error": tail_summary,
    })
}

fn run_bs_full_hp_checks() -> Value {
    let mut call = Vec::new();
    let mut put = Vec::new();
    let mut delta = Vec::new();
    let mut gamma = Vec::new();
    let mut vega = Vec::new();
    let mut theta = Vec::new();
    let mut rho = Vec::new();

    let spots = [80.0, 90.0, 100.0, 110.0, 120.0];
    let strikes = [80.0, 90.0, 100.0, 110.0, 120.0];
    let rates = [0.0, 0.02, 0.05];
    let sigmas = [0.1, 0.2, 0.5];
    let times = [0.25, 0.5, 1.0];
    let mut samples = 0usize;

    for &s in &spots {
        for &k in &strikes {
            for &r in &rates {
                for &sigma in &sigmas {
                    for &t in &times {
                        let actual = bs_full_hp(
                            scale_u_from_f64(s),
                            scale_u_from_f64(k),
                            scale_u_from_f64(r),
                            scale_u_from_f64(sigma),
                            scale_u_from_f64(t),
                        )
                        .unwrap();
                        let expected = bs_ref(s, k, r, sigma, t);
                        samples += 1;

                        call.push(abs_diff_u128(actual.call, scale_u_from_f64(expected.call)));
                        put.push(abs_diff_u128(actual.put, scale_u_from_f64(expected.put)));
                        delta.push(abs_diff_i128(
                            actual.call_delta,
                            scale_i_from_f64(expected.delta_call),
                        ));
                        delta.push(abs_diff_i128(
                            actual.put_delta,
                            scale_i_from_f64(expected.delta_put),
                        ));
                        gamma.push(abs_diff_i128(
                            actual.gamma,
                            scale_i_from_f64(expected.gamma),
                        ));
                        vega.push(abs_diff_i128(actual.vega, scale_i_from_f64(expected.vega)));
                        theta.push(abs_diff_i128(
                            actual.call_theta,
                            scale_i_from_f64(expected.theta_call),
                        ));
                        theta.push(abs_diff_i128(
                            actual.put_theta,
                            scale_i_from_f64(expected.theta_put),
                        ));
                        rho.push(abs_diff_i128(
                            actual.call_rho,
                            scale_i_from_f64(expected.rho_call),
                        ));
                        rho.push(abs_diff_i128(
                            actual.put_rho,
                            scale_i_from_f64(expected.rho_put),
                        ));
                    }
                }
            }
        }
    }

    let call_summary = summary_u64(&mut call);
    let put_summary = summary_u64(&mut put);
    let delta_summary = summary_u64(&mut delta);
    let gamma_summary = summary_u64(&mut gamma);
    let vega_summary = summary_u64(&mut vega);
    let theta_summary = summary_u64(&mut theta);
    let rho_summary = summary_u64(&mut rho);
    let pass = call_summary["max"].as_u64().unwrap_or(u64::MAX) <= 4
        && put_summary["max"].as_u64().unwrap_or(u64::MAX) <= 4
        && delta_summary["max"].as_u64().unwrap_or(u64::MAX) <= 4
        && gamma_summary["max"].as_u64().unwrap_or(u64::MAX) <= 4
        && vega_summary["max"].as_u64().unwrap_or(u64::MAX) <= 4
        && theta_summary["max"].as_u64().unwrap_or(u64::MAX) <= 8
        && rho_summary["max"].as_u64().unwrap_or(u64::MAX) <= 4;

    json!({
        "pass": pass,
        "samples": samples,
        "call": call_summary,
        "put": put_summary,
        "delta": delta_summary,
        "gamma": gamma_summary,
        "vega": vega_summary,
        "theta": theta_summary,
        "rho": rho_summary,
    })
}

fn run_implied_vol_checks() -> Value {
    let mut easy = Vec::new();
    let mut moderate = Vec::new();
    let mut hard = Vec::new();
    let mut total = 0usize;
    let mut passed = 0usize;

    let s = 100.0;
    let strikes = [80.0, 90.0, 100.0, 110.0, 120.0];
    let rates = [0.05];
    let sigmas = [0.1, 0.2, 0.5, 1.0];
    let times = [0.1, 0.25, 0.5, 1.0];

    for &k in &strikes {
        for &r in &rates {
            for &sigma in &sigmas {
                for &t in &times {
                    let bs = bs_full(
                        scale_u_from_f64(s),
                        scale_u_from_f64(k),
                        scale_u_from_f64(r),
                        scale_u_from_f64(sigma),
                        scale_u_from_f64(t),
                    )
                    .unwrap();
                    if bs.call < 2 {
                        continue;
                    }

                    total += 1;
                    let sigma_in = scale_u_from_f64(sigma);
                    let sigma_out = match implied_vol(
                        bs.call,
                        scale_u_from_f64(s),
                        scale_u_from_f64(k),
                        scale_u_from_f64(r),
                        scale_u_from_f64(t),
                    ) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    let err = abs_diff_u128(sigma_out, sigma_in);
                    if err <= 1_000 {
                        passed += 1;
                    }

                    let moneyness_bp = (((k / s) - 1.0).abs() * 10_000.0).round() as u32;
                    if moneyness_bp <= 1_000 && t >= 0.25 && sigma <= 0.5 {
                        easy.push(err);
                    } else if moneyness_bp >= 2_000 || t <= 0.1 || sigma >= 1.0 {
                        hard.push(err);
                    } else {
                        moderate.push(err);
                    }
                }
            }
        }
    }

    let easy_summary = summary_u64(&mut easy);
    let moderate_summary = summary_u64(&mut moderate);
    let hard_summary = summary_u64(&mut hard);
    let pass_rate = if total == 0 {
        0.0
    } else {
        passed as f64 / total as f64
    };

    json!({
        "pass": pass_rate >= 0.85,
        "total": total,
        "passed": passed,
        "pass_rate": pass_rate,
        "easy": easy_summary,
        "moderate": moderate_summary,
        "hard": hard_summary,
    })
}

fn bs_ref(s: f64, k: f64, r: f64, sigma: f64, t: f64) -> BsRef {
    let sqrt_t = sqrt(t);
    let sigma_sqrt_t = sigma * sqrt_t;
    let d1 = (log(s / k) + (r + 0.5 * sigma * sigma) * t) / sigma_sqrt_t;
    let d2 = d1 - sigma_sqrt_t;
    let phi_d1 = norm_cdf_ref(d1);
    let phi_d2 = norm_cdf_ref(d2);
    let pdf_d1 = norm_pdf_ref(d1);
    let discount = exp(-r * t);
    let call = s * phi_d1 - k * discount * phi_d2;
    let put = k * discount * norm_cdf_ref(-d2) - s * norm_cdf_ref(-d1);
    let delta_call = phi_d1;
    let delta_put = phi_d1 - 1.0;
    let gamma = pdf_d1 / (s * sigma_sqrt_t);
    let vega = s * pdf_d1 * sqrt_t;
    let theta_common = -(s * pdf_d1 * sigma) / (2.0 * sqrt_t);
    let theta_call = theta_common - r * k * discount * phi_d2;
    let theta_put = theta_common + r * k * discount * norm_cdf_ref(-d2);
    let rho_call = k * t * discount * phi_d2;
    let rho_put = -k * t * discount * norm_cdf_ref(-d2);

    BsRef {
        call,
        put,
        delta_call,
        delta_put,
        gamma,
        vega,
        theta_call,
        theta_put,
        rho_call,
        rho_put,
    }
}

fn norm_cdf_ref(x: f64) -> f64 {
    0.5 * (1.0 + erf(x / SQRT_2))
}

fn norm_pdf_ref(x: f64) -> f64 {
    exp(-0.5 * x * x) / SQRT_2PI
}

fn scale_i_from_f64(x: f64) -> i128 {
    (x * SCALE_I as f64).round() as i128
}

fn scale_u_from_f64(x: f64) -> u128 {
    (x * SCALE as f64).round() as u128
}

fn scale6_from_f64(x: f64) -> i64 {
    (x * SCALE_6 as f64).round() as i64
}

fn abs_diff_i128(a: i128, b: i128) -> u64 {
    a.abs_diff(b).min(u64::MAX as u128) as u64
}

fn abs_diff_u128(a: u128, b: u128) -> u64 {
    a.abs_diff(b).min(u64::MAX as u128) as u64
}

fn abs_diff_i64(a: i64, b: i64) -> u64 {
    a.abs_diff(b)
}

fn relative_error(actual: f64, expected: f64) -> f64 {
    let denom = expected.abs().max(1.0);
    (actual - expected).abs() / denom
}

fn summary_u64(errors: &mut Vec<u64>) -> Value {
    if errors.is_empty() {
        return json!({"n": 0, "max": 0, "p99": 0, "median": 0, "mean": 0.0});
    }
    errors.sort_unstable();
    let n = errors.len();
    let sum: u128 = errors.iter().map(|&x| x as u128).sum();
    json!({
        "n": n,
        "max": errors[n - 1],
        "p99": errors[((n as f64 * 0.99).ceil() as usize).saturating_sub(1).min(n - 1)],
        "median": errors[n / 2],
        "mean": sum as f64 / n as f64,
    })
}

fn summary_f64(errors: &mut Vec<f64>) -> Value {
    if errors.is_empty() {
        return json!({"n": 0, "max": 0.0, "p99": 0.0, "median": 0.0, "mean": 0.0});
    }
    errors.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = errors.len();
    let sum: f64 = errors.iter().sum();
    json!({
        "n": n,
        "max": errors[n - 1],
        "p99": errors[((n as f64 * 0.99).ceil() as usize).saturating_sub(1).min(n - 1)],
        "median": errors[n / 2],
        "mean": sum / n as f64,
    })
}
