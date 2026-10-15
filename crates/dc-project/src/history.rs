// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/projectStore.ts
//   src/store/slices/historySlice.ts

//! Snapshot undo/redo for a [`Project`].
//!
//! Every edit produces a whole new [`Project`] that is pushed onto a
//! [`History`]; undo and redo move between those snapshots. History lives in
//! memory only: it is not serialized and is not part of a `.dcv` file.

use crate::Project;

/// The most undo steps a [`History`] keeps. Pushing past this drops the
/// oldest snapshot, matching PureCutCNC's 100-entry undo stack.
pub const MAX_UNDO_STEPS: usize = 100;

/// An in-memory undo/redo stack of whole-[`Project`] snapshots.
///
/// Holds the current project, up to [`MAX_UNDO_STEPS`] earlier snapshots to
/// undo to, and the snapshots undone since the last [`push`](Self::push) to
/// redo to. Pushing a new snapshot discards the redo snapshots. History is
/// not persisted: it has no serde support and starts empty on every load.
#[derive(Debug, Clone, PartialEq)]
pub struct History {
    /// Earlier snapshots, oldest first.
    past: Vec<Project>,
    /// The snapshot the user is looking at.
    current: Project,
    /// Undone snapshots, most recently undone last.
    future: Vec<Project>,
}

impl History {
    /// Starts a history whose only snapshot is `initial`.
    pub fn new(initial: Project) -> Self {
        let _ = initial;
        todo!()
    }

    /// The current snapshot.
    pub fn current(&self) -> &Project {
        todo!()
    }

    /// Makes `next` the current snapshot. The previous current snapshot
    /// becomes undoable, every redo snapshot is discarded, and the oldest
    /// snapshot is dropped once more than [`MAX_UNDO_STEPS`] are kept.
    pub fn push(&mut self, next: Project) {
        let _ = next;
        todo!()
    }

    /// Steps back one snapshot and returns the new current snapshot, or
    /// `None` (leaving the history unchanged) when there is nothing to undo.
    pub fn undo(&mut self) -> Option<&Project> {
        todo!()
    }

    /// Steps forward one undone snapshot and returns the new current
    /// snapshot, or `None` (leaving the history unchanged) when there is
    /// nothing to redo.
    pub fn redo(&mut self) -> Option<&Project> {
        todo!()
    }

    /// Whether [`undo`](Self::undo) would change the current snapshot.
    pub fn can_undo(&self) -> bool {
        todo!()
    }

    /// Whether [`redo`](Self::redo) would change the current snapshot.
    pub fn can_redo(&self) -> bool {
        todo!()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::sample_project;

    fn named(name: &str) -> Project {
        Project {
            name: name.into(),
            ..sample_project()
        }
    }

    fn current_name(history: &History) -> &str {
        &history.current().name
    }

    #[test]
    fn undo_restores_previous_snapshot() {
        let mut history = History::new(named("A"));
        history.push(named("B"));
        assert_eq!(history.undo().map(|p| p.name.as_str()), Some("A"));
        assert_eq!(current_name(&history), "A");
    }

    #[test]
    fn redo_after_undo() {
        let mut history = History::new(named("A"));
        history.push(named("B"));
        history.undo();
        assert!(history.can_redo());
        assert_eq!(history.redo().map(|p| p.name.as_str()), Some("B"));
        assert_eq!(current_name(&history), "B");
    }

    #[test]
    fn new_edit_truncates_redo() {
        let mut history = History::new(named("A"));
        history.push(named("B"));
        history.undo();
        history.push(named("C"));
        assert!(!history.can_redo());
        assert_eq!(history.redo(), None);
        assert_eq!(current_name(&history), "C");
        assert_eq!(history.undo().map(|p| p.name.as_str()), Some("A"));
    }

    #[test]
    fn undo_at_start_returns_none() {
        let mut history = History::new(named("A"));
        assert!(!history.can_undo());
        assert_eq!(history.undo(), None);
        assert_eq!(current_name(&history), "A");
    }

    #[test]
    fn redo_at_end_returns_none() {
        let mut history = History::new(named("A"));
        history.push(named("B"));
        assert!(!history.can_redo());
        assert_eq!(history.redo(), None);
        assert_eq!(current_name(&history), "B");
    }

    #[test]
    fn undo_depth_is_capped() {
        let mut history = History::new(named("0"));
        let pushes = MAX_UNDO_STEPS + 5;
        for i in 1..=pushes {
            history.push(named(&i.to_string()));
        }
        let mut undos = 0;
        while history.undo().is_some() {
            undos += 1;
        }
        assert_eq!(undos, MAX_UNDO_STEPS);
        let oldest_kept = (pushes - MAX_UNDO_STEPS).to_string();
        assert_eq!(current_name(&history), oldest_kept);
    }
}
