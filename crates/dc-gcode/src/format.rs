// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/utils.ts

//! Number formatting for G-code output, the one place numbers become text
//! (`docs/rust-guidelines.md`, "Floating point and determinism";
//! `docs/specs/phase-1/design.md`, "Emission", step 5).
//!
//! Ported from `formatGCodeNumber`. Changes from upstream: negative zero is
//! written as zero, and a non-finite value is an error instead of `NaN`.
//! Rust's fixed-precision formatting never uses exponent notation, which
//! neither GRBL nor grblHAL reads.

use dc_project::{NumberFormat, Units};

use crate::ExportError;

/// Decimal places `format` gives numbers in `units`.
#[must_use]
pub fn decimal_places(format: &NumberFormat, units: Units) -> u8 {
    match units {
        Units::Mm => format.decimal_places.mm,
        Units::Inch => format.decimal_places.inch,
    }
}

/// Writes `value` (already in output `units`) with the post-processor's
/// decimal places, trailing-zero, and leading-zero rules. Never uses
/// exponent notation; negative zero becomes zero.
///
/// # Errors
///
/// [`ExportError::NonFiniteNumber`] if `value` is `NaN` or infinite.
pub fn format_number(
    value: f64,
    format: &NumberFormat,
    units: Units,
) -> Result<String, ExportError> {
    format_fixed(value, decimal_places(format, units), format)
}

/// Writes `value` with exactly `places` decimals before the
/// post-processor's trailing-zero and leading-zero rules apply. For
/// numbers that are not lengths, such as dwell seconds.
///
/// # Errors
///
/// [`ExportError::NonFiniteNumber`] if `value` is `NaN` or infinite.
pub fn format_fixed(value: f64, places: u8, format: &NumberFormat) -> Result<String, ExportError> {
    if !value.is_finite() {
        return Err(ExportError::NonFiniteNumber { value });
    }
    // `{:.N}` on f64 prints every integer digit and never an exponent.
    let mut s = format!("{:.*}", usize::from(places), value);
    // A value that rounds to zero from below prints as "-0.000".
    if let Some(unsigned) = s.strip_prefix('-')
        && unsigned.chars().all(|c| c == '0' || c == '.')
    {
        s = unsigned.to_owned();
    }
    if !format.trailing_zeros && s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_owned();
    }
    if !format.leading_zero {
        if let Some(rest) = s.strip_prefix("0.") {
            s = format!(".{rest}");
        } else if let Some(rest) = s.strip_prefix("-0.") {
            s = format!("-.{rest}");
        }
    }
    Ok(s)
}

/// The number a controller reads from `text`, a word value this module
/// wrote. `None` only if `text` is not a number.
#[must_use]
pub fn read_back(text: &str) -> Option<f64> {
    text.parse::<f64>().ok().filter(|v| v.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::DecimalPlaces;

    fn fmt(trailing_zeros: bool, leading_zero: bool) -> NumberFormat {
        NumberFormat {
            decimal_places: DecimalPlaces { mm: 3, inch: 4 },
            trailing_zeros,
            leading_zero,
        }
    }

    fn mm(value: f64, format: &NumberFormat) -> String {
        format_number(value, format, Units::Mm).unwrap()
    }

    // Cases follow `formatGCodeNumber` in PureCutCNC's `utils.ts`:
    // `toFixed(places)`, then optional trailing-zero and leading-zero
    // stripping. Upstream has no direct test of it; these pin its behaviour.
    #[test]
    fn decimal_places_and_trailing_zeros() {
        let keep = fmt(true, true);
        assert_eq!(mm(1.5, &keep), "1.500");
        assert_eq!(mm(-12.3456, &keep), "-12.346");
        assert_eq!(mm(0.0, &keep), "0.000");
        assert_eq!(mm(100.0, &keep), "100.000");
        assert_eq!(
            format_number(1.23456, &keep, Units::Inch).unwrap(),
            "1.2346"
        );
        assert_eq!(decimal_places(&keep, Units::Mm), 3);
        assert_eq!(decimal_places(&keep, Units::Inch), 4);

        let strip = fmt(false, true);
        assert_eq!(mm(1.5, &strip), "1.5");
        assert_eq!(mm(2.0, &strip), "2");
        assert_eq!(mm(100.0, &strip), "100");
        assert_eq!(mm(0.0, &strip), "0");
        assert_eq!(mm(-0.25, &strip), "-0.25");

        let no_lead = fmt(true, false);
        assert_eq!(mm(0.5, &no_lead), ".500");
        assert_eq!(mm(-0.5, &no_lead), "-.500");
        assert_eq!(mm(10.5, &no_lead), "10.500");
        let neither = fmt(false, false);
        assert_eq!(mm(0.5, &neither), ".5");
        assert_eq!(mm(0.0, &neither), "0");
    }

    #[test]
    fn negative_zero_is_written_as_zero() {
        assert_eq!(mm(-0.0, &fmt(true, true)), "0.000");
        assert_eq!(mm(-0.0001, &fmt(true, true)), "0.000");
        assert_eq!(mm(-0.0001, &fmt(false, true)), "0");
    }

    #[test]
    fn read_back_reads_every_written_form() {
        for (text, value) in [("1.500", 1.5), (".5", 0.5), ("-.5", -0.5), ("2", 2.0)] {
            assert_eq!(read_back(text), Some(value), "{text}");
        }
        assert_eq!(read_back("NaN"), None);
    }

    #[test]
    fn non_finite_is_an_error() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert!(matches!(
                format_number(value, &fmt(true, true), Units::Mm),
                Err(ExportError::NonFiniteNumber { .. })
            ));
        }
    }

    #[test]
    fn very_small_and_large_values_are_plain_decimals() {
        let keep = fmt(true, true);
        assert_eq!(mm(1e-12, &keep), "0.000");
        assert_eq!(mm(1e20, &keep), "100000000000000000000.000");
        assert_eq!(mm(-1e20, &keep), "-100000000000000000000.000");
    }
}
