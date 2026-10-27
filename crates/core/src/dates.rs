// SPDX-License-Identifier: Apache-2.0
//! Dates for the query language (`created:`, `modified:`) and front matter
//! timestamps (format.md §4).
//!
//! A [`DateSpec`] names a period: a year, a month, a day, today, yesterday,
//! or the time since `N` days, weeks, months or years ago. [`DateSpec::period`]
//! turns it into milliseconds since the Unix epoch, as `[start, end)`, for a
//! given "now" and the user's UTC offset, so days begin at local midnight.

/// Milliseconds per day.
const DAY_MS: i64 = 86_400_000;

/// Days since 1970-01-01 of a date in the proleptic Gregorian calendar
/// (Howard Hinnant's days-from-civil).
#[must_use]
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date of a day since 1970-01-01: (year, month, day).
#[must_use]
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = u32::try_from(doy - (153 * mp + 2) / 5 + 1).unwrap_or(1);
    let month = u32::try_from(if mp < 10 { mp + 3 } else { mp - 9 }).unwrap_or(1);
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

fn days_in_month(year: i64, month: u32) -> u32 {
    let next = if month == 12 {
        days_from_civil(year + 1, 1, 1)
    } else {
        days_from_civil(year, month + 1, 1)
    };
    u32::try_from(next - days_from_civil(year, month, 1)).unwrap_or(28)
}

/// Milliseconds since the Unix epoch of an RFC 3339 timestamp such as
/// `2026-10-13T21:10:00+02:00`, `2026-10-13T19:10:00Z` or with fractions of a
/// second; `None` if it is not one.
#[must_use]
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let text = text.trim();
    let b = text.as_bytes();
    let num = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = text.get(range)?;
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    if b.len() < 20 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b't' | b' ') {
        return None;
    }
    if b[13] != b':' || b[16] != b':' {
        return None;
    }
    let (year, month, day) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (hour, minute, second) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let month = u32::try_from(month).ok()?;
    let day = u32::try_from(day).ok()?;
    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = &text[19..];
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let first3: String = fraction[..digits]
            .chars()
            .chain("000".chars())
            .take(3)
            .collect();
        millis = first3.parse::<i64>().ok()?;
        rest = &fraction[digits..];
    }
    let offset_minutes = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hm = &rest[1..];
            if hm.len() != 5 || hm.as_bytes()[2] != b':' {
                return None;
            }
            let h: i64 = hm[..2].parse().ok()?;
            let m: i64 = hm[3..].parse().ok()?;
            if h > 23 || m > 59 {
                return None;
            }
            sign * (h * 60 + m)
        }
    };
    let local = days_from_civil(year, month, day) * DAY_MS
        + ((hour * 60 + minute) * 60 + second) * 1000
        + millis;
    Some(local - offset_minutes * 60_000)
}

/// A unit for "the last N …".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgoUnit {
    /// 24 hours.
    Day,
    /// 7 days.
    Week,
    /// A calendar month.
    Month,
    /// A calendar year.
    Year,
}

/// A period named in a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSpec {
    /// `2026`
    Year(i64),
    /// `2026-10`
    Month(i64, u32),
    /// `2026-10-23`
    Day(i64, u32, u32),
    /// `today`
    Today,
    /// `yesterday`
    Yesterday,
    /// `7d`, `2w`, `3m`, `1y`: from that long ago until now.
    Ago(u32, AgoUnit),
}

impl DateSpec {
    /// Parses `2026`, `2026-10`, `2026-10-23`, `today`, `yesterday` or
    /// `Nd`/`Nw`/`Nm`/`Ny`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "today" => return Some(Self::Today),
            "yesterday" => return Some(Self::Yesterday),
            _ => {}
        }
        if let Some(unit) = text
            .chars()
            .last()
            .and_then(|c| match c.to_ascii_lowercase() {
                'd' => Some(AgoUnit::Day),
                'w' => Some(AgoUnit::Week),
                'm' => Some(AgoUnit::Month),
                'y' => Some(AgoUnit::Year),
                _ => None,
            })
        {
            let n = &text[..text.len() - 1];
            return (!n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
                .then(|| n.parse().ok())?
                .map(|n| Self::Ago(n, unit));
        }
        let parts: Vec<&str> = text.split('-').collect();
        let number = |s: &str, len: usize| -> Option<i64> {
            (s.len() == len && s.bytes().all(|b| b.is_ascii_digit()))
                .then(|| s.parse().ok())
                .flatten()
        };
        let year = number(parts.first()?, 4)?;
        match parts.as_slice() {
            [_] => Some(Self::Year(year)),
            [_, m] => {
                let m = u32::try_from(number(m, 2)?).ok()?;
                (1..=12).contains(&m).then_some(Self::Month(year, m))
            }
            [_, m, d] => {
                let m = u32::try_from(number(m, 2)?).ok()?;
                let d = u32::try_from(number(d, 2)?).ok()?;
                ((1..=12).contains(&m) && d >= 1 && d <= days_in_month(year, m))
                    .then_some(Self::Day(year, m, d))
            }
            _ => None,
        }
    }

    /// The period as `[start, end)` in milliseconds since the Unix epoch, for
    /// the moment `now_ms` and a UTC offset of `offset_minutes` (local time
    /// minus UTC, e.g. 120 for CEST).
    #[must_use]
    pub fn period(self, now_ms: i64, offset_minutes: i64) -> (i64, i64) {
        let offset = offset_minutes * 60_000;
        // Local midnight of a date, as UTC milliseconds.
        let midnight = |days: i64| days * DAY_MS - offset;
        let today = (now_ms + offset).div_euclid(DAY_MS);
        match self {
            Self::Year(y) => (
                midnight(days_from_civil(y, 1, 1)),
                midnight(days_from_civil(y + 1, 1, 1)),
            ),
            Self::Month(y, m) => {
                let (ny, nm) = if m == 12 { (y + 1, 1) } else { (y, m + 1) };
                (
                    midnight(days_from_civil(y, m, 1)),
                    midnight(days_from_civil(ny, nm, 1)),
                )
            }
            Self::Day(y, m, d) => {
                let day = days_from_civil(y, m, d);
                (midnight(day), midnight(day + 1))
            }
            Self::Today => (midnight(today), midnight(today + 1)),
            Self::Yesterday => (midnight(today - 1), midnight(today)),
            Self::Ago(n, unit) => {
                let n = i64::from(n);
                let start = match unit {
                    AgoUnit::Day => now_ms - n * DAY_MS,
                    AgoUnit::Week => now_ms - 7 * n * DAY_MS,
                    AgoUnit::Month | AgoUnit::Year => {
                        // The same local date and time, N months or years back.
                        let local = now_ms + offset;
                        let (y, m, d) = civil_from_days(local.div_euclid(DAY_MS));
                        let months = if unit == AgoUnit::Year { 12 * n } else { n };
                        let total = y * 12 + i64::from(m) - 1 - months;
                        let (ty, tm) = (
                            total.div_euclid(12),
                            u32::try_from(total.rem_euclid(12) + 1).unwrap_or(1),
                        );
                        let td = d.min(days_in_month(ty, tm));
                        days_from_civil(ty, tm, td) * DAY_MS + local.rem_euclid(DAY_MS) - offset
                    }
                };
                (start, now_ms + 1)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_round_trips() {
        for days in [-719_468, -1, 0, 1, 11_016, 20_744, 47_541] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(
            civil_from_days(days_from_civil(2026, 10, 18)),
            (2026, 10, 18)
        );
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2026, 2), 28);
    }

    #[test]
    fn parses_rfc3339() {
        let utc = parse_rfc3339("2026-10-13T19:10:00Z").unwrap();
        assert_eq!(parse_rfc3339("2026-10-13T21:10:00+02:00"), Some(utc));
        assert_eq!(parse_rfc3339("2026-10-13T14:10:00-05:00"), Some(utc));
        assert_eq!(parse_rfc3339("2026-10-13T19:10:00.250Z"), Some(utc + 250));
        assert_eq!(parse_rfc3339("2026-10-13T19:10:00.5Z"), Some(utc + 500));
        assert_eq!(
            utc,
            days_from_civil(2026, 10, 13) * DAY_MS + (19 * 60 + 10) * 60_000
        );
        for bad in [
            "",
            "2026-10-13",
            "2026-10-13T19:10Z",
            "2026-13-01T00:00:00Z",
            "2026-02-30T00:00:00Z",
            "2026-10-13T19:10:00",
            "2026-10-13T19:10:00+2:00",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
    }

    #[test]
    fn parses_date_specs() {
        assert_eq!(DateSpec::parse("2026"), Some(DateSpec::Year(2026)));
        assert_eq!(DateSpec::parse("2026-10"), Some(DateSpec::Month(2026, 10)));
        assert_eq!(
            DateSpec::parse("2026-10-23"),
            Some(DateSpec::Day(2026, 10, 23))
        );
        assert_eq!(DateSpec::parse("Today"), Some(DateSpec::Today));
        assert_eq!(DateSpec::parse("7d"), Some(DateSpec::Ago(7, AgoUnit::Day)));
        assert_eq!(
            DateSpec::parse("12M"),
            Some(DateSpec::Ago(12, AgoUnit::Month))
        );
        for bad in [
            "",
            "26",
            "2026-1",
            "2026-13",
            "2026-02-30",
            "d",
            "x7d",
            "7x",
            "2026-10-23-1",
        ] {
            assert_eq!(DateSpec::parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn periods_follow_local_midnight() {
        // 2026-10-23T22:30:00Z = 2026-10-24 00:30 in CEST (+120).
        let now = parse_rfc3339("2026-10-23T22:30:00Z").unwrap();
        let at = |s: &str| parse_rfc3339(s).unwrap();
        assert_eq!(
            DateSpec::Today.period(now, 120),
            (
                at("2026-10-24T00:00:00+02:00"),
                at("2026-10-25T00:00:00+02:00")
            )
        );
        assert_eq!(DateSpec::Today.period(now, 0).0, at("2026-10-23T00:00:00Z"));
        assert_eq!(
            DateSpec::Yesterday.period(now, 120),
            (
                at("2026-10-23T00:00:00+02:00"),
                at("2026-10-24T00:00:00+02:00")
            )
        );
        assert_eq!(
            DateSpec::Month(2026, 12).period(now, 60),
            (
                at("2026-12-01T00:00:00+01:00"),
                at("2027-01-01T00:00:00+01:00")
            )
        );
        assert_eq!(
            DateSpec::Ago(7, AgoUnit::Day).period(now, 120).0,
            now - 7 * DAY_MS
        );
        // A month back from March 31 is February 28.
        let march = at("2026-03-31T10:00:00Z");
        assert_eq!(
            DateSpec::Ago(1, AgoUnit::Month).period(march, 0).0,
            at("2026-02-28T10:00:00Z")
        );
        assert_eq!(
            DateSpec::Ago(1, AgoUnit::Year).period(march, 0).0,
            at("2025-03-31T10:00:00Z")
        );
    }
}
