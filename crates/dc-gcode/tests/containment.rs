// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Containment order end to end (issue #85, invariant O1 in
//! `docs/research/toolpath-ordering.md`): a frame with a coaster inside its
//! hole, drawn outermost first, exports through the safety pass (S1–S7)
//! with the coaster cut first, and the export is byte-for-byte repeatable.
//! The cut order itself is checked move by move in `dc_cam::profile`.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_cam::generate_profile;
use dc_cam::test_support::assert_safe;
use dc_gcode::{emit, parse_motion};
use dc_geometry::{Point, Polyline};
use dc_project::test_support::sample_project;
use dc_project::{CutDirection, Operation, Project, Side, Stock};

const DATE: &str = "2026-01-01";
const THICKNESS_MM: f64 = 6.0;

/// A 200 × 150 mm frame (outline, then a 160 × 110 mm hole) with a 60 mm
/// coaster inside the hole, drawn in that order, profiled outside and
/// through the 6 mm stock in one operation.
fn island_project() -> Project {
    let mut project = sample_project();
    project.stock = Stock {
        width_mm: 300.0,
        height_mm: 200.0,
        thickness_mm: Some(THICKNESS_MM),
    };
    let tool = &mut project.tools[0];
    tool.feed_mm_min = Some(1000.0);
    tool.plunge_mm_min = Some(300.0);
    tool.rpm = Some(18000);
    tool.step_down_mm = Some(2.0);

    let instance = &mut project.instances[0];
    instance.transform = dc_geometry::Transform::translate(20.0, 20.0);
    instance.z_top_mm = Some(THICKNESS_MM);
    instance.z_bottom_mm = Some(0.0);
    let definition = project
        .definitions
        .get_mut(&instance.definition)
        .expect("the sample instance has a definition");
    definition.profiles = vec![
        Polyline::rect(Point::new(0.0, 0.0), 200.0, 150.0),
        Polyline::rect(Point::new(20.0, 20.0), 160.0, 110.0),
        Polyline::circle(Point::new(100.0, 75.0), 30.0),
    ];
    let targets = vec![instance.id.clone()];

    let Some(Operation::Profile(op)) = project.operations.first_mut() else {
        panic!("the sample project has a profile operation");
    };
    op.targets = targets;
    op.side = Side::Outside;
    op.depth_mm = Some(THICKNESS_MM);
    op.step_down_mm = None;
    op.direction = CutDirection::Climb;
    op.through_cut_allowance_mm = 0.0;
    op.safe_z_mm = 5.0;
    op.tabs = None;
    project
}

/// Generates and emits the project's only operation; `emit` runs the
/// safety pass, so an `Ok` program passed S1–S7.
fn export(project: &Project) -> String {
    let Some(Operation::Profile(op)) = project.operations.first() else {
        panic!("the island project has a profile operation");
    };
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .expect("the operation's tool exists");
    let (toolpath, warnings) = generate_profile(project, op).expect("the profile generates");
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_safe(&toolpath, project, op);
    let (program, _) = emit(
        project,
        &[(Operation::Profile(op.clone()), tool.clone(), toolpath)],
        DATE,
    )
    .expect("the program passes the safety pass");
    program.text
}

#[test]
fn island_exports_safely_and_repeatably() {
    let project = island_project();
    let first = export(&project);
    let second = export(&project);
    assert_eq!(first, second, "two exports are byte-identical");

    // The origin is the stock's bottom-left corner and top, so the first
    // move into the stock is the coaster's first plunge: on its tool-centre
    // circle, radius 30 + 3.175 about (120, 95).
    let plunge = parse_motion(&first)
        .expect("the program parses")
        .into_iter()
        .find(|mv| mv.to.z.is_some_and(|z| z < 0.0))
        .expect("the program cuts");
    let (x, y) = (
        plunge.to.x.expect("x is known"),
        plunge.to.y.expect("y is known"),
    );
    let radius_mm = (x - 120.0).hypot(y - 95.0);
    assert!(
        (radius_mm - 33.175).abs() < 1e-3,
        "first cut at ({x}, {y}) is not on the coaster"
    );
}
