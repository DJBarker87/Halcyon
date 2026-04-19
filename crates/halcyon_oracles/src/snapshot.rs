use anchor_lang::prelude::*;

use crate::errors::OracleError;

/// Normalised Pyth price at SCALE_6 with publish metadata. Produced by every
/// backend and consumed directly by product handlers + pricers.
///
/// `price_s6` and `conf_s6` are already scaled to 1e-6 regardless of the raw
/// Pyth exponent, so callers never touch `expo`. `expo` is retained for
/// diagnostics and event emission.
#[derive(AnchorSerialize, AnchorDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PriceSnapshot {
    pub price_s6: i64,
    pub conf_s6: i64,
    pub publish_slot: u64,
    pub publish_ts: i64,
    pub expo: i32,
}

impl PriceSnapshot {
    /// Convert a Pyth-native `(price, conf, exponent)` triple into the SCALE_6
    /// representation Halcyon's quote crates consume.
    ///
    /// Pyth exponents for SOL/USD and the equity feeds sit in [-12, -4]; we
    /// accept [-18, 0] to give room for future feeds without rejecting valid
    /// inputs. Outside that window → `OracleError::ExponentOutOfRange`.
    pub fn from_raw(
        price: i64,
        conf: u64,
        expo: i32,
        publish_ts: i64,
        publish_slot: u64,
    ) -> Result<Self> {
        let price_s6 = rescale_to_s6(price, expo)?;
        let conf_signed = i64::try_from(conf).map_err(|_| error!(OracleError::ScaleOverflow))?;
        let conf_s6 = rescale_to_s6(conf_signed, expo)?;
        Ok(Self {
            price_s6,
            conf_s6,
            publish_slot,
            publish_ts,
            expo,
        })
    }
}

/// Rescale an `i64` value with a Pyth-style negative exponent to SCALE_6.
///
/// `value * 10^(expo + 6)`. Cases:
///   expo = -6 → identity
///   expo = -8 → divide by 100
///   expo = -4 → multiply by 100
///
/// Division truncates toward zero (Pyth `conf` is always non-negative so this
/// matches Pyth's own rounding for the signed channel too).
fn rescale_to_s6(value: i64, expo: i32) -> Result<i64> {
    let shift = expo
        .checked_add(6)
        .ok_or(error!(OracleError::ExponentOutOfRange))?;
    if !(-18..=6).contains(&expo) {
        return err!(OracleError::ExponentOutOfRange);
    }
    if shift == 0 {
        return Ok(value);
    }
    if shift > 0 {
        let factor = pow10_i64(shift as u32)?;
        return value
            .checked_mul(factor)
            .ok_or_else(|| error!(OracleError::ScaleOverflow));
    }
    let factor = pow10_i64((-shift) as u32)?;
    Ok(value / factor)
}

fn pow10_i64(n: u32) -> Result<i64> {
    let mut out: i64 = 1;
    for _ in 0..n {
        out = out
            .checked_mul(10)
            .ok_or_else(|| error!(OracleError::ScaleOverflow))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rescale_identity() {
        assert_eq!(rescale_to_s6(1_234_567, -6).unwrap(), 1_234_567);
    }

    #[test]
    fn rescale_neg_eight() {
        // SOL at $135.12345678 reported as 13_512_345_678 × 10^-8.
        // Expected SCALE_6: 135_123_456.
        assert_eq!(rescale_to_s6(13_512_345_678, -8).unwrap(), 135_123_456);
    }

    #[test]
    fn rescale_neg_four() {
        // Value reported as 1_234 × 10^-4. Expected SCALE_6: 123_400.
        assert_eq!(rescale_to_s6(1_234, -4).unwrap(), 123_400);
    }

    #[test]
    fn rescale_exponent_out_of_range_rejects() {
        assert!(rescale_to_s6(1, -19).is_err());
        assert!(rescale_to_s6(1, 7).is_err());
    }

    #[test]
    fn snapshot_from_raw_sol_usd() {
        let snap =
            PriceSnapshot::from_raw(13_512_345_678, 12_000_000, -8, 1_700_000_000, 300).unwrap();
        assert_eq!(snap.price_s6, 135_123_456);
        assert_eq!(snap.conf_s6, 120_000);
        assert_eq!(snap.expo, -8);
        assert_eq!(snap.publish_slot, 300);
    }
}
