mod common;

use common::{fixtures_path, MidlifeFixtureFile, SCHEMA_VERSION};
use halcyon_flagship_quote::midlife_pricer::{
    advance_midlife_nav_in_place, checkpoint_next_coupon_index, compute_midlife_nav,
    finish_midlife_nav_from_bytes, start_midlife_nav_into, MidlifeInputs, MIDLIFE_CHECKPOINT_BYTES,
};

const NAV_TOLERANCE_S6: i64 = 100;

#[test]
fn committed_midlife_fixture_file_loads() {
    let path = fixtures_path();
    assert!(path.exists(), "missing committed fixtures at {:?}", path);

    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {:?}: {e}", path));
    let file: MidlifeFixtureFile =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse {:?}: {e}", path));

    assert_eq!(file.schema_version, SCHEMA_VERSION);
    assert_eq!(file.quadrature, "GH9");
    assert_eq!(file.reference_fn, "nav_c1_filter_mid_life");
    assert!(
        !file.vectors.is_empty(),
        "fixture file is present but contains zero vectors"
    );
}

#[test]
fn midlife_nav_matches_committed_vectors() {
    let path = fixtures_path();
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {:?}: {e}", path));
    let file: MidlifeFixtureFile =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse {:?}: {e}", path));

    for fixture in file.vectors {
        let nav = compute_midlife_nav(&fixture.inputs)
            .unwrap_or_else(|err| panic!("{} failed: {:?}", fixture.label, err));
        let nav_diff = (nav.nav_s6 - fixture.expected_nav_s6).abs();
        assert!(
            nav_diff <= NAV_TOLERANCE_S6,
            "{} nav mismatch: got {} expected {} diff {} tolerance {}",
            fixture.label,
            nav.nav_s6,
            fixture.expected_nav_s6,
            nav_diff,
            NAV_TOLERANCE_S6
        );
        assert_eq!(
            nav.ki_level_usd_s6, fixture.expected_ki_level_usd_s6,
            "{} ki level mismatch",
            fixture.label
        );
    }
}

#[test]
fn checkpoint_boundaries_preserve_committed_vector_navs() {
    let path = fixtures_path();
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {:?}: {e}", path));
    let file: MidlifeFixtureFile =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse {:?}: {e}", path));

    for fixture in file.vectors {
        assert_checkpoint_boundaries_preserve_one_shot(&fixture.label, &fixture.inputs);
    }
}

#[test]
fn checkpoint_boundaries_preserve_held_out_generated_states() {
    let path = fixtures_path();
    let raw =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {:?}: {e}", path));
    let file: MidlifeFixtureFile =
        serde_json::from_str(&raw).unwrap_or_else(|e| panic!("failed to parse {:?}: {e}", path));

    for (seed, fixture) in file.vectors.iter().step_by(23).take(13).enumerate() {
        let mut inputs = fixture.inputs;
        let seed = seed as i64 + 1;
        inputs.current_spy_s6 += 17_003 * seed;
        inputs.current_qqq_s6 -= 11_011 * seed;
        inputs.current_iwm_s6 += 7_019 * (seed % 5);
        inputs.sigma_common_s6 =
            (inputs.sigma_common_s6 + 3_001 * ((seed % 7) - 3)).clamp(80_000, 320_000);
        inputs.regression_residual_vol_s6 =
            (inputs.regression_residual_vol_s6 + 2_003 * ((seed % 5) - 2)).max(1);
        inputs.offered_coupon_bps_s6 =
            (inputs.offered_coupon_bps_s6 + 1_000_000 * ((seed % 3) - 1)).max(0);

        assert_checkpoint_boundaries_preserve_one_shot(
            &format!("held_out/{}", fixture.label),
            &inputs,
        );
    }
}

fn assert_checkpoint_boundaries_preserve_one_shot(label: &str, inputs: &MidlifeInputs) {
    let one_shot = compute_midlife_nav(inputs)
        .unwrap_or_else(|err| panic!("{label} one-shot failed: {err:?}"));
    for chunk_size in [1u8, 2, 3, 6, 18] {
        let mut checkpoint = vec![0u8; MIDLIFE_CHECKPOINT_BYTES];
        let mut cursor = inputs.next_coupon_index;
        let initial_stop = cursor.saturating_add(chunk_size).min(18);
        start_midlife_nav_into(inputs, initial_stop, &mut checkpoint)
            .unwrap_or_else(|err| panic!("{label} chunk {chunk_size} prepare failed: {err:?}"));
        cursor = checkpoint_next_coupon_index(&checkpoint).unwrap_or_else(|err| {
            panic!("{label} chunk {chunk_size} prepare cursor failed: {err:?}")
        });

        let mut guard = 0usize;
        while cursor < 18 {
            guard += 1;
            assert!(
                guard <= 64,
                "{label} chunk {chunk_size} did not make bounded checkpoint progress"
            );
            let next_stop = cursor.saturating_add(chunk_size).min(18);
            advance_midlife_nav_in_place(inputs, &mut checkpoint, next_stop).unwrap_or_else(
                |err| panic!("{label} chunk {chunk_size} advance to {next_stop} failed: {err:?}"),
            );
            let next_cursor = checkpoint_next_coupon_index(&checkpoint).unwrap_or_else(|err| {
                panic!("{label} chunk {chunk_size} advance cursor failed: {err:?}")
            });
            assert!(
                next_cursor > cursor,
                "{label} chunk {chunk_size} stalled at coupon {cursor}"
            );
            cursor = next_cursor;
        }

        let resumed = finish_midlife_nav_from_bytes(inputs, &checkpoint)
            .unwrap_or_else(|err| panic!("{label} chunk {chunk_size} finish failed: {err:?}"));
        assert_eq!(
            resumed, one_shot,
            "{label} chunk {chunk_size} checkpointed NAV diverged from one-shot"
        );
    }
}
