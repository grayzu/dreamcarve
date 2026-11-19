// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Acceptance 61w: a V-Carve's corners with a tapered ball nose
//! (`docs/specs/phase-3/design.md`, "Algorithms → Tapered ball nose";
//! functional 49j).

use super::*;
use dc_cam::{RunLabel, WarningCode, generate_vcarve};
use dc_geometry::nearest_point;
use dc_project::test_support::{sample_v_carve, tool_tbn_fine, tool_tbn_ultra};
use dc_project::{Operation, Tool, ToolId, VCarveOp};

/// The stock's thickness, mm.
const TOP_MM: f64 = 30.0;

/// Acceptance 61w, "the outline at Z 0 is the drawn one" (functional 49j:
/// "at the surface the bit reaches every corner … so the outline is cut
/// as drawn"). A 20 × 20 mm square with each bench bit, `flat_depth_mm`
/// 2, warns `tapered_corners_softened`: below the surface its corners are
/// round. At the stock top the bit, its tip `d` down, cuts a disc as wide
/// as its profile is at `d`. The union of those discs along the program is
/// the square: no disc reaches past the outline, and every point of the
/// outline, its four corners too, is on a disc's rim, within 0.01 mm.
#[test]
fn tapered_carve_cuts_the_drawn_outline_at_the_surface() {
    let ultra = tool_tbn_ultra();
    let fine = Tool {
        id: ToolId::new("t2"),
        ..tool_tbn_fine()
    };
    for bit in [ultra, fine] {
        let name = bit.name.clone();
        let profile = bit.carve_profile().expect("every value is set");
        let square = Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0);
        let mut p = drawing_project(vec![square.clone()], TOP_MM);
        p.tools.push(bit);
        let op = VCarveOp {
            targets: vec![InstanceId::new("i1")],
            tool: ToolId::new("t2"),
            flat_depth_mm: Some(2.0),
            step_over_fraction: Some(0.3),
            ..sample_v_carve()
        };
        p.operations = vec![Operation::VCarve(op.clone())];
        let (runs, warnings) = generate_vcarve(&p, &op).expect("the carve generates");
        assert!(
            warnings
                .iter()
                .any(|w| w.code() == WarningCode::TaperedCornersSoftened),
            "{name}: {warnings:?}"
        );
        let toolpath = &runs
            .iter()
            .find(|r| r.label == RunLabel::Main)
            .expect("the bit's run")
            .toolpath;

        let outline = Transform::translate(AT.0, AT.1)
            .apply_polyline(&square)
            .expect("placed");
        // Each sampled tip position with the radius it cuts at the top.
        let discs: Vec<(Point, f64)> = cut_samples(toolpath, 0.01)
            .into_iter()
            .map(|q| (q.xy(), profile.half_width_mm((TOP_MM - q.z).max(0.0))))
            .collect();
        assert!(discs.len() > 1000, "{name}: the carve cuts");
        for &(centre, radius_mm) in &discs {
            let room = nearest_point(&outline, centre).expect("an outline").gap_mm;
            assert!(
                radius_mm <= room + 0.01,
                "{name}: at {centre:?} the bit cuts {radius_mm} mm wide at the top, {room} mm from the outline"
            );
        }
        let corners = [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)];
        for (k, &(ax, ay)) in corners.iter().enumerate() {
            let (bx, by) = corners[(k + 1) % 4];
            // Along one side from its corner, 0.25 mm apart.
            for step in 0..80 {
                let t = f64::from(step) / 80.0;
                let q = Point::new(AT.0 + ax + (bx - ax) * t, AT.1 + ay + (by - ay) * t);
                let gap = discs
                    .iter()
                    .map(|&(centre, radius_mm)| centre.distance_to(q) - radius_mm)
                    .fold(f64::INFINITY, f64::min);
                assert!(
                    gap.abs() <= 0.01,
                    "{name}: the cut at the top stops {gap} mm from the outline at {q:?}"
                );
            }
        }
    }
}
