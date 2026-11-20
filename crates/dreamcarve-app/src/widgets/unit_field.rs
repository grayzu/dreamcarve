// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The text field with a trailing unit (`docs/specs/phase-4/experience.md`,
//! section 7, "Text field with trailing unit", "Tool-derived placeholder",
//! "Numeric entry", "Inline validation message"; section 12,
//! `unit_field`): `surface.sunken`, 32 px, the control radius, the value
//! left in tabular figures, the unit right-aligned in text.secondary
//! inside the outline, which is `border.default` at rest, `border.strong`
//! under the pointer and `focus.ring` while typing. The text the user
//! types lives in egui's memory while the field has focus; it commits on
//! leaving the field by any means (Tab, Enter, a click anywhere, the
//! window losing the focus, a command's key) and Escape restores the
//! value, so an edit is one undo step. The outline takes the
//! validation tier's colour; the message under the field is the caller's
//! ([`validation_line`] in a field column, [`crate::widgets::row::message`]
//! beside a label). A tool-derived field shows the tool's value as grey
//! placeholder text while empty, and a ⟲ inside its right end while it
//! holds a value of its own.
//!
//! One form for every panel: the Stock and Export steps (#536), the tool
//! editor (#537), the placement fields (#534) and the operation editor
//! (#539).
//!
//! An unset value the file needs before export (functional spec, 66b;
//! section 7, "required (unset)"; slice 26, #618) is [`UnitField::required`]:
//! the error outline, `circle-x` inside the field before the unit,
//! *Required* as the placeholder, and the file's message as the tooltip,
//! with no message line under it.

use dc_job::Tier;
use eframe::egui::{
    self, Align, Align2, Id, Key, Layout, Margin, Rect, Response, Sense, Stroke, StrokeKind,
    TextEdit, TextStyle, Ui, UiBuilder,
};

use crate::shell::theme::{self, Palette};
use crate::widgets::icon::Icon;

/// A control's height, px (section 3).
pub(crate) const HEIGHT_PX: f32 = theme::CONTROL_PX;
/// The unit's share of the field's right end, px (section 3).
const UNIT_PX: f32 = 28.0;
/// The reset button's square, px.
const RESET_PX: f32 = 24.0;
/// Inner padding, px.
const PADDING_PX: f32 = 10.0;

/// A field to draw.
#[derive(Debug, Clone, Copy)]
pub(crate) struct UnitField<'a> {
    /// The field's id, stable across frames.
    pub id: Id,
    /// Its accessible name: the row's label.
    pub name: &'a str,
    /// The project's value as text, with no unit; empty for `null`.
    pub value: &'a str,
    /// The unit drawn after the value; empty for none.
    pub unit: &'a str,
    /// Shown in text.disabled while the field is empty (`tool's: 3 mm`).
    pub placeholder: &'a str,
    /// Whether it can be typed into.
    pub enabled: bool,
    /// Why not, shown on hover when disabled.
    pub disabled_reason: Option<&'a str>,
    /// The validation state, which colours the outline.
    pub tier: Option<Tier>,
    /// The ⟲ button's tooltip, when the field shows one (a tool-derived
    /// field holding a value of its own).
    pub reset: Option<&'a str>,
    /// The required-field look (functional 66b), while the value is unset
    /// and the file needs it: the placeholder (*Required*) and the file's
    /// message, its tooltip. The caller draws no message under the field.
    pub required: Option<Required<'a>>,
}

/// An unset required value's placeholder and message.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Required<'a> {
    /// *Required*, from the string table.
    pub placeholder: &'a str,
    /// The file's message (`tools[t1].feed_mm_min is not set…`).
    pub message: &'a str,
}

impl<'a> UnitField<'a> {
    /// An enabled field `id` named `name`, showing `value` in `unit`, with
    /// no placeholder, tier or reset.
    #[must_use]
    pub(crate) const fn new(id: Id, name: &'a str, value: &'a str, unit: &'a str) -> Self {
        Self {
            id,
            name,
            value,
            unit,
            placeholder: "",
            enabled: true,
            disabled_reason: None,
            tier: None,
            reset: None,
            required: None,
        }
    }
}

/// What a field did this frame.
#[derive(Debug)]
pub(crate) struct FieldOutput {
    /// The text edit's response.
    pub response: Response,
    /// The typed text, once, when the user leaves a field they changed.
    pub committed: Option<String>,
    /// Whether ⟲ was clicked.
    pub reset: bool,
}

/// What is being typed into a field: the text, the project's value it
/// started from, and the frame it was last drawn in.
#[derive(Debug, Clone)]
struct Draft {
    typed: String,
    seed: String,
    frame_nr: u64,
}

/// Where the frame a flush was asked in is kept.
fn flush_id() -> Id {
    Id::unique("dc.unit_field.flush")
}

/// Where the frame a flush wrote a field in is kept.
fn flushed_id() -> Id {
    Id::unique("dc.unit_field.flushed")
}

/// Asks the field being typed in to write its text this frame, keeping
/// the focus: a command's key was pressed (Save, Undo, a step's key), and
/// the command acts on the project as the user sees it, with the typed
/// value in it. Called before the panels draw.
pub(crate) fn flush(ctx: &egui::Context) {
    let frame_nr = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(flush_id(), frame_nr));
}

/// Whether [`flush`] was called this frame.
fn flush_asked(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<u64>(flush_id())) == Some(ctx.cumulative_frame_nr())
}

/// Whether a field wrote its text this frame because [`flush`] asked: the
/// window then runs the field's command before the key's.
pub(crate) fn flushed(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<u64>(flushed_id())) == Some(ctx.cumulative_frame_nr())
}

/// Whether the window lost the focus this frame, or is without it.
fn window_left(ui: &Ui) -> bool {
    ui.input(|i| {
        !i.focused
            || i.events
                .iter()
                .any(|e| matches!(e, egui::Event::WindowFocused(false)))
    })
}

/// Whether a text field of another kind (a name edited in place) is left
/// this frame though the focus is still in it: the window lost the focus,
/// or a command's key asked the fields to write ([`flush`]), which is
/// then noted for the window ([`flushed`]).
pub(crate) fn left_with_focus(ui: &Ui) -> bool {
    if flush_asked(ui.ctx()) {
        let frame_nr = ui.ctx().cumulative_frame_nr();
        ui.data_mut(|d| d.insert_temp(flushed_id(), frame_nr));
        return true;
    }
    window_left(ui)
}

/// Draws `spec` across the available width.
pub(crate) fn field(ui: &mut Ui, spec: UnitField<'_>) -> FieldOutput {
    let palette = Palette::current(ui);
    let (rect, frame_response) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), HEIGHT_PX), Sense::hover());
    let buffer_id = spec.id.with("typed");
    let frame_nr = ui.ctx().cumulative_frame_nr();
    // What is typed, and the value it started from: the draft, kept from
    // the first frame the field has the focus until it is left. It is
    // read whoever has the focus now, because the focus may already have
    // gone when the field draws: a click on a field drawn before this one
    // gives that one the focus first, and Shift+Tab and an assistive tool
    // move it between frames. A draft that is not from the last frame
    // belongs to a field that went unseen, and is dropped, not written
    // late.
    let draft = ui
        .data(|d| d.get_temp::<Draft>(buffer_id))
        .filter(|draft| draft.frame_nr + 1 >= frame_nr);
    // While nothing has been typed over the value the draft started from
    // the field follows the project: a field tabbed into as its neighbour
    // commits (W, then H, which follows W) shows the new value, and
    // leaving it writes nothing back.
    let (mut text, seed) = match &draft {
        Some(draft) if draft.typed != draft.seed || draft.seed == spec.value => {
            (draft.typed.clone(), draft.seed.clone())
        }
        _ => (spec.value.to_owned(), spec.value.to_owned()),
    };

    // The frame is painted under the text, into a slot taken before it.
    let background = ui.painter().add(egui::Shape::Noop);
    // The unit's share: 32 px, or the unit's own width when wider
    // (`mm/min`, `in/min`), so nothing before it overlaps it.
    let unit_px = if spec.unit.is_empty() {
        0.0
    } else {
        let width = ui
            .painter()
            .layout_no_wrap(
                spec.unit.to_owned(),
                TextStyle::Body.resolve(ui.style()),
                palette.text_secondary,
            )
            .size()
            .x;
        UNIT_PX.max(width + 4.0)
    };
    let reset_px = if spec.reset.is_some() { RESET_PX } else { 0.0 };
    // The required look's glyph, before the unit (section 7).
    let glyph_px = if spec.required.is_some() {
        theme::ICON_SMALL_PX + 2.0
    } else {
        0.0
    };
    let tier = spec.required.map_or(spec.tier, |_| Some(Tier::Error));
    let placeholder_text = spec.required.map_or(spec.placeholder, |r| r.placeholder);
    // One line of text, centred in the control. The text edit is the
    // control's whole height, so a click anywhere in the frame lands in
    // it and assistive tools are given the field, not a line of it.
    let text_rect = Rect::from_min_max(
        egui::pos2(rect.left() + PADDING_PX, rect.top()),
        egui::pos2(
            rect.right() - PADDING_PX - unit_px - reset_px - glyph_px,
            rect.bottom(),
        ),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .max_rect(text_rect)
            .layout(Layout::left_to_right(Align::Center)),
    );
    if !spec.enabled {
        child.disable();
    }
    let placeholder = egui::RichText::new(placeholder_text.to_owned()).color(palette.text_disabled);
    let response = child
        .add(
            TextEdit::singleline(&mut text)
                .id(spec.id)
                .frame(egui::Frame::NONE)
                .margin(Margin::ZERO)
                .font(TextStyle::Body)
                .hint_text(placeholder)
                .align(Align2::LEFT_CENTER)
                .min_size(egui::vec2(0.0, HEIGHT_PX))
                .desired_width(text_rect.width()),
        )
        .accessible_name(spec.name);
    // The focus is the text's; the ring goes round the field's frame.
    crate::shell::focus::outline(ui.ctx(), spec.id, rect, theme::CONTROL_RADIUS_PX);

    // Leaving the field by any means writes what was typed (functional
    // 52): Tab, a click anywhere, another widget taking the focus, the
    // window losing it, and a command's key, which may take the field's
    // panel away with the focus still in it ([`flush`]). Escape is the
    // one way out that restores the value; the Viewport reads that key
    // before a panel draws, so the focus router is asked too.
    let escaped = ui.input(|i| i.key_pressed(Key::Escape))
        || crate::shell::focus::escape_left_widget(ui.ctx());
    let window_left = window_left(ui);
    let flushed = flush_asked(ui.ctx());
    let mut committed = None;
    if response.has_focus() && !window_left && !flushed {
        ui.data_mut(|d| {
            d.insert_temp(
                buffer_id,
                Draft {
                    typed: text.clone(),
                    seed,
                    frame_nr,
                },
            );
        });
    } else if draft.is_some() {
        ui.data_mut(|d| d.remove::<Draft>(buffer_id));
        if !escaped && text != spec.value {
            committed = Some(text);
            if flushed {
                ui.data_mut(|d| d.insert_temp(flushed_id(), frame_nr));
            }
        }
    }

    let mut reset = false;
    if let Some(tooltip) = spec.reset {
        let at = Rect::from_center_size(
            egui::pos2(
                rect.right() - PADDING_PX - unit_px - RESET_PX / 2.0,
                rect.center().y,
            ),
            egui::vec2(RESET_PX, RESET_PX),
        );
        let button = ui.interact(at, spec.id.with("reset"), Sense::click());
        button.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tooltip));
        if button.hovered() {
            ui.painter()
                .rect_filled(at, theme::pill(RESET_PX), palette.surface_hover);
        }
        let glyph = Icon::RotateCcw.galley(ui, theme::ICON_SMALL_PX, palette.text_secondary);
        ui.painter().galley(
            at.center() - glyph.size() / 2.0,
            glyph,
            palette.text_secondary,
        );
        reset = button.on_hover_text(tooltip).clicked();
    }

    if ui.is_rect_visible(rect) {
        let hovered = spec.enabled && (frame_response.hovered() || response.hovered());
        let outline = match tier {
            Some(Tier::Error) => palette.status_error.stroke,
            Some(Tier::Warning) => palette.status_warning.stroke,
            _ if !spec.enabled => palette.border_default,
            _ if response.has_focus() => palette.focus_ring,
            _ if hovered => palette.border_strong,
            _ => palette.border_default,
        };
        let painter = ui.painter();
        painter.set(
            background,
            egui::epaint::RectShape::new(
                rect,
                theme::CONTROL_RADIUS_PX,
                palette.surface_sunken,
                Stroke::new(1.0, outline),
                StrokeKind::Inside,
            ),
        );
        if !spec.unit.is_empty() {
            let colour = if spec.enabled {
                palette.text_secondary
            } else {
                palette.text_disabled
            };
            painter.text(
                egui::pos2(rect.right() - PADDING_PX, rect.center().y),
                Align2::RIGHT_CENTER,
                spec.unit,
                TextStyle::Body.resolve(ui.style()),
                colour,
            );
        }
        if spec.required.is_some() {
            let glyph = Icon::CircleX.galley(ui, theme::ICON_SMALL_PX, palette.status_error.stroke);
            let right = rect.right() - PADDING_PX - unit_px - reset_px;
            ui.painter().galley(
                egui::pos2(
                    right - glyph.size().x - 2.0,
                    rect.center().y - glyph.size().y / 2.0,
                ),
                glyph,
                palette.status_error.stroke,
            );
        }
    }
    let response = match (spec.enabled, spec.disabled_reason, spec.required) {
        (false, Some(reason), _) => {
            frame_response.on_hover_text(reason);
            response.on_disabled_hover_text(reason)
        }
        (true, _, Some(required)) => {
            frame_response.on_hover_text(required.message);
            response.on_hover_text(required.message)
        }
        _ => response,
    };
    FieldOutput {
        response,
        committed,
        reset,
    }
}

/// The 12-px message under a field in its column, in the error state:
/// `circle-x`, then the file's message verbatim, wrapped to the field.
pub fn validation_line(ui: &mut Ui, message: &str) {
    let palette = Palette::current(ui);
    ui.add_space(4.0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let glyph = Icon::CircleX.galley(ui, 12.0, palette.status_error.text);
        let (rect, _) = ui.allocate_exact_size(glyph.size(), Sense::hover());
        ui.painter()
            .galley(rect.min, glyph, palette.status_error.text);
        ui.add(
            egui::Label::new(
                egui::RichText::new(message)
                    .text_style(TextStyle::Small)
                    .color(palette.status_error.text),
            )
            .wrap()
            .selectable(false),
        );
    });
    ui.add_space(4.0);
}

/// An editor row: the label in its 112-px column, then `field` at the
/// rest of the width. The label names the field for assistive tools.
pub fn row<R>(ui: &mut Ui, label: &str, field: impl FnOnce(&mut Ui) -> R) -> (Response, R) {
    ui.horizontal_top(|ui| {
        let label_response = crate::widgets::row::label_slot(ui, label, false);
        let inner = ui.vertical(|ui| field(ui)).inner;
        (label_response, inner)
    })
    .inner
}
