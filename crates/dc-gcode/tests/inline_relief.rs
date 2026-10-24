// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #373: corner relief is cut inline, on the pass that cuts the final
//! wall (maintainer decision 2026-10-24; `docs/specs/phase-2/design.md`,
//! "Corner relief", step 5). Checked on the emitted G-code: a Pocket and
//! an inside Profile of the sample's 50 × 30 mm rectangle, cut 4 mm deep in
//! the tool's 2 mm steps, export through the safety pass. Every level
//! cuts each corner's notch, with no rapid added. On a heightfield the
//! notches are cut and nothing else differs from the same cut without
//! relief.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

#[path = "support/heightfield.rs"]
mod heightfield;

use dc_cam::{Toolpath, generate_pocket, generate_profile};
use dc_gcode::emit;
use dc_gcode::parse::MotionKind;
use dc_geometry::{Point, Polyline, Vec2};
use dc_project::test_support::{sample_pocket, sample_project};
use dc_project::{CornerRelief, Operation, PocketOp, Project, Side};

const DATE: &str = "2026-10-24";
const RADIUS_MM: f64 = 6.35 / 2.0;
const STEP_DOWN_MM: f64 = 2.0;
const STOCK_MM: f64 = 6.0;
/// The two pass floors, 4 mm deep in 2 mm steps.
const LEVELS_MM: [f64; 2] = [4.0, 2.0];
const LATTICE_MM: f64 = 0.1;

/// The rectangle's corners in project space, each with the unit
/// directions of its two edges from it, the X-parallel (longer) edge
/// first.
fn corners() -> [(Point, Vec2, Vec2); 4] {
    let (x0, y0, x1, y1) = (10.0, 10.0, 60.0, 40.0);
    [
        (Point::new(x0, y0), Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)),
        (
            Point::new(x1, y0),
            Vec2::new(-1.0, 0.0),
            Vec2::new(0.0, 1.0),
        ),
        (
            Point::new(x1, y1),
            Vec2::new(-1.0, 0.0),
            Vec2::new(0.0, -1.0),
        ),
        (
            Point::new(x0, y1),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, -1.0),
        ),
    ]
}

/// Each corner's notch: its descend point, where the wall's path turns
/// the corner, and its end point for `relief`.
fn notches(relief: CornerRelief) -> Vec<(Point, Point)> {
    corners()
        .iter()
        .map(|&(c, u, v)| {
            let descend = c + u * RADIUS_MM + v * RADIUS_MM;
            let end = match relief {
                CornerRelief::TBone => c + u * RADIUS_MM,
                _ => {
                    c + u * (RADIUS_MM / std::f64::consts::SQRT_2)
                        + v * (RADIUS_MM / std::f64::consts::SQRT_2)
                }
            };
            (descend, end)
        })
        .collect()
}

/// The sample project, cut by a 4 mm deep Pocket or inside Profile of its
/// rectangle with `relief`.
fn project(pocket: bool, relief: CornerRelief) -> Project {
    let mut project = sample_project();
    let op = if pocket {
        Operation::Pocket(PocketOp {
            depth_mm: Some(4.0),
            corner_relief: relief,
            ..sample_pocket()
        })
    } else {
        let Operation::Profile(op) = project.operations[0].clone() else {
            unreachable!("the sample starts with a Profile");
        };
        Operation::Profile(dc_project::ProfileOp {
            side: Side::Inside,
            depth_mm: Some(4.0),
            tabs: None,
            corner_relief: relief,
            ..op
        })
    };
    project.operations = vec![op];
    project
}

fn toolpath(project: &Project) -> Toolpath {
    match &project.operations[0] {
        Operation::Pocket(op) => generate_pocket(project, op).unwrap().0,
        Operation::Profile(op) => generate_profile(project, op).unwrap().0,
        _ => unreachable!("a Pocket or a Profile"),
    }
}

/// The program `project` exports; export runs the safety pass, S9
/// included.
fn program(project: &Project) -> String {
    let (program, _) = emit(
        project,
        &[(
            project.operations[0].clone(),
            project.tools[0].clone(),
            toolpath(project),
        )
            .into()],
        DATE,
    )
    .unwrap_or_else(|e| panic!("the safety pass: {e}"));
    program.text
}

fn rapids(text: &str) -> usize {
    dc_gcode::parse_motion(text)
        .expect("the program parses")
        .iter()
        .filter(|m| m.kind == MotionKind::Rapid)
        .count()
}

fn without(project: &Project) -> Project {
    let mut plain = project.clone();
    match &mut plain.operations[0] {
        Operation::Pocket(op) => op.corner_relief = CornerRelief::None,
        Operation::Profile(op) => op.corner_relief = CornerRelief::None,
        _ => {}
    }
    plain
}

/// The cases: a Pocket with dogbones and with T-bones, and an inside
/// Profile with dogbones.
fn cases() -> Vec<(&'static str, Project, CornerRelief)> {
    vec![
        (
            "pocket dogbone",
            project(true, CornerRelief::Dogbone),
            CornerRelief::Dogbone,
        ),
        (
            "pocket t-bone",
            project(true, CornerRelief::TBone),
            CornerRelief::TBone,
        ),
        (
            "profile dogbone",
            project(false, CornerRelief::Dogbone),
            CornerRelief::Dogbone,
        ),
    ]
}

/// Each exports through S9; each level cuts every notch, once, from the
/// wall's path at that level: between its first and last notch there is
/// no rapid and no rise above the level, and the program has no more
/// rapids than the same cut without relief.
#[test]
fn inline_relief_pocket_and_profile_export() {
    for (name, project, relief) in cases() {
        let text = program(&project);
        assert_eq!(
            rapids(&text),
            rapids(&program(&without(&project))),
            "{name}: relief adds no lift"
        );
        let motion = dc_gcode::parse_motion(&text).expect("the program parses");
        let cuts = heightfield::cuts(&project, &text, LATTICE_MM);
        for z in LEVELS_MM {
            // The index of each notch's outward cut at `z` among the cuts.
            let at: Vec<usize> = notches(relief)
                .iter()
                .map(|&(_, end)| {
                    let hits: Vec<usize> = cuts
                        .iter()
                        .enumerate()
                        .filter(|(_, c)| {
                            c.to.xy().distance_to(end) < 0.01 && (c.to.z - z).abs() < 0.01
                        })
                        .map(|(k, _)| k)
                        .collect();
                    assert_eq!(hits.len(), 1, "{name}: notch to {end:?} at Z {z}: {hits:?}");
                    hits[0]
                })
                .collect();
            let (first, last) = (*at.iter().min().unwrap(), *at.iter().max().unwrap());
            for c in &cuts[first..=last] {
                assert!(
                    c.to.z <= z + 0.01,
                    "{name}: a rise between notches at Z {z}"
                );
            }
            // The cuts between them follow each other with no rapid: each
            // starts where the one before ended.
            for pair in cuts[first..=last].windows(2) {
                assert!(
                    pair[0].to.xy().distance_to(pair[1].from.xy()) < 0.01
                        && (pair[0].to.z - pair[1].from.z).abs() < 0.01,
                    "{name}: a rapid between notches at Z {z}"
                );
            }
        }
        assert!(!motion.is_empty());
    }
}

/// The distance from `p` to the segment `a`–`b`.
fn to_segment(p: Point, a: Point, b: Point) -> f64 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.dot(ab)).clamp(0.0, 1.0);
    p.distance_to(a + ab * t)
}

/// On a heightfield of each program, every point the notches sweep is cut
/// to the final depth, every point well clear of them stands as high as
/// without relief, and no straight plunge goes deeper than the tool's
/// step-down into uncut material.
#[test]
fn inline_relief_heightfield_matches_the_separate_pass() {
    let floor_mm = LEVELS_MM[1];
    let margin_mm = 2.0 * LATTICE_MM;
    for (name, project, relief) in cases() {
        let swept = |text: &str| {
            let cuts = heightfield::cuts(&project, text, LATTICE_MM);
            // One lattice over the rectangle for both programs.
            let outline = Polyline::rect(Point::new(10.0, 10.0), 50.0, 30.0);
            let mut heights = heightfield::Heights::with(&outline, LATTICE_MM, 1.0, STOCK_MM);
            for cut in &cuts {
                heights.sweep(cut, RADIUS_MM);
            }
            (cuts, heights)
        };
        let text = program(&project);
        let (cuts, with) = swept(&text);
        let (_, plain) = swept(&program(&without(&project)));
        let (deepest_mm, at) =
            heightfield::deepest_plunge_mm(&cuts, RADIUS_MM, LATTICE_MM, STOCK_MM);
        assert!(
            deepest_mm <= STEP_DOWN_MM + 0.01,
            "{name}: cut {at:?} plunges {deepest_mm} mm into uncut material"
        );
        let notches = notches(relief);
        for j in 0..with.ny {
            for i in 0..with.nx {
                let p = with.point(i, j);
                let near = notches
                    .iter()
                    .map(|&(d, e)| to_segment(p, d, e))
                    .fold(f64::INFINITY, f64::min);
                let h = with.at(i, j);
                if near < RADIUS_MM - margin_mm {
                    assert!(
                        h <= floor_mm + 0.01,
                        "{name}: {p:?} in a notch stands at {h}"
                    );
                } else if near > RADIUS_MM + margin_mm {
                    let was = plain.at(i, j);
                    assert!(
                        (h - was).abs() <= 0.01,
                        "{name}: {p:?} is at {h}, {was} without relief"
                    );
                }
            }
        }
    }
}
