// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The tapered ball nose through the library and the project (sub-phase
//! 3.6, slice 27a, #677; `docs/specs/phase-3/design.md`, "Data model →
//! Tapered ball nose", "The library and required fields"): `tools list`,
//! `tools import` of a DreamCarve library, `tools add`, `check` and
//! `export` on its unset values, and format 28's unchanged programs.

use super::*;

use dc_project::ToolKind;
use dc_project::test_support::{tool_tbn_fine, tool_tbn_ultra};

/// The line `tools list` and `check` print for the Ultra-High Detail bit,
/// after its id.
const ULTRA_LINE: &str = "IDC Ultra-High Detail: tapered ball nose, 6.350 mm cut, tip 0.762 mm (radius 0.381 mm), 6.7° per side, 19.050 mm flutes";

/// The Ultra bit with no flute length and no feeds or speed, as a
/// vendor's library might give it.
fn ultra_partly_unset() -> dc_project::Tool {
    let mut tool = tool_tbn_ultra();
    tool.cutter = ToolKind::TaperedBallNose {
        tip_diameter_mm: Some(0.762),
        taper_angle_deg: Some(6.7),
        flute_length_mm: None,
    };
    tool.feed_mm_min = None;
    tool.rpm = None;
    tool
}

/// `tools list` shows a tapered ball nose's cut diameter, tip, taper per
/// side and flutes, `-` for an unset value.
#[test]
fn tools_list_shows_a_tapered_ball_nose() {
    let dir = work_dir("tools_list_shows_a_tapered_ball_nose");
    let mut unset = tool_tbn_fine();
    unset.cutter = ToolKind::TaperedBallNose {
        tip_diameter_mm: Some(2.286),
        taper_angle_deg: None,
        flute_length_mm: None,
    };
    write_library_file(&dir, "bits.json", &[(tool_tbn_ultra(), None), (unset, None)]);
    let out = run(&dir, &["tools", "import", "bits.json"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));

    let out = run(&dir, &["tools", "list"]);
    assert_eq!(out.status.code(), Some(0), "list: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.lines().any(|l| l == format!("lt1 {ULTRA_LINE}")),
        "{text}"
    );
    assert!(
        text.lines().any(|l| l
            == "lt2 IDC Fine Detail: tapered ball nose, 6.350 mm cut, tip 2.286 mm (radius 1.143 mm), -° per side, - mm flutes"),
        "{text}"
    );
    // Each tool's row of the table says what it is.
    let rows: Vec<&str> = text.lines().filter(|l| l.starts_with("lt")).collect();
    assert!(
        rows.iter()
            .filter(|l| l.contains("  tapered ball nose  "))
            .count()
            == 2,
        "{text}"
    );
}

/// `tools add` copies a tool with values unset and says, with their
/// locations in the project, which it still needs.
#[test]
fn tools_add_prints_the_values_still_to_set() {
    let dir = work_dir("tools_add_prints_the_values_still_to_set");
    let dcv = init_coaster(&dir);
    write_library_file(&dir, "bits.json", &[(ultra_partly_unset(), None)]);
    let out = run(&dir, &["tools", "import", "bits.json"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));

    let out = run(&dir, &["tools", "add", "coaster.dcv", "IDC Ultra-High Detail"]);
    assert_eq!(out.status.code(), Some(0), "add: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.lines().any(|l| l
            == "t2: set tools[t2].cutter.tapered_ball_nose.flute_length_mm (and feed_mm_min, rpm) before an operation uses it"),
        "{text}"
    );
    let project = load_file(&dcv).project;
    assert_eq!(project.tools[1].cutter, ultra_partly_unset().cutter);

    // A tool with every value set has nothing to say.
    write_library_file(&dir, "set.json", &[(tool_tbn_fine(), None)]);
    let out = run(&dir, &["tools", "import", "set.json"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));
    let out = run(&dir, &["tools", "add", "coaster.dcv", "IDC Fine Detail"]);
    assert_eq!(out.status.code(), Some(0), "add: {}", stderr(&out));
    assert!(!stdout(&out).contains(": set "), "{}", stdout(&out));
}

/// Import a DreamCarve library holding both bench bits, list them, add
/// one to a project. Its import line names its unset value; once that and
/// its feeds are set, `check` finds no error for it: unused, it is
/// `unused_tool` only (a V-Carve with it is refused until 27c, #679).
#[test]
fn tapered_ball_nose_library_round_trip() {
    let dir = work_dir("tapered_ball_nose_library_round_trip");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let out = run(&dir, &["check", "coaster.dcv"]);
    assert_eq!(out.status.code(), Some(0), "check: {}", stdout(&out));

    write_library_file(
        &dir,
        "bits.json",
        &[(ultra_partly_unset(), None), (tool_tbn_fine(), None)],
    );
    let out = run(&dir, &["tools", "import", "bits.json"]);
    assert_eq!(out.status.code(), Some(0), "import: {}", stderr(&out));
    let text = stdout(&out);
    assert!(
        text.contains(
            "`IDC Ultra-High Detail`: imported (lt1); set cutter.tapered_ball_nose.flute_length_mm"
        ),
        "{text}"
    );
    assert!(
        text.lines()
            .any(|l| l == "`IDC Fine Detail`: imported (lt2)"),
        "{text}"
    );
    assert!(text.contains("2 imported"), "{text}");
    let lib = read_library(&dir);
    assert_eq!(lib.tools[0].tool.cutter, ultra_partly_unset().cutter);
    assert_eq!(lib.tools[1].tool.cutter, tool_tbn_fine().cutter);

    let out = run(&dir, &["tools", "list"]);
    assert_eq!(out.status.code(), Some(0), "list: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("lt1 IDC Ultra-High Detail"), "{text}");
    assert!(text.contains("lt2 IDC Fine Detail"), "{text}");

    let out = run(&dir, &["tools", "add", "coaster.dcv", "lt1"]);
    assert_eq!(out.status.code(), Some(0), "add: {}", stderr(&out));
    let mut project = load_file(&dcv).project;
    let tool = &mut project.tools[1];
    tool.cutter = tool_tbn_ultra().cutter;
    tool.feed_mm_min = Some(1000.0);
    tool.rpm = Some(18000);
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");

    let out = run(&dir, &["check", "coaster.dcv"]);
    let text = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "check: {text}");
    assert!(text.contains("0 error(s)"), "{text}");
    assert!(
        text.lines()
            .any(|l| l.contains("warning: unused_tool:") && l.contains("`t2`")),
        "{text}"
    );
    assert!(text.lines().any(|l| l == format!("t2 {ULTRA_LINE}")), "{text}");
}

/// Every unset value of a tapered ball nose an operation uses is
/// `tool_value_unset` at its field, so `check` lists them in one run and
/// `export` refuses until each is set.
#[test]
fn check_lists_unset_tapered_values() {
    let dir = work_dir("check_lists_unset_tapered_values");
    let dcv = init_coaster(&dir);
    fill_in(&dcv, 1500.0, 500.0, 18000);
    let mut project = load_file(&dcv).project;
    project.tools[0].cutter = ToolKind::TaperedBallNose {
        tip_diameter_mm: None,
        taper_angle_deg: None,
        flute_length_mm: None,
    };
    let text = save(&project).expect("project saves");
    std::fs::write(&dcv, &text).expect("dcv written");
    let map = dc_project::SourceMap::parse(&text).expect("the file parses");

    let out = run(&dir, &["check", "coaster.dcv"]);
    assert_eq!(out.status.code(), Some(1), "check: {}", stdout(&out));
    let report = stdout(&out);
    for field in ["tip_diameter_mm", "taper_angle_deg", "flute_length_mm"] {
        let at = format!("tools[t1].cutter.tapered_ball_nose.{field}");
        let position = map.locate(&at).expect("the field is in the file");
        assert!(
            report
                .lines()
                .any(|l| l.starts_with(&format!("coaster.dcv:{position}: error: "))
                    && l.contains(&at)),
            "{at}:\n{report}"
        );
    }

    let out = run(&dir, &["export", "coaster.dcv", "--date", FIXED_DATE]);
    assert_eq!(out.status.code(), Some(1), "export: {}", stdout(&out));
    let said = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        said.contains("tools[t1].cutter.tapered_ball_nose.taper_angle_deg"),
        "{said}"
    );
    assert!(programs_in(&dir).is_empty(), "{:?}", programs_in(&dir));
}

/// Format 28 (#677) adds a cutter and changes nothing a version 27 file
/// cuts. The inlay pair's projects, the sign and the plaque, each written
/// as a version 27 file, export the same programs, byte for byte, as the
/// file written at version 28.
#[test]
fn v27_project_exports_the_same_programs() {
    let sources = [
        inlay_migration_fixture("dc-male.dcv"),
        inlay_migration_fixture("star-female.dcv"),
        tests_fixture("sign.dcv"),
        tests_fixture("plaque.dcv"),
    ];
    for path in sources {
        let stem = path
            .file_stem()
            .expect("a file name")
            .to_string_lossy()
            .into_owned();
        let dir = work_dir(&format!("v27_project_exports_the_same_programs_{stem}"));
        let project = load_file(&path).project;
        assert_eq!(project.version, 28);
        let current = save(&project).expect("project saves");
        let v27 = current.replacen("\"version\": 28,", "\"version\": 27,", 1);
        assert_ne!(v27, current, "{stem}");
        std::fs::write(dir.join("old.dcv"), &v27).expect("dcv written");
        std::fs::write(dir.join("new.dcv"), &current).expect("dcv written");
        for file in ["old.dcv", "new.dcv"] {
            let out = run(&dir, &["export", file, "--date", FIXED_DATE]);
            assert_eq!(
                out.status.code(),
                Some(0),
                "{stem} {file}: {}",
                stderr(&out)
            );
        }
        let programs = programs_in(&dir);
        let old: Vec<&String> = programs.iter().filter(|n| n.starts_with("old-")).collect();
        assert!(!old.is_empty(), "{stem}: {programs:?}");
        assert_eq!(
            old.len(),
            programs.iter().filter(|n| n.starts_with("new-")).count(),
            "{stem}: {programs:?}"
        );
        for name in old {
            let a = std::fs::read_to_string(dir.join(name)).expect("old written");
            let b = std::fs::read_to_string(dir.join(name.replacen("old-", "new-", 1)))
                .expect("new written");
            assert!(a == b, "{stem} {name}:\n{a}\n{b}");
        }
    }
}
