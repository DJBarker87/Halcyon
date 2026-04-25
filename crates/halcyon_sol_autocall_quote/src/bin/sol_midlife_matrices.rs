use std::env;

use halcyon_sol_autocall_quote::midlife::{
    build_midlife_transition_matrix_for_upload, SOL_AUTOCALL_MIDLIFE_COS_TERMS,
    SOL_AUTOCALL_MIDLIFE_MARKOV_STATES,
};
use serde::Serialize;

#[derive(Serialize)]
struct MatrixStep {
    step_days_s6: i64,
    values: Vec<i64>,
}

#[derive(Serialize)]
struct MatrixUpload {
    sigma_ann_s6: i64,
    n_states: usize,
    cos_terms: u16,
    steps: Vec<MatrixStep>,
}

fn parse_i64_arg(value: &str, label: &str) -> i64 {
    value
        .parse::<i64>()
        .unwrap_or_else(|err| panic!("invalid {label} `{value}`: {err}"))
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    if args.len() < 3 {
        eprintln!(
            "usage: {} <sigma_ann_s6> <step_days_s6> [<step_days_s6>...]",
            args[0]
        );
        std::process::exit(2);
    }

    let sigma_ann_s6 = parse_i64_arg(&args[1], "sigma_ann_s6");
    let mut steps = Vec::with_capacity(args.len() - 2);
    for arg in &args[2..] {
        let step_days_s6 = parse_i64_arg(arg, "step_days_s6");
        let values = build_midlife_transition_matrix_for_upload(sigma_ann_s6, step_days_s6)
            .unwrap_or_else(|err| {
                panic!("failed to build matrix for step_days_s6={step_days_s6}: {err:?}")
            });
        steps.push(MatrixStep {
            step_days_s6,
            values,
        });
    }

    let upload = MatrixUpload {
        sigma_ann_s6,
        n_states: SOL_AUTOCALL_MIDLIFE_MARKOV_STATES,
        cos_terms: SOL_AUTOCALL_MIDLIFE_COS_TERMS,
        steps,
    };
    println!(
        "{}",
        serde_json::to_string(&upload).expect("serialize upload")
    );
}
