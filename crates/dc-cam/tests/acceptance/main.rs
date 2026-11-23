// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Clauses of phase 3's acceptance tests asserted on the generators'
//! output through the crate's public functions
//! (`docs/specs/phase-3/functional.md`, "Acceptance tests";
//! `docs/specs/phase-3/audit.md` lists which clause each test holds).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod boundary;
mod clearing;
mod tapered;

use dc_cam::{Move, MoveKind, Toolpath};
use dc_geometry::{Point, Point3, Polyline, Transform};
use dc_project::test_support::sample_project;
use dc_project::{DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Project, Stock};

/// Where [`drawing_project`] places its drawing, mm.
const AT: (f64, f64) = (20.0, 20.0);

/// A 200 × 150 mm project `thickness_mm` thick with `profiles` placed at
/// [`AT`] as instance `i1`, the sample end mill `t1`, and no operations.
fn drawing_project(profiles: Vec<Polyline>, thickness_mm: f64) -> Project {
    let mut p = sample_project();
    p.stock = Stock {
        width_mm: 200.0,
        height_mm: 150.0,
        thickness_mm: Some(thickness_mm),
    };
    let def = FeatureDefinition {
        id: DefinitionId::new("d1"),
        name: "Drawing".into(),
        shape: None,
        profiles,
    };
    p.instances = vec![FeatureInstance {
        id: InstanceId::new("i1"),
        definition: def.id.clone(),
        transform: Transform::translate(AT.0, AT.1),
        name: "Drawing".into(),
        visible: true,
        locked: false,
        construction: false,
        folder: None,
        z_top_mm: Some(thickness_mm),
        z_bottom_mm: Some(0.0),
    }];
    p.definitions = std::collections::BTreeMap::from([(def.id.clone(), def)]);
    p.operations.clear();
    p
}

/// Each cutting move of `toolpath` with the point it starts from.
fn cuts(toolpath: &Toolpath) -> Vec<(Point3, Move)> {
    let mut out = Vec::new();
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        if let (Some(a), true) = (from, mv.is_cut()) {
            out.push((a, *mv));
        }
        from = Some(mv.to);
    }
    out
}

/// The angle an arc from `a` to `b` about `centre` sweeps, radians:
/// positive counter-clockwise, a full turn when the two points meet.
fn sweep(a: Point, b: Point, centre: Point, ccw: bool) -> f64 {
    let start = (a.y - centre.y).atan2(a.x - centre.x);
    let mut sweep = (b.y - centre.y).atan2(b.x - centre.x) - start;
    if ccw && sweep <= 0.0 {
        sweep += std::f64::consts::TAU;
    } else if !ccw && sweep >= 0.0 {
        sweep -= std::f64::consts::TAU;
    }
    sweep
}

/// The points the cutting moves of `toolpath` pass through, at most
/// `step_mm` apart along each move and with both its ends: straight moves
/// by length, arcs by angle, Z changing evenly along the move.
fn cut_samples(toolpath: &Toolpath, step_mm: f64) -> Vec<Point3> {
    let mut out = Vec::new();
    for (from, mv) in cuts(toolpath) {
        let (a, b) = (from.xy(), mv.to.xy());
        let arc = match mv.kind {
            MoveKind::ArcCw { centre } => Some((centre, false)),
            MoveKind::ArcCcw { centre } => Some((centre, true)),
            _ => None,
        };
        let length_mm = match arc {
            Some((centre, ccw)) => a.distance_to(centre) * sweep(a, b, centre, ccw).abs(),
            None => a.distance_to(b).max((from.z - mv.to.z).abs()),
        };
        let n = (length_mm / step_mm).ceil().max(1.0);
        let mut k = 0.0;
        while k <= n {
            let t = k / n;
            let xy = match arc {
                Some((centre, ccw)) => {
                    let r = a.distance_to(centre);
                    let angle =
                        (a.y - centre.y).atan2(a.x - centre.x) + sweep(a, b, centre, ccw) * t;
                    Point::new(centre.x + r * angle.cos(), centre.y + r * angle.sin())
                }
                None => Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t),
            };
            out.push(Point3::new(xy.x, xy.y, from.z + (mv.to.z - from.z) * t));
            k += 1.0;
        }
    }
    out
}
