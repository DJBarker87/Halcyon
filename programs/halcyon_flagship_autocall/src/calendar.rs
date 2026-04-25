use anchor_lang::prelude::*;
use halcyon_common::HalcyonError;

use crate::state::SECONDS_PER_DAY;
use crate::state::TENOR_TRADING_DAYS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct CivilDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl CivilDate {
    pub const fn new(year: i32, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }

    pub fn add_days(self, days: i64) -> Self {
        civil_from_days(days_from_civil(self.year, self.month, self.day) + days)
    }

    pub fn weekday_monday_zero(self) -> u8 {
        let days = days_from_civil(self.year, self.month, self.day);
        (days + 3).rem_euclid(7) as u8
    }

    pub fn is_weekend(self) -> bool {
        matches!(self.weekday_monday_zero(), 5 | 6)
    }
}

pub fn issue_trade_date(issued_at: i64) -> CivilDate {
    let local_ts = issued_at.saturating_add(new_york_utc_offset_secs(issued_at));
    civil_from_days(local_ts.div_euclid(SECONDS_PER_DAY))
}

pub fn trading_close_timestamp_utc(date: CivilDate) -> i64 {
    let close_hour_utc = if is_new_york_dst_on_close(date) {
        20
    } else {
        21
    };
    days_from_civil(date.year, date.month, date.day)
        .saturating_mul(SECONDS_PER_DAY)
        .saturating_add(i64::from(close_hour_utc) * 3_600)
}

pub fn nth_trading_day_after(issue_date: CivilDate, nth: u16) -> Result<CivilDate> {
    require!(nth > 0, HalcyonError::Overflow);
    let mut remaining = nth;
    let mut cursor = issue_date;
    while remaining > 0 {
        cursor = cursor.add_days(1);
        if is_us_equity_trading_day(cursor) {
            remaining -= 1;
        }
    }
    Ok(cursor)
}

pub fn trading_days_elapsed_since_issue(issued_at: i64, now: i64) -> Result<u16> {
    if now <= issued_at {
        return Ok(0);
    }

    let issue_date = issue_trade_date(issued_at);
    let mut elapsed = 0u16;
    let mut cursor = issue_date;

    while elapsed < TENOR_TRADING_DAYS {
        cursor = cursor.add_days(1);
        if !is_us_equity_trading_day(cursor) {
            continue;
        }
        if trading_close_timestamp_utc(cursor) > now {
            break;
        }
        elapsed = elapsed.checked_add(1).ok_or(HalcyonError::Overflow)?;
    }

    Ok(elapsed)
}

pub fn is_us_equity_trading_day(date: CivilDate) -> bool {
    !date.is_weekend() && !is_nyse_holiday(date)
}

fn is_nyse_holiday(date: CivilDate) -> bool {
    let year = date.year;
    date == observed_fixed_holiday(year, 1, 1)
        || date == nth_weekday_of_month(year, 1, 0, 3)
        || date == nth_weekday_of_month(year, 2, 0, 3)
        || date == easter_sunday(year).add_days(-2)
        || date == last_weekday_of_month(year, 5, 0)
        || date == observed_fixed_holiday(year, 6, 19)
        || date == observed_fixed_holiday(year, 7, 4)
        || date == nth_weekday_of_month(year, 9, 0, 1)
        || date == nth_weekday_of_month(year, 11, 3, 4)
        || date == observed_fixed_holiday(year, 12, 25)
}

fn observed_fixed_holiday(year: i32, month: u8, day: u8) -> CivilDate {
    let holiday = CivilDate::new(year, month, day);
    match holiday.weekday_monday_zero() {
        5 => holiday.add_days(-1),
        6 => holiday.add_days(1),
        _ => holiday,
    }
}

fn nth_weekday_of_month(year: i32, month: u8, weekday_monday_zero: u8, nth: u8) -> CivilDate {
    let first = CivilDate::new(year, month, 1);
    let offset = (i16::from(weekday_monday_zero) - i16::from(first.weekday_monday_zero()))
        .rem_euclid(7) as i64;
    first.add_days(offset + i64::from(nth.saturating_sub(1)) * 7)
}

fn last_weekday_of_month(year: i32, month: u8, weekday_monday_zero: u8) -> CivilDate {
    let first_next_month = if month == 12 {
        CivilDate::new(year + 1, 1, 1)
    } else {
        CivilDate::new(year, month + 1, 1)
    };
    let mut cursor = first_next_month.add_days(-1);
    while cursor.weekday_monday_zero() != weekday_monday_zero {
        cursor = cursor.add_days(-1);
    }
    cursor
}

fn easter_sunday(year: i32) -> CivilDate {
    let a = year.rem_euclid(19);
    let b = year.div_euclid(100);
    let c = year.rem_euclid(100);
    let d = b.div_euclid(4);
    let e = b.rem_euclid(4);
    let f = (b + 8).div_euclid(25);
    let g = (b - f + 1).div_euclid(3);
    let h = (19 * a + b - d - g + 15).rem_euclid(30);
    let i = c.div_euclid(4);
    let k = c.rem_euclid(4);
    let l = (32 + 2 * e + 2 * i - h - k).rem_euclid(7);
    let m = (a + 11 * h + 22 * l).div_euclid(451);
    let month = (h + l - 7 * m + 114).div_euclid(31);
    let day = (h + l - 7 * m + 114).rem_euclid(31) + 1;
    CivilDate::new(year, month as u8, day as u8)
}

fn is_new_york_dst_on_close(date: CivilDate) -> bool {
    let start = nth_weekday_of_month(date.year, 3, 6, 2);
    let end = nth_weekday_of_month(date.year, 11, 6, 1);
    date.year == start.year && date.year == end.year && date >= start && date < end
}

fn new_york_utc_offset_secs(timestamp: i64) -> i64 {
    let year = civil_from_days(timestamp.div_euclid(SECONDS_PER_DAY)).year;
    let dst_start_utc = local_transition_utc(nth_weekday_of_month(year, 3, 6, 2), 7);
    let dst_end_utc = local_transition_utc(nth_weekday_of_month(year, 11, 6, 1), 6);
    if timestamp >= dst_start_utc && timestamp < dst_end_utc {
        -4 * 3_600
    } else {
        -5 * 3_600
    }
}

fn local_transition_utc(date: CivilDate, utc_hour: i64) -> i64 {
    days_from_civil(date.year, date.month, date.day)
        .saturating_mul(SECONDS_PER_DAY)
        .saturating_add(utc_hour * 3_600)
}

fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 }.div_euclid(400);
    let year_of_era = year - era * 400;
    let month = i32::from(month);
    let day = i32::from(day);
    let day_of_year = (153 * (month + if month > 2 { -3 } else { 9 }) + 2).div_euclid(5) + day - 1;
    let day_of_era =
        year_of_era * 365 + year_of_era.div_euclid(4) - year_of_era.div_euclid(100) + day_of_year;
    i64::from(era * 146_097 + day_of_era - 719_468)
}

fn civil_from_days(days: i64) -> CivilDate {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 }.div_euclid(146_097);
    let day_of_era = z - era * 146_097;
    let year_of_era = (day_of_era - day_of_era.div_euclid(1_460) + day_of_era.div_euclid(36_524)
        - day_of_era.div_euclid(146_096))
    .div_euclid(365);
    let year = year_of_era + era * 400;
    let day_of_year =
        day_of_era - (365 * year_of_era + year_of_era.div_euclid(4) - year_of_era.div_euclid(100));
    let month_param = (5 * day_of_year + 2).div_euclid(153);
    let day = day_of_year - (153 * month_param + 2).div_euclid(5) + 1;
    let month = month_param + if month_param < 10 { 3 } else { -9 };
    CivilDate::new(
        (year + i64::from(month <= 2)) as i32,
        month as u8,
        day as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_standard_holidays() {
        assert!(!is_us_equity_trading_day(CivilDate::new(2026, 7, 3)));
        assert!(!is_us_equity_trading_day(CivilDate::new(2026, 4, 3)));
        assert!(is_us_equity_trading_day(CivilDate::new(2026, 7, 6)));
    }

    #[test]
    fn trading_close_uses_dst() {
        assert_eq!(
            trading_close_timestamp_utc(CivilDate::new(2026, 1, 15)),
            days_from_civil(2026, 1, 15) * SECONDS_PER_DAY + 21 * 3_600
        );
        assert_eq!(
            trading_close_timestamp_utc(CivilDate::new(2026, 3, 17)),
            days_from_civil(2026, 3, 17) * SECONDS_PER_DAY + 20 * 3_600
        );
    }

    #[test]
    fn first_coupon_boundary_skips_holidays() {
        let issue = CivilDate::new(2026, 1, 2);
        assert_eq!(
            nth_trading_day_after(issue, 21).unwrap(),
            CivilDate::new(2026, 2, 3)
        );
    }

    #[test]
    fn first_coupon_boundary_carries_dst_forward() {
        let issue = CivilDate::new(2026, 2, 13);
        let first_coupon = nth_trading_day_after(issue, 21).unwrap();
        assert_eq!(first_coupon, CivilDate::new(2026, 3, 17));
        assert_eq!(
            trading_close_timestamp_utc(first_coupon),
            days_from_civil(2026, 3, 17) * SECONDS_PER_DAY + 20 * 3_600
        );
    }

    #[test]
    fn elapsed_trading_days_waits_for_close() {
        let issued_at = trading_close_timestamp_utc(CivilDate::new(2026, 1, 2));
        let before_first_close = trading_close_timestamp_utc(CivilDate::new(2026, 1, 5)) - 1;
        let at_first_close = trading_close_timestamp_utc(CivilDate::new(2026, 1, 5));

        assert_eq!(
            trading_days_elapsed_since_issue(issued_at, before_first_close).unwrap(),
            0
        );
        assert_eq!(
            trading_days_elapsed_since_issue(issued_at, at_first_close).unwrap(),
            1
        );
    }

    #[test]
    fn elapsed_trading_days_matches_monthly_boundary() {
        let issued_at = trading_close_timestamp_utc(CivilDate::new(2026, 2, 13));
        let first_coupon_close = trading_close_timestamp_utc(CivilDate::new(2026, 3, 17));

        assert_eq!(
            trading_days_elapsed_since_issue(issued_at, first_coupon_close).unwrap(),
            21
        );
    }
}
