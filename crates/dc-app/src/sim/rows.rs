// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Simulate step's panel as text (phase 6 functional spec,
//! capabilities 107, 108, 114, 120 and 122): the summary line, a row an
//! operation in cut order with its runs and how far the playback is
//! through it, and the Result section's lines. The window draws these;
//! the tests read them without one.

use std::ops::Range;

use dc_job::RunLabel;
use dc_job::sim::Timeline;
use dc_job::simulate::{describe, describe_area, describe_duration, describe_uncut};
use dc_project::{OperationId, OriginZ, Project, Stock, ToolId};

use super::camera::Box3;
use super::job::{Built, SimState};
use super::probe::Hit;
use super::session::Blocked;
use crate::app::App;
use crate::operations::{Kind, colour_at};
use crate::strings::{StrKey, Strings};
use crate::view::coordinate;

/// Times closer than this, seconds, are the same moment: a run's end and
/// the next one's start.
const SAME_S: f64 = 1e-9;

/// How far the playback is through a row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Reached {
    /// The playback has not reached it: the row is dimmed.
    Ahead,
    /// It is being cut: the part done, from 0 to 1.
    Cutting(f64),
    /// It is cut.
    Done,
}

impl Reached {
    /// Where a time is in a span of the timeline.
    fn of(time_s: f64, span_s: (f64, f64)) -> Self {
        let (start, end) = span_s;
        if time_s <= SAME_S || time_s < start - SAME_S {
            Self::Ahead
        } else if time_s >= end - SAME_S {
            Self::Done
        } else if time_s <= start + SAME_S {
            Self::Ahead
        } else {
            Self::Cutting(((time_s - start) / (end - start)).clamp(0.0, 1.0))
        }
    }
}

/// One tool run of an operation that has several.
#[derive(Debug, Clone, PartialEq)]
pub struct RunRow {
    /// Its place in the board's plan: what a click on it plays to the
    /// end of.
    pub run: usize,
    /// What it is and what it cuts with: *Clearing · 1/4 in End Mill*.
    pub label: String,
    /// How long it takes.
    pub time: String,
    /// How far the playback is through it.
    pub reached: Reached,
}

/// One operation.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// The operation.
    pub op: OperationId,
    /// Its kind, for the glyph; `None` when the project no longer has
    /// the operation (the simulation shown is an earlier project's).
    pub kind: Option<Kind>,
    /// The palette index of its colour, the Operations list's.
    pub colour: usize,
    /// Its name.
    pub name: String,
    /// Its tool's name.
    pub tool: String,
    /// How long its runs take.
    pub time: String,
    /// Its runs in the board's plan. A click on the row plays to the end
    /// of the last.
    pub runs: Range<usize>,
    /// Its runs, when it has more than one.
    pub parts: Vec<RunRow>,
    /// How far the playback is through it.
    pub reached: Reached,
}

/// The rows of one board's simulation.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Rows {
    /// A row an operation with a run on the board, in cut order.
    pub rows: Vec<Row>,
    /// How long the whole job takes, seconds.
    pub total_s: f64,
}

/// A tool's name, or its id when the project no longer has it.
fn tool_name(project: &Project, id: &ToolId) -> String {
    project
        .tools
        .iter()
        .find(|t| t.id == *id)
        .map_or_else(|| id.to_string(), |t| t.name.clone())
}

/// The rows of `built`, with the playback at `time_s` into the job. An
/// operation is found in `project` by its id, since the simulation shown
/// may be of the project as it was a moment ago.
#[must_use]
pub fn rows(project: &Project, strings: &Strings, built: &Built, time_s: f64) -> Rows {
    let timeline = Timeline::of(&built.runs);
    let span = |runs: &Range<usize>| {
        let start = timeline.run_span_s(runs.start).map_or(0.0, |s| s.0);
        let end = runs
            .end
            .checked_sub(1)
            .and_then(|last| timeline.run_span_s(last))
            .map_or(start, |s| s.1);
        (start, end)
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut start = 0;
    let plan = &built.plan.runs;
    while let Some(first) = plan.get(start) {
        let count = plan
            .iter()
            .skip(start)
            .take_while(|r| r.op == first.op)
            .count();
        let runs = start..start + count;
        let operation = project.operations.iter().find(|op| *op.id() == first.op);
        let kind = operation.map(|op| strings.get(Kind::of(op).label()));
        let parts = if count > 1 {
            plan.iter()
                .enumerate()
                .skip(start)
                .take(count)
                .map(|(k, run)| {
                    let tool = tool_name(project, &run.tool);
                    let label = match &run.label {
                        RunLabel::Main => strings.fmt(
                            StrKey::SimRowsMain,
                            &[("kind", kind.unwrap_or_default()), ("tool", &tool)],
                        ),
                        RunLabel::Clearing(_) => {
                            strings.fmt(StrKey::SimRowsClearing, &[("tool", &tool)])
                        }
                        RunLabel::CutOut(_) => {
                            strings.fmt(StrKey::SimRowsCutOut, &[("tool", &tool)])
                        }
                    };
                    let one = k..k + 1;
                    let (from, to) = span(&one);
                    RunRow {
                        run: k,
                        label,
                        time: describe_duration(to - from),
                        reached: Reached::of(time_s, (from, to)),
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        let (from, to) = span(&runs);
        rows.push(Row {
            op: first.op.clone(),
            kind: operation.map(Kind::of),
            colour: crate::operations::colour(project, &first.op)
                .unwrap_or_else(|| colour_at(first.op_index)),
            name: operation.map_or_else(|| first.op.to_string(), |op| op.name().to_owned()),
            tool: operation.map_or_else(String::new, |op| tool_name(project, op.tool())),
            time: describe_duration(to - from),
            runs,
            parts,
            reached: Reached::of(time_s, (from, to)),
        });
        start += count;
    }
    Rows {
        rows,
        total_s: timeline.total_s(),
    }
}

/// The line under the step's title: what blocks the simulation, else
/// where it stands, else what `built` holds (*5 operations · 14 min
/// 20 s*).
#[must_use]
pub fn summary(
    strings: &Strings,
    state: &SimState,
    blocked: Option<&Blocked>,
    built: Option<&Built>,
) -> String {
    match (blocked, state) {
        (Some(Blocked::NoOperations), _) => strings.get(StrKey::SimSummaryEmpty).to_owned(),
        (Some(Blocked::Errors(count)), _) => {
            strings.fmt(StrKey::SimSummaryBlocked, &[("count", &count.to_string())])
        }
        (Some(Blocked::Generating), _) => strings.get(StrKey::SimSummaryWaiting).to_owned(),
        (Some(Blocked::Held), _) => strings.get(StrKey::SimSummaryHeld).to_owned(),
        (None, SimState::Stopped) => strings.get(StrKey::SimSummaryStopped).to_owned(),
        (None, SimState::Failed(reason)) => {
            strings.fmt(StrKey::SimSummaryFailed, &[("reason", reason)])
        }
        (None, SimState::Building(Some(progress))) => strings.fmt(
            StrKey::SimSummaryBuilding,
            &[("percent", &percent(progress.fraction()))],
        ),
        (None, SimState::Building(None) | SimState::Empty) => {
            strings.get(StrKey::SimSummaryStarting).to_owned()
        }
        (None, SimState::Current | SimState::Stale) => match built {
            Some(built) => {
                let mut ops: Vec<usize> = built.plan.runs.iter().map(|r| r.op_index).collect();
                ops.dedup();
                strings.fmt(
                    StrKey::SimSummaryBuilt,
                    &[
                        ("count", &ops.len().to_string()),
                        (
                            "time",
                            &describe_duration(Timeline::of(&built.runs).total_s()),
                        ),
                    ],
                )
            }
            None => strings.get(StrKey::SimSummaryStarting).to_owned(),
        },
    }
}

/// A fraction as a whole percentage.
fn percent(fraction: f64) -> String {
    format!("{:.0}", (fraction.clamp(0.0, 1.0) * 100.0).floor())
}

/// A time into the job as the transport shows it: `03:10`, and
/// `1:03:10` from an hour on.
#[must_use]
pub fn clock(seconds: f64) -> String {
    let total = if seconds.is_nan() {
        0.0
    } else {
        seconds.max(0.0).floor()
    };
    let hours = (total / 3600.0).floor();
    let minutes = ((total - hours * 3600.0) / 60.0).floor();
    let secs = total - hours * 3600.0 - minutes * 60.0;
    if hours > 0.0 {
        format!("{hours:.0}:{minutes:02.0}:{secs:02.0}")
    } else {
        format!("{minutes:02.0}:{secs:02.0}")
    }
}

/// The Result section's facts (capability 122): the CLI report's lines
/// about the finished stock, without its first two, which the panel's
/// summary and Quality already say.
#[must_use]
pub fn facts_lines(project: &Project, built: &Built) -> Vec<String> {
    describe(
        project,
        &built.plan,
        &built.finished,
        built.plan.runs.len(),
        &built.facts,
    )
    .into_iter()
    .skip(2)
    .collect()
}

/// The Result section's uncut material (capability 120): the CLI
/// report's lines.
#[must_use]
pub fn uncut_lines(project: &Project, built: &Built) -> Vec<String> {
    describe_uncut(
        project,
        &built.comparison,
        built.settings.uncut_mm,
        built.finished.grid().cell_mm(),
    )
}

/// One operation's uncut material (capability 120).
#[derive(Debug, Clone, PartialEq)]
pub struct UncutRow {
    /// The operation whose shape asks for the cut, as its place in the
    /// project's list when the simulation was made; `None` for a place
    /// no one operation asks for.
    pub op: Option<u16>,
    /// *Square recess · 4 places · 8.7 mm² · up to 6.0 mm*.
    pub text: String,
}

/// The uncut material of `built` by operation, in the report's order:
/// its places' number, their area together and the greatest height.
#[must_use]
pub fn uncut_rows(project: &Project, strings: &Strings, built: &Built) -> Vec<UncutRow> {
    let places = &built.comparison.places;
    let mut rows = Vec::new();
    let mut start = 0;
    while let Some(first) = places.get(start) {
        let of_op = places.iter().skip(start).take_while(|p| p.op == first.op);
        let (mut count, mut mm2, mut height_mm) = (0_usize, 0.0, 0.0_f64);
        for place in of_op {
            count += 1;
            mm2 += place.area_mm2;
            height_mm = height_mm.max(place.height_mm);
        }
        // The plan's operation at that place, by its id in the project
        // as it is now.
        let name = first
            .op
            .and_then(|index| {
                built
                    .plan
                    .runs
                    .iter()
                    .find(|run| run.op_index == usize::from(index))
            })
            .and_then(|run| project.operations.iter().find(|op| *op.id() == run.op))
            .map_or_else(
                || strings.get(StrKey::SimUncutNoOperation).to_owned(),
                |op| op.name().to_owned(),
            );
        rows.push(UncutRow {
            op: first.op,
            text: strings.fmt(
                StrKey::SimUncutRow,
                &[
                    ("name", &name),
                    ("count", &count.to_string()),
                    ("area", &describe_area(project, mm2)),
                    (
                        "height",
                        &format!(
                            "{} {}",
                            crate::units::summary_text(height_mm, project.display_units),
                            crate::units::suffix(project.display_units)
                        ),
                    ),
                ],
            ),
        });
        start += count;
    }
    rows
}

/// The box round the uncut places of operation `op_index`, from the bed
/// to the stock's top: what a click on its line frames. `None` when it
/// has none.
#[must_use]
pub fn uncut_bounds(built: &Built, op_index: Option<u16>) -> Option<Box3> {
    let mut around: Option<(f64, f64, f64, f64)> = None;
    for place in built.comparison.places.iter().filter(|p| p.op == op_index) {
        let b = place.bounds;
        around = Some(around.map_or((b.min.x, b.min.y, b.max.x, b.max.y), |a| {
            (
                a.0.min(b.min.x),
                a.1.min(b.min.y),
                a.2.max(b.max.x),
                a.3.max(b.max.y),
            )
        }));
    }
    let (x0, y0, x1, y1) = around?;
    Some(Box3 {
        min: dc_geometry::Point3::new(x0, y0, 0.0),
        max: dc_geometry::Point3::new(x1, y1, built.finished.top_z_mm()),
    })
}

/// What the read-out says of the surface under the pointer (capability
/// 111): its X, Y and Z from the machine origin, in the project's units,
/// and the operation that cut there. `built` is the simulation shown,
/// whose board the point is on.
#[must_use]
pub fn readout(app: &App, built: Option<&Built>, hit: &Hit) -> String {
    let project = app.project();
    let strings = app.strings();
    let units = project.display_units;
    // The origin's preset, on the board shown.
    let board = built.map_or_else(
        || project.stock.clone(),
        |built| Stock {
            width_mm: built.plan.width_mm,
            height_mm: built.plan.height_mm,
            thickness_mm: Some(built.plan.thickness_mm),
        },
    );
    let zero = project.origin.xy.point_mm(&board);
    let zero_z = match project.origin.z {
        OriginZ::StockTop => board.thickness_mm.unwrap_or(0.0),
        OriginZ::StockBottom => 0.0,
    };
    let at = strings.fmt(
        StrKey::SimViewReadout,
        &[
            ("x", &coordinate(hit.at.x - zero.x, units)),
            ("y", &coordinate(hit.at.y - zero.y, units)),
            ("z", &coordinate(hit.at.z - zero_z, units)),
        ],
    );
    let cut_by = hit
        .cut_by
        .and_then(|run| built?.plan.runs.get(usize::from(run)))
        .and_then(|run| project.operations.iter().find(|op| *op.id() == run.op));
    match cut_by {
        Some(operation) => strings.fmt(
            StrKey::SimViewReadoutCut,
            &[("at", &at), ("operation", operation.name())],
        ),
        None => at,
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "test code reports failure by panicking"
    )]

    use super::*;

    /// Capability 114: a row is ahead until the playback enters it, cut
    /// once it has left, and in between says how far; at the very start
    /// nothing is cut, and the moment between two rows belongs to the
    /// one before.
    #[test]
    fn a_row_is_ahead_cutting_or_done() {
        let span = (10.0, 30.0);
        assert_eq!(Reached::of(0.0, (0.0, 10.0)), Reached::Ahead);
        assert_eq!(Reached::of(5.0, span), Reached::Ahead);
        assert_eq!(Reached::of(10.0, span), Reached::Ahead);
        assert_eq!(Reached::of(10.0, (0.0, 10.0)), Reached::Done);
        assert_eq!(Reached::of(15.0, span), Reached::Cutting(0.25));
        assert_eq!(Reached::of(30.0, span), Reached::Done);
        assert_eq!(Reached::of(99.0, span), Reached::Done);
        // A run of no time at all is done once the playback is past it.
        assert_eq!(Reached::of(10.0, (10.0, 10.0)), Reached::Done);
    }

    #[test]
    fn the_clock_reads_minutes_and_seconds() {
        assert_eq!(clock(0.0), "00:00");
        assert_eq!(clock(190.4), "03:10");
        assert_eq!(clock(860.0), "14:20");
        assert_eq!(clock(3790.0), "1:03:10");
        assert_eq!(clock(-4.0), "00:00");
        assert_eq!(clock(f64::NAN), "00:00");
    }

    #[test]
    fn a_fraction_is_a_whole_percentage_that_never_reads_done_early() {
        assert_eq!(percent(0.0), "0");
        assert_eq!(percent(0.428), "42");
        assert_eq!(percent(0.999), "99");
        assert_eq!(percent(1.0), "100");
    }
}
