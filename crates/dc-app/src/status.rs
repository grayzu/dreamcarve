// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The status bar's cells as data (functional spec, capability 50b;
//! `experience.md`, section 2, "Status bar"; slice 24, #616). Left to
//! right: the units button, the cursor, the *Toolpaths* button, the
//! generation state, the counts, the estimate, the Z range, then what the
//! pointer and the app have to say. A cell with nothing to say is `None`
//! (or empty, for the cursor's fixed cell), and the shell draws neither
//! it nor its rule. The stock size is not a cell: Project Settings shows
//! it.

use dc_geometry::Point;
use dc_job::EstimateConfig;
use dc_project::{OperationId, Units};

use crate::app::App;
use crate::selection::Selection;
use crate::strings::{StrKey, Strings};
use crate::warnings::{self, Counts, Level};

/// What the status bar reads from outside the app: the pointer over the
/// Viewport and the window's last message.
#[derive(Debug, Clone, Copy)]
pub struct Inputs<'a> {
    /// The pointer's position in project space, mm, while it is over the
    /// Viewport.
    pub cursor_mm: Option<Point>,
    /// A drag's travel, scale or angle, as the Viewport words it.
    pub drag: Option<&'a str>,
    /// The shape under the pointer, by name, and whether it is locked.
    pub hover: Option<(&'a str, bool)>,
    /// The last status message.
    pub message: Option<&'a str>,
    /// The user's estimate settings, for the total time (capability 78).
    pub estimate: &'a EstimateConfig,
}

/// What the generation cell's button does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationAction {
    /// Stop generating (capability 71).
    Stop,
    /// Force every operation to generate again: offered after Stop and
    /// nowhere else.
    RegenerateAll,
}

/// The generation cell's button and its label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationButton {
    /// What it does.
    pub action: GenerationAction,
    /// Its label.
    pub label: String,
}

/// The generation cell, shown while anything is pending and after Stop.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GenerationCell {
    /// *Generating 2 of 5…*, *Checking…* or *Stopped*.
    pub text: String,
    /// Whether a spinner goes before the text.
    pub spinner: bool,
    /// The button after the text.
    pub action: Option<GenerationButton>,
}

/// The counts cell: one button that opens the Warnings drawer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CountsCell {
    /// The counts behind it.
    pub counts: Counts,
    /// Each tier with rows, its glyph's tier and its count
    /// (`1 error`, `3 warnings`, `2 hints`), errors first.
    pub tiers: Vec<(Level, String)>,
    /// *No problems*, when every tier is empty.
    pub none: Option<String>,
}

/// The status bar, cell by cell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusBar {
    /// The project's display units, which the units button's menu
    /// switches.
    pub units: Units,
    /// The units button's label: `mm` or `in`.
    pub units_label: String,
    /// The cursor's position from the origin (`X 64.2  Y 51.8`); empty
    /// while the pointer is off the Viewport.
    pub cursor: String,
    /// Whether the *Toolpaths* button shows: once the project has a
    /// toolpath.
    pub toolpaths: bool,
    /// The generation cell; `None` when every toolpath is current.
    pub generation: Option<GenerationCell>,
    /// The counts.
    pub counts: CountsCell,
    /// The programs' estimated time (capability 78).
    pub estimate: Option<String>,
    /// The program's Z range while the extents are shown (capability 74).
    pub z_range: Option<String>,
    /// A drag in progress.
    pub drag: Option<String>,
    /// The shape under the pointer.
    pub hover: Option<String>,
    /// The last status message, or, while an operation that carves is
    /// selected and nothing newer has been said, its carve summary
    /// (capability 76a): `Lettering · deepest cut 1.658 mm, no flat
    /// depth`.
    pub message: Option<String>,
    /// The operation whose carve summary [`Self::message`] is: clicking
    /// the cell opens the Warnings drawer at its note.
    pub message_op: Option<OperationId>,
}

impl StatusBar {
    /// Every text the bar shows, cell by cell.
    #[must_use]
    pub fn texts(&self) -> Vec<&str> {
        let mut out = vec![self.units_label.as_str(), self.cursor.as_str()];
        if let Some(generation) = &self.generation {
            out.push(&generation.text);
            out.extend(generation.action.as_ref().map(|a| a.label.as_str()));
        }
        out.extend(self.counts.tiers.iter().map(|(_, t)| t.as_str()));
        out.extend(self.counts.none.as_deref());
        for cell in [
            &self.estimate,
            &self.z_range,
            &self.drag,
            &self.hover,
            &self.message,
        ] {
            out.extend(cell.as_deref());
        }
        out
    }
}

/// `n` of a thing as the status bar counts it: `1 warning`, `3 warnings`.
#[must_use]
pub fn counted(strings: &Strings, n: usize, one: StrKey, many: StrKey) -> String {
    if n == 1 {
        strings.get(one).to_owned()
    } else {
        strings.fmt(many, &[("count", &n.to_string())])
    }
}

/// The status bar of `app` as it stands, with `inputs`.
#[must_use]
pub fn cells(app: &App, inputs: &Inputs<'_>) -> StatusBar {
    let strings = app.strings();
    let units = app.project().display_units;
    let units_label = strings
        .get(match units {
            Units::Mm => StrKey::AppStatusMm,
            Units::Inch => StrKey::AppStatusInch,
        })
        .to_owned();
    let (message, message_op) = message(app, inputs.message);
    StatusBar {
        units,
        units_label,
        cursor: inputs
            .cursor_mm
            .map(|p| cursor(strings, p, units))
            .unwrap_or_default(),
        toolpaths: app
            .scene()
            .operations()
            .iter()
            .any(|op| !op.runs.is_empty()),
        generation: generation(app),
        counts: counts(app),
        estimate: crate::export::total_time(app, inputs.estimate)
            .map(|time| strings.fmt(StrKey::ExportStatusTotal, &[("time", &time)])),
        z_range: z_range(app),
        drag: inputs.drag.map(str::to_owned),
        hover: inputs.hover.map(|(name, locked)| {
            if locked {
                strings.fmt(StrKey::AppViewportHoverLocked, &[("name", name)])
            } else {
                name.to_owned()
            }
        }),
        message,
        message_op,
    }
}

/// The message cell: the last message, else the selected operation's
/// carve summary (capability 76a) and that operation. The shell forgets
/// its message when the selection changes, so a message is newer than
/// the selection while it is held.
fn message(app: &App, said: Option<&str>) -> (Option<String>, Option<OperationId>) {
    if let Some(said) = said {
        return (Some(said.to_owned()), None);
    }
    let Some(op) = selected_operation(app) else {
        return (None, None);
    };
    match warnings::carve_message(app, op) {
        Some(text) => (Some(text), Some(op.clone())),
        None => (None, None),
    }
}

/// The operation the status bar speaks for: the selection's, else the
/// one the Operations list has open in the editor.
#[must_use]
pub fn selected_operation(app: &App) -> Option<&OperationId> {
    match app.selection() {
        Selection::Operation(op) => Some(op),
        _ => app.editing(),
    }
}

/// `p` as the cursor cell writes it (`experience.md`, section 9): one
/// decimal in mm and three in inches, from the origin, never `-0.0`.
fn cursor(strings: &Strings, p: Point, units: Units) -> String {
    let decimals = match units {
        Units::Mm => 1,
        Units::Inch => 3,
    };
    let coordinate = |mm: f64| {
        let text = format!("{:.decimals$}", crate::units::to_display(mm, units));
        match text.strip_prefix('-') {
            Some(digits) if digits.chars().all(|c| c == '0' || c == '.') => digits.to_owned(),
            _ => text,
        }
    };
    strings.fmt(
        StrKey::AppViewportCursor,
        &[("x", &coordinate(p.x)), ("y", &coordinate(p.y))],
    )
}

/// The generation cell (capability 71; `experience.md`, section 8,
/// "Progress"): *Generating k of n…* with a spinner and Stop while an
/// operation is pending, *Checking…* while only the project-level pass
/// runs, *Stopped* and *Regenerate all* once Stop held anything back, and
/// nothing when every toolpath is current.
fn generation(app: &App) -> Option<GenerationCell> {
    let strings = app.strings();
    let generation = app.generation();
    let button = |action, key| {
        Some(GenerationButton {
            action,
            label: strings.get(key).to_owned(),
        })
    };
    let running = match generation.progress() {
        Some(progress) => Some(strings.fmt(
            StrKey::AppGenerationGenerating,
            &[
                ("at", &progress.at.to_string()),
                ("of", &progress.of.to_string()),
            ],
        )),
        None if generation.busy() => Some(strings.get(StrKey::AppGenerationChecking).to_owned()),
        None => None,
    };
    match running {
        Some(text) => Some(GenerationCell {
            text,
            spinner: true,
            action: button(GenerationAction::Stop, StrKey::AppGenerationStop),
        }),
        None if generation.stopped() => Some(GenerationCell {
            text: strings.get(StrKey::AppGenerationStopped).to_owned(),
            spinner: false,
            action: button(
                GenerationAction::RegenerateAll,
                StrKey::AppGenerationRegenerateAll,
            ),
        }),
        None => None,
    }
}

/// The counts cell: each tier with rows, or *No problems*.
fn counts(app: &App) -> CountsCell {
    let strings = app.strings();
    let counts = warnings::counts(app);
    let tiers: Vec<(Level, String)> = [
        (
            Level::Error,
            counts.errors,
            StrKey::AppStatusError,
            StrKey::AppStatusErrors,
        ),
        (
            Level::Warning,
            counts.warnings,
            StrKey::AppStatusWarning,
            StrKey::AppStatusWarnings,
        ),
        (
            Level::Hint,
            counts.hints,
            StrKey::AppWarningsHint,
            StrKey::AppWarningsHints,
        ),
    ]
    .into_iter()
    .filter(|(_, n, _, _)| *n > 0)
    .map(|(level, n, one, many)| (level, counted(strings, n, one, many)))
    .collect();
    let none = tiers
        .is_empty()
        .then(|| strings.get(StrKey::AppStatusNoProblems).to_owned());
    CountsCell {
        counts,
        tiers,
        none,
    }
}

/// The program's Z range while the extents are shown (capability 74;
/// `experience.md`, section 9): the tool centre's, in machine coordinates
/// as `check` prints it.
fn z_range(app: &App) -> Option<String> {
    if !app.view().visibility.extents {
        return None;
    }
    let finished = app.generation().finished()?;
    let extents = crate::view::program_extents(app.project(), finished)?;
    let z = crate::view::range_label(extents.z?, extents.units);
    Some(app.strings().fmt(StrKey::AppToolpathsZRange, &[("z", &z)]))
}
