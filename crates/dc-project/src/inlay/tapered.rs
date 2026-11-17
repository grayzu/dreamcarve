// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! An Inlay cut with a tapered ball nose (slice 27e, #681;
//! `docs/specs/phase-3/design.md`, "Algorithms → Tapered ball nose →
//! Inlay fit" and "The taper's ±0.5°, and calibration").
//!
//! Both boards are cut by the one bit, so wherever both are cut by its
//! cone the walls mate as a V-bit's do. The ball changes three things:
//!
//! - **The design is rounded for the bit** ([`rounded_for_bit`]): opened
//!   by `R_min = (D − G)·t + w_t`, so the plug's cone-cut points and fins
//!   meet a pocket the ball reached, and closed by `w_t`, so the male's
//!   chains into the design's notches and gaps run on the cone. What the
//!   rounding cannot fix, a part or hole it would remove, split or join,
//!   is refused ([`TooNarrow`]).
//! - **The glue gap and the plug depth** should each be at least the
//!   bit's tangent height `h_t` ([`glue_gap_below_ball`],
//!   [`plug_depth_below_ball`]): below it the plug's top edge rides on the
//!   pocket's fillet, or its wall near the surface is the ball's.
//! - **The seat moves with the bit's numbers** ([`seat_sensitivity`]):
//!   an error in the taper or the tip radius moves the line the cone cuts
//!   on both boards, so the walls still mate but the plug seats at another
//!   height. A test fit is the calibration (question 27).
//!
//! A V-bit has none of this: [`rounded_for_bit`] returns its design as it
//! is, and the other two answer `None`.

use dc_geometry::profile::BitProfile;
use dc_geometry::{
    Contour, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Rect, Region, area_mm2, closest_points,
    difference, floor_regions, intersection, nearest_point, nest, offset, union, wall_tolerance_mm,
};

/// What a rounding changes, or where it is refused (#681): a point of the
/// design (a convex corner, a narrow part), or a notch (an inward corner,
/// a narrow gap or hole).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CornerKind {
    /// A convex corner or a part narrower than `2 R_min`: the opening by
    /// `R_min` takes wood off the design.
    Point,
    /// An inward corner, or a gap or hole narrower than `2 w_t`: the
    /// closing by `w_t` adds wood to the design.
    Notch,
}

impl CornerKind {
    /// The kind in words: `point` or `notch`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Point => "point",
            Self::Notch => "notch",
        }
    }
}

/// The two radii an Inlay's design is rounded by for a tapered ball nose
/// (#681): opened by [`Self::open_mm`], closed by [`Self::close_mm`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BitRounding {
    /// `R_min = (D − G)·t + w_t`, mm, which is `w(D − G + h_t)`: every point
    /// of the design needs that much clearance for the pocket the ball
    /// leaves to take the plug's cone-cut point.
    pub open_mm: f64,
    /// `w_t = ρ cos β`, mm: a notch or gap narrower than `2 w_t` puts the
    /// male's chain in the ball, whose body above the start plane reaches
    /// past the plug's wall.
    pub close_mm: f64,
}

impl BitRounding {
    /// The rounding of a design carved by `profile` with the plug `rise_mm`
    /// (`D − G`) tall; `None` for a V-bit, which needs none, or a value
    /// that is not usable.
    #[must_use]
    pub fn of(profile: &BitProfile, rise_mm: f64) -> Option<Self> {
        let BitProfile::TaperedBall { .. } = profile else {
            return None;
        };
        let close_mm = profile.tangent_half_width_mm();
        let open_mm = rise_mm * profile.slope() + close_mm;
        let usable = |v: f64| v.is_finite() && v > 0.0;
        (usable(close_mm) && usable(open_mm) && rise_mm.is_finite() && rise_mm >= 0.0)
            .then_some(Self { open_mm, close_mm })
    }

    /// The radius of `kind`: [`Self::open_mm`] for a point,
    /// [`Self::close_mm`] for a notch.
    #[must_use]
    pub const fn radius_mm(&self, kind: CornerKind) -> f64 {
        match kind {
            CornerKind::Point => self.open_mm,
            CornerKind::Notch => self.close_mm,
        }
    }
}

/// The largest change the rounding makes of one kind (#681): the point of
/// the design as given that lies farthest from the rounded outline (a
/// point), or the point of the rounded outline farthest from the design
/// (a notch), and that distance.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundingChange {
    /// A point or a notch.
    pub kind: CornerKind,
    /// Where, mm, in the design's frame.
    pub at: Point,
    /// How far the outline moved there, mm.
    pub change_mm: f64,
    /// The radius it was rounded by, mm: `R_min` or `w_t`.
    pub radius_mm: f64,
}

/// An Inlay's design rounded for its bit ([`rounded_for_bit`]).
#[derive(Debug, Clone, PartialEq)]
pub struct RoundedDesign {
    /// The rounded regions, nested.
    pub regions: Vec<Region>,
    /// For each of [`Self::regions`], the place in the design given of
    /// the region it was rounded from: the rounding keeps the topology, so
    /// there is exactly one.
    pub sources: Vec<usize>,
    /// The largest change of each kind that moved the outline by more than
    /// the flatten tolerance, points first; empty when nothing moved.
    pub changes: Vec<RoundingChange>,
}

/// Where rounding an Inlay's design for its bit would change its topology
/// (#681): a part or a hole removed, a part or hole split, or two joined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TooNarrow {
    /// A part too narrow (`Point`), or a gap or hole too narrow (`Notch`).
    pub kind: CornerKind,
    /// Where, mm, in the design's frame: the middle of the widest place of
    /// the piece lost.
    pub at: Point,
    /// How wide it is, mm: twice the largest clearance of the piece lost
    /// (a part, the wood between a hole and the outside, a hole), or for
    /// two parts joined across a gap, how near they come.
    pub width_mm: f64,
    /// How wide it must be, mm: `2 R_min` for a part, `2 w_t` for a gap.
    pub needed_mm: f64,
}

/// `design` (an Inlay's, nested, its near parts already merged) rounded
/// for a carve by `profile` with the plug `rise_mm` (`D − G`) tall (#681;
/// design spec, "Inlay fit"): `X′ = ((((X ⊖ R_min) ⊕ R_min) ⊕ w_t) ⊖ w_t)`,
/// round joins ([`BitRounding`]), flattened within the bit's wall
/// tolerance ([`wall_tolerance_mm`] of `tolerance_mm`, the project's
/// flatten tolerance). The generator, the safety pass and validation all
/// call it, so the female's design, the male's (on the mirrored design)
/// and the safety pass's are the same.
///
/// For a V-bit, which needs no rounding, `design` comes back as it is,
/// with no change, whatever `rise_mm`.
///
/// The changes are measured against `tolerance_mm`: one
/// [`RoundingChange`] for points and one for notches, each at its
/// largest, when it is more than the tolerance.
///
/// # Errors
///
/// [`TooNarrow`] when the rounding would remove a part or a hole, split a
/// part or a hole, or join two parts or two holes.
pub fn rounded_for_bit(
    design: &[Region],
    profile: &BitProfile,
    rise_mm: f64,
    tolerance_mm: f64,
) -> Result<RoundedDesign, TooNarrow> {
    let unchanged = || RoundedDesign {
        regions: design.to_vec(),
        sources: (0..design.len()).collect(),
        changes: Vec::new(),
    };
    let Some(rounding) = BitRounding::of(profile, rise_mm) else {
        return Ok(unchanged());
    };
    if design.is_empty() {
        return Ok(unchanged());
    }
    let tolerance_mm = usable_tolerance(tolerance_mm);
    let wall_mm = wall_tolerance_mm(tolerance_mm, profile.slope());
    // A closing can leave a pinch the opening would take again (where it
    // fills a notch between two rounded points), so the two are repeated
    // until the design is both open and closed within the tolerance: the
    // pocket's ball then reaches every point of the plug.
    let mut closed = open_close(design, rounding, wall_mm);
    for _ in 0..MAX_ROUNDINGS {
        let again = open_close(&closed, rounding, wall_mm);
        let moved_mm = farthest_changes(&closed, &again, tolerance_mm)
            .iter()
            .map(|(_, d, _)| *d)
            .fold(0.0, f64::max);
        if moved_mm <= tolerance_mm {
            break;
        }
        closed = again;
    }
    let sources = topology(design, &closed, rounding, tolerance_mm)?;
    let changes: Vec<RoundingChange> = farthest_changes(design, &closed, tolerance_mm)
        .into_iter()
        .map(|(kind, change_mm, at)| RoundingChange {
            kind,
            at,
            change_mm,
            radius_mm: rounding.radius_mm(kind),
        })
        .collect();
    // A design the rounding moves nowhere by more than the tolerance is
    // cut as it is, its arcs kept: rounded, it would differ only by the
    // flattening.
    if changes.is_empty() {
        return Ok(unchanged());
    }
    Ok(RoundedDesign {
        regions: closed,
        sources,
        changes,
    })
}

/// How many times [`rounded_for_bit`] repeats the opening and closing at
/// most, after the first, for the design to settle open and closed.
const MAX_ROUNDINGS: usize = 4;

/// `regions` opened by `R_min` and closed by `w_t`, round joins, within
/// `wall_mm`: `((((X ⊖ R_min) ⊕ R_min) ⊕ w_t) ⊖ w_t)`.
///
/// Drawn as chords, an arc of exactly the radius the next opening shrinks
/// by lies a chord's sag inside it, and a round end of that radius loses
/// `√(2 R sag)` to it, which repeats each time the design is rounded. So
/// the opening grows back by `R_min` plus `wall_mm`, held to the design
/// (`∩ X`), and the closing shrinks back by `w_t` plus `wall_mm`, the
/// opened design kept (`∪ X`): its round ends and fillets are a chord's
/// sag rounder than the radii, and rounding the result again leaves it as
/// it is ([`rounded_for_bit`]'s property).
fn open_close(regions: &[Region], rounding: BitRounding, wall_mm: f64) -> Vec<Region> {
    let opened = intersection(
        &union(
            &grown(
                &floor_regions(regions, rounding.open_mm, wall_mm),
                rounding.open_mm + wall_mm,
            ),
            &[],
            wall_mm,
        ),
        regions,
        wall_mm,
    );
    let closed = floor_regions(
        &union(&grown(&opened, rounding.close_mm), &[], wall_mm),
        rounding.close_mm + wall_mm,
        wall_mm,
    );
    union(&closed, &opened, wall_mm)
}

/// `tolerance_mm` when it is a positive finite number, else the default.
fn usable_tolerance(tolerance_mm: f64) -> f64 {
    if tolerance_mm.is_finite() && tolerance_mm > 0.0 {
        tolerance_mm
    } else {
        FLATTEN_TOLERANCE_DEFAULT_MM
    }
}

/// `regions` grown by `distance_mm` with round joins: each outer offset
/// outward and each hole inward, nested again; the pieces may overlap,
/// which the caller unites.
fn grown(regions: &[Region], distance_mm: f64) -> Vec<Region> {
    regions
        .iter()
        .flat_map(|region| {
            let mut loops = offset(region.outer(), distance_mm);
            for hole in region.holes() {
                loops.extend(offset(hole, -distance_mm));
            }
            nest(loops).map(|(nested, _)| nested).unwrap_or_default()
        })
        .collect()
}

/// The box of `contour`, or an empty one at the origin.
fn box_of(contour: &Contour) -> Rect {
    contour.polyline().bounds().unwrap_or(Rect {
        min: Point::new(0.0, 0.0),
        max: Point::new(0.0, 0.0),
    })
}

/// Whether boxes `a` and `b` meet.
fn boxes_meet(a: Rect, b: Rect) -> bool {
    a.min.x <= b.max.x && b.min.x <= a.max.x && a.min.y <= b.max.y && b.min.y <= a.max.y
}

/// The region `contour` encloses, wound outward; `None` when it is
/// degenerate.
fn filled(contour: &Contour) -> Option<Region> {
    let outer = if contour.is_ccw() {
        contour.clone()
    } else {
        contour.reversed()
    };
    nest(vec![outer])
        .ok()
        .and_then(|(regions, _)| regions.into_iter().next())
}

/// Whether `a` and `b` share more than a sliver of area: more than the
/// square of `tolerance_mm`.
fn overlap(a: &Region, b: &Region, tolerance_mm: f64) -> bool {
    boxes_meet(box_of(a.outer()), box_of(b.outer()))
        && area_mm2(&intersection(
            std::slice::from_ref(a),
            std::slice::from_ref(b),
            tolerance_mm,
        )) > tolerance_mm * tolerance_mm
}

/// The places `j` of `to` whose region overlaps `from`.
fn overlapping(from: &Region, to: &[Region], tolerance_mm: f64) -> Vec<usize> {
    to.iter()
        .enumerate()
        .filter(|(_, r)| overlap(from, r, tolerance_mm))
        .map(|(j, _)| j)
        .collect()
}

/// The source of each region of `rounded` in `design`, or the place the
/// rounding changes the topology of the design ([`TooNarrow`]).
fn topology(
    design: &[Region],
    rounded: &[Region],
    rounding: BitRounding,
    tolerance_mm: f64,
) -> Result<Vec<usize>, TooNarrow> {
    let lost_part = |pieces: Vec<Region>, kind: CornerKind| {
        let (clearance_mm, at) = widest(&pieces, tolerance_mm);
        TooNarrow {
            kind,
            at,
            width_mm: 2.0 * clearance_mm,
            needed_mm: 2.0 * rounding.radius_mm(kind),
        }
    };
    let point = |i: usize| {
        let part = design.get(i).cloned().into_iter().collect::<Vec<_>>();
        lost_part(difference(&part, rounded, tolerance_mm), CornerKind::Point)
    };
    let notch = |j: usize| {
        let part = rounded.get(j).cloned().into_iter().collect::<Vec<_>>();
        lost_part(difference(&part, design, tolerance_mm), CornerKind::Notch)
    };
    let mut sources = vec![usize::MAX; rounded.len()];
    // Two parts joined across a gap.
    for (j, region) in rounded.iter().enumerate() {
        let from = overlapping(region, design, tolerance_mm);
        if from.len() >= 2 {
            // The gap is as wide as the nearest two of the parts joined
            // come.
            let (width_mm, at) = from
                .iter()
                .enumerate()
                .flat_map(|(k, &a)| from.iter().skip(k + 1).map(move |&b| (a, b)))
                .filter_map(|(a, b)| Some(nearest_between(design.get(a)?, design.get(b)?)))
                .fold((f64::INFINITY, Point::new(0.0, 0.0)), |best, next| {
                    if next.0 < best.0 { next } else { best }
                });
            return Err(TooNarrow {
                kind: CornerKind::Notch,
                at,
                width_mm,
                needed_mm: 2.0 * rounding.close_mm,
            });
        }
        if let Some(&i) = from.first()
            && let Some(source) = sources.get_mut(j)
        {
            *source = i;
        }
    }
    for (i, region) in design.iter().enumerate() {
        let to = overlapping(region, rounded, tolerance_mm);
        match to.as_slice() {
            // A part removed.
            [] => {
                return Err(lost_part(vec![region.clone()], CornerKind::Point));
            }
            [j] => holes(region, rounded.get(*j), tolerance_mm).map_err(|kind| match kind {
                HoleChange::Filled(hole) => lost_part(vec![hole], CornerKind::Notch),
                HoleChange::Opened => point(i),
                HoleChange::Closed => notch(*j),
            })?,
            // A part split.
            _ => return Err(point(i)),
        }
    }
    Ok(sources)
}

/// How the rounding changed a hole of a part.
enum HoleChange {
    /// The hole was filled: the closing joined the wood round it.
    Filled(Region),
    /// The wood between a hole and the outside, or between two holes, was
    /// opened away: a part too narrow.
    Opened,
    /// The closing split a hole, or closed a bay into a new one: a gap too
    /// narrow.
    Closed,
}

/// Whether a part of the design, `region`, keeps its holes as `after`,
/// the rounded region made from it: each hole of one overlaps exactly one
/// hole of the other.
fn holes(region: &Region, after: Option<&Region>, tolerance_mm: f64) -> Result<(), HoleChange> {
    let Some(after) = after else {
        return Ok(());
    };
    let before: Vec<Region> = region.holes().iter().filter_map(filled).collect();
    let now: Vec<Region> = after.holes().iter().filter_map(filled).collect();
    for hole in &before {
        match overlapping(hole, &now, tolerance_mm).len() {
            0 => {
                // Filled when the rounded part covers the hole; else the
                // wood round it was opened to the outside.
                let covered = area_mm2(&intersection(
                    std::slice::from_ref(hole),
                    std::slice::from_ref(after),
                    tolerance_mm,
                ));
                return Err(if covered >= 0.5 * hole.area_mm2() {
                    HoleChange::Filled(hole.clone())
                } else {
                    HoleChange::Opened
                });
            }
            1 => {}
            _ => return Err(HoleChange::Closed),
        }
    }
    for hole in &now {
        match overlapping(hole, &before, tolerance_mm).len() {
            0 => return Err(HoleChange::Closed),
            1 => {}
            _ => return Err(HoleChange::Opened),
        }
    }
    Ok(())
}

/// How near regions `a` and `b` come, mm, and the middle of the gap
/// there: the least distance between their outlines.
fn nearest_between(a: &Region, b: &Region) -> (f64, Point) {
    let segments = |r: &Region| -> Vec<dc_geometry::Segment> {
        std::iter::once(r.outer())
            .chain(r.holes())
            .flat_map(|c| c.polyline().segments().collect::<Vec<_>>())
            .collect()
    };
    let (sa, sb) = (segments(a), segments(b));
    let mut best = (f64::INFINITY, Point::new(0.0, 0.0));
    for &s in &sa {
        for &t in &sb {
            let (p, q) = closest_points(s, t);
            let d = p.distance_to(q);
            if d < best.0 {
                best = (d, Point::new((p.x + q.x) / 2.0, (p.y + q.y) / 2.0));
            }
        }
    }
    best
}

/// The largest clearance of any of `pieces`, mm, and the middle of its
/// widest place: the radius of the largest disc inside one, found by
/// halving the inset that leaves something ([`floor_regions`]) to well
/// below the tolerance. Pieces no larger than a sliver are left out.
fn widest(pieces: &[Region], tolerance_mm: f64) -> (f64, Point) {
    let mut best = (0.0, Point::new(0.0, 0.0));
    let mut found = false;
    for piece in pieces {
        if piece.area_mm2() <= tolerance_mm * tolerance_mm {
            continue;
        }
        let b = box_of(piece.outer());
        let one = std::slice::from_ref(piece);
        let (mut lo, mut hi) = (
            0.0_f64,
            0.5 * b.width_mm().min(b.height_mm()) + tolerance_mm,
        );
        let mut at = Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
        for _ in 0..40 {
            let mid = 0.5 * (lo + hi);
            let inset = floor_regions(one, mid, tolerance_mm);
            if let Some(c) = inset.first().map(|r| box_of(r.outer())) {
                lo = mid;
                at = Point::new((c.min.x + c.max.x) / 2.0, (c.min.y + c.max.y) / 2.0);
            } else {
                hi = mid;
            }
            if hi - lo < 1e-6 {
                break;
            }
        }
        if !found || lo > best.0 {
            best = (lo, at);
            found = true;
        }
    }
    best
}

/// The largest change of each kind the rounding of `design` into
/// `rounded` makes, more than `tolerance_mm`, points first: over every
/// point of the design's outlines (its arcs flattened within the
/// tolerance), its distance to the nearest outline of `rounded`; a point
/// the rounded design leaves outside is a point's change, one it covers a
/// notch's. Each with its distance and the point.
fn farthest_changes(
    design: &[Region],
    rounded: &[Region],
    tolerance_mm: f64,
) -> Vec<(CornerKind, f64, Point)> {
    let walls: Vec<&dc_geometry::Polyline> = rounded
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(Contour::polyline)
        .collect();
    let mut point: Option<(f64, Point)> = None;
    let mut notch: Option<(f64, Point)> = None;
    for contour in design
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
    {
        let flat = contour.polyline().with_arcs_flattened(tolerance_mm);
        for vertex in flat.vertices() {
            let p = vertex.point();
            let gap_mm = walls
                .iter()
                .filter_map(|w| nearest_point(w, p))
                .map(|n| n.gap_mm)
                .fold(f64::INFINITY, f64::min);
            if !(gap_mm.is_finite() && gap_mm > tolerance_mm) {
                continue;
            }
            let covered = rounded.iter().any(|r| r.contains_point(p));
            let slot = if covered { &mut notch } else { &mut point };
            if slot.is_none_or(|(d, _)| gap_mm > d) {
                *slot = Some((gap_mm, p));
            }
        }
    }
    [(CornerKind::Point, point), (CornerKind::Notch, notch)]
        .into_iter()
        .filter_map(|(kind, found)| found.map(|(d, p)| (kind, d, p)))
        .collect()
}

/// The taper step the seat line is given for, degrees (#681): the bench's
/// measured ±0.5°.
pub const SEAT_TAPER_STEP_DEG: f64 = 0.5;

/// The tip-radius step the seat line is given for, mm (#681).
pub const SEAT_TIP_STEP_MM: f64 = 0.05;

/// How far an Inlay's plug seat moves for an error in its tapered ball
/// nose's numbers (#681; design spec, "The taper's ±0.5°, and
/// calibration"): information for the test fit, not a fault.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SeatSensitivity {
    /// How far the seat moves per [`SEAT_TAPER_STEP_DEG`] of taper, mm.
    pub taper_mm: f64,
    /// How far the seat moves per [`SEAT_TIP_STEP_MM`] of tip radius, mm.
    pub tip_mm: f64,
}

/// The seat's sensitivity for an Inlay carved `depth_mm` (`D`) deep with
/// a plug `plug_depth_mm` (`P`) by `profile`; `None` for a V-bit, or a
/// value that is not usable. The plug sinks by `(Δw(P) + Δw(D)) / t`
/// where, on the cone, `Δw(d) = ((d + δ)/cos² β − ρ / sin β)·Δβ` for the
/// taper and `t·Δδ` for the tip radius, with `Δδ = (1/sin β − 1)·Δρ`.
#[must_use]
pub fn seat_sensitivity(
    profile: &BitProfile,
    depth_mm: f64,
    plug_depth_mm: f64,
) -> Option<SeatSensitivity> {
    let BitProfile::TaperedBall {
        tip_radius_mm: rho,
        slope: t,
        ..
    } = *profile
    else {
        return None;
    };
    let beta = t.atan();
    let (sin, cos) = beta.sin_cos();
    let delta = profile.apex_offset_mm();
    let step_rad = SEAT_TAPER_STEP_DEG.to_radians();
    let dw = |d: f64| ((d + delta) / (cos * cos) - rho / sin) * step_rad;
    let taper_mm = (dw(plug_depth_mm) + dw(depth_mm)) / t;
    let d_delta = (1.0 / sin - 1.0) * SEAT_TIP_STEP_MM;
    let tip_mm = (t * d_delta + t * d_delta) / t;
    (taper_mm.is_finite() && tip_mm.is_finite()).then_some(SeatSensitivity { taper_mm, tip_mm })
}

/// What a glue gap below a tapered ball nose's tangent height costs
/// (`inlay_glue_gap_below_ball`, #681).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GlueGapBelowBall {
    /// The bit's tangent height `h_t`, mm.
    pub tangent_height_mm: f64,
    /// How far the plug stands proud, its top edge riding on the pocket's
    /// fillet, mm: `e` with `w_ball(G + e) = (G + δ)·t`.
    pub lift_mm: f64,
    /// How far its walls open at the surface, mm: `e·t`.
    pub wall_gap_mm: f64,
}

/// [`GlueGapBelowBall`] when `glue_gap_mm` (`G`) is below `profile`'s
/// tangent height; `None` for a V-bit, or a gap at or above it.
#[must_use]
pub fn glue_gap_below_ball(profile: &BitProfile, glue_gap_mm: f64) -> Option<GlueGapBelowBall> {
    let BitProfile::TaperedBall {
        tip_radius_mm: rho,
        slope: t,
        ..
    } = *profile
    else {
        return None;
    };
    let tangent_height_mm = profile.tangent_height_mm();
    if !(glue_gap_mm.is_finite() && glue_gap_mm >= 0.0 && glue_gap_mm < tangent_height_mm) {
        return None;
    }
    // The ball's height at the half-width the cone has at `G`: the smaller
    // root of `x² − 2ρx + c² = 0`.
    let c = (glue_gap_mm + profile.apex_offset_mm()) * t;
    let x = rho - (rho * rho - c * c).max(0.0).sqrt();
    let lift_mm = (x - glue_gap_mm).max(0.0);
    Some(GlueGapBelowBall {
        tangent_height_mm,
        lift_mm,
        wall_gap_mm: lift_mm * t,
    })
}

/// What a plug depth below a tapered ball nose's tangent height costs
/// (`inlay_plug_depth_below_ball`, #681).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlugDepthBelowBall {
    /// The bit's tangent height `h_t`, mm.
    pub tangent_height_mm: f64,
    /// How much narrower than the pocket the plug's wall is just below the
    /// surface, at most, mm: `(P + δ)·t − w(P)`.
    pub wall_gap_mm: f64,
}

/// [`PlugDepthBelowBall`] when `plug_depth_mm` (`P`) is below `profile`'s
/// tangent height; `None` for a V-bit, or a depth at or above it.
#[must_use]
pub fn plug_depth_below_ball(
    profile: &BitProfile,
    plug_depth_mm: f64,
) -> Option<PlugDepthBelowBall> {
    let BitProfile::TaperedBall { slope: t, .. } = *profile else {
        return None;
    };
    let tangent_height_mm = profile.tangent_height_mm();
    if !(plug_depth_mm.is_finite() && plug_depth_mm > 0.0 && plug_depth_mm < tangent_height_mm) {
        return None;
    }
    Some(PlugDepthBelowBall {
        tangent_height_mm,
        wall_gap_mm: (plug_depth_mm + profile.apex_offset_mm()) * t
            - profile.half_width_mm(plug_depth_mm),
    })
}

#[cfg(test)]
mod tests;
