// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The property (#464): for every design and operation, generation either
//! succeeds, every run passing `assert_run_safe` and every program the
//! whole safety pass (S1–S19), an Inlay pair seating within
//! [`SEAT_MM`]; or it fails with a user-facing error of [`explains`]'s
//! allow-list. Anything else is a failure, reported with the case that
//! found it.

use super::designs::{Design, bounds};
use super::rng::Rng;
use dc_cam::test_support::assert_run_safe;
use dc_cam::test_support::seat::{SEAT_MM, fit};
use dc_cam::{CamError, ToolRun, generate_inlay, generate_vcarve};
use dc_gcode::{EmitOp, ProgramSplit, emit_programs};
use dc_geometry::Transform;
use dc_project::test_support::{sample_project, tool_6_35_upcut};
use dc_project::{
    ClearanceStrategy, ClearingPass, CutDirection, DefinitionId, Entry, FeatureDefinition,
    FeatureInstance, InlayCutOut, InlayOp, InlaySide, InstanceId, MachiningOrder, Operation,
    OperationId, Problem, Project, Stock, TabPlacement, TabShape, TabSpec, Tool, ToolId, ToolKind,
    Units, VCarveOp, VCarveStrategy, validate,
};
use std::fmt::Write as _;
use std::panic::{AssertUnwindSafe, catch_unwind};

const DATE: &str = "2026-11-01";

/// The bench V-bits' included angles, degrees: all 6.35 mm (1/4 in).
const V_BITS_DEG: [f64; 4] = [15.0, 30.0, 60.0, 90.0];

/// The clearing and cut-out end mill's diameter, mm.
const END_MILL_MM: f64 = 3.175;

/// The operations each design is carved with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Inlay female.
    Female,
    /// Inlay male, no clearing: the band's edge is a V wall.
    MaleVWall,
    /// Inlay male with the end mill's clearing pass: an open edge.
    MaleOpenEdge,
    /// Inlay male with `outline_rounding_mm`.
    MaleRounded,
    /// Inlay male with a cut-out.
    MaleCutOut,
    /// V-Carve from the stock top.
    VCarveTop,
    /// V-Carve from a start depth on a floor cleared beforehand.
    VCarveStart,
}

/// Which pairs the seat check simulates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seats {
    /// None.
    None,
    /// One male, by turn: the case's seed picks it.
    One,
    /// Every male.
    All,
}

/// One design ready to carve: its project (no operations) and the
/// operations, each to be generated in a copy of it alone.
pub struct Case {
    /// The family, seed and drawing, to name the case.
    pub label: String,
    /// The family's name.
    pub family: String,
    pub seed: u64,
    pub project: Project,
    pub ops: Vec<(Kind, Operation)>,
}

/// The result of carving a case.
#[derive(Debug, Default)]
pub struct Outcome {
    /// Operations that generated and passed.
    pub carved: usize,
    /// Operations refused with an allowed error, as `kind: error`.
    pub explained: Vec<String>,
    /// Everything else, each with the case's label.
    pub failures: Vec<String>,
    /// Pairs seated.
    pub seated: usize,
    /// Seat failures of an open issue ([`known_issue`]), each with the
    /// issue and the case.
    pub known: Vec<String>,
}

impl Outcome {
    /// Adds `other`'s counts and lines.
    pub fn absorb(&mut self, other: Outcome) {
        self.carved += other.carved;
        self.seated += other.seated;
        self.explained.extend(other.explained);
        self.failures.extend(other.failures);
        self.known.extend(other.known);
    }
}

/// The errors a valid design may still get (#464): each names a setting
/// the user chose that does not fit the design or the stock, says how to
/// change it, and is documented for users. Everything else is a defect:
/// above all the generator refusing its own move (`ConeOffMaterial`,
/// `ConeArcMove`, `OffMaterial`) or any other internal-consistency error.
pub fn explains(error: &CamError) -> bool {
    matches!(
        error,
        // The swept tool would leave the stock (a stock overflow): the
        // stock is sized round the design and the band, but a rounded
        // band's or a cut-out's reach is the user's to fit.
        CamError::OffStock { .. }
            // The male's cut-out end mill is wider than the band's flat
            // floor ring: the user raises `margin_mm` (the message says
            // to what). The margin is drawn at random, so it can be short.
            | CamError::InlayCutOutNoRoom { .. }
            // #495: the male's band runs past the stock, which clips it,
            // but the design is too near that edge for the band's room
            // there: the user moves the design or grows the stock (the
            // message says by how much).
            | CamError::InlayTooNearEdge { .. }
            // A cut-out's part is narrower than its end mill everywhere
            // (a tiny design): the user picks a smaller tool.
            | CamError::PartNarrowerThanTool { .. }
            // A cut-out round two of the design's parts whose band is
            // narrower than its end mill: the user picks a smaller tool
            // or moves the parts apart.
            | CamError::BandNarrowerThanTool { .. }
    )
}

/// Whether `error` is the generator refusing its own move: its own wall
/// (S17) or material (S9) test, or a V move it cannot test.
pub fn refuses_own_move(error: &CamError) -> bool {
    matches!(
        error,
        CamError::ConeOffMaterial { .. }
            | CamError::ConeArcMove { .. }
            | CamError::OffMaterial { .. }
    )
}

/// `design` as a case of `family`, its settings drawn from `seed`: one of
/// the bench V-bits; inlay depths that keep the male within the bit's
/// cone; margins, roundings, clearing passes and floors; millimetre or
/// inch programs (#469: the written quantum differs).
pub fn case(family: &str, seed: u64, design: Design) -> Case {
    let mut rng = Rng::new(seed ^ 0x5eed_0464);
    let angle_deg = *rng.pick(&V_BITS_DEG);
    let slope = (angle_deg / 2.0_f64).to_radians().tan();
    let cone_mm = 3.175 / slope;
    let units = if rng.chance(0.3) {
        Units::Inch
    } else {
        Units::Mm
    };
    let thickness_mm = 12.0;
    // Inlay values: the male engages `D − G + P`, at most 95 % of the
    // cone and 9 mm (the stock is 12 mm).
    let room_mm = (0.95 * cone_mm).min(9.0);
    let depth_mm = rng.range(0.4, 0.65) * room_mm;
    // The glue gap covers the female floor's ridges, `0.1 · D` at the
    // step-over the user guide asks for: the export's summary reports
    // them ("V-bit floor ridges"), and a plug on a smaller gap rests on
    // them.
    let glue_gap_mm = rng.range(0.12, 0.4) * depth_mm;
    let plug_depth_mm = (room_mm - depth_mm + glue_gap_mm) * rng.range(0.3, 0.95);
    // A small family's design (the CI tests') gets a narrower band, so its
    // male and seat check cost less.
    let small = family.starts_with("small");
    let margin_mm = rng.range(3.0, if small { 6.0 } else { 12.0 });
    let rounding_mm = rng.log_range(1.0, if small { 8.0 } else { 25.0 });
    let safe_z_mm = rng.range(3.0, 10.0);

    let b = bounds(&design.profiles);
    let room = margin_mm + rounding_mm.max(END_MILL_MM) + 8.0;
    let (w, h) = (b.max.x - b.min.x, b.max.y - b.min.y);
    let mut project = sample_project();
    project.name = format!("{family} {seed}");
    project.display_units = units;
    project.stock = Stock {
        width_mm: w + 2.0 * room,
        height_mm: h + 2.0 * room,
        thickness_mm: Some(thickness_mm),
    };
    let def = FeatureDefinition {
        id: DefinitionId::new("d1"),
        name: "Design".into(),
        profiles: design.profiles.clone(),
    };
    project.definitions = std::collections::BTreeMap::from([(def.id.clone(), def)]);
    project.instances = vec![FeatureInstance {
        id: InstanceId::new("i1"),
        definition: DefinitionId::new("d1"),
        transform: Transform::translate(room - b.min.x, room - b.min.y),
        name: "Design".into(),
        visible: true,
        locked: false,
        z_top_mm: None,
        z_bottom_mm: None,
    }];
    project.tools = vec![
        Tool {
            id: ToolId::new("t1"),
            name: format!("{angle_deg}° V-bit"),
            cutter: ToolKind::VBit {
                included_angle_deg: angle_deg,
            },
            step_down_mm: None,
            step_over_fraction: Some(0.1),
            ..tool_6_35_upcut()
        },
        Tool {
            id: ToolId::new("t2"),
            name: "3.175 mm end mill".into(),
            diameter_mm: END_MILL_MM,
            step_down_mm: Some(rng.range(1.0, 3.0)),
            ..tool_6_35_upcut()
        },
    ];
    project.operations = Vec::new();
    // Arc fitting at export (S12) one time in three: it is slow on the
    // V-bit's long level runs (#480), and the generators are what is
    // under test.
    project.arc_fit_tolerance_mm = rng.chance(1.0 / 3.0).then_some(0.01);

    let pass = |rng: &mut Rng| ClearingPass {
        tool: ToolId::new("t2"),
        strategy: if rng.chance(0.75) {
            ClearanceStrategy::Offset
        } else {
            ClearanceStrategy::Raster {
                angle_deg: rng.range(0.0, 180.0),
            }
        },
        direction: if rng.chance(0.5) {
            CutDirection::Climb
        } else {
            CutDirection::Conventional
        },
        entry: if rng.chance(0.5) {
            Entry::Plunge
        } else {
            Entry::Ramp {
                angle_deg: rng.range(3.0, 20.0),
            }
        },
    };
    let inlay = |side: InlaySide| InlayOp {
        id: OperationId::new("o1"),
        name: "Inlay".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        side,
        depth_mm: Some(depth_mm),
        glue_gap_mm: Some(glue_gap_mm),
        plug_depth_mm: Some(plug_depth_mm),
        margin_mm: Some(margin_mm),
        outline_rounding_mm: None,
        clearing: Vec::new(),
        // The user guide's step-over for an inlay's V-bit floor.
        step_over_fraction: Some(0.1),
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm,
        cut_out: None,
    };
    let female_clearing = if rng.chance(0.5) {
        vec![pass(&mut rng)]
    } else {
        Vec::new()
    };
    let rounded_clearing = if rng.chance(0.5) {
        vec![pass(&mut rng)]
    } else {
        Vec::new()
    };
    let cut_out_clearing = if rng.chance(0.5) {
        vec![pass(&mut rng)]
    } else {
        Vec::new()
    };
    let tabs = if rng.chance(0.5) {
        Some(TabSpec {
            placement: TabPlacement::Count {
                count: u8::try_from(rng.int(2, 4)).unwrap_or(2),
            },
            width_mm: rng.range(3.0, 8.0),
            height_mm: rng.range(1.0, 4.0),
            shape: TabShape::Rect,
            on_holes: false,
            avoid_corners: true,
        })
    } else {
        None
    };
    let open_clearing = vec![pass(&mut rng)];
    let carve = |rng: &mut Rng, start_depth_mm: f64, clearing: Vec<ClearingPass>| VCarveOp {
        id: OperationId::new("o2"),
        name: "V-Carve".into(),
        enabled: true,
        targets: vec![InstanceId::new("i1")],
        tool: ToolId::new("t1"),
        strategy: if rng.chance(0.85) {
            VCarveStrategy::Medial
        } else {
            VCarveStrategy::Offset {
                step_over_mm: rng.range(0.1, 0.8),
                direction: CutDirection::Climb,
            }
        },
        clearing,
        // The flank engages from the stock top, start depth and all (an
        // external clearing does not count, `v_bit_cone_exceeded`): at
        // most 90 % of the cone, and 6 mm.
        flat_depth_mm: Some(rng.range(0.1, 1.0) * ((0.9 * cone_mm).min(6.0) - start_depth_mm)),
        start_depth_mm,
        start_plane_cleared_externally: start_depth_mm > 0.0,
        step_over_fraction: Some(rng.range(0.1, 0.4)),
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm,
    };
    let top_clearing = if rng.chance(0.5) {
        vec![pass(&mut rng)]
    } else {
        Vec::new()
    };
    let top = carve(&mut rng, 0.0, top_clearing);
    let start_clearing = if rng.chance(0.5) {
        vec![pass(&mut rng)]
    } else {
        Vec::new()
    };
    let start_depth_mm = rng.range(0.1, 0.5) * (0.9 * cone_mm).min(6.0);
    let start = carve(&mut rng, start_depth_mm, start_clearing);
    let ops = vec![
        (
            Kind::Female,
            Operation::Inlay(InlayOp {
                clearing: female_clearing,
                ..inlay(InlaySide::Female)
            }),
        ),
        (Kind::MaleVWall, Operation::Inlay(inlay(InlaySide::Male))),
        (
            Kind::MaleOpenEdge,
            Operation::Inlay(InlayOp {
                clearing: open_clearing,
                ..inlay(InlaySide::Male)
            }),
        ),
        (
            Kind::MaleRounded,
            Operation::Inlay(InlayOp {
                clearing: rounded_clearing,
                outline_rounding_mm: Some(rounding_mm),
                ..inlay(InlaySide::Male)
            }),
        ),
        (
            Kind::MaleCutOut,
            Operation::Inlay(InlayOp {
                clearing: cut_out_clearing,
                cut_out: Some(InlayCutOut {
                    tool: ToolId::new("t2"),
                    tabs,
                    through_cut_allowance_mm: 0.2,
                }),
                ..inlay(InlaySide::Male)
            }),
        ),
        (Kind::VCarveTop, Operation::VCarve(top)),
        (Kind::VCarveStart, Operation::VCarve(start)),
    ];
    let mut label = format!(
        "{family} seed {seed:#x}: {angle_deg}° V-bit, {units:?}, D {depth_mm:.3} G {glue_gap_mm:.3} P {plug_depth_mm:.3} M {margin_mm:.2} R {rounding_mm:.2}"
    );
    for note in &design.notes {
        let _ = write!(label, "\n    {note}");
    }
    Case {
        label,
        family: family.to_owned(),
        seed,
        project,
        ops,
    }
}

/// `project` with `op` as its only operation.
fn with_op(project: &Project, op: &Operation) -> Project {
    let mut p = project.clone();
    p.operations = vec![op.clone()];
    p
}

/// A panic's message.
fn message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| payload.downcast_ref::<&str>().map(|s| (*s).to_owned()))
        .unwrap_or_else(|| "a panic".into())
}

/// Generates `op` in `project`, which holds it.
fn generate(project: &Project, op: &Operation) -> Result<Vec<ToolRun>, CamError> {
    match op {
        Operation::Inlay(i) => generate_inlay(project, i).map(|(runs, _)| runs),
        Operation::VCarve(v) => generate_vcarve(project, v).map(|(runs, _)| runs),
        _ => unreachable!("the stress tests carve and inlay only"),
    }
}

/// The open issue a seat failure of `case` is, if any. Each is a class
/// the sweep found that this PR does not fix, filed with its own
/// reproduction and an ignored regression test; a failure matching one is
/// reported, not failed, and the issue's fix removes its line here.
/// Nothing else is excused: a generation, safety or panic failure never
/// is.
fn known_issue(case: &Case, kind: Kind) -> Option<u32> {
    KNOWN_CASES
        .iter()
        .find(|&&(family, seed, k, _)| case.family == family && case.seed == seed && k == kind)
        .map(|&(.., issue)| issue)
}

/// Seat failures of the CI tests' designs, each with the open issue it
/// is filed under:
/// family, seed, operation, issue.
const KNOWN_CASES: [(&str, u64, Kind, u32); 4] = [
    // #484: beside the last "e"'s mouth, 0.36 mm.
    ("small lettering", 0x0464_1000, Kind::MaleVWall, 484),
    // #501: inside a curve of the "a", 0.015 mm sideways at every bit,
    // 0.057 mm with the 30° bit.
    ("small lettering", 0x0464_1001, Kind::MaleOpenEdge, 501),
    // #499: the written-precision margin at a sharp corner, 0.055 mm with
    // the 30° bit.
    ("small lettering", 0x0464_1002, Kind::MaleRounded, 499),
    // #500: the female's groove stops 0.025 mm into a 125° corner,
    // 0.092 mm with the 30° bit.
    ("line art", 0x34, Kind::MaleOpenEdge, 500),
];

/// The validation problems a valid design may still raise with the
/// settings drawn: as [`explains`], each names a setting that does not
/// fit and how to change it. `inlay_cut_out_no_room` is
/// [`CamError::InlayCutOutNoRoom`], which validation reports first.
const EXPLAINED_PROBLEMS: [&str; 1] = ["inlay_cut_out_no_room"];

/// Generates and checks every operation of `case`; with `full` false,
/// only generation is tried and only the generator refusing its own move
/// is a failure (`generator_never_refuses_its_own_move`). With
/// `STRESS_VERBOSE` set, each step's time goes to stderr.
pub fn carve(case: &Case, seats: Seats, full: bool) -> Outcome {
    let mut out = Outcome::default();
    let fail = |out: &mut Outcome, kind: Kind, what: String| {
        out.failures
            .push(format!("{kind:?} of {}\n  {what}", case.label));
    };
    let verbose = std::env::var_os("STRESS_VERBOSE").is_some();
    let started = std::time::Instant::now();
    let lap = |what: &str, kind: Kind| {
        if verbose {
            eprintln!(
                "  seed {:#x} {kind:?}: {what} by {:.2?}",
                case.seed,
                started.elapsed()
            );
        }
    };
    let mut female: Option<(Project, InlayOp, Vec<ToolRun>)> = None;
    let mut males: Vec<(Kind, Project, InlayOp, Vec<ToolRun>)> = Vec::new();
    for (kind, op) in &case.ops {
        let kind = *kind;
        let project = with_op(&case.project, op);
        let problems: Vec<Problem> = validate(&project)
            .into_iter()
            .filter(Problem::is_error)
            .collect();
        if !problems.is_empty() {
            if problems
                .iter()
                .all(|p| EXPLAINED_PROBLEMS.contains(&p.code()))
            {
                out.explained.push(format!("{kind:?}: {problems:?}"));
            } else {
                fail(
                    &mut out,
                    kind,
                    format!("the project is not valid: {problems:?}"),
                );
            }
            continue;
        }
        let generated = catch_unwind(AssertUnwindSafe(|| generate(&project, op)));
        lap("generated", kind);
        let runs = match generated {
            Ok(Ok(runs)) => runs,
            Ok(Err(e)) if explains(&e) => {
                out.explained.push(format!("{kind:?}: {e}"));
                continue;
            }
            Ok(Err(e)) => {
                if full || refuses_own_move(&e) {
                    fail(&mut out, kind, format!("generation fails: {e}\n  {e:?}"));
                }
                continue;
            }
            Err(p) => {
                fail(
                    &mut out,
                    kind,
                    format!("generation panics: {}", message(&*p)),
                );
                continue;
            }
        };
        if !full {
            out.carved += 1;
            continue;
        }
        let checked = catch_unwind(AssertUnwindSafe(|| {
            for run in &runs {
                assert_run_safe(run, &project, op);
            }
        }));
        if let Err(p) = checked {
            fail(&mut out, kind, format!("assert_run_safe: {}", message(&*p)));
            continue;
        }
        let emit_ops: Vec<EmitOp> = runs
            .iter()
            .map(|run| EmitOp {
                op: op.clone(),
                tool: project
                    .tools
                    .iter()
                    .find(|t| t.id == run.tool)
                    .cloned()
                    .expect("the run's tool"),
                toolpath: run.toolpath.clone(),
                holes: None,
                label: run.label.clone(),
            })
            .collect();
        match catch_unwind(AssertUnwindSafe(|| {
            emit_programs(&project, &emit_ops, ProgramSplit::ProgramPerTool, DATE)
        })) {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                fail(&mut out, kind, format!("the safety pass: {e}\n  {e:?}"));
                continue;
            }
            Err(p) => {
                fail(&mut out, kind, format!("emission panics: {}", message(&*p)));
                continue;
            }
        }
        lap("passed S1–S19", kind);
        out.carved += 1;
        if let Operation::Inlay(inlay) = op {
            match kind {
                Kind::Female => female = Some((project, inlay.clone(), runs)),
                _ => males.push((kind, project, inlay.clone(), runs)),
            }
        }
    }
    let Some((female_project, female_op, female_runs)) = female else {
        return out;
    };
    let chosen: Vec<&(Kind, Project, InlayOp, Vec<ToolRun>)> = match seats {
        Seats::None => Vec::new(),
        Seats::All => males.iter().collect(),
        Seats::One => {
            #[allow(clippy::cast_possible_truncation, reason = "a small count")]
            let pick = (case.seed % 4) as usize;
            males
                .iter()
                .skip(pick % males.len().max(1))
                .take(1)
                .collect()
        }
    };
    for (kind, male_project, male_op, male_runs) in chosen {
        let seated = catch_unwind(AssertUnwindSafe(|| {
            fit(
                (&female_project, &female_op, &female_runs),
                (male_project, male_op, male_runs),
                0.1,
            )
        }));
        lap("seated", *kind);
        match seated {
            Ok(f) if f.is_flush() => out.seated += 1,
            Ok(f) => {
                let what = format!(
                    "the plug stands {:.3} mm proud at {:?} (the male's share {:.3} mm at {:?}); at most {SEAT_MM}",
                    f.seat_mm, f.seat_at, f.male_mm, f.male_at
                );
                match known_issue(case, *kind) {
                    Some(issue) => out
                        .known
                        .push(format!("#{issue}: {kind:?} of {}\n  {what}", case.label)),
                    None => fail(&mut out, *kind, what),
                }
            }
            Err(p) => fail(
                &mut out,
                *kind,
                format!("the seat check panics: {}", message(&*p)),
            ),
        }
    }
    out
}
