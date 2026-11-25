// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The stock kept after runs (`docs/specs/phase-6/design.md`, "Runs in
//! order, and what is reused"): what a build starts from after a change
//! and what a seek backwards restores.
//!
//! A checkpoint is the stock after some number of runs, shared by
//! reference. They are kept within a byte budget: past it every other
//! one is dropped, then again, the first and the last always kept. A
//! caller that needs a dropped one takes the nearest before it and cuts
//! forward.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::field::Heightfield;

/// The bytes of stock a set of checkpoints keeps unless told otherwise:
/// 256 MiB.
pub const CHECKPOINT_BYTES: usize = 256 << 20;

/// The stock after runs, by how many runs have cut it.
#[derive(Debug, Clone, PartialEq)]
pub struct Checkpoints {
    budget_bytes: usize,
    kept: BTreeMap<usize, Arc<Heightfield>>,
}

impl Default for Checkpoints {
    fn default() -> Self {
        Self::within(CHECKPOINT_BYTES)
    }
}

impl Checkpoints {
    /// No checkpoints yet, to be kept within `budget_bytes`.
    #[must_use]
    pub const fn within(budget_bytes: usize) -> Self {
        Self {
            budget_bytes,
            kept: BTreeMap::new(),
        }
    }

    /// Keeps `field` as the stock after `runs_cut` runs, then drops
    /// others if the budget is passed.
    pub fn keep(&mut self, runs_cut: usize, field: &Heightfield) {
        self.keep_shared(runs_cut, Arc::new(field.clone()));
    }

    /// As [`Checkpoints::keep`], for stock that is shared already.
    pub fn keep_shared(&mut self, runs_cut: usize, field: Arc<Heightfield>) {
        self.kept.insert(runs_cut, field);
        while self.bytes() > self.budget_bytes && self.kept.len() > 2 {
            let last = self.kept.len() - 1;
            let dropped: Vec<usize> = self
                .kept
                .keys()
                .enumerate()
                .filter(|&(place, _)| place % 2 == 1 && place != last)
                .map(|(_, &runs)| runs)
                .collect();
            if dropped.is_empty() {
                break;
            }
            for runs in dropped {
                self.kept.remove(&runs);
            }
        }
    }

    /// Forgets every checkpoint after `runs_cut` runs: what a change to
    /// a later run makes untrue.
    pub fn truncate(&mut self, runs_cut: usize) {
        self.kept.retain(|&runs, _| runs <= runs_cut);
    }

    /// The stock after exactly `runs_cut` runs, if it is kept.
    #[must_use]
    pub fn get(&self, runs_cut: usize) -> Option<&Arc<Heightfield>> {
        self.kept.get(&runs_cut)
    }

    /// The latest kept stock at or before `runs_cut` runs, with how many
    /// runs have cut it.
    #[must_use]
    pub fn at_or_before(&self, runs_cut: usize) -> Option<(usize, &Arc<Heightfield>)> {
        self.kept
            .range(..=runs_cut)
            .next_back()
            .map(|(&runs, field)| (runs, field))
    }

    /// How many are kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.kept.len()
    }

    /// Whether none is kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.kept.is_empty()
    }

    /// The bytes of height and owner they hold.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.kept.values().map(|f| f.bytes()).sum()
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "test code reports failure by panicking"
    )]

    use super::*;
    use crate::grid::{CellSpec, Grid};

    fn stock() -> Heightfield {
        let grid = Grid::of(10.0, 10.0, CellSpec::CellMm(1.0)).unwrap();
        Heightfield::uncut(grid, 5.0).unwrap()
    }

    #[test]
    fn the_latest_at_or_before_is_found() {
        let mut kept = Checkpoints::default();
        assert!(kept.is_empty() && kept.at_or_before(3).is_none());
        for runs in [0, 2, 5] {
            kept.keep(runs, &stock());
        }
        assert_eq!(kept.len(), 3);
        assert_eq!(kept.at_or_before(0).map(|c| c.0), Some(0));
        assert_eq!(kept.at_or_before(4).map(|c| c.0), Some(2));
        assert_eq!(kept.at_or_before(9).map(|c| c.0), Some(5));
        assert!(kept.get(2).is_some() && kept.get(3).is_none());
        kept.truncate(2);
        assert_eq!(kept.at_or_before(9).map(|c| c.0), Some(2));
    }

    /// Past the budget every other checkpoint goes, the first and the
    /// last staying, until the rest fit or two are left.
    #[test]
    fn dropping_keeps_the_first_and_the_last() {
        let one = stock().bytes();
        assert_eq!(one, 100 * (8 + 2));
        let mut kept = Checkpoints::within(one * 4);
        for runs in 0..4 {
            kept.keep(runs, &stock());
        }
        assert_eq!(kept.len(), 4, "four fit");
        kept.keep(4, &stock());
        // Five do not: places 1 and 3 go, 0, 2 and 4 stay.
        let runs: Vec<usize> = (0..=4).filter(|r| kept.get(*r).is_some()).collect();
        assert_eq!(runs, [0, 2, 4]);
        assert!(kept.bytes() <= one * 4);
        // A budget too small for anything still keeps the two ends.
        let mut tight = Checkpoints::within(1);
        for runs in 0..9 {
            tight.keep(runs, &stock());
        }
        assert_eq!(tight.len(), 2);
        assert!(tight.get(0).is_some() && tight.get(8).is_some());
    }
}
