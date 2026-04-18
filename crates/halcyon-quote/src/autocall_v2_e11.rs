use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use nalgebra::{DMatrix, DVector, SVD};
use solmath_core::SCALE_6;

use crate::autocall_v2::{
    build_markov_grid_info, build_transition_matrix_on_grid_info, solve_fair_coupon_e11,
    AutocallParams, AutocallPriceResult, AutocallV2Error, DeimFactors, DeimLegData, E11Factors,
    MarkovGridInfo, NigParams6,
};

pub const E11_LIVE_QUOTE_N_STATES: usize = 50;
pub const E11_LIVE_QUOTE_D: usize = 15;
pub const E11_LIVE_QUOTE_M: usize = 12;
pub const E11_LIVE_QUOTE_SIGMA_MIN: f64 = 0.50;
pub const E11_LIVE_QUOTE_SIGMA_MAX: f64 = 2.50;

struct E11QuoteContext {
    factors: E11Factors,
    deim_base: DeimFactors,
    grid_info: MarkovGridInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct E11QuoteCacheKey {
    alpha_6: i64,
    beta_6: i64,
    reference_step_days: i64,
    n_obs: usize,
    knock_in_log_6: i64,
    autocall_log_6: i64,
    n_states: usize,
    d: usize,
    m: usize,
}

static E11_QUOTE_CACHE: OnceLock<Mutex<HashMap<E11QuoteCacheKey, Arc<E11QuoteContext>>>> =
    OnceLock::new();

pub fn live_quote_uses_e11(sigma_ann: f64, contract: &AutocallParams) -> bool {
    sigma_ann.is_finite()
        && contract.n_obs == 8
        && (E11_LIVE_QUOTE_SIGMA_MIN..=E11_LIVE_QUOTE_SIGMA_MAX).contains(&sigma_ann)
}

pub fn solve_fair_coupon_e11_cached(
    sigma_ann_6: i64,
    alpha_6: i64,
    beta_6: i64,
    reference_step_days: i64,
    contract: &AutocallParams,
) -> Result<AutocallPriceResult, AutocallV2Error> {
    let key = E11QuoteCacheKey {
        alpha_6,
        beta_6,
        reference_step_days,
        n_obs: contract.n_obs,
        knock_in_log_6: contract.knock_in_log_6,
        autocall_log_6: contract.autocall_log_6,
        n_states: E11_LIVE_QUOTE_N_STATES,
        d: E11_LIVE_QUOTE_D,
        m: E11_LIVE_QUOTE_M,
    };
    let cache = E11_QUOTE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Some(context) = cache
        .lock()
        .expect("e11 quote cache poisoned")
        .get(&key)
        .cloned()
    {
        return solve_from_context(
            &context,
            sigma_ann_6,
            alpha_6,
            beta_6,
            reference_step_days,
            contract,
        );
    }

    let built = Arc::new(build_quote_context(
        alpha_6,
        beta_6,
        reference_step_days,
        contract,
        E11_LIVE_QUOTE_N_STATES,
        E11_LIVE_QUOTE_D,
        E11_LIVE_QUOTE_M,
    )?);
    let context = {
        let mut guard = cache.lock().expect("e11 quote cache poisoned");
        guard
            .entry(key)
            .or_insert_with(|| Arc::clone(&built))
            .clone()
    };
    solve_from_context(
        &context,
        sigma_ann_6,
        alpha_6,
        beta_6,
        reference_step_days,
        contract,
    )
}

fn solve_from_context(
    context: &E11QuoteContext,
    sigma_ann_6: i64,
    alpha_6: i64,
    beta_6: i64,
    reference_step_days: i64,
    contract: &AutocallParams,
) -> Result<AutocallPriceResult, AutocallV2Error> {
    let nig =
        NigParams6::from_vol_with_step_days(sigma_ann_6, alpha_6, beta_6, reference_step_days)?;
    solve_fair_coupon_e11(
        &context.factors,
        &nig,
        &context.grid_info,
        &context.deim_base,
        contract,
    )
}

fn build_quote_context(
    alpha_6: i64,
    beta_6: i64,
    reference_step_days: i64,
    contract: &AutocallParams,
    n_states: usize,
    d: usize,
    m: usize,
) -> Result<E11QuoteContext, AutocallV2Error> {
    let nig_max = NigParams6::from_vol_with_step_days(
        to_scale6_round(E11_LIVE_QUOTE_SIGMA_MAX),
        alpha_6,
        beta_6,
        reference_step_days,
    )?;
    let grid_info = build_markov_grid_info(n_states, &nig_max, contract)?;
    let s = grid_info.n_states;
    let atm = grid_info.atm_state;

    let train_sigmas = sigma_training_grid();
    let mut p_flat_snaps: Vec<Vec<f64>> = Vec::with_capacity(train_sigmas.len());
    let mut snapshots_v: Vec<Vec<f64>> = Vec::new();
    let mut snapshots_u: Vec<Vec<f64>> = Vec::new();

    for &sigma in &train_sigmas {
        let nig = NigParams6::from_vol_with_step_days(
            to_scale6_round(sigma),
            alpha_6,
            beta_6,
            reference_step_days,
        )?;
        let mat_i64 = build_transition_matrix_on_grid_info(&grid_info, &nig)?;

        let mut p_mat = DMatrix::<f64>::zeros(s, s);
        let mut p_flat = vec![0.0f64; s * s];
        for i in 0..s {
            for j in 0..s {
                let value = mat_i64[i][j] as f64 / SCALE_6 as f64;
                p_mat[(i, j)] = value;
                p_flat[i * s + j] = value;
            }
        }
        p_flat_snaps.push(p_flat);

        let reps_f64 = grid_info
            .reps
            .iter()
            .map(|&value| value as f64 / SCALE_6 as f64)
            .collect::<Vec<_>>();
        snapshots_v.extend(backward_pass_f64_snapshots(
            &p_mat, &reps_f64, &grid_info, 0.0, contract,
        ));
        snapshots_u.extend(backward_pass_f64_snapshots(
            &p_mat, &reps_f64, &grid_info, 1.0, contract,
        ));
    }

    let ref_idx = 10.min(p_flat_snaps.len().saturating_sub(1));
    let p_ref_flat = &p_flat_snaps[ref_idx];

    let n_train = p_flat_snaps.len();
    let mut s_mat = DMatrix::<f64>::zeros(s * s, n_train);
    for (col, snap) in p_flat_snaps.iter().enumerate() {
        for row in 0..(s * s) {
            s_mat[(row, col)] = snap[row] - p_ref_flat[row];
        }
    }
    let svd_op = SVD::new(s_mat, true, false);
    let u_op = svd_op.u.ok_or(AutocallV2Error::InvalidGrid)?;
    let m_eff = m.min(n_train).min(u_op.ncols());
    if m_eff == 0 {
        return Err(AutocallV2Error::InvalidGrid);
    }

    let mut u_modes = DMatrix::<f64>::zeros(s * s, m_eff);
    for i in 0..(s * s) {
        for k in 0..m_eff {
            u_modes[(i, k)] = u_op[(i, k)];
        }
    }
    let eim_flat_indices = deim_select_f64(&u_modes, m_eff);
    let eim_rows: Vec<usize> = eim_flat_indices.iter().map(|&idx| idx / s).collect();
    let eim_cols: Vec<usize> = eim_flat_indices.iter().map(|&idx| idx % s).collect();

    let mut b_mat = DMatrix::<f64>::zeros(m_eff, m_eff);
    for i in 0..m_eff {
        for k in 0..m_eff {
            b_mat[(i, k)] = u_modes[(eim_flat_indices[i], k)];
        }
    }
    let b_inv_f64 = b_mat.try_inverse().ok_or(AutocallV2Error::InvalidGrid)?;

    let n_snaps_v = snapshots_v.len();
    let n_snaps_u = snapshots_u.len();
    let mut sv_mat = DMatrix::<f64>::zeros(s, n_snaps_v);
    let mut su_mat = DMatrix::<f64>::zeros(s, n_snaps_u);
    for (col, snap) in snapshots_v.iter().enumerate() {
        for row in 0..s {
            sv_mat[(row, col)] = snap[row];
        }
    }
    for (col, snap) in snapshots_u.iter().enumerate() {
        for row in 0..s {
            su_mat[(row, col)] = snap[row];
        }
    }
    let svd_v = SVD::new(sv_mat, true, false);
    let svd_u = SVD::new(su_mat, true, false);
    let basis_v = svd_v.u.ok_or(AutocallV2Error::InvalidGrid)?;
    let basis_u = svd_u.u.ok_or(AutocallV2Error::InvalidGrid)?;
    let d_eff = d.min(s).min(basis_v.ncols()).min(basis_u.ncols());
    if d_eff == 0 {
        return Err(AutocallV2Error::InvalidGrid);
    }

    let mat_target = build_transition_matrix_on_grid_info(&grid_info, &nig_max)?;
    let mut p_f64 = DMatrix::<f64>::zeros(s, s);
    for i in 0..s {
        for j in 0..s {
            p_f64[(i, j)] = mat_target[i][j] as f64 / SCALE_6 as f64;
        }
    }

    let v_leg = build_deim_leg(&basis_v, &p_f64, &grid_info, contract, atm, d_eff)?;
    let u_leg = build_deim_leg(&basis_u, &p_f64, &grid_info, contract, atm, d_eff)?;
    let deim_base = DeimFactors {
        v_leg,
        u_leg,
        n: s,
        atm_state: atm,
    };

    let mut p_ref_mat = DMatrix::<f64>::zeros(s, s);
    for i in 0..s {
        for j in 0..s {
            p_ref_mat[(i, j)] = p_ref_flat[i * s + j];
        }
    }

    let p_ref_at_eim = eim_flat_indices
        .iter()
        .map(|&idx| to_scale6_round(p_ref_flat[idx]))
        .collect::<Vec<_>>();
    let b_inv = (0..(m_eff * m_eff))
        .map(|idx| to_scale6_round(b_inv_f64[(idx / m_eff, idx % m_eff)]))
        .collect::<Vec<_>>();

    let (atoms_v, p_ref_red_v) =
        build_e11_atoms(&deim_base.v_leg, &u_op, &p_ref_mat, s, d_eff, m_eff);
    let (atoms_u, p_ref_red_u) =
        build_e11_atoms(&deim_base.u_leg, &u_op, &p_ref_mat, s, d_eff, m_eff);

    let factors = E11Factors {
        m: m_eff,
        d: d_eff,
        atoms_v,
        atoms_u,
        p_ref_red_v,
        p_ref_red_u,
        p_ref_at_eim,
        b_inv,
        eim_rows,
        eim_cols,
        grid_reps: grid_info.reps.clone(),
        grid_bounds: grid_info.bounds.clone(),
    };

    Ok(E11QuoteContext {
        factors,
        deim_base,
        grid_info,
    })
}

fn build_deim_leg(
    basis: &DMatrix<f64>,
    p_f64: &DMatrix<f64>,
    grid_info: &MarkovGridInfo,
    contract: &AutocallParams,
    atm: usize,
    d_eff: usize,
) -> Result<DeimLegData, AutocallV2Error> {
    let s = grid_info.n_states;
    let mut phi_f64 = DMatrix::<f64>::zeros(s, d_eff);
    for i in 0..s {
        for k in 0..d_eff {
            phi_f64[(i, k)] = basis[(i, k)];
        }
    }

    let idx = deim_select_f64(&phi_f64, d_eff);
    let mut phi_at_idx_f64 = DMatrix::<f64>::zeros(d_eff, d_eff);
    for i in 0..d_eff {
        for k in 0..d_eff {
            phi_at_idx_f64[(i, k)] = phi_f64[(idx[i], k)];
        }
    }
    let pt_inv_f64 = phi_at_idx_f64
        .clone()
        .try_inverse()
        .ok_or(AutocallV2Error::InvalidGrid)?;

    let p_red_f64 = phi_f64.transpose() * p_f64 * &phi_f64;

    let mut ki_diag = DMatrix::<f64>::zeros(s, s);
    let mut nki_diag = DMatrix::<f64>::zeros(s, s);
    for i in 0..s {
        if i <= grid_info.ki_state_max {
            ki_diag[(i, i)] = 1.0;
        } else {
            nki_diag[(i, i)] = 1.0;
        }
    }
    let m_ki_f64 = phi_f64.transpose() * &ki_diag * &phi_f64;
    let m_nki_f64 = phi_f64.transpose() * &nki_diag * &phi_f64;

    let mut ki_at = vec![false; d_eff];
    let mut cpn_at = vec![false; d_eff];
    let mut ac_at = vec![false; d_eff];
    for i in 0..d_eff {
        let state = idx[i];
        ki_at[i] = state <= grid_info.ki_state_max;
        cpn_at[i] = grid_info.reps[state] >= 0;
        ac_at[i] = grid_info.reps[state] >= contract.autocall_log_6;
    }

    let mut phi_atm = vec![0i64; d_eff];
    for k in 0..d_eff {
        phi_atm[k] = to_scale6_round(phi_f64[(atm, k)]);
    }

    let mut p_red = vec![0i64; d_eff * d_eff];
    let mut phi_at_idx = vec![0i64; d_eff * d_eff];
    let mut pt_inv = vec![0i64; d_eff * d_eff];
    let mut m_ki_red = vec![0i64; d_eff * d_eff];
    let mut m_nki_red = vec![0i64; d_eff * d_eff];
    let mut phi = vec![0i64; s * d_eff];
    for i in 0..d_eff {
        for j in 0..d_eff {
            p_red[i * d_eff + j] = to_scale6_round(p_red_f64[(i, j)]);
            phi_at_idx[i * d_eff + j] = to_scale6_round(phi_at_idx_f64[(i, j)]);
            pt_inv[i * d_eff + j] = to_scale6_round(pt_inv_f64[(i, j)]);
            m_ki_red[i * d_eff + j] = to_scale6_round(m_ki_f64[(i, j)]);
            m_nki_red[i * d_eff + j] = to_scale6_round(m_nki_f64[(i, j)]);
        }
    }
    for i in 0..s {
        for k in 0..d_eff {
            phi[i * d_eff + k] = to_scale6_round(phi_f64[(i, k)]);
        }
    }

    Ok(DeimLegData {
        p_red,
        phi_at_idx,
        pt_inv,
        phi_atm,
        m_ki_red,
        m_nki_red,
        ki_at_idx: ki_at,
        cpn_at_idx: cpn_at,
        ac_at_idx: ac_at,
        phi,
        d: d_eff,
    })
}

fn build_e11_atoms(
    leg: &DeimLegData,
    u_op: &DMatrix<f64>,
    p_ref_mat: &DMatrix<f64>,
    s: usize,
    d_eff: usize,
    m_eff: usize,
) -> (Vec<i64>, Vec<i64>) {
    let mut phi_f64 = DMatrix::<f64>::zeros(s, d_eff);
    for i in 0..s {
        for k in 0..d_eff {
            phi_f64[(i, k)] = leg.phi[i * d_eff + k] as f64 / SCALE_6 as f64;
        }
    }
    let phi_t = phi_f64.transpose();

    let mut atoms = vec![0i64; m_eff * d_eff * d_eff];
    for im in 0..m_eff {
        let mut dp_mode = DMatrix::<f64>::zeros(s, s);
        for i in 0..s {
            for j in 0..s {
                dp_mode[(i, j)] = u_op[(i * s + j, im)];
            }
        }
        let dp_red = &phi_t * &dp_mode * &phi_f64;
        let base = im * d_eff * d_eff;
        for i in 0..d_eff {
            for j in 0..d_eff {
                atoms[base + i * d_eff + j] = to_scale6_round(dp_red[(i, j)]);
            }
        }
    }

    let p_ref_red_f64 = &phi_t * p_ref_mat * &phi_f64;
    let p_ref_red = (0..(d_eff * d_eff))
        .map(|idx| to_scale6_round(p_ref_red_f64[(idx / d_eff, idx % d_eff)]))
        .collect::<Vec<_>>();
    (atoms, p_ref_red)
}

fn backward_pass_f64_snapshots(
    p: &DMatrix<f64>,
    reps: &[f64],
    grid_info: &MarkovGridInfo,
    coupon_val: f64,
    contract: &AutocallParams,
) -> Vec<Vec<f64>> {
    let s = reps.len();
    let mut snapshots = Vec::with_capacity(contract.n_obs + 1);

    let cpn_mask: Vec<bool> = reps.iter().map(|&r| r >= 0.0).collect();
    let ac_mask: Vec<bool> = reps
        .iter()
        .map(|&r| r >= contract.autocall_log_6 as f64 / SCALE_6 as f64)
        .collect();

    let cpn_term = cpn_mask
        .iter()
        .map(|&is_coupon| if is_coupon { coupon_val } else { 0.0 })
        .collect::<Vec<_>>();
    let mut val_u = cpn_term
        .iter()
        .map(|&coupon| 1.0 + coupon)
        .collect::<Vec<_>>();
    let mut val_t = reps
        .iter()
        .zip(cpn_term.iter())
        .map(|(&rep, &coupon)| {
            let redemption = if rep < 0.0 { rep.exp() } else { 1.0 };
            redemption + coupon
        })
        .collect::<Vec<_>>();

    for step in 0..contract.n_obs {
        let is_day0 = step == contract.n_obs - 1;
        let autocall_suppressed = !is_day0
            && contract.no_autocall_first_n_obs > 0
            && (contract.n_obs - 1 - step) <= contract.no_autocall_first_n_obs;
        snapshots.push(val_u.clone());

        let mut e_t = vec![0.0f64; s];
        let mut e_u = vec![0.0f64; s];
        for i in 0..s {
            let mut sum_t = 0.0;
            let mut sum_u = 0.0;
            for j in 0..s {
                let pij = p[(i, j)];
                let branch_u = if j <= grid_info.ki_state_max {
                    val_t[j]
                } else {
                    val_u[j]
                };
                sum_u += pij * branch_u;
                sum_t += pij * val_t[j];
            }
            e_u[i] = sum_u;
            e_t[i] = sum_t;
        }

        if is_day0 {
            val_u = e_u;
            val_t = e_t;
        } else {
            for i in 0..s {
                let coupon = if cpn_mask[i] { coupon_val } else { 0.0 };
                if !autocall_suppressed && ac_mask[i] {
                    val_u[i] = 1.0 + coupon;
                    val_t[i] = 1.0 + coupon;
                } else if i <= grid_info.ki_state_max {
                    val_u[i] = e_t[i] + coupon;
                    val_t[i] = e_t[i] + coupon;
                } else {
                    val_u[i] = e_u[i] + coupon;
                    val_t[i] = e_t[i] + coupon;
                }
            }
        }
    }
    snapshots.push(val_u);
    snapshots
}

fn deim_select_f64(phi: &DMatrix<f64>, d: usize) -> Vec<usize> {
    let n = phi.nrows();
    let mut indices = Vec::with_capacity(d);

    let mut first_idx = 0usize;
    let mut first_val = 0.0f64;
    for i in 0..n {
        let value = phi[(i, 0)].abs();
        if value > first_val {
            first_val = value;
            first_idx = i;
        }
    }
    indices.push(first_idx);

    for k in 1..d {
        let mut p_t = DMatrix::<f64>::zeros(k, k);
        for (row, &idx) in indices.iter().enumerate() {
            for col in 0..k {
                p_t[(row, col)] = phi[(idx, col)];
            }
        }
        let mut rhs = DVector::<f64>::zeros(k);
        for (row, &idx) in indices.iter().enumerate() {
            rhs[row] = phi[(idx, k)];
        }
        let coeffs = p_t.lu().solve(&rhs).unwrap_or_else(|| DVector::zeros(k));

        let mut best_idx = 0usize;
        let mut best_residual = 0.0f64;
        for i in 0..n {
            let mut approx = 0.0;
            for col in 0..k {
                approx += phi[(i, col)] * coeffs[col];
            }
            let residual = (phi[(i, k)] - approx).abs();
            if residual > best_residual {
                best_residual = residual;
                best_idx = i;
            }
        }
        indices.push(best_idx);
    }

    indices
}

fn sigma_training_grid() -> Vec<f64> {
    (0..21).map(|i| 0.50 + i as f64 * 0.10).collect()
}

fn to_scale6_round(value: f64) -> i64 {
    (value * SCALE_6 as f64).round() as i64
}
