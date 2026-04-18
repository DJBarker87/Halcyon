//! Daily-KI cadence correction table.
//!
//! correction[i] = sensitivity(σ_i) × cos3d_delta(σ_i)  at micro-bps scale.
//! Closes the gap between the c1 filter's obs-date-only KI monitoring and
//! the product's actual daily-KI monitoring (see product_economics/
//! worst_of_math_stack.md §5).
//!
//! cos3d source: method="mc_bridge"  (MC bridge now; COS later — swap
//! is a one-line change to COS3D_PATH in generate_daily_ki_correction.py).
//! Sensitivity: 40 σ anchors, 100000 MC path-pairs/mode.
//!
//! Storage: 256 × i64 micro-bps (1 unit = 1e-6 bps) = 2,048 bytes.
//! Lookup: Catmull-Rom on σ, ~150 CU. Apply by adding the result (divided
//! by 1_000_000) to the filter's fair_coupon_bps after k12_correction.
//!
//! Content SHA-256: 34c7f25be74cbfc7b1a495fb188a113d65d50e1f1a921d34420f63fe83e0333e
//!
//! Regenerate: `python3 halcyon-hedge-lab/cos3d/scripts/generate_daily_ki_correction.py`

const N: usize = 256;
const SIGMA_MIN_S6: i64 = 80000;
const SIGMA_MAX_S6: i64 = 800000;
const SIGMA_RANGE_S6: i64 = SIGMA_MAX_S6 - SIGMA_MIN_S6;
const N1: i64 = (N - 1) as i64;

const DAILY_KI_CORRECTION: [i64; N] = [
    32385338, 33469536, 36206890, 35560924, 40553324, 37353144, 40684715, 41740693, 
    44087231, 46427102, 48002816, 44751570, 46485163, 48167518, 51706836, 51489157, 
    49924886, 52696358, 56194869, 58834785, 60567343, 56931661, 61855149, 59171789, 
    60688877, 59615306, 60791718, 62519000, 59282842, 63644858, 66710908, 64102017, 
    63210454, 65791241, 64228002, 64862640, 66877288, 65915169, 62824511, 67880414, 
    64914531, 61425770, 64832508, 65324216, 64315952, 64936495, 67661155, 67129679, 
    65166092, 67503412, 66151779, 63091757, 66349417, 62046823, 61967074, 63023197, 
    64303158, 61080867, 62133907, 58948411, 59569423, 58155447, 59558634, 59058684, 
    57530934, 60391181, 54386200, 54343404, 54413243, 53751892, 57542496, 51775975, 
    53155897, 49515033, 51347583, 50421312, 51713909, 50559022, 47559016, 46998283, 
    46541476, 45894194, 47792666, 44710742, 44588854, 43777144, 43707204, 41196994, 
    40937233, 42142394, 41425144, 39469882, 37505406, 39217355, 35655591, 35675053, 
    35974184, 34836518, 33980244, 33541746, 32450506, 33363653, 32319837, 29639452, 
    30190428, 29986737, 27645511, 28061686, 26501900, 26995844, 26017804, 25400029, 
    24834774, 23397210, 23602718, 22986220, 21170812, 21857003, 20755730, 19775386, 
    19654198, 19789380, 18162252, 17809094, 17182293, 17101551, 16332556, 15762044, 
    16073412, 15628981, 14993848, 14096357, 13482157, 13249241, 12740813, 12920498, 
    12040575, 11661092, 11659589, 11171174, 11365887, 10417341, 9939608, 10167325, 
    9519360, 9289291, 8894659, 8376478, 8238605, 8109458, 7633471, 7651020, 
    7234119, 7127252, 6965436, 6621507, 6610662, 6364801, 6532330, 6110469, 
    5890057, 5860865, 5799587, 5633219, 5444785, 5406574, 5271456, 5211560, 
    4953745, 5077926, 4915297, 4881325, 4801189, 4971394, 4876593, 4854692, 
    4720496, 4665512, 4772073, 4847342, 4843330, 4759261, 4888418, 4947394, 
    4877135, 5004566, 5015490, 5092499, 5175559, 5164170, 5467636, 5379004, 
    5450388, 5197386, 5504510, 5468384, 5740750, 5643013, 5619591, 5967737, 
    5904406, 5855889, 6130285, 6139354, 6179327, 5998151, 6132541, 6274213, 
    6435190, 6606873, 6475717, 6657725, 6694613, 7031269, 6832068, 6924976, 
    7051239, 6734255, 6753676, 6873584, 7226106, 6962746, 7203291, 7022084, 
    6945475, 6867665, 6997940, 7002205, 6878200, 6813936, 6774659, 6538656, 
    6488312, 6512031, 6221575, 6260813, 6121090, 5953539, 5755268, 5630118, 
    5100321, 5059545, 4919653, 4668326, 4510282, 4073396, 3822580, 3502743, 
    3094796, 2691540, 2357222, 1984918, 1553625, 1149443, 652435, 173850
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
