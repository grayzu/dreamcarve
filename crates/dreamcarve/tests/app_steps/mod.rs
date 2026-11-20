// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The steps of the quick start "From SVG to G-code in the app", read
//! from the chapter and run on a [`dc_app::App`] (#550; functional spec,
//! acceptance 90). The chapter's tables are the steps: what the reader
//! types into which field, in which order. Reading them here means the
//! chapter and the tests cannot drift apart, as `quick_start.rs` reads
//! the command-line chapters' steps from their `doc-test` markers.
//!
//! A table is marked with an HTML comment, which the rendered book does
//! not show, on the line before it:
//!
//! - `<!-- app-test: tools -->`: the tools the reader makes in the Tool
//!   Library, one per row. The header cells are the editing pane's
//!   labels; a cell is what is typed into that field, and an empty cell
//!   is a field left alone.
//! - `<!-- app-test: part NAME -->`: how part `NAME` starts, a row per
//!   step: `Drawing` (the file *New from drawing…* opens, in a code
//!   span), `Stock` (`**Thickness** 11.8`), `Tools` (the library tools
//!   added to the project, in code spans, one at a time in that order).
//! - `<!-- app-test: operations NAME -->`: the part's operations in cut
//!   order, a row each: its name, the kind picked from *Add*, the shapes
//!   selected first (code spans, in order), its tool, and the fields set
//!   after, as `**Label** value` pairs with `;` between. A switch's
//!   value is `on` or `off`; `**Clearing passes**` takes the tool of a
//!   pass added with *Add pass*; `**Place tabs**` is the button.
//!
//! Every value goes through the function the window calls for that
//! field (`dc_app::library::edit`, `dc_app::project::Field::set`,
//! `dc_app::operations::add`, `dc_app::editor::Slot`), so a label the
//! app does not have, or a value a field refuses, fails here.
//!
//! Shared by `gui_equality.rs`, which compares the programs with the
//! committed ones, and by `dreamcarve-app`'s `tests/book.rs`, which opens
//! the projects these steps build in the window for the book's pictures.

#![allow(dead_code, reason = "each test binary uses the helpers it needs")]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use dc_app::App;
use dc_library::Library;

/// The quick start "From SVG to G-code in the app", relative to the
/// repository root.
pub const CHAPTER: &str = "docs/book/src/quick-start/app.md";

/// The repository root.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The chapter's text.
pub fn chapter() -> String {
    std::fs::read_to_string(repo_root().join(CHAPTER)).expect("the app quick start is readable")
}

/// The lines of the table under `<!-- app-test: MARKER -->` in the
/// chapter `text`, its header first.
fn table_lines<'a>(text: &'a str, marker: &str) -> Vec<&'a str> {
    let wanted = format!("<!-- app-test: {marker} -->");
    let lines: Vec<&str> = text
        .lines()
        .skip_while(|line| line.trim() != wanted)
        .skip(1)
        .take_while(|line| line.starts_with('|'))
        .collect();
    assert!(lines.len() > 2, "{CHAPTER} has no table under `{wanted}`");
    lines
}

/// The cells of one line of a table, trimmed.
fn cells(line: &str) -> Vec<String> {
    let line = line.trim().trim_start_matches('|').trim_end_matches('|');
    line.split('|').map(|cell| cell.trim().to_owned()).collect()
}

/// The rows of the table under marker `marker`: each row's cells,
/// without the header and its rule.
pub fn table(text: &str, marker: &str) -> Vec<Vec<String>> {
    table_lines(text, marker)
        .into_iter()
        .skip(2)
        .map(cells)
        .collect()
}

/// The header cells of the table under marker `marker`, their bold
/// markers removed: the fields' labels.
pub fn header(text: &str, marker: &str) -> Vec<String> {
    cells(table_lines(text, marker)[0])
        .into_iter()
        .map(|cell| cell.replace("**", ""))
        .collect()
}

/// The names a cell gives in code spans, in order: `` `a`, `b` `` is
/// `["a", "b"]`.
pub fn names(cell: &str) -> Vec<String> {
    cell.split('`')
        .skip(1)
        .step_by(2)
        .map(str::to_owned)
        .collect()
}

/// The `**Label** value` pairs of a cell, in order, split at `;`: what
/// the reader sets, field by field. A label alone (`**Place tabs**`) is a
/// button, with an empty value; a value in a code span is a name.
pub fn settings(cell: &str) -> Vec<(String, String)> {
    cell.split(';')
        .filter_map(|part| {
            let (_, part) = part.split_once("**")?;
            let (label, value) = part.split_once("**")?;
            Some((label.trim().to_owned(), value.trim().replace('`', "")))
        })
        .collect()
}

/// The position among `options` of the one whose text is `label`.
fn position_of<T: Copy>(
    options: &[T],
    label: &str,
    text: impl Fn(T) -> String,
    what: &str,
) -> usize {
    options
        .iter()
        .position(|o| text(*o) == label)
        .unwrap_or_else(|| panic!("{CHAPTER}: {what} has no option `{label}`"))
}

/// The library the chapter's tools table makes, a row at a time as the
/// Tool Library sheet does: *New tool*, then each field typed in the
/// editing pane, in millimetres. A V-bit's switch is held until its
/// angle is typed, as in the sheet.
pub fn library(text: &str) -> Library {
    use dc_app::tools::{KindChoice, KindSwitch, ToolField};
    let strings = dc_app::Strings::english();
    let header = header(text, "tools");
    let mut lib = Library::default();
    for row in table(text, "tools") {
        let (next, id) = dc_app::library::new_tool(&lib);
        lib = next;
        let mut held = false;
        for (label, cell) in header.iter().zip(&row) {
            let value = cell.replace('`', "");
            if value.is_empty() {
                continue;
            }
            let tool = lib
                .tools
                .iter()
                .find(|t| t.id == id)
                .expect("the new tool")
                .tool
                .clone();
            if label == strings.get(dc_app::StrKey::AppToolsKind) {
                let at = position_of(
                    &KindChoice::ALL,
                    &value,
                    |k| strings.get(k.label()).to_owned(),
                    "Type",
                );
                match dc_app::tools::set_kind(&tool, KindChoice::ALL[at]) {
                    KindSwitch::Unchanged => {}
                    KindSwitch::Update(next) => {
                        lib = dc_app::library::set_tool(&lib, &id, *next).expect("the type sets");
                    }
                    KindSwitch::HoldForAngle => held = true,
                }
                continue;
            }
            if held && label == strings.get(ToolField::Angle.label()) {
                let Some(dc_app::Command::UpdateTool(next)) =
                    dc_app::tools::finish_v_bit(&tool, &value).expect("an angle")
                else {
                    panic!("{CHAPTER}: the V-bit `{}` has no angle", tool.name);
                };
                lib = dc_app::library::set_tool(&lib, &id, next).expect("the V-bit sets");
                held = false;
                continue;
            }
            let field = dc_app::tools::fields(&tool)
                .into_iter()
                .find(|f| strings.get(f.label()) == label)
                .unwrap_or_else(|| {
                    panic!("{CHAPTER}: the tool `{}` has no field `{label}`", tool.name)
                });
            lib = dc_app::library::edit(&lib, &id, field, &value, dc_project::Units::Mm)
                .unwrap_or_else(|e| panic!("{CHAPTER}: {label} `{value}`: {e}"));
        }
        assert!(!held, "{CHAPTER}: a V-bit needs its Angle column");
    }
    lib
}

/// Where the drawing a part starts from is committed.
pub fn drawing_path(name: &str) -> PathBuf {
    match name {
        "coaster.svg" => repo_root().join("fixtures/coaster.svg"),
        other => repo_root().join("tests/fixtures").join(other),
    }
}

/// The drawing part `part` starts from, as the chapter names it.
pub fn drawing_of(text: &str, part: &str) -> String {
    table(text, &format!("part {part}"))
        .into_iter()
        .find(|row| row[0] == "Drawing")
        .map(|row| names(&row[1]).remove(0))
        .unwrap_or_else(|| panic!("{CHAPTER}: part {part} has no Drawing row"))
}

/// The operation `id` of the app's project.
fn operation(app: &App, id: &dc_project::OperationId) -> dc_project::Operation {
    app.project()
        .operations
        .iter()
        .find(|op| op.id() == id)
        .expect("the operation")
        .clone()
}

/// The project tool named `name`.
fn tool_named(app: &App, name: &str) -> dc_project::ToolId {
    app.project()
        .tools
        .iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("{CHAPTER}: the project has no tool `{name}`"))
        .id
        .clone()
}

/// Sets the editor's field `label` of operation `id` to `value`, as the
/// Inspector does for the row's kind: text typed and left, an option
/// picked, a switch turned `on` or `off`, a tool picked by name, a
/// clearing pass added with *Add pass* and given its tool, or the *Place
/// tabs* button pressed.
fn set_in_editor(app: &mut App, id: &dc_project::OperationId, label: &str, value: &str) {
    use dc_app::editor::fields::{self, FieldKind};
    use dc_app::editor::{Slot, clearing};
    let strings = dc_app::Strings::english();
    let op = operation(app, id);
    if label == strings.get(dc_app::StrKey::OperationsPlaceTabs) {
        app.place_tabs(id).expect("the request places");
        return;
    }
    let rows: Vec<_> = fields::rows(&op)
        .into_iter()
        .filter(|row| strings.get(row.label) == label)
        .collect();
    let [row] = rows[..] else {
        panic!(
            "{CHAPTER}: `{}` has {} fields labelled `{label}`",
            op.name(),
            rows.len()
        );
    };
    let slot = Slot::of(row);
    let project = app.project().clone();
    let on = || match value {
        "on" => true,
        "off" => false,
        other => panic!("{CHAPTER}: {label} is `on` or `off`, not `{other}`"),
    };
    let command = match row.kind {
        FieldKind::Choice(options) => {
            let at = position_of(options, value, |o| strings.get(o.label).to_owned(), label);
            slot.choose(&project, &op, at)
        }
        FieldKind::Checkbox => slot.set_checked(&op, on()),
        FieldKind::Toggle(_) | FieldKind::TabRequest => slot.switch(&project, &op, on()),
        FieldKind::SwitchedLength if value == "off" => slot.switch(&project, &op, false),
        FieldKind::ToolPicker => slot.set_tool(&op, &tool_named(app, value)),
        FieldKind::ClearingList => {
            let added = clearing::add(&project, &op, row)
                .expect("a pass is added")
                .expect("a command");
            app.run(added).expect("the pass is written");
            let op = operation(app, id);
            let last = clearing::passes(&op, row).len() - 1;
            let picker = clearing::rows(&op, row, last)
                .into_iter()
                .find(|slot| matches!(slot.row.kind, FieldKind::ToolPicker))
                .expect("a pass has a tool");
            picker.set_tool(&op, &tool_named(app, value))
        }
        _ => slot.commit_text(&project, &op, value),
    }
    .unwrap_or_else(|e| panic!("{CHAPTER}: `{}`, {label} `{value}`: {e}", op.name()));
    if let Some(command) = command {
        app.run(command)
            .unwrap_or_else(|e| panic!("{CHAPTER}: `{}`, {label} `{value}`: {e}", op.name()));
    }
}

/// Part `part` built in the app by the chapter's steps, from its drawing:
/// *New from drawing*, the Stock step's thickness, the tools added from
/// the library `lib` one at a time in the chapter's order, then each
/// operation of the part's table: its shapes selected in order, *Add ▸
/// kind*, its name typed, its tool picked and its fields set. Nothing is
/// generated yet.
pub fn build(text: &str, part: &str, lib: &Library) -> App {
    use dc_app::import::{DrawingFormat, DrawingImport, Purpose};
    let strings = dc_app::Strings::english();
    let mut app = App::new(
        App::new_project().expect("a new project"),
        dc_app::Strings::english(),
        dc_app::Prefs::default(),
    );
    for row in table(text, &format!("part {part}")) {
        let [step, what] = &row[..] else {
            panic!("{CHAPTER}: a part's row is a step and what to set");
        };
        match step.as_str() {
            "Drawing" => {
                let name = names(what).remove(0);
                let format = if name.ends_with(".dxf") {
                    DrawingFormat::Dxf
                } else {
                    DrawingFormat::Svg
                };
                let bytes = std::fs::read(drawing_path(&name)).expect("the drawing");
                let sheet = DrawingImport::open(format, &name, bytes, Purpose::New);
                assert_eq!(
                    sheet.blocked(),
                    None,
                    "{CHAPTER}: {name} imports as the sheet opens it"
                );
                app.new_from_drawing(&sheet)
                    .expect("a project from the drawing");
            }
            "Stock" => {
                for (label, value) in settings(what) {
                    assert_eq!(label, strings.get(dc_app::StrKey::ProjectStockThickness));
                    let command = dc_app::project::Field::StockThickness
                        .set(app.project(), &value)
                        .expect("a thickness")
                        .expect("a command");
                    app.run(command).expect("the thickness sets");
                }
            }
            "Tools" => {
                for name in names(what) {
                    let entry = dc_library::find(lib, &name)
                        .unwrap_or_else(|_| panic!("{CHAPTER}: the tools table has no `{name}`"));
                    let ticked = BTreeSet::from([entry.id.clone()]);
                    let command = dc_app::tools::add_from_library(app.project(), lib, &ticked)
                        .expect("a tool to add");
                    app.run(command).expect("the tool is added");
                }
            }
            other => panic!("{CHAPTER}: a part has no step `{other}`"),
        }
    }

    for row in table(text, &format!("operations {part}")) {
        let [name, kind, shapes, tool, then] = &row[..] else {
            panic!("{CHAPTER}: an operation's row has five cells");
        };
        let name = names(name).remove(0);
        let at = position_of(
            &dc_app::operations::Kind::ALL,
            kind,
            |k| strings.get(k.label()).to_owned(),
            "Add",
        );
        let kind = dc_app::operations::Kind::ALL[at];
        let selected: Vec<dc_project::InstanceId> = names(shapes)
            .iter()
            .map(|shape| {
                app.project()
                    .instances
                    .iter()
                    .find(|i| &i.name == shape)
                    .unwrap_or_else(|| panic!("{CHAPTER}: {part} has no shape `{shape}`"))
                    .id
                    .clone()
            })
            .collect();
        let add = dc_app::operations::add(app.project(), &strings, kind, &selected, &[])
            .unwrap_or_else(|| panic!("{CHAPTER}: Add does not offer `{name}` its kind"));
        app.run(add).expect("the operation is added");
        let id = app
            .project()
            .operations
            .last()
            .expect("the new operation")
            .id()
            .clone();
        if let Some(rename) = dc_app::editor::rename(&operation(&app, &id), &name).expect("a name")
        {
            app.run(rename).expect("renamed");
        }
        assert_eq!(operation(&app, &id).name(), name);
        let tool_label = strings.get(dc_app::StrKey::EditorFieldTool);
        set_in_editor(&mut app, &id, tool_label, &names(tool).remove(0));
        for (label, value) in settings(then) {
            set_in_editor(&mut app, &id, &label, &value);
        }
    }
    app
}
