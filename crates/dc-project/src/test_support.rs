// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Fixture builders for tests in this crate and, with the `test-support`
//! feature, in crates that depend on it (`docs/rust-guidelines.md`, "Tests").

use crate::ids::{DefinitionId, InstanceId, OperationId, TabId, ToolId};
use crate::migrate::CURRENT_VERSION;
use crate::model::{
    BoreOp, ClearanceStrategy, ClearingPass, CornerRelief, CutDirection, DrillCycle, DrillOp,
    EngraveOp, Entry, FeatureDefinition, FeatureInstance, Heights, InlayOp, InlaySide,
    MachineOrigin, MachiningOrder, Operation, OriginXy, OriginZ, PlacedTab, PocketOp, ProfileOp,
    Project, Side, StartAt, Stock, TabPlacement, TabShape, TabSpec, Tool, ToolKind, Units,
    VCarveOp, VCarveStrategy, WallPass,
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
        vendor: None,
        product_id: None,
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

/// A 90°, 6.35 mm (1/4 in) V-bit, id `t2`, with every value set: its
/// cone is 3.175 mm tall.
pub fn tool_v_bit_90() -> Tool {
    Tool {
        id: ToolId::new("t2"),
        name: "90° V-bit".into(),
        cutter: ToolKind::VBit {
            included_angle_deg: 90.0,
        },
        step_down_mm: None,
        ..tool_6_35_upcut()
    }
}

/// A valid project with no validation problems: 100 × 100 × 6 mm stock, one
/// 50 × 30 mm rectangle placed at (10, 10), one tool, and one outside climb
/// profile through the stock with a request for four tabs, none placed
/// yet. Matches the committed fixture `tests/fixtures/dcv/v24/sample.dcv`.
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
        heights: Heights::unset(),
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
                // As every older fixture migrates (#285).
                avoid_corners: false,
            }),
            machining_order: MachiningOrder::FullDepthFirst,
            entry: Entry::Plunge,
            lead: None,
            corner_relief: CornerRelief::None,
            last_pass: None,
            start: StartAt::Auto,
        })],
        // No placed tabs: the Profile's tab request is placed by `dc-cam`
        // (#105), so the sample is a project as a format 10 file migrates.
        tabs: Vec::new(),
        post_processor: grbl_post_processor(),
    }
}

/// A 6 × 2 mm rect tab `id` on the sample rectangle's outline, its centre
/// at (`x_mm`, `y_mm`) in definition space (#105), placed by hand: it
/// does not hold a slug (#302).
pub fn sample_tab(id: &str, x_mm: f64, y_mm: f64) -> PlacedTab {
    PlacedTab {
        id: TabId::new(id),
        instance: InstanceId::new("i1"),
        profile: 0,
        x_mm,
        y_mm,
        width_mm: 6.0,
        height_mm: 2.0,
        shape: TabShape::Rect,
        holds_slug: Some(false),
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
        stock_to_leave_mm: 0.0,
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
        allow_tool_wider_than_hole: false,
    }
}

/// A valid Bore of the sample rectangle with the sample tool, id `o5`,
/// 3 mm deep, climb, at the tool's step-down per turn (format 18).
pub fn sample_bore() -> BoreOp {
    BoreOp {
        id: OperationId::new("o5"),
        name: "Bore".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        depth_mm: Some(3.0),
        pitch_mm: None,
        direction: CutDirection::Climb,
        stock_to_leave_mm: 0.0,
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

/// A valid medial V-Carve of the sample rectangle with
/// [`tool_v_bit_90`] (`t2`, which the caller adds to the project), id
/// `o6`: no flat depth, no clearing, from the stock top (format 20).
pub fn sample_v_carve() -> VCarveOp {
    VCarveOp {
        id: OperationId::new("o6"),
        name: "V-Carve".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t2"),
        strategy: VCarveStrategy::Medial,
        clearing: Vec::new(),
        flat_depth_mm: None,
        start_depth_mm: 0.0,
        start_plane_cleared_externally: false,
        step_over_fraction: None,
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
    }
}

/// A valid female Inlay of the sample rectangle with [`tool_v_bit_90`]
/// (`t2`, which the caller adds to the project), id `o7` (format 22):
/// Corbin's superficial-inlay values, `depth_mm` 2.5, `glue_gap_mm` 0.25,
/// `plug_depth_mm` 1.5, and a 6 mm `margin_mm`, no clearing. The female
/// engages the bit 2.5 mm, within its 3.175 mm cone; the male of the
/// same values engages it 3.75 mm (security review H4).
pub fn sample_inlay() -> InlayOp {
    InlayOp {
        id: OperationId::new("o7"),
        name: "Inlay".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t2"),
        side: InlaySide::Female,
        depth_mm: Some(2.5),
        glue_gap_mm: Some(0.25),
        plug_depth_mm: Some(1.5),
        margin_mm: Some(6.0),
        outline_rounding_mm: None,
        clearing: Vec::new(),
        step_over_fraction: None,
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
        cut_out: None,
    }
}

/// The ridge between neighbouring cells of [`line_art_project`], mm,
/// left to right: 0.2, 0.5 and 1.
pub const LINE_ART_RIDGES_MM: [f64; 3] = [0.2, 0.5, 1.0];

/// A thin wedge of material `length_mm` long, `half_deg` either side of
/// its axis, standing out of a wall at `base` toward `heading_deg`: the
/// end of a drawn line that stops inside a region. Its foot's two points
/// and its tip, as `[left, tip, right]` seen looking along the heading.
fn wedge(base: (f64, f64), heading_deg: f64, length_mm: f64, half_deg: f64) -> [(f64, f64); 3] {
    let (sin, cos) = heading_deg.to_radians().sin_cos();
    let half_mm = length_mm * half_deg.to_radians().tan();
    let (ax, ay) = (-sin * half_mm, cos * half_mm);
    [
        (base.0 + ax, base.1 + ay),
        (base.0 + cos * length_mm, base.1 + sin * length_mm),
        (base.0 - ax, base.1 - ay),
    ]
}

/// A closed polyline through `points`, straight edges.
fn closed(points: &[(f64, f64)]) -> Polyline {
    Polyline::new(
        points
            .iter()
            .map(|&(x, y)| dc_geometry::Vertex::line(x, y))
            .collect(),
        true,
    )
}

/// A synthetic line-art design (#463), as a drawing traced to regions
/// gives one: four 12 mm cells in a row, A to D, the ridges between them
/// [`LINE_ART_RIDGES_MM`] wide, under a 48 × 20 mm region E 0.5 mm above
/// them. A and E have dangling line ends (thin wedges of material, up to a
/// few medial steps long, standing into the region from its outline); B
/// holds a 4 mm island with a 2 mm region in it, C a diamond island with a
/// line end of its own, D an island with two small regions in it. Placed
/// at (8, 8) as instance `i1` on 64 × 50 × 12 mm stock, with a 60° 6.35 mm
/// V-bit `t1` (floor step-over 0.1) and a 3.175 mm end mill `t2`; no
/// operations. Before #463 every carve of it stopped at E's line ends.
pub fn line_art_project() -> Project {
    let [r0, r1, r2] = LINE_ART_RIDGES_MM;
    let (b0, c0, d0) = (12.0 + r0, 24.0 + r1, 36.0 + r2);
    // A: a line end down from its top edge, tilted, and one in from its
    // right edge.
    let top = wedge((5.0, 12.0), -73.0, 0.375, 6.0);
    let right = wedge((12.0, 6.0), 197.0, 0.6, 11.0);
    let a = closed(&[
        (0.0, 0.0),
        (12.0, 0.0),
        right[0],
        right[1],
        right[2],
        (12.0, 12.0),
        top[0],
        top[1],
        top[2],
        (0.0, 12.0),
    ]);
    // E: two short line ends down from its top edge, each half a medial
    // step long (E's step is 20 / 160 mm), one straight, one slanted.
    let slanted = wedge((36.0, 32.5), -60.0, 0.0625, 20.0);
    let straight = wedge((12.0, 32.5), -90.0, 0.0625, 20.0);
    let e = closed(&[
        (0.0, 12.5),
        (48.0, 12.5),
        (48.0, 32.5),
        slanted[0],
        slanted[1],
        slanted[2],
        straight[0],
        straight[1],
        straight[2],
        (0.0, 32.5),
    ]);
    let rect =
        |x0: f64, y0: f64, x1: f64, y1: f64| closed(&[(x0, y0), (x1, y0), (x1, y1), (x0, y1)]);
    // C's island: a diamond with a line end out of its right corner.
    let out = wedge((32.0, 6.0), 17.0, 0.5, 6.0);
    let island = closed(&[
        (30.0, 3.0),
        out[2],
        out[1],
        out[0],
        (30.0, 9.0),
        (28.0, 6.0),
    ]);
    let design = FeatureDefinition {
        id: DefinitionId::new("d1"),
        name: "Line art".into(),
        profiles: vec![
            a,
            rect(b0, 0.0, 24.0, 12.0),
            rect(16.0, 4.0, 20.0, 8.0),
            rect(17.0, 5.0, 19.0, 7.0),
            rect(c0, 0.0, 36.0, 12.0),
            island,
            rect(d0, 0.0, 48.0, 12.0),
            rect(39.0, 2.0, 46.0, 10.0),
            rect(40.0, 3.0, 41.0, 4.0),
            closed(&[(42.0, 7.0), (44.0, 7.0), (43.0, 8.5)]),
            e,
        ],
    };
    let mut project = sample_project();
    project.name = "Line art".into();
    project.stock = Stock {
        width_mm: 64.0,
        height_mm: 50.0,
        thickness_mm: Some(12.0),
    };
    project.definitions = BTreeMap::from([(design.id.clone(), design)]);
    project.instances = vec![FeatureInstance {
        id: InstanceId::new("i1"),
        definition: DefinitionId::new("d1"),
        transform: Transform::translate(8.0, 8.0),
        name: "Line art".into(),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    }];
    project.tools = vec![
        Tool {
            id: ToolId::new("t1"),
            name: "60° V-bit".into(),
            cutter: ToolKind::VBit {
                included_angle_deg: 60.0,
            },
            step_down_mm: None,
            step_over_fraction: Some(0.1),
            ..tool_6_35_upcut()
        },
        Tool {
            id: ToolId::new("t2"),
            name: "3.175 mm end mill".into(),
            diameter_mm: 3.175,
            ..tool_6_35_upcut()
        },
    ];
    project.operations = Vec::new();
    project
}

/// An Inlay `o1` of [`line_art_project`]'s design, side `side`, as the
/// maintainer's line-art inlay (#463): `depth_mm` 4, `glue_gap_mm` 1,
/// `plug_depth_mm` 2, a 6 mm margin, the V-bit's floor at step-over 0.1,
/// and a clearing pass with the end mill, offset, climb, ramping in at
/// 15°.
pub fn line_art_inlay(side: InlaySide) -> InlayOp {
    InlayOp {
        id: OperationId::new("o1"),
        name: "Line art".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        side,
        depth_mm: Some(4.0),
        glue_gap_mm: Some(1.0),
        plug_depth_mm: Some(2.0),
        margin_mm: Some(6.0),
        outline_rounding_mm: None,
        clearing: vec![ClearingPass {
            tool: ToolId::new("t2"),
            strategy: ClearanceStrategy::Offset,
            direction: CutDirection::Climb,
            entry: Entry::Ramp { angle_deg: 15.0 },
        }],
        step_over_fraction: Some(0.1),
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 10.0,
        cut_out: None,
    }
}

/// A plain medial V-Carve `o2` of [`line_art_project`]'s design with its
/// V-bit: a 1.5 mm flat depth at step-over 0.1, no clearing.
pub fn line_art_v_carve() -> VCarveOp {
    VCarveOp {
        id: OperationId::new("o2"),
        name: "Line art".into(),
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        flat_depth_mm: Some(1.5),
        step_over_fraction: Some(0.1),
        ..sample_v_carve()
    }
}
