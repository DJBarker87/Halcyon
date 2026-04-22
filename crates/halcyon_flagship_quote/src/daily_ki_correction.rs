//! Daily-KI cadence correction table.
//!
//! correction[i] = sensitivity(σ_i) × cos3d_delta(σ_i)  at micro-bps scale.
//! Closes the gap between the c1 filter's obs-date-only KI monitoring and
//! the product's actual daily-KI monitoring (see product_economics/
//! worst_of_math_stack.md §5).
//!
//! cos3d source: method="hybrid".
//! Upstream source-table provenance SHA-256:
//! 907e9d60af467b0967a43712fd6b6fad63a5237e447b0fe6e635fdc75a94d26a
//! Sensitivity: 40 σ anchors, 100000 MC path-pairs/mode.
//!
//! Storage: 256 × i64 micro-bps (1 unit = 1e-6 bps) = 2,048 bytes.
//! Lookup: Catmull-Rom on σ, ~150 CU. Apply by adding the result (divided
//! by 1_000_000) to the filter's fair_coupon_bps after k12_correction.
//!
//! Derived daily-KI artifact SHA-256:
//! f89ac13789dafe2f933d473799f9b617b666d2ac7682c90bc50f59862e61ee0f
//!
//! Regenerate: `python3 halcyon-hedge-lab/cos3d/scripts/generate_daily_ki_correction.py`

const N: usize = 256;
const SIGMA_MIN_S6: i64 = 80000;
const SIGMA_MAX_S6: i64 = 800000;
const SIGMA_RANGE_S6: i64 = SIGMA_MAX_S6 - SIGMA_MIN_S6;
const N1: i64 = (N - 1) as i64;

const DAILY_KI_CORRECTION: [i64; N] = [
    34495093, 35817895, 36689098, 37526185, 38689902, 40277288, 42142120, 44033598, 45738799,
    47165226, 48344618, 49383897, 50402106, 51482928, 52656064, 53902882, 55174806, 56415286,
    57578372, 58638936, 59593716, 60455948, 61247157, 61989104, 62698023, 63382062, 64041501,
    64670556, 65259722, 65798140, 66275749, 66685002, 67021909, 67286315, 67481532, 67613590,
    67690346, 67720601, 67713271, 67676625, 65492152, 67541533, 60817623, 65586701, 62960294,
    65272780, 62583181, 65136510, 66302624, 63800382, 65299041, 60388284, 62791333, 62115580,
    60928075, 62163786, 62307572, 63429548, 62613781, 61433228, 61446051, 59404285, 58413583,
    57683096, 56497915, 57798695, 54213055, 55051851, 54787838, 55340645, 53178923, 53904018,
    54139541, 52458709, 51733889, 51007847, 50280579, 49551994, 48821952, 48090296, 47356888,
    46621629, 45884485, 45145493, 44404766, 43662498, 42918952, 42174456, 41429387, 40684166,
    39939238, 39195068, 38452128, 37710890, 36971821, 36235378, 35502006, 34772138, 34046194,
    33324582, 32607698, 31895928, 31189648, 30489227, 29795022, 29107383, 28426649, 27753150,
    27087200, 26429105, 25779152, 25137615, 24504750, 23880796, 23265974, 22660486, 22064516,
    21478232, 20901782, 20335301, 19778907, 19232705, 18696787, 18171234, 17656115, 17151494,
    16657423, 16173949, 15701113, 15220620, 14565480, 14457235, 13982384, 13422007, 13227465,
    12427759, 12017031, 11914886, 11562532, 10995141, 10350043, 10352279, 9878032, 9794352,
    9440532, 8952051, 9301332, 8657629, 8425626, 8288291, 8199589, 7833265, 7605810, 7258250,
    7294006, 6956146, 6827698, 6425414, 6323922, 6198377, 6117391, 6088776, 6025556, 5681903,
    5647872, 5542753, 5385783, 5472873, 5200025, 5211606, 5050631, 4884020, 5047795, 5136389,
    4880652, 5037108, 5003385, 5260081, 4953272, 5058342, 4949525, 5105819, 4868755, 4933616,
    5147068, 5043373, 5048084, 5144116, 5125958, 5321779, 5251640, 5345573, 5384915, 5445420,
    5480408, 5532709, 5650137, 5763436, 5815163, 5829555, 5791773, 6061362, 6267589, 6106157,
    6283188, 6272391, 6227509, 6521467, 6483269, 6484956, 6653260, 6421447, 6683473, 6665348,
    6774623, 6639506, 6636416, 6857059, 6910767, 6746774, 6816834, 6855748, 7046640, 7078932,
    6666364, 6865591, 6686802, 6844685, 6473851, 6593310, 6466411, 6364392, 6454861, 6221417,
    6164110, 6085500, 5670783, 5667571, 5516720, 5256396, 5189831, 4891813, 4544691, 4504975,
    4145338, 3821238, 3467235, 3176746, 2834061, 2476079, 2090288, 1702260, 1285486, 835233,
    386551, -89478,
];

// Catmull-Rom interpolation on the 256-point σ grid (C¹ across knots).
// Matches the layout of k12_correction.rs::CR_W.
const WN: usize = 1024;
const WS: u32 = 30;

const CR_W: [[i32; 4]; WN] = {
    let mut out = [[0i32; 4]; WN];
    let scale: i64 = 1 << WS;
    let mut k = 0usize;
    while k < WN {
        let t = (k as i64) << (WS - 10);
        let t2 = t * t >> WS;
        let t3 = t2 * t >> WS;
        out[k][0] = ((-t + 2 * t2 - t3) / 2) as i32;
        out[k][1] = ((2 * scale - 5 * t2 + 3 * t3) / 2) as i32;
        out[k][2] = ((t + 4 * t2 - 3 * t3) / 2) as i32;
        out[k][3] = ((-t2 + t3) / 2) as i32;
        k += 1;
    }
    out
};

#[inline(always)]
fn cr_dot(w: &[i32; 4], p0: i64, p1: i64, p2: i64, p3: i64) -> i64 {
    (w[0] as i64 * p0 + w[1] as i64 * p1 + w[2] as i64 * p2 + w[3] as i64 * p3) >> WS
}

/// Look up daily-KI correction at a given sigma via Catmull-Rom on the
/// N sample points. Returns micro-bps. Add `result as f64 / 1e6` to
/// fair_coupon_bps (after the k12 correction) to apply.
#[inline(always)]
pub fn daily_ki_correction_lookup(sigma_s6: i64) -> i64 {
    let sigma = sigma_s6.clamp(SIGMA_MIN_S6, SIGMA_MAX_S6);
    let offset = sigma - SIGMA_MIN_S6;
    let scaled = offset * N1;
    let i0 = (scaled / SIGMA_RANGE_S6) as usize;
    let i0 = if i0 >= N - 1 { N - 2 } else { i0 };
    let frac_num = scaled - i0 as i64 * SIGMA_RANGE_S6;
    let wi = ((frac_num as i128 * WN as i128 / SIGMA_RANGE_S6 as i128) as usize).min(WN - 1);
    let w = &CR_W[wi];
    let n = N as i64;
    let p0 = DAILY_KI_CORRECTION[((i0 as i64) - 1).clamp(0, n - 1) as usize];
    let p1 = DAILY_KI_CORRECTION[(i0 as i64).clamp(0, n - 1) as usize];
    let p2 = DAILY_KI_CORRECTION[((i0 as i64) + 1).clamp(0, n - 1) as usize];
    let p3 = DAILY_KI_CORRECTION[((i0 as i64) + 2).clamp(0, n - 1) as usize];
    cr_dot(w, p0, p1, p2, p3)
}
