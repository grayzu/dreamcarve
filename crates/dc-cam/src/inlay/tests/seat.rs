// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The seat check of an inlay pair (#452), built on the heightfield
//! simulation of [`crate::heightfield`]: the programs themselves, not the
//! outlines they were built from, decide whether the plug seats.

use crate::heightfield::Heightfield;
use dc_geometry::Point;
use std::collections::VecDeque;

/// How far above its start plane the male's band floor may read and still
/// part the plug from the board round it, mm: the band's edges meet the
/// start plane exactly.
const FLOOR_SLACK_MM: f64 = 0.01;

/// One cell of the plug, seated ([`seat`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct SeatCell {
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
pub(super) fn seat(
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
