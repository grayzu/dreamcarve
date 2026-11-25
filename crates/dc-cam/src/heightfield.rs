// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A heightfield simulation of a carve's programs, for tests (#452,
//! #456): the programs themselves, not the outlines they were built from,
//! decide what is cut.
//!
//! Runs are cut into a grid of heights. A cell keeps the
//! lowest Z any tool's surface reaches over it: a V-bit's cone, `|q − p| /
//! slope` above its tip, a tapered ball nose's ball and cone, `w⁻¹(|q − p|)`
//! above it (#679), or an end mill's flat disc. Each cell's height is
//! exact for every straight move (the cone's and the ball's least height
//! over a move are found in closed form, the disc's at the lower end of the stretch it
//! covers); arcs are cut as chords within [`CHORD_MM`]. A coarse grid
//! therefore samples the true surface at fewer points, but never reports
//! a cell higher than the programs leave it.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    clippy::indexing_slicing,
    reason = "grid indices and chord counts are small, non-negative and clamped"
)]
// Built for tests, and with the `test-support` feature for the seat check
// other crates' tests run (#464); some helpers are this crate's tests' only.
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "the seat check uses part of the module")
)]

use crate::run::ToolRun;
use crate::toolpath::{Move, MoveKind, Toolpath};
use dc_geometry::profile::BitProfile;
use dc_geometry::{Arc, Point, Point3, Rect};
// The closed forms themselves are the simulation's kernel since phase 6
// (`docs/specs/phase-6/design.md`, "The cut of one move"): one kernel,
// which these tests wrote first.
use dc_sim::sweep::disc_low_mm;
pub(crate) use dc_sim::sweep::{cone_low_mm, profile_low_mm};

/// How far an arc's chords may stray from it, mm.
const CHORD_MM: f64 = 0.001;

/// The cutting surface of a tool, as the heightfield sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Cutter {
    /// A V-bit: a cone of radius `(z − tip) · slope` at height `z`.
    VBit {
        /// `tan(α/2)`.
        slope: f64,
    },
    /// A tapered ball nose (slice 27c, #679): a ball of `tip_radius_mm`
    /// tangent to a cone of `slope` (`tan β`), capped at `cap_radius_mm`.
    TaperedBall {
        /// The ball's radius `ρ`, mm.
        tip_radius_mm: f64,
        /// `tan β`.
        slope: f64,
        /// Half the cutting diameter, mm.
        cap_radius_mm: f64,
    },
    /// An end mill's flat bottom.
    Flat {
        /// The radius, mm.
        radius_mm: f64,
    },
}

/// The heights a side's runs leave on its stock, on a square grid. With a
/// mirror line, cell `(i, j)` holds the height at the point mirrored about
/// it: the male's grid is laid over the female's, as the plug seats.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Heightfield {
    /// The first cell's centre, mm, in the grid's (the female's) frame.
    origin: Point,
    /// The cell size, mm.
    step_mm: f64,
    /// Cells across and up.
    nx: usize,
    ny: usize,
    /// X of the line the toolpaths are mirrored about, mm, if any.
    mirror_x_mm: Option<f64>,
    /// The stock top, mm.
    pub top_z_mm: f64,
    /// Heights, row by row from the bottom, mm.
    z_mm: Vec<f64>,
}

impl Heightfield {
    /// Uncut stock `top_z_mm` high over `bounds` (the female's frame),
    /// cells `step_mm` apart; toolpaths are read mirrored about X =
    /// `mirror_x_mm` when it is set.
    pub(crate) fn new(bounds: Rect, step_mm: f64, top_z_mm: f64, mirror_x_mm: Option<f64>) -> Self {
        let count = |span: f64| (span / step_mm).ceil() as usize + 1;
        let (nx, ny) = (
            count(bounds.max.x - bounds.min.x),
            count(bounds.max.y - bounds.min.y),
        );
        Self {
            origin: bounds.min,
            step_mm,
            nx,
            ny,
            mirror_x_mm,
            top_z_mm,
            z_mm: vec![top_z_mm; nx * ny],
        }
    }

    /// Cells across and up.
    pub(crate) fn size(&self) -> (usize, usize) {
        (self.nx, self.ny)
    }

    /// The height of cell `(i, j)`, mm.
    pub(crate) fn z_mm(&self, i: usize, j: usize) -> f64 {
        self.z_mm[j * self.nx + i]
    }

    /// Sets the height of cell `(i, j)`, mm: a test's own field.
    #[cfg(test)]
    pub(crate) fn set_z_mm(&mut self, i: usize, j: usize, z_mm: f64) {
        self.z_mm[j * self.nx + i] = z_mm;
    }

    /// Cell `(i, j)`'s centre in the grid's frame.
    pub(crate) fn cell(&self, i: usize, j: usize) -> Point {
        Point::new(
            self.origin.x + i as f64 * self.step_mm,
            self.origin.y + j as f64 * self.step_mm,
        )
    }

    /// Cell `(i, j)`'s centre in the toolpaths' frame.
    pub(crate) fn sample(&self, i: usize, j: usize) -> Point {
        let p = self.cell(i, j);
        self.mirror_x_mm
            .map_or(p, |cx| Point::new(2.0 * cx - p.x, p.y))
    }

    /// Cuts every run of `runs`, each with `cutter_of(run)`.
    pub(crate) fn cut_runs(&mut self, runs: &[ToolRun], cutter_of: impl Fn(&ToolRun) -> Cutter) {
        for run in runs {
            let cutter = cutter_of(run);
            let mut from: Option<Point3> = None;
            for mv in &run.toolpath.moves {
                if let Some(a) = from
                    && mv.is_cut()
                {
                    for (p, q) in chords(a, mv) {
                        self.cut_segment(p, q, cutter);
                    }
                }
                from = Some(mv.to);
            }
        }
    }

    /// Cuts the straight move `a`–`b` with `cutter`.
    fn cut_segment(&mut self, a: Point3, b: Point3, cutter: Cutter) {
        let low_mm = a.z.min(b.z);
        let reach_mm = match cutter {
            Cutter::VBit { slope } => (self.top_z_mm - low_mm) * slope,
            Cutter::TaperedBall { cap_radius_mm, .. } => cap_radius_mm,
            Cutter::Flat { radius_mm } => radius_mm,
        };
        if low_mm >= self.top_z_mm || reach_mm <= 0.0 {
            return;
        }
        // The cells the move can reach, in the grid's frame.
        let (x0, x1) = match self.mirror_x_mm {
            None => (a.x.min(b.x), a.x.max(b.x)),
            Some(cx) => (2.0 * cx - a.x.max(b.x), 2.0 * cx - a.x.min(b.x)),
        };
        let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
        let index = |v: f64, origin: f64, n: usize| -> usize {
            (((v - origin) / self.step_mm).max(0.0) as usize).min(n)
        };
        let i0 = index(x0 - reach_mm, self.origin.x, self.nx);
        let i1 = index(x1 + reach_mm + self.step_mm, self.origin.x, self.nx);
        let j0 = index(y0 - reach_mm, self.origin.y, self.ny);
        let j1 = index(y1 + reach_mm + self.step_mm, self.origin.y, self.ny);
        for j in j0..j1 {
            for i in i0..i1 {
                let q = self.sample(i, j);
                let z = match cutter {
                    Cutter::VBit { slope } => cone_low_mm(a, b, q, slope),
                    Cutter::TaperedBall {
                        tip_radius_mm,
                        slope,
                        cap_radius_mm,
                    } => profile_low_mm(
                        a,
                        b,
                        q,
                        &BitProfile::TaperedBall {
                            tip_radius_mm,
                            slope,
                            cap_radius_mm,
                        },
                    ),
                    Cutter::Flat { radius_mm } => disc_low_mm(a, b, q, radius_mm),
                };
                let cell = &mut self.z_mm[j * self.nx + i];
                if z < *cell {
                    *cell = z;
                }
            }
        }
    }
}

/// The lowest a V-bit cone of `slope` reaches over `q` along any cutting
/// move of `toolpath`, mm (infinite when it cuts nothing): exact for
/// straight moves, arcs as chords within [`CHORD_MM`]. The carve reaches
/// `q` within `δ` at the plane `z` exactly when this is at most `z + δ /
/// slope`.
pub(crate) fn carved_z_mm(toolpath: &Toolpath, q: Point, slope: f64) -> f64 {
    let mut low = f64::INFINITY;
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        if let Some(a) = from
            && mv.is_cut()
        {
            for (p, r) in chords(a, mv) {
                low = low.min(cone_low_mm(p, r, q, slope));
            }
        }
        from = Some(mv.to);
    }
    low
}

/// The lowest a bit of `profile` reaches over `q` along any cutting move
/// of `toolpath`, mm (infinite when it cuts nothing), as
/// [`carved_z_mm`] reads a V-bit's.
pub(crate) fn carved_profile_z_mm(toolpath: &Toolpath, q: Point, profile: &BitProfile) -> f64 {
    let mut low = f64::INFINITY;
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        if let Some(a) = from
            && mv.is_cut()
        {
            for (p, r) in chords(a, mv) {
                low = low.min(profile_low_mm(p, r, q, profile));
            }
        }
        from = Some(mv.to);
    }
    low
}

/// The straight pieces of the move `mv` from `from`: itself, or an arc's
/// chords within [`CHORD_MM`], Z falling or rising evenly along it.
pub(crate) fn chords(from: Point3, mv: &Move) -> Vec<(Point3, Point3)> {
    let (centre, ccw) = match mv.kind {
        MoveKind::ArcCw { centre } => (centre, false),
        MoveKind::ArcCcw { centre } => (centre, true),
        _ => return vec![(from, mv.to)],
    };
    let arc = Arc::from_ends(from.xy(), mv.to.xy(), centre, ccw);
    let r = arc.radius_mm;
    let per_chord_rad = if r > CHORD_MM {
        2.0 * (1.0 - CHORD_MM / r).acos()
    } else {
        std::f64::consts::PI
    };
    let n = ((arc.sweep_rad.abs() / per_chord_rad).ceil() as usize).max(1);
    let point = |k: usize| {
        if k == n {
            return mv.to;
        }
        let t = k as f64 / n as f64;
        let angle = arc.start_angle_rad + arc.sweep_rad * t;
        Point3::new(
            centre.x + r * angle.cos(),
            centre.y + r * angle.sin(),
            from.z + (mv.to.z - from.z) * t,
        )
    };
    (0..n).map(|k| (point(k), point(k + 1))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The bench's Ultra-High Detail bit: tip radius 0.381 mm, 6.7° per
    /// side, cutting diameter 6.35 mm.
    fn ultra() -> (Cutter, BitProfile) {
        let (tip_radius_mm, slope, cap_radius_mm) = (0.381, 6.7_f64.to_radians().tan(), 3.175);
        (
            Cutter::TaperedBall {
                tip_radius_mm,
                slope,
                cap_radius_mm,
            },
            BitProfile::TaperedBall {
                tip_radius_mm,
                slope,
                cap_radius_mm,
            },
        )
    }

    /// Design step 8: in a plunge, a cell 0.2 mm from the axis (in the
    /// ball, under `w_t` 0.378) is `w⁻¹(0.2) = ρ − √(ρ² − 0.04)` above the
    /// tip, and one 1 mm out is on the cone, `1 / t − δ` above it.
    #[test]
    fn tapered_tool_drops_as_ball_then_cone() {
        let (cutter, profile) = ultra();
        let top_mm = 20.0;
        let tip = Point3::new(0.0, 0.0, 12.0);
        let mut field = Heightfield::new(
            Rect {
                min: Point::new(-2.0, -2.0),
                max: Point::new(2.0, 2.0),
            },
            0.1,
            top_mm,
            None,
        );
        field.cut_segment(Point3::new(0.0, 0.0, top_mm + 1.0), tip, cutter);
        let (nx, ny) = field.size();
        let cell_at = |x: f64| {
            let mut best = (0, 0);
            for j in 0..ny {
                for i in 0..nx {
                    let c = field.cell(i, j);
                    let b = field.cell(best.0, best.1);
                    if (c.x - x).abs() + c.y.abs() < (b.x - x).abs() + b.y.abs() {
                        best = (i, j);
                    }
                }
            }
            best
        };
        let rho: f64 = 0.381;
        for x in [0.2, 1.0] {
            let (i, j) = cell_at(x);
            let r = field.cell(i, j).distance_to(Point::new(0.0, 0.0));
            assert!((r - x).abs() < 1e-9, "the cell at {r}");
            let want = if x < profile.tangent_half_width_mm() {
                rho - (rho * rho - r * r).sqrt()
            } else {
                r / profile.slope() - profile.apex_offset_mm()
            };
            assert!(
                (field.z_mm(i, j) - (tip.z + want)).abs() < 1e-9,
                "{x} mm out: {} above the tip, want {want}",
                field.z_mm(i, j) - tip.z
            );
            assert!((want - profile.height_mm(r).expect("below the cap")).abs() < 1e-12);
        }
    }
}
