// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `.dcv` load and save (ADR-0002; `docs/specs/phase-1/design.md`, "Data
//! model → dc-project").
//!
//! [`save`] writes pretty-printed JSON with keys in declaration order, the
//! settings first and the geometry last, each vertex on one line
//! ([`crate::layout`]), and a trailing newline. [`load`] parses bytes, reads `version`, migrates to
//! [`crate::CURRENT_VERSION`], deserializes strictly (unknown fields and missing
//! fields both fail), and then validates. A schema error names the path to
//! the bad value and, when the file has a value there, its line and column
//! (#65).
//!
//! `load` fails only when the bytes cannot become a [`Project`]. Validation
//! problems, errors included, come back beside the project in [`Loaded`] so
//! a caller such as the CLI `check` command can report all of them; callers
//! that machine the project must refuse it when [`Loaded::has_errors`].

use crate::layout::DcvFormatter;
use crate::locate::{PathSegment, SourceMap, TextPosition};
use crate::migrate::migrate;
use crate::model::{BoreOp, DrillOp, EngraveOp, PocketOp, ProfileOp, Project};
use crate::post_processor::PostProcessorError;
use crate::validate::{Problem, validate};
use serde::Serialize;
use serde_json::Value;
use serde_path_to_error::Segment;

/// Why bytes could not be loaded as a project.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LoadError {
    /// The bytes are not valid JSON. NaN and infinite numbers fail here,
    /// since JSON cannot express them.
    #[error("file is not valid JSON at line {line}, column {column}: {message}")]
    Json {
        /// 1-based line of the error.
        line: usize,
        /// 1-based column of the error.
        column: usize,
        /// The parser's description.
        message: String,
    },
    /// The JSON root is not an object.
    #[error("file is not a DreamCarve project: the top level is not a JSON object")]
    NotAnObject,
    /// The root object has no `version` field.
    #[error("file has no `version` field")]
    MissingVersion,
    /// `version` is not a non-negative integer that fits in 32 bits.
    #[error("file version `{found}` is not a valid version number")]
    InvalidVersion {
        /// The `version` value as written.
        found: String,
    },
    /// The file was written by a newer DreamCarve.
    #[error("file version {found} is newer than this build supports (version {supported})")]
    NewerVersion {
        /// The file's version.
        found: u32,
        /// The newest version this build reads.
        supported: u32,
    },
    /// A migration step could not rewrite the file.
    #[error("migration from version {from} to {to} failed{}: {message}", schema_where("", *at))]
    Migration {
        /// Version migrated from.
        from: u32,
        /// Version migrated to.
        to: u32,
        /// What went wrong.
        message: String,
        /// Where in the file's text the value the message names first
        /// is (`operations[0].last_pass`, in backquotes), when [`load`]
        /// read it from text and the file has that value. `None` from
        /// [`crate::migrate`](fn@crate::migrate), which has no text.
        at: Option<TextPosition>,
    },
    /// The JSON does not match the project schema (unknown or missing
    /// field, wrong type).
    #[error("file does not match the project format{}: {message}", schema_where(path, *at))]
    Schema {
        /// Where in the project the mismatch is, as keys and indices:
        /// `operations[0].tabs.placement`. Empty for the top level.
        path: String,
        /// Where in the file's text the mismatch is, when the file has a
        /// value at `path`.
        at: Option<TextPosition>,
        /// serde's description, naming the offending field.
        message: String,
    },
    /// A number is NaN or infinite.
    #[error("{field} is not a finite number")]
    NonFinite {
        /// Path to the field.
        field: String,
    },
}

/// Why a project could not be saved.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SaveError {
    /// A number is NaN or infinite, which JSON cannot hold.
    #[error("{field} is not a finite number and cannot be saved")]
    NonFinite {
        /// Path to the field.
        field: String,
    },
    /// The serializer failed.
    #[error("project could not be serialized: {message}")]
    Serialize {
        /// serde's description.
        message: String,
    },
}

/// A successfully parsed project and everything [`validate`] found in it.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    /// The project, migrated to [`crate::CURRENT_VERSION`].
    pub project: Project,
    /// Validation problems, errors and warnings.
    pub problems: Vec<Problem>,
}

impl Loaded {
    /// Whether any problem is error-class, so the project must not be
    /// machined until it is fixed.
    pub fn has_errors(&self) -> bool {
        self.problems.iter().any(Problem::is_error)
    }
}

/// Serializes `project` as `.dcv` text: pretty JSON, keys in declaration
/// order, each vertex on one line, `Option` fields as `null`, ending in a
/// newline.
///
/// Fails with [`SaveError::NonFinite`] rather than writing a NaN or
/// infinity, which serde_json would silently turn into `null`.
pub fn save(project: &Project) -> Result<String, SaveError> {
    if let Some(field) = first_non_finite(&validate(project)) {
        return Err(SaveError::NonFinite { field });
    }
    let mut bytes = Vec::new();
    let mut serializer =
        serde_json::Serializer::with_formatter(&mut bytes, DcvFormatter::default());
    project
        .serialize(&mut serializer)
        .map_err(|e| SaveError::Serialize {
            message: e.to_string(),
        })?;
    bytes.push(b'\n');
    String::from_utf8(bytes).map_err(|e| SaveError::Serialize {
        message: e.to_string(),
    })
}

/// Parses `.dcv` bytes into a project, migrating older versions and then
/// validating. See the module docs for which failures are errors here and
/// which are returned as [`Loaded::problems`].
pub fn load(bytes: &[u8]) -> Result<Loaded, LoadError> {
    let root: Value = serde_json::from_slice(bytes).map_err(|e| LoadError::Json {
        line: e.line(),
        column: e.column(),
        message: e.to_string(),
    })?;
    let version = read_version(&root)?;
    let root = migrate(root, version).map_err(|err| locate_migration(err, bytes))?;
    let project = deserialize_project(&root, bytes)?;
    check_post_processor(&project, bytes)?;
    let problems = validate(&project);
    // serde_json rejects NaN and out-of-range numbers while parsing, so this
    // is a backstop for the "reject non-finite at the boundary" rule.
    if let Some(field) = first_non_finite(&problems) {
        return Err(LoadError::NonFinite { field });
    }
    Ok(Loaded { project, problems })
}

/// `err` with the position of the first field its message names, in
/// backquotes as `operations[0].last_pass`, when it is a
/// [`LoadError::Migration`] and `bytes` has a value there (#310 review),
/// as [`LoadError::Schema`] gives one.
fn locate_migration(err: LoadError, bytes: &[u8]) -> LoadError {
    let LoadError::Migration {
        from,
        to,
        message,
        at: None,
    } = err
    else {
        return err;
    };
    let at = message.split('`').skip(1).step_by(2).find_map(|field| {
        std::str::from_utf8(bytes)
            .ok()
            .and_then(|text| SourceMap::parse(text).ok())
            .and_then(|map| map.locate(field))
    });
    LoadError::Migration {
        from,
        to,
        message,
        at,
    }
}

/// The serde error that stopped a deserialization, with the path to it.
type PathError = serde_path_to_error::Error<serde_json::Error>;

/// Deserializes the migrated `root` strictly. On a mismatch, says where it
/// is (#65): the path from serde_path_to_error, and the line and column of
/// that path in `bytes`, the file as the user has it, found with
/// [`SourceMap`]. The same lookup serves a file at the current version and
/// a migrated one; a path only the migration created has no position.
fn deserialize_project(root: &Value, bytes: &[u8]) -> Result<Project, LoadError> {
    let err = match serde_path_to_error::deserialize::<_, Project>(root) {
        Ok(project) => return Ok(project),
        Err(err) => err,
    };
    let mut path = path_segments(&err);
    let mut message = err.inner().to_string();
    if let Some(inner) = operation_error(root, &path) {
        path.extend(path_segments(&inner));
        message = inner.inner().to_string();
    }
    let at = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| SourceMap::parse(text).ok())
        .and_then(|map| schema_error_position(&map, &path, &message));
    Err(LoadError::Schema {
        path: path_text(&path),
        at,
        message,
    })
}

/// A `post_processor` that names no bundled definition version is a
/// schema error at its `id` or `definition_version` (#106): the file is
/// well formed, but nothing can export it. The message lists what is
/// bundled.
fn check_post_processor(project: &Project, bytes: &[u8]) -> Result<(), LoadError> {
    let err = match project.post_processor.definition() {
        Ok(_) => return Ok(()),
        Err(err) => err,
    };
    let mut path = vec![PathSegment::Key("post_processor".into())];
    match err {
        PostProcessorError::UnknownId { .. } => path.push(PathSegment::Key("id".into())),
        PostProcessorError::UnknownVersion { .. } => {
            path.push(PathSegment::Key("definition_version".into()));
        }
        PostProcessorError::BundledDefinition { .. } => {}
    }
    let at = std::str::from_utf8(bytes)
        .ok()
        .and_then(|text| SourceMap::parse(text).ok())
        .and_then(|map| map.position(&path));
    Err(LoadError::Schema {
        path: path_text(&path),
        at,
        message: err.to_string(),
    })
}

/// The path of `err` as [`PathSegment`]s, up to the first step that is
/// not a key or an index.
fn path_segments(err: &PathError) -> Vec<PathSegment> {
    err.path()
        .iter()
        .map_while(|segment| match segment {
            Segment::Map { key } => Some(PathSegment::Key(key.clone())),
            // An externally tagged variant is the key of a one-key object.
            Segment::Enum { variant } => Some(PathSegment::Key(variant.clone())),
            Segment::Seq { index } => Some(PathSegment::Index(*index)),
            Segment::Unknown => None,
        })
        .collect()
}

/// serde reads an internally tagged enum such as
/// [`Operation`](crate::model::Operation) whole before
/// it picks the variant, so serde_path_to_error's path stops at
/// `operations[i]`. Reading that operation's fields again as its variant's
/// struct gives the error with the rest of the path. `None` when `path` is
/// not an operation or the error is not inside its fields.
fn operation_error(root: &Value, path: &[PathSegment]) -> Option<PathError> {
    let [PathSegment::Key(key), PathSegment::Index(i)] = path else {
        return None;
    };
    if key != "operations" {
        return None;
    }
    let mut fields = root.get(key)?.get(*i)?.as_object()?.clone();
    let kind = fields.shift_remove("kind")?;
    let fields = Value::Object(fields);
    // One arm per `Operation` variant, by its `kind`.
    match kind.as_str()? {
        "profile" => serde_path_to_error::deserialize::<_, ProfileOp>(&fields).err(),
        "pocket" => serde_path_to_error::deserialize::<_, PocketOp>(&fields).err(),
        "drill" => serde_path_to_error::deserialize::<_, DrillOp>(&fields).err(),
        "bore" => serde_path_to_error::deserialize::<_, BoreOp>(&fields).err(),
        "engrave" => serde_path_to_error::deserialize::<_, EngraveOp>(&fields).err(),
        _ => None,
    }
}

/// Where the value at `path` starts in the file. An unknown field inside
/// an internally tagged enum is not in the path (see [`operation_error`]),
/// so when `message` names one that the object at `path` has, the
/// position is that field's.
fn schema_error_position(
    map: &SourceMap,
    path: &[PathSegment],
    message: &str,
) -> Option<TextPosition> {
    let unknown = message
        .strip_prefix("unknown field `")
        .and_then(|rest| rest.split('`').next());
    if let Some(field) = unknown {
        let mut field_path = path.to_vec();
        field_path.push(PathSegment::Key(field.to_owned()));
        if let Some(at) = map.position(&field_path) {
            return Some(at);
        }
    }
    map.position(path)
}

/// `path` written as keys and indices: `operations[0].tabs.placement`.
fn path_text(path: &[PathSegment]) -> String {
    let mut out = String::new();
    for segment in path {
        match segment {
            PathSegment::Key(key) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(key);
            }
            PathSegment::Index(i) => out.push_str(&format!("[{i}]")),
        }
    }
    out
}

/// ` at `path` (line L, column C)`, as much of it as is known, for
/// [`LoadError::Schema`].
fn schema_where(path: &str, at: Option<TextPosition>) -> String {
    let mut out = String::new();
    if !path.is_empty() {
        out.push_str(&format!(" at `{path}`"));
    }
    if let Some(at) = at {
        out.push_str(&format!(" (line {}, column {})", at.line, at.column));
    }
    out
}

/// Reads the root object's `version` without interpreting anything else.
fn read_version(root: &Value) -> Result<u32, LoadError> {
    let obj = root.as_object().ok_or(LoadError::NotAnObject)?;
    let value = obj.get("version").ok_or(LoadError::MissingVersion)?;
    value
        .as_u64()
        .and_then(|v| u32::try_from(v).ok())
        .ok_or_else(|| LoadError::InvalidVersion {
            found: value.to_string(),
        })
}

fn first_non_finite(problems: &[Problem]) -> Option<String> {
    problems.iter().find_map(|p| match p {
        Problem::NonFinite { field } => Some(field.clone()),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        ClearanceStrategy, DrillCycle, Entry, Operation, OriginXy, PeckRetract, ToolKind,
    };
    use crate::test_support::{
        profile_mut, sample_drill, sample_engrave, sample_pocket, sample_project,
    };
    use crate::validate::validate;
    use serde_json::json;

    /// The sample project as this build saves it.
    const CURRENT_FIXTURE: &str = include_str!("../tests/fixtures/dcv/v19/sample.dcv");
    const V1_FIXTURE: &str = include_str!("../tests/fixtures/dcv/v1/sample.dcv");
    /// The sample in the layout before #76: geometry first, and each vertex
    /// over five lines.
    const V3_FIXTURE: &str = include_str!("../tests/fixtures/dcv/v3/sample.dcv");
    const V4_GEOMETRY_FIRST_FIXTURE: &str =
        include_str!("../tests/fixtures/dcv/v4/sample-geometry-first.dcv");

    #[test]
    fn round_trip_is_identity() {
        let p = sample_project();
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
        assert_eq!(loaded.problems, vec![]);
    }

    #[test]
    fn round_trip_keeps_unset_options() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = None;
        p.tools[0].rpm = None;
        p.stock.thickness_mm = None;
        let op = profile_mut(&mut p);
        op.tabs = None;
        op.depth_mm = None;
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
    }

    /// Addendum A6: a value written with its unit is read as millimetres,
    /// and a save writes the millimetre number back, not the string.
    #[test]
    fn save_writes_numbers_after_unit_input() {
        let mut root = serde_json::to_value(sample_project()).unwrap();
        root["operations"][0]["depth_mm"] = json!("0.5 in");
        root["tools"][0]["feed_mm_min"] = json!("60 in/min");
        let text = serde_json::to_string_pretty(&root).unwrap();
        let saved = save(&load(text.as_bytes()).unwrap().project).unwrap();
        assert!(saved.contains("\"depth_mm\": 12.7,"), "{saved}");
        assert!(saved.contains("\"feed_mm_min\": 1524.0,"), "{saved}");
        assert!(!saved.contains(" in"), "{saved}");
        // And the saved numbers load back to the same project.
        let reloaded = load(saved.as_bytes()).unwrap().project;
        assert_eq!(save(&reloaded).unwrap(), saved);
    }

    /// #105: placed tabs save after the operations, every field in
    /// declaration order, and load back equal.
    #[test]
    fn placed_tabs_round_trip() {
        let mut p = sample_project();
        profile_mut(&mut p).tabs = None;
        p.tabs = vec![
            crate::test_support::sample_tab("tab1", 25.0, -3.175),
            crate::PlacedTab {
                shape: crate::TabShape::Smooth,
                height_mm: 3.5,
                ..crate::test_support::sample_tab("tab2", 50.0, 15.0)
            },
        ];
        let text = save(&p).unwrap();
        let compact: String = text.split_whitespace().collect();
        assert!(
            compact.contains(
                "\"tabs\":[{\"id\":\"tab1\",\"instance\":\"i1\",\"profile\":0,\"x_mm\":25.0,\"y_mm\":-3.175,\"width_mm\":6.0,\"height_mm\":2.0,\"shape\":\"rect\",\"holds_slug\":false}"
            ),
            "{text}"
        );
        assert!(
            text.find("\"operations\"") < text.find("\"tabs\": [")
                && text.find("\"tabs\": [") < text.find("\"instances\""),
            "{text}"
        );
        let loaded = load(text.as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
        assert_eq!(loaded.problems, vec![]);
    }

    #[test]
    fn keys_are_stable() {
        assert_eq!(save(&sample_project()).unwrap(), CURRENT_FIXTURE);
    }

    #[test]
    fn crlf_line_endings_load() {
        // A file edited on Windows may come back with CRLF line endings.
        let crlf = CURRENT_FIXTURE.replace('\n', "\r\n");
        let loaded = load(crlf.as_bytes()).unwrap();
        assert_eq!(loaded.project, sample_project());
    }

    #[test]
    fn committed_v1_fixture_loads() {
        let loaded = load(V1_FIXTURE.as_bytes()).unwrap();
        assert_eq!(loaded.project, sample_project());
    }

    #[test]
    fn committed_current_fixture_loads() {
        let loaded = load(CURRENT_FIXTURE.as_bytes()).unwrap();
        assert_eq!(loaded.project, sample_project());
    }

    #[test]
    fn option_fields_serialize_as_null() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = None;
        p.tools[0].step_over_fraction = None;
        p.stock.thickness_mm = None;
        let op = profile_mut(&mut p);
        op.tabs = None;
        op.depth_mm = None;
        let text = save(&p).unwrap();
        assert!(text.contains("\"thickness_mm\": null"), "{text}");
        assert!(text.contains("\"depth_mm\": null"), "{text}");
        assert!(text.contains("\"feed_mm_min\": null"), "{text}");
        assert!(text.contains("\"step_over_fraction\": null"), "{text}");
        assert!(text.contains("\"step_down_mm\": null"), "{text}");
        assert!(text.contains("\"tabs\": null"), "{text}");
    }

    #[test]
    fn missing_option_field_is_rejected() {
        let text = CURRENT_FIXTURE.replacen("      \"feed_mm_min\": 1000.0,\n", "", 1);
        assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { message, .. } if message.contains("feed_mm_min")),
            "{err:?}"
        );
    }

    /// #95, #65: a current-version file without `tool_change_mode` is
    /// rejected, naming the field. Only an older file gets it, from the
    /// migration.
    #[test]
    fn missing_tool_change_mode_is_rejected() {
        let text =
            CURRENT_FIXTURE.replacen("    \"tool_change_mode\": \"separate_programs\",\n", "", 1);
        assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { message, .. }
                if message.contains("tool_change_mode")),
            "{err:?}"
        );
    }

    /// #99: an instance whose depths are unset saves both as `null` and
    /// loads back equal.
    #[test]
    fn instance_depths_round_trip_as_null() {
        let mut p = sample_project();
        p.instances[0].z_top_mm = None;
        p.instances[0].z_bottom_mm = None;
        let text = save(&p).unwrap();
        assert!(text.contains("\"z_top_mm\": null"), "{text}");
        assert!(text.contains("\"z_bottom_mm\": null"), "{text}");
        let loaded = load(text.as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
    }

    /// #99, #65: the instance depths may be `null`, but the keys stay
    /// required. A file without one fails to load, naming the instance and
    /// the missing key.
    #[test]
    fn missing_instance_z_top_fails_to_load() {
        let cases = [
            (
                "      \"z_top_mm\": 6.0,\n      \"z_bottom_mm\": 0.0\n",
                "      \"z_bottom_mm\": 0.0\n",
                "z_top_mm",
            ),
            (
                "      \"z_top_mm\": 6.0,\n      \"z_bottom_mm\": 0.0\n",
                "      \"z_top_mm\": 6.0\n",
                "z_bottom_mm",
            ),
        ];
        for (from, to, key) in cases {
            let text = CURRENT_FIXTURE.replacen(from, to, 1);
            assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
            let err = load(text.as_bytes()).unwrap_err();
            assert!(
                matches!(&err, LoadError::Schema { path, at, message }
                    if path == "instances[0]"
                        && at.is_some()
                        && message.contains(&format!("missing field `{key}`"))),
                "{err:?}"
            );
            let shown = err.to_string();
            assert!(shown.contains("`instances[0]`"), "{shown}");
            assert!(shown.contains(key), "{shown}");
        }
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        let text = CURRENT_FIXTURE.replacen("{\n", "{\n  \"colour\": \"red\",\n", 1);
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { message, .. } if message.contains("colour")),
            "{err:?}"
        );
    }

    #[test]
    fn unknown_nested_field_is_rejected() {
        let text = CURRENT_FIXTURE.replacen(
            "\"thickness_mm\": 6.0",
            "\"thickness_mm\": 6.0,\n    \"grain\": \"oak\"",
            1,
        );
        assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { message, .. } if message.contains("grain")),
            "{err:?}"
        );
    }

    #[test]
    fn unknown_operation_field_is_rejected() {
        let text = CURRENT_FIXTURE.replacen(
            "\"safe_z_mm\": 5.0",
            "\"safe_z_mm\": 5.0,\n      \"ramp\": true",
            1,
        );
        assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { message, .. } if message.contains("ramp")),
            "{err:?}"
        );
    }

    #[test]
    fn nan_literal_is_rejected() {
        let text = CURRENT_FIXTURE.replacen("\"width_mm\": 100.0", "\"width_mm\": NaN", 1);
        assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
        assert!(matches!(load(text.as_bytes()), Err(LoadError::Json { .. })));
    }

    #[test]
    fn infinite_number_is_rejected() {
        let text = CURRENT_FIXTURE.replacen("\"width_mm\": 100.0", "\"width_mm\": 1e999", 1);
        assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
        assert!(matches!(load(text.as_bytes()), Err(LoadError::Json { .. })));
    }

    #[test]
    fn saving_nan_fails() {
        let mut p = sample_project();
        p.stock.width_mm = f64::NAN;
        assert_eq!(
            save(&p),
            Err(SaveError::NonFinite {
                field: "stock.width_mm".into()
            })
        );
    }

    #[test]
    fn missing_version_is_rejected() {
        let line = format!("  \"version\": {},\n", crate::CURRENT_VERSION);
        let text = CURRENT_FIXTURE.replacen(&line, "", 1);
        assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
        assert_eq!(load(text.as_bytes()), Err(LoadError::MissingVersion));
    }

    #[test]
    fn non_object_root_is_rejected() {
        assert_eq!(load(b"[1, 2]"), Err(LoadError::NotAnObject));
    }

    #[test]
    fn invalid_json_reports_position() {
        let err = load(b"{\n  \"version\": 1,\n  oops\n}").unwrap_err();
        assert!(matches!(err, LoadError::Json { line: 3, .. }), "{err:?}");
    }

    #[test]
    fn load_returns_validation_errors_with_project() {
        let mut p = sample_project();
        p.stock.thickness_mm = Some(0.0);
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert!(loaded.has_errors());
        assert_eq!(loaded.problems, validate(&p));
    }

    #[test]
    fn load_returns_warnings_without_errors() {
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.enabled = false;
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert!(!loaded.has_errors());
        // The disabled operation, and its tool, which nothing else uses.
        assert_eq!(loaded.problems.len(), 2);
    }

    #[test]
    fn edited_post_processor_round_trips() {
        let mut p = sample_project();
        p.post_processor.file_extension = "gcode".into();
        p.post_processor.spindle_on_dwell_s = 5.5;
        p.post_processor.decimal_places.inch = 5;
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
    }

    #[test]
    fn unknown_post_processor_field_is_rejected() {
        let text = CURRENT_FIXTURE.replacen(
            "  \"post_processor\": {\n",
            "  \"post_processor\": {\n    \"colour\": \"red\",\n",
            1,
        );
        assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
        assert!(matches!(
            load(text.as_bytes()),
            Err(LoadError::Schema { .. })
        ));
    }

    #[test]
    fn named_origin_saves_as_a_string() {
        let text = save(&sample_project()).unwrap();
        assert!(text.contains("\"xy\": \"bottom_left\","), "{text}");
    }

    #[test]
    fn custom_origin_round_trips_as_a_keyed_object() {
        let mut p = sample_project();
        p.origin.xy = OriginXy::Custom {
            x_mm: 20.0,
            y_mm: 5.0,
        };
        let value = serde_json::to_value(&p).unwrap();
        assert_eq!(
            value["origin"]["xy"],
            serde_json::json!({ "custom": { "x_mm": 20.0, "y_mm": 5.0 } })
        );
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
    }

    #[test]
    fn plain_cutter_saves_as_a_string() {
        let text = save(&sample_project()).unwrap();
        assert!(text.contains("\"cutter\": \"end_mill\","), "{text}");
    }

    #[test]
    fn v_bit_cutter_round_trips_as_a_keyed_object() {
        let mut p = sample_project();
        p.tools[0].cutter = ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        let value = serde_json::to_value(&p).unwrap();
        assert_eq!(
            value["tools"][0]["cutter"],
            serde_json::json!({ "v_bit": { "included_angle_deg": 60.0 } })
        );
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
    }

    /// The version 2 form is read only through the migration.
    #[test]
    fn kind_object_is_rejected_at_current_version() {
        let text = CURRENT_FIXTURE.replacen(
            "\"xy\": \"bottom_left\",",
            "\"xy\": { \"kind\": \"bottom_left\" },",
            1,
        );
        assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
        assert!(matches!(
            load(text.as_bytes()),
            Err(LoadError::Schema { .. })
        ));
    }

    /// The byte offset of the top-level `key` in `text`.
    fn top_level_offset(text: &str, key: &str) -> usize {
        text.find(&format!("\n  \"{key}\": "))
            .unwrap_or_else(|| panic!("{key} is a top-level key: {text}"))
    }

    /// #76: everything a user fills in comes before the geometry.
    #[test]
    fn settings_are_written_before_geometry() {
        let text = save(&sample_project()).unwrap();
        let settings = ["stock", "tools", "operations"].map(|k| top_level_offset(&text, k));
        let rest =
            ["instances", "post_processor", "definitions"].map(|k| top_level_offset(&text, k));
        for setting in settings {
            assert!(rest.iter().all(|r| setting < *r), "{text}");
        }
        assert!(rest.windows(2).all(|w| w[0] < w[1]), "{text}");
    }

    /// #76: each vertex object is on one line; the rest of the file is the
    /// committed pretty layout.
    #[test]
    fn vertices_are_one_per_line() {
        let text = save(&sample_project()).unwrap();
        let vertex_lines: Vec<&str> = text.lines().filter(|l| l.contains("\"x\"")).collect();
        assert_eq!(vertex_lines.len(), 4, "{text}");
        for line in vertex_lines {
            let line = line.trim().trim_end_matches(',');
            assert!(line.starts_with("{ \"x\": "), "{line}");
            assert!(line.contains(", \"y\": "), "{line}");
            assert!(line.contains(", \"bulge\": "), "{line}");
            assert!(line.ends_with(" }"), "{line}");
        }
        assert!(!text.contains("\"y\": 0.0,\n"), "{text}");
        assert_eq!(text, CURRENT_FIXTURE);
    }

    /// #76: the compact layout loses nothing: every `f64` comes back
    /// bit-for-bit, and saving again gives the same bytes.
    #[test]
    fn compact_output_round_trips() {
        let mut p = sample_project();
        let vertices = [
            (0.1 + 0.2, 1e-7, 0.0),
            (137.684_123_456_789_01, -88.535, 0.414_213_562_373_095_1),
            (f64::MIN_POSITIVE, 5_999.999_999_999_999, -1.0 / 3.0),
        ]
        .map(|(x, y, bulge)| dc_geometry::Vertex { x, y, bulge });
        p.definitions.values_mut().for_each(|d| {
            d.profiles = vec![dc_geometry::Polyline::new(vertices.to_vec(), true)];
        });
        let first = save(&p).unwrap();
        let loaded = load(first.as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
        let profile = &loaded.project.definitions.values().next().unwrap().profiles[0];
        for (got, want) in profile.vertices().iter().zip(&vertices) {
            assert_eq!(got.x.to_bits(), want.x.to_bits());
            assert_eq!(got.y.to_bits(), want.y.to_bits());
            assert_eq!(got.bulge.to_bits(), want.bulge.to_bits());
        }
        assert_eq!(save(&loaded.project).unwrap(), first);
    }

    /// #76 changed only the layout, not the version: a file in the old
    /// order and layout (geometry first, a vertex over five lines) loads,
    /// at version 3 and at version 4.
    #[test]
    fn old_key_order_still_loads() {
        for text in [V3_FIXTURE, V4_GEOMETRY_FIRST_FIXTURE] {
            assert!(
                text.find("\"definitions\"") < text.find("\"tools\""),
                "fixture is geometry first"
            );
            assert!(text.contains("\"x\": 0.0,\n"), "fixture has the old layout");
            let loaded = load(text.as_bytes()).unwrap();
            assert_eq!(loaded.project, sample_project());
            assert_eq!(loaded.problems, vec![]);
        }
    }

    /// Readers take the members in any order, as JSON allows.
    #[test]
    fn any_key_order_loads() {
        let value: Value = serde_json::from_str(CURRENT_FIXTURE).unwrap();
        let Value::Object(map) = value else {
            panic!("root is an object")
        };
        let reversed: serde_json::Map<_, _> = map.into_iter().rev().collect();
        let text = serde_json::to_string(&Value::Object(reversed)).unwrap();
        assert!(text.starts_with("{\"definitions\""), "{text}");
        assert_eq!(load(text.as_bytes()).unwrap().project, sample_project());
    }

    const V2_FIXTURE: &str = include_str!("../tests/fixtures/dcv/v2/sample.dcv");
    /// A whole grblHAL definition, as a project held it up to version 6.
    const GRBLHAL: &str = include_str!("../tests/fixtures/post/grblhal.json");

    /// Every `Option` field of a post-processor definition, as keys.
    const POST_OPTION_KEYS: &[&[&str]] = &[
        &["vendor"],
        &["units", "mm_command"],
        &["units", "inch_command"],
        &["work_coordinates", "select_command"],
        &["work_coordinates", "machine_command"],
        &["canned_cycles"],
        &["canned_cycles", "drill_command"],
        &["canned_cycles", "drill_with_dwell_command"],
        &["canned_cycles", "peck_drill_command"],
        &["canned_cycles", "chip_break_drill_command"],
        &["canned_cycles", "retract_mode"],
        &["canned_cycles", "reentry_gap_mm"],
        &["coolant"],
    ];

    /// #65: an `Option` is required, `null` when unset, so a
    /// post-processor definition with a field left out fails rather than
    /// reading as `None`.
    #[test]
    fn every_missing_post_option_fails() {
        let mut post: Value = serde_json::from_str(GRBLHAL).unwrap();
        // The fixture predates the phase 2 definition fields (#117).
        post["feed_speed"]["spindle_on_dwell"]["units"] = Value::from("seconds");
        post["use_canned_cycles"] = Value::from(false);
        // And the one #283 added.
        post["work_coordinates"]["machine_command"] = Value::from("G53");
        // grblHAL has no canned cycles; fill them so their options are there
        // to remove.
        post["canned_cycles"] = serde_json::json!({
            "drill_command": "G81",
            "drill_with_dwell_command": "G82",
            "peck_drill_command": "G83",
            "chip_break_drill_command": "G73",
            "peck_step_word": "Q",
            "retract_mode": "G98",
            "cancel_command": "G80",
            "reentry_gap_mm": null
        });
        let parse = serde_json::from_value::<crate::PostProcessor>;
        assert!(parse(post.clone()).is_ok(), "the grblHAL sample parses");
        for keys in POST_OPTION_KEYS {
            let mut edited = post.clone();
            let (last, parents) = keys.split_last().unwrap();
            let mut parent = &mut edited;
            for key in parents {
                parent = &mut parent[*key];
            }
            let removed = parent.as_object_mut().and_then(|o| o.shift_remove(*last));
            assert!(removed.is_some(), "{keys:?} is in the sample");
            let err = parse(edited).unwrap_err();
            let missing = format!("missing field `{last}`");
            assert!(err.to_string().contains(&missing), "{keys:?}: {err}");
        }
    }

    /// The 1-based line of the first line of `text` containing `needle`.
    fn line_of(text: &str, needle: &str) -> usize {
        text.lines()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("{needle} in {text}"))
            + 1
    }

    /// #65: an unknown key in a file at the current version is reported at
    /// its line and column, also inside an internally tagged enum.
    #[test]
    fn schema_error_has_line_and_column() {
        let cases = [
            (
                "\"thickness_mm\": 6.0",
                "\"thickness_mm\": 6.0,\n    \"grain\": \"oak\"",
                "\"grain\"",
                "stock.grain",
                14,
            ),
            (
                "\"count\": 4",
                "\"count\": 4,\n          \"bogus\": 1",
                "\"bogus\"",
                "operations[0].tabs.placement",
                20,
            ),
        ];
        for (from, to, key, path, column) in cases {
            let text = CURRENT_FIXTURE.replacen(from, to, 1);
            assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
            let line = line_of(&text, key);
            let err = load(text.as_bytes()).unwrap_err();
            assert!(
                matches!(&err, LoadError::Schema { path: p, at, .. }
                    if p == path && *at == Some(TextPosition { line, column })),
                "{err:?}"
            );
            let shown = err.to_string();
            assert!(shown.contains(&format!("`{path}`")), "{shown}");
            assert!(
                shown.contains(&format!("line {line}, column {column}")),
                "{shown}"
            );
        }
    }

    /// #65: after a migration the error still names where the bad value is.
    #[test]
    fn schema_error_after_migration_has_path() {
        let text = V2_FIXTURE.replacen("\"count\": 4", "\"count\": 4,\n          \"bogus\": 1", 1);
        assert_ne!(text, V2_FIXTURE, "fixture layout changed");
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { path, message, .. }
                if path == "operations[0].tabs.placement" && message.contains("bogus")),
            "{err:?}"
        );
        assert!(
            err.to_string().contains("`operations[0].tabs.placement`"),
            "{err}"
        );
    }

    /// #65: a `decimal_places` of the wrong form says where it is and what
    /// it may be.
    #[test]
    fn bad_decimal_places_message() {
        let text = CURRENT_FIXTURE.replacen(
            "\"decimal_places\": {\n      \"mm\": 3,\n      \"inch\": 4\n    }",
            "\"decimal_places\": \"three\"",
            1,
        );
        assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed");
        let err = load(text.as_bytes()).unwrap_err();
        let shown = err.to_string();
        assert!(shown.contains("`post_processor.decimal_places`"), "{shown}");
        assert!(shown.contains("a whole number"), "{shown}");
        assert!(shown.contains("`mm` and `inch`"), "{shown}");
    }

    // Phase 2, #117: the new types and fields in the file.

    /// Functional spec, capability 21a: a tool's number is `null` until
    /// set, and a set one is written as the integer.
    #[test]
    fn tool_number_round_trips() {
        let mut p = sample_project();
        let text = save(&p).unwrap();
        assert!(text.contains("\"number\": null"), "{text}");
        p.tools[0].number = Some(3);
        let text = save(&p).unwrap();
        assert!(text.contains("\"number\": 3,"), "{text}");
        let loaded = load(text.as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
        assert_eq!(loaded.project.tools[0].number, Some(3));
    }

    /// Design spec, "Data model → dc-project": the four operation kinds
    /// are one internally tagged list, each tagged by `kind`.
    #[test]
    fn bore_round_trips() {
        let mut p = sample_project();
        let mut bore = crate::test_support::sample_bore();
        bore.pitch_mm = Some(1.5);
        bore.direction = crate::model::CutDirection::Conventional;
        bore.stock_to_leave_mm = 0.1;
        p.operations.push(Operation::Bore(bore));
        let text = save(&p).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let fields: Vec<&str> = value["operations"][1]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            fields,
            [
                "kind",
                "id",
                "name",
                "enabled",
                "targets",
                "tool",
                "depth_mm",
                "pitch_mm",
                "direction",
                "stock_to_leave_mm",
                "through_cut_allowance_mm",
                "safe_z_mm"
            ]
        );
        assert_eq!(value["operations"][1]["kind"], json!("bore"));
        assert_eq!(value["operations"][1]["direction"], json!("conventional"));
        assert_eq!(load(text.as_bytes()).unwrap().project, p);
    }

    /// #394 (format 19): a Pocket, a Bore and a Profile's last pass save
    /// the stock they leave as `stock_to_leave_mm`, never `allowance_mm`,
    /// and load back.
    #[test]
    fn save_writes_stock_to_leave_never_allowance() {
        let project = load(include_bytes!(
            "../tests/fixtures/dcv/v18/stock-to-leave.dcv"
        ))
        .unwrap()
        .project;
        let text = save(&project).unwrap();
        assert!(!text.contains("\"allowance_mm\""), "{text}");
        assert_eq!(
            text.matches("\"stock_to_leave_mm\": 0.5").count(),
            2,
            "{text}"
        );
        assert_eq!(
            text.matches("\"stock_to_leave_mm\": 0.1").count(),
            1,
            "{text}"
        );
        assert_eq!(text.matches("\"through_cut_allowance_mm\"").count(), 3);
        assert_eq!(load(text.as_bytes()).unwrap().project, project);
    }

    /// Format 18: an unknown field in a Bore is named, as in any operation.
    #[test]
    fn bore_unknown_field_is_named() {
        let mut p = sample_project();
        p.operations
            .push(Operation::Bore(crate::test_support::sample_bore()));
        let text = save(&p).unwrap().replacen("\"pitch_mm\"", "\"pitch\"", 1);
        let err = load(text.as_bytes()).unwrap_err().to_string();
        assert!(err.contains("pitch"), "{err}");
    }

    #[test]
    fn pocket_drill_engrave_round_trip() {
        let mut p = sample_project();
        let mut pocket = sample_pocket();
        pocket.strategy = ClearanceStrategy::Raster { angle_deg: 45.0 };
        pocket.step_over_fraction = Some(0.5);
        let mut drill = sample_drill();
        drill.cycle = DrillCycle::Peck {
            peck_depth_mm: None,
            retract: PeckRetract::ChipBreak { gap_mm: 0.5 },
        };
        drill.dwell_s = 0.5;
        let mut engrave = sample_engrave();
        engrave.alternate_direction = true;
        p.operations.push(Operation::Pocket(pocket));
        p.operations.push(Operation::Drill(drill));
        p.operations.push(Operation::Engrave(engrave));
        let text = save(&p).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        let kinds: Vec<&str> = value["operations"]
            .as_array()
            .unwrap()
            .iter()
            .map(|op| op["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["profile", "pocket", "drill", "engrave"]);
        assert_eq!(value["operations"][1]["rest_of"], serde_json::Value::Null);
        assert_eq!(
            value["operations"][2]["cycle"],
            json!({ "peck": { "peck_depth_mm": null, "retract": { "chip_break": { "gap_mm": 0.5 } } } })
        );
        let loaded = load(text.as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
        assert_eq!(loaded.problems, vec![]);
    }

    /// Design spec: `Entry` is a string for `plunge` and a one-key object
    /// for the variants with values.
    #[test]
    fn entry_variants_serialize_as_string_or_object() {
        for (entry, expected) in [
            (Entry::Plunge, json!("plunge")),
            (
                Entry::Ramp { angle_deg: 15.0 },
                json!({ "ramp": { "angle_deg": 15.0 } }),
            ),
            (
                Entry::Helix {
                    diameter_fraction: 0.8,
                    angle_deg: 5.0,
                },
                json!({ "helix": { "diameter_fraction": 0.8, "angle_deg": 5.0 } }),
            ),
        ] {
            let mut p = sample_project();
            profile_mut(&mut p).entry = entry.clone();
            let value = serde_json::to_value(&p).unwrap();
            assert_eq!(value["operations"][0]["entry"], expected, "{entry:?}");
            let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
            assert_eq!(loaded.project, p, "{entry:?}");
        }
    }

    /// Design spec: `offset` is a string, `raster` carries its angle; the
    /// drill cycle and peck retract follow the same shape.
    #[test]
    fn clearance_strategy_raster_carries_angle() {
        let mut p = sample_project();
        p.operations.push(Operation::Pocket(sample_pocket()));
        p.operations.push(Operation::Drill(sample_drill()));
        let value = serde_json::to_value(&p).unwrap();
        assert_eq!(value["operations"][1]["strategy"], json!("offset"));
        assert_eq!(value["operations"][1]["wall_pass"], json!("last"));
        assert_eq!(value["operations"][2]["cycle"], json!("simple"));
        let Some(Operation::Pocket(pocket)) = p.operations.get_mut(1) else {
            panic!("a pocket");
        };
        pocket.strategy = ClearanceStrategy::Raster { angle_deg: 45.0 };
        let Some(Operation::Drill(drill)) = p.operations.get_mut(2) else {
            panic!("a drill");
        };
        drill.cycle = DrillCycle::Peck {
            peck_depth_mm: Some(2.0),
            retract: PeckRetract::Full,
        };
        let value = serde_json::to_value(&p).unwrap();
        assert_eq!(
            value["operations"][1]["strategy"],
            json!({ "raster": { "angle_deg": 45.0 } })
        );
        assert_eq!(
            value["operations"][2]["cycle"],
            json!({ "peck": { "peck_depth_mm": 2.0, "retract": "full" } })
        );
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
    }

    /// #117, #65: a current-version file without a phase 2 field is
    /// rejected, naming the field at the operation. Only an older file
    /// gets it, from the migration.
    #[test]
    fn missing_phase_2_field_fails_to_load() {
        for line in [
            "      \"machining_order\": \"full_depth_first\",\n",
            "      \"entry\": \"plunge\",\n",
            "      \"lead\": null,\n",
            "  \"arc_fit_tolerance_mm\": 0.01,\n",
            "      \"number\": null,\n",
            "    \"use_canned_cycles\": false\n",
        ] {
            let text = CURRENT_FIXTURE.replacen(line, "", 1);
            assert_ne!(text, CURRENT_FIXTURE, "fixture layout changed: {line:?}");
            // Removing the last key of the block leaves a trailing comma.
            let text = text.replace("},\n  },\n  \"definitions\"", "}\n  },\n  \"definitions\"");
            let key = line.trim().split('"').nth(1).unwrap();
            let err = load(text.as_bytes()).unwrap_err();
            assert!(
                matches!(&err, LoadError::Schema { message, .. } if message.contains(key)),
                "{key}: {err:?}"
            );
            if key == "machining_order" {
                let LoadError::Schema { path, .. } = &err else {
                    panic!("{err:?}");
                };
                assert_eq!(path, "operations[0]");
            }
        }
    }
}
