// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/simulation/grid.ts
//   src/engine/simulation/types.ts

//! The cells a board is divided into (`docs/specs/phase-6/design.md`,
//! "Algorithms → The grid"; functional capability 112).
//!
//! Upstream's rule is kept: square cells, counted along the board's
//! longer side, a cell's height read at its centre, rows growing with Y.
//! Re-derived or changed: rows grow with Y-up here, which needs no flip,
//! since the grid has no handedness; upstream's floor of 24 cells on each
//! axis is not ported, because it makes a narrow board's grid reach past
//! the board as stock that is not there (the survey, section 2.2); a
//! least cell size and a cap on the cell count are added (ADR-0003: the
//! resolution is a user setting with a budget).

use dc_geometry::{MAX_COORDINATE_MM, Point};

use crate::SimError;

/// The smallest cell a simulation uses, mm. A small board at a fine
/// quality is held to it.
pub const MIN_CELL_MM: f64 = 0.02;

/// The most cells a grid may have: 16 777 216, eight bytes of height
/// each.
pub const MAX_CELLS: usize = 1 << 24;

/// Slack on a cell count, in cells: a side that is a whole number of
/// cells but for rounding does not gain one.
const COUNT_SLACK: f64 = 1e-9;

/// How finely the app divides a board, as cells along its longer side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Quality {
    /// 512 cells.
    Draft,
    /// 1024 cells.
    #[default]
    Standard,
    /// 2048 cells.
    Fine,
}

impl Quality {
    /// The cells along the board's longer side.
    #[must_use]
    pub const fn cells(self) -> u32 {
        match self {
            Self::Draft => 512,
            Self::Standard => 1024,
            Self::Fine => 2048,
        }
    }
}

/// How many cells a grid has: a named quality, or a cell size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellSpec {
    /// A quality: its count of cells along the longer side, the cell
    /// never under [`MIN_CELL_MM`].
    Quality(Quality),
    /// A cell size, mm, at least [`MIN_CELL_MM`].
    CellMm(f64),
}

/// Square cells over a board. Cell `(i, j)` covers `[i·c, (i+1)·c) ×
/// [j·c, (j+1)·c)` from the board's bottom-left corner, `c` the cell
/// size; row 0 is at the lowest Y. The last column or row may reach past
/// the board by part of a cell.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    cell_mm: f64,
    nx: u32,
    ny: u32,
    width_mm: f64,
    height_mm: f64,
}

impl Grid {
    /// The grid of a board `width_mm` by `height_mm`.
    ///
    /// # Errors
    ///
    /// [`SimError::BadBoard`] for a side that is not finite, not above 0
    /// or past [`MAX_COORDINATE_MM`]; [`SimError::BadCell`] for a cell
    /// size that is not finite or is under [`MIN_CELL_MM`];
    /// [`SimError::TooManyCells`] past [`MAX_CELLS`].
    pub fn of(width_mm: f64, height_mm: f64, spec: CellSpec) -> Result<Self, SimError> {
        let side_ok = |side: f64| side.is_finite() && side > 0.0 && side <= MAX_COORDINATE_MM;
        if !(side_ok(width_mm) && side_ok(height_mm)) {
            return Err(SimError::BadBoard {
                width_mm,
                height_mm,
            });
        }
        let cell_mm = match spec {
            CellSpec::Quality(quality) => {
                (width_mm.max(height_mm) / f64::from(quality.cells())).max(MIN_CELL_MM)
            }
            CellSpec::CellMm(cell_mm) => {
                if !(cell_mm.is_finite() && cell_mm >= MIN_CELL_MM) {
                    return Err(SimError::BadCell { cell_mm });
                }
                cell_mm
            }
        };
        let (across, up) = (count(width_mm, cell_mm), count(height_mm, cell_mm));
        let cells = across * up;
        if cells > MAX_CELLS as f64 {
            return Err(SimError::TooManyCells {
                cell_mm,
                // Sides are at most 6 m and cells at least 0.02 mm: under
                // 1e11 cells, exact in an f64 and in a u64.
                #[expect(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    reason = "a positive whole number under 1e11"
                )]
                cells: cells as u64,
                least_cell_mm: least_cell_mm(width_mm, height_mm),
            });
        }
        // Each count is at least 1 and their product at most 2²⁴.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "whole numbers from 1 to 2²⁴"
        )]
        let (nx, ny) = (across as u32, up as u32);
        Ok(Self {
            cell_mm,
            nx,
            ny,
            width_mm,
            height_mm,
        })
    }

    /// The cell size, mm.
    #[must_use]
    pub const fn cell_mm(&self) -> f64 {
        self.cell_mm
    }

    /// Cells across, along X.
    #[must_use]
    pub const fn nx(&self) -> u32 {
        self.nx
    }

    /// Cells up, along Y.
    #[must_use]
    pub const fn ny(&self) -> u32 {
        self.ny
    }

    /// The board's width, mm.
    #[must_use]
    pub const fn width_mm(&self) -> f64 {
        self.width_mm
    }

    /// The board's height, mm.
    #[must_use]
    pub const fn height_mm(&self) -> f64 {
        self.height_mm
    }

    /// How many cells the grid has.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.nx as usize * self.ny as usize
    }

    /// Whether the grid has no cell. It never has none; the method keeps
    /// [`Grid::len`] company.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The centre of cell `(i, j)`, mm, from the board's bottom-left
    /// corner.
    #[must_use]
    pub fn centre(&self, i: u32, j: u32) -> Point {
        Point::new(
            (f64::from(i) + 0.5) * self.cell_mm,
            (f64::from(j) + 0.5) * self.cell_mm,
        )
    }

    /// The cell that holds `p`, if `p` is on the grid.
    #[must_use]
    pub fn cell_at(&self, p: Point) -> Option<(u32, u32)> {
        let along = |v: f64, n: u32| {
            let k = (v / self.cell_mm).floor();
            // `k` is checked to be a whole number in `0..n`.
            #[expect(
                clippy::cast_possible_truncation,
                clippy::cast_sign_loss,
                reason = "a whole number in 0..n"
            )]
            (k >= 0.0 && k < f64::from(n)).then_some(k as u32)
        };
        Some((along(p.x, self.nx)?, along(p.y, self.ny)?))
    }

    /// Whether cell `(i, j)`'s centre is on the board: false only in a
    /// last column or row that hangs past the board by more than half a
    /// cell, and for a cell the grid does not have.
    #[must_use]
    pub fn on_board(&self, i: u32, j: u32) -> bool {
        let c = self.centre(i, j);
        i < self.nx && j < self.ny && c.x <= self.width_mm && c.y <= self.height_mm
    }

    /// Cell `(i, j)`'s place in a row-major array, rows from the lowest
    /// Y; `None` for a cell the grid does not have.
    #[must_use]
    pub const fn index(&self, i: u32, j: u32) -> Option<usize> {
        if i < self.nx && j < self.ny {
            Some(j as usize * self.nx as usize + i as usize)
        } else {
            None
        }
    }

    /// The cells whose centres can lie in `[lo_mm, hi_mm]` along an axis
    /// of `n` cells: a half-open range, empty when the stretch misses
    /// the grid.
    pub(crate) fn span(&self, lo_mm: f64, hi_mm: f64, n: u32) -> (u32, u32) {
        // A centre `(k + ½)c` is in the stretch for `k` from
        // `ceil(lo/c − ½)` to `floor(hi/c − ½)`.
        let first = (lo_mm / self.cell_mm - 0.5).ceil().max(0.0);
        let last = (hi_mm / self.cell_mm - 0.5).floor().min(f64::from(n) - 1.0);
        if first > last || first.is_nan() || last.is_nan() {
            return (0, 0);
        }
        // Both are whole numbers in `0..n`.
        #[expect(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "whole numbers in 0..n"
        )]
        (first as u32, last as u32 + 1)
    }
}

/// How many cells of `cell_mm` cover a side of `side_mm`, as a whole
/// number in an `f64`.
fn count(side_mm: f64, cell_mm: f64) -> f64 {
    (side_mm / cell_mm - COUNT_SLACK).ceil().max(1.0)
}

/// The smallest cell a board fits [`MAX_CELLS`] in, mm, rounded up to a
/// micrometre.
fn least_cell_mm(width_mm: f64, height_mm: f64) -> f64 {
    let round_up = |cell_mm: f64| (cell_mm * 1000.0).ceil() / 1000.0;
    let mut cell_mm = round_up((width_mm * height_mm / MAX_CELLS as f64).sqrt()).max(MIN_CELL_MM);
    // The square root ignores the part cells at two edges: step up until
    // the whole cells fit. A micrometre a step, a few steps at most.
    while count(width_mm, cell_mm) * count(height_mm, cell_mm) > MAX_CELLS as f64 {
        cell_mm = round_up(cell_mm + 0.001);
    }
    cell_mm
}

/// Cells `[i0, i1) × [j0, j1)` of a grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellRect {
    /// The first column.
    pub i0: u32,
    /// One past the last column.
    pub i1: u32,
    /// The first row.
    pub j0: u32,
    /// One past the last row.
    pub j1: u32,
}

impl CellRect {
    /// The smallest rectangle that holds both.
    #[must_use]
    pub fn union(self, other: Self) -> Self {
        Self {
            i0: self.i0.min(other.i0),
            i1: self.i1.max(other.i1),
            j0: self.j0.min(other.j0),
            j1: self.j1.max(other.j1),
        }
    }

    /// Whether it holds cell `(i, j)`.
    #[must_use]
    pub const fn contains(&self, i: u32, j: u32) -> bool {
        self.i0 <= i && i < self.i1 && self.j0 <= j && j < self.j1
    }

    /// How many cells it holds.
    #[must_use]
    pub const fn len(&self) -> usize {
        (self.i1.saturating_sub(self.i0)) as usize * (self.j1.saturating_sub(self.j0)) as usize
    }

    /// Whether it holds no cell.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
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
    use dc_geometry::assert_approx_eq;

    /// Acceptance 131: a 200 × 100 mm stock at Standard has 1024 × 512
    /// cells of 0.1953 mm; Draft has half as many each way and Fine
    /// twice.
    #[test]
    fn cells_are_counted_along_the_longer_side() {
        for (quality, nx, ny) in [
            (Quality::Draft, 512, 256),
            (Quality::Standard, 1024, 512),
            (Quality::Fine, 2048, 1024),
        ] {
            let grid = Grid::of(200.0, 100.0, CellSpec::Quality(quality)).unwrap();
            assert_eq!((grid.nx(), grid.ny()), (nx, ny), "{quality:?}");
            assert_approx_eq!(grid.cell_mm(), 200.0 / f64::from(nx), 1e-12);
        }
        // The longer side is whichever is longer.
        let tall = Grid::of(100.0, 200.0, CellSpec::Quality(Quality::Standard)).unwrap();
        assert_eq!((tall.nx(), tall.ny()), (512, 1024));
    }

    /// Acceptance 131: a 10 × 5 mm stock at Fine would have 0.0049 mm
    /// cells; it is held to 0.02 mm, 500 × 250.
    #[test]
    fn a_small_stock_is_held_to_the_least_cell() {
        let grid = Grid::of(10.0, 5.0, CellSpec::Quality(Quality::Fine)).unwrap();
        assert_approx_eq!(grid.cell_mm(), MIN_CELL_MM, 1e-15);
        assert_eq!((grid.nx(), grid.ny()), (500, 250));
    }

    /// Upstream gives each axis at least 24 cells, so a 1000 × 50 board
    /// at 280 cells gets a grid 85.7 mm deep (the survey, section 2.2).
    /// Here a narrow board has only the cells that cover it.
    #[test]
    fn a_narrow_stock_has_no_cells_beyond_it() {
        let grid = Grid::of(1000.0, 50.0, CellSpec::CellMm(1000.0 / 280.0)).unwrap();
        assert_eq!((grid.nx(), grid.ny()), (280, 14));
        let reach_mm = f64::from(grid.ny()) * grid.cell_mm();
        assert!(
            reach_mm - 50.0 < grid.cell_mm(),
            "the grid is {reach_mm} deep"
        );
        // 14 cells are exactly 50 mm: every centre is on the board.
        assert!(grid.on_board(279, 13));
    }

    /// The last column hangs past the board when the side is no whole
    /// number of cells; a cell whose centre is past the edge is off the
    /// board.
    #[test]
    fn a_part_cell_is_on_the_board_only_by_its_centre() {
        // 10.4 mm in 1 mm cells: 11 columns, the last centred at 10.5.
        let grid = Grid::of(10.4, 3.0, CellSpec::CellMm(1.0)).unwrap();
        assert_eq!((grid.nx(), grid.ny()), (11, 3));
        assert!(grid.on_board(9, 0));
        assert!(!grid.on_board(10, 0), "its centre is at 10.5, past 10.4");
        assert!(!grid.on_board(11, 0), "no such cell");
        // 10.6 mm: the last centre, 10.5, is on the board.
        let grid = Grid::of(10.6, 3.0, CellSpec::CellMm(1.0)).unwrap();
        assert!(grid.on_board(10, 0));
    }

    /// Acceptance 131: `--cell 0.02` on a 1220 × 610 mm sheet is refused,
    /// with the cell that would fit.
    #[test]
    fn too_many_cells_name_the_cell_that_fits() {
        let Err(SimError::TooManyCells {
            cell_mm,
            cells,
            least_cell_mm,
        }) = Grid::of(1220.0, 610.0, CellSpec::CellMm(0.02))
        else {
            panic!("0.02 mm cells on a full sheet must be refused");
        };
        assert_approx_eq!(cell_mm, 0.02, 1e-15);
        assert_eq!(cells, 61_000 * 30_500);
        // The cell named fits, and one a micrometre smaller does not.
        let fits = Grid::of(1220.0, 610.0, CellSpec::CellMm(least_cell_mm)).unwrap();
        assert!(fits.len() <= MAX_CELLS);
        assert!(matches!(
            Grid::of(1220.0, 610.0, CellSpec::CellMm(least_cell_mm - 0.001)),
            Err(SimError::TooManyCells { .. })
        ));
        // The three qualities never reach the cap: 2048² is a quarter of it.
        let square = Grid::of(6000.0, 6000.0, CellSpec::Quality(Quality::Fine)).unwrap();
        assert_eq!(square.len(), 2048 * 2048);
    }

    #[test]
    fn a_board_or_a_cell_that_is_no_size_is_refused() {
        for (w, h) in [
            (0.0, 10.0),
            (10.0, -1.0),
            (f64::NAN, 10.0),
            (10.0, f64::INFINITY),
            (MAX_COORDINATE_MM + 1.0, 10.0),
        ] {
            assert!(
                matches!(
                    Grid::of(w, h, CellSpec::Quality(Quality::Draft)),
                    Err(SimError::BadBoard { .. })
                ),
                "{w} × {h}"
            );
        }
        for cell_mm in [0.0, -1.0, f64::NAN, f64::INFINITY, MIN_CELL_MM / 2.0] {
            assert!(
                matches!(
                    Grid::of(10.0, 10.0, CellSpec::CellMm(cell_mm)),
                    Err(SimError::BadCell { .. })
                ),
                "{cell_mm}"
            );
        }
    }

    /// A cell's centre is half a cell in from its corner, rows from the
    /// lowest Y, and `cell_at` is its inverse over the whole cell.
    #[test]
    fn centres_and_cells_agree() {
        let grid = Grid::of(8.0, 4.0, CellSpec::CellMm(0.5)).unwrap();
        let c = grid.centre(3, 2);
        assert_approx_eq!(c.x, 1.75, 1e-12);
        assert_approx_eq!(c.y, 1.25, 1e-12);
        assert_eq!(grid.cell_at(c), Some((3, 2)));
        assert_eq!(grid.cell_at(Point::new(1.5, 1.0)), Some((3, 2)));
        assert_eq!(grid.cell_at(Point::new(1.999, 1.499)), Some((3, 2)));
        assert_eq!(grid.cell_at(Point::new(-0.001, 1.0)), None);
        assert_eq!(grid.cell_at(Point::new(8.0, 1.0)), None);
        assert_eq!(grid.index(3, 2), Some(2 * 16 + 3));
        assert_eq!(grid.index(16, 0), None);
    }

    /// `span` is the cells whose centres are in a stretch, clamped to the
    /// grid, and empty when the stretch misses it.
    #[test]
    fn a_span_is_the_cells_whose_centres_it_holds() {
        let grid = Grid::of(10.0, 10.0, CellSpec::CellMm(1.0)).unwrap();
        // Centres at 0.5, 1.5, …: [1.5, 3.5] holds cells 1, 2 and 3.
        assert_eq!(grid.span(1.5, 3.5, 10), (1, 4));
        assert_eq!(grid.span(1.51, 3.49, 10), (2, 3));
        assert_eq!(grid.span(-5.0, 0.6, 10), (0, 1));
        assert_eq!(grid.span(8.7, 50.0, 10), (9, 10));
        let (a, b) = grid.span(20.0, 30.0, 10);
        assert_eq!(a, b, "a stretch past the grid holds no cell");
        let (a, b) = grid.span(-9.0, -1.0, 10);
        assert_eq!(a, b);
        let (a, b) = grid.span(2.1, 2.4, 10);
        assert_eq!(a, b, "a stretch between two centres holds no cell");
    }

    #[test]
    fn a_rectangles_union_and_size() {
        let a = CellRect {
            i0: 2,
            i1: 5,
            j0: 1,
            j1: 3,
        };
        let b = CellRect {
            i0: 4,
            i1: 9,
            j0: 0,
            j1: 2,
        };
        let u = a.union(b);
        assert_eq!((u.i0, u.i1, u.j0, u.j1), (2, 9, 0, 3));
        assert_eq!(a.len(), 6);
        assert!(a.contains(4, 2) && !a.contains(5, 2) && !a.contains(4, 3));
        assert!(
            CellRect {
                i0: 3,
                i1: 3,
                j0: 0,
                j1: 9
            }
            .is_empty()
        );
    }
}
