// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Photo V-Carve generator (#646; `docs/specs/phase-3/design.md`,
//! "Algorithms → Photo V-Carve", the test plan's "dc-cam — photo";
//! functional acceptance 59). Every test that emits motion runs
//! [`assert_safe`], which holds a Photo's toolpath to S19 too.
//!
//! The picture is built in code, not read from `tests/fixtures/photo/
//! gradient.png` (#645 commits that file): `gradient_bytes` is the same
//! 64 × 32 picture, black at its left edge to white at its right, with a
//! pure white 8 × 8 block at columns 8 to 15, rows 12 to 19, and a pure
//! black one at columns 48 to 55, rows 12 to 19. Placed 100 mm wide at
//! (5, 5) on 110 × 60 × 6 mm stock, a pixel is 1.5625 mm square, the
//! white block covers X 17.5 to 30, the black one X 80 to 92.5, both Y
//! 23.75 to 36.25, and 50 % grey (127.5) is at X 55.

use super::*;
use crate::test_support::assert_safe;
use crate::toolpath::MoveKind;
use dc_geometry::{EPS_MM, Point3, Polyline, Transform};
use dc_project::test_support::{
    gradient_bytes, gray_png_base64, sample_photo_v_carve, sample_project,
};
use dc_project::{
    Bitmap, BitmapId, DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Operation,
    PhotoStrategy, PocketOp, ToolId, ToolKind,
};

/// The stock top, mm.
const TOP: f64 = 6.0;
/// The footprint: X 5 to 105, Y 5 to 55.
const X0: f64 = 5.0;
const Y0: f64 = 5.0;
const W: f64 = 100.0;
const H: f64 = 50.0;
/// One pixel, mm.
const PX: f64 = 100.0 / 64.0;

/// A bitmap `b1` of `bytes` (`width_px` × `height_px`), its footprint
/// `width_mm` wide at (`x_mm`, `y_mm`).
fn bitmap(width_px: u32, height_px: u32, bytes: &[u8], at: (f64, f64), width_mm: f64) -> Bitmap {
    Bitmap {
        id: BitmapId::new("b1"),
        name: "gradient".into(),
        width_px,
        height_px,
        x_mm: at.0,
        y_mm: at.1,
        width_mm,
        height_mm: width_mm * f64::from(height_px) / f64::from(width_px),
        visible: true,
        locked: false,
        png_base64: gray_png_base64(width_px, height_px, bytes),
    }
}

/// A 6.35 mm V-bit `t2` of `angle_deg`, feed 1000, plunge 300.
fn v_bit(angle_deg: f64) -> dc_project::Tool {
    dc_project::Tool {
        id: ToolId::new("t2"),
        name: format!("{angle_deg}° V-bit"),
        cutter: ToolKind::VBit {
            included_angle_deg: angle_deg,
        },
        step_down_mm: None,
        ..dc_project::test_support::tool_6_35_upcut()
    }
}

/// Acceptance 59's Photo V-Carve `o8`: the 30° bit, 2 mm deep at most,
/// lines at 22.5°, 1.1 groove widths apart, sampled every 0.25 mm,
/// lifting 1 mm between grooves.
fn acceptance_op() -> PhotoVCarveOp {
    PhotoVCarveOp {
        max_depth_mm: Some(2.0),
        ..sample_photo_v_carve()
    }
}

/// 110 × 60 × 6 mm stock with the end mill `t1`, the 30° V-bit `t2`, the
/// picture `pixels` and `op` alone.
fn project_with(picture: Bitmap, op: PhotoVCarveOp) -> Project {
    let mut project = sample_project();
    project.stock.width_mm = 110.0;
    project.stock.height_mm = 60.0;
    project.tools.push(v_bit(30.0));
    project.bitmaps = vec![picture];
    project.operations = vec![Operation::PhotoVCarve(op)];
    project
}

/// The gradient, 100 mm wide at (5, 5), carved by `op`.
fn gradient_project(op: PhotoVCarveOp) -> Project {
    project_with(bitmap(64, 32, &gradient_bytes(), (X0, Y0), W), op)
}

/// `project`'s Photo V-Carve, generated and checked by [`assert_safe`].
fn generate(project: &Project) -> (Toolpath, Vec<Warning>) {
    let op = project
        .operations
        .iter()
        .find_map(|o| match o {
            Operation::PhotoVCarve(p) => Some(p.clone()),
            _ => None,
        })
        .expect("the project has a Photo V-Carve");
    let (toolpath, warnings) = generate_photo(project, &op).expect("the Photo V-Carve generates");
    assert_safe(&toolpath, project, &Operation::PhotoVCarve(op));
    (toolpath, warnings)
}

/// The grooves of `toolpath`, in cut order: each one's points from the
/// end of the plunge into it to the last cut before the bit rises above
/// `start_plane_z_mm`.
fn grooves_of(toolpath: &Toolpath, start_plane_z_mm: f64) -> Vec<Vec<Point3>> {
    let mut out: Vec<Vec<Point3>> = Vec::new();
    let mut current: Option<Vec<Point3>> = None;
    let mut previous: Option<Point3> = None;
    for mv in &toolpath.moves {
        let below = mv.to.z <= start_plane_z_mm + EPS_MM;
        let plunge = mv.kind == MoveKind::Linear
            && previous.is_some_and(|p| {
                p.xy().distance_to(mv.to.xy()) <= EPS_MM && p.z > start_plane_z_mm + EPS_MM
            });
        match (&mut current, below) {
            (None, true) if plunge => current = Some(vec![mv.to]),
            (Some(points), true) if mv.kind == MoveKind::Linear => points.push(mv.to),
            (Some(_), _) => {
                out.extend(current.take());
            }
            _ => {}
        }
        previous = Some(mv.to);
    }
    out.extend(current);
    out
}

/// `(−sin θ, cos θ)`, the family's normal at `angle_deg`.
fn normal(angle_deg: f64) -> Vec2 {
    let theta = angle_deg.to_radians();
    Vec2::new(-theta.sin(), theta.cos())
}

/// The full-depth width of acceptance 59: `2 · 2 · tan 15°`.
fn full_width_mm() -> f64 {
    4.0 * 15f64.to_radians().tan()
}

/// The Z of `groove` at every point of its moves that lies in `region`
/// (X range, Y range), checked every 0.1 mm along them.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "a test: a count of 0.1 mm steps along a short move"
)]
fn z_in(grooves: &[Vec<Point3>], xs: (f64, f64), ys: (f64, f64)) -> Vec<f64> {
    let mut out = Vec::new();
    for groove in grooves {
        for pair in groove.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let steps = (a.xy().distance_to(b.xy()) / 0.1).ceil().max(1.0) as usize;
            for i in 0..=steps {
                let t = i as f64 / steps as f64;
                let p = Point3::new(
                    a.x + (b.x - a.x) * t,
                    a.y + (b.y - a.y) * t,
                    a.z + (b.z - a.z) * t,
                );
                if (xs.0..=xs.1).contains(&p.x) && (ys.0..=ys.1).contains(&p.y) {
                    out.push(p.z);
                }
            }
        }
    }
    out
}

/// Acceptance 59: 30°, 2 mm, 1.1 give grooves 1.179 mm apart (1.1 ×
/// 1.072), within 0.001 mm, measured perpendicular to them.
#[test]
fn grooves_are_spaced_by_the_fraction_of_the_full_depth_width() {
    let (toolpath, _) = generate(&gradient_project(acceptance_op()));
    let grooves = grooves_of(&toolpath, TOP);
    assert!(grooves.len() > 50, "{} grooves", grooves.len());
    let n = normal(22.5);
    let offsets: Vec<f64> = grooves
        .iter()
        .map(|g| n.dot(g[0].xy() - Point::new(0.0, 0.0)))
        .collect();
    let expected = 1.1 * full_width_mm();
    assert!((expected - 1.179).abs() < 0.001, "{expected}");
    for pair in offsets.windows(2) {
        let spacing = pair[0] - pair[1];
        assert!(
            (spacing - 1.179).abs() <= 0.001,
            "grooves {spacing} mm apart: {offsets:?}"
        );
    }
}

/// Acceptance 59: every groove is straight and at 22.5° within 0.01°, and
/// consecutive grooves run in opposite directions.
#[test]
fn grooves_run_at_the_line_angle_and_alternate() {
    let (toolpath, _) = generate(&gradient_project(acceptance_op()));
    let grooves = grooves_of(&toolpath, TOP);
    let mut last_direction: Option<(usize, Vec2)> = None;
    for (i, groove) in grooves.iter().enumerate() {
        let (first, last) = (groove[0].xy(), groove[groove.len() - 1].xy());
        if first.distance_to(last) < 1e-3 {
            continue;
        }
        let direction = (last - first).normalized().expect("a groove has a length");
        let angle = direction
            .y
            .atan2(direction.x)
            .to_degrees()
            .rem_euclid(180.0);
        assert!((angle - 22.5).abs() <= 0.01, "a groove at {angle}°");
        let n = Vec2::new(-direction.y, direction.x);
        for p in groove {
            assert!(
                n.dot(p.xy() - first).abs() <= 1e-6,
                "a groove bends at {p:?}"
            );
        }
        if let Some((j, previous)) = last_direction
            && j + 1 == i
        {
            assert!(
                previous.dot(direction) < -0.999,
                "two grooves in a row run the same way"
            );
        }
        last_direction = Some((i, direction));
    }
}

/// Design spec, "Photo V-Carve", step 3: a groove's samples run every
/// `sample_mm` from the point it is cut from, whichever way it runs, and
/// only the last gap, to its end, is shorter, as RasterCarve samples
/// (#647: its golden programs showed DreamCarve's odd grooves sampled from
/// their far end, the first gap the short one).
#[test]
fn samples_run_every_step_from_each_grooves_start() {
    let footprint = Rect {
        min: Point::new(X0, Y0),
        max: Point::new(X0 + W, Y0 + H),
    };
    let sample_mm = 0.25;
    let family = grooves(&OperationId::new("o8"), footprint, 22.5, 1.179, sample_mm)
        .expect("the family lays out");
    let mut odd_seen = false;
    for (index, line) in family.lines.iter().enumerate() {
        if line.len() < 3 {
            continue;
        }
        odd_seen |= index % 2 == 1;
        let gaps: Vec<f64> = line.windows(2).map(|p| p[0].distance_to(p[1])).collect();
        let (last, regular) = gaps.split_last().expect("gaps");
        for gap in regular {
            assert!(
                (gap - sample_mm).abs() <= 1e-9,
                "groove {index}: a {gap} mm gap before its last: {gaps:?}"
            );
        }
        assert!(*last <= sample_mm + 1e-9, "groove {index}: last gap {last}");
    }
    assert!(odd_seen, "an odd groove of three samples or more");
}

/// Acceptance 59 (changed in the design spec): the family passes through
/// the picture's top-left corner, and the first groove is that one.
#[test]
fn one_groove_passes_through_the_top_left_corner() {
    let (toolpath, _) = generate(&gradient_project(acceptance_op()));
    let grooves = grooves_of(&toolpath, TOP);
    let corner = Point::new(X0, Y0 + H);
    assert!(
        grooves[0][0].xy().distance_to(corner) <= 1e-9,
        "the first groove starts at {:?}",
        grooves[0][0]
    );
    let n = normal(22.5);
    let through = grooves
        .iter()
        .filter(|g| n.dot(g[0].xy() - corner).abs() <= 1e-9)
        .count();
    assert_eq!(through, 1);
}

/// Acceptance 59: at 0° every groove is parallel to X, the first along
/// the picture's top edge.
#[test]
fn zero_angle_grooves_are_parallel_to_x() {
    let op = PhotoVCarveOp {
        line_angle_deg: 0.0,
        ..acceptance_op()
    };
    let (toolpath, _) = generate(&gradient_project(op));
    let grooves = grooves_of(&toolpath, TOP);
    assert!(
        (grooves[0][0].y - (Y0 + H)).abs() <= 1e-9,
        "{:?}",
        grooves[0][0]
    );
    for groove in &grooves {
        for p in groove {
            assert!((p.y - groove[0].y).abs() <= 1e-9, "{groove:?}");
        }
        let span = (groove[groove.len() - 1].x - groove[0].x).abs();
        assert!((span - W).abs() <= 1e-9, "a groove {span} mm long");
    }
}

/// Acceptance 59: every cutting move lies inside the 100 × 50 mm
/// footprint, links included.
#[test]
fn every_cut_is_inside_the_footprint() {
    for strategy in [PhotoStrategy::Raster, PhotoStrategy::Hatch] {
        let op = PhotoVCarveOp {
            strategy,
            ..acceptance_op()
        };
        let (toolpath, _) = generate(&gradient_project(op));
        for mv in toolpath.moves.iter().filter(|m| m.is_cut()) {
            let p = mv.to;
            assert!(
                p.x >= X0 - 1e-9
                    && p.x <= X0 + W + 1e-9
                    && p.y >= Y0 - 1e-9
                    && p.y <= Y0 + H + 1e-9,
                "{strategy:?}: a cut ends at {p:?}"
            );
        }
    }
}

/// Acceptance 59: over the black block the grooves are at Z −2.000, over
/// the white block at 0.000, within 0.01 mm; over 50 % grey at −1.00
/// within 0.02 mm (Z below the stock top). A pixel in from each block's
/// edge, where the bilinear sample is the block's own.
#[test]
fn black_is_max_depth_white_is_the_surface_and_grey_is_between() {
    let (toolpath, _) = generate(&gradient_project(acceptance_op()));
    let grooves = grooves_of(&toolpath, TOP);
    let rows = (23.75 + PX, 36.25 - PX);
    let black = z_in(&grooves, (80.0 + PX, 92.5 - PX), rows);
    let white = z_in(&grooves, (17.5 + PX, 30.0 - PX), rows);
    let grey = z_in(&grooves, (54.95, 55.05), (Y0, Y0 + H));
    for (name, zs, expected, within) in [
        ("black", &black, TOP - 2.0, 0.01),
        ("white", &white, TOP, 0.01),
        ("grey", &grey, TOP - 1.0, 0.02),
    ] {
        assert!(zs.len() > 10, "{name}: {} points", zs.len());
        for z in zs {
            assert!(
                (z - expected).abs() <= within,
                "{name}: Z {z}, not {expected}"
            );
        }
    }
}

/// Acceptance 59: with `invert` the black and white depths swap.
#[test]
fn invert_swaps_black_and_white() {
    let op = PhotoVCarveOp {
        invert: true,
        ..acceptance_op()
    };
    let (toolpath, _) = generate(&gradient_project(op));
    let grooves = grooves_of(&toolpath, TOP);
    let rows = (23.75 + PX, 36.25 - PX);
    let black = z_in(&grooves, (80.0 + PX, 92.5 - PX), rows);
    let white = z_in(&grooves, (17.5 + PX, 30.0 - PX), rows);
    assert!(black.len() > 10 && white.len() > 10);
    assert!(black.iter().all(|z| (z - TOP).abs() <= 0.01), "{black:?}");
    assert!(
        white.iter().all(|z| (z - (TOP - 2.0)).abs() <= 0.01),
        "{white:?}"
    );
}

/// Acceptance 59: `hatch` adds a second family at 112.5° after the
/// first, spaced and alternated the same way.
#[test]
fn hatch_adds_a_second_family_at_ninety_degrees() {
    let raster = grooves_of(&generate(&gradient_project(acceptance_op())).0, TOP);
    let op = PhotoVCarveOp {
        strategy: PhotoStrategy::Hatch,
        ..acceptance_op()
    };
    let hatch = grooves_of(&generate(&gradient_project(op)).0, TOP);
    assert!(
        hatch.len() > raster.len() + 50,
        "{} vs {}",
        hatch.len(),
        raster.len()
    );
    assert_eq!(
        &hatch[..raster.len()],
        &raster[..],
        "the first family is the raster"
    );
    let second = &hatch[raster.len()..];
    let n = normal(112.5);
    let mut last_direction: Option<(usize, Vec2)> = None;
    for pair in second.windows(2) {
        let spacing = (n.dot(pair[0][0].xy() - Point::new(0.0, 0.0))
            - n.dot(pair[1][0].xy() - Point::new(0.0, 0.0)))
        .abs();
        assert!((spacing - 1.179).abs() <= 0.001, "{spacing}");
    }
    for (i, groove) in second.iter().enumerate() {
        let (first, last) = (groove[0].xy(), groove[groove.len() - 1].xy());
        if first.distance_to(last) < 1e-3 {
            continue;
        }
        let direction = (last - first).normalized().expect("a length");
        let angle = direction
            .y
            .atan2(direction.x)
            .to_degrees()
            .rem_euclid(180.0);
        assert!(
            (angle - 112.5).abs() <= 0.01,
            "a second-family groove at {angle}°"
        );
        if let Some((j, previous)) = last_direction
            && j + 1 == i
        {
            assert!(
                previous.dot(direction) < -0.999,
                "two grooves run the same way"
            );
        }
        last_direction = Some((i, direction));
    }
}

/// Acceptance 59: between two grooves of a family the bit rises to Z +1
/// above the start plane and crosses there, both at the feed rate, then
/// plunges at the plunge rate; no rapid.
#[test]
fn between_grooves_the_bit_rises_to_the_retract_and_feeds_across() {
    let (toolpath, _) = generate(&gradient_project(acceptance_op()));
    let moves = &toolpath.moves;
    let links: Vec<usize> = (1..moves.len())
        .filter(|&i| {
            moves[i].kind == MoveKind::Linear
                && (moves[i].to.z - (TOP + 1.0)).abs() <= EPS_MM
                && moves[i - 1].to.z < TOP + EPS_MM
        })
        .collect();
    let grooves = grooves_of(&toolpath, TOP).len();
    assert_eq!(
        links.len(),
        grooves - 1,
        "one link between each two grooves"
    );
    for i in links {
        let (rise, across, plunge) = (moves[i], moves[i + 1], moves[i + 2]);
        assert!(
            rise.to.xy().distance_to(moves[i - 1].to.xy()) <= EPS_MM,
            "{rise:?}"
        );
        assert_eq!(rise.feed_mm_min, Some(1000.0));
        assert_eq!(across.kind, MoveKind::Linear, "{across:?}");
        assert!((across.to.z - (TOP + 1.0)).abs() <= EPS_MM, "{across:?}");
        assert_eq!(across.feed_mm_min, Some(1000.0));
        assert_eq!(plunge.kind, MoveKind::Linear, "{plunge:?}");
        assert!(plunge.to.xy().distance_to(across.to.xy()) <= EPS_MM);
        assert_eq!(plunge.feed_mm_min, Some(300.0));
    }
    let rapids_below_safe = moves
        .iter()
        .skip(1)
        .filter(|m| m.is_rapid() && m.to.z < TOP + 5.0 - EPS_MM)
        .count();
    // The rapid down to the first plunge, above the stock top.
    assert_eq!(rapids_below_safe, 1);
}

/// Acceptance 59: a spacing below 1 warns `photo_lines_overlap`.
#[test]
fn spacing_below_one_warns() {
    let op = PhotoVCarveOp {
        line_spacing_fraction: 0.9,
        ..acceptance_op()
    };
    let (_, warnings) = generate(&gradient_project(op));
    assert_eq!(
        warnings,
        vec![Warning::PhotoLinesOverlap {
            op: OperationId::new("o8"),
            fraction: 0.9,
        }]
    );
    assert_eq!(warnings[0].code().as_str(), "photo_lines_overlap");
    let (_, quiet) = generate(&gradient_project(acceptance_op()));
    assert_eq!(quiet, vec![]);
}

/// A flat white stretch is one move: on an all-white picture at 0° each
/// groove is its plunge and one cut.
#[test]
fn collinear_samples_merge_into_one_move() {
    let white = vec![255_u8; 64 * 32];
    let op = PhotoVCarveOp {
        line_angle_deg: 0.0,
        ..acceptance_op()
    };
    let project = project_with(bitmap(64, 32, &white, (X0, Y0), W), op);
    let (toolpath, _) = generate(&project);
    let grooves = grooves_of(&toolpath, TOP);
    assert!(grooves.len() > 30);
    for groove in &grooves {
        assert_eq!(groove.len(), 2, "{groove:?}");
        assert!((groove[0].z - TOP).abs() <= EPS_MM);
    }
}

/// `sample` is bilinear between the four nearest pixel centres and
/// clamped to the edge pixels: a 2 × 2 picture 2 mm square at the origin
/// has its centres at (0.5, 1.5) (0, top left), (1.5, 1.5) (100),
/// (0.5, 0.5) (200) and (1.5, 0.5) (255).
#[test]
fn sample_is_bilinear_and_clamped_at_the_edges() {
    let picture = bitmap(2, 2, &[0, 100, 200, 255], (0.0, 0.0), 2.0);
    let pixels = picture.pixels().expect("the picture decodes");
    let at = |x: f64, y: f64| sample(&picture, &pixels, Point::new(x, y));
    assert!((at(0.5, 1.5) - 0.0).abs() < 1e-9);
    assert!((at(1.5, 1.5) - 100.0).abs() < 1e-9);
    assert!((at(0.5, 0.5) - 200.0).abs() < 1e-9);
    assert!((at(1.5, 0.5) - 255.0).abs() < 1e-9);
    assert!((at(1.0, 1.5) - 50.0).abs() < 1e-9);
    assert!((at(1.0, 1.0) - 138.75).abs() < 1e-9);
    // Clamped: the corners and far outside read the edge pixels.
    assert!((at(0.0, 2.0) - 0.0).abs() < 1e-9);
    assert!((at(-5.0, -5.0) - 200.0).abs() < 1e-9);
    assert!((at(50.0, 50.0) - 100.0).abs() < 1e-9);
    assert!((at(1e300, -1e300) - 255.0).abs() < 1e-9);
    // Not a number reads a pixel, never panics.
    assert!((0.0..=255.0).contains(&at(f64::NAN, f64::INFINITY)));
}

/// Row 0 is the picture's top row, at the highest Y: black on top, white
/// below, carved at 0° the top groove is the deep one.
#[test]
fn top_row_is_at_the_highest_y() {
    let picture = bitmap(1, 2, &[0, 255], (0.0, 0.0), 10.0);
    let pixels = picture.pixels().expect("the picture decodes");
    assert!(sample(&picture, &pixels, Point::new(5.0, 19.0)) < 1.0);
    assert!(sample(&picture, &pixels, Point::new(5.0, 1.0)) > 254.0);

    let picture = bitmap(4, 2, &[0, 0, 0, 0, 255, 255, 255, 255], (X0, Y0), 100.0);
    let op = PhotoVCarveOp {
        line_angle_deg: 0.0,
        ..acceptance_op()
    };
    let (toolpath, _) = generate(&project_with(picture, op));
    let grooves = grooves_of(&toolpath, TOP);
    let top = &grooves[0];
    assert!((top[0].y - 55.0).abs() <= 1e-9);
    assert!(
        top.iter().all(|p| (p.z - (TOP - 2.0)).abs() <= 1e-9),
        "{top:?}"
    );
    let bottom = &grooves[grooves.len() - 1];
    assert!(
        bottom.iter().all(|p| (p.z - TOP).abs() <= 1e-9),
        "{bottom:?}"
    );
}

/// A maximum depth past the bit's cone is refused: the 90° 6.35 mm bit's
/// cone is 3.175 mm.
#[test]
fn max_depth_beyond_the_cone_is_an_error() {
    let mut project = gradient_project(PhotoVCarveOp {
        max_depth_mm: Some(4.0),
        ..acceptance_op()
    });
    project.tools[1] = v_bit(90.0);
    let Operation::PhotoVCarve(op) = project.operations[0].clone() else {
        unreachable!()
    };
    let err = generate_photo(&project, &op).unwrap_err();
    assert!(
        matches!(err, CamError::VBitConeExceeded { depth_mm, cone_mm, .. }
            if (depth_mm - 4.0).abs() < 1e-9 && (cone_mm - 3.175).abs() < 1e-9),
        "{err:?}"
    );
}

/// An earlier Pocket of the footprint 1 mm deep clears the start plane:
/// with `start_depth_mm` 1 every groove is 1 mm lower, the white block at
/// Z 5, the black at 3, and the links 1 mm above the start plane.
#[test]
fn start_depth_lowers_the_grooves() {
    let op = PhotoVCarveOp {
        start_depth_mm: 1.0,
        ..acceptance_op()
    };
    let project = with_pocket(gradient_project(op), 1.0);
    let (toolpath, _) = generate(&project);
    let plane = TOP - 1.0;
    let grooves = grooves_of(&toolpath, plane);
    let rows = (23.75 + PX, 36.25 - PX);
    let black = z_in(&grooves, (80.0 + PX, 92.5 - PX), rows);
    let white = z_in(&grooves, (17.5 + PX, 30.0 - PX), rows);
    assert!(
        black.iter().all(|z| (z - (plane - 2.0)).abs() <= 0.01),
        "{black:?}"
    );
    assert!(white.iter().all(|z| (z - plane).abs() <= 0.01), "{white:?}");
    let highest_cut = grooves
        .iter()
        .flatten()
        .map(|p| p.z)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(highest_cut <= plane + EPS_MM);
    assert!(
        toolpath
            .moves
            .iter()
            .any(|m| m.kind == MoveKind::Linear && (m.to.z - (plane + 1.0)).abs() <= EPS_MM)
    );
}

/// `project` with a 102 × 52 mm rectangle `i9` round the footprint and an
/// end mill Pocket `o2` of it, `depth_mm` deep, before its operations.
fn with_pocket(mut project: Project, depth_mm: f64) -> Project {
    let rect = FeatureDefinition {
        id: DefinitionId::new("d9"),
        name: "Clearing".into(),
        profiles: vec![Polyline::rect(Point::new(0.0, 0.0), 102.0, 52.0)],
    };
    project.definitions.insert(rect.id.clone(), rect);
    project.instances.push(FeatureInstance {
        id: InstanceId::new("i9"),
        definition: DefinitionId::new("d9"),
        transform: Transform::translate(4.0, 4.0),
        name: "Clearing".into(),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    });
    project.operations.insert(
        0,
        Operation::Pocket(PocketOp {
            targets: vec![InstanceId::new("i9")],
            depth_mm: Some(depth_mm),
            ..dc_project::test_support::sample_pocket()
        }),
    );
    project
}

/// A Photo V-Carve of a bitmap the project does not hold is refused.
#[test]
fn unknown_bitmap_is_an_error() {
    let op = PhotoVCarveOp {
        image: BitmapId::new("b9"),
        ..acceptance_op()
    };
    let project = gradient_project(op.clone());
    assert_eq!(
        generate_photo(&project, &op).unwrap_err(),
        CamError::PhotoBitmapUnknown {
            op: OperationId::new("o8"),
            bitmap: BitmapId::new("b9"),
        }
    );
}

/// Determinism: the same project twice gives the same moves and
/// warnings, raster and hatch.
#[test]
fn same_input_twice_is_identical() {
    for strategy in [PhotoStrategy::Raster, PhotoStrategy::Hatch] {
        let project = gradient_project(PhotoVCarveOp {
            strategy,
            line_spacing_fraction: 0.95,
            ..acceptance_op()
        });
        assert_eq!(generate(&project), generate(&project));
    }
}

/// Security review L3: a step along the groove or a spacing of 1e-9 in a
/// shared file is refused before anything is allocated, naming the field
/// and what to change.
#[test]
fn move_budget_refuses_a_microscopic_sample_step() {
    for (field, op) in [
        (
            "sample_mm",
            PhotoVCarveOp {
                sample_mm: 1e-9,
                ..acceptance_op()
            },
        ),
        (
            "line_spacing_fraction",
            PhotoVCarveOp {
                line_spacing_fraction: 1e-9,
                ..acceptance_op()
            },
        ),
    ] {
        let project = gradient_project(op.clone());
        let started = std::time::Instant::now();
        let err = generate_photo(&project, &op).unwrap_err();
        assert!(started.elapsed().as_secs_f64() < 1.0, "{field}: it ran");
        assert!(
            matches!(&err, CamError::PhotoSetting { field: f, problem: PhotoProblem::TooManyMoves { limit, .. }, .. }
                if *f == field && *limit == MAX_PHOTO_MOVES),
            "{field}: {err:?}"
        );
        assert!(
            err.to_string()
                .contains("raise sample_mm or line_spacing_fraction"),
            "{err}"
        );
    }
    let footprint = Rect {
        min: Point::new(0.0, 0.0),
        max: Point::new(100.0, 50.0),
    };
    let op = OperationId::new("o8");
    assert!(matches!(
        grooves(&op, footprint, 22.5, 1.0, 1e-9),
        Err(CamError::PhotoSetting { .. })
    ));
    assert!(matches!(
        grooves(&op, footprint, 22.5, 1e-9, 0.25),
        Err(CamError::PhotoSetting { .. })
    ));
    let family = grooves(&op, footprint, 22.5, 1.0, 0.25).expect("a family");
    assert!((family.spacing_mm - 1.0).abs() < 1e-12);
    assert!((family.direction.x - 22.5f64.to_radians().cos()).abs() < 1e-12);
}

/// Security review L1: when the user vouches for a start plane nothing in
/// the project cleared, the links run no lower than the stock top plus
/// the entry clearance, not `stepover_retract_mm` over the start plane
/// through material the project cannot see.
#[test]
fn link_height_clears_the_stock_top_when_the_start_plane_is_only_vouched_for() {
    let op = PhotoVCarveOp {
        start_depth_mm: 3.0,
        start_plane_cleared_externally: true,
        ..acceptance_op()
    };
    let (toolpath, _) = generate(&gradient_project(op));
    let plane = TOP - 3.0;
    let grooves = grooves_of(&toolpath, plane);
    assert!(grooves.len() > 50);
    let links: Vec<&Move> = toolpath
        .moves
        .windows(2)
        .filter(|w| {
            w[1].kind == MoveKind::Linear && w[0].to.xy().distance_to(w[1].to.xy()) > EPS_MM
        })
        .map(|w| &w[1])
        .filter(|m| m.to.z > plane + EPS_MM)
        .collect();
    assert!(!links.is_empty());
    for link in links {
        assert!(
            (link.to.z - (TOP + 1.0)).abs() <= EPS_MM,
            "a link at {link:?}"
        );
    }
}

/// Security review M2: a start depth no Pocket clears and the user does
/// not vouch for is refused, as validation refuses it.
#[test]
fn uncleared_start_depth_is_refused() {
    let op = PhotoVCarveOp {
        start_depth_mm: 1.0,
        ..acceptance_op()
    };
    let project = gradient_project(op.clone());
    assert_eq!(
        generate_photo(&project, &op).unwrap_err(),
        CamError::StartDepthUncleared {
            op: OperationId::new("o8"),
            start_depth_mm: 1.0,
        }
    );
}

/// Every setting validation checks is re-checked, as every generator
/// re-checks validation: none is defaulted or clamped.
#[test]
fn settings_are_checked_again() {
    let cases: Vec<(&str, PhotoVCarveOp)> = vec![
        (
            "max_depth_mm",
            PhotoVCarveOp {
                max_depth_mm: None,
                ..acceptance_op()
            },
        ),
        (
            "max_depth_mm",
            PhotoVCarveOp {
                max_depth_mm: Some(0.0),
                ..acceptance_op()
            },
        ),
        (
            "line_angle_deg",
            PhotoVCarveOp {
                line_angle_deg: 180.0,
                ..acceptance_op()
            },
        ),
        (
            "line_angle_deg",
            PhotoVCarveOp {
                line_angle_deg: -1.0,
                ..acceptance_op()
            },
        ),
        (
            "line_spacing_fraction",
            PhotoVCarveOp {
                line_spacing_fraction: 0.0,
                ..acceptance_op()
            },
        ),
        (
            "sample_mm",
            PhotoVCarveOp {
                sample_mm: f64::NAN,
                ..acceptance_op()
            },
        ),
        (
            "stepover_retract_mm",
            PhotoVCarveOp {
                stepover_retract_mm: 0.0,
                ..acceptance_op()
            },
        ),
    ];
    for (field, op) in cases {
        let err = generate_photo(&gradient_project(op.clone()), &op).unwrap_err();
        assert!(
            matches!(&err, CamError::PhotoSetting { field: f, .. } if *f == field),
            "{field}: {err:?}"
        );
    }
    // The tool must be a V-bit, and have its rates.
    let mut project = gradient_project(acceptance_op());
    project.tools[1].cutter = ToolKind::EndMill;
    assert!(matches!(
        generate_photo(&project, &acceptance_op()),
        Err(CamError::VCarveNeedsVBit { .. })
    ));
    let mut project = gradient_project(acceptance_op());
    project.tools[1].plunge_mm_min = None;
    assert!(matches!(
        generate_photo(&project, &acceptance_op()),
        Err(CamError::MissingFeed { .. })
    ));
}

/// A stored picture whose data is corrupt behind a good header loads and
/// validates, and fails here with the bitmap's id and a typed reason.
#[test]
fn undecodable_picture_is_a_typed_error() {
    let mut project = gradient_project(acceptance_op());
    let text = &mut project.bitmaps[0].png_base64;
    let cut = text.len() / 2;
    text.truncate(cut - cut % 4);
    let err = generate_photo(&project, &acceptance_op()).unwrap_err();
    assert!(
        matches!(&err, CamError::BitmapDecode { bitmap, .. } if bitmap.as_str() == "b1"),
        "{err:?}"
    );
}

/// The summary `check` prints: the footprint, the groove count and the
/// full-depth width, read without sampling the picture.
#[test]
fn layout_counts_the_grooves() {
    let project = gradient_project(acceptance_op());
    let (toolpath, _) = generate(&project);
    let grooves = grooves_of(&toolpath, TOP).len();
    let layout = layout(&project, &acceptance_op()).expect("a layout");
    assert_eq!(layout.grooves, grooves);
    assert!((layout.width_mm - full_width_mm()).abs() < 1e-12);
    assert_eq!(layout.footprint.min, Point::new(X0, Y0));
    assert_eq!(layout.footprint.max, Point::new(X0 + W, Y0 + H));
    let hatch = PhotoVCarveOp {
        strategy: PhotoStrategy::Hatch,
        ..acceptance_op()
    };
    let project = gradient_project(hatch.clone());
    let all = grooves_of(&generate(&project).0, TOP).len();
    assert_eq!(layout_of(&project, &hatch), all);
}

fn layout_of(project: &Project, op: &PhotoVCarveOp) -> usize {
    layout(project, op).expect("a layout").grooves
}
