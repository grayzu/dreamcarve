// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Warnings panel (`docs/specs/phase-4/design.md`, "Warnings";
//! functional spec, capability 76, acceptance 82): `check` on the project
//! as it stands. Validation's problems (synchronous on every snapshot),
//! each operation's generation findings, `finish`'s findings and the
//! notes of opening or importing a file, merged as `build` merges them,
//! tiered, and grouped by operation with project-level rows first.
//!
//! Each row's [`Row::line`] is the line `dreamcarve check` prints for it:
//! `plaque.dcv:22:5: warning: unused_tool: …` when its field is in the
//! file, `warning: …` when not, `plaque.dcv: note: …` for a note. A
//! position is the file's only while the project is what the file holds:
//! once it is edited, the lines carry none. [`copy_text`] is `check`'s
//! text: its notes, errors, warnings and carve lines in the order it
//! prints them, then its count line.
//!
//! Tiers follow PureCutCNC's `src/engine/toolpaths/warningCodes.ts`
//! (`warningSeverity`): tiers at presentation, codes in the engine. The
//! tier is [`dc_job::Tier`]'s; `check` labels a hint `warning`, as the
//! panel's text does.
//!
//! Deduplication is `build`'s: [`dc_job::generate_operation`] leaves out
//! the warnings validation already reports (`clearing_not_descending`,
//! `inlay_plug_base_thin`), so validation's findings followed by each
//! operation's and then `finish`'s are `build`'s list, and nothing here
//! drops or reorders a finding but the grouping.
//!
//! Carve notes (slice 28, #620; capability 76a): the summary `check`
//! prints for each operation that carves ([`dc_job::carve_facts`]) is a
//! [`Level::Note`] row last in its operation's group, laid out as
//! labelled facts with a heading per board when there are two
//! ([`CarveNote`]). It is information, so [`counts`] skips it; its
//! [`Row::line`] is `check`'s lines for it, which [`copy_text`] writes
//! after the warnings as `check` prints them. As `check` prints them only
//! when nothing stopped the build, the notes show only while there is no
//! error.

use dc_job::summary::{CarveFacts, Fact, Top};
use dc_job::{Finding, Findings, ShapeRef, Tier};
use dc_project::{OperationId, Project, SourceMap};

use crate::app::App;
use crate::selection::Selection;
use crate::strings::{StrKey, Strings};

/// The Warnings chapter of the book, whose entries the rows' codes link
/// to.
pub const CHAPTER_URL: &str = "https://grayzu.github.io/dreamcarve/user-guide/warnings.html";

/// A row's tier, as the panel draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// Export refuses until it is fixed.
    Error,
    /// The program is safe; the user should know.
    Warning,
    /// Advice about the job's setup (`tool_change_avoidable`,
    /// `unused_tool`).
    Hint,
    /// What opening or importing a file changed or left out, or a carve
    /// note: information, not a problem.
    Note,
}

impl From<Tier> for Level {
    fn from(tier: Tier) -> Self {
        match tier {
            Tier::Error => Self::Error,
            Tier::Warning => Self::Warning,
            Tier::Hint => Self::Hint,
        }
    }
}

/// One row of the panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Its tier.
    pub level: Level,
    /// The finding's stable code; errors and notes have none.
    pub code: Option<String>,
    /// The message, as `check` prints it after the code.
    pub message: String,
    /// The field it is about, as validation names it
    /// (`operations[o1].depth_mm`).
    pub field: Option<String>,
    /// The operation it is about.
    pub op: Option<OperationId>,
    /// The shape it is about.
    pub shape: Option<ShapeRef>,
    /// The whole line as `check` prints it; a carve note's lines, one
    /// per board, joined by newlines.
    pub line: String,
    /// The carve note's facts, for a carve note; `None` for every other
    /// row.
    pub note: Option<CarveNote>,
}

/// A carve note laid out (capability 76a): a section per board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CarveNote {
    /// Its sections, the female board's first.
    pub sections: Vec<NoteSection>,
}

/// The facts of one board of a carve note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteSection {
    /// *Female board* or *Male board*, when the note has two boards.
    pub heading: Option<String>,
    /// Its facts, one per line, the deepest cut first.
    pub facts: Vec<NoteFact>,
}

/// One labelled fact: *Deepest cut* · *1.658 mm below the stock top*.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteFact {
    /// The label.
    pub label: String,
    /// What it says.
    pub value: String,
}

impl Row {
    /// What clicking the row selects (capability 76): the operation when
    /// the row names one of its fields (the editor then focuses it), else
    /// the shape it is about, else its operation.
    #[must_use]
    pub fn selection(&self) -> Option<Selection> {
        match (&self.field, &self.op, &self.shape) {
            (Some(_), Some(op), _) => Some(Selection::Operation(op.clone())),
            (_, _, Some(shape)) => Some(Selection::Instances(vec![shape.instance.clone()])),
            (_, Some(op), None) => Some(Selection::Operation(op.clone())),
            (_, None, None) => None,
        }
    }

    /// The code's entry in the Warnings chapter, when the row has a code.
    #[must_use]
    pub fn chapter_url(&self) -> Option<String> {
        self.code
            .as_ref()
            .map(|code| format!("{CHAPTER_URL}#{code}"))
    }
}

/// The rows about one operation, or about the project.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The operation; `None` for the project-level rows.
    pub op: Option<OperationId>,
    /// Its rows, in `check`'s order.
    pub rows: Vec<Row>,
    /// Whether the drawer names the group above its rows: only when there
    /// are two or more groups, since a lone heading would label the only
    /// thing there (capability 76). A row still names its operation in
    /// its message.
    pub heading: bool,
}

/// How many rows of each tier: the status bar's counts and the drawer's
/// title.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Errors.
    pub errors: usize,
    /// Warnings, hints aside.
    pub warnings: usize,
    /// Hints.
    pub hints: usize,
    /// Notes.
    pub notes: usize,
}

/// Validation's findings, then generation's and `finish`'s, as `build`
/// collects them.
#[must_use]
pub fn merge(validation: &Findings, generation: Findings) -> Findings {
    let mut merged = validation.clone();
    merged.extend(generation);
    merged
}

/// The file name the lines are prefixed with and where the file's values
/// are, as `check` prints them run in the file's folder.
struct Location<'a> {
    name: String,
    map: Option<&'a SourceMap>,
}

impl Location<'_> {
    fn of(app: &App) -> Location<'_> {
        Location {
            name: app.file_name(),
            map: app.source_map(),
        }
    }

    /// `finding` as `check` prints it: `name:line:column: label: message`
    /// when its field is in the file, `label: message` otherwise, the code
    /// before the message when it has one.
    fn describe(&self, label: &str, finding: &Finding) -> String {
        let at = finding.field.as_deref().and_then(|f| self.map?.locate(f));
        let message = match &finding.code {
            Some(code) => format!("{code}: {}", finding.message),
            None => finding.message.clone(),
        };
        match at {
            Some(at) => format!("{}:{at}: {label}: {message}", self.name),
            None => format!("{label}: {message}"),
        }
    }
}

/// Every row in `check`'s order: the notes, the errors, the warnings
/// (hints among them), then the carve notes.
fn rows(app: &App) -> Vec<Row> {
    let location = Location::of(app);
    let notes = app.notes().iter().map(|note| Row {
        level: Level::Note,
        code: None,
        message: note.clone(),
        field: None,
        op: None,
        shape: None,
        line: format!("{}: note: {note}", location.name),
        note: None,
    });
    let findings = app.findings();
    let labelled = findings
        .errors
        .iter()
        .map(|f| ("error", f))
        .chain(findings.warnings.iter().map(|f| ("warning", f)));
    let found = labelled.map(|(label, finding)| Row {
        level: Level::from(finding.tier),
        code: finding.code.clone(),
        message: finding.message.clone(),
        field: finding.field.clone(),
        op: finding.op.clone(),
        shape: finding.shape.clone(),
        line: location.describe(label, finding),
        note: None,
    });
    // `check` prints the carves only when the build was not stopped.
    let carves = if findings.has_errors() {
        Vec::new()
    } else {
        app.project()
            .operations
            .iter()
            .filter_map(|op| carve_facts(app, op.id()))
            .map(|facts| carve_row(app.strings(), facts))
            .collect()
    };
    notes.chain(found).chain(carves).collect()
}

/// The carve summary of operation `op` (capability 76a), from the
/// generation result the app holds, which the worker found when it
/// generated it; `None` for a kind `check` prints no such line for, and
/// while the operation has no current result (before it generates, while
/// it regenerates, or when validation stops generation, as `check` then
/// prints none).
#[must_use]
pub fn carve_facts<'a>(app: &'a App, op: &OperationId) -> Option<&'a CarveFacts> {
    let generation = app.generation();
    generation
        .result(op)
        .filter(|_| generation.is_current(op))
        .and_then(|result| result.carves.iter().find(|c| &c.op == op))
}

/// The note row of `facts`.
fn carve_row(strings: &Strings, facts: &CarveFacts) -> Row {
    let text = facts.lines().join("\n");
    Row {
        level: Level::Note,
        code: None,
        message: text.clone(),
        field: None,
        op: Some(facts.op.clone()),
        shape: None,
        line: text,
        note: Some(carve_note(strings, facts)),
    }
}

/// `facts` laid out as labelled facts, under a heading per board when
/// there are two.
#[must_use]
pub fn carve_note(strings: &Strings, facts: &CarveFacts) -> CarveNote {
    let headed = facts.boards.len() >= 2;
    CarveNote {
        sections: facts
            .boards
            .iter()
            .map(|board| NoteSection {
                heading: board.board.filter(|_| headed).map(|b| {
                    strings
                        .get(match b {
                            dc_project::Board::Female => StrKey::AppCarveFemaleBoard,
                            dc_project::Board::Male => StrKey::AppCarveMaleBoard,
                        })
                        .to_owned()
                }),
                facts: board
                    .facts
                    .iter()
                    .map(|fact| labelled(strings, fact))
                    .collect(),
            })
            .collect(),
    }
}

/// `fact` with its label, as the drawer lays it out.
fn labelled(strings: &Strings, fact: &Fact) -> NoteFact {
    let (label, value) = match fact {
        Fact::DeepestCut { depth, below } => (
            StrKey::AppCarveDeepestCut,
            strings.fmt(
                match below {
                    Top::Stock => StrKey::AppCarveBelowStockTop,
                    Top::MaleBoard => StrKey::AppCarveBelowMaleTop,
                },
                &[("depth", depth)],
            ),
        ),
        Fact::FlatDepth(None) => (
            StrKey::AppCarveFlatDepth,
            strings.get(StrKey::AppCarveNoFlatDepth).to_owned(),
        ),
        Fact::FlatDepth(Some(depth)) => (
            StrKey::AppCarveFlatDepth,
            strings.fmt(StrKey::AppCarveClamped, &[("depth", depth)]),
        ),
        Fact::FlatFloor { depth } => (
            StrKey::AppCarveFlatFloor,
            strings.fmt(StrKey::AppCarveFlatFloorAt, &[("depth", depth)]),
        ),
        Fact::StartPlane { depth } => (
            StrKey::AppCarveStartPlane,
            strings.fmt(StrKey::AppCarveBelowMaleTop, &[("depth", depth)]),
        ),
        Fact::SurroundFloor { depth } => (
            StrKey::AppCarveSurroundFloor,
            strings.fmt(StrKey::AppCarveBelowStartPlane, &[("depth", depth)]),
        ),
        Fact::FloorRidges {
            straight,
            corner,
            cleared,
        } => {
            let heights = match corner {
                Some(corner) => strings.fmt(
                    StrKey::AppCarveRidgeHeights,
                    &[("straight", straight), ("corner", corner)],
                ),
                None => straight.clone(),
            };
            let key = if *cleared {
                StrKey::AppCarveRidgesCleared
            } else {
                StrKey::AppCarveRidgesAlone
            };
            (
                StrKey::AppCarveFloorRidges,
                strings.fmt(key, &[("heights", &heights)]),
            )
        }
        Fact::Band {
            boundary,
            narrowest,
            edge_pass,
        } => {
            let edge = match edge_pass {
                Some(tool) => strings.fmt(StrKey::AppCarveOpenEdge, &[("tool", tool)]),
                None => strings.get(StrKey::AppCarveVWall).to_owned(),
            };
            let boundary = boundary.join(strings.get(StrKey::AppCarveListSeparator));
            (
                StrKey::AppCarveBand,
                strings.fmt(
                    StrKey::AppCarveDrawnBand,
                    &[
                        ("boundary", &boundary),
                        ("narrowest", narrowest),
                        ("edge", &edge),
                    ],
                ),
            )
        }
        Fact::BandEdge { tool, rounding } => (
            StrKey::AppCarveBandEdge,
            strings.fmt(
                StrKey::AppCarveUprightEdge,
                &[("tool", tool), ("rounding", rounding)],
            ),
        ),
        Fact::CutOut {
            inset,
            outside,
            through,
        } => (
            StrKey::AppCarveCutOut,
            strings.fmt(
                StrKey::AppCarveFreesPlug,
                &[("inset", inset), ("outside", outside), ("through", through)],
            ),
        ),
    };
    NoteFact {
        label: strings.get(label).to_owned(),
        value,
    }
}

/// The status bar's message for operation `op` while it is selected
/// (capabilities 50b and 76a): its name and the first facts of its first
/// board, `Lettering · deepest cut 1.658 mm, no flat depth`; `None` when
/// it has no carve summary.
#[must_use]
pub fn carve_message(app: &App, op: &OperationId) -> Option<String> {
    let strings = app.strings();
    let facts = carve_facts(app, op)?;
    let board = facts.boards.first()?;
    let parts: Vec<String> = board
        .facts
        .iter()
        .take(2)
        .filter_map(|fact| match fact {
            Fact::DeepestCut { depth, .. } => {
                Some(strings.fmt(StrKey::AppCarveSummaryDeepest, &[("depth", depth)]))
            }
            Fact::FlatDepth(None) => Some(strings.get(StrKey::AppCarveSummaryNoFlat).to_owned()),
            Fact::FlatDepth(Some(depth)) => {
                Some(strings.fmt(StrKey::AppCarveSummaryFlatDepth, &[("depth", depth)]))
            }
            Fact::FlatFloor { depth } => {
                Some(strings.fmt(StrKey::AppCarveSummaryFlatFloor, &[("depth", depth)]))
            }
            Fact::StartPlane { depth } => {
                Some(strings.fmt(StrKey::AppCarveSummaryStartPlane, &[("depth", depth)]))
            }
            _ => None,
        })
        .collect();
    let facts_text = parts.join(strings.get(StrKey::AppCarveSummarySeparator));
    Some(strings.fmt(
        StrKey::AppCarveSummary,
        &[("name", &facts.name), ("facts", &facts_text)],
    ))
}

/// Where `op`'s group goes: the project-level group first, then the
/// operations in the project's order, then any the project does not have.
fn group_rank(project: &Project, op: Option<&OperationId>) -> usize {
    op.map_or(0, |op| {
        1 + project
            .operations
            .iter()
            .position(|o| o.id() == op)
            .unwrap_or(project.operations.len())
    })
}

/// The panel's rows, grouped by operation (capability 76): project-level
/// rows first (thickness unset, a tool-change height on a post without
/// `G53`, `tool_change_avoidable`, the notes), then each operation's in
/// the project's order, each group in `check`'s order, headed only when
/// there are two or more.
#[must_use]
pub fn panel(app: &App) -> Vec<Group> {
    let project = app.project();
    let mut groups: Vec<Group> = Vec::new();
    for row in rows(app) {
        match groups.iter_mut().find(|g| g.op == row.op) {
            Some(group) => group.rows.push(row),
            None => groups.push(Group {
                op: row.op.clone(),
                rows: vec![row],
                heading: false,
            }),
        }
    }
    // Stable: operations the project lacks keep their first appearance.
    groups.sort_by_key(|g| group_rank(project, g.op.as_ref()));
    let headed = groups.len() >= 2;
    for group in &mut groups {
        group.heading = headed;
    }
    groups
}

/// The lines `check` prints for the project as it stands, in its order,
/// without its count line.
#[must_use]
pub fn lines(app: &App) -> Vec<String> {
    rows(app)
        .iter()
        .flat_map(|r| r.line.lines().map(str::to_owned).collect::<Vec<_>>())
        .collect()
}

/// How many rows of each tier.
#[must_use]
pub fn counts(app: &App) -> Counts {
    let mut counts = Counts::default();
    // A carve note is information, not counted (capability 76a).
    for row in rows(app).into_iter().filter(|r| r.note.is_none()) {
        match row.level {
            Level::Error => counts.errors += 1,
            Level::Warning => counts.warnings += 1,
            Level::Hint => counts.hints += 1,
            Level::Note => counts.notes += 1,
        }
    }
    counts
}

/// What *Copy* puts on the clipboard (capability 76): [`lines`], then
/// `check`'s count line (`plaque.dcv: 0 error(s), 6 warning(s)`, hints
/// counted as warnings as `check` counts them), each ending in a newline.
#[must_use]
pub fn copy_text(app: &App) -> String {
    let counts = counts(app);
    let mut text = String::new();
    for line in lines(app) {
        text.push_str(&line);
        text.push('\n');
    }
    text.push_str(&format!(
        "{}: {} error(s), {} warning(s)\n",
        app.file_name(),
        counts.errors,
        counts.warnings + counts.hints
    ));
    text
}
