// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! In-memory `.dcv` schema migrations (ADR-0002; `docs/specs/phase-1/design.md`,
//! "Data model → dc-project").
//!
//! Each step rewrites the raw JSON of version N−1 into version N, so a step
//! never depends on the current Rust types. Steps run in sequence up to
//! [`CURRENT_VERSION`].
//!
//! Version history:
//!
//! - **0**: the pre-release shape. Identical to version 1 except that it has
//!   no `flatten_tolerance_mm`; curves were flattened at a fixed 0.01 mm.
//! - **1**: adds `flatten_tolerance_mm`, set to
//!   [`dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM`] by the migration.

use crate::serialize::LoadError;
use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
use serde_json::{Map, Value};

/// The `.dcv` schema version this build writes.
pub const CURRENT_VERSION: u32 = 1;

/// One migration step: rewrites a root object of version `from` into
/// version `from + 1`.
type Step = fn(Map<String, Value>) -> Result<Map<String, Value>, String>;

/// The step that migrates from version `from`, if there is one.
fn step(from: u32) -> Option<Step> {
    match from {
        0 => Some(v0_to_v1),
        _ => None,
    }
}

const FLATTEN_TOLERANCE_KEY: &str = "flatten_tolerance_mm";

/// Migrates the raw JSON root of a `.dcv` file from `from_version` to
/// [`CURRENT_VERSION`] and sets its `version` field accordingly.
///
/// Fails with [`LoadError::NewerVersion`] when `from_version` is newer than
/// this build, [`LoadError::NotAnObject`] when the root is not an object, and
/// [`LoadError::Migration`] when a step cannot rewrite the file.
pub fn migrate(root: Value, from_version: u32) -> Result<Value, LoadError> {
    if from_version > CURRENT_VERSION {
        return Err(LoadError::NewerVersion {
            found: from_version,
            supported: CURRENT_VERSION,
        });
    }
    let Value::Object(mut obj) = root else {
        return Err(LoadError::NotAnObject);
    };
    for from in from_version..CURRENT_VERSION {
        let to = from + 1;
        let step = step(from).ok_or_else(|| LoadError::Migration {
            from,
            to,
            message: "no migration step is defined".into(),
        })?;
        obj = step(obj).map_err(|message| LoadError::Migration { from, to, message })?;
    }
    obj.insert("version".into(), Value::from(CURRENT_VERSION));
    Ok(Value::Object(obj))
}

/// Version 0 → 1: adds `flatten_tolerance_mm` after `display_units`.
fn v0_to_v1(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    if root.contains_key(FLATTEN_TOLERANCE_KEY) {
        return Err(format!(
            "a version 0 file has no `{FLATTEN_TOLERANCE_KEY}`, but this one does"
        ));
    }
    // Rebuild the map so the new key lands where version 1 declares it.
    let mut out = Map::with_capacity(root.len() + 1);
    let mut inserted = false;
    for (key, value) in root {
        let after_this = key == "display_units";
        out.insert(key, value);
        if after_this {
            out.insert(
                FLATTEN_TOLERANCE_KEY.into(),
                Value::from(FLATTEN_TOLERANCE_DEFAULT_MM),
            );
            inserted = true;
        }
    }
    if !inserted {
        out.insert(
            FLATTEN_TOLERANCE_KEY.into(),
            Value::from(FLATTEN_TOLERANCE_DEFAULT_MM),
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::sample_project;
    use crate::{load, save};
    use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;

    const V0_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v0/sample.dcv");

    #[test]
    fn version_0_fixture_loads() {
        let loaded = load(V0_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
    }

    #[test]
    fn version_0_fixture_gets_default_flatten_tolerance() {
        let loaded = load(V0_FIXTURE).unwrap();
        dc_geometry::assert_approx_eq!(
            loaded.project.flatten_tolerance_mm,
            FLATTEN_TOLERANCE_DEFAULT_MM,
            0.0
        );
    }

    #[test]
    fn newer_version_is_rejected() {
        let text = save(&sample_project())
            .unwrap()
            .replacen("\"version\": 1", "\"version\": 2", 1);
        let err = load(text.as_bytes()).unwrap_err();
        assert_eq!(
            err,
            LoadError::NewerVersion {
                found: 2,
                supported: CURRENT_VERSION
            }
        );
    }

    #[test]
    fn migrate_sets_current_version() {
        let mut root = serde_json::to_value(sample_project()).unwrap();
        let obj = root.as_object_mut().unwrap();
        obj.remove("flatten_tolerance_mm");
        obj.insert("version".into(), Value::from(0));
        let migrated = migrate(root, 0).unwrap();
        assert_eq!(migrated["version"], Value::from(CURRENT_VERSION));
    }

    #[test]
    fn v0_with_flatten_tolerance_already_present_fails() {
        let mut root = serde_json::to_value(sample_project()).unwrap();
        root["version"] = Value::from(0);
        let err = migrate(root, 0).unwrap_err();
        assert!(
            matches!(err, LoadError::Migration { from: 0, to: 1, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn current_version_is_unchanged() {
        let root = serde_json::to_value(sample_project()).unwrap();
        assert_eq!(migrate(root.clone(), CURRENT_VERSION).unwrap(), root);
    }
}
