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
//! `max_y − s/2` while above `min_y + s/2`, step `s`, intersect each with
//! the region by the even-odd rule over its outer and islands, and rotate
//! the pieces back. Upstream scans from `minY + s/2` of its Y-down frame,
//! the top of the region on the machine, so the Y-up scan starts at the
//! top (#195). The angle convention needs no code change for Y-up
//! (ADR-0001): the rotation is the same arithmetic, but in a Y-up frame
//! `+angle` is counter-clockwise from +X as drawn, so PureCutCNC's +45°
//! (Y-down) is DreamCarve's −45°, that is 135°. `order_nearest_first`
//! is upstream `orderOpenSegmentsGreedy`, which orders the pieces for
//! cutting.
//!
//! Changed from upstream:
//! - A region shorter than one step-over across the lines gets one line
//!   through its middle, where upstream's scan gives none (design step 7).
//! - When the scan leaves more than one step-over between its last line
//!   and the far side of the region, one more line is added half a
//!   step-over from that side. Upstream's last line can sit up to 1.5
//!   step-overs from it, which at a step-over above two thirds of the
//!   diameter leaves a ridge the wall pass does not reach.
//! - The pieces are ordered nearest-first per tool-centre region, not
//!   over every region of the pocket at once; the regions keep their
//!   order, each starting nearest the end of the one before (#150).
//! - With a wall pass, two line ends against one island that no straight
//!   link joins inside the region are joined along the island's
//!   tool-centre ring, at the pass Z and the feed, when that way is no
//!   longer than a stay-down link may be (#202). Upstream stays down on a
//!   straight link up to a diameter long there, which cuts into a curved
//!   island side. The same holds for two line ends against the pocket
//!   wall where it curves inward, joined along the region's outer ring
//!   (#218).

use super::rings::{Ring, RingNode, RingTree, WallRings, closest_point, inset_region, shorter_way};
use super::{MAX_RINGS, ring_path};
use crate::footprint::capsule;
use crate::sequence::{link_inside_cleared, nearest_of};
use dc_geometry::{
    EPS_MM, Point, Polyline, Region, Vertex, area_mm2, difference, intersection, union,
};
use dc_project::CutDirection;
use std::collections::BTreeMap;

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
/// counter-clockwise, Y-up), `stepover_mm` apart, in scan order: scan line
/// by scan line from the top of the rotated region, the first running
/// along `+angle` unless `first_reversed`, each next one the other way;
/// `order_nearest_first` orders them for cutting. The region's arcs are
/// flattened to chords within `tolerance_mm` first, so every line ends on
/// a chord. A line
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

    // Upstream scans from `minY` of its Y-down frame, which is `max_y`
    // here: top down, while above `min_y + s/2` (#195).
    let mut ys = Vec::new();
    if max_y - min_y < stepover_mm {
        ys.push((min_y + max_y) / 2.0);
    } else {
        let mut y = max_y - stepover_mm / 2.0;
        while y > min_y + stepover_mm / 2.0 - SCAN_EPS_MM {
            if ys.len() >= max_lines {
                return None;
            }
            ys.push(y);
            y = max_y - stepover_mm / 2.0 - stepover_mm * ys.len() as f64;
        }
        if ys
            .last()
            .is_some_and(|&last| last - min_y > stepover_mm + EPS_MM)
        {
            ys.push(min_y + stepover_mm / 2.0);
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

/// A raster line piece and the strip it clears ([`region_paths`]).
pub(super) type Piece = (RasterLine, Option<Region>);

/// One end of a piece: its index, and 0 for its start or 1 for its end.
type End = (usize, usize);

/// The rings of one tool-centre region that a link may follow instead of
/// lifting: its holes, the tool-centre boundary round each island (#202),
/// and its outer, the boundary along the pocket wall (#218), where the
/// tool touches the wall or island and does not cut into it. Following
/// one cuts only the band the wall pass's ring there cuts, so they are
/// offered only when the wall pass runs ([`Self::of`]). Straight links
/// win wherever they are valid, so the outer is followed only where it
/// curves inward: along an inward arc of the wall, or round the arc of
/// radius `r` at a reflex corner.
#[derive(Debug, Clone, Default)]
pub(super) struct RingEdges {
    rings: Vec<Polyline>,
    /// How far a line end may lie off a ring and still be on it, mm: the
    /// lines end on the ring's chords ([`raster_lines`] flattens it).
    tolerance_mm: f64,
}

impl RingEdges {
    /// The outer and island rings of `root`, when `wall` cuts them; none
    /// without a wall pass, where the band along a wall or island is not
    /// the operation's to cut. A ring a rest operation does not cut (#167)
    /// sweeps only cleared material, so it is offered too.
    pub(super) fn of(root: &Region, wall: WallRings, tolerance_mm: f64) -> Self {
        let rings = match wall {
            WallRings::Last | WallRings::First => std::iter::once(root.outer())
                .chain(root.holes())
                .map(|c| c.polyline().clone())
                .collect(),
            WallRings::Omitted => Vec::new(),
        };
        Self {
            rings,
            tolerance_mm,
        }
    }

    /// The point of `ring` nearest `p`, when `p` lies on it.
    fn on_ring(&self, ring: &Polyline, p: Point) -> Option<Point> {
        let (q, d) = closest_point(ring, p)?;
        (d <= self.tolerance_mm + EPS_MM).then_some(q)
    }

    /// The edge link from `p` to `q`, both on one ring: to the ring (a
    /// move no longer than the tolerance, from a line end on a chord), the
    /// shorter way along it ([`shorter_way`]), and off it to `q`. `None`
    /// when they are not on one ring or the link is longer than
    /// `link_max_mm`, a stay-down link's limit.
    fn link(&self, p: Point, q: Point, link_max_mm: f64) -> Option<Polyline> {
        // The first ring both lie on: where the outer and an island ring
        // come within the tolerance of each other, a point may be on both.
        let (ring, on_p, on_q) = self
            .rings
            .iter()
            .find_map(|ring| Some((ring, self.on_ring(ring, p)?, self.on_ring(ring, q)?)))?;
        let way = shorter_way(ring, on_p, on_q)?;
        let mut vertices = way.vertices().to_vec();
        match vertices.first_mut() {
            Some(first) if on_p.distance_to(p) <= EPS_MM => (first.x, first.y) = (p.x, p.y),
            _ => vertices.insert(0, Vertex::line(p.x, p.y)),
        }
        match vertices.last_mut() {
            Some(last) if on_q.distance_to(q) <= EPS_MM => (last.x, last.y) = (q.x, q.y),
            _ => vertices.push(Vertex::line(q.x, q.y)),
        }
        let link = Polyline::new(vertices, false);
        (link.length_mm() <= link_max_mm + EPS_MM).then_some(link)
    }
}

/// The edge links [`order_nearest_first`] found, from one end to another.
type EdgeLinks = BTreeMap<(End, End), Polyline>;

/// `pieces` in cut order, nearest-first (upstream `orderOpenSegmentsGreedy`,
/// #150): from `start`, the next piece is the one with an end nearest
/// where the tool is, entered at that end, so a piece runs either way.
/// Ties go to the earlier piece in `pieces`, its start before its end, so
/// the order is deterministic. Without `start` the first piece goes first.
///
/// Changed from upstream, which stays down on any link up to a diameter
/// long, even one that cuts across an island's edge:
/// - The next piece is the nearest one `sequence` reaches by a stay-down
///   link: one at most `link_max_mm` long and inside the strips of the two
///   pieces ([`link_inside_cleared`]), its rule. Only when there is none,
///   so the tool lifts anyway, may a nearer piece win.
/// - When the tool lifts, it goes to the nearest end that no stay-down
///   link reaches from a piece still to cut (a dead end, such as a line's
///   end against a round island, where every straight link between ends
///   cuts into the island), so a lift is spent where a link could not be;
///   else to the nearest end.
/// - When `pieces` as given (scan order) would lift fewer times, they are
///   kept as given. Round an island whose sides facing the lines are
///   curved and with no edge to follow, every line end against it is a
///   dead end, so any order lifts about once per split line, and scan
///   order is as good as it gets.
///
/// So the tool zigzags along one side of an island as far as its links
/// stay inside the region before it crosses, and the order never lifts
/// more than scan order does.
///
/// Where no straight link is inside the strips but both ends lie on one
/// of `edges`' rings, a link along that ring, no longer than
/// `link_max_mm` either, counts as a stay-down link too: round a curved
/// island side, where every straight link enters the island (#202), or
/// along a wall that curves inward, where every straight link leaves the
/// region (#218). Each
/// piece comes with the edge link that leads into it from the piece
/// before, which [`region_paths`] joins into one path; `None` where the
/// tool links straight or lifts.
#[must_use]
pub(super) fn order_nearest_first(
    pieces: Vec<Piece>,
    start: Option<Point>,
    link_max_mm: f64,
    edges: &RingEdges,
) -> Vec<(Piece, Option<Polyline>)> {
    let Some(start) = start.or_else(|| pieces.first().map(|(l, _)| l.start)) else {
        return Vec::new();
    };
    let point = |(index, end): End| {
        pieces
            .get(index)
            .map(|(l, _)| if end == 0 { l.start } else { l.end })
    };
    let (links, mut edge_links) = stay_down_links(&pieces, link_max_mm, edges);
    let links_of = |e: End| links.get(e.0).and_then(|l| l.get(e.1));
    let mut done = vec![false; pieces.len()];
    let is_live = |done: &[bool], index: usize| done.get(index).is_some_and(|d| !d);
    let mut order: Vec<End> = Vec::with_capacity(pieces.len());
    let mut current = start;
    while order.len() < pieces.len() {
        // The previous piece's exit is the end other than its entry.
        let exit = order.last().map(|&(index, entry)| (index, 1 - entry));
        let stay = exit
            .and_then(links_of)
            .and_then(|l| l.iter().copied().find(|&(j, _)| is_live(&done, j)));
        let next = stay.or_else(|| {
            // Nearest end, dead ends first once a piece is cut: (live?,
            // end, distance); `false < true`, so live ends come after.
            // Ends within `NEAREST_TIE_MM` of the nearest are a tie, taken
            // in scan order (#215).
            let ends: Vec<(bool, End, f64)> = (0..pieces.len())
                .filter(|&j| is_live(&done, j))
                .flat_map(|j| [(j, 0), (j, 1)])
                .filter_map(|e| {
                    let d = point(e)?.distance_to(current);
                    let live = exit.is_some()
                        && links_of(e).is_some_and(|l| l.iter().any(|&(k, _)| is_live(&done, k)));
                    Some((live, e, d))
                })
                .collect();
            let dead_first = ends.iter().map(|(live, _, _)| *live).min()?;
            nearest_of(
                ends.into_iter()
                    .filter(|(live, _, _)| *live == dead_first)
                    .map(|(_, e, d)| (e, d)),
            )
        });
        let Some(entry) = next else {
            break;
        };
        if let Some(d) = done.get_mut(entry.0) {
            *d = true;
        }
        current = point((entry.0, 1 - entry.1)).unwrap_or(current);
        order.push(entry);
    }
    // Scan order, each piece entered at its start, when it lifts less.
    let lifts = |order: &[End]| {
        order
            .windows(2)
            .filter(|pair| match pair {
                [(i, entry), next] => !links_of((*i, 1 - entry)).is_some_and(|l| l.contains(next)),
                _ => false,
            })
            .count()
    };
    let scan: Vec<End> = (0..pieces.len()).map(|i| (i, 0)).collect();
    if lifts(&scan) < lifts(&order) {
        order = scan;
    }
    let mut slots: Vec<Option<Piece>> = pieces.into_iter().map(Some).collect();
    let mut exit: Option<End> = None;
    order
        .into_iter()
        .filter_map(|(index, entry)| {
            let (mut line, strip) = slots.get_mut(index)?.take()?;
            if entry == 1 {
                std::mem::swap(&mut line.start, &mut line.end);
            }
            let edge = exit.and_then(|from| edge_links.remove(&(from, (index, entry))));
            exit = Some((index, 1 - entry));
            Some(((line, strip), edge))
        })
        .collect()
}

/// For every end of every piece, the ends of other pieces a stay-down link
/// reaches from it, nearest first, ties by index: a straight link at most
/// `link_max_mm` long and inside both strips, else an edge link along a
/// ring of `edges` no longer than that, measured along it, and
/// returned from each end to the other. A link is the same either way, so
/// the lists are symmetric. Ends are swept in X order, so only ends within
/// the limit in X are compared.
fn stay_down_links(
    pieces: &[Piece],
    link_max_mm: f64,
    edges: &RingEdges,
) -> (Vec<[Vec<End>; 2]>, EdgeLinks) {
    let ends: Vec<(Point, End)> = pieces
        .iter()
        .enumerate()
        .flat_map(|(i, (l, _))| [(l.start, (i, 0)), (l.end, (i, 1))])
        .collect();
    let mut by_x: Vec<usize> = (0..ends.len()).collect();
    by_x.sort_by(|&a, &b| {
        let x = |k: usize| ends.get(k).map_or(0.0, |e| e.0.x);
        x(a).total_cmp(&x(b)).then(a.cmp(&b))
    });
    let mut found: Vec<[Vec<(f64, End)>; 2]> = vec![[Vec::new(), Vec::new()]; pieces.len()];
    let mut edge_links = EdgeLinks::new();
    for (k, &a) in by_x.iter().enumerate() {
        let Some(&(p, (i, e))) = ends.get(a) else {
            continue;
        };
        for &b in by_x.iter().skip(k + 1) {
            let Some(&(q, (j, f))) = ends.get(b) else {
                continue;
            };
            if q.x - p.x > link_max_mm + EPS_MM {
                break;
            }
            let mut d = p.distance_to(q);
            if i == j || d > link_max_mm + EPS_MM {
                continue;
            }
            let inside = match (pieces.get(i), pieces.get(j)) {
                (Some((_, Some(a))), Some((_, Some(b)))) => link_inside_cleared(a, b, p, q),
                _ => false,
            };
            if !inside {
                let Some(link) = edges.link(p, q, link_max_mm) else {
                    continue;
                };
                d = link.length_mm();
                edge_links.insert(((j, f), (i, e)), link.reversed());
                edge_links.insert(((i, e), (j, f)), link);
            }
            if let Some(list) = found.get_mut(i).and_then(|l| l.get_mut(e)) {
                list.push((d, (j, f)));
            }
            if let Some(list) = found.get_mut(j).and_then(|l| l.get_mut(f)) {
                list.push((d, (i, e)));
            }
        }
    }
    let links = found
        .into_iter()
        .map(|lists| {
            lists.map(|mut list| {
                list.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
                list.into_iter().map(|(_, end)| end).collect()
            })
        })
        .collect();
    (links, edge_links)
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
/// with the area it has cleared at its Z: the raster lines (open paths)
/// nearest-first from where the tool is ([`order_nearest_first`]), lines
/// joined by an edge link along an island's or the wall's ring (#202,
/// #218) as one path
/// ([`join_edge_links`]), then the wall rings for `WallRings::Last`, the
/// rings first for
/// `WallRings::First` (starting nearest the first scan line), or none.
/// Wall rings `keep` refuses are not cut ([`RingTree::walk_keeping`]).
/// `index` is the region's place among the pocket's tool-centre regions;
/// an odd region's scan order starts with a reversed line, as upstream's
/// does. `anchor` is the end of the previous path and is
/// moved to the end of the last one. `None` when the lines would be more
/// than [`MAX_RINGS`].
///
/// Cleared areas, in tool-centre terms (issue #137: never grown past the
/// tool-centre region, so a stay-down link keeps the footprint inside the
/// pocket):
/// - a line: its strip, the line grown by the tool radius, cut to `root`.
///   Two lines' strips meet when the lines are at most a diameter apart,
///   so `sequence` stays down from one line to the next inside `root`, and
///   lifts across an island, which lies outside both.
/// - lines joined by edge links: their strips and the band the links
///   sweep, cut to `root`.
/// - a wall ring cut last: all of `root`, which the lines have cleared.
/// - a wall ring cut first: the band of `root` within the tool radius of
///   its rings, which is all the ring itself has cleared.
pub(super) fn region_paths(
    root: &Region,
    index: usize,
    settings: &RasterSettings,
    keep: &dyn Fn(&Ring) -> bool,
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
    // The rings cut first start near the first scan line, so the ring that
    // ends there links to it without a lift.
    if settings.wall == WallRings::First {
        *anchor = lines.first().map(|l| l.start).or(*anchor);
    }
    let first_rings = match settings.wall {
        WallRings::First => wall_paths(root, settings.direction, keep, anchor),
        WallRings::Last | WallRings::Omitted => Vec::new(),
    };
    // Nearest-first from where the tool is (#150).
    let pieces = lines
        .into_iter()
        .map(|line| (line, strip(root, &line, settings)))
        .collect();
    let link_max_mm = 2.0 * settings.radius_mm + settings.stepover_mm;
    let edges = RingEdges::of(root, settings.wall, settings.tolerance_mm);
    let pieces = order_nearest_first(pieces, *anchor, link_max_mm, &edges);
    let last_end = pieces.last().map(|((l, _), _)| l.end);
    let joined = join_edge_links(root, pieces, settings);
    let line_paths = || joined.iter().cloned();
    let mut out = Vec::new();
    match settings.wall {
        WallRings::First => {
            let band = difference(
                std::slice::from_ref(root),
                &inset_region(root, settings.radius_mm, settings.tolerance_mm),
                settings.tolerance_mm,
            );
            for (path, start) in first_rings {
                let cleared = band.iter().find(|r| r.contains_point(start)).cloned();
                out.push((path, cleared));
            }
            out.extend(line_paths());
            *anchor = last_end.or(*anchor);
        }
        WallRings::Last => {
            out.extend(line_paths());
            *anchor = last_end.or(*anchor);
            for (path, _) in wall_paths(root, settings.direction, keep, anchor) {
                out.push((path, Some(root.clone())));
            }
        }
        WallRings::Omitted => {
            out.extend(line_paths());
            *anchor = last_end.or(*anchor);
        }
    }
    Some(out)
}

/// The ordered `pieces` as open paths, each with the area it has cleared:
/// a piece on its own is its line and its strip; a run of pieces joined
/// by edge links (#202, #218) is one path, line, edge link, line and so on, so
/// `sequence` sees no link to check inside it, and it has cleared the
/// pieces' strips and the band its edge links sweep (each grown by the
/// tool radius, cut to `root`), the piece of their union holding its first
/// line. In tool-centre terms, as a strip is, so a straight link to or
/// from it still stays inside `root`.
fn join_edge_links(
    root: &Region,
    pieces: Vec<(Piece, Option<Polyline>)>,
    settings: &RasterSettings,
) -> Vec<(Polyline, Option<Region>)> {
    // One path being built: its vertices, its pieces' strips, its edge
    // links, and where its first line is.
    struct Joined {
        vertices: Vec<Vertex>,
        strips: Vec<Region>,
        links: Vec<Polyline>,
        first_mid: Point,
    }
    let finish = |joined: Joined| -> (Polyline, Option<Region>) {
        let path = Polyline::new(joined.vertices, false);
        if joined.links.is_empty() {
            return (path, joined.strips.into_iter().next());
        }
        let grown: Vec<Region> = joined
            .links
            .iter()
            .flat_map(|link| {
                let flat = link.with_arcs_flattened(settings.tolerance_mm);
                flat.segments()
                    .filter_map(|s| capsule(s.start, s.end, settings.radius_mm))
                    .collect::<Vec<_>>()
            })
            .collect();
        let band = intersection(
            &union(&grown, &[], settings.tolerance_mm),
            std::slice::from_ref(root),
            settings.tolerance_mm,
        );
        let cleared = union(&joined.strips, &band, settings.tolerance_mm);
        let piece = cleared
            .iter()
            .find(|r| r.contains_point(joined.first_mid))
            .or_else(|| {
                cleared.iter().max_by(|a, b| {
                    area_mm2(std::slice::from_ref(a)).total_cmp(&area_mm2(std::slice::from_ref(b)))
                })
            })
            .cloned();
        (path, piece)
    };
    let mut out = Vec::new();
    let mut current: Option<Joined> = None;
    for ((line, strip), edge) in pieces {
        match (current.as_mut(), edge) {
            (Some(joined), Some(link)) => {
                // The link starts where the last line ends, with the
                // bulge of its first segment, and ends where this line
                // starts.
                joined.vertices.pop();
                joined.vertices.extend(link.vertices().iter().copied());
                joined.vertices.push(Vertex::line(line.end.x, line.end.y));
                joined.strips.extend(strip);
                joined.links.push(link);
            }
            _ => {
                out.extend(current.take().map(finish));
                current = Some(Joined {
                    vertices: vec![
                        Vertex::line(line.start.x, line.start.y),
                        Vertex::line(line.end.x, line.end.y),
                    ],
                    strips: strip.into_iter().collect(),
                    links: Vec::new(),
                    first_mid: Point::new(
                        (line.start.x + line.end.x) / 2.0,
                        (line.start.y + line.end.y) / 2.0,
                    ),
                });
            }
        }
    }
    out.extend(current.map(finish));
    out
}

/// The wall rings of `root` (its outer and its islands) that `keep`
/// accepts, nearest `anchor` first, each oriented by the direction rule and
/// starting at its point nearest the previous ring's end; with each ring's
/// start.
fn wall_paths(
    root: &Region,
    direction: CutDirection,
    keep: &dyn Fn(&Ring) -> bool,
    anchor: &mut Option<Point>,
) -> Vec<(Polyline, Point)> {
    let tree = RingTree {
        nodes: vec![RingNode {
            region: root.clone(),
            children: Vec::new(),
            residual: false,
            splice: None,
        }],
    };
    tree.walk_keeping(WallRings::Last, anchor, keep)
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

    /// #150: round a square island, the pieces are cut nearest-first: down
    /// the right side with links along the island's edge, one lift to the
    /// bottom line (a dead end: no link reaches it from a piece still to
    /// cut), then up the left side. Scan order lifts at every split line.
    #[test]
    fn pieces_round_a_square_island_are_cut_nearest_first() {
        let r = region(vec![
            Polyline::rect(Point::new(0.0, 0.0), 30.0, 10.0),
            Polyline::rect(Point::new(10.0, 2.0), 10.0, 6.0),
        ]);
        let settings = RasterSettings {
            stepover_mm: 2.0,
            radius_mm: 1.0,
            angle_deg: 0.0,
            tolerance_mm: TOL,
            wall: WallRings::Omitted,
            direction: CutDirection::Climb,
        };
        let pieces: Vec<Piece> = raster_lines(&r, 2.0, 0.0, TOL, false, 100)
            .unwrap()
            .into_iter()
            .map(|l| (l, strip(&r, &l, &settings)))
            .collect();
        let ordered = nearest_first(pieces.clone(), &RingEdges::default());
        let got: Vec<(f64, f64, f64)> = ordered
            .iter()
            .map(|(l, _)| (l.start.y, l.start.x, l.end.x))
            .collect();
        let want = [
            (9.0, 0.0, 30.0),
            (7.0, 30.0, 20.0),
            (5.0, 20.0, 30.0),
            (3.0, 30.0, 20.0),
            (1.0, 30.0, 0.0),
            (3.0, 0.0, 10.0),
            (5.0, 10.0, 0.0),
            (7.0, 0.0, 10.0),
        ];
        assert_eq!(got.len(), want.len(), "{got:?}");
        for (g, w) in got.iter().zip(want) {
            assert!(
                (g.0 - w.0).abs() < 1e-9 && (g.1 - w.1).abs() < 1e-9 && (g.2 - w.2).abs() < 1e-9,
                "{got:?}"
            );
        }
        assert_eq!(nearest_first(pieces, &RingEdges::default()), ordered);
    }

    /// [`order_nearest_first`] from no anchor with a 4 mm link limit, the
    /// pieces only.
    fn nearest_first(pieces: Vec<Piece>, edges: &RingEdges) -> Vec<Piece> {
        order_nearest_first(pieces, None, 4.0, edges)
            .into_iter()
            .map(|(piece, _)| piece)
            .collect()
    }

    /// The 40 × 20 rect with a 5 mm circle island and its pieces at 0°,
    /// step 2, tool radius 1.
    fn circle_pieces() -> (Region, Vec<Piece>) {
        let r = region(vec![
            Polyline::rect(Point::new(0.0, 0.0), 40.0, 20.0),
            Polyline::circle(Point::new(20.0, 10.0), 5.0),
        ]);
        let settings = RasterSettings {
            stepover_mm: 2.0,
            radius_mm: 1.0,
            angle_deg: 0.0,
            tolerance_mm: TOL,
            wall: WallRings::Omitted,
            direction: CutDirection::Climb,
        };
        let pieces = raster_lines(&r, 2.0, 0.0, TOL, false, 100)
            .unwrap()
            .into_iter()
            .map(|l| (l, strip(&r, &l, &settings)))
            .collect();
        (r, pieces)
    }

    /// Round a circle every straight link between line ends against it
    /// enters the island, so with no edge to follow (no wall pass) each
    /// such end is a dead end, nearest-first cannot lift less than scan
    /// order, and scan order is kept.
    #[test]
    fn scan_order_is_kept_round_a_circle() {
        let (_, pieces) = circle_pieces();
        assert_eq!(nearest_first(pieces.clone(), &RingEdges::default()), pieces);
        let edges = RingEdges::of(&circle_pieces().0, WallRings::Omitted, TOL);
        assert_eq!(nearest_first(pieces.clone(), &edges), pieces);
    }

    /// #202: with the island's ring to follow, the line ends against the
    /// circle link along it: every edge link starts where the piece before
    /// ends and ends where its piece starts, keeps to the ring (radius 5)
    /// within the flattening, and is no longer than the limit along it.
    #[test]
    fn pieces_round_a_circle_link_along_its_edge() {
        let (r, pieces) = circle_pieces();
        let edges = RingEdges::of(&r, WallRings::Last, TOL);
        let ordered = order_nearest_first(pieces.clone(), None, 4.0, &edges);
        assert_eq!(ordered.len(), pieces.len());
        let mut links = 0;
        for pair in ordered.windows(2) {
            let (((before, _), _), ((piece, _), edge)) = (&pair[0], &pair[1]);
            let Some(edge) = edge else { continue };
            links += 1;
            let v = edge.vertices();
            assert!(v[0].point().distance_to(before.end) <= EPS_MM, "{edge:?}");
            assert!(v[v.len() - 1].point().distance_to(piece.start) <= EPS_MM);
            assert!(edge.length_mm() <= 4.0 + EPS_MM, "{edge:?}");
            for p in edge.with_arcs_flattened(TOL).vertices() {
                let d = p.point().distance_to(Point::new(20.0, 10.0));
                assert!(
                    (5.0 - TOL - EPS_MM..=5.0 + EPS_MM).contains(&d),
                    "{p:?} at {d}"
                );
            }
        }
        assert!(links >= 4, "{ordered:?}");
        assert_eq!(order_nearest_first(pieces, None, 4.0, &edges), ordered);
    }

    #[test]
    fn too_many_lines_is_none() {
        assert!(raster_lines(&rect(20.0, 10.0), 0.001, 0.0, TOL, false, 100).is_none());
    }
}
