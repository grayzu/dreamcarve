// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The golden parity harness. Not written yet: every case fails.

use dc_project::Project;

/// One golden case: the captured directory under `tests/golden/` and the
/// equivalent DreamCarve project.
pub struct Case {
    /// Directory name under `tests/golden/`.
    pub name: &'static str,
    /// The DreamCarve project equivalent to the case's `.camj`.
    pub project: Project,
    /// Intentional differences from PureCutCNC that the case exercises.
    pub differences: Vec<Difference>,
}

/// An intentional difference from PureCutCNC that the harness checks for
/// instead of comparing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Difference {
    /// Rect tab steps are vertical moves at the plunge rate in DreamCarve
    /// and at the cutting feed in PureCutCNC.
    RectTabStepsAtPlungeRate,
}

/// Compares DreamCarve's program for `case` with the golden program.
pub fn check(case: &Case) {
    let _ = (&case.project, &case.differences);
    panic!("golden harness not written yet: {}", case.name);
}
