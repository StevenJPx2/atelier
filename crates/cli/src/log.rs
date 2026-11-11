//! The daemon's log: one line per event, to standard output, which the
//! Homebrew service sends to `atelier.log`.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

/// Prints `2026-11-11T12:22:03Z StevenJPx2/chauffeur text`.
pub fn log(source: &str, text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{} {source} {text}", now());
    let _ = out.flush();
}

/// The UTC time, to the second.
fn now() -> String {
    stamp(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_secs()),
    )
}

/// `secs` after 1970 as `2026-11-11T12:22:03Z`.
fn stamp(secs: u64) -> String {
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil(i64::try_from(days).unwrap_or(0));

    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// The calendar date `days` after 1970-01-01, by Howard Hinnant's
/// `civil_from_days`.
#[expect(
    clippy::arithmetic_side_effects,
    reason = "days since 1970 from the system clock: every step stays far inside i64"
)]
const fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };

    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_since_1970_are_calendar_dates_across_leap_years_and_new_years() {
        let cases = [
            (0, (1970, 1, 1)),
            (-1, (1969, 12, 31)),
            (11_016, (2000, 2, 29)),
            (19_782, (2024, 2, 29)),
            (20_088, (2024, 12, 31)),
            (20_089, (2025, 1, 1)),
            (20_403, (2025, 11, 11)),
            (20_768, (2026, 11, 11)),
            (47_541, (2100, 3, 1)),
        ];

        for (days, date) in cases {
            assert_eq!(civil(days), date, "{days}");
        }
    }

    #[test]
    fn an_instant_is_its_utc_date_and_time() {
        assert_eq!(stamp(1_794_408_756), "2026-11-11T14:52:36Z");
        assert_eq!(stamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(stamp(86_399), "1970-01-01T23:59:59Z");
    }

    #[test]
    fn the_time_is_utc_to_the_second() {
        let time = now();
        let digits = |range: std::ops::Range<usize>| {
            time.get(range)
                .is_some_and(|part| part.bytes().all(|b| b.is_ascii_digit()))
        };

        assert_eq!(time.len(), 20, "{time}");
        assert!(time.starts_with("20"), "{time}");
        assert!(digits(0..4) && digits(5..7) && digits(8..10), "{time}");
        assert!(digits(11..13) && digits(14..16) && digits(17..19), "{time}");
        assert_eq!(
            [
                time.get(4..5),
                time.get(10..11),
                time.get(13..14),
                time.get(19..20)
            ],
            [Some("-"), Some("T"), Some(":"), Some("Z")]
        );
    }
}
