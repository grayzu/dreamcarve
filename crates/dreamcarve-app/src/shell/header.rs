// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The header (`docs/specs/phase-4/redesign.md`, "The window"; functional
//! spec, capability 50): 48 px on the `surface.window` backdrop, across
//! the window. At the left the menus ([`crate::shell::menu`]) as quiet
//! text buttons; in the centre the project's name, with a dot while it
//! has unsaved changes (the OS window title carries it too); at the right
//! Undo and Redo, the generation chip while toolpaths regenerate, and the
//! view's one primary button, a 34-px accent pill. On every step but
//! Export it reads *Continue* and opens the next step that is not
//! complete ([`dc_app::steps::next`]); on Export it reads *Export G-code*
//! and opens the export sheet, disabled with the reason in its tooltip
//! while an error blocks export.

use dc_app::StrKey;
use dc_app::steps::{self, Step};
use eframe::egui::{self, Align, Id, Layout, Rect, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::menu;
use crate::shell::theme::{self, Palette};
use crate::shell::window::{Action, FrameState};
use crate::widgets::button::{ButtonSpec, Kind, button};
use crate::widgets::icon::Icon;
use crate::widgets::spinner::spinner;

/// The header's height, px.
const HEIGHT_PX: f32 = theme::HEADER_PX;
/// The header's side margin, px: the window's.
const SIDE_PX: i8 = 8;
/// The room the right-hand group takes before it has been measured, px.
const GROUP_GUESS_PX: f32 = 240.0;
/// The gap between the right-hand group's parts, px.
const GROUP_GAP_PX: f32 = 12.0;
/// The room kept clear either side of the project's name, px.
const NAME_GAP_PX: f32 = 16.0;
/// The unsaved dot, px across.
const DOT_PX: f32 = 6.0;

/// Where the right-hand group's width is kept between frames, so it can
/// be laid out left to right from its left edge: Tab then walks it in
/// reading order.
fn group_width_id() -> Id {
    Id::unique("dc.header.group_width")
}

/// Draws the header; its rectangle.
pub(crate) fn bar(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) -> Rect {
    let palette = Palette::current(ui);
    egui::Panel::top("header")
        .exact_size(HEIGHT_PX)
        .resizable(false)
        .show_separator_line(false)
        .frame(
            egui::Frame::NONE
                .fill(palette.surface_window)
                .inner_margin(egui::Margin::symmetric(SIDE_PX, 0)),
        )
        .show(ui, |ui| {
            if state.inert() {
                ui.disable();
            }
            let full = ui.max_rect();
            let width: f32 = ui
                .data(|d| d.get_temp(group_width_id()))
                .unwrap_or(GROUP_GUESS_PX);
            let group = Rect::from_min_max(egui::pos2(full.right() - width, full.top()), full.max);
            let menus = Rect::from_min_max(full.min, egui::pos2(group.left(), full.bottom()));
            let mut left = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(menus)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            let menus_end = menu::menus(&mut left, state, out);
            let mut right = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(group)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            right.spacing_mut().item_spacing.x = 4.0;
            history(&mut right, state, out);
            generation(&mut right, state);
            right.add_space(GROUP_GAP_PX - 4.0);
            primary(&mut right, state, out);
            let used = right.min_rect().width();
            if (used - width).abs() > 0.5 {
                ui.data_mut(|d| d.insert_temp(group_width_id(), used));
                ui.ctx().request_repaint();
            }
            name(
                ui,
                state,
                full,
                menus_end + NAME_GAP_PX..=group.left() - NAME_GAP_PX,
            );
            ui.allocate_rect(full, Sense::hover());
        })
        .response
        .rect
}

/// Undo and Redo: icon buttons, each naming the step it would take in
/// its tooltip.
fn history(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    let app = state.app;
    for (key, icon, named, label, enabled, action) in [
        (
            StrKey::HeaderUndo,
            Icon::Undo,
            StrKey::AppMenuUndoNamed,
            app.undo_label(),
            app.can_undo(),
            Action::Undo,
        ),
        (
            StrKey::HeaderRedo,
            Icon::Redo,
            StrKey::AppMenuRedoNamed,
            app.redo_label(),
            app.can_redo(),
            Action::Redo,
        ),
    ] {
        let name = strings.get(key);
        let tip = label.map_or_else(
            || name.to_owned(),
            |label| strings.fmt(named, &[("label", label)]),
        );
        let spec = ButtonSpec::new(Kind::Quiet, "")
            .icon(icon, theme::ICON_LARGE_PX)
            .named(name)
            .tooltip(&tip)
            .enabled(enabled);
        if button(ui, spec).clicked() {
            out.push(action);
        }
    }
}

/// The generation chip: the spinner and *Generating 2 of 5…* while
/// toolpaths regenerate, *Stopped* after Stop. It only shows; *Stop* and
/// *Regenerate all* are the status bar's (capability 71), which also
/// names the state for assistive tools.
fn generation(ui: &mut Ui, state: &FrameState<'_>) {
    let Some(cell) = &state.cells.generation else {
        return;
    };
    let palette = Palette::current(ui);
    ui.add_space(GROUP_GAP_PX - 4.0);
    let galley = ui.painter().layout_no_wrap(
        cell.text.clone(),
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );
    let slot = ui.painter().add(egui::Shape::Noop);
    let inner = ui.allocate_ui_with_layout(
        egui::vec2(0.0, theme::CHIP_PX),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.add_space(10.0);
            if cell.spinner {
                spinner(ui, "");
            }
            let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
            ui.painter()
                .galley(rect.min, galley, palette.text_secondary);
            ui.add_space(10.0);
        },
    );
    let rect = Rect::from_center_size(
        inner.response.rect.center(),
        egui::vec2(inner.response.rect.width(), theme::CHIP_PX),
    );
    ui.painter().set(
        slot,
        egui::epaint::RectShape::new(
            rect,
            theme::pill(theme::CHIP_PX),
            palette.surface_panel,
            Stroke::new(1.0, palette.border_default),
            StrokeKind::Inside,
        ),
    );
}

/// The primary button: *Continue* to the next step that is not complete,
/// or on the Export step *Export G-code*, the export command.
fn primary(ui: &mut Ui, state: &FrameState<'_>, out: &mut Vec<Action>) {
    let strings = state.app.strings();
    if state.step == Step::Export {
        // Disabled while an error blocks export, saying which: the CLI's
        // refusal (S20), as for an unset thickness (#536).
        let refusal = dc_app::project::export_refusal(state.app);
        let mut export = ButtonSpec::new(Kind::Primary, strings.get(StrKey::HeaderExport))
            .icon(Icon::Download, theme::ICON_LARGE_PX)
            .height(theme::HEADER_PRIMARY_PX)
            .enabled(refusal.is_none());
        if let Some(refusal) = refusal {
            export = export.tooltip(refusal.message.as_str());
        }
        if button(ui, export).clicked() {
            out.push(Action::OpenExport);
        }
        return;
    }
    let next = steps::next(&state.states, state.step);
    let tip = strings.fmt(
        StrKey::HeaderContinueTo,
        &[("step", strings.get(next.label()))],
    );
    let spec = ButtonSpec::new(Kind::Primary, strings.get(StrKey::HeaderContinue))
        .icon_after(Icon::ArrowRight, theme::ICON_SMALL_PX)
        .height(theme::HEADER_PRIMARY_PX)
        .tooltip(&tip);
    if button(ui, spec).clicked() {
        out.push(Action::Continue);
    }
}

/// The project's name, centred on the header when the room between the
/// menus and the right-hand group (`free`) allows, else centred in that
/// room, cut with an ellipsis; a dot before it while the project has
/// unsaved changes.
fn name(ui: &mut Ui, state: &FrameState<'_>, full: Rect, free: std::ops::RangeInclusive<f32>) {
    let palette = Palette::current(ui);
    let strings = state.app.strings();
    let dirty = state.app.is_dirty();
    let dot = if dirty { DOT_PX + 8.0 } else { 0.0 };
    let room = (free.end() - free.start() - dot).max(0.0);
    let mut job = egui::text::LayoutJob::simple_singleline(
        state.display_name.clone(),
        TextStyle::Name(theme::MEDIUM_STYLE.into()).resolve(ui.style()),
        palette.text_primary,
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let galley = ui.painter().layout_job(job);
    let width = galley.size().x + dot;
    let left = (full.center().x - width / 2.0)
        .min(free.end() - width)
        .max(*free.start());
    let rect = Rect::from_min_size(
        egui::pos2(left, full.center().y - galley.size().y / 2.0),
        egui::vec2(width, galley.size().y),
    );
    let response = ui.interact(rect, Id::unique("dc.header.name"), Sense::hover());
    let said = if dirty {
        format!(
            "{} · {}",
            state.display_name,
            strings.get(StrKey::HeaderUnsaved)
        )
    } else {
        state.display_name.clone()
    };
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &said));
    if dirty {
        ui.painter().circle_filled(
            egui::pos2(rect.left() + DOT_PX / 2.0, rect.center().y),
            DOT_PX / 2.0,
            palette.text_secondary,
        );
        let _ = response.on_hover_text(strings.get(StrKey::HeaderUnsaved));
    }
    ui.painter().galley(
        egui::pos2(rect.left() + dot, rect.top()),
        galley,
        palette.text_primary,
    );
}
