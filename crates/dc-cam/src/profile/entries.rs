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
    MIN_BOUNDARY_SAFETY_MM, boundary_safety_mm, plan_at_wall,
};
use crate::error::{CamError, ShapeRef};
use crate::footprint::footprint_at;
use crate::pocket::rings::inset_region;
use crate::tabs::{PathTabs, Tabs};
use crate::toolpath::{Move, Toolpath};
use dc_geometry::{EPS_MM, Point, Point3, Polyline, Region, area_mm2, difference};
use dc_project::{Entry, ProfileOp, Side, Stock};

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
    pub entry_tool: &'a dyn Fn(Wall) -> EntryTool,
    pub tolerance_mm: f64,
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
    Wall {
        plain: Box<EntryPlan>,
        tabbed: Option<Box<EntryPlan>>,
    },
}

/// One pass of one path, for [`PathEntry::descend`].
pub(super) struct PassEntry<'a> {
    pub path: &'a Polyline,
    pub pass: usize,
    pub z_mm: f64,
    /// The pass is below the tab top, so the tabs stand.
    pub below_tabs: bool,
    pub safe_z_mm: f64,
    pub tool: EntryTool,
    pub tolerance_mm: f64,
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
        (Side::OnLine, Entry::Ramp { angle_deg }) => setup
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
        (Side::OnLine, Entry::Helix { .. }) => (0..n).map(|_| None).collect(),
        (Side::Inside | Side::Outside, _) => (0..n)
            .map(|i| {
                let (domain, target) = (domains.get(i)?, targets.get(i)?);
                let tool = tool_for(setup, i);
                let place = |space: &Space, at: Point| {
                    Box::new(plan_at_wall(&space.domain, &op.entry, at, &tool))
                };
                Some(PathEntry::Wall {
                    plain: place(&domain.plain, target.plain),
                    tabbed: domain.tabbed.as_ref().map(|t| place(t, target.tabbed)),
                })
            })
            .collect(),
    }
}

/// The entry tool of path `i`.
fn tool_for(setup: &EntrySetup<'_, '_>, i: usize) -> EntryTool {
    let wall = setup.walls.get(i).map_or(Wall::Outside, |(w, _)| *w);
    (setup.entry_tool)(wall)
}

/// Inside and Outside: each path's waste domain ([`waste::domains`]) and,
/// when some pass is below the tab top, that domain less every path's tab
/// keep-outs ([`waste::tab_keep_out`]). Empty for On line.
pub(super) fn wall_domains(setup: &EntrySetup<'_, '_>) -> Vec<PathDomain> {
    if setup.op.side == Side::OnLine {
        return Vec::new();
    }
    let radius_mm = tool_for(setup, 0).diameter_mm / 2.0;
    let tolerance_mm = setup.tolerance_mm;
    let wall_paths: Vec<WallPath<'_>> = setup
        .paths
        .iter()
        .zip(setup.walls)
        .map(|((path, _), (_, region))| WallPath {
            path,
            region: *region,
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
    /// The descent of one pass that starts at `from_z_mm` (safe Z on the
    /// first pass, the previous pass Z after), and the Z it starts at.
    /// `previous` is which plan (tabbed or not) and entry the path's
    /// previous pass cut, updated here: a helix after the first pass
    /// rejoins the bore its previous pass cut, or starts again from safe Z
    /// when that pass cut none.
    pub(super) fn descend(
        &self,
        pass: &PassEntry<'_>,
        from_z_mm: f64,
        previous: &mut Option<(bool, EntryKind)>,
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
                *previous = Some((pass.below_tabs, d.used));
                (d, from_z_mm)
            }
            Self::Wall { plain, tabbed } => {
                let (key, plan) = match tabbed {
                    Some(t) if pass.below_tabs => (true, t),
                    _ => (false, plain),
                };
                let mut from_z = from_z_mm;
                let mut d = plan.descend(from_z, pass.z_mm);
                if pass.pass > 0 && d.used == EntryKind::Helix {
                    if *previous == Some((key, EntryKind::Helix)) {
                        d = plan.rejoin(d, from_z);
                    } else {
                        // The previous pass cut no bore here: helix from
                        // safe Z (the sequencer retracts, since the helix
                        // does not start where the lap ended), or ramp
                        // from where the tool is.
                        let restart = plan.descend(pass.safe_z_mm, pass.z_mm);
                        if restart.used == EntryKind::Helix {
                            from_z = pass.safe_z_mm;
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
                *previous = Some((key, d.used));
                (d, from_z)
            }
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
