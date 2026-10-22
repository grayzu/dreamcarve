// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Reads the `$G` modal-state report both firmwares print,
//! `[GC:G1 G54 G17 G21 G90 G94 M5 M9 T0 F1500 S0]`.
//!
//! The format is GRBL 1.1's (`report_gcode_modes`, `grbl/report.c`), which
//! grblHAL keeps and extends (`report.c`): the motion mode first, then the
//! other modal groups, then `T`, `F` and `S`. grblHAL adds the scale state
//! (`G50`, or `G51:` and the scaled axes) and may print more decimals.
//! This reader is written against the pinned commits' output.

use dc_gcode::parse::MotionKind;

/// The words of a `$G` report, or `None` if `line` is not one.
#[must_use]
pub fn parse_modal_state(line: &str) -> Option<Vec<String>> {
    let body = line.trim().strip_prefix("[GC:")?.strip_suffix(']')?;
    Some(body.split_whitespace().map(str::to_owned).collect())
}

/// The motion mode a report names: `G0` to `G3`. `None` for `G80` or a
/// report without one.
#[must_use]
pub fn motion_mode(modal: &[String]) -> Option<MotionKind> {
    modal.iter().find_map(|w| match w.as_str() {
        "G0" => Some(MotionKind::Rapid),
        "G1" => Some(MotionKind::Linear),
        "G2" => Some(MotionKind::ArcCw),
        "G3" => Some(MotionKind::ArcCcw),
        _ => None,
    })
}

/// The feed a report names, and one unit of its last printed digit: both
/// print `F1500` with no decimals in millimetres, and grblHAL truncates
/// rather than rounds (a feed of 1499.9995 mm/min, `F59.055` in inches,
/// prints `F1499`), so the feed is within one digit of the report. It is in
/// report units, mm/min with the default `$13=0`, whatever `G20` or `G21`
/// the program set: both firmwares hold the feed in mm/min.
#[must_use]
pub fn feed(modal: &[String]) -> Option<(f64, f64)> {
    let text = modal.iter().find_map(|w| w.strip_prefix('F'))?;
    let value: f64 = text.parse().ok()?;
    let decimals = text.split_once('.').map_or(0, |(_, d)| d.len());
    let digit = 1.0 / 10_f64.powi(i32::try_from(decimals).unwrap_or(0));
    Some((value, digit))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::assert_approx_eq;

    /// `$G` as the pinned grblHAL Simulator printed it after `G51 Z-1`
    /// and a header with `G50`, and as grbl-sim printed it after `G21 G90`.
    #[test]
    fn reads_modal_state_reports() {
        let hal =
            parse_modal_state("[GC:G0 G54 G17 G21 G90 G94 G40 G49 G98 G51:Z M5 M9 T0 F600 S0.]")
                .expect("a report");
        assert!(hal.iter().any(|w| w == "G51:Z"));
        assert_eq!(motion_mode(&hal), Some(MotionKind::Rapid));
        let grbl =
            parse_modal_state("[GC:G2 G54 G17 G21 G90 G94 M5 M9 T0 F1500 S0]").expect("a report");
        assert_eq!(motion_mode(&grbl), Some(MotionKind::ArcCw));
        let (f, digit) = feed(&grbl).expect("a feed");
        assert_approx_eq!(f, 1500.0, 1e-12);
        assert_approx_eq!(digit, 1.0, 1e-12);
        assert_eq!(parse_modal_state("ok"), None);
    }

    #[test]
    fn a_feed_with_decimals_has_a_finer_resolution() {
        let words = vec!["G1".to_owned(), "F39.4".to_owned()];
        let (f, digit) = feed(&words).expect("a feed");
        assert_approx_eq!(f, 39.4, 1e-12);
        assert_approx_eq!(digit, 0.1, 1e-12);
        assert_eq!(motion_mode(&["G80".to_owned()]), None);
    }
}
