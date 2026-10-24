// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/toolLibrary.ts

//! The library and its file (`docs/specs/phase-2/design.md`, "Data model →
//! dc-library"; "Algorithms → Tool library CLI").
//!
//! The file is JSON: `{ "version": 2, "tools": [ … ] }`, each tool a block
//! with its library `id`, its `source` and the `tool` itself, saved exactly
//! as a `.dcv` saves a tool (every optional value written, `null` when
//! unset). The `id` inside `tool` is written for completeness and ignored
//! on load: the library id is the one outside it, and `tools add` replaces
//! it with a project id anyway.
//!
//! Versions: **1**, the first; **2** (maintainer decision 2026-10-24) adds
//! each tool's `vendor` and `product_id` after its `name`, as format 16
//! of a `.dcv` does. [`load`] migrates a version 1 file in memory, giving
//! every tool `null` for both, and [`save`] writes version 2.
//!
//! Names stay unique ([`add`]): a tool whose name is taken is stored as
//! `<name> (<product id>)` when it has a product id and that name is
//! free, else as `<name> (2)`, `(3)`, ….
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
pub const LIBRARY_VERSION: u32 = 2;

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

/// What [`add`] or [`replace`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AddOutcome {
    /// The tool was added under this id. Its name may have gained a
    /// ` (<product id>)` or ` (2)` suffix to stay unique; read it from the
    /// library.
    Added {
        /// The new tool's id.
        id: LibraryToolId,
    },
    /// A tool with the same fields (ignoring ids, and a suffix [`add`]
    /// gave its name) is already in the library; nothing changed.
    AlreadyPresent {
        /// The id of the existing tool.
        id: LibraryToolId,
    },
    /// [`replace`] overwrote the tool with the same source, keeping its id.
    Replaced {
        /// The id of the overwritten tool.
        id: LibraryToolId,
    },
}

/// Parses library bytes: JSON, `version` read and checked against
/// [`LIBRARY_VERSION`], a version 1 file migrated (module docs), then
/// deserialized strictly (unknown fields and
/// missing fields both fail), then ids checked for duplicates and every
/// tool validated ([`validate_tool`]). Each tool's own `id` is set to its
/// library id.
pub fn load(bytes: &[u8]) -> Result<Library, LibraryError> {
    let root: Value = serde_json::from_slice(bytes).map_err(|e| LibraryError::Json {
        line: e.line(),
        column: e.column(),
        message: e.to_string(),
    })?;
    let obj = root.as_object().ok_or(LibraryError::NotAnObject)?;
    let value = obj.get("version").ok_or(LibraryError::MissingVersion)?;
    let version = value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .filter(|&v| v >= 1)
        .ok_or_else(|| LibraryError::InvalidVersion {
            found: value.to_string(),
        })?;
    if version > LIBRARY_VERSION {
        return Err(LibraryError::NewerVersion {
            found: version,
            supported: LIBRARY_VERSION,
        });
    }
    let root = if version < 2 { v1_to_v2(root)? } else { root };
    let mut lib: Library = serde_path_to_error::deserialize(&root).map_err(|e| {
        let path = e.path().to_string();
        LibraryError::Schema {
            path: if path == "." { String::new() } else { path },
            message: e.inner().to_string(),
        }
    })?;
    for (k, entry) in lib.tools.iter().enumerate() {
        if lib.tools.iter().take(k).any(|other| other.id == entry.id) {
            return Err(LibraryError::DuplicateId {
                id: entry.id.clone(),
            });
        }
    }
    for entry in &mut lib.tools {
        entry.tool.id = ToolId::new(entry.id.as_str());
        validate_tool(&entry.id, &entry.tool)?;
    }
    Ok(lib)
}

/// Version 1 → 2: `vendor: null` and `product_id: null` right after each
/// `tools[].tool.name`, and `version` 2. A block that is not shaped as a
/// tool is left for the schema check to report; a tool that already has
/// either key is not a version 1 tool.
fn v1_to_v2(mut root: Value) -> Result<Value, LibraryError> {
    let migration = |message: String| LibraryError::Migration {
        from: 1,
        to: 2,
        message,
    };
    if let Some(tools) = root.get_mut("tools").and_then(Value::as_array_mut) {
        for (i, entry) in tools.iter_mut().enumerate() {
            let Some(tool) = entry.get_mut("tool") else {
                continue;
            };
            let Value::Object(fields) = std::mem::take(tool) else {
                continue;
            };
            for key in ["vendor", "product_id"] {
                if fields.contains_key(key) {
                    return Err(migration(format!(
                        "a version 1 library has no `tools[{i}].tool.{key}`, but this one does"
                    )));
                }
            }
            let mut out = serde_json::Map::with_capacity(fields.len() + 2);
            let mut inserted = false;
            for (key, value) in fields {
                let after_this = key == "name";
                out.insert(key, value);
                if after_this {
                    out.insert("vendor".into(), Value::Null);
                    out.insert("product_id".into(), Value::Null);
                    inserted = true;
                }
            }
            if !inserted {
                out.insert("vendor".into(), Value::Null);
                out.insert("product_id".into(), Value::Null);
            }
            *tool = Value::Object(out);
        }
    }
    if let Some(obj) = root.as_object_mut() {
        obj.insert("version".into(), Value::from(2));
    }
    Ok(root)
}

/// Serializes the library as its file text: pretty JSON, keys in
/// declaration order, one tool per block, every `Option` as `null`, ending
/// in a newline. Deterministic: the same library gives the same bytes.
/// Fails rather than writing a NaN or infinity, which serde_json would
/// silently turn into `null`.
pub fn save(lib: &Library) -> Result<String, LibraryError> {
    for entry in &lib.tools {
        if let Some(field) = non_finite_field(&entry.tool) {
            return Err(LibraryError::NonFinite {
                id: entry.id.clone(),
                name: entry.tool.name.clone(),
                field: field.into(),
            });
        }
    }
    let mut text = serde_json::to_string_pretty(lib).map_err(|e| LibraryError::Serialize {
        message: e.to_string(),
    })?;
    text.push('\n');
    Ok(text)
}

/// The first of `tool`'s numbers that is NaN or infinite, by field name.
fn non_finite_field(tool: &Tool) -> Option<&'static str> {
    let angle = match tool.cutter {
        ToolKind::VBit { included_angle_deg } => Some(included_angle_deg),
        _ => None,
    };
    [
        ("diameter_mm", Some(tool.diameter_mm)),
        ("cutter.v_bit.included_angle_deg", angle),
        ("feed_mm_min", tool.feed_mm_min),
        ("plunge_mm_min", tool.plunge_mm_min),
        ("step_down_mm", tool.step_down_mm),
        ("step_over_fraction", tool.step_over_fraction),
    ]
    .into_iter()
    .find_map(|(field, value)| value.filter(|v| !v.is_finite()).map(|_| field))
}

/// Checks the validation rules (module docs) on `tool`, which would be
/// stored under `id`; the error names both and the field.
pub fn validate_tool(id: &LibraryToolId, tool: &Tool) -> Result<(), LibraryError> {
    let invalid = |field: &str, message: String| LibraryError::InvalidTool {
        id: id.clone(),
        name: tool.name.clone(),
        field: field.into(),
        message,
    };
    if let Some(field) = non_finite_field(tool) {
        return Err(LibraryError::NonFinite {
            id: id.clone(),
            name: tool.name.clone(),
            field: field.into(),
        });
    }
    if tool.name.trim().is_empty() {
        return Err(invalid("name", "must not be empty".into()));
    }
    // Upstream: `diameter > 0`.
    if tool.diameter_mm <= 0.0 {
        return Err(invalid(
            "diameter_mm",
            format!("must be greater than 0, got {}", tool.diameter_mm),
        ));
    }
    // Upstream: `flutes >= 1`.
    if tool.flutes == 0 {
        return Err(invalid("flutes", "must be 1 or more, got 0".into()));
    }
    // Upstream: `vBitAngle > 0 && vBitAngle < 180`.
    if let ToolKind::VBit { included_angle_deg } = tool.cutter
        && !(included_angle_deg > 0.0 && included_angle_deg < 180.0)
    {
        return Err(invalid(
            "cutter.v_bit.included_angle_deg",
            format!("must be between 0 and 180 (exclusive), got {included_angle_deg}"),
        ));
    }
    // Upstream: `defaultStepover > 0 && defaultStepover <= 1`.
    if let Some(fraction) = tool.step_over_fraction
        && !(fraction > 0.0 && fraction <= 1.0)
    {
        return Err(invalid(
            "step_over_fraction",
            format!("must be greater than 0 and at most 1, got {fraction}"),
        ));
    }
    // Upstream requires these positive; here they may be unset, as in a
    // project, but a set value must be positive.
    for (field, value) in [
        ("feed_mm_min", tool.feed_mm_min),
        ("plunge_mm_min", tool.plunge_mm_min),
        ("step_down_mm", tool.step_down_mm),
    ] {
        if let Some(value) = value
            && value <= 0.0
        {
            return Err(invalid(
                field,
                format!("must be greater than 0, got {value}"),
            ));
        }
    }
    if tool.rpm == Some(0) {
        return Err(invalid("rpm", "must be greater than 0, got 0".into()));
    }
    if tool.number == Some(0) {
        return Err(invalid("number", "must be 1 or more, got 0".into()));
    }
    Ok(())
}

/// The prefix of library tool ids.
const ID_PREFIX: &str = "lt";

/// The id after the highest `lt<n>` in the library: `lt1` for an empty
/// one. Gaps left by [`remove`] are not refilled.
fn next_id(lib: &Library) -> LibraryToolId {
    let highest = lib
        .tools
        .iter()
        .filter_map(|t| t.id.as_str().strip_prefix(ID_PREFIX)?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    LibraryToolId::new(format!("{ID_PREFIX}{}", highest.saturating_add(1)))
}

/// Whether `stored`, a name in the library, is `name` or `name` with a
/// suffix [`add`] gives a name already taken: ` (<product id>)` or
/// ` (n)`.
fn is_name_or_suffixed(stored: &str, name: &str, product_id: Option<&str>) -> bool {
    stored == name
        || product_name(name, product_id).is_some_and(|with_id| stored == with_id)
        || stored
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix(" ("))
            .and_then(|rest| rest.strip_suffix(')'))
            .is_some_and(|n| n.parse::<u32>().is_ok_and(|n| n >= 2))
}

/// `<name> (<product id>)`, the first name [`add`] tries when `name` is
/// taken; `None` without a product id, or when `name` already ends with
/// it in brackets.
fn product_name(name: &str, product_id: Option<&str>) -> Option<String> {
    let id = product_id?;
    let with_id = format!("{name} ({id})");
    (!name.ends_with(&format!(" ({id})"))).then_some(with_id)
}

/// Whether `existing` is `tool`: equal in every field but the id, and in
/// the name up to a suffix [`add`] gives.
fn same_tool(existing: &Tool, tool: &Tool) -> bool {
    is_name_or_suffixed(&existing.name, &tool.name, tool.product_id.as_deref())
        && Tool {
            id: existing.id.clone(),
            name: existing.name.clone(),
            ..tool.clone()
        } == *existing
}

/// `name`, else `name (<product id>)`, else the first `name (n)` from 2
/// up, whichever no tool other than `except` has.
fn unique_name(
    lib: &Library,
    name: &str,
    product_id: Option<&str>,
    except: Option<&LibraryToolId>,
) -> String {
    let taken = |candidate: &str| {
        lib.tools
            .iter()
            .any(|t| Some(&t.id) != except && t.tool.name == candidate)
    };
    if !taken(name) {
        return name.to_owned();
    }
    if let Some(with_id) = product_name(name, product_id).filter(|n| !taken(n)) {
        return with_id;
    }
    (2_u32..)
        .map(|n| format!("{name} ({n})"))
        .find(|candidate| !taken(candidate))
        .unwrap_or_else(|| name.to_owned())
}

/// Adds `tool` to the library under the next `lt<n>` id, with `source`
/// saying where it came from. The tool's own `id` is ignored. When the
/// same tool (ids aside, and up to a ` (n)` name suffix) is already
/// there, whatever its source, nothing changes and the outcome names it.
/// When a different tool already has the same name, the new one is stored
/// as `<name> (<product id>)` when it has a product id and no tool has
/// that name, else as `<name> (2)` (then ` (3)`, …), so [`find`] by name
/// stays unambiguous. A tool that fails [`validate_tool`] is an error and is
/// not added.
pub fn add(
    lib: &mut Library,
    tool: Tool,
    source: Option<String>,
) -> Result<AddOutcome, LibraryError> {
    let id = next_id(lib);
    validate_tool(&id, &tool)?;
    if let Some(existing) = lib.tools.iter().find(|t| same_tool(&t.tool, &tool)) {
        return Ok(AddOutcome::AlreadyPresent {
            id: existing.id.clone(),
        });
    }
    let name = unique_name(lib, &tool.name, tool.product_id.as_deref(), None);
    lib.tools.push(LibraryTool {
        id: id.clone(),
        source,
        tool: Tool {
            id: ToolId::new(id.as_str()),
            name,
            ..tool
        },
    });
    Ok(AddOutcome::Added { id })
}

/// [`add`], except that a tool whose `source` is set and equal to that of
/// a library tool overwrites that tool in place, keeping its id, instead
/// of being added beside it (`tools import --replace`). The same tool is
/// still [`AddOutcome::AlreadyPresent`].
pub fn replace(
    lib: &mut Library,
    tool: Tool,
    source: Option<String>,
) -> Result<AddOutcome, LibraryError> {
    let index = source
        .as_ref()
        .and_then(|s| lib.tools.iter().position(|t| t.source.as_ref() == Some(s)));
    let Some(existing) = index.and_then(|index| lib.tools.get(index)) else {
        return add(lib, tool, source);
    };
    let id = existing.id.clone();
    validate_tool(&id, &tool)?;
    if same_tool(&existing.tool, &tool) {
        return Ok(AddOutcome::AlreadyPresent { id });
    }
    let name = unique_name(lib, &tool.name, tool.product_id.as_deref(), Some(&id));
    if let Some(slot) = index.and_then(|index| lib.tools.get_mut(index)) {
        slot.tool = Tool {
            id: ToolId::new(id.as_str()),
            name,
            ..tool
        };
    }
    Ok(AddOutcome::Replaced { id })
}

/// The tool whose id is `key`, else the one tool whose name is `key`. A
/// name shared by several tools is [`LibraryError::Ambiguous`], listing
/// them; no match is [`LibraryError::NotFound`].
pub fn find<'a>(lib: &'a Library, key: &str) -> Result<&'a LibraryTool, LibraryError> {
    let index = position(lib, key)?;
    lib.tools.get(index).ok_or_else(|| LibraryError::NotFound {
        key: key.to_owned(),
    })
}

/// The index of the tool [`find`] returns.
fn position(lib: &Library, key: &str) -> Result<usize, LibraryError> {
    if let Some(index) = lib.tools.iter().position(|t| t.id.as_str() == key) {
        return Ok(index);
    }
    let named: Vec<&LibraryTool> = lib.tools.iter().filter(|t| t.tool.name == key).collect();
    match named.as_slice() {
        [] => Err(LibraryError::NotFound {
            key: key.to_owned(),
        }),
        [only] => lib
            .tools
            .iter()
            .position(|t| t.id == only.id)
            .ok_or_else(|| LibraryError::NotFound {
                key: key.to_owned(),
            }),
        _ => Err(LibraryError::Ambiguous {
            key: key.to_owned(),
            matches: named.iter().map(|t| format!("`{}`", t.id)).collect(),
        }),
    }
}

/// Removes and returns the tool [`find`] would return for `key`.
pub fn remove(lib: &mut Library, key: &str) -> Result<LibraryTool, LibraryError> {
    let index = position(lib, key)?;
    Ok(lib.tools.remove(index))
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
            vendor: Some("Acme Router Bits".into()),
            product_id: Some("AC-V90".into()),
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
            vendor: None,
            product_id: None,
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
        assert!(
            text.starts_with("{\n  \"version\": 2,\n  \"tools\": [\n"),
            "{text}"
        );
        assert!(text.ends_with("}\n"), "{text}");
        assert!(text.contains("\"source\": null"), "{text}");
        assert!(text.contains("\"step_down_mm\": null"), "{text}");
        // A tool's vendor and product id follow its name, set or unset.
        assert!(
            text.contains(
                "\"name\": \"90° V-bit\",\n        \"vendor\": \"Acme Router Bits\",\n        \"product_id\": \"AC-V90\",\n"
            ),
            "{text}"
        );
        assert!(
            text.contains(
                "\"name\": \"3 mm ball\",\n        \"vendor\": null,\n        \"product_id\": null,\n"
            ),
            "{text}"
        );
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
        // A suffixed copy is the same tool too: re-adding the faster tool
        // under its original name is already present as lt4.
        let mut faster_again = tool_6_35_upcut();
        faster_again.feed_mm_min = Some(1200.0);
        assert_eq!(
            add(&mut lib, faster_again, None).unwrap(),
            AddOutcome::AlreadyPresent {
                id: LibraryToolId::new("lt4")
            }
        );
        // New ids follow the highest one, so a gap is never refilled:
        // after removing lt2 the next is lt6.
        remove(&mut lib, "lt2").unwrap();
        let mut other = ball_bare();
        other.name = "Other".into();
        assert_eq!(
            add(&mut lib, other, None).unwrap(),
            AddOutcome::Added {
                id: LibraryToolId::new("lt6")
            }
        );
        assert_eq!(
            find(&lib, "lt6").unwrap().tool.name,
            "Other",
            "unique: no suffix"
        );
    }

    /// A library file as version 1 wrote it: `lib` saved, then each tool's
    /// `vendor` and `product_id` taken out and the version set to 1.
    fn as_version_1(lib: &Library) -> String {
        let mut root: Value = serde_json::from_str(&save(lib).unwrap()).unwrap();
        root["version"] = 1.into();
        for entry in root["tools"].as_array_mut().unwrap() {
            let tool = entry["tool"].as_object_mut().unwrap();
            tool.shift_remove("vendor");
            tool.shift_remove("product_id");
        }
        let mut text = serde_json::to_string_pretty(&root).unwrap();
        text.push('\n');
        text
    }

    /// Maintainer decision 2026-10-24: a version 1 library, which has no
    /// vendor or product id, loads with both unset and saves as version
    /// 2 with them after each name. A version 1 tool that has either key
    /// is not a version 1 tool.
    #[test]
    fn version_1_file_migrates() {
        let mut lib = sample_library();
        for entry in &mut lib.tools {
            entry.tool.vendor = None;
            entry.tool.product_id = None;
        }
        let v1 = as_version_1(&lib);
        assert!(v1.starts_with("{\n  \"version\": 1,"), "{v1}");
        assert!(!v1.contains("vendor") && !v1.contains("product_id"), "{v1}");
        let loaded = load(v1.as_bytes()).unwrap();
        assert_eq!(loaded, lib);
        assert_eq!(loaded.version, LIBRARY_VERSION);
        let saved = save(&loaded).unwrap();
        assert!(saved.starts_with("{\n  \"version\": 2,"), "{saved}");
        assert_eq!(saved.matches("\"vendor\": null,").count(), 3, "{saved}");
        assert_eq!(saved.matches("\"product_id\": null,").count(), 3, "{saved}");
        assert_eq!(saved, save(&lib).unwrap());

        for key in ["vendor", "product_id"] {
            let bad = v1.replacen(
                "\"name\": \"90° V-bit\",",
                &format!("\"name\": \"90° V-bit\", \"{key}\": null,"),
                1,
            );
            assert_ne!(bad, v1);
            let err = load(bad.as_bytes()).unwrap_err();
            assert!(
                matches!(&err, LibraryError::Migration { message, .. }
                    if message.contains(&format!("tools[1].tool.{key}"))),
                "{key}: {err:?}"
            );
        }
        // A version 2 tool missing them is a schema error, as any missing
        // field is.
        let missing = save(&lib)
            .unwrap()
            .replacen("        \"vendor\": null,\n", "", 1);
        assert!(matches!(load(missing.as_bytes()).unwrap_err(),
                LibraryError::Schema { message, .. } if message.contains("vendor")),);
    }

    /// Maintainer decision 2026-10-24: a clashing name takes the tool's
    /// product id before ` (2)`, and the same tool under either suffix is
    /// already present.
    #[test]
    fn name_clash_takes_product_id_first() {
        let engraver = |diameter_mm: f64, product_id: Option<&str>| Tool {
            name: "Acme Diamond Engraver".into(),
            vendor: Some("Acme".into()),
            product_id: product_id.map(str::to_owned),
            diameter_mm,
            ..vbit_90()
        };
        let mut lib = Library::default();
        add(&mut lib, engraver(12.7, Some("ED-12")), None).unwrap();
        add(&mut lib, engraver(6.35, Some("ED-14")), None).unwrap();
        let names: Vec<&str> = lib.tools.iter().map(|t| t.tool.name.as_str()).collect();
        assert_eq!(
            names,
            ["Acme Diamond Engraver", "Acme Diamond Engraver (ED-14)"]
        );
        // Again: already present, whatever the stored suffix.
        for (diameter_mm, id, expected) in [(12.7, "ED-12", "lt1"), (6.35, "ED-14", "lt2")] {
            assert_eq!(
                add(&mut lib, engraver(diameter_mm, Some(id)), None).unwrap(),
                AddOutcome::AlreadyPresent {
                    id: LibraryToolId::new(expected)
                }
            );
        }
        // No product id: ` (2)`.
        add(&mut lib, engraver(3.0, None), None).unwrap();
        assert_eq!(lib.tools[2].tool.name, "Acme Diamond Engraver (2)");
        // A product id whose name is taken: the next free ` (n)`.
        add(&mut lib, engraver(4.0, Some("ED-14")), None).unwrap();
        assert_eq!(lib.tools[3].tool.name, "Acme Diamond Engraver (3)");
        // A name that already ends with the product id gets ` (n)`, not
        // the id twice.
        let mut by_id = engraver(5.0, Some("ED-14"));
        by_id.name = "Acme Diamond Engraver (ED-14)".into();
        add(&mut lib, by_id, None).unwrap();
        assert_eq!(lib.tools[4].tool.name, "Acme Diamond Engraver (ED-14) (2)");
        // `find` takes the new names.
        assert_eq!(
            find(&lib, "Acme Diamond Engraver (ED-14)").unwrap().id,
            LibraryToolId::new("lt2")
        );
        // `replace` settles a clash the same way.
        let mut replacing = Library::default();
        add(
            &mut replacing,
            engraver(12.7, Some("ED-12")),
            Some("s1".into()),
        )
        .unwrap();
        add(
            &mut replacing,
            engraver(6.0, Some("ED-14")),
            Some("s2".into()),
        )
        .unwrap();
        assert_eq!(
            replacing.tools[1].tool.name,
            "Acme Diamond Engraver (ED-14)"
        );
        assert_eq!(
            replace(
                &mut replacing,
                engraver(6.35, Some("ED-14")),
                Some("s2".into())
            )
            .unwrap(),
            AddOutcome::Replaced {
                id: LibraryToolId::new("lt2")
            }
        );
        assert_eq!(
            replacing.tools[1].tool.name,
            "Acme Diamond Engraver (ED-14)"
        );
    }

    /// Maintainer decision 2026-10-24: load never rounds. A stored value
    /// may be one the user typed, and the file round-trips byte for byte.
    #[test]
    #[allow(
        clippy::float_cmp,
        reason = "a rounded value must be exactly the decimal"
    )]
    fn load_keeps_stored_values() {
        let mut lib = Library::default();
        let mut noisy = tool_6_35_upcut();
        noisy.diameter_mm = 4.762_499_999_999_999;
        noisy.feed_mm_min = Some(1_524.000_000_000_000_5);
        add(&mut lib, noisy, None).unwrap();
        let text = save(&lib).unwrap();
        assert!(
            text.contains("\"diameter_mm\": 4.762499999999999,"),
            "{text}"
        );
        assert!(
            text.contains("\"feed_mm_min\": 1524.0000000000005,"),
            "{text}"
        );
        let loaded = load(text.as_bytes()).unwrap();
        assert!(loaded.tools[0].tool.diameter_mm == 4.762_499_999_999_999);
        assert_eq!(save(&loaded).unwrap(), text);
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
        /// One edit that breaks one rule.
        type Edit = Box<dyn Fn(&mut Tool)>;
        let cases: Vec<(&str, Edit)> = vec![
            ("diameter_mm", Box::new(|t| t.diameter_mm = 0.0)),
            ("diameter_mm", Box::new(|t| t.diameter_mm = -1.0)),
            ("flutes", Box::new(|t| t.flutes = 0)),
            (
                "step_over_fraction",
                Box::new(|t| t.step_over_fraction = Some(0.0)),
            ),
            (
                "step_over_fraction",
                Box::new(|t| t.step_over_fraction = Some(1.5)),
            ),
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
            // The name the error gives: the edited one in the `name` case.
            let bad_name = bad.name.clone();
            // `add` refuses it…
            let err = add(&mut lib, bad.clone(), None).unwrap_err();
            assert!(
                matches!(&err, LibraryError::InvalidTool { id, name, field: f, .. }
                    if id.as_str() == "lt4" && *name == bad_name && f == field),
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
                    if id.as_str() == "lt4" && *name == bad_name && f == field),
                "load with bad {field}: {err:?}"
            );
            let message = err.to_string();
            assert!(
                message.contains(&format!("`{bad_name}`"))
                    && message.contains("`lt4`")
                    && message.contains(field),
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
            load(b"{\"version\": 3, \"tools\": []}").unwrap_err(),
            LibraryError::NewerVersion {
                found: 3,
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
        let err = load(b"{\"version\": 2, \"tools\": [], \"name\": \"x\"}").unwrap_err();
        assert!(
            matches!(&err, LibraryError::Schema { message, .. } if message.contains("name")),
            "{err:?}"
        );
        // The current version with no tools is the empty library, and so
        // is version 1 with none.
        for version in [1, 2] {
            let text = format!("{{\"version\": {version}, \"tools\": []}}");
            assert_eq!(load(text.as_bytes()).unwrap(), Library::default());
        }

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
