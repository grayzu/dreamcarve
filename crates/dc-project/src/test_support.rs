// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Fixture builders for tests in this crate and, with the `test-support`
//! feature, in crates that depend on it (`docs/rust-guidelines.md`, "Tests").

use crate::ids::{DefinitionId, InstanceId, OperationId, ToolId};
use crate::migrate::CURRENT_VERSION;
use crate::model::{
    ClearanceStrategy, CornerRelief, CutDirection, DrillCycle, DrillOp, EngraveOp, Entry,
    FeatureDefinition, FeatureInstance, MachineOrigin, MachiningOrder, Operation, OriginXy,
    OriginZ, PocketOp, ProfileOp, Project, Side, Stock, TabPlacement, TabShape, TabSpec, Tool,
    ToolKind, Units, WallPass,
};
use crate::post_processor::{PostProcessorRef, bundled_definition};
use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, Point, Polyline, Transform};
use std::collections::BTreeMap;

/// A reference to the bundled GRBL definition, version 1, with its
/// default settings. Pinned to version 1, not the latest, so the committed
/// fixtures stay valid when a version is added.
#[allow(
    clippy::expect_used,
    reason = "GRBL version 1 is bundled for good; a failure is a broken build"
)]
pub fn grbl_post_processor() -> PostProcessorRef {
    bundled_definition("grbl", 1)
        .expect("GRBL version 1 is bundled")
        .default_ref()
}

/// A 6.35 mm two-flute up-cut end mill with every value set.
pub fn tool_6_35_upcut() -> Tool {
    Tool {
        id: ToolId::new("t1"),
        name: "6.35 mm upcut".into(),
        number: None,
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
/// `tests/fixtures/dcv/v9/sample.dcv`.
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
        arc_fit_tolerance_mm: Some(0.01),
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
            z_top_mm: Some(6.0),
            z_bottom_mm: Some(0.0),
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
                // As every older fixture migrates (#109); the rectangle
                // has no hole, so either value cuts the same.
                on_holes: true,
            }),
            machining_order: MachiningOrder::FullDepthFirst,
            entry: Entry::Plunge,
            lead: None,
            corner_relief: CornerRelief::None,
        })],
        post_processor: grbl_post_processor(),
    }
}

/// The sample project's Profile operation, to edit. The sample has
/// exactly one operation, and it is a Profile.
#[allow(
    clippy::panic,
    reason = "the sample project is built above with a Profile first"
)]
pub fn profile_mut(project: &mut Project) -> &mut ProfileOp {
    match project.operations.first_mut() {
        Some(Operation::Profile(op)) => op,
        _ => panic!("the sample project starts with a Profile operation"),
    }
}

/// A valid 3 mm deep offset Pocket of the sample rectangle with the sample
/// tool, id `o2`, every phase 2 field at its `init` value.
pub fn sample_pocket() -> PocketOp {
    PocketOp {
        id: OperationId::new("o2"),
        name: "Pocket".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        strategy: ClearanceStrategy::Offset,
        wall_pass: WallPass::Last,
        depth_mm: Some(3.0),
        step_down_mm: None,
        step_over_fraction: None,
        direction: CutDirection::Climb,
        allowance_mm: 0.0,
        rest_of: None,
        entry: Entry::Plunge,
        corner_relief: CornerRelief::None,
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
    }
}

/// A valid simple Drill of the sample rectangle with the sample tool, id
/// `o3`, 3 mm deep, retracting 1 mm above the hole.
pub fn sample_drill() -> DrillOp {
    DrillOp {
        id: OperationId::new("o3"),
        name: "Drill".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        cycle: DrillCycle::Simple,
        dwell_s: 0.0,
        depth_mm: Some(3.0),
        retract_mm: 1.0,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
    }
}

/// A valid 1 mm deep Engrave of the sample rectangle with the sample tool,
/// id `o4`.
pub fn sample_engrave() -> EngraveOp {
    EngraveOp {
        id: OperationId::new("o4"),
        name: "Engrave".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        depth_mm: Some(1.0),
        step_down_mm: None,
        alternate_direction: false,
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
    }
}
