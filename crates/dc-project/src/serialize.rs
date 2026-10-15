// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `.dcv` load and save (ADR-0002; `docs/specs/phase-1/design.md`, "Data
//! model → dc-project").
//!
//! [`save`] writes pretty-printed JSON with keys in declaration order and a
//! trailing newline. [`load`] parses bytes, reads `version`, migrates to
//! [`crate::CURRENT_VERSION`], deserializes strictly (unknown fields and missing
//! fields both fail), and then validates.
//!
//! `load` fails only when the bytes cannot become a [`Project`]. Validation
//! problems, errors included, come back beside the project in [`Loaded`] so
//! a caller such as the CLI `check` command can report all of them; callers
//! that machine the project must refuse it when [`Loaded::has_errors`].

use crate::migrate::migrate;
use crate::model::Project;
use crate::validate::{Problem, validate};
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
/// order, `Option` fields as `null`, ending in a newline.
///
/// Fails with [`SaveError::NonFinite`] rather than writing a NaN or
/// infinity, which serde_json would silently turn into `null`.
pub fn save(project: &Project) -> Result<String, SaveError> {
    if let Some(field) = first_non_finite(&validate(project)) {
        return Err(SaveError::NonFinite { field });
    }
    let mut text = serde_json::to_string_pretty(project).map_err(|e| SaveError::Serialize {
        message: e.to_string(),
    })?;
    text.push('\n');
    Ok(text)
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
    use crate::model::Operation;
    use crate::test_support::sample_project;
    use crate::validate::validate;

    const V1_FIXTURE: &str = include_str!("../tests/fixtures/dcv/v1/sample.dcv");

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
        let Operation::Profile(op) = &mut p.operations[0];
        op.tabs = None;
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
    }

    #[test]
    fn keys_are_stable() {
        assert_eq!(save(&sample_project()).unwrap(), V1_FIXTURE);
    }

    #[test]
    fn committed_v1_fixture_loads() {
        let loaded = load(V1_FIXTURE.as_bytes()).unwrap();
        assert_eq!(loaded.project, sample_project());
    }

    #[test]
    fn option_fields_serialize_as_null() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = None;
        p.tools[0].step_over_fraction = None;
        let Operation::Profile(op) = &mut p.operations[0];
        op.tabs = None;
        let text = save(&p).unwrap();
        assert!(text.contains("\"feed_mm_min\": null"), "{text}");
        assert!(text.contains("\"step_over_fraction\": null"), "{text}");
        assert!(text.contains("\"step_down_mm\": null"), "{text}");
        assert!(text.contains("\"tabs\": null"), "{text}");
    }

    #[test]
    fn missing_option_field_is_rejected() {
        let text = V1_FIXTURE.replacen("      \"feed_mm_min\": 1000.0,\n", "", 1);
        assert_ne!(text, V1_FIXTURE, "fixture layout changed");
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { message } if message.contains("feed_mm_min")),
            "{err:?}"
        );
    }

    #[test]
    fn unknown_top_level_field_is_rejected() {
        let text = V1_FIXTURE.replacen("{\n", "{\n  \"colour\": \"red\",\n", 1);
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { message } if message.contains("colour")),
            "{err:?}"
        );
    }

    #[test]
    fn unknown_nested_field_is_rejected() {
        let text = V1_FIXTURE.replacen(
            "\"thickness_mm\": 6.0",
            "\"thickness_mm\": 6.0,\n    \"grain\": \"oak\"",
            1,
        );
        assert_ne!(text, V1_FIXTURE, "fixture layout changed");
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { message } if message.contains("grain")),
            "{err:?}"
        );
    }

    #[test]
    fn unknown_operation_field_is_rejected() {
        let text = V1_FIXTURE.replacen(
            "\"safe_z_mm\": 5.0",
            "\"safe_z_mm\": 5.0,\n      \"ramp\": true",
            1,
        );
        assert_ne!(text, V1_FIXTURE, "fixture layout changed");
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { message } if message.contains("ramp")),
            "{err:?}"
        );
    }

    #[test]
    fn nan_literal_is_rejected() {
        let text = V1_FIXTURE.replacen("\"width_mm\": 100.0", "\"width_mm\": NaN", 1);
        assert_ne!(text, V1_FIXTURE, "fixture layout changed");
        assert!(matches!(load(text.as_bytes()), Err(LoadError::Json { .. })));
    }

    #[test]
    fn infinite_number_is_rejected() {
        let text = V1_FIXTURE.replacen("\"width_mm\": 100.0", "\"width_mm\": 1e999", 1);
        assert_ne!(text, V1_FIXTURE, "fixture layout changed");
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
        let text = V1_FIXTURE.replacen("  \"version\": 1,\n", "", 1);
        assert_ne!(text, V1_FIXTURE, "fixture layout changed");
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
        p.stock.thickness_mm = 0.0;
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert!(loaded.has_errors());
        assert_eq!(loaded.problems, validate(&p));
    }

    #[test]
    fn load_returns_warnings_without_errors() {
        let mut p = sample_project();
        p.tools[0].rpm = None;
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert!(!loaded.has_errors());
        assert_eq!(loaded.problems.len(), 1);
    }

    #[test]
    fn post_processor_round_trips_unchanged() {
        let mut p = sample_project();
        p.post_processor = crate::EmbeddedPostProcessor(serde_json::json!({
            "id": "custom",
            "motion": { "rapidCommand": "G0", "linearCommand": "G1" },
            "number_format": { "decimals": 3 }
        }));
        let loaded = load(save(&p).unwrap().as_bytes()).unwrap();
        assert_eq!(loaded.project, p);
    }
}
