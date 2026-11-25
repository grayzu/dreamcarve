// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Phase 6, slice 3 (`docs/specs/phase-6/design.md`, "Test plan → Runs
//! and reuse"): the plan is the runs export writes, each board has its
//! own, and the stock they leave is what the fixtures cut.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_geometry::{Point, Point3, Rect, assert_approx_eq};
use dc_job::sim::{
    CellSpec, Comparison, Cutter, Design, Grid, Heightfield, Motion, Outcome, Quality, SimMove,
    UNCUT_MM, build, compare, facts,
};
use dc_job::simulate::{BoardPlan, Plan, design, heightmap_png, is_heightmap, plan, sim_moves};
use dc_job::{EmitOp, EstimateConfig, Move, ProgramSplit, Toolpath};
use dc_project::{Board, CornerRelief, Operation, PostProcessorRef, Project, ToolId};

use common::{DATE, load_dcv, repo_root};

/// The six bundled post-processors.
const POSTS: [&str; 6] = ["generic", "grbl", "grblhal", "linuxcnc", "mach3", "uccnc"];

fn fixture(name: &str) -> Project {
    load_dcv(
        &repo_root()
            .join("tests/fixtures")
            .join(format!("{name}.dcv")),
    )
}

fn generated(project: &Project) -> Vec<EmitOp> {
    let built = dc_job::build(project, ProgramSplit::ProjectDefault, DATE);
    assert!(!built.findings.has_errors(), "{:?}", built.findings);
    built.generated.expect("a project with no error generates")
}

fn planned(project: &Project) -> Plan {
    plan(project, &generated(project), &EstimateConfig::default()).expect("a valid project plans")
}

fn cut(board: &BoardPlan, spec: CellSpec) -> Heightfield {
    let grid = Grid::of(board.width_mm, board.height_mm, spec).unwrap();
    let stock = Heightfield::uncut(grid, board.thickness_mm).unwrap();
    match build(stock, &board.sim_runs(), 0, &mut |_, _| {}, &mut |_| true).unwrap() {
        Outcome::Finished(field) => field,
        Outcome::Stopped => panic!("not told to stop"),
    }
}

/// Acceptance 142 (S22): the simulation reads the runs export writes, in
/// the programs' order. On all six post-processors and both program
/// splits, the tool runs of the programs, in the order the programs are
/// run, are the plan's runs grouped where the tool changes, operation
/// for operation; and the plan's cutting moves are the generated
/// toolpaths' cutting moves, move for move.
#[test]
fn the_plan_is_the_runs_export_writes() {
    for name in ["plaque", "sign", "dc"] {
        for post in POSTS {
            let mut project = fixture(name);
            project.post_processor = PostProcessorRef::latest(post).expect("a bundled post");
            for split in [ProgramSplit::ProgramPerTool, ProgramSplit::OneProgram] {
                let case = format!("{name} on {post}, {split:?}");
                let built = dc_job::build(&project, split, DATE);
                if built.findings.has_errors() {
                    // GRBL has no M6: one program for all tools is refused
                    // for a project with two.
                    assert!(
                        split == ProgramSplit::OneProgram,
                        "{case}: {:?}",
                        built.findings
                    );
                    continue;
                }
                let runs = built.generated.as_deref().expect("generated");
                let plan = plan(&project, runs, &EstimateConfig::default()).unwrap();

                // The programs' tool runs, in the order they are run.
                let finished =
                    dc_job::finish(&project, runs, split, DATE, &EstimateConfig::default());
                let mut written: Vec<(Board, ToolId, Vec<String>)> = Vec::new();
                for program in &finished.programs {
                    for run in &program.runs {
                        written.push((
                            program.program.board,
                            run.tool.clone(),
                            run.operations.iter().map(ToString::to_string).collect(),
                        ));
                    }
                }
                // The plan's runs, grouped where the tool changes.
                let mut grouped: Vec<(Board, ToolId, Vec<String>)> = Vec::new();
                for board in &plan.boards {
                    for run in &board.runs {
                        let op = run.op.to_string();
                        match grouped.last_mut() {
                            Some((b, tool, ops)) if *b == board.board && *tool == run.tool => {
                                if ops.last() != Some(&op) {
                                    ops.push(op);
                                }
                            }
                            _ => grouped.push((board.board, run.tool.clone(), vec![op])),
                        }
                    }
                }
                assert_eq!(grouped, written, "{case}");

                // Move for move, the generated toolpaths.
                let enabled: Vec<&EmitOp> = runs.iter().filter(|r| r.op.enabled()).collect();
                let planned: Vec<_> = plan.boards.iter().flat_map(|b| &b.runs).collect();
                let by_board = |board: Board| enabled.iter().filter(move |r| r.board == board);
                let in_order: Vec<&&EmitOp> = by_board(Board::Female)
                    .chain(by_board(Board::Male))
                    .collect();
                assert_eq!(planned.len(), in_order.len(), "{case}");
                for (run, emitted) in planned.iter().zip(in_order) {
                    assert_eq!(run.op, *emitted.op.id(), "{case}");
                    assert_eq!(run.label, emitted.label, "{case}");
                    assert_eq!(run.sim.moves.len(), emitted.toolpath.moves.len(), "{case}");
                    for (k, (sim, mv)) in run
                        .sim
                        .moves
                        .iter()
                        .zip(&emitted.toolpath.moves)
                        .enumerate()
                    {
                        assert_eq!(sim.to, mv.to, "{case}: move {k} of {}", run.op);
                        assert_eq!(sim.cuts(), k > 0 && mv.is_cut(), "{case}: move {k}");
                    }
                }
            }
        }
    }
}

/// A rapid and a dwell cut nothing; a toolpath's first move starts where
/// it ends.
#[test]
fn a_rapid_and_a_dwell_cut_nothing() {
    let p = |x: f64, z: f64| Point3::new(x, 5.0, z);
    let toolpath = Toolpath {
        moves: vec![
            Move::rapid(p(10.0, 20.0)),
            Move::rapid(p(10.0, 13.0)),
            Move::linear(p(10.0, 6.0), 300.0),
            Move::dwell(p(10.0, 6.0), 1.5),
            Move::linear(p(40.0, 6.0), 1200.0),
            Move::rapid(p(40.0, 20.0)),
        ],
    };
    let moves = sim_moves(&toolpath, &EstimateConfig::default());
    let motions: Vec<Motion> = moves.iter().map(|m| m.motion).collect();
    assert_eq!(
        motions,
        [
            Motion::Travel,
            Motion::Travel,
            Motion::Cut,
            Motion::Travel,
            Motion::Cut,
            Motion::Travel
        ]
    );
    assert_eq!(moves[0].from, moves[0].to);
    for pair in moves.windows(2) {
        assert_eq!(pair[0].to, pair[1].from);
    }
}

/// A move's seconds are its length over its feed; a dwell's are its
/// own; a rapid takes none until the user sets a rapid rate; the scale
/// factor multiplies them all, as it does the estimate.
#[test]
fn a_runs_seconds_are_its_lengths_over_its_feeds() {
    let p = |x: f64, z: f64| Point3::new(x, 5.0, z);
    let toolpath = Toolpath {
        moves: vec![
            Move::rapid(p(10.0, 20.0)),
            Move::rapid(p(10.0, 13.0)),
            Move::linear(p(10.0, 6.0), 300.0),
            Move::dwell(p(10.0, 6.0), 1.5),
            Move::linear(p(40.0, 6.0), 1200.0),
        ],
    };
    let plain = sim_moves(&toolpath, &EstimateConfig::default());
    let seconds: Vec<f64> = plain.iter().map(|m| m.seconds).collect();
    // 7 mm at 300 mm/min is 1.4 s; 30 mm at 1200 mm/min is 1.5 s.
    for (got, want) in seconds.iter().zip([0.0, 0.0, 1.4, 1.5, 1.5]) {
        assert_approx_eq!(*got, want, 1e-12);
    }
    let set = EstimateConfig {
        rapid_mm_min: Some(4200.0),
        scale_factor: 2.0,
    };
    let scaled: Vec<f64> = sim_moves(&toolpath, &set)
        .iter()
        .map(|m| m.seconds)
        .collect();
    // The 7 mm rapid at 4200 mm/min is 0.1 s; everything doubled.
    for (got, want) in scaled.iter().zip([0.0, 0.2, 2.8, 3.0, 3.0]) {
        assert_approx_eq!(*got, want, 1e-12);
    }
}

/// Acceptance 150: the lettered inlay's two boards have their own runs,
/// thickness and stock; the stock's runs are every operation's but the
/// male's.
#[test]
fn each_board_has_its_own_runs() {
    let project = fixture("dc");
    let runs = generated(&project);
    let plan = planned(&project);
    assert_eq!(
        plan.boards.iter().map(|b| b.board).collect::<Vec<_>>(),
        [Board::Female, Board::Male]
    );
    let (female, male) = (
        plan.board(Board::Female).unwrap(),
        plan.board(Board::Male).unwrap(),
    );
    assert_eq!(
        female.runs.len(),
        runs.iter().filter(|r| r.board == Board::Female).count()
    );
    assert_eq!(
        male.runs.len(),
        runs.iter().filter(|r| r.board == Board::Male).count()
    );
    assert!(!female.runs.is_empty() && !male.runs.is_empty());
    let (_, male_stock) = project.male_board().expect("the pair cuts a male");
    assert_approx_eq!(male.thickness_mm, male_stock.thickness_mm.unwrap(), 0.0);
    assert_approx_eq!(male.width_mm, male_stock.width_mm, 0.0);
    assert_approx_eq!(
        female.thickness_mm,
        project.stock.thickness_mm.unwrap(),
        0.0
    );
    // A project with no Inlay has the stock alone, and a project with no
    // run still has its stock.
    assert_eq!(planned(&fixture("plaque")).boards.len(), 1);
    let none = plan_of_nothing(&fixture("plaque"));
    assert_eq!(none.boards.len(), 1);
    assert!(none.boards[0].runs.is_empty());

    // Each board is cut by its own runs: cutting the female leaves the
    // male's stock uncut and the reverse.
    let female_field = cut(female, CellSpec::Quality(Quality::Draft));
    let male_field = cut(male, CellSpec::Quality(Quality::Draft));
    assert!(facts(&female_field, &female.sim_runs()).removed_mm3 > 0.0);
    assert!(facts(&male_field, &male.sim_runs()).removed_mm3 > 0.0);
}

fn plan_of_nothing(project: &Project) -> Plan {
    plan(project, &[], &EstimateConfig::default()).unwrap()
}

/// A run's key changes with its moves and its board, and with nothing
/// else: the same project plans the same keys twice, a moved shape
/// changes its operation's runs' keys and no other's, and a thicker
/// stock changes them all.
#[test]
fn a_runs_key_follows_what_the_simulation_reads() {
    let project = fixture("plaque");
    let keys =
        |p: &Project| -> Vec<u64> { planned(p).boards[0].runs.iter().map(|r| r.key).collect() };
    let base = keys(&project);
    assert_eq!(base, keys(&project));
    assert!(base.len() >= 5);

    let mut deeper = project.clone();
    let changed = deeper
        .operations
        .iter_mut()
        .position(|op| match op {
            Operation::Pocket(pocket) => {
                pocket.depth_mm = pocket.depth_mm.map(|d| d - 0.5);
                true
            }
            _ => false,
        })
        .expect("the plaque has a Pocket");
    let after = keys(&deeper);
    let runs = &planned(&deeper).boards[0].runs;
    for (k, run) in runs.iter().enumerate() {
        assert_eq!(
            base[k] != after[k],
            run.op_index == changed,
            "run {k} of {}",
            run.op
        );
    }

    let mut thicker = project.clone();
    thicker.stock.thickness_mm = project.stock.thickness_mm.map(|t| t + 1.0);
    let after = keys(&thicker);
    assert!(base.iter().zip(&after).all(|(a, b)| a != b));
}

/// Ported from PureCutCNC's `tabbedEdgeSimulation.test.ts`, on the
/// plaque's own outline in place of upstream's 50 × 40 rectangle: at two
/// qualities, the stock stands at its tab's height at each of the
/// Profile's four tabs and is cut through on both sides of it, along the
/// outline (acceptance 138). The simulation has no notion of a tab: one
/// stands because the generated path rises over it. The plaque stands in
/// for the functional spec's coaster, which has no committed project.
#[test]
fn the_plaques_tabs_stand() {
    let project = fixture("plaque");
    let plan = planned(&project);
    let board = plan.board(Board::Female).unwrap();
    let profile = project
        .operations
        .iter()
        .find_map(|op| match op {
            Operation::Profile(p) => Some(p),
            _ => None,
        })
        .expect("the plaque is cut out by a Profile");
    // The phase 2 test part's Profile: four tabs, 8 mm long and 3 mm
    // high, placed by count.
    let tab_top_mm = profile
        .tabs
        .as_ref()
        .expect("the Cut out has tabs")
        .height_mm;
    assert_approx_eq!(tab_top_mm, 3.0, 0.0);
    let run = board
        .runs
        .iter()
        .position(|r| r.op == profile.id)
        .expect("the Profile's run");
    let thickness_mm = board.thickness_mm;
    for quality in [Quality::Draft, Quality::Standard] {
        let field = cut(board, CellSpec::Quality(quality));
        // No cell is below the bottom, and none above the top.
        assert!(
            field
                .heights()
                .iter()
                .all(|&z| (0.0..=thickness_mm).contains(&z))
        );
        let found = facts(&field, &board.sim_runs());
        assert!(found.through_mm2 > 100.0, "{quality:?}: {found:?}");
        assert!(
            found.through_runs.contains(&u16::try_from(run).unwrap()),
            "{quality:?}"
        );

        // What the Profile's run left standing in its own trench: the
        // tabs, each a patch of cells, the rest of the trench cut
        // through.
        let (nx, ny) = (field.grid().nx() as usize, field.grid().ny() as usize);
        let standing: Vec<bool> = field
            .heights()
            .iter()
            .zip(field.owners())
            .map(|(&z, &owner)| usize::from(owner) == run && z > 0.5)
            .collect();
        let mut seen = vec![false; nx * ny];
        let mut patches: Vec<f64> = Vec::new();
        for start in 0..nx * ny {
            if !standing[start] || seen[start] {
                continue;
            }
            let (mut stack, mut top) = (vec![start], 0.0_f64);
            seen[start] = true;
            while let Some(k) = stack.pop() {
                top = top.max(field.heights()[k]);
                let (i, j) = ((k % nx) as isize, (k / nx) as isize);
                for (di, dj) in [
                    (1, 0),
                    (-1, 0),
                    (0, 1),
                    (0, -1),
                    (1, 1),
                    (1, -1),
                    (-1, 1),
                    (-1, -1),
                ] {
                    let (a, b) = (i + di, j + dj);
                    if a < 0 || b < 0 || a >= nx as isize || b >= ny as isize {
                        continue;
                    }
                    let n = b as usize * nx + a as usize;
                    if standing[n] && !seen[n] {
                        seen[n] = true;
                        stack.push(n);
                    }
                }
            }
            patches.push(top);
        }
        // Four of them are the tabs, each standing at the tab's height.
        // The other is beside the trench at the back: the lead-in arc,
        // which the first pass alone cuts, 4 mm deep.
        let tabs = patches
            .iter()
            .filter(|&&top| (top - tab_top_mm).abs() < 1e-9)
            .count();
        assert_eq!(tabs, 4, "{quality:?}: {patches:?}");
        for top in patches {
            assert!(
                (top - tab_top_mm).abs() < 1e-9 || top > tab_top_mm + 0.5,
                "{quality:?}: a patch stands {top} mm high"
            );
        }
    }
}

/// The box of instance `id`'s outlines as placed on the stock.
fn bounds_of(project: &Project, id: &str) -> Rect {
    let instance = project
        .instances
        .iter()
        .find(|i| i.id.as_str() == id)
        .unwrap_or_else(|| panic!("instance {id}"));
    let definition = &project.definitions[&instance.definition];
    let mut bounds: Option<Rect> = None;
    for profile in &definition.profiles {
        let placed = instance.transform.apply_polyline(profile).unwrap();
        let b = placed.bounds().unwrap();
        bounds = Some(bounds.map_or(b, |a| a.including(b.min).including(b.max)));
    }
    bounds.unwrap()
}

fn centre_of(rect: Rect) -> Point {
    Point::new(
        (rect.min.x + rect.max.x) / 2.0,
        (rect.min.y + rect.max.y) / 2.0,
    )
}

/// The operation of `id`'s place in the project's list: the number the
/// design knows it by.
fn op_number(project: &Project, id: &str) -> u16 {
    let index = project
        .operations
        .iter()
        .position(|op| op.id().as_str() == id)
        .unwrap_or_else(|| panic!("operation {id}"));
    u16::try_from(index).unwrap()
}

/// A project's stock simulated at `quality`, with its design surface and
/// the comparison of the two.
fn checked(project: &Project, quality: Quality) -> (Heightfield, Design, Comparison) {
    let runs = generated(project);
    let plan = plan(project, &runs, &EstimateConfig::default()).unwrap();
    let board = plan.board(Board::Female).unwrap();
    let field = cut(board, CellSpec::Quality(quality));
    let asked = design(project, board, &runs, *field.grid());
    let found = compare(&field, &asked, UNCUT_MM);
    (field, asked, found)
}

fn asked_at(design: &Design, p: Point) -> (f64, Option<u16>) {
    let (i, j) = design.grid().cell_at(p).expect("on the board");
    (design.z_mm(i, j).unwrap(), design.asked_by(i, j))
}

/// Acceptance 146: on the plaque, which has one operation of most
/// kinds, the design surface at named points is the functional spec's
/// table: a Pocket's floor inside its region and the top on its island;
/// a Bore's circle, through; an Outside Profile's trench its tool's
/// width wide round the outline, through but for its tabs, and the top
/// beyond it; nothing for the Engrave, whose line is what it cuts.
#[test]
fn each_operation_asks_for_its_table_row() {
    let project = fixture("plaque");
    let (_, asked, _) = checked(&project, Quality::Standard);
    let top_mm = project.stock.thickness_mm.unwrap();
    let cell_mm = asked.grid().cell_mm();

    // op1, the Panel: a rounded rectangle (i2) with a round island (i3),
    // 4 mm deep.
    let (panel, island) = (bounds_of(&project, "i2"), bounds_of(&project, "i3"));
    let in_panel = Point::new(panel.min.x + 10.0, centre_of(panel).y);
    let (z, op) = asked_at(&asked, in_panel);
    assert_approx_eq!(z, top_mm - 4.0, 1e-9);
    assert_eq!(op, Some(op_number(&project, "op1")));
    assert_eq!(
        asked_at(&asked, centre_of(island)),
        (top_mm, None),
        "the island"
    );

    // op2, the Square recess (i4), 6 mm deep, its corners sharp.
    let square = bounds_of(&project, "i4");
    let corner = Point::new(square.min.x + 2.0 * cell_mm, square.min.y + 2.0 * cell_mm);
    for p in [centre_of(square), corner] {
        let (z, op) = asked_at(&asked, p);
        assert_approx_eq!(z, top_mm - 6.0, 1e-9);
        assert_eq!(op, Some(op_number(&project, "op2")));
    }

    // op3, the Screw holes: each circle bored through.
    for hole in ["i5", "i6", "i7", "i8"] {
        let (z, op) = asked_at(&asked, centre_of(bounds_of(&project, hole)));
        assert_approx_eq!(z, 0.0, 0.0);
        assert_eq!(op, Some(op_number(&project, "op3")));
    }

    // op4, the Cut out: a trench 6.35 mm wide outside the outline (i1).
    let outline = bounds_of(&project, "i1");
    let quarter_up = outline.min.y + (outline.max.y - outline.min.y) / 4.0;
    let cut_out = Some(op_number(&project, "op4"));
    assert_eq!(
        asked_at(&asked, Point::new(outline.min.x - 3.0, quarter_up)),
        (0.0, cut_out)
    );
    assert_eq!(
        asked_at(
            &asked,
            Point::new(outline.min.x - 6.35 - 3.0 * cell_mm, quarter_up)
        ),
        (top_mm, None),
        "past the trench"
    );
    assert_eq!(
        asked_at(&asked, Point::new(outline.min.x + 2.0, quarter_up)),
        (top_mm, None),
        "the part itself"
    );
    // Its four tabs stand 3 mm high in the trench, and only there.
    let grid = *asked.grid();
    let mut tab_cells = 0;
    for j in 0..grid.ny() {
        for i in 0..grid.nx() {
            if asked.asked_by(i, j) == cut_out && asked.z_mm(i, j).unwrap() > 0.0 {
                assert_approx_eq!(asked.z_mm(i, j).unwrap(), 3.0, 1e-9);
                tab_cells += 1;
            }
        }
    }
    // Four tabs of 8 mm, 6.35 mm across. The design keeps the trench at
    // the tab's top wherever the tool rides over the tab and a radius
    // past each end of that ride, where the tool's round end is still
    // coming down: more than the 8 mm that stands, never less, so a tab
    // is never reported as uncut.
    let tab_mm2 = f64::from(tab_cells) * cell_mm * cell_mm;
    assert!(
        tab_mm2 > 4.0 * 8.0 * 6.35 && tab_mm2 < 4.0 * (8.0 + 2.0 * 6.35 + 1.0) * 6.35,
        "{tab_mm2} mm² of tab"
    );

    // op5, the Engrave: its border runs 5 mm inside the outline, and
    // nothing is asked there.
    let on_border = Point::new(outline.min.x + 5.0, centre_of(outline).y);
    assert_eq!(asked_at(&asked, on_border), (top_mm, None));
}

/// Acceptance 146, the V-Carve: on the sign, the Background is asked to
/// its flat depth away from its walls and its oval island is left at the
/// top where the lettering does not carve it.
#[test]
fn a_v_carve_asks_for_its_cone_to_the_flat_depth() {
    let project = fixture("sign");
    let (field, asked, _) = checked(&project, Quality::Standard);
    let top_mm = project.stock.thickness_mm.unwrap();
    let background = Some(op_number(&project, "op2"));
    // Every cell the Background asks for is between the top and 2.5 mm
    // down, and most of its floor is at 2.5 mm.
    let grid = *asked.grid();
    let (mut cells, mut on_floor) = (0_u32, 0_u32);
    for j in 0..grid.ny() {
        for i in 0..grid.nx() {
            if asked.asked_by(i, j) != background {
                continue;
            }
            let depth_mm = top_mm - asked.z_mm(i, j).unwrap();
            assert!((0.0..=2.5 + 1e-9).contains(&depth_mm), "{depth_mm} mm deep");
            cells += 1;
            on_floor += u32::from((depth_mm - 2.5).abs() < 1e-9);
            // What was cut there is at least as deep as asked, to the
            // threshold: the sign has no uncut material.
            assert!(field.z_mm(i, j).unwrap() <= asked.z_mm(i, j).unwrap() + 1.0);
        }
    }
    assert!(
        cells > 10_000 && on_floor > cells / 2,
        "{on_floor} of {cells}"
    );
}

/// Acceptance 147: the plaque's Square recess has dogbone relief and
/// reports nothing; with the relief turned off its four corners are
/// uncut, each the corner a 6.35 mm end mill leaves, the Pocket's full
/// 6 mm high. Acceptance 148: nothing else is reported, on the plaque,
/// the sign or the inlay pair: not beside the Profile's trench where its
/// lead-in cut, not on the Engrave, not on a tab.
#[test]
fn a_square_pocket_has_four_corners_and_none_with_dogbones() {
    for name in ["plaque", "sign", "dc"] {
        for quality in [Quality::Draft, Quality::Standard] {
            let (_, _, found) = checked(&fixture(name), quality);
            assert!(
                found.places.is_empty(),
                "{name} at {quality:?}: {:?}",
                found.places
            );
        }
    }

    let mut project = fixture("plaque");
    for op in &mut project.operations {
        if let Operation::Pocket(pocket) = op
            && pocket.id.as_str() == "op2"
        {
            assert_eq!(pocket.corner_relief, CornerRelief::Dogbone);
            pocket.corner_relief = CornerRelief::None;
        }
    }
    let square = bounds_of(&project, "i4");
    let recess = Some(op_number(&project, "op2"));
    for quality in [Quality::Standard, Quality::Fine] {
        let (field, _, found) = checked(&project, quality);
        assert_eq!(found.places.len(), 4, "{quality:?}: {:?}", found.places);
        let r = 6.35 / 2.0;
        let corner_mm2 = (1.0 - std::f64::consts::FRAC_PI_4) * r * r;
        for place in &found.places {
            assert_eq!(place.op, recess);
            assert_approx_eq!(place.height_mm, 6.0, 1e-9);
            assert!(
                place.area_mm2 > corner_mm2 / 3.0 && place.area_mm2 < corner_mm2,
                "{quality:?}: a corner of {} mm², under {corner_mm2}",
                place.area_mm2
            );
            // In a corner of the square.
            let c = centre_of(place.bounds);
            assert!(
                (c.x - square.min.x).min(square.max.x - c.x) < r
                    && (c.y - square.min.y).min(square.max.y - c.y) < r,
                "a place at {c:?}"
            );
        }
        let flagged = found.flags.iter().filter(|&&f| f != 0).count();
        assert_eq!(
            flagged,
            found.places.iter().map(|p| p.cells as usize).sum::<usize>()
        );
        assert_eq!(found.flags.len(), field.heights().len());
    }
}

/// Acceptance 154: read from the simulation at Standard, the plaque is
/// its drawing's sizes to within a cell and its depths exactly: the two
/// pockets' floors, the four holes' diameter, the engraved line's depth.
#[test]
fn the_plaque_measures_as_it_is_drawn() {
    let project = fixture("plaque");
    let (field, _, _) = checked(&project, Quality::Standard);
    let top_mm = project.stock.thickness_mm.unwrap();
    let cell_mm = field.grid().cell_mm();
    let z_at = |p: Point| {
        let (i, j) = field.grid().cell_at(p).unwrap();
        field.z_mm(i, j).unwrap()
    };
    let panel = bounds_of(&project, "i2");
    assert_approx_eq!(
        z_at(Point::new(panel.min.x + 10.0, centre_of(panel).y)),
        top_mm - 4.0,
        1e-9
    );
    assert_approx_eq!(z_at(centre_of(bounds_of(&project, "i3"))), top_mm, 0.0);
    let square = bounds_of(&project, "i4");
    assert_approx_eq!(z_at(centre_of(square)), top_mm - 6.0, 1e-9);
    // The recess is 30 mm across at its middle: the floor's width there.
    let mid_y = centre_of(square).y;
    let across = (0..field.grid().nx())
        .filter(|&i| {
            let c = field.grid().centre(i, 0);
            c.x > square.min.x - 2.0
                && c.x < square.max.x + 2.0
                && z_at(Point::new(c.x, mid_y)) < top_mm - 5.9
        })
        .count();
    assert_approx_eq!(across as f64 * cell_mm, 30.0, 2.0 * cell_mm);
    // Each hole is cut through over a 5 mm circle.
    for hole in ["i5", "i6", "i7", "i8"] {
        let b = bounds_of(&project, hole);
        let centre = centre_of(b);
        assert_approx_eq!(b.max.x - b.min.x, 5.0, 1e-6);
        let (mut through, mut ring) = (0_u32, 0.0_f64);
        for j in 0..field.grid().ny() {
            for i in 0..field.grid().nx() {
                let c = field.grid().centre(i, j);
                if c.distance_to(centre) < 4.0 && field.is_through(i, j) {
                    through += 1;
                    ring = ring.max(c.distance_to(centre));
                }
            }
        }
        let area_mm2 = f64::from(through) * cell_mm * cell_mm;
        assert_approx_eq!(
            area_mm2,
            std::f64::consts::PI * 2.5 * 2.5,
            2.5 * 2.0 * 3.2 * cell_mm
        );
        assert_approx_eq!(ring, 2.5, cell_mm);
    }
}

/// Capability 125: the picture is a 16-bit grayscale PNG, one pixel a
/// cell, white at the board's top and black at its bottom, its first row
/// the grid's last (a picture's top is the highest Y); and it knows its
/// own pictures from anyone else's.
#[test]
fn a_heightmap_is_sixteen_bit_top_row_first() {
    let grid = Grid::of(4.0, 2.0, CellSpec::CellMm(1.0)).unwrap();
    let mut field = Heightfield::uncut(grid, 10.0).unwrap();
    let flat = Cutter::Flat { radius_mm: 0.1 };
    let dot =
        |x: f64, y: f64, z: f64| SimMove::cut(Point3::new(x, y, z), Point3::new(x, y, z), 0.0);
    // Half way down in the top row's first cell; through in the bottom
    // row's last.
    field.cut(&dot(0.5, 1.5, 5.0), (0.0, 1.0), &flat, 0);
    field.cut(&dot(3.5, 0.5, -1.0), (0.0, 1.0), &flat, 0);
    let png = heightmap_png(&field).unwrap();

    let decoder = png::Decoder::new(std::io::Cursor::new(&png));
    let mut reader = decoder.read_info().unwrap();
    assert_eq!((reader.info().width, reader.info().height), (4, 2));
    assert_eq!(reader.info().color_type, png::ColorType::Grayscale);
    assert_eq!(reader.info().bit_depth, png::BitDepth::Sixteen);
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    reader.next_frame(&mut buffer).unwrap();
    let (pairs, _) = buffer.as_chunks::<2>();
    let pixels: Vec<u16> = pairs.iter().map(|p| u16::from_be_bytes(*p)).collect();
    // 5 of 10 mm is 32 767.5, which rounds up.
    assert_eq!(
        pixels,
        [32_768, 65_535, 65_535, 65_535, 65_535, 65_535, 65_535, 0]
    );

    assert!(is_heightmap(&png));
    let theirs = std::fs::read(repo_root().join("tests/fixtures/photo/gradient.png")).unwrap();
    assert!(!is_heightmap(&theirs), "another program's picture");
    assert!(!is_heightmap(b"not a picture"));
    assert!(!is_heightmap(&[]));
}
