// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/xyLead.test.ts
//   src/engine/toolpaths/xyLeadEdge.test.ts

//! Lead-in and lead-out (`docs/specs/phase-2/design.md`, "Test plan →
//! dc-cam — lead"; functional spec capability 17, acceptance 28). Tests
//! marked "port" follow PureCutCNC's `xyLead.test.ts` or
//! `xyLeadEdge.test.ts` on their fixtures: a 6 mm tool, a 100 × 100 open
//! domain, square rings from 20 to 80. Upstream's radius ladder is in tool
//! diameters; DreamCarve's starts at the user's radius, so the ports set
//! the radius to one diameter (6 mm), which gives upstream's ladder.
//! Upstream's leads are tessellated; DreamCarve's are true arcs, so a
//! tangent is exact rather than within a tessellation step. Upstream's
//! rings are wound counter-clockwise on screen (Y-down); read as Y-up the
//! same coordinates run counter-clockwise too, and the free side is found
//! by the probe either way, so no expected value changes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::panic,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "tests"
)]

use super::*;
use crate::profile::generate_profile;
use crate::test_support::assert_safe;
use crate::toolpath::{Move, MoveKind, Toolpath};
use crate::warning::{Warning, WarningCode};
use dc_geometry::{Contour, EPS_MM, Point3, Region, Transform, Vertex, assert_approx_eq, nest};
use dc_project::test_support::sample_project;
use dc_project::{
    CornerRelief, CutDirection, DefinitionId, Entry, FeatureDefinition, FeatureInstance,
    InstanceId, MachiningOrder, Operation, OperationId, ProfileOp, Project, Side, Stock,
    TabPlacement, TabShape, TabSpec, Tool, ToolId, ToolKind,
};
use std::collections::BTreeMap;
use std::f64::consts::{PI, SQRT_2};

/// Upstream `TOOL_DIAMETER`.
const D: f64 = 6.0;

// ── Planner fixtures (port) ──────────────────────────────────────────

/// Upstream `squareRing`: a square from `min` to `max`, `per_side`
/// vertices along each side, counter-clockwise from (`min`, `min`).
fn square_ring(min: f64, max: f64, per_side: u32) -> Polyline {
    let mut vertices = Vec::new();
    let mut push = |from: (f64, f64), to: (f64, f64)| {
        for step in 0..per_side {
            let t = f64::from(step) / f64::from(per_side);
            vertices.push(Vertex::line(
                from.0 + (to.0 - from.0) * t,
                from.1 + (to.1 - from.1) * t,
            ));
        }
    };
    push((min, min), (max, min));
    push((max, min), (max, max));
    push((max, max), (min, max));
    push((min, max), (min, min));
    Polyline::new(vertices, true)
}

fn ring_of(points: &[(f64, f64)]) -> Polyline {
    Polyline::new(
        points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
        true,
    )
}

fn region_of(path: Polyline) -> Region {
    let (regions, _) = nest(vec![Contour::new(path).unwrap()]).unwrap();
    regions.into_iter().next().unwrap()
}

/// Upstream `DOMAIN_OUTER`: the open 100 × 100 domain.
fn open_domain() -> EntryDomain {
    EntryDomain::new(
        &[region_of(Polyline::rect(
            Point::new(0.0, 0.0),
            100.0,
            100.0,
        ))],
        D,
        0.01,
    )
}

/// Upstream `domain(predicate)`: one predicate for both the near-join and
/// the clear-of-walls test, as upstream's fixtures set both.
struct Pred<F: Fn(Point) -> bool>(F);

impl<F: Fn(Point) -> bool> LeadSpace for Pred<F> {
    fn inside(&self, p: Point) -> bool {
        (self.0)(p)
    }
    fn clear_of_walls(&self, p: Point) -> bool {
        (self.0)(p)
    }
    fn safety_mm(&self) -> f64 {
        0.1 * D
    }
}

/// A lead of `radius_mm` sweeping 90°, with a lead-out and no overcut.
fn lead_of(radius_mm: f64) -> Lead {
    Lead {
        radius_mm,
        sweep_deg: 90.0,
        lead_out: true,
        overcut_mm: 0.0,
    }
}

fn angle_between(a: Vec2, b: Vec2) -> f64 {
    a.cross(b).atan2(a.dot(b)).abs()
}

fn unit(from: Point, to: Point) -> Vec2 {
    (to - from).normalized().unwrap()
}

fn start_of(path: &Polyline) -> Point {
    path.vertices()[0].point()
}

/// The tangent at the start of `path`'s first segment.
fn start_tangent(path: &Polyline) -> Vec2 {
    path.segments().next().unwrap().point_at(0.0).unwrap().1
}

// ── Planner tests (port) ─────────────────────────────────────────────

/// Port: upstream `testEntryArrivesTangentToTheRing`. The lead-in ends on
/// the re-seamed contour's start, along the contour's own tangent there
/// (exactly: the arc is a true arc), and its staging point is off the ring.
#[test]
fn entry_arrives_tangent_to_the_ring() {
    let ring = square_ring(20.0, 80.0, 6);
    let domain = open_domain();
    let space = DomainSpace::new(&domain, D, 0.0);
    let plan = plan_lead_in(&ring, &lead_of(D), D, &space, None).expect("a lead on an open ring");
    assert!(plan.arrival().distance_to(start_of(&plan.seam)) < 1e-9);
    let approach = plan.arc.tangent_at(plan.arrival());
    assert!(angle_between(approach, start_tangent(&plan.seam)) < 1e-9);
    let offset = plan.staging().distance_to(plan.arrival());
    assert!(offset > 0.25 * D, "staging is off the ring ({offset})");
}

/// Port: upstream `testExitDepartsTangentFromTheRing`. The lead-out starts
/// where the lap ended, continues the closing tangent, and ends off the
/// ring.
#[test]
fn exit_departs_tangent() {
    let ring = square_ring(20.0, 80.0, 6);
    let domain = open_domain();
    let space = DomainSpace::new(&domain, D, 0.0);
    let out = plan_lead_out(&ring, &lead_of(D), D, &space);
    assert!(out.overcut.is_none(), "no overcut was asked for");
    let arc = out.arc.expect("a lead-out on an open ring");
    let start = start_of(&ring);
    assert!(arc.from.distance_to(start) < 1e-9);
    let last = ring.vertices().last().unwrap().point();
    let closing = unit(last, start);
    assert!(angle_between(arc.tangent_at(arc.from), closing) < 1e-9);
    assert!(arc.to.distance_to(start) > 0.25 * D);
}

/// Port: upstream `testJoinAvoidsCornersThenTakesTheNearest`. On a 60 × 40
/// rectangle in open space every candidate fits, so ranking decides: the
/// join (and the seam) is more than a diameter from every corner, and
/// coming from either end of the top the join moves that way.
#[test]
fn join_avoids_corners_then_takes_nearest() {
    let corners = [(20.0, 20.0), (80.0, 20.0), (80.0, 60.0), (20.0, 60.0)];
    let ring = ring_of(&corners);
    let corner_distance = |p: Point| {
        corners
            .iter()
            .map(|&(x, y)| p.distance_to(Point::new(x, y)))
            .fold(f64::MAX, f64::min)
    };
    let domain = open_domain();
    let space = DomainSpace::new(&domain, D, 0.0);
    let plan = plan_lead_in(&ring, &lead_of(D), D, &space, None).unwrap();
    assert!(corner_distance(plan.arrival()) > D, "{:?}", plan.arrival());
    assert!(corner_distance(start_of(&plan.seam)) > D);

    let near = plan_lead_in(&ring, &lead_of(D), D, &space, Some(Point::new(22.0, 90.0))).unwrap();
    let far = plan_lead_in(&ring, &lead_of(D), D, &space, Some(Point::new(78.0, 90.0))).unwrap();
    assert!(near.arrival().x < far.arrival().x, "{near:?} {far:?}");
    assert!(corner_distance(near.arrival()) > D);
    assert!(corner_distance(far.arrival()) > D);
}

/// Port: upstream `testAFilletedCornerRanksLikeAMitredOne`. Turn is summed
/// over a diameter either way of a candidate, so a corner spread over many
/// vertices, or over a true arc, ranks as a corner: the join stays clear
/// of the whole fillet. Upstream's tessellated fixture, then the same
/// corner as one arc segment, which upstream cannot express.
#[test]
fn filleted_corner_ranks_like_mitred() {
    let radius = 6.0;
    let mut points = vec![(20.0, 20.0), (80.0, 20.0)];
    for step in 0..=18 {
        let angle = -PI / 2.0 + f64::from(step) / 18.0 * (PI / 2.0);
        points.push((
            80.0 + radius * angle.cos(),
            60.0 - radius + radius * angle.sin(),
        ));
    }
    points.push((20.0, 60.0));
    let tessellated = ring_of(&points);
    let arc_fillet = Polyline::new(
        vec![
            Vertex::line(20.0, 20.0),
            Vertex::line(80.0, 20.0),
            Vertex::new(80.0, 54.0, (PI / 8.0).tan()),
            Vertex::line(74.0, 60.0),
            Vertex::line(20.0, 60.0),
        ],
        true,
    );
    let domain = open_domain();
    let space = DomainSpace::new(&domain, D, 0.0);
    let centre = Point::new(80.0 - radius, 60.0 - radius);
    for ring in [tessellated, arc_fillet] {
        let plan = plan_lead_in(&ring, &lead_of(D), D, &space, None).unwrap();
        let to_centre = plan.arrival().distance_to(centre);
        assert!(to_centre > radius + D, "{to_centre} for {ring:?}");
    }
}

/// Port: upstream `testACircleFallsThroughToTravel`. A circle has no
/// corner, so every candidate ties on turn and travel decides: the join is
/// on the side the tool comes from. Upstream's 72-vertex circle, then a
/// true circle of two arcs.
#[test]
fn circle_falls_through_to_travel() {
    let tessellated = Polyline::new(
        (0..72)
            .map(|step| {
                let angle = f64::from(step) / 72.0 * 2.0 * PI;
                Vertex::line(50.0 + 20.0 * angle.cos(), 50.0 + 20.0 * angle.sin())
            })
            .collect(),
        true,
    );
    let true_circle = Polyline::circle(Point::new(50.0, 50.0), 20.0);
    let domain = open_domain();
    let space = DomainSpace::new(&domain, D, 0.0);
    for ring in [tessellated, true_circle] {
        let east = plan_lead_in(&ring, &lead_of(D), D, &space, Some(Point::new(95.0, 50.0)));
        let west = plan_lead_in(&ring, &lead_of(D), D, &space, Some(Point::new(5.0, 50.0)));
        assert!(east.unwrap().arrival().x > 60.0, "east");
        assert!(west.unwrap().arrival().x < 40.0, "west");
    }
}

/// Upstream `strip(h)`: a band `h` deep inward from the square ring's
/// bottom edge, and nothing else.
fn strip(h: f64) -> impl Fn(Point) -> bool {
    move |p: Point| p.y >= 20.0 - 1e-9 && p.y <= 20.0 + h && p.x >= 15.0 && p.x <= 85.0
}

/// Port: upstream `testRadiusLadderTakesTheWidestArcThatFits`, from the
/// user's radius. Open space takes the radius and sweep asked for; a strip
/// a 90° arc of that radius cannot fit keeps the radius and shortens the
/// sweep (the ladder is radius-major); a very shallow strip takes the
/// ladder's last rung, a quarter of the radius.
#[test]
fn radius_ladder_takes_widest_that_fits() {
    let ring = square_ring(20.0, 80.0, 6);
    let domain = open_domain();
    let open = plan_lead_in(
        &ring,
        &lead_of(D),
        D,
        &DomainSpace::new(&domain, D, 0.0),
        None,
    )
    .unwrap();
    assert_approx_eq!(open.arc.radius_mm, D, 1e-9);
    assert_approx_eq!(open.arc.sweep_deg, 90.0, 1e-9);
    let chord = open.staging().distance_to(open.arrival());
    assert_approx_eq!(chord, D * SQRT_2, 1e-9);

    let narrowed = plan_lead_in(&ring, &lead_of(D), D, &Pred(strip(3.5)), None).unwrap();
    assert_approx_eq!(narrowed.arc.radius_mm, D, 1e-9);
    assert!(narrowed.staging().distance_to(narrowed.arrival()) < chord);

    let tight = plan_lead_in(&ring, &lead_of(D), D, &Pred(strip(0.5)), None).unwrap();
    assert_approx_eq!(tight.arc.radius_mm, 0.25 * D, 1e-9);
}

/// Port: upstream `testDomainRejectionLeavesNoLead`. A domain that is the
/// ring's own path admits no curve off it, so neither a lead-in nor a
/// lead-out is planned. Through the generator, a slot barely wider than
/// the tool takes no lead and says so with `lead_dropped` (`no_room`).
#[test]
fn domain_rejection_leaves_no_lead_and_warns() {
    let ring = square_ring(20.0, 80.0, 6);
    let hairline = Pred(|p: Point| {
        let on_vertical = ((p.x - 20.0).abs() < 1e-9 || (p.x - 80.0).abs() < 1e-9)
            && (20.0..=80.0).contains(&p.y);
        let on_horizontal = ((p.y - 20.0).abs() < 1e-9 || (p.y - 80.0).abs() < 1e-9)
            && (20.0..=80.0).contains(&p.x);
        on_vertical || on_horizontal
    });
    assert!(plan_lead_in(&ring, &lead_of(D), D, &hairline, None).is_none());
    assert!(
        plan_lead_out(&ring, &lead_of(D), D, &hairline)
            .arc
            .is_none()
    );

    let p = project_of(&[("slot", vec![rect(20.0, 20.0, 6.5, 30.0)])]);
    let (tp, warnings) = generate(&p, &inside(&p, Some(lead_of(3.0))));
    let dropped = codes(&warnings, WarningCode::LeadDropped);
    assert_eq!(dropped.len(), 1, "{warnings:?}");
    assert_eq!(
        dropped_of(dropped[0]),
        (LeadDroppedReason::NoRoom, Some("slot"))
    );
    assert!(arcs(&tp).is_empty(), "no lead arc is cut");
}

/// Port: upstream `testSweptSamplingSeesBetweenVertices`. The check every
/// candidate passes samples between the 5° steps upstream tessellates at:
/// a forbidden spot on a quarter circle that misses every 5° vertex is
/// still found. A space that refuses everything admits no lead.
#[test]
fn swept_sampling_sees_between_vertices() {
    let arc = LeadArc {
        from: Point::new(6.0, 0.0),
        to: Point::new(0.0, 6.0),
        centre: Point::new(0.0, 0.0),
        ccw: true,
        radius_mm: 6.0,
        sweep_deg: 90.0,
    };
    let spot = Point::new(
        6.0 * 36.9_f64.to_radians().cos(),
        6.0 * 36.9_f64.to_radians().sin(),
    );
    let hole = |p: Point| p.distance_to(spot) > 0.1;
    for step in 0..=18 {
        let a = (f64::from(step) * 5.0).to_radians();
        assert!(
            hole(Point::new(6.0 * a.cos(), 6.0 * a.sin())),
            "every 5° vertex is clear, so only sampling between them sees the spot"
        );
    }
    assert!(!lead_fits(arc.to, &arc, &Pred(hole)), "the spot is caught");
    assert!(lead_fits(arc.to, &arc, &Pred(|_| true)), "a clear arc fits");

    let ring = square_ring(20.0, 80.0, 6);
    assert!(plan_lead_in(&ring, &lead_of(D), D, &Pred(|_| false), None).is_none());
}

/// Port: upstream `testRotateRingForLead`, and DreamCarve's arcs. The
/// re-seamed contour starts at the join, drops no vertex, and has the
/// same length: re-seaming moves where the cut starts, never what it
/// cuts. Re-seaming mid-segment inserts one vertex, mid-arc too.
#[test]
fn rotate_ring_for_lead_preserves_perimeter() {
    let ring = square_ring(20.0, 80.0, 4);
    let domain = open_domain();
    let plan = plan_lead_in(
        &ring,
        &lead_of(D),
        D,
        &DomainSpace::new(&domain, D, 0.0),
        None,
    )
    .unwrap();
    assert!(start_of(&plan.seam).distance_to(plan.arrival()) < 1e-9);
    assert!(plan.seam.vertices().len() >= ring.vertices().len());
    assert_approx_eq!(plan.seam.length_mm(), ring.length_mm(), 1e-6);

    assert_eq!(reseam(&ring, 0.0), ring, "at the start nothing moves");
    let mid = reseam(&ring, 5.0);
    assert_eq!(mid.vertices().len(), ring.vertices().len() + 1);
    assert!(start_of(&mid).distance_to(Point::new(25.0, 20.0)) < 1e-9);
    assert_approx_eq!(mid.length_mm(), ring.length_mm(), 1e-9);

    let circle = Polyline::circle(Point::new(50.0, 50.0), 20.0);
    let quarter = circle.length_mm() / 4.0;
    let turned = reseam(&circle, quarter);
    assert_eq!(turned.vertices().len(), 3);
    assert!(start_of(&turned).distance_to(Point::new(50.0, 70.0)) < 1e-9);
    assert_approx_eq!(turned.length_mm(), circle.length_mm(), 1e-9);
    assert!(turned.is_closed());
}

/// Port: upstream `testDeterminismAndValidatedPlacement`. The same input
/// gives the same plan; with everything below y = 45 forbidden the search
/// walks past the positions that cannot take a lead and every sample of
/// the one it takes is where the space allows, deterministically too.
#[test]
fn determinism() {
    let ring = square_ring(20.0, 80.0, 6);
    let domain = open_domain();
    let space = DomainSpace::new(&domain, D, 0.0);
    let first = plan_lead_in(&ring, &lead_of(D), D, &space, None);
    let second = plan_lead_in(&ring, &lead_of(D), D, &space, None);
    assert!(first.is_some());
    assert_eq!(first, second);

    let half_open = Pred(|p: Point| p.x > 0.0 && p.x < 100.0 && p.y >= 45.0);
    let walked = plan_lead_in(&ring, &lead_of(D), D, &half_open, None).unwrap();
    assert!(walked.arrival().y >= 45.0);
    assert!(lead_samples(&walked.arc).iter().all(|p| p.y >= 45.0));
    assert_eq!(
        plan_lead_in(&ring, &lead_of(D), D, &half_open, None),
        Some(walked)
    );
}

// ── Generator fixtures ───────────────────────────────────────────────

/// The coaster's 6.35 mm bit.
const TOOL_MM: f64 = 6.35;
const R_MM: f64 = TOOL_MM / 2.0;
const FEED: f64 = 1000.0;
const PLUNGE: f64 = 300.0;
const THICKNESS_MM: f64 = 6.0;
/// The project's flatten tolerance (`sample_project`'s).
const TOLERANCE_MM: f64 = dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
/// 2 mm steps through the 6 mm stock.
const PASS_Z_MM: [f64; 3] = [4.0, 2.0, 0.0];

fn rect(x: f64, y: f64, w: f64, h: f64) -> Polyline {
    Polyline::rect(Point::new(x, y), w, h)
}

/// A 120 × 100 × 6 mm project with one instance per shape, each its own
/// definition placed where it is drawn, and a 6.35 mm end mill.
fn project_of(shapes: &[(&str, Vec<Polyline>)]) -> Project {
    let mut p = sample_project();
    p.stock = Stock {
        width_mm: 120.0,
        height_mm: 100.0,
        thickness_mm: Some(THICKNESS_MM),
    };
    p.tools = vec![Tool {
        id: ToolId::new("t1"),
        name: "t1".into(),
        vendor: None,
        product_id: None,
        number: None,
        cutter: ToolKind::EndMill,
        diameter_mm: TOOL_MM,
        flutes: 2,
        feed_mm_min: Some(FEED),
        plunge_mm_min: Some(PLUNGE),
        rpm: Some(18000),
        step_down_mm: Some(2.0),
        step_over_fraction: Some(0.4),
    }];
    p.definitions = BTreeMap::new();
    p.instances = Vec::new();
    for (id, profiles) in shapes {
        let def = DefinitionId::new(format!("d{id}"));
        p.definitions.insert(
            def.clone(),
            FeatureDefinition {
                id: def.clone(),
                name: (*id).to_owned(),
                profiles: profiles.clone(),
            },
        );
        p.instances.push(FeatureInstance {
            id: InstanceId::new(*id),
            definition: def,
            transform: Transform::IDENTITY,
            name: (*id).to_owned(),
            visible: true,
            locked: false,
            z_top_mm: Some(THICKNESS_MM),
            z_bottom_mm: Some(0.0),
        });
    }
    p.operations.clear();
    p
}

/// A climb Profile of every instance of `p` on `side`, 2 mm steps through
/// the stock, plunge entry, no tabs, with `lead`.
fn profile(p: &Project, side: Side, lead: Option<Lead>) -> ProfileOp {
    ProfileOp {
        id: OperationId::new("o1"),
        name: "Profile".into(),
        enabled: true,
        targets: p.instances.iter().map(|i| i.id.clone()).collect(),
        tool: ToolId::new("t1"),
        side,
        depth_mm: Some(THICKNESS_MM),
        step_down_mm: None,
        direction: CutDirection::Climb,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
        tabs: None,
        machining_order: MachiningOrder::FullDepthFirst,
        entry: Entry::Plunge,
        lead,
        corner_relief: CornerRelief::None,
        last_pass: None,
        start: dc_project::StartAt::Auto,
    }
}

fn inside(p: &Project, lead: Option<Lead>) -> ProfileOp {
    profile(p, Side::Inside, lead)
}

fn generate(p: &Project, o: &ProfileOp) -> (Toolpath, Vec<Warning>) {
    let (tp, warnings) = generate_profile(p, o).unwrap();
    assert_safe(&tp, p, &Operation::Profile(o.clone()));
    (tp, warnings)
}

fn codes(warnings: &[Warning], code: WarningCode) -> Vec<&Warning> {
    warnings.iter().filter(|w| w.code() == code).collect()
}

/// The reason of a `lead_dropped` warning, and the instance it names.
fn dropped_of(warning: &Warning) -> (LeadDroppedReason, Option<&str>) {
    let Warning::LeadDropped { shape, reason, .. } = warning else {
        panic!("{warning:?}");
    };
    (*reason, shape.as_ref().map(|s| s.instance.as_str()))
}

/// The instance a `lead_shrunk` warning names, and its radii and sweeps
/// asked for and cut.
fn shrunk_of(warning: &Warning) -> (&str, f64, f64, f64, f64) {
    let Warning::LeadShrunk {
        shape,
        requested_mm,
        used_mm,
        requested_sweep_deg,
        used_sweep_deg,
        ..
    } = warning
    else {
        panic!("{warning:?}");
    };
    (
        shape.instance.as_str(),
        *requested_mm,
        *used_mm,
        *requested_sweep_deg,
        *used_sweep_deg,
    )
}

/// Every arc move with the point it starts from.
fn arcs(tp: &Toolpath) -> Vec<(Point3, Move)> {
    tp.moves
        .windows(2)
        .filter(|w| matches!(w[1].kind, MoveKind::ArcCw { .. } | MoveKind::ArcCcw { .. }))
        .map(|w| (w[0].to, w[1]))
        .collect()
}

/// The centre of an arc move.
fn centre_of(mv: &Move) -> Point {
    match mv.kind {
        MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => centre,
        _ => panic!("not an arc: {mv:?}"),
    }
}

/// The arc move from `from` as a [`LeadArc`].
fn as_lead_arc(from: Point3, mv: &Move) -> LeadArc {
    let centre = centre_of(mv);
    let ccw = matches!(mv.kind, MoveKind::ArcCcw { .. });
    let geometry = dc_geometry::Arc::from_ends(from.xy(), mv.to.xy(), centre, ccw);
    LeadArc {
        from: from.xy(),
        to: mv.to.xy(),
        centre,
        ccw,
        radius_mm: geometry.radius_mm,
        sweep_deg: geometry.sweep_rad.abs().to_degrees(),
    }
}

/// Distance from `p` to the boundary of the axis-aligned rectangle
/// `min`..`max`, whichever side `p` is on.
fn to_rect_boundary(p: Point, min: Point, max: Point) -> f64 {
    let inside = p.x >= min.x && p.x <= max.x && p.y >= min.y && p.y <= max.y;
    if inside {
        (p.x - min.x)
            .min(max.x - p.x)
            .min(p.y - min.y)
            .min(max.y - p.y)
    } else {
        let dx = (min.x - p.x).max(p.x - max.x).max(0.0);
        let dy = (min.y - p.y).max(p.y - max.y).max(0.0);
        dx.hypot(dy)
    }
}

/// The 60 × 40 hole at (20, 20) of acceptance 28, and its tool-centre
/// rectangle for the 6.35 mm bit.
fn hole_60x40() -> (Project, Point, Point) {
    let p = project_of(&[("hole", vec![rect(20.0, 20.0, 60.0, 40.0)])]);
    (
        p,
        Point::new(20.0 + R_MM, 20.0 + R_MM),
        Point::new(80.0 - R_MM, 60.0 - R_MM),
    )
}

fn corners_of(min: Point, max: Point) -> [Point; 4] {
    [min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)]
}

/// The lead-in arcs (ending on the tool-centre rectangle) and lead-out
/// arcs (starting on it), each with its index in `tp`.
fn leads_on_rect(tp: &Toolpath, min: Point, max: Point) -> (Vec<usize>, Vec<usize>) {
    let on = |p: Point| to_rect_boundary(p, min, max) < 1e-6;
    let mut ins = Vec::new();
    let mut outs = Vec::new();
    for i in 1..tp.moves.len() {
        let mv = &tp.moves[i];
        if !matches!(mv.kind, MoveKind::ArcCw { .. } | MoveKind::ArcCcw { .. }) {
            continue;
        }
        let from = tp.moves[i - 1].to.xy();
        match (on(from), on(mv.to.xy())) {
            (false, true) => ins.push(i),
            (true, false) => outs.push(i),
            _ => panic!("arc {i} is neither a lead-in nor a lead-out: {mv:?}"),
        }
    }
    (ins, outs)
}

// ── Generator tests ──────────────────────────────────────────────────

/// Acceptance 28: an inside Profile of a 60 × 40 rectangle with a 3 mm
/// lead, 2 mm overcut. Each pass reaches its depth (a plunge, here) at the
/// staging point, 3 mm from the tool-centre path (R·(1 − cos 90°)); the
/// tool arrives on the wall along an arc tangent to it, more than one
/// diameter from every corner; after the lap it overcuts 2 mm along the
/// wall, then leaves along an arc tangent to it. With a ramp or a helix
/// entry the descent ends at the staging point too.
#[test]
fn inside_rectangle_lead_arrives_tangent_away_from_corners() {
    let (p, min, max) = hole_60x40();
    let lead = Lead {
        overcut_mm: 2.0,
        ..lead_of(3.0)
    };
    let helix = Entry::Helix {
        diameter_fraction: 0.8,
        angle_deg: 5.0,
    };
    for entry in [Entry::Plunge, Entry::Ramp { angle_deg: 15.0 }, helix] {
        let o = ProfileOp {
            entry: entry.clone(),
            ..inside(&p, Some(lead.clone()))
        };
        let (tp, warnings) = generate(&p, &o);
        assert!(
            codes(&warnings, WarningCode::LeadShrunk).is_empty(),
            "{warnings:?}"
        );
        assert!(
            codes(&warnings, WarningCode::LeadDropped).is_empty(),
            "{warnings:?}"
        );
        let (ins, outs) = leads_on_rect(&tp, min, max);
        assert_eq!(
            ins.len(),
            PASS_Z_MM.len(),
            "{entry:?}: one lead-in per pass"
        );
        assert_eq!(
            outs.len(),
            PASS_Z_MM.len(),
            "{entry:?}: one lead-out per pass"
        );
        for (k, (&i, &j)) in ins.iter().zip(&outs).enumerate() {
            let z = PASS_Z_MM[k];
            let staging = tp.moves[i - 1].to;
            assert_approx_eq!(staging.z, z, EPS_MM);
            assert!(
                tp.moves[i - 2].to.z > z + EPS_MM
                    || tp.moves[i - 2].to.xy().distance_to(staging.xy()) > EPS_MM,
                "{entry:?}: the descent ends at the staging point"
            );
            assert!(
                to_rect_boundary(staging.xy(), min, max) >= 3.0 - 1e-6,
                "{entry:?}: pass {z} reaches depth {} mm from the wall's tool path",
                to_rect_boundary(staging.xy(), min, max)
            );
            if entry == Entry::Plunge {
                let before = tp.moves[i - 2].to;
                assert!(before.xy().distance_to(staging.xy()) <= EPS_MM && before.z > z);
            }
            // Tangent arrival.
            let lead_in = as_lead_arc(staging, &tp.moves[i]);
            assert_approx_eq!(lead_in.radius_mm, 3.0, 1e-9);
            let arrival = lead_in.to;
            let first_cut = unit(arrival, tp.moves[i + 1].to.xy());
            assert!(angle_between(lead_in.tangent_at(arrival), first_cut) < 1e-9);
            for c in corners_of(min, max) {
                assert!(arrival.distance_to(c) > TOOL_MM, "{arrival:?} near {c:?}");
            }
            // The lap returns to the arrival, then overcuts 2 mm.
            let lap_end = tp.moves[j - 2].to;
            assert!(lap_end.xy().distance_to(arrival) < 1e-9, "the lap closes");
            let overcut = tp.moves[j - 1];
            assert_eq!(overcut.kind, MoveKind::Linear);
            assert_approx_eq!(overcut.to.xy().distance_to(arrival), 2.0, 1e-9);
            assert!(to_rect_boundary(overcut.to.xy(), min, max) < 1e-9);
            // Tangent departure.
            let lead_out = as_lead_arc(overcut.to, &tp.moves[j]);
            let along = unit(arrival, overcut.to.xy());
            assert!(angle_between(lead_out.tangent_at(lead_out.from), along) < 1e-9);
            assert!(to_rect_boundary(lead_out.to, min, max) >= 3.0 - 1e-6);
        }
    }
}

/// Acceptance 28: with a 30 mm radius the lead does not fit the 60 × 40
/// hole as asked, so it is cut smaller and `lead_shrunk` says so.
#[test]
fn thirty_mm_radius_is_shrunk() {
    let (p, min, max) = hole_60x40();
    let (tp, warnings) = generate(&p, &inside(&p, Some(lead_of(30.0))));
    let shrunk = codes(&warnings, WarningCode::LeadShrunk);
    assert_eq!(shrunk.len(), 1, "{warnings:?}");
    let (instance, requested_mm, ..) = shrunk_of(shrunk[0]);
    assert_approx_eq!(requested_mm, 30.0, EPS_MM);
    assert_eq!(instance, "hole");
    let (ins, _) = leads_on_rect(&tp, min, max);
    assert_eq!(ins.len(), PASS_Z_MM.len(), "a lead is still cut");
}

/// Acceptance 28: in a 7 mm hole a 6.35 mm tool leaves no room for any
/// rung of the ladder, so the lead is dropped with `lead_dropped`; the
/// only arcs are the lap's, about the hole's centre.
#[test]
fn seven_mm_hole_drops_the_lead() {
    let centre = Point::new(60.0, 50.0);
    let p = project_of(&[("hole", vec![Polyline::circle(centre, 3.5)])]);
    let (tp, warnings) = generate(&p, &inside(&p, Some(lead_of(3.0))));
    let dropped = codes(&warnings, WarningCode::LeadDropped);
    assert_eq!(dropped.len(), 1, "{warnings:?}");
    assert_eq!(dropped_of(dropped[0]).0, LeadDroppedReason::NoRoom);
    for (_, mv) in arcs(&tp) {
        assert!(centre_of(&mv).distance_to(centre) < 1e-6, "{mv:?}");
    }
}

/// Validation rejects a lead radius of 0 or less, a sweep outside (0,
/// 180] and a negative overcut; the generator refuses them too, naming
/// the field, rather than cut something validation did not pass.
#[test]
fn invalid_lead_values_are_refused() {
    let (p, _, _) = hole_60x40();
    for (lead, field) in [
        (lead_of(0.0), "radius_mm"),
        (
            Lead {
                sweep_deg: 181.0,
                ..lead_of(3.0)
            },
            "sweep_deg",
        ),
        (
            Lead {
                overcut_mm: -1.0,
                ..lead_of(3.0)
            },
            "overcut_mm",
        ),
    ] {
        let err = generate_profile(&p, &inside(&p, Some(lead))).unwrap_err();
        assert!(
            matches!(err, CamError::InvalidLead { field: f, .. } if f == field),
            "{err:?}"
        );
    }
}

/// Decision (slice 11): an On line Profile has no waste side, so a lead
/// set on it is not cut and `lead_dropped` (`on_line`) says so, once for
/// the operation.
#[test]
fn on_line_drops_the_lead_and_warns() {
    let p = project_of(&[
        ("a", vec![rect(20.0, 20.0, 30.0, 20.0)]),
        ("b", vec![rect(70.0, 20.0, 30.0, 20.0)]),
    ]);
    let (tp, warnings) = generate(&p, &profile(&p, Side::OnLine, Some(lead_of(3.0))));
    let dropped = codes(&warnings, WarningCode::LeadDropped);
    assert_eq!(dropped.len(), 1, "{warnings:?}");
    assert_eq!(dropped_of(dropped[0]), (LeadDroppedReason::OnLine, None));
    assert_eq!(dropped[0].op().map(OperationId::as_str), Some("o1"));
    assert!(arcs(&tp).is_empty());
}

/// Design spec, lead step 4: the exit departs `overcut_mm` along the
/// contour past the arrival, where the lap began and ended.
#[test]
fn overcut_continues_past_start_before_exit() {
    let (p, min, max) = hole_60x40();
    let lead = Lead {
        overcut_mm: 5.0,
        ..lead_of(3.0)
    };
    let (tp, _) = generate(&p, &inside(&p, Some(lead)));
    let (ins, outs) = leads_on_rect(&tp, min, max);
    assert_eq!(ins.len(), PASS_Z_MM.len());
    assert_eq!(outs.len(), PASS_Z_MM.len());
    for (&i, &j) in ins.iter().zip(&outs) {
        let arrival = tp.moves[i].to.xy();
        let departure = tp.moves[j - 1].to.xy();
        assert_approx_eq!(departure.distance_to(arrival), 5.0, 1e-9);
        assert!(to_rect_boundary(departure, min, max) < 1e-9);
        assert!(tp.moves[j - 2].to.xy().distance_to(arrival) < 1e-9);
    }
}

/// Capability 17: with `lead_out` off there is no exit arc, and each pass
/// ends on the contour (at the arrival, with no overcut) before the tool
/// leaves it.
#[test]
fn lead_out_false_ends_on_contour() {
    let (p, min, max) = hole_60x40();
    let lead = Lead {
        lead_out: false,
        ..lead_of(3.0)
    };
    let (tp, warnings) = generate(&p, &inside(&p, Some(lead)));
    assert!(
        codes(&warnings, WarningCode::LeadDropped).is_empty(),
        "{warnings:?}"
    );
    let (ins, outs) = leads_on_rect(&tp, min, max);
    assert_eq!(ins.len(), PASS_Z_MM.len());
    assert!(outs.is_empty(), "no lead-out");
    for &i in &ins {
        let arrival = tp.moves[i].to.xy();
        let leaves = (i + 1..tp.moves.len())
            .find(|&k| tp.moves[k].is_rapid())
            .unwrap();
        let last = tp.moves[leaves - 1].to.xy();
        assert!(
            last.distance_to(arrival) < 1e-9,
            "ends at the arrival: {last:?}"
        );
    }
}

/// Design spec, lead step 5: every lead move is an arc at the cutting
/// feed (not plunge-scaled: the descent is over) in the plane of its
/// pass, and the descent ends where the lead-in starts.
#[test]
fn lead_runs_at_feed_rate_at_pass_z() {
    let (p, min, max) = hole_60x40();
    let o = ProfileOp {
        entry: Entry::Ramp { angle_deg: 15.0 },
        ..inside(&p, Some(lead_of(3.0)))
    };
    let (tp, _) = generate(&p, &o);
    let (ins, outs) = leads_on_rect(&tp, min, max);
    assert!(!ins.is_empty() && !outs.is_empty());
    for (from, mv) in arcs(&tp) {
        assert_eq!(mv.feed_mm_min, Some(FEED));
        assert_approx_eq!(mv.to.z, from.z, 1e-12);
        assert!(PASS_Z_MM.iter().any(|z| (mv.to.z - z).abs() < 1e-12));
    }
    for &i in &ins {
        let descent_end = tp.moves[i - 1];
        assert!(
            descent_end.is_cut(),
            "a descent move ends at the staging point"
        );
        assert_approx_eq!(descent_end.to.z, tp.moves[i].to.z, 1e-12);
    }
}

/// `lead_shrunk` names the radius asked for and the one cut: in a 16 mm
/// square hole a 10 mm lead does not fit at any sweep, so the ladder
/// steps down the radius.
#[test]
fn lead_shrunk_warns_with_both_radii() {
    let p = project_of(&[("hole", vec![rect(40.0, 40.0, 16.0, 16.0)])]);
    let (_, warnings) = generate(&p, &inside(&p, Some(lead_of(10.0))));
    let shrunk = codes(&warnings, WarningCode::LeadShrunk);
    assert_eq!(shrunk.len(), 1, "{warnings:?}");
    let (_, requested_mm, used, requested_sweep_deg, used_sweep_deg) = shrunk_of(shrunk[0]);
    assert_approx_eq!(requested_mm, 10.0, EPS_MM);
    assert!(
        [7.5, 5.0, 2.5].iter().any(|r| (used - r).abs() < 1e-9),
        "a rung of the ladder: {used}"
    );
    assert!(requested_sweep_deg.is_finite() && used_sweep_deg.is_finite());
}

/// Samples of an arc move, every 1°.
fn arc_samples(from: Point3, mv: &Move) -> Vec<Point> {
    let arc = as_lead_arc(from, mv);
    let sign = if arc.ccw { 1.0 } else { -1.0 };
    let start = (arc.from - arc.centre).y.atan2((arc.from - arc.centre).x);
    let steps = arc.sweep_deg.ceil().max(1.0);
    (0..=steps as u32)
        .map(|k| {
            let a = start + sign * (arc.sweep_deg * f64::from(k) / steps).to_radians();
            arc.centre + Vec2::new(a.cos(), a.sin()) * arc.radius_mm
        })
        .collect()
}

/// Port: upstream `testOutsideDomainKeepsTheArcOffThePart`, with a
/// neighbour and the stock edge. Two 20 mm squares Outside, 6 mm apart,
/// the first 8 mm from the stock's left edge, a 6 mm lead: there is no
/// room between the parts or beside the stock edge, and every lead sample
/// keeps the cutter on the stock and off its own part, and keeps the tool
/// centre out of the neighbour's kerf (2r from it, less the flatten
/// tolerance twice: once for the kerf's arcs, once for the slack a point
/// joining a curved wall is allowed). The leads are the 6 mm arcs; the
/// others are the laps' corners.
#[test]
fn lead_stays_off_neighbouring_shapes_and_stock_edge() {
    let a = (8.0, 30.0);
    let b = (34.0, 30.0);
    let p = project_of(&[
        ("a", vec![rect(a.0, a.1, 20.0, 20.0)]),
        ("b", vec![rect(b.0, b.1, 20.0, 20.0)]),
    ]);
    let (tp, _) = generate(&p, &profile(&p, Side::Outside, Some(lead_of(6.0))));
    let leads: Vec<(Point3, Move)> = arcs(&tp)
        .into_iter()
        .filter(|(from, mv)| (as_lead_arc(*from, mv).radius_mm - 6.0).abs() < 1e-6)
        .collect();
    assert_eq!(leads.len(), 2 * 2 * PASS_Z_MM.len(), "leads are cut");
    let part = |(x, y): (f64, f64)| (Point::new(x, y), Point::new(x + 20.0, y + 20.0));
    for (from, mv) in leads {
        let samples = arc_samples(from, &mv);
        // Which part this lead belongs to: the one it touches.
        let (own, other) = {
            let (amin, amax) = part(a);
            let near_a = samples
                .iter()
                .map(|s| to_rect_boundary(*s, amin, amax))
                .fold(f64::MAX, f64::min);
            if (near_a - R_MM).abs() < 1e-6 {
                (a, b)
            } else {
                (b, a)
            }
        };
        let (omin, omax) = part(own);
        let (nmin, nmax) = part(other);
        for s in samples {
            assert!(
                s.x >= R_MM - 1e-6 && s.y >= R_MM - 1e-6,
                "{s:?} off the stock"
            );
            assert!(s.x <= 120.0 - R_MM + 1e-6 && s.y <= 100.0 - R_MM + 1e-6);
            assert!(
                to_rect_boundary(s, omin, omax) >= R_MM - 1e-6,
                "{s:?} in its part"
            );
            assert!(
                to_rect_boundary(s, nmin, nmax) >= 2.0 * R_MM - 2.0 * TOLERANCE_MM - 1e-6,
                "{s:?} in the neighbour's kerf"
            );
        }
    }
}

/// The XY points where the tool rises onto a tab top in `tp`.
fn tab_lifts(tp: &Toolpath, top_z_mm: f64) -> Vec<Point> {
    let mut out: Vec<Point> = tp
        .moves
        .windows(2)
        .filter(|w| {
            w[1].is_cut() && w[0].to.z < top_z_mm - EPS_MM && (w[1].to.z - top_z_mm).abs() < EPS_MM
        })
        .map(|w| w[1].to.xy())
        .collect();
    out.sort_by(|p, q| p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)));
    out.dedup_by(|p, q| p.distance_to(*q) < 1e-6);
    out
}

/// Design spec, lead step 5: tabs apply to the contour only. With rect
/// tabs 3 mm high, the lap rises over them at the same places as without
/// a lead (the tabs stay where they are along the outline, though the
/// seam moved), and every lead arc stays at its pass Z.
#[test]
fn tabs_apply_to_contour_not_lead() {
    let (p, min, max) = hole_60x40();
    let tabs = TabSpec {
        placement: TabPlacement::Count { count: 2 },
        width_mm: 6.0,
        height_mm: 3.0,
        shape: TabShape::Rect,
        on_holes: false,
        avoid_corners: false,
    };
    let base = ProfileOp {
        tabs: Some(tabs),
        ..inside(&p, None)
    };
    let with_lead = ProfileOp {
        lead: Some(lead_of(3.0)),
        ..base.clone()
    };
    let (plain, _) = generate(&p, &base);
    let (led, warnings) = generate(&p, &with_lead);
    assert!(
        codes(&warnings, WarningCode::LeadDropped).is_empty(),
        "{warnings:?}"
    );
    let lifts = tab_lifts(&plain, 3.0);
    assert!(!lifts.is_empty(), "the tabs are cut");
    assert_eq!(tab_lifts(&led, 3.0).len(), lifts.len());
    for (a, b) in lifts.iter().zip(tab_lifts(&led, 3.0)) {
        assert!(a.distance_to(b) < 1e-6, "{a:?} moved to {b:?}");
    }
    let (ins, outs) = leads_on_rect(&led, min, max);
    assert_eq!(ins.len(), PASS_Z_MM.len());
    assert_eq!(outs.len(), PASS_Z_MM.len());
    for (from, mv) in arcs(&led) {
        assert!(PASS_Z_MM.iter().any(|z| (mv.to.z - z).abs() < 1e-12));
        assert_approx_eq!(from.z, mv.to.z, 1e-12);
    }
}
