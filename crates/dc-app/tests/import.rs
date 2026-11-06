// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 6 of phase 4 (#535): the Import drawing sheet's model
//! (`docs/specs/phase-4/design.md`, "Test plan → Import"; functional
//! spec, capability 55 and acceptance 64). The sheet reads the bytes the
//! shell hands it with `dc-import`, as `dreamcarve init` does with its
//! flags: the DXF units (required when the file declares none), the join
//! tolerance and the layers. What it imports is what the importer returns
//! for those options, placed as drawn, as one undo step, and its notes
//! reach the warnings once.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::import::{DrawingFormat, DrawingImport, Purpose};
use dc_app::test_support::Harness;
use dc_app::{App, Command, Selection};
use dc_import::{DxfImportOptions, DxfUnits, ImportWarning, JOIN_TOLERANCE_DEFAULT_MM};
use dc_job::{Drawing, Tier};
use dc_project::{Stock, Units};

use common::{fixture, read};

/// The sheet over `tests/fixtures/<path>`, to import into the open
/// project.
fn sheet(path: &str) -> DrawingImport {
    let file = fixture(path);
    let name = file.file_name().unwrap().to_string_lossy().into_owned();
    let format = if path.ends_with(".dxf") {
        DrawingFormat::Dxf
    } else {
        DrawingFormat::Svg
    };
    DrawingImport::open(format, &name, read(&file), Purpose::Import)
}

/// What `dc_import::import_dxf` returns for `tests/fixtures/<path>` with
/// `options`: what `dreamcarve init` reads for the same flags.
fn imported(path: &str, options: &DxfImportOptions) -> (dc_import::DxfImport, Vec<ImportWarning>) {
    dc_import::import_dxf(&read(&fixture(path)), options).expect("the fixture imports")
}

/// The DXF the sheet would import, or `None` when *Import* is disabled.
fn sheet_dxf(sheet: &DrawingImport) -> Option<dc_import::DxfImport> {
    match sheet.command()? {
        Command::ImportDrawing {
            drawing: Drawing::Dxf(import),
            ..
        } => Some(import),
        other => panic!("not a DXF import: {other:?}"),
    }
}

/// An empty project, as *New* makes it.
fn empty() -> Harness {
    Harness::from_project(App::new_project().expect("a new project"))
}

/// Acceptance 64: a DXF with no `$INSUNITS` disables *Import* until units
/// are chosen, with the message the CLI prints for `init` without
/// `--units`. Choosing them imports what `init --units` reads; a file that
/// declares units reads them, and the dropdown overrides them.
#[test]
fn dxf_without_units_blocks_until_chosen() {
    let mut sheet = sheet("dxf/no_units.dxf");
    assert!(sheet.units_required(), "the file declares no units");
    assert_eq!(sheet.declared_units(), None);
    // The importer's refusal, which `init` prints after `cannot import
    // <file>: ` (its `--units` hint names a flag the app does not have).
    let refusal = dc_import::import_dxf(
        &read(&fixture("dxf/no_units.dxf")),
        &DxfImportOptions::new(),
    )
    .unwrap_err()
    .to_string();
    assert!(refusal.contains("$INSUNITS"), "{refusal}");
    assert_eq!(sheet.blocked().as_deref(), Some(refusal.as_str()));
    assert!(sheet.command().is_none(), "Import is disabled");
    assert!(sheet.summary().is_none(), "nothing is read yet");

    for units in [Units::Inch, Units::Mm] {
        sheet.set_units(Some(units));
        assert_eq!(sheet.blocked(), None, "{units:?} chosen");
        let options = DxfImportOptions {
            units: Some(units),
            ..DxfImportOptions::new()
        };
        assert_eq!(
            sheet_dxf(&sheet),
            Some(imported("dxf/no_units.dxf", &options).0),
            "{units:?}: what `init --units` reads"
        );
        assert!(sheet.summary().is_some());
        assert!(
            sheet.units_required(),
            "still the file's lack, not the choice"
        );
    }
    sheet.set_units(None);
    assert_eq!(sheet.blocked().as_deref(), Some(refusal.as_str()));
    assert!(
        sheet.command().is_none(),
        "unset again, Import is disabled again"
    );

    // The plaque declares millimetres: read from the file, overridable.
    let mut plaque = self::sheet("plaque.dxf");
    assert!(!plaque.units_required());
    assert_eq!(plaque.declared_units(), Some(DxfUnits::Millimetres));
    assert_eq!(plaque.blocked(), None);
    assert_eq!(
        sheet_dxf(&plaque),
        Some(imported("plaque.dxf", &DxfImportOptions::new()).0)
    );
    plaque.set_units(Some(Units::Inch));
    let inch = DxfImportOptions {
        units: Some(Units::Inch),
        ..DxfImportOptions::new()
    };
    assert_eq!(sheet_dxf(&plaque), Some(imported("plaque.dxf", &inch).0));

    // The join tolerance starts at the importer's 0.01 mm and is passed on.
    assert!((plaque.join_tolerance_mm() - JOIN_TOLERANCE_DEFAULT_MM).abs() < 1e-12);
    plaque.set_units(None);
    plaque.set_join_tolerance_mm(0.0);
    let exact = DxfImportOptions {
        join_tolerance_mm: 0.0,
        ..DxfImportOptions::new()
    };
    assert_eq!(sheet_dxf(&plaque), Some(imported("plaque.dxf", &exact).0));
    plaque.set_join_tolerance_mm(-1.0);
    assert!(
        plaque
            .blocked()
            .is_some_and(|m| m.contains("join tolerance")),
        "a negative tolerance is the importer's refusal: {:?}",
        plaque.blocked()
    );

    // An SVG has no options.
    let svg = self::sheet("sign.svg");
    assert_eq!(svg.format(), DrawingFormat::Svg);
    assert!(!svg.units_required());
    assert!(svg.layers().is_empty());
    assert_eq!(svg.blocked(), None);
}

/// Acceptance 64: every layer is listed with what it holds and starts
/// ticked; ticking one layer imports that layer only, as `init --layer`
/// does; with none ticked there is nothing to import. Imported into an
/// empty project, the shapes are the ones `init` reads, named by layer,
/// placed as drawn and selected.
#[test]
fn layer_subset() {
    let mut sheet = sheet("plaque.dxf");
    let (all, _) = imported("plaque.dxf", &DxfImportOptions::new());
    assert_eq!(sheet.layers(), all.layers.as_slice());
    let names: Vec<String> = sheet.layers().iter().map(|l| l.name.clone()).collect();
    assert!(names.len() > 1, "the plaque has several layers: {names:?}");
    assert!(
        names.iter().all(|n| sheet.is_ticked(n)),
        "every layer starts ticked"
    );
    let summary = sheet.summary().expect("the plaque reads");
    assert_eq!(summary.layers, names.len());
    assert_eq!(summary.closed + summary.open, all.definitions.len());
    assert_eq!(summary.closed, 9, "init targets nine closed shapes");
    assert_eq!(summary.open, 1, "and leaves one open path");

    for name in names.iter().filter(|n| *n != "holes") {
        sheet.set_layer(name, false);
    }
    assert!(sheet.is_ticked("holes"));
    let holes = DxfImportOptions {
        layers: Some(vec!["holes".to_owned()]),
        ..DxfImportOptions::new()
    };
    let (expected, _) = imported("plaque.dxf", &holes);
    assert_eq!(
        sheet_dxf(&sheet),
        Some(expected.clone()),
        "init --layer holes"
    );
    assert_eq!(
        sheet.layers(),
        all.layers.as_slice(),
        "the list is the file's"
    );
    let summary = sheet.summary().unwrap();
    assert_eq!((summary.closed, summary.open), (4, 0));

    let mut harness = empty();
    let label = harness
        .app
        .import_drawing(&sheet)
        .expect("the holes import");
    assert_eq!(label, "Import plaque.dxf");
    let project = harness.app.project();
    let got: Vec<&str> = project.instances.iter().map(|i| i.name.as_str()).collect();
    let want: Vec<&str> = expected.instances.iter().map(|i| i.name.as_str()).collect();
    assert_eq!(got, want, "named by layer, as init names them");
    let (placed, ids) =
        dc_job::drawing_into(&App::new_project().unwrap(), Drawing::Dxf(expected.clone()));
    assert_eq!(project, &placed, "placed as drawn");
    assert_eq!(harness.app.selection(), &Selection::Instances(ids));

    sheet.set_layer("holes", false);
    assert!(
        sheet.command().is_none(),
        "no layer ticked, nothing to import"
    );
    assert!(sheet.blocked().is_some());
    sheet.set_layer("holes", true);
    assert!(sheet.command().is_some());
}

/// Acceptance 64: entities the importer does not read are listed on their
/// layer, a layer of nothing else has no shapes to import, and the
/// warning reaches the app's warnings once, under the import's name, and
/// stays once through later edits and generation. Opening a project
/// clears it.
#[test]
fn unread_entities_noted_once() {
    let sheet = sheet("dxf/unsupported.dxf");
    let notes = sheet
        .layers()
        .iter()
        .find(|l| l.name == "notes")
        .expect("the notes layer is listed");
    assert_eq!(notes.closed + notes.open, 0, "nothing on it is read");
    let unread: Vec<(&str, usize)> = notes
        .skipped
        .iter()
        .map(|(k, n)| (k.as_str(), *n))
        .collect();
    assert_eq!(unread, [("MTEXT", 1), ("TEXT", 1)]);
    let skipped: Vec<&ImportWarning> = sheet
        .warnings()
        .iter()
        .filter(|w| matches!(w, ImportWarning::DxfEntitiesSkipped { .. }))
        .collect();
    assert_eq!(skipped.len(), 1, "one warning for every unread entity");
    let warning = skipped[0].to_string();

    let noted = |harness: &Harness| -> Vec<dc_job::Finding> {
        harness
            .warnings()
            .iter()
            .filter(|f| f.message.contains("dxf_entities_skipped"))
            .cloned()
            .collect()
    };
    let mut harness = empty();
    assert!(noted(&harness).is_empty());
    harness
        .app
        .import_drawing(&sheet)
        .expect("the file imports");
    let found = noted(&harness);
    assert_eq!(found.len(), 1, "{found:?}");
    assert_eq!(found[0].tier, Tier::Warning);
    assert!(found[0].message.contains(&warning), "{}", found[0].message);
    assert!(
        found[0].message.contains("unsupported.dxf"),
        "under the import's name: {}",
        found[0].message
    );

    harness
        .run(Command::SetStock(Stock {
            width_mm: 200.0,
            height_mm: 150.0,
            thickness_mm: Some(12.0),
        }))
        .expect("stock");
    harness.generate();
    assert_eq!(noted(&harness).len(), 1, "still once after an edit");

    harness
        .app
        .open(&read(&fixture("plaque.dcv")), None)
        .expect("the plaque opens");
    assert!(noted(&harness).is_empty(), "another project, no notes");
}

/// Acceptance 64: one import is one undo step, labelled with the file's
/// name; undo returns the project as it was, redo brings the shapes back.
/// Into a project that has shapes, the new ones are appended under new
/// ids and become the selection.
#[test]
fn one_import_is_one_undo_step() {
    let mut harness = Harness::open(&read(&fixture("plaque.dcv"))).expect("the plaque opens");
    let before = harness.app.project().clone();
    let sheet = sheet("plaque.dxf");
    harness
        .app
        .import_drawing(&sheet)
        .expect("the drawing imports");
    let after = harness.app.project().clone();
    let (all, _) = imported("plaque.dxf", &DxfImportOptions::new());
    let (expected, ids) = dc_job::drawing_into(&before, Drawing::Dxf(all));
    assert_eq!(after, expected);
    assert_eq!(harness.app.selection(), &Selection::Instances(ids));
    assert_eq!(
        after.instances.len(),
        before.instances.len() + sheet.summary().map_or(0, |s| s.closed + s.open)
    );
    assert_eq!(harness.app.undo_label(), Some("Import plaque.dxf"));

    assert_eq!(harness.app.undo().as_deref(), Some("Import plaque.dxf"));
    assert_eq!(harness.app.project(), &before, "one step undoes the import");
    assert!(!harness.app.can_undo(), "and it was the only step");
    assert_eq!(harness.app.redo().as_deref(), Some("Import plaque.dxf"));
    assert_eq!(harness.app.project(), &after);
}

/// Acceptance 65: importing `fixtures/photo/gradient.png` at width 100
/// adds the bitmap `image add --width 100` adds, at the origin, with the
/// grayscale note for a colour file.
#[test]
#[ignore = "needs phase 3.4's Bitmap (format 26); see #544"]
fn picture_at_width_equals_image_add() {
    // The picture half of slice 6 waits for `dc_project::Bitmap`, which
    // phase 3.4 adds with format 26 (main is at format 25), and for
    // `dc_job::project_from::picture` and `Command::ImportPicture` (#544).
    panic!("Import picture is not built: there is no Bitmap type yet");
}
