// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S17 and S9 against a drawn band (#604, slice 21k;
//! `docs/specs/phase-3/design.md`, "Drawn boundary", "Safety pass";
//! acceptance 61r).
//!
//! The safety pass derives the male's band `B` from the project's
//! boundary instances with `dc_project::band_edge_of`, never from the
//! generator and never from a number stored with the program. The
//! hand-written programs cut the two shapes' male of the Inlay tests (the
//! rectangle X 10 to 40 and the square X 70 to 76, Y 20 to 30 and 20 to 26,
//! mirrored about X 43 to X 46 to 76 and X 10 to 16) with the 90° bit
//! `t2` (slope 1) and the end mill's pass `t3`, `P` 1, so the floor is at
//! machine Z −2.5. A drawn rectangle from X 4 to 82 is its own mirror
//! image about X 43, and from Y 14 to 36 it is 6 mm clear of the shapes
//! all round: `B`'s lower edge is the line Y 14.

use super::inlay::{
    assert_cone_off, assert_off, check_male, check_run, cut_out, edge_pass, export_inlay,
    four_tabs, inlay, program_at, small_star, two_shapes,
};
use super::*;
use crate::emit::{EmitOp, ProgramSplit, emit_programs};
use dc_cam::RunLabel;
use dc_geometry::{Contour, Polyline, Transform, Vertex, nest, rounded_band};
use dc_project::{
    Board, DefinitionId, FeatureDefinition, FeatureInstance, InlayOp, InstanceId, ToolId,
};

const DATE: &str = "2026-11-11";

/// A rectangle from (`x0`, `y0`) to (`x1`, `y1`).
fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Polyline {
    Polyline::rect(Point::new(x0, y0), x1 - x0, y1 - y0)
}

/// An ellipse `rx_mm` × `ry_mm` round `centre`, as 128 chords.
fn oval(centre: (f64, f64), rx_mm: f64, ry_mm: f64) -> Polyline {
    let n = 128;
    Polyline::new(
        (0..n)
            .map(|k| {
                let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
                Vertex::line(centre.0 + rx_mm * a.cos(), centre.1 + ry_mm * a.sin())
            })
            .collect(),
        true,
    )
}

/// `project` with `shapes` added as boundary instances `b1`, `b2`, …, and
/// `op` listing them, its margin and rounding unset (a drawn band ignores
/// both).
fn with_boundary(project: &Project, op: &InlayOp, shapes: Vec<Polyline>) -> (Project, InlayOp) {
    let mut p = project.clone();
    let mut op = op.clone();
    op.margin_mm = None;
    op.outline_rounding_mm = None;
    op.boundary.clear();
    for (k, shape) in shapes.into_iter().enumerate() {
        let id = format!("b{}", k + 1);
        let definition = FeatureDefinition {
            id: DefinitionId::new(format!("bd{}", k + 1)),
            name: format!("Boundary {}", k + 1),
            profiles: vec![shape],
        };
        p.instances.push(FeatureInstance {
            id: InstanceId::new(id.clone()),
            definition: definition.id.clone(),
            transform: Transform::IDENTITY,
            name: format!("Boundary {}", k + 1),
            visible: true,
            locked: false,
            z_top_mm: None,
            z_bottom_mm: None,
        });
        p.definitions.insert(definition.id.clone(), definition);
        op.boundary.push(InstanceId::new(id));
    }
    (p, op)
}

/// `project` with the boundary instance `id` moved by (`dx_mm`, `dy_mm`).
fn moved(project: &Project, id: &str, dx_mm: f64, dy_mm: f64) -> Project {
    let mut p = project.clone();
    let instance = p
        .instances
        .iter_mut()
        .find(|i| i.id.as_str() == id)
        .expect("the boundary instance");
    instance.transform = instance.transform.then(Transform::translate(dx_mm, dy_mm));
    p
}

/// The two shapes' male: margin 6 would be the computed band; the drawn
/// one is `lo` to `hi` in Y, X 4 to 82. `clearing` adds the end mill's
/// pass (an open edge), `cut` the cut-out.
fn two_shapes_drawn(lo_mm: f64, hi_mm: f64, clearing: bool, cut: bool) -> (Project, InlayOp) {
    let op = InlayOp {
        clearing: if clearing {
            vec![edge_pass()]
        } else {
            Vec::new()
        },
        cut_out: cut.then(|| cut_out(None)),
        // Ignored with a boundary; set to a value that would be a V wall
        // were it read.
        ..inlay(Board::Male, "t2", 1.0, 1.0)
    };
    with_boundary(&two_shapes(), &op, vec![rect(4.0, lo_mm, 82.0, hi_mm)])
}

/// A cut at the floor along X from `x` to `x + 1`, at Y `y`.
fn floor_cut(x: f64, y: f64) -> String {
    program_at(x, y, &format!("G1 Z-2.5 F300\nX{} F1000\n", x + 1.0))
}

/// Every run of `op` in `project` emitted as a program of its own through
/// the whole safety pass: its label, its tool and its text.
fn run_programs(project: &Project, op: &InlayOp) -> Vec<(RunLabel, String, String)> {
    let mut project = project.clone();
    project.operations = vec![Operation::Inlay(op.clone())];
    let (runs, _) = dc_cam::generate_inlay_board(&project, op, Board::Male)
        .unwrap_or_else(|e| panic!("the inlay generates: {e}\n{e:?}"));
    runs.into_iter()
        .map(|run| {
            let tool = project
                .tools
                .iter()
                .find(|t| t.id == run.tool)
                .cloned()
                .expect("the run's tool");
            let one = EmitOp {
                op: Operation::Inlay(op.clone()),
                tool: tool.clone(),
                toolpath: run.toolpath,
                holes: None,
                label: run.label.clone(),
                board: run.board,
            };
            let (programs, _) = emit_programs(&project, &[one], ProgramSplit::ProgramPerTool, DATE)
                .unwrap_or_else(|e| panic!("{:?} fails its own project: {e}", run.label));
            let text = programs.into_iter().next().expect("a program").text;
            (run.label, tool.id.as_str().to_owned(), text)
        })
        .collect()
}

/// #604: the band is the drawn rectangle, read from the boundary
/// instance, whatever the margin says. The V-bit's cut at the floor,
/// whose cone is 2.5 mm wide at the stock top, is 3 mm inside `B` (Y 14)
/// at Y 17 and passes; at Y 15.5 the disc crosses `B` below the stock top
/// and fails. Drawn 6 mm further out (Y 8) the same cut passes.
#[test]
fn boundary_band_is_derived_from_the_instance() {
    let (project, open) = two_shapes_drawn(14.0, 36.0, true, false);
    check_male(&floor_cut(56.0, 17.0), &project, &open).unwrap();
    assert_cone_off(check_male(&floor_cut(56.0, 15.5), &project, &open), 6);

    // The same Inlay, its boundary instance redrawn: the verdict follows
    // the instance, not the program or the earlier band.
    let (wider, open) = two_shapes_drawn(8.0, 42.0, true, false);
    check_male(&floor_cut(56.0, 15.5), &wider, &open).unwrap();

    // Moving the instance moves the band: the cut at Y 13.5 is 5.5 mm
    // inside B once the lower edge is at Y 8 and crosses it at Y 12.
    check_male(&floor_cut(56.0, 13.5), &wider, &open).unwrap();
    let shifted = moved(&wider, "b1", 0.0, 4.0);
    assert_cone_off(check_male(&floor_cut(56.0, 13.5), &shifted, &open), 6);

    // The margin is ignored, set or not.
    let with_margin = InlayOp {
        margin_mm: Some(1.0),
        ..open.clone()
    };
    check_male(&floor_cut(56.0, 17.0), &wider, &with_margin).unwrap();
}

/// 61r: a male program exported for a boundary is checked against the
/// same project with the boundary instance moved 5 mm, so some cuts lie
/// outside the new band: its V-bit run fails S17 and its clearing run S9.
/// The program exported after the move passes. The boundary spans Y 8 to
/// 42, 12 mm clear of the shapes above and below; moved up 5 mm its
/// lower edge is at Y 13, still 7 mm clear, so the edge is open in both.
#[test]
fn moved_boundary_fails_the_old_program() {
    for (clearing, cut) in [(false, false), (true, true)] {
        let (project, op) = two_shapes_drawn(8.0, 42.0, clearing, cut);
        let programs = run_programs(&project, &op);
        assert_eq!(programs.len(), 1 + 2 * usize::from(clearing), "{clearing}");
        for (label, tool, text) in &programs {
            check_run(text, &project, &op, label, tool)
                .unwrap_or_else(|e| panic!("{label:?} fails its own project: {e}"));
        }

        let after = moved(&project, "b1", 0.0, 5.0);
        let mut failed_s17 = 0;
        let mut failed_s9 = 0;
        for (label, tool, text) in &programs {
            match check_run(text, &after, &op, label, tool) {
                Err(ExportError::ConeOffMaterial { .. }) => failed_s17 += 1,
                Err(ExportError::OffMaterial { .. }) => failed_s9 += 1,
                other => assert!(
                    clearing && *label == RunLabel::Main,
                    "{label:?} (clearing {clearing}): {other:?}"
                ),
            }
        }
        // S17 refuses the V wall's V-bit run, S9 the end mill's two.
        assert_eq!(
            (failed_s17, failed_s9),
            (usize::from(!clearing), 2 * usize::from(clearing))
        );

        // The program exported after the move passes the whole pass.
        export_inlay(&after, &op).unwrap_or_else(|e| panic!("the moved boundary exports: {e}"));
    }
}

/// #604: the cut-out is held to its path, the drawn band's edge `B`
/// inset by the end mill's radius (open edge) or by `P·s + r` (V wall),
/// and to the floor ring out to `B`. With the boundary's lower edge at Y
/// 14 the open edge's path is at Y 15.5875; off it, in the ring or across
/// `B`, fails S9. A boundary drawn further out puts the path further out,
/// and a V wall's is 1 mm further in.
#[test]
fn cut_out_off_the_boundary_ring_fails() {
    let (project, open) = two_shapes_drawn(14.0, 36.0, true, true);
    let label = RunLabel::CutOut(ToolId::new("t3"));
    let at = |y: f64| program_at(55.0, y, "G1 Z-12.2 F300\nX60 F1000\n");
    check_run(&at(15.5875), &project, &open, &label, "t3").unwrap();
    // Off the path in the ring (16.5, 17.6), across B (15.4), and outside
    // the drawn band altogether (13, 12).
    for y in [16.5, 15.4, 17.6, 13.0, 12.0] {
        assert_off(check_run(&at(y), &project, &open, &label, "t3"), 6);
    }

    // Redrawn 6 mm further out the path is at Y 9.5875.
    let (wider, wide_op) = two_shapes_drawn(8.0, 42.0, true, true);
    check_run(&at(9.5875), &wider, &wide_op, &label, "t3").unwrap();
    assert_off(check_run(&at(15.5875), &wider, &wide_op, &label, "t3"), 6);

    // With a V wall the path is B inset by P·s + r, at Y 16.5875.
    let (_, v_wall) = two_shapes_drawn(14.0, 36.0, false, true);
    check_run(&at(16.5875), &project, &v_wall, &label, "t3").unwrap();
    for y in [15.5875, 13.0] {
        assert_off(check_run(&at(y), &project, &v_wall, &label, "t3"), 6);
    }
}

/// #604: the V-bit never cuts where the board's full height stands
/// outside the drawn band. A hand-built toolpath that crosses `B` below
/// the stock top fails S17: the disc at the stock top, `(top − z)·s`,
/// crosses the edge, whether the V-bit runs along it at the floor (Y
/// 15.5) or leaves the band at Z −1 (Y 17 to Y 10). With a V wall, where
/// no end mill clears the edge, a cut leaving the surround fails too.
#[test]
fn v_bit_across_a_drawn_open_edge_fails_s17() {
    let (project, open) = two_shapes_drawn(14.0, 36.0, true, false);
    check_male(&floor_cut(56.0, 17.0), &project, &open).unwrap();
    assert_cone_off(check_male(&floor_cut(56.0, 15.5), &project, &open), 6);

    // Crossing B at Z -1 (above the start plane, below the stock top):
    // the plunge at Y 17 passes, the move to Y 10 fails on line 7.
    let crossing = program_at(56.0, 17.0, "G1 Z-1 F300\nY10 F1000\n");
    assert_cone_off(check_male(&crossing, &project, &open), 7);

    // The same crossing down to the floor, and along the board's edge.
    let deep = program_at(56.0, 17.0, "G1 Z-2.5 F300\nY12 F1000\n");
    assert_cone_off(check_male(&deep, &project, &open), 7);

    // A cut over the drawn band's side, outside it from the start.
    assert_cone_off(check_male(&floor_cut(56.0, 12.0), &project, &open), 6);

    // A V wall: the cone is held to the surround at the start plane, so a
    // cut leaving it (the band's V wall stands at B) fails, and the cut at
    // Y 15.5, 1.5 mm in with a 1 mm disc at the start plane, passes.
    let (_, v_wall) = two_shapes_drawn(14.0, 36.0, false, false);
    check_male(&floor_cut(56.0, 15.5), &project, &v_wall).unwrap();
    let leaving = program_at(56.0, 15.5, "G1 Z-2.5 F300\nY12 F1000\n");
    assert_cone_off(check_male(&leaving, &project, &v_wall), 7);
}

/// #604: the generator's male for a drawn boundary passes S17 and the
/// clearing and cut-out S9 through the whole safety pass: the small star
/// with a rectangle, an oval, the two together, a contour round the
/// design and a rectangle that runs past the board (clipped), each with
/// and without the end mill's pass and with and without a cut-out (with
/// tabs and without).
#[test]
fn generated_boundary_males_pass_s17_and_s9() {
    let star = small_star();
    let contour = {
        let instance = star.instances.first().expect("the star");
        let profiles = &star.definitions[&instance.definition].profiles;
        let contours: Vec<Contour> = profiles
            .iter()
            .map(|p| Contour::new(p.clone()).expect("a contour"))
            .collect();
        let (design, _) = nest(contours).expect("nests");
        rounded_band(&design, 6.0, 0.0, star.flatten_tolerance_mm)[0]
            .outer()
            .polyline()
            .clone()
    };
    let shapes: [(&str, Vec<Polyline>); 5] = [
        ("rectangle", vec![rect(8.0, 8.0, 52.0, 52.0)]),
        ("oval", vec![oval((30.0, 30.0), 22.0, 20.0)]),
        (
            "rectangle and oval",
            vec![rect(9.0, 9.0, 51.0, 51.0), oval((30.0, 30.0), 24.0, 22.0)],
        ),
        ("contour", vec![contour]),
        ("clipped", vec![rect(-4.0, -4.0, 64.0, 64.0)]),
    ];
    let cut_outs = [None, Some(cut_out(None)), Some(cut_out(Some(four_tabs())))];
    for (name, shape) in shapes {
        for clearing in [false, true] {
            for cut in &cut_outs {
                let op = InlayOp {
                    clearing: if clearing {
                        vec![edge_pass()]
                    } else {
                        Vec::new()
                    },
                    cut_out: cut.clone(),
                    ..inlay(Board::Male, "t5", 1.0, 4.0)
                };
                let (project, op) = with_boundary(&star, &op, shape.clone());
                export_inlay(&project, &op).unwrap_or_else(|e| {
                    panic!(
                        "{name}, clearing {clearing}, cut-out {}: {e}",
                        cut.is_some()
                    )
                });
            }
        }
    }
}

/// #604: no run of the male reads a band the program does not carry: the
/// same Inlay with the boundary list emptied (the computed band, margin
/// 6) is a different band, and a program for the drawn one is refused by
/// it where the two differ (the drawn band's 12 mm reaches beyond 6).
#[test]
fn boundary_programs_are_held_to_the_drawn_band_not_the_margin() {
    let (project, op) = two_shapes_drawn(8.0, 42.0, true, false);
    let programs = run_programs(&project, &op);
    let computed = InlayOp {
        boundary: Vec::new(),
        margin_mm: Some(6.0),
        ..op.clone()
    };
    let refused = programs
        .iter()
        .filter(|(label, tool, text)| check_run(text, &project, &computed, label, tool).is_err())
        .count();
    assert!(refused > 0, "the margin's band held the drawn band's cuts");
}

/// #604: a boundary that no longer resolves (its instance deleted, or
/// turned into an open outline) leaves the program unchecked against any
/// band, so every cut of the old program is refused, never held to the
/// margin the Inlay still carries.
#[test]
fn boundary_that_no_longer_resolves_refuses_the_old_program() {
    let (project, op) = two_shapes_drawn(8.0, 42.0, true, true);
    let programs = run_programs(&project, &op);
    let mut gone = project.clone();
    gone.instances.retain(|i| i.id.as_str() != "b1");
    let mut open = project.clone();
    open.definitions
        .get_mut(&DefinitionId::new("bd1"))
        .expect("the definition")
        .profiles = vec![Polyline::new(
        vec![Vertex::line(4.0, 8.0), Vertex::line(82.0, 8.0)],
        false,
    )];
    let op = InlayOp {
        margin_mm: Some(6.0),
        ..op
    };
    for broken in [&gone, &open] {
        for (label, tool, text) in &programs {
            assert!(
                check_run(text, broken, &op, label, tool).is_err(),
                "{label:?} passes with an unresolved boundary"
            );
        }
    }
}
