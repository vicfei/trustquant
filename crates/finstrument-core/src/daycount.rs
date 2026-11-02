//! Day counters, closed versioned enum (ADR-0002). Semantics pinned by
//! QuantLib fixtures; note two subtleties the fixtures arbitrate:
//! - Thirty360USA applies the end-of-February rule to d1 AFTER the d2 cap
//!   check (dc_30u_0003/0005);
//! - ActualActualISDA cross-year = (Dec31-d1+1)/D1 + (d2-Jan1)/D2 + full
//!   years (inclusive first stub, exclusive second stub).

use crate::contract::ErrorEnvelope;
use crate::date::{Date, add_months, is_leap_year};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DayCounterId {
    Actual360,
    Actual365Fixed,
    Thirty360USA,
    Thirty360European,
    ActualActualISDA,
    ActualActualISMA,
}

/// Recognized names that are intentionally NOT supported in M0. Everything
/// else unknown is an input error, not a capability statement.
pub const UNSUPPORTED_COUNTERS: &[&str] = &["Business252", "Simple", "One", "Actual364"];

pub fn parse_counter(s: &str) -> Result<DayCounterId, ErrorEnvelope> {
    use DayCounterId::*;
    let id = match s {
        "Actual360" => Actual360,
        "Actual365Fixed" => Actual365Fixed,
        "Thirty360USA" => Thirty360USA,
        "Thirty360European" => Thirty360European,
        "ActualActualISDA" => ActualActualISDA,
        "ActualActualISMA" => ActualActualISMA,
        other => {
            return if UNSUPPORTED_COUNTERS.contains(&other) {
                Err(ErrorEnvelope::unsupported_feature(format!(
                    "day counter '{other}' is recognized but not supported in this version"
                )))
            } else {
                Err(ErrorEnvelope::invalid_input(
                    format!("unknown day counter '{other}'"),
                    "day_counter",
                ))
            };
        }
    };
    Ok(id)
}

pub struct DayCountRequest {
    pub counter: DayCounterId,
    pub start: Date,
    pub end: Date,
    /// Required for ActualActualISMA.
    pub ref_period: Option<(Date, Date)>,
}

pub fn year_fraction(req: &DayCountRequest) -> Result<f64, ErrorEnvelope> {
    if req.start > req.end {
        return Err(ErrorEnvelope::invalid_input(
            "start must be <= end (negative periods are rejected in M0)".to_string(),
            "start",
        ));
    }
    match req.counter {
        DayCounterId::Actual360 => Ok(f64::from(req.end.days_since(req.start)) / 360.0),
        DayCounterId::Actual365Fixed => Ok(f64::from(req.end.days_since(req.start)) / 365.0),
        DayCounterId::Thirty360USA => {
            let (y1, m1, d1) = req.start.ymd();
            let (y2, m2, d2) = req.end.ymd();
            let (mut dd1, mut dd2) = (d1 as i32, d2 as i32);
            if dd1 == 31 {
                dd1 = 30;
            }
            if dd2 == 31 && dd1 == 30 {
                dd2 = 30;
            }
            if req.start.is_last_day_of_feb() {
                dd1 = 30;
            }
            let days = 360 * (y2 - y1) + 30 * (m2 as i32 - m1 as i32) + (dd2 - dd1);
            Ok(f64::from(days) / 360.0)
        }
        DayCounterId::Thirty360European => {
            let (y1, m1, d1) = req.start.ymd();
            let (y2, m2, d2) = req.end.ymd();
            let dd1 = if d1 == 31 { 30 } else { d1 as i32 };
            let dd2 = if d2 == 31 { 30 } else { d2 as i32 };
            let days = 360 * (y2 - y1) + 30 * (m2 as i32 - m1 as i32) + (dd2 - dd1);
            Ok(f64::from(days) / 360.0)
        }
        DayCounterId::ActualActualISDA => {
            let (y1, _, _) = req.start.ymd();
            let (y2, _, _) = req.end.ymd();
            let d1 = if is_leap_year(y1) { 366.0 } else { 365.0 };
            let d2 = if is_leap_year(y2) { 366.0 } else { 365.0 };
            if y1 == y2 {
                return Ok(f64::from(req.end.days_since(req.start)) / d1);
            }
            let dec31 = Date::from_ymd(y1, 12, 31)
                .map_err(|e| ErrorEnvelope::internal(format!("calendar error: {e}")))?;
            let jan1 = Date::from_ymd(y2, 1, 1)
                .map_err(|e| ErrorEnvelope::internal(format!("calendar error: {e}")))?;
            let first = f64::from(dec31.days_since(req.start) + 1) / d1;
            let last = f64::from(req.end.days_since(jan1)) / d2;
            Ok(first + last + f64::from(y2 - y1 - 1))
        }
        DayCounterId::ActualActualISMA => {
            let Some((ref_start, ref_end)) = req.ref_period else {
                return Err(ErrorEnvelope::invalid_input(
                    "ActualActualISMA requires ref_period_start and ref_period_end".to_string(),
                    "ref_period_start",
                ));
            };
            if ref_start >= ref_end {
                return Err(ErrorEnvelope::invalid_input(
                    "ref_period_start must be < ref_period_end".to_string(),
                    "ref_period_start",
                ));
            }
            if req.start == req.end {
                return Ok(0.0);
            }
            let ref_days = ref_end.days_since(ref_start);
            let months = ((12.0 * f64::from(ref_days)) / 365.0).round() as u32;
            if months == 0 {
                return Err(ErrorEnvelope::invalid_input(
                    "reference period shorter than one month is not supported".to_string(),
                    "ref_period_end",
                ));
            }
            let mut anchor = req.start;
            let mut whole_months: u32 = 0;
            loop {
                let next = add_months(anchor, months)
                    .map_err(|e| ErrorEnvelope::invalid_input(e, "end"))?;
                if next <= req.end {
                    anchor = next;
                    whole_months += months;
                } else {
                    break;
                }
            }
            let rem = req.end.days_since(anchor);
            let whole_years = f64::from(whole_months) / 12.0;
            Ok(whole_years + f64::from(rem) * (f64::from(months) / 12.0) / f64::from(ref_days))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        Date::parse_iso(s).unwrap()
    }

    fn yf(counter: DayCounterId, s: &str, e: &str, refp: Option<(&str, &str)>) -> f64 {
        year_fraction(&DayCountRequest {
            counter,
            start: d(s),
            end: d(e),
            ref_period: refp.map(|(a, b)| (d(a), d(b))),
        })
        .unwrap()
    }

    #[test]
    fn thirty360_usa_feb_rule_order() {
        // d1 = Feb EOM -> 30, d2 = 31 NOT capped (checked before Feb rule).
        assert_eq!(
            yf(DayCounterId::Thirty360USA, "2023-02-28", "2023-03-31", None),
            31.0 / 360.0
        );
        assert_eq!(
            yf(DayCounterId::Thirty360USA, "2024-02-29", "2024-08-31", None),
            181.0 / 360.0
        );
        assert_eq!(
            yf(DayCounterId::Thirty360USA, "2023-01-31", "2023-07-31", None),
            0.5
        );
    }

    #[test]
    fn act_act_isda_cross_year() {
        assert_eq!(
            yf(
                DayCounterId::ActualActualISDA,
                "2023-10-15",
                "2024-04-15",
                None
            ),
            78.0 / 365.0 + 105.0 / 366.0
        );
        assert_eq!(
            yf(
                DayCounterId::ActualActualISDA,
                "2022-06-30",
                "2025-06-30",
                None
            ),
            3.0
        );
    }

    #[test]
    fn act_act_isma_ref_period() {
        assert_eq!(
            yf(
                DayCounterId::ActualActualISMA,
                "2023-06-15",
                "2023-12-15",
                Some(("2023-06-15", "2023-12-15"))
            ),
            0.5
        );
        assert_eq!(
            yf(
                DayCounterId::ActualActualISMA,
                "2023-08-15",
                "2023-12-15",
                Some(("2023-06-15", "2023-12-15"))
            ),
            122.0 * 0.5 / 183.0
        );
        assert_eq!(
            yf(
                DayCounterId::ActualActualISMA,
                "2023-06-15",
                "2024-06-15",
                Some(("2023-06-15", "2024-06-15"))
            ),
            1.0
        );
    }

    #[test]
    fn rejects_reversed_period() {
        let err = year_fraction(&DayCountRequest {
            counter: DayCounterId::Actual360,
            start: d("2024-01-01"),
            end: d("2023-01-01"),
            ref_period: None,
        })
        .unwrap_err();
        assert_eq!(err.error_type_str(), "invalid_input");
        assert!(err.correctable);
    }
}
