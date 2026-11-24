// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 5 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan → Tree
//! and placement"; #534): the Properties panel's placement fields
//! (functional capability 58, acceptance 66) and their mixed values for a
//! multi-selection (PureCutCNC's `mixedValue.ts`, whose own cases are
//! ported beside `dc_app::mixed`). Slice 25 (#617) leaves X, Y, W and H
//! with the proportion link to their right, and a locked selection's
//! note with *Unlock*.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::f64::consts::FRAC_PI_2;

use dc_app::properties::{self, Field, Placement};
use dc_app::test_support::Harness;
use dc_app::{Command, CommandError};
use dc_geometry::{Point, Rect, Transform};
use dc_project::{InstanceId, OperationId, Project, Units};

use common::{assert_near, assert_same_transform, coaster, instance_of, plaque_bytes};

/// Transform coefficients agree to this, mm or dimensionless.
const TOL: f64 = 1e-9;

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn centre(rect: Rect) -> Point {
    Point::new(
        (rect.min.x + rect.max.x) / 2.0,
        (rect.min.y + rect.max.y) / 2.0,
    )
}

/// `delta` about `pivot`, as `TransformInstances` applies it.
fn about(pivot: Point, delta: Transform) -> Transform {
    Transform::translate(-pivot.x, -pivot.y)
        .then(delta)
        .then(Transform::translate(pivot.x, pivot.y))
}

fn placement_of(project: &Project, ids: &[InstanceId]) -> Placement {
    properties::placement(project, ids).expect("the selection has a placement")
}

/// Types `text` into `field` for `ids` as the panel does, and runs the
/// command it makes. `Ok(false)` when the value was already there.
fn type_into(
    harness: &mut Harness,
    ids: &[InstanceId],
    field: Field,
    text: &str,
    uniform: bool,
) -> Result<bool, CommandError> {
    let project = harness.app.project().clone();
    let placement = placement_of(&project, ids);
    let edit = properties::parse(&placement, field, text, project.display_units, uniform)?;
    match properties::edit(&project, ids, edit) {
        Ok(command) => harness.run(command).map(|_| true),
        Err(CommandError::NothingToDo) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Acceptance 66 (capability 58): with the coaster selected, typing X 10
/// and Y 5 puts its box's bottom-left there, by a translation and nothing
/// else; W 50 % is the scale about the box's centre; with the proportion
/// link broken, H scales one axis only; each is one undo step and undo
/// restores the transform exactly; a locked instance refuses each.
/// Rotation and mirroring are Arrange actions (58a, `tests/arrange.rs`).
/// On the plaque, a typed move regenerates the toolpath the CLI generates
/// for the saved file.
#[test]
fn typed_x_y_w_h_produce_the_expected_transforms() {
    let mut harness = Harness::from_project(coaster());
    let ids: Vec<InstanceId> = harness
        .app
        .project()
        .instances
        .iter()
        .map(|i| i.id.clone())
        .collect();
    let first = ids[0].as_str().to_owned();
    let transform = |h: &Harness| instance_of(h.app.project(), &first).transform;
    let start = placement_of(harness.app.project(), &ids);
    assert_eq!(properties::field_text(&start, Field::X, Units::Mm), "10");
    assert_eq!(properties::field_text(&start, Field::W, Units::Mm), "90");

    // X 10, Y 5: a translation by what moves the box's corner there.
    let t0 = transform(&harness);
    let steps = harness.app.history().undo_labels().count();
    type_into(&mut harness, &ids, Field::X, "10", true).expect("X");
    type_into(&mut harness, &ids, Field::Y, "5", true).expect("Y");
    let moved = placement_of(harness.app.project(), &ids);
    assert_near(
        moved.bounds.min,
        Point::new(10.0, 5.0),
        1e-9,
        "the box's corner",
    );
    let dx = 10.0 - start.bounds.min.x;
    let dy = 5.0 - start.bounds.min.y;
    assert_same_transform(
        transform(&harness),
        t0.then(Transform::translate(dx, dy)),
        TOL,
        "X and Y",
    );
    assert!(
        (moved.bounds.width_mm() - start.bounds.width_mm()).abs() < 1e-9,
        "a move does not scale"
    );
    let moves = harness.app.history().undo_labels().count() - steps;
    assert!(moves <= 2, "one undo step per typed value, {moves} steps");

    let before = transform(&harness);
    let c = centre(moved.bounds);

    // W 50 %, proportions kept: a uniform scale by a half.
    type_into(&mut harness, &ids, Field::W, "50 %", true).expect("W");
    assert_same_transform(
        transform(&harness),
        before.then(about(c, Transform::scale(0.5, 0.5))),
        TOL,
        "W 50 %",
    );
    let scaled = placement_of(harness.app.project(), &ids);
    assert_near(centre(scaled.bounds), c, 1e-9, "the centre stays");
    assert!(
        (scaled.bounds.height_mm() - start.bounds.height_mm() / 2.0).abs() < 1e-9,
        "H follows W"
    );
    assert_eq!(harness.app.undo_label(), Some("Scale “coaster”"));
    harness.app.undo();
    assert_eq!(transform(&harness), before, "undo restores it exactly");

    // A locked selection refuses every edit, naming the shape.
    harness
        .run(Command::SetInstanceFlags {
            ids: ids.clone(),
            visible: None,
            locked: Some(true),
        })
        .expect("locks");
    let locked = harness.app.project().clone();
    for (field, text) in [
        (Field::X, "20"),
        (Field::Y, "20"),
        (Field::W, "40"),
        (Field::H, "40"),
    ] {
        match type_into(&mut harness, &ids, field, text, true) {
            Err(CommandError::Locked { names }) => assert_eq!(names, ["coaster"], "{field:?}"),
            other => panic!("{field:?}: expected a locked refusal, got {other:?}"),
        }
    }
    assert_eq!(harness.app.project(), &locked);

    // The link broken, H scales Y alone (the plaque's square has no arcs,
    // so a non-uniform scale can place it).
    let mut plaque = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let square = vec![id("i4")];
    let p = placement_of(plaque.app.project(), &square);
    let t = instance_of(plaque.app.project(), "i4").transform;
    let h = p.bounds.height_mm() / 2.0;
    type_into(&mut plaque, &square, Field::H, &format!("{h}"), false).expect("H");
    assert_same_transform(
        instance_of(plaque.app.project(), "i4").transform,
        t.then(about(centre(p.bounds), Transform::scale(1.0, 0.5))),
        TOL,
        "H with the link broken",
    );
    plaque.app.undo();

    // A typed move regenerates the toolpath the CLI makes of the saved file.
    type_into(&mut plaque, &square, Field::X, "60", true).expect("X");
    type_into(&mut plaque, &square, Field::Y, "20", true).expect("Y");
    plaque.generate();
    let op2 = OperationId::new("op2");
    let runs = &plaque
        .app
        .generation()
        .result(&op2)
        .expect("the Square recess regenerated")
        .runs;
    let saved = plaque.save().expect("saves");
    let reloaded = dc_project::load(saved.as_bytes()).expect("reloads").project;
    let cli = dc_job::generate_operation(&reloaded, &op2);
    assert!(!runs.is_empty(), "the square has a toolpath");
    assert_eq!(runs, &cli.runs, "the app's toolpath is the CLI's");
}

/// Slice 25 (#617; capability 58, acceptance 95): the Properties section
/// of a selection offers X, Y, W and H, in that order, and nothing else:
/// rotation and mirroring are Arrange actions (58a). The same for one
/// shape, several, and a locked one (whose fields are read-only).
#[test]
fn placement_is_x_y_w_h_only() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    for ids in [vec![id("i4")], vec![id("i2"), id("i4")]] {
        let placement = placement_of(harness.app.project(), &ids);
        assert_eq!(
            properties::fields(&placement),
            [Field::X, Field::Y, Field::W, Field::H],
            "{ids:?}"
        );
        assert!(!properties::read_only(&placement));
    }
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![id("i4")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks");
    let locked = placement_of(harness.app.project(), &[id("i4")]);
    assert_eq!(
        properties::fields(&locked),
        [Field::X, Field::Y, Field::W, Field::H]
    );
    assert!(
        properties::read_only(&locked),
        "a locked shape's are read-only"
    );
    // The note's *Unlock* is one step that unlocks the locked ones.
    let unlock = properties::unlock(harness.app.project(), &[id("i2"), id("i4")])
        .expect("something to unlock");
    assert_eq!(
        unlock,
        Command::SetInstanceFlags {
            ids: vec![id("i4")],
            visible: None,
            locked: Some(false),
        }
    );
    harness.run(unlock).expect("unlocks");
    assert_eq!(harness.app.undo_label(), Some("Unlock “square”"));
    assert_eq!(properties::unlock(harness.app.project(), &[id("i4")]), None);
}

/// Slice 25 (#617; capability 58, acceptance 95): with the proportion link
/// broken, typing W scales X alone about the box's centre, H unchanged;
/// with it linked, H follows.
#[test]
fn broken_link_scales_one_axis() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let square = vec![id("i4")];
    let p = placement_of(harness.app.project(), &square);
    let t = instance_of(harness.app.project(), "i4").transform;
    let w = p.bounds.width_mm() * 2.0;
    type_into(&mut harness, &square, Field::W, &format!("{w}"), false).expect("W");
    assert_same_transform(
        instance_of(harness.app.project(), "i4").transform,
        t.then(about(centre(p.bounds), Transform::scale(2.0, 1.0))),
        TOL,
        "W with the link broken",
    );
    let after = placement_of(harness.app.project(), &square);
    assert!((after.bounds.height_mm() - p.bounds.height_mm()).abs() < 1e-9);
    assert!((after.bounds.width_mm() - w).abs() < 1e-9);
    assert_eq!(harness.app.undo_label(), Some("Scale “square”"));
    harness.app.undo();

    type_into(&mut harness, &square, Field::W, &format!("{w}"), true).expect("W");
    let linked = placement_of(harness.app.project(), &square);
    assert!(
        (linked.bounds.height_mm() - p.bounds.height_mm() * 2.0).abs() < 1e-9,
        "with the link, H follows W"
    );
}

/// Capability 58 with PureCutCNC's mixed-value display: for several
/// shapes X, Y, W and H are the joint box (never mixed), and the rotation
/// (read, not a field), Visible and Locked are mixed when the shapes
/// differ.
#[test]
fn multi_selection_shows_mixed_values() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let pair = vec![id("i2"), id("i4")];
    let one = placement_of(harness.app.project(), &[id("i4")]);
    let other = placement_of(harness.app.project(), &[id("i2")]);
    let joint = placement_of(harness.app.project(), &pair);
    let want = one
        .bounds
        .including(other.bounds.min)
        .including(other.bounds.max);
    assert_near(joint.bounds.min, want.min, 1e-9, "the joint box's corner");
    assert_near(
        joint.bounds.max,
        want.max,
        1e-9,
        "the joint box's far corner",
    );
    assert_eq!(joint.count, 2);
    assert!(
        joint.rotation_deg.is_some_and(|r| r.abs() < 1e-9),
        "both unrotated: {:?}",
        joint.rotation_deg
    );
    assert_eq!(joint.visible, Some(true));
    assert_eq!(joint.locked, Some(false));
    assert_eq!(joint.targeted_by, ["Panel", "Square recess"]);

    // Rotate the square alone, lock the pocket: both mixed.
    harness
        .run(Command::TransformInstances {
            ids: vec![id("i4")],
            delta: Transform::rotate(FRAC_PI_2),
        })
        .expect("rotates");
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![id("i2")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks");
    let project = harness.app.project().clone();
    let mixed = placement_of(&project, &pair);
    assert_eq!(mixed.rotation_deg, None, "rotations differ");
    assert_eq!(mixed.locked, None, "one is locked");
    assert!(mixed.any_locked);
    assert!(!properties::field_text(&mixed, Field::X, Units::Mm).is_empty());
    let single = placement_of(&project, &[id("i4")]);
    assert!(
        single.rotation_deg.is_some_and(|r| (r - 90.0).abs() < 1e-9),
        "{:?}",
        single.rotation_deg
    );

    // Inch fields show inches.
    let inch = placement_of(&project, &[id("i4")]);
    let x_mm = inch.bounds.min.x;
    let shown = properties::field_text(&inch, Field::X, Units::Inch);
    let back: f64 = shown.parse().expect("a number");
    assert!(
        (back * 25.4 - x_mm).abs() < 0.01,
        "{shown} in for {x_mm} mm"
    );
}

/// A new project with one open path through `points`, placed by
/// `transform`: instance `i1`.
fn with_path(points: &[(f64, f64)], closed: bool, transform: Transform) -> Harness {
    let mut harness = Harness::from_project(dc_app::App::new_project().unwrap());
    harness
        .run(Command::AddShapes(vec![dc_app::command::NewShape {
            name: "Line".to_owned(),
            shape: None,
            profiles: vec![dc_geometry::Polyline::new(
                points
                    .iter()
                    .map(|(x, y)| dc_geometry::Vertex::line(*x, *y))
                    .collect(),
                closed,
            )],
            transform,
            construction: false,
            group: None,
        }]))
        .unwrap();
    harness
}

/// The one shape's outline as it is placed.
fn placed(harness: &Harness) -> Vec<(f64, f64)> {
    let project = harness.app.project();
    let instance = &project.instances[0];
    instance
        .transform
        .apply_polyline(&project.definitions[&instance.definition].profiles[0])
        .unwrap()
        .vertices()
        .iter()
        .map(|v| (v.x, v.y))
        .collect()
}

#[track_caller]
fn assert_points(got: &[(f64, f64)], want: &[(f64, f64)]) {
    assert_eq!(got.len(), want.len(), "{got:?}");
    for (g, w) in got.iter().zip(want) {
        assert!(
            (g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-9,
            "{got:?}"
        );
    }
}

/// The maintainer's review of 2026-11-23: a line is laid level by typing
/// 0 for its height, and stood upright by 0 for its width. Its points go
/// onto the horizontal, or the vertical, through the Origin; the other
/// size stays, whatever Keep proportions says. One undo step, named for
/// what it did.
#[test]
fn zero_height_lays_a_line_level_and_zero_width_stands_it_upright() {
    let line = [(10.0, 20.0), (50.0, 28.0)];
    let ids = [id("i1")];
    // H 0, about the middle of its box.
    let mut harness = with_path(&line, false, Transform::IDENTITY);
    assert!(matches!(
        type_into(&mut harness, &ids, Field::H, "0", true),
        Ok(true)
    ));
    assert_points(&placed(&harness), &[(10.0, 24.0), (50.0, 24.0)]);
    assert_eq!(harness.app.undo_label(), Some("Make “Line” level"));
    let placement = placement_of(harness.app.project(), &ids);
    assert_eq!(
        properties::field_text(&placement, Field::H, Units::Mm),
        "0",
        "its box has no height now"
    );
    assert_eq!(
        properties::field_text(&placement, Field::W, Units::Mm),
        "40"
    );
    // Level already: nothing to do, and no step.
    assert!(matches!(
        type_into(&mut harness, &ids, Field::H, "0", true),
        Ok(false)
    ));
    harness.app.undo();
    assert_points(&placed(&harness), &line);

    // W 0: upright, through the middle.
    assert!(matches!(
        type_into(&mut harness, &ids, Field::W, "0 mm", false),
        Ok(true)
    ));
    assert_points(&placed(&harness), &[(30.0, 20.0), (30.0, 28.0)]);
    assert_eq!(harness.app.undo_label(), Some("Make “Line” upright"));
    harness.app.undo();

    // About another origin: the bottom left stays, so the line lies along
    // the bottom of the box it had.
    let project = harness.app.project().clone();
    let edit = properties::Edit::H {
        mm: 0.0,
        uniform: false,
    };
    let command =
        properties::edit_about(&project, &ids, edit, dc_app::origin::Origin::BottomLeft).unwrap();
    harness.run(command).unwrap();
    assert_points(&placed(&harness), &[(10.0, 20.0), (50.0, 20.0)]);

    // A line placed by a scale and a move is level as it is placed.
    let mut harness = with_path(
        &[(0.0, 0.0), (20.0, 4.0)],
        false,
        Transform::translate(5.0, 5.0).then(Transform::scale(2.0, 2.0)),
    );
    let before = placed(&harness);
    assert!(matches!(
        type_into(&mut harness, &ids, Field::H, "0", false),
        Ok(true)
    ));
    let after = placed(&harness);
    let middle = f64::midpoint(before[0].1, before[1].1);
    assert_points(&after, &[(before[0].0, middle), (before[1].0, middle)]);

    // A path of several straight pieces goes onto one level too.
    let mut harness = with_path(
        &[(0.0, 0.0), (10.0, 6.0), (30.0, 2.0)],
        false,
        Transform::IDENTITY,
    );
    assert!(matches!(
        type_into(&mut harness, &ids, Field::H, "0", false),
        Ok(true)
    ));
    assert_points(&placed(&harness), &[(0.0, 3.0), (10.0, 3.0), (30.0, 3.0)]);
}

/// What has no zero size: a closed shape, which would have no inside
/// left; a line that would be left with no length; and several shapes at
/// once. Each says why, and nothing changes.
#[test]
fn zero_is_refused_where_nothing_would_be_left() {
    let ids = [id("i1")];
    let reason = |result: Result<bool, CommandError>| match result {
        Err(CommandError::NotAQuantity { reason, .. }) => reason,
        other => panic!("refused with a reason: {other:?}"),
    };
    // A closed triangle.
    let mut harness = with_path(
        &[(0.0, 0.0), (40.0, 0.0), (40.0, 30.0)],
        true,
        Transform::IDENTITY,
    );
    let before = harness.app.project().clone();
    assert_eq!(
        reason(type_into(&mut harness, &ids, Field::H, "0", false)),
        "only an open path of straight lines can have no height"
    );
    assert_eq!(
        reason(type_into(&mut harness, &ids, Field::W, "0", false)),
        "only an open path of straight lines can have no width"
    );
    assert_eq!(harness.app.project(), &before);
    // An upright line laid level would be a point.
    let mut harness = with_path(&[(10.0, 0.0), (10.0, 30.0)], false, Transform::IDENTITY);
    assert_eq!(
        reason(type_into(&mut harness, &ids, Field::H, "0", false)),
        "nothing would be left of the line"
    );
    // Two lines at once.
    let mut harness = with_path(&[(0.0, 0.0), (40.0, 5.0)], false, Transform::IDENTITY);
    let first = harness.app.project().instances[0].clone();
    harness
        .run(Command::CopyInstances {
            ids: vec![first.id],
            placements: vec![Transform::translate(0.0, 20.0)],
            group: None,
        })
        .unwrap();
    let both: Vec<InstanceId> = harness
        .app
        .project()
        .instances
        .iter()
        .map(|i| i.id.clone())
        .collect();
    assert_eq!(
        reason(type_into(&mut harness, &both, Field::H, "0", false)),
        "select one line to give it no height"
    );
    // A negative size is still no size.
    let mut harness = with_path(&[(0.0, 0.0), (40.0, 5.0)], false, Transform::IDENTITY);
    assert!(matches!(
        type_into(&mut harness, &ids, Field::H, "-1", false),
        Err(CommandError::InvalidValue { .. } | CommandError::NotAQuantity { .. })
    ));
}
