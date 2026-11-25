// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of the `.dcv` migrations: each step's rewrite, the fixture of
//! every older format, and what a step refuses.

use super::*;
use crate::test_support::{sample_drill, sample_project};
use crate::{Operation, OriginXy, Side, SpindleControl, ToolChangeMode, ToolKind, load, save};
use dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;
use serde_json::json;

const V0_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v0/sample.dcv");
const V1_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v1/sample.dcv");
const V2_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v2/sample.dcv");
const V3_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v3/sample.dcv");
const V4_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v4/sample.dcv");
const V5_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v5/sample.dcv");

/// The sample Pocket as raw JSON of `version`: before version 19 its
/// `stock_to_leave_mm` is `allowance_mm`.
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
/// The male board a migrated version 24 male Inlay gets: the sample
/// stock's, 100 × 100 × 6 mm (#498).
fn stock_board() -> crate::model::MaleBoard {
    crate::model::MaleBoard {
        width_mm: Some(100.0),
        height_mm: Some(100.0),
        thickness_mm: Some(6.0),
    }
}

/// `op` (an operation as the current version writes it) as version 24
/// wrote it, when it is an Inlay: `side` in place of `male_board`,
/// `"male"` when the board has a thickness, else `"female"`,
/// and no `boundary`.
fn as_v24(op: &mut Value) {
    let Some(fields) = op.as_object_mut() else {
        return;
    };
    fields.shift_remove("boundary");
    let Some(board) = fields.get("male_board") else {
        return;
    };
    let side = if board["thickness_mm"].is_null() {
        "female"
    } else {
        "male"
    };
    let old: Map<String, Value> = std::mem::take(fields)
        .into_iter()
        .map(|(k, v)| {
            if k == "male_board" {
                ("side".to_owned(), json!(side))
            } else {
                (k, v)
            }
        })
        .collect();
    *fields = old;
}

/// `root`, a file from before format 27 whose other fields are the
/// current ones already, as the current version holds it: its
/// `version`, `"bitmaps": []` after `instances`, and what format 29
/// adds.
fn at_current(root: &Value) -> Value {
    let mut out = Map::new();
    for (key, value) in root.as_object().unwrap() {
        out.insert(key.clone(), value.clone());
        if key == "instances" {
            out.insert("bitmaps".into(), json!([]));
        }
    }
    out.insert("version".into(), json!(CURRENT_VERSION));
    with_format_30(&with_format_29(&Value::Object(out)))
}

/// Takes out of `root` what formats 30 and 31 added, leaving it as a
/// format 29 file held it: each Engrave's `enter` and `leave` (format
/// 31's), or `enter_fraction` and `leave_fraction` (format 30's).
fn without_format_30(root: &mut Value) {
    if let Some(operations) = root.get_mut("operations").and_then(Value::as_array_mut) {
        for operation in operations {
            let operation = operation.as_object_mut().unwrap();
            for key in ["enter", "leave", "enter_fraction", "leave_fraction"] {
                operation.shift_remove(key);
            }
        }
    }
}

/// `root`, a file from before format 30, with what formats 30 and 31 add
/// as the current version writes it, each key where a saved file has it:
/// each Engrave's `"enter": "auto"` and `"leave": "auto"`. Written apart
/// from the migration, so the two are checked against each other.
fn with_format_30(root: &Value) -> Value {
    let mut out = root.clone();
    if let Some(Value::Array(operations)) = out.get_mut("operations") {
        for operation in operations {
            if operation["kind"] != json!("engrave") {
                continue;
            }
            let added = [("enter", json!("auto")), ("leave", json!("auto"))];
            let fields = insert_after(
                operation.as_object().unwrap(),
                "alternate_direction",
                &added,
            );
            *operation = Value::Object(fields);
        }
    }
    out
}

/// `root`, a current file, as format 30 held it: each Engrave's `enter`
/// and `leave` written as `enter_fraction` and `leave_fraction` in their
/// place, `null` for `"auto"` and the number of a `fraction`. Panics on
/// any other point, which format 30 could not hold.
fn as_format_30(root: &Value) -> Value {
    let mut out = root.clone();
    out["version"] = json!(30);
    if let Some(Value::Array(operations)) = out.get_mut("operations") {
        for operation in operations {
            if operation["kind"] != json!("engrave") {
                continue;
            }
            let fields: Map<String, Value> = operation
                .as_object()
                .unwrap()
                .iter()
                .map(|(k, v)| {
                    let fraction = |v: &Value| match v {
                        Value::String(auto) if auto == "auto" => Value::Null,
                        _ => v["fraction"]["length_fraction"].clone(),
                    };
                    match k.as_str() {
                        "enter" => ("enter_fraction".to_owned(), fraction(v)),
                        "leave" => ("leave_fraction".to_owned(), fraction(v)),
                        _ => (k.clone(), v.clone()),
                    }
                })
                .collect();
            *operation = Value::Object(fields);
        }
    }
    out
}

/// Takes out of `root` what format 29 added, leaving it as a format 28
/// file held it: `groups`, each definition's `shape`, each instance's
/// `construction` and `group`, each bitmap's `opacity_fraction`; and
/// what format 30 added after it.
fn without_format_29(root: &mut Value) {
    without_format_30(root);
    let root = root.as_object_mut().unwrap();
    root.shift_remove("groups");
    if let Some(definitions) = root.get_mut("definitions").and_then(Value::as_object_mut) {
        for definition in definitions.values_mut() {
            definition.as_object_mut().unwrap().shift_remove("shape");
        }
    }
    if let Some(instances) = root.get_mut("instances").and_then(Value::as_array_mut) {
        for instance in instances {
            let instance = instance.as_object_mut().unwrap();
            instance.shift_remove("construction");
            instance.shift_remove("group");
        }
    }
    if let Some(bitmaps) = root.get_mut("bitmaps").and_then(Value::as_array_mut) {
        for bitmap in bitmaps {
            bitmap
                .as_object_mut()
                .unwrap()
                .shift_remove("opacity_fraction");
        }
    }
}

/// `map` with `added` put in right after its key `after`.
fn insert_after(
    map: &Map<String, Value>,
    after: &str,
    added: &[(&str, Value)],
) -> Map<String, Value> {
    let mut out = Map::new();
    for (key, value) in map {
        out.insert(key.clone(), value.clone());
        if key == after {
            for (key, value) in added {
                out.insert((*key).to_owned(), value.clone());
            }
        }
    }
    out
}

/// `root`, a file from before format 29, with what format 29 adds, each
/// key where a saved file has it. Written apart from the migration, so
/// the two are checked against each other.
fn with_format_29(root: &Value) -> Value {
    let mut out = insert_after(root.as_object().unwrap(), "tabs", &[("groups", json!([]))]);
    if let Some(Value::Object(definitions)) = out.get_mut("definitions") {
        for definition in definitions.values_mut() {
            let fields = insert_after(
                definition.as_object().unwrap(),
                "name",
                &[("shape", Value::Null)],
            );
            *definition = Value::Object(fields);
        }
    }
    if let Some(Value::Array(instances)) = out.get_mut("instances") {
        for instance in instances {
            let added = [("construction", json!(false)), ("group", Value::Null)];
            let fields = insert_after(instance.as_object().unwrap(), "locked", &added);
            *instance = Value::Object(fields);
        }
    }
    if let Some(Value::Array(bitmaps)) = out.get_mut("bitmaps") {
        for bitmap in bitmaps {
            let added = [("opacity_fraction", json!(1.0))];
            let fields = insert_after(bitmap.as_object().unwrap(), "locked", &added);
            *bitmap = Value::Object(fields);
        }
    }
    Value::Object(out)
}

/// Asserts that `saved`, a file this build wrote, is `before`, the text
/// a format 28 build would have written for it, and what format 29 adds:
/// the same keys in the same order with the same values, and those.
#[track_caller]
fn assert_saved_adds_format_29_only(saved: &str, before: &str, what: &str) {
    let saved: Value = serde_json::from_str(saved).unwrap();
    let before: Value = serde_json::from_str(before).unwrap();
    assert_eq!(
        serde_json::to_string_pretty(&saved).unwrap(),
        serde_json::to_string_pretty(&with_format_30(&with_format_29(&before))).unwrap(),
        "{what}"
    );
}

fn sample_root_at(version: u32) -> Value {
    let mut root = serde_json::to_value(sample_project()).unwrap();
    if version < 30 {
        without_format_30(&mut root);
    }
    if version < 29 {
        without_format_29(&mut root);
    }
    if version < 27 {
        root.as_object_mut().unwrap().shift_remove("bitmaps");
    }
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
            serde_json::from_str(include_str!("../../tests/fixtures/post/grbl.json")).unwrap();
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

/// Every older format's sample fixture loads as the current sample
/// project. Each row is one former `version_N_fixture_loads` test, and
/// the loop checks what each of them did: all, the version and the
/// project; from version 8, that validation finds nothing; from 21, that
/// the file says its version in so many words; from 24, that the
/// migration notes nothing. Version 18 has no plain sample and versions
/// 26 to 28 hold more than one fixture; their tests are further down.
/// The fixtures of 21, 22 and 25 that hold what those formats added are
/// in the table after this one.
#[test]
fn every_older_sample_fixture_loads_as_the_sample_project() {
    let fixtures: [(u32, &[u8]); 25] = [
        (0, V0_FIXTURE),
        (1, V1_FIXTURE),
        (2, V2_FIXTURE),
        (3, V3_FIXTURE),
        (4, V4_FIXTURE),
        (5, V5_FIXTURE),
        (6, V6_FIXTURE),
        (7, V7_FIXTURE),
        (8, V8_FIXTURE),
        (9, V9_FIXTURE),
        (10, V10_FIXTURE),
        (11, V11_FIXTURE),
        (12, V12_FIXTURE),
        (13, V13_FIXTURE),
        (14, V14_FIXTURE),
        (15, V15_FIXTURE),
        (16, V16_FIXTURE),
        (17, V17_FIXTURE),
        (19, V19_FIXTURE),
        (20, V20_FIXTURE),
        (21, V21_FIXTURE),
        (22, V22_FIXTURE),
        (23, V23_FIXTURE),
        (24, V24_FIXTURE),
        (25, V25_FIXTURE),
    ];
    for (version, bytes) in fixtures {
        let case = format!("version_{version}_fixture_loads");
        if version >= 21 {
            let text = std::str::from_utf8(bytes).unwrap();
            assert!(
                text.contains(&format!("\"version\": {version},")),
                "{case}: a version {version} file"
            );
        }
        let loaded = load(bytes).unwrap_or_else(|err| panic!("{case}: {err}"));
        assert_eq!(loaded.project.version, CURRENT_VERSION, "{case}");
        assert_eq!(loaded.project, sample_project(), "{case}");
        if version >= 8 {
            assert_eq!(loaded.problems, vec![], "{case}");
        }
        if version >= 24 {
            assert_eq!(loaded.notes, vec![], "{case}: no Inlay, nothing to note");
        }
        // The sample's heights are unset, so the version 11 file exports
        // what it did before version 12.
        if version == 11 {
            assert_eq!(loaded.project.heights, crate::Heights::unset(), "{case}");
        }
        if version == 15 {
            for tool in &loaded.project.tools {
                assert_eq!((&tool.vendor, &tool.product_id), (&None, &None), "{case}");
            }
        }
    }
}

const V21_COUNTERSINK_FIXTURE: &[u8] =
    include_bytes!("../../tests/fixtures/dcv/v21/countersink.dcv");
const V22_INLAY_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v22/inlay.dcv");
const V25_MALE_BOARD_FIXTURE: &[u8] =
    include_bytes!("../../tests/fixtures/dcv/v25/inlay-male-board.dcv");

/// One fixture that holds what its format added, where the plain sample
/// does not.
struct FeatureFixture {
    /// The fixture's name under its version's folder.
    name: &'static str,
    /// The version the file is written in.
    version: u32,
    /// The file.
    bytes: &'static [u8],
    /// What the format wrote for the feature, and what a later format
    /// added or replaced, which a file of this version does not hold.
    holds: &'static [&'static str],
    lacks: &'static [&'static str],
    /// The project the file loads as at the current version.
    expected: fn() -> crate::Project,
    /// Whether a project holds the feature the format added.
    has_feature: fn(&crate::Project) -> bool,
}

/// The sample project with the 90° V-bit `t2` and `op` after its Profile.
fn sample_with_v_bit_and(op: Operation) -> crate::Project {
    let mut project = sample_project();
    project.tools.push(crate::test_support::tool_v_bit_90());
    project.operations.push(op);
    project
}

/// The plain samples of formats 21, 22 and 25 hold nothing those formats
/// added (a countersink, an Inlay, an Inlay's male board), so each has a
/// second fixture that does, written by hand in its own format from the
/// sample of that version and the step's description of the format: no
/// current build can save an old format. Each row's file says its
/// version, holds the format's own words and none of a later format's,
/// loads as the project the row expects at the current version with
/// nothing for validation to report and nothing noted, and that project
/// holds the feature.
///
/// - 21: a countersink Drill `o3` of a 3.175 mm hole `i2` with the V-bit.
/// - 22: the female Inlay `o7` as format 22 wrote it, with `side` and
///   without the fields formats 23 to 26 added; it gains them, unset.
/// - 25: the Inlay with a measured male board 10 mm thick, so the one
///   file cuts both boards; `plug_depth_mm` 0.5 keeps the male's
///   engagement within the bit's cone.
#[test]
fn feature_fixtures_load_holding_what_their_format_added() {
    use crate::model::{
        DrillCycle, DrillOp, FeatureDefinition, FeatureInstance, InlayOp, MaleBoard,
    };
    use crate::test_support::sample_inlay;
    use crate::{DefinitionId, InstanceId, ToolId};
    use dc_geometry::{Point, Polyline, Transform};

    let fixtures = [
        FeatureFixture {
            name: "countersink",
            version: 21,
            bytes: V21_COUNTERSINK_FIXTURE,
            holds: &[
                "\"countersink\": {",
                "\"start_plane_cleared_externally\": false",
            ],
            lacks: &["\"inlay\"", "\"bitmaps\""],
            expected: || {
                let mut project = sample_with_v_bit_and(Operation::Drill(DrillOp {
                    targets: vec![InstanceId::new("i2")],
                    tool: ToolId::new("t2"),
                    depth_mm: None,
                    cycle: DrillCycle::Countersink {
                        diameter_mm: 6.0,
                        start_depth_mm: 0.0,
                        start_plane_cleared_externally: false,
                    },
                    ..sample_drill()
                }));
                let hole = FeatureDefinition {
                    id: DefinitionId::new("d2"),
                    name: "Hole".into(),
                    shape: None,
                    profiles: vec![Polyline::circle(Point::new(0.0, 0.0), 3.175 / 2.0)],
                };
                project.instances.push(FeatureInstance {
                    id: InstanceId::new("i2"),
                    definition: hole.id.clone(),
                    transform: Transform::translate(35.0, 25.0),
                    name: "Hole".into(),
                    visible: true,
                    locked: false,
                    construction: false,
                    group: None,
                    z_top_mm: Some(6.0),
                    z_bottom_mm: Some(0.0),
                });
                project.definitions.insert(hole.id.clone(), hole);
                project
            },
            has_feature: |project| {
                project.operations.iter().any(|op| {
                    matches!(
                        op,
                        Operation::Drill(DrillOp {
                            cycle: DrillCycle::Countersink { .. },
                            ..
                        })
                    )
                })
            },
        },
        FeatureFixture {
            name: "inlay",
            version: 22,
            bytes: V22_INLAY_FIXTURE,
            holds: &["\"kind\": \"inlay\"", "\"side\": \"female\""],
            lacks: &[
                "\"cut_out\"",
                "\"outline_rounding_mm\"",
                "\"male_board\"",
                "\"boundary\"",
                "\"bitmaps\"",
            ],
            expected: || sample_with_v_bit_and(Operation::Inlay(sample_inlay())),
            has_feature: |project| {
                project
                    .operations
                    .iter()
                    .any(|op| matches!(op, Operation::Inlay(_)))
            },
        },
        FeatureFixture {
            name: "inlay-male-board",
            version: 25,
            bytes: V25_MALE_BOARD_FIXTURE,
            holds: &["\"male_board\": {", "\"thickness_mm\": 10.0"],
            lacks: &[
                "\"side\": \"female\"",
                "\"side\": \"male\"",
                "\"boundary\"",
                "\"bitmaps\"",
            ],
            expected: || {
                sample_with_v_bit_and(Operation::Inlay(InlayOp {
                    male_board: MaleBoard {
                        width_mm: None,
                        height_mm: None,
                        thickness_mm: Some(10.0),
                    },
                    plug_depth_mm: Some(0.5),
                    ..sample_inlay()
                }))
            },
            has_feature: |project| {
                project
                    .operations
                    .iter()
                    .any(|op| matches!(op, Operation::Inlay(inlay) if inlay.cuts_male()))
            },
        },
    ];
    for fixture in fixtures {
        let FeatureFixture { name, version, .. } = fixture;
        let case = format!("v{version}/{name}.dcv");
        let text = std::str::from_utf8(fixture.bytes).unwrap();
        assert!(
            text.contains(&format!("\"version\": {version},")),
            "{case}: a version {version} file"
        );
        for words in fixture.holds {
            assert!(text.contains(words), "{case} holds {words}");
        }
        for words in fixture.lacks {
            assert!(!text.contains(words), "{case} holds {words}");
        }
        // The plain sample of the same version does not hold the feature.
        assert!(
            !(fixture.has_feature)(&sample_project()),
            "{case}: the sample has the feature"
        );

        let loaded = load(fixture.bytes).unwrap_or_else(|err| panic!("{case}: {err}"));
        assert_eq!(loaded.project.version, CURRENT_VERSION, "{case}");
        assert_eq!(loaded.project, (fixture.expected)(), "{case}");
        assert!((fixture.has_feature)(&loaded.project), "{case}");
        assert_eq!(loaded.problems, vec![], "{case}");
        assert_eq!(loaded.notes, vec![], "{case}");

        // Saved, it is a current file that loads as the same project.
        let saved = save(&loaded.project).unwrap_or_else(|err| panic!("{case}: {err}"));
        assert!(
            saved.contains(&format!("\"version\": {CURRENT_VERSION},")),
            "{case}"
        );
        assert_eq!(
            load(saved.as_bytes()).unwrap().project,
            loaded.project,
            "{case}"
        );
    }
}

/// A reader of the version before each bump refuses the bumped file as
/// newer, which is what the bump is for (ADR-0002): a version 17 reader
/// must not meet a Bore, nor a version 19 one a V-Carve. Each row is one
/// former `version_N_is_newer_than_N-1` test: the version it bumped to,
/// and the lowest `CURRENT_VERSION` it accepted.
#[test]
fn each_bump_makes_an_older_reader_refuse_the_file_as_newer() {
    let bumps: [(u32, u32); 10] = [
        (18, 19),
        (19, 20),
        (20, 21),
        (21, 22),
        (22, 23),
        (23, 23),
        (24, 24),
        (25, 25),
        (26, 26),
        (27, 27),
    ];
    for (version, least_current) in bumps {
        let case = format!("version_{version}_is_newer_than_{}", version - 1);
        assert!(CURRENT_VERSION >= least_current, "{case}");
        let err = migrate(sample_root_at(CURRENT_VERSION), CURRENT_VERSION + 1).unwrap_err();
        assert!(
            matches!(err, LoadError::NewerVersion { .. }),
            "{case}: {err:?}"
        );
    }
}

/// One file that is not of the version it says: it holds something only
/// a later version writes.
struct NotOfItsVersion {
    /// The test this case was, and what the file holds.
    case: &'static str,
    /// The version the file says, which `migrate` starts from.
    says: u32,
    /// The version whose step must refuse it.
    step: u32,
    /// The file's root.
    root: fn() -> Value,
    /// What the message must name; empty where the case checked only
    /// which step failed.
    names: &'static [&'static str],
}

/// A root made by `edit` from `root`.
fn edited(mut root: Value, edit: impl FnOnce(&mut Value)) -> Value {
    edit(&mut root);
    root
}

/// The version 18 fixture's root: a Profile with a last pass, a Pocket and
/// a Bore, each with an `allowance_mm`.
fn v18_stock_to_leave_root() -> Value {
    serde_json::from_slice(V18_STOCK_TO_LEAVE_FIXTURE).unwrap()
}

/// A file that already holds a key its next version adds is not a file of
/// its version, and the step to that version refuses it, naming the key.
/// Each row is one case of a former `vN_with_X_already_present_fails`
/// test, named in `case`.
#[test]
fn a_file_holding_what_a_later_version_adds_fails_at_that_step() {
    let cases = [
        NotOfItsVersion {
            case: "v0_with_flatten_tolerance_already_present_fails",
            says: 0,
            step: 0,
            root: || edited(sample_root_at(1), |root| root["version"] = Value::from(0)),
            names: &[],
        },
        NotOfItsVersion {
            case: "v1_with_spindle_control_already_present_fails",
            says: 1,
            step: 1,
            root: || edited(sample_root_at(2), |root| root["version"] = Value::from(1)),
            names: &[],
        },
        NotOfItsVersion {
            case: "v4_with_tool_change_mode_already_present_fails",
            says: 4,
            step: 4,
            root: || edited(sample_root_at(5), |root| root["version"] = Value::from(4)),
            names: &[],
        },
        NotOfItsVersion {
            case: "v7_with_phase_2_field_already_present_fails: arc_fit_tolerance_mm",
            says: 7,
            step: 7,
            root: || {
                edited(sample_root_at(7), |root| {
                    root["arc_fit_tolerance_mm"] = json!(0.02);
                })
            },
            names: &["arc_fit_tolerance_mm"],
        },
        NotOfItsVersion {
            case: "v7_with_phase_2_field_already_present_fails: number",
            says: 7,
            step: 7,
            root: || {
                edited(sample_root_at(7), |root| {
                    root["tools"][0]["number"] = json!(1);
                })
            },
            names: &["number"],
        },
        NotOfItsVersion {
            case: "v7_with_phase_2_field_already_present_fails: entry",
            says: 7,
            step: 7,
            root: || {
                edited(sample_root_at(7), |root| {
                    root["operations"][0]["entry"] = json!("plunge");
                })
            },
            names: &["entry"],
        },
        NotOfItsVersion {
            case: "v7_with_phase_2_field_already_present_fails: use_canned_cycles",
            says: 7,
            step: 7,
            root: || {
                edited(sample_root_at(7), |root| {
                    root["post_processor"]["use_canned_cycles"] = json!(true);
                })
            },
            names: &["use_canned_cycles"],
        },
        NotOfItsVersion {
            case: "v8_with_on_holes_already_present_fails",
            says: 8,
            step: 8,
            root: || {
                edited(sample_root_at(8), |root| {
                    root["operations"][0]["tabs"]["on_holes"] = json!(false);
                })
            },
            names: &["on_holes"],
        },
        NotOfItsVersion {
            case: "v10_with_tabs_already_present_fails",
            says: 10,
            step: 10,
            root: || edited(sample_root_at(10), |root| root["tabs"] = json!([])),
            names: &["tabs"],
        },
        NotOfItsVersion {
            case: "v12_with_last_pass_already_present_fails",
            says: 12,
            step: 12,
            root: || {
                edited(sample_root_at(12), |root| {
                    root["operations"][0]["last_pass"] = Value::Null;
                })
            },
            names: &["last_pass"],
        },
        NotOfItsVersion {
            case: "v13_with_start_already_present_fails: start",
            says: 13,
            step: 13,
            root: || {
                edited(sample_root_at(13), |root| {
                    root["operations"][0]["start"] = json!("auto");
                })
            },
            names: &["start"],
        },
        NotOfItsVersion {
            case: "v13_with_start_already_present_fails: tabs.avoid_corners",
            says: 13,
            step: 13,
            root: || {
                edited(sample_root_at(13), |root| {
                    root["operations"][0]["tabs"]["avoid_corners"] = json!(true);
                })
            },
            names: &["tabs.avoid_corners"],
        },
        NotOfItsVersion {
            case: "v14_with_holds_slug_already_present_fails",
            says: 14,
            step: 14,
            root: || {
                edited(v14_root_with_tabs(), |root| {
                    root["tabs"][1]["holds_slug"] = json!(true);
                })
            },
            names: &["`tabs[1].holds_slug`"],
        },
        NotOfItsVersion {
            case: "v16_with_allow_tool_wider_than_hole_already_present_fails",
            says: 16,
            step: 16,
            root: || {
                edited(v16_root_with_drill(), |root| {
                    root["operations"][1]["allow_tool_wider_than_hole"] = json!(true);
                })
            },
            names: &["`operations[1].allow_tool_wider_than_hole`"],
        },
        // A Pocket, Bore or last pass that already has `stock_to_leave_mm`
        // is not a version 18 file.
        NotOfItsVersion {
            case: "v18_with_stock_to_leave_already_present_fails: a Profile's last pass",
            says: 18,
            step: 18,
            root: || {
                edited(v18_stock_to_leave_root(), |root| {
                    root["operations"][0]["last_pass"]["stock_to_leave_mm"] = json!(0.0);
                })
            },
            names: &["`operations[0].last_pass.stock_to_leave_mm`"],
        },
        NotOfItsVersion {
            case: "v18_with_stock_to_leave_already_present_fails: operations[1]",
            says: 18,
            step: 18,
            root: || {
                edited(v18_stock_to_leave_root(), |root| {
                    root["operations"][1]["stock_to_leave_mm"] = json!(0.0);
                })
            },
            names: &["`operations[1].stock_to_leave_mm`"],
        },
        NotOfItsVersion {
            case: "v18_with_stock_to_leave_already_present_fails: operations[2]",
            says: 18,
            step: 18,
            root: || {
                edited(v18_stock_to_leave_root(), |root| {
                    root["operations"][2]["stock_to_leave_mm"] = json!(0.0);
                })
            },
            names: &["`operations[2].stock_to_leave_mm`"],
        },
        // A file that says 26 and holds `bitmaps`, or a `photo_v_carve`
        // operation, is not a version 26 file: the kind and the list
        // arrive together in 27, and the migration refuses it, naming
        // what it found.
        NotOfItsVersion {
            case: "v26_with_bitmaps_already_present_fails: an empty list",
            says: 26,
            step: 26,
            root: || edited(sample_root_at(26), |root| root["bitmaps"] = json!([])),
            names: &["`bitmaps`"],
        },
        NotOfItsVersion {
            case: "v26_with_bitmaps_already_present_fails: a bitmap",
            says: 26,
            step: 26,
            root: || {
                edited(sample_root_at(26), |root| {
                    root["bitmaps"] = json!([{ "id": "b1" }]);
                })
            },
            names: &["`bitmaps`"],
        },
        NotOfItsVersion {
            case: "v26_with_bitmaps_already_present_fails: null",
            says: 26,
            step: 26,
            root: || edited(sample_root_at(26), |root| root["bitmaps"] = json!(null)),
            names: &["`bitmaps`"],
        },
        NotOfItsVersion {
            case: "v26_with_bitmaps_already_present_fails: a photo_v_carve",
            says: 26,
            step: 26,
            root: || {
                edited(sample_root_at(26), |root| {
                    root["operations"][0]["kind"] = json!("photo_v_carve");
                })
            },
            names: &["`operations[0]`", "photo_v_carve"],
        },
        // An older file is refused by the same step, at the end of its
        // chain.
        NotOfItsVersion {
            case: "v26_with_bitmaps_already_present_fails: a version 20 file",
            says: 20,
            step: 26,
            root: || edited(sample_root_at(20), |root| root["bitmaps"] = json!([])),
            names: &[],
        },
    ];
    for NotOfItsVersion {
        case,
        says,
        step,
        root,
        names,
    } in cases
    {
        let err = migrate(root(), says).unwrap_err();
        let LoadError::Migration {
            from, to, message, ..
        } = &err
        else {
            panic!("{case}: {err:?}");
        };
        assert_eq!((*from, *to), (step, step + 1), "{case}: {err:?}");
        for name in names {
            assert!(message.contains(name), "{case}: {name}: {message}");
        }
    }
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

/// Every file before version 2 was exported with the spindle switched by
/// the program, so the migration keeps that.
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

/// A file before version 5 has no tool-change mode. The migration
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

/// Version 6 lets the instance depths be unset, but the migration
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

/// Version 4 lets thickness and depth be unset, but a version 3
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
/// object.
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

const V6_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v6/sample.dcv");

/// The migrated block is the reference, in the order a version 7
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

/// A version 6 file with no embedded post-processor is broken,
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
const V6_GRBLHAL_MANUAL: &str = include_str!("../../tests/fixtures/dcv/v6/grblhal-manual.dcv");

/// A version 6 copy that matches the bundled definition becomes a
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

/// A version 6 copy predates `spindle_on_dwell.units` and
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

/// A version 6 copy that already names a dwell unit is not a
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

/// A copy whose dialect differs from the bundled version 1 is
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

/// A post-processor that was never bundled cannot become a
/// reference either.
#[test]
fn v6_with_unbundled_id_fails_to_load() {
    let message = v6_edited_error("\"id\": \"grblhal\"", "\"id\": \"custom\"");
    assert!(message.contains("`custom`"), "{message}");
    assert!(message.contains("grbl, grblhal"), "{message}");
    assert!(message.contains("post_processor.id"), "{message}");
    assert_has_replacement_block(&message, "grblhal");
}

// Phase 2: version 7 → 8 adds the phase 2 fields with the values
// that keep a phase 1 project cutting the same.

const V7_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v7/sample.dcv");

/// Design spec, "Migration v6 → v7" (version 7 → 8 here): each
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

// Version 8 → 9: holes get no tabs unless `tabs.on_holes` is
// set, so a project from before keeps them on its holes.

const V8_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v8/sample.dcv");

/// A version 8 Profile put its tabs on every outline it cut,
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

// Version 10 → 11: placed tabs. The tab setting stays as a
// request, which `dreamcarve` places when it loads the project.

const V9_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v9/sample.dcv");
const V10_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v10/sample.dcv");

/// A version 10 project gains an empty list of placed tabs right
/// after its operations; each Profile keeps its tab setting, now a
/// request, so the project cuts the same.
#[test]
fn v10_project_gains_an_empty_tab_list() {
    let root: Value = serde_json::from_slice(V10_FIXTURE).unwrap();
    let mut tabs_before = root["operations"][0]["tabs"].clone();
    let migrated = migrate(root, 10).unwrap();
    assert_eq!(migrated["tabs"], json!([]));
    // Version 14 adds `avoid_corners: false` to the request.
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

/// Version 10 only adds the `left` and `right` sides, so the
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

/// The new sides save as `"left"` and `"right"` and load back.
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

// Version 11 → 12: the clearance, tool-change and park heights.

const V11_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v11/sample.dcv");

/// Design addendum A7: a version 11 project gains `heights`
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

// Version 12 → 13: the Profile last pass allowance.

const V12_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v12/sample.dcv");

/// Every Profile of a version 12 file gains `last_pass: null`
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
        // Version 14 adds `start` after it.
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

/// Loaded from a file, a migration that fails on a key
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

/// A last pass saves with every field and loads back.
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

// Version 13 → 14: tabs off corners, and the start point.

const V13_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v13/sample.dcv");

/// Every Profile of a version 13 file gains `start: "auto"`
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

/// Each start saves as the spec writes it and loads back; a
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

// Version 14 → 15: whether a placed tab holds its slug.

const V14_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v14/sample.dcv");

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

/// Every placed tab of a version 14 file gains `holds_slug:
/// null` at its end, and nothing else changes. A migration step
/// rewrites JSON only, so it cannot tell whether a tab sits on the
/// tool path a Profile cuts round a hole, which is how format 14 told
/// a hole tab placed with `on_holes` from one placed by hand.
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

/// Loaded from a file, the version 14 file with a
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

/// Each value saves as written, at the end of the tab, and
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

/// A hand-edited `holds_slug` that is not `true`, `false` or
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

// Version 15 → 16: a tool's vendor
// and product id.

const V15_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v15/sample.dcv");

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
    others.as_object_mut().unwrap().shift_remove("bitmaps");
    without_format_29(&mut others);
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

// Version 16 → 17: a Drill's
// `allow_tool_wider_than_hole`.

const V16_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v16/sample.dcv");

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

const V17_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v17/sample.dcv");

/// Format 18: the Bore is a new
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
    let expected = at_current(&root);
    assert_eq!(migrate(root, 17).unwrap(), expected);
    assert_eq!(
        migrate(sample_root_at(17), 17).unwrap(),
        sample_root_at(CURRENT_VERSION)
    );
}

// Version 18 → 19: `allowance_mm` is `stock_to_leave_mm` on a
// Pocket, a Bore and a Profile's last pass.

const V18_STOCK_TO_LEAVE_FIXTURE: &[u8] =
    include_bytes!("../../tests/fixtures/dcv/v18/stock-to-leave.dcv");

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
        serde_json::from_str(&text.replace("\"allowance_mm\"", "\"stock_to_leave_mm\"")).unwrap();
    expected = at_current(&expected);
    assert_eq!(
        serde_json::to_string_pretty(&migrated).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap()
    );
    assert_eq!(
        migrate(sample_root_at(18), 18).unwrap(),
        sample_root_at(CURRENT_VERSION)
    );
}

// Version 19 → 20 (sub-phase 3.1): the V-Carve operation.

const V19_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v19/sample.dcv");

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
    // An Engrave of that version has nothing of format 30.
    without_format_30(&mut root);
    let expected = at_current(&root);
    let migrated = migrate(root, 19).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(
        serde_json::to_string_pretty(&migrated).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
        "the keys keep their order"
    );
}

// Version 20 → 21 (sub-phase 3.2): the countersink Drill cycle.

const V20_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v20/sample.dcv");

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
    // An Engrave of that version has nothing of format 30.
    without_format_30(&mut root);
    let expected = at_current(&root);
    let migrated = migrate(root, 20).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(
        serde_json::to_string_pretty(&migrated).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
        "the keys keep their order"
    );
}

// Version 21 → 22 (sub-phase 3.3): the Inlay operation kind.

const V21_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v21/sample.dcv");

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
    // An Engrave of that version has nothing of format 30.
    without_format_30(&mut root);
    let expected = at_current(&root);
    let migrated = migrate(root, 21).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(
        serde_json::to_string_pretty(&migrated).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
        "the keys keep their order"
    );
}

// Version 22 → 23: a male Inlay's cut-out.

const V22_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v22/sample.dcv");

/// Format 23: every Inlay gains `"cut_out": null` right after its
/// `safe_z_mm`, whatever its side and clearing; no other operation
/// changes, and the keys keep their order.
#[test]
fn v22_to_v23_adds_a_null_cut_out() {
    use crate::ids::ToolId;
    use crate::model::{ClearanceStrategy, ClearingPass, CutDirection, Entry, InlayOp};
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
        male_board: stock_board(),
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
        as_v24(&mut old);
        if let Some(fields) = old.as_object_mut() {
            fields.shift_remove("cut_out");
            // Format 24 adds it after.
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

// Version 23 → 24: a male Inlay's outline rounding.

const V23_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v23/sample.dcv");

/// Format 24: every Inlay gains `"outline_rounding_mm": null` right
/// after its `margin_mm`, whatever its side, clearing and cut-out; no
/// other operation changes, and the keys keep their order. A version
/// 23 Inlay that already has the field is not one.
#[test]
fn v23_to_v24_adds_null_outline_rounding() {
    use crate::ids::ToolId;
    use crate::model::{
        ClearanceStrategy, ClearingPass, CutDirection, Entry, InlayCutOut, InlayOp,
    };
    use crate::test_support::{sample_inlay, sample_pocket, sample_v_carve};
    const { assert!(CURRENT_VERSION >= 24) };
    let fixture: Value = serde_json::from_slice(V23_FIXTURE).unwrap();
    let stepped = v23_to_v24(fixture.as_object().unwrap().clone()).unwrap();
    assert_eq!(
        Value::Object(stepped),
        serde_json::from_slice::<Value>(V23_FIXTURE).unwrap(),
        "a file with no Inlay is unchanged"
    );

    let male = InlayOp {
        male_board: stock_board(),
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
        as_v24(&mut old);
        if let Some(fields) = old.as_object_mut() {
            fields.shift_remove("outline_rounding_mm");
        }
        root["operations"].as_array_mut().unwrap().push(old);
        expected["operations"].as_array_mut().unwrap().push(current);
    }
    for op in expected["operations"].as_array().unwrap() {
        if op["kind"] == json!("inlay") {
            let keys: Vec<&str> = op.as_object().unwrap().keys().map(String::as_str).collect();
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

#[test]
fn current_version_is_unchanged() {
    let root = serde_json::to_value(sample_project()).unwrap();
    assert_eq!(migrate(root.clone(), CURRENT_VERSION).unwrap(), root);
}

// Version 24 → 25: one-file Inlay, both boards from one project.

const V24_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v24/sample.dcv");

/// The sample root at version 24 with one Inlay `inlay` (written at
/// the current version, then given `side` in place of its
/// `male_board`).
fn v24_root_with_inlay(inlay: &crate::model::InlayOp, side: &str) -> Value {
    let mut root = sample_root_at(24);
    let mut op = serde_json::to_value(Operation::Inlay(inlay.clone())).unwrap();
    let fields = std::mem::take(op.as_object_mut().unwrap());
    let mut old = Map::new();
    for (k, v) in fields {
        if k == "male_board" {
            old.insert("side".into(), json!(side));
        } else if k != "boundary" {
            old.insert(k, v);
        }
    }
    root["operations"]
        .as_array_mut()
        .unwrap()
        .push(Value::Object(old));
    root
}

/// Format 25: a version 24 female Inlay gains an unset male
/// board, `{ "width_mm": null, "height_mm": null, "thickness_mm":
/// null }`, in the place its `side` held, after `tool`; nothing else
/// changes and the keys keep their order. Its male is not cut until
/// the user measures the male board, so the file exports the female
/// programs it did. A female is not noted.
#[test]
fn v24_to_v25_female_gets_an_unset_male_board() {
    use crate::model::{InlayOp, MaleBoard};
    use crate::test_support::sample_inlay;
    let female = InlayOp {
        male_board: MaleBoard::default(),
        ..sample_inlay()
    };
    let root = v24_root_with_inlay(&female, "female");
    let mut expected = sample_root_at(CURRENT_VERSION);
    expected["operations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(Operation::Inlay(female.clone())).unwrap());
    let inlay = &expected["operations"][1];
    assert_eq!(
        inlay["male_board"],
        json!({ "width_mm": null, "height_mm": null, "thickness_mm": null })
    );
    let keys: Vec<&str> = inlay
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let at = keys.iter().position(|k| *k == "tool").unwrap();
    assert_eq!(keys.get(at + 1), Some(&"male_board"));
    assert!(!keys.contains(&"side"), "{keys:?}");

    let (migrated, notes) = migrate_with_notes(root, 24).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(
        serde_json::to_string_pretty(&migrated).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
        "the keys keep their order"
    );
    assert_eq!(notes, vec![]);

    // A version 24 Inlay that already has a male board is not one.
    let mut bad = sample_root_at(24);
    bad["operations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(Operation::Inlay(female)).unwrap());
    let err = migrate(bad, 24).unwrap_err();
    assert!(
        matches!(err, LoadError::Migration { from: 24, .. }),
        "{err:?}"
    );
}

/// Format 25: a version 24 male Inlay gains a male board the
/// size of the stock, its width, height and thickness, so its male
/// programs are cut on the same board as before, byte for byte; the
/// project keeps its stock, on which the female is now cut too. The
/// migration notes it, naming the operation, so `check` can say that
/// the file now also cuts the female. A side that is neither is
/// refused, and so is a male whose stock has no width.
#[test]
fn v24_to_v25_male_gets_the_stock_as_its_male_board() {
    use crate::model::{InlayCutOut, InlayOp, MaleBoard};
    use crate::test_support::sample_inlay;
    let stock = sample_project().stock;
    let male = InlayOp {
        male_board: MaleBoard {
            width_mm: Some(stock.width_mm),
            height_mm: Some(stock.height_mm),
            thickness_mm: stock.thickness_mm,
        },
        cut_out: Some(InlayCutOut {
            tool: crate::ids::ToolId::new("t1"),
            tabs: None,
            through_cut_allowance_mm: 0.2,
        }),
        ..sample_inlay()
    };
    let root = v24_root_with_inlay(&male, "male");
    let mut expected = sample_root_at(CURRENT_VERSION);
    expected["operations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(Operation::Inlay(male.clone())).unwrap());
    assert_eq!(
        expected["operations"][1]["male_board"],
        json!({ "width_mm": 100.0, "height_mm": 100.0, "thickness_mm": 6.0 })
    );
    let (migrated, notes) = migrate_with_notes(root.clone(), 24).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(migrated["stock"], expected["stock"], "the stock is kept");
    assert_eq!(
        notes,
        vec![MigrationNote::MaleInlayGainsFemale {
            operation: male.id.clone(),
            name: male.name.clone(),
        }]
    );
    let text = notes[0].to_string();
    assert!(text.contains("female"), "{text}");
    assert!(text.contains("male programs"), "{text}");

    // An unset stock thickness carries over as an unset male one.
    let mut unmeasured = root.clone();
    unmeasured["stock"]["thickness_mm"] = Value::Null;
    let (migrated, _) = migrate_with_notes(unmeasured, 24).unwrap();
    assert_eq!(
        migrated["operations"][1]["male_board"],
        json!({ "width_mm": 100.0, "height_mm": 100.0, "thickness_mm": null })
    );

    for (field, value) in [("side", json!("both")), ("side", Value::Null)] {
        let mut bad = root.clone();
        bad["operations"][1][field] = value;
        let err = migrate(bad, 24).unwrap_err();
        assert!(
            matches!(err, LoadError::Migration { from: 24, .. }),
            "{err:?}"
        );
    }
    let mut no_stock = root;
    no_stock["stock"]
        .as_object_mut()
        .unwrap()
        .shift_remove("width_mm");
    let err = migrate(no_stock, 24).unwrap_err();
    assert!(
        matches!(err, LoadError::Migration { from: 24, .. }),
        "{err:?}"
    );
}

// Version 25 → 26: the Inlay's drawn boundary.

const V25_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v25/sample.dcv");
const V26_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v26/sample.dcv");
const V26_BOUNDARY_FIXTURE: &[u8] =
    include_bytes!("../../tests/fixtures/dcv/v26/inlay-boundary.dcv");

/// An Inlay `inlay` as a version 25 file wrote it: no `boundary`.
fn v25_inlay(inlay: &crate::model::InlayOp) -> Value {
    let mut op = serde_json::to_value(Operation::Inlay(inlay.clone())).unwrap();
    op.as_object_mut().unwrap().shift_remove("boundary");
    op
}

/// Format 26: every Inlay gains `"boundary": []` right after
/// `outline_rounding_mm`, and nothing else changes, keys in order.
/// Other operations are untouched. An Inlay that already has a
/// `boundary` is not a version 25 one.
#[test]
fn v25_to_v26_adds_an_empty_boundary() {
    use crate::model::{InlayOp, MaleBoard};
    use crate::test_support::sample_inlay;
    let female = sample_inlay();
    let male = InlayOp {
        id: crate::ids::OperationId::new("o8"),
        male_board: MaleBoard {
            width_mm: None,
            height_mm: None,
            thickness_mm: Some(8.0),
        },
        outline_rounding_mm: Some(4.0),
        ..sample_inlay()
    };
    let mut root = sample_root_at(25);
    let mut expected = sample_root_at(CURRENT_VERSION);
    for inlay in [&female, &male] {
        root["operations"]
            .as_array_mut()
            .unwrap()
            .push(v25_inlay(inlay));
        expected["operations"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::to_value(Operation::Inlay(inlay.clone())).unwrap());
    }
    for i in [1, 2] {
        let inlay = expected["operations"][i].as_object().unwrap();
        assert_eq!(inlay["boundary"], json!([]));
        let keys: Vec<&str> = inlay.keys().map(String::as_str).collect();
        let at = keys
            .iter()
            .position(|k| *k == "outline_rounding_mm")
            .unwrap();
        assert_eq!(keys.get(at + 1), Some(&"boundary"));
    }

    let (migrated, notes) = migrate_with_notes(root, 25).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(
        serde_json::to_string_pretty(&migrated).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
        "the keys keep their order"
    );
    assert_eq!(notes, vec![]);

    let mut bad = sample_root_at(25);
    bad["operations"]
        .as_array_mut()
        .unwrap()
        .push(serde_json::to_value(Operation::Inlay(female)).unwrap());
    let err = migrate(bad, 25).unwrap_err();
    assert!(
        matches!(err, LoadError::Migration { from: 25, .. }),
        "{err:?}"
    );
}

/// The version 26 fixtures load as they are: the plain sample, and
/// one whose Inlay lists a boundary instance, `i2`, that is not a
/// target.
#[test]
fn version_26_fixtures_load() {
    let text = std::str::from_utf8(V26_FIXTURE).unwrap();
    assert!(text.contains("\"version\": 26,"), "a version 26 file");
    assert_eq!(load(V26_FIXTURE).unwrap().project, sample_project());

    let loaded = load(V26_BOUNDARY_FIXTURE).unwrap();
    assert_eq!(loaded.project.version, CURRENT_VERSION);
    let inlay = loaded
        .project
        .operations
        .iter()
        .find_map(|o| match o {
            Operation::Inlay(i) => Some(i),
            _ => None,
        })
        .expect("an Inlay");
    assert_eq!(inlay.boundary, vec![crate::ids::InstanceId::new("i2")]);
    assert!(inlay.has_boundary());
    assert_eq!(loaded.problems, vec![]);
}

/// A version 25 project, whatever it holds, exports the programs it
/// did: the migration changes nothing a program is made of. Its Inlay
/// loads as the project written at version 26 with an empty boundary,
/// so `has_boundary` is false and `margin_mm` and
/// `outline_rounding_mm` still make the band, and saving it adds
/// exactly one line per Inlay, `"boundary": []`, to the file. (The CLI
/// test `v25_project_exports_the_same_programs` compares the
/// programs themselves.)
#[test]
fn a_v25_project_exports_the_same_programs() {
    use crate::model::{InlayOp, MaleBoard};
    use crate::test_support::{sample_inlay, tool_v_bit_90};
    let male = InlayOp {
        male_board: MaleBoard {
            width_mm: None,
            height_mm: None,
            thickness_mm: Some(8.0),
        },
        outline_rounding_mm: Some(4.0),
        ..sample_inlay()
    };
    let mut project = sample_project();
    project.tools.push(tool_v_bit_90());
    project.operations.push(Operation::Inlay(male));
    project.version = CURRENT_VERSION;
    let mut v25 = serde_json::to_value(&project).unwrap();
    v25["operations"][1]
        .as_object_mut()
        .unwrap()
        .shift_remove("boundary");
    v25.as_object_mut().unwrap().shift_remove("bitmaps");
    without_format_29(&mut v25);
    v25["version"] = json!(25);
    let text25 = serde_json::to_string_pretty(&v25).unwrap();

    let loaded = load(text25.as_bytes()).unwrap();
    assert_eq!(loaded.project, project, "the same project");
    let Operation::Inlay(inlay) = &loaded.project.operations[1] else {
        panic!("an Inlay");
    };
    assert!(!inlay.has_boundary());
    assert_eq!(inlay.margin_mm, Some(6.0));
    assert_eq!(inlay.outline_rounding_mm, Some(4.0));

    let saved = crate::save(&loaded.project).unwrap();
    let without: Vec<&str> = saved
        .lines()
        .filter(|l| !l.trim_start().starts_with("\"boundary\":"))
        .collect();
    assert_eq!(
        saved.lines().count() - without.len(),
        1,
        "one added line per Inlay"
    );
}

// Version 26 → 27 (sub-phase 3.5): bitmaps and the Photo V-Carve.

const V27_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v27/sample.dcv");
const V27_PHOTO_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v27/photo.dcv");

/// `root`, a project as the current version writes it, as version 26
/// wrote it: no `bitmaps`.
fn as_v26(mut root: Value) -> Value {
    without_format_29(&mut root);
    root.as_object_mut().unwrap().shift_remove("bitmaps");
    root["version"] = json!(26);
    root
}

/// Format 27: the root gains `"bitmaps": []` right after `instances`,
/// and nothing else changes, keys in order. The project here holds
/// every kind a version 26 file can, so none of them is touched.
#[test]
fn v26_to_v27_adds_an_empty_bitmap_list() {
    use crate::test_support::{sample_inlay, sample_v_carve, tool_v_bit_90};
    let mut project = sample_project();
    project.tools.push(tool_v_bit_90());
    project.operations.push(Operation::VCarve(sample_v_carve()));
    project.operations.push(Operation::Inlay(sample_inlay()));
    let expected = serde_json::to_value(&project).unwrap();
    assert_eq!(expected["bitmaps"], json!([]));
    let keys: Vec<&str> = expected
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let at = keys.iter().position(|k| *k == "instances").unwrap();
    assert_eq!(keys.get(at + 1), Some(&"bitmaps"));

    let root = as_v26(expected.clone());
    assert!(root.get("bitmaps").is_none());
    let (migrated, notes) = migrate_with_notes(root, 26).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(
        serde_json::to_string_pretty(&migrated).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
        "the keys keep their order"
    );
    assert_eq!(notes, vec![]);

    // A root with no `instances` (not a file this build wrote) still
    // gains the list, at the end.
    let mut bare = as_v26(expected);
    bare.as_object_mut().unwrap().shift_remove("instances");
    let migrated = migrate(bare, 26).unwrap();
    assert_eq!(migrated["bitmaps"], json!([]));
}

/// The version 27 fixtures load as they are: the plain sample, whose
/// bitmap list is empty, and one with a bitmap `b1` and a Photo
/// V-Carve of it.
#[test]
fn version_27_fixtures_load() {
    use crate::test_support::{SAMPLE_BITMAP_BYTES, photo_project};
    let text = std::str::from_utf8(V27_FIXTURE).unwrap();
    assert!(text.contains("\"version\": 27,"), "a version 27 file");
    assert!(text.contains("\n  \"bitmaps\": [],\n"), "{text}");
    let loaded = load(V27_FIXTURE).unwrap();
    assert_eq!(loaded.project, sample_project());
    assert_eq!(loaded.problems, vec![]);
    assert_eq!(loaded.notes, vec![]);

    let text = std::str::from_utf8(V27_PHOTO_FIXTURE).unwrap();
    assert!(text.contains("\"version\": 27,"), "a version 27 file");
    let loaded = load(V27_PHOTO_FIXTURE).unwrap();
    assert_eq!(loaded.project.version, CURRENT_VERSION);
    assert_eq!(loaded.problems, vec![]);
    // The same project but for the PNG's bytes: an encoder release
    // may write the same pixels differently, so no test compares
    // them (design spec, "Data model"); the pixels are compared below.
    let mut expected = photo_project();
    expected.bitmaps[0].png_base64 = loaded.project.bitmaps[0].png_base64.clone();
    assert_eq!(loaded.project, expected);
    assert_saved_adds_format_29_only(
        &crate::save(&loaded.project).unwrap(),
        &text.replacen(
            "\"version\": 27,",
            &format!("\"version\": {CURRENT_VERSION},"),
            1,
        ),
        "the version line, and what format 29 adds",
    );
    let Some(Operation::PhotoVCarve(photo)) = loaded.project.operations.last() else {
        panic!("a Photo V-Carve");
    };
    let bitmap = loaded
        .project
        .bitmaps
        .iter()
        .find(|b| b.id == photo.image)
        .expect("the bitmap the Photo V-Carve names");
    assert_eq!(bitmap.pixels().unwrap().bytes, SAMPLE_BITMAP_BYTES);
}

/// A version 26 project, whatever it holds, exports the programs it
/// did: it loads as the same project written at version 27 with an
/// empty bitmap list, and saving it adds exactly one line to the
/// file, `"bitmaps": []`. (The CLI test
/// `v26_project_exports_the_same_programs` compares the programs.)
#[test]
fn a_v26_project_loads_as_the_same_project() {
    for fixture in [V26_FIXTURE, V26_BOUNDARY_FIXTURE] {
        let old = std::str::from_utf8(fixture).unwrap();
        assert!(old.contains("\"version\": 26,"), "a version 26 file");
        assert!(!old.contains("bitmaps"), "a version 26 file");
        let loaded = load(fixture).unwrap();
        assert_eq!(loaded.project.version, CURRENT_VERSION);
        assert_eq!(loaded.project.bitmaps, vec![]);
        assert_eq!(loaded.notes, vec![]);
        let saved = crate::save(&loaded.project).unwrap();
        let expected = old
            .replacen(
                "\"version\": 26,",
                &format!("\"version\": {CURRENT_VERSION},"),
                1,
            )
            .replacen(
                "\n  \"post_processor\": {",
                "\n  \"bitmaps\": [],\n  \"post_processor\": {",
                1,
            );
        assert_saved_adds_format_29_only(&saved, &expected, "one line added, after the instances");
    }
}

// Version 27 → 28 (sub-phase 3.6): the tapered ball nose.

const V28_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v28/sample.dcv");
const V28_TAPERED_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v28/tapered.dcv");

/// Format 28 adds a cutter a version 27 file cannot hold, so the step
/// changes nothing but the version: every key, value and key order of
/// a project holding every kind a version 27 file can stays as it was.
#[test]
fn v27_to_v28_bumps_the_version_only() {
    use crate::test_support::{photo_project, sample_inlay, sample_v_carve};
    let mut project = photo_project();
    project.operations.push(Operation::VCarve(sample_v_carve()));
    project.operations.push(Operation::Inlay(sample_inlay()));
    let expected = serde_json::to_value(&project).unwrap();
    let mut root = expected.clone();
    without_format_29(&mut root);
    root["version"] = json!(27);
    let (migrated, notes) = migrate_with_notes(root, 27).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(
        serde_json::to_string_pretty(&migrated).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
        "the keys keep their order"
    );
    assert_eq!(notes, vec![]);
}

/// A file that says 27 and holds a `tapered_ball_nose` cutter is not
/// a version 27 file: the migration refuses it, naming the tool.
#[test]
fn v27_file_with_a_tapered_ball_nose_fails() {
    let mut root = serde_json::to_value(crate::test_support::tapered_project()).unwrap();
    root["version"] = json!(27);
    let err = migrate(root, 27).unwrap_err();
    match &err {
        LoadError::Migration {
            from: 27,
            to: 28,
            message,
            ..
        } => {
            assert!(message.contains("`tools[1].cutter`"), "{message}");
            assert!(message.contains("tapered_ball_nose"), "{message}");
        }
        _ => panic!("{err:?}"),
    }
    // An older file is refused by the same step, at the end of its chain.
    let mut root = sample_root_at(20);
    root["tools"][0]["cutter"] = json!({ "tapered_ball_nose": {
        "tip_diameter_mm": 0.762, "taper_angle_deg": 6.7, "flute_length_mm": 19.05
    } });
    let err = migrate(root, 20).unwrap_err();
    assert!(
        matches!(
            err,
            LoadError::Migration {
                from: 27,
                to: 28,
                ..
            }
        ),
        "{err:?}"
    );
}

/// The version 27 fixtures load, now as version 28 projects.
#[test]
fn version_27_fixture_loads() {
    let text = std::str::from_utf8(V27_FIXTURE).unwrap();
    assert!(text.contains("\"version\": 27,"), "a version 27 file");
    let loaded = load(V27_FIXTURE).unwrap();
    assert_eq!(loaded.project.version, CURRENT_VERSION);
    assert_eq!(loaded.project, sample_project());
    assert_eq!(loaded.problems, vec![]);
    assert_eq!(loaded.notes, vec![]);
    let loaded = load(V27_PHOTO_FIXTURE).unwrap();
    assert_eq!(loaded.project.version, CURRENT_VERSION);
    assert_eq!(loaded.problems, vec![]);
}

/// The version 28 fixtures load, now as version 29 projects: the plain
/// sample, and the sample with the Ultra-High Detail bit and a V-Carve
/// using it, which validation accepts since carving with a tapered ball
/// nose was built.
#[test]
fn version_28_fixtures_load() {
    use crate::test_support::tapered_project;
    let text = std::str::from_utf8(V28_FIXTURE).unwrap();
    assert!(text.contains("\"version\": 28,"), "a version 28 file");
    let loaded = load(V28_FIXTURE).unwrap();
    assert_eq!(loaded.project, sample_project());
    assert_eq!(loaded.problems, vec![]);
    assert_eq!(loaded.notes, vec![]);

    let text = std::str::from_utf8(V28_TAPERED_FIXTURE).unwrap();
    assert!(text.contains("\"version\": 28,"), "a version 28 file");
    assert!(text.contains("\"tapered_ball_nose\": {"), "{text}");
    let loaded = load(V28_TAPERED_FIXTURE).unwrap();
    assert_eq!(loaded.project, tapered_project());
    assert_eq!(loaded.problems, vec![]);
}

/// A version 27 project, whatever it holds, exports the programs it
/// did: it loads as the same project, and saving it changes one line
/// of the file, its version. (The CLI test
/// `v27_project_exports_the_same_programs` compares the programs.)
#[test]
fn a_v27_project_exports_the_same_programs() {
    for fixture in [V27_FIXTURE, V27_PHOTO_FIXTURE] {
        let old = std::str::from_utf8(fixture).unwrap();
        let loaded = load(fixture).unwrap();
        assert_eq!(loaded.notes, vec![]);
        let saved = crate::save(&loaded.project).unwrap();
        let expected = old.replacen(
            "\"version\": 27,",
            &format!("\"version\": {CURRENT_VERSION},"),
            1,
        );
        assert_saved_adds_format_29_only(&saved, &expected, "the version line only");
    }
}

// Version 28 → 29 (phase 5): what drawing in the app saves.

const V29_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v29/sample.dcv");
const V29_DRAWING_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v29/drawing.dcv");

/// Format 29 adds a definition's `shape`, an instance's `construction`
/// and `group`, the root's `groups` and a bitmap's `opacity_fraction`,
/// each where a saved file has it, and changes nothing else: a project
/// holding every kind a version 28 file can migrates to what this build
/// writes for it, keys in order (phase 5 acceptance 107).
#[test]
fn v28_to_v29_adds_the_drawing_fields() {
    use crate::test_support::{photo_project, sample_inlay, sample_v_carve};
    let mut project = photo_project();
    project.operations.push(Operation::VCarve(sample_v_carve()));
    project.operations.push(Operation::Inlay(sample_inlay()));
    let expected = serde_json::to_value(&project).unwrap();
    assert_eq!(expected["groups"], json!([]));
    assert_eq!(expected["definitions"]["d1"]["shape"], Value::Null);
    assert_eq!(expected["instances"][0]["construction"], json!(false));
    assert_eq!(expected["instances"][0]["group"], Value::Null);
    assert_eq!(expected["bitmaps"][0]["opacity_fraction"], json!(1.0));

    let mut root = expected.clone();
    without_format_29(&mut root);
    root["version"] = json!(28);
    assert!(root.get("groups").is_none());
    let (migrated, notes) = migrate_with_notes(root.clone(), 28).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(
        serde_json::to_string_pretty(&migrated).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
        "the keys keep their order"
    );
    assert_eq!(notes, vec![]);
    // The test's own reading of the format agrees with the step.
    let mut by_hand = with_format_29(&root);
    by_hand["version"] = json!(CURRENT_VERSION);
    assert_eq!(by_hand, expected);
}

/// A file that says 28 and already holds a format 29 field is not a
/// version 28 file: the migration refuses it, naming the field
/// (acceptance 107).
#[test]
fn a_file_claiming_28_with_a_format_29_field_is_refused() {
    let current = serde_json::to_value(crate::test_support::photo_project()).unwrap();
    type PutBack = fn(&mut Value);
    let cases: [(&str, PutBack); 5] = [
        ("`groups`", |root| root["groups"] = json!([])),
        ("`definitions[d1].shape`", |root| {
            root["definitions"]["d1"]["shape"] = Value::Null;
        }),
        ("`instances[0].construction`", |root| {
            root["instances"][0]["construction"] = json!(false);
        }),
        ("`instances[0].group`", |root| {
            root["instances"][0]["group"] = Value::Null;
        }),
        ("`bitmaps[0].opacity_fraction`", |root| {
            root["bitmaps"][0]["opacity_fraction"] = json!(1.0);
        }),
    ];
    for (field, put_back) in cases {
        let mut root = current.clone();
        without_format_29(&mut root);
        root["version"] = json!(28);
        put_back(&mut root);
        let err = migrate(root, 28).unwrap_err();
        match &err {
            LoadError::Migration {
                from: 28,
                to: 29,
                message,
                ..
            } => assert!(message.contains(field), "{field}: {message}"),
            _ => panic!("{field}: {err:?}"),
        }
    }
}

/// A version 28 project, whatever it holds, exports the programs it
/// did: it loads as the same project, and saving it adds what format 29
/// adds and changes nothing else. (The CLI test
/// `v28_project_exports_the_same_programs` compares the programs.)
#[test]
fn a_v28_project_exports_the_same_programs() {
    for fixture in [V28_FIXTURE, V28_TAPERED_FIXTURE] {
        let old = std::str::from_utf8(fixture).unwrap();
        let loaded = load(fixture).unwrap();
        assert_eq!(loaded.notes, vec![]);
        let saved = crate::save(&loaded.project).unwrap();
        let expected = old.replacen(
            "\"version\": 28,",
            &format!("\"version\": {CURRENT_VERSION},"),
            1,
        );
        assert_saved_adds_format_29_only(&saved, &expected, "the later formats' fields only");
    }
}

/// The version 29 fixtures, written by the build that had format 29 for
/// its own, load as the projects they were and save as version 30 files
/// with nothing else changed: neither holds an Engrave, the one kind
/// format 30 adds to.
#[test]
fn version_29_fixtures_load() {
    use crate::test_support::drawing_project;
    for (fixture, project) in [
        (V29_FIXTURE, sample_project()),
        (V29_DRAWING_FIXTURE, drawing_project()),
    ] {
        let text = std::str::from_utf8(fixture).unwrap();
        assert!(text.contains("\"version\": 29,"), "a version 29 file");
        let loaded = load(fixture).unwrap();
        assert_eq!(loaded.project, project);
        assert_eq!(loaded.problems, vec![]);
        assert_eq!(loaded.notes, vec![]);
        assert_eq!(
            crate::save(&loaded.project).unwrap(),
            text.replacen(
                "\"version\": 29,",
                &format!("\"version\": {CURRENT_VERSION},"),
                1
            )
        );
    }
}

const V30_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v30/sample.dcv");
const V30_DRAWING_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v30/drawing.dcv");

/// Format 30 adds where an Engrave's bit enters and leaves each path,
/// `enter_fraction` and `leave_fraction`, both `null` after
/// `alternate_direction`, and changes nothing else: a project with an
/// Engrave among the other kinds migrates to what this build writes for
/// it, keys in order. A project with no Engrave gains nothing but its
/// version.
#[test]
fn v29_to_v30_adds_where_an_engrave_enters_and_leaves() {
    use crate::test_support::{sample_engrave, sample_pocket};
    let mut project = sample_project();
    project.operations.push(Operation::Pocket(sample_pocket()));
    project
        .operations
        .push(Operation::Engrave(sample_engrave()));
    let expected = serde_json::to_value(&project).unwrap();
    let engrave = expected["operations"].as_array().unwrap().last().unwrap();
    assert_eq!(engrave["enter"], json!("auto"));
    assert_eq!(engrave["leave"], json!("auto"));

    let mut root = expected.clone();
    without_format_30(&mut root);
    root["version"] = json!(29);
    assert!(
        root["operations"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .get("enter")
            .is_none()
    );
    let (migrated, notes) = migrate_with_notes(root.clone(), 29).unwrap();
    assert_eq!(migrated, expected);
    assert_eq!(
        serde_json::to_string_pretty(&migrated).unwrap(),
        serde_json::to_string_pretty(&expected).unwrap(),
        "the keys keep their order"
    );
    assert_eq!(notes, vec![]);
    // The test's own reading of the format agrees with the step.
    let mut by_hand = with_format_30(&root);
    by_hand["version"] = json!(CURRENT_VERSION);
    assert_eq!(by_hand, expected);
    // What it means is what a version 29 Engrave was: an end of the path.
    let loaded: crate::Project = serde_json::from_value(migrated).unwrap();
    let Some(Operation::Engrave(engrave)) = loaded.operations.last() else {
        panic!("the Engrave is there");
    };
    assert_eq!(
        (engrave.enter, engrave.leave),
        (crate::PathPoint::Auto, crate::PathPoint::Auto)
    );
}

/// A file that says 29 and already holds a format 30 field is not a
/// version 29 file: the migration refuses it, naming the field.
#[test]
fn a_file_claiming_29_with_a_format_30_field_is_refused() {
    use crate::test_support::sample_engrave;
    let mut project = sample_project();
    project.operations = vec![Operation::Engrave(sample_engrave())];
    let current = serde_json::to_value(&project).unwrap();
    for field in ["enter_fraction", "leave_fraction"] {
        let mut root = current.clone();
        without_format_30(&mut root);
        root["version"] = json!(29);
        root["operations"][0][field] = Value::Null;
        let err = migrate(root, 29).unwrap_err();
        match &err {
            LoadError::Migration {
                from: 29,
                to: 30,
                message,
                ..
            } => assert!(
                message.contains(&format!("`operations[0].{field}`")),
                "{field}: {message}"
            ),
            _ => panic!("{field}: {err:?}"),
        }
    }
}

/// The version 30 fixtures, written by the build that had format 30 for
/// its own, load as the projects they were and save as current files with
/// nothing but the version changed: neither holds an Engrave, the one
/// kind format 31 changes.
#[test]
fn version_30_fixtures_load() {
    use crate::test_support::drawing_project;
    for (fixture, project) in [
        (V30_FIXTURE, sample_project()),
        (V30_DRAWING_FIXTURE, drawing_project()),
    ] {
        let text = std::str::from_utf8(fixture).unwrap();
        assert!(text.contains("\"version\": 30,"), "a version 30 file");
        let loaded = load(fixture).unwrap();
        assert_eq!(loaded.project, project);
        assert_eq!(loaded.problems, vec![]);
        assert_eq!(loaded.notes, vec![]);
        assert_eq!(
            crate::save(&loaded.project).unwrap(),
            text.replacen(
                "\"version\": 30,",
                &format!("\"version\": {CURRENT_VERSION},"),
                1
            )
        );
    }
}

// Version 30 → 31: an Engrave enters and leaves at any point.

const V31_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v31/sample.dcv");
const V31_DRAWING_FIXTURE: &[u8] = include_bytes!("../../tests/fixtures/dcv/v31/drawing.dcv");

/// Format 31 turns an Engrave's `enter_fraction` and `leave_fraction`
/// into `enter` and `leave`, in their place: `null` becomes `"auto"` and
/// a number a `fraction` of the same number, so every Engrave cuts as it
/// did (the CLI's `version_30_engrave_points_export_byte_identical_programs`
/// compares the programs). Every other key keeps its value and its place,
/// and no note is given. A project with no Engrave gains only its version.
#[test]
fn v30_to_v31_keeps_where_an_engrave_enters_and_leaves() {
    use crate::PathPoint;
    use crate::test_support::{sample_engrave, sample_pocket};
    for (enter, leave) in [
        (PathPoint::Auto, PathPoint::Auto),
        (
            PathPoint::Fraction {
                length_fraction: 0.25,
            },
            PathPoint::Fraction {
                length_fraction: 0.75,
            },
        ),
        (
            PathPoint::Fraction {
                length_fraction: 0.0,
            },
            PathPoint::Auto,
        ),
        (
            PathPoint::Auto,
            PathPoint::Fraction {
                length_fraction: 1.0,
            },
        ),
    ] {
        let mut project = sample_project();
        project.operations.push(Operation::Pocket(sample_pocket()));
        let mut engrave = sample_engrave();
        engrave.enter = enter;
        engrave.leave = leave;
        project.operations.push(Operation::Engrave(engrave));
        let expected = serde_json::to_value(&project).unwrap();

        let root = as_format_30(&expected);
        let old = root["operations"].as_array().unwrap().last().unwrap();
        assert!(old.get("enter").is_none(), "{old}");
        let (migrated, notes) = migrate_with_notes(root.clone(), 30).unwrap();
        assert_eq!(
            serde_json::to_string_pretty(&migrated).unwrap(),
            serde_json::to_string_pretty(&expected).unwrap(),
            "{enter:?} {leave:?}: the same values, keys in order"
        );
        assert_eq!(notes, vec![]);
        let loaded: crate::Project = serde_json::from_value(migrated).unwrap();
        assert_eq!(loaded, project, "{enter:?} {leave:?}");
    }

    // With no Engrave, only the version changes.
    let root = sample_root_at(30);
    let mut expected = root.clone();
    expected["version"] = json!(CURRENT_VERSION);
    assert_eq!(migrate(root, 30).unwrap(), expected);
}

/// A file that says 30 and holds an Engrave's `enter` or `leave`, or lacks
/// its `enter_fraction` or `leave_fraction`, is not a version 30 file: the
/// migration refuses it, naming the field.
#[test]
fn a_file_claiming_30_with_a_format_31_field_is_refused() {
    use crate::test_support::sample_engrave;
    let mut project = sample_project();
    project.operations = vec![Operation::Engrave(sample_engrave())];
    let v30 = as_format_30(&serde_json::to_value(&project).unwrap());
    let refusal = |root: Value| match migrate(root, 30).unwrap_err() {
        LoadError::Migration {
            from: 30,
            to: 31,
            message,
            ..
        } => message,
        err => panic!("{err:?}"),
    };
    for field in ["enter", "leave"] {
        let mut root = v30.clone();
        root["operations"][0][field] = json!("auto");
        let message = refusal(root);
        assert!(
            message.contains(&format!("`operations[0].{field}`")),
            "{field}: {message}"
        );
    }
    for field in ["enter_fraction", "leave_fraction"] {
        let mut root = v30.clone();
        root["operations"][0]
            .as_object_mut()
            .unwrap()
            .shift_remove(field);
        let message = refusal(root);
        assert!(
            message.contains(&format!("`operations[0].{field}`")),
            "{field}: {message}"
        );
    }
}

/// Set this environment variable to rewrite the version 31 fixtures from
/// their builders instead of only comparing them; the test then fails so
/// the run is not mistaken for a check. Run it again without.
const UPDATE_V31: &str = "DC_PROJECT_UPDATE_V31_FIXTURES";

/// The version 31 fixtures, the current ones, load as they are and save
/// byte for byte: the plain sample, and the drawing with a shape of every
/// kind but text.
#[test]
fn version_31_fixtures_load() {
    use crate::test_support::drawing_project;
    if std::env::var_os(UPDATE_V31).is_some() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dcv/v31");
        for (name, project) in [("sample", sample_project()), ("drawing", drawing_project())] {
            let text = crate::save(&project).unwrap();
            std::fs::write(format!("{dir}/{name}.dcv"), text).unwrap();
        }
        panic!("{UPDATE_V31} is set: the fixtures were rewritten, not checked");
    }
    for (fixture, project) in [
        (V31_FIXTURE, sample_project()),
        (V31_DRAWING_FIXTURE, drawing_project()),
    ] {
        let text = std::str::from_utf8(fixture).unwrap();
        assert!(text.contains("\"version\": 31,"), "a version 31 file");
        let loaded = load(fixture).unwrap();
        assert_eq!(loaded.project, project);
        assert_eq!(loaded.problems, vec![]);
        assert_eq!(crate::save(&loaded.project).unwrap(), text);
    }
    // What the drawing holds, by its keys in the file.
    let text = std::str::from_utf8(V31_DRAWING_FIXTURE).unwrap();
    for key in [
        "\"kind\": \"rectangle\"",
        "\"corner\": \"square\"",
        "\"rounded\": {",
        "\"chamfered\": {",
        "\"kind\": \"circle\"",
        "\"kind\": \"ellipse\"",
        "\"kind\": \"polygon\"",
        "\"measured\": \"flats\"",
        "\"kind\": \"slot\"",
        "\"kind\": \"gear\"",
        "\"flanks\": \"involute\"",
        "\"kind\": \"path\"",
        "\"construction\": true",
        "\"group\": \"g1\"",
        "\"together\": true",
        "\"opacity_fraction\": 0.4",
    ] {
        assert!(text.contains(key), "{key}");
    }
}

/// A version 30 reader refuses a version 31 file as newer (ADR-0002).
#[test]
fn version_31_is_newer_than_30() {
    assert_eq!(CURRENT_VERSION, 31);
    let err = migrate(sample_root_at(CURRENT_VERSION), CURRENT_VERSION + 1).unwrap_err();
    assert!(matches!(err, LoadError::NewerVersion { .. }), "{err:?}");
}

/// Every point an Engrave can enter or leave at saves as the chapter
/// writes it and reads back as itself.
#[test]
fn every_path_point_round_trips() {
    use crate::PathPoint;
    for (point, text) in [
        (PathPoint::Auto, json!("auto")),
        (
            PathPoint::Fraction {
                length_fraction: 0.25,
            },
            json!({ "fraction": { "length_fraction": 0.25 } }),
        ),
        (
            PathPoint::FromStart { distance_mm: 10.0 },
            json!({ "from_start": { "distance_mm": 10.0 } }),
        ),
        (
            PathPoint::FromEnd { distance_mm: 2.5 },
            json!({ "from_end": { "distance_mm": 2.5 } }),
        ),
        (
            PathPoint::Near {
                x_mm: 1.0,
                y_mm: -2.0,
            },
            json!({ "near": { "x_mm": 1.0, "y_mm": -2.0 } }),
        ),
    ] {
        assert_eq!(serde_json::to_value(point).unwrap(), text);
        assert_eq!(serde_json::from_value::<PathPoint>(text).unwrap(), point);
    }
    // A distance takes a unit, as every length does.
    assert_eq!(
        serde_json::from_value::<PathPoint>(json!({ "from_end": { "distance_mm": "1 in" } }))
            .unwrap(),
        PathPoint::FromEnd { distance_mm: 25.4 }
    );
    assert!(
        serde_json::from_value::<PathPoint>(json!({ "from_end": { "distance": 1.0 } })).is_err()
    );
}
