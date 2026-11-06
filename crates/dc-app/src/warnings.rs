// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Warnings panel's rows (#538). TODO(#538): stubs until the slice's
//! implementation.

use dc_job::ShapeRef;
use dc_project::OperationId;

use crate::app::App;
use crate::selection::Selection;

/// A row's tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// An error.
    Error,
    /// A warning.
    Warning,
    /// A hint.
    Hint,
    /// A note.
    Note,
}

/// One row.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    /// Its tier.
    pub level: Level,
    /// Its code.
    pub code: Option<String>,
    /// Its message.
    pub message: String,
    /// The field it is about.
    pub field: Option<String>,
    /// The operation it is about.
    pub op: Option<OperationId>,
    /// The shape it is about.
    pub shape: Option<ShapeRef>,
    /// The line `check` prints.
    pub line: String,
}

impl Row {
    /// What clicking the row selects.
    #[must_use]
    pub fn selection(&self) -> Option<Selection> {
        None
    }
}

/// One group of rows.
#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    /// The operation, or `None` for the project.
    pub op: Option<OperationId>,
    /// Its rows.
    pub rows: Vec<Row>,
}

/// The counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Counts {
    /// Errors.
    pub errors: usize,
    /// Warnings.
    pub warnings: usize,
    /// Hints.
    pub hints: usize,
    /// Notes.
    pub notes: usize,
}

/// The panel's groups.
#[must_use]
pub fn panel(_app: &App) -> Vec<Group> {
    Vec::new()
}

/// The lines `check` prints.
#[must_use]
pub fn lines(_app: &App) -> Vec<String> {
    Vec::new()
}

/// The text *Copy* puts on the clipboard.
#[must_use]
pub fn copy_text(_app: &App) -> String {
    String::new()
}

/// The counts.
#[must_use]
pub fn counts(_app: &App) -> Counts {
    Counts::default()
}
