// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Golden parity: DreamCarve's output against PureCutCNC's, captured into
//! `tests/golden/<case>/` by `tools/parity/`
//! (`docs/specs/phase-1/functional.md`, acceptance test 14;
//! `docs/specs/phase-2/functional.md`, acceptance test 38;
//! `docs/specs/phase-2/design.md`, "Test plan → dc-gcode").
//!
//! Each test has the DreamCarve project equivalent to the case's
//! PureCutCNC project (`tools/parity/cases/<case>.camj`): the phase 1
//! Profile cases build it here, and the phase 2 cases load it from
//! `tests/golden/<case>/dreamcarve.dcv`. PureCutCNC is Y-down with its
//! origin at the stock's top-left corner (Y-down) and Z zero at the stock
//! top; DreamCarve is Y-up (ADR-0001), so every DreamCarve Y is the stock
//! height minus the PureCutCNC Y, and the origin is the bottom-left corner,
//! Z zero at the stock top. Both programs then share one machine frame and
//! are compared in it. What the harness checks is in [`harness::check`];
//! each case's README lists the differences.
//!
//! Every phase 2 case uses 150 × 100 × 12 mm stock and a 6.35 mm end mill
//! at feed 1500, plunge 500 and 18000 RPM unless its comment says
//! otherwise, with climb milling and safe Z 5 mm above the stock top.

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
use harness::{Case, Difference, Order};

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
        thickness_mm: Some(THICKNESS_MM),
    };
    project.origin = MachineOrigin {
        xy: OriginXy::BottomLeft,
        z: OriginZ::StockTop,
    };
    project.post_processor =
        dc_project::PostProcessorRef::latest("grblhal").expect("grblHAL is bundled");
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
            z_top_mm: Some(THICKNESS_MM),
            z_bottom_mm: Some(0.0),
        });
        targets.push(instance);
    }

    let Some(Operation::Profile(op)) = project.operations.first_mut() else {
        panic!("sample project has a profile operation");
    };
    op.targets = targets;
    op.side = side;
    op.depth_mm = Some(depth_mm);
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
        order: Order::Sets,
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
        order: Order::Sets,
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
        order: Order::Sets,
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
                on_holes: false,
            }),
        ),
        differences: vec![Difference::RectTabStepsAtPlungeRate],
        order: Order::Sets,
    });
}

/// A phase 2 case: its DreamCarve project is
/// `tests/golden/<name>/dreamcarve.dcv`.
fn case(name: &'static str, differences: Vec<Difference>, order: Order) -> Case {
    Case {
        name,
        project: harness::load(name),
        differences,
        order,
    }
}

/// Offset Pocket of a 60 × 40 mm rectangle at (30, 30) Y-up ((30, 30)
/// Y-down too: the stock is 100 high and the pocket centred), 6 mm deep in
/// 3 mm passes at step-over 0.4. The rings are compared per level as sets;
/// the diagonal ring-to-ring links, which move with the rings' start
/// corner, by their lengths. PureCutCNC's `roundOutsideCorners` is off: it
/// would round the inner rings' corners, which DreamCarve keeps sharp.
#[test]
fn pocket_offset_rect() {
    harness::check(&case("pocket-offset-rect", vec![], Order::Sets));
}

/// Offset Pocket of an 80 × 60 mm rectangle at (20, 20) round a 20 mm
/// circular island at (60, 50), 6 mm deep in 3 mm passes. The `.camj`
/// island is a 360-sided polygon on the circle, because PureCutCNC samples
/// a `circle` too coarsely for 0.01 mm; DreamCarve's is a true circle.
#[test]
fn pocket_offset_island() {
    harness::check(&case("pocket-offset-island", vec![], Order::Sets));
}

/// Raster Pocket of the `pocket-offset-rect` rectangle at 0°, wall pass
/// first as PureCutCNC cuts it. The lines are compared without direction
/// and the links between them not at all ([`Difference::RasterLineOrder`]).
#[test]
fn pocket_raster_0deg() {
    harness::check(&case(
        "pocket-raster-0deg",
        vec![Difference::RasterLineOrder],
        Order::Sets,
    ));
}

/// As `pocket_raster_0deg` at 45°: lines along (1, 1). PureCutCNC rotates
/// by −`pocketAngle` in its Y-down frame, so its +45° is DreamCarve's 135°
/// (#146); the `.camj` says `pocketAngle: -45`.
#[test]
fn pocket_raster_45deg() {
    harness::check(&case(
        "pocket-raster-45deg",
        vec![Difference::RasterLineOrder],
        Order::Sets,
    ));
}

/// The rectangle roughed with the 6.35 mm tool leaving 0.5 mm, then
/// cleared with a 3.175 mm tool (feed 1200, plunge 400) in one program with
/// an `M6`. DreamCarve's second Pocket is `rest_of` the first; PureCutCNC's
/// is a finish pass on the rough's `stockToLeaveRadial`
/// ([`Difference::RestMachiningAgainstFinishPass`]).
#[test]
fn pocket_rough_then_rest() {
    harness::check(&case(
        "pocket-rough-then-rest",
        vec![Difference::RestMachiningAgainstFinishPass],
        Order::Sets,
    ));
}

/// Four 6 mm circles drilled 8 mm deep with a 5 mm drill (feed 1500,
/// plunge 300, 12000 RPM), retract 1 mm. They are drawn at (20, 50),
/// (95, 65), (130, 45) and (55, 30) Y-up and drilled nearest-first from the
/// first: (20, 50), (55, 30), (95, 65), (130, 45). PureCutCNC's walk starts
/// elsewhere but gives the same order here. Holes are compared in order.
#[test]
fn drill_simple() {
    harness::check(&case("drill-simple", vec![], Order::Sets));
}

/// The same holes 12 mm deep in 4 mm pecks with full retracts: DreamCarve
/// pecks to Z −3, −7, −11 and −12 from its retract plane at 1, PureCutCNC to
/// −4, −8 and −12 from the stock top ([`Difference::PecksFromRetractPlane`],
/// #153).
#[test]
fn drill_peck() {
    harness::check(&case(
        "drill-peck",
        vec![Difference::PecksFromRetractPlane],
        Order::Sets,
    ));
}

/// Engrave, 3 mm deep in 1.5 mm passes: an open zigzag through (20, 20),
/// (50, 40), (80, 20) and (110, 40), and a closed rectangle drawn from
/// (30, 80) clockwise in the machine frame, the way the `.camj` draws it
/// (Engrave follows the drawing).
#[test]
fn engrave_open_closed() {
    harness::check(&case("engrave-open-closed", vec![], Order::Sets));
}

/// Outside Profile of a 100 × 50 mm rectangle at (25, 25), 12 mm deep in
/// 4 mm passes, with a 15° ramp. DreamCarve's ramp starts at the lap's
/// entry point and later passes ramp down from the previous one; PureCutCNC
/// centres its zigzag elsewhere and ramps from safe Z every pass
/// ([`Difference::EntryPlacement`], #165, #174).
#[test]
fn profile_ramp() {
    harness::check(&case(
        "profile-ramp",
        vec![Difference::EntryPlacement],
        Order::Sets,
    ));
}

/// Inside Profile of a 40 mm circle at (75, 50), 12 mm deep in 4 mm
/// passes, entered by a helix half the tool's diameter at 10°. Both
/// programs helix down counter-clockwise (climb inside) on a 3.175 mm
/// circle; DreamCarve places it at its lap start and later passes rejoin
/// the bore, PureCutCNC places it elsewhere and helixes from safe Z every
/// pass ([`Difference::EntryPlacement`]).
#[test]
fn profile_helix_inside() {
    harness::check(&case(
        "profile-helix-inside",
        vec![Difference::EntryPlacement],
        Order::Sets,
    ));
}

/// Outside Profile of a 100 × 50 mm rectangle at (25, 25), one 4 mm pass,
/// with a 6.35 mm, 90° lead-in and lead-out. PureCutCNC leads only with a
/// ramped entry, so both enter by a 15° ramp. DreamCarve's rectangle is
/// drawn from its top-right corner so that its contour starts where
/// PureCutCNC's does: the lead's arrival ties on the contour's edge order
/// (#185). The lead arcs match; their feed differs by design
/// ([`Difference::LeadArcsAtCuttingFeed`]). One pass, because DreamCarve
/// plans one lead per path and PureCutCNC one per level.
#[test]
fn profile_lead() {
    harness::check(&case(
        "profile-lead",
        vec![
            Difference::EntryPlacement,
            Difference::LeadArcsAtCuttingFeed,
        ],
        Order::Sets,
    ));
}

/// Inside Profile of a 40 × 30 mm rectangle at (55, 35), 6 mm deep in
/// 3 mm passes, with dogbones: four notches along the corner bisectors,
/// cut out and back at the tool's 4 mm step-down levels, Z −4 and −6.
/// The notch at the lap's start and end rises from the lap at Z −6 to
/// Z −4 rather than retracting ([`Difference::NotchRisesFromLapEnd`]).
#[test]
fn profile_dogbone() {
    harness::check(&case(
        "profile-dogbone",
        vec![Difference::NotchRisesFromLapEnd],
        Order::Sets,
    ));
}

/// Three 30 × 30 mm parts at x 10, 60 and 110 (y 35), each with a 12 mm
/// hole at its centre, cut through in 4 mm passes under
/// `full_depth_first` (PureCutCNC `feature_first`). PureCutCNC routes an
/// inside edge only round `subtract` features and an outside edge only
/// round `add` ones, so both projects have two operations: the holes, then
/// the parts. Compared in order: each shape to full depth, nearest first.
#[test]
fn order_full_depth_first() {
    harness::check(&case("order-full-depth-first", vec![], Order::InOrder));
}

/// The `order_full_depth_first` parts under `pass_first` (PureCutCNC
/// `level_first`). The passes are compared in order, each as a set; the
/// order of shapes within a pass differs by design
/// ([`Difference::PassOrderWithinLayer`]).
#[test]
fn order_pass_first() {
    harness::check(&case(
        "order-pass-first",
        vec![Difference::PassOrderWithinLayer],
        Order::InOrder,
    ));
}
