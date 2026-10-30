// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The golden parity harness (`docs/specs/phase-1/functional.md`,
//! acceptance test 14; `docs/specs/phase-2/functional.md`, acceptance
//! test 38; `docs/rust-guidelines.md`, "Tests").
//!
//! [`check`] generates every enabled operation of a case's DreamCarve
//! project, emits them as one program, reads it and the captured PureCutCNC
//! program back with [`parse_program`], and compares the two in the machine
//! frame they share:
//!
//! 1. **Safety (S1–S3)** on both programs: no Z below the stock bottom
//!    minus the allowance, rapids that move in XY only at or above safe Z
//!    (a Drill also rapids in Z alone down to its retract plane and inside
//!    its hole; any operation rapids straight down from safe Z to a ramp
//!    or helix 1 mm above the stock top, #214), and every cut at a
//!    positive feed that is one of the project's tool rates.
//! 2. **Levels.** The Z values of the horizontal cutting moves are equal.
//! 3. **Moves per level**, after start-point alignment (acceptance test
//!    38). Each run of horizontal cuts is split into *loops* (stretches that
//!    end where they began: a lap, a pocket ring), *links* (one move from
//!    the end of a loop to the start of the next, which moves with the
//!    loops' start points) and *open paths* (everything else: an engraved
//!    line, raster lines). Every point of every loop and open path, sampled
//!    every [`SAMPLE_STEP_MM`], lies within [`ALLOWED_MM`] (the
//!    [`TOLERANCE_MM`] of acceptance test 14 plus each program's rounding)
//!    of the other program's moves at that level, in both directions; it
//!    travels the same way as a move there; and it is cut at the same feed.
//!    Comparing sets is what aligns the start points: a loop entered
//!    somewhere else, or contours cut in another order, still match. The
//!    total length of the loops and open paths per level must agree, so a
//!    lap cut twice does not pass, and the links must have the same
//!    lengths.
//! 4. **Sloped moves** (ramps and helices, which change Z and XY together)
//!    are compared the same way in three dimensions.
//! 5. **Vertical moves.** The feeds of the moves that change only Z are
//!    equal, except where a [`Difference`] says otherwise.
//! 6. **Holes**, for a project with a Drill: the holes, in the order they
//!    are drilled, are at the same places and depths.
//! 7. **Order**, with [`Order::InOrder`]: the runs are compared one by one,
//!    in program order, so the order of shapes and passes must agree as
//!    well.
//!
//! A V-Carve case is compared by [`vcarve::compare`] instead of steps 2 to
//! 7: its cuts move in X, Y and Z together.
//!
//! A [`Difference`] replaces a comparison with a check of each program
//! against its own rule. On failure the message names the check and prints
//! a unified diff of the two programs, normalised as
//! `docs/rust-guidelines.md` requires.

use std::f64::consts::{PI, TAU};
use std::fmt::Write as _;
use std::path::PathBuf;

use dc_cam::entry::ENTRY_CLEARANCE_MM;
use dc_cam::test_support::assert_safe;
use dc_cam::{
    generate_drill, generate_engrave, generate_pocket, generate_profile, generate_vcarve,
};
use dc_gcode::parse::{Block, MotionKind, parse_program};
use dc_gcode::{EmitOp, ProgramSplit, emit_programs};
use dc_geometry::{Point, Vec2, approx_eq};
use dc_project::{ClearanceStrategy, DrillCycle, Entry, Operation, Project, Tool};

#[path = "vcarve.rs"]
mod vcarve;

/// How far apart two programs' moves may be, mm (acceptance test 14).
pub const TOLERANCE_MM: f64 = 0.01;

/// How far a written point can sit from the point its program computed:
/// half a unit in the third decimal place on each of X and Y. Both
/// programs write millimetres to three places (the bundled grblHAL
/// definition's default here, PureCutCNC's builtin `grblhal` definition
/// for the golden files).
pub const ROUNDING_MM: f64 = 0.0005 * std::f64::consts::SQRT_2;

/// How far apart two written programs' moves may be: [`TOLERANCE_MM`]
/// between the computed paths, plus each program's rounding.
pub const ALLOWED_MM: f64 = TOLERANCE_MM + 2.0 * ROUNDING_MM;

/// Spacing of the points each move is sampled at, mm. Small next to
/// [`TOLERANCE_MM`] over the radii in the corpus.
pub const SAMPLE_STEP_MM: f64 = 0.05;

/// Coordinates read from the same rounded G-code number are equal to well
/// within this, mm.
const SAME_MM: f64 = 1e-6;

/// How far a written Z can sit from the Z its program computed, mm.
const Z_ROUNDING_MM: f64 = 0.0005;

/// The date DreamCarve's program is emitted with; [`normalise`] replaces
/// the header line in both programs anyway.
const DATE: &str = "2026-01-01";

/// One golden case: the captured directory under `tests/golden/` and the
/// equivalent DreamCarve project.
pub struct Case {
    /// Directory name under `tests/golden/`.
    pub name: &'static str,
    /// The DreamCarve project equivalent to the case's `.camj`.
    pub project: Project,
    /// Intentional differences from PureCutCNC that the case exercises.
    pub differences: Vec<Difference>,
    /// Whether the order of the cuts is compared too.
    pub order: Order,
}

/// What a case compares besides the sets of moves at each level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    /// Each level is a set: the order of contours and passes is free.
    Sets,
    /// The runs of horizontal cuts are compared one by one in program
    /// order, so the shapes and passes must be visited in the same order
    /// (`docs/specs/phase-2/functional.md`, acceptance test 38). A
    /// V-Carve's regions are visited in the same order, each compared as
    /// a set ([`vcarve::compare`]).
    InOrder,
}

/// An intentional difference from PureCutCNC that the harness checks for
/// instead of comparing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difference {
    /// Rect tab steps are vertical moves at the plunge rate in DreamCarve
    /// and at the cutting feed in PureCutCNC. A tab step is a vertical move
    /// between the tab top and a lower level.
    RectTabStepsAtPlungeRate,
    /// The ramp or helix entry is placed differently (#147, #165, #174):
    /// DreamCarve's Profile ramp starts at the lap's entry point and a later
    /// pass descends from the previous one; PureCutCNC centres its zigzag
    /// elsewhere and comes down from safe Z every pass. The sloped moves
    /// are not compared, and neither are the horizontal moves that belong
    /// to the entry (those lying wholly within a tool radius of the
    /// descent: a ramp's tail, a helix's flat turn and its links to the
    /// lap). The rest of each level must still lie on the other program's
    /// moves, both ways, and its laps (loops of at least the tool's
    /// circumference) must have the same length. Every descent of both
    /// programs is checked against the operation's entry instead: it
    /// descends no steeper than the entry angle; a helix turns on a circle
    /// of the entry's diameter, the same way in both programs; and a
    /// DreamCarve ramp starts on the path it enters.
    EntryPlacement,
    /// DreamCarve counts a peck cycle's pecks from the retract plane, peck
    /// k ending at `max(bottom, R − k·peck)`; PureCutCNC counts them from
    /// the hole top (#153). The holes are compared, not the peck depths,
    /// and each program's pecks are checked against its own rule.
    PecksFromRetractPlane,
    /// The second operation is DreamCarve rest machining (`rest_of` the
    /// first) where PureCutCNC runs a finish pass on the rough's radial
    /// stock. Only the first operation is compared. Each program must then
    /// clear, at the final depth, every point of the pocket that its
    /// smaller tool can reach; DreamCarve's rest operation must cut at
    /// every level the rough did, and PureCutCNC's finish pass at the
    /// final depth alone.
    RestMachiningAgainstFinishPass,
    /// Raster lines are ordered greedily nearest-first in PureCutCNC; in
    /// DreamCarve nearest-first among the pieces a link inside the region
    /// reaches, or in scan order where that lifts less (#146, decision 5;
    /// #150). So the two programs cross the region in different orders and
    /// link different pairs of lines. The raster lines (the open moves along the raster angle) are
    /// compared without their direction (the design spec: "raster lines
    /// ignore direction"); the wall pass rings are compared as usual; the
    /// links between lines, which follow the order, are not compared.
    RasterLineOrder,
    /// Lead arcs run at the cutting feed in DreamCarve (#180, #185) and at
    /// a reduced feed in PureCutCNC, which holds one feed across a lead.
    /// Their feed is not compared; DreamCarve's must be the tool's feed,
    /// PureCutCNC's below it.
    LeadArcsAtCuttingFeed,
    /// Within one pass of a `pass_first` operation, DreamCarve visits the
    /// shapes nearest-first from the previous pass's last exit (D5,
    /// `docs/specs/phase-2/design.md`, "Sequence", step 3); PureCutCNC's
    /// `level_first` routes an outside edge's outlines in drawing order at
    /// every level and starts an inside edge's walk elsewhere. The order of
    /// the passes is compared (each pass of every shape before the next,
    /// deeper pass), each pass as a set; DreamCarve's walk within a pass is
    /// checked against D5.
    PassOrderWithinLayer,
    /// A Profile's relief notches are cut from the lap (#240; inline on
    /// every lap since the maintainer decision of 2026-10-24, #373): where
    /// the lap passes a descend point, DreamCarve moves straight up, at the
    /// plunge rate, in the column the lap just cut, to the notch's first
    /// level, where PureCutCNC cuts its notches after the laps, retracting
    /// to safe Z and plunging at each. DreamCarve's lifts are not compared
    /// with PureCutCNC's (it has none); each must run at the tool's plunge
    /// rate.
    NotchRisesFromLapEnd,
    /// A V-Carve cut that descends while it moves in XY is held to the
    /// plunge rate in DreamCarve (S10): it runs no faster than the cutting
    /// feed and falls no faster than the plunge rate. PureCutCNC runs it at
    /// the cutting feed, so a steep corner arm falls faster than the
    /// plunge rate. Such cuts' feeds are not compared; DreamCarve's are
    /// checked against S10, PureCutCNC's must be the cutting feed.
    DescentsHeldToPlunge,
    /// Where the two programs' medial chains differ in depth at one XY
    /// (within the tolerance in XY, not in Z), each is checked against the
    /// walls instead (#404, decision 2; #410). PureCutCNC runs straight
    /// between the points its depth-aware simplification keeps, which near
    /// a reflex corner or a curved wall takes the cone past the wall;
    /// DreamCarve's depth fitting splits such a stretch until its cone
    /// stays inside, which only raises the carve. DreamCarve's point must
    /// lie on the V surface (its cone touching a wall, within the
    /// tolerance), higher or deeper; a golden point deeper than
    /// DreamCarve's must reach past a wall; a golden point higher than it
    /// stops short of the surface DreamCarve's reaches.
    ChainDepthFitted,
}

/// The DreamCarve project of case `name`: `tests/golden/<name>/dreamcarve.dcv`.
pub fn load(name: &str) -> Project {
    let path = golden_dir(name).join("dreamcarve.dcv");
    let bytes = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("{name}: cannot read {}: {e}", path.display()));
    let loaded = dc_project::load(&bytes)
        .unwrap_or_else(|e| panic!("{name}: cannot load {}: {e}", path.display()));
    assert!(
        !loaded.has_errors(),
        "{name}: the project has errors: {:?}",
        loaded.problems
    );
    loaded.project
}

/// Compares DreamCarve's program for `case` with the golden program.
pub fn check(case: &Case) {
    let project = &case.project;
    let golden_text = std::fs::read_to_string(golden_dir(case.name).join("program.nc"))
        .unwrap_or_else(|e| panic!("{}: cannot read the golden program: {e}", case.name));
    let ours_text = emit_program(case.name, project);

    let golden = normalise(&golden_text);
    let ours = normalise(&ours_text);
    let fail = |what: &str, message: String| -> ! {
        panic!(
            "{}: {what}: {message}\n\n--- golden (PureCutCNC)\n+++ DreamCarve\n{}",
            case.name,
            unified_diff(&golden, &ours, 3)
        )
    };
    let has = |d: Difference| case.differences.contains(&d);

    let limits = Limits::of(project);
    let golden_moves = read_moves(&golden_text);
    let ours_moves = read_moves(&ours_text);
    for (who, moves) in [("golden", &golden_moves), ("DreamCarve", &ours_moves)] {
        if let Err(e) = limits.check_safety(moves) {
            fail(&format!("{who} program is unsafe"), e);
        }
    }

    if project
        .operations
        .iter()
        .any(|op| op.enabled() && matches!(op, Operation::VCarve(_)))
    {
        match vcarve::compare(
            project,
            &golden_moves,
            &ours_moves,
            &case.differences,
            case.order,
        ) {
            Ok((golden, ours)) => eprintln!(
                "{}: sampled points (golden, DreamCarve): matched ({}, {}), at the flat depth ({}, {}), on links ({}, {}), depth-fitted ({}, {})",
                case.name,
                golden.matched,
                ours.matched,
                golden.floor,
                ours.floor,
                golden.link,
                ours.link,
                golden.fitted,
                ours.fitted
            ),
            Err(e) => fail("V-Carve cuts differ", e),
        }
        return;
    }

    let rules = Rules::of(project, &case.differences);
    // With rest machining only the first operation is compared move for move.
    let rest = has(Difference::RestMachiningAgainstFinishPass);
    let compared = |moves: &[Motion]| -> Vec<Motion> {
        moves
            .iter()
            .filter(|m| !rest || m.op == 0)
            .copied()
            .collect()
    };
    let cuts = |moves: &[Motion], who: &str| {
        Cuts::of(moves, &rules).unwrap_or_else(|e| fail(&format!("{who} program"), e))
    };
    let golden_cuts = cuts(&compared(&golden_moves), "golden");
    let ours_cuts = cuts(&compared(&ours_moves), "DreamCarve");

    if let Err(e) = compare_levels(&golden_cuts, &ours_cuts, &rules) {
        fail("cutting moves differ", e);
    }
    if rules.entry {
        for (who, cuts, ours) in [
            ("golden", &golden_cuts, false),
            ("DreamCarve", &ours_cuts, true),
        ] {
            if let Err(e) = check_descents(project, cuts, ours) {
                fail(&format!("{who} entry"), e);
            }
        }
        if matches!(entry_of(project), Some((Entry::Helix { .. }, _)))
            && let Err(e) = compare_helix_sense(&golden_cuts, &ours_cuts)
        {
            fail("helix entries differ", e);
        }
    } else if let Err(e) = compare_slopes(&golden_cuts.slopes, &ours_cuts.slopes) {
        fail("sloped moves differ", e);
    }
    if let Some(radius) = rules.lead_radius
        && let Err(e) = check_lead_feeds(&golden_cuts, &ours_cuts, radius, &limits)
    {
        fail("lead feeds", e);
    }
    if let Err(e) = compare_verticals(
        &golden_cuts,
        &ours_cuts,
        &limits,
        tab_top_z(project),
        &case.differences,
    ) {
        fail("vertical moves differ", e);
    }
    if project
        .operations
        .iter()
        .any(|op| matches!(op, Operation::Drill(_)))
        && let Err(e) = compare_holes(project, &golden_cuts, &ours_cuts, &case.differences)
    {
        fail("holes differ", e);
    }
    if case.order == Order::InOrder {
        let compared = if has(Difference::PassOrderWithinLayer) {
            compare_layers(&golden_cuts.runs, &ours_cuts.runs, &rules)
                .and_then(|()| check_nearest_first(&ours_cuts.runs))
        } else {
            compare_runs(&golden_cuts.runs, &ours_cuts.runs, &rules)
        };
        if let Err(e) = compared {
            fail("cuts are made in a different order", e);
        }
    }
    if rest {
        let golden_all = cuts(&golden_moves, "golden");
        let ours_all = cuts(&ours_moves, "DreamCarve");
        for (who, cuts) in [("golden", &golden_all), ("DreamCarve", &ours_all)] {
            if let Err(e) = check_rest_coverage(project, cuts) {
                fail(&format!("{who} program leaves material"), e);
            }
        }
        if let Err(e) = check_rest_levels(&golden_all, &ours_all) {
            fail("rest levels", e);
        }
    }
}

fn golden_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name)
}

/// The operation's tool.
fn tool_of<'a>(project: &'a Project, op: &Operation) -> &'a Tool {
    project
        .tools
        .iter()
        .find(|t| &t.id == op.tool())
        .expect("the operation's tool is in the project")
}

/// Generates, checks and emits the project's enabled operations as one
/// program, as `dreamcarve export --one-program` does. A project whose
/// Profiles hold tab requests emits the same bytes once `tabs place` has
/// written them as placed tabs (#105, ADR-0005), which is checked too.
fn emit_program(name: &str, project: &Project) -> String {
    let program = emit_program_as_is(name, project);
    let (placed, _) = dc_cam::placed_tabs::place_requests(project)
        .unwrap_or_else(|e| panic!("{name}: the tabs place: {e}"));
    if placed != *project {
        let from_placed = emit_program_as_is(name, &placed);
        assert!(
            from_placed == program,
            "{name}: placed tabs cut differently:\n{from_placed}"
        );
    }
    program
}

/// [`emit_program`] for the project as it is.
fn emit_program_as_is(name: &str, project: &Project) -> String {
    let mut ops = Vec::new();
    for op in project.operations.iter().filter(|op| op.enabled()) {
        let tool = tool_of(project, op).clone();
        let (emit_op, warnings) = match op {
            Operation::Profile(p) => {
                let (t, w) = generate_profile(project, p).expect("the profile generates");
                (EmitOp::from((op.clone(), tool, t)), w)
            }
            Operation::Pocket(p) => {
                let (t, w) = generate_pocket(project, p).expect("the pocket generates");
                (EmitOp::from((op.clone(), tool, t)), w)
            }
            Operation::Engrave(p) => {
                let (t, w) = generate_engrave(project, p).expect("the engrave generates");
                (EmitOp::from((op.clone(), tool, t)), w)
            }
            Operation::Drill(p) => {
                let (d, w) = generate_drill(project, p).expect("the drill generates");
                (EmitOp::drill(op.clone(), tool, d), w)
            }
            Operation::Bore(p) => {
                let (t, w) = dc_cam::generate_bore(project, p).expect("the bore generates");
                (EmitOp::from((op.clone(), tool, t)), w)
            }
            Operation::VCarve(p) => {
                // One tool run per clearing pass, then the V-bit's, each
                // written with its own tool (as `export` does).
                let (runs, w) = generate_vcarve(project, p).expect("the V-Carve generates");
                assert!(
                    w.is_empty(),
                    "{name}: operation `{}` generates without warnings: {w:?}",
                    op.id()
                );
                for run in runs {
                    let tool = project
                        .tools
                        .iter()
                        .find(|t| t.id == run.tool)
                        .expect("the run's tool is in the project")
                        .clone();
                    assert_safe(&run.toolpath, project, op);
                    ops.push(EmitOp {
                        op: op.clone(),
                        tool,
                        toolpath: run.toolpath,
                        holes: None,
                        label: run.label,
                    });
                }
                continue;
            }
            Operation::Inlay(_) => panic!("{name}: no Inlay is generated before #436"),
        };
        assert!(
            warnings.is_empty(),
            "{name}: operation `{}` generates without warnings: {warnings:?}",
            op.id()
        );
        assert_safe(&emit_op.toolpath, project, op);
        ops.push(emit_op);
    }
    let (programs, warnings) = emit_programs(project, &ops, ProgramSplit::OneProgram, DATE)
        .unwrap_or_else(|e| panic!("{name}: the program emits: {e}"));
    assert!(
        warnings.is_empty(),
        "{name}: the program emits without warnings: {warnings:?}"
    );
    let [program] = programs.as_slice() else {
        panic!("{name}: one program, not {}", programs.len());
    };
    program.text.clone()
}

/// The stock thickness; every case sets it.
fn thickness(project: &Project) -> f64 {
    project
        .stock
        .thickness_mm
        .expect("every case sets the stock thickness")
}

/// The machine Z of the tab top, if a Profile has tabs. Machine Z zero is
/// the stock top in every case.
fn tab_top_z(project: &Project) -> Option<f64> {
    project.operations.iter().find_map(|op| match op {
        Operation::Profile(p) => p
            .tabs
            .as_ref()
            .map(|tabs| tabs.height_mm - thickness(project)),
        _ => None,
    })
}

/// How a case's levels are compared, from its differences.
struct Rules {
    /// [`Difference::EntryPlacement`].
    entry: bool,
    /// [`Difference::RasterLineOrder`]: the raster lines' direction.
    raster: Option<Vec2>,
    /// [`Difference::LeadArcsAtCuttingFeed`]: the lead radius.
    lead_radius: Option<f64>,
    /// The largest tool radius of the enabled operations, mm.
    tool_radius: f64,
}

impl Rules {
    fn of(project: &Project, differences: &[Difference]) -> Self {
        let lead_radius = differences
            .contains(&Difference::LeadArcsAtCuttingFeed)
            .then(|| {
                project.operations.iter().find_map(|op| match op {
                    Operation::Profile(p) => p.lead.as_ref().map(|l| l.radius_mm),
                    _ => None,
                })
            })
            .flatten();
        let tool_radius = project
            .operations
            .iter()
            .filter(|o| o.enabled())
            .map(|o| tool_of(project, o).diameter_mm / 2.0)
            .fold(0.0, f64::max);
        let raster = differences
            .contains(&Difference::RasterLineOrder)
            .then(|| {
                project.operations.iter().find_map(|op| match op {
                    Operation::Pocket(p) => match p.strategy {
                        ClearanceStrategy::Raster { angle_deg } => {
                            let (sin, cos) = angle_deg.to_radians().sin_cos();
                            Some(Vec2::new(cos, sin))
                        }
                        ClearanceStrategy::Offset => None,
                    },
                    _ => None,
                })
            })
            .flatten();
        Self {
            entry: differences.contains(&Difference::EntryPlacement),
            raster,
            lead_radius,
            tool_radius,
        }
    }

    /// Whether `piece` is a raster line: an open line along the raster
    /// angle, either way.
    fn is_raster_line(&self, piece: &Piece) -> bool {
        let Some(dir) = self.raster else {
            return false;
        };
        let along = Vec2::new(piece.to.x - piece.from.x, piece.to.y - piece.from.y);
        piece.role == Role::Open
            && piece.arc.is_none()
            && along
                .normalized()
                .is_some_and(|u| u.cross(dir).abs() <= 1e-3)
    }

    /// Whether `piece` is a lead arc whose feed is not compared.
    fn is_lead(&self, piece: &Piece) -> bool {
        match (self.lead_radius, piece.arc) {
            (Some(radius), Some(arc)) => {
                piece.role == Role::Open && approx_eq(arc.r0, radius, ALLOWED_MM)
            }
            _ => false,
        }
    }
}

// ---------------------------------------------------------------------------
// Normalisation and diff

/// `text` with `\n` line endings, no trailing whitespace, and the header
/// timestamp replaced, as lines.
fn normalise(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n")
        .lines()
        .map(|line| {
            let line = line.trim_end();
            if line.contains("Generated by") {
                "( Generated on <date> )".to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect()
}

/// A unified diff from `a` to `b` with `context` lines around each change.
fn unified_diff(a: &[String], b: &[String], context: usize) -> String {
    // Longest common subsequence table, then a walk that lists each line as
    // kept, removed or added.
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0_usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut ops = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            ops.push((' ', i, j));
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            ops.push(('+', i, j));
            j += 1;
        } else {
            ops.push(('-', i, j));
            i += 1;
        }
    }

    let changed: Vec<usize> = (0..ops.len()).filter(|&k| ops[k].0 != ' ').collect();
    let mut out = String::new();
    let mut k = 0;
    while k < changed.len() {
        let start = changed[k].saturating_sub(context);
        let mut end = changed[k] + context + 1;
        while k + 1 < changed.len() && changed[k + 1] <= end + context {
            k += 1;
            end = changed[k] + context + 1;
        }
        let end = end.min(ops.len());
        let (_, a0, b0) = ops[start];
        let a_len = ops[start..end].iter().filter(|o| o.0 != '+').count();
        let b_len = ops[start..end].iter().filter(|o| o.0 != '-').count();
        let _ = writeln!(out, "@@ -{},{a_len} +{},{b_len} @@", a0 + 1, b0 + 1);
        for &(tag, ai, bj) in &ops[start..end] {
            let line = if tag == '+' { &b[bj] } else { &a[ai] };
            let _ = writeln!(out, "{tag}{line}");
        }
        k += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// Reading programs

/// A point in the machine frame, mm.
#[derive(Debug, Clone, Copy, PartialEq)]
struct P3 {
    x: f64,
    y: f64,
    z: f64,
}

impl P3 {
    fn xy(self) -> Point {
        Point::new(self.x, self.y)
    }

    fn distance_to(self, other: Self) -> f64 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2) + (self.z - other.z).powi(2))
            .sqrt()
    }
}

/// One move of a program, machine frame, mm.
#[derive(Debug, Clone, Copy)]
struct Motion {
    line: usize,
    /// Index of the operation the move belongs to, counted from the
    /// program's `( Operation k: … )` comments; 0 before the first.
    op: usize,
    kind: MotionKind,
    /// `None` until every axis has been set.
    from: Option<P3>,
    to: Option<P3>,
    /// Z the move starts at, even when X or Y is not known yet.
    from_z: Option<f64>,
    /// Z the move ends at, even when X or Y is not known yet.
    to_z: Option<f64>,
    /// Whether the move changes X or Y, or sets one for the first time.
    moves_xy: bool,
    /// Arc centre, from I and J.
    centre: Option<Point>,
    feed: Option<f64>,
    /// `G53`: the block's coordinates are machine coordinates.
    machine_coordinates: bool,
}

/// The operation index of each line of `text` (1-based lines), from the
/// `( Operation k: … )` comments both programs write.
fn operation_of_lines(text: &str) -> Vec<usize> {
    let mut current = None::<usize>;
    let mut of_line = vec![0];
    for line in text.lines() {
        if line.trim_start().starts_with("( Operation ") {
            current = Some(current.map_or(0, |k| k + 1));
        }
        of_line.push(current.unwrap_or(0));
    }
    of_line
}

/// The moves of a program, which must parse and be in millimetres.
fn read_moves(text: &str) -> Vec<Motion> {
    let blocks: Vec<Block> = parse_program(text).expect("the program parses");
    let op_of_line = operation_of_lines(text);
    let point = |p: dc_gcode::parse::MachinePosition| {
        Some(P3 {
            x: p.x?,
            y: p.y?,
            z: p.z?,
        })
    };
    blocks
        .iter()
        .filter_map(|block| {
            let mv = block.motion?;
            assert!(
                mv.units == dc_project::Units::Mm,
                "line {}: golden comparison runs in millimetres",
                mv.line
            );
            let from = point(mv.from);
            let centre = match (from, mv.arc.i, mv.arc.j) {
                (Some(f), Some(i), Some(j)) => Some(Point::new(f.x + i, f.y + j)),
                _ => None,
            };
            assert!(
                mv.arc.r.is_none(),
                "line {}: R-form arcs are not compared",
                mv.line
            );
            Some(Motion {
                line: mv.line,
                op: op_of_line.get(mv.line).copied().unwrap_or(0),
                kind: mv.kind,
                from,
                to: point(mv.to),
                from_z: mv.from.z,
                to_z: mv.to.z,
                moves_xy: differs(mv.from.x, mv.to.x) || differs(mv.from.y, mv.to.y),
                centre,
                feed: mv.feed,
                machine_coordinates: block.has('G', 53.0),
            })
        })
        .collect()
}

/// True unless both are known and equal.
fn differs(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => !approx_eq(a, b, SAME_MM),
        (None, None) => false,
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// Safety

/// The S1–S3 limits of a case, machine frame, mm.
struct Limits {
    floor_z: f64,
    /// The lowest safe Z of the enabled operations.
    safe_z: f64,
    /// Whether a Drill may rapid down in Z alone below safe Z.
    drills: bool,
    /// The feed and plunge rate of each enabled operation's tool, in
    /// operation order.
    rates: Vec<f64>,
    /// The plunge rate and feed of a V-Carve's tool, if the case has one:
    /// a cut that descends while it moves in XY may run between them (S3
    /// with S10's scaling, as `dc_gcode`'s safety pass reads it).
    held_descents: Option<(f64, f64)>,
}

impl Limits {
    fn of(project: &Project) -> Self {
        // Machine Z zero is the stock top in every case.
        let ops: Vec<&Operation> = project.operations.iter().filter(|o| o.enabled()).collect();
        let allowance = ops
            .iter()
            .map(|o| o.through_cut_allowance_mm())
            .fold(0.0, f64::max);
        let safe_z = ops
            .iter()
            .map(|o| o.safe_z_mm())
            .fold(f64::INFINITY, f64::min);
        let mut rates = Vec::new();
        for op in &ops {
            let tool = tool_of(project, op);
            rates.extend(tool.feed_mm_min);
            rates.push(tool.plunge_mm_min.expect("the tool has a plunge rate"));
        }
        let held_descents = ops.iter().find_map(|op| match op {
            Operation::VCarve(_) => {
                let tool = tool_of(project, op);
                Some((tool.plunge_mm_min?, tool.feed_mm_min?))
            }
            _ => None,
        });
        Self {
            floor_z: -thickness(project) - allowance,
            safe_z,
            drills: ops.iter().any(|o| matches!(o, Operation::Drill(_))),
            rates,
            held_descents,
        }
    }

    /// S1 to S3 on `moves`, as `dc_gcode`'s safety pass checks them on
    /// emitted text. A `G53` rapid that only moves Z is allowed: machine Z
    /// zero is the top of travel on GRBL and grblHAL, above any safe Z.
    fn check_safety(&self, moves: &[Motion]) -> Result<(), String> {
        let first = moves.first().ok_or("the program has no moves")?;
        if first.kind != MotionKind::Rapid || first.to_z.is_none_or(|z| z < self.safe_z) {
            return Err(format!(
                "line {}: the first move is not a rapid to safe Z",
                first.line
            ));
        }
        // The cuts read so far, for the floors a rapid down may stop over.
        let mut cuts: Vec<&Motion> = Vec::new();
        for mv in moves {
            let line = mv.line;
            if mv.machine_coordinates {
                if mv.kind != MotionKind::Rapid || mv.moves_xy {
                    return Err(format!("line {line}: G53 is allowed only on a Z rapid"));
                }
                continue;
            }
            // S1.
            if let Some(z) = mv.to_z
                && z < self.floor_z - SAME_MM
            {
                return Err(format!(
                    "line {line}: Z {z} is below the floor {}",
                    self.floor_z
                ));
            }
            if mv.kind == MotionKind::Rapid {
                // S2. A Drill rapids down to its retract plane, and between
                // pecks inside its hole, in Z alone. Any operation rapids
                // straight down from safe Z to the start of a ramp, helix
                // or plunge the entry clearance above the stock top (#214),
                // or above the lowest floor a cut of the same operation
                // reached through that XY (#227).
                let low = |z: Option<f64>| z.is_none_or(|z| z < self.safe_z - SAME_MM);
                let drill_plunge = self.drills && !mv.moves_xy;
                let clear_of = |floor: f64| {
                    mv.to_z
                        .is_some_and(|z| z >= floor + ENTRY_CLEARANCE_MM - Z_ROUNDING_MM)
                };
                let over_cut = || {
                    mv.to.is_some_and(|to| {
                        cuts.iter()
                            .filter(|c| c.op == mv.op)
                            .filter_map(|c| floor_through(c, to))
                            .any(clear_of)
                    })
                };
                let entry_approach =
                    !mv.moves_xy && !low(mv.from_z) && (clear_of(0.0) || over_cut());
                if !drill_plunge
                    && !entry_approach
                    && (low(mv.to_z) || (mv.moves_xy && low(mv.from_z)))
                {
                    return Err(format!(
                        "line {line}: rapid below safe Z {}: {mv:?}",
                        self.safe_z
                    ));
                }
            } else {
                // S3.
                let Some(feed) = mv.feed.filter(|f| *f > 0.0) else {
                    return Err(format!("line {line}: cut without a feed"));
                };
                let descends = mv.moves_xy
                    && matches!((mv.from_z, mv.to_z), (Some(a), Some(b)) if b < a - SAME_MM);
                let held = descends
                    && self.held_descents.is_some_and(|(plunge, top)| {
                        (plunge - SAME_MM..=top + SAME_MM).contains(&feed)
                    });
                if !held && !self.rates.iter().any(|r| approx_eq(feed, *r, SAME_MM)) {
                    return Err(format!(
                        "line {line}: feed {feed} is none of the tools' rates {:?}",
                        self.rates
                    ));
                }
                cuts.push(mv);
            }
        }
        Ok(())
    }
}

/// How far, mm, a cut may pass from a rapid's XY and still be the floor
/// under it: `dc_cam`'s 0.01 mm reach plus the program's rounding.
const FLOOR_REACH_MM: f64 = 0.02;

/// The Z at which cut `cut` passes within [`FLOOR_REACH_MM`] of `at` in
/// XY, if it does: a straight move's Z where it comes nearest; an arc's
/// higher end Z, counted only near its ends (enough for the corpus, and
/// never lower than the arc reached).
fn floor_through(cut: &Motion, at: P3) -> Option<f64> {
    let (a, b) = (cut.from?, cut.to?);
    let near = |p: P3| (p.x - at.x).hypot(p.y - at.y) <= FLOOR_REACH_MM;
    if cut.centre.is_some() {
        return (near(a) || near(b)).then_some(a.z.max(b.z));
    }
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length_sq = dx * dx + dy * dy;
    let t = if length_sq > SAME_MM * SAME_MM {
        (((at.x - a.x) * dx + (at.y - a.y) * dy) / length_sq).clamp(0.0, 1.0)
    } else {
        // Straight down: the tool reached the lower end here.
        return near(a).then_some(a.z.min(b.z));
    };
    let nearest = P3 {
        x: a.x + dx * t,
        y: a.y + dy * t,
        z: a.z + (b.z - a.z) * t,
    };
    near(nearest).then_some(nearest.z)
}

// ---------------------------------------------------------------------------
// Geometry

/// What a horizontal move is part of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    /// A stretch of moves that ends where it began.
    Loop,
    /// The one move from the end of a loop to the start of the next.
    Link,
    /// Anything else.
    Open,
}

/// A cutting move that is not vertical, machine frame, mm: horizontal (a
/// level move) or sloped (a ramp or helix move).
#[derive(Debug, Clone, Copy)]
struct Piece {
    line: usize,
    op: usize,
    from: Point,
    to: Point,
    /// Z at the start.
    z0: f64,
    /// Z at the end; the level's Z for a horizontal move.
    z: f64,
    /// `Some` for an arc.
    arc: Option<ArcShape>,
    feed: f64,
    role: Role,
    /// The loop it belongs to, numbered through the program.
    group: usize,
    /// With [`Difference::EntryPlacement`], whether the move belongs to a
    /// descent's entry rather than to the path.
    entry: bool,
}

/// The circle an arc move follows.
#[derive(Debug, Clone, Copy)]
struct ArcShape {
    centre: Point,
    /// Start and end radius; the controller blends between them.
    r0: f64,
    r1: f64,
    start_angle: f64,
    /// Signed sweep, radians: positive counter-clockwise.
    sweep: f64,
}

impl Piece {
    fn new(mv: &Motion) -> Result<Self, String> {
        let (Some(from), Some(to), Some(feed)) = (mv.from, mv.to, mv.feed) else {
            return Err(format!("line {}: cut from an unknown position", mv.line));
        };
        let arc = match mv.kind {
            MotionKind::ArcCw | MotionKind::ArcCcw => {
                let centre = mv
                    .centre
                    .ok_or_else(|| format!("line {}: arc without I and J", mv.line))?;
                let start_angle = angle_of(centre, from.xy());
                let end_angle = angle_of(centre, to.xy());
                let ccw = mv.kind == MotionKind::ArcCcw;
                let mut sweep = if ccw {
                    (end_angle - start_angle).rem_euclid(TAU)
                } else {
                    -(start_angle - end_angle).rem_euclid(TAU)
                };
                if sweep.abs() < 1e-9 {
                    sweep = if ccw { TAU } else { -TAU };
                }
                Some(ArcShape {
                    centre,
                    r0: centre.distance_to(from.xy()),
                    r1: centre.distance_to(to.xy()),
                    start_angle,
                    sweep,
                })
            }
            _ => None,
        };
        Ok(Self {
            line: mv.line,
            op: mv.op,
            from: from.xy(),
            to: to.xy(),
            z0: from.z,
            z: to.z,
            arc,
            feed,
            role: Role::Open,
            group: 0,
            entry: false,
        })
    }

    /// Length in XY.
    fn length(&self) -> f64 {
        match self.arc {
            None => self.from.distance_to(self.to),
            Some(a) => (a.r0 + a.r1) / 2.0 * a.sweep.abs(),
        }
    }

    /// The point and unit tangent at fraction `t` of the move, in XY.
    fn at(&self, t: f64) -> (Point, Vec2) {
        match self.arc {
            None => {
                let d = Vec2::new(self.to.x - self.from.x, self.to.y - self.from.y);
                let p = Point::new(self.from.x + d.x * t, self.from.y + d.y * t);
                (p, d.normalized().unwrap_or(Vec2::new(0.0, 0.0)))
            }
            Some(a) => {
                let angle = a.start_angle + a.sweep * t;
                let r = a.r0 + (a.r1 - a.r0) * t;
                let (sin, cos) = angle.sin_cos();
                let p = Point::new(a.centre.x + r * cos, a.centre.y + r * sin);
                let tangent = Vec2::new(-sin, cos);
                let tangent = if a.sweep > 0.0 {
                    tangent
                } else {
                    Vec2::new(-tangent.x, -tangent.y)
                };
                (p, tangent)
            }
        }
    }

    /// The point at fraction `t`, in three dimensions.
    fn at3(&self, t: f64) -> P3 {
        let (p, _) = self.at(t);
        P3 {
            x: p.x,
            y: p.y,
            z: self.z0 + (self.z - self.z0) * t,
        }
    }

    /// The fraction of the move whose XY point is nearest `p`.
    fn nearest_t(&self, p: Point) -> f64 {
        match self.arc {
            None => {
                let d = Vec2::new(self.to.x - self.from.x, self.to.y - self.from.y);
                let len2 = d.dot(d);
                if len2 <= 0.0 {
                    0.0
                } else {
                    (Vec2::new(p.x - self.from.x, p.y - self.from.y).dot(d) / len2).clamp(0.0, 1.0)
                }
            }
            Some(a) => {
                // Fraction of the sweep at p's angle; outside the sweep, the
                // nearer end.
                let offset = (angle_of(a.centre, p) - a.start_angle) * a.sweep.signum();
                let along = offset.rem_euclid(TAU) / a.sweep.abs();
                if along <= 1.0 {
                    along
                } else {
                    let past_end = self.to.distance_to(p);
                    let before_start = self.from.distance_to(p);
                    if past_end < before_start { 1.0 } else { 0.0 }
                }
            }
        }
    }

    /// The point of this move nearest `p` in XY: its distance, the move's
    /// tangent there, and whether it is one of the move's ends.
    fn nearest(&self, p: Point) -> (f64, Vec2, bool) {
        let t = self.nearest_t(p);
        let (q, tangent) = self.at(t);
        let at_end = t <= 1e-9 || t >= 1.0 - 1e-9;
        (q.distance_to(p), tangent, at_end)
    }

    /// The distance from `p` to this sloped move, in three dimensions. A
    /// line is projected exactly; an arc (a helix turn) at the fraction
    /// nearest in XY, which over the steep, short turns of an entry helix
    /// differs from the true nearest point by far less than the tolerance.
    fn distance3(&self, p: P3) -> f64 {
        let t = match self.arc {
            None => {
                let (a, b) = (self.at3(0.0), self.at3(1.0));
                let d = (b.x - a.x, b.y - a.y, b.z - a.z);
                let len2 = d.0 * d.0 + d.1 * d.1 + d.2 * d.2;
                if len2 <= 0.0 {
                    0.0
                } else {
                    (((p.x - a.x) * d.0 + (p.y - a.y) * d.1 + (p.z - a.z) * d.2) / len2)
                        .clamp(0.0, 1.0)
                }
            }
            Some(_) => self.nearest_t(p.xy()),
        };
        self.at3(t).distance_to(p)
    }

    /// Fractions of the move no more than [`SAMPLE_STEP_MM`] apart in XY,
    /// ends included.
    fn sample_ts(&self) -> Vec<f64> {
        let ratio = self.length() / SAMPLE_STEP_MM;
        let mut count = 1_u32;
        while f64::from(count) < ratio {
            count += 1;
        }
        (0..=count)
            .map(|k| f64::from(k) / f64::from(count))
            .collect()
    }

    /// Points along the move no more than [`SAMPLE_STEP_MM`] apart, ends
    /// included, with the tangent at each.
    fn samples(&self) -> Vec<(Point, Vec2)> {
        self.sample_ts().into_iter().map(|t| self.at(t)).collect()
    }
}

fn angle_of(centre: Point, p: Point) -> f64 {
    (p.y - centre.y).atan2(p.x - centre.x)
}

/// A vertical move: XY fixed, Z changes.
#[derive(Debug, Clone, Copy)]
struct Vertical {
    line: usize,
    at: Point,
    from_z: f64,
    to_z: f64,
    feed: f64,
}

/// Consecutive horizontal cuts at one Z, in program order: what one pass
/// of one shape looks like. A rapid, a vertical or a sloped move ends a
/// run.
#[derive(Debug, Clone)]
struct Run {
    z: f64,
    op: usize,
    pieces: Vec<Piece>,
}

/// A program's cutting moves: runs of horizontal moves, sloped moves and
/// vertical moves.
struct Cuts {
    /// Sloped moves in program order.
    slopes: Vec<Piece>,
    verticals: Vec<Vertical>,
    runs: Vec<Run>,
}

impl Cuts {
    fn of(moves: &[Motion], rules: &Rules) -> Result<Self, String> {
        let mut verticals = Vec::new();
        let mut slopes = Vec::new();
        let mut runs: Vec<Run> = Vec::new();
        // Whether the next horizontal cut continues the last run.
        let mut in_run = false;
        for mv in moves {
            if !mv.kind.is_cut() {
                in_run = false;
                continue;
            }
            let (Some(from), Some(to)) = (mv.from, mv.to) else {
                return Err(format!("line {}: cut from an unknown position", mv.line));
            };
            let feed = mv
                .feed
                .ok_or_else(|| format!("line {}: cut without a feed", mv.line))?;
            let flat = approx_eq(from.z, to.z, SAME_MM);
            let still = from.xy().distance_to(to.xy()) <= SAME_MM && mv.kind == MotionKind::Linear;
            if flat {
                let piece = Piece::new(mv)?;
                match runs.last_mut() {
                    Some(run) if in_run && approx_eq(run.z, piece.z, SAME_MM) => {
                        run.pieces.push(piece);
                    }
                    _ => runs.push(Run {
                        z: piece.z,
                        op: piece.op,
                        pieces: vec![piece],
                    }),
                }
                in_run = true;
            } else if still {
                in_run = false;
                verticals.push(Vertical {
                    line: mv.line,
                    at: to.xy(),
                    from_z: from.z,
                    to_z: to.z,
                    feed,
                });
            } else {
                in_run = false;
                slopes.push(Piece::new(mv)?);
            }
        }
        let mut group = 0;
        for run in &mut runs {
            classify(&mut run.pieces, &mut group);
        }
        let mut cuts = Self {
            slopes,
            verticals,
            runs,
        };
        if rules.entry {
            cuts.mark_entries(rules.tool_radius);
        }
        Ok(cuts)
    }

    /// Marks the horizontal moves that lie wholly within `radius` of a
    /// descent's path in XY as part of the entry.
    fn mark_entries(&mut self, radius: f64) {
        let paths: Vec<Point> = self
            .slopes
            .iter()
            .flat_map(|p| p.samples().into_iter().map(|(q, _)| q))
            .collect();
        let near = |p: Point| paths.iter().any(|q| q.distance_to(p) <= radius);
        let lap = TAU * radius;
        for run in &mut self.runs {
            let lengths: Vec<f64> = run
                .pieces
                .iter()
                .map(|p| {
                    let group: Vec<&Piece> = run.pieces.iter().collect();
                    group_length(&group, p.group)
                })
                .collect();
            for (piece, length) in run.pieces.iter_mut().zip(lengths) {
                // A lap is never part of the entry, however close the
                // descent comes to it.
                let lap_loop = piece.role == Role::Loop && length >= lap;
                piece.entry = !lap_loop && piece.samples().iter().all(|(p, _)| near(*p));
            }
        }
    }

    /// The horizontal moves grouped by Z, highest first, in program order.
    fn levels(&self) -> Vec<(f64, Vec<Piece>)> {
        let mut levels: Vec<(f64, Vec<Piece>)> = Vec::new();
        for run in &self.runs {
            match levels
                .iter_mut()
                .find(|(z, _)| approx_eq(*z, run.z, SAME_MM))
            {
                Some((_, pieces)) => pieces.extend(run.pieces.iter().copied()),
                None => levels.push((run.z, run.pieces.clone())),
            }
        }
        levels.sort_by(|a, b| b.0.total_cmp(&a.0));
        levels
    }
}

/// Sets the role of each move of one run: a stretch that ends where an
/// earlier move of it began is a loop; a single move between two loops is
/// a link; the rest is open. `group` numbers the loops.
fn classify(pieces: &mut [Piece], group: &mut usize) {
    let mut start = 0;
    // Each item is (first, last, is_loop), inclusive.
    let mut items: Vec<(usize, usize, bool)> = Vec::new();
    for j in 0..pieces.len() {
        let end = pieces[j].to;
        if let Some(i) = (start..=j).find(|&i| pieces[i].from.distance_to(end) <= ALLOWED_MM) {
            if i > start {
                items.push((start, i - 1, false));
            }
            items.push((i, j, true));
            start = j + 1;
        }
    }
    if start < pieces.len() {
        items.push((start, pieces.len() - 1, false));
    }
    for (k, &(first, last, is_loop)) in items.iter().enumerate() {
        let between_loops =
            k > 0 && items.get(k + 1).is_some_and(|next| next.2) && items[k - 1].2 && first == last;
        let role = if is_loop {
            *group += 1;
            Role::Loop
        } else if between_loops {
            Role::Link
        } else {
            Role::Open
        };
        for piece in &mut pieces[first..=last] {
            piece.role = role;
            piece.group = *group;
        }
    }
}

/// Every level in both programs, with the same moves on each.
fn compare_levels(golden: &Cuts, ours: &Cuts, rules: &Rules) -> Result<(), String> {
    let (gl, ol) = (golden.levels(), ours.levels());
    let zs = |levels: &[(f64, Vec<Piece>)]| levels.iter().map(|(z, _)| *z).collect::<Vec<_>>();
    let (gz, oz) = (zs(&gl), zs(&ol));
    if gz.len() != oz.len()
        || gz
            .iter()
            .zip(&oz)
            .any(|(g, o)| !approx_eq(*g, *o, TOLERANCE_MM))
    {
        return Err(format!(
            "cutting levels differ: golden Z {gz:?}, DreamCarve Z {oz:?}"
        ));
    }
    for ((z, g), (_, o)) in gl.iter().zip(&ol) {
        same_moves(g, o, rules).map_err(|e| format!("Z {z}: {e}"))?;
    }
    Ok(())
}

/// The moves of `pieces` that are compared point by point, as the rules
/// say: not links, not entry moves, and with [`Difference::RasterLineOrder`]
/// of the open moves only the raster lines.
fn subjects<'a>(pieces: &'a [Piece], rules: &Rules) -> Vec<&'a Piece> {
    pieces
        .iter()
        .filter(|p| p.role != Role::Link && !p.entry)
        .filter(|p| rules.raster.is_none() || p.role == Role::Loop || rules.is_raster_line(p))
        .collect()
}

/// `g` and `o` are the same set of moves, as the module docs say.
fn same_moves(g: &[Piece], o: &[Piece], rules: &Rules) -> Result<(), String> {
    let (gs, os) = (subjects(g, rules), subjects(o, rules));
    within(&gs, o, rules).map_err(|e| format!("golden {e}"))?;
    within(&os, g, rules).map_err(|e| format!("DreamCarve {e}"))?;

    // The laps: with the entry left out, only loops at least the tool's
    // circumference long (a helix's flat turn is shorter).
    let lap = TAU * rules.tool_radius;
    let measured = |pieces: &[&Piece]| -> Vec<Piece> {
        let mut out: Vec<Piece> = Vec::new();
        for piece in pieces {
            let keep = if rules.entry {
                piece.role == Role::Loop && group_length(pieces, piece.group) >= lap
            } else {
                true
            };
            if keep {
                out.push(**piece);
            }
        }
        out
    };
    let (gm, om) = (measured(&gs), measured(&os));
    let (gl, ol) = (total_length(&gm), total_length(&om));
    // Two closed convex loops each within the allowance of the other
    // differ in length by at most 2π times it (Cauchy's perimeter formula
    // for parallel curves), and each open end of a chain of moves can stop
    // up to the allowance short of its counterpart.
    let (g_loops, g_ends) = chains(&gm);
    let (o_loops, o_ends) = chains(&om);
    let slack = ALLOWED_MM * (TAU * f64::from(g_loops.max(o_loops)) + f64::from(g_ends + o_ends));
    if (gl - ol).abs() > slack {
        return Err(format!(
            "cut length {gl:.4} mm in golden, {ol:.4} mm in DreamCarve (allowed {slack} mm apart)"
        ));
    }

    // Links follow the loops' start points; only their lengths are
    // compared.
    if !rules.entry && rules.raster.is_none() {
        let links = |pieces: &[Piece]| {
            let mut lengths: Vec<f64> = pieces
                .iter()
                .filter(|p| p.role == Role::Link)
                .map(Piece::length)
                .collect();
            lengths.sort_by(f64::total_cmp);
            lengths
        };
        let (gk, ok) = (links(g), links(o));
        if gk.len() != ok.len()
            || gk
                .iter()
                .zip(&ok)
                .any(|(a, b)| !approx_eq(*a, *b, 2.0 * ALLOWED_MM))
        {
            return Err(format!(
                "links between loops are {gk:.3?} mm long in golden, {ok:.3?} mm in DreamCarve"
            ));
        }
    }
    Ok(())
}

/// The length of loop `group` among `pieces`.
fn group_length(pieces: &[&Piece], group: usize) -> f64 {
    pieces
        .iter()
        .filter(|p| p.role == Role::Loop && p.group == group)
        .map(|p| p.length())
        .sum()
}

/// Every sampled point of `a` lies within [`ALLOWED_MM`] of `b`, travels
/// the same way as a move of `b` there, and is cut at the same feed. With
/// [`Difference::RasterLineOrder`] a raster line may travel either way.
fn within(a: &[&Piece], b: &[Piece], rules: &Rules) -> Result<(), String> {
    for piece in a {
        let directed = !rules.is_raster_line(piece);
        for (p, tangent) in piece.samples() {
            let near: Vec<(&Piece, (f64, Vec2, bool))> = b
                .iter()
                .map(|q| (q, q.nearest(p)))
                .filter(|(_, (d, _, _))| *d <= ALLOWED_MM)
                .collect();
            if near.is_empty() {
                let (nearest, distance) = b
                    .iter()
                    .map(|q| (q, q.nearest(p).0))
                    .min_by(|x, y| x.1.total_cmp(&y.1))
                    .ok_or("level has no moves in the other program")?;
                return Err(format!(
                    "line {} point ({:.4}, {:.4}) is {distance:.4} mm from the nearest move (line {})",
                    piece.line, p.x, p.y, nearest.line
                ));
            }
            // Out-and-back moves (a relief notch) lie on each other: any
            // move here that travels the same way will do.
            let along: Vec<&Piece> = near
                .iter()
                .filter(|(_, (_, t, at_end))| !directed || *at_end || tangent.dot(*t) > 0.0)
                .map(|(q, _)| *q)
                .collect();
            let Some(first) = along.first() else {
                return Err(format!(
                    "line {} at ({:.4}, {:.4}) travels against line {}",
                    piece.line, p.x, p.y, near[0].0.line
                ));
            };
            let feed_free = rules.is_lead(piece) || along.iter().any(|q| rules.is_lead(q));
            if !feed_free && !along.iter().any(|q| approx_eq(piece.feed, q.feed, SAME_MM)) {
                return Err(format!(
                    "line {} feeds at {}, line {} at {}",
                    piece.line, piece.feed, first.line, first.feed
                ));
            }
        }
    }
    Ok(())
}

fn total_length(pieces: &[Piece]) -> f64 {
    pieces.iter().map(Piece::length).sum()
}

/// The chains of consecutive, connected moves in `pieces`: how many close
/// on themselves, and how many open ends the rest have.
fn chains(pieces: &[Piece]) -> (u32, u32) {
    let mut chains: Vec<(Point, Point)> = Vec::new();
    for piece in pieces {
        match chains.last_mut() {
            Some((_, end)) if end.distance_to(piece.from) <= SAME_MM => *end = piece.to,
            _ => chains.push((piece.from, piece.to)),
        }
    }
    let open = chains
        .iter()
        .filter(|(start, end)| start.distance_to(*end) > SAME_MM)
        .count();
    let count = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
    (count(chains.len() - open), count(open * 2))
}

/// The sloped moves of both programs are the same set, in three
/// dimensions, at the same feeds.
fn compare_slopes(golden: &[Piece], ours: &[Piece]) -> Result<(), String> {
    for (who, a, b) in [("golden", golden, ours), ("DreamCarve", ours, golden)] {
        for piece in a {
            for t in piece.sample_ts() {
                let p = piece.at3(t);
                let Some((nearest, distance)) = b
                    .iter()
                    .map(|q| (q, q.distance3(p)))
                    .min_by(|x, y| x.1.total_cmp(&y.1))
                else {
                    return Err(format!(
                        "{who} line {} is a sloped move; the other program has none",
                        piece.line
                    ));
                };
                if distance > ALLOWED_MM {
                    return Err(format!(
                        "{who} line {} point ({:.4}, {:.4}, {:.4}) is {distance:.4} mm from the nearest sloped move (line {})",
                        piece.line, p.x, p.y, p.z, nearest.line
                    ));
                }
                if !approx_eq(piece.feed, nearest.feed, SAME_MM) {
                    return Err(format!(
                        "{who} line {} feeds at {}, line {} at {}",
                        piece.line, piece.feed, nearest.line, nearest.feed
                    ));
                }
            }
        }
    }
    Ok(())
}

/// The descents of a program: runs of consecutive sloped moves.
fn descents(cuts: &Cuts) -> Vec<Vec<Piece>> {
    let mut out: Vec<Vec<Piece>> = Vec::new();
    for piece in &cuts.slopes {
        match out.last_mut() {
            Some(descent)
                if descent.last().is_some_and(|last| {
                    last.to.distance_to(piece.from) <= SAME_MM
                        && approx_eq(last.z, piece.z0, SAME_MM)
                        && piece.line == last.line + 1
                }) =>
            {
                descent.push(*piece);
            }
            _ => out.push(vec![*piece]),
        }
    }
    out
}

/// The entry of the case's Profile or Pocket, with its tool diameter.
fn entry_of(project: &Project) -> Option<(Entry, f64)> {
    project.operations.iter().find_map(|op| {
        let entry = match op {
            Operation::Profile(p) => p.entry.clone(),
            Operation::Pocket(p) => p.entry.clone(),
            _ => return None,
        };
        Some((entry, tool_of(project, op).diameter_mm))
    })
}

/// Checks every descent of `cuts` against the operation's entry: it goes
/// down, no steeper than the entry angle; a helix turns on a circle of the
/// entry's diameter; DreamCarve's ramp (`ours`) starts on the path it
/// enters (#165).
fn check_descents(project: &Project, cuts: &Cuts, ours: bool) -> Result<(), String> {
    let (entry, diameter) = entry_of(project).ok_or("the case has no entry to check")?;
    let angle_deg = match entry {
        Entry::Plunge => return Err("the case's entry is a plunge".into()),
        Entry::Ramp { angle_deg } | Entry::Helix { angle_deg, .. } => angle_deg,
    };
    let descents = descents(cuts);
    if descents.is_empty() {
        return Err("no ramp or helix".into());
    }
    let levels = cuts.levels();
    for descent in &descents {
        let first = descent.first().ok_or("an empty descent")?;
        let last = descent.last().ok_or("an empty descent")?;
        let drop = first.z0 - last.z;
        let mut run: f64 = descent.iter().map(Piece::length).sum();
        if let Entry::Helix {
            diameter_fraction, ..
        } = entry
        {
            let radius = diameter_fraction * diameter / 2.0;
            let points: Vec<Point> = descent.iter().map(|p| p.from).collect();
            let centre = circle_centre(&points).ok_or("a helix with no turn")?;
            for p in &points {
                let off = (centre.distance_to(*p) - radius).abs();
                if off > ALLOWED_MM + ROUNDING_MM {
                    return Err(format!(
                        "line {}: the helix point ({:.4}, {:.4}) is {off:.4} mm off its {radius} mm radius",
                        first.line, p.x, p.y
                    ));
                }
            }
            // The helix's run is along its circle: a program that writes
            // it as chords travels a little less in XY than it computed.
            run = descent
                .iter()
                .map(|p| {
                    let turn = (angle_of(centre, p.to) - angle_of(centre, p.from) + PI)
                        .rem_euclid(TAU)
                        - PI;
                    turn.abs() * radius
                })
                .sum();
        }
        // The descent's two end Zs are rounded, and each move's XY ends.
        let slope = angle_deg.to_radians().tan();
        let moves = f64::from(u32::try_from(descent.len()).unwrap_or(u32::MAX));
        let slack = (2.0 * Z_ROUNDING_MM + 2.0 * ROUNDING_MM * moves * slope) / run;
        if drop <= 0.0 || (drop / run - slope).abs() > slack {
            return Err(format!(
                "line {}: the descent drops {drop:.4} mm over {run:.4} mm, not at {angle_deg}°",
                first.line
            ));
        }
        if ours && matches!(entry, Entry::Ramp { .. }) {
            let on_path = levels
                .iter()
                .find(|(z, _)| approx_eq(*z, last.z, SAME_MM))
                .is_some_and(|(_, pieces)| {
                    pieces
                        .iter()
                        .any(|p| !p.entry && p.nearest(first.from).0 <= ALLOWED_MM)
                });
            if !on_path {
                return Err(format!(
                    "line {}: the ramp starts at ({:.4}, {:.4}), not on the path it enters",
                    first.line, first.from.x, first.from.y
                ));
            }
        }
    }
    Ok(())
}

/// The centre of the circle through the first, a middle and the last of
/// `points`, which lie on a helix turn.
fn circle_centre(points: &[Point]) -> Option<Point> {
    let a = *points.first()?;
    let b = *points.get(points.len() / 3)?;
    let c = *points.get(2 * points.len() / 3)?;
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() < 1e-9 {
        return None;
    }
    let sq = |p: Point| p.x * p.x + p.y * p.y;
    Some(Point::new(
        (sq(a) * (b.y - c.y) + sq(b) * (c.y - a.y) + sq(c) * (a.y - b.y)) / d,
        (sq(a) * (c.x - b.x) + sq(b) * (a.x - c.x) + sq(c) * (b.x - a.x)) / d,
    ))
}

/// Whether a descent turns counter-clockwise, by the sign of the area it
/// sweeps about its first point; `None` for a straight one (a ramp).
fn turns_ccw(descent: &[Piece]) -> Option<bool> {
    let o = descent.first()?.from;
    let area: f64 = descent
        .iter()
        .map(|p| {
            Vec2::new(p.from.x - o.x, p.from.y - o.y).cross(Vec2::new(p.to.x - o.x, p.to.y - o.y))
        })
        .sum();
    (area.abs() > 1e-6).then_some(area > 0.0)
}

/// A helix entry turns the same way in both programs.
fn compare_helix_sense(golden: &Cuts, ours: &Cuts) -> Result<(), String> {
    let senses = |cuts: &Cuts| {
        let mut s: Vec<bool> = descents(cuts).iter().filter_map(|d| turns_ccw(d)).collect();
        s.dedup();
        s
    };
    let (g, o) = (senses(golden), senses(ours));
    if g != o {
        return Err(format!(
            "golden descents turn counter-clockwise: {g:?}, DreamCarve's: {o:?}"
        ));
    }
    Ok(())
}

/// With [`Difference::LeadArcsAtCuttingFeed`]: DreamCarve's lead arcs run
/// at the tool's cutting feed, PureCutCNC's below it.
fn check_lead_feeds(
    golden: &Cuts,
    ours: &Cuts,
    radius: f64,
    limits: &Limits,
) -> Result<(), String> {
    let feed = *limits.rates.first().ok_or("the case has a tool")?;
    let leads = |cuts: &Cuts| -> Vec<Piece> {
        cuts.runs
            .iter()
            .flat_map(|r| r.pieces.iter().copied())
            .filter(|p| {
                p.role == Role::Open && p.arc.is_some_and(|a| approx_eq(a.r0, radius, ALLOWED_MM))
            })
            .collect()
    };
    for (who, cuts, ok) in [
        (
            "golden",
            golden,
            &(|f: f64| f < feed - SAME_MM) as &dyn Fn(f64) -> bool,
        ),
        ("DreamCarve", ours, &|f: f64| approx_eq(f, feed, SAME_MM)),
    ] {
        let arcs = leads(cuts);
        if arcs.is_empty() {
            return Err(format!("{who} program has no {radius} mm lead arcs"));
        }
        if let Some(bad) = arcs.iter().find(|p| !ok(p.feed)) {
            return Err(format!(
                "{who} line {}: lead arc at {} mm/min (cutting feed {feed})",
                bad.line, bad.feed
            ));
        }
    }
    Ok(())
}

/// Feeds of the vertical moves: the same feeds appear in both programs.
/// With [`Difference::RectTabStepsAtPlungeRate`], tab steps are taken out
/// of that comparison and must instead run at the cutting feed in the
/// golden program and at the plunge rate in DreamCarve's. With
/// [`Difference::NotchRisesFromLapEnd`], DreamCarve's lifts are taken out
/// and must run at the plunge rate.
fn compare_verticals(
    golden: &Cuts,
    ours: &Cuts,
    limits: &Limits,
    tab_top_z: Option<f64>,
    differences: &[Difference],
) -> Result<(), String> {
    let rect_steps = differences.contains(&Difference::RectTabStepsAtPlungeRate);
    // A tab step joins the tab top and a level below it.
    let is_tab_step = |v: &Vertical| {
        rect_steps
            && tab_top_z.is_some_and(|top| {
                let (high, low) = if v.from_z > v.to_z {
                    (v.from_z, v.to_z)
                } else {
                    (v.to_z, v.from_z)
                };
                approx_eq(high, top, SAME_MM) && low < top - SAME_MM
            })
    };
    if rect_steps {
        // The tab cases have one tool: its feed, then its plunge rate.
        let [feed, plunge] = limits.rates[..] else {
            return Err("a tab case has one tool".into());
        };
        for (who, cuts, step_feed) in [("golden", golden, feed), ("DreamCarve", ours, plunge)] {
            let steps: Vec<&Vertical> = cuts.verticals.iter().filter(|v| is_tab_step(v)).collect();
            if steps.is_empty() {
                return Err(format!("{who} program has no tab steps"));
            }
            for v in steps {
                if !approx_eq(v.feed, step_feed, SAME_MM) {
                    return Err(format!(
                        "{who} line {}: tab step at ({:.3}, {:.3}) feeds at {} mm/min, expected {step_feed}",
                        v.line, v.at.x, v.at.y, v.feed
                    ));
                }
            }
        }
    }
    let notch_rises = differences.contains(&Difference::NotchRisesFromLapEnd);
    if notch_rises {
        // The notch cases have one tool: its feed, then its plunge rate.
        let [_, plunge] = limits.rates[..] else {
            return Err("a notch case has one tool".into());
        };
        let rises: Vec<&Vertical> = ours
            .verticals
            .iter()
            .filter(|v| v.to_z > v.from_z)
            .collect();
        if rises.is_empty() {
            return Err("DreamCarve program has no notch rises".into());
        }
        if let Some(v) = rises.iter().find(|v| !approx_eq(v.feed, plunge, SAME_MM)) {
            return Err(format!(
                "DreamCarve line {}: rise at ({:.3}, {:.3}) feeds at {} mm/min, expected {plunge}",
                v.line, v.at.x, v.at.y, v.feed
            ));
        }
    }
    let feeds = |cuts: &Cuts, down: bool| {
        let skip_lifts = notch_rises && std::ptr::eq(cuts, ours) && !down;
        let mut feeds: Vec<f64> = cuts
            .verticals
            .iter()
            .filter(|_| !skip_lifts)
            .filter(|v| !is_tab_step(v) && (v.to_z < v.from_z) == down)
            .map(|v| v.feed)
            .collect();
        feeds.sort_by(f64::total_cmp);
        feeds.dedup_by(|a, b| approx_eq(*a, *b, SAME_MM));
        feeds
    };
    for (down, what) in [(true, "descents"), (false, "lifts")] {
        let (g, o) = (feeds(golden, down), feeds(ours, down));
        if g.len() != o.len() || g.iter().zip(&o).any(|(a, b)| !approx_eq(*a, *b, SAME_MM)) {
            return Err(format!(
                "{what} feed at {g:?} mm/min in golden, {o:?} in DreamCarve"
            ));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Holes

/// A drilled hole: where, and the Z each plunge ends at, in order.
#[derive(Debug)]
struct Hole {
    at: Point,
    line: usize,
    plunges: Vec<f64>,
}

impl Hole {
    fn bottom(&self) -> f64 {
        self.plunges.iter().copied().fold(f64::INFINITY, f64::min)
    }
}

/// The holes of a program in drilling order: consecutive downward vertical
/// cuts at one XY.
fn holes(cuts: &Cuts) -> Vec<Hole> {
    let mut holes: Vec<Hole> = Vec::new();
    for v in cuts.verticals.iter().filter(|v| v.to_z < v.from_z) {
        match holes.last_mut() {
            Some(hole) if hole.at.distance_to(v.at) <= SAME_MM => hole.plunges.push(v.to_z),
            _ => holes.push(Hole {
                at: v.at,
                line: v.line,
                plunges: vec![v.to_z],
            }),
        }
    }
    holes
}

/// The same holes, in the same order, at the same depths. Without
/// [`Difference::PecksFromRetractPlane`] each hole's plunges end at the
/// same Zs too; with it, each program's plunges follow its own rule.
fn compare_holes(
    project: &Project,
    golden: &Cuts,
    ours: &Cuts,
    differences: &[Difference],
) -> Result<(), String> {
    let (g, o) = (holes(golden), holes(ours));
    if g.len() != o.len() {
        return Err(format!(
            "{} holes in golden, {} in DreamCarve",
            g.len(),
            o.len()
        ));
    }
    let pecks_differ = differences.contains(&Difference::PecksFromRetractPlane);
    for (k, (gh, oh)) in g.iter().zip(&o).enumerate() {
        if gh.at.distance_to(oh.at) > ALLOWED_MM {
            return Err(format!(
                "hole {k}: golden line {} at ({:.3}, {:.3}), DreamCarve line {} at ({:.3}, {:.3})",
                gh.line, gh.at.x, gh.at.y, oh.line, oh.at.x, oh.at.y
            ));
        }
        if !approx_eq(gh.bottom(), oh.bottom(), SAME_MM) {
            return Err(format!(
                "hole {k} at ({:.3}, {:.3}): golden bottom {}, DreamCarve {}",
                gh.at.x,
                gh.at.y,
                gh.bottom(),
                oh.bottom()
            ));
        }
        if !pecks_differ {
            let same = gh.plunges.len() == oh.plunges.len()
                && gh
                    .plunges
                    .iter()
                    .zip(&oh.plunges)
                    .all(|(a, b)| approx_eq(*a, *b, SAME_MM));
            if !same {
                return Err(format!(
                    "hole {k} at ({:.3}, {:.3}): plunges end at {:?} in golden, {:?} in DreamCarve",
                    gh.at.x, gh.at.y, gh.plunges, oh.plunges
                ));
            }
        }
    }
    if pecks_differ {
        let Some((peck, retract_z)) = project.operations.iter().find_map(|op| match op {
            Operation::Drill(d) => match d.cycle {
                DrillCycle::Peck {
                    peck_depth_mm: Some(peck),
                    ..
                } => Some((peck, d.retract_mm.min(d.safe_z_mm))),
                _ => None,
            },
            _ => None,
        }) else {
            return Err("the case has no peck drill with a peck depth".into());
        };
        // Machine Z zero is the stock top: PureCutCNC counts from it,
        // DreamCarve from the retract plane (#153).
        for (who, holes, from) in [("golden", &g, 0.0), ("DreamCarve", &o, retract_z)] {
            for hole in holes {
                let expected = pecks(from, peck, hole.bottom());
                let same = expected.len() == hole.plunges.len()
                    && expected
                        .iter()
                        .zip(&hole.plunges)
                        .all(|(a, b)| approx_eq(*a, *b, Z_ROUNDING_MM + SAME_MM));
                if !same {
                    return Err(format!(
                        "{who} hole at ({:.3}, {:.3}): pecks end at {:?}, expected {expected:?} counted from Z {from}",
                        hole.at.x, hole.at.y, hole.plunges
                    ));
                }
            }
        }
    }
    Ok(())
}

/// Where the pecks of a hole end, counted from `from`: `max(bottom, from −
/// k·peck)` for k = 1, 2, … until the bottom.
fn pecks(from: f64, peck: f64, bottom: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut z = from;
    while z > bottom + SAME_MM {
        z = (z - peck).max(bottom);
        out.push(z);
    }
    out
}

// ---------------------------------------------------------------------------
// Order

/// A short description of each run, for messages.
fn describe(runs: &[Run]) -> Vec<String> {
    runs.iter()
        .map(|r| {
            let first = r.pieces.first().map_or(0, |p| p.line);
            format!("Z {} from line {first}", r.z)
        })
        .collect()
}

/// The runs of both programs, one by one in program order: the same
/// number, each at the same Z with the same moves.
fn compare_runs(golden: &[Run], ours: &[Run], rules: &Rules) -> Result<(), String> {
    if golden.len() != ours.len() {
        return Err(format!(
            "{} runs of cuts in golden, {} in DreamCarve: golden {:?}, DreamCarve {:?}",
            golden.len(),
            ours.len(),
            describe(golden),
            describe(ours)
        ));
    }
    for (k, (g, o)) in golden.iter().zip(ours).enumerate() {
        if !approx_eq(g.z, o.z, TOLERANCE_MM) {
            return Err(format!(
                "run {k}: golden at Z {}, DreamCarve at Z {}",
                g.z, o.z
            ));
        }
        same_moves(&g.pieces, &o.pieces, rules)
            .map_err(|e| format!("run {k} at Z {}: {e}", g.z))?;
    }
    Ok(())
}

/// The runs grouped into layers: consecutive runs of one operation at one
/// Z, which under `pass_first` are one pass across every shape.
fn layers(runs: &[Run]) -> Vec<Vec<&Run>> {
    let mut out: Vec<Vec<&Run>> = Vec::new();
    for run in runs {
        match out.last_mut() {
            Some(layer)
                if layer
                    .last()
                    .is_some_and(|l| l.op == run.op && approx_eq(l.z, run.z, SAME_MM)) =>
            {
                layer.push(run);
            }
            _ => out.push(vec![run]),
        }
    }
    out
}

/// The layers of both programs, one by one in program order: the same
/// number, each at the same Z with the same runs, as a set.
fn compare_layers(golden: &[Run], ours: &[Run], rules: &Rules) -> Result<(), String> {
    let (g, o) = (layers(golden), layers(ours));
    let zs = |layers: &[Vec<&Run>]| {
        layers
            .iter()
            .map(|l| {
                (
                    l.first().map_or(0, |r| r.op),
                    l.first().map_or(0.0, |r| r.z),
                    l.len(),
                )
            })
            .collect::<Vec<_>>()
    };
    let (gz, oz) = (zs(&g), zs(&o));
    let same = gz.len() == oz.len()
        && gz
            .iter()
            .zip(&oz)
            .all(|(a, b)| a.0 == b.0 && approx_eq(a.1, b.1, TOLERANCE_MM) && a.2 == b.2);
    if !same {
        return Err(format!(
            "passes (operation, Z, runs) differ: golden {gz:?}, DreamCarve {oz:?}"
        ));
    }
    for (k, (gl, ol)) in g.iter().zip(&o).enumerate() {
        let pieces = |layer: &[&Run]| -> Vec<Piece> {
            layer
                .iter()
                .flat_map(|r| r.pieces.iter().copied())
                .collect()
        };
        same_moves(&pieces(gl), &pieces(ol), rules)
            .map_err(|e| format!("pass {k} at Z {}: {e}", gz[k].1))?;
    }
    Ok(())
}

/// D5 within each of DreamCarve's layers: after the first run of an
/// operation, each run starts nearer the previous run's end than any run
/// of its layer still to come.
fn check_nearest_first(runs: &[Run]) -> Result<(), String> {
    let start = |r: &Run| r.pieces.first().map(|p| p.from);
    let end = |r: &Run| r.pieces.last().map(|p| p.to);
    let layers = layers(runs);
    let mut previous: Option<(usize, Point)> = None;
    for layer in &layers {
        for (k, run) in layer.iter().enumerate() {
            if let Some((op, from)) = previous
                && op == run.op
            {
                let here = start(run).ok_or("an empty run")?;
                let d = from.distance_to(here);
                for later in &layer[k + 1..] {
                    let there = start(later).ok_or("an empty run")?;
                    if from.distance_to(there) < d - SAME_MM {
                        return Err(format!(
                            "DreamCarve cuts the run at line {} ({d:.3} mm away) before the one at line {} ({:.3} mm away)",
                            run.pieces[0].line,
                            later.pieces[0].line,
                            from.distance_to(there)
                        ));
                    }
                }
            }
            previous = end(run).map(|p| (run.op, p));
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Rest machining

/// The rest case's pocket as a world rectangle (min, max) and the depth
/// both operations cut to, as machine Z.
fn rest_pocket(project: &Project) -> Result<(Point, Point, f64), String> {
    let Some(Operation::Pocket(rest)) = project.operations.get(1) else {
        return Err("the second operation is not a Pocket".into());
    };
    let [target] = rest.targets.as_slice() else {
        return Err("the rest pocket has one target".into());
    };
    let instance = project
        .instances
        .iter()
        .find(|i| &i.id == target)
        .ok_or("the rest target is an instance")?;
    let definition = project
        .definitions
        .get(&instance.definition)
        .ok_or("the instance's definition exists")?;
    let [outline] = definition.profiles.as_slice() else {
        return Err("the pocket has one outline".into());
    };
    if outline.vertices().len() != 4 || outline.vertices().iter().any(|v| v.bulge.abs() > 0.0) {
        return Err("the rest pocket is a rectangle".into());
    }
    let points: Vec<Point> = outline
        .vertices()
        .iter()
        .map(|v| instance.transform.apply(Point::new(v.x, v.y)))
        .collect();
    let min = Point::new(
        points.iter().map(|p| p.x).fold(f64::INFINITY, f64::min),
        points.iter().map(|p| p.y).fold(f64::INFINITY, f64::min),
    );
    let max = Point::new(
        points.iter().map(|p| p.x).fold(f64::NEG_INFINITY, f64::max),
        points.iter().map(|p| p.y).fold(f64::NEG_INFINITY, f64::max),
    );
    let depth = rest.depth_mm.ok_or("the rest pocket has a depth")?;
    Ok((min, max, -depth))
}

/// At the pocket's final depth, the program's cuts with both tools sweep
/// every point of the pocket that the smaller tool can reach: a point is
/// reachable when it is within the tool radius of the pocket inset by that
/// radius. Points are taken every 0.25 mm.
fn check_rest_coverage(project: &Project, cuts: &Cuts) -> Result<(), String> {
    let (min, max, bottom) = rest_pocket(project)?;
    let radius = |op: usize| {
        project
            .operations
            .get(op)
            .map_or(0.0, |o| tool_of(project, o).diameter_mm / 2.0)
    };
    let small = radius(0).min(radius(1));
    let levels = cuts.levels();
    let pieces: Vec<&Piece> = levels
        .iter()
        .filter(|(z, _)| approx_eq(*z, bottom, TOLERANCE_MM))
        .flat_map(|(_, pieces)| pieces)
        .collect();
    if pieces.is_empty() {
        return Err(format!("no cuts at the final depth Z {bottom}"));
    }
    let step = 0.25;
    let mut y = min.y;
    while y <= max.y {
        let mut x = min.x;
        while x <= max.x {
            let p = Point::new(x, y);
            // The nearest point the small tool's centre can reach.
            let centre = Point::new(
                x.clamp(min.x + small, max.x - small),
                y.clamp(min.y + small, max.y - small),
            );
            let reachable = p.distance_to(centre) <= small - ALLOWED_MM;
            if reachable
                && !pieces
                    .iter()
                    .any(|q| q.nearest(p).0 <= radius(q.op) + ALLOWED_MM)
            {
                return Err(format!(
                    "({x:.2}, {y:.2}) at Z {bottom} is reachable by the {} mm tool but no cut sweeps it",
                    2.0 * small
                ));
            }
            x += step;
        }
        y += step;
    }
    Ok(())
}

/// DreamCarve's rest operation cuts at every level its rough did;
/// PureCutCNC's finish pass at the final depth alone.
fn check_rest_levels(golden: &Cuts, ours: &Cuts) -> Result<(), String> {
    let levels = |cuts: &Cuts, op: usize| {
        cuts.levels()
            .iter()
            .filter(|(_, pieces)| pieces.iter().any(|p| p.op == op))
            .map(|(z, _)| *z)
            .collect::<Vec<_>>()
    };
    let (rough, rest) = (levels(ours, 0), levels(ours, 1));
    if rough.len() != rest.len()
        || rough
            .iter()
            .zip(&rest)
            .any(|(a, b)| !approx_eq(*a, *b, SAME_MM))
    {
        return Err(format!(
            "DreamCarve's rough cuts at Z {rough:?}, its rest at Z {rest:?}"
        ));
    }
    let (rough, finish) = (levels(golden, 0), levels(golden, 1));
    match (rough.last(), finish.as_slice()) {
        (Some(bottom), [only]) if approx_eq(*bottom, *only, SAME_MM) => Ok(()),
        _ => Err(format!(
            "golden's rough cuts at Z {rough:?}, its finish at Z {finish:?}; expected the final depth alone"
        )),
    }
}
