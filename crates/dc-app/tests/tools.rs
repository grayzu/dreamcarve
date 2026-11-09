// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 8 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan → Tools
//! and library"; functional spec, capability 63 and acceptance 71): the
//! project's tools. A tool added from the library is the copy `tools add`
//! writes; a tool with an unset feed, plunge rate or RPM is marked, its
//! refusal is the CLI's, and export is blocked until it is typed back;
//! *Remove* is refused while an operation uses the tool, naming the
//! operations; *Save to library* copies a project tool into the library.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::collections::BTreeSet;

use dc_app::test_support::{DATE, Harness};
use dc_app::tools::{self, KindChoice, KindSwitch, ToolField};
use dc_app::{Command, CommandError};
use dc_job::{ProgramSplit, Tier};
use dc_library::{AddOutcome, Library, LibraryToolId};
use dc_project::{Project, Tool, ToolId, ToolKind, Units};

use common::{fixture, plaque, plaque_bytes, read};

fn tool<'a>(project: &'a Project, id: &str) -> &'a Tool {
    project
        .tools
        .iter()
        .find(|t| t.id.as_str() == id)
        .unwrap_or_else(|| panic!("the project has tool {id}"))
}

/// The library `tools import tests/fixtures/fusion/sample.tools` makes
/// from nothing, read in place.
fn sample_library() -> Library {
    let bytes = read(&fixture("fusion/sample.tools"));
    dc_app::library::import(&Library::default(), &bytes, false)
        .expect("the sample imports")
        .library
}

/// The library tool named `name`.
fn entry_named<'a>(lib: &'a Library, name: &str) -> &'a dc_library::LibraryTool {
    lib.tools
        .iter()
        .find(|t| t.tool.name == name)
        .unwrap_or_else(|| panic!("the library has no `{name}`"))
}

/// Acceptance 71, first sentence (with acceptance 35's `tools add`
/// rules, `crates/dreamcarve/tests/cli.rs`,
/// `tools_add_copies_into_project_with_new_id`): adding the 6.35 mm end
/// mill from the library writes the copy `tools add` writes, under the
/// next free project id `t<n>`, every field kept, its number included,
/// and no library id in the file. Adding it again is a second copy, as a
/// second `tools add` is. A library change afterwards leaves the project
/// alone (ADR-0002). The byte comparison with the CLI itself is
/// `crates/dreamcarve/tests/gui_equality.rs`.
#[test]
fn add_from_library_equals_tools_add() {
    let lib = sample_library();
    let end_mill = entry_named(&lib, "1/4 in Flat End Mill");
    dc_geometry::assert_approx_eq!(end_mill.tool.diameter_mm, 6.35, 1e-9);

    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let before = harness.app.project().clone();
    let selected = BTreeSet::from([end_mill.id.clone()]);
    let command = tools::add_from_library(harness.app.project(), &lib, &selected)
        .expect("one ticked row is something to add");
    let label = harness.run(command).expect("the tool is added");
    assert_eq!(label, "Add tool “1/4 in Flat End Mill”");

    let project = harness.app.project();
    assert_eq!(project.tools.len(), before.tools.len() + 1);
    let added = project.tools.last().expect("a tool was added");
    assert_eq!(added.id, ToolId::new("t4"), "one past the plaque's t3");
    let expected = Tool {
        id: ToolId::new("t4"),
        ..end_mill.tool.clone()
    };
    assert_eq!(*added, expected, "every field copied, the number kept");
    assert_eq!(
        dc_library::project_copy(&before.tools, &end_mill.tool),
        expected,
        "the copy `tools add` makes"
    );
    assert_eq!(project.tools[..3], before.tools[..], "the others unchanged");
    assert_eq!(project.operations, before.operations);
    let file = harness.save().expect("the project saves");
    assert!(
        !file.contains(&format!("\"{}\"", end_mill.id)),
        "the library id is not in the file"
    );

    // Again: a second copy under the next id, as `tools add` twice gives.
    let command = tools::add_from_library(harness.app.project(), &lib, &selected).unwrap();
    harness.run(command).unwrap();
    let project = harness.app.project();
    assert_eq!(project.tools.last().unwrap().id, ToolId::new("t5"));

    // Several ticked rows: one undo step, in library order.
    let drill = entry_named(&lib, "5 mm Drill");
    let both = BTreeSet::from([end_mill.id.clone(), drill.id.clone()]);
    let command = tools::add_from_library(harness.app.project(), &lib, &both).unwrap();
    assert_eq!(harness.run(command).unwrap(), "Add 2 tools");
    let names: Vec<&str> = harness.app.project().tools[5..]
        .iter()
        .map(|t| t.name.as_str())
        .collect();
    assert_eq!(names, ["5 mm Drill", "1/4 in Flat End Mill"]);
    assert_eq!(
        harness.app.project().tools[5].feed_mm_min,
        None,
        "never filled"
    );

    // Nothing ticked is nothing to do.
    assert!(tools::add_from_library(harness.app.project(), &lib, &BTreeSet::new()).is_none());

    // A library change never alters the open project.
    let snapshot = harness.app.project().clone();
    let mut changed = lib.clone();
    dc_library::remove(&mut changed, end_mill.id.as_str()).unwrap();
    assert_eq!(harness.app.project(), &snapshot);
}

/// Acceptance 71, second sentence, and capability 63's "feed, plunge and
/// RPM fields are empty, never prefilled": clearing the 1/4 end mill's
/// plunge rate marks the tool, gives the field the file's message, lists
/// the CLI's refusal among the errors and stops export; typing it back
/// clears all three. The message is `dc_project::validate`'s
/// `ToolValueUnset`, the line `check` prints.
#[test]
fn missing_rate_is_an_error_and_blocks_export() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    assert!(!harness.app.validation().has_errors(), "the plaque exports");
    let t2 = tool(harness.app.project(), "t2").clone();
    assert!(!tools::lacks_rate(&t2));
    assert_eq!(tools::field_text(&t2, ToolField::Plunge, Units::Mm), "500");
    assert_eq!(
        tools::field_unit(ToolField::Plunge, Units::Mm),
        "mm/min",
        "the unit follows the value"
    );

    let cleared = tools::set_field(&t2, ToolField::Plunge, "", Units::Mm).expect("clears");
    assert_eq!(cleared.plunge_mm_min, None);
    harness.run(Command::UpdateTool(cleared)).unwrap();

    let message = "tools[t2].plunge_mm_min is not set (mm/min) for tool `1/4 end mill`";
    let t2 = tool(harness.app.project(), "t2").clone();
    assert!(tools::lacks_rate(&t2), "the row's red mark");
    assert_eq!(
        tools::unset_rates(&t2),
        [("plunge_mm_min", message.to_owned())],
        "the field's message is the file's"
    );
    assert_eq!(tools::field_text(&t2, ToolField::Plunge, Units::Mm), "");
    let refusal = harness
        .warnings()
        .iter()
        .find(|f| f.field.as_deref() == Some("tools[t2].plunge_mm_min"))
        .expect("the refusal is listed");
    assert_eq!(refusal.tier, Tier::Error);
    assert_eq!(refusal.message, message);
    assert!(harness.app.validation().has_errors(), "export is disabled");
    let built = dc_job::build(harness.app.project(), ProgramSplit::ProjectDefault, DATE);
    assert!(built.programs.is_empty(), "no program is written");
    assert!(
        built.findings.errors.iter().any(|f| f.message == message),
        "{:?}",
        built.findings.errors
    );
    harness.generate();
    assert!(harness.programs().is_empty(), "nothing generated to export");

    // Typed back: the mark, the message and the refusal all go.
    let typed = tools::set_field(&t2, ToolField::Plunge, "500", Units::Mm).unwrap();
    harness.run(Command::UpdateTool(typed)).unwrap();
    let t2 = tool(harness.app.project(), "t2");
    assert_eq!(t2.plunge_mm_min, Some(500.0));
    assert!(!tools::lacks_rate(t2));
    assert!(tools::unset_rates(t2).is_empty());
    assert!(
        !harness
            .warnings()
            .iter()
            .any(|f| f.field.as_deref() == Some("tools[t2].plunge_mm_min"))
    );
    assert!(!harness.app.validation().has_errors());
    assert_eq!(harness.app.project(), &plaque(), "back to the file");

    // A rate in another unit means what it means in a file.
    let inch = tools::set_field(t2, ToolField::Feed, "60 in/min", Units::Mm).unwrap();
    dc_geometry::assert_approx_eq!(inch.feed_mm_min.unwrap(), 1524.0, 1e-9);
    assert!(matches!(
        tools::set_field(t2, ToolField::Feed, "fast", Units::Mm),
        Err(CommandError::NotAQuantity { .. })
    ));

    // The unused placeholder tool lacks all three, and is marked too.
    let t1 = tool(harness.app.project(), "t1");
    assert!(tools::lacks_rate(t1));
    let fields: Vec<&str> = tools::unset_rates(t1).iter().map(|(f, _)| *f).collect();
    assert_eq!(fields, ["feed_mm_min", "plunge_mm_min", "rpm"]);
    for field in [ToolField::Feed, ToolField::Plunge, ToolField::Rpm] {
        assert_eq!(
            tools::field_text(t1, field, Units::Mm),
            "",
            "{field:?} is never prefilled"
        );
    }
}

/// Capability 63 and acceptance 71: *Remove* on a tool in use is refused,
/// naming every operation that uses it, in project order, and changes
/// nothing; the panel disables the button with the same names. A tool no
/// operation uses is removed as one undo step, and the plaque's unused
/// placeholder carries the `unused_tool` hint `check` prints.
#[test]
fn remove_in_use_is_refused_naming_operations() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let before = harness.app.project().clone();
    let t2 = ToolId::new("t2");
    assert_eq!(
        tools::users(&before, &t2),
        ["Panel", "Square recess", "Cut out"]
    );

    let refused = harness.run(Command::RemoveTool(t2.clone())).unwrap_err();
    assert!(
        matches!(refused, CommandError::ToolInUse { .. }),
        "{refused:?}"
    );
    assert_eq!(
        refused.to_string(),
        "tool “1/4 end mill” is used by “Panel”, “Square recess” and “Cut out”"
    );
    assert_eq!(harness.app.project(), &before, "nothing removed");
    assert!(!harness.app.can_undo(), "no undo step");

    let t1 = ToolId::new("t1");
    assert!(tools::users(&before, &t1).is_empty());
    let hint = tools::unused_hint(harness.warnings(), &t1).expect("t1 is unused");
    assert_eq!(hint.tier, Tier::Hint);
    assert_eq!(hint.code.as_deref(), Some("unused_tool"));
    assert_eq!(
        hint.message,
        "tool `t1` (`Set me`) is defined but not used by any enabled operation"
    );
    assert!(tools::unused_hint(harness.warnings(), &t2).is_none());

    let label = harness.run(Command::RemoveTool(t1)).unwrap();
    assert_eq!(label, "Remove tool “Set me”");
    let ids: Vec<&str> = harness
        .app
        .project()
        .tools
        .iter()
        .map(|t| t.id.as_str())
        .collect();
    assert_eq!(ids, ["t2", "t3"]);
    harness.app.undo();
    assert_eq!(harness.app.project(), &before);

    // Duplicate: a copy under the next id, named as copies are named.
    let command = tools::duplicate(harness.app.project(), &t2).unwrap();
    harness.run(command).unwrap();
    let copy = harness.app.project().tools.last().unwrap();
    assert_eq!(copy.id, ToolId::new("t4"));
    assert_eq!(copy.name, "1/4 end mill (2)");
    assert_eq!(
        Tool {
            id: t2.clone(),
            name: "1/4 end mill".into(),
            ..copy.clone()
        },
        *tool(&before, "t2")
    );
}

/// Capability 63, *Save to library*, and capability 64's path "add to
/// project, edit there, save to library": a project tool goes into the
/// library under the next `lt<n>` id with every value, as `dc_library::add`
/// stores it; saving it again is *already present* and changes nothing;
/// the project is untouched, and the library row is then marked as in
/// the project.
#[test]
fn save_to_library() {
    let harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let project = harness.app.project().clone();
    let t2 = tool(&project, "t2");

    let (lib, outcome) =
        tools::save_to_library(&Library::default(), t2).expect("a valid tool saves");
    assert_eq!(
        outcome,
        AddOutcome::Added {
            id: LibraryToolId::new("lt1")
        }
    );
    assert_eq!(lib.tools.len(), 1);
    let entry = &lib.tools[0];
    assert_eq!(entry.source, None, "made by hand, not imported");
    assert_eq!(
        entry.tool,
        Tool {
            id: ToolId::new("lt1"),
            ..t2.clone()
        },
        "every value kept"
    );
    assert_eq!(harness.app.project(), &project, "the project is untouched");

    let (again, outcome) = tools::save_to_library(&lib, t2).unwrap();
    assert_eq!(
        outcome,
        AddOutcome::AlreadyPresent {
            id: LibraryToolId::new("lt1")
        }
    );
    assert_eq!(again, lib, "nothing changed");

    let marked = dc_app::library::annotate(&lib, &project.tools);
    assert!(
        marked[0].in_project,
        "the saved tool is marked in the project"
    );

    // An edited copy with the same name is stored beside it, renamed so
    // `tools add <name>` stays unambiguous.
    let mut faster = t2.clone();
    faster.feed_mm_min = Some(2000.0);
    let (lib, outcome) = tools::save_to_library(&lib, &faster).unwrap();
    assert_eq!(
        outcome,
        AddOutcome::Added {
            id: LibraryToolId::new("lt2")
        }
    );
    assert_eq!(lib.tools[1].tool.name, "1/4 end mill (2)");

    // A tool the library refuses is an error naming the field.
    let mut broken = t2.clone();
    broken.diameter_mm = -1.0;
    let error = tools::save_to_library(&lib, &broken).unwrap_err();
    assert!(error.to_string().contains("diameter_mm"), "{error}");
}

/// Capability 63: every field carries its unit, follows the project's
/// display units, and parses what the file accepts.
#[test]
fn editor_fields_carry_units() {
    let project = plaque();
    let t2 = tool(&project, "t2");
    for (field, mm_text, mm_unit, inch_text, inch_unit) in [
        (ToolField::Diameter, "6.35", "mm", "0.25", "in"),
        (ToolField::Feed, "1500", "mm/min", "59.0551", "in/min"),
        (ToolField::Rpm, "18000", "RPM", "18000", "RPM"),
        (ToolField::StepDown, "4", "mm", "0.1575", "in"),
        (ToolField::StepOver, "40", "%", "40", "%"),
        (ToolField::Flutes, "2", "", "2", ""),
        (ToolField::Number, "1", "", "1", ""),
    ] {
        assert_eq!(
            tools::field_text(t2, field, Units::Mm),
            mm_text,
            "{field:?}"
        );
        assert_eq!(tools::field_unit(field, Units::Mm), mm_unit, "{field:?}");
        assert_eq!(
            tools::field_text(t2, field, Units::Inch),
            inch_text,
            "{field:?}"
        );
        assert_eq!(
            tools::field_unit(field, Units::Inch),
            inch_unit,
            "{field:?}"
        );
    }
    let quarter = tools::set_field(t2, ToolField::Diameter, "1/4 in", Units::Mm).unwrap();
    dc_geometry::assert_approx_eq!(quarter.diameter_mm, 6.35, 1e-9);
    let bare = tools::set_field(t2, ToolField::Diameter, "0.5", Units::Inch).unwrap();
    dc_geometry::assert_approx_eq!(bare.diameter_mm, 12.7, 1e-9);
    let over = tools::set_field(t2, ToolField::StepOver, "25 %", Units::Mm).unwrap();
    dc_geometry::assert_approx_eq!(over.step_over_fraction.unwrap(), 0.25, 1e-12);
    let unset = tools::set_field(t2, ToolField::StepDown, "", Units::Mm).unwrap();
    assert_eq!(unset.step_down_mm, None, "an optional value clears to null");
    assert!(tools::set_field(t2, ToolField::Diameter, "", Units::Mm).is_err());
    let vendor = tools::set_field(t2, ToolField::Vendor, "Amana", Units::Mm).unwrap();
    assert_eq!(vendor.vendor.as_deref(), Some("Amana"));
    let angle = Tool {
        cutter: ToolKind::VBit {
            included_angle_deg: 90.0,
        },
        ..t2.clone()
    };
    let sixty = tools::set_field(&angle, ToolField::Angle, "60", Units::Mm).unwrap();
    assert_eq!(
        sixty.cutter,
        ToolKind::VBit {
            included_angle_deg: 60.0
        }
    );
}

/// #581, decision 1 (#537 deviation 4): choosing *V-bit* for an end mill
/// changes nothing in the project. The switch is held until an included
/// angle in (0°, 180°) is typed; the switch with that angle is one
/// command and one undo step; an empty field discards it. Switching away
/// from a V-bit, or to the type a tool already has, is not held.
#[test]
fn switching_to_v_bit_holds_until_an_angle_is_typed() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let before = harness.app.project().clone();
    let undo_before = harness.app.undo_label().map(str::to_owned);
    let t2 = tool(&before, "t2").clone();
    assert_eq!(t2.cutter, ToolKind::EndMill);

    // The switch itself writes nothing.
    assert_eq!(
        tools::set_kind(&t2, KindChoice::VBit),
        KindSwitch::HoldForAngle
    );
    assert_eq!(harness.app.project(), &before, "no project change");
    assert_eq!(
        harness.app.undo_label().map(str::to_owned),
        undo_before,
        "no undo step"
    );

    // Leaving the field empty discards the pending switch.
    assert_eq!(tools::finish_v_bit(&t2, "").unwrap(), None);
    assert_eq!(tools::finish_v_bit(&t2, "   ").unwrap(), None);

    // An angle outside (0°, 180°), or not a number, is refused.
    for bad in ["0", "180", "-30", "200°", "sixty", "6,5"] {
        assert!(
            tools::finish_v_bit(&t2, bad).is_err(),
            "`{bad}` is not an included angle"
        );
    }

    // A typed angle makes the switch: one command, one undo step.
    let command = tools::finish_v_bit(&t2, "60°")
        .expect("60° is an angle")
        .expect("and something to do");
    harness.run(command).expect("the switch runs");
    assert_eq!(
        tool(harness.app.project(), "t2").cutter,
        ToolKind::VBit {
            included_angle_deg: 60.0
        }
    );
    harness.app.undo().expect("one step to undo");
    assert_eq!(harness.app.project(), &before, "one undo step restores it");

    // A V-bit keeps its angle; switching away is immediate.
    let v_bit = Tool {
        cutter: ToolKind::VBit {
            included_angle_deg: 30.0,
        },
        ..t2.clone()
    };
    assert_eq!(
        tools::set_kind(&v_bit, KindChoice::VBit),
        KindSwitch::Unchanged
    );
    assert_eq!(
        tools::set_kind(&v_bit, KindChoice::EndMill),
        KindSwitch::Update(Box::new(t2.clone()))
    );
    assert_eq!(
        tools::set_kind(&t2, KindChoice::EndMill),
        KindSwitch::Unchanged
    );
    assert_eq!(
        tools::set_kind(&t2, KindChoice::Drill),
        KindSwitch::Update(Box::new(Tool {
            cutter: ToolKind::Drill,
            ..t2.clone()
        }))
    );
}

/// #581, decision 1: no code path writes an included angle the user did
/// not type. The model: a type switch never produces a V-bit with an
/// angle it made up. The source: outside test code, `dc-app` and the app
/// never build a V-bit from a number literal or a named default.
#[test]
fn no_default_v_bit_angle_is_written() {
    let t2 = tool(&plaque(), "t2").clone();
    for start in [
        ToolKind::EndMill,
        ToolKind::BallNose,
        ToolKind::Engraver,
        ToolKind::Drill,
    ] {
        let from = Tool {
            cutter: start.clone(),
            ..t2.clone()
        };
        for choice in KindChoice::ALL {
            if let KindSwitch::Update(next) = tools::set_kind(&from, choice) {
                assert!(
                    !matches!(next.cutter, ToolKind::VBit { .. }),
                    "{start:?} → {choice:?} wrote a V-bit angle nobody typed"
                );
            }
        }
    }

    let root = common::repo_root();
    let mut offenders = Vec::new();
    for dir in ["crates/dc-app/src", "crates/dreamcarve-app/src"] {
        for path in rust_files(&root.join(dir)) {
            let text = String::from_utf8(read(&path)).expect("UTF-8 source");
            // Unit tests sit at the end of a file, after `#[cfg(test)]`.
            let code = text.split("#[cfg(test)]").next().unwrap_or_default();
            for (n, line) in code.lines().enumerate() {
                let Some((_, value)) = line.split_once("included_angle_deg:") else {
                    continue;
                };
                let value = value.trim_start();
                let literal = value.starts_with(|c: char| c.is_ascii_digit());
                let named = value.starts_with(|c: char| c.is_ascii_uppercase());
                if literal || named {
                    offenders.push(format!("{}:{}: {}", path.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "a V-bit angle nobody typed: {offenders:#?}"
    );
}

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display())) {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}
