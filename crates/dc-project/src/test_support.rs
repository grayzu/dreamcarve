// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Fixture builders for tests in this crate and, with the `test-support`
//! feature, in crates that depend on it (`docs/rust-guidelines.md`, "Tests").

use crate::ids::{DefinitionId, InstanceId, OperationId, ToolId};
use crate::migrate::CURRENT_VERSION;
use crate::model::{
    CutDirection, FeatureDefinition, FeatureInstance, MachineOrigin, Operation, OriginXy, OriginZ,
    ProfileOp, Project, Side, Stock, TabPlacement, TabShape, TabSpec, Tool, ToolKind, Units,
};
use crate::post_processor::PostProcessor;
use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, Point, Polyline, Transform};
use std::collections::BTreeMap;

/// The GRBL post-processor: a copy of `dc-gcode`'s bundled `grbl.json`,
/// kept identical by a test in `dc-gcode`.
#[allow(
    clippy::expect_used,
    reason = "fixed fixture file; a parse failure is a broken test setup"
)]
pub fn grbl_post_processor() -> PostProcessor {
    serde_json::from_str(include_str!("../tests/fixtures/post/grbl.json"))
        .expect("tests/fixtures/post/grbl.json is a valid post-processor")
}

/// A 6.35 mm two-flute up-cut end mill with every value set.
pub fn tool_6_35_upcut() -> Tool {
    Tool {
        id: ToolId::new("t1"),
        name: "6.35 mm upcut".into(),
        cutter: ToolKind::EndMill,
        diameter_mm: 6.35,
        flutes: 2,
        feed_mm_min: Some(1000.0),
        plunge_mm_min: Some(300.0),
        rpm: Some(18000),
        step_down_mm: Some(2.0),
        step_over_fraction: Some(0.4),
    }
}

/// A valid project with no validation problems: 100 × 100 × 6 mm stock, one
/// 50 × 30 mm rectangle placed at (10, 10), one tool, and one outside climb
/// profile through the stock with four tabs. Matches the committed fixture
/// `tests/fixtures/dcv/v4/sample.dcv`.
pub fn sample_project() -> Project {
    let rect = FeatureDefinition {
        id: DefinitionId::new("d1"),
        name: "Rectangle".into(),
        profiles: vec![Polyline::rect(Point { x: 0.0, y: 0.0 }, 50.0, 30.0)],
    };
    Project {
        version: CURRENT_VERSION,
        name: "Sample".into(),
        display_units: Units::Mm,
        flatten_tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
        stock: Stock {
            width_mm: 100.0,
            height_mm: 100.0,
            thickness_mm: Some(6.0),
        },
        origin: MachineOrigin {
            xy: OriginXy::BottomLeft,
            z: OriginZ::StockTop,
        },
        definitions: BTreeMap::from([(rect.id.clone(), rect)]),
        instances: vec![FeatureInstance {
            id: InstanceId::new("i1"),
            definition: DefinitionId::new("d1"),
            transform: Transform::translate(10.0, 10.0),
            name: "Rectangle 1".into(),
            visible: true,
            locked: false,
            z_top_mm: 6.0,
            z_bottom_mm: 0.0,
        }],
        tools: vec![tool_6_35_upcut()],
        operations: vec![Operation::Profile(ProfileOp {
            id: OperationId::new("o1"),
            name: "Cut out".into(),
            enabled: true,
            targets: vec![InstanceId::new("i1")],
            tool: ToolId::new("t1"),
            side: Side::Outside,
            depth_mm: Some(6.0),
            step_down_mm: None,
            direction: CutDirection::Climb,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
            tabs: Some(TabSpec {
                placement: TabPlacement::Count { count: 4 },
                width_mm: 6.0,
                height_mm: 2.0,
                shape: TabShape::Rect,
            }),
        })],
        post_processor: grbl_post_processor(),
    }
}
