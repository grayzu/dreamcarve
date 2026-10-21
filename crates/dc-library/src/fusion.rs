// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Fusion 360 tool library import (`docs/specs/phase-2/design.md`,
//! "Algorithms → Fusion import (dc-library::fusion)"; functional spec,
//! capability 22).
//!
//! Autodesk publishes the format (`FusionToolLibrarySchemas.html` in
//! `cam.autodesk.com/tools/tools/Tooling Schema/JSON Schema.zip`), so
//! reading a user's library is interoperability (`AGENTS.md`, "File-format
//! interoperability"). The importer is written against that schema; no
//! Autodesk file is in the repository, and the test fixtures in
//! `tests/fixtures/fusion/` are hand-written in its structure.
//!
//! A `.tools` file is a zip holding the JSON document as `tools.json`; a
//! `.json` file is the document itself. [`import`] tells them apart by the
//! zip signature, not the extension. The document is `{ "version": >= 11,
//! "data": [ … ] }`. Unknown keys are ignored everywhere (the schema allows
//! additional properties and newer versions add fields); only the keys
//! mapped below are read.
//!
//! Mapping, per item of `data`:
//!
//! - `type` `"flat end mill"`, `"bull nose end mill"` and `"slot mill"` →
//!   [`ToolKind::EndMill`] (a bull nose's corner radius `RE` is not
//!   modelled; the name gains ` (bull nose r<RE>)`, `RE` as written in the
//!   file's unit); `"ball end mill"` → [`ToolKind::BallNose`];
//!   `"chamfer mill"` → [`ToolKind::VBit`]; `"drill"`, `"spot drill"`,
//!   `"center drill"`, `"counter sink"`, `"counter bore"` and `"reamer"` →
//!   [`ToolKind::Drill`]. Every other type the schema names is
//!   [`SkipReason::UnsupportedType`]; a type it does not name is
//!   [`SkipReason::UnknownType`].
//! - **Chamfer angle.** The schema describes `TA` as the "included angle
//!   between generatrixes in the axial plane section", which reads as the
//!   full angle. It is not: Fusion's own 90° chamfer mills carry `TA` 45,
//!   and IDC Woodcraft's published Fusion library agrees (maintainer
//!   decision, functional spec open question 10). So `TA` is the half
//!   angle and `included_angle_deg = 2·TA`. Do not re-derive this from the
//!   schema's wording.
//! - `unit` `"millimeters"` or `"inches"`; every length (`DC`, `stepdown`,
//!   `stepover`) and feed (`v_f`, `v_f_plunge`) is in that unit and is
//!   converted to mm or mm/min. `n` is RPM in either unit.
//! - `geometry.DC` → `diameter_mm`; absent or not positive is
//!   [`SkipReason::NoDiameter`]. `geometry.NOF` → `flutes`; 0 or absent is
//!   1 with [`ImportNote::FlutesUnset`].
//! - `post-process.number` above 0 → [`Tool::number`]; 0 or absent → `None`.
//!   Every preset of one Fusion tool shares it.
//! - `start-values.presets[]`: one library tool per preset, named
//!   `description` when there is one preset and `<description> (<preset
//!   name>)` when there are several. `n` → `rpm`, `v_f` → `feed_mm_min`,
//!   `v_f_plunge` → `plunge_mm_min`; a missing, zero or negative value stays
//!   `None` (never defaulted: export refuses until the user sets it) and an
//!   [`ImportNote`] says so. `stepdown` → `step_down_mm` and `stepover / DC`
//!   → `step_over_fraction` (clamped to 1, with a note), each only when its
//!   `use-stepdown` / `use-stepover` flag is true or absent. A tool with no
//!   presets imports once with every feed unset.
//! - `source` is `fusion:<tool guid>:<preset guid>`, the preset's index
//!   when it has no guid and `no-preset` for a tool without presets (and
//!   `data[<index>]` for a tool without a guid), so a re-import recognises
//!   each tool.
//!
//! Each imported tool passes [`crate::validate_tool`]; one that fails (a
//! `TA` of 90 or more, say) is skipped with [`SkipReason::Invalid`] rather
//! than failing the whole import. Nothing in the mapping depends on the
//! coordinate frame.

use std::borrow::Cow;
use std::fmt;
use std::io::Read;

use dc_project::{Tool, ToolId, ToolKind};
use serde_json::{Map, Value};

use crate::{LibraryError, LibraryToolId};

/// The oldest Fusion tool library `version` the importer reads (the
/// schema's minimum).
pub const MIN_FUSION_VERSION: u64 = 11;

/// The largest `tools.json` the importer unpacks from a `.tools` archive,
/// in bytes, so a hostile archive cannot exhaust memory. Real libraries
/// are a few megabytes.
pub const MAX_DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;

/// Millimetres per inch.
const MM_PER_INCH: f64 = 25.4;

/// What [`import`] read from a Fusion library.
#[derive(Debug, Clone, PartialEq)]
pub struct FusionImport {
    /// The tools that map to a DreamCarve cutter, one per preset, in file
    /// order.
    pub tools: Vec<ImportedTool>,
    /// The items that do not, in file order.
    pub skipped: Vec<Skipped>,
}

/// One library tool made from one preset of a Fusion tool.
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedTool {
    /// The tool, in millimetres. Its `id` is a placeholder (`t<n>`, the
    /// position in [`FusionImport::tools`]); [`crate::add`] replaces it.
    pub tool: Tool,
    /// `fusion:<tool guid>:<preset guid or index>`, the library tool's
    /// [`crate::LibraryTool::source`].
    pub source: String,
    /// What the user should know about the tool: values the file did not
    /// set, and values the import changed.
    pub notes: Vec<ImportNote>,
}

/// Something about an imported tool the report states.
#[derive(Debug, Clone, PartialEq)]
pub enum ImportNote {
    /// The Fusion tool has no presets, so feeds, plunge rate, spindle
    /// speed, step-down and stepover are all unset.
    NoPresets,
    /// The preset has no cutting feed (`v_f`).
    FeedUnset,
    /// The preset has no plunge feed (`v_f_plunge`).
    PlungeUnset,
    /// The preset has no spindle speed (`n`).
    RpmUnset,
    /// `NOF` is 0 or absent; the tool has 1 flute.
    FlutesUnset,
    /// The preset's stepover is wider than the tool; the fraction is 1.
    StepOverClamped {
        /// The preset's stepover, millimetres.
        step_over_mm: f64,
        /// The tool's diameter, millimetres.
        diameter_mm: f64,
    },
}

impl fmt::Display for ImportNote {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPresets => f.write_str("no presets in the file"),
            Self::FeedUnset => f.write_str("feed unset"),
            Self::PlungeUnset => f.write_str("plunge rate unset"),
            Self::RpmUnset => f.write_str("spindle speed unset"),
            Self::FlutesUnset => f.write_str("flute count unset, using 1"),
            Self::StepOverClamped {
                step_over_mm,
                diameter_mm,
            } => write!(
                f,
                "stepover {step_over_mm} mm is wider than the {diameter_mm} mm tool, using 1"
            ),
        }
    }
}

/// A Fusion library item that was not imported.
#[derive(Debug, Clone, PartialEq)]
pub struct Skipped {
    /// The item's `description`, else its `product-id`, else its type,
    /// else its position (`data[<index>]`).
    pub description: String,
    /// Why it was skipped.
    pub reason: SkipReason,
}

/// Why a Fusion library item was not imported.
#[derive(Debug, Clone, PartialEq)]
pub enum SkipReason {
    /// A type the schema names that DreamCarve has no cutter for (thread
    /// mill, tap, face mill, turning tool, probe, holder, …).
    UnsupportedType(String),
    /// A type the schema does not name.
    UnknownType(String),
    /// The item is not an object or has no `type` string.
    MissingType,
    /// `unit` is absent or neither `millimeters` nor `inches`; the
    /// numbers cannot be read safely without it.
    UnknownUnit(Option<String>),
    /// `geometry.DC` is absent or not positive.
    NoDiameter,
    /// A chamfer mill without `geometry.TA`.
    NoTaperAngle,
    /// The mapped tool breaks a library validation rule.
    Invalid {
        /// The field, such as `cutter.v_bit.included_angle_deg`.
        field: String,
        /// What is wrong with it.
        message: String,
    },
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedType(kind) => write!(f, "unsupported tool type `{kind}`"),
            Self::UnknownType(kind) => write!(f, "unknown tool type `{kind}`"),
            Self::MissingType => f.write_str("no tool type"),
            Self::UnknownUnit(Some(unit)) => write!(f, "unknown unit `{unit}`"),
            Self::UnknownUnit(None) => f.write_str("no unit"),
            Self::NoDiameter => f.write_str("no cutting diameter (`DC`)"),
            Self::NoTaperAngle => f.write_str("chamfer mill has no taper angle (`TA`)"),
            Self::Invalid { field, message } => write!(f, "{field} {message}"),
        }
    }
}

/// Whether `bytes` look like a Fusion 360 tool library rather than a
/// DreamCarve one: a `.tools` archive (a zip, by its signature) or a
/// `.json` document whose top-level object has a `data` array, which the
/// DreamCarve format never has.
#[must_use]
pub fn is_fusion_library(bytes: &[u8]) -> bool {
    if is_zip(bytes) {
        return true;
    }
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|root| root.get("data").map(Value::is_array))
        .unwrap_or(false)
}

/// Whether `bytes` start with a zip signature.
fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK")
}

/// Imports a Fusion 360 library: a `.tools` archive or its JSON document
/// (module docs).
pub fn import(_bytes: &[u8]) -> Result<FusionImport, LibraryError> {
    Err(LibraryError::NotAFusionLibrary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// `tests/fixtures/fusion/sample.json`.
    const SAMPLE_JSON: &[u8] = include_bytes!("../../../tests/fixtures/fusion/sample.json");
    /// `tests/fixtures/fusion/sample.tools`, the same document zipped.
    const SAMPLE_TOOLS: &[u8] = include_bytes!("../../../tests/fixtures/fusion/sample.tools");

    /// `a` and `b` agree to well within any rounding a conversion adds.
    fn approx(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    /// Asserts `actual` is `Some` and approximately `expected`.
    #[track_caller]
    fn assert_some_approx(actual: Option<f64>, expected: f64) {
        match actual {
            Some(value) => assert!(approx(value, expected), "{value} != {expected}"),
            None => panic!("expected {expected}, got None"),
        }
    }

    /// A version-36 library holding `items`.
    fn library(items: &[Value]) -> Vec<u8> {
        serde_json::to_vec(&json!({ "version": 36, "data": items })).expect("serializes")
    }

    /// Imports a library holding `items`.
    fn import_items(items: &[Value]) -> FusionImport {
        import(&library(items)).expect("imports")
    }

    /// A 6 mm, 2-flute flat end mill, number 5, with one preset.
    fn flat_end_mill() -> Value {
        json!({
            "type": "flat end mill",
            "unit": "millimeters",
            "description": "6 mm Flat End Mill",
            "guid": "tool-guid-1",
            "geometry": { "DC": 6, "NOF": 2, "LCF": 22, "OAL": 50 },
            "post-process": { "number": 5 },
            "start-values": { "presets": [ {
                "name": "Hardwood",
                "guid": "preset-guid-1",
                "n": 18000,
                "v_f": 1200,
                "v_f_plunge": 400,
                "stepdown": 2,
                "stepover": 2.4,
                "use-stepdown": true,
                "use-stepover": true
            } ] }
        })
    }

    /// `item` of `type` with a 6 mm diameter and no presets.
    fn of_type(kind: &str) -> Value {
        json!({
            "type": kind,
            "unit": "millimeters",
            "description": format!("a {kind}"),
            "guid": format!("guid-{kind}"),
            "geometry": { "DC": 6, "NOF": 2 }
        })
    }

    /// The one tool a single-preset item imports as.
    fn only_tool(item: Value) -> ImportedTool {
        let imported = import_items(&[item]);
        assert_eq!(imported.skipped, [], "nothing skipped");
        assert_eq!(imported.tools.len(), 1, "{:?}", imported.tools);
        imported.tools.into_iter().next().expect("one tool")
    }

    /// Sets `value` at `path` (keys) in `item`.
    fn set(item: &mut Value, path: &[&str], value: Value) {
        let mut at = item;
        for key in path {
            at = at.get_mut(*key).unwrap_or_else(|| panic!("no key {key}"));
        }
        *at = value;
    }

    /// The first preset of `item`, to edit.
    fn preset(item: &mut Value) -> &mut Map<String, Value> {
        item["start-values"]["presets"][0]
            .as_object_mut()
            .expect("preset object")
    }

    #[test]
    fn zip_and_plain_json_both_read() {
        let from_json = import(SAMPLE_JSON).expect("json imports");
        let from_zip = import(SAMPLE_TOOLS).expect("zip imports");
        assert_eq!(from_zip, from_json);
        assert!(!from_json.tools.is_empty());
        // The committed zip holds the committed JSON byte for byte.
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(SAMPLE_TOOLS)).expect("a zip");
        let mut entry = archive.by_name("tools.json").expect("tools.json entry");
        let mut inner = Vec::new();
        entry.read_to_end(&mut inner).expect("entry reads");
        assert!(
            inner == SAMPLE_JSON,
            "sample.tools is out of date; see the fixture README"
        );
    }

    #[test]
    fn flat_end_mill_maps_with_geometry_and_preset() {
        let imported = only_tool(flat_end_mill());
        assert_eq!(imported.source, "fusion:tool-guid-1:preset-guid-1");
        assert_eq!(imported.notes, []);
        let tool = imported.tool;
        assert_eq!(
            tool.name, "6 mm Flat End Mill",
            "one preset: the description"
        );
        assert_eq!(tool.cutter, ToolKind::EndMill);
        assert!(approx(tool.diameter_mm, 6.0));
        assert_eq!(tool.flutes, 2);
        assert_eq!(tool.number, Some(5));
        assert_eq!(tool.rpm, Some(18000));
        assert_some_approx(tool.feed_mm_min, 1200.0);
        assert_some_approx(tool.plunge_mm_min, 400.0);
        assert_some_approx(tool.step_down_mm, 2.0);
        assert_some_approx(tool.step_over_fraction, 0.4);
    }

    #[test]
    fn inch_tool_converts_to_mm() {
        let mut item = flat_end_mill();
        set(&mut item, &["unit"], json!("inches"));
        set(&mut item, &["geometry", "DC"], json!(0.25));
        let p = preset(&mut item);
        p.insert("v_f".into(), json!(60));
        p.insert("v_f_plunge".into(), json!(20));
        p.insert("stepdown".into(), json!(0.125));
        p.insert("stepover".into(), json!(0.1));
        let tool = only_tool(item).tool;
        assert!(approx(tool.diameter_mm, 6.35), "{}", tool.diameter_mm);
        assert_some_approx(tool.feed_mm_min, 1524.0);
        assert_some_approx(tool.plunge_mm_min, 508.0);
        assert_some_approx(tool.step_down_mm, 3.175);
        assert_some_approx(tool.step_over_fraction, 0.4);
        assert_eq!(tool.rpm, Some(18000), "RPM has no unit");
    }

    /// The included angle of the V-bit a chamfer mill with `TA` imports as.
    fn chamfer_angle(ta: f64) -> f64 {
        let mut item = of_type("chamfer mill");
        set(
            &mut item,
            &["geometry"],
            json!({ "DC": 12, "NOF": 2, "TA": ta }),
        );
        match only_tool(item).tool.cutter {
            ToolKind::VBit { included_angle_deg } => included_angle_deg,
            other => panic!("not a V-bit: {other:?}"),
        }
    }

    #[test]
    fn chamfer_mill_is_v_bit_with_double_taper() {
        assert!(approx(chamfer_angle(30.0), 60.0));
        assert!(approx(chamfer_angle(15.0), 30.0));
        // No TA: skipped, not guessed.
        let mut item = of_type("chamfer mill");
        set(&mut item, &["geometry"], json!({ "DC": 12 }));
        let imported = import_items(&[item]);
        assert_eq!(imported.tools, []);
        assert_eq!(imported.skipped[0].reason, SkipReason::NoTaperAngle);
        // TA 90 would be a 180° "V-bit": invalid, so skipped naming the field.
        let mut item = of_type("chamfer mill");
        set(&mut item, &["geometry"], json!({ "DC": 12, "TA": 90 }));
        let imported = import_items(&[item]);
        assert_eq!(imported.tools, []);
        assert!(
            matches!(&imported.skipped[0].reason, SkipReason::Invalid { field, .. }
                if field == "cutter.v_bit.included_angle_deg"),
            "{:?}",
            imported.skipped
        );
    }

    #[test]
    fn chamfer_mill_ta_45_is_a_90_degree_v_bit() {
        assert!(approx(chamfer_angle(45.0), 90.0));
    }

    #[test]
    fn drill_types_map_to_drill() {
        for kind in [
            "drill",
            "spot drill",
            "center drill",
            "counter sink",
            "counter bore",
            "reamer",
        ] {
            let tool = only_tool(of_type(kind)).tool;
            assert_eq!(tool.cutter, ToolKind::Drill, "{kind}");
            assert!(approx(tool.diameter_mm, 6.0), "{kind}");
        }
        for (kind, cutter) in [
            ("ball end mill", ToolKind::BallNose),
            ("slot mill", ToolKind::EndMill),
            ("bull nose end mill", ToolKind::EndMill),
        ] {
            assert_eq!(only_tool(of_type(kind)).tool.cutter, cutter, "{kind}");
        }
        let mut item = of_type("bull nose end mill");
        set(
            &mut item,
            &["geometry"],
            json!({ "DC": 8, "NOF": 3, "RE": 1 }),
        );
        assert_eq!(
            only_tool(item).tool.name,
            "a bull nose end mill (bull nose r1)"
        );
    }

    #[test]
    fn two_presets_become_two_tools() {
        let mut item = flat_end_mill();
        let first = preset(&mut item).clone();
        let mut second = first.clone();
        second.insert("name".into(), json!("Acrylic"));
        second.insert("guid".into(), json!("preset-guid-2"));
        second.insert("v_f".into(), json!(800));
        let mut third = second.clone();
        third.remove("guid");
        third.insert("name".into(), json!("MDF"));
        set(
            &mut item,
            &["start-values", "presets"],
            json!([first, second, third]),
        );
        let imported = import_items(&[item]);
        let names: Vec<&str> = imported
            .tools
            .iter()
            .map(|t| t.tool.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "6 mm Flat End Mill (Hardwood)",
                "6 mm Flat End Mill (Acrylic)",
                "6 mm Flat End Mill (MDF)"
            ]
        );
        let sources: Vec<&str> = imported.tools.iter().map(|t| t.source.as_str()).collect();
        assert_eq!(
            sources,
            [
                "fusion:tool-guid-1:preset-guid-1",
                "fusion:tool-guid-1:preset-guid-2",
                "fusion:tool-guid-1:2"
            ],
            "the preset guid, else its index"
        );
        assert_some_approx(imported.tools[1].tool.feed_mm_min, 800.0);
        assert!(
            imported.tools.iter().all(|t| t.tool.number == Some(5)),
            "every preset shares the number"
        );
    }

    #[test]
    fn no_presets_imports_with_feeds_unset() {
        let mut item = flat_end_mill();
        item.as_object_mut().expect("object").remove("start-values");
        let imported = only_tool(item);
        assert_eq!(imported.source, "fusion:tool-guid-1:no-preset");
        let tool = &imported.tool;
        assert_eq!(tool.name, "6 mm Flat End Mill");
        assert_eq!(tool.feed_mm_min, None);
        assert_eq!(tool.plunge_mm_min, None);
        assert_eq!(tool.rpm, None);
        assert_eq!(tool.step_down_mm, None);
        assert_eq!(tool.step_over_fraction, None);
        assert_eq!(tool.number, Some(5));
        for note in [
            ImportNote::NoPresets,
            ImportNote::FeedUnset,
            ImportNote::PlungeUnset,
            ImportNote::RpmUnset,
        ] {
            assert!(
                imported.notes.contains(&note),
                "{note:?} in {:?}",
                imported.notes
            );
        }
        // An empty presets list is the same.
        let mut item = flat_end_mill();
        set(&mut item, &["start-values", "presets"], json!([]));
        assert_eq!(only_tool(item).tool, imported.tool);
    }

    #[test]
    fn missing_feed_stays_none() {
        let mut item = flat_end_mill();
        preset(&mut item).remove("v_f");
        let imported = only_tool(item);
        assert_eq!(imported.tool.feed_mm_min, None, "never defaulted");
        assert_some_approx(imported.tool.plunge_mm_min, 400.0);
        assert_eq!(imported.tool.rpm, Some(18000));
        assert_eq!(imported.notes, [ImportNote::FeedUnset]);

        // Zero is not a feed either: unset, not a sentinel.
        let mut item = flat_end_mill();
        let p = preset(&mut item);
        p.insert("v_f_plunge".into(), json!(0));
        p.remove("n");
        let imported = only_tool(item);
        assert_eq!(imported.tool.plunge_mm_min, None);
        assert_eq!(imported.tool.rpm, None);
        assert_eq!(
            imported.notes,
            [ImportNote::PlungeUnset, ImportNote::RpmUnset]
        );
    }

    #[test]
    fn drill_preset_without_v_f_imports_with_feed_unset() {
        let mut item = of_type("drill");
        item["start-values"] = json!({ "presets": [
            { "name": "Drilling", "guid": "p", "n": 6000, "v_f_plunge": 300, "v_f_retract": 300 }
        ] });
        let imported = only_tool(item);
        assert_eq!(imported.tool.cutter, ToolKind::Drill);
        assert_eq!(imported.tool.feed_mm_min, None);
        assert_some_approx(imported.tool.plunge_mm_min, 300.0);
        assert_eq!(imported.tool.rpm, Some(6000));
        assert_eq!(imported.notes, [ImportNote::FeedUnset]);
    }

    #[test]
    fn stepover_length_becomes_fraction() {
        let tool = only_tool(flat_end_mill()).tool;
        assert_some_approx(tool.step_over_fraction, 0.4);

        // A slotting preset's stepover equal to the diameter is 1.
        let mut item = flat_end_mill();
        preset(&mut item).insert("stepover".into(), json!(6));
        assert_some_approx(only_tool(item).tool.step_over_fraction, 1.0);

        // Wider than the tool: clamped to 1, with a note.
        let mut item = flat_end_mill();
        preset(&mut item).insert("stepover".into(), json!(9));
        let imported = only_tool(item);
        assert_some_approx(imported.tool.step_over_fraction, 1.0);
        assert_eq!(
            imported.notes,
            [ImportNote::StepOverClamped {
                step_over_mm: 9.0,
                diameter_mm: 6.0
            }]
        );

        // Flags absent: the values still apply.
        let mut item = flat_end_mill();
        let p = preset(&mut item);
        p.remove("use-stepover");
        p.remove("use-stepdown");
        let tool = only_tool(item).tool;
        assert_some_approx(tool.step_over_fraction, 0.4);
        assert_some_approx(tool.step_down_mm, 2.0);
    }

    #[test]
    fn use_stepdown_false_leaves_step_down_unset() {
        let mut item = flat_end_mill();
        preset(&mut item).insert("use-stepdown".into(), json!(false));
        let tool = only_tool(item).tool;
        assert_eq!(tool.step_down_mm, None);
        assert_some_approx(tool.step_over_fraction, 0.4);

        let mut item = flat_end_mill();
        preset(&mut item).insert("use-stepover".into(), json!(false));
        let tool = only_tool(item).tool;
        assert_eq!(tool.step_over_fraction, None);
        assert_some_approx(tool.step_down_mm, 2.0);
    }

    #[test]
    fn unsupported_types_skipped_by_name() {
        let kinds = [
            "thread mill",
            "tap right hand",
            "tap left hand",
            "face mill",
            "form mill",
            "lollipop mill",
            "dovetail mill",
            "tapered mill",
            "radius mill",
            "corner chamfer end mill",
            "circle segment barrel",
            "boring bar",
            "turning general",
            "turning threading",
            "probe",
            "holder",
            "laser cutter",
            "tool block",
        ];
        let items: Vec<Value> = kinds.iter().map(|kind| of_type(kind)).collect();
        let imported = import_items(&items);
        assert_eq!(imported.tools, []);
        let expected: Vec<Skipped> = kinds
            .iter()
            .map(|kind| Skipped {
                description: format!("a {kind}"),
                reason: SkipReason::UnsupportedType((*kind).into()),
            })
            .collect();
        assert_eq!(imported.skipped, expected);
        assert_eq!(
            imported.skipped[0].reason.to_string(),
            "unsupported tool type `thread mill`"
        );

        // A type outside the schema; no type; no diameter; no unit.
        let mut no_type = of_type("flat end mill");
        no_type.as_object_mut().expect("object").remove("type");
        let mut no_dc = of_type("flat end mill");
        set(&mut no_dc, &["geometry", "DC"], json!(0));
        let mut no_unit = of_type("flat end mill");
        no_unit.as_object_mut().expect("object").remove("unit");
        let mut feet = of_type("flat end mill");
        set(&mut feet, &["unit"], json!("feet"));
        let imported = import_items(&[
            of_type("laser sword"),
            no_type,
            no_dc,
            no_unit,
            feet,
            json!(42),
        ]);
        assert_eq!(imported.tools, []);
        let reasons: Vec<&SkipReason> = imported.skipped.iter().map(|s| &s.reason).collect();
        assert_eq!(
            reasons,
            [
                &SkipReason::UnknownType("laser sword".into()),
                &SkipReason::MissingType,
                &SkipReason::NoDiameter,
                &SkipReason::UnknownUnit(None),
                &SkipReason::UnknownUnit(Some("feet".into())),
                &SkipReason::MissingType,
            ]
        );
        assert_eq!(imported.skipped[5].description, "data[5]");
    }

    #[test]
    fn unknown_keys_are_ignored() {
        let plain = import_items(&[flat_end_mill()]);
        let mut item = flat_end_mill();
        let obj = item.as_object_mut().expect("object");
        obj.insert("x-new-field".into(), json!({ "nested": [1, 2, 3] }));
        obj.insert("holder".into(), json!({ "type": "holder", "segments": [] }));
        item["geometry"]["x-new"] = json!("value");
        item["post-process"]["x-new"] = json!(true);
        item["start-values"]["x-new"] = json!(null);
        preset(&mut item).insert("x-new".into(), json!(7));
        let bytes = serde_json::to_vec(&json!({
            "version": 99,
            "x-root-field": "ignored",
            "data": [item]
        }))
        .expect("serializes");
        assert_eq!(import(&bytes).expect("imports"), plain);
    }

    #[test]
    fn not_a_library_is_error() {
        assert!(matches!(
            import(b"{ not json"),
            Err(LibraryError::Json { line: 1, .. })
        ));
        assert_eq!(import(b"[1, 2]"), Err(LibraryError::NotAFusionLibrary));
        assert_eq!(
            import(br#"{"version": 36, "tools": []}"#),
            Err(LibraryError::NotAFusionLibrary)
        );
        assert_eq!(
            import(br#"{"version": 36, "data": {}}"#),
            Err(LibraryError::NotAFusionLibrary)
        );
        assert!(matches!(
            import(b"PK\x03\x04not really a zip"),
            Err(LibraryError::FusionArchive { .. })
        ));
        // A real zip without a JSON entry.
        let mut bytes = std::io::Cursor::new(Vec::new());
        {
            let mut writer = zip::ZipWriter::new(&mut bytes);
            writer
                .start_file(
                    "readme.txt",
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Stored),
                )
                .expect("entry starts");
            std::io::Write::write_all(&mut writer, b"hello").expect("entry writes");
            writer.finish().expect("zip finishes");
        }
        let err = import(bytes.get_ref()).expect_err("no JSON entry");
        assert!(
            matches!(&err, LibraryError::FusionArchive { message } if message.contains("tools.json")),
            "{err}"
        );
    }

    #[test]
    fn schema_version_below_11_is_rejected() {
        let with_version = |version: Value| {
            serde_json::to_vec(&json!({ "version": version, "data": [flat_end_mill()] }))
                .expect("serializes")
        };
        assert_eq!(
            import(&with_version(json!(10))),
            Err(LibraryError::FusionVersion { found: "10".into() })
        );
        assert!(
            import(&with_version(json!(10)))
                .expect_err("rejected")
                .to_string()
                .contains("10")
        );
        assert_eq!(
            import(&with_version(json!("eleven"))),
            Err(LibraryError::FusionVersion {
                found: "\"eleven\"".into()
            })
        );
        assert_eq!(
            import(&with_version(json!(11)))
                .expect("11 reads")
                .tools
                .len(),
            1
        );
        assert_eq!(
            import(&serde_json::to_vec(&json!({ "data": [] })).expect("serializes")),
            Err(LibraryError::MissingVersion)
        );
    }

    #[test]
    fn tool_number_is_read_when_non_zero() {
        assert_eq!(only_tool(flat_end_mill()).tool.number, Some(5));
        let mut item = flat_end_mill();
        set(&mut item, &["post-process", "number"], json!(12));
        assert_eq!(only_tool(item).tool.number, Some(12));
    }

    #[test]
    fn tool_number_zero_is_none() {
        let mut item = flat_end_mill();
        set(&mut item, &["post-process", "number"], json!(0));
        assert_eq!(only_tool(item).tool.number, None);
        let mut item = flat_end_mill();
        item.as_object_mut().expect("object").remove("post-process");
        assert_eq!(only_tool(item).tool.number, None);
    }

    #[test]
    fn sample_library_imports_every_supported_type_and_skips_the_rest() {
        let imported = import(SAMPLE_JSON).expect("sample imports");
        let summary: Vec<(&str, &ToolKind)> = imported
            .tools
            .iter()
            .map(|t| (t.tool.name.as_str(), &t.tool.cutter))
            .collect();
        let v90 = ToolKind::VBit {
            included_angle_deg: 90.0,
        };
        assert_eq!(
            summary,
            [
                ("6 mm Flat End Mill", &ToolKind::EndMill),
                ("3 mm Ball End Mill", &ToolKind::BallNose),
                ("8 mm Bull Nose End Mill (bull nose r1)", &ToolKind::EndMill),
                ("20 mm Slot Mill", &ToolKind::EndMill),
                ("12 mm 90-Degree Chamfer Mill", &v90),
                ("5 mm Drill", &ToolKind::Drill),
                ("6 mm 90-Degree Spot Drill", &ToolKind::Drill),
                ("10 mm 90-Degree Countersink", &ToolKind::Drill),
                ("8 mm Counter Bore", &ToolKind::Drill),
                ("6 mm Reamer", &ToolKind::Drill),
                ("1/4 in Flat End Mill", &ToolKind::EndMill),
                ("3.175 mm Flat End Mill (Plywood)", &ToolKind::EndMill),
                ("3.175 mm Flat End Mill (Acrylic)", &ToolKind::EndMill),
            ]
        );
        let skipped: Vec<(&str, String)> = imported
            .skipped
            .iter()
            .map(|s| (s.description.as_str(), s.reason.to_string()))
            .collect();
        let unsupported = |kind: &str| format!("unsupported tool type `{kind}`");
        assert_eq!(
            skipped,
            [
                ("6 mm Thread Mill, 1 mm Pitch", unsupported("thread mill")),
                ("M6 Right-Hand Tap", unsupported("tap right hand")),
                ("50 mm Face Mill", unsupported("face mill")),
                ("Ogee Form Router Bit", unsupported("form mill")),
                ("6 mm Lollipop Mill", unsupported("lollipop mill")),
                (
                    "12 mm 14-Degree Dovetail Mill",
                    unsupported("dovetail mill")
                ),
                (
                    "3 mm Tapered Ball Mill, 2-Degree Taper",
                    unsupported("tapered mill")
                ),
                ("General Turning Tool", unsupported("turning general")),
                ("Probe, 2 mm Stylus", unsupported("probe")),
                ("ER20 Collet Holder", unsupported("holder")),
            ]
        );
        // Every imported tool passes the library rules.
        for t in &imported.tools {
            crate::validate_tool(&LibraryToolId::new("lt1"), &t.tool).expect("valid");
        }
        // The flat end mill carries number 5, the ball end mill's 0 is None,
        // the two-preset tool shares 7; the inch tool is in mm.
        let by_name = |name: &str| {
            imported
                .tools
                .iter()
                .find(|t| t.tool.name == name)
                .unwrap_or_else(|| panic!("no {name}"))
        };
        assert_eq!(by_name("6 mm Flat End Mill").tool.number, Some(5));
        assert_eq!(by_name("3 mm Ball End Mill").tool.number, None);
        assert_eq!(
            by_name("3.175 mm Flat End Mill (Plywood)").tool.number,
            Some(7)
        );
        assert_eq!(
            by_name("3.175 mm Flat End Mill (Acrylic)").tool.number,
            Some(7)
        );
        assert_eq!(
            by_name("3.175 mm Flat End Mill (Acrylic)")
                .tool
                .step_down_mm,
            None
        );
        assert!(approx(
            by_name("1/4 in Flat End Mill").tool.diameter_mm,
            6.35
        ));
        assert_eq!(by_name("5 mm Drill").notes, [ImportNote::FeedUnset]);
        assert_eq!(by_name("5 mm Drill").tool.feed_mm_min, None);
        // Sources are unique, so a re-import recognises each tool.
        let mut sources: Vec<&str> = imported.tools.iter().map(|t| t.source.as_str()).collect();
        sources.sort_unstable();
        sources.dedup();
        assert_eq!(sources.len(), imported.tools.len());
    }
}
