// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The design surface (phase 6 functional spec, capability 119;
//! `docs/specs/phase-6/design.md`, "Algorithms → The design surface"):
//! what the stock would look like if every operation cut exactly its
//! shapes to its depth with a perfectly sharp tool. It exists to be
//! compared with the simulated stock ([`crate::compare`]).
//!
//! It starts as the uncut stock, and each operation lowers it over its
//! shapes; a cell keeps the lowest asked of it and the operation that
//! asked. It is never above the board's top, and where no operation asks
//! for a cut it is the top, so nothing is ever reported there. This
//! module knows shapes and heights; which shapes and heights an
//! operation asks for is `dc-job`'s to say.

#![allow(
    clippy::indexing_slicing,
    reason = "every index comes from `Grid::index` or a range the grid's own counts bound"
)]

use dc_geometry::{Point, Polyline, Region};

use crate::edt::distances_cells;
use crate::grid::Grid;

/// The owner of a cell no operation asks to cut.
const NO_OP: u16 = u16::MAX;

/// Arcs are flattened to within this part of a cell before a shape is
/// laid on the grid: far inside the cell tolerance.
const FLATTEN_CELLS: f64 = 0.125;

/// Which side of a shape's edge a trench runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BandSide {
    /// In the shape, along its edge and round its islands: an Inside
    /// Profile's.
    Inside,
    /// Out of the shape, round its edge and in its holes: an Outside
    /// Profile's.
    Outside,
}

/// What the operations' shapes ask for: a height a cell, mm from the
/// board's bottom, and the operation that asked for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Design {
    grid: Grid,
    top_z_mm: f64,
    z_mm: Vec<f64>,
    asked_by: Vec<u16>,
}

impl Design {
    /// The uncut stock: nothing is asked.
    #[must_use]
    pub fn uncut(grid: Grid, top_z_mm: f64) -> Self {
        Self {
            grid,
            top_z_mm,
            z_mm: vec![top_z_mm; grid.len()],
            asked_by: vec![NO_OP; grid.len()],
        }
    }

    /// The cells.
    #[must_use]
    pub const fn grid(&self) -> &Grid {
        &self.grid
    }

    /// The height asked of cell `(i, j)`, mm; `None` for a cell the grid
    /// does not have.
    #[must_use]
    pub fn z_mm(&self, i: u32, j: u32) -> Option<f64> {
        self.grid.index(i, j).map(|k| self.z_mm[k])
    }

    /// The operation that asks for cell `(i, j)`'s height, as the number
    /// its caller gave it; `None` where nothing is asked.
    #[must_use]
    pub fn asked_by(&self, i: u32, j: u32) -> Option<u16> {
        let op = self.asked_by[self.grid.index(i, j)?];
        (op != NO_OP).then_some(op)
    }

    /// Every height, row by row from the lowest Y, mm.
    #[must_use]
    pub fn heights(&self) -> &[f64] {
        &self.z_mm
    }

    /// Lowers the surface to `z_mm` over `regions`, for operation `op`:
    /// a Pocket's floor, a Bore's hole, a Profile's trench. A cell is in
    /// a region when its centre is.
    pub fn lower_regions(&mut self, regions: &[Region], z_mm: f64, op: u16) {
        let mask = self.mask(regions);
        for (k, inside) in mask.into_iter().enumerate() {
            if inside {
                self.lower(k, z_mm, op);
            }
        }
    }

    /// Lowers the surface to `z_mm` in a band `width_mm` wide beside the
    /// edges of `regions`, on `side` of them, for operation `op`: the
    /// trench of an Inside or an Outside Profile, as wide as its tool,
    /// with the shape's own corners kept sharp. A cell is in the band
    /// when its centre is within the width of the edge, the edge taken
    /// as half a cell from the nearest centre on its other side.
    pub fn lower_band(
        &mut self,
        regions: &[Region],
        width_mm: f64,
        side: BandSide,
        z_mm: f64,
        op: u16,
    ) {
        let mut mask = self.mask(regions);
        if side == BandSide::Outside {
            for inside in &mut mask {
                *inside = !*inside;
            }
        }
        let (nx, ny) = (self.grid.nx() as usize, self.grid.ny() as usize);
        let cells = distances_cells(&mask, nx, ny);
        let cell_mm = self.grid.cell_mm();
        for (k, on_side) in mask.into_iter().enumerate() {
            if on_side && (cells[k] - 0.5) * cell_mm <= width_mm {
                self.lower(k, z_mm, op);
            }
        }
    }

    /// Lowers the surface to `z_mm` within `reach_mm` of `paths`, for
    /// operation `op`: the trench of a Profile that runs on its line, or
    /// beside an open path, as its tool sweeps it.
    pub fn lower_near(&mut self, paths: &[Polyline], reach_mm: f64, z_mm: f64, op: u16) {
        for k in self.cells_near(paths, reach_mm) {
            self.lower(k, z_mm, op);
        }
    }

    /// Raises to `z_mm` the cells within `reach_mm` of `paths` that
    /// operation `op` asked lower: the tabs in a Profile's trench, each
    /// under the stretch of the tool's path that rides over it.
    pub fn raise_near(&mut self, paths: &[Polyline], reach_mm: f64, z_mm: f64, op: u16) {
        let z_mm = z_mm.min(self.top_z_mm);
        for k in self.cells_near(paths, reach_mm) {
            if self.asked_by[k] == op && self.z_mm[k] < z_mm {
                self.z_mm[k] = z_mm;
            }
        }
    }

    /// The cells whose centres are within `reach_mm` of any of `paths`,
    /// arcs flattened to a part of a cell; a cell near two segments is
    /// given twice.
    fn cells_near(&self, paths: &[Polyline], reach_mm: f64) -> Vec<usize> {
        let grid = self.grid;
        let mut cells = Vec::new();
        for path in paths {
            let flat = path.with_arcs_flattened(grid.cell_mm() * FLATTEN_CELLS);
            for segment in flat.segments() {
                let (a, b) = (segment.start, segment.end);
                let (i0, i1) =
                    grid.span(a.x.min(b.x) - reach_mm, a.x.max(b.x) + reach_mm, grid.nx());
                let (j0, j1) =
                    grid.span(a.y.min(b.y) - reach_mm, a.y.max(b.y) + reach_mm, grid.ny());
                let (dx, dy) = (b.x - a.x, b.y - a.y);
                let length2 = dx * dx + dy * dy;
                for j in j0..j1 {
                    for i in i0..i1 {
                        let q = grid.centre(i, j);
                        let t = if length2 > 0.0 {
                            (((q.x - a.x) * dx + (q.y - a.y) * dy) / length2).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                        let near = Point::new(a.x + dx * t, a.y + dy * t);
                        if q.distance_to(near) <= reach_mm
                            && let Some(k) = grid.index(i, j)
                        {
                            cells.push(k);
                        }
                    }
                }
            }
        }
        cells
    }

    /// Lowers the surface over `regions` to what a perfectly sharp V of
    /// `slope` (`tan(α/2)`) carves from `start_z_mm`: at each cell, as
    /// deep as its distance from the regions' edge over the slope, and no
    /// deeper than `flat_mm` when a flat depth is set. The distance is
    /// from the cell's centre to the nearest cell centre outside the
    /// regions, less half a cell: within a cell of the true distance.
    pub fn lower_cone(
        &mut self,
        regions: &[Region],
        start_z_mm: f64,
        slope: f64,
        flat_mm: Option<f64>,
        op: u16,
    ) {
        // A slope that is no slope carves nothing.
        if slope.is_nan() || slope <= 0.0 {
            return;
        }
        let mask = self.mask(regions);
        let (nx, ny) = (self.grid.nx() as usize, self.grid.ny() as usize);
        let cells = distances_cells(&mask, nx, ny);
        let cell_mm = self.grid.cell_mm();
        for (k, inside) in mask.into_iter().enumerate() {
            if !inside {
                continue;
            }
            let from_edge_mm = ((cells[k] - 0.5) * cell_mm).max(0.0);
            let depth_mm =
                flat_mm.map_or(from_edge_mm / slope, |flat| flat.min(from_edge_mm / slope));
            self.lower(k, start_z_mm - depth_mm, op);
        }
    }

    /// Lowers cell `k` to `z_mm` for `op` if that is lower than is asked
    /// of it already; never below the bottom, never above the top.
    fn lower(&mut self, k: usize, z_mm: f64, op: u16) {
        let z_mm = z_mm.clamp(0.0, self.top_z_mm);
        if z_mm < self.z_mm[k] {
            self.z_mm[k] = z_mm;
            self.asked_by[k] = op;
        }
    }

    /// The cells whose centres are in any of `regions`, by scan lines:
    /// along each row of centres, the stretches between the crossings of
    /// each region's outline, its holes left out.
    fn mask(&self, regions: &[Region]) -> Vec<bool> {
        let grid = &self.grid;
        let (nx, ny, cell_mm) = (grid.nx(), grid.ny(), grid.cell_mm());
        let mut mask = vec![false; grid.len()];
        let mut crossings: Vec<f64> = Vec::new();
        for region in regions {
            // Each outline as straight edges, arcs within a part of a
            // cell.
            let mut edges: Vec<(Point, Point)> = Vec::new();
            for contour in std::iter::once(region.outer()).chain(region.holes()) {
                let flat = contour
                    .polyline()
                    .with_arcs_flattened(cell_mm * FLATTEN_CELLS);
                edges.extend(flat.segments().map(|s| (s.start, s.end)));
            }
            let Some((lo, hi)) = edges
                .iter()
                .fold(None, |range: Option<(f64, f64)>, (a, b)| {
                    let (lo, hi) = (a.y.min(b.y), a.y.max(b.y));
                    Some(range.map_or((lo, hi), |(l, h)| (l.min(lo), h.max(hi))))
                })
            else {
                continue;
            };
            let (j0, j1) = grid.span(lo, hi, ny);
            for j in j0..j1 {
                let y = grid.centre(0, j).y;
                crossings.clear();
                for &(a, b) in &edges {
                    // An edge crosses the row when one end is below it
                    // and the other at or above: each vertex counts once.
                    if (a.y < y) != (b.y < y) {
                        crossings.push(a.x + (y - a.y) / (b.y - a.y) * (b.x - a.x));
                    }
                }
                crossings.sort_by(f64::total_cmp);
                // The outer winds one way and its holes the other, and
                // none cross: between each pair of crossings is inside.
                let (pairs, _) = crossings.as_chunks::<2>();
                for &[enter, leave] in pairs {
                    let (i0, i1) = grid.span(enter, leave, nx);
                    for i in i0..i1 {
                        if let Some(k) = grid.index(i, j) {
                            mask[k] = true;
                        }
                    }
                }
            }
        }
        mask
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
    use dc_geometry::{Contour, Polyline, assert_approx_eq, nest};

    const TOP_MM: f64 = 12.0;

    fn design(width_mm: f64, height_mm: f64, cell_mm: f64) -> Design {
        let grid = Grid::of(width_mm, height_mm, CellSpec::CellMm(cell_mm)).unwrap();
        Design::uncut(grid, TOP_MM)
    }

    fn regions(outlines: Vec<Polyline>) -> Vec<Region> {
        let contours = outlines
            .into_iter()
            .map(|p| Contour::new(p).unwrap())
            .collect();
        nest(contours).unwrap().0
    }

    fn at(design: &Design, x: f64, y: f64) -> (f64, Option<u16>) {
        let (i, j) = design.grid().cell_at(Point::new(x, y)).unwrap();
        (design.z_mm(i, j).unwrap(), design.asked_by(i, j))
    }

    /// Acceptance 146: a Pocket asks for its floor inside its region and
    /// the top on its island; a cell is in the region when its centre is,
    /// along straight sides and round ones.
    #[test]
    fn a_region_is_lowered_and_its_island_left() {
        let mut design = design(60.0, 40.0, 0.25);
        let pocket = regions(vec![
            Polyline::rect(Point::new(10.0, 10.0), 40.0, 20.0),
            Polyline::circle(Point::new(30.0, 20.0), 5.0),
        ]);
        assert_eq!(pocket.len(), 1, "a rectangle with a hole");
        design.lower_regions(&pocket, TOP_MM - 4.0, 3);
        assert_eq!(at(&design, 12.0, 12.0), (8.0, Some(3)));
        assert_eq!(at(&design, 30.0, 20.0), (TOP_MM, None), "the island");
        assert_eq!(at(&design, 5.0, 20.0), (TOP_MM, None), "outside");
        // Every cell by its centre: inside the rectangle and outside the
        // circle, a few microns of arc flattening aside.
        let grid = *design.grid();
        for j in 0..grid.ny() {
            for i in 0..grid.nx() {
                let c = grid.centre(i, j);
                let in_rect = c.x > 10.0 && c.x < 50.0 && c.y > 10.0 && c.y < 30.0;
                let from_centre = c.distance_to(Point::new(30.0, 20.0));
                if (from_centre - 5.0).abs() < 0.05 {
                    continue;
                }
                let want = in_rect && from_centre > 5.0;
                assert_eq!(
                    design.asked_by(i, j).is_some(),
                    want,
                    "cell {i}, {j} at {c:?}"
                );
            }
        }
    }

    /// A cell keeps the lowest asked of it and the operation that asked;
    /// nothing is asked below the bottom or above the top.
    #[test]
    fn the_lowest_asked_wins_and_stays_in_the_board() {
        let mut design = design(40.0, 30.0, 0.5);
        let wide = regions(vec![Polyline::rect(Point::new(5.0, 5.0), 30.0, 20.0)]);
        let narrow = regions(vec![Polyline::rect(Point::new(15.0, 10.0), 10.0, 10.0)]);
        design.lower_regions(&wide, 9.0, 0);
        design.lower_regions(&narrow, 6.0, 1);
        design.lower_regions(&wide, 10.0, 2);
        assert_eq!(at(&design, 8.0, 8.0), (9.0, Some(0)));
        assert_eq!(at(&design, 20.0, 15.0), (6.0, Some(1)));
        // A through cut asks for the bottom, and no lower.
        design.lower_regions(&narrow, -0.2, 4);
        assert_eq!(at(&design, 20.0, 15.0), (0.0, Some(4)));
        // Asking above the top asks nothing.
        design.lower_regions(&wide, TOP_MM + 3.0, 5);
        assert!(
            design
                .heights()
                .iter()
                .all(|&z| (0.0..=TOP_MM).contains(&z))
        );
        assert_eq!(at(&design, 8.0, 8.0), (9.0, Some(0)));
    }

    /// Acceptance 146: an Outside Profile asks for a trench its tool's
    /// width wide round its shape, and an Inside Profile for one inside
    /// it; the shape's own corners stay sharp, so the corner a round bit
    /// cannot reach is in the trench.
    #[test]
    fn a_band_is_the_tools_width_beside_the_edge() {
        let cell_mm = 0.25;
        let square = regions(vec![Polyline::rect(Point::new(20.0, 10.0), 20.0, 20.0)]);
        let mut outside = design(60.0, 40.0, cell_mm);
        outside.lower_band(&square, 6.0, BandSide::Outside, 0.0, 1);
        let mut inside = design(60.0, 40.0, cell_mm);
        inside.lower_band(&square, 6.0, BandSide::Inside, 0.0, 2);
        let grid = *outside.grid();
        for j in 0..grid.ny() {
            for i in 0..grid.nx() {
                let c = grid.centre(i, j);
                // Signed distance to the square: negative inside.
                let (dx, dy) = ((c.x - 30.0).abs() - 10.0, (c.y - 20.0).abs() - 10.0);
                let signed = if dx > 0.0 || dy > 0.0 {
                    dx.max(0.0).hypot(dy.max(0.0))
                } else {
                    dx.max(dy)
                };
                // Within a cell of either edge of a band the grid may
                // say either.
                let clear = |edge: f64| (signed - edge).abs() > cell_mm;
                if clear(0.0) && clear(6.0) {
                    assert_eq!(
                        outside.asked_by(i, j).is_some(),
                        signed > 0.0 && signed < 6.0,
                        "outside, cell {i}, {j}, {signed} mm out"
                    );
                }
                if clear(0.0) && clear(-6.0) {
                    assert_eq!(
                        inside.asked_by(i, j).is_some(),
                        signed < 0.0 && signed > -6.0,
                        "inside, cell {i}, {j}, {signed} mm out"
                    );
                }
            }
        }
        // The inside corner itself is asked for.
        assert_eq!(at(&inside, 20.3, 10.3), (0.0, Some(2)));
        // An island's edge has a trench too: outside a ring is in its hole.
        let ring = regions(vec![
            Polyline::circle(Point::new(30.0, 20.0), 15.0),
            Polyline::circle(Point::new(30.0, 20.0), 8.0),
        ]);
        let mut round = design(60.0, 40.0, cell_mm);
        round.lower_band(&ring, 3.0, BandSide::Outside, 0.0, 0);
        assert_eq!(
            at(&round, 30.0 + 6.5, 20.0).1,
            Some(0),
            "in the hole, by its edge"
        );
        assert_eq!(at(&round, 30.0, 20.0).1, None, "the hole's middle");
        assert_eq!(at(&round, 30.0 + 11.0, 20.0).1, None, "the ring itself");
        assert_eq!(at(&round, 30.0 + 16.5, 20.0).1, Some(0), "outside the ring");
    }

    /// A Profile on its line asks for what its tool sweeps: within the
    /// tool's radius of the path, arcs and all.
    #[test]
    fn a_path_is_lowered_within_reach() {
        let mut design = design(60.0, 40.0, 0.25);
        let path = Polyline::new(
            vec![
                dc_geometry::Vertex::new(10.0, 10.0, 0.0),
                dc_geometry::Vertex::new(40.0, 10.0, 1.0),
                dc_geometry::Vertex::new(40.0, 30.0, 0.0),
            ],
            false,
        );
        design.lower_near(&[path], 2.0, 7.0, 5);
        assert_eq!(at(&design, 25.0, 11.5), (7.0, Some(5)), "beside the line");
        assert_eq!(at(&design, 25.0, 12.5), (TOP_MM, None), "past its reach");
        assert_eq!(at(&design, 8.6, 10.0), (7.0, Some(5)), "round its end");
        // The arc bulges to +X: a half circle of radius 10 about (40, 20).
        assert_eq!(at(&design, 49.0, 20.0), (7.0, Some(5)), "on the arc");
        assert_eq!(at(&design, 40.0, 20.0), (TOP_MM, None), "the arc's centre");
    }

    /// A tab stands in its Profile's trench: the cells that Profile asked
    /// lower are raised to the tab's top round the tab's centre, and
    /// nothing else is.
    #[test]
    fn a_tab_is_raised_in_its_own_trench() {
        let mut design = design(40.0, 30.0, 0.5);
        let trench = regions(vec![Polyline::rect(Point::new(5.0, 10.0), 30.0, 6.0)]);
        let pocket = regions(vec![Polyline::rect(Point::new(5.0, 20.0), 30.0, 6.0)]);
        design.lower_regions(&trench, 0.0, 7);
        design.lower_regions(&pocket, 8.0, 2);
        // The tool, 6 mm wide, rides over the tab from x 15 to x 29 along
        // the trench's middle: 8 mm of tab and the tool's width. Its
        // reach is given a margin that takes in the pocket's edge too.
        let ride = Polyline::new(
            vec![
                dc_geometry::Vertex::new(15.0, 13.0, 0.0),
                dc_geometry::Vertex::new(29.0, 13.0, 0.0),
            ],
            false,
        );
        design.raise_near(&[ride], 7.5, 3.0, 7);
        assert_eq!(at(&design, 22.0, 13.0), (3.0, Some(7)), "the tab");
        assert_eq!(at(&design, 30.0, 15.5), (3.0, Some(7)), "its far corner");
        assert_eq!(
            at(&design, 6.0, 13.0),
            (0.0, Some(7)),
            "the trench beside it"
        );
        assert_eq!(
            at(&design, 22.0, 20.4),
            (8.0, Some(2)),
            "another operation's floor"
        );
        assert_eq!(at(&design, 22.0, 9.0), (TOP_MM, None), "outside the trench");
    }

    /// Acceptance 146: a V-Carve asks for `depth = distance ÷ tan(α/2)`
    /// from the nearest edge, no deeper than the flat depth. On a bar
    /// 12 mm wide with a 90° bit: a cell `d` from the nearer long edge is
    /// `d` deep, to within a cell, 6 mm at the middle; with a 2 mm flat
    /// depth the floor is flat from 2 mm in.
    #[test]
    fn lower_cone_is_distance_over_slope_capped() {
        let cell_mm = 0.25;
        let bar = regions(vec![Polyline::rect(Point::new(10.0, 9.0), 60.0, 12.0)]);
        let mut sharp = design(80.0, 30.0, cell_mm);
        sharp.lower_cone(&bar, TOP_MM, 1.0, None, 0);
        let mut flat = design(80.0, 30.0, cell_mm);
        flat.lower_cone(&bar, TOP_MM, 1.0, Some(2.0), 0);
        let grid = *sharp.grid();
        for j in 0..grid.ny() {
            // Along the bar's middle, far from its ends.
            let c = grid.centre(160, j);
            let from_edge = (c.y - 9.0).min(21.0 - c.y);
            let (deep, capped) = (sharp.z_mm(160, j).unwrap(), flat.z_mm(160, j).unwrap());
            if from_edge <= 0.0 {
                assert_approx_eq!(deep, TOP_MM, 0.0);
                assert_approx_eq!(capped, TOP_MM, 0.0);
            } else {
                assert_approx_eq!(TOP_MM - deep, from_edge, cell_mm);
                assert_approx_eq!(TOP_MM - capped, from_edge.min(2.0), cell_mm);
            }
        }
        let (middle, op) = at(&sharp, 40.0, 15.0);
        assert_approx_eq!(TOP_MM - middle, 6.0, cell_mm);
        assert_eq!(op, Some(0));
        // A narrower angle carves deeper for the same width: 60° is
        // 1 / tan 30° = 1.73 times as deep.
        let mut steep = design(80.0, 30.0, cell_mm);
        steep.lower_cone(&bar, TOP_MM, 30.0_f64.to_radians().tan(), None, 0);
        let (z, _) = at(&steep, 40.0, 13.0);
        assert_approx_eq!(TOP_MM - z, 4.0 / 30.0_f64.to_radians().tan(), 2.0 * cell_mm);
        // A start plane below the top carves from there.
        let mut started = design(80.0, 30.0, cell_mm);
        started.lower_cone(&bar, TOP_MM - 3.0, 1.0, Some(2.0), 0);
        assert_approx_eq!(at(&started, 40.0, 15.0).0, TOP_MM - 5.0, 1e-9);
    }

    /// The design is only ever at or below the uncut top, whatever is
    /// asked: the reason uncut material needs no notion of part and
    /// waste.
    #[test]
    fn the_design_is_never_above_the_top() {
        let mut design = design(40.0, 30.0, 0.5);
        let shape = regions(vec![Polyline::circle(Point::new(20.0, 15.0), 9.0)]);
        design.lower_cone(&shape, TOP_MM + 5.0, 0.5, None, 0);
        design.lower_regions(&shape, 20.0, 1);
        design.lower_band(&shape, 4.0, BandSide::Outside, -3.0, 1);
        design.raise_near(
            &[Polyline::circle(Point::new(20.0, 15.0), 6.0)],
            30.0,
            99.0,
            1,
        );
        design.lower_cone(&shape, TOP_MM, f64::NAN, None, 2);
        design.lower_cone(&shape, TOP_MM, 0.0, None, 2);
        assert!(
            design
                .heights()
                .iter()
                .all(|&z| (0.0..=TOP_MM).contains(&z))
        );
    }
}
