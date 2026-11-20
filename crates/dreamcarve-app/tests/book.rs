// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The book's screenshots (#550; `docs/specs/phase-4/functional.md`,
//! "Documentation deliverables": "screenshots from the shipped build").
//! Every picture of the app a chapter shows is written here, by driving
//! the window as the chapter tells the reader to: the same clicks, the
//! same typed values. Nothing is drawn or edited by hand.
//!
//! Ignored, since it writes files and needs a wgpu adapter; run it by
//! hand after a change to the window or to a chapter's steps:
//!
//! ```sh
//! cargo test -p dreamcarve-app --test book -- --ignored
//! ```
//!
//! The pictures go to `docs/book/src/quick-start/figures/app/` and
//! `docs/book/src/user-guide/figures/app/`, where they are committed
//! (`DC_BOOK_DIR` writes them under another folder instead). They are
//! rendered at 2× in the default, dark appearance, in a 1280 × 800 point
//! window, and most are cropped to the region the chapter is about, so
//! the labels read at the book's width.
//! `docs::book_builds_and_every_screenshot_exists`
//! (`crates/dreamcarve/tests/docs.rs`) checks that every picture a
//! chapter names is there and that none is left unused.
//!
//! The quick start's values are not repeated here: they are read from the
//! chapter's own tables (`crates/dreamcarve/tests/app_steps`), the tables
//! `gui_equality::quick_start_parts_from_the_app` builds the four parts
//! from. The coaster is driven through the window, click by click, and
//! the project it leaves must be the one those steps build headless; the
//! plaque, the sign and the inlay pair are built by the steps and opened
//! in the window.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

#[path = "../../dreamcarve/tests/app_steps/mod.rs"]
mod app_steps;

use std::path::{Path, PathBuf};

use common::{QueuedDialogs, go_to, open_operation, options};
use eframe::egui;
use eframe::egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

type H = Harness<'static, dreamcarve_app::Shell>;

/// The scale the pictures are rendered at.
const SCALE: f32 = 2.0;

/// The window the pictures are of, points.
const WINDOW: egui::Vec2 = egui::vec2(1280.0, 800.0);

/// Where the pictures of `part` (`quick-start` or `user-guide`) go.
fn out_dir(part: &str) -> PathBuf {
    let book = std::env::var_os("DC_BOOK_DIR")
        .map_or_else(|| common::repo_root().join("docs/book/src"), PathBuf::from);
    let dir = book.join(part).join("figures/app");
    std::fs::create_dir_all(&dir).expect("the figures directory");
    dir
}

/// Steps the window until generation, the worker and any export plan are
/// done.
fn settle(h: &mut H) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(180);
    while h.state().app().generation().busy()
        || h.state().worker_busy()
        || h.state().export_planning()
    {
        assert!(std::time::Instant::now() < deadline, "the window settles");
        h.step();
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    h.run();
}

/// A window with a config directory under `dir` and `open` (if any) open,
/// in the dark appearance whatever the OS says; `None` with no wgpu
/// adapter.
fn window(dir: &Path, open: Option<PathBuf>, dialogs: &QueuedDialogs) -> Option<H> {
    if !common::wgpu_adapter_available() {
        println!("book: no wgpu adapter on this machine, so nothing is rendered");
        return None;
    }
    let options = options(&dir.join("config"), open, dialogs);
    let mut h = Harness::builder()
        .with_size(WINDOW)
        .with_pixels_per_point(SCALE)
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::new())
        .build_eframe(move |cc| dreamcarve_app::Shell::new(cc, options).expect("the shell starts"));
    h.state_mut().app_mut().prefs_mut().theme = dc_app::Theme::Dark;
    settle(&mut h);
    Some(h)
}

/// Takes the pointer off the window, so no control shows its hover.
fn park_pointer(h: &mut H) {
    h.event(egui::Event::PointerGone);
    h.run();
}

/// What a picture shows of the window.
#[derive(Debug, Clone, Copy)]
enum Crop {
    /// All of it.
    Window,
    /// The open sheet, with a margin of the dimmed window round it.
    Sheet,
    /// The rail and the step panel.
    StepPanel,
    /// The Viewport and the Inspector.
    ViewportAndInspector,
}

/// The open sheet's card: the box round its buttons, fields and text
/// (the sheet itself reports no rectangle), with the sheet's 24-px
/// padding put back.
fn sheet_rect(h: &H) -> egui::Rect {
    let sheet = h.get_by_role(Role::Dialog);
    let mut content = egui::Rect::NOTHING;
    for role in [Role::Button, Role::TextInput, Role::Label, Role::CheckBox] {
        for node in sheet.query_all_by_role(role) {
            content = content.union(node.rect());
        }
    }
    assert!(content.is_positive(), "the sheet has content");
    content.expand(24.0)
}

/// The part of the window `crop` names, points.
fn region(h: &H, crop: Crop) -> egui::Rect {
    let full = egui::Rect::from_min_size(egui::Pos2::ZERO, WINDOW);
    let layout = h.state().layout();
    // Most of a card's 8-px gap, kept round a region so its corners show
    // and its neighbours do not.
    let gap = 6.0;
    match crop {
        Crop::Window => full,
        Crop::Sheet => sheet_rect(h).expand(24.0),
        Crop::StepPanel => {
            let panel = layout.step_panel.expect("the step panel shows");
            egui::Rect::from_min_max(
                egui::pos2(0.0, panel.min.y - gap),
                egui::pos2(panel.max.x + gap, panel.max.y + gap),
            )
        }
        Crop::ViewportAndInspector => {
            let inspector = layout.inspector.expect("the Inspector shows");
            layout.viewport.union(inspector).expand(gap)
        }
    }
    .intersect(full)
}

/// Writes the window as `dir/name.png`, cropped to `crop`.
fn shoot(h: &mut H, dir: &Path, name: &str, crop: Crop) {
    park_pointer(h);
    let crop = region(h, crop);
    let image = h.render().expect("the window renders");
    let (width, height) = (image.width(), image.height());
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "pixel coordinates inside the window"
    )]
    let px = |points: f32, most: u32| ((points * SCALE).round() as u32).min(most);
    let (x0, y0) = (px(crop.min.x, width), px(crop.min.y, height));
    let (x1, y1) = (px(crop.max.x, width), px(crop.max.y, height));
    assert!(x1 > x0 && y1 > y0, "{name}: an empty crop");
    // RGB, since the window is opaque.
    let raw = image.as_raw();
    let mut rgb = Vec::with_capacity(((x1 - x0) * (y1 - y0) * 3) as usize);
    for y in y0..y1 {
        let row = (y * width * 4) as usize;
        for x in x0..x1 {
            let at = row + (x * 4) as usize;
            rgb.extend_from_slice(&raw[at..at + 3]);
        }
    }
    let path = dir.join(format!("{name}.png"));
    let file = std::fs::File::create(&path).expect("the picture's file");
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), x1 - x0, y1 - y0);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().expect("the PNG header");
    writer.write_image_data(&rgb).expect("the PNG data");
    writer.finish().expect("the PNG is written");
}

fn click(h: &mut H, label: &str) {
    h.get_by_label(label).click();
    h.run();
}

fn click_button(h: &mut H, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    h.run();
}

fn click_in_sheet(h: &mut H, label: &str) {
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::Button, label)
        .click();
    h.run();
}

/// Types `text` into the text field named `label`, replacing what it
/// holds, and leaves it with Enter, so the field commits and no field
/// keeps the keyboard focus.
fn type_in(h: &mut H, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text(text.to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
}

/// What the text field named `label` shows.
fn field(h: &H, label: &str) -> String {
    h.get_by_role_and_label(Role::TextInput, label)
        .value()
        .unwrap_or_default()
}

/// Whether the last frame painted text containing `needle`: the
/// Viewport's legend and captions are painted, not controls.
fn painted(h: &H, needle: &str) -> bool {
    h.output().shapes.iter().any(|clipped| {
        matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.text().contains(needle))
    })
}

/// The step **Undo** would take back, as the Edit menu names it.
fn undo_label(h: &H) -> String {
    h.state().app().undo_label().unwrap_or_default().to_owned()
}

/// [`type_in`] for a field of the open sheet.
fn type_in_sheet(h: &mut H, label: &str, text: &str) {
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::TextInput, label)
        .focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text(text.to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
}

/// Opens the operation editor's group `title`: its header, the rightmost
/// button of that name, since the editor is the right-hand card.
fn open_group(h: &mut H, title: &str) {
    h.query_all_by_role_and_label(Role::Button, title)
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .unwrap_or_else(|| panic!("no {title} group"))
        .click();
    h.run();
}

/// Opens the Export step's *Program Details*, unless it is open: the
/// window leaves it as the user left it, and closed it says how many
/// programs there are.
fn open_program_details(h: &mut H) {
    // Closed, its header's value is its summary; open, it has none.
    let closed = h.get_by_label("Program Details").value().is_some();
    if closed {
        click(h, "Program Details");
    }
}

/// Makes row `row` of the chapter's tools table in the open Tool Library:
/// *New tool*, then each cell typed into the editing pane's field of the
/// header's label. An end mill is what *New tool* makes, so the Type is
/// only checked.
fn make_library_tool(h: &mut H, header: &[String], row: &[String]) {
    h.get_by_role(Role::Dialog)
        .query_all_by_role_and_label(Role::Button, "New tool")
        .next()
        .expect("New tool")
        .click();
    h.run();
    for (label, cell) in header.iter().zip(row) {
        let value = cell.replace('`', "");
        match (label.as_str(), value.as_str()) {
            (_, "") => {}
            ("Type", kind) => assert_eq!(kind, "End mill", "New tool makes an end mill"),
            (label, value) => type_in_sheet(h, label, value),
        }
    }
}

/// Ticks library row `name` in the open Tool Library.
fn tick(h: &mut H, name: &str) {
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::CheckBox, name)
        .click();
    h.run();
}

/// The drawing part `part` starts from, copied into `dir` as a reader
/// puts it in a folder, so the sheet shows a plain path.
fn drawing_in(dir: &Path, text: &str, part: &str) -> PathBuf {
    let name = app_steps::drawing_of(text, part);
    let to = dir.join(&name);
    std::fs::copy(app_steps::drawing_path(&name), &to).expect("the drawing is copied");
    to
}

/// The Stock step's thickness the chapter gives part `part`.
fn thickness_of(text: &str, part: &str) -> String {
    let rows = app_steps::table(text, &format!("part {part}"));
    let stock = rows
        .iter()
        .find(|row| row[0] == "Stock")
        .expect("a Stock row");
    app_steps::settings(&stock[1])[0].1.clone()
}

/// The coaster, end to end, in the window: every step of the chapter's
/// first part, with a picture at each.
fn coaster(h: &mut H, dialogs: &QueuedDialogs, dir: &Path, out: &Path, text: &str) {
    let header = app_steps::header(text, "tools");
    let tools = app_steps::table(text, "tools");

    // 1. The start screen, and New from drawing…
    shoot(h, out, "01-start-screen", Crop::Window);
    dialogs.answer(&drawing_in(dir, text, "coaster"));
    click_button(h, "New from drawing…");
    shoot(h, out, "02-new-from-drawing", Crop::Sheet);
    click_in_sheet(h, "Create project");
    settle(h);
    assert_eq!(
        h.state().step(),
        dc_app::steps::Step::Stock,
        "a project fresh from a drawing opens on Stock"
    );

    // 2. The stock: the thickness.
    shoot(h, out, "03-stock-thickness-required", Crop::Window);
    type_in(h, "Thickness", &thickness_of(text, "coaster"));
    settle(h);
    shoot(h, out, "04-stock", Crop::StepPanel);

    // 3. The tool: made in the library, then added to the project.
    click_button(h, "Continue");
    assert_eq!(h.state().step(), dc_app::steps::Step::Tools);
    shoot(h, out, "05-tools-empty", Crop::StepPanel);
    click_button(h, "Tool Library…");
    make_library_tool(h, &header, &tools[0]);
    tick(h, "1/4 end mill");
    shoot(h, out, "06-tool-library", Crop::Sheet);
    click_in_sheet(h, "Add 1 tool to project");
    settle(h);
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    shoot(h, out, "07-tools", Crop::Window);

    // 4. The Profile: select the circle, Add ▸ Profile, its two numbers,
    // Place tabs.
    click_button(h, "Continue");
    assert_eq!(h.state().step(), dc_app::steps::Step::Operations);
    let on_the_circle = common::on_screen(h, dc_geometry::Point::new(100.0, 55.0));
    common::click_at(h, on_the_circle, egui::Modifiers::NONE);
    click_button(h, "Add");
    shoot(h, out, "08-add-menu", Crop::Window);
    h.get_by_role_and_label(Role::MenuItem, "Profile").click();
    h.run();
    settle(h);
    shoot(h, out, "09-profile-new", Crop::ViewportAndInspector);
    open_group(h, "Depth");
    let row = &app_steps::table(text, "operations coaster")[0];
    for (label, value) in app_steps::settings(&row[4]) {
        if value.is_empty() {
            click_button(h, &label);
        } else {
            type_in(h, &label, &value);
        }
        settle(h);
    }
    shoot(h, out, "10-profile", Crop::Window);
    go_to(h, "Design");
    assert!(
        h.query_by_label_contains("4 tabs").is_some(),
        "the placed tabs are listed under Tabs on the Design step"
    );
    go_to(h, "Operations");

    // 5. The preview: the Inspector closed, the Layers popover, then the
    // cuts coloured by depth with the program's extents drawn.
    click(h, "Close the Inspector");
    click_button(h, "Layers");
    shoot(h, out, "11-layers", Crop::Window);
    click(h, "Program extents");
    h.key_press(egui::Key::Escape);
    h.run();
    click(h, "By depth");
    shoot(h, out, "12-by-depth", Crop::Window);
    click(h, "By operation");
    click_button(h, "Layers");
    click(h, "Program extents");
    h.key_press(egui::Key::Escape);
    h.run();

    // 6. Save, then Export.
    let saved = dir.join("coaster.dcv");
    dialogs.answer(&saved);
    common::command_key(h, egui::Key::S);
    h.run();
    settle(h);
    assert!(saved.exists(), "⌘S saved the project");
    click_button(h, "Continue");
    assert_eq!(h.state().step(), dc_app::steps::Step::Export);
    settle(h);
    open_program_details(h);
    shoot(h, out, "13-export", Crop::Window);
    click_button(h, "Export G-code");
    settle(h);
    click(h, "Run instructions");
    shoot(h, out, "14-export-result", Crop::StepPanel);

    // The window did what the chapter's tables say: the project it holds
    // is the one the steps build headless, and the program it wrote names
    // the operation and the tool as the reader left them.
    let lib = app_steps::library(text);
    let steps = app_steps::build(text, "coaster", &lib);
    assert_eq!(
        h.state().app().serialize().unwrap(),
        steps.serialize().unwrap(),
        "the window's coaster is the chapter's"
    );
    let program = std::fs::read_to_string(dir.join("coaster.nc")).expect("coaster.nc is written");
    assert!(
        program.contains("( Operation 1: Profile 1 )")
            && program.contains("( Tool 1: 1/4 end mill )"),
        "the program names the operation and the tool"
    );
}

/// A part of the chapter built by its steps and saved as `<stem>.dcv` in
/// a folder named `stem` beside `dir`.
fn built(text: &str, part: &str, stem: &str, dir: &Path) -> PathBuf {
    let lib = app_steps::library(text);
    let app = app_steps::build(text, part, &lib);
    let dir = dir.with_file_name(stem);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the part's folder");
    let path = dir.join(format!("{stem}.dcv"));
    std::fs::write(&path, app.serialize().expect("the project saves")).expect("written");
    path
}

/// The plaque's second tool, made and added in the window that still
/// holds the coaster's library.
fn second_tool(h: &mut H, dialogs: &QueuedDialogs, dir: &Path, out: &Path, text: &str) {
    let header = app_steps::header(text, "tools");
    let tools = app_steps::table(text, "tools");
    dialogs.answer(&drawing_in(dir, text, "plaque"));
    click_button(h, "File");
    click(h, "New Project from Drawing…");
    click_in_sheet(h, "Create project");
    settle(h);
    // The squiggle is an open path nothing cuts yet, a warning that marks
    // Design, so the plaque opens there.
    assert_eq!(h.state().step(), dc_app::steps::Step::Design);
    go_to(h, "Stock");
    type_in(h, "Thickness", &thickness_of(text, "plaque"));
    go_to(h, "Tools");
    click_button(h, "Tool Library…");
    make_library_tool(h, &header, &tools[1]);
    tick(h, "1/4 end mill");
    tick(h, "1/8 end mill");
    shoot(h, out, "15-plaque-tool-library", Crop::Sheet);
    click_in_sheet(h, "Add 2 tools to project");
    settle(h);
    let names: Vec<String> = h
        .state()
        .app()
        .project()
        .tools
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(names, ["1/4 end mill", "1/8 end mill"]);

    // The plaque's first operation, as the chapter's table gives it,
    // with the gestures the chapter names: the shapes picked in the
    // Design Tree (click, then Shift-click), Add ▸ kind on the Operations
    // step, the name typed in the editor's header, the tool picked from
    // the Tool row, then each field.
    let row = &app_steps::table(text, "operations plaque")[0];
    let [name, kind, shapes, tool, then] = &row[..] else {
        panic!("an operation's row has five cells");
    };
    go_to(h, "Design");
    for (k, shape) in app_steps::names(shapes).iter().enumerate() {
        let at = h
            .get_by_role_and_label(Role::TreeItem, shape)
            .rect()
            .center();
        let modifiers = if k == 0 {
            egui::Modifiers::NONE
        } else {
            egui::Modifiers::SHIFT
        };
        common::click_at(h, at, modifiers);
    }
    go_to(h, "Operations");
    click_button(h, "Add");
    h.get_by_role_and_label(Role::MenuItem, kind).click();
    h.run();
    let made = format!("{kind} 1");
    click_button(h, &made);
    h.get_by_role_and_label(Role::TextInput, &format!("Rename {made}"))
        .type_text(&app_steps::names(name)[0]);
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    h.get_by_role_and_label(Role::ComboBox, "Tool").click();
    h.run();
    h.get_by_role_and_label(Role::Button, &format!("{} (T2)", app_steps::names(tool)[0]))
        .click();
    h.run();
    for group in ["Depth", "Heights"] {
        open_group(h, group);
    }
    for (label, value) in app_steps::settings(then) {
        type_in(h, &label, &value);
    }
    settle(h);
    shoot(h, out, "16-plaque-first-operation", Crop::Window);

    // The operation the window made is the one the chapter's steps build.
    let lib = app_steps::library(text);
    let steps = app_steps::build(text, "plaque", &lib);
    assert_eq!(
        h.state().app().project().operations[0],
        steps.project().operations[0],
        "the window's first plaque operation is the chapter's"
    );
    assert_eq!(h.state().app().project().tools, steps.project().tools);
}

/// The quick start "From SVG to G-code in the app"
/// (`docs/book/src/quick-start/app.md`): the coaster driven through the
/// window, the plaque's second tool made in it, then the plaque, the sign
/// and the inlay pair as the chapter's tables build them, each opened in
/// the window.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn quick_start_screenshots() {
    let out = out_dir("quick-start");
    // A short path, so the folder reads whole in the pictures.
    let dir = std::env::temp_dir().join("coaster");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let text = app_steps::chapter();
    let dialogs = QueuedDialogs::default();
    let Some(mut h) = window(&dir, None, &dialogs) else {
        return;
    };
    coaster(&mut h, &dialogs, &dir, &out, &text);
    second_tool(&mut h, &dialogs, &dir, &out, &text);
    drop(h);

    let plaque = built(&text, "plaque", "plaque", &dir);
    let mut h = window(&dir, Some(plaque), &dialogs).unwrap();
    open_operation(&mut h, "Panel");
    settle(&mut h);
    shoot(&mut h, &out, "17-plaque-operations", Crop::Window);
    go_to(&mut h, "Export");
    settle(&mut h);
    open_program_details(&mut h);
    shoot(&mut h, &out, "18-plaque-export", Crop::StepPanel);
    drop(h);

    let sign = built(&text, "sign", "sign", &dir);
    let mut h = window(&dir, Some(sign), &dialogs).unwrap();
    open_operation(&mut h, "Background");
    settle(&mut h);
    shoot(&mut h, &out, "19-sign-v-carve", Crop::Window);
    drop(h);

    let inlay = built(&text, "inlay", "dc", &dir);
    let mut h = window(&dir, Some(inlay), &dialogs).unwrap();
    open_operation(&mut h, "DC");
    settle(&mut h);
    shoot(&mut h, &out, "20-inlay", Crop::Window);
    go_to(&mut h, "Export");
    settle(&mut h);
    open_program_details(&mut h);
    shoot(&mut h, &out, "21-inlay-export", Crop::StepPanel);
    drop(h);
    for folder in ["coaster", "plaque", "sign", "dc"] {
        std::fs::remove_dir_all(dir.with_file_name(folder)).ok();
    }
}

/// The plaque as the quick start's tables build it, in a folder named
/// `plaque` under a fresh scratch directory `name`, and a window on it;
/// `None` with no wgpu adapter.
fn plaque_window(name: &str, dialogs: &QueuedDialogs) -> Option<(H, PathBuf)> {
    let dir = std::env::temp_dir().join(name).join("scratch");
    let _ = std::fs::remove_dir_all(dir.parent().expect("a parent"));
    std::fs::create_dir_all(&dir).expect("the scratch directory");
    let plaque = built(&app_steps::chapter(), "plaque", "plaque", &dir);
    let h = window(&dir, Some(plaque), dialogs)?;
    Some((h, dir))
}

/// The plaque's first tool that an operation cuts with, without its feed:
/// an error that blocks export.
fn delete_a_feed(h: &mut H) {
    let project = h.state().app().project();
    let used = project.operations[0].tool().clone();
    let mut tool = project.tools.iter().find(|t| t.id == used).unwrap().clone();
    tool.feed_mm_min = None;
    h.state_mut()
        .app_mut()
        .run(dc_app::Command::UpdateTool(tool))
        .unwrap();
    settle(h);
}

/// The chapter "The app" (`docs/book/src/user-guide/the-app.md`): the
/// window with a shape selected, the rail's marks and the Warnings drawer
/// with an error in the project, and the light appearance.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn the_app_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-app", &dialogs) else {
        return;
    };

    // The six regions: a shape selected on the Design step.
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "square").click();
    h.run();
    shoot(&mut h, &out, "app-window", Crop::Window);
    h.key_press(egui::Key::Escape);
    h.run();

    // The same window in the light appearance, an operation selected.
    open_operation(&mut h, "Panel");
    settle(&mut h);
    h.state_mut().app_mut().prefs_mut().theme = dc_app::Theme::Light;
    h.run();
    shoot(&mut h, &out, "app-light", Crop::Window);
    h.state_mut().app_mut().prefs_mut().theme = dc_app::Theme::Dark;
    h.run();

    // A tool without its feed: Tools and Export need attention, and the
    // Warnings drawer says why.
    delete_a_feed(&mut h);
    go_to(&mut h, "Tools");
    shoot(&mut h, &out, "app-rail-marks", Crop::StepPanel);
    h.get_by_label_contains("Warnings:").click();
    h.run();
    shoot(&mut h, &out, "app-warnings-drawer", Crop::Window);
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// The chapter "Importing drawings and pictures"
/// (`docs/book/src/user-guide/importing.md`): a project made round a
/// picture (a landscape drawn here, so no photograph needs committing),
/// the picture selected on the Design step, and a DXF drawing added to
/// it.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn importing_screenshots() {
    let out = out_dir("user-guide");
    let dir = std::env::temp_dir().join("dreamcarve-book-import");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let picture = common::write_colour_png(&dir, "landscape.png", &common::landscape(480, 320));
    let drawing = dir.join("plaque.dxf");
    std::fs::copy(app_steps::drawing_path("plaque.dxf"), &drawing).unwrap();
    let dialogs = QueuedDialogs::default();
    let Some(mut h) = window(&dir, None, &dialogs) else {
        return;
    };

    // New from picture…: the sheet asks for one side of the picture.
    dialogs.answer(&picture);
    click_button(&mut h, "New from picture…");
    common::settle_picture(&mut h);
    type_in_sheet(&mut h, "Width", "120");
    shoot(&mut h, &out, "import-picture-sheet", Crop::Sheet);
    click_in_sheet(&mut h, "Create project");
    settle(&mut h);

    // The picture on the Design step, selected: its row, its handles on
    // the Viewport, its Properties in the Inspector.
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "landscape").click();
    h.run();
    settle(&mut h);
    shoot(&mut h, &out, "import-picture-selected", Crop::Window);

    // Add drawing: a DXF's sheet has the importer's options.
    dialogs.answer(&drawing);
    click_button(&mut h, "Add drawing");
    shoot(&mut h, &out, "import-dxf-sheet", Crop::Sheet);
    drop(h);
    std::fs::remove_dir_all(&dir).ok();
}

/// The chapter "Stock, origin and heights"
/// (`docs/book/src/user-guide/stock-and-origin.md`): the plaque's Stock
/// step, and the Heights group of its Export step.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn stock_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-stock", &dialogs) else {
        return;
    };
    go_to(&mut h, "Stock");
    shoot(&mut h, &out, "stock-step", Crop::Window);
    go_to(&mut h, "Export");
    settle(&mut h);
    click_button(&mut h, "Heights");
    shoot(&mut h, &out, "stock-heights", Crop::StepPanel);
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// The chapter "Checking and exporting"
/// (`docs/book/src/user-guide/exporting.md`): the plaque's Export step
/// with a tool that lacks its feed, so an error blocks export, and with
/// the Post-processor group open.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn exporting_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-export", &dialogs) else {
        return;
    };
    go_to(&mut h, "Export");
    settle(&mut h);
    click_button(&mut h, "Post-processor");
    shoot(&mut h, &out, "export-post-processor", Crop::StepPanel);
    click_button(&mut h, "Post-processor");
    delete_a_feed(&mut h);
    shoot(&mut h, &out, "export-blocked", Crop::StepPanel);
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// A drag of the primary button from `from` to `to` with `modifiers`
/// held, a frame per step.
fn drag_with(h: &mut H, from: egui::Pos2, to: egui::Pos2, modifiers: egui::Modifiers) {
    h.event(egui::Event::PointerMoved(from));
    h.event(egui::Event::ModifiersChanged(modifiers));
    h.event(egui::Event::PointerButton {
        pos: from,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers,
    });
    h.run();
    for k in 1..=4_u8 {
        let t = f32::from(k) / 4.0;
        h.event(egui::Event::PointerMoved(from + (to - from) * t));
        h.run();
    }
    h.event(egui::Event::PointerButton {
        pos: to,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers,
    });
    h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
    h.run();
}

/// Ctrl Z, or ⌘Z on macOS.
fn undo(h: &mut H) {
    common::command_key(h, egui::Key::Z);
    h.run();
}

/// Where project point (`x_mm`, `y_mm`) is on the window.
fn at_mm(h: &H, x_mm: f64, y_mm: f64) -> egui::Pos2 {
    common::on_screen(h, dc_geometry::Point::new(x_mm, y_mm))
}

/// What `check` says of the project, as the Warnings drawer copies it.
fn problems(h: &H) -> Vec<String> {
    dc_app::warnings::lines(h.state().app())
}

/// The chapter "Placing shapes"
/// (`docs/book/src/user-guide/placing-shapes.md`), on the plaque: every
/// gesture the chapter names, done as it says and checked against the
/// values it gives, with a picture of the square selected, of the Rotate
/// popover, of the list with a hidden row, a locked row and a linked
/// copy, of a locked shape, and of a row's menu.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn placing_shapes_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-place", &dialogs) else {
        return;
    };
    go_to(&mut h, "Design");
    let row = |h: &mut H, name: &str| {
        h.get_by_role_and_label(Role::TreeItem, name).click();
        h.run();
    };
    let size = |h: &H| ["X", "Y", "W", "H"].map(|label| field(h, label));
    let saved = h.state().app().project().clone();

    // The square, selected from its row: its Properties.
    row(&mut h, "square");
    assert_eq!(size(&h), ["70", "18", "30", "30"]);
    assert!(h.query_by_label("Targeted by: Square recess").is_some());
    shoot(&mut h, &out, "place-selected", Crop::ViewportAndInspector);

    // Moving: X typed, the arrow keys, a drag.
    type_in(&mut h, "X", "80");
    assert_eq!(size(&h), ["80", "18", "30", "30"]);
    assert_eq!(undo_label(&h), "Move “square”");
    undo(&mut h);
    assert_eq!(size(&h), ["70", "18", "30", "30"], "Undo puts it back");
    h.key_press(egui::Key::ArrowRight);
    h.run();
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::ArrowUp);
    h.run();
    h.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowLeft);
    h.run();
    assert_eq!(
        size(&h),
        ["70.9", "28", "30", "30"],
        "1 mm right, 10 mm up, 0.1 mm left"
    );
    for _ in 0..3 {
        undo(&mut h);
    }
    let (from, to) = (at_mm(&h, 70.0, 33.0), at_mm(&h, 60.0, 38.0));
    drag_with(&mut h, from, to, egui::Modifiers::NONE);
    assert_eq!(size(&h), ["60", "23", "30", "30"], "dragged by its edge");
    assert_eq!(undo_label(&h), "Move “square”");
    undo(&mut h);

    // Resizing: W typed with the link closed and open, a corner dragged
    // with and without Alt.
    type_in(&mut h, "W", "20");
    assert_eq!(size(&h), ["75", "23", "20", "20"], "about its centre");
    assert_eq!(undo_label(&h), "Scale “square”");
    undo(&mut h);
    click(&mut h, "Keep proportions");
    type_in(&mut h, "H", "40");
    assert_eq!(size(&h), ["70", "13", "30", "40"], "the link open");
    undo(&mut h);
    click(&mut h, "Keep proportions");
    let (from, to) = (at_mm(&h, 100.0, 48.0), at_mm(&h, 105.0, 53.0));
    drag_with(&mut h, from, to, egui::Modifiers::NONE);
    assert_eq!(size(&h), ["65", "13", "40", "40"], "a corner, 5 mm out");
    undo(&mut h);
    let (from, to) = (at_mm(&h, 100.0, 48.0), at_mm(&h, 110.0, 48.0));
    drag_with(&mut h, from, to, egui::Modifiers::ALT);
    assert_eq!(size(&h), ["60", "18", "50", "30"], "with Alt, one side");
    undo(&mut h);

    // Rotating: the button, R and Shift R, the handle with Shift, and
    // the popover's angle.
    click(&mut h, "Rotate 90° clockwise");
    assert_eq!(undo_label(&h), "Rotate “square” 90° clockwise");
    undo(&mut h);
    h.key_press(egui::Key::R);
    h.run();
    assert_eq!(undo_label(&h), "Rotate “square” 90° clockwise");
    undo(&mut h);
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::R);
    h.run();
    assert_eq!(undo_label(&h), "Rotate “square” 90° anticlockwise");
    undo(&mut h);
    let handle = at_mm(&h, 85.0, 48.0) - egui::vec2(0.0, 20.0);
    let to = at_mm(&h, 85.0, 33.0) + egui::vec2(-60.0, -50.0);
    drag_with(&mut h, handle, to, egui::Modifiers::SHIFT);
    assert_eq!(
        size(&h),
        ["63.7868", "11.7868", "42.4264", "42.4264"],
        "with Shift the handle turns in 15° steps: here 45°"
    );
    assert_eq!(undo_label(&h), "Rotate “square”");
    undo(&mut h);
    click(&mut h, "Rotate by an angle");
    h.get_by_label("Angle").click();
    h.run();
    h.get_by_label("Angle").type_text("30");
    h.run();
    shoot(&mut h, &out, "place-rotate", Crop::ViewportAndInspector);
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(undo_label(&h), "Rotate “square” 30° anticlockwise");
    assert_eq!(size(&h), ["64.5096", "12.5096", "40.9808", "40.9808"]);
    undo(&mut h);
    click_button(&mut h, "Arrange");
    click(&mut h, "Rotate…");
    assert!(
        h.query_by_label("Angle").is_some(),
        "Arrange ▸ Rotate… opens the same popover"
    );
    h.key_press(egui::Key::Escape);
    h.run();

    // Flipping, and Centre on Stock (the stock is 170 × 120).
    click(&mut h, "Flip horizontal");
    assert_eq!(undo_label(&h), "Flip “square” horizontally");
    undo(&mut h);
    click(&mut h, "Flip vertical");
    assert_eq!(undo_label(&h), "Flip “square” vertically");
    undo(&mut h);
    click_button(&mut h, "Arrange");
    click(&mut h, "Centre on Stock");
    assert_eq!(size(&h), ["70", "45", "30", "30"]);
    assert_eq!(undo_label(&h), "Centre “square” on stock");
    undo(&mut h);
    assert_eq!(h.state().app().project(), &saved, "every step was undone");

    // A linked copy; a hidden shape; a locked one.
    common::command_key(&h, egui::Key::D);
    h.run();
    assert_eq!(undo_label(&h), "Duplicate “square”");
    assert!(h.query_by_label("Properties · square (2)").is_some());
    assert_eq!(size(&h), ["80", "28", "30", "30"], "10 mm right and up");
    assert!(
        h.query_by_label("No operation targets the selection")
            .is_some()
    );
    row(&mut h, "island");
    h.key_press(egui::Key::H);
    h.run();
    assert_eq!(undo_label(&h), "Hide “island”");
    row(&mut h, "square (2)");
    h.key_press(egui::Key::L);
    h.run();
    assert_eq!(undo_label(&h), "Lock “square (2)”");
    settle(&mut h);
    assert!(
        problems(&h).is_empty() && h.query_by_label("Est. 3 min 39 s").is_some(),
        "a copy nothing cuts, a hidden shape and a locked one change no cut"
    );
    shoot(&mut h, &out, "place-list", Crop::StepPanel);
    shoot(&mut h, &out, "place-locked", Crop::ViewportAndInspector);
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!(size(&h), ["80", "28", "30", "30"], "a locked shape stays");
    assert!(h.query_by_label("shape “square (2)” is locked").is_some());
    // With the square selected too: a nudge moves neither, and Rotate
    // turns the one that is not locked.
    h.get_by_role_and_label(Role::TreeItem, "square")
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    h.key_press(egui::Key::ArrowRight);
    h.run();
    assert_eq!(
        undo_label(&h),
        "Lock “square (2)”",
        "no step: neither moved"
    );
    click(&mut h, "Rotate 90° clockwise");
    assert_eq!(undo_label(&h), "Rotate “square” 90° clockwise");
    assert!(
        h.query_by_label_contains("“square (2)” is locked, so it stayed")
            .is_some(),
        "the status bar names the shape that stayed"
    );
    undo(&mut h);
    row(&mut h, "square (2)");
    h.key_press(egui::Key::L);
    h.run();
    assert_eq!(undo_label(&h), "Unlock “square (2)”");

    // A shape moved past the stock's right edge: its cut is an error,
    // and the Stock step says the shape reaches past the stock.
    row(&mut h, "square");
    type_in(&mut h, "X", "150");
    settle(&mut h);
    assert_eq!(
        problems(&h),
        ["error: operation `op4` cuts off the stock: 10.930 mm past the +X (right) edge"]
    );
    go_to(&mut h, "Stock");
    assert!(
        h.query_by_label_contains("reaches past the stock")
            .is_some()
    );
    go_to(&mut h, "Design");
    undo(&mut h);
    row(&mut h, "island");
    h.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::H);
    h.run();
    assert_eq!(undo_label(&h), "Show “island”");

    // A row's menu.
    h.get_by_role_and_label(Role::TreeItem, "pocket")
        .click_secondary();
    h.run();
    for item in ["Rename", "Duplicate", "Zoom to", "Add operation"] {
        assert!(h.query_by_label(item).is_some(), "the menu has {item}");
    }
    shoot(&mut h, &out, "place-menu", Crop::StepPanel);
    h.key_press(egui::Key::Escape);
    h.run();

    // Deleting: the copy no operation targets, then the square the
    // Square recess cuts, which leaves that Pocket with nothing to cut.
    row(&mut h, "square (2)");
    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(undo_label(&h), "Delete “square (2)”");
    row(&mut h, "square");
    h.key_press(egui::Key::Delete);
    h.run();
    settle(&mut h);
    assert_eq!(undo_label(&h), "Delete “square”");
    assert_eq!(
        problems(&h),
        [
            "error: operation `Square recess` (`op4`) is enabled but has no targets, so it would \
          cut nothing; add targets or disable it"
        ]
    );
    let without = h.state().app().project().clone();
    undo(&mut h);
    settle(&mut h);
    let targets = |project: &dc_project::Project| project.operations[3].targets().len();
    assert_eq!(
        (targets(&without), targets(h.state().app().project())),
        (0, 1),
        "Undo puts the square back among the Square recess's targets"
    );
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// The pictures chapters, "Images"
/// (`docs/book/src/user-guide/images.md`, "In the app") and "Photo
/// V-Carve operation" (`photo-v-carve.md`, "In the app"), on a landscape
/// drawn here so no photograph needs committing. First a picture added
/// to the plaque: the sheet, the note about its colour, and every way
/// the chapter gives of moving and resizing it. Then a project made
/// round the picture and carved: the three required values, the editor,
/// and the grooves by depth.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn pictures_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-pictures", &dialogs) else {
        return;
    };
    let picture = common::write_colour_png(&dir, "landscape.png", &common::landscape(480, 320));

    // Add picture: the sheet's button is off until a side is typed, and
    // the other side follows the pixels (480 × 320).
    go_to(&mut h, "Design");
    dialogs.answer(&picture);
    click_button(&mut h, "Add picture");
    common::settle_picture(&mut h);
    let add = h
        .get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::Button, "Add picture");
    assert!(add.accesskit_node().is_disabled(), "no size typed yet");
    type_in_sheet(&mut h, "Height", "40");
    let width = h
        .get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::TextInput, "Width")
        .value();
    assert_eq!(width.as_deref(), Some("60"));
    shoot(&mut h, &out, "picture-add-sheet", Crop::Sheet);
    click_in_sheet(&mut h, "Add picture");
    settle(&mut h);

    // It lands at the stock's bottom-left corner, selected, as one step,
    // and the conversion to grayscale is a note in the Warnings drawer.
    let size = |h: &H| ["X", "Y", "W", "H"].map(|label| field(h, label));
    assert_eq!(size(&h), ["0", "0", "60", "40"]);
    assert_eq!(field(&h, "Name"), "landscape");
    assert_eq!(field(&h, "Pixels"), "480 × 320 px");
    assert_eq!(undo_label(&h), "Import landscape.png");
    assert!(
        h.query_by_label("No Photo V-Carve carves the selection")
            .is_some()
    );
    assert!(
        h.query_by_label("Flip horizontal").is_none(),
        "no Arrange group"
    );
    assert_eq!(
        problems(&h),
        [
            "plaque.dcv: note: landscape.png: image_colour_converted: the picture is in colour; \
          it was converted to grayscale by luminance (0.299 R + 0.587 G + 0.114 B)"
        ]
    );
    h.get_by_label_contains("Warnings:").click();
    h.run();
    shoot(&mut h, &out, "picture-added", Crop::Window);
    click(&mut h, "Close the Warnings drawer");

    // Moving and resizing: X, the arrows, a drag, W, a corner handle.
    type_in(&mut h, "X", "20");
    assert_eq!(size(&h), ["20", "0", "60", "40"]);
    assert_eq!(undo_label(&h), "Move “landscape”");
    let inside = at_mm(&h, 45.0, 3.0);
    common::click_at(&mut h, inside, egui::Modifiers::NONE);
    h.key_press(egui::Key::ArrowUp);
    h.run();
    assert_eq!(size(&h), ["20", "1", "60", "40"]);
    let (from, to) = (at_mm(&h, 45.0, 3.0), at_mm(&h, 55.0, 12.0));
    drag_with(&mut h, from, to, egui::Modifiers::NONE);
    assert_eq!(size(&h), ["30", "10", "60", "40"], "dragged 10 right, 9 up");
    type_in(&mut h, "W", "90");
    assert_eq!(size(&h), ["15", "0", "90", "60"], "about its centre");
    assert_eq!(undo_label(&h), "Resize “landscape”");
    let (from, to) = (at_mm(&h, 105.0, 60.0), at_mm(&h, 90.0, 50.0));
    drag_with(&mut h, from, to, egui::Modifiers::NONE);
    assert_eq!(size(&h), ["30", "10", "60", "40"], "a corner, 15 mm in");

    // Locked, it stays; deleted, it is gone.
    h.get_by_role_and_label(Role::Switch, "Locked").click();
    h.run();
    assert_eq!(undo_label(&h), "Lock “landscape”");
    h.key_press(egui::Key::ArrowUp);
    h.run();
    assert!(h.query_by_label("picture “landscape” is locked").is_some());
    assert_eq!(size(&h), ["30", "10", "60", "40"]);
    h.key_press(egui::Key::L);
    h.run();
    assert_eq!(undo_label(&h), "Unlock “landscape”");

    // Add ▸ Photo V-Carve carves the selected picture. The plaque has no
    // V-bit, so the new operation has no tool yet.
    go_to(&mut h, "Operations");
    click_button(&mut h, "Add");
    h.get_by_role_and_label(Role::MenuItem, "Photo V-Carve")
        .click();
    h.run();
    settle(&mut h);
    assert_eq!(undo_label(&h), "Add “Photo V-Carve 1”");
    let picture_row = h.get_by_role_and_label(Role::ComboBox, "Picture").value();
    assert_eq!(picture_row.as_deref(), Some("landscape"));
    let no_tool = "error: operation `op6` uses tool ``, which does not exist";
    assert!(problems(&h).iter().any(|line| line == no_tool));
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "landscape").click();
    h.run();
    assert!(h.query_by_label("Carved by: Photo V-Carve 1").is_some());
    h.key_press(egui::Key::Delete);
    h.run();
    settle(&mut h);
    assert_eq!(undo_label(&h), "Delete “landscape”");
    assert!(h.state().app().project().bitmaps.is_empty());
    let no_picture = "error: operation `op6` carves bitmap `b1`, which does not exist";
    assert!(
        problems(&h).iter().any(|line| line == no_picture),
        "the Photo V-Carve that lost its picture says so"
    );
    open_operation(&mut h, "Photo V-Carve 1");
    let picture_row = h.get_by_role_and_label(Role::ComboBox, "Picture").value();
    assert_eq!(picture_row.as_deref(), Some("b1 · not in the project"));
    drop(h);

    // A project round the picture.
    let dir = dir.with_file_name("carve");
    std::fs::create_dir_all(&dir).unwrap();
    let mut h = window(&dir, None, &dialogs).unwrap();
    dialogs.answer(&picture);
    click_button(&mut h, "New from picture…");
    common::settle_picture(&mut h);
    type_in_sheet(&mut h, "Width", "120");
    click_in_sheet(&mut h, "Create project");
    settle(&mut h);
    // It opens on Stock, with one Photo V-Carve of the picture and a
    // 60° V-bit for it, and four values still to set.
    assert_eq!(h.state().step(), dc_app::steps::Step::Stock);
    let errors: Vec<String> = problems(&h)
        .into_iter()
        .filter(|line| line.starts_with("error"))
        .collect();
    assert_eq!(
        errors,
        [
            "error: stock.thickness_mm is not set (mm)",
            "error: tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`",
            "error: tools[t1].plunge_mm_min is not set (mm/min) for tool `Set me`",
            "error: tools[t1].rpm is not set (RPM) for tool `Set me`",
            "error: operations[op1].max_depth_mm is not set (mm) for operation `Photo V-Carve`",
        ]
    );
    let tool = &h.state().app().project().tools[0];
    assert_eq!(
        (tool.diameter_mm.to_string().as_str(), &tool.cutter),
        (
            "6.35",
            &dc_project::ToolKind::VBit {
                included_angle_deg: 60.0
            }
        )
    );

    // The chapter's example values: the thickness, the V-bit's rates,
    // the max depth.
    type_in(&mut h, "Thickness", "12");
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "Set me").click();
    h.run();
    for (label, value) in [("Feed", "1000"), ("Plunge", "300"), ("RPM", "18000")] {
        type_in(&mut h, label, value);
    }
    open_operation(&mut h, "Photo V-Carve");
    for group in ["Depth", "Strategy"] {
        open_group(&mut h, group);
    }
    shoot(&mut h, &out, "photo-required", Crop::Window);
    type_in(&mut h, "Max depth", "1.5");
    settle(&mut h);
    assert!(
        problems(&h).iter().all(|line| !line.starts_with("error")),
        "nothing stops export: {:?}",
        problems(&h)
    );
    assert!(
        h.query_all_by_label("Photo V-Carve · 63 grooves 1.732 mm wide, to 1.500 mm")
            .next()
            .is_some(),
        "the status bar's message is the carve's summary"
    );
    shoot(&mut h, &out, "photo-editor", Crop::Window);

    // By depth: the grooves shade with their depth, and the legend's
    // deepest value is this picture's darkest grey, not the max depth.
    click(&mut h, "Close the Inspector");
    click(&mut h, "By depth");
    assert!(painted(&h, "1.27"), "the legend's deepest value");
    shoot(&mut h, &out, "photo-by-depth", Crop::Window);

    // Hatch adds the second set of grooves.
    click(&mut h, "By operation");
    open_operation(&mut h, "Photo V-Carve");
    h.get_by_role_and_label(Role::RadioButton, "Hatch").click();
    h.run();
    settle(&mut h);
    assert_eq!(undo_label(&h), "Set strategy of “Photo V-Carve”");
    assert!(
        painted(&h, "Est. 11 min") && !painted(&h, "Est. 5 min 46 s"),
        "Hatch about doubles the estimate of 5 min 46 s"
    );
    assert!(
        problems(&h).iter().any(|line| line.contains("138 grooves")),
        "Hatch cuts a second set of grooves: {:?}",
        problems(&h)
    );
    h.get_by_role_and_label(Role::CheckBox, "Invert").click();
    h.run();
    assert_eq!(undo_label(&h), "Set invert of “Photo V-Carve”");
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// Whether the field named `label` is empty and says *Required*.
fn required(h: &H, label: &str) -> bool {
    h.query_all_by_role_and_label(Role::TextInput, label)
        .any(|n| n.accesskit_node().placeholder() == Some("Required"))
}

/// The right-hand of the open sheet's dropdowns named `label`: the
/// editing pane's, where the list's filter shares the name.
fn click_pane_dropdown(h: &mut H, label: &str) {
    h.get_by_role(Role::Dialog)
        .query_all_by_role_and_label(Role::ComboBox, label)
        .max_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .unwrap_or_else(|| panic!("no {label} dropdown"))
        .click();
    h.run();
}

/// The library's type filter: the left-hand of the sheet's two Type
/// dropdowns. Sets it to `kind`.
fn filter_library(h: &mut H, kind: &str) {
    h.get_by_role(Role::Dialog)
        .query_all_by_role_and_label(Role::ComboBox, "Type")
        .min_by(|a, b| a.rect().min.x.total_cmp(&b.rect().min.x))
        .expect("the type filter")
        .click();
    h.run();
    h.query_all_by_label(kind)
        .max_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
        .unwrap_or_else(|| panic!("the filter offers {kind}"))
        .click();
    h.run();
}

/// The names of the library rows the open sheet lists, top to bottom.
fn library_rows(h: &H) -> Vec<String> {
    let mut rows: Vec<(f32, String)> = h
        .get_by_role(Role::Dialog)
        .query_all_by_role(Role::ListItem)
        .map(|n| {
            (
                n.rect().min.y,
                n.accesskit_node().label().unwrap_or_default(),
            )
        })
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.into_iter().map(|(_, name)| name).collect()
}

/// The chapters "Tool library" (`docs/book/src/user-guide/tool-library.md`,
/// "In the app") and "Tapered ball nose" (`tapered-ball-nose.md`, "In the
/// app"), on the plaque with an empty library: a tool made on the Tools
/// step, turned into a V-bit and saved to the library; the Fusion sample
/// imported through the review; a tapered ball nose made in the library,
/// found with the type filter, and added to the project; and a tapered
/// ball nose made on the Tools step, with nothing measured yet.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn tools_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-tools", &dialogs) else {
        return;
    };
    let tapered = "0.762 mm tip tapered ball nose";

    // New tool: an end mill named for its place, its name ready to type
    // over, its rates required.
    go_to(&mut h, "Tools");
    click_button(&mut h, "New tool");
    assert_eq!(undo_label(&h), "New tool “tool3”");
    assert_eq!(field(&h, "Diameter"), "6.35");
    assert_eq!(field(&h, "Flutes"), "2");
    for rate in ["Feed", "Plunge", "RPM"] {
        assert!(required(&h, rate), "{rate} is required");
    }
    h.event(egui::Event::Text("60 degree V-bit".to_owned()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(undo_label(&h), "Edit tool “60 degree V-bit”");

    // Type ▸ V-bit waits for its angle.
    h.get_by_role_and_label(Role::ComboBox, "Type").click();
    h.run();
    for kind in [
        "End mill",
        "Ball nose",
        "V-bit",
        "Engraver",
        "Drill",
        "Tapered ball nose",
    ] {
        assert!(
            h.query_all_by_label(kind).next().is_some(),
            "Type offers {kind}"
        );
    }
    h.get_by_label("V-bit").click();
    h.run();
    settle(&mut h);
    assert!(
        h.query_by_label("Type the included angle to make this tool a V-bit")
            .is_some()
    );
    assert_eq!(
        h.state().app().project().tools[2].cutter,
        dc_project::ToolKind::EndMill,
        "still an end mill until the angle is typed"
    );
    shoot(&mut h, &out, "tools-editor", Crop::Window);
    type_in(&mut h, "Angle", "60");
    for (label, value) in [("Feed", "1000"), ("Plunge", "300"), ("RPM", "18000")] {
        type_in(&mut h, label, value);
    }
    let made = h.state().app().project().tools[2].clone();
    assert_eq!(
        made.cutter,
        dc_project::ToolKind::VBit {
            included_angle_deg: 60.0
        }
    );
    assert!(made.unset_values().is_empty(), "nothing left to set");

    // Save to library, twice.
    click_button(&mut h, "Save to library");
    assert_eq!(
        h.state().message(),
        Some("Saved “60 degree V-bit” to the library as lt1")
    );
    click_button(&mut h, "Save to library");
    assert_eq!(
        h.state().message(),
        Some("“60 degree V-bit” is already in the library as lt1")
    );

    // A tool an operation uses cannot be deleted; Duplicate copies it.
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    assert!(
        h.get_by_role_and_label(Role::Button, "Delete")
            .accesskit_node()
            .is_disabled()
    );
    click_button(&mut h, "Duplicate");
    assert_eq!(undo_label(&h), "Add tool “1/4 end mill (2)”");
    undo(&mut h);

    // The library: Import… shows the review before anything is written.
    click_button(&mut h, "Tool Library…");
    assert_eq!(library_rows(&h), ["60 degree V-bit"]);
    dialogs.answer(&common::fixture("fusion/sample.tools"));
    h.get_by_role(Role::Dialog)
        .query_all_by_label("Import…")
        .next()
        .expect("Import…")
        .click();
    h.run();
    for count in [
        "13 New",
        "0 Already in your library",
        "0 Changed",
        "10 Skipped",
    ] {
        assert!(
            h.get_by_role(Role::Dialog).query_by_label(count).is_some(),
            "the review counts {count}"
        );
    }
    shoot(&mut h, &out, "library-import-review", Crop::Sheet);
    click(&mut h, "Import 13 tools");
    assert!(
        h.query_by_label("Imported 13 tools from sample.tools. 10 skipped.")
            .is_some()
    );
    assert_eq!(library_rows(&h).len(), 14);

    // A tapered ball nose made in the library: the type is taken at
    // once, and its three values are required until typed.
    h.get_by_role(Role::Dialog)
        .query_all_by_role_and_label(Role::Button, "New tool")
        .next()
        .expect("New tool")
        .click();
    h.run();
    click_pane_dropdown(&mut h, "Type");
    h.get_by_label("Tapered ball nose").click();
    h.run();
    for value in ["Tip diameter", "Taper per side", "Flute length"] {
        assert!(required(&h, value), "{value} is required");
    }
    for (label, value) in [
        ("Name", tapered),
        ("Tip diameter", "0.762"),
        ("Taper per side", "6.7"),
        ("Flute length", "19.05"),
        ("Feed", "1524"),
        ("Plunge", "635"),
        ("RPM", "19000"),
        ("Stepover", "10"),
    ] {
        type_in_sheet(&mut h, label, value);
    }

    // The type filter, and the search, which reads a tip as a diameter.
    filter_library(&mut h, "V-bit");
    assert_eq!(
        library_rows(&h),
        ["60 degree V-bit", "12 mm 90-Degree Chamfer Mill"]
    );
    filter_library(&mut h, "Tapered ball nose");
    assert_eq!(library_rows(&h), [tapered]);
    filter_library(&mut h, "All types");
    let search = h
        .get_by_role(Role::Dialog)
        .query_all_by_role(Role::TextInput)
        .find(|n| {
            n.accesskit_node().placeholder() == Some("Search name, vendor, product, diameter")
        })
        .expect("the search field")
        .rect()
        .center();
    common::click_at(&mut h, search, egui::Modifiers::NONE);
    h.event(egui::Event::Text("0.762".to_owned()));
    h.run();
    assert_eq!(library_rows(&h), [tapered], "the search reads the tip");
    click(&mut h, "Clear the search");
    assert_eq!(library_rows(&h).len(), 15);

    // The sheet's own units: inches show the 6.35 mm cutter as 0.25.
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::RadioButton, "in")
        .click();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::TextInput, "Diameter")
        .scroll_to_me();
    h.run();
    let diameter = h
        .get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::TextInput, "Diameter")
        .value();
    assert_eq!(diameter.as_deref(), Some("0.25"));
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::RadioButton, "mm")
        .click();
    h.run();

    // The picture: the tapered ball nose's row and the top of its pane.
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, tapered)
        .scroll_to_me();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::TextInput, "Name")
        .scroll_to_me();
    h.run();
    settle(&mut h);
    shoot(&mut h, &out, "library-sheet", Crop::Sheet);

    // Tick it and add it: the sheet closes and the project has a copy.
    tick(&mut h, tapered);
    click_in_sheet(&mut h, "Add 1 tool to project");
    settle(&mut h);
    assert!(h.query_by_role(Role::Dialog).is_none(), "the sheet closed");
    assert_eq!(h.state().message(), Some("Added 1 tool from the library"));
    assert_eq!(undo_label(&h), format!("Add tool “{tapered}”"));
    assert!(
        painted(&h, "6.35 mm · tip 0.762 mm · 6.7°/side"),
        "its row names the tip and the angle per side"
    );

    // A tapered ball nose made on the Tools step, nothing measured yet.
    click_button(&mut h, "New tool");
    h.key_press(egui::Key::Enter);
    h.run();
    h.get_by_role_and_label(Role::ComboBox, "Type").click();
    h.run();
    h.get_by_label("Tapered ball nose").click();
    h.run();
    settle(&mut h);
    assert_eq!(undo_label(&h), "Edit tool “tool5”");
    for value in [
        "Tip diameter",
        "Taper per side",
        "Flute length",
        "Feed",
        "Plunge",
        "RPM",
    ] {
        assert!(required(&h, value), "{value} is required");
    }
    shoot(&mut h, &out, "tools-tapered-unset", Crop::Window);
    type_in(&mut h, "Tip diameter", "0.762");
    assert!(!required(&h, "Tip diameter"));
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// The chapter "Tapered ball nose", "In the app": the tapered ball nose
/// test part (`tests/fixtures/dc-tbn.dcv`, the part of the quick start
/// "Carving and inlaying with a tapered ball nose") with its Inlay in the
/// editor. Under the Tool row is the bit's cutting height, and under
/// Depth the engagement against it and the seat line.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn tapered_ball_nose_screenshots() {
    let out = out_dir("user-guide");
    let dir = std::env::temp_dir().join("dreamcarve-book-tbn");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let part = common::copy_fixture("dc-tbn.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let Some(mut h) = window(&dir, Some(part), &dialogs) else {
        return;
    };
    open_operation(&mut h, "DC inlay");
    open_group(&mut h, "Depth");
    settle(&mut h);
    assert!(
        h.query_by_label("one bit for both boards · cutting height 19.05 mm")
            .is_some()
    );
    assert!(
        h.query_by_label_contains("of the tapered ball nose's 19.05 mm cutting height")
            .is_some()
    );
    assert!(
        h.query_by_label_contains(
            "seat: moves 0.55 mm per 0.5° of taper, 0.76 mm per 0.05 mm of tip radius"
        )
        .is_some()
    );
    shoot(&mut h, &out, "tapered-inlay", Crop::Window);

    // A V-Carve's Tool row offers the project's tapered ball noses.
    open_operation(&mut h, "DC carve");
    h.get_by_role_and_label(Role::ComboBox, "Tool").click();
    h.run();
    for tool in [
        "2.286 mm tip tapered ball nose (T1)",
        "0.762 mm tip tapered ball nose (T2)",
    ] {
        assert!(
            h.query_all_by_role_and_label(Role::Button, tool)
                .next()
                .is_some(),
            "the picker offers {tool}"
        );
    }
    drop(h);
    std::fs::remove_dir_all(&dir).ok();
}
