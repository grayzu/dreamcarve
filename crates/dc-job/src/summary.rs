// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The lines of the export summary that both shells show
//! (`docs/specs/phase-4/design.md`, "Data model → dc-job"), moved from
//! `crates/dreamcarve/src/export.rs` with their text unchanged: each
//! exported operation and its depth, a carve's deepest cut and floor
//! ridges, a program's extents (#62) and cycle-time estimate (functional
//! spec, capability 30), the board headings (#498), and what to do before
//! a program starts (#57). Lengths are stated in the project's display
//! units with the post-processor's decimals (addendum A6).

use dc_cam::RunLabel;
use dc_gcode::format::format_number;
use dc_gcode::transform::mm_per_unit;
use dc_gcode::{AxisRange, EmitOp, Estimate, ExportWarning, Extents, Program, ProgramSplit};
use dc_project::{
    Board, DrillCycle, DrillOp, Operation, OperationId, Project, ToolChangeMode, ToolId, Units,
};
use serde::Deserialize;

/// The `[estimate]` table of the user config: facts about the user's
/// machine that the printed cycle-time estimate uses, and nothing else
/// (functional spec, capability 30). No program depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EstimateConfig {
    /// The machine's rapid rate, mm/min, finite and above 0. Unset, rapids
    /// are reported as a distance and left out of the time, rather than
    /// guessed.
    #[serde(deserialize_with = "positive_some")]
    pub rapid_mm_min: Option<f64>,
    /// What every time is multiplied by, as VCarve's "Scale Factor": the
    /// user's correction for acceleration and the like. Finite and above
    /// 0; 1 unless set.
    #[serde(deserialize_with = "positive")]
    pub scale_factor: f64,
}

impl Default for EstimateConfig {
    fn default() -> Self {
        Self {
            rapid_mm_min: None,
            scale_factor: 1.0,
        }
    }
}

/// A number that is finite and above 0; anything else is an error at the
/// value, which the config reader locates.
fn positive<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let value = f64::deserialize(deserializer)?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(format!(
            "must be a number above 0, not {value}"
        )))
    }
}

/// [`positive`], for a key that may be left out.
fn positive_some<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<f64>, D::Error> {
    positive(deserializer).map(Some)
}

/// The cycle-time estimate of each program (functional spec, capability
/// 30), with the user's `[estimate]` settings to print them with. It reads
/// the programs and changes nothing.
#[derive(Debug, Clone)]
pub struct Estimates {
    /// Each program's estimate, in program order, or why there is none.
    per_program: Vec<Result<Estimate, String>>,
    settings: EstimateConfig,
}

impl Estimates {
    /// The estimates of `programs`, emitted from `project`.
    #[must_use]
    pub fn new(project: &Project, programs: &[Program], settings: EstimateConfig) -> Self {
        let post = project.post_processor.resolve().map_err(|e| e.to_string());
        let per_program = programs
            .iter()
            .map(|program| {
                let post = post.as_ref().map_err(Clone::clone)?;
                dc_gcode::estimate(program, post).map_err(|e| e.to_string())
            })
            .collect();
        Self {
            per_program,
            settings,
        }
    }

    /// The estimate of program `k`, when it could be read.
    #[must_use]
    pub fn estimate(&self, k: usize) -> Option<Estimate> {
        self.per_program
            .get(k)
            .and_then(|e| e.as_ref().ok())
            .copied()
    }

    /// The `Estimated time: …` line of program `k`.
    #[must_use]
    pub fn program_line(&self, k: usize) -> String {
        match self.per_program.get(k) {
            Some(Ok(estimate)) => describe_estimate("Estimated time", estimate, &self.settings),
            Some(Err(reason)) => format!("Estimated time: not available ({reason})."),
            None => "Estimated time: not available.".to_owned(),
        }
    }

    /// The `<label>: …` line over the programs `ks` (#498: one board's);
    /// `None` when `ks` is empty.
    #[must_use]
    pub fn total_of(&self, ks: &[usize], label: &str) -> Option<String> {
        if ks.is_empty() {
            return None;
        }
        let total = ks.iter().try_fold(Estimate::default(), |sum, &k| {
            self.per_program
                .get(k)
                .and_then(|e| e.as_ref().ok())
                .map(|e| sum + *e)
        });
        Some(match total {
            Some(total) => describe_estimate(label, &total, &self.settings),
            None => format!("{label}: not available."),
        })
    }

    /// The `Estimated total time: …` line over every program, when there
    /// is more than one.
    #[must_use]
    pub fn total_line(&self) -> Option<String> {
        if self.per_program.len() < 2 {
            return None;
        }
        let total = self
            .per_program
            .iter()
            .try_fold(Estimate::default(), |sum, e| {
                e.as_ref().ok().map(|e| sum + *e)
            });
        Some(match total {
            Some(total) => describe_estimate("Estimated total time", &total, &self.settings),
            None => "Estimated total time: not available.".to_owned(),
        })
    }
}

/// `estimate` as one line, as capability 30 shows it: the total, then the
/// cutting and dwell times, the rapids' time when the user has given a
/// rapid rate or else their distance and how to include it, the scale
/// factor when it is not 1, and what is left out: acceleration, and the
/// time at each pause and tool change, counted. Every time is multiplied
/// by the scale factor.
#[must_use]
pub fn describe_estimate(label: &str, estimate: &Estimate, settings: &EstimateConfig) -> String {
    let scale = settings.scale_factor;
    let cutting_s = estimate.feed_s * scale;
    let dwells_s = estimate.dwell_s * scale;
    let rapids = settings
        .rapid_mm_min
        .map(|rate| (estimate.rapid_mm / rate * SECONDS_PER_MINUTE * scale, rate));
    let total_s = cutting_s + dwells_s + rapids.map_or(0.0, |(s, _)| s);
    let mut parts = format!(
        "cutting {}, dwells {}",
        duration(cutting_s),
        duration(dwells_s)
    );
    if let Some((rapids_s, rate)) = rapids {
        parts.push_str(&format!(", rapids {} at {rate} mm/min", duration(rapids_s)));
    }
    if (scale - 1.0).abs() > f64::EPSILON {
        parts.push_str(&format!("; scale factor {scale}"));
    }
    let mut line = format!("{label}: {} ({parts})", duration(total_s));
    if rapids.is_none() && estimate.rapid_mm > 0.0 {
        line.push_str(&format!(
            "; {} of rapids not included: set [estimate] rapid_mm_min in config.toml",
            distance(estimate.rapid_mm)
        ));
    }
    line.push_str(". Excludes acceleration");
    match estimate.stops {
        0 => {}
        1 => line.push_str(" and 1 pause"),
        n => line.push_str(&format!(" and {n} pauses")),
    }
    line.push('.');
    line
}

const SECONDS_PER_MINUTE: f64 = 60.0;

/// `seconds`, rounded to the second, as `1 h 2 min 3 s`, `14 min 5 s` or
/// `13 s`.
fn duration(seconds: f64) -> String {
    let total = seconds.max(0.0).round();
    let hours = (total / 3600.0).floor();
    let minutes = ((total - hours * 3600.0) / 60.0).floor();
    let secs = total - hours * 3600.0 - minutes * 60.0;
    if hours > 0.0 {
        format!("{hours:.0} h {minutes:.0} min {secs:.0} s")
    } else if minutes > 0.0 {
        format!("{minutes:.0} min {secs:.0} s")
    } else {
        format!("{secs:.0} s")
    }
}

/// `mm` as `50 mm`, or from a metre up as `4.3 m`.
fn distance(mm: f64) -> String {
    if mm < 1000.0 {
        format!("{mm:.0} mm")
    } else {
        format!("{:.1} m", mm / 1000.0)
    }
}

/// An export warning as `check` and `export` print it.
pub(crate) fn describe_export_warning(warning: &ExportWarning) -> String {
    match warning {
        ExportWarning::DisabledOperationSkipped { op } => {
            format!("operation `{op}` is disabled and was left out")
        }
        ExportWarning::ArcsFlattened { op, count } => format!(
            "operation `{op}`: {count} arc(s) written as short straight moves because the controller would reject them once rounded"
        ),
        ExportWarning::CannedCycleExpanded { op, reason } => {
            format!("operation `{op}`: written as plain moves, not canned cycles, because {reason}")
        }
        ExportWarning::UnitsSetByController { post, units } => {
            let units = match units {
                Units::Mm => "millimetres",
                Units::Inch => "inches",
            };
            format!(
                "post-processor `{post}` has no units words (G20/G21), so the program has none; it is in {units}, so run it only on a machine set up in {units}"
            )
        }
        ExportWarning::ArcFitFallback { op, runs } => format!(
            "operation `{op}`: {runs} run(s) of straight moves kept as written because, once rounded, the arc fitted to them would be rejected by the controller or stray from them"
        ),
    }
}

/// The deepest cut of operation `op` among `generated`, mm below the
/// stock top (`docs/specs/phase-3/design.md`, "CLI": the summary prints
/// it for a carve, whose depth follows its shapes); `None` when it cuts
/// nothing or the stock has no thickness. A male Inlay's cut-out (#443)
/// is not the carve: it goes through the board, and its line says so.
/// An Inlay's female's runs only: its male's are on their own board
/// ([`deepest_cut_on_mm`]).
#[must_use]
pub fn deepest_cut_mm(project: &Project, generated: &[EmitOp], op: &OperationId) -> Option<f64> {
    deepest_cut_on_mm(project, generated, op, Board::Female)
}

/// [`deepest_cut_mm`] on `board` (#498), mm below that board's top:
/// `project` is the project as given, its male board resolved for an
/// Inlay's male runs.
#[must_use]
pub fn deepest_cut_on_mm(
    project: &Project,
    generated: &[EmitOp],
    op: &OperationId,
    board: Board,
) -> Option<f64> {
    generated
        .iter()
        .filter(|e| e.op.id() == op && e.board == board && !matches!(e.label, RunLabel::CutOut(_)))
        .filter_map(|e| deepest_mm(&on_board(project, e), &e.toolpath))
        .reduce(f64::max)
}

/// `project` as entry `entry`'s board cuts it (#498): itself, or on the
/// male board of `entry`'s Inlay.
fn on_board<'p>(project: &'p Project, entry: &EmitOp) -> std::borrow::Cow<'p, Project> {
    match (&entry.op, entry.board) {
        (Operation::Inlay(inlay), Board::Male) => {
            std::borrow::Cow::Owned(project.on_male_board(inlay))
        }
        _ => std::borrow::Cow::Borrowed(project),
    }
}

/// The V-bit floor's ridges for [`describe_carves`] (design step 6): their
/// height between rings and, where it is higher, at the rings' corners
/// (#522); then, with no clearing pass, what flattens them, or with one,
/// that they stand only where no pass reaches.
fn describe_ridges(project: &Project, ridges: dc_cam::vcarve::FloorRidges, alone: bool) -> String {
    let straight = describe_length(project, ridges.straight_mm);
    let corner = describe_length(project, ridges.corner_mm);
    let heights = if corner == straight {
        straight
    } else {
        format!("{straight} along the rings and up to {corner} at their corners")
    };
    if alone {
        format!(
            "; V-bit floor ridges {heights}; a clearing pass or a smaller step_over_fraction flattens them"
        )
    } else {
        format!("; V-bit floor ridges {heights} where no clearing pass reaches")
    }
}

/// The line of Inlay `inlay`'s `board` for [`describe_carves`] (#498),
/// when `generated` cuts it: its deepest cut below the board's top, the
/// female's flat floor or the male's start plane and floor, its floor
/// ridges, and on the male its band's upright edge and its cut-out.
fn describe_inlay(
    project: &Project,
    generated: &[EmitOp],
    inlay: &dc_project::InlayOp,
    board: Board,
) -> Option<String> {
    let deepest = deepest_cut_on_mm(project, generated, &inlay.id, board)?;
    // Validation refuses an unset value, so an exported one is set.
    let length =
        |mm: Option<f64>| mm.map_or_else(|| "?".to_owned(), |mm| describe_length(project, mm));
    let (top, mut floor) = match board {
        Board::Female => (
            "the stock top",
            format!("a flat floor at depth_mm {}", length(inlay.depth_mm)),
        ),
        Board::Male => (
            "the male board's top",
            format!(
                "the plug's start plane {} below the male board's top and the surround's flat floor plug_depth_mm {} below that",
                length(inlay.depth_mm.zip(inlay.glue_gap_mm).map(|(d, g)| d - g)),
                length(inlay.plug_depth_mm)
            ),
        ),
    };
    if let Some(ridges) = dc_cam::inlay::floor_ridges(project, inlay, board) {
        floor.push_str(&describe_ridges(project, ridges, inlay.clearing.is_empty()));
    }
    let band = (board == Board::Male)
        .then(|| dc_project::male_band(project, inlay))
        .flatten();
    // #448: the end mill's upright edge, and its rounding.
    if let Some(pass) = band.as_ref().and_then(|b| b.edge_pass()) {
        let rounding = band.as_ref().map_or(0.0, |b| b.edge_rounding_mm);
        floor.push_str(&format!(
            "; clearing pass `{}` cuts the band's outer edge upright, its inward curves rounded to {}",
            pass.tool,
            describe_length(project, rounding)
        ));
    }
    // #443, #461: where the cut-out frees the plug, and how deep.
    if let (Some(cut_out), Some(band)) = (&inlay.cut_out, band.as_ref())
        && let Some(tool) = project.tools.iter().find(|t| t.id == cut_out.tool)
        && generated
            .iter()
            .any(|e| e.op.id() == &inlay.id && matches!(e.label, RunLabel::CutOut(_)))
    {
        let inset_mm = band.cut_out_inset_mm(tool.diameter_mm);
        floor.push_str(&format!(
            "; the cut-out frees the plug along the floor's outer edge, its centre {} inside the band's edge and at least {} out from the design, through the board to {} below it",
            describe_length(project, inset_mm),
            describe_length(project, band.margin_mm - inset_mm),
            describe_length(project, cut_out.through_cut_allowance_mm)
        ));
    }
    Some(format!(
        "Inlay {} `{}` ({} board): deepest cut {} below {top}, {floor}.",
        inlay.id,
        inlay.name,
        board.as_str(),
        describe_length(project, deepest)
    ))
}

/// One line per enabled V-Carve or Inlay of `generated`, for `check` and
/// `export`: its deepest cut, whether it has a flat depth (capability 39)
/// and, when the V-bit cuts a floor, the height of the ridges it leaves
/// (design step 6) and whether clearing passes cut the rest. An Inlay has
/// a line per board it cuts (#498): the female's, and the male's with its
/// start plane, `depth_mm − glue_gap_mm` below the male board's top
/// (#436); its mirror line is the `inlay_male_mirrored` warning's. A male
/// with a cut-out says where it runs and how far through the board
/// (#443); its deepest cut is the carve's, not the cut-out's.
#[must_use]
pub fn describe_carves(project: &Project, generated: &[EmitOp]) -> Vec<String> {
    project
        .operations
        .iter()
        .filter(|op| op.enabled())
        .flat_map(|op| match op {
            Operation::VCarve(carve) => {
                let Some(deepest) = deepest_cut_mm(project, generated, &carve.id) else {
                    return Vec::new();
                };
                let mut floor = carve.flat_depth_mm.map_or_else(
                    || "no flat depth".to_owned(),
                    |f| format!("clamped at a flat depth of {}", describe_length(project, f)),
                );
                // Design step 6: the V-bit's floor is scalloped; with
                // clearing passes, only where none of them reaches (#406).
                if let Some(ridges) = dc_cam::vcarve::floor_ridges(project, carve) {
                    floor.push_str(&describe_ridges(project, ridges, carve.clearing.is_empty()));
                }
                vec![format!(
                    "V-Carve {} `{}`: deepest cut {} below the stock top, {floor}.",
                    carve.id,
                    carve.name,
                    describe_length(project, deepest)
                )]
            }
            // #498: one line per board it cuts.
            Operation::Inlay(inlay) => [Board::Female, Board::Male]
                .into_iter()
                .filter_map(|board| describe_inlay(project, generated, inlay, board))
                .collect(),
            _ => Vec::new(),
        })
        .collect()
}

/// One line per exported tool run, in cut order: each enabled operation's
/// id, name, tool, target count and depth, a carve's clearing runs before
/// it named `o6 (clearing t1)` with their own tools and depths (#406), and
/// a male Inlay's cut-out after it, `op1 (cut-out t2)` (#443).
#[must_use]
pub fn describe_operations(project: &Project, generated: &[EmitOp]) -> Vec<String> {
    let tool_name = |id: &ToolId| {
        project
            .tools
            .iter()
            .find(|t| &t.id == id)
            .map_or("?", |t| t.name.as_str())
    };
    let mut lines = Vec::new();
    let run_line = |op: &Operation, run: &EmitOp| {
        // #498: a male Inlay's run is as deep below its male board's top.
        let depth = deepest_mm(&on_board(project, run), &run.toolpath).map_or_else(
            || "? mm deep".to_owned(),
            |d| format!("{} deep", describe_length(project, d)),
        );
        let on = match run.board {
            Board::Female => "",
            Board::Male => " on the male board",
        };
        format!(
            "{} `{}`: tool `{}` ({}), {} target(s), {depth}{on}",
            run.label.naming(op.id().as_str()),
            op.name(),
            run.tool.id,
            tool_name(&run.tool.id),
            op.targets().len(),
        )
    };
    for op in project.operations.iter().filter(|op| op.enabled()) {
        // #498: an Inlay's runs, board by board, each as it is cut.
        if matches!(op, Operation::Inlay(_)) {
            lines.extend(
                generated
                    .iter()
                    .filter(|e| e.op.id() == op.id() && e.board == Board::Female)
                    .chain(
                        generated
                            .iter()
                            .filter(|e| e.op.id() == op.id() && e.board == Board::Male),
                    )
                    .map(|run| run_line(op, run)),
            );
            continue;
        }
        // Design "Clearing passes", step 4: a carve's clearing runs are cut
        // before it, each to the carve's floor.
        for run in generated
            .iter()
            .filter(|e| e.op.id() == op.id() && matches!(e.label, RunLabel::Clearing(_)))
        {
            lines.push(run_line(op, run));
        }
        // Validation refuses an unset depth, so an exported one is set; a
        // carve's depth follows its shapes, so its deepest cut is stated,
        // and a countersink's follows its mouth and bit (format 21), so
        // its mouth and the depth its plunges reach are (#428).
        let deepest = || {
            generated
                .iter()
                .filter(|e| e.op.id() == op.id() && e.label == RunLabel::Main)
                .find_map(|e| deepest_mm(project, &e.toolpath))
        };
        let depth = match op {
            Operation::VCarve(_) => deepest().map_or_else(
                || "? mm deep".to_owned(),
                |d| format!("cut {} deep at most", describe_length(project, d)),
            ),
            Operation::Drill(DrillOp {
                cycle: DrillCycle::Countersink { diameter_mm, .. },
                ..
            }) => format!(
                "{} countersinks, {}",
                describe_length(project, *diameter_mm),
                deepest().map_or_else(
                    || "? mm deep".to_owned(),
                    |d| format!("{} deep", describe_length(project, d)),
                ),
            ),
            _ => op.depth_mm().map_or_else(
                || "? mm deep".to_owned(),
                |d| format!("{} deep", describe_length(project, d)),
            ),
        };
        lines.push(format!(
            "{} `{}`: tool `{}` ({}), {} target(s), {depth}",
            op.id(),
            op.name(),
            op.tool(),
            tool_name(op.tool()),
            op.targets().len(),
        ));
        // #443: a male Inlay's cut-out is cut after it, through the board.
        for run in generated
            .iter()
            .filter(|e| e.op.id() == op.id() && matches!(e.label, RunLabel::CutOut(_)))
        {
            lines.push(run_line(op, run));
        }
    }
    lines
}

/// The deepest cut of `toolpath`, mm below the stock top; `None` when it
/// cuts nothing or the stock has no thickness.
fn deepest_mm(project: &Project, toolpath: &dc_cam::Toolpath) -> Option<f64> {
    let thickness_mm = project.stock.thickness_mm?;
    toolpath
        .moves
        .iter()
        .filter(|m| m.is_cut())
        .map(|m| thickness_mm - m.to.z)
        .reduce(f64::max)
}

/// `length_mm` as the summary states a length (addendum A6): in the
/// project's display units, with the post-processor's decimal places for
/// them, then the unit: `12.000 mm`, `0.5000 in`, as VCarve states lengths
/// in its job units.
#[must_use]
pub fn describe_length(project: &Project, length_mm: f64) -> String {
    let units = project.display_units;
    let value = length_mm / mm_per_unit(units);
    let unit = match units {
        Units::Mm => "mm",
        Units::Inch => "in",
    };
    // The project was exported with this post-processor, so it resolves.
    let number = project
        .post_processor
        .resolve()
        .ok()
        .and_then(|post| format_number(value, &post.number_format, units).ok())
        .unwrap_or_else(|| value.to_string());
    format!("{number} {unit}")
}

/// Whether `project`'s programs are named and listed by board (#498): it
/// has an enabled Inlay, whose female is cut on the stock and whose male
/// on its male board.
#[must_use]
pub fn by_board(project: &Project) -> bool {
    project
        .operations
        .iter()
        .any(|op| matches!(op, Operation::Inlay(inlay) if inlay.enabled))
}

/// The heading of `board`'s programs (#498): `Female board, the stock,
/// 110.000 mm × 80.000 mm × 12.000 mm:`, or the male board's the same,
/// its size as the project's male board resolves it.
#[must_use]
pub fn board_heading(project: &Project, board: Board) -> String {
    let (name, stock) = match board {
        Board::Female => ("Female board, the stock", Some(project.stock.clone())),
        Board::Male => ("Male board", project.male_board().map(|(_, stock)| stock)),
    };
    match stock {
        Some(stock) => format!(
            "{name}, {} × {} × {}:",
            describe_length(project, stock.width_mm),
            describe_length(project, stock.height_mm),
            stock
                .thickness_mm
                .map_or_else(|| "?".to_owned(), |t| describe_length(project, t)),
        ),
        None => format!("{name}:"),
    }
}

/// ` <name>`, or nothing for an empty name.
fn name_suffix(name: &str) -> String {
    if name.is_empty() {
        String::new()
    } else {
        format!(" {name}")
    }
}

/// What to do before program `program` starts (#57): fit its first tool
/// and set work zero with it. `single` is a project's one program, which
/// sets work zero; among several, the first of a board (`k_in_board` 0)
/// sets work zero in X, Y and Z, and a later one keeps X and Y and sets Z
/// zero with its new bit, whose length differs (#43).
#[must_use]
pub fn before_you_start(program: &Program, k_in_board: usize, single: bool) -> String {
    let tool = &program.first_tool;
    let fit = format!("fit T{}{}", tool.number, name_suffix(&tool.name));
    if single {
        format!("{fit} and set work zero with it.")
    } else if k_in_board == 0 {
        format!("{fit} and set work zero (X, Y and Z) with it.")
    } else {
        format!("{fit} and set Z zero with it. Keep X and Y zero.")
    }
}

/// Whether consecutive enabled operations of `project` use different tools.
#[must_use]
pub fn changes_tools(project: &Project) -> bool {
    // A carve's clearing passes are tool runs of their own (#406).
    let tools: Vec<_> = project
        .operations
        .iter()
        .filter(|op| op.enabled())
        .flat_map(Operation::run_tools)
        .collect();
    tools.windows(2).any(|pair| pair.first() != pair.get(1))
}

/// Whether `split` writes one program with `M6` tool changes (#95): the
/// `OneProgram` flag, or the project's own `m6` mode by default. Such a
/// program stops only if the sender lets `M6` through or stops for it
/// itself.
#[must_use]
pub fn one_program(project: &Project, split: ProgramSplit) -> bool {
    match split {
        ProgramSplit::OneProgram => true,
        ProgramSplit::ProgramPerTool => false,
        ProgramSplit::ProjectDefault => {
            project.post_processor.tool_change_mode == ToolChangeMode::M6
        }
    }
}

/// The radius of the largest tool an enabled operation uses, mm: how far
/// the cutter's edge reaches past the tool centre in a program that uses
/// them all.
#[must_use]
pub fn largest_tool_radius_mm(project: &Project) -> f64 {
    project
        .operations
        .iter()
        .filter(|op| op.enabled())
        .flat_map(Operation::run_tools)
        .filter_map(|tool| project.tools.iter().find(|t| &t.id == tool))
        .map(|t| t.diameter_mm / 2.0)
        .fold(0.0, f64::max)
}

/// The radius of the one tool of `program`, a tool run's program (#95), mm.
/// The tool is found by id: its `T` number, once set, is no place in the
/// tool list (#120).
#[must_use]
pub fn tool_radius_mm(project: &Project, program: &Program) -> f64 {
    project
        .tools
        .iter()
        .find(|t| t.id == program.first_tool.id)
        .map_or(0.0, |t| t.diameter_mm / 2.0)
}

/// `extents` widened in X and Y by `radius_mm`, the radius of the
/// program's largest tool: where the cutter's edge reaches (#62). Z is the
/// tool centre's.
#[must_use]
pub fn edge_extents(extents: &Extents, radius_mm: f64) -> Extents {
    let widen = radius_mm / mm_per_unit(extents.units);
    let wider = |range: Option<AxisRange>| {
        range.map(|r| AxisRange {
            min: r.min - widen,
            max: r.max + widen,
        })
    };
    Extents {
        units: extents.units,
        x: wider(extents.x),
        y: wider(extents.y),
        z: extents.z,
    }
}

/// The program's extents in machine coordinates (#62), written as the
/// program writes numbers: first the tool centre's, every axis, then in X
/// and Y widened by `radius_mm`, the largest radius of the program's tools,
/// which is where the cutter's edge reaches. Users place clamps from them.
/// Empty for a program that never moves.
#[must_use]
pub fn describe_extents(project: &Project, program: &Program, radius_mm: f64) -> Vec<String> {
    let extents = &program.extents;
    let units = extents.units;
    // The program was emitted with this post-processor, so it resolves.
    let post = project.post_processor.resolve().ok();
    let number = |value: f64| {
        post.as_ref()
            .and_then(|post| format_number(value, &post.number_format, units).ok())
            .unwrap_or_else(|| value.to_string())
    };
    let ranges = |widen: f64, axes: &[(&str, Option<AxisRange>)]| -> Vec<String> {
        axes.iter()
            .filter_map(|(axis, range)| {
                range.map(|r| {
                    format!(
                        "{axis} {}..{}",
                        number(r.min - widen),
                        number(r.max + widen)
                    )
                })
            })
            .collect()
    };
    let centre = ranges(0.0, &[("X", extents.x), ("Y", extents.y), ("Z", extents.z)]);
    if centre.is_empty() {
        return Vec::new();
    }
    let unit = match units {
        Units::Mm => "mm",
        Units::Inch => "in",
    };
    let mut lines = vec![format!(
        "Extents (machine coordinates, {unit}, tool centre): {}",
        centre.join(", ")
    )];
    let edge = ranges(
        radius_mm / mm_per_unit(units),
        &[("X", extents.x), ("Y", extents.y)],
    );
    if radius_mm > 0.0 && !edge.is_empty() {
        lines.push(format!(
            "  The cutter's edge reaches {}; keep clamps outside that.",
            edge.join(", ")
        ));
    }
    lines
}

/// A program's XY extents (#62) back in its board's project space, mm,
/// Y-up: the boxes the app's Viewport draws (phase 4 functional spec,
/// capability 74).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectExtents {
    /// Where the tool centre goes.
    pub centre: dc_geometry::Rect,
    /// Where the cutter's edge reaches: `centre` widened by the radius of
    /// the program's largest tool.
    pub edge: dc_geometry::Rect,
}

/// `summary`'s extents, read in machine coordinates from its program as
/// `check` prints them, mapped back through the post-processor's axes and
/// the origin to project space; `None` when the program never moves in X
/// and Y or its post-processor no longer resolves. A male board program's
/// (#498) are on the male board's own project space.
#[must_use]
pub fn project_extents(
    project: &Project,
    summary: &crate::ProgramSummary,
) -> Option<ProjectExtents> {
    let on_board = match summary.program.board {
        Board::Female => std::borrow::Cow::Borrowed(project),
        Board::Male => {
            let (inlay, _) = project.male_board()?;
            std::borrow::Cow::Owned(project.on_male_board(inlay))
        }
    };
    let post = on_board.post_processor.resolve().ok()?;
    let transform = dc_gcode::transform::MachineTransform::new(&on_board, &post).ok()?;
    let rect = |extents: &Extents| -> Option<dc_geometry::Rect> {
        let (x, y) = (extents.x?, extents.y?);
        let z = extents.z.map_or(0.0, |z| z.min);
        let a = transform.to_project(dc_geometry::Point3::new(x.min, y.min, z), extents.units);
        let b = transform.to_project(dc_geometry::Point3::new(x.max, y.max, z), extents.units);
        Some(dc_geometry::Rect {
            min: dc_geometry::Point::new(a.x.min(b.x), a.y.min(b.y)),
            max: dc_geometry::Point::new(a.x.max(b.x), a.y.max(b.y)),
        })
    };
    Some(ProjectExtents {
        centre: rect(&summary.program.extents)?,
        edge: rect(&summary.edge_extents)?,
    })
}
