// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Importing a library file into the library, with the per-tool report
//! (`docs/specs/phase-2/functional.md`, capabilities 21 and 22, acceptance
//! 35; `docs/specs/phase-4/functional.md`, capability 64). Moved here from
//! the `dreamcarve` CLI's `tools import` so the app's Tool Library window
//! makes the same library and shows the same lines: the CLI prints
//! [`ImportReport::lines`] and [`ImportReport::summary`], the app shows
//! them.
//!
//! A file is a DreamCarve library or a Fusion 360 library, told apart by
//! content ([`crate::fusion::is_fusion_library`]), not by extension.
//! [`read`] parses it; [`merge`] adds each tool as [`crate::add`] (or
//! [`crate::replace`] for `--replace`) decides. [`review`] is the app's
//! import review (phase 4, capability 64b): both merges, computed without
//! writing, with each item's [`Status`], so the user chooses *Keep mine*
//! or *Replace with imported* before anything is written.

use std::fmt;

use dc_project::Tool;

use crate::library::{self, AddOutcome, Library, LibraryToolId};
use crate::{LibraryError, fusion};

/// One tool of a file being imported: the tool, where it came from, and
/// the notes its report line carries (a Fusion preset's unset values).
#[derive(Debug, Clone, PartialEq)]
pub struct IncomingTool {
    /// The tool as the file describes it.
    pub tool: Tool,
    /// Its source, `fusion:<guid>:<preset>` for a Fusion preset.
    pub source: Option<String>,
    /// What the report line notes, such as `feed unset`.
    pub notes: Vec<String>,
}

/// A parsed library file, before it is merged.
#[derive(Debug, Clone, PartialEq)]
pub struct Incoming {
    /// The tools, in file order.
    pub tools: Vec<IncomingTool>,
    /// A Fusion library's items DreamCarve has no cutter for, each with
    /// its description and the reason; `None` for a DreamCarve library,
    /// whose report has no skipped count.
    pub skipped: Option<Vec<fusion::Skipped>>,
}

/// Parses `bytes` as a Fusion 360 library or a DreamCarve library.
///
/// # Errors
///
/// [`LibraryError`] when the file does not load: a damaged archive, a
/// Fusion version too old, JSON that is not a library, a tool that breaks
/// a validation rule.
pub fn read(bytes: &[u8]) -> Result<Incoming, LibraryError> {
    if fusion::is_fusion_library(bytes) {
        let fusion = fusion::import(bytes)?;
        let tools = fusion
            .tools
            .into_iter()
            .map(|t| IncomingTool {
                tool: t.tool,
                source: Some(t.source),
                notes: t.notes.iter().map(ToString::to_string).collect(),
            })
            .collect();
        Ok(Incoming {
            tools,
            skipped: Some(fusion.skipped),
        })
    } else {
        let tools = library::load(bytes)?
            .tools
            .into_iter()
            .map(|entry| IncomingTool {
                tool: entry.tool,
                source: entry.source,
                notes: Vec::new(),
            })
            .collect();
        Ok(Incoming {
            tools,
            skipped: None,
        })
    }
}

/// What happened to one item of an imported file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Added under `id`, as `stored_name` (the file's name, or that name
    /// with the suffix that keeps it unique).
    Imported {
        /// The new library id.
        id: LibraryToolId,
        /// The name it is stored under.
        stored_name: String,
    },
    /// The same tool is already in the library under `id`.
    AlreadyPresent {
        /// The existing tool's id.
        id: LibraryToolId,
    },
    /// *Replace existing*: the tool with the same source, `id`, was
    /// overwritten.
    Replaced {
        /// The overwritten tool's id.
        id: LibraryToolId,
    },
    /// A Fusion item DreamCarve has no cutter for.
    Skipped {
        /// Why, as `tools import` prints it.
        reason: String,
    },
}

/// One line of the report: an item's name, what happened, and its notes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportEntry {
    /// The tool's name in the file, or a skipped item's description.
    pub name: String,
    /// What happened to it.
    pub outcome: Outcome,
    /// The notes on an imported tool's line (`feed unset`).
    pub notes: Vec<String>,
}

impl fmt::Display for ReportEntry {
    /// The line `tools import` prints: `` `name`: imported (lt1); feed
    /// unset ``, `` `name`: already present (lt2) ``, `` `name`: skipped:
    /// <reason> ``.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = &self.name;
        match &self.outcome {
            Outcome::Imported { id, stored_name } if *stored_name == self.name => {
                write!(f, "`{name}`: imported ({id})")?;
            }
            Outcome::Imported { id, stored_name } => {
                write!(f, "`{name}`: imported as `{stored_name}` ({id})")?;
            }
            Outcome::AlreadyPresent { id } => write!(f, "`{name}`: already present ({id})")?,
            Outcome::Replaced { id } => write!(f, "`{name}`: replaced ({id})")?,
            Outcome::Skipped { reason } => return write!(f, "`{name}`: skipped: {reason}"),
        }
        if !self.notes.is_empty() {
            write!(f, "; {}", self.notes.join(", "))?;
        }
        Ok(())
    }
}

/// The report of one import: a line per item, then the counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportReport {
    /// One entry per tool, in file order, then each skipped item.
    pub entries: Vec<ReportEntry>,
    /// How many tools were added.
    pub imported: usize,
    /// How many were already in the library.
    pub present: usize,
    /// How many overwrote a tool with the same source.
    pub replaced: usize,
    /// How many Fusion items were skipped; `None` for a DreamCarve file.
    pub skipped: Option<usize>,
}

impl ImportReport {
    /// The per-item lines, as `tools import` prints them.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.entries.iter().map(ToString::to_string).collect()
    }

    /// The last line `tools import` prints: the counts and the library
    /// file, `13 imported, 0 already present, 0 replaced, 10 skipped;
    /// library: <library_file>`.
    #[must_use]
    pub fn summary(&self, library_file: &str) -> String {
        let skipped = self
            .skipped
            .map_or(String::new(), |n| format!(", {n} skipped"));
        format!(
            "{} imported, {} already present, {} replaced{skipped}; library: {library_file}",
            self.imported, self.present, self.replaced
        )
    }
}

/// The library after an import, and its report.
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    /// The library with the file's tools in it. Equal to the library
    /// merged into when nothing was added or replaced, so the caller
    /// writes the file only when it differs.
    pub library: Library,
    /// What happened to each item.
    pub report: ImportReport,
}

/// Adds each of `incoming`'s tools to a copy of `lib`, in file order, as
/// [`crate::add`] decides, or as [`crate::replace`] does when `replace`.
///
/// # Errors
///
/// [`LibraryError`] when a tool breaks a validation rule; nothing is
/// merged then.
pub fn merge(lib: &Library, incoming: Incoming, replace: bool) -> Result<Merged, LibraryError> {
    let mut library = lib.clone();
    let mut entries = Vec::with_capacity(incoming.tools.len());
    let (mut imported, mut present, mut replaced) = (0_usize, 0_usize, 0_usize);
    for item in incoming.tools {
        let name = item.tool.name.clone();
        let outcome = if replace {
            library::replace(&mut library, item.tool, item.source)
        } else {
            library::add(&mut library, item.tool, item.source)
        }?;
        let outcome = match outcome {
            AddOutcome::Added { id } => {
                imported += 1;
                let stored_name = library::find(&library, id.as_str())
                    .map_or_else(|_| name.clone(), |t| t.tool.name.clone());
                Outcome::Imported { id, stored_name }
            }
            AddOutcome::AlreadyPresent { id } => {
                present += 1;
                Outcome::AlreadyPresent { id }
            }
            AddOutcome::Replaced { id } => {
                replaced += 1;
                Outcome::Replaced { id }
            }
        };
        entries.push(ReportEntry {
            name,
            outcome,
            notes: item.notes,
        });
    }
    let skipped = incoming.skipped.map(|items| {
        let count = items.len();
        entries.extend(items.into_iter().map(|item| ReportEntry {
            name: item.description,
            outcome: Outcome::Skipped {
                reason: item.reason.to_string(),
            },
            notes: Vec::new(),
        }));
        count
    });
    Ok(Merged {
        library,
        report: ImportReport {
            entries,
            imported,
            present,
            replaced,
            skipped,
        },
    })
}

/// What an item of an imported file is beside the library, whichever way
/// the import goes (phase 4, capability 64b).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    /// Not in the library: it is added.
    New,
    /// The same tool is already in the library: nothing changes.
    Present,
    /// A library tool came from the same source and the file's entry now
    /// differs: kept beside it, or written over it with *Replace*.
    Changed,
    /// A Fusion item DreamCarve has no cutter for.
    Skipped,
}

/// One item of the review: what the file holds, its status, and what each
/// way of importing does with it.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewItem {
    /// The tool's name in the file, or a skipped item's description.
    pub name: String,
    /// The tool as the file describes it; `None` for a skipped item.
    pub tool: Option<Tool>,
    /// What it is beside the library.
    pub status: Status,
    /// Its outcome when the import keeps yours (`tools import`).
    pub keep: Outcome,
    /// Its outcome when the import replaces (`tools import --replace`).
    pub replace: Outcome,
    /// The notes on its report line (`feed unset`).
    pub notes: Vec<String>,
}

/// The import review (capability 64b): both imports `tools import` can
/// make of a file, computed without writing, and each item's status.
#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    /// One item per tool, in file order, then each skipped item: the
    /// report's order.
    pub items: Vec<ReviewItem>,
    /// What `tools import` makes: a changed tool added beside yours.
    pub keep: Merged,
    /// What `tools import --replace` makes: a changed tool written over
    /// yours, keeping its id.
    pub replace: Merged,
}

impl Review {
    /// How many items have `status`.
    #[must_use]
    pub fn count(&self, status: Status) -> usize {
        self.items.iter().filter(|i| i.status == status).count()
    }

    /// Whether any item is [`Status::Changed`]: only then do the two ways
    /// of importing differ, and only then is the choice offered.
    #[must_use]
    pub fn has_changed(&self) -> bool {
        self.items.iter().any(|i| i.status == Status::Changed)
    }

    /// The import `replace` chooses: [`Self::replace`] or [`Self::keep`].
    #[must_use]
    pub const fn merged(&self, replace: bool) -> &Merged {
        if replace { &self.replace } else { &self.keep }
    }

    /// How many tools the import `replace` chooses writes: added plus
    /// replaced.
    #[must_use]
    pub fn writes(&self, replace: bool) -> usize {
        let report = &self.merged(replace).report;
        report.imported.saturating_add(report.replaced)
    }
}

/// The review of importing `incoming` into `lib`: [`merge`] both ways,
/// with each item's [`Status`]. `lib` is not changed and nothing is
/// written; the caller writes [`Review::merged`]'s library once the user
/// chooses.
///
/// An item is [`Status::Changed`] when the replacing import overwrites a
/// tool with its source, [`Status::Present`] when it is already there,
/// [`Status::Skipped`] when the file's item has no cutter, and
/// [`Status::New`] otherwise.
///
/// # Errors
///
/// [`LibraryError`] when a tool breaks a validation rule, as [`merge`].
pub fn review(lib: &Library, incoming: Incoming) -> Result<Review, LibraryError> {
    let tools: Vec<Tool> = incoming.tools.iter().map(|t| t.tool.clone()).collect();
    let keep = merge(lib, incoming.clone(), false)?;
    let replace = merge(lib, incoming, true)?;
    let items = keep
        .report
        .entries
        .iter()
        .zip(&replace.report.entries)
        .enumerate()
        .map(|(n, (kept, replaced))| {
            let status = match (&kept.outcome, &replaced.outcome) {
                (Outcome::Skipped { .. }, _) => Status::Skipped,
                (_, Outcome::Replaced { .. }) => Status::Changed,
                (Outcome::AlreadyPresent { .. }, Outcome::AlreadyPresent { .. }) => Status::Present,
                _ => Status::New,
            };
            ReviewItem {
                name: kept.name.clone(),
                tool: tools.get(n).cloned(),
                status,
                keep: kept.outcome.clone(),
                replace: replaced.outcome.clone(),
                notes: kept.notes.clone(),
            }
        })
        .collect();
    Ok(Review {
        items,
        keep,
        replace,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::test_support::tool_6_35_upcut;

    fn entry(outcome: Outcome, notes: &[&str]) -> ReportEntry {
        ReportEntry {
            name: "1/4 end mill".into(),
            outcome,
            notes: notes.iter().map(|n| (*n).to_owned()).collect(),
        }
    }

    /// Each line reads as `tools import` printed it before the move.
    #[test]
    fn lines_read_as_the_cli_printed_them() {
        let id = LibraryToolId::new("lt2");
        let imported = Outcome::Imported {
            id: id.clone(),
            stored_name: "1/4 end mill".into(),
        };
        assert_eq!(
            entry(imported.clone(), &[]).to_string(),
            "`1/4 end mill`: imported (lt2)"
        );
        assert_eq!(
            entry(imported, &["feed unset", "rpm unset"]).to_string(),
            "`1/4 end mill`: imported (lt2); feed unset, rpm unset"
        );
        let renamed = Outcome::Imported {
            id: id.clone(),
            stored_name: "1/4 end mill (2)".into(),
        };
        assert_eq!(
            entry(renamed, &[]).to_string(),
            "`1/4 end mill`: imported as `1/4 end mill (2)` (lt2)"
        );
        assert_eq!(
            entry(Outcome::AlreadyPresent { id: id.clone() }, &[]).to_string(),
            "`1/4 end mill`: already present (lt2)"
        );
        assert_eq!(
            entry(Outcome::Replaced { id }, &[]).to_string(),
            "`1/4 end mill`: replaced (lt2)"
        );
        let skipped = Outcome::Skipped {
            reason: "unsupported tool type `probe`".into(),
        };
        assert_eq!(
            entry(skipped, &["ignored"]).to_string(),
            "`1/4 end mill`: skipped: unsupported tool type `probe`"
        );
    }

    /// A DreamCarve file merges tool by tool and has no skipped count; a
    /// second merge is all "already present" and changes nothing.
    #[test]
    fn merge_counts_and_leaves_the_input_alone() {
        let mut file = Library::default();
        library::add(&mut file, tool_6_35_upcut(), None).unwrap();
        let bytes = library::save(&file).unwrap();
        let incoming = read(bytes.as_bytes()).unwrap();
        let empty = Library::default();
        let merged = merge(&empty, incoming.clone(), false).unwrap();
        assert_eq!(empty, Library::default(), "the input is not changed");
        assert_eq!(merged.report.imported, 1);
        assert_eq!(merged.report.skipped, None);
        assert_eq!(
            merged.report.summary("tools.json"),
            "1 imported, 0 already present, 0 replaced; library: tools.json"
        );
        let again = merge(&merged.library, incoming, false).unwrap();
        assert_eq!(again.library, merged.library);
        assert_eq!(again.report.present, 1);
    }

    /// The review is both merges: a tool whose source is in the library
    /// with other values is Changed, added beside yours when kept and
    /// written over yours when replaced; one already there is Present; the
    /// input library is not changed.
    #[test]
    fn review_is_both_merges() {
        let mut file = Library::default();
        let source = Some("file:a".to_owned());
        library::add(&mut file, tool_6_35_upcut(), source.clone()).unwrap();
        let mut other = tool_6_35_upcut();
        other.name = "Other".into();
        other.diameter_mm = 3.175;
        library::add(&mut file, other, Some("file:b".into())).unwrap();
        let incoming = read(library::save(&file).unwrap().as_bytes()).unwrap();

        let fresh = review(&Library::default(), incoming.clone()).unwrap();
        assert_eq!(fresh.count(Status::New), 2);
        assert!(!fresh.has_changed());
        assert_eq!(fresh.keep, fresh.replace, "nothing changed: one import");
        assert_eq!(fresh.writes(false), 2);

        // Yours: the first tool edited, so the file's entry now differs.
        let mut mine = file.clone();
        let first = mine.tools[0].id.clone();
        let mut edited = mine.tools[0].tool.clone();
        edited.feed_mm_min = Some(250.0);
        library::update(&mut mine, &first, edited).unwrap();
        let before = mine.clone();
        let changed = review(&mine, incoming.clone()).unwrap();
        assert_eq!(mine, before, "the library is not changed");
        let statuses: Vec<Status> = changed.items.iter().map(|i| i.status).collect();
        assert_eq!(statuses, [Status::Changed, Status::Present]);
        assert!(changed.has_changed());
        assert_eq!(changed.keep, merge(&mine, incoming.clone(), false).unwrap());
        assert_eq!(changed.replace, merge(&mine, incoming, true).unwrap());
        assert!(matches!(
            &changed.items[0].keep,
            Outcome::Imported { stored_name, .. } if stored_name.ends_with(" (2)")
        ));
        assert_eq!(
            changed.items[0].replace,
            Outcome::Replaced { id: first.clone() }
        );
        assert_eq!(changed.replace.library, file, "replaced in place, id kept");
        assert_eq!((changed.writes(false), changed.writes(true)), (1, 1));
        assert_eq!(
            changed.items[0].tool.as_ref().map(|t| t.feed_mm_min),
            Some(file.tools[0].tool.feed_mm_min)
        );
    }
}
