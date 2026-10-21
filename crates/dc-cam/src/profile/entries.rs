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
//! below their tops; here each path places its entry twice at most, in its
//! waste domain and in that domain less the tab keep-outs, and each pass
//! takes the one for its Z.

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

/// What [`path_entries`] needs from the generator.
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

/// An entry placed in one domain: the domain, its regions (for the S9
/// report) and the plan.
pub(super) struct Placed {
    domain: EntryDomain,
    regions: Vec<Region>,
    plan: EntryPlan,
}

/// How one path enters its passes.
pub(super) enum PathEntry {
    /// On line: the ramp along the path at `angle_deg`; below the tab top
    /// it stops `clear_mm` along, short of the first tab.
    Along { angle_deg: f64, clear_mm: f64 },
    /// Inside or Outside: the entry placed in the waste domain, and, when
    /// some pass is below the tab top, in that domain less the tabs.
    Wall {
        plain: Box<Placed>,
        tabbed: Option<Box<Placed>>,
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
/// cuts as phase 1 did.
pub(super) fn path_entries(setup: &EntrySetup<'_, '_>) -> Vec<Option<PathEntry>> {
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
        (Side::Inside | Side::Outside, _) => wall_entries(setup),
    }
}

/// Inside and Outside: each path's plans.
fn wall_entries(setup: &EntrySetup<'_, '_>) -> Vec<Option<PathEntry>> {
    let tool_for = |i: usize| {
        let wall = setup.walls.get(i).map_or(Wall::Outside, |(w, _)| *w);
        (setup.entry_tool)(wall)
    };
    let radius_mm = tool_for(0).diameter_mm / 2.0;
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
    setup
        .paths
        .iter()
        .zip(domains)
        .enumerate()
        .map(|(i, ((path, _), regions))| {
            let target = path.vertices().first()?.point();
            let tool = tool_for(i);
            let place = |regions: Vec<Region>| {
                let domain = EntryDomain::new(&regions, tool.diameter_mm, tolerance_mm);
                let plan = plan_at_wall(&domain, &setup.op.entry, target, &tool);
                Box::new(Placed {
                    domain,
                    regions,
                    plan,
                })
            };
            let tabbed = (!keep_out.is_empty())
                .then(|| place(difference(&regions, &keep_out, tolerance_mm)));
            Some(PathEntry::Wall {
                plain: place(regions),
                tabbed,
            })
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
                let (key, placed) = match tabbed {
                    Some(t) if pass.below_tabs => (true, t),
                    _ => (false, plain),
                };
                let plan = &placed.plan;
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
    /// tool centre lies in the domain the entry was placed in, or the
    /// generator has a defect and no toolpath is produced
    /// ([`CamError::OffMaterial`]). On line descents follow the path and
    /// have no domain.
    pub(super) fn check(
        &self,
        op: &ProfileOp,
        shape: &ShapeRef,
        pass: &PassEntry<'_>,
        descent: &Descent,
        from_z_mm: f64,
    ) -> Result<(), CamError> {
        let Self::Wall { plain, tabbed } = self else {
            return Ok(());
        };
        let placed = match tabbed {
            Some(t) if pass.below_tabs => t,
            _ => plain,
        };
        let from = Point3::new(descent.from.x, descent.from.y, from_z_mm);
        let slack_mm = pass.tolerance_mm + MIN_BOUNDARY_SAFETY_MM;
        let Some(i) = placed
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
        let radius_mm = pass.tool.diameter_mm / 2.0;
        let stray = Toolpath {
            moves: vec![Move::rapid(start), *mv],
        };
        let swept = footprint_at(&stray, radius_mm, pass.tolerance_mm);
        let allowed: Vec<Region> = placed
            .regions
            .iter()
            .flat_map(|r| inset_region(r, -radius_mm, pass.tolerance_mm))
            .collect();
        Err(CamError::OffMaterial {
            op: op.id.clone(),
            shape: shape.clone(),
            area_mm2: area_mm2(&difference(&swept, &allowed, pass.tolerance_mm)),
        })
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
