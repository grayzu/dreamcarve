// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/cam/toolLibraryDialogModel.test.ts

//! Slice 8 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan → Tools
//! and library"; functional spec, capability 64 and acceptance 71): the
//! Tool Library window's model. Filtering, search and the "in project"
//! marking are ported from PureCutCNC's `toolLibraryDialogModel.test.ts`,
//! case by case, with these changes:
//!
//! - DreamCarve's library is millimetres only, so there is no units
//!   filter; the diameter a search matches is the one the table shows,
//!   in the project's display units. Upstream's entries carry their own
//!   units, so `0.25` and `6` matched one entry each; here the inch case
//!   searches an inch display, and `6` also matches the 6.35 mm end mill.
//! - Search also reads the vendor (capability 64: name, vendor, diameter).
//! - *Add to project* copies every ticked row, in-project ones included,
//!   as `tools add` copies whatever it is given (ADR-0002, mock-up 04's
//!   "Add 2 tools to project" with an in-project row ticked). Upstream
//!   skipped rows already imported; the count and the build follow.
//! - "In project" compares every field but the id, where upstream
//!   compared its own list of fields (and ignored `maxCutDepth`, which
//!   DreamCarve does not have).
//!
//! Plus the Fusion import report, which must be the one `tools import`
//! prints (acceptance 35 and 71).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::collections::BTreeSet;

use dc_app::library::{Filter, KindFilter, annotate, build_add, count_selected, filter};
use dc_library::{Library, LibraryTool, LibraryToolId};
use dc_project::{Tool, ToolId, ToolKind, Units};

use common::{fixture, plaque, read};

/// Upstream's `makeEntry`: a 1/4" end mill with every value set, under
/// library id `key`.
fn entry(key: &str, name: &str, cutter: ToolKind, diameter_mm: f64) -> LibraryTool {
    LibraryTool {
        id: LibraryToolId::new(key),
        source: None,
        tool: Tool {
            id: ToolId::new(key),
            name: name.into(),
            vendor: None,
            product_id: None,
            number: None,
            cutter,
            diameter_mm,
            flutes: 2,
            feed_mm_min: Some(1016.0),
            plunge_mm_min: Some(508.0),
            rpm: Some(18000),
            step_down_mm: Some(3.175),
            step_over_fraction: Some(0.4),
        },
    }
}

/// Upstream's four `filterLibraryEntries` entries, in millimetres.
fn four() -> Library {
    Library {
        tools: vec![
            entry("e1", "1/4\" Endmill", ToolKind::EndMill, 6.35),
            entry("e2", "6mm Endmill", ToolKind::EndMill, 6.0),
            entry("e3", "1/8\" Ball", ToolKind::BallNose, 3.175),
            entry(
                "e4",
                "90° V-Bit",
                ToolKind::VBit {
                    included_angle_deg: 90.0,
                },
                12.0,
            ),
        ],
        ..Library::default()
    }
}

fn keys(found: &[&LibraryTool]) -> Vec<String> {
    found.iter().map(|t| t.id.to_string()).collect()
}

fn search(text: &str) -> Filter {
    Filter {
        search: text.into(),
        kind: KindFilter::All,
    }
}

fn of_kind(kind: KindFilter) -> Filter {
    Filter {
        search: String::new(),
        kind,
    }
}

/// Upstream `describe('filterLibraryEntries')` and
/// `describe('filterLibraryEntryStates')`, with the changes in the module
/// docs.
#[test]
fn filter_and_search() {
    let lib = four();
    let mm = Units::Mm;

    // 'returns all entries with default filters'
    assert_eq!(filter(&lib, &Filter::default(), mm).len(), 4);
    // 'filters by type'
    assert_eq!(keys(&filter(&lib, &of_kind(KindFilter::VBit), mm)), ["e4"]);
    assert_eq!(
        keys(&filter(&lib, &of_kind(KindFilter::EndMill), mm)),
        ["e1", "e2"]
    );
    assert!(filter(&lib, &of_kind(KindFilter::Drill), mm).is_empty());
    // A V-bit of any angle is a V-bit.
    assert!(KindFilter::VBit.matches(&ToolKind::VBit {
        included_angle_deg: 30.0
    }));
    // 'filters by type and search together' (upstream: type and units)
    let both = Filter {
        search: "1/4".into(),
        kind: KindFilter::EndMill,
    };
    assert_eq!(keys(&filter(&lib, &both, mm)), ["e1"]);
    // 'matches search text against name'
    assert_eq!(keys(&filter(&lib, &search("Endmill"), mm)), ["e1", "e2"]);
    // 'matches search text against diameter': the inch display shows
    // 6.35 mm as 0.25.
    assert_eq!(
        keys(&filter(&lib, &search("0.25"), Units::Inch)),
        ["e1"],
        "the diameter as the table shows it"
    );
    assert_eq!(keys(&filter(&lib, &search("3.175"), mm)), ["e3"]);
    // 'matches partial diameter': `6` is in 6mm's name and in 6.35.
    assert_eq!(keys(&filter(&lib, &search("6"), mm)), ["e1", "e2"]);
    assert_eq!(keys(&filter(&lib, &search("12"), mm)), ["e4"]);
    // 'returns empty array when nothing matches filters'
    assert!(filter(&lib, &search("nonexistent"), mm).is_empty());
    // 'is case-insensitive'
    assert_eq!(filter(&lib, &search("endmill"), mm).len(), 2);
    // Upstream's terms: whitespace is collapsed, and any term matching is
    // a match.
    assert_eq!(filter(&lib, &search("  ENDMILL   ball "), mm).len(), 3);
    assert_eq!(
        filter(&lib, &search("   "), mm).len(),
        4,
        "blank is no search"
    );

    // Search reads the vendor too (capability 64).
    let mut vendors = lib.clone();
    vendors.tools[2].tool.vendor = Some("Amana Tool".into());
    assert_eq!(keys(&filter(&vendors, &search("amana"), mm)), ["e3"]);

    // `filterLibraryEntryStates`: the same contract over annotated rows.
    let annotated_lib = Library {
        tools: vec![
            entry("a", "Aluminum Specific Tool", ToolKind::EndMill, 6.0),
            entry("b", "Roughing Mill", ToolKind::BallNose, 6.0),
            entry("c", "Finish Endmill", ToolKind::EndMill, 6.0),
        ],
        ..Library::default()
    };
    let project_tools = [Tool {
        id: ToolId::new("t1"),
        ..annotated_lib.tools[1].tool.clone()
    }];
    let states = annotate(&annotated_lib, &project_tools);
    let shown = |f: &Filter| -> Vec<String> {
        dc_app::library::filter_states(&states, f, mm)
            .iter()
            .map(|s| s.entry.id.to_string())
            .collect()
    };
    // 'filters annotated entries by type'
    assert_eq!(shown(&of_kind(KindFilter::BallNose)), ["b"]);
    // 'filters annotated entries by search'
    assert_eq!(shown(&search("Aluminum")), ["a"]);
    // 'returns empty when nothing matches'
    assert!(shown(&search("nonexistent")).is_empty());
}

/// Upstream `describe('toolMatchesLibraryEntry')` and
/// `describe('annotateEntryStates')`: a library row is marked "in
/// project" when a project tool equals it in every field but the id; a
/// changed name, diameter or feed is another tool. Then the plaque: a
/// copy added from the library is marked, and editing the project's copy
/// unmarks it (the project keeps its copy, ADR-0002).
#[test]
fn in_project_marking() {
    let library_tool = entry("e1", "1/4\" Endmill", ToolKind::EndMill, 6.35);
    let tool = Tool {
        id: ToolId::new("proj_tool_1"),
        ..library_tool.tool.clone()
    };
    let matches = dc_app::library::tool_matches_library_entry;
    // 'matches identical entries'
    assert!(matches(&tool, &library_tool));
    // 'rejects a tool with different name'
    let renamed = Tool {
        name: "Different".into(),
        ..tool.clone()
    };
    assert!(!matches(&renamed, &library_tool));
    // 'rejects a tool with different units' has no DreamCarve case; a
    // different feed stands in for upstream's list of compared values.
    let faster = Tool {
        feed_mm_min: Some(2000.0),
        ..tool.clone()
    };
    assert!(!matches(&faster, &library_tool));
    // 'rejects a tool with different diameter'
    let wider = Tool {
        diameter_mm: 12.7,
        ..tool.clone()
    };
    assert!(!matches(&wider, &library_tool));

    // 'marks entries already in the project'
    let lib = Library {
        tools: vec![library_tool.clone()],
        ..Library::default()
    };
    let states = annotate(&lib, std::slice::from_ref(&tool));
    assert_eq!(states.len(), 1);
    assert!(states[0].in_project);
    // 'leaves entries not in the project as not imported'
    let unique = Library {
        tools: vec![entry("e1", "Unique Tool", ToolKind::EndMill, 6.35)],
        ..Library::default()
    };
    assert!(!annotate(&unique, std::slice::from_ref(&renamed))[0].in_project);
    // 'handles empty project tools'
    let two = Library {
        tools: vec![
            library_tool.clone(),
            entry("e2", "Other", ToolKind::EndMill, 6.35),
        ],
        ..Library::default()
    };
    let states = annotate(&two, &[]);
    assert_eq!(states.len(), 2);
    assert!(states.iter().all(|s| !s.in_project));
    // 'handles empty library entries'
    assert!(annotate(&Library::default(), &[tool]).is_empty());

    // The plaque: nothing of the sample library is in it, until added.
    let bytes = read(&fixture("fusion/sample.tools"));
    let lib = dc_app::library::import(&Library::default(), &bytes, false)
        .unwrap()
        .library;
    let mut project = plaque();
    assert!(annotate(&lib, &project.tools).iter().all(|s| !s.in_project));
    let drill = lib
        .tools
        .iter()
        .find(|t| t.tool.name == "5 mm Drill")
        .unwrap();
    let selected = BTreeSet::from([drill.id.clone()]);
    project
        .tools
        .extend(build_add(&lib, &project.tools, &selected));
    let marked: Vec<String> = annotate(&lib, &project.tools)
        .iter()
        .filter(|s| s.in_project)
        .map(|s| s.entry.tool.name.clone())
        .collect();
    assert_eq!(marked, ["5 mm Drill"]);
    project.tools.last_mut().unwrap().feed_mm_min = Some(200.0);
    assert!(annotate(&lib, &project.tools).iter().all(|s| !s.in_project));
}

/// Upstream `describe('countImportableSelected')` and
/// `describe('buildImportInput')`: ticks survive filtering, and the build
/// copies the ticked rows, hidden ones included, as project tools under
/// fresh ids in library order. Changed from upstream: a ticked
/// in-project row is counted and copied (module docs).
#[test]
fn ticked_rows_survive_filtering() {
    let lib = Library {
        tools: vec![
            entry("a", "Visible", ToolKind::EndMill, 6.0),
            entry("b", "Hidden", ToolKind::EndMill, 3.0),
        ],
        ..Library::default()
    };
    let states = annotate(&lib, &[]);
    let ids = |keys: &[&str]| -> BTreeSet<LibraryToolId> {
        keys.iter().map(|k| LibraryToolId::new(*k)).collect()
    };
    // 'counts selected new entries'
    assert_eq!(count_selected(&states, &ids(&["a"])), 1);
    // 'returns zero when nothing is selected'
    assert_eq!(count_selected(&states, &BTreeSet::new()), 0);
    // 'counts selected entries even when they are not in the filtered view'
    let shown = dc_app::library::filter_states(&states, &search("Visible"), Units::Mm);
    assert_eq!(shown.len(), 1);
    assert_eq!(count_selected(&states, &ids(&["b"])), 1);
    // A tick on a row the library no longer has is not counted.
    assert_eq!(count_selected(&states, &ids(&["gone"])), 0);

    // 'includes selected entries not in the filtered view'
    let built = build_add(&lib, &[], &ids(&["b"]));
    assert_eq!(built.len(), 1);
    assert_eq!(built[0].name, "Hidden");
    // 'excludes unselected entries'
    assert_eq!(build_add(&lib, &[], &ids(&["a"])).len(), 1);
    // 'returns empty array when nothing is selected'
    assert!(build_add(&lib, &[], &BTreeSet::new()).is_empty());
    // 'maps selected entries to tools': project ids, library order.
    let project_tools = [Tool {
        id: ToolId::new("t1"),
        ..lib.tools[0].tool.clone()
    }];
    let built = build_add(&lib, &project_tools, &ids(&["b", "a"]));
    let names: Vec<(&str, &str)> = built
        .iter()
        .map(|t| (t.id.as_str(), t.name.as_str()))
        .collect();
    assert_eq!(names, [("t2", "Visible"), ("t3", "Hidden")]);
    // Changed: an in-project row ticked is counted and copied again.
    let states = annotate(&lib, &project_tools);
    assert!(states[0].in_project);
    assert_eq!(count_selected(&states, &ids(&["a", "b"])), 2);
}

/// The tools `tests/fixtures/fusion/sample.json` imports as, in order
/// (`crates/dreamcarve/tests/cli.rs`, `FUSION_IMPORTED`).
const FUSION_IMPORTED: [&str; 13] = [
    "DreamCarve fixtures 6 mm Flat End Mill",
    "3 mm Ball End Mill",
    "8 mm Bull Nose End Mill (bull nose r1)",
    "20 mm Slot Mill",
    "12 mm 90-Degree Chamfer Mill",
    "5 mm Drill",
    "6 mm 90-Degree Spot Drill",
    "10 mm 90-Degree Countersink",
    "8 mm Counter Bore",
    "6 mm Reamer",
    "1/4 in Flat End Mill",
    "3.175 mm Flat End Mill (Plywood)",
    "3.175 mm Flat End Mill (Acrylic)",
];

/// The items of the sample Fusion library that are skipped, with their
/// types (`crates/dreamcarve/tests/cli.rs`, `FUSION_SKIPPED`).
const FUSION_SKIPPED: [(&str, &str); 10] = [
    ("6 mm Thread Mill, 1 mm Pitch", "thread mill"),
    ("M6 Right-Hand Tap", "tap right hand"),
    ("50 mm Face Mill", "face mill"),
    ("Ogee Form Router Bit", "form mill"),
    ("6 mm Lollipop Mill", "lollipop mill"),
    ("12 mm 14-Degree Dovetail Mill", "dovetail mill"),
    ("3 mm Tapered Ball Mill, 2-Degree Taper", "tapered mill"),
    ("General Turning Tool", "turning general"),
    ("Probe, 2 mm Stylus", "probe"),
    ("ER20 Collet Holder", "holder"),
];

/// Acceptance 71's import, acceptance 35's report: importing
/// `fixtures/fusion/sample.tools` into an empty library reports each tool
/// as `tools import` prints it (imported with its id, notes for unset
/// values, then the skipped items with the reason), then the counts; the
/// library is the one the CLI writes. Importing again is "already
/// present" for every tool, with or without *Replace existing*, and
/// changes nothing. The byte comparison with the CLI's output is
/// `crates/dreamcarve/tests/gui_equality.rs`.
#[test]
fn fusion_import_report_equals_tools_import() {
    let bytes = read(&fixture("fusion/sample.tools"));
    let merged = dc_app::library::import(&Library::default(), &bytes, false).expect("imports");
    let lines = merged.report.lines();
    assert_eq!(
        lines.len(),
        FUSION_IMPORTED.len() + FUSION_SKIPPED.len(),
        "one line per tool:\n{}",
        lines.join("\n")
    );
    for (k, name) in FUSION_IMPORTED.iter().enumerate() {
        let expected = format!("`{name}`: imported (lt{})", k + 1);
        assert!(
            lines[k].starts_with(&expected),
            "line {k} is `{}`, not `{expected}`",
            lines[k]
        );
    }
    for (k, (description, kind)) in FUSION_SKIPPED.iter().enumerate() {
        assert_eq!(
            lines[FUSION_IMPORTED.len() + k],
            format!("`{description}`: skipped: unsupported tool type `{kind}`")
        );
    }
    assert!(lines.contains(&"`5 mm Drill`: imported (lt6); feed unset".to_owned()));
    assert!(lines.contains(&"`DreamCarve fixtures 6 mm Flat End Mill`: imported (lt1)".to_owned()));
    assert_eq!(
        merged.report.summary("/config/tools.json"),
        "13 imported, 0 already present, 0 replaced, 10 skipped; library: /config/tools.json"
    );
    let lib = merged.library;
    assert_eq!(lib.tools.len(), 13);
    let drill = lib
        .tools
        .iter()
        .find(|t| t.tool.name == "5 mm Drill")
        .unwrap();
    assert_eq!(drill.tool.feed_mm_min, None, "never defaulted");

    for replace in [false, true] {
        let again = dc_app::library::import(&lib, &bytes, replace).unwrap();
        assert_eq!(again.library, lib, "replace {replace} changed the library");
        let text = again.report.lines().join("\n");
        assert_eq!(text.matches(": already present (lt").count(), 13, "{text}");
        assert_eq!(text.matches(": skipped: ").count(), 10, "{text}");
        assert_eq!(
            again.report.summary("tools.json"),
            "0 imported, 13 already present, 0 replaced, 10 skipped; library: tools.json"
        );
    }

    // A DreamCarve library file is read too, with no skipped count.
    let json = dc_library::save(&lib).unwrap();
    let from_json = dc_app::library::import(&Library::default(), json.as_bytes(), false).unwrap();
    assert_eq!(from_json.library, lib);
    assert_eq!(
        from_json.report.summary("tools.json"),
        "13 imported, 0 already present, 0 replaced; library: tools.json"
    );

    // A damaged archive is an error and nothing is imported.
    let broken = dc_app::library::import(&lib, b"PK\x03\x04not really a zip", false);
    assert!(broken.unwrap_err().to_string().contains("archive"));
}

// Slice 30 (#622; functional 64 and 64a; acceptance 101, first half):
// vendor groups, editing a library tool, *Revert changes*, *New tool* and
// *Duplicate* in the library.

/// The fixture library: `tools import` of `fixtures/fusion/sample.tools`
/// then of `fixtures/fusion/vendors.json`, as the CLI would write it.
fn fixture_library() -> Library {
    let mut lib = Library::default();
    for name in ["fusion/sample.tools", "fusion/vendors.json"] {
        lib = dc_app::library::import(&lib, &read(&fixture(name)), false)
            .unwrap()
            .library;
    }
    lib
}

/// The library tool named `name`.
fn named<'a>(lib: &'a Library, name: &str) -> &'a LibraryTool {
    lib.tools
        .iter()
        .find(|t| t.tool.name == name)
        .unwrap_or_else(|| panic!("the library has no `{name}`"))
}

/// Capability 64: one group per vendor, sorted by name, *No vendor* (no
/// vendor, or an empty one) last; each group's rows in library order; a
/// search hides the groups with no match, and reads the product id too.
#[test]
fn groups_by_vendor_with_no_vendor_last() {
    use dc_app::library::groups;
    use dc_app::tools::ToolField;

    let lib = fixture_library();
    // One of the sample's tools is an Amana bit, typed in the library.
    let id = named(&lib, "5 mm Drill").id.clone();
    let lib = dc_app::library::edit(&lib, &id, ToolField::Vendor, "Amana", Units::Mm).unwrap();
    let states = annotate(&lib, &[]);

    let all = groups(&states, &Filter::default(), Units::Mm);
    let vendors: Vec<Option<&str>> = all.iter().map(|g| g.vendor).collect();
    assert_eq!(
        vendors,
        [
            Some("Acme Router Bits"),
            Some("Amana"),
            Some("Birch Tooling"),
            Some("DreamCarve fixtures"),
            None
        ]
    );
    // Every tool once, in its vendor's group, in library order.
    let total: usize = all.iter().map(|g| g.rows.len()).sum();
    assert_eq!(total, lib.tools.len());
    let position = |state: &dc_app::library::EntryState<'_>| {
        lib.tools
            .iter()
            .position(|t| t.id == state.entry.id)
            .unwrap()
    };
    for group in &all {
        for row in &group.rows {
            let vendor = row
                .entry
                .tool
                .vendor
                .as_deref()
                .map(str::trim)
                .filter(|v| !v.is_empty());
            assert_eq!(vendor, group.vendor, "{}", row.entry.tool.name);
        }
        let order: Vec<usize> = group.rows.iter().map(|r| position(r)).collect();
        let mut sorted = order.clone();
        sorted.sort_unstable();
        assert_eq!(order, sorted, "library order within {:?}", group.vendor);
    }
    // vendors.json's tool with an empty vendor is under *No vendor*.
    let none = all.last().unwrap();
    assert!(
        none.rows.iter().any(|r| r.entry.tool.name == "3 mm Upcut"),
        "an empty vendor is no vendor"
    );

    // A search for `amana` leaves one group, with the one tool.
    let found = groups(&states, &search("amana"), Units::Mm);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].vendor, Some("Amana"));
    let names: Vec<&str> = found[0]
        .rows
        .iter()
        .map(|r| r.entry.tool.name.as_str())
        .collect();
    assert_eq!(names, ["5 mm Drill"]);
    // The product id is searched too (capability 64).
    let by_product = groups(&states, &search("bt-dr5"), Units::Mm);
    assert_eq!(by_product.len(), 1);
    assert_eq!(by_product[0].vendor, Some("Birch Tooling"));
    // Nothing found is no group at all.
    assert!(groups(&states, &search("nonexistent"), Units::Mm).is_empty());
}

/// Capability 64a: *Revert changes* puts the selected tool back as it was
/// when the sheet opened, in place, and leaves every other edit alone. A
/// tool made since the sheet opened has nothing to go back to.
#[test]
fn revert_restores_the_tool_as_opened() {
    use dc_app::library::{LibraryEdit, edit};
    use dc_app::tools::ToolField;

    let opened = fixture_library();
    let session = LibraryEdit::open(&opened);
    let drill = named(&opened, "5 mm Drill").id.clone();
    let ball = named(&opened, "3 mm Ball End Mill").id.clone();

    let lib = edit(&opened, &drill, ToolField::Feed, "250", Units::Mm).unwrap();
    let lib = edit(&lib, &drill, ToolField::Name, "Club drill", Units::Mm).unwrap();
    let lib = edit(&lib, &ball, ToolField::Rpm, "16000", Units::Mm).unwrap();
    assert!(session.changed(&lib, &drill));
    assert!(session.changed(&lib, &ball));
    let edited = dc_library::find(&lib, drill.as_str()).unwrap();
    assert_eq!(edited.tool.feed_mm_min, Some(250.0));
    assert_eq!(edited.tool.name, "Club drill");
    assert_eq!(
        edited.source,
        dc_library::find(&opened, drill.as_str()).unwrap().source,
        "an edit keeps the tool's source"
    );

    let reverted = session.revert(&lib, &drill).unwrap();
    assert_eq!(
        dc_library::find(&reverted, drill.as_str()).unwrap(),
        dc_library::find(&opened, drill.as_str()).unwrap(),
        "the drill as opened"
    );
    let at = |l: &Library, id: &LibraryToolId| l.tools.iter().position(|t| t.id == *id);
    assert_eq!(at(&reverted, &drill), at(&opened, &drill), "in its place");
    assert!(!session.changed(&reverted, &drill));
    assert_eq!(
        dc_library::find(&reverted, ball.as_str()).unwrap().tool.rpm,
        Some(16000),
        "the other edit stays"
    );

    // A tool made after the sheet opened has nothing to revert to.
    let (with_new, new_id) = dc_app::library::new_tool(&reverted);
    assert!(!session.changed(&with_new, &new_id));
    assert!(session.revert(&with_new, &new_id).is_err());
}

/// Capability 64a: *New tool* in the library is the Tools tab's `tool<n>`
/// placeholder (63), numbered within the library, under the next library
/// id; *Duplicate* copies a tool below itself as `<name> (2)` with a new
/// id and no `source`, every value kept.
#[test]
fn new_and_duplicate() {
    use dc_app::library::{duplicate, edit, new_tool};
    use dc_app::tools::ToolField;

    let lib = fixture_library();
    let count = lib.tools.len();
    let (next, id) = new_tool(&lib);
    assert_eq!(next.tools.len(), count + 1);
    assert_eq!(id.as_str(), format!("lt{}", count + 1));
    let made = dc_library::find(&next, id.as_str()).unwrap();
    assert_eq!(made.tool.name, format!("tool{}", count + 1));
    assert_eq!(made.source, None);
    assert_eq!(made.tool.cutter, ToolKind::EndMill);
    assert!((made.tool.diameter_mm - 6.35).abs() < 1e-12);
    assert_eq!(made.tool.flutes, 2);
    assert_eq!(
        (
            made.tool.feed_mm_min,
            made.tool.plunge_mm_min,
            made.tool.rpm,
            made.tool.step_down_mm,
            made.tool.step_over_fraction
        ),
        (None, None, None, None, None),
        "no rate is invented"
    );
    // A second is the next number.
    let (again, second) = new_tool(&next);
    assert_eq!(
        dc_library::find(&again, second.as_str()).unwrap().tool.name,
        format!("tool{}", count + 2)
    );
    // When its number's name is taken, the next free one.
    let first = lib.tools[0].id.clone();
    let taken = edit(
        &lib,
        &first,
        ToolField::Name,
        &format!("tool{}", count + 1),
        Units::Mm,
    )
    .unwrap();
    let (named_on, id) = new_tool(&taken);
    assert_eq!(
        dc_library::find(&named_on, id.as_str()).unwrap().tool.name,
        format!("tool{}", count + 2)
    );

    // Duplicate an imported tool: below itself, new id, no source.
    let original = named(&lib, "5 mm Drill").clone();
    assert!(original.source.is_some(), "an imported tool has a source");
    let (copied, copy_id) = duplicate(&lib, &original.id).unwrap();
    assert_ne!(copy_id, original.id);
    assert_eq!(copy_id.as_str(), format!("lt{}", count + 1));
    let copy = dc_library::find(&copied, copy_id.as_str()).unwrap();
    assert_eq!(copy.tool.name, "5 mm Drill (2)");
    assert_eq!(copy.source, None, "Replace never overwrites a copy");
    assert_eq!(
        Tool {
            id: original.tool.id.clone(),
            name: original.tool.name.clone(),
            ..copy.tool.clone()
        },
        original.tool,
        "every value kept"
    );
    let at = |l: &Library, id: &LibraryToolId| l.tools.iter().position(|t| t.id == *id);
    assert_eq!(
        at(&copied, &copy_id),
        at(&copied, &original.id).map(|i| i + 1),
        "below the original"
    );
    // Duplicating again is ` (3)`.
    let (twice, third) = duplicate(&copied, &original.id).unwrap();
    assert_eq!(
        dc_library::find(&twice, third.as_str()).unwrap().tool.name,
        "5 mm Drill (3)"
    );
    // The library still saves and loads as it is.
    let saved = dc_library::save(&twice).unwrap();
    assert_eq!(dc_library::load(saved.as_bytes()).unwrap(), twice);
    // An unknown id is an error, not a panic.
    assert!(duplicate(&lib, &LibraryToolId::new("lt999")).is_err());
}

/// Capability 64a: a library edit is checked as the file is
/// (`dc_library::validate_tool`), and a name stays unique, so `tools add
/// <name>` is never ambiguous; a refused edit leaves the library as it
/// was.
#[test]
fn edits_are_validated_as_the_file_is() {
    use dc_app::library::edit;
    use dc_app::tools::ToolField;

    let lib = fixture_library();
    let drill = named(&lib, "5 mm Drill").id.clone();
    assert!(edit(&lib, &drill, ToolField::Diameter, "0", Units::Mm).is_err());
    assert!(edit(&lib, &drill, ToolField::Feed, "-5", Units::Mm).is_err());
    assert!(edit(&lib, &drill, ToolField::Feed, "fast", Units::Mm).is_err());
    let taken = edit(&lib, &drill, ToolField::Name, "6 mm Reamer", Units::Mm)
        .unwrap_err()
        .to_string();
    assert!(taken.contains("6 mm Reamer"), "{taken}");
    // A typed inch value is stored in millimetres.
    let inch = edit(&lib, &drill, ToolField::Feed, "10", Units::Inch).unwrap();
    let feed = dc_library::find(&inch, drill.as_str())
        .unwrap()
        .tool
        .feed_mm_min
        .unwrap();
    assert!((feed - 254.0).abs() < 1e-9, "{feed}");
    // Clearing a rate is allowed, and never filled in.
    let cleared = edit(&inch, &drill, ToolField::Feed, "", Units::Mm).unwrap();
    assert_eq!(
        dc_library::find(&cleared, drill.as_str())
            .unwrap()
            .tool
            .feed_mm_min,
        None
    );
}
