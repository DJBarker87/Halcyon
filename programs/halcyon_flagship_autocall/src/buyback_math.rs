use anchor_lang::prelude::*;
use halcyon_common::HalcyonError;

pub const BUYBACK_HAIRCUT_S6: i64 = 100_000;
pub const RETAIL_REDEMPTION_HAIRCUT_S6: i64 = 50_000;
pub const SCALE_S6: i64 = 1_000_000;

pub fn discounted_value_s6(nav_s6: i64, ki_level_s6: i64, haircut_s6: i64) -> i64 {
    let nav_less_haircut = nav_s6.saturating_sub(haircut_s6);
    let ki_less_haircut = ki_level_s6.saturating_sub(haircut_s6);
    nav_less_haircut.min(ki_less_haircut).max(0)
}

pub fn lending_value_s6(nav_s6: i64, ki_level_s6: i64) -> i64 {
    discounted_value_s6(nav_s6, ki_level_s6, BUYBACK_HAIRCUT_S6)
}

pub fn retail_redemption_value_s6(nav_s6: i64, ki_level_s6: i64) -> i64 {
    discounted_value_s6(nav_s6, ki_level_s6, RETAIL_REDEMPTION_HAIRCUT_S6)
}

pub fn lending_value_payout_usdc(notional_usdc: u64, lending_value_s6: i64) -> Result<u64> {
    if lending_value_s6 <= 0 {
        return Ok(0);
    }
    let payout = (notional_usdc as u128)
        .checked_mul(lending_value_s6 as u128)
        .ok_or(HalcyonError::Overflow)?
        .checked_div(SCALE_S6 as u128)
        .ok_or(HalcyonError::Overflow)?;
    u64::try_from(payout).map_err(|_| error!(HalcyonError::Overflow))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lending_value_caps_healthy_nav_at_ki_minus_haircut() {
        assert_eq!(lending_value_s6(970_000, 800_000), 700_000);
    }

    #[test]
    fn lending_value_follows_stressed_nav_down() {
        assert_eq!(lending_value_s6(580_000, 800_000), 480_000);
    }

    #[test]
    fn lending_value_clamps_below_zero() {
        assert_eq!(lending_value_s6(50_000, 800_000), 0);
    }

    #[test]
    fn lending_value_payout_scales_notional() {
        assert_eq!(
            lending_value_payout_usdc(100_000_000, 700_000).unwrap(),
            70_000_000
        );
    }

    #[test]
    fn retail_redemption_uses_tighter_notice_haircut() {
        assert_eq!(retail_redemption_value_s6(970_000, 800_000), 750_000);
    }
}
