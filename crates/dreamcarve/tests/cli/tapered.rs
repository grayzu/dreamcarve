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
    write_library_file(
        &dir,
        "bits.json",
        &[(tool_tbn_ultra(), None), (unset, None)],
    );
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

    let out = run(
        &dir,
        &["tools", "add", "coaster.dcv", "IDC Ultra-High Detail"],
    );
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
    assert!(
        text.lines().any(|l| l == format!("t2 {ULTRA_LINE}")),
        "{text}"
    );
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
            report.lines().any(
                |l| l.starts_with(&format!("coaster.dcv:{position}: error: ")) && l.contains(&at)
            ),
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

/// Slice 27e (#681; design spec, "The taper's ±0.5°"): `check` prints the
/// seat line of an Inlay cut with a tapered ball nose, from its own
/// `depth_mm` and `plug_depth_mm`: the "DC" pair with the Ultra-High
/// Detail bit at the 6 mm test part's depths (`D` 6, `G` 0.85, `P` 2, a
/// 19 mm male board) moves 0.55 mm per 0.5° of taper and 0.76 mm per
/// 0.05 mm of tip radius. The pair with its 60° V-bit prints none.
#[test]
fn check_prints_the_seat_line() {
    let dir = work_dir("check_prints_the_seat_line");
    let dcv = copy_fixture(&dir, "dc.dcv", "dc.dcv");
    let out = run(&dir, &["check", "dc.dcv"]);
    assert_eq!(out.status.code(), Some(0), "check: {}", stdout(&out));
    assert!(!stdout(&out).contains("seat:"), "{}", stdout(&out));

    let mut project = load_file(&dcv).project;
    let bit = project
        .tools
        .iter_mut()
        .find(|t| t.id.as_str() == "t3")
        .expect("the pair's V-bit");
    *bit = dc_project::Tool {
        id: bit.id.clone(),
        ..tool_tbn_ultra()
    };
    for op in &mut project.operations {
        if let dc_project::Operation::Inlay(inlay) = op {
            inlay.depth_mm = Some(6.0);
            inlay.glue_gap_mm = Some(0.85);
            inlay.plug_depth_mm = Some(2.0);
            inlay.male_board.thickness_mm = Some(19.0);
        }
    }
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let out = run(&dir, &["check", "dc.dcv"]);
    let report = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "check: {report}");
    assert!(
        report.lines().any(|l| l
            == "op1 `DC` seat: moves 0.55 mm per 0.5° of taper, 0.76 mm per 0.05 mm of tip radius; test-fit before gluing"),
        "{report}"
    );
}

// ── Test part 5's projects (slice 27g, #683; functional spec, "Test ────────
// ── parts → Tapered ball nose carve and inlay") ─────────────────────────

/// The seat line `check` prints for the "DC" pair of `dc-tbn.dcv`: the
/// fine tip at `D` 6, `P` 2 (design spec, "Slice 27e's decisions").
const DC_TBN_SEAT: &str = "op2 `DC inlay` seat: moves 0.55 mm per 0.5° of taper, 0.76 mm per 0.05 mm of tip radius; test-fit before gluing";

/// `tests/fixtures/dc-tbn.dcv` copied into `dir`, with its carve (`op1`)
/// and its Inlay (`op2`) enabled as asked.
fn dc_tbn(dir: &Path, carve: bool, inlay: bool) -> PathBuf {
    let dcv = copy_fixture(dir, "dc-tbn.dcv", "dc-tbn.dcv");
    let mut project = load_file(&dcv).project;
    for op in &mut project.operations {
        match op {
            Operation::VCarve(op) => op.enabled = carve,
            Operation::Inlay(op) => op.enabled = inlay,
            other => panic!("dc-tbn.dcv holds a carve and an Inlay, not {other:?}"),
        }
    }
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    dcv
}

/// The lowest and highest Z of the cutting moves of the program `name` in
/// `dir`.
fn cut_z_range(dir: &Path, name: &str) -> (f64, f64) {
    let program = std::fs::read_to_string(dir.join(name)).expect("program readable");
    dc_gcode::parse_motion(&program)
        .unwrap_or_else(|e| panic!("{name} parses: {e}"))
        .iter()
        .filter(|m| m.kind.is_cut())
        .filter_map(|m| m.to.z)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), z| {
            (lo.min(z), hi.max(z))
        })
}

/// The project's one enabled Inlay.
fn enabled_inlay(project: &Project) -> &dc_project::InlayOp {
    project
        .operations
        .iter()
        .find_map(|op| match op {
            Operation::Inlay(inlay) if inlay.enabled => Some(inlay),
            _ => None,
        })
        .expect("the project has an enabled Inlay")
}

/// Generates the Inlay of `dcv`, holds every run to the safety pass on its
/// own board, and seats the plug in the pocket by heightfield: within
/// 0.05 mm (the `dc-cam` seat tests' measure).
fn assert_pair_seats(dcv: &Path) {
    let project = load_file(dcv).project;
    let inlay = enabled_inlay(&project);
    let (runs, _) = dc_cam::generate_inlay(&project, inlay).expect("the inlay generates");
    for run in &runs {
        dc_cam::test_support::assert_run_safe(run, &project, &Operation::Inlay(inlay.clone()));
    }
    let fit = dc_cam::test_support::seat::fit(&project, inlay, &runs, 0.2);
    assert!(
        fit.is_flush(),
        "the plug stands {:.3} mm proud (the male's share {:.3}) at {:?}",
        fit.seat_mm,
        fit.male_mm,
        fit.seat_at
    );
}

/// `tests/fixtures/dc-tbn.dcv`'s carve: the "DC" letters V-carved with the
/// 2.286 mm tip, `flat_depth_mm` 2, `step_over_fraction` 0.1, no clearing,
/// on the 12 mm stock. It exports one program, which passes S1 to S19,
/// cuts no deeper than the flat depth and reaches it, and warns
/// `tapered_corners_softened` for the serifs.
#[test]
fn tapered_carve_exports() {
    let dir = work_dir("tapered_carve_exports");
    dc_tbn(&dir, true, false);
    let out = run(&dir, &["export", "dc-tbn.dcv", "--date", FIXED_DATE]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(programs_in(&dir), ["dc-tbn.nc"], "{report}");
    assert!(
        warning_codes(&report).contains(&"tapered_corners_softened".to_owned()),
        "{report}"
    );
    assert!(
        report.contains("V-Carve op1 `DC carve`: deepest cut 2.000 mm below the stock top"),
        "{report}"
    );
    let (lowest, _) = cut_z_range(&dir, "dc-tbn.nc");
    assert!((lowest + 2.0).abs() < 1e-3, "the floor at Z −2: {lowest}");
}

/// `tests/fixtures/dc-tbn.dcv`'s inlay pair (test part 5): the "DC" letters
/// with the 0.762 mm tip at `depth_mm` 6, `glue_gap_mm` 0.85,
/// `plug_depth_mm` 2, `margin_mm` 8, `outline_rounding_mm` 10, the
/// 3.175 mm end mill's clearing pass and cut-out, the male board 19 mm
/// thick. The male's floor is at Z −7.15 and nothing but the cut-out goes
/// below it; the flank is engaged 7.15 mm against the bit's 19.05 mm
/// flutes; 11.85 mm of male board is left, so no `inlay_plug_base_thin`;
/// the female's floor is inset `w(6)` = 1.044 mm; the design is rounded to
/// 0.983 mm; the band's edge is open; `check` prints the seat line. Every
/// program passes S1 to S19 and the pair seats within 0.05 mm by
/// heightfield. No hairline of the letters is too narrow for the bit, so
/// the letters are not scaled.
#[test]
fn tapered_inlay_exports_both_boards() {
    let dir = work_dir("tapered_inlay_exports_both_boards");
    let dcv = copy_fixture(&dir, "dc-tbn.dcv", "dc-tbn.dcv");
    let project = load_file(&dcv).project;
    let inlay = enabled_inlay(&project);
    assert!(
        project
            .operations
            .iter()
            .all(|op| matches!(op, Operation::Inlay(_)) || !op.enabled()),
        "the fixture's carve is disabled, so the stock is the female"
    );
    assert_eq!(
        (inlay.depth_mm, inlay.glue_gap_mm, inlay.plug_depth_mm),
        (Some(6.0), Some(0.85), Some(2.0))
    );
    assert_eq!(inlay.male_board.thickness_mm, Some(19.0));
    let bit = project
        .tools
        .iter()
        .find(|t| t.id == inlay.tool)
        .expect("the Inlay's bit");
    let cutting_mm = bit.cutting_height_mm().expect("a cutting height");
    assert!((cutting_mm - 19.05).abs() < 1e-9, "{cutting_mm}");
    assert_eq!(bit.cutting_limit(), Some(dc_project::CuttingLimit::Flutes));
    let profile = bit.carve_profile().expect("the bit's profile");
    let floor_inset_mm = profile.half_width_mm(6.0);
    assert!((floor_inset_mm - 1.044).abs() < 5e-4, "{floor_inset_mm}");
    let band = dc_project::male_band(&project, inlay).expect("the band");
    assert!(band.is_open(), "{band:?}");

    let out = run(&dir, &["check", "dc-tbn.dcv"]);
    let report = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "check: {report}");
    assert!(report.lines().any(|l| l == DC_TBN_SEAT), "{report}");
    assert!(
        report.contains("deepest cut 7.150 mm below the male board's top"),
        "{report}"
    );
    assert!(report.contains("radius of 0.983 mm"), "{report}");
    let codes = warning_codes(&report);
    assert!(
        codes.contains(&"inlay_rounded_for_bit".to_owned()),
        "{report}"
    );
    for absent in [
        "inlay_plug_base_thin",
        "inlay_glue_gap_below_ball",
        "inlay_plug_depth_below_ball",
    ] {
        assert!(!codes.contains(&absent.to_owned()), "{absent}: {report}");
    }

    let out = run(&dir, &["export", "dc-tbn.dcv", "--date", FIXED_DATE]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(
        programs_in(&dir),
        [
            "dc-tbn-female-1-T3.nc",
            "dc-tbn-female-2-T2.nc",
            "dc-tbn-male-1-T3.nc",
            "dc-tbn-male-2-T2.nc",
            "dc-tbn-male-3-T3.nc",
        ],
        "{report}"
    );
    for female in ["dc-tbn-female-1-T3.nc", "dc-tbn-female-2-T2.nc"] {
        let (lowest, _) = cut_z_range(&dir, female);
        assert!((lowest + 6.0).abs() < 0.01, "{female}: {lowest}");
    }
    for male in ["dc-tbn-male-1-T3.nc", "dc-tbn-male-2-T2.nc"] {
        let (lowest, _) = cut_z_range(&dir, male);
        assert!((lowest + 7.15).abs() < 0.01, "{male}: {lowest}");
    }
    let (cut_out, _) = cut_z_range(&dir, "dc-tbn-male-3-T3.nc");
    assert!((cut_out + 19.2).abs() < 1e-3, "the cut-out: {cut_out}");

    assert_pair_seats(&dcv);
}

/// The coupon of test part 5, `tests/fixtures/tbn-coupon.dcv`: the 20 mm
/// rounded square as an Inlay at the pair's depths (`depth_mm` 6,
/// `glue_gap_mm` 0.85, `plug_depth_mm` 2, `margin_mm` 8) with the 0.762 mm
/// tip, the end mill's clearing pass and cut-out, the male board 19 mm
/// thick. Both boards export, every program passing S1 to S19; `check`
/// prints the pair's seat line; nothing is rounded for the bit (its 3 mm
/// corners are rounder than 0.983 mm); and the plug seats within 0.05 mm
/// by heightfield.
#[test]
fn tapered_coupon_exports() {
    let dir = work_dir("tapered_coupon_exports");
    let dcv = copy_fixture(&dir, "tbn-coupon.dcv", "tbn-coupon.dcv");
    let out = run(&dir, &["check", "tbn-coupon.dcv"]);
    let report = stdout(&out);
    assert_eq!(out.status.code(), Some(0), "check: {report}");
    assert!(
        report.lines().any(|l| l
            == "op1 `Coupon` seat: moves 0.55 mm per 0.5° of taper, 0.76 mm per 0.05 mm of tip radius; test-fit before gluing"),
        "{report}"
    );
    assert!(
        !warning_codes(&report).contains(&"inlay_rounded_for_bit".to_owned()),
        "{report}"
    );

    let out = run(&dir, &["export", "tbn-coupon.dcv", "--date", FIXED_DATE]);
    let report = format!("{}{}", stdout(&out), stderr(&out));
    assert_eq!(out.status.code(), Some(0), "{report}");
    assert_eq!(
        programs_in(&dir),
        [
            "tbn-coupon-female-1-T3.nc",
            "tbn-coupon-female-2-T2.nc",
            "tbn-coupon-male-1-T3.nc",
            "tbn-coupon-male-2-T2.nc",
            "tbn-coupon-male-3-T3.nc",
        ],
        "{report}"
    );
    assert_pair_seats(&dcv);
}

/// A Photo V-Carve with a tapered ball nose is refused (capability 49h,
/// acceptance 61z): `check` and `export` exit 1, as for every validation
/// error, with `v_carve_needs_v_bit` saying a Photo V-Carve needs a V-bit,
/// and `export` writes nothing.
#[test]
fn photo_with_a_tapered_ball_nose_is_refused() {
    let dir = work_dir("photo_with_a_tapered_ball_nose_is_refused");
    copy_fixture(&dir, "photo/gradient.png", "gradient.png");
    let out = run(&dir, &["init", "gradient.png", "--width", "100"]);
    assert_eq!(out.status.code(), Some(0), "{}", stderr(&out));
    let dcv = dir.join("gradient.dcv");
    let mut project = load_file(&dcv).project;
    project.stock.thickness_mm = Some(12.0);
    let [tool] = project.tools.as_mut_slice() else {
        panic!("init writes one tool: {:?}", project.tools);
    };
    *tool = dc_project::Tool {
        id: tool.id.clone(),
        ..tool_tbn_ultra()
    };
    for op in &mut project.operations {
        if let Operation::PhotoVCarve(op) = op {
            op.max_depth_mm = Some(1.5);
        }
    }
    std::fs::write(&dcv, save(&project).expect("project saves")).expect("dcv written");
    let codes: Vec<&str> = dc_project::validate(&project)
        .iter()
        .filter(|p| p.is_error())
        .map(dc_project::Problem::code)
        .collect();
    assert_eq!(codes, ["v_carve_needs_v_bit"]);

    for command in ["check", "export"] {
        let out = run(&dir, &[command, "gradient.dcv"]);
        let report = format!("{}{}", stdout(&out), stderr(&out));
        assert_eq!(out.status.code(), Some(1), "{command}: {report}");
        assert!(
            report.lines().any(|l| l.contains("error: ")
                && l.contains("a tapered ball nose; a Photo V-Carve needs a V-bit")),
            "{command}: {report}"
        );
    }
    assert!(programs_in(&dir).is_empty(), "{:?}", programs_in(&dir));
}
