// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The stock's heights (`docs/specs/phase-6/design.md`, "Data model →
//! dc-sim"): one height a cell, from the board's bottom, and the run that
//! cut the cell last.
//!
//! Heights are `f64`, as all geometry is (`docs/rust-guidelines.md`,
//! "Types"); the renderer narrows them at its own edge. A cell is never
//! above the board's top and never below its bottom: a cut that would go
//! lower leaves the cell at 0, which is **cut through**.

#![allow(
    clippy::indexing_slicing,
    reason = "every index comes from `Grid::index` or a range the grid's own counts bound"
)]

use crate::SimError;
use crate::grid::Grid;

/// The owner of a cell no run has cut, in [`Heightfield::owners`].
pub const UNCUT: u16 = u16::MAX;

/// The heights a board's runs leave, mm from the board's bottom, with
/// the run that lowered each cell last.
#[derive(Debug, Clone, PartialEq)]
pub struct Heightfield {
    grid: Grid,
    top_z_mm: f64,
    /// Row-major from the lowest Y; `top_z_mm` where nothing has cut.
    z_mm: Vec<f64>,
    /// [`UNCUT`], or the run that lowered the cell last.
    cut_by: Vec<u16>,
}

impl Heightfield {
    /// Uncut stock `top_z_mm` thick over `grid`.
    ///
    /// # Errors
    ///
    /// [`SimError::BadThickness`] for a thickness that is not finite or
    /// not above 0.
    pub fn uncut(grid: Grid, top_z_mm: f64) -> Result<Self, SimError> {
        if !(top_z_mm.is_finite() && top_z_mm > 0.0) {
            return Err(SimError::BadThickness {
                thickness_mm: top_z_mm,
            });
        }
        Ok(Self {
            grid,
            top_z_mm,
            z_mm: vec![top_z_mm; grid.len()],
            cut_by: vec![UNCUT; grid.len()],
        })
    }

    /// The cells.
    #[must_use]
    pub const fn grid(&self) -> &Grid {
        &self.grid
    }

    /// The board's top, mm above its bottom: its thickness.
    #[must_use]
    pub const fn top_z_mm(&self) -> f64 {
        self.top_z_mm
    }

    /// The height of cell `(i, j)`, mm; `None` for a cell the grid does
    /// not have.
    #[must_use]
    pub fn z_mm(&self, i: u32, j: u32) -> Option<f64> {
        self.grid.index(i, j).map(|k| self.z_mm[k])
    }

    /// The run that lowered cell `(i, j)` last, as its place in the
    /// plan; `None` for an uncut cell and for one the grid does not have.
    #[must_use]
    pub fn cut_by(&self, i: u32, j: u32) -> Option<u16> {
        let owner = self.cut_by[self.grid.index(i, j)?];
        (owner != UNCUT).then_some(owner)
    }

    /// Whether cell `(i, j)` is cut through: at the board's bottom.
    #[must_use]
    pub fn is_through(&self, i: u32, j: u32) -> bool {
        self.z_mm(i, j).is_some_and(|z| z <= 0.0)
    }

    /// Every height, row by row from the lowest Y, mm: what a renderer
    /// uploads.
    #[must_use]
    pub fn heights(&self) -> &[f64] {
        &self.z_mm
    }

    /// Every cell's owner, row by row from the lowest Y: [`UNCUT`], or
    /// the run that lowered the cell last.
    #[must_use]
    pub fn owners(&self) -> &[u16] {
        &self.cut_by
    }

    /// Makes this stock a copy of `from`, which must be on the same
    /// grid. Returns whether anything changed; on another grid nothing
    /// is copied.
    pub fn restore(&mut self, from: &Self) -> bool {
        if self.grid != from.grid || (self.z_mm == from.z_mm && self.cut_by == from.cut_by) {
            return false;
        }
        self.top_z_mm = from.top_z_mm;
        self.z_mm.clone_from(&from.z_mm);
        self.cut_by.clone_from(&from.cut_by);
        true
    }

    /// The bytes its heights and owners take.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.z_mm.len() * size_of::<f64>() + self.cut_by.len() * size_of::<u16>()
    }

    /// Lowers the cell at `index` to `z_mm` for run `owner`, if that is
    /// lower than it stands; never below the bottom. Returns whether the
    /// cell changed. `index` is from [`Grid::index`].
    pub(crate) fn lower(&mut self, index: usize, z_mm: f64, owner: u16) -> bool {
        let z_mm = z_mm.max(0.0);
        if z_mm < self.z_mm[index] {
            self.z_mm[index] = z_mm;
            self.cut_by[index] = owner;
            true
        } else {
            false
        }
    }
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
    use crate::grid::CellSpec;
    use dc_geometry::assert_approx_eq;

    fn field() -> Heightfield {
        let grid = Grid::of(4.0, 2.0, CellSpec::CellMm(1.0)).unwrap();
        Heightfield::uncut(grid, 12.0).unwrap()
    }

    #[test]
    fn uncut_stock_is_at_its_top_and_cut_by_nothing() {
        let field = field();
        assert_eq!(field.heights().len(), 8);
        for j in 0..2 {
            for i in 0..4 {
                assert_approx_eq!(field.z_mm(i, j).unwrap(), 12.0, 0.0);
                assert_eq!(field.cut_by(i, j), None, "cell {i}, {j}");
                assert!(!field.is_through(i, j));
            }
        }
        assert_eq!(field.z_mm(4, 0), None);
        assert_eq!(field.z_mm(0, 2), None);
        assert!(field.owners().iter().all(|&o| o == UNCUT));
    }

    /// Acceptance 138: a cell only ever goes down, is never below the
    /// bottom, and at the bottom is cut through; its owner is the last
    /// run to lower it, not the last to pass over it.
    #[test]
    fn cells_only_go_down_and_never_below_the_bottom() {
        let mut field = field();
        let k = field.grid().index(2, 1).unwrap();
        assert!(field.lower(k, 8.0, 0));
        assert!(!field.lower(k, 9.0, 1), "a higher cut changes nothing");
        assert!(!field.lower(k, 8.0, 1), "an equal cut changes nothing");
        assert_approx_eq!(field.z_mm(2, 1).unwrap(), 8.0, 0.0);
        assert_eq!(field.cut_by(2, 1), Some(0));
        assert!(field.lower(k, -0.2, 3), "a through cut lowers it");
        assert_approx_eq!(field.z_mm(2, 1).unwrap(), 0.0, 0.0);
        assert!(field.is_through(2, 1));
        assert_eq!(field.cut_by(2, 1), Some(3));
        assert!(!field.lower(k, -5.0, 4), "nothing is below the bottom");
        assert_eq!(field.cut_by(2, 1), Some(3));
    }

    #[test]
    fn a_board_with_no_thickness_is_refused() {
        let grid = Grid::of(4.0, 2.0, CellSpec::CellMm(1.0)).unwrap();
        for thickness_mm in [0.0, -3.0, f64::NAN, f64::INFINITY] {
            assert!(
                matches!(
                    Heightfield::uncut(grid, thickness_mm),
                    Err(SimError::BadThickness { .. })
                ),
                "{thickness_mm}"
            );
        }
    }
}
