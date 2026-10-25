// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/edge.ts

//! How each pass of a Profile reaches its depth (`docs/specs/phase-2/
//! design.md`, "Algorithms → Entry"; functional spec capability 16, S9,
//! S10). Upstream resolves an entry policy per level (`insideEntryForLevel`,
//! `outsideEntryForLevel`), because the tabs it must keep out of stand only
//! below their tops; here each path has its waste domain and, when some
//! pass is below the tab top, that domain less the tab keep-outs
//! ([`wall_domains`]); its entry is placed in each, and each pass takes
//! the one for its Z. The leads ([`crate::lead`]) use the same domains.

use super::Wall;
use super::waste::{self, WallPath};
use crate::entry::{
    self, Descent, EntryDomain, EntryKind, EntryPlan, EntryTool, FallbackReason,
    MIN_BOUNDARY_SAFETY_MM, RAMP_MIN_LAST_DROP_MM, boundary_safety_mm, plan_at_wall,
};
use crate::error::{CamError, ShapeRef};
use crate::footprint::footprint_at;
use crate::lead::reseam;
use crate::pocket::rings::inset_region;
use crate::tabs::{PathTabs, Tabs};
use crate::toolpath::{Move, Toolpath};
use dc_geometry::{EPS_MM, Point, Point3, Polyline, Region, area_mm2, difference};
use dc_project::{CutDirection, Entry, ProfileOp, Side, Stock};

/// What [`path_entries`] and [`wall_domains`] need from the generator.
pub(super) struct EntrySetup<'a, 'b> {
    pub op: &'a ProfileOp,
    pub stock: &'a Stock,
    pub regions: &'a [Region],
    /// Every tool-centre path, closed, with its shape, in drawing order.
    pub paths: &'a [(Polyline, &'b ShapeRef)],
    /// The wall each path cuts and the index of its region.
    pub walls: &'a [(Wall, usize)],
    pub layouts: &'a [Option<(PathTabs, bool)>],
    pub tabs: Option<&'a Tabs>,
    pub pass_z: &'a [f64],
    /// The Z a descent starts at where no pass before cut: the entry
    /// clearance above the stock top (#214).
    pub start_z_mm: f64,
    pub entry_tool: &'a dyn Fn(Wall) -> EntryTool,
    pub tolerance_mm: f64,
    /// How much farther than the radius each path runs from its wall: a
    /// last pass stock to leave (#284), 0 for the part's own path. Missing
    /// entries are 0.
    pub stocks_to_leave: &'a [f64],
    /// The paths cut against `op.direction` (#284: a reversed last
    /// pass), whose entries turn the other way. Missing entries are
    /// false.
    pub reversed: &'a [bool],
}

/// One domain a path's entries and leads are placed in: its regions (for
/// the S9 report) and the [`EntryDomain`] built from them.
pub(super) struct Space {
    pub regions: Vec<Region>,
    pub domain: EntryDomain,
}

/// A path's waste domain and, when some pass is below the tab top, that
/// domain less the tab keep-outs.
pub(super) struct PathDomain {
    pub plain: Space,
    pub tabbed: Option<Space>,
}

impl PathDomain {
    /// The domain a pass uses: the tabbed one below the tab top.
    pub(super) fn for_pass(&self, below_tabs: bool) -> &Space {
        match &self.tabbed {
            Some(t) if below_tabs => t,
            _ => &self.plain,
        }
    }
}

/// Where a path's entries end, on the passes that use the plain domain
/// and on those that use the tabbed one: the contour start, or a lead's
/// staging point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Targets {
    pub plain: Point,
    pub tabbed: Point,
}

/// How one path enters its passes.
pub(super) enum PathEntry {
    /// On line: the ramp along the path at `angle_deg`; below the tab top
    /// it stops `clear_mm` along, short of the first tab.
    Along { angle_deg: f64, clear_mm: f64 },
    /// Inside or Outside: the entry placed in the waste domain, and, when
    /// some pass is below the tab top, in that domain less the tabs.
    /// `after_plain` is how a pass below the tab top ramps after a pass
    /// that ramped on `plain` ([`follow_plain`], #352); `None` plunges.
    Wall {
        plain: Box<EntryPlan>,
        tabbed: Option<Box<EntryPlan>>,
        after_plain: Option<Box<EntryPlan>>,
    },
}

/// #352: the plan a pass below the tab top, placed as `tabbed` in
/// `domain` (the waste domain less the tabs), ramps with after a pass that
/// ramped on `plain`: the passes above cut only `plain`'s ramp line, so
/// `tabbed` itself when it ramps on that line; else `tabbed` ramping
/// along that line as far as `domain` holds it
/// ([`EntryPlan::following`]), when both start at one point. `None` when
/// neither: `plain` has no ramp, the two start apart, or the line meets
/// the tabs too soon. Such a pass plunges, one step down, at its target.
pub(super) fn follow_plain(
    plain: &EntryPlan,
    tabbed: &EntryPlan,
    domain: &EntryDomain,
) -> Option<Box<EntryPlan>> {
    let (start, end) = plain.ramp_line()?;
    let same = |a: Point, b: Point| a.distance_to(b) <= EPS_MM;
    if tabbed
        .ramp_line()
        .is_some_and(|(s, e)| same(s, start) && same(e, end))
    {
        return Some(Box::new(tabbed.clone()));
    }
    if !same(start, tabbed.target()) {
        return None;
    }
    tabbed.following(domain, end).map(Box::new)
}

/// One pass of one path, for [`PathEntry::descend`].
pub(super) struct PassEntry<'a> {
    pub path: &'a Polyline,
    pub pass: usize,
    pub z_mm: f64,
    /// The pass is below the tab top, so the tabs stand.
    pub below_tabs: bool,
    /// The Z a descent starts at where no pass before cut: the entry
    /// clearance above the stock top (#214).
    pub start_z_mm: f64,
    /// The stock top, mm (#356).
    pub top_z_mm: f64,
    pub tool: EntryTool,
    pub tolerance_mm: f64,
}

/// The entry one pass of a path cut, which the next pass's follows
/// ([`PathEntry::descend`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CutEntry {
    /// The pass cut on the tabbed plan, or along its line.
    pub tabbed: bool,
    /// The entry it cut.
    pub used: EntryKind,
    /// #356: how far from its start a ramp ran below the stock top, by
    /// at least one unit in the last written place, mm ([`reach_below_mm`]);
    /// 0 for any other entry.
    pub reach_mm: f64,
}

impl CutEntry {
    /// A pass that cut `used` on the tabbed plan (`tabbed`) or the plain
    /// one, and ran no ramp.
    pub(super) const fn new(tabbed: bool, used: EntryKind) -> Self {
        Self {
            tabbed,
            used,
            reach_mm: 0.0,
        }
    }

    /// Whether the pass cut `used` on the tabbed plan (`tabbed`) or the
    /// plain one.
    fn was(previous: Option<Self>, tabbed: bool, used: EntryKind) -> bool {
        previous.is_some_and(|p| p.tabbed == tabbed && p.used == used)
    }
}

/// #356: how far from `d`'s start XY its moves ran at least `quantum_mm`
/// below `top_z_mm`, mm, the descent starting at `from_z_mm`: the stretch
/// of its ramp line it cut a trench along, even once the program rounds
/// each Z by half that unit. 0 when none ran so low. A first pass's ramp
/// starts above the stock, so its first runs cut nothing.
pub(super) fn reach_below_mm(d: &Descent, from_z_mm: f64, top_z_mm: f64, quantum_mm: f64) -> f64 {
    let floor_mm = top_z_mm - quantum_mm;
    let start = d.from;
    let mut at = Point3::new(start.x, start.y, from_z_mm);
    let mut reach_mm: f64 = 0.0;
    for mv in &d.moves {
        let (a, b) = (at, mv.to);
        at = b;
        let point = match (a.z < floor_mm, b.z < floor_mm) {
            (false, false) => continue,
            (true, true) => a.xy().distance_to(start).max(b.xy().distance_to(start)),
            (a_below, _) => {
                let t = (floor_mm - a.z) / (b.z - a.z);
                let cross = a.xy() + (b.xy() - a.xy()) * t;
                let below = if a_below { a } else { b };
                below.xy().distance_to(start).max(cross.distance_to(start))
            }
        };
        reach_mm = reach_mm.max(point);
    }
    reach_mm
}

/// The entry of each path of `setup`, in order: `None` for a plunge, which
/// cuts as phase 1 did. Inside and Outside entries are placed in each
/// path's `domains` (from [`wall_domains`]) at its `targets`.
pub(super) fn path_entries(
    setup: &EntrySetup<'_, '_>,
    domains: &[PathDomain],
    targets: &[Targets],
) -> Vec<Option<PathEntry>> {
    let op = setup.op;
    let n = setup.paths.len();
    match (op.side, &op.entry) {
        (_, Entry::Plunge) => (0..n).map(|_| None).collect(),
        // #207: Left and Right ramp along their offset path as On line
        // ramps along its line.
        (Side::OnLine | Side::Left | Side::Right, Entry::Ramp { angle_deg }) => setup
            .layouts
            .iter()
            .map(|layout| {
                let tool = (setup.entry_tool)(Wall::Outside);
                let clear_mm = layout.as_ref().map_or(f64::INFINITY, |(l, _)| {
                    l.clear_from_start_mm() - boundary_safety_mm(tool.diameter_mm)
                });
                Some(PathEntry::Along {
                    angle_deg: *angle_deg,
                    clear_mm,
                })
            })
            .collect(),
        // Refused before this is reached; a plunge if it ever is.
        (Side::OnLine | Side::Left | Side::Right, Entry::Helix { .. }) => {
            (0..n).map(|_| None).collect()
        }
        (Side::Inside | Side::Outside, _) => (0..n)
            .map(|i| {
                let (domain, target) = (domains.get(i)?, targets.get(i)?);
                let tool = tool_for(setup, i);
                let place = |space: &Space, at: Point| {
                    Box::new(plan_at_wall(&space.domain, &op.entry, at, &tool))
                };
                let plain = place(&domain.plain, target.plain);
                let tabbed = domain
                    .tabbed
                    .as_ref()
                    .map(|space| (space, place(space, target.tabbed)));
                let after_plain = tabbed
                    .as_ref()
                    .and_then(|(space, t)| follow_plain(&plain, t, &space.domain));
                Some(PathEntry::Wall {
                    plain,
                    tabbed: tabbed.map(|(_, t)| t),
                    after_plain,
                })
            })
            .collect(),
    }
}

/// #176: where each closed path of `setup` should start so its ramp or
/// helix fits clear of the tabs, as a distance along it from its start;
/// `None` keeps the start. Only a path whose seam is free (`kept` false:
/// no lead, and no explicit start, #285, which wins over the entry) whose
/// passes below the tab top would plunge at its start is moved:
/// its start under a tab span, or so near one that the entry has no room
/// in the waste domain less the tab keep-outs (Inside, Outside) or before
/// the first tab (On line). The candidates are the middles of the
/// stretches between tab spans, longest first, then nearest the start;
/// the first where no pass below the tab top plunges wins. When none
/// does, the start is kept and those passes plunge as before. One start
/// serves every pass, and the tabs keep their distances from the
/// original start ([`PathTabs::reseamed`]).
pub(super) fn tab_seams(
    setup: &EntrySetup<'_, '_>,
    domains: &[PathDomain],
    kept: &[bool],
) -> Vec<Option<f64>> {
    let none = || setup.paths.iter().map(|_| None).collect();
    let Some(tabs) = setup.tabs else {
        return none();
    };
    let below: Vec<(f64, f64)> = setup
        .pass_z
        .iter()
        .enumerate()
        .filter(|(_, z)| **z < tabs.top_z_mm() - EPS_MM)
        .map(|(k, z)| {
            let from_z_mm = k
                .checked_sub(1)
                .and_then(|j| setup.pass_z.get(j).copied())
                .unwrap_or(setup.start_z_mm);
            (from_z_mm, *z)
        })
        .collect();
    if below.is_empty() || matches!(setup.op.entry, Entry::Plunge) {
        return none();
    }
    let above = below.len() < setup.pass_z.len();
    let along = match (setup.op.side, &setup.op.entry) {
        (Side::OnLine | Side::Left | Side::Right, Entry::Ramp { angle_deg }) => Some(*angle_deg),
        (Side::OnLine | Side::Left | Side::Right, _) => return none(),
        _ => None,
    };
    setup
        .paths
        .iter()
        .enumerate()
        .map(|(i, (path, _))| {
            let layout = match setup.layouts.get(i) {
                Some(Some((layout, _))) if path.is_closed() => layout,
                _ => return None,
            };
            if kept.get(i).copied().unwrap_or(false) {
                return None;
            }
            let tool = tool_for(setup, i);
            let fits = |seam_mm: f64| {
                let moved = layout.reseamed(seam_mm);
                if moved.clear_from_start_mm() <= EPS_MM {
                    return false;
                }
                let seamed = reseam(path, seam_mm);
                match along {
                    Some(angle_deg) => {
                        let clear_mm =
                            moved.clear_from_start_mm() - boundary_safety_mm(tool.diameter_mm);
                        below.iter().all(|&(from_z_mm, z_mm)| {
                            let d = entry::along_path(
                                &seamed,
                                angle_deg,
                                clear_mm,
                                from_z_mm,
                                z_mm,
                                &tool,
                                setup.tolerance_mm,
                            );
                            d.used != EntryKind::Plunge
                        })
                    }
                    None => {
                        let (Some(space), Some(at)) = (
                            domains.get(i).map(|d| d.for_pass(true)),
                            seamed.vertices().first().map(|v| v.point()),
                        ) else {
                            return false;
                        };
                        let mut plan = plan_at_wall(&space.domain, &setup.op.entry, at, &tool);
                        // #352: after passes above the tab top, a ramp
                        // follows their line.
                        if above && matches!(setup.op.entry, Entry::Ramp { .. }) {
                            let Some(plain) = domains
                                .get(i)
                                .map(|d| plan_at_wall(&d.plain.domain, &setup.op.entry, at, &tool))
                            else {
                                return false;
                            };
                            match follow_plain(&plain, &plan, &space.domain) {
                                Some(follow) => plan = *follow,
                                None => return false,
                            }
                        }
                        below.iter().all(|&(from_z_mm, z_mm)| {
                            plan.descend(from_z_mm, z_mm).used != EntryKind::Plunge
                        })
                    }
                }
            };
            if fits(0.0) {
                return None;
            }
            let length_mm = layout.length_mm();
            layout
                .gaps_mm()
                .into_iter()
                .map(|(start_mm, gap_mm)| (start_mm + gap_mm / 2.0).rem_euclid(length_mm))
                .find(|&seam_mm| fits(seam_mm))
        })
        .collect()
}

/// The entry tool of path `i`: cutting the other way on a reversed path
/// (#284).
pub(super) fn tool_for(setup: &EntrySetup<'_, '_>, i: usize) -> EntryTool {
    let wall = setup.walls.get(i).map_or(Wall::Outside, |(w, _)| *w);
    let tool = (setup.entry_tool)(wall);
    if setup.reversed.get(i).copied().unwrap_or(false) {
        EntryTool {
            direction: match tool.direction {
                CutDirection::Climb => CutDirection::Conventional,
                CutDirection::Conventional => CutDirection::Climb,
            },
            ..tool
        }
    } else {
        tool
    }
}

/// Inside and Outside: each path's waste domain ([`waste::domains`]) and,
/// when some pass is below the tab top, that domain less every path's tab
/// keep-outs ([`waste::tab_keep_out`]). Empty for On line, Left and
/// Right, which have no waste area.
pub(super) fn wall_domains(setup: &EntrySetup<'_, '_>) -> Vec<PathDomain> {
    if !setup.op.side.has_waste_area() {
        return Vec::new();
    }
    let radius_mm = tool_for(setup, 0).diameter_mm / 2.0;
    let tolerance_mm = setup.tolerance_mm;
    let wall_paths: Vec<WallPath<'_>> = setup
        .paths
        .iter()
        .zip(setup.walls)
        .enumerate()
        .map(|(i, ((path, _), (_, region)))| WallPath {
            path,
            region: *region,
            stock_to_leave_mm: setup.stocks_to_leave.get(i).copied().unwrap_or(0.0),
        })
        .collect();
    let domains = waste::domains(
        setup.op.side,
        setup.regions,
        &wall_paths,
        setup.stock,
        radius_mm,
        tolerance_mm,
    );
    // Tabs stand below their top: keep the entries off all of them.
    let keep_out: Vec<Region> = match setup.tabs {
        Some(tabs) if setup.pass_z.iter().any(|z| *z < tabs.top_z_mm() - EPS_MM) => setup
            .paths
            .iter()
            .zip(setup.layouts)
            .filter_map(|((path, _), layout)| {
                layout.as_ref().map(|(l, _)| {
                    waste::tab_keep_out(path, &l.stretches_mm(tabs), radius_mm, tolerance_mm)
                })
            })
            .flatten()
            .collect(),
        _ => Vec::new(),
    };
    domains
        .into_iter()
        .enumerate()
        .map(|(i, regions)| {
            let diameter_mm = tool_for(setup, i).diameter_mm;
            let space = |regions: Vec<Region>| Space {
                domain: EntryDomain::new(&regions, diameter_mm, tolerance_mm),
                regions,
            };
            let tabbed = (!keep_out.is_empty())
                .then(|| space(difference(&regions, &keep_out, tolerance_mm)));
            PathDomain {
                plain: space(regions),
                tabbed,
            }
        })
        .collect()
}

impl PathEntry {
    /// The descent of one pass that starts at `from_z_mm` (the entry
    /// clearance above the stock top on the first pass, #214; the previous
    /// pass Z after), and the Z it starts at. `previous` is which plan
    /// (tabbed or not) and entry the path's previous pass cut, updated
    /// here: a helix after the first pass rejoins the bore its previous
    /// pass cut, or starts again from just above the stock when that pass
    /// cut none; a ramp after the first pass keeps to the stretch of the
    /// line its previous pass ramped on below the stock top, or plunges
    /// ([`PathEntry::in_trench`], #352, #356). A pass that follows the plain plan's line records the plain
    /// plan, whose line it cut.
    pub(super) fn descend(
        &self,
        pass: &PassEntry<'_>,
        from_z_mm: f64,
        previous: &mut Option<CutEntry>,
    ) -> (Descent, f64) {
        match self {
            Self::Along {
                angle_deg,
                clear_mm,
            } => {
                let max_run_mm = if pass.below_tabs {
                    *clear_mm
                } else {
                    f64::INFINITY
                };
                let d = entry::along_path(
                    pass.path,
                    *angle_deg,
                    max_run_mm,
                    from_z_mm,
                    pass.z_mm,
                    &pass.tool,
                    pass.tolerance_mm,
                );
                *previous = Some(CutEntry::new(pass.below_tabs, d.used));
                (d, from_z_mm)
            }
            Self::Wall {
                plain,
                tabbed,
                after_plain,
            } => {
                let (key, plan) = match tabbed {
                    Some(t) if pass.below_tabs => (true, t),
                    _ => (false, plain),
                };
                let mut from_z = from_z_mm;
                let mut d = plan.descend(from_z, pass.z_mm);
                if pass.pass > 0 && d.used == EntryKind::Helix {
                    if CutEntry::was(*previous, key, EntryKind::Helix) {
                        d = plan.rejoin(d, from_z);
                    } else {
                        // The previous pass cut no bore here: helix from
                        // just above the stock (the sequencer retracts,
                        // since the helix does not start where the lap
                        // ended, and rapids down to it), or ramp from
                        // where the tool is.
                        let restart = plan.descend(pass.start_z_mm, pass.z_mm);
                        if restart.used == EntryKind::Helix {
                            from_z = pass.start_z_mm;
                            d = restart;
                        } else {
                            d = plan.descend_without_helix(
                                from_z,
                                pass.z_mm,
                                FallbackReason::TooManyMoves,
                            );
                        }
                    }
                }
                let mut cut_key = key;
                if pass.pass > 0 && d.used == EntryKind::Ramp {
                    (d, cut_key) = Self::in_trench(
                        d,
                        plan,
                        after_plain.as_deref(),
                        pass,
                        from_z,
                        key,
                        *previous,
                    );
                }
                let reach_mm = if d.used == EntryKind::Ramp {
                    reach_below_mm(&d, from_z, pass.top_z_mm, pass.tool.quantum_mm)
                } else {
                    0.0
                };
                *previous = Some(CutEntry {
                    tabbed: cut_key,
                    used: d.used,
                    reach_mm,
                });
                (d, from_z)
            }
        }
    }

    /// #352: ramp `d`, of a pass after the first planned on `plan` (`key`:
    /// the tabbed plan), kept to the stock the pass above cut, and the
    /// plan whose line it ramps on (`key`, or `false` for the plain plan's
    /// line). The pass above cut only its own ramp line (`previous`), so:
    ///
    /// - a ramp on the line the pass above ramped on is kept;
    /// - a pass below the tab top after one that ramped on the plain plan
    ///   ramps along that line as far as the tabs leave it
    ///   (`after_plain`, [`follow_plain`]);
    /// - either ramp runs only as far along its line as the pass above's
    ///   ran below the stock top (`CutEntry::reach_mm`), zigzagging on its
    ///   line cut short there ([`EntryPlan::cut_short`]), or plunges, warned
    ///   `no_room`, when that leaves no run (#356: a first pass's ramp
    ///   starts above the stock, and its first runs cut nothing);
    /// - any other ramp (the pass above plunged, helixed, or cut another
    ///   path: a last pass after its stock passes, #284) plunges at
    ///   the target, off the tabs, one step down, warned `no_room`;
    /// - a ramp that would fall less than [`RAMP_MIN_LAST_DROP_MM`] cuts
    ///   its one run the whole length of its line, farther than the passes
    ///   above reached when each fell less than a run's drop, so it plunges
    ///   that little instead, warned `too_small`.
    fn in_trench(
        d: Descent,
        plan: &EntryPlan,
        after_plain: Option<&EntryPlan>,
        pass: &PassEntry<'_>,
        from_z_mm: f64,
        key: bool,
        previous: Option<CutEntry>,
    ) -> (Descent, bool) {
        let plunge = |reason| (plunged(plan.target(), d.requested, reason), key);
        if from_z_mm - pass.z_mm < RAMP_MIN_LAST_DROP_MM {
            return plunge(FallbackReason::TooSmall);
        }
        let reach_mm = previous.map_or(0.0, |p| p.reach_mm);
        let reason = d.reason.unwrap_or(FallbackReason::NoRoom);
        // #356: only as far along the line as the pass above cut it below
        // the stock top.
        let within = |plan: &EntryPlan, d: Descent| {
            let ran_mm = d
                .moves
                .iter()
                .map(|m| m.to.xy().distance_to(d.from))
                .fold(0.0, f64::max);
            if d.used != EntryKind::Ramp || ran_mm <= reach_mm + EPS_MM {
                return d;
            }
            match plan.cut_short(reach_mm) {
                Some(short) => short.descend_without_helix(from_z_mm, pass.z_mm, reason),
                None => plunged(plan.target(), d.requested, FallbackReason::NoRoom),
            }
        };
        if CutEntry::was(previous, key, EntryKind::Ramp) {
            return (within(plan, d), key);
        }
        match after_plain.filter(|_| key && CutEntry::was(previous, false, EntryKind::Ramp)) {
            Some(follow) => {
                let d = follow.descend_without_helix(from_z_mm, pass.z_mm, reason);
                let d = within(follow, d);
                let cut_key = if d.used == EntryKind::Ramp {
                    false
                } else {
                    key
                };
                (d, cut_key)
            }
            None => plunge(FallbackReason::NoRoom),
        }
    }

    /// S9 for a descent this entry cut for `shape`: every descent move's
    /// tool centre lies in the domain the entry was placed in (`domain`'s
    /// space for the pass), or the generator has a defect and no toolpath
    /// is produced ([`CamError::OffMaterial`]). On line descents follow
    /// the path and have no domain.
    pub(super) fn check(
        &self,
        op: &ProfileOp,
        shape: &ShapeRef,
        pass: &PassEntry<'_>,
        descent: &Descent,
        from_z_mm: f64,
        domain: Option<&PathDomain>,
    ) -> Result<(), CamError> {
        let (Self::Wall { .. }, Some(domain)) = (self, domain) else {
            return Ok(());
        };
        let space = domain.for_pass(pass.below_tabs);
        let from = Point3::new(descent.from.x, descent.from.y, from_z_mm);
        let slack_mm = pass.tolerance_mm + MIN_BOUNDARY_SAFETY_MM;
        let Some(i) = space
            .domain
            .first_move_outside(from, &descent.moves, slack_mm)
        else {
            return Ok(());
        };
        let (Some(mv), start) = (
            descent.moves.get(i),
            i.checked_sub(1)
                .and_then(|k| descent.moves.get(k))
                .map_or(from, |m| m.to),
        ) else {
            return Ok(());
        };
        Err(off_material(
            op,
            shape,
            start,
            *mv,
            space,
            pass.tool.diameter_mm / 2.0,
            pass.tolerance_mm,
        ))
    }
}

/// The S9 error for move `mv`, from `start`, which leaves `space`: how
/// much of its swept area lies outside the area the cutter may sweep
/// there.
pub(super) fn off_material(
    op: &ProfileOp,
    shape: &ShapeRef,
    start: Point3,
    mv: Move,
    space: &Space,
    radius_mm: f64,
    tolerance_mm: f64,
) -> CamError {
    let stray = Toolpath {
        moves: vec![Move::rapid(start), mv],
    };
    let swept = footprint_at(&stray, radius_mm, tolerance_mm);
    let allowed: Vec<Region> = space
        .regions
        .iter()
        .flat_map(|r| inset_region(r, -radius_mm, tolerance_mm))
        .collect();
    CamError::OffMaterial {
        op: op.id.clone(),
        shape: shape.clone(),
        area_mm2: area_mm2(&difference(&swept, &allowed, tolerance_mm)),
    }
}

/// A plunge where `requested` was asked for, for `reason`.
pub(super) fn plunged(at: Point, requested: EntryKind, reason: FallbackReason) -> Descent {
    Descent {
        from: at,
        moves: Vec::new(),
        requested,
        used: EntryKind::Plunge,
        reason: Some(reason),
        helix: None,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use super::*;
    use crate::entry::CutSide;
    use dc_geometry::{Contour, FLATTEN_TOLERANCE_DEFAULT_MM, nest};

    const D_MM: f64 = 4.0;

    fn tool() -> EntryTool {
        EntryTool {
            diameter_mm: D_MM,
            feed_mm_min: 800.0,
            plunge_mm_min: 200.0,
            direction: CutDirection::Conventional,
            side: CutSide::Internal,
            quantum_mm: 0.001,
        }
    }

    /// The 20 mm wide rectangle `height_mm` tall at the origin, as a
    /// domain.
    fn rect_domain(height_mm: f64) -> EntryDomain {
        let c = Contour::new(Polyline::rect(Point::new(0.0, 0.0), 20.0, height_mm)).unwrap();
        let (regions, _) = nest(vec![c]).unwrap();
        EntryDomain::new(&regions, D_MM, FLATTEN_TOLERANCE_DEFAULT_MM)
    }

    /// A 10° ramp entering at (10, 0): placed in the 20 mm square above
    /// the tab top, and below it in the square cut down to `tabbed_mm`
    /// tall, which stands in for the domain less the tabs.
    fn entry(tabbed_mm: f64) -> PathEntry {
        let target = Point::new(10.0, 0.0);
        let ramp = Entry::Ramp { angle_deg: 10.0 };
        let plain = Box::new(plan_at_wall(&rect_domain(20.0), &ramp, target, &tool()));
        let tabbed = Box::new(plan_at_wall(
            &rect_domain(tabbed_mm),
            &ramp,
            target,
            &tool(),
        ));
        let after_plain = follow_plain(&plain, &tabbed, &rect_domain(tabbed_mm));
        PathEntry::Wall {
            plain,
            tabbed: Some(tabbed),
            after_plain,
        }
    }

    /// Pass `pass` down to `z_mm`, below the tab top when `below_tabs`.
    fn pass_entry(path: &Polyline, pass: usize, z_mm: f64, below_tabs: bool) -> PassEntry<'_> {
        PassEntry {
            path,
            pass,
            z_mm,
            below_tabs,
            start_z_mm: 13.0,
            top_z_mm: 12.0,
            tool: tool(),
            tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
        }
    }

    /// Every XY a descent's moves reach.
    fn reached(d: &Descent) -> Vec<Point> {
        std::iter::once(d.from)
            .chain(d.moves.iter().map(|m| m.to.xy()))
            .collect()
    }

    /// #352: the passes above the tab top ramp on the plain plan's line;
    /// the first below it, and every one after, ramps along that same
    /// line, cut short of the tabs, though the tabbed plan placed its
    /// ramp on another line. With the line blocked at its start, the pass
    /// below the tab top plunges at the target, warned `no_room`, one
    /// step down from the pass above.
    #[test]
    fn ramp_below_the_tab_top_follows_the_line_the_pass_above_cut() {
        let path = Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0);
        let wall = entry(7.0);
        let PathEntry::Wall { plain, tabbed, .. } = &wall else {
            unreachable!("a wall entry");
        };
        let (start, end) = plain.ramp_line().unwrap();
        assert_ne!(tabbed.as_ref().unwrap().ramp_line(), Some((start, end)));
        let along = (end - start) * (1.0 / start.distance_to(end));
        let mut previous = None;
        let mut from_z_mm = 13.0;
        for (pass, z_mm, below_tabs) in [(0, 8.0, false), (1, 4.0, true), (2, 0.0, true)] {
            let (d, _) = wall.descend(
                &pass_entry(&path, pass, z_mm, below_tabs),
                from_z_mm,
                &mut previous,
            );
            assert_eq!(d.used, EntryKind::Ramp, "pass {pass}");
            for p in reached(&d) {
                assert!(along.cross(p - start).abs() <= 1e-9, "pass {pass}: {p:?}");
                assert!(
                    !below_tabs || p.y <= 7.0 - boundary_safety_mm(D_MM) * along.y + 1e-9,
                    "pass {pass}: {p:?}"
                );
            }
            from_z_mm = z_mm;
        }

        // The tabbed domain too thin for any run along the line.
        let wall = entry(0.4);
        let mut previous = None;
        let (d, _) = wall.descend(&pass_entry(&path, 0, 8.0, false), 13.0, &mut previous);
        assert_eq!(d.used, EntryKind::Ramp);
        let (d, from_z_mm) = wall.descend(&pass_entry(&path, 1, 4.0, true), 8.0, &mut previous);
        assert_eq!(d.used, EntryKind::Plunge);
        assert_eq!(d.reason, Some(FallbackReason::NoRoom));
        assert!(d.moves.is_empty());
        assert!(d.from.distance_to(Point::new(10.0, 0.0)) <= EPS_MM);
        assert!((from_z_mm - 8.0).abs() <= EPS_MM, "one step down");
    }

    /// #352: a ramp after the first pass that the pass above did not cut
    /// (no pass above on this path, as on a last pass after its stock to leave
    /// passes, or one that plunged) plunges; so does one that would fall
    /// less than `RAMP_MIN_LAST_DROP_MM`, whose one run would run the
    /// whole line.
    #[test]
    fn ramp_the_pass_above_did_not_cut_plunges() {
        let path = Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0);
        let wall = entry(7.0);
        for previous in [None, Some(CutEntry::new(false, EntryKind::Plunge))] {
            let mut previous = previous;
            let (d, _) = wall.descend(&pass_entry(&path, 1, 4.0, false), 8.0, &mut previous);
            assert_eq!(d.used, EntryKind::Plunge, "{previous:?}");
            assert_eq!(d.reason, Some(FallbackReason::NoRoom));
        }
        let mut previous = Some(CutEntry::new(false, EntryKind::Ramp));
        let (d, _) = wall.descend(&pass_entry(&path, 1, 7.97, false), 8.0, &mut previous);
        assert_eq!(d.used, EntryKind::Plunge);
        assert_eq!(d.reason, Some(FallbackReason::TooSmall));
    }
}
