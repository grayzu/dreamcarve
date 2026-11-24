// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 22 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Organising"): construction geometry (functional capability 100,
//! acceptance 122 to 125). A shape marked Construction is never cut: it
//! leaves every operation's targets, no picker offers it, the artwork
//! checks and Fit Stock to Shapes pass it by. The file's own refusal of a
//! construction target is `dc-project`'s (`construction_targeted`, S21).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::command::NewShape;
use dc_app::operations::{self, Kind};
use dc_app::test_support::Harness;
use dc_app::tool::{Event, ToolOption};
use dc_app::tree::{self, MenuItem, Tree};
use dc_app::{App, Command, CommandError, Drawing, Pointer, Strings, Tool};
use dc_geometry::{Point, Polyline, Transform};
use dc_project::{InstanceId, OperationId};

use common::plaque_bytes;

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn plaque() -> App {
    Harness::open(&plaque_bytes()).unwrap().app
}

fn mark(ids: &[&str], construction: bool) -> Command {
    Command::SetConstruction {
        ids: ids.iter().map(|i| id(i)).collect(),
        construction,
    }
}

fn targets(app: &App, op: &str) -> Vec<String> {
    app.project()
        .operations
        .iter()
        .find(|o| o.id().as_str() == op)
        .unwrap()
        .targets()
        .iter()
        .map(|t| t.as_str().to_owned())
        .collect()
}

fn is_construction(app: &App, instance: &str) -> bool {
    app.project()
        .instances
        .iter()
        .find(|i| i.id.as_str() == instance)
        .unwrap()
        .construction
}

/// Acceptance 122 and 123: marking a shape Construction takes it out of
/// every operation's targets, in one undo step, and the Warnings drawer
/// says which operation is left with none, as deleting the shape would.
/// Turning it off again does not put it back.
#[test]
fn construction_leaves_every_operation() {
    let mut app = plaque();
    assert_eq!(targets(&app, "op2"), ["i4"]);
    assert_eq!(targets(&app, "op3"), ["i5", "i6", "i7", "i8"]);
    let label = app.run(mark(&["i4", "i5"], true)).unwrap();
    assert_eq!(label, "Turn Construction on for 2 shapes");
    assert!(is_construction(&app, "i4") && is_construction(&app, "i5"));
    assert!(targets(&app, "op2").is_empty());
    assert_eq!(targets(&app, "op3"), ["i6", "i7", "i8"]);
    // An error has no code: it is known by what it says.
    let said: Vec<&str> = app.warnings().iter().map(|f| f.message.as_str()).collect();
    assert!(
        said.iter()
            .any(|m| m.contains("`Square recess` (`op2`) is enabled but has no targets")),
        "{said:?}"
    );
    assert!(
        !said.iter().any(|m| m.contains("construction geometry")),
        "{said:?}"
    );

    // Already so: nothing to do.
    assert!(matches!(
        app.run(mark(&["i4"], true)),
        Err(CommandError::NothingToDo)
    ));
    let label = app.run(mark(&["i4"], false)).unwrap();
    assert_eq!(label, "Turn Construction off for “square”");
    assert!(!is_construction(&app, "i4"));
    assert!(targets(&app, "op2").is_empty(), "it is not put back");

    assert!(app.undo().is_some());
    assert!(app.undo().is_some());
    assert_eq!(targets(&app, "op2"), ["i4"], "one undo step");
    assert!(!is_construction(&app, "i5"));
}

/// Acceptance 124: no operation offers construction geometry as a
/// target. *Add operation* passes a construction shape by, and says why
/// when it is all that is selected; *Add to* is not offered for one;
/// target editing does not take one, by a click or by a marquee; and an
/// Inlay's boundary does not list one.
#[test]
fn pickers_skip_construction() {
    let mut app = plaque();
    app.run(mark(&["i4"], true)).unwrap();
    let strings = Strings::english();
    let project = app.project().clone();

    // Alone: every kind that cuts shapes says why not.
    let menu = operations::add_menu(&project, &strings, &[id("i4")], &[]);
    assert!(!menu.available.contains(&Kind::Profile));
    let (_, reason) = menu
        .unavailable
        .iter()
        .find(|(kind, _)| *kind == Kind::Profile)
        .unwrap();
    assert_eq!(
        reason.as_deref(),
        Some("Construction geometry is never cut: turn Construction off for the shape first")
    );
    assert_eq!(
        operations::add(&project, &strings, Kind::Profile, &[id("i4")], &[]),
        None
    );
    // With a shape that is cut: the operation takes that one alone.
    let both = [id("i4"), id("i2")];
    let menu = operations::add_menu(&project, &strings, &both, &[]);
    assert!(menu.available.contains(&Kind::Profile));
    let Some(Command::AddOperation(made)) =
        operations::add(&project, &strings, Kind::Profile, &both, &[])
    else {
        panic!("a Profile is offered");
    };
    assert_eq!(made.targets(), [id("i2")]);

    // The row menu: no Add to for a construction row.
    let panel = project
        .operations
        .iter()
        .find(|o| o.id().as_str() == "op1")
        .unwrap();
    let items = tree::menu(&project, &[id("i4")], Some(panel), false);
    assert!(!items.contains(&MenuItem::AddTo));
    let mixed = tree::menu(&project, &[id("i4"), id("i1")], Some(panel), false);
    assert!(mixed.contains(&MenuItem::AddTo));
    let Some(Command::UpdateOperation(added)) =
        tree::target_command(&project, panel, MenuItem::AddTo, &[id("i4"), id("i1")]).unwrap()
    else {
        panic!("the outline is added");
    };
    assert_eq!(added.targets(), [id("i2"), id("i3"), id("i1")]);

    // Target editing: a click and a marquee pass it by.
    assert!(app.edit_targets(&OperationId::new("op1")));
    app.toggle_target(&id("i4"));
    app.box_targets(&[id("i4"), id("i1")], false);
    assert_eq!(
        app.target_editing().unwrap().targets(),
        [id("i2"), id("i3"), id("i1")]
    );
    app.cancel_targets();

    // An Inlay's boundary.
    let inlay = operations::new_operation(&project, &strings, Kind::Inlay, &[id("i2")]).unwrap();
    let offered = dc_app::editor::offered_boundaries(&project, &inlay);
    assert!(offered.iter().all(|(shape, _)| shape != &id("i4")));
    assert!(offered.iter().any(|(shape, _)| shape == &id("i1")));
}

/// Acceptance 124: the artwork checks pass construction geometry by, and
/// so does *Fit Stock to Shapes*.
#[test]
fn checks_and_fit_stock_pass_it_by() {
    let overlapping = |x: f64| NewShape {
        name: "guide".to_owned(),
        shape: None,
        profiles: vec![Polyline::rect(Point::new(x, 10.0), 30.0, 30.0)],
        transform: Transform::IDENTITY,
        construction: false,
        group: None,
    };
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.run(Command::AddShapes(vec![
        overlapping(10.0),
        overlapping(25.0),
    ]))
    .unwrap();
    let codes = |app: &App| -> Vec<&'static str> {
        dc_job::checks::project(app.project())
            .iter()
            .map(|check| check.code)
            .collect()
    };
    assert!(codes(&app).contains(&"shapes_overlap"), "{:?}", codes(&app));
    app.run(mark(&["i2"], true)).unwrap();
    assert!(
        !codes(&app).contains(&"shapes_overlap"),
        "{:?}",
        codes(&app)
    );
    assert!(
        app.artwork_checks()
            .iter()
            .all(|c| c.code != "shapes_overlap")
    );

    // Fit Stock to Shapes: round the cut shape, 10 to 40, alone.
    app.run(Command::FitStockToShapes { margin_mm: 5.0 })
        .unwrap();
    assert_eq!(app.project().stock.width_mm, 40.0);
    // Construction geometry alone is nothing to fit to.
    app.run(mark(&["i1"], true)).unwrap();
    assert!(matches!(
        app.run(Command::FitStockToShapes { margin_mm: 5.0 }),
        Err(CommandError::NoShapes)
    ));
}

/// The Design Tree's row says it, and the row menu marks and unmarks.
#[test]
fn the_tree_marks_construction_rows() {
    let mut app = plaque();
    let row = |app: &App, name: &str| {
        Tree::build(app.project())
            .shapes
            .into_iter()
            .find(|row| row.name == name)
            .unwrap()
    };
    assert!(!row(&app, "square").construction);
    let items = tree::menu(app.project(), &[id("i4")], None, false);
    assert!(items.contains(&MenuItem::Construction));
    assert!(!items.contains(&MenuItem::NotConstruction));
    let on = tree::menu_command(app.project(), MenuItem::Construction, &[id("i4")]).unwrap();
    assert_eq!(on, mark(&["i4"], true));
    app.run(on).unwrap();
    assert!(row(&app, "square").construction);
    // Every row construction: the item turns it off. Mixed: on.
    let items = tree::menu(app.project(), &[id("i4")], None, false);
    assert!(items.contains(&MenuItem::NotConstruction));
    let items = tree::menu(app.project(), &[id("i4"), id("i1")], None, false);
    assert!(items.contains(&MenuItem::Construction));
    let off = tree::menu_command(app.project(), MenuItem::NotConstruction, &[id("i4")]).unwrap();
    assert_eq!(off, mark(&["i4"], false));
}

/// The tool bar's Construction switch while drawing: the shapes that
/// follow are construction geometry until it is turned off.
#[test]
fn the_drawing_tools_have_a_construction_switch() {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.prefs_mut().snap.enabled = false;
    let click = |app: &mut App, x: f64, y: f64| {
        let at = Pointer {
            at_mm: Point::new(x, y),
            shift: false,
            alt: false,
            command: false,
        };
        app.tool_event(Event::Press(at));
        app.tool_event(Event::Release(at));
    };
    for drawing in [Drawing::Rectangle, Drawing::Line, Drawing::Gear] {
        app.arm(Tool::Draw(drawing));
        let bar = app.tool_bar().unwrap();
        let switch = bar
            .choices
            .iter()
            .find(|choice| choice.label == "Construction")
            .unwrap_or_else(|| panic!("{drawing:?} has the switch"));
        let options: Vec<&str> = switch.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!((options, switch.selected), (vec!["Off", "On"], 0));
    }
    app.arm(Tool::Draw(Drawing::Rectangle));
    app.tool_event(Event::Option(ToolOption::Construction(true)));
    assert_eq!(
        app.tool_bar()
            .unwrap()
            .choices
            .iter()
            .find(|choice| choice.label == "Construction")
            .unwrap()
            .selected,
        1
    );
    click(&mut app, 10.0, 10.0);
    click(&mut app, 40.0, 30.0);
    assert!(app.project().instances[0].construction);
    // It stays on for the next shape, and across tools.
    app.arm(Tool::Draw(Drawing::Circle));
    click(&mut app, 60.0, 60.0);
    click(&mut app, 70.0, 60.0);
    assert!(app.project().instances[1].construction);
    app.tool_event(Event::Option(ToolOption::Construction(false)));
    click(&mut app, 80.0, 20.0);
    click(&mut app, 85.0, 20.0);
    assert!(!app.project().instances[2].construction);
}
