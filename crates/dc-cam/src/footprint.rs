// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The area a toolpath's cutting moves sweep (`docs/specs/phase-2/design.md`,
//! "Data model → dc-cam", `footprint`; "Algorithms → Pocket" step 12 and
//! the coverage check). Safety rule S9 and the coverage tests compare it
//! against the regions an operation may cut.
//!
//! A closed loop of moves (a pocket ring, a profile lap) sweeps the band
//! between its offsets by `±r`, which `dc_geometry::offset` gives exactly.
//! Any other cutting move sweeps a capsule: the segment grown by `r` with
//! round ends; arcs are first split into chords within the tolerance.

use crate::toolpath::{Move, MoveKind, Toolpath};
use dc_geometry::{
    Arc, Contour, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Polyline, Region, Vertex, nest,
    offset, union,
};

/// The area, mm², Y-up, that a tool of radius `radius_mm` sweeps along
/// the cutting moves of `toolpath`: every linear and arc move's swept
/// band, round-ended, as the union of them all. Rapids are left out (S2
/// keeps them above the stock) and so are vertical moves, which sweep only
/// the disc at their end that the move before or after sweeps too. Arcs
/// are swept as arcs. The result is polygonal, flattened at
/// [`FLATTEN_TOLERANCE_DEFAULT_MM`], as [`dc_geometry::union`] returns it;
/// the crate-private `footprint_at` takes the tolerance.
#[must_use]
pub fn footprint(toolpath: &Toolpath, radius_mm: f64) -> Vec<Region> {
    footprint_at(toolpath, radius_mm, FLATTEN_TOLERANCE_DEFAULT_MM)
}

/// [`footprint`] flattened at `tolerance_mm`.
#[must_use]
pub fn footprint_at(toolpath: &Toolpath, radius_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    if !(radius_mm.is_finite() && radius_mm > 0.0) {
        return Vec::new();
    }
    let mut pieces: Vec<Region> = Vec::new();
    for run in runs(toolpath) {
        sweep_run(&run, radius_mm, tolerance_mm, &mut pieces);
    }
    if pieces.is_empty() {
        return Vec::new();
    }
    union(&pieces, &[], tolerance_mm)
}

/// One move of a run: its start, and the move.
type Step = (Point, Move);

/// The runs of consecutive XY cutting moves at one Z.
fn runs(toolpath: &Toolpath) -> Vec<Vec<Step>> {
    let mut out: Vec<Vec<Step>> = Vec::new();
    let mut current: Vec<Step> = Vec::new();
    let mut pos = match toolpath.moves.first() {
        Some(m) => m.to,
        None => return out,
    };
    for mv in &toolpath.moves {
        let level = (mv.to.z - pos.z).abs() <= EPS_MM;
        let xy = pos.xy().distance_to(mv.to.xy()) > EPS_MM;
        let cutting = !mv.is_rapid() && xy;
        if cutting && level {
            current.push((pos.xy(), *mv));
        } else {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
            // A move that changes Z and XY (a ramp) sweeps like any other.
            if cutting {
                out.push(vec![(pos.xy(), *mv)]);
            }
        }
        pos = mv.to;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Adds the swept area of `run`: each closed loop in it as a band, every
/// other move as capsules.
fn sweep_run(run: &[Step], radius_mm: f64, tolerance_mm: f64, out: &mut Vec<Region>) {
    let mut start = 0;
    let mut i = 0;
    while i < run.len() {
        let Some(&(_, mv)) = run.get(i) else { break };
        let end = mv.to.xy();
        // Does this move close a loop begun at some move j ≥ start?
        let closing = (start..i).find(|&j| {
            run.get(j)
                .is_some_and(|(from, _)| from.distance_to(end) <= EPS_MM)
        });
        if let Some(j) = closing {
            let open = run.get(start..j).unwrap_or(&[]);
            let looped = run.get(j..=i).unwrap_or(&[]);
            capsules(open, radius_mm, tolerance_mm, out);
            if !band(looped, radius_mm, out) {
                capsules(looped, radius_mm, tolerance_mm, out);
            }
            start = i + 1;
        }
        i += 1;
    }
    capsules(
        run.get(start..).unwrap_or(&[]),
        radius_mm,
        tolerance_mm,
        out,
    );
}

/// The bulge of `mv` from `from`: 0 for a line.
pub(crate) fn move_bulge(from: Point, mv: &Move) -> f64 {
    let (centre, ccw) = match mv.kind {
        MoveKind::ArcCcw { centre } => (centre, true),
        MoveKind::ArcCw { centre } => (centre, false),
        _ => return 0.0,
    };
    let sweep = Arc::from_ends(from, mv.to.xy(), centre, ccw).sweep_rad;
    (sweep / 4.0).tan()
}

/// Adds the band a closed loop sweeps; false when the loop is not a simple
/// contour (it crosses itself), so the caller sweeps it move by move.
fn band(looped: &[Step], radius_mm: f64, out: &mut Vec<Region>) -> bool {
    let vertices: Vec<Vertex> = looped
        .iter()
        .map(|(from, mv)| Vertex::new(from.x, from.y, move_bulge(*from, mv)))
        .collect();
    let Ok(contour) = Contour::new(Polyline::new(vertices, true)) else {
        return false;
    };
    let mut contours = offset(&contour, radius_mm);
    if contours.is_empty() {
        return false;
    }
    // The inward offsets are what the loop leaves uncut inside it.
    contours.extend(offset(&contour, -radius_mm));
    match nest(contours) {
        Ok((regions, _)) if regions.iter().all(|r| r.depth() == 0) => {
            out.extend(regions);
            true
        }
        _ => false,
    }
}

/// Adds a capsule per line, and per chord of each arc.
fn capsules(steps: &[Step], radius_mm: f64, tolerance_mm: f64, out: &mut Vec<Region>) {
    for &(from, mv) in steps {
        let path = Polyline::new(
            vec![
                Vertex::new(from.x, from.y, move_bulge(from, &mv)),
                Vertex::line(mv.to.x, mv.to.y),
            ],
            false,
        )
        .with_arcs_flattened(tolerance_mm);
        for segment in path.segments() {
            if let Some(region) = capsule(segment.start, segment.end, radius_mm) {
                out.push(region);
            }
        }
    }
}

/// The segment `a → b` grown by `radius_mm`, round-ended.
pub(crate) fn capsule(a: Point, b: Point, radius_mm: f64) -> Option<Region> {
    let along = (b - a).normalized()?;
    let left = along.perp() * radius_mm;
    // Counter-clockwise: up the right side, round b, back down the left,
    // round a. Each end is a half circle (bulge 1).
    let corners = [a + -left, b + -left, b + left, a + left];
    let bulges = [0.0, 1.0, 0.0, 1.0];
    let vertices = corners
        .iter()
        .zip(bulges)
        .map(|(p, bulge)| Vertex::new(p.x, p.y, bulge))
        .collect();
    let contour = Contour::new(Polyline::new(vertices, true)).ok()?;
    nest(vec![contour])
        .ok()
        .and_then(|(regions, _)| regions.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::{Point3, area_mm2, assert_approx_eq};
    use std::f64::consts::PI;

    /// A 1 mm circle flattened to chords 0.01 mm off it loses about
    /// 0.036 mm² (two thirds of the sagitta times the circumference).
    const CHORD_LOSS_MM2: f64 = 0.05;

    fn path(points: &[(f64, f64)]) -> Toolpath {
        let mut moves = vec![Move::rapid(Point3::new(points[0].0, points[0].1, 10.0))];
        moves.push(Move::linear(
            Point3::new(points[0].0, points[0].1, 0.0),
            300.0,
        ));
        for &(x, y) in &points[1..] {
            moves.push(Move::linear(Point3::new(x, y, 0.0), 1000.0));
        }
        moves.push(Move::rapid(Point3::new(
            points[points.len() - 1].0,
            points[points.len() - 1].1,
            10.0,
        )));
        Toolpath { moves }
    }

    #[test]
    fn a_line_sweeps_a_capsule() {
        let fp = footprint(&path(&[(0.0, 0.0), (10.0, 0.0)]), 1.0);
        assert_eq!(fp.len(), 1);
        assert_approx_eq!(area_mm2(&fp), 20.0 + PI, CHORD_LOSS_MM2);
    }

    #[test]
    fn a_closed_square_sweeps_a_band() {
        let fp = footprint(
            &path(&[
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
                (0.0, 0.0),
            ]),
            1.0,
        );
        assert_eq!(fp.len(), 1);
        assert_eq!(fp[0].holes().len(), 1);
        // Outer 12 × 12 less the round corners, less the 8 × 8 inside.
        assert_approx_eq!(area_mm2(&fp), 144.0 - (4.0 - PI) - 64.0, CHORD_LOSS_MM2);
    }

    #[test]
    fn a_half_circle_arc_sweeps_an_annular_sector() {
        let tp = Toolpath {
            moves: vec![
                Move::rapid(Point3::new(10.0, 0.0, 10.0)),
                Move::linear(Point3::new(10.0, 0.0, 0.0), 300.0),
                Move {
                    kind: MoveKind::ArcCcw {
                        centre: Point::new(0.0, 0.0),
                    },
                    to: Point3::new(-10.0, 0.0, 0.0),
                    feed_mm_min: Some(1000.0),
                },
            ],
        };
        let fp = footprint(&tp, 1.0);
        // Half the annulus 9..11 plus two half discs at the ends.
        let want = PI / 2.0 * (121.0 - 81.0) + PI;
        assert_approx_eq!(area_mm2(&fp), want, 0.2);
        assert!(fp.iter().all(|r| !r.contains_point(Point::new(0.0, -10.0))));
        assert!(fp.iter().any(|r| r.contains_point(Point::new(0.0, 10.5))));
    }
}
