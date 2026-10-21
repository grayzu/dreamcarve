// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/toolLibrary.ts

//! The library and its file (`docs/specs/phase-2/design.md`, "Data model →
//! dc-library"; "Algorithms → Tool library CLI").
//!
//! The file is JSON: `{ "version": 1, "tools": [ … ] }`, each tool a block
//! with its library `id`, its `source` and the `tool` itself, saved exactly
//! as a `.dcv` saves a tool (every optional value written, `null` when
//! unset). The `id` inside `tool` is written for completeness and ignored
//! on load: the library id is the one outside it, and `tools add` replaces
//! it with a project id anyway.
//!
//! Validation rules are ported from PureCutCNC's `parseToolLibraryFile`
//! (`src/toolLibrary.ts`): a positive diameter, flutes of 1 or more, a
//! step-over fraction in (0, 1], a V-bit angle in (0, 180). Upstream
//! silently drops an entry that fails them; here it is an error naming
//! the tool and the field, so a typo in the file is never a vanished tool.
//! Values upstream requires (feed, plunge, RPM, step-down) are optional
//! here, as in a project, but when set must be positive. Nothing in the
//! rules depends on the coordinate frame, so nothing was re-derived for
//! Y-up.

use dc_project::{Tool, ToolId, ToolKind};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;

use crate::LibraryError;

/// The library file format version this build reads and writes.
pub const LIBRARY_VERSION: u32 = 1;

/// Identifies a [`LibraryTool`]. `lt<n>`, so it can never be mistaken for
/// a project's `t<n>`.
///
/// Serialized as a plain JSON string. Ordered, so it can key a `BTreeMap`
/// and sort deterministically.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LibraryToolId(String);

impl LibraryToolId {
    /// Wraps a string as a library tool id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LibraryToolId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for LibraryToolId {
    fn from(id: &str) -> Self {
        Self::new(id)
    }
}

/// The tool library: what `<config dir>/tools.json` holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Library {
    /// The file format version, [`LIBRARY_VERSION`].
    pub version: u32,
    /// The tools, in the order they were added.
    pub tools: Vec<LibraryTool>,
}

impl Default for Library {
    /// An empty library at the current version.
    fn default() -> Self {
        Self {
            version: LIBRARY_VERSION,
            tools: Vec::new(),
        }
    }
}

/// One tool in the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LibraryTool {
    /// The library id, `lt<n>`.
    pub id: LibraryToolId,
    /// Where the tool came from: `fusion:<guid>:<preset>` for a tool
    /// imported from a Fusion 360 library, `None` for one added by hand
    /// or from a DreamCarve file without a source.
    #[serde(deserialize_with = "Option::deserialize")]
    pub source: Option<String>,
    /// The tool, as a project embeds it. Its `id` mirrors [`Self::id`]
    /// and is ignored on load.
    pub tool: Tool,
}

/// What [`add`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    /// The tool was added under this id. Its name may have gained a
    /// ` (2)` suffix to stay unique; read it from the library.
    Added {
        /// The new tool's id.
        id: LibraryToolId,
    },
    /// A tool with the same fields (ignoring ids) is already in the
    /// library; nothing changed.
    AlreadyPresent {
        /// The id of the existing tool.
        id: LibraryToolId,
    },
}

/// Parses library bytes: JSON, `version` read and checked against
/// [`LIBRARY_VERSION`], then deserialized strictly (unknown fields and
/// missing fields both fail), then every tool validated ([`validate_tool`])
/// and ids checked for duplicates.
pub fn load(bytes: &[u8]) -> Result<Library, LibraryError> {
    let _ = bytes;
    unimplemented!("slice 20")
}

/// Serializes the library as its file text: pretty JSON, keys in
/// declaration order, one tool per block, every `Option` as `null`, ending
/// in a newline. Deterministic: the same library gives the same bytes.
/// Fails rather than writing a NaN or infinity, which serde_json would
/// silently turn into `null`.
pub fn save(lib: &Library) -> Result<String, LibraryError> {
    let _ = lib;
    unimplemented!("slice 20")
}

/// Checks the validation rules (module docs) on `tool`, which would be
/// stored under `id`; the error names both.
pub fn validate_tool(id: &LibraryToolId, tool: &Tool) -> Result<(), LibraryError> {
    let _ = (id, tool);
    unimplemented!("slice 20")
}

/// Adds `tool` to the library under the next free `lt<n>` id, with
/// `source` saying where it came from. The tool's own `id` is ignored.
/// When a tool with the same fields (ids aside) is already there, nothing
/// changes and the outcome names it. When a different tool already has
/// the same name, the new one is stored as `<name> (2)` (then ` (3)`, …),
/// so [`find`] by name stays unambiguous. A tool that fails
/// [`validate_tool`] is an error and is not added.
pub fn add(
    lib: &mut Library,
    tool: Tool,
    source: Option<String>,
) -> Result<AddOutcome, LibraryError> {
    let _ = (lib, tool, source);
    unimplemented!("slice 20")
}

/// The tool whose id is `key`, else the one tool whose name is `key`. A
/// name shared by several tools is [`LibraryError::Ambiguous`], listing
/// them; no match is [`LibraryError::NotFound`].
pub fn find<'a>(lib: &'a Library, key: &str) -> Result<&'a LibraryTool, LibraryError> {
    let _ = (lib, key);
    unimplemented!("slice 20")
}

/// Removes and returns the tool [`find`] would return for `key`.
pub fn remove(lib: &mut Library, key: &str) -> Result<LibraryTool, LibraryError> {
    let _ = (lib, key);
    unimplemented!("slice 20")
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::test_support::tool_6_35_upcut;

    /// A 90° V-bit with every value set and a tool number.
    fn vbit_90() -> Tool {
        Tool {
            id: ToolId::new("t1"),
            name: "90° V-bit".into(),
            number: Some(5),
            cutter: ToolKind::VBit {
                included_angle_deg: 90.0,
            },
            diameter_mm: 12.7,
            flutes: 2,
            feed_mm_min: Some(800.0),
            plunge_mm_min: Some(200.0),
            rpm: Some(16000),
            step_down_mm: Some(1.5),
            step_over_fraction: Some(0.1),
        }
    }

    /// A ball nose with nothing optional set.
    fn ball_bare() -> Tool {
        Tool {
            id: ToolId::new("t1"),
            name: "3 mm ball".into(),
            number: None,
            cutter: ToolKind::BallNose,
            diameter_mm: 3.0,
            flutes: 2,
            feed_mm_min: None,
            plunge_mm_min: None,
            rpm: None,
            step_down_mm: None,
            step_over_fraction: None,
        }
    }

    /// A library of the three tools above, added in that order, the V-bit
    /// tagged with a Fusion source.
    fn sample_library() -> Library {
        let mut lib = Library::default();
        add(&mut lib, tool_6_35_upcut(), None).unwrap();
        add(&mut lib, vbit_90(), Some("fusion:guid-1:preset-a".into())).unwrap();
        add(&mut lib, ball_bare(), None).unwrap();
        lib
    }

    /// The file is deterministic and reads back as written: every cutter
    /// kind, set and unset optionals, a source and no source all survive,
    /// and saving the loaded library gives the same bytes.
    #[test]
    fn round_trip_is_byte_identical() {
        let lib = sample_library();
        let text = save(&lib).unwrap();
        assert!(text.starts_with("{\n  \"version\": 1,\n  \"tools\": [\n"), "{text}");
        assert!(text.ends_with("}\n"), "{text}");
        assert!(text.contains("\"source\": null"), "{text}");
        assert!(text.contains("\"step_down_mm\": null"), "{text}");
        let loaded = load(text.as_bytes()).unwrap();
        assert_eq!(loaded, lib);
        assert_eq!(save(&loaded).unwrap(), text);
        assert_eq!(save(&lib).unwrap(), text, "deterministic");
        // The library ids are `lt<n>`, in order, and each tool's own id
        // mirrors them, so no project `t<n>` id ever appears.
        let ids: Vec<&str> = lib.tools.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["lt1", "lt2", "lt3"]);
        for tool in &lib.tools {
            assert_eq!(tool.tool.id.as_str(), tool.id.as_str(), "{tool:?}");
        }
    }

    /// Equality ignores ids: the same tool under any id is already
    /// present. One changed field makes a new tool, stored under the next
    /// id and, since its name is taken, as `<name> (2)`.
    #[test]
    fn add_detects_already_present() {
        let mut lib = sample_library();
        let mut same = tool_6_35_upcut();
        same.id = ToolId::new("t9");
        assert_eq!(
            add(&mut lib, same, None).unwrap(),
            AddOutcome::AlreadyPresent {
                id: LibraryToolId::new("lt1")
            }
        );
        assert_eq!(lib.tools.len(), 3, "nothing added");

        let mut faster = tool_6_35_upcut();
        faster.feed_mm_min = Some(1200.0);
        assert_eq!(
            add(&mut lib, faster, None).unwrap(),
            AddOutcome::Added {
                id: LibraryToolId::new("lt4")
            }
        );
        assert_eq!(lib.tools.len(), 4);
        let added = find(&lib, "lt4").unwrap();
        assert_eq!(added.tool.name, "6.35 mm upcut (2)");
        assert_eq!(added.tool.feed_mm_min, Some(1200.0));
        assert_eq!(added.source, None);
        // A third of the same name takes the next free suffix.
        let mut faster_still = tool_6_35_upcut();
        faster_still.feed_mm_min = Some(1400.0);
        add(&mut lib, faster_still, None).unwrap();
        assert_eq!(find(&lib, "lt5").unwrap().tool.name, "6.35 mm upcut (3)");
        // The source is not part of "same fields": the same tool from
        // another source is still already present.
        let again = tool_6_35_upcut();
        assert_eq!(
            add(&mut lib, again, Some("fusion:x:y".into())).unwrap(),
            AddOutcome::AlreadyPresent {
                id: LibraryToolId::new("lt1")
            }
        );
        // Ids are never reused: after removing lt5 the next is lt6.
        remove(&mut lib, "lt5").unwrap();
        let mut other = ball_bare();
        other.name = "Other".into();
        assert_eq!(
            add(&mut lib, other, None).unwrap(),
            AddOutcome::Added {
                id: LibraryToolId::new("lt6")
            }
        );
        assert_eq!(find(&lib, "lt6").unwrap().tool.name, "Other", "unique: no suffix");
    }

    /// An id match wins over a name match, a unique name finds its tool,
    /// and an unknown key is an error naming it.
    #[test]
    fn find_by_id_then_unique_name() {
        let mut lib = sample_library();
        // A tool named like another tool's id: the id wins.
        lib.tools[0].tool.name = "lt2".into();
        assert_eq!(find(&lib, "lt2").unwrap().id, LibraryToolId::new("lt2"));
        assert_eq!(find(&lib, "lt1").unwrap().tool.name, "lt2");
        let by_name = find(&lib, "90° V-bit").unwrap();
        assert_eq!(by_name.id, LibraryToolId::new("lt2"));
        assert_eq!(by_name.source.as_deref(), Some("fusion:guid-1:preset-a"));
        assert_eq!(
            find(&lib, "nothing").unwrap_err(),
            LibraryError::NotFound {
                key: "nothing".into()
            }
        );
        // Names match exactly, not by case or prefix.
        assert!(matches!(
            find(&lib, "90° v-bit"),
            Err(LibraryError::NotFound { .. })
        ));
        // `remove` finds the same way and gives the tool back.
        let removed = remove(&mut lib, "3 mm ball").unwrap();
        assert_eq!(removed.id, LibraryToolId::new("lt3"));
        assert_eq!(lib.tools.len(), 2);
        assert!(matches!(
            remove(&mut lib, "lt3"),
            Err(LibraryError::NotFound { .. })
        ));
    }

    /// Two tools with one name (possible in a hand-edited file): finding
    /// by that name is an error listing both, so the user picks an id.
    #[test]
    fn ambiguous_name_is_error() {
        let mut lib = sample_library();
        lib.tools[2].tool.name = "90° V-bit".into();
        let err = find(&lib, "90° V-bit").unwrap_err();
        assert_eq!(
            err,
            LibraryError::Ambiguous {
                key: "90° V-bit".into(),
                matches: vec!["`lt2`".into(), "`lt3`".into()],
            }
        );
        assert!(err.to_string().contains("lt2") && err.to_string().contains("lt3"));
        // By id, each is still reachable.
        assert_eq!(find(&lib, "lt3").unwrap().tool.cutter, ToolKind::BallNose);
    }

    /// Port of the `parseToolLibraryFile` rules checked by PureCutCNC's
    /// `src/toolLibrary.test.ts`: diameter > 0, flutes ≥ 1, step-over in
    /// (0, 1], V-bit angle in (0, 180). Upstream drops a failing entry;
    /// here `load` fails naming the tool and the field, and `add` refuses
    /// the tool. The boundary values that pass upstream pass here.
    #[test]
    fn validation_rules_from_upstream() {
        let cases: Vec<(&str, Box<dyn Fn(&mut Tool)>)> = vec![
            ("diameter_mm", Box::new(|t| t.diameter_mm = 0.0)),
            ("diameter_mm", Box::new(|t| t.diameter_mm = -1.0)),
            ("flutes", Box::new(|t| t.flutes = 0)),
            ("step_over_fraction", Box::new(|t| t.step_over_fraction = Some(0.0))),
            ("step_over_fraction", Box::new(|t| t.step_over_fraction = Some(1.5))),
            (
                "cutter.v_bit.included_angle_deg",
                Box::new(|t| {
                    t.cutter = ToolKind::VBit {
                        included_angle_deg: 0.0,
                    };
                }),
            ),
            (
                "cutter.v_bit.included_angle_deg",
                Box::new(|t| {
                    t.cutter = ToolKind::VBit {
                        included_angle_deg: 180.0,
                    };
                }),
            ),
            // DreamCarve's own: a set rate must be positive, a number ≥ 1,
            // a name non-empty.
            ("feed_mm_min", Box::new(|t| t.feed_mm_min = Some(0.0))),
            ("plunge_mm_min", Box::new(|t| t.plunge_mm_min = Some(-5.0))),
            ("rpm", Box::new(|t| t.rpm = Some(0))),
            ("step_down_mm", Box::new(|t| t.step_down_mm = Some(0.0))),
            ("number", Box::new(|t| t.number = Some(0))),
            ("name", Box::new(|t| t.name = "  ".into())),
        ];
        for (field, edit) in &cases {
            let mut lib = sample_library();
            let mut bad = tool_6_35_upcut();
            bad.name = "Bad".into();
            edit(&mut bad);
            // `add` refuses it…
            let err = add(&mut lib, bad.clone(), None).unwrap_err();
            assert!(
                matches!(&err, LibraryError::InvalidTool { id, name, field: f, .. }
                    if id.as_str() == "lt4" && name == "Bad" && f == field),
                "add with bad {field}: {err:?}"
            );
            assert_eq!(lib.tools.len(), 3, "nothing added for bad {field}");
            // …and so does `load` of a file holding it.
            lib.tools.push(LibraryTool {
                id: LibraryToolId::new("lt4"),
                source: None,
                tool: bad,
            });
            let text = save(&lib).unwrap();
            let err = load(text.as_bytes()).unwrap_err();
            assert!(
                matches!(&err, LibraryError::InvalidTool { id, name, field: f, .. }
                    if id.as_str() == "lt4" && name == "Bad" && f == field),
                "load with bad {field}: {err:?}"
            );
            let message = err.to_string();
            assert!(
                message.contains("`Bad`") && message.contains("`lt4`") && message.contains(field),
                "{message}"
            );
        }
        // The boundaries that pass upstream pass here.
        let mut lib = sample_library();
        let mut edge = tool_6_35_upcut();
        edge.name = "Edge".into();
        edge.step_over_fraction = Some(1.0);
        edge.cutter = ToolKind::VBit {
            included_angle_deg: 179.9,
        };
        edge.flutes = 1;
        assert!(matches!(
            add(&mut lib, edge, None).unwrap(),
            AddOutcome::Added { .. }
        ));
        let mut edge = tool_6_35_upcut();
        edge.name = "Edge 2".into();
        edge.cutter = ToolKind::VBit {
            included_angle_deg: 0.1,
        };
        assert!(matches!(
            add(&mut lib, edge, None).unwrap(),
            AddOutcome::Added { .. }
        ));
        assert_eq!(load(save(&lib).unwrap().as_bytes()).unwrap(), lib);
        // A NaN never reaches the file: `save` refuses it by name.
        lib.tools[0].tool.diameter_mm = f64::NAN;
        assert!(matches!(
            save(&lib).unwrap_err(),
            LibraryError::NonFinite { field, .. } if field == "diameter_mm"
        ));
    }

    /// A file from a newer DreamCarve is refused rather than misread, as
    /// are a file without a version, a non-object, invalid JSON, an
    /// unknown field, a missing optional, and a repeated id.
    #[test]
    fn newer_version_is_rejected() {
        assert_eq!(
            load(b"{\"version\": 2, \"tools\": []}").unwrap_err(),
            LibraryError::NewerVersion {
                found: 2,
                supported: LIBRARY_VERSION
            }
        );
        assert_eq!(
            load(b"{\"tools\": []}").unwrap_err(),
            LibraryError::MissingVersion
        );
        assert_eq!(
            load(b"{\"version\": \"1\", \"tools\": []}").unwrap_err(),
            LibraryError::InvalidVersion {
                found: "\"1\"".into()
            }
        );
        assert_eq!(
            load(b"{\"version\": 0, \"tools\": []}").unwrap_err(),
            LibraryError::InvalidVersion { found: "0".into() }
        );
        assert_eq!(load(b"[]").unwrap_err(), LibraryError::NotAnObject);
        assert!(matches!(
            load(b"{").unwrap_err(),
            LibraryError::Json { line: 1, .. }
        ));
        let err = load(b"{\"version\": 1, \"tools\": [], \"name\": \"x\"}").unwrap_err();
        assert!(
            matches!(&err, LibraryError::Schema { message, .. } if message.contains("name")),
            "{err:?}"
        );
        // The current version with no tools is the empty library.
        assert_eq!(
            load(b"{\"version\": 1, \"tools\": []}").unwrap(),
            Library::default()
        );

        // A tool block missing an optional, or with an unknown key, names
        // its path; a repeated id is an error too.
        let text = save(&sample_library()).unwrap();
        let without_source = text.replacen("      \"source\": null,\n", "", 1);
        let err = load(without_source.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LibraryError::Schema { path, message }
                if path.starts_with("tools[0]") && message.contains("source")),
            "{err:?}"
        );
        let with_extra = text.replacen("\"flutes\": 2,", "\"flutes\": 2, \"coating\": \"TiN\",", 1);
        let err = load(with_extra.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LibraryError::Schema { path, message }
                if path.starts_with("tools[0].tool") && message.contains("coating")),
            "{err:?}"
        );
        let repeated = text.replacen("\"id\": \"lt2\"", "\"id\": \"lt1\"", 1);
        assert_eq!(
            load(repeated.as_bytes()).unwrap_err(),
            LibraryError::DuplicateId {
                id: LibraryToolId::new("lt1")
            }
        );
        // The id inside `tool` is ignored: the outer one wins.
        // The inner id is the one followed by `name`; the outer by `source`.
        let inner = text.replacen("\"lt3\",\n        \"name\"", "\"t7\",\n        \"name\"", 1);
        assert_ne!(inner, text, "the inner id was edited");
        let lib = load(inner.as_bytes()).unwrap();
        assert_eq!(lib.tools[2].tool.id, ToolId::new("lt3"));
    }
}
