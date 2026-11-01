// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A heightfield simulation of a carve's programs, for tests (#452,
//! #456): the programs themselves, not the outlines they were built from,
//! decide what is cut.
//!
//! Runs are cut into a grid of heights. A cell keeps the
//! lowest Z any tool's surface reaches over it: a V-bit's cone, `|q − p| /
//! slope` above its tip, or an end mill's flat disc. Each cell's height is
//! exact for every straight move (the cone's least height over a move is
//! found in closed form, the disc's at the lower end of the stretch it
//! covers); arcs are cut as chords within [`CHORD_MM`]. A coarse grid
//! therefore samples the true surface at fewer points, but never reports
//! a cell higher than the programs leave it.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "grid indices and chord counts are small, non-negative and clamped"
)]

use crate::run::ToolRun;
use crate::toolpath::{Move, MoveKind, Toolpath};
use dc_geometry::{Arc, Point, Point3, Rect};

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

/// The lowest a V-bit cone of `slope` reaches over `q` while its tip runs
/// straight from `a` to `b`: the least over `t` of `z(t) + |q − p(t)| /
/// slope`, which is convex in `t`, so its stationary point clamped to the
/// move is its least.
pub(crate) fn cone_low_mm(a: Point3, b: Point3, q: Point, slope: f64) -> f64 {
    let at = |t: f64| {
        let p = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
        a.z + (b.z - a.z) * t + q.distance_to(p) / slope
    };
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx.hypot(dy);
    if length < 1e-12 {
        return at(if a.z <= b.z { 0.0 } else { 1.0 });
    }
    let (ux, uy) = (dx / length, dy / length);
    let (qx, qy) = (q.x - a.x, q.y - a.y);
    // Along the move to the foot of `q`, mm, and the distance off it.
    let along = qx * ux + qy * uy;
    let off = (qx * uy - qy * ux).abs();
    // `f'(t) = dz + L·w / (slope·√(off² + w²))` with `w = L(t − u)`.
    let k = -(b.z - a.z) * slope / length;
    if k.abs() >= 1.0 {
        return at(0.0).min(at(1.0));
    }
    let w = k * off / (1.0 - k * k).sqrt();
    at(((along + w) / length).clamp(0.0, 1.0))
}

/// The lowest an end mill's flat bottom of `radius_mm` reaches over `q`
/// while its centre runs straight from `a` to `b`; infinite when it never
/// covers `q`.
fn disc_low_mm(a: Point3, b: Point3, q: Point, radius_mm: f64) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length = dx.hypot(dy);
    if length < 1e-12 {
        return if q.distance_to(a.xy()) <= radius_mm {
            a.z.min(b.z)
        } else {
            f64::INFINITY
        };
    }
    let (ux, uy) = (dx / length, dy / length);
    let (qx, qy) = (q.x - a.x, q.y - a.y);
    let along = qx * ux + qy * uy;
    let off = (qx * uy - qy * ux).abs();
    if off > radius_mm {
        return f64::INFINITY;
    }
    let half = (radius_mm * radius_mm - off * off).sqrt();
    let t0 = ((along - half) / length).max(0.0);
    let t1 = ((along + half) / length).min(1.0);
    if t0 > t1 {
        return f64::INFINITY;
    }
    let z = |t: f64| a.z + (b.z - a.z) * t;
    z(t0).min(z(t1))
}
