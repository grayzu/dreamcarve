// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! In-memory `.dcv` schema migrations (ADR-0002; `docs/specs/phase-1/design.md`,
//! "Data model → dc-project").
//!
//! Each step rewrites the raw JSON of version N−1 into version N, so a step
//! never depends on the current Rust types. Steps run in sequence up to
//! [`CURRENT_VERSION`].
//!
//! What each format version changed for the user is in `CHANGELOG.md`.
//! What its migration does to a file is on its step, and [`STEPS`] lists
//! the steps by the version they start from.

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
/// the file loads.
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

/// The steps: `STEPS[n]` migrates from version `n`. The array is
/// [`CURRENT_VERSION`] long, so a version bump without a step does not
/// compile.
const STEPS: [Step; CURRENT_VERSION as usize] = [
    v0_to_v1, v1_to_v2, v2_to_v3, v3_to_v4, v4_to_v5, v5_to_v6, v6_to_v7, v7_to_v8, v8_to_v9,
    v9_to_v10, v10_to_v11, v11_to_v12, v12_to_v13, v13_to_v14, v14_to_v15, v15_to_v16, v16_to_v17,
    v17_to_v18, v18_to_v19, v19_to_v20, v20_to_v21, v21_to_v22, v22_to_v23, v23_to_v24, v24_to_v25,
    v25_to_v26, v26_to_v27, v27_to_v28,
];

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
        let failed = |message: String| LoadError::Migration {
            from,
            to: from + 1,
            message,
            at: None,
        };
        let step = usize::try_from(from)
            .ok()
            .and_then(|from| STEPS.get(from))
            .ok_or_else(|| failed("no migration step is defined".into()))?;
        obj = step(obj).map_err(failed)?;
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

/// Version 0 → 1: adds `flatten_tolerance_mm` after `display_units`. A
/// version 0 file flattened curves at a fixed 0.01 mm.
fn v0_to_v1(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let tolerance = Value::from(FLATTEN_TOLERANCE_DEFAULT_MM);
    add_field(
        0,
        &mut root,
        Some("display_units"),
        FLATTEN_TOLERANCE_KEY,
        tolerance,
        "",
    )?;
    Ok(root)
}

/// Version 1 → 2: adds `spindle_control: "automatic"` at the end of
/// `post_processor.feed_speed`, which is how every earlier file was
/// exported.
fn v1_to_v2(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let feed_speed = root
        .get_mut("post_processor")
        .and_then(|post| post.get_mut("feed_speed"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "`post_processor.feed_speed` is missing or not an object".to_owned())?;
    let automatic = Value::from("automatic");
    add_field(1, feed_speed, None, SPINDLE_CONTROL_KEY, automatic, "")?;
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
/// `post_processor.tool_change`: a project whose operations use more than
/// one tool exports one program per tool run.
fn v4_to_v5(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let tool_change = root
        .get_mut("post_processor")
        .and_then(|post| post.get_mut("tool_change"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "`post_processor.tool_change` is missing or not an object".to_owned())?;
    let separate = Value::from("separate_programs");
    add_field(
        4,
        tool_change,
        None,
        TOOL_CHANGE_MODE_KEY,
        separate,
        "tool_change.",
    )?;
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

/// Version 6 → 7 (ADR-0004): the embedded post-processor becomes a
/// reference to the bundled definition's version 1 and the user's
/// settings. Fails, naming every difference, when the rest of the copy is
/// not exactly that definition: swapping in another dialect would change
/// the G-code without the user knowing.
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
/// bundled version 1 it was made from: the dwell word was always seconds
/// on GRBL and grblHAL, and no controller then documented a chip-break
/// re-entry gap; both document `G53`. These are definition facts, so they
/// never appear in a version 7 or later project; the comparison is the
/// only place a version 6 copy meets them.
fn upgrade_embedded_copy(copy: &mut Value) -> Result<(), String> {
    let dwell = copy
        .get_mut("feed_speed")
        .and_then(|feed| feed.get_mut("spindle_on_dwell"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "`post_processor.feed_speed.spindle_on_dwell` is missing".to_owned())?;
    add_field(
        6,
        dwell,
        None,
        "units",
        Value::from("seconds"),
        "post_processor.feed_speed.spindle_on_dwell.",
    )?;
    // The machine-coordinate command is a fact of the controller, not of
    // the copy, so it is the bundled version 1's (`"G53"` on grblHAL and
    // GRBL, the only definitions a version 6 file held), and nothing
    // writes it unless a project sets the height.
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
    add_field(
        6,
        work_coordinates,
        None,
        "machine_command",
        machine_command,
        "post_processor.work_coordinates.",
    )?;
    if let Some(cycles) = copy.get_mut("canned_cycles").and_then(Value::as_object_mut) {
        add_field(
            6,
            cycles,
            None,
            "reentry_gap_mm",
            Value::Null,
            "post_processor.canned_cycles.",
        )?;
    }
    Ok(())
}

/// Version 7 → 8 (`docs/specs/phase-2/design.md`, "Migration v6 → v7",
/// which is 7 → 8 here since the post-processor reference took version
/// 7): the phase 2 fields, with the values that keep a phase 1 project
/// cutting the same. Each new key lands where a version 8 file writes it.
/// Pocket, Drill and Engrave operations exist from version 8 on, so an
/// operation that is not a Profile is refused.
fn v7_to_v8(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    add_field(
        7,
        &mut root,
        Some("flatten_tolerance_mm"),
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
            add_field(7, fields, Some("name"), "number", Value::Null, &at)?;
        }
    }
    if let Some(ops) = root.get_mut("operations").and_then(Value::as_array_mut) {
        for (i, op) in ops.iter_mut().enumerate() {
            let fields = op
                .as_object_mut()
                .ok_or_else(|| format!("`operations[{i}]` is not an object"))?;
            if fields.get("kind").and_then(Value::as_str) != Some("profile") {
                return Err(format!(
                    "`operations[{i}].kind` is {}; a version 7 file holds only `\"profile\"` operations",
                    fields
                        .get("kind")
                        .map_or_else(|| "missing".to_owned(), Value::to_string)
                ));
            }
            let at = format!("operations[{i}].");
            for (key, value) in [
                ("machining_order", Value::from("full_depth_first")),
                ("entry", Value::from("plunge")),
                ("lead", Value::Null),
                ("corner_relief", Value::from("none")),
            ] {
                add_field(7, fields, None, key, value, &at)?;
            }
        }
    }
    let post = root
        .get_mut("post_processor")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| "`post_processor` is missing or not an object".to_owned())?;
    add_field(
        7,
        post,
        None,
        "use_canned_cycles",
        Value::from(false),
        "post_processor.",
    )?;
    Ok(root)
}

/// Version 8 → 9: `on_holes: true` at the end of each Profile's `tabs`
/// object, so a project whose holes had tabs keeps them: a Profile now
/// leaves its holes without tabs unless `on_holes` is set. A Profile with
/// `tabs: null` and the other operations are unchanged.
fn v8_to_v9(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    each_operation(&mut root, &["profile"], |i, profile| {
        let Some(tabs) = profile.get_mut("tabs").and_then(Value::as_object_mut) else {
            return Ok(());
        };
        let at = format!("operations[{i}].tabs.");
        add_field(8, tabs, None, "on_holes", Value::from(true), &at)
    })?;
    Ok(root)
}

/// Version 9 → 10: Left and Right are new values of a Profile's `side`,
/// an existing field, so a version 9 file is already a version 10 one.
/// The bump makes an older reader refuse a file that uses them.
fn v9_to_v10(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    Ok(root)
}

/// Version 10 → 11 (ADR-0005): `tabs: []` after `operations`, the
/// project's placed tabs, none yet. Each Profile's `tabs` setting is left
/// as it is: it is now a tab request, placed when the project is cut or
/// saved by `dreamcarve tabs`, since placing it needs the Profile's tool
/// paths, which only `dc-cam` builds.
fn v10_to_v11(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let none = Value::Array(Vec::new());
    add_field(10, &mut root, Some("operations"), "tabs", none, "")?;
    Ok(root)
}

/// Version 11 → 12: `heights` after `origin`, with the clearance height,
/// the tool-change machine Z and the park point all `null`, so the
/// project exports what it did.
fn v11_to_v12(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let mut heights = Map::with_capacity(3);
    for key in ["clearance_mm", "tool_change_machine_z_mm", "park"] {
        heights.insert(key.into(), Value::Null);
    }
    let heights = Value::Object(heights);
    add_field(11, &mut root, Some("origin"), "heights", heights, "")?;
    Ok(root)
}

/// Version 12 → 13: `last_pass: null` right after each Profile's
/// `corner_relief`, so every pass cuts on the part's path as before. Other
/// operations are unchanged.
fn v12_to_v13(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    add_operation_field(
        12,
        &mut root,
        "profile",
        "corner_relief",
        "last_pass",
        &Value::Null,
    )?;
    Ok(root)
}

/// Version 13 → 14: `avoid_corners: false` at the end of each Profile's
/// non-null `tabs` request, so its tabs are laid where they were, and
/// `start: "auto"` right after its `last_pass`, so each cut starts where
/// it did. Other operations are unchanged.
fn v13_to_v14(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    each_operation(&mut root, &["profile"], |i, profile| {
        let at = format!("operations[{i}].");
        let auto = Value::from("auto");
        add_field(13, profile, Some("last_pass"), "start", auto, &at)?;
        let Some(tabs) = profile.get_mut("tabs").and_then(Value::as_object_mut) else {
            return Ok(());
        };
        let at = format!("{at}tabs.");
        add_field(13, tabs, None, "avoid_corners", Value::Bool(false), &at)
    })?;
    Ok(root)
}

/// Version 14 → 15: `holds_slug: null` at the end of each placed tab.
/// Whether a format 14 hole tab was placed to hold its slug shows only in
/// where it sits against the tool path of the Profile that cuts it, which
/// only `dc-cam` builds, so the step records nothing and generation keeps
/// that rule for an unset tab: the file warns as it did.
fn v14_to_v15(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(tabs) = root.get_mut("tabs").and_then(Value::as_array_mut) else {
        return Ok(root);
    };
    for (i, tab) in tabs.iter_mut().enumerate() {
        let Value::Object(fields) = tab else {
            return Err(format!("`tabs[{i}]` is not an object"));
        };
        let at = format!("tabs[{i}].");
        add_field(14, fields, None, "holds_slug", Value::Null, &at)?;
    }
    Ok(root)
}

/// Version 15 → 16: `vendor: null` and `product_id: null` right after
/// each tool's `name`: who makes the tool and the maker's part number,
/// which Fusion import fills in. No program reads them.
fn v15_to_v16(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let Some(tools) = root.get_mut("tools").and_then(Value::as_array_mut) else {
        return Ok(root);
    };
    for (i, tool) in tools.iter_mut().enumerate() {
        let Value::Object(fields) = tool else {
            return Err(format!("`tools[{i}]` is not an object"));
        };
        let at = format!("tools[{i}].");
        add_field(15, fields, Some("name"), "vendor", Value::Null, &at)?;
        add_field(15, fields, Some("vendor"), "product_id", Value::Null, &at)?;
    }
    Ok(root)
}

/// Version 16 → 17: `allow_tool_wider_than_hole: false` at the end of
/// each Drill, so a hole narrower than the tool is skipped with a warning
/// rather than drilled larger than drawn, which an earlier file did.
/// Other operations are unchanged.
fn v16_to_v17(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    each_operation(&mut root, &["drill"], |i, drill| {
        let at = format!("operations[{i}].");
        let key = "allow_tool_wider_than_hole";
        add_field(16, drill, None, key, Value::Bool(false), &at)
    })?;
    Ok(root)
}

/// Version 17 → 18: the Bore operation, `"kind": "bore"`, which no
/// version 17 file holds, so a version 17 file is already a version 18
/// one. The bump makes a version 17 reader refuse a file with a Bore as
/// newer instead of failing on an unknown kind (ADR-0002).
fn v17_to_v18(root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    Ok(root)
}

/// Version 18 → 19: `allowance_mm` becomes `stock_to_leave_mm`, in place
/// and with its value, in each Pocket, each Bore and each Profile's
/// non-null `last_pass`: the material they leave on the wall is their
/// stock to leave, and the through-cut allowance is the only allowance.
/// Other operations, and `through_cut_allowance_mm`, are unchanged.
fn v18_to_v19(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    each_operation(&mut root, &["pocket", "bore", "profile"], |i, op| {
        let at = format!("operations[{i}].");
        if op.get("kind").and_then(Value::as_str) != Some("profile") {
            return rename_key(18, op, "allowance_mm", "stock_to_leave_mm", &at);
        }
        let Some(last_pass) = op.get_mut("last_pass").and_then(Value::as_object_mut) else {
            return Ok(());
        };
        let at = format!("{at}last_pass.");
        rename_key(18, last_pass, "allowance_mm", "stock_to_leave_mm", &at)
    })?;
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

/// Version 22 → 23: a male Inlay's cut-out. Inserts `"cut_out": null`
/// after `safe_z_mm` (at the end when it is missing) in every Inlay, so a
/// version 22 file cuts the same, with the plug sawn free. Other
/// operations are unchanged.
fn v22_to_v23(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    add_operation_field(22, &mut root, "inlay", "safe_z_mm", "cut_out", &Value::Null)?;
    Ok(root)
}

/// Version 23 → 24: a male Inlay's outline rounding. Inserts
/// `"outline_rounding_mm": null` after `margin_mm` (at the end when it is
/// missing) in every Inlay. Other operations are unchanged. The male's
/// band edge changed in the same release (the end mill cuts it when a
/// clearing pass fits), so a version 23 male exports what the same
/// project written at version 24 does.
fn v23_to_v24(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    add_operation_field(
        23,
        &mut root,
        "inlay",
        "margin_mm",
        "outline_rounding_mm",
        &Value::Null,
    )?;
    Ok(root)
}

/// Version 24 → 25: one Inlay cuts both boards. Replaces each Inlay's
/// `side`, in its place, with its `male_board`: all `null` for a
/// `"female"`, whose male is not cut until the user measures the male
/// board; the stock's `width_mm`, `height_mm` and `thickness_mm` for a
/// `"male"`, whose male programs are then cut on the same board, byte for
/// byte. That male now also cuts the female on the stock, which
/// [`migrate_with_notes`] notes. The stock is kept. A side that is
/// neither, an Inlay that already has a male board, or a male whose stock
/// has no width or height, is refused.
fn v24_to_v25(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let stock = root.get("stock").cloned().unwrap_or(Value::Null);
    each_operation(&mut root, &["inlay"], |i, inlay| {
        if inlay.contains_key("male_board") {
            return Err(already_has(24, &format!("operations[{i}]."), "male_board"));
        }
        let board = match inlay.get("side").and_then(Value::as_str) {
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
        let mut board = Some(board);
        *inlay = std::mem::take(inlay)
            .into_iter()
            .filter_map(|(key, value)| {
                if key == "side" {
                    board.take().map(|board| ("male_board".to_owned(), board))
                } else {
                    Some((key, value))
                }
            })
            .collect();
        Ok(())
    })?;
    Ok(root)
}

/// Version 25 → 26: an Inlay's drawn boundary. Inserts `"boundary": []`
/// after `outline_rounding_mm` (at the end when it is missing) in every
/// Inlay, so a version 25 file's band is still its margin and rounding.
/// Other operations are unchanged.
fn v25_to_v26(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let none = Value::Array(Vec::new());
    add_operation_field(
        25,
        &mut root,
        "inlay",
        "outline_rounding_mm",
        "boundary",
        &none,
    )?;
    Ok(root)
}

/// Version 26 → 27 (sub-phase 3.5): bitmaps and the Photo V-Carve.
/// Inserts `"bitmaps": []` after `instances` (at the end when it is
/// missing). Operations are unchanged. Fails if the root already has
/// `bitmaps`, or an operation is a `photo_v_carve`: the list and the kind
/// arrive together in 27, so that is not a version 26 file.
fn v26_to_v27(mut root: Map<String, Value>) -> Result<Map<String, Value>, String> {
    let none = Value::Array(Vec::new());
    add_field(26, &mut root, Some("instances"), "bitmaps", none, "")?;
    let photo = root
        .get("operations")
        .and_then(Value::as_array)
        .and_then(|ops| {
            ops.iter()
                .position(|op| op.get("kind").and_then(Value::as_str) == Some("photo_v_carve"))
        });
    match photo {
        Some(i) => Err(format!(
            "`operations[{i}]` is a photo_v_carve, which a version 26 file cannot hold"
        )),
        None => Ok(root),
    }
}

/// Version 27 → 28 (`docs/specs/phase-3/design.md`, sub-phase 3.6): the
/// tapered ball nose cutter, which no version 27 file holds, so a version
/// 27 file is already a version 28 one. The bump makes a version 27
/// reader refuse a file with a tapered ball nose as newer instead of
/// failing on an unknown cutter (ADR-0002). Fails if a tool's cutter is
/// already a `tapered_ball_nose`: that is not a version 27 file.
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
    match tapered {
        Some(i) => Err(format!(
            "`tools[{i}].cutter` is a tapered_ball_nose, which a version 27 file cannot hold"
        )),
        None => Ok(root),
    }
}

/// Why a step from `version` refuses a file that already holds `key`
/// under `at` (a path prefix such as `operations[1].`): only a later
/// version writes it, so the file is not of the version it says.
fn already_has(version: u32, at: &str, key: &str) -> String {
    format!("a version {version} file has no `{at}{key}`, but this one does")
}

/// Runs `rewrite` on each operation whose `kind` is one of `kinds`, with
/// its index in `operations`. A root with no `operations` array is left
/// as it is.
fn each_operation(
    root: &mut Map<String, Value>,
    kinds: &[&str],
    mut rewrite: impl FnMut(usize, &mut Map<String, Value>) -> Result<(), String>,
) -> Result<(), String> {
    let Some(ops) = root.get_mut("operations").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    for (i, op) in ops.iter_mut().enumerate() {
        let Value::Object(fields) = op else { continue };
        let kind = fields.get("kind").and_then(Value::as_str);
        if kind.is_some_and(|kind| kinds.contains(&kind)) {
            rewrite(i, fields)?;
        }
    }
    Ok(())
}

/// Adds `key: value` right after `after` (at the end when `after` is
/// missing) to each operation of `kind`, for the step from `version`.
/// Fails if one already has `key`.
fn add_operation_field(
    version: u32,
    root: &mut Map<String, Value>,
    kind: &str,
    after: &str,
    key: &str,
    value: &Value,
) -> Result<(), String> {
    each_operation(root, &[kind], |i, fields| {
        let at = format!("operations[{i}].");
        add_field(version, fields, Some(after), key, value.clone(), &at)
    })
}

/// Renames `map`'s key `from` to `to`, in its place, for the step from
/// `version`. Fails if `to` is already there, naming it under `at` (a
/// path prefix such as `operations[1].`).
fn rename_key(
    version: u32,
    map: &mut Map<String, Value>,
    from: &str,
    to: &str,
    at: &str,
) -> Result<(), String> {
    if map.contains_key(to) {
        return Err(already_has(version, at, to));
    }
    *map = std::mem::take(map)
        .into_iter()
        .map(|(k, v)| {
            if k == from {
                (to.to_owned(), v)
            } else {
                (k, v)
            }
        })
        .collect();
    Ok(())
}

/// Adds `key: value` to `map` right after `after`, or at the end when
/// `after` is `None` or absent, for the step from `version`: where the
/// next version declares the key, so a migrated file reads like a saved
/// one. Fails if `key` is already there, naming it under `at` (a path
/// prefix such as `tools[0].`): that is not a file of `version`.
fn add_field(
    version: u32,
    map: &mut Map<String, Value>,
    after: Option<&str>,
    key: &str,
    value: Value,
    at: &str,
) -> Result<(), String> {
    if map.contains_key(key) {
        return Err(already_has(version, at, key));
    }
    let index = after
        .and_then(|after| map.keys().position(|k| k == after))
        .map_or(map.len(), |anchor| anchor + 1);
    map.shift_insert(index, key.to_owned(), value);
    Ok(())
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
