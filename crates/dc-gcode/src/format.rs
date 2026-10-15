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
    let _ = (format, units);
    0
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
    let _ = (value, format, units);
    Ok(String::new())
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
