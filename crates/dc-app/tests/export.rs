// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 18 of phase 4 (`docs/specs/phase-4/design.md`, "Architecture →
//! Export" and "Test plan → Export"; #547): `dc_app::export::plan` waits
//! for generation, then builds the snapshot afresh with `dc_job`'s own
//! entry points, so the programs the export sheet lists and writes are
//! the CLI's (functional capability 77, S20). The byte comparison with
//! the `dreamcarve` binary itself is `crates/dreamcarve/tests/gui_equality.rs`.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::Command;
use dc_app::export::{self, Boards, Options, Plan, Ready};
use dc_app::test_support::{DATE, Harness};
use dc_job::summary::before_you_start;
use dc_job::{Built, EstimateConfig, ProgramSplit, Tier};
use dc_project::{Board, Project, ToolChangeMode};

use common::{fixture, plaque_bytes, read, repo_root};

/// The export options with the harness's date and the default estimate
/// settings.
fn options() -> Options {
    Options::new(DATE, EstimateConfig::default())
}

/// The plan, which must be ready.
fn ready(harness: &Harness, options: &Options) -> Ready {
    match export::plan(&harness.app, options) {
        Plan::Ready(ready) => *ready,
        Plan::Waiting => panic!("the plan waits after generation finished"),
    }
}

/// An app on `bytes`, every operation generated.
fn generated(bytes: &[u8]) -> Harness {
    let mut harness = Harness::open(bytes).expect("the fixture opens");
    harness.generate();
    harness
}

/// What the CLI builds for `project`: `dc_job::build_board` with the same
/// arguments `export` passes.
fn cli_build(project: &Project, split: ProgramSplit, board: Option<Board>) -> Built {
    dc_job::build_board(project, split, DATE, board)
}

/// Export waits while any operation is pending, generating or stale, and
/// is ready once every result is in; an edit makes it wait again, and so
/// does Stop, which leaves stale operations held.
#[test]
fn plan_waits_for_generation() {
    let mut harness = Harness::open(&plaque_bytes()).unwrap();
    assert!(!harness.app.pending().is_empty());
    assert_eq!(export::plan(&harness.app, &options()), Plan::Waiting);
    // Half done: one operation in, the rest still pending.
    let work = harness.app.next_work().unwrap();
    let result = dc_app::generation::execute(
        harness.app.project(),
        work,
        DATE,
        &EstimateConfig::default(),
    );
    harness.app.accept(result);
    assert_eq!(export::plan(&harness.app, &options()), Plan::Waiting);

    harness.generate();
    assert!(matches!(
        export::plan(&harness.app, &options()),
        Plan::Ready(_)
    ));

    let mut stock = harness.app.project().stock.clone();
    stock.width_mm += 5.0;
    harness.run(Command::SetStock(stock)).unwrap();
    assert_eq!(
        export::plan(&harness.app, &options()),
        Plan::Waiting,
        "every toolpath is stale after the stock changed"
    );
    harness.app.stop();
    assert!(harness.app.pending().is_empty());
    assert_eq!(
        export::plan(&harness.app, &options()),
        Plan::Waiting,
        "Stop leaves the toolpaths stale: export still waits"
    );
    harness.app.regenerate_all();
    harness.generate();
    assert!(matches!(
        export::plan(&harness.app, &options()),
        Plan::Ready(_)
    ));
}

/// The rows are the CLI's programs, built afresh: the same text, the
/// names `export` gives them under the chosen stem, the operations the
/// incremental `finish` lists, and the step before each as the CLI's
/// report says it. The plaque, the sign and the inlay fixture.
#[test]
fn plan_lists_programs_as_the_cli_prints_them() {
    for name in ["plaque", "sign", "dc"] {
        let harness = generated(&read(&fixture(&format!("{name}.dcv"))));
        let ready = ready(&harness, &options());
        let project = harness.app.project();
        let built = cli_build(project, ProgramSplit::ProjectDefault, None);
        assert!(built.findings.errors.is_empty(), "{name}: {built:?}");
        let texts: Vec<&str> = ready
            .programs
            .iter()
            .map(|p| p.program.text.as_str())
            .collect();
        let cli: Vec<&str> = built.programs.iter().map(|p| p.text.as_str()).collect();
        assert!(texts == cli, "{name}: the programs differ from build's");

        let stem = format!("{name}-out");
        assert_eq!(
            ready.names(&stem),
            dc_job::program_names(project, &stem, &built.programs),
            "{name}: the names"
        );
        assert_eq!(ready.by_board, name == "dc", "{name}: grouped by board");

        // The incremental results agree on what each program cuts.
        let incremental = harness.programs();
        assert_eq!(incremental.len(), ready.programs.len(), "{name}");
        for (row, finished) in ready.programs.iter().zip(incremental) {
            assert_eq!(row.operations, finished.operations, "{name}: {}", row.name);
            assert_eq!(row.estimate_line, finished.estimate_line, "{name}");
        }
        // The step before each, as `export` prints it.
        let mut k_in_board = 0;
        let mut board = None;
        let single = built.programs.len() == 1 && !ready.by_board;
        for (row, program) in ready.programs.iter().zip(&built.programs) {
            if board != Some(program.board) {
                board = Some(program.board);
                k_in_board = 0;
            }
            assert_eq!(row.before, before_you_start(program, k_in_board, single));
            k_in_board += 1;
        }
        assert!(ready.blocked().is_none(), "{name}: nothing blocks");
    }
}

/// Acceptance 76 and 85: with the inlay fixture's female board unticked,
/// the plan lists the male board's programs alone, as `export --board
/// male` writes them, and nothing in the project changes; unticking both
/// leaves nothing to export.
#[test]
fn board_tick_filters_without_saving() {
    let bytes = read(&fixture("dc.dcv"));
    let harness = generated(&bytes);
    let before = harness.save().unwrap();
    let undo = harness.app.undo_label().map(str::to_owned);

    for (boards, board) in [
        (
            Boards {
                female: false,
                male: true,
            },
            Board::Male,
        ),
        (
            Boards {
                female: true,
                male: false,
            },
            Board::Female,
        ),
    ] {
        let options = Options {
            boards,
            ..options()
        };
        let ready = ready(&harness, &options);
        let built = cli_build(
            harness.app.project(),
            ProgramSplit::ProjectDefault,
            Some(board),
        );
        assert!(!ready.programs.is_empty(), "{board:?}");
        assert!(ready.programs.iter().all(|p| p.program.board == board));
        let texts: Vec<&str> = ready
            .programs
            .iter()
            .map(|p| p.program.text.as_str())
            .collect();
        let cli: Vec<&str> = built.programs.iter().map(|p| p.text.as_str()).collect();
        assert!(texts == cli, "{board:?}: the programs differ");
        let prefix = format!("dc-{}-", board.as_str());
        assert!(
            ready.names("dc").iter().all(|n| n.starts_with(&prefix)),
            "{:?}",
            ready.names("dc")
        );
    }

    let none = Options {
        boards: Boards {
            female: false,
            male: false,
        },
        ..options()
    };
    let ready = ready(&harness, &none);
    assert!(ready.programs.is_empty());
    assert!(
        ready.blocked().is_some(),
        "no board ticked: nothing to write"
    );

    assert_eq!(harness.save().unwrap(), before, "the project is unchanged");
    assert!(!harness.app.is_dirty());
    assert_eq!(harness.app.undo_label().map(str::to_owned), undo);
}

/// Acceptance 84: a tool without a feed is the CLI's error, which blocks
/// export; a project with warnings only (the version 14 plaque, whose
/// holes are too small for its drill) exports and lists them.
#[test]
fn errors_disable_warnings_do_not() {
    let mut harness = Harness::open(&plaque_bytes()).unwrap();
    // The first operation's tool: the plaque has one tool it never uses.
    let project = harness.app.project();
    let used = project.operations[0].run_tools()[0].clone();
    let mut tool = project.tools.iter().find(|t| t.id == used).unwrap().clone();
    tool.feed_mm_min = None;
    harness.run(Command::UpdateTool(tool)).unwrap();
    harness.generate();
    let blocked = ready(&harness, &options());
    let cli = cli_build(harness.app.project(), ProgramSplit::ProjectDefault, None);
    assert!(!cli.findings.errors.is_empty());
    assert_eq!(blocked.findings, cli.findings, "the CLI's findings");
    assert!(blocked.programs.is_empty());
    assert_eq!(
        blocked.blocked().as_deref(),
        Some(cli.findings.errors[0].message.as_str()),
        "Export is disabled with the first error"
    );

    let warned =
        repo_root().join("crates/dreamcarve/tests/fixtures/holds-slug-migration/plaque.dcv");
    let harness = generated(&read(&warned));
    let ready = ready(&harness, &options());
    assert!(ready.blocked().is_none(), "warnings do not block");
    assert!(!ready.programs.is_empty());
    assert!(
        ready
            .findings
            .warnings
            .iter()
            .any(|f| f.tier == Tier::Warning && f.code.as_deref() == Some("drill_hole_too_small")),
        "the warnings are listed: {:?}",
        ready.findings.warnings
    );
}

/// Acceptance 85: exporting the sign with *one program* overridden writes
/// one program with `M6` and leaves `tool_change_mode` as it was; on a
/// post-processor without `M6` the override is refused with the CLI's
/// message.
#[test]
fn override_not_saved() {
    let mut harness = generated(&read(&fixture("sign.dcv")));
    let before = harness.save().unwrap();
    let mode = harness.app.project().post_processor.tool_change_mode;
    assert_eq!(mode, ToolChangeMode::SeparatePrograms);

    let one = Options {
        split: ProgramSplit::OneProgram,
        ..options()
    };
    let ready = ready(&harness, &one);
    assert_eq!(ready.programs.len(), 1, "one program");
    assert!(ready.programs[0].program.text.contains("M6"));
    assert_eq!(harness.save().unwrap(), before, "the file is unchanged");
    assert_eq!(harness.app.project().post_processor.tool_change_mode, mode);

    harness
        .run(Command::SwitchPost {
            id: "grbl".to_owned(),
        })
        .unwrap();
    harness.generate();
    let refused = self::ready(&harness, &one);
    let cli = cli_build(harness.app.project(), ProgramSplit::OneProgram, None);
    assert!(refused.programs.is_empty());
    assert_eq!(refused.findings, cli.findings);
    assert_eq!(
        refused.blocked().as_deref(),
        Some(cli.findings.errors[0].message.as_str()),
        "refused with the CLI's message"
    );
}

/// Acceptance 83: the preview the plan writes is `--preview`'s picture of
/// the same build, and `<stem>-preview.svg` is its name.
#[test]
fn preview_svg_equals_the_cli() {
    for name in ["plaque", "sign", "dc"] {
        let harness = generated(&read(&fixture(&format!("{name}.dcv"))));
        let ready = ready(&harness, &options());
        let built = cli_build(harness.app.project(), ProgramSplit::ProjectDefault, None);
        let generated = built.generated.expect("every operation generated");
        let svg = dc_job::preview::render_generated(harness.app.project(), &generated);
        assert!(ready.preview.as_deref() == Some(svg.as_str()), "{name}");
        let files = ready.files(name, true);
        let (last_name, last_text) = files.last().unwrap();
        assert_eq!(last_name, &format!("{name}-preview.svg"));
        assert!(
            *last_text == svg.as_str(),
            "{name}: the preview is written last"
        );
        assert_eq!(files.len(), ready.programs.len() + 1);
        assert_eq!(ready.files(name, false).len(), ready.programs.len());
    }
}
