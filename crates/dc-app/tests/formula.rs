// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Formulas in fields (the maintainer's request, 2026-11-25): a field
//! that takes a number takes a sum, `1.5 + .375 / 2`, and holds what it
//! comes to. Nothing of the formula is kept: no variables, no memory.
//! PureCutCNC has nothing of the kind to port.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::formula;
use dc_app::units;
use dc_project::Units;

/// What `text` resolves to, as a field's parser then reads it.
fn resolved(text: &str) -> String {
    formula::resolved(text).unwrap().into_owned()
}

/// The rules of the sum: `*` and `/` before `+` and `-`, brackets, a
/// sign before a number or a bracket, and a decimal with or without its
/// leading zero.
#[test]
fn a_formula_resolves_to_its_number() {
    assert_eq!(resolved("1.5 + .375 / 2"), "1.6875");
    assert_eq!(resolved("(1.5 + .375) / 2"), "0.9375");
    assert_eq!(resolved("2*3+4"), "10");
    assert_eq!(resolved("2 * (3 + 4)"), "14");
    assert_eq!(resolved("10 - 4 - 3"), "3", "left to right");
    assert_eq!(resolved("100 / 4 / 5"), "5");
    assert_eq!(resolved("-3 + 5"), "2");
    assert_eq!(resolved("5 * -2"), "-10");
    assert_eq!(resolved("-(2 + 3)"), "-5");
    assert_eq!(resolved("((2))+1."), "3");
    assert_eq!(resolved("3 × 4 ÷ 6"), "2", "the signs a keyboard may have");
    // To nine places, with no tail of binary noise.
    assert_eq!(resolved("0.1 + 0.2"), "0.3");
    assert_eq!(resolved("1 / 3"), "0.333333333");
    assert_eq!(resolved("5 - 5"), "0");
}

/// A unit is written once, after the sum, and stays with what the sum
/// comes to; so does a percent sign or a degree sign.
#[test]
fn the_unit_after_a_formula_is_the_results() {
    assert_eq!(resolved("1.5 + .375 / 2 in"), "1.6875 in");
    assert_eq!(resolved("(25.4 * 2)mm"), "50.8 mm");
    assert_eq!(resolved("100 / 3 %"), "33.333333333 %");
    assert_eq!(resolved("360 / 7 °"), "51.428571429 °");
    assert_eq!(resolved("90 / 2 deg"), "45 deg");
    assert_eq!(resolved("30 * 2 in/min"), "60 in/min");
}

/// What is no formula is handed on as typed, to mean what it meant: a
/// number, with its unit or not; a fraction and a mixed number, which
/// the file's grammar reads exactly; and text no sum could be, for the
/// field to refuse in its own words.
#[test]
fn what_is_no_formula_is_left_as_typed() {
    for plain in [
        "12",
        "-12.5",
        ".5",
        "+3",
        "3 mm",
        "1/4",
        "1/4 in",
        "1 1/2 in",
        "-1 1/2",
        "60 in/min",
        "50 %",
        "45°",
        "",
        "  ",
        "abc",
        "1,5",
        "1e-3",
    ] {
        assert_eq!(resolved(plain), plain, "{plain:?}");
    }
    // In a sum, a fraction is a division and a mixed number a number.
    assert_eq!(resolved("1/4 + 1/8"), "0.375");
    assert_eq!(resolved("1 1/2 + 3/8 in"), "1.875 in");
    assert_eq!(resolved("2 * 1 3/4"), "3.5");
}

/// A field of plain decimals (a percentage, an angle, a count) has no
/// fractions of its own, so there a fraction is a sum too.
#[test]
fn a_fraction_is_a_sum_where_the_field_has_no_fractions() {
    let decimal = |text: &str| formula::resolved_decimal(text).unwrap().into_owned();
    assert_eq!(decimal("360/7"), "51.428571429");
    assert_eq!(decimal("1/3 %"), "0.333333333 %");
    assert_eq!(decimal("3*4"), "12");
    assert_eq!(decimal("45"), "45");
    assert_eq!(decimal("-2.5 °"), "-2.5 °");
}

/// A sum that cannot be worked out says why, in the field: nothing is
/// guessed.
#[test]
fn a_formula_that_cannot_be_worked_out_says_why() {
    let why = |text: &str| formula::resolved(text).unwrap_err();
    assert_eq!(
        why("1.5 +"),
        "`1.5 +` is not finished: a number is missing at its end"
    );
    assert_eq!(why("(1 + 2"), "`(1 + 2` has a bracket that is never closed");
    assert_eq!(
        why("1 + 2)"),
        "`1 + 2)` closes a bracket that was never opened"
    );
    assert_eq!(why("4 / (2 - 2)"), "`4 / (2 - 2)` divides by zero");
    assert_eq!(
        why("2 (3 + 4)"),
        "`2 (3 + 4)` is not a sum this field can work out: use numbers with + - * / and brackets"
    );
    assert_eq!(
        why("1 + * 2"),
        "`1 + * 2` is not a sum this field can work out: use numbers with + - * / and brackets"
    );
    assert_eq!(
        why("1 in + 3 mm"),
        "`1 in + 3 mm` has a unit inside it: write the unit once, at the end, as in `(1 + 3) mm`"
    );
}

/// In a field: a length in the project's units or with a unit of its
/// own, and a rate. An inch project reads a bare sum in inches.
#[test]
fn a_length_and_a_rate_take_a_formula() {
    let mm = |text: &str| units::parse_length_mm(text, Units::Mm).unwrap();
    assert!((mm("1.5 + .375 / 2") - 1.6875).abs() < 1e-12);
    assert!((mm("(1 + 1/2) in") - 38.1).abs() < 1e-9);
    let inch = |text: &str| units::parse_length_mm(text, Units::Inch).unwrap();
    assert!((inch("1.5 + .375 / 2") - 42.8625).abs() < 1e-9);
    assert!((inch("3/4 - 1/16") - 17.4625).abs() < 1e-9);
    assert!((inch("10 + 2.7 mm") - 12.7).abs() < 1e-9, "its own unit");
    let rate = units::parse_rate_mm_min("1500 / 2", Units::Mm).unwrap();
    assert!((rate - 750.0).abs() < 1e-9);
    // Refused with the sum's own reason.
    let refused = units::parse_length_mm("3 +", Units::Mm).unwrap_err();
    assert!(refused.to_string().contains("is not finished"), "{refused}");
    // What it meant before, it means still.
    assert!((mm("1/4 in") - 6.35).abs() < 1e-12);
    assert!((inch("1 1/2") - 38.1).abs() < 1e-12);
}

/// Every kind of field takes one: an operation's depth, percentage and
/// count; a tool's rate, speed and stepover; a size typed while drawing;
/// a selection's width; a rotation. What the field then holds is the
/// number, and the file never sees the sum.
#[test]
fn every_kind_of_field_takes_a_formula() {
    use dc_app::editor::{self, fields};
    use dc_app::test_support::Harness;
    use dc_app::tool::{Event, SizeField};
    use dc_app::tools::{self, ToolField};
    use dc_app::{Drawing, Pointer, Tool, properties};
    use dc_project::Operation;

    let mut h = Harness::from_project(common::plaque());
    let row = |op: &Operation, key: &str| {
        fields::rows(op)
            .into_iter()
            .find(|r| r.key == key)
            .unwrap_or_else(|| panic!("no row {key}"))
    };
    // An operation's fields: the Profile's depth, the Panel's stepover.
    let profile = h.app.project().operations[4].clone();
    let command = editor::commit_text(
        h.app.project(),
        &profile,
        row(&profile, "depth_mm"),
        "5.5 + 1 / 2",
    )
    .unwrap()
    .unwrap();
    h.run(command).unwrap();
    assert_eq!(h.app.project().operations[4].depth_mm(), Some(6.0));
    let pocket = h.app.project().operations[2].clone();
    let command = editor::commit_text(
        h.app.project(),
        &pocket,
        row(&pocket, "step_over_fraction"),
        "90 / 2 %",
    )
    .unwrap()
    .unwrap();
    h.run(command).unwrap();
    let Operation::Pocket(panel) = &h.app.project().operations[2] else {
        panic!("the Panel is a Pocket");
    };
    assert_eq!(panel.step_over_fraction, Some(0.45));
    let saved = h.app.serialize().unwrap();
    assert!(!saved.contains('+'), "the file holds numbers, no sum");

    // A tool's: a rate, a whole number, a percentage.
    let tool = h.app.project().tools[1].clone();
    let fed = tools::set_field(&tool, ToolField::Feed, "3000 / 2", Units::Mm).unwrap();
    assert_eq!(fed.feed_mm_min, Some(1500.0));
    let spun = tools::set_field(&tool, ToolField::Rpm, "9000 * 2", Units::Mm).unwrap();
    assert_eq!(spun.rpm, Some(18000));
    let stepped = tools::set_field(&tool, ToolField::StepOver, "100 / 4", Units::Mm).unwrap();
    assert_eq!(stepped.step_over_fraction, Some(0.25));
    assert!(
        tools::set_field(&tool, ToolField::Rpm, "9000 / 7", Units::Mm).is_err(),
        "a speed is a whole number still"
    );

    // A selection's width in Properties.
    let ids = [dc_project::InstanceId::new("i4")];
    let placement = properties::placement(h.app.project(), &ids).unwrap();
    let edit =
        properties::parse(&placement, properties::Field::W, "25 + 15", Units::Mm, true).unwrap();
    let command = properties::edit(h.app.project(), &ids, edit).unwrap();
    h.run(command).unwrap();
    let placement = properties::placement(h.app.project(), &ids).unwrap();
    assert_eq!(
        properties::field_text(&placement, properties::Field::W, Units::Mm),
        "40"
    );
    // A rotation.
    assert_eq!(dc_app::arrange::parse_angle("360 / 8").unwrap(), 45.0);

    // A size typed while drawing: a rectangle from a click and two sums.
    h.app.select(dc_app::Selection::Nothing);
    h.app.prefs_mut().snap.enabled = false;
    h.app.arm(Tool::Draw(Drawing::Rectangle));
    let at = Pointer::at(dc_geometry::Point::new(10.0, 10.0));
    h.app.tool_event(Event::Move(at));
    h.app.tool_event(Event::Press(at));
    h.app.tool_event(Event::Release(at));
    for (field, text) in [
        (SizeField::Width, "1.5 + .375 / 2"),
        (SizeField::Height, "2 * 3"),
    ] {
        h.app.tool_event(Event::Field {
            field,
            text: text.to_owned(),
        });
    }
    h.app.tool_event(Event::Key(dc_app::tool::Key::Enter));
    let drawn = h.app.project().instances.last().unwrap().clone();
    let definition = &h.app.project().definitions[&drawn.definition];
    assert_eq!(
        definition.shape,
        Some(dc_project::Shape::Rectangle {
            width_mm: 1.6875,
            height_mm: 6.0,
            corner: dc_project::Corner::Square,
        })
    );
}
