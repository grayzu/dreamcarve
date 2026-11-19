// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/pocket.ts

//! Tests of the ring tree: the inset of a pocket, the rings stepping in
//! from it, and where each ring starts.

use super::*;
use crate::pocket::ring_path;
use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, area_mm2, assert_approx_eq, nest};
use dc_project::CutDirection;

fn region(polylines: Vec<Polyline>) -> Region {
    let contours = polylines
        .into_iter()
        .map(|p| Contour::new(p).unwrap())
        .collect();
    nest(contours).unwrap().0.remove(0)
}

#[test]
fn inset_keeps_arcs_when_island_is_clear() {
    let r = region(vec![
        Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0),
        Polyline::circle(Point::new(50.0, 25.0), 10.0),
    ]);
    let out = inset_region(&r, 3.0, FLATTEN_TOLERANCE_DEFAULT_MM);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].holes().len(), 1);
    let hole = out[0].holes()[0].polyline();
    assert!(
        hole.segments().any(|s| !s.is_line()),
        "the island stays round"
    );
    assert_approx_eq!(
        area_mm2(&out),
        94.0 * 44.0 - std::f64::consts::PI * 13.0 * 13.0,
        1e-6
    );
}

#[test]
fn inset_splits_around_a_bar() {
    let r = region(vec![
        Polyline::rect(Point::new(0.0, 0.0), 100.0, 50.0),
        Polyline::rect(Point::new(45.0, 1.0), 10.0, 48.0),
    ]);
    let out = inset_region(&r, 3.0, FLATTEN_TOLERANCE_DEFAULT_MM);
    assert_eq!(out.len(), 2, "{out:?}");
}

/// A band thinner than twice the inset (what a rough Pocket's
/// stock to leave makes for rest machining) insets to nothing. The grown
/// island then encloses the shrunk outer, and must not be read back
/// as the outer of an inverted annulus, which grows on every inset.
#[test]
fn inset_of_a_thin_band_is_empty() {
    let r = region(vec![
        Polyline::rect(Point::new(0.0, 0.0), 40.0, 40.0),
        Polyline::rect(Point::new(1.0, 1.0), 38.0, 38.0),
    ]);
    let out = inset_region(&r, 1.2, FLATTEN_TOLERANCE_DEFAULT_MM);
    assert!(out.is_empty(), "{out:?}");
    let tree = RingTree::build(r, 1.2, 1.5, FLATTEN_TOLERANCE_DEFAULT_MM, 100).unwrap();
    assert_eq!(tree.nodes.len(), 1);
}

/// #196: a ring starts at its point nearest the anchor, on an arc too,
/// not at its nearest vertex. A circle of radius 10 about the origin
/// has vertices at (±10, 0); from (12, 1) its nearest point is at
/// θ = atan2(1, 12), and the ring's contour gains a vertex there.
#[test]
fn ring_starts_at_nearest_point_of_an_arc() {
    let r = region(vec![Polyline::circle(Point::new(0.0, 0.0), 10.0)]);
    let tree = RingTree::build(r, 50.0, 25.0, FLATTEN_TOLERANCE_DEFAULT_MM, 100).unwrap();
    let mut anchor = Some(Point::new(12.0, 1.0));
    let rings = tree.walk(WallRings::Last, &mut anchor).expect("a walk");
    assert_eq!(rings.len(), 1);
    let theta = 1.0_f64.atan2(12.0);
    let want = Point::new(10.0 * theta.cos(), 10.0 * theta.sin());
    assert!(
        rings[0].start.distance_to(want) <= EPS_MM,
        "starts at {:?}, want {want:?}",
        rings[0].start
    );
    assert_eq!(anchor, Some(rings[0].start));
    // The ring's path starts there, split into arcs of the same
    // circle: the same length and area.
    let start = rings[0].start;
    for direction in [CutDirection::Climb, CutDirection::Conventional] {
        let path = ring_path(rings[0].clone(), direction);
        let first = path.vertices().first().unwrap().point();
        assert!(first.distance_to(start) <= EPS_MM, "{path:?}");
        assert_eq!(path.vertices().len(), 3, "{path:?}");
        assert_approx_eq!(path.length_mm(), TAU * 10.0, 1e-9);
        assert_approx_eq!(
            path.signed_area_mm2().abs(),
            std::f64::consts::PI * 100.0,
            1e-9
        );
    }
}

/// Splitting a line keeps both halves straight; a point that is a
/// vertex, or on no segment, changes nothing.
#[test]
fn split_at_a_line_and_at_a_vertex() {
    let rect = Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0);
    let split = split_at(&rect, Point::new(5.0, 0.0));
    assert_eq!(split.vertices().len(), 5);
    assert!(split.segments().all(|s| s.is_line()));
    assert_approx_eq!(split.length_mm(), 60.0, 1e-9);
    assert_eq!(split_at(&rect, Point::new(20.0, 0.0)), rect);
    assert_eq!(split_at(&rect, Point::new(5.0, 5.0)), rect);
    // A straight edge offers only its corners, as upstream's polygon
    // does: from inside the rect, its nearest corner.
    let r = region(vec![rect]);
    let (p, d) = nearest(r.outer(), Some(Point::new(5.0, 2.0))).expect("a nearest point");
    assert!(p.distance_to(Point::new(0.0, 0.0)) <= EPS_MM, "{p:?}");
    assert_approx_eq!(d, 29.0_f64.sqrt(), 1e-9);
}

/// #258: a 6.35 mm tool at step-over 1.0 (`s = 6.35`, `r = 3.175`). A
/// tool-centre circle of radius 5.325 has no room for another ring, and
/// its ring reaches only 3.175 in; it gets one residual ring, the circle
/// inset by `r` and the tolerance, which reaches the centre. At radius
/// 3.0 the ring reaches the centre and none is added; nor is one at
/// step-over 0.5, whatever the radius.
#[test]
fn leaf_gets_a_residual_ring_only_when_needed() {
    let tol = FLATTEN_TOLERANCE_DEFAULT_MM;
    let circle = |radius_mm| region(vec![Polyline::circle(Point::new(0.0, 0.0), radius_mm)]);
    let tree = RingTree::build(circle(5.325), 6.35, 3.175, tol, 100).unwrap();
    assert_eq!(tree.nodes.len(), 2, "{tree:?}");
    assert_eq!(tree.nodes[0].children, [1]);
    let residual = &tree.nodes[1];
    assert!(residual.residual && residual.children.is_empty());
    assert_approx_eq!(
        residual.region.area_mm2(),
        std::f64::consts::PI * (5.325 - 3.175 - tol).powi(2),
        1e-6
    );
    // #267: it is cut from the ring, out and back along a radius.
    let rings = tree.walk(WallRings::Last, &mut None).expect("a walk");
    assert_eq!(rings.iter().map(|r| r.node).collect::<Vec<_>>(), [0]);
    let cleanups = &rings[0].cleanups;
    assert_eq!(cleanups.len(), 1, "{cleanups:?}");
    assert_eq!(cleanups[0].ring.node, 1);
    assert_approx_eq!(
        cleanups[0].at.distance_to(cleanups[0].ring.start),
        3.175 + tol,
        1e-6
    );
    // With the wall pass omitted the residual ring goes with it.
    assert_eq!(tree.walk(WallRings::Omitted, &mut None), Ok(Vec::new()));

    let tree = RingTree::build(circle(3.0), 6.35, 3.175, tol, 100).unwrap();
    assert_eq!(tree.nodes.len(), 1);
    let tree = RingTree::build(circle(5.325), 3.175, 3.175, tol, 100).unwrap();
    assert!(tree.nodes.iter().all(|n| !n.residual), "{tree:?}");
}

/// #258: at step-over 1.0 the child of a rect has its corners `2r` in
/// from each side, so a point `1.1r` from both sides of a corner is
/// more than `r` from the ring and from the child (`0.9·√2·r`). The
/// 100 × 50 pocket's tool-centre rect gets one residual ring in each
/// corner beside its child. At 0.8 the child's corner is near enough
/// (a right angle needs `s ≤ (1 + √2/2)·r`) and none is added.
#[test]
fn square_corners_get_residual_rings_at_full_step_over() {
    let tol = FLATTEN_TOLERANCE_DEFAULT_MM;
    let rect = || region(vec![Polyline::rect(Point::new(0.0, 0.0), 93.65, 43.65)]);
    let tree = RingTree::build(rect(), 6.35, 3.175, tol, 1000).unwrap();
    let root = &tree.nodes[0];
    let residual: Vec<&RingNode> = root
        .children
        .iter()
        .map(|&c| &tree.nodes[c])
        .filter(|n| n.residual)
        .collect();
    assert_eq!(root.children.len(), 5, "{root:?}");
    assert_eq!(residual.len(), 4);
    for corner in [(0.0, 0.0), (93.65, 0.0), (93.65, 43.65), (0.0, 43.65)] {
        let corner = Point::new(corner.0, corner.1);
        assert!(
            residual.iter().any(|n| {
                n.region
                    .outer()
                    .polyline()
                    .vertices()
                    .iter()
                    .all(|v| v.point().distance_to(corner) < 2.5 * 3.175)
            }),
            "no residual ring at {corner:?}"
        );
        // #267: each is cut from the root ring, out from its corner.
        assert!(
            residual.iter().any(|n| n.splice.is_some_and(|s| s.ring == 0
                && s.at.distance_to(corner) <= EPS_MM
                && (s.start.distance_to(corner) - (3.175 + tol) * 2.0_f64.sqrt()).abs() <= 1e-6)),
            "no residual ring cut from {corner:?}: {residual:?}"
        );
    }
    // Every residual ring of the tree is a clean-up of its parent's ring.
    let rings = tree.walk(WallRings::Last, &mut None).expect("a walk");
    assert!(rings.iter().all(|r| !tree.nodes[r.node].residual));
    let cleanups: usize = rings.iter().map(|r| r.cleanups.len()).sum();
    assert_eq!(cleanups, tree.nodes.iter().filter(|n| n.residual).count());
    let tree = RingTree::build(rect(), 0.8 * 6.35, 3.175, tol, 1000).unwrap();
    assert!(tree.nodes.iter().all(|n| !n.residual));
}

#[test]
fn tree_of_a_rect_is_a_chain() {
    let r = region(vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0)]);
    let tree = RingTree::build(r, 2.0, 2.5, FLATTEN_TOLERANCE_DEFAULT_MM, 100).unwrap();
    // Half-height 5: insets of 2 and 4 remain, 6 does not.
    assert_eq!(tree.nodes.len(), 3);
    let mut anchor = None;
    let rings = tree.walk(WallRings::Last, &mut anchor).expect("a walk");
    let nodes: Vec<usize> = rings.iter().map(|r| r.node).collect();
    assert_eq!(nodes, [2, 1, 0]);
    assert!(
        RingTree::build(
            region(vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0)]),
            2.0,
            2.5,
            FLATTEN_TOLERANCE_DEFAULT_MM,
            2
        )
        .is_none()
    );
}

/// #235 (found by the dc-gcode property test): the nested circular
/// pocket's tool-centre region, a circle of radius 1.4994 mm, at the
/// property case's 0.2998 mm step-over is 5.0006 step-overs across its
/// radius, so a sixth ring of radius 0.0002 mm was kept: three arcs
/// whose ends round to one point in the program (S6). A ring under
/// twice the tolerance across is noise by the rule rest machining
/// uses (area under the tolerance times the perimeter), and nothing
/// the tool does not already reach from the ring before it: five
/// rings, none under 0.02 mm across.
#[test]
fn no_ring_smaller_than_the_tolerance() {
    let tool_radius_mm = 1.715_671_296_509_893_3 / 2.0;
    let centre_radius_mm = 2.906_933_833_965_864_4 - tool_radius_mm - 0.549_697_563_087_318_1;
    let stepover_mm = 1.715_671_296_509_893_3 * 0.174_767_951_923_974_57;
    let root = region(vec![Polyline::circle(
        Point::new(120.0, 80.0),
        centre_radius_mm,
    )]);
    let tree = RingTree::build(
        root,
        stepover_mm,
        tool_radius_mm,
        FLATTEN_TOLERANCE_DEFAULT_MM,
        100,
    )
    .unwrap();
    assert_eq!(tree.nodes.len(), 5, "{:?}", tree.nodes);
    for (i, node) in tree.nodes.iter().enumerate() {
        let bounds = node.region.outer().polyline().bounds().unwrap();
        let across_mm = bounds.width_mm().min(bounds.height_mm());
        assert!(
            across_mm >= 2.0 * FLATTEN_TOLERANCE_DEFAULT_MM,
            "ring {i} is {across_mm} mm across"
        );
    }
}

/// Code review finding 1.2: a tree that names a child it lacks is
/// refused, where the walk once left that child's rings uncut.
#[test]
fn walk_refuses_a_child_the_tree_lacks() {
    let r = region(vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0)]);
    let mut tree = RingTree::build(r, 1.2, 1.5, FLATTEN_TOLERANCE_DEFAULT_MM, 100).unwrap();
    let missing = tree.nodes.len();
    tree.nodes[0].children.push(missing);
    for wall in [WallRings::Last, WallRings::First, WallRings::Omitted] {
        assert_eq!(
            tree.walk(wall, &mut None),
            Err(CamError::internal("a ring tree lacks a node it names")),
            "{wall:?}"
        );
    }
}

/// A residual node spliced onto a ring its parent lacks is refused,
/// where it was once cut neither from a lap nor as a ring of its own.
#[test]
fn walk_refuses_a_splice_onto_no_ring() {
    let r = region(vec![Polyline::rect(Point::new(0.0, 0.0), 20.0, 10.0)]);
    let at = Point::new(1.0, 1.0);
    let tree = RingTree {
        nodes: vec![
            RingNode {
                region: r.clone(),
                children: vec![1],
                residual: false,
                splice: None,
            },
            RingNode {
                region: r,
                children: Vec::new(),
                residual: true,
                // The parent has an outer and no hole: no ring 3.
                splice: Some(Splice {
                    ring: 3,
                    at,
                    start: at,
                }),
            },
        ],
    };
    assert_eq!(
        tree.walk(WallRings::Last, &mut None),
        Err(CamError::internal("a ring is of no hole of its node"))
    );
}

/// With no anchor a ring starts at its first vertex, at no distance.
#[test]
fn nearest_with_no_anchor_is_the_first_vertex() {
    let r = region(vec![Polyline::rect(Point::new(2.0, 3.0), 20.0, 10.0)]);
    let first = r.outer().polyline().vertices()[0].point();
    assert_eq!(nearest(r.outer(), None), Ok((first, 0.0)));
}
