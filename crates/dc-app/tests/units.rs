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

/// The value at `key` of `op`'s record as millimetres: a number, or a
/// list's one number.
fn stored_mm(op: &dc_project::Operation, key: &str) -> Option<f64> {
    let record = dc_app::editor::fields::record(op).ok()?;
    match dc_app::editor::fields::get(&record, key)? {
        serde_json::Value::Array(values) if values.len() == 1 => values[0].as_f64(),
        value => value.as_f64(),
    }
}

/// `op` after `text` is committed into `slot` in `project`, or why not.
fn committed(
    project: &dc_project::Project,
    op: &dc_project::Operation,
    slot: &dc_app::editor::Slot<'_>,
    text: &str,
) -> Result<dc_project::Operation, String> {
    match slot.commit_text(project, op, text) {
        Ok(Some(Command::UpdateOperation(next))) => Ok(next),
        Ok(None) => Ok(op.clone()),
        Ok(Some(other)) => Err(format!("committed {other:?}")),
        Err(e) => Err(e.to_string()),
    }
}

/// The typed slots of `op` measured in lengths or feed rates: its own
/// rows and each clearing pass's.
fn length_and_rate_slots(op: &dc_project::Operation) -> Vec<dc_app::editor::Slot<'static>> {
    use dc_app::editor::fields::{FieldKind, Unit};
    let typed = |slot: &dc_app::editor::Slot<'_>| {
        matches!(slot.row.unit(), Unit::Length | Unit::Rate)
            && !matches!(slot.row.kind, FieldKind::CountersinkDepth)
    };
    let mut slots = Vec::new();
    for row in dc_app::editor::fields::rows(op) {
        if matches!(row.kind, FieldKind::ClearingList) {
            for index in 0..dc_app::editor::clearing::passes(op, row).len() {
                slots.extend(
                    dc_app::editor::clearing::rows(op, row, index)
                        .into_iter()
                        .filter(|s| typed(s)),
                );
            }
        }
        let slot = dc_app::editor::Slot::of(row);
        if typed(&slot) {
            slots.push(slot);
        }
    }
    slots
}

/// `op` and the records the table's choices and switches make of it, one
/// row at a time and on down through the rows each new record shows (a
/// Profile's start point, a peck's chip break), as `tests/editor.rs`
/// reaches every row.
fn variants(
    project: &dc_project::Project,
    op: &dc_project::Operation,
) -> Vec<dc_project::Operation> {
    use dc_app::editor::{self, fields::FieldKind};
    let mut found = vec![op.clone()];
    let mut next = 0;
    while let Some(op) = found.get(next).cloned() {
        next += 1;
        for row in dc_app::editor::fields::rows(&op) {
            let made: Vec<Option<Command>> = match row.kind {
                FieldKind::Choice(options) => (0..options.len())
                    .map(|i| editor::choose(project, &op, row, i).ok().flatten())
                    .collect(),
                FieldKind::Toggle(_) | FieldKind::TabRequest => [true, false]
                    .into_iter()
                    .map(|on| editor::switch(project, &op, row, on).ok().flatten())
                    .collect(),
                _ => Vec::new(),
            };
            for command in made {
                if let Some(Command::UpdateOperation(variant)) = command
                    && !found.contains(&variant)
                    && found.len() < 400
                {
                    found.push(variant);
                }
            }
        }
    }
    found
}

/// Acceptance 89 over the editor's field table (capability 53): in every
/// operation kind (each kind's new operation on the sign, and every
/// quick start's and golden project's operations), every length and feed
/// rate field, clearing passes' included, takes a unit string in a mm
/// project (`1/4 in` stores 6.35, `10 in/min` stores 254), takes a bare
/// number in an inch project as inches (`0.5` stores 12.7), takes `mm` in
/// an inch project, and shows the stored value back in inches with `in`.
/// The Project panel's and the Tools tab's lengths and rates do the same.
#[test]
fn every_length_and_rate_field_accepts_unit_strings() {
    use dc_app::editor::fields::Unit;
    use dc_app::operations::{self, Kind};

    let strings = dc_app::Strings::english();
    let sign = common::load_dcv(&common::fixture("sign.dcv"));
    let targets: Vec<_> = sign.instances.iter().map(|i| i.id.clone()).collect();
    let mut seeds: Vec<(dc_project::Project, dc_project::Operation)> = Kind::ALL
        .into_iter()
        .filter_map(|kind| operations::new_operation(&sign, &strings, kind, &targets))
        .map(|op| (sign.clone(), op))
        .collect();
    for (_, path) in common::quick_start_projects()
        .into_iter()
        .chain(common::golden_projects())
    {
        let project = common::load_dcv(&path);
        seeds.extend(
            project
                .operations
                .iter()
                .map(|op| (project.clone(), op.clone())),
        );
    }

    let seeds: Vec<_> = seeds
        .iter()
        .flat_map(|(project, op)| {
            variants(project, op)
                .into_iter()
                .map(|variant| (project.clone(), variant))
        })
        .collect();
    let mut checked = std::collections::BTreeSet::new();
    for (project, op) in &seeds {
        let mut mm = project.clone();
        mm.display_units = Units::Mm;
        let mut inch = project.clone();
        inch.display_units = Units::Inch;
        for slot in length_and_rate_slots(op) {
            let case = format!("{} `{}`", op.id(), slot.key());
            let (typed, bare, millimetres, stored, want, suffix) = match slot.row.unit() {
                Unit::Rate => ("10 in/min", "10", "254 mm/min", 254.0, 254.0, "in/min"),
                _ => ("1/4 in", "0.5", "12.7 mm", 6.35, 12.7, "in"),
            };
            let next = committed(&mm, op, &slot, typed)
                .unwrap_or_else(|e| panic!("{case}: `{typed}` in a mm project: {e}"));
            assert_close(
                stored_mm(&next, slot.key()).unwrap_or(f64::NAN),
                stored,
                &format!("{case}: `{typed}` in a mm project"),
            );
            for text in [bare, millimetres] {
                let next = committed(&inch, op, &slot, text)
                    .unwrap_or_else(|e| panic!("{case}: `{text}` in an inch project: {e}"));
                assert_close(
                    stored_mm(&next, slot.key()).unwrap_or(f64::NAN),
                    want,
                    &format!("{case}: `{text}` in an inch project"),
                );
                assert_eq!(
                    slot.value_text(&inch, &next),
                    bare,
                    "{case} shows inches in an inch project"
                );
            }
            assert_eq!(
                dc_app::editor::unit_suffix(slot.row, Units::Inch),
                suffix,
                "{case}'s unit"
            );
            checked.insert(format!("{:?} {}", Kind::of(op), slot.row.key));
        }
    }
    let mut unreached = Vec::new();
    for kind in Kind::ALL {
        for row in dc_app::editor::fields::table(kind) {
            let typed = matches!(row.unit(), Unit::Length | Unit::Rate)
                && !matches!(
                    row.kind,
                    dc_app::editor::fields::FieldKind::CountersinkDepth
                );
            if typed && !checked.contains(&format!("{kind:?} {}", row.key)) {
                unreached.push(format!("{kind:?} {}", row.key));
            }
        }
    }
    assert!(
        unreached.is_empty(),
        "every length and rate row of the table was typed into: {unreached:?}"
    );

    // The Project panel's lengths.
    use dc_app::project::{Field, Quantity};
    let plaque = common::plaque();
    let mut inch = plaque.clone();
    inch.display_units = Units::Inch;
    for field in [
        Field::StockWidth,
        Field::StockHeight,
        Field::StockThickness,
        Field::OriginX,
        Field::OriginY,
        Field::Clearance,
        Field::ToolChangeZ,
        Field::FlattenTolerance,
        Field::ArcFitTolerance,
    ] {
        assert_eq!(field.quantity(), Quantity::Length);
        for (project, text) in [(&plaque, "1/4 in"), (&inch, "0.25"), (&inch, "6.35 mm")] {
            let mut harness = Harness::from_project(project.clone());
            let command = field
                .set(project, text)
                .unwrap_or_else(|e| panic!("{field:?} `{text}`: {e}"))
                .unwrap_or_else(|| panic!("{field:?} `{text}` changes it"));
            harness.run(command).expect("the field's command runs");
            let shown = if project.display_units == Units::Inch {
                "0.25"
            } else {
                "6.35"
            };
            assert_eq!(
                field.text(harness.app.project()),
                shown,
                "{field:?} `{text}`"
            );
        }
        assert_eq!(field.suffix(Units::Inch), "in", "{field:?}");
    }
    let park = Field::Park
        .set(&inch, "1/4 in, 12.7 mm")
        .expect("the park point takes unit strings")
        .expect("and changes");
    let mut harness = Harness::from_project(inch.clone());
    harness.run(park).expect("the park runs");
    assert_eq!(Field::Park.text(harness.app.project()), "0.25, 0.5");

    // The Tools tab's lengths and rates.
    use dc_app::tools::{self, ToolField};
    let tool = &plaque.tools[1];
    for (field, typed, stored, shown) in [
        (ToolField::Diameter, "1/4 in", 6.35, "0.25"),
        (ToolField::StepDown, "1/8 in", 3.175, "0.125"),
        (ToolField::Feed, "60 in/min", 1524.0, "60"),
        (ToolField::Plunge, "20 in/min", 508.0, "20"),
    ] {
        let next = tools::set_field(tool, field, typed, Units::Mm)
            .unwrap_or_else(|e| panic!("{field:?} `{typed}`: {e}"));
        let value = match field {
            ToolField::Diameter => Some(next.diameter_mm),
            ToolField::StepDown => next.step_down_mm,
            ToolField::Feed => next.feed_mm_min,
            _ => next.plunge_mm_min,
        };
        assert_close(
            value.unwrap_or(f64::NAN),
            stored,
            &format!("{field:?} `{typed}`"),
        );
        assert_eq!(
            tools::field_text(&next, field, Units::Inch),
            shown,
            "{field:?}"
        );
        let again = tools::set_field(tool, field, shown, Units::Inch)
            .unwrap_or_else(|e| panic!("{field:?} `{shown}` in inches: {e}"));
        assert_eq!(
            tools::field_text(&again, field, Units::Inch),
            shown,
            "{field:?}"
        );
    }
}
