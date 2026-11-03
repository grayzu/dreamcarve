// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The seat check of an inlay pair (#452), built on the heightfield
//! simulation of [`crate::heightfield`]: the programs themselves, not the
//! outlines they were built from, decide whether the plug seats. Shared
//! by the Inlay generator's tests and, with the `test-support` feature,
//! the stress tests in `dc-gcode` (#464).

#![allow(
    clippy::indexing_slicing,
    reason = "grid indices are within the grid; a test helper reports failure by panicking"
)]

use crate::heightfield::{Cutter, Heightfield};
use crate::run::{RunLabel, ToolRun};
use dc_geometry::{Point, Polyline, Rect, Region, nearest_point};
use dc_project::{Board, InlayOp, Project, ToolKind};
use std::collections::VecDeque;

/// How far the plug may stand proud of its female, mm: from the male's
/// own cuts (#452), and in all (#457).
pub const SEAT_MM: f64 = 0.05;

/// How far above its start plane the male's band floor may read and still
/// part the plug from the board round it, mm: the band's edges meet the
/// start plane exactly.
const FLOOR_SLACK_MM: f64 = 0.01;

/// One cell of the plug, seated ([`seat`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SeatCell {
    /// The cell's centre, mm, in the female's frame.
    pub at: Point,
    /// How far this cell holds the plug's start plane above the female's
    /// surface, mm: the male's height above its start plane less the
    /// female's depth, both here. Negative where they clear.
    pub need_mm: f64,
    /// The male's height above its start plane here, mm.
    pub plug_mm: f64,
}

/// The plug's cells, seated on the female (#452). `female` and `male`
/// share one grid, the male's mirrored. The plug is every cell not joined
/// to the grid's border through male wood standing above the start plane
/// (by more than [`FLOOR_SLACK_MM`]), cell to cell through sides or
/// corners (#501): the band's floor rings the plug, and
/// the board beyond it, V wall and all, falls away when the piece is cut
/// out. Wood left above the start plane in the band but joined to the
/// plug is part of it, as it would bind. The plug's start plane stops the
/// largest `need_mm` above the female's surface.
pub(crate) fn seat(
    female: &Heightfield,
    male: &Heightfield,
    start_plane_z_mm: f64,
) -> Vec<SeatCell> {
    let (nx, ny) = male.size();
    assert_eq!(female.size(), (nx, ny), "one grid for both sides");
    let standing = |i: usize, j: usize| male.z_mm(i, j) > start_plane_z_mm + FLOOR_SLACK_MM;
    let mut outside = vec![false; nx * ny];
    let mut queue = VecDeque::new();
    for j in 0..ny {
        for i in 0..nx {
            let border = i == 0 || j == 0 || i == nx - 1 || j == ny - 1;
            if border && standing(i, j) {
                outside[j * nx + i] = true;
                queue.push_back((i, j));
            }
        }
    }
    while let Some((i, j)) = queue.pop_front() {
        // #501: through the corners too. A ridge of the board thinner than
        // a cell that runs across the grid has cells that touch only at
        // their corners; joined through the sides alone it falls apart
        // into cells counted as plug. The band, a margin wide and cut to
        // its floor, keeps the plug's wood from ever coming that near the
        // board's.
        let (west, south) = (i.wrapping_sub(1), j.wrapping_sub(1));
        let neighbours = [
            (west, j),
            (i + 1, j),
            (i, south),
            (i, j + 1),
            (west, south),
            (i + 1, south),
            (west, j + 1),
            (i + 1, j + 1),
        ];
        for (a, b) in neighbours {
            if a < nx && b < ny && !outside[b * nx + a] && standing(a, b) {
                outside[b * nx + a] = true;
                queue.push_back((a, b));
            }
        }
    }
    let mut cells = Vec::new();
    for j in 0..ny {
        for i in 0..nx {
            if outside[j * nx + i] {
                continue;
            }
            let plug_mm = male.z_mm(i, j) - start_plane_z_mm;
            cells.push(SeatCell {
                at: female.cell(i, j),
                need_mm: plug_mm - (female.top_z_mm - female.z_mm(i, j)),
                plug_mm,
            });
        }
    }
    cells
}

/// How an inlay pair seats (#452), from its two sides' programs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Fit {
    /// How far the plug's start plane stops above the female's surface,
    /// mm, and the cell (female's frame) that stops it.
    pub seat_mm: f64,
    /// That cell's centre.
    pub seat_at: Point,
    /// The male's share of it, mm: the most, over the cells that hold the
    /// plug up more than [`SEAT_MM`], by which the plug stands higher
    /// above its start plane than the pocket the female's design describes
    /// is deep there, `min(e / s, D)` for a point `e` inside the design;
    /// and that cell. Where the female's own V-Carve falls short of its
    /// design, this is what the male adds to it.
    pub male_mm: f64,
    /// That cell's centre.
    pub male_at: Point,
}

impl Fit {
    /// Whether the plug seats within [`SEAT_MM`]: the male's share and the
    /// whole seat both.
    #[must_use]
    pub fn is_flush(&self) -> bool {
        self.male_mm <= SEAT_MM && self.seat_mm <= SEAT_MM
    }
}

/// The cutter a run of `project` cuts with, as the heightfield models it.
pub(crate) fn cutter_of(project: &Project, run: &ToolRun) -> Cutter {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == run.tool)
        .expect("the run's tool is in the project");
    match tool.cutter {
        ToolKind::VBit { included_angle_deg } => Cutter::VBit {
            slope: (included_angle_deg.to_radians() / 2.0).tan(),
        },
        _ => Cutter::Flat {
            radius_mm: tool.diameter_mm / 2.0,
        },
    }
}

/// Every outline of `regions`.
pub(crate) fn outlines(regions: &[Region]) -> Vec<Polyline> {
    regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(|c| c.polyline().clone())
        .collect()
}

/// The distance from `p` to the nearest outline of `contours`, mm.
pub(crate) fn distance_to(contours: &[Polyline], p: Point) -> f64 {
    contours
        .iter()
        .filter_map(|c| nearest_point(c, p))
        .map(|n| n.gap_mm)
        .fold(f64::INFINITY, f64::min)
}

/// The signed distance from `p` into `design`, mm: positive inside.
pub(crate) fn inset_mm(design: &[Region], p: Point) -> f64 {
    let gap_mm = distance_to(&outlines(design), p);
    if design.iter().any(|r| r.contains_point(p)) {
        gap_mm
    } else {
        -gap_mm
    }
}

/// The fit of Inlay `op`'s two boards in `project` (which holds it), from
/// the runs it generated, both boards' (#498: one project; the female's
/// runs on the stock, the male's on its male board, told apart by
/// [`ToolRun::board`]): both boards' runs simulated as heightfields
/// ([`seat`]), cells `step_mm` apart, over the design's box grown by the
/// margin and 2 mm, the male mirrored as it seats. The two boards share
/// their bottom-left corner, so their cells line up.
pub fn fit(project: &Project, op: &InlayOp, runs: &[ToolRun], step_mm: f64) -> Fit {
    let female_runs: Vec<ToolRun> = runs
        .iter()
        .filter(|r| r.board == Board::Female)
        .cloned()
        .collect();
    let male_runs: Vec<ToolRun> = runs
        .iter()
        .filter(|r| r.board == Board::Male)
        .cloned()
        .collect();
    let (female_project, female, female_runs) = (project, op, female_runs.as_slice());
    let male_project = &project.on_male_board(op);
    let (male, male_runs) = (op, male_runs.as_slice());
    let resolve = |board: Board| {
        crate::inlay::side(project, op, board, &mut Vec::new()).expect("the board resolves")
    };
    let female_side = resolve(Board::Female);
    let male_side = resolve(Board::Male);
    // The mirror is about the middle of the design's box, so the mirrored
    // design's box is the design's.
    let mut bounds: Option<Rect> = None;
    for outline in outlines(&male_side.design) {
        let b = outline.bounds().expect("an outline has a box");
        bounds = Some(bounds.map_or(b, |all| all.including(b.min).including(b.max)));
    }
    let grow_mm = male.margin_mm.expect("a male has a margin") + 2.0;
    let b = bounds.expect("the design has an outline");
    let bounds = Rect {
        min: Point::new(b.min.x - grow_mm, b.min.y - grow_mm),
        max: Point::new(b.max.x + grow_mm, b.max.y + grow_mm),
    };
    let top_mm = |p: &Project| p.stock.thickness_mm.expect("stock has a thickness");
    let mut female_field = Heightfield::new(bounds, step_mm, top_mm(female_project), None);
    female_field.cut_runs(female_runs, |run| cutter_of(female_project, run));
    let mut male_field =
        Heightfield::new(bounds, step_mm, top_mm(male_project), male_side.mirror_x_mm);
    male_field.cut_runs(male_runs, |run| cutter_of(male_project, run));
    let cells = seat(&female_field, &male_field, male_side.start_plane_z_mm);
    let worst = cells
        .iter()
        .max_by(|a, b| a.need_mm.total_cmp(&b.need_mm))
        .expect("the plug has cells");
    let main = female_runs
        .iter()
        .find(|r| r.label == RunLabel::Main)
        .expect("a V-bit run");
    let Cutter::VBit { slope } = cutter_of(female_project, main) else {
        panic!("the female's main run is a V-bit's");
    };
    let depth_mm = female.depth_mm.expect("a depth");
    let share = |c: &SeatCell| {
        let pocket_mm = (inset_mm(&female_side.targets.regions, c.at) / slope).clamp(0.0, depth_mm);
        (c.plug_mm - pocket_mm, c.at)
    };
    // Where the plug seats within `SEAT_MM` everywhere, the male's share
    // at the cell that holds it highest.
    let male_share = cells
        .iter()
        .filter(|c| c.need_mm > SEAT_MM)
        .map(share)
        .max_by(|a, b| a.0.total_cmp(&b.0))
        .unwrap_or_else(|| share(worst));
    Fit {
        seat_mm: worst.need_mm,
        seat_at: worst.at,
        male_mm: male_share.0,
        male_at: male_share.1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// #501 (line art `0x30`'s V-wall male, 15° bit): a thin ridge of the
    /// board standing in the band is part of the board, not the plug, when
    /// it runs at 45° to the grid. A sliver of board 0.13 to 0.3 mm wide
    /// pokes in from the band's outer edge, and the V wall cuts it down
    /// from both sides to a ridge up to 0.8 mm above the start plane; on a
    /// 0.1 mm grid its cells touch only at their corners. The flood fill
    /// that finds the board joined cells through their sides alone, so it
    /// cut the ridge into single cells and counted each as plug: the plug
    /// stood 0.332 mm proud on a ridge that falls away with the board (on
    /// a 0.05 mm grid, whose cells join along it, it seated within
    /// 0.073 mm). Here a 20 × 20 grid has board along its left and bottom
    /// edges, a diagonal ridge 0.5 mm above the start plane running in
    /// from the corner, and a plug of four cells 0.02 mm up. The seat is
    /// the plug's 0.02 mm.
    #[test]
    fn diagonal_ridge_of_the_board_is_not_plug() {
        let bounds = Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(1.9, 1.9),
        };
        let (top_mm, start_mm, floor_mm) = (10.0, 7.0, 6.5);
        let female = Heightfield::new(bounds, 0.1, top_mm, None);
        let mut male = Heightfield::new(bounds, 0.1, top_mm, None);
        let (nx, ny) = male.size();
        for j in 0..ny {
            for i in 0..nx {
                let board = i == 0 || j == 0;
                let ridge = i == j && i < 9;
                let plug = (14..16).contains(&i) && (14..16).contains(&j);
                let z_mm = if board {
                    top_mm
                } else if ridge {
                    start_mm + 0.5
                } else if plug {
                    start_mm + 0.02
                } else {
                    floor_mm
                };
                male.set_z_mm(i, j, z_mm);
            }
        }
        let cells = seat(&female, &male, start_mm);
        let worst = cells
            .iter()
            .max_by(|a, b| a.need_mm.total_cmp(&b.need_mm))
            .expect("the plug has cells");
        assert!(
            (worst.need_mm - 0.02).abs() < 1e-9,
            "the plug stands {:.3} mm proud at {:?}",
            worst.need_mm,
            worst.at
        );
    }
}
