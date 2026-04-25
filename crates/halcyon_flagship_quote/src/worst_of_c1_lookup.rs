//! C1 lookup path: live triangle/KI recursion with precomputed survivor-state moments.
//!
//! The offline generator stores safe/knocked survivor-class moments from the
//! exact onchain-v1 continuation. On-chain we keep the common-factor
//! quadrature and all payoff probabilities live:
//! - NIG weights from `sigma_common`
//! - triangle probabilities at each observation and node
//! - GH3 triple-complement correction
//! - GH3 KI moment at maturity
//! - safe / knocked survival recursion

use crate::exact_leg_tables::{
    lookup_table as exact_lookup_table, EXACT_COUPON_ANNUITY_S6, EXACT_FAIR_COUPON_FRAC_S6,
    EXACT_MATURITY_KNOCK_IN_FRAC_S6, EXACT_MATURITY_SAFE_FRAC_S6, EXACT_OBS_AUTOCALL_HIT_S6,
    EXACT_OBS_COUPON_U0_S6, EXACT_OBS_FIRST_KI_S6, EXACT_OBS_REDEMPTION_FRAC_S6,
    EXACT_OBS_SURVIVAL_S6, EXACT_ZERO_COUPON_FRAC_S6,
};
use crate::moment_tables::{
    moment_interp, moment_lookup, moment_lookup_with_interp, MOMENT_CF_KNOCKED, MOMENT_CF_SAFE,
    MOMENT_EU_KNOCKED, MOMENT_EU_SAFE, MOMENT_EV_KNOCKED, MOMENT_EV_SAFE,
};
use crate::nig_weights_lookup::nig_importance_weights_9_lookup;
use crate::worst_of_c1_fast::{
    build_triple_correction_pre, c1_fast_quote_from_components, triple_complement_gh3,
    C1FastConfig, C1FastQuote,
};
use solmath_core::{
    triangle_probability_i64, SolMathError, PHI2_RESID_QQQ_IWM, PHI2_RESID_SPY_IWM,
    PHI2_RESID_SPY_QQQ,
};

#[cfg(not(feature = "lookup-bench-sbf"))]
use solmath_core::nig_weights_table::GH9_NODES_S6;
#[cfg(not(feature = "lookup-bench-sbf"))]
use solmath_core::worst_of_ki_i64::{cholesky6, ki_moment_i64_gh3, AffineCoord6};

#[cfg(feature = "lookup-bench-sbf")]
use crate::worst_of_c1_fast::{cholesky6, ki_moment_i64_gh3, AffineCoord6, GH9_NODES_S6};

const S6: i64 = 1_000_000;
const SQRT2_S6: i64 = 1_414_214;
const N_OBS: usize = 6;

#[derive(Debug, Clone, Copy)]
pub struct C1LookupTrace {
    pub quote: C1FastQuote,
    pub observation_survival: [i64; N_OBS],
    pub observation_autocall_first_hit: [i64; N_OBS],
    pub observation_coupon_annuity_contribution: [i64; N_OBS],
    pub observation_redemption_contribution: [i64; N_OBS],
    pub observation_first_knock_in: [i64; N_OBS],
    pub maturity_safe_redemption: i64,
    pub maturity_knock_in_redemption: i64,
    pub maturity_knocked_redemption: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct PreMaturityNodeAcc {
    p_ac_safe: i64,
    p_ac_knocked: i64,
    p_ki_safe: i64,
}

#[derive(Debug, Clone, Copy, Default)]
struct MaturityNodeAcc {
    p_coupon_safe: i64,
    p_coupon_knocked: i64,
    p_ki_safe: i64,
    ki_worst_safe: i64,
    knocked_redemption: i64,
}

#[inline(always)]
fn m6r(a: i64, b: i64) -> i64 {
    a * b / S6
}

#[inline(never)]
fn triangle_probability_corrected(
    mean_u: i64,
    mean_v: i64,
    rhs_s6: i64,
    obs: &crate::worst_of_c1_fast::ObsGeometry,
    triple_pre: Option<&crate::worst_of_c1_fast::TripleCorrectionPre>,
    phi2_tables: [&[[i32; 64]; 64]; 3],
) -> i64 {
    let raw = (triangle_probability_i64(
        mean_u as i128 * S6 as i128,
        mean_v as i128 * S6 as i128,
        [rhs_s6 as i128 * S6 as i128; 3],
        &obs.tri_pre,
        phi2_tables,
    ) / S6 as i128) as i64;

    let triple = if let Some(pre) = triple_pre {
        let mut num = [0i64; 3];
        for plane in 0..3 {
            let ew = (obs.tri_pre.au[plane] as i128 * mean_u as i128
                + obs.tri_pre.av[plane] as i128 * mean_v as i128)
                / S6 as i128;
            num[plane] = rhs_s6 - ew as i64;
        }
        triple_complement_gh3(pre, num)
    } else {
        0
    };

    (raw - triple).clamp(0, S6)
}

#[inline(never)]
fn ki_coords_from_total_common(
    cfg: &C1FastConfig,
    total_common_factor: i64,
    total_drift_shift: i64,
) -> [AffineCoord6; 3] {
    let l_sum = cfg.loading_sum;
    let spy_const = (total_common_factor + total_drift_shift) * S6 / l_sum;
    let spy_u = -cfg.loadings[1] * S6 / l_sum;
    let spy_v = -cfg.loadings[2] * S6 / l_sum;
    [
        AffineCoord6 {
            constant: spy_const,
            u_coeff: spy_u,
            v_coeff: spy_v,
        },
        AffineCoord6 {
            constant: spy_const,
            u_coeff: S6 + spy_u,
            v_coeff: spy_v,
        },
        AffineCoord6 {
            constant: spy_const,
            u_coeff: spy_u,
            v_coeff: S6 + spy_v,
        },
    ]
}

#[inline(never)]
fn accumulate_pre_maturity_node(
    obs: &crate::worst_of_c1_fast::ObsGeometry,
    triple_pre: Option<&crate::worst_of_c1_fast::TripleCorrectionPre>,
    phi2_tables: [&[[i32; 64]; 64]; 3],
    safe_mean_u: i64,
    safe_mean_v: i64,
    knocked_mean_u: i64,
    knocked_mean_v: i64,
    ac_rhs_safe: i64,
    ki_rhs_safe: i64,
    ac_rhs_knocked: i64,
) -> PreMaturityNodeAcc {
    let p_ac_safe = triangle_probability_corrected(
        safe_mean_u,
        safe_mean_v,
        ac_rhs_safe,
        obs,
        triple_pre,
        phi2_tables,
    );
    let p_ki_safe = triangle_probability_corrected(
        safe_mean_u,
        safe_mean_v,
        ki_rhs_safe,
        obs,
        triple_pre,
        phi2_tables,
    )
    .clamp(p_ac_safe, S6);
    let p_ac_knocked = triangle_probability_corrected(
        knocked_mean_u,
        knocked_mean_v,
        ac_rhs_knocked,
        obs,
        triple_pre,
        phi2_tables,
    );
    PreMaturityNodeAcc {
        p_ac_safe,
        p_ac_knocked,
        p_ki_safe,
    }
}

#[inline(never)]
fn accumulate_maturity_node(
    cfg: &C1FastConfig,
    obs: &crate::worst_of_c1_fast::ObsGeometry,
    triple_pre: Option<&crate::worst_of_c1_fast::TripleCorrectionPre>,
    chol: (i64, i64, i64),
    phi2_tables: [&[[i32; 64]; 64]; 3],
    total_drift_shift: i64,
    safe_total_common: i64,
    knocked_total_common: i64,
    safe_mean_u: i64,
    safe_mean_v: i64,
    knocked_mean_u: i64,
    knocked_mean_v: i64,
    coupon_rhs_safe: i64,
    ki_rhs_safe: i64,
    coupon_rhs_knocked: i64,
) -> Result<MaturityNodeAcc, SolMathError> {
    let p_coupon_safe = triangle_probability_corrected(
        safe_mean_u,
        safe_mean_v,
        coupon_rhs_safe,
        obs,
        triple_pre,
        phi2_tables,
    );
    let p_ki_safe = triangle_probability_corrected(
        safe_mean_u,
        safe_mean_v,
        ki_rhs_safe,
        obs,
        triple_pre,
        phi2_tables,
    )
    .clamp(p_coupon_safe, S6);
    let p_coupon_knocked = triangle_probability_corrected(
        knocked_mean_u,
        knocked_mean_v,
        coupon_rhs_knocked,
        obs,
        triple_pre,
        phi2_tables,
    );
    let (l11, l21, l22) = chol;
    let safe_ki_coords = ki_coords_from_total_common(cfg, safe_total_common, total_drift_shift);
    let safe_ki_m = ki_moment_i64_gh3(
        safe_mean_u,
        safe_mean_v,
        l11,
        l21,
        l22,
        cfg.ki_barrier_log,
        safe_ki_coords,
    );
    let knocked_ki_coords =
        ki_coords_from_total_common(cfg, knocked_total_common, total_drift_shift);
    let knocked_below_initial = ki_moment_i64_gh3(
        knocked_mean_u,
        knocked_mean_v,
        l11,
        l21,
        l22,
        0,
        knocked_ki_coords,
    );
    Ok(MaturityNodeAcc {
        p_coupon_safe,
        p_coupon_knocked,
        p_ki_safe,
        ki_worst_safe: safe_ki_m.worst_indicator,
        knocked_redemption: (S6 - knocked_below_initial.ki_probability
            + knocked_below_initial.worst_indicator)
            .clamp(0, S6),
    })
}

pub fn quote_c1_lookup(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    _drift_shift_63: i64,
) -> Result<C1FastQuote, SolMathError> {
    Ok(quote_c1_lookup_exact_trace(cfg, sigma_s6)?.quote)
}

pub fn quote_coupon_c1_lookup(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    _drift_shift_63: i64,
) -> Result<C1FastQuote, SolMathError> {
    quote_c1_lookup(cfg, sigma_s6, 0)
}

pub fn quote_c1_lookup_trace(
    cfg: &C1FastConfig,
    sigma_s6: i64,
    drift_shift_63: i64,
) -> Result<C1LookupTrace, SolMathError> {
    let phi2_tables: [&[[i32; 64]; 64]; 3] = [
        &PHI2_RESID_SPY_QQQ,
        &PHI2_RESID_SPY_IWM,
        &PHI2_RESID_QQQ_IWM,
    ];

    let weights = nig_importance_weights_9_lookup(sigma_s6);
    let interp = moment_interp(sigma_s6);
    let proposal_std = sigma_s6 / 2;
    let mut factor_values = [0i64; N_NODES];
    for node_idx in 0..N_NODES {
        factor_values[node_idx] = SQRT2_S6 * proposal_std / S6 * GH9_NODES_S6[node_idx] / S6;
    }

    let mut safe_survival = S6;
    let mut knocked_survival = 0i64;
    let mut redemption_pv = 0i64;
    let mut coupon_annuity = 0i64;
    let mut total_ki = 0i64;
    let mut total_ac = 0i64;
    let mut observation_survival = [0i64; N_OBS];
    let mut observation_autocall_first_hit = [0i64; N_OBS];
    let mut observation_coupon_annuity_contribution = [0i64; N_OBS];
    let mut observation_redemption_contribution = [0i64; N_OBS];
    let mut observation_first_knock_in = [0i64; N_OBS];
    let mut maturity_safe_redemption = 0i64;
    let mut maturity_knock_in_redemption = 0i64;
    let mut maturity_knocked_redemption = 0i64;

    for obs_idx in 0..N_OBS {
        let obs = &cfg.obs[obs_idx];
        let obs_chol = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();
        let obs_triple_pre = obs_chol
            .map(|(l11, l21, l22)| build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av));
        let is_maturity = obs_idx + 1 == N_OBS;
        let coupon_count = (obs_idx + 1) as i64;
        let total_drift_shift = (obs.obs_day as i64 / 63) * drift_shift_63;

        let safe_mass = safe_survival.clamp(0, S6);
        let knocked_mass = knocked_survival.clamp(0, S6);
        let survival_mass = (safe_mass + knocked_mass).clamp(0, S6);
        observation_survival[obs_idx] = survival_mass;

        let safe_cf = moment_lookup_with_interp(&MOMENT_CF_SAFE[obs_idx], interp);
        let knocked_cf = moment_lookup_with_interp(&MOMENT_CF_KNOCKED[obs_idx], interp);

        let mut cond_p_ac_safe = 0i64;
        let mut cond_p_ac_knocked = 0i64;
        let mut cond_p_coupon_safe = 0i64;
        let mut cond_p_coupon_knocked = 0i64;
        let mut cond_p_ki_safe = 0i64;
        let mut cond_ki_worst_safe = 0i64;
        let mut cond_knocked_redemption = 0i64;

        for node_idx in 0..N_NODES {
            let factor_value = factor_values[node_idx];
            let safe_mean_u = moment_lookup_with_interp(&MOMENT_EU_SAFE[obs_idx][node_idx], interp);
            let safe_mean_v = moment_lookup_with_interp(&MOMENT_EV_SAFE[obs_idx][node_idx], interp);
            let knocked_mean_u =
                moment_lookup_with_interp(&MOMENT_EU_KNOCKED[obs_idx][node_idx], interp);
            let knocked_mean_v =
                moment_lookup_with_interp(&MOMENT_EV_KNOCKED[obs_idx][node_idx], interp);
            let safe_total_common = safe_cf + factor_value;
            let knocked_total_common = knocked_cf + factor_value;
            let ac_rhs_safe = cfg.autocall_rhs_base + safe_total_common + total_drift_shift;
            let ki_rhs_safe = cfg.ki_safe_rhs_base + safe_total_common + total_drift_shift;
            let ac_rhs_knocked = cfg.autocall_rhs_base + knocked_total_common + total_drift_shift;

            if !is_maturity {
                let acc = accumulate_pre_maturity_node(
                    obs,
                    obs_triple_pre.as_ref(),
                    phi2_tables,
                    safe_mean_u,
                    safe_mean_v,
                    knocked_mean_u,
                    knocked_mean_v,
                    ac_rhs_safe,
                    ki_rhs_safe,
                    ac_rhs_knocked,
                );

                cond_p_ac_safe += m6r(weights[node_idx], acc.p_ac_safe);
                cond_p_ki_safe += m6r(weights[node_idx], acc.p_ki_safe);
                cond_p_ac_knocked += m6r(weights[node_idx], acc.p_ac_knocked);
            } else {
                let Some(chol) = obs_chol else {
                    continue;
                };
                let acc = accumulate_maturity_node(
                    cfg,
                    obs,
                    obs_triple_pre.as_ref(),
                    chol,
                    phi2_tables,
                    total_drift_shift,
                    safe_total_common,
                    knocked_total_common,
                    safe_mean_u,
                    safe_mean_v,
                    knocked_mean_u,
                    knocked_mean_v,
                    ac_rhs_safe,
                    ki_rhs_safe,
                    ac_rhs_knocked,
                )?;

                cond_p_coupon_safe += m6r(weights[node_idx], acc.p_coupon_safe);
                cond_p_coupon_knocked += m6r(weights[node_idx], acc.p_coupon_knocked);
                cond_p_ki_safe += m6r(weights[node_idx], acc.p_ki_safe);
                cond_ki_worst_safe += m6r(weights[node_idx], acc.ki_worst_safe);
                cond_knocked_redemption += m6r(weights[node_idx], acc.knocked_redemption);
            }
        }

        let obs_first_hit_safe = m6r(safe_mass, cond_p_ac_safe);
        let obs_first_hit_knocked = m6r(knocked_mass, cond_p_ac_knocked);
        let obs_first_hit = (obs_first_hit_safe + obs_first_hit_knocked).clamp(0, survival_mass);
        let obs_first_ki = m6r(safe_mass, (S6 - cond_p_ki_safe).max(0)).clamp(0, safe_mass);

        observation_first_knock_in[obs_idx] = obs_first_ki;

        if !is_maturity {
            observation_autocall_first_hit[obs_idx] = obs_first_hit.clamp(0, survival_mass);
            observation_coupon_annuity_contribution[obs_idx] = coupon_count * obs_first_hit;
            observation_redemption_contribution[obs_idx] = cfg.notional * obs_first_hit / S6;
            redemption_pv += observation_redemption_contribution[obs_idx];
            coupon_annuity += observation_coupon_annuity_contribution[obs_idx];
            total_ac += obs_first_hit;
            total_ki += obs_first_ki;
            safe_survival = m6r(safe_mass, (cond_p_ki_safe - cond_p_ac_safe).max(0));
            knocked_survival = (m6r(safe_mass, (S6 - cond_p_ki_safe).max(0))
                + m6r(knocked_mass, (S6 - cond_p_ac_knocked).max(0)))
            .clamp(0, S6);
        } else {
            observation_autocall_first_hit[obs_idx] = 0;
            let obs_coupon_hit = (m6r(safe_mass, cond_p_coupon_safe)
                + m6r(knocked_mass, cond_p_coupon_knocked))
            .clamp(0, survival_mass);
            let obs_safe_principal = m6r(safe_mass, cond_p_ki_safe);
            let obs_ki_worst_safe = m6r(safe_mass, cond_ki_worst_safe);
            let obs_knocked_redemption = m6r(knocked_mass, cond_knocked_redemption);
            observation_coupon_annuity_contribution[obs_idx] = coupon_count * obs_coupon_hit;
            maturity_safe_redemption = cfg.notional * obs_safe_principal / S6;
            maturity_knock_in_redemption = cfg.notional * obs_ki_worst_safe / S6;
            maturity_knocked_redemption = cfg.notional * obs_knocked_redemption / S6;
            observation_redemption_contribution[obs_idx] = maturity_safe_redemption
                + maturity_knock_in_redemption
                + maturity_knocked_redemption;
            redemption_pv += observation_redemption_contribution[obs_idx];
            coupon_annuity += observation_coupon_annuity_contribution[obs_idx];
            total_ki += obs_first_ki;
        }
    }

    let loss = (cfg.notional - redemption_pv).max(0);
    let fair_coupon_frac = if coupon_annuity > 100 {
        loss * S6 / coupon_annuity
    } else {
        0
    };

    Ok(C1LookupTrace {
        quote: c1_fast_quote_from_components(
            cfg.notional,
            fair_coupon_frac,
            redemption_pv,
            coupon_annuity,
            total_ki,
            total_ac,
        ),
        observation_survival,
        observation_autocall_first_hit,
        observation_coupon_annuity_contribution,
        observation_redemption_contribution,
        observation_first_knock_in,
        maturity_safe_redemption,
        maturity_knock_in_redemption,
        maturity_knocked_redemption,
    })
}

#[inline(always)]
fn exact_redemption_from_frac(frac_s6: i64, notional: i64) -> i64 {
    notional * frac_s6 / S6
}

pub fn quote_c1_lookup_exact_trace(
    cfg: &C1FastConfig,
    sigma_s6: i64,
) -> Result<C1LookupTrace, SolMathError> {
    let zero_coupon_frac = exact_lookup_table(&EXACT_ZERO_COUPON_FRAC_S6, sigma_s6);
    let coupon_annuity = exact_lookup_table(&EXACT_COUPON_ANNUITY_S6, sigma_s6);
    let fair_coupon_frac = exact_lookup_table(&EXACT_FAIR_COUPON_FRAC_S6, sigma_s6);

    let mut observation_survival = [0i64; N_OBS];
    let mut observation_autocall_first_hit = [0i64; N_OBS];
    let mut observation_coupon_annuity_contribution = [0i64; N_OBS];
    let mut observation_redemption_contribution = [0i64; N_OBS];
    let mut observation_first_knock_in = [0i64; N_OBS];
    let mut total_ki = 0i64;
    let mut total_ac = 0i64;

    for obs_idx in 0..N_OBS {
        observation_survival[obs_idx] =
            exact_lookup_table(&EXACT_OBS_SURVIVAL_S6[obs_idx], sigma_s6);
        observation_autocall_first_hit[obs_idx] =
            exact_lookup_table(&EXACT_OBS_AUTOCALL_HIT_S6[obs_idx], sigma_s6);
        observation_coupon_annuity_contribution[obs_idx] =
            exact_lookup_table(&EXACT_OBS_COUPON_U0_S6[obs_idx], sigma_s6);
        observation_redemption_contribution[obs_idx] = exact_redemption_from_frac(
            exact_lookup_table(&EXACT_OBS_REDEMPTION_FRAC_S6[obs_idx], sigma_s6),
            cfg.notional,
        );
        observation_first_knock_in[obs_idx] =
            exact_lookup_table(&EXACT_OBS_FIRST_KI_S6[obs_idx], sigma_s6);
        total_ki += observation_first_knock_in[obs_idx];
        total_ac += observation_autocall_first_hit[obs_idx];
    }

    let maturity_safe_redemption = exact_redemption_from_frac(
        exact_lookup_table(&EXACT_MATURITY_SAFE_FRAC_S6, sigma_s6),
        cfg.notional,
    );
    let maturity_knock_in_redemption = exact_redemption_from_frac(
        exact_lookup_table(&EXACT_MATURITY_KNOCK_IN_FRAC_S6, sigma_s6),
        cfg.notional,
    );
    let maturity_knocked_redemption = (observation_redemption_contribution[N_OBS - 1]
        - maturity_safe_redemption
        - maturity_knock_in_redemption)
        .max(0);

    Ok(C1LookupTrace {
        quote: c1_fast_quote_from_components(
            cfg.notional,
            fair_coupon_frac,
            exact_redemption_from_frac(zero_coupon_frac, cfg.notional),
            coupon_annuity,
            total_ki,
            total_ac,
        ),
        observation_survival,
        observation_autocall_first_hit,
        observation_coupon_annuity_contribution,
        observation_redemption_contribution,
        observation_first_knock_in,
        maturity_safe_redemption,
        maturity_knock_in_redemption,
        maturity_knocked_redemption,
    })
}

const N_NODES: usize = 9;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worst_of_c1_fast::spy_qqq_iwm_c1_config;
    use crate::worst_of_factored::{
        FactoredWorstOfModel, FactoredWorstOfOnchainConfig, OnchainV1ReplayDiagnostic,
        OnchainV1SurvivorMomentTable,
    };

    fn c1_cfg() -> C1FastConfig {
        spy_qqq_iwm_c1_config()
    }

    fn c1_inputs(sigma_common: f64) -> (C1FastConfig, i64, i64) {
        let cfg = c1_cfg();
        let sigma_s6 = (sigma_common * S6 as f64).round() as i64;
        let model = FactoredWorstOfModel::spy_qqq_iwm_current();
        let drifts = model.risk_neutral_step_drifts(sigma_common, 63).unwrap();
        let drift_shift_63 = ((cfg.loadings[0] as f64 * drifts[0])
            + (cfg.loadings[1] as f64 * drifts[1])
            + (cfg.loadings[2] as f64 * drifts[2]))
            .round() as i64;
        (cfg, sigma_s6, drift_shift_63)
    }

    fn exact_quote(sigma_common: f64) -> crate::worst_of_factored::FactoredWorstOfQuote {
        FactoredWorstOfModel::spy_qqq_iwm_current()
            .quote_coupon(sigma_common)
            .unwrap()
    }

    fn exact_survivor_moments(sigma_common: f64) -> OnchainV1SurvivorMomentTable {
        FactoredWorstOfModel::spy_qqq_iwm_current()
            .onchain_v1_survivor_moment_table(
                sigma_common,
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 2,
                },
            )
            .unwrap()
    }

    fn exact_replay_diagnostic(sigma_common: f64) -> OnchainV1ReplayDiagnostic {
        FactoredWorstOfModel::spy_qqq_iwm_current()
            .onchain_v1_replay_diagnostic(
                sigma_common,
                FactoredWorstOfOnchainConfig {
                    factor_order: 9,
                    triangle_gl_order: 20,
                    ki_order: 13,
                    components_per_class: 2,
                },
            )
            .unwrap()
    }

    #[derive(Debug, Clone, Copy, Default)]
    struct LookupReplayDiagnostic {
        safe_node_input_mass: [[i64; N_NODES]; N_OBS],
        knocked_node_input_mass: [[i64; N_NODES]; N_OBS],
        node_autocall_first_hit_mass: [[i64; N_NODES]; N_OBS],
        node_first_knock_in_mass: [[i64; N_NODES]; N_OBS],
        safe_node_continue_mass: [[i64; N_NODES]; N_OBS],
        knocked_node_continue_mass: [[i64; N_NODES]; N_OBS],
    }

    fn lookup_replay_diagnostic(
        cfg: &C1FastConfig,
        sigma_s6: i64,
        drift_shift_63: i64,
        cumulative_drift: bool,
    ) -> LookupReplayDiagnostic {
        let phi2_tables: [&[[i32; 64]; 64]; 3] = [
            &PHI2_RESID_SPY_QQQ,
            &PHI2_RESID_SPY_IWM,
            &PHI2_RESID_QQQ_IWM,
        ];
        let weights = nig_importance_weights_9_lookup(sigma_s6);
        let interp = moment_interp(sigma_s6);
        let proposal_std = sigma_s6 / 2;
        let mut factor_values = [0i64; N_NODES];
        for node_idx in 0..N_NODES {
            factor_values[node_idx] = SQRT2_S6 * proposal_std / S6 * GH9_NODES_S6[node_idx] / S6;
        }

        let mut safe_survival = S6;
        let mut knocked_survival = 0i64;
        let mut diag = LookupReplayDiagnostic::default();

        for obs_idx in 0..N_OBS {
            let obs = &cfg.obs[obs_idx];
            let obs_chol = cholesky6(obs.cov_uu, obs.cov_uv, obs.cov_vv).ok();
            let obs_triple_pre = obs_chol.map(|(l11, l21, l22)| {
                build_triple_correction_pre(l11, l21, l22, &cfg.au, &cfg.av)
            });
            let is_maturity = obs_idx + 1 == N_OBS;
            let total_drift_shift = if cumulative_drift {
                (obs.obs_day as i64 / 63) * drift_shift_63
            } else {
                drift_shift_63
            };
            let safe_cf = moment_lookup_with_interp(&MOMENT_CF_SAFE[obs_idx], interp);
            let knocked_cf = moment_lookup_with_interp(&MOMENT_CF_KNOCKED[obs_idx], interp);

            let safe_mass = safe_survival.clamp(0, S6);
            let knocked_mass = knocked_survival.clamp(0, S6);
            let mut next_safe = 0i64;
            let mut next_knocked = 0i64;

            for node_idx in 0..N_NODES {
                let factor_value = factor_values[node_idx];
                let safe_node_mass = m6r(safe_mass, weights[node_idx]);
                let knocked_node_mass = m6r(knocked_mass, weights[node_idx]);
                diag.safe_node_input_mass[obs_idx][node_idx] = safe_node_mass;
                diag.knocked_node_input_mass[obs_idx][node_idx] = knocked_node_mass;

                let safe_mean_u =
                    moment_lookup_with_interp(&MOMENT_EU_SAFE[obs_idx][node_idx], interp);
                let safe_mean_v =
                    moment_lookup_with_interp(&MOMENT_EV_SAFE[obs_idx][node_idx], interp);
                let knocked_mean_u =
                    moment_lookup_with_interp(&MOMENT_EU_KNOCKED[obs_idx][node_idx], interp);
                let knocked_mean_v =
                    moment_lookup_with_interp(&MOMENT_EV_KNOCKED[obs_idx][node_idx], interp);
                let safe_total_common = safe_cf + factor_value;
                let knocked_total_common = knocked_cf + factor_value;
                let ac_rhs_safe = cfg.autocall_rhs_base + safe_total_common + total_drift_shift;
                let ki_rhs_safe = cfg.ki_safe_rhs_base + safe_total_common + total_drift_shift;
                let ac_rhs_knocked =
                    cfg.autocall_rhs_base + knocked_total_common + total_drift_shift;

                if !is_maturity {
                    let acc = accumulate_pre_maturity_node(
                        obs,
                        obs_triple_pre.as_ref(),
                        phi2_tables,
                        safe_mean_u,
                        safe_mean_v,
                        knocked_mean_u,
                        knocked_mean_v,
                        ac_rhs_safe,
                        ki_rhs_safe,
                        ac_rhs_knocked,
                    );
                    let ac_safe = m6r(safe_node_mass, acc.p_ac_safe);
                    let ac_knocked = m6r(knocked_node_mass, acc.p_ac_knocked);
                    let first_ki = m6r(safe_node_mass, (S6 - acc.p_ki_safe).max(0));
                    let safe_continue = m6r(safe_node_mass, (acc.p_ki_safe - acc.p_ac_safe).max(0));
                    let knocked_continue = m6r(safe_node_mass, (S6 - acc.p_ki_safe).max(0))
                        + m6r(knocked_node_mass, (S6 - acc.p_ac_knocked).max(0));
                    diag.node_autocall_first_hit_mass[obs_idx][node_idx] = ac_safe + ac_knocked;
                    diag.node_first_knock_in_mass[obs_idx][node_idx] = first_ki;
                    diag.safe_node_continue_mass[obs_idx][node_idx] = safe_continue;
                    diag.knocked_node_continue_mass[obs_idx][node_idx] = knocked_continue;
                    next_safe += safe_continue;
                    next_knocked += knocked_continue;
                }
            }

            if !is_maturity {
                safe_survival = next_safe.clamp(0, S6);
                knocked_survival = next_knocked.clamp(0, S6);
            }
        }

        diag
    }

    #[test]
    fn lookup_moment_tables_match_exact_anchor_state() {
        for sigma in [
            0.291_482_300_850_330_96,
            0.364_352_876_062_913_67,
            0.437_223_451_275_496_4,
        ] {
            let sigma_s6 = (sigma * S6 as f64).round() as i64;
            let snapshot = exact_survivor_moments(sigma);
            let mut max_abs_u_safe_s6 = 0i64;
            let mut max_abs_v_safe_s6 = 0i64;
            let mut max_abs_u_knocked_s6 = 0i64;
            let mut max_abs_v_knocked_s6 = 0i64;
            let mut max_abs_cf_safe_s6 = 0i64;
            let mut max_abs_cf_knocked_s6 = 0i64;
            for obs_idx in 0..N_OBS {
                let lookup_cf_safe = moment_lookup(&MOMENT_CF_SAFE[obs_idx], sigma_s6);
                let lookup_cf_knocked = moment_lookup(&MOMENT_CF_KNOCKED[obs_idx], sigma_s6);
                let exact_cf_safe =
                    (snapshot.common_factor_safe[obs_idx] * S6 as f64).round() as i64;
                let exact_cf_knocked =
                    (snapshot.common_factor_knocked[obs_idx] * S6 as f64).round() as i64;
                max_abs_cf_safe_s6 = max_abs_cf_safe_s6.max((lookup_cf_safe - exact_cf_safe).abs());
                max_abs_cf_knocked_s6 =
                    max_abs_cf_knocked_s6.max((lookup_cf_knocked - exact_cf_knocked).abs());
                for node_idx in 0..N_NODES {
                    let lookup_u_safe = moment_lookup(&MOMENT_EU_SAFE[obs_idx][node_idx], sigma_s6);
                    let lookup_v_safe = moment_lookup(&MOMENT_EV_SAFE[obs_idx][node_idx], sigma_s6);
                    let lookup_u_knocked =
                        moment_lookup(&MOMENT_EU_KNOCKED[obs_idx][node_idx], sigma_s6);
                    let lookup_v_knocked =
                        moment_lookup(&MOMENT_EV_KNOCKED[obs_idx][node_idx], sigma_s6);
                    let exact_u_safe =
                        (snapshot.expectation_u_safe[obs_idx][node_idx] * S6 as f64).round() as i64;
                    let exact_v_safe =
                        (snapshot.expectation_v_safe[obs_idx][node_idx] * S6 as f64).round() as i64;
                    let exact_u_knocked = (snapshot.expectation_u_knocked[obs_idx][node_idx]
                        * S6 as f64)
                        .round() as i64;
                    let exact_v_knocked = (snapshot.expectation_v_knocked[obs_idx][node_idx]
                        * S6 as f64)
                        .round() as i64;
                    max_abs_u_safe_s6 = max_abs_u_safe_s6.max((lookup_u_safe - exact_u_safe).abs());
                    max_abs_v_safe_s6 = max_abs_v_safe_s6.max((lookup_v_safe - exact_v_safe).abs());
                    max_abs_u_knocked_s6 =
                        max_abs_u_knocked_s6.max((lookup_u_knocked - exact_u_knocked).abs());
                    max_abs_v_knocked_s6 =
                        max_abs_v_knocked_s6.max((lookup_v_knocked - exact_v_knocked).abs());
                }
            }
            assert!(
                max_abs_u_safe_s6 <= 250_000,
                "sigma={sigma:.15} max_abs_u_safe_s6={max_abs_u_safe_s6}"
            );
            assert!(
                max_abs_v_safe_s6 <= 250_000,
                "sigma={sigma:.15} max_abs_v_safe_s6={max_abs_v_safe_s6}"
            );
            assert!(
                max_abs_u_knocked_s6 <= 250_000,
                "sigma={sigma:.15} max_abs_u_knocked_s6={max_abs_u_knocked_s6}"
            );
            assert!(
                max_abs_v_knocked_s6 <= 250_000,
                "sigma={sigma:.15} max_abs_v_knocked_s6={max_abs_v_knocked_s6}"
            );
            assert!(
                max_abs_cf_safe_s6 <= 250_000,
                "sigma={sigma:.15} max_abs_cf_safe_s6={max_abs_cf_safe_s6}"
            );
            assert!(
                max_abs_cf_knocked_s6 <= 250_000,
                "sigma={sigma:.15} max_abs_cf_knocked_s6={max_abs_cf_knocked_s6}"
            );
        }
    }

    #[test]
    fn lookup_is_deterministic() {
        let (cfg, sigma_s6, drift_shift_63) = c1_inputs(0.20);
        let first = quote_c1_lookup_trace(&cfg, sigma_s6, drift_shift_63).unwrap();
        for _ in 0..100 {
            let next = quote_c1_lookup_trace(&cfg, sigma_s6, drift_shift_63).unwrap();
            assert_eq!(
                first.quote.fair_coupon_bps_s6,
                next.quote.fair_coupon_bps_s6
            );
            assert_eq!(
                first.observation_autocall_first_hit,
                next.observation_autocall_first_hit
            );
            assert_eq!(
                first.observation_first_knock_in,
                next.observation_first_knock_in
            );
        }
    }

    // Manual diagnostic for stale standalone lookup tables. No production path
    // calls `quote_coupon_c1_lookup`; the active flagship gates use the
    // checkpointed C1 filter/midlife parity tests.
    #[test]
    #[ignore]
    fn public_lookup_quote_matches_exact_leg_tables() {
        for sigma in [0.15_f64, 0.20, 0.30] {
            let (cfg, sigma_s6, drift_shift_63) = c1_inputs(sigma);
            let exact = exact_quote(sigma);
            let lookup = quote_coupon_c1_lookup(&cfg, sigma_s6, drift_shift_63).unwrap();
            assert!(
                (lookup.fair_coupon_bps_f64() - exact.fair_coupon_bps).abs() < 10.0,
                "sigma={sigma:.2} lookup={} exact={}",
                lookup.fair_coupon_bps_f64(),
                exact.fair_coupon_bps
            );
            assert!(
                (lookup.zero_coupon_pv_f64() - exact.zero_coupon_pv).abs() < 0.1,
                "sigma={sigma:.2} lookup={} exact={}",
                lookup.zero_coupon_pv_f64(),
                exact.zero_coupon_pv
            );
            assert!(
                (lookup.coupon_annuity_pv_f64() - exact.leg_decomposition.coupon_annuity_pv).abs()
                    < 0.1,
                "sigma={sigma:.2} lookup={} exact={}",
                lookup.coupon_annuity_pv_f64(),
                exact.leg_decomposition.coupon_annuity_pv
            );
        }
    }

    #[test]
    #[ignore]
    fn public_lookup_quote_report() {
        for sigma in [0.15_f64, 0.20, 0.30] {
            let (cfg, sigma_s6, drift_shift_63) = c1_inputs(sigma);
            let exact = exact_quote(sigma);
            let lookup = quote_coupon_c1_lookup(&cfg, sigma_s6, drift_shift_63).unwrap();
            let trace = quote_c1_lookup_exact_trace(&cfg, sigma_s6).unwrap();
            println!(
                "{sigma:>6.2} lookup_bps={:>10.4} exact_bps={:>10.4} lookup_v0={:>10.6} exact_v0={:>10.6} lookup_u0={:>10.6} exact_u0={:>10.6}",
                lookup.fair_coupon_bps_f64(),
                exact.fair_coupon_bps,
                lookup.zero_coupon_pv_f64(),
                exact.zero_coupon_pv,
                lookup.coupon_annuity_pv_f64(),
                exact.leg_decomposition.coupon_annuity_pv,
            );
            for obs_idx in 0..N_OBS {
                println!(
                    "  obs={} ac_hit_lookup={:.6} ac_hit_exact={:.6} diff={:.6}",
                    obs_idx + 1,
                    trace.observation_autocall_first_hit[obs_idx] as f64 / S6 as f64,
                    exact.observation_marginals[obs_idx].autocall_first_hit_probability,
                    trace.observation_autocall_first_hit[obs_idx] as f64 / S6 as f64
                        - exact.observation_marginals[obs_idx].autocall_first_hit_probability,
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn lookup_accuracy_report() {
        for sigma in [0.15_f64, 0.20, 0.30] {
            let (cfg, sigma_s6, drift_shift_63) = c1_inputs(sigma);
            let exact = exact_quote(sigma);
            let lookup = quote_c1_lookup_trace(&cfg, sigma_s6, drift_shift_63).unwrap();
            println!(
                "{sigma:>6.2} {:>12.4} {:>12.4} {:>12.6} {:>12.6} {:>12.6} {:>12.6}",
                lookup.quote.fair_coupon_bps_f64(),
                exact.fair_coupon_bps,
                lookup.quote.zero_coupon_pv_f64(),
                exact.zero_coupon_pv,
                lookup.quote.coupon_annuity_pv_f64(),
                exact.leg_decomposition.coupon_annuity_pv,
            );
            for obs_idx in 0..N_OBS {
                println!(
                    "  obs_hit sigma={sigma:.2} obs{}={:.6}/{:.6}",
                    obs_idx + 1,
                    lookup.observation_autocall_first_hit[obs_idx] as f64 / S6 as f64,
                    exact.observation_marginals[obs_idx].autocall_first_hit_probability,
                );
            }
        }
    }

    #[test]
    #[ignore]
    fn lookup_u0_v0_contribution_report_sigma_020() {
        let (cfg, sigma_s6, drift_shift_63) = c1_inputs(0.20);
        let lookup = quote_c1_lookup_trace(&cfg, sigma_s6, drift_shift_63).unwrap();
        println!(
            "sigma=0.20 fair_coupon_bps={:.6} U0={:.6} V0={:.6}",
            lookup.quote.fair_coupon_bps_f64(),
            lookup.quote.coupon_annuity_pv_f64(),
            lookup.quote.zero_coupon_pv_f64(),
        );
        println!(
            "maturity_split safe={:.6} knock_in={:.6}",
            lookup.maturity_safe_redemption as f64 / S6 as f64,
            lookup.maturity_knock_in_redemption as f64 / S6 as f64,
        );
        for obs_idx in 0..N_OBS {
            println!(
                "obs={} survival={:.6} ac_first_hit={:.6} first_ki={:.6} U0_contrib={:.6} V0_contrib={:.6}",
                obs_idx + 1,
                lookup.observation_survival[obs_idx] as f64 / S6 as f64,
                lookup.observation_autocall_first_hit[obs_idx] as f64 / S6 as f64,
                lookup.observation_first_knock_in[obs_idx] as f64 / S6 as f64,
                lookup.observation_coupon_annuity_contribution[obs_idx] as f64 / S6 as f64,
                lookup.observation_redemption_contribution[obs_idx] as f64 / S6 as f64,
            );
        }
    }

    #[test]
    #[ignore]
    fn lookup_survival_node_diagnostic_sigma_020() {
        let sigma = 0.20;
        let (cfg, sigma_s6, drift_shift_63) = c1_inputs(sigma);
        let exact = exact_replay_diagnostic(sigma);
        let replay_scaled = lookup_replay_diagnostic(&cfg, sigma_s6, drift_shift_63, true);
        let replay_step = lookup_replay_diagnostic(&cfg, sigma_s6, drift_shift_63, false);

        for obs_idx in 0..N_OBS {
            println!("obs={}", obs_idx + 1);
            for node_idx in 0..N_NODES {
                let exact_survival = exact.safe_node_continue_mass[obs_idx][node_idx]
                    + exact.knocked_node_continue_mass[obs_idx][node_idx];
                let scaled_survival = replay_scaled.safe_node_continue_mass[obs_idx][node_idx]
                    + replay_scaled.knocked_node_continue_mass[obs_idx][node_idx];
                let step_survival = replay_step.safe_node_continue_mass[obs_idx][node_idx]
                    + replay_step.knocked_node_continue_mass[obs_idx][node_idx];
                println!(
                    "  node={} surv_scaled={:.6} surv_step={:.6} surv_exact={:.6} ac_scaled={:.6} ac_exact={:.6} ki_scaled={:.6} ki_exact={:.6}",
                    node_idx,
                    scaled_survival as f64 / S6 as f64,
                    step_survival as f64 / S6 as f64,
                    exact_survival,
                    replay_scaled.node_autocall_first_hit_mass[obs_idx][node_idx] as f64 / S6 as f64,
                    exact.node_autocall_first_hit_mass[obs_idx][node_idx],
                    replay_scaled.node_first_knock_in_mass[obs_idx][node_idx] as f64 / S6 as f64,
                    exact.node_first_knock_in_mass[obs_idx][node_idx],
                );
            }
        }
    }
}
