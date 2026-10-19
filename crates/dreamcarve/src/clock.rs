// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! UTC dates from the system clock, with no date crate: the program header's
//! date and the build time in `--version`. `build.rs` includes this file too.

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since 1970-01-01 UTC; 0 when the clock is before then.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// The UTC date of `secs` as `YYYY-MM-DD`.
pub fn date_utc(secs: u64) -> String {
    let (y, m, d) = civil_from_days(days(secs));
    format!("{y:04}-{m:02}-{d:02}")
}

/// The UTC time of `secs` as one number, `YYYYMMDDHHMMSS`, which sorts in
/// time order.
#[allow(
    dead_code,
    reason = "build.rs and the tests use it; the binary does not"
)]
pub fn timestamp_utc(secs: u64) -> String {
    let (y, m, d) = civil_from_days(days(secs));
    let of_day = secs % 86_400;
    let (hh, mm, ss) = (of_day / 3600, of_day % 3600 / 60, of_day % 60);
    format!("{y:04}{m:02}{d:02}{hh:02}{mm:02}{ss:02}")
}

fn days(secs: u64) -> i64 {
    i64::try_from(secs / 86_400).unwrap_or(0)
}

/// Proleptic Gregorian date of `days` since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`, public domain).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    // `d` is 1..=31 and `m` 1..=12 by construction.
    (
        y,
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_741), (2026, 10, 15));
    }

    #[test]
    fn date_utc_formats_with_dashes() {
        assert_eq!(date_utc(0), "1970-01-01");
        assert_eq!(date_utc(1_792_367_112), "2026-10-18");
    }

    #[test]
    fn timestamp_utc_matches_known_times() {
        assert_eq!(timestamp_utc(0), "19700101000000");
        assert_eq!(timestamp_utc(1_792_367_112), "20261018234512");
        assert_eq!(timestamp_utc(1_709_208_000), "20240229120000");
        assert_eq!(timestamp_utc(951_868_800), "20000301000000");
        assert_eq!(timestamp_utc(4_133_980_799), "21001231235959");
    }
}
