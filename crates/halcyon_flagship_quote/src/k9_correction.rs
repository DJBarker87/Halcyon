//! K=9 discretisation correction table.
//!
//! correction[i] = (fc_live_K15 - fc_frozen_K9) at sigma_i.
//! Stored in micro-bps (1 unit = 1e-6 bps).
//!
//! Regenerate: `cargo run -p halcyon-quote --bin gen_k9_correction`

const N: usize = 64;
const SIGMA_MIN_S6: i64 = 80000;
const SIGMA_MAX_S6: i64 = 800000;
const SIGMA_RANGE_S6: i64 = SIGMA_MAX_S6 - SIGMA_MIN_S6;
const N1: i64 = (N - 1) as i64;

const K9_CORRECTION: [i64; N] = [
    -47583400,
    -62086600,
    -54522000,
    -65209300,
    -28001900,
    -56579300,
    -51999400,
    -21475300,
    -32540500,
    -44997200,
    -48376800,
    -38991000,
    -45989000,
    -55330800,
    -28830200,
    -24196500,
    -37638300,
    -64692400,
    -60683200,
    -60499000,
    -77495200,
    -72729400,
    -67229800,
    -94603100,
    -94874900,
    -100914200,
    -96908100,
    -122608000,
    -135992700,
    -160464700,
    -185743800,
    -187660400,
    -186186200,
    -187677600,
    -217171000,
    -242702200,
    -247607500,
    -243550000,
    -304993400,
    -318040700,
    -328091900,
    -334282500,
    -383604600,
    -406327200,
    -410652200,
    -460840900,
    -490525000,
    -490102200,
    -616778800,
    -726737100,
    -740806500,
    -753340200,
    -841487700,
    -865621500,
    -912363300,
    -943180800,
    -1000417200,
    -1035715100,
    -1091493300,
    -1112074200,
    -1075332000,
    -1092482500,
    -1123115000,
    -1148020300,
];

/// Look up K=9 correction at a given sigma. Returns micro-bps.
#[inline(always)]
pub fn k9_correction_lookup(sigma_s6: i64) -> i64 {
    let sigma = sigma_s6.clamp(SIGMA_MIN_S6, SIGMA_MAX_S6);
    let offset = sigma - SIGMA_MIN_S6;
    let scaled = offset * N1;
    let i0 = (scaled / SIGMA_RANGE_S6) as usize;
    let i0 = if i0 >= N - 1 { N - 2 } else { i0 };
    let frac_num = scaled - i0 as i64 * SIGMA_RANGE_S6;
    let v0 = K9_CORRECTION[i0];
    let v1 = K9_CORRECTION[i0 + 1];
    v0 + (v1 - v0) * frac_num / SIGMA_RANGE_S6
}
