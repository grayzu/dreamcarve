// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S9 on the parsed program: every operation's cuts stay in its own
//! material (`docs/specs/phase-2/functional.md`, S9;
//! `docs/specs/phase-2/design.md`, "Data model → dc-gcode",
//! [`ExportError::OffMaterial`]).
//!
//! The material comes from the project, found the way the generators find
//! their targets, and never from the generator: dc-gcode uses only
//! dc-cam's toolpath types and trusts none of its callers
//! (`docs/rust-guidelines.md`, "Export preconditions"). Per operation:
//!
//! - **Pocket:** the tool, swept along every cutting move, stays inside
//!   the Pocket's regions (its targets' closed profiles, nested) shrunk by
//!   its allowance, and outside their islands. For a round tool of radius
//!   `r` that is exactly: the move's tool-centre path lies inside a region
//!   and at least `r + allowance` from its outline and from every island's.
//!   The check measures that distance segment against segment, arcs as
//!   arcs ([`dc_geometry::segment_distance_mm`]), against only the outline
//!   segments whose bounds come near the move. A helix of a thousand chords
//!   costs a thousand such tests, not the union of a thousand swept
//!   capsules that `dc_cam::footprint` builds (#158). Rest machining is
//!   held to its own region the same way (S14).
//! - **Corner relief, the notch rule** (slice 12; Profile since #177): a
//!   Pocket or an inside or outside Profile with `corner_relief` cuts
//!   dogbone or T-bone notches past its walls on purpose, so a straight cut
//!   that fails the clearance above is still in the material when it lies
//!   in a corner's relief zone. A corner is a vertex of a target outline
//!   where the outline turns toward the waste by more than 15° between two
//!   edges at least the tool's diameter long (`dc_cam`'s corner test,
//!   re-derived here from the project). The zone is the corner's wedge,
//!   between its two edge lines, at least `r` from the corner (the cutter
//!   at most touches it) and at most `(r + allowance) / sin(θ/2)` from it
//!   (the tool-centre path's miter point, where a notch starts); the cut
//!   must also keep its clearance from every other edge. So a notch can
//!   bite at most `r` past one of its corner's two walls, and nowhere else.
//! - **Profile** (#177): the waste the Profile removes, from its targets'
//!   closed profiles nested as `dc_cam::generate_profile` nests them, and
//!   every cutting move is held to it: laps, tab rises, ramp and helix
//!   descents, lead arcs and overcuts, and notches.
//!   - **Inside:** as a Pocket with no allowance: the tool centre stays in
//!     a target region (the hole being cut, outside its islands), at least
//!     `r` from its walls, so no cut enters the part around the hole or an
//!     island left in it.
//!   - **Outside:** the reverse: the tool centre stays outside every
//!     target region (the parts; a part's own holes are waste), at least
//!     `r` from every outline, so a ramp, helix or lead arc enters neither
//!     its own part nor a neighbour. The stock edge is S7's.
//!   - **On line:** as an Engrave, the tool centre stays on a target's
//!     line, and also at least the tool's diameter from every other line
//!     of that line's region (the targets nested as for Inside): the bit
//!     cuts `r` of the part beside each line, so two lines of one region
//!     closer than the diameter would cut through the ring or web between
//!     them, which dc-cam refuses (`CamError::BandNarrowerThanTool`,
//!     #199). Lines of different regions are not looked at: a gouge there
//!     is the overlap check's warning, as for a lap. The On line ramp runs
//!     along the line; there is no lead or notch. A cut on one of the
//!     targets' open paths (#175) stays on that path, as an Engrave's.
//!     When a region's material holds the path (every vertex and segment
//!     middle, lines included), the cut also keeps the diameter from
//!     every line of that region and from every other open path it holds,
//!     the stretches of both within `√2·d` of a point where the two meet
//!     (touch, cross, or come within the flatten tolerance) set aside: so
//!     a slot drawn from the edge passes and one beside a wall fails, as
//!     dc-cam refuses it (`BandNarrowerThanTool`, #210).
//!     A cut also keeps the diameter from the stretches of its own line
//!     that lie more than `√2·d` along it from where it runs (#221,
//!     [`dc_geometry::SelfBand`]): a loop of a region, or an open path a
//!     region holds. The stretches within `√2·d` of a point where the line
//!     meets itself (a corner sharper than 90°, or where it touches or
//!     crosses itself) are set aside, and a loop counts only the pairs with
//!     the region's material between them, so a cut along a hairpin, a
//!     tight spiral or an outline's narrow neck fails and one along a
//!     narrow notch passes, as dc-cam decides. No cut may run on a loop
//!     of a region the cut removes whole (#223): its outline shrunk by the
//!     radius, less the slack, is empty, as for a small circle or a strip
//!     narrower than the bit (`CamError::PartNarrowerThanTool`).
//!   - **Left and Right** (#207): the tool centre stays on one of the
//!     targets' open paths offset by the tool radius to that side
//!     ([`dc_geometry::offset_open`], looking from each path's first vertex
//!     to its last), its ends and middle within the slack of it, and the
//!     whole cut keeps the radius from every open path of the targets and
//!     from every path's kept edge (the path offset by the radius to its
//!     other side): the cutter never crosses a line nor reaches the strip
//!     one radius wide that a line keeps, as `dc_cam` refuses
//!     (`CamError::CutsIntoKeptSide`). Closed targets are not cut. The
//!     ramp and the tabs run along the offset path; there is no lead or
//!     notch. With a last pass (#311) the path offset by the radius plus
//!     the allowance is a tool-centre path too, for the passes before the
//!     last; the kept edges stay one radius off.
//!   - **A lap** of one outline, a cut that runs at the tool radius from
//!     it on its waste side, passes even where an outline of another
//!     region comes nearer than the radius. That cut is the outline's
//!     offset, which S6 holds the program to, and the gouge into the other
//!     shape is the one `dc_cam`'s overlap check warns about
//!     (`toolpaths_overlap`): the project asked for two shapes closer than
//!     the bit, and phase 1 decided to warn there, not to refuse. The lap
//!     must still keep its clearance from every outline of its own region
//!     (#192): within one region the overlap check never warns, and a lap
//!     that comes nearer another line of it, an island's lap in a band
//!     narrower than the bit, is a gouge dc-cam refuses
//!     (`CamError::BandNarrowerThanTool`). Entries, leads and notches never
//!     run along an outline at the radius, so the lap rule never lets one
//!     through. dc-cam's waste domain also keeps entries out of other
//!     shapes' kerfs; that is waste too, so S9 does not hold the program
//!     to it.
//! - **Tabs** (#337): a Profile leaves its tabs standing. A straight
//!   plunge (Z falls, XY stays) that ends below a tab's top is refused
//!   when it lies on the bit's path round the tab's outline (the outline
//!   offset by the tool radius, or the radius plus a last pass's
//!   allowance, either way, within the slack) less than half the tab's
//!   width plus the radius from the tab's centre along that path, the
//!   stretch a lap rises over (ADR-0005), less a tenth of the radius for
//!   a rect tab's step at its end. The tabs are the project's placed tabs
//!   on the Profile's targets and those its tab request places
//!   (`dc_cam::placed_tabs::place`, the documented layout rule, not the
//!   toolpath). So a lap that starts on a tab and plunges there, below
//!   its top, is refused, wherever on the tab it starts.
//! - **Drill:** every cut stays inside one of the Drill's holes, the
//!   closed targets that are circles ([`dc_geometry::as_circle`], as
//!   `dc_cam::generate_drill` finds them): its ends and middle lie within
//!   the hole's radius of its centre. A Drill only plunges, so that is the
//!   hole it drills.
//! - **Engrave:** the tool centre stays on the line: every cut's ends and
//!   middle lie on one of the Engrave's target paths.
//!
//! A move is allowed to depart from where the toolpath put it by what the
//! program may already differ by: one unit in the last written place (the
//! numbers' rounding), the flatten tolerance (arcs written as chords, and
//! the tool-centre outlines and raster ends the generator flattens), and
//! the arc-fit tolerance when arc fitting is on (a run of chords written
//! as an arc, S12).
//!
//! **Cost.** Every move is tested only against the outline segments whose
//! bounds come within the clearance of its own, and for an outside
//! Profile only against the parts whose bounds do; the lap rule runs only
//! for a move the plain rule rejects. A 20-part outside Profile with
//! ramps, leads and tabs checks in a few milliseconds (#177).

use std::ops::Range;

use dc_geometry::{
    Arc, Contour, EPS_MM, Point, Point3, Polyline, Rect, Region, Segment, SelfBand, Vec2,
    as_circle, closest_points, inset_is_empty, meeting_points, nearest_point, nest, offset,
    offset_open, point_segment_distance_mm, segment_distance_mm,
};
use dc_project::{CornerRelief, InstanceId, Operation, PostProcessor, Project, Side, TabShape};

use crate::ExportError;
use crate::parse::{Block, MachineMove, MotionKind};
use crate::safety::{OpSpan, Tolerances, quantum_mm, r_centre};
use crate::transform::MachineTransform;

/// The material one operation may cut, worked out from the project.
#[derive(Debug, Clone)]
pub(crate) enum Material {
    /// A Pocket's regions, or an inside Profile's: the tool centre stays
    /// inside one, clear of its walls.
    Inside {
        /// The regions, outer and islands, from the targets' closed
        /// profiles.
        areas: Vec<Area>,
        /// The allowance left on every wall and island, mm; 0 for a
        /// Profile.
        allowance_mm: f64,
        /// Corner relief is on, so the notch rule applies.
        relief: bool,
        /// A Profile: the lap rule applies.
        laps: bool,
    },
    /// An outside Profile's parts: the tool centre stays outside every
    /// one, clear of its outlines.
    Outside {
        /// The parts, outer and holes, from the targets' closed profiles.
        areas: Vec<Area>,
        /// Corner relief is on, so the notch rule applies.
        relief: bool,
    },
    /// A Drill's holes: centre and radius, mm.
    Drill {
        /// Each hole.
        holes: Vec<(Point, f64)>,
    },
    /// An On line Profile's regions: the tool centre stays on one of
    /// their lines, at least the tool's diameter from the region's other
    /// lines (#199), or on one of its targets' open paths (#175), as far
    /// from the lines beside that path away from where they meet (#210),
    /// and as far from its own line's stretches far along it (#221).
    OnLineProfile {
        /// The regions, outer and holes, from the targets' closed
        /// profiles.
        areas: Vec<Area>,
        /// The targets' open paths.
        open: Vec<OpenPath>,
    },
    /// A Left or Right Profile's open paths (#207): the tool centre stays
    /// on one path's offset, clear of every path and every kept edge.
    SideProfile {
        /// Every segment of every offset path, as cut.
        centres: Vec<Edge>,
        /// Every segment of every open path, and of every path's kept
        /// edge: what the cut keeps the tool radius from.
        keep_clear: Vec<Edge>,
    },
    /// An Engrave's target paths, as segments.
    OnLine {
        /// Every segment of every path.
        edges: Vec<Edge>,
    },
    /// The targets do not resolve to anything an operation of this kind
    /// cuts (an unknown instance, crossing outlines, an invalid
    /// allowance): no cut is in the material, fail-closed.
    Unresolved,
    /// Tests of the other rules on hand-written programs: S9 is not
    /// checked.
    #[cfg(test)]
    Unchecked,
}

/// One region, its outline segments, the corners a relief notch may cut
/// into, and its loops.
#[derive(Debug, Clone)]
pub(crate) struct Area {
    region: Region,
    /// Every loop's segments, each loop wound with the waste on its left.
    edges: Vec<Edge>,
    corners: Vec<Corner>,
    loops: Vec<Loop>,
    /// The bounds of every edge.
    bounds: Rect,
}

/// One outline of an [`Area`], for the lap rule.
#[derive(Debug, Clone)]
struct Loop {
    /// Its segments in [`Area::edges`].
    edges: Range<usize>,
    /// The loop alone, to tell its sides apart.
    alone: Option<Region>,
    /// The loop as wound, for measuring it against itself (#221).
    line: Polyline,
    /// The waste is inside the loop.
    waste_inside: bool,
}

/// The least turn toward the waste at a vertex that makes it a corner
/// for the notch rule: 15°, as `dc_cam`'s relief.
const CORNER_MIN_TURN_RAD: f64 = std::f64::consts::PI / 12.0;

/// A corner of an area, for the notch rule.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Corner {
    /// The corner, mm.
    at: Point,
    /// Unit direction from the corner along its incoming edge.
    edge_a: Vec2,
    /// Unit direction from the corner along its outgoing edge.
    edge_b: Vec2,
    /// The shorter edge's length, mm.
    shorter_mm: f64,
    /// The sine of half the interior angle on the waste side.
    sin_half: f64,
    /// The indices in [`Area::edges`] of the two edges.
    edges: [usize; 2],
}

impl Corner {
    /// Whether `p` is in the corner's wedge, `slack_mm` included.
    fn in_wedge(&self, p: Point, slack_mm: f64) -> bool {
        let d = p - self.at;
        let side_a = self.edge_a.cross(self.edge_b).signum();
        let side_b = self.edge_b.cross(self.edge_a).signum();
        self.edge_a.cross(d) * side_a >= -slack_mm && self.edge_b.cross(d) * side_b >= -slack_mm
    }
}

/// The corners of `polyline`, a closed loop with the waste on its left,
/// whose segments are `edges[base..]` in order.
fn corners_of(polyline: &Polyline, base: usize) -> Vec<Corner> {
    let segments: Vec<Segment> = polyline.segments().collect();
    let n = segments.len();
    let mut out = Vec::new();
    if n < 2 {
        return out;
    }
    for (k, next) in segments.iter().enumerate() {
        let before = (k + n - 1) % n;
        let Some(prev) = segments.get(before) else {
            continue;
        };
        let (Some((_, incoming)), Some((_, outgoing))) =
            (prev.point_at(prev.length_mm()), next.point_at(0.0))
        else {
            continue;
        };
        let (edge_a, edge_b) = (-incoming, outgoing);
        let turn = (-edge_a.cross(edge_b)).atan2(-edge_a.dot(edge_b));
        if turn.is_nan() || turn <= CORNER_MIN_TURN_RAD {
            continue;
        }
        out.push(Corner {
            at: next.start,
            edge_a,
            edge_b,
            shorter_mm: prev.length_mm().min(next.length_mm()),
            sin_half: ((std::f64::consts::PI - turn) / 2.0).sin(),
            edges: [base + before, base + k],
        });
    }
    out
}

/// S9's notch rule for one operation: its tool's radius, its allowance,
/// and the slack a written move may depart by.
#[derive(Debug, Clone, Copy)]
struct Notch {
    radius_mm: f64,
    allowance_mm: f64,
    slack_mm: f64,
}

impl Notch {
    /// Whether straight tool-centre path `path`, which comes nearer a wall
    /// of `area` than `clearance_mm`, is a relief notch: it lies in the
    /// relief zone of one of the area's corners on the edges `scope`
    /// (module docs), clear of the scope's other edges.
    fn allows(self, area: &Area, scope: &Range<usize>, path: Segment, clearance_mm: f64) -> bool {
        if !path.is_line() {
            return false;
        }
        let (r, slack) = (self.radius_mm, self.slack_mm);
        let bounds = bounds_of(path);
        area.corners
            .iter()
            .filter(|c| c.edges.iter().all(|e| scope.contains(e)))
            .any(|c| {
                let reach_mm = (r + self.allowance_mm) / c.sin_half + slack;
                c.shorter_mm >= 2.0 * r - slack
                    && c.in_wedge(path.start, slack)
                    && c.in_wedge(path.end, slack)
                    && path.start.distance_to(c.at) <= reach_mm
                    && path.end.distance_to(c.at) <= reach_mm
                    && point_segment_distance_mm(path, c.at) >= r - slack
                    && area
                        .edges
                        .iter()
                        .enumerate()
                        .filter(|(i, e)| {
                            scope.contains(i)
                                && !c.edges.contains(i)
                                && near(bounds, e.bounds, clearance_mm)
                        })
                        .all(|(_, e)| segment_distance_mm(path, e.segment) >= clearance_mm)
            })
    }
}

/// One outline segment and its bounds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Edge {
    segment: Segment,
    bounds: Rect,
}

impl Edge {
    fn new(segment: Segment) -> Self {
        Self {
            segment,
            bounds: bounds_of(segment),
        }
    }
}

/// One open path an On line Profile cuts, and the lines it must keep the
/// tool's diameter from (#210).
#[derive(Debug, Clone)]
pub(crate) struct OpenPath {
    /// The path's segments.
    edges: Vec<Edge>,
    /// The path, when a region's material holds it: then it is measured
    /// against itself too (#221).
    held: Option<Polyline>,
    /// When a region's material holds the path: each line of that region
    /// and each other open path it holds.
    beside: Vec<Beside>,
}

/// A line beside an [`OpenPath`] and the points where the two meet.
#[derive(Debug, Clone)]
struct Beside {
    edges: Vec<Edge>,
    meetings: Vec<Point>,
}

/// How far from a point where two lines of one region meet their On line
/// cuts may come closer than the tool's diameter, as a multiple of the
/// diameter: √2, so straight lines meeting at 45° or more pass. The same
/// as `dc_cam::generate_profile`'s rule (#210), re-stated here because S9
/// trusts no generator.
const MEETING_REACH_DIAMETERS: f64 = std::f64::consts::SQRT_2;

/// The open paths of `paths` with length, each with the lines beside it:
/// the lines of the first of `regions` whose material holds all of it
/// (every vertex and segment middle, lines included) and the other open
/// paths that region holds, with the points where each meets it within
/// `tolerance_mm`. A path no region holds has nothing beside it.
fn open_paths_of(paths: &[Polyline], regions: &[Region], tolerance_mm: f64) -> Vec<OpenPath> {
    // A path drawn twice, either way round, is cut once (#221).
    let mut open: Vec<&Polyline> = Vec::new();
    for path in paths
        .iter()
        .filter(|p| !p.is_closed() && p.length_mm() > EPS_MM)
    {
        let reversed = path.reversed();
        if !open.iter().any(|&kept| *kept == *path || *kept == reversed) {
            open.push(path);
        }
    }
    let holds = |region: &Region, path: &Polyline| {
        let middles = path
            .segments()
            .filter_map(|s| s.point_at(s.length_mm() / 2.0).map(|(p, _)| p));
        path.vertices()
            .iter()
            .map(|v| v.point())
            .chain(middles)
            .all(|p| region.contains_point(p))
    };
    let held: Vec<Option<usize>> = open
        .iter()
        .map(|path| regions.iter().position(|r| holds(r, path)))
        .collect();
    let tolerance_mm = tolerance_mm.max(EPS_MM);
    open.iter()
        .zip(&held)
        .enumerate()
        .map(|(index, (path, region_index))| {
            let lines: Vec<&Polyline> = match region_index.and_then(|i| regions.get(i)) {
                Some(region) => std::iter::once(region.outer())
                    .chain(region.holes())
                    .map(Contour::polyline)
                    .chain(
                        open.iter()
                            .zip(&held)
                            .enumerate()
                            .filter(|(j, (_, h))| *j != index && *h == region_index)
                            .map(|(_, (p, _))| *p),
                    )
                    .collect(),
                None => Vec::new(),
            };
            OpenPath {
                edges: edges_of(path).collect(),
                held: region_index.is_some().then(|| (*path).clone()),
                beside: lines
                    .into_iter()
                    .map(|line| Beside {
                        edges: edges_of(line).collect(),
                        meetings: meeting_points(path, line, tolerance_mm),
                    })
                    .collect(),
            }
        })
        .collect()
}

/// The segments of `polyline`, with bounds.
fn edges_of(polyline: &Polyline) -> impl Iterator<Item = Edge> + '_ {
    polyline.segments().map(Edge::new)
}

/// The bounds of `segment`, arc bulge included.
fn bounds_of(segment: Segment) -> Rect {
    let mut rect = Rect::from_point(segment.start).including(segment.end);
    if let Some(arc) = segment.arc() {
        for extreme in arc.axis_extremes() {
            rect = rect.including(extreme);
        }
    }
    rect
}

/// Whether `a` grown by `margin_mm` on every side meets `b`.
fn near(a: Rect, b: Rect, margin_mm: f64) -> bool {
    a.min.x - margin_mm <= b.max.x
        && b.min.x <= a.max.x + margin_mm
        && a.min.y - margin_mm <= b.max.y
        && b.min.y <= a.max.y + margin_mm
}

/// The closed paths of `targets`, nested into regions as the generators
/// nest them; `None` when a target does not resolve or outlines cross.
fn regions_of(project: &Project, targets: &[InstanceId]) -> Option<Vec<Region>> {
    let contours: Option<Vec<Contour>> = placed(project, targets)?
        .into_iter()
        .filter(Polyline::is_closed)
        .map(|p| Contour::new(p).ok())
        .collect();
    let (regions, _) = nest(contours?).ok()?;
    Some(regions)
}

/// `regions` as areas, each loop wound with the waste on its left: the
/// outline counter-clockwise and the islands clockwise when the waste is
/// inside the regions (`waste_inside`: a Pocket, an inside Profile), the
/// reverse when it is outside them (an outside Profile).
fn areas_of(regions: Vec<Region>, waste_inside: bool) -> Vec<Area> {
    regions
        .into_iter()
        .map(|region| {
            let mut edges = Vec::new();
            let mut corners = Vec::new();
            let mut loops = Vec::new();
            let contours = std::iter::once((region.outer(), waste_inside))
                .chain(region.holes().iter().map(|h| (h, !waste_inside)));
            for (contour, inside) in contours {
                let polyline = if contour.is_ccw() == inside {
                    contour.polyline().clone()
                } else {
                    contour.polyline().reversed()
                };
                let start = edges.len();
                corners.extend(corners_of(&polyline, start));
                edges.extend(edges_of(&polyline));
                loops.push(Loop {
                    edges: start..edges.len(),
                    alone: nest(vec![contour.clone()])
                        .ok()
                        .and_then(|(r, _)| r.into_iter().next()),
                    line: polyline,
                    waste_inside: inside,
                });
            }
            let bounds = edges
                .iter()
                .map(|e| e.bounds)
                .reduce(|a, b| a.including(b.min).including(b.max))
                .unwrap_or_else(|| Rect::from_point(Point::new(0.0, 0.0)));
            Area {
                region,
                edges,
                corners,
                loops,
                bounds,
            }
        })
        .collect()
}

impl Material {
    /// The material `op` may cut in `project`.
    pub(crate) fn of(project: &Project, op: &Operation) -> Self {
        match op {
            Operation::Profile(profile) => {
                let relief = profile.corner_relief != CornerRelief::None;
                match profile.side {
                    Side::OnLine => match (
                        regions_of(project, &profile.targets),
                        placed(project, &profile.targets),
                    ) {
                        (Some(regions), Some(paths)) => Self::OnLineProfile {
                            open: open_paths_of(&paths, &regions, project.flatten_tolerance_mm),
                            areas: areas_of(regions, true),
                        },
                        _ => Self::Unresolved,
                    },
                    Side::Left | Side::Right => match (
                        placed(project, &profile.targets),
                        project
                            .tools
                            .iter()
                            .find(|t| t.id == profile.tool)
                            .map(|t| t.diameter_mm / 2.0),
                    ) {
                        (Some(paths), Some(radius_mm)) => {
                            let left_mm = if profile.side == Side::Left {
                                radius_mm
                            } else {
                                -radius_mm
                            };
                            let allowance_mm = profile.last_pass.as_ref().map(|l| l.allowance_mm);
                            side_profile(&paths, left_mm, allowance_mm)
                        }
                        _ => Self::Unresolved,
                    },
                    Side::Inside => match regions_of(project, &profile.targets) {
                        Some(regions) => Self::Inside {
                            areas: areas_of(regions, true),
                            allowance_mm: 0.0,
                            relief,
                            laps: true,
                        },
                        None => Self::Unresolved,
                    },
                    Side::Outside => match regions_of(project, &profile.targets) {
                        Some(regions) => Self::Outside {
                            areas: areas_of(regions, false),
                            relief,
                        },
                        None => Self::Unresolved,
                    },
                }
            }
            Operation::Pocket(pocket) => {
                let allowance_mm = pocket.allowance_mm;
                if !(allowance_mm.is_finite() && allowance_mm >= 0.0) {
                    return Self::Unresolved;
                }
                match regions_of(project, &pocket.targets) {
                    Some(regions) => Self::Inside {
                        areas: areas_of(regions, true),
                        allowance_mm,
                        relief: pocket.corner_relief != CornerRelief::None,
                        laps: false,
                    },
                    None => Self::Unresolved,
                }
            }
            Operation::Drill(drill) => match placed(project, &drill.targets) {
                Some(paths) => Self::Drill {
                    holes: paths
                        .iter()
                        .filter(|p| p.is_closed())
                        .filter_map(|p| as_circle(p, project.flatten_tolerance_mm))
                        .collect(),
                },
                None => Self::Unresolved,
            },
            Operation::Engrave(engrave) => match placed(project, &engrave.targets) {
                Some(paths) => Self::OnLine {
                    edges: paths.iter().flat_map(edges_of).collect(),
                },
                None => Self::Unresolved,
            },
        }
    }
}

/// The material of a Left or Right Profile of `paths` (#207): the open
/// ones with length, offset by `left_mm` (positive to the left) to give
/// the tool-centre paths, and offset the other way to give their kept
/// edges. With a last pass `allowance_mm` (#311) each is offset by
/// `left_mm` plus the allowance, to the same side, too: the path every
/// pass but the last cuts. The kept edges stay one radius off.
fn side_profile(paths: &[Polyline], left_mm: f64, allowance_mm: Option<f64>) -> Material {
    let open: Vec<&Polyline> = paths
        .iter()
        .filter(|p| !p.is_closed() && p.length_mm() > EPS_MM)
        .collect();
    let allowance_left_mm = allowance_mm
        .filter(|a| a.is_finite() && *a > 0.0)
        .map(|a| (left_mm.abs() + a).copysign(left_mm));
    let centres = open
        .iter()
        .flat_map(|p| {
            let mut paths = offset_open(p, left_mm);
            if let Some(far_mm) = allowance_left_mm {
                paths.extend(offset_open(p, far_mm));
            }
            paths
        })
        .flat_map(|c| edges_of(&c).collect::<Vec<_>>())
        .collect();
    let kept = open.iter().flat_map(|p| offset_open(p, -left_mm));
    let keep_clear = open
        .iter()
        .flat_map(|p| edges_of(p))
        .chain(kept.flat_map(|k| edges_of(&k).collect::<Vec<_>>()))
        .collect();
    Material::SideProfile {
        centres,
        keep_clear,
    }
}

/// Every profile of every instance in `targets`, placed in project space;
/// `None` when an instance or its definition is missing or a transform
/// fails.
fn placed(project: &Project, targets: &[InstanceId]) -> Option<Vec<Polyline>> {
    let mut paths = Vec::new();
    for id in targets {
        let instance = project.instances.iter().find(|i| &i.id == id)?;
        let definition = project.definitions.get(&instance.definition)?;
        for polyline in &definition.profiles {
            paths.push(instance.transform.apply_polyline(polyline).ok()?);
        }
    }
    Some(paths)
}

/// One tab a Profile must leave standing (S9, #337), measured as the
/// generator's span rule measures it (ADR-0005): along the bit's path
/// round its outline, from the path's point nearest the tab.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TabSpine {
    /// The bit's paths round the tab's outline: the outline offset by the
    /// tool radius both ways (and by the radius plus a last pass's
    /// allowance, #284), project space.
    paths: Vec<Polyline>,
    /// The tab's point, project space.
    point: Point,
    /// How far along a path either side of the tab's centre a lap holds
    /// the tab top, mm: the tool radius for a smooth tab, which then
    /// slopes down; for a rect tab, which does not, half its width plus
    /// the radius, its half span.
    hold_mm: f64,
    /// How far a smooth tab's slope runs past the hold, mm: half its
    /// width; 0 for a rect tab.
    slope_mm: f64,
    /// How much nearer than the span a cut may come down, mm:
    /// [`TAB_SPAN_MARGIN`] of the radius.
    margin_mm: f64,
    /// The tab's cross-section.
    shape: TabShape,
    /// The cut's floor, above the stock bottom, mm: where a smooth tab's
    /// slope ends on the last pass.
    floor_z_mm: f64,
    /// How far from a path the tab's point may be and still be crossed by
    /// it, mm: the tool radius plus half the tab's width, as the
    /// generator crosses it.
    reach_mm: f64,
    /// The tab's top above the stock bottom, mm.
    top_z_mm: f64,
}

/// Fraction of the tool radius a plunge may come inside a tab's span (the
/// tab's width plus the tool diameter, along the bit's path) without being
/// refused (#337): a rect tab steps down at the span's end, and the
/// offsets S9 builds round the outline may differ that much from the
/// generator's along the path.
const TAB_SPAN_MARGIN: f64 = 0.1;

/// The tabs Profile `op` must leave standing in `project` (S9, #337): its
/// placed tabs on closed outlines of its targets, and those its tab
/// request places (`dc_cam::placed_tabs::place`, the documented layout
/// rule, not the toolpath). Empty for another kind of operation, or when
/// the tool, an instance, a definition or a profile is missing.
pub(crate) fn tab_spines(project: &Project, op: &Operation) -> Vec<TabSpine> {
    let Operation::Profile(profile) = op else {
        return Vec::new();
    };
    let Some(radius_mm) = project
        .tools
        .iter()
        .find(|t| t.id == profile.tool)
        .map(|t| t.diameter_mm / 2.0)
    else {
        return Vec::new();
    };
    let allowance_mm = profile.last_pass.as_ref().map(|l| l.allowance_mm);
    let Some(floor_z_mm) = project
        .stock
        .thickness_mm
        .zip(profile.depth_mm)
        .map(|(thickness_mm, depth_mm)| thickness_mm - depth_mm)
    else {
        return Vec::new();
    };
    let requested = profile
        .tabs
        .as_ref()
        .and_then(|spec| dc_cam::placed_tabs::place(project, &profile.id, spec).ok())
        .map(|placement| placement.tabs)
        .unwrap_or_default();
    project
        .tabs
        .iter()
        .chain(&requested)
        .filter(|t| profile.targets.contains(&t.instance))
        .filter_map(|tab| {
            let instance = project.instances.iter().find(|i| i.id == tab.instance)?;
            let outline = project
                .definitions
                .get(&instance.definition)?
                .profiles
                .get(tab.profile)?;
            let outline = instance.transform.apply_polyline(outline).ok()?;
            let contour = Contour::new(outline.clone()).ok()?;
            let mut paths = vec![outline];
            for delta_mm in std::iter::once(radius_mm).chain(allowance_mm.map(|a| radius_mm + a)) {
                for sign in [1.0, -1.0] {
                    paths.extend(
                        offset(&contour, sign * delta_mm)
                            .into_iter()
                            .map(Contour::into_polyline),
                    );
                }
            }
            let (hold_mm, slope_mm) = match tab.shape {
                TabShape::Rect => (radius_mm + tab.width_mm / 2.0, 0.0),
                TabShape::Smooth => (radius_mm, tab.width_mm / 2.0),
            };
            Some(TabSpine {
                paths,
                point: instance.transform.apply(Point::new(tab.x_mm, tab.y_mm)),
                hold_mm,
                slope_mm,
                margin_mm: radius_mm * TAB_SPAN_MARGIN,
                shape: tab.shape,
                floor_z_mm,
                reach_mm: radius_mm + tab.width_mm / 2.0,
                top_z_mm: tab.height_mm,
            })
        })
        .collect()
}

impl TabSpine {
    /// Whether a cut down to `z_mm` at `p`, on one of the bit's paths
    /// round the tab's outline within `slack_mm`, one that crosses the tab,
    /// goes below the tab there ([`TabSpine::lowest_z_mm`]).
    fn cuts_into(&self, p: Point, z_mm: f64, slack_mm: f64) -> bool {
        self.paths.iter().any(|path| {
            let (Some(at), Some(centre)) =
                (nearest_point(path, p), nearest_point(path, self.point))
            else {
                return false;
            };
            if at.gap_mm > slack_mm || centre.gap_mm > self.reach_mm + slack_mm {
                return false;
            }
            let raw = (at.along_mm - centre.along_mm).abs();
            let along = raw.min(path.length_mm() - raw);
            z_mm < self.lowest_z_mm(along) - slack_mm
        })
    }

    /// The lowest a cut may come down to `along_mm` along the bit's path
    /// from the tab's centre: the top over a rect tab's span; over a
    /// smooth tab's, the top within the tool radius of its centre, then
    /// straight down to the cut's floor over half its width, the slope the
    /// last pass cuts (an earlier pass's is higher). The margin lets a cut
    /// come [`TAB_SPAN_MARGIN`] of the radius nearer.
    fn lowest_z_mm(&self, along_mm: f64) -> f64 {
        let d = along_mm + self.margin_mm;
        if d >= self.hold_mm + self.slope_mm {
            return f64::NEG_INFINITY;
        }
        match self.shape {
            TabShape::Rect => self.top_z_mm,
            TabShape::Smooth => {
                let f = ((d - self.hold_mm) / self.slope_mm).clamp(0.0, 1.0);
                self.top_z_mm - (self.top_z_mm - self.floor_z_mm) * f
            }
        }
    }
}

/// S9's tab rule (#337) for one move `mv` of `span`: `false` when it is a
/// straight plunge that ends below a tab's top within the tab's span
/// along the bit's path ([`TabSpine`]).
fn keeps_tabs(
    mv: &MachineMove,
    transform: &MachineTransform,
    span: &OpSpan<'_>,
    slack_mm: f64,
) -> bool {
    if span.tabs.is_empty() {
        return true;
    }
    let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
        (mv.from.x, mv.from.y, mv.from.z, mv.to.x, mv.to.y, mv.to.z)
    else {
        return true;
    };
    let from = transform.to_project(Point3::new(fx, fy, fz), mv.units);
    let to = transform.to_project(Point3::new(tx, ty, tz), mv.units);
    let plunge = from.xy().distance_to(to.xy()) <= slack_mm && to.z < from.z - EPS_MM;
    !plunge
        || span
            .tabs
            .iter()
            .all(|tab| to.z >= tab.top_z_mm - slack_mm || !tab.cuts_into(to.xy(), to.z, slack_mm))
}

/// S9 for `span`: every cutting move in its lines stays in its material.
///
/// # Errors
///
/// [`ExportError::OffMaterial`] at the first move that leaves it, or whose
/// position in XY is not known.
pub(crate) fn check(
    blocks: &[Block],
    post: &PostProcessor,
    transform: &MachineTransform,
    tolerances: Tolerances,
    span: &OpSpan<'_>,
) -> Result<(), ExportError> {
    #[cfg(test)]
    if matches!(span.material, Material::Unchecked) {
        return Ok(());
    }
    let radius_mm = span.tool.diameter_mm / 2.0;
    // #221: each On line loop and held open path, measured against itself.
    let departure_mm = tolerances.flatten_mm + tolerances.arc_fit_mm.unwrap_or(0.0) + EPS_MM;
    let selves = match span.material {
        Material::OnLineProfile { areas, open } => Some(SelfBands::new(
            areas,
            open,
            MEETING_REACH_DIAMETERS * 2.0 * radius_mm,
            tolerances.flatten_mm,
            radius_mm - departure_mm,
        )),
        _ => None,
    };
    // The region the last cut ended in, and where.
    let mut last: Option<(usize, Point)> = None;
    let moves = blocks
        .iter()
        .filter(|b| span.lines.contains(&b.line))
        .filter_map(|b| b.motion.as_ref());
    for mv in moves {
        if !mv.kind.is_cut() {
            last = None;
            continue;
        }
        let slack_mm = quantum_mm(post, mv.units) + departure_mm;
        if !keeps_tabs(mv, transform, span, slack_mm) {
            return Err(ExportError::OffMaterial {
                op: span.op.id().clone(),
                line: mv.line,
            });
        }
        let path = project_path(mv, transform);
        let inside = path.is_some_and(|path| match span.material {
            #[cfg(test)]
            Material::Unchecked => true,
            Material::Unresolved => false,
            Material::Inside {
                areas,
                allowance_mm,
                relief,
                laps,
            } => {
                let clearance_mm = radius_mm + allowance_mm - slack_mm;
                let notch = relief.then_some(Notch {
                    radius_mm,
                    allowance_mm: *allowance_mm,
                    slack_mm,
                });
                in_regions(areas, path, clearance_mm, notch, &mut last)
                    || (*laps && is_lap(areas, path, clearance_mm, radius_mm + slack_mm, notch))
            }
            Material::Outside { areas, relief } => {
                let clearance_mm = radius_mm - slack_mm;
                let notch = relief.then_some(Notch {
                    radius_mm,
                    allowance_mm: 0.0,
                    slack_mm,
                });
                outside_regions(areas, path, clearance_mm, notch)
                    || is_lap(areas, path, clearance_mm, radius_mm + slack_mm, notch)
            }
            Material::Drill { holes } => points_of(path).iter().all(|&p| {
                holes
                    .iter()
                    .any(|(centre, r)| p.distance_to(*centre) <= r + slack_mm)
            }),
            Material::OnLineProfile { areas, open } => {
                let band = Band {
                    band_mm: 2.0 * radius_mm - slack_mm,
                    reach_mm: MEETING_REACH_DIAMETERS * 2.0 * radius_mm + slack_mm,
                    slack_mm,
                };
                let (own_areas, own_open, gone) =
                    selves.as_ref().map_or((&[][..], &[][..], &[][..]), |s| {
                        (&s.areas[..], &s.open[..], &s.gone[..])
                    });
                on_own_line(areas, own_areas, gone, path, band)
                    || on_open_path(open, own_open, path, band)
            }
            Material::SideProfile {
                centres,
                keep_clear,
            } => {
                let clearance_mm = radius_mm - slack_mm;
                let bounds = bounds_of(path);
                points_of(path)
                    .iter()
                    .all(|&p| on_line(centres, p, slack_mm))
                    && keep_clear
                        .iter()
                        .filter(|e| near(bounds, e.bounds, clearance_mm))
                        .all(|e| segment_distance_mm(path, e.segment) >= clearance_mm)
            }
            Material::OnLine { edges } => {
                points_of(path).iter().all(|&p| on_line(edges, p, slack_mm))
            }
        });
        if !inside {
            return Err(ExportError::OffMaterial {
                op: span.op.id().clone(),
                line: mv.line,
            });
        }
    }
    Ok(())
}

/// Whether tool-centre path `path` keeps `clearance_mm` from the edges
/// `scope` of `area`, or is a relief notch at one of their corners when
/// `notch` is set.
fn walls_allow(
    area: &Area,
    scope: &Range<usize>,
    path: Segment,
    clearance_mm: f64,
    notch: Option<Notch>,
) -> bool {
    let bounds = bounds_of(path);
    let clear = area
        .edges
        .get(scope.clone())
        .unwrap_or_default()
        .iter()
        .filter(|e| near(bounds, e.bounds, clearance_mm))
        .all(|e| segment_distance_mm(path, e.segment) >= clearance_mm);
    clear || notch.is_some_and(|n| n.allows(area, scope, path, clearance_mm))
}

/// Whether tool-centre path `path` lies in one of `areas`, at least
/// `clearance_mm` from its outline and its islands', or is a relief notch
/// when `notch` is set. The area is the one the last cut ended in, if
/// `path` starts there (`last`), else the one that holds its start; `last`
/// is updated to where `path` ends.
fn in_regions(
    areas: &[Area],
    path: Segment,
    clearance_mm: f64,
    notch: Option<Notch>,
    last: &mut Option<(usize, Point)>,
) -> bool {
    let index = match *last {
        Some((i, end)) if end.distance_to(path.start) <= EPS_MM => Some(i),
        _ => areas
            .iter()
            .position(|a| a.region.contains_point(path.start)),
    };
    let Some((index, area)) = index.and_then(|i| Some((i, areas.get(i)?))) else {
        *last = None;
        return false;
    };
    let clear = walls_allow(area, &(0..area.edges.len()), path, clearance_mm, notch);
    *last = clear.then_some((index, path.end));
    clear
}

/// The point of `path` that tells which side of an outline it is on: its
/// middle, not an end (#231). A T-bone notch's return leg starts on the
/// wall it slots into, and the program's rounding moves that end up to a
/// quantum to either side, so its start tells nothing; its middle lies in
/// the waste. A path whose middle is on the waste side but which reaches
/// into the part crosses a wall, which the clearance test after this one
/// refuses.
fn side_point(path: Segment) -> Point {
    let [_, middle, _] = points_of(path);
    middle
}

/// Whether tool-centre path `path` lies outside every one of `areas`, at
/// least `clearance_mm` from all their outlines, or is a relief notch at
/// one of their corners when `notch` is set. Only the areas whose bounds
/// come within the clearance of the path's are looked at: the path cannot
/// be in or near the others. Its side is judged at [`side_point`].
fn outside_regions(areas: &[Area], path: Segment, clearance_mm: f64, notch: Option<Notch>) -> bool {
    let bounds = bounds_of(path);
    let side = side_point(path);
    areas
        .iter()
        .filter(|a| near(bounds, a.bounds, clearance_mm.max(0.0)))
        .all(|a| {
            !a.region.contains_point(side)
                && walls_allow(a, &(0..a.edges.len()), path, clearance_mm, notch)
        })
}

/// The lap rule (module docs): whether tool-centre path `path` runs along
/// one loop of `areas`, its ends and middle within `reach_mm` of it, on
/// its waste side (judged at [`side_point`]), and clear of every loop of that loop's own area (or a
/// notch at one of their corners). Other areas' outlines are not looked
/// at; the loop's own area's always are (#192).
fn is_lap(
    areas: &[Area],
    path: Segment,
    clearance_mm: f64,
    reach_mm: f64,
    notch: Option<Notch>,
) -> bool {
    let bounds = bounds_of(path);
    let points = points_of(path);
    areas
        .iter()
        .filter(|a| near(bounds, a.bounds, reach_mm))
        .any(|area| {
            area.loops.iter().any(|l| {
                let edges = area.edges.get(l.edges.clone()).unwrap_or_default();
                points.iter().all(|&p| {
                    edges
                        .iter()
                        .any(|e| point_segment_distance_mm(e.segment, p) <= reach_mm)
                }) && l
                    .alone
                    .as_ref()
                    .is_some_and(|r| r.contains_point(side_point(path)) == l.waste_inside)
                    && walls_allow(area, &(0..area.edges.len()), path, clearance_mm, notch)
            })
        })
}

/// The On line Profile rule (module docs): whether tool-centre path
/// `path` lies on one loop of `areas`, its ends and middle within the
/// slack of it, at least the band from every other loop of that loop's
/// area (#199), and from the stretches of its own loop far along it
/// (#221, [`keeps_self`]; `selves` holds each loop's [`SelfBand`]), and
/// on no loop of an area the cut removes whole (#223, `gone`). Other
/// areas' loops are not looked at.
fn on_own_line(
    areas: &[Area],
    selves: &[Vec<SelfBand>],
    gone: &[bool],
    path: Segment,
    band: Band,
) -> bool {
    let Band {
        band_mm, slack_mm, ..
    } = band;
    let bounds = bounds_of(path);
    let points = points_of(path);
    areas
        .iter()
        .zip(selves)
        .zip(gone)
        .filter(|((a, _), gone)| !**gone && near(bounds, a.bounds, slack_mm))
        .any(|((area, own), _)| {
            area.loops.iter().zip(own).any(|(l, own)| {
                let edges = area.edges.get(l.edges.clone()).unwrap_or_default();
                points.iter().all(|&p| on_line(edges, p, slack_mm))
                    && area
                        .edges
                        .iter()
                        .enumerate()
                        .filter(|(i, e)| !l.edges.contains(i) && near(bounds, e.bounds, band_mm))
                        .all(|(_, e)| segment_distance_mm(path, e.segment) >= band_mm)
                    && keeps_self(own, path, band, |p, q| {
                        material_between(&area.region, edges, p, q)
                    })
            })
        })
}

/// The On line rule for open paths (module docs, #210): whether tool-centre
/// path `path` lies on one of `open`, its ends and middle within the
/// slack of it, and keeps the band from every line beside that path once
/// the stretches of both within the reach of a point where they meet are
/// set aside, and from the stretches of the path itself far along it
/// when a region holds it (#221; `selves` holds each path's
/// [`SelfBand`]).
fn on_open_path(open: &[OpenPath], selves: &[Option<SelfBand>], path: Segment, band: Band) -> bool {
    let Band {
        band_mm,
        reach_mm,
        slack_mm,
    } = band;
    let bounds = bounds_of(path);
    let points = points_of(path);
    open.iter().zip(selves).any(|(o, own)| {
        points.iter().all(|&p| on_line(&o.edges, p, slack_mm))
            && o.beside.iter().all(|b| {
                let pieces = path.outside_discs(&b.meetings, reach_mm);
                b.edges
                    .iter()
                    .filter(|e| near(bounds, e.bounds, band_mm))
                    .all(|e| {
                        pieces
                            .iter()
                            .all(|&p| segment_distance_mm(p, e.segment) >= band_mm)
                            && e.segment
                                .outside_discs(&b.meetings, reach_mm)
                                .into_iter()
                                .all(|q| segment_distance_mm(path, q) >= band_mm)
                    })
            })
            && own
                .as_ref()
                .is_none_or(|own| keeps_self(own, path, band, |_, _| true))
    })
}

/// The On line band rule for one move and one line (#221): the band,
/// mm, the reach about a meeting point within which a pair is set aside,
/// mm, and the slack a written move may depart by, mm.
#[derive(Debug, Clone, Copy)]
struct Band {
    band_mm: f64,
    reach_mm: f64,
    slack_mm: f64,
}

/// Each On line loop of each area, and each open path a region holds,
/// as a [`SelfBand`] (#221): the loops in [`Area::loops`] order, the open
/// paths in `open` order, `None` for a path no region holds. And whether
/// each area is a part the cut removes whole (#223).
struct SelfBands {
    areas: Vec<Vec<SelfBand>>,
    open: Vec<Option<SelfBand>>,
    gone: Vec<bool>,
}

impl SelfBands {
    /// The bands of `areas` and `open`, compared beyond `reach_mm` along
    /// each line and meeting where they come within `tolerance_mm` of
    /// themselves, as `dc_cam::generate_profile` measures them. An area
    /// is gone when its outline shrunk by `inset_mm` (the tool radius less
    /// the slack a program may depart by, so S9 never refuses what the
    /// generator cuts) is empty: every point of the part lies within the
    /// bit's reach of its line.
    fn new(
        areas: &[Area],
        open: &[OpenPath],
        reach_mm: f64,
        tolerance_mm: f64,
        inset_mm: f64,
    ) -> Self {
        let tolerance_mm = tolerance_mm.max(EPS_MM);
        Self {
            gone: areas
                .iter()
                .map(|a| inset_is_empty(a.region.outer(), inset_mm))
                .collect(),
            areas: areas
                .iter()
                .map(|a| {
                    a.loops
                        .iter()
                        .map(|l| SelfBand::new(&l.line, reach_mm, tolerance_mm))
                        .collect()
                })
                .collect(),
            open: open
                .iter()
                .map(|o| {
                    o.held
                        .as_ref()
                        .map(|line| SelfBand::new(line, reach_mm, tolerance_mm))
                })
                .collect(),
        }
    }
}

/// #221: whether tool-centre path `path`, which runs on the line `own`
/// measures, keeps `band.band_mm` from the stretches of that line that
/// lie more than the reach along it from where `path` runs, for every
/// pair of closest points `counts` accepts. The stretches of both within
/// `band.reach_mm` of a point where the line meets itself are set aside,
/// as for two lines (#210).
fn keeps_self(
    own: &SelfBand,
    path: Segment,
    band: Band,
    counts: impl Fn(Point, Point) -> bool,
) -> bool {
    let far = own.far_from(path, band.slack_mm, band.band_mm);
    if far.is_empty() {
        return true;
    }
    let pieces = path.outside_discs(own.meetings(), band.reach_mm);
    far.into_iter().all(|stretch| {
        let away = stretch.outside_discs(own.meetings(), band.reach_mm);
        pieces
            .iter()
            .map(|&p| (p, stretch))
            .chain(away.into_iter().map(|a| (path, a)))
            .all(|(s, t)| {
                let (p, q) = closest_points(s, t);
                p.distance_to(q) >= band.band_mm || !counts(p, q)
            })
    })
}

/// Whether the part lies between points `p` and `q` of one loop of
/// `region`, whose segments are `edges` (#221, as `dc_cam` decides it):
/// the point midway lies in the region's material, a quarter of their gap
/// or more from the loop. Across a notch or a hole's neck it lies in the
/// waste; between two points along one edge it lies on the loop.
fn material_between(region: &Region, edges: &[Edge], p: Point, q: Point) -> bool {
    let middle = p + (q - p) * 0.5;
    let depth_mm = p.distance_to(q) / 4.0;
    region.contains_point(middle)
        && edges
            .iter()
            .all(|e| point_segment_distance_mm(e.segment, middle) >= depth_mm)
}

/// Whether `p` lies within `slack_mm` of one of `edges`.
fn on_line(edges: &[Edge], p: Point, slack_mm: f64) -> bool {
    let at = Rect::from_point(p);
    edges
        .iter()
        .filter(|e| near(at, e.bounds, slack_mm))
        .any(|e| point_segment_distance_mm(e.segment, p) <= slack_mm)
}

/// The ends and the middle of `path`.
fn points_of(path: Segment) -> [Point; 3] {
    let middle = path
        .point_at(path.length_mm() / 2.0)
        .map_or(path.start, |(p, _)| p);
    [path.start, middle, path.end]
}

/// Move `mv`'s tool-centre path in XY, project space: a line, or an arc
/// as the controller turns it (a mirroring axis map swaps its direction).
/// `None` when a start or end coordinate is not known.
pub(crate) fn project_path(mv: &MachineMove, transform: &MachineTransform) -> Option<Segment> {
    let (sx, sy, ex, ey) = (mv.from.x?, mv.from.y?, mv.to.x?, mv.to.y?);
    let to_project = |x: f64, y: f64| transform.to_project(Point3::new(x, y, 0.0), mv.units).xy();
    let (start, end) = (to_project(sx, sy), to_project(ex, ey));
    let machine_cw = match mv.kind {
        MotionKind::ArcCw => true,
        MotionKind::ArcCcw => false,
        MotionKind::Rapid | MotionKind::Linear => {
            return Some(Segment {
                start,
                end,
                bulge: 0.0,
            });
        }
    };
    let machine_centre = match mv.arc.r {
        Some(r) => r_centre(sx, sy, ex, ey, r, machine_cw),
        None => Point::new(sx + mv.arc.i.unwrap_or(0.0), sy + mv.arc.j.unwrap_or(0.0)),
    };
    let centre = to_project(machine_centre.x, machine_centre.y);
    let ccw = machine_cw == transform.mirrors_xy();
    let sweep_rad = Arc::from_ends(start, end, centre, ccw).sweep_rad;
    Some(Segment {
        start,
        end,
        bulge: (sweep_rad / 4.0).tan(),
    })
}
