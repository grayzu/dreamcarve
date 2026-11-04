// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 2 of phase 4: units display and entry (functional acceptance 89).
//! Lengths show in the project's units with the unit after the value;
//! typed values parse as the file parses them, a bare number in the
//! project's units; the file keeps millimetres whatever is displayed.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::test_support::Harness;
use dc_app::{Command, units};
use dc_project::Units;

use common::plaque_bytes;

fn assert_close(actual: f64, expected: f64, case: &str) {
    assert!(
        (actual - expected).abs() < 1e-9,
        "{case}: {actual} is not {expected}"
    );
}

/// Acceptance 89: an inch project shows every length in inches with `in`;
/// a mm project shows `mm`; negative zero prints as zero; feed rates take
/// the per-minute unit.
#[test]
fn inch_project_displays_inches() {
    assert_eq!(units::format(25.4, Units::Inch, 3), "1.000 in");
    assert_eq!(units::format(6.35, Units::Inch, 4), "0.2500 in");
    assert_eq!(units::format(6.35, Units::Mm, 2), "6.35 mm");
    assert_eq!(units::format(12.0, Units::Mm, 1), "12.0 mm");
    assert_eq!(units::format(-0.0001, Units::Mm, 2), "0.00 mm");
    assert_eq!(units::format(-0.0, Units::Inch, 3), "0.000 in");
    assert_eq!(units::format_rate(1524.0, Units::Inch, 1), "60.0 in/min");
    assert_eq!(units::format_rate(1500.0, Units::Mm, 0), "1500 mm/min");
    assert_eq!(units::suffix(Units::Inch), "in");
    assert_eq!(units::suffix(Units::Mm), "mm");
    assert_close(
        units::to_display(25.4, Units::Inch),
        1.0,
        "25.4 mm in inches",
    );
    assert_close(units::to_display(25.4, Units::Mm), 25.4, "25.4 mm in mm");

    // Every length of an inch project displays in inches: a sample of the
    // plaque's fields, switched to inches.
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness
        .run(Command::SetDisplayUnits(Units::Inch))
        .expect("units");
    let project = harness.app.project();
    let shown = units::format(project.stock.width_mm, project.display_units, 3);
    assert!(shown.ends_with(" in"), "{shown}");
    let displayed: f64 = shown.trim_end_matches(" in").parse().unwrap();
    let exact = project.stock.width_mm / 25.4;
    assert!(
        (displayed - exact).abs() <= 0.0005,
        "stock width: {displayed} is not {exact} to three places"
    );
}

/// Acceptance 89: `1/4 in` in a mm project stores 6.35; `0.5` in an inch
/// project stores 12.7; unit strings mean what they mean in a file
/// (capability 32), including mixed numbers; a rate takes `in/min`; a
/// length does not take a rate's unit; and switching the display and back
/// leaves the file unchanged.
#[test]
fn unit_strings_parse_as_the_file_does() {
    let parse = |text: &str, project_units: Units| {
        units::parse_length_mm(text, project_units)
            .unwrap_or_else(|e| panic!("{text:?} in {project_units:?}: {e}"))
    };
    assert_close(parse("1/4 in", Units::Mm), 6.35, "1/4 in");
    assert_close(parse("0.5", Units::Inch), 12.7, "0.5 in an inch project");
    assert_close(parse("0.5", Units::Mm), 0.5, "0.5 in a mm project");
    assert_close(parse("1 1/4 in", Units::Mm), 31.75, "1 1/4 in");
    assert_close(
        parse("12.7 mm", Units::Inch),
        12.7,
        "12.7 mm in an inch project",
    );
    assert_close(parse("1/4in", Units::Mm), 6.35, "1/4in without a space");
    assert_close(
        parse("  3/16 in ", Units::Mm),
        4.7625,
        "3/16 in with spaces",
    );
    assert_close(
        parse("1/2", Units::Inch),
        12.7,
        "a bare fraction in an inch project",
    );

    // "As the file does": the same string on a field of a .dcv loads to
    // the same millimetres.
    let mut json: serde_json::Value = serde_json::from_slice(&plaque_bytes()).unwrap();
    json["flatten_tolerance_mm"] = serde_json::Value::String("3/16 in".into());
    let loaded = dc_project::load(serde_json::to_string(&json).unwrap().as_bytes())
        .unwrap()
        .project;
    assert_eq!(
        parse("3/16 in", Units::Mm).to_bits(),
        loaded.flatten_tolerance_mm.to_bits(),
        "the app and the file parse differently"
    );

    assert!(units::parse_length_mm("abc", Units::Mm).is_err());
    assert!(units::parse_length_mm("", Units::Mm).is_err());
    assert!(units::parse_length_mm("1500 mm/min", Units::Mm).is_err());
    assert!(units::parse_length_mm("inf", Units::Mm).is_err());
    assert!(units::parse_length_mm("nan mm", Units::Mm).is_err());

    let rate = |text: &str, project_units: Units| {
        units::parse_rate_mm_min(text, project_units)
            .unwrap_or_else(|e| panic!("{text:?} in {project_units:?}: {e}"))
    };
    assert_close(rate("60 in/min", Units::Mm), 1524.0, "60 in/min");
    assert_close(
        rate("1000", Units::Inch),
        25400.0,
        "1000 in an inch project",
    );
    assert_close(rate("1500", Units::Mm), 1500.0, "1500 in a mm project");
    assert!(units::parse_rate_mm_min("6.35 mm", Units::Mm).is_err());

    // The stored value is millimetres and the display does not touch it.
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let before = harness.app.serialize().unwrap();
    harness
        .run(Command::SetTolerances {
            flatten_mm: parse("1/4 in", Units::Mm),
            arc_fit_mm: harness.app.project().arc_fit_tolerance_mm,
        })
        .expect("tolerances");
    assert!(
        harness
            .app
            .serialize()
            .unwrap()
            .contains("\"flatten_tolerance_mm\": 6.35"),
        "the file holds 6.35"
    );
    harness.app.undo();
    harness
        .run(Command::SetDisplayUnits(Units::Inch))
        .expect("units");
    harness
        .run(Command::SetDisplayUnits(Units::Mm))
        .expect("units");
    assert_eq!(
        harness.app.serialize().unwrap(),
        before,
        "switching the display and back changed the file"
    );
}
