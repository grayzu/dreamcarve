// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What a project's operations ask of a board (phase 6 functional spec,
//! capability 119; `docs/specs/phase-6/design.md`, "Algorithms → The
//! design surface"): the surface their shapes and depths describe, cut
//! with a perfectly sharp tool, for the check for uncut material to
//! compare the simulated stock with.
//!
//! Each operation's shapes are found from the project, the way the
//! safety pass finds an operation's material (`dc_gcode::material`):
//! its targets' outlines as placed, closed ones nested into regions with
//! their islands. Nothing is read from a generator but the stretches of
//! a Profile's path that ride over its tabs, which the preview finds for
//! the Viewport already.
//!
//! | Operation | What it asks for |
//! | --- | --- |
//! | Pocket | Its regions at its depth |
//! | Profile | A trench its tool's width wide beside its outlines (Inside, Outside), or what its tool sweeps along them (On line, Left, Right), at its depth, less its tabs |
//! | Bore | Its circles at its depth |
//! | V-Carve | In its regions, as deep as a sharp V of its bit's angle reaches from the nearest edge, no deeper than its flat depth |
//! | Drill, Engrave, Inlay, Photo V-Carve | Nothing |

use dc_geometry::{Contour, Polyline, Region, as_circle, nest, offset_open};
use dc_project::{Board, InstanceId, Operation, Project, Side, Tool};
use dc_sim::{BandSide, Design, Grid};

use super::BoardPlan;
use crate::EmitOp;
use crate::preview::{Run, marks};

/// The design surface of `board` on `grid`: what `project`'s enabled
/// operations ask of it. `generated` are the project's runs, read for
/// where each Profile's path rides over its tabs. An operation is known
/// to the design by its place in the project's list.
#[must_use]
pub fn design(project: &Project, board: &BoardPlan, generated: &[EmitOp], grid: Grid) -> Design {
    let top_mm = board.thickness_mm;
    let mut design = Design::uncut(grid, top_mm);
    // Only the stock is asked anything: the male board is an Inlay's.
    if board.board != Board::Female {
        return design;
    }
    for (index, op) in project.operations.iter().enumerate() {
        let Ok(number) = u16::try_from(index) else {
            break;
        };
        if !op.enabled() {
            continue;
        }
        let tool = project.tools.iter().find(|t| &t.id == op.tool());
        match op {
            Operation::Pocket(pocket) => {
                if let (Some(depth_mm), Some(regions)) =
                    (pocket.depth_mm, regions_of(project, &pocket.targets))
                {
                    design.lower_regions(&regions, top_mm - depth_mm, number);
                }
            }
            Operation::Bore(bore) => {
                if let (Some(depth_mm), Some(paths)) =
                    (bore.depth_mm, placed(project, &bore.targets))
                {
                    let holes: Vec<Polyline> = paths
                        .iter()
                        .filter_map(|p| as_circle(p, project.flatten_tolerance_mm))
                        .map(|(centre, radius_mm)| Polyline::circle(centre, radius_mm))
                        .collect();
                    if let Some(regions) = nested(holes) {
                        design.lower_regions(&regions, top_mm - depth_mm, number);
                    }
                }
            }
            Operation::Profile(profile) => {
                let (Some(depth_mm), Some(tool)) = (profile.depth_mm, tool) else {
                    continue;
                };
                let z_mm = top_mm - depth_mm;
                let radius_mm = tool.diameter_mm / 2.0;
                match profile.side {
                    Side::Inside | Side::Outside => {
                        let side = if profile.side == Side::Inside {
                            BandSide::Inside
                        } else {
                            BandSide::Outside
                        };
                        if let Some(regions) = regions_of(project, &profile.targets) {
                            design.lower_band(&regions, tool.diameter_mm, side, z_mm, number);
                        }
                    }
                    Side::OnLine => {
                        if let Some(paths) = placed(project, &profile.targets) {
                            design.lower_near(&paths, radius_mm, z_mm, number);
                        }
                    }
                    Side::Left | Side::Right => {
                        let left_mm = if profile.side == Side::Left {
                            radius_mm
                        } else {
                            -radius_mm
                        };
                        if let Some(paths) = placed(project, &profile.targets) {
                            let beside: Vec<Polyline> = paths
                                .iter()
                                .filter(|p| !p.is_closed())
                                .flat_map(|p| offset_open(p, left_mm))
                                .collect();
                            design.lower_near(&beside, radius_mm, z_mm, number);
                        }
                    }
                }
                if let Some(tab_top_mm) = tab_top_mm(project, op) {
                    for run in generated
                        .iter()
                        .filter(|r| r.op.id() == op.id() && r.board == board.board)
                    {
                        let rides = marks(&Run::from(run)).tab_marks;
                        // A cell of margin: the trench's edge is the grid's.
                        design.raise_near(&rides, radius_mm + grid.cell_mm(), tab_top_mm, number);
                    }
                }
            }
            Operation::VCarve(carve) => {
                let slope = tool.and_then(Tool::carve_profile).map(|p| p.slope());
                if let (Some(slope), Some(regions)) = (slope, regions_of(project, &carve.targets)) {
                    design.lower_cone(
                        &regions,
                        top_mm - carve.start_depth_mm,
                        slope,
                        carve.flat_depth_mm,
                        number,
                    );
                }
            }
            Operation::Drill(_)
            | Operation::Engrave(_)
            | Operation::Inlay(_)
            | Operation::PhotoVCarve(_) => {}
        }
    }
    design
}

/// The top of the tallest tab `op` leaves, mm above the board's bottom:
/// of its placed tabs and of its tab request. `None` for an operation
/// with no tab.
fn tab_top_mm(project: &Project, op: &Operation) -> Option<f64> {
    let Operation::Profile(profile) = op else {
        return None;
    };
    let placed = project
        .tabs
        .iter()
        .filter(|tab| profile.targets.contains(&tab.instance))
        .map(|tab| tab.height_mm);
    let asked = profile.tabs.as_ref().map(|spec| spec.height_mm);
    placed.chain(asked).fold(None, |top: Option<f64>, h| {
        Some(top.map_or(h, |t| t.max(h)))
    })
}

/// The outlines of `targets` as placed on the board; `None` when a
/// target or its definition is not in the project.
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

/// The closed outlines of `targets` as regions, islands as holes.
fn regions_of(project: &Project, targets: &[InstanceId]) -> Option<Vec<Region>> {
    nested(
        placed(project, targets)?
            .into_iter()
            .filter(Polyline::is_closed)
            .collect(),
    )
}

/// `outlines` nested into regions; `None` when one is no simple outline.
fn nested(outlines: Vec<Polyline>) -> Option<Vec<Region>> {
    let contours: Option<Vec<Contour>> =
        outlines.into_iter().map(|p| Contour::new(p).ok()).collect();
    let (regions, _) = nest(contours?).ok()?;
    Some(regions)
}
