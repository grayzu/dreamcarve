// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/simulation/heightfieldShader.ts (`edgeIsStep`, the surface gradient)
//   src/engine/simulation/stepThreshold.ts

//! What the renderer needs to light the stock (phase 6 design, "Shading
//! data"): for each edge between two cells, whether it is a **step**, a
//! wall to light by its own face, or part of a **slope**, to light as
//! the surface it belongs to; and each cell's gradient, the slope of the
//! surface through its centre, steps left out.
//!
//! The rule and its numbers are upstream's: a change of at most half a
//! cell is always a slope (a ball finish sampled at two or three cells a
//! pass would otherwise draw as squares outlined by walls); a steeper
//! one is a slope when the next edge on either side changes the same
//! way and this one is at most four times as steep; an edge beside a
//! cut-through cell is always a step, however thin the skin. Upstream
//! runs the rule in its fragment shader and tests the shader's text
//! through a transpiler; here it runs on the CPU, over the cells that
//! changed, into two small textures, so the tests below exercise the
//! code that ships (design open question 3). The zoomed-out widening is
//! not ported. Heights have no handedness: "east" is +X and "north" is
//! +Y, whatever upstream's Y-down sheet called them.

#![allow(
    clippy::indexing_slicing,
    reason = "every index is bounded by the grid's own counts a few lines above"
)]

use dc_job::sim::{CellRect, Heightfield};

/// A change of at most this many cells between neighbours is always a
/// slope: upstream's `SHALLOW_SLOPE_CELLS`.
pub const SHALLOW_SLOPE_CELLS: f64 = 0.5;
/// An edge is part of a slope when it is at most this many times as
/// steep as the next edge that changes the same way: upstream's
/// `SLOPE_CONTINUATION_RATIO`.
pub const SLOPE_CONTINUATION_RATIO: f64 = 4.0;
/// At or below this height, mm, a cell is cut through.
const THROUGH_MM: f64 = 1e-6;
/// What a change may exceed half a cell by and still be one, mm: two
/// heights half a cell apart on paper differ by a rounding more or less
/// once they are `f64`s, and the rule's edge is not to turn on it.
const EDGE_SLACK_MM: f64 = 1e-9;

/// The edge to the cell's +X neighbour is a step.
pub const EAST_STEP: u8 = 1;
/// The edge to the cell's +Y neighbour is a step.
pub const NORTH_STEP: u8 = 2;

/// Whether the edge between two neighbouring cells of heights `near`
/// and `far`, mm, is a step. `near_beyond` and `far_beyond` are the next
/// cell out on each side; a cell off the grid is at the bottom, 0, which
/// makes the stock's own sides steps.
#[must_use]
pub fn edge_is_step(near: f64, far: f64, near_beyond: f64, far_beyond: f64, cell_mm: f64) -> bool {
    let change = far - near;
    if change.abs() <= 0.0 {
        return false;
    }
    if near.min(far) <= THROUGH_MM {
        return true;
    }
    if change.abs() <= SHALLOW_SLOPE_CELLS * cell_mm + EDGE_SLACK_MM {
        return false;
    }
    // The change across the next edge on each side; a cut-through cell
    // beyond continues no slope.
    let before = if near_beyond > THROUGH_MM {
        near - near_beyond
    } else {
        0.0
    };
    let after = if far_beyond > THROUGH_MM {
        far_beyond - far
    } else {
        0.0
    };
    let continues =
        |next: f64| next * change > 0.0 && change.abs() <= SLOPE_CONTINUATION_RATIO * next.abs();
    !(continues(before) || continues(after))
}

/// The shading of a grid of cells, row by row from the lowest Y.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Shading {
    /// [`EAST_STEP`] and [`NORTH_STEP`], a cell.
    pub classes: Vec<u8>,
    /// The surface's rise in mm for a mm along +X and along +Y through
    /// each cell's centre: narrowed to `f32` here, for the texture the
    /// renderer reads.
    pub gradients: Vec<[f32; 2]>,
}

impl Shading {
    /// The shading of every cell of `field`.
    #[must_use]
    pub fn of(field: &Heightfield) -> Self {
        let grid = field.grid();
        let mut shading = Self {
            classes: vec![0; grid.len()],
            gradients: vec![[0.0; 2]; grid.len()],
        };
        shading.update(
            field,
            CellRect {
                i0: 0,
                i1: grid.nx(),
                j0: 0,
                j1: grid.ny(),
            },
        );
        shading
    }

    /// Shades again the cells of `changed` and the two cells round
    /// them, which read their heights, and returns the cells it wrote:
    /// what to send to the renderer's textures. A shading of another
    /// grid's size is made anew.
    pub fn update(&mut self, field: &Heightfield, changed: CellRect) -> CellRect {
        let grid = *field.grid();
        let (nx, ny) = (grid.nx(), grid.ny());
        if self.classes.len() != grid.len() || self.gradients.len() != grid.len() {
            self.classes = vec![0; grid.len()];
            self.gradients = vec![[0.0; 2]; grid.len()];
        }
        let around = CellRect {
            i0: changed.i0.saturating_sub(2),
            i1: (changed.i1 + 2).min(nx),
            j0: changed.j0.saturating_sub(2),
            j1: (changed.j1 + 2).min(ny),
        };
        let heights = field.heights();
        let (width, cell_mm) = (nx as usize, grid.cell_mm());
        // A cell's height by signed offsets; 0 off the grid.
        let at = |i: i64, j: i64| -> f64 {
            if i < 0 || j < 0 || i >= i64::from(nx) || j >= i64::from(ny) {
                return 0.0;
            }
            // Both are whole numbers inside the grid.
            #[expect(
                clippy::cast_sign_loss,
                clippy::cast_possible_truncation,
                reason = "checked to be inside the grid"
            )]
            heights[j as usize * width + i as usize]
        };
        // Whether the edge from (i, j) toward (i + di, j + dj) is a step.
        let step = |i: i64, j: i64, di: i64, dj: i64| {
            edge_is_step(
                at(i, j),
                at(i + di, j + dj),
                at(i - di, j - dj),
                at(i + 2 * di, j + 2 * dj),
                cell_mm,
            )
        };
        for j in around.j0..around.j1 {
            for i in around.i0..around.i1 {
                let (x, y) = (i64::from(i), i64::from(j));
                let (east, north) = (step(x, y, 1, 0), step(x, y, 0, 1));
                let (west, south) = (step(x - 1, y, 1, 0), step(x, y - 1, 0, 1));
                let here = at(x, y);
                // A central difference; an edge that is a step, or leads
                // off the grid, adds nothing.
                let rise = |blocked: bool, to: f64, sign: f64| {
                    if blocked { 0.0 } else { (to - here) * sign }
                };
                let dx = (rise(east, at(x + 1, y), 1.0) + rise(west, at(x - 1, y), -1.0)) / 2.0;
                let dy = (rise(north, at(x, y + 1), 1.0) + rise(south, at(x, y - 1), -1.0)) / 2.0;
                let k = j as usize * width + i as usize;
                self.classes[k] = (u8::from(east) * EAST_STEP) | (u8::from(north) * NORTH_STEP);
                // The renderer's edge: the one place a slope is narrowed.
                #[expect(clippy::cast_possible_truncation, reason = "narrowed for the GPU")]
                {
                    self.gradients[k] = [(dx / cell_mm) as f32, (dy / cell_mm) as f32];
                }
            }
        }
        around
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
    use dc_geometry::{Point3, assert_approx_eq};
    use dc_job::sim::{CellSpec, Cutter, Grid, SimMove};

    /// Whether each edge of a row of heights is a step, cells off its
    /// ends at the bottom: the edge before the first cell, between each
    /// pair, and after the last.
    fn steps(row: &[f64], cell_mm: f64) -> Vec<bool> {
        let at = |k: i64| {
            usize::try_from(k)
                .ok()
                .and_then(|k| row.get(k))
                .copied()
                .unwrap_or(0.0)
        };
        let n = i64::try_from(row.len()).unwrap();
        (-1..n)
            .map(|k| edge_is_step(at(k), at(k + 1), at(k - 1), at(k + 2), cell_mm))
            .collect()
    }

    /// Ported from `testTabBridgeAndCutThroughStaySteps`: a tab beside a
    /// through cut and a through cut's rim are steps, however thin the
    /// skin; a tab 1 to 19.4 high beside stock 20 high is a step, and
    /// one within half a cell of it is a slope; the stock's own sides
    /// are steps.
    #[test]
    fn a_tab_bridge_and_a_cut_through_rim_are_steps() {
        // Through, through, a tab 3 high, stock 20 high.
        assert_eq!(
            steps(&[0.0, 0.0, 3.0, 3.0, 20.0, 20.0], 1.0),
            [false, false, true, false, true, false, true]
        );
        // A one-cell tab bridge between two through cuts.
        assert_eq!(
            steps(&[0.0, 0.0, 3.0, 0.0, 0.0], 1.0),
            [false, false, true, true, false, false]
        );
        for tab in [1.0, 5.0, 12.0, 19.0, 19.4] {
            assert!(
                steps(&[20.0, tab, 20.0], 1.0)[1],
                "a tab {tab} high beside 20"
            );
        }
        for tab in [19.5, 19.8] {
            assert!(
                !steps(&[20.0, tab, 20.0], 1.0)[1],
                "a tab {tab} high beside 20"
            );
        }
        for skin in [0.05, 0.3, 1.0, 5.0] {
            assert!(
                steps(&[skin, skin, 0.0, 0.0], 1.0)[2],
                "a skin {skin} thick beside a through cut"
            );
        }
        // The stock's perimeter: the cells off the grid are at the bottom.
        let row = steps(&[12.0, 12.0, 12.0], 1.0);
        assert!(row[0] && row[3] && !row[1] && !row[2]);
    }

    /// Ported from `testWallsAndSlopesKeepTheirClass`: a pocket's wall,
    /// terraces and a one-cell wall are steps; a change of 0.4 or 0.5 of
    /// a cell is a slope and 0.6 a step, at cells of 0.1, 1, 2 and 2.5;
    /// a V's ridge and a roundover are slopes.
    #[test]
    fn walls_and_slopes_keep_their_class() {
        // A pocket 4 deep in stock 12 thick.
        assert_eq!(
            steps(&[12.0, 12.0, 8.0, 8.0, 8.0, 12.0], 1.0)[1..6],
            [false, true, false, false, true]
        );
        // Terraces: each riser a step.
        assert_eq!(
            steps(&[12.0, 12.0, 9.0, 9.0, 6.0, 6.0], 1.0)[1..6],
            [false, true, false, true, false]
        );
        // A wall one cell wide standing in a pocket.
        assert_eq!(
            steps(&[8.0, 8.0, 12.0, 8.0, 8.0], 1.0)[1..5],
            [false, true, true, false]
        );
        for cell_mm in [0.1, 1.0, 2.0, 2.5] {
            for (cells, is_step) in [(0.4, false), (0.5, false), (0.6, true)] {
                let low = 10.0 - cells * cell_mm;
                assert_eq!(
                    steps(&[10.0, 10.0, low, low], cell_mm)[2],
                    is_step,
                    "{cells} of a {cell_mm} mm cell"
                );
            }
        }
        // A 90° V's ridge, a cell a step: every edge rises or falls a
        // whole cell and the next continues it, but for the ridge's own
        // two, which the next edge out continues.
        let ridge = [6.0, 7.0, 8.0, 9.0, 8.0, 7.0, 6.0];
        assert!(
            steps(&ridge, 1.0)[1..7].iter().all(|step| !step),
            "{:?}",
            steps(&ridge, 1.0)
        );
        // A roundover: steeper and steeper, each edge under four times
        // the one before.
        let round = [10.0, 9.9, 9.6, 9.0, 7.8, 5.6, 2.0];
        assert!(
            steps(&round, 1.0)[1..7].iter().all(|step| !step),
            "{:?}",
            steps(&round, 1.0)
        );
    }

    /// Ported from `testShallowScallopIsSlope`, upstream's "guitar": a
    /// ball of radius 3.175 mm finishing at a stepover of 1 mm on cells
    /// of 0.37 mm. The scallops are two or three cells a pass and their
    /// slope turns over at nearly every edge; every edge is a slope.
    #[test]
    fn a_shallow_scallop_is_a_slope() {
        let (cell_mm, radius, stepover) = (0.37, 3.175_f64, 1.0);
        let row: Vec<f64> = (0..200)
            .map(|k| {
                let x = (f64::from(k) + 0.5) * cell_mm;
                let from_pass = (x / stepover).fract() * stepover;
                let d = from_pass.min(stepover - from_pass);
                8.0 + radius - (radius * radius - d * d).sqrt()
            })
            .collect();
        let classes = steps(&row, cell_mm);
        assert!(classes[1..200].iter().all(|step| !step));
    }

    /// A field with one slot 6 wide and 4 deep along X, a 90° V groove
    /// along X, and a hole cut through.
    fn field() -> Heightfield {
        let grid = Grid::of(40.0, 30.0, CellSpec::CellMm(0.5)).unwrap();
        let mut field = Heightfield::uncut(grid, 12.0).unwrap();
        let cut =
            |y: f64, z: f64| SimMove::cut(Point3::new(8.0, y, z), Point3::new(32.0, y, z), 1.0);
        field.cut(
            &cut(8.0, 8.0),
            (0.0, 1.0),
            &Cutter::Flat { radius_mm: 3.0 },
            0,
        );
        let v = Cutter::Vee {
            slope: 1.0,
            radius_mm: 4.0,
        };
        field.cut(&cut(21.0, 9.0), (0.0, 1.0), &v, 1);
        let hole = SimMove::cut(
            Point3::new(4.0, 26.0, -1.0),
            Point3::new(4.0, 26.0, -1.0),
            0.0,
        );
        field.cut(&hole, (0.0, 1.0), &Cutter::Flat { radius_mm: 2.0 }, 2);
        field
    }

    fn at(shading: &Shading, field: &Heightfield, x: f64, y: f64) -> (u8, [f32; 2]) {
        let (i, j) = field.grid().cell_at(dc_geometry::Point::new(x, y)).unwrap();
        let k = field.grid().index(i, j).unwrap();
        (shading.classes[k], shading.gradients[k])
    }

    /// Ported from `testGradientIsContinuousAndExact` and
    /// `testGradientLeavesStepsOut`: on a slope the gradient at a cell's
    /// centre is the central difference of its neighbours (a 90° V's
    /// wall rises a millimetre a millimetre); a flat top and a flat
    /// floor have none, up to a wall's rim, because a step adds nothing.
    #[test]
    fn the_gradient_is_the_slope_and_leaves_steps_out() {
        let field = field();
        let shading = Shading::of(&field);
        // The V's far wall rises with +Y, its near wall falls; along X
        // it is level.
        let (_, far_wall) = at(&shading, &field, 20.0, 23.0);
        assert_approx_eq!(f64::from(far_wall[1]), 1.0, 1e-6);
        assert_approx_eq!(f64::from(far_wall[0]), 0.0, 1e-6);
        let (_, near_wall) = at(&shading, &field, 20.0, 19.0);
        assert_approx_eq!(f64::from(near_wall[1]), -1.0, 1e-6);
        // The slot's floor and the top beside it are flat, right up to
        // the wall between them, which is a step.
        for (x, y) in [
            (20.0, 8.0),
            (20.0, 10.7),
            (20.0, 11.3),
            (20.0, 14.0),
            (36.0, 3.0),
        ] {
            let (_, gradient) = at(&shading, &field, x, y);
            assert_eq!(gradient, [0.0, 0.0], "at {x}, {y}");
        }
        let (class, _) = at(&shading, &field, 20.0, 10.7);
        assert_eq!(
            class & NORTH_STEP,
            NORTH_STEP,
            "the slot's wall is north of its last floor cell"
        );
        let (class, _) = at(&shading, &field, 20.0, 8.0);
        assert_eq!(class, 0, "the floor's middle has no step");
        // The hole's rim is a step, and so is the board's own edge.
        let (class, _) = at(&shading, &field, 5.8, 26.2);
        assert_eq!(class & EAST_STEP, EAST_STEP);
        let (class, _) = at(&shading, &field, 39.8, 15.0);
        assert_eq!(class & EAST_STEP, EAST_STEP);
    }

    /// Shading only the cells that changed, and the two round them,
    /// leaves what shading everything leaves.
    #[test]
    fn shading_a_changed_rectangle_equals_shading_all() {
        let mut field = field();
        let mut shading = Shading::of(&field);
        let more = SimMove::cut(
            Point3::new(30.0, 4.0, 5.0),
            Point3::new(36.0, 27.0, 7.0),
            1.0,
        );
        let stamp = field.cut(&more, (0.0, 1.0), &Cutter::Ball { radius_mm: 2.5 }, 3);
        let wrote = shading.update(&field, stamp.changed.unwrap());
        assert_eq!(shading, Shading::of(&field));
        let changed = stamp.changed.unwrap();
        assert!(wrote.i0 <= changed.i0.saturating_sub(2) && wrote.j1 >= changed.j1);
        // Another grid's shading is made anew.
        let small =
            Heightfield::uncut(Grid::of(4.0, 4.0, CellSpec::CellMm(1.0)).unwrap(), 5.0).unwrap();
        shading.update(
            &small,
            CellRect {
                i0: 0,
                i1: 1,
                j0: 0,
                j1: 1,
            },
        );
        assert_eq!(shading.classes.len(), 16);
    }
}
