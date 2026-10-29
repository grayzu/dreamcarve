// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tool runs (`docs/specs/phase-3/design.md`, "Data model → dc-cam"): the
//! toolpaths of one operation, one per tool. A carve with clearing passes
//! cuts with an end mill per pass before its V-bit (#406); every other
//! operation, and a carve without them, is one run labelled
//! [`RunLabel::Main`].

use crate::toolpath::Toolpath;
use dc_project::ToolId;

/// One tool's toolpath of an operation, project space (mm, Y-up).
#[derive(Debug, Clone, PartialEq)]
pub struct ToolRun {
    /// Which run of the operation this is.
    pub label: RunLabel,
    /// The tool that cuts it.
    pub tool: ToolId,
    /// Its moves.
    pub toolpath: Toolpath,
}

/// What a [`ToolRun`] cuts for its operation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunLabel {
    /// The operation's own cut, with the operation's tool.
    Main,
    /// A clearing pass of a carve, with that pass's end mill.
    Clearing(ToolId),
}

impl RunLabel {
    /// How programs, summaries and the preview name the run of the
    /// operation called `name` (an id or a name): `name` itself for the
    /// operation's own run, `op2 (clearing t1)` for a clearing pass with
    /// tool `t1` (`docs/specs/phase-3/design.md`, "Clearing passes", step
    /// 4).
    #[must_use]
    pub fn naming(&self, name: &str) -> String {
        match self {
            Self::Main => name.to_owned(),
            Self::Clearing(tool) => format!("{name} (clearing {tool})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_clearing_run_is_named_after_its_tool() {
        assert_eq!(RunLabel::Main.naming("op2"), "op2");
        assert_eq!(
            RunLabel::Clearing(ToolId::new("t1")).naming("op2"),
            "op2 (clearing t1)"
        );
    }
}
