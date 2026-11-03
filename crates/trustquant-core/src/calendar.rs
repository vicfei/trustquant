//! Calendars and schedules (M2, EUR use case). Semantics are pinned by the
//! `calendar` fixtures generated from QuantLib 1.33 TARGET; the Rust side
//! mirrors the DUMPED dates, not a memory of QuantLib internals.
//!
//! Scope: TARGET (EUR market) + WeekendsOnly + NullCalendar; conventions
//! Following / ModifiedFollowing / Preceding / Unadjusted; schedules with
//! the Forward date-generation rule. Other calendars arrive with the use
//! cases that need them (per-ADR convergence), each fixture-pinned.

use crate::contract::ErrorEnvelope;
use crate::date::{Date, Weekday};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CalendarId {
    Target,
    WeekendsOnly,
    NullCalendar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum BusinessDayConvention {
    Following,
    ModifiedFollowing,
    Preceding,
    Unadjusted,
}

/// BusinessDayConvention names as they appear on the wire.
pub fn parse_convention(s: &str) -> Result<BusinessDayConvention, ErrorEnvelope> {
    match s {
        "Following" | "following" => Ok(BusinessDayConvention::Following),
        "ModifiedFollowing" | "modified_following" => Ok(BusinessDayConvention::ModifiedFollowing),
        "Preceding" | "preceding" => Ok(BusinessDayConvention::Preceding),
        "Unadjusted" | "unadjusted" => Ok(BusinessDayConvention::Unadjusted),
        other => Err(ErrorEnvelope::invalid_input(
            format!("unknown business day convention '{other}'"),
            "convention",
        )),
    }
}

/// Anonymous Gregorian algorithm for Easter Sunday (valid for the supported
/// 1901–2199 year range).
pub fn easter_sunday(year: i32) -> Date {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = ((h + l - 7 * m + 114) % 31) + 1;
    Date::from_ymd(year, month as u32, day as u32).expect("easter algorithm yields a valid date")
}

pub fn is_business_day(cal: CalendarId, d: Date) -> bool {
    let (_, m, day) = d.ymd();
    match cal {
        CalendarId::NullCalendar => true,
        CalendarId::WeekendsOnly | CalendarId::Target => {
            if matches!(d.weekday(), Weekday::Sat | Weekday::Sun) {
                return false;
            }
            if cal == CalendarId::WeekendsOnly {
                return true;
            }
            // TARGET fixed holidays (post-2002 rules; fixture coverage 2023-2026)
            if (m == 1 && day == 1) || (m == 5 && day == 1) || (m == 12 && (day == 25 || day == 26))
            {
                return false;
            }
            let easter = easter_sunday(d.year());
            let good_friday = easter.add_days(-2).expect("easter-2 in range");
            let easter_monday = easter.add_days(1).expect("easter+1 in range");
            d != good_friday && d != easter_monday
        }
    }
}

pub fn adjust(cal: CalendarId, d: Date, conv: BusinessDayConvention) -> Date {
    if is_business_day(cal, d) {
        return d;
    }
    match conv {
        BusinessDayConvention::Unadjusted => d,
        BusinessDayConvention::Following => {
            let mut r = d;
            while !is_business_day(cal, r) {
                r = r.add_days(1).expect("adjust stays in supported range");
            }
            r
        }
        BusinessDayConvention::Preceding => {
            let mut r = d;
            while !is_business_day(cal, r) {
                r = r.add_days(-1).expect("adjust stays in supported range");
            }
            r
        }
        BusinessDayConvention::ModifiedFollowing => {
            let mut r = d;
            while !is_business_day(cal, r) {
                r = r.add_days(1).expect("adjust stays in supported range");
            }
            if r.month() != d.month() {
                adjust(cal, d, BusinessDayConvention::Preceding)
            } else {
                r
            }
        }
    }
}

/// Advance by whole months then adjust (schedule-building primitive; matches
/// QuantLib Calendar::advance with a month count: month arithmetic with
/// end-of-month clamping, then convention adjustment).
pub fn advance_months(
    cal: CalendarId,
    d: Date,
    months: i32,
    conv: BusinessDayConvention,
) -> Result<Date, ErrorEnvelope> {
    let unadjusted = crate::date::add_months_signed(d, months)
        .map_err(|e| ErrorEnvelope::invalid_input(e, "date"))?;
    Ok(adjust(cal, unadjusted, conv))
}

/// Spot date: n TARGET business days after the evaluation date (Following).
pub fn spot_date(eval_date: Date, days: i32) -> Result<Date, ErrorEnvelope> {
    let mut d = eval_date;
    let mut remaining = days;
    while remaining > 0 {
        d = d
            .add_days(1)
            .map_err(|e| ErrorEnvelope::invalid_input(e, "eval_date"))?;
        if is_business_day(CalendarId::Target, d) {
            remaining -= 1;
        }
    }
    Ok(d)
}

// ---------------- Schedule (Forward rule) ----------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frequency {
    Annual,
    Semiannual,
    Quarterly,
}

pub fn parse_frequency(s: &str) -> Result<Frequency, ErrorEnvelope> {
    match s {
        "annual" | "Annual" | "1Y" => Ok(Frequency::Annual),
        "semiannual" | "Semiannual" | "6M" => Ok(Frequency::Semiannual),
        "quarterly" | "Quarterly" | "3M" => Ok(Frequency::Quarterly),
        other => Err(ErrorEnvelope::invalid_input(
            format!("unsupported frequency '{other}' (annual|semiannual|quarterly)"),
            "frequency",
        )),
    }
}

impl Frequency {
    fn months(self) -> u32 {
        match self {
            Frequency::Annual => 12,
            Frequency::Semiannual => 6,
            Frequency::Quarterly => 3,
        }
    }
}

/// Generate a Forward-rule schedule: unadjusted periods from start to the
/// unadjusted maturity, then each date adjusted with the given convention
/// (QuantLib Schedule semantics for Forward + explicit termination date).
/// Returns the adjusted payment dates INCLUDING the start date as [0].
pub fn schedule(
    cal: CalendarId,
    start: Date,
    maturity_unadjusted: Date,
    freq: Frequency,
    conv: BusinessDayConvention,
) -> Result<Vec<Date>, ErrorEnvelope> {
    if maturity_unadjusted <= start {
        return Err(ErrorEnvelope::invalid_input(
            "maturity must be after start".to_string(),
            "maturity_date",
        ));
    }
    let step = freq.months() as i32;
    let mut dates = vec![adjust(cal, start, conv)];
    let mut anchor = start;
    loop {
        let candidate =
            crate::date::add_months_signed(anchor, step).map_err(ErrorEnvelope::internal)?;
        if candidate >= maturity_unadjusted {
            break;
        }
        dates.push(adjust(cal, candidate, conv));
        anchor = candidate;
    }
    dates.push(adjust(cal, maturity_unadjusted, conv));
    Ok(dates)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easter_anchor_years() {
        // pinned by the calendar fixtures (Good Friday column)
        assert_eq!(easter_sunday(2023).to_iso(), "2023-04-09");
        assert_eq!(easter_sunday(2024).to_iso(), "2024-03-31");
        assert_eq!(easter_sunday(2025).to_iso(), "2025-04-20");
        assert_eq!(easter_sunday(2026).to_iso(), "2026-04-05");
    }

    #[test]
    fn spot_is_two_business_days() {
        // 2023-06-21 (Wed) + 2 TARGET days = 2023-06-23 (Fri)
        let d = Date::parse_iso("2023-06-21").unwrap();
        assert_eq!(spot_date(d, 2).unwrap().to_iso(), "2023-06-23");
        // 2023-06-22 (Thu) + 2 = 2023-06-26 (Mon, weekend skipped)
        let d = Date::parse_iso("2023-06-22").unwrap();
        assert_eq!(spot_date(d, 2).unwrap().to_iso(), "2023-06-26");
    }
}
