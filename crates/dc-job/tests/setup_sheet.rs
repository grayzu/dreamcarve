// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 33 of phase 4 (#625; functional spec, capability 77b; acceptance
//! 102): the setup sheet `dc_job::setup_sheet::render` writes is one
//! self-contained HTML page that states the same facts as the programs
//! the export wrote: their files in run order, each one's tool, the step
//! before it, its extents and its estimate, in the project's display
//! units, byte-identical for the same date. Its labels are the app's
//! string table's `[setup_sheet]` entries, read here from
//! `crates/dc-app/strings/en.toml` where it lives.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_job::setup_sheet::{self, Label, Sheet};
use dc_job::summary::{describe_estimated_time, describe_length};
use dc_job::{EstimateConfig, ProgramSplit, ProgramSummary};
use dc_project::{Project, Units};

use common::{DATE, load_dcv, repo_root};

/// The version the footer names in these tests, pinned so the committed
/// page does not change with a release.
const VERSION: &str = "0.0.0-test";

/// The committed page `deterministic_with_the_date_pinned` compares with.
const PLAQUE_HTML: &str = "crates/dc-job/tests/fixtures/setup_sheet/plaque.html";

/// The `[setup_sheet]` table of the app's English strings.
fn english() -> toml::Table {
    let path = repo_root().join("crates/dc-app/strings/en.toml");
    let text = std::fs::read_to_string(&path).expect("the string table is readable");
    let table: toml::Table = text.parse().expect("the string table parses");
    table
        .get("setup_sheet")
        .and_then(toml::Value::as_table)
        .cloned()
        .expect("the table has a [setup_sheet] section")
}

/// `label`'s English text, from the string table.
fn text(table: &toml::Table, label: Label) -> String {
    let key = label
        .key()
        .strip_prefix("setup_sheet.")
        .unwrap_or_else(|| panic!("{} is not a setup_sheet key", label.key()));
    table
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| panic!("{} is not in the table", label.key()))
        .to_owned()
}

/// What the export of `project` built and listed: its program rows,
/// named under `stem`, and the warnings it found.
struct Exported {
    project: Project,
    programs: Vec<ProgramSummary>,
    names: Vec<String>,
    warnings: Vec<dc_job::Finding>,
}

/// `project` exported as `dreamcarve export` exports it, with the date
/// pinned.
fn export(project: Project, stem: &str) -> Exported {
    let built = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
    assert!(built.findings.errors.is_empty(), "{:?}", built.findings);
    let generated = built.generated.clone().unwrap_or_default();
    let (programs, _) = dc_job::summarize(
        &project,
        &generated,
        built.programs,
        ProgramSplit::ProjectDefault,
        &EstimateConfig::default(),
    );
    let raw: Vec<dc_job::Program> = programs.iter().map(|p| p.program.clone()).collect();
    let names = dc_job::program_names(&project, stem, &raw);
    Exported {
        project,
        programs,
        names,
        warnings: built.findings.warnings,
    }
}

/// The quick-start plaque, exported.
fn plaque() -> Exported {
    export(
        load_dcv(&repo_root().join("tests/fixtures/plaque.dcv")),
        "plaque",
    )
}

/// The sheet of `exported`, dated [`DATE`].
fn render(exported: &Exported) -> String {
    let table = english();
    setup_sheet::render(
        &Sheet {
            project: &exported.project,
            programs: &exported.programs,
            names: &exported.names,
            warnings: &exported.warnings,
            split: ProgramSplit::ProjectDefault,
            estimate: EstimateConfig::default(),
            date: DATE,
            version: VERSION,
        },
        &|label| text(&table, label),
    )
}

/// `text` as the sheet escapes it.
fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Capability 77b: no script, no external resource, no image.
#[test]
fn render_is_self_contained() {
    for name in ["plaque", "sign", "dc"] {
        let exported = export(
            load_dcv(&repo_root().join(format!("tests/fixtures/{name}.dcv"))),
            name,
        );
        let html = render(&exported).to_ascii_lowercase();
        assert!(html.starts_with("<!doctype html>"), "{name}: a page");
        for banned in ["<script", "http", "<img", "url(", "<link", "@import"] {
            assert!(!html.contains(banned), "{name}: `{banned}` in the sheet");
        }
    }
}

/// Capability 77b and acceptance 102: the programs in run order, each
/// with its file, its tool, the step before it as `export` says it, its
/// extents (tool centre, bit edge, Z range) as the CLI writes the numbers
/// and its estimate, equal what `build` and `summarize` give; then the
/// warnings and the controller notes.
#[test]
fn matches_the_export() {
    let exported = plaque();
    let html = render(&exported);
    let project = &exported.project;
    assert_eq!(exported.names, ["plaque-1-T2.nc", "plaque-2-T1.nc"]);
    let mut last = 0;
    for (summary, name) in exported.programs.iter().zip(&exported.names) {
        let at = html
            .find(&escaped(name))
            .unwrap_or_else(|| panic!("{name} is on the sheet"));
        assert!(at > last, "{name}: the programs in run order");
        last = at;
        let tool = &summary.program.first_tool;
        let block = &html[at..];
        assert!(block.contains(&format!("T{}", tool.number)), "{name}");
        assert!(block.contains(&escaped(&tool.name)), "{name}: its tool");
        let fitted = project.tools.iter().find(|t| t.id == tool.id).unwrap();
        assert!(
            block.contains(&escaped(&describe_length(project, fitted.diameter_mm))),
            "{name}: its tool's diameter"
        );
        assert!(
            block.contains(&escaped(&summary.before)),
            "{name}: the step before, as export says it: {}",
            summary.before
        );
        let time = describe_estimated_time(
            &summary.estimate.expect("the plaque estimates"),
            &EstimateConfig::default(),
        );
        assert!(block.contains(&time), "{name}: its estimate {time}");
        for range in setup_sheet::ranges(project, summary) {
            assert!(block.contains(&escaped(&range)), "{name}: extent {range}");
        }
        for id in &summary.operations {
            let op = project.operations.iter().find(|o| o.id() == id).unwrap();
            assert!(block.contains(&escaped(op.name())), "{name}: {}", op.name());
        }
    }
    // The extents are the CLI's: its numbers, in its order.
    let first = &exported.programs[0];
    let ranges = setup_sheet::ranges(project, first);
    assert_eq!(
        ranges,
        [
            "X 15.000..155.000, Y 15.000..105.000",
            "X 13.412..156.588, Y 13.412..106.588",
            "Z -12.000..15.000",
        ]
    );
    for warning in &exported.warnings {
        assert!(
            html.contains(&escaped(&warning.message)),
            "the warning {}",
            warning.message
        );
    }
    for note in dc_job::report::notes(project, &exported.programs, ProgramSplit::ProjectDefault)
    {
        assert!(html.contains(&escaped(&note)), "the note {note}");
    }
    assert!(html.contains(DATE), "the export date");
    assert!(html.contains("grblHAL"), "the post-processor");
}

/// Capability 77b: with the date pinned, two renders are byte-identical
/// and equal the committed page.
#[test]
fn deterministic_with_the_date_pinned() {
    let exported = plaque();
    let first = render(&exported);
    assert!(first == render(&exported), "two renders differ");
    let committed = repo_root().join(PLAQUE_HTML);
    let expected = std::fs::read_to_string(&committed)
        .unwrap_or_else(|e| panic!("{}: {e}", committed.display()));
    assert!(
        first == expected,
        "the sheet differs from {PLAQUE_HTML}; the new page:\n{first}"
    );
}

/// Capability 77b: an inch project's sheet states its lengths in inches,
/// the stock's and the tools' as the CLI states lengths and the extents
/// as the programs write them.
#[test]
fn uses_display_units() {
    let mut project = load_dcv(&repo_root().join("tests/fixtures/plaque.dcv"));
    project.display_units = Units::Inch;
    let exported = export(project, "plaque");
    let html = render(&exported);
    let project = &exported.project;
    let width = describe_length(project, project.stock.width_mm);
    assert!(width.ends_with(" in"), "{width}");
    assert!(html.contains(&width), "the stock width in inches: {width}");
    for summary in &exported.programs {
        assert_eq!(summary.program.extents.units, Units::Inch);
        for range in setup_sheet::ranges(project, summary) {
            assert!(html.contains(&escaped(&range)), "extent {range}");
        }
    }
    assert!(!html.contains(" mm"), "no length in millimetres:\n{html}");
}
