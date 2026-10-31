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
//! - **8**: phase 2 (#117; `docs/specs/phase-2/design.md`, "Migration
//!   v6 → v7", which is 7 → 8 here since #106 took version 7). Adds
//!   `arc_fit_tolerance_mm: 0.01` to the project, `number: null` to every
//!   tool, `machining_order: "full_depth_first"`, `entry: "plunge"`,
//!   `lead: null` and `corner_relief: "none"` to every Profile, and
//!   `use_canned_cycles: false` to the post-processor settings: the values
//!   that keep a phase 1 project cutting the same. Pocket, Drill and
//!   Engrave operations exist from this version on.
//! - **9**: adds `on_holes` to each Profile's `tabs`, set to `true` by the
//!   migration (#109): a Profile now leaves its holes without tabs unless
//!   `on_holes` is set, and every outline had tabs before.
//! - **10**: a Profile's `side` may be `"left"` or `"right"` (#207), which
//!   cut open paths beside them. No version 9 file holds either, so the
//!   migration changes only the version; the bump makes an older reader
//!   refuse a file that uses them.
//! - **11**: adds the project's placed tabs, `tabs: []` after
//!   `operations` (#105, ADR-0005). A Profile's `tabs` setting stays as a
//!   tab request: placing it needs the Profile's tool paths, which only
//!   `dc-cam` builds, so generation places it before it cuts, and each
//!   `dreamcarve tabs` command that saves the project writes it as placed
//!   tabs.
//! - **12**: adds the project's `heights` after `origin` (#283):
//!   `clearance_mm`, `tool_change_machine_z_mm` and `park`, all `null`,
//!   which exports what an earlier file did.
//! - **13**: adds each Profile's last pass allowance, `last_pass: null`
//!   after `corner_relief` (#284): every pass cuts on the part's path, as
//!   before.
//! - **14**: adds `avoid_corners: false` at the end of each Profile's
//!   non-null `tabs` request and `start: "auto"` after its `last_pass`
//!   (#285): tabs stay where they were laid and each cut starts where it
//!   did, so the file exports what it did.
//! - **15**: adds `holds_slug: null` at the end of each placed tab (#302):
//!   whether the tab is meant to hold the slug of the hole it is on, which
//!   format 14 did not record. Telling needs the Profile's tool paths, so
//!   the migration leaves it unset, and an unset tab warns `tab_on_hole`
//!   as format 14 did; cuts never depend on it.
//! - **16**: adds `vendor: null` and `product_id: null` right after each
//!   tool's `name` (maintainer decision 2026-10-24): who makes the tool
//!   and the maker's part number, which Fusion import fills in. No
//!   program reads them, so the file exports what it did.
//! - **17**: adds `allow_tool_wider_than_hole: false` at the end of each
//!   Drill operation (maintainer decision 2026-10-24): a Drill now skips a
//!   hole narrower than its tool unless the setting is `true`. An earlier
//!   file drilled such a hole at the tool's size; migrated, it skips it
//!   with a warning, the behaviour change the decision asks for.
//! - **18**: adds the Bore operation, `"kind": "bore"` (maintainer decision
//!   2026-10-24). No version 17 file holds one, so the migration changes
//!   only the version.
//! - **19**: renames `allowance_mm` to `stock_to_leave_mm` in each Pocket,
//!   each Bore and each Profile's non-null `last_pass` (#394, maintainer
//!   decision 2026-10-25): the material they leave on the wall is their
//!   stock to leave, and the through-cut allowance is the only allowance.
//!   The value and the key's place are kept, so the file exports what it
//!   did.
//! - **20**: adds the V-Carve operation, `"kind": "v_carve"`, with its
//!   clearing passes, start depth and flat depth (`docs/specs/phase-3/
//!   design.md`, sub-phase 3.1). No version 19 file holds one, so the
//!   migration changes only the version.
//! - **21**: adds the countersink Drill cycle, `{ "countersink": {
//!   "diameter_mm", "start_depth_mm", "start_plane_cleared_externally" }
//!   }` (sub-phase 3.2). No version 20 file holds one, so the migration
//!   changes only the version.
//! - **22**: adds the Inlay operation, `"kind": "inlay"`, with its `side`
//!   (`"female"` or `"male"`), `depth_mm`, `glue_gap_mm`, `plug_depth_mm`,
//!   `margin_mm` and clearing passes (sub-phase 3.3). No version 21 file
//!   holds one, so the migration changes only the version.
//! - **23**: adds a male Inlay's cut-out (#443), `"cut_out"`, after
//!   `safe_z_mm`: `null`, or `{ "tool", "tabs", "through_cut_allowance_mm"
//!   }`. The migration writes `"cut_out": null` in every Inlay, so a
//!   version 22 file cuts the same.

use crate::post_processor::{
    BundledDefinition, PostProcessorError, SETTING_PATHS, V6_SETTING_COUNT, bundled_definition,
    bundled_definitions,
};
use crate::serialize::LoadError;
use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
use serde_json::{Map, Value};

/// The `.dcv` schema version this build writes.
pub const CURRENT_VERSION: u32 = 23;

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
        7 => Some(v7_to_v8),
        8 => Some(v8_to_v9),
        9 => Some(v9_to_v10),
        10 => Some(v10_to_v11),
        11 => Some(v11_to_v12),
        12 => Some(v12_to_v13),
        13 => Some(v13_to_v14),
        14 => Some(v14_to_v15),
        15 => Some(v15_to_v16),
        16 => Some(v16_to_v17),
        17 => Some(v17_to_v18),
        18 => Some(v18_to_v19),
        19 => Some(v19_to_v20),
        20 => Some(v20_to_v21),
        21 => Some(v21_to_v22),
        22 => Some(v22_to_v23),
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
            at: None,
        })?;
        obj = step(obj).map_err(|message| LoadError::Migration {
            from,
            to,
            message,
            at: None,
        })?;
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
    let mut settings = Map::with_capacity(V6_SETTING_COUNT);
    for (key, path) in SETTING_PATHS.iter().take(V6_SETTING_COUNT) {
        let value = take_at(&mut copy, path)
            .ok_or_else(|| format!("`post_processor.{}` is missing", path.join(".")))?;
        settings.insert((*key).to_owned(), value);
    }
    upgrade_embedded_copy(&mut copy)?;
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

/// Gives a version 6 embedded copy the definition fields the schema has
/// gained since, with the values of its time, so it compares equal to the
/// bundled version 1 it was made from (#117): the dwell word was always
/// seconds on GRBL and grblHAL, and no controller then documented a
/// chip-break re-entry gap; both document `G53` (#283). These are
/// definition facts, so they never
/// appear in a version 7 or later project; the comparison is the only
/// place a version 6 copy meets them.
fn upgrade_embedded_copy(copy: &mut Value) -> Result<(), String> {
    let dwell = copy
        .get_mut("feed_speed")
        .and_then(|feed| feed.get_mut("spindle_on_dwell"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "`post_processor.feed_speed.spindle_on_dwell` is missing".to_owned())?;
    if dwell.contains_key("units") {
        return Err(
            "a version 6 file has no `post_processor.feed_speed.spindle_on_dwell.units`, but this one does"
                .into(),
        );
    }
    dwell.insert("units".into(), Value::from("seconds"));
    // #283: the machine-coordinate command is a fact of the controller,
    // not of the copy, so it is the bundled version 1's (`"G53"` on
    // grblHAL and GRBL, the only definitions a version 6 file held), and
    // nothing writes it unless a project sets the height.
    let machine_command = copy
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| bundled_definition(id, V6_DEFINITION_VERSION).ok())
        .and_then(|d| d.post_processor().work_coordinates.machine_command.clone())
        .map_or(Value::Null, Value::from);
    let work_coordinates = copy
        .get_mut("work_coordinates")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "`post_processor.work_coordinates` is missing".to_owned())?;
    if work_coordinates.contains_key("machine_command") {
        return Err(
            "a version 6 file has no `post_processor.work_coordinates.machine_command`, but this one does"
                .into(),
        );
    }
    work_coordinates.insert("machine_command".into(), machine_command);
    if let Some(cycles) = copy.get_mut("canned_cycles").and_then(Value::as_object_mut) {
        if cycles.contains_key("reentry_gap_mm") {
            return Err(
                "a version 6 file has no `post_processor.canned_cycles.reentry_gap_mm`, but this one does"
                    .into(),
            );
        }
        cycles.insert("reentry_gap_mm".into(), Value::Null);
    }
    Ok(())
}

/// Version 7 → 8 (#117): the phase 2 fields, with the values that keep a
/// phase 1 project cutting the same. Each new key lands where a version 8
/// file writes it. Fails if any of them is already there: that is not a
/// version 7 file.
fn v7_to_v8(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let mut root = insert_after(
        root,
        "flatten_tolerance_mm",
        "arc_fit_tolerance_mm",
        Value::from(0.01),
        "",
    )?;
    if let Some(tools) = root.get_mut("tools").and_then(Value::as_array_mut) {
        for (i, tool) in tools.iter_mut().enumerate() {
            let fields = tool
                .as_object_mut()
                .ok_or_else(|| format!("`tools[{i}]` is not an object"))?;
            let at = format!("tools[{i}].");
            *fields = insert_after(std::mem::take(fields), "name", "number", Value::Null, &at)?;
        }
    }
    if let Some(ops) = root.get_mut("operations").and_then(Value::as_array_mut) {
        for (i, op) in ops.iter_mut().enumerate() {
            let fields = op
                .as_object_mut()
                .ok_or_else(|| format!("`operations[{i}]` is not an object"))?;
            // Only Profile exists before version 8.
            if fields.get("kind").and_then(Value::as_str) != Some("profile") {
                return Err(format!(
                    "`operations[{i}].kind` is {}; a version 7 file holds only `\"profile\"` operations",
                    fields
                        .get("kind")
                        .map_or_else(|| "missing".to_owned(), Value::to_string)
                ));
            }
            for (key, value) in [
                ("machining_order", Value::from("full_depth_first")),
                ("entry", Value::from("plunge")),
                ("lead", Value::Null),
                ("corner_relief", Value::from("none")),
            ] {
                if fields.contains_key(key) {
                    return Err(format!(
                        "a version 7 file has no `operations[{i}].{key}`, but this one does"
                    ));
                }
                fields.insert(key.to_owned(), value);
            }
        }
    }
    let post = root
        .get_mut("post_processor")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "`post_processor` is missing or not an object".to_owned())?;
    if post.contains_key("use_canned_cycles") {
        return Err(
            "a version 7 file has no `post_processor.use_canned_cycles`, but this one does".into(),
        );
    }
    post.insert("use_canned_cycles".into(), Value::from(false));
    Ok(root)
}

/// Version 8 → 9 (#109): `on_holes: true` at the end of each Profile's
/// `tabs` object, so a project whose holes had tabs keeps them. A Profile
/// with `tabs: null` and the other operations are unchanged. Fails if the
/// key is already there: that is not a version 8 file.
fn v8_to_v9(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(ops) = root.get_mut("operations").and_then(Value::as_array_mut) else {
        return Ok(root);
    };
    for (i, op) in ops.iter_mut().enumerate() {
        if op.get("kind").and_then(Value::as_str) != Some("profile") {
            continue;
        }
        let Some(tabs) = op.get_mut("tabs").and_then(Value::as_object_mut) else {
            continue;
        };
        if tabs.contains_key("on_holes") {
            return Err(format!(
                "a version 8 file has no `operations[{i}].tabs.on_holes`, but this one does"
            ));
        }
        tabs.insert("on_holes".into(), Value::from(true));
    }
    Ok(root)
}

/// Version 9 → 10 (#207): Left and Right are new values of an existing
/// field, so a version 9 file is already a version 10 one.
fn v9_to_v10(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    Ok(root)
}

/// Version 10 → 11 (#105): `tabs: []` after `operations`, the project's
/// placed tabs, none yet. Each Profile's `tabs` setting is left as it is:
/// it is now a tab request, placed when the project is cut or saved by
/// `dreamcarve tabs` (ADR-0005). Fails if the project already has `tabs`: that is
/// not a version 10 file.
fn v10_to_v11(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    if root.contains_key("tabs") {
        return Err("a version 10 file has no `tabs`, but this one does".into());
    }
    let mut out = Map::with_capacity(root.len() + 1);
    let mut inserted = false;
    for (key, value) in root {
        let after_this = key == "operations";
        out.insert(key, value);
        if after_this {
            out.insert("tabs".into(), Value::Array(Vec::new()));
            inserted = true;
        }
    }
    if !inserted {
        out.insert("tabs".into(), Value::Array(Vec::new()));
    }
    Ok(out)
}

/// Version 11 → 12 (#283): `heights` after `origin`, with the clearance
/// height, the tool-change machine Z and the park point all `null`, so the
/// project exports what it did. Fails if the project already has
/// `heights`: that is not a version 11 file.
fn v11_to_v12(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    if root.contains_key("heights") {
        return Err("a version 11 file has no `heights`, but this one does".into());
    }
    let mut heights = Map::with_capacity(3);
    for key in ["clearance_mm", "tool_change_machine_z_mm", "park"] {
        heights.insert(key.into(), Value::Null);
    }
    let mut out = Map::with_capacity(root.len() + 1);
    let mut heights = Some(Value::Object(heights));
    for (key, value) in root {
        let after_this = key == "origin";
        out.insert(key, value);
        if after_this && let Some(heights) = heights.take() {
            out.insert("heights".into(), heights);
        }
    }
    if let Some(heights) = heights {
        out.insert("heights".into(), heights);
    }
    Ok(out)
}

/// Version 12 → 13 (#284): `last_pass: null` right after each Profile's
/// `corner_relief`, so every pass cuts on the part's path as before. Other
/// operations are unchanged. Fails if a Profile already has the key: that
/// is not a version 12 file.
fn v12_to_v13(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(ops) = root.get_mut("operations").and_then(Value::as_array_mut) else {
        return Ok(root);
    };
    for (i, op) in ops.iter_mut().enumerate() {
        if op.get("kind").and_then(Value::as_str) != Some("profile") {
            continue;
        }
        let Value::Object(fields) = std::mem::take(op) else {
            return Err(format!("`operations[{i}]` is not an object"));
        };
        if fields.contains_key("last_pass") {
            return Err(format!(
                "a version 12 file has no `operations[{i}].last_pass`, but this one does"
            ));
        }
        let mut out = Map::with_capacity(fields.len() + 1);
        let mut inserted = false;
        for (key, value) in fields {
            let after_this = key == "corner_relief";
            out.insert(key, value);
            if after_this {
                out.insert("last_pass".into(), Value::Null);
                inserted = true;
            }
        }
        if !inserted {
            out.insert("last_pass".into(), Value::Null);
        }
        *op = Value::Object(out);
    }
    Ok(root)
}

/// Version 13 → 14 (#285): `avoid_corners: false` at the end of each
/// Profile's non-null `tabs` request, so its tabs are laid where they
/// were, and `start: "auto"` right after its `last_pass`, so each cut
/// starts where it did. Other operations are unchanged. Fails if a Profile
/// already has either key: that is not a version 13 file.
fn v13_to_v14(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(ops) = root.get_mut("operations").and_then(Value::as_array_mut) else {
        return Ok(root);
    };
    for (i, op) in ops.iter_mut().enumerate() {
        if op.get("kind").and_then(Value::as_str) != Some("profile") {
            continue;
        }
        let Value::Object(fields) = std::mem::take(op) else {
            return Err(format!("`operations[{i}]` is not an object"));
        };
        if fields.contains_key("start") {
            return Err(format!(
                "a version 13 file has no `operations[{i}].start`, but this one does"
            ));
        }
        let mut out = Map::with_capacity(fields.len() + 1);
        let mut inserted = false;
        for (key, mut value) in fields {
            if key == "tabs"
                && let Value::Object(tabs) = &mut value
            {
                if tabs.contains_key("avoid_corners") {
                    return Err(format!(
                        "a version 13 file has no `operations[{i}].tabs.avoid_corners`, but this one does"
                    ));
                }
                tabs.insert("avoid_corners".into(), Value::Bool(false));
            }
            let after_this = key == "last_pass";
            out.insert(key, value);
            if after_this {
                out.insert("start".into(), Value::from("auto"));
                inserted = true;
            }
        }
        if !inserted {
            out.insert("start".into(), Value::from("auto"));
        }
        *op = Value::Object(out);
    }
    Ok(root)
}

/// Version 14 → 15 (#302): `holds_slug: null` at the end of each placed
/// tab. Whether a format 14 hole tab was placed to hold its slug shows
/// only in where it sits against the tool path of the Profile that cuts
/// it (#300), which only `dc-cam` builds, so the step records nothing and
/// generation keeps that rule for an unset tab: the file warns as it did.
/// Fails if a tab already has the key: that is not a version 14 file.
fn v14_to_v15(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(tabs) = root.get_mut("tabs").and_then(Value::as_array_mut) else {
        return Ok(root);
    };
    for (i, tab) in tabs.iter_mut().enumerate() {
        let Value::Object(fields) = tab else {
            return Err(format!("`tabs[{i}]` is not an object"));
        };
        if fields.contains_key("holds_slug") {
            return Err(format!(
                "a version 14 file has no `tabs[{i}].holds_slug`, but this one does"
            ));
        }
        fields.insert("holds_slug".into(), Value::Null);
    }
    Ok(root)
}

/// Version 15 → 16 (maintainer decision 2026-10-24): `vendor: null` and
/// `product_id: null` right after each tool's `name`. Fails if a tool
/// already has either key: that is not a version 15 file.
fn v15_to_v16(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(tools) = root.get_mut("tools").and_then(Value::as_array_mut) else {
        return Ok(root);
    };
    for (i, tool) in tools.iter_mut().enumerate() {
        let Value::Object(fields) = std::mem::take(tool) else {
            return Err(format!("`tools[{i}]` is not an object"));
        };
        for key in ["vendor", "product_id"] {
            if fields.contains_key(key) {
                return Err(format!(
                    "a version 15 file has no `tools[{i}].{key}`, but this one does"
                ));
            }
        }
        let mut out = Map::with_capacity(fields.len() + 2);
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
    Ok(root)
}

/// Version 16 → 17 (maintainer decision 2026-10-24):
/// `allow_tool_wider_than_hole: false` at the end of each Drill, so a hole
/// narrower than the tool is skipped with a warning rather than drilled
/// larger than drawn. Other operations are unchanged. Fails if a Drill
/// already has the key: that is not a version 16 file.
fn v16_to_v17(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(ops) = root.get_mut("operations").and_then(Value::as_array_mut) else {
        return Ok(root);
    };
    for (i, op) in ops.iter_mut().enumerate() {
        if op.get("kind").and_then(Value::as_str) != Some("drill") {
            continue;
        }
        let Value::Object(fields) = op else {
            return Err(format!("`operations[{i}]` is not an object"));
        };
        if fields.contains_key("allow_tool_wider_than_hole") {
            return Err(format!(
                "a version 16 file has no `operations[{i}].allow_tool_wider_than_hole`, but this one does"
            ));
        }
        fields.insert("allow_tool_wider_than_hole".into(), Value::Bool(false));
    }
    Ok(root)
}

/// Version 17 → 18 (maintainer decision 2026-10-24): the Bore operation,
/// `"kind": "bore"`, which no version 17 file holds, so a version 17 file
/// is already a version 18 one. The bump makes a version 17 reader refuse
/// a file with a Bore as newer instead of failing on an unknown kind
/// (ADR-0002).
fn v17_to_v18(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    Ok(root)
}

/// Version 18 → 19 (#394, maintainer decision 2026-10-25): `allowance_mm`
/// becomes `stock_to_leave_mm`, in place and with its value, in each
/// Pocket, each Bore and each Profile's non-null `last_pass`. Other
/// operations, and `through_cut_allowance_mm`, are unchanged. Fails if
/// one of them already has `stock_to_leave_mm`: that is not a version 18
/// file.
fn v18_to_v19(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(ops) = root.get_mut("operations").and_then(Value::as_array_mut) else {
        return Ok(root);
    };
    for (i, op) in ops.iter_mut().enumerate() {
        let (target, at) = match op.get("kind").and_then(Value::as_str) {
            Some("pocket" | "bore") => (op, format!("operations[{i}].")),
            Some("profile") => match op.get_mut("last_pass") {
                Some(last_pass @ Value::Object(_)) => {
                    (last_pass, format!("operations[{i}].last_pass."))
                }
                _ => continue,
            },
            _ => continue,
        };
        let Value::Object(fields) = std::mem::take(target) else {
            return Err(format!("`{at}` is not an object"));
        };
        *target = Value::Object(rename_key(
            fields,
            "allowance_mm",
            "stock_to_leave_mm",
            &at,
        )?);
    }
    Ok(root)
}

/// Version 19 → 20 (`docs/specs/phase-3/design.md`, sub-phase 3.1): the
/// V-Carve operation, `"kind": "v_carve"`, with its clearing passes, which
/// no version 19 file holds, so a version 19 file is already a version 20
/// one. The bump makes a version 19 reader refuse a file with a V-Carve as
/// newer instead of failing on an unknown kind (ADR-0002).
fn v19_to_v20(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    Ok(root)
}

/// Version 20 → 21 (`docs/specs/phase-3/design.md`, sub-phase 3.2): the
/// countersink Drill cycle, which no version 20 file holds, so a version
/// 20 file is already a version 21 one. The bump makes a version 20
/// reader refuse a file with a countersink as newer instead of failing on
/// an unknown cycle (ADR-0002).
fn v20_to_v21(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    Ok(root)
}

/// Version 21 → 22 (`docs/specs/phase-3/design.md`, sub-phase 3.3): the
/// Inlay operation kind, which no version 21 file holds, so a version 21
/// file is already a version 22 one. The bump makes a version 21 reader
/// refuse a file with an Inlay as newer instead of failing on an unknown
/// kind (ADR-0002).
fn v21_to_v22(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    Ok(root)
}

/// Version 22 → 23 (#443): a male Inlay's cut-out. Inserts `"cut_out":
/// null` after `safe_z_mm` (at the end when it is missing) in every
/// Inlay, so a version 22 file cuts the same, with the plug sawn free.
/// Other operations are unchanged. Fails if an Inlay already has
/// `cut_out`: that is not a version 22 file.
fn v22_to_v23(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(ops) = root.get_mut("operations").and_then(Value::as_array_mut) else {
        return Ok(root);
    };
    for (i, op) in ops.iter_mut().enumerate() {
        if op.get("kind").and_then(Value::as_str) != Some("inlay") {
            continue;
        }
        let Value::Object(fields) = std::mem::take(op) else {
            return Err(format!("`operations[{i}]` is not an object"));
        };
        if fields.contains_key("cut_out") {
            return Err(format!(
                "a version 22 file has no `operations[{i}].cut_out`, but this one does"
            ));
        }
        let at_end = !fields.contains_key("safe_z_mm");
        let mut out = Map::new();
        for (k, v) in fields {
            let after = k == "safe_z_mm";
            out.insert(k, v);
            if after {
                out.insert("cut_out".into(), Value::Null);
            }
        }
        if at_end {
            out.insert("cut_out".into(), Value::Null);
        }
        *op = Value::Object(out);
    }
    Ok(root)
}

/// `map` with key `from` renamed `to`, in its place. Fails if `to` is
/// already there, naming it under `at` (a path prefix such as
/// `operations[1].`).
fn rename_key(
    map: Map<String, Value>,
    from: &str,
    to: &str,
    at: &str,
) -> Result<Map<String, Value>, String> {
    if map.contains_key(to) {
        return Err(format!(
            "a version 18 file has no `{at}{to}`, but this one does"
        ));
    }
    Ok(map
        .into_iter()
        .map(|(k, v)| {
            if k == from {
                (to.to_owned(), v)
            } else {
                (k, v)
            }
        })
        .collect())
}

/// `map` with `key: value` inserted right after `after`, or at the end
/// when `after` is absent. Fails if `key` is already there, naming it
/// under `at` (a path prefix such as `tools[0].`).
fn insert_after(
    map: Map<String, Value>,
    after: &str,
    key: &str,
    value: Value,
    at: &str,
) -> Result<Map<String, Value>, String> {
    if map.contains_key(key) {
        return Err(format!(
            "a version 7 file has no `{at}{key}`, but this one does"
        ));
    }
    let mut out = Map::with_capacity(map.len() + 1);
    let mut inserted = false;
    for (k, v) in map {
        let after_this = k == after;
        out.insert(k, v);
        if after_this {
            out.insert(key.to_owned(), value.clone());
            inserted = true;
        }
    }
    if !inserted {
        out.insert(key.to_owned(), value);
    }
    Ok(out)
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
    use crate::test_support::{sample_drill, sample_project};
    use crate::{Operation, OriginXy, Side, SpindleControl, ToolChangeMode, ToolKind, load, save};
    use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
    use serde_json::json;

    const V0_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v0/sample.dcv");
    const V1_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v1/sample.dcv");
    const V2_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v2/sample.dcv");
    const V3_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v3/sample.dcv");
    const V4_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v4/sample.dcv");
    const V5_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v5/sample.dcv");

    /// The sample Pocket as raw JSON of `version`: before version 19 its
    /// `stock_to_leave_mm` is `allowance_mm` (#394).
    fn pocket_root_at(version: u32) -> Value {
        let pocket =
            serde_json::to_value(Operation::Pocket(crate::test_support::sample_pocket())).unwrap();
        if version >= 19 {
            return pocket;
        }
        let text = serde_json::to_string(&pocket)
            .unwrap()
            .replace("\"stock_to_leave_mm\"", "\"allowance_mm\"");
        serde_json::from_str(&text).unwrap()
    }

    /// The sample project as raw JSON of `version`, without the fields
    /// added after it. Before version 7 it embeds the whole GRBL
    /// definition that `grbl/v1.json` was made from.
    fn sample_root_at(version: u32) -> Value {
        let mut root = serde_json::to_value(sample_project()).unwrap();
        if version < 17 {
            for op in root["operations"].as_array_mut().unwrap() {
                op.as_object_mut()
                    .unwrap()
                    .shift_remove("allow_tool_wider_than_hole");
            }
        }
        if version < 16 {
            for tool in root["tools"].as_array_mut().unwrap() {
                let tool = tool.as_object_mut().unwrap();
                tool.shift_remove("vendor");
                tool.shift_remove("product_id");
            }
        }
        if version < 15 {
            for tab in root["tabs"].as_array_mut().unwrap() {
                tab.as_object_mut().unwrap().shift_remove("holds_slug");
            }
        }
        if version < 14 {
            for op in root["operations"].as_array_mut().unwrap() {
                op.as_object_mut().unwrap().shift_remove("start");
                if let Some(tabs) = op["tabs"].as_object_mut() {
                    tabs.shift_remove("avoid_corners");
                }
            }
        }
        if version < 13 {
            for op in root["operations"].as_array_mut().unwrap() {
                op.as_object_mut().unwrap().shift_remove("last_pass");
            }
        }
        if version < 12 {
            root.as_object_mut().unwrap().shift_remove("heights");
        }
        if version < 11 {
            root.as_object_mut().unwrap().shift_remove("tabs");
        }
        if version < 9 {
            for op in root["operations"].as_array_mut().unwrap() {
                if let Some(tabs) = op["tabs"].as_object_mut() {
                    tabs.shift_remove("on_holes");
                }
            }
        }
        if version < 8 {
            // `shift_remove`: `remove` swaps the last key in, which would
            // scramble the order the tests compare.
            let root_map = root.as_object_mut().unwrap();
            root_map.shift_remove("arc_fit_tolerance_mm");
            for tool in root_map["tools"].as_array_mut().unwrap() {
                tool.as_object_mut().unwrap().shift_remove("number");
            }
            for op in root_map["operations"].as_array_mut().unwrap() {
                let op = op.as_object_mut().unwrap();
                for key in ["machining_order", "entry", "lead", "corner_relief"] {
                    op.shift_remove(key);
                }
            }
            root_map["post_processor"]
                .as_object_mut()
                .unwrap()
                .shift_remove("use_canned_cycles");
        }
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
                matches!(&err, LoadError::Migration { from: 5, to: 6, message, .. }
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
        dc_geometry::assert_approx_eq!(project.operations[0].depth_mm().unwrap(), 18.5, 0.0);
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
        let Value::Object(root) = sample_root_at(6) else {
            panic!("an object");
        };
        let mut migrated = v6_to_v7(root).unwrap();
        migrated.insert("version".into(), Value::from(7));
        assert_eq!(Value::Object(migrated), sample_root_at(7));
    }

    /// #106: a version 6 file with no embedded post-processor is broken,
    /// not one to give a default.
    #[test]
    fn v6_without_post_processor_fails() {
        let mut root = sample_root_at(6);
        root.as_object_mut().unwrap().remove("post_processor");
        let err = migrate(root, 6).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 6, to: 7, message, .. }
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
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(
            serde_json::to_value(&loaded.project.post_processor).unwrap(),
            json!({
                "id": "grblhal",
                "definition_version": 1,
                "file_extension": "nc",
                "spindle_control": "manual",
                "spindle_on_dwell_s": 5.0,
                "tool_change_mode": "separate_programs",
                "decimal_places": { "mm": 3, "inch": 4 },
                "use_canned_cycles": false
            })
        );
    }

    /// #117: a version 6 copy predates `spindle_on_dwell.units` and
    /// `canned_cycles.reentry_gap_mm`, which every bundled definition now
    /// has. The step gives the copy the values of the time (seconds; no
    /// gap) before comparing, so the copy still matches version 1.
    #[test]
    fn v6_copy_gains_dwell_units_before_comparison() {
        assert!(
            !V6_GRBLHAL_MANUAL.contains("\"units\": \"seconds\""),
            "the fixture predates `spindle_on_dwell.units`"
        );
        assert!(!V6_GRBLHAL_MANUAL.contains("reentry_gap_mm"));
        let loaded = load(V6_GRBLHAL_MANUAL.as_bytes()).unwrap();
        assert_eq!(loaded.project.post_processor.id, "grblhal");
        assert_eq!(loaded.project.post_processor.definition_version, 1);
        assert!(!loaded.project.post_processor.use_canned_cycles);
    }

    /// #117: a version 6 copy that already names a dwell unit is not a
    /// version 6 file.
    #[test]
    fn v6_with_dwell_units_already_present_fails() {
        let message = v6_edited_error(
            "\"command\": \"G4 P{seconds}\"",
            "\"command\": \"G4 P{seconds}\", \"units\": \"seconds\"",
        );
        assert!(message.contains("units"), "{message}");
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
                ..
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

    // Phase 2, #117: version 7 → 8 adds the phase 2 fields with the values
    // that keep a phase 1 project cutting the same.

    const V7_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v7/sample.dcv");

    #[test]
    fn version_7_fixture_loads() {
        let loaded = load(V7_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
    }

    /// Design spec, "Migration v6 → v7" (version 7 → 8 here, #117): each
    /// Profile plunges, cuts each shape to full depth first, has no lead
    /// and no corner relief, exactly as phase 1 cut it; arc fitting is on
    /// at 0.01 mm; tools have no number; canned cycles are off.
    #[test]
    fn v7_profile_gains_phase_2_fields_with_defaults() {
        let root: Value = serde_json::from_slice(V7_FIXTURE).unwrap();
        let migrated = migrate(root, 7).unwrap();
        assert_eq!(migrated["arc_fit_tolerance_mm"], json!(0.01));
        assert_eq!(migrated["tools"][0]["number"], Value::Null);
        let op = &migrated["operations"][0];
        assert_eq!(op["machining_order"], "full_depth_first");
        assert_eq!(op["entry"], "plunge");
        assert_eq!(op["lead"], Value::Null);
        assert_eq!(op["corner_relief"], "none");
        assert_eq!(
            migrated["post_processor"]["use_canned_cycles"],
            json!(false)
        );
        let loaded = load(V7_FIXTURE).unwrap();
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// The new keys land where a version 8 file writes them, so a migrated
    /// file saves with the same layout as a new one.
    #[test]
    fn v7_to_v8_writes_the_defaults() {
        // The one step: `migrate` would go on to the current version.
        let Value::Object(root) = sample_root_at(7) else {
            panic!("the sample is an object");
        };
        let mut migrated = Value::Object(v7_to_v8(root).unwrap());
        migrated["version"] = Value::from(8);
        assert_eq!(migrated, sample_root_at(8));
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&sample_root_at(8)).unwrap(),
            "key order"
        );
    }

    /// An edit to a raw root, and the key it adds.
    type Edit = (fn(&mut Value), &'static str);

    #[test]
    fn v7_with_phase_2_field_already_present_fails() {
        let edits: [Edit; 4] = [
            (
                |root| root["arc_fit_tolerance_mm"] = json!(0.02),
                "arc_fit_tolerance_mm",
            ),
            (|root| root["tools"][0]["number"] = json!(1), "number"),
            (
                |root| root["operations"][0]["entry"] = json!("plunge"),
                "entry",
            ),
            (
                |root| root["post_processor"]["use_canned_cycles"] = json!(true),
                "use_canned_cycles",
            ),
        ];
        for (edit, key) in edits {
            let mut root = sample_root_at(7);
            edit(&mut root);
            let err = migrate(root, 7).unwrap_err();
            assert!(
                matches!(&err, LoadError::Migration { from: 7, to: 8, message, .. }
                    if message.contains(key)),
                "{key}: {err:?}"
            );
        }
    }

    // Version 8 → 9 (#109): holes get no tabs unless `tabs.on_holes` is
    // set, so a project from before keeps them on its holes.

    const V8_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v8/sample.dcv");

    #[test]
    fn version_8_fixture_loads() {
        let loaded = load(V8_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// #109: a version 8 Profile put its tabs on every outline it cut,
    /// holes included. The migration writes `on_holes: true` into every
    /// Profile's tabs, so the project cuts the same; a Profile with no
    /// tabs stays `null`. The key lands where a version 9 file writes it.
    #[test]
    fn v8_tabs_keep_their_holes_tabbed() {
        let root: Value = serde_json::from_slice(V8_FIXTURE).unwrap();
        let migrated = migrate(root, 8).unwrap();
        assert_eq!(migrated["operations"][0]["tabs"]["on_holes"], json!(true));

        let migrated = migrate(sample_root_at(8), 8).unwrap();
        assert_eq!(migrated, sample_root_at(CURRENT_VERSION));
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&sample_root_at(CURRENT_VERSION)).unwrap(),
            "key order"
        );

        let mut untabbed = sample_root_at(8);
        untabbed["operations"][0]["tabs"] = Value::Null;
        let migrated = migrate(untabbed, 8).unwrap();
        assert_eq!(migrated["operations"][0]["tabs"], Value::Null);
    }

    #[test]
    fn v8_with_on_holes_already_present_fails() {
        let mut root = sample_root_at(8);
        root["operations"][0]["tabs"]["on_holes"] = json!(false);
        let err = migrate(root, 8).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 8, to: 9, message, .. }
                if message.contains("on_holes")),
            "{err:?}"
        );
    }

    // Version 10 → 11 (#105): placed tabs. The tab setting stays as a
    // request, which `dreamcarve` places when it loads the project.

    const V9_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v9/sample.dcv");
    const V10_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v10/sample.dcv");

    #[test]
    fn version_9_fixture_loads() {
        let loaded = load(V9_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    #[test]
    fn version_10_fixture_loads() {
        let loaded = load(V10_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// #105: a version 10 project gains an empty list of placed tabs right
    /// after its operations; each Profile keeps its tab setting, now a
    /// request, so the project cuts the same.
    #[test]
    fn v10_project_gains_an_empty_tab_list() {
        let root: Value = serde_json::from_slice(V10_FIXTURE).unwrap();
        let mut tabs_before = root["operations"][0]["tabs"].clone();
        let migrated = migrate(root, 10).unwrap();
        assert_eq!(migrated["tabs"], json!([]));
        // Version 14 (#285) adds `avoid_corners: false` to the request.
        tabs_before["avoid_corners"] = json!(false);
        assert_eq!(migrated["operations"][0]["tabs"], tabs_before);
        let keys: Vec<&String> = migrated.as_object().unwrap().keys().collect();
        let at = |key: &str| keys.iter().position(|k| *k == key).unwrap();
        assert_eq!(at("tabs"), at("operations") + 1, "{keys:?}");
        assert_eq!(
            migrate(sample_root_at(10), 10).unwrap(),
            sample_root_at(CURRENT_VERSION)
        );
    }

    #[test]
    fn v10_with_tabs_already_present_fails() {
        let mut root = sample_root_at(10);
        root["tabs"] = json!([]);
        let err = migrate(root, 10).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 10, to: 11, message, .. }
                if message.contains("tabs")),
            "{err:?}"
        );
    }

    /// #207: version 10 only adds the `left` and `right` sides, so the
    /// migration changes nothing but the version: every version 9 file
    /// cuts the same. The bump makes a reader of version 9 refuse a file
    /// that uses the new sides, instead of failing on an unknown side.
    #[test]
    fn v9_to_v10_changes_only_the_version() {
        let root: Value = serde_json::from_slice(V9_FIXTURE).unwrap();
        let stepped = v9_to_v10(root.as_object().unwrap().clone()).unwrap();
        assert_eq!(Value::Object(stepped), root);
        for side in ["inside", "outside", "on_line"] {
            let mut root = sample_root_at(9);
            root["operations"][0]["side"] = json!(side);
            let migrated = migrate(root, 9).unwrap();
            assert_eq!(migrated["operations"][0]["side"], json!(side));
        }
        assert_eq!(
            migrate(sample_root_at(9), 9).unwrap(),
            sample_root_at(CURRENT_VERSION)
        );
    }

    /// #207: the new sides save as `"left"` and `"right"` and load back.
    #[test]
    fn left_and_right_sides_round_trip() {
        for (side, text) in [(Side::Left, "\"left\""), (Side::Right, "\"right\"")] {
            assert_eq!(serde_json::to_string(&side).unwrap(), text);
            assert_eq!(serde_json::from_str::<Side>(text).unwrap(), side);
            let mut p = sample_project();
            crate::test_support::profile_mut(&mut p).side = side;
            assert_eq!(load(save(&p).unwrap().as_bytes()).unwrap().project, p);
        }
    }

    // Version 11 → 12 (#283): the clearance, tool-change and park heights.

    const V11_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v11/sample.dcv");

    /// #283, design addendum A7: a version 11 project gains `heights`
    /// right after `origin`, with its three fields `null`, which exports
    /// what the file did; everything else is unchanged.
    #[test]
    fn v11_to_v12_adds_null_heights() {
        let root: Value = serde_json::from_slice(V11_FIXTURE).unwrap();
        let Value::Object(map) = root.clone() else {
            panic!("the fixture is an object");
        };
        // The one step: `migrate` would go on to the current version.
        let mut migrated = Value::Object(v11_to_v12(map).unwrap());
        migrated["version"] = Value::from(12);
        assert_eq!(
            migrated["heights"],
            json!({ "clearance_mm": null, "tool_change_machine_z_mm": null, "park": null })
        );
        let keys: Vec<&String> = migrated.as_object().unwrap().keys().collect();
        let heights_keys: Vec<&String> = migrated["heights"].as_object().unwrap().keys().collect();
        assert_eq!(
            heights_keys,
            ["clearance_mm", "tool_change_machine_z_mm", "park"],
            "{heights_keys:?}"
        );
        let at = |key: &str| keys.iter().position(|k| *k == key).unwrap();
        assert_eq!(at("heights"), at("origin") + 1, "{keys:?}");
        let mut without = migrated.clone();
        without.as_object_mut().unwrap().shift_remove("heights");
        without["version"] = Value::from(11);
        assert_eq!(without, root, "nothing else changes");
        assert_eq!(
            migrate(sample_root_at(11), 11).unwrap(),
            sample_root_at(CURRENT_VERSION)
        );

        let mut already = sample_root_at(11);
        already["heights"] = json!({});
        let err = migrate(already, 11).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 11, to: 12, message, .. }
                if message.contains("heights")),
            "{err:?}"
        );
    }

    // Version 12 → 13 (#284): the Profile last pass allowance.

    const V12_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v12/sample.dcv");

    #[test]
    fn version_12_fixture_loads() {
        let loaded = load(V12_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// #284: every Profile of a version 12 file gains `last_pass: null`
    /// right after `corner_relief`, so each pass cuts on the line as
    /// before; other operations are unchanged. The fixture then loads as
    /// the sample, whose program `dreamcarve`'s migration corpus test
    /// holds byte for byte.
    #[test]
    fn v12_to_v13_adds_null_last_pass() {
        let mut root: Value = serde_json::from_slice(V12_FIXTURE).unwrap();
        let profile = root["operations"][0].clone();
        let mut second = profile.clone();
        second["id"] = json!("o2");
        let pocket = pocket_root_at(12);
        let operations = root["operations"].as_array_mut().unwrap();
        operations.push(second);
        operations.push(pocket.clone());
        let migrated = migrate(root, 12).unwrap();
        let operations = migrated["operations"].as_array().unwrap();
        for op in operations.iter().take(2) {
            assert_eq!(op["last_pass"], Value::Null, "{op}");
            let keys: Vec<&String> = op.as_object().unwrap().keys().collect();
            let at = |key: &str| keys.iter().position(|k| *k == key).unwrap();
            assert_eq!(at("last_pass"), at("corner_relief") + 1, "{keys:?}");
            // Version 14 (#285) adds `start` after it.
            assert_eq!(keys.len(), at("last_pass") + 2, "{keys:?}");
        }
        assert_eq!(
            operations[2],
            pocket_root_at(CURRENT_VERSION),
            "a Pocket has no last pass"
        );

        let migrated = migrate(sample_root_at(12), 12).unwrap();
        assert_eq!(migrated, sample_root_at(CURRENT_VERSION));
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&sample_root_at(CURRENT_VERSION)).unwrap(),
            "key order"
        );
        let mut loaded = load(V12_FIXTURE).unwrap();
        assert_eq!(loaded.project, sample_project());
        assert_eq!(
            crate::test_support::profile_mut(&mut loaded.project).last_pass,
            None
        );
    }

    #[test]
    fn v12_with_last_pass_already_present_fails() {
        let mut root = sample_root_at(12);
        root["operations"][0]["last_pass"] = Value::Null;
        let err = migrate(root, 12).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 12, to: 13, message, .. }
                if message.contains("last_pass")),
            "{err:?}"
        );
    }

    /// #310 review: loaded from a file, a migration that fails on a key
    /// the file should not have says where that key is, as a schema error
    /// does.
    #[test]
    fn v12_with_last_pass_says_where() {
        let text = String::from_utf8(V12_FIXTURE.to_vec()).unwrap().replacen(
            "\"corner_relief\": \"none\"",
            "\"corner_relief\": \"none\",\n      \"last_pass\": null",
            1,
        );
        let line = text
            .lines()
            .position(|l| l.contains("\"last_pass\""))
            .unwrap()
            + 1;
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 12, to: 13, at: Some(at), .. }
                if at.line == line && at.column == 20),
            "{err:?}"
        );
        assert!(
            err.to_string()
                .contains(&format!("(line {line}, column 20)")),
            "{err}"
        );
    }

    /// #284: a last pass saves with every field and loads back.
    #[test]
    fn last_pass_round_trips() {
        let mut p = sample_project();
        crate::test_support::profile_mut(&mut p).last_pass = Some(crate::LastPass {
            stock_to_leave_mm: 0.5,
            thickness_mm: Some(1.0),
            reverse: true,
        });
        let text = save(&p).unwrap();
        assert!(
            text.contains(
                "\"last_pass\": {\n        \"stock_to_leave_mm\": 0.5,\n        \"thickness_mm\": 1.0,\n        \"reverse\": true\n      }"
            ),
            "{text}"
        );
        assert_eq!(load(text.as_bytes()).unwrap().project, p);
    }

    // Version 13 → 14 (#285): tabs off corners, and the start point.

    const V13_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v13/sample.dcv");

    #[test]
    fn version_13_fixture_loads() {
        let loaded = load(V13_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// #285: every Profile of a version 13 file gains `start: "auto"`
    /// right after `last_pass`, and its tab request, when it has one,
    /// `avoid_corners: false` at its end, so its tabs are laid and its
    /// cuts start where they were; a Profile without a request and other
    /// operations are otherwise unchanged. The fixture then loads as the
    /// sample, whose program `dreamcarve`'s migration corpus test holds
    /// byte for byte.
    #[test]
    fn v13_to_v14_adds_corner_and_start_defaults() {
        let mut root: Value = serde_json::from_slice(V13_FIXTURE).unwrap();
        let profile = root["operations"][0].clone();
        let mut untabbed = profile.clone();
        untabbed["id"] = json!("o2");
        untabbed["tabs"] = Value::Null;
        let pocket = pocket_root_at(13);
        let operations = root["operations"].as_array_mut().unwrap();
        operations.push(untabbed);
        operations.push(pocket.clone());
        let migrated = migrate(root, 13).unwrap();
        let operations = migrated["operations"].as_array().unwrap();
        for op in operations.iter().take(2) {
            assert_eq!(op["start"], json!("auto"), "{op}");
            let keys: Vec<&String> = op.as_object().unwrap().keys().collect();
            let at = |key: &str| keys.iter().position(|k| *k == key).unwrap();
            assert_eq!(at("start"), at("last_pass") + 1, "{keys:?}");
            assert_eq!(keys.len(), at("start") + 1, "{keys:?}");
        }
        let tabs = operations[0]["tabs"].as_object().unwrap();
        assert_eq!(tabs["avoid_corners"], json!(false));
        assert_eq!(
            tabs.keys().next_back().map(String::as_str),
            Some("avoid_corners")
        );
        assert_eq!(operations[1]["tabs"], Value::Null, "no request, no key");
        assert_eq!(
            operations[2],
            pocket_root_at(CURRENT_VERSION),
            "a Pocket has neither"
        );

        let migrated = migrate(sample_root_at(13), 13).unwrap();
        assert_eq!(migrated, sample_root_at(CURRENT_VERSION));
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&sample_root_at(CURRENT_VERSION)).unwrap(),
            "key order"
        );
        let mut loaded = load(V13_FIXTURE).unwrap();
        assert_eq!(loaded.project, sample_project());
        let op = crate::test_support::profile_mut(&mut loaded.project);
        assert_eq!(op.start, crate::StartAt::Auto);
        assert!(!op.tabs.as_ref().unwrap().avoid_corners);
    }

    #[test]
    fn v13_with_start_already_present_fails() {
        let mut root = sample_root_at(13);
        root["operations"][0]["start"] = json!("auto");
        let err = migrate(root, 13).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 13, to: 14, message, .. }
                if message.contains("start")),
            "{err:?}"
        );
        let mut root = sample_root_at(13);
        root["operations"][0]["tabs"]["avoid_corners"] = json!(true);
        let err = migrate(root, 13).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 13, to: 14, message, .. }
                if message.contains("tabs.avoid_corners")),
            "{err:?}"
        );
    }

    /// #285: each start saves as the spec writes it and loads back; a
    /// `near` point takes inch input as every length does.
    #[test]
    fn start_round_trips() {
        for (start, text) in [
            (crate::StartAt::Auto, "\"start\": \"auto\""),
            (crate::StartAt::FirstVertex, "\"start\": \"first_vertex\""),
            (
                crate::StartAt::Near {
                    x_mm: 0.0,
                    y_mm: 12.5,
                },
                "\"start\": {\n        \"near\": {\n          \"x_mm\": 0.0,\n          \"y_mm\": 12.5\n        }\n      }",
            ),
        ] {
            let mut p = sample_project();
            let op = crate::test_support::profile_mut(&mut p);
            op.start = start;
            op.tabs.as_mut().unwrap().avoid_corners = true;
            let saved = save(&p).unwrap();
            assert!(saved.contains(text), "{saved}");
            assert!(saved.contains("\"avoid_corners\": true"), "{saved}");
            assert_eq!(load(saved.as_bytes()).unwrap().project, p);
        }
        let inches = String::from_utf8(V13_FIXTURE.to_vec())
            .unwrap()
            .replace("\"version\": 13", "\"version\": 14")
            .replace(
                "\"last_pass\": null",
                "\"last_pass\": null,\n      \"start\": { \"near\": { \"x_mm\": \"1 in\", \"y_mm\": 2 } }",
            )
            .replace(
                "\"on_holes\": true",
                "\"on_holes\": true,\n        \"avoid_corners\": true",
            );
        let mut loaded = load(inches.as_bytes()).unwrap();
        assert_eq!(
            crate::test_support::profile_mut(&mut loaded.project).start,
            crate::StartAt::Near {
                x_mm: 25.4,
                y_mm: 2.0
            }
        );
    }

    // Version 14 → 15 (#302): whether a placed tab holds its slug.

    const V14_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v14/sample.dcv");

    #[test]
    fn version_14_fixture_loads() {
        let loaded = load(V14_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// The sample at `version` with two placed tabs, its request placed:
    /// one on the rectangle's bottom edge and one a tool radius below it.
    fn v14_root_with_tabs() -> Value {
        let mut root = sample_root_at(14);
        root["operations"][0]["tabs"] = Value::Null;
        root["tabs"] = json!([
            {
                "id": "tab1",
                "instance": "i1",
                "profile": 0,
                "x_mm": 25.0,
                "y_mm": 0.0,
                "width_mm": 6.0,
                "height_mm": 2.0,
                "shape": "rect"
            },
            {
                "id": "tab2",
                "instance": "i1",
                "profile": 0,
                "x_mm": 25.0,
                "y_mm": -3.175,
                "width_mm": 6.0,
                "height_mm": 2.0,
                "shape": "smooth"
            }
        ]);
        root
    }

    /// #302: every placed tab of a version 14 file gains `holds_slug:
    /// null` at its end, and nothing else changes. A migration step
    /// rewrites JSON only, so it cannot tell whether a tab sits on the
    /// tool path a Profile cuts round a hole, which is how format 14 told
    /// a hole tab placed with `on_holes` from one placed by hand (#300).
    /// `null` keeps that rule for the tab, so the file warns and cuts as
    /// it did (`dc-cam`'s
    /// `placed_tabs::v14_hole_tabs_on_the_path_hold_their_slugs`, and
    /// `dreamcarve`'s corpus test of format 14 projects).
    #[test]
    fn v14_placed_tabs_migrate_with_holds_slug_unset() {
        let migrated = migrate(v14_root_with_tabs(), 14).unwrap();
        assert_eq!(migrated["version"], json!(CURRENT_VERSION));
        let tabs = migrated["tabs"].as_array().unwrap();
        assert_eq!(tabs.len(), 2);
        let before = v14_root_with_tabs();
        for (i, tab) in tabs.iter().enumerate() {
            let fields = tab.as_object().unwrap();
            assert_eq!(fields["holds_slug"], Value::Null, "{tab}");
            let keys: Vec<&String> = fields.keys().collect();
            assert_eq!(
                keys.last().map(|k| k.as_str()),
                Some("holds_slug"),
                "{keys:?}"
            );
            let mut rest = fields.clone();
            rest.shift_remove("holds_slug");
            assert_eq!(Value::Object(rest), before["tabs"][i], "tab {i}");
        }
        let text = serde_json::to_string_pretty(&v14_root_with_tabs()).unwrap();
        let loaded = load(text.as_bytes()).unwrap();
        assert_eq!(loaded.problems, vec![]);
        assert!(
            loaded.project.tabs.iter().all(|t| t.holds_slug.is_none()),
            "{:?}",
            loaded.project.tabs
        );

        // A file with no tabs changes only its version.
        let migrated = migrate(sample_root_at(14), 14).unwrap();
        assert_eq!(migrated, sample_root_at(CURRENT_VERSION));
    }

    #[test]
    fn v14_with_holds_slug_already_present_fails() {
        let mut root = v14_root_with_tabs();
        root["tabs"][1]["holds_slug"] = json!(true);
        let err = migrate(root, 14).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 14, to: 15, message, .. }
                if message.contains("`tabs[1].holds_slug`")),
            "{err:?}"
        );
    }

    /// #302: loaded from a file, the version 14 file with a
    /// `holds_slug` it should not have says where that key is.
    #[test]
    fn v14_with_holds_slug_says_where() {
        let mut root = v14_root_with_tabs();
        root["tabs"][0]["holds_slug"] = json!(false);
        let text = serde_json::to_string_pretty(&root).unwrap();
        let line = text
            .lines()
            .position(|l| l.contains("\"holds_slug\""))
            .unwrap()
            + 1;
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 14, to: 15, at: Some(at), .. }
                if at.line == line && at.column == 21),
            "{err:?}"
        );
        assert!(
            err.to_string()
                .contains(&format!("(line {line}, column 21)")),
            "{err}"
        );
    }

    /// #302: each value saves as written, at the end of the tab, and
    /// loads back.
    #[test]
    fn holds_slug_round_trips() {
        for (holds_slug, text) in [
            (Some(true), "\"holds_slug\": true"),
            (Some(false), "\"holds_slug\": false"),
            (None, "\"holds_slug\": null"),
        ] {
            let mut p = sample_project();
            p.tabs = vec![crate::PlacedTab {
                holds_slug,
                ..crate::test_support::sample_tab("tab1", 25.0, 0.0)
            }];
            let saved = save(&p).unwrap();
            assert!(
                saved.contains(&format!("\"shape\": \"rect\",\n      {text}\n    }}")),
                "{saved}"
            );
            assert_eq!(load(saved.as_bytes()).unwrap().project, p);
        }
    }

    /// #302: a hand-edited `holds_slug` that is not `true`, `false` or
    /// `null`, or one left out, fails to load, naming the field and
    /// where it is in the file.
    #[test]
    fn invalid_holds_slug_says_where() {
        let mut p = sample_project();
        p.tabs = vec![
            crate::test_support::sample_tab("tab1", 25.0, 0.0),
            crate::test_support::sample_tab("tab2", 50.0, 15.0),
        ];
        let saved = save(&p).unwrap();
        // The second tab's flag, the last in the file.
        let at = saved.rfind("\"holds_slug\": false").unwrap();
        let line = saved[..at].lines().count();
        for bad in ["\"yes\"", "1", "[]"] {
            let text = format!(
                "{}\"holds_slug\": {bad}{}",
                &saved[..at],
                &saved[at + "\"holds_slug\": false".len()..]
            );
            let err = load(text.as_bytes()).unwrap_err();
            let LoadError::Schema {
                path,
                at: Some(pos),
                message,
            } = &err
            else {
                panic!("{bad}: a schema error with its place, not {err:?}");
            };
            assert_eq!(path, "tabs[1].holds_slug", "{bad}: {err}");
            assert_eq!(pos.line, line, "{bad}: {err}");
            assert!(message.contains("expected a boolean"), "{bad}: {message}");
        }
        let text = format!(
            "{}{}",
            saved[..at].trim_end().trim_end_matches(','),
            &saved[at + "\"holds_slug\": false".len()..]
        );
        let err = load(text.as_bytes()).unwrap_err();
        assert!(
            matches!(&err, LoadError::Schema { path, at: Some(_), message }
                if path == "tabs[1]" && message.contains("missing field `holds_slug`")),
            "{err:?}"
        );
    }

    /// #283: the version 11 fixture loads as the sample project, whose
    /// heights are unset, so it exports what it did before version 12.
    #[test]
    fn version_11_fixture_loads() {
        let loaded = load(V11_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project.heights, crate::Heights::unset());
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    // Version 15 → 16 (maintainer decision 2026-10-24): a tool's vendor
    // and product id.

    const V15_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v15/sample.dcv");

    #[test]
    fn version_15_fixture_loads() {
        let loaded = load(V15_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
        for tool in &loaded.project.tools {
            assert_eq!((&tool.vendor, &tool.product_id), (&None, &None));
        }
    }

    /// Every tool of a version 15 file gains `vendor: null` and
    /// `product_id: null` right after its `name`, and nothing else
    /// changes; a tool that already has either key is not a version 15
    /// tool, and the step says which.
    #[test]
    fn v15_tools_gain_no_vendor() {
        let mut before = sample_root_at(15);
        let second = before["tools"][0].clone();
        before["tools"].as_array_mut().unwrap().push(second);
        before["tools"][1]["id"] = json!("t2");
        let migrated = migrate(before.clone(), 15).unwrap();
        assert_eq!(migrated["version"], json!(CURRENT_VERSION));
        let tools = migrated["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        for (i, tool) in tools.iter().enumerate() {
            let fields = tool.as_object().unwrap();
            let keys: Vec<&str> = fields.keys().map(String::as_str).collect();
            assert_eq!(
                keys.get(1..4),
                Some(&["name", "vendor", "product_id"][..]),
                "{keys:?}"
            );
            assert_eq!(fields["vendor"], Value::Null);
            assert_eq!(fields["product_id"], Value::Null);
            let mut rest = fields.clone();
            rest.shift_remove("vendor");
            rest.shift_remove("product_id");
            assert_eq!(Value::Object(rest), before["tools"][i], "tool {i}");
        }
        // Only the tools change.
        let mut others = migrated.clone();
        others["tools"] = before["tools"].clone();
        others["version"] = json!(15);
        assert_eq!(others, before);
        assert_eq!(
            migrate(sample_root_at(15), 15).unwrap(),
            sample_root_at(CURRENT_VERSION)
        );

        for key in ["vendor", "product_id"] {
            let mut root = before.clone();
            root["tools"][1][key] = json!("x");
            let err = migrate(root, 15).unwrap_err();
            assert!(
                matches!(&err, LoadError::Migration { from: 15, to: 16, message, .. }
                    if message.contains(&format!("`tools[1].{key}`"))),
                "{key}: {err:?}"
            );
        }
    }

    /// A vendor and product id save after the name and load back.
    #[test]
    fn vendor_and_product_id_round_trip() {
        let mut project = sample_project();
        project.tools[0].vendor = Some("IDC Woodcraft".into());
        project.tools[0].product_id = Some("DC-14".into());
        let text = crate::save(&project).unwrap();
        assert!(
            text.contains(
                "\"name\": \"6.35 mm upcut\",\n      \"vendor\": \"IDC Woodcraft\",\n      \"product_id\": \"DC-14\",\n"
            ),
            "{text}"
        );
        let loaded = load(text.as_bytes()).unwrap();
        assert_eq!(loaded.project, project);
        assert_eq!(crate::save(&loaded.project).unwrap(), text);
    }

    // Version 16 → 17 (maintainer decision 2026-10-24): a Drill's
    // `allow_tool_wider_than_hole`.

    const V16_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v16/sample.dcv");

    #[test]
    fn version_16_fixture_loads() {
        let loaded = load(V16_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// The sample at version 16 with the sample Drill after its Profile.
    fn v16_root_with_drill() -> Value {
        let mut root = sample_root_at(16);
        let mut drill = serde_json::to_value(Operation::Drill(sample_drill())).unwrap();
        drill
            .as_object_mut()
            .unwrap()
            .shift_remove("allow_tool_wider_than_hole");
        root["operations"].as_array_mut().unwrap().push(drill);
        root
    }

    /// Every Drill of a version 16 file gains `allow_tool_wider_than_hole:
    /// false` at its end, and nothing else changes: the decision is that a
    /// hole the tool is too big for is not drilled unless the user says
    /// so, so such a hole in an old project is now skipped with a warning.
    #[test]
    fn v16_drill_migrates_with_allow_tool_wider_than_hole_false() {
        let before = v16_root_with_drill();
        let migrated = migrate(before.clone(), 16).unwrap();
        assert_eq!(migrated["version"], json!(CURRENT_VERSION));
        let ops = migrated["operations"].as_array().unwrap();
        assert_eq!(ops.len(), 2);
        // The Profile is unchanged.
        assert_eq!(ops[0], before["operations"][0]);
        let fields = ops[1].as_object().unwrap();
        assert_eq!(fields["allow_tool_wider_than_hole"], json!(false));
        assert_eq!(
            fields.keys().next_back().map(String::as_str),
            Some("allow_tool_wider_than_hole")
        );
        let mut rest = fields.clone();
        rest.shift_remove("allow_tool_wider_than_hole");
        assert_eq!(Value::Object(rest), before["operations"][1]);

        let text = serde_json::to_string_pretty(&before).unwrap();
        let loaded = load(text.as_bytes()).unwrap();
        assert_eq!(loaded.problems, vec![]);
        let Some(Operation::Drill(drill)) = loaded.project.operations.get(1) else {
            panic!("{:?}", loaded.project.operations);
        };
        assert!(!drill.allow_tool_wider_than_hole);
        assert_eq!(drill, &sample_drill());

        // A file with no Drill changes only its version.
        let migrated = migrate(sample_root_at(16), 16).unwrap();
        assert_eq!(migrated, sample_root_at(CURRENT_VERSION));
    }

    #[test]
    fn v16_with_allow_tool_wider_than_hole_already_present_fails() {
        let mut root = v16_root_with_drill();
        root["operations"][1]["allow_tool_wider_than_hole"] = json!(true);
        let err = migrate(root, 16).unwrap_err();
        assert!(
            matches!(&err, LoadError::Migration { from: 16, to: 17, message, .. }
                if message.contains("`operations[1].allow_tool_wider_than_hole`")),
            "{err:?}"
        );
    }

    /// Both values save as written, at the end of the Drill, and load
    /// back.
    #[test]
    fn allow_tool_wider_than_hole_round_trips() {
        for allow in [false, true] {
            let mut project = sample_project();
            let mut drill = sample_drill();
            drill.allow_tool_wider_than_hole = allow;
            project.operations.push(Operation::Drill(drill));
            let saved = save(&project).unwrap();
            assert!(
                saved.contains(&format!(
                    "\"safe_z_mm\": 5.0,\n      \"allow_tool_wider_than_hole\": {allow}\n"
                )),
                "{saved}"
            );
            assert_eq!(load(saved.as_bytes()).unwrap().project, project);
        }
    }

    const V17_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v17/sample.dcv");

    #[test]
    fn version_17_fixture_loads() {
        let loaded = load(V17_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// Format 18 (maintainer decision 2026-10-24): the Bore is a new
    /// operation kind no version 17 file holds, so the step changes only
    /// the version, with a Drill too.
    #[test]
    fn v17_to_v18_changes_only_the_version() {
        let root: Value = serde_json::from_slice(V17_FIXTURE).unwrap();
        let stepped = v17_to_v18(root.as_object().unwrap().clone()).unwrap();
        assert_eq!(Value::Object(stepped), root);
        let mut root = sample_root_at(17);
        let drill = serde_json::to_value(Operation::Drill(sample_drill())).unwrap();
        root["operations"].as_array_mut().unwrap().push(drill);
        let mut expected = root.clone();
        expected["version"] = json!(CURRENT_VERSION);
        assert_eq!(migrate(root, 17).unwrap(), expected);
        assert_eq!(
            migrate(sample_root_at(17), 17).unwrap(),
            sample_root_at(CURRENT_VERSION)
        );
    }

    /// A version 17 reader refuses a file with a Bore as newer, which is
    /// what the bump is for (ADR-0002).
    #[test]
    fn version_18_is_newer_than_17() {
        const { assert!(CURRENT_VERSION > 18) };
        let err = migrate(sample_root_at(CURRENT_VERSION), CURRENT_VERSION + 1).unwrap_err();
        assert!(matches!(err, LoadError::NewerVersion { .. }), "{err:?}");
    }

    // Version 18 → 19 (#394): `allowance_mm` is `stock_to_leave_mm` on a
    // Pocket, a Bore and a Profile's last pass.

    const V18_STOCK_TO_LEAVE_FIXTURE: &[u8] =
        include_bytes!("../tests/fixtures/dcv/v18/stock-to-leave.dcv");

    /// The version 18 fixture with its Profile's last pass, Pocket and
    /// Bore loads with each value as `stock_to_leave_mm`, and the step
    /// renames the key in place, keeping the key order, and changes
    /// nothing else.
    #[test]
    fn v18_allowance_loads_as_stock_to_leave() {
        let loaded = load(V18_STOCK_TO_LEAVE_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.problems, vec![]);
        let saved: Value = serde_json::from_str(&save(&loaded.project).unwrap()).unwrap();
        let ops = &saved["operations"];
        assert_eq!(ops[0]["last_pass"]["stock_to_leave_mm"], json!(0.5));
        assert_eq!(ops[1]["kind"], json!("pocket"));
        assert_eq!(ops[1]["stock_to_leave_mm"], json!(0.5));
        assert_eq!(ops[2]["kind"], json!("bore"));
        assert_eq!(ops[2]["stock_to_leave_mm"], json!(0.1));

        let before: Value = serde_json::from_slice(V18_STOCK_TO_LEAVE_FIXTURE).unwrap();
        let migrated = migrate(before.clone(), 18).unwrap();
        let text = serde_json::to_string_pretty(&before).unwrap();
        assert_eq!(text.matches("\"allowance_mm\"").count(), 3);
        let mut expected: Value =
            serde_json::from_str(&text.replace("\"allowance_mm\"", "\"stock_to_leave_mm\""))
                .unwrap();
        expected["version"] = json!(CURRENT_VERSION);
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&expected).unwrap()
        );
        assert_eq!(
            migrate(sample_root_at(18), 18).unwrap(),
            sample_root_at(CURRENT_VERSION)
        );
    }

    /// A Pocket, Bore or last pass that already has `stock_to_leave_mm` is
    /// not a version 18 file.
    #[test]
    fn v18_with_stock_to_leave_already_present_fails() {
        let root: Value = serde_json::from_slice(V18_STOCK_TO_LEAVE_FIXTURE).unwrap();
        for (path, named) in [
            (
                &["operations", "0", "last_pass"][..],
                "`operations[0].last_pass.stock_to_leave_mm`",
            ),
            (
                &["operations", "1"][..],
                "`operations[1].stock_to_leave_mm`",
            ),
            (
                &["operations", "2"][..],
                "`operations[2].stock_to_leave_mm`",
            ),
        ] {
            let mut root = root.clone();
            let mut at = &mut root;
            for key in path {
                at = match key.parse::<usize>() {
                    Ok(i) => &mut at[i],
                    Err(_) => &mut at[*key],
                };
            }
            at["stock_to_leave_mm"] = json!(0.0);
            let err = migrate(root, 18).unwrap_err();
            assert!(
                matches!(&err, LoadError::Migration { from: 18, to: 19, message, .. }
                    if message.contains(named)),
                "{named}: {err:?}"
            );
        }
    }

    /// A version 18 reader refuses a version 19 file as newer (ADR-0002).
    #[test]
    fn version_19_is_newer_than_18() {
        const { assert!(CURRENT_VERSION > 19) };
        let err = migrate(sample_root_at(CURRENT_VERSION), CURRENT_VERSION + 1).unwrap_err();
        assert!(matches!(err, LoadError::NewerVersion { .. }), "{err:?}");
    }

    // Version 19 → 20 (sub-phase 3.1): the V-Carve operation.

    const V19_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v19/sample.dcv");

    #[test]
    fn version_19_fixture_loads() {
        let loaded = load(V19_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// Format 20: the V-Carve is a new operation kind no version 19 file
    /// holds, so the step changes only the version, whatever operations
    /// the file has: every field of a Profile, Pocket, Drill, Bore and
    /// Engrave comes through unchanged.
    #[test]
    fn v19_to_v20_bumps_the_version_only() {
        use crate::test_support::{sample_bore, sample_engrave, sample_pocket};
        const { assert!(CURRENT_VERSION >= 20) };
        let root: Value = serde_json::from_slice(V19_FIXTURE).unwrap();
        let stepped = v19_to_v20(root.as_object().unwrap().clone()).unwrap();
        assert_eq!(Value::Object(stepped), root);

        let mut root = sample_root_at(19);
        assert_eq!(root["version"], json!(19));
        for op in [
            Operation::Pocket(sample_pocket()),
            Operation::Drill(sample_drill()),
            Operation::Bore(sample_bore()),
            Operation::Engrave(sample_engrave()),
        ] {
            let op = serde_json::to_value(op).unwrap();
            root["operations"].as_array_mut().unwrap().push(op);
        }
        let mut expected = root.clone();
        expected["version"] = json!(CURRENT_VERSION);
        let migrated = migrate(root, 19).unwrap();
        assert_eq!(migrated, expected);
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&expected).unwrap(),
            "the keys keep their order"
        );
    }

    /// A version 19 reader refuses a version 20 file as newer (ADR-0002).
    #[test]
    fn version_20_is_newer_than_19() {
        const { assert!(CURRENT_VERSION > 20) };
        let err = migrate(sample_root_at(CURRENT_VERSION), CURRENT_VERSION + 1).unwrap_err();
        assert!(matches!(err, LoadError::NewerVersion { .. }), "{err:?}");
    }

    // Version 20 → 21 (sub-phase 3.2): the countersink Drill cycle.

    const V20_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v20/sample.dcv");

    #[test]
    fn version_20_fixture_loads() {
        let loaded = load(V20_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// Format 21: the countersink is a new Drill cycle no version 20 file
    /// holds, so the step changes only the version, whatever operations
    /// the file has: every field of a Profile, Pocket, simple and peck
    /// Drill, Bore, Engrave and V-Carve comes through unchanged.
    #[test]
    fn v20_to_v21_bumps_the_version_only() {
        use crate::model::{DrillCycle, PeckRetract};
        use crate::test_support::{sample_bore, sample_engrave, sample_pocket, sample_v_carve};
        const { assert!(CURRENT_VERSION >= 21) };
        let root: Value = serde_json::from_slice(V20_FIXTURE).unwrap();
        let stepped = v20_to_v21(root.as_object().unwrap().clone()).unwrap();
        assert_eq!(Value::Object(stepped), root);

        let mut root = sample_root_at(20);
        assert_eq!(root["version"], json!(20));
        let peck = crate::model::DrillOp {
            cycle: DrillCycle::Peck {
                peck_depth_mm: Some(1.0),
                retract: PeckRetract::ChipBreak { gap_mm: 0.5 },
            },
            ..sample_drill()
        };
        for op in [
            Operation::Pocket(sample_pocket()),
            Operation::Drill(sample_drill()),
            Operation::Drill(peck),
            Operation::Bore(sample_bore()),
            Operation::Engrave(sample_engrave()),
            Operation::VCarve(sample_v_carve()),
        ] {
            let op = serde_json::to_value(op).unwrap();
            root["operations"].as_array_mut().unwrap().push(op);
        }
        let mut expected = root.clone();
        expected["version"] = json!(CURRENT_VERSION);
        let migrated = migrate(root, 20).unwrap();
        assert_eq!(migrated, expected);
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&expected).unwrap(),
            "the keys keep their order"
        );
    }

    /// A version 20 reader refuses a version 21 file as newer (ADR-0002).
    #[test]
    fn version_21_is_newer_than_20() {
        const { assert!(CURRENT_VERSION > 21) };
        let err = migrate(sample_root_at(CURRENT_VERSION), CURRENT_VERSION + 1).unwrap_err();
        assert!(matches!(err, LoadError::NewerVersion { .. }), "{err:?}");
    }

    // Version 21 → 22 (sub-phase 3.3): the Inlay operation kind.

    const V21_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v21/sample.dcv");

    #[test]
    fn version_21_fixture_loads() {
        let text = std::str::from_utf8(V21_FIXTURE).unwrap();
        assert!(text.contains("\"version\": 21,"), "a version 21 file");
        let loaded = load(V21_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// Format 22: the Inlay is a new operation kind no version 21 file
    /// holds, so the step changes only the version, whatever operations
    /// the file has: every field of a Profile, Pocket, simple, peck and
    /// countersink Drill, Bore, Engrave and V-Carve, clearing passes
    /// included, comes through unchanged.
    #[test]
    fn v21_to_v22_bumps_the_version_only() {
        use crate::ids::ToolId;
        use crate::model::{
            ClearanceStrategy, ClearingPass, CutDirection, DrillCycle, DrillOp, Entry, PeckRetract,
            VCarveOp,
        };
        use crate::test_support::{sample_bore, sample_engrave, sample_pocket, sample_v_carve};
        const { assert!(CURRENT_VERSION >= 22) };
        let root: Value = serde_json::from_slice(V21_FIXTURE).unwrap();
        let stepped = v21_to_v22(root.as_object().unwrap().clone()).unwrap();
        assert_eq!(Value::Object(stepped), root);

        let mut root = sample_root_at(21);
        assert_eq!(root["version"], json!(21));
        let peck = DrillOp {
            cycle: DrillCycle::Peck {
                peck_depth_mm: Some(1.0),
                retract: PeckRetract::ChipBreak { gap_mm: 0.5 },
            },
            ..sample_drill()
        };
        let countersink = DrillOp {
            tool: ToolId::new("t2"),
            depth_mm: None,
            cycle: DrillCycle::Countersink {
                diameter_mm: 6.0,
                start_depth_mm: 0.0,
                start_plane_cleared_externally: false,
            },
            ..sample_drill()
        };
        let floored = VCarveOp {
            flat_depth_mm: Some(1.0),
            clearing: vec![ClearingPass {
                tool: ToolId::new("t1"),
                strategy: ClearanceStrategy::Offset,
                direction: CutDirection::Climb,
                entry: Entry::Plunge,
            }],
            ..sample_v_carve()
        };
        for op in [
            Operation::Pocket(sample_pocket()),
            Operation::Drill(sample_drill()),
            Operation::Drill(peck),
            Operation::Drill(countersink),
            Operation::Bore(sample_bore()),
            Operation::Engrave(sample_engrave()),
            Operation::VCarve(sample_v_carve()),
            Operation::VCarve(floored),
        ] {
            let op = serde_json::to_value(op).unwrap();
            root["operations"].as_array_mut().unwrap().push(op);
        }
        let mut expected = root.clone();
        expected["version"] = json!(CURRENT_VERSION);
        let migrated = migrate(root, 21).unwrap();
        assert_eq!(migrated, expected);
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&expected).unwrap(),
            "the keys keep their order"
        );
    }

    /// A version 21 reader refuses a version 22 file as newer (ADR-0002).
    #[test]
    fn version_22_is_newer_than_21() {
        const { assert!(CURRENT_VERSION > 22) };
        let err = migrate(sample_root_at(CURRENT_VERSION), CURRENT_VERSION + 1).unwrap_err();
        assert!(matches!(err, LoadError::NewerVersion { .. }), "{err:?}");
    }

    // Version 22 → 23 (#443): a male Inlay's cut-out.

    const V22_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v22/sample.dcv");

    #[test]
    fn version_22_fixture_loads() {
        let text = std::str::from_utf8(V22_FIXTURE).unwrap();
        assert!(text.contains("\"version\": 22,"), "a version 22 file");
        let loaded = load(V22_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// Format 23: every Inlay gains `"cut_out": null` right after its
    /// `safe_z_mm`, whatever its side and clearing; no other operation
    /// changes, and the keys keep their order.
    #[test]
    fn v22_to_v23_adds_a_null_cut_out() {
        use crate::ids::ToolId;
        use crate::model::{
            ClearanceStrategy, ClearingPass, CutDirection, Entry, InlayOp, InlaySide,
        };
        use crate::test_support::{sample_inlay, sample_pocket, sample_v_carve};
        const { assert!(CURRENT_VERSION >= 23) };
        let fixture: Value = serde_json::from_slice(V22_FIXTURE).unwrap();
        let stepped = v22_to_v23(fixture.as_object().unwrap().clone()).unwrap();
        assert_eq!(
            Value::Object(stepped),
            serde_json::from_slice::<Value>(V22_FIXTURE).unwrap(),
            "a file with no Inlay is unchanged"
        );

        let male = InlayOp {
            side: InlaySide::Male,
            clearing: vec![ClearingPass {
                tool: ToolId::new("t1"),
                strategy: ClearanceStrategy::Offset,
                direction: CutDirection::Climb,
                entry: Entry::Plunge,
            }],
            ..sample_inlay()
        };
        let mut root = sample_root_at(22);
        let mut expected = sample_root_at(CURRENT_VERSION);
        for op in [
            Operation::Pocket(sample_pocket()),
            Operation::Inlay(sample_inlay()),
            Operation::VCarve(sample_v_carve()),
            Operation::Inlay(male),
        ] {
            let current = serde_json::to_value(op).unwrap();
            let mut old = current.clone();
            if let Some(fields) = old.as_object_mut() {
                fields.shift_remove("cut_out");
                // Format 24 (#448) adds it after.
                fields.shift_remove("outline_rounding_mm");
            }
            root["operations"].as_array_mut().unwrap().push(old);
            expected["operations"].as_array_mut().unwrap().push(current);
        }
        for op in expected["operations"].as_array().unwrap() {
            if op["kind"] == json!("inlay") {
                let keys: Vec<&String> = op.as_object().unwrap().keys().collect();
                assert_eq!(keys.last().map(|k| k.as_str()), Some("cut_out"));
                assert_eq!(op["cut_out"], Value::Null);
            }
        }
        let migrated = migrate(root, 22).unwrap();
        assert_eq!(migrated, expected);
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&expected).unwrap(),
            "the keys keep their order"
        );

        // A version 22 Inlay that already has a cut-out is not one.
        let mut bad = sample_root_at(22);
        bad["operations"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::to_value(Operation::Inlay(sample_inlay())).unwrap());
        let err = migrate(bad, 22).unwrap_err();
        assert!(
            matches!(err, LoadError::Migration { from: 22, .. }),
            "{err:?}"
        );
    }

    /// A version 22 reader refuses a version 23 file as newer (ADR-0002).
    #[test]
    fn version_23_is_newer_than_22() {
        const { assert!(CURRENT_VERSION > 22) };
        let err = migrate(sample_root_at(CURRENT_VERSION), CURRENT_VERSION + 1).unwrap_err();
        assert!(matches!(err, LoadError::NewerVersion { .. }), "{err:?}");
    }

    // Version 23 → 24 (#448): a male Inlay's outline rounding.

    const V23_FIXTURE: &[u8] = include_bytes!("../tests/fixtures/dcv/v23/sample.dcv");

    #[test]
    fn version_23_fixture_loads() {
        let text = std::str::from_utf8(V23_FIXTURE).unwrap();
        assert!(text.contains("\"version\": 23,"), "a version 23 file");
        let loaded = load(V23_FIXTURE).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project, sample_project());
        assert_eq!(loaded.problems, vec![]);
    }

    /// Format 24: every Inlay gains `"outline_rounding_mm": null` right
    /// after its `margin_mm`, whatever its side, clearing and cut-out; no
    /// other operation changes, and the keys keep their order. A version
    /// 23 Inlay that already has the field is not one.
    #[test]
    fn v23_to_v24_adds_null_outline_rounding() {
        use crate::ids::ToolId;
        use crate::model::{
            ClearanceStrategy, ClearingPass, CutDirection, Entry, InlayCutOut, InlayOp,
            InlaySide,
        };
        use crate::test_support::{sample_inlay, sample_pocket, sample_v_carve};
        assert_eq!(CURRENT_VERSION, 24);
        let fixture: Value = serde_json::from_slice(V23_FIXTURE).unwrap();
        let stepped = v23_to_v24(fixture.as_object().unwrap().clone()).unwrap();
        assert_eq!(
            Value::Object(stepped),
            serde_json::from_slice::<Value>(V23_FIXTURE).unwrap(),
            "a file with no Inlay is unchanged"
        );

        let male = InlayOp {
            side: InlaySide::Male,
            clearing: vec![ClearingPass {
                tool: ToolId::new("t1"),
                strategy: ClearanceStrategy::Offset,
                direction: CutDirection::Climb,
                entry: Entry::Plunge,
            }],
            cut_out: Some(InlayCutOut {
                tool: ToolId::new("t1"),
                tabs: None,
                through_cut_allowance_mm: 0.2,
            }),
            ..sample_inlay()
        };
        let mut root = sample_root_at(23);
        let mut expected = sample_root_at(CURRENT_VERSION);
        for op in [
            Operation::Pocket(sample_pocket()),
            Operation::Inlay(sample_inlay()),
            Operation::VCarve(sample_v_carve()),
            Operation::Inlay(male),
        ] {
            let current = serde_json::to_value(op).unwrap();
            let mut old = current.clone();
            if let Some(fields) = old.as_object_mut() {
                fields.shift_remove("outline_rounding_mm");
            }
            root["operations"].as_array_mut().unwrap().push(old);
            expected["operations"].as_array_mut().unwrap().push(current);
        }
        for op in expected["operations"].as_array().unwrap() {
            if op["kind"] == json!("inlay") {
                let keys: Vec<&str> = op
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect();
                let at = keys.iter().position(|k| *k == "margin_mm").unwrap();
                assert_eq!(keys.get(at + 1), Some(&"outline_rounding_mm"));
                assert_eq!(op["outline_rounding_mm"], Value::Null);
            }
        }
        let migrated = migrate(root, 23).unwrap();
        assert_eq!(migrated, expected);
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&expected).unwrap(),
            "the keys keep their order"
        );

        let mut bad = sample_root_at(23);
        bad["operations"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::to_value(Operation::Inlay(sample_inlay())).unwrap());
        let err = migrate(bad, 23).unwrap_err();
        assert!(
            matches!(err, LoadError::Migration { from: 23, .. }),
            "{err:?}"
        );
    }

    /// A version 23 reader refuses a version 24 file as newer (ADR-0002).
    #[test]
    fn version_24_is_newer_than_23() {
        assert_eq!(CURRENT_VERSION, 24);
        let err = migrate(sample_root_at(CURRENT_VERSION), CURRENT_VERSION + 1).unwrap_err();
        assert!(matches!(err, LoadError::NewerVersion { .. }), "{err:?}");
    }

    #[test]
    fn current_version_is_unchanged() {
        let root = serde_json::to_value(sample_project()).unwrap();
        assert_eq!(migrate(root.clone(), CURRENT_VERSION).unwrap(), root);
    }
}
