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
//! - **2**: adds `post_processor.feed_speed.spindle_control`, set to
//!   `"automatic"` by the migration, which is how every earlier file was
//!   exported.
//! - **3**: `origin.xy` and each `tools[].cutter` change from an object
//!   tagged by `kind` to serde's external tagging: a choice without data is
//!   a plain string (`"bottom_left"`), one with data is a one-key object
//!   (`{ "custom": { "x_mm": 20.0, "y_mm": 5.0 } }`) (#46).
//! - **4**: `stock.thickness_mm` and each Profile's `depth_mm` may be
//!   `null` (unset), which `init` writes so export refuses until the user
//!   sets them (#64). Earlier files always held numbers, which carry over
//!   unchanged.

use crate::serialize::LoadError;
use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
use serde_json::{Map, Value};

/// The `.dcv` schema version this build writes.
pub const CURRENT_VERSION: u32 = 4;

/// One migration step: rewrites a root object of version `from` into
/// version `from + 1`.
type Step = fn(Map<String, Value>) -> Result<Map<String, Value>, String>;

/// The step that migrates from version `from`, if there is one.
fn step(from: u32) -> Option<Step> {
    match from {
        0 => Some(v0_to_v1),
        1 => Some(v1_to_v2),
        2 => Some(v2_to_v3),
        3 => Some(v3_to_v4),
        _ => None,
    }
}

const FLATTEN_TOLERANCE_KEY: &str = "flatten_tolerance_mm";
const SPINDLE_CONTROL_KEY: &str = "spindle_control";

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

/// Version 1 → 2: adds `spindle_control: "automatic"` at the end of
/// `post_processor.feed_speed`.
fn v1_to_v2(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let feed_speed = root
        .get_mut("post_processor")
        .and_then(|post| post.get_mut("feed_speed"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "`post_processor.feed_speed` is missing or not an object".to_owned())?;
    if feed_speed.contains_key(SPINDLE_CONTROL_KEY) {
        return Err(format!(
            "a version 1 file has no `{SPINDLE_CONTROL_KEY}`, but this one does"
        ));
    }
    feed_speed.insert(SPINDLE_CONTROL_KEY.into(), Value::from("automatic"));
    Ok(root)
}

/// Version 2 → 3: rewrites `origin.xy` and every `tools[].cutter` from
/// `{ "kind": k, ..data }` to `k` or `{ k: { ..data } }`.
fn v2_to_v3(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let xy = root
        .get_mut("origin")
        .and_then(|origin| origin.get_mut("xy"))
        .ok_or_else(|| "`origin.xy` is missing".to_owned())?;
    *xy = untag(xy.take(), "origin.xy")?;
    let tools = root
        .get_mut("tools")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "`tools` is missing or not an array".to_owned())?;
    for (i, tool) in tools.iter_mut().enumerate() {
        let path = format!("tools[{i}].cutter");
        let cutter = tool
            .get_mut("cutter")
            .ok_or_else(|| format!("`{path}` is missing"))?;
        *cutter = untag(cutter.take(), &path)?;
    }
    Ok(root)
}

/// Version 3 → 4: thickness and depth become optional. Every version 3
/// value is a number and stays as it is; this step only checks that, so a
/// broken file is not carried over as "unset".
fn v3_to_v4(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let thickness = root
        .get("stock")
        .and_then(|stock| stock.get("thickness_mm"));
    if !thickness.is_some_and(Value::is_number) {
        return Err("`stock.thickness_mm` is missing or not a number".into());
    }
    let operations = root
        .get("operations")
        .and_then(Value::as_array)
        .ok_or_else(|| "`operations` is missing or not an array".to_owned())?;
    for (i, op) in operations.iter().enumerate() {
        if !op.get("depth_mm").is_some_and(Value::is_number) {
            return Err(format!(
                "`operations[{i}].depth_mm` is missing or not a number"
            ));
        }
    }
    Ok(root)
}

/// Rewrites one `{ "kind": k, ..data }` object as `k` when there is no
/// data, else as `{ k: { ..data } }`. `path` names the value in errors.
fn untag(value: Value, path: &str) -> Result<Value, String> {
    let Value::Object(mut fields) = value else {
        return Err(format!("`{path}` is not an object"));
    };
    let Some(Value::String(kind)) = fields.remove("kind") else {
        return Err(format!("`{path}` has no string `kind`"));
    };
    if fields.is_empty() {
        Ok(Value::String(kind))
    } else {
        let mut keyed = Map::with_capacity(1);
        keyed.insert(kind, Value::Object(fields));
        Ok(Value::Object(keyed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::sample_project;
    use crate::{OriginXy, SpindleControl, ToolKind, load, save};
    use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
    use serde_json::json;

    const V0_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v0/sample.dcv");
    const V1_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v1/sample.dcv");
    const V2_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v2/sample.dcv");
    const V3_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v3/sample.dcv");

    /// The sample project as raw JSON of `version`, without the fields
    /// added after it.
    fn sample_root_at(version: u32) -> Value {
        let mut root = serde_json::to_value(sample_project()).unwrap();
        if version < 3 {
            root["origin"]["xy"] = json!({ "kind": "bottom_left" });
            root["tools"][0]["cutter"] = json!({ "kind": "end_mill" });
        }
        if version < 2 {
            root["post_processor"]["feed_speed"]
                .as_object_mut()
                .unwrap()
                .remove("spindle_control");
        }
        if version < 1 {
            root.as_object_mut().unwrap().remove("flatten_tolerance_mm");
        }
        root["version"] = Value::from(version);
        root
    }

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
    fn version_1_fixture_loads() {
        let loaded = load(V1_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
    }

    /// Every file before version 2 was exported with the spindle switched by
    /// the program, so the migration keeps that (#34).
    #[test]
    fn version_1_fixture_gets_automatic_spindle_control() {
        let loaded = load(V1_FIXTURE).unwrap();
        assert_eq!(
            loaded.project.post_processor.feed_speed.spindle_control,
            SpindleControl::Automatic
        );
        let migrated = migrate(sample_root_at(1), 1).unwrap();
        assert_eq!(
            migrated,
            sample_root_at(CURRENT_VERSION),
            "the key lands at the end of feed_speed, where version 2 declares it"
        );
    }

    #[test]
    fn version_2_fixture_loads() {
        let loaded = load(V2_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
    }

    #[test]
    fn version_3_fixture_loads() {
        let loaded = load(V3_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
    }

    /// #64: version 4 lets thickness and depth be unset, but a version 3
    /// file always had numbers, and they carry over unchanged.
    #[test]
    fn v3_to_v4_keeps_thickness_and_depth() {
        let mut root = sample_root_at(3);
        root["stock"]["thickness_mm"] = json!(18.3);
        root["operations"][0]["depth_mm"] = json!(18.5);
        let migrated = migrate(root, 3).unwrap();
        let project: crate::Project = serde_json::from_value(migrated).unwrap();
        dc_geometry::assert_approx_eq!(project.stock.thickness_mm.unwrap(), 18.3, 0.0);
        let crate::Operation::Profile(op) = &project.operations[0];
        dc_geometry::assert_approx_eq!(op.depth_mm.unwrap(), 18.5, 0.0);
    }

    /// A version 3 file had no way to leave thickness unset, so a `null`
    /// there is a broken file, not one to carry over as unset.
    #[test]
    fn v3_with_null_thickness_fails() {
        let mut root = sample_root_at(3);
        root["stock"]["thickness_mm"] = Value::Null;
        let err = migrate(root, 3).unwrap_err();
        assert!(
            matches!(err, LoadError::Migration { from: 3, to: 4, .. }),
            "{err:?}"
        );
    }

    /// Unit variants become strings; a variant with data becomes a one-key
    /// object (#46).
    #[test]
    fn v2_to_v3_rewrites_origin_and_cutters() {
        let mut root = sample_root_at(2);
        root["origin"]["xy"] = json!({ "kind": "custom", "x_mm": 20.0, "y_mm": 5.0 });
        let mut v_bit = root["tools"][0].clone();
        v_bit["id"] = json!("t2");
        v_bit["cutter"] = json!({ "kind": "v_bit", "included_angle_deg": 60.0 });
        root["tools"].as_array_mut().unwrap().push(v_bit);

        let migrated = migrate(root, 2).unwrap();
        assert_eq!(
            migrated["origin"]["xy"],
            json!({ "custom": { "x_mm": 20.0, "y_mm": 5.0 } })
        );
        assert_eq!(migrated["tools"][0]["cutter"], json!("end_mill"));
        assert_eq!(
            migrated["tools"][1]["cutter"],
            json!({ "v_bit": { "included_angle_deg": 60.0 } })
        );
        let project: crate::Project = serde_json::from_value(migrated).unwrap();
        assert_eq!(
            project.origin.xy,
            OriginXy::Custom {
                x_mm: 20.0,
                y_mm: 5.0
            }
        );
        assert_eq!(
            project.tools[1].cutter,
            ToolKind::VBit {
                included_angle_deg: 60.0
            }
        );
    }

    #[test]
    fn v2_with_malformed_origin_fails() {
        let mut root = sample_root_at(2);
        root["origin"]["xy"] = json!({ "x_mm": 20.0 });
        let err = migrate(root, 2).unwrap_err();
        assert!(
            matches!(err, LoadError::Migration { from: 2, to: 3, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn newer_version_is_rejected() {
        let newer = CURRENT_VERSION + 1;
        let text = save(&sample_project()).unwrap().replacen(
            &format!("\"version\": {CURRENT_VERSION}"),
            &format!("\"version\": {newer}"),
            1,
        );
        let err = load(text.as_bytes()).unwrap_err();
        assert_eq!(
            err,
            LoadError::NewerVersion {
                found: newer,
                supported: CURRENT_VERSION
            }
        );
    }

    #[test]
    fn migrate_sets_current_version() {
        let migrated = migrate(sample_root_at(0), 0).unwrap();
        assert_eq!(migrated["version"], Value::from(CURRENT_VERSION));
    }

    #[test]
    fn v1_with_spindle_control_already_present_fails() {
        let mut root = sample_root_at(CURRENT_VERSION);
        root["version"] = Value::from(1);
        let err = migrate(root, 1).unwrap_err();
        assert!(
            matches!(err, LoadError::Migration { from: 1, to: 2, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn v0_with_flatten_tolerance_already_present_fails() {
        let mut root = sample_root_at(1);
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
