// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The quick start "Your first design" done in the window (phase 5
//! functional spec, exit criteria: "the hook plate drawn in the app with
//! no import"). The chapter's `draw` and `arrange` tables
//! (`crates/dreamcarve/tests/design_steps`) are followed as the reader
//! follows them: a tool chosen from the Draw button's popover, its
//! options clicked or typed in the tool bar, its start and sizes typed
//! into the bar's fields with Tab between and Enter after, **Done**
//! pressed; a shape selected by its row, *Arrange ▸ Array…* and *Arrange
//! ▸ Align* from the menu bar, and X or Y typed in Properties.
//!
//! Shared by `first_design.rs`, which holds the window's project to the
//! one the steps build with no window, and by `book.rs`, which takes the
//! chapter's pictures on the way: `stage` is called at each point a
//! picture can be taken, with the point's name.

#![allow(dead_code, reason = "each test binary uses the helpers it needs")]

use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable as _;

use super::common;
use super::design_steps::{self, CHAPTER};

type H = Harness<'static, dreamcarve_app::Shell>;

/// Frames enough for the tool bar to settle after it changes.
const SETTLE: usize = 12;

/// A point at which the chapter can show a picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage<'a> {
    /// The tool is armed and its options are set; nothing is typed.
    Armed(&'a str),
    /// The shape's sizes are typed and not yet entered: the preview.
    Typed(&'a str),
    /// The shape is drawn.
    Drawn(&'a str),
    /// A command's values are set in the tool bar and not yet done.
    Set(&'a str),
    /// The command is done.
    Arranged(&'a str),
}

/// Takes the pointer off the window: a typed size goes the way a tool
/// goes with no pointer, and no control shows its hover.
fn park(h: &mut H) {
    h.event(egui::Event::PointerGone);
    h.run_steps(2);
}

/// Types `text` over what the field `label` holds; it is written when
/// the field is left.
fn retype(h: &mut H, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run_steps(2);
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run_steps(2);
    h.event(egui::Event::Text(text.to_owned()));
    h.run_steps(2);
}

/// Sets one thing in the tool bar: a field typed and left with Tab, a
/// choice clicked, a list's row picked, a text's words typed.
fn set_in_bar(h: &mut H, label: &str, value: &str) {
    let bar = h
        .state()
        .app()
        .tool_bar()
        .unwrap_or_else(|| panic!("{CHAPTER}: no tool is armed to set {label}"));
    if bar.fields.iter().any(|row| row.label == label) {
        retype(h, label, value);
        h.key_press(egui::Key::Tab);
        h.run_steps(SETTLE);
    } else if let Some(choice) = bar.choices.iter().find(|choice| choice.label == label) {
        if choice.list {
            h.get_by_role_and_label(Role::ComboBox, label).click();
            h.run_steps(SETTLE);
            let row = h.get_by_label(value);
            row.scroll_to_me();
            h.run_steps(2);
            h.get_by_label(value).click();
        } else {
            h.get_by_role_and_label(Role::RadioButton, value).click();
        }
        h.run_steps(SETTLE);
    } else if bar.text.as_ref().is_some_and(|row| row.label == label) {
        h.get_by_role_and_label(Role::MultilineTextInput, label)
            .type_text(value);
        h.run_steps(SETTLE);
    } else {
        panic!("{CHAPTER}: the tool bar has no `{label}`");
    }
}

/// Types a group of the bar's fields: Tab leaves each, which writes it,
/// and Enter in the last enters the group, as the chapter says to.
/// `before_enter` runs with every value written and none entered, so the
/// preview is the shape that Enter will make.
fn type_group(h: &mut H, pairs: &[(String, String)], before_enter: &mut dyn FnMut(&mut H)) {
    let Some((last_label, _)) = pairs.last() else {
        return;
    };
    for (label, value) in pairs {
        set_in_bar(h, label, value);
    }
    park(h);
    before_enter(h);
    let bar = h.state().app().tool_bar().expect("a tool is armed");
    // The words of a text, or a choice, have nothing to enter.
    if bar.fields.iter().any(|row| &row.label == last_label) {
        h.get_by_role_and_label(Role::TextInput, last_label).focus();
        h.run_steps(2);
        h.key_press(egui::Key::Enter);
        h.run_steps(SETTLE);
    }
}

/// Draws one row of the chapter's `draw` table in the window.
pub fn draw(h: &mut H, row: &[String], stage: &mut dyn FnMut(&mut H, Stage<'_>)) {
    let [shape, tool, first, start, then] = row else {
        panic!("{CHAPTER}: a drawn shape's row has five cells");
    };
    let shape = design_steps::names(shape).remove(0);
    let before = h.state().app().project().instances.len();
    // The tool, from the tool strip: its group's button, then its row
    // in the list beside it; Text has a button of its own.
    let strings = dc_app::Strings::english();
    let group = dc_app::Drawing::ALL
        .into_iter()
        .find(|drawing| strings.get(drawing.name()) == tool)
        .unwrap_or_else(|| panic!("{CHAPTER}: there is no drawing tool `{tool}`"))
        .group();
    if let Some(group) = group {
        h.get_by_role_and_label(Role::Button, strings.get(group.name()))
            .click();
        h.run_steps(SETTLE);
    }
    h.get_by_role_and_label(Role::Button, tool).click();
    h.run_steps(SETTLE);
    park(h);
    for (label, value) in design_steps::settings(first) {
        set_in_bar(h, &label, &value);
    }
    park(h);
    stage(h, Stage::Armed(&shape));
    // Where it starts: typed, or clicked where the tool takes no typing.
    let start = design_steps::settings(start);
    let bar = h
        .state()
        .app()
        .tool_bar()
        .expect("a drawing tool has a bar");
    let typed = start
        .iter()
        .all(|(label, _)| bar.fields.iter().any(|row| &row.label == label));
    if typed {
        type_group(h, &start, &mut |_| {});
    } else {
        let at = common::on_screen(
            h,
            dc_geometry::Point::new(
                design_steps::number(&start, "X"),
                design_steps::number(&start, "Y"),
            ),
        );
        common::click_at(h, at, egui::Modifiers::NONE);
        h.run_steps(SETTLE);
        park(h);
    }
    type_group(h, &design_steps::settings(then), &mut |h| {
        stage(h, Stage::Typed(&shape));
    });
    if h.state().app().project().instances.len() == before {
        h.get_by_role_and_label(Role::Button, "Done").click();
        h.run_steps(SETTLE);
    }
    let made: Vec<String> = h
        .state()
        .app()
        .project()
        .instances
        .iter()
        .skip(before)
        .map(|i| i.name.clone())
        .collect();
    assert_eq!(
        made,
        std::slice::from_ref(&shape),
        "{CHAPTER}: what {tool} drew"
    );
    park(h);
    stage(h, Stage::Drawn(&shape));
}

/// Runs one row of the chapter's `arrange` table in the window.
pub fn arrange(h: &mut H, row: &[String], stage: &mut dyn FnMut(&mut H, Stage<'_>)) {
    let [shapes, command, set] = row else {
        panic!("{CHAPTER}: an arrange row has three cells");
    };
    let names = design_steps::names(shapes);
    let [shape] = &names[..] else {
        panic!("{CHAPTER}: the window's steps select one shape a row");
    };
    // Back to Select, and the shape by its row in the Design Tree.
    h.key_press(egui::Key::V);
    h.run_steps(SETTLE);
    h.get_by_role_and_label(Role::TreeItem, shape).click();
    h.run_steps(SETTLE);
    let path: Vec<String> = command
        .replace("**", "")
        .split('▸')
        .map(|part| part.trim().to_owned())
        .collect();
    for (at, item) in path.iter().enumerate() {
        h.get_by_role_and_label(Role::Button, item).click();
        h.run_steps(if at == 0 { 2 } else { SETTLE });
    }
    park(h);
    let set = design_steps::settings(set);
    if h.state().app().tool_bar().is_some() {
        // A command that takes its values in the tool bar, then Done.
        for (label, value) in &set {
            set_in_bar(h, label, value);
        }
        park(h);
        stage(h, Stage::Set(shape));
        h.get_by_role_and_label(Role::Button, "Done").click();
        h.run_steps(SETTLE);
    } else {
        // Then Properties: a field typed and entered.
        for (label, value) in &set {
            retype(h, label, value);
            h.key_press(egui::Key::Enter);
            h.run_steps(SETTLE);
        }
    }
    park(h);
    stage(h, Stage::Arranged(shape));
}

/// Draws and arranges the whole of the chapter's part in the window `h`,
/// which is on a blank project with the chapter's stock, on the Design
/// step.
pub fn draw_all(h: &mut H, text: &str, stage: &mut dyn FnMut(&mut H, Stage<'_>)) {
    for row in design_steps::draw_rows(text) {
        draw(h, &row, stage);
    }
    for row in design_steps::arrange_rows(text) {
        arrange(h, &row, stage);
    }
    h.key_press(egui::Key::V);
    h.run_steps(SETTLE);
}

/// Holds the project in the window to the one the chapter's steps build
/// with no window: the same shapes by name, in the same groups, with
/// the same sizes, each where the other is to a millionth of a
/// millimetre (a click on the Viewport is a whole pixel, and what is
/// aligned after it is placed from there).
pub fn assert_same_drawing(window: &dc_project::Project, steps: &dc_project::Project) {
    let names = |p: &dc_project::Project| -> Vec<(String, bool, Option<String>)> {
        p.instances
            .iter()
            .map(|i| {
                let group = i.group.as_ref().and_then(|id| {
                    p.groups
                        .iter()
                        .find(|f| f.id == *id)
                        .map(|f| f.name.clone())
                });
                (i.name.clone(), i.construction, group)
            })
            .collect()
    };
    assert_eq!(names(window), names(steps), "the shapes and their groups");
    assert_eq!(window.stock, steps.stock);
    for (a, b) in window.instances.iter().zip(&steps.instances) {
        let (in_window, in_steps) = (
            &window.definitions[&a.definition],
            &steps.definitions[&b.definition],
        );
        assert_eq!(in_window.shape, in_steps.shape, "{}: its sizes", a.name);
        let placed = |p: &dc_project::Project, id: &dc_project::InstanceId| {
            dc_app::properties::placement(p, std::slice::from_ref(id))
                .expect("a placed shape")
                .bounds
        };
        let (x, y) = (placed(window, &a.id), placed(steps, &b.id));
        for (got, wanted) in [
            (x.min.x, y.min.x),
            (x.min.y, y.min.y),
            (x.max.x, y.max.x),
            (x.max.y, y.max.y),
        ] {
            assert!(
                (got - wanted).abs() < 1e-6,
                "{}: at {x:?} in the window, {y:?} by the steps",
                a.name
            );
        }
    }
}
