// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The empty state (functional spec, capability 79; mock-up
//! `12-empty-state.svg`; `experience.md`, section 8, "Empty states"): a
//! 640-px raised card centred on the Viewport while the project has no
//! shapes. Under its heading and one line are two paths side by side,
//! each in its own well so they cannot be read as one list: **Your
//! drawing** (Import a drawing, Import a picture, Open a project) and
//! **Examples** (the three quick starts' projects, each a card with a
//! thumbnail drawn from the example's own shapes). The card goes when a
//! shape arrives or on its close button.
//!
//! *Import a drawing* starts a new project from the chosen file, so the
//! stock is sized round the drawing. *Import a picture* waits for picture
//! import, as the toolbar's *Add picture* does.

use dc_app::StrKey;
use dc_app::onboarding::{THUMBNAIL_HEIGHT_PX, THUMBNAIL_WIDTH_PX, Thumbnail};
use eframe::egui::{self, Key, Modifiers, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, Example, FrameState, shortcut_text};
use crate::widgets::button::{ButtonSpec, Kind, button, focus_ring, text};
use crate::widgets::icon::Icon;

/// The card's width, px.
const WIDTH_PX: f32 = 640.0;
/// Its padding, px.
const PADDING_PX: f32 = 24.0;
/// The height the card takes, px, to centre it before it is laid out.
const HEIGHT_PX: f32 = 436.0;
/// The gap between the card's copy and the wells, px.
const BLOCK_GAP_PX: f32 = 32.0;
/// The gap between the two wells, px.
const WELL_GAP_PX: f32 = 16.0;
/// A well's padding, px.
const WELL_PADDING_PX: f32 = 16.0;
/// The gap under a well's heading, px.
const HEADING_GAP_PX: f32 = 12.0;
/// The gap between a well's rows, px.
const ROW_GAP_PX: f32 = 8.0;
/// The primary action's height, px.
const PRIMARY_PX: f32 = 40.0;
/// A secondary action's height, px.
const SECONDARY_PX: f32 = 32.0;
/// An example card's height, px.
const EXAMPLE_PX: f32 = 72.0;
/// The room round an example card's thumbnail, px.
const THUMBNAIL_MARGIN_PX: f32 = (EXAMPLE_PX - THUMBNAIL_HEIGHT_PX) / 2.0;
/// The height of what a well holds, px: the Examples well's heading and
/// three cards. Both wells take it, so they end level.
const WELL_CONTENT_PX: f32 = 21.0 + HEADING_GAP_PX + 3.0 * EXAMPLE_PX + 2.0 * ROW_GAP_PX;

/// Draws the card centred on `pane`.
pub(crate) fn card(ui: &mut Ui, pane: Rect, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let size = egui::vec2(WIDTH_PX.min(pane.width() - 16.0), HEIGHT_PX);
    let rect = Rect::from_center_size(pane.center(), size).intersect(pane);
    let mut card_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    let ui = &mut card_ui;
    egui::Frame::NONE
        .fill(palette.surface_raised)
        .stroke(Stroke::new(1.0, palette.border_default))
        .corner_radius(8)
        .shadow(ui.visuals().window_shadow)
        .inner_margin(PADDING_PX)
        .show(ui, |ui| {
            ui.set_width(rect.width() - 2.0 * PADDING_PX);
            ui.horizontal(|ui| {
                text(
                    ui,
                    strings.get(StrKey::AppEmptyHeading),
                    TextStyle::Name(theme::DISPLAY.into()),
                    palette.text_primary,
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                    let close = ButtonSpec::new(Kind::Quiet, "")
                        .icon(Icon::X, theme::ICON_SMALL_PX)
                        .tooltip(strings.get(StrKey::AppEmptyClose));
                    if button(ui, close).clicked() {
                        out.push(Action::DismissEmpty);
                    }
                });
            });
            ui.add(
                egui::Label::new(
                    egui::RichText::new(strings.get(StrKey::AppEmptyIntro))
                        .color(palette.text_secondary),
                )
                .wrap()
                .selectable(false),
            );
            ui.add_space(BLOCK_GAP_PX);
            let well_width = (ui.available_width() - WELL_GAP_PX) / 2.0;
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = WELL_GAP_PX;
                well(ui, palette, well_width, |ui| your_drawing(ui, state, out));
                well(ui, palette, well_width, |ui| examples(ui, state, out));
            });
        });
}

/// One of the card's two paths: a `surface.panel` well `width` wide.
fn well(ui: &mut Ui, palette: &Palette, width: f32, contents: impl FnOnce(&mut Ui)) {
    egui::Frame::NONE
        .fill(palette.surface_panel)
        .corner_radius(6)
        .inner_margin(WELL_PADDING_PX)
        .show(ui, |ui| {
            ui.set_width(width - 2.0 * WELL_PADDING_PX);
            ui.set_min_height(WELL_CONTENT_PX);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                contents(ui);
            });
        });
}

/// A well's heading.
fn heading(ui: &mut Ui, label: &str) {
    let palette = Palette::current(ui);
    text(ui, label, TextStyle::Heading, palette.text_primary);
    ui.add_space(HEADING_GAP_PX);
}

/// The first path: bring your own drawing, or open a project.
fn your_drawing(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    heading(ui, strings.get(StrKey::AppEmptyYourDrawing));
    let width = ui.available_width();
    let import = ButtonSpec::new(Kind::Primary, strings.get(StrKey::AppEmptyImportDrawing))
        .icon(Icon::FileInput, theme::ICON_SMALL_PX)
        .height(PRIMARY_PX)
        .width(width)
        .leading();
    if button(ui, import).clicked() {
        // On a project with no shapes the drawing starts the project:
        // the stock is sized round it (capability 79).
        out.push(Action::NewFromDrawing);
    }
    ui.add_space(6.0);
    text(
        ui,
        strings.get(StrKey::AppEmptyImportDrawingHint),
        TextStyle::Small,
        palette.text_secondary,
    );
    ui.add_space(HEADING_GAP_PX);
    let secondary = |label: StrKey, icon: Icon| {
        ButtonSpec::new(Kind::Secondary, strings.get(label))
            .icon(icon, theme::ICON_SMALL_PX)
            .height(SECONDARY_PX)
            .width(width)
    };
    button(
        ui,
        secondary(StrKey::AppEmptyImportPicture, Icon::ImagePlus)
            .tooltip(strings.get(StrKey::AppEmptyPictureUnavailable))
            .enabled(false),
    );
    ui.add_space(ROW_GAP_PX);
    let shortcut = shortcut_text(ui.ctx(), Modifiers::COMMAND, Key::O);
    if button(
        ui,
        secondary(StrKey::AppEmptyOpenProject, Icon::FolderOpen).trailing(&shortcut),
    )
    .clicked()
    {
        out.push(Action::Open);
    }
}

/// The second path: start from a built-in example.
fn examples(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    heading(ui, strings.get(StrKey::AppEmptyExamples));
    for (index, (example, thumbnail)) in state.examples.iter().enumerate() {
        if index > 0 {
            ui.add_space(ROW_GAP_PX);
        }
        let name = strings.get(example.name());
        let teaches = strings.get(example.teaches());
        if example_card(ui, *example, thumbnail, name, teaches).clicked() {
            out.push(Action::OpenExample(*example));
        }
    }
}

/// An example card (`experience.md`, section 7, "Example card"): the
/// thumbnail, then `name` over the one line `teaches`. The whole card is
/// the button, named `name`.
fn example_card(
    ui: &mut Ui,
    example: Example,
    thumbnail: &Thumbnail,
    name: &str,
    teaches: &str,
) -> egui::Response {
    let palette = Palette::current(ui);
    let (_, rect) = ui.allocate_space(egui::vec2(ui.available_width(), EXAMPLE_PX));
    // The card's fill goes under its text, and its hover wash is known
    // only once the card, the topmost widget here, has been added.
    let background = ui.painter().add(egui::Shape::Noop);

    let picture = Rect::from_min_size(
        rect.min + egui::Vec2::splat(THUMBNAIL_MARGIN_PX),
        egui::vec2(THUMBNAIL_WIDTH_PX, THUMBNAIL_HEIGHT_PX),
    );
    let text_left = picture.right() + 12.0;
    let name_galley = ui.painter().layout_no_wrap(
        name.to_owned(),
        TextStyle::Name(theme::MEDIUM_STYLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    let line_height = ui.text_style_height(&TextStyle::Small);
    let line_gap = 2.0;
    let top = rect.center().y - (name_galley.size().y + line_gap + line_height) / 2.0;
    let line = Rect::from_min_max(
        egui::pos2(text_left, top + name_galley.size().y + line_gap),
        egui::pos2(
            rect.right() - THUMBNAIL_MARGIN_PX,
            top + name_galley.size().y + line_gap + line_height,
        ),
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
            egui::Shape::rect_filled(rect, 6.0, palette.surface_raised),
            egui::Shape::rect_filled(rect, 6.0, wash),
            egui::Shape::rect_stroke(rect, 6.0, border, StrokeKind::Inside),
        ]);
        ui.painter().set(background, fill);
        let painter = ui.painter_at(rect);
        painter.rect(
            picture,
            3.0,
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
            egui::pos2(text_left, top),
            name_galley,
            palette.text_primary,
        );
        focus_ring(ui, &response);
    }
    response
}
