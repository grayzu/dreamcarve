// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The facts of a simulation as `simulate` prints them (phase 6
//! functional spec, capabilities 122 and 125): the board and its cells,
//! what was cut and how long the moves take, the deepest cut and its
//! operation, where the stock is cut through, the volume removed, and the
//! tools shown as plain cylinders.

use dc_project::{Project, Units};
use dc_sim::{Comparison, Facts, Heightfield, Timeline};

use super::{BoardPlan, PlannedRun};
use crate::summary::describe_length;

const MM_PER_INCH: f64 = 25.4;

/// The report of `field`, the stock the first `runs_cut` runs of `board`
/// leave, with their `facts`.
#[must_use]
pub fn describe(
    project: &Project,
    board: &BoardPlan,
    field: &Heightfield,
    runs_cut: usize,
    facts: &Facts,
) -> Vec<String> {
    let runs = board.runs.get(..runs_cut).unwrap_or(&board.runs);
    let grid = field.grid();
    let name = match board.board {
        dc_project::Board::Female => "the stock",
        dc_project::Board::Male => "the male board",
    };
    let mut lines = vec![format!(
        "Simulated {name}, {} × {} × {}, as {} × {} cells of {}.",
        describe_length(project, board.width_mm),
        describe_length(project, board.height_mm),
        describe_length(project, board.thickness_mm),
        grid.nx(),
        grid.ny(),
        describe_length(project, grid.cell_mm()),
    )];

    let mut ops: Vec<usize> = runs.iter().map(|r| r.op_index).collect();
    ops.dedup();
    ops.sort_unstable();
    ops.dedup();
    let sims: Vec<_> = runs.iter().map(|r| r.sim.clone()).collect();
    lines.push(format!(
        "{} in {}, {} of moves.",
        count(ops.len(), "operation"),
        count(runs.len(), "tool run"),
        describe_duration(Timeline::of(&sims).total_s()),
    ));
    if runs.len() < board.runs.len() {
        lines.push(format!(
            "The {} after them are left out (--until).",
            count(board.runs.len() - runs.len(), "tool run")
        ));
    }

    match facts.deepest_run.and_then(|k| runs.get(usize::from(k))) {
        Some(run) => lines.push(format!(
            "Deepest cut: {} by {}.",
            describe_length(project, facts.deepest_mm),
            operation(project, run)
        )),
        None => lines.push("Nothing is cut.".to_owned()),
    }
    if facts.through_runs.is_empty() {
        lines.push("Not cut through anywhere.".to_owned());
    } else {
        let mut by: Vec<String> = Vec::new();
        for run in facts
            .through_runs
            .iter()
            .filter_map(|&k| runs.get(usize::from(k)))
        {
            let name = operation(project, run);
            if !by.contains(&name) {
                by.push(name);
            }
        }
        lines.push(format!(
            "Cut through: {} by {}.",
            area(project, facts.through_mm2),
            by.join(", ")
        ));
    }
    if facts.deepest_run.is_some() {
        lines.push(format!("Removed: {}.", volume(project, facts.removed_mm3)));
    }

    let mut plain: Vec<String> = Vec::new();
    for run in facts.plain_runs.iter().filter_map(|&k| runs.get(k)) {
        let name = project
            .tools
            .iter()
            .find(|t| t.id == run.tool)
            .map_or_else(|| run.tool.to_string(), |t| format!("`{}`", t.name));
        if !plain.contains(&name) {
            plain.push(name);
        }
    }
    if !plain.is_empty() {
        lines.push(format!(
            "Shown as {}: {}. The project holds no point angle for a drill and no shape for an engraving bit.",
            if plain.len() == 1 {
                "a plain cylinder"
            } else {
                "plain cylinders"
            },
            plain.join(", ")
        ));
    }
    lines
}

/// The uncut material of a simulation as `simulate` lists it (capability
/// 120): for each operation whose shape asks for more than was cut, how
/// many places, their area and the most that is left, or one line saying
/// there is none. `uncut_mm` is the threshold the comparison was made
/// with and `cell_mm` the grid's cell.
#[must_use]
pub fn describe_uncut(
    project: &Project,
    comparison: &Comparison,
    uncut_mm: f64,
    cell_mm: f64,
) -> Vec<String> {
    let over = describe_length(project, uncut_mm);
    if comparison.places.is_empty() {
        return vec![format!(
            "No uncut material: nowhere is the stock more than {over} above what the shapes ask for."
        )];
    }
    let mut lines = vec![format!(
        "Uncut material, more than {over} above what the shapes ask for:"
    )];
    let mut k = 0;
    while let Some(first) = comparison.places.get(k) {
        let of_op = comparison
            .places
            .iter()
            .skip(k)
            .take_while(|p| p.op == first.op);
        let (mut places, mut mm2, mut height_mm) = (0, 0.0, 0.0_f64);
        for place in of_op {
            places += 1;
            mm2 += place.area_mm2;
            height_mm = height_mm.max(place.height_mm);
        }
        let name = first
            .op
            .and_then(|index| project.operations.get(usize::from(index)))
            .map_or_else(
                || "an operation".to_owned(),
                |op| {
                    if op.name().is_empty() {
                        format!("{}", op.id())
                    } else {
                        format!("`{}` ({})", op.name(), op.id())
                    }
                },
            );
        lines.push(format!(
            "  {name}: {}, {}, up to {}",
            count(places, "place"),
            area(project, mm2),
            describe_length(project, height_mm)
        ));
        k += places;
    }
    lines.push(format!(
        "A place narrower than about two cells ({}) is not found.",
        describe_length(project, 2.0 * cell_mm)
    ));
    lines
}

/// A run's operation as a message names it: its name and its id.
fn operation(project: &Project, run: &PlannedRun) -> String {
    match project.operations.get(run.op_index) {
        Some(op) if !op.name().is_empty() => format!("`{}` ({})", op.name(), run.op),
        _ => format!("{}", run.op),
    }
}

/// `1 tool run`, `3 tool runs`.
fn count(n: usize, what: &str) -> String {
    if n == 1 {
        format!("1 {what}")
    } else {
        format!("{n} {what}s")
    }
}

/// An area in the project's units: `1234.5 mm²`, `1.913 in²`.
fn area(project: &Project, mm2: f64) -> String {
    match project.display_units {
        Units::Mm => format!("{mm2:.1} mm²"),
        Units::Inch => format!("{:.3} in²", mm2 / (MM_PER_INCH * MM_PER_INCH)),
    }
}

/// A volume in the project's units: `45.31 cm³`, `2.765 in³`.
fn volume(project: &Project, mm3: f64) -> String {
    match project.display_units {
        Units::Mm => format!("{:.2} cm³", mm3 / 1000.0),
        Units::Inch => format!("{:.3} in³", mm3 / (MM_PER_INCH * MM_PER_INCH * MM_PER_INCH)),
    }
}

/// A time as the estimate writes one: `14 min 5 s`. The report's and
/// the Simulate step's rows'.
#[must_use]
pub fn describe_duration(seconds: f64) -> String {
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
