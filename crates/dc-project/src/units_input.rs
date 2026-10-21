// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Lengths and feed rates written with their unit (functional spec,
//! capability 32; `docs/specs/phase-2/design.md`, "Phase 2 addendum (#73)
//! → A6").
//!
//! Every persisted `*_mm` field of the model reads through [`mm`] (or
//! [`opt_mm`], [`vec_mm`]) and every `*_mm_min` field through [`mm_min`]
//! (or [`opt_mm_min`]). A JSON number is in the field's unit, as it always
//! was. A JSON string is `<number> <unit>`, the space optional:
//!
//! - `<number>` is a decimal with an optional sign (`0.5`, `-2`, `.25`), a
//!   fraction `a/b`, or a mixed number `w a/b` (`1 1/4`);
//! - the unit is `mm` or `in` on a length, `mm/min` or `in/min` on a feed
//!   rate.
//!
//! An inch value is `n × 25.4`, one `f64` product, so a load is
//! deterministic. Any other string is a serde error, which
//! [`crate::load`] reports at the field's line and column. The model keeps
//! millimetres (ADR-0001), so [`crate::save`] writes numbers back.

use serde::de::{Deserialize, Deserializer, Visitor};
use std::fmt;

/// Millimetres per inch, exactly.
const MM_PER_INCH: f64 = 25.4;

/// What a field holds, which decides the units it takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Quantity {
    /// A length, millimetres.
    Length,
    /// A feed rate, millimetres per minute.
    Rate,
}

impl Quantity {
    /// The units this quantity takes and each one's size in the field's
    /// unit.
    const fn units(self) -> &'static [(&'static str, f64)] {
        match self {
            Self::Length => &[("mm", 1.0), ("in", MM_PER_INCH)],
            Self::Rate => &[("mm/min", 1.0), ("in/min", MM_PER_INCH)],
        }
    }

    /// The other quantity's units, to say why a unit of it is wrong here.
    const fn other(self) -> Self {
        match self {
            Self::Length => Self::Rate,
            Self::Rate => Self::Length,
        }
    }

    /// The quantity in words, with its article.
    const fn noun(self) -> &'static str {
        match self {
            Self::Length => "a length",
            Self::Rate => "a feed rate",
        }
    }

    /// Two examples of a valid string.
    const fn examples(self) -> &'static str {
        match self {
            Self::Length => "\"12.7 mm\", \"0.5 in\" or \"1 1/4 in\"",
            Self::Rate => "\"1500 mm/min\" or \"60 in/min\"",
        }
    }
}

/// Reads a length, millimetres: a number, or a string with its unit.
///
/// # Errors
///
/// A string that is not `<number> <unit>` with a length unit.
pub fn mm<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    deserializer.deserialize_any(QuantityVisitor(Quantity::Length))
}

/// [`mm`] for an `Option` field: `null` is `None`. The field stays
/// required, as `Option::deserialize` made it.
///
/// # Errors
///
/// As for [`mm`].
pub fn opt_mm<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    Ok(Option::<Length>::deserialize(deserializer)?.map(|l| l.0))
}

/// [`mm`] for each element of a list.
///
/// # Errors
///
/// As for [`mm`], at the first element that fails.
pub fn vec_mm<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Vec<f64>, D::Error> {
    Ok(Vec::<Length>::deserialize(deserializer)?
        .into_iter()
        .map(|l| l.0)
        .collect())
}

/// Reads a feed rate, millimetres per minute: a number, or a string with
/// its unit.
///
/// # Errors
///
/// A string that is not `<number> <unit>` with a rate unit.
pub fn mm_min<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    deserializer.deserialize_any(QuantityVisitor(Quantity::Rate))
}

/// [`mm_min`] for an `Option` field: `null` is `None`. The field stays
/// required.
///
/// # Errors
///
/// As for [`mm_min`].
pub fn opt_mm_min<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<f64>, D::Error> {
    Ok(Option::<Rate>::deserialize(deserializer)?.map(|r| r.0))
}

/// A length read by [`mm`], so `Option` and `Vec` can wrap it.
struct Length(f64);

impl<'de> Deserialize<'de> for Length {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        mm(deserializer).map(Self)
    }
}

/// A rate read by [`mm_min`], so `Option` can wrap it.
struct Rate(f64);

impl<'de> Deserialize<'de> for Rate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        mm_min(deserializer).map(Self)
    }
}

/// Takes a number as is and parses a string with [`parse`].
struct QuantityVisitor(Quantity);

impl Visitor<'_> for QuantityVisitor {
    type Value = f64;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a number, or {} with its unit such as {}",
            self.0.noun(),
            self.0.examples()
        )
    }

    fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<f64, E> {
        Ok(v)
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "as serde reads an integer into an f64 field"
    )]
    fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<f64, E> {
        Ok(v as f64)
    }

    #[allow(
        clippy::cast_precision_loss,
        reason = "as serde reads an integer into an f64 field"
    )]
    fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<f64, E> {
        Ok(v as f64)
    }

    fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<f64, E> {
        parse(v, self.0).map_err(E::custom)
    }
}

/// `text` as a value of `quantity` in the field's unit, or why it is not
/// one.
fn parse(text: &str, quantity: Quantity) -> Result<f64, String> {
    let trimmed = text.trim();
    // The unit starts at the first letter; the number is what comes before.
    let split = trimmed
        .find(|c: char| c.is_alphabetic())
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(split);
    let (number, unit) = (number.trim_end(), unit);
    let not_valid = || {
        format!(
            "`{text}` is not {}: write a number, or the value with its unit, such as {}",
            quantity.noun(),
            quantity.examples()
        )
    };
    if unit.is_empty() {
        return Err(format!(
            "`{text}` has no unit: write a number, or the value with its unit, such as {}",
            quantity.examples()
        ));
    }
    let Some(&(_, factor)) = quantity.units().iter().find(|(name, _)| *name == unit) else {
        if quantity
            .other()
            .units()
            .iter()
            .any(|(name, _)| *name == unit)
        {
            let wanted: Vec<&str> = quantity.units().iter().map(|(name, _)| *name).collect();
            return Err(format!(
                "`{text}` is in `{unit}`, but this field is {}: use {}",
                quantity.noun(),
                wanted.join(" or ")
            ));
        }
        return Err(not_valid());
    };
    let value = parse_number(number).ok_or_else(not_valid)? * factor;
    if value.is_finite() {
        Ok(value)
    } else {
        Err(format!("`{text}` is too large"))
    }
}

/// A decimal with an optional sign, `a/b` or `w a/b`; `None` for anything
/// else, a zero denominator included.
fn parse_number(text: &str) -> Option<f64> {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let magnitude = match unsigned.split_once('/') {
        Some((before, denominator)) => {
            let (whole, numerator) = match before.trim_end().rsplit_once(char::is_whitespace) {
                Some((whole, numerator)) => (Some(whole.trim_end()), numerator),
                None => (None, before),
            };
            // No space either side of the slash: `1/4`, not `1 / 4`.
            if before.ends_with(char::is_whitespace) {
                return None;
            }
            let whole = match whole {
                Some(w) => digits(w)?,
                None => 0.0,
            };
            let denominator = digits(denominator)?;
            if denominator == 0.0 {
                return None;
            }
            whole + digits(numerator)? / denominator
        }
        None => decimal(unsigned)?,
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// A run of ASCII digits as a number.
fn digits(text: &str) -> Option<f64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `d`, `d.`, `d.d` or `.d`, digits only: no sign, exponent, `inf` or
/// `NaN`, which `f64::from_str` would take.
fn decimal(text: &str) -> Option<f64> {
    let (int, frac) = text.split_once('.').unwrap_or((text, ""));
    let all_digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if (int.is_empty() && frac.is_empty()) || !all_digits(int) || !all_digits(frac) {
        return None;
    }
    text.parse().ok()
}

#[cfg(test)]
mod tests {

    use crate::model::{Operation, Tool};
    use crate::serialize::{LoadError, load};
    use crate::test_support::{sample_project, tool_6_35_upcut};
    use dc_geometry::assert_approx_eq;
    use serde_json::{Value, json};

    /// The sample project as JSON, to edit before loading.
    fn sample_json() -> Value {
        serde_json::to_value(sample_project()).unwrap()
    }

    /// Loads `root`, written as `save` lays it out.
    fn load_json(root: &Value) -> Result<crate::Project, LoadError> {
        let text = serde_json::to_string_pretty(root).unwrap();
        load(text.as_bytes()).map(|loaded| loaded.project)
    }

    /// The sample with operation 0's `depth_mm` written as `value`, loaded.
    fn load_depth(value: Value) -> Result<f64, LoadError> {
        let mut root = sample_json();
        root["operations"][0]["depth_mm"] = value;
        load_json(&root).map(|p| p.operations[0].depth_mm().unwrap())
    }

    /// The sample tool with `feed_mm_min` written as `value`.
    fn tool_with_feed(value: Value) -> Result<Tool, serde_json::Error> {
        let mut tool = serde_json::to_value(tool_6_35_upcut()).unwrap();
        tool["feed_mm_min"] = value;
        serde_json::from_value(tool)
    }

    #[test]
    fn inch_string_converts_exactly() {
        assert_approx_eq!(load_depth(json!("0.5 in")).unwrap(), 0.5 * 25.4, 0.0);
        assert_approx_eq!(load_depth(json!("0.5 in")).unwrap(), 12.7, 0.0);
        assert_approx_eq!(load_depth(json!("0.5in")).unwrap(), 12.7, 0.0);
        assert_approx_eq!(load_depth(json!(" 0.5  in ")).unwrap(), 12.7, 0.0);
    }

    /// #283: the heights read lengths with their unit too, as every
    /// `*_mm` field does (capability 32).
    #[test]
    fn heights_take_inches() {
        let mut root = sample_json();
        root["heights"] = json!({
            "clearance_mm": "3/4 in",
            "tool_change_machine_z_mm": "-0.25 in",
            "park": { "x_mm": "1 in", "y_mm": 2.0 }
        });
        let heights = load_json(&root).unwrap().heights;
        assert_approx_eq!(heights.clearance_mm.unwrap(), 19.05, 1e-12);
        assert_approx_eq!(heights.tool_change_machine_z_mm.unwrap(), -6.35, 1e-12);
        let park = heights.park.unwrap();
        assert_approx_eq!(park.x_mm, 25.4, 0.0);
        assert_approx_eq!(park.y_mm, 2.0, 0.0);
    }

    #[test]
    fn fraction_and_mixed_number() {
        assert_approx_eq!(load_depth(json!("1/4 in")).unwrap(), 0.25 * 25.4, 0.0);
        assert_approx_eq!(load_depth(json!("1/4 in")).unwrap(), 6.35, 1e-12);
        assert_approx_eq!(load_depth(json!("1 1/4 in")).unwrap(), 1.25 * 25.4, 0.0);
        assert_approx_eq!(load_depth(json!("1 1/4 in")).unwrap(), 31.75, 1e-12);
        assert_approx_eq!(load_depth(json!("3/8in")).unwrap(), 0.375 * 25.4, 0.0);
        for bad in [
            "1/0 in",
            "1 / 4 in",
            "1/ 4 in",
            "1.5/4 in",
            "1 1 1/4 in",
            "/4 in",
        ] {
            assert!(load_depth(json!(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn rate_in_per_min() {
        let tool = tool_with_feed(json!("60 in/min")).unwrap();
        assert_approx_eq!(tool.feed_mm_min.unwrap(), 1524.0, 1e-9);
        assert_approx_eq!(tool.feed_mm_min.unwrap(), 60.0 * 25.4, 0.0);
        let tool = tool_with_feed(json!("1500 mm/min")).unwrap();
        assert_approx_eq!(tool.feed_mm_min.unwrap(), 1500.0, 0.0);
        assert_eq!(tool_with_feed(Value::Null).unwrap().feed_mm_min, None);
    }

    #[test]
    fn bare_number_is_mm() {
        let number = load_depth(json!(12.7)).unwrap();
        let string = load_depth(json!("12.7 mm")).unwrap();
        assert_approx_eq!(number, 12.7, 0.0);
        assert_approx_eq!(string, number, 0.0);
        assert_approx_eq!(load_depth(json!(12)).unwrap(), 12.0, 0.0);
        // A string without a unit is not a bare number.
        let err = load_depth(json!("12.7")).unwrap_err().to_string();
        assert!(err.contains("has no unit"), "{err}");
    }

    #[test]
    fn unknown_unit_is_located() {
        let mut root = sample_json();
        root["operations"][0]["depth_mm"] = json!("0.5 furlong");
        let text = serde_json::to_string_pretty(&root).unwrap();
        // Where the value starts: the line and column of its opening quote.
        let (line, column) = text
            .lines()
            .enumerate()
            .find_map(|(i, l)| l.find("\"0.5 furlong\"").map(|c| (i + 1, c + 1)))
            .unwrap();
        let err = load(text.as_bytes()).unwrap_err();
        let LoadError::Schema { path, at, message } = &err else {
            panic!("a schema error, not {err:?}");
        };
        assert_eq!(path, "operations[0].depth_mm");
        let at = at.unwrap();
        assert_eq!((at.line, at.column), (line, column));
        assert!(message.contains("0.5 furlong"), "{message}");
        let shown = err.to_string();
        assert!(shown.contains("operations[0].depth_mm"), "{shown}");
        assert!(
            shown.contains(&format!("line {line}, column {column}")),
            "{shown}"
        );
    }

    #[test]
    fn length_unit_on_rate_field_is_error() {
        let err = tool_with_feed(json!("60 in")).unwrap_err().to_string();
        assert!(err.contains("a feed rate"), "{err}");
        assert!(err.contains("in/min"), "{err}");
        let err = load_depth(json!("60 in/min")).unwrap_err().to_string();
        assert!(err.contains("a length"), "{err}");
    }

    #[test]
    fn rejects_what_is_not_a_decimal() {
        for bad in [
            "1e3 mm", "inf mm", "NaN mm", ". mm", "1..2 mm", "0x10 mm", "in", "",
        ] {
            assert!(load_depth(json!(bad)).is_err(), "{bad}");
        }
        let huge = format!("{} in", "9".repeat(400));
        let err = load_depth(json!(huge)).unwrap_err().to_string();
        assert!(err.contains("too large"), "{err}");
        assert_approx_eq!(load_depth(json!("-2 mm")).unwrap(), -2.0, 0.0);
        assert_approx_eq!(load_depth(json!(".5 in")).unwrap(), 12.7, 0.0);
    }

    /// Every persisted length and rate takes a unit string, in every place
    /// the model has one: the project, the stock, the origin, an instance,
    /// a tool, each operation kind, a lead, tabs and a peck.
    #[test]
    fn every_length_field_takes_units() {
        let mut root = sample_json();
        root["flatten_tolerance_mm"] = json!("0.01 mm");
        root["arc_fit_tolerance_mm"] = json!("0.01 mm");
        root["stock"]["width_mm"] = json!("4 in");
        root["stock"]["thickness_mm"] = json!("1/4 in");
        root["origin"]["xy"] = json!({ "custom": { "x_mm": "1 in", "y_mm": "2 mm" } });
        root["instances"][0]["z_top_mm"] = json!("1/4 in");
        root["instances"][0]["z_bottom_mm"] = json!("0 in");
        let tool = &mut root["tools"][0];
        tool["diameter_mm"] = json!("1/4 in");
        tool["plunge_mm_min"] = json!("20 in/min");
        tool["step_down_mm"] = json!("1/16 in");
        let op = &mut root["operations"][0];
        op["step_down_mm"] = json!("2 mm");
        op["safe_z_mm"] = json!("1/4 in");
        op["through_cut_allowance_mm"] = json!("0 mm");
        op["lead"] = json!({
            "radius_mm": "1/8 in", "sweep_deg": 90.0, "lead_out": true, "overcut_mm": "1 mm"
        });
        op["tabs"]["width_mm"] = json!("1/4 in");
        op["tabs"]["height_mm"] = json!("2 mm");
        op["tabs"]["placement"] = json!({ "kind": "at_distances", "distances_mm": ["1 in", 10] });
        let project = load_json(&root).unwrap();
        assert_approx_eq!(project.stock.width_mm, 101.6, 1e-12);
        assert_approx_eq!(project.stock.thickness_mm.unwrap(), 6.35, 1e-12);
        assert_approx_eq!(project.tools[0].plunge_mm_min.unwrap(), 508.0, 1e-9);
        let Operation::Profile(op) = &project.operations[0] else {
            panic!("the sample's operation is a Profile");
        };
        assert_approx_eq!(op.lead.as_ref().unwrap().radius_mm, 3.175, 1e-12);
        let Some(crate::TabPlacement::AtDistances { distances_mm }) =
            op.tabs.as_ref().map(|t| &t.placement)
        else {
            panic!("at_distances tabs");
        };
        assert_approx_eq!(distances_mm[0], 25.4, 0.0);
        assert_approx_eq!(distances_mm[1], 10.0, 0.0);
    }

    /// The other operation kinds' own lengths: Pocket's allowance, Drill's
    /// retract, peck depth and chip-break gap.
    #[test]
    fn pocket_and_drill_lengths_take_units() {
        use crate::test_support::{sample_drill, sample_pocket};
        let mut project = sample_project();
        project.operations.push(Operation::Pocket(sample_pocket()));
        project.operations.push(Operation::Drill(sample_drill()));
        let mut root = serde_json::to_value(&project).unwrap();
        root["operations"][1]["allowance_mm"] = json!("1/64 in");
        root["operations"][2]["retract_mm"] = json!("1 mm");
        root["operations"][2]["cycle"] = json!({
            "peck": { "peck_depth_mm": "1/16 in", "retract": { "chip_break": { "gap_mm": "0.02 in" } } }
        });
        let project = load_json(&root).unwrap();
        let Operation::Pocket(pocket) = &project.operations[1] else {
            panic!("a Pocket");
        };
        assert_approx_eq!(pocket.allowance_mm, 25.4 / 64.0, 1e-12);
        let Operation::Drill(drill) = &project.operations[2] else {
            panic!("a Drill");
        };
        let crate::DrillCycle::Peck {
            peck_depth_mm,
            retract: crate::PeckRetract::ChipBreak { gap_mm },
        } = &drill.cycle
        else {
            panic!("a chip-break peck");
        };
        assert_approx_eq!(peck_depth_mm.unwrap(), 1.5875, 1e-12);
        assert_approx_eq!(*gap_mm, 0.508, 1e-12);
    }

    /// Fields in other units take numbers only.
    #[test]
    fn other_units_take_numbers_only() {
        let mut root = sample_json();
        root["tools"][0]["step_over_fraction"] = json!("0.4 in");
        assert!(load_json(&root).is_err());
    }
}
