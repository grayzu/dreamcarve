// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of a carve's clearing passes (#406; `docs/specs/phase-3/design.md`,
//! "Clearing passes (dc-cam::vcarve::clearing)" and "Test plan → dc-cam —
//! vcarve (flat floor and clearing)"; functional capability 40, acceptance
//! 51).
//!
//! Acceptance 51 is acceptance 50's 30 × 10 mm rectangle (90° 6.35 mm
//! V-bit `t2`, flat depth 2 mm, a 26 × 6 mm floor) with a 3.175 mm end mill
//! `t3`; a 1.5 mm end mill `t4` is the second pass, and the sample's
//! 6.35 mm end mill `t1` is too wide for the floor.

use super::floor::{F_MM, FLOOR_Z_MM, block_cuts, distance_to_cuts, floor_cuts, with_90};
use super::*;
use crate::error::ClearingProblem;
use crate::footprint::footprint_at;
use crate::pocket::generate_pocket;
use crate::test_support::assert_run_safe;
use dc_geometry::{area_mm2, difference, floor_regions};
use dc_project::{
    ClearanceStrategy, ClearingPass, CornerRelief, CutDirection, Entry, Problem, WallPass, validate,
};

/// The floor's corners, relative to [`AT`]: the rectangle inset by
/// `F · tan 45° = 2`.
const FLOOR: (f64, f64, f64, f64) = (2.0, 2.0, 28.0, 8.0);

/// A flat end mill `id` of `diameter_mm`, every rate set, stepping down
/// 1 mm at a 0.4 step-over.
fn end_mill(id: &str, diameter_mm: f64) -> Tool {
    Tool {
        id: ToolId::new(id),
        name: format!("{diameter_mm} mm end mill"),
        diameter_mm,
        step_down_mm: Some(1.0),
        step_over_fraction: Some(0.4),
        ..dc_project::test_support::tool_6_35_upcut()
    }
}

/// A pass with `tool`, offset, climb, plunging.
fn pass(tool: &str) -> ClearingPass {
    ClearingPass {
        tool: ToolId::new(tool),
        strategy: ClearanceStrategy::Offset,
        direction: CutDirection::Climb,
        entry: Entry::Plunge,
    }
}

/// Acceptance 51: `profiles` (acceptance 50's rectangle when `None`) with
/// the end mills `t3` (3.175 mm) and `t4` (1.5 mm) added, the V-bit floor
/// at the operation's `step_over_fraction`, and `clearing`.
fn with_passes(
    profiles: Option<Vec<Polyline>>,
    step_over_fraction: Option<f64>,
    clearing: Vec<ClearingPass>,
) -> (Project, VCarveOp) {
    let (mut p, op) = with_90(
        profiles.unwrap_or_else(|| vec![rect(30.0, 10.0)]),
        step_over_fraction,
    );
    p.tools.push(end_mill("t3", 3.175));
    p.tools.push(end_mill("t4", 1.5));
    let op = VCarveOp { clearing, ..op };
    (with_op(&p, &op), op)
}

/// The generator's runs and warnings for `op` in `p`, each run checked with
/// `assert_run_safe`.
fn runs_of(p: &Project, op: &VCarveOp) -> (Vec<ToolRun>, Vec<Warning>) {
    let p = with_op(p, op);
    let (runs, warnings) = generate_vcarve(&p, op).expect("the carve generates");
    for run in &runs {
        assert_run_safe(run, &p, &Operation::VCarve(op.clone()));
    }
    (runs, warnings)
}

/// The V-bit's run of `runs`, the last.
fn main_run(runs: &[ToolRun]) -> &Toolpath {
    let run = runs.last().expect("a V-bit run");
    assert_eq!(run.label, RunLabel::Main);
    &run.toolpath
}

/// The clearing run of `runs` with `tool`.
fn clearing_run<'r>(runs: &'r [ToolRun], tool: &str) -> &'r Toolpath {
    &runs
        .iter()
        .find(|r| r.label == RunLabel::Clearing(ToolId::new(tool)))
        .unwrap_or_else(|| panic!("no clearing run with {tool}: {runs:?}"))
        .toolpath
}

/// The XY length of `cuts`.
fn length_mm(cuts: &[(Point, Point)]) -> f64 {
    cuts.iter().map(|(a, b)| a.distance_to(*b)).sum()
}

/// The XY cuts of `toolpath` at Z `z_mm`.
fn cuts_at(toolpath: &Toolpath, z_mm: f64) -> Vec<(Point, Point)> {
    xy_cuts(toolpath)
        .into_iter()
        .filter(|(a, b)| (a.z - z_mm).abs() <= 1e-6 && (b.z - z_mm).abs() <= 1e-6)
        .map(|(a, b)| (a.xy(), b.xy()))
        .collect()
}

/// The distance from `q` to the floor rectangle's boundary.
fn to_floor_boundary(q: Point) -> f64 {
    let (x0, y0, x1, y1) = FLOOR;
    let lo = at(x0, y0);
    let hi = at(x1, y1);
    (q.x - lo.x).min(hi.x - q.x).min(q.y - lo.y).min(hi.y - q.y)
}

/// The floor's corners, project space.
fn floor_corners() -> [Point; 4] {
    let (x0, y0, x1, y1) = FLOOR;
    [at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)]
}

/// Acceptance 51: the clearing run is the end mill's, at the floor's
/// depth, 2 mm below the stock top, stepping down 1 mm a pass from it (the
/// end mill steps down; the V-bit does not), and its footprint fills the
/// 26 × 6 mm floor region out to its edges and no further: every tool
/// centre at least the radius inside it, and some exactly there on each
/// side.
#[test]
fn clearing_pass_takes_the_floor_region_and_depth() {
    let (p, op) = with_passes(None, None, vec![pass("t3")]);
    let (runs, _) = runs_of(&p, &op);
    let run = runs.first().expect("a clearing run");
    assert_eq!(run.label, RunLabel::Clearing(ToolId::new("t3")));
    assert_eq!(run.tool, ToolId::new("t3"));
    let points = cut_points(&run.toolpath);
    assert_approx_eq!(min_z(&run.toolpath), FLOOR_Z_MM, 1e-9);
    assert!(
        points.iter().any(|q| (q.z - (TOP_MM - 1.0)).abs() <= 1e-9),
        "no pass at the 1 mm step-down"
    );
    let radius_mm = 3.175 / 2.0;
    let mut touches = [false; 4];
    for q in &points {
        let (x0, y0, x1, y1) = FLOOR;
        let (lo, hi) = (at(x0, y0), at(x1, y1));
        let gaps = [
            q.x - lo.x - radius_mm,
            hi.x - q.x - radius_mm,
            q.y - lo.y - radius_mm,
            hi.y - q.y - radius_mm,
        ];
        for (side, gap) in gaps.into_iter().enumerate() {
            assert!(gap >= -1e-6, "{q:?} reaches past the floor by {}", -gap);
            if gap <= 1e-6 {
                touches[side] = true;
            }
        }
    }
    assert_eq!(
        touches, [true; 4],
        "the run reaches every side of the floor"
    );
}

/// Design "Clearing passes", step 4: one run per pass in list order, each
/// with its own end mill, then the V-bit's.
#[test]
fn clearing_runs_come_before_the_v_bit_run_with_their_own_tools() {
    let (p, op) = with_passes(None, None, vec![pass("t3"), pass("t4")]);
    let (runs, warnings) = runs_of(&p, &op);
    assert!(warnings.is_empty(), "{warnings:?}");
    let labels: Vec<(RunLabel, ToolId)> = runs
        .iter()
        .map(|r| (r.label.clone(), r.tool.clone()))
        .collect();
    assert_eq!(
        labels,
        vec![
            (RunLabel::Clearing(ToolId::new("t3")), ToolId::new("t3")),
            (RunLabel::Clearing(ToolId::new("t4")), ToolId::new("t4")),
            (RunLabel::Main, ToolId::new("t2")),
        ]
    );
    for run in &runs {
        assert!(!run.toolpath.moves.is_empty(), "{:?} is empty", run.label);
    }
}

/// The V-bit's floor of `op` in `p` as its blocks give it, the clearing
/// passes' reach taken off: the cuts of the blocks with a cleared area,
/// its rings and boundary trace (medial runs have none), each `(from, to)`
/// in XY.
fn v_bit_floor(p: &Project, op: &VCarveOp) -> Vec<(Point, Point)> {
    let p = with_op(p, op);
    let tool = p
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .expect("the bit")
        .clone();
    let targets = carve_regions(&p, &op.id, &op.targets, &mut Vec::new()).expect("targets");
    let carve = Carve {
        regions: targets.regions,
        shapes: targets.shapes,
        start_plane_z_mm: TOP_MM,
        engaged_from_z_mm: TOP_MM,
        flat_depth_mm: op.flat_depth_mm,
        strategy: &VCarveStrategy::Medial,
        step_over_fraction: op.step_over_fraction.or(tool.step_over_fraction),
        cleared_by: op
            .clearing
            .iter()
            .filter_map(|pass| p.tools.iter().find(|t| t.id == pass.tool))
            .map(|t| Clearing {
                radius_mm: t.diameter_mm / 2.0,
            })
            .collect(),
    };
    blocks(&p, &op.id, &tool, &carve, &mut Vec::new())
        .expect("blocks")
        .iter()
        .filter(|b| b.cleared.is_some())
        .flat_map(block_cuts)
        .filter(|(a, b)| a.xy().distance_to(b.xy()) > EPS_MM)
        .map(|(a, b)| (a.xy(), b.xy()))
        .collect()
}

/// Acceptance 51: with the end mill the V-bit's floor (its rings and
/// boundary trace at Z −2) lies only where the end mill could not reach,
/// within its 1.5875 mm radius of the floor boundary (the trace and the
/// four corners), and is under a third of its length without the pass (at
/// the recommended `step_over_fraction` 0.1; at 0.4 the trace alone is
/// 40 % of it). The medial chains from the corners keep their stretch at
/// Z −2 to the junction either way (design step 6): they are not the
/// floor's.
#[test]
fn floor_cutting_shrinks_to_what_the_end_mill_cannot_reach() {
    let (p, op) = with_passes(None, Some(0.1), vec![pass("t3")]);
    runs_of(&p, &op);
    let floor = v_bit_floor(&p, &op);
    assert!(!floor.is_empty(), "the V-bit still traces the floor");
    for &(a, b) in &floor {
        for k in 0..=10 {
            let t = f64::from(k) / 10.0;
            let q = Point::new(a.x + (b.x - a.x) * t, a.y + (b.y - a.y) * t);
            let d = to_floor_boundary(q);
            assert!(
                d <= 1.5875 + 1e-6,
                "the V-bit cuts the floor at {q:?}, {d} mm inside it"
            );
        }
    }
    let alone = VCarveOp {
        clearing: Vec::new(),
        ..op.clone()
    };
    let before_mm = length_mm(&v_bit_floor(&p, &alone));
    let after_mm = length_mm(&floor);
    assert!(
        after_mm < before_mm / 3.0,
        "{after_mm} mm at the floor with the pass, {before_mm} mm without"
    );
}

/// Acceptance 51: the end mill's footprint at Z −2 and the V-bit's 4 mm
/// wide one there together still cover the floor within 0.05 mm.
#[test]
fn floor_coverage_with_the_pass_still_reaches_the_floor() {
    let (p, op) = with_passes(None, None, vec![pass("t3")]);
    let (runs, _) = runs_of(&p, &op);
    let v_bit = floor_cuts(main_run(&runs));
    let end_mill = cuts_at(clearing_run(&runs, "t3"), FLOOR_Z_MM);
    let (x0, y0, ..) = FLOOR;
    for i in 0..=260 {
        for j in 0..=60 {
            let q = at(x0 + f64::from(i) * 0.1, y0 + f64::from(j) * 0.1);
            let by_v_bit = distance_to_cuts(&v_bit, q) <= F_MM + 0.05;
            let by_end_mill = distance_to_cuts(&end_mill, q) <= 3.175 / 2.0 + 0.05;
            assert!(by_v_bit || by_end_mill, "floor point {q:?} is not cut");
        }
    }
}

/// Acceptance 51: a 1.5 mm pass after the 3.175 mm one clears only what
/// that one left, the floor's corners: every tool centre of its run is
/// within its radius of what the first left, which lies within 1.5875 mm
/// of a corner, and it cuts a small part of what it cuts alone.
#[test]
fn second_pass_clears_only_what_the_first_left() {
    let (p, op) = with_passes(None, None, vec![pass("t3"), pass("t4")]);
    let (runs, _) = runs_of(&p, &op);
    let second = clearing_run(&runs, "t4");
    let points = cut_points(second);
    assert!(!points.is_empty(), "the second pass cuts");
    for q in &points {
        let near = floor_corners()
            .iter()
            .map(|c| c.distance_to(q.xy()))
            .fold(f64::INFINITY, f64::min);
        assert!(
            near <= 1.5875 + 0.75 + 1e-6,
            "the 1.5 mm pass cuts at {q:?}, {near} mm from a floor corner"
        );
    }
    let alone = VCarveOp {
        clearing: vec![pass("t4")],
        ..op.clone()
    };
    let (runs_alone, _) = runs_of(&p, &alone);
    let rest_mm = length_mm(&cuts_at(second, FLOOR_Z_MM));
    let full_mm = length_mm(&cuts_at(clearing_run(&runs_alone, "t4"), FLOOR_Z_MM));
    assert!(
        rest_mm < full_mm / 5.0,
        "{rest_mm} mm after the 3.175 mm pass, {full_mm} mm alone"
    );
}

/// Acceptance 51 and capability 40: the two passes the other way round warn
/// `clearing_not_descending` and the carve still generates. The 3.175 mm
/// pass after the 1.5 mm one can reach nothing that one left (a disc
/// reaches nothing a narrower disc cannot), so it also warns
/// `clearing_no_room` and has no run.
#[test]
fn wider_second_pass_warns_not_descending_and_still_runs() {
    let (p, op) = with_passes(None, None, vec![pass("t4"), pass("t3")]);
    let (runs, warnings) = runs_of(&p, &op);
    assert!(
        warnings.contains(&Warning::ClearingNotDescending {
            op: op.id.clone(),
            tool: ToolId::new("t3"),
            diameter_mm: 3.175,
            earlier_tool: ToolId::new("t4"),
            earlier_diameter_mm: 1.5,
        }),
        "{warnings:?}"
    );
    assert!(
        warnings
            .iter()
            .any(|w| w.code() == WarningCode::ClearingNoRoom),
        "{warnings:?}"
    );
    let labels: Vec<RunLabel> = runs.iter().map(|r| r.label.clone()).collect();
    assert_eq!(
        labels,
        vec![RunLabel::Clearing(ToolId::new("t4")), RunLabel::Main]
    );
    // Validation says it too.
    let codes: Vec<&str> = validate(&with_op(&p, &op))
        .iter()
        .map(Problem::code)
        .collect();
    assert!(codes.contains(&"clearing_not_descending"), "{codes:?}");
}

/// Acceptance 51: a pass on a carve with no flat depth has no floor to
/// clear: validation's `clearing_needs_flat_depth`, and the generator's
/// `ClearingInvalid`.
#[test]
fn clearing_without_a_flat_depth_is_an_error() {
    let (p, op) = with_passes(None, None, vec![pass("t3")]);
    let op = VCarveOp {
        flat_depth_mm: None,
        ..op
    };
    let p = with_op(&p, &op);
    let codes: Vec<&str> = validate(&p).iter().map(Problem::code).collect();
    assert!(codes.contains(&"clearing_needs_flat_depth"), "{codes:?}");
    match generate_vcarve(&p, &op) {
        Err(CamError::ClearingInvalid {
            op: id,
            tool,
            reason: ClearingProblem::NoFlatDepth,
        }) => {
            assert_eq!(id, op.id);
            assert_eq!(tool, ToolId::new("t3"));
        }
        other => panic!("expected ClearingInvalid, got {other:?}"),
    }
}

/// Acceptance 51: a pass naming the V-bit is `clearing_needs_end_mill`, and
/// the generator refuses it.
#[test]
fn clearing_pass_with_a_v_bit_is_an_error() {
    let (p, op) = with_passes(None, None, vec![pass("t2")]);
    let codes: Vec<&str> = validate(&p).iter().map(Problem::code).collect();
    assert!(codes.contains(&"clearing_needs_end_mill"), "{codes:?}");
    assert!(
        matches!(
            generate_vcarve(&p, &op),
            Err(CamError::ClearingInvalid {
                reason: ClearingProblem::NotEndMill { .. },
                ..
            })
        ),
        "{:?}",
        generate_vcarve(&p, &op)
    );
}

/// Acceptance 51 and capability 40: the carve never trusts a clearing run,
/// so removing the list gives the V-bit the whole floor again: one run,
/// acceptance 50's carve move for move, with more floor cutting than with
/// the pass.
#[test]
fn removing_the_list_gives_the_v_bit_the_whole_floor() {
    let (p, op) = with_passes(None, None, vec![pass("t3")]);
    let (with_pass, _) = runs_of(&p, &op);
    let removed = VCarveOp {
        clearing: Vec::new(),
        ..op.clone()
    };
    let (runs, _) = runs_of(&p, &removed);
    assert_eq!(runs.len(), 1, "{runs:?}");
    let (p50, op50) = with_90(vec![rect(30.0, 10.0)], None);
    let (plain, _) = carve(&p50, &op50);
    assert_eq!(main_run(&runs), &plain);
    assert!(
        length_mm(&floor_cuts(main_run(&runs))) > length_mm(&floor_cuts(main_run(&with_pass))),
        "the V-bit cuts more floor without the pass"
    );
}

/// Capability 40: a pass whose end mill reaches nothing (the 6.35 mm end
/// mill in the 6 mm wide floor) warns `clearing_no_room` and has no run,
/// and the V-bit then cuts the floor as without it.
#[test]
fn pass_that_reaches_nothing_warns_and_emits_no_run() {
    let (p, op) = with_passes(None, None, vec![pass("t1")]);
    let (runs, warnings) = runs_of(&p, &op);
    assert_eq!(
        warnings,
        vec![Warning::ClearingNoRoom {
            op: op.id.clone(),
            tool: "6.35 mm upcut".to_owned(),
            tool_id: ToolId::new("t1"),
            diameter_mm: 6.35,
        }]
    );
    assert_eq!(runs.len(), 1, "{runs:?}");
    let (p50, op50) = with_90(vec![rect(30.0, 10.0)], None);
    let (plain, _) = carve(&p50, &op50);
    assert_eq!(main_run(&runs), &plain);
}

/// S9 and S14: every clearing run's footprint, entries included, stays in
/// the carve's floor regions, as `floor_regions` re-derives them from the
/// targets, the V-bit's angle and the flat depth: here a square ring, whose
/// floor's island is the hole grown by 2 mm, cut with a helix and a raster
/// with ramps.
#[test]
fn clearing_run_footprint_stays_inside_the_floor() {
    let square_ring = vec![
        rect(30.0, 30.0),
        Polyline::rect(Point::new(10.0, 10.0), 10.0, 10.0),
    ];
    let helix = ClearingPass {
        entry: Entry::Helix {
            angle_deg: 3.0,
            diameter_fraction: 0.9,
        },
        ..pass("t3")
    };
    let raster = ClearingPass {
        strategy: ClearanceStrategy::Raster { angle_deg: 30.0 },
        direction: CutDirection::Conventional,
        entry: Entry::Ramp { angle_deg: 3.0 },
        ..pass("t4")
    };
    let (p, op) = with_passes(Some(square_ring), None, vec![helix, raster]);
    let (runs, _) = runs_of(&p, &op);
    let tolerance_mm = p.flatten_tolerance_mm;
    let floors = floor_regions(&regions_of(&p, &op), F_MM * slope_of(90.0), tolerance_mm);
    let boundary_mm: f64 = floors
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(|c| c.polyline().length_mm())
        .sum();
    for (tool, radius_mm) in [("t3", 3.175 / 2.0), ("t4", 0.75)] {
        let footprint = footprint_at(clearing_run(&runs, tool), radius_mm, tolerance_mm);
        assert!(!footprint.is_empty(), "{tool} cuts");
        let outside = area_mm2(&difference(&footprint, &floors, tolerance_mm));
        assert!(
            outside <= 2.0 * tolerance_mm * boundary_mm,
            "{tool}'s footprint is {outside} mm² outside the floor"
        );
    }
}

/// Design "Clearing passes", step 3: a pass is cut by the Pocket code. Its
/// run equals, move for move, a user's Pocket of the floor region drawn as
/// a shape, with the pass's tool, strategy, direction and entry, to the
/// floor's depth, wall pass last, no stock to leave, no corner relief, the
/// carve's safe Z, order and allowance; and the second pass equals a rest
/// Pocket of the same shape resting on the first.
#[test]
fn clearing_pass_is_cut_by_the_pocket_code() {
    let raster = ClearingPass {
        strategy: ClearanceStrategy::Raster { angle_deg: 30.0 },
        direction: CutDirection::Conventional,
        entry: Entry::Ramp { angle_deg: 3.0 },
        ..pass("t3")
    };
    for first in [pass("t3"), raster] {
        let second = pass("t4");
        let (p, op) = with_passes(None, None, vec![first.clone(), second.clone()]);
        let (runs, _) = runs_of(&p, &op);

        let tolerance_mm = p.flatten_tolerance_mm;
        let floors = floor_regions(&regions_of(&p, &op), F_MM * slope_of(90.0), tolerance_mm);
        let [floor] = floors.as_slice() else {
            panic!("one floor region: {floors:?}");
        };
        let mut user = p.clone();
        let drawn = FeatureDefinition {
            id: DefinitionId::new("dfloor"),
            name: "Floor".into(),
            profiles: vec![floor.outer().polyline().clone()],
        };
        user.definitions.insert(drawn.id.clone(), drawn);
        user.instances.push(FeatureInstance {
            id: InstanceId::new("ifloor"),
            definition: DefinitionId::new("dfloor"),
            transform: Transform::IDENTITY,
            name: "Floor".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(THICKNESS_MM),
            z_bottom_mm: Some(0.0),
        });
        let pocket_of = |id: &str, pass: &ClearingPass, rest_of: Option<OperationId>| PocketOp {
            id: OperationId::new(id),
            name: "Floor pocket".into(),
            enabled: true,
            targets: vec![InstanceId::new("ifloor")],
            tool: pass.tool.clone(),
            strategy: pass.strategy.clone(),
            wall_pass: WallPass::Last,
            depth_mm: Some(op.start_depth_mm + F_MM),
            step_down_mm: None,
            step_over_fraction: None,
            direction: pass.direction,
            stock_to_leave_mm: 0.0,
            rest_of,
            entry: pass.entry.clone(),
            corner_relief: CornerRelief::None,
            machining_order: op.machining_order,
            through_cut_allowance_mm: op.through_cut_allowance_mm,
            safe_z_mm: op.safe_z_mm,
        };
        let rough = pocket_of("p1", &first, None);
        let rest = pocket_of("p2", &second, Some(rough.id.clone()));
        user.operations = vec![
            Operation::Pocket(rough.clone()),
            Operation::Pocket(rest.clone()),
        ];
        let (rough_path, _) = generate_pocket(&user, &rough).expect("the user's Pocket");
        let (rest_path, _) = generate_pocket(&user, &rest).expect("the user's rest Pocket");
        assert_eq!(clearing_run(&runs, "t3"), &rough_path, "{first:?}");
        assert_eq!(clearing_run(&runs, "t4"), &rest_path, "{first:?}");
    }
}

/// The offset strategy (#407) shares the floor: with the pass its V-bit
/// floor shrinks to what the end mill cannot reach too, and every run is
/// safe, the cone test included.
#[test]
fn offset_carve_clears_with_the_pass_too() {
    let (p, op) = with_passes(None, Some(0.1), vec![pass("t3")]);
    let offset = VCarveOp {
        strategy: VCarveStrategy::Offset {
            step_over_mm: 0.5,
            direction: CutDirection::Climb,
        },
        ..op
    };
    let (runs, _) = runs_of(&p, &offset);
    assert_eq!(runs.len(), 2, "{runs:?}");
    let alone = VCarveOp {
        clearing: Vec::new(),
        ..offset.clone()
    };
    let (runs_alone, _) = runs_of(&p, &alone);
    let with_mm = length_mm(&floor_cuts(main_run(&runs)));
    let without_mm = length_mm(&floor_cuts(main_run(&runs_alone)));
    assert!(
        with_mm < without_mm / 2.0,
        "{with_mm} mm with the pass, {without_mm} mm without"
    );
}
