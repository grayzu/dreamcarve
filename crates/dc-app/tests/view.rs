// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 16 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan";
//! #545): the toolpath display's view state. *By depth*'s ramp and legend
//! (functional spec, capability 73, acceptance 80), and the visibility
//! controls as view state that never reaches the file or the history
//! (acceptance 81).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::test_support::Harness;
use dc_app::view::{ColourMode, DEPTH_STEPS, Visibility, depth_legend, operation_key};
use dc_app::{Command, Selection};
use dc_geometry::{Point, approx_eq};
use dc_project::OperationId;

use common::plaque_bytes;

/// The plaque's legend ticks as the legend writes them.
fn tick_texts(harness: &Harness) -> Vec<String> {
    let legend = depth_legend(harness.app.project(), harness.app.scene()).expect("the plaque cuts");
    legend
        .ticks
        .iter()
        .map(|t| t.text(harness.app.strings()))
        .collect()
}

/// The plaque with a clearance height of 10 mm and every operation's safe
/// Z at 5 mm (the fixture leaves the clearance unset and retracts to
/// 15 mm), and Z zero where `z` says.
fn plaque_with_heights(z: &str) -> Harness {
    let text = String::from_utf8(plaque_bytes()).expect("UTF-8");
    let text = text
        .replace("\"safe_z_mm\": 15.0", "\"safe_z_mm\": 5.0")
        .replace("\"clearance_mm\": null", "\"clearance_mm\": 10.0")
        .replace("\"z\": \"stock_top\"", &format!("\"z\": \"{z}\""));
    let mut harness = Harness::open(text.as_bytes()).expect("the plaque opens");
    harness.generate();
    harness
}

/// Capability 73a, acceptance 80 (#549, row 13): above the stock top the
/// legend has the band where the bit moves without cutting, with ticks at
/// the clearance height and each distinct safe Z; then the stock top and
/// each operation's depth, in work coordinates with an explicit sign.
/// With Z zero at the stock bottom the stock top reads `+11.8` and the
/// cuts count up from the bottom: every one at or above it but the
/// through cuts, which go the through-cut allowance (0.2 mm) below.
#[test]
fn depth_legend_ticks_include_clearance_and_safe_z() {
    let harness = plaque_with_heights("stock_top");
    assert_eq!(
        tick_texts(&harness),
        [
            "+10 clearance",
            "+5 safe Z",
            "0 stock top",
            "\u{2212}1 Border and squiggle",
            "\u{2212}4 Panel",
            "\u{2212}6 Square recess",
            "\u{2212}12 Screw holes, Cut out",
        ]
    );
    let legend = depth_legend(harness.app.project(), harness.app.scene()).unwrap();
    let above: Vec<bool> = legend.ticks.iter().map(|t| t.fraction < 0.0).collect();
    assert_eq!(
        above,
        [true, true, false, false, false, false, false],
        "the clearance and safe Z sit in the band above the ramp"
    );
    assert!(
        legend.ticks[0].fraction < legend.ticks[1].fraction,
        "clearance above safe Z"
    );

    let harness = plaque_with_heights("stock_bottom");
    assert_eq!(
        tick_texts(&harness),
        [
            "+21.8 clearance",
            "+16.8 safe Z",
            "+11.8 stock top",
            "+10.8 Border and squiggle",
            "+7.8 Panel",
            "+5.8 Square recess",
            "\u{2212}0.2 Screw holes, Cut out",
        ]
    );

    // The fixture as committed: no clearance height set, so the band has
    // the safe Z alone.
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness.generate();
    assert_eq!(&tick_texts(&harness)[..2], ["+15 safe Z", "0 stock top"]);
}

/// Acceptance 80: on the plaque (stock 11.8 mm thick, top at Z 11.8), the
/// cutting ramp's ticks are the stock top, each operation's depth and the
/// deepest cut: the engraving at 1, the Panel pocket at 4, the Square
/// recess at 6 and the bores and the cut-out at 12 (each cuts 12 mm
/// through the 11.8 mm stock), the deepest. *By depth* colours the Panel
/// pocket's floor moves by their Z: each sits at the Panel's tick on the
/// ramp, a third of the way down, and every shallower pass of it above
/// that.
#[test]
fn depth_legend_labels_stock_top_each_depth_and_deepest() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness.generate();
    let project = harness.app.project().clone();
    let scene = harness.app.scene();
    let legend = depth_legend(&project, scene).expect("the plaque cuts");
    let top = project.stock.thickness_mm.unwrap();
    assert!(approx_eq(legend.ramp.top_z_mm, top, 1e-9));
    assert!(approx_eq(legend.ramp.deepest_z_mm, top - 12.0, 1e-6));

    let cuts: Vec<_> = legend.ticks.iter().filter(|t| t.fraction >= 0.0).collect();
    let labels: Vec<(&str, Vec<&str>)> = cuts
        .iter()
        .map(|t| (t.label.as_str(), t.operations().collect()))
        .collect();
    assert_eq!(
        labels,
        [
            ("0", vec![]),
            ("\u{2212}1", vec!["Border and squiggle"]),
            ("\u{2212}4", vec!["Panel"]),
            ("\u{2212}6", vec!["Square recess"]),
            ("\u{2212}12", vec!["Screw holes", "Cut out"]),
        ]
    );
    let flags: Vec<(bool, bool)> = cuts.iter().map(|t| (t.top, t.deepest)).collect();
    assert_eq!(
        flags,
        [
            (true, false),
            (false, false),
            (false, false),
            (false, false),
            (false, true)
        ]
    );
    for (tick, depth) in cuts.iter().zip([0.0, 1.0, 4.0, 6.0, 12.0]) {
        assert!(
            approx_eq(tick.fraction, depth / 12.0, 1e-6),
            "{}: at {} on the ramp",
            tick.label,
            tick.fraction
        );
    }

    // The Panel pocket's floor moves take the Panel tick's colour.
    let panel = scene.operation(&OperationId::new("op1")).unwrap();
    let floor_z = top - 4.0;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss,
        reason = "a fraction of 255"
    )]
    let panel_step = (cuts[2].fraction * (DEPTH_STEPS - 1) as f64).round() as u8;
    let mut floor_moves = 0;
    for run in panel.runs.iter() {
        for m in run.run.toolpath.moves.iter().filter(|m| !m.is_rapid()) {
            let step = legend.ramp.step(m.to.z);
            if approx_eq(m.to.z, floor_z, 1e-6) {
                assert_eq!(step, panel_step, "a floor move at {:?}", m.to);
                floor_moves += 1;
            } else {
                assert!(step < panel_step, "a pass above the floor: {:?}", m.to);
            }
        }
    }
    assert!(floor_moves > 10, "{floor_moves} floor moves");
}

/// An inch project's legend is in inches: the unit once, in its title.
#[test]
fn depth_legend_is_in_project_units() {
    let mut project = common::plaque();
    project.display_units = dc_project::Units::Inch;
    let mut harness = Harness::from_project(project);
    harness.generate();
    let legend = depth_legend(harness.app.project(), harness.app.scene()).unwrap();
    assert_eq!(legend.units, dc_project::Units::Inch);
    let labels: Vec<&str> = legend.ticks.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(labels.first(), Some(&"+0.591"), "15 mm is 0.591 in");
    assert_eq!(labels.last(), Some(&"\u{2212}0.472"), "12 mm is 0.472 in");
}

/// Capability 73 (`experience.md`, section 9, "Toolpath view popover"):
/// the by-operation key lists one entry per enabled operation in list
/// order, named as the file names it, in the colour the Viewport draws
/// it: the preview palette's, by the operation's index in the Scene. A
/// disabled operation has no entry and takes no colour.
#[test]
fn by_operation_key_lists_each_operation_colour() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness.generate();
    let key = operation_key(harness.app.scene());
    let names: Vec<&str> = key.iter().map(|e| e.name.as_str()).collect();
    let file: Vec<&str> = harness
        .app
        .project()
        .operations
        .iter()
        .map(|o| o.name())
        .collect();
    assert_eq!(names, file, "every operation, in list order");
    let colours: Vec<usize> = key.iter().map(|e| e.colour).collect();
    assert_eq!(colours, [0, 1, 2, 3, 4], "the palette by index");
    for (entry, op) in key.iter().zip(harness.app.scene().operations()) {
        assert_eq!(entry.id, op.id);
        assert_eq!(entry.colour, op.colour, "the colour the Viewport draws");
    }

    // Disable the Panel pocket: it leaves the key, the rest close up.
    harness
        .run(Command::SetOperationEnabled {
            id: OperationId::new("op1"),
            enabled: false,
        })
        .unwrap();
    harness.generate();
    let key = operation_key(harness.app.scene());
    let names: Vec<&str> = key.iter().map(|e| e.name.as_str()).collect();
    assert!(!names.contains(&"Panel"), "{names:?}");
    assert_eq!(names.len(), 4);
    let colours: Vec<usize> = key.iter().map(|e| e.colour).collect();
    assert_eq!(colours, [0, 1, 2, 3]);
}

/// Acceptance 81 (capability 73): toggling every visibility control, both
/// colour modes, *Only the selected operation*, the extents, an
/// operation's eye, *Show all* and *Hide all*, and panning and zooming,
/// leaves the file unchanged on save and adds nothing to the undo
/// history; nothing regenerates. The settings that persist reach the
/// preferences `gui.toml` keeps; the eyes and the camera are the
/// session's.
#[test]
fn visibility_toggles_change_no_file_and_no_history() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness.generate();
    let saved = harness.save().unwrap();
    let revision = harness.app.scene().revision();
    assert!(!harness.app.can_undo() && !harness.app.can_redo());

    let flips: [fn(&mut Visibility); 9] = [
        |v| v.cuts = !v.cuts,
        |v| v.rapids = !v.rapids,
        |v| v.plunges = !v.plunges,
        |v| v.arrows = !v.arrows,
        |v| v.start_points = !v.start_points,
        |v| v.tab_marks = !v.tab_marks,
        |v| v.grid = !v.grid,
        |v| v.extents = !v.extents,
        |v| v.only_selected = !v.only_selected,
    ];
    for flip in flips {
        let mut visibility = harness.app.view().visibility;
        flip(&mut visibility);
        harness.app.set_visibility(visibility);
        assert_eq!(harness.app.view().visibility, visibility);
        assert_eq!(
            harness.app.prefs().visibility,
            visibility,
            "remembered between sessions"
        );
    }
    harness.app.set_colour(ColourMode::ByDepth);
    assert_eq!(harness.app.prefs().colour, ColourMode::ByDepth);
    harness.app.set_colour(ColourMode::ByOperation);

    // The eyes: one hidden, Hide all, Show all.
    let panel = OperationId::new("op1");
    harness.app.view_mut().hidden_ops.insert(panel.clone());
    let selection = Selection::Nothing;
    assert!(!harness.app.view().shows(&panel, &selection));
    let scene = harness.app.scene().clone();
    harness.app.view_mut().hide_all(&scene);
    assert!(
        scene
            .operations()
            .iter()
            .all(|o| !harness.app.view().shows(&o.id, &selection)),
        "Hide all hides every operation"
    );
    harness.app.view_mut().show_all();
    let mut visibility = harness.app.view().visibility;
    visibility.only_selected = false;
    harness.app.set_visibility(visibility);
    assert!(
        scene
            .operations()
            .iter()
            .all(|o| harness.app.view().shows(&o.id, &selection)),
        "Show all shows every operation"
    );

    // Only the selected operation.
    visibility.only_selected = true;
    harness.app.set_visibility(visibility);
    let pocket = Selection::Operation(panel.clone());
    assert!(harness.app.view().shows(&panel, &pocket));
    assert!(!harness.app.view().shows(&OperationId::new("op4"), &pocket));

    // Pan and zoom.
    let camera = &mut harness.app.view_mut().camera;
    camera.centre_mm = Point::new(12.0, 34.0);
    camera.px_per_mm *= 3.0;

    assert_eq!(harness.save().unwrap(), saved, "the file is unchanged");
    assert!(!harness.app.is_dirty());
    assert!(
        !harness.app.can_undo() && !harness.app.can_redo(),
        "nothing is in the history"
    );
    assert!(harness.app.pending().is_empty(), "nothing regenerates");
    assert_eq!(harness.app.scene().revision(), revision, "nor rebuilds");

    // A real edit still is a step, so the history was live all along.
    harness
        .run(Command::SetDisplayUnits(dc_project::Units::Inch))
        .unwrap();
    assert!(harness.app.can_undo());
}
