// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The start screen (functional spec, capability 79;
//! `docs/specs/phase-4/redesign.md`, "The start screen"): what the
//! Viewport's card shows, in place of an empty stock, while the project
//! has no shapes, pictures or operations and the Design step is open. A
//! heading and one sentence; **New from drawing…** (the one primary
//! button), **New from picture…**, **Draw from scratch**, which arms the
//! drawing tool on the empty stock (phase 5), and a quiet *Open…*; the three quick
//! starts' projects, each a card with a thumbnail drawn from the
//! example's own shapes; and the recent files. It goes when a shape
//! arrives or another step is opened, where the stock shows.
//!
//! *New from drawing…* starts a new project from the chosen file, so the
//! stock is sized round the drawing. *New from picture…* does the same
//! round a picture, with the Photo V-Carve that carves it (File ▸ New
//! Project from Picture…).

use std::path::Path;

use dc_app::StrKey;
use dc_app::onboarding::{THUMBNAIL_HEIGHT_PX, THUMBNAIL_WIDTH_PX, Thumbnail};
use eframe::egui::{self, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::files;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, Example, FrameState, shortcut_text};
use crate::widgets::button::{ButtonSpec, Kind, button, focus_ring, text};
use crate::widgets::icon::Icon;

/// The screen's content width, px.
const WIDTH_PX: f32 = 624.0;
/// The room kept clear round the content, px.
const MARGIN_PX: f32 = 24.0;
/// The gap between the screen's blocks, px.
const BLOCK_GAP_PX: f32 = 32.0;
/// The gap between the three example cards, px.
const CARD_GAP_PX: f32 = 12.0;
/// An example card's padding, px.
const CARD_PADDING_PX: f32 = 12.0;
/// The well an example card's thumbnail sits in, px high.
const WELL_PX: f32 = THUMBNAIL_HEIGHT_PX + 16.0;
/// An example card's height, px: the well, the name and one line.
const CARD_PX: f32 = CARD_PADDING_PX + WELL_PX + 12.0 + 18.0 + 2.0 + 16.0 + CARD_PADDING_PX;
/// The start screen's buttons, px high.
const BUTTON_PX: f32 = 40.0;
/// A block heading's height with the gap under it, px.
const HEADING_PX: f32 = 20.0 + 10.0;
/// A recent file's row, px high.
const RECENT_PX: f32 = 32.0;
/// How many recent files the screen lists.
const RECENT_MAX: usize = 4;

/// Draws the start screen centred on `pane`, the Viewport's card.
pub(crate) fn screen(ui: &mut Ui, pane: Rect, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let recent: Vec<&Path> = state
        .recent
        .iter()
        .take(RECENT_MAX)
        .map(std::path::PathBuf::as_path)
        .collect();
    #[allow(clippy::cast_precision_loss, reason = "at most four rows")]
    let recent_px = if recent.is_empty() {
        0.0
    } else {
        BLOCK_GAP_PX + HEADING_PX + RECENT_PX * recent.len() as f32
    };
    let height =
        24.0 + 6.0 + 20.0 + 24.0 + BUTTON_PX + BLOCK_GAP_PX + HEADING_PX + CARD_PX + recent_px;
    let width = WIDTH_PX.min(pane.width() - 2.0 * MARGIN_PX).max(0.0);
    let top = (pane.center().y - height / 2.0).max(pane.top() + MARGIN_PX);
    let rect = Rect::from_min_size(
        egui::pos2(pane.center().x - width / 2.0, top),
        egui::vec2(width, height),
    );
    let mut column = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    column.set_clip_rect(pane.intersect(ui.clip_rect()));
    let ui = &mut column;
    ui.spacing_mut().item_spacing.y = 0.0;
    ui.set_width(width);
    text(
        ui,
        strings.get(StrKey::AppEmptyHeading),
        TextStyle::Name(theme::DISPLAY.into()),
        palette.text_primary,
    );
    ui.add_space(6.0);
    text(
        ui,
        strings.get(StrKey::AppEmptyIntro),
        TextStyle::Body,
        palette.text_secondary,
    );
    ui.add_space(24.0);
    actions(ui, state, out);
    ui.add_space(BLOCK_GAP_PX);
    heading(ui, strings.get(StrKey::AppEmptyExamples));
    examples(ui, state, out);
    if !recent.is_empty() {
        ui.add_space(BLOCK_GAP_PX);
        heading(ui, strings.get(StrKey::AppEmptyRecent));
        for path in recent {
            recent_row(ui, path, out);
        }
    }
}

/// A block's heading: 13-px SemiBold.
fn heading(ui: &mut Ui, label: &str) {
    let palette = Palette::current(ui);
    text(
        ui,
        label,
        TextStyle::Name(theme::TITLE.into()),
        palette.text_primary,
    );
    ui.add_space(10.0);
}

/// The three ways in: a new project from a drawing, one from a picture,
/// or an existing project.
fn actions(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = theme::GAP_PX;
        let drawing = ButtonSpec::new(Kind::Primary, strings.get(StrKey::AppEmptyImportDrawing))
            .icon(Icon::FileInput, theme::ICON_SMALL_PX)
            .height(BUTTON_PX)
            .tooltip(strings.get(StrKey::AppEmptyImportDrawingHint));
        if button(ui, drawing).clicked() {
            // On a project with nothing in it the drawing starts the
            // project: the stock is sized round it (capability 79).
            out.push(Action::NewFromDrawing);
        }
        let picture = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppEmptyImportPicture))
            .icon(Icon::ImagePlus, theme::ICON_SMALL_PX)
            .height(BUTTON_PX)
            .tooltip(strings.get(StrKey::AppEmptyImportPictureHint));
        if button(ui, picture).clicked() {
            out.push(Action::NewFromPicture);
        }
        // Phase 5: a design can start on the empty stock (capability 81).
        let draw = ButtonSpec::new(Kind::Secondary, strings.get(StrKey::AppEmptyDraw))
            .icon(Icon::PencilLine, theme::ICON_SMALL_PX)
            .height(BUTTON_PX)
            .tooltip(strings.get(StrKey::AppEmptyDrawHint));
        if button(ui, draw).clicked() {
            out.push(Action::Arm(dc_app::Tool::Draw(
                crate::viewport::held_drawing(state.app),
            )));
        }
        let shortcut = shortcut_text(ui.ctx(), Modifiers::COMMAND, Key::O);
        let open = ButtonSpec::new(Kind::Quiet, strings.get(StrKey::AppEmptyOpenProject))
            .icon(Icon::FolderOpen, theme::ICON_SMALL_PX)
            .height(BUTTON_PX)
            .tooltip(&shortcut);
        if button(ui, open).clicked() {
            out.push(Action::Open);
        }
    });
}

/// The quick starts' projects, three cards in a row.
fn examples(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    #[allow(clippy::cast_precision_loss, reason = "three cards")]
    let count = state.examples.len().max(1) as f32;
    let width = ((ui.available_width() - CARD_GAP_PX * (count - 1.0)) / count).max(0.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = CARD_GAP_PX;
        for (example, thumbnail) in state.examples {
            let name = strings.get(example.name());
            let teaches = strings.get(example.teaches());
            if example_card(ui, *example, thumbnail, width, (name, teaches)).clicked() {
                out.push(Action::OpenExample(*example));
            }
        }
    });
}

/// An example card (`experience.md`, section 7, "Example card"): a card
/// inside the card, `width` wide, with the thumbnail in a well over
/// `name` and the one line `teaches`. The whole card is the button,
/// named `name`.
fn example_card(
    ui: &mut Ui,
    example: Example,
    thumbnail: &Thumbnail,
    width: f32,
    (name, teaches): (&str, &str),
) -> egui::Response {
    let palette = Palette::current(ui);
    let (_, rect) = ui.allocate_space(egui::vec2(width, CARD_PX));
    // The card's fill goes under its text, and its hover wash is known
    // only once the card, the topmost widget here, has been added.
    let background = ui.painter().add(egui::Shape::Noop);
    let inner = rect.shrink(CARD_PADDING_PX);
    let well = Rect::from_min_size(inner.min, egui::vec2(inner.width(), WELL_PX));
    let picture = Rect::from_center_size(
        well.center(),
        egui::vec2(THUMBNAIL_WIDTH_PX, THUMBNAIL_HEIGHT_PX),
    );
    let name_galley = ui.painter().layout_no_wrap(
        name.to_owned(),
        TextStyle::Name(theme::MEDIUM_STYLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    let name_top = well.bottom() + 12.0;
    let line = Rect::from_min_max(
        egui::pos2(inner.left(), name_top + 18.0 + 2.0),
        egui::pos2(inner.right(), inner.bottom()),
    );
    // The line is a label, so assistive tools read it; the name is the
    // button's.
    let mut line_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(line)
            .layout(egui::Layout::left_to_right(egui::Align::Min)),
    );
    text(
        &mut line_ui,
        teaches,
        TextStyle::Small,
        palette.text_secondary,
    );

    let response = ui.interact(
        rect,
        egui::Id::unique(("empty-example", example)),
        Sense::click(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));

    if ui.is_rect_visible(rect) {
        let wash = if response.is_pointer_button_down_on() {
            palette.surface_active
        } else if response.hovered() {
            palette.surface_hover
        } else {
            theme::NONE
        };
        let border = Stroke::new(1.0, palette.border_default);
        let fill = egui::Shape::Vec(vec![
            egui::Shape::rect_filled(rect, theme::INNER_CARD_RADIUS_PX, palette.surface_raised),
            egui::Shape::rect_filled(rect, theme::INNER_CARD_RADIUS_PX, wash),
            egui::Shape::rect_stroke(
                rect,
                theme::INNER_CARD_RADIUS_PX,
                border,
                StrokeKind::Inside,
            ),
        ]);
        ui.painter().set(background, fill);
        let painter = ui.painter_at(rect);
        painter.rect(
            well,
            theme::CONTROL_RADIUS_PX - 2.0,
            palette.viewport_stock_fill,
            border,
            StrokeKind::Inside,
        );
        let stroke = Stroke::new(1.0, palette.viewport_shape);
        for outline in &thumbnail.outlines {
            let points: Vec<egui::Pos2> = outline
                .points_px
                .iter()
                .map(|p| picture.min + egui::vec2(p[0], p[1]))
                .collect();
            painter.add(if outline.closed {
                egui::Shape::closed_line(points, stroke)
            } else {
                egui::Shape::line(points, stroke)
            });
        }
        painter.galley(
            egui::pos2(inner.left(), name_top),
            name_galley,
            palette.text_primary,
        );
        focus_ring(ui, &response);
    }
    response
}

/// A recent file: its name, then the folder it is in, cut with an
/// ellipsis; the whole path is the row's tooltip, as File ▸ Open Recent
/// shows it. The row is the button, named for the file.
fn recent_row(ui: &mut Ui, path: &Path, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let name = files::file_name(path);
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), RECENT_PX), Sense::click());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    if response.clicked() {
        out.push(Action::OpenRecent(path.to_path_buf()));
    }
    let response = response.on_hover_text(path.display().to_string());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let wash = if response.is_pointer_button_down_on() {
        palette.surface_active
    } else if response.hovered() {
        palette.surface_hover
    } else {
        theme::NONE
    };
    // The wash reaches 8 px past the text, so the names line up with the
    // headings.
    ui.painter().rect_filled(
        rect.expand2(egui::vec2(8.0, 0.0))
            .shrink2(egui::vec2(0.0, 1.0)),
        theme::CONTROL_RADIUS_PX,
        wash,
    );
    let name_galley = ui.painter().layout_no_wrap(
        name,
        TextStyle::Body.resolve(ui.style()),
        palette.text_primary,
    );
    let name_width = name_galley.size().x;
    ui.painter().galley(
        egui::pos2(rect.left(), rect.center().y - name_galley.size().y / 2.0),
        name_galley,
        palette.text_primary,
    );
    let folder = path
        .parent()
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut job = egui::text::LayoutJob::simple_singleline(
        folder,
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );
    let left = rect.left() + name_width + 12.0;
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - left).max(0.0));
    let folder_galley = ui.painter().layout_job(job);
    ui.painter().galley(
        egui::pos2(left, rect.center().y - folder_galley.size().y / 2.0),
        folder_galley,
        palette.text_secondary,
    );
    focus_ring(ui, &response);
}
