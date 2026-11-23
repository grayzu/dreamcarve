// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Property tests for the geometry invariants (issue #72;
//! `docs/rust-guidelines.md`, "Tests"): offsets stay simple and at their
//! distance, nesting does not depend on input order, flattening stays
//! within its tolerance, a difference never adds area, and every regular
//! polygon fine enough for the tolerance is recognised as a circle.
//!
//! The seed is fixed so every run, in CI and locally, tries the same
//! cases; set `PROPTEST_RNG_SEED` to explore others. Failures proptest
//! finds are kept in `proptest-regressions/` and replayed first.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    reason = "test code reports failure by panicking"
)]

use dc_geometry::{
    Contour, EPS_MM, Point, Polyline, Region, Segment, Vertex, area_mm2, as_circle, closest_points,
    difference, flatten, nest, offset, point_segment_distance_mm, segment_distance_mm,
};
use kurbo::{CubicBez, ParamCurve, ParamCurveNearest};
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};
use std::f64::consts::{PI, TAU};

/// The seed every run starts from unless `PROPTEST_RNG_SEED` is set.
const SEED: u64 = 0x0072_d0ca_4e00_0072;

/// `cases` cases from the fixed seed, or from `PROPTEST_RNG_SEED` when set.
/// Failures are saved to, and replayed from, the crate's
/// `proptest-regressions/properties.txt` (`cargo test` runs integration
/// tests in the crate directory).
fn config(cases: u32) -> ProptestConfig {
    let mut config = ProptestConfig::with_cases(cases);
    if std::env::var_os("PROPTEST_RNG_SEED").is_none() {
        config.rng_seed = RngSeed::Fixed(SEED);
    }
    config.failure_persistence = Some(Box::new(FileFailurePersistence::Direct(
        "proptest-regressions/properties.txt",
    )));
    config
}

/// A star-shaped polygon about a centre in ±500 mm: 3 to 12 vertices, one
/// per equal sector of the full turn, each 30–70 % of the way across its
/// sector and 5–100 mm out. Consecutive vertices are less than half a turn
/// apart about the centre, so the polygon is simple by construction.
fn star_polygon() -> impl Strategy<Value = Polyline> {
    (3_usize..=12, -500.0..500.0_f64, -500.0..500.0_f64)
        .prop_flat_map(|(n, cx, cy)| {
            (
                Just(Point::new(cx, cy)),
                prop::collection::vec((0.3..0.7_f64, 5.0..100.0_f64), n),
            )
        })
        .prop_map(|(centre, spokes)| star_from(centre, &spokes))
}

fn star_from(centre: Point, spokes: &[(f64, f64)]) -> Polyline {
    let n = spokes.len() as f64;
    let vertices = spokes
        .iter()
        .enumerate()
        .map(|(k, &(across, radius_mm))| {
            let angle = TAU * (k as f64 + across) / n;
            Vertex::line(
                centre.x + radius_mm * angle.cos(),
                centre.y + radius_mm * angle.sin(),
            )
        })
        .collect();
    Polyline::new(vertices, true)
}

/// Shortest distance from `p` to the line of `polyline`, mm.
fn distance_to(polyline: &Polyline, p: Point) -> f64 {
    polyline
        .segments()
        .map(|s| point_segment_distance_mm(s, p))
        .fold(f64::INFINITY, f64::min)
}

/// The start and midpoint of `seg`, following an arc.
fn start_and_mid(seg: Segment) -> [Point; 2] {
    let mid = seg
        .point_at(seg.length_mm() / 2.0)
        .map_or(seg.start, |(p, _)| p);
    [seg.start, mid]
}

proptest! {
    #![proptest_config(config(1024))]

    /// An offset of a simple polygon, outward or inward, of an outer or a
    /// hole: every result is simple, keeps the input's winding, and every
    /// vertex and segment midpoint is `|delta|` from the input. Outward
    /// always leaves something.
    #[test]
    fn offset_of_simple_polygon_is_simple_and_at_distance(
        polygon in star_polygon(),
        delta_mm in prop_oneof![0.1..20.0_f64, -20.0..-0.1_f64],
        hole in any::<bool>(),
    ) {
        let contour = Contour::new(polygon.clone());
        prop_assert!(contour.is_ok(), "a star polygon is simple: {contour:?}");
        let contour = contour.expect("checked above");
        let contour = if hole { contour.reversed() } else { contour };
        let results = offset(&contour, delta_mm);
        if delta_mm > 0.0 {
            prop_assert!(!results.is_empty(), "an outward offset leaves something");
        }
        // cavalier_contours works to 1e-5 in its own intersection tests.
        let tolerance_mm = 1e-5 * (1.0 + delta_mm.abs());
        for result in &results {
            prop_assert_eq!(result.is_ccw(), contour.is_ccw());
            let simple = Contour::new(result.polyline().clone());
            prop_assert!(simple.is_ok(), "offset crosses itself: {simple:?}");
            for seg in result.polyline().segments() {
                for p in start_and_mid(seg) {
                    let d_mm = distance_to(contour.polyline(), p);
                    prop_assert!(
                        (d_mm - delta_mm.abs()).abs() <= tolerance_mm,
                        "({}, {}) is {d_mm} mm from the input, not {}",
                        p.x, p.y, delta_mm.abs()
                    );
                }
            }
        }
    }
}

/// One square of [`nested_squares`]: its outline, and where it sits.
#[derive(Debug, Clone)]
struct Square {
    outline: Polyline,
    /// Which stack it belongs to.
    stack: usize,
    /// How many squares of its stack enclose it.
    level: usize,
}

/// A sort key for a contour: its lower-left corner, to the micrometre.
fn corner_key(contour: &Contour) -> (i64, i64) {
    let b = contour.polyline().bounds().expect("a contour has bounds");
    (
        (b.min.x * 1e6).round() as i64,
        (b.min.y * 1e6).round() as i64,
    )
}

/// A region as `(depth, outer, sorted holes)`, for comparing nestings.
type RegionKey = (usize, (i64, i64), Vec<(i64, i64)>);

fn region_key(region: &Region) -> RegionKey {
    let mut holes: Vec<(i64, i64)> = region.holes().iter().map(corner_key).collect();
    holes.sort_unstable();
    (region.depth(), corner_key(region.outer()), holes)
}

/// Up to four stacks of up to four squares, each square strictly inside the
/// one before it at a random place with a gap of at least 5 % of the
/// parent's side, the stacks 300 mm apart; each square wound either way.
fn nested_squares() -> impl Strategy<Value = Vec<Square>> {
    let stack = prop::collection::vec(
        (0.3..0.8_f64, 0.0..1.0_f64, 0.0..1.0_f64, any::<bool>()),
        1..=4,
    );
    (prop::collection::vec((100.0..200.0_f64, stack), 1..=4)).prop_map(|stacks| {
        let mut out = Vec::new();
        for (s, (first_side_mm, levels)) in stacks.into_iter().enumerate() {
            let mut min = Point::new(300.0 * s as f64, 0.0);
            let mut side_mm = first_side_mm;
            for (level, (shrink, u, v, reverse)) in levels.into_iter().enumerate() {
                if level > 0 {
                    let gap_mm = 0.05 * side_mm;
                    let inner_mm = side_mm * shrink;
                    let room_mm = side_mm - inner_mm - 2.0 * gap_mm;
                    min = Point::new(min.x + gap_mm + u * room_mm, min.y + gap_mm + v * room_mm);
                    side_mm = inner_mm;
                }
                let square = Polyline::rect(min, side_mm, side_mm);
                out.push(Square {
                    outline: if reverse { square.reversed() } else { square },
                    stack: s,
                    level,
                });
            }
        }
        out
    })
}

/// The nesting `squares` should give: even levels are outers at that
/// depth, each with the next level of its stack as its one hole.
fn expected_nesting(squares: &[Square]) -> Vec<RegionKey> {
    let key = |sq: &Square| corner_key(&Contour::new(sq.outline.clone()).expect("a square"));
    let mut out: Vec<RegionKey> = squares
        .iter()
        .filter(|sq| sq.level % 2 == 0)
        .map(|outer| {
            let holes = squares
                .iter()
                .filter(|h| h.stack == outer.stack && h.level == outer.level + 1)
                .map(key)
                .collect();
            (outer.level, key(outer), holes)
        })
        .collect();
    out.sort();
    out
}

fn nesting(squares: &[Square]) -> Result<Vec<RegionKey>, TestCaseError> {
    let contours = squares
        .iter()
        .map(|sq| Contour::new(sq.outline.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TestCaseError::fail(format!("a square is a contour: {e}")))?;
    let (regions, warnings) =
        nest(contours).map_err(|e| TestCaseError::fail(format!("squares do not cross: {e}")))?;
    prop_assert!(warnings.is_empty(), "{warnings:?}");
    let mut keys: Vec<RegionKey> = regions.iter().map(region_key).collect();
    keys.sort();
    Ok(keys)
}

proptest! {
    #![proptest_config(config(1024))]

    /// Nested, non-crossing squares give the same parts, holes and depths
    /// in any input order, and the ones their stacking implies.
    #[test]
    fn nest_is_order_independent(
        (squares, shuffled) in nested_squares()
            .prop_flat_map(|s| (Just(s.clone()), Just(s).prop_shuffle())),
    ) {
        let expected = expected_nesting(&squares);
        prop_assert_eq!(&nesting(&squares)?, &expected);
        prop_assert_eq!(&nesting(&shuffled)?, &expected);
    }
}

/// A point in ±100 mm.
fn point() -> impl Strategy<Value = kurbo::Point> {
    (-100.0..100.0_f64, -100.0..100.0_f64).prop_map(|(x, y)| kurbo::Point::new(x, y))
}

proptest! {
    #![proptest_config(config(1024))]

    /// A random cubic Bézier flattens to one open polyline whose vertices
    /// and segment points all lie within the tolerance of the curve, and
    /// which passes within the tolerance of every point of the curve.
    /// flatten asks kurbo for half the tolerance, its own bound being
    /// approximate (#225).
    #[test]
    fn flatten_stays_within_tolerance(
        (p0, p1, p2, p3) in (point(), point(), point(), point()),
        tolerance_mm in 0.001..1.0_f64,
    ) {
        let curve = CubicBez::new(p0, p1, p2, p3);
        let mut path = kurbo::BezPath::new();
        path.move_to(p0);
        path.curve_to(p1, p2, p3);
        let out = flatten(&path, tolerance_mm);
        prop_assert_eq!(out.len(), 1);
        let polyline = &out[0];
        prop_assert!(!polyline.is_closed());
        // Vertices closer than EPS_MM are merged, moving the line by that.
        let slack_mm = tolerance_mm + 2.0 * EPS_MM;
        for seg in polyline.segments() {
            for k in 0..=4 {
                let along = seg.length_mm() * f64::from(k) / 4.0;
                let p = seg.point_at(along).map_or(seg.start, |(p, _)| p);
                let nearest = curve.nearest(kurbo::Point::new(p.x, p.y), 1e-12);
                let d_mm = nearest.distance_sq.sqrt();
                prop_assert!(
                    d_mm <= slack_mm,
                    "({}, {}) is {d_mm} mm off the curve, tolerance {tolerance_mm}",
                    p.x, p.y
                );
            }
        }
        for k in 0..=512 {
            let c = curve.eval(f64::from(k) / 512.0);
            let d_mm = distance_to(polyline, Point::new(c.x, c.y));
            prop_assert!(
                d_mm <= slack_mm,
                "curve point ({}, {}) is {d_mm} mm from the polyline, tolerance {tolerance_mm}",
                c.x, c.y
            );
        }
    }
}

/// A shape for the boolean test: a star polygon or a circle (two arcs)
/// about a centre in ±150 mm, optionally with a circular hole small
/// enough to sit inside it about the same centre.
fn shape() -> impl Strategy<Value = Vec<Polyline>> {
    let centre = (-150.0..150.0_f64, -150.0..150.0_f64).prop_map(|(x, y)| Point::new(x, y));
    (
        centre,
        prop_oneof![
            prop::collection::vec((0.3..0.7_f64, 5.0..100.0_f64), 3..=12).prop_map(Some),
            Just(None),
        ],
        1.0..100.0_f64,
        any::<bool>(),
    )
        .prop_map(|(centre, spokes, radius_mm, with_hole)| {
            let (outline, inradius_mm) = match spokes {
                // Every edge of a star is at least `min r × cos(gap / 2)`
                // from the centre, and a gap is under 0.94 π (see
                // `star_polygon`), so 0.09 × min r is inside.
                Some(spokes) => {
                    let min_r = spokes.iter().map(|s| s.1).fold(f64::INFINITY, f64::min);
                    (star_from(centre, &spokes), 0.09 * min_r)
                }
                None => (Polyline::circle(centre, radius_mm), 0.9 * radius_mm),
            };
            let mut out = vec![outline];
            if with_hole {
                out.push(Polyline::circle(centre, inradius_mm));
            }
            out
        })
}

fn regions_of(polylines: &[Polyline]) -> Result<Vec<Region>, TestCaseError> {
    let contours = polylines
        .iter()
        .map(|p| Contour::new(p.clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| TestCaseError::fail(format!("the shape is simple: {e}")))?;
    let (regions, _) =
        nest(contours).map_err(|e| TestCaseError::fail(format!("the hole is inside: {e}")))?;
    Ok(regions)
}

proptest! {
    #![proptest_config(config(1024))]

    /// `area(A − B)` lies between `area(A) − area(B)` and `area(A)`, to
    /// within the flattening tolerance times the arc length.
    #[test]
    fn boolean_difference_area_is_bounded(
        a in shape(),
        b in shape(),
        tolerance_mm in 0.001..0.1_f64,
    ) {
        let (ra, rb) = (regions_of(&a)?, regions_of(&b)?);
        let (area_a, area_b) = (area_mm2(&ra), area_mm2(&rb));
        let result = difference(&ra, &rb, tolerance_mm);
        let area_diff = area_mm2(&result);
        let length_mm: f64 = a.iter().chain(&b).map(Polyline::length_mm).sum();
        let slack_mm2 = tolerance_mm * length_mm + 1e-6 * (1.0 + area_a + area_b);
        prop_assert!(
            area_diff <= area_a + slack_mm2,
            "A − B has {area_diff} mm², more than A's {area_a} mm²"
        );
        prop_assert!(
            area_diff >= area_a - area_b - slack_mm2,
            "A − B has {area_diff} mm², less than A − B could ({area_a} − {area_b})"
        );
        for region in &result {
            prop_assert!(region.outer().is_ccw());
            prop_assert!(region.holes().iter().all(|h| !h.is_ccw()));
        }
    }
}

/// The fewest sides a regular polygon of circumradius `radius_mm` needs
/// for its edge midpoints to lie within 99 % of `tolerance_mm` of the
/// circle: the sagitta `r (1 − cos(π / n))`. At least 3.
fn fewest_sides(radius_mm: f64, tolerance_mm: f64) -> usize {
    let half_angle = (1.0 - 0.99 * tolerance_mm / radius_mm).acos();
    ((PI / half_angle).ceil() as usize).max(3)
}

proptest! {
    #![proptest_config(config(1024))]

    /// Every regular n-gon whose edges stay within the tolerance of its
    /// circumcircle is a circle, with that centre and radius, whatever its
    /// size, place, rotation and direction.
    #[test]
    fn as_circle_accepts_every_regular_ngon_above_tolerance_resolution(
        cx in -1000.0..1000.0_f64,
        cy in -1000.0..1000.0_f64,
        radius_mm in 0.5..500.0_f64,
        tolerance_mm in 0.001..0.1_f64,
        extra_sides in 0_usize..200,
        phase in 0.0..TAU,
        reverse in any::<bool>(),
    ) {
        let sides = fewest_sides(radius_mm, tolerance_mm) + extra_sides;
        let vertices = (0..sides)
            .map(|k| {
                let angle = phase + TAU * k as f64 / sides as f64;
                Vertex::line(cx + radius_mm * angle.cos(), cy + radius_mm * angle.sin())
            })
            .collect();
        let polygon = Polyline::new(vertices, true);
        let polygon = if reverse { polygon.reversed() } else { polygon };
        let found = as_circle(&polygon, tolerance_mm);
        prop_assert!(found.is_some(), "a {sides}-gon of radius {radius_mm} is not a circle");
        let (centre, found_mm) = found.expect("checked above");
        prop_assert!(centre.distance_to(Point::new(cx, cy)) <= 1e-6 * (1.0 + radius_mm));
        prop_assert!((found_mm - radius_mm).abs() <= 1e-6 * (1.0 + radius_mm));
    }
}

/// A segment with ends in ±10 mm: a line, or an arc of either direction
/// sweeping up to 355°, past a half turn (|bulge| > 1) as often as not.
fn any_segment() -> impl Strategy<Value = Segment> {
    (
        -10.0..10.0_f64,
        -10.0..10.0_f64,
        -10.0..10.0_f64,
        -10.0..10.0_f64,
        prop_oneof![Just(0.0), -355.0..-1.0_f64, 1.0..355.0_f64],
    )
        .prop_map(|(sx, sy, ex, ey, sweep_deg)| Segment {
            start: Point::new(sx, sy),
            end: Point::new(ex, ey),
            bulge: (sweep_deg.to_radians() / 4.0).tan(),
        })
}

/// The least distance from points spread densely along `s` to `t`: never
/// below the true distance, and above it by no more than the spacing.
fn sampled_distance_mm(s: Segment, t: Segment) -> f64 {
    const SAMPLES: u32 = 4000;
    let length_mm = s.length_mm();
    (0..=SAMPLES)
        .filter_map(|k| s.point_at(length_mm * f64::from(k) / f64::from(SAMPLES)))
        .map(|(p, _)| point_segment_distance_mm(t, p))
        .fold(f64::INFINITY, f64::min)
}

proptest! {
    #![proptest_config(config(1024))]

    /// #686: the exact segment distance, and the closest pair's gap, are
    /// never more than what dense sampling of either segment against the
    /// other finds, whatever the arcs sweep. Over-reading is the unsafe
    /// side: S9 and S17 hold cuts to it.
    #[test]
    fn segment_distance_is_never_above_sampling(s in any_segment(), t in any_segment()) {
        prop_assume!(s.start.distance_to(s.end) > 1e-3 && t.start.distance_to(t.end) > 1e-3);
        let sampled_mm = sampled_distance_mm(s, t).min(sampled_distance_mm(t, s));
        let exact_mm = segment_distance_mm(s, t);
        prop_assert!(exact_mm <= sampled_mm + 1e-9, "{exact_mm} > sampled {sampled_mm}");
        let (p, q) = closest_points(s, t);
        prop_assert!(
            p.distance_to(q) <= sampled_mm + 1e-9,
            "closest pair {} apart > sampled {sampled_mm}",
            p.distance_to(q)
        );
    }
}

proptest! {
    #![proptest_config(config(512))]

    /// Phase 5, shape outlines: whatever its sizes, a drawn shape's
    /// outline is a simple closed contour, counter-clockwise, whose
    /// bounding box is its sizes. (A slot's length is centre to centre,
    /// so its box is longer by its width.) A corner is held to half the
    /// shorter side, so no corner size makes an outline cross itself.
    #[test]
    fn a_shapes_outline_is_simple_and_its_box_is_its_sizes(
        width_mm in 0.5..600.0_f64,
        height_mm in 0.5..600.0_f64,
        corner_mm in 0.01..400.0_f64,
        sides in 3_u32..=50,
        tolerance_mm in 0.001..0.1_f64,
    ) {
        use dc_geometry::shapes;
        let shorter = width_mm.min(height_mm);
        let outlines = [
            ("rectangle", shapes::rectangle(width_mm, height_mm), width_mm, height_mm),
            ("rounded", shapes::rounded_rectangle(width_mm, height_mm, corner_mm), width_mm, height_mm),
            ("chamfered", shapes::chamfered_rectangle(width_mm, height_mm, corner_mm), width_mm, height_mm),
            ("circle", shapes::circle(shorter), shorter, shorter),
            ("ellipse", shapes::ellipse(width_mm, height_mm, tolerance_mm), width_mm, height_mm),
            ("slot", shapes::slot(width_mm, height_mm), width_mm + height_mm, height_mm),
        ];
        for (name, outline, box_width_mm, box_height_mm) in outlines {
            let contour = Contour::new(outline.clone());
            prop_assert!(contour.is_ok(), "{name}: {contour:?}");
            prop_assert!(outline.signed_area_mm2() > 0.0, "{name} is clockwise");
            let bounds = outline.bounds().expect("an outline has bounds");
            prop_assert!((bounds.width_mm() - box_width_mm).abs() < 1e-9, "{name}: {bounds:?}");
            prop_assert!((bounds.height_mm() - box_height_mm).abs() < 1e-9, "{name}: {bounds:?}");
            prop_assert!((bounds.min.x + bounds.max.x).abs() < 1e-9, "{name} is centred");
            prop_assert!((bounds.min.y + bounds.max.y).abs() < 1e-9, "{name} is centred");
        }
        let polygon = shapes::regular_polygon(sides, shorter);
        prop_assert!(Contour::new(polygon.clone()).is_ok(), "{sides} sides");
        prop_assert!(polygon.signed_area_mm2() > 0.0);
        prop_assert_eq!(polygon.vertices().len(), sides as usize);
        // To the flats, the middle of each side is at the radius.
        let flats = shapes::regular_polygon(sides, shapes::corner_radius_of_flats(sides, shorter));
        for side in flats.segments() {
            let (middle, _) = side.point_at(side.length_mm() / 2.0).expect("a side has a middle");
            prop_assert!((middle.distance_to(Point::new(0.0, 0.0)) - shorter).abs() < 1e-9);
        }
    }

    /// A gear of any tooth count its sizes allow is a simple closed
    /// contour between its root and outside circles, with one tooth's
    /// turn mapping it onto itself.
    #[test]
    fn a_gears_outline_is_simple_and_symmetric(
        teeth in 6_u32..=120,
        outside_radius_mm in 5.0..300.0_f64,
        straight in any::<bool>(),
        rounded_crest in any::<bool>(),
        root in 0_u8..3,
    ) {
        use dc_geometry::shapes::gear::{Gear, GearCrest, GearFlanks, GearRoot};
        let defaults = Gear::with_defaults(outside_radius_mm);
        let module = outside_radius_mm * 2.0 / (f64::from(teeth) + 2.0);
        let mut gear = Gear {
            teeth,
            whole_depth_mm: module * 2.25,
            flanks: if straight { GearFlanks::Straight } else { GearFlanks::Involute },
            crest: if rounded_crest { GearCrest::Rounded } else { GearCrest::Flat },
            root: [GearRoot::Rounded, GearRoot::Flat, GearRoot::Sharp][usize::from(root)],
            ..defaults
        };
        gear.root_fillet_mm = gear.max_root_fillet_mm() * 0.5;
        gear.crest_radius_mm = gear.max_crest_radius_mm() * 0.5;
        prop_assume!(gear.problems().is_empty());
        let outline = gear.outline().expect("the sizes give a gear").remove(0);
        let contour = Contour::new(outline.clone());
        prop_assert!(contour.is_ok(), "{gear:?}: {contour:?}");
        prop_assert!(outline.signed_area_mm2() > 0.0);
        let origin = Point::new(0.0, 0.0);
        for vertex in outline.vertices() {
            let radius_mm = vertex.point().distance_to(origin);
            prop_assert!(radius_mm <= gear.outside_radius_mm + 1e-9, "{radius_mm}");
            prop_assert!(radius_mm >= gear.root_radius_mm() - 1e-9, "{radius_mm}");
        }
        let count = outline.vertices().len();
        prop_assert_eq!(count % teeth as usize, 0, "the same points on every tooth");
        let per_tooth = count / teeth as usize;
        let turn = dc_geometry::Transform::rotate(TAU / f64::from(teeth));
        for (i, vertex) in outline.vertices().iter().enumerate() {
            let next = outline.vertices()[(i + per_tooth) % count].point();
            prop_assert!(turn.apply(vertex.point()).distance_to(next) < 1e-7, "vertex {i}");
        }
    }
}
