// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 25 of phase 4 (#617; `docs/specs/phase-4/design.md`, "Review
//! redesign slices"): the Arrange actions (functional capability 58a,
//! acceptance 66 and 95). Rotate and flip act on the selection about its
//! bounding-box centre, each one undo step named for it; the group is
//! disabled with a reason when nothing can turn; a partly locked selection
//! turns its unlocked shapes and names the rest.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::f64::consts::FRAC_PI_2;

use dc_app::arrange::{self, Action, Disabled};
use dc_app::test_support::Harness;
use dc_app::{Command, StrKey, Strings};
use dc_geometry::{Point, Transform};
use dc_project::InstanceId;

use common::{assert_same_transform, instance_of, plaque_bytes};

/// Transform coefficients agree to this, mm or dimensionless.
const TOL: f64 = 1e-9;

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

/// `delta` about `pivot`, as acceptance 66 expects it.
fn about(pivot: Point, delta: Transform) -> Transform {
    Transform::translate(-pivot.x, -pivot.y)
        .then(delta)
        .then(Transform::translate(pivot.x, pivot.y))
}

/// The centre of `ids`' joint bounding box.
fn centre_of(harness: &Harness, ids: &[InstanceId]) -> Point {
    let b = dc_app::properties::placement(harness.app.project(), ids)
        .expect("a placement")
        .bounds;
    Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0)
}

/// Acceptance 95 (capabilities 58a and 66): with the square selected the
/// toolbar's Rotate turns it 90° clockwise, the popover's angle 30 turns
/// it 30° anticlockwise, and each flip mirrors it, each about its
/// bounding-box centre: the transform acceptance 66 expects for the same
/// rotation or mirror. Each is one undo step with its label, and undo
/// restores the transform exactly.
#[test]
fn rotate_90_and_flip_equal_the_typed_transforms() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let square = vec![id("i4")];
    let before = instance_of(harness.app.project(), "i4").transform;
    let c = centre_of(&harness, &square);

    for (action, delta, label) in [
        (
            Action::RotateClockwise,
            Transform::rotate(-FRAC_PI_2),
            "Rotate “square” 90° clockwise",
        ),
        (
            Action::RotateAnticlockwise,
            Transform::rotate(FRAC_PI_2),
            "Rotate “square” 90° anticlockwise",
        ),
        (
            Action::Rotate { angle_deg: 30.0 },
            Transform::rotate(30f64.to_radians()),
            "Rotate “square” 30° anticlockwise",
        ),
        (
            Action::FlipHorizontal,
            Transform::scale(-1.0, 1.0),
            "Flip “square” horizontally",
        ),
        (
            Action::FlipVertical,
            Transform::scale(1.0, -1.0),
            "Flip “square” vertically",
        ),
    ] {
        let project = harness.app.project().clone();
        let arranged = arrange::arrange(&project, &square, 0, action).expect("the square turns");
        assert!(arranged.locked.is_empty(), "{action:?}: nothing locked");
        let steps = harness.app.history().undo_labels().count();
        harness.run(arranged.command).expect("applies");
        assert_eq!(
            harness.app.history().undo_labels().count(),
            steps + 1,
            "{action:?}: one undo step"
        );
        assert_same_transform(
            instance_of(harness.app.project(), "i4").transform,
            before.then(about(c, delta)),
            TOL,
            &format!("{action:?}"),
        );
        assert_eq!(harness.app.undo_label(), Some(label), "{action:?}");
        harness.app.undo();
        assert_eq!(
            instance_of(harness.app.project(), "i4").transform,
            before,
            "{action:?}: undo restores it exactly"
        );
    }

    // A typed angle that turns nothing is no step.
    let full = arrange::arrange(
        harness.app.project(),
        &square,
        0,
        Action::Rotate { angle_deg: 360.0 },
    )
    .expect("the selection can turn");
    assert!(
        matches!(
            full.command.apply(harness.app.project()),
            Err(dc_app::CommandError::NothingToDo)
        ),
        "a full turn changes nothing"
    );
    assert_eq!(arrange::parse_angle("30°").expect("degrees"), 30.0);
    assert_eq!(arrange::parse_angle(" -45 ").expect("a number"), -45.0);
    assert!(arrange::parse_angle("abc").is_err());
}

/// Capability 58a: the group is disabled, with the reason in its tooltip,
/// when nothing is selected, when every selected shape is locked, and when
/// only pictures are selected (a picture cannot rotate or flip, 56). Each
/// reason is a string key.
#[test]
fn disabled_with_a_reason() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let project = harness.app.project().clone();
    let square = vec![id("i4")];
    assert_eq!(arrange::disabled(&project, &square, 0), None);

    assert_eq!(
        arrange::disabled(&project, &[], 0),
        Some(Disabled::NothingSelected)
    );
    assert_eq!(
        Disabled::NothingSelected.reason(),
        StrKey::ArrangeDisabledNothing
    );

    assert_eq!(
        arrange::disabled(&project, &[], 2),
        Some(Disabled::OnlyPictures)
    );
    assert_eq!(
        Disabled::OnlyPictures.reason(),
        StrKey::ArrangeDisabledPictures
    );

    harness
        .run(Command::SetInstanceFlags {
            ids: vec![id("i4"), id("i5")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks");
    let locked = harness.app.project().clone();
    let both = vec![id("i4"), id("i5")];
    assert_eq!(
        arrange::disabled(&locked, &both, 0),
        Some(Disabled::AllLocked)
    );
    assert_eq!(Disabled::AllLocked.reason(), StrKey::ArrangeDisabledLocked);
    assert_eq!(
        arrange::disabled(&locked, &both, 1),
        Some(Disabled::AllLocked),
        "pictures beside locked shapes: still nothing can turn"
    );

    // Every action refuses with the same reason, and the strings say it.
    for action in [
        Action::RotateClockwise,
        Action::RotateAnticlockwise,
        Action::Rotate { angle_deg: 30.0 },
        Action::FlipHorizontal,
        Action::FlipVertical,
        Action::CentreOnStock,
    ] {
        assert_eq!(
            arrange::arrange(&locked, &both, 0, action).map(|a| a.command),
            Err(Disabled::AllLocked),
            "{action:?}"
        );
        assert_eq!(
            arrange::arrange(&project, &[], 0, action).map(|a| a.command),
            Err(Disabled::NothingSelected),
            "{action:?}"
        );
    }
    let strings = Strings::english();
    for reason in [
        Disabled::NothingSelected,
        Disabled::AllLocked,
        Disabled::OnlyPictures,
    ] {
        assert!(!strings.get(reason.reason()).is_empty());
    }
}

/// Capability 58a: with some selected shapes locked, the unlocked ones
/// turn about their own box's centre, the locked ones stay, and the
/// status bar's message names the locked ones.
#[test]
fn partly_locked_turns_the_unlocked_and_names_the_rest() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness
        .run(Command::SetInstanceFlags {
            ids: vec![id("i5")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks holes-1");
    let selection = vec![id("i4"), id("i5")];
    let project = harness.app.project().clone();
    assert_eq!(arrange::disabled(&project, &selection, 0), None);
    let square_before = instance_of(&project, "i4").transform;
    let hole_before = instance_of(&project, "i5").transform;
    let c = centre_of(&harness, &[id("i4")]);

    let arranged = arrange::arrange(&project, &selection, 0, Action::RotateClockwise)
        .expect("the square can turn");
    assert_eq!(arranged.locked, ["holes-1"], "the locked one is named");
    harness.run(arranged.command.clone()).expect("applies");
    assert_same_transform(
        instance_of(harness.app.project(), "i4").transform,
        square_before.then(about(c, Transform::rotate(-FRAC_PI_2))),
        TOL,
        "the square turns about its own centre",
    );
    assert_eq!(
        instance_of(harness.app.project(), "i5").transform,
        hole_before,
        "the locked hole stays"
    );
    assert_eq!(
        harness.app.undo_label(),
        Some("Rotate “square” 90° clockwise")
    );

    let strings = Strings::english();
    let message = arrange::locked_message(&strings, &arranged.locked).expect("a message");
    assert!(message.contains("holes-1"), "{message}");
    assert_eq!(arrange::locked_message(&strings, &[]), None);
    let two = arrange::locked_message(&strings, &["holes-1".to_owned(), "holes-2".to_owned()])
        .expect("a message");
    assert!(two.contains("holes-1") && two.contains("holes-2"), "{two}");

    // Centre on stock moves the unlocked ones alone, the same way.
    let arranged = arrange::arrange(harness.app.project(), &selection, 0, Action::CentreOnStock)
        .expect("the square can move");
    assert_eq!(arranged.locked, ["holes-1"]);
    assert_eq!(arranged.command, Command::CentreOnStock(vec![id("i4")]));
}
