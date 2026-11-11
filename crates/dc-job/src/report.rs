// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What `dreamcarve export` prints once its files are written, line for
//! line: the files in the order to run them with what to do before each,
//! each exported operation, each program's extents (#62), estimated time
//! (capability 30) and pauses, the board totals (#498) and the total.
//! Moved from `crates/dreamcarve/src/export.rs` with the text unchanged.
//! Each line carries what it states ([`Part`]), so the app's export result
//! shows the same lines in its three parts, Status, Files and Run
//! instructions (`docs/specs/phase-4/design.md`, "Architecture → Export";
//! functional spec, capability 77a), while the CLI prints them in order.
//! The shell prints the lines; nothing here does I/O.

use dc_gcode::{EmitOp, Program, ProgramSplit};
use dc_project::{Board, Project};

use crate::build::ProgramSummary;
use crate::summary::{
    Estimates, before_you_start, board_heading, by_board, changes_tools, describe_carves,
    describe_extents, describe_operations, largest_tool_radius_mm, one_program, tool_radius_mm,
};

/// The post-processors whose programs leave the tool length offset
/// (`G43.1`) alone: grblHAL and GRBL fold an active offset into the work
/// zero the user sets (`G10 L20`), so a `G49` after the user zeroes Z
/// would move every Z by it (#279; `dc-project/src/definitions/README.md`,
/// "Header modal state").
const KEEPS_TOOL_LENGTH_OFFSET: [&str; 2] = ["grblhal", "grbl"];

/// What an export wrote, for its report.
#[derive(Debug, Clone, Copy)]
pub struct Written<'a> {
    /// The programs, in the order they are run.
    pub programs: &'a [Program],
    /// Where each went, as the report names it (the path as given).
    pub outputs: &'a [String],
    /// The program split the export used.
    pub split: ProgramSplit,
    /// Every enabled operation's runs, as `build` generated them.
    pub generated: &'a [EmitOp],
    /// The preview's path, when one was written.
    pub preview: Option<&'a str>,
    /// What `--post` changed (`Updated …` and each reset), printed before
    /// the operations; empty when the post-processor was not switched.
    pub switched: &'a [String],
}

/// What a line of the export report states, so a shell can show each
/// line where it belongs (the app's export result, capability 77a) while
/// the CLI prints them in order. Program indices count every program of
/// the export, in run order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// What was written: `Wrote …`, `Post-processor …`, what `--post`
    /// changed, and where the preview went.
    Status,
    /// A board's heading, with an Inlay (#498).
    Board,
    /// What to do before program `k` starts: its tool and work zero.
    Step(usize),
    /// A controller note for every program: the `G53` lift, the tool
    /// length offset, scaling, `M6`.
    Note,
    /// One exported operation's tool run.
    Operation,
    /// Program `k`'s file and line count, heading its details.
    Program(usize),
    /// Program `k`'s extents (#62).
    Extents(usize),
    /// Program `k`'s estimated time (capability 30).
    Estimate(usize),
    /// Program `k`'s pauses and tool changes.
    Pause(usize),
    /// A board's estimated time, or the total.
    Total,
    /// A V-Carve's or Inlay's facts.
    Carve,
}

/// One line of the export report and what it states.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    /// What the line states.
    pub part: Part,
    /// The line, as `export` prints it.
    pub text: String,
}

/// The lines of a report, each pushed with its part.
#[derive(Debug, Default)]
struct Lines(Vec<Line>);

impl Lines {
    fn push(&mut self, part: Part, text: String) {
        self.0.push(Line { part, text });
    }

    fn extend(&mut self, part: Part, texts: impl IntoIterator<Item = String>) {
        for text in texts {
            self.push(part, text);
        }
    }
}

/// The export report: every line `export` prints once `written` is in
/// place, in order. One program of a project without an Inlay gets the
/// one-program summary; otherwise the programs are listed in the order to
/// run them, by board with an Inlay. Then the carves' lines and, with a
/// preview, where it went.
#[must_use]
pub fn export_report(
    project: &Project,
    written: &Written<'_>,
    estimates: &Estimates,
) -> Vec<String> {
    export_report_lines(project, written, estimates)
        .into_iter()
        .map(|line| line.text)
        .collect()
}

/// [`export_report`]'s lines, each with what it states: the same lines
/// in the same order.
#[must_use]
pub fn export_report_lines(
    project: &Project,
    written: &Written<'_>,
    estimates: &Estimates,
) -> Vec<Line> {
    let mut lines = match (written.programs, written.outputs) {
        ([program], [output]) if !by_board(project) => {
            report(project, program, output, written, estimates)
        }
        _ => report_runs(project, written, estimates),
    };
    lines.extend(Part::Carve, describe_carves(project, written.generated));
    if let Some(preview) = written.preview {
        lines.push(Part::Status, format!("Wrote preview {preview}."));
    }
    lines.0
}

/// The controller notes `export` prints for `programs`, the rows of one
/// export with `split`, in its order: the `G53` lift (#283), the tool
/// length offset (capability 29), scaling (#320) and, for one program
/// with `M6`, that the sender must stop at `M6` (#95). The setup sheet
/// repeats them (capability 77b).
#[must_use]
pub fn notes(project: &Project, programs: &[ProgramSummary], split: ProgramSplit) -> Vec<String> {
    let programs: Vec<&Program> = programs.iter().map(|p| &p.program).collect();
    let single = programs.len() == 1 && !by_board(project);
    let mut notes: Vec<String> = machine_lift_note(project, &programs, single)
        .into_iter()
        .chain(tool_length_offset(project))
        .chain(scaling(project))
        .collect();
    if single {
        notes.extend(m6_note(project, split));
    }
    notes
}

/// The summary of one program: the file written, one line per exported
/// operation, the program's extents (#62), its estimated run time
/// (capability 30), the tool to fit and set work zero with before
/// starting (#57), then every pause and tool change with its line number
/// and message, because the controller or sender may not show the message
/// (design step 8).
fn report(
    project: &Project,
    program: &Program,
    output: &str,
    written: &Written<'_>,
    estimates: &Estimates,
) -> Lines {
    let mut lines = Lines::default();
    lines.push(
        Part::Status,
        format!(
            "Wrote {output} ({} lines, post-processor {}).",
            program.text.lines().count(),
            project.post_processor.id
        ),
    );
    post_and_operations(&mut lines, project, written);
    lines.extend(
        Part::Extents(0),
        describe_extents(project, program, largest_tool_radius_mm(project)),
    );
    lines.push(Part::Estimate(0), estimates.program_line(0));
    // #57: the program does not stop for the first tool.
    lines.push(
        Part::Step(0),
        format!("Before you start: {}", before_you_start(program, 0, true)),
    );
    // #283: `G53` moves to a machine position, which is right only on a
    // homed machine; the program cannot check that.
    lines.extend(Part::Note, machine_lift_note(project, &[program], true));
    lines.extend(Part::Note, tool_length_offset(project));
    lines.extend(Part::Note, scaling(project));
    pauses(&mut lines, 0, program, "");
    lines.extend(Part::Note, m6_note(project, written.split));
    lines
}

/// #95: for one program with `M6` tool changes, chosen in the project or
/// with `--one-program`, that it stops only if the sender lets `M6`
/// through or stops for it itself; `None` otherwise.
fn m6_note(project: &Project, split: ProgramSplit) -> Option<String> {
    (one_program(project, split) && changes_tools(project)).then(|| {
        "This program changes tools with M6, so your sender must stop at M6; see \"One program with M6\" in the post-processors chapter."
            .to_owned()
    })
}

/// #283: that the machine must be homed, since `programs` move to a
/// machine Z with `G53` (every program of an export ends with the same
/// lift); `None` without a lift.
fn machine_lift_note(project: &Project, programs: &[&Program], single: bool) -> Option<String> {
    let z = programs.iter().find_map(|p| p.machine_lift.as_ref())?;
    let which = if single {
        "this program"
    } else {
        "each program"
    };
    Some(format!(
        "Before you start: home the machine; {which} moves to machine Z {z} (G53).{}",
        limits_note(project)
    ))
}

/// #308: the post-processor's condition for a `G53` lift outside the
/// travel to stop before it moves, with a leading space, or nothing.
fn limits_note(project: &Project) -> String {
    dc_gcode::machine_lift_limits_note(&project.post_processor.id)
        .map(|note| format!(" {note}"))
        .unwrap_or_default()
}

/// The summary of the programs of tool runs (#95): the files in the order
/// to run them, with what to do before each, then one line per exported
/// operation, then each program's extents (#62), estimated run time and
/// pauses, then the total run time (capability 30).
fn report_runs(project: &Project, written: &Written<'_>, estimates: &Estimates) -> Lines {
    let programs = written.programs;
    let outputs = written.outputs;
    let boards = by_board(project);
    let mut lines = Lines::default();
    if programs.len() == 1 {
        lines.push(Part::Status, "Wrote 1 program:".to_owned());
    } else {
        lines.push(
            Part::Status,
            format!("Wrote {} programs; run them in this order:", programs.len()),
        );
    }
    let mut k = 0;
    let mut board = None;
    for (index, (program, output)) in programs.iter().zip(outputs).enumerate() {
        // #498: each board is its own setup, its programs counted afresh.
        if boards && board != Some(program.board) {
            board = Some(program.board);
            k = 0;
            lines.push(Part::Board, board_heading(project, program.board));
        }
        // The first program sets work zero; a later one keeps X and Y and
        // sets Z zero with its new bit, whose length differs (#43).
        let before = before_you_start(program, k, false);
        k += 1;
        lines.push(
            Part::Step(index),
            format!("  {k}. {output}  Before you start: {before}"),
        );
    }
    // #283: every program of the export ends with the same lift.
    let all: Vec<&Program> = programs.iter().collect();
    lines.extend(Part::Note, machine_lift_note(project, &all, false));
    lines.extend(Part::Note, tool_length_offset(project));
    lines.extend(Part::Note, scaling(project));
    lines.push(
        Part::Status,
        format!("Post-processor {}.", project.post_processor.id),
    );
    post_and_operations(&mut lines, project, written);
    let mut board = None;
    for (k, (program, output)) in programs.iter().zip(outputs).enumerate() {
        // #498: each board's programs together, with the board's total.
        if boards && board != Some(program.board) {
            if let Some(done) = board {
                board_total(&mut lines, programs, estimates, done);
            }
            board = Some(program.board);
            lines.push(Part::Board, board_heading(project, program.board));
        }
        lines.push(
            Part::Program(k),
            format!("{output} ({} lines):", program.text.lines().count()),
        );
        for line in describe_extents(project, program, tool_radius_mm(project, program)) {
            lines.push(Part::Extents(k), format!("  {line}"));
        }
        lines.push(
            Part::Estimate(k),
            format!("  {}", estimates.program_line(k)),
        );
        pauses(&mut lines, k, program, "  ");
    }
    if let Some(done) = board {
        board_total(&mut lines, programs, estimates, done);
    }
    if let Some(total) = estimates.total_line() {
        lines.push(Part::Total, total);
    }
    lines
}

/// The estimated time of `board`'s programs, when it has more than one
/// and they are not all of the export (#498).
fn board_total(lines: &mut Lines, programs: &[Program], estimates: &Estimates, board: Board) {
    let ks: Vec<usize> = programs
        .iter()
        .enumerate()
        .filter(|(_, p)| p.board == board)
        .map(|(k, _)| k)
        .collect();
    if ks.len() > 1
        && ks.len() < programs.len()
        && let Some(line) =
            estimates.total_of(&ks, &format!("Estimated {} board time", board.as_str()))
    {
        lines.push(Part::Total, line);
    }
}

/// What `--post` changed, then one line per exported operation.
fn post_and_operations(lines: &mut Lines, project: &Project, written: &Written<'_>) {
    lines.extend(Part::Status, written.switched.iter().cloned());
    for line in describe_operations(project, written.generated) {
        lines.push(Part::Operation, format!("  {line}"));
    }
}

/// For a program that leaves the tool length offset alone, when to set
/// one (functional spec, capability 29).
fn tool_length_offset(project: &Project) -> Option<String> {
    KEEPS_TOOL_LENGTH_OFFSET
        .contains(&project.post_processor.id.as_str())
        .then(|| {
            "This program does not change the tool length offset (G43.1). If your sender sets one, set it before you set Z zero."
                .to_owned()
        })
}

/// For a grblHAL program whose header cancels scaling with `G50`, that
/// grblHAL's `G50` misses a scale that leaves X at 1, so to soft-reset
/// after any `G51` and check `$G` (#320; [`dc_gcode::scaling_note`]). It
/// is printed, not written into the program.
fn scaling(project: &Project) -> Option<String> {
    // The project was exported with this post-processor, so it resolves.
    project
        .post_processor
        .resolve()
        .ok()
        .and_then(|post| dc_gcode::scaling_note(&post))
        .map(str::to_owned)
}

/// Every pause and tool change of program `k`, each line after `indent`.
fn pauses(lines: &mut Lines, k: usize, program: &Program, indent: &str) {
    if program.pauses.is_empty() {
        return;
    }
    lines.push(
        Part::Pause(k),
        format!("{indent}Pauses and tool changes; resume each with cycle start:"),
    );
    for pause in &program.pauses {
        let message = if pause.message.is_empty() {
            "(no message)"
        } else {
            pause.message.as_str()
        };
        lines.push(
            Part::Pause(k),
            format!("{indent}  line {}: {message}", pause.line),
        );
    }
}
