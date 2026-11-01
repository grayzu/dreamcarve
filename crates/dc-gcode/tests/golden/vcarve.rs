// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The V-Carve comparison (`docs/specs/phase-3/design.md`, "Algorithms →
//! Golden corpus"; `docs/specs/phase-3/functional.md`, acceptance test 49).
//!
//! A V-Carve's cuts move in X, Y and Z together, so they are compared in
//! three dimensions rather than level by level. [`compare`] reads both
//! programs' cuts in the machine frame they share and checks:
//!
//! 1. **Regions.** Every cut that moves in XY lies in one region of the
//!    carve (an outer outline less its holes); with [`Order::InOrder`] both
//!    programs visit the regions in the same order.
//! 2. **Chains or rings, as sets.** A medial carve: within each region,
//!    every point of every cut that moves in XY, sampled every
//!    [`SAMPLE_STEP_MM`], lies within [`ALLOWED_MM`] in XYZ of the other
//!    program's cuts in that region, both ways. Direction is not compared:
//!    DreamCarve starts a region at its shallowest chain end (#404,
//!    decision 3), so it may cut a chain the other way, which cuts the same
//!    groove. An offset carve: the level cuts, level by level, the same
//!    way, and travelling the same way (the rings' direction).
//! 3. **Depth.** Neither program cuts below the flat depth, and the
//!    deepest cut above it is the same in both.
//! 4. **The flat depth.** Cuts at the flat depth are not compared point for
//!    point: DreamCarve's floor trace and V-bit floor (#405) are its own,
//!    PureCutCNC clamps but has no floor. Every golden cut there must lie
//!    within the V-bit's flat-depth radius `F · tan(α/2)` of a DreamCarve
//!    cut at that depth, so DreamCarve's floor clears what upstream's
//!    clamped chains cut.
//! 5. **Links** are not compared. Upstream bridges chain ends that
//!    DreamCarve retracts between (security review H2), and an offset
//!    carve's ring links follow the rings' start points (#407). A medial
//!    cut off the other program's path is accepted as a link only when it
//!    is one straight move between two points where the other program
//!    lifts and plunges instead. An offset carve's links, which descend
//!    from level to level, are not compared at all.
//! 6. **Feeds.** Plunges and lifts run at the plunge rate in both
//!    programs; other cuts at the cutting feed, except as
//!    [`Difference::DescentsHeldToPlunge`] says.
//!
//! [`Difference::ChainDepthFitted`] accepts medial chains that differ in
//! depth at one XY when each program's point there is checked against the
//! walls instead: DreamCarve's lies on the V surface, and a golden point
//! deeper than it reaches past a wall.

use dc_gcode::parse::MotionKind;
use dc_geometry::medial::convex_corners;
use dc_geometry::{Point, Region, Segment, approx_eq, nearest_point, point_segment_distance_mm};
use dc_project::{Operation, Project, ToolKind, VCarveOp, VCarveStrategy};

use super::{
    Difference, Motion, Order, P3, Piece, ROUNDING_MM, SAME_MM, SAMPLE_STEP_MM, Z_ROUNDING_MM,
    tool_of,
};

/// How far apart two programs' V-Carve cuts may be, mm, in XYZ
/// (functional acceptance test 49).
pub const TOLERANCE_MM: f64 = 0.02;

/// [`TOLERANCE_MM`] plus each program's rounding of X, Y and Z to three
/// decimal places, mm.
pub const ALLOWED_MM: f64 = TOLERANCE_MM + 2.0 * 0.0005 * 1.732_050_808;

/// How far a point may sit off the V surface (the room between the cone
/// and the walls) from rounding alone, mm: the XY rounding, plus the Z
/// rounding through the cone's slope (at most 1 for the corpus's 60° and
/// 90° bits).
const ROOM_ROUNDING_MM: f64 = ROUNDING_MM + Z_ROUNDING_MM;

/// The least turn of a corner [`Difference::EveryCornerReached`] reads,
/// radians: 20°. A drawing's curves, flattened, turn less at each vertex.
const CORNER_TURN_RAD: f64 = 20.0 * std::f64::consts::PI / 180.0;

/// How near a carve must come to a corner to reach it, mm (#456).
const CORNER_REACH_MM: f64 = 0.01;

/// What the comparison needs to know about the case's carve, in the
/// machine frame (Z zero at the stock top, XY as the project's: every
/// case's origin is the stock's bottom-left corner).
struct Carve {
    /// The regions the carve cuts, as `dc_cam` nests its targets.
    regions: Vec<Region>,
    /// Every outline segment of the regions.
    walls: Vec<Segment>,
    /// Z of the start plane.
    start_z: f64,
    /// Z of the flat depth, if there is one.
    flat_z: Option<f64>,
    /// `tan(α/2)` of the V-bit.
    slope: f64,
    /// Whether the carve is in offset rings.
    offset: bool,
    /// The tool's feed and plunge rate, mm/min.
    feed: f64,
    plunge: f64,
    /// The regions' convex corners turning more than [`CORNER_TURN_RAD`].
    corners: Vec<Point>,
}

impl Carve {
    fn of(project: &Project) -> Result<Self, String> {
        let mut carves = project.operations.iter().filter_map(|op| match op {
            Operation::VCarve(c) if c.enabled => Some((op, c)),
            _ => None,
        });
        let (op, carve): (&Operation, &VCarveOp) =
            carves.next().ok_or("the case has no enabled V-Carve")?;
        if carves.next().is_some() {
            return Err("a V-Carve case has one V-Carve".into());
        }
        let tool = tool_of(project, op);
        let ToolKind::VBit { included_angle_deg } = tool.cutter else {
            return Err("the V-Carve's tool is not a V-bit".into());
        };
        let mut contours = Vec::new();
        for id in &carve.targets {
            let instance = project
                .instances
                .iter()
                .find(|i| &i.id == id)
                .ok_or_else(|| format!("unknown target {id}"))?;
            let definition = project
                .definitions
                .get(&instance.definition)
                .ok_or_else(|| format!("unknown definition {}", instance.definition))?;
            for profile in &definition.profiles {
                let placed = instance
                    .transform
                    .apply_polyline(profile)
                    .map_err(|e| format!("{id}: {e}"))?;
                contours.push(dc_geometry::Contour::new(placed).map_err(|e| format!("{id}: {e}"))?);
            }
        }
        let (regions, _) = dc_geometry::nest(contours).map_err(|e| e.to_string())?;
        let walls = regions
            .iter()
            .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
            .flat_map(|c| c.polyline().segments().collect::<Vec<_>>())
            .collect();
        let start_z = -carve.start_depth_mm;
        let corners = regions
            .iter()
            .flat_map(|r| convex_corners(r, CORNER_TURN_RAD, project.flatten_tolerance_mm))
            .map(|c| c.p)
            .collect();
        Ok(Self {
            corners,
            regions,
            walls,
            start_z,
            flat_z: carve.flat_depth_mm.map(|f| start_z - f),
            slope: (included_angle_deg.to_radians() / 2.0).tan(),
            offset: matches!(carve.strategy, VCarveStrategy::Offset { .. }),
            feed: tool.feed_mm_min.ok_or("the V-bit has a feed")?,
            plunge: tool.plunge_mm_min.ok_or("the V-bit has a plunge rate")?,
        })
    }

    /// The flat depth's Z, or below every cut without one.
    fn floor_z(&self) -> f64 {
        self.flat_z.unwrap_or(f64::NEG_INFINITY)
    }

    /// Whether `z` is at the flat depth, within the programs' rounding.
    fn at_floor(&self, z: f64) -> bool {
        z <= self.floor_z() + 2.0 * Z_ROUNDING_MM
    }

    /// The room between the V-bit's cone with its tip at `p` and the
    /// nearest wall, mm: negative where the cone reaches past a wall.
    fn room(&self, p: P3) -> f64 {
        let wall = self
            .walls
            .iter()
            .map(|s| point_segment_distance_mm(*s, p.xy()))
            .fold(f64::INFINITY, f64::min);
        wall - (self.start_z - p.z) * self.slope
    }

    /// The region `p` lies in, or for a point on an outline (a cut at the
    /// surface through a pinch, where a glyph's outline nearly meets
    /// itself) the region whose outline is nearest, within the tolerance.
    fn region_of(&self, p: Point) -> Option<usize> {
        self.regions
            .iter()
            .position(|r| r.contains_point(p))
            .or_else(|| {
                self.regions
                    .iter()
                    .map(|r| {
                        std::iter::once(r.outer())
                            .chain(r.holes())
                            .flat_map(|c| c.polyline().segments().collect::<Vec<_>>())
                            .map(|s| point_segment_distance_mm(s, p))
                            .fold(f64::INFINITY, f64::min)
                    })
                    .enumerate()
                    .filter(|&(_, d)| d <= ALLOWED_MM)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(k, _)| k)
            })
    }
}

/// A straight cut, machine frame, mm.
#[derive(Debug, Clone, Copy)]
struct Cut {
    line: usize,
    a: P3,
    b: P3,
    feed: f64,
}

impl Cut {
    fn xy_length(&self) -> f64 {
        self.a.xy().distance_to(self.b.xy())
    }

    fn moves_xy(&self) -> bool {
        self.xy_length() > SAME_MM
    }

    fn mid(&self) -> Point {
        Point::new((self.a.x + self.b.x) / 2.0, (self.a.y + self.b.y) / 2.0)
    }

    fn at(&self, t: f64) -> P3 {
        P3 {
            x: self.a.x + (self.b.x - self.a.x) * t,
            y: self.a.y + (self.b.y - self.a.y) * t,
            z: self.a.z + (self.b.z - self.a.z) * t,
        }
    }

    /// Points no more than [`SAMPLE_STEP_MM`] apart in XY, ends included.
    fn samples(&self) -> Vec<P3> {
        let ratio = self.xy_length() / SAMPLE_STEP_MM;
        let mut count = 1_u32;
        while f64::from(count) < ratio {
            count += 1;
        }
        (0..=count)
            .map(|k| self.at(f64::from(k) / f64::from(count)))
            .collect()
    }

    /// The fraction of the cut nearest `p`, in three dimensions.
    fn nearest_t3(&self, p: P3) -> f64 {
        let d = (
            self.b.x - self.a.x,
            self.b.y - self.a.y,
            self.b.z - self.a.z,
        );
        let len2 = d.0 * d.0 + d.1 * d.1 + d.2 * d.2;
        if len2 <= 0.0 {
            return 0.0;
        }
        (((p.x - self.a.x) * d.0 + (p.y - self.a.y) * d.1 + (p.z - self.a.z) * d.2) / len2)
            .clamp(0.0, 1.0)
    }

    /// The distance from `p` to the cut in three dimensions.
    fn distance3(&self, p: P3) -> f64 {
        self.at(self.nearest_t3(p)).distance_to(p)
    }

    /// The fraction of the cut nearest `p` in XY.
    fn nearest_t_xy(&self, p: Point) -> f64 {
        let (dx, dy) = (self.b.x - self.a.x, self.b.y - self.a.y);
        let len2 = dx * dx + dy * dy;
        if len2 <= 0.0 {
            return 0.0;
        }
        (((p.x - self.a.x) * dx + (p.y - self.a.y) * dy) / len2).clamp(0.0, 1.0)
    }

    /// The lowest a V-bit cone of `slope` reaches over `q` while its tip
    /// runs along the cut: the least over `t` of `z(t) + |q − p(t)| /
    /// slope`, convex in `t`, so its stationary point clamped to the cut.
    fn cone_low(&self, q: Point, slope: f64) -> f64 {
        let at = |t: f64| {
            let p = self.at(t);
            p.z + q.distance_to(p.xy()) / slope
        };
        let length = self.xy_length();
        if length < 1e-12 {
            return at(0.0).min(at(1.0));
        }
        let (ux, uy) = (
            (self.b.x - self.a.x) / length,
            (self.b.y - self.a.y) / length,
        );
        let (qx, qy) = (q.x - self.a.x, q.y - self.a.y);
        let along = qx * ux + qy * uy;
        let off = (qx * uy - qy * ux).abs();
        let k = -(self.b.z - self.a.z) * slope / length;
        if k.abs() >= 1.0 {
            return at(0.0).min(at(1.0));
        }
        let w = k * off / (1.0 - k * k).sqrt();
        at(((along + w) / length).clamp(0.0, 1.0))
    }

    /// Whether the cut, from `a` to `b`, points the same way as `other`
    /// in XY.
    fn same_way(&self, other: &Self) -> bool {
        let u = (self.b.x - self.a.x, self.b.y - self.a.y);
        let v = (other.b.x - other.a.x, other.b.y - other.a.y);
        u.0 * v.0 + u.1 * v.1 > 0.0
    }
}

/// One program's V-Carve cuts.
struct Program {
    who: &'static str,
    /// Cuts that move in XY, in program order.
    cuts: Vec<Cut>,
    /// The region of each of `cuts`.
    region: Vec<usize>,
    /// Cuts that move only in Z.
    verticals: Vec<Cut>,
    /// Where the tool starts or ends a stroke of cuts that move in XY: it
    /// plunges or lifts there, or rapids.
    stroke_ends: Vec<P3>,
}

impl Program {
    fn of(who: &'static str, moves: &[Motion], carve: &Carve) -> Result<Self, String> {
        let mut cuts = Vec::new();
        let mut verticals = Vec::new();
        let mut stroke_ends = Vec::new();
        let mut in_stroke = false;
        let mut last: Option<P3> = None;
        for mv in moves.iter().filter(|m| !m.machine_coordinates) {
            let pieces = match mv.kind {
                MotionKind::Rapid => vec![],
                MotionKind::Linear => {
                    let (Some(a), Some(b), Some(feed)) = (mv.from, mv.to, mv.feed) else {
                        return Err(format!(
                            "{who} line {}: cut from an unknown position",
                            mv.line
                        ));
                    };
                    vec![Cut {
                        line: mv.line,
                        a,
                        b,
                        feed,
                    }]
                }
                // The emitter fits arcs to a floor ring's round joins; the
                // carve's other cuts are straight (`cone_in_material`
                // refuses arcs). An arc becomes chords no more than
                // [`SAMPLE_STEP_MM`] long, within 0.0003 mm of it over the
                // corpus's radii.
                MotionKind::ArcCw | MotionKind::ArcCcw => {
                    let arc = Piece::new(mv)?;
                    if !(carve.at_floor(arc.z0) && carve.at_floor(arc.z)) {
                        return Err(format!(
                            "{who} line {}: an arc above the flat depth; a V-Carve cuts straight moves",
                            mv.line
                        ));
                    }
                    let ts = arc.sample_ts();
                    ts.windows(2)
                        .map(|w| Cut {
                            line: mv.line,
                            a: arc.at3(w[0]),
                            b: arc.at3(w[1]),
                            feed: arc.feed,
                        })
                        .collect()
                }
            };
            if pieces.is_empty() || !pieces.iter().all(Cut::moves_xy) {
                if in_stroke && let Some(end) = last {
                    stroke_ends.push(end);
                }
                in_stroke = false;
                verticals.extend(pieces);
                continue;
            }
            for c in pieces {
                if !in_stroke {
                    stroke_ends.push(c.a);
                }
                in_stroke = true;
                last = Some(c.b);
                cuts.push(c);
            }
        }
        if in_stroke && let Some(end) = last {
            stroke_ends.push(end);
        }
        let region = cuts
            .iter()
            .map(|c| {
                carve.region_of(c.mid()).ok_or_else(|| {
                    format!(
                        "{who} line {}: the cut lies in no region of the carve",
                        c.line
                    )
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            who,
            cuts,
            region,
            verticals,
            stroke_ends,
        })
    }

    /// The cuts in region `r`.
    fn in_region(&self, r: usize) -> Vec<Cut> {
        self.cuts
            .iter()
            .zip(&self.region)
            .filter(|&(_, &k)| k == r)
            .map(|(c, _)| *c)
            .collect()
    }

    /// The regions in the order the program cuts in them, plunges included
    /// (a carve may be one plunge), each run of cuts in one region once.
    fn region_order(&self, carve: &Carve) -> Vec<usize> {
        let mut visits: Vec<(usize, usize)> = self
            .cuts
            .iter()
            .map(|c| c.line)
            .zip(self.region.iter().copied())
            .chain(
                self.verticals
                    .iter()
                    .filter_map(|v| Some((v.line, carve.region_of(v.mid())?))),
            )
            .collect();
        visits.sort_unstable();
        let mut order: Vec<usize> = Vec::new();
        for (_, r) in visits {
            if order.last() != Some(&r) {
                order.push(r);
            }
        }
        order
    }

    /// The deepest Z the program cuts to, and the deepest Z above the flat
    /// depth that a cut moving in XY reaches (`None` if every such cut is
    /// at the flat depth).
    fn deepest(&self, carve: &Carve) -> (f64, Option<f64>) {
        let deepest = self
            .cuts
            .iter()
            .chain(&self.verticals)
            .flat_map(|c| [c.a.z, c.b.z])
            .fold(f64::INFINITY, f64::min);
        let above = self
            .cuts
            .iter()
            .flat_map(|c| [c.a.z, c.b.z])
            .filter(|z| !carve.at_floor(*z))
            .reduce(f64::min);
        (deepest, above)
    }

    /// How far, mm, the program's cuts miss `q` at the start plane: the
    /// least over its cuts of the cone's reach short of `q` (0 or less
    /// where a cone reaches it).
    fn reach_gap(&self, q: Point, carve: &Carve) -> f64 {
        let low = self
            .cuts
            .iter()
            .chain(&self.verticals)
            .map(|c| c.cone_low(q, carve.slope))
            .fold(f64::INFINITY, f64::min);
        (low - carve.start_z) * carve.slope
    }

    /// Whether `p` is where the program starts or ends a stroke.
    fn ends_stroke_at(&self, p: P3) -> bool {
        self.stroke_ends
            .iter()
            .any(|e| e.distance_to(p) <= ALLOWED_MM)
    }
}

/// Compares the V-Carve programs `golden` and `ours` of `project`, as the
/// module docs say. Returns how many sampled points of the golden's and
/// of DreamCarve's cuts each rule took.
pub fn compare(
    project: &Project,
    golden: &[Motion],
    ours: &[Motion],
    differences: &[Difference],
    order: Order,
) -> Result<(Tally, Tally), String> {
    let carve = Carve::of(project)?;
    let golden = Program::of("golden", golden, &carve)?;
    let ours = Program::of("DreamCarve", ours, &carve)?;
    let has = |d: Difference| differences.contains(&d);

    for program in [&golden, &ours] {
        check_feeds(
            program,
            &carve,
            has(Difference::DescentsHeldToPlunge) && program.who == "DreamCarve",
        )?;
    }
    for program in [&golden, &ours] {
        let (deepest, _) = program.deepest(&carve);
        if deepest < carve.floor_z() - Z_ROUNDING_MM {
            return Err(format!(
                "{} cuts to Z {deepest}, below the flat depth {}",
                program.who,
                carve.floor_z()
            ));
        }
    }
    let ((_, g), (_, o)) = (golden.deepest(&carve), ours.deepest(&carve));
    let same = match (g, o) {
        (Some(g), Some(o)) => approx_eq(g, o, 2.0 * Z_ROUNDING_MM),
        (g, o) => g.is_none() && o.is_none(),
    };
    if !same {
        return Err(format!(
            "the deepest cut above the flat depth is Z {g:?} in the golden, Z {o:?} in DreamCarve's"
        ));
    }
    let (g, o) = (golden.region_order(&carve), ours.region_order(&carve));
    if g.is_empty() || (order == Order::InOrder && g != o) {
        return Err(format!(
            "the regions are cut in another order: golden {g:?}, DreamCarve {o:?}"
        ));
    }
    check_floor(&golden, &ours, &carve)?;
    let (mut gt, mut ot) = (Tally::default(), Tally::default());
    let rules = Rules {
        fitted: has(Difference::ChainDepthFitted),
        corners: if has(Difference::EveryCornerReached) {
            corners_missed(&golden, &ours, &carve)?
        } else {
            Vec::new()
        },
    };
    for r in 0..carve.regions.len() {
        let (g, o) = (golden.in_region(r), ours.in_region(r));
        let region = |e: String| format!("region {r}: {e}");
        if carve.offset {
            compare_levels(&g, &o, &carve, &mut gt, &mut ot).map_err(region)?;
        } else {
            within(&golden, &g, &ours, &o, &carve, &rules, &mut gt).map_err(region)?;
            within(&ours, &o, &golden, &g, &carve, &rules, &mut ot).map_err(region)?;
        }
    }
    Ok((gt, ot))
}

/// Plunges and lifts at the plunge rate; every other cut at the cutting
/// feed, except that with `held`, a cut that descends while it moves in XY
/// runs no faster than the feed and falls no faster than the plunge rate
/// (S10, [`Difference::DescentsHeldToPlunge`]).
fn check_feeds(program: &Program, carve: &Carve, held: bool) -> Result<(), String> {
    let who = program.who;
    for v in &program.verticals {
        if !approx_eq(v.feed, carve.plunge, SAME_MM) {
            return Err(format!(
                "{who} line {}: a plunge or lift at F{}, not the plunge rate {}",
                v.line, v.feed, carve.plunge
            ));
        }
    }
    for c in &program.cuts {
        let drop = c.a.z - c.b.z;
        if held && drop > SAME_MM {
            let length = c.a.distance_to(c.b);
            let rate = c.feed * drop / length;
            if c.feed > carve.feed + SAME_MM || rate > carve.plunge * (1.0 + 1e-9) {
                return Err(format!(
                    "{who} line {}: a descent at F{} falls {rate} mm/min, past the plunge rate {}",
                    c.line, c.feed, carve.plunge
                ));
            }
        } else if !approx_eq(c.feed, carve.feed, SAME_MM) {
            return Err(format!(
                "{who} line {}: a cut at F{}, not the cutting feed {}",
                c.line, c.feed, carve.feed
            ));
        }
    }
    Ok(())
}

/// Every golden cut at the flat depth, and every golden plunge to it, lies
/// within the V-bit's flat-depth radius of a DreamCarve cut at that depth.
fn check_floor(golden: &Program, ours: &Program, carve: &Carve) -> Result<(), String> {
    let Some(flat_z) = carve.flat_z else {
        return Ok(());
    };
    let reach = (carve.start_z - flat_z) * carve.slope + ALLOWED_MM;
    let floor: Vec<&Cut> = ours
        .cuts
        .iter()
        .filter(|c| carve.at_floor(c.a.z) && carve.at_floor(c.b.z))
        .collect();
    let golden_points = golden
        .cuts
        .iter()
        .flat_map(Cut::samples)
        .chain(
            golden
                .verticals
                .iter()
                .map(|v| if v.a.z < v.b.z { v.a } else { v.b }),
        )
        .filter(|p| carve.at_floor(p.z));
    for p in golden_points {
        let near = floor
            .iter()
            .map(|c| c.at(c.nearest_t_xy(p.xy())).xy().distance_to(p.xy()))
            .fold(f64::INFINITY, f64::min);
        if near > reach {
            return Err(format!(
                "the golden cut at ({:.3}, {:.3}, {:.3}) is {near:.3} mm from DreamCarve's floor, past the V-bit's reach {reach:.3}",
                p.x, p.y, p.z
            ));
        }
    }
    Ok(())
}

/// The declared differences [`within`] applies.
struct Rules {
    /// [`Difference::ChainDepthFitted`].
    fitted: bool,
    /// With [`Difference::EveryCornerReached`], the convex corners the
    /// golden program misses; empty otherwise.
    corners: Vec<Point>,
}

/// [`Difference::EveryCornerReached`]: DreamCarve reaches every convex
/// corner of the carve within [`CORNER_REACH_MM`]; the corners the golden
/// program misses by more, which must be some (else the difference is not
/// one).
fn corners_missed(golden: &Program, ours: &Program, carve: &Carve) -> Result<Vec<Point>, String> {
    let mut missed = Vec::new();
    for &c in &carve.corners {
        let gap = ours.reach_gap(c, carve);
        if gap > CORNER_REACH_MM {
            return Err(format!(
                "DreamCarve misses the corner at ({:.3}, {:.3}) by {gap:.3} mm",
                c.x, c.y
            ));
        }
        if golden.reach_gap(c, carve) > CORNER_REACH_MM {
            missed.push(c);
        }
    }
    if missed.is_empty() {
        return Err(
            "the golden program reaches every corner: EveryCornerReached is not a difference here"
                .into(),
        );
    }
    Ok(missed)
}

/// How many sampled points of one program each rule of [`within`] took.
#[derive(Debug, Default)]
pub struct Tally {
    /// Within [`ALLOWED_MM`] of the other program's cuts.
    pub matched: usize,
    /// At the flat depth, so not compared point for point.
    pub floor: usize,
    /// On a link.
    pub link: usize,
    /// At the same XY as the other program's, at another depth
    /// ([`Difference::ChainDepthFitted`]).
    pub fitted: usize,
    /// On DreamCarve's carve into a corner the golden program misses
    /// ([`Difference::EveryCornerReached`]).
    pub corner: usize,
}

/// Every sampled point of `a_cuts` (program `a`) above the flat depth lies
/// within [`ALLOWED_MM`] of `b_cuts` (program `b`, the same region), or is
/// on a link, or (with `fitted`) differs only in depth where the deeper
/// program is checked against the walls.
fn within(
    a: &Program,
    a_cuts: &[Cut],
    b: &Program,
    b_cuts: &[Cut],
    carve: &Carve,
    rules: &Rules,
    tally: &mut Tally,
) -> Result<(), String> {
    let who = a.who;
    let fitted = rules.fitted;
    for cut in a_cuts {
        for p in cut.samples() {
            if carve.at_floor(p.z) {
                tally.floor += 1;
                continue;
            }
            if b_cuts.iter().any(|c| c.distance3(p) <= ALLOWED_MM) {
                tally.matched += 1;
                continue;
            }
            let here = format!(
                "{who} line {} at ({:.3}, {:.3}, {:.3})",
                cut.line, p.x, p.y, p.z
            );
            // The other program at this XY, at the depth nearest p's.
            let other_z = b_cuts
                .iter()
                .filter_map(|c| {
                    let q = c.at(c.nearest_t_xy(p.xy()));
                    (q.xy().distance_to(p.xy()) <= ALLOWED_MM).then_some(q.z)
                })
                .min_by(|x, y| (x - p.z).abs().total_cmp(&(y - p.z).abs()));
            let verdict = match other_z {
                Some(z) if fitted => {
                    // DreamCarve's point is on the V surface, higher or
                    // deeper; a deeper golden point reaches past a wall; a
                    // higher golden point stops short of the surface that
                    // DreamCarve's, checked from its side, reaches.
                    let room = carve.room(p);
                    let right = if a.who == "DreamCarve" {
                        room.abs() <= ALLOWED_MM
                    } else {
                        p.z > z || room <= ROOM_ROUNDING_MM
                    };
                    if right {
                        Ok(&mut tally.fitted)
                    } else {
                        Err(format!(
                            "{here} is {:.3} mm {} than {} with {room:.4} mm between its cone and the walls",
                            (z - p.z).abs(),
                            if p.z < z { "deeper" } else { "higher" },
                            b.who
                        ))
                    }
                }
                Some(z) => Err(format!(
                    "{here} is {:.3} mm from {}'s cut at Z {z:.3} there",
                    (p.z - z).abs(),
                    b.who
                )),
                None => {
                    let link = [cut.a, cut.b].iter().all(|&end| {
                        b.ends_stroke_at(end)
                            && b_cuts.iter().any(|c| c.distance3(end) <= ALLOWED_MM)
                    });
                    if link {
                        Ok(&mut tally.link)
                    } else {
                        Err(format!("{here} is on no cut of {}", b.who))
                    }
                }
            };
            match verdict {
                Ok(count) => *count += 1,
                Err(e) => {
                    if a.who == "DreamCarve"
                        && !rules.corners.is_empty()
                        && into_corner(p, cut, carve, b)
                    {
                        tally.corner += 1;
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }
    Ok(())
}

/// [`Difference::EveryCornerReached`]: DreamCarve's point `p` of `cut`
/// lies on the V surface, its cone touching a wall within the tolerance,
/// and `cut` reaches a wall point the golden program misses by more than
/// [`CORNER_REACH_MM`]: the point nearest some sample of `cut`, which its
/// cone touches.
fn into_corner(p: P3, cut: &Cut, carve: &Carve, golden: &Program) -> bool {
    let touched = |q: P3| {
        carve
            .regions
            .iter()
            .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
            .filter_map(|c| nearest_point(c.polyline(), q.xy()))
            .min_by(|a, b| a.gap_mm.total_cmp(&b.gap_mm))
            .map(|n| n.point)
    };
    let gap = cut
        .samples()
        .into_iter()
        .filter_map(touched)
        .map(|w| golden.reach_gap(w, carve))
        .fold(f64::NEG_INFINITY, f64::max);
    carve.room(p).abs() <= ALLOWED_MM && gap > CORNER_REACH_MM
}

/// Offset rings: the levels above the flat depth are the same in both
/// programs, and their level cuts are the same set, travelling the same
/// way. Cuts that change Z are links between levels and not compared.
fn compare_levels(
    golden: &[Cut],
    ours: &[Cut],
    carve: &Carve,
    golden_tally: &mut Tally,
    ours_tally: &mut Tally,
) -> Result<(), String> {
    let level = |c: &Cut| (c.a.z - c.b.z).abs() <= SAME_MM;
    let levels = |cuts: &[Cut], tally: &mut Tally| -> Vec<Cut> {
        for c in cuts {
            let n = c.samples().len();
            if !level(c) {
                tally.link += n;
            } else if carve.at_floor(c.a.z) {
                tally.floor += n;
            } else {
                tally.matched += n;
            }
        }
        cuts.iter()
            .filter(|c| level(c) && !carve.at_floor(c.a.z))
            .copied()
            .collect()
    };
    let (g, o) = (levels(golden, golden_tally), levels(ours, ours_tally));
    let zs = |cuts: &[Cut]| -> Vec<f64> {
        let mut zs: Vec<f64> = cuts.iter().map(|c| c.a.z).collect();
        zs.sort_by(f64::total_cmp);
        zs.dedup_by(|x, y| approx_eq(*x, *y, SAME_MM));
        zs
    };
    let (gz, oz) = (zs(&g), zs(&o));
    if gz.len() != oz.len()
        || gz
            .iter()
            .zip(&oz)
            .any(|(x, y)| !approx_eq(*x, *y, 2.0 * Z_ROUNDING_MM))
    {
        return Err(format!(
            "the levels differ: golden {gz:?}, DreamCarve {oz:?}"
        ));
    }
    for (who, from, to) in [("golden", &g, &o), ("DreamCarve", &o, &g)] {
        for cut in from {
            for p in cut.samples() {
                let matched = to
                    .iter()
                    .any(|c| c.distance3(p) <= ALLOWED_MM && c.same_way(cut));
                if !matched {
                    return Err(format!(
                        "{who} line {} at ({:.3}, {:.3}, {:.3}) is on no level cut of the other program travelling its way",
                        cut.line, p.x, p.y, p.z
                    ));
                }
            }
        }
    }
    Ok(())
}
