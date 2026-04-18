// Gauss-Hermite quadrature constants (physicist convention).
//
// ∫ f(x) exp(-x²) dx ≈ Σ ω_k f(x_k)
//
// To integrate against the standard normal density:
//   (1/√π) Σ ω_k f(√2 · x_k)
//
// Nodes and weights from mpmath.gauss_hermite(10, 50), rounded to nearest
// integer at SCALE = 1e12.  Symmetric: node[k] = -node[9-k],
// weight[k] = weight[9-k].  Weights sum to √π · SCALE = 1_772_453_850_906.

/// 3-point Gauss-Hermite nodes at SCALE (physicist convention).
pub const GH3_NODES: [i128; 3] = [-1_224_744_871_392, 0, 1_224_744_871_392];

/// 3-point Gauss-Hermite weights at SCALE (physicist convention).
pub const GH3_WEIGHTS: [i128; 3] = [295_408_975_151, 1_181_635_900_604, 295_408_975_151];

/// 4-point Gauss-Hermite nodes at SCALE (physicist convention).
pub const GH4_NODES: [i128; 4] = [
    -1_650_680_123_886,
    -524_647_623_275,
    524_647_623_275,
    1_650_680_123_886,
];

/// 4-point Gauss-Hermite weights at SCALE (physicist convention).
pub const GH4_WEIGHTS: [i128; 4] = [
    81_312_835_447,
    804_914_090_006,
    804_914_090_006,
    81_312_835_447,
];

/// 5-point Gauss-Hermite nodes at SCALE (physicist convention).
pub const GH5_NODES: [i128; 5] = [
    -2_020_182_870_456,
    -958_572_464_614,
    0,
    958_572_464_614,
    2_020_182_870_456,
];

/// 5-point Gauss-Hermite weights at SCALE (physicist convention).
pub const GH5_WEIGHTS: [i128; 5] = [
    19_953_242_059,
    393_619_323_152,
    945_308_720_483,
    393_619_323_152,
    19_953_242_059,
];

/// 6-point Gauss-Hermite nodes at SCALE (physicist convention).
pub const GH6_NODES: [i128; 6] = [
    -2_350_604_973_674,
    -1_335_849_074_014,
    -436_077_411_928,
    436_077_411_928,
    1_335_849_074_014,
    2_350_604_973_674,
];

/// 6-point Gauss-Hermite weights at SCALE (physicist convention).
pub const GH6_WEIGHTS: [i128; 6] = [
    4_530_009_906,
    157_067_320_323,
    724_629_595_224,
    724_629_595_224,
    157_067_320_323,
    4_530_009_906,
];

/// 7-point Gauss-Hermite nodes at SCALE (physicist convention).
pub const GH7_NODES: [i128; 7] = [
    -2_651_961_356_835,
    -1_673_551_628_767,
    -816_287_882_859,
    0,
    816_287_882_859,
    1_673_551_628_767,
    2_651_961_356_835,
];

/// 7-point Gauss-Hermite weights at SCALE (physicist convention).
pub const GH7_WEIGHTS: [i128; 7] = [
    971_781_245,
    54_515_582_819,
    425_607_252_610,
    810_264_617_557,
    425_607_252_610,
    54_515_582_819,
    971_781_245,
];

/// 8-point Gauss-Hermite nodes at SCALE (physicist convention).
pub const GH8_NODES: [i128; 8] = [
    -2_930_637_420_257,
    -1_981_656_756_696,
    -1_157_193_712_447,
    -381_186_990_207,
    381_186_990_207,
    1_157_193_712_447,
    1_981_656_756_696,
    2_930_637_420_257,
];

/// 8-point Gauss-Hermite weights at SCALE (physicist convention).
pub const GH8_WEIGHTS: [i128; 8] = [
    199_604_072,
    17_077_983_007,
    207_802_325_815,
    661_147_012_558,
    661_147_012_558,
    207_802_325_815,
    17_077_983_007,
    199_604_072,
];

/// 9-point Gauss-Hermite nodes at SCALE (physicist convention).
pub const GH9_NODES: [i128; 9] = [
    -3_190_993_201_782,
    -2_266_580_584_532,
    -1_468_553_289_217,
    -723_551_018_753,
    0,
    723_551_018_753,
    1_468_553_289_217,
    2_266_580_584_532,
    3_190_993_201_782,
];

/// 9-point Gauss-Hermite weights at SCALE (physicist convention).
pub const GH9_WEIGHTS: [i128; 9] = [
    39_606_977,
    4_943_624_276,
    88_474_527_394,
    432_651_559_003,
    720_235_215_606,
    432_651_559_003,
    88_474_527_394,
    4_943_624_276,
    39_606_977,
];

/// 10-point Gauss-Hermite nodes at SCALE (physicist convention).
///
/// Symmetric about zero: `GH10_NODES[k] == -GH10_NODES[9 - k]`.
pub const GH10_NODES: [i128; 10] = [
    -3_436_159_118_838, // x_1 = -3.436159118837737603327…
    -2_532_731_674_233, // x_2 = -2.532731674232789839987…
    -1_756_683_649_300, // x_3 = -1.756683649299881773451…
    -1_036_610_829_790, // x_4 = -1.036610829789513654178…
    -342_901_327_224,   // x_5 = -0.342901327223704608789…
    342_901_327_224,    // x_6 = +0.342901327223704608789…
    1_036_610_829_790,  // x_7 = +1.036610829789513654178…
    1_756_683_649_300,  // x_8 = +1.756683649299881773451…
    2_532_731_674_233,  // x_9 = +2.532731674232789839987…
    3_436_159_118_838,  // x_10= +3.436159118837737603327…
];

/// 10-point Gauss-Hermite weights at SCALE (physicist convention).
///
/// Symmetric: `GH10_WEIGHTS[k] == GH10_WEIGHTS[9 - k]`.
/// Sum = √π · SCALE = 1_772_453_850_906.
pub const GH10_WEIGHTS: [i128; 10] = [
    7_640_433,       // ω_1  = 0.000007640432855232621…
    1_343_645_747,   // ω_2  = 0.001343645746781232692…
    33_874_394_455,  // ω_3  = 0.033874394455481063085…
    240_138_611_082, // ω_4  = 0.240138611082314686417…
    610_862_633_735, // ω_5  = 0.610862633735325798784…
    610_862_633_735, // ω_6  = 0.610862633735325798784…
    240_138_611_082, // ω_7  = 0.240138611082314686417…
    33_874_394_455,  // ω_8  = 0.033874394455481063085…
    1_343_645_747,   // ω_9  = 0.001343645746781232692…
    7_640_433,       // ω_10 = 0.000007640432855232621…
];

/// 13-point Gauss-Hermite nodes at SCALE (physicist convention).
pub const GH13_NODES: [i128; 13] = [
    -4_101_337_596_179,
    -3_246_608_978_372,
    -2_519_735_685_678,
    -1_853_107_651_602,
    -1_220_055_036_591,
    -605_763_879_171,
    0,
    605_763_879_171,
    1_220_055_036_591,
    1_853_107_651_602,
    2_519_735_685_678,
    3_246_608_978_372,
    4_101_337_596_179,
];

/// 13-point Gauss-Hermite weights at SCALE (physicist convention).
pub const GH13_WEIGHTS: [i128; 13] = [
    48_257,
    20_430_360,
    1_207_459_993,
    20_862_775_296,
    140_323_320_687,
    421_616_296_899,
    604_393_187_921,
    421_616_296_899,
    140_323_320_687,
    20_862_775_296,
    1_207_459_993,
    20_430_360,
    48_257,
];

/// 1/√π at SCALE = 564_189_583_548.
pub const INV_SQRT_PI: i128 = 564_189_583_548;

/// 5-point Gauss-Legendre nodes on [-1, 1] at SCALE.
pub const GL5_NODES: [i128; 5] = [
    -906_179_845_939, // -0.906179845938663964
    -538_469_310_106, // -0.538469310105683108
    0,
    538_469_310_106, //  0.538469310105683108
    906_179_845_939, //  0.906179845938663964
];

/// 5-point Gauss-Legendre weights on [-1, 1] at SCALE.  Sum = 2·SCALE.
pub const GL5_WEIGHTS: [i128; 5] = [
    236_926_885_056, // 0.236926885056189390
    478_628_670_499, // 0.478628670499366249
    568_888_888_889, // 0.568888888888888555
    478_628_670_499, // 0.478628670499366249
    236_926_885_056, // 0.236926885056189390
];

/// 7-point Gauss-Legendre nodes on [-1, 1] at SCALE.
pub const GL7_NODES: [i128; 7] = [
    -949_107_912_343, // -0.949107912342758486
    -741_531_185_599, // -0.741531185599394460
    -405_845_151_377, // -0.405845151377397184
    0,
    405_845_151_377, //  0.405845151377397184
    741_531_185_599, //  0.741531185599394460
    949_107_912_343, //  0.949107912342758486
];

/// 7-point Gauss-Legendre weights on [-1, 1] at SCALE.  Sum = 2·SCALE.
pub const GL7_WEIGHTS: [i128; 7] = [
    129_484_966_169, // 0.129484966168870647
    279_705_391_489, // 0.279705391489276589
    381_830_050_505, // 0.381830050505118479
    417_959_183_673, // 0.417959183673468848
    381_830_050_505, // 0.381830050505118479
    279_705_391_489, // 0.279705391489276589
    129_484_966_169, // 0.129484966168870647
];

/// Return a supported Gauss-Hermite rule by order.
///
/// Supported orders are 3, 4, 5, 6, 7, 8, 9, 10, and 13. All arrays use the physicist
/// convention and SCALE = 1e12.
pub(crate) fn gh_rule(order: usize) -> Option<(&'static [i128], &'static [i128])> {
    match order {
        3 => Some((&GH3_NODES, &GH3_WEIGHTS)),
        4 => Some((&GH4_NODES, &GH4_WEIGHTS)),
        5 => Some((&GH5_NODES, &GH5_WEIGHTS)),
        6 => Some((&GH6_NODES, &GH6_WEIGHTS)),
        7 => Some((&GH7_NODES, &GH7_WEIGHTS)),
        8 => Some((&GH8_NODES, &GH8_WEIGHTS)),
        9 => Some((&GH9_NODES, &GH9_WEIGHTS)),
        10 => Some((&GH10_NODES, &GH10_WEIGHTS)),
        13 => Some((&GH13_NODES, &GH13_WEIGHTS)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_sqrt_pi() {
        let sum: i128 = GH10_WEIGHTS.iter().sum();
        // √π · SCALE = 1_772_453_850_906 (from mpmath)
        let sqrt_pi_scale: i128 = 1_772_453_850_906;
        let diff = (sum - sqrt_pi_scale).abs();
        // Allow ±1 from rounding of 10 weights
        assert!(
            diff <= 10,
            "weight sum {sum} vs √π·SCALE {sqrt_pi_scale}, diff {diff}"
        );
    }

    #[test]
    fn nodes_symmetric() {
        for k in 0..5 {
            assert_eq!(
                GH10_NODES[k],
                -GH10_NODES[9 - k],
                "node symmetry failed at k={k}"
            );
        }
    }

    #[test]
    fn weights_symmetric() {
        for k in 0..5 {
            assert_eq!(
                GH10_WEIGHTS[k],
                GH10_WEIGHTS[9 - k],
                "weight symmetry failed at k={k}"
            );
        }
    }

    #[test]
    fn quadrature_x_squared() {
        // ∫ x² exp(-x²) dx = √π/2.  GH10 is exact for degree ≤ 19.
        let scale = 1_000_000_000_000i128;
        let mut sum: i128 = 0;
        for k in 0..10 {
            let xk = GH10_NODES[k];
            // x² at SCALE: xk * xk / SCALE
            let x2 = xk * xk / scale;
            sum += GH10_WEIGHTS[k] * x2 / scale;
        }
        // Expected: √π/2 · SCALE = 886_226_925_453
        let expected: i128 = 886_226_925_453;
        let diff = (sum - expected).abs();
        assert!(
            diff <= 10,
            "∫x²exp(-x²)dx = {sum}, expected {expected}, diff {diff}"
        );
    }

    #[test]
    fn quadrature_x_fourth() {
        // ∫ x⁴ exp(-x²) dx = 3√π/4.  GH10 is exact for degree ≤ 19.
        let scale = 1_000_000_000_000i128;
        let mut sum: i128 = 0;
        for k in 0..10 {
            let xk = GH10_NODES[k];
            let x2 = xk * xk / scale;
            let x4 = x2 * x2 / scale;
            sum += GH10_WEIGHTS[k] * x4 / scale;
        }
        // Expected: 3√π/4 · SCALE = 1_329_340_388_179
        let expected: i128 = 1_329_340_388_179;
        let diff = (sum - expected).abs();
        assert!(
            diff <= 50,
            "∫x⁴exp(-x²)dx = {sum}, expected {expected}, diff {diff}"
        );
    }

    #[test]
    fn gh_rule_exposes_supported_orders() {
        for &order in &[3usize, 4, 5, 6, 7, 8, 9, 10, 13] {
            let (nodes, weights) = gh_rule(order).expect("supported order");
            assert_eq!(nodes.len(), order);
            assert_eq!(weights.len(), order);
        }
        assert!(gh_rule(11).is_none());
    }
}
