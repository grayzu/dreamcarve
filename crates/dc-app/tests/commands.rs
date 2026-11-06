// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 5 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan → Tree
//! and placement"; #534): the Project menu's *Centre on stock* and *Fit
//! stock to shapes* (functional capability 61, acceptance 69).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::properties;
use dc_app::test_support::Harness;
use dc_app::{Command, CommandError};
use dc_geometry::{Point, Rect, Transform};
use dc_project::{InstanceId, Stock};

use common::{assert_near, assert_same_transform, coaster, plaque_bytes};

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn centre(rect: Rect) -> Point {
    Point::new(
        (rect.min.x + rect.max.x) / 2.0,
        (rect.min.y + rect.max.y) / 2.0,
    )
}

/// Capability 61: *Centre on stock* moves the selection so its joint box
/// is centred on the stock, by one translation, as one undo step; the
/// other shapes stay; a locked shape refuses.
#[test]
fn centre_on_stock() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let before = harness.app.project().clone();
    let pair = vec![id("i4"), id("i9")];
    let box_ = properties::placement(&before, &pair).expect("a box").bounds;
    harness
        .run(Command::CentreOnStock(pair.clone()))
        .expect("centres");
    let after = harness.app.project().clone();
    let moved = properties::placement(&after, &pair).expect("a box").bounds;
    let stock = Point::new(before.stock.width_mm / 2.0, before.stock.height_mm / 2.0);
    assert_near(
        centre(moved),
        stock,
        1e-9,
        "the box's centre is the stock's",
    );
    let shift = Transform::translate(stock.x - centre(box_).x, stock.y - centre(box_).y);
    for (b, a) in before.instances.iter().zip(&after.instances) {
        let want = if pair.contains(&b.id) {
            b.transform.then(shift)
        } else {
            b.transform
        };
        assert_same_transform(a.transform, want, 1e-9, b.id.as_str());
    }
    assert_eq!(after.stock, before.stock, "the stock stays");
    assert_eq!(
        harness.app.undo_label(),
        Some("Centre 2 shapes on stock"),
        "one undo step"
    );
    harness.app.undo();
    assert_eq!(harness.app.project(), &before);

    harness
        .run(Command::SetInstanceFlags {
            ids: vec![id("i9")],
            visible: None,
            locked: Some(true),
        })
        .expect("locks");
    match harness.run(Command::CentreOnStock(pair)) {
        Err(CommandError::Locked { names }) => assert_eq!(names, ["border"]),
        other => panic!("expected a locked refusal, got {other:?}"),
    }
}

/// Acceptance 69 (capability 61): *Fit stock to shapes* with `init`'s
/// margin reproduces the stock `init` writes for the coaster, whatever
/// the stock was and wherever the coaster had been moved, and puts the
/// coaster back where `init` put it; the origin preset is kept.
#[test]
fn fit_stock_to_shapes_equals_init() {
    let init = coaster();
    let mut harness = Harness::from_project(init.clone());
    let ids: Vec<InstanceId> = init.instances.iter().map(|i| i.id.clone()).collect();
    harness
        .run(Command::TransformInstances {
            ids,
            delta: Transform::translate(37.25, -12.5),
        })
        .expect("moves");
    harness
        .run(Command::SetStock(Stock {
            width_mm: 300.0,
            height_mm: 220.0,
            thickness_mm: init.stock.thickness_mm,
        }))
        .expect("sets the stock");
    let origin = harness.app.project().origin.clone();

    harness
        .run(Command::FitStockToShapes {
            margin_mm: dc_job::project_from::STOCK_MARGIN_MM,
        })
        .expect("fits");
    let fitted = harness.app.project();
    assert!(
        (fitted.stock.width_mm - init.stock.width_mm).abs() < 1e-9
            && (fitted.stock.height_mm - init.stock.height_mm).abs() < 1e-9,
        "init's stock: {:?}, want {:?}",
        fitted.stock,
        init.stock
    );
    assert_eq!(fitted.stock.thickness_mm, init.stock.thickness_mm);
    assert_eq!(fitted.origin, origin, "the origin preset is kept");
    for (got, want) in fitted.instances.iter().zip(&init.instances) {
        assert_same_transform(got.transform, want.transform, 1e-6, want.id.as_str());
    }
    assert_eq!(harness.app.undo_label(), Some("Fit stock to shapes"));

    // A typed margin sizes the stock round the shapes with it.
    let wide = Command::FitStockToShapes { margin_mm: 25.0 }
        .apply(&init)
        .expect("fits with a wider margin");
    let shapes = properties::placement(&wide, &[id("coaster")])
        .or_else(|| {
            let all: Vec<InstanceId> = wide.instances.iter().map(|i| i.id.clone()).collect();
            properties::placement(&wide, &all)
        })
        .expect("a box")
        .bounds;
    assert!((shapes.min.x - 25.0).abs() < 0.01 && (shapes.min.y - 25.0).abs() < 0.01);
    assert!((wide.stock.width_mm - shapes.max.x - 25.0).abs() < 0.01);
    assert!(
        Command::FitStockToShapes { margin_mm: -1.0 }
            .apply(&init)
            .is_err(),
        "a negative margin is refused"
    );
}
