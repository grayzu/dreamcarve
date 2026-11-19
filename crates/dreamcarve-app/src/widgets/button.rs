// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The app's buttons (`docs/specs/phase-4/redesign.md`, "Components";
//! `docs/specs/phase-4/experience.md`, section 7, "Buttons"): primary (an
//! accent pill, once per view), secondary (raised, outlined, the control
//! radius), quiet (text and icon, a wash on hover) and destructive (a
//! secondary button in the error colour), with an optional Lucide icon and
//! trailing text. Painted here rather than with egui's `Button` so the
//! accessible name is the label alone (not the icon's private-use code
//! point), the heights are the spec's, and the focus ring is drawn.

use eframe::egui::{self, Color32, Response, Sense, Stroke, StrokeKind, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::widgets::icon::Icon;

/// Which of the spec's button styles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Text and icon only; `surface.hover` behind it on hover.
    Quiet,
    /// `surface.raised` with a `border.default` outline, the control
    /// radius.
    Secondary,
    /// An `accent.fill` pill with `text.on-accent`; `accent.hover` under
    /// the pointer. One per view.
    Primary,
    /// A secondary button whose label and icon take `status.error`.
    Destructive,
}

impl Kind {
    /// The space left and right of the content, px: a pill needs more
    /// than a rounded rectangle, and a quiet button has no edge to clear.
    const fn padding_px(self) -> f32 {
        match self {
            Self::Quiet => 8.0,
            Self::Secondary | Self::Destructive => 12.0,
            Self::Primary => 16.0,
        }
    }

    /// The corner radius at `height_px`: a pill for the primary button,
    /// else the control radius.
    const fn radius_px(self, height_px: f32) -> f32 {
        match self {
            Self::Primary => theme::pill(height_px),
            Self::Quiet | Self::Secondary | Self::Destructive => theme::CONTROL_RADIUS_PX,
        }
    }
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
    /// Shown on hover; for an icon-only button, also its accessible name
    /// unless `name` gives one.
    pub tooltip: Option<&'a str>,
    /// The accessible name of an icon-only button whose tooltip says
    /// something else (a disabled button's reason).
    pub name: Option<&'a str>,
    /// Text at the right edge in text.secondary, such as the action's
    /// shortcut; not part of the accessible name.
    pub trailing: Option<&'a str>,
    /// Whether the content starts at the left edge whatever the style: a
    /// full-width button in a column of left-aligned ones.
    pub leading: bool,
    /// Height, px: 32 for a control, 34 for the toolbar's primary button,
    /// 40 for the empty state's.
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
    /// A 32-px button of `kind` reading `label`.
    #[must_use]
    pub const fn new(kind: Kind, label: &'a str) -> Self {
        Self {
            kind,
            icon: None,
            icon_px: theme::ICON_SMALL_PX,
            label,
            tooltip: None,
            name: None,
            trailing: None,
            leading: false,
            height_px: theme::CONTROL_PX,
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

    /// With a tooltip.
    #[must_use]
    pub const fn tooltip(mut self, tooltip: &'a str) -> Self {
        self.tooltip = Some(tooltip);
        self
    }

    /// Named `name` for assistive tools, whatever the tooltip says.
    #[must_use]
    pub const fn named(mut self, name: &'a str) -> Self {
        self.name = Some(name);
        self
    }

    /// With `trailing` at the right edge.
    #[must_use]
    pub const fn trailing(mut self, trailing: &'a str) -> Self {
        self.trailing = Some(trailing);
        self
    }

    /// With the content starting at the left edge.
    #[must_use]
    pub const fn leading(mut self) -> Self {
        self.leading = true;
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

/// Fill, the wash over it, outline and text colours of `kind` in `state`.
fn colours(
    palette: &Palette,
    kind: Kind,
    response: &Response,
    enabled: bool,
    held: bool,
) -> (Color32, Color32, Stroke, Color32) {
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
            (theme::NONE, wash, Stroke::NONE, text)
        }
        Kind::Secondary | Kind::Destructive => {
            let text = match (enabled, kind) {
                (false, _) => palette.text_disabled,
                (true, Kind::Destructive) => palette.status_error.text,
                (true, _) => palette.text_primary,
            };
            let stroke = match (hovered || pressed, kind) {
                (true, Kind::Destructive) => palette.status_error.stroke,
                (true, _) => palette.border_strong,
                (false, _) => palette.border_default,
            };
            (palette.surface_raised, wash, Stroke::new(1.0, stroke), text)
        }
        Kind::Primary => {
            if !enabled {
                (
                    palette.surface_sunken,
                    theme::NONE,
                    Stroke::new(1.0, palette.border_default),
                    palette.text_disabled,
                )
            } else if hovered && !pressed {
                (
                    palette.accent_hover,
                    theme::NONE,
                    Stroke::NONE,
                    palette.text_on_accent,
                )
            } else {
                (
                    palette.accent_fill,
                    theme::NONE,
                    Stroke::NONE,
                    palette.text_on_accent,
                )
            }
        }
    }
}

/// Draws `spec`; the response's `clicked` is the press.
pub fn button(ui: &mut Ui, spec: ButtonSpec<'_>) -> Response {
    let palette = Palette::current(ui);
    let gap = theme::GAP_PX;
    let body = TextStyle::Body.resolve(ui.style());
    let small = TextStyle::Small.resolve(ui.style());
    let painter = ui.painter().clone();
    let label = (!spec.label.is_empty())
        .then(|| painter.layout_no_wrap(spec.label.to_owned(), body.clone(), theme::PAINT_TIME));
    let icon_w = spec.icon.map_or(0.0, |_| spec.icon_px);
    let text_w = label.as_ref().map_or(0.0, |g| g.size().x);
    let inner = icon_w
        + if icon_w > 0.0 && text_w > 0.0 {
            gap
        } else {
            0.0
        }
        + text_w;
    // An icon alone sits in a square, whatever the style.
    let padding = if label.is_some() {
        spec.kind.padding_px()
    } else {
        0.0
    };
    let width = spec
        .width_px
        .unwrap_or_else(|| (inner + 2.0 * padding).max(spec.height_px));
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(width, spec.height_px),
        if spec.enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let name = match spec.name {
        Some(name) => name,
        None if spec.label.is_empty() => spec.tooltip.unwrap_or_default(),
        None => spec.label,
    };
    let enabled = spec.enabled;
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, name));

    if ui.is_rect_visible(rect) {
        let (fill, wash, stroke, text) =
            colours(palette, spec.kind, &response, enabled, spec.pressed);
        let radius = spec.kind.radius_px(rect.height());
        let painter = ui.painter_at(rect.expand(4.0));
        painter.rect(rect, radius, fill, stroke, StrokeKind::Inside);
        if wash != theme::NONE {
            painter.rect_filled(rect, radius, wash);
        }
        let padding = spec.kind.padding_px();
        let mut x = if spec.leading {
            rect.left() + padding
        } else {
            rect.center().x - inner / 2.0
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
        if let Some(label) = label {
            let y = rect.center().y - label.size().y / 2.0;
            painter.galley(egui::pos2(x, y), label, text);
        }
        if let Some(trailing) = spec.trailing {
            let colour = if enabled {
                palette.text_secondary
            } else {
                palette.text_disabled
            };
            let galley = painter.layout_no_wrap(trailing.to_owned(), small, theme::PAINT_TIME);
            let at = egui::pos2(
                rect.right() - spec.kind.padding_px() - galley.size().x,
                rect.center().y - galley.size().y / 2.0,
            );
            painter.galley(at, galley, colour);
        }
        focus_ring_at(ui, &response, radius);
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
    focus_ring_at(ui, response, theme::CONTROL_RADIUS_PX);
}

/// [`focus_ring`] round a control whose corner radius is `radius_px`: a
/// pill's ring is a pill.
pub fn focus_ring_at(ui: &Ui, response: &Response, radius_px: f32) {
    if response.has_focus() {
        let palette = Palette::current(ui);
        ui.painter().rect_stroke(
            response.rect.expand(3.0),
            radius_px + 3.0,
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
