// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A field's problem takes no room (`docs/specs/phase-4/experience.md`,
//! section 7, "Field problem"; functional spec, capabilities 60, 63, 66
//! and 66b; the maintainer's review, 2026-11-20): a field with an error,
//! a warning or a hint shows its outline and its glyph, and no message
//! in the layout. Each test reads the window through the accessibility
//! tree: the glyph is a node named with the problems, the field's
//! description carries the same words, hovering either shows them, the
//! panel's help line shows them while the field has the keyboard, and a
//! click on the glyph opens the Warnings drawer at the finding's row.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

mod common;

use common::{QueuedDialogs, copy_fixture, go_to, harness, open_operation, options, scratch};
use dc_job::{Finding, Tier};
use dreamcarve_app::{Shell, theme};
use eframe::egui;
use eframe::egui::accesskit::{Role, Toggled};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT as _, Queryable as _};

/// The plaque, open in a 1280 × 800 window, with its scratch directory.
fn plaque(name: &str) -> (Harness<'static, Shell>, std::path::PathBuf) {
    let dir = scratch(name);
    let plaque = copy_fixture("plaque.dcv", &dir);
    let dialogs = QueuedDialogs::default();
    let h = harness(
        options(&dir.join("config"), Some(plaque), &dialogs),
        1280.0,
        800.0,
    );
    (h, dir)
}

/// Focuses the text field named `label`, types `text` over what it holds
/// and leaves it with Tab.
fn type_in(h: &mut Harness<'_, Shell>, label: &str, text: &str) {
    h.get_by_role_and_label(Role::TextInput, label).focus();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    h.run();
    h.event(egui::Event::Text(text.to_owned()));
    h.run();
    h.key_press(egui::Key::Tab);
    h.run();
    common::settle(h);
}

/// Steps the window until a tooltip has had time to show (egui's
/// `tooltip_delay`, 0.5 s), at 0.1 s a frame.
fn wait_for_tooltip(h: &mut Harness<'_, Shell>) {
    for _ in 0..10 {
        h.step();
    }
    h.run();
}

/// The findings the app shows that are located at `field`.
fn findings_at(h: &Harness<'_, Shell>, field: &str) -> Vec<Finding> {
    h.state()
        .app()
        .warnings()
        .iter()
        .filter(|f| f.field.as_deref() == Some(field))
        .cloned()
        .collect()
}

/// A finding as a field's glyph and description word it: the tier's
/// word, the code when it has one, the message.
fn line(finding: &Finding) -> String {
    let word = match finding.tier {
        Tier::Error => "error",
        Tier::Warning => "warning",
        Tier::Hint => "hint",
    };
    match &finding.code {
        Some(code) => format!("{word}: {code}: {}", finding.message),
        None => format!("{word}: {}", finding.message),
    }
}

/// Where the Lucide glyph `name` is painted this frame, by its codepoint
/// in the bundled icon font's own map.
fn glyph_rects(h: &Harness<'_, Shell>, name: &str) -> Vec<egui::Rect> {
    let key = format!("\"{name}\": ");
    let glyph = include_str!("../assets/icons/lucide/codepoints.json")
        .lines()
        .find_map(|line| line.trim().strip_prefix(key.as_str()))
        .and_then(|code| code.trim_end_matches(',').parse::<u32>().ok())
        .and_then(char::from_u32)
        .unwrap_or_else(|| panic!("the bundled map names {name}"))
        .to_string();
    h.output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Text(text) if text.galley.text() == glyph => {
                Some(egui::Rect::from_min_size(text.pos, text.galley.size()))
            }
            _ => None,
        })
        .collect()
}

/// The rectangles painted this frame with a 1-px outline in `colour`.
fn outlines(h: &Harness<'_, Shell>, colour: egui::Color32) -> Vec<egui::Rect> {
    h.output()
        .shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) if rect.stroke.color == colour && rect.stroke.width > 0.0 => {
                Some(rect.rect)
            }
            _ => None,
        })
        .collect()
}

/// The node `role` named `label`.
fn node<'h>(h: &'h Harness<'_, Shell>, role: Role, label: &'h str) -> egui_kittest::Node<'h> {
    h.query_all_by_role_and_label(role, label)
        .next()
        .unwrap_or_else(|| panic!("no {role:?} named {label}"))
}

/// Asserts the control `role` named `label` is marked with `problems`
/// (each a line as the glyph words it, most severe first), `glyph` being
/// the worst's: no message in the layout, the glyph on the control's
/// line inside or just beside it and named with the problems, the
/// control outlined in `stroke`, and its accessible description starting
/// with the same words. Returns the glyph's node rectangle.
fn assert_marked(
    h: &Harness<'_, Shell>,
    (role, label): (Role, &str),
    problems: &[String],
    glyph: &str,
    stroke: egui::Color32,
) -> egui::Rect {
    let text = problems.join("\n");
    let control = node(h, role, label);
    let frame = control.rect();
    // No message line: nothing in the window is named with a problem's
    // words but the one glyph, and none with its message alone.
    for problem in problems {
        for (at, _) in problem.match_indices(": ") {
            let message = &problem[at + 2..];
            assert!(
                h.query_by_label(message).is_none(),
                "{label}: no line carries the message `{message}`"
            );
        }
    }
    let marks: Vec<_> = h.query_all_by_label(&text).collect();
    assert_eq!(marks.len(), 1, "{label}: one glyph named `{text}`");
    let mark = marks[0].rect();
    assert!(
        frame.y_range().contains(mark.center().y)
            && mark.center().x > frame.left()
            && mark.center().x < frame.right() + 32.0,
        "{label}: the glyph {mark:?} is on the control's line, in it or beside it: {frame:?}"
    );
    let painted: Vec<egui::Rect> = glyph_rects(h, glyph)
        .into_iter()
        .filter(|at| mark.contains(at.center()))
        .collect();
    assert_eq!(painted.len(), 1, "{label}: `{glyph}` painted in its node");
    assert!(
        outlines(h, stroke).iter().any(|outline| {
            outline.y_range().contains(mark.center().y)
                && outline.left() <= frame.left() + 4.0
                && outline.height() <= frame.height() + 1.0
        }),
        "{label}: outlined in its status colour"
    );
    let described = control.accesskit_node().description().unwrap_or_default();
    assert!(
        described.starts_with(&text),
        "{label}: described with its problems first: `{described}`"
    );
    mark
}

/// The help line at the foot of a panel: the node whose name holds
/// `label` and a colon at the start of one of its lines, when it shows.
fn help_line(h: &Harness<'_, Shell>, label: &str) -> Option<(String, egui::Rect)> {
    let start = format!("{label}: ");
    h.query_all_by_role(Role::Label)
        .filter_map(|node| {
            let node_ak = node.accesskit_node();
            let name = node_ak.label().or_else(|| node_ak.value())?;
            name.lines()
                .any(|line| line.starts_with(&start))
                .then(|| (name, node.rect()))
        })
        .next()
}

/// Sets the plaque's Cut out to 0 tabs through its Count field: the file
/// takes the value and validation locates an error at it, which is not
/// an unset value, so the field keeps its `0`.
fn zero_tabs(h: &mut Harness<'_, Shell>) -> Finding {
    open_operation(h, "Cut out");
    type_in(h, "Count", "0");
    let found = findings_at(h, "operations[op4].tabs.placement.count");
    assert_eq!(found.len(), 1, "validation locates the count: {found:?}");
    assert_eq!(found[0].tier, Tier::Error);
    found[0].clone()
}

/// Gives the plaque's 1/4 end mill a plunge rate above its feed, in its
/// editor: `plunge_above_feed`, a warning located at the plunge.
fn plunge_above_feed(h: &mut Harness<'_, Shell>) -> Finding {
    go_to(h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    let feed: f64 = h
        .get_by_role_and_label(Role::TextInput, "Feed")
        .value()
        .unwrap_or_default()
        .parse()
        .expect("the plaque's end mill has a feed");
    type_in(h, "Plunge", &format!("{}", feed + 500.0));
    let found: Vec<Finding> = h
        .state()
        .app()
        .warnings()
        .iter()
        .filter(|f| f.code.as_deref() == Some("plunge_above_feed"))
        .cloned()
        .collect();
    assert_eq!(found.len(), 1, "validation warns of the plunge: {found:?}");
    assert_eq!(found[0].tier, Tier::Warning);
    found[0].clone()
}

/// The status strokes of both appearances: a test window's theme is the
/// machine's.
fn strokes(of: impl Fn(&theme::Palette) -> egui::Color32) -> [egui::Color32; 2] {
    [of(&theme::DARK), of(&theme::LIGHT)]
}

/// [`assert_marked`] in whichever appearance the window has.
fn assert_marked_any(
    h: &Harness<'_, Shell>,
    control: (Role, &str),
    problems: &[String],
    glyph: &str,
    strokes: [egui::Color32; 2],
) -> egui::Rect {
    let stroke = strokes
        .into_iter()
        .find(|stroke| !outlines(h, *stroke).is_empty())
        .unwrap_or_else(|| panic!("{}: no outline in a status colour", control.1));
    assert_marked(h, control, problems, glyph, stroke)
}

/// The maintainer's review, item 1 (capability 66; "Field problem"): an
/// error located at a field draws no message under it. The field has the
/// error outline and `circle-x` inside it, the glyph is named with the
/// finding as the drawer words it, the field's description starts with
/// the same words, and hovering the field shows them as a tooltip: the
/// tier's word, the message, with no code since an error has none.
#[test]
fn an_error_marks_its_field_and_draws_no_message() {
    let (mut h, dir) = plaque("problem-error");
    let finding = zero_tabs(&mut h);
    let words = line(&finding);
    assert_marked_any(
        &h,
        (Role::TextInput, "Count"),
        std::slice::from_ref(&words),
        "circle-x",
        strokes(|p| p.status_error.stroke),
    );
    // The tooltip: the message itself, shown only once the pointer rests
    // on the field.
    h.get_by_role_and_label(Role::TextInput, "Count").hover();
    wait_for_tooltip(&mut h);
    assert!(
        h.query_by_label(&finding.message).is_some(),
        "the tooltip says the message: {}",
        finding.message
    );
    assert!(h.query_by_label("error").is_some(), "under its tier's word");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The same for a warning (capability 63): `plunge_above_feed` at a
/// tool's Plunge field is the warning outline and `triangle-alert`, and
/// its tooltip adds the code, which the drawer shows in the mono face.
#[test]
fn a_warning_marks_its_field_and_draws_no_message() {
    let (mut h, dir) = plaque("problem-warning");
    let finding = plunge_above_feed(&mut h);
    let words = line(&finding);
    assert!(words.starts_with("warning: plunge_above_feed: "), "{words}");
    let mark = assert_marked_any(
        &h,
        (Role::TextInput, "Plunge"),
        std::slice::from_ref(&words),
        "triangle-alert",
        strokes(|p| p.status_warning.stroke),
    );
    // The glyph is inside the field, before its unit.
    let field = h.get_by_role_and_label(Role::TextInput, "Plunge").rect();
    assert!(
        mark.center().x > field.center().x,
        "the glyph is at the field's right: {mark:?} in {field:?}"
    );
    h.event(egui::Event::PointerMoved(mark.center()));
    wait_for_tooltip(&mut h);
    assert!(
        h.query_by_label(&finding.message).is_some(),
        "the glyph's tooltip says the message"
    );
    assert!(
        h.query_by_label("plunge_above_feed").is_some(),
        "and the code"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The same for a hint, and for a dropdown (capability 62): a project on
/// an older version of its post-processor says so on the Controller
/// dropdown with the hint outline and the `info` glyph inside it, before
/// the chevron, not in a line under the row.
#[test]
fn a_hint_marks_its_dropdown_and_draws_no_message() {
    let dir = scratch("problem-hint");
    let path = copy_fixture("plaque.dcv", &dir);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"definition_version\": 4"));
    std::fs::write(
        &path,
        text.replace("\"definition_version\": 4", "\"definition_version\": 3"),
    )
    .unwrap();
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&dir.join("config"), Some(path), &dialogs),
        1280.0,
        800.0,
    );
    common::settle(&mut h);
    go_to(&mut h, "Export");
    common::settle(&mut h);
    h.get_by_role_and_label(Role::Button, "Post-processor")
        .click();
    h.run();
    let words = "hint: This project uses v3; v4 is the latest. Pick it to upgrade.".to_owned();
    let mark = assert_marked_any(
        &h,
        (Role::ComboBox, "Controller"),
        std::slice::from_ref(&words),
        "info",
        strokes(|p| p.status_info.stroke),
    );
    let dropdown = h.get_by_role_and_label(Role::ComboBox, "Controller").rect();
    assert!(
        dropdown.contains(mark.center()) && mark.right() < dropdown.right() - 20.0,
        "inside the dropdown, clear of its chevron: {mark:?} in {dropdown:?}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A refused entry (capability 52; "Field problem"): what the field
/// would not take is said by the field's own glyph and tooltip, why and
/// what it takes, not under it. It is not a finding of the project, so
/// the glyph is not a button and a click on it opens no drawer.
#[test]
fn a_refused_entry_marks_its_field_and_opens_no_drawer() {
    let (mut h, dir) = plaque("problem-refused");
    go_to(&mut h, "Stock");
    let before = h.state().app().project().stock.width_mm;
    type_in(&mut h, "Width", "wide");
    assert_eq!(h.state().app().project().stock.width_mm, before);
    let reason = dc_app::units::parse_length_mm("wide", dc_project::Units::Mm)
        .unwrap_err()
        .to_string();
    let words = format!("error: {reason}");
    let mark = assert_marked_any(
        &h,
        (Role::TextInput, "Width"),
        std::slice::from_ref(&words),
        "circle-x",
        strokes(|p| p.status_error.stroke),
    );
    assert!(
        h.query_all_by_role_and_label(Role::Button, &words)
            .next()
            .is_none(),
        "the glyph of a refused entry is no button"
    );
    common::click_at(&mut h, mark.center(), egui::Modifiers::NONE);
    assert!(!h.state().warnings_open(), "it opens no drawer");
    // A value the field takes clears it.
    type_in(&mut h, "Width", "200");
    assert!(h.query_by_label(&words).is_none(), "the mark is gone");
    assert_eq!(h.state().app().project().stock.width_mm, 200.0);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A field with two problems (capability 66): the most severe sets the
/// outline and the glyph, and the one glyph names both, the refusal of
/// what was just typed before the finding the file's value still has;
/// the tooltip lists both.
#[test]
fn a_field_with_two_problems_lists_both_most_severe_first() {
    let (mut h, dir) = plaque("problem-two");
    let finding = zero_tabs(&mut h);
    // Typed over with what is no count: refused, and the file's 0 stays.
    type_in(&mut h, "Count", "a few");
    let both: Vec<String> = h
        .query_all_by_role(Role::Button)
        .filter_map(|node| node.accesskit_node().label())
        .filter(|name| name.ends_with(&line(&finding)) && name.contains('\n'))
        .collect();
    assert_eq!(both.len(), 1, "one glyph names both problems: {both:?}");
    let lines: Vec<String> = both[0].lines().map(str::to_owned).collect();
    assert_eq!(lines.len(), 2, "{lines:?}");
    assert!(lines[0].starts_with("error: "), "{lines:?}");
    assert_eq!(lines[1], line(&finding), "the finding is second");
    assert_marked_any(
        &h,
        (Role::TextInput, "Count"),
        &lines,
        "circle-x",
        strokes(|p| p.status_error.stroke),
    );
    h.get_by_role_and_label(Role::TextInput, "Count").hover();
    wait_for_tooltip(&mut h);
    assert!(h.query_by_label(&finding.message).is_some());
    let refusal = lines[0].trim_start_matches("error: ");
    assert!(
        h.query_by_label(refusal).is_some(),
        "the tooltip lists the refusal too: {refusal}"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The reverse trip (capability 66): a click on a field's glyph opens
/// the Warnings drawer with the finding's row selected and in view.
#[test]
fn clicking_a_glyph_opens_the_drawer_at_the_finding() {
    let (mut h, dir) = plaque("problem-drawer");
    let finding = plunge_above_feed(&mut h);
    let words = line(&finding);
    assert!(!h.state().warnings_open());
    let glyph = node(&h, Role::Button, &words).rect();
    common::click_at(&mut h, glyph.center(), egui::Modifiers::NONE);
    h.run();
    assert!(h.state().warnings_open(), "the glyph opens the drawer");
    let drawer = h.state().layout().drawer.expect("the drawer");
    let rows: Vec<_> = h
        .query_all_by_role_and_label(Role::Button, &words)
        .filter(|node| drawer.contains(node.rect().center()))
        .collect();
    assert_eq!(rows.len(), 1, "the finding's row is in view in the drawer");
    assert_eq!(
        rows[0].accesskit_node().toggled(),
        Some(Toggled::True),
        "and selected"
    );
    // No other row is.
    let selected = h
        .query_all_by_role(Role::Button)
        .filter(|node| {
            drawer.contains(node.rect().center())
                && node.accesskit_node().toggled() == Some(Toggled::True)
        })
        .count();
    assert_eq!(selected, 1);
    // Closing the drawer forgets the row.
    h.get_by_label("Close the Warnings drawer").click();
    h.run();
    assert!(!h.state().warnings_open());
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A hover is not reachable by keyboard ("Help affordance"): while a
/// field with a problem has the keyboard, the help line at the foot of
/// its panel shows the problem, before the field's ordinary help, and
/// the line is clear of the field.
#[test]
fn the_focused_fields_problem_shows_in_the_help_line() {
    let (mut h, dir) = plaque("problem-help-line");
    let finding = plunge_above_feed(&mut h);
    assert!(help_line(&h, "Plunge").is_none(), "nothing while unfocused");
    h.get_by_role_and_label(Role::TextInput, "Plunge").focus();
    h.run();
    let (text, at) = help_line(&h, "Plunge").expect("the plunge's help line");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "the problem, then the help: {text}");
    assert_eq!(lines[0], line(&finding), "the problem first");
    assert!(lines[1].starts_with("Plunge: "), "then the help: {text}");
    let inspector = h.state().layout().inspector.expect("the Inspector");
    assert!(
        inspector.contains_rect(at) && at.bottom() > inspector.bottom() - 12.0,
        "at the foot of the Inspector: {at:?} in {inspector:?}"
    );
    let field = h.get_by_role_and_label(Role::TextInput, "Plunge").rect();
    assert!(field.bottom() <= at.top(), "clear of the field");
    // A field with no problem has its help alone.
    h.get_by_role_and_label(Role::TextInput, "Feed").focus();
    h.run();
    let (text, _) = help_line(&h, "Feed").expect("the feed's help line");
    assert_eq!(text.lines().count(), 1, "{text}");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Leaves whatever field has the keyboard, so no help line shows.
fn unfocus(h: &mut Harness<'_, Shell>) {
    h.key_press(egui::Key::Escape);
    h.run();
}

/// Where every text field of the panel is, top to bottom, by name.
fn field_tops(h: &Harness<'_, Shell>, within: egui::Rect) -> Vec<(String, f32, f32)> {
    let mut tops: Vec<(String, f32, f32)> = h
        .query_all_by_role(Role::TextInput)
        .filter(|node| within.contains(node.rect().center()))
        .map(|node| {
            let rect = node.rect();
            (
                node.accesskit_node().label().unwrap_or_default(),
                rect.top(),
                rect.height(),
            )
        })
        .collect();
    tops.sort_by(|a, b| a.1.total_cmp(&b.1));
    tops
}

/// Nothing moves when a problem comes or goes ("Field problem"): every
/// field of the tool editor is where it was and as tall as it was, with
/// a warning on the Plunge field and a refused entry in none, and every
/// field of the Stock step likewise with a refused Width. The row is the
/// plain row.
#[test]
fn a_rows_height_is_the_same_with_and_without_a_problem() {
    let (mut h, dir) = plaque("problem-height");
    go_to(&mut h, "Tools");
    h.get_by_role_and_label(Role::ListItem, "1/4 end mill")
        .click();
    h.run();
    let inspector = h.state().layout().inspector.expect("the Inspector");
    let plain = field_tops(&h, inspector);
    assert!(plain.len() >= 8, "the tool's fields: {plain:?}");
    plunge_above_feed(&mut h);
    // The focus left the Plunge field for the next, whose help line is
    // at the panel's foot and moves no row either.
    unfocus(&mut h);
    assert_eq!(field_tops(&h, inspector), plain, "with a warning");

    go_to(&mut h, "Stock");
    let panel = h.state().layout().step_panel.expect("the step panel");
    let plain = field_tops(&h, panel);
    type_in(&mut h, "Width", "wide");
    unfocus(&mut h);
    assert_eq!(field_tops(&h, panel), plain, "with a refused entry");

    // An operation's row with an error: the rows under it stay put.
    open_operation(&mut h, "Cut out");
    let inspector = h.state().layout().inspector.expect("the Inspector");
    let plain = field_tops(&h, inspector);
    zero_tabs(&mut h);
    unfocus(&mut h);
    assert_eq!(field_tops(&h, inspector), plain, "with an error");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// A file dropped on the window: a path on disk.
#[derive(Debug)]
struct Dropped(std::path::PathBuf);

impl egui::DroppedFile for Dropped {
    fn path(&self) -> &std::path::Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

/// The Import drawing sheet (capability 55): a DXF that declares no
/// units has its Units dropdown in the error state, the importer's
/// message its glyph, and a join tolerance the field refuses marks that
/// field. Neither writes a line into the sheet, so the sheet is the size
/// it was and every row is where it was.
#[test]
fn an_import_sheets_problems_mark_their_fields_and_move_nothing() {
    let (mut h, dir) = plaque("problem-import");
    go_to(&mut h, "Design");
    h.input_mut()
        .dropped_files
        .push(std::sync::Arc::new(Dropped(common::fixture(
            "dxf/no_units.dxf",
        ))));
    h.run();
    let refusal = dc_import::import_dxf(
        &std::fs::read(common::fixture("dxf/no_units.dxf")).unwrap(),
        &dc_import::DxfImportOptions::new(),
    )
    .unwrap_err()
    .to_string();
    assert_marked_any(
        &h,
        (Role::ComboBox, "Units"),
        &[format!("error: {refusal}")],
        "circle-x",
        strokes(|p| p.status_error.stroke),
    );
    // The sheet is as tall as its content: its footer's button says
    // where it ends.
    let footer = h.get_by_label("Add shapes").rect();
    let tolerance = h
        .get_by_role_and_label(Role::TextInput, "Join tolerance")
        .rect();

    type_in(&mut h, "Join tolerance", "tight");
    let reason = dc_app::units::parse_length_mm("tight", dc_project::Units::Mm)
        .unwrap_err()
        .to_string();
    assert!(
        h.query_by_label(&format!("error: {reason}")).is_some(),
        "the refused tolerance marks its field"
    );
    assert!(h.query_by_label(&reason).is_none(), "and draws no line");
    assert_eq!(
        h.get_by_label("Add shapes").rect(),
        footer,
        "the sheet is the size it was"
    );
    assert_eq!(
        h.get_by_role_and_label(Role::TextInput, "Join tolerance")
            .rect()
            .top(),
        tolerance.top(),
        "the row is where it was"
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// The Tool Library's editing pane (capability 64a): a value its field
/// refuses marks the field, glyph and description, with no line under
/// it, and the pane's rows stay where they were.
#[test]
fn a_library_fields_refused_entry_marks_it_and_moves_no_row() {
    let dir = scratch("problem-library");
    let config = dir.join("config");
    std::fs::create_dir_all(&config).unwrap();
    let bytes = std::fs::read(common::fixture("fusion/sample.tools")).unwrap();
    let library = dc_app::library::import(&dc_library::Library::default(), &bytes, false)
        .unwrap()
        .library;
    std::fs::write(
        config.join("tools.json"),
        dc_library::save(&library).unwrap(),
    )
    .unwrap();
    let dialogs = QueuedDialogs::default();
    let mut h = harness(
        options(&config, Some(common::fixture("plaque.dcv")), &dialogs),
        1280.0,
        800.0,
    );
    h.get_by_label("Project").click();
    h.run();
    h.get_by_label("Tool Library…").click();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "5 mm Drill")
        .scroll_to_me();
    h.run();
    h.get_by_role(Role::Dialog)
        .get_by_role_and_label(Role::ListItem, "5 mm Drill")
        .click();
    h.run();
    let everywhere = egui::Rect::EVERYTHING;
    let footer = h.get_by_label("Add to project").rect();
    let plain = field_tops(&h, everywhere);
    assert!(plain.len() >= 8, "the pane's fields: {plain:?}");

    type_in(&mut h, "Feed", "fast");
    unfocus_in_sheet(&mut h);
    let words: Vec<String> = h
        .query_all_by_role(Role::Image)
        .filter_map(|node| node.accesskit_node().label())
        .filter(|name| name.starts_with("error: ") && name.contains("fast"))
        .collect();
    assert_eq!(words.len(), 1, "the Feed's glyph says why: {words:?}");
    let message = words[0].trim_start_matches("error: ");
    assert!(
        h.query_by_label(message).is_none(),
        "no line under the field"
    );
    assert!(
        h.get_by_role(Role::Dialog)
            .get_by_role_and_label(Role::TextInput, "Feed")
            .accesskit_node()
            .description()
            .is_some_and(|d| d.starts_with(&words[0])),
        "the field is described with it"
    );
    assert_eq!(field_tops(&h, everywhere), plain, "no row moved");
    assert_eq!(h.get_by_label("Add to project").rect(), footer);
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Leaves the sheet's focused field for its title, which takes no
/// focus, without Escape (which closes a sheet).
fn unfocus_in_sheet(h: &mut Harness<'_, Shell>) {
    let title = h.get_by_label("Tool Library").rect().center();
    common::click_at(h, title, egui::Modifiers::NONE);
}
