// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the pointer is over (phase 6 functional spec, capability 111):
//! where a ray from the eye meets the simulated stock, for the read-out.
//!
//! The ray is walked across the grid a cell at a time, by its shadow on
//! the board, from where it enters the board's box to where it leaves:
//! the first cell whose stock the ray is at or below is the one hit, on
//! its top where the ray comes down onto it, or on its wall where the
//! ray runs into its side.

use dc_geometry::{Point, Point3};
use dc_job::sim::Heightfield;

use super::camera::Ray;

/// Where a ray meets the stock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    /// The point, mm, project space.
    pub at: Point3,
    /// The cell hit.
    pub cell: (u32, u32),
    /// The run that cut the cell last, as its place in the plan; `None`
    /// for uncut stock.
    pub cut_by: Option<u16>,
}

/// Where `ray` first meets the stock of `field`; `None` when it passes
/// the board by, or meets only where the stock is cut through.
#[must_use]
pub fn probe(field: &Heightfield, ray: Ray) -> Option<Hit> {
    let grid = *field.grid();
    let (o, d) = (ray.origin, ray.direction);
    // The stretch of the ray inside the board's box.
    let (mut t0, mut t1) = (0.0_f64, f64::INFINITY);
    for (origin, direction, low, high) in [
        (o.x, d.x, 0.0, grid.width_mm()),
        (o.y, d.y, 0.0, grid.height_mm()),
        (o.z, d.z, 0.0, field.top_z_mm()),
    ] {
        if direction.abs() < 1e-12 {
            if origin < low || origin > high {
                return None;
            }
            continue;
        }
        let (a, b) = ((low - origin) / direction, (high - origin) / direction);
        t0 = t0.max(a.min(b));
        t1 = t1.min(a.max(b));
    }
    if t0 > t1 {
        return None;
    }

    let cell_mm = grid.cell_mm();
    // The crossings of cell edges along X and along Y, in order.
    let step = |origin: f64, direction: f64, t: f64| -> (f64, f64) {
        if direction.abs() < 1e-12 {
            return (f64::INFINITY, f64::INFINITY);
        }
        let at = origin + direction * t;
        let cell = (at / cell_mm).floor();
        let next_edge = if direction > 0.0 { cell + 1.0 } else { cell } * cell_mm;
        let mut next = (next_edge - origin) / direction;
        if next <= t {
            next += cell_mm / direction.abs();
        }
        (next, cell_mm / direction.abs())
    };
    let (mut next_x, per_x) = step(o.x, d.x, t0);
    let (mut next_y, per_y) = step(o.y, d.y, t0);
    let mut t = t0;
    // A ray crosses at most this many cells.
    let most = (grid.nx() as usize + grid.ny() as usize + 2) * 2;
    for _ in 0..most {
        let leave = next_x.min(next_y).min(t1);
        let mid = ray.at(f64::midpoint(t, leave));
        if let Some((i, j)) = grid.cell_at(Point::new(mid.x, mid.y))
            && let Some(z) = field.z_mm(i, j)
            && z > 0.0
        {
            let (z_in, z_out) = (o.z + d.z * t, o.z + d.z * leave);
            if z_in.min(z_out) <= z {
                // Into the cell's side when it enters below the top,
                // else down onto the top.
                let at = if z_in <= z || d.z.abs() < 1e-12 {
                    t
                } else {
                    (z - o.z) / d.z
                };
                return Some(Hit {
                    at: ray.at(at.clamp(t, leave)),
                    cell: (i, j),
                    cut_by: field.cut_by(i, j),
                });
            }
        }
        if leave >= t1 {
            break;
        }
        t = leave;
        if next_x <= next_y {
            next_x += per_x;
        } else {
            next_y += per_y;
        }
    }
    None
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
    use crate::sim::mat::Vec3;
    use dc_geometry::assert_approx_eq;
    use dc_job::sim::{CellSpec, Cutter, Grid, SimMove};

    /// A stock 40 × 30 × 12 with a slot 6 wide and 4 deep along X at
    /// y = 15, from x 10 to x 30, cut as run 3.
    fn slotted() -> Heightfield {
        let grid = Grid::of(40.0, 30.0, CellSpec::CellMm(0.25)).unwrap();
        let mut field = Heightfield::uncut(grid, 12.0).unwrap();
        let cut = SimMove::cut(
            Point3::new(10.0, 15.0, 8.0),
            Point3::new(30.0, 15.0, 8.0),
            1.0,
        );
        field.cut(&cut, (0.0, 1.0), &Cutter::Flat { radius_mm: 3.0 }, 3);
        field
    }

    fn ray(from: (f64, f64, f64), toward: (f64, f64, f64)) -> Ray {
        let origin = Point3::new(from.0, from.1, from.2);
        let direction = Vec3::between(origin, Point3::new(toward.0, toward.1, toward.2))
            .normalized()
            .unwrap();
        Ray { origin, direction }
    }

    /// Acceptance 152: straight down over the slot's floor the read-out
    /// is the floor's height and the run that cut it; over the uncut top
    /// it is the top and no run.
    #[test]
    fn reads_a_slots_floor_and_its_run() {
        let field = slotted();
        let floor = probe(&field, ray((20.0, 15.0, 100.0), (20.0, 15.0, 0.0))).unwrap();
        assert_approx_eq!(floor.at.z, 8.0, 1e-9);
        assert_approx_eq!(floor.at.x, 20.0, 1e-9);
        assert_eq!(floor.cut_by, Some(3));
        let top = probe(&field, ray((5.1, 5.1, 100.0), (5.1, 5.1, 0.0))).unwrap();
        assert_approx_eq!(top.at.z, 12.0, 1e-9);
        assert_eq!(top.cut_by, None);
        assert_eq!(
            top.cell,
            field.grid().cell_at(Point::new(5.1, 5.1)).unwrap()
        );
    }

    /// A slanting ray comes down onto the first top it reaches, runs
    /// into a wall where one stands in its way, and reads the floor only
    /// where it can see it.
    #[test]
    fn a_slanting_ray_meets_what_is_in_front() {
        let field = slotted();
        // From the front, 45° down, aimed at the slot's floor at y 15:
        // it passes over the near wall's top edge at y 12 only if it is
        // above 12 there; aimed at the floor's middle it is 11 high at
        // y 12, so it meets the top face before the slot.
        let blocked = probe(&field, ray((20.0, -30.0, 53.0), (20.0, 15.0, 8.0))).unwrap();
        assert_approx_eq!(blocked.at.z, 12.0, 1e-9);
        assert!(
            blocked.at.y < 12.0,
            "on the top, in front of the slot: {:?}",
            blocked.at
        );
        assert_eq!(blocked.cut_by, None);
        // Steeper, it sees the floor.
        let seen = probe(&field, ray((20.0, 10.0, 60.0), (20.0, 16.0, 8.0))).unwrap();
        assert_approx_eq!(seen.at.z, 8.0, 1e-9);
        assert_eq!(seen.cut_by, Some(3));
        // Along the slot from inside it, level and below the top, it
        // runs into the slot's far end wall.
        let wall = probe(&field, ray((12.0, 15.0, 10.0), (60.0, 15.0, 10.0))).unwrap();
        assert_approx_eq!(wall.at.z, 10.0, 1e-9);
        assert!(
            wall.at.x > 32.9 && wall.at.x < 33.3,
            "the end wall is at x 33: {:?}",
            wall.at
        );
        assert_eq!(wall.cut_by, None);
        // From outside, level, it meets the stock's own side.
        let side = probe(&field, ray((-20.0, 5.0, 6.0), (60.0, 5.0, 6.0))).unwrap();
        assert_approx_eq!(side.at.x, 0.0, 1e-9);
        assert_approx_eq!(side.at.z, 6.0, 1e-9);
    }

    /// A ray that passes the board by, looks away from it, or comes down
    /// where it is cut through, reads nothing.
    #[test]
    fn nothing_is_read_off_the_stock() {
        let mut field = slotted();
        assert_eq!(
            probe(&field, ray((-5.0, -5.0, 100.0), (-5.0, -5.0, 0.0))),
            None
        );
        assert_eq!(
            probe(&field, ray((20.0, 15.0, 100.0), (20.0, 15.0, 200.0))),
            None
        );
        assert_eq!(
            probe(&field, ray((-10.0, 50.0, 20.0), (50.0, 50.0, 20.0))),
            None
        );
        let hole = SimMove::cut(
            Point3::new(5.0, 25.0, -1.0),
            Point3::new(5.0, 25.0, -1.0),
            0.0,
        );
        field.cut(&hole, (0.0, 1.0), &Cutter::Flat { radius_mm: 2.0 }, 0);
        assert_eq!(
            probe(&field, ray((5.0, 25.0, 100.0), (5.0, 25.0, 0.0))),
            None
        );
    }
}
