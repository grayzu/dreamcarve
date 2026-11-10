// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What `dreamcarve export` prints once its files are written, line for
//! line: the files in the order to run them with what to do before each,
//! each exported operation, each program's extents (#62), estimated time
//! (capability 30) and pauses, the board totals (#498) and the total.
//! Moved from `crates/dreamcarve/src/export.rs` with the text unchanged,
//! so the app's export result sheet is the CLI's report
//! (`docs/specs/phase-4/design.md`, "Architecture → Export"). The shell
//! prints the lines; nothing here does I/O.

use dc_gcode::{EmitOp, Program, ProgramSplit};
use dc_project::{Board, Project};

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
    let mut lines = match (written.programs, written.outputs) {
        ([program], [output]) if !by_board(project) => {
            report(project, program, output, written, estimates)
        }
        _ => report_runs(project, written, estimates),
    };
    lines.extend(describe_carves(project, written.generated));
    if let Some(preview) = written.preview {
        lines.push(format!("Wrote preview {preview}."));
    }
    lines
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
) -> Vec<String> {
    let mut lines = vec![format!(
        "Wrote {output} ({} lines, post-processor {}).",
        program.text.lines().count(),
        project.post_processor.id
    )];
    post_and_operations(&mut lines, project, written);
    lines.extend(describe_extents(
        project,
        program,
        largest_tool_radius_mm(project),
    ));
    lines.push(estimates.program_line(0));
    // #57: the program does not stop for the first tool.
    lines.push(format!(
        "Before you start: {}",
        before_you_start(program, 0, true)
    ));
    // #283: `G53` moves to a machine position, which is right only on a
    // homed machine; the program cannot check that.
    if let Some(z) = &program.machine_lift {
        lines.push(format!(
            "Before you start: home the machine; this program moves to machine Z {z} (G53).{}",
            limits_note(project)
        ));
    }
    tool_length_offset(&mut lines, project);
    scaling(&mut lines, project);
    pauses(&mut lines, program, "");
    // #95: the user chose one program with M6, in the project or with
    // `--one-program`; it stops only if the sender lets M6 through or
    // stops for it itself.
    if one_program(project, written.split) && changes_tools(project) {
        lines.push(
            "This program changes tools with M6, so your sender must stop at M6; see \"One program with M6\" in the post-processors chapter."
                .to_owned(),
        );
    }
    lines
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
fn report_runs(project: &Project, written: &Written<'_>, estimates: &Estimates) -> Vec<String> {
    let programs = written.programs;
    let outputs = written.outputs;
    let boards = by_board(project);
    let mut lines = Vec::new();
    if programs.len() == 1 {
        lines.push("Wrote 1 program:".to_owned());
    } else {
        lines.push(format!(
            "Wrote {} programs; run them in this order:",
            programs.len()
        ));
    }
    let mut k = 0;
    let mut board = None;
    for (program, output) in programs.iter().zip(outputs) {
        // #498: each board is its own setup, its programs counted afresh.
        if boards && board != Some(program.board) {
            board = Some(program.board);
            k = 0;
            lines.push(board_heading(project, program.board));
        }
        // The first program sets work zero; a later one keeps X and Y and
        // sets Z zero with its new bit, whose length differs (#43).
        let before = before_you_start(program, k, false);
        k += 1;
        lines.push(format!("  {k}. {output}  Before you start: {before}"));
    }
    // #283: every program of the export ends with the same lift.
    if let Some(z) = programs.iter().find_map(|p| p.machine_lift.as_ref()) {
        lines.push(format!(
            "Before you start: home the machine; each program moves to machine Z {z} (G53).{}",
            limits_note(project)
        ));
    }
    tool_length_offset(&mut lines, project);
    scaling(&mut lines, project);
    lines.push(format!("Post-processor {}.", project.post_processor.id));
    post_and_operations(&mut lines, project, written);
    let mut board = None;
    for (k, (program, output)) in programs.iter().zip(outputs).enumerate() {
        // #498: each board's programs together, with the board's total.
        if boards && board != Some(program.board) {
            if let Some(done) = board {
                board_total(&mut lines, programs, estimates, done);
            }
            board = Some(program.board);
            lines.push(board_heading(project, program.board));
        }
        lines.push(format!(
            "{output} ({} lines):",
            program.text.lines().count()
        ));
        for line in describe_extents(project, program, tool_radius_mm(project, program)) {
            lines.push(format!("  {line}"));
        }
        lines.push(format!("  {}", estimates.program_line(k)));
        pauses(&mut lines, program, "  ");
    }
    if let Some(done) = board {
        board_total(&mut lines, programs, estimates, done);
    }
    if let Some(total) = estimates.total_line() {
        lines.push(total);
    }
    lines
}

/// The estimated time of `board`'s programs, when it has more than one
/// and they are not all of the export (#498).
fn board_total(lines: &mut Vec<String>, programs: &[Program], estimates: &Estimates, board: Board) {
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
        lines.push(line);
    }
}

/// What `--post` changed, then one line per exported operation.
fn post_and_operations(lines: &mut Vec<String>, project: &Project, written: &Written<'_>) {
    lines.extend(written.switched.iter().cloned());
    for line in describe_operations(project, written.generated) {
        lines.push(format!("  {line}"));
    }
}

/// For a program that leaves the tool length offset alone, when to set
/// one (functional spec, capability 29).
fn tool_length_offset(lines: &mut Vec<String>, project: &Project) {
    if KEEPS_TOOL_LENGTH_OFFSET.contains(&project.post_processor.id.as_str()) {
        lines.push(
            "This program does not change the tool length offset (G43.1). If your sender sets one, set it before you set Z zero."
                .to_owned(),
        );
    }
}

/// For a grblHAL program whose header cancels scaling with `G50`, that
/// grblHAL's `G50` misses a scale that leaves X at 1, so to soft-reset
/// after any `G51` and check `$G` (#320; [`dc_gcode::scaling_note`]). It
/// is printed, not written into the program.
fn scaling(lines: &mut Vec<String>, project: &Project) {
    // The project was exported with this post-processor, so it resolves.
    let note = project
        .post_processor
        .resolve()
        .ok()
        .and_then(|post| dc_gcode::scaling_note(&post));
    if let Some(note) = note {
        lines.push(note.to_owned());
    }
}

/// Every pause and tool change of `program`, each line after `indent`.
fn pauses(lines: &mut Vec<String>, program: &Program, indent: &str) {
    if program.pauses.is_empty() {
        return;
    }
    lines.push(format!(
        "{indent}Pauses and tool changes; resume each with cycle start:"
    ));
    for pause in &program.pauses {
        let message = if pause.message.is_empty() {
            "(no message)"
        } else {
            pause.message.as_str()
        };
        lines.push(format!("{indent}  line {}: {message}", pause.line));
    }
}
