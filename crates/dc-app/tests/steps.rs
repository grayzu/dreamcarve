// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice D3 of the redesign (`docs/specs/phase-4/redesign.md`, "The
//! steps"): the five steps as data. Each step's state is computed from the
//! project and its findings; the header's *Continue* goes to the next step
//! that is not complete; a newly opened project starts on the first one.
//! Each test is named for the rule it pins.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::steps::{self, Session, Severity, State, States, Step};
use dc_app::test_support::Harness;
use dc_app::{App, Command, Selection};
use dc_job::Finding;
use dc_project::{OperationId, ToolId};

use common::{coaster, plaque, plaque_bytes};

/// A session in which nothing was imported with a warning and nothing
/// exported.
fn fresh() -> Session {
    Session::default()
}

/// A new, empty project.
fn new_project() -> Harness {
    Harness::from_project(App::new_project().unwrap())
}

/// The tool the project's first operation cuts with: one an enabled
/// operation uses.
fn used_tool(harness: &Harness) -> String {
    harness.app.project().operations[0].tool().to_string()
}

/// Deletes tool `id`'s feed.
fn without_feed(harness: &mut Harness, id: &str) {
    let mut tool = harness
        .app
        .project()
        .tools
        .iter()
        .find(|t| t.id == ToolId::new(id))
        .unwrap()
        .clone();
    tool.feed_mm_min = None;
    harness.run(Command::UpdateTool(tool)).unwrap();
}

/// The states of `harness`'s app in a fresh session.
fn states(harness: &Harness) -> States {
    steps::states(&harness.app, &fresh())
}

/// The order is the workflow's, and it is the rail's.
#[test]
fn the_steps_run_design_stock_tools_operations_export() {
    assert_eq!(
        Step::ALL,
        [
            Step::Design,
            Step::Stock,
            Step::Tools,
            Step::Operations,
            Step::Export
        ]
    );
    let strings = dc_app::Strings::english();
    let labels: Vec<&str> = Step::ALL.iter().map(|s| strings.get(s.label())).collect();
    assert_eq!(labels, ["Design", "Stock", "Tools", "Operations", "Export"]);
}

/// A new project has nothing in any step, so the rail shows no mark: its
/// unset thickness is not yet a problem, and there is nothing to export.
#[test]
fn a_new_project_has_every_step_empty() {
    let harness = new_project();
    for step in Step::ALL {
        assert_eq!(states(&harness).of(step), State::Empty, "{step:?}");
    }
}

/// Design is complete once the project has a shape.
#[test]
fn design_is_complete_with_a_shape() {
    let harness = Harness::from_project(plaque());
    assert_eq!(states(&harness).of(Step::Design), State::Complete);
}

/// Design needs attention while an importer's warning about a drawing is
/// still noted in the Warnings panel.
#[test]
fn design_needs_attention_while_an_import_warning_is_noted() {
    let harness = Harness::from_project(plaque());
    let session = Session {
        import_warnings: 2,
        ..fresh()
    };
    assert_eq!(
        steps::states(&harness.app, &session).of(Step::Design),
        State::Attention(Severity::Warning)
    );
}

/// Under a drawing, the unset thickness is the Stock step's error: the
/// coaster as `init` makes it.
#[test]
fn stock_needs_attention_when_thickness_is_unset_under_a_drawing() {
    let harness = Harness::from_project(coaster());
    assert_eq!(
        states(&harness).of(Step::Stock),
        State::Attention(Severity::Error)
    );
}

/// Stock is complete when the thickness is set and every shape is on the
/// stock.
#[test]
fn stock_is_complete_with_thickness_and_every_shape_on_it() {
    let harness = Harness::from_project(plaque());
    assert_eq!(states(&harness).of(Step::Stock), State::Complete);
}

/// A shape that reaches past the stock keeps the step from complete, and
/// the rail says so: no finding names it, so the step does.
#[test]
fn stock_needs_attention_when_a_shape_reaches_past_it() {
    let mut harness = Harness::from_project(plaque());
    let mut stock = harness.app.project().stock.clone();
    stock.width_mm = 60.0;
    harness.run(Command::SetStock(stock)).unwrap();
    assert!(steps::off_stock(&harness.app));
    assert_eq!(
        states(&harness).of(Step::Stock),
        State::Attention(Severity::Warning)
    );
}

/// Tools is empty until the project has a tool.
#[test]
fn tools_is_empty_without_a_tool() {
    let mut project = plaque();
    project.operations.clear();
    project.tabs.clear();
    project.tools.clear();
    let harness = Harness::from_project(project);
    assert_eq!(states(&harness).of(Step::Tools), State::Empty);
}

/// A tool an enabled operation uses, without a feed, is the Tools step's
/// error.
#[test]
fn tools_needs_attention_when_a_used_tool_lacks_a_rate() {
    let mut harness = Harness::from_project(plaque());
    assert_eq!(states(&harness).of(Step::Tools), State::Complete);
    let used = used_tool(&harness);
    without_feed(&mut harness, &used);
    assert_eq!(
        states(&harness).of(Step::Tools),
        State::Attention(Severity::Error)
    );
}

/// A hint asks for nothing: the plaque's unused tool leaves Tools
/// complete, with or without its rates, since nothing cuts with it.
#[test]
fn an_unused_tool_leaves_tools_complete() {
    let mut harness = Harness::from_project(plaque());
    let hints: Vec<&Finding> = harness
        .warnings()
        .iter()
        .filter(|f| f.code.as_deref() == Some("unused_tool"))
        .collect();
    assert_eq!(hints.len(), 1, "the plaque has one unused tool");
    let unused = hints[0].field.clone().unwrap();
    let id = unused
        .strip_prefix("tools[")
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap()
        .to_owned();
    without_feed(&mut harness, &id);
    assert_eq!(states(&harness).of(Step::Tools), State::Complete);
}

/// Operations is empty without an operation, and complete with an
/// enabled one that has no error or warning.
#[test]
fn operations_is_complete_with_an_enabled_operation_and_no_finding() {
    let harness = new_project();
    assert_eq!(states(&harness).of(Step::Operations), State::Empty);
    let mut harness = Harness::from_project(plaque());
    harness.generate();
    assert_eq!(states(&harness).of(Step::Operations), State::Complete);
}

/// An operation's error (the coaster's Profile has no depth) is the
/// Operations step's.
#[test]
fn operations_needs_attention_when_an_operation_has_an_error() {
    let harness = Harness::from_project(coaster());
    assert!(
        harness
            .warnings()
            .iter()
            .any(|f| f.op.is_some() && f.tier == dc_job::Tier::Error),
        "the coaster's Profile has unset values"
    );
    assert_eq!(
        states(&harness).of(Step::Operations),
        State::Attention(Severity::Error)
    );
}

/// An operation's warning (a disabled operation is one) is the Operations
/// step's, as a warning.
#[test]
fn operations_needs_attention_when_an_operation_has_a_warning() {
    let mut harness = Harness::from_project(plaque());
    let id = harness.app.project().operations[0].id().clone();
    harness
        .run(Command::SetOperationEnabled { id, enabled: false })
        .unwrap();
    assert_eq!(
        states(&harness).of(Step::Operations),
        State::Attention(Severity::Warning)
    );
}

/// Anything that blocks export is the Export step's error, whichever
/// step owns the field.
#[test]
fn export_needs_attention_when_an_error_blocks_it() {
    let mut harness = Harness::from_project(plaque());
    assert_eq!(states(&harness).of(Step::Export), State::Empty);
    let used = used_tool(&harness);
    without_feed(&mut harness, &used);
    assert_eq!(
        states(&harness).of(Step::Export),
        State::Attention(Severity::Error)
    );
}

/// Export is complete when the last export wrote the current snapshot,
/// and no longer once the project has changed.
#[test]
fn export_is_complete_until_the_project_changes() {
    let mut harness = Harness::from_project(plaque());
    let session = Session {
        exported: Some(harness.app.snapshot()),
        ..fresh()
    };
    assert_eq!(
        steps::states(&harness.app, &session).of(Step::Export),
        State::Complete
    );
    let mut stock = harness.app.project().stock.clone();
    stock.width_mm += 10.0;
    harness.run(Command::SetStock(stock)).unwrap();
    assert_eq!(
        steps::states(&harness.app, &session).of(Step::Export),
        State::Empty
    );
}

/// A finding belongs to the step that edits its field: stock and origin
/// to Stock, a tool's to Tools, an operation's to Operations, and the
/// output settings and anything unplaced to Export.
#[test]
fn a_finding_belongs_to_the_step_that_edits_its_field() {
    let at = |field: &str| Finding::error("x").at_field(Some(field.to_owned()));
    for (field, step) in [
        ("stock.thickness_mm", Step::Stock),
        ("origin.xy", Step::Stock),
        ("tools[t1].feed_mm_min", Step::Tools),
        ("tools[t1]", Step::Tools),
        ("operations[o1].depth_mm", Step::Operations),
        ("operations[o1].clearing[0].tool", Step::Operations),
        ("instances[i1].definition", Step::Design),
        ("tabs[tab1].instance", Step::Design),
        ("heights.clearance_mm", Step::Export),
        ("post_processor.file_extension", Step::Export),
        ("flatten_tolerance_mm", Step::Export),
    ] {
        assert_eq!(steps::step_of(&at(field)), step, "{field}");
    }
    assert_eq!(steps::step_of(&Finding::error("x")), Step::Export);
    let about_op = Finding::error("x").for_op(Some(OperationId::new("o1")));
    assert_eq!(steps::step_of(&about_op), Step::Operations);
}

/// *Continue* goes to the next step after the current one that is not
/// complete.
#[test]
fn continue_goes_to_the_next_step_that_is_not_complete() {
    // The coaster: Design is done, Stock lacks its thickness.
    let harness = Harness::from_project(coaster());
    let all = states(&harness);
    assert_eq!(steps::next(&all, Step::Design), Step::Stock);
    // The plaque: everything is done but the export.
    let harness = Harness::from_project(plaque());
    let all = states(&harness);
    for step in [Step::Design, Step::Stock, Step::Tools, Step::Operations] {
        assert_eq!(steps::next(&all, step), Step::Export, "{step:?}");
    }
}

/// With nothing left after the current step, *Continue* looks before it;
/// with every other step complete it goes to Export.
#[test]
fn continue_wraps_to_an_earlier_step_and_ends_on_export() {
    let mut harness = Harness::from_project(plaque());
    let session = Session {
        exported: Some(harness.app.snapshot()),
        ..fresh()
    };
    let all = steps::states(&harness.app, &session);
    assert_eq!(steps::next(&all, Step::Tools), Step::Export);
    assert_eq!(steps::next(&all, Step::Export), Step::Export);
    // Exported, then a tool loses its feed: Export is the first after
    // Operations that is not complete; from Export it wraps to Tools.
    let used = used_tool(&harness);
    without_feed(&mut harness, &used);
    let all = steps::states(&harness.app, &session);
    assert_eq!(steps::next(&all, Step::Operations), Step::Export);
    assert_eq!(steps::next(&all, Step::Export), Step::Tools);
}

/// Opening a project selects the first step that is not complete: an
/// empty project Design, the coaster Stock, a finished project Export.
#[test]
fn a_project_opens_on_its_first_step_that_is_not_complete() {
    assert_eq!(steps::opening(&states(&new_project())), Step::Design);
    assert_eq!(
        steps::opening(&states(&Harness::from_project(coaster()))),
        Step::Stock
    );
    let harness = Harness::open(&plaque_bytes()).unwrap();
    assert_eq!(steps::opening(&states(&harness)), Step::Export);
}

/// While toolpaths regenerate, an operation's findings are gone until its
/// result is back; the mark shown for Operations and Export holds, so it
/// does not flicker. A mark that appears shows at once, and the other
/// steps are never held.
#[test]
fn a_mark_holds_while_toolpaths_regenerate() {
    let warned = State::Attention(Severity::Warning);
    let failed = State::Attention(Severity::Error);
    let shown = States::new([State::Complete, warned, State::Complete, warned, failed]);
    let now = States::new([
        State::Complete,
        State::Complete,
        State::Complete,
        State::Complete,
        State::Empty,
    ]);
    let held = steps::settle(&shown, now, true);
    assert_eq!(held.of(Step::Stock), State::Complete, "Stock is not held");
    assert_eq!(held.of(Step::Operations), warned);
    assert_eq!(held.of(Step::Export), failed);
    assert_eq!(steps::settle(&shown, now, false), now, "settled: as it is");
    let worse = States::new([
        State::Complete,
        State::Complete,
        State::Complete,
        failed,
        failed,
    ]);
    assert_eq!(
        steps::settle(&shown, worse, true).of(Step::Operations),
        failed,
        "a worse mark shows at once"
    );
}

/// An operation belongs to Operations, a shape or a placed tab to Design:
/// what selecting one from the Warnings drawer switches to.
#[test]
fn a_selection_belongs_to_the_step_that_edits_it() {
    assert_eq!(
        steps::owner(&Selection::Operation(OperationId::new("o1"))),
        Some(Step::Operations)
    );
    assert_eq!(
        steps::owner(&Selection::Instances(vec![dc_project::InstanceId::new(
            "i1"
        )])),
        Some(Step::Design)
    );
    assert_eq!(steps::owner(&Selection::Nothing), None);
}

/// Each step's one line under its title says what it holds.
#[test]
fn each_step_has_a_one_line_summary() {
    let harness = new_project();
    let line = |h: &Harness, s: &Session, step| steps::summary(&h.app, s, step);
    assert_eq!(
        line(&harness, &fresh(), Step::Design),
        "Bring in a drawing or a picture to cut."
    );
    assert_eq!(
        line(&harness, &fresh(), Step::Stock),
        "100 × 100 mm · thickness not set"
    );
    assert_eq!(
        line(&harness, &fresh(), Step::Tools),
        "The cutters this job uses."
    );
    assert_eq!(
        line(&harness, &fresh(), Step::Operations),
        "What to cut, in cut order."
    );
    assert_eq!(
        line(&harness, &fresh(), Step::Export),
        "Nothing to export yet."
    );

    let mut harness = Harness::from_project(plaque());
    assert_eq!(line(&harness, &fresh(), Step::Design), "10 shapes");
    assert_eq!(line(&harness, &fresh(), Step::Stock), "170 × 120 × 11.8 mm");
    assert_eq!(line(&harness, &fresh(), Step::Tools), "3 tools");
    assert_eq!(line(&harness, &fresh(), Step::Operations), "5 operations");
    assert_eq!(line(&harness, &fresh(), Step::Export), "Ready to export.");
    let exported = Session {
        exported: Some(harness.app.snapshot()),
        ..fresh()
    };
    assert_eq!(
        line(&harness, &exported, Step::Export),
        "Exported. Nothing has changed since."
    );
    let used = used_tool(&harness);
    without_feed(&mut harness, &used);
    assert_eq!(
        line(&harness, &fresh(), Step::Export),
        "1 error blocks export."
    );
}
