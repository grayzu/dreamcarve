// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/folderGroup.test.ts

//! Slice 21 of phase 5 (`docs/specs/phase-5/design.md`, "Test plan →
//! Organising"): folders and groups (functional capability 99). Folders
//! are flat; a group is a folder whose shapes are selected together.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use dc_app::command::NewShape;
use dc_app::test_support::Harness;
use dc_app::tree::{self, Row, Tree};
use dc_app::{App, ClickMode, Command, CommandError, Hit, Selection};
use dc_geometry::{Point, Polyline, Transform};
use dc_project::{FolderId, InstanceId};

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn folder(s: &str) -> FolderId {
    FolderId::new(s)
}

fn rectangle(name: &str, x: f64) -> NewShape {
    NewShape {
        name: name.to_owned(),
        shape: None,
        profiles: vec![Polyline::rect(Point::new(x, 10.0), 10.0, 10.0)],
        transform: Transform::IDENTITY,
        construction: false,
        folder: None,
    }
}

/// Four squares A, B, C and D: `i1` to `i4`.
fn four() -> App {
    let mut app = Harness::from_project(App::new_project().unwrap()).app;
    app.run(Command::AddShapes(vec![
        rectangle("A", 10.0),
        rectangle("B", 30.0),
        rectangle("C", 50.0),
        rectangle("D", 70.0),
    ]))
    .unwrap();
    app.select(Selection::Nothing);
    app
}

fn names(app: &App) -> Vec<&str> {
    app.project()
        .instances
        .iter()
        .map(|i| i.name.as_str())
        .collect()
}

fn folder_of(app: &App, instance: &str) -> Option<String> {
    app.project()
        .instances
        .iter()
        .find(|i| i.id.as_str() == instance)
        .unwrap()
        .folder
        .as_ref()
        .map(|f| f.as_str().to_owned())
}

/// The Shapes section as it is listed: a folder as `[name]`, a group as
/// `{name}`, a row in a folder indented.
fn listed(app: &App) -> Vec<String> {
    let tree = Tree::build(app.project());
    let mut out = Vec::new();
    for row in tree.rows() {
        match row {
            Row::Folder(f) if f.group => out.push(format!("{{{}}}", f.name)),
            Row::Folder(f) => out.push(format!("[{}]", f.name)),
            Row::Shape(s) if s.folder.is_some() => out.push(format!("  {}", s.name)),
            Row::Shape(s) => out.push(s.name.clone()),
            _ => {}
        }
    }
    out
}

fn new_folder(ids: &[&str], group: bool) -> Command {
    Command::NewFolder {
        name: if group { "Group 1" } else { "Folder 1" }.to_owned(),
        ids: ids.iter().map(|i| id(i)).collect(),
        group,
    }
}

/// *New Folder* makes a folder holding the selection: its rows are
/// gathered where the first of them was, in their order, and the folder
/// is listed there. One undo step.
#[test]
fn a_new_folder_holds_the_selection() {
    let mut app = four();
    assert_eq!(tree::next_folder_name(app.project(), false), "Folder 1");
    let label = app.run(new_folder(&["i3", "i1"], false)).unwrap();
    assert_eq!(label, "New folder “Folder 1”");
    let project = app.project();
    assert_eq!(project.folders.len(), 1);
    let made = &project.folders[0];
    assert_eq!(
        (made.id.as_str(), made.name.as_str(), made.group),
        ("f1", "Folder 1", false)
    );
    assert_eq!(folder_of(&app, "i1").as_deref(), Some("f1"));
    assert_eq!(folder_of(&app, "i3").as_deref(), Some("f1"));
    assert_eq!(folder_of(&app, "i2"), None);
    assert_eq!(names(&app), ["A", "C", "B", "D"], "gathered, in file order");
    assert_eq!(listed(&app), ["[Folder 1]", "  A", "  C", "B", "D"]);
    assert_eq!(tree::next_folder_name(app.project(), false), "Folder 2");
    assert_eq!(tree::next_folder_name(app.project(), true), "Group 1");

    assert!(app.undo().is_some());
    assert!(app.project().folders.is_empty());
    assert_eq!(names(&app), ["A", "B", "C", "D"]);

    // With nothing selected the folder is empty, and listed after the
    // shapes.
    app.run(new_folder(&[], false)).unwrap();
    assert_eq!(listed(&app), ["A", "B", "C", "D", "[Folder 1]"]);
}

/// A folder is shown where its first row is with its rows under it, its
/// eye and padlock the rows' common state; a row filed in a folder that
/// is not there is refused.
#[test]
fn folders_file_rows() {
    let mut app = four();
    app.run(new_folder(&["i2", "i3"], false)).unwrap();
    assert_eq!(listed(&app), ["A", "[Folder 1]", "  B", "  C", "D"]);
    let tree = Tree::build(app.project());
    let row = &tree.folders[0];
    assert_eq!(
        (row.rows.as_slice(), row.visible, row.locked),
        ([id("i2"), id("i3")].as_slice(), Some(true), Some(false))
    );
    app.run(Command::SetInstanceFlags {
        ids: vec![id("i2")],
        visible: Some(false),
        locked: None,
    })
    .unwrap();
    assert_eq!(Tree::build(app.project()).folders[0].visible, None, "mixed");

    // A copy of a row in a folder is filed with it, beside the folder's
    // other rows, not at the end of the file.
    app.run(Command::DuplicateInstances(vec![id("i3")]))
        .unwrap();
    assert_eq!(
        listed(&app),
        ["A", "[Folder 1]", "  B", "  C", "  C (2)", "D"]
    );
    assert_eq!(names(&app), ["A", "B", "C", "C (2)", "D"]);

    let nowhere = Command::MoveRows {
        ids: vec![id("i1")],
        before: None,
        folder: Some(folder("f9")),
    };
    assert!(matches!(
        app.run(nowhere),
        Err(CommandError::UnknownFolder(_))
    ));
}

/// Rows drag within the tree, into a folder and out of one: the order
/// is the file's.
#[test]
fn rows_reorder_the_file() {
    let mut app = four();
    let moved = |ids: &[&str], before: Option<&str>, to: Option<&str>| Command::MoveRows {
        ids: ids.iter().map(|i| id(i)).collect(),
        before: before.map(id),
        folder: to.map(folder),
    };
    let label = app.run(moved(&["i4"], Some("i1"), None)).unwrap();
    assert_eq!(label, "Reorder “D”");
    assert_eq!(names(&app), ["D", "A", "B", "C"]);
    // To the end; several rows keep their order.
    app.run(moved(&["i1", "i4"], None, None)).unwrap();
    assert_eq!(names(&app), ["B", "C", "D", "A"]);
    // Where it is already: nothing to do.
    assert!(matches!(
        app.run(moved(&["i1"], None, None)),
        Err(CommandError::NothingToDo)
    ));

    // Into a folder, before one of its rows; and out again.
    app.run(new_folder(&["i2", "i3"], false)).unwrap();
    assert_eq!(listed(&app), ["[Folder 1]", "  B", "  C", "D", "A"]);
    let label = app.run(moved(&["i1"], Some("i3"), Some("f1"))).unwrap();
    assert_eq!(label, "Move “A” to “Folder 1”");
    assert_eq!(listed(&app), ["[Folder 1]", "  B", "  A", "  C", "D"]);
    // Onto the folder itself: its last row.
    app.run(moved(&["i4"], None, Some("f1"))).unwrap();
    assert_eq!(listed(&app), ["[Folder 1]", "  B", "  A", "  C", "  D"]);
    app.run(moved(&["i2"], None, None)).unwrap();
    assert_eq!(listed(&app), ["[Folder 1]", "  A", "  C", "  D", "B"]);
    assert_eq!(folder_of(&app, "i2"), None);
    // Every row out: a plain folder stays, empty.
    app.run(moved(&["i1", "i3", "i4"], Some("i2"), None))
        .unwrap();
    assert_eq!(listed(&app), ["A", "C", "D", "B", "[Folder 1]"]);
}

/// Port of *groupSelectedFeaturesIntoNewFolder creates a new grouped
/// folder* and of the selection tests: a group's shapes are selected
/// together from the Viewport; Ungroup keeps the shapes; a group left
/// with no shape goes.
#[test]
fn a_group_selects_as_one() {
    let mut app = four();
    let label = app.run(new_folder(&["i2", "i3"], true)).unwrap();
    assert_eq!(label, "Group 2 shapes");
    assert_eq!(listed(&app), ["A", "{Group 1}", "  B", "  C", "D"]);
    let project = app.project();
    assert_eq!(
        tree::with_groups(project, &[id("i3")]),
        [id("i2"), id("i3")]
    );
    assert_eq!(tree::with_groups(project, &[id("i1")]), [id("i1")]);
    assert_eq!(
        tree::with_groups(project, &[id("i4"), id("i2"), id("i3")]),
        [id("i4"), id("i2"), id("i3")],
        "each shape once"
    );
    assert_eq!(
        tree::groups_of(project, &[id("i2"), id("i1")]),
        [folder("f1")]
    );
    assert!(tree::groups_of(project, &[id("i1")]).is_empty());

    // A click on one of them on the Viewport is a click on the group.
    let clicked = |held: &[&str], on: &str, mode| {
        let held = Selection::Instances(held.iter().map(|i| id(i)).collect());
        held.clicked_in(project, &Hit::Instance(id(on)), mode)
            .instances()
            .iter()
            .map(|i| i.as_str().to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(clicked(&["i1"], "i3", ClickMode::Replace), ["i2", "i3"]);
    assert_eq!(clicked(&["i1"], "i3", ClickMode::Add), ["i1", "i2", "i3"]);
    assert_eq!(
        clicked(&["i1", "i2"], "i3", ClickMode::Toggle),
        ["i1", "i2", "i3"]
    );
    assert_eq!(
        clicked(&["i2", "i1", "i3"], "i3", ClickMode::Toggle),
        ["i1"]
    );
    assert_eq!(clicked(&["i2"], "i1", ClickMode::Replace), ["i1"]);
    let boxed = Selection::Nothing.boxed_in(project, &[id("i2"), id("i4")], ClickMode::Replace);
    assert_eq!(boxed.instances(), [id("i2"), id("i3"), id("i4")]);

    // A plain folder's shapes are selected one at a time.
    app.run(new_folder(&["i1", "i4"], false)).unwrap();
    assert_eq!(tree::with_groups(app.project(), &[id("i1")]), [id("i1")]);
    app.undo();

    // Ungroup: the folder goes, its rows stay where they are.
    let label = app.run(Command::Ungroup(vec![folder("f1")])).unwrap();
    assert_eq!(label, "Ungroup “Group 1”");
    assert_eq!(listed(&app), ["A", "B", "C", "D"]);
    assert!(app.project().folders.is_empty());
    app.undo();

    // The last shape dragged out of a group: the group goes with it.
    app.run(Command::MoveRows {
        ids: vec![id("i2"), id("i3")],
        before: None,
        folder: None,
    })
    .unwrap();
    assert_eq!(listed(&app), ["A", "D", "B", "C"]);
    assert!(app.project().folders.is_empty());
}

/// Deleting a folder deletes its shapes, as Delete would, in one step;
/// renaming it is one step.
#[test]
fn delete_and_rename_a_folder() {
    let mut app = four();
    app.run(new_folder(&["i2", "i3"], false)).unwrap();
    let label = app
        .run(Command::RenameFolder {
            id: folder("f1"),
            name: "Feet".to_owned(),
        })
        .unwrap();
    assert_eq!(label, "Rename folder “Folder 1”");
    assert_eq!(listed(&app), ["A", "[Feet]", "  B", "  C", "D"]);
    assert!(matches!(
        app.run(Command::RenameFolder {
            id: folder("f1"),
            name: "Feet".to_owned(),
        }),
        Err(CommandError::NothingToDo)
    ));

    let label = app.run(Command::DeleteFolder(folder("f1"))).unwrap();
    assert_eq!(label, "Delete folder “Feet”");
    assert_eq!(names(&app), ["A", "D"]);
    assert!(app.project().folders.is_empty());
    assert_eq!(app.project().definitions.len(), 2);
    assert!(app.undo().is_some());
    assert_eq!(listed(&app), ["A", "[Feet]", "  B", "  C", "D"]);

    assert!(matches!(
        app.run(Command::DeleteFolder(folder("f9"))),
        Err(CommandError::UnknownFolder(_))
    ));
}

/// Deleting every shape of a group deletes the group; a plain folder
/// emptied by Delete stays.
#[test]
fn a_group_goes_with_its_last_shape() {
    let mut app = four();
    app.run(new_folder(&["i2", "i3"], true)).unwrap();
    app.run(Command::DeleteInstances(vec![id("i2"), id("i3")]))
        .unwrap();
    assert!(app.project().folders.is_empty());
    app.run(new_folder(&["i1"], false)).unwrap();
    app.run(Command::DeleteInstances(vec![id("i1")])).unwrap();
    assert_eq!(listed(&app), ["D", "[Folder 1]"]);
}

/// The row menu's folder items: New Folder for any rows, Group for two
/// or more, Ungroup while one of them is in a group; several groups are
/// dissolved in one step.
#[test]
fn the_row_menu_makes_folders_and_groups() {
    use dc_app::tree::MenuItem;
    let mut app = four();
    let items = |app: &App, ids: &[&str]| {
        let ids: Vec<InstanceId> = ids.iter().map(|i| id(i)).collect();
        tree::menu(app.project(), &ids, None, false)
    };
    let one = items(&app, &["i1"]);
    assert!(one.contains(&MenuItem::NewFolder));
    assert!(!one.contains(&MenuItem::Group) && !one.contains(&MenuItem::Ungroup));
    assert!(items(&app, &["i1", "i2"]).contains(&MenuItem::Group));

    let both = [id("i1"), id("i2")];
    let group = tree::menu_command(app.project(), MenuItem::Group, &both).unwrap();
    assert_eq!(app.run(group).unwrap(), "Group 2 shapes");
    let rest = [id("i3"), id("i4")];
    let group = tree::menu_command(app.project(), MenuItem::Group, &rest).unwrap();
    app.run(group).unwrap();
    assert_eq!(
        listed(&app),
        ["{Group 1}", "  A", "  B", "{Group 2}", "  C", "  D"]
    );
    assert!(items(&app, &["i1"]).contains(&MenuItem::Ungroup));

    let across = [id("i2"), id("i3")];
    let ungroup = tree::menu_command(app.project(), MenuItem::Ungroup, &across).unwrap();
    assert_eq!(app.run(ungroup).unwrap(), "Ungroup 2 groups");
    assert_eq!(listed(&app), ["A", "B", "C", "D"]);

    let folder = tree::menu_command(app.project(), MenuItem::NewFolder, &[id("i4")]).unwrap();
    assert_eq!(app.run(folder).unwrap(), "New folder “Folder 1”");
    assert_eq!(listed(&app), ["A", "B", "C", "[Folder 1]", "  D"]);
}
