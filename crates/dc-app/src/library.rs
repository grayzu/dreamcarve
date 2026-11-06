// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/cam/toolLibraryDialogModel.ts

//! The Tool Library window's model (`docs/specs/phase-4/design.md`,
//! "PureCutCNC sources", the `dc-app::library::filter` row; functional
//! spec, capability 64): search and the type filter over the library,
//! the "in project" marking, the ticked rows and what *Add to project*
//! copies, and the import with its report. Pure, as the upstream module
//! is: the shell reads the library file through `dreamcarve-shell` and
//! keeps the search text and the ticks between frames.
//!
//! Changed from upstream:
//!
//! - The library is millimetres only, so there is no units filter. The
//!   diameter a search matches is the one the table shows, in the
//!   project's display units, and the vendor is searched with the name
//!   (capability 64: "name, vendor, diameter").
//! - *Add to project* copies every ticked row, in-project ones included,
//!   under new project ids as `tools add` does (ADR-0002; mock-up 04's
//!   "Add 2 tools to project" with an in-project row ticked). Upstream
//!   left out rows already imported.
//! - "In project" is every field but the id equal, where upstream listed
//!   the fields it compared.
//!
//! Nothing here depends on the coordinate frame, so nothing was
//! re-derived for Y-up.

use std::collections::BTreeSet;

use dc_library::import::Merged;
use dc_library::{Library, LibraryError, LibraryTool, LibraryToolId};
use dc_project::{Tool, ToolKind, Units};

/// The type filter's choices: every type, or one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KindFilter {
    /// Every type.
    #[default]
    All,
    /// End mills.
    EndMill,
    /// Ball noses.
    BallNose,
    /// V-bits of any angle.
    VBit,
    /// Engravers.
    Engraver,
    /// Drills.
    Drill,
}

impl KindFilter {
    /// The choices, in the dropdown's order.
    pub const ALL: [Self; 6] = [
        Self::All,
        Self::EndMill,
        Self::BallNose,
        Self::VBit,
        Self::Engraver,
        Self::Drill,
    ];

    /// Whether a tool of `kind` passes.
    #[must_use]
    pub const fn matches(self, kind: &ToolKind) -> bool {
        matches!(
            (self, kind),
            (Self::All, _)
                | (Self::EndMill, ToolKind::EndMill)
                | (Self::BallNose, ToolKind::BallNose)
                | (Self::VBit, ToolKind::VBit { .. })
                | (Self::Engraver, ToolKind::Engraver)
                | (Self::Drill, ToolKind::Drill)
        )
    }
}

/// What the window filters by (upstream `LibraryFilterValues`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// The search box's text.
    pub search: String,
    /// The type filter.
    pub kind: KindFilter,
}

/// Whether `tool`, a project's, is `entry` (upstream
/// `toolMatchesLibraryEntry`): every field but the id equal. A project
/// copy edited after it was added is another tool.
#[must_use]
pub fn tool_matches_library_entry(tool: &Tool, entry: &LibraryTool) -> bool {
    Tool {
        id: tool.id.clone(),
        ..entry.tool.clone()
    } == *tool
}

/// Lower case, runs of whitespace as one space, trimmed (upstream
/// `normalizeSearch`).
fn normalize(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// `diameter_mm` as the table shows it in `units`: up to four decimals,
/// trailing zeros dropped (`6.35`, `6`, `0.25`).
#[must_use]
pub fn diameter_text(diameter_mm: f64, units: Units) -> String {
    trimmed(crate::units::to_display(diameter_mm, units), 4)
}

/// `value` to at most `decimals` places, with no trailing zeros or point
/// and no negative zero.
pub(crate) fn trimmed(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    };
    if text == "-0" { "0".to_owned() } else { text }
}

/// Upstream `entryMatchesFilters`: the type filter, then the search. A
/// search is whitespace-separated terms; an entry matches when any term
/// is in its name, vendor or diameter (in `units`), ignoring case.
#[must_use]
pub fn entry_matches(entry: &LibraryTool, filter: &Filter, units: Units) -> bool {
    if !filter.kind.matches(&entry.tool.cutter) {
        return false;
    }
    let search = normalize(&filter.search);
    if search.is_empty() {
        return true;
    }
    let haystack = normalize(&format!(
        "{} {} {}",
        entry.tool.name,
        entry.tool.vendor.as_deref().unwrap_or_default(),
        diameter_text(entry.tool.diameter_mm, units)
    ));
    search.split(' ').any(|term| haystack.contains(term))
}

/// The library's tools that pass `filter`, in library order (upstream
/// `filterLibraryEntries`).
#[must_use]
pub fn filter<'a>(lib: &'a Library, filter: &Filter, units: Units) -> Vec<&'a LibraryTool> {
    lib.tools
        .iter()
        .filter(|entry| entry_matches(entry, filter, units))
        .collect()
}

/// A library row with its "in project" mark (upstream `LibraryEntryState`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EntryState<'a> {
    /// The library tool.
    pub entry: &'a LibraryTool,
    /// Whether a project tool equals it ([`tool_matches_library_entry`]).
    pub in_project: bool,
}

/// Every library row, marked against `project_tools` (upstream
/// `annotateEntryStates`).
#[must_use]
pub fn annotate<'a>(lib: &'a Library, project_tools: &[Tool]) -> Vec<EntryState<'a>> {
    lib.tools
        .iter()
        .map(|entry| EntryState {
            entry,
            in_project: project_tools
                .iter()
                .any(|tool| tool_matches_library_entry(tool, entry)),
        })
        .collect()
}

/// The marked rows that pass `filter`, for display (upstream
/// `filterLibraryEntryStates`). Ticks and counts work on the full list,
/// so a tick survives a search that hides its row.
#[must_use]
pub fn filter_states<'s, 'a>(
    states: &'s [EntryState<'a>],
    filter: &Filter,
    units: Units,
) -> Vec<&'s EntryState<'a>> {
    states
        .iter()
        .filter(|state| entry_matches(state.entry, filter, units))
        .collect()
}

/// How many of the ticked ids are rows of the library, hidden or not
/// (upstream `countImportableSelected`, in-project rows included: module
/// docs).
#[must_use]
pub fn count_selected(states: &[EntryState<'_>], selected: &BTreeSet<LibraryToolId>) -> usize {
    states
        .iter()
        .filter(|state| selected.contains(&state.entry.id))
        .count()
}

/// The ticked library tools as copies for a project holding
/// `project_tools`, in library order, each under the next free project id
/// (upstream `buildImportInput`; `dc_library::project_copy`, what `tools
/// add` writes, applied in turn).
#[must_use]
pub fn build_add(
    lib: &Library,
    project_tools: &[Tool],
    selected: &BTreeSet<LibraryToolId>,
) -> Vec<Tool> {
    let mut tools = project_tools.to_vec();
    let mut added = Vec::new();
    for entry in lib.tools.iter().filter(|t| selected.contains(&t.id)) {
        let copy = dc_library::project_copy(&tools, &entry.tool);
        tools.push(copy.clone());
        added.push(copy);
    }
    added
}

/// *Import…*: the tools of a Fusion `.tools` or `.json` library or a
/// DreamCarve library in `bytes` merged into `lib`, with the per-tool
/// report `tools import` prints (`dc_library::import`). `replace` is
/// *Replace existing* (`--replace`).
///
/// # Errors
///
/// [`LibraryError`] when the file does not load or one of its tools is
/// not valid; nothing is imported then.
pub fn import(lib: &Library, bytes: &[u8], replace: bool) -> Result<Merged, LibraryError> {
    let incoming = dc_library::import::read(bytes)?;
    dc_library::import::merge(lib, incoming, replace)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trimmed_drops_trailing_zeros() {
        assert_eq!(trimmed(6.35, 4), "6.35");
        assert_eq!(trimmed(6.0, 4), "6");
        assert_eq!(trimmed(0.236_220_47, 4), "0.2362");
        assert_eq!(trimmed(-0.000_01, 4), "0");
        assert_eq!(trimmed(18000.0, 0), "18000");
    }
}
