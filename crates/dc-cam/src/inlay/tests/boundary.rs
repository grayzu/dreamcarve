// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #603 (slice 21j): the drawn band in the generator
//! (`docs/specs/phase-3/design.md`, "Drawn boundary"; functional 49f,
//! acceptance 61l, 61m, 61o, 61p).
//!
//! The star of the inlay test part is centred at (50, 50) on 100 × 100 mm
//! stock, 60 mm tip to tip, so an 80 mm square boundary centred on it is
//! 10 mm from each tip. The boundary shapes are further instances of the
//! project, listed by id.

use super::board::{diameter_of, off_stock};
use super::*;
use crate::stock::StockEdge;
use dc_geometry::{area_mm2, difference_within};

/// A square from `low` to `high` on both axes, mm.
fn square(low_mm: f64, high_mm: f64) -> Polyline {
    Polyline::rect(
        Point::new(low_mm, low_mm),
        high_mm - low_mm,
        high_mm - low_mm,
    )
}

/// An ellipse `half_x_mm` × `half_y_mm` round `centre`, as 128 chords.
fn oval(centre: (f64, f64), half_x_mm: f64, half_y_mm: f64) -> Polyline {
    let n = 128;
    polygon(
        &(0..n)
            .map(|k| {
                let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
                (
                    centre.0 + half_x_mm * a.cos(),
                    centre.1 + half_y_mm * a.sin(),
                )
            })
            .collect::<Vec<_>>(),
    )
}

/// `project` with the boundary shapes `shapes` (project space) added as
/// instances `b1`, `b2`, … and listed by `op`, which ignores the margin
/// and rounding: both unset.
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

/// The star male with the 80 mm square boundary of acceptance 61l.
fn star_square(clearing: bool) -> (Project, InlayOp) {
    let op = InlayOp {
        clearing: if clearing {
            vec![clearing_pass()]
        } else {
            Vec::new()
        },
        ..inlay(Board::Male)
    };
    with_boundary(&star_project(THICKNESS_MM), &op, vec![square(10.0, 90.0)])
}

/// The Chebyshev distance of `p` from the star's centre, mm: 40 on the
/// 80 mm square.
fn reach(p: Point) -> f64 {
    (p.x - CENTRE.0).abs().max((p.y - CENTRE.1).abs())
}

/// The start plane's Z on the male board, mm.
fn start_plane_z_mm() -> f64 {
    THICKNESS_MM - (DEPTH_MM - GLUE_GAP_MM)
}

/// The bounds of `regions`' outlines.
fn bounds(regions: &[Region]) -> Rect {
    bounds_of(regions.iter().map(Region::outer)).expect("a region")
}

/// 61l: `M` is the narrowest band, 10 mm for the star in an 80 mm square,
/// the `BandSource` is the drawn one, and neither the margin nor the
/// rounding is read.
#[test]
fn male_band_reads_the_narrowest_band_of_a_drawn_boundary() {
    let (project, op) = star_square(false);
    let band = dc_project::male_band(&project, &op).expect("a band");
    assert!((band.margin_mm - 10.0).abs() < 0.01, "{}", band.margin_mm);
    assert!(matches!(
        band.source,
        dc_project::BandSource::Drawn { narrowest_mm } if (narrowest_mm - 10.0).abs() < 0.01
    ));
    assert!(band.outline_rounding_mm.abs() < 1e-12);
    // Set, they change nothing the band reads.
    let set = InlayOp {
        margin_mm: Some(3.0),
        outline_rounding_mm: Some(20.0),
        ..op
    };
    let again = dc_project::male_band(&project, &set).expect("a band");
    assert_eq!(again, band);
}

/// 61l and the design's rules: the edge is open exactly when `M − P·s ≥
/// 2r` and the flank `(D − G + 2P)·s` stays inside `M`, on the narrowest
/// band. 10 and 5 mm open it; 4 mm does not (`4 − 1.155 < 3.175`), and
/// then names `max(P·s + 2r, (D − G + 2P)·s)` = 4.330 mm.
#[test]
fn drawn_boundary_opens_the_edge_by_the_same_rules() {
    for (low_mm, open) in [(10.0, true), (15.0, true), (16.0, false)] {
        let op = InlayOp {
            clearing: vec![clearing_pass()],
            ..inlay(Board::Male)
        };
        let (project, op) = with_boundary(
            &star_project(THICKNESS_MM),
            &op,
            vec![square(low_mm, 100.0 - low_mm)],
        );
        let band = dc_project::male_band(&project, &op).expect("a band");
        assert_eq!(band.is_open(), open, "square from {low_mm}: {band:?}");
        if !open {
            match band.edge {
                BandEdge::VWall {
                    margin_needed_mm: Some(m),
                } => assert!((m - (PLUG_DEPTH_MM * slope() + 3.175)).abs() < 1e-6, "{m}"),
                other => panic!("{other:?}"),
            }
            assert!(band.edge_rounding_mm.abs() < 1e-12);
        } else {
            assert!((band.edge_rounding_mm - 1.5875).abs() < 1e-9);
        }
    }
}

/// The generator's carve is `band_edge` less the design: the one function
/// the safety pass calls too.
#[test]
fn band_edge_is_one_function_for_the_generator_and_the_safety_pass() {
    for clearing in [false, true] {
        let (project, op) = star_square(clearing);
        let mirror = male_mirror(&project, &op.targets).expect("a mirror");
        let design = mirrored_design(&project);
        let edge = dc_project::band_edge(&project, &op, Some(mirror), &design)
            .expect("a drawn band resolves");
        // 80 × 80, mirrored about the star's own centre line: the same.
        let b = bounds(&edge);
        assert!(
            (b.min.x - 10.0).abs() < 0.01 && (b.max.x - 90.0).abs() < 0.01,
            "{b:?}"
        );
        let want = difference_within(
            &edge,
            &design,
            project.flatten_tolerance_mm,
            plug_tolerance_mm(&project, &op),
        );
        let side = side(&project, &op, Board::Male, &mut Vec::new()).expect("a side");
        assert_eq!(side.targets.regions, want, "clearing {clearing}");
    }
}

/// Without a boundary `band_edge` is `rounded_band` of the design,
/// exactly, with the margin and the edge rounding the band reads (the
/// goldens and the stress corpus are held by `cli::` and `dc-gcode`).
#[test]
fn no_boundary_is_byte_for_byte_unchanged() {
    let project = star_project(THICKNESS_MM);
    let design = mirrored_design(&project);
    for (clearing, rounding_mm) in [(false, None), (true, None), (true, Some(6.0))] {
        let op = InlayOp {
            clearing: if clearing {
                vec![clearing_pass()]
            } else {
                Vec::new()
            },
            outline_rounding_mm: rounding_mm,
            ..inlay(Board::Male)
        };
        let band = dc_project::male_band(&project, &op).expect("a band");
        let mirror = male_mirror(&project, &op.targets).expect("a mirror");
        let edge = dc_project::band_edge(&project, &op, Some(mirror), &design).expect("an edge");
        assert_eq!(
            edge,
            rounded_band(
                &design,
                MARGIN_MM,
                band.edge_rounding_mm,
                project.flatten_tolerance_mm
            ),
            "clearing {clearing}, rounding {rounding_mm:?}"
        );
    }
}

/// Acceptance 61l: with the boundary, a set `margin_mm` warns
/// `inlay_margin_ignored` (validation's, once; the generator names it
/// too) and an unset one does not; every cut's cone at the start plane
/// lies inside the square and outside the mirrored star but for the
/// circle, carved as a hole, and nothing is cut below the floor.
#[test]
fn boundary_replaces_margin_and_rounding() {
    let (project, op) = star_square(false);
    let (runs, warnings) = inlay_runs(&project, &op);
    assert!(
        !warnings
            .iter()
            .any(|w| matches!(w, Warning::InlayMarginIgnored { .. })),
        "{warnings:?}"
    );
    let set = InlayOp {
        margin_mm: Some(8.0),
        outline_rounding_mm: Some(10.0),
        ..op.clone()
    };
    let (_, warnings) = inlay_runs(&project, &set);
    let fields: Vec<&str> = warnings
        .iter()
        .filter_map(|w| match w {
            Warning::InlayMarginIgnored { field, .. } => Some(*field),
            _ => None,
        })
        .collect();
    assert_eq!(fields, ["margin_mm", "outline_rounding_mm"], "{warnings:?}");

    let design = mirrored_design(&project);
    let walls = outlines(&design);
    let floor_z_mm = male_floor_z_mm();
    for p in cut_points(main_run(&runs)) {
        assert!(p.z >= floor_z_mm - 1e-6, "{p:?} is below the floor");
        let r = (start_plane_z_mm() - p.z).max(0.0) * slope();
        assert!(
            reach(p.xy()) + r <= 40.0 + 0.03,
            "a cone at {p:?} reaches past the square"
        );
        assert!(
            distance_to(&walls, p.xy()) >= r - 0.03,
            "a cone at {p:?} reaches into the design"
        );
    }
}

/// A band of a drawn boundary is cut inside it: no cut of the male, the
/// clearing pass or the V-bit, leaves the square (the end mill's centre
/// its radius inside), with and without clearing.
#[test]
fn boundary_male_cuts_stay_inside_the_band() {
    for clearing in [false, true] {
        let (project, op) = star_square(clearing);
        let (runs, _) = inlay_runs(&project, &op);
        for run in &runs {
            let radius_mm = diameter_of(run, &project) / 2.0;
            for p in cut_samples(&run.toolpath) {
                let limit = if run.label == RunLabel::Main {
                    40.0
                } else {
                    40.0 - radius_mm
                };
                assert!(
                    reach(p.xy()) <= limit + 0.01,
                    "clearing {clearing}: {:?} cut at {p:?} is past {limit}",
                    run.label
                );
            }
        }
    }
}

/// The "DC" project of the lettered pair with its 90 × 60 mm frame (`i3`)
/// as the boundary, moved `shift_mm` right.
fn dc_with_frame(shift_mm: f64) -> (Project, InlayOp) {
    let (mut project, mut op) = fixture(include_str!("../../../../../tests/fixtures/dc.dcv"));
    op.boundary = vec![InstanceId::new("i3")];
    op.margin_mm = None;
    op.outline_rounding_mm = None;
    let frame = project
        .instances
        .iter_mut()
        .find(|i| i.id.as_str() == "i3")
        .expect("the frame");
    frame.transform = frame.transform.then(Transform::translate(shift_mm, 0.0));
    (project, op)
}

/// Acceptance 61m: the drawn frame is mirrored with the design about the
/// targets' centre line, X = 55: the band's edge is the frame mirrored.
#[test]
fn boundary_is_mirrored_with_the_design() {
    let (project, op) = dc_with_frame(3.0);
    let axis = match male_mirror(&project, &op.targets) {
        Ok(m) => m.apply(Point::new(0.0, 0.0)).x / 2.0,
        Err(e) => panic!("{e}"),
    };
    assert!((axis - 55.0).abs() < 0.01, "{axis}");
    let drawn = dc_project::drawn_boundary(&project, &op)
        .expect("resolves")
        .expect("a boundary");
    let drawn_bounds = bounds(&drawn.band);
    let mirror = male_mirror(&project, &op.targets).expect("a mirror");
    let edge = dc_project::band_edge(&project, &op, Some(mirror), &[]).expect("an edge");
    let edge_bounds = bounds(&edge);
    let tol = 0.02;
    assert!(
        (edge_bounds.min.x - (2.0 * axis - drawn_bounds.max.x)).abs() < tol
            && (edge_bounds.max.x - (2.0 * axis - drawn_bounds.min.x)).abs() < tol
            && (edge_bounds.min.y - drawn_bounds.min.y).abs() < tol
            && (edge_bounds.max.y - drawn_bounds.max.y).abs() < tol,
        "{edge_bounds:?} is not {drawn_bounds:?} mirrored about {axis}"
    );
    let (_, warnings) = inlay_runs(&project, &op);
    assert!((mirror_axis(&warnings) - 55.0).abs() < 0.01);
}

/// Acceptance 61l: with `clearing: [t2]` the end mill's outermost path at
/// the floor is the square inset by its radius, 1.5875 mm, within 0.01.
#[test]
fn boundary_edge_pass_sweeps_to_the_boundary() {
    let (project, op) = star_square(true);
    let (runs, _) = inlay_runs(&project, &op);
    let clearing = runs
        .iter()
        .find(|r| matches!(r.label, RunLabel::Clearing(_)))
        .expect("a clearing run");
    let far = cut_points(&clearing.toolpath)
        .into_iter()
        .filter(|p| (p.z - male_floor_z_mm()).abs() < 1e-6)
        .map(|p| reach(p.xy()))
        .fold(0.0, f64::max);
    assert!((far - (40.0 - 1.5875)).abs() < 0.01, "{far}");
}

/// Acceptance 61l: without clearing the square's edge is a V wall, as a
/// computed band's is: the V-bit's floor ring runs `P·s` inside it and
/// there is no clearing run.
#[test]
fn boundary_v_wall_without_clearing() {
    let (project, op) = star_square(false);
    let band = dc_project::male_band(&project, &op).expect("a band");
    assert!(!band.is_open());
    let (runs, _) = inlay_runs(&project, &op);
    assert!(runs.iter().all(|r| r.label == RunLabel::Main));
    let far = cut_points(main_run(&runs))
        .into_iter()
        .filter(|p| (p.z - male_floor_z_mm()).abs() < 1e-6)
        .map(|p| reach(p.xy()))
        .fold(0.0, f64::max);
    let want = 40.0 - PLUG_DEPTH_MM * slope();
    assert!((far - want).abs() < 0.02, "{far} is not {want}");
}

/// Acceptance 61p: the cut-out's tool-centre path is 1.5875 mm inside the
/// square, within 0.01.
#[test]
fn boundary_cut_out_hugs_the_edge() {
    let op = InlayOp {
        clearing: vec![clearing_pass()],
        ..cut_out_male()
    };
    let (project, op) = with_boundary(&star_project(THICKNESS_MM), &op, vec![square(10.0, 90.0)]);
    let (runs, _) = inlay_runs(&project, &op);
    let cut_out = runs
        .iter()
        .find(|r| matches!(r.label, RunLabel::CutOut(_)))
        .expect("a cut-out run");
    let points = cut_samples(&cut_out.toolpath);
    assert!(points.len() > 50);
    for p in points {
        assert!(
            (reach(p.xy()) - (40.0 - 1.5875)).abs() < 0.01,
            "{p:?} is not 1.5875 mm inside the square"
        );
    }
}

/// Acceptance 61p: a 68 mm square (4 mm clear of each tip) with no
/// clearing names `2 · P · tan 30° + 3.175` = 5.484 mm.
#[test]
fn boundary_cut_out_no_room_names_5_484() {
    let op = InlayOp {
        cut_out: cut_out_male().cut_out,
        ..inlay(Board::Male)
    };
    let (project, op) = with_boundary(&star_project(THICKNESS_MM), &op, vec![square(16.0, 84.0)]);
    match try_inlay(&project, &op) {
        Err(CamError::InlayCutOutNoRoom { needed_mm, .. }) => {
            assert!((needed_mm - 5.484).abs() < 0.001, "{needed_mm}");
        }
        other => panic!("{other:?}"),
    }
}

/// Acceptance 61p: a boundary 6 mm past the male board's left edge is
/// clipped to it, warns `inlay_band_clipped`, and nothing is cut past the
/// board. The boundary is mirrored with the design, so to run past the
/// male board's left edge it is drawn 6 mm past the stock's right one.
#[test]
fn boundary_clipped_to_the_board_warns_and_stays_on_it() {
    let op = InlayOp {
        clearing: vec![clearing_pass()],
        ..inlay(Board::Male)
    };
    let wide = Polyline::rect(Point::new(10.0, 10.0), 96.0, 80.0);
    let (project, op) = with_boundary(&star_project(THICKNESS_MM), &op, vec![wide]);
    let (runs, warnings) = inlay_runs(&project, &op);
    let clipped: Vec<_> = warnings
        .iter()
        .filter_map(|w| match w {
            Warning::InlayBandClipped { edges, open, .. } => Some((edges.clone(), *open)),
            _ => None,
        })
        .collect();
    assert_eq!(clipped, [(vec![StockEdge::MinX], true)], "{warnings:?}");
    for run in &runs {
        let radius_mm = diameter_of(run, &project) / 2.0;
        assert!(
            off_stock(&run.toolpath, &project, radius_mm, 0.01).is_empty(),
            "{:?} cuts past the board",
            run.label
        );
    }
}

/// The female never reads the boundary: its moves are those of the same
/// Inlay with an empty list.
#[test]
fn boundary_female_is_unchanged() {
    let project = star_project(THICKNESS_MM);
    let plain = inlay(Board::Female);
    let (with, with_op) = with_boundary(&project, &plain, vec![square(10.0, 90.0)]);
    let with_op = InlayOp {
        margin_mm: plain.margin_mm,
        ..with_op
    };
    let (a, a_warnings) = inlay_runs(&project, &plain);
    let (b, b_warnings) = inlay_runs(&with, &with_op);
    assert_eq!(a, b);
    assert_eq!(a_warnings, b_warnings);
}

/// Acceptance 61o: two stars 120 mm apart with a square each make one
/// band of two squares less the stars; a boundary that holds only one is
/// `inlay_boundary_crosses_design`; one oval round both is one band.
#[test]
fn several_boundaries_make_one_band() {
    let mut project = star_project(THICKNESS_MM);
    project.stock.width_mm = 300.0;
    project.instances[0].transform = Transform::translate(90.0, 50.0);
    project.instances[0].name = "First".into();
    project.instances.push(FeatureInstance {
        id: InstanceId::new("i2"),
        transform: Transform::translate(210.0, 50.0),
        name: "Second".into(),
        ..project.instances[0].clone()
    });
    let op = InlayOp {
        targets: vec![InstanceId::new("i1"), InstanceId::new("i2")],
        ..inlay(Board::Male)
    };
    let squares = vec![
        Polyline::rect(Point::new(50.0, 10.0), 80.0, 80.0),
        Polyline::rect(Point::new(170.0, 10.0), 80.0, 80.0),
    ];
    let (both, op_both) = with_boundary(&project, &op, squares.clone());
    let edge = |p: &Project, o: &InlayOp| {
        let mirror = male_mirror(p, &o.targets).expect("a mirror");
        dc_project::band_edge(p, o, Some(mirror), &[]).expect("an edge")
    };
    let band = edge(&both, &op_both);
    assert_eq!(band.len(), 2, "two squares");
    assert!(
        (area_mm2(&band) - 2.0 * 6400.0).abs() < 1.0,
        "{}",
        area_mm2(&band)
    );
    let (runs, _) = inlay_runs(&both, &op_both);
    assert!(!runs.is_empty());

    let (one, op_one) = with_boundary(&project, &op, squares[..1].to_vec());
    match try_inlay(&one, &op_one) {
        Err(CamError::InlayBoundary {
            problem: dc_project::BoundaryProblem::CrossesDesign { .. },
            ..
        }) => {}
        other => panic!("{other:?}"),
    }

    let (round, op_round) = with_boundary(&project, &op, vec![oval((150.0, 50.0), 130.0, 50.0)]);
    assert_eq!(edge(&round, &op_round).len(), 1);
    let _ = inlay_runs(&round, &op_round);
}

/// The pair seats flush within [`SEAT_MM`] for a square, an oval and a
/// contour 4 mm round the design (acceptance 61l, the walls mate).
#[test]
fn boundary_walls_mate_and_the_plug_seats_flush() {
    let project = star_project(THICKNESS_MM);
    let design = mirrored_design(&project);
    let contour = rounded_band(&design, 4.0, 0.0, project.flatten_tolerance_mm);
    let around = contour[0].outer().polyline().clone();
    let shapes = [
        ("square", square(10.0, 90.0)),
        ("oval", oval(CENTRE, 46.0, 40.0)),
        ("contour", around),
    ];
    let op = InlayOp {
        clearing: vec![clearing_pass()],
        step_over_fraction: Some(0.1),
        ..inlay(Board::Male)
    };
    for (name, shape) in shapes {
        let (p, o) = with_boundary(&project, &op, vec![shape]);
        let fit = fit_of(&p, &o, 0.2);
        assert!(
            fit.male_mm <= SEAT_MM && fit.seat_mm <= SEAT_MM,
            "{name}: the plug stands {:.3} mm proud (the male's share {:.3}) at {:?}",
            fit.seat_mm,
            fit.male_mm,
            fit.seat_at
        );
    }
}

/// A notch of the drawn outline narrower than the end mill is filled when
/// the edge is open (the closing), and left as drawn with a V wall; the
/// pair seats either way.
#[test]
fn a_notch_narrower_than_the_end_mill_is_filled_and_the_pair_seats() {
    let notched = polygon(&[
        (10.0, 10.0),
        (90.0, 10.0),
        (90.0, 90.0),
        (10.0, 90.0),
        (10.0, 51.0),
        (13.0, 51.0),
        (13.0, 49.0),
        (10.0, 49.0),
    ]);
    let in_notch = Point::new(11.5, 50.0);
    for clearing in [true, false] {
        let op = InlayOp {
            clearing: if clearing {
                vec![clearing_pass()]
            } else {
                Vec::new()
            },
            step_over_fraction: Some(0.1),
            ..inlay(Board::Male)
        };
        let (project, op) = with_boundary(&star_project(THICKNESS_MM), &op, vec![notched.clone()]);
        let mirror = male_mirror(&project, &op.targets).expect("a mirror");
        let edge = dc_project::band_edge(&project, &op, Some(mirror), &[]).expect("an edge");
        // The notch is on the drawn left edge, so on the right once
        // mirrored about X = 50.
        let mirrored_notch = Point::new(100.0 - in_notch.x, in_notch.y);
        let holds = edge.iter().any(|r| r.contains_point(mirrored_notch));
        // The slot is outside the band as drawn, which a V wall keeps; an
        // open edge's closing fills it.
        assert_eq!(holds, clearing, "clearing {clearing}");
        let fit = fit_of(&project, &op, 0.2);
        assert!(
            fit.male_mm <= SEAT_MM && fit.seat_mm <= SEAT_MM,
            "clearing {clearing}: {:.3} / {:.3} mm proud",
            fit.seat_mm,
            fit.male_mm
        );
    }
}
