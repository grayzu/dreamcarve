// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Golden parity: DreamCarve's Profile output against PureCutCNC's, captured
//! into `tests/golden/profile-*` by `tools/parity/`
//! (`docs/specs/phase-1/functional.md`, acceptance test 14;
//! `docs/specs/phase-1/design.md`, "Test plan → golden").
//!
//! Each test builds the DreamCarve project equivalent to the case's
//! PureCutCNC project (`tools/parity/cases/<case>.camj`). PureCutCNC is
//! Y-down with its origin at the stock's top-left corner (Y-down) and Z zero
//! at the stock top; DreamCarve is Y-up (ADR-0001), so every DreamCarve Y is
//! the stock height minus the PureCutCNC Y, and the origin is the bottom-left
//! corner, Z zero at the stock top. Both programs then share one machine
//! frame and are compared in it. What the harness checks is in
//! [`harness::check`]; each case's README lists the differences.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

#[path = "golden/harness.rs"]
mod harness;

use std::f64::consts::FRAC_PI_4;

use dc_geometry::{Point, Polyline, Transform};
use dc_project::test_support::sample_project;
use dc_project::{
    CutDirection, DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, MachineOrigin,
    Operation, OriginXy, OriginZ, Project, Side, Stock, TabPlacement, TabShape, TabSpec,
};
use harness::{Case, Difference};

/// Stock thickness of every case, mm (PureCutCNC `stock.thickness`).
const THICKNESS_MM: f64 = 12.0;

/// One shape of a case: a definition-space outline and where it is placed.
struct Shape {
    outline: Polyline,
    placement: Transform,
}

/// A rectangle `width_mm` × `height_mm` with its lower-left corner at
/// (`x_mm`, `y_mm`), Y-up.
fn rect(x_mm: f64, y_mm: f64, width_mm: f64, height_mm: f64) -> Shape {
    Shape {
        outline: Polyline::rect(Point::new(0.0, 0.0), width_mm, height_mm),
        placement: Transform::translate(x_mm, y_mm),
    }
}

/// A circle of `diameter_mm` centred at (`x_mm`, `y_mm`), Y-up, whose
/// outline starts `start_rad` counter-clockwise from +X.
fn circle(x_mm: f64, y_mm: f64, diameter_mm: f64, start_rad: f64) -> Shape {
    Shape {
        outline: Polyline::circle(Point::new(0.0, 0.0), diameter_mm / 2.0),
        placement: Transform::rotate(start_rad).then(Transform::translate(x_mm, y_mm)),
    }
}

/// The settings every case shares with its `.camj`: a 6.35 mm tool at feed
/// 1500, plunge 500, 18000 RPM; 4 mm per pass; climb; safe Z 5 mm above the
/// stock top; origin at the stock's bottom-left corner (PureCutCNC's
/// top-left, Y-down), Z zero at the stock top; the grblHAL post-processor.
fn project(
    name: &str,
    width_mm: f64,
    height_mm: f64,
    shapes: Vec<Shape>,
    side: Side,
    depth_mm: f64,
    tabs: Option<TabSpec>,
) -> Project {
    let mut project = sample_project();
    project.name = name.into();
    project.stock = Stock {
        width_mm,
        height_mm,
        thickness_mm: THICKNESS_MM,
    };
    project.origin = MachineOrigin {
        xy: OriginXy::BottomLeft,
        z: OriginZ::StockTop,
    };
    project.post_processor = dc_gcode::post::lookup("grblhal").expect("grblHAL is bundled");
    let tool = &mut project.tools[0];
    tool.feed_mm_min = Some(1500.0);
    tool.plunge_mm_min = Some(500.0);
    tool.rpm = Some(18000);

    project.definitions.clear();
    project.instances.clear();
    let mut targets = Vec::new();
    for (index, shape) in shapes.into_iter().enumerate() {
        let definition = DefinitionId::new(format!("d{}", index + 1));
        let instance = InstanceId::new(format!("i{}", index + 1));
        project.definitions.insert(
            definition.clone(),
            FeatureDefinition {
                id: definition.clone(),
                name: format!("Shape {}", index + 1),
                profiles: vec![shape.outline],
            },
        );
        project.instances.push(FeatureInstance {
            id: instance.clone(),
            definition,
            transform: shape.placement,
            name: format!("Shape {}", index + 1),
            visible: true,
            locked: false,
            z_top_mm: THICKNESS_MM,
            z_bottom_mm: 0.0,
        });
        targets.push(instance);
    }

    let Some(Operation::Profile(op)) = project.operations.first_mut() else {
        panic!("sample project has a profile operation");
    };
    op.targets = targets;
    op.side = side;
    op.depth_mm = depth_mm;
    op.step_down_mm = Some(4.0);
    op.direction = CutDirection::Climb;
    op.through_cut_allowance_mm = 0.0;
    op.safe_z_mm = 5.0;
    op.tabs = tabs;
    project
}

/// Outside profile of a 100 × 50 mm rectangle (functional acceptance test
/// 8's size), through 12 mm stock in three 4 mm passes. The rectangle sits
/// off-centre in the 150 × 100 mm stock, at (20, 30) Y-up, which is (20, 20)
/// Y-down in the `.camj`, so a lost Y flip moves every point.
#[test]
fn profile_outside_rect() {
    harness::check(&Case {
        name: "profile-outside-rect",
        project: project(
            "profile-outside-rect",
            150.0,
            100.0,
            vec![rect(20.0, 30.0, 100.0, 50.0)],
            Side::Outside,
            12.0,
            None,
        ),
        differences: vec![],
    });
}

/// Inside profile of a 60 mm circle centred at (70, 55) Y-up, 10 mm deep:
/// passes at Z 8, 4 and 2 above the stock bottom, the last one shallower
/// (functional acceptance test 4). PureCutCNC cuts the inside of a
/// `subtract` feature whose `z_bottom` is 2.
#[test]
fn profile_inside_circle() {
    harness::check(&Case {
        name: "profile-inside-circle",
        project: project(
            "profile-inside-circle",
            150.0,
            100.0,
            vec![circle(70.0, 55.0, 60.0, 0.0)],
            Side::Inside,
            10.0,
            None,
        ),
        differences: vec![],
    });
}

/// A 60 mm circle containing a 30 mm circle, outside profile (functional
/// acceptance test 10): DreamCarve nests the inner circle as a hole and
/// cuts it on the inside, then the outer circle on the outside, in one
/// operation. PureCutCNC needs two operations for the same cuts, an inside
/// route on a `subtract` circle and then an outside route on an `add`
/// circle. The harness compares each Z level as a set of moves, so the
/// order of the two contours does not matter.
#[test]
fn profile_ring() {
    harness::check(&Case {
        name: "profile-ring",
        project: project(
            "profile-ring",
            150.0,
            100.0,
            vec![circle(75.0, 50.0, 60.0, 0.0), circle(75.0, 50.0, 30.0, 0.0)],
            Side::Outside,
            12.0,
            None,
        ),
        differences: vec![],
    });
}

/// The coaster (functional spec, "Test part"): outside profile of a 90 mm
/// circle with four rect tabs 8 mm wide and 3 mm tall (acceptance test 11).
///
/// Intentional differences, also in `tests/golden/profile-tabs-circle/README.md`:
///
/// - **Tabs are placed along the path.** DreamCarve raises
///   `width + diameter` = 14.35 mm of tool-centre path per tab. PureCutCNC
///   raises the part of the path inside a world rectangle grown by the tool
///   radius, which on this curve is 0.054 mm longer. The `.camj` tabs are
///   7.947 mm wide so that PureCutCNC's raised span on this 48.175 mm
///   radius path is also 14.35 mm long, and they sit where DreamCarve's
///   `Count(4)` puts its tabs: the circle is placed rotated 45°, so its
///   path starts on a diagonal and the tab centres, an eighth of the path
///   from the start and a quarter apart, fall on the axes.
///
/// The `.camj` outline is a 360-sided polygon on the circle rather than a
/// PureCutCNC `circle`, which is sampled every 10°: where tabs split that
/// coarser path, the split points sit up to 0.034 mm inside the circle.
/// - **Rect tab steps run at the plunge rate.** DreamCarve lifts onto and
///   drops off a rect tab with vertical moves at the plunge rate
///   (`docs/specs/phase-1/design.md`, "Profile contour", step 7; a straight
///   plunge must carry the plunge rate, functional acceptance test 7).
///   PureCutCNC makes both steps at the cutting feed.
#[test]
fn profile_tabs_circle() {
    harness::check(&Case {
        name: "profile-tabs-circle",
        project: project(
            "profile-tabs-circle",
            120.0,
            120.0,
            vec![circle(60.0, 60.0, 90.0, FRAC_PI_4)],
            Side::Outside,
            12.0,
            Some(TabSpec {
                placement: TabPlacement::Count { count: 4 },
                width_mm: 8.0,
                height_mm: 3.0,
                shape: TabShape::Rect,
            }),
        ),
        differences: vec![Difference::RectTabStepsAtPlungeRate],
    });
}
