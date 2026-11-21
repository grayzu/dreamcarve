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
//! tier is [`dc_job::Tier`]'s. The hint tier is the app's own (capability
//! 76): `check` has two labels and prints a hint as a `warning:` line, so
//! a row's [`Row::line`] and [`copy_text`], which are `check`'s text, say
//! `warning` for a hint, and everything the drawer itself says of the row
//! ([`Row::level`], [`Row::name`], [`counts`]) says hint.
//!
//! Deduplication is `build`'s: [`dc_job::generate_operation`] leaves out
//! the warnings validation already reports (`clearing_not_descending`,
//! `inlay_plug_base_thin`), so validation's findings followed by each
//! operation's and then `finish`'s are `build`'s list, and nothing here
//! drops or reorders a finding but the grouping.
//!
//! What the app shows of a finding is its message with names in it
//! ([`present`]): the engine's texts, which the CLI prints too, name an
//! operation or a tool by its id where they have no name to hand
//! (`operation `op4` cuts off the stock`), and the app shows ids nowhere
//! else. A row keeps both: [`Row::message`] and [`Row::line`] are
//! `check`'s, which [`copy_text`] writes, and [`Row::shown`] is what the
//! drawer draws. [`App::warnings`](crate::App::warnings) carries the shown
//! form, so a field's line, a row's hover and the status bar read the
//! same words as the drawer.
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
    /// The message as the drawer shows it ([`present`]): `message` with
    /// the name of each operation, tool, shape or picture it gives by id
    /// alone.
    pub shown: String,
    /// The field it is about, as validation names it
    /// (`operations[o1].depth_mm`).
    pub field: Option<String>,
    /// The operation it is about.
    pub op: Option<OperationId>,
    /// The shape it is about.
    pub shape: Option<ShapeRef>,
    /// The whole line as `check` prints it; a carve note's lines, one
    /// per board, joined by newlines. `check` labels a hint `warning`.
    /// Empty for one of the app's own hints ([`App::hints`]), which
    /// `check` does not print.
    pub line: String,
    /// The row's name for assistive tools: what the drawer shows of it,
    /// in `check`'s order but in the drawer's words, `hint: unused_tool:
    /// tool …`. Its tier is the drawer's ([`Row::level`]), so a hint reads
    /// as a hint, its message is the shown one ([`Row::shown`]), and it
    /// carries no file position; a carve note's is its [`Row::line`].
    pub name: String,
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

/// What names a finding's row in the drawer, for the trip from a field's
/// glyph to its row (capability 66): the row's name as the drawer words
/// it, and the field it is about, which tells two rows of one wording
/// apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RowKey {
    /// [`Row::name`].
    pub name: String,
    /// [`Row::field`].
    pub field: Option<String>,
}

/// The drawer row of `finding`, one of
/// [`App::warnings`](crate::App::warnings)'s (its message already worded
/// for the app).
#[must_use]
pub fn key_of(strings: &Strings, finding: &Finding) -> RowKey {
    RowKey {
        name: row_name(
            strings,
            Level::from(finding.tier),
            finding.code.as_deref(),
            &finding.message,
        ),
        field: finding.field.clone(),
    }
}

impl Row {
    /// What names the row for a field's glyph ([`key_of`]).
    #[must_use]
    pub fn key(&self) -> RowKey {
        RowKey {
            name: self.name.clone(),
            field: self.field.clone(),
        }
    }

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

/// The name `project` gives the thing `id` is, when `noun`, the word an
/// engine message puts before the id, says what kind of thing that is.
/// `None` for another word, for an id the project does not have (a
/// dangling reference: the id is what is wrong), and for a thing whose
/// name is empty or is its id.
fn named<'a>(project: &'a Project, noun: &str, id: &str) -> Option<&'a str> {
    let noun = noun.to_lowercase();
    let name = match noun.strip_suffix('s').unwrap_or(&noun) {
        "operation" => project
            .operations
            .iter()
            .find(|o| o.id().as_str() == id)
            .map(dc_project::Operation::name),
        "tool" => project
            .tools
            .iter()
            .find(|t| t.id.as_str() == id)
            .map(|t| t.name.as_str()),
        "instance" | "shape" => project
            .instances
            .iter()
            .find(|i| i.id.as_str() == id)
            .map(|i| i.name.as_str()),
        "bitmap" | "picture" => project
            .bitmaps
            .iter()
            .find(|b| b.id.as_str() == id)
            .map(|b| b.name.as_str()),
        _ => None,
    };
    name.filter(|name| !name.is_empty() && *name != id)
}

/// Whether `noun` is a word an engine message puts before an id.
fn names_an_id(noun: &str) -> bool {
    let noun = noun.to_lowercase();
    matches!(
        noun.strip_suffix('s').unwrap_or(&noun),
        "operation" | "tool" | "instance" | "shape" | "bitmap" | "picture"
    )
}

/// The word `text` ends with before a single space: the noun an engine
/// message puts before a backticked id (`operation `, `tool `). Empty
/// when `text` ends any other way (`(`, the start of a message).
fn noun_before(text: &str) -> &str {
    text.strip_suffix(' ').map_or("", |before| {
        before
            .rsplit(|c: char| !c.is_alphabetic())
            .next()
            .unwrap_or("")
    })
}

/// `message`, a finding's text, as the app shows it (capability 76: "the
/// message with the operation and shape named"). The engine's messages
/// are the CLI's too, and name an operation, a tool, a shape or a
/// picture by its id where they have no name to hand: `operation `op4`
/// cuts off the stock`. The app shows ids nowhere else, so each id that
/// stands alone after its noun gains the name it has in `project`, in
/// the form the messages that carry both already use: `operation `Cut
/// out` (`op4`) cuts off the stock`.
///
/// Left as written: an id already beside its name, in either order; an
/// id the project does not have, since the id is then what is wrong; and
/// any backticked word that does not follow `operation`, `tool`,
/// `instance`, `shape`, `bitmap` or `picture`. An empty id after one of
/// those (an operation with no tool picked: `uses tool ```) reads *(none
/// chosen)*. Presentation only: [`Row::line`] and [`copy_text`] keep the
/// engine's text, which must equal `check`'s.
#[must_use]
pub fn present(project: &Project, strings: &Strings, message: &str) -> String {
    let mut out = String::with_capacity(message.len() + 16);
    let mut rest = message;
    while let Some((before, tail)) = rest.split_once('`') {
        out.push_str(before);
        let Some((id, after)) = tail.split_once('`') else {
            // An unpaired tick: the rest as written.
            out.push('`');
            rest = tail;
            break;
        };
        let noun = noun_before(&out);
        // `tool `t2` (`1/4 end mill`)`: the name already follows.
        let named_after = after.starts_with(" (`");
        match named(project, noun, id).filter(|_| !named_after) {
            Some(name) => {
                out.push('`');
                out.push_str(name);
                out.push_str("` (`");
                out.push_str(id);
                out.push_str("`)");
            }
            None if id.is_empty() && names_an_id(noun) => {
                out.push_str(strings.get(StrKey::AppWarningsNoneChosen));
            }
            None => {
                out.push('`');
                out.push_str(id);
                out.push('`');
            }
        }
        rest = after;
    }
    out.push_str(rest);
    out
}

/// `findings`' errors then warnings, as `check` lists them, each with
/// its message as the app shows it ([`present`]): what
/// [`App::warnings`](crate::App::warnings) holds.
#[must_use]
pub(crate) fn listed(project: &Project, strings: &Strings, findings: &Findings) -> Vec<Finding> {
    findings
        .errors
        .iter()
        .chain(&findings.warnings)
        .map(|finding| Finding {
            message: present(project, strings, &finding.message),
            ..finding.clone()
        })
        .collect()
}

/// The drawer's word for `level`: a row's chip, and the tier its name
/// starts with.
#[must_use]
pub const fn level_word(level: Level) -> StrKey {
    match level {
        Level::Error => StrKey::AppWarningsLevelError,
        Level::Warning => StrKey::AppWarningsLevelWarning,
        Level::Hint => StrKey::AppWarningsLevelHint,
        Level::Note => StrKey::AppWarningsLevelNote,
    }
}

/// A row's [`Row::name`]: `level`'s word, the code when it has one, and
/// `message`.
fn row_name(strings: &Strings, level: Level, code: Option<&str>, message: &str) -> String {
    let word = strings.get(level_word(level));
    match code {
        Some(code) => format!("{word}: {code}: {message}"),
        None => format!("{word}: {message}"),
    }
}

/// Every row: the app's own hints ([`App::hints`]), then `check`'s in its
/// order: the notes, the errors, the warnings (hints among them), then
/// the carve notes.
fn rows(app: &App) -> Vec<Row> {
    let location = Location::of(app);
    let strings = app.strings();
    let project = app.project();
    let own = app.hints().iter().map(|hint| Row {
        level: Level::Hint,
        code: None,
        message: hint.clone(),
        shown: hint.clone(),
        field: None,
        op: None,
        shape: None,
        line: String::new(),
        name: row_name(strings, Level::Hint, None, hint),
        note: None,
    });
    let notes = app.notes().iter().map(|note| {
        let shown = present(project, strings, note);
        Row {
            level: Level::Note,
            code: None,
            message: note.clone(),
            field: None,
            op: None,
            shape: None,
            line: format!("{}: note: {note}", location.name),
            name: row_name(strings, Level::Note, None, &shown),
            shown,
            note: None,
        }
    });
    let findings = app.findings();
    let labelled = findings
        .errors
        .iter()
        .map(|f| ("error", f))
        .chain(findings.warnings.iter().map(|f| ("warning", f)));
    let found = labelled.map(|(label, finding)| {
        let shown = present(project, strings, &finding.message);
        Row {
            level: Level::from(finding.tier),
            code: finding.code.clone(),
            message: finding.message.clone(),
            field: finding.field.clone(),
            op: finding.op.clone(),
            shape: finding.shape.clone(),
            line: location.describe(label, finding),
            name: row_name(
                strings,
                Level::from(finding.tier),
                finding.code.as_deref(),
                &shown,
            ),
            shown,
            note: None,
        }
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
    own.chain(notes).chain(found).chain(carves).collect()
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
        shown: text.clone(),
        field: None,
        op: Some(facts.op.clone()),
        shape: None,
        line: text.clone(),
        name: text,
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
        Fact::Picture { name, id } => (
            StrKey::AppCarvePicture,
            strings.fmt(StrKey::AppCarvePictureNamed, &[("name", name), ("id", id)]),
        ),
        Fact::Footprint {
            width,
            height,
            x,
            y,
        } => (
            StrKey::AppCarveFootprint,
            strings.fmt(
                StrKey::AppCarveFootprintAt,
                &[("width", width), ("height", height), ("x", x), ("y", y)],
            ),
        ),
        Fact::Grooves {
            count,
            width,
            depth,
        } => (
            StrKey::AppCarveGrooves,
            strings.fmt(
                StrKey::AppCarveGroovesWide,
                &[
                    ("count", &count.to_string()),
                    ("width", width),
                    ("depth", depth),
                ],
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
            Fact::Grooves {
                count,
                width,
                depth,
            } => Some(strings.fmt(
                StrKey::AppCarveSummaryGrooves,
                &[
                    ("count", &count.to_string()),
                    ("width", width),
                    ("depth", depth),
                ],
            )),
            _ => None,
        })
        .take(2)
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

/// The rows that block export: the panel's errors, in its order. The
/// Export step lists them where the export is refused.
#[must_use]
pub fn blocking(app: &App) -> Vec<Row> {
    panel(app)
        .into_iter()
        .flat_map(|group| group.rows)
        .filter(|row| row.level == Level::Error)
        .collect()
}

/// The lines `check` prints for the project as it stands, in its order,
/// without its count line. The app's own hints have none.
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
/// counted as warnings as `check` counts them, the app's own left out),
/// each ending in a newline.
/// It is `check`'s text and not the drawer's: `check` has no hint tier,
/// so a row the drawer calls a hint is a `warning:` line here
/// (acceptance 82), which the button's tooltip says.
#[must_use]
pub fn copy_text(app: &App) -> String {
    let counts = counts(app);
    let mut text = String::new();
    for line in lines(app) {
        text.push_str(&line);
        text.push('\n');
    }
    // The app's own hints are not `check`'s to count.
    let hints = counts.hints.saturating_sub(app.hints().len());
    text.push_str(&format!(
        "{}: {} error(s), {} warning(s)\n",
        app.file_name(),
        counts.errors,
        counts.warnings + hints
    ));
    text
}
