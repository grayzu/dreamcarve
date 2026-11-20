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

/// Writes the window as `dir/name.png`, cropped to `crop`, once
/// generation has settled: a picture taken while the worker is busy
/// shows *Checking…* or a stale toolpath, and which it shows depends on
/// how loaded the machine is.
fn shoot(h: &mut H, dir: &Path, name: &str, crop: Crop) {
    settle(h);
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

/// Clicks the button `label` and lets the window settle. A button that
/// asks for a picture (*New from picture…*, *Add picture*) hands the
/// file to a thread that decodes it, and the sheet opens when that lands.
/// `run` gives a window four frames to stop asking for more, so a picture
/// landing between two of them failed the test with "exceeded max_steps";
/// the frames are drawn one at a time until the picture has landed, as
/// the kittest drop tests draw them (`common::settle_picture`), and only
/// then is the window left to settle. A button that starts no decode
/// costs three frames more.
fn click_button(h: &mut H, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    common::settle_picture(h);
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
    // The result is a sheet: Done closes it, and the step keeps a line.
    shoot(h, out, "14-export-result", Crop::Sheet);
    click_button(h, "Done");
    assert!(
        h.query_by_role_and_label(Role::Button, "Show result")
            .is_some(),
        "the step can open the result again"
    );

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

/// The names of the project's operations, in cut order.
fn operation_names(h: &H) -> Vec<String> {
    let project = h.state().app().project();
    project
        .operations
        .iter()
        .map(|op| op.name().to_owned())
        .collect()
}

/// The names of the shapes operation `name` cuts, in order.
fn targets_of(h: &H, name: &str) -> Vec<String> {
    let project = h.state().app().project();
    let op = project
        .operations
        .iter()
        .find(|op| op.name() == name)
        .unwrap_or_else(|| panic!("no operation {name}"));
    op.targets()
        .iter()
        .map(|id| {
            let instance = project.instances.iter().find(|i| i.id == *id);
            instance.map_or_else(|| id.as_str().to_owned(), |i| i.name.clone())
        })
        .collect()
}

/// The rows of the open menu: each one's label and whether it is off.
fn menu_rows(h: &H) -> Vec<(String, bool)> {
    h.query_all_by_role(Role::MenuItem)
        .map(|n| {
            let node = n.accesskit_node();
            (node.label().unwrap_or_default(), node.is_disabled())
        })
        .collect()
}

/// What the empty field named `label` shows in grey.
fn placeholder(h: &H, label: &str) -> String {
    h.get_by_role_and_label(Role::TextInput, label)
        .accesskit_node()
        .placeholder()
        .unwrap_or_default()
        .to_owned()
}

/// The chapter "Operations in the app"
/// (`docs/book/src/user-guide/operations.md`), on the plaque: the list
/// and its row controls, the Add menu, the editor's header, Setup and
/// groups, the three ways of changing an operation's targets, the tool
/// picker, and a value the tool gives.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn operations_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-operations", &dialogs) else {
        return;
    };
    let plaque = [
        "Screw holes",
        "Border and squiggle",
        "Panel",
        "Square recess",
        "Cut out",
    ];
    open_operation(&mut h, "Cut out");
    settle(&mut h);
    assert_eq!(operation_names(&h), plaque);
    shoot(&mut h, &out, "operations-step", Crop::Window);

    // The header's name is a field once clicked.
    click_button(&mut h, "Cut out");
    h.get_by_role_and_label(Role::TextInput, "Rename Cut out")
        .type_text("Outline");
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(undo_label(&h), "Rename “Cut out”");
    assert_eq!(operation_names(&h)[4], "Outline");
    undo(&mut h);

    // The checkbox turns an operation off, and its time leaves the
    // estimate; the eye only hides its toolpath.
    h.get_by_role_and_label(Role::CheckBox, "Enable Panel")
        .click();
    h.run();
    settle(&mut h);
    assert_eq!(undo_label(&h), "Disable “Panel”");
    assert!(!painted(&h, "Est. 3 min 39 s"), "the estimate is shorter");
    assert_eq!(
        problems(&h),
        ["warning: operation_disabled: operation `op3` is disabled and will not be exported"]
    );

    // Text that is no length is refused: the field keeps its value and
    // says why, and the project is untouched.
    type_in(&mut h, "Depth", "deep");
    assert_eq!(undo_label(&h), "Disable “Panel”", "no step was made");
    assert_eq!(field(&h, "Depth"), "12");
    assert!(
        h.query_by_label_contains("`deep` is not a length")
            .is_some()
    );
    h.get_by_role_and_label(Role::TextInput, "Depth").focus();
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    h.get_by_role_and_label(Role::CheckBox, "Enable Panel")
        .click();
    h.run();
    settle(&mut h);
    assert_eq!(undo_label(&h), "Enable “Panel”");
    assert!(painted(&h, "Est. 3 min 39 s"));
    let steps = undo_label(&h);
    click(&mut h, "Hide the toolpath of Panel");
    assert_eq!(undo_label(&h), steps, "hiding a toolpath is no undo step");
    assert!(painted(&h, "Est. 3 min 39 s"), "and changes no program");
    click(&mut h, "Show the toolpath of Panel");

    // Dragging a row by its grip changes the cut order.
    let grips: Vec<egui::Pos2> = h
        .query_all_by_label("Drag to reorder")
        .map(|n| n.rect().center())
        .collect();
    assert_eq!(grips.len(), 5, "a grip on each row");
    common::drag(
        &mut h,
        grips[4],
        grips[0] - egui::vec2(0.0, 12.0),
        egui::PointerButton::Primary,
    );
    assert_eq!(
        operation_names(&h),
        [
            "Cut out",
            "Screw holes",
            "Border and squiggle",
            "Panel",
            "Square recess"
        ]
    );
    assert_eq!(undo_label(&h), "Reorder operations");
    undo(&mut h);

    // Duplicate and Delete act on the selected operation.
    click_button(&mut h, "Duplicate");
    assert_eq!(undo_label(&h), "Duplicate “Cut out”");
    assert_eq!(operation_names(&h)[5], "Cut out (2)");
    click_button(&mut h, "Delete");
    assert_eq!(undo_label(&h), "Delete “Cut out (2)”");
    undo(&mut h);
    undo(&mut h);
    assert_eq!(operation_names(&h), plaque);

    // Add, with an open path selected: the kinds it can take, then the
    // rest, each with its reason.
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "squiggle").click();
    h.run();
    go_to(&mut h, "Operations");
    settle(&mut h);
    click_button(&mut h, "Add");
    assert_eq!(
        menu_rows(&h),
        [
            ("Profile", false),
            ("Engrave", false),
            ("Pocket", true),
            ("Drill", true),
            ("Bore", true),
            ("V-Carve", true),
            ("Inlay", true),
            ("Photo V-Carve", true),
        ]
        .map(|(kind, off)| (kind.to_owned(), off))
    );
    for reason in [
        "An open path cannot take a Pocket",
        "A Drill needs circles",
        "A Photo V-Carve needs a picture",
    ] {
        assert!(painted(&h, reason), "the menu says: {reason}");
    }
    shoot(&mut h, &out, "operations-add-menu", Crop::Window);
    h.get_by_role_and_label(Role::MenuItem, "Engrave").click();
    h.run();
    settle(&mut h);
    assert_eq!(undo_label(&h), "Add “Engrave 1”");
    assert_eq!(targets_of(&h, "Engrave 1"), ["squiggle"]);
    assert!(required(&h, "Depth"), "a new operation's depth is required");
    assert_eq!(
        problems(&h),
        ["error: operations[op6].depth_mm is not set (mm) for operation `Engrave 1`"]
    );
    undo(&mut h);
    h.key_press(egui::Key::Escape);
    h.run();
    click_button(&mut h, "Add");
    assert!(
        menu_rows(&h).iter().all(|(_, off)| *off),
        "with nothing selected every kind is off"
    );
    assert!(painted(&h, "Select shapes to add an operation"));
    h.key_press(egui::Key::Escape);
    h.run();

    // Edit targets: click a shape to add it, Enter to finish.
    open_operation(&mut h, "Panel");
    assert_eq!(targets_of(&h, "Panel"), ["pocket", "island"]);
    click(&mut h, "Edit targets");
    let on_the_square = at_mm(&h, 85.0, 20.0);
    common::click_at(&mut h, on_the_square, egui::Modifiers::NONE);
    shoot(&mut h, &out, "operations-targets", Crop::Window);
    assert_eq!(
        targets_of(&h, "Panel"),
        ["pocket", "island"],
        "nothing is written until Done"
    );
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(targets_of(&h, "Panel"), ["pocket", "island", "square"]);
    assert_eq!(undo_label(&h), "Set targets of “Panel”");
    undo(&mut h);
    click(&mut h, "Edit targets");
    common::click_at(&mut h, on_the_square, egui::Modifiers::NONE);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(targets_of(&h, "Panel"), ["pocket", "island"], "Cancel");

    // The Targets row's menu takes the selection.
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "square").click();
    h.run();
    go_to(&mut h, "Operations");
    click(&mut h, "Target actions");
    click(&mut h, "Add selection");
    assert_eq!(targets_of(&h, "Panel"), ["pocket", "island", "square"]);
    click(&mut h, "Target actions");
    click(&mut h, "Use selection");
    assert_eq!(targets_of(&h, "Panel"), ["square"]);
    undo(&mut h);
    undo(&mut h);

    // A shape's row on the Design step offers the selected operation.
    go_to(&mut h, "Design");
    h.get_by_role_and_label(Role::TreeItem, "square")
        .click_secondary();
    h.run();
    click(&mut h, "Add to “Panel”");
    assert_eq!(targets_of(&h, "Panel"), ["pocket", "island", "square"]);
    h.get_by_role_and_label(Role::TreeItem, "square")
        .click_secondary();
    h.run();
    click(&mut h, "Remove from “Panel”");
    assert_eq!(targets_of(&h, "Panel"), ["pocket", "island"]);
    h.key_press(egui::Key::Escape);
    h.run();

    // The Tool row lists the project's tools that fit the kind.
    open_operation(&mut h, "Panel");
    h.get_by_role_and_label(Role::ComboBox, "Tool").click();
    h.run();
    for row in [
        "1/4 end mill (T1)",
        "1/8 end mill (T2)",
        "Add from library…",
    ] {
        assert!(
            h.query_all_by_role_and_label(Role::Button, row)
                .next()
                .is_some(),
            "the picker has {row}"
        );
    }
    h.get_by_role_and_label(Role::Button, "1/8 end mill (T2)")
        .click();
    h.run();
    assert_eq!(undo_label(&h), "Set tool of “Panel”");
    undo(&mut h);

    // A value the tool gives: empty, the field shows the tool's; typed,
    // the operation's own, with a button to go back.
    open_group(&mut h, "Depth");
    assert_eq!(placeholder(&h, "Step-down"), "tool's: 4 mm");
    assert!(h.query_by_label("Use the tool's value").is_none());
    type_in(&mut h, "Step-down", "2");
    assert_eq!(undo_label(&h), "Set step down of “Panel”");
    click(&mut h, "Use the tool's value");
    assert_eq!(placeholder(&h, "Step-down"), "tool's: 4 mm");

    // A problem located at a field.
    type_in(&mut h, "Step-down", "8");
    settle(&mut h);
    let warning = "operation `Panel` (`op3`) cuts 8 mm per pass, deeper than the 6.35 mm \
                   diameter of tool `1/4 end mill`; passes that deep can break the bit, so use \
                   a step-down of at most 6.35 mm";
    assert_eq!(
        problems(&h),
        [format!("warning: step_down_above_diameter: {warning}")]
    );
    assert!(
        h.query_by_label(warning).is_some(),
        "the warning is under its field"
    );
    shoot(
        &mut h,
        &out,
        "operations-field-warning",
        Crop::ViewportAndInspector,
    );
    undo(&mut h);

    // A value out of range is kept, and is an error under its field.
    open_group(&mut h, "Entry and leads");
    type_in(&mut h, "Helix angle", "60");
    settle(&mut h);
    let error = "operations[op3].entry.helix.angle_deg is 60, expected an angle greater than \
                 0 and at most 45 degrees";
    assert_eq!(problems(&h), [format!("error: {error}")]);
    assert!(h.query_by_label(error).is_some());
    undo(&mut h);
    settle(&mut h);
    assert!(problems(&h).is_empty());
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// One picture of operation `name`'s editor, with `groups` open: the
/// Viewport and the Inspector, as `<picture>.png`. Every row in `rows`
/// must be in the editor and in view, so the chapter's table of field
/// names is the picture's.
fn editor_picture(
    h: &mut H,
    out: &Path,
    name: &str,
    groups: &[&str],
    rows: &[&str],
    picture: &str,
) {
    open_operation(h, name);
    for group in groups {
        open_group(h, group);
    }
    settle(h);
    let inspector = h.state().layout().inspector.expect("the Inspector shows");
    for row in rows {
        let shown = h
            .query_all_by_label(row)
            .any(|n| inspector.contains_rect(n.rect()) && n.rect().is_positive());
        assert!(shown, "{picture}: the row {row} is in view in the editor");
    }
    shoot(h, out, picture, Crop::ViewportAndInspector);
}

/// The operation chapters' "In the app" parts (`profile.md`, `pocket.md`,
/// `drill.md`, `bore.md`, `engrave.md`, `v-carve.md`, `inlay.md`): one
/// picture of each kind's editor, on the parts the app quick start
/// builds, with the groups open that the window has room for.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn operation_editor_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-editors", &dialogs) else {
        return;
    };
    editor_picture(
        &mut h,
        &out,
        "Cut out",
        &["Depth"],
        &[
            "Targets",
            "Tool",
            "Side",
            "Depth",
            "Tab request",
            "Placement",
            "Count",
            "Step-down",
            "Through-cut allowance",
            "Strategy",
            "Entry and leads",
            "Corners and tabs",
            "Order",
            "Heights",
        ],
        "op-profile",
    );
    editor_picture(
        &mut h,
        &out,
        "Panel",
        &["Strategy"],
        &[
            "Targets",
            "Tool",
            "Depth",
            "Strategy",
            "Raster angle",
            "Direction",
            "Wall pass",
            "Stepover",
            "Rest of",
            "Entry and leads",
            "Corners and tabs",
            "Order",
            "Heights",
        ],
        "op-pocket",
    );
    editor_picture(
        &mut h,
        &out,
        "Screw holes",
        &["Depth", "Strategy", "Heights"],
        &[
            "Targets",
            "Tool",
            "Depth",
            "Pitch",
            "Stock to leave",
            "Through-cut allowance",
            "Direction",
            "Safe Z",
        ],
        "op-bore",
    );
    editor_picture(
        &mut h,
        &out,
        "Border and squiggle",
        &["Depth", "Strategy", "Order", "Heights"],
        &[
            "Targets",
            "Tool",
            "Depth",
            "Step-down",
            "Through-cut allowance",
            "Alternate direction",
            "Machining order",
            "Safe Z",
        ],
        "op-engrave",
    );
    drop(h);

    let text = app_steps::chapter();
    let sign = built(&text, "sign", "sign", &dir);
    let mut h = window(&dir, Some(sign), &dialogs).unwrap();
    editor_picture(
        &mut h,
        &out,
        "Hanging holes",
        &["Depth", "Strategy", "Heights"],
        &[
            "Targets",
            "Tool",
            "Cycle",
            "Depth",
            "Retract height",
            "Through-cut allowance",
            "Dwell",
            "Allow tool wider than hole",
            "Safe Z",
        ],
        "op-drill",
    );
    editor_picture(
        &mut h,
        &out,
        "Lettering",
        &["Strategy"],
        &[
            "Targets",
            "Tool",
            "Flat depth",
            "Clearing passes",
            "Add pass",
            "Depth",
            "Strategy",
            "Floor stepover",
            "Order",
            "Heights",
        ],
        "op-v-carve",
    );
    assert!(
        painted(&h, "Lettering · deepest cut 1.658 mm, no flat depth"),
        "the status bar's message is the selected carve's summary"
    );
    drop(h);

    let inlay = built(&text, "inlay", "dc", &dir);
    let mut h = window(&dir, Some(inlay), &dialogs).unwrap();
    editor_picture(
        &mut h,
        &out,
        "DC",
        &["Male board"],
        &[
            "Targets",
            "Tool",
            "Inlay depth",
            "Glue gap",
            "Plug depth",
            "Depth",
            "Board thickness",
            "Board width",
            "Board height",
            "Strategy",
            "Cut-out",
            "Order",
            "Heights",
        ],
        "op-inlay",
    );
    drop(h);

    // An Inlay with a drawn boundary, which the editor has no row for:
    // editing another of its values leaves the boundary as the file had
    // it.
    let drawn = common::copy_fixture("dc-boundary.dcv", &dir);
    let mut h = window(&dir, Some(drawn), &dialogs).unwrap();
    open_operation(&mut h, "DC");
    assert!(
        h.query_by_label("Boundary").is_none(),
        "no row for the boundary"
    );
    type_in(&mut h, "Glue gap", "0.8");
    let dc_project::Operation::Inlay(inlay) = &h.state().app().project().operations[0] else {
        panic!("the fixture's Inlay");
    };
    assert_eq!(inlay.glue_gap_mm, Some(0.8));
    assert_eq!(inlay.boundary, [dc_project::InstanceId::new("i3")]);
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// The project's placed tabs: each one's id and where it is on the
/// stock, mm.
fn placed_tabs(h: &H) -> Vec<(String, dc_geometry::Point)> {
    let tabs = h.state().app().scene().tabs();
    tabs.iter()
        .map(|tab| (tab.id.as_str().to_owned(), tab.at_mm))
        .collect()
}

/// The labels of the Design step's tab rows, top to bottom.
fn tab_rows(h: &H) -> Vec<String> {
    h.query_all_by_role(Role::TreeItem)
        .map(|n| n.accesskit_node().label().unwrap_or_default())
        .filter(|label| label.starts_with("tab"))
        .collect()
}

/// The chapter "Tabs" (`docs/book/src/user-guide/tabs.md`, "In the
/// app"), on the plaque: *Place tabs* on the Cut out's request, a tab
/// selected from its mark and from its row, its Properties, a drag along
/// the outline, Delete, several tabs edited at once, and the Tab tool on
/// two outlines, one of which no Profile cuts.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn tabs_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-tabs", &dialogs) else {
        return;
    };

    // Place tabs: the request of four becomes four placed tabs, the
    // request's switch goes off, and the cut is the same cut.
    open_operation(&mut h, "Cut out");
    settle(&mut h);
    assert!(h.state().app().project().tabs.is_empty());
    click_button(&mut h, "Place tabs");
    settle(&mut h);
    assert_eq!(undo_label(&h), "Place 4 tabs");
    let dc_project::Operation::Profile(cut_out) = &h.state().app().project().operations[4] else {
        panic!("the Cut out is a Profile");
    };
    assert!(
        cut_out.tabs.is_none(),
        "the request is placed, so it is gone"
    );
    assert!(
        h.get_by_role_and_label(Role::Button, "Place tabs")
            .accesskit_node()
            .is_disabled()
    );
    assert!(painted(&h, "Est. 3 min 39 s"), "the same program");
    let tabs = placed_tabs(&h);
    assert_eq!(tabs.len(), 4);

    // On the Design step they are listed under Tabs, by their outline.
    go_to(&mut h, "Design");
    assert!(h.query_by_label_contains("outline · 4 tabs").is_some());
    assert_eq!(
        tab_rows(&h),
        [
            "tab1  (82.85, 113.17)",
            "tab2  (163.18, 62.15)",
            "tab3  (87.15, 6.83)",
            "tab4  (6.83, 57.85)"
        ]
    );

    // A click on a tab's mark selects the tab, not the shape under it.
    let (id, at) = tabs[0].clone();
    let mark = common::on_screen(&h, at);
    common::click_at(&mut h, mark, egui::Modifiers::NONE);
    assert!(h.query_by_label(&format!("Properties · {id}")).is_some());
    assert_eq!(
        ["Width", "Height", "Position"].map(|label| field(&h, label)),
        ["8", "3", "82.85, 113.17"]
    );
    assert!(
        h.get_by_role_and_label(Role::TextInput, "Position")
            .accesskit_node()
            .is_disabled(),
        "the position is read, not typed"
    );
    assert!(h.query_by_label("Crossed by: Cut out.").is_some());
    shoot(&mut h, &out, "tabs-selected", Crop::Window);

    // Its size and shape.
    type_in(&mut h, "Width", "6");
    assert_eq!(undo_label(&h), "Edit tab");
    h.get_by_role_and_label(Role::RadioButton, "Smooth").click();
    h.run();
    assert_eq!(undo_label(&h), "Edit tab");
    settle(&mut h);

    // Dragged along its outline, it stays on the outline.
    let from = common::on_screen(&h, at);
    let to = common::on_screen(&h, dc_geometry::Point::new(at.x + 20.0, at.y + 3.0));
    drag_with(&mut h, from, to, egui::Modifiers::NONE);
    assert_eq!(undo_label(&h), "Move tab");
    assert_eq!(tab_rows(&h)[0], "tab1  (102.85, 110)");

    // Delete, from the key and from the button.
    h.key_press(egui::Key::Delete);
    h.run();
    assert_eq!(undo_label(&h), "Delete tab");
    assert_eq!(h.state().app().project().tabs.len(), 3);
    undo(&mut h);
    h.get_by_role_and_label(Role::TreeItem, "tab1  (102.85, 110)")
        .click();
    h.run();
    click_button(&mut h, "Delete");
    assert_eq!(h.state().app().project().tabs.len(), 3);
    undo(&mut h);

    // Several at once, from their rows.
    h.get_by_role_and_label(Role::TreeItem, "tab2  (163.18, 62.15)")
        .click();
    h.run();
    h.get_by_role_and_label(Role::TreeItem, "tab4  (6.83, 57.85)")
        .click_modifiers(egui::Modifiers::SHIFT);
    h.run();
    assert!(h.query_by_label("Properties · 2 tabs").is_some());
    type_in(&mut h, "Height", "4");
    assert_eq!(undo_label(&h), "Edit 2 tabs");
    h.key_press(egui::Key::Escape);
    h.run();
    settle(&mut h);

    // The Tab tool: each click on an outline adds a tab there, until
    // Escape. The square is pocketed, not profiled, so a tab on it is
    // crossed by no cut.
    h.key_press(egui::Key::T);
    h.run();
    let on_the_outline = at_mm(&h, 10.0, 40.0);
    common::click_at(&mut h, on_the_outline, egui::Modifiers::NONE);
    assert_eq!(undo_label(&h), "Add tab");
    let on_the_square = at_mm(&h, 70.0, 33.0);
    common::click_at(&mut h, on_the_square, egui::Modifiers::NONE);
    assert_eq!(h.state().app().project().tabs.len(), 6);
    for tab in &h.state().app().project().tabs[4..] {
        assert_eq!(
            (
                tab.width_mm.to_string().as_str(),
                tab.height_mm.to_string().as_str(),
                tab.shape
            ),
            ("8", "4", dc_project::TabShape::Rect),
            "a new tab has the size and shape last given to a tab"
        );
    }
    settle(&mut h);
    assert!(h.query_by_label("Properties · tab6").is_some());
    assert!(
        h.query_by_label_contains("No enabled Profile crosses it")
            .is_some()
    );
    assert_eq!(
        problems(&h),
        [
            "warning: tab_not_crossed: tab `tab6` on `square` (instance `i4`, profile 0) is \
          crossed by no enabled Profile operation, so it holds nothing; cut that outline \
          with a Profile, or remove the tab (`dreamcarve tabs clear <dcv> --tab tab6`)"
        ]
    );
    shoot(&mut h, &out, "tabs-tool", Crop::Window);

    // Escape ends the tool: the next click selects, and adds nothing.
    h.key_press(egui::Key::Escape);
    h.run();
    common::click_at(&mut h, on_the_outline, egui::Modifiers::NONE);
    assert_eq!(h.state().app().project().tabs.len(), 6);
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// Clicks the Warnings drawer's row named `line` (`Row::name`): the one of that name
/// that is not in the step panel.
fn click_drawer_row(h: &mut H, line: &str) {
    let panel = h.state().layout().step_panel;
    h.query_all_by_label(line)
        .find(|node| panel.is_none_or(|panel| !panel.contains_rect(node.rect())))
        .unwrap_or_else(|| panic!("no drawer row `{line}`"))
        .click();
    h.run();
}

/// The drawer's row whose line of `check` contains `needle`.
fn drawer_row(h: &H, needle: &str) -> dc_app::warnings::Row {
    dc_app::warnings::panel(h.state().app())
        .into_iter()
        .flat_map(|group| group.rows)
        .find(|row| row.line.contains(needle))
        .unwrap_or_else(|| panic!("no finding about `{needle}`"))
}

/// The chapter "Warnings" (`docs/book/src/user-guide/warnings.md`, "In
/// the app"), on the plaque given one finding of each kind through the
/// window: a feed deleted (an error), a plunge rate above its feed (a
/// warning) and a tool nothing uses (a hint). The status bar's counts,
/// the rail's marks, the drawer, a row clicked to go to its field, and
/// *Copy*.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn warnings_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-warnings", &dialogs) else {
        return;
    };
    assert!(problems(&h).is_empty());
    assert!(painted(&h, "No problems"));

    // An error: the 1/8 end mill's feed deleted. A warning: the 1/4 end
    // mill plunging faster than it cuts. A hint: a tool nothing uses.
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "1/8 end mill")
        .click();
    h.run();
    h.get_by_role_and_label(Role::TextInput, "Feed").focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.key_press(egui::Key::Backspace);
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(required(&h, "Feed"), "the feed is unset again");
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    type_in(&mut h, "Plunge", "5000");
    click_button(&mut h, "New tool");
    h.key_press(egui::Key::Enter);
    h.run();
    h.key_press(egui::Key::Escape);
    h.run();
    settle(&mut h);
    assert_eq!(
        problems(&h),
        [
            "error: tools[t2].feed_mm_min is not set (mm/min) for tool `1/8 end mill`",
            "warning: plunge_above_feed: tool `1/4 end mill` (`t1`) plunges at 5000 mm/min, \
             faster than it cuts (1500 mm/min); a plunge rate is usually well below the feed \
             rate, so check the two are not swapped",
            "warning: unused_tool: tool `t3` (`tool3`) is defined but not used by any enabled \
             operation",
        ]
    );

    // The status bar counts each kind, and the rail marks the steps that
    // hold an error: Tools, and Export, which it blocks.
    for count in ["1 error", "1 warning", "1 hint"] {
        assert!(painted(&h, count), "the status bar says {count}");
    }
    use dc_app::steps::{Severity, State};
    let states = h.state().step_states();
    for (step, state) in [
        (dc_app::steps::Step::Design, State::Complete),
        (dc_app::steps::Step::Stock, State::Complete),
        (
            dc_app::steps::Step::Tools,
            State::Attention(Severity::Error),
        ),
        (dc_app::steps::Step::Operations, State::Complete),
        (
            dc_app::steps::Step::Export,
            State::Attention(Severity::Error),
        ),
    ] {
        assert_eq!(states.of(step), state, "the rail's mark on {step:?}");
    }

    // The drawer, from the rail's button.
    go_to(&mut h, "Operations");
    h.get_by_label_contains("Warnings: 1 error").click();
    h.run();
    assert!(h.state().warnings_open());
    assert!(painted(&h, "Warnings · 1 error · 1 warning · 1 hint"));
    let link = drawer_row(&h, "plunge_above_feed").chapter_url();
    assert_eq!(
        link.as_deref(),
        Some("https://grayzu.github.io/dreamcarve/user-guide/warnings.html#plunge_above_feed"),
        "a row's code is a link to its entry in the chapter"
    );
    assert!(h.query_all_by_label("plunge_above_feed").next().is_some());
    shoot(&mut h, &out, "warnings-drawer", Crop::Window);

    // A row goes to what it is about: a tool's row to the Tools step,
    // with the tool in the Inspector. The drawer stays open, and follows
    // the project once the value is put right.
    let row = drawer_row(&h, "plunge_above_feed");
    assert_eq!(row.field.as_deref(), Some("tools[t1].plunge_mm_min"));
    click_drawer_row(&mut h, &row.name);
    assert_eq!(h.state().step(), dc_app::steps::Step::Tools);
    assert_eq!(field(&h, "Name"), "1/4 end mill");
    assert!(h.state().warnings_open(), "the drawer stays open");
    shoot(&mut h, &out, "warnings-go-to", Crop::Window);
    type_in(&mut h, "Plunge", "500");
    settle(&mut h);
    assert!(
        !problems(&h).iter().any(|l| l.contains("plunge_above_feed")),
        "and the drawer follows the project"
    );

    // Copy is `check`'s text, with its own count: a hint is a warning
    // there.
    click_button(&mut h, "Copy");
    assert_eq!(
        dc_app::warnings::copy_text(h.state().app()),
        "error: tools[t2].feed_mm_min is not set (mm/min) for tool `1/8 end mill`\n\
         warning: unused_tool: tool `t3` (`tool3`) is defined but not used by any enabled \
         operation\n\
         plaque.dcv: 1 error(s), 1 warning(s)\n"
    );
    click(&mut h, "Close the Warnings drawer");
    assert!(!h.state().warnings_open());
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// The chapter "Configuration"
/// (`docs/book/src/user-guide/configuration.md`, "The app's
/// preferences"): the Appearance menu and its three choices, what the
/// app writes to `gui.toml` beside `config.toml`, and that the file can
/// be deleted or damaged: the app starts with its defaults and writes a
/// new one.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn configuration_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-config", &dialogs) else {
        return;
    };
    let gui_toml = dir.join("config").join("gui.toml");
    let written = || std::fs::read_to_string(&gui_toml).unwrap_or_default();
    assert!(
        written().contains("plaque.dcv"),
        "opening a project writes it to the recent files"
    );

    // Appearance, from the rail: three choices, the current one ticked.
    h.get_by_label_contains("Appearance: Dark").click();
    h.run();
    for choice in ["System", "Light", "Dark"] {
        assert!(h.query_by_label(choice).is_some(), "the menu has {choice}");
    }
    shoot(&mut h, &out, "appearance-menu", Crop::StepPanel);
    click(&mut h, "Light");
    h.run();
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::Light);
    assert!(written().contains("theme = \"light\""), "written at once");

    // View ▸ Appearance is the same menu.
    click_button(&mut h, "View");
    click(&mut h, "Appearance");
    click(&mut h, "System");
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(h.state().app().prefs().theme, dc_app::Theme::System);

    // A group left open and the colour mode are remembered too.
    open_operation(&mut h, "Panel");
    open_group(&mut h, "Depth");
    click(&mut h, "By depth");
    h.run();
    drop(h);
    let text = written();
    for line in [
        "theme = \"system\"",
        "colour = \"by_depth\"",
        "\"operation.pocket.depth\" = true",
        "[visibility]",
        "[panels]",
        "[tab]",
    ] {
        assert!(text.contains(line), "gui.toml has `{line}`:\n{text}");
    }

    // Deleted, it is the defaults again: dark, the groups closed.
    std::fs::remove_file(&gui_toml).unwrap();
    let plaque = dir.with_file_name("plaque").join("plaque.dcv");
    let mut h = window(&dir, Some(plaque.clone()), &dialogs).unwrap();
    assert!(h.state().app().prefs().sections.is_empty());
    assert!(gui_toml.exists(), "and the app writes a new one");
    open_operation(&mut h, "Panel");
    assert!(
        h.query_by_role_and_label(Role::TextInput, "Step-down")
            .is_none(),
        "the Depth group is closed again"
    );
    drop(h);

    // Damaged, it is the defaults too: the app starts all the same. (The
    // window is built without `window`'s override of the appearance.)
    std::fs::write(&gui_toml, "theme = 7\n").unwrap();
    let h = Harness::builder()
        .with_size(WINDOW)
        .with_pixels_per_point(SCALE)
        .renderer(egui_kittest::wgpu::WgpuTestRenderer::new())
        .build_eframe({
            let options = options(&dir.join("config"), Some(plaque), &dialogs);
            move |cc| dreamcarve_app::Shell::new(cc, options).expect("the shell starts")
        });
    assert_eq!(
        h.state().app().prefs().theme,
        dc_app::Theme::Dark,
        "a file the app cannot read is the defaults"
    );
    assert_eq!(h.state().app().project().operations.len(), 5);
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}

/// The Controller dropdown's rows while it is open, top to bottom: each
/// bundled post-processor with its latest version.
fn controller_rows(h: &H) -> Vec<String> {
    let mut rows: Vec<(f32, String)> = h
        .query_all_by_role(Role::Button)
        .map(|n| {
            (
                n.rect().min.y,
                n.accesskit_node().label().unwrap_or_default(),
            )
        })
        .filter(|(_, label)| label.ends_with("(latest)"))
        .collect();
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.into_iter().map(|(_, label)| label).collect()
}

/// The chapter "Post-processors"
/// (`docs/book/src/user-guide/post-processors.md`, "In the app"): the
/// Export step's Post-processor group on the plaque. The six controllers,
/// a setting grblHAL has and LinuxCNC has not, and the switch that
/// resets it and says so.
#[test]
#[ignore = "writes the book's screenshots; run by hand"]
fn post_processor_screenshots() {
    let out = out_dir("user-guide");
    let dialogs = QueuedDialogs::default();
    let Some((mut h, dir)) = plaque_window("dreamcarve-book-post", &dialogs) else {
        return;
    };
    go_to(&mut h, "Export");
    settle(&mut h);
    click_button(&mut h, "Post-processor");
    let controller = |h: &H| {
        h.get_by_role_and_label(Role::ComboBox, "Controller")
            .value()
            .unwrap_or_default()
    };
    assert_eq!(controller(&h), "grblHAL v4 (latest)");
    assert_eq!(field(&h, "File extension"), "nc");
    assert_eq!(field(&h, "Spin-up dwell"), "3");
    assert!(
        h.query_by_label("Not offered: grblHAL has no canned cycles.")
            .is_some()
    );
    h.get_by_role_and_label(Role::ComboBox, "Controller")
        .click();
    h.run();
    assert_eq!(
        controller_rows(&h),
        [
            "grblHAL v4 (latest)",
            "GRBL 1.1 v3 (latest)",
            "LinuxCNC v2 (latest)",
            "Mach3 v3 (latest)",
            "UCCNC v3 (latest)",
            "Generic RS274/NGC v1 (latest)"
        ]
    );
    h.key_press(egui::Key::Escape);
    h.run();

    // One program for all tools, which grblHAL can write.
    h.get_by_role_and_label(Role::ComboBox, "Programs")
        .scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::ComboBox, "Programs").click();
    h.run();
    click(&mut h, "One program for all tools");
    settle(&mut h);
    assert_eq!(undo_label(&h), "Set post-processor settings");
    assert!(problems(&h).is_empty());

    // LinuxCNC has no M6: the switch resets that setting and says so, and
    // takes LinuxCNC's own extension, since the old one was a default.
    h.get_by_role_and_label(Role::ComboBox, "Controller")
        .click();
    h.run();
    click(&mut h, "LinuxCNC v2 (latest)");
    settle(&mut h);
    assert_eq!(undo_label(&h), "Switch post-processor to linuxcnc");
    assert_eq!(controller(&h), "LinuxCNC v2 (latest)");
    assert_eq!(field(&h, "File extension"), "ngc");
    assert!(
        h.query_by_label_contains(
            "Reset tool_change_mode \"m6\" → \"separate_programs\": post-processor linuxcnc has \
             no M6 tool change."
        )
        .is_some()
    );
    assert!(
        h.query_by_role_and_label(Role::CheckBox, "Canned drill cycles")
            .is_some(),
        "LinuxCNC has canned cycles, so the tick is offered"
    );
    shoot(&mut h, &out, "post-processor-switched", Crop::StepPanel);
    h.get_by_role_and_label(Role::ComboBox, "Programs")
        .scroll_to_me();
    h.run();
    h.get_by_role_and_label(Role::ComboBox, "Programs").click();
    h.run();
    assert!(
        h.query_all_by_label("One program for all tools")
            .all(|choice| choice.accesskit_node().is_disabled()),
        "a mode the controller lacks is listed and off"
    );
    h.key_press(egui::Key::Escape);
    h.run();

    // Undo puts the old controller back, with the setting it had.
    undo(&mut h);
    assert_eq!(controller(&h), "grblHAL v4 (latest)");
    assert_eq!(
        h.state().app().project().post_processor.tool_change_mode,
        dc_project::ToolChangeMode::M6
    );
    drop(h);
    std::fs::remove_dir_all(dir.parent().unwrap()).ok();
}
