// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Photo V-Carve comparison against RasterCarve
//! (`docs/specs/phase-3/design.md`, "Golden corpus → RasterCarve cases";
//! `docs/specs/phase-3/functional.md`, acceptance 60; #647).
//!
//! `tests/golden/<case>/program.nc` is what RasterCarve's command line
//! wrote on the case's fixture picture (`tools/parity/photo/capture.py`);
//! `dreamcarve.dcv` is the DreamCarve project carving the same picture
//! with the same bit, depth, angle, spacing and sample step. [`check`]
//! reads both programs' **grooves** and compares them groove for groove,
//! in order:
//!
//! - **Frame.** RasterCarve's origin is the picture's top-left corner,
//!   Y-up, so the picture lies at `Y ≤ 0`, and its Z is zero at the stock
//!   top. With `k` its unit in mm (25.4 for an inch program), a golden
//!   point maps to DreamCarve's project frame as `x = x_mm + k · x_rc`,
//!   `y = y_mm + height_mm + k · y_rc`, `z = thickness_mm + k · z_rc`
//!   (each case's README; the spec's `− y_rc` was the wrong sign, #647).
//! - **Grooves.** A groove is a run of moves at or below the stock top:
//!   the plunge's end and the cuts after it, up to the lift. Rapids, the
//!   lifts and links between grooves (RasterCarve lifts to its safe Z,
//!   DreamCarve to its link height) and RasterCarve's final traverse are
//!   not compared. Both programs cut the same number of grooves.
//! - **Samples.** Every golden sample lies within [`XY_MM`] in XY of
//!   DreamCarve's groove, and its Z within [`Z_MM`] of DreamCarve's Z
//!   there; every DreamCarve point likewise of the golden groove; and the
//!   grooves start and end within [`XY_MM`] of each other, so they run
//!   the same way over the same length. DreamCarve merges collinear
//!   samples (a straight run of the ramp is one move), so samples are
//!   compared against the other program's polyline, not index by index.

use dc_cam::{MoveKind, generate_photo};
use dc_geometry::{Point, Point3};
use dc_project::{Operation, Project};

/// How far a sample may be from the other program's groove in XY, mm
/// (design spec, "Golden corpus → RasterCarve cases").
pub const XY_MM: f64 = 0.01;

/// How far a sample's Z may be from the other program's at the same XY, mm
/// (design spec): RasterCarve's 256-level depth quantisation
/// (`max_depth / 256`, 0.008 mm here) plus the two programs' different
/// resampling of the picture.
pub const Z_MM: f64 = 0.05;

/// A point is in a groove when it is at most this far above the stock top,
/// mm: a white sample is at the stock top exactly.
const AT_TOP_MM: f64 = 1e-6;

/// What a case's README records of RasterCarve's own behaviour, which the
/// comparison allows for in that case alone (#647).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Difference {
    /// RasterCarve stops one line short of the picture's far corner: its
    /// statistics count the line ("Number of lines: 111") but its program
    /// does not cut it (110 grooves). DreamCarve cuts every line that
    /// meets the footprint (design spec, "Photo V-Carve", step 2), so it
    /// cuts one more groove, its last, one spacing past RasterCarve's last.
    /// That groove is DreamCarve's alone and is checked only for where it
    /// lies; the others are compared as usual.
    LastLineUncut,
    /// RasterCarve resamples the picture to twice its output resolution
    /// ("Scaled image by f=0.31 (200.0 PPI)") and reads a sample's depth
    /// from that grid, so its depth along a groove steps by whole resampled
    /// pixels. On a picture it shrinks (64 pixels into 20), a resampled
    /// pixel of `pixel_mm` holds several of the picture's, and its depth
    /// lags DreamCarve's bilinear depth by up to one resampled pixel's
    /// worth of the picture's slope. A sample passes the Z check when its
    /// Z is within [`Z_MM`] of the other program's Z anywhere on the
    /// other groove within `pixel_mm` of it in XY.
    Resampled {
        /// One resampled pixel, mm: `1 / 200` in here.
        pixel_mm: f64,
    },
}

/// Compares DreamCarve's Photo V-Carve on case `name` with RasterCarve's
/// program, whose units are `unit_mm` millimetres (25.4 for inches),
/// allowing for `differences` alone.
pub fn check(name: &str, unit_mm: f64, differences: &[Difference]) {
    let project = crate::harness::load(name);
    let golden_text = std::fs::read_to_string(crate::harness::golden_dir(name).join("program.nc"))
        .unwrap_or_else(|e| panic!("{name}: cannot read the golden program: {e}"));
    let mut ours = dreamcarve_grooves(name, &project);
    let golden = golden_grooves(name, &project, &golden_text, unit_mm);

    if differences.contains(&Difference::LastLineUncut) {
        assert_eq!(
            ours.len(),
            golden.len() + 1,
            "{name}: DreamCarve cuts the one line RasterCarve leaves uncut"
        );
        let extra = ours.pop().expect("DreamCarve's last groove");
        check_last_line(name, &project, &ours, &extra);
    }
    assert_eq!(
        ours.len(),
        golden.len(),
        "{name}: DreamCarve cuts {} grooves, RasterCarve {}",
        ours.len(),
        golden.len()
    );
    let window_mm = differences
        .iter()
        .find_map(|d| match d {
            Difference::Resampled { pixel_mm } => Some(*pixel_mm),
            Difference::LastLineUncut => None,
        })
        .unwrap_or(0.0);
    let picture = Picture::of(name, &project);
    let mut worst = Worst::default();
    for (index, (dc, rc)) in ours.iter().zip(&golden).enumerate() {
        let what = |end: &str| format!("{name}: groove {index}'s {end}");
        let (first_dc, first_rc) = (dc[0], rc[0]);
        let (last_dc, last_rc) = (dc[dc.len() - 1], rc[rc.len() - 1]);
        for (end, a, b) in [("start", first_dc, first_rc), ("end", last_dc, last_rc)] {
            let gap = xy_distance(a, b);
            assert!(
                gap <= XY_MM,
                "{}: DreamCarve {a:?}, RasterCarve {b:?}, {gap:.4} mm apart in XY",
                what(end)
            );
        }
        // Both sample every step from the groove's start (design spec,
        // "Photo V-Carve", step 3): each DreamCarve point that merging
        // kept is one of RasterCarve's samples.
        for &p in dc {
            let gap = rc
                .iter()
                .map(|&q| xy_distance(p, q))
                .fold(f64::INFINITY, f64::min);
            assert!(
                gap <= XY_MM,
                "{name}: groove {index}: DreamCarve's sample {p:?} is {gap:.4} mm from \
                 RasterCarve's nearest sample (allowed {XY_MM})"
            );
        }
        for (from, to, golden_is_from) in [(rc, dc, true), (dc, rc, false)] {
            let label = if golden_is_from {
                "RasterCarve"
            } else {
                "DreamCarve"
            };
            for &p in from {
                let (xy_mm, z_at) = nearest_on(to, p);
                let dz = (p.z - z_at).abs();
                worst.note(xy_mm, dz);
                // RasterCarve's Z here: its sample, or its cut at the
                // DreamCarve sample's place.
                let golden_z = if golden_is_from { p.z } else { z_at };
                let within = dz <= Z_MM || picture.depth_near(p, golden_z, window_mm);
                assert!(
                    xy_mm <= XY_MM && within,
                    "{name}: groove {index}: {label}'s sample {p:?} is {xy_mm:.4} mm in XY \
                     from the other program's groove and {dz:.4} mm from its Z there \
                     (allowed {XY_MM} and {Z_MM}, or the picture's Z within {window_mm} mm)"
                );
            }
        }
    }
    eprintln!(
        "{name}: {} grooves; worst XY {:.4} mm, worst Z {:.4} mm",
        ours.len(),
        worst.xy_mm,
        worst.z_mm
    );
}

/// [`Difference::LastLineUncut`]: DreamCarve's `extra` groove, after
/// `ours`, is the next line of the family, one spacing past its last
/// compared groove, and inside the picture's footprint.
fn check_last_line(name: &str, project: &Project, ours: &[Vec<Point3>], extra: &[Point3]) {
    let Some(Operation::PhotoVCarve(op)) = project.operations.first() else {
        panic!("{name}: a Photo V-Carve");
    };
    let slope = (op_angle_deg(project, name) / 2.0).to_radians().tan();
    let spacing_mm =
        op.line_spacing_fraction * 2.0 * op.max_depth_mm.expect("a maximum depth") * slope;
    let previous = ours.last().expect("a groove before DreamCarve's last");
    let (a, b) = (previous[0], previous[previous.len() - 1]);
    let length = xy_distance(a, b);
    let gap_mm = ((b.x - a.x) * (extra[0].y - a.y) - (b.y - a.y) * (extra[0].x - a.x)) / length;
    assert!(
        (gap_mm.abs() - spacing_mm).abs() <= 1e-6,
        "{name}: DreamCarve's last groove is {gap_mm} mm from its previous, not one spacing \
         ({spacing_mm} mm)"
    );
    let bitmap = project.bitmaps.first().expect("the picture");
    for p in extra {
        assert!(
            p.x >= bitmap.x_mm - 1e-9
                && p.x <= bitmap.x_mm + bitmap.width_mm + 1e-9
                && p.y >= bitmap.y_mm - 1e-9
                && p.y <= bitmap.y_mm + bitmap.height_mm + 1e-9,
            "{name}: DreamCarve's last groove stays in the picture: {p:?}"
        );
    }
}

/// The included angle of case `name`'s V-bit, degrees.
fn op_angle_deg(project: &Project, name: &str) -> f64 {
    match project.tools.first().map(|t| &t.cutter) {
        Some(dc_project::ToolKind::VBit { included_angle_deg }) => *included_angle_deg,
        other => panic!("{name}: a V-bit, not {other:?}"),
    }
}

/// The case's picture as the depth it asks for: the Z a sample of it has
/// at a point, `top − (1 − g/255) · max_depth`, with DreamCarve's bilinear
/// [`dc_cam::photo::sample`].
struct Picture {
    bitmap: dc_project::Bitmap,
    pixels: dc_project::Grayscale,
    top_mm: f64,
    max_depth_mm: f64,
}

impl Picture {
    fn of(name: &str, project: &Project) -> Self {
        let Some(Operation::PhotoVCarve(op)) = project.operations.first() else {
            panic!("{name}: a Photo V-Carve");
        };
        let bitmap = project.bitmaps.first().expect("the picture").clone();
        let pixels = bitmap.pixels().expect("the picture decodes");
        Self {
            bitmap,
            pixels,
            top_mm: top_mm(name, project),
            max_depth_mm: op.max_depth_mm.expect("a maximum depth"),
        }
    }

    /// The picture's Z at `(x, y)`.
    fn z_at(&self, x: f64, y: f64) -> f64 {
        let g = dc_cam::photo::sample(&self.bitmap, &self.pixels, Point::new(x, y));
        self.top_mm - (1.0 - g / 255.0) * self.max_depth_mm
    }

    /// [`Difference::Resampled`]: whether RasterCarve's `golden_z` at `p`
    /// is within [`Z_MM`] of the picture's Z somewhere within `window_mm`
    /// of `p` in XY (a 41 × 41 grid over the window's square, the points
    /// inside its disc). Never, with no window.
    fn depth_near(&self, p: Point3, golden_z: f64, window_mm: f64) -> bool {
        if window_mm <= 0.0 {
            return false;
        }
        let steps = 20_i32;
        (-steps..=steps).any(|i| {
            (-steps..=steps).any(|j| {
                let (dx, dy) = (
                    window_mm * f64::from(i) / f64::from(steps),
                    window_mm * f64::from(j) / f64::from(steps),
                );
                dx.hypot(dy) <= window_mm
                    && (self.z_at(p.x + dx, p.y + dy) - golden_z).abs() <= Z_MM
            })
        })
    }
}

/// The largest differences seen, for the test's log.
#[derive(Default)]
struct Worst {
    xy_mm: f64,
    z_mm: f64,
}

impl Worst {
    fn note(&mut self, xy_mm: f64, z_mm: f64) {
        self.xy_mm = self.xy_mm.max(xy_mm);
        self.z_mm = self.z_mm.max(z_mm);
    }
}

/// The stock top, mm.
fn top_mm(name: &str, project: &Project) -> f64 {
    project
        .stock
        .thickness_mm
        .unwrap_or_else(|| panic!("{name}: the stock has a thickness"))
}

/// DreamCarve's grooves on case `name`, project frame.
fn dreamcarve_grooves(name: &str, project: &Project) -> Vec<Vec<Point3>> {
    let mut ops = project.operations.iter().filter(|op| op.enabled());
    let Some(Operation::PhotoVCarve(op)) = ops.next() else {
        panic!("{name}: the project's one operation is a Photo V-Carve");
    };
    assert!(ops.next().is_none(), "{name}: one operation");
    let (toolpath, warnings) =
        generate_photo(project, op).unwrap_or_else(|e| panic!("{name}: generates: {e}"));
    assert!(warnings.is_empty(), "{name}: no warnings: {warnings:?}");
    let top = top_mm(name, project);
    let mut points = Vec::new();
    for m in &toolpath.moves {
        match m.kind {
            MoveKind::Linear | MoveKind::Rapid => points.push(m.to),
            MoveKind::ArcCw { .. } | MoveKind::ArcCcw { .. } | MoveKind::Dwell { .. } => {
                panic!("{name}: a Photo V-Carve moves in straight lines: {m:?}")
            }
        }
        if m.kind == MoveKind::Rapid {
            assert!(
                m.to.z > top + AT_TOP_MM,
                "{name}: a rapid stays above the stock: {m:?}"
            );
        }
    }
    grooves(&points, top)
}

/// RasterCarve's grooves in `text`, its units `unit_mm`, mapped to the
/// project frame of the picture of `project`.
fn golden_grooves(name: &str, project: &Project, text: &str, unit_mm: f64) -> Vec<Vec<Point3>> {
    let bitmap = project
        .bitmaps
        .first()
        .unwrap_or_else(|| panic!("{name}: the project has its picture"));
    let top = top_mm(name, project);
    let to_project = |x: f64, y: f64, z: f64| {
        Point3::new(
            bitmap.x_mm + unit_mm * x,
            bitmap.y_mm + bitmap.height_mm + unit_mm * y,
            top + unit_mm * z,
        )
    };
    let (mut x, mut y, mut z) = (0.0, 0.0, 0.0);
    let mut points = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('(') {
            continue;
        }
        // A motion word with an axis word: `N1 G00 G20` moves nowhere.
        let (mut motion, mut axis) = (false, false);
        for word in line.split_whitespace() {
            let (letter, value) = word.split_at(1);
            let number = || {
                value
                    .parse::<f64>()
                    .unwrap_or_else(|e| panic!("{name}: `{word}` in `{line}`: {e}"))
            };
            match letter {
                "G" => motion |= matches!(value, "0" | "00" | "1" | "01"),
                "X" => (x, axis) = (number(), true),
                "Y" => (y, axis) = (number(), true),
                "Z" => (z, axis) = (number(), true),
                _ => {}
            }
        }
        if motion && axis {
            points.push(to_project(x, y, z));
        }
    }
    grooves(&points, top)
}

/// The grooves of a run of move end points: each maximal run of points at
/// or below the stock `top`.
fn grooves(points: &[Point3], top: f64) -> Vec<Vec<Point3>> {
    let mut out: Vec<Vec<Point3>> = Vec::new();
    let mut open = false;
    for &p in points {
        if p.z <= top + AT_TOP_MM {
            if !open {
                out.push(Vec::new());
                open = true;
            }
            if let Some(groove) = out.last_mut()
                && groove.last() != Some(&p)
            {
                groove.push(p);
            }
        } else {
            open = false;
        }
    }
    out
}

/// The XY distance between `a` and `b`, mm.
fn xy_distance(a: Point3, b: Point3) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// The XY distance from `p` to the polyline `path`, and the polyline's Z
/// at its nearest point.
fn nearest_on(path: &[Point3], p: Point3) -> (f64, f64) {
    let mut best = (xy_distance(path[0], p), path[0].z);
    for pair in path.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let length2 = dx * dx + dy * dy;
        let t = if length2 > 0.0 {
            (((p.x - a.x) * dx + (p.y - a.y) * dy) / length2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let on = Point3::new(a.x + t * dx, a.y + t * dy, a.z + t * (b.z - a.z));
        let d = xy_distance(on, p);
        if d < best.0 {
            best = (d, on.z);
        }
    }
    best
}
