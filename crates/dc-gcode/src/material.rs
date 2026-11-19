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
//!   An arc is held to the same material point by point: every piece of
//!   it keeps the clearance above or lies in one corner's zone, so it may
//!   run from clear stock into a notch and nowhere a straight cut may
//!   not. Arc fitting writes such an arc for an entry's handoff to a
//!   corner and the notch that starts there when they lie within the
//!   tolerance of one circle, as they do after a helix a few hundredths
//!   of a millimetre across (2026-11-18, the Profile property's seed
//!   `cc 67e54832…`: the rule took straight cuts only and refused it).
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
//!   toolpath); a male Inlay's cut-out's are those its request places on
//!   the path this pass derives, the open pieces of one outline taking
//!   the request between them (`dc_cam::placed_tabs::place_shared`,
//!   #710). So a lap that starts on a tab and plunges there, below
//!   its top, is refused, wherever on the tab it starts. A tab on an
//!   open outline an On line Profile cuts (#175) is held the same way
//!   along that outline and its open offsets, its span never wrapping
//!   from one end to the other: each open piece of a clipped cut-out
//!   holds its own tabs. A tab on an open path of a Left or Right Profile
//!   (#207) is held along that path's offset to the cut side alone, where
//!   its request centres it and the bit runs ([`tab_spine`]).
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
//! - **Photo V-Carve, S19** (format 27, #646; [`photo`] has the rule):
//!   every line of its span. Every cut a `G1`; one that ends at or below
//!   the start plane inside the picture's footprint, no deeper than its
//!   maximum depth, its flanks engaged no deeper than the cone (S17's tip,
//!   depth and shank rules with the footprint as the region; the picture's
//!   edge is no wall, so no disc test); one that moves across above the
//!   start plane at the link height, fed; no rapid across below safe Z.
//!   The groove's width is S7's, which widens each cut by its width at
//!   the stock top.
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
//!   180°) or a tapered ball nose with every value set and usable
//!   ([`Tool::carve_profile`]); the rule is the same for the medial and
//!   the offset strategy, floors and links included. Anything else is
//!   unresolved.
//!
//!   **A tapered ball nose** (3.6, #680) is held to the same rule on its
//!   profile: the disc at the start plane is `w(d)` across, ball, cone or
//!   cylinder, measured exactly by the shared
//!   [`dc_geometry::profile::least_room`] ([`cone`]); the shank's limit is
//!   its cutting height `min(flute_length_mm, h_R)`
//!   ([`Tool::cutting_height_mm`]), named in `ConeHeightExceeded`; its
//!   floor inset is `w(F)`. S7 widens its cuts by its cut radius
//!   `min(w(flute_length_mm), R)` ([`Tool::cut_radius_mm`]). An Inlay
//!   side cut with one (slice 27e, #681) is held the same way to its
//!   design rounded for the bit ([`dc_project::rounded_for_bit`]), on
//!   either board, as the generator rounds it; a design the rounding
//!   refuses is unresolved.
//! - **V-Carve, a clearing run** (#406, security review M3): the run
//!   labelled with one of the carve's passes and cut with that pass's flat
//!   end mill is held as a Pocket of the carve's floor regions with no
//!   stock to leave: the end mill's centre stays inside one, at least its
//!   radius from its outline and islands, and no lower than the floor. The
//!   floor regions are re-derived from the project: the targets' regions
//!   inset by `w(F)`, `F · tan(α/2)` for a V-bit
//!   ([`dc_geometry::floor_regions`]), less those
//!   too thin to cut (area at most the flatten tolerance times the
//!   perimeter), as the generator's floor drops them. The earlier passes'
//!   reach does not bound the run: a rest pass's footprint runs into what
//!   they cleared, and only its centres must touch what they left.
//!
//! - **Inlay** (format 22, #436): each side is held as a V-Carve, its
//!   V-bit's run to S17 and its clearing runs to its floor, with its
//!   regions, start plane and floor re-derived from the project. On
//!   either side the design's parts that come closer than
//!   [`INLAY_MERGE_MM`] are one shape ([`merge_near_parts`], #479), as the
//!   generator merges them. The
//!   female carves the targets' regions from the stock top, flat at
//!   `depth_mm` `D`. The male carves the **surround** of its design: the
//!   targets' closed outlines mirrored about the centre line of their
//!   joint bounding box (`Transform::mirror_x_about`), nested, and
//!   [`dc_geometry::surround_within`] `margin_mm` wide, the band's edge at
//!   the project's flatten tolerance and the plug's outline at the V-bit's
//!   [`dc_geometry::wall_tolerance_mm`] (#478), the generator's
//!   construction, never the generator; where the band runs past the
//!   board it is clipped to [`MaleBand::board`] of the project's stock
//!   (#495, [`dc_geometry::band_on_board`]) and the surround is the
//!   clipped band less the design. Its
//!   start plane is `D − G` below the stock top (`glue_gap_mm` `G`), its
//!   floor `plug_depth_mm` below that. Either side's flank is engaged from
//!   the stock top (security review H4): the plug above the male's start
//!   plane is never cleared. A cut inside the plug, or sideways above the
//!   male's start plane, fails as it would for a V-Carve. Each board is
//!   its own (#498): a run tagged [`dc_project::Board::Male`] is held
//!   against the project on the Inlay's male board
//!   ([`Project::on_male_board`], which the emitter hands this pass), so
//!   "the stock" above, its top, its rectangle and its bottom, is the
//!   male board's for the male, and the female's design and the male's
//!   surround each follow from the board the run is tagged with.
//! - **Inlay cut-out** (format 23, #443): a male's run labelled with its
//!   cut-out's flat end mill, and cut with it, is held to the band's flat
//!   floor rings: the floor regions of its surround (the surround inset
//!   by `P·tan(α/2)`, less noise, as its clearing runs are held), less
//!   those inside a design outline (the floors of the design's holes,
//!   which stay in the plug). The tool centre stays inside a ring, at
//!   least its radius from both its edges, at every depth: above the
//!   floor the carve has cleared the ring to the top, and below it the
//!   cut-out cuts the board's base, which S1 bounds with the cut-out's own
//!   through-cut allowance ([`Material::floor_mm`]). And it stays on the
//!   cut-out's path (#461), derived here: the band's edge round the
//!   mirrored design ([`dc_geometry::rounded_band`]) inset by the tool's radius with
//!   an open edge, or by `P·tan(α/2)` plus it with a V wall
//!   ([`dc_geometry::inset_outers`], [`MaleBand::cut_out_inset_mm`]), on
//!   the band clipped to the board where it runs past it, less, with an
//!   open edge, the stretches along the board's edge, which frees the
//!   plug there (#495, [`dc_geometry::without_rect_sides`]). S7 still
//!   holds every cut, its tool's full radius, on the stock. Its
//!   tabs are held as an On line Profile's: the Profile the cut-out is,
//!   built here on that path, and its tab request placed by the
//!   documented layout rule. Nothing comes from the generator.
//!
//! A move is allowed to depart from where the toolpath put it by what the
//! program may already differ by: one unit in the last written place (the
//! numbers' rounding), the flatten tolerance (arcs written as chords, and
//! the tool-centre outlines and raster ends the generator flattens), and
//! the arc-fit tolerance when arc fitting is on (a run of chords written
//! as an arc, S12).
//!
//! **Fail closed** (code review 1.1, #711). A check that cannot be
//! finished has not passed. Project data that does not resolve (targets,
//! a tool, a stock top, a tab's outline, a tab request the layout rule
//! cannot place) is [`Material::Unresolved`]: the run has no material, so
//! every cut of it is off it ([`Material::held_run`]). A move whose
//! position is not known is refused by each rule that needs the position.
//! The rules read their own data without a lookup that could miss: a loop
//! owns its edges, and a wall test takes its whole area. Where the pass
//! must still look into data it built itself, in S6's matching
//! (`crate::safety`), a miss is [`ExportError::SafetyCheckDefect`], which
//! stops export. The audit of every fallback in the pass, site by site,
//! is in `docs/specs/phase-1/design.md`, "The safety pass fails closed".
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
mod photo;

use cone::{Cone, Fault};
use countersink::Sink;
use dc_cam::RunLabel;
use dc_geometry::profile::BitProfile;
use dc_geometry::{
    Arc, Contour, EPS_MM, Point, Point3, Polyline, Rect, Region, Segment, SelfBand, Transform,
    Vec2, as_circle, band_on_board, closest_points, difference_within, floor_regions,
    inset_is_empty, inset_outers, is_design_hole, meeting_points, merge_near_parts, nearest_point,
    nest, offset, offset_open, point_segment_distance_mm, segment_distance_mm, strip_ends,
    surround_floor_regions, surround_within, wall_tolerance_mm, without_rect_sides,
};
use dc_project::{
    Board, ClearingPass, CornerRelief, CutDirection, DefinitionId, Entry, FeatureDefinition,
    FeatureInstance, INLAY_MERGE_MM, InlayOp, InstanceId, MaleBand, Operation, PlacedTab,
    PostProcessor, ProfileOp, Project, Side, StartAt, TabShape, Tool, ToolId, Units, VCarveOp,
    band_edge_of, cleared_start_plane, male_band,
};
use photo::Picture;

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
        /// A Profile: the trench rule applies (#352). `None` for a
        /// Pocket, which clears its whole region.
        trench: Option<TrenchRule>,
    },
    /// An outside Profile's parts: the tool centre stays outside every
    /// one, clear of its outlines.
    Outside {
        /// The parts, outer and holes, from the targets' closed profiles.
        areas: Vec<Area>,
        /// Corner relief is on, so the notch rule applies.
        relief: bool,
        /// The trench rule (#352).
        trench: TrenchRule,
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
    /// A Photo V-Carve (format 27, #646): S19 (module docs, [`photo`]).
    Photo(Picture),
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
    /// A male Inlay's cut-out run (format 23, #443): the end mill's
    /// centre stays inside the band's floor rings, at least its radius
    /// from their outlines, at any depth: above the floor the carve has
    /// cleared the ring to the top, below it the cut-out cuts the board's
    /// base, which S1 bounds with the cut-out's own allowance. And it
    /// stays on the cut-out's path (#461).
    CutOut {
        /// The floor rings, outer and holes.
        areas: Vec<Area>,
        /// Every segment of every cut-out path, as cut.
        centres: Vec<Edge>,
        /// The cut-out's end mill's radius, mm.
        radius_mm: f64,
        /// The lowest Z it may reach, mm: its through-cut allowance below
        /// the stock bottom.
        floor_mm: f64,
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
    /// Its segments, in order: its own copy of its part of
    /// [`Area::edges`], so no rule looks them up by a range that could
    /// miss (#711).
    edges: Vec<Edge>,
    /// The loop alone, to tell its sides apart.
    alone: Option<Region>,
    /// The loop as wound, for measuring it against itself (#221).
    line: Polyline,
    /// The waste is inside the loop.
    waste_inside: bool,
}

/// Bounds that are near every cut: those of an [`Area`] whose own are not
/// known, so that no rule passes it by (#711).
const EVERYWHERE: Rect = Rect {
    min: Point {
        x: f64::NEG_INFINITY,
        y: f64::NEG_INFINITY,
    },
    max: Point {
        x: f64::INFINITY,
        y: f64::INFINITY,
    },
};

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
    // Each segment with the one before it, the last before the first.
    let before = segments.iter().cycle().skip(n - 1);
    for (k, (prev, next)) in before.zip(&segments).enumerate() {
        let before = (k + n - 1) % n;
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

/// The shortest piece of an arc the notch rule looks at, in slacks: a
/// shorter piece that is neither clear of the walls nor in a relief zone
/// fails the arc. Every point within the slack of a corner's miter point
/// is both clear and in the zone, and a fitted arc that stands for a
/// handoff and a notch passes that point within the arc-fit tolerance
/// and a written unit (S12), the slack less the flatten tolerance. So a
/// stretch of it is both, 1.7 slacks long at the default tolerances, and
/// pieces shorter than that stretch are each clear or in the zone.
const NOTCH_PIECE_SLACKS: f64 = 0.125;

impl Notch {
    /// Whether tool-centre path `path`, which comes nearer a wall of
    /// `area` than `clearance_mm`, is a relief notch (module docs). A
    /// straight path is one when it lies in the relief zone of one of the
    /// area's corners, clear of the area's other edges. An arc is one
    /// when every piece of it is clear of the area's walls or in one
    /// corner's zone: the fitted arc that stands for a handoff and the
    /// notch after it (module docs).
    fn allows(self, area: &Area, path: Segment, clearance_mm: f64) -> bool {
        if path.is_line() {
            return area
                .corners
                .iter()
                .any(|c| self.in_zone(area, c, path, clearance_mm));
        }
        let least_mm = (self.slack_mm * NOTCH_PIECE_SLACKS).max(EPS_MM);
        area.corners
            .iter()
            .filter(|c| point_segment_distance_mm(path, c.at) <= self.reach_mm(c))
            .any(|c| self.arc_allowed(area, c, path, clearance_mm, least_mm))
    }

    /// The farthest from corner `c` its relief zone reaches: the
    /// tool-centre path's miter point, and the slack.
    fn reach_mm(self, c: &Corner) -> f64 {
        (self.radius_mm + self.stock_to_leave_mm) / c.sin_half + self.slack_mm
    }

    /// Whether `path` lies in the relief zone of corner `c` of `area`,
    /// clear of the area's other edges. An arc is held to the zone by its
    /// ends, with the slack less its bow: every point of it is within the
    /// bow of its chord, and the wedge and the reach each hold a chord
    /// whose ends they hold.
    fn in_zone(self, area: &Area, c: &Corner, path: Segment, clearance_mm: f64) -> bool {
        let (r, slack) = (self.radius_mm, self.slack_mm);
        let bow_mm = path.arc().map_or(0.0, |arc| {
            arc.radius_mm * (1.0 - (arc.sweep_rad / 2.0).cos())
        });
        let reach_mm = self.reach_mm(c) - bow_mm;
        let bounds = bounds_of(path);
        c.shorter_mm >= 2.0 * r - slack
            && c.in_wedge(path.start, slack - bow_mm)
            && c.in_wedge(path.end, slack - bow_mm)
            && path.start.distance_to(c.at) <= reach_mm
            && path.end.distance_to(c.at) <= reach_mm
            && point_segment_distance_mm(path, c.at) >= r - slack
            && area
                .edges
                .iter()
                .enumerate()
                .filter(|(i, e)| !c.edges.contains(i) && near(bounds, e.bounds, clearance_mm))
                .all(|(_, e)| segment_distance_mm(path, e.segment) >= clearance_mm)
    }

    /// Whether every piece of arc `piece` is clear of `area`'s walls or
    /// in corner `c`'s relief zone. A piece that is neither is halved,
    /// down to `least_mm`, so only the stretch where the arc passes from
    /// clear stock into the zone is cut small, and an arc that leaves
    /// both fails at the first such piece.
    fn arc_allowed(
        self,
        area: &Area,
        c: &Corner,
        piece: Segment,
        clearance_mm: f64,
        least_mm: f64,
    ) -> bool {
        if clear_of(area, piece, clearance_mm) || self.in_zone(area, c, piece, clearance_mm) {
            return true;
        }
        let length_mm = piece.length_mm();
        let half_mm = length_mm / 2.0;
        // A length that is not a number is not over the least, and fails.
        let long_enough = length_mm > least_mm;
        let halves = (
            sub_segment(piece, length_mm, 0.0, half_mm),
            sub_segment(piece, length_mm, half_mm, length_mm),
        );
        match halves {
            (Some(first), Some(second)) if long_enough => {
                self.arc_allowed(area, c, first, clearance_mm, least_mm)
                    && self.arc_allowed(area, c, second, clearance_mm, least_mm)
            }
            _ => false,
        }
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
    // The region that holds each path, with its place among `regions`:
    // found once and kept, not looked up again by that place (#711: a
    // miss there left a held path with no line beside it to keep clear
    // of).
    let held: Vec<Option<(usize, &Region)>> = open
        .iter()
        .map(|path| regions.iter().enumerate().find(|(_, r)| holds(r, path)))
        .collect();
    let tolerance_mm = tolerance_mm.max(EPS_MM);
    open.iter()
        .zip(&held)
        .enumerate()
        .map(|(index, (path, held_by))| {
            let lines: Vec<&Polyline> = match held_by {
                Some((region_index, region)) => std::iter::once(region.outer())
                    .chain(region.holes())
                    .map(Contour::polyline)
                    .chain(
                        open.iter()
                            .zip(&held)
                            .enumerate()
                            .filter(|(j, (_, h))| {
                                *j != index && h.is_some_and(|(i, _)| i == *region_index)
                            })
                            .map(|(_, (p, _))| *p),
                    )
                    .collect(),
                None => Vec::new(),
            };
            OpenPath {
                edges: edges_of(path).collect(),
                held: held_by.is_some().then(|| (*path).clone()),
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
                let own: Vec<Edge> = edges_of(&polyline).collect();
                corners.extend(corners_of(&polyline, edges.len()));
                edges.extend(own.iter().copied());
                loops.push(Loop {
                    edges: own,
                    alone: nest(vec![contour.clone()])
                        .ok()
                        .and_then(|(r, _)| r.into_iter().next()),
                    line: polyline,
                    waste_inside: inside,
                });
            }
            // #711: an area with no edge has no bounds to be far from, so
            // it is near every cut and no rule skips it (a point at the
            // origin stood in before, which an outside Profile's cut
            // elsewhere was "clear" of).
            let bounds = edges
                .iter()
                .map(|e| e.bounds)
                .reduce(|a, b| a.including(b.min).including(b.max))
                .unwrap_or(EVERYWHERE);
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
    /// unresolved. An Inlay's run is held as its `board`'s (#498): the
    /// female's design on the stock, or the male's surround on its male
    /// board, `project` then being the project on that board
    /// ([`Project::on_male_board`]); a cut-out on the female board is
    /// unresolved.
    pub(crate) fn of_run(
        project: &Project,
        op: &Operation,
        label: &RunLabel,
        tool: &Tool,
        board: Board,
    ) -> Self {
        match (op, label) {
            (Operation::VCarve(carve), RunLabel::Main) if tool.id == carve.tool => {
                v_bit_material(project, carve)
            }
            (Operation::VCarve(carve), RunLabel::Clearing(pass)) if tool.id == *pass => {
                floor_material(project, carve, pass)
            }
            // Format 22 (#436): an Inlay side's runs, as a V-Carve's.
            (Operation::Inlay(inlay), RunLabel::Main) if tool.id == inlay.tool => {
                inlay_material(project, inlay, board)
            }
            (Operation::Inlay(inlay), RunLabel::Clearing(pass)) if tool.id == *pass => {
                inlay_floor_material(project, inlay, pass, board)
            }
            // Format 23 (#443): a male's cut-out, with its own end mill.
            (Operation::Inlay(inlay), RunLabel::CutOut(cut)) if tool.id == *cut => {
                cut_out_material(project, inlay, tool, board)
            }
            (Operation::VCarve(_) | Operation::Inlay(_), _)
            | (_, RunLabel::Clearing(_) | RunLabel::CutOut(_)) => Self::Unresolved,
            // S18 holds a countersink to its own V-bit's angle and cone.
            (Operation::Drill(drill), RunLabel::Main)
                if drill.cycle.is_countersink() && tool.id != drill.tool =>
            {
                Self::Unresolved
            }
            // S19 holds a Photo V-Carve to its own V-bit's cone.
            (Operation::PhotoVCarve(photo), RunLabel::Main) if tool.id != photo.tool => {
                Self::Unresolved
            }
            (_, RunLabel::Main) => Self::of(project, op),
        }
    }

    /// What the run `label` of `op`, cut with `tool` on `board`, is held
    /// to in `project`: its material ([`Self::of_run`]) and the tabs it
    /// must leave standing ([`run_tab_spines`]). When the tabs do not
    /// resolve the run has no material at all, [`Material::Unresolved`]
    /// (#711): a tab the pass cannot lay out is not a tab the run need
    /// not leave.
    pub(crate) fn held_run(
        project: &Project,
        op: &Operation,
        label: &RunLabel,
        tool: &Tool,
        board: Board,
    ) -> (Self, Vec<TabSpine>) {
        match run_tab_spines(project, op, label, tool, board) {
            Some(tabs) => (Self::of_run(project, op, label, tool, board), tabs),
            None => (Self::Unresolved, Vec::new()),
        }
    }

    /// The lowest Z, mm, a run of `op` held to this material may reach
    /// (S1): a male Inlay's cut-out's own through-cut allowance below the
    /// stock bottom (#443), else the operation's.
    pub(crate) fn floor_mm(&self, op: &Operation) -> f64 {
        match self {
            Self::CutOut { floor_mm, .. } => *floor_mm,
            _ => -op.through_cut_allowance_mm(),
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
                    // #711: without a stock top there is no trench rule,
                    // and a Profile is not held without one.
                    Side::Inside => match (
                        regions_of(project, &profile.targets),
                        trench_rule(project, profile),
                    ) {
                        (Some(regions), Some(trench)) => Self::Inside {
                            areas: areas_of(regions, true),
                            stock_to_leave_mm: 0.0,
                            relief,
                            laps: true,
                            trench: Some(trench),
                        },
                        _ => Self::Unresolved,
                    },
                    Side::Outside => match (
                        regions_of(project, &profile.targets),
                        trench_rule(project, profile),
                    ) {
                        (Some(regions), Some(trench)) => Self::Outside {
                            areas: areas_of(regions, false),
                            relief,
                            trench,
                        },
                        _ => Self::Unresolved,
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
            Operation::Inlay(inlay) => inlay_material(project, inlay, Board::Female),
            // Format 27 (#646): S19, the picture's footprint, depth range
            // and link height.
            Operation::PhotoVCarve(photo) => {
                Picture::of(project, photo).map_or(Self::Unresolved, Self::Photo)
            }
        }
    }
}

/// The carving bit's material of V-Carve `carve` (module docs, [`cone`]):
/// its regions, start plane, bit profile, flat-depth floor, engagement
/// and cutting height, all from the project; [`Material::Unresolved`]
/// when its tool is not a usable V-bit or tapered ball nose, its start or
/// flat depth is not usable, or its targets do not resolve.
fn v_bit_material(project: &Project, carve: &VCarveOp) -> Material {
    match cone_of(project, carve) {
        Some(cone) => Material::VBit(cone),
        None => Material::Unresolved,
    }
}

/// The carving bit `id` of `project` as S17 needs it (slice 27d, #680):
/// its profile ([`Tool::carve_profile`]) and its cutting height, mm
/// ([`Tool::cutting_height_mm`]: a V-bit's cone height, a tapered ball
/// nose's `min(flute_length_mm, h_R)`); `None` when it is missing or not
/// a V-bit with an angle in (0°, 180°) and a positive diameter, nor a
/// tapered ball nose with every value set and usable.
pub(crate) fn carve_bit_of(project: &Project, id: &ToolId) -> Option<(BitProfile, f64)> {
    let tool = project.tools.iter().find(|t| &t.id == id)?;
    Some((tool.carve_profile()?, tool.cutting_height_mm()?))
}

/// The V-bit `id` of `project`: its slope `tan(α/2)` and its cone
/// height, mm, through [`carve_bit_of`]; `None` for a tapered ball nose
/// too. A Photo V-Carve's S19 needs a V-bit.
pub(crate) fn v_bit_of(project: &Project, id: &ToolId) -> Option<(f64, f64)> {
    match carve_bit_of(project, id)? {
        (BitProfile::VBit { slope }, cone_mm) => Some((slope, cone_mm)),
        (BitProfile::TaperedBall { .. }, _) => None,
    }
}

/// [`v_bit_material`]'s cone, `None` when it is unresolved.
fn cone_of(project: &Project, carve: &VCarveOp) -> Option<Cone> {
    let thickness_mm = project.stock.thickness_mm?;
    let (profile, cone_mm) = carve_bit_of(project, &carve.tool)?;
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
        profile,
        floor_z_mm: carve.flat_depth_mm.map(|f| start_plane_z_mm - f),
        engaged_from_z_mm: if cleared_at_start {
            start_plane_z_mm
        } else {
            thickness_mm
        },
        cone_mm,
        edge: None,
    })
}

/// The V-bit material of Inlay side `inlay` (#436; module docs, [`cone`]):
/// [`Material::Unresolved`] when [`inlay_cone`] is.
fn inlay_material(project: &Project, inlay: &InlayOp, board: Board) -> Material {
    inlay_cone(project, inlay, board).map_or(Material::Unresolved, |(cone, _)| Material::VBit(cone))
}

/// A male Inlay's band as the safety pass derives it (#448; module docs):
/// the band's edge rule from [`dc_project::male_band`], the geometry from
/// `dc_geometry`, never the generator.
struct MaleGeometry {
    /// The edge, open or a V wall, and the roundings.
    band: MaleBand,
    /// The mirrored design, nested ([`male_design`]).
    design: Vec<Region>,
    /// The male's mirror ([`male_design`]), which a drawn boundary is
    /// mirrored by too ([`dc_project::band_edge`], #603).
    mirror: Option<Transform>,
    /// The carved regions: the band less the design ([`surround_within`]).
    surround: Vec<Region>,
    /// The band `B` ([`dc_geometry::rounded_band`]) when its edge is open; empty with
    /// a V wall. Clipped to the board when it runs past it (#495).
    edge: Vec<Region>,
}

impl MaleGeometry {
    /// The male Inlay `inlay`'s; `None` when its band, its design or its
    /// bit does not resolve. Where the band `B` runs past the board it is clipped to
    /// [`MaleBand::board`] (#495; [`male_band_on_board`]), and the
    /// surround is the clipped band less the design.
    fn of(project: &Project, inlay: &InlayOp) -> Option<Self> {
        let band = male_band(project, inlay)?;
        let (design, mirror) = male_design(project, inlay)?;
        let tolerance_mm = project.flatten_tolerance_mm;
        // #478: the plug's outline within the bit's wall tolerance, as the
        // generator draws it (#681: a tapered ball nose's `tan β`). #711:
        // without a usable bit there is no such tolerance, and no band.
        let (profile, _) = carve_bit_of(project, &inlay.tool)?;
        let plug_tolerance_mm = wall_tolerance_mm(tolerance_mm, profile.slope());
        let (edge, clipped) = male_band_on_board(project, inlay, &band, mirror, &design)?;
        let drawn = matches!(band.source, dc_project::BandSource::Drawn { .. });
        let surround = if clipped || drawn {
            difference_within(&edge, &design, tolerance_mm, plug_tolerance_mm)
        } else {
            surround_within(
                &design,
                band.margin_mm,
                band.edge_rounding_mm,
                tolerance_mm,
                plug_tolerance_mm,
            )
        };
        let edge = if band.is_open() { edge } else { Vec::new() };
        Some(Self {
            band,
            design,
            mirror,
            surround,
            edge,
        })
    }

    /// The floor regions of the surround, inset by `inset_mm`: out to the
    /// band's edge where it is open ([`surround_floor_regions`]), less
    /// noise, as the generator's floor drops it.
    fn floors(&self, inset_mm: f64, tolerance_mm: f64) -> Vec<Region> {
        surround_floor_regions(
            &self.surround,
            &self.design,
            inset_mm,
            self.band.is_open(),
            tolerance_mm,
        )
        .into_iter()
        .filter(|f| !is_noise(f, tolerance_mm))
        .collect()
    }
}

/// The cone of Inlay side `inlay`, every value re-derived from the project
/// (design "Inlay" steps 1 to 4, "Safety pass" S17; security review H4),
/// with the male's band geometry:
///
/// - **Female:** the targets' regions, their near parts merged
///   ([`merged_design`]), from the stock top, flat at `depth_mm` `D`.
/// - **Male:** the surround of the targets mirrored about the centre line
///   of their closed outlines' joint bounding box ([`MaleGeometry`]): the
///   band `margin_mm` wide, closed by its edge rounding (#448), less the
///   design; the start plane `D − G` below the stock top, flat at
///   `plug_depth_mm` `P` below that. When a clearing pass cuts the band's
///   edge upright, the cone is also held to the band at the stock top
///   ([`Cone::edge`]).
///
/// Either side's flank is engaged from the stock top: the female starts
/// there, and the plug above the male's start plane is never cleared.
/// `None` when the tool is not a usable V-bit, a value is unset or out of
/// the range validation allows, or the targets do not resolve.
fn inlay_cone(
    project: &Project,
    inlay: &InlayOp,
    board: Board,
) -> Option<(Cone, Option<MaleGeometry>)> {
    let thickness_mm = project.stock.thickness_mm.filter(|t| t.is_finite())?;
    // #681: a V-bit or a tapered ball nose, on its profile.
    let (profile, cone_mm) = carve_bit_of(project, &inlay.tool)?;
    let depth_mm = inlay.depth_mm.filter(|d| d.is_finite() && *d > 0.0)?;
    let glue_gap_mm = inlay
        .glue_gap_mm
        .filter(|g| g.is_finite() && *g >= 0.0 && *g < depth_mm)?;
    let plug_depth_mm = inlay.plug_depth_mm.filter(|p| p.is_finite() && *p > 0.0)?;
    let (regions, start_depth_mm, flat_depth_mm, male) = match board {
        Board::Female => (
            merged_design(project, inlay, regions_of(project, &inlay.targets)?)?,
            0.0,
            depth_mm,
            None,
        ),
        Board::Male => {
            let male = MaleGeometry::of(project, inlay)?;
            (
                male.surround.clone(),
                depth_mm - glue_gap_mm,
                plug_depth_mm,
                Some(male),
            )
        }
    };
    let start_plane_z_mm = thickness_mm - start_depth_mm;
    let edge = male.as_ref().filter(|m| m.band.is_open()).map(|m| {
        Box::new(Cone {
            areas: areas_of(m.edge.clone(), true),
            start_plane_z_mm: thickness_mm,
            profile,
            floor_z_mm: None,
            engaged_from_z_mm: thickness_mm,
            cone_mm,
            edge: None,
        })
    });
    let cone = Cone {
        areas: areas_of(regions, true),
        start_plane_z_mm,
        profile,
        floor_z_mm: Some(start_plane_z_mm - flat_depth_mm),
        engaged_from_z_mm: thickness_mm,
        cone_mm,
        edge,
    };
    Some((cone, male))
}

/// The band `B` of male `design` ([`dc_project::band_edge_of`]: the
/// design grown by `band`'s margin and closed by its edge rounding, or the
/// drawn boundary mirrored, #603) on the board (#495): clipped to [`MaleBand::board`]
/// of the project's stock ([`band_on_board`]) when it runs past it, and
/// whether it was; as it is when it stays on the board. `None` when a
/// drawn boundary does not resolve (#711): the band computed from the
/// margin stood in for it before, and the run was held to an edge the
/// project does not draw.
fn male_band_on_board(
    project: &Project,
    inlay: &InlayOp,
    band: &MaleBand,
    mirror: Option<Transform>,
    design: &[Region],
) -> Option<(Vec<Region>, bool)> {
    let tolerance_mm = project.flatten_tolerance_mm;
    // #603: `dc_project::band_edge`, as the generator derives it.
    let edge = band_edge_of(project, inlay, band, mirror, design)?;
    Some(
        match band_on_board(&edge, band.board(&project.stock), tolerance_mm) {
            Some(clipped) => (clipped, true),
            None => (edge, false),
        },
    )
}

/// A male Inlay's design: the closed outlines of `targets`, placed,
/// mirrored about X = the centre of their joint bounding box (arc extremes
/// included, [`Transform::mirror_x_about`]), nested, its near parts merged
/// ([`merged_design`]). `None` when a target does not resolve or
/// outlines cross.
fn male_design(project: &Project, inlay: &InlayOp) -> Option<(Vec<Region>, Option<Transform>)> {
    let closed: Vec<Polyline> = placed(project, &inlay.targets)?
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
    let mirror = bounds.map(|b| Transform::mirror_x_about((b.min.x + b.max.x) / 2.0));
    let contours: Vec<Contour> = match mirror {
        Some(mirror) => closed
            .iter()
            .map(|p| {
                mirror
                    .apply_polyline(p)
                    .ok()
                    .and_then(|m| Contour::new(m).ok())
            })
            .collect::<Option<_>>()?,
        None => Vec::new(),
    };
    let (design, _) = nest(contours).ok()?;
    Some((merged_design(project, inlay, design)?, mirror))
}

/// Inlay `inlay`'s `design` (nested) with the parts that come closer
/// than [`INLAY_MERGE_MM`] merged into one shape, drawn within its bit's
/// wall tolerance ([`wall_tolerance_mm`], #478), as the generator merges
/// them (#479; [`merge_near_parts`]), then rounded for a tapered ball nose
/// ([`dc_project::rounded_design`], #681), as the generator rounds it.
/// `None` when the Inlay has no usable bit to take the wall tolerance
/// from (#711), or when the rounding refuses the design, which the
/// generator refuses too: no program of it is held to anything.
fn merged_design(project: &Project, inlay: &InlayOp, design: Vec<Region>) -> Option<Vec<Region>> {
    let tolerance_mm = project.flatten_tolerance_mm;
    let (profile, _) = carve_bit_of(project, &inlay.tool)?;
    let plug_tolerance_mm = wall_tolerance_mm(tolerance_mm, profile.slope());
    let merged = merge_near_parts(&design, INLAY_MERGE_MM, plug_tolerance_mm).regions;
    dc_project::rounded_design(project, inlay, &merged)
        .ok()
        .map(|rounded| rounded.regions)
}

/// What a male Inlay's cut-out run is held to (#443, #448; module docs):
/// the rings of the band's flat floor round the outer outlines of the
/// mirrored design, re-derived from the project with `dc_geometry` and
/// [`dc_project::male_band`] alone, and the cut-out's paths, for its tabs.
struct CutOutGeometry {
    /// The floor rings: the floor regions of the male's surround (out to
    /// the band's edge when it is open, [`MaleGeometry::floors`], as its
    /// clearing runs are held), except those inside a design outline, the
    /// floors of the design's holes, which stay in the plug.
    rings: Vec<Region>,
    /// The tool-centre paths (#461): the outer outlines of the band's
    /// edge round the mirrored design ([`dc_geometry::rounded_band`], closed by the
    /// edge's rounding, clipped to the board where it runs past it, #495)
    /// inset by [`MaleBand::cut_out_inset_mm`]
    /// ([`dc_geometry::inset_outers`]); with an open edge clipped to the
    /// board, less their stretches along the board's edge inset by the
    /// same ([`without_rect_sides`]), so open from the board's edge to the
    /// board's edge.
    paths: Vec<Polyline>,
    /// For each outline the board's edge leaves as open paths, where its
    /// pieces are in `paths` (#710): they are one cut, and take the
    /// cut-out's tab request between them.
    pieces: Vec<Range<usize>>,
}

/// [`CutOutGeometry`] of Inlay `inlay`'s cut-out run with `tool`; `None`
/// when it has no cut-out, is not a male, `tool` is not the cut-out's
/// flat end mill, its V-bit or values are unusable, or its targets do
/// not resolve.
fn cut_out_geometry(
    project: &Project,
    inlay: &InlayOp,
    tool: &Tool,
    board: Board,
) -> Option<CutOutGeometry> {
    let cut_out = inlay.cut_out.as_ref()?;
    let radius_mm = tool.diameter_mm / 2.0;
    let usable_tool = tool.id == cut_out.tool
        && tool.cutter.is_flat()
        && radius_mm.is_finite()
        && radius_mm > 0.0;
    if board != Board::Male || !usable_tool {
        return None;
    }
    let (cone, male) = inlay_cone(project, inlay, board)?;
    let male = male?;
    let floor_z_mm = cone.floor_z_mm?;
    let inset_mm = cone.radius_mm(floor_z_mm);
    let tolerance_mm = project.flatten_tolerance_mm;
    let rings = male
        .floors(inset_mm, tolerance_mm)
        .into_iter()
        .filter(|f| !is_design_hole(f, &male.design))
        .collect();
    let inset_mm = male.band.cut_out_inset_mm(tool.diameter_mm);
    let (edge, clipped) =
        male_band_on_board(project, inlay, &male.band, male.mirror, &male.design)?;
    let outlines = inset_outers(&edge, inset_mm, tolerance_mm);
    let mut pieces = Vec::new();
    let paths = if clipped && male.band.is_open() {
        // #495: the board's edge frees the plug where the band is clipped
        // to it.
        let board = male.band.board(&project.stock);
        let within = Rect {
            min: Point::new(board.min.x + inset_mm, board.min.y + inset_mm),
            max: Point::new(board.max.x - inset_mm, board.max.y - inset_mm),
        };
        let mut paths = Vec::new();
        for outline in &outlines {
            let from = paths.len();
            paths.extend(without_rect_sides(
                std::slice::from_ref(outline),
                within,
                tolerance_mm,
            ));
            pieces.push(from..paths.len());
        }
        paths
    } else {
        outlines.into_iter().map(Contour::into_polyline).collect()
    };
    Some(CutOutGeometry {
        rings,
        paths,
        pieces,
    })
}

/// The material of Inlay `inlay`'s cut-out run with `tool` (#443): its
/// floor rings, the tool's radius and the cut-out's own through-cut
/// allowance; [`Material::Unresolved`] when [`cut_out_geometry`] is
/// `None`.
fn cut_out_material(project: &Project, inlay: &InlayOp, tool: &Tool, board: Board) -> Material {
    let Some((geometry, cut_out)) =
        cut_out_geometry(project, inlay, tool, board).zip(inlay.cut_out.as_ref())
    else {
        return Material::Unresolved;
    };
    let allowance_mm = cut_out.through_cut_allowance_mm;
    if !(allowance_mm.is_finite() && allowance_mm >= 0.0) {
        return Material::Unresolved;
    }
    Material::CutOut {
        areas: areas_of(geometry.rings, true),
        centres: geometry.paths.iter().flat_map(edges_of).collect(),
        radius_mm: tool.diameter_mm / 2.0,
        floor_mm: -allowance_mm,
    }
}

/// The material of carve `carve`'s clearing run with end mill `pass`
/// (#406, security review M3; module docs): the floor regions re-derived
/// from the targets, the V-bit's angle and the flat depth
/// ([`floor_regions`], less noise as the generator drops it), at the
/// floor's Z. [`Material::Unresolved`] when `pass` is none of the carve's
/// passes or not a flat end mill, or the carve has no usable V-bit or
/// flat depth.
fn floor_material(project: &Project, carve: &VCarveOp, pass: &ToolId) -> Material {
    floor_of(project, &carve.clearing, pass, |inset_mm, tolerance_mm| {
        let cone = cone_of(project, carve)?;
        let regions: Vec<Region> = cone.areas.iter().map(|a| a.region.clone()).collect();
        let floors = floor_regions(&regions, inset_mm(&cone)?, tolerance_mm)
            .into_iter()
            .filter(|f| !is_noise(f, tolerance_mm))
            .collect();
        Some((cone, floors))
    })
}

/// The material of male or female Inlay `inlay`'s clearing run with end
/// mill `pass` (#436): its floor, out to the band's edge when a clearing
/// pass cuts that edge upright (#448, [`MaleGeometry::floors`]).
fn inlay_floor_material(
    project: &Project,
    inlay: &InlayOp,
    pass: &ToolId,
    board: Board,
) -> Material {
    floor_of(project, &inlay.clearing, pass, |inset_mm, tolerance_mm| {
        let (cone, male) = inlay_cone(project, inlay, board)?;
        let inset_mm = inset_mm(&cone)?;
        let floors = match male {
            Some(male) => male.floors(inset_mm, tolerance_mm),
            None => {
                let regions: Vec<Region> = cone.areas.iter().map(|a| a.region.clone()).collect();
                floor_regions(&regions, inset_mm, tolerance_mm)
                    .into_iter()
                    .filter(|f| !is_noise(f, tolerance_mm))
                    .collect()
            }
        };
        Some((cone, floors))
    })
}

/// [`floor_material`] for a carve with passes `clearing`: the floor
/// regions `floors` gives with its V-bit material's cone (a V-Carve's, or
/// an Inlay side's, #436), at the floor's Z. `floors` is handed a function
/// that gives a cone's floor inset, `F · tan(α/2)`, and the tolerance.
fn floor_of(
    project: &Project,
    clearing: &[ClearingPass],
    pass: &ToolId,
    floors: impl FnOnce(&dyn Fn(&Cone) -> Option<f64>, f64) -> Option<(Cone, Vec<Region>)>,
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
        let inset_mm = |cone: &Cone| cone.floor_z_mm.map(|floor_z_mm| cone.radius_mm(floor_z_mm));
        let (cone, floors) = floors(&inset_mm, project.flatten_tolerance_mm)?;
        Some(Material::Floor {
            areas: areas_of(floors, true),
            floor_z_mm: cone.floor_z_mm?,
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
    /// Closed on a closed outline, open on an open one. For an open path
    /// of a Left or Right Profile, its offset to the cut side alone.
    paths: Vec<Polyline>,
    /// The laps among [`TabSpine::paths`]: the outline offset by the tool
    /// radius both ways (to the cut side alone beside an open path of a
    /// Left or Right Profile), project space. Empty for an On line
    /// Profile, whose lap is the outline.
    laps: Vec<Polyline>,
    /// The paths of the passes before a last pass (#284): the outline
    /// offset by the radius plus the last pass's stock to leave, the ways
    /// the laps are, project space. Empty without a last pass.
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

/// The tabs run `label` of `op`, cut with `tool`, must leave standing in
/// `project` (S9): a Profile's ([`tab_spines`]), and a male Inlay's
/// cut-out's (#443). The cut-out is held as the On line Profile it is
/// ([`dc_cam::inlay::cut_out_profile`]) on the paths this pass derives
/// itself ([`cut_out_geometry`]), so its tabs are those its tab request
/// places there, by the documented layout rule ([`cut_out_placed`]).
/// Empty for every other run. `None` when the tabs do not resolve
/// ([`tab_spines`]) or the cut-out does not (#711): the run is then
/// unresolved ([`Material::held_run`]), where it was held to no tab.
pub(crate) fn run_tab_spines(
    project: &Project,
    op: &Operation,
    label: &RunLabel,
    tool: &Tool,
    board: Board,
) -> Option<Vec<TabSpine>> {
    match (op, label) {
        (_, RunLabel::Main) => tab_spines(project, op),
        (Operation::Inlay(inlay), RunLabel::CutOut(_)) => {
            let copy = cut_out_placed(project, inlay, tool, board)?;
            tab_spines(&copy, copy.operations.first()?)
        }
        _ => Some(Vec::new()),
    }
}

/// `project` as S9 holds Inlay `inlay`'s cut-out run with `tool` (#443,
/// #710): the On line Profile the cut-out is, on the paths this pass
/// derives itself ([`cut_out_geometry`], [`cut_out_profile`]), with the
/// cut-out's tab request placed on them as placed tabs, as the generator
/// places it before it cuts (ADR-0005). The request is for each outline;
/// the open paths the board's edge leaves of one outline take it between
/// them, by the documented layout rule
/// ([`dc_cam::placed_tabs::place_shared`]), so a cut-out asked for four
/// tabs is held to four however many pieces it is cut in. `None` when
/// the cut-out does not resolve, or its request cannot be placed (#711:
/// no tab was placed then, so the run was held to none; the generator
/// refuses such a request).
pub(crate) fn cut_out_placed(
    project: &Project,
    inlay: &InlayOp,
    tool: &Tool,
    board: Board,
) -> Option<Project> {
    let geometry = cut_out_geometry(project, inlay, tool, board)?;
    let cut_out = inlay.cut_out.as_ref()?;
    let thickness_mm = project.stock.thickness_mm?;
    let mut copy = cut_out_profile(project, inlay, cut_out, &geometry.paths, thickness_mm);
    if let Some(spec) = &cut_out.tabs {
        let placement =
            dc_cam::placed_tabs::place_shared(&copy, &inlay.id, spec, &geometry.pieces).ok()?;
        copy.tabs.extend(placement.tabs);
    }
    Some(copy)
}

/// `project` holding the On line Profile a male Inlay's cut-out is
/// (#443), as its only operation: `paths` (project space; closed, or
/// open where the board's edge frees the plug) as the
/// profiles of one definition placed in place, which the Profile targets
/// with the cut-out's tool and through-cut allowance, through the
/// stock `thickness_mm` thick, climb, plunging, its start where the
/// paths start, with the Inlay's id. It carries no tab request: the
/// cut-out's is placed on these paths ([`cut_out_placed`]), as
/// `docs/book`'s Inlay chapter documents the cut-out.
fn cut_out_profile(
    project: &Project,
    inlay: &InlayOp,
    cut_out: &dc_project::InlayCutOut,
    paths: &[Polyline],
    thickness_mm: f64,
) -> Project {
    let mut copy = project.clone();
    let mut stem = format!("{}-cut-out", inlay.id);
    while copy.instances.iter().any(|i| i.id.as_str() == stem)
        || copy.definitions.keys().any(|d| d.as_str() == stem)
    {
        stem.push('_');
    }
    let definition = DefinitionId::new(stem.clone());
    let instance = InstanceId::new(stem);
    copy.definitions.insert(
        definition.clone(),
        FeatureDefinition {
            id: definition.clone(),
            name: format!("{} cut-out", inlay.name),
            profiles: paths.to_vec(),
        },
    );
    copy.instances.push(FeatureInstance {
        id: instance.clone(),
        definition,
        transform: Transform::IDENTITY,
        name: format!("{} cut-out", inlay.name),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    });
    copy.operations = vec![Operation::Profile(ProfileOp {
        id: inlay.id.clone(),
        name: inlay.name.clone(),
        enabled: true,
        targets: vec![instance],
        tool: cut_out.tool.clone(),
        side: Side::OnLine,
        depth_mm: Some(thickness_mm + cut_out.through_cut_allowance_mm),
        step_down_mm: None,
        direction: CutDirection::Climb,
        through_cut_allowance_mm: cut_out.through_cut_allowance_mm,
        safe_z_mm: inlay.safe_z_mm,
        tabs: None,
        machining_order: inlay.machining_order,
        entry: Entry::Plunge,
        lead: None,
        corner_relief: CornerRelief::None,
        last_pass: None,
        start: StartAt::Auto,
    })];
    copy
}

/// The tabs Profile `op` must leave standing in `project` (S9, #337): its
/// placed tabs on the outlines of its targets that it cuts, and those its
/// tab request places (`dc_cam::placed_tabs::place`, the documented layout
/// rule, not the toolpath). Empty for another kind of operation, and for
/// a Profile with no tab on its targets.
///
/// A tab's spine runs along its outline ([`tab_spine`]): a closed one, or
/// an open one an On line Profile cuts (#175), such as each piece the
/// board's edge leaves of a male Inlay's cut-out (#495). A tab on an open
/// outline of an inside or outside Profile, which cuts closed outlines
/// only, is no tab of that Profile. A tab on an open outline of a Left or
/// Right Profile (#207) has its spine along the path offset to the cut
/// side (`docs/specs/phase-3/design.md`, "S9, an Inlay cut-out").
///
/// `None` when a tab cannot be laid out (#711), and the run is then
/// unresolved ([`Material::held_run`]): the request cannot be placed, the
/// tool or the cut's floor (the stock thickness and the depth) is
/// missing, or a tab's instance, definition or profile is, or its outline
/// cannot be placed or is no contour. Each of those dropped the tab
/// before, and the run was held to the tabs that were left.
pub(crate) fn tab_spines(project: &Project, op: &Operation) -> Option<Vec<TabSpine>> {
    let Operation::Profile(profile) = op else {
        return Some(Vec::new());
    };
    let requested = match &profile.tabs {
        Some(spec) => {
            dc_cam::placed_tabs::place(project, &profile.id, spec)
                .ok()?
                .tabs
        }
        None => Vec::new(),
    };
    let tabs: Vec<&PlacedTab> = project
        .tabs
        .iter()
        .chain(&requested)
        .filter(|t| profile.targets.contains(&t.instance))
        .collect();
    if tabs.is_empty() {
        return Some(Vec::new());
    }
    let radius_mm = project
        .tools
        .iter()
        .find(|t| t.id == profile.tool)
        .map(|t| t.diameter_mm / 2.0)?;
    let floor_z_mm = project
        .stock
        .thickness_mm
        .zip(profile.depth_mm)
        .map(|(thickness_mm, depth_mm)| thickness_mm - depth_mm)?;
    let mut spines = Vec::with_capacity(tabs.len());
    for tab in tabs {
        if let Some(spine) = tab_spine(project, profile, tab, radius_mm, floor_z_mm)? {
            spines.push(spine);
        }
    }
    Some(spines)
}

/// The spine of `tab`, one of Profile `profile`'s, cut with a tool of
/// `radius_mm` to `floor_z_mm` above the stock bottom ([`tab_spines`]).
/// `Some(None)` for a tab with no spine: on an open outline of an inside
/// or outside Profile, which does not cut it, or on a closed outline that
/// is no contour of a Left or Right Profile, which cuts open paths only.
/// `None` when the tab's outline does not resolve.
///
/// On a closed outline the bit's paths are the outline and its offsets by
/// the radius both ways ([`offset`]). On an open one an On line Profile
/// cuts they are the same, each offset the open paths beside it
/// ([`offset_open`]): a spine was built on a closed outline only, so a
/// plunge into a tab on a piece of a cut-out the board's edge had left
/// open was not held. On an open one a Left or Right Profile cuts (#207)
/// they are its offset by the radius to that side alone, to the left or
/// the right looking from its first vertex to its last, as the generator
/// cuts it and [`side_profile`] holds it; with a last pass, the stock
/// path the same way. The bit runs nowhere else beside such a path, and a
/// tab its request lays is centred on that offset, so its span is
/// measured along the path the generator laid it on.
fn tab_spine(
    project: &Project,
    profile: &ProfileOp,
    tab: &PlacedTab,
    radius_mm: f64,
    floor_z_mm: f64,
) -> Option<Option<TabSpine>> {
    let instance = project.instances.iter().find(|i| i.id == tab.instance)?;
    let outline = project
        .definitions
        .get(&instance.definition)?
        .profiles
        .get(tab.profile)?;
    let outline = instance.transform.apply_polyline(outline).ok()?;
    let contour = if outline.is_closed() {
        match Contour::new(outline.clone()) {
            Ok(contour) => Some(contour),
            // A Left or Right Profile cuts open paths only (#207), so a
            // closed outline that is no contour is none of its to resolve.
            Err(_) if matches!(profile.side, Side::Left | Side::Right) => return Some(None),
            Err(_) => return None,
        }
    } else if matches!(profile.side, Side::OnLine | Side::Left | Side::Right) {
        None
    } else {
        return Some(None);
    };
    // The sides of the outline a lap runs on: both, but for an open path
    // of a Left or Right Profile, which is cut to that side of it only.
    let (signs, cut_beside): (&[f64], bool) = match (&contour, profile.side) {
        (None, Side::Left) => (&[1.0], true),
        (None, Side::Right) => (&[-1.0], true),
        _ => (&[1.0, -1.0], false),
    };
    let offsets = |delta_mm: f64| -> Vec<Polyline> {
        signs
            .iter()
            .flat_map(|sign| match &contour {
                Some(contour) => offset(contour, sign * delta_mm)
                    .into_iter()
                    .map(Contour::into_polyline)
                    .collect(),
                None => offset_open(&outline, sign * delta_mm),
            })
            .collect()
    };
    let laps = offsets(radius_mm);
    let stock_paths = profile
        .last_pass
        .as_ref()
        .map(|l| offsets(radius_mm + l.stock_to_leave_mm))
        .unwrap_or_default();
    let mut paths = if cut_beside {
        Vec::new()
    } else {
        vec![outline]
    };
    paths.extend(laps.iter().cloned());
    let (hold_mm, slope_mm) = match tab.shape {
        TabShape::Rect => (radius_mm + tab.width_mm / 2.0, 0.0),
        TabShape::Smooth => (radius_mm, tab.width_mm / 2.0),
    };
    Some(Some(TabSpine {
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
    }))
}

impl TabSpine {
    /// Whether a cut down to `z_mm` at `p`, within `slack_mm` of one of
    /// the bit's paths round the tab's outline, goes below the tab there.
    /// On a lap's path (or the outline) that is a cut within the tab's
    /// span ([`TabSpine::lowest_z_mm`]). On the path of the passes before
    /// a last pass (#353) it is a cut into the tab material the laps leave
    /// standing ([`TabSpine::cuts_material`]), the tabs of the outline
    /// `tabs` merged; on an On line Profile, which has no such laps, the
    /// span again. Along a closed path the span runs either way round it;
    /// along an open one (a piece of a clipped cut-out, #495) only along
    /// it, never from one end round to the other.
    ///
    /// A path with no length, on which no point is nearest (#711), is one
    /// the cut cannot be told to be clear of: the cut goes below the tab.
    fn cuts_into(&self, p: Point, z_mm: f64, slack_mm: f64, tabs: &[TabSpine]) -> bool {
        let by_span = |path: &Polyline| {
            let (Some(at), Some(centre)) =
                (nearest_point(path, p), nearest_point(path, self.point))
            else {
                return true;
            };
            if at.gap_mm > slack_mm || centre.gap_mm > self.reach_mm + slack_mm {
                return false;
            }
            let raw = (at.along_mm - centre.along_mm).abs();
            let along = if path.is_closed() {
                raw.min(path.length_mm() - raw)
            } else {
                raw
            };
            z_mm < self.lowest_z_mm(along) - slack_mm
        };
        let on_stock_path = || {
            self.stock_paths
                .iter()
                .any(|path| nearest_point(path, p).is_none_or(|at| at.gap_mm <= slack_mm))
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
    /// [`TAB_SEARCH_CELLS`] cells refuses the plunge, and so does a lap a
    /// tab cannot be found on or that cannot be split whole (#711).
    fn cuts_material(&self, p: Point, z_mm: f64, slack_mm: f64, tabs: &[TabSpine]) -> bool {
        let r = self.radius_mm;
        let reach_mm = 2.0 * r + slack_mm;
        let at = Rect::from_point(p);
        let mut high: Vec<Segment> = Vec::new();
        let mut low: Vec<Segment> = Vec::new();
        for lap in &self.laps {
            let length_mm = lap.length_mm();
            let mut held: Vec<(f64, f64)> = Vec::new();
            for t in tabs.iter().filter(|t| t.outline_id == self.outline_id) {
                let Some(centre) = nearest_point(lap, t.point) else {
                    return true;
                };
                let half_mm = t.held_mm(z_mm, slack_mm);
                if centre.gap_mm <= t.reach_mm + slack_mm && half_mm > 0.0 {
                    held.push((centre.along_mm - half_mm, centre.along_mm + half_mm));
                }
            }
            let whole = split_lap(lap, length_mm, &held, |piece, is_high| {
                if near(at, bounds_of(piece), reach_mm) {
                    if is_high {
                        high.push(piece);
                    } else {
                        low.push(piece);
                    }
                }
            });
            if !whole {
                return true;
            }
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
/// [`nearest_point`] measures along a path. `false` when a piece could
/// not be cut out of its segment (#711): the lap was not handed over
/// whole, and the caller must not read what it got as all of it.
#[must_use]
fn split_lap(
    lap: &Polyline,
    length_mm: f64,
    held: &[(f64, f64)],
    mut f: impl FnMut(Segment, bool),
) -> bool {
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
            let Some(piece) = sub_segment(segment, seg_mm, a, b) else {
                return false;
            };
            f(piece, in_held(walked_mm + (a + b) / 2.0));
        }
        walked_mm += seg_mm;
    }
    true
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
/// pass, into the tab material the laps leave (#353) ([`TabSpine`]); and
/// `false` when the span has tabs and where the move starts or ends is
/// not known (#711), since it cannot then be told from such a plunge.
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
        return false;
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
/// when the stock thickness is not set, and the Profile is then
/// unresolved ([`Material::of`]).
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
    /// ([`below_top`]). Nor does a cut whose start or end is not known
    /// (#711): it cannot be told from such a ramp, and ends the run.
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
            return false;
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
                // #711: a point of the path that cannot be found is in
                // no trench (the path's end stood in for it before).
                let in_trench = path
                    .point_at(s)
                    .is_some_and(|(p, _)| self.on_ring(p, ring_mm) || self.on_earlier(p, near_mm));
                if z < top_mm && !in_trench {
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
pub(crate) fn slack_mm(post: &PostProcessor, units: Units, tolerances: Tolerances) -> f64 {
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
    // S19 likewise: a Photo V-Carve's links and rapids are its business.
    if let Material::Photo(picture) = span.material {
        return picture.check(blocks, post, transform, tolerances, span);
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
            trench: rule,
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
            Material::Countersink(_) | Material::Photo(_) => false,
            Material::Floor {
                areas,
                floor_z_mm,
                radius_mm,
            } => z_mm.is_some_and(|(from_z, to_z)| {
                from_z.min(to_z) >= floor_z_mm - slack_mm
                    && in_regions(areas, path, radius_mm - slack_mm, None, &mut last)
            }),
            Material::CutOut {
                areas,
                centres,
                radius_mm,
                ..
            } => {
                in_regions(areas, path, radius_mm - slack_mm, None, &mut last)
                    && points_of(path)
                        .iter()
                        .all(|&p| on_line(centres, p, slack_mm))
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

/// Whether tool-centre path `path` keeps `clearance_mm` from every edge
/// of `area`, or is a relief notch at one of its corners when `notch` is
/// set. It took a range of the area's edges to hold the path to, and read
/// a range outside them as no wall at all (#711); every caller meant the
/// whole area, so there is no range now.
fn walls_allow(area: &Area, path: Segment, clearance_mm: f64, notch: Option<Notch>) -> bool {
    clear_of(area, path, clearance_mm) || notch.is_some_and(|n| n.allows(area, path, clearance_mm))
}

/// Whether tool-centre path `path` keeps `clearance_mm` from every edge
/// of `area`.
fn clear_of(area: &Area, path: Segment, clearance_mm: f64) -> bool {
    let bounds = bounds_of(path);
    area.edges
        .iter()
        .filter(|e| near(bounds, e.bounds, clearance_mm))
        .all(|e| segment_distance_mm(path, e.segment) >= clearance_mm)
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
    let clear = walls_allow(area, path, clearance_mm, notch);
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
        .all(|a| !a.region.contains_point(side) && walls_allow(a, path, clearance_mm, notch))
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
                points.iter().all(|&p| {
                    l.edges
                        .iter()
                        .any(|e| point_segment_distance_mm(e.segment, p) <= reach_mm)
                }) && l
                    .alone
                    .as_ref()
                    .is_some_and(|r| r.contains_point(side_point(path)) == l.waste_inside)
                    && walls_allow(area, path, clearance_mm, notch)
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
            let loops = area.loops.iter().zip(own).enumerate();
            loops.into_iter().any(|(this, (l, own))| {
                // Every other loop of the area.
                let others = area
                    .loops
                    .iter()
                    .enumerate()
                    .filter(|(other, _)| *other != this)
                    .flat_map(|(_, o)| &o.edges);
                points.iter().all(|&p| on_line(&l.edges, p, slack_mm))
                    && others
                        .filter(|e| near(bounds, e.bounds, band_mm))
                        .all(|e| segment_distance_mm(path, e.segment) >= band_mm)
                    && keeps_self(own, path, band, |p, q| {
                        material_between(&area.region, &l.edges, p, q)
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
mod tests;
