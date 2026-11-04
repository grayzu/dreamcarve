// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The test harness (`docs/specs/phase-4/design.md`, "Test harness"): an
//! [`App`] built from fixture bytes, commands run on it, and the pending
//! jobs executed inline on the test thread with the real `dc_job`
//! functions, so every acceptance test that reads toolpaths, warnings or
//! programs runs headless under `cargo test`. Enabled by the
//! `test-support` feature for other crates' tests.

use dc_job::{EstimateConfig, Finding, ProgramSplit, ProgramSummary};
use dc_project::Project;

use crate::app::App;
use crate::command::Command;
use crate::error::CommandError;
use crate::generation::JobResult;
use crate::prefs::Prefs;
use crate::strings::Strings;

/// The date every program the harness finishes is headed with, so its
/// bytes compare against the CLI's with `--date` pinned.
pub const DATE: &str = "2026-10-15";

/// An app driven from a test.
#[derive(Debug, Clone, PartialEq)]
pub struct Harness {
    /// The app under test.
    pub app: App,
}

impl Harness {
    /// An app with the project in `bytes` open, English strings and
    /// default preferences.
    ///
    /// # Errors
    ///
    /// [`CommandError::Load`] when the bytes are not a project.
    pub fn open(bytes: &[u8]) -> Result<Self, CommandError> {
        let mut app = App::new(App::new_project()?, Strings::default(), Prefs::default());
        app.open(bytes, None)?;
        Ok(Self { app })
    }

    /// An app on `project`, as if newly made.
    #[must_use]
    pub fn from_project(project: Project) -> Self {
        Self {
            app: App::new(project, Strings::default(), Prefs::default()),
        }
    }

    /// Runs `command` ([`App::run`]).
    ///
    /// # Errors
    ///
    /// The command's.
    pub fn run(&mut self, command: Command) -> Result<String, CommandError> {
        self.app.run(command)
    }

    /// Executes every pending job inline with [`dc_job::generate_operation`],
    /// then [`dc_job::finish`] when every operation is in, with the
    /// project's split, [`DATE`] and the default estimate settings: what
    /// the worker thread does, on this thread.
    pub fn generate(&mut self) {
        loop {
            let jobs = self.app.take_pending();
            if jobs.is_empty() {
                break;
            }
            for job in jobs {
                let generated = dc_job::generate_operation(self.app.project(), &job.op);
                self.app.accept(JobResult::Operation { job, generated });
            }
        }
        if self.app.needs_finish()
            && let Some(runs) = self.app.finish_input()
        {
            let finished = dc_job::finish(
                self.app.project(),
                &runs,
                ProgramSplit::ProjectDefault,
                DATE,
                &EstimateConfig::default(),
            );
            let snapshot = self.app.snapshot();
            self.app.accept(JobResult::Finished { snapshot, finished });
        }
    }

    /// The merged findings ([`App::warnings`]).
    #[must_use]
    pub fn warnings(&self) -> &[Finding] {
        self.app.warnings()
    }

    /// The programs `finish` produced for the current snapshot, once
    /// [`Harness::generate`] has run; empty before, or when an error
    /// stopped generation.
    #[must_use]
    pub fn programs(&self) -> &[ProgramSummary] {
        self.app
            .generation()
            .finished()
            .map_or(&[], |f| f.programs.as_slice())
    }

    /// The current snapshot as the file ([`App::serialize`]).
    ///
    /// # Errors
    ///
    /// [`CommandError::Save`].
    pub fn save(&self) -> Result<String, CommandError> {
        self.app.serialize()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::test_support::sample_project;

    #[test]
    fn generate_runs_every_operation_and_finishes() {
        let mut harness = Harness::from_project(sample_project());
        assert!(harness.programs().is_empty());
        harness.generate();
        assert!(harness.app.pending().is_empty());
        if harness.app.validation().has_errors() {
            assert!(harness.programs().is_empty());
        } else {
            assert!(!harness.programs().is_empty() || !harness.warnings().is_empty());
        }
    }
}
