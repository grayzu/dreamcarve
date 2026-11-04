// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Undo labels beside [`dc_project::History`] (`docs/specs/phase-4/design.md`,
//! "Data model → dc-app"; functional spec, capability 52): one label per
//! past snapshot, so the Edit menu can say *Undo Move “Oval”*, and one per
//! future snapshot for *Redo*. The snapshots themselves, and the cap of
//! [`dc_project::MAX_UNDO_STEPS`], are `History`'s; the labels follow it
//! step for step.

use std::collections::VecDeque;

use dc_project::{History, MAX_UNDO_STEPS, Project};

/// Snapshot history with the name of each step.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelledHistory {
    history: History,
    /// One per past snapshot, oldest first: the label of the command that
    /// moved on from it.
    undo_labels: VecDeque<String>,
    /// One per future snapshot, the next redo last.
    redo_labels: Vec<String>,
}

impl LabelledHistory {
    /// A history whose only snapshot is `initial`: nothing to undo.
    #[must_use]
    pub fn new(initial: Project) -> Self {
        Self {
            history: History::new(initial),
            undo_labels: VecDeque::new(),
            redo_labels: Vec::new(),
        }
    }

    /// The current snapshot.
    #[must_use]
    pub fn current(&self) -> &Project {
        self.history.current()
    }

    /// Makes `next` current under `label`; the previous snapshot becomes
    /// undoable and the redo stack is cleared.
    pub fn push(&mut self, next: Project, label: String) {
        self.history.push(next);
        self.undo_labels.push_back(label);
        while self.undo_labels.len() > MAX_UNDO_STEPS {
            self.undo_labels.pop_front();
        }
        self.redo_labels.clear();
    }

    /// Steps back one snapshot; the label of the step undone.
    pub fn undo(&mut self) -> Option<String> {
        self.history.undo()?;
        let label = self.undo_labels.pop_back().unwrap_or_default();
        self.redo_labels.push(label.clone());
        Some(label)
    }

    /// Steps forward one snapshot; the label of the step redone.
    pub fn redo(&mut self) -> Option<String> {
        self.history.redo()?;
        let label = self.redo_labels.pop().unwrap_or_default();
        self.undo_labels.push_back(label.clone());
        Some(label)
    }

    /// The label undo would report, for the Edit menu.
    #[must_use]
    pub fn undo_label(&self) -> Option<&str> {
        self.undo_labels.back().map(String::as_str)
    }

    /// The label redo would report, for the Edit menu.
    #[must_use]
    pub fn redo_label(&self) -> Option<&str> {
        self.redo_labels.last().map(String::as_str)
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

    /// Every undo label, oldest first.
    pub fn undo_labels(&self) -> impl Iterator<Item = &str> {
        self.undo_labels.iter().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::test_support::sample_project;

    fn named(name: &str) -> Project {
        Project {
            name: name.into(),
            ..sample_project()
        }
    }

    #[test]
    fn labels_follow_the_snapshots() {
        let mut history = LabelledHistory::new(named("A"));
        history.push(named("B"), "to B".into());
        history.push(named("C"), "to C".into());
        assert_eq!(history.undo_label(), Some("to C"));
        assert_eq!(history.undo(), Some("to C".into()));
        assert_eq!(history.current().name, "B");
        assert_eq!(history.redo_label(), Some("to C"));
        assert_eq!(history.undo(), Some("to B".into()));
        assert_eq!(history.undo(), None);
        assert_eq!(history.redo(), Some("to B".into()));
        assert_eq!(history.redo(), Some("to C".into()));
        assert_eq!(history.redo(), None);
        assert_eq!(history.current().name, "C");
    }

    #[test]
    fn labels_are_capped_with_the_snapshots() {
        let mut history = LabelledHistory::new(named("0"));
        for k in 1..=(MAX_UNDO_STEPS + 5) {
            history.push(named(&k.to_string()), format!("to {k}"));
        }
        let mut undone = 0;
        while let Some(label) = history.undo() {
            undone += 1;
            assert_eq!(label, format!("to {}", MAX_UNDO_STEPS + 6 - undone));
        }
        assert_eq!(undone, MAX_UNDO_STEPS);
        assert_eq!(history.undo_labels().count(), 0);
    }
}
