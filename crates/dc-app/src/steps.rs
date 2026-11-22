// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The five steps (`docs/specs/phase-4/redesign.md`, "The steps"; functional
//! spec, capability 50): Design, Stock, Tools, Operations, Export, in the
//! order the work is done. Each step's [`State`] is computed from the
//! project and its findings, so the rail's marks, the header's *Continue*
//! and the step a project opens on are tested without a window.
//!
//! A step is **complete**, **needs attention** (with the worse of a warning
//! and an error), or is **empty**: nothing in it yet and nothing wrong.
//! Hints never ask for attention. Nothing is locked: the states guide, and
//! any step can be opened at any time.
//!
//! | Step | Complete when | Needs attention when |
//! | --- | --- | --- |
//! | Design | The project has a shape or a picture | A finding is about a shape, an artwork check fired on the shapes, or an importer's warning is still noted |
//! | Stock | The thickness is set and every shape is on the stock | A stock or origin field has a problem, or a shape reaches past the stock |
//! | Tools | A tool exists | A finding is about a tool |
//! | Operations | An enabled operation exists | A finding is about an operation |
//! | Export | The last export wrote the current snapshot | An error blocks the export of an enabled operation, or an output setting has a problem |
//!
//! Two rules keep the marks quiet where they would otherwise shout. A
//! blank project (no shape, picture or operation) has no marks: its unset
//! thickness is the Stock step's error only once there is something to
//! cut, and Export has nothing to refuse until an operation is enabled.
//! The status bar's counts and the rail's Warnings badge follow the same
//! rule ([`counts`]), so the three agree: a blank project shows no
//! problem. And a tool no enabled operation uses asks for nothing, whatever it
//! lacks: validation reports no problem for it, since nothing cuts with
//! it.

use dc_geometry::EPS_MM;
use dc_job::{Finding, Tier};
use dc_project::Project;

use crate::app::App;
use crate::generation::SnapshotId;
use crate::selection::Selection;
use crate::status::counted;
use crate::strings::StrKey;
use crate::{units, warnings};

/// The field validation names when the stock has no thickness.
const STOCK_THICKNESS: &str = "stock.thickness_mm";

/// One of the five steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Step {
    /// Bringing geometry in: drawings, pictures, the Design Tree.
    Design,
    /// Units, the stock's size and the origin.
    Stock,
    /// The project's tools.
    Tools,
    /// The operations, in cut order.
    Operations,
    /// Checking, the output settings and the export.
    Export,
}

impl Step {
    /// Every step, in the workflow's order: the rail's.
    pub const ALL: [Self; 5] = [
        Self::Design,
        Self::Stock,
        Self::Tools,
        Self::Operations,
        Self::Export,
    ];

    /// Its place in [`Step::ALL`].
    #[must_use]
    pub const fn index(self) -> usize {
        match self {
            Self::Design => 0,
            Self::Stock => 1,
            Self::Tools => 2,
            Self::Operations => 3,
            Self::Export => 4,
        }
    }

    /// Its name: the rail's label and the step panel's title.
    #[must_use]
    pub const fn label(self) -> StrKey {
        match self {
            Self::Design => StrKey::StepsLabelDesign,
            Self::Stock => StrKey::StepsLabelStock,
            Self::Tools => StrKey::StepsLabelTools,
            Self::Operations => StrKey::StepsLabelOperations,
            Self::Export => StrKey::StepsLabelExport,
        }
    }
}

/// How much a step that needs attention matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The program is safe; the user should look.
    Warning,
    /// Export refuses until it is fixed.
    Error,
}

/// A step's state: the mark on its rail button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Nothing in it yet, and nothing wrong: no mark.
    Empty,
    /// Done: a check.
    Complete,
    /// Something in it has a problem: a dot in the severity's colour.
    Attention(Severity),
}

/// Every step's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct States([State; 5]);

impl States {
    /// The states of the five steps, in [`Step::ALL`]'s order.
    #[must_use]
    pub const fn new(states: [State; 5]) -> Self {
        Self(states)
    }

    /// The state of `step`.
    #[must_use]
    pub const fn of(&self, step: Step) -> State {
        let [design, stock, tools, operations, export] = self.0;
        match step {
            Step::Design => design,
            Step::Stock => stock,
            Step::Tools => tools,
            Step::Operations => operations,
            Step::Export => export,
        }
    }
}

impl Default for States {
    /// Every step empty: a blank project's.
    fn default() -> Self {
        Self([State::Empty; 5])
    }
}

/// What the steps read that the project does not hold: this session's
/// import warnings and its last export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Session {
    /// How many of an importer's warnings about a drawing are still noted
    /// in the Warnings panel: what an import left out or changed
    /// ([`crate::import::DrawingImport::left_out`]).
    pub import_warnings: usize,
    /// The snapshot the last successful export wrote, when there was one.
    pub exported: Option<SnapshotId>,
}

/// The step that edits what `finding` is about: an operation's findings
/// are Operations'; else by the root of its field path, stock and origin
/// to Stock, a tool's to Tools, a shape's or a placed tab's to Design.
/// The output settings (heights, tolerances, the post-processor) and a
/// finding about no one field are Export's.
#[must_use]
pub fn step_of(finding: &Finding) -> Step {
    if finding.op.is_some() {
        return Step::Operations;
    }
    match finding.field.as_deref() {
        Some(field) => step_of_field(field),
        None if finding.shape.is_some() => Step::Design,
        None => Step::Export,
    }
}

/// The step that edits the field at `path`, as validation writes a field
/// (`tools[t1].feed_mm_min`): by the path's root.
#[must_use]
pub fn step_of_field(path: &str) -> Step {
    match path.split(['.', '[']).next().unwrap_or(path) {
        "operations" => Step::Operations,
        "tools" => Step::Tools,
        "stock" | "origin" | "display_units" => Step::Stock,
        "instances" | "definitions" | "tabs" | "bitmaps" => Step::Design,
        _ => Step::Export,
    }
}

/// Whether `project` has nothing to cut yet: no shape, no picture and no
/// operation. The Viewport shows the start screen for it.
#[must_use]
pub fn blank(project: &Project) -> bool {
    project.instances.is_empty() && project.bitmaps.is_empty() && project.operations.is_empty()
}

/// Whether a visible shape reaches past the stock, by more than
/// [`EPS_MM`].
#[must_use]
pub fn off_stock(app: &App) -> bool {
    let project = app.project();
    let Some(bounds) = app.scene().shapes_bounds() else {
        return false;
    };
    bounds.min.x < -EPS_MM
        || bounds.min.y < -EPS_MM
        || bounds.max.x > project.stock.width_mm + EPS_MM
        || bounds.max.y > project.stock.height_mm + EPS_MM
}

/// What a finding of `tier` asks of its step; a hint asks nothing.
const fn severity(tier: Tier) -> Option<Severity> {
    match tier {
        Tier::Error => Some(Severity::Error),
        Tier::Warning => Some(Severity::Warning),
        Tier::Hint => None,
    }
}

/// The worst severity among `app`'s findings that belong to `step`. On a
/// blank project the unset thickness is not counted.
fn worst(app: &App, step: Step) -> Option<Severity> {
    let blank = blank(app.project());
    app.warnings()
        .iter()
        .filter(|f| step_of(f) == step)
        .filter(|f| !(blank && f.field.as_deref() == Some(STOCK_THICKNESS)))
        .filter_map(|f| severity(f.tier))
        .max()
}

/// How many problems of each tier the status bar and the rail's Warnings
/// badge count: [`warnings::counts`], less the unset thickness on a blank
/// project, by the rule the marks follow ([`states`]). `check` reports it
/// and the Warnings drawer lists it, but with nothing to cut it is not
/// yet a problem to count.
#[must_use]
pub fn counts(app: &App) -> warnings::Counts {
    let mut counts = warnings::counts(app);
    if !blank(app.project()) {
        return counts;
    }
    let findings = app.findings();
    for finding in findings
        .errors
        .iter()
        .chain(&findings.warnings)
        .filter(|f| f.field.as_deref() == Some(STOCK_THICKNESS))
    {
        let tier = match warnings::Level::from(finding.tier) {
            warnings::Level::Error => &mut counts.errors,
            warnings::Level::Warning => &mut counts.warnings,
            warnings::Level::Hint => &mut counts.hints,
            warnings::Level::Note => &mut counts.notes,
        };
        *tier = tier.saturating_sub(1);
    }
    counts
}

/// `attention` as a state, else complete when `complete`, else empty.
const fn state(attention: Option<Severity>, complete: bool) -> State {
    match attention {
        Some(severity) => State::Attention(severity),
        None if complete => State::Complete,
        None => State::Empty,
    }
}

/// Every step's state for `app`'s current snapshot in `session`.
#[must_use]
pub fn states(app: &App, session: &Session) -> States {
    let project = app.project();
    let enabled = project
        .operations
        .iter()
        .any(dc_project::Operation::enabled);

    let imported = (session.import_warnings > 0).then_some(Severity::Warning);
    // The artwork checks are about the shapes, whichever they name.
    let artwork = app
        .artwork_checks()
        .iter()
        .filter_map(|check| severity(check.tier()))
        .max();
    let design = state(
        worst(app, Step::Design).max(imported).max(artwork),
        !project.instances.is_empty() || !project.bitmaps.is_empty(),
    );

    let off = off_stock(app).then_some(Severity::Warning);
    let stock = state(
        worst(app, Step::Stock).max(off),
        project.stock.thickness_mm.is_some(),
    );

    let tools = state(worst(app, Step::Tools), !project.tools.is_empty());
    let operations = state(worst(app, Step::Operations), enabled);

    let export = if session.exported == Some(app.snapshot()) {
        State::Complete
    } else {
        let blocked = (enabled && app.findings().has_errors()).then_some(Severity::Error);
        state(worst(app, Step::Export).max(blocked), false)
    };
    States::new([design, stock, tools, operations, export])
}

/// The states to show: `now`, except that while toolpaths regenerate
/// (`generating`) Operations and Export keep the attention mark `shown`
/// had until `now` has one at least as bad. An operation's findings are
/// gone from the moment it is edited until its result is back, and the
/// mark would blink off and on with every edit.
#[must_use]
pub fn settle(shown: &States, now: States, generating: bool) -> States {
    if !generating {
        return now;
    }
    States::new(Step::ALL.map(|step| {
        let held = matches!(step, Step::Operations | Step::Export);
        match (shown.of(step), now.of(step)) {
            (State::Attention(was), State::Attention(is)) if held && is < was => {
                State::Attention(was)
            }
            (State::Attention(was), State::Empty | State::Complete) if held => {
                State::Attention(was)
            }
            (_, is) => is,
        }
    }))
}

/// The step *Continue* goes to from `current`: the first after it that
/// is not complete, looking on round to the steps before it; Export when
/// every other step is complete.
#[must_use]
pub fn next(states: &States, current: Step) -> Step {
    Step::ALL
        .into_iter()
        .cycle()
        .skip(current.index() + 1)
        .take(Step::ALL.len() - 1)
        .find(|step| states.of(*step) != State::Complete)
        .unwrap_or(Step::Export)
}

/// The step a newly opened project starts on: the first that is not
/// complete, so an empty project opens on Design and a finished one on
/// Export.
#[must_use]
pub fn opening(states: &States) -> Step {
    Step::ALL
        .into_iter()
        .find(|step| states.of(*step) != State::Complete)
        .unwrap_or(Step::Export)
}

/// The step that edits `selection`: Operations for an operation, Design
/// for shapes or placed tabs.
#[must_use]
pub const fn owner(selection: &Selection) -> Option<Step> {
    match selection {
        Selection::Operation(_) => Some(Step::Operations),
        Selection::Instances(_) | Selection::Tabs(_) | Selection::Bitmaps(_) => Some(Step::Design),
        Selection::Nothing => None,
    }
}

/// The one line under `step`'s title: what the step holds, or what to do
/// in it while it holds nothing.
#[must_use]
pub fn summary(app: &App, session: &Session, step: Step) -> String {
    let strings = app.strings();
    let project = app.project();
    let text = |key: StrKey| strings.get(key).to_owned();
    match step {
        Step::Design => {
            let parts: Vec<String> = [
                (
                    project.instances.len(),
                    StrKey::StepsSummaryShape,
                    StrKey::StepsSummaryShapes,
                ),
                (
                    project.bitmaps.len(),
                    StrKey::StepsSummaryPicture,
                    StrKey::StepsSummaryPictures,
                ),
            ]
            .into_iter()
            .filter(|(n, _, _)| *n > 0)
            .map(|(n, one, many)| counted(strings, n, one, many))
            .collect();
            if parts.is_empty() {
                text(StrKey::StepsSummaryDesignEmpty)
            } else {
                parts.join(" · ")
            }
        }
        Step::Stock if off_stock(app) => text(StrKey::StepsSummaryStockOff),
        Step::Stock => {
            let units = project.display_units;
            let length = |mm: f64| units::summary_text(mm, units);
            let width = length(project.stock.width_mm);
            let height = length(project.stock.height_mm);
            let size = [
                ("width", width.as_str()),
                ("height", height.as_str()),
                ("unit", units::suffix(units)),
            ];
            match project.stock.thickness_mm {
                Some(thickness_mm) => {
                    let thickness = length(thickness_mm);
                    let mut args = size.to_vec();
                    args.push(("thickness", thickness.as_str()));
                    strings.fmt(StrKey::StepsSummaryStock, &args)
                }
                None => strings.fmt(StrKey::StepsSummaryStockNoThickness, &size),
            }
        }
        Step::Tools if project.tools.is_empty() => text(StrKey::StepsSummaryToolsEmpty),
        Step::Tools => counted(
            strings,
            project.tools.len(),
            StrKey::StepsSummaryTool,
            StrKey::StepsSummaryTools,
        ),
        Step::Operations if project.operations.is_empty() => {
            text(StrKey::StepsSummaryOperationsEmpty)
        }
        Step::Operations => counted(
            strings,
            project.operations.len(),
            StrKey::StepsSummaryOperation,
            StrKey::StepsSummaryOperations,
        ),
        Step::Export => {
            let errors = warnings::counts(app).errors;
            let enabled = project
                .operations
                .iter()
                .any(dc_project::Operation::enabled);
            if session.exported == Some(app.snapshot()) {
                text(StrKey::StepsSummaryExportDone)
            } else if !enabled {
                text(StrKey::StepsSummaryExportNothing)
            } else if errors > 0 {
                counted(
                    strings,
                    errors,
                    StrKey::StepsSummaryExportBlocked,
                    StrKey::StepsSummaryExportBlockedMany,
                )
            } else {
                text(StrKey::StepsSummaryExportReady)
            }
        }
    }
}
