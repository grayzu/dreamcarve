// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The steps of the quick start "Your first design", read from the
//! chapter and run on a [`dc_app::App`] (phase 5 functional spec, "Test
//! part"; acceptance 128). The chapter draws the hook plate in the app
//! with no import; its tables are the steps, as `app_steps` reads the
//! tables of "From SVG to G-code in the app", so the chapter and the
//! tests cannot drift apart.
//!
//! Beside the tables `app_steps` reads (`tools`, `part NAME`, `operations
//! NAME`; a part with no `Drawing` row starts blank, as *Draw from
//! scratch* does) this chapter has two of its own:
//!
//! - `<!-- app-test: draw NAME -->`: the shapes drawn, a row each: the
//!   name the app gives the shape, the drawing tool, what is set in the
//!   tool bar first, where the shape starts, and what is typed after.
//!   Every cell but the first two is `**Label** value` pairs with `;`
//!   between: a label is a field of the tool bar, or one of its choices
//!   with the value the option picked. *Start* is typed into **X** and
//!   **Y** and entered; a tool with no such fields (Text) takes a click
//!   there. The last cell is typed and entered, and **Done** is pressed
//!   when the tool is still waiting (a path, a text).
//! - `<!-- app-test: arrange NAME -->`: what is done to shapes already
//!   drawn, a row each: the shapes selected (code spans), the command
//!   (`Arrange ▸ Array…`, `Arrange ▸ Align ▸ Centre`), and what is set
//!   after: the Array bar's fields and choices, then **Done**; or the
//!   Properties panel's **X** and **Y**.
//!
//! Every value goes through the event or the function the window sends
//! for that control (`dc_app::tool::Event`, `dc_app::arrange::arrange`,
//! `dc_app::properties`), so a label the app does not have, or a value a
//! field refuses, fails here.

#![allow(dead_code, reason = "each test binary uses the helpers it needs")]

use std::path::{Path, PathBuf};

use dc_app::tool::{Adjust, Event, Key};
use dc_app::{App, Drawing, Pointer, Selection, StrKey, Strings, Tool};
use dc_geometry::Point;
use dc_library::Library;
use dc_project::InstanceId;

use super::app_steps::{self, table};
pub use super::app_steps::{names, settings};

/// The quick start "Your first design", relative to the repository root.
pub const CHAPTER: &str = "docs/book/src/quick-start/first-design.md";

/// The part the chapter makes.
pub const PART: &str = "hook-plate";

/// The repository root.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The chapter's text.
pub fn chapter() -> String {
    std::fs::read_to_string(repo_root().join(CHAPTER))
        .expect("the first-design quick start is readable")
}

/// Sets `label` to `value` in the armed tool's bar: a field typed, a
/// choice picked, or the words of a text.
fn set_in_bar(app: &mut App, label: &str, value: &str) {
    let bar = app
        .tool_bar()
        .unwrap_or_else(|| panic!("{CHAPTER}: no tool is armed to set {label}"));
    let event = if let Some(row) = bar.fields.iter().find(|row| row.label == label) {
        Event::Field {
            field: row.field,
            text: value.to_owned(),
        }
    } else if let Some(choice) = bar.choices.iter().find(|choice| choice.label == label) {
        let option = choice
            .options
            .iter()
            .find(|option| option.label == value)
            .unwrap_or_else(|| {
                let have: Vec<&str> = choice.options.iter().map(|o| o.label.as_str()).collect();
                panic!("{CHAPTER}: {label} has no option `{value}`; it has {have:?}")
            });
        Event::Option(option.option.clone())
    } else if bar.text.as_ref().is_some_and(|row| row.label == label) {
        Event::Text(value.to_owned())
    } else {
        let fields: Vec<&str> = bar.fields.iter().map(|row| row.label.as_str()).collect();
        let choices: Vec<&str> = bar.choices.iter().map(|c| c.label.as_str()).collect();
        panic!(
            "{CHAPTER}: the tool bar (“{}”) has no `{label}`; its fields are {fields:?} and \
             its choices {choices:?}",
            bar.prompt
        );
    };
    let reply = app.tool_event(event);
    assert_eq!(reply.refused, None, "{CHAPTER}: {label} `{value}`");
    let after = app.tool_bar().expect("the tool is still armed");
    if let Some(row) = after.fields.iter().find(|row| row.label == label) {
        assert_eq!(row.problem, None, "{CHAPTER}: {label} `{value}`");
    }
}

/// The value of `label` among `pairs`, as a number.
pub fn number(pairs: &[(String, String)], label: &str) -> f64 {
    pairs
        .iter()
        .find(|(l, _)| l == label)
        .and_then(|(_, value)| value.parse().ok())
        .unwrap_or_else(|| panic!("{CHAPTER}: a start gives `{label}` as a number"))
}

/// Draws one row of the `draw` table.
fn draw(app: &mut App, strings: &Strings, row: &[String]) {
    let [shape, tool, first, start, then] = row else {
        panic!("{CHAPTER}: a drawn shape's row has five cells");
    };
    let shape = names(shape).remove(0);
    let drawing = Drawing::ALL
        .into_iter()
        .find(|drawing| strings.get(drawing.name()) == tool)
        .unwrap_or_else(|| panic!("{CHAPTER}: there is no drawing tool `{tool}`"));
    app.arm(Tool::Draw(drawing));
    for (label, value) in settings(first) {
        set_in_bar(app, &label, &value);
    }
    let before = app.project().instances.len();
    // Where it starts: typed, or clicked where the tool takes no typing.
    let start = settings(start);
    let bar = app.tool_bar().expect("a drawing tool has a bar");
    let typed = start
        .iter()
        .all(|(label, _)| bar.fields.iter().any(|row| &row.label == label));
    if typed {
        for (label, value) in &start {
            set_in_bar(app, label, value);
        }
        app.tool_event(Event::Key(Key::Enter));
    } else {
        let at = Pointer::at(Point::new(number(&start, "X"), number(&start, "Y")));
        app.tool_event(Event::Move(at));
        app.tool_event(Event::Press(at));
        app.tool_event(Event::Release(at));
    }
    for (label, value) in settings(then) {
        set_in_bar(app, &label, &value);
    }
    let entered = app.tool_event(Event::Key(Key::Enter));
    assert_eq!(entered.refused, None, "{CHAPTER}: {shape}");
    if app.project().instances.len() == before {
        let done = app.tool_event(Event::Done);
        assert_eq!(done.refused, None, "{CHAPTER}: {shape}, Done");
    }
    let made: Vec<&str> = app
        .project()
        .instances
        .iter()
        .skip(before)
        .map(|i| i.name.as_str())
        .collect();
    assert_eq!(made, [shape.as_str()], "{CHAPTER}: what {tool} drew");
}

/// The instances named `shapes`, in that order.
fn ids(app: &App, shapes: &[String]) -> Vec<InstanceId> {
    shapes
        .iter()
        .map(|shape| {
            app.project()
                .instances
                .iter()
                .find(|i| &i.name == shape)
                .unwrap_or_else(|| {
                    let have: Vec<&str> = app
                        .project()
                        .instances
                        .iter()
                        .map(|i| i.name.as_str())
                        .collect();
                    panic!("{CHAPTER}: there is no shape `{shape}`; there are {have:?}")
                })
                .id
                .clone()
        })
        .collect()
}

/// Runs one row of the `arrange` table.
fn arrange(app: &mut App, strings: &Strings, row: &[String]) {
    let [shapes, command, set] = row else {
        panic!("{CHAPTER}: an arrange row has three cells");
    };
    let selected = ids(app, &names(shapes));
    app.arm(Tool::Select);
    app.select(Selection::Instances(selected.clone()));
    let menu = |keys: &[StrKey]| -> String {
        let path: Vec<&str> = keys.iter().map(|key| strings.get(*key)).collect();
        path.join(" ▸ ")
    };
    let command = command.replace("**", "");
    let array = menu(&[StrKey::AppMenuArrange, StrKey::ArrangeMenuArray]);
    let align = |side: StrKey| menu(&[StrKey::AppMenuArrange, StrKey::ArrangeMenuAlign, side]);
    if command == array {
        app.arm(Tool::Adjust(Adjust::Array));
        for (label, value) in settings(set) {
            set_in_bar(app, &label, &value);
        }
        let done = app.tool_event(Event::Done);
        assert_eq!(done.refused, None, "{CHAPTER}: {command} of {shapes}");
        assert!(done.ran.is_some(), "{CHAPTER}: {command} of {shapes}");
        return;
    }
    let sides = [
        (StrKey::ArrangeMenuAlignLeft, dc_app::arrange::Align::Left),
        (
            StrKey::ArrangeMenuAlignCentre,
            dc_app::arrange::Align::Centre,
        ),
        (StrKey::ArrangeMenuAlignRight, dc_app::arrange::Align::Right),
        (StrKey::ArrangeMenuAlignTop, dc_app::arrange::Align::Top),
        (
            StrKey::ArrangeMenuAlignMiddle,
            dc_app::arrange::Align::Middle,
        ),
        (
            StrKey::ArrangeMenuAlignBottom,
            dc_app::arrange::Align::Bottom,
        ),
    ];
    let Some((_, side)) = sides.into_iter().find(|(key, _)| command == align(*key)) else {
        panic!("{CHAPTER}: the arrange table has no command `{command}`");
    };
    let action = dc_app::arrange::Action::Align {
        side,
        to_stock: app.prefs().adjust.align_to_stock,
    };
    let arranged = dc_app::arrange::arrange(app.project(), &selected, 0, action)
        .unwrap_or_else(|why| panic!("{CHAPTER}: {command} of {shapes} is disabled: {why:?}"));
    app.run(arranged.command).expect("the shapes align");
    // Then Properties: X and Y typed.
    for (label, value) in settings(set) {
        use dc_app::properties::{self, Field};
        let field = [Field::X, Field::Y, Field::W, Field::H]
            .into_iter()
            .find(|field| strings.get(field.label()) == label)
            .unwrap_or_else(|| panic!("{CHAPTER}: Properties has no field `{label}`"));
        let placement =
            properties::placement(app.project(), &selected).expect("a selection is placed");
        let units = app.project().display_units;
        let command = properties::parse(&placement, field, &value, units, true)
            .and_then(|edit| properties::edit(app.project(), &selected, edit))
            .unwrap_or_else(|e| panic!("{CHAPTER}: {shapes}, {label} `{value}`: {e}"));
        app.run(command).expect("the shape is placed");
    }
}

/// What the part's table sets at `step` (`Stock`, `Tools`).
pub fn part_cell(text: &str, step: &str) -> String {
    table(text, &format!("part {PART}"))
        .into_iter()
        .find(|row| row[0] == step)
        .map(|row| row[1].clone())
        .unwrap_or_else(|| panic!("{CHAPTER}: the part has no `{step}` row"))
}

/// The rows of the chapter's `draw` table.
pub fn draw_rows(text: &str) -> Vec<Vec<String>> {
    table(text, &format!("draw {PART}"))
}

/// The rows of the chapter's `arrange` table.
pub fn arrange_rows(text: &str) -> Vec<Vec<String>> {
    table(text, &format!("arrange {PART}"))
}

/// The hook plate's shapes drawn in the app by the chapter's steps, from
/// a blank project: the Stock step's sizes, each shape of the `draw`
/// table, and each row of the `arrange` table. No tool, no operation.
pub fn drawn(text: &str) -> App {
    let strings = Strings::english();
    let mut app = App::new(
        App::new_project().expect("a new project"),
        Strings::english(),
        dc_app::Prefs::default(),
    );
    app_steps::set_stock(&mut app, &part_cell(text, "Stock"));
    for row in draw_rows(text) {
        draw(&mut app, &strings, &row);
    }
    for row in arrange_rows(text) {
        arrange(&mut app, &strings, &row);
    }
    app.arm(Tool::Select);
    app.select(Selection::Nothing);
    app
}

/// The tools added to `app`'s project from the library `lib`, and each
/// operation of the chapter: what follows the drawing.
pub fn finish(app: &mut App, text: &str, lib: &Library) {
    app_steps::add_tools(app, lib, &part_cell(text, "Tools"));
    app_steps::add_operations(app, text, PART);
}

/// The hook plate made in the app by the chapter's steps: [`drawn`],
/// then [`finish`]. Nothing is generated yet.
pub fn build(text: &str, lib: &Library) -> App {
    let mut app = drawn(text);
    finish(&mut app, text, lib);
    app
}
