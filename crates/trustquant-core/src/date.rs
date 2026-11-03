//! Calendar date. External contract is ISO 8601 `YYYY-MM-DD`, no timezone
//! (ADR-0002). The internal representation (days since 1970-01-01) is NOT
//! part of the contract; only observable semantics are (ordering, arithmetic,
//! weekday, end-of-month), pinned by `date_daycount` fixtures.

use std::fmt;

pub const MIN_YEAR: i32 = 1901;
pub const MAX_YEAR: i32 = 2199;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    days: i32, // days since 1970-01-01; internal only
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Weekday {
    pub fn as_str(self) -> &'static str {
        match self {
            Weekday::Mon => "Mon",
            Weekday::Tue => "Tue",
            Weekday::Wed => "Wed",
            Weekday::Thu => "Thu",
            Weekday::Fri => "Fri",
            Weekday::Sat => "Sat",
            Weekday::Sun => "Sun",
        }
    }
}

pub fn is_leap_year(y: i32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(y) => 29,
        2 => 28,
        _ => 0,
    }
}

/// days since civil epoch, Howard Hinnant's algorithm.
fn days_from_civil(y: i32, m: u32, d: u32) -> i32 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146097 + doe - 719468) as i32
}

fn civil_from_days(z: i32) -> (i32, u32, u32) {
    let z = z as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

impl Date {
    pub fn from_ymd(y: i32, m: u32, d: u32) -> Result<Date, String> {
        if !(MIN_YEAR..=MAX_YEAR).contains(&y) {
            return Err(format!(
                "year {y} out of supported range [{MIN_YEAR},{MAX_YEAR}]"
            ));
        }
        if !(1..=12).contains(&m) {
            return Err(format!("month {m} out of range [1,12]"));
        }
        if d == 0 || d > days_in_month(y, m) {
            return Err(format!("day {d} invalid for {y}-{m:02}"));
        }
        Ok(Date {
            days: days_from_civil(y, m, d),
        })
    }

    pub fn parse_iso(s: &str) -> Result<Date, String> {
        let bytes = s.as_bytes();
        if bytes.len() != 10 || bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(format!("date '{s}' is not ISO 8601 YYYY-MM-DD"));
        }
        let num = |a: usize, b: usize| -> Result<i32, String> {
            s.get(a..b)
                .and_then(|t| t.parse::<i32>().ok())
                .filter(|_| s[a..b].bytes().all(|c| c.is_ascii_digit()))
                .ok_or_else(|| format!("date '{s}' has non-numeric components"))
        };
        let y = num(0, 4)?;
        let m = num(5, 7)? as u32;
        let d = num(8, 10)? as u32;
        Date::from_ymd(y, m, d)
    }

    pub fn ymd(self) -> (i32, u32, u32) {
        civil_from_days(self.days)
    }

    pub fn year(self) -> i32 {
        self.ymd().0
    }

    pub fn month(self) -> u32 {
        self.ymd().1
    }

    pub fn day(self) -> u32 {
        self.ymd().2
    }

    pub fn to_iso(self) -> String {
        let (y, m, d) = self.ymd();
        format!("{y:04}-{m:02}-{d:02}")
    }

    pub fn add_days(self, n: i32) -> Result<Date, String> {
        let d = self.days + n;
        let (y, _, _) = civil_from_days(d);
        if !(MIN_YEAR..=MAX_YEAR).contains(&y) {
            return Err(format!(
                "resulting year {y} out of supported range [{MIN_YEAR},{MAX_YEAR}]"
            ));
        }
        Ok(Date { days: d })
    }

    pub fn days_since(self, other: Date) -> i32 {
        self.days - other.days
    }

    /// 1970-01-01 (epoch, days == 0) was a Thursday.
    pub fn weekday(self) -> Weekday {
        match self.days.rem_euclid(7) {
            0 => Weekday::Thu,
            1 => Weekday::Fri,
            2 => Weekday::Sat,
            3 => Weekday::Sun,
            4 => Weekday::Mon,
            5 => Weekday::Tue,
            _ => Weekday::Wed,
        }
    }

    pub fn is_last_day_of_feb(self) -> bool {
        let (y, m, d) = self.ymd();
        m == 2 && d == days_in_month(y, 2)
    }

    pub fn end_of_month(self) -> Date {
        let (y, m, _) = self.ymd();
        Date {
            days: days_from_civil(y, m, days_in_month(y, m)),
        }
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_iso())
    }
}

/// Add whole calendar months, clamping the day to the target month's end
/// (used by ActualActual ISMA; M0 fixtures do not exercise the clamp branch,
/// which is documented scope — see fixtures/date_daycount README).
pub fn add_months(d: Date, n: u32) -> Result<Date, String> {
    let (y, m, day) = d.ymd();
    let total = (y as i64) * 12 + (m as i64 - 1) + n as i64;
    let ny = (total.div_euclid(12)) as i32;
    let nm = (total.rem_euclid(12) + 1) as u32;
    let nd = day.min(days_in_month(ny, nm));
    Date::from_ymd(ny, nm, nd)
}

/// Add (possibly negative) whole months with end-of-month clamping.
pub fn add_months_signed(d: Date, n: i32) -> Result<Date, String> {
    if n == 0 {
        return Ok(d);
    }
    let (y, m, day) = d.ymd();
    let total = (y as i64) * 12 + (m as i64 - 1) + n as i64;
    let ny = total.div_euclid(12) as i32;
    let nm = (total.rem_euclid(12) + 1) as u32;
    let nd = day.min(days_in_month(ny, nm));
    Date::from_ymd(ny, nm, nd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_weekday() {
        assert!(matches!(
            Date::parse_iso("1970-01-01").unwrap().weekday(),
            Weekday::Thu
        ));
        assert!(matches!(
            Date::parse_iso("2000-01-01").unwrap().weekday(),
            Weekday::Sat
        ));
    }

    #[test]
    fn iso_roundtrip_and_validation() {
        for s in ["1901-01-01", "2024-02-29", "2199-12-31"] {
            assert_eq!(Date::parse_iso(s).unwrap().to_iso(), s);
        }
        for bad in [
            "1900-01-01",
            "2200-01-01",
            "2023-02-29",
            "2023-13-01",
            "2023-1-01",
            "not-a-date",
        ] {
            assert!(Date::parse_iso(bad).is_err(), "{bad} should be rejected");
        }
    }
}
