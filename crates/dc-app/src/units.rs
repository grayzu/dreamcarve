// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Units display and entry (`docs/specs/phase-4/design.md`, "Units";
//! functional spec, capability 53). The model keeps millimetres
//! (ADR-0001); a field shows the project's display units with the unit
//! after the value, and parses what the file accepts (capability 32):
//! `0.5 in`, `1/4 in`, `1 1/4 in`, `12.7 mm`, `60 in/min`. A bare number is
//! in the project's units. Parsing goes through
//! [`dc_project::units_input`], the file's own deserializer, so the app
//! and a hand-edited `.dcv` never disagree on what a string means.

use dc_project::Units;

use crate::error::CommandError;

/// Millimetres per inch, exactly.
const MM_PER_INCH: f64 = 25.4;

/// The unit a length shows: `mm` or `in`.
#[must_use]
pub const fn suffix(units: Units) -> &'static str {
    match units {
        Units::Mm => "mm",
        Units::Inch => "in",
    }
}

/// The unit a feed rate shows: `mm/min` or `in/min`.
#[must_use]
pub const fn rate_suffix(units: Units) -> &'static str {
    match units {
        Units::Mm => "mm/min",
        Units::Inch => "in/min",
    }
}

/// `value_mm` in the display units: unchanged for mm, divided by 25.4
/// for inches. Feed rates convert the same way.
#[must_use]
pub fn to_display(value_mm: f64, units: Units) -> f64 {
    match units {
        Units::Mm => value_mm,
        Units::Inch => value_mm / MM_PER_INCH,
    }
}

/// A length for a field: `value_mm` in `units` to `decimals` places, the
/// unit after it (`6.35 mm`, `0.250 in`). Negative zero prints as zero.
#[must_use]
pub fn format(value_mm: f64, units: Units, decimals: usize) -> String {
    format!(
        "{} {}",
        number(to_display(value_mm, units), decimals),
        suffix(units)
    )
}

/// A feed rate for a field: `value_mm_min` in `units` per minute to
/// `decimals` places, the unit after it (`1500 mm/min`, `60.0 in/min`).
#[must_use]
pub fn format_rate(value_mm_min: f64, units: Units, decimals: usize) -> String {
    format!(
        "{} {}",
        number(to_display(value_mm_min, units), decimals),
        rate_suffix(units)
    )
}

/// Decimal places a field shows of a length in millimetres: a micron
/// and a tenth, finer than any tolerance the file takes.
const FIELD_DECIMALS_MM: usize = 4;
/// Decimal places a field shows of a length in inches: the same
/// resolution as [`FIELD_DECIMALS_MM`].
const FIELD_DECIMALS_INCH: usize = 5;

/// A length as a field's text: `value_mm` in `units`, trailing zeros
/// dropped (`160`, `12.7`, `0.5`), with no unit, since the field draws
/// its unit after the value (`experience.md`, section 7). Parsing the
/// text back with [`parse_length_mm`] gives the value to the field's
/// resolution.
#[must_use]
pub fn field_text(value_mm: f64, units: Units) -> String {
    let decimals = match units {
        Units::Mm => FIELD_DECIMALS_MM,
        Units::Inch => FIELD_DECIMALS_INCH,
    };
    trimmed(&number(to_display(value_mm, units), decimals))
}

/// Decimal places a closed group's summary shows of a length in
/// millimetres (functional spec, capability 66): a glance, not the field.
const SUMMARY_DECIMALS_MM: usize = 2;
/// Decimal places a summary shows of a length in inches: a thou.
const SUMMARY_DECIMALS_INCH: usize = 3;

/// A length as a closed group's summary words it: `value_mm` in `units`
/// to a hundredth of a millimetre or a thou, trailing zeros dropped (`5`,
/// `0.197`), with no unit.
#[must_use]
pub fn summary_text(value_mm: f64, units: Units) -> String {
    let decimals = match units {
        Units::Mm => SUMMARY_DECIMALS_MM,
        Units::Inch => SUMMARY_DECIMALS_INCH,
    };
    trimmed(&number(to_display(value_mm, units), decimals))
}

/// `text`, a formatted decimal, without trailing zeros or a bare point.
fn trimmed(text: &str) -> String {
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text.to_owned()
    }
}

/// `value` to `decimals` places with negative zero normalised, as
/// `dc-gcode` formats numbers for output.
fn number(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    match text.strip_prefix('-') {
        Some(digits) if digits.chars().all(|c| c == '0' || c == '.') => digits.to_owned(),
        _ => text,
    }
}

/// A typed length as millimetres (functional spec, acceptance 89): a
/// string with a unit means what it means in a file; a bare number is in
/// `project_units`. `1/4 in` is 6.35 whatever the project; `0.5` in an
/// inch project is 12.7.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] with the file parser's message when the
/// text is not a length: empty, not a number, a rate's unit, or not finite.
pub fn parse_length_mm(text: &str, project_units: Units) -> Result<f64, CommandError> {
    parse(text, suffix(project_units), dc_project::units_input::mm)
}

/// A typed feed rate as millimetres per minute: a string with a unit
/// (`mm/min`, `in/min`) means what it means in a file; a bare number is
/// in `project_units` per minute.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when the text is not a feed rate.
pub fn parse_rate_mm_min(text: &str, project_units: Units) -> Result<f64, CommandError> {
    parse(
        text,
        rate_suffix(project_units),
        dc_project::units_input::mm_min,
    )
}

/// `text` through the file's deserializer `read`, with `default_unit`
/// appended when it has none, so one grammar serves the file and the app.
fn parse(
    text: &str,
    default_unit: &str,
    read: fn(serde_json::Value) -> Result<f64, serde_json::Error>,
) -> Result<f64, CommandError> {
    let trimmed = text.trim();
    let refused = |reason: String| CommandError::NotAQuantity {
        text: text.to_owned(),
        reason,
    };
    if trimmed.is_empty() {
        return Err(refused("nothing was typed".to_owned()));
    }
    let with_unit = if trimmed.chars().any(char::is_alphabetic) {
        trimmed.to_owned()
    } else {
        format!("{trimmed} {default_unit}")
    };
    let value = read(serde_json::Value::String(with_unit)).map_err(|e| refused(e.to_string()))?;
    if !value.is_finite() {
        return Err(refused(format!("`{text}` is not a finite number")));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_normalises_negative_zero() {
        assert_eq!(number(-0.0, 2), "0.00");
        assert_eq!(number(-0.0001, 2), "0.00");
        assert_eq!(number(-0.01, 2), "-0.01");
        assert_eq!(number(1.5, 0), "2");
    }

    #[test]
    fn bare_numbers_take_the_project_unit() {
        let mm = parse_length_mm("2", Units::Mm).unwrap_or_default();
        assert!((mm - 2.0).abs() < 1e-12);
        let inch = parse_length_mm("2", Units::Inch).unwrap_or_default();
        assert!((inch - 50.8).abs() < 1e-12);
    }
}
