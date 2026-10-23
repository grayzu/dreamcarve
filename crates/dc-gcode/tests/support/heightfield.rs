// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A material heightfield of an emitted program, shared by the G-code
//! checks (#339, #348): the program is parsed back into project space and
//! its cuts are swept, in order, through the stock on a square lattice;
//! before each straight plunge the lattice says how much uncut material
//! lies under the cutter.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    dead_code,
    reason = "test code reports failure by panicking; lattice indices of coordinates within the stock; each test binary uses part of it"
)]

use dc_gcode::CycleModel;
use dc_gcode::parse::MotionKind;
use dc_gcode::transform::MachineTransform;
use dc_geometry::{Arc, Point, Point3, Polyline, Vec2};
use dc_project::Project;

/// One cutting move in project space, sampled at most a lattice step
/// apart, Z interpolated, with its feed.
pub struct Cut {
    pub from: Point3,
    pub to: Point3,
    pub feed: f64,
    /// An arc's radius, mm, as written.
    pub radius_mm: Option<f64>,
    /// An arc's centre, project space.
    pub centre: Option<Point>,
    pub samples: Vec<Point3>,
}

impl Cut {
    /// Straight down: Z falls and XY stays.
    pub fn is_plunge(&self) -> bool {
        self.to.z < self.from.z - 1e-9 && self.from.xy().distance_to(self.to.xy()) <= 1e-6
    }
}

/// The cutting moves of `text`, the program `project` exported, in
/// project space and in order, each sampled at most `step_mm` apart.
pub fn cuts(project: &Project, text: &str, step_mm: f64) -> Vec<Cut> {
    let post = project.post_processor.resolve().expect("bundled");
    let transform = MachineTransform::new(project, &post).expect("a valid mapping");
    // A controller with no units words (UCCNC) reads the program in the
    // units it is set up in: the program's, as the safety pass reads it.
    let model = CycleModel {
        units: transform.output_units(),
        ..CycleModel::of(&post)
    };
    let mut cuts = Vec::new();
    for mv in dc_gcode::parse_motion_with(text, &model).expect("the program parses") {
        let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
            (mv.from.x, mv.from.y, mv.from.z, mv.to.x, mv.to.y, mv.to.z)
        else {
            continue;
        };
        if !mv.kind.is_cut() {
            continue;
        }
        let from = transform.to_project(Point3::new(fx, fy, fz), mv.units);
        let to = transform.to_project(Point3::new(tx, ty, tz), mv.units);
        let arc = match (mv.kind, mv.arc.i, mv.arc.j) {
            (MotionKind::ArcCw | MotionKind::ArcCcw, Some(i), Some(j)) => {
                let centre = transform
                    .to_project(Point3::new(fx + i, fy + j, fz), mv.units)
                    .xy();
                Some(Arc::from_ends(
                    from.xy(),
                    to.xy(),
                    centre,
                    mv.kind == MotionKind::ArcCcw,
                ))
            }
            _ => None,
        };
        let length_mm = arc.as_ref().map_or_else(
            || from.xy().distance_to(to.xy()),
            |a| a.radius_mm * a.sweep_rad.abs(),
        );
        let steps = (length_mm / step_mm).ceil().max(1.0) as usize;
        let samples = (0..=steps)
            .map(|k| {
                let f = k as f64 / steps as f64;
                let xy = match &arc {
                    Some(a) => {
                        let angle = a.start_angle_rad + a.sweep_rad * f;
                        a.centre + Vec2::new(angle.cos(), angle.sin()) * a.radius_mm
                    }
                    None => from.xy() + (to.xy() - from.xy()) * f,
                };
                Point3::new(xy.x, xy.y, from.z + (to.z - from.z) * f)
            })
            .collect();
        cuts.push(Cut {
            from,
            to,
            feed: mv.feed.expect("a cut has a feed"),
            radius_mm: arc.as_ref().map(|a| a.radius_mm),
            centre: arc.as_ref().map(|a| a.centre),
            samples,
        });
    }
    cuts
}

/// A heightfield over a neighbourhood: the lowest Z cut over each lattice
/// point, the stock top where nothing is.
#[derive(Clone)]
pub struct Heights {
    pub origin: Point,
    pub nx: usize,
    pub ny: usize,
    /// The lattice step, mm.
    pub step: f64,
    /// The stock top, mm.
    pub top_mm: f64,
    pub z: Vec<f64>,
}

impl Heights {
    /// Over the drawing `outline` and `margin_mm` round it, on a lattice
    /// of `step_mm`, under stock `top_mm` thick.
    pub fn with(outline: &Polyline, step_mm: f64, margin_mm: f64, top_mm: f64) -> Self {
        let (mut lo, mut hi) = (
            Point::new(f64::MAX, f64::MAX),
            Point::new(f64::MIN, f64::MIN),
        );
        for v in outline.vertices() {
            lo = Point::new(lo.x.min(v.x), lo.y.min(v.y));
            hi = Point::new(hi.x.max(v.x), hi.y.max(v.y));
        }
        // A circle's two vertices span only one axis; pad by the span.
        let pad = (hi.x - lo.x).max(hi.y - lo.y) / 2.0 + margin_mm;
        Self::over(lo, hi, pad, step_mm, top_mm)
    }

    /// Over every sample of `cuts` and `radius_mm` round them.
    pub fn round(cuts: &[Cut], radius_mm: f64, step_mm: f64, top_mm: f64) -> Self {
        let (mut lo, mut hi) = (
            Point::new(f64::MAX, f64::MAX),
            Point::new(f64::MIN, f64::MIN),
        );
        for p in cuts.iter().flat_map(|c| &c.samples) {
            lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
            hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
        }
        Self::over(lo, hi, radius_mm + step_mm, step_mm, top_mm)
    }

    fn over(lo: Point, hi: Point, pad: f64, step_mm: f64, top_mm: f64) -> Self {
        let origin = Point::new(lo.x - pad, lo.y - pad);
        let nx = ((hi.x - lo.x + 2.0 * pad) / step_mm).ceil() as usize + 1;
        let ny = ((hi.y - lo.y + 2.0 * pad) / step_mm).ceil() as usize + 1;
        Self {
            origin,
            nx,
            ny,
            step: step_mm,
            top_mm,
            z: vec![top_mm; nx * ny],
        }
    }

    pub fn point(&self, i: usize, j: usize) -> Point {
        Point::new(
            self.origin.x + i as f64 * self.step,
            self.origin.y + j as f64 * self.step,
        )
    }

    pub fn at(&self, i: usize, j: usize) -> f64 {
        self.z[j * self.nx + i]
    }

    /// The index of every lattice point within `radius_mm` of `p`.
    pub fn disc(&self, p: Point, radius_mm: f64) -> Vec<usize> {
        let mut out = Vec::new();
        let j0 = ((p.y - radius_mm - self.origin.y) / self.step)
            .ceil()
            .max(0.0) as usize;
        let j1 =
            (((p.y + radius_mm - self.origin.y) / self.step).floor() as usize).min(self.ny - 1);
        for j in j0..=j1 {
            let dy = self.origin.y + j as f64 * self.step - p.y;
            let reach = radius_mm * radius_mm - dy * dy;
            if reach < 0.0 {
                continue;
            }
            let half = reach.sqrt();
            let i0 = ((p.x - half - self.origin.x) / self.step).ceil().max(0.0) as usize;
            let i1 = (((p.x + half - self.origin.x) / self.step).floor() as usize).min(self.nx - 1);
            out.extend((i0..=i1).map(|i| j * self.nx + i));
        }
        out
    }

    /// The highest material within `radius_mm` of `p`.
    pub fn highest(&self, p: Point, radius_mm: f64) -> f64 {
        self.disc(p, radius_mm)
            .into_iter()
            .map(|k| self.z[k])
            .fold(f64::MIN, f64::max)
    }

    /// Lowers every lattice point within `radius_mm` of each sample of
    /// `cut` to the sample's Z.
    pub fn sweep(&mut self, cut: &Cut, radius_mm: f64) {
        for p in &cut.samples {
            if p.z >= self.top_mm {
                continue;
            }
            for k in self.disc(p.xy(), radius_mm) {
                let cell = &mut self.z[k];
                *cell = cell.min(p.z);
            }
        }
    }

    /// How deep a straight plunge `cut` goes into uncut material, mm:
    /// from the highest material under the cutter, or the plunge's start
    /// if lower, to its end. 0 for any other move.
    pub fn plunge_depth_mm(&self, cut: &Cut, radius_mm: f64) -> f64 {
        if !cut.is_plunge() {
            return 0.0;
        }
        let top = self.highest(cut.from.xy(), radius_mm).min(cut.from.z);
        (top - cut.to.z).max(0.0)
    }
}

/// The deepest straight plunge of `cuts` into uncut material, mm, and the
/// index of the cut, sweeping them in order through stock `top_mm` thick
/// on a lattice of `step_mm` with a cutter of `radius_mm`.
pub fn deepest_plunge_mm(
    cuts: &[Cut],
    radius_mm: f64,
    step_mm: f64,
    top_mm: f64,
) -> (f64, Option<usize>) {
    let mut heights = Heights::round(cuts, radius_mm, step_mm, top_mm);
    let mut deepest: (f64, Option<usize>) = (0.0, None);
    for (k, cut) in cuts.iter().enumerate() {
        let depth_mm = heights.plunge_depth_mm(cut, radius_mm);
        if depth_mm > deepest.0 {
            deepest = (depth_mm, Some(k));
        }
        heights.sweep(cut, radius_mm);
    }
    deepest
}
