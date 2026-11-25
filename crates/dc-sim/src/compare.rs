// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Uncut material (phase 6 functional spec, capability 120;
//! `docs/specs/phase-6/design.md`, "Algorithms → Compare"): where the
//! finished stock stands above the design surface by more than a
//! threshold.
//!
//! A cell is compared with the highest the design asks in its 3 × 3
//! neighbourhood, so a wall that the two grids place a cell apart is not
//! reported: that is the grid's own tolerance. Flagged cells are grouped
//! into places, and a place of fewer than [`MIN_PLACE_CELLS`] cells is
//! dropped. Gouges, the other way, wait for phase 7's models (capability
//! 121): a flag is one bit so that theirs can be the next.

#![allow(
    clippy::indexing_slicing,
    reason = "every index comes from `Grid::index` or a range the grid's own counts bound"
)]

use dc_geometry::{Point, Rect};

use crate::design::Design;
use crate::field::Heightfield;

/// How far above the design the stock stands before it is uncut, unless
/// the user changes it, mm. Phase 3 chose ridges of 0.25 mm as
/// acceptable on its sign, so the threshold passes them.
pub const UNCUT_MM: f64 = 0.3;

/// The least and the most the threshold may be, mm.
pub const UNCUT_RANGE_MM: (f64, f64) = (0.05, 5.0);

/// The flag of a cell of uncut material, in [`Comparison::flags`].
pub const UNCUT_FLAG: u8 = 1;

/// A place of fewer cells than this is not reported.
pub const MIN_PLACE_CELLS: u32 = 4;

/// One place where material is left: a patch of touching cells.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// The operation whose shape asks for the cut, as the number the
    /// design was given for it.
    pub op: Option<u16>,
    /// How many cells it has.
    pub cells: u32,
    /// Its area, mm².
    pub area_mm2: f64,
    /// The most the stock stands above the design in it, mm.
    pub height_mm: f64,
    /// The box of its cells, mm, on the board.
    pub bounds: Rect,
}

/// The stock against the design.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Comparison {
    /// A flag a cell, row by row from the lowest Y: [`UNCUT_FLAG`] where
    /// the cell is part of a place, else 0.
    pub flags: Vec<u8>,
    /// The places, by operation and then from the largest.
    pub places: Vec<Place>,
}

/// Where `field` stands above `design` by more than `uncut_mm`. A
/// threshold outside [`UNCUT_RANGE_MM`] is held to it. A design on
/// another grid compares with nothing.
#[must_use]
pub fn compare(field: &Heightfield, design: &Design, uncut_mm: f64) -> Comparison {
    let grid = *field.grid();
    if *design.grid() != grid {
        return Comparison::default();
    }
    let uncut_mm = if uncut_mm.is_nan() {
        UNCUT_MM
    } else {
        uncut_mm.clamp(UNCUT_RANGE_MM.0, UNCUT_RANGE_MM.1)
    };
    let (nx, ny) = (grid.nx() as usize, grid.ny() as usize);
    let (stock, asked) = (field.heights(), design.heights());
    // How far each cell stands above the highest asked around it.
    let mut over = vec![0.0_f64; nx * ny];
    let mut flags = vec![0_u8; nx * ny];
    for j in 0..ny {
        for i in 0..nx {
            // Cell counts are at most 2¹²·2¹²; indices fit a `u32`.
            #[expect(clippy::cast_possible_truncation, reason = "grid indices fit a u32")]
            if !grid.on_board(i as u32, j as u32) {
                continue;
            }
            let mut highest = f64::NEG_INFINITY;
            for b in j.saturating_sub(1)..(j + 2).min(ny) {
                for a in i.saturating_sub(1)..(i + 2).min(nx) {
                    highest = highest.max(asked[b * nx + a]);
                }
            }
            let k = j * nx + i;
            over[k] = stock[k] - highest;
            if over[k] > uncut_mm {
                flags[k] = UNCUT_FLAG;
            }
        }
    }

    // Patches of touching flagged cells, corners included.
    let mut places = Vec::new();
    let mut seen = vec![false; nx * ny];
    let mut stack = Vec::new();
    let mut patch = Vec::new();
    for start in 0..nx * ny {
        if flags[start] == 0 || seen[start] {
            continue;
        }
        patch.clear();
        stack.push(start);
        seen[start] = true;
        while let Some(k) = stack.pop() {
            patch.push(k);
            let (i, j) = (k % nx, k / nx);
            for b in j.saturating_sub(1)..(j + 2).min(ny) {
                for a in i.saturating_sub(1)..(i + 2).min(nx) {
                    let n = b * nx + a;
                    if flags[n] != 0 && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
        }
        #[expect(
            clippy::cast_possible_truncation,
            reason = "a patch has at most 2²⁴ cells"
        )]
        let cells = patch.len() as u32;
        if cells < MIN_PLACE_CELLS {
            for &k in &patch {
                flags[k] = 0;
            }
            continue;
        }
        let cell_mm = grid.cell_mm();
        let (mut lo, mut hi) = ((usize::MAX, usize::MAX), (0, 0));
        let (mut height_mm, mut highest) = (0.0, start);
        for &k in &patch {
            let (i, j) = (k % nx, k / nx);
            lo = (lo.0.min(i), lo.1.min(j));
            hi = (hi.0.max(i), hi.1.max(j));
            if over[k] > height_mm {
                height_mm = over[k];
                highest = k;
            }
        }
        #[expect(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            reason = "grid indices are far under 2³²"
        )]
        let (bounds, op) = (
            Rect {
                min: Point::new(lo.0 as f64 * cell_mm, lo.1 as f64 * cell_mm),
                max: Point::new((hi.0 + 1) as f64 * cell_mm, (hi.1 + 1) as f64 * cell_mm),
            },
            design.asked_by((highest % nx) as u32, (highest / nx) as u32),
        );
        places.push(Place {
            op,
            cells,
            area_mm2: f64::from(cells) * cell_mm * cell_mm,
            height_mm,
            bounds,
        });
    }
    places.sort_by(|a, b| {
        a.op.cmp(&b.op)
            .then(b.area_mm2.total_cmp(&a.area_mm2))
            .then(a.bounds.min.y.total_cmp(&b.bounds.min.y))
            .then(a.bounds.min.x.total_cmp(&b.bounds.min.x))
    });
    Comparison { flags, places }
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
    use crate::cut::SimMove;
    use crate::cutter::Cutter;
    use crate::grid::{CellSpec, Grid};
    use dc_geometry::{Contour, Point3, Polyline, Region, assert_approx_eq, nest};

    const TOP_MM: f64 = 12.0;

    fn grid(cell_mm: f64) -> Grid {
        Grid::of(60.0, 50.0, CellSpec::CellMm(cell_mm)).unwrap()
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Vec<Region> {
        let contour = Contour::new(Polyline::rect(Point::new(x, y), w, h)).unwrap();
        nest(vec![contour]).unwrap().0
    }

    /// A square pocket `side` wide at (15, 10), `depth_mm` deep, cleared
    /// by level passes of an end mill of `radius_mm` and a pass round its
    /// wall: the stock it leaves.
    fn pocketed(grid: Grid, side: f64, depth_mm: f64, radius_mm: f64) -> Heightfield {
        let mut field = Heightfield::uncut(grid, TOP_MM).unwrap();
        let cutter = Cutter::Flat { radius_mm };
        let z = TOP_MM - depth_mm;
        let (x0, x1) = (15.0 + radius_mm, 15.0 + side - radius_mm);
        let (y0, y1) = (10.0 + radius_mm, 10.0 + side - radius_mm);
        let mut y = y0;
        loop {
            let pass = SimMove::cut(Point3::new(x0, y, z), Point3::new(x1, y, z), 1.0);
            field.cut(&pass, (0.0, 1.0), &cutter, 0);
            if y >= y1 {
                break;
            }
            y = (y + radius_mm).min(y1);
        }
        // The wall pass, round the inside.
        let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1), (x0, y0)];
        for pair in corners.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let wall = SimMove::cut(Point3::new(a.0, a.1, z), Point3::new(b.0, b.1, z), 1.0);
            field.cut(&wall, (0.0, 1.0), &cutter, 0);
        }
        field
    }

    /// Acceptance 147: a 30 mm square Pocket cut with a 6 mm end mill
    /// leaves the four corners its round bit cannot reach, each
    /// `(1 − π/4)·r²` in area less the cells along its two walls, which
    /// the neighbourhood keeps out; each stands the Pocket's full depth
    /// and names the Pocket.
    #[test]
    fn a_square_pocket_has_four_corners() {
        let (cell_mm, r) = (0.1, 3.0);
        let grid = grid(cell_mm);
        let field = pocketed(grid, 30.0, 6.0, r);
        let mut design = Design::uncut(grid, TOP_MM);
        design.lower_regions(&rect(15.0, 10.0, 30.0, 30.0), TOP_MM - 6.0, 4);
        let found = compare(&field, &design, UNCUT_MM);
        assert_eq!(found.places.len(), 4, "{:?}", found.places);
        let corner_mm2 = (1.0 - std::f64::consts::FRAC_PI_4) * r * r;
        for place in &found.places {
            assert_eq!(place.op, Some(4));
            assert_approx_eq!(place.height_mm, 6.0, 1e-9);
            // The patch is the corner less a cell along each of its two
            // walls, each about 0.64 r long.
            assert!(
                place.area_mm2 < corner_mm2
                    && place.area_mm2 > corner_mm2 - 2.0 * r * cell_mm * 2.0,
                "a corner of {} mm², want about {corner_mm2}",
                place.area_mm2
            );
            assert!(place.bounds.max.x - place.bounds.min.x < r);
        }
        // The four are in the four corners.
        let corners: Vec<(bool, bool)> = found
            .places
            .iter()
            .map(|p| (p.bounds.min.x < 30.0, p.bounds.min.y < 25.0))
            .collect();
        for want in [(true, true), (true, false), (false, true), (false, false)] {
            assert!(corners.contains(&want), "{corners:?}");
        }
        let flagged = found.flags.iter().filter(|&&f| f == UNCUT_FLAG).count();
        let in_places: u32 = found.places.iter().map(|p| p.cells).sum();
        assert_eq!(flagged, in_places as usize);
    }

    /// The stock cut exactly as the design asks reports nothing, and so
    /// does stock cut deeper or wider than it asks: that would be a
    /// gouge, which is not this phase's.
    #[test]
    fn nothing_is_reported_where_no_shape_asks() {
        let grid = grid(0.25);
        let field = pocketed(grid, 30.0, 6.0, 3.0);
        // Nothing asked at all: the cut stock is below the uncut design.
        assert!(
            compare(&field, &Design::uncut(grid, TOP_MM), UNCUT_MM)
                .places
                .is_empty()
        );
        // The uncut stock against itself.
        let uncut = Heightfield::uncut(grid, TOP_MM).unwrap();
        assert!(
            compare(&uncut, &Design::uncut(grid, TOP_MM), UNCUT_MM)
                .places
                .is_empty()
        );
        // A shallower design than the cut.
        let mut shallow = Design::uncut(grid, TOP_MM);
        shallow.lower_regions(&rect(18.0, 13.0, 24.0, 24.0), TOP_MM - 2.0, 0);
        assert!(compare(&field, &shallow, UNCUT_MM).places.is_empty());
    }

    /// The cell tolerance: a wall the stock has one cell further in than
    /// the design is not a place; two cells further in is.
    #[test]
    fn a_wall_one_cell_off_is_not_a_place() {
        let cell_mm = 0.5;
        let grid = grid(cell_mm);
        let mut design = Design::uncut(grid, TOP_MM);
        design.lower_regions(&rect(10.0, 10.0, 30.0, 20.0), 6.0, 0);
        let stock_with = |x0: f64| {
            let mut asked = Design::uncut(grid, TOP_MM);
            asked.lower_regions(&rect(x0, 10.0, 40.0 - x0, 20.0), 6.0, 0);
            // A field with the design's own heights: cut by one wide
            // pass a cell at a time.
            let mut field = Heightfield::uncut(grid, TOP_MM).unwrap();
            for j in 0..grid.ny() {
                for i in 0..grid.nx() {
                    if asked.z_mm(i, j).unwrap() < TOP_MM {
                        let c = grid.centre(i, j);
                        let dot = SimMove::cut(
                            Point3::new(c.x, c.y, 6.0),
                            Point3::new(c.x, c.y, 6.0),
                            0.0,
                        );
                        field.cut(&dot, (0.0, 1.0), &Cutter::Flat { radius_mm: 0.01 }, 0);
                    }
                }
            }
            field
        };
        assert!(
            compare(&stock_with(10.0), &design, UNCUT_MM)
                .places
                .is_empty()
        );
        assert!(
            compare(&stock_with(10.0 + cell_mm), &design, UNCUT_MM)
                .places
                .is_empty(),
            "a wall one cell in"
        );
        let two = compare(&stock_with(10.0 + 2.0 * cell_mm), &design, UNCUT_MM);
        assert_eq!(two.places.len(), 1, "a wall two cells in");
        // One column of cells, 20 mm long: the other is the tolerance's.
        assert_approx_eq!(
            two.places[0].area_mm2,
            20.0 * cell_mm,
            2.0 * cell_mm * cell_mm
        );
    }

    /// Ridges follow the threshold (acceptance 147): material 0.25 mm
    /// high is not reported at the 0.3 mm the app starts with, and is at
    /// 0.2 mm; a threshold out of range is held to it.
    #[test]
    fn ridges_follow_the_threshold() {
        let grid = grid(0.5);
        let mut design = Design::uncut(grid, TOP_MM);
        design.lower_regions(&rect(10.0, 10.0, 40.0, 30.0), 6.0, 0);
        // The whole floor left 0.25 mm high.
        let mut field = Heightfield::uncut(grid, TOP_MM).unwrap();
        let pass =
            |y: f64| SimMove::cut(Point3::new(10.0, y, 6.25), Point3::new(50.0, y, 6.25), 1.0);
        for k in 0..31 {
            field.cut(
                &pass(10.0 + f64::from(k)),
                (0.0, 1.0),
                &Cutter::Flat { radius_mm: 1.0 },
                0,
            );
        }
        assert!(compare(&field, &design, UNCUT_MM).places.is_empty());
        let fussy = compare(&field, &design, 0.2);
        assert_eq!(fussy.places.len(), 1);
        assert_approx_eq!(fussy.places[0].height_mm, 0.25, 1e-9);
        // Below the least threshold is the least: 0.25 mm is over 0.05.
        assert_eq!(compare(&field, &design, 0.0).places.len(), 1);
        assert!(compare(&field, &design, 99.0).places.is_empty());
        assert!(compare(&field, &design, f64::NAN).places.is_empty());
    }

    /// A speck of fewer than four cells is not a place, and its flags go
    /// with it; places name their own operations and come by operation,
    /// then largest first.
    #[test]
    fn small_patches_are_dropped_and_places_name_their_operations() {
        let grid = grid(0.5);
        let mut design = Design::uncut(grid, TOP_MM);
        // Three shapes asked and never cut: a big one and a small one of
        // operation 2, a middling one of operation 1, and a speck.
        design.lower_regions(&rect(5.0, 5.0, 10.0, 10.0), 6.0, 2);
        design.lower_regions(&rect(30.0, 5.0, 4.0, 4.0), 6.0, 2);
        design.lower_regions(&rect(5.0, 30.0, 6.0, 6.0), 6.0, 1);
        design.lower_regions(&rect(40.0, 40.0, 2.5, 2.5), 6.0, 3);
        let uncut = Heightfield::uncut(grid, TOP_MM).unwrap();
        let found = compare(&uncut, &design, UNCUT_MM);
        // Each shape less the ring of cells its edge's neighbourhood
        // keeps: the 2.5 mm speck is 5 × 5 cells, 3 × 3 after it, which
        // is nine and stays; shrink the grid's view of it instead.
        let ops: Vec<Option<u16>> = found.places.iter().map(|p| p.op).collect();
        assert_eq!(ops, [Some(1), Some(2), Some(2), Some(3)]);
        assert!(found.places[1].area_mm2 > found.places[2].area_mm2);
        for place in &found.places {
            assert_approx_eq!(place.height_mm, 6.0, 1e-9);
            assert!(place.cells >= MIN_PLACE_CELLS);
        }
        // A shape three cells wide is one cell after the tolerance: gone.
        let mut speck = Design::uncut(grid, TOP_MM);
        speck.lower_regions(&rect(20.0, 20.0, 1.5, 1.5), 6.0, 0);
        let none = compare(&uncut, &speck, UNCUT_MM);
        assert!(none.places.is_empty());
        assert!(none.flags.iter().all(|&f| f == 0));
        // A design on another grid compares with nothing.
        let other = Design::uncut(Grid::of(60.0, 50.0, CellSpec::CellMm(1.0)).unwrap(), TOP_MM);
        assert_eq!(compare(&uncut, &other, UNCUT_MM), Comparison::default());
    }
}
