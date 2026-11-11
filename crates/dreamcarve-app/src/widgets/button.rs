// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The app's buttons (`docs/specs/phase-4/experience.md`, section 7,
//! "Buttons"): quiet (toolbar, row actions), secondary (sheets, the empty
//! state) and primary (once per surface), with an optional Lucide icon and
//! a second line. Painted here rather than with egui's `Button` so the
//! accessible name is the label alone (not the icon's private-use code
//! point), the heights are the spec's, and the focus ring is drawn.

use eframe::egui::{self, Color32, Response, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::widgets::icon::Icon;

/// Which of the spec's button styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// No outline; a wash on hover.
    Quiet,
    /// Raised fill, strong outline.
    Secondary,
    /// The accent fill, white text.
    Primary,
    /// Secondary at rest; error-red text and outline on hover only.
    Destructive,
}

/// A button to draw.
#[derive(Debug, Clone, Copy)]
pub struct ButtonSpec<'a> {
    /// The style.
    pub kind: Kind,
    /// The icon left of the label.
    pub icon: Option<Icon>,
    /// The icon's size, px.
    pub icon_px: f32,
    /// The label, and the accessible name. Empty for an icon-only button,
    /// whose name is `tooltip`.
    pub label: &'a str,
    /// A second line under the label, 12 px.
    pub hint: Option<&'a str>,
    /// Shown on hover; for an icon-only button, also its accessible name.
    pub tooltip: Option<&'a str>,
    /// Height, px: 26 for a control, 28 in the toolbar and sheet footers,
    /// 40 for the empty state's rows.
    pub height_px: f32,
    /// Width, px; `None` fits the content.
    pub width_px: Option<f32>,
    /// Whether it can be pressed.
    pub enabled: bool,
    /// Whether it shows pressed, as a button does while its popover is
    /// open.
    pub pressed: bool,
}

impl<'a> ButtonSpec<'a> {
    /// A 26-px button of `kind` reading `label`.
    #[must_use]
    pub const fn new(kind: Kind, label: &'a str) -> Self {
        Self {
            kind,
            icon: None,
            icon_px: theme::ICON_SMALL_PX,
            label,
            hint: None,
            tooltip: None,
            height_px: 26.0,
            width_px: None,
            enabled: true,
            pressed: false,
        }
    }

    /// With `icon` at `px`.
    #[must_use]
    pub const fn icon(mut self, icon: Icon, px: f32) -> Self {
        self.icon = Some(icon);
        self.icon_px = px;
        self
    }

    /// With a second line.
    #[must_use]
    pub const fn hint(mut self, hint: &'a str) -> Self {
        self.hint = Some(hint);
        self
    }

    /// With a tooltip.
    #[must_use]
    pub const fn tooltip(mut self, tooltip: &'a str) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    /// At `px` tall.
    #[must_use]
    pub const fn height(mut self, px: f32) -> Self {
        self.height_px = px;
        self
    }

    /// At `px` wide.
    #[must_use]
    pub const fn width(mut self, px: f32) -> Self {
        self.width_px = Some(px);
        self
    }

    /// Shown pressed or not.
    #[must_use]
    pub const fn pressed(mut self, pressed: bool) -> Self {
        self.pressed = pressed;
        self
    }

    /// Enabled or not.
    #[must_use]
    pub const fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// Fill, outline and text colours of `kind` in `state`.
fn colours(
    palette: &Palette,
    kind: Kind,
    response: &Response,
    enabled: bool,
    held: bool,
) -> (Color32, Stroke, Color32) {
    let hovered = enabled && response.hovered();
    let pressed = enabled && (held || response.is_pointer_button_down_on());
    let wash = if pressed {
        palette.surface_active
    } else if hovered {
        palette.surface_hover
    } else {
        theme::NONE
    };
    match kind {
        Kind::Quiet => {
            let text = if enabled {
                palette.text_primary
            } else {
                palette.text_disabled
            };
            (wash, Stroke::NONE, text)
        }
        Kind::Secondary => {
            let (stroke, text) = if enabled {
                (palette.border_strong, palette.text_primary)
            } else {
                (palette.border_default, palette.text_disabled)
            };
            (palette.surface_raised, Stroke::new(1.0, stroke), text)
        }
        Kind::Destructive => {
            let (stroke, text) = if !enabled {
                (palette.border_default, palette.text_disabled)
            } else if hovered {
                (palette.status_error.stroke, palette.status_error.text)
            } else {
                (palette.border_strong, palette.text_primary)
            };
            (palette.surface_raised, Stroke::new(1.0, stroke), text)
        }
        Kind::Primary => {
            if enabled {
                (palette.accent_fill, Stroke::NONE, palette.text_on_accent)
            } else {
                (
                    palette.surface_sunken,
                    Stroke::new(1.0, palette.border_default),
                    palette.text_disabled,
                )
            }
        }
    }
}

/// Draws `spec`; the response's `clicked` is the press.
pub fn button(ui: &mut Ui, spec: ButtonSpec<'_>) -> Response {
    let palette = Palette::current(ui);
    let padding = 8.0;
    let gap = 8.0;
    let body = TextStyle::Body.resolve(ui.style());
    let small = TextStyle::Small.resolve(ui.style());
    let painter = ui.painter().clone();
    let label = (!spec.label.is_empty())
        .then(|| painter.layout_no_wrap(spec.label.to_owned(), body.clone(), theme::PAINT_TIME));
    let hint = spec
        .hint
        .map(|h| painter.layout_no_wrap(h.to_owned(), small.clone(), theme::PAINT_TIME));
    let icon_w = spec.icon.map_or(0.0, |_| spec.icon_px);
    let text_w = label
        .as_ref()
        .map_or(0.0, |g| g.size().x)
        .max(hint.as_ref().map_or(0.0, |g| g.size().x));
    let inner = icon_w
        + if icon_w > 0.0 && text_w > 0.0 {
            gap
        } else {
            0.0
        }
        + text_w;
    let width = spec
        .width_px
        .unwrap_or(inner + 2.0 * padding)
        .max(spec.height_px);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, spec.height_px),
        if spec.enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let name = if spec.label.is_empty() {
        spec.tooltip.unwrap_or_default()
    } else {
        spec.label
    };
    let enabled = spec.enabled;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));

    if ui.is_rect_visible(rect) {
        let (fill, stroke, text) = colours(palette, spec.kind, &response, enabled, spec.pressed);
        let painter = ui.painter_at(rect.expand(4.0));
        painter.rect(rect, 4.0, fill, stroke, StrokeKind::Inside);
        let mut x = match spec.width_px {
            Some(_) if spec.hint.is_none() && spec.kind != Kind::Secondary => {
                rect.center().x - inner / 2.0
            }
            _ => rect.left() + padding,
        };
        if let Some(icon) = spec.icon {
            let glyph = icon.galley(ui, spec.icon_px, text);
            let at = egui::pos2(
                x + (spec.icon_px - glyph.size().x) / 2.0,
                rect.center().y - glyph.size().y / 2.0,
            );
            painter.galley(at, glyph, text);
            x += spec.icon_px + gap;
        }
        let line_gap = 2.0;
        let total_h = label.as_ref().map_or(0.0, |g| g.size().y)
            + hint.as_ref().map_or(0.0, |g| g.size().y + line_gap);
        let mut y = rect.center().y - total_h / 2.0;
        if let Some(label) = label {
            let h = label.size().y;
            painter.galley(egui::pos2(x, y), label, text);
            y += h + line_gap;
        }
        if let Some(hint) = hint {
            let colour = if enabled {
                palette.text_secondary
            } else {
                palette.text_disabled
            };
            painter.galley(egui::pos2(x, y), hint, colour);
        }
        focus_ring(ui, &response);
    }
    match spec.tooltip {
        Some(tip) if enabled => response.on_hover_text(tip),
        Some(tip) => response.on_disabled_hover_text(tip),
        None => response,
    }
}

/// The keyboard focus ring (section 7, "focus"; section 12, "Focus ring"):
/// 2 px of `focus.ring` outside the control, 2 px off it. egui gives a
/// clickable widget focus only from the keyboard, so `has_focus` is the
/// keyboard case.
pub fn focus_ring(ui: &Ui, response: &Response) {
    if response.has_focus() {
        let palette = Palette::current(ui);
        ui.painter().rect_stroke(
            response.rect.expand(3.0),
            6.0,
            Stroke::new(2.0, palette.focus_ring),
            StrokeKind::Middle,
        );
    }
}

/// A label in `colour` at text style `style`, never wrapping: an
/// ellipsis when it does not fit (section 12, "Text truncation").
pub fn text(ui: &mut Ui, text: &str, style: TextStyle, colour: Color32) -> Response {
    ui.add(
        egui::Label::new(egui::RichText::new(text).text_style(style).color(colour))
            .truncate()
            .selectable(false),
    )
}
