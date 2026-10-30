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
//!   its stock to leave, and outside their islands. For a round tool of radius
//!   `r` that is exactly: the move's tool-centre path lies inside a region
//!   and at least `r + stock to leave` from its outline and from every island's.
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
//!   at most touches it) and at most `(r + stock to leave) / sin(θ/2)` from it
//!   (the tool-centre path's miter point, where a notch starts); the cut
//!   must also keep its clearance from every other edge. So a notch can
//!   bite at most `r` past one of its corner's two walls, and nowhere else.
//! - **Profile** (#177): the waste the Profile removes, from its targets'
//!   closed profiles nested as `dc_cam::generate_profile` nests them, and
//!   every cutting move is held to it: laps, tab rises, ramp and helix
//!   descents, lead arcs and overcuts, and notches.
//!   - **Inside:** as a Pocket with no stock to leave: the tool centre stays in
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
//!     other side) and from the strip's two ends, the normals through the
//!     path's end vertices one radius long, each cut short at the first
//!     line it meets ([`dc_geometry::strip_ends`], #342): the cutter never
//!     crosses a line nor reaches the strip one radius wide that a line
//!     keeps, past an end included, wherever the offset runs, as `dc_cam`
//!     refuses (`CamError::CutsIntoKeptSide`). Closed targets are not cut. The
//!     ramp and the tabs run along the offset path; there is no lead or
//!     notch. With a last pass (#311) the path offset by the radius plus
//!     the stock to leave is a tool-centre path too, for the passes before the
//!     last; the kept edges stay one radius off.
//!     The last pass's level step across the stock to leave from the earlier
//!     passes' path onto its own (#339) runs between the two paths, or
//!     between one segment's two offsets taken whole (#358): where an
//!     end vertex comes back beside another segment, the two paths stop
//!     on circles of `r` and `r + a` round it rather than on one normal.
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
//!   offset by the tool radius either way, within the slack) less than
//!   half the tab's width plus the radius from the tab's centre along
//!   that path, the stretch a lap rises over (ADR-0005), less a tenth of
//!   the radius for a rect tab's step at its end. On the path of the
//!   passes before a last pass (the radius plus its stock to leave, #353),
//!   which run beside the tab rather than over it, it is refused when its
//!   cutter reaches into the tab material the laps leave: within the
//!   radius of a lap held above the plunge's Z by that stretch (without
//!   the tenth) and at least the radius from the rest of the outline's
//!   laps, which cut to their floor. The tabs are the project's placed tabs
//!   on the Profile's targets and those its tab request places
//!   (`dc_cam::placed_tabs::place`, the documented layout rule, not the
//!   toolpath). So a lap that starts on a tab and plunges there, below
//!   its top, is refused, wherever on the tab it starts.
//! - **Trenches** (#352): an inside or outside Profile cuts only a trench,
//!   its kerf, and the lines its entries ramp on; the waste beside them
//!   stands full height. So a ramp below the stock top must run where
//!   the operation's earlier cuts ran. A cut that falls while it runs in
//!   XY (a ramp leg, a helix chord), in a run of such cuts that started
//!   below the stock top, is refused at the first point below the stock
//!   top, checked a tenth of the tool radius apart, that lies neither on
//!   the ring the laps cut (within the tool radius, plus a last pass's
//!   stock to leave and the slack, of a target outline) nor within twice the
//!   slack of the tool-centre path of an earlier cut of the operation
//!   where it ran below the stock top: the stock top itself, with no
//!   slack, so a ramp leg that dips below it only at its end cut a trench
//!   only there (#356). A run that starts at or above the stock top (a
//!   first pass's ramp or helix, a helix that starts again from above the
//!   stock) clears its own way and is not held to it, and a straight
//!   plunge is the tab rule's. A level move too short to fall a unit in
//!   the last place does not end a run. Pockets clear their whole region
//!   and are not held to it.
//! - **Drill:** every cut stays inside one of the Drill's holes, the
//!   closed targets that are circles ([`dc_geometry::as_circle`], as
//!   `dc_cam::generate_drill` finds them): its ends and middle lie within
//!   the hole's radius of its centre. A Drill only plunges, so that is the
//!   hole it drills.
//! - **Countersink Drill, S18** (format 21, #427; [`countersink`] has the
//!   rule): every line of its span, not only its cuts. No canned cycle;
//!   every cut a `G1` straight down at the centre of one of its holes
//!   (the target circles less those the mouth rule skips), from no higher
//!   than the retract plane to exactly the start plane less the depth its
//!   mouth and its V-bit's angle derive, within rounding; every rapid at
//!   or above the retract plane; and the flank engaged no deeper than the
//!   cone, from the start plane when the stock top is it or an enabled
//!   earlier Pocket or V-Carve floor at the start depth cleared it
//!   ([`dc_project::cleared_start_plane`]), else from the stock top. The
//!   tool must be the operation's, a V-bit; anything else is unresolved.
//! - **Bore** (format 18; maintainer decision 2026-10-24): every cut stays
//!   in one of the Bore's holes, the closed targets that are circles, as
//!   for a Drill: its ends and middle lie within `ρ − r − stock to leave` of the
//!   hole's centre (radius `ρ`, tool radius `r`), all three in the same
//!   hole. That is the helix's own circle, so a helix, circle or move to
//!   the centre that reaches wider would cut the wall beyond its drawn
//!   size; a hole narrower than the tool allows no cut at all.
//! - **Engrave:** the tool centre stays on the line: every cut's ends and
//!   middle lie on one of the Engrave's target paths.
//! - **V-Carve, the V-bit's run: S17** (format 20, #404, exact since #408;
//!   `docs/specs/phase-3/design.md`, "Safety pass"; [`cone`] has the
//!   geometry). The regions are the targets' closed profiles nested as for
//!   a Pocket, the start plane is `start_depth_mm` below the stock top, and
//!   the flank is engaged from the stock top unless an earlier floor at
//!   the start depth holds the targets, an enabled Pocket's or an enabled
//!   V-Carve's flat floor ([`dc_project::cleared_start_plane`], #426, as
//!   the generator and validation decide it; security review H4). Every
//!   cutting move:
//!   - its tip (the tool-centre path) stays in the regions: its ends and
//!     middle inside or within the slack of an outline, and it meets no
//!     outline away from its ends;
//!   - a straight descent (no XY motion beyond the slack) that stays above
//!     the start plane runs through the air over the carve and is held
//!     only to that; any other descent is held at its lowest point, where
//!     its cone is widest, as a cut (H3, see below);
//!   - any other cut runs no higher than the start plane;
//!   - nothing reaches below the flat depth (`ConeOffMaterial`), nor
//!     engages the flank deeper than the cone is tall
//!     (`ConeHeightExceeded`; a tip below the cone height is this too);
//!   - the cone's disc at the start plane stays inside the regions all
//!     along the move, measured exactly ([`cone`]): segment against
//!     segment, arcs as arcs, at constant Z; in closed form for a straight
//!     move whose Z varies. No sampling.
//!
//!   **H3.** The design's exemption skips S17 for a straight descent from
//!   the stock top plus the entry clearance that ends no lower than a
//!   later cut through its XY, relying on that later cut. This pass skips
//!   nothing below the start plane: every descent's lowest point is held
//!   to the tip, depth, shank and cone tests directly. A cone at one XY
//!   contains every cone above it there, so that point is the whole of
//!   what the descent cuts, and the rule is the stricter of the two. It
//!   also accepts a descent that starts lower, where S2 lets the rapid
//!   stop above an earlier cut (#214), which the design's literal form
//!   would refuse in generated programs: with a 0.5 mm flat depth the
//!   floor rings are re-entered from 0.5 mm above the start plane.
//!
//!   The tool must be the operation's, a V-bit with an angle in (0°,
//!   180°); the rule is the same for the medial and the offset strategy,
//!   floors and links included. Anything else is unresolved.
//! - **V-Carve, a clearing run** (#406, security review M3): the run
//!   labelled with one of the carve's passes and cut with that pass's flat
//!   end mill is held as a Pocket of the carve's floor regions with no
//!   stock to leave: the end mill's centre stays inside one, at least its
//!   radius from its outline and islands, and no lower than the floor. The
//!   floor regions are re-derived from the project: the targets' regions
//!   inset by `F · tan(α/2)` ([`dc_geometry::floor_regions`]), less those
//!   too thin to cut (area at most the flatten tolerance times the
//!   perimeter), as the generator's floor drops them. The earlier passes'
//!   reach does not bound the run: a rest pass's footprint runs into what
//!   they cleared, and only its centres must touch what they left.
//!
//! - **Inlay** (format 22, #436): each side is held as a V-Carve, its
//!   V-bit's run to S17 and its clearing runs to its floor, with its
//!   regions, start plane and floor re-derived from the project. The
//!   female carves the targets' regions from the stock top, flat at
//!   `depth_mm` `D`. The male carves the **surround** of its design: the
//!   targets' closed outlines mirrored about the centre line of their
//!   joint bounding box (`Transform::mirror_x_about`), nested, and
//!   [`dc_geometry::surround`] `margin_mm` wide at the project's flatten
//!   tolerance, the generator's construction, never the generator. Its
//!   start plane is `D − G` below the stock top (`glue_gap_mm` `G`), its
//!   floor `plug_depth_mm` below that. Either side's flank is engaged from
//!   the stock top (security review H4): the plug above the male's start
//!   plane is never cleared. A cut inside the plug, or sideways above the
//!   male's start plane, fails as it would for a V-Carve.
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

use std::collections::BTreeMap;
use std::ops::Range;

mod cone;
mod countersink;

use cone::{Cone, Fault};
use countersink::Sink;
use dc_cam::RunLabel;
use dc_geometry::{
    Arc, Contour, EPS_MM, Point, Point3, Polyline, Rect, Region, Segment, SelfBand, Transform,
    Vec2, as_circle, closest_points, floor_regions, inset_is_empty, meeting_points, nearest_point,
    nest, offset, offset_open, point_segment_distance_mm, segment_distance_mm, strip_ends,
    surround,
};
use dc_project::{
    ClearingPass, CornerRelief, InlayOp, InlaySide, InstanceId, Operation, PostProcessor,
    ProfileOp, Project, Side, TabShape, Tool, ToolId, ToolKind, Units, VCarveOp,
    cleared_start_plane,
};

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
        /// The stock to leave left on every wall and island, mm; 0 for a
        /// Profile.
        stock_to_leave_mm: f64,
        /// Corner relief is on, so the notch rule applies.
        relief: bool,
        /// A Profile: the lap rule applies.
        laps: bool,
        /// A Profile whose stock top is known: the trench rule applies
        /// (#352).
        trench: Option<TrenchRule>,
    },
    /// An outside Profile's parts: the tool centre stays outside every
    /// one, clear of its outlines.
    Outside {
        /// The parts, outer and holes, from the targets' closed profiles.
        areas: Vec<Area>,
        /// Corner relief is on, so the notch rule applies.
        relief: bool,
        /// The stock top is known: the trench rule applies (#352).
        trench: Option<TrenchRule>,
    },
    /// A Drill's holes: centre and radius, mm.
    Drill {
        /// Each hole.
        holes: Vec<(Point, f64)>,
    },
    /// A Bore's holes (format 18): centre and radius, mm, and the radial
    /// stock to leave it leaves on their walls.
    Bore {
        /// Each hole.
        holes: Vec<(Point, f64)>,
        /// The stock to leave, mm, 0 or more.
        stock_to_leave_mm: f64,
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
        /// Every segment of every open path, of every path's kept edge
        /// and of its strip's two ends (#342): what the cut keeps the
        /// tool radius from.
        keep_clear: Vec<Edge>,
        /// With a last pass (#339): where its step across the stock to leave,
        /// from the earlier passes' path to its own, may run.
        steps: Option<StockToLeaveBand>,
    },
    /// An Engrave's target paths, as segments.
    OnLine {
        /// Every segment of every path.
        edges: Vec<Edge>,
    },
    /// The V-bit's run of a V-Carve (format 20) or of an Inlay side
    /// (format 22): S17 (module docs, [`cone`]).
    VBit(Cone),
    /// A countersink Drill (format 21): S18 (module docs,
    /// [`countersink`]).
    Countersink(Sink),
    /// A clearing run of a V-Carve or an Inlay side (#406, #436; security
    /// review M3): the end
    /// mill's centre stays inside the carve's floor regions, at least its
    /// radius from their outlines, and no lower than the floor.
    Floor {
        /// The floor regions, outer and islands.
        areas: Vec<Area>,
        /// The floor, mm, project space.
        floor_z_mm: f64,
        /// The pass's end mill's radius, mm.
        radius_mm: f64,
    },
    /// The targets do not resolve to anything an operation of this kind
    /// cuts (an unknown instance, crossing outlines, an invalid stock to
    /// leave), a V-Carve or Inlay whose tool or settings are unusable, or
    /// a run whose label or tool the operation does not have: no cut is in
    /// the material, fail-closed.
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

/// S9's notch rule for one operation: its tool's radius, its stock to leave,
/// and the slack a written move may depart by.
#[derive(Debug, Clone, Copy)]
struct Notch {
    radius_mm: f64,
    stock_to_leave_mm: f64,
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
                let reach_mm = (r + self.stock_to_leave_mm) / c.sin_half + slack;
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
    /// The material the run `label` of `op`, cut with `tool`, may cut in
    /// `project` (#406): a V-Carve's or an Inlay side's own run is held to
    /// S17 and only when cut with the carve's tool; its clearing run with
    /// end mill `t` to its floor, and only when `t` is one of its passes
    /// and the run's tool. Every other operation has only its own run,
    /// which is held as [`Material::of`] says; a clearing run of one is
    /// unresolved.
    pub(crate) fn of_run(project: &Project, op: &Operation, label: &RunLabel, tool: &Tool) -> Self {
        match (op, label) {
            (Operation::VCarve(carve), RunLabel::Main) if tool.id == carve.tool => {
                v_bit_material(project, carve)
            }
            (Operation::VCarve(carve), RunLabel::Clearing(pass)) if tool.id == *pass => {
                floor_material(project, carve, pass)
            }
            // Format 22 (#436): an Inlay side's runs, as a V-Carve's.
            (Operation::Inlay(inlay), RunLabel::Main) if tool.id == inlay.tool => {
                inlay_material(project, inlay)
            }
            (Operation::Inlay(inlay), RunLabel::Clearing(pass)) if tool.id == *pass => {
                floor_of(project, &inlay.clearing, pass, || {
                    inlay_cone(project, inlay)
                })
            }
            (Operation::VCarve(_) | Operation::Inlay(_), _)
            | (_, RunLabel::Clearing(_) | RunLabel::CutOut(_)) => Self::Unresolved,
            // S18 holds a countersink to its own V-bit's angle and cone.
            (Operation::Drill(drill), RunLabel::Main)
                if drill.cycle.is_countersink() && tool.id != drill.tool =>
            {
                Self::Unresolved
            }
            (_, RunLabel::Main) => Self::of(project, op),
        }
    }

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
                            let stock_to_leave_mm =
                                profile.last_pass.as_ref().map(|l| l.stock_to_leave_mm);
                            side_profile(&paths, left_mm, stock_to_leave_mm)
                        }
                        _ => Self::Unresolved,
                    },
                    Side::Inside => match regions_of(project, &profile.targets) {
                        Some(regions) => Self::Inside {
                            areas: areas_of(regions, true),
                            stock_to_leave_mm: 0.0,
                            relief,
                            laps: true,
                            trench: trench_rule(project, profile),
                        },
                        None => Self::Unresolved,
                    },
                    Side::Outside => match regions_of(project, &profile.targets) {
                        Some(regions) => Self::Outside {
                            areas: areas_of(regions, false),
                            relief,
                            trench: trench_rule(project, profile),
                        },
                        None => Self::Unresolved,
                    },
                }
            }
            Operation::Pocket(pocket) => {
                let stock_to_leave_mm = pocket.stock_to_leave_mm;
                if !(stock_to_leave_mm.is_finite() && stock_to_leave_mm >= 0.0) {
                    return Self::Unresolved;
                }
                match regions_of(project, &pocket.targets) {
                    Some(regions) => Self::Inside {
                        areas: areas_of(regions, true),
                        stock_to_leave_mm,
                        relief: pocket.corner_relief != CornerRelief::None,
                        laps: false,
                        trench: None,
                    },
                    None => Self::Unresolved,
                }
            }
            // Format 21: S18 (#427), the plunge at each hole's centre to
            // the derived depth.
            Operation::Drill(drill) if drill.cycle.is_countersink() => {
                Sink::of(project, drill).map_or(Self::Unresolved, Self::Countersink)
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
            Operation::Bore(bore) => {
                let stock_to_leave_mm = bore.stock_to_leave_mm;
                if !(stock_to_leave_mm.is_finite() && stock_to_leave_mm >= 0.0) {
                    return Self::Unresolved;
                }
                match placed(project, &bore.targets) {
                    Some(paths) => Self::Bore {
                        holes: paths
                            .iter()
                            .filter(|p| p.is_closed())
                            .filter_map(|p| as_circle(p, project.flatten_tolerance_mm))
                            .collect(),
                        stock_to_leave_mm,
                    },
                    None => Self::Unresolved,
                }
            }
            Operation::Engrave(engrave) => match placed(project, &engrave.targets) {
                Some(paths) => Self::OnLine {
                    edges: paths.iter().flat_map(edges_of).collect(),
                },
                None => Self::Unresolved,
            },
            Operation::VCarve(carve) => v_bit_material(project, carve),
            // Format 22 (#436): S17 on the side's carve, the male's being
            // the surround of its mirrored targets.
            Operation::Inlay(inlay) => inlay_material(project, inlay),
        }
    }
}

/// The V-bit material of V-Carve `carve` (module docs, [`cone`]): its
/// regions, start plane, slope, flat-depth floor, engagement and cone
/// height, all from the project; [`Material::Unresolved`] when its tool
/// is not a usable V-bit, its start or flat depth is not usable, or its
/// targets do not resolve.
fn v_bit_material(project: &Project, carve: &VCarveOp) -> Material {
    match cone_of(project, carve) {
        Some(cone) => Material::VBit(cone),
        None => Material::Unresolved,
    }
}

/// The V-bit `id` of `project` as S17 needs it: its slope `tan(α/2)` and
/// its cone height, mm; `None` when it is missing or not a V-bit with an
/// angle in (0°, 180°) and a positive diameter.
fn v_bit_of(project: &Project, id: &ToolId) -> Option<(f64, f64)> {
    let tool = project.tools.iter().find(|t| &t.id == id)?;
    let ToolKind::VBit { included_angle_deg } = tool.cutter else {
        return None;
    };
    let slope = (included_angle_deg.to_radians() / 2.0).tan();
    let usable = included_angle_deg > 0.0
        && included_angle_deg < 180.0
        && slope.is_finite()
        && slope > 0.0
        && tool.diameter_mm.is_finite()
        && tool.diameter_mm > 0.0;
    usable.then(|| (slope, tool.diameter_mm / (2.0 * slope)))
}

/// [`v_bit_material`]'s cone, `None` when it is unresolved.
fn cone_of(project: &Project, carve: &VCarveOp) -> Option<Cone> {
    let thickness_mm = project.stock.thickness_mm?;
    let (slope, cone_mm) = v_bit_of(project, &carve.tool)?;
    let start_depth_mm = carve.start_depth_mm;
    let flat_usable = carve.flat_depth_mm.is_none_or(|f| f.is_finite() && f > 0.0);
    let usable = thickness_mm.is_finite()
        && start_depth_mm.is_finite()
        && start_depth_mm >= 0.0
        && start_depth_mm <= thickness_mm
        && flat_usable;
    if !usable {
        return None;
    }
    let regions = regions_of(project, &carve.targets)?;
    let start_plane_z_mm = thickness_mm - start_depth_mm;
    // H4: the flank is engaged from the stock top unless an earlier
    // floor at the start depth (a Pocket's, or a V-Carve's flat floor,
    // #426) holds the targets, as the generator and validation decide it.
    let cleared_at_start = start_depth_mm <= 0.0
        || cleared_start_plane(project, &Operation::VCarve(carve.clone()))
            .is_some_and(|plane| plane.is_at(start_depth_mm));
    Some(Cone {
        areas: areas_of(regions, true),
        start_plane_z_mm,
        slope,
        floor_z_mm: carve.flat_depth_mm.map(|f| start_plane_z_mm - f),
        engaged_from_z_mm: if cleared_at_start {
            start_plane_z_mm
        } else {
            thickness_mm
        },
        cone_mm,
    })
}

/// The V-bit material of Inlay side `inlay` (#436; module docs, [`cone`]):
/// [`Material::Unresolved`] when [`inlay_cone`] is.
fn inlay_material(project: &Project, inlay: &InlayOp) -> Material {
    inlay_cone(project, inlay).map_or(Material::Unresolved, Material::VBit)
}

/// The cone of Inlay side `inlay`, every value re-derived from the project
/// (design "Inlay" steps 1 to 4, "Safety pass" S17; security review H4):
///
/// - **Female:** the targets' regions, from the stock top, flat at
///   `depth_mm` `D`.
/// - **Male:** the surround of the targets mirrored about the centre line
///   of their closed outlines' joint bounding box ([`male_surround`]);
///   the start plane `D − G` below the stock top, flat at `plug_depth_mm`
///   `P` below that.
///
/// Either side's flank is engaged from the stock top: the female starts
/// there, and the plug above the male's start plane is never cleared.
/// `None` when the tool is not a usable V-bit, a value is unset or out of
/// the range validation allows, or the targets do not resolve.
fn inlay_cone(project: &Project, inlay: &InlayOp) -> Option<Cone> {
    let thickness_mm = project.stock.thickness_mm.filter(|t| t.is_finite())?;
    let (slope, cone_mm) = v_bit_of(project, &inlay.tool)?;
    let depth_mm = inlay.depth_mm.filter(|d| d.is_finite() && *d > 0.0)?;
    let glue_gap_mm = inlay
        .glue_gap_mm
        .filter(|g| g.is_finite() && *g >= 0.0 && *g < depth_mm)?;
    let plug_depth_mm = inlay.plug_depth_mm.filter(|p| p.is_finite() && *p > 0.0)?;
    let (regions, start_depth_mm, flat_depth_mm) = match inlay.side {
        InlaySide::Female => (regions_of(project, &inlay.targets)?, 0.0, depth_mm),
        InlaySide::Male => {
            let margin_mm = inlay.margin_mm.filter(|m| m.is_finite() && *m > 0.0)?;
            (
                male_surround(project, &inlay.targets, margin_mm)?,
                depth_mm - glue_gap_mm,
                plug_depth_mm,
            )
        }
    };
    let start_plane_z_mm = thickness_mm - start_depth_mm;
    Some(Cone {
        areas: areas_of(regions, true),
        start_plane_z_mm,
        slope,
        floor_z_mm: Some(start_plane_z_mm - flat_depth_mm),
        engaged_from_z_mm: thickness_mm,
        cone_mm,
    })
}

/// A male Inlay's carved regions (design "Inlay" step 4): the closed
/// outlines of `targets`, placed, mirrored about X = the centre of their
/// joint bounding box (arc extremes included,
/// [`Transform::mirror_x_about`]), nested, and the [`surround`] of that
/// design `margin_mm` wide at the project's flatten tolerance. Pure
/// geometry on the project, as the generator derives it, never the
/// generator. `None` when a target does not resolve or outlines cross.
fn male_surround(project: &Project, targets: &[InstanceId], margin_mm: f64) -> Option<Vec<Region>> {
    let closed: Vec<Polyline> = placed(project, targets)?
        .into_iter()
        .filter(Polyline::is_closed)
        .collect();
    let mut bounds: Option<Rect> = None;
    for b in closed.iter().filter_map(Polyline::bounds) {
        bounds = Some(match bounds {
            Some(all) => all.including(b.min).including(b.max),
            None => b,
        });
    }
    let contours: Vec<Contour> = match bounds {
        Some(b) => {
            let mirror = Transform::mirror_x_about((b.min.x + b.max.x) / 2.0);
            closed
                .iter()
                .map(|p| {
                    mirror
                        .apply_polyline(p)
                        .ok()
                        .and_then(|m| Contour::new(m).ok())
                })
                .collect::<Option<_>>()?
        }
        None => Vec::new(),
    };
    let (design, _) = nest(contours).ok()?;
    Some(surround(&design, margin_mm, project.flatten_tolerance_mm))
}

/// The material of carve `carve`'s clearing run with end mill `pass`
/// (#406, security review M3; module docs): the floor regions re-derived
/// from the targets, the V-bit's angle and the flat depth
/// ([`floor_regions`], less noise as the generator drops it), at the
/// floor's Z. [`Material::Unresolved`] when `pass` is none of the carve's
/// passes or not a flat end mill, or the carve has no usable V-bit or
/// flat depth.
fn floor_material(project: &Project, carve: &VCarveOp, pass: &ToolId) -> Material {
    floor_of(project, &carve.clearing, pass, || cone_of(project, carve))
}

/// [`floor_material`] for a carve with passes `clearing` whose V-bit
/// material is `cone` (a V-Carve's, or an Inlay side's, #436): the
/// carve's floor regions at its floor's Z.
fn floor_of(
    project: &Project,
    clearing: &[ClearingPass],
    pass: &ToolId,
    cone: impl FnOnce() -> Option<Cone>,
) -> Material {
    let resolved = || {
        if !clearing.iter().any(|p| &p.tool == pass) {
            return None;
        }
        let end_mill = project.tools.iter().find(|t| &t.id == pass)?;
        let radius_mm = end_mill.diameter_mm / 2.0;
        if !(end_mill.cutter.is_flat() && radius_mm.is_finite() && radius_mm > 0.0) {
            return None;
        }
        let cone = cone()?;
        let floor_z_mm = cone.floor_z_mm?;
        let inset_mm = (cone.start_plane_z_mm - floor_z_mm) * cone.slope;
        let tolerance_mm = project.flatten_tolerance_mm;
        let regions: Vec<Region> = cone.areas.into_iter().map(|a| a.region).collect();
        let floors = floor_regions(&regions, inset_mm, tolerance_mm)
            .into_iter()
            .filter(|f| !is_noise(f, tolerance_mm))
            .collect();
        Some(Material::Floor {
            areas: areas_of(floors, true),
            floor_z_mm,
            radius_mm,
        })
    };
    resolved().unwrap_or(Material::Unresolved)
}

/// Whether floor region `region` is too thin to cut at `tolerance_mm`:
/// its area is at most the tolerance times its perimeter, the generator's
/// noise rule for a Pocket's rings and a carve's floor.
fn is_noise(region: &Region, tolerance_mm: f64) -> bool {
    let perimeter_mm: f64 = std::iter::once(region.outer())
        .chain(region.holes())
        .map(|c| c.polyline().length_mm())
        .sum();
    region.area_mm2() <= tolerance_mm * perimeter_mm
}

/// The material of a Left or Right Profile of `paths` (#207): the open
/// ones with length, offset by `left_mm` (positive to the left) to give
/// the tool-centre paths, and offset the other way to give their kept
/// edges, with the strip's two ends along the normals through the end
/// vertices ([`strip_ends`], #342). With a last pass `stock_to_leave_mm` (#311) each is offset by
/// `left_mm` plus the stock to leave, to the same side, too: the path every
/// pass but the last cuts. The kept edges stay one radius off.
fn side_profile(paths: &[Polyline], left_mm: f64, stock_to_leave_mm: Option<f64>) -> Material {
    let open: Vec<&Polyline> = paths
        .iter()
        .filter(|p| !p.is_closed() && p.length_mm() > EPS_MM)
        .collect();
    let stock_to_leave_left_mm = stock_to_leave_mm
        .filter(|a| a.is_finite() && *a > 0.0)
        .map(|a| (left_mm.abs() + a).copysign(left_mm));
    let offsets = |by_mm: f64| -> Vec<Edge> {
        open.iter()
            .flat_map(|p| offset_open(p, by_mm))
            .flat_map(|c| edges_of(&c).collect::<Vec<_>>())
            .collect()
    };
    let near = offsets(left_mm);
    let far = stock_to_leave_left_mm.map(offsets);
    let centres = near.iter().chain(far.iter().flatten()).copied().collect();
    // #342: the strip closed at each end by the normal through the end
    // vertex, cut short at the first line it meets, so a cutter that
    // reaches past an end into the strip fails however the offset runs.
    let kept = open.iter().flat_map(|p| {
        let mut edges = offset_open(p, -left_mm);
        edges.extend(strip_ends(p, -left_mm, &open));
        edges
    });
    let keep_clear = open
        .iter()
        .flat_map(|p| edges_of(p))
        .chain(kept.flat_map(|k| edges_of(&k).collect::<Vec<_>>()))
        .collect();
    let steps = far.zip(stock_to_leave_left_mm).map(|(far, far_left_mm)| {
        let stock_to_leave_mm = far_left_mm.abs() - left_mm.abs();
        // #358: each segment's own band, its two offsets whole.
        let strips = open
            .iter()
            .flat_map(|p| p.segments())
            .filter_map(|s| {
                let near = segment_offset(s, left_mm)?;
                let far = segment_offset(s, far_left_mm)?;
                Some((Edge::new(near), Edge::new(far)))
            })
            .collect();
        StockToLeaveBand {
            near,
            far,
            strips,
            stock_to_leave_mm,
            step_mm: dc_cam::last_pass_step_reach_mm(2.0 * left_mm.abs(), stock_to_leave_mm),
        }
    });
    Material::SideProfile {
        centres,
        keep_clear,
        steps,
    }
}

/// The band between a Left or Right Profile's last pass path and the
/// path its earlier passes cut, `stock_to_leave_mm` farther to the side
/// (#339): the last pass steps across it at its start, from the earlier
/// passes' kerf onto its own path.
#[derive(Debug, Clone)]
pub(crate) struct StockToLeaveBand {
    /// The last pass's paths' segments.
    near: Vec<Edge>,
    /// The earlier passes' paths' segments.
    far: Vec<Edge>,
    /// Each segment of each open path offset whole, by the radius and by
    /// the radius plus the stock to leave (#358): the band beside that one
    /// segment, before [`offset_open`] trims each offset where it comes
    /// nearer another part of the path.
    strips: Vec<(Edge, Edge)>,
    stock_to_leave_mm: f64,
    /// The longest step, mm.
    step_mm: f64,
}

impl StockToLeaveBand {
    /// True when `path`, a level cut, is a step across the band: from
    /// the earlier passes' path to the last pass's, no longer than
    /// [`StockToLeaveBand::step_mm`], its middle between the two
    /// ([`StockToLeaveBand::holds`]). A cut along the band, several short ones
    /// chained in it, or one that falls through it, is no move the
    /// generator makes: each would start or end inside the band, or fall.
    fn allows(&self, path: Segment, level: bool, slack_mm: f64) -> bool {
        let on = |edges: &[Edge], p: Point| on_line(edges, p, slack_mm);
        level
            && path.length_mm() <= self.step_mm + slack_mm
            && on(&self.far, path.start)
            && on(&self.near, path.end)
            && points_of(path).iter().all(|&p| self.holds(p, slack_mm))
    }

    /// True when `p` lies between the two paths, or between one
    /// segment's two whole offsets (#358): its distances to them add up
    /// to the stock to leave, within `slack_mm` each.
    ///
    /// The second covers where the two paths are trimmed short of each
    /// other: near an open path's end vertex that comes back beside
    /// another of its segments, each offset of that segment stops where
    /// it comes within its own distance of the vertex, `r` for the last
    /// pass and `r + a` for the others, so the two stop on two circles
    /// round it and not on one normal. Between a segment's whole offsets
    /// `p` lies on its side, abreast of it, `r` to `r + a` from it: the
    /// strip the passes beside it cut, not past its ends (beyond the
    /// slack) nor on the kept side. The cut's distance from every line, which S9 checks
    /// apart, keeps it from the vertex that trimmed them.
    fn holds(&self, p: Point, slack_mm: f64) -> bool {
        let reach_mm = self.stock_to_leave_mm + slack_mm;
        let at = Rect::from_point(p);
        let within = 2.0f64.mul_add(slack_mm, self.stock_to_leave_mm);
        let gap = |edges: &[Edge]| {
            edges
                .iter()
                .filter(|e| near(at, e.bounds, reach_mm))
                .map(|e| point_segment_distance_mm(e.segment, p))
                .fold(f64::INFINITY, f64::min)
        };
        gap(&self.near) + gap(&self.far) <= within
            || self.strips.iter().any(|(n, f)| {
                near(at, n.bounds, reach_mm)
                    && point_segment_distance_mm(n.segment, p)
                        + point_segment_distance_mm(f.segment, p)
                        <= within
            })
    }
}

#[cfg(test)]
mod stock_to_leave_band_tests {
    use super::*;
    use dc_geometry::Vertex;

    const RADIUS_MM: f64 = 1.0;
    const STOCK_TO_LEAVE_MM: f64 = 0.3;
    const SLACK_MM: f64 = 0.012;

    fn line_path(points: &[(f64, f64)]) -> Polyline {
        Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            false,
        )
    }

    /// The stock to leave band of a Left Profile of `path` with a last pass.
    fn band(path: &Polyline) -> StockToLeaveBand {
        match side_profile(
            std::slice::from_ref(path),
            RADIUS_MM,
            Some(STOCK_TO_LEAVE_MM),
        ) {
            Material::SideProfile {
                steps: Some(band), ..
            } => band,
            other => panic!("not a Left Profile with a last pass: {other:?}"),
        }
    }

    /// The first point of `path` offset `left_mm`, which must be one piece.
    fn offset_start(path: &Polyline, left_mm: f64) -> Point {
        let pieces = offset_open(path, left_mm);
        assert_eq!(pieces.len(), 1, "{pieces:?}");
        let v = pieces[0].vertices()[0];
        Point::new(v.x, v.y)
    }

    fn level(start: Point, end: Point, bulge: f64) -> Segment {
        Segment { start, end, bulge }
    }

    /// #358 case 1: an open path that nearly closes, cut Left (inside the
    /// loop), its end vertex E 1.5 mm above the line its first segment
    /// runs along. Both offsets start where they come `r` and `r + a`
    /// from E, so the two passes' starts are not on one normal: the last
    /// pass's step from the stock path's start onto its own ends
    /// 0.52 mm from the stock path, farther than the stock to leave. It
    /// still runs between the first segment's own two offsets, at least
    /// `r` from every line, and is a step S9 accepts.
    #[test]
    fn last_pass_step_between_starts_trimmed_by_the_end_vertex_is_allowed() {
        let path = line_path(&[
            (0.0, 0.0),
            (10.0, 0.0),
            (10.0, 5.0),
            (-0.5, 5.0),
            (-0.5, 1.5),
        ]);
        let far = offset_start(&path, RADIUS_MM + STOCK_TO_LEAVE_MM);
        let near = offset_start(&path, RADIUS_MM);
        // Each pass starts on its circle round E, not on the first
        // segment's normal through (0, 0).
        let e = Point::new(-0.5, 1.5);
        assert!((far.distance_to(e) - (RADIUS_MM + STOCK_TO_LEAVE_MM)).abs() < 1e-6);
        assert!((near.distance_to(e) - RADIUS_MM).abs() < 1e-6);
        assert!((far.x - near.x).abs() > 0.4, "{far:?} {near:?}");
        let band = band(&path);
        let step = level(far, near, 0.0);
        assert!(step.length_mm() <= band.step_mm, "{}", step.length_mm());
        assert!(band.allows(step, true, SLACK_MM));
        // A step that is not level is no step.
        assert!(!band.allows(step, false, SLACK_MM));
    }

    /// #358: the band at an open end stays the strip's. A level arc from
    /// the stock path's end to the last pass's path that bows past
    /// the path's end vertex, `r` to `r + a` from it, cuts past the open
    /// end and still fails, and no point on the kept side, `r` to `r + a`
    /// from the path, lies in the band.
    #[test]
    fn last_pass_step_past_an_open_end_or_on_the_kept_side_fails() {
        let path = line_path(&[(0.0, 0.0), (10.0, 0.0)]);
        let band = band(&path);
        let (far_end, near) = (Point::new(10.0, 1.3), Point::new(9.9, 1.0));
        assert!(band.allows(level(far_end, near, 0.0), true, SLACK_MM));
        let bowed = level(far_end, near, -1.5);
        let (middle, _) = bowed.point_at(bowed.length_mm() / 2.0).expect("an arc");
        assert!(middle.x > 10.1, "{middle:?}");
        let from_end_mm = middle.distance_to(Point::new(10.0, 0.0));
        assert!((RADIUS_MM..=RADIUS_MM + STOCK_TO_LEAVE_MM).contains(&from_end_mm));
        assert!(bowed.length_mm() <= band.step_mm, "{}", bowed.length_mm());
        assert!(!band.allows(bowed, true, SLACK_MM));
        assert!(!band.holds(Point::new(10.4, 1.15), SLACK_MM));
        assert!(!band.holds(Point::new(5.0, -1.15), SLACK_MM));
        assert!(band.holds(Point::new(5.0, 1.15), SLACK_MM));
    }
}

/// Segment `s` offset whole by `left_mm` (positive to the left of travel),
/// untrimmed: a line moved along its normal, an arc the same sweep about
/// the same centre. `None` for a segment of no length, or an arc that
/// would shrink to its centre or past it.
fn segment_offset(s: Segment, left_mm: f64) -> Option<Segment> {
    let (start, start_dir) = s.point_at(0.0)?;
    let (end, end_dir) = s.point_at(s.length_mm())?;
    if let Some(arc) = s.arc() {
        // The left of a counter-clockwise arc is towards its centre.
        let inward_mm = left_mm * s.bulge.signum();
        if arc.radius_mm - inward_mm <= EPS_MM {
            return None;
        }
    }
    Some(Segment {
        start: start + start_dir.perp() * left_mm,
        end: end + end_dir.perp() * left_mm,
        bulge: s.bulge,
    })
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
    /// The tab's outline: its instance and its index in the definition's
    /// profiles. Tabs of one outline share its laps, where their spans
    /// merge.
    outline_id: (InstanceId, usize),
    /// The bit's paths round the tab's outline a lap runs on: the outline
    /// and the outline offset by the tool radius both ways, project space.
    paths: Vec<Polyline>,
    /// The laps among [`TabSpine::paths`]: the outline offset by the tool
    /// radius both ways, project space. Empty for an On line Profile,
    /// whose lap is the outline.
    laps: Vec<Polyline>,
    /// The paths of the passes before a last pass (#284): the outline
    /// offset by the radius plus the last pass's stock to leave both ways,
    /// project space. Empty without a last pass.
    stock_paths: Vec<Polyline>,
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
    /// The tool radius, mm.
    radius_mm: f64,
    /// The tab's top above the stock bottom, mm.
    top_z_mm: f64,
}

/// Fraction of the tool radius a plunge may come inside a tab's span (the
/// tab's width plus the tool diameter, along the bit's path) without being
/// refused (#337): a rect tab steps down at the span's end, and the
/// offsets S9 builds round the outline may differ that much from the
/// generator's along the path.
const TAB_SPAN_MARGIN: f64 = 0.1;

/// How many cells S9 looks at for tab material under one stock to leave
/// pass's plunge (#353) before it gives up and refuses the plunge: the
/// search halves cells only where the cutter comes within a cell of the
/// tab, so a plunge clear of it by more than the slack needs a few
/// hundred.
const TAB_SEARCH_CELLS: usize = 200_000;

/// The tabs Profile `op` must leave standing in `project` (S9, #337): its
/// placed tabs on closed outlines of its targets, and those its tab
/// request places (`dc_cam::placed_tabs::place`, the documented layout
/// rule, not the toolpath). Empty for another kind of operation, or when
/// the tool, an instance, a definition or a profile is missing.
pub(crate) fn run_tab_spines(project: &Project, op: &Operation, label: &RunLabel) -> Vec<TabSpine> {
    match label {
        RunLabel::Main => tab_spines(project, op),
        RunLabel::Clearing(_) | RunLabel::CutOut(_) => Vec::new(),
    }
}

/// The tabs Profile `op` must leave standing.
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
    let stock_to_leave_mm = profile.last_pass.as_ref().map(|l| l.stock_to_leave_mm);
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
            let offsets = |delta_mm: f64| -> Vec<Polyline> {
                [1.0, -1.0]
                    .into_iter()
                    .flat_map(|sign| offset(&contour, sign * delta_mm))
                    .map(Contour::into_polyline)
                    .collect()
            };
            let laps = offsets(radius_mm);
            let stock_paths = stock_to_leave_mm
                .map(|a| offsets(radius_mm + a))
                .unwrap_or_default();
            let mut paths = vec![outline];
            paths.extend(laps.iter().cloned());
            let (hold_mm, slope_mm) = match tab.shape {
                TabShape::Rect => (radius_mm + tab.width_mm / 2.0, 0.0),
                TabShape::Smooth => (radius_mm, tab.width_mm / 2.0),
            };
            Some(TabSpine {
                outline_id: (tab.instance.clone(), tab.profile),
                paths,
                laps: if profile.side == Side::OnLine {
                    Vec::new()
                } else {
                    laps
                },
                stock_paths,
                point: instance.transform.apply(Point::new(tab.x_mm, tab.y_mm)),
                hold_mm,
                slope_mm,
                margin_mm: radius_mm * TAB_SPAN_MARGIN,
                shape: tab.shape,
                floor_z_mm,
                reach_mm: radius_mm + tab.width_mm / 2.0,
                radius_mm,
                top_z_mm: tab.height_mm,
            })
        })
        .collect()
}

impl TabSpine {
    /// Whether a cut down to `z_mm` at `p`, within `slack_mm` of one of
    /// the bit's paths round the tab's outline, goes below the tab there.
    /// On a lap's path (or the outline) that is a cut within the tab's
    /// span ([`TabSpine::lowest_z_mm`]). On the path of the passes before
    /// a last pass (#353) it is a cut into the tab material the laps leave
    /// standing ([`TabSpine::cuts_material`]), the tabs of the outline
    /// `tabs` merged; on an On line Profile, which has no such laps, the
    /// span again.
    fn cuts_into(&self, p: Point, z_mm: f64, slack_mm: f64, tabs: &[TabSpine]) -> bool {
        let by_span = |path: &Polyline| {
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
        };
        let on_stock_path = || {
            self.stock_paths
                .iter()
                .any(|path| nearest_point(path, p).is_some_and(|at| at.gap_mm <= slack_mm))
        };
        if self.paths.iter().any(by_span) {
            return true;
        }
        if self.laps.is_empty() {
            self.stock_paths.iter().any(by_span)
        } else {
            on_stock_path() && self.cuts_material(p, z_mm, slack_mm, tabs)
        }
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

    /// How far along a lap either side of the tab's centre the lap runs
    /// above `z_mm`, mm, by the span rule (ADR-0005) without
    /// [`TAB_SPAN_MARGIN`]: a rect tab's whole half span below its top
    /// (less `slack_mm`); a smooth tab's hold and the part of its slope
    /// above `z_mm`. 0 when the tab's top is not above it. The margin
    /// lets a lap come down early; the tab it leaves is still the span's
    /// as the generator cuts it, and that is what the passes before a
    /// last pass must not cut.
    fn held_mm(&self, z_mm: f64, slack_mm: f64) -> f64 {
        let above_mm = self.top_z_mm - slack_mm - z_mm;
        if above_mm <= 0.0 {
            return 0.0;
        }
        let rise_mm = self.top_z_mm - self.floor_z_mm;
        let f = if rise_mm > EPS_MM {
            (above_mm / rise_mm).min(1.0)
        } else {
            1.0
        };
        (self.hold_mm + self.slope_mm * f).max(0.0)
    }

    /// Whether the cutter plunged to `z_mm` at `p` cuts the tab material
    /// the laps round this tab's outline leave standing above `z_mm`
    /// (#353), the outline's `tabs` merged.
    ///
    /// The laps run at their floor except where the tab rule holds them
    /// above `z_mm` ([`TabSpine::held_mm`]): every lap point is "high" or
    /// "low". The material that stands above `z_mm` once the laps are cut
    /// is every point within the radius of a high lap point and at least
    /// the radius from every low one: the tab. The laps' segments are
    /// split exactly where they turn high.
    ///
    /// The search halves square cells over the cutter, the radius round
    /// `p`. A cell is dropped when no point of it can be inside the cutter
    /// and `slack_mm` inside the tab (its centre's distances, give or
    /// take its half diagonal, rule it out), and the plunge is refused
    /// when a cell's centre lies in both, or the centre of a cell whose
    /// half diagonal is at most the slack lies in the tab and within that
    /// half diagonal of the cutter. So the rule refuses every cutter that
    /// reaches the slack into the tab, and passes every cutter that, grown
    /// by the slack, misses it. A search that needs more than
    /// [`TAB_SEARCH_CELLS`] cells refuses the plunge.
    fn cuts_material(&self, p: Point, z_mm: f64, slack_mm: f64, tabs: &[TabSpine]) -> bool {
        let r = self.radius_mm;
        let reach_mm = 2.0 * r + slack_mm;
        let at = Rect::from_point(p);
        let mut high: Vec<Segment> = Vec::new();
        let mut low: Vec<Segment> = Vec::new();
        for lap in &self.laps {
            let length_mm = lap.length_mm();
            let held: Vec<(f64, f64)> = tabs
                .iter()
                .filter(|t| t.outline_id == self.outline_id)
                .filter_map(|t| {
                    let centre = nearest_point(lap, t.point)?;
                    let half_mm = t.held_mm(z_mm, slack_mm);
                    (centre.gap_mm <= t.reach_mm + slack_mm && half_mm > 0.0)
                        .then_some((centre.along_mm - half_mm, centre.along_mm + half_mm))
                })
                .collect();
            split_lap(lap, length_mm, &held, |piece, is_high| {
                if near(at, bounds_of(piece), reach_mm) {
                    if is_high {
                        high.push(piece);
                    } else {
                        low.push(piece);
                    }
                }
            });
        }
        if high.is_empty() {
            return false;
        }
        let gap = |segments: &[Segment], q: Point| {
            segments
                .iter()
                .map(|s| point_segment_distance_mm(*s, q))
                .fold(f64::INFINITY, f64::min)
        };
        let tolerance_mm = slack_mm.max(EPS_MM);
        // Cells as (centre, half side).
        let mut cells: Vec<(Point, f64)> = vec![(p, r)];
        let mut looked = 0;
        while let Some((q, half_mm)) = cells.pop() {
            looked += 1;
            if looked > TAB_SEARCH_CELLS {
                return true;
            }
            let blur_mm = half_mm * std::f64::consts::SQRT_2;
            let (cutter, high_gap, low_gap) = (q.distance_to(p), gap(&high, q), gap(&low, q));
            if cutter - blur_mm >= r
                || high_gap - blur_mm >= r - tolerance_mm
                || low_gap + blur_mm < r + tolerance_mm
            {
                continue;
            }
            let in_tab = high_gap < r && low_gap >= r;
            if in_tab && cutter < r {
                return true;
            }
            if blur_mm <= tolerance_mm {
                if in_tab && cutter < r + blur_mm {
                    return true;
                }
                continue;
            }
            let quarter = half_mm / 2.0;
            for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                cells.push((q + Vec2::new(dx * quarter, dy * quarter), quarter));
            }
        }
        false
    }
}

/// Splits the closed or open lap `lap`, `length_mm` long, into pieces at
/// the ends of the stretches `held` (from and to, mm along it from its
/// start, either possibly past an end of a closed lap, where it wraps)
/// and hands each piece to `f` with whether it lies in one of them.
/// Segments no longer than [`EPS_MM`] are skipped, as
/// [`nearest_point`] measures along a path.
fn split_lap(
    lap: &Polyline,
    length_mm: f64,
    held: &[(f64, f64)],
    mut f: impl FnMut(Segment, bool),
) {
    let closed = lap.is_closed() && length_mm > EPS_MM;
    // The stretches within [0, length], unwrapped.
    let mut stretches: Vec<(f64, f64)> = Vec::new();
    for &(from, to) in held {
        if closed && to - from >= length_mm {
            stretches.push((0.0, length_mm));
            continue;
        }
        for shift in [-length_mm, 0.0, length_mm] {
            if !closed && shift != 0.0 {
                continue;
            }
            let (a, b) = ((from + shift).max(0.0), (to + shift).min(length_mm));
            if b > a {
                stretches.push((a, b));
            }
        }
    }
    let in_held = |d: f64| stretches.iter().any(|&(a, b)| a <= d && d <= b);
    let mut walked_mm = 0.0;
    for segment in lap.segments() {
        let seg_mm = segment.length_mm();
        if seg_mm <= EPS_MM {
            continue;
        }
        let mut cuts: Vec<f64> = vec![0.0, seg_mm];
        for &(a, b) in &stretches {
            for d in [a - walked_mm, b - walked_mm] {
                if d > 0.0 && d < seg_mm {
                    cuts.push(d);
                }
            }
        }
        cuts.sort_by(f64::total_cmp);
        for (&a, &b) in cuts.iter().zip(cuts.iter().skip(1)) {
            if b - a <= EPS_MM {
                continue;
            }
            if let Some(piece) = sub_segment(segment, seg_mm, a, b) {
                f(piece, in_held(walked_mm + (a + b) / 2.0));
            }
        }
        walked_mm += seg_mm;
    }
}

/// The part of `segment`, `length_mm` long, from `from_mm` to `to_mm`
/// along it, an arc's part an arc of the same circle.
fn sub_segment(segment: Segment, length_mm: f64, from_mm: f64, to_mm: f64) -> Option<Segment> {
    let (start, _) = segment.point_at(from_mm)?;
    let (end, _) = segment.point_at(to_mm)?;
    let bulge = segment.arc().map_or(0.0, |arc| {
        (arc.sweep_rad * (to_mm - from_mm) / length_mm / 4.0).tan()
    });
    Some(Segment { start, end, bulge })
}

/// S9's tab rule (#337) for one move `mv` of `span`: `false` when it is a
/// straight plunge that ends below a tab's top within the tab's span
/// along the bit's path, or, on the path of the passes before a last
/// pass, into the tab material the laps leave (#353) ([`TabSpine`]).
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
        || span.tabs.iter().all(|tab| {
            to.z >= tab.top_z_mm - slack_mm || !tab.cuts_into(to.xy(), to.z, slack_mm, span.tabs)
        })
}

/// What S9's trench rule (#352) needs from the project for an inside or
/// outside Profile.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TrenchRule {
    /// The stock top, above its bottom, mm.
    stock_top_mm: f64,
    /// A last pass's stock to leave, mm; 0 without one.
    stock_to_leave_mm: f64,
}

/// The trench rule's data for Profile `profile` of `project`; `None`
/// when the stock thickness is not set.
fn trench_rule(project: &Project, profile: &ProfileOp) -> Option<TrenchRule> {
    project.stock.thickness_mm.map(|stock_top_mm| TrenchRule {
        stock_top_mm,
        stock_to_leave_mm: profile
            .last_pass
            .as_ref()
            .map_or(0.0, |l| l.stock_to_leave_mm.max(0.0)),
    })
}

/// The side of the square cells [`Trench`] indexes the cuts by, mm.
const TRENCH_CELL_MM: f64 = 2.0;

/// How far apart [`Trench`] checks the points of a ramping move, as a
/// fraction of the tool radius.
const TRENCH_STEP_FRACTION: f64 = 0.1;

/// The part of `path`, a cut falling or rising linearly from `from_z` to
/// `to_z`, that runs below `top_mm`: the stock it cut. `None` when it runs
/// wholly at or above it. (#356: the whole path was taken when any of it
/// ran below the stock top less the slack, so a ramp leg that dipped below
/// only at its end counted as trench along its whole length, or not at
/// all, as the arc-fit tolerance in the slack fell.)
fn below_top(path: Segment, from_z: f64, to_z: f64, top_mm: f64) -> Option<Segment> {
    match (from_z < top_mm, to_z < top_mm) {
        (true, true) => Some(path),
        (false, false) => None,
        _ if path.length_mm() <= EPS_MM => Some(path),
        (from_below, _) => {
            let t = (top_mm - from_z) / (to_z - from_z);
            if from_below {
                path.piece(0.0, t)
            } else {
                path.piece(t, 1.0)
            }
        }
    }
}

/// S9's trench rule (#352) over one operation's moves, in order (module
/// docs, "Trenches").
struct Trench<'a> {
    rule: TrenchRule,
    areas: &'a [Area],
    radius_mm: f64,
    /// The tool-centre path in XY, project space, of every cut so far,
    /// where it ran below the stock top (#356).
    cuts: Vec<Segment>,
    /// For each cell, the cuts whose bounds, grown by the reach a point
    /// is measured to, meet it.
    cells: BTreeMap<(i64, i64), Vec<usize>>,
    /// While a run of ramping moves lasts: whether it started at or
    /// above the stock top.
    fresh: Option<bool>,
}

impl<'a> Trench<'a> {
    fn new(rule: TrenchRule, areas: &'a [Area], radius_mm: f64) -> Self {
        Self {
            rule,
            areas,
            radius_mm,
            cuts: Vec::new(),
            cells: BTreeMap::new(),
            fresh: None,
        }
    }

    /// A rapid: a run of ramping moves ends.
    fn lift(&mut self) {
        self.fresh = None;
    }

    /// The cell holding `(x, y)`, mm.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "cell indices of project coordinates, clamped far inside i64"
    )]
    fn cell(x: f64, y: f64) -> (i64, i64) {
        let index = |v: f64| (v / TRENCH_CELL_MM).floor().clamp(-1e12, 1e12) as i64;
        (index(x), index(y))
    }

    /// Whether cut `mv` keeps to the trench rule, `slack_mm` allowed, and
    /// records it. It does not when it ramps (falls while it runs more
    /// than the slack in XY) in a run of ramping moves that started below
    /// the stock top, through a point below the stock top that lies
    /// neither on the ring the laps cut (within the tool radius, plus a
    /// last pass's stock to leave and the slack, of a target outline) nor
    /// within twice the slack of the tool-centre path an earlier cut ran
    /// below the stock top. The points are checked a tenth of the radius
    /// apart. Only the part of `mv` below the stock top is recorded
    /// ([`below_top`]).
    fn allows(&mut self, mv: &MachineMove, transform: &MachineTransform, slack_mm: f64) -> bool {
        let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz), Some(path)) = (
            mv.from.x,
            mv.from.y,
            mv.from.z,
            mv.to.x,
            mv.to.y,
            mv.to.z,
            project_path(mv, transform),
        ) else {
            self.fresh = None;
            return true;
        };
        let from_z = transform.to_project(Point3::new(fx, fy, fz), mv.units).z;
        let to_z = transform.to_project(Point3::new(tx, ty, tz), mv.units).z;
        let top_mm = self.rule.stock_top_mm - slack_mm;
        let length_mm = path.length_mm();
        let step_mm = self.radius_mm * TRENCH_STEP_FRACTION;
        let ramping = to_z < from_z - EPS_MM && length_mm > slack_mm;
        // A move too short to fall a unit in the last place is written
        // level: it neither ramps nor ends the run.
        let sliver = (to_z - from_z).abs() <= EPS_MM && length_mm <= step_mm;
        let fresh = if ramping {
            *self.fresh.get_or_insert(from_z >= top_mm)
        } else {
            if !sliver {
                self.fresh = None;
            }
            true
        };
        let mut allowed = true;
        if !fresh && to_z < top_mm {
            let ring_mm = self.radius_mm + self.rule.stock_to_leave_mm + slack_mm;
            let near_mm = 2.0 * slack_mm;
            let mut s_mm: f64 = 0.0;
            loop {
                let s = s_mm.min(length_mm);
                let z = from_z + (to_z - from_z) * (s / length_mm);
                let p = path.point_at(s).map_or(path.end, |(p, _)| p);
                if z < top_mm && !self.on_ring(p, ring_mm) && !self.on_earlier(p, near_mm) {
                    allowed = false;
                    break;
                }
                if s >= length_mm || step_mm <= 0.0 {
                    break;
                }
                s_mm += step_mm;
            }
        }
        if let Some(cut) = below_top(path, from_z, to_z, self.rule.stock_top_mm) {
            self.record(cut, 2.0 * slack_mm);
        }
        allowed
    }

    /// Whether `p` lies within `reach_mm` of a target outline.
    fn on_ring(&self, p: Point, reach_mm: f64) -> bool {
        let at = Rect::from_point(p);
        self.areas
            .iter()
            .filter(|a| near(at, a.bounds, reach_mm))
            .any(|a| {
                a.edges.iter().any(|e| {
                    near(at, e.bounds, reach_mm)
                        && point_segment_distance_mm(e.segment, p) <= reach_mm
                })
            })
    }

    /// Whether `p` lies within `reach_mm` of an earlier cut's path.
    fn on_earlier(&self, p: Point, reach_mm: f64) -> bool {
        self.cells.get(&Self::cell(p.x, p.y)).is_some_and(|cuts| {
            cuts.iter().any(|&i| {
                self.cuts
                    .get(i)
                    .is_some_and(|c| point_segment_distance_mm(*c, p) <= reach_mm)
            })
        })
    }

    /// Records the cut along `path`, indexed for the points within
    /// `reach_mm` of it.
    fn record(&mut self, path: Segment, reach_mm: f64) {
        let index = self.cuts.len();
        self.cuts.push(path);
        let b = bounds_of(path);
        let (x0, y0) = Self::cell(b.min.x - reach_mm, b.min.y - reach_mm);
        let (x1, y1) = Self::cell(b.max.x + reach_mm, b.max.y + reach_mm);
        for x in x0..=x1 {
            for y in y0..=y1 {
                self.cells.entry((x, y)).or_default().push(index);
            }
        }
    }
}

/// How far a move may depart from where the toolpath put it, rounding
/// aside (module docs): the flatten tolerance, and the arc-fit tolerance
/// when arc fitting is on, mm.
fn departure_mm(tolerances: Tolerances) -> f64 {
    tolerances.flatten_mm + tolerances.arc_fit_mm.unwrap_or(0.0) + EPS_MM
}

/// How far a move written in `units` may depart from where the toolpath
/// put it (module docs): one unit in the last written place and
/// [`departure_mm`], mm.
fn slack_mm(post: &PostProcessor, units: Units, tolerances: Tolerances) -> f64 {
    quantum_mm(post, units) + departure_mm(tolerances)
}

/// S9, S17 and S18 for `span`: every cutting move in its lines stays in
/// its material.
///
/// # Errors
///
/// At the first move that leaves it: [`ExportError::ConeOffMaterial`] or
/// [`ExportError::ConeHeightExceeded`] for a V-bit's run (S17),
/// [`ExportError::CountersinkDepth`] or [`ExportError::ConeHeightExceeded`]
/// for a countersink (S18), else
/// [`ExportError::OffMaterial`], which is also the error for a move whose
/// position is not known or whose material is unresolved.
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
    // S18 holds every line of a countersink's span, not only its cuts.
    if let Material::Countersink(sink) = span.material {
        return sink.check(blocks, post, transform, span);
    }
    let radius_mm = span.tool.diameter_mm / 2.0;
    // #221: each On line loop and held open path, measured against itself.
    let departure_mm = departure_mm(tolerances);
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
    let mut trench = match span.material {
        Material::Inside {
            areas,
            trench: Some(rule),
            ..
        }
        | Material::Outside {
            areas,
            trench: Some(rule),
            ..
        } => Some(Trench::new(*rule, areas, radius_mm)),
        _ => None,
    };
    let moves = blocks
        .iter()
        .filter(|b| span.lines.contains(&b.line))
        .filter_map(|b| b.motion.as_ref());
    for mv in moves {
        if !mv.kind.is_cut() {
            last = None;
            if let Some(trench) = trench.as_mut() {
                trench.lift();
            }
            continue;
        }
        let slack_mm = slack_mm(post, mv.units, tolerances);
        let in_trench = trench
            .as_mut()
            .is_none_or(|t| t.allows(mv, transform, slack_mm));
        if !in_trench || !keeps_tabs(mv, transform, span, slack_mm) {
            return Err(ExportError::OffMaterial {
                op: span.op.id().clone(),
                line: mv.line,
            });
        }
        let path = project_path(mv, transform);
        let z_mm = mv.from.z.zip(mv.to.z).map(|(from_z, to_z)| {
            (
                transform.project_z(from_z, mv.units),
                transform.project_z(to_z, mv.units),
            )
        });
        if let (Material::VBit(cone), Some(path), Some(z_mm)) = (span.material, path, z_mm) {
            match cone.check(path, z_mm, slack_mm) {
                Ok(()) => continue,
                Err(Fault::Off { at, overlap_mm }) => {
                    return Err(ExportError::ConeOffMaterial {
                        op: span.op.id().clone(),
                        line: mv.line,
                        x_mm: at.x,
                        y_mm: at.y,
                        z_mm: at.z,
                        overlap_mm,
                    });
                }
                Err(Fault::Shank { engaged_mm }) => {
                    return Err(ExportError::ConeHeightExceeded {
                        op: span.op.id().clone(),
                        line: mv.line,
                        engaged_mm,
                        cone_mm: cone.cone_mm,
                    });
                }
            }
        }
        let inside = path.is_some_and(|path| match span.material {
            #[cfg(test)]
            Material::Unchecked => true,
            Material::Unresolved => false,
            Material::Inside {
                areas,
                stock_to_leave_mm,
                relief,
                laps,
                ..
            } => {
                let clearance_mm = radius_mm + stock_to_leave_mm - slack_mm;
                let notch = relief.then_some(Notch {
                    radius_mm,
                    stock_to_leave_mm: *stock_to_leave_mm,
                    slack_mm,
                });
                in_regions(areas, path, clearance_mm, notch, &mut last)
                    || (*laps && is_lap(areas, path, clearance_mm, radius_mm + slack_mm, notch))
            }
            Material::Outside { areas, relief, .. } => {
                let clearance_mm = radius_mm - slack_mm;
                let notch = relief.then_some(Notch {
                    radius_mm,
                    stock_to_leave_mm: 0.0,
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
            Material::Bore {
                holes,
                stock_to_leave_mm,
            } => holes.iter().any(|(centre, r)| {
                farthest_mm(path, *centre) <= r - radius_mm - stock_to_leave_mm + slack_mm
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
                steps,
            } => {
                let clearance_mm = radius_mm - slack_mm;
                let bounds = bounds_of(path);
                (points_of(path)
                    .iter()
                    .all(|&p| on_line(centres, p, slack_mm))
                    || steps.as_ref().is_some_and(|s| {
                        let level = mv
                            .from
                            .z
                            .zip(mv.to.z)
                            .is_some_and(|(a, b)| (a - b).abs() <= EPS_MM);
                        s.allows(path, level, slack_mm)
                    }))
                    && keep_clear
                        .iter()
                        .filter(|e| near(bounds, e.bounds, clearance_mm))
                        .all(|e| segment_distance_mm(path, e.segment) >= clearance_mm)
            }
            Material::OnLine { edges } => {
                points_of(path).iter().all(|&p| on_line(edges, p, slack_mm))
            }
            // Its Z or XY is not known: S17 cannot hold it.
            Material::VBit(_) => false,
            // Checked above, line by line.
            Material::Countersink(_) => false,
            Material::Floor {
                areas,
                floor_z_mm,
                radius_mm,
            } => z_mm.is_some_and(|(from_z, to_z)| {
                from_z.min(to_z) >= floor_z_mm - slack_mm
                    && in_regions(areas, path, radius_mm - slack_mm, None, &mut last)
            }),
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
/// The farthest any point of `path` comes from `centre`, mm: an end of a
/// line, or of an arc, or the arc's point straight away from `centre`
/// when its sweep passes there. A Bore's helix, circle and move to the
/// centre must all stay within its hole's helix radius (format 18), so an
/// arc that bulges out between its ends and middle is caught too.
fn farthest_mm(path: Segment, centre: Point) -> f64 {
    let ends = path
        .start
        .distance_to(centre)
        .max(path.end.distance_to(centre));
    let Some(arc) = path.arc() else {
        return ends;
    };
    let away = arc.centre - centre;
    let offset_mm = away.length();
    if offset_mm <= EPS_MM {
        return ends.max(arc.radius_mm);
    }
    let angle = away.y.atan2(away.x);
    let along = (angle - arc.start_angle_rad) * arc.sweep_rad.signum();
    if along.rem_euclid(std::f64::consts::TAU) <= arc.sweep_rad.abs() {
        ends.max(offset_mm + arc.radius_mm)
    } else {
        ends
    }
}

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

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::*;
    use crate::parse::{ArcWords, MachinePosition};
    use dc_project::test_support::sample_project;

    /// Format 18: the farthest a cut comes from a Bore hole's centre is an
    /// end of a line, or the arc's point straight away from the centre when
    /// the arc passes it, so an arc that bulges between its ends and middle
    /// is measured there.
    #[test]
    fn farthest_point_of_an_arc_is_where_it_bulges() {
        let centre = Point::new(0.0, 0.0);
        let line = Segment {
            start: Point::new(1.0, 0.0),
            end: Point::new(0.0, 2.0),
            bulge: 0.0,
        };
        assert!((farthest_mm(line, centre) - 2.0).abs() < 1e-12);
        // A counter-clockwise half circle about (1, 0), radius 1, from
        // (1, −1) to (1, 1): it passes (2, 0), 2 mm out.
        let half = Segment {
            start: Point::new(1.0, -1.0),
            end: Point::new(1.0, 1.0),
            bulge: 1.0,
        };
        assert!((farthest_mm(half, centre) - 2.0).abs() < 1e-9);
        // The other way round it passes (0, 0) and its ends are farthest.
        let back = Segment {
            start: Point::new(1.0, 1.0),
            end: Point::new(1.0, -1.0),
            bulge: 1.0,
        };
        assert!((farthest_mm(back, centre) - 2.0_f64.sqrt()).abs() < 1e-9);
        // About the centre itself an arc is its radius out everywhere.
        let round = Segment {
            start: Point::new(1.5, 0.0),
            end: Point::new(0.0, 1.5),
            bulge: (std::f64::consts::FRAC_PI_2 / 4.0).tan(),
        };
        assert!((farthest_mm(round, centre) - 1.5).abs() < 1e-9);
    }

    const TOP_MM: f64 = 20.0;
    const RADIUS_MM: f64 = 3.6;

    /// The cut from `from` to `to`, project space, written in mm on line
    /// `line`.
    fn cut(transform: &MachineTransform, line: usize, from: Point3, to: Point3) -> MachineMove {
        let at = |p: Point3| {
            let m = transform.to_machine(p);
            MachinePosition {
                x: Some(m.x),
                y: Some(m.y),
                z: Some(m.z),
            }
        };
        MachineMove {
            line,
            kind: MotionKind::Linear,
            from: at(from),
            to: at(to),
            feed: Some(1000.0),
            units: Units::Mm,
            arc: ArcWords::default(),
        }
    }

    /// #356: a first pass's ramp leg from 0.5 mm above the stock top to
    /// 0.015 mm below it, 20 mm along X, then a later pass's ramp leg back
    /// from its end, from Z `TOP_MM` − 0.1 to X `back_to_x`: whether the
    /// trench rule allows that later leg, with `arc_fit_mm`. Only the last
    /// 0.58 mm of the first leg (X 19.42 to 20) is below the stock top.
    fn later_leg_allowed(back_to_x: f64, arc_fit_mm: Option<f64>) -> bool {
        let project = sample_project();
        let post = project.post_processor.resolve().unwrap();
        let transform = MachineTransform::new(&project, &post).unwrap();
        let tolerances = Tolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm,
        };
        let slack = slack_mm(&post, Units::Mm, tolerances);
        let rule = TrenchRule {
            stock_top_mm: TOP_MM,
            stock_to_leave_mm: 0.0,
        };
        let mut trench = Trench::new(rule, &[], RADIUS_MM);
        let first = cut(
            &transform,
            1,
            Point3::new(0.0, 0.0, TOP_MM + 0.5),
            Point3::new(20.0, 0.0, TOP_MM - 0.015),
        );
        assert!(trench.allows(&first, &transform, slack), "a fresh ramp");
        trench.lift();
        let later = cut(
            &transform,
            2,
            Point3::new(20.0, 0.0, TOP_MM - 0.1),
            Point3::new(back_to_x, 0.0, TOP_MM - 0.2),
        );
        trench.allows(&later, &transform, slack)
    }

    /// #356: a ramp leg above the stock top but for its last 0.015 mm is
    /// trench only along that last part. A later ramp leg within that part
    /// is allowed; one that runs past it, along the part above the top,
    /// fails S9, with arc fitting off and on alike.
    #[test]
    fn only_the_part_of_a_cut_below_the_top_is_trench() {
        for arc_fit_mm in [None, Some(0.01)] {
            assert!(later_leg_allowed(19.6, arc_fit_mm), "{arc_fit_mm:?}");
            assert!(!later_leg_allowed(10.0, arc_fit_mm), "{arc_fit_mm:?}");
            assert!(!later_leg_allowed(19.2, arc_fit_mm), "{arc_fit_mm:?}");
        }
    }
}
