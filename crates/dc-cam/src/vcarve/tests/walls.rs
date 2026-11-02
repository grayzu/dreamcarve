// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests that a V-Carve's groove reaches its walls (#457;
//! `docs/specs/phase-3/design.md`, "V-Carve generator", step 5,
//! "Simplify"). The programs are read as a heightfield reads them: a wall
//! point is reached within `δ` when some cutting move's cone comes within
//! `δ` of it.

use super::*;
use crate::heightfield::cone_low_mm;

/// How near the groove must come to every wall, mm (#457).
const REACH_MM: f64 = 0.01;

/// How far apart the walls are sampled, mm.
const SAMPLE_MM: f64 = 0.05;

/// Heights above the start plane the groove's walls are read at, mm: the
/// start plane, and the V wall above it, where a carve whose start plane
/// was cleared, or a plug, has material.
const HEIGHTS_MM: [f64; 3] = [0.0, 0.5, 1.0];

/// The cutting moves of a toolpath, each with the box its cone can reach
/// within [`REACH_MM`] of the start plane, for quick rejection.
struct Moves {
    moves: Vec<(Point3, Point3, dc_geometry::Rect)>,
}

impl Moves {
    fn of(toolpath: &Toolpath, start_z_mm: f64, slope: f64) -> Self {
        let reach = |z: f64| (start_z_mm - z).max(0.0) * slope + REACH_MM + 1.0;
        let moves = cuts(toolpath)
            .into_iter()
            .map(|(a, b)| {
                let r = reach(a.z.min(b.z));
                let rect = dc_geometry::Rect {
                    min: Point::new(a.x.min(b.x) - r, a.y.min(b.y) - r),
                    max: Point::new(a.x.max(b.x) + r, a.y.max(b.y) + r),
                };
                (a, b, rect)
            })
            .collect();
        Self { moves }
    }

    /// The lowest the cone reaches over `q`, mm.
    fn low_mm(&self, q: Point, slope: f64) -> f64 {
        self.moves
            .iter()
            .filter(|(_, _, r)| {
                q.x >= r.min.x && q.x <= r.max.x && q.y >= r.min.y && q.y <= r.max.y
            })
            .map(|&(a, b, _)| cone_low_mm(a, b, q, slope))
            .fold(f64::INFINITY, f64::min)
    }
}

/// How far from wall point `w` the groove may stop, mm: [`REACH_MM`], or
/// within `written / sin(θ/2)` of a convex corner of angle `θ` of
/// `regions`, that distance (#469). There the carve stops where its tip
/// is `written_mm` from both walls, the most writing a point can move it,
/// and leaves a sliver no wider than that.
fn allowance_mm(regions: &[Region], w: Point, written_mm: f64, tolerance_mm: f64) -> f64 {
    regions
        .iter()
        .flat_map(|r| dc_geometry::medial::convex_corners(r, 0.0, tolerance_mm))
        .map(|c| {
            let short_mm = written_mm / ((std::f64::consts::PI - c.turn_rad) / 2.0).sin();
            if c.p.distance_to(w) <= short_mm {
                short_mm + REACH_MM
            } else {
                REACH_MM
            }
        })
        .fold(REACH_MM, f64::max)
}

/// The wall points `toolpath` misses by more than [`allowance_mm`], with
/// by how far and at which height above the start plane: the walls of
/// `regions`, arcs flattened within `tolerance_mm` as the generator
/// carves them, sampled every [`SAMPLE_MM`], each read at the start plane
/// and moved out from the region by `h · slope` for each height `h` of
/// [`HEIGHTS_MM`]. Returns the misses and how many points were read.
fn missed_walls(
    toolpath: &Toolpath,
    regions: &[Region],
    start_z_mm: f64,
    slope: f64,
    (tolerance_mm, written_mm): (f64, f64),
) -> (Vec<(Point, f64, f64)>, usize) {
    let moves = Moves::of(toolpath, start_z_mm, slope);
    let mut missed = Vec::new();
    let mut count = 0;
    for contour in regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
    {
        let flat = contour.polyline().with_arcs_flattened(tolerance_mm);
        for segment in flat.segments() {
            let (a, b) = (segment.start, segment.end);
            let length = a.distance_to(b);
            if length <= EPS_MM {
                continue;
            }
            // The region is on the left of every loop (ADR-0001), so out
            // of it is to the right.
            let out = Point::new((b.y - a.y) / length, (a.x - b.x) / length);
            let pieces = (length / SAMPLE_MM).ceil().max(1.0);
            let mut k = 0.0;
            while k < pieces {
                let t = k / pieces;
                let w = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
                for h in HEIGHTS_MM {
                    let q = Point::new(w.x + out.x * h * slope, w.y + out.y * h * slope);
                    let gap_mm = (moves.low_mm(q, slope) - start_z_mm - h) * slope;
                    count += 1;
                    if gap_mm > REACH_MM
                        && gap_mm > allowance_mm(regions, w, written_mm, tolerance_mm)
                    {
                        missed.push((w, h, gap_mm));
                    }
                }
                k += 1.0;
            }
        }
    }
    (missed, count)
}

/// A wedge whose walls meet at 5°, 30 mm long.
fn wedge() -> Polyline {
    let half = 2.5_f64.to_radians().tan() * 30.0;
    polygon(&[(0.0, 0.0), (30.0, -half), (30.0, half)])
}

/// #457: the V-bit's groove reaches every wall within [`REACH_MM`], at
/// the start plane and on the V wall above it: the sign's lettering
/// (`tests/fixtures/sign.dcv`, op3: serif letters, the 90° bit, no flat
/// depth), a 5° wedge and a 6 mm circle with the 60° bit. Before #457 the
/// medial chains were simplified with upstream's Douglas-Peucker tolerance
/// (a quarter of the sampling step, measured on the medial axis), so a
/// chord could run that far off the axis and its cone, fitted to its own
/// depth, stopped up to twice that short of the farther wall. Since #469 a
/// corner's groove stops where its tip is `quantum / √2` from both walls
/// ([`allowance_mm`]): 0.016 mm short of the wedge's 5° point.
#[test]
fn groove_reaches_the_walls() {
    let sign = dc_project::load(include_bytes!("../../../../../tests/fixtures/sign.dcv"))
        .expect("the sign loads")
        .project;
    let lettering = sign
        .operations
        .iter()
        .find_map(|o| match o {
            Operation::VCarve(op) if op.name == "Lettering" => Some(op.clone()),
            _ => None,
        })
        .expect("the sign has its lettering");
    let wedge = project(vec![wedge()]);
    let circle = project(vec![Polyline::circle(Point::new(3.0, 3.0), 3.0)]);
    let cases = [
        ("the sign's lettering", sign.clone(), lettering, 90.0),
        ("a 5° wedge", wedge.clone(), carve_op(&wedge), 60.0),
        ("a circle", circle.clone(), carve_op(&circle), 60.0),
    ];
    let mut failures = Vec::new();
    for (name, p, op, angle_deg) in cases {
        let (toolpath, _) = carve(&p, &op);
        let regions = regions_of(&with_op(&p, &op), &op);
        let (missed, count) = missed_walls(
            &toolpath,
            &regions,
            p.stock.thickness_mm.expect("a thickness") - op.start_depth_mm,
            slope_of(angle_deg),
            (p.flatten_tolerance_mm, written_mm(&p)),
        );
        assert!(count > 100, "{name}: {count} wall points");
        if let Some(worst) = missed.iter().max_by(|a, b| a.2.total_cmp(&b.2)) {
            failures.push(format!(
                "{name}: {} of {count} wall points missed, the worst by {:.4} mm at {:?}, {} mm up",
                missed.len(),
                worst.2,
                worst.0,
                worst.1
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
