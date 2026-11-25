// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! How far each cell of a shape is from the shape's edge
//! (`docs/specs/phase-6/design.md`, "Algorithms → The design surface"):
//! the exact Euclidean distance transform of a mask of cells, by the
//! two-pass lower envelope of parabolas of Felzenszwalb and Huttenlocher
//! ("Distance Transforms of Sampled Functions", 2012), written from the
//! paper. Each pass is linear in the cells of a line, so the whole is
//! linear in the grid.

#![allow(
    clippy::indexing_slicing,
    reason = "every index is bounded by the line's or the grid's own length a few lines above"
)]

/// A squared distance that stands for "no cell outside the shape": far
/// past any grid's diagonal, and finite, so the envelope's arithmetic
/// has no infinity minus infinity.
const FAR: f64 = 1e20;

/// For each cell of an `nx × ny` mask, row-major: 0 for a cell outside
/// the shape (`false`), and for a cell inside it the distance, in cells,
/// from its centre to the centre of the nearest cell outside. A mask
/// with no cell outside gives a distance past any grid's size
/// everywhere.
pub(crate) fn distances_cells(mask: &[bool], nx: usize, ny: usize) -> Vec<f64> {
    debug_assert_eq!(mask.len(), nx * ny, "a mask of the grid's size");
    let mut squared: Vec<f64> = mask
        .iter()
        .map(|&inside| if inside { FAR } else { 0.0 })
        .collect();
    let longest = nx.max(ny);
    let (mut line, mut out) = (vec![0.0; longest], vec![0.0; longest]);
    let mut scratch = Envelope::new(longest);
    // Along each column, then along each row of the result.
    for i in 0..nx {
        for j in 0..ny {
            line[j] = squared[j * nx + i];
        }
        scratch.transform(&line[..ny], &mut out[..ny]);
        for j in 0..ny {
            squared[j * nx + i] = out[j];
        }
    }
    for j in 0..ny {
        let row = j * nx..(j + 1) * nx;
        line[..nx].copy_from_slice(&squared[row.clone()]);
        scratch.transform(&line[..nx], &mut out[..nx]);
        squared[row].copy_from_slice(&out[..nx]);
    }
    squared.iter().map(|d2| d2.sqrt()).collect()
}

/// The lower envelope of the parabolas `f[q] + (p − q)²`, kept between
/// lines so no line allocates.
struct Envelope {
    /// The sample each parabola of the envelope sits over.
    vertex: Vec<usize>,
    /// Where each parabola of the envelope starts being the lowest.
    from: Vec<f64>,
}

impl Envelope {
    fn new(longest: usize) -> Self {
        Self {
            vertex: vec![0; longest],
            from: vec![0.0; longest + 1],
        }
    }

    /// `out[p] = min over q of f[q] + (p − q)²`.
    fn transform(&mut self, f: &[f64], out: &mut [f64]) {
        let n = f.len();
        if n == 0 {
            return;
        }
        // Indices and counts are far under 2⁵³.
        #[expect(clippy::cast_precision_loss, reason = "cell indices are far under 2⁵³")]
        let at = |q: usize| q as f64;
        let (vertex, from) = (&mut self.vertex, &mut self.from);
        let mut k = 0;
        vertex[0] = 0;
        from[0] = f64::NEG_INFINITY;
        from[1] = f64::INFINITY;
        for q in 1..n {
            // Where the parabola over `q` meets the envelope's last.
            let meet = |v: usize| {
                ((f[q] + at(q) * at(q)) - (f[v] + at(v) * at(v))) / (2.0 * at(q) - 2.0 * at(v))
            };
            let mut s = meet(vertex[k]);
            while s <= from[k] && k > 0 {
                k -= 1;
                s = meet(vertex[k]);
            }
            k += 1;
            vertex[k] = q;
            from[k] = s;
            from[k + 1] = f64::INFINITY;
        }
        k = 0;
        for (p, slot) in out.iter_mut().enumerate().take(n) {
            while from[k + 1] < at(p) {
                k += 1;
            }
            let d = at(p) - at(vertex[k]);
            *slot = d * d + f[vertex[k]];
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::cast_precision_loss,
        reason = "test code reports failure by panicking"
    )]

    use super::*;
    use dc_geometry::assert_approx_eq;

    fn brute(mask: &[bool], nx: usize, ny: usize) -> Vec<f64> {
        let mut out = vec![0.0; nx * ny];
        for j in 0..ny {
            for i in 0..nx {
                if !mask[j * nx + i] {
                    continue;
                }
                let mut best = f64::INFINITY;
                for b in 0..ny {
                    for a in 0..nx {
                        if !mask[b * nx + a] {
                            let (dx, dy) = (a as f64 - i as f64, b as f64 - j as f64);
                            best = best.min(dx.hypot(dy));
                        }
                    }
                }
                out[j * nx + i] = best;
            }
        }
        out
    }

    /// The transform is the brute force, to rounding, on masks of every
    /// density from a fixed stream of numbers: blobs, specks, a mask with
    /// one cell outside.
    #[test]
    fn distances_equal_brute_force_on_random_masks() {
        let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for (nx, ny, one_in) in [
            (24, 18, 2),
            (31, 7, 3),
            (5, 40, 8),
            (20, 20, 40),
            (1, 9, 2),
            (13, 1, 3),
        ] {
            let mut mask: Vec<bool> = (0..nx * ny).map(|_| next() % one_in != 0).collect();
            mask[0] = false;
            let (got, want) = (distances_cells(&mask, nx, ny), brute(&mask, nx, ny));
            for (k, (g, w)) in got.iter().zip(&want).enumerate() {
                assert_approx_eq!(*g, *w, 1e-9);
                assert_eq!(*g > 0.0, mask[k], "cell {k} of {nx} × {ny}");
            }
        }
    }

    /// A bar 5 cells wide: the middle cell is 3 cells from the nearest
    /// centre outside, its neighbours 2, the edge cells 1.
    #[test]
    fn a_bars_cells_count_from_its_edge() {
        let (nx, ny) = (9, 30);
        let mask: Vec<bool> = (0..nx * ny).map(|k| (2..7).contains(&(k % nx))).collect();
        let d = distances_cells(&mask, nx, ny);
        let row = 15 * nx;
        let got: Vec<f64> = (0..nx).map(|i| d[row + i]).collect();
        assert_eq!(got, [0.0, 0.0, 1.0, 2.0, 3.0, 2.0, 1.0, 0.0, 0.0]);
    }

    /// A shape that fills the grid has no edge in it.
    #[test]
    fn a_full_mask_is_far_from_any_edge() {
        let d = distances_cells(&[true; 12], 4, 3);
        assert!(d.iter().all(|&v| v > 1e6));
        assert!(distances_cells(&[], 0, 0).is_empty());
    }
}
