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
//! - **5**: adds `post_processor.tool_change.mode`, set to
//!   `"separate_programs"` by the migration (#95): a project whose
//!   operations use more than one tool exports one program per tool run.
//!   Before, such a project exported one program with `M6` (grblHAL) or
//!   failed (GRBL); `"m6"` keeps the first.
//! - **6**: each `instances[].z_top_mm` and `z_bottom_mm` may be `null`
//!   (unset), which `init` and the SVG importer write (#99). Earlier files
//!   always held numbers, which carry over unchanged, including the 12 mm
//!   placeholder `init` used to write for `z_top_mm`.
//! - **7**: `post_processor` names a bundled definition version and holds
//!   only the user's settings (#106, ADR-0004). The migration moves the
//!   five settings out of the embedded copy, keeping their values, and
//!   replaces the copy with a reference to the bundled definition's
//!   version 1 only if everything else in it equals that version. A copy
//!   that differs, or one whose id is not bundled, fails to load with
//!   every difference and the block to paste instead: swapping in another
//!   dialect would change the G-code without the user knowing.

use crate::post_processor::{
    BundledDefinition, PostProcessorError, SETTING_PATHS, bundled_definition, bundled_definitions,
};
use crate::serialize::LoadError;
use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
use serde_json::{Map, Value};

/// The `.dcv` schema version this build writes.
pub const CURRENT_VERSION: u32 = 7;

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
        4 => Some(v4_to_v5),
        5 => Some(v5_to_v6),
        6 => Some(v6_to_v7),
        _ => None,
    }
}

const FLATTEN_TOLERANCE_KEY: &str = "flatten_tolerance_mm";
const SPINDLE_CONTROL_KEY: &str = "spindle_control";
const TOOL_CHANGE_MODE_KEY: &str = "mode";

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

/// Version 4 → 5: adds `mode: "separate_programs"` at the end of
/// `post_processor.tool_change`.
fn v4_to_v5(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let tool_change = root
        .get_mut("post_processor")
        .and_then(|post| post.get_mut("tool_change"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "`post_processor.tool_change` is missing or not an object".to_owned())?;
    if tool_change.contains_key(TOOL_CHANGE_MODE_KEY) {
        return Err(format!(
            "a version 4 file has no `tool_change.{TOOL_CHANGE_MODE_KEY}`, but this one does"
        ));
    }
    tool_change.insert(
        TOOL_CHANGE_MODE_KEY.into(),
        Value::from("separate_programs"),
    );
    Ok(root)
}

/// Version 5 → 6: instance depths become optional. Every version 5 value
/// is a number and stays as it is; this step only checks that, so a broken
/// file is not carried over as "unset".
fn v5_to_v6(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let instances = root
        .get("instances")
        .and_then(Value::as_array)
        .ok_or_else(|| "`instances` is missing or not an array".to_owned())?;
    for (i, instance) in instances.iter().enumerate() {
        for key in ["z_top_mm", "z_bottom_mm"] {
            if !instance.get(key).is_some_and(Value::is_number) {
                return Err(format!("`instances[{i}].{key}` is missing or not a number"));
            }
        }
    }
    Ok(root)
}

/// The definition version a version 6 project's embedded copy is compared
/// with: the one bundled while version 6 was current.
const V6_DEFINITION_VERSION: u32 = 1;

/// Version 6 → 7 (#106): the embedded post-processor becomes a reference
/// to the bundled definition's version 1 and the user's settings. Fails,
/// naming every difference, when the rest of the copy is not exactly that
/// definition.
fn v6_to_v7(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let mut copy = root
        .get_mut("post_processor")
        .filter(|post| post.is_object())
        .map(Value::take)
        .ok_or_else(|| "`post_processor` is missing or not an object".to_owned())?;
    let mut settings = Map::with_capacity(SETTING_PATHS.len());
    for (key, path) in SETTING_PATHS {
        let value = take_at(&mut copy, path)
            .ok_or_else(|| format!("`post_processor.{}` is missing", path.join(".")))?;
        settings.insert(key.to_owned(), value);
    }
    let id = copy
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| "`post_processor.id` is missing or not a string".to_owned())?
        .to_owned();
    match bundled_definition(&id, V6_DEFINITION_VERSION) {
        Ok(definition) => {
            let differences = differences(&copy, definition);
            if differences.is_empty() {
                root.insert("post_processor".into(), reference(definition, &settings));
                return Ok(root);
            }
            Err(format!(
                "the post-processor in this file differs from the bundled `{id}` definition, \
                 version {V6_DEFINITION_VERSION}. Since format version 7 a project names a \
                 bundled definition instead of holding a copy, and using that one would change \
                 the G-code, so the file is not loaded. It differs in:\n{}",
                replacement(definition, &settings, &differences)
            ))
        }
        Err(err @ PostProcessorError::UnknownId { .. }) => {
            // Offer the bundled definition closest to the copy.
            let closest = bundled_definitions()
                .map_err(|e| e.to_string())?
                .iter()
                .filter(|d| d.version == V6_DEFINITION_VERSION)
                .map(|d| (differences(&copy, d), d))
                .min_by_key(|(differences, _)| differences.len());
            let Some((differences, definition)) = closest else {
                return Err(err.to_string());
            };
            Err(format!(
                "{err}. Since format version 7 a project names a bundled definition instead \
                 of holding a copy, so the file is not loaded. The closest bundled definition \
                 is `{}`, version {V6_DEFINITION_VERSION}; it differs from this file in:\n{}",
                definition.id,
                replacement(definition, &settings, &differences)
            ))
        }
        Err(err) => Err(err.to_string()),
    }
}

/// Removes and returns the value at `path` in `value`.
fn take_at(value: &mut Value, path: &[&str]) -> Option<Value> {
    let (last, parents) = path.split_last()?;
    let mut at = value;
    for key in parents {
        at = at.get_mut(*key)?;
    }
    at.as_object_mut()?.shift_remove(*last)
}

/// The version 7 `post_processor` block: `definition` and `settings`, in
/// the order a project writes them.
fn reference(definition: &BundledDefinition, settings: &Map<String, Value>) -> Value {
    let mut block = Map::with_capacity(2 + settings.len());
    block.insert("id".into(), Value::from(definition.id));
    block.insert("definition_version".into(), Value::from(definition.version));
    block.extend(settings.clone());
    Value::Object(block)
}

/// The end of a failed version 6 → 7 message: one line per difference,
/// then the block to paste in place of the old one.
fn replacement(
    definition: &BundledDefinition,
    settings: &Map<String, Value>,
    differences: &[String],
) -> String {
    let block = serde_json::to_string_pretty(&reference(definition, settings))
        .unwrap_or_else(|e| format!("(the block could not be written: {e})"));
    format!(
        "{}\nTo use the bundled definition, change \"version\": 6 to \"version\": 7 and replace the whole \
         post_processor block with this one, which keeps your settings:\n\
         \"post_processor\": {block}",
        differences.join("\n")
    )
}

/// Every field where `copy`, an embedded definition without its settings,
/// differs from `definition`'s, as `  post_processor.<path>: this file has
/// <value>, <id> version <n> has <value>`.
fn differences(copy: &Value, definition: &BundledDefinition) -> Vec<String> {
    let label = format!("{} version {}", definition.id, definition.version);
    let mut out = Vec::new();
    collect_differences(
        "post_processor",
        copy,
        definition.dialect(),
        &label,
        &mut out,
    );
    out
}

fn collect_differences(
    path: &str,
    ours: &Value,
    theirs: &Value,
    label: &str,
    out: &mut Vec<String>,
) {
    match (ours, theirs) {
        (Value::Object(a), Value::Object(b)) => {
            for (key, value) in a {
                let at = format!("{path}.{key}");
                match b.get(key) {
                    Some(other) => collect_differences(&at, value, other, label, out),
                    None => out.push(format!(
                        "  {at}: this file has {value}, {label} has no such field"
                    )),
                }
            }
            for (key, other) in b {
                if !a.contains_key(key) {
                    out.push(format!(
                        "  {path}.{key}: this file has none, {label} has {other}"
                    ));
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (value, other)) in a.iter().zip(b).enumerate() {
                collect_differences(&format!("{path}[{i}]"), value, other, label, out);
            }
        }
        _ if ours == theirs => {}
        _ => out.push(format!(
            "  {path}: this file has {ours}, {label} has {theirs}"
        )),
    }
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
    use crate::{OriginXy, SpindleControl, ToolChangeMode, ToolKind, load, save};
    use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
    use serde_json::json;

    const V0_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v0/sample.dcv");
    const V1_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v1/sample.dcv");
    const V2_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v2/sample.dcv");
    const V3_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v3/sample.dcv");
    const V4_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v4/sample.dcv");
    const V5_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v5/sample.dcv");

    /// The sample project as raw JSON of `version`, without the fields
    /// added after it. Before version 7 it embeds the whole GRBL
    /// definition that `grbl/v1.json` was made from.
    fn sample_root_at(version: u32) -> Value {
        let mut root = serde_json::to_value(sample_project()).unwrap();
        if version < 7 {
            root["post_processor"] =
                serde_json::from_str(include_str!("../tests/fixtures/post/grbl.json")).unwrap();
        }
        if version < 5 {
            root["post_processor"]["tool_change"]
                .as_object_mut()
                .unwrap()
                .remove("mode");
        }
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
            loaded.project.post_processor.spindle_control,
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

    #[test]
    fn version_4_fixture_loads() {
        let loaded = load(V4_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
    }

    /// #95: a file before version 5 has no tool-change mode. The migration
    /// sets `"separate_programs"`, the new default, at the end of
    /// `tool_change`. A file that changed tools exported with `M6` before;
    /// it now exports one program per tool run, which every sender runs
    /// correctly, and `"m6"` brings the old output back.
    #[test]
    fn version_4_fixture_gets_separate_programs() {
        let loaded = load(V4_FIXTURE).unwrap();
        assert_eq!(
            loaded.project.post_processor.tool_change_mode,
            ToolChangeMode::SeparatePrograms
        );
        let migrated = migrate(sample_root_at(4), 4).unwrap();
        assert_eq!(
            migrated,
            sample_root_at(CURRENT_VERSION),
            "the key lands at the end of tool_change, where version 5 declares it"
        );
    }

    #[test]
    fn v4_with_tool_change_mode_already_present_fails() {
        let mut root = sample_root_at(5);
        root["version"] = Value::from(4);
        let err = migrate(root, 4).unwrap_err();
        assert!(
            matches!(err, LoadError::Migration { from: 4, to: 5, .. }),
            "{err:?}"
        );
    }

    #[test]
    fn version_5_fixture_loads() {
        let loaded = load(V5_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
    }

    /// #99: version 6 lets the instance depths be unset, but the migration
    /// never changes a value the user may have set. A version 5 file's
    /// numbers, including `init`'s old 12 mm placeholder, carry over.
    #[test]
    fn v5_to_v6_keeps_instance_depths() {
        let mut root = sample_root_at(5);
        root["instances"][0]["z_top_mm"] = json!(12.0);
        root["instances"][0]["z_bottom_mm"] = json!(0.0);
        let text = serde_json::to_string_pretty(&root).unwrap();
        let loaded = load(text.as_bytes()).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        let instance = &loaded.project.instances[0];
        assert_eq!(instance.z_top_mm, Some(12.0));
        assert_eq!(instance.z_bottom_mm, Some(0.0));
        let mut stepped = v5_to_v6(sample_root_at(5).as_object().unwrap().clone()).unwrap();
        stepped.insert("version".into(), Value::from(6));
        assert_eq!(
            Value::Object(stepped),
            sample_root_at(6),
            "the step changes nothing"
        );
    }

    /// A version 5 file had no way to leave an instance depth unset, so a
    /// `null` there is a broken file, not one to carry over as unset.
    #[test]
    fn v5_with_null_instance_depth_fails() {
        for key in ["z_top_mm", "z_bottom_mm"] {
            let mut root = sample_root_at(5);
            root["instances"][0][key] = Value::Null;
            let err = migrate(root, 5).unwrap_err();
            assert!(
                matches!(&err, LoadError::Migration { from: 5, to: 6, message }
                    if message.contains(&format!("instances[0].{key}"))),
                "{err:?}"
            );
        }
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
        let mut root = sample_root_at(2);
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

    const V6_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v6/sample.dcv");

    #[test]
    fn version_6_fixture_loads() {
        let loaded = load(V6_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
    }

    /// #106: the migrated block is the reference, in the order a version 7
    /// file writes it.
    #[test]
    fn v6_to_v7_writes_the_reference() {
        let migrated = migrate(sample_root_at(6), 6).unwrap();
        assert_eq!(migrated, sample_root_at(7));
    }

    /// #106: a version 6 file with no embedded post-processor is broken,
    /// not one to give a default.
    #[test]
    fn v6_without_post_processor_fails() {
        let mut root = sample_root_at(6);
        root.as_object_mut().unwrap().remove("post_processor");
        let err = migrate(root, 6).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 6, to: 7, message }
                if message.contains("post_processor")),
            "{err:?}"
        );
    }

    /// A version 6 project embedding the bundled grblHAL definition of the
    /// time, with the router switched by hand and a 5 s spin-up dwell.
    const V6_GRBLHAL_MANUAL: &str = include_str!("../tests/fixtures/dcv/v6/grblhal-manual.dcv");

    /// #106: a version 6 copy that matches the bundled definition becomes a
    /// reference to its version 1, keeping the user's settings.
    #[test]
    fn v6_with_bundled_copy_migrates_to_reference() {
        let loaded = load(V6_GRBLHAL_MANUAL.as_bytes()).unwrap();
        assert_eq!(loaded.project.version, 7);
        assert_eq!(
            serde_json::to_value(&loaded.project.post_processor).unwrap(),
            json!({
                "id": "grblhal",
                "definition_version": 1,
                "file_extension": "nc",
                "spindle_control": "manual",
                "spindle_on_dwell_s": 5.0,
                "tool_change_mode": "separate_programs",
                "decimal_places": { "mm": 3, "inch": 4 }
            })
        );
    }

    /// The migration error for `V6_GRBLHAL_MANUAL` with `from` replaced by
    /// `to`.
    fn v6_edited_error(from: &str, to: &str) -> String {
        assert!(V6_GRBLHAL_MANUAL.contains(from), "the fixture has `{from}`");
        let text = V6_GRBLHAL_MANUAL.replacen(from, to, 1);
        match load(text.as_bytes()) {
            Err(LoadError::Migration {
                from: 6,
                to: 7,
                message,
            }) => message,
            other => panic!("a migration error from 6 to 7: {other:?}"),
        }
    }

    /// The block to paste in place of the old one, with the fixture's
    /// settings filled in.
    fn assert_has_replacement_block(message: &str, id: &str) {
        let block = format!(
            r#""post_processor": {{
  "id": "{id}",
  "definition_version": 1,
  "file_extension": "nc",
  "spindle_control": "manual",
  "spindle_on_dwell_s": 5.0,
  "tool_change_mode": "separate_programs",
  "decimal_places": {{
    "mm": 3,
    "inch": 4
  }}
}}"#
        );
        assert!(message.contains(&block), "{message}");
        assert!(message.contains("\"version\": 7"), "{message}");
    }

    /// #106: a copy whose dialect differs from the bundled version 1 is
    /// never replaced by it, which would change the G-code: loading fails,
    /// naming each differing field with both values, and shows the
    /// reference block to use instead.
    #[test]
    fn v6_with_edited_dialect_fails_to_load() {
        let message = v6_edited_error("\"rapid_command\": \"G0\"", "\"rapid_command\": \"G1\"");
        assert!(
            message.contains("post_processor.motion.rapid_command"),
            "{message}"
        );
        assert!(message.contains("\"G1\""), "{message}");
        assert!(message.contains("\"G0\""), "{message}");
        assert!(!message.contains("linear_command"), "{message}");
        assert_has_replacement_block(&message, "grblhal");
    }

    /// #106: a post-processor that was never bundled cannot become a
    /// reference either.
    #[test]
    fn v6_with_unbundled_id_fails_to_load() {
        let message = v6_edited_error("\"id\": \"grblhal\"", "\"id\": \"custom\"");
        assert!(message.contains("`custom`"), "{message}");
        assert!(message.contains("grbl, grblhal"), "{message}");
        assert!(message.contains("post_processor.id"), "{message}");
        assert_has_replacement_block(&message, "grblhal");
    }

    #[test]
    fn current_version_is_unchanged() {
        let root = serde_json::to_value(sample_project()).unwrap();
        assert_eq!(migrate(root.clone(), CURRENT_VERSION).unwrap(), root);
    }
}
