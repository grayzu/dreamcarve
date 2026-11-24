// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/definitionEditing.test.ts
//   src/platform/featureClipboard.test.ts

//! Slice 18 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Organising"): Make Unique, cut, copy and paste, and the linked copy an
//! Alt-drag leaves (functional capability 98).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

use dc_app::command::{Edit, NewShape};
use dc_app::test_support::Harness;
use dc_app::tool::SizeField;
use dc_app::{App, Command, CommandError, Selection};
use dc_geometry::{Point, Polyline, Rect, Transform};
use dc_project::{DefinitionId, InstanceId, Project};

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn rectangle(name: &str, (x, y): (f64, f64), (w, h): (f64, f64)) -> NewShape {
    NewShape {
        name: name.to_owned(),
        shape: None,
        profiles: vec![Polyline::rect(Point::new(x, y), w, h)],
        transform: Transform::IDENTITY,
        construction: false,
        group: None,
    }
}

/// A blank project with `shapes`, those named `selected` selected.
fn app_with(shapes: Vec<NewShape>, selected: &[&str]) -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.run(Command::AddShapes(shapes)).unwrap();
    app.select(Selection::Instances(
        selected.iter().map(|name| id(name)).collect(),
    ));
    app
}

/// A square, `i1`, and its linked copy `i2`, 10 mm off.
fn linked() -> App {
    let mut app = app_with(vec![rectangle("square", (10.0, 10.0), (20.0, 20.0))], &[]);
    app.run(Command::DuplicateInstances(vec![id("i1")]))
        .unwrap();
    app.select(Selection::Nothing);
    app
}

fn definition_of(project: &Project, instance: &str) -> DefinitionId {
    project
        .instances
        .iter()
        .find(|i| i.id.as_str() == instance)
        .unwrap()
        .definition
        .clone()
}

/// Where instance `instance` lies in the project.
fn bounds_of(project: &Project, instance: &str) -> Rect {
    let placed = project
        .instances
        .iter()
        .find(|i| i.id.as_str() == instance)
        .unwrap();
    placed
        .transform
        .apply_polyline(&project.definitions[&placed.definition].profiles[0])
        .unwrap()
        .bounds()
        .unwrap()
}

fn names(app: &App) -> Vec<&str> {
    app.project()
        .instances
        .iter()
        .map(|i| i.name.as_str())
        .collect()
}

fn selected(app: &App) -> Vec<&str> {
    app.selection()
        .instances()
        .iter()
        .map(InstanceId::as_str)
        .collect()
}

/// Port of *makeUnique clones the definition and isolates later edits*:
/// the copy gets a definition of its own, lies where it lay, and an edit
/// of the original no longer reaches it.
#[test]
fn make_unique_clones_the_definition() {
    let mut app = linked();
    assert_eq!(
        definition_of(app.project(), "i1"),
        definition_of(app.project(), "i2")
    );
    let before = bounds_of(app.project(), "i2");
    app.select(Selection::Instances(vec![id("i2")]));
    let label = app.run(Command::MakeUnique(vec![id("i2")])).unwrap();
    assert_eq!(label, "Make “square (2)” unique");
    let project = app.project();
    assert_ne!(definition_of(project, "i1"), definition_of(project, "i2"));
    assert_eq!(project.definitions.len(), 2);
    assert_eq!(bounds_of(project, "i2"), before, "it lies where it lay");
    assert_eq!(selected(&app), ["i2"], "the copy stays selected");

    // An edit of the original is its own now.
    app.run(Command::SetOutline {
        instance: id("i1"),
        shape: None,
        profiles: vec![Polyline::rect(Point::new(0.0, 0.0), 5.0, 5.0)],
        what: Edit::Size(SizeField::Width),
    })
    .unwrap();
    assert_eq!(bounds_of(app.project(), "i2"), before);

    // A shape that is no linked copy is left alone, and said.
    let refused = app.run(Command::MakeUnique(vec![id("i2")]));
    assert!(
        matches!(refused, Err(CommandError::NotLinked)),
        "{refused:?}"
    );
    assert_eq!(
        refused.unwrap_err().to_string(),
        "nothing selected is a linked copy"
    );
}

/// Every copy selected: each but the last gets a definition, the last
/// keeps the one they shared, which nothing else places any more.
#[test]
fn make_unique_of_all_the_copies_keeps_one_definition() {
    let mut app = linked();
    app.run(Command::DuplicateInstances(vec![id("i1")]))
        .unwrap();
    let label = app
        .run(Command::MakeUnique(vec![id("i1"), id("i2"), id("i3")]))
        .unwrap();
    assert_eq!(label, "Make 3 shapes unique");
    let project = app.project();
    let definitions: std::collections::BTreeSet<_> = ["i1", "i2", "i3"]
        .iter()
        .map(|i| definition_of(project, i))
        .collect();
    assert_eq!(definitions.len(), 3);
    assert_eq!(project.definitions.len(), 3, "none is left unused");
    assert!(app.undo().is_some());
    assert_eq!(app.project().definitions.len(), 1, "one undo step");
}

/// Port of *copy should not dirty the project* and *cut should delete
/// the original feature* with one history entry.
#[test]
fn copy_changes_nothing_and_cut_is_one_step() {
    let mut app = app_with(
        vec![
            rectangle("square", (10.0, 10.0), (20.0, 20.0)),
            rectangle("panel", (50.0, 50.0), (30.0, 10.0)),
        ],
        &[],
    );
    assert!(!app.can_paste());
    assert_eq!(app.copy_selection(), 0, "nothing selected, nothing copied");
    assert!(!app.can_paste());
    let steps = app.undo_label().map(str::to_owned);
    app.select(Selection::Instances(vec![id("i1")]));
    assert_eq!(app.copy_selection(), 1);
    assert!(app.can_paste());
    assert_eq!(app.undo_label().map(str::to_owned), steps, "no undo step");
    assert_eq!(names(&app), ["square", "panel"]);

    app.select(Selection::Instances(vec![id("i2")]));
    assert_eq!(app.cut_selection().unwrap(), "Cut “panel”");
    assert_eq!(names(&app), ["square"]);
    assert_eq!(app.project().definitions.len(), 1);
    // The clipboard holds what was cut, though the project no longer does.
    assert_eq!(app.paste(None).unwrap(), "Paste “panel”");
    assert_eq!(names(&app), ["square", "panel"], "its name was free again");
    let pasted = selected(&app)[0].to_owned();
    let at = bounds_of(app.project(), &pasted);
    assert_eq!((at.min.x, at.min.y), (50.0, 50.0), "a cut pastes in place");
    assert!(app.undo().is_some());
    assert_eq!(names(&app), ["square"]);
    assert!(app.undo().is_some());
    assert_eq!(names(&app), ["square", "panel"]);

    // Nothing selected: nothing to cut.
    app.select(Selection::Nothing);
    assert!(matches!(
        app.cut_selection(),
        Err(CommandError::NothingSelected)
    ));
}

/// Port of *paste should create one feature per clipboard feature* and
/// *independent paste should assign a cloned definition id*: pasted
/// shapes are new, selected, shapes of their own, one undo step; with no
/// pointer they land 10 mm off the originals, and 10 mm further each
/// time; with a pointer, their middle is under it. The clipboard
/// outlives the project.
#[test]
fn paste_is_unlinked_and_crosses_projects() {
    let mut app = linked();
    app.select(Selection::Instances(vec![id("i1"), id("i2")]));
    assert_eq!(app.copy_selection(), 2);
    assert_eq!(app.paste(None).unwrap(), "Paste 2 shapes");
    let project = app.project();
    assert_eq!(project.instances.len(), 4);
    assert_eq!(selected(&app), ["i3", "i4"]);
    assert_eq!(
        names(&app),
        ["square", "square (2)", "square (3)", "square (4)"]
    );
    let definitions: std::collections::BTreeSet<_> = ["i1", "i3", "i4"]
        .iter()
        .map(|i| definition_of(project, i))
        .collect();
    assert_eq!(definitions.len(), 3, "each pasted shape is its own");
    let (first, copy) = (bounds_of(project, "i1"), bounds_of(project, "i3"));
    assert_eq!(
        (copy.min.x - first.min.x, copy.min.y - first.min.y),
        (10.0, 10.0)
    );
    assert_eq!(copy.width_mm(), first.width_mm());
    // Again: 10 mm further, so the copies do not hide each other.
    app.paste(None).unwrap();
    let again = bounds_of(app.project(), "i5");
    assert_eq!(
        (again.min.x - first.min.x, again.min.y - first.min.y),
        (20.0, 20.0)
    );
    assert!(app.undo().is_some());
    assert!(app.undo().is_some());
    assert_eq!(app.project().instances.len(), 2, "a paste is one step");

    // Under the pointer: the middle of what was copied goes there. The
    // two squares cover (10, 10) to (40, 40).
    app.paste(Some(Point::new(70.0, 60.0))).unwrap();
    let project = app.project();
    let (a, b) = (bounds_of(project, "i3"), bounds_of(project, "i4"));
    assert_eq!((a.min.x, a.min.y), (55.0, 45.0));
    assert_eq!((b.max.x, b.max.y), (85.0, 75.0));

    // Another project in the same window: the clipboard is the app's.
    let other = dc_project::save(&App::new_project().unwrap()).unwrap();
    app.open(other.as_bytes(), None).unwrap();
    assert!(app.project().instances.is_empty());
    assert!(app.can_paste());
    assert_eq!(app.paste(None).unwrap(), "Paste 2 shapes");
    assert_eq!(names(&app), ["square", "square (2)"], "the names are free");
    assert_eq!(app.project().definitions.len(), 2);
}

/// What is pasted is visible and unlocked whatever the original was, and
/// keeps its construction mark.
#[test]
fn a_pasted_shape_is_visible_and_unlocked() {
    let mut construction = rectangle("guide", (10.0, 10.0), (20.0, 20.0));
    construction.construction = true;
    let mut app = app_with(vec![construction], &["i1"]);
    app.run(Command::SetInstanceFlags {
        ids: vec![id("i1")],
        visible: Some(false),
        locked: Some(true),
    })
    .unwrap();
    app.select(Selection::Instances(vec![id("i1")]));
    assert_eq!(app.copy_selection(), 1);
    app.paste(None).unwrap();
    let pasted = app.project().instances.last().unwrap();
    assert!(pasted.visible && !pasted.locked && pasted.construction);
    assert_eq!(pasted.name, "guide (2)");
}

/// An Alt-drag leaves a linked copy where the drag ended and the
/// original where it was: one step, the copy selected.
#[test]
fn copy_instances_places_linked_copies() {
    let mut app = app_with(
        vec![rectangle("square", (10.0, 10.0), (20.0, 20.0))],
        &["i1"],
    );
    let label = app
        .run(Command::CopyInstances {
            ids: vec![id("i1")],
            placements: vec![Transform::translate(40.0, 5.0)],
            group: None,
        })
        .unwrap();
    assert_eq!(label, "Copy “square”");
    let project = app.project();
    assert_eq!(names(&app), ["square", "square (2)"]);
    assert_eq!(definition_of(project, "i1"), definition_of(project, "i2"));
    let (original, copy) = (bounds_of(project, "i1"), bounds_of(project, "i2"));
    assert_eq!((original.min.x, original.min.y), (10.0, 10.0));
    assert_eq!((copy.min.x, copy.min.y), (50.0, 15.0));
    assert_eq!(selected(&app), ["i2"]);

    // Several placements: a set of copies for each, in order.
    app.select(Selection::Instances(vec![id("i1")]));
    app.run(Command::CopyInstances {
        ids: vec![id("i1")],
        placements: vec![
            Transform::translate(0.0, 30.0),
            Transform::translate(0.0, 60.0),
        ],
        group: None,
    })
    .unwrap();
    assert_eq!(
        names(&app),
        ["square", "square (2)", "square (3)", "square (4)"]
    );
    assert_eq!(bounds_of(app.project(), "i4").min.y, 70.0);
    assert_eq!(selected(&app), ["i3", "i4"]);

    // No placement, nothing to do.
    assert!(matches!(
        app.run(Command::CopyInstances {
            ids: vec![id("i1")],
            placements: Vec::new(),
            group: None,
        }),
        Err(CommandError::NothingToDo)
    ));
}

/// The context menu of a Design Tree row and of a shape on the Viewport:
/// Cut and Copy for any shape, Paste when something was copied, Make
/// Unique for a linked copy, Convert to Path for a shape that keeps its
/// sizes.
#[test]
fn the_context_menu_offers_what_applies() {
    use dc_app::tree::{self, MenuItem};
    let mut app = linked();
    let items = |app: &App, ids: &[&str]| {
        let ids: Vec<InstanceId> = ids.iter().map(|i| id(i)).collect();
        tree::menu(app.project(), &ids, None, app.can_paste())
    };
    assert_eq!(
        items(&app, &["i1"]),
        [
            MenuItem::Rename,
            MenuItem::Hide,
            MenuItem::Lock,
            MenuItem::Cut,
            MenuItem::Copy,
            MenuItem::Duplicate,
            MenuItem::Array,
            MenuItem::Delete,
            MenuItem::Group,
            MenuItem::MakeUnique,
            MenuItem::Construction,
            MenuItem::ZoomTo,
            MenuItem::AddOperation,
        ]
    );
    app.select(Selection::Instances(vec![id("i1")]));
    app.copy_selection();
    let with_paste = items(&app, &["i1"]);
    assert_eq!(
        with_paste[3..6],
        [MenuItem::Cut, MenuItem::Copy, MenuItem::Paste]
    );
    let unique = tree::menu_command(app.project(), MenuItem::MakeUnique, &[id("i1")]).unwrap();
    assert_eq!(unique, Command::MakeUnique(vec![id("i1")]));
    app.run(unique).unwrap();
    assert!(!items(&app, &["i1"]).contains(&MenuItem::MakeUnique));
    assert!(!items(&app, &["i1"]).contains(&MenuItem::ConvertToPath));
    // Cut, Copy and Paste are the app's, not commands on the project.
    for item in [MenuItem::Cut, MenuItem::Copy, MenuItem::Paste] {
        assert_eq!(tree::menu_command(app.project(), item, &[id("i1")]), None);
    }

    // The drawing project's slot keeps its sizes.
    let drawing = Harness::from_project(dc_project::test_support::drawing_project()).app;
    let slot = tree::menu(drawing.project(), &[id("i9")], None, false);
    assert!(slot.contains(&MenuItem::ConvertToPath) && !slot.contains(&MenuItem::MakeUnique));
    assert_eq!(
        tree::menu_command(drawing.project(), MenuItem::ConvertToPath, &[id("i9")]),
        Some(Command::ConvertToPath(vec![id("i9")]))
    );
}
