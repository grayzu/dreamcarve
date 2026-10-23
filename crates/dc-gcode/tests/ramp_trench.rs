// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #352: with a lead, a ramp entry and tabs, a pass below the tab top
//! ramped along a line no earlier pass cut, into full-height stock at the
//! cutting feed. Checked on the emitted G-code, independently of the
//! generator: the program is parsed back into project space and every cut
//! is swept through the stock on a 0.1 mm lattice, in program order.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "test code reports failure by panicking; lattice indices of coordinates within the stock"
)]

use dc_cam::generate_profile;
use dc_cam::placed_tabs::place_requests;
use dc_gcode::emit;
use dc_gcode::parse::MotionKind;
use dc_gcode::transform::MachineTransform;
use dc_geometry::{Arc, Point, Point3, Polyline, Vec2};
use dc_project::test_support::sample_project;
use dc_project::{
    CutDirection, Entry, Lead, Operation, Project, Side, Stock, TabPlacement, TabShape, TabSpec,
};

const STOCK_MM: f64 = 12.0;
const STEP_MM: f64 = 4.0;
const RADIUS_MM: f64 = 6.35 / 2.0;
const LATTICE_MM: f64 = 0.1;
/// The pass floors above the stock bottom, the stock top first: 12 mm
/// deep in 4 mm steps.
const FLOORS_MM: [f64; 4] = [STOCK_MM, 8.0, 4.0, 0.0];
/// How much nearer than the radius a lattice point must lie to a sample
/// to be counted under the bit: the lattice step and the arc sampling.
const REACH_MARGIN_MM: f64 = 0.15;
/// How far above a cut's Z a lattice point must stand to be cut by it.
const CUT_MM: f64 = 0.01;

/// The sample project on 100 × 100 × 12 mm stock, its one definition a
/// circle of `diameter_mm` at (50, 50), cut 12 mm deep in 4 mm steps
/// with a 4 mm / 90° lead and `entry`, and `tabs` tabs 6 mm wide and 3 mm
/// tall: requested, or placed from the request when `placed`.
fn project(
    side: Side,
    direction: CutDirection,
    entry: &Entry,
    tabs: u8,
    shape: TabShape,
    placed: bool,
) -> Project {
    let mut project = sample_project();
    project.stock = Stock {
        width_mm: 100.0,
        height_mm: 100.0,
        thickness_mm: Some(STOCK_MM),
    };
    let diameter_mm = match side {
        Side::Outside => 15.0,
        _ => 40.0,
    };
    if let Some(definition) = project.definitions.values_mut().next() {
        definition.profiles = vec![Polyline::circle(Point::new(0.0, 0.0), diameter_mm / 2.0)];
    }
    project.instances[0].transform = dc_geometry::Transform::translate(50.0, 50.0);
    project.instances[0].z_top_mm = Some(STOCK_MM);
    let op = dc_project::test_support::profile_mut(&mut project);
    op.side = side;
    op.direction = direction;
    op.depth_mm = Some(STOCK_MM);
    op.step_down_mm = Some(STEP_MM);
    op.entry = entry.clone();
    op.lead = Some(Lead {
        radius_mm: 4.0,
        sweep_deg: 90.0,
        lead_out: true,
        overcut_mm: 1.0,
    });
    op.tabs = Some(TabSpec {
        placement: TabPlacement::Count { count: tabs },
        width_mm: 6.0,
        height_mm: 3.0,
        shape,
        on_holes: false,
        avoid_corners: false,
    });
    if placed {
        project = place_requests(&project).expect("the tabs place").0;
    }
    project
}

/// The cutting moves of the program `project` exports, in project space,
/// each sampled at most a lattice step apart, Z interpolated. Export runs
/// the safety pass.
fn cuts(project: &Project) -> Vec<Vec<Point3>> {
    let Operation::Profile(op) = &project.operations[0] else {
        unreachable!("a Profile");
    };
    let (toolpath, _) = generate_profile(project, op).unwrap_or_else(|e| panic!("{e}"));
    let (program, _) = emit(
        project,
        &[(
            project.operations[0].clone(),
            project.tools[0].clone(),
            toolpath,
        )
            .into()],
        "2026-10-21",
    )
    .unwrap_or_else(|e| panic!("the safety pass: {e}"));
    let post = project.post_processor.resolve().expect("bundled");
    let transform = MachineTransform::new(project, &post).expect("a valid mapping");
    let mut out = Vec::new();
    for mv in dc_gcode::parse_motion(&program.text).expect("the program parses") {
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
        let steps = (length_mm.max((from.z - to.z).abs()) / LATTICE_MM)
            .ceil()
            .max(1.0) as usize;
        out.push(
            (0..=steps)
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
                .collect(),
        );
    }
    out
}

/// The floor of the pass above Z `z_mm` (the stock top on the first
/// pass), and that pass's step down to the next floor.
fn floor_above(z_mm: f64) -> (f64, f64) {
    FLOORS_MM
        .windows(2)
        .rev()
        .find(|w| w[0] > z_mm + CUT_MM)
        .map_or((STOCK_MM, STEP_MM), |w| (w[0], w[0] - w[1]))
}

/// A sample that cuts stock no move before it cleared to within one step
/// down of the floor above the sample: where, how high that stock stood,
/// and the floor.
#[derive(Debug)]
#[allow(dead_code, reason = "read through the failure's Debug output")]
struct Uncut {
    at: Point3,
    stock_z_mm: f64,
    floor_mm: f64,
}

/// The lattice: the lowest Z cut over each point of the stock, the stock
/// top where nothing is.
struct Heights {
    n: usize,
    z: Vec<f64>,
}

impl Heights {
    fn new() -> Self {
        let n = (100.0 / LATTICE_MM) as usize + 1;
        Self {
            n,
            z: vec![STOCK_MM; n * n],
        }
    }

    /// Calls `f` with each lattice point within `radius_mm` of `p` in XY.
    fn each_within(&mut self, p: Point3, radius_mm: f64, mut f: impl FnMut(&mut f64)) {
        let n = self.n;
        let j0 = ((p.y - radius_mm) / LATTICE_MM).ceil().max(0.0) as usize;
        let j1 = (((p.y + radius_mm) / LATTICE_MM).floor() as usize).min(n - 1);
        for j in j0..=j1 {
            let dy = j as f64 * LATTICE_MM - p.y;
            let r2 = radius_mm * radius_mm - dy * dy;
            if r2 < 0.0 {
                continue;
            }
            let half = r2.sqrt();
            let i0 = ((p.x - half) / LATTICE_MM).ceil().max(0.0) as usize;
            let i1 = (((p.x + half) / LATTICE_MM).floor() as usize).min(n - 1);
            for i in i0..=i1 {
                f(&mut self.z[j * n + i]);
            }
        }
    }
}

/// Sweeps every cut of `cuts` through the stock on the lattice, in order,
/// and returns the first sample that cuts stock which no move before it
/// cleared to within one step down of the floor above the sample. Each
/// move is checked against the moves before it, then swept.
fn first_uncut(cuts: &[Vec<Point3>]) -> Option<Uncut> {
    let mut heights = Heights::new();
    for samples in cuts {
        for p in samples.iter().filter(|p| p.z < STOCK_MM - CUT_MM) {
            let (floor_mm, step_mm) = floor_above(p.z);
            let limit_mm = floor_mm + step_mm - 0.05;
            let mut worst: Option<f64> = None;
            heights.each_within(*p, RADIUS_MM - REACH_MARGIN_MM, |cell| {
                if *cell > p.z + CUT_MM && *cell > limit_mm {
                    worst = Some(worst.map_or(*cell, |w| w.max(*cell)));
                }
            });
            if let Some(stock_z_mm) = worst {
                return Some(Uncut {
                    at: *p,
                    stock_z_mm,
                    floor_mm,
                });
            }
        }
        for p in samples.iter().filter(|p| p.z < STOCK_MM) {
            heights.each_within(*p, RADIUS_MM, |cell| *cell = cell.min(p.z));
        }
    }
    None
}

/// #352: the issue's repro, a 15 mm boss cut Outside (and a 40 mm hole
/// cut Inside), 12 mm deep in 4 mm steps with a 6.35 mm bit, a 4 mm / 90°
/// lead, a 10° ramp or a helix, and 1 to 4 tabs 6 × 3 mm, rect and
/// smooth, requested and placed, climb and conventional. On the emitted
/// G-code, every cut below the stock top runs only through stock the
/// moves before it cleared to within one step down of the floor of the
/// pass above: no pass ramps or helixes into stock the passes above did
/// not cut, apart from its own step down. A helix that starts again from
/// above the stock clears its own bore as it goes. On main, the boss's
/// third pass ramped 19 mm into full-height stock.
#[test]
fn lead_ramp_below_tab_top_stays_in_the_trench() {
    let entries = [
        Entry::Ramp { angle_deg: 10.0 },
        Entry::Helix {
            diameter_fraction: 0.8,
            angle_deg: 10.0,
        },
    ];
    let mut failures = Vec::new();
    for side in [Side::Outside, Side::Inside] {
        for direction in [CutDirection::Climb, CutDirection::Conventional] {
            for entry in &entries {
                for tabs in 1..=4 {
                    for shape in [TabShape::Rect, TabShape::Smooth] {
                        for placed in [false, true] {
                            let case = format!(
                                "{side:?} {direction:?} {entry:?} {tabs} {shape:?} placed {placed}"
                            );
                            let p = project(side, direction, entry, tabs, shape, placed);
                            if let Some(uncut) = first_uncut(&cuts(&p)) {
                                failures.push(format!("{case}: {uncut:?}"));
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
