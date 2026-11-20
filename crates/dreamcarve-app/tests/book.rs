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
use egui_kittest::kittest::Queryable as _;

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
    let closed = h.query_by_label("1 program").is_some()
        || h.query_by_label_contains("programs, run in this order")
            .is_some();
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

/// A click on the Viewport's backdrop, off the stock. The first click
/// after *Add to project* closes the Tool Library does nothing, wherever
/// it lands (seen with a pointer click and with an accessibility click,
/// 2026-11-19; reported with #550); this spends it where a click does
/// nothing anyway.
fn spend_the_click_after_the_library(h: &mut H) {
    let viewport = h.state().layout().viewport;
    common::click_at(
        h,
        viewport.right_bottom() - egui::vec2(12.0, 12.0),
        egui::Modifiers::NONE,
    );
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
    spend_the_click_after_the_library(h);
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
    spend_the_click_after_the_library(h);
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
