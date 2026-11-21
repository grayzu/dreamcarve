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
//! value, so an edit is one undo step. A tool-derived field shows the
//! tool's value as grey placeholder text while empty, and a ⟲ inside its
//! right end while it holds a value of its own.
//!
//! What is wrong at the field ([`UnitField::problems`]: the findings
//! located at it, the reason it refused what was typed) takes no room in
//! the layout (section 7, "Field problem"): the outline takes the worst
//! tier's stroke and that tier's glyph stands inside the field before the
//! unit ([`crate::widgets::problem`]). Hovering the glyph or the field
//! shows every problem, the field's accessible description says the
//! same, the panel's help line shows them while the field has the
//! keyboard, and a click on the glyph opens the Warnings drawer at the
//! finding.
//!
//! One form for every panel: the Stock and Export steps (#536), the tool
//! editor (#537), the placement fields (#534) and the operation editor
//! (#539).
//!
//! An unset value the file needs before export (functional spec, 66b;
//! section 7, "required (unset)"; slice 26, #618) is [`UnitField::required`]:
//! *Required* as the placeholder, with the file's message among the
//! field's problems, so the error outline and `circle-x` inside the field
//! before the unit, as for any other problem.

use dc_app::problems::Problems;
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
/// What is kept in view under a field as it takes the
/// keyboard, px: a line of note under it (a stepover's distance between
/// passes, which is not a problem and stays in the layout).
const REVEAL_PX: f32 = 24.0;

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
    /// What is wrong at the field, most severe first: the outline and
    /// the glyph inside the field are the worst's, and the rest is in
    /// the tooltip, the accessible description and the help line.
    pub problems: Option<&'a Problems>,
    /// The ⟲ button's tooltip, when the field shows one (a tool-derived
    /// field holding a value of its own).
    pub reset: Option<&'a str>,
    /// The required-field look (functional 66b), while the value is unset
    /// and the file needs it: the placeholder, *Required* from the string
    /// table. The file's message (`tools[t1].feed_mm_min is not set…`) is
    /// one of [`UnitField::problems`].
    pub required: Option<&'a str>,
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
            problems: None,
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
    /// What the field shows this frame: the project's value, or what is
    /// being typed over it. A note that follows the typing reads it (a
    /// stepover's distance between passes).
    pub text: String,
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
    // The help the field's label offered: its tooltip here, and the
    // panel's help line while it is typed in.
    let help = crate::widgets::help::take(ui, spec.name);
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
    // The problem's glyph, before the unit (section 7).
    let problems = spec.problems.filter(|problems| !problems.is_empty());
    let glyph_px = if problems.is_some() {
        crate::widgets::problem::GLYPH_PX + 2.0
    } else {
        0.0
    };
    let tier = problems.and_then(Problems::worst);
    let placeholder_text = spec.required.unwrap_or(spec.placeholder);
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
    let shown = text.clone();
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
            Some(tier) => crate::widgets::problem::stroke(palette, tier),
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
    }
    // The glyph of the worst problem, inside the field before the unit:
    // its tooltip lists them, and its click opens the drawer at the
    // finding.
    if let Some(problems) = problems {
        let right = rect.right() - PADDING_PX - unit_px - reset_px;
        let centre = egui::pos2(
            right - 2.0 - crate::widgets::problem::GLYPH_PX / 2.0,
            rect.center().y,
        );
        let _ = crate::widgets::problem::glyph(ui, spec.id.with("problem"), centre, problems);
    }
    // The help line takes its room at the panel's foot in the frame
    // after the field takes the keyboard: the field, and a line of note
    // under it, are then brought clear of it.
    let reveal_id = spec.id.with("reveal");
    if ui.data(|d| d.get_temp::<bool>(reveal_id)).is_some() {
        ui.data_mut(|d| d.remove::<bool>(reveal_id));
        // Only when the line would hide it: a field in view stays put.
        let kept = rect.with_max_y(rect.bottom() + REVEAL_PX);
        if kept.bottom() > ui.clip_rect().bottom() || kept.top() < ui.clip_rect().top() {
            ui.scroll_to_rect(kept, None);
        }
    }
    if ui.memory(|m| m.has_focus(spec.id)) {
        // The problem first, then the help: both on the panel's help
        // line, which is where a keyboard user reads what a hover shows.
        let wrong = problems
            .is_some_and(|problems| crate::widgets::help::problems(ui, spec.name, problems));
        let helped = help
            .as_ref()
            .is_some_and(|help| crate::widgets::help::focused(ui, spec.name, help));
        if wrong || helped {
            ui.data_mut(|d| d.insert_temp(reveal_id, true));
        }
    }
    // The tooltips: what is wrong, then why the field is disabled or what
    // it is, each a tooltip of its own; the same words describe the field
    // for assistive tools.
    let none = Problems::none();
    let problems = problems.unwrap_or(&none);
    let said = match (spec.enabled, spec.disabled_reason) {
        (false, Some(reason)) => Some(reason),
        (false, None) => None,
        (true, _) => help.as_deref(),
    };
    let _ = crate::widgets::problem::hover(frame_response, problems, said);
    let response = if spec.enabled {
        crate::widgets::problem::hover(response, problems, said)
    } else {
        let response = if problems.is_empty() {
            response
        } else {
            response.on_disabled_hover_ui(|ui| crate::widgets::problem::tooltip(ui, problems))
        };
        match said {
            Some(reason) => response.on_disabled_hover_text(reason),
            None => response,
        }
    };
    crate::widgets::problem::set_description(ui, spec.id, problems, said);
    FieldOutput {
        response,
        committed,
        text: shown,
        reset,
    }
}

/// A quiet 12-px note under a field in its column, in text.secondary with
/// no glyph: what the value comes to (a stepover's distance between
/// passes). It is not a problem, and does not look like one: a problem
/// takes no line (section 7, "Field problem").
pub fn quiet_line(ui: &mut Ui, text: &str) {
    let palette = Palette::current(ui);
    ui.add_space(2.0);
    ui.add(
        egui::Label::new(
            egui::RichText::new(text)
                .text_style(TextStyle::Small)
                .color(palette.text_secondary),
        )
        .wrap()
        .selectable(false),
    );
    ui.add_space(2.0);
}

/// An editor row: the label in its 112-px column, then `field` at the
/// rest of the width. The label names the field for assistive tools, and
/// shows `help` on hover; the field drawn under the same name takes it.
pub fn row<R>(
    ui: &mut Ui,
    label: &str,
    help: &str,
    field: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    ui.horizontal_top(|ui| {
        let label_response = crate::widgets::row::label(ui, label, help, None);
        let inner = ui.vertical(|ui| field(ui)).inner;
        (label_response, inner)
    })
    .inner
}
