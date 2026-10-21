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
//! - **Drill:** every cut stays inside one of the Drill's holes, the
//!   closed targets that are circles ([`dc_geometry::as_circle`], as
//!   `dc_cam::generate_drill` finds them): its ends and middle lie within
//!   the hole's radius of its centre. A Drill only plunges, so that is the
//!   hole it drills.
//! - **Engrave:** the tool centre stays on the line: every cut's ends and
//!   middle lie on one of the Engrave's target paths.
//! - **Profile:** nothing more is checked here. Its cuts are its targets
//!   offset by the tool radius, which S6 holds the program to; S7 keeps
//!   them on the stock; and dc-cam's overlap check keeps one shape's cut
//!   out of another's part.
//!
//! A move is allowed to depart from where the toolpath put it by what the
//! program may already differ by: one unit in the last written place (the
//! numbers' rounding), the flatten tolerance (arcs written as chords, and
//! the tool-centre outlines and raster ends the generator flattens), and
//! the arc-fit tolerance when arc fitting is on (a run of chords written
//! as an arc, S12).

use dc_geometry::{
    Arc, Contour, EPS_MM, Point, Point3, Polyline, Rect, Region, Segment, as_circle, nest,
    point_segment_distance_mm, segment_distance_mm,
};
use dc_project::{InstanceId, Operation, PostProcessor, Project};

use crate::ExportError;
use crate::parse::{Block, MachineMove, MotionKind};
use crate::safety::{OpSpan, Tolerances, quantum_mm, r_centre};
use crate::transform::MachineTransform;

/// The material one operation may cut, worked out from the project.
#[derive(Debug, Clone)]
pub(crate) enum Material {
    /// A Profile: S6, S7 and dc-cam's overlap check cover it.
    Profile,
    /// A Pocket's regions, each with its outline segments.
    Pocket {
        /// The regions, outer and islands, from the targets' closed
        /// profiles.
        areas: Vec<Area>,
        /// The allowance left on every wall and island, mm.
        allowance_mm: f64,
    },
    /// A Drill's holes: centre and radius, mm.
    Drill {
        /// Each hole.
        holes: Vec<(Point, f64)>,
    },
    /// An Engrave's target paths, as segments.
    Engrave {
        /// Every segment of every path.
        edges: Vec<Edge>,
    },
    /// The targets do not resolve to anything an operation of this kind
    /// cuts (an unknown instance, crossing outlines, an invalid
    /// allowance): no cut is in the material, fail-closed.
    Unresolved,
}

/// One region and its outline segments.
#[derive(Debug, Clone)]
pub(crate) struct Area {
    region: Region,
    edges: Vec<Edge>,
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

impl Material {
    /// The material `op` may cut in `project`.
    pub(crate) fn of(project: &Project, op: &Operation) -> Self {
        match op {
            Operation::Profile(_) => Self::Profile,
            Operation::Pocket(pocket) => {
                let allowance_mm = pocket.allowance_mm;
                if !(allowance_mm.is_finite() && allowance_mm >= 0.0) {
                    return Self::Unresolved;
                }
                let Some(paths) = placed(project, &pocket.targets) else {
                    return Self::Unresolved;
                };
                let contours: Option<Vec<Contour>> = paths
                    .into_iter()
                    .filter(Polyline::is_closed)
                    .map(|p| Contour::new(p).ok())
                    .collect();
                let Some(Ok((regions, _))) = contours.map(nest) else {
                    return Self::Unresolved;
                };
                let areas = regions
                    .into_iter()
                    .map(|region| {
                        let edges = std::iter::once(region.outer())
                            .chain(region.holes())
                            .flat_map(|c| edges_of(c.polyline()))
                            .collect();
                        Area { region, edges }
                    })
                    .collect();
                Self::Pocket {
                    areas,
                    allowance_mm,
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
                Some(paths) => Self::Engrave {
                    edges: paths.iter().flat_map(edges_of).collect(),
                },
                None => Self::Unresolved,
            },
        }
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
    if matches!(span.material, Material::Profile) {
        return Ok(());
    }
    let radius_mm = span.tool.diameter_mm / 2.0;
    let departure_mm = tolerances.flatten_mm + tolerances.arc_fit_mm.unwrap_or(0.0) + EPS_MM;
    // The Pocket region the last cut ended in, and where.
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
        let path = project_path(mv, transform);
        let inside = path.is_some_and(|path| match span.material {
            Material::Profile => true,
            Material::Unresolved => false,
            Material::Pocket {
                areas,
                allowance_mm,
            } => {
                let clearance_mm = radius_mm + allowance_mm - slack_mm;
                in_pocket(areas, path, clearance_mm, &mut last)
            }
            Material::Drill { holes } => points_of(path).iter().all(|&p| {
                holes
                    .iter()
                    .any(|(centre, r)| p.distance_to(*centre) <= r + slack_mm)
            }),
            Material::Engrave { edges } => {
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

/// Whether tool-centre path `path` lies in one of `areas`, at least
/// `clearance_mm` from its outline and its islands'. The area is the one
/// the last cut ended in, if `path` starts there (`last`), else the one
/// that holds its start; `last` is updated to where `path` ends.
fn in_pocket(
    areas: &[Area],
    path: Segment,
    clearance_mm: f64,
    last: &mut Option<(usize, Point)>,
) -> bool {
    let index = match *last {
        Some((i, end)) if end.distance_to(path.start) <= EPS_MM => Some(i),
        _ => areas
            .iter()
            .position(|a| a.region.contains_point(path.start)),
    };
    let Some((index, area)) = index.and_then(|i| Some((i, areas.get(i)?))) else {
        return false;
    };
    let bounds = bounds_of(path);
    let clear = area
        .edges
        .iter()
        .filter(|e| near(bounds, e.bounds, clearance_mm))
        .all(|e| segment_distance_mm(path, e.segment) >= clearance_mm);
    *last = clear.then_some((index, path.end));
    clear
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
fn project_path(mv: &MachineMove, transform: &MachineTransform) -> Option<Segment> {
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
