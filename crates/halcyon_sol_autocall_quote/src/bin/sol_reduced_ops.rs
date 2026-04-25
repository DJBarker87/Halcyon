use halcyon_sol_autocall_quote::autocall_v2::AutocallParams;
use halcyon_sol_autocall_quote::autocall_v2_e11::{
    precompute_reduced_operators_from_const, solve_fair_coupon_deim_from_precomputed_const,
};
use halcyon_sol_autocall_quote::generated::pod_deim_table::{
    TRAINING_ALPHA_S6, TRAINING_AUTOCALL_LOG_6, TRAINING_BETA_S6, TRAINING_KNOCK_IN_LOG_6,
    TRAINING_NO_AUTOCALL_FIRST_N_OBS, TRAINING_N_OBS, TRAINING_REFERENCE_STEP_DAYS,
};
use serde::Serialize;

#[derive(Serialize)]
struct ReducedOpsJson {
    sigma_ann_s6: i64,
    fair_coupon_bps: u64,
    p_red_v: Vec<i64>,
    p_red_u: Vec<i64>,
}

fn main() {
    let sigma_ann_s6 = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "500000".to_string())
        .parse::<i64>()
        .expect("sigma_ann_s6 must be an i64");
    let contract = AutocallParams {
        n_obs: TRAINING_N_OBS,
        knock_in_log_6: TRAINING_KNOCK_IN_LOG_6,
        autocall_log_6: TRAINING_AUTOCALL_LOG_6,
        no_autocall_first_n_obs: TRAINING_NO_AUTOCALL_FIRST_N_OBS,
    };
    let reduced = precompute_reduced_operators_from_const(
        sigma_ann_s6,
        TRAINING_ALPHA_S6,
        TRAINING_BETA_S6,
        TRAINING_REFERENCE_STEP_DAYS,
        &contract,
    )
    .expect("precompute reduced operators");
    let priced = solve_fair_coupon_deim_from_precomputed_const(
        &reduced.p_red_v,
        &reduced.p_red_u,
        &contract,
    )
    .expect("price reduced operators");
    let json = ReducedOpsJson {
        sigma_ann_s6: reduced.sigma_ann_6,
        fair_coupon_bps: priced.fair_coupon_bps,
        p_red_v: reduced.p_red_v,
        p_red_u: reduced.p_red_u,
    };
    println!("{}", serde_json::to_string(&json).expect("serialize json"));
}
