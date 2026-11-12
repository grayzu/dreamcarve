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
//!   library's display units, and the vendor and product id are searched
//!   with the name (capability 64: "name, vendor, product, diameter").
//!
//! Not in upstream (slice 30, #622; capabilities 64 and 64a): the rows
//! grouped by vendor ([`groups`]), and editing the library in the sheet:
//! a field ([`edit`]), *New tool* ([`new_tool`]), *Duplicate*
//! ([`duplicate`]) and *Revert changes* ([`LibraryEdit`]). Each returns
//! the whole library, which the shell writes at once; none touches a
//! project (ADR-0002). Slice 31 (#623; capability 64b): the import
//! review ([`review`], [`review_groups`]), both imports computed before
//! anything is written.
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

use dc_library::import::{Incoming, Merged, Review, ReviewItem};
use dc_library::{Library, LibraryError, LibraryTool, LibraryToolId};
use dc_project::{Tool, ToolId, ToolKind, Units};

use crate::command::copy_name;
use crate::error::CommandError;
use crate::tools::ToolField;

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

/// A feed or plunge rate, `mm_min` millimetres a minute, as the table
/// shows it in `units`: to a tenth, trailing zeros dropped (`1524`,
/// `47.2`). The editing pane shows every digit.
#[must_use]
pub fn rate_text(mm_min: f64, units: Units) -> String {
    trimmed(crate::units::to_display(mm_min, units), 1)
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
/// is in its name, vendor, product id or diameter (in `units`), ignoring
/// case.
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
        "{} {} {} {}",
        entry.tool.name,
        entry.tool.vendor.as_deref().unwrap_or_default(),
        entry.tool.product_id.as_deref().unwrap_or_default(),
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

/// The tools of a Fusion `.tools` or `.json` library or a DreamCarve
/// library in `bytes` merged into `lib`, with the per-tool report `tools
/// import` prints (`dc_library::import`). `replace` is `--replace`.
///
/// # Errors
///
/// [`LibraryError`] when the file does not load or one of its tools is
/// not valid; nothing is imported then.
pub fn import(lib: &Library, bytes: &[u8], replace: bool) -> Result<Merged, LibraryError> {
    let incoming = dc_library::import::read(bytes)?;
    dc_library::import::merge(lib, incoming, replace)
}

/// *Import…* (capability 64b): the file in `bytes` read, and its review
/// against `lib`, both merges computed without writing
/// (`dc_library::import::review`). The file's tools come back too, so the
/// import can be merged again into the library as it is on disk when the
/// user chooses *Import*.
///
/// # Errors
///
/// [`LibraryError`] when the file does not load or one of its tools is
/// not valid.
pub fn review(lib: &Library, bytes: &[u8]) -> Result<(Incoming, Review), LibraryError> {
    let incoming = dc_library::import::read(bytes)?;
    let review = dc_library::import::review(lib, incoming.clone())?;
    Ok((incoming, review))
}

/// One vendor's items in the import review's table: the vendor (`None`
/// is *No vendor*, where skipped items go too) and its items in file
/// order.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewGroup<'a> {
    /// The vendor, trimmed.
    pub vendor: Option<&'a str>,
    /// Its items, in file order, each with its place in the file (the
    /// review's index), which identifies it.
    pub items: Vec<(usize, &'a ReviewItem)>,
}

/// The review's items grouped by vendor as the library table groups its
/// rows ([`groups`]): sorted by vendor, *No vendor* last.
#[must_use]
pub fn review_groups(review: &Review) -> Vec<ReviewGroup<'_>> {
    let mut groups: Vec<ReviewGroup<'_>> = Vec::new();
    for (index, item) in review.items.iter().enumerate() {
        let vendor = item.tool.as_ref().and_then(vendor_of);
        if let Some(group) = groups.iter_mut().find(|g| g.vendor == vendor) {
            group.items.push((index, item));
        } else {
            groups.push(ReviewGroup {
                vendor,
                items: vec![(index, item)],
            });
        }
    }
    groups.sort_by(|a, b| vendor_order(a.vendor, b.vendor));
    groups
}

/// Vendor groups' order: by name ignoring case, then exactly; *No vendor*
/// last.
fn vendor_order(a: Option<&str>, b: Option<&str>) -> std::cmp::Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.to_lowercase().cmp(&b.to_lowercase()).then(a.cmp(b)),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => std::cmp::Ordering::Equal,
    }
}

/// One vendor's rows in the library table (capability 64): the group
/// header `Amana · 2` and its tools.
#[derive(Debug, Clone, PartialEq)]
pub struct VendorGroup<'s, 'a> {
    /// The vendor, trimmed; `None` is *No vendor* (no vendor, or a blank
    /// one).
    pub vendor: Option<&'a str>,
    /// The group's rows that pass the filter, in library order.
    pub rows: Vec<&'s EntryState<'a>>,
}

/// `tool`'s vendor as the table groups it: trimmed, a blank one none.
fn vendor_of(tool: &Tool) -> Option<&str> {
    tool.vendor
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// The rows of `states` that pass `filter`, grouped by vendor: groups
/// sorted by vendor name (ignoring case, then exactly), *No vendor*
/// last; rows in library order. A group with no row passing the filter
/// is left out, so a search hides it.
#[must_use]
pub fn groups<'s, 'a>(
    states: &'s [EntryState<'a>],
    filter: &Filter,
    units: Units,
) -> Vec<VendorGroup<'s, 'a>> {
    let mut groups: Vec<VendorGroup<'s, 'a>> = Vec::new();
    for state in filter_states(states, filter, units) {
        let vendor = vendor_of(&state.entry.tool);
        if let Some(group) = groups.iter_mut().find(|g| g.vendor == vendor) {
            group.rows.push(state);
        } else {
            groups.push(VendorGroup {
                vendor,
                rows: vec![state],
            });
        }
    }
    groups.sort_by(|a, b| vendor_order(a.vendor, b.vendor));
    groups
}

/// The tool `id` of `lib`.
fn tool_of<'l>(lib: &'l Library, id: &LibraryToolId) -> Result<&'l LibraryTool, CommandError> {
    lib.tools.iter().find(|t| t.id == *id).ok_or_else(|| {
        CommandError::Library(LibraryError::NotFound {
            key: id.to_string(),
        })
    })
}

/// `lib` with library tool `id` set to `tool` in place (its id, place and
/// `source` kept; `dc_library::update`): what the editing pane writes.
///
/// # Errors
///
/// [`CommandError::Library`] when there is no such tool, the tool fails
/// `dc_library::validate_tool`, or another tool has its name.
pub fn set_tool(lib: &Library, id: &LibraryToolId, tool: Tool) -> Result<Library, CommandError> {
    let mut next = lib.clone();
    dc_library::update(&mut next, id, tool)?;
    Ok(next)
}

/// Capability 64a: `lib` with field `field` of library tool `id` set from
/// `text`, typed in the library's display units `units`, as the project
/// tool editor reads it ([`crate::tools::set_field`]); the library stays
/// in millimetres. Blank text clears an optional value, and nothing is
/// ever filled in (principle 5). The shell writes the result at once; no
/// project changes.
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when the text is not a value of the
/// field's kind, and the errors of [`set_tool`].
pub fn edit(
    lib: &Library,
    id: &LibraryToolId,
    field: ToolField,
    text: &str,
    units: Units,
) -> Result<Library, CommandError> {
    let current = tool_of(lib, id)?;
    let tool = crate::tools::set_field(&current.tool, field, text, units)?;
    set_tool(lib, id, tool)
}

/// *New tool* in the library (capability 64a): the Tools tab's
/// placeholder (63; an end mill, 6.35 mm, 2 flutes, no rate), named
/// `tool<n>` within the library ([`crate::tools::new_tool_name`]), added
/// at the end under the next `lt<n>` id with no `source`. Returns the
/// library and the new tool's id.
#[must_use]
pub fn new_tool(lib: &Library) -> (Library, LibraryToolId) {
    let id = dc_library::next_id(lib);
    let tools: Vec<Tool> = lib.tools.iter().map(|t| t.tool.clone()).collect();
    let tool = dc_job::project_from::placeholder_tool(
        ToolId::new(id.as_str()),
        crate::tools::new_tool_name(&tools),
    );
    let mut next = lib.clone();
    next.tools.push(LibraryTool {
        id: id.clone(),
        source: None,
        tool,
    });
    (next, id)
}

/// *Duplicate* in the library (capability 64a): a copy of tool `id`
/// right below it, named `<name> (2)` (or the next free suffix), under
/// the next `lt<n>` id, every value kept and no `source`, so a later
/// import with *Replace* never overwrites the copy. Returns the library
/// and the copy's id.
///
/// # Errors
///
/// [`CommandError::Library`] when there is no such tool.
pub fn duplicate(
    lib: &Library,
    id: &LibraryToolId,
) -> Result<(Library, LibraryToolId), CommandError> {
    let original = tool_of(lib, id)?;
    let copy_id = dc_library::next_id(lib);
    let taken: Vec<&str> = lib.tools.iter().map(|t| t.tool.name.as_str()).collect();
    let copy = LibraryTool {
        id: copy_id.clone(),
        source: None,
        tool: Tool {
            id: ToolId::new(copy_id.as_str()),
            name: copy_name(&taken, &original.tool.name),
            ..original.tool.clone()
        },
    };
    dc_library::validate_tool(&copy_id, &copy.tool)?;
    let mut next = lib.clone();
    let at = next
        .tools
        .iter()
        .position(|t| t.id == *id)
        .map_or(next.tools.len(), |i| i.saturating_add(1));
    next.tools.insert(at, copy);
    Ok((next, copy_id))
}

/// The library editing session (capability 64a): the library as the
/// sheet opened it, so *Revert changes* can put a tool back. Library
/// edits have no undo; this covers the session.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LibraryEdit {
    opened: Library,
}

impl LibraryEdit {
    /// A session over `lib`, as opened now.
    #[must_use]
    pub fn open(lib: &Library) -> Self {
        Self {
            opened: lib.clone(),
        }
    }

    /// Tool `id` as the sheet opened it; `None` for a tool made since.
    #[must_use]
    pub fn opened(&self, id: &LibraryToolId) -> Option<&LibraryTool> {
        self.opened.tools.iter().find(|t| t.id == *id)
    }

    /// Whether tool `id` of `lib` differs from the tool as opened: what
    /// enables *Revert changes*. A tool made since the sheet opened has
    /// nothing to go back to.
    #[must_use]
    pub fn changed(&self, lib: &Library, id: &LibraryToolId) -> bool {
        match (self.opened(id), lib.tools.iter().find(|t| t.id == *id)) {
            (Some(opened), Some(now)) => opened != now,
            _ => false,
        }
    }

    /// *Revert changes*: `lib` with tool `id` as the sheet opened it, in
    /// its place, every other tool as it is.
    ///
    /// # Errors
    ///
    /// [`CommandError::Library`] when `lib` has no such tool, the sheet
    /// opened without it, or another tool has since taken its name.
    pub fn revert(&self, lib: &Library, id: &LibraryToolId) -> Result<Library, CommandError> {
        let opened = self.opened(id).ok_or_else(|| {
            CommandError::Library(LibraryError::NotFound {
                key: id.to_string(),
            })
        })?;
        let mut next = set_tool(lib, id, opened.tool.clone())?;
        if let Some(slot) = next.tools.iter_mut().find(|t| t.id == *id) {
            slot.source.clone_from(&opened.source);
        }
        Ok(next)
    }
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
