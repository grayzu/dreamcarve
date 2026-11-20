// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every field of the app has help (the maintainer's smoke test,
//! 2026-11-20, item 6; functional spec, capability 66): each row of every
//! table names a help string that says more than its label, and where a
//! help string names a limit, the limit is the validator's.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::editor::fields;
use dc_app::operations::Kind;
use dc_app::tools::ToolField;
use dc_app::{StrKey, Strings, project, properties};
use dc_project::Problem;

/// Every field of every table, as (where it is, its label, its help).
fn every_field() -> Vec<(String, StrKey, StrKey)> {
    let mut all = Vec::new();
    for field in project::Field::ALL {
        all.push((format!("project {field:?}"), field.label(), field.help()));
    }
    for choice in project::Choice::ALL {
        all.push((format!("project {choice:?}"), choice.label(), choice.help()));
    }
    for field in ToolField::ALL {
        all.push((format!("tool {field:?}"), field.label(), field.help()));
    }
    for field in properties::FIELDS {
        all.push((format!("shape {field:?}"), field.label(), field.help()));
        all.push((
            format!("picture {field:?}"),
            field.label(),
            field.picture_help(),
        ));
    }
    for kind in Kind::ALL {
        for row in fields::table(kind) {
            all.push((format!("{kind:?} {}", row.key), row.label, row.help));
        }
    }
    for row in fields::CLEARING_PASS {
        all.push((format!("clearing pass {}", row.key), row.label, row.help));
    }
    // The rows that are not a table's: the units, the fit margin, a
    // tool's type, the library's own controls, a tab's and a picture's
    // Properties, the sheets' fields and the Export step's.
    for (label, help) in [
        (StrKey::ProjectUnits, StrKey::ProjectUnitsHelp),
        (StrKey::ProjectStockMargin, StrKey::ProjectStockMarginHelp),
        (StrKey::AppToolsKind, StrKey::AppToolsKindHelp),
        (StrKey::AppLibrarySearch, StrKey::AppLibrarySearchTip),
        (StrKey::AppToolsKind, StrKey::AppLibraryFilterTip),
        (StrKey::PlacementTabWidth, StrKey::PlacementTabWidthHelp),
        (StrKey::PlacementTabHeight, StrKey::PlacementTabHeightHelp),
        (StrKey::PlacementTabShape, StrKey::PlacementTabShapeHelp),
        (
            StrKey::PlacementTabPosition,
            StrKey::PlacementTabPositionHelp,
        ),
        (
            StrKey::PlacementPictureName,
            StrKey::PlacementPictureNameHelp,
        ),
        (
            StrKey::PlacementPicturePixels,
            StrKey::PlacementPicturePixelsHelp,
        ),
        (
            StrKey::PlacementPictureLocked,
            StrKey::PlacementPictureLockedHelp,
        ),
        (StrKey::ArrangeAngle, StrKey::ArrangeAngleHelp),
        (StrKey::AppPictureWidth, StrKey::AppPictureWidthHelp),
        (StrKey::AppPictureHeight, StrKey::AppPictureHeightHelp),
        (
            StrKey::AppImportJoinTolerance,
            StrKey::AppImportJoinToleranceHelp,
        ),
        (StrKey::ExportDestination, StrKey::ExportDestinationHelp),
        (StrKey::ExportStem, StrKey::ExportStemHelp),
        (StrKey::ExportBoards, StrKey::ExportBoardsHelp),
    ] {
        all.push((format!("{label:?}"), label, help));
    }
    all
}

/// Each field's help is there, is a sentence of its own (it ends with a
/// full stop and says more than the label does), and holds no leftover
/// placeholder.
#[test]
fn every_field_of_every_table_has_help() {
    let strings = Strings::english();
    let fields = every_field();
    assert!(
        fields.len() > 200,
        "the tables are walked: {}",
        fields.len()
    );
    for (at, label, help) in fields {
        let (label, help) = (strings.get(label), strings.get(help));
        assert!(!label.is_empty(), "{at}: a label");
        assert!(
            help.len() >= 30 && help.len() > label.len() + 20,
            "{at}: its help says what the field is: `{help}`"
        );
        assert!(help.ends_with('.'), "{at}: a sentence: `{help}`");
        assert!(!help.contains('{'), "{at}: no placeholder left: `{help}`");
        assert_ne!(help, label, "{at}");
    }
}

/// Whether validation finds a problem located at `field` in `project`.
fn refused(project: &dc_project::Project, field: &str) -> bool {
    dc_project::validate(project)
        .iter()
        .filter(|p| p.severity() == dc_project::Severity::Error)
        .filter_map(Problem::field)
        .any(|at| at == field)
}

/// A help string that names a limit names the validator's: the number in
/// the sentence is the constant validation checks against, and a value at
/// the limit is taken while one past it is refused.
#[test]
fn a_help_string_names_the_validators_range() {
    let strings = Strings::english();
    let help = |key: StrKey| strings.get(key).to_owned();

    // The limits validation exports, each named in its field's help.
    for (key, limit) in [
        (
            StrKey::AppToolsFeedHelp,
            format!("at most {} mm/min", dc_project::MAX_RATE_MM_MIN),
        ),
        (
            StrKey::AppToolsPlungeHelp,
            format!("at most {} mm/min", dc_project::MAX_RATE_MM_MIN),
        ),
        (
            StrKey::AppToolsRpmHelp,
            format!("over {} gets a warning", dc_project::MAX_RPM),
        ),
        (
            StrKey::AppToolsDiameterHelp,
            format!("Over {} mm", dc_project::MAX_TOOL_DIAMETER_MM),
        ),
        (
            StrKey::AppToolsTaperHelp,
            format!("at most {}", dc_project::MAX_TAPER_ANGLE_DEG),
        ),
        (
            StrKey::ProjectTolerancesFlattenHelp,
            format!("at most {} mm", dc_project::MAX_FLATTEN_TOLERANCE_MM),
        ),
        (
            StrKey::ProjectPostExtensionHelp,
            format!("1 to {} letters", dc_project::MAX_FILE_EXTENSION_CHARS),
        ),
        (
            StrKey::ProjectStockWidthHelp,
            format!("at most {} mm", dc_geometry::MAX_COORDINATE_MM),
        ),
        (
            StrKey::ProjectStockHeightHelp,
            format!("at most {} mm", dc_geometry::MAX_COORDINATE_MM),
        ),
        (
            StrKey::ProjectStockThicknessHelp,
            format!("at most {} mm", dc_geometry::MAX_COORDINATE_MM),
        ),
        (
            StrKey::ProjectOriginXHelp,
            format!("from -{0} to {0} mm", dc_geometry::MAX_COORDINATE_MM),
        ),
    ] {
        assert!(
            help(key).contains(&limit),
            "{key:?} names `{limit}`: `{}`",
            help(key)
        );
    }

    // And the limits hold as the sentences say, on the plaque.
    let plaque = common::plaque();
    assert!(
        !dc_project::validate(&plaque)
            .iter()
            .any(|p| p.severity() == dc_project::Severity::Error)
    );

    // The stock's width: above 0 and at most 6000 mm.
    let stock = |width_mm: f64| {
        let mut project = plaque.clone();
        project.stock.width_mm = width_mm;
        refused(&project, "stock.width_mm")
    };
    assert!(!stock(dc_geometry::MAX_COORDINATE_MM), "the limit is taken");
    assert!(stock(dc_geometry::MAX_COORDINATE_MM + 1.0));
    assert!(stock(0.0), "above 0");

    // A tool's stepover: above 0 and at most 100 %.
    assert!(help(StrKey::AppToolsStepOverHelp).contains("above 0 and at most 100 %"));
    let stepover = |fraction: f64| {
        let mut project = plaque.clone();
        project.tools[1].step_over_fraction = Some(fraction);
        let at = format!("tools[{}].step_over_fraction", project.tools[1].id);
        refused(&project, &at)
    };
    assert!(!stepover(1.0), "100 % is taken");
    assert!(stepover(1.01));
    assert!(stepover(0.0), "above 0");
    assert!(!stepover(0.0008), "a tiny stepover is in range");

    // A tool's feed: above 0 and at most 20000 mm/min.
    let feed = |feed_mm_min: f64| {
        let mut project = plaque.clone();
        project.tools[1].feed_mm_min = Some(feed_mm_min);
        project.tools[1].plunge_mm_min = Some(1.0);
        let at = format!("tools[{}].feed_mm_min", project.tools[1].id);
        refused(&project, &at)
    };
    assert!(!feed(dc_project::MAX_RATE_MM_MIN));
    assert!(feed(dc_project::MAX_RATE_MM_MIN + 1.0));
    assert!(feed(0.0));

    // A V-bit's angle: above 0 and below 180.
    assert!(help(StrKey::AppToolsAngleHelp).contains("above 0 and below 180"));
    let angle = |included_angle_deg: f64| {
        let mut project = plaque.clone();
        project.tools[1].cutter = dc_project::ToolKind::VBit { included_angle_deg };
        let at = format!(
            "tools[{}].cutter.v_bit.included_angle_deg",
            project.tools[1].id
        );
        refused(&project, &at)
    };
    assert!(!angle(179.0));
    assert!(angle(180.0));
    assert!(angle(0.0));

    // A file extension: 1 to 8 letters or digits, never dcv.
    let extension = |text: &str| {
        let mut project = plaque.clone();
        project.post_processor.file_extension = text.to_owned();
        refused(&project, "post_processor.file_extension")
    };
    assert!(!extension("gcode123"), "8 characters");
    assert!(extension("gcode1234"), "9");
    assert!(extension("dcv"));
    assert!(extension(""));
}
