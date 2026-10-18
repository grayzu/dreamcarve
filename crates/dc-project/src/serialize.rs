// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `.dcv` load and save (ADR-0002; `docs/specs/phase-1/design.md`, "Data
//! model → dc-project").
//!
//! [`save`] writes pretty-printed JSON with keys in declaration order, the
//! settings first and the geometry last, each vertex on one line
//! ([`crate::layout`]), and a trailing newline. [`load`] parses bytes, reads `version`, migrates to
//! [`crate::CURRENT_VERSION`], deserializes strictly (unknown fields and missing
//! fields both fail), and then validates.
//!
//! `load` fails only when the bytes cannot become a [`Project`]. Validation
//! problems, errors included, come back beside the project in [`Loaded`] so
//! a caller such as the CLI `check` command can report all of them; callers
//! that machine the project must refuse it when [`Loaded::has_errors`].

use crate::layout::DcvFormatter;
use crate::migrate::migrate;
use crate::model::Project;
use crate::validate::{Problem, validate};
use serde::Serialize;
use serde_json::Value;

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
    #[error("migration from version {from} to {to} failed: {message}")]
    Migration {
        /// Version migrated from.
        from: u32,
        /// Version migrated to.
        to: u32,
        /// What went wrong.
        message: String,
    },
    /// The JSON does not match the project schema (unknown or missing
    /// field, wrong type).
    #[error("file does not match the project format: {message}")]
    Schema {
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
    let root = migrate(root, version)?;
    let project: Project = serde_json::from_value(root).map_err(|e| LoadError::Schema {
        message: e.to_string(),
    })?;
    let problems = validate(&project);
    // serde_json rejects NaN and out-of-range numbers while parsing, so this
    // is a backstop for the "reject non-finite at the boundary" rule.
    if let Some(field) = first_non_finite(&problems) {
        return Err(LoadError::NonFinite { field });
    }
    Ok(Loaded { project, problems })
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
    use crate::model::{Operation, OriginXy, ToolKind};
    use crate::test_support::sample_project;
    use crate::validate::validate;

    /// The sample project as this build saves it.
    const CURRENT_FIXTURE: &str = include_str!("../tests/fixtures/dcv/v4/sample.dcv");
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
        let Operation::Profile(op) = &mut p.operations[0];
        op.tabs = None;
        op.depth_mm = None;
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
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
        let Operation::Profile(op) = &mut p.operations[0];
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
            matches!(&err, LoadError::Schema { message } if message.contains("feed_mm_min")),
            "{err:?}"
        );
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        let text = CURRENT_FIXTURE.replacen("{\n", "{\n  \"colour\": \"red\",\n", 1);
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { message } if message.contains("colour")),
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
            matches!(&err, LoadError::Schema { message } if message.contains("grain")),
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
            matches!(&err, LoadError::Schema { message } if message.contains("ramp")),
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
        let Operation::Profile(op) = &mut p.operations[0];
        op.enabled = false;
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert!(!loaded.has_errors());
        // The disabled operation, and its tool, which nothing else uses.
        assert_eq!(loaded.problems.len(), 2);
    }

    #[test]
    fn edited_post_processor_round_trips() {
        let mut p = sample_project();
        p.post_processor.id = "my-router".into();
        p.post_processor.file_extension = "gcode".into();
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
}
