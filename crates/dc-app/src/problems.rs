// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What is wrong at one field (`docs/specs/phase-4/functional.md`,
//! capabilities 60, 63, 66 and 66b; `experience.md`, section 7, "Field
//! problem"): every finding validation or generation locates at the
//! field, and the reason the field refused what was typed, most severe
//! first. The window draws none of it in the layout: the field takes the
//! outline and the glyph of [`Problems::worst`], the glyph's tooltip and
//! the field's accessible description read [`Problems::text`], the help
//! line at the panel's foot shows it while the field has the keyboard,
//! and a click on the glyph opens the Warnings drawer at the row of the
//! first problem that has one ([`Problems::row`]).
//!
//! A finding's words are the drawer's ([`crate::warnings::present`], which
//! [`crate::App::warnings`] has applied): its tier's word, its code, its
//! message with names beside ids. A refused entry is not a finding of the
//! project: it has no code and no drawer row.

use dc_job::{Finding, Tier};

use crate::strings::Strings;
use crate::warnings::{self, Level, RowKey};

/// One thing wrong at a field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// How much it matters.
    pub tier: Tier,
    /// The tier's word, as the drawer's chip says it (*error*).
    pub word: String,
    /// The message, as the app shows it: names beside ids.
    pub message: String,
    /// The finding's stable code, when it has one.
    pub code: Option<String>,
    /// Its row in the Warnings drawer; `None` for what is not a finding
    /// of the project (a refused entry, a sheet's own check).
    pub row: Option<RowKey>,
}

impl Problem {
    /// The problem on one line, as the drawer names its row: `error:
    /// message`, or `warning: code: message`.
    #[must_use]
    pub fn line(&self) -> String {
        match &self.code {
            Some(code) => format!("{}: {code}: {}", self.word, self.message),
            None => format!("{}: {}", self.word, self.message),
        }
    }
}

/// Where a tier stands: errors, then warnings, then hints.
const fn rank(tier: Tier) -> u8 {
    match tier {
        Tier::Error => 0,
        Tier::Warning => 1,
        Tier::Hint => 2,
    }
}

/// Everything wrong at one field, most severe first; within a tier, in
/// the order it was found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Problems {
    list: Vec<Problem>,
}

impl Problems {
    /// Nothing wrong.
    #[must_use]
    pub const fn none() -> Self {
        Self { list: Vec::new() }
    }

    /// `findings` ([`crate::App::warnings`]'s, already worded for the
    /// app), each with its drawer row.
    #[must_use]
    pub fn found<'a>(strings: &Strings, findings: impl IntoIterator<Item = &'a Finding>) -> Self {
        let mut problems = Self::none();
        for finding in findings {
            problems.push(Problem {
                tier: finding.tier,
                word: strings
                    .get(warnings::level_word(Level::from(finding.tier)))
                    .to_owned(),
                message: finding.message.clone(),
                code: finding.code.clone(),
                row: Some(warnings::key_of(strings, finding)),
            });
        }
        problems
    }

    /// One problem that is not a finding of the project, so has no code
    /// and no drawer row: a sheet's own check, a write that failed.
    #[must_use]
    pub fn said(strings: &Strings, tier: Tier, message: &str) -> Self {
        Self {
            list: vec![Problem {
                tier,
                word: strings
                    .get(warnings::level_word(Level::from(tier)))
                    .to_owned(),
                message: message.to_owned(),
                code: None,
                row: None,
            }],
        }
    }

    /// The field refused what was typed: `reason`, which says why and
    /// what it takes. An error with no drawer row.
    #[must_use]
    pub fn refused(strings: &Strings, reason: &str) -> Self {
        Self::said(strings, Tier::Error, reason)
    }

    /// These and `more`, most severe first, these before `more`'s within
    /// a tier.
    #[must_use]
    pub fn with(mut self, more: Self) -> Self {
        for problem in more.list {
            self.push(problem);
        }
        self
    }

    /// Adds `problem` after the last of its tier or a worse one.
    fn push(&mut self, problem: Problem) {
        let at = self
            .list
            .iter()
            .position(|p| rank(p.tier) > rank(problem.tier))
            .unwrap_or(self.list.len());
        self.list.insert(at, problem);
    }

    /// Whether nothing is wrong.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    /// How many problems.
    #[must_use]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    /// The problems, most severe first.
    pub fn iter(&self) -> impl Iterator<Item = &Problem> {
        self.list.iter()
    }

    /// The most severe tier: the field's outline and glyph.
    #[must_use]
    pub fn worst(&self) -> Option<Tier> {
        self.list.first().map(|p| p.tier)
    }

    /// The drawer row a click on the glyph opens: the most severe
    /// problem's that has one. `None` when none is a finding (a refused
    /// entry alone).
    #[must_use]
    pub fn row(&self) -> Option<&RowKey> {
        self.list.iter().find_map(|p| p.row.as_ref())
    }

    /// Every problem, a line each ([`Problem::line`]), most severe first:
    /// the field's accessible description, and what the tooltip says.
    #[must_use]
    pub fn text(&self) -> String {
        self.list
            .iter()
            .map(Problem::line)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(tier: Tier, code: Option<&str>, message: &str, field: &str) -> Finding {
        Finding {
            tier,
            code: code.map(str::to_owned),
            message: message.to_owned(),
            field: Some(field.to_owned()),
            op: None,
            shape: None,
        }
    }

    /// Capability 66: where a field has more than one finding the most
    /// severe sets its colour and glyph, and the tooltip lists them most
    /// severe first, each tier in the order `check` gives.
    #[test]
    fn most_severe_first_whatever_the_order_found() {
        let strings = Strings::english();
        let findings = [
            finding(Tier::Hint, Some("unused_tool"), "a hint", "tools[t1]"),
            finding(Tier::Warning, Some("plunge_above_feed"), "first", "f"),
            finding(Tier::Error, None, "an error", "f"),
            finding(Tier::Warning, Some("step_over_too_fine"), "second", "f"),
        ];
        let problems = Problems::found(&strings, &findings);
        assert_eq!(problems.worst(), Some(Tier::Error));
        assert_eq!(
            problems.text(),
            "error: an error\n\
             warning: plunge_above_feed: first\n\
             warning: step_over_too_fine: second\n\
             hint: unused_tool: a hint"
        );
    }

    /// A refused entry is the field's own: an error with no code, which
    /// stands before a finding's warning and has no drawer row, so the
    /// glyph's click goes to the finding's.
    #[test]
    fn refused_entry_has_no_drawer_row() {
        let strings = Strings::english();
        let refused = Problems::refused(&strings, "`abc` is not a length");
        assert_eq!(refused.row(), None);
        assert_eq!(refused.text(), "error: `abc` is not a length");
        let warning = finding(Tier::Warning, Some("plunge_above_feed"), "too fast", "f");
        let both = Problems::found(&strings, [&warning]).with(refused);
        assert_eq!(both.worst(), Some(Tier::Error));
        assert_eq!(
            both.row(),
            Some(&warnings::key_of(&strings, &warning)),
            "the finding's row"
        );
        assert_eq!(Problems::none().worst(), None);
        assert!(Problems::none().text().is_empty());
    }
}
