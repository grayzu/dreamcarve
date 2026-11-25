// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The app and the CLI on the same input give the same bytes
//! (`docs/specs/phase-4/design.md`, "Test harness"; functional spec, S20).
//! Each test runs the `dreamcarve` binary in its own directory under
//! `CARGO_TARGET_TMPDIR` and compares its output with what `dc-app`
//! makes of the same input through the test harness.
//!
//! Slice 8 (#537, acceptance 71): the Tool Library window's import report
//! and library file are `tools import`'s, and a tool the app adds from the
//! library is the copy `tools add` writes.
//!
//! Slice 17 (#546, acceptance 78): the tabs the app places, drags, deletes
//! and adds export to the program the CLI writes for the saved file.
//!
//! Slice 18 (#547, acceptance 83; S20): what the export sheet writes is
//! what `dreamcarve export` writes for the file the app saves, byte for
//! byte, on every post-processor, with and without the program-split
//! override; the Viewport's SVG dump is the file `--preview` writes
//! (slice 32, #624: the sheet no longer writes the preview); and the
//! result's text is the CLI's report, every line of it in exactly one of
//! the result's three parts (slice 33, #625).
//!
//! Slice 21 (#550, acceptance 90): the coaster, the plaque, the sign and
//! the inlay pair built in the app by the steps of the quick start "From
//! SVG to G-code in the app", read from the chapter itself (`app_steps`),
//! export the programs the command-line quick starts write.

#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test helpers fail the test on a broken setup"
)]

mod app_steps;
mod design_steps;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use dc_app::test_support::Harness;
use dc_library::Library;

/// A fresh, empty working directory for test `name`.
fn work_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("gui_equality")
        .join(name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("old work dir removed");
    }
    std::fs::create_dir_all(&dir).expect("work dir created");
    dir
}

/// The repository root.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The user config directory of a test working in `dir`.
fn config_dir(dir: &Path) -> PathBuf {
    dir.join("config")
}

/// Runs `dreamcarve` with `args` in `dir`, its config directory
/// [`config_dir`]; fails the test unless it exits 0.
fn run(dir: &Path, args: &[&str]) -> String {
    let out: Output = Command::new(env!("CARGO_BIN_EXE_dreamcarve"))
        .current_dir(dir)
        .env("DREAMCARVE_CONFIG_DIR", config_dir(dir))
        .args(args)
        .output()
        .expect("binary runs");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

/// Acceptance 71: `tools import tests/fixtures/fusion/sample.tools` (the
/// fixture read in place) prints the report the app shows, line for line,
/// and leaves the library file the app writes; importing again with
/// `--replace` agrees too.
#[test]
fn tools_import_report_equals_the_app() {
    let dir = work_dir("tools_import_report_equals_the_app");
    let sample = repo_root().join("tests/fixtures/fusion/sample.tools");
    let sample_arg = sample.to_str().expect("utf-8 path");
    let library_file = config_dir(&dir).join("tools.json");
    let library_text = library_file.display().to_string();
    let bytes = std::fs::read(&sample).expect("fixture readable");

    let mut lib = Library::default();
    for replace in [false, true] {
        let mut args = vec!["tools", "import", sample_arg];
        if replace {
            args.push("--replace");
        }
        let printed = run(&dir, &args);
        let merged = dc_app::library::import(&lib, &bytes, replace).expect("the app imports");
        let mut expected = merged.report.lines().join("\n");
        expected.push('\n');
        expected.push_str(&merged.report.summary(&library_text));
        expected.push('\n');
        assert_eq!(printed, expected, "replace {replace}");
        assert_eq!(
            std::fs::read_to_string(&library_file).expect("library written"),
            dc_library::save(&merged.library).unwrap(),
            "replace {replace}: the library file"
        );
        lib = merged.library;
    }
}

/// Acceptance 71: the tool the app adds from the library is the one
/// `tools add` writes, so the two project files are byte-identical.
#[test]
fn tools_add_equals_the_app() {
    let dir = work_dir("tools_add_equals_the_app");
    std::fs::copy(
        repo_root().join("fixtures/coaster.svg"),
        dir.join("coaster.svg"),
    )
    .expect("svg copied");
    run(&dir, &["init", "coaster.svg"]);
    let sample = repo_root().join("tests/fixtures/fusion/sample.tools");
    run(
        &dir,
        &["tools", "import", sample.to_str().expect("utf-8 path")],
    );
    let dcv = dir.join("coaster.dcv");
    let before = std::fs::read(&dcv).expect("dcv readable");

    run(
        &dir,
        &["tools", "add", "coaster.dcv", "1/4 in Flat End Mill"],
    );
    run(&dir, &["tools", "add", "coaster.dcv", "5 mm Drill"]);
    let cli = std::fs::read_to_string(&dcv).expect("dcv readable");

    let lib = dc_library::load(&std::fs::read(config_dir(&dir).join("tools.json")).unwrap())
        .expect("library loads");
    let mut harness = Harness::open(&before).expect("the project opens");
    for name in ["1/4 in Flat End Mill", "5 mm Drill"] {
        let entry = dc_library::find(&lib, name).expect("in the library");
        let selected = BTreeSet::from([entry.id.clone()]);
        let command = dc_app::tools::add_from_library(harness.app.project(), &lib, &selected)
            .expect("something to add");
        harness.run(command).expect("added");
    }
    assert_eq!(harness.save().expect("saves"), cli);
}

/// The date the tab test's programs are headed with: the harness's.
const DATE: &str = dc_app::test_support::DATE;

/// `init`'s coaster in `dir` with the quick start's numbers filled in
/// (the phase 1 test part's feeds, an 11.8 mm thickness and a depth
/// through it), so it exports: `coaster.dcv`, with its tab request.
fn ready_coaster(dir: &Path) -> PathBuf {
    std::fs::copy(
        repo_root().join("fixtures/coaster.svg"),
        dir.join("coaster.svg"),
    )
    .expect("svg copied");
    run(dir, &["init", "coaster.svg"]);
    let dcv = dir.join("coaster.dcv");
    let mut project = dc_project::load(&std::fs::read(&dcv).expect("dcv readable"))
        .expect("dcv loads")
        .project;
    for tool in &mut project.tools {
        tool.feed_mm_min = Some(1500.0);
        tool.plunge_mm_min = Some(500.0);
        tool.rpm = Some(18000);
        tool.step_down_mm = Some(2.0);
    }
    project.stock.thickness_mm = Some(11.8);
    for op in &mut project.operations {
        if let dc_project::Operation::Profile(p) = op {
            p.depth_mm = Some(12.0);
            p.through_cut_allowance_mm = 0.2;
        }
    }
    std::fs::write(&dcv, dc_project::save(&project).expect("saves")).expect("dcv written");
    dcv
}

/// The one program the harness finished, as text.
fn app_program(harness: &mut Harness) -> String {
    harness.generate();
    let programs = harness.programs();
    assert_eq!(programs.len(), 1, "{:?}", harness.warnings());
    programs[0].program.text.clone()
}

/// Acceptance 78 (functional capability 69; #546): *Place tabs* on the
/// coaster's Profile saves the file `tabs place coaster.dcv <op>` saves,
/// and the program the app finishes is the one `export` writes for it.
/// Then a tab dragged along the outline, one deleted and one added with
/// the Tab tool: the CLI exports the file the app saves to the program
/// the app shows, byte for byte.
#[test]
fn tabbed_coaster_exports_the_same_program() {
    let dir = work_dir("tabbed_coaster_exports_the_same_program");
    let dcv = ready_coaster(&dir);
    let before = std::fs::read(&dcv).expect("dcv readable");

    let mut harness = Harness::open(&before).expect("the project opens");
    let op = harness.app.project().operations[0].id().clone();
    run(&dir, &["tabs", "place", "coaster.dcv", op.as_str()]);
    harness.app.place_tabs(&op).expect("the request places");
    assert_eq!(
        harness.save().expect("saves"),
        std::fs::read_to_string(&dcv).expect("dcv readable"),
        "the file `tabs place` saves"
    );
    run(&dir, &["export", "coaster.dcv", "--date", DATE]);
    let cli = std::fs::read_to_string(dir.join("coaster.nc")).expect("program written");
    assert!(
        app_program(&mut harness) == cli,
        "placed: the program differs"
    );

    // Drag the first tab an eighth of the way round, delete the second,
    // add one with the Tab tool's last-used size.
    let project = harness.app.project().clone();
    let first = project.tabs[0].clone();
    let instance = &project.instances[0];
    let outline = &project.definitions[&instance.definition].profiles[0];
    let (target, _) = outline
        .point_at(outline.length_mm() * 0.125)
        .expect("a point");
    let (x_mm, y_mm) =
        dc_app::viewport::tabs::tab_drag(&project, &first, instance.transform.apply(target));
    harness
        .run(dc_app::Command::MoveTab {
            id: first.id.clone(),
            x_mm,
            y_mm,
        })
        .expect("moved");
    harness
        .run(dc_app::Command::DeleteTabs(vec![
            project.tabs[1].id.clone(),
        ]))
        .expect("deleted");
    let (at_mm, _) = outline
        .point_at(outline.length_mm() * 0.7)
        .expect("a point");
    let add = dc_app::viewport::tabs::add_tab(
        harness.app.project(),
        &dc_app::viewport::tabs::OutlinePoint {
            instance: instance.id.clone(),
            profile: 0,
            at_mm,
        },
        harness.app.prefs().tab,
    );
    harness.run(add).expect("added");
    assert_eq!(harness.app.project().tabs.len(), 4);

    std::fs::write(dir.join("edited.dcv"), harness.save().expect("saves")).expect("written");
    run(&dir, &["export", "edited.dcv", "--date", DATE]);
    let cli = std::fs::read_to_string(dir.join("edited.nc")).expect("program written");
    assert!(
        app_program(&mut harness) == cli,
        "edited: the program differs"
    );
}

/// The six bundled post-processors (acceptance 83).
const POSTS: [&str; 6] = ["generic", "grbl", "grblhal", "linuxcnc", "mach3", "uccnc"];

/// The program splits the sheet offers, with the CLI flag of each
/// override: as the project says, one program per tool, one program with
/// `M6`.
const SPLITS: [(dc_job::ProgramSplit, Option<&str>); 3] = [
    (dc_job::ProgramSplit::ProjectDefault, None),
    (
        dc_job::ProgramSplit::ProgramPerTool,
        Some("--programs-per-tool"),
    ),
    (dc_job::ProgramSplit::OneProgram, Some("--one-program")),
];

/// Runs `dreamcarve` with `args` in `dir`; its exit code, stdout and
/// stderr.
fn run_any(dir: &Path, args: &[&str]) -> (Option<i32>, String, String) {
    let out: Output = Command::new(env!("CARGO_BIN_EXE_dreamcarve"))
        .current_dir(dir)
        .env("DREAMCARVE_CONFIG_DIR", config_dir(dir))
        .args(args)
        .output()
        .expect("binary runs");
    (
        out.status.code(),
        String::from_utf8(out.stdout).expect("utf-8 stdout"),
        String::from_utf8(out.stderr).expect("utf-8 stderr"),
    )
}

/// The files in `dir`, sorted, but the project and the config directory.
fn written(dir: &Path, project: &str) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("dir readable")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .filter(|n| n != project && n != "config")
        .collect();
    names.sort();
    names
}

/// One fixture on one post-processor: the app opens `bytes`, switches to
/// `post` with the Project panel's command (an undoable project change)
/// and saves; for each split the CLI exports that saved file with the
/// date pinned and the preview asked for, and the app's plan for the same
/// snapshot must give the same programs, byte for byte, the Viewport's
/// SVG dump the same preview, the same report, or the same refusal with
/// nothing written.
fn export_case(root: &Path, name: &str, bytes: &[u8], post: &str) -> usize {
    let mut harness = Harness::open(bytes).expect("the project opens");
    if harness.app.project().post_processor.id != post {
        harness
            .run(dc_app::Command::SwitchPost {
                id: post.to_owned(),
            })
            .expect("the post-processor switches");
    }
    harness.generate();
    let saved = harness.save().expect("saves");
    let project_file = format!("{name}.dcv");
    let preview = dc_app::export::preview_name(name);
    let mut compared = 0;
    for (split, flag) in SPLITS {
        let dir = root.join(format!("{name}-{post}-{}", flag.unwrap_or("default")));
        std::fs::create_dir_all(&dir).expect("case dir");
        std::fs::write(dir.join(&project_file), &saved).expect("project written");
        let mut args = vec![
            "export",
            project_file.as_str(),
            "--date",
            DATE,
            "--preview",
            preview.as_str(),
        ];
        args.extend(flag);
        let (code, stdout, stderr) = run_any(&dir, &args);

        let options = dc_app::export::Options {
            split,
            ..dc_app::export::Options::new(DATE, dc_job::EstimateConfig::default())
        };
        let ready = match dc_app::export::plan(&harness.app, &options) {
            dc_app::export::Plan::Ready(ready) => ready,
            dc_app::export::Plan::Waiting => panic!("{name} {post}: the plan waits"),
        };
        let case = format!(
            "{name} on {post}, {}",
            flag.unwrap_or("as the project says")
        );
        match ready.blocked() {
            None => {
                assert_eq!(code, Some(0), "{case}: the CLI failed: {stderr}");
                // The sheet writes the programs; the preview is the
                // Viewport's SVG dump (capability 77, acceptance 83).
                let dump = harness.app.scene().preview_svg(harness.app.project());
                let mut files = ready.files(name);
                files.push((preview.clone(), dump.as_str()));
                let mut expected: Vec<String> = files.iter().map(|(n, _)| n.clone()).collect();
                expected.sort();
                assert_eq!(written(&dir, &project_file), expected, "{case}: the files");
                for (file, text) in &files {
                    let cli = std::fs::read_to_string(dir.join(file)).expect("file written");
                    assert!(cli == *text, "{case}: {file} differs from the CLI's");
                    compared += 1;
                }
                let names = ready.names(name);
                let mut report = ready.report(&names, Some(&preview)).join("\n");
                report.push('\n');
                assert_eq!(
                    stdout, report,
                    "{case}: the result sheet is the CLI's report"
                );
                // Capability 77a (#625): the result shows the report in
                // three parts, every line the CLI printed in exactly one
                // (the preview line aside: the sheet writes none).
                let result = ready.result(&names, &dir, &dc_app::Strings::english());
                let parts = result.parts();
                for line in stdout.lines().filter(|l| !l.starts_with("Wrote preview ")) {
                    let line = line.trim();
                    let holding = parts
                        .iter()
                        .filter(|(_, texts)| texts.iter().any(|t| t == line))
                        .count();
                    assert_eq!(holding, 1, "{case}: `{line}` is in {holding} parts");
                }
            }
            Some(reason) => {
                assert_eq!(
                    code,
                    Some(1),
                    "{case}: the CLI exported what the app refuses"
                );
                assert!(
                    stderr.contains(&reason),
                    "{case}: the CLI's refusal names `{reason}`: {stderr}"
                );
                assert!(
                    written(&dir, &project_file).is_empty(),
                    "{case}: nothing is written"
                );
            }
        }
        // The program split override is never saved.
        assert_eq!(harness.save().expect("saves"), saved, "{case}");
    }
    compared
}

/// Acceptance 83 (S20): the coaster, plaque, sign and inlay fixture, each
/// on all six post-processors, as the project says and with each
/// program-split override, exported from the app equal what
/// `dreamcarve export --date` writes for the saved file, byte for byte,
/// and the Viewport's SVG dump equals its `--preview` file. The fixtures
/// are read where they live.
#[test]
fn app_export_equals_cli_export_on_six_posts() {
    let root = work_dir("app_export_equals_cli_export_on_six_posts");
    let coaster_dir = root.join("coaster-init");
    std::fs::create_dir_all(&coaster_dir).expect("dir");
    let coaster = std::fs::read(ready_coaster(&coaster_dir)).expect("coaster readable");
    let fixtures: Vec<(&str, Vec<u8>)> = vec![
        ("coaster", coaster),
        (
            "plaque",
            std::fs::read(repo_root().join("tests/fixtures/plaque.dcv")).expect("plaque"),
        ),
        (
            "sign",
            std::fs::read(repo_root().join("tests/fixtures/sign.dcv")).expect("sign"),
        ),
        (
            "dc",
            std::fs::read(repo_root().join("tests/fixtures/dc.dcv")).expect("inlay"),
        ),
    ];
    let compared: usize = std::thread::scope(|scope| {
        let handles: Vec<_> = fixtures
            .iter()
            .flat_map(|(name, bytes)| POSTS.iter().map(move |post| (*name, bytes, *post)))
            .map(|(name, bytes, post)| {
                let root = &root;
                scope.spawn(move || export_case(root, name, bytes, post))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("case passes"))
            .sum()
    });
    assert!(compared > 0, "some files were compared");
}

/// The operations the written programs cut, in the order the files run
/// (their names sort in run order): each `( Operation <n>: <name> )`
/// comment of each file in `dir`, the project file aside.
fn written_operations(dir: &Path, project: &str) -> Vec<String> {
    written(dir, project)
        .iter()
        .flat_map(|file| {
            std::fs::read_to_string(dir.join(file))
                .expect("program readable")
                .lines()
                .filter_map(|line| {
                    let (_, name) = line.strip_prefix("( Operation ")?.split_once(": ")?;
                    Some(name.trim_end_matches(" )").to_owned())
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Acceptance 74's two clauses about the files (capability 65): the
/// plaque's Bore moved below its Panel pocket in the app is the order of
/// the saved file's list and of the written programs, and an operation
/// unticked is `"enabled": false` in the saved file and in no written
/// program. The app saves, `dreamcarve export` writes the programs for
/// that file, and each is byte for byte what the app plans; the
/// operations the written files name, in run order, are the list's.
#[test]
fn reorder_and_untick_reach_the_saved_file_and_the_written_programs() {
    let root = work_dir("reorder_and_untick_reach_the_saved_file_and_the_written_programs");
    let plaque = std::fs::read(repo_root().join("tests/fixtures/plaque.dcv")).expect("plaque");
    let mut harness = Harness::open(&plaque).expect("the plaque opens");
    let names = |harness: &Harness| -> Vec<String> {
        harness
            .app
            .project()
            .operations
            .iter()
            .filter(|o| o.enabled())
            .map(|o| o.name().to_owned())
            .collect()
    };
    let export = |harness: &mut Harness, case: &str| -> (String, Vec<String>) {
        let dir = root.join(case);
        std::fs::create_dir_all(&dir).expect("case dir");
        let saved = harness.save().expect("saves");
        std::fs::write(dir.join("plaque.dcv"), &saved).expect("project written");
        run(&dir, &["export", "plaque.dcv", "--date", DATE]);
        harness.generate();
        let files = app_programs(harness, "plaque");
        let mut expected: Vec<String> = files.iter().map(|(n, _)| n.clone()).collect();
        expected.sort();
        assert_eq!(written(&dir, "plaque.dcv"), expected, "{case}: the files");
        for (name, text) in &files {
            let cli = std::fs::read_to_string(dir.join(name)).expect("file written");
            assert!(cli == *text, "{case}: {name} differs from the CLI's");
        }
        (saved, written_operations(&dir, "plaque.dcv"))
    };

    let (_, as_opened) = export(&mut harness, "as-opened");
    assert_eq!(
        as_opened,
        [
            "Screw holes",
            "Border and squiggle",
            "Panel",
            "Square recess",
            "Cut out"
        ]
    );
    assert_eq!(as_opened, names(&harness));

    // The Bore dragged below the Panel pocket.
    let bore = dc_project::OperationId::new("op3");
    let reorder = dc_app::operations::reorder(harness.app.project(), &bore, 2).expect("a move");
    harness.run(reorder).expect("reordered");
    let (saved, reordered) = export(&mut harness, "reordered");
    assert_eq!(
        reordered,
        [
            "Border and squiggle",
            "Panel",
            "Screw holes",
            "Square recess",
            "Cut out"
        ],
        "the written programs follow the list"
    );
    let file: serde_json::Value = serde_json::from_str(&saved).expect("the file is JSON");
    let listed: Vec<&str> = file["operations"]
        .as_array()
        .expect("operations")
        .iter()
        .filter_map(|o| o["name"].as_str())
        .collect();
    assert_eq!(listed, reordered, "and so does the saved file's list");

    // The Engrave unticked.
    harness
        .run(dc_app::Command::SetOperationEnabled {
            id: dc_project::OperationId::new("op5"),
            enabled: false,
        })
        .expect("unticked");
    let (saved, unticked) = export(&mut harness, "unticked");
    let file: serde_json::Value = serde_json::from_str(&saved).expect("the file is JSON");
    let engrave = file["operations"]
        .as_array()
        .expect("operations")
        .iter()
        .find(|o| o["id"] == "op5")
        .expect("the Engrave is still in the file");
    assert_eq!(engrave["enabled"], false, "the saved file says so");
    assert_eq!(
        unticked,
        ["Panel", "Screw holes", "Square recess", "Cut out"],
        "and no written program cuts it"
    );
}

/// Capability 68 for an Inlay's drawn boundary (phase 3's capability 49f;
/// the phase 4 audit's one functional gap; S20): the lettered pair given
/// its boundary in the app exports what the CLI exports for the project
/// the Inlay chapter makes by hand. The app opens `tests/fixtures/dc.dcv`,
/// the pair with a margin; the editor's Boundary row takes the frame, the
/// Band margin field is emptied and Outline rounding switched off, each
/// through the function the window calls for that row. The project the
/// app then holds is the committed `tests/fixtures/dc-boundary.dcv`
/// (which `quick_start.rs::inlay_boundary_chapter_runs` holds to the
/// chapter's hand edit), and every program the app plans, both boards',
/// is byte for byte the file `dreamcarve export --date` writes for that
/// committed project, as is the Viewport's SVG dump its `--preview`.
#[test]
fn inlay_boundary_set_in_the_app_equals_the_clis_project() {
    let dir = work_dir("inlay_boundary_set_in_the_app_equals_the_clis_project");
    let fixtures = repo_root().join("tests/fixtures");
    let mut harness =
        Harness::open(&std::fs::read(fixtures.join("dc.dcv")).expect("the pair")).expect("opens");
    let inlay = |harness: &Harness| harness.app.project().operations[0].clone();
    let row = |harness: &Harness, key: &str| {
        dc_app::editor::fields::rows(&inlay(harness))
            .into_iter()
            .find(|r| r.key == key)
            .unwrap_or_else(|| panic!("no row `{key}`"))
    };
    let edit = |harness: &mut Harness,
                command: Result<Option<dc_app::Command>, dc_app::CommandError>| {
        let command = command.expect("accepted").expect("a change");
        harness.run(command).expect("runs");
    };

    let boundary = row(&harness, "boundary");
    let offered = dc_app::editor::offered_boundaries(harness.app.project(), &inlay(&harness));
    let [(frame, name)] = &offered[..] else {
        panic!("the frame alone is offered: {offered:?}");
    };
    assert_eq!(name, "frame");
    let set = dc_app::editor::set_boundary(&inlay(&harness), boundary, std::slice::from_ref(frame));
    edit(&mut harness, set);
    let margin = row(&harness, "margin_mm");
    let cleared = dc_app::editor::commit_text(harness.app.project(), &inlay(&harness), margin, "");
    edit(&mut harness, cleared);
    let rounding = row(&harness, "outline_rounding_mm");
    let off = dc_app::editor::switch(harness.app.project(), &inlay(&harness), rounding, false);
    edit(&mut harness, off);

    // The project is the one the chapter's hand edit makes.
    let committed = std::fs::read(fixtures.join("dc-boundary.dcv")).expect("the fixture");
    let by_hand = dc_project::load(&committed).expect("it loads").project;
    assert_eq!(harness.app.project(), &by_hand);

    // The CLI exports the committed file; the app plans its own.
    std::fs::write(dir.join("dc-boundary.dcv"), &committed).expect("copied");
    let preview = dc_app::export::preview_name("dc-boundary");
    run(
        &dir,
        &[
            "export",
            "dc-boundary.dcv",
            "--date",
            DATE,
            "--preview",
            &preview,
        ],
    );
    harness.generate();
    let mut files = app_programs(&harness, "dc-boundary");
    let males = files.iter().filter(|(n, _)| n.contains("-male-")).count();
    let females = files.iter().filter(|(n, _)| n.contains("-female-")).count();
    assert!(males > 0 && females > 0, "both boards: {files:?}");
    files.push((
        preview,
        harness.app.scene().preview_svg(harness.app.project()),
    ));
    let mut expected: Vec<String> = files.iter().map(|(n, _)| n.clone()).collect();
    expected.sort();
    assert_eq!(written(&dir, "dc-boundary.dcv"), expected, "the files");
    for (name, text) in &files {
        let cli = std::fs::read_to_string(dir.join(name)).expect("file written");
        assert!(cli == *text, "{name} differs from the CLI's");
    }
}

/// Part `part` of the app quick start built by the chapter's steps
/// ([`app_steps::build`]) and generated, ready to export.
fn generated_part(text: &str, part: &str, lib: &Library) -> Harness {
    let mut harness = Harness {
        app: app_steps::build(text, part, lib),
    };
    harness.generate();
    harness
}

/// The programs the app would write for `harness`'s project under file
/// name `stem`, by name.
fn app_programs(harness: &Harness, stem: &str) -> Vec<(String, String)> {
    let options = dc_app::export::Options::new(DATE, dc_job::EstimateConfig::default());
    let ready = match dc_app::export::plan(&harness.app, &options) {
        dc_app::export::Plan::Ready(ready) => ready,
        dc_app::export::Plan::Waiting => panic!("{stem}: the plan waits"),
    };
    assert_eq!(
        ready.blocked(),
        None,
        "{stem}: export is refused: {:?}",
        harness.warnings()
    );
    ready
        .files(stem)
        .into_iter()
        .map(|(name, text)| (name, text.to_owned()))
        .collect()
}

/// Acceptance 90 (#550; functional spec, exit criteria): the coaster, the
/// plaque, the sign and the inlay pair, each built in the app from its
/// drawing by the steps of the quick start "From SVG to G-code in the
/// app", export the quick starts' committed programs byte for byte. The
/// steps are read from the chapter's own tables (`app_steps`), so the
/// chapter and this test cannot drift apart, and every value goes through
/// the function the window calls for that field.
///
/// The committed programs of the plaque, the sign and the inlay pair are
/// what `dreamcarve export` writes for the committed projects their
/// quick starts build (`tests/fixtures/plaque.dcv`, `sign.dcv`,
/// `dc.dcv`; `quick_start.rs` holds each chapter to its fixture). The
/// coaster's is `tests/expected/coaster-grblhal.nc`.
///
/// One difference is the chapter's and stays: the CLI's coaster keeps
/// `init`'s placeholder names, a tool called `Set me` and an operation
/// called `Profile`, and the app has no placeholder, so its tool and
/// operation carry the names the reader gave them. The coaster's program
/// is therefore compared twice: as built, it is the committed program but
/// for those two comment lines; with the two renamed to the placeholder's
/// names, it is the committed program byte for byte.
#[test]
fn quick_start_parts_from_the_app() {
    let root = work_dir("quick_start_parts_from_the_app");
    let text = app_steps::chapter();
    let lib = app_steps::library(&text);

    // The coaster.
    let mut coaster = generated_part(&text, "coaster", &lib);
    let committed = include_str!("expected/coaster-grblhal.nc");
    let built = app_programs(&coaster, "coaster");
    let [(name, program)] = &built[..] else {
        panic!("the coaster is one program: {built:?}");
    };
    assert_eq!(name, "coaster.nc");
    let (op, tool) = {
        let project = coaster.app.project();
        (project.operations[0].clone(), project.tools[0].clone())
    };
    let named = committed
        .replace(
            "( Operation 1: Profile )",
            &format!("( Operation 1: {} )", op.name()),
        )
        .replace("( Tool 1: Set me )", &format!("( Tool 1: {} )", tool.name));
    assert_ne!(
        named, committed,
        "the committed coaster names its placeholder"
    );
    assert!(
        *program == named,
        "the coaster differs from the committed program in more than its two names:\n{program}"
    );
    let rename = dc_app::editor::rename(&op, "Profile")
        .expect("a name")
        .expect("a command");
    coaster.run(rename).expect("renamed");
    coaster
        .run(dc_app::Command::UpdateTool(dc_project::Tool {
            name: "Set me".to_owned(),
            ..tool
        }))
        .expect("the tool is renamed");
    coaster.generate();
    let renamed = app_programs(&coaster, "coaster");
    assert!(
        renamed[0].1 == committed,
        "the coaster with the placeholder's names differs from the committed program"
    );

    // The plaque, the sign and the inlay pair, against the CLI's export
    // of the committed project each quick start builds.
    for (part, fixture, stem) in [
        ("plaque", "plaque.dcv", "plaque"),
        ("sign", "sign.dcv", "sign"),
        ("inlay", "dc.dcv", "dc"),
    ] {
        let dir = root.join(part);
        std::fs::create_dir_all(&dir).expect("case dir");
        std::fs::copy(
            repo_root().join("tests/fixtures").join(fixture),
            dir.join(fixture),
        )
        .expect("fixture copied");
        run(&dir, &["export", fixture, "--date", DATE]);
        let harness = generated_part(&text, part, &lib);
        let built = app_programs(&harness, stem);
        let mut expected: Vec<String> = built.iter().map(|(name, _)| name.clone()).collect();
        expected.sort();
        assert_eq!(written(&dir, fixture), expected, "{part}: the programs");
        assert!(built.len() > 1, "{part}: more than one program");
        for (name, program) in &built {
            let cli = std::fs::read_to_string(dir.join(name)).expect("program written");
            if *program != cli {
                // Kept for a diff; the directory is cleared on the next run.
                let kept = dir.join(format!("{name}.app"));
                std::fs::write(&kept, program).expect("the app's program is kept");
                panic!(
                    "{part}: {name} differs from the committed project's program; the app's \
                     is {}",
                    kept.display()
                );
            }
        }
    }
}

/// The hook plate (phase 5 functional spec, "Test part"), drawn in the
/// app with no import by the steps of the quick start "Your first
/// design", read from the chapter's own tables (`design_steps`), and
/// generated.
fn hook_plate() -> Harness {
    let text = design_steps::chapter();
    let lib = app_steps::library(&text);
    let mut harness = Harness {
        app: design_steps::build(&text, &lib),
    };
    harness.generate();
    harness
}

/// Acceptance 128 (S21; exit criteria): the hook plate drawn in the app,
/// saved, and exported by `dreamcarve export` gives the programs the app
/// writes, byte for byte, on all six post-processors and with each
/// program-split override. Nothing of it was imported: the rectangle, the
/// slots, the circles, the hexagon and the text are shapes that keep
/// their sizes, cut by their saved outlines.
#[test]
fn the_hook_plate_drawn_equals_the_cli() {
    let root = work_dir("the_hook_plate_drawn_equals_the_cli");
    let harness = hook_plate();
    assert_eq!(harness.warnings(), [], "the part has nothing to report");
    let project = harness.app.project();
    // What the chapter says was drawn: seven kinds of thing, none
    // imported, each but the construction line keeping its sizes.
    let kept = project
        .definitions
        .values()
        .filter(|definition| definition.shape.is_some())
        .count();
    assert_eq!((project.definitions.len(), kept), (6, 5));
    assert_eq!(project.instances.len(), 10);
    assert!(project.bitmaps.is_empty());
    assert_eq!(project.operations.len(), 5);
    let saved = harness.save().expect("saves");
    let compared: usize = std::thread::scope(|scope| {
        let handles: Vec<_> = POSTS
            .iter()
            .map(|post| {
                let (root, saved) = (&root, &saved);
                scope.spawn(move || export_case(root, "hook-plate", saved.as_bytes(), post))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("case passes"))
            .sum()
    });
    assert!(compared >= POSTS.len(), "every post wrote a program");
}

/// `project`'s shapes that are cut, as an SVG a drawing program would
/// save: one path a shape, its outlines as they are placed, lines and
/// circular arcs, millimetres, Y down. Construction geometry is left
/// out, as it is from everything that is cut.
fn svg_of(project: &dc_project::Project) -> String {
    use std::fmt::Write as _;
    let (width, height) = (project.stock.width_mm, project.stock.height_mm);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}mm\" height=\"{height}mm\" \
         viewBox=\"0 0 {width} {height}\">\n"
    );
    for instance in project.instances.iter().filter(|i| !i.construction) {
        let definition = &project.definitions[&instance.definition];
        let mut d = String::new();
        for profile in &definition.profiles {
            let placed = instance
                .transform
                .apply_polyline(profile)
                .expect("a placed outline");
            for (at, segment) in placed.segments().enumerate() {
                if at == 0 {
                    let _ = write!(d, "M{:?} {:?} ", segment.start.x, height - segment.start.y);
                }
                let (x, y) = (segment.end.x, height - segment.end.y);
                match segment.arc() {
                    None => {
                        let _ = write!(d, "L{x:?} {y:?} ");
                    }
                    Some(arc) => {
                        let large = u8::from(arc.sweep_rad.abs() > std::f64::consts::PI);
                        // Anticlockwise with Y up is sweep 0 with Y down.
                        let sweep = u8::from(arc.sweep_rad < 0.0);
                        let r = arc.radius_mm;
                        let _ = write!(d, "A{r:?} {r:?} 0 {large} {sweep} {x:?} {y:?} ");
                    }
                }
            }
            if placed.is_closed() {
                d.push_str("Z ");
            }
        }
        let id = instance.name.replace(' ', "-");
        let _ = writeln!(
            svg,
            "  <path id=\"{id}\" fill=\"black\" d=\"{}\"/>",
            d.trim_end()
        );
    }
    svg.push_str("</svg>\n");
    svg
}

/// Acceptance 128, its second half (S21): the outlines of the hook plate
/// imported from an SVG of the part, with the same stock, tools and
/// operations, give the programs the drawn part gives: move for move,
/// every coordinate within its last decimal place ([`first_difference`]),
/// and nothing else different but the project's name in a comment; the
/// V-Carve of the text is the same carve with its strokes in an order of
/// its own ([`Cut`]). Then, exactly: the drawn part with its sizes taken
/// away gives the drawn part's programs byte for byte. A generator reads
/// a shape's saved outline and nothing else of it, so what was drawn and
/// what was imported cut alike.
#[test]
fn drawn_equals_imported() {
    use dc_app::import::{DrawingFormat, DrawingImport, Purpose};
    let text = design_steps::chapter();
    let lib = app_steps::library(&text);
    let drawn = hook_plate();
    let svg = svg_of(drawn.app.project());

    let sheet = DrawingImport::open(
        DrawingFormat::Svg,
        "hook-plate.svg",
        svg.clone().into_bytes(),
        Purpose::New,
    );
    assert_eq!(sheet.blocked(), None, "the SVG imports");
    let mut app = dc_app::App::new(
        dc_app::App::new_project().expect("a new project"),
        dc_app::Strings::english(),
        dc_app::Prefs::default(),
    );
    app.new_from_drawing(&sheet)
        .expect("a project from the SVG");
    // The same board: the import's own stock is the part and 10 mm round
    // it, which is the stock the chapter sets.
    app_steps::set_stock(&mut app, &design_steps::part_cell(&text, "Stock"));
    assert_eq!(app.project().stock, drawn.app.project().stock);
    // The shapes, named as the drawn ones are, so the operations' table
    // finds them; none keeps sizes, since an import has none to keep.
    let cut: Vec<&dc_project::FeatureInstance> = drawn
        .app
        .project()
        .instances
        .iter()
        .filter(|i| !i.construction)
        .collect();
    let imported: Vec<dc_project::InstanceId> = app
        .project()
        .instances
        .iter()
        .map(|i| i.id.clone())
        .collect();
    assert_eq!(imported.len(), cut.len(), "a shape for a path");
    for (id, like) in imported.iter().zip(&cut) {
        app.run(dc_app::Command::RenameInstance {
            id: id.clone(),
            name: like.name.clone(),
        })
        .expect("renamed");
    }
    assert!(
        app.project()
            .definitions
            .values()
            .all(|d| d.shape.is_none()),
        "an imported outline keeps no sizes"
    );
    design_steps::finish(&mut app, &text, &lib);
    let mut imported = Harness { app };
    imported.generate();

    let drawn_programs = app_programs(&drawn, "hook-plate");
    let imported_programs = app_programs(&imported, "hook-plate");
    assert_eq!(drawn_programs.len(), imported_programs.len());
    let root = work_dir("drawn_equals_imported");
    // The names in comments are the projects' own: one is named by its
    // file, the other has no file yet.
    let named = |program: &str, project: &dc_project::Project| {
        program.replace(&format!("( Project: {} )", project.name), "( Project )")
    };
    let (mut moves, mut lettering) = (0, 0);
    for ((name, drawn_program), (other, imported_program)) in
        drawn_programs.iter().zip(&imported_programs)
    {
        assert_eq!(name, other);
        let drawn = named(drawn_program, drawn.app.project());
        let imported = named(imported_program, imported.app.project());
        assert_ne!(&drawn, drawn_program, "{name} names its project");
        let keep = || {
            std::fs::write(root.join(format!("{name}.drawn")), &drawn).expect("kept");
            std::fs::write(root.join(format!("{name}.imported")), &imported).expect("kept");
            std::fs::write(root.join("hook-plate.svg"), &svg).expect("kept");
            root.display()
        };
        if drawn.contains(": Lettering )") {
            // The V-Carve: the same carve, its strokes in an order of
            // its own ([`Cut`]).
            let (a, b) = (Cut::of(&drawn), Cut::of(&imported));
            lettering += 1;
            assert!(
                a.same_as(&b),
                "{name}: the imported lettering is another carve: {a:?} and {b:?}; both \
                 are in {}",
                keep()
            );
        } else if let Some(line) = first_difference(&drawn, &imported) {
            panic!(
                "{name}, line {line}: the imported part's program differs from the drawn \
                 part's; both are in {}",
                keep()
            );
        }
        moves += drawn.lines().count();
    }
    assert!(moves > 500, "the programs were compared: {moves} lines");
    assert_eq!(lettering, 1, "one program carves the lettering");

    // And exactly: the drawn part with every shape's sizes taken away,
    // which leaves the outlines an import would have made of them, gives
    // the drawn part's programs byte for byte. Nothing a generator reads
    // was among the sizes (S21, ADR-0006).
    let mut plain = drawn.app.project().clone();
    for definition in plain.definitions.values_mut() {
        definition.shape = None;
    }
    let mut plain = Harness::from_project(plain);
    plain.generate();
    assert!(
        app_programs(&plain, "hook-plate") == drawn_programs,
        "a shape's sizes reached a program"
    );
}

/// What a program cuts, whatever order it cuts it in: how many lines it
/// has, the box its feed moves stay in, and how far they travel.
///
/// A V-Carve follows the middle lines of each letter, and where two
/// strokes are as near to hand as each other it takes one first. The
/// millionth of a millimetre an import rounds a coordinate by is enough
/// to make it take the other, so the imported lettering is the same
/// strokes in another order here and there, which no line-by-line
/// comparison shows. This does: the same count of moves, within the same
/// box, cutting the same length.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Cut {
    lines: usize,
    min: [f64; 3],
    max: [f64; 3],
    length_mm: f64,
}

impl Cut {
    /// Reads `program`: linear moves only, as a V-Carve's are.
    fn of(program: &str) -> Self {
        let mut at = [0.0_f64; 3];
        let mut feeding = false;
        let mut cut = Self {
            lines: program.lines().count(),
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
            length_mm: 0.0,
        };
        for line in program.lines().filter(|line| !line.starts_with('(')) {
            let from = at;
            let mut moved = false;
            for word in line.split_whitespace() {
                match word {
                    "G0" | "G00" => feeding = false,
                    "G1" | "G01" => feeding = true,
                    "G2" | "G02" | "G3" | "G03" => {
                        panic!("an arc in a program read as lines: {line}")
                    }
                    _ => {}
                }
                let (letter, value) = word.split_at(1);
                match (letter, value.parse::<f64>()) {
                    ("X", Ok(v)) => (at[0], moved) = (v, true),
                    ("Y", Ok(v)) => (at[1], moved) = (v, true),
                    ("Z", Ok(v)) => (at[2], moved) = (v, true),
                    _ => {}
                }
            }
            if feeding && moved {
                let squares: f64 = at.iter().zip(from).map(|(a, b)| (a - b).powi(2)).sum();
                cut.length_mm += squares.sqrt();
                for (axis, value) in at.iter().enumerate() {
                    cut.min[axis] = cut.min[axis].min(*value);
                    cut.max[axis] = cut.max[axis].max(*value);
                }
            }
        }
        cut
    }

    /// Whether `other` is the same carve: as many lines, the same box to
    /// two units of a coordinate's last place, the same length of cut to
    /// a tenth of a millimetre.
    fn same_as(&self, other: &Self) -> bool {
        let near =
            |a: &[f64; 3], b: &[f64; 3]| a.iter().zip(b).all(|(a, b)| (a - b).abs() <= 0.002);
        self.lines == other.lines
            && near(&self.min, &other.min)
            && near(&self.max, &other.max)
            && (self.length_mm - other.length_mm).abs() <= 0.1
    }
}

/// What the app would write for `harness`'s project with `split`: each
/// program by name, or why the export is refused (one program with tool
/// changes, on a post-processor with no tool change).
fn written_with(
    harness: &Harness,
    stem: &str,
    split: dc_job::ProgramSplit,
) -> Result<Vec<(String, String)>, String> {
    let mut options = dc_app::export::Options::new(DATE, dc_job::EstimateConfig::default());
    options.split = split;
    let ready = match dc_app::export::plan(&harness.app, &options) {
        dc_app::export::Plan::Ready(ready) => ready,
        dc_app::export::Plan::Waiting => panic!("{stem}: the plan waits"),
    };
    match ready.blocked() {
        Some(why) => Err(why),
        None => Ok(ready
            .files(stem)
            .into_iter()
            .map(|(name, text)| (name, text.to_owned()))
            .collect()),
    }
}

/// Acceptance 156 (phase 6 functional spec, capability 124; S22): the
/// simulation is a reader. On each of the six post-processors and with
/// each program split, with the simulation built for each board, with a
/// build stopped, and with the step never opened, the programs the app
/// writes (or its refusal to write them) and the project it saves are
/// byte for byte the same; the project is not marked changed, no warning
/// comes or goes, and the saved file has nothing of a simulation in it.
#[test]
fn simulating_changes_no_program() {
    use dc_app::sim::{self, SimResult, SimState};
    use dc_project::Board;

    let opened = |bytes: &[u8], post: &str| {
        let mut harness = Harness::open(bytes).expect("the project opens");
        if harness.app.project().post_processor.id != post {
            harness
                .run(dc_app::Command::SwitchPost {
                    id: post.to_owned(),
                })
                .expect("the post-processor switches");
        }
        harness.generate();
        harness
    };
    let every_split = |harness: &Harness, stem: &str| {
        SPLITS
            .iter()
            .map(|(split, _)| written_with(harness, stem, *split))
            .collect::<Vec<_>>()
    };
    let mut written = 0;
    for name in ["plaque.dcv", "sign.dcv", "dc.dcv"] {
        let bytes = std::fs::read(repo_root().join("tests/fixtures").join(name)).unwrap();
        let stem = name.trim_end_matches(".dcv");
        for post in POSTS {
            let case = format!("{name} on {post}");
            let never = opened(&bytes, post);
            let programs = every_split(&never, stem);
            written += programs.iter().flatten().count();
            let saved = never.save().unwrap();
            assert!(
                !saved.contains("simul"),
                "{case}: the file holds nothing of a simulation"
            );

            // Built, for the stock and for the male board where there
            // is one.
            let mut built = opened(&bytes, post);
            let mut made = 0;
            for board in [Board::Female, Board::Male] {
                built.app.set_sim_board(board);
                let Some((job, runs)) = built.app.next_sim_job() else {
                    continue;
                };
                let result = sim::execute(
                    built.app.project(),
                    &runs,
                    &job,
                    &dc_job::EstimateConfig::default(),
                    &mut |_| true,
                );
                assert!(matches!(result, SimResult::Built(_)), "{case} {board:?}");
                assert!(built.app.accept_sim(result));
                assert_eq!(built.app.sim_state(), SimState::Current);
                made += 1;
            }
            assert!(made >= 1, "{case}: the stock is simulated");
            assert_eq!(every_split(&built, stem), programs, "{case}: built");
            assert_eq!(built.save().unwrap(), saved, "{case}: built");
            assert_eq!(
                built.app.is_dirty(),
                never.app.is_dirty(),
                "{case}: a simulation is no change"
            );
            assert_eq!(built.warnings(), never.warnings(), "{case}: nor a warning");

            // A build stopped part-way.
            let mut stopped = opened(&bytes, post);
            let (job, _) = stopped.app.next_sim_job().expect("a job is due");
            stopped.app.stop_sim();
            stopped.app.accept_sim(SimResult::Stopped {
                snapshot: job.snapshot,
            });
            assert_eq!(stopped.app.sim_state(), SimState::Stopped);
            assert_eq!(every_split(&stopped, stem), programs, "{case}: stopped");
            assert_eq!(stopped.save().unwrap(), saved, "{case}: stopped");
        }
    }
    assert!(
        written >= 3 * POSTS.len(),
        "programs were compared: {written}"
    );
}

/// The first line, from 1, at which programs `a` and `b` differ by more
/// than the last decimal place of a coordinate; `None` when they are the
/// same program. An import rounds a drawing's coordinates to a millionth
/// of a millimetre, so a coordinate that falls on a rounding tie can be
/// written one unit of its last place apart (`Y34.608`, `Y34.607`). A
/// V-Carve's feed is worked out from the lengths of moves a few
/// hundredths of a millimetre long, where that millionth shows as a few
/// thousandths of a millimetre a minute (`F465.016`, `F465.020`); a
/// hundredth is allowed. Every word that is not a number, and every
/// number's letter, must be the same.
fn first_difference(a: &str, b: &str) -> Option<usize> {
    /// One unit in the last place of a millimetre coordinate, and a hair.
    const LAST_PLACE: f64 = 0.001_000_1;
    /// What a feed may differ by, mm/min.
    const FEED: f64 = 0.01;
    let same_word = |a: &str, b: &str| {
        if a == b {
            return true;
        }
        let (Some(letter_a), Some(letter_b)) = (a.chars().next(), b.chars().next()) else {
            return false;
        };
        let numbers = (
            a[letter_a.len_utf8()..].parse::<f64>(),
            b[letter_b.len_utf8()..].parse::<f64>(),
        );
        match numbers {
            (Ok(x), Ok(y)) => {
                let allowed = if letter_a == 'F' { FEED } else { LAST_PLACE };
                letter_a == letter_b && (x - y).abs() <= allowed
            }
            _ => false,
        }
    };
    let (mut lines_a, mut lines_b) = (a.lines(), b.lines());
    for line in 1.. {
        match (lines_a.next(), lines_b.next()) {
            (None, None) => return None,
            (Some(a), Some(b)) => {
                let (words_a, words_b): (Vec<&str>, Vec<&str>) = (
                    a.split_whitespace().collect(),
                    b.split_whitespace().collect(),
                );
                let same = words_a.len() == words_b.len()
                    && words_a.iter().zip(&words_b).all(|(a, b)| same_word(a, b));
                if !same {
                    return Some(line);
                }
            }
            _ => return Some(line),
        }
    }
    None
}
