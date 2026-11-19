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

use dc_job::Finding;

use crate::app::App;
use crate::generation::SnapshotId;
use crate::selection::Selection;
use crate::strings::StrKey;

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
            Self::Design => StrKey::StepsDesign,
            Self::Stock => StrKey::StepsStock,
            Self::Tools => StrKey::StepsTools,
            Self::Operations => StrKey::StepsOperations,
            Self::Export => StrKey::StepsExport,
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

/// What the steps read that the project does not hold: this session's
/// import warnings and its last export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Session {
    /// How many of an importer's warnings about a drawing are still noted
    /// in the Warnings panel.
    pub import_warnings: usize,
    /// The snapshot the last successful export wrote, when there was one.
    pub exported: Option<SnapshotId>,
}

/// The step that edits what `finding` is about.
#[must_use]
pub fn step_of(_finding: &Finding) -> Step {
    Step::Design
}

/// Whether a visible shape reaches past the stock.
#[must_use]
pub fn off_stock(_app: &App) -> bool {
    false
}

/// Every step's state for `app`'s current snapshot in `session`.
#[must_use]
pub fn states(_app: &App, _session: &Session) -> States {
    States::new([State::Empty; 5])
}

/// The states to show: `now`, with the marks of `shown` held while
/// toolpaths regenerate.
#[must_use]
pub fn settle(_shown: &States, now: States, _generating: bool) -> States {
    now
}

/// The step *Continue* goes to from `current`.
#[must_use]
pub fn next(_states: &States, current: Step) -> Step {
    current
}

/// The step a newly opened project starts on.
#[must_use]
pub fn opening(_states: &States) -> Step {
    Step::Design
}

/// The step that edits `selection`.
#[must_use]
pub fn owner(_selection: &Selection) -> Option<Step> {
    None
}

/// The one line under `step`'s title.
#[must_use]
pub fn summary(_app: &App, _session: &Session, _step: Step) -> String {
    String::new()
}
