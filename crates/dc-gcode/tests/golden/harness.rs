// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The golden parity harness (`docs/specs/phase-1/functional.md`,
//! acceptance test 14; `docs/rust-guidelines.md`, "Tests").
//!
//! [`check`] generates and emits DreamCarve's program for a case, reads it
//! and the captured PureCutCNC program back with [`parse_program`], and
//! compares the two in the machine frame they share:
//!
//! 1. **Safety (S1–S3)** on both programs: no Z below the stock bottom
//!    minus the allowance, rapids only at or above safe Z, and every cut at
//!    a positive feed that is one of the tool's two rates.
//! 2. **Levels.** The Z values of the horizontal cutting moves are equal.
//! 3. **Moves per level.** Each level is a set of line and arc moves.
//!    Every point of every move, sampled every [`SAMPLE_STEP_MM`], lies
//!    within [`ALLOWED_MM`] (the [`TOLERANCE_MM`] of acceptance test 14 plus
//!    each program's rounding) of the other program's moves at that level,
//!    in both directions; it travels the same way as the nearest move there;
//!    and it is cut at the same feed. Comparing sets is what aligns the start
//!    points: a loop entered somewhere else, or contours cut in another
//!    order, still match. The total cut length per level must agree too, so
//!    a lap cut twice does not pass.
//! 4. **Vertical moves.** The feeds of the moves that change only Z are
//!    equal, except where a [`Difference`] says otherwise.
//!
//! On failure the message names the check and prints a unified diff of the
//! two programs, normalised as `docs/rust-guidelines.md` requires.

use std::f64::consts::TAU;
use std::fmt::Write as _;
use std::path::PathBuf;

use dc_cam::generate_profile;
use dc_cam::test_support::assert_safe;
use dc_gcode::emit;
use dc_gcode::parse::{Block, MotionKind, parse_program};
use dc_geometry::{Point, Vec2, approx_eq};
use dc_project::{Operation, ProfileOp, Project, Tool};

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
}

/// An intentional difference from PureCutCNC that the harness checks for
/// instead of comparing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difference {
    /// Rect tab steps are vertical moves at the plunge rate in DreamCarve
    /// and at the cutting feed in PureCutCNC. A tab step is a vertical move
    /// between the tab top and a lower level.
    RectTabStepsAtPlungeRate,
}

/// Compares DreamCarve's program for `case` with the golden program.
pub fn check(case: &Case) {
    let project = &case.project;
    let (op, tool) = profile_op(project);
    let golden_text = std::fs::read_to_string(golden_dir(case.name).join("program.nc"))
        .unwrap_or_else(|e| panic!("{}: cannot read the golden program: {e}", case.name));
    let ours_text = emit_program(project);

    let golden = normalise(&golden_text);
    let ours = normalise(&ours_text);
    let fail = |what: &str, message: String| -> ! {
        panic!(
            "{}: {what}: {message}\n\n--- golden (PureCutCNC)\n+++ DreamCarve\n{}",
            case.name,
            unified_diff(&golden, &ours, 3)
        )
    };

    let limits = Limits::of(project, op, tool);
    let golden_moves = read_moves(&golden_text);
    let ours_moves = read_moves(&ours_text);
    for (who, moves) in [("golden", &golden_moves), ("DreamCarve", &ours_moves)] {
        if let Err(e) = limits.check_safety(moves) {
            fail(&format!("{who} program is unsafe"), e);
        }
    }

    let golden_cuts = Cuts::of(&golden_moves).unwrap_or_else(|e| fail("golden program", e));
    let ours_cuts = Cuts::of(&ours_moves).unwrap_or_else(|e| fail("DreamCarve program", e));
    if let Err(e) = compare_levels(&golden_cuts, &ours_cuts) {
        fail("cutting moves differ", e);
    }
    let tab_top_z = project_tab_top_z(project, op);
    if let Err(e) = compare_verticals(
        &golden_cuts,
        &ours_cuts,
        &limits,
        tab_top_z,
        &case.differences,
    ) {
        fail("vertical moves differ", e);
    }
}

/// The case's one Profile operation and its tool.
fn profile_op(project: &Project) -> (&ProfileOp, &Tool) {
    let [Operation::Profile(op)] = project.operations.as_slice() else {
        panic!("a golden case has exactly one Profile operation");
    };
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .expect("the operation's tool is in the project");
    (op, tool)
}

fn golden_dir(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(name)
}

/// Generates, checks and emits the project's program.
fn emit_program(project: &Project) -> String {
    let (op, tool) = profile_op(project);
    let (toolpath, warnings) = generate_profile(project, op).expect("the profile generates");
    assert!(
        warnings.is_empty(),
        "a golden case generates without warnings: {warnings:?}"
    );
    assert_safe(&toolpath, project, &Operation::Profile(op.clone()));
    let (program, warnings) = emit(
        project,
        &[(Operation::Profile(op.clone()), tool.clone(), toolpath).into()],
        DATE,
    )
    .expect("the program emits");
    assert!(
        warnings.is_empty(),
        "a golden case emits without warnings: {warnings:?}"
    );
    program.text
}

/// The stock thickness; every case sets it.
fn thickness(project: &Project) -> f64 {
    project
        .stock
        .thickness_mm
        .expect("every case sets the stock thickness")
}

/// The machine Z of the tab top, if the operation has tabs. Machine Z zero
/// is the stock top in every case.
fn project_tab_top_z(project: &Project, op: &ProfileOp) -> Option<f64> {
    op.tabs
        .as_ref()
        .map(|tabs| tabs.height_mm - thickness(project))
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
}

/// One move of a program, machine frame, mm.
#[derive(Debug, Clone, Copy)]
struct Motion {
    line: usize,
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

/// The moves of a program, which must parse and be in millimetres.
fn read_moves(text: &str) -> Vec<Motion> {
    let blocks: Vec<Block> = parse_program(text).expect("the program parses");
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
    safe_z: f64,
    feed: f64,
    plunge: f64,
}

impl Limits {
    fn of(project: &Project, op: &ProfileOp, tool: &Tool) -> Self {
        // Machine Z zero is the stock top in every case.
        Self {
            floor_z: -thickness(project) - op.through_cut_allowance_mm,
            safe_z: op.safe_z_mm,
            feed: tool.feed_mm_min.expect("the tool has a feed"),
            plunge: tool.plunge_mm_min.expect("the tool has a plunge rate"),
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
                // S2.
                let low = |z: Option<f64>| z.is_none_or(|z| z < self.safe_z - SAME_MM);
                if low(mv.to_z) || (mv.moves_xy && low(mv.from_z)) {
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
                if !(approx_eq(feed, self.feed, SAME_MM) || approx_eq(feed, self.plunge, SAME_MM)) {
                    return Err(format!(
                        "line {line}: feed {feed} is neither the tool's feed nor its plunge rate"
                    ));
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Geometry

/// A horizontal line or arc move, machine frame, mm.
#[derive(Debug, Clone, Copy)]
struct Piece {
    line: usize,
    from: Point,
    to: Point,
    z: f64,
    /// `Some` for an arc.
    arc: Option<ArcShape>,
    feed: f64,
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
            from: from.xy(),
            to: to.xy(),
            z: to.z,
            arc,
            feed,
        })
    }

    fn length(&self) -> f64 {
        match self.arc {
            None => self.from.distance_to(self.to),
            Some(a) => (a.r0 + a.r1) / 2.0 * a.sweep.abs(),
        }
    }

    /// The point and unit tangent at fraction `t` of the move.
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

    /// The point of this move nearest `p`: its distance, the move's
    /// tangent there, and whether it is one of the move's ends.
    fn nearest(&self, p: Point) -> (f64, Vec2, bool) {
        let t = match self.arc {
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
        };
        let (q, tangent) = self.at(t);
        let at_end = t <= 1e-9 || t >= 1.0 - 1e-9;
        (q.distance_to(p), tangent, at_end)
    }

    /// Points along the move no more than [`SAMPLE_STEP_MM`] apart, ends
    /// included, with the tangent at each.
    fn samples(&self) -> Vec<(Point, Vec2)> {
        let ratio = self.length() / SAMPLE_STEP_MM;
        let mut count = 1_u32;
        while f64::from(count) < ratio {
            count += 1;
        }
        (0..=count)
            .map(|k| self.at(f64::from(k) / f64::from(count)))
            .collect()
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

/// A program's cutting moves, split into horizontal levels and vertical
/// moves.
struct Cuts {
    /// Levels by Z, highest first; each holds its moves in program order.
    levels: Vec<(f64, Vec<Piece>)>,
    verticals: Vec<Vertical>,
}

impl Cuts {
    fn of(moves: &[Motion]) -> Result<Self, String> {
        let mut levels: Vec<(f64, Vec<Piece>)> = Vec::new();
        let mut verticals = Vec::new();
        for mv in moves.iter().filter(|m| m.kind.is_cut()) {
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
                match levels
                    .iter_mut()
                    .find(|(z, _)| approx_eq(*z, piece.z, SAME_MM))
                {
                    Some((_, pieces)) => pieces.push(piece),
                    None => levels.push((piece.z, vec![piece])),
                }
            } else if still {
                verticals.push(Vertical {
                    line: mv.line,
                    at: to.xy(),
                    from_z: from.z,
                    to_z: to.z,
                    feed,
                });
            } else {
                return Err(format!(
                    "line {}: a move that changes Z and XY together is not compared yet",
                    mv.line
                ));
            }
        }
        levels.sort_by(|a, b| b.0.total_cmp(&a.0));
        Ok(Self { levels, verticals })
    }
}

/// Every level in both programs, with the same moves on each.
fn compare_levels(golden: &Cuts, ours: &Cuts) -> Result<(), String> {
    let zs = |c: &Cuts| c.levels.iter().map(|(z, _)| *z).collect::<Vec<_>>();
    let (gz, oz) = (zs(golden), zs(ours));
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
    for ((z, g), (_, o)) in golden.levels.iter().zip(&ours.levels) {
        within(g, o).map_err(|e| format!("Z {z}: golden {e}"))?;
        within(o, g).map_err(|e| format!("Z {z}: DreamCarve {e}"))?;
        let (gl, ol) = (total_length(g), total_length(o));
        // Two closed convex loops each within the allowance of the other
        // differ in length by at most 2π times it (Cauchy's perimeter
        // formula for parallel curves), and each open end of a chain of
        // moves can stop up to the allowance short of its counterpart.
        let (g_loops, g_ends) = chains(g);
        let (o_loops, o_ends) = chains(o);
        let slack =
            ALLOWED_MM * (TAU * f64::from(g_loops.max(o_loops)) + f64::from(g_ends + o_ends));
        if (gl - ol).abs() > slack {
            return Err(format!(
                "Z {z}: cut length {gl:.4} mm in golden, {ol:.4} mm in DreamCarve (allowed {slack} mm apart)"
            ));
        }
    }
    Ok(())
}

/// Every sampled point of `a` lies within [`TOLERANCE_MM`] of `b`, travels
/// the same way as `b` there, and is cut at the same feed.
fn within(a: &[Piece], b: &[Piece]) -> Result<(), String> {
    for piece in a {
        for (p, tangent) in piece.samples() {
            let (nearest, (distance, b_tangent, at_end)) = b
                .iter()
                .map(|q| (q, q.nearest(p)))
                .min_by(|x, y| x.1.0.total_cmp(&y.1.0))
                .ok_or("level has no moves in the other program")?;
            if distance > ALLOWED_MM {
                return Err(format!(
                    "line {} point ({:.4}, {:.4}) is {distance:.4} mm from the nearest move (line {})",
                    piece.line, p.x, p.y, nearest.line
                ));
            }
            if !at_end && tangent.dot(b_tangent) <= 0.0 {
                return Err(format!(
                    "line {} at ({:.4}, {:.4}) travels against line {}",
                    piece.line, p.x, p.y, nearest.line
                ));
            }
            if !approx_eq(piece.feed, nearest.feed, SAME_MM) {
                return Err(format!(
                    "line {} feeds at {}, line {} at {}",
                    piece.line, piece.feed, nearest.line, nearest.feed
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

/// Feeds of the vertical moves: the same feeds appear in both programs.
/// With [`Difference::RectTabStepsAtPlungeRate`], tab steps are taken out
/// of that comparison and must instead run at the cutting feed in the
/// golden program and at the plunge rate in DreamCarve's.
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
    for (who, cuts, step_feed) in [
        ("golden", golden, limits.feed),
        ("DreamCarve", ours, limits.plunge),
    ] {
        let steps: Vec<&Vertical> = cuts.verticals.iter().filter(|v| is_tab_step(v)).collect();
        if rect_steps && steps.is_empty() {
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
    let feeds = |cuts: &Cuts, down: bool| {
        let mut feeds: Vec<f64> = cuts
            .verticals
            .iter()
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
