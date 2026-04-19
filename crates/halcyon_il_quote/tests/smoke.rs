use halcyon_il_quote::insurance::european_nig::nig_european_il_premium;
use halcyon_il_quote::insurance::settlement::compute_settlement_from_prices;

const SCALE_6_F64: f64 = 1_000_000.0;
const SCALE_12_U64: u64 = 1_000_000_000_000;
const SCALE_12_U128: u128 = 1_000_000_000_000;

fn s6(value: f64) -> i64 {
    (value * SCALE_6_F64).round() as i64
}

#[test]
fn il_product_smoke() {
    let premium_frac =
        nig_european_il_premium(s6(1.0), 30, s6(0.01), s6(0.07), s6(3.1401), s6(1.2139))
            .expect("nig premium") as f64
            / SCALE_6_F64;
    assert!(premium_frac > 0.0);

    let (terminal_il, payout_raw) = compute_settlement_from_prices(
        SCALE_12_U64 / 2,
        300 * SCALE_12_U128,
        SCALE_12_U128,
        150 * SCALE_12_U128,
        SCALE_12_U128,
        10_000_000_000,
        (0.01 * SCALE_12_U64 as f64).round() as u64,
        (0.07 * SCALE_12_U64 as f64).round() as u64,
    )
    .expect("settlement");

    assert!(terminal_il > 0);
    assert_eq!(payout_raw, 600_000_000);
}
