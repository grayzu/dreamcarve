// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/cornerRelief.ts
//   src/engine/toolpaths/cornerRelief.test.ts

//! Corner relief (`docs/specs/phase-2/design.md`, "Algorithms → Corner
//! relief (dc-cam::relief)"; functional spec capability 18). Stubbed until
//! the implementation commit.

use crate::error::{CamError, ShapeRef};
use crate::profile::Wall;
use crate::tabs::LapRates;
use crate::toolpath::Toolpath;
use crate::warning::Warning;
use dc_geometry::{Contour, Point, Polyline, Region, Vec2};
use dc_project::{CornerRelief, OperationId, Side, Tool};

/// The smallest turn toward the cleared side at one vertex that counts as
/// a corner.
pub const RELIEF_MIN_TURN_RAD: f64 = std::f64::consts::PI / 12.0;

/// Position tolerance, mm.
pub const RELIEF_POSITION_TOLERANCE_MM: f64 = 1e-3;

/// The two relief styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Style {
    /// Along the bisector.
    Dogbone,
    /// Along the longer edge.
    TBone,
}

impl Style {
    /// The style `relief` asks for.
    pub(crate) const fn of(relief: CornerRelief) -> Option<Self> {
        match relief {
            CornerRelief::None => None,
            CornerRelief::Dogbone => Some(Self::Dogbone),
            CornerRelief::TBone => Some(Self::TBone),
        }
    }
}

/// One corner of a cleared loop.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CornerGeometry {
    pub corner: Point,
    pub edge_a: Vec2,
    pub edge_b: Vec2,
    pub length_a_mm: f64,
    pub length_b_mm: f64,
    pub bisector: Vec2,
    pub interior_angle_rad: f64,
}

/// One closed boundary loop of the cleared region.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReliefLoop {
    pub polyline: Polyline,
    pub cleared_inside: bool,
    pub shape: ShapeRef,
}

/// The loops of one region and its tool-centre walls.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct Site {
    pub loops: Vec<ReliefLoop>,
    pub walls: Vec<Polyline>,
}

/// A corner to relieve.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReliefCorner {
    pub geometry: CornerGeometry,
    pub descend: Point,
    pub end: Point,
    pub shape: ShapeRef,
}

/// Where the excursion ends.
pub(crate) fn excursion_end(geometry: &CornerGeometry, _style: Style, _distance_mm: f64) -> Point {
    geometry.corner
}

/// The corners of `site`.
pub(crate) fn collect(
    _op: &OperationId,
    _style: Style,
    _radius_mm: f64,
    _site: &Site,
    _tolerance_mm: f64,
) -> (Vec<ReliefCorner>, Vec<Warning>) {
    (Vec::new(), Vec::new())
}

/// Whether the main path cut at `point` at or below `max_z_mm`.
pub(crate) fn main_path_cuts_at(
    _toolpath: &Toolpath,
    _point: Point,
    _max_z_mm: f64,
    _tolerance_mm: f64,
) -> bool {
    false
}

/// What the relief pass needs.
#[derive(Debug, Clone)]
pub(crate) struct Relief {
    pub op: OperationId,
    pub style: Style,
    pub radius_mm: f64,
    pub levels: Vec<f64>,
    pub safe_z_mm: f64,
    pub rates: LapRates,
    pub tolerance_mm: f64,
}

/// The settings a generator hands over.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Setup<'a> {
    pub op: &'a OperationId,
    pub relief: CornerRelief,
    pub tool: &'a Tool,
    pub thickness_mm: f64,
    pub depth_mm: f64,
    pub through_cut_allowance_mm: f64,
    pub safe_z_mm: f64,
    pub rates: LapRates,
    pub tolerance_mm: f64,
}

impl Setup<'_> {
    /// The relief pass.
    pub(crate) fn plan(&self, _warnings: &mut Vec<Warning>) -> Result<Option<Relief>, CamError> {
        Ok(None)
    }
}

impl Relief {
    /// Appends the relief pass.
    pub(crate) fn append(
        &self,
        _sites: &[Site],
        _toolpath: &mut Toolpath,
        _warnings: &mut Vec<Warning>,
    ) -> Result<Vec<ReliefCorner>, CamError> {
        Ok(Vec::new())
    }
}

/// The lobe a notch may remove.
pub(crate) fn lobe(_corner: &ReliefCorner, _radius_mm: f64, _tolerance_mm: f64) -> Vec<Region> {
    Vec::new()
}

/// The relief sites of a Profile.
pub(crate) fn profile_sites(
    _side: Side,
    _regions: &[Region],
    _shapes: &[(ShapeRef, Contour)],
    _paths: &[(Polyline, &ShapeRef)],
    _walls: &[(Wall, usize)],
) -> Vec<Site> {
    Vec::new()
}

/// A `relief_skipped` warning.
pub(crate) fn skipped(op: &OperationId, _relief: CornerRelief, _reason: &str) -> Warning {
    Warning::new(crate::warning::WarningCode::ReliefSkipped).with("op", op)
}

#[cfg(test)]
mod tests {
    //! Ported from PureCutCNC's `src/engine/toolpaths/cornerRelief.test.ts`
    //! (Copyright 2026 Franja (Frank) Povazanj, Apache-2.0), re-derived for
    //! Y-up, plus the DreamCarve acceptance 29 and S9 tests.
    //!
    //! As upstream, the central assertion is coverage of the emitted
    //! motion: the swept capsules of the cutting moves must contain the
    //! corner wedge, and the same model with the excursion ending short
    //! must not.

    use super::*;
    use crate::footprint::footprint;
    use crate::pocket::generate_pocket;
    use crate::profile::generate_profile;
    use crate::test_support::assert_safe;
    use crate::toolpath::{Move, MoveKind};
    use crate::warning::WarningCode;
    use dc_geometry::{
        Arc, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point3, Segment, Transform, Vertex,
        assert_approx_eq, difference, nest, point_segment_distance_mm,
    };
    use dc_project::test_support::sample_project;
    use dc_project::{
        ClearanceStrategy, CutDirection, DefinitionId, Entry, FeatureDefinition, FeatureInstance,
        InstanceId, MachiningOrder, Operation, PocketOp, ProfileOp, Project, Stock, TabPlacement,
        TabShape, TabSpec, ToolId, ToolKind, WallPass,
    };
    use std::collections::BTreeMap;
    use std::f64::consts::{FRAC_1_SQRT_2, PI, SQRT_2};

    /// Every drawn shape is placed this far from the stock origin in X
    /// and Y, so outside cuts stay on the stock.
    const O: f64 = 20.0;
    const THICKNESS_MM: f64 = 6.0;
    /// The 4 mm test tool's radius.
    const R: f64 = 2.0;
    const TEST_ANGLES: [f64; 7] = [150.0, 120.0, 90.0, 75.0, 60.0, 45.0, 30.0];

    fn approx(a: f64, b: f64, tolerance: f64) -> bool {
        (a - b).abs() <= tolerance
    }

    fn at(x: f64, y: f64) -> Point {
        Point::new(x + O, y + O)
    }

    fn shape() -> ShapeRef {
        ShapeRef {
            instance: InstanceId::new("i1"),
            instance_name: "Shape".into(),
            profile: 0,
        }
    }

    fn op_id() -> OperationId {
        OperationId::new("o1")
    }

    // ── Coverage model ──────────────────────────────────────────────────

    /// One cutting move: its path in XY and the Z at its two ends.
    type Cut = (Segment, f64, f64);

    /// The bulge of `mv` from `from`: 0 for a line.
    fn bulge_of(from: Point, mv: &Move) -> f64 {
        let (centre, ccw) = match mv.kind {
            MoveKind::ArcCcw { centre } => (centre, true),
            MoveKind::ArcCw { centre } => (centre, false),
            _ => return 0.0,
        };
        (Arc::from_ends(from, mv.to.xy(), centre, ccw).sweep_rad / 4.0).tan()
    }

    /// The cutting moves of `moves`, from `start`.
    fn cuts_from(start: Point3, moves: &[Move]) -> Vec<Cut> {
        let mut pos = start;
        let mut out = Vec::new();
        for mv in moves {
            if mv.is_cut() {
                let segment = Segment {
                    start: pos.xy(),
                    end: mv.to.xy(),
                    bulge: bulge_of(pos.xy(), mv),
                };
                out.push((segment, pos.z, mv.to.z));
            }
            pos = mv.to;
        }
        out
    }

    fn cuts(toolpath: &Toolpath) -> Vec<Cut> {
        match toolpath.moves.first() {
            Some(first) => cuts_from(first.to, &toolpath.moves),
            None => Vec::new(),
        }
    }

    fn line(a: Point, b: Point, z: f64) -> Cut {
        (
            Segment {
                start: a,
                end: b,
                bulge: 0.0,
            },
            z,
            z,
        )
    }

    /// True when a flat cutter of `radius` sweeping `cuts` removes the
    /// material at `point` at Z `z`: a cut at or below `z` clears the
    /// column above it too.
    fn swept_covers(point: Point, cuts: &[Cut], radius: f64, z: f64) -> bool {
        let tolerance = radius * 1e-6;
        cuts.iter().any(|(segment, z0, z1)| {
            *z0 <= z + 1e-9
                && *z1 <= z + 1e-9
                && point_segment_distance_mm(*segment, point) <= radius + tolerance
        })
    }

    /// Points strictly inside the corner wedge within `radius` of the
    /// corner: what a mating square part needs removed.
    fn wedge_samples(geometry: &CornerGeometry, radius: f64) -> Vec<Point> {
        let base = geometry.edge_a.y.atan2(geometry.edge_a.x);
        let towards_b = geometry.edge_a.cross(geometry.bisector).signum();
        let mut samples = Vec::new();
        let mut fraction = 0.02;
        while fraction <= 0.98 {
            let angle = base + towards_b * geometry.interior_angle_rad * fraction;
            let mut depth = 0.1;
            while depth <= 1.0001 {
                samples
                    .push(geometry.corner + Vec2::new(angle.cos(), angle.sin()) * (radius * depth));
                depth += 0.1;
            }
            fraction += 0.06;
        }
        samples
    }

    /// A corner at the origin opening symmetrically about +X, and the
    /// wall contour a miter offset by `radius` gives: two legs meeting at
    /// the descend point.
    fn synthetic_corner(angle_deg: f64, radius: f64) -> (CornerGeometry, Vec<Cut>, Point) {
        let half = angle_deg.to_radians() / 2.0;
        let edge_a = Vec2::new(half.cos(), half.sin());
        let edge_b = Vec2::new(half.cos(), -half.sin());
        let geometry = CornerGeometry {
            corner: Point::new(0.0, 0.0),
            edge_a,
            edge_b,
            length_a_mm: 40.0,
            length_b_mm: 40.0,
            bisector: Vec2::new(1.0, 0.0),
            interior_angle_rad: 2.0 * half,
        };
        let descend = Point::new(radius / half.sin(), 0.0);
        let walls = vec![
            line(descend + edge_a * 40.0, descend, 0.0),
            line(descend, descend + edge_b * 40.0, 0.0),
        ];
        (geometry, walls, descend)
    }

    /// The right-angled corner at `corner` whose edges run along +X·`sx`
    /// and +Y·`sy`.
    fn right_corner(
        corner: Point,
        sx: f64,
        sy: f64,
        length_x: f64,
        length_y: f64,
    ) -> CornerGeometry {
        CornerGeometry {
            corner,
            edge_a: Vec2::new(sx, 0.0),
            edge_b: Vec2::new(0.0, sy),
            length_a_mm: length_x,
            length_b_mm: length_y,
            bisector: Vec2::new(sx * FRAC_1_SQRT_2, sy * FRAC_1_SQRT_2),
            interior_angle_rad: PI / 2.0,
        }
    }

    /// Distance from `p` to the line through `corner` along `edge`.
    fn off_line(edge: Vec2, p: Point, corner: Point) -> f64 {
        edge.cross(p - corner).abs()
    }

    // ── Project fixtures ────────────────────────────────────────────────

    fn tool(id: &str, diameter_mm: f64, step_down_mm: Option<f64>) -> Tool {
        Tool {
            id: ToolId::new(id),
            name: format!("{diameter_mm} mm end mill"),
            number: None,
            cutter: ToolKind::EndMill,
            diameter_mm,
            flutes: 2,
            feed_mm_min: Some(800.0),
            plunge_mm_min: Some(300.0),
            rpm: Some(18000),
            step_down_mm,
            step_over_fraction: Some(0.4),
        }
    }

    /// A closed polyline of lines through `points`, offset by (`O`, `O`).
    fn poly(points: &[(f64, f64)]) -> Polyline {
        Polyline::new(
            points
                .iter()
                .map(|&(x, y)| Vertex::line(x + O, y + O))
                .collect(),
            true,
        )
    }

    fn rect(width: f64, height: f64) -> Polyline {
        poly(&[(0.0, 0.0), (width, 0.0), (width, height), (0.0, height)])
    }

    /// A `size` square with all four corners filleted by `radius`.
    fn filleted_square(size: f64, radius: f64) -> Polyline {
        let b = (PI / 8.0).tan();
        let v = |x: f64, y: f64, bulge: f64| Vertex::new(x + O, y + O, bulge);
        Polyline::new(
            vec![
                v(radius, 0.0, 0.0),
                v(size - radius, 0.0, b),
                v(size, radius, 0.0),
                v(size, size - radius, b),
                v(size - radius, size, 0.0),
                v(radius, size, b),
                v(0.0, size - radius, 0.0),
                v(0.0, radius, b),
            ],
            true,
        )
    }

    const L_PART: [(f64, f64); 6] = [
        (0.0, 0.0),
        (60.0, 0.0),
        (60.0, 60.0),
        (30.0, 60.0),
        (30.0, 30.0),
        (0.0, 30.0),
    ];

    /// A 200 × 200 × 6 mm project with one instance of `profiles`.
    fn project(profiles: Vec<Polyline>, tools: Vec<Tool>) -> Project {
        let mut p = sample_project();
        p.stock = Stock {
            width_mm: 200.0,
            height_mm: 200.0,
            thickness_mm: Some(THICKNESS_MM),
        };
        let def = FeatureDefinition {
            id: DefinitionId::new("d1"),
            name: "Shape".into(),
            profiles,
        };
        p.definitions = BTreeMap::from([(def.id.clone(), def)]);
        p.instances = vec![FeatureInstance {
            id: InstanceId::new("i1"),
            definition: DefinitionId::new("d1"),
            transform: Transform::IDENTITY,
            name: "Shape".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(THICKNESS_MM),
            z_bottom_mm: Some(0.0),
        }];
        p.tools = tools;
        p.operations.clear();
        p
    }

    fn one_tool(profiles: Vec<Polyline>) -> Project {
        project(profiles, vec![tool("t1", 4.0, Some(2.0))])
    }

    /// An offset Pocket of the instance, 2 mm deep, with `relief`.
    fn pocket_op(relief: CornerRelief) -> PocketOp {
        PocketOp {
            id: op_id(),
            name: "Pocket".into(),
            enabled: true,
            targets: vec![InstanceId::new("i1")],
            tool: ToolId::new("t1"),
            strategy: ClearanceStrategy::Offset,
            wall_pass: WallPass::Last,
            depth_mm: Some(2.0),
            step_down_mm: None,
            step_over_fraction: Some(0.4),
            direction: CutDirection::Climb,
            allowance_mm: 0.0,
            rest_of: None,
            entry: Entry::Plunge,
            corner_relief: relief,
            machining_order: MachiningOrder::FullDepthFirst,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
        }
    }

    /// A Profile of the instance on `side`, 2 mm deep, with `relief`.
    fn profile_op(side: Side, relief: CornerRelief) -> ProfileOp {
        ProfileOp {
            id: op_id(),
            name: "Profile".into(),
            enabled: true,
            targets: vec![InstanceId::new("i1")],
            tool: ToolId::new("t1"),
            side,
            depth_mm: Some(2.0),
            step_down_mm: None,
            direction: CutDirection::Climb,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
            tabs: None,
            machining_order: MachiningOrder::FullDepthFirst,
            entry: Entry::Plunge,
            lead: None,
            corner_relief: relief,
        }
    }

    /// Generates `op` with its relief and without it, checks the one with
    /// [`assert_safe`] and that the relief left the main path untouched,
    /// and returns the toolpath with relief, its warnings and the relief
    /// cutting moves alone.
    fn pocket_run(project: &Project, op: &PocketOp) -> (Toolpath, Vec<Warning>, Vec<Cut>) {
        let (with, warnings) = generate_pocket(project, op).unwrap();
        let none = PocketOp {
            corner_relief: CornerRelief::None,
            ..op.clone()
        };
        let (without, _) = generate_pocket(project, &none).unwrap();
        assert_safe(&with, project, &Operation::Pocket(op.clone()));
        let relief = relief_cuts(&with, &without);
        (with, warnings, relief)
    }

    fn profile_run(project: &Project, op: &ProfileOp) -> (Toolpath, Vec<Warning>, Vec<Cut>) {
        let (with, warnings) = generate_profile(project, op).unwrap();
        let none = ProfileOp {
            corner_relief: CornerRelief::None,
            ..op.clone()
        };
        let (without, _) = generate_profile(project, &none).unwrap();
        assert_safe(&with, project, &Operation::Profile(op.clone()));
        let relief = relief_cuts(&with, &without);
        (with, warnings, relief)
    }

    /// The XY cutting moves `with` adds after `without`, which must be
    /// its prefix: relief is appended after the main path.
    fn relief_cuts(with: &Toolpath, without: &Toolpath) -> Vec<Cut> {
        let n = without.moves.len();
        assert_eq!(
            with.moves.get(..n),
            Some(without.moves.as_slice()),
            "relief must leave the main path untouched"
        );
        let Some(last) = without.moves.last() else {
            return cuts(with);
        };
        cuts_from(last.to, &with.moves[n..])
            .into_iter()
            .filter(|(s, _, _)| s.start.distance_to(s.end) > EPS_MM)
            .collect()
    }

    /// The outward excursions: every other relief cut, from the first.
    fn outward(relief: &[Cut]) -> Vec<Segment> {
        relief.iter().step_by(2).map(|(s, _, _)| *s).collect()
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(|w| w.code).collect()
    }

    fn relief_codes(warnings: &[Warning]) -> Vec<WarningCode> {
        codes(warnings)
            .into_iter()
            .filter(|c| {
                matches!(
                    c,
                    WarningCode::ReliefCornerTooTight
                        | WarningCode::ReliefCornerNotCut
                        | WarningCode::ReliefSkipped
                )
            })
            .collect()
    }

    /// The four corners of a `w` × `h` rectangle at (`O`, `O`), each with
    /// its inward edge directions, the X-parallel edge first.
    fn rect_corners(w: f64, h: f64) -> [CornerGeometry; 4] {
        [
            right_corner(at(0.0, 0.0), 1.0, 1.0, w, h),
            right_corner(at(w, 0.0), -1.0, 1.0, w, h),
            right_corner(at(w, h), -1.0, -1.0, w, h),
            right_corner(at(0.0, h), 1.0, -1.0, w, h),
        ]
    }

    /// Asserts every corner wedge of `corners` is swept at `z` by `cuts`.
    fn assert_wedges_cleared(corners: &[CornerGeometry], cuts: &[Cut], radius: f64, z: f64) {
        for g in corners {
            for p in wedge_samples(g, radius) {
                assert!(
                    swept_covers(p, cuts, radius, z),
                    "({:.3}, {:.3}) uncut near corner {:?}",
                    p.x,
                    p.y,
                    g.corner
                );
            }
        }
    }

    // ── 1. The endpoint rule ────────────────────────────────────────────

    /// Upstream: "dogbone ends on the bisector at exactly the cutter
    /// radius".
    #[test]
    fn dogbone_ends_on_bisector_at_radius() {
        for angle in TEST_ANGLES {
            let (g, _, _) = synthetic_corner(angle, 3.0);
            let end = excursion_end(&g, Style::Dogbone, 3.0);
            assert_approx_eq!(end.distance_to(g.corner), 3.0, 1e-9);
            assert_approx_eq!(end.y, 0.0, 1e-9);
        }
    }

    /// Upstream: "t-bone ends on an adjacent edge" and "t-bone takes the
    /// X-parallel edge; longest_edge takes the longer one". DreamCarve's
    /// T-bone is VCarve's: the longer edge, the X-parallel one on a tie.
    #[test]
    fn t_bone_ends_on_longer_edge_at_radius() {
        for angle in TEST_ANGLES {
            let (g, _, _) = synthetic_corner(angle, 3.0);
            let end = excursion_end(&g, Style::TBone, 3.0);
            assert_approx_eq!(end.distance_to(g.corner), 3.0, 1e-9);
            let on_a = off_line(g.edge_a, end, g.corner) < 1e-9;
            let on_b = off_line(g.edge_b, end, g.corner) < 1e-9;
            assert!(on_a || on_b, "t-bone at {angle}° ends on an edge");
        }
        // +X is the short edge, +Y the long one: the slot goes along +Y.
        let g = right_corner(Point::new(0.0, 0.0), 1.0, 1.0, 10.0, 40.0);
        let end = excursion_end(&g, Style::TBone, 2.0);
        assert!(
            approx(end.x, 0.0, 1e-9) && approx(end.y, 2.0, 1e-9),
            "{end:?}"
        );
        // Equal edges: the X-parallel one.
        let g = right_corner(Point::new(0.0, 0.0), 1.0, 1.0, 40.0, 40.0);
        let end = excursion_end(&g, Style::TBone, 2.0);
        assert!(
            approx(end.x, 2.0, 1e-9) && approx(end.y, 0.0, 1e-9),
            "{end:?}"
        );
        // The same corner with its edges the other way round.
        let swapped = CornerGeometry {
            edge_a: g.edge_b,
            edge_b: g.edge_a,
            ..g
        };
        assert_eq!(excursion_end(&swapped, Style::TBone, 2.0), end);
    }

    /// Upstream: "notch depth past the wall is r(1 − sin(θ/2)) for a
    /// dogbone, r for a t-bone", and `r(1 − sin θ)` into the other wall.
    #[test]
    fn notch_depth_past_wall() {
        let r = 3.0;
        for angle in TEST_ANGLES {
            let (g, _, _) = synthetic_corner(angle, r);
            let half = g.interior_angle_rad / 2.0;
            let bite = |end: Point, edge: Vec2| r - off_line(edge, end, g.corner);
            let dogbone = excursion_end(&g, Style::Dogbone, r);
            assert_approx_eq!(bite(dogbone, g.edge_a), r * (1.0 - half.sin()), 1e-9);
            let t_bone = excursion_end(&g, Style::TBone, r);
            let on_a = off_line(g.edge_a, t_bone, g.corner) < 1e-9;
            let (chosen, other) = if on_a {
                (g.edge_a, g.edge_b)
            } else {
                (g.edge_b, g.edge_a)
            };
            assert_approx_eq!(bite(t_bone, chosen), r, 1e-9);
            assert_approx_eq!(
                bite(t_bone, other),
                (r * (1.0 - g.interior_angle_rad.sin())).max(0.0),
                1e-9
            );
        }
    }

    /// Upstream: "at a right angle the t-bone leaves the perpendicular
    /// wall untouched".
    #[test]
    fn right_angle_t_bone_leaves_perpendicular_wall() {
        let r = 3.0;
        let (g, _, _) = synthetic_corner(90.0, r);
        let end = excursion_end(&g, Style::TBone, r);
        let on_a = off_line(g.edge_a, end, g.corner) < 1e-9;
        let other = if on_a { g.edge_b } else { g.edge_a };
        assert!(r - off_line(other, end, g.corner) <= 1e-9);
    }

    // ── 2. Coverage: touch clears, ending short does not ────────────────

    /// The wall legs and an excursion from `descend` to `end` and back.
    fn excursion_cuts(walls: &[Cut], descend: Point, end: Point) -> Vec<Cut> {
        let mut all = walls.to_vec();
        all.push(line(descend, end, 0.0));
        all.push(line(end, descend, 0.0));
        all
    }

    /// Upstream: "{style} at r clears the whole wedge at every angle", and
    /// "without relief the wall contour alone always leaves the corner
    /// uncut" (else the coverage test proves nothing).
    #[test]
    fn clears_whole_wedge_at_every_angle() {
        let r = 3.0;
        for style in [Style::Dogbone, Style::TBone] {
            for angle in TEST_ANGLES {
                let (g, walls, descend) = synthetic_corner(angle, r);
                let end = excursion_end(&g, style, r);
                let all = excursion_cuts(&walls, descend, end);
                for p in wedge_samples(&g, r) {
                    assert!(
                        swept_covers(p, &all, r, 0.0),
                        "{style:?} at {angle}° left {p:?} uncut"
                    );
                }
                let uncut = wedge_samples(&g, r)
                    .into_iter()
                    .any(|p| !swept_covers(p, &walls, r, 0.0));
                assert!(uncut, "the {angle}° corner is uncut before relief");
            }
        }
    }

    /// Upstream: "{style} stopping short at 1.2r leaves the corner uncut"
    /// (a T-bone only up to 90°: past about 105° its excursion runs across
    /// the corner whatever its length), and "a very obtuse t-bone clears
    /// even when short".
    #[test]
    fn stopping_short_leaves_corner() {
        let r = 3.0;
        for style in [Style::Dogbone, Style::TBone] {
            for angle in TEST_ANGLES {
                if style == Style::TBone && angle > 90.0 {
                    continue;
                }
                let (g, walls, descend) = synthetic_corner(angle, r);
                let short = excursion_end(&g, style, r * 1.2);
                let all = excursion_cuts(&walls, descend, short);
                let uncut = wedge_samples(&g, r)
                    .into_iter()
                    .any(|p| !swept_covers(p, &all, r, 0.0));
                assert!(uncut, "{style:?} at {angle}° clears the wedge from 1.2r");
            }
        }
        let (g, walls, descend) = synthetic_corner(150.0, r);
        let short = excursion_end(&g, Style::TBone, r * 1.2);
        let all = excursion_cuts(&walls, descend, short);
        assert!(
            wedge_samples(&g, r)
                .into_iter()
                .all(|p| swept_covers(p, &all, r, 0.0))
        );
    }

    // ── 3. Corner detection ─────────────────────────────────────────────

    fn site(loops: Vec<(Polyline, bool)>, walls: Vec<Polyline>) -> Site {
        Site {
            loops: loops
                .into_iter()
                .map(|(polyline, cleared_inside)| ReliefLoop {
                    polyline,
                    cleared_inside,
                    shape: shape(),
                })
                .collect(),
            walls,
        }
    }

    fn square_site() -> Site {
        site(
            vec![(rect(40.0, 40.0), true)],
            vec![poly(&[(2.0, 2.0), (38.0, 2.0), (38.0, 38.0), (2.0, 38.0)])],
        )
    }

    /// Upstream: "a square boundary yields four corners; its wall path
    /// yields the descend points".
    #[test]
    fn square_pocket_yields_four_corners_and_descend_points() {
        let (corners, warnings) = collect(&op_id(), Style::Dogbone, R, &square_site(), 0.01);
        assert_eq!(corners.len(), 4);
        assert!(warnings.is_empty(), "{warnings:?}");
        for c in &corners {
            let miter = R / (c.geometry.interior_angle_rad / 2.0).sin();
            assert_approx_eq!(c.descend.distance_to(c.geometry.corner), miter, 1e-6);
            assert_approx_eq!(c.end.distance_to(c.geometry.corner), R, 1e-9);
        }
    }

    /// Upstream: "a cleared-outside loop relieves its concave corners
    /// only".
    #[test]
    fn cleared_outside_loop_relieves_concave_only() {
        let wall = poly(&[
            (28.0, 32.0),
            (28.0, 62.0),
            (62.0, 62.0),
            (62.0, -2.0),
            (-2.0, -2.0),
            (-2.0, 32.0),
        ]);
        let (corners, warnings) = collect(
            &op_id(),
            Style::Dogbone,
            R,
            &site(vec![(poly(&L_PART), false)], vec![wall]),
            0.01,
        );
        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(corners.len(), 1);
        assert!(corners[0].geometry.corner.distance_to(at(30.0, 30.0)) < 1e-9);
        assert!(corners[0].descend.distance_to(at(28.0, 32.0)) < 1e-6);
    }

    /// Upstream: "a corner whose adjacent edges cannot hold the notch is
    /// rejected, with a warning naming it".
    #[test]
    fn too_tight_corner_rejected_with_warning() {
        let notch = poly(&[
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (37.0, 20.0),
            (37.0, 23.0),
            (0.0, 23.0),
        ]);
        let (_, warnings) = collect(
            &op_id(),
            Style::Dogbone,
            4.0,
            &site(vec![(notch.clone(), true)], vec![notch]),
            0.01,
        );
        let tight: Vec<&Warning> = warnings
            .iter()
            .filter(|w| w.code == WarningCode::ReliefCornerTooTight)
            .collect();
        assert!(!tight.is_empty());
        for w in tight {
            assert!(
                w.param("x_mm").is_some() && w.param("y_mm").is_some(),
                "{w:?}"
            );
            assert_eq!(w.param("op"), Some("o1"));
            assert_eq!(w.param("instance"), Some("i1"));
        }
    }

    /// Upstream: "style 'none' collects nothing at all" and "'none' and a
    /// legacy operation with the field absent are identical to today":
    /// `none` has no style, and a dogbone Pocket is the `none` Pocket with
    /// the relief appended ([`relief_cuts`] checks the prefix).
    #[test]
    fn none_collects_nothing() {
        assert_eq!(Style::of(CornerRelief::None), None);
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let (_, warnings) = generate_pocket(&p, &pocket_op(CornerRelief::None)).unwrap();
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let (_, _, relief) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
        assert!(!relief.is_empty(), "a real style changes the toolpath");
    }

    // ── 4. The step-down and the descend guard ──────────────────────────

    /// Upstream: "the guard reads fed moves at or below the level, and
    /// ignores rapids and plunges".
    #[test]
    fn main_path_cuts_at_reads_cuts_at_or_below_the_level() {
        let path = |from: (f64, f64, f64), mv: Move| Toolpath {
            moves: vec![Move::rapid(Point3::new(from.0, from.1, from.2)), mv],
        };
        let cut = |x: f64, y: f64, z: f64| Move::linear(Point3::new(x, y, z), 800.0);
        let p = Point::new(5.0, 0.0);
        let tol = RELIEF_POSITION_TOLERANCE_MM;
        let level = |z: f64| path((0.0, 0.0, z), cut(10.0, 0.0, z));
        assert!(main_path_cuts_at(&level(-5.0), p, -5.0, tol));
        assert!(main_path_cuts_at(&level(-6.0), p, -5.0, tol));
        assert!(!main_path_cuts_at(&level(-4.0), p, -5.0, tol));
        let rapid = Move::rapid(Point3::new(10.0, 0.0, -5.0));
        assert!(!main_path_cuts_at(
            &path((0.0, 0.0, -5.0), rapid),
            p,
            -5.0,
            tol
        ));
        let plunge = cut(0.0, 0.0, -5.0);
        assert!(!main_path_cuts_at(
            &path((0.0, 0.0, 5.0), plunge),
            Point::new(0.0, 0.0),
            -5.0,
            tol
        ));
        assert!(!main_path_cuts_at(
            &level(-5.0),
            Point::new(5.0, 1.0),
            -5.0,
            tol
        ));
        // An arc through the point counts.
        let arc = Move {
            kind: MoveKind::ArcCcw {
                centre: Point::new(0.0, 0.0),
            },
            to: Point3::new(0.0, 5.0, -5.0),
            feed_mm_min: Some(800.0),
        };
        let on_arc = Point::new(5.0 * FRAC_1_SQRT_2, 5.0 * FRAC_1_SQRT_2);
        assert!(main_path_cuts_at(
            &path((5.0, 0.0, -5.0), arc),
            on_arc,
            -5.0,
            tol
        ));
    }

    fn relief_for(levels: Vec<f64>) -> Relief {
        Relief {
            op: op_id(),
            style: Style::Dogbone,
            radius_mm: R,
            levels,
            safe_z_mm: 11.0,
            rates: LapRates {
                feed_mm_min: 800.0,
                plunge_mm_min: 300.0,
            },
            tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
        }
    }

    /// Upstream: "a corner the main path never cut emits no motion and
    /// one warning".
    #[test]
    fn uncut_corner_emits_no_motion_and_warns() {
        let mut toolpath = Toolpath::default();
        let mut warnings = Vec::new();
        let cut = relief_for(vec![4.0, 2.0])
            .append(&[square_site()], &mut toolpath, &mut warnings)
            .unwrap();
        assert!(cut.is_empty());
        assert!(toolpath.moves.is_empty());
        assert_eq!(codes(&warnings), vec![WarningCode::ReliefCornerNotCut; 4]);
        assert!(warnings.iter().all(|w| w.param("x_mm").is_some()));
    }

    // ── 5. Pocket ───────────────────────────────────────────────────────

    /// Upstream: "{style} on a square pocket clears all four corners of
    /// the emitted motion": four out-and-back pairs, every wedge swept.
    #[test]
    fn pocket_square_clears_all_four_corners_eight_cuts() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        for relief in [CornerRelief::Dogbone, CornerRelief::TBone] {
            let (toolpath, warnings, cut) = pocket_run(&p, &pocket_op(relief));
            assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
            assert_eq!(cut.len(), 8, "{relief:?}");
            assert_wedges_cleared(&rect_corners(40.0, 40.0), &cuts(&toolpath), R, 4.0);
        }
    }

    /// Upstream: "acute, right and obtuse corners of one pocket are all
    /// relieved"; the acute corner needs the longest excursion,
    /// `r(1/sin(θ/2) − 1)`.
    #[test]
    fn acute_right_and_obtuse_corners_are_relieved() {
        let p = one_tool(vec![poly(&[(0.0, 0.0), (60.0, 0.0), (0.0, 25.0)])]);
        let (_, warnings, cut) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        assert_eq!(cut.len(), 6);
        let mut lengths: Vec<f64> = outward(&cut).iter().map(Segment::length_mm).collect();
        lengths.sort_by(f64::total_cmp);
        let acute_half = 25.0f64.atan2(60.0) / 2.0;
        assert_approx_eq!(lengths[2], R * (1.0 / acute_half.sin() - 1.0), 1e-3);
        assert_approx_eq!(lengths[0], R * (SQRT_2 - 1.0), 1e-3);
    }

    /// Upstream: "reflex island corners are relieved and convex island
    /// corners are not".
    #[test]
    fn island_reflex_corners_relieved_convex_not() {
        let island = poly(&[
            (20.0, 20.0),
            (40.0, 20.0),
            (40.0, 40.0),
            (30.0, 40.0),
            (30.0, 30.0),
            (20.0, 30.0),
        ]);
        let p = one_tool(vec![rect(60.0, 60.0), island]);
        let (_, warnings, cut) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let out = outward(&cut);
        assert_eq!(
            out.len(),
            5,
            "four wall corners and the island's reflex one"
        );
        let near_island = out
            .iter()
            .filter(|s| s.start.distance_to(at(30.0, 30.0)) < 5.0)
            .count();
        assert_eq!(near_island, 1);
    }

    /// Upstream: "tessellated arcs and existing fillets produce zero
    /// relief": a circle and squares filleted at 1, 5 and 12 mm.
    #[test]
    fn arcs_and_fillets_produce_no_relief() {
        let mut drawn = vec![Polyline::circle(at(25.0, 25.0), 20.0)];
        drawn.extend([1.0, 5.0, 12.0].map(|r| filleted_square(40.0, r)));
        for shape in drawn {
            let p = one_tool(vec![shape]);
            let (_, warnings, cut) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
            assert!(cut.is_empty(), "{cut:?}");
            assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        }
    }

    /// Upstream: "the relief pass steps down at the tool stepdown, not at
    /// operation.stepdown".
    #[test]
    fn steps_down_at_tool_step_down() {
        let p = project(vec![rect(40.0, 40.0)], vec![tool("t1", 4.0, Some(1.5))]);
        let op = PocketOp {
            depth_mm: Some(6.0),
            step_down_mm: Some(6.0),
            ..pocket_op(CornerRelief::Dogbone)
        };
        let (_, _, cut) = pocket_run(&p, &op);
        let mut levels: Vec<f64> = cut.iter().map(|(_, z, _)| *z).collect();
        levels.sort_by(|a, b| b.total_cmp(a));
        levels.dedup_by(|a, b| approx(*a, *b, 1e-9));
        assert_eq!(levels.len(), 4, "{levels:?}");
        for pair in levels.windows(2) {
            assert!(pair[0] - pair[1] <= 1.5 + 1e-9);
        }
        assert_approx_eq!(levels[3], 0.0, 1e-9);
    }

    /// Upstream: "a tool with no stepdown fails the relief closed with a
    /// warning".
    #[test]
    fn no_step_down_skips_with_warning() {
        let p = project(vec![rect(40.0, 40.0)], vec![tool("t1", 4.0, None)]);
        let op = PocketOp {
            step_down_mm: Some(2.0),
            ..pocket_op(CornerRelief::Dogbone)
        };
        let (_, warnings, cut) = pocket_run(&p, &op);
        assert!(cut.is_empty());
        assert_eq!(relief_codes(&warnings), vec![WarningCode::ReliefSkipped]);
        let w = &warnings[0];
        assert_eq!(w.param("reason"), Some("no_step_down"));
        assert_eq!(w.param("requested"), Some("dogbone"));
    }

    /// Upstream: "a pocket finish with finishWalls off skips every corner
    /// and says which": with no wall pass the main path never reaches the
    /// descend points.
    #[test]
    fn wall_pass_none_skips_every_corner() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let op = PocketOp {
            wall_pass: WallPass::None,
            ..pocket_op(CornerRelief::Dogbone)
        };
        let (_, warnings, cut) = pocket_run(&p, &op);
        assert!(cut.is_empty());
        assert_eq!(
            relief_codes(&warnings),
            vec![WarningCode::ReliefCornerNotCut; 4]
        );
    }

    /// Upstream: "radial stock sizes the relief to the wall the pass
    /// actually leaves": the excursion ends one radius from the corner of
    /// the region inset by the allowance.
    #[test]
    fn allowance_sizes_relief_to_the_wall_left() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let op = PocketOp {
            allowance_mm: 0.5,
            ..pocket_op(CornerRelief::Dogbone)
        };
        let (_, warnings, cut) = pocket_run(&p, &op);
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let out = outward(&cut);
        assert_eq!(out.len(), 4);
        let cleared_corner = at(39.5, 39.5);
        let nearest = out
            .iter()
            .map(|s| s.end.distance_to(cleared_corner))
            .fold(f64::INFINITY, f64::min);
        assert_approx_eq!(nearest, R, 1e-3);
    }

    /// Upstream: "rough big with stock and no relief, then finish small
    /// with relief, clears the corner", the finish as DreamCarve's rest
    /// machining: a 10 mm rough at a 0.4 mm allowance, then a rest Pocket
    /// with a 3 mm bit and dogbones. Together they sweep the corner wedge,
    /// and the finish bites no further past a wall than the notch depth,
    /// `r(1 − sin 45°)`.
    #[test]
    fn rough_with_allowance_then_rest_with_relief_clears_corner() {
        let mut p = project(
            vec![rect(40.0, 40.0)],
            vec![
                tool("rough", 10.0, Some(3.0)),
                tool("finish", 3.0, Some(1.0)),
            ],
        );
        let rough = PocketOp {
            id: OperationId::new("rough"),
            tool: ToolId::new("rough"),
            depth_mm: Some(4.0),
            allowance_mm: 0.4,
            ..pocket_op(CornerRelief::None)
        };
        let finish = PocketOp {
            id: OperationId::new("finish"),
            tool: ToolId::new("finish"),
            depth_mm: Some(4.0),
            rest_of: Some(rough.id.clone()),
            ..pocket_op(CornerRelief::Dogbone)
        };
        p.operations = vec![
            Operation::Pocket(rough.clone()),
            Operation::Pocket(finish.clone()),
        ];
        let (rough_path, _) = generate_pocket(&p, &rough).unwrap();
        let (finish_path, warnings) = generate_pocket(&p, &finish).unwrap();
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        assert_safe(&finish_path, &p, &Operation::Pocket(finish.clone()));
        let mut all = cuts(&rough_path);
        all.extend(cuts(&finish_path));
        let r = 1.5;
        let corner = right_corner(at(40.0, 40.0), -1.0, -1.0, 40.0, 40.0);
        assert_wedges_cleared(&[corner], &all, r, THICKNESS_MM - 4.0);
        let depth = r * (1.0 - FRAC_1_SQRT_2);
        for (s, _, _) in cuts(&finish_path) {
            for q in [s.start, s.end] {
                let bite = r - (O + 40.0 - q.x).min(O + 40.0 - q.y);
                assert!(bite <= depth + 1e-6, "{q:?} bites {bite} past a wall");
            }
        }
    }

    /// Upstream: "the parallel pocket pattern relieves the same corners as
    /// the offset pattern".
    #[test]
    fn raster_relieves_the_same_corners_as_offset() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let ends = |strategy| {
            let op = PocketOp {
                strategy,
                ..pocket_op(CornerRelief::Dogbone)
            };
            let (_, _, cut) = pocket_run(&p, &op);
            let mut ends: Vec<String> = outward(&cut)
                .iter()
                .map(|s| format!("{:.4},{:.4}", s.end.x, s.end.y))
                .collect();
            ends.sort();
            ends
        };
        let offset = ends(ClearanceStrategy::Offset);
        assert_eq!(offset.len(), 4);
        assert_eq!(offset, ends(ClearanceStrategy::Raster { angle_deg: 0.0 }));
    }

    // ── 6. Profile ──────────────────────────────────────────────────────

    /// Upstream: "an inside edge route relieves the convex corners of its
    /// boundary".
    #[test]
    fn inside_profile_relieves_hole_corners() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let (toolpath, warnings, cut) =
            profile_run(&p, &profile_op(Side::Inside, CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        assert_eq!(cut.len(), 8);
        assert_wedges_cleared(&rect_corners(40.0, 40.0), &cuts(&toolpath), R, 4.0);
    }

    /// Upstream: "an outside edge route relieves concave part corners
    /// only": one notch in an L part, descending at the wall path's corner
    /// outside (30, 30), and none on a square part.
    #[test]
    fn outside_profile_relieves_concave_part_corners() {
        let p = one_tool(vec![poly(&L_PART)]);
        let (_, warnings, cut) = profile_run(&p, &profile_op(Side::Outside, CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let out = outward(&cut);
        assert_eq!(out.len(), 1);
        assert!(out[0].start.distance_to(at(28.0, 32.0)) < 1e-6, "{out:?}");

        let square = one_tool(vec![rect(40.0, 40.0)]);
        let (_, _, cut) = profile_run(&square, &profile_op(Side::Outside, CornerRelief::Dogbone));
        assert!(cut.is_empty(), "the cutter wraps a convex part's corners");
    }

    /// The distance along the Profile's lap at Z `z` from its start to
    /// `point`, where the lap passes through it.
    fn distance_along_lap(toolpath: &Toolpath, z: f64, point: Point) -> f64 {
        let mut along = 0.0;
        for (s, z0, z1) in cuts(toolpath) {
            if !(approx(z0, z, 1e-9) && approx(z1, z, 1e-9)) {
                continue;
            }
            if s.end.distance_to(point) < 1e-6 {
                return along + s.length_mm();
            }
            along += s.length_mm();
        }
        panic!("the lap at {z} does not pass through {point:?}");
    }

    /// Upstream: "a corner under a tab is skipped rather than plunged
    /// into" and "a tab clear of the corner leaves the relief in place".
    /// Keep-outs are dropped (design spec): the tab lifts the lap over the
    /// descend point, so the guard skips the corner.
    #[test]
    fn corner_under_tab_is_skipped() {
        let p = one_tool(vec![poly(&L_PART)]);
        let plain = profile_op(Side::Outside, CornerRelief::Dogbone);
        let (untabbed, _) = generate_profile(&p, &plain).unwrap();
        let d = distance_along_lap(&untabbed, 4.0, at(28.0, 32.0));
        let tabbed = |distance_mm: f64| ProfileOp {
            tabs: Some(TabSpec {
                placement: TabPlacement::AtDistances {
                    distances_mm: vec![distance_mm],
                },
                width_mm: 6.0,
                height_mm: 5.0,
                shape: TabShape::Rect,
            }),
            ..plain.clone()
        };
        let (_, warnings, cut) = profile_run(&p, &tabbed(d));
        assert!(cut.is_empty(), "{cut:?}");
        assert_eq!(
            relief_codes(&warnings),
            vec![WarningCode::ReliefCornerNotCut]
        );
        let (_, warnings, cut) = profile_run(&p, &tabbed(d + 40.0));
        assert_eq!(cut.len(), 2);
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
    }

    /// Decision: On line has no waste side for a notch, so the relief is
    /// skipped with a warning.
    #[test]
    fn on_line_profile_skips_with_warning() {
        let p = one_tool(vec![rect(40.0, 40.0)]);
        let (_, warnings, cut) = profile_run(&p, &profile_op(Side::OnLine, CornerRelief::TBone));
        assert!(cut.is_empty());
        assert_eq!(relief_codes(&warnings), vec![WarningCode::ReliefSkipped]);
        assert_eq!(warnings[0].param("reason"), Some("on_line"));
        assert_eq!(warnings[0].param("requested"), Some("t_bone"));
    }

    // ── 7. Acceptance 29 and S9 ─────────────────────────────────────────

    /// The 40 × 30 rectangle's corners' dogbone ends: one radius past each
    /// corner along its inward bisector.
    fn dogbone_ends_40x30() -> Vec<Point> {
        rect_corners(40.0, 30.0)
            .iter()
            .map(|g| g.corner + g.bisector * R)
            .collect()
    }

    /// Acceptance 29: a 40 × 30 mm pocket with dogbones gets four notches
    /// along the corner bisectors, after the main passes, each ending
    /// exactly one tool radius past the corner, so a 40 × 30 block fits.
    #[test]
    fn acceptance_29_pocket_dogbone_four_notches_on_bisectors() {
        let p = one_tool(vec![rect(40.0, 30.0)]);
        let (toolpath, warnings, cut) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let corners = rect_corners(40.0, 30.0);
        let out = outward(&cut);
        assert_eq!(out.len(), 4, "{out:?}");
        for g in &corners {
            let end = g.corner + g.bisector * R;
            assert_approx_eq!(end.distance_to(g.corner), R, 1e-12);
            let notch = out
                .iter()
                .find(|s| s.end.distance_to(end) < 1e-9)
                .unwrap_or_else(|| panic!("no notch ends at {end:?}: {out:?}"));
            // Inward along the bisector, toward the corner.
            let along = (notch.end - notch.start).normalized().unwrap();
            assert!(along.cross(g.bisector).abs() < 1e-9, "{notch:?}");
            assert!(along.dot(g.bisector) < 0.0, "{notch:?}");
        }
        assert_wedges_cleared(&corners, &cuts(&toolpath), R, 4.0);
    }

    /// Acceptance 29: `t_bone` puts each notch along the longer, 40 mm,
    /// edge: it ends one radius from the corner on that edge's line.
    #[test]
    fn acceptance_29_t_bone_along_the_40_mm_edges() {
        let p = one_tool(vec![rect(40.0, 30.0)]);
        let (toolpath, warnings, cut) = pocket_run(&p, &pocket_op(CornerRelief::TBone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let corners = rect_corners(40.0, 30.0);
        let out = outward(&cut);
        assert_eq!(out.len(), 4);
        for g in &corners {
            let end = g.corner + g.edge_a * R;
            assert!(
                out.iter().any(|s| s.end.distance_to(end) < 1e-9),
                "{end:?}: {out:?}"
            );
        }
        assert_wedges_cleared(&corners, &cuts(&toolpath), R, 4.0);
    }

    /// Acceptance 29: an inside Profile of the same rectangle gets the
    /// same four notches.
    #[test]
    fn acceptance_29_inside_profile_same_four() {
        let p = one_tool(vec![rect(40.0, 30.0)]);
        let (_, _, pocket_cut) = pocket_run(&p, &pocket_op(CornerRelief::Dogbone));
        let (_, warnings, cut) = profile_run(&p, &profile_op(Side::Inside, CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let key = |cut: &[Cut]| {
            let mut ends: Vec<String> = outward(cut)
                .iter()
                .map(|s| {
                    format!(
                        "{:.6},{:.6}->{:.6},{:.6}",
                        s.start.x, s.start.y, s.end.x, s.end.y
                    )
                })
                .collect();
            ends.sort();
            ends
        };
        assert_eq!(key(&cut).len(), 4);
        assert_eq!(key(&cut), key(&pocket_cut));
    }

    /// Acceptance 29: an outside Profile of an L-shaped part gets one
    /// notch, in its concave corner: from the miter (28, 32) to one radius
    /// up and left of (30, 30).
    #[test]
    fn acceptance_29_outside_l_part_one_notch() {
        let p = one_tool(vec![poly(&L_PART)]);
        let (_, warnings, cut) = profile_run(&p, &profile_op(Side::Outside, CornerRelief::Dogbone));
        assert!(relief_codes(&warnings).is_empty(), "{warnings:?}");
        let out = outward(&cut);
        assert_eq!(out.len(), 1);
        let want = at(30.0 - R * FRAC_1_SQRT_2, 30.0 + R * FRAC_1_SQRT_2);
        assert!(out[0].end.distance_to(want) < 1e-9, "{out:?}");
        assert!(out[0].start.distance_to(at(28.0, 32.0)) < 1e-6, "{out:?}");
    }

    /// S9: the notches cut past the pocket's walls only in their corners'
    /// lobes, within one radius of each dogbone end and at most the notch
    /// depth `r(1 − sin 45°)` past a wall, and the generator's own S9
    /// check accepted them.
    #[test]
    fn footprint_of_notch_stays_in_removed_material() {
        let p = one_tool(vec![rect(40.0, 30.0)]);
        let op = pocket_op(CornerRelief::Dogbone);
        let (with, _) = generate_pocket(&p, &op).unwrap();
        let none = PocketOp {
            corner_relief: CornerRelief::None,
            ..op.clone()
        };
        let (without, _) = generate_pocket(&p, &none).unwrap();
        let notches = Toolpath {
            moves: with.moves[without.moves.len() - 1..].to_vec(),
        };
        let swept = footprint(&notches, R);
        assert!(!swept.is_empty());
        let (pocket, _) = nest(vec![Contour::new(rect(40.0, 30.0)).unwrap()]).unwrap();
        let outside = difference(&swept, &pocket, FLATTEN_TOLERANCE_DEFAULT_MM);
        assert!(!outside.is_empty(), "the notches do cut past the walls");
        let ends = dogbone_ends_40x30();
        let depth = R * (1.0 - FRAC_1_SQRT_2);
        let tol = FLATTEN_TOLERANCE_DEFAULT_MM + 1e-6;
        for region in &outside {
            for v in region.outer().polyline().vertices() {
                let q = v.point();
                assert!(
                    ends.iter().any(|e| e.distance_to(q) <= R + tol),
                    "{q:?} is outside every corner's lobe"
                );
                let past = (O - q.x)
                    .max(q.x - O - 40.0)
                    .max(O - q.y)
                    .max(q.y - O - 30.0);
                assert!(past <= depth + tol, "{q:?} is {past} past a wall");
            }
        }
    }
}
