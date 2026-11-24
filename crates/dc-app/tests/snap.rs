// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/canvas/snappingHelpers.test.ts

//! Slice 4 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan → Tool
//! model and snapping"): snapping (functional capabilities 82 and 82a,
//! acceptance 113). The first four tests are PureCutCNC's
//! `snappingHelpers.test.ts`, under its names; the rest are DreamCarve's.
//!
//! Upstream's view is one pixel to the unit; these keep it, so a radius
//! in pixels is a radius in millimetres. Upstream draws Y-down; no case
//! depends on direction.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use std::collections::BTreeMap;

use dc_app::snap::{self, SnapKind, SnapSettings, Snapped, Targets};
use dc_app::test_support::Harness;
use dc_app::{Pointer, Scene};
use dc_geometry::{Point, Polyline, Transform, Vertex};
use dc_project::test_support::sample_project;
use dc_project::{
    DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Project, Shape, Stock, Units,
};

fn pt(x: f64, y: f64) -> Point {
    Point::new(x, y)
}

/// `lineProfile(from, to)`.
fn line(from: Point, to: Point) -> Polyline {
    Polyline::new(
        vec![Vertex::line(from.x, from.y), Vertex::line(to.x, to.y)],
        false,
    )
}

/// One shape of a test project.
struct Drawn {
    id: &'static str,
    profile: Polyline,
    shape: Option<Shape>,
    transform: Transform,
    visible: bool,
    construction: bool,
}

fn drawn(id: &'static str, profile: Polyline) -> Drawn {
    Drawn {
        id,
        profile,
        shape: None,
        transform: Transform::IDENTITY,
        visible: true,
        construction: false,
    }
}

/// `makeProject(features)`: the shapes on the sample's 100 × 100 mm
/// stock, with nothing else in the project.
fn project(shapes: Vec<Drawn>) -> Project {
    let mut project = sample_project();
    project.operations.clear();
    project.tabs.clear();
    project.definitions = BTreeMap::new();
    project.instances = Vec::new();
    for shape in shapes {
        let definition = DefinitionId::new(shape.id);
        project.definitions.insert(
            definition.clone(),
            FeatureDefinition {
                id: definition.clone(),
                name: shape.id.to_owned(),
                shape: shape.shape,
                profiles: vec![shape.profile],
            },
        );
        project.instances.push(FeatureInstance {
            id: InstanceId::new(shape.id),
            definition,
            transform: shape.transform,
            name: shape.id.to_owned(),
            visible: shape.visible,
            locked: false,
            construction: shape.construction,
            group: None,
            z_top_mm: None,
            z_bottom_mm: None,
        });
    }
    project
}

fn scene_of(project: Project) -> Scene {
    Harness::from_project(project).app.scene().clone()
}

/// `snapSettings(modes, pixelRadius = 6)`.
fn only(kinds: &[SnapKind], radius_px: f64) -> SnapSettings {
    SnapSettings {
        enabled: true,
        kinds: kinds.iter().copied().collect(),
        radius_px,
    }
}

const STOCK: Stock = Stock {
    width_mm: 100.0,
    height_mm: 100.0,
    thickness_mm: None,
};

/// The snap of a pointer at `at` with nothing held, at one pixel to the
/// millimetre, no grid.
fn snap_of(scene: &Scene, settings: &SnapSettings, at: Point) -> Snapped {
    snap_with(scene, settings, &Pointer::at(at), None, None, &[])
}

fn snap_with(
    scene: &Scene,
    settings: &SnapSettings,
    pointer: &Pointer,
    reference: Option<Point>,
    grid_step_mm: Option<f64>,
    skip: &[InstanceId],
) -> Snapped {
    let targets = Targets {
        scene,
        stock: &STOCK,
        origin: pt(0.0, 0.0),
        px_per_mm: 1.0,
        grid_step_mm,
        skip,
    };
    snap::resolve(&targets, settings, pointer, reference)
}

#[track_caller]
fn assert_at(snapped: &Snapped, kind: Option<SnapKind>, x: f64, y: f64) {
    assert_eq!(snapped.kind, kind, "{snapped:?}");
    assert!(
        snapped.at_mm.distance_to(pt(x, y)) <= 1e-6,
        "expected ({x}, {y}), got {:?}",
        snapped.at_mm
    );
}

/// Two crossing lines, away from the stock's edges.
fn cross() -> Scene {
    scene_of(project(vec![
        drawn("horizontal", line(pt(20.0, 30.0), pt(40.0, 30.0))),
        drawn("vertical", line(pt(30.0, 20.0), pt(30.0, 40.0))),
    ]))
}

/// `testLineLineIntersectionSnap`: near where two lines cross, with only
/// Intersection on, the point is the crossing. (Upstream also returns
/// which segments crossed, for its constraints; phase 5 has none.)
#[test]
fn line_line_intersection_snap() {
    let snapped = snap_of(
        &cross(),
        &only(&[SnapKind::Intersection], 6.0),
        pt(30.8, 29.4),
    );
    assert_at(&snapped, Some(SnapKind::Intersection), 30.0, 30.0);
    assert_eq!(snapped.guide, None);
}

/// `testIntersectionSnapRespectsPixelRadius`: outside the radius nothing
/// snaps and the point is the pointer's.
#[test]
fn intersection_snap_respects_pixel_radius() {
    let snapped = snap_of(
        &cross(),
        &only(&[SnapKind::Intersection], 3.0),
        pt(34.0, 34.0),
    );
    assert_at(&snapped, None, 34.0, 34.0);
}

/// `testPointSnapKeepsPriorityAtSharedEndpoint`: two lines that share an
/// end meet at an endpoint, not at an intersection.
#[test]
fn point_snap_keeps_priority_at_shared_endpoint() {
    let scene = scene_of(project(vec![
        drawn("horizontal", line(pt(20.0, 20.0), pt(40.0, 20.0))),
        drawn("vertical", line(pt(20.0, 20.0), pt(20.0, 40.0))),
    ]));
    let settings = only(&[SnapKind::Endpoint, SnapKind::Intersection], 6.0);
    let snapped = snap_of(&scene, &settings, pt(20.2, 20.2));
    assert_at(&snapped, Some(SnapKind::Endpoint), 20.0, 20.0);
}

/// `testStockIntersectionSnapHasNoLiveConstraintMetadata`: a line that
/// crosses the stock's edge snaps where it crosses.
#[test]
fn stock_intersection_snap() {
    let scene = scene_of(project(vec![drawn(
        "vertical",
        line(pt(10.0, -10.0), pt(10.0, 20.0)),
    )]));
    let snapped = snap_of(&scene, &only(&[SnapKind::Intersection], 6.0), pt(10.2, 0.2));
    assert_at(&snapped, Some(SnapKind::Intersection), 10.0, 0.0);
}

/// `testIntersectionSnapIsNotEnabledByDefault`, turned round: upstream
/// leaves Intersection off because it compares segments pairwise; the
/// roadmap lists it among phase 5's object snaps, and only segments near
/// the pointer are compared, so here it is on. Perpendicular alone is
/// off, as upstream has it.
#[test]
fn every_kind_but_perpendicular_is_on_by_default() {
    let settings = SnapSettings::default();
    assert!(settings.enabled);
    assert_eq!(settings.radius_px.to_bits(), 14.0f64.to_bits());
    for kind in SnapKind::ALL {
        assert_eq!(
            settings.offers(kind),
            kind != SnapKind::Perpendicular,
            "{kind:?}"
        );
    }
}

/// A rectangle, a circle and a line that crosses the rectangle: the
/// fixture of acceptance 113.
fn fixture() -> Scene {
    scene_of(project(vec![
        drawn("rect", Polyline::rect(pt(20.0, 20.0), 40.0, 20.0)),
        drawn("circle", Polyline::circle(pt(80.0, 70.0), 10.0)),
        drawn("crossing", line(pt(30.0, 10.0), pt(30.0, 55.0))),
    ]))
}

/// Acceptance 113: 10 px from a rectangle's corner and 2 px from its
/// side, the point is the corner; 20 px from the corner and 2 px from
/// the side, the side's nearest point. The nearest candidate of the
/// highest rank wins, not the nearest candidate.
#[test]
fn the_nearest_of_the_highest_rank_wins() {
    let scene = fixture();
    let settings = SnapSettings::default();
    // The rectangle's top-right corner is (60, 40); its top side is y = 40.
    let near_corner = snap_of(&scene, &settings, pt(50.2, 42.0));
    assert_at(&near_corner, Some(SnapKind::Endpoint), 60.0, 40.0);
    // Far enough along the side that the corner and the midpoint (40, 40)
    // are both out of reach.
    let on_side = snap_of(&scene, &Default::default(), pt(14.0, 20.5));
    assert_eq!(
        on_side.kind,
        Some(SnapKind::Endpoint),
        "the corner (20, 20)"
    );
    let scene = scene_of(project(vec![drawn(
        "wide",
        Polyline::rect(pt(10.0, 20.0), 80.0, 20.0),
    )]));
    // (32, 42): 2 px above the top side; its corner (10, 40) is 22 px
    // away and its midpoint (50, 40) 18 px.
    let on_side = snap_of(&scene, &settings, pt(32.0, 42.0));
    assert_at(&on_side, Some(SnapKind::OnOutline), 32.0, 40.0);
    assert_eq!(on_side.guide, Some((pt(32.0, 42.0), pt(32.0, 40.0))));
    // Two endpoints in reach: the nearer.
    let between = snap_of(&scene, &only(&[SnapKind::Endpoint], 100.0), pt(55.0, 30.0));
    assert_eq!(between.kind, Some(SnapKind::Endpoint));
    assert!(between.at_mm.x > 80.0, "{between:?}");
}

/// Each kind's candidate, on the fixture (acceptance 113).
#[test]
fn each_kind_is_found_on_the_fixture() {
    let scene = fixture();
    let one = |kind: SnapKind, at: Point| snap_of(&scene, &only(&[kind], 6.0), at);
    // A corner of the rectangle, a corner of the stock, the origin.
    assert_at(
        &one(SnapKind::Endpoint, pt(21.0, 19.0)),
        Some(SnapKind::Endpoint),
        20.0,
        20.0,
    );
    assert_at(
        &one(SnapKind::Endpoint, pt(98.0, 99.0)),
        Some(SnapKind::Endpoint),
        100.0,
        100.0,
    );
    assert_at(
        &one(SnapKind::Endpoint, pt(1.0, -1.0)),
        Some(SnapKind::Endpoint),
        0.0,
        0.0,
    );
    // The middle of the rectangle's bottom side, and of the circle's
    // upper half, its top.
    assert_at(
        &one(SnapKind::Midpoint, pt(41.0, 21.0)),
        Some(SnapKind::Midpoint),
        40.0,
        20.0,
    );
    assert_at(
        &one(SnapKind::Midpoint, pt(81.0, 79.0)),
        Some(SnapKind::Midpoint),
        80.0,
        80.0,
    );
    // The circle's centre.
    assert_at(
        &one(SnapKind::Centre, pt(82.0, 71.0)),
        Some(SnapKind::Centre),
        80.0,
        70.0,
    );
    // Where the line crosses the rectangle's two long sides.
    assert_at(
        &one(SnapKind::Intersection, pt(31.0, 21.0)),
        Some(SnapKind::Intersection),
        30.0,
        20.0,
    );
    assert_at(
        &one(SnapKind::Intersection, pt(29.0, 41.0)),
        Some(SnapKind::Intersection),
        30.0,
        40.0,
    );
    // The nearest point of the circle, by the arc and not its chord.
    let on_circle = one(
        SnapKind::OnOutline,
        pt(80.0 + 12.0 * 0.6, 70.0 + 12.0 * 0.8),
    );
    assert_at(&on_circle, Some(SnapKind::OnOutline), 86.0, 78.0);
    // Nothing of a kind in reach: the pointer.
    assert_at(&one(SnapKind::Centre, pt(40.0, 30.0)), None, 40.0, 30.0);
}

/// The foot of the perpendicular from the tool's last point, with its
/// guide; none without a last point, and none where the nearest point of
/// the segment is its end.
#[test]
fn perpendicular_needs_a_reference_point() {
    let scene = fixture();
    let settings = only(&[SnapKind::Perpendicular], 6.0);
    let from = pt(45.0, 60.0);
    let pointer = Pointer::at(pt(46.0, 41.0));
    let snapped = snap_with(&scene, &settings, &pointer, Some(from), None, &[]);
    assert_at(&snapped, Some(SnapKind::Perpendicular), 45.0, 40.0);
    assert_eq!(snapped.guide, Some((from, pt(45.0, 40.0))));
    assert_eq!(snap_of(&scene, &settings, pt(46.0, 41.0)).kind, None);
    // From beyond the side's end the nearest point is the corner, which
    // is no perpendicular foot.
    let pointer = Pointer::at(pt(61.0, 41.0));
    let beyond = snap_with(&scene, &settings, &pointer, Some(pt(70.0, 60.0)), None, &[]);
    assert_eq!(beyond.kind, None);
    // Onto an arc: the point of the circle toward the last point.
    let pointer = Pointer::at(pt(80.0, 81.0));
    let onto = snap_with(&scene, &settings, &pointer, Some(pt(80.0, 95.0)), None, &[]);
    assert_at(&onto, Some(SnapKind::Perpendicular), 80.0, 80.0);
}

/// A kind turned off is not offered; the master switch turns all off; a
/// held `Ctrl` or `⌘` takes the pointer's point (acceptance 113).
#[test]
fn a_kind_turned_off_is_not_offered() {
    let scene = fixture();
    let corner = pt(21.0, 19.0);
    // The crossing line meets the bottom side 9 px away, so Intersection
    // and Midpoint stay out of this one.
    let mut settings = only(&[SnapKind::Endpoint, SnapKind::OnOutline], 14.0);
    assert_eq!(
        snap_of(&scene, &settings, corner).kind,
        Some(SnapKind::Endpoint)
    );
    settings.set(SnapKind::Endpoint, false);
    assert_eq!(
        snap_of(&scene, &settings, corner).kind,
        Some(SnapKind::OnOutline)
    );
    settings.set(SnapKind::OnOutline, false);
    assert_at(&snap_of(&scene, &settings, corner), None, 21.0, 19.0);
    settings.set(SnapKind::Endpoint, true);
    assert_eq!(
        snap_of(&scene, &settings, corner).kind,
        Some(SnapKind::Endpoint)
    );
    settings.enabled = false;
    assert_at(&snap_of(&scene, &settings, corner), None, 21.0, 19.0);

    let held = Pointer {
        command: true,
        ..Pointer::at(corner)
    };
    let snapped = snap_with(
        &scene,
        &SnapSettings::default(),
        &held,
        None,
        Some(5.0),
        &[],
    );
    assert_at(&snapped, None, 21.0, 19.0);
}

/// With nothing in reach the point is the pointer's own: a click is
/// never discarded, as upstream discards it (digest §5.4).
#[test]
fn nothing_in_reach_takes_the_pointer() {
    let scene = fixture();
    let snapped = snap_of(&scene, &SnapSettings::default(), pt(84.0, 26.0));
    assert_at(&snapped, None, 84.0, 26.0);
    assert_eq!(snapped.guide, None);
}

/// The grid has no reach test: with it on and nothing better, the point
/// is on it, however far; anything else in reach wins over it.
#[test]
fn the_grid_takes_what_nothing_else_does() {
    let scene = fixture();
    let settings = SnapSettings::default();
    let on_grid = |at: Point, step: f64| {
        snap_with(&scene, &settings, &Pointer::at(at), None, Some(step), &[])
    };
    assert_at(
        &on_grid(pt(84.0, 26.0), 5.0),
        Some(SnapKind::Grid),
        85.0,
        25.0,
    );
    assert_at(
        &on_grid(pt(84.0, 26.0), 50.0),
        Some(SnapKind::Grid),
        100.0,
        50.0,
    );
    assert_at(
        &on_grid(pt(-33.4, -48.1), 2.0),
        Some(SnapKind::Grid),
        -34.0,
        -48.0,
    );
    // A corner in reach wins, though the grid point is nearer.
    assert_at(
        &on_grid(pt(22.0, 22.5), 2.0),
        Some(SnapKind::Endpoint),
        20.0,
        20.0,
    );
    // The grid hidden, or turned off: no grid snap.
    assert_at(
        &snap_of(&scene, &settings, pt(84.0, 26.0)),
        None,
        84.0,
        26.0,
    );
    let off = only(&[SnapKind::Endpoint], 14.0);
    let snapped = snap_with(
        &scene,
        &off,
        &Pointer::at(pt(84.0, 26.0)),
        None,
        Some(5.0),
        &[],
    );
    assert_at(&snapped, None, 84.0, 26.0);
}

/// The automatic grid follows the zoom in steps of 1, 2 and 5, never
/// closer than 8 px, so no point is out of a grid point's reach; a
/// pinned spacing is the spacing (capability 82a, acceptance 113).
#[test]
fn the_grid_follows_the_zoom_or_the_pin() {
    let cases = [
        (100.0, 0.1),
        (8.0, 1.0),
        (7.9, 2.0),
        (5.0, 2.0),
        (3.9, 5.0),
        (1.6, 5.0),
        (1.0, 10.0),
        (0.1, 100.0),
    ];
    for (px_per_mm, step_mm) in cases {
        let step = snap::grid_step_mm(px_per_mm, Units::Mm, None);
        assert!((step - step_mm).abs() < 1e-9, "{px_per_mm} px/mm: {step}");
        let spacing_px = step * px_per_mm;
        assert!((8.0..20.0 + 1e-9).contains(&spacing_px), "{spacing_px} px");
    }
    // An inch project steps in hundredths, tenths and whole inches.
    let inch = snap::grid_step_mm(5.0, Units::Inch, None);
    assert!((inch - 0.1 * 25.4).abs() < 1e-9, "{inch}");
    let fine = snap::grid_step_mm(40.0, Units::Inch, None);
    assert!((fine - 0.01 * 25.4).abs() < 1e-9, "{fine}");
    // Pinned: whatever the zoom.
    for px_per_mm in [0.1, 5.0, 400.0] {
        let pinned = snap::grid_step_mm(px_per_mm, Units::Mm, Some(6.35));
        assert_eq!(pinned.to_bits(), 6.35f64.to_bits());
    }
    // A pin that is no spacing is no pin.
    assert_eq!(
        snap::grid_step_mm(5.0, Units::Mm, Some(0.0)).to_bits(),
        2.0f64.to_bits()
    );
    assert!(snap::grid_step_mm(f64::NAN, Units::Mm, None) > 0.0);
}

/// `Shift` holds the direction from the last point to 15° steps, the
/// distance kept: (30, 3) from (0, 0) is 5.7° round, so 0°, at the same
/// 30.15 mm (acceptance 113). A candidate on the ray still wins; one off
/// it does not; the grid sets the length along the ray.
#[test]
fn shift_steps_the_angle_by_fifteen_degrees() {
    let held = snap::constrain_angle(pt(0.0, 0.0), pt(30.0, 3.0));
    assert!((held.x - 30.0f64.hypot(3.0)).abs() < 1e-9, "{held:?}");
    assert_eq!(held.y.to_bits(), 0.0f64.to_bits(), "exactly on the axis");
    let diagonal = snap::constrain_angle(pt(10.0, 10.0), pt(20.0, 21.0));
    assert!((diagonal.x - diagonal.y).abs() < 1e-9, "45°: {diagonal:?}");
    let up = snap::constrain_angle(pt(5.0, 5.0), pt(5.4, -20.0));
    assert_eq!(up.x.to_bits(), 5.0f64.to_bits(), "straight down");
    assert_eq!(
        snap::constrain_angle(pt(1.0, 1.0), pt(1.0, 1.0)),
        pt(1.0, 1.0)
    );

    let scene = fixture();
    let settings = SnapSettings::default();
    let from = pt(20.0, 60.0);
    let shift = |at: Point| Pointer {
        shift: true,
        ..Pointer::at(at)
    };
    // Along the horizontal from (20, 60): the crossing line stops at
    // y = 55, so nothing is on the ray, and the point is the pointer's
    // turned onto it.
    let free = snap_with(
        &scene,
        &settings,
        &shift(pt(50.0, 62.0)),
        Some(from),
        None,
        &[],
    );
    assert_eq!(free.kind, None);
    assert_eq!(free.at_mm.y.to_bits(), 60.0f64.to_bits());
    // Straight down from (60, 60) the rectangle's corner (60, 40) is on
    // the ray and wins.
    let onto = snap_with(
        &scene,
        &settings,
        &shift(pt(61.0, 42.0)),
        Some(pt(60.0, 60.0)),
        None,
        &[],
    );
    assert_at(&onto, Some(SnapKind::Endpoint), 60.0, 40.0);
    // The same pointer from (58, 60): the corner is off the ray, 2 mm to
    // its side, and is not taken; the side it crosses is.
    let beside = snap_with(
        &scene,
        &settings,
        &shift(pt(59.0, 42.0)),
        Some(pt(58.0, 60.0)),
        None,
        &[],
    );
    assert_at(&beside, Some(SnapKind::OnOutline), 58.0, 40.0);
    // With the grid, the length along the ray is a whole step.
    let stepped = snap_with(
        &scene,
        &settings,
        &shift(pt(43.0, 61.5)),
        Some(from),
        Some(5.0),
        &[],
    );
    assert_at(&stepped, Some(SnapKind::Grid), 45.0, 60.0);
    // Without a last point `Shift` has nothing to turn about.
    let no_reference = snap_with(&scene, &settings, &shift(pt(84.0, 26.0)), None, None, &[]);
    assert_at(&no_reference, None, 84.0, 26.0);
}

/// The shape being dragged or edited is no target of its own snap; a
/// hidden shape is not in the Scene, so does not snap; construction
/// geometry does (capability 82).
#[test]
fn what_is_and_is_not_a_target() {
    let shapes = || {
        vec![
            drawn("rect", Polyline::rect(pt(20.0, 20.0), 40.0, 20.0)),
            Drawn {
                visible: false,
                ..drawn("hidden", Polyline::rect(pt(70.0, 20.0), 10.0, 10.0))
            },
            Drawn {
                construction: true,
                ..drawn("guide", line(pt(10.0, 80.0), pt(90.0, 80.0)))
            },
        ]
    };
    let scene = scene_of(project(shapes()));
    let settings = only(&[SnapKind::Endpoint], 6.0);
    assert_eq!(
        snap_of(&scene, &settings, pt(21.0, 19.0)).kind,
        Some(SnapKind::Endpoint)
    );
    let skip = [InstanceId::new("rect")];
    let skipped = snap_with(
        &scene,
        &settings,
        &Pointer::at(pt(21.0, 19.0)),
        None,
        None,
        &skip,
    );
    assert_eq!(skipped.kind, None, "its own corner");
    assert_eq!(
        snap_of(&scene, &settings, pt(71.0, 19.0)).kind,
        None,
        "hidden"
    );
    assert_at(
        &snap_of(&scene, &settings, pt(11.0, 81.0)),
        Some(SnapKind::Endpoint),
        10.0,
        80.0,
    );
}

/// A parametric shape's centre snaps, placed where the instance is; a
/// slot offers its two end centres, by its arcs, and its middle. A circle
/// that came in as straight segments has a centre too. PureCutCNC snaps
/// only to such a shape's frame.
#[test]
fn centres_of_parametric_shapes_and_flattened_circles() {
    let slot = Shape::Slot {
        length_mm: 40.0,
        width_mm: 12.0,
    };
    let placed =
        Transform::rotate(std::f64::consts::FRAC_PI_2).then(Transform::translate(50.0, 50.0));
    let polygon = dc_geometry::shapes::regular_polygon(48, 8.0);
    let scene = scene_of(project(vec![
        Drawn {
            shape: Some(slot.clone()),
            transform: placed,
            ..drawn("slot", slot.outline(0.01).unwrap().unwrap().remove(0))
        },
        Drawn {
            transform: Transform::translate(20.0, 80.0),
            ..drawn("facets", polygon)
        },
    ]));
    let settings = only(&[SnapKind::Centre], 6.0);
    // The slot stands upright at (50, 50): its end centres are 20 above
    // and below, its middle between them.
    for (at, x, y) in [
        (pt(51.0, 51.0), 50.0, 50.0),
        (pt(49.0, 71.0), 50.0, 70.0),
        (pt(51.0, 29.0), 50.0, 30.0),
        (pt(21.0, 79.0), 20.0, 80.0),
    ] {
        assert_at(
            &snap_of(&scene, &settings, at),
            Some(SnapKind::Centre),
            x,
            y,
        );
    }
}

/// Settings come back from a file held to their range: a radius of 6 to
/// 32 px (`normalizeSnapSettings`).
#[test]
fn the_radius_is_held_to_its_range() {
    for (given, held) in [(2.0, 6.0), (14.0, 14.0), (99.0, 32.0), (f64::NAN, 14.0)] {
        let settings = SnapSettings {
            radius_px: given,
            ..SnapSettings::default()
        }
        .normalized();
        assert_eq!(settings.radius_px.to_bits(), f64::to_bits(held), "{given}");
    }
    for kind in SnapKind::ALL {
        assert_eq!(SnapKind::from_key(kind.key()), Some(kind));
    }
    assert_eq!(SnapKind::from_key("tangent"), None);
}
