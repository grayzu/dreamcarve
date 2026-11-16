// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The quick-start plaque's screw holes come out the size they are drawn
//! (maintainer decision 2026-10-24): its "Screw holes" Bore (#387),
//! exported from the committed `tests/fixtures/plaque.dcv`, is parsed back
//! into project space and the whole program is swept through the stock on
//! a 0.05 mm lattice round each hole. Every drawn 5 mm circle is cut
//! round, through to the operation's depth, and nothing outside it is
//! touched.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

#[path = "support/heightfield.rs"]
mod heightfield;

use dc_cam::generate_bore;
use dc_cam::test_support::assert_safe;
use dc_gcode::{EmitOp, emit};
use dc_project::{BoreOp, Operation, Project};
use heightfield::Heights;

const LATTICE_MM: f64 = 0.05;
/// How far from the drawn circle a lattice point must lie to be judged:
/// two lattice steps, for the lattice and the arc sampling.
const EDGE_MM: f64 = 0.1;
const HOLE_DIAMETER_MM: f64 = 5.0;
const TOOL_DIAMETER_MM: f64 = 3.175;
/// The neighbourhood of each hole the heightfield covers, beyond the
/// circle, mm.
const MARGIN_MM: f64 = 3.0;

fn plaque() -> Project {
    dc_project::load(include_bytes!("../../../tests/fixtures/plaque.dcv"))
        .expect("the plaque loads")
        .project
}

/// The plaque's "Screw holes" Bore.
fn screw_holes(project: &Project) -> BoreOp {
    project
        .operations
        .iter()
        .find_map(|o| match o {
            Operation::Bore(b) if b.name == "Screw holes" => Some(b.clone()),
            _ => None,
        })
        .expect("the plaque's Screw holes are a Bore")
}

/// The program `op` exports on its own, through the safety pass, after
/// its toolpath passes the shared safety assertions.
fn program(project: &Project, op: &BoreOp) -> String {
    let (toolpath, _) = generate_bore(project, op).unwrap_or_else(|e| panic!("{e}"));
    let operation = Operation::Bore(op.clone());
    assert_safe(&toolpath, project, &operation);
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .expect("its tool is in the project")
        .clone();
    let emit_op: EmitOp = (operation, tool, toolpath).into();
    emit(project, &[emit_op], "2026-10-24")
        .unwrap_or_else(|e| panic!("the safety pass: {e}"))
        .0
        .text
}

/// Each of the four screw holes, swept from the whole program on a
/// heightfield of its own neighbourhood, is 5 mm across: every lattice
/// point more than [`EDGE_MM`] inside the drawn circle is cut to the
/// Bore's floor (the 11.8 mm stock top less its 12 mm depth, 0.2 mm below
/// the stock bottom), every one more than [`EDGE_MM`] outside it keeps the
/// stock top, and the cut reaches to within [`EDGE_MM`] of the circle all
/// round: the nearest uncut point and the farthest cut one are each
/// within [`EDGE_MM`] of the 2.5 mm radius.
#[test]
fn plaque_screw_holes_are_round_and_through_on_the_heightfield() {
    let project = plaque();
    let op = screw_holes(&project);
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .expect("its tool is in the project");
    assert!(
        (tool.diameter_mm - TOOL_DIAMETER_MM).abs() < 1e-12,
        "the 3.175 mm bit, not {} mm",
        tool.diameter_mm
    );
    let radius_mm = tool.diameter_mm / 2.0;
    let top_mm = project
        .stock
        .thickness_mm
        .expect("the plaque's thickness is set");
    let depth_mm = op.depth_mm.expect("the Bore's depth is set");
    let floor_mm = top_mm - depth_mm;
    assert!(
        floor_mm < 0.0 && -floor_mm <= op.through_cut_allowance_mm + 1e-9,
        "the holes go through, to Z {floor_mm}"
    );

    let text = program(&project, &op);
    let cuts = heightfield::cuts(&project, &text, LATTICE_MM);
    assert!(!cuts.is_empty(), "the operation cuts");

    assert_eq!(op.targets.len(), 4, "four screw holes");
    for target in &op.targets {
        let instance = project
            .instances
            .iter()
            .find(|i| &i.id == target)
            .expect("the target exists");
        let circle = instance
            .transform
            .apply_polyline(&project.definitions[&instance.definition].profiles[0])
            .expect("the circle transforms");
        let (centre, drawn_radius_mm) =
            dc_geometry::as_circle(&circle, project.flatten_tolerance_mm).expect("a circle");
        assert!(
            (2.0 * drawn_radius_mm - HOLE_DIAMETER_MM).abs() < 1e-6,
            "{target}: drawn {} mm",
            2.0 * drawn_radius_mm
        );

        // Every cut of the program, the other holes' included: those lie
        // wholly outside this neighbourhood and must touch none of it.
        let mut heights = Heights::with(&circle, LATTICE_MM, MARGIN_MM, top_mm);
        for cut in &cuts {
            heights.sweep(cut, radius_mm);
        }
        let hole_radius_mm = HOLE_DIAMETER_MM / 2.0;
        let mut farthest_cut_mm: f64 = 0.0;
        let mut nearest_uncut_mm = f64::INFINITY;
        for j in 0..heights.ny {
            for i in 0..heights.nx {
                let p = heights.point(i, j);
                let r = p.distance_to(centre);
                let z = heights.at(i, j);
                if z < top_mm {
                    farthest_cut_mm = farthest_cut_mm.max(r);
                } else {
                    nearest_uncut_mm = nearest_uncut_mm.min(r);
                }
                if r <= hole_radius_mm - EDGE_MM {
                    assert!(
                        z <= floor_mm + 1e-6,
                        "{target}: {p:?}, {r:.3} mm from the centre, is at Z {z:.3}, not through to {floor_mm}"
                    );
                }
                if r >= hole_radius_mm + EDGE_MM {
                    assert!(
                        z >= top_mm,
                        "{target}: {p:?}, {r:.3} mm from the centre, is cut to Z {z:.3}: the hole is oversize"
                    );
                }
            }
        }
        println!(
            "{target}: cut {:.3} mm across, uncut beyond {:.3} mm across",
            2.0 * farthest_cut_mm,
            2.0 * nearest_uncut_mm
        );
        assert!(
            (farthest_cut_mm - hole_radius_mm).abs() <= EDGE_MM,
            "{target}: the cut reaches {:.3} mm across",
            2.0 * farthest_cut_mm
        );
        assert!(
            (nearest_uncut_mm - hole_radius_mm).abs() <= EDGE_MM,
            "{target}: material stands {:.3} mm across",
            2.0 * nearest_uncut_mm
        );
    }
}
