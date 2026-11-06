// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The application state (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app"): one project history with its labels, the selection, the view,
//! the generation bookkeeping, the strings, the preferences, and the bytes
//! last saved, which decide whether the project is dirty (functional spec,
//! capability 51). Every mutation of the project is a [`Command`] through
//! [`App::run`]; the shell never edits model fields in place.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
use dc_job::{EmitOp, Finding, Findings};
use dc_project::{
    CURRENT_VERSION, Heights, InstanceId, MachineOrigin, MigrationNote, OperationId, OriginXy,
    OriginZ, Problem, Project, SettingReset, SourceMap, Stock, Units,
};

use crate::command::Command;
use crate::error::CommandError;
use crate::generation::{Generation, Job, JobResult, SnapshotId, Work};
use crate::history::LabelledHistory;
use crate::import::DrawingImport;
use crate::prefs::Prefs;
use crate::scene::Scene;
use crate::selection::Selection;
use crate::strings::{StrKey, Strings};
use crate::view::View;

/// The bundled post-processor a new project names (capability 51).
const DEFAULT_POST_ID: &str = "grblhal";
/// A new project's stock, mm, square (capability 51).
const DEFAULT_STOCK_MM: f64 = 100.0;
/// A new project's name, before it is saved.
const UNTITLED: &str = "untitled";

/// The app's state: everything but the window.
#[derive(Debug, Clone, PartialEq)]
pub struct App {
    history: LabelledHistory,
    selection: Selection,
    view: View,
    generation: Generation,
    strings: Strings,
    prefs: Prefs,
    /// Where the project was opened from or last saved to.
    source: Option<PathBuf>,
    /// The clean state, when there is one: the project as opened,
    /// serialized, or the bytes the shell last wrote. What
    /// [`App::is_dirty`] compares the current snapshot against.
    saved: Option<Vec<u8>>,
    dirty: bool,
    /// Validation of the current snapshot, run on every change.
    validation: Findings,
    /// Validation's, generation's and `finish`'s findings, merged
    /// (`warnings::merge`).
    findings: Findings,
    /// `findings`, errors then warnings, as `check` prints them.
    listed: Vec<Finding>,
    /// What opening the file (its migration) or importing into it noted,
    /// for the Warnings panel; cleared when the file is saved.
    notes: Vec<String>,
    /// Where each value is in the file's text, as opened or last saved,
    /// so a finding's line names its position as `check`'s does.
    source_map: Option<SourceMap>,
    /// The settings the last command reset, when it switched the
    /// post-processor (functional spec, capability 62).
    last_resets: Vec<SettingReset>,
    /// What the Viewport draws, rebuilt with the snapshot and the results.
    scene: Scene,
}

/// What opening a file reported (capability 51): the migration notes and
/// the loader's validation problems, for the Warnings panel.
#[derive(Debug, Clone, PartialEq)]
pub struct Opened {
    /// What the migration to the current version changed.
    pub notes: Vec<MigrationNote>,
    /// What validation found in the opened project.
    pub problems: Vec<Problem>,
}

impl App {
    /// An app on `project` with nothing to undo and no file behind it.
    #[must_use]
    pub fn new(project: Project, strings: Strings, prefs: Prefs) -> Self {
        let validation = dc_job::validation(&project);
        let generation = Generation::new(&project, !validation.has_errors());
        let mut app = Self {
            history: LabelledHistory::new(project),
            selection: Selection::Nothing,
            view: View::default(),
            generation,
            strings,
            prefs,
            source: None,
            saved: None,
            dirty: false,
            validation,
            findings: Findings::default(),
            listed: Vec::new(),
            notes: Vec::new(),
            source_map: None,
            last_resets: Vec::new(),
            scene: Scene::default(),
        };
        app.refresh_findings();
        app.rebuild_scene();
        app
    }

    /// The project *New* makes (capability 51): a 100 × 100 mm stock,
    /// thickness unset, origin bottom-left at the stock top, no heights,
    /// grblHAL at its latest version, no tools, operations or shapes.
    ///
    /// # Errors
    ///
    /// [`CommandError::PostProcessor`] if the bundled grblHAL definition
    /// cannot be read, which no release ships.
    pub fn new_project() -> Result<Project, CommandError> {
        let post_processor = dc_project::latest_definition(DEFAULT_POST_ID)?.default_ref();
        Ok(Project {
            version: CURRENT_VERSION,
            name: UNTITLED.to_owned(),
            display_units: Units::Mm,
            flatten_tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
            arc_fit_tolerance_mm: Some(dc_job::project_from::ARC_FIT_TOLERANCE_MM),
            stock: Stock {
                width_mm: DEFAULT_STOCK_MM,
                height_mm: DEFAULT_STOCK_MM,
                thickness_mm: None,
            },
            origin: MachineOrigin {
                xy: OriginXy::BottomLeft,
                z: OriginZ::StockTop,
            },
            heights: Heights::unset(),
            tools: Vec::new(),
            operations: Vec::new(),
            tabs: Vec::new(),
            instances: Vec::new(),
            post_processor,
            definitions: BTreeMap::new(),
        })
    }

    /// Opens the project in `bytes`, read from `path` when there is one:
    /// the history is cleared, the selection dropped, and the project as
    /// loaded becomes the clean state (acceptance 62). The clean state is
    /// the loaded project serialized, not the bytes read: a hand-formatted
    /// or older-version file opens clean, since nothing of the user's has
    /// changed, and saving it writes the current layout.
    ///
    /// # Errors
    ///
    /// [`CommandError::Load`] when the bytes are not a project; the app
    /// keeps the project it had.
    pub fn open(&mut self, bytes: &[u8], path: Option<PathBuf>) -> Result<Opened, CommandError> {
        let loaded = dc_project::load(bytes)?;
        let clean = dc_project::save(&loaded.project)
            .ok()
            .map(String::into_bytes);
        self.replace_project(loaded.project, path, clean);
        // `check` locates findings in the file as read and prints what the
        // migration changed first (#498): so does the Warnings panel.
        self.source_map = source_map(bytes);
        self.notes = loaded.notes.iter().map(ToString::to_string).collect();
        Ok(Opened {
            notes: loaded.notes,
            problems: loaded.problems,
        })
    }

    /// Makes `project` the whole history, as *New* does.
    pub fn reset(&mut self, project: Project) {
        self.replace_project(project, None, None);
    }

    fn replace_project(&mut self, project: Project, path: Option<PathBuf>, saved: Option<Vec<u8>>) {
        self.validation = dc_job::validation(&project);
        self.generation = Generation::after(
            self.generation.snapshot(),
            &project,
            !self.validation.has_errors(),
        );
        self.history = LabelledHistory::new(project);
        self.selection = Selection::Nothing;
        self.source = path;
        self.saved = saved;
        self.notes.clear();
        self.source_map = None;
        self.last_resets.clear();
        self.refresh_dirty();
        self.refresh_findings();
        self.rebuild_scene();
    }

    /// Imports the drawing `sheet` shows into the project, placed as
    /// drawn, as one undo step (*Import plaque.dxf*), and selects the new
    /// shapes (functional spec, capability 55). Each of the importer's
    /// warnings becomes one note in the Warnings panel ([`App::notes`]).
    ///
    /// # Errors
    ///
    /// [`CommandError::NotImportable`] with the sheet's reason when its
    /// *Import* is disabled; nothing changes.
    pub fn import_drawing(&mut self, sheet: &DrawingImport) -> Result<String, CommandError> {
        let command = sheet.command().ok_or_else(|| CommandError::NotImportable {
            reason: sheet.blocked().unwrap_or_default(),
        })?;
        let label = self.run(command)?;
        self.note_import(sheet);
        Ok(label)
    }

    /// Replaces the project with a new one round the drawing `sheet`
    /// shows, as *New from drawing…* does (functional spec, capability 51
    /// and open question 1): what `dreamcarve init` builds, named by the
    /// file's stem, on grblHAL at its latest version in millimetres, with
    /// the stock and the shapes only (no placeholder tool, no Profile).
    /// The history is cleared and the project has no file yet, as after
    /// *New*; the importer's warnings become its notes.
    ///
    /// # Errors
    ///
    /// [`CommandError::NotImportable`] when the sheet's *Import* is
    /// disabled, and [`CommandError::Job`] when `init` would refuse the
    /// drawing (a stock past the coordinate limit); nothing changes.
    pub fn new_from_drawing(&mut self, sheet: &DrawingImport) -> Result<(), CommandError> {
        let drawing = sheet.drawing().ok_or_else(|| CommandError::NotImportable {
            reason: sheet.blocked().unwrap_or_default(),
        })?;
        let post_processor = dc_project::latest_definition(DEFAULT_POST_ID)?.default_ref();
        let project = dc_job::project_from::drawing(
            sheet.stem(),
            drawing.clone(),
            post_processor,
            Units::Mm,
            false,
        )?;
        self.reset(project);
        self.note_import(sheet);
        Ok(())
    }

    /// The importer's warnings as the Warnings panel's notes
    /// ([`App::add_note`]), each under the file's name.
    fn note_import(&mut self, sheet: &DrawingImport) {
        for warning in sheet.warnings() {
            let note = self.strings.fmt(
                StrKey::AppImportNote,
                &[("name", sheet.name()), ("note", &warning.to_string())],
            );
            self.add_note(note);
        }
    }

    /// The current snapshot.
    #[must_use]
    pub fn project(&self) -> &Project {
        self.history.current()
    }

    /// The file the project came from or was last saved to.
    #[must_use]
    pub fn source(&self) -> Option<&Path> {
        self.source.as_deref()
    }

    /// The current snapshot as the file the CLI would write, for the shell
    /// to save. The app stays dirty until [`App::mark_saved`].
    ///
    /// # Errors
    ///
    /// [`CommandError::Save`] when the project cannot be serialized.
    pub fn serialize(&self) -> Result<String, CommandError> {
        Ok(dc_project::save(self.project())?)
    }

    /// Records that the shell wrote `bytes` to `path` (or to the source
    /// when `None`), so the project is clean until the next change.
    pub fn mark_saved(&mut self, bytes: &[u8], path: Option<PathBuf>) {
        if path.is_some() {
            self.source = path;
        }
        self.saved = Some(bytes.to_vec());
        // The file now holds the current version: nothing migrated.
        self.source_map = source_map(bytes);
        self.notes.clear();
        self.refresh_dirty();
    }

    /// The file name the Warnings panel's lines are prefixed with, as
    /// `check` prints them run in the file's folder: the source's name,
    /// or the project's with `.dcv` before it has a file.
    #[must_use]
    pub fn file_name(&self) -> String {
        self.source
            .as_deref()
            .and_then(Path::file_name)
            .map_or_else(
                || format!("{}.dcv", self.project().name),
                |n| n.to_string_lossy().into_owned(),
            )
    }

    /// Where each value of the file is, while the project is what the file
    /// holds; `None` once it is edited (the file's positions no longer
    /// name the values checked) or with no file.
    #[must_use]
    pub fn source_map(&self) -> Option<&SourceMap> {
        self.source_map.as_ref().filter(|_| !self.dirty)
    }

    /// What opening or importing noted, oldest first, for the Warnings
    /// panel.
    #[must_use]
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Adds a note to the Warnings panel: an importer's warning about what
    /// a drawing it read left out ([`App::import_drawing`]).
    pub fn add_note(&mut self, note: String) {
        self.notes.push(note);
    }

    /// Whether the project has unsaved changes (capability 51): the
    /// current snapshot serialized differs from the clean state (the
    /// project as opened, or the bytes last saved), or, with no file yet,
    /// the project has been edited. Undoing every change makes it clean
    /// again.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    fn refresh_dirty(&mut self) {
        self.dirty = match &self.saved {
            Some(bytes) => dc_project::save(self.project())
                .map_or(true, |text| text.as_bytes() != bytes.as_slice()),
            None => self.history.can_undo(),
        };
    }

    /// Applies `command` to the current snapshot, pushes the result under
    /// its label and returns the label. New shapes and a new operation
    /// become the selection.
    ///
    /// # Errors
    ///
    /// The command's [`CommandError`]; nothing changes.
    pub fn run(&mut self, command: Command) -> Result<String, CommandError> {
        let before = self.history.current();
        let after = command.apply(before)?;
        let label = command.label(&self.strings, before, &after);
        // `apply` keeps only the project; what the switch reset is asked
        // again, from the same pure function, for the panel to report.
        self.last_resets = match &command {
            Command::SwitchPost { id } => dc_project::switch_post(&before.post_processor, id)
                .map(|(_, resets)| resets)
                .unwrap_or_default(),
            _ => Vec::new(),
        };
        let new_instances: Vec<InstanceId> = after
            .instances
            .iter()
            .filter(|i| !before.instances.iter().any(|b| b.id == i.id))
            .map(|i| i.id.clone())
            .collect();
        let new_operations: Vec<OperationId> = after
            .operations
            .iter()
            .filter(|o| !before.operations.iter().any(|b| b.id() == o.id()))
            .map(|o| o.id().clone())
            .collect();
        self.history.push(after, label.clone());
        if !new_instances.is_empty() {
            self.selection = Selection::Instances(new_instances);
        } else if let [id] = new_operations.as_slice() {
            self.selection = Selection::Operation(id.clone());
        }
        self.snapshot_changed();
        Ok(label)
    }

    /// Steps back one snapshot; the label of the step undone.
    pub fn undo(&mut self) -> Option<String> {
        let label = self.history.undo()?;
        self.last_resets.clear();
        self.snapshot_changed();
        Some(label)
    }

    /// Steps forward one snapshot; the label of the step redone.
    pub fn redo(&mut self) -> Option<String> {
        let label = self.history.redo()?;
        self.last_resets.clear();
        self.snapshot_changed();
        Some(label)
    }

    /// The settings the last command reset because the post-processor it
    /// switched to cannot use them (functional spec, capability 62; the
    /// rule of `export --post`, [`dc_project::switch_post`]). Empty after
    /// any other command, an undo, a redo or an open.
    #[must_use]
    pub fn last_resets(&self) -> &[SettingReset] {
        &self.last_resets
    }

    /// The label undo would report, for the Edit menu.
    #[must_use]
    pub fn undo_label(&self) -> Option<&str> {
        self.history.undo_label()
    }

    /// The label redo would report, for the Edit menu.
    #[must_use]
    pub fn redo_label(&self) -> Option<&str> {
        self.history.redo_label()
    }

    /// Whether there is a step to undo.
    #[must_use]
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Whether there is a step to redo.
    #[must_use]
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// The history with its labels.
    #[must_use]
    pub fn history(&self) -> &LabelledHistory {
        &self.history
    }

    /// After the current snapshot changed: validate, re-digest, prune the
    /// selection, recompute dirty and the findings.
    fn snapshot_changed(&mut self) {
        let project = self.history.current();
        self.validation = dc_job::validation(project);
        let runnable = !self.validation.has_errors();
        self.generation.advance(project, runnable);
        self.selection.retain_existing(project);
        self.refresh_dirty();
        self.refresh_findings();
        self.rebuild_scene();
    }

    /// Builds the Scene of the current snapshot and results under the
    /// next revision.
    fn rebuild_scene(&mut self) {
        let revision = self.scene.revision().wrapping_add(1);
        self.scene = Scene::build(self.history.current(), &self.generation, revision);
    }

    /// What the Viewport draws and picks from: rebuilt when the snapshot
    /// or a generation result changes, never for view state.
    #[must_use]
    pub fn scene(&self) -> &Scene {
        &self.scene
    }

    /// The Scene's operations re-read after a result, Stop or Regenerate
    /// all, under the next revision; its shapes are kept, since the
    /// snapshot did not change.
    fn refresh_scene_operations(&mut self) {
        let revision = self.scene.revision().wrapping_add(1);
        self.scene
            .refresh_operations(self.history.current(), &self.generation, revision);
    }

    fn refresh_findings(&mut self) {
        self.findings = crate::warnings::merge(&self.validation, self.generation.findings());
        self.listed = self
            .findings
            .errors
            .iter()
            .chain(&self.findings.warnings)
            .cloned()
            .collect();
    }

    /// What is selected.
    #[must_use]
    pub fn selection(&self) -> &Selection {
        &self.selection
    }

    /// Selects `selection`, less anything the project does not have. Not
    /// an undo step.
    pub fn select(&mut self, selection: Selection) {
        self.selection = selection;
        self.selection.retain_existing(self.history.current());
    }

    /// The view settings.
    #[must_use]
    pub fn view(&self) -> &View {
        &self.view
    }

    /// The view settings, to change. Not an undo step.
    pub fn view_mut(&mut self) -> &mut View {
        &mut self.view
    }

    /// The preferences.
    #[must_use]
    pub fn prefs(&self) -> &Prefs {
        &self.prefs
    }

    /// The preferences, to change. Not an undo step; the shell writes
    /// `gui.toml`.
    pub fn prefs_mut(&mut self) -> &mut Prefs {
        &mut self.prefs
    }

    /// The string table.
    #[must_use]
    pub fn strings(&self) -> &Strings {
        &self.strings
    }

    /// The current snapshot's id.
    #[must_use]
    pub fn snapshot(&self) -> SnapshotId {
        self.generation.snapshot()
    }

    /// The generation bookkeeping: results, staleness, what is finished.
    #[must_use]
    pub fn generation(&self) -> &Generation {
        &self.generation
    }

    /// What the worker should run next (see [`Generation::pending`]).
    #[must_use]
    pub fn pending(&self) -> Vec<Job> {
        self.generation.pending()
    }

    /// [`App::pending`], each job marked in flight so it is handed out once.
    pub fn take_pending(&mut self) -> Vec<Job> {
        self.generation.take_pending()
    }

    /// Whether [`dc_job::finish`] should run on [`App::finish_input`].
    #[must_use]
    pub fn needs_finish(&self) -> bool {
        self.generation.needs_finish()
    }

    /// Every enabled operation's runs for `finish`, once all are in.
    #[must_use]
    pub fn finish_input(&self) -> Option<Vec<EmitOp>> {
        self.generation.finish_input()
    }

    /// Stores a worker's result; ignored unless its snapshot is current.
    /// Returns whether it was kept.
    pub fn accept(&mut self, result: JobResult) -> bool {
        let kept = self.generation.accept(result);
        if kept {
            self.refresh_findings();
            self.refresh_scene_operations();
        }
        kept
    }

    /// The next work for the worker, marked handed out
    /// ([`Generation::next_work`]): the shell posts one at a time, so Stop
    /// has nothing queued to take back.
    pub fn next_work(&mut self) -> Option<Work> {
        self.generation.next_work()
    }

    /// Stop (capability 71): nothing pending, the running job's result
    /// dropped when it lands, the stale operations held until an edit
    /// changes them or Regenerate all.
    pub fn stop(&mut self) {
        self.generation.stop();
        self.refresh_scene_operations();
    }

    /// Regenerate all (capability 71): every enabled operation
    /// regenerates, whatever its result.
    pub fn regenerate_all(&mut self) {
        self.generation.regenerate_all();
        self.refresh_scene_operations();
    }

    /// Validation's, generation's and `finish`'s findings for the current
    /// snapshot, in the order `check` prints them: the errors, then the
    /// warnings, each in the order `build` finds them (validation, each
    /// operation in project order, the project-level checks, the export
    /// warnings). Each carries its tier.
    #[must_use]
    pub fn warnings(&self) -> &[Finding] {
        &self.listed
    }

    /// [`App::warnings`] as errors and warnings.
    #[must_use]
    pub fn findings(&self) -> &Findings {
        &self.findings
    }

    /// Validation of the current snapshot alone.
    #[must_use]
    pub fn validation(&self) -> &Findings {
        &self.validation
    }
}

/// Where each value is in `bytes`, when they are JSON text.
fn source_map(bytes: &[u8]) -> Option<SourceMap> {
    std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| SourceMap::parse(text).ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::test_support::sample_project;

    #[test]
    fn a_new_app_is_clean_and_has_nothing_to_undo() {
        let app = App::new(sample_project(), Strings::default(), Prefs::default());
        assert!(!app.is_dirty());
        assert!(!app.can_undo());
        assert_eq!(app.selection(), &Selection::Nothing);
        assert_eq!(app.source(), None);
    }

    #[test]
    fn opening_bytes_that_are_not_a_project_keeps_the_current_one() {
        let mut app = App::new(sample_project(), Strings::default(), Prefs::default());
        let before = app.project().clone();
        assert!(app.open(b"not json", None).is_err());
        assert_eq!(app.project(), &before);
    }

    #[test]
    fn a_duplicate_becomes_the_selection() {
        let mut app = App::new(sample_project(), Strings::default(), Prefs::default());
        let first = app.project().instances.first().map(|i| i.id.clone());
        let Some(first) = first else {
            return;
        };
        app.run(Command::DuplicateInstances(vec![first.clone()]))
            .unwrap_or_default();
        let selected = app.selection().instances();
        assert_eq!(selected.len(), 1);
        assert_ne!(selected.first(), Some(&first));
    }
}
