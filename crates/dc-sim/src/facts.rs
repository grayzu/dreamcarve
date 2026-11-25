// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The facts of cut stock (phase 6 functional spec, capability 122): how
//! deep it is cut and by which run, where it is cut through, how much
//! was removed, and which runs cut with a plain cylinder.
//!
//! Upstream's statistics count cells and are never shown (the survey,
//! section 5.5); these are in millimetres and are the Result section's
//! and the report's. A cell whose centre is off the board is left out.

#![allow(
    clippy::indexing_slicing,
    reason = "indices come from `Grid::index` over the grid's own counts"
)]

use crate::build::SimRun;
use crate::field::{Heightfield, UNCUT};

/// What the cut stock is like.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Facts {
    /// The deepest cut, mm below the board's top; 0 for uncut stock.
    pub deepest_mm: f64,
    /// The run that made it.
    pub deepest_run: Option<u16>,
    /// The area cut through, mm².
    pub through_mm2: f64,
    /// The runs that cut through, in the plan's order.
    pub through_runs: Vec<u16>,
    /// The volume removed, mm³.
    pub removed_mm3: f64,
    /// The runs whose tool is shown as a plain cylinder, in the plan's
    /// order (capability 110).
    pub plain_runs: Vec<usize>,
}

/// The facts of `field`, cut by `runs`.
#[must_use]
pub fn facts(field: &Heightfield, runs: &[SimRun]) -> Facts {
    let grid = field.grid();
    let (top_mm, cell_mm2) = (field.top_z_mm(), grid.cell_mm() * grid.cell_mm());
    let (heights, owners) = (field.heights(), field.owners());
    let mut facts = Facts {
        plain_runs: runs
            .iter()
            .enumerate()
            .filter(|(_, run)| run.cutter.plain)
            .map(|(k, _)| k)
            .collect(),
        ..Facts::default()
    };
    let mut through = std::collections::BTreeSet::new();
    let (mut removed_mm, mut through_cells) = (0.0, 0_u32);
    for j in 0..grid.ny() {
        for i in 0..grid.nx() {
            let Some(index) = grid.index(i, j) else {
                continue;
            };
            if !grid.on_board(i, j) || owners[index] == UNCUT {
                continue;
            }
            let depth_mm = top_mm - heights[index];
            removed_mm += depth_mm;
            if depth_mm > facts.deepest_mm {
                facts.deepest_mm = depth_mm;
                facts.deepest_run = Some(owners[index]);
            }
            if heights[index] <= 0.0 {
                through_cells += 1;
                through.insert(owners[index]);
            }
        }
    }
    facts.removed_mm3 = removed_mm * cell_mm2;
    facts.through_mm2 = f64::from(through_cells) * cell_mm2;
    facts.through_runs = through.into_iter().collect();
    facts
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "test code reports failure by panicking"
    )]

    use super::*;
    use crate::build::{Outcome, build};
    use crate::cut::SimMove;
    use crate::cutter::{Cutter, CutterShape};
    use crate::grid::{CellSpec, Grid};
    use dc_geometry::{Point3, assert_approx_eq};

    fn cut(runs: &[SimRun]) -> Heightfield {
        let grid = Grid::of(40.0, 30.0, CellSpec::CellMm(0.1)).unwrap();
        let stock = Heightfield::uncut(grid, 12.0).unwrap();
        match build(stock, runs, 0, &mut |_, _| {}, &mut |_| true).unwrap() {
            Outcome::Finished(field) => field,
            Outcome::Stopped => panic!("not told to stop"),
        }
    }

    fn slot(radius_mm: f64, z_mm: f64, y: f64, plain: bool) -> SimRun {
        SimRun {
            cutter: CutterShape {
                cutter: Cutter::Flat { radius_mm },
                plain,
            },
            moves: vec![SimMove::cut(
                Point3::new(10.0, y, z_mm),
                Point3::new(30.0, y, z_mm),
                1.0,
            )],
        }
    }

    #[test]
    fn uncut_stock_has_no_facts() {
        assert_eq!(facts(&cut(&[]), &[]), Facts::default());
    }

    /// A slot 20 mm long cut 3 mm deep with a 6 mm end mill removes its
    /// stadium's area, `20·6 + π·3²`, times 3, to within the cells on
    /// its rim.
    #[test]
    fn removed_volume_of_a_slot() {
        let runs = [slot(3.0, 9.0, 15.0, false)];
        let found = facts(&cut(&runs), &runs);
        let area_mm2 = 20.0 * 6.0 + std::f64::consts::PI * 9.0;
        // The rim is 2·20 + 2π·3 = 58.8 mm long; a cell is 0.1 mm.
        assert_approx_eq!(found.removed_mm3, area_mm2 * 3.0, 58.8 * 0.1 * 3.0);
        assert_approx_eq!(found.deepest_mm, 3.0, 1e-12);
        assert_eq!(found.deepest_run, Some(0));
        assert_approx_eq!(found.through_mm2, 0.0, 0.0);
        assert!(found.through_runs.is_empty() && found.plain_runs.is_empty());
    }

    /// The deepest cut names its run; a cut through the board names the
    /// runs that went through and its area; a run with a plain cylinder
    /// is listed.
    #[test]
    fn the_deepest_cut_and_the_through_cut_name_their_runs() {
        let runs = [
            slot(3.0, 9.0, 8.0, false),
            slot(2.0, -0.2, 20.0, true),
            slot(1.0, 4.0, 8.0, false),
        ];
        let found = facts(&cut(&runs), &runs);
        assert_approx_eq!(found.deepest_mm, 12.0, 1e-12);
        assert_eq!(found.deepest_run, Some(1));
        assert_eq!(found.through_runs, [1]);
        let stadium_mm2 = 20.0 * 4.0 + std::f64::consts::PI * 4.0;
        assert_approx_eq!(found.through_mm2, stadium_mm2, 52.6 * 0.1);
        assert_eq!(found.plain_runs, [1]);
    }
}
