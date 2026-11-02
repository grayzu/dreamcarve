// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Region constructions a carve and the safety pass both need
//! (`docs/specs/phase-3/design.md`, "dc-cam" → the two region
//! constructions; security review M3): they live here so `dc-gcode`
//! re-derives material without calling a generator.

use crate::boolean::{difference, union};
use crate::contour::Contour;
use crate::offset::offset;
use crate::region::{Region, nest};

/// A carve's floor: each of `regions` inset by `inset_mm` (for a V-bit
/// carve with a flat depth `F`, `F · tan(α/2)`, the groove's half-width at
/// the floor). Each outer is offset inward and each hole outward by
/// `inset_mm`, with round joins, and the grown holes cut from the shrunk
/// outers. A region can split into several floors or leave none; the
/// result is empty when nothing is left. Outers come back counter-clockwise
/// and holes clockwise (mm, Y-up), in the order of `regions`.
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
        .flat_map(|r| inset_region(r, inset_mm, tolerance_mm))
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
/// Holes of the design are never offset.
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
    let united = union(
        &grown_outers(regions, margin_mm + rounding_mm),
        &[],
        tolerance_mm,
    );
    if rounding_mm == 0.0 {
        return united;
    }
    floor_regions(&united, rounding_mm, tolerance_mm)
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
    if !(usable(margin_mm) && (rounding_mm == 0.0 || usable(rounding_mm))) {
        return Vec::new();
    }
    let band = if rounding_mm == 0.0 {
        grown_outers(regions, margin_mm)
    } else {
        rounded_band(regions, margin_mm, rounding_mm, tolerance_mm)
    };
    difference(&band, regions, tolerance_mm)
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
    let foot = grown_outers(design, inset_mm);
    surround
        .iter()
        .flat_map(|region| {
            if is_design_hole(region, design) {
                inset_region(region, inset_mm, tolerance_mm)
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
    floor_regions(regions, inset_mm, tolerance_mm)
        .into_iter()
        .map(|r| r.outer().clone())
        .collect()
}

/// Whether `mm` is a positive finite length.
fn usable(mm: f64) -> bool {
    mm.is_finite() && mm > 0.0
}

/// Every region's outer offset outward by `distance_mm` with round joins,
/// each the region it encloses; they may overlap.
fn grown_outers(regions: &[Region], distance_mm: f64) -> Vec<Region> {
    regions
        .iter()
        .flat_map(|r| offset(r.outer(), distance_mm))
        .filter_map(single_region)
        .collect()
}

/// [`floor_regions`] of one region.
fn inset_region(region: &Region, inset_mm: f64, tolerance_mm: f64) -> Vec<Region> {
    let outers = offset(region.outer(), -inset_mm);
    if outers.is_empty() {
        return Vec::new();
    }
    // `offset` keeps a hole's clockwise winding and grows it for a
    // positive delta.
    let grown: Vec<Contour> = region
        .holes()
        .iter()
        .flat_map(|hole| offset(hole, inset_mm))
        .collect();
    if grown.is_empty() {
        return outers.into_iter().filter_map(single_region).collect();
    }
    let mut all = outers.clone();
    all.extend(grown.iter().cloned());
    if let Ok((nested, warnings)) = nest(all) {
        let holes: usize = nested.iter().map(|r| r.holes().len()).sum();
        // A grown hole that encloses the shrunk outer nests as the outer
        // of an inverted annulus; only the shrunk outers may be outers.
        let clean = warnings.is_empty()
            && nested.len() == outers.len()
            && holes == grown.len()
            && nested
                .iter()
                .all(|r| r.depth() == 0 && outers.contains(r.outer()));
        if clean {
            return nested;
        }
    }
    let subject: Vec<Region> = outers.into_iter().filter_map(single_region).collect();
    let clip: Vec<Region> = grown
        .iter()
        .filter_map(|c| single_region(c.reversed()))
        .collect();
    difference(&subject, &clip, tolerance_mm)
}

/// The region `contour` alone encloses.
fn single_region(contour: Contour) -> Option<Region> {
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
}
