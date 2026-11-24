// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/folderGroup.test.ts

//! Groups (functional capability 99, as the maintainer's review of
//! 2026-11-23 merged it): one thing files shapes in the Shapes list and
//! keeps them together on the Viewport. It was two, a folder and a
//! group; a folder is now a group, and whether its shapes are selected
//! together is a switch on it, on when it is made. The file and the code
//! call it a group too (`groups`, an instance's `group`, `together`).

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
use dc_project::{GroupId, InstanceId};

fn id(s: &str) -> InstanceId {
    InstanceId::new(s)
}

fn gid(s: &str) -> GroupId {
    GroupId::new(s)
}

fn rectangle(name: &str, x: f64) -> NewShape {
    NewShape {
        name: name.to_owned(),
        shape: None,
        profiles: vec![Polyline::rect(Point::new(x, 10.0), 10.0, 10.0)],
        transform: Transform::IDENTITY,
        construction: false,
        group: None,
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

fn group_of(app: &App, instance: &str) -> Option<String> {
    app.project()
        .instances
        .iter()
        .find(|i| i.id.as_str() == instance)
        .unwrap()
        .group
        .as_ref()
        .map(|f| f.as_str().to_owned())
}

/// The Shapes section as it is listed: a group as `{name}`, one whose
/// shapes are selected one at a time as `[name]`, a row in a group
/// indented.
fn listed(app: &App) -> Vec<String> {
    let tree = Tree::build(app.project());
    let mut out = Vec::new();
    for row in tree.rows() {
        match row {
            Row::Group(f) if f.together => out.push(format!("{{{}}}", f.name)),
            Row::Group(f) => out.push(format!("[{}]", f.name)),
            Row::Shape(s) if s.group.is_some() => out.push(format!("  {}", s.name)),
            Row::Shape(s) => out.push(s.name.clone()),
            _ => {}
        }
    }
    out
}

/// *New Group*, or Arrange ▸ Group, of `ids`.
fn group(ids: &[&str]) -> Command {
    Command::NewGroup {
        name: "Group 1".to_owned(),
        ids: ids.iter().map(|i| id(i)).collect(),
        together: true,
    }
}

/// A group holds the selection: its rows are gathered where the first of
/// them was, in their order, and the group is listed there, its shapes
/// selected together. One undo step. A group may be made of one shape,
/// or of none, to be filled.
#[test]
fn a_new_group_holds_the_selection() {
    let mut app = four();
    assert_eq!(tree::next_group_name(app.project()), "Group 1");
    let label = app.run(group(&["i3", "i1"])).unwrap();
    assert_eq!(label, "Group 2 shapes");
    let project = app.project();
    assert_eq!(project.groups.len(), 1);
    let made = &project.groups[0];
    assert_eq!(
        (made.id.as_str(), made.name.as_str(), made.together),
        ("g1", "Group 1", true)
    );
    assert_eq!(group_of(&app, "i1").as_deref(), Some("g1"));
    assert_eq!(group_of(&app, "i3").as_deref(), Some("g1"));
    assert_eq!(group_of(&app, "i2"), None);
    assert_eq!(names(&app), ["A", "C", "B", "D"], "gathered, in file order");
    assert_eq!(listed(&app), ["{Group 1}", "  A", "  C", "B", "D"]);
    assert_eq!(tree::next_group_name(app.project()), "Group 2");

    assert!(app.undo().is_some());
    assert!(app.project().groups.is_empty());
    assert_eq!(names(&app), ["A", "B", "C", "D"]);

    // One shape is a group too.
    assert_eq!(app.run(group(&["i2"])).unwrap(), "Group “B”");
    assert_eq!(listed(&app), ["A", "{Group 1}", "  B", "C", "D"]);
    app.undo();
    // With nothing selected the group is empty, listed after the shapes,
    // and stays until it is filled.
    assert_eq!(app.run(group(&[])).unwrap(), "New group “Group 1”");
    assert_eq!(listed(&app), ["A", "B", "C", "D", "{Group 1}"]);
    app.run(Command::SetInstanceFlags {
        ids: vec![id("i1")],
        visible: Some(false),
        locked: None,
    })
    .unwrap();
    assert_eq!(app.project().groups.len(), 1, "another change leaves it");
}

/// A group is shown where its first row is with its rows under it, its
/// eye and padlock the rows' common state; a row filed in a group that
/// is not there is refused.
#[test]
fn groups_file_rows() {
    let mut app = four();
    app.run(group(&["i2", "i3"])).unwrap();
    assert_eq!(listed(&app), ["A", "{Group 1}", "  B", "  C", "D"]);
    let tree = Tree::build(app.project());
    let row = &tree.groups[0];
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
    assert_eq!(Tree::build(app.project()).groups[0].visible, None, "mixed");

    // A copy of a row in a group is filed with it, beside the group's
    // other rows, not at the end of the file.
    app.run(Command::DuplicateInstances(vec![id("i3")]))
        .unwrap();
    assert_eq!(
        listed(&app),
        ["A", "{Group 1}", "  B", "  C", "  C (2)", "D"]
    );
    assert_eq!(names(&app), ["A", "B", "C", "C (2)", "D"]);

    let nowhere = Command::MoveRows {
        ids: vec![id("i1")],
        before: None,
        group: Some(gid("g9")),
    };
    assert!(matches!(
        app.run(nowhere),
        Err(CommandError::UnknownGroup(_))
    ));
}

/// Rows drag within the tree, into a group and out of one: the order is
/// the file's. A group whose last row is dragged out goes with it.
#[test]
fn rows_reorder_the_file() {
    let mut app = four();
    let moved = |ids: &[&str], before: Option<&str>, to: Option<&str>| Command::MoveRows {
        ids: ids.iter().map(|i| id(i)).collect(),
        before: before.map(id),
        group: to.map(gid),
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

    // Into a group, before one of its rows; and out again.
    app.run(group(&["i2", "i3"])).unwrap();
    assert_eq!(listed(&app), ["{Group 1}", "  B", "  C", "D", "A"]);
    let label = app.run(moved(&["i1"], Some("i3"), Some("g1"))).unwrap();
    assert_eq!(label, "Move “A” to “Group 1”");
    assert_eq!(listed(&app), ["{Group 1}", "  B", "  A", "  C", "D"]);
    // Onto the group itself: its last row.
    app.run(moved(&["i4"], None, Some("g1"))).unwrap();
    assert_eq!(listed(&app), ["{Group 1}", "  B", "  A", "  C", "  D"]);
    app.run(moved(&["i2"], None, None)).unwrap();
    assert_eq!(listed(&app), ["{Group 1}", "  A", "  C", "  D", "B"]);
    assert_eq!(group_of(&app, "i2"), None);
    // Every row out: the group goes with the last of them.
    app.run(moved(&["i1", "i3", "i4"], Some("i2"), None))
        .unwrap();
    assert_eq!(listed(&app), ["A", "C", "D", "B"]);
    assert!(app.project().groups.is_empty());
}

/// Port of *groupSelectedFeaturesIntoNewFolder creates a new grouped
/// group* and of the selection tests: a group's shapes are selected
/// together from the Viewport; with *Select Together* off they are
/// selected one at a time and the group only files them; Ungroup keeps
/// the shapes.
#[test]
fn a_group_selects_as_one() {
    let mut app = four();
    let label = app.run(group(&["i2", "i3"])).unwrap();
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
    assert_eq!(tree::groups_of(project, &[id("i2"), id("i1")]), [gid("g1")]);
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

    // Select Together off: the group still files its shapes, and each is
    // selected on its own. It is one undo step, and on again is another.
    let apart = Command::SetGroupTogether {
        id: gid("g1"),
        together: false,
    };
    assert_eq!(
        app.run(apart.clone()).unwrap(),
        "Select the shapes of “Group 1” one at a time"
    );
    assert!(matches!(app.run(apart), Err(CommandError::NothingToDo)));
    assert_eq!(listed(&app), ["A", "[Group 1]", "  B", "  C", "D"]);
    assert_eq!(tree::with_groups(app.project(), &[id("i3")]), [id("i3")]);
    // It is still a group to ungroup.
    assert_eq!(tree::groups_of(app.project(), &[id("i3")]), [gid("g1")]);
    let together = Command::SetGroupTogether {
        id: gid("g1"),
        together: true,
    };
    assert_eq!(
        app.run(together).unwrap(),
        "Select the shapes of “Group 1” together"
    );
    assert_eq!(
        tree::with_groups(app.project(), &[id("i3")]),
        [id("i2"), id("i3")]
    );
    assert!(matches!(
        app.run(Command::SetGroupTogether {
            id: gid("g9"),
            together: false,
        }),
        Err(CommandError::UnknownGroup(_))
    ));

    // Ungroup: the group goes, its rows stay where they are.
    let label = app.run(Command::Ungroup(vec![gid("g1")])).unwrap();
    assert_eq!(label, "Ungroup “Group 1”");
    assert_eq!(listed(&app), ["A", "B", "C", "D"]);
    assert!(app.project().groups.is_empty());
}

/// Deleting a group deletes its shapes, as Delete would, in one step;
/// renaming it is one step.
#[test]
fn delete_and_rename_a_group() {
    let mut app = four();
    app.run(group(&["i2", "i3"])).unwrap();
    let label = app
        .run(Command::RenameGroup {
            id: gid("g1"),
            name: "Feet".to_owned(),
        })
        .unwrap();
    assert_eq!(label, "Rename group “Group 1”");
    assert_eq!(listed(&app), ["A", "{Feet}", "  B", "  C", "D"]);
    assert!(matches!(
        app.run(Command::RenameGroup {
            id: gid("g1"),
            name: "Feet".to_owned(),
        }),
        Err(CommandError::NothingToDo)
    ));

    let label = app.run(Command::DeleteGroup(gid("g1"))).unwrap();
    assert_eq!(label, "Delete group “Feet”");
    assert_eq!(names(&app), ["A", "D"]);
    assert!(app.project().groups.is_empty());
    assert_eq!(app.project().definitions.len(), 2);
    assert!(app.undo().is_some());
    assert_eq!(listed(&app), ["A", "{Feet}", "  B", "  C", "D"]);

    assert!(matches!(
        app.run(Command::DeleteGroup(gid("g9"))),
        Err(CommandError::UnknownGroup(_))
    ));
}

/// A group goes with its last shape, deleted or moved out, whether its
/// shapes are selected together or not; a project saved with an empty
/// one keeps it.
#[test]
fn a_group_goes_with_its_last_shape() {
    let mut app = four();
    app.run(group(&["i2", "i3"])).unwrap();
    app.run(Command::DeleteInstances(vec![id("i2")])).unwrap();
    assert_eq!(app.project().groups.len(), 1, "one shape is left in it");
    app.run(Command::DeleteInstances(vec![id("i3")])).unwrap();
    assert!(app.project().groups.is_empty());
    app.run(group(&["i1"])).unwrap();
    app.run(Command::SetGroupTogether {
        id: gid("g1"),
        together: false,
    })
    .unwrap();
    app.run(Command::DeleteInstances(vec![id("i1")])).unwrap();
    assert_eq!(listed(&app), ["D"]);
    // Grouping shapes that are another group's takes them from it, and
    // a group left empty by that goes too.
    let mut app = four();
    app.run(group(&["i1", "i2"])).unwrap();
    app.run(Command::NewGroup {
        name: "Group 2".to_owned(),
        ids: vec![id("i1"), id("i2"), id("i3")],
        together: true,
    })
    .unwrap();
    assert_eq!(listed(&app), ["{Group 2}", "  A", "  B", "  C", "D"]);
}

/// The row menu: Group for any rows, Ungroup while one of them is in a
/// group; several groups are dissolved in one step. There is no second
/// way to file rows.
#[test]
fn the_row_menu_groups_and_ungroups() {
    use dc_app::tree::MenuItem;
    let mut app = four();
    let items = |app: &App, ids: &[&str]| {
        let ids: Vec<InstanceId> = ids.iter().map(|i| id(i)).collect();
        tree::menu(app.project(), &ids, None, false)
    };
    let one = items(&app, &["i1"]);
    assert!(one.contains(&MenuItem::Group), "one shape can be grouped");
    assert!(!one.contains(&MenuItem::Ungroup));
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

    let single = tree::menu_command(app.project(), MenuItem::Group, &[id("i4")]).unwrap();
    assert_eq!(app.run(single).unwrap(), "Group “D”");
    assert_eq!(listed(&app), ["A", "B", "C", "{Group 1}", "  D"]);
}
