mod common;

use common::{
    edge_day_before_maturity, final_interval_start, fixtures_path, inputs_for_state,
    snapshot_fixture, MidlifeFixture, MidlifeFixtureFile, SCHEMA_VERSION,
};
use halcyon_flagship_quote::midlife_pricer::{
    compute_midlife_nav, compute_midlife_nav_monthly_debug,
};
#[cfg(not(target_os = "solana"))]
use halcyon_flagship_quote::midlife_reference::nav_c1_filter_mid_life;

#[derive(Clone, Debug)]
struct FixtureCase {
    label: String,
    inputs: halcyon_flagship_quote::midlife_pricer::MidlifeInputs,
}

fn fixture_input_corpus() -> Vec<FixtureCase> {
    let timepoints = [0u8, 3, 6, 9, 12, 17];
    let sigmas = [100_000i64, 180_000, 280_000];
    let healthy_levels = [
        ("r120", [1_200_000, 1_180_000, 1_220_000]),
        ("r110", [1_100_000, 1_080_000, 1_120_000]),
        ("r103", [1_030_000, 1_020_000, 1_050_000]),
        ("r100", [1_010_000, 1_000_000, 1_040_000]),
        ("r095", [960_000, 950_000, 980_000]),
    ];
    let near_ki_levels = [
        ("r083", [850_000, 830_000, 860_000]),
        ("r082", [840_000, 820_000, 850_000]),
        ("r081", [830_000, 810_000, 845_000]),
        ("r080", [820_000, 800_000, 840_000]),
        ("r079", [810_000, 790_000, 835_000]),
    ];
    let post_ki_moderate = [
        ("r078", [800_000, 780_000, 810_000]),
        ("r074", [760_000, 740_000, 770_000]),
        ("r070", [720_000, 700_000, 735_000]),
        ("r065", [680_000, 650_000, 690_000]),
    ];
    let post_ki_severe = [
        ("r060", [630_000, 600_000, 620_000]),
        ("r050", [530_000, 500_000, 520_000]),
    ];
    let edge_levels = [
        ("healthy", [1_050_000, 1_020_000, 1_080_000], false, 0u8),
        ("near_ki", [830_000, 810_000, 845_000], false, 1u8),
        ("post_ki", [720_000, 700_000, 735_000], true, 2u8),
        ("severe", [530_000, 500_000, 520_000], true, 3u8),
    ];

    let mut cases = Vec::new();
    for &sigma in &sigmas {
        for &coupon_idx in &timepoints {
            let now = final_interval_start(coupon_idx);
            let coupons_paid = u64::from(coupon_idx) * 250_000;
            let near_ki_missed = 1.min(coupon_idx);
            let moderate_missed = 2.min(coupon_idx);
            let severe_missed = 3.min(coupon_idx);
            for &(spot_label, ratios) in &healthy_levels {
                let label = format!("healthy/{spot_label}/sigma={sigma}/coupon={coupon_idx}");
                let inputs =
                    inputs_for_state(ratios, sigma, coupon_idx, false, 0, coupons_paid, now);
                cases.push(FixtureCase { label, inputs });
            }
            for &(spot_label, ratios) in &near_ki_levels {
                let label = format!("near_ki/{spot_label}/sigma={sigma}/coupon={coupon_idx}");
                let inputs = inputs_for_state(
                    ratios,
                    sigma,
                    coupon_idx,
                    false,
                    near_ki_missed,
                    coupons_paid,
                    now,
                );
                cases.push(FixtureCase { label, inputs });
            }
            for &(spot_label, ratios) in &post_ki_moderate {
                let label =
                    format!("post_ki_moderate/{spot_label}/sigma={sigma}/coupon={coupon_idx}");
                let inputs = inputs_for_state(
                    ratios,
                    sigma,
                    coupon_idx,
                    true,
                    moderate_missed,
                    coupons_paid,
                    now,
                );
                cases.push(FixtureCase { label, inputs });
            }
            for &(spot_label, ratios) in &post_ki_severe {
                let label =
                    format!("post_ki_severe/{spot_label}/sigma={sigma}/coupon={coupon_idx}");
                let inputs = inputs_for_state(
                    ratios,
                    sigma,
                    coupon_idx,
                    true,
                    severe_missed,
                    coupons_paid,
                    now,
                );
                cases.push(FixtureCase { label, inputs });
            }
        }

        for &(edge_label, ratios, ki_latched, missed) in &edge_levels {
            let label = format!("edge/{edge_label}/sigma={sigma}");
            let inputs = inputs_for_state(
                ratios,
                sigma,
                17,
                ki_latched,
                missed,
                4_250_000,
                edge_day_before_maturity(),
            );
            cases.push(FixtureCase { label, inputs });
        }
    }

    cases
}

#[cfg(not(target_os = "solana"))]
fn generate_reference_vectors() -> Vec<MidlifeFixture> {
    let cases = fixture_input_corpus();
    let thread_count = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(1)
        .min(cases.len().max(1));
    let chunk_size = cases.len().div_ceil(thread_count);

    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for chunk in cases.chunks(chunk_size) {
            handles.push(scope.spawn(move || {
                chunk
                    .iter()
                    .map(|case| {
                        let nav = nav_c1_filter_mid_life(&case.inputs)
                            .unwrap_or_else(|err| panic!("{}: {:?}", case.label, err));
                        snapshot_fixture(case.label.clone(), case.inputs, nav)
                    })
                    .collect::<Vec<_>>()
            }));
        }

        let mut out = Vec::with_capacity(cases.len());
        for handle in handles {
            out.extend(handle.join().expect("reference fixture worker panicked"));
        }
        out
    })
}

#[test]
fn generated_midlife_snapshot_is_large_enough() {
    let cases = fixture_input_corpus();
    assert_eq!(cases.len(), 300, "expected a 300-vector snapshot corpus");
}

#[test]
#[ignore = "writes the committed snapshot fixture file"]
fn regenerate_midlife_fixture_file() {
    let path = fixtures_path();
    std::fs::create_dir_all(path.parent().expect("fixture dir")).expect("fixture dir");
    let file = MidlifeFixtureFile {
        schema_version: SCHEMA_VERSION,
        reference_fn: "nav_c1_filter_mid_life".to_string(),
        quadrature: "GH9".to_string(),
        vectors: generate_reference_vectors(),
    };
    let json = serde_json::to_string_pretty(&file).expect("serialize fixture file");
    std::fs::write(&path, json).unwrap_or_else(|e| panic!("failed to write {:?}: {e}", path));
}

#[cfg(not(target_os = "solana"))]
#[test]
#[ignore = "diagnostic comparison against the host-side reference model"]
fn compare_snapshot_against_midlife_reference() {
    let vectors = fixture_input_corpus();
    let mut max_abs_diff = 0i64;
    let mut worst_label = String::new();

    for fixture in vectors {
        let reference = nav_c1_filter_mid_life(&fixture.inputs).expect("host reference nav");
        let fixed_point = compute_midlife_nav(&fixture.inputs).expect("fixed-point nav");
        let diff = (fixed_point.nav_s6 - reference.nav_s6).abs();
        if diff > max_abs_diff {
            max_abs_diff = diff;
            worst_label = fixture.label.clone();
        }
    }

    println!(
        "snapshot vs host reference max_abs_diff_s6={} worst={}",
        max_abs_diff, worst_label
    );
}

#[cfg(not(target_os = "solana"))]
#[test]
#[ignore = "diagnostic comparison against the committed independent fixture values"]
fn compare_monthly_debug_against_committed_midlife_vectors() {
    let path = fixtures_path();
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {:?}: {e}", path));
    let file: MidlifeFixtureFile =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse {:?}: {e}", path));

    let mut max_abs_diff = 0i64;
    let mut p95_abs_diff = 0i64;
    let mut overstated = 0usize;
    let mut understated = 0usize;
    let mut exact = 0usize;
    let mut worst = Vec::new();
    let mut diffs = Vec::with_capacity(file.vectors.len());

    for fixture in file.vectors {
        let nav = compute_midlife_nav_monthly_debug(&fixture.inputs)
            .unwrap_or_else(|err| panic!("{} failed: {:?}", fixture.label, err));
        let signed = nav.nav_s6 - fixture.expected_nav_s6;
        let abs = signed.abs();
        if signed > 0 {
            overstated += 1;
        } else if signed < 0 {
            understated += 1;
        } else {
            exact += 1;
        }
        max_abs_diff = max_abs_diff.max(abs);
        diffs.push(abs);
        worst.push((
            abs,
            signed,
            fixture.label,
            nav.nav_s6,
            fixture.expected_nav_s6,
        ));
    }

    diffs.sort_unstable();
    if !diffs.is_empty() {
        let idx = ((diffs.len() - 1) * 95) / 100;
        p95_abs_diff = diffs[idx];
    }
    worst.sort_by(|lhs, rhs| rhs.0.cmp(&lhs.0));

    println!(
        "monthly debug committed vectors count={} over={} under={} exact={} max_abs_diff_s6={} p95_abs_diff_s6={}",
        diffs.len(),
        overstated,
        understated,
        exact,
        max_abs_diff,
        p95_abs_diff
    );
    for (abs, signed, label, got, expected) in worst.iter().take(25) {
        println!("{label} signed_diff_s6={signed} abs_diff_s6={abs} got={got} expected={expected}");
    }
}

#[cfg(not(target_os = "solana"))]
#[test]
#[ignore = "diagnostic comparison against the committed independent fixture values"]
fn compare_pricer_against_committed_midlife_vectors() {
    let path = fixtures_path();
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {:?}: {e}", path));
    let file: MidlifeFixtureFile =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse {:?}: {e}", path));

    let mut max_abs_diff = 0i64;
    let mut p95_abs_diff = 0i64;
    let mut overstated = 0usize;
    let mut understated = 0usize;
    let mut worst = Vec::new();
    let mut diffs = Vec::with_capacity(file.vectors.len());

    for fixture in file.vectors {
        let nav = compute_midlife_nav(&fixture.inputs)
            .unwrap_or_else(|err| panic!("{} failed: {:?}", fixture.label, err));
        let signed = nav.nav_s6 - fixture.expected_nav_s6;
        let abs = signed.abs();
        if signed > 0 {
            overstated += 1;
        } else if signed < 0 {
            understated += 1;
        }
        max_abs_diff = max_abs_diff.max(abs);
        diffs.push(abs);
        worst.push((
            abs,
            signed,
            fixture.label,
            nav.nav_s6,
            fixture.expected_nav_s6,
        ));
    }

    diffs.sort_unstable();
    if !diffs.is_empty() {
        let idx = ((diffs.len() - 1) * 95) / 100;
        p95_abs_diff = diffs[idx];
    }
    worst.sort_by(|lhs, rhs| rhs.0.cmp(&lhs.0));

    println!(
        "committed vectors count={} over={} under={} max_abs_diff_s6={} p95_abs_diff_s6={}",
        diffs.len(),
        overstated,
        understated,
        max_abs_diff,
        p95_abs_diff
    );
    for (abs, signed, label, got, expected) in worst.iter().take(25) {
        println!("{label} signed_diff_s6={signed} abs_diff_s6={abs} got={got} expected={expected}");
    }
}

#[cfg(not(target_os = "solana"))]
#[test]
#[ignore = "writes current pricer outputs for the committed midlife vectors"]
fn dump_pricer_outputs_for_committed_midlife_vectors() {
    let path = fixtures_path();
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {:?}: {e}", path));
    let file: MidlifeFixtureFile =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse {:?}: {e}", path));

    let entries = file
        .vectors
        .into_iter()
        .map(|fixture| {
            let nav = compute_midlife_nav(&fixture.inputs)
                .unwrap_or_else(|err| panic!("{} failed: {:?}", fixture.label, err));
            serde_json::json!({
                "label": fixture.label,
                "inputs": fixture.inputs,
                "expected_nav_s6": fixture.expected_nav_s6,
                "priced_nav_s6": nav.nav_s6,
                "correction_s6": fixture.expected_nav_s6 - nav.nav_s6,
            })
        })
        .collect::<Vec<_>>();

    let out_path = std::env::var("MIDLIFE_DUMP_PRICER_OUTPUTS")
        .unwrap_or_else(|_| "research/midlife_pricer_outputs.json".to_string());
    std::fs::write(
        &out_path,
        serde_json::to_string_pretty(&entries).expect("serialize pricer dump"),
    )
    .unwrap_or_else(|err| panic!("failed to write {out_path}: {err}"));
    println!("wrote {out_path}");
}

#[cfg(not(target_os = "solana"))]
#[test]
#[ignore = "diagnostic host-reference values for production-path states"]
fn print_production_path_reference_values() {
    let offered_coupon_bps_s6 = std::env::var("MIDLIFE_DIAG_OFFERED_COUPON_BPS_S6")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(500_000_000);
    let sigma_common_s6 = std::env::var("MIDLIFE_DIAG_SIGMA_S6")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(400_000);
    let mut cases = [
        (
            "production/r100/coupon=0",
            inputs_for_state(
                [1_010_000, 1_000_000, 1_040_000],
                sigma_common_s6,
                0,
                false,
                0,
                0,
                final_interval_start(0),
            ),
        ),
        (
            "production/r100/ceiling-sigma/coupon=0",
            inputs_for_state(
                [1_010_000, 1_000_000, 1_040_000],
                800_000,
                0,
                false,
                0,
                0,
                final_interval_start(0),
            ),
        ),
        (
            "production/r095/coupon=6",
            inputs_for_state(
                [960_000, 950_000, 980_000],
                sigma_common_s6,
                6,
                false,
                0,
                1_500_000,
                final_interval_start(6),
            ),
        ),
        (
            "production/near_ki_r080/coupon=17",
            inputs_for_state(
                [820_000, 800_000, 840_000],
                sigma_common_s6,
                17,
                false,
                1,
                4_250_000,
                final_interval_start(17),
            ),
        ),
    ];

    for (_, inputs) in cases.iter_mut() {
        inputs.offered_coupon_bps_s6 = offered_coupon_bps_s6;
    }

    for (label, inputs) in cases {
        let nav = nav_c1_filter_mid_life(&inputs).expect("host reference nav");
        println!(
            "{label} sigma_s6={} offered_coupon_bps_s6={} nav_s6={} coupon_pv_s6={} par_recovery_s6={} ki_level_s6={}",
            inputs.sigma_common_s6,
            inputs.offered_coupon_bps_s6,
            nav.nav_s6,
            nav.remaining_coupon_pv_s6,
            nav.par_recovery_probability_s6,
            nav.ki_level_usd_s6
        );
    }
}
