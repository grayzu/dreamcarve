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
//! - **24**: adds a male Inlay's outline rounding (#448),
//!   `"outline_rounding_mm"`, after `margin_mm`: `null` or a length above
//!   0. The migration writes `"outline_rounding_mm": null` in every
//!   Inlay. The male's band edge changed in the same release (it is cut
//!   by the end mill when a clearing pass fits), so a version 23 male
//!   exports what the same project written at version 24 does.
//! - **25**: one Inlay cuts both boards (#498): each Inlay's `side` is
//!   replaced, in its place, by `"male_board": { "width_mm",
//!   "height_mm", "thickness_mm" }`. A `"female"` Inlay gets all three
//!   `null`: its male is not cut until the user measures the male board,
//!   so the file exports the female programs it did. A `"male"` Inlay gets
//!   the stock's width, height and thickness, and the project keeps its
//!   stock, so its male programs are cut on the same board, byte for
//!   byte; it now also cuts the female on the stock, which
//!   [`migrate_with_notes`] notes ([`MigrationNote::MaleInlayGainsFemale`]).
//! - **26**: the Inlay's drawn boundary (#449, #602): `"boundary": []`
//!   after `outline_rounding_mm` in every Inlay. Empty, the male's band
//!   is `margin_mm` and `outline_rounding_mm` as it was, so the file
//!   exports what it did, byte for byte.
//! - **27**: bitmaps and the Photo V-Carve (sub-phase 3.5, #644):
//!   `"bitmaps": []` after `instances`. A version 26 file holds no
//!   picture and no `photo_v_carve` operation, so it exports what it did,
//!   byte for byte; one that holds either is refused.
//! - **28**: the tapered ball nose cutter (sub-phase 3.6, #677). Only the
//!   version changes: a version 27 file holds no `tapered_ball_nose`, so
//!   it exports what it did, byte for byte; one that holds one is refused.

use crate::ids::OperationId;
use crate::post_processor::{
    BundledDefinition, PostProcessorError, SETTING_PATHS, V6_SETTING_COUNT, bundled_definition,
    bundled_definitions,
};
use crate::serialize::LoadError;
use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
use serde_json::{Map, Value};

/// The `.dcv` schema version this build writes.
pub const CURRENT_VERSION: u32 = 28;

/// Something a migration changed that the user should hear about, though
/// the file loads (#498).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationNote {
    /// A version 24 (or earlier) male Inlay, one side of a two-file
    /// pair, now cuts both boards: its male on a male board the size of
    /// the stock, exactly as before, and the female on the stock too.
    MaleInlayGainsFemale {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
    },
}

impl std::fmt::Display for MigrationNote {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MaleInlayGainsFemale { operation, name } => write!(
                f,
                "operations[{operation}] `{name}` was the male of a two-file inlay pair (format 24); one Inlay now cuts both boards, so this file cuts its male on a male board the size of the stock, as before, and also the female on the stock. Run only its male programs (export --board male), or make one project for the pair from the design and set its male_board"
            ),
        }
    }
}

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
        23 => Some(v23_to_v24),
        24 => Some(v24_to_v25),
        25 => Some(v25_to_v26),
        26 => Some(v26_to_v27),
        27 => Some(v27_to_v28),
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

/// [`migrate`], and what the migration changed that the user should hear
/// about ([`MigrationNote`]), in operation order. [`crate::load`] keeps
/// the notes in [`crate::Loaded::notes`].
///
/// # Errors
///
/// As [`migrate`].
pub fn migrate_with_notes(
    root: Value,
    from_version: u32,
) -> Result<(Value, Vec<MigrationNote>), LoadError> {
    let notes = if (22..25).contains(&from_version) {
        male_inlays(&root)
    } else {
        Vec::new()
    };
    Ok((migrate(root, from_version)?, notes))
}

/// [`MigrationNote::MaleInlayGainsFemale`] for each `"side": "male"` Inlay
/// of a version 22 to 24 root (the formats with a side), in operation
/// order.
fn male_inlays(root: &Value) -> Vec<MigrationNote> {
    let Some(ops) = root.get("operations").and_then(Value::as_array) else {
        return Vec::new();
    };
    ops.iter()
        .filter(|op| {
            op.get("kind").and_then(Value::as_str) == Some("inlay")
                && op.get("side").and_then(Value::as_str) == Some("male")
        })
        .map(|op| MigrationNote::MaleInlayGainsFemale {
            operation: OperationId::new(op.get("id").and_then(Value::as_str).unwrap_or_default()),
            name: op
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        })
        .collect()
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

/// Version 23 → 24 (#448): a male Inlay's outline rounding. Inserts
/// `"outline_rounding_mm": null` after `margin_mm` (at the end when it is
/// missing) in every Inlay. Other operations are unchanged. Fails if an
/// Inlay already has `outline_rounding_mm`: that is not a version 23
/// file.
fn v23_to_v24(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
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
        if fields.contains_key("outline_rounding_mm") {
            return Err(format!(
                "a version 23 file has no `operations[{i}].outline_rounding_mm`, but this one does"
            ));
        }
        let at_end = !fields.contains_key("margin_mm");
        let mut out = Map::new();
        for (k, v) in fields {
            let after = k == "margin_mm";
            out.insert(k, v);
            if after {
                out.insert("outline_rounding_mm".into(), Value::Null);
            }
        }
        if at_end {
            out.insert("outline_rounding_mm".into(), Value::Null);
        }
        *op = Value::Object(out);
    }
    Ok(root)
}

/// Version 24 → 25 (#498): replaces each Inlay's `side`, in its place,
/// with its `male_board`: all `null` for a `"female"`, the stock's
/// `width_mm`, `height_mm` and `thickness_mm` for a `"male"`. The stock is
/// kept. A side that is neither, an Inlay that already has a male board,
/// or a male whose stock has no width or height, is refused.
fn v24_to_v25(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let stock = root.get("stock").cloned().unwrap_or(Value::Null);
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
        if fields.contains_key("male_board") {
            return Err(format!(
                "a version 24 file has no `operations[{i}].male_board`, but this one does"
            ));
        }
        let board = match fields.get("side").and_then(Value::as_str) {
            Some("female") => serde_json::json!({
                "width_mm": null,
                "height_mm": null,
                "thickness_mm": null,
            }),
            Some("male") => {
                let field = |key: &str| {
                    stock
                        .get(key)
                        .cloned()
                        .ok_or_else(|| format!("`stock.{key}` is missing"))
                };
                let width = field("width_mm")?;
                let height = field("height_mm")?;
                if !(width.is_number() && height.is_number()) {
                    return Err("`stock.width_mm` and `stock.height_mm` must be numbers".into());
                }
                serde_json::json!({
                    "width_mm": width,
                    "height_mm": height,
                    "thickness_mm": stock.get("thickness_mm").cloned().unwrap_or(Value::Null),
                })
            }
            _ => {
                return Err(format!(
                    "`operations[{i}].side` is not \"female\" or \"male\""
                ));
            }
        };
        let mut out = Map::new();
        let mut board = Some(board);
        for (k, v) in fields {
            if k == "side" {
                if let Some(board) = board.take() {
                    out.insert("male_board".into(), board);
                }
            } else {
                out.insert(k, v);
            }
        }
        *op = Value::Object(out);
    }
    Ok(root)
}

/// Version 25 → 26 (#449, #602): an Inlay's drawn boundary. Inserts
/// `"boundary": []` after `outline_rounding_mm` (at the end when it is
/// missing) in every Inlay, so a version 25 file's band is still its
/// margin and rounding. Other operations are unchanged. Fails if an Inlay
/// already has `boundary`: that is not a version 25 file.
fn v25_to_v26(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
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
        if fields.contains_key("boundary") {
            return Err(format!(
                "a version 25 file has no `operations[{i}].boundary`, but this one does"
            ));
        }
        let at_end = !fields.contains_key("outline_rounding_mm");
        let mut out = Map::new();
        for (k, v) in fields {
            let after = k == "outline_rounding_mm";
            out.insert(k, v);
            if after {
                out.insert("boundary".into(), Value::Array(Vec::new()));
            }
        }
        if at_end {
            out.insert("boundary".into(), Value::Array(Vec::new()));
        }
        *op = Value::Object(out);
    }
    Ok(root)
}

/// Version 26 → 27 (sub-phase 3.5, #644): bitmaps and the Photo V-Carve.
/// Inserts `"bitmaps": []` after `instances` (at the end when it is
/// missing). Operations are unchanged. Fails if the root already has
/// `bitmaps`, or an operation is a `photo_v_carve`: the list and the kind
/// arrive together in 27, so that is not a version 26 file.
fn v26_to_v27(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    if root.contains_key("bitmaps") {
        return Err("a version 26 file has no `bitmaps`, but this one does".into());
    }
    let photo = root
        .get("operations")
        .and_then(Value::as_array)
        .and_then(|ops| {
            ops.iter()
                .position(|op| op.get("kind").and_then(Value::as_str) == Some("photo_v_carve"))
        });
    if let Some(i) = photo {
        return Err(format!(
            "`operations[{i}]` is a photo_v_carve, which a version 26 file cannot hold"
        ));
    }
    let at_end = !root.contains_key("instances");
    let mut out = Map::new();
    for (k, v) in root {
        let after = k == "instances";
        out.insert(k, v);
        if after {
            out.insert("bitmaps".into(), Value::Array(Vec::new()));
        }
    }
    if at_end {
        out.insert("bitmaps".into(), Value::Array(Vec::new()));
    }
    Ok(out)
}

/// Version 27 → 28 (`docs/specs/phase-3/design.md`, sub-phase 3.6,
/// #677): the tapered ball nose cutter, which no version 27 file holds,
/// so a version 27 file is already a version 28 one. The bump makes a
/// version 27 reader refuse a file with a tapered ball nose as newer
/// instead of failing on an unknown cutter (ADR-0002). Fails if a tool's
/// cutter is already a `tapered_ball_nose`: that is not a version 27 file.
fn v27_to_v28(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let tapered = root
        .get("tools")
        .and_then(Value::as_array)
        .and_then(|tools| {
            tools.iter().position(|tool| {
                tool.get("cutter")
                    .and_then(Value::as_object)
                    .is_some_and(|cutter| cutter.contains_key("tapered_ball_nose"))
            })
        });
    if let Some(i) = tapered {
        return Err(format!(
            "`tools[{i}].cutter` is a tapered_ball_nose, which a version 27 file cannot hold"
        ));
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
mod tests;
