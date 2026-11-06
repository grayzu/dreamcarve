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
use dc_app::view::{ColourMode, DEPTH_STEPS, Visibility, depth_legend};
use dc_app::{Command, Selection};
use dc_geometry::{Point, approx_eq};
use dc_project::OperationId;

use common::plaque_bytes;

/// Acceptance 80: on the plaque (stock 11.8 mm thick, top at Z 11.8), the
/// legend's ticks are the stock top, each operation's depth and the
/// deepest cut, in project units with the unit once at the top: `0 mm`,
/// then the engraving at 1, the Panel pocket at 4, the Square recess at 6
/// and the bores and the cut-out at 12 (each cuts 12 mm through the
/// 11.8 mm stock), the deepest. *By depth* colours the Panel pocket's
/// floor moves by their Z: each sits at the Panel's tick on the ramp, a
/// third of the way down, and every shallower pass of it above that.
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

    let labels: Vec<(&str, Vec<&str>)> = legend
        .ticks
        .iter()
        .map(|t| {
            (
                t.label.as_str(),
                t.operations.iter().map(String::as_str).collect(),
            )
        })
        .collect();
    assert_eq!(
        labels,
        [
            ("0 mm", vec![]),
            ("\u{2212}1", vec!["Border and squiggle"]),
            ("\u{2212}4", vec!["Panel"]),
            ("\u{2212}6", vec!["Square recess"]),
            ("\u{2212}12", vec!["Screw holes", "Cut out"]),
        ]
    );
    let flags: Vec<(bool, bool)> = legend.ticks.iter().map(|t| (t.top, t.deepest)).collect();
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
    for (tick, depth) in legend.ticks.iter().zip([0.0, 1.0, 4.0, 6.0, 12.0]) {
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
    let panel_step = (legend.ticks[2].fraction * (DEPTH_STEPS - 1) as f64).round() as u8;
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

/// An inch project's legend is in inches, the unit once at the top.
#[test]
fn depth_legend_is_in_project_units() {
    let mut project = common::plaque();
    project.display_units = dc_project::Units::Inch;
    let mut harness = Harness::from_project(project);
    harness.generate();
    let legend = depth_legend(harness.app.project(), harness.app.scene()).unwrap();
    let labels: Vec<&str> = legend.ticks.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(labels.first(), Some(&"0 in"));
    assert_eq!(labels.last(), Some(&"\u{2212}0.472"), "12 mm is 0.472 in");
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
