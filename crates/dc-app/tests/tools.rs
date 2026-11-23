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
    assert!(!tools::lacks_rate(&t2) && !tools::lacks_value(&t2));
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
    assert!(
        tools::lacks_value(&t2),
        "which the Tools step draws from `lacks_value`"
    );
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
    assert!(!tools::lacks_rate(t2) && !tools::lacks_value(t2));
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

    let refused = harness.run(Command::DeleteTool(t2.clone())).unwrap_err();
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

    let label = harness.run(Command::DeleteTool(t1)).unwrap();
    assert_eq!(label, "Delete tool “Set me”");
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

/// The names of `project`'s tools, in order.
fn names(project: &Project) -> Vec<&str> {
    project.tools.iter().map(|t| t.name.as_str()).collect()
}

/// Slice 29 (#621; functional 63, acceptance 100): *New tool* on an empty
/// project makes `tool1`, an end mill with `init`'s placeholder geometry
/// (6.35 mm, 2 flutes), as one undo step `New tool “tool1”`; the second
/// is `tool2`. The number is the tool's place in the list, or the next
/// free one when that name is taken: on a project whose third tool is
/// already `tool4`, the fourth is `tool5`.
#[test]
fn new_tool_is_a_placeholder_named_by_number() {
    let mut harness = Harness::from_project(dc_app::App::new_project().unwrap());
    assert!(harness.app.project().tools.is_empty());
    let label = harness
        .run(tools::new_tool(harness.app.project()))
        .expect("a new tool is added");
    assert_eq!(label, "New tool “tool1”");
    let first = harness.app.project().tools[0].clone();
    assert_eq!(first.id, ToolId::new("t1"));
    assert_eq!(first.name, "tool1");
    assert_eq!(first.cutter, ToolKind::EndMill);
    dc_geometry::assert_approx_eq!(first.diameter_mm, 6.35, 1e-9);
    assert_eq!(first.flutes, 2);
    assert_eq!(first.number, None, "its number is its place in the list");
    assert_eq!(first.vendor, None);
    assert_eq!(first.product_id, None);

    harness.run(tools::new_tool(harness.app.project())).unwrap();
    assert_eq!(names(harness.app.project()), ["tool1", "tool2"]);
    assert_eq!(harness.app.project().tools[1].id, ToolId::new("t2"));
    harness.app.undo();
    assert_eq!(
        names(harness.app.project()),
        ["tool1"],
        "one undo step each"
    );

    // The plaque's third tool renamed `tool4`: the fourth skips to `tool5`.
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let third = Tool {
        name: "tool4".into(),
        ..tool(harness.app.project(), "t3").clone()
    };
    harness.run(Command::UpdateTool(third)).unwrap();
    let label = harness.run(tools::new_tool(harness.app.project())).unwrap();
    assert_eq!(label, "New tool “tool5”");
    assert_eq!(
        names(harness.app.project()),
        ["Set me", "1/4 end mill", "tool4", "tool5"]
    );
    assert_eq!(harness.app.project().tools[3].id, ToolId::new("t4"));
}

/// Slice 29 (#621; principle 5, S20): a new tool has no feed, plunge rate,
/// RPM, step-down or stepover, on an empty project and on one whose other
/// tools have them, and its editor shows those fields empty.
#[test]
fn new_tool_never_fills_a_rate() {
    for project in [dc_app::App::new_project().unwrap(), plaque()] {
        let mut harness = Harness::from_project(project);
        harness.run(tools::new_tool(harness.app.project())).unwrap();
        let new = harness.app.project().tools.last().unwrap().clone();
        assert_eq!(new.feed_mm_min, None);
        assert_eq!(new.plunge_mm_min, None);
        assert_eq!(new.rpm, None);
        assert_eq!(new.step_down_mm, None);
        assert_eq!(new.step_over_fraction, None);
        assert!(tools::lacks_rate(&new), "the row's red mark");
        for field in [
            ToolField::Feed,
            ToolField::Plunge,
            ToolField::Rpm,
            ToolField::StepDown,
            ToolField::StepOver,
        ] {
            assert_eq!(tools::field_text(&new, field, Units::Mm), "", "{field:?}");
        }
    }
}

/// Slice 29 (#621; acceptance 100): a new tool given to an operation
/// blocks export with the CLI's refusal for the saved file, naming each
/// unset rate; typing the rates in (and the step-down the Panel takes
/// from its tool) lifts it.
#[test]
fn export_refuses_a_new_tool_until_its_rates_are_set() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    harness.run(tools::new_tool(harness.app.project())).unwrap();
    let new = harness.app.project().tools.last().unwrap().clone();
    assert_eq!(new.id, ToolId::new("t4"));
    let panel = harness
        .app
        .project()
        .operations
        .iter()
        .find(|op| op.name() == "Panel")
        .unwrap()
        .clone();
    let row = dc_app::editor::fields::rows(&panel)
        .into_iter()
        .find(|r| r.key == "tool")
        .expect("Panel has a tool row");
    let command = dc_app::editor::set_tool(&panel, row, &new.id)
        .unwrap()
        .expect("the tool changes");
    harness.run(command).unwrap();
    harness.generate();

    let options = dc_app::export::Options::new(DATE, dc_job::EstimateConfig::default());
    let dc_app::export::Plan::Ready(ready) = dc_app::export::plan(&harness.app, &options) else {
        panic!("the plan waits after generation finished");
    };
    let saved = dc_project::load(harness.save().unwrap().as_bytes())
        .expect("the saved file loads")
        .project;
    let cli = dc_job::build_board(&saved, ProgramSplit::ProjectDefault, DATE, None);
    assert_eq!(ready.findings, cli.findings, "the CLI's refusal");
    assert!(ready.programs.is_empty(), "export is refused");
    for field in ["feed_mm_min", "plunge_mm_min", "rpm"] {
        let located = format!("tools[t4].{field}");
        assert!(
            cli.findings
                .errors
                .iter()
                .any(|f| f.field.as_deref() == Some(located.as_str())),
            "{located} is named: {:?}",
            cli.findings.errors
        );
    }

    let mut set = new;
    for (field, text) in [
        (ToolField::Feed, "1000"),
        (ToolField::Plunge, "300"),
        (ToolField::Rpm, "18000"),
        // The Panel takes its step-down from the tool, so it needs one too.
        (ToolField::StepDown, "3"),
    ] {
        set = tools::set_field(&set, field, text, Units::Mm).unwrap();
    }
    harness.run(Command::UpdateTool(set)).unwrap();
    harness.generate();
    let dc_app::export::Plan::Ready(ready) = dc_app::export::plan(&harness.app, &options) else {
        panic!("the plan waits after generation finished");
    };
    assert!(
        !ready.findings.errors.iter().any(|f| f
            .field
            .as_deref()
            .is_some_and(|f| f.starts_with("tools[t4]"))),
        "{:?}",
        ready.findings.errors
    );
}

// #685 (phase 3.6's model in the app; functional 63, 64a and 66b): the
// tapered ball nose in the tool editor.

/// The tapered ball nose test part: `t2` and `t3` are tapered ball noses
/// with a 6.35 mm cutting diameter (tips 2.286 mm and 0.762 mm, 6.7° per
/// side, 19.05 mm of flute), `t3` the enabled Inlay's bit; `t1` is
/// `init`'s placeholder, which nothing uses.
fn tapered_bytes() -> Vec<u8> {
    read(&fixture("dc-tbn.dcv"))
}

/// A tapered ball nose's three values: tip, taper, flute length.
fn tapered(tool: &Tool) -> (Option<f64>, Option<f64>, Option<f64>) {
    match tool.cutter {
        ToolKind::TaperedBallNose {
            tip_diameter_mm,
            taper_angle_deg,
            flute_length_mm,
        } => (tip_diameter_mm, taper_angle_deg, flute_length_mm),
        ref other => panic!("not a tapered ball nose: {other:?}"),
    }
}

/// #685; capability 63: the type list offers the tapered ball nose, and a
/// tool of that type has its own rows, tip diameter, taper (labelled per
/// side) and flute length, after the type; Diameter keeps its label and
/// is the cutting diameter. Choosing the type writes the cutter with
/// nothing set (no value is invented, S20), and each typed value is one
/// `UpdateTool`, so one undo step. A taper is degrees per side in (0°,
/// 45°]; a blank field clears its value to `null`.
#[test]
fn tapered_ball_nose_editor_fields() {
    let strings = dc_app::Strings::english();
    assert_eq!(
        KindChoice::ALL.last(),
        Some(&KindChoice::TaperedBallNose),
        "the dropdown offers it, after the kinds it had"
    );
    assert_eq!(
        strings.get(KindChoice::TaperedBallNose.label()),
        "Tapered ball nose"
    );

    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let before = harness.app.project().clone();
    let t2 = tool(&before, "t2").clone();
    let common = [
        ToolField::Diameter,
        ToolField::Flutes,
        ToolField::Feed,
        ToolField::Plunge,
        ToolField::Rpm,
        ToolField::StepDown,
        ToolField::StepOver,
        ToolField::Number,
        ToolField::Vendor,
        ToolField::ProductId,
    ];
    let rows_of = |own: &[ToolField]| -> Vec<ToolField> {
        [ToolField::Name]
            .into_iter()
            .chain(own.iter().copied())
            .chain(common)
            .collect()
    };
    assert_eq!(tools::fields(&t2), rows_of(&[]), "an end mill's rows");
    assert_eq!(
        tools::fields_for(KindChoice::VBit),
        rows_of(&[ToolField::Angle])
    );

    // Choosing the type: the cutter with nothing set, one undo step.
    let KindSwitch::Update(switched) = tools::set_kind(&t2, KindChoice::TaperedBallNose) else {
        panic!("choosing the type changes an end mill");
    };
    assert_eq!(tapered(&switched), (None, None, None), "nothing invented");
    assert_eq!(
        Tool {
            cutter: ToolKind::EndMill,
            ..(*switched).clone()
        },
        t2,
        "every other value kept"
    );
    harness.run(Command::UpdateTool(*switched)).unwrap();
    let unset = tool(harness.app.project(), "t2").clone();
    let own = [
        ToolField::TipDiameter,
        ToolField::Taper,
        ToolField::FluteLength,
    ];
    assert_eq!(tools::fields(&unset), rows_of(&own));
    assert_eq!(
        tools::fields_for(KindChoice::TaperedBallNose),
        rows_of(&own)
    );
    let labels: Vec<&str> = own.iter().map(|f| strings.get(f.label())).collect();
    assert_eq!(labels, ["Tip diameter", "Taper per side", "Flute length"]);
    assert_eq!(strings.get(ToolField::Diameter.label()), "Diameter");
    assert_eq!(
        own.map(ToolField::file_name),
        [
            "cutter.tapered_ball_nose.tip_diameter_mm",
            "cutter.tapered_ball_nose.taper_angle_deg",
            "cutter.tapered_ball_nose.flute_length_mm",
        ],
        "the file's names, as validation locates them"
    );
    for field in own {
        assert_eq!(tools::field_text(&unset, field, Units::Mm), "", "{field:?}");
    }

    // Each value typed is one command, so one undo step.
    let mut steps = vec![harness.app.project().clone()];
    for (field, text) in [
        (ToolField::TipDiameter, "0.03 in"),
        (ToolField::Taper, "6.7°"),
        (ToolField::FluteLength, "19.05"),
    ] {
        let current = tool(harness.app.project(), "t2").clone();
        let next = tools::set_field(&current, field, text, Units::Mm).expect("a value");
        harness.run(Command::UpdateTool(next)).unwrap();
        steps.push(harness.app.project().clone());
    }
    let set = tool(harness.app.project(), "t2").clone();
    let (tip, taper, flutes) = tapered(&set);
    dc_geometry::assert_approx_eq!(tip.unwrap(), 0.762, 1e-9);
    dc_geometry::assert_approx_eq!(taper.unwrap(), 6.7, 1e-12);
    dc_geometry::assert_approx_eq!(flutes.unwrap(), 19.05, 1e-12);
    dc_geometry::assert_approx_eq!(set.diameter_mm, 6.35, 1e-12);
    for (field, mm_text, mm_unit, inch_text, inch_unit) in [
        (ToolField::Diameter, "6.35", "mm", "0.25", "in"),
        (ToolField::TipDiameter, "0.762", "mm", "0.03", "in"),
        (ToolField::Taper, "6.7", "°", "6.7", "°"),
        (ToolField::FluteLength, "19.05", "mm", "0.75", "in"),
    ] {
        assert_eq!(
            (
                tools::field_text(&set, field, Units::Mm),
                tools::field_unit(field, Units::Mm)
            ),
            (mm_text.to_owned(), mm_unit),
            "{field:?}"
        );
        assert_eq!(
            (
                tools::field_text(&set, field, Units::Inch),
                tools::field_unit(field, Units::Inch)
            ),
            (inch_text.to_owned(), inch_unit),
            "{field:?}"
        );
    }
    // The rows as the editor draws them.
    let rows = tools::rows(&set, Units::Mm);
    assert_eq!(
        rows.iter().map(|r| r.field).collect::<Vec<_>>(),
        tools::fields(&set)
    );
    let taper_row = rows
        .iter()
        .find(|r| r.field == ToolField::Taper)
        .expect("a taper row");
    assert_eq!(
        (
            strings.get(taper_row.label),
            taper_row.text.as_str(),
            taper_row.unit
        ),
        ("Taper per side", "6.7", "°")
    );
    assert_eq!(taper_row.required, None);
    for expected in steps.iter().rev().skip(1) {
        harness.app.undo().expect("a step to undo");
        assert_eq!(harness.app.project(), expected, "one edit, one step");
    }
    harness.app.undo().expect("the type switch is a step too");
    assert_eq!(harness.app.project(), &before);

    // A taper is degrees per side, more than 0° and at most 45°.
    for good in ["6.7", "6.7°", "6.7 deg", "45"] {
        assert!(
            tools::set_field(&set, ToolField::Taper, good, Units::Mm).is_ok(),
            "`{good}` is a taper"
        );
    }
    for bad in ["0", "-2", "45.1", "90", "steep", "6,7"] {
        assert!(
            matches!(
                tools::set_field(&set, ToolField::Taper, bad, Units::Mm),
                Err(CommandError::NotAQuantity { .. })
            ),
            "`{bad}` is not a taper per side"
        );
    }
    // A blank field clears its value: the file holds `null`.
    for field in own {
        let cleared = tools::set_field(&set, field, " ", Units::Mm).expect("clears");
        let (tip, taper, flutes) = tapered(&cleared);
        let now_unset = match field {
            ToolField::TipDiameter => tip.is_none(),
            ToolField::Taper => taper.is_none(),
            _ => flutes.is_none(),
        };
        assert!(now_unset, "{field:?} cleared");
        assert_eq!(cleared.unset_values().len(), 1, "and only it");
    }
    // The fields are a tapered ball nose's: on another type they change
    // nothing, as a V-bit's angle does not.
    for field in own {
        assert_eq!(tools::set_field(&t2, field, "3", Units::Mm).unwrap(), t2);
        assert_eq!(tools::field_text(&t2, field, Units::Mm), "");
    }

    // The type again changes nothing; another type drops the values; a
    // V-bit is still held for its angle.
    assert_eq!(
        tools::set_kind(&set, KindChoice::TaperedBallNose),
        KindSwitch::Unchanged
    );
    assert_eq!(
        tools::set_kind(&set, KindChoice::EndMill),
        KindSwitch::Update(Box::new(t2.clone()))
    );
    assert_eq!(
        tools::set_kind(&set, KindChoice::VBit),
        KindSwitch::HoldForAngle
    );
}

/// #685; capabilities 63, 64a and 66b: each value a tapered ball nose
/// still needs (tip, taper, flute length) is flagged at its own field,
/// from `Tool::unset_values`, with the message the file gives
/// (`tool_value_unset`, the line `check` prints), whether or not an
/// operation uses the tool yet. `diameter_mm` is always present, so it
/// is never flagged. The library's editing pane reads the same.
#[test]
fn tapered_ball_nose_unset_fields_show_required() {
    let mut harness = Harness::open(&tapered_bytes()).expect("the test part opens");
    assert!(!harness.app.validation().has_errors(), "it exports");
    let t3 = tool(harness.app.project(), "t3").clone();
    assert!(!tools::lacks_value(&t3));
    assert!(tools::unset_values(&t3).is_empty());
    assert!(
        tools::rows(&t3, Units::Mm)
            .iter()
            .all(|r| r.required.is_none())
    );

    // The Inlay's bit without its flute length.
    let cleared = tools::set_field(&t3, ToolField::FluteLength, "", Units::Mm).unwrap();
    harness.run(Command::UpdateTool(cleared)).unwrap();
    let t3 = tool(harness.app.project(), "t3").clone();
    let message = "tools[t3].cutter.tapered_ball_nose.flute_length_mm is not set (mm) for tool `0.762 mm tip tapered ball nose`";
    assert_eq!(
        tools::required(&t3, ToolField::FluteLength).as_deref(),
        Some(message)
    );
    assert_eq!(
        tools::unset_values(&t3),
        [(
            "cutter.tapered_ball_nose.flute_length_mm",
            message.to_owned()
        )]
    );
    assert!(tools::lacks_value(&t3), "the row's red mark");
    assert!(!tools::lacks_rate(&t3), "its rates are all set");
    assert!(tools::unset_rates(&t3).is_empty());
    for field in [
        ToolField::Diameter,
        ToolField::TipDiameter,
        ToolField::Taper,
        ToolField::Feed,
    ] {
        assert_eq!(tools::required(&t3, field), None, "{field:?} is set");
    }
    let flagged: Vec<(ToolField, Option<String>)> = tools::rows(&t3, Units::Mm)
        .into_iter()
        .filter(|r| r.required.is_some())
        .map(|r| (r.field, r.required))
        .collect();
    assert_eq!(
        flagged,
        [(ToolField::FluteLength, Some(message.to_owned()))],
        "the row carries its message"
    );
    // The message is the file's: the refusal `check` prints.
    let refusal = harness
        .warnings()
        .iter()
        .find(|f| f.field.as_deref() == Some("tools[t3].cutter.tapered_ball_nose.flute_length_mm"))
        .expect("the refusal is listed");
    assert_eq!(refusal.tier, Tier::Error);
    assert_eq!(refusal.message, message);
    assert!(harness.app.validation().has_errors(), "export is disabled");

    // Every value unset, on a tool no operation uses: validation says
    // nothing of them yet, and the fields are flagged all the same.
    let KindSwitch::Update(unused) = tools::set_kind(
        tool(harness.app.project(), "t1"),
        KindChoice::TaperedBallNose,
    ) else {
        panic!("the placeholder becomes a tapered ball nose");
    };
    harness.run(Command::UpdateTool(*unused)).unwrap();
    assert!(
        !harness.warnings().iter().any(|f| f
            .field
            .as_deref()
            .is_some_and(|f| f.starts_with("tools[t1]."))),
        "an unused tool's unset values are not errors: {:?}",
        harness.warnings()
    );
    let unused = tool(harness.app.project(), "t1").clone();
    let named: Vec<&str> = tools::unset_values(&unused)
        .into_iter()
        .map(|(field, _)| field)
        .collect();
    assert_eq!(
        named,
        [
            "cutter.tapered_ball_nose.tip_diameter_mm",
            "cutter.tapered_ball_nose.taper_angle_deg",
            "cutter.tapered_ball_nose.flute_length_mm",
            "feed_mm_min",
            "plunge_mm_min",
            "rpm",
        ],
        "the cutter's values, then the rates: the file's order"
    );
    assert_eq!(
        tools::required(&unused, ToolField::Taper).as_deref(),
        Some(
            "tools[t1].cutter.tapered_ball_nose.taper_angle_deg is not set (degrees per side) for tool `Set me`"
        )
    );
    assert_eq!(
        tools::required(&unused, ToolField::TipDiameter).as_deref(),
        Some(
            "tools[t1].cutter.tapered_ball_nose.tip_diameter_mm is not set (mm) for tool `Set me`"
        )
    );
    assert_eq!(
        tools::required(&unused, ToolField::Diameter),
        None,
        "the cutting diameter is always present"
    );
    assert_eq!(
        tools::unset_rates(&unused)
            .iter()
            .map(|(field, _)| *field)
            .collect::<Vec<_>>(),
        ["feed_mm_min", "plunge_mm_min", "rpm"],
        "the rates alone, as before"
    );

    // Typed back: the mark and the refusal go.
    let typed = tools::set_field(&t3, ToolField::FluteLength, "19.05", Units::Mm).unwrap();
    harness.run(Command::UpdateTool(typed)).unwrap();
    let t3 = tool(harness.app.project(), "t3").clone();
    assert_eq!(tools::required(&t3, ToolField::FluteLength), None);
    assert!(!tools::lacks_value(&t3));
    assert!(!harness.app.validation().has_errors());

    // The library's editing pane: the same fields, the same marks.
    let mut lib = Library::default();
    dc_library::add(&mut lib, t3, None).expect("the library takes it");
    let id = lib.tools[0].id.clone();
    let lib = dc_app::library::edit(&lib, &id, ToolField::Taper, "", Units::Inch)
        .expect("an unset value is not a fault in the library");
    let entry = &lib.tools[0].tool;
    assert_eq!(tapered(entry).1, None);
    assert!(
        tools::required(entry, ToolField::Taper)
            .is_some_and(|m| m.contains("cutter.tapered_ball_nose.taper_angle_deg is not set")),
        "flagged in the library's pane too"
    );
    let lib = dc_app::library::edit(&lib, &id, ToolField::TipDiameter, "0.045", Units::Inch)
        .expect("a tip in the library's units");
    dc_geometry::assert_approx_eq!(tapered(&lib.tools[0].tool).0.unwrap(), 1.143, 1e-9);
    assert!(
        dc_app::library::edit(&lib, &id, ToolField::TipDiameter, "0.3", Units::Inch).is_err(),
        "a tip wider than the cutting diameter is refused, as the file is"
    );
}

/// Capability 63 (#581, #685): while a tool's switch to V-bit is held for
/// its angle, the editor draws a V-bit's rows over the tool the project
/// still holds (`rows_for`): the *Angle* row empty, and none of a tapered
/// ball nose's own rows, whatever the tool still is. For its own type the
/// rows are `rows`'.
#[test]
fn a_held_switch_draws_the_v_bits_rows() {
    let tbn = dc_project::test_support::tool_tbn_ultra();
    assert_eq!(
        tools::rows_for(&tbn, KindChoice::TaperedBallNose, Units::Mm),
        tools::rows(&tbn, Units::Mm)
    );
    let held = tools::rows_for(&tbn, KindChoice::VBit, Units::Mm);
    assert_eq!(
        held.iter().map(|r| r.field).collect::<Vec<_>>(),
        tools::fields_for(KindChoice::VBit)
    );
    let angle = held
        .iter()
        .find(|r| r.field == ToolField::Angle)
        .expect("an Angle row");
    assert_eq!((angle.text.as_str(), angle.unit), ("", "°"));
    assert_eq!(angle.required, None, "the editor says what to type");
    let diameter = held
        .iter()
        .find(|r| r.field == ToolField::Diameter)
        .expect("a Diameter row");
    assert_eq!(diameter.text, "6.35", "the tool's own values still show");
}

/// Capability 76 (slice D9): a finding's field path names the tool and
/// the editor's field a Warnings row opens and focuses. A rate is named
/// by its own name, a cutter's value under its variant, and `unused_tool`
/// names the tool alone; a path of another kind, or of a tool the project
/// does not have, names none.
#[test]
fn a_findings_path_names_the_tool_and_its_field() {
    let project = plaque();
    let t2 = ToolId::new("t2");
    let located = |path: &str| tools::located(&project, path);
    assert_eq!(
        located("tools[t2].feed_mm_min"),
        Some((t2.clone(), Some(ToolField::Feed)))
    );
    assert_eq!(
        located("tools[t2].rpm"),
        Some((t2.clone(), Some(ToolField::Rpm)))
    );
    assert_eq!(located("tools[t2]"), Some((t2.clone(), None)));
    assert_eq!(
        located("tools[t2].cutter.v_bit.included_angle_deg"),
        Some((t2, None)),
        "an end mill has no angle field to focus"
    );
    assert_eq!(located("tools[t9].feed_mm_min"), None);
    assert_eq!(located("stock.thickness_mm"), None);
    assert_eq!(located("operations[op1].tool"), None);

    let sign = common::load_dcv(&fixture("sign.dcv"));
    assert_eq!(
        tools::located(&sign, "tools[t4].cutter.v_bit.included_angle_deg"),
        Some((ToolId::new("t4"), Some(ToolField::Angle)))
    );
    let tapered = dc_project::load(&tapered_bytes()).unwrap().project;
    let nose = tapered
        .tools
        .iter()
        .find(|t| matches!(t.cutter, ToolKind::TaperedBallNose { .. }))
        .unwrap();
    for (name, field) in [
        ("tip_diameter_mm", ToolField::TipDiameter),
        ("taper_angle_deg", ToolField::Taper),
        ("flute_length_mm", ToolField::FluteLength),
    ] {
        let path = format!("tools[{}].cutter.tapered_ball_nose.{name}", nose.id);
        assert_eq!(
            tools::located(&tapered, &path),
            Some((nose.id.clone(), Some(field))),
            "{path}"
        );
    }
}

/// Capability 63 (slice D9): a tool's row carries what is said of the
/// tool, as an operation's row does of the operation. On the plaque the
/// unused `Set me` lacks its rates, with the `unused_tool` hint behind
/// the dot, and shows the hint once they are set; the others are clean.
/// A plunge rate above the feed on the 1/4 end mill is a warning located
/// on that tool, so its row is marked with one warning and holds its
/// message, where the row showed nothing. An error that names the tool
/// (its diameter put at zero) outranks it. A value the tool still needs
/// is the red dot, as before, with the warnings still behind it.
#[test]
fn a_tools_row_carries_the_findings_that_name_it() {
    use tools::RowMark;
    let mut harness = Harness::from_project(plaque());
    harness.generate();
    let status = |harness: &Harness, id: &str| {
        tools::row_status(tool(harness.app.project(), id), harness.warnings())
    };
    let placeholder = status(&harness, "t1");
    assert_eq!(placeholder.mark, RowMark::LacksValue);
    assert!(
        placeholder
            .messages
            .last()
            .unwrap()
            .contains("not used by any"),
        "{:?}",
        placeholder.messages
    );
    let mut set = tool(harness.app.project(), "t1").clone();
    (set.feed_mm_min, set.plunge_mm_min, set.rpm) = (Some(900.0), Some(300.0), Some(18_000));
    harness.run(Command::UpdateTool(set)).unwrap();
    assert_eq!(status(&harness, "t1").mark, RowMark::Hint);
    assert_eq!(status(&harness, "t2").mark, RowMark::Clean);
    assert_eq!(status(&harness, "t3").mark, RowMark::Clean);
    assert!(status(&harness, "t2").messages.is_empty());

    let mut fast = tool(harness.app.project(), "t2").clone();
    fast.plunge_mm_min = Some(5000.0);
    harness.run(Command::UpdateTool(fast.clone())).unwrap();
    let warned = status(&harness, "t2");
    assert_eq!(warned.mark, RowMark::Warnings(1));
    assert_eq!(warned.messages.len(), 1);
    assert!(
        warned.messages[0].contains("plunges at 5000 mm/min"),
        "{:?}",
        warned.messages
    );
    assert_eq!(status(&harness, "t3").mark, RowMark::Clean, "only its own");

    let mut flat = fast.clone();
    flat.diameter_mm = 0.0;
    harness.run(Command::UpdateTool(flat)).unwrap();
    let wrong = status(&harness, "t2");
    assert_eq!(wrong.mark, RowMark::Error);
    assert!(
        wrong.messages[0].contains("diameter_mm is 0"),
        "the error first: {:?}",
        wrong.messages
    );

    let mut unset = fast;
    unset.rpm = None;
    harness.run(Command::UpdateTool(unset)).unwrap();
    let lacking = status(&harness, "t2");
    assert_eq!(lacking.mark, RowMark::LacksValue);
    assert!(lacking.messages[0].contains("tools[t2].rpm is not set"));
    assert!(
        lacking
            .messages
            .iter()
            .any(|m| m.contains("plunges at 5000")),
        "the warning is still listed: {:?}",
        lacking.messages
    );
}

/// The smoke test's item 7: a stepover is a percentage of the diameter,
/// and `0.08 %` typed where 8 % was meant is 0.005 mm between passes, a
/// job four times as long. Beside the percentage the editor says the
/// distance it comes to, from what is typed, in the units shown: 8 % of a
/// 6.35 mm bit is 0.508 mm, 0.08 % is 0.005 mm, and in inches 0.02 in and
/// 0.0002 in. Nothing typed, or text that is no percentage, says nothing.
#[test]
fn a_stepover_says_the_distance_between_passes() {
    let strings = dc_app::Strings::english();
    let project = plaque();
    let tool = &project.tools[1];
    assert!((tool.diameter_mm - 6.35).abs() < 1e-12);
    let note = |typed: &str, units| tools::step_over_note(&strings, tool, typed, units);
    assert_eq!(
        note("8", Units::Mm).as_deref(),
        Some("0.508 mm between passes")
    );
    assert_eq!(
        note("8 %", Units::Mm).as_deref(),
        Some("0.508 mm between passes")
    );
    assert_eq!(
        note("0.08", Units::Mm).as_deref(),
        Some("0.005 mm between passes"),
        "the maintainer's 0.08 %"
    );
    assert_eq!(
        note("40", Units::Mm).as_deref(),
        Some("2.54 mm between passes")
    );
    assert_eq!(
        note("8", Units::Inch).as_deref(),
        Some("0.02 in between passes")
    );
    assert_eq!(
        note("0.08", Units::Inch).as_deref(),
        Some("0.0002 in between passes")
    );
    // Too small for the figures shown: said so, never as 0.
    assert_eq!(
        note("0.001", Units::Mm).as_deref(),
        Some("less than 0.001 mm between passes")
    );
    for nothing in ["", "  ", "wide", "-5", "0"] {
        assert_eq!(note(nothing, Units::Mm), None, "`{nothing}`");
    }

    // An operation's own stepover says it with its tool's diameter, and
    // an empty one with the tool's stepover, which it takes. A V-Carve's
    // floor stepover is a share of the cut's width there, not of the
    // diameter, so it says nothing.
    let pocket = &project.operations[2];
    assert!(matches!(pocket, dc_project::Operation::Pocket(_)));
    let row = dc_app::editor::fields::rows(pocket)
        .into_iter()
        .find(|r| r.key == "step_over_fraction")
        .expect("a Pocket has a stepover");
    let of = |typed: &str| dc_app::editor::step_over_note(&project, &strings, pocket, row, typed);
    assert_eq!(of("50").as_deref(), Some("3.175 mm between passes"));
    assert_eq!(
        of("").as_deref(),
        Some("2.54 mm between passes"),
        "empty takes the tool's 40 %"
    );
    assert_eq!(of("wide"), None);
    let depth = dc_app::editor::fields::rows(pocket)
        .into_iter()
        .find(|r| r.key == "depth_mm")
        .unwrap();
    assert_eq!(
        dc_app::editor::step_over_note(&project, &strings, pocket, depth, "50"),
        None,
        "only a stepover"
    );
    let sign = common::load_dcv(&fixture("sign.dcv"));
    for op in &sign.operations {
        for row in dc_app::editor::fields::rows(op) {
            if !matches!(op, dc_project::Operation::Pocket(_)) {
                assert_eq!(
                    dc_app::editor::step_over_note(&sign, &strings, op, row, "50"),
                    None,
                    "{}: {}",
                    op.name(),
                    row.key
                );
            }
        }
    }
}

/// The smoke test's item 7, second half: a finding located at a tool's
/// field shows under that field in the tool editor, as an operation's
/// does under its own, so the warning for an absurd stepover has
/// somewhere to show. A plunge rate above the feed is located at the
/// plunge; the value a tool still needs is not repeated here, since its
/// field says Required.
#[test]
fn a_finding_at_a_tools_field_is_that_fields_status() {
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let mut fast = harness.app.project().tools[1].clone();
    fast.plunge_mm_min = Some(5000.0);
    fast.feed_mm_min = Some(1000.0);
    harness.run(Command::UpdateTool(fast.clone())).unwrap();
    harness.generate();
    let tool = harness.app.project().tools[1].clone();
    let status = tools::field_status(harness.app.project(), harness.app.warnings(), &tool);
    let plunge = status
        .iter()
        .find(|(field, _)| *field == ToolField::Plunge)
        .map(|(_, status)| *status)
        .expect("the plunge has a finding");
    assert_eq!(plunge.tier, Tier::Warning);
    assert!(
        plunge.message.contains("plunges at 5000"),
        "{}",
        plunge.message
    );
    assert!(
        status.iter().all(|(field, _)| *field == ToolField::Plunge),
        "only that field: {status:?}"
    );
    // Another tool's findings are not this one's.
    let other = harness.app.project().tools[2].clone();
    assert!(tools::field_status(harness.app.project(), harness.app.warnings(), &other).is_empty());
    // An unset value is the field's Required look, not a line under it.
    fast.rpm = None;
    harness.run(Command::UpdateTool(fast)).unwrap();
    harness.generate();
    let tool = harness.app.project().tools[1].clone();
    let status = tools::field_status(harness.app.project(), harness.app.warnings(), &tool);
    assert!(
        status.iter().all(|(field, _)| *field != ToolField::Rpm),
        "{status:?}"
    );
}

/// An angle's field says the same angle the other way, as it is typed: a
/// tapered ball nose's taper is per side and its note gives the included
/// angle; a V-bit's angle is included and its note gives the angle per
/// side. A bit sold as "6.7°" entered as 6.7 per side reads
/// `13.4° included`, which is how the slip shows.
#[test]
fn an_angle_field_says_the_angle_the_other_way() {
    let strings = dc_app::strings::Strings::english();
    let mut tapered = dc_project::test_support::tool_6_35_upcut();
    tapered.cutter = dc_project::ToolKind::TaperedBallNose {
        tip_diameter_mm: Some(2.286),
        taper_angle_deg: Some(6.7),
        flute_length_mm: Some(19.05),
    };
    let taper = |typed: &str| tools::angle_note(&strings, &tapered, ToolField::Taper, typed);
    assert_eq!(taper("6.7").as_deref(), Some("13.4° included"));
    assert_eq!(taper("3.35").as_deref(), Some("6.7° included"));
    assert_eq!(taper("3.35°").as_deref(), Some("6.7° included"));
    assert_eq!(taper(""), None, "empty is not set");
    assert_eq!(taper("steep"), None);
    assert_eq!(taper("60"), None, "a taper the field refuses has no note");

    let mut v_bit = dc_project::test_support::tool_6_35_upcut();
    v_bit.cutter = dc_project::ToolKind::VBit {
        included_angle_deg: 60.0,
    };
    let angle = |typed: &str| tools::angle_note(&strings, &v_bit, ToolField::Angle, typed);
    assert_eq!(angle("60").as_deref(), Some("30° per side"));
    assert_eq!(angle("90").as_deref(), Some("45° per side"));
    assert_eq!(angle("13.4").as_deref(), Some("6.7° per side"));
    assert_eq!(angle(""), None);

    // Only the two angle fields, and only on the cutter that has them.
    assert_eq!(
        tools::angle_note(&strings, &tapered, ToolField::Angle, "60"),
        None
    );
    assert_eq!(
        tools::angle_note(&strings, &v_bit, ToolField::Taper, "6.7"),
        None
    );
    assert_eq!(
        tools::angle_note(&strings, &v_bit, ToolField::Diameter, "6"),
        None
    );
}

/// Review feedback, 2026-11-23: *Add from library…* in an operation's
/// Tool picker adds the tool and chooses it for that operation, in one
/// undo step. It was two: add the tool, then pick it.
#[test]
fn a_tool_added_from_an_operations_picker_is_chosen_for_it() {
    let lib = sample_library();
    let end_mill = entry_named(&lib, "1/4 in Flat End Mill");
    let mut harness = Harness::open(&plaque_bytes()).expect("the plaque opens");
    let before = harness.app.project().clone();
    let op = before.operations[0].clone();
    let one = BTreeSet::from([end_mill.id.clone()]);
    let command = dc_app::editor::add_from_library_for(&before, &lib, &one, op.id(), "tool")
        .expect("one ticked row is something to add");
    let label = harness.run(command).expect("the tool is added and chosen");
    assert_eq!(
        label,
        format!("Add tool “1/4 in Flat End Mill” for “{}”", op.name())
    );
    let project = harness.app.project();
    assert_eq!(project.tools.len(), before.tools.len() + 1);
    let added = project.tools.last().expect("a tool was added");
    assert_eq!(project.operations[0].tool(), &added.id, "and chosen");
    // One step: Undo takes both back.
    assert!(harness.app.undo().is_some());
    assert_eq!(harness.app.project(), &before);

    // Several ticked rows have no one tool to choose: they are added, as
    // *Add to project* adds them, and the operation keeps its tool.
    let both: BTreeSet<_> = lib.tools.iter().take(2).map(|e| e.id.clone()).collect();
    assert_eq!(both.len(), 2);
    let command = dc_app::editor::add_from_library_for(&before, &lib, &both, op.id(), "tool")
        .expect("something to add");
    assert!(matches!(command, Command::AddTools(ref tools) if tools.len() == 2));
    // A tool the operation cannot carve with is added and not chosen: a
    // V-Carve takes no end mill.
    let v_carve = before
        .operations
        .iter()
        .find(|op| matches!(op, dc_project::Operation::VCarve(_)));
    if let Some(v_carve) = v_carve {
        let command =
            dc_app::editor::add_from_library_for(&before, &lib, &one, v_carve.id(), "tool")
                .expect("something to add");
        assert!(matches!(command, Command::AddTools(_)), "{command:?}");
    }
    // Nothing ticked is nothing to add.
    assert!(
        dc_app::editor::add_from_library_for(&before, &lib, &BTreeSet::new(), op.id(), "tool")
            .is_none()
    );
}
