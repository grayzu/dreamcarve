// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 4 of phase 4 (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app", `Scene`; #533): the read model the Viewport draws and `pick`
//! reads. It holds every visible instance's outlines in project space,
//! placed by the instance's transform as the generators place them, and
//! is rebuilt when the snapshot or a generation result changes, never
//! for the selection, the camera or any other view state.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::test_support::Harness;
use dc_app::viewport::camera::Pos;
use dc_app::{Command, Selection};
use dc_geometry::{EPS_MM, Point, Transform, approx_eq};

use common::plaque;

/// The Scene follows the snapshot: built once with the app, rebuilt by
/// each command, undo and redo, and by each generation result the app
/// keeps; untouched by selecting, by moving the camera and by the other
/// view settings, which change every frame.
#[test]
fn rebuilds_only_when_the_snapshot_changes() {
    let mut harness = Harness::from_project(plaque());
    let project = harness.app.project().clone();
    let scene = harness.app.scene();
    let built = scene.revision();

    // Every visible instance, in file order (the last drawn on top), its
    // outlines the definition's profiles placed by its transform.
    let ids: Vec<_> = scene.shapes().iter().map(|s| s.instance.clone()).collect();
    let file_order: Vec<_> = project.instances.iter().map(|i| i.id.clone()).collect();
    assert_eq!(ids, file_order);
    for (shape, instance) in scene.shapes().iter().zip(&project.instances) {
        let definition = &project.definitions[&instance.definition];
        assert_eq!(shape.outlines.len(), definition.profiles.len());
        assert_eq!(shape.name, instance.name);
        for (outline, profile) in shape.outlines.iter().zip(&definition.profiles) {
            let placed = instance.transform.apply_polyline(profile).unwrap();
            assert_eq!(
                outline, &placed,
                "{} placed as the generators place it",
                shape.instance
            );
        }
    }
    // The placed tabs, at their project-space points.
    assert_eq!(scene.tabs().len(), project.tabs.len());
    for (mark, tab) in scene.tabs().iter().zip(&project.tabs) {
        let instance = project
            .instances
            .iter()
            .find(|i| i.id == tab.instance)
            .unwrap();
        let want = instance.transform.apply(Point::new(tab.x_mm, tab.y_mm));
        assert_eq!(mark.id, tab.id);
        assert!(approx_eq(mark.at_mm.x, want.x, EPS_MM) && approx_eq(mark.at_mm.y, want.y, EPS_MM));
    }

    // View state does not rebuild it.
    let first = project.instances[0].id.clone();
    harness
        .app
        .select(Selection::Instances(vec![first.clone()]));
    harness.app.select(Selection::Nothing);
    let view = harness.app.view_mut();
    view.camera.zoom_about(Pos::new(10.0, 10.0), 2.0);
    view.camera.pan(40.0, -3.0);
    view.only_selected = true;
    view.visibility.rapids = false;
    assert_eq!(
        harness.app.scene().revision(),
        built,
        "selection and view keep the Scene"
    );

    // A command does, and the Scene reads the new snapshot.
    harness
        .run(Command::TransformInstances {
            ids: vec![first.clone()],
            delta: Transform::translate(5.0, 0.0),
        })
        .unwrap();
    let moved = harness.app.scene().revision();
    assert_ne!(moved, built, "a command rebuilds the Scene");
    let now = harness.app.project().instances[0].transform;
    assert_eq!(
        harness.app.scene().shapes()[0].outlines[0],
        now.apply_polyline(&project.definitions[&project.instances[0].definition].profiles[0])
            .unwrap()
    );
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![first.clone()],
            visible: Some(false),
            locked: None,
        })
        .unwrap();
    assert!(
        harness.app.scene().shape(&first).is_none(),
        "hidden is absent"
    );
    let hidden = harness.app.scene().revision();
    assert_ne!(hidden, moved);

    // Undo and redo are snapshot changes too.
    harness.app.undo();
    assert!(harness.app.scene().shape(&first).is_some());
    let undone = harness.app.scene().revision();
    assert_ne!(undone, hidden);
    harness.app.redo();
    assert_ne!(harness.app.scene().revision(), undone);
    harness.app.undo();

    // A generation result the app keeps rebuilds it (its operations'
    // stale flags change); a frame with nothing new does not.
    let before_results = harness.app.scene().revision();
    assert!(
        harness.app.scene().operations().iter().all(|o| o.stale),
        "nothing is generated yet"
    );
    harness.generate();
    let generated = harness.app.scene().revision();
    assert_ne!(generated, before_results, "results rebuild the Scene");
    assert!(
        harness.app.scene().operations().iter().any(|o| !o.stale),
        "the results are in"
    );
    harness.generate();
    assert_eq!(
        harness.app.scene().revision(),
        generated,
        "nothing new, no rebuild"
    );
}
