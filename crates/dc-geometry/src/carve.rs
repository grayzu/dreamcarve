// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Region constructions a carve and the safety pass both need
//! (`docs/specs/phase-3/design.md`, "dc-cam" → the two region
//! constructions; security review M3): they live here so `dc-gcode`
//! re-derives material without calling a generator.

use crate::boolean::{difference, difference_within, intersection, union};
use crate::contour::Contour;
use crate::distance::segment_distance_mm;
use crate::offset::offset;
use crate::point::{EPS_MM, Point, Rect};
use crate::polyline::{Polyline, Segment, Vertex};
use crate::region::{Region, nest};

/// The slope, `tan(α/2)`, of the steepest V-bit whose walls are drawn
/// within the whole flatten tolerance ([`wall_tolerance_mm`]; #478): the
/// 60° bit's, the inlay test part's and most of the corpus's: `tan 30°`
/// rounded down, so that bit's own slope, however computed, is not below
/// it.
const WALL_REFERENCE_SLOPE: f64 = 0.577_350_269_189_625;

/// How near, mm, a carve with a V-bit of slope `slope` (`tan(α/2)`) draws
/// the arcs of the walls it cuts (#478): the flatten tolerance
/// `tolerance_mm`, scaled down for a bit steeper than the 60° bit by its
/// slope over that one's. An arc drawn as lines a distance `e` inside its
/// wall stops the cut `e` short of it, and the cone turns that into height
/// at `1 / slope`: the 15° bit's flank rises 7.6 mm per mm, so the
/// flatten tolerance's 0.01 mm held a star's inlay plug 0.140 mm proud
/// where the 60° bit's held it 0.031 mm. Scaled, a wall stands no further
/// off its true surface, measured up the flank, than the 60° bit's
/// (`tolerance_mm / tan 30°`) whatever the bit, and a 60° or wider bit's
/// walls are drawn as before. `slope` is positive and finite.
#[must_use]
pub fn wall_tolerance_mm(tolerance_mm: f64, slope: f64) -> f64 {
    tolerance_mm * (slope / WALL_REFERENCE_SLOPE).min(1.0)
}

/// A carve's floor: each of `regions` inset by `inset_mm` (for a V-bit
/// carve with a flat depth `F`, `F · tan(α/2)`, the groove's half-width at
/// the floor). Each outer is offset inward and each hole outward by
/// `inset_mm`, with round joins, and the grown holes cut from the shrunk
/// outers. A region can split into several floors or leave none; the
/// result is empty when nothing is left. Outers come back counter-clockwise
/// and holes clockwise (mm, Y-up), in the order of `regions`.
///
/// A hole shaped like a C whose mouth is narrower than `2·inset_mm` grows
/// over its mouth; the bay inside it is a floor of its own, the points of
/// the bay more than `inset_mm` from the hole, after the floors round the
/// holes (#489).
///
/// When the grown holes lie apart inside the shrunk outer, the floors are
/// built from the offsets directly and keep their arcs; when they touch or
/// cross, the holes are cut with [`crate::difference`], which flattens
/// arcs to chords within `tolerance_mm`. The same construction as a
/// Pocket's ring inset, so the safety pass and the generators agree.
#[must_use]
pub fn floor_regions(regions: &[Region], inset_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    regions
        .iter()
        .flat_map(|r| inset_region(r, inset_mm, tolerance_mm, Bays::Floor))
        .collect()
}

/// A male inlay's band (`docs/specs/phase-3/design.md`, "Algorithms →
/// Inlay" step 4; #448): the design's outers grown by `margin_mm` and
/// closed by `rounding_mm`, `B = ((outers) ⊕ (M + R)) ⊖ R`, with round
/// joins. Every region's outer is offset outward by `M + R`, the offsets
/// are united, and the union is inset by `R` ([`floor_regions`]: its
/// outers shrunk, its holes grown). Inward curves of the plain band
/// tighter than `R` become arcs of radius `R`, and notches and gaps
/// narrower than about `2R` fill; nothing reaches farther than the design
/// grown by `M + R`, and no more than the plain band where it is convex.
/// With `rounding_mm` 0 it is the union of the margin offsets exactly.
/// Holes of the design are never offset. The band has no holes
/// (#484): board the margins ring round is band too.
///
/// The union is [`crate::union`], so the result is polygonal: arcs are
/// flattened to chords within `tolerance_mm`. Outers come back
/// counter-clockwise and holes clockwise (mm, Y-up, ADR-0001), in a fixed
/// order, so the same input gives the same regions. Empty when `regions`
/// is empty, `margin_mm` is not a positive finite number, or
/// `rounding_mm` is not 0 or a positive finite number (the caller
/// validates both).
#[must_use]
pub fn rounded_band(
    regions: &[Region],
    margin_mm: f64,
    rounding_mm: f64,
    tolerance_mm: f64,
) -> Vec<Region> {
    if !(usable(margin_mm) && (rounding_mm == 0.0 || usable(rounding_mm))) {
        return Vec::new();
    }
    let united = filled(union(
        &grown_outers(regions, margin_mm + rounding_mm, Bays::Island),
        &[],
        tolerance_mm,
    ));
    if rounding_mm == 0.0 {
        return united;
    }
    floor_regions(&united, rounding_mm, tolerance_mm)
}

/// `regions` with their holes filled: each outer alone (#484). A hole of
/// the margins' union is board the design's parts ring round, farther than
/// the margin from all of them; no part lies in it, since each part's own
/// margin covers it. Left out of the band, it stood to the stock top
/// inside the band's floor, and held the plug off the female's surface.
fn filled(regions: Vec<Region>) -> Vec<Region> {
    if regions.iter().all(|r| r.holes().is_empty()) {
        return regions;
    }
    regions
        .into_iter()
        .filter_map(|r| single_region(r.outer().clone()))
        .collect()
}

/// A male inlay's surround (`docs/specs/phase-3/design.md`, "Algorithms →
/// Inlay" step 4): the area of its band ([`rounded_band`], `margin_mm`
/// beyond the design, closed by `rounding_mm`; #448) outside the design,
/// which the male carve removes around its plug. `regions` themselves are
/// subtracted from the band. Subtracting a region removes its outer and
/// gives back its holes, so a hole in the design (a circle inside a star)
/// comes back as a surround region of its own, with walls all round;
/// holes are never offset. Overlapping margins merge into one region.
///
/// With `rounding_mm` 0 the band is the margin offsets themselves, which
/// [`crate::difference`] fills non-zero, so they count as their union:
/// exactly the plain surround. The subtraction flattens arcs to chords
/// within `tolerance_mm`, and the area is off the exact one by at most
/// `tolerance_mm` times the arc length flattened. Outers come back
/// counter-clockwise and holes clockwise (mm, Y-up, ADR-0001), in
/// [`crate::boolean()`]'s canonical order, so the same input gives the
/// same regions. Empty as [`rounded_band`] is.
#[must_use]
pub fn surround(
    regions: &[Region],
    margin_mm: f64,
    rounding_mm: f64,
    tolerance_mm: f64,
) -> Vec<Region> {
    surround_within(regions, margin_mm, rounding_mm, tolerance_mm, tolerance_mm)
}

/// [`surround`] with the design's outlines, which are the plug's,
/// flattened within `design_tolerance_mm`, and the band's edge within
/// `tolerance_mm` (#478). A V-bit carve's walls stand `1 / tan(α/2)`
/// higher off true per mm they lie off it, so a steep bit wants the
/// plug's outline nearer its arcs; the band's edge is not the plug's and
/// keeps the coarser tolerance, so the floor's rings that step in from it
/// keep their few vertices. Empty as [`surround`] is.
#[must_use]
pub fn surround_within(
    regions: &[Region],
    margin_mm: f64,
    rounding_mm: f64,
    tolerance_mm: f64,
    design_tolerance_mm: f64,
) -> Vec<Region> {
    if !(usable(margin_mm) && (rounding_mm == 0.0 || usable(rounding_mm))) {
        return Vec::new();
    }
    let band = if rounding_mm == 0.0 {
        let mut grown = grown_outers(regions, margin_mm, Bays::Island);
        // #484: the board the margins ring round is band too
        // ([`filled`]), added as regions of its own, which the difference
        // fills with the rest.
        let rings: Vec<Region> = union(&grown, &[], tolerance_mm)
            .iter()
            .flat_map(|r| r.holes().iter().map(Contour::reversed))
            .filter_map(single_region)
            .collect();
        grown.extend(rings);
        grown
    } else {
        rounded_band(regions, margin_mm, rounding_mm, tolerance_mm)
    };
    difference_within(&band, regions, tolerance_mm, design_tolerance_mm)
}

/// The floor of a male inlay's surround, flat `inset_mm` (`P·tan(α/2)`)
/// below its start plane (`docs/specs/phase-3/design.md`, "Algorithms →
/// Inlay", step 4; #448). With a V-walled outer edge (`open_edge` false)
/// it is [`floor_regions`] of `surround`: every wall inset. With an open
/// edge, which an end mill cuts upright, a band region (one outside every
/// outer of `design`, the mirrored design nested) keeps its outer edge
/// where it is and loses only the design's outers grown by `inset_mm`
/// (the plug's foot), cut with [`crate::difference`] within
/// `tolerance_mm`; a region inside a design outer (a design hole) has
/// walls all round and is inset as with a V wall. Outers come back
/// counter-clockwise and holes clockwise, region by region in the order
/// of `surround`.
#[must_use]
pub fn surround_floor_regions(
    surround: &[Region],
    design: &[Region],
    inset_mm: f64,
    open_edge: bool,
    tolerance_mm: f64,
) -> Vec<Region> {
    if !open_edge {
        return floor_regions(surround, inset_mm, tolerance_mm);
    }
    let foot = grown_outers(design, inset_mm, Bays::Floor);
    surround
        .iter()
        .flat_map(|region| {
            if is_design_hole(region, design) {
                inset_region(region, inset_mm, tolerance_mm, Bays::Floor)
            } else {
                difference(std::slice::from_ref(region), &foot, tolerance_mm)
            }
        })
        .collect()
}

/// Whether surround region `region` lies inside an outer of `design`: a
/// hole of the design, not the band round it. Its outer's first vertex is
/// tested, which for a hole region lies on the design hole's outline,
/// inside its part's outer.
#[must_use]
pub fn is_design_hole(region: &Region, design: &[Region]) -> bool {
    let Some(probe) = region.outer().polyline().vertices().first() else {
        return false;
    };
    design.iter().any(|d| {
        single_region(d.outer().clone()).is_some_and(|outer| outer.contains_point(probe.point()))
    })
}

/// The outer outlines of `regions` inset by `inset_mm`: a male inlay's
/// cut-out path (`docs/specs/phase-3/design.md`, "Algorithms → Inlay",
/// step 6; #443, #448, #461), its band's edge `B` ([`rounded_band`]) inset
/// by the cut-out's radius `r` with an open edge, or by `P·tan(α/2) + r`
/// with a V wall, which the generator cuts and the safety pass holds it
/// to. The regions are inset as [`floor_regions`] insets them (outers
/// shrunk, holes grown, with round joins), so a stretch where a hole of
/// `regions` comes within `2·inset_mm` of the outer is skirted, and every
/// outline comes back at least `inset_mm` inside `regions`. Only the
/// outers of the result come back, counter-clockwise (mm, Y-up), in its
/// order: a hole of `B` (board no carve reached, ringed by the design) is
/// not cut round. Empty when nothing is left, or when `inset_mm` is not
/// a positive finite number.
#[must_use]
pub fn inset_outers(regions: &[Region], inset_mm: f64, tolerance_mm: f64) -> Vec<Contour> {
    if !usable(inset_mm) {
        return Vec::new();
    }
    regions
        .iter()
        .flat_map(|r| inset_region(r, inset_mm, tolerance_mm, Bays::Island))
        .map(|r| r.outer().clone())
        .collect()
}

/// A male inlay's band on the board (`docs/specs/phase-3/design.md`,
/// "Algorithms → Inlay", step 4; #495): `band` ([`rounded_band`]) clipped
/// to `board`, the stock's rectangle or that rectangle inset, with
/// [`crate::intersection`] (polygonal within `tolerance_mm`, as the band
/// is). `None` when every outline of `band` lies on `board` within
/// [`EPS_MM`]: the band is then used as it is, so a band
/// that stays on the board is unchanged.
#[must_use]
pub fn band_on_board(band: &[Region], board: Rect, tolerance_mm: f64) -> Option<Vec<Region>> {
    let on_board = band
        .iter()
        .filter_map(|r| r.outer().polyline().bounds())
        .all(|b| {
            b.min.x >= board.min.x - EPS_MM
                && b.min.y >= board.min.y - EPS_MM
                && b.max.x <= board.max.x + EPS_MM
                && b.max.y <= board.max.y + EPS_MM
        });
    if on_board {
        return None;
    }
    let rect = Polyline::rect(board.min, board.width_mm(), board.height_mm());
    let clip: Vec<Region> = Contour::new(rect)
        .ok()
        .and_then(single_region)
        .into_iter()
        .collect();
    Some(intersection(band, &clip, tolerance_mm))
}

/// Each of `outlines` (closed) less its straight stretches that run along
/// a side of `rect`, within `tolerance_mm` (#495): a male inlay's cut-out
/// path on a band clipped to the board ([`band_on_board`]), inset by the
/// cut-out's radius, runs along the board's edge inset by the same
/// radius where the band was clipped; the board's own edge frees the plug
/// there, so those stretches are dropped. An outline with no such stretch
/// comes back whole and closed; one with some comes back as the open
/// stretches between them, in its order, each from one side of `rect` to
/// a side of `rect`, its arcs kept. An outline that runs only along
/// `rect` gives nothing.
#[must_use]
pub fn without_rect_sides(outlines: &[Contour], rect: Rect, tolerance_mm: f64) -> Vec<Polyline> {
    let on = |a: f64, b: f64| (a - b).abs() <= tolerance_mm;
    let along = |s: &Segment| {
        s.is_line()
            && ((on(s.start.x, rect.min.x) && on(s.end.x, rect.min.x))
                || (on(s.start.x, rect.max.x) && on(s.end.x, rect.max.x))
                || (on(s.start.y, rect.min.y) && on(s.end.y, rect.min.y))
                || (on(s.start.y, rect.max.y) && on(s.end.y, rect.max.y)))
    };
    let mut out = Vec::new();
    for outline in outlines {
        let polyline = outline.polyline();
        let vertices = polyline.vertices();
        let dropped: Vec<bool> = polyline.segments().map(|s| along(&s)).collect();
        let Some(first) = dropped.iter().position(|&d| d) else {
            out.push(polyline.clone());
            continue;
        };
        // Walk once round from just after a dropped stretch, gathering the
        // kept segments into open paths.
        let n = vertices.len();
        let mut current: Vec<Vertex> = Vec::new();
        for k in 1..=n {
            let i = (first + k) % n;
            let (Some(&vertex), Some(&drop)) = (vertices.get(i), dropped.get(i)) else {
                continue;
            };
            if drop {
                // An open path ends at this vertex, where the dropped
                // stretch starts.
                if !current.is_empty() {
                    current.push(Vertex::line(vertex.x, vertex.y));
                    out.push(Polyline::new(std::mem::take(&mut current), false));
                }
            } else {
                current.push(vertex);
            }
        }
    }
    out
}

/// Whether `mm` is a positive finite length.
fn usable(mm: f64) -> bool {
    mm.is_finite() && mm > 0.0
}

/// Every region's outer offset outward by `distance_mm` with round joins,
/// as the regions it encloses, a bay it closes as `bays` says
/// ([`grown_contour`]); they may overlap.
fn grown_outers(regions: &[Region], distance_mm: f64, bays: Bays) -> Vec<Region> {
    regions
        .iter()
        .flat_map(|r| grown_contour(r.outer(), distance_mm, bays))
        .collect()
}

/// What a contour grown outward ([`grown_contour`]) makes of a bay it
/// closes (#489): a contour shaped like a C whose mouth is narrower than
/// twice the distance grows over the mouth, and its offset comes back as
/// two loops, its outline and an inner one round the bay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Bays {
    /// The bay is a hole of the grown contour: the points in it farther
    /// than the distance from the contour, which a floor keeps.
    Floor,
    /// The bay is filled, as if the inner loop enclosed the contour too: a
    /// cut-out path ([`inset_outers`]) goes round a region's edge, not
    /// round a bay one of its holes closes, and a male's band
    /// ([`rounded_band`], [`surround`]) covers a bay its design's outer
    /// closes, so the male carves it.
    Island,
}

/// [`floor_regions`] of one region, its holes' bays as `bays` says.
fn inset_region(region: &Region, inset_mm: f64, tolerance_mm: f64, bays: Bays) -> Vec<Region> {
    let outers = offset(region.outer(), -inset_mm);
    if outers.is_empty() {
        return Vec::new();
    }
    let grown: Vec<Region> = region
        .holes()
        .iter()
        .flat_map(|hole| grown_contour(hole, inset_mm, bays))
        .collect();
    if grown.is_empty() {
        return outers.into_iter().filter_map(single_region).collect();
    }
    let hulls: Vec<Contour> = grown.iter().map(|r| r.outer().clone()).collect();
    // Counter-clockwise, as `nest` gives back a region's outer.
    let bay_loops: Vec<Contour> = grown
        .iter()
        .flat_map(|r| r.holes().iter().map(Contour::reversed))
        .collect();
    let mut all = outers.clone();
    all.extend(hulls.iter().cloned());
    all.extend(bay_loops.iter().cloned());
    if let Some(Ok((nested, warnings))) = holes_stay_apart(region, inset_mm).then(|| nest(all)) {
        let holes: usize = nested.iter().map(|r| r.holes().len()).sum();
        // A grown hole that encloses the shrunk outer nests as the outer
        // of an inverted annulus; only the shrunk outers may be outers, and
        // the bays the grown holes enclose regions inside them.
        let clean = warnings.is_empty()
            && nested.len() == outers.len() + bay_loops.len()
            && holes == hulls.len()
            && nested.iter().all(|r| match r.depth() {
                0 => outers.contains(r.outer()),
                2 => bay_loops.contains(r.outer()),
                _ => false,
            });
        if clean {
            return nested;
        }
    }
    let subject: Vec<Region> = outers.into_iter().filter_map(single_region).collect();
    difference(&subject, &grown, tolerance_mm)
}

/// The area `contour` (an outer or a hole) encloses, grown by
/// `distance_mm` with round joins, as regions. A contour that grows over a
/// bay's mouth comes back as one region whose holes are its bays with
/// [`Bays::Floor`] (#489: before, each loop was a region of its own, so a
/// floor lost the bay with the island round it), and as one region per
/// loop with [`Bays::Island`]. `offset` gives every loop the contour's
/// winding, so they are told apart by containment.
pub(crate) fn grown_contour(contour: &Contour, distance_mm: f64, bays: Bays) -> Vec<Region> {
    let loops = offset(contour, distance_mm);
    if bays == Bays::Floor
        && let Ok((regions, _)) = nest(loops.clone())
    {
        return regions;
    }
    loops.into_iter().filter_map(single_region).collect()
}

/// Whether `region`'s holes, grown by `inset_mm`, stay apart from each
/// other and from its outer shrunk by `inset_mm`: no hole comes within
/// `2·inset_mm` of another or of the outer. Offsets with round joins are
/// the points within `inset_mm` of a line, so two grown holes share area
/// exactly where their holes come that near. [`nest`] judges the same by
/// the area the offsets share, which `cavalier_contours` misses where two
/// grown holes' round corners nearly coincide and their straight sides
/// run on one line (#464: two holes 0.001 mm apart, each grown by more
/// than the gap, came back as two holes overlapping by 2.3 mm, and the
/// floor between them was cut inside the design).
fn holes_stay_apart(region: &Region, inset_mm: f64) -> bool {
    let reach_mm = 2.0 * inset_mm;
    let outer = region.outer().polyline();
    let holes: Vec<&Polyline> = region.holes().iter().map(Contour::polyline).collect();
    holes.iter().enumerate().all(|(i, hole)| {
        !closer_than(hole, outer, reach_mm)
            && holes
                .iter()
                .take(i)
                .all(|other| !closer_than(hole, other, reach_mm))
    })
}

/// Whether `a` comes within `reach_mm` of `b` anywhere, following arcs.
/// Only segments whose boxes come that near are measured.
pub(crate) fn closer_than(a: &Polyline, b: &Polyline, reach_mm: f64) -> bool {
    let near = |p: &Rect, q: &Rect| {
        p.min.x - reach_mm <= q.max.x
            && q.min.x - reach_mm <= p.max.x
            && p.min.y - reach_mm <= q.max.y
            && q.min.y - reach_mm <= p.max.y
    };
    let Some(a_box) = a.bounds() else {
        return false;
    };
    let candidates: Vec<(Segment, Rect)> = b
        .segments()
        .map(|t| (t, segment_box(t)))
        .filter(|(_, t_box)| near(&a_box, t_box))
        .collect();
    a.segments().any(|s| {
        let s_box = segment_box(s);
        candidates
            .iter()
            .any(|(t, t_box)| near(&s_box, t_box) && segment_distance_mm(s, *t) < reach_mm)
    })
}

/// A box holding `segment`: its ends' for a line, its whole circle's for
/// an arc.
pub(crate) fn segment_box(segment: Segment) -> Rect {
    match segment.arc() {
        Some(arc) => {
            let (c, r) = (arc.centre, arc.radius_mm);
            Rect {
                min: Point::new(c.x - r, c.y - r),
                max: Point::new(c.x + r, c.y + r),
            }
        }
        None => Rect::from_point(segment.start).including(segment.end),
    }
}

/// The region `contour` alone encloses.
pub(crate) fn single_region(contour: Contour) -> Option<Region> {
    nest(vec![contour])
        .ok()
        .and_then(|(regions, _)| regions.into_iter().next())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contour::Contour;
    use crate::point::Point;
    use crate::polyline::{Polyline, Vertex};
    use crate::region::{area_mm2, nest};
    use std::f64::consts::PI;

    const TOL: f64 = crate::flatten::FLATTEN_TOLERANCE_DEFAULT_MM;

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Contour {
        Contour::new(Polyline::rect(Point::new(x, y), w, h)).expect("a rectangle")
    }

    /// A 40 × 20 mm plate with a 10 × 5 mm hole, inset 2 mm: the outer
    /// shrinks to 36 × 16 (its convex corners stay sharp), the hole grows
    /// to 14 × 9 with 2 mm round corners, so the floor's area is
    /// 576 − (50 + 2·2·(10 + 5) + 4π) mm², and it keeps one hole. A
    /// second, separate square becomes a floor of its own.
    #[test]
    fn floor_regions_inset_outers_and_grow_holes() {
        let (plate, _) = nest(vec![rect(0.0, 0.0, 40.0, 20.0), rect(15.0, 7.5, 10.0, 5.0)])
            .expect("plate nests");
        let (square, _) = nest(vec![rect(60.0, 0.0, 10.0, 10.0)]).expect("square nests");
        let regions = [plate, square].concat();

        let floor = floor_regions(&regions, 2.0, TOL);
        assert_eq!(floor.len(), 2, "one floor per region: {floor:?}");
        assert_eq!(floor[0].holes().len(), 1, "the grown hole stays a hole");
        let bounds = floor[0].outer().polyline().bounds().expect("bounds");
        assert!((bounds.min.x - 2.0).abs() < 1e-9 && (bounds.max.x - 38.0).abs() < 1e-9);
        assert!((bounds.min.y - 2.0).abs() < 1e-9 && (bounds.max.y - 18.0).abs() < 1e-9);
        let hole = floor[0].holes()[0]
            .polyline()
            .bounds()
            .expect("hole bounds");
        assert!((hole.min.x - 13.0).abs() < 1e-9 && (hole.max.x - 27.0).abs() < 1e-9);
        assert!((hole.min.y - 5.5).abs() < 1e-9 && (hole.max.y - 14.5).abs() < 1e-9);
        let expected = 36.0 * 16.0 - (50.0 + 4.0 * 15.0 + 4.0 * PI);
        assert!(
            (floor[0].area_mm2() - expected).abs() < 1e-6,
            "{}",
            floor[0].area_mm2()
        );
        assert!(
            (floor[1].area_mm2() - 36.0).abs() < 1e-9,
            "the square's floor is 6 × 6"
        );
        assert!(
            floor.iter().all(|r| r.outer().is_ccw()),
            "outers stay counter-clockwise"
        );
    }

    /// A 20 × 4 mm bar inset 2 mm or more leaves no floor, inset 1 mm an
    /// 18 × 2 one. A square ring with 3 mm walls keeps only its four corner
    /// pockets at an inset of 1.6 mm (the widest disc there has radius
    /// 3·(2 − √2) ≈ 1.757 mm, as in the medial ring test), and nothing at
    /// 2 mm, where the grown hole covers the shrunk outer.
    #[test]
    fn floor_regions_vanish_when_the_shape_is_too_narrow() {
        let (bar, _) = nest(vec![rect(0.0, 0.0, 20.0, 4.0)]).expect("bar nests");
        assert!(floor_regions(&bar, 2.0, TOL).is_empty());
        assert!(floor_regions(&bar, 2.5, TOL).is_empty());
        assert!(
            (area_mm2(&floor_regions(&bar, 1.0, TOL)) - 36.0).abs() < 1e-9,
            "1 mm leaves an 18 × 2 floor"
        );

        let (ring, _) =
            nest(vec![rect(0.0, 0.0, 20.0, 20.0), rect(3.0, 3.0, 14.0, 14.0)]).expect("ring nests");
        assert_eq!(
            floor_regions(&ring, 1.6, TOL).len(),
            4,
            "the corner pockets"
        );
        assert!(
            floor_regions(&ring, 2.0, TOL).is_empty(),
            "3 mm walls inset 2 mm from both sides"
        );
    }

    /// A four-point star, 60 mm tip to tip, centred at the origin: tips on
    /// the axes 30 mm out, inner vertices on the diagonals 12 mm out.
    fn star() -> Contour {
        let inner = 12.0 * std::f64::consts::FRAC_1_SQRT_2;
        let points = [
            (30.0, 0.0),
            (inner, inner),
            (0.0, 30.0),
            (-inner, inner),
            (-30.0, 0.0),
            (-inner, -inner),
            (0.0, -30.0),
            (inner, -inner),
        ];
        let vertices = points.iter().map(|&(x, y)| Vertex::line(x, y)).collect();
        Contour::new(Polyline::new(vertices, true)).expect("a star")
    }

    fn circle(centre: Point, radius_mm: f64) -> Contour {
        Contour::new(Polyline::circle(centre, radius_mm)).expect("a circle")
    }

    /// The star of the inlay test part with a 12 mm circle in its middle,
    /// margin 8 mm: the surround is the 8 mm band round the star (one
    /// region whose hole is the star) and the circle (a region of its own,
    /// carved on the male so the female's island has a socket).
    #[test]
    fn surround_of_a_star_with_a_hole_has_the_hole_as_a_region() {
        let (design, _) =
            nest(vec![star(), circle(Point::new(0.0, 0.0), 6.0)]).expect("star nests");
        assert_eq!(design.len(), 1, "a star with a hole");

        let s = surround(&design, 8.0, 0.0, TOL);
        assert_eq!(s.len(), 2, "the band and the circle: {s:?}");
        let band = s
            .iter()
            .find(|r| r.holes().len() == 1)
            .expect("the band keeps the star as its hole");
        let disc = s
            .iter()
            .find(|r| r.holes().is_empty())
            .expect("the circle is a region of its own");

        let outer = band.outer().polyline().bounds().expect("band bounds");
        for v in [outer.max.x, outer.max.y, -outer.min.x, -outer.min.y] {
            assert!((v - 38.0).abs() < TOL, "tips grow by the margin: {outer:?}");
        }
        let hole = band.holes()[0].polyline().bounds().expect("hole bounds");
        for v in [hole.max.x, hole.max.y, -hole.min.x, -hole.min.y] {
            assert!((v - 30.0).abs() < 1e-9, "the hole is the star: {hole:?}");
        }
        let d = disc.outer().polyline().bounds().expect("disc bounds");
        for v in [d.max.x, d.max.y, -d.min.x, -d.min.y] {
            assert!((v - 6.0).abs() < TOL, "the circle, 6 mm radius: {d:?}");
        }
        assert!(
            (disc.area_mm2() - 36.0 * PI).abs() < TOL * 12.0 * PI,
            "{}",
            disc.area_mm2()
        );
        assert!(
            s.iter()
                .all(|r| r.outer().is_ccw() && r.holes().iter().all(|h| !h.is_ccw())),
            "outers counter-clockwise, holes clockwise"
        );
    }

    /// Two 10 mm squares 6 mm apart with a 4 mm margin: their margins
    /// overlap and merge into one surround with two holes. A third square
    /// far off gets a surround of its own.
    #[test]
    fn surround_of_disjoint_shapes_merges_overlapping_margins() {
        let (design, _) = nest(vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(16.0, 0.0, 10.0, 10.0),
            rect(100.0, 0.0, 10.0, 10.0),
        ])
        .expect("squares nest");
        assert_eq!(design.len(), 3);

        let s = surround(&design, 4.0, 0.0, TOL);
        assert_eq!(s.len(), 2, "the merged pair and the lone square: {s:?}");
        let mut holes: Vec<usize> = s.iter().map(|r| r.holes().len()).collect();
        holes.sort_unstable();
        assert_eq!(holes, [1, 2]);
        let pair = s
            .iter()
            .find(|r| r.holes().len() == 2)
            .expect("the merged pair");
        let b = pair.outer().polyline().bounds().expect("bounds");
        assert!((b.min.x + 4.0).abs() < TOL && (b.max.x - 30.0).abs() < TOL);
        assert!((b.min.y + 4.0).abs() < TOL && (b.max.y - 14.0).abs() < TOL);
        assert_eq!(
            surround(&design, 4.0, 0.0, TOL),
            s,
            "the same input gives the same regions"
        );
    }

    /// #461: the star's 8 mm band (with its circle, a hole of the design,
    /// which the band does not offset) inset by 2 mm is one outline,
    /// counter-clockwise, every vertex 2 mm from the band's edge within
    /// the tolerance and inside it, its tips 36 mm out. A band with a hole
    /// of its own (a 40 mm square less a 20 mm one) comes back as its
    /// outer alone, 3 mm in. Where its wall is 4 mm wide, inset by 3 mm,
    /// the grown hole crosses the shrunk outer, and the outlines skirt the
    /// hole, at least 3 mm from it. A zero or NaN inset is empty.
    #[test]
    fn inset_outers_runs_inside_the_band_edge() {
        let (design, _) =
            nest(vec![star(), circle(Point::new(0.0, 0.0), 6.0)]).expect("star nests");
        let band = rounded_band(&design, 8.0, 0.0, TOL);
        let path = inset_outers(&band, 2.0, TOL);
        assert_eq!(path.len(), 1, "{path:?}");
        assert!(path[0].is_ccw());
        let edge: Vec<Polyline> = band
            .iter()
            .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
            .map(|c| c.polyline().clone())
            .collect();
        let gap_mm = |p: Point| {
            edge.iter()
                .filter_map(|o| crate::nearest_point(o, p))
                .map(|n| n.gap_mm)
                .fold(f64::INFINITY, f64::min)
        };
        for v in path[0].polyline().vertices() {
            assert!(holds(&band, v.point(), 0.0), "{v:?} is outside the band");
            let gap = gap_mm(v.point());
            assert!((gap - 2.0).abs() <= TOL, "{v:?} is {gap} mm from the edge");
        }
        let b = path[0].polyline().bounds().expect("bounds");
        for v in [b.max.x, b.max.y, -b.min.x, -b.min.y] {
            assert!((v - 36.0).abs() < TOL, "tips 36 mm out: {b:?}");
        }

        let (ring, _) = nest(vec![
            rect(0.0, 0.0, 40.0, 40.0),
            rect(10.0, 10.0, 20.0, 20.0),
        ])
        .expect("nests");
        assert_eq!(ring.len(), 1, "{ring:?}");
        assert_eq!(ring[0].holes().len(), 1);
        let path = inset_outers(&ring, 3.0, TOL);
        assert_eq!(path.len(), 1, "the outer alone: {path:?}");
        let b = path[0].polyline().bounds().expect("bounds");
        assert!((b.min.x - 3.0).abs() < 1e-9 && (b.max.x - 37.0).abs() < 1e-9);

        // A wall 4 mm wide at the bottom: inset by 3 mm, the hole grown
        // by 3 crosses the outer shrunk by 3, and the path goes round.
        let (thin, _) = nest(vec![
            rect(0.0, 0.0, 40.0, 40.0),
            rect(10.0, 4.0, 20.0, 26.0),
        ])
        .expect("nests");
        let path = inset_outers(&thin, 3.0, TOL);
        assert!(!path.is_empty());
        let hole = thin[0].holes()[0].polyline().clone();
        for c in &path {
            for v in c.polyline().vertices() {
                let to_hole = crate::nearest_point(&hole, v.point()).expect("near").gap_mm;
                assert!(to_hole >= 3.0 - TOL, "{v:?} is {to_hole} mm from the hole");
            }
        }

        assert!(inset_outers(&band, 0.0, TOL).is_empty());
        assert!(inset_outers(&band, f64::NAN, TOL).is_empty());
    }

    /// The surround's area is the margin offset's area minus the design's:
    /// a 40 × 20 mm plate with a 10 × 5 mm hole and a separate 5 mm circle,
    /// margin 3 mm. The plate's offset covers 800 + 2·3·(40 + 20) + 9π mm²
    /// and the circle's 64π; the design covers 750 + 25π, so its hole is
    /// back in the surround. Flattening moves the area by at most the
    /// tolerance times the length of arc flattened.
    #[test]
    fn surround_area_is_the_offset_minus_the_design() {
        let (design, _) = nest(vec![
            rect(0.0, 0.0, 40.0, 20.0),
            rect(15.0, 7.5, 10.0, 5.0),
            circle(Point::new(80.0, 10.0), 5.0),
        ])
        .expect("design nests");
        let margin = 3.0;

        let s = surround(&design, margin, 0.0, TOL);
        let offset_area = 800.0 + 2.0 * margin * 60.0 + PI * margin * margin + PI * 64.0;
        let design_area = 750.0 + PI * 25.0;
        let arcs_mm = 2.0 * PI * (margin + 8.0 + 5.0);
        let got = area_mm2(&s);
        assert!(
            (got - (offset_area - design_area)).abs() <= TOL * arcs_mm,
            "{got} vs {}",
            offset_area - design_area
        );
        assert!(
            surround(&design, 0.0, 0.0, TOL).is_empty(),
            "no margin, no surround"
        );
    }

    // #448: the rounded band, the open floor and the rounded cut-out.

    /// Whether `p` lies in one of `regions`, or within `slack_mm` of an
    /// outline of one.
    fn holds(regions: &[Region], p: Point, slack_mm: f64) -> bool {
        regions.iter().any(|r| r.contains_point(p))
            || regions.iter().any(|r| {
                std::iter::once(r.outer())
                    .chain(r.holes())
                    .filter_map(|c| crate::nearest_point(c.polyline(), p))
                    .any(|n| n.gap_mm <= slack_mm)
            })
    }

    /// The star with its circle, a square beside it and an L, which has a
    /// reflex corner: the design the rounding tests share.
    fn notched_design() -> Vec<Region> {
        let l = Contour::new(Polyline::new(
            [
                (60.0, -30.0),
                (90.0, -30.0),
                (90.0, -20.0),
                (70.0, -20.0),
                (70.0, 10.0),
                (60.0, 10.0),
            ]
            .iter()
            .map(|&(x, y)| Vertex::line(x, y))
            .collect(),
            true,
        ))
        .expect("an L");
        let (design, _) = nest(vec![
            star(),
            circle(Point::new(0.0, 0.0), 6.0),
            rect(40.0, -5.0, 10.0, 10.0),
            l,
        ])
        .expect("the design nests");
        design
    }

    /// #448: with no rounding the surround is the plain one, the union of
    /// the margin offsets less the design, region for region, and the band
    /// is the union of the offsets.
    #[test]
    fn rounding_zero_is_the_plain_surround() {
        let design = notched_design();
        let grown: Vec<Region> = design
            .iter()
            .flat_map(|r| offset(r.outer(), 8.0))
            .filter_map(single_region)
            .collect();
        assert_eq!(
            surround(&design, 8.0, 0.0, TOL),
            difference(&grown, &design, TOL)
        );
        assert_eq!(
            rounded_band(&design, 8.0, 0.0, TOL),
            union(&grown, &[], TOL)
        );
    }

    /// #484: four 4 × 4 mm parts at the corners of a 24 mm square, margin
    /// 9 mm: the margins overlap side by side and ring round the middle,
    /// 11.3 mm from every part, without covering it. The band, plain
    /// or rounded, and the surround have no hole there: the board the
    /// parts ring round is band too, which the male cuts, so it cannot
    /// stand inside the band's floor and hold the plug up.
    #[test]
    fn band_fills_the_board_its_parts_ring_round() {
        let parts: Vec<Contour> = [(-12.0, -12.0), (8.0, -12.0), (8.0, 8.0), (-12.0, 8.0)]
            .iter()
            .map(|&(x, y)| rect(x, y, 4.0, 4.0))
            .collect();
        let (design, _) = nest(parts).expect("the parts nest");
        let middle = Point::new(0.0, 0.0);
        for rounding_mm in [0.0, 2.0] {
            let band = rounded_band(&design, 9.0, rounding_mm, TOL);
            assert!(
                band.iter().all(|r| r.holes().is_empty()),
                "rounding {rounding_mm}: the band has a hole: {band:?}"
            );
            assert!(
                band.iter().any(|r| r.contains_point(middle)),
                "rounding {rounding_mm}: the band misses the middle"
            );
            let around = surround(&design, 9.0, rounding_mm, TOL);
            assert!(
                around.iter().any(|r| r.contains_point(middle)),
                "rounding {rounding_mm}: the surround misses the middle"
            );
        }
    }

    /// #448: rounding only adds: every outline point of the plain band
    /// lies in the rounded band, whose area is at least the plain one's;
    /// and the rounded band reaches no farther than the design grown by
    /// the margin and the rounding.
    #[test]
    fn rounded_band_contains_the_plain_band() {
        let design = notched_design();
        let plain = rounded_band(&design, 8.0, 0.0, TOL);
        for rounding in [2.0, 5.0, 12.0] {
            let rounded = rounded_band(&design, 8.0, rounding, TOL);
            assert!(!rounded.is_empty());
            for p in plain
                .iter()
                .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
                .flat_map(|c| c.polyline().vertices().iter().map(|v| v.point()))
            {
                assert!(holds(&rounded, p, 2.0 * TOL), "R {rounding}: {p:?}");
            }
            assert!(
                area_mm2(&rounded) >= area_mm2(&plain) - 1e-6,
                "R {rounding}: {} < {}",
                area_mm2(&rounded),
                area_mm2(&plain)
            );
            let reach = rounded_band(&design, 8.0 + rounding, 0.0, TOL);
            for p in rounded
                .iter()
                .flat_map(|r| r.outer().polyline().vertices().iter().map(|v| v.point()))
            {
                assert!(holds(&reach, p, 2.0 * TOL), "R {rounding}: {p:?}");
            }
        }
    }

    /// #448: two 10 mm squares 6 mm apart with a 2 mm margin leave a 2 mm
    /// gap between their bands; the gap's centre is outside the plain
    /// band and inside the band rounded by 5 mm, which fills gaps
    /// narrower than about twice its radius. So is it in the surround.
    #[test]
    fn rounding_fills_a_gap_narrower_than_twice_the_radius() {
        let (design, _) = nest(vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(16.0, 0.0, 10.0, 10.0),
        ])
        .expect("squares nest");
        let centre = Point::new(13.0, 5.0);
        let plain = rounded_band(&design, 2.0, 0.0, TOL);
        assert_eq!(plain.len(), 2, "two bands: {plain:?}");
        assert!(!plain.iter().any(|r| r.contains_point(centre)));
        let rounded = rounded_band(&design, 2.0, 5.0, TOL);
        assert_eq!(rounded.len(), 1, "one band: {rounded:?}");
        assert!(rounded[0].contains_point(centre));
        assert!(
            surround(&design, 2.0, 5.0, TOL)
                .iter()
                .any(|r| r.contains_point(centre))
        );
        assert!(
            !surround(&design, 2.0, 0.0, TOL)
                .iter()
                .any(|r| r.contains_point(centre))
        );
    }

    /// #448: the rounded surround is the band less the design, so the
    /// star's circle, a hole of the design, is still a region of its own
    /// with walls all round, whatever the rounding.
    #[test]
    fn rounding_keeps_design_holes_as_regions() {
        let (design, _) =
            nest(vec![star(), circle(Point::new(0.0, 0.0), 6.0)]).expect("star nests");
        for rounding in [3.0, 12.0] {
            let s = surround(&design, 8.0, rounding, TOL);
            assert_eq!(s.len(), 2, "R {rounding}: the band and the circle: {s:?}");
            let disc = s
                .iter()
                .find(|r| r.holes().is_empty())
                .expect("the circle is a region of its own");
            assert!(
                (disc.area_mm2() - 36.0 * PI).abs() < TOL * 12.0 * PI,
                "{}",
                disc.area_mm2()
            );
            let band = s
                .iter()
                .find(|r| r.holes().len() == 1)
                .expect("the band has the star as its hole");
            let hole = band.holes()[0].polyline().bounds().expect("hole bounds");
            for v in [hole.max.x, hole.max.y, -hole.min.x, -hole.min.y] {
                assert!((v - 30.0).abs() < 1e-6, "the hole is the star: {hole:?}");
            }
        }
    }

    /// #448: the floor of a surround with an open outer edge keeps the
    /// band's outer edge where it is and grows only the design's outers
    /// by the inset; a design hole's floor is inset all round, as with a
    /// V wall. With a V wall it is `floor_regions`.
    #[test]
    fn open_floor_keeps_the_band_edge() {
        let (design, _) =
            nest(vec![star(), circle(Point::new(0.0, 0.0), 6.0)]).expect("star nests");
        let s = surround(&design, 8.0, 0.0, TOL);
        assert_eq!(
            surround_floor_regions(&s, &design, 1.0, false, TOL),
            floor_regions(&s, 1.0, TOL)
        );
        let open = surround_floor_regions(&s, &design, 1.0, true, TOL);
        assert_eq!(open.len(), 2, "{open:?}");
        let ring = open
            .iter()
            .find(|r| !r.holes().is_empty())
            .expect("the band's floor ring");
        let b = ring.outer().polyline().bounds().expect("bounds");
        for v in [b.max.x, b.max.y, -b.min.x, -b.min.y] {
            assert!((v - 38.0).abs() < TOL, "the outer edge stays: {b:?}");
        }
        let foot = ring.holes()[0].polyline().bounds().expect("bounds");
        for v in [foot.max.x, foot.max.y, -foot.min.x, -foot.min.y] {
            assert!((v - 31.0).abs() < TOL, "the star grown by 1 mm: {foot:?}");
        }
        let disc = open
            .iter()
            .find(|r| r.holes().is_empty())
            .expect("the circle's floor");
        assert!((disc.area_mm2() - 25.0 * PI).abs() < TOL * 10.0 * PI);
    }

    /// #448, #461: the cut-out path rounds as the band does: two squares
    /// 6 mm apart with a 2 mm margin, inset by 1 mm, are two outlines;
    /// with the band rounded by 5 mm, which closes the gap, one, across it.
    #[test]
    fn inset_outers_rounds_across_a_gap() {
        let (pair, _) = nest(vec![
            rect(0.0, 0.0, 10.0, 10.0),
            rect(16.0, 0.0, 10.0, 10.0),
        ])
        .expect("squares nest");
        let plain = rounded_band(&pair, 2.0, 0.0, TOL);
        assert_eq!(inset_outers(&plain, 1.0, TOL).len(), 2);
        let rounded = inset_outers(&rounded_band(&pair, 2.0, 5.0, TOL), 1.0, TOL);
        assert_eq!(rounded.len(), 1, "{rounded:?}");
        let (region, _) = nest(rounded).expect("nests");
        assert!(region[0].contains_point(Point::new(13.0, 5.0)));
    }

    /// #464: two holes a hair apart, 0.001 and 0.003 mm, each grown by
    /// more than the gap (2·tan 30°, a 60° bit's floor inset at a 2 mm
    /// flat depth), share area, so the floor keeps one hole, their union
    /// grown: a rounded rectangle `(20 + g + 2i) × (10 + 2i)` with corners
    /// of radius `i`, out of the plate shrunk to `(60 − 2i) × (30 − 2i)`.
    /// The union's arcs are cut as chords within [`TOL`], so the area is
    /// held to 0.1 mm². Before, `nest` judged the grown holes apart
    /// (cavalier's boolean missed their nearly coincident round corners,
    /// whose straight sides run on one line) and the floor kept both, 2.3
    /// mm deep in each other, its area short by their overlap, 27 mm²; the
    /// male inlay of two squares that near cut its floor inside them.
    #[test]
    fn floor_of_holes_a_hair_apart_merges_them() {
        let i = 2.0 * (PI / 6.0).tan();
        for gap in [0.001, 0.003] {
            let (plate, _) = nest(vec![
                rect(20.0, 30.0, 60.0, 30.0),
                rect(40.0 - gap / 2.0, 40.0, 10.0, 10.0),
                rect(50.0 + gap / 2.0, 40.0, 10.0, 10.0),
            ])
            .expect("plate nests");
            let floor = floor_regions(&plate, i, TOL);
            assert_eq!(floor.len(), 1, "gap {gap}: {floor:?}");
            assert_eq!(
                floor[0].holes().len(),
                1,
                "gap {gap}: the grown holes merge"
            );
            let union = (20.0 + gap + 2.0 * i) * (10.0 + 2.0 * i) - (4.0 - PI) * i * i;
            let expected = (60.0 - 2.0 * i) * (30.0 - 2.0 * i) - union;
            assert!(
                (floor[0].area_mm2() - expected).abs() < 0.1,
                "gap {gap}: {} mm², not {expected}",
                floor[0].area_mm2()
            );
            assert!(
                !floor[0].contains_point(Point::new(50.0, 45.0)),
                "gap {gap}: the floor runs between the holes"
            );
        }
    }

    /// #495: a 10 mm square's 8 mm band on a board: on a 40 mm board with
    /// the square in its middle the band stays on it, `None`; with the
    /// square 2 mm from the board's left edge the band runs 6 mm past it
    /// and is clipped there: its left edge on the board's, its area the
    /// band's less the 6 mm strip past the edge (`26 × 6` less the two
    /// corner pieces the band's round corners leave, `(4 − π)·8²/4` each,
    /// within the flattening).
    #[test]
    fn band_on_board_clips_only_what_runs_past() {
        let square = |x: f64| nest(vec![rect(x, 15.0, 10.0, 10.0)]).expect("nests").0;
        let board = Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(40.0, 40.0),
        };
        let centred = rounded_band(&square(15.0), 8.0, 0.0, TOL);
        assert_eq!(band_on_board(&centred, board, TOL), None);

        let near = rounded_band(&square(2.0), 8.0, 0.0, TOL);
        let clipped = band_on_board(&near, board, TOL).expect("clipped");
        let b = clipped[0].outer().polyline().bounds().expect("bounds");
        assert!(b.min.x.abs() < 1e-9, "{b:?}");
        assert!((b.max.x - 20.0).abs() < TOL, "{b:?}");
        let past = 26.0 * 6.0 - 2.0 * (4.0 - PI) * 16.0;
        let arcs_mm = 2.0 * PI * 8.0;
        assert!(
            (area_mm2(&clipped) - (area_mm2(&near) - past)).abs() <= TOL * arcs_mm,
            "{} vs {}",
            area_mm2(&clipped),
            area_mm2(&near) - past
        );
    }

    /// #495: an outline loses its straight stretches along the rectangle's
    /// sides and comes back as the open stretches between them. A 10 mm
    /// square in the rectangle's corner loses its left and bottom sides:
    /// one open path from (10, 0) round to (0, 10). A D whose arc's ends
    /// both lie on the left side keeps the arc, which bulges into the
    /// rectangle, and loses its straight side: one open path, the arc. An
    /// outline that never runs along a side comes back closed.
    #[test]
    fn without_rect_sides_opens_outlines_along_the_board() {
        let board = Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(20.0, 20.0),
        };
        let corner = rect(0.0, 0.0, 10.0, 10.0);
        let paths = without_rect_sides(&[corner], board, TOL);
        assert_eq!(paths.len(), 1, "{paths:?}");
        assert!(!paths[0].is_closed());
        let points: Vec<(f64, f64)> = paths[0].vertices().iter().map(|v| (v.x, v.y)).collect();
        assert_eq!(points, [(10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);

        // Counter-clockwise: up the left side, then the arc back down
        // through (5, 10).
        let d = Contour::new(Polyline::new(
            vec![Vertex::line(0.0, 5.0), Vertex::new(0.0, 15.0, -1.0)],
            true,
        ))
        .expect("a D");
        let paths = without_rect_sides(&[d], board, TOL);
        assert_eq!(paths.len(), 1, "{paths:?}");
        let v = paths[0].vertices();
        assert_eq!(v.len(), 2);
        assert!(!paths[0].is_closed());
        assert!(
            (v[0].bulge.abs() - 1.0).abs() < 1e-12,
            "the arc is kept: {v:?}"
        );

        let inside = rect(5.0, 5.0, 10.0, 10.0);
        let paths = without_rect_sides(std::slice::from_ref(&inside), board, TOL);
        assert_eq!(paths, [inside.polyline().clone()]);
    }

    /// #489: a C-shaped island, a 20 mm square wall 4 mm thick from
    /// (10, 10) with a mouth 1 mm wide in its right side, in a 40 mm square.
    fn closed_c() -> Contour {
        let points = [
            (10.0, 10.0),
            (30.0, 10.0),
            (30.0, 19.5),
            (26.0, 19.5),
            (26.0, 14.0),
            (14.0, 14.0),
            (14.0, 26.0),
            (26.0, 26.0),
            (26.0, 20.5),
            (30.0, 20.5),
            (30.0, 30.0),
            (10.0, 30.0),
        ];
        let vertices = points.iter().map(|&(x, y)| Vertex::line(x, y)).collect();
        Contour::new(Polyline::new(vertices, true)).expect("a C")
    }

    /// #489: inset 2 mm, the C's island grows over its 1 mm mouth, and the
    /// 12 mm bay inside it keeps a floor of its own: the bay shrunk to 8 ×
    /// 8 mm, plus a lens `l = 2∫₀^½ (2 − √(4 − s²)) ds` ≈ 0.021 mm² where
    /// it reaches toward the mouth past the round joins about the mouth's
    /// corners. The floor round the island is the 36 mm square less the
    /// 24 mm one with 2 mm round corners, less the same lens notched in
    /// where the island's outside meets the mouth. Areas within 0.1 mm²
    /// (arcs cut as chords within [`TOL`]). Alone (its grown island apart
    /// from the outer) and with a 4 mm square 1 mm right of the C (their
    /// grown islands merge, so the holes are cut with `difference`), the
    /// bay is a floor. Before, the grown island's inner loop round the bay
    /// was cut away as island too: the floor was 723.5 mm², the bay left
    /// out.
    #[test]
    fn floor_keeps_the_bay_of_a_closed_c() {
        let s: f64 = 0.5;
        let integral = s / 2.0 * (4.0 - s * s).sqrt() + 2.0 * (s / 2.0).asin();
        let lens = 2.0 * (2.0 * s - integral);
        let bay = 64.0 + lens;
        let round = 36.0 * 36.0 - (24.0 * 24.0 - (4.0 - PI) * 4.0 - lens);
        let in_bay = Point::new(20.0, 20.0);
        let (alone, _) = nest(vec![rect(0.0, 0.0, 40.0, 40.0), closed_c()]).expect("nests");
        let floor = floor_regions(&alone, 2.0, TOL);
        assert!(
            (area_mm2(&floor) - (round + bay)).abs() < 0.1,
            "{} mm², not {}",
            area_mm2(&floor),
            round + bay
        );
        let (beside, _) = nest(vec![
            rect(0.0, 0.0, 40.0, 40.0),
            closed_c(),
            rect(31.0, 12.0, 4.0, 4.0),
        ])
        .expect("nests");
        for (case, regions) in [("alone", alone), ("beside a square", beside)] {
            let floor = floor_regions(&regions, 2.0, TOL);
            let pieces: Vec<&Region> = floor.iter().filter(|r| r.contains_point(in_bay)).collect();
            assert_eq!(pieces.len(), 1, "{case}: the bay is a floor: {floor:?}");
            assert!(
                (pieces[0].area_mm2() - bay).abs() < 0.1,
                "{case}: the bay's floor is {} mm², not {bay}",
                pieces[0].area_mm2()
            );
            assert!(
                floor.iter().all(|r| r.outer().is_ccw()),
                "{case}: outers stay counter-clockwise"
            );
        }
    }

    /// #478: a 60° bit or a wider one draws its walls within the flatten
    /// tolerance; a steeper one within it scaled by its slope over the
    /// 60° bit's, so its walls stand no further off true up the flank.
    #[test]
    fn wall_tolerance_scales_with_the_slope_below_the_60_degree_bit() {
        let slope = |angle_deg: f64| (angle_deg.to_radians() / 2.0).tan();
        for angle_deg in [60.0, 90.0, 120.0] {
            assert!(
                (wall_tolerance_mm(TOL, slope(angle_deg)) - TOL).abs() <= 1e-15,
                "{angle_deg}°"
            );
        }
        for angle_deg in [15.0, 30.0, 45.0] {
            let wall_mm = wall_tolerance_mm(TOL, slope(angle_deg));
            assert!(
                (wall_mm / slope(angle_deg) - TOL / slope(60.0)).abs() <= 1e-12,
                "{angle_deg}°: {wall_mm} mm stands {} mm up the flank",
                wall_mm / slope(angle_deg)
            );
        }
    }

    /// The largest distance, mm, by which an edge of `contour` runs inside
    /// the circle of `radius_mm` about `centre` (its sagitta), and the
    /// largest distance of a vertex off that circle.
    fn off_circle(contour: &Contour, centre: Point, radius_mm: f64) -> (f64, f64) {
        let vertices = contour.polyline().vertices();
        let mut sagitta_mm: f64 = 0.0;
        let mut vertex_mm: f64 = 0.0;
        for (k, a) in vertices.iter().enumerate() {
            let b = vertices[(k + 1) % vertices.len()];
            let middle = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
            sagitta_mm = sagitta_mm.max(radius_mm - middle.distance_to(centre));
            vertex_mm = vertex_mm.max((a.point().distance_to(centre) - radius_mm).abs());
        }
        (sagitta_mm, vertex_mm)
    }

    /// #478: `surround_within` flattens the design's arcs, the plug's
    /// outline, within its own tolerance and the band's within the
    /// coarser one. The star with its 12 mm circle, an 8 mm margin closed
    /// by a 20 mm rounding: the circle region's chords run no more than
    /// the design's tolerance inside the circle; the band's edge keeps the
    /// vertices `surround` gives it; with one tolerance it is `surround`.
    #[test]
    fn surround_within_draws_the_design_finer() {
        let (design, _) =
            nest(vec![star(), circle(Point::new(0.0, 0.0), 6.0)]).expect("star nests");
        let fine_mm = TOL / 8.0;
        let coarse = surround(&design, 8.0, 20.0, TOL);
        let fine = surround_within(&design, 8.0, 20.0, TOL, fine_mm);
        assert_eq!(surround_within(&design, 8.0, 20.0, TOL, TOL), coarse);
        let disc = |s: &[Region]| {
            s.iter()
                .find(|r| r.holes().is_empty())
                .expect("the circle is a region of its own")
                .outer()
                .clone()
        };
        let band = |s: &[Region]| {
            s.iter()
                .find(|r| r.holes().len() == 1)
                .expect("the band")
                .outer()
                .polyline()
                .vertices()
                .len()
        };
        let (sagitta_mm, vertex_mm) = off_circle(&disc(&fine), Point::new(0.0, 0.0), 6.0);
        assert!(
            sagitta_mm <= fine_mm + 1e-5,
            "{sagitta_mm} inside the circle"
        );
        assert!(vertex_mm <= 1e-5, "a vertex {vertex_mm} off the circle");
        let (coarse_mm, _) = off_circle(&disc(&coarse), Point::new(0.0, 0.0), 6.0);
        assert!(
            coarse_mm > fine_mm,
            "{coarse_mm}: the coarse circle is coarser"
        );
        assert_eq!(band(&fine), band(&coarse), "the band's edge as before");
    }

    // The drawn band (3.4, #449; `docs/specs/phase-3/design.md`, "Drawn
    // boundary").

    /// The regions of `contours` nested.
    fn regions(contours: Vec<Contour>) -> Vec<Region> {
        nest(contours).expect("nests").0
    }

    /// A closed polygon through `points`.
    fn polygon(points: &[(f64, f64)]) -> Contour {
        let vertices = points.iter().map(|&(x, y)| Vertex::line(x, y)).collect();
        Contour::new(Polyline::new(vertices, true)).expect("a polygon")
    }

    /// Two overlapping 20 × 20 squares are one band of 20 × 30 mm; two
    /// apart stay two.
    #[test]
    fn drawn_band_is_the_union_of_the_outers() {
        let overlapping = [
            regions(vec![rect(0.0, 0.0, 20.0, 20.0)]),
            regions(vec![rect(10.0, 0.0, 20.0, 20.0)]),
        ]
        .concat();
        let band = drawn_band(&overlapping, TOL);
        assert_eq!(band.len(), 1, "{band:?}");
        assert!(
            (area_mm2(&band) - 600.0).abs() < 1e-9,
            "{}",
            area_mm2(&band)
        );
        assert!(band.iter().all(|r| r.holes().is_empty()));

        let apart = [
            regions(vec![rect(0.0, 0.0, 20.0, 20.0)]),
            regions(vec![rect(40.0, 0.0, 20.0, 20.0)]),
        ]
        .concat();
        let band = drawn_band(&apart, TOL);
        assert_eq!(band.len(), 2, "{band:?}");
        assert!((area_mm2(&band) - 800.0).abs() < 1e-9);
        assert!(drawn_band(&[], TOL).is_empty());
    }

    /// A ring-shaped boundary: its hole is band too, since board standing
    /// in the band would hold the plug off the female (#484). Four bars
    /// that ring a gap between them leave no hole either.
    #[test]
    fn drawn_band_drops_holes() {
        let ring = regions(vec![
            rect(0.0, 0.0, 40.0, 40.0),
            rect(10.0, 10.0, 20.0, 20.0),
        ]);
        assert_eq!(ring.len(), 1);
        assert_eq!(ring[0].holes().len(), 1);
        let band = drawn_band(&ring, TOL);
        assert_eq!(band.len(), 1, "{band:?}");
        assert!(band[0].holes().is_empty(), "the hole is band");
        assert!((area_mm2(&band) - 1600.0).abs() < 1e-9);

        let bars = [
            regions(vec![rect(0.0, 0.0, 30.0, 10.0)]),
            regions(vec![rect(0.0, 20.0, 30.0, 10.0)]),
            regions(vec![rect(0.0, 0.0, 10.0, 30.0)]),
            regions(vec![rect(20.0, 0.0, 10.0, 30.0)]),
        ]
        .concat();
        let band = drawn_band(&bars, TOL);
        assert_eq!(band.len(), 1, "{band:?}");
        assert!(band[0].holes().is_empty(), "the ringed gap is band");
        assert!((area_mm2(&band) - 900.0).abs() < 1e-9);
    }

    /// The four-point star (tips 30 mm out) in an 80 mm square: the tips
    /// are 10 mm from the edge, the least room anywhere, nearest at a tip.
    #[test]
    fn band_clearance_of_a_star_in_a_square() {
        let design = regions(vec![star()]);
        let band = regions(vec![rect(-40.0, -40.0, 80.0, 80.0)]);
        let BandClearance::Inside {
            narrowest_mm,
            x_mm,
            y_mm,
        } = band_clearance(&design, &band, TOL)
        else {
            panic!("the star is inside");
        };
        assert!((narrowest_mm - 10.0).abs() < 1e-9, "{narrowest_mm}");
        let at_tip = (x_mm.abs() - 30.0).abs() < 1e-9 && y_mm.abs() < 1e-9
            || (y_mm.abs() - 30.0).abs() < 1e-9 && x_mm.abs() < 1e-9;
        assert!(at_tip, "at a tip: ({x_mm}, {y_mm})");
    }

    /// A triangle whose left corner, (-8, 0), runs 8 mm past a band's edge
    /// at x = 0: the crossing is reported at that corner, 8 mm out.
    #[test]
    fn band_clearance_reports_a_crossing_with_its_point_and_depth() {
        let design = regions(vec![polygon(&[(-8.0, 0.0), (20.0, -5.0), (20.0, 5.0)])]);
        let band = regions(vec![rect(0.0, -50.0, 100.0, 100.0)]);
        let BandClearance::Crosses {
            x_mm,
            y_mm,
            depth_mm,
        } = band_clearance(&design, &band, TOL)
        else {
            panic!("the corner is past the edge");
        };
        assert!(
            (x_mm + 8.0).abs() < 1e-9 && y_mm.abs() < 1e-9,
            "({x_mm}, {y_mm})"
        );
        assert!((depth_mm - 8.0).abs() < 1e-9, "{depth_mm}");
    }

    /// A design whose outline runs along the band's edge, or touches it at
    /// a corner, is a crossing with depth 0: the plug's wall would have
    /// no band beyond it.
    #[test]
    fn a_boundary_touching_the_design_is_a_crossing() {
        let design = regions(vec![rect(0.0, 0.0, 10.0, 10.0)]);
        for band in [
            regions(vec![rect(0.0, -10.0, 50.0, 30.0)]),
            regions(vec![rect(-10.0, -10.0, 20.0, 20.0)]),
            regions(vec![polygon(&[
                (-5.0, -5.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (-5.0, 15.0),
            ])]),
        ] {
            let result = band_clearance(&design, &band, TOL);
            assert!(
                matches!(result, BandClearance::Crosses { depth_mm, .. } if depth_mm.abs() < 1e-6),
                "{result:?}"
            );
        }
        let apart = regions(vec![rect(-0.1, -0.1, 10.2, 10.2)]);
        let BandClearance::Inside { narrowest_mm, .. } = band_clearance(&design, &apart, TOL)
        else {
            panic!("0.1 mm of room is inside");
        };
        assert!((narrowest_mm - 0.1).abs() < 1e-9, "{narrowest_mm}");
    }

    /// A part wholly outside the band is a crossing, as far out as its
    /// farthest point; one part outside among several is too.
    #[test]
    fn a_design_part_outside_is_a_crossing() {
        let band = regions(vec![rect(0.0, 0.0, 50.0, 50.0)]);
        let outside = regions(vec![rect(60.0, 0.0, 10.0, 5.0)]);
        let BandClearance::Crosses { x_mm, depth_mm, .. } = band_clearance(&outside, &band, TOL)
        else {
            panic!("outside");
        };
        assert!((x_mm - 70.0).abs() < 1e-9, "{x_mm}");
        assert!((depth_mm - 20.0).abs() < 1e-9, "{depth_mm}");

        let two = [
            regions(vec![rect(10.0, 10.0, 5.0, 5.0)]),
            regions(vec![rect(80.0, 10.0, 5.0, 5.0)]),
        ]
        .concat();
        assert!(matches!(
            band_clearance(&two, &band, TOL),
            BandClearance::Crosses { .. }
        ));
        assert!(matches!(
            band_clearance(&two, &[], TOL),
            BandClearance::Crosses { .. }
        ));
    }

    /// Closing an L-shaped band by 3 mm rounds only its inward corner: the
    /// outline keeps its sharp outward corners (the bounding box and each
    /// outward vertex stay, and the area grows by the fillet at (20, 20),
    /// `r² (1 − π/4)`), and a slot narrower than twice the radius fills.
    #[test]
    fn closed_band_rounds_inward_corners_only() {
        let corners = [
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (20.0, 40.0),
            (0.0, 40.0),
        ];
        let l = regions(vec![polygon(&[
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (20.0, 20.0),
            (20.0, 40.0),
            (0.0, 40.0),
        ])]);
        let closed = closed_band(&l, 3.0, TOL);
        assert_eq!(closed.len(), 1, "{closed:?}");
        let bounds = closed[0].outer().polyline().bounds().expect("bounds");
        assert!(bounds.min.x.abs() < 1e-9 && bounds.min.y.abs() < 1e-9);
        assert!((bounds.max.x - 40.0).abs() < 1e-9 && (bounds.max.y - 40.0).abs() < 1e-9);
        let fillet = 9.0 * (1.0 - PI / 4.0);
        let added = area_mm2(&closed) - area_mm2(&l);
        assert!(
            (added - fillet).abs() < 0.05,
            "added {added}, fillet {fillet}"
        );
        for (x, y) in corners {
            assert!(
                closed[0]
                    .outer()
                    .polyline()
                    .vertices()
                    .iter()
                    .any(|v| (v.point().x - x).abs() < 1e-6 && (v.point().y - y).abs() < 1e-6),
                "outward corner ({x}, {y}) stays sharp"
            );
        }

        // A 2 mm slot cut into a block closes at r = 3 and stays at r = 0.5.
        let slotted = regions(vec![polygon(&[
            (0.0, 0.0),
            (30.0, 0.0),
            (30.0, 20.0),
            (16.0, 20.0),
            (16.0, 10.0),
            (14.0, 10.0),
            (14.0, 20.0),
            (0.0, 20.0),
        ])]);
        let filled = closed_band(&slotted, 3.0, TOL);
        assert!(
            area_mm2(&filled) > area_mm2(&slotted) + 19.0,
            "the slot fills: {} vs {}",
            area_mm2(&filled),
            area_mm2(&slotted)
        );
        let open = closed_band(&slotted, 0.5, TOL);
        assert!(
            area_mm2(&open) < area_mm2(&slotted) + 1.0,
            "{}",
            area_mm2(&open)
        );
    }

    /// With no rounding the closed band is the band united; a negative or
    /// non-finite rounding gives nothing, as `rounded_band`'s does.
    #[test]
    fn closed_band_without_rounding_is_the_band() {
        let band = regions(vec![rect(0.0, 0.0, 20.0, 20.0)]);
        let same = closed_band(&band, 0.0, TOL);
        assert_eq!(same.len(), 1);
        assert!((area_mm2(&same) - 400.0).abs() < 1e-9);
        for bad in [-1.0, f64::NAN, f64::INFINITY] {
            assert!(closed_band(&band, bad, TOL).is_empty(), "{bad}");
        }
    }

    /// `rounded_band` is the design's outers grown by `M + R`, united and
    /// closed by `R`: the same regions, to the last place, as the
    /// construction written out before `closed_band` shared its tail.
    #[test]
    fn rounded_band_is_unchanged_by_the_refactor() {
        let (design, _) = nest(vec![star(), circle(Point::new(0.0, 0.0), 6.0)]).expect("nests");
        let pair = [
            regions(vec![rect(0.0, 0.0, 10.0, 10.0)]),
            regions(vec![rect(14.0, 3.0, 8.0, 6.0)]),
        ]
        .concat();
        for design in [design, pair] {
            for (margin_mm, rounding_mm) in [(8.0, 0.0), (8.0, 5.0), (2.0, 5.0), (9.0, 12.0)] {
                let grown = grown_outers(&design, margin_mm + rounding_mm, Bays::Island);
                let united = filled(union(&grown, &[], TOL));
                let before = if rounding_mm == 0.0 {
                    united
                } else {
                    floor_regions(&united, rounding_mm, TOL)
                };
                assert_eq!(
                    rounded_band(&design, margin_mm, rounding_mm, TOL),
                    before,
                    "M {margin_mm} R {rounding_mm}"
                );
            }
        }
    }

    /// A drawn band runs past the board like a computed one: clipped to
    /// the rectangle by `band_on_board`, and unchanged (`None`) when it
    /// stays on the board.
    #[test]
    fn drawn_band_clips_to_the_board_like_a_computed_one() {
        let drawn = drawn_band(&regions(vec![rect(-10.0, -10.0, 60.0, 60.0)]), TOL);
        let board = Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(40.0, 40.0),
        };
        let clipped = band_on_board(&drawn, board, TOL).expect("it runs past the board");
        assert!((area_mm2(&clipped) - 1600.0).abs() < 1e-9);
        let bounds = clipped[0].outer().polyline().bounds().expect("bounds");
        assert!(bounds.min.x.abs() < 1e-9 && (bounds.max.y - 40.0).abs() < 1e-9);

        let inside = drawn_band(&regions(vec![rect(5.0, 5.0, 20.0, 20.0)]), TOL);
        assert_eq!(band_on_board(&inside, board, TOL), None);
    }
}
