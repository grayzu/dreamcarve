// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 9 of phase 4 (`docs/specs/phase-4/design.md`, "Warnings"; #538):
//! the Warnings panel is `check` on the project as it stands (functional
//! capability 76, acceptance 82). The expected text is the CLI's own,
//! committed beside its tests: each `.warnings` file under
//! `crates/dreamcarve/tests/expected/` is what `dreamcarve check` prints as
//! its warning lines for the fixture of the same name, which
//! `crates/dreamcarve/tests/cli.rs` asserts on every run. The projects
//! are generated through the harness with the real `dc_job` functions.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::PathBuf;
use std::sync::OnceLock;

use dc_app::test_support::Harness;
use dc_app::warnings::{self, Level};
use dc_app::{Command, Selection};
use dc_job::Tier;
use dc_project::test_support::sample_project;
use dc_project::{Operation, OperationId};

use common::{read, repo_root};

/// A fixture with expected `check` output: its file name, the file's
/// bytes and the warning lines `check` prints for it.
struct Expected {
    file: String,
    bytes: Vec<u8>,
    lines: Vec<String>,
}

/// Every fixture under `crates/dreamcarve/tests/fixtures/` with expected
/// `check` output: the version 14 corpus (`holds-slug-migration`, 19
/// projects) and the version 18 Pocket and Bore project
/// (`stock-to-leave-migration`), each opened from its own bytes, so
/// `check`'s `file:line:column` positions are the file's.
fn expected() -> Vec<Expected> {
    let cli = repo_root().join("crates/dreamcarve/tests");
    let mut out = Vec::new();
    for corpus in ["holds-slug-migration", "stock-to-leave-migration"] {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(cli.join("fixtures").join(corpus))
            .expect("corpus readable")
            .map(|e| e.expect("entry").path())
            .filter(|p| p.extension().is_some_and(|x| x == "dcv"))
            .collect();
        paths.sort();
        for path in paths {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            let warnings = cli
                .join("expected")
                .join(corpus)
                .join(format!("{stem}.warnings"));
            let text = String::from_utf8(read(&warnings)).unwrap();
            out.push(Expected {
                file: format!("{stem}.dcv"),
                bytes: read(&path),
                lines: text.lines().map(str::to_owned).collect(),
            });
        }
    }
    assert_eq!(out.len(), 20, "the fixtures with expected check output");
    out
}

/// `bytes` opened in the app as the file `file`, generated to the end.
fn generated(file: &str, bytes: &[u8]) -> Harness {
    let mut harness = Harness::from_project(sample_project());
    harness
        .app
        .open(bytes, Some(PathBuf::from(file)))
        .unwrap_or_else(|e| panic!("{file}: {e}"));
    harness.generate();
    assert!(harness.app.pending().is_empty(), "{file}");
    assert!(!harness.app.generation().busy(), "{file}: generation done");
    harness
}

/// Each fixture with expected output, generated once for every test in
/// this file.
fn corpus() -> &'static [(Expected, Harness)] {
    static CORPUS: OnceLock<Vec<(Expected, Harness)>> = OnceLock::new();
    CORPUS.get_or_init(|| {
        expected()
            .into_iter()
            .map(|e| {
                let harness = generated(&e.file, &e.bytes);
                (e, harness)
            })
            .collect()
    })
}

/// The lines of `text` `check` labels a warning.
fn warning_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.contains("warning: "))
        .map(str::to_owned)
        .collect()
}

/// Acceptance 82: for each fixture with expected `check` output, the panel
/// lists exactly `check`'s lines, each row reading as `check` prints it
/// (`plaque.dcv:22:5: warning: unused_tool: …`). The panel groups them,
/// project-level rows first, then each operation's in the project's
/// order (capability 76), and within a group keeps `check`'s order; so
/// the panel is `check`'s list, regrouped and nothing else. The Pocket
/// and Bore project shows the grouping: its `tool_change_avoidable`
/// hints, which `check` prints after `op4`'s `lead_shrunk`, sit with
/// `unused_tool` in the project's group above it.
#[test]
fn panel_equals_check_line_for_line() {
    for (expected, harness) in corpus() {
        let file = &expected.file;
        let groups = warnings::panel(&harness.app);
        let shown: Vec<&warnings::Row> = groups.iter().flat_map(|g| &g.rows).collect();
        let mut unmatched: Vec<Option<&String>> = expected.lines.iter().map(Some).collect();
        for group in &groups {
            let mut last = None;
            for row in &group.rows {
                let at = unmatched
                    .iter()
                    .position(|l| l.is_some_and(|l| *l == row.line))
                    .unwrap_or_else(|| {
                        panic!(
                            "{file}: the panel shows a line check does not: {}",
                            row.line
                        )
                    });
                unmatched[at] = None;
                assert!(
                    last.is_none_or(|last| last < at),
                    "{file}: `{}` is out of check's order within its group",
                    row.line
                );
                last = Some(at);
            }
        }
        let missing: Vec<&String> = unmatched.into_iter().flatten().collect();
        assert!(missing.is_empty(), "{file}: check prints {missing:?}");
        assert_eq!(shown.len(), expected.lines.len(), "{file}");

        // Project-level first, then the operations in the project's order.
        let project = harness.app.project();
        let order: Vec<Option<&OperationId>> = groups.iter().map(|g| g.op.as_ref()).collect();
        let mut sorted = order.clone();
        sorted.sort_by_key(|op| {
            op.map_or(0, |op| {
                1 + project
                    .operations
                    .iter()
                    .position(|o| o.id() == op)
                    .unwrap_or(project.operations.len())
            })
        });
        assert_eq!(order, sorted, "{file}: groups out of order");
        for group in &groups {
            assert!(!group.rows.is_empty(), "{file}: an empty group");
            for row in &group.rows {
                assert_eq!(row.op.as_ref(), group.op.as_ref(), "{file}: {}", row.line);
            }
        }
    }

    let (_, pocket_bore) = corpus()
        .iter()
        .find(|(e, _)| e.file == "pocket-bore.dcv")
        .unwrap();
    let groups = warnings::panel(&pocket_bore.app);
    let codes: Vec<(Option<String>, Vec<&str>)> = groups
        .iter()
        .map(|g| {
            (
                g.op.as_ref().map(ToString::to_string),
                g.rows.iter().filter_map(|r| r.code.as_deref()).collect(),
            )
        })
        .collect();
    assert_eq!(
        codes,
        [
            (
                None,
                vec![
                    "unused_tool",
                    "tool_change_avoidable",
                    "tool_change_avoidable"
                ]
            ),
            (Some("op4".to_owned()), vec!["lead_shrunk"]),
        ]
    );
}

/// Capability 76: a row about one value of the file names that field,
/// as validation names it, and its line is located in the file as
/// `check` locates it; clicking it selects what it is about. On the
/// sample with its Profile's depth unset: the error is in the Profile's
/// group, names `operations[o1].depth_mm`, reads `sample.dcv:L:C: error:
/// …` at that value's line and column, and selects the Profile. A row
/// about a shape and no field (the plaque Drill's holes too small for its
/// bit) selects the shape. Once the project is edited the file on disk is
/// no longer what is checked, so lines carry no position.
#[test]
fn located_rows_name_the_field() {
    let mut project = sample_project();
    let Some(Operation::Profile(profile)) = project.operations.first_mut() else {
        panic!("the sample starts with its Profile");
    };
    profile.depth_mm = None;
    let text = dc_project::save(&project).unwrap();
    let harness = generated("sample.dcv", text.as_bytes());
    let o1 = OperationId::new("o1");
    let groups = warnings::panel(&harness.app);
    let group = groups
        .iter()
        .find(|g| g.op.as_ref() == Some(&o1))
        .expect("the Profile's group");
    let row = group
        .rows
        .iter()
        .find(|r| r.level == Level::Error)
        .expect("the unset depth");
    assert_eq!(row.field.as_deref(), Some("operations[o1].depth_mm"));
    let map = dc_project::SourceMap::parse(&text).unwrap();
    let at = map.locate("operations[o1].depth_mm").unwrap();
    assert!(
        row.line.starts_with(&format!("sample.dcv:{at}: error: ")),
        "{}",
        row.line
    );
    assert_eq!(row.selection(), Some(Selection::Operation(o1.clone())));
    for row in groups.iter().flat_map(|g| &g.rows) {
        if let Some(field) = &row.field {
            let finding = harness
                .app
                .warnings()
                .iter()
                .find(|f| f.message == row.shown)
                .expect("each row is a finding");
            assert_eq!(finding.field.as_ref(), Some(field), "{}", row.line);
        }
    }

    // A shape row with no field selects the shape.
    let (_, plaque) = corpus()
        .iter()
        .find(|(e, _)| e.file == "plaque.dcv")
        .unwrap();
    let hole = warnings::panel(&plaque.app)
        .into_iter()
        .flat_map(|g| g.rows)
        .find(|r| r.code.as_deref() == Some("drill_hole_too_small"))
        .expect("the plaque's small holes");
    let shape = hole.shape.clone().expect("about one hole");
    assert!(hole.field.is_none());
    assert_eq!(
        hole.selection(),
        Some(Selection::Instances(vec![shape.instance]))
    );
    // `unused_tool` is located at the tool, as `check` locates it.
    let unused = warnings::panel(&plaque.app)
        .into_iter()
        .flat_map(|g| g.rows)
        .find(|r| r.code.as_deref() == Some("unused_tool"))
        .unwrap();
    assert!(
        unused
            .field
            .as_deref()
            .is_some_and(|f| f.starts_with("tools["))
    );
    assert!(unused.line.starts_with("plaque.dcv:22:5: warning: "));

    // Edited: no positions, the same messages.
    let mut edited = harness;
    let mut stock = edited.app.project().stock.clone();
    stock.width_mm += 10.0;
    edited.run(Command::SetStock(stock)).unwrap();
    edited.generate();
    let lines = warnings::lines(&edited.app);
    assert!(lines.iter().any(|l| l.starts_with("error: ")), "{lines:?}");
    assert!(
        !lines.iter().any(|l| l.starts_with("sample.dcv:")),
        "{lines:?}"
    );
}

/// Capability 76: *Copy* puts the panel's text on the clipboard in
/// `check`'s format: the migration notes, the errors, the warnings, then
/// `check`'s count line, `plaque.dcv: 0 error(s), 6 warning(s)`. Its
/// warning lines are each fixture's expected `check` lines, in order.
#[test]
fn copy_is_the_cli_text() {
    for (expected, harness) in corpus() {
        let file = &expected.file;
        let text = warnings::copy_text(&harness.app);
        assert!(text.ends_with('\n'), "{file}");
        assert_eq!(warning_lines(&text), expected.lines, "{file}");
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines.last().copied(),
            Some(format!("{file}: 0 error(s), {} warning(s)", expected.lines.len()).as_str()),
            "{file}"
        );
        // The notes come first, as `check` prints them.
        let notes = dc_project::load(&expected.bytes).unwrap().notes;
        for (line, note) in lines.iter().zip(&notes) {
            assert_eq!(*line, format!("{file}: note: {note}"), "{file}");
        }
        assert_eq!(
            lines.len(),
            notes.len() + expected.lines.len() + 1,
            "{file}: {text}"
        );
        let panel: String = warnings::lines(&harness.app)
            .iter()
            .map(|l| format!("{l}\n"))
            .collect();
        assert_eq!(text, panel + lines.last().unwrap() + "\n", "{file}");
    }
}

/// Capability 76: three tiers as `check` prints them. Every line `check`
/// labels `warning:` is a warning or a hint, a hint exactly for
/// `tool_change_avoidable` and `unused_tool` (`dc_job::Tier`, after
/// PureCutCNC's `warningSeverity`); every `error:` line is an error; the
/// status bar's counts are `check`'s count line's. A project with an
/// unset tool feed shows the error tier.
#[test]
fn tiers_match_the_cli() {
    for (expected, harness) in corpus() {
        let file = &expected.file;
        let rows: Vec<warnings::Row> = warnings::panel(&harness.app)
            .into_iter()
            .flat_map(|g| g.rows)
            .collect();
        for row in &rows {
            let hint = matches!(
                row.code.as_deref(),
                Some("tool_change_avoidable" | "unused_tool")
            );
            let want = if hint { Level::Hint } else { Level::Warning };
            assert_eq!(row.level, want, "{file}: {}", row.line);
            assert!(row.line.contains(": warning: ") || row.line.starts_with("warning: "));
        }
        let counts = warnings::counts(&harness.app);
        assert_eq!(counts.errors, 0, "{file}");
        assert_eq!(
            counts.warnings + counts.hints,
            expected.lines.len(),
            "{file}"
        );
    }

    let mut project = sample_project();
    project.tools[0].feed_mm_min = None;
    let text = dc_project::save(&project).unwrap();
    let harness = generated("sample.dcv", text.as_bytes());
    let rows: Vec<warnings::Row> = warnings::panel(&harness.app)
        .into_iter()
        .flat_map(|g| g.rows)
        .collect();
    let errors: Vec<&warnings::Row> = rows.iter().filter(|r| r.level == Level::Error).collect();
    assert!(!errors.is_empty());
    for row in &errors {
        assert!(row.line.contains(": error: "), "{}", row.line);
        assert!(row.code.is_none(), "an error prints its message alone");
    }
    let counts = warnings::counts(&harness.app);
    assert_eq!(counts.errors, errors.len());
    let findings = harness.app.warnings();
    assert_eq!(
        findings.iter().filter(|f| f.tier == Tier::Error).count(),
        counts.errors
    );
    let text = warnings::copy_text(&harness.app);
    assert!(
        text.ends_with(&format!(
            "sample.dcv: {} error(s), {} warning(s)\n",
            counts.errors,
            counts.warnings + counts.hints
        )),
        "{text}"
    );
}

/// Capability 76 (#549, row 11): the drawer names a group only when there
/// are two or more, since a lone heading labels the only thing there. The
/// quick-start plaque's one finding is the project's unused tool: one
/// group, no heading. The version 14 plaque adds its Drill's findings: a
/// project row and an operation's rows, two groups, two headings.
#[test]
fn single_group_has_no_heading() {
    let mut one = Harness::open(&read(&repo_root().join("tests/fixtures/plaque.dcv")))
        .expect("the plaque opens");
    one.generate();
    let groups = warnings::panel(&one.app);
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(groups[0].op, None, "the project's");
    assert!(
        groups.iter().all(|g| !g.heading),
        "one group has no heading"
    );

    let path = repo_root().join("crates/dreamcarve/tests/fixtures/holds-slug-migration/plaque.dcv");
    let two = generated("plaque.dcv", &read(&path));
    let groups = warnings::panel(&two.app);
    let ops: Vec<Option<OperationId>> = groups.iter().map(|g| g.op.clone()).collect();
    assert_eq!(
        ops,
        [None, Some(OperationId::new("op3"))],
        "the project's, then the Drill's"
    );
    assert!(groups.iter().all(|g| g.heading), "two groups, two headings");
}

/// `tests/fixtures/<name>.dcv` opened as `<name>.dcv` and generated.
fn quick_start(name: &str) -> Harness {
    let file = format!("{name}.dcv");
    generated(
        &file,
        &read(&repo_root().join("tests/fixtures").join(&file)),
    )
}

/// What `check` prints as its carve lines for the project `harness`
/// holds: `describe_carves` over a full build's runs.
fn check_carves(harness: &Harness) -> Vec<String> {
    let project = harness.app.project();
    let built = dc_job::build(project, dc_job::ProgramSplit::ProjectDefault, "check");
    dc_job::describe_carves(project, built.generated.as_deref().unwrap_or_default())
}

/// Functional 76a, acceptance 99 (slice 28, #620): on the sign, the
/// Lettering V-Carve's carve summary is a note, the last row of its
/// operation's group, laid out as labelled facts with *Deepest cut*
/// first; the notes are information, so the counts are what they are
/// without them: `check`'s count line's.
#[test]
fn notes_follow_their_operation_and_are_not_counted() {
    let sign = quick_start("sign");
    let groups = warnings::panel(&sign.app);
    let lettering = OperationId::new("op3");
    let group = groups
        .iter()
        .find(|g| g.op.as_ref() == Some(&lettering))
        .expect("Lettering has a group");
    let last = group.rows.last().expect("rows");
    let note = last.note.as_ref().expect("its last row is the carve note");
    assert_eq!(last.level, Level::Note);
    assert_eq!(last.op.as_ref(), Some(&lettering));
    assert_eq!(note.sections.len(), 1, "a V-Carve cuts one board");
    let section = &note.sections[0];
    assert_eq!(section.heading, None, "one board, no sub-heading");
    let first = section.facts.first().expect("facts");
    assert_eq!(first.label, "Deepest cut");
    assert_eq!(first.value, "1.658 mm below the stock top");
    for group in &groups {
        let notes: Vec<usize> = group
            .rows
            .iter()
            .enumerate()
            .filter(|(_, r)| r.note.is_some())
            .map(|(k, _)| k)
            .collect();
        assert!(
            notes.iter().all(|k| *k + 1 == group.rows.len()),
            "a note is last in its group"
        );
    }

    let counts = warnings::counts(&sign.app);
    let mut without = warnings::Counts::default();
    for row in groups.iter().flat_map(|g| &g.rows) {
        match (row.level, &row.note) {
            (_, Some(_)) => {}
            (Level::Error, None) => without.errors += 1,
            (Level::Warning, None) => without.warnings += 1,
            (Level::Hint, None) => without.hints += 1,
            (Level::Note, None) => without.notes += 1,
        }
    }
    assert_eq!(counts, without, "the notes are not counted");
    let text = warnings::copy_text(&sign.app);
    assert!(
        text.ends_with(&format!(
            "sign.dcv: 0 error(s), {} warning(s)\n",
            counts.warnings + counts.hints
        )),
        "{text}"
    );
}

/// Functional 76a (slice 28, #620): an Inlay that cuts both boards has a
/// sub-heading per board, *Female board* then *Male board*, each with
/// its deepest cut first and the male's start plane among its facts.
#[test]
fn inlay_notes_have_board_subheadings() {
    let dc = quick_start("dc");
    let note = warnings::panel(&dc.app)
        .into_iter()
        .flat_map(|g| g.rows)
        .find_map(|r| r.note)
        .expect("the Inlay's note");
    let headings: Vec<Option<&str>> = note.sections.iter().map(|s| s.heading.as_deref()).collect();
    assert_eq!(headings, [Some("Female board"), Some("Male board")]);
    for section in &note.sections {
        assert_eq!(
            section.facts.first().map(|f| f.label.as_str()),
            Some("Deepest cut")
        );
    }
    let male: Vec<&str> = note.sections[1]
        .facts
        .iter()
        .map(|f| f.label.as_str())
        .collect();
    assert!(male.contains(&"Start plane"), "{male:?}");
    assert!(male.contains(&"Cut-out"), "{male:?}");
}

/// Acceptance 82 and 99 (slice 28, #620): *Copy* writes the carve notes
/// as `check` prints them, after the warnings and before the count line,
/// so the drawer's text stays `check`'s line for line: on the sign (two
/// V-Carves) and the one-file Inlay (a line per board).
#[test]
fn copy_includes_notes_as_check_prints_them() {
    for name in ["sign", "dc"] {
        let harness = quick_start(name);
        let carves = check_carves(&harness);
        assert!(!carves.is_empty(), "{name} carves");
        let text = warnings::copy_text(&harness.app);
        let lines: Vec<&str> = text.lines().collect();
        let count = lines.len() - 1;
        assert_eq!(
            lines[count - carves.len()..count].to_vec(),
            carves.iter().map(String::as_str).collect::<Vec<_>>(),
            "{name}: {text}"
        );
        assert!(
            lines[count].starts_with(&format!("{name}.dcv: ")),
            "{name}: the count line last"
        );
        let panel: Vec<String> = warnings::lines(&harness.app);
        assert_eq!(
            panel.iter().map(String::as_str).collect::<Vec<_>>(),
            lines[..count].to_vec(),
            "{name}: the panel's lines are Copy's"
        );
    }
}

/// Slice D9 (capability 76): the app's own hints, what the session has to
/// say that is not about the project (a preferences file it could not
/// read), are rows of the drawer's hint tier, first in the project's
/// group, and counted with the hints. They are not `check`'s: *Copy*,
/// which must equal `check` on the file, neither lists nor counts them.
#[test]
fn the_apps_own_hint_is_listed_and_counted_but_not_copied() {
    let mut harness = quick_start("plaque");
    let before = warnings::counts(&harness.app);
    let copied = warnings::copy_text(&harness.app);
    assert_eq!(before.hints, 1, "the plaque's unused tool");

    let said = "gui.toml:2:8: could not be read; defaults are in use";
    harness.app.set_hints(vec![said.to_owned()]);
    let after = warnings::counts(&harness.app);
    assert_eq!(after.hints, 2);
    assert_eq!(
        (after.errors, after.warnings),
        (before.errors, before.warnings)
    );
    let rows: Vec<warnings::Row> = warnings::panel(&harness.app)
        .into_iter()
        .filter(|group| group.op.is_none())
        .flat_map(|group| group.rows)
        .collect();
    let row = &rows[0];
    assert_eq!((row.level, row.message.as_str()), (Level::Hint, said));
    assert_eq!(row.name, format!("hint: {said}"));
    assert_eq!((row.code.as_ref(), row.selection()), (None, None));
    assert_eq!(
        warnings::copy_text(&harness.app),
        copied,
        "Copy is check's text, with or without it"
    );
    assert!(
        !warnings::lines(&harness.app)
            .iter()
            .any(|l| l.contains(said))
    );

    harness.app.set_hints(Vec::new());
    assert_eq!(warnings::counts(&harness.app), before);
}

/// Slice D9: where the app shows a finding it names the operation, the
/// tool, the shape or the picture, since it shows ids nowhere else. An
/// engine message that gives an id alone after its noun (`operation
/// `op4``) gains the name before it, in the form the messages that have
/// both already use (`operation `Cut out` (`op4`)`). One that names both
/// is left as written, in either order; an id the project does not have
/// stays, since the id is what is wrong; an empty id reads *(none
/// chosen)*; and a backticked word that follows no such noun is not an
/// id to look up.
#[test]
fn a_shown_message_names_what_an_id_stands_for() {
    let project = common::plaque();
    let strings = dc_app::Strings::english();
    let shown = |message: &str| warnings::present(&project, &strings, message);
    assert_eq!(
        shown("operation `op4` cuts off the stock: 2.5 mm past its right edge"),
        "operation `Cut out` (`op4`) cuts off the stock: 2.5 mm past its right edge"
    );
    assert_eq!(
        shown("tool `t2` of operation `op1` has no feed"),
        "tool `1/4 end mill` (`t2`) of operation `Panel` (`op1`) has no feed"
    );
    assert_eq!(
        shown("operation `op1` targets instance `i4`, which is open"),
        "operation `Panel` (`op1`) targets instance `square` (`i4`), which is open"
    );
    assert_eq!(
        shown("Operations `op1` and `op2` share a tool"),
        "Operations `Panel` (`op1`) and `op2` share a tool",
        "the noun says what kind of id follows it"
    );
    for kept in [
        "operation `Panel` (`op1`) is enabled but has no targets",
        "tool `t2` (`1/4 end mill`) plunges faster than it cuts",
        "operation `op9` uses tool `t7`, which does not exist",
        "the value `op4` is not a number",
        "an unpaired ` in a message about operation `op4",
        "nothing to name",
    ] {
        assert_eq!(shown(kept), kept);
    }
    assert_eq!(
        shown("operation `op4` uses tool ``, which does not exist"),
        "operation `Cut out` (`op4`) uses tool (none chosen), which does not exist"
    );
    assert_eq!(
        shown("clearing pass 1 of operation `op1` uses tool `t3`"),
        "clearing pass 1 of operation `Panel` (`op1`) uses tool `1/8 end mill` (`t3`)"
    );
}

/// Slice D9 (capability 76: "the message with the operation and shape
/// named"): the plaque's outline moved half off the stock makes its
/// Profile cut off the stock, which the generator reports by the
/// operation's id. The drawer's row shows the name ([`Row::shown`], and
/// its [`Row::name`]), and so does every other place the app shows the
/// finding ([`App::warnings`]: the operation's row, the line under a
/// field, the status bar). `check` prints the id, so the row's line and
/// *Copy* keep it. A new Inlay, which the plaque has no V-bit for, names
/// no tool: its error says so in words.
///
/// [`Row::shown`]: warnings::Row::shown
/// [`Row::name`]: warnings::Row::name
/// [`App::warnings`]: dc_app::App::warnings
#[test]
fn findings_are_shown_with_names_and_copied_with_ids() {
    let mut harness = quick_start("plaque");
    harness
        .run(Command::TransformInstances {
            ids: vec![dc_project::InstanceId::new("i1")],
            delta: dc_geometry::Transform::translate(60.0, 0.0),
        })
        .unwrap();
    harness.generate();
    let raw = "operation `op4` cuts off the stock";
    let named = "operation `Cut out` (`op4`) cuts off the stock";
    let row = warnings::panel(&harness.app)
        .into_iter()
        .flat_map(|group| group.rows)
        .find(|row| row.message.starts_with(raw))
        .expect("the Profile leaves the stock");
    assert_eq!(row.level, Level::Error);
    assert!(row.shown.starts_with(named), "{}", row.shown);
    assert!(
        row.name.starts_with(&format!("error: {named}")),
        "{}",
        row.name
    );
    assert!(row.line.ends_with(&row.message) && row.line.contains(raw));
    let copied = warnings::copy_text(&harness.app);
    assert!(copied.contains(raw) && !copied.contains(named), "{copied}");
    assert!(
        harness
            .warnings()
            .iter()
            .any(|f| f.message.starts_with(named)),
        "what the app lists is what it shows"
    );
    assert!(
        harness
            .app
            .findings()
            .errors
            .iter()
            .any(|f| f.message.starts_with(raw)),
        "and check's text is kept beside it"
    );
    let hover = dc_app::operations::rows(&harness.app)
        .into_iter()
        .find(|r| r.name == "Cut out")
        .unwrap()
        .messages;
    assert!(hover.iter().any(|m| m.starts_with(named)), "{hover:?}");

    harness.app.undo();
    let ids = [dc_project::InstanceId::new("i4")];
    let inlay = dc_app::operations::new_operation(
        harness.app.project(),
        harness.app.strings(),
        dc_app::operations::Kind::Inlay,
        &ids,
    )
    .unwrap();
    assert_eq!(inlay.tool().as_str(), "", "the plaque has no V-bit");
    harness.run(Command::AddOperation(inlay)).unwrap();
    let said: Vec<&str> = harness
        .warnings()
        .iter()
        .map(|f| f.message.as_str())
        .filter(|m| m.contains("does not exist"))
        .collect();
    assert_eq!(
        said,
        ["operation `Inlay 1` (`op6`) uses tool (none chosen), which does not exist"]
    );
    assert!(
        warnings::copy_text(&harness.app)
            .contains("error: operation `op6` uses tool ``, which does not exist"),
        "Copy is check's line"
    );
}

/// The messy drawing as `init` wrote it (`tests/fixtures/prep/messy.dcv`):
/// two squares that overlap and a speck.
fn messy() -> Harness {
    let bytes = read(&common::fixture("prep/messy.dcv"));
    generated("messy.dcv", &bytes)
}

/// The panel's rows, flat.
fn all_rows(harness: &Harness) -> Vec<warnings::Row> {
    warnings::panel(&harness.app)
        .into_iter()
        .flat_map(|group| group.rows)
        .collect()
}

/// Capability 76, acceptance 106: the artwork checks `check` reads from a
/// project's shapes are rows of the drawer, after the warnings as `check`
/// prints them, in its tier (a warning) and its words, counted as it
/// counts them. The overlap's row names its two shapes, which clicking it
/// selects; the speck's names none. (`check`'s lines for the file are
/// compared with the binary's in the CLI's `warnings_panel` test.)
#[test]
fn artwork_checks_are_rows_after_the_warnings() {
    let harness = messy();
    let checks = dc_job::checks::project(harness.app.project());
    assert_eq!(checks.len(), 2, "{checks:?}");
    assert_eq!(harness.app.artwork_checks(), checks);

    let rows = all_rows(&harness);
    let overlap = rows
        .iter()
        .find(|r| r.code.as_deref() == Some("shapes_overlap"))
        .expect("the overlap's row");
    assert_eq!(overlap.level, Level::Warning);
    assert_eq!(overlap.message, checks[0].message);
    assert_eq!(overlap.shown, checks[0].message, "the shapes are named");
    assert_eq!(overlap.line, checks[0].line());
    assert_eq!(
        overlap.name,
        format!("warning: shapes_overlap: {}", checks[0].message)
    );
    let ids = |names: [&str; 2]| -> Vec<dc_project::InstanceId> {
        names
            .iter()
            .map(|name| {
                harness
                    .app
                    .project()
                    .instances
                    .iter()
                    .find(|i| i.name == *name)
                    .expect("the square")
                    .id
                    .clone()
            })
            .collect()
    };
    assert_eq!(
        overlap.selection(),
        Some(Selection::Instances(ids(["sq-a", "sq-b"])))
    );
    assert_eq!(
        overlap.chapter_url().as_deref(),
        Some("https://grayzu.github.io/dreamcarve/user-guide/warnings.html#shapes_overlap")
    );
    let speck = rows
        .iter()
        .find(|r| r.code.as_deref() == Some("feature_below_size"))
        .expect("the speck's row");
    assert_eq!(speck.level, Level::Warning);
    assert_eq!(speck.line, checks[1].line());
    assert_eq!(speck.selection(), None, "it names no shape");

    // After the warnings, in check's order, and counted with them.
    let lines = warnings::lines(&harness.app);
    let at = |needle: &str| {
        lines
            .iter()
            .position(|l| l.contains(needle))
            .unwrap_or_else(|| panic!("no line with {needle}: {lines:?}"))
    };
    assert!(at("open_profile") < at("shapes_overlap"));
    assert_eq!(at("shapes_overlap") + 1, at("feature_below_size"));
    assert_eq!(at("feature_below_size") + 1, lines.len());
    let counts = warnings::counts(&harness.app);
    assert_eq!(counts.warnings, 3, "the open path and the two checks");
    assert!(
        warnings::copy_text(&harness.app).ends_with("messy.dcv: 6 error(s), 3 warning(s)\n"),
        "{}",
        warnings::copy_text(&harness.app)
    );
}

/// Capability 76, acceptance 106: the checks follow the shapes. Moving
/// one square clear of the other ends the overlap and its row goes; undo
/// brings both back; hiding a square ends it too, as `check` leaves a
/// hidden shape out; deleting the speck ends the other check. An edit
/// that changes no shape leaves the checks as they were.
#[test]
fn moving_a_shape_apart_clears_the_overlap() {
    let mut harness = messy();
    let codes = |harness: &Harness| -> Vec<&'static str> {
        harness
            .app
            .artwork_checks()
            .iter()
            .map(|c| c.code)
            .collect()
    };
    let id_of = |harness: &Harness, name: &str| {
        harness
            .app
            .project()
            .instances
            .iter()
            .find(|i| i.name == name)
            .unwrap_or_else(|| panic!("no shape {name}"))
            .id
            .clone()
    };
    assert_eq!(codes(&harness), ["shapes_overlap", "feature_below_size"]);
    let square = id_of(&harness, "sq-b");

    harness
        .run(Command::TransformInstances {
            ids: vec![square.clone()],
            delta: dc_geometry::Transform::translate(8.0, 0.0),
        })
        .expect("the square moves");
    assert_eq!(codes(&harness), ["feature_below_size"]);
    assert!(
        !all_rows(&harness)
            .iter()
            .any(|r| r.code.as_deref() == Some("shapes_overlap")),
        "the row went with the overlap"
    );
    assert_eq!(warnings::counts(&harness.app).warnings, 2);

    harness.app.undo().expect("the move is undone");
    assert_eq!(codes(&harness), ["shapes_overlap", "feature_below_size"]);

    harness
        .run(Command::SetInstanceFlags {
            ids: vec![square],
            visible: Some(false),
            locked: None,
        })
        .expect("the square is hidden");
    assert_eq!(codes(&harness), ["feature_below_size"]);
    harness.app.undo().expect("shown again");

    let before = harness.app.artwork_checks().to_vec();
    let mut stock = harness.app.project().stock.clone();
    stock.thickness_mm = Some(12.0);
    harness.run(Command::SetStock(stock)).expect("the stock");
    assert_eq!(harness.app.artwork_checks(), before);

    let dot = id_of(&harness, "dot");
    harness
        .run(Command::DeleteInstances(vec![dot]))
        .expect("the speck is deleted");
    assert_eq!(codes(&harness), ["shapes_overlap"]);
}

/// The maintainer's tester, 2026-11-24: messages named the file's fields
/// (`operations[op1].depth_mm is not set`, `raise margin_mm`), which the
/// window shows nowhere. Where the app shows a finding, a field's name
/// reads as the label its editor has for it: the operation's own row,
/// the tool's field, the stock's. A path that names an operation or a
/// tool says whose it is, unless the message already does. What the app
/// has no label for stays as written, and so does every word that is no
/// field: an option's value, a code, a name someone chose.
#[test]
fn a_shown_message_calls_a_field_by_its_label() {
    let plaque = common::plaque();
    let strings = dc_app::Strings::english();
    let shown = |message: &str| warnings::present(&plaque, &strings, message);
    // A path, with the operation named after it or not at all.
    assert_eq!(
        shown("operations[op4].depth_mm is not set (mm) for operation `Cut out`"),
        "Depth is not set (mm) for operation `Cut out`"
    );
    assert_eq!(
        shown(
            "operations[op1].step_over_fraction is 1.5, expected a value greater than 0 and at most 1"
        ),
        "Stepover of operation `Panel` (`op1`) is 1.5, expected a value greater than 0 and at most 1"
    );
    // A bare name is the operation's the message is about.
    assert_eq!(
        shown(
            "operation `Cut out` (`op4`) has last_pass.stock_to_leave_mm 2.5 mm with tool `1/4 end mill` (6.35 mm)"
        ),
        "operation `Cut out` (`op4`) has Stock to leave 2.5 mm with tool `1/4 end mill` (6.35 mm)"
    );
    assert_eq!(
        shown(
            "heights.clearance_mm is 3.5 mm, below the 5 mm safe_z_mm of operation `Cut out` (`op4`)"
        ),
        "Clearance is 3.5 mm, below the 5 mm Safe Z of operation `Cut out` (`op4`)"
    );
    // A tool's, by its path or after its name; in ticks or out of them.
    assert_eq!(
        shown("tool `1/4 end mill` (`t2`): feed_mm_min is 25000, expected a value greater than 0"),
        "tool `1/4 end mill` (`t2`): Feed is 25000, expected a value greater than 0"
    );
    assert_eq!(
        shown("more than its 2.5 mm step-down (tools[t2].step_down_mm); set `step_down_mm`"),
        "more than its 2.5 mm step-down (Step-down of tool `1/4 end mill` (`t2`)); set Step-down"
    );
    // The stock's.
    assert_eq!(
        shown("stock.thickness_mm is not set (mm)"),
        "Thickness of the stock is not set (mm)"
    );
    // Left as written: no label for it, an option's value, a code, a
    // command, and a name that happens to look like a field.
    for kept in [
        "set [estimate] rapid_mm_min in config.toml",
        "set the operation's entry to `plunge`, or its order to `pass_first`",
        "tab_not_crossed: remove the tab (`dreamcarve tabs clear <dcv> --tab tab1`)",
        "operation `op9` has depth_mm 3 mm",
        "nothing to name",
    ] {
        assert_eq!(shown(kept), kept, "{kept}");
    }

    // An Inlay's fields have an Inlay's labels.
    let dc = common::load_dcv(&common::fixture("dc.dcv"));
    let shown = |message: &str| warnings::present(&dc, &strings, message);
    assert_eq!(
        shown(
            "operation `DC` (`op1`) has glue_gap_mm 4.5 mm, not below its depth_mm 4 mm; the male starts depth_mm − glue_gap_mm below the stock top. Raise `margin_mm` to at least 7.73 mm, or set male_board.thickness_mm; use a smaller depth_mm or plug_depth_mm."
        ),
        "operation `DC` (`op1`) has Glue gap 4.5 mm, not below its Inlay depth 4 mm; the male starts Inlay depth − Glue gap below the stock top. Raise Band margin to at least 7.73 mm, or set Board thickness; use a smaller Inlay depth or Plug depth."
    );
}

/// Every row of every kind of operation has its label where a message
/// names its key after the operation: none is left as the file's name,
/// but a key several of the operation's rows end with, which could be
/// any of them.
#[test]
fn every_row_of_an_operation_is_called_by_its_label() {
    use dc_app::editor::fields;
    let strings = dc_app::Strings::english();
    for project in [
        common::plaque(),
        common::load_dcv(&common::fixture("dc.dcv")),
    ] {
        for op in &project.operations {
            for row in fields::rows(op) {
                if !row.key.contains('_') {
                    continue;
                }
                let message = format!("operation `{}` has {} wrong", op.id(), row.key);
                let shown = warnings::present(&project, &strings, &message);
                assert!(
                    shown.contains(strings.get(row.label)) && !shown.contains(row.key),
                    "{}: {} -> {shown}",
                    op.name(),
                    row.key
                );
            }
        }
    }
}
