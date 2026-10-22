// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/entry.ts
//   src/engine/toolpaths/entry.test.ts

//! Entry (`docs/specs/phase-2/design.md`, "Algorithms → Entry
//! (dc-cam::entry)"; functional spec capability 16, safety rules S9 and
//! S10): how the tool reaches each pass depth. [`plan`] places a helix or a
//! zigzag ramp in the pass's clearance regions (the tool-centre area the
//! pass removes) next to a block's entry point, once per entry point;
//! [`EntryPlan::descend`] writes the descent from a start Z down to a pass
//! Z. When the entry asked for does not fit, the chain is helix → ramp →
//! plunge, and the [`Descent`] says which was used and why. Every
//! descending move runs at the cutting feed scaled so its vertical rate is
//! at most the plunge rate (S10).
//!
//! Re-derived for Y-up (ADR-0001) rather than copied: upstream's
//! `helixAngularDirection` returns −1 for an internal climb because its
//! positive rotation is clockwise once its Y-down project space is shown
//! Y-up. Here positive rotation is counter-clockwise, so [`helix_is_ccw`]
//! is true for an internal climb: the negation of upstream's sign.
//! Distances, clearance circles and chords are symmetric under the flip and
//! are ported as they are.
//!
//! Other departures: upstream caches clearance circles in a `WeakMap` keyed
//! by region; here each region of an [`EntryDomain`] computes its circle
//! once. Upstream marks its handoff move for a pocket S-link splice that
//! DreamCarve does not have. Upstream's initial quadtree grid is unbounded
//! for long thin regions; here its cell size grows so the grid stays within
//! [`MAX_CLEARANCE_CELLS`].

use crate::error::ShapeRef;
use crate::toolpath::{Move, Toolpath};
use crate::warning::Warning;
use dc_geometry::{EPS_MM, Point, Point3, Region, Vec2};
use dc_project::{CutDirection, Entry, OperationId};
use std::cell::OnceCell;
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::f64::consts::{PI, TAU};

mod wall;
pub use wall::{along_path, plan_at_wall};

/// The ramp angle a new project's Profile gets, degrees: `init` writes
/// `entry: { "ramp": { "angle_deg": 15.0 } }` (functional spec, capability
/// 16 and open question 6).
pub const INIT_RAMP_ANGLE_DEG: f64 = 15.0;

/// Upstream's default descent angle, degrees (`DEFAULT_ENTRY_RAMP_ANGLE`),
/// which DreamCarve keeps as a new helix's angle; a new ramp's is 15°
/// (functional spec, capability 16).
pub const DEFAULT_RAMP_ANGLE_DEG: f64 = 5.0;
/// Upstream's default helix diameter as a fraction of the tool diameter
/// (`DEFAULT_ENTRY_HELIX_DIAMETER_PERCENT`).
pub const DEFAULT_HELIX_FRACTION: f64 = 0.8;
/// Straight moves per helix revolution, descending and flat.
pub const HELIX_SEGMENTS_PER_REV: usize = 48;
/// Entry moves stay this fraction of the tool diameter inside the
/// clearance region's boundary, so a ramp or helix does not score the wall
/// a finishing ring leaves.
pub const BOUNDARY_SAFETY_FRACTION: f64 = 0.1;
/// The least boundary safety, mm, for very small tools.
pub const MIN_BOUNDARY_SAFETY_MM: f64 = 1e-4;
/// A ramp run is at most this many tool diameters long: a local zigzag
/// near the entry point, not a traverse of the pocket.
pub const MAX_RAMP_RUN_DIAMETERS: f64 = 3.0;
/// The least a zigzag or On line ramp's last leg may fall, mm (#234): five
/// units of the coarsest place a post-processor may write in mm, and two
/// in inches, so its ends never round to one Z. A last leg that would
/// fall less is not cut as it stands (see `EntryPlan::emit_ramp` and
/// [`along_path`]).
pub const RAMP_MIN_LAST_DROP_MM: f64 = 0.05;
/// Most descending moves one descent may have; an entry that needs more
/// falls back to the next in the chain.
pub const MAX_DESCENT_MOVES: usize = 20_000;
/// Ramp directions tried, evenly over a half circle (a line covers both
/// ways).
pub const RAMP_DIRECTIONS: usize = 72;
/// Helix centres tried round the target for the tangent placement.
pub const HELIX_TANGENT_ANGLES: usize = 72;
/// Helix radii tried, each half the one before.
pub const MAX_HELIX_RADIUS_ATTEMPTS: usize = 4;
/// Legal helix centres the reachable placement tries before it gives up on
/// a radius: centres come nearest the target first, so farther ones cannot
/// do better.
pub const MAX_HELIX_ENDPOINT_CANDIDATES: usize = 12;
/// Helix end angles tried per centre by the reachable placement.
pub const HELIX_ENDPOINT_ANGLE_SAMPLES: usize = 24;
/// Most quadtree cells one clearance search visits, and the most cells its
/// first grid may have.
pub const MAX_CLEARANCE_CELLS: usize = 20_000;
/// The entry clearance, mm (#214): a ramp or helix that would start at
/// safe Z starts this far above the stock top instead, and a Pocket's
/// later pass this far above the floor the pass before cleared
/// (PureCutCNC `pocket.ts`, `levelEntryStartZ`, whose clearance is the
/// project's `operationClearanceZ`). The tool gets there by a rapid
/// straight down, which S2 allows for exactly this: ending at least this
/// far above the stock top, or above the lowest Z a cut of the same
/// operation reached through that point. 1 mm, a Drill's usual
/// `retract_mm`: enough for a stock top a little off its stated thickness,
/// little enough that the ramp barely cuts air. A constant, not a setting,
/// until users ask for one.
pub const ENTRY_CLEARANCE_MM: f64 = 1.0;
/// How near, mm, a cut must pass the point a rapid stops over for the
/// floor it reached there to count as cleared (S2, #214): a helix's start
/// lies on the circle the pass before cut flat at its floor, which its 48
/// chords follow within 0.0022 of its radius. The safety pass allows its
/// own rounding and arc tolerances on top.
pub const CLEARED_POINT_TOLERANCE_MM: f64 = 0.01;

/// The Z, mm, a ramp or helix starts at on a pass whose descent would
/// otherwise start at safe Z: [`ENTRY_CLEARANCE_MM`] above `top_z_mm` (the
/// stock top, or a Pocket's previous pass Z), and never above safe Z.
#[must_use]
pub fn descent_start_z_mm(top_z_mm: f64, safe_z_mm: f64) -> f64 {
    (top_z_mm + ENTRY_CLEARANCE_MM).min(safe_z_mm)
}

/// Tolerance of the geometric predicates here (upstream `ENTRY_EPSILON`).
const ENTRY_EPS: f64 = 1e-9;
/// The clearance searches refine cells to this fraction of the tool
/// diameter (upstream `toolDiameter * 0.0025`).
const PRECISION_FRACTION: f64 = 0.0025;
/// The finest clearance search precision, mm.
const MIN_PRECISION_MM: f64 = 1e-4;
/// A helix smaller than this fraction of the tool diameter (or than the
/// radius asked for, if smaller) is not tried.
const MIN_HELIX_RADIUS_FRACTION: f64 = 0.05;
/// A ramp run shorter than this fraction of the tool diameter is no room.
const MIN_RAMP_RUN_FRACTION: f64 = 0.02;
/// How far outside a region a point may be and still count as on it, mm:
/// targets lie on the boundary, and flattening moves it by rounding only.
const ON_BOUNDARY_MM: f64 = 1e-7;

/// Which side of the wall being cut the tool is on, for the helix sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CutSide {
    /// Inside a wall: a pocket, an inside profile, a hole.
    Internal,
    /// Outside a boss: an outside profile.
    External,
}

/// An entry style, as asked for or as used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    /// Straight down at the plunge rate.
    Plunge,
    /// A zigzag ramp.
    Ramp,
    /// A helix.
    Helix,
}

impl EntryKind {
    /// The kind of `entry`.
    #[must_use]
    pub const fn of(entry: &Entry) -> Self {
        match entry {
            Entry::Plunge => Self::Plunge,
            Entry::Ramp { .. } => Self::Ramp,
            Entry::Helix { .. } => Self::Helix,
        }
    }

    /// The name the project file and warnings use: `plunge`, `ramp`,
    /// `helix`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Plunge => "plunge",
            Self::Ramp => "ramp",
            Self::Helix => "helix",
        }
    }
}

/// Why an entry was not used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackReason {
    /// The clearance regions leave no room for it near the target.
    NoRoom,
    /// It fits, but at its angle the depth would take more than
    /// [`MAX_DESCENT_MOVES`] moves.
    TooManyMoves,
    /// A helix whose descending moves the program cannot write: each
    /// would fall less than one unit in its last place, or run less than
    /// √2 units in XY, so its ends round to one Z or one XY (#265).
    TooSmall,
}

impl FallbackReason {
    /// The name warnings use: `no_room`, `too_many_moves`, `too_small`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NoRoom => "no_room",
            Self::TooManyMoves => "too_many_moves",
            Self::TooSmall => "too_small",
        }
    }
}

/// The largest circle inside a clearance region (its pole of
/// inaccessibility), mm, Y-up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClearanceCircle {
    /// Its centre, mm.
    pub centre: Point,
    /// Its radius, mm: the centre's distance to the nearest boundary.
    pub radius_mm: f64,
}

/// The clearance regions an entry may use at one pass: tool-centre
/// regions (mm, Y-up) where the tool centre may be at that Z, inside each
/// outer and outside each hole. Each region's arcs are flattened once at
/// the tolerance given, and its largest clearance circle is found at most
/// once, the first time a helix needs it.
#[derive(Debug, Clone)]
pub struct EntryDomain {
    areas: Vec<Area>,
    precision_mm: f64,
}

impl EntryDomain {
    /// The domain of `regions` for a tool `tool_diameter_mm` across, arcs
    /// flattened within `tolerance_mm`. The searches refine to
    /// `max(1e-4, 0.0025 × diameter)` mm, as upstream's.
    #[must_use]
    pub fn new(regions: &[Region], tool_diameter_mm: f64, tolerance_mm: f64) -> Self {
        Self {
            areas: regions
                .iter()
                .filter_map(|r| Area::new(r, tolerance_mm))
                .collect(),
            precision_mm: MIN_PRECISION_MM.max(tool_diameter_mm * PRECISION_FRACTION),
        }
    }

    /// The largest clearance circle of any region; the first region's on a
    /// tie. `None` when no region has any room.
    #[must_use]
    pub fn largest_clearance_circle(&self) -> Option<ClearanceCircle> {
        let mut best: Option<ClearanceCircle> = None;
        for area in &self.areas {
            if let Some(c) = area.circle(self.precision_mm)
                && best.is_none_or(|b| c.radius_mm > b.radius_mm)
            {
                best = Some(c);
            }
        }
        best
    }

    /// The regions holding `target`, else all of them (upstream's
    /// fallback for a target just off every region).
    fn candidates(&self, target: Point) -> Vec<&Area> {
        let holding: Vec<&Area> = self.areas.iter().filter(|a| a.contains(target)).collect();
        if holding.is_empty() {
            self.areas.iter().collect()
        } else {
            holding
        }
    }
}

/// The tool and cut an entry is for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntryTool {
    /// Tool diameter, mm.
    pub diameter_mm: f64,
    /// Cutting feed, mm/min.
    pub feed_mm_min: f64,
    /// Plunge rate, mm/min: no descent is faster in Z.
    pub plunge_mm_min: f64,
    /// Climb or conventional, for the helix sense.
    pub direction: CutDirection,
    /// Inside a wall or outside a boss, for the helix sense.
    pub side: CutSide,
    /// One unit in the last place the program writes, mm (the project's
    /// decimal places in its display units,
    /// `DecimalPlaces::quantum_mm`). A helix whose descending moves would
    /// fall less than this, or run less than √2 of it in XY, is not cut
    /// (#265).
    pub quantum_mm: f64,
}

/// Requested and used helix diameters, mm (the diameter of the path the
/// tool centre follows).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HelixDiameter {
    /// The diameter asked for: tool diameter × fraction.
    pub requested_mm: f64,
    /// The diameter cut.
    pub used_mm: f64,
}

/// One block's descent, in project space (mm, Y-up, Z-up).
#[derive(Debug, Clone, PartialEq)]
pub struct Descent {
    /// XY where the descent starts, at the start Z: the ramp or helix
    /// start, or the target for a plunge.
    pub from: Point,
    /// The descent moves from `from` at the start Z to the target at the
    /// pass Z, every one `Linear` with a feed. Empty for a plunge, which
    /// the sequencer writes at the plunge rate.
    pub moves: Vec<Move>,
    /// The entry asked for.
    pub requested: EntryKind,
    /// The entry cut.
    pub used: EntryKind,
    /// Why `used` is not `requested`; `None` when it is.
    pub reason: Option<FallbackReason>,
    /// The helix diameters, when a helix is cut.
    pub helix: Option<HelixDiameter>,
}

impl Descent {
    /// True when the entry cut is not the one asked for.
    #[must_use]
    pub fn fell_back(&self) -> bool {
        self.used != self.requested
    }

    /// The helix diameters when the helix cut is smaller than asked.
    #[must_use]
    pub fn diameter_reduced(&self) -> Option<HelixDiameter> {
        self.helix
            .filter(|h| h.used_mm < h.requested_mm - ENTRY_EPS.max(h.requested_mm * ENTRY_EPS))
    }
}

/// A helix: its centre and radius, and the point on it where the descent
/// reaches the pass Z.
#[derive(Debug, Clone, Copy, PartialEq)]
struct HelixPlacement {
    centre: Point,
    endpoint: Point,
    radius_mm: f64,
}

/// A ramp run: the descent zigzags between its ends.
#[derive(Debug, Clone, Copy, PartialEq)]
struct RampPlacement {
    start: Point,
    end: Point,
}

/// An entry placed at one target: where its helix and ramp go, found once
/// and reused for every pass that starts there.
#[derive(Debug, Clone, PartialEq)]
pub struct EntryPlan {
    target: Point,
    tool: EntryTool,
    requested: EntryKind,
    angle_deg: f64,
    requested_helix_mm: f64,
    helix: Option<HelixPlacement>,
    ramp: Option<RampPlacement>,
}

/// Places `entry` at `target` (mm, Y-up) in `domain` for `tool`: a helix
/// (largest-clearance radius bound, tangent placement, then the reachable
/// one nearest the target, halving the radius up to
/// [`MAX_HELIX_RADIUS_ATTEMPTS`] times) and, for a helix's fallback or a
/// ramp, the zigzag run (the longest chord through the target over
/// [`RAMP_DIRECTIONS`] directions, shrunk by the boundary safety, at most
/// [`MAX_RAMP_RUN_DIAMETERS`] diameters, centred on the target). A plunge
/// places nothing. Angles and the helix fraction are used as given;
/// callers validate them.
#[must_use]
pub fn plan(domain: &EntryDomain, entry: &Entry, target: Point, tool: &EntryTool) -> EntryPlan {
    plan_with(domain, entry, target, tool, place_ramp)
}

/// How a ramp is placed: in `domain`, through `target`, for a tool of the
/// diameter given.
type RampPlacer = fn(&EntryDomain, Point, f64) -> Option<RampPlacement>;

/// [`plan`] with the ramp placed by `place`.
fn plan_with(
    domain: &EntryDomain,
    entry: &Entry,
    target: Point,
    tool: &EntryTool,
    place: RampPlacer,
) -> EntryPlan {
    let (angle_deg, fraction) = match *entry {
        Entry::Plunge => (0.0, 0.0),
        Entry::Ramp { angle_deg } => (angle_deg, 0.0),
        Entry::Helix {
            diameter_fraction,
            angle_deg,
        } => (angle_deg, diameter_fraction),
    };
    let requested = EntryKind::of(entry);
    let requested_helix_mm = tool.diameter_mm * fraction;
    let helix = (requested == EntryKind::Helix)
        .then(|| place_helix(domain, target, requested_helix_mm / 2.0, tool.diameter_mm))
        .flatten();
    let ramp = (requested != EntryKind::Plunge)
        .then(|| place(domain, target, tool.diameter_mm))
        .flatten();
    EntryPlan {
        target,
        tool: *tool,
        requested,
        angle_deg,
        requested_helix_mm,
        helix,
        ramp,
    }
}

/// [`plan`] then [`EntryPlan::descend`], for one descent.
#[must_use]
pub fn synthesize(
    domain: &EntryDomain,
    entry: &Entry,
    target: Point,
    start_z_mm: f64,
    z_mm: f64,
    tool: &EntryTool,
) -> Descent {
    plan(domain, entry, target, tool).descend(start_z_mm, z_mm)
}

impl EntryPlan {
    /// The descent from `start_z_mm` down to the target at `z_mm`: the
    /// helix (one flat revolution at `z_mm`, then a straight handoff to
    /// the target if the helix does not end on it), else the ramp (zigzag,
    /// the last run cut short at `z_mm`, then the handoff), else a plunge.
    /// A placed helix or ramp that would need more than
    /// [`MAX_DESCENT_MOVES`] descending moves falls through too, and so
    /// does a helix whose chords the program cannot write
    /// ([`FallbackReason::TooSmall`]). With no depth to descend the
    /// descent is empty.
    #[must_use]
    pub fn descend(&self, start_z_mm: f64, z_mm: f64) -> Descent {
        self.descend_with(start_z_mm, z_mm, None)
    }

    /// [`EntryPlan::descend`]; with `skip_helix` set, a requested helix
    /// is not tried and the chain starts at the ramp, the helix's fallback
    /// reason being the one given.
    fn descend_with(
        &self,
        start_z_mm: f64,
        z_mm: f64,
        skip_helix: Option<FallbackReason>,
    ) -> Descent {
        let depth_mm = start_z_mm - z_mm;
        let mut descent = Descent {
            from: self.target,
            moves: Vec::new(),
            requested: self.requested,
            used: self.requested,
            reason: None,
            helix: None,
        };
        if self.requested == EntryKind::Plunge || depth_mm <= EPS_MM || depth_mm.is_nan() {
            return descent;
        }
        let mut reason = None;
        if self.requested == EntryKind::Helix && skip_helix.is_some() {
            reason = skip_helix;
        } else if self.requested == EntryKind::Helix {
            match self.helix {
                Some(h) => match helix_move_count(depth_mm, h.radius_mm, self.angle_deg) {
                    Some(n)
                        if !helix_writable(
                            depth_mm,
                            h.radius_mm,
                            self.angle_deg,
                            n,
                            self.tool.quantum_mm,
                        ) =>
                    {
                        reason = Some(FallbackReason::TooSmall);
                    }
                    Some(n) => {
                        let (from, moves) = self.emit_helix(&h, n, start_z_mm, z_mm);
                        descent.from = from;
                        descent.moves = moves;
                        descent.helix = Some(HelixDiameter {
                            requested_mm: self.requested_helix_mm,
                            used_mm: 2.0 * h.radius_mm,
                        });
                        return descent;
                    }
                    None => reason = Some(FallbackReason::TooManyMoves),
                },
                None => reason = Some(FallbackReason::NoRoom),
            }
        }
        match self.ramp {
            Some(r) => match ramp_move_count(depth_mm, &r, self.angle_deg) {
                Some(n) => {
                    descent.from = r.start;
                    descent.moves = self.emit_ramp(&r, n, start_z_mm, z_mm);
                    descent.used = EntryKind::Ramp;
                    descent.reason = reason;
                    return descent;
                }
                None => reason = reason.or(Some(FallbackReason::TooManyMoves)),
            },
            None => reason = reason.or(Some(FallbackReason::NoRoom)),
        }
        descent.used = EntryKind::Plunge;
        descent.reason = reason;
        descent
    }

    /// A move to `to` from `from` at the plunge-limited feed.
    fn cut(&self, from: Point3, to: Point3) -> Move {
        Move::linear(to, move_feed_mm_min(&self.tool, from, to))
    }

    /// The helix of upstream `emitHelix`: `n` descending chords, Z linear
    /// in the angle, ending on `h.endpoint` at `z_mm`; a flat revolution;
    /// the handoff. Returns the start XY and the moves.
    fn emit_helix(
        &self,
        h: &HelixPlacement,
        n: usize,
        start_z_mm: f64,
        z_mm: f64,
    ) -> (Point, Vec<Move>) {
        let sense = if helix_is_ccw(self.tool.direction, self.tool.side) {
            1.0
        } else {
            -1.0
        };
        let depth_mm = start_z_mm - z_mm;
        let pitch_mm = helix_pitch_mm(2.0 * h.radius_mm, self.angle_deg);
        let turns = depth_mm / pitch_mm;
        let end_angle = (h.endpoint - h.centre).y.atan2((h.endpoint - h.centre).x);
        let start_angle = end_angle - sense * turns * TAU;
        let at = |angle: f64| h.centre + Vec2::new(angle.cos(), angle.sin()) * h.radius_mm;
        let from = at(start_angle);
        let mut moves = Vec::with_capacity(n + HELIX_SEGMENTS_PER_REV + 1);
        let mut current = Point3::new(from.x, from.y, start_z_mm);
        let steps = n as f64;
        for k in 1..=n {
            let next = if k == n {
                Point3::new(h.endpoint.x, h.endpoint.y, z_mm)
            } else {
                let t = k as f64 / steps;
                let p = at(start_angle + sense * turns * TAU * t);
                Point3::new(p.x, p.y, start_z_mm - depth_mm * t)
            };
            moves.push(self.cut(current, next));
            current = next;
        }
        // One flat revolution flattens the spiral floor.
        let per_rev = HELIX_SEGMENTS_PER_REV as f64;
        for k in 1..=HELIX_SEGMENTS_PER_REV {
            let p = if k == HELIX_SEGMENTS_PER_REV {
                h.endpoint
            } else {
                at(end_angle + sense * TAU * k as f64 / per_rev)
            };
            let next = Point3::new(p.x, p.y, z_mm);
            moves.push(Move::linear(next, self.tool.feed_mm_min));
            current = next;
        }
        if current.xy().distance_to(self.target) > EPS_MM {
            moves.push(Move::linear(
                Point3::new(self.target.x, self.target.y, z_mm),
                self.tool.feed_mm_min,
            ));
        }
        (from, moves)
    }

    /// The zigzag of upstream `emitRamp`: `n` runs alternating between the
    /// placement's ends, each falling `run × tan(angle)`, the last cut
    /// short at `z_mm`; then the handoff to the target. When that last run
    /// would fall less than [`RAMP_MIN_LAST_DROP_MM`], the last two runs
    /// share their depth instead: each falls half of it at the angle, the
    /// first cut short and the second back to where the first began. A
    /// ramp of one such run runs the whole run, shallower. So no run is a
    /// fragment, and none is steeper than asked. (Changed in #234: a depth
    /// a sliver over a whole number of runs left a last run a few
    /// micrometres long, whose ends the program writes at one Z, a level
    /// move at the ramp's plunge-limited feed, which S3 refuses.)
    fn emit_ramp(&self, r: &RampPlacement, n: usize, start_z_mm: f64, z_mm: f64) -> Vec<Move> {
        let drop_mm = r.start.distance_to(r.end) * self.angle_deg.to_radians().tan();
        let rest_mm = start_z_mm - z_mm - n.saturating_sub(1) as f64 * drop_mm;
        let short_last = rest_mm < RAMP_MIN_LAST_DROP_MM;
        let mut moves = Vec::with_capacity(n + 1);
        let mut current = Point3::new(r.start.x, r.start.y, start_z_mm);
        let mut towards = r.end;
        for k in 1..=n {
            let next = if k == n {
                let ratio = if short_last {
                    1.0
                } else {
                    ((current.z - z_mm) / drop_mm).min(1.0)
                };
                let p = current.xy() + (towards - current.xy()) * ratio;
                Point3::new(p.x, p.y, z_mm)
            } else if short_last && k + 1 == n {
                let ratio = (drop_mm + rest_mm) / (2.0 * drop_mm);
                let p = current.xy() + (towards - current.xy()) * ratio;
                Point3::new(p.x, p.y, current.z - drop_mm * ratio)
            } else {
                Point3::new(towards.x, towards.y, current.z - drop_mm)
            };
            moves.push(self.cut(current, next));
            current = next;
            towards = if towards.distance_to(r.end) <= EPS_MM {
                r.start
            } else {
                r.end
            };
        }
        if current.xy().distance_to(self.target) > EPS_MM {
            moves.push(Move::linear(
                Point3::new(self.target.x, self.target.y, z_mm),
                self.tool.feed_mm_min,
            ));
        }
        moves
    }
}

/// Helix pitch, mm per revolution, for a path `path_diameter_mm` across
/// descending at `angle_deg`: `π · d · tan(angle)` (upstream
/// `pitchFromRampAngle`). 0 for a non-positive diameter or angle.
#[must_use]
pub fn helix_pitch_mm(path_diameter_mm: f64, angle_deg: f64) -> f64 {
    if !(path_diameter_mm > 0.0 && angle_deg > 0.0) {
        return 0.0;
    }
    PI * path_diameter_mm * angle_deg.to_radians().tan()
}

/// The factor (at most 1) the cutting feed is scaled by on a descent at
/// `angle_deg` so its vertical component is at most the plunge rate:
/// `min(1, plunge / (feed · sin(angle)))` (S10; upstream
/// `plungeLimitedFeedScale`). 1 for non-positive rates or angle.
#[must_use]
pub fn plunge_limited_feed_scale(feed_mm_min: f64, plunge_mm_min: f64, angle_deg: f64) -> f64 {
    if !(feed_mm_min > 0.0 && plunge_mm_min > 0.0) {
        return 1.0;
    }
    let vertical = angle_deg.to_radians().sin();
    if vertical <= 0.0 || vertical.is_nan() {
        return 1.0;
    }
    (plunge_mm_min / (feed_mm_min * vertical)).min(1.0)
}

/// The feed, mm/min, of a straight move `from → to` for `tool`
/// ([`plunge_limited_feed_mm_min`]).
fn move_feed_mm_min(tool: &EntryTool, from: Point3, to: Point3) -> f64 {
    plunge_limited_feed_mm_min(tool.feed_mm_min, tool.plunge_mm_min, from, to)
}

/// The feed, mm/min, of a straight move `from → to`: the cutting feed
/// `feed_mm_min`, scaled down on a descending move so its vertical
/// component is at most `plunge_mm_min`, `min(feed, plunge × length /
/// drop)` (S10; upstream `plungeLimitedMoveFeedScale`, applied per move
/// because a chord of a helix is a little steeper than the helix). Entry
/// descents and smooth tabs' down slopes both use it (#157).
pub(crate) fn plunge_limited_feed_mm_min(
    feed_mm_min: f64,
    plunge_mm_min: f64,
    from: Point3,
    to: Point3,
) -> f64 {
    let (dx, dy, dz) = (to.x - from.x, to.y - from.y, from.z - to.z);
    let length = (dx * dx + dy * dy + dz * dz).sqrt();
    if !(length > ENTRY_EPS && dz > ENTRY_EPS) {
        return feed_mm_min;
    }
    feed_mm_min.min(plunge_mm_min * length / dz)
}

/// True when a helix for `direction` on `side` turns counter-clockwise
/// (viewed from +Z, Y-up). The spindle turns clockwise, so climb keeps
/// the material on the right of travel: a tool inside a wall (pocket,
/// inside profile, hole) climbs counter-clockwise, and one outside a boss
/// (outside profile) clockwise, the same senses as the rings and laps it
/// enters. Conventional is the reverse. Upstream's
/// `helixAngularDirection` returns the negation, being Y-down.
#[must_use]
pub fn helix_is_ccw(direction: CutDirection, side: CutSide) -> bool {
    let climb_is_ccw = match side {
        CutSide::Internal => true,
        CutSide::External => false,
    };
    match direction {
        CutDirection::Climb => climb_is_ccw,
        CutDirection::Conventional => !climb_is_ccw,
    }
}

/// How far, mm, entry moves stay inside the clearance boundary:
/// `max(1e-4, 0.1 × diameter)` (upstream `entryBoundarySafety`).
#[must_use]
pub fn boundary_safety_mm(tool_diameter_mm: f64) -> f64 {
    MIN_BOUNDARY_SAFETY_MM.max(tool_diameter_mm * BOUNDARY_SAFETY_FRACTION)
}

/// Descending moves of a helix of `radius_mm` over `depth_mm`, or `None`
/// over [`MAX_DESCENT_MOVES`] (upstream `helixDescentMoveCount`).
fn helix_move_count(depth_mm: f64, radius_mm: f64, angle_deg: f64) -> Option<usize> {
    let pitch_mm = helix_pitch_mm(2.0 * radius_mm, angle_deg);
    if pitch_mm <= ENTRY_EPS || pitch_mm.is_nan() {
        return None;
    }
    count_within_cap(depth_mm.max(0.0) / pitch_mm * HELIX_SEGMENTS_PER_REV as f64)
}

/// Whether a helix of `radius_mm` at `angle_deg` cut in `n` chords over
/// `depth_mm` can be written by a program whose last place is
/// `quantum_mm`: every chord falls at least one unit and runs at least
/// √2 units in XY (#265). Two numbers a unit or more apart round apart, so
/// each chord's ends are written at two Z values and two XY points, and
/// the move keeps its slope: written level, it would run at the
/// plunge-limited feed, which is neither of the tool's rates (S3);
/// written straight down, faster than the plunge rate (S10). A helix that
/// small removes nothing a ramp or plunge would not.
fn helix_writable(
    depth_mm: f64,
    radius_mm: f64,
    angle_deg: f64,
    n: usize,
    quantum_mm: f64,
) -> bool {
    let pitch_mm = helix_pitch_mm(2.0 * radius_mm, angle_deg);
    let steps = n as f64;
    let drop_mm = depth_mm / steps;
    let turn_rad = depth_mm / pitch_mm * TAU / steps;
    let run_mm = 2.0 * radius_mm * (turn_rad / 2.0).sin();
    drop_mm >= quantum_mm && run_mm >= std::f64::consts::SQRT_2 * quantum_mm
}

/// Runs of the ramp `r` over `depth_mm`, or `None` over
/// [`MAX_DESCENT_MOVES`] (upstream `rampDescentMoveCount`).
fn ramp_move_count(depth_mm: f64, r: &RampPlacement, angle_deg: f64) -> Option<usize> {
    let drop_mm = r.start.distance_to(r.end) * angle_deg.to_radians().tan();
    if drop_mm <= ENTRY_EPS || drop_mm.is_nan() {
        return None;
    }
    count_within_cap(depth_mm.max(0.0) / drop_mm)
}

/// `ceil(moves)`, at least 1, when it is at most [`MAX_DESCENT_MOVES`].
fn count_within_cap(moves: f64) -> Option<usize> {
    let n = moves.ceil().max(1.0);
    // Checked against the cap first, so the cast is of a whole number in
    // 1..=20 000.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (n <= MAX_DESCENT_MOVES as f64).then_some(n as usize)
}

/// Upstream `findHelixPlacement`: per candidate region, the radius
/// `min(requested, D/2, clearance − safety)`, tried tangent then
/// reachable, halving.
fn place_helix(
    domain: &EntryDomain,
    target: Point,
    requested_radius_mm: f64,
    tool_diameter_mm: f64,
) -> Option<HelixPlacement> {
    if requested_radius_mm <= ENTRY_EPS || requested_radius_mm.is_nan() {
        return None;
    }
    let precision = domain.precision_mm;
    let safety = boundary_safety_mm(tool_diameter_mm);
    let no_core_mm = tool_diameter_mm / 2.0;
    let min_useful = requested_radius_mm.min(tool_diameter_mm * MIN_HELIX_RADIUS_FRACTION);
    for area in domain.candidates(target) {
        let Some(circle) = area.circle(precision) else {
            continue;
        };
        let mut radius = requested_radius_mm
            .min(no_core_mm)
            .min((circle.radius_mm - safety).max(0.0));
        for _ in 0..MAX_HELIX_RADIUS_ATTEMPTS {
            if radius < precision.max(min_useful) {
                break;
            }
            if let Some(p) = tangent_helix(area, target, radius, safety) {
                return Some(p);
            }
            if let Some(p) = reachable_helix(area, target, radius, safety, precision) {
                return Some(p);
            }
            radius *= 0.5;
        }
    }
    None
}

/// Upstream `findTargetTouchingHelix`: a helix through the target whose
/// circle keeps `safety` from the boundary, its centre tried at
/// [`HELIX_TANGENT_ANGLES`] angles round the target.
fn tangent_helix(area: &Area, target: Point, radius: f64, safety: f64) -> Option<HelixPlacement> {
    (0..HELIX_TANGENT_ANGLES).find_map(|i| {
        let angle = TAU * i as f64 / HELIX_TANGENT_ANGLES as f64;
        let centre = target + Vec2::new(-angle.cos(), -angle.sin()) * radius;
        (area.signed_distance(centre) >= radius + safety - ENTRY_EPS).then_some(HelixPlacement {
            centre,
            endpoint: target,
            radius_mm: radius,
        })
    })
}

/// Upstream `findReachableHelixPlacement`: a quadtree search for a helix
/// centre with `radius + safety` clearance, nearest the target first, whose
/// circle has a point joined to the target by a segment inside the region.
fn reachable_helix(
    area: &Area,
    target: Point,
    radius: f64,
    safety: f64,
    precision: f64,
) -> Option<HelixPlacement> {
    let (cell_mm, centres) = area.grid()?;
    let mut cells: BinaryHeap<Nearest> = centres
        .into_iter()
        .map(|c| Nearest(area.cell(c, cell_mm / 2.0, target)))
        .collect();
    let required = radius + safety;
    // A coarse cell's centre can be legal while a closer legal centre lies
    // inside it: refuse placements above this size so the search refines
    // round the target first.
    let accept_mm = precision.max(radius / 2.0);
    let mut visited = 0;
    let mut attempts = 0;
    while visited < MAX_CLEARANCE_CELLS {
        let Some(Nearest(cell)) = cells.pop() else {
            break;
        };
        visited += 1;
        if cell.max_distance_mm < required - ENTRY_EPS {
            continue;
        }
        if cell.distance_mm >= required - ENTRY_EPS && cell.half_mm <= accept_mm + ENTRY_EPS {
            if attempts >= MAX_HELIX_ENDPOINT_CANDIDATES {
                return None;
            }
            attempts += 1;
            if let Some(endpoint) = safe_endpoint(area, cell.centre, radius, target) {
                return Some(HelixPlacement {
                    centre: cell.centre,
                    endpoint,
                    radius_mm: radius,
                });
            }
        }
        if cell.half_mm <= precision / 2.0 {
            continue;
        }
        for child in cell.children() {
            cells.push(Nearest(area.cell(child.0, child.1, target)));
        }
    }
    None
}

/// Upstream `findSafeHelixEndpoint`: the point of the circle nearest the
/// target's direction, then alternately either side of it, whose segment
/// to the target stays inside the region. Only the piece that reaches the
/// target may leave the flattened rings, and by no more than the target
/// itself does, at most the target slack (#213): a target on a round
/// boundary lies on the arc, outside its chords. (Changed in #272: the
/// slack held along the whole handoff let one to a target on an island's
/// chord run 0.005 mm inside the chords, 0.015 mm inside the island's
/// tool-centre circle, past the flatten tolerance S9 allows.)
fn safe_endpoint(area: &Area, centre: Point, radius: f64, target: Point) -> Option<Point> {
    let outside_mm = (-area.signed_distance(target)).clamp(0.0, area.tolerance_mm);
    let end_slack_mm = outside_mm + ON_BOUNDARY_MM;
    let to_target = target - centre;
    let preferred = to_target.y.atan2(to_target.x);
    let step = TAU / HELIX_ENDPOINT_ANGLE_SAMPLES as f64;
    (0..HELIX_ENDPOINT_ANGLE_SAMPLES).find_map(|i| {
        // 0, +1, −1, +2, −2, … steps from the preferred angle.
        let offset = i.div_ceil(2) as f64;
        let offset = if i % 2 == 0 { -offset } else { offset };
        let angle = preferred + offset * step;
        let endpoint = centre + Vec2::new(angle.cos(), angle.sin()) * radius;
        area.segment_inside_ending(endpoint, target, ON_BOUNDARY_MM, end_slack_mm)
            .then_some(endpoint)
    })
}

/// Upstream `findRampPlacement` (pocket zigzag): over [`RAMP_DIRECTIONS`]
/// directions through the target, the chord interval holding it, shrunk by
/// the boundary safety at both ends; a run of at most
/// [`MAX_RAMP_RUN_DIAMETERS`] diameters centred on the target and slid to
/// stay inside. The longest run wins, then the most room, then the first
/// direction.
fn place_ramp(domain: &EntryDomain, target: Point, tool_diameter_mm: f64) -> Option<RampPlacement> {
    let safety = boundary_safety_mm(tool_diameter_mm);
    let max_run = tool_diameter_mm * MAX_RAMP_RUN_DIAMETERS;
    let min_run = ENTRY_EPS.max(tool_diameter_mm * MIN_RAMP_RUN_FRACTION);
    let mut best: Option<(RampPlacement, f64, f64)> = None;
    for area in domain.candidates(target) {
        for i in 0..RAMP_DIRECTIONS {
            let angle = PI * i as f64 / RAMP_DIRECTIONS as f64;
            let direction = Vec2::new(angle.cos(), angle.sin());
            let Some((min, max)) = area.line_interval(target, direction, area.target_slack_mm())
            else {
                continue;
            };
            let low = min + safety;
            let high = max - safety;
            let room = high - low;
            if room <= min_run || room.is_nan() {
                continue;
            }
            let start = low.max(-max_run / 2.0);
            let end = high.min(start + max_run);
            let start = low.max(end - max_run);
            let length = end - start;
            if length <= min_run || length.is_nan() {
                continue;
            }
            if let Some((_, best_length, best_room)) = best {
                match length.total_cmp(&best_length) {
                    Ordering::Less => continue,
                    Ordering::Equal if room <= best_room => continue,
                    _ => {}
                }
            }
            best = Some((
                RampPlacement {
                    start: target + direction * start,
                    end: target + direction * end,
                },
                length,
                room,
            ));
        }
    }
    best.map(|(placement, _, _)| placement)
}

/// One clearance region, flattened: its outer and holes as closed point
/// rings, the outer's bounds, and its clearance circle once found.
#[derive(Debug, Clone)]
struct Area {
    /// The outer first, then the holes; each closed, its first point not
    /// repeated.
    rings: Vec<Vec<Point>>,
    min: Point,
    max: Point,
    /// The tolerance its arcs were flattened within, mm: a point on an
    /// arc of the region may lie this far outside the flattened rings.
    tolerance_mm: f64,
    circle: OnceCell<Option<ClearanceCircle>>,
}

/// One quadtree cell of a clearance search.
#[derive(Debug, Clone, Copy)]
struct Cell {
    centre: Point,
    half_mm: f64,
    /// Signed distance of the centre to the boundary: positive inside.
    distance_mm: f64,
    /// The most any point of the cell can be from the boundary.
    max_distance_mm: f64,
    /// Distance of the centre to the target, for the target-first search.
    to_target_mm: f64,
}

impl Cell {
    /// The four quarter cells, as (centre, half size).
    fn children(&self) -> [(Point, f64); 4] {
        let h = self.half_mm / 2.0;
        let c = self.centre;
        [
            (Point::new(c.x - h, c.y - h), h),
            (Point::new(c.x + h, c.y - h), h),
            (Point::new(c.x - h, c.y + h), h),
            (Point::new(c.x + h, c.y + h), h),
        ]
    }
}

/// Roomiest first: the largest possible clearance pops first.
struct Roomiest(Cell);

impl Ord for Roomiest {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.max_distance_mm.total_cmp(&other.0.max_distance_mm)
    }
}
impl PartialOrd for Roomiest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Roomiest {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Roomiest {}

/// Nearest the target first, then roomiest (upstream's lexicographic
/// order on exact keys, so the heap's order is a strict weak order).
struct Nearest(Cell);

impl Ord for Nearest {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .0
            .to_target_mm
            .total_cmp(&self.0.to_target_mm)
            .then(self.0.max_distance_mm.total_cmp(&other.0.max_distance_mm))
    }
}
impl PartialOrd for Nearest {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl PartialEq for Nearest {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}
impl Eq for Nearest {}

impl Area {
    fn new(region: &Region, tolerance_mm: f64) -> Option<Self> {
        let ring = |c: &dc_geometry::Contour| -> Vec<Point> {
            let flat = c.polyline().with_arcs_flattened(tolerance_mm);
            let mut points: Vec<Point> = flat.vertices().iter().map(|v| v.point()).collect();
            if points.len() > 1
                && let (Some(first), Some(last)) = (points.first(), points.last())
                && first.distance_to(*last) <= EPS_MM
            {
                points.pop();
            }
            points
        };
        let outer = ring(region.outer());
        if outer.len() < 3 {
            return None;
        }
        let first = *outer.first()?;
        let (min, max) = outer.iter().fold((first, first), |(lo, hi), p| {
            (
                Point::new(lo.x.min(p.x), lo.y.min(p.y)),
                Point::new(hi.x.max(p.x), hi.y.max(p.y)),
            )
        });
        let mut rings = vec![outer];
        rings.extend(region.holes().iter().map(ring).filter(|r| r.len() >= 3));
        Some(Self {
            rings,
            min,
            max,
            tolerance_mm,
            circle: OnceCell::new(),
        })
    }

    /// Every boundary edge, outer and holes.
    fn edges(&self) -> impl Iterator<Item = (Point, Point)> + '_ {
        self.rings.iter().flat_map(|ring| {
            ring.iter()
                .zip(ring.iter().cycle().skip(1))
                .map(|(a, b)| (*a, *b))
        })
    }

    /// Distance from `p` to the nearest boundary, positive inside the
    /// region (in the outer, in no hole), negative outside.
    fn signed_distance(&self, p: Point) -> f64 {
        let mut nearest_sq = f64::INFINITY;
        let mut inside = false;
        for (a, b) in self.edges() {
            nearest_sq = nearest_sq.min(segment_distance_sq(p, a, b));
            if (a.y > p.y) != (b.y > p.y) && p.x < (b.x - a.x) * (p.y - a.y) / (b.y - a.y) + a.x {
                inside = !inside;
            }
        }
        let distance = nearest_sq.sqrt();
        if inside { distance } else { -distance }
    }

    /// How far outside the flattened rings a target may lie and still be
    /// on the region, mm (#213): a ring's start on a round corner of the
    /// tool-centre region lies on the arc, outside the chords that stand
    /// for it here by up to the flattening tolerance.
    fn target_slack_mm(&self) -> f64 {
        self.tolerance_mm + ON_BOUNDARY_MM
    }

    /// True when `p` is in the region or on its boundary.
    fn contains(&self, p: Point) -> bool {
        self.contains_within(p, ON_BOUNDARY_MM)
    }

    /// True when `p` is in the region or at most `slack_mm` outside it.
    fn contains_within(&self, p: Point, slack_mm: f64) -> bool {
        self.signed_distance(p) >= -slack_mm
    }

    /// The cell centred at `centre`, `half_mm` either way.
    fn cell(&self, centre: Point, half_mm: f64, target: Point) -> Cell {
        let distance_mm = self.signed_distance(centre);
        Cell {
            centre,
            half_mm,
            distance_mm,
            max_distance_mm: distance_mm + half_mm * std::f64::consts::SQRT_2,
            to_target_mm: centre.distance_to(target),
        }
    }

    /// The first grid of a quadtree search over the outer's bounds:
    /// square cells the shorter side across, enlarged when a long thin
    /// region would need more than [`MAX_CLEARANCE_CELLS`]; their size and
    /// centres. `None` for a degenerate outer.
    fn grid(&self) -> Option<(f64, Vec<Point>)> {
        let (w, h) = (self.max.x - self.min.x, self.max.y - self.min.y);
        let cell = w.min(h).max(w.max(h) / MAX_CLEARANCE_CELLS as f64);
        if cell <= 0.0 || cell.is_nan() {
            return None;
        }
        let mut centres = Vec::new();
        let mut x = self.min.x;
        while x < self.max.x && centres.len() < 2 * MAX_CLEARANCE_CELLS {
            let mut y = self.min.y;
            while y < self.max.y && centres.len() < 2 * MAX_CLEARANCE_CELLS {
                centres.push(Point::new(x + cell / 2.0, y + cell / 2.0));
                y += cell;
            }
            x += cell;
        }
        Some((cell, centres))
    }

    /// Upstream `findRegionClearanceCircle`, computed once at `precision`
    /// (the domain's precision never changes).
    fn circle(&self, precision: f64) -> Option<ClearanceCircle> {
        *self.circle.get_or_init(|| self.compute_circle(precision))
    }

    /// Upstream `computeRegionClearanceCircle`: the pole of inaccessibility
    /// by a best-first quadtree search, roomiest cell first, seeded with the
    /// centroid and the bounds' centre.
    fn compute_circle(&self, precision: f64) -> Option<ClearanceCircle> {
        let (cell_mm, centres) = self.grid()?;
        let nowhere = Point::new(0.0, 0.0);
        let mut cells: BinaryHeap<Roomiest> = centres
            .into_iter()
            .map(|c| Roomiest(self.cell(c, cell_mm / 2.0, nowhere)))
            .collect();
        let mut best = self.cell(self.centroid(), 0.0, nowhere);
        let middle = Point::new(
            (self.min.x + self.max.x) / 2.0,
            (self.min.y + self.max.y) / 2.0,
        );
        let middle = self.cell(middle, 0.0, nowhere);
        if middle.distance_mm > best.distance_mm {
            best = middle;
        }
        let mut visited = 0;
        while visited < MAX_CLEARANCE_CELLS {
            let Some(Roomiest(cell)) = cells.pop() else {
                break;
            };
            visited += 1;
            if cell.distance_mm > best.distance_mm {
                best = cell;
            }
            if cell.max_distance_mm - best.distance_mm <= precision
                || cell.half_mm <= precision / 2.0
            {
                continue;
            }
            for child in cell.children() {
                cells.push(Roomiest(self.cell(child.0, child.1, nowhere)));
            }
        }
        (best.distance_mm > ENTRY_EPS).then_some(ClearanceCircle {
            centre: best.centre,
            radius_mm: best.distance_mm,
        })
    }

    /// The area centroid of the outer, or its first point when it has no
    /// area.
    fn centroid(&self) -> Point {
        let Some(outer) = self.rings.first() else {
            return self.min;
        };
        let (mut twice_area, mut x, mut y) = (0.0, 0.0, 0.0);
        for (a, b) in outer.iter().zip(outer.iter().cycle().skip(1)) {
            let term = a.x * b.y - b.x * a.y;
            twice_area += term;
            x += (a.x + b.x) * term;
            y += (a.y + b.y) * term;
        }
        if twice_area.abs() <= ENTRY_EPS {
            return outer.first().copied().unwrap_or(self.min);
        }
        Point::new(x / (3.0 * twice_area), y / (3.0 * twice_area))
    }

    /// Upstream `segmentInsideRegion`: the segment, split where it crosses
    /// any boundary edge, has every piece's midpoint in the region (or at
    /// most `slack_mm` outside it).
    fn segment_inside(&self, from: Point, to: Point, slack_mm: f64) -> bool {
        self.segment_inside_ending(from, to, slack_mm, slack_mm)
    }

    /// [`Area::segment_inside`], the last piece (the one ending at `to`)
    /// held to `end_slack_mm` and every other piece to `slack_mm`.
    fn segment_inside_ending(
        &self,
        from: Point,
        to: Point,
        slack_mm: f64,
        end_slack_mm: f64,
    ) -> bool {
        let mut ts = vec![0.0, 1.0];
        ts.extend(
            self.edges()
                .filter_map(|(a, b)| crossing_parameter(from, to, a, b)),
        );
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
        let last = ts.len().saturating_sub(2);
        ts.windows(2).enumerate().all(|(i, w)| match w {
            [a, b] => self.contains_within(
                from + (to - from) * ((a + b) / 2.0),
                if i == last { end_slack_mm } else { slack_mm },
            ),
            _ => true,
        })
    }

    /// Upstream `lineIntervalInsideRegion`: along the line `origin + t ·
    /// direction`, the longest interval `[min, max]` between consecutive
    /// boundary crossings that holds `t = 0`, or starts at most
    /// `origin_slack_mm` past it (an origin just outside the flattened
    /// rings, #213), and lies in the region.
    fn line_interval(
        &self,
        origin: Point,
        direction: Vec2,
        origin_slack_mm: f64,
    ) -> Option<(f64, f64)> {
        let mut ts: Vec<f64> = self
            .edges()
            .filter_map(|(a, b)| {
                let edge = b - a;
                let denominator = direction.cross(edge);
                if denominator.abs() <= ENTRY_EPS {
                    return None;
                }
                let offset = a - origin;
                let along_line = offset.cross(edge) / denominator;
                let along_edge = offset.cross(direction) / denominator;
                (-ENTRY_EPS..=1.0 + ENTRY_EPS)
                    .contains(&along_edge)
                    .then_some(along_line)
            })
            .collect();
        ts.sort_by(f64::total_cmp);
        ts.dedup_by(|a, b| (*a - *b).abs() <= 1e-12);
        let mut best: Option<(f64, f64)> = None;
        for w in ts.windows(2) {
            let [min, max] = *w else { continue };
            if min > origin_slack_mm.max(ENTRY_EPS) || max < -ENTRY_EPS {
                continue;
            }
            if !self.contains(origin + direction * ((min + max) / 2.0)) {
                continue;
            }
            if best.is_none_or(|(lo, hi)| max - min > hi - lo) {
                best = Some((min, max));
            }
        }
        best
    }
}

/// Squared distance from `p` to the segment `a → b`.
fn segment_distance_sq(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let length_sq = ab.dot(ab);
    let t = if length_sq > 0.0 {
        ((p - a).dot(ab) / length_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let q = a + ab * t;
    let d = p - q;
    d.dot(d)
}

/// Upstream `segmentIntersectionParameter`: where along `a → b` (strictly
/// inside it) the segment crosses `c → d`.
fn crossing_parameter(a: Point, b: Point, c: Point, d: Point) -> Option<f64> {
    let ab = b - a;
    let cd = d - c;
    let denominator = ab.cross(cd);
    if denominator.abs() <= ENTRY_EPS {
        return None;
    }
    let offset = c - a;
    let t = offset.cross(cd) / denominator;
    let u = offset.cross(ab) / denominator;
    (t > ENTRY_EPS && t < 1.0 - ENTRY_EPS && (-ENTRY_EPS..=1.0 + ENTRY_EPS).contains(&u))
        .then_some(t)
}

/// True when `mv`, from `from`, is a descent move: straight, falling, and
/// moving in XY (a ramp or helix chord).
pub(crate) fn is_descent_move(from: Point3, mv: &Move) -> bool {
    matches!(mv.kind, crate::toolpath::MoveKind::Linear)
        && mv.to.z < from.z - EPS_MM
        && from.xy().distance_to(mv.to.xy()) > EPS_MM
}

/// S9 for descents: the index of the first descent move of `toolpath`
/// ([`is_descent_move`]) whose tool centre leaves `regions`, the
/// tool-centre regions it may use, by more than `tolerance_mm`. A tool
/// centre inside the tool-centre region keeps the whole tool in the
/// material, so this is the footprint check on descents, done on the
/// segments instead of the union of their swept capsules: a helix is
/// hundreds of overlapping chords, and their union is slow to build.
pub(crate) fn first_descent_outside(
    toolpath: &Toolpath,
    regions: &[Region],
    tolerance_mm: f64,
) -> Option<usize> {
    let areas: Vec<Area> = regions
        .iter()
        .filter_map(|r| Area::new(r, tolerance_mm))
        .collect();
    let slack_mm = tolerance_mm + EPS_MM;
    toolpath.moves.windows(2).enumerate().find_map(|(i, w)| {
        let [from, mv] = w else { return None };
        if !is_descent_move(from.to, mv) {
            return None;
        }
        let (a, b) = (from.to.xy(), mv.to.xy());
        let inside = areas.iter().any(|area| {
            area.contains_within(a, slack_mm)
                && area.contains_within(b, slack_mm)
                && area.segment_inside(a, b, slack_mm)
        });
        (!inside).then_some(i + 1)
    })
}

/// `toolpath` with every descent move ([`is_descent_move`]) made a rapid
/// to the same point, so [`crate::footprint`](fn@crate::footprint) sweeps only the rest; the
/// descents are checked by [`first_descent_outside`].
pub(crate) fn without_descents(toolpath: &Toolpath) -> Toolpath {
    let mut moves = toolpath.moves.clone();
    for (i, w) in toolpath.moves.windows(2).enumerate() {
        if let [from, mv] = w
            && is_descent_move(from.to, mv)
            && let Some(slot) = moves.get_mut(i + 1)
        {
            *slot = Move::rapid(mv.to);
        }
    }
    Toolpath { moves }
}

/// The angle in (0, 45] degrees and the helix fraction in (0, 1] that
/// validation requires of operation `op`'s `entry`, checked again: an
/// entry outside them would cut something other than what validation
/// passed.
pub(crate) fn check(op: &OperationId, entry: &Entry) -> Result<(), crate::CamError> {
    let (angle_deg, fraction) = match *entry {
        Entry::Plunge => return Ok(()),
        Entry::Ramp { angle_deg } => (angle_deg, None),
        Entry::Helix {
            diameter_fraction,
            angle_deg,
        } => (angle_deg, Some(diameter_fraction)),
    };
    if !(angle_deg > 0.0 && angle_deg <= 45.0) {
        return Err(crate::CamError::InvalidEntry {
            op: op.clone(),
            field: "angle_deg",
            value: angle_deg,
            range: "more than 0 and at most 45",
        });
    }
    match fraction {
        Some(f) if !(f > 0.0 && f <= 1.0) => Err(crate::CamError::InvalidEntry {
            op: op.clone(),
            field: "diameter_fraction",
            value: f,
            range: "more than 0 and at most 1",
        }),
        _ => Ok(()),
    }
}

/// The entry warnings of one operation, one per shape and outcome however
/// many blocks of the shape fell back: `EntryFallback` per (shape, entry
/// used), and one `HelixDiameterReduced` per shape with the smallest
/// diameter cut. In the order shapes first fell back.
#[derive(Debug, Default)]
pub(crate) struct EntryWarnings {
    fallbacks: Vec<(ShapeRef, EntryKind, EntryKind, FallbackReason)>,
    reduced: Vec<(ShapeRef, HelixDiameter)>,
}

impl EntryWarnings {
    /// Notes what `descent`, cut for `shape`, fell back on.
    pub(crate) fn record(&mut self, shape: &ShapeRef, descent: &Descent) {
        if descent.fell_back()
            && !self
                .fallbacks
                .iter()
                .any(|(s, _, used, _)| s == shape && *used == descent.used)
        {
            self.fallbacks.push((
                shape.clone(),
                descent.requested,
                descent.used,
                descent.reason.unwrap_or(FallbackReason::NoRoom),
            ));
        }
        if let Some(d) = descent.diameter_reduced() {
            match self.reduced.iter_mut().find(|(s, _)| s == shape) {
                Some((_, kept)) => kept.used_mm = kept.used_mm.min(d.used_mm),
                None => self.reduced.push((shape.clone(), d)),
            }
        }
    }

    /// The warnings, each naming `op`.
    pub(crate) fn into_warnings(self, op: &OperationId) -> Vec<Warning> {
        let fallbacks = self
            .fallbacks
            .into_iter()
            .map(|(shape, requested, used, reason)| Warning::EntryFallback {
                op: op.clone(),
                shape,
                requested,
                used,
                reason,
            });
        let reduced = self
            .reduced
            .into_iter()
            .map(|(shape, d)| Warning::HelixDiameterReduced {
                op: op.clone(),
                shape,
                requested_mm: d.requested_mm,
                used_mm: d.used_mm,
            });
        fallbacks.chain(reduced).collect()
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "tests"
)]
mod tests {
    //! Ported from PureCutCNC `src/engine/toolpaths/entry.test.ts` unless a
    //! test says otherwise. Upstream's fixture policy is kept: a 4 mm tool,
    //! feed 800, plunge 200, conventional, internal, helix 80 % at 5°.

    use super::*;
    use crate::toolpath::MoveKind;
    use dc_geometry::{
        Contour, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point3, Polyline, Vertex, assert_approx_eq,
        nest,
    };
    use std::f64::consts::PI;

    const D_MM: f64 = 4.0;
    const FEED: f64 = 800.0;
    const PLUNGE: f64 = 200.0;

    fn rect(min: (f64, f64), w: f64, h: f64) -> Polyline {
        Polyline::rect(Point::new(min.0, min.1), w, h)
    }

    /// The region of `outer` with `islands` as holes.
    fn region(outer: Polyline, islands: Vec<Polyline>) -> Region {
        let mut contours = vec![Contour::new(outer).unwrap()];
        contours.extend(islands.into_iter().map(|p| Contour::new(p).unwrap()));
        let (mut regions, _) = nest(contours).unwrap();
        regions.remove(0)
    }

    fn rectangle(w: f64, h: f64) -> Region {
        region(rect((0.0, 0.0), w, h), Vec::new())
    }

    /// Upstream's 10 × 10 with the 2 × 2 island at its centre.
    fn with_island() -> Region {
        region(
            rect((0.0, 0.0), 10.0, 10.0),
            vec![rect((4.0, 4.0), 2.0, 2.0)],
        )
    }

    fn domain(region: Region) -> EntryDomain {
        EntryDomain::new(&[region], D_MM, FLATTEN_TOLERANCE_DEFAULT_MM)
    }

    fn tool() -> EntryTool {
        EntryTool {
            diameter_mm: D_MM,
            feed_mm_min: FEED,
            plunge_mm_min: PLUNGE,
            direction: CutDirection::Conventional,
            side: CutSide::Internal,
            quantum_mm: 0.001,
        }
    }

    fn helix(fraction: f64, angle_deg: f64) -> Entry {
        Entry::Helix {
            diameter_fraction: fraction,
            angle_deg,
        }
    }

    fn upstream_helix() -> Entry {
        helix(0.8, 5.0)
    }

    fn entry(region: Region, entry: &Entry, target: (f64, f64), start_z: f64, z: f64) -> Descent {
        synthesize(
            &domain(region),
            entry,
            Point::new(target.0, target.1),
            start_z,
            z,
            &tool(),
        )
    }

    /// The descent's start at `start_z`, then every move's end.
    fn points(d: &Descent, start_z: f64) -> Vec<Point3> {
        let mut out = vec![Point3::new(d.from.x, d.from.y, start_z)];
        out.extend(d.moves.iter().map(|m| m.to));
        out
    }

    /// Each move with the point it starts from.
    fn steps(d: &Descent, start_z: f64) -> Vec<(Point3, Move)> {
        let pts = points(d, start_z);
        pts.iter().copied().zip(d.moves.iter().copied()).collect()
    }

    fn descends(from: Point3, mv: &Move) -> bool {
        mv.to.z < from.z - 1e-9
    }

    fn len3(a: Point3, b: Point3) -> f64 {
        ((b.x - a.x).powi(2) + (b.y - a.y).powi(2) + (b.z - a.z).powi(2)).sqrt()
    }

    fn xy_len(a: Point3, b: Point3) -> f64 {
        a.xy().distance_to(b.xy())
    }

    /// Every descent ends at the target at the pass Z, every move a
    /// `Linear` with a feed: the shape a `CutBlock` descent must have.
    fn assert_ends_at_target(d: &Descent, target: (f64, f64), z: f64) {
        if d.moves.is_empty() {
            assert!(d.from.distance_to(Point::new(target.0, target.1)) <= EPS_MM);
            return;
        }
        let last = d.moves.last().unwrap().to;
        assert!(
            last.xy().distance_to(Point::new(target.0, target.1)) <= EPS_MM,
            "descent ends at {last:?}, not the target {target:?}"
        );
        assert_approx_eq!(last.z, z, EPS_MM);
        for (i, m) in d.moves.iter().enumerate() {
            assert_eq!(m.kind, MoveKind::Linear, "move {i}: {m:?}");
            assert!(m.feed_mm_min.is_some_and(|f| f > 0.0), "move {i}: {m:?}");
        }
    }

    #[test]
    fn pitch_from_ramp_angle() {
        let expected = PI * 3.2 * (5.0_f64).to_radians().tan();
        assert_approx_eq!(helix_pitch_mm(3.2, 5.0), expected, 1e-12);
    }

    /// S10: the scale keeps `feed × scale × sin(angle)` at or under the
    /// plunge rate, and every descending helix move's own vertical rate is
    /// at or under it too (upstream "limits the entry feed so its vertical
    /// component does not exceed plunge feed").
    #[test]
    fn feed_scale_keeps_vertical_rate_under_plunge() {
        let scale = plunge_limited_feed_scale(1000.0, 80.0, 15.0);
        let vertical = 1000.0 * scale * (15.0_f64).to_radians().sin();
        assert!(vertical <= 80.0 + 1e-9, "vertical feed {vertical}");
        assert!(scale < 1.0, "the scale applies: {scale}");
        assert_approx_eq!(plunge_limited_feed_scale(800.0, 200.0, 5.0), 1.0, 1e-12);

        let fast = EntryTool {
            feed_mm_min: 1000.0,
            plunge_mm_min: 80.0,
            ..tool()
        };
        let d = synthesize(
            &domain(rectangle(20.0, 20.0)),
            &helix(0.8, 15.0),
            Point::new(10.0, 10.0),
            5.0,
            0.0,
            &fast,
        );
        assert_eq!(d.used, EntryKind::Helix);
        let mut descending = 0;
        for (i, (from, mv)) in steps(&d, 5.0).into_iter().enumerate() {
            let f = mv.feed_mm_min.unwrap();
            assert!(f <= 1000.0 + 1e-9, "move {i} feeds above the feed: {f}");
            if descends(from, &mv) {
                descending += 1;
                let vertical = f * (from.z - mv.to.z) / len3(from, mv.to);
                assert!(
                    vertical <= 80.0 + 1e-9,
                    "move {i} descends at {vertical} mm/min: {mv:?}"
                );
            }
        }
        assert!(descending > 0);
    }

    #[test]
    fn largest_clearance_circle_avoids_island() {
        let circle = domain(with_island()).largest_clearance_circle().unwrap();
        assert!(
            circle.radius_mm > 2.0,
            "the open corners are used: {circle:?}"
        );
        let c = circle.centre;
        assert!(
            !(c.x > 4.0 && c.x < 6.0 && c.y > 4.0 && c.y < 6.0),
            "centre on the island: {circle:?}"
        );
    }

    /// Upstream asks for 150 %; DreamCarve's validation stops at 100 %, but
    /// the entry module still clamps whatever it is given to the tool's
    /// diameter (the no-core limit) and reports the reduction.
    #[test]
    fn helix_diameter_clamped_to_tool_and_warns() {
        let d = entry(
            rectangle(20.0, 20.0),
            &helix(1.5, 5.0),
            (10.0, 10.0),
            6.0,
            0.0,
        );
        assert_eq!(d.used, EntryKind::Helix);
        let reduced = d.diameter_reduced().expect("the clamp is reported");
        assert_approx_eq!(reduced.requested_mm, 6.0, 1e-9);
        assert!(reduced.used_mm <= D_MM + 1e-9, "{reduced:?}");
        assert_ends_at_target(&d, (10.0, 10.0), 0.0);
    }

    #[test]
    fn helix_bounded_by_clearance_and_warns() {
        let d = entry(rectangle(3.0, 3.0), &upstream_helix(), (1.5, 1.5), 4.0, 0.0);
        assert_eq!(d.used, EntryKind::Helix);
        assert!(d.diameter_reduced().is_some(), "{d:?}");
        for p in points(&d, 4.0) {
            assert!(
                (-1e-6..=3.0 + 1e-6).contains(&p.x) && (-1e-6..=3.0 + 1e-6).contains(&p.y),
                "helix point outside the clearance: {p:?}"
            );
        }
        assert_ends_at_target(&d, (1.5, 1.5), 0.0);
    }

    #[test]
    fn helix_stays_outside_islands() {
        let d = entry(with_island(), &upstream_helix(), (2.0, 5.0), 4.0, 0.0);
        assert_eq!(d.used, EntryKind::Helix);
        for p in points(&d, 4.0) {
            assert!(
                !(p.x > 4.0 && p.x < 6.0 && p.y > 4.0 && p.y < 6.0),
                "helix point on the island: {p:?}"
            );
        }
        assert_ends_at_target(&d, (2.0, 5.0), 0.0);
    }

    #[test]
    fn helix_descent_revolutions_plus_flat_bottom() {
        let d = entry(
            rectangle(20.0, 20.0),
            &upstream_helix(),
            (10.0, 10.0),
            5.0,
            0.0,
        );
        assert_eq!(d.used, EntryKind::Helix);
        let revolutions = 5.0 / helix_pitch_mm(3.2, 5.0);
        let want = (revolutions * 48.0).ceil() as usize;
        let s = steps(&d, 5.0);
        let descending = s.iter().filter(|(f, m)| descends(*f, m)).count();
        assert_eq!(descending, want, "descent segments");
        let flat = s
            .iter()
            .filter(|(f, m)| f.z.abs() <= 1e-9 && m.to.z.abs() <= 1e-9 && xy_len(*f, m.to) > 0.0)
            .count();
        assert!(flat >= 48, "a full flat revolution: {flat} flat moves");
        assert_ends_at_target(&d, (10.0, 10.0), 0.0);
    }

    /// The signed sweep, radians, of the descent's XY path about `c`.
    fn sweep_about(d: &Descent, c: Point) -> f64 {
        let pts = points(d, 0.0);
        pts.windows(2)
            .map(|w| {
                let (u, v) = (w[0].xy() - c, w[1].xy() - c);
                u.cross(v).atan2(u.dot(v))
            })
            .sum()
    }

    /// The centre of the flat bottom revolution: the mean of its 48 ends.
    fn flat_centre(d: &Descent, z: f64) -> Point {
        let s = steps(d, f64::INFINITY);
        let flat: Vec<Point> = s
            .iter()
            .filter(|(f, m)| (f.z - z).abs() <= 1e-9 && (m.to.z - z).abs() <= 1e-9)
            .take(HELIX_SEGMENTS_PER_REV)
            .map(|(_, m)| m.to.xy())
            .collect();
        assert_eq!(flat.len(), HELIX_SEGMENTS_PER_REV);
        let n = flat.len() as f64;
        Point::new(
            flat.iter().map(|p| p.x).sum::<f64>() / n,
            flat.iter().map(|p| p.y).sum::<f64>() / n,
        )
    }

    /// Port of "maps climb/conventional direction for internal and external
    /// cuts", sign flipped: upstream returns −1 for an internal climb
    /// because its positive rotation is clockwise once its Y-down project
    /// space is shown Y-up. DreamCarve is Y-up (ADR-0001), so its positive
    /// rotation is counter-clockwise and an internal climb is CCW, as the
    /// pocket wall's climb ring is. The emitted helix turns that way.
    #[test]
    fn helix_sense_climb_inside_is_ccw() {
        assert!(helix_is_ccw(CutDirection::Climb, CutSide::Internal));
        assert!(!helix_is_ccw(CutDirection::Conventional, CutSide::Internal));
        let climb = EntryTool {
            direction: CutDirection::Climb,
            ..tool()
        };
        for (t, ccw) in [(climb, true), (tool(), false)] {
            let d = synthesize(
                &domain(rectangle(20.0, 20.0)),
                &upstream_helix(),
                Point::new(10.0, 10.0),
                5.0,
                0.0,
                &t,
            );
            let sweep = sweep_about(&d, flat_centre(&d, 0.0));
            assert_eq!(sweep > 0.0, ccw, "{:?}: sweep {sweep}", t.direction);
        }
    }

    /// The external half of the same upstream test, sign flipped for Y-up:
    /// outside a boss climb is clockwise, as an outside profile's climb
    /// lap is.
    #[test]
    fn helix_sense_climb_outside_is_cw() {
        assert!(!helix_is_ccw(CutDirection::Climb, CutSide::External));
        assert!(helix_is_ccw(CutDirection::Conventional, CutSide::External));
        let outside_climb = EntryTool {
            direction: CutDirection::Climb,
            side: CutSide::External,
            ..tool()
        };
        let d = synthesize(
            &domain(rectangle(20.0, 20.0)),
            &upstream_helix(),
            Point::new(10.0, 10.0),
            5.0,
            0.0,
            &outside_climb,
        );
        assert!(sweep_about(&d, flat_centre(&d, 0.0)) < 0.0);
    }

    #[test]
    fn narrow_slot_falls_back_to_ramp() {
        let d = entry(
            rectangle(20.0, 0.1),
            &upstream_helix(),
            (0.0, 0.05),
            3.0,
            0.0,
        );
        assert_eq!(d.requested, EntryKind::Helix);
        assert_eq!(d.used, EntryKind::Ramp, "{d:?}");
        assert!(d.fell_back());
        assert!(!d.moves.is_empty(), "the ramp has moves");
        for (i, (from, mv)) in steps(&d, 3.0).into_iter().enumerate() {
            assert!(mv.to.z <= from.z + 1e-9, "move {i} climbs: {mv:?}");
            assert!(
                !(descends(from, &mv) && xy_len(from, mv.to) <= EPS_MM),
                "move {i} is a plunge: {mv:?}"
            );
        }
        assert_ends_at_target(&d, (0.0, 0.05), 0.0);
    }

    #[test]
    fn no_room_falls_back_to_plunge() {
        let d = entry(
            rectangle(0.01, 0.01),
            &upstream_helix(),
            (0.005, 0.005),
            2.0,
            0.0,
        );
        assert_eq!(d.requested, EntryKind::Helix);
        assert_eq!(d.used, EntryKind::Plunge);
        assert!(d.fell_back());
        assert!(d.moves.is_empty(), "a plunge is the sequencer's: {d:?}");
        assert!(d.from.distance_to(Point::new(0.005, 0.005)) <= EPS_MM);
    }

    /// A 1 % helix at 0.1° needs far more than [`MAX_DESCENT_MOVES`]; the
    /// fallback is a ramp whose move count is bounded.
    #[test]
    fn shallow_angle_move_count_is_bounded() {
        let d = entry(
            rectangle(20.0, 20.0),
            &helix(0.01, 0.1),
            (10.0, 10.0),
            0.0,
            -100.0,
        );
        assert_eq!(d.used, EntryKind::Ramp, "bounded fallback");
        assert!(d.fell_back());
        assert!(
            d.moves.len() < MAX_DESCENT_MOVES + 10,
            "{} moves",
            d.moves.len()
        );
    }

    #[test]
    fn ramp_run_is_capped_at_three_diameters() {
        let ramp = Entry::Ramp { angle_deg: 5.0 };
        let d = entry(rectangle(200.0, 200.0), &ramp, (100.0, 100.0), 2.0, -2.0);
        assert_eq!(d.used, EntryKind::Ramp);
        // Three diameters of XY run, lengthened only by the descent.
        let cap = D_MM * 3.0 / (5.0_f64).to_radians().cos() + 1e-6;
        for (i, (from, mv)) in steps(&d, 2.0).into_iter().enumerate() {
            let l = len3(from, mv.to);
            assert!(l <= cap, "move {i} is {l} mm, over the {cap} mm cap");
        }
        assert_ends_at_target(&d, (100.0, 100.0), -2.0);
    }

    /// The drop of every descending move of `d` from `start_z`.
    fn drops(d: &Descent, start_z: f64) -> Vec<f64> {
        steps(d, start_z)
            .into_iter()
            .filter(|(from, mv)| descends(*from, mv))
            .map(|(from, mv)| from.z - mv.to.z)
            .collect()
    }

    /// #234 (found by the dc-gcode property test): a ramp over 7.0018
    /// legs' depth ended with an eighth leg falling 0.0019 mm over
    /// 0.02 mm, which the program writes level at the ramp's
    /// plunge-limited feed (S3). Now the last two legs share their
    /// 1.0519 mm: each runs 50.09 % of the 12 mm run at 5°, out and back.
    /// With 7.6 legs' depth the last leg falls 0.63 mm, more than
    /// [`RAMP_MIN_LAST_DROP_MM`], and is cut short at 60 % of the run as
    /// upstream's. Either way every leg runs at 5°, none falls less than
    /// the least drop, and the drops sum to the depth.
    #[test]
    fn ramp_has_no_fragment_leg() {
        let ramp = Entry::Ramp { angle_deg: 5.0 };
        let tan = 5.0_f64.to_radians().tan();
        let run_mm = 3.0 * D_MM;
        let full_mm = run_mm * tan;
        let shared = (1.0 + 0.0018) / 2.0 * run_mm;
        let cases = [
            (
                7.0018,
                [
                    run_mm, run_mm, run_mm, run_mm, run_mm, run_mm, shared, shared,
                ],
            ),
            (
                7.6,
                [
                    run_mm,
                    run_mm,
                    run_mm,
                    run_mm,
                    run_mm,
                    run_mm,
                    run_mm,
                    0.6 * run_mm,
                ],
            ),
        ];
        for (legs, runs) in cases {
            let depth_mm = legs * full_mm;
            let d = entry(
                rectangle(200.0, 200.0),
                &ramp,
                (100.0, 100.0),
                depth_mm,
                0.0,
            );
            assert_eq!(d.used, EntryKind::Ramp);
            let sloped: Vec<(f64, f64)> = steps(&d, depth_mm)
                .into_iter()
                .filter(|(from, mv)| descends(*from, mv))
                .map(|(from, mv)| (xy_len(from, mv.to), from.z - mv.to.z))
                .collect();
            assert_eq!(sloped.len(), runs.len(), "{legs}: {sloped:?}");
            for (i, ((run, drop), want_mm)) in sloped.iter().zip(runs).enumerate() {
                assert!(
                    (run - want_mm).abs() <= 1e-9
                        && (drop - run * tan).abs() <= 1e-9
                        && *drop >= RAMP_MIN_LAST_DROP_MM,
                    "{legs}: leg {i} falls {drop} mm over {run} mm, not {want_mm} mm at 5°"
                );
            }
            let total: f64 = sloped.iter().map(|(_, drop)| drop).sum();
            assert_approx_eq!(total, depth_mm, 1e-9);
            assert_ends_at_target(&d, (100.0, 100.0), 0.0);
        }
    }

    /// #234's helix half: its `ceil` count of chords splits the depth
    /// evenly already (Z linear in the angle), so a depth a sliver over a
    /// whole number of chords leaves no fragment either: every chord drops
    /// the same, and the drops sum to the depth.
    #[test]
    fn helix_has_no_fragment_segment() {
        let chord_mm = helix_pitch_mm(3.2, 5.0) / HELIX_SEGMENTS_PER_REV as f64;
        let depth_mm = 100.0018 * chord_mm;
        let d = entry(
            rectangle(20.0, 20.0),
            &upstream_helix(),
            (10.0, 10.0),
            depth_mm,
            0.0,
        );
        assert_eq!(d.used, EntryKind::Helix);
        let drops = drops(&d, depth_mm);
        assert_eq!(drops.len(), 101, "{drops:?}");
        for (i, drop) in drops.iter().enumerate() {
            assert!(
                (drop - depth_mm / 101.0).abs() <= 1e-9,
                "chord {i} drops {drop} mm of {depth_mm} mm"
            );
        }
        assert_approx_eq!(drops.iter().sum::<f64>(), depth_mm, 1e-9);
    }

    /// #265 (found by the dc-gcode Profile property, `PROPTEST_RNG_SEED=108`):
    /// a 0.05 helix of a 0.5508 mm tool is 0.0275 mm across; at 14.14°
    /// each of its chords falls 0.00045 mm over 0.0018 mm, less than one
    /// unit of a 3-place mm program either way, so most are written level
    /// at the plunge-limited feed, which S3 refuses. Such a helix is not
    /// cut: the chain falls back to the ramp, reason `too_small`. The same
    /// tool's 0.8 helix (0.44 mm across) is a helix, every chord of it
    /// falling at least a unit and running at least √2 units in XY, so its
    /// ends round apart on both counts.
    #[test]
    fn tiny_helix_chords_are_writable() {
        let quantum_mm = 0.001;
        let small = EntryTool {
            diameter_mm: 0.550_832_666_619_620_7,
            feed_mm_min: 3_840.280_223_564_292_6,
            plunge_mm_min: 645.988_501_885_48,
            ..tool()
        };
        let domain = EntryDomain::new(
            &[rectangle(20.0, 20.0)],
            small.diameter_mm,
            FLATTEN_TOLERANCE_DEFAULT_MM,
        );
        let target = Point::new(10.0, 10.0);
        let (start_z, z) = (1.436_449_925_824_323_2, 0.0);
        let d = synthesize(
            &domain,
            &helix(0.05, 14.137_048_663_595_39),
            target,
            start_z,
            z,
            &small,
        );
        assert_eq!(d.used, EntryKind::Ramp, "{:?}", d.reason);
        assert!(d.fell_back());
        assert_eq!(d.reason.map(FallbackReason::as_str), Some("too_small"));
        assert_ends_at_target(&d, (10.0, 10.0), z);

        let d = synthesize(
            &domain,
            &helix(0.8, 14.137_048_663_595_39),
            target,
            start_z,
            z,
            &small,
        );
        assert_eq!(d.used, EntryKind::Helix, "{:?}", d.reason);
        let sloped: Vec<(Point3, Move)> = steps(&d, start_z)
            .into_iter()
            .filter(|(from, mv)| descends(*from, mv))
            .collect();
        assert!(!sloped.is_empty());
        for (i, (from, mv)) in sloped.iter().enumerate() {
            let (drop, run) = (from.z - mv.to.z, xy_len(*from, mv.to));
            assert!(
                drop >= quantum_mm && run >= std::f64::consts::SQRT_2 * quantum_mm,
                "chord {i} falls {drop} mm over {run} mm"
            );
        }
    }

    /// Entry must not score the wall the finish pass leaves, so every ramp
    /// point stays a real standoff (0.1 · D) inside the boundary.
    #[test]
    fn ramp_stays_clear_of_boundary() {
        let ramp = Entry::Ramp { angle_deg: 5.0 };
        let d = entry(rectangle(40.0, 40.0), &ramp, (20.0, 20.0), 2.0, -2.0);
        assert_eq!(d.used, EntryKind::Ramp);
        let standoff = D_MM * 0.1;
        for p in points(&d, 2.0) {
            let to_edge = p.x.min(40.0 - p.x).min(p.y).min(40.0 - p.y);
            assert!(
                to_edge >= standoff - 1e-6,
                "ramp point {p:?} is {to_edge} mm from the wall"
            );
        }
    }

    /// Upstream "still places a helix when the cut start sits on the
    /// region boundary": the tangent placement cannot succeed there, so the
    /// bounded reachable search must.
    #[test]
    fn helix_found_when_target_on_boundary() {
        let d = entry(
            rectangle(40.0, 40.0),
            &upstream_helix(),
            (0.0, 20.0),
            2.0,
            -2.0,
        );
        assert_eq!(d.used, EntryKind::Helix);
        assert!(steps(&d, 2.0).iter().any(|(f, m)| descends(*f, m)));
        assert_ends_at_target(&d, (0.0, 20.0), -2.0);
    }

    /// Upstream regression #581: the reachable helix sits next to the cut
    /// start, not at the region's roomiest spot.
    #[test]
    fn helix_placed_near_target_not_roomiest_spot() {
        let d = entry(
            rectangle(200.0, 200.0),
            &upstream_helix(),
            (0.0, 100.0),
            2.0,
            -2.0,
        );
        assert_eq!(d.used, EntryKind::Helix);
        let target = Point::new(0.0, 100.0);
        for p in points(&d, 2.0) {
            let away = p.xy().distance_to(target);
            assert!(
                away <= D_MM * 2.0 + 1e-6,
                "helix strayed {away} mm from the cut start: {p:?}"
            );
        }
    }

    /// #272 (the Pocket property, `PROPTEST_RNG_SEED=108`): the
    /// tool-centre region of a ring pocket (outer 13.7528 mm, island
    /// 6.8532 mm, by a 3.2349 mm tool, rotated −5.49°), entered by a 0.976
    /// helix at 9.316° at a raster start on the island's flattened circle.
    /// The reachable placement's handoff to it was checked with the target
    /// slack (#213) along its whole length, so it ran 0.0051 mm inside the
    /// island's chords, 0.0151 mm inside its circle: past the flatten
    /// tolerance S9 allows, into the island. The slack is for the piece
    /// that ends on the target; the rest of the handoff stays in the
    /// flattened region, so it keeps the island's radius less the
    /// tolerance.
    #[test]
    fn helix_handoff_keeps_out_of_the_island() {
        let d_mm = 3.234_923_320_453_882;
        let (c, along) = (
            Point::new(40.0, 120.0),
            Vec2::new(0.995_419_341_093_451_9, -0.095_605_101_208_450_6),
        );
        let circle = |r: f64| {
            let (a, b) = (c + along * -r, c + along * r);
            Polyline::new(
                vec![Vertex::new(a.x, a.y, -1.0), Vertex::new(b.x, b.y, -1.0)],
                true,
            )
        };
        let island_mm = 6.853_196_837_408_963 + d_mm / 2.0;
        let ring = region(
            circle(13.752_766_298_916_859 - d_mm / 2.0),
            vec![circle(island_mm)],
        );
        let tool = EntryTool {
            diameter_mm: d_mm,
            direction: CutDirection::Climb,
            ..tool()
        };
        let target = Point::new(31.983_307, 117.293_382);
        let domain = EntryDomain::new(&[ring], d_mm, FLATTEN_TOLERANCE_DEFAULT_MM);
        let d = synthesize(
            &domain,
            &helix(0.976_100_214_112_066_5, 9.315_963_293_422_808),
            target,
            21.9,
            17.765,
            &tool,
        );
        assert_eq!(d.used, EntryKind::Helix);
        for (i, (from, mv)) in steps(&d, 21.9).iter().enumerate() {
            let near = segment_distance_sq(c, from.xy(), mv.to.xy()).sqrt();
            assert!(
                near >= island_mm - FLATTEN_TOLERANCE_DEFAULT_MM - 1e-9,
                "move {i} passes {near} mm from the island's centre: {mv:?}"
            );
        }
    }

    /// Not a port: `Entry::Plunge` (every phase 1 project after migration)
    /// gives an empty descent at the target, which the sequencer writes as
    /// the phase 1 straight plunge, and no fallback.
    #[test]
    fn plunge_is_default_and_unchanged_from_phase_1() {
        let d = entry(
            rectangle(20.0, 20.0),
            &Entry::Plunge,
            (10.0, 10.0),
            5.0,
            0.0,
        );
        assert_eq!(d.requested, EntryKind::Plunge);
        assert_eq!(d.used, EntryKind::Plunge);
        assert!(!d.fell_back());
        assert!(d.diameter_reduced().is_none());
        assert!(d.moves.is_empty());
        assert!(d.from.distance_to(Point::new(10.0, 10.0)) <= EPS_MM);
    }

    #[test]
    fn boundary_safety_is_a_tenth_of_the_diameter() {
        assert_approx_eq!(boundary_safety_mm(4.0), 0.4, 1e-12);
        assert_approx_eq!(boundary_safety_mm(1e-6), MIN_BOUNDARY_SAFETY_MM, 1e-12);
    }

    #[test]
    fn output_is_deterministic() {
        let a = entry(with_island(), &upstream_helix(), (2.0, 5.0), 4.0, 0.0);
        let b = entry(with_island(), &upstream_helix(), (2.0, 5.0), 4.0, 0.0);
        assert_eq!(a, b);
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "tests"
)]
mod profile_tests;

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "tests"
)]
mod start_tests;
