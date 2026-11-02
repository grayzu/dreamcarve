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
use dc_project::{InlayOp, Project, ToolKind};
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
/// (by more than [`FLOOR_SLACK_MM`]): the band's floor rings the plug, and
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
        let neighbours = [
            (i.wrapping_sub(1), j),
            (i + 1, j),
            (i, j.wrapping_sub(1)),
            (i, j + 1),
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

/// The fit of the female `female` and the male `male`, from the runs each
/// generated (`female_runs` in `female_project`, `male_runs` in
/// `male_project`; each project holds its operation): both sides' runs
/// simulated as heightfields ([`seat`]), cells `step_mm` apart, over the
/// design's box grown by the margin and 2 mm, the male mirrored as it
/// seats.
pub fn fit(
    (female_project, female, female_runs): (&Project, &InlayOp, &[ToolRun]),
    (male_project, male, male_runs): (&Project, &InlayOp, &[ToolRun]),
    step_mm: f64,
) -> Fit {
    let resolve = |project: &Project, op: &InlayOp| {
        crate::inlay::side(project, op, &mut Vec::new()).expect("the side resolves")
    };
    let female_side = resolve(female_project, female);
    let male_side = resolve(male_project, male);
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
