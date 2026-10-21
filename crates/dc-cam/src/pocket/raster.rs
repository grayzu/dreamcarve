// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/pocket.ts (buildPocketParallelSegments)

//! The raster clearance strategy of a pocket (`docs/specs/phase-2/design.md`,
//! "Algorithms → Pocket", step 7): parallel lines across each tool-centre
//! region, alternating direction, then (or before them) the wall rings.
//!
//! [`raster_lines`] is upstream `buildPocketParallelSegments`: rotate the
//! region by −angle about the origin, scan horizontal lines from
//! `min_y + s/2` while below `max_y − s/2`, step `s`, intersect each with
//! the region by the even-odd rule over its outer and islands, and rotate
//! the pieces back. The angle convention needs no code change for Y-up
//! (ADR-0001): the rotation is the same arithmetic, but in a Y-up frame
//! `+angle` is counter-clockwise from +X as drawn, so PureCutCNC's +45°
//! (Y-down) is DreamCarve's −45°, that is 135°.
//!
//! Changed from upstream:
//! - A region shorter than one step-over across the lines gets one line
//!   through its middle, where upstream's scan gives none (design step 7).
//! - When the scan leaves more than one step-over between its last line
//!   and the far side of the region, one more line is added half a
//!   step-over from that side. Upstream's last line can sit up to 1.5
//!   step-overs from it, which at a step-over above two thirds of the
//!   diameter leaves a ridge the wall pass does not reach.
//! - A reversed line also reverses the order of its pieces, so a line
//!   split by an island is cut in one sweep in its direction. Upstream
//!   reorders every piece greedily afterwards (`orderOpenSegmentsGreedy`);
//!   here the pieces keep scan order and `sequence` links them.

use super::rings::{RingNode, RingTree, WallRings, inset_region};
use super::{MAX_RINGS, ring_path};
use crate::footprint::capsule;
use dc_geometry::{EPS_MM, Point, Polyline, Region, Vertex, area_mm2, difference, intersection};
use dc_project::CutDirection;

/// One raster line piece, in cut direction (mm, Y-up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RasterLine {
    /// Where the cut starts.
    pub start: Point,
    /// Where the cut ends.
    pub end: Point,
}

/// Below this, mm, two crossings of a scan line are one, and an edge is
/// horizontal (upstream's `1e-9`).
const SCAN_EPS_MM: f64 = 1e-9;

/// The raster lines across `region` at `angle_deg` (degrees from +X,
/// counter-clockwise, Y-up), `stepover_mm` apart, in cut order: scan line
/// by scan line, the first running along `+angle` unless `first_reversed`,
/// each next one the other way. The region's arcs are flattened to chords
/// within `tolerance_mm` first, so every line ends on a chord. A line
/// crossing an island is several pieces, each its own [`RasterLine`].
/// `None` when the region would need more than `max_lines` scan lines.
#[must_use]
pub fn raster_lines(
    region: &Region,
    stepover_mm: f64,
    angle_deg: f64,
    tolerance_mm: f64,
    first_reversed: bool,
    max_lines: usize,
) -> Option<Vec<RasterLine>> {
    let (sin, cos) = angle_deg.to_radians().sin_cos();
    // Into the scan frame (rotate by −angle) and back (by +angle).
    let to_scan = |p: Point| Point::new(p.x * cos + p.y * sin, -p.x * sin + p.y * cos);
    let from_scan = |p: Point| Point::new(p.x * cos - p.y * sin, p.x * sin + p.y * cos);
    let loops: Vec<Vec<Point>> = std::iter::once(region.outer())
        .chain(region.holes())
        .map(|contour| {
            contour
                .polyline()
                .with_arcs_flattened(tolerance_mm)
                .vertices()
                .iter()
                .map(|v| to_scan(v.point()))
                .collect()
        })
        .collect();
    let Some((min_y, max_y)) = loops.first().and_then(|outer| y_bounds(outer)) else {
        return Some(Vec::new());
    };

    let mut ys = Vec::new();
    if max_y - min_y < stepover_mm {
        ys.push((min_y + max_y) / 2.0);
    } else {
        let mut y = min_y + stepover_mm / 2.0;
        while y < max_y - stepover_mm / 2.0 + SCAN_EPS_MM {
            if ys.len() >= max_lines {
                return None;
            }
            ys.push(y);
            y = min_y + stepover_mm / 2.0 + stepover_mm * ys.len() as f64;
        }
        if ys
            .last()
            .is_some_and(|&last| max_y - last > stepover_mm + EPS_MM)
        {
            ys.push(max_y - stepover_mm / 2.0);
        }
    }
    if ys.len() > max_lines {
        return None;
    }

    let mut lines = Vec::new();
    for (scan, &y) in ys.iter().enumerate() {
        let reversed = (scan % 2 == 1) != first_reversed;
        let mut pieces: Vec<RasterLine> = intervals(&loops, y)
            .into_iter()
            .map(|(a, b)| RasterLine {
                start: from_scan(Point::new(a, y)),
                end: from_scan(Point::new(b, y)),
            })
            .collect();
        if reversed {
            pieces.reverse();
            for piece in &mut pieces {
                std::mem::swap(&mut piece.start, &mut piece.end);
            }
        }
        lines.extend(pieces);
    }
    Some(lines)
}

/// The lowest and highest Y of `points`, `None` for fewer than three.
fn y_bounds(points: &[Point]) -> Option<(f64, f64)> {
    if points.len() < 3 {
        return None;
    }
    let min_y = points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let max_y = points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max);
    (min_y.is_finite() && max_y.is_finite()).then_some((min_y, max_y))
}

/// The intervals of the line at `y` inside `loops` by the even-odd rule,
/// ascending: upstream `scanlineCrossings` over every loop, then paired.
/// Horizontal edges are skipped and an edge counts when `a.y ≤ y < b.y`
/// or `b.y ≤ y < a.y`, so a vertex on the line is crossed once. Intervals
/// no longer than [`EPS_MM`] are dropped: there is nothing to cut.
fn intervals(loops: &[Vec<Point>], y: f64) -> Vec<(f64, f64)> {
    let mut xs = Vec::new();
    for points in loops {
        let next = points.iter().skip(1).chain(points.first());
        for (a, b) in points.iter().zip(next) {
            if (a.y - b.y).abs() <= SCAN_EPS_MM {
                continue;
            }
            if (a.y <= y && b.y > y) || (b.y <= y && a.y > y) {
                xs.push(a.x + (b.x - a.x) * (y - a.y) / (b.y - a.y));
            }
        }
    }
    xs.sort_by(f64::total_cmp);
    xs.as_chunks::<2>()
        .0
        .iter()
        .filter_map(|&[a, b]| (b - a > EPS_MM).then_some((a, b)))
        .collect()
}

/// What one tool-centre region's raster paths need.
#[derive(Debug, Clone, Copy)]
pub(super) struct RasterSettings {
    /// Distance between lines, mm.
    pub stepover_mm: f64,
    /// Tool radius, mm.
    pub radius_mm: f64,
    /// Degrees from +X, counter-clockwise, in [0, 180).
    pub angle_deg: f64,
    /// Flattening tolerance, mm.
    pub tolerance_mm: f64,
    /// Where the wall rings go.
    pub wall: WallRings,
    /// Climb or conventional, for the wall rings only.
    pub direction: CutDirection,
}

/// The raster paths of one tool-centre region `root` in cut order, each
/// with the area it has cleared at its Z: the raster lines (open paths),
/// then the wall rings for `WallRings::Last`, the rings first for
/// `WallRings::First`, or none. `index` is the region's place among the
/// pocket's tool-centre regions; odd regions start with a reversed line,
/// as upstream does. `anchor` is the end of the previous path and is moved
/// to the end of the last one. `None` when the lines would be more than
/// [`MAX_RINGS`].
///
/// Cleared areas, in tool-centre terms (issue #137: never grown past the
/// tool-centre region, so a stay-down link keeps the footprint inside the
/// pocket):
/// - a line: its strip, the line grown by the tool radius, cut to `root`.
///   Two lines' strips meet when the lines are at most a diameter apart,
///   so `sequence` stays down from one line to the next inside `root`, and
///   lifts across an island, which lies outside both.
/// - a wall ring cut last: all of `root`, which the lines have cleared.
/// - a wall ring cut first: the band of `root` within the tool radius of
///   its rings, which is all the ring itself has cleared.
pub(super) fn region_paths(
    root: &Region,
    index: usize,
    settings: &RasterSettings,
    anchor: &mut Option<Point>,
) -> Option<Vec<(Polyline, Option<Region>)>> {
    let lines = raster_lines(
        root,
        settings.stepover_mm,
        settings.angle_deg,
        settings.tolerance_mm,
        index % 2 == 1,
        MAX_RINGS,
    )?;
    let line_paths = || {
        lines.iter().map(|line| {
            let path = Polyline::new(
                vec![
                    Vertex::line(line.start.x, line.start.y),
                    Vertex::line(line.end.x, line.end.y),
                ],
                false,
            );
            (path, strip(root, line, settings))
        })
    };
    let mut out = Vec::new();
    match settings.wall {
        WallRings::First => {
            // Start the rings near the first line, so the ring that ends
            // there links to it without a lift.
            *anchor = lines.first().map(|l| l.start).or(*anchor);
            let band = difference(
                std::slice::from_ref(root),
                &inset_region(root, settings.radius_mm, settings.tolerance_mm),
                settings.tolerance_mm,
            );
            for (path, start) in wall_paths(root, settings.direction, anchor) {
                let cleared = band.iter().find(|r| r.contains_point(start)).cloned();
                out.push((path, cleared));
            }
            out.extend(line_paths());
            *anchor = lines.last().map(|l| l.end).or(*anchor);
        }
        WallRings::Last => {
            out.extend(line_paths());
            *anchor = lines.last().map(|l| l.end).or(*anchor);
            for (path, _) in wall_paths(root, settings.direction, anchor) {
                out.push((path, Some(root.clone())));
            }
        }
        WallRings::Omitted => {
            out.extend(line_paths());
            *anchor = lines.last().map(|l| l.end).or(*anchor);
        }
    }
    Some(out)
}

/// The wall rings of `root` (its outer and its islands), nearest `anchor`
/// first, each oriented by the direction rule and starting at its vertex
/// nearest the previous ring's end; with each ring's start.
fn wall_paths(
    root: &Region,
    direction: CutDirection,
    anchor: &mut Option<Point>,
) -> Vec<(Polyline, Point)> {
    let tree = RingTree {
        nodes: vec![RingNode {
            region: root.clone(),
            children: Vec::new(),
        }],
    };
    tree.walk(WallRings::Last, anchor)
        .into_iter()
        .map(|ring| {
            let start = ring.start;
            (ring_path(ring, direction), start)
        })
        .collect()
}

/// The strip `line` clears in tool-centre terms: the line grown by the
/// tool radius, cut to `root`; the piece holding the line. `None` when
/// there is none, so `sequence` lifts on both sides of the line.
fn strip(root: &Region, line: &RasterLine, settings: &RasterSettings) -> Option<Region> {
    let grown = capsule(line.start, line.end, settings.radius_mm)?;
    let pieces = intersection(
        std::slice::from_ref(&grown),
        std::slice::from_ref(root),
        settings.tolerance_mm,
    );
    let mid = Point::new(
        (line.start.x + line.end.x) / 2.0,
        (line.start.y + line.end.y) / 2.0,
    );
    pieces
        .iter()
        .find(|r| r.contains_point(mid))
        .or_else(|| {
            pieces.iter().max_by(|a, b| {
                area_mm2(std::slice::from_ref(a)).total_cmp(&area_mm2(std::slice::from_ref(b)))
            })
        })
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::{Contour, FLATTEN_TOLERANCE_DEFAULT_MM, assert_approx_eq, nest};

    const TOL: f64 = FLATTEN_TOLERANCE_DEFAULT_MM;

    fn region(polylines: Vec<Polyline>) -> Region {
        let contours = polylines
            .into_iter()
            .map(|p| Contour::new(p).unwrap())
            .collect();
        nest(contours).unwrap().0.remove(0)
    }

    fn rect(width_mm: f64, height_mm: f64) -> Region {
        region(vec![Polyline::rect(
            Point::new(0.0, 0.0),
            width_mm,
            height_mm,
        )])
    }

    /// Upstream's scan starts from its Y-down `minY`, which is the top in
    /// Y-up (#195): 10 high at step 2 gives lines at 9, 7, 5, 3 and 1, top
    /// first, alternating, each across the full width.
    #[test]
    fn scan_starts_half_a_step_below_the_top() {
        let lines = raster_lines(&rect(20.0, 10.0), 2.0, 0.0, TOL, false, 100).unwrap();
        let ys: Vec<f64> = lines.iter().map(|l| l.start.y).collect();
        assert_eq!(ys.len(), 5, "{lines:?}");
        for (y, want) in ys.iter().zip([9.0, 7.0, 5.0, 3.0, 1.0]) {
            assert_approx_eq!(*y, want, 1e-9);
        }
        assert_approx_eq!(lines[0].start.x, 0.0, 1e-9);
        assert_approx_eq!(lines[0].end.x, 20.0, 1e-9);
        assert_approx_eq!(lines[1].start.x, 20.0, 1e-9);
        assert_approx_eq!(lines[1].end.x, 0.0, 1e-9);
        let reversed = raster_lines(&rect(20.0, 10.0), 2.0, 0.0, TOL, true, 100).unwrap();
        assert_approx_eq!(reversed[0].start.x, 20.0, 1e-9);
    }

    /// Narrower than a step-over: one line through the middle.
    #[test]
    fn narrow_region_gets_one_middle_line() {
        let lines = raster_lines(&rect(20.0, 1.5), 2.0, 0.0, TOL, false, 100).unwrap();
        assert_eq!(lines.len(), 1);
        assert_approx_eq!(lines[0].start.y, 0.75, 1e-9);
    }

    /// 10 high at step 3.6: the scan from the top gives 8.2 and 4.6, 4.6
    /// above the bottom; a closing line at 1.8 keeps the gap within a
    /// step-over.
    #[test]
    fn closing_line_when_the_scan_stops_short() {
        let lines = raster_lines(&rect(20.0, 10.0), 3.6, 0.0, TOL, false, 100).unwrap();
        let ys: Vec<f64> = lines.iter().map(|l| l.start.y).collect();
        assert_eq!(ys.len(), 3, "{ys:?}");
        for (y, want) in ys.iter().zip([8.2, 4.6, 1.8]) {
            assert_approx_eq!(*y, want, 1e-9);
        }
    }

    /// At 90° the lines run along +Y and −Y, one step apart in X.
    #[test]
    fn ninety_degrees_runs_along_y() {
        let lines = raster_lines(&rect(10.0, 20.0), 2.0, 90.0, TOL, false, 100).unwrap();
        assert_eq!(lines.len(), 5);
        assert_approx_eq!(lines[0].start.x, lines[0].end.x, 1e-9);
        assert!(lines[0].end.y > lines[0].start.y, "{:?}", lines[0]);
        let mut xs: Vec<f64> = lines.iter().map(|l| l.start.x).collect();
        xs.sort_by(f64::total_cmp);
        assert_approx_eq!(xs[0], 1.0, 1e-9);
        assert_approx_eq!(xs[4], 9.0, 1e-9);
    }

    /// An island splits a line in two; a reversed line is cut right to
    /// left, so its right piece comes first.
    #[test]
    fn island_splits_lines_and_reversed_lines_keep_their_sweep() {
        let r = region(vec![
            Polyline::rect(Point::new(0.0, 0.0), 30.0, 10.0),
            Polyline::rect(Point::new(10.0, 2.0), 10.0, 6.0),
        ]);
        let lines = raster_lines(&r, 2.0, 0.0, TOL, false, 100).unwrap();
        // Y 9 and 1 clear the island; 7, 5 and 3 are split.
        assert_eq!(lines.len(), 8, "{lines:?}");
        // Y 7 is the second scan line, reversed: right piece first.
        let at_7: Vec<&RasterLine> = lines
            .iter()
            .filter(|l| (l.start.y - 7.0).abs() < 1e-9)
            .collect();
        assert_eq!(at_7.len(), 2);
        assert_approx_eq!(at_7[0].start.x, 30.0, 1e-9);
        assert_approx_eq!(at_7[0].end.x, 20.0, 1e-9);
        assert_approx_eq!(at_7[1].start.x, 10.0, 1e-9);
        assert_approx_eq!(at_7[1].end.x, 0.0, 1e-9);
    }

    #[test]
    fn too_many_lines_is_none() {
        assert!(raster_lines(&rect(20.0, 10.0), 0.001, 0.0, TOL, false, 100).is_none());
    }
}
