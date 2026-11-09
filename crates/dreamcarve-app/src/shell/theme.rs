// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The design tokens and their egui realisation
//! (`docs/specs/phase-4/experience.md`, sections 3, 4, 5 and 12): the
//! light and dark palettes, the `Visuals` and `Style` built from them, the
//! bundled fonts, and [`apply`], which installs both themes and follows
//! the OS appearance unless the preference overrides it.
//!
//! The one place a colour is written: every widget takes its colours from
//! a [`Palette`] (section 13, item 1; `tests/theme.rs` scans for literals
//! elsewhere). The operation colours and the depth ramp (slice 16) are
//! here too: the light operation set is `dc_job::preview::PALETTE`, the
//! CLI's preview colours, read at compile time, so the light Viewport and
//! `--preview` share one table (design open question 5); the dark set is
//! the app's alone. The Viewport's renderer takes its line colours from
//! here, as a style table (`viewport::renderer`).

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Shadow,
    Stroke, Style, TextStyle, Theme, ThemePreference, Visuals,
    style::{ScrollStyle, Selection, TextCursorStyle, WidgetVisuals, Widgets},
};

/// An opaque `#RRGGBB`.
const fn rgb(hex: u32) -> Color32 {
    let [_, r, g, b] = hex.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

/// `#RRGGBBAA`, alpha not premultiplied, as the spec writes it.
const fn rgba(hex: u32) -> Color32 {
    let [r, g, b, a] = hex.to_be_bytes();
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

/// One hex digit's value; anything else is 0.
const fn hex_digit(digit: u8) -> u8 {
    match digit {
        b'0'..=b'9' => digit - b'0',
        b'a'..=b'f' => digit - b'a' + 10,
        b'A'..=b'F' => digit - b'A' + 10,
        _ => 0,
    }
}

/// `#rrggbb` as written in `dc_job::preview::PALETTE`, opaque; anything
/// else is transparent, which `tests/theme.rs` would catch.
const fn hex(text: &str) -> Color32 {
    match *text.as_bytes() {
        [b'#', r1, r0, g1, g0, b1, b0] => Color32::from_rgb(
            hex_digit(r1) * 16 + hex_digit(r0),
            hex_digit(g1) * 16 + hex_digit(g0),
            hex_digit(b1) * 16 + hex_digit(b0),
        ),
        _ => Color32::TRANSPARENT,
    }
}

/// The light operation set: the preview's palette, one table.
const fn preview_palette() -> [Color32; 8] {
    let p = dc_job::preview::PALETTE;
    [
        hex(p[0]),
        hex(p[1]),
        hex(p[2]),
        hex(p[3]),
        hex(p[4]),
        hex(p[5]),
        hex(p[6]),
        hex(p[7]),
    ]
}

/// No colour: an unwashed button, a cleared fill.
pub const NONE: Color32 = Color32::TRANSPARENT;
/// Text laid out now and coloured when painted.
pub const PAINT_TIME: Color32 = Color32::PLACEHOLDER;

/// A status colour (section 5, "Status"): text that reads at 4.5:1 on a
/// panel, the stroke for glyphs and field outlines, and the tint for row
/// and field backgrounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusColours {
    /// Message text.
    pub text: Color32,
    /// Glyphs and outlines.
    pub stroke: Color32,
    /// Row and field backgrounds; transparent where the spec has none.
    pub tint: Color32,
}

/// One appearance's tokens (section 5), named as the spec names them with
/// the dots as underscores.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    /// Whether this is the dark appearance.
    pub dark: bool,
    /// The window behind the regions: the gutters and dividers.
    pub surface_window: Color32,
    /// Side panels, the toolbar and the status bar.
    pub surface_panel: Color32,
    /// Popovers, menus, cards and sheets.
    pub surface_raised: Color32,
    /// Fields and lists.
    pub surface_sunken: Color32,
    /// Alternate table rows.
    pub surface_stripe: Color32,
    /// The hover wash.
    pub surface_hover: Color32,
    /// The pressed wash.
    pub surface_active: Color32,
    /// Rules and quiet outlines.
    pub border_default: Color32,
    /// Field and secondary-button outlines.
    pub border_strong: Color32,
    /// Body text.
    pub text_primary: Color32,
    /// Labels, units, the status bar.
    pub text_secondary: Color32,
    /// Disabled text and placeholders.
    pub text_disabled: Color32,
    /// Text on an accent fill.
    pub text_on_accent: Color32,
    /// Selection, links, the focus colour.
    pub accent: Color32,
    /// The primary button's fill.
    pub accent_fill: Color32,
    /// Selected rows and text selection.
    pub accent_soft: Color32,
    /// The keyboard focus ring.
    pub focus_ring: Color32,
    /// Export refuses.
    pub status_error: StatusColours,
    /// Safe, but the user should know.
    pub status_warning: StatusColours,
    /// A hint.
    pub status_info: StatusColours,
    /// Done.
    pub status_success: StatusColours,
    /// The Viewport's background, the stage.
    pub viewport_bg: Color32,
    /// The grid's minor lines.
    pub viewport_grid_minor: Color32,
    /// The grid's major lines.
    pub viewport_grid_major: Color32,
    /// The origin crosshair and the axis lines through it.
    pub viewport_axis: Color32,
    /// The stock's dashed outline.
    pub viewport_stock: Color32,
    /// The stock's fill, a shade off the background.
    pub viewport_stock_fill: Color32,
    /// The male inlay board: its outline, origin mark and mirrored shapes.
    pub viewport_male_board: Color32,
    /// A shape's outline, solid when closed, dashed when open.
    pub viewport_shape: Color32,
    /// The outline under the pointer.
    pub viewport_shape_hover: Color32,
    /// A selected outline.
    pub viewport_shape_selected: Color32,
    /// A locked outline and its padlock.
    pub viewport_shape_locked: Color32,
    /// The park point's crossed circle.
    pub viewport_park: Color32,
    /// Rapids, thin and dashed.
    pub viewport_rapid: Color32,
    /// The mark where the tool feeds straight down.
    pub viewport_plunge: Color32,
    /// Tab marks, heavy.
    pub viewport_tab: Color32,
    /// The program extents' tool-centre box, solid.
    pub viewport_extents_centre: Color32,
    /// The program extents' bit-edge box, dashed, outside it.
    pub viewport_extents_edge: Color32,
    /// The pill behind a caption on the Viewport.
    pub viewport_label_bg: Color32,
    /// The depth ramp's five stops, stock top first (section 5, "Depth
    /// ramp"), interpolated in linear sRGB.
    pub depth_ramp: [Color32; 5],
    /// The scrim under a sheet (section 8, "Sheets").
    pub scrim: Color32,
    /// Raised surfaces' shadow (section 12, "Visuals").
    pub shadow: Color32,
}

/// The light appearance.
pub const LIGHT: Palette = Palette {
    dark: false,
    surface_window: rgb(0xECEEF1),
    surface_panel: rgb(0xF7F8FA),
    surface_raised: rgb(0xFFFFFF),
    surface_sunken: rgb(0xFFFFFF),
    surface_stripe: rgb(0xF2F3F6),
    surface_hover: rgba(0x1C21_2814),
    surface_active: rgba(0x1C21_2820),
    border_default: rgb(0xD7DBE2),
    border_strong: rgb(0xB9C0CA),
    text_primary: rgb(0x1C2128),
    text_secondary: rgb(0x5B6472),
    text_disabled: rgb(0x9AA3AF),
    text_on_accent: rgb(0xFFFFFF),
    accent: rgb(0x2563D0),
    accent_fill: rgb(0x2563D0),
    accent_soft: rgba(0x2563_D01F),
    focus_ring: rgb(0x2563D0),
    status_error: StatusColours {
        text: rgb(0xC62E27),
        stroke: rgb(0xC62E27),
        tint: rgba(0xC62E_2714),
    },
    status_warning: StatusColours {
        text: rgb(0xA3580A),
        stroke: rgb(0xD98A1E),
        tint: rgba(0xD98A_1E1A),
    },
    status_info: StatusColours {
        text: rgb(0x2A6CC4),
        stroke: rgb(0x2A6CC4),
        tint: rgba(0x2A6C_C414),
    },
    status_success: StatusColours {
        text: rgb(0x1F7F43),
        stroke: rgb(0x1F7F43),
        tint: Color32::TRANSPARENT,
    },
    viewport_bg: rgb(0xFAFAF8),
    viewport_grid_minor: rgb(0xECEEF1),
    viewport_grid_major: rgb(0xD9DCE2),
    viewport_axis: rgb(0x6B7380),
    viewport_stock: rgb(0x6B7380),
    viewport_stock_fill: rgb(0xFFFFFF),
    viewport_male_board: rgb(0x8A929E),
    viewport_shape: rgb(0x23272E),
    viewport_shape_hover: rgb(0x5B8DE8),
    viewport_shape_selected: rgb(0x2563D0),
    viewport_shape_locked: rgb(0x8A929E),
    viewport_park: rgb(0x5B6472),
    viewport_rapid: rgb(0x8A929E),
    viewport_plunge: rgb(0xC62E27),
    viewport_tab: rgb(0x1C2128),
    viewport_extents_centre: rgb(0x5B6472),
    viewport_extents_edge: rgb(0x8A929E),
    viewport_label_bg: rgba(0xFFFF_FFE0),
    depth_ramp: [
        rgb(0xAFC2EE),
        rgb(0x7C98E0),
        rgb(0x4A68C9),
        rgb(0x2A44A6),
        rgb(0x1A2E8C),
    ],
    scrim: rgba(0x0000_0066),
    shadow: rgba(0x0000_001F),
};

/// The dark appearance.
pub const DARK: Palette = Palette {
    dark: true,
    surface_window: rgb(0x15181D),
    surface_panel: rgb(0x1E2229),
    surface_raised: rgb(0x262B33),
    surface_sunken: rgb(0x121519),
    surface_stripe: rgb(0x22262E),
    surface_hover: rgba(0xE8EB_F014),
    surface_active: rgba(0xE8EB_F020),
    border_default: rgb(0x333A44),
    border_strong: rgb(0x48515D),
    text_primary: rgb(0xE8EBF0),
    text_secondary: rgb(0xA3ABB7),
    text_disabled: rgb(0x6B7380),
    text_on_accent: rgb(0xFFFFFF),
    accent: rgb(0x6AA3F2),
    accent_fill: rgb(0x2F6FD6),
    accent_soft: rgba(0x6AA3_F229),
    focus_ring: rgb(0x6AA3F2),
    status_error: StatusColours {
        text: rgb(0xF2726A),
        stroke: rgb(0xF2726A),
        tint: rgba(0xF272_6A1F),
    },
    status_warning: StatusColours {
        text: rgb(0xF0B347),
        stroke: rgb(0xF0B347),
        tint: rgba(0xF0B3_471F),
    },
    status_info: StatusColours {
        text: rgb(0x78B0F5),
        stroke: rgb(0x78B0F5),
        tint: rgba(0x78B0_F51F),
    },
    status_success: StatusColours {
        text: rgb(0x5FC67F),
        stroke: rgb(0x5FC67F),
        tint: Color32::TRANSPARENT,
    },
    viewport_bg: rgb(0x1B1F26),
    viewport_grid_minor: rgb(0x232830),
    viewport_grid_major: rgb(0x2C323B),
    viewport_axis: rgb(0x9AA3AF),
    viewport_stock: rgb(0x9AA3AF),
    viewport_stock_fill: rgb(0x1F242C),
    viewport_male_board: rgb(0x7B8491),
    viewport_shape: rgb(0xE6E9EE),
    viewport_shape_hover: rgb(0x8FBAF7),
    viewport_shape_selected: rgb(0x6AA3F2),
    viewport_shape_locked: rgb(0x7B8491),
    viewport_park: rgb(0xA3ABB7),
    viewport_rapid: rgb(0x6E7786),
    viewport_plunge: rgb(0xF2726A),
    viewport_tab: rgb(0xF5F7FA),
    viewport_extents_centre: rgb(0xA3ABB7),
    viewport_extents_edge: rgb(0x7B8491),
    viewport_label_bg: rgba(0x262B_33E6),
    depth_ramp: [
        rgb(0xE4ECFF),
        rgb(0xBCCBF8),
        rgb(0x93A8F2),
        rgb(0x6E86EE),
        rgb(0x4A66E8),
    ],
    scrim: rgba(0x0000_0066),
    shadow: rgba(0x0000_004D),
};

impl Palette {
    /// The palette of `theme`.
    #[must_use]
    pub const fn of(theme: Theme) -> &'static Self {
        match theme {
            Theme::Light => &LIGHT,
            Theme::Dark => &DARK,
        }
    }

    /// The palette `ui` is drawn in.
    #[must_use]
    pub fn current(ui: &egui::Ui) -> &'static Self {
        if ui.visuals().dark_mode {
            &DARK
        } else {
            &LIGHT
        }
    }

    /// Every single-colour token with the name `experience.md` gives it, for
    /// checking the palette against the spec.
    #[must_use]
    pub fn tokens(&self) -> Vec<(&'static str, Color32)> {
        vec![
            ("surface.window", self.surface_window),
            ("surface.panel", self.surface_panel),
            ("surface.raised", self.surface_raised),
            ("surface.sunken", self.surface_sunken),
            ("surface.stripe", self.surface_stripe),
            ("surface.hover", self.surface_hover),
            ("surface.active", self.surface_active),
            ("border.default", self.border_default),
            ("border.strong", self.border_strong),
            ("text.primary", self.text_primary),
            ("text.secondary", self.text_secondary),
            ("text.disabled", self.text_disabled),
            ("text.on-accent", self.text_on_accent),
            ("accent", self.accent),
            ("accent.fill", self.accent_fill),
            ("accent.soft", self.accent_soft),
            ("focus.ring", self.focus_ring),
            ("viewport.bg", self.viewport_bg),
            ("viewport.grid.minor", self.viewport_grid_minor),
            ("viewport.grid.major", self.viewport_grid_major),
            ("viewport.axis", self.viewport_axis),
            ("viewport.stock", self.viewport_stock),
            ("viewport.stock.fill", self.viewport_stock_fill),
            ("viewport.male-board", self.viewport_male_board),
            ("viewport.shape", self.viewport_shape),
            ("viewport.shape.hover", self.viewport_shape_hover),
            ("viewport.shape.selected", self.viewport_shape_selected),
            ("viewport.shape.locked", self.viewport_shape_locked),
            ("viewport.park", self.viewport_park),
            ("viewport.rapid", self.viewport_rapid),
            ("viewport.plunge", self.viewport_plunge),
            ("viewport.tab", self.viewport_tab),
            ("viewport.extents.centre", self.viewport_extents_centre),
            ("viewport.extents.edge", self.viewport_extents_edge),
            ("viewport.label.bg", self.viewport_label_bg),
        ]
    }
}

/// One sRGB channel, 0 to 1, as linear light.
#[must_use]
pub fn linear(gamma: f32) -> f32 {
    if gamma <= 0.04045 {
        gamma / 12.92
    } else {
        ((gamma + 0.055) / 1.055).powf(2.4)
    }
}

/// One linear channel, 0 to 1, as sRGB.
#[must_use]
pub fn gamma(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

/// The depth ramp of `stops` at `t` (0 at the stock top to 1 at the
/// deepest cut) in linear light (section 5, "Depth ramp": "Five stops
/// interpolated in linear sRGB").
#[must_use]
pub fn ramp_linear(stops: &[Color32; 5], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0) * 4.0;
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "t is in [0, 4]"
    )]
    let i = (t.floor() as usize).min(3);
    #[allow(clippy::cast_precision_loss, reason = "i is under 4")]
    let f = t - i as f32;
    let lin = |c: &Color32| {
        let [r, g, b, _] = c.to_normalized_gamma_f32();
        [linear(r), linear(g), linear(b)]
    };
    let (a, b) = (
        stops.get(i).map_or([0.0; 3], lin),
        stops.get(i + 1).map_or([0.0; 3], lin),
    );
    [
        a[0] + (b[0] - a[0]) * f,
        a[1] + (b[1] - a[1]) * f,
        a[2] + (b[2] - a[2]) * f,
    ]
}

impl Palette {
    /// The depth ramp's colour at `t`, 0 at the stock top to 1 at the
    /// deepest cut: the legend bar's, and the renderer's texel.
    #[must_use]
    pub fn ramp_at(&self, t: f32) -> Color32 {
        let [r, g, b] = ramp_linear(&self.depth_ramp, t);
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a channel in [0, 1] times 255, rounded"
        )]
        let byte = |c: f32| (gamma(c).clamp(0.0, 1.0) * 255.0).round() as u8;
        Color32::from_rgb(byte(r), byte(g), byte(b))
    }
}

/// The operation colours on the light Viewport (`experience.md`, section
/// 5, "Operation colours", the light set the maintainer accepted with
/// design open question 5): `dc_job::preview::PALETTE`, read at compile
/// time, so the light Viewport and `--preview` share one table.
pub const OPERATION_LIGHT: [Color32; 8] = preview_palette();

/// The operation colours on the dark Viewport: section 5's dark set, the
/// same hues in the same order.
pub const OPERATION_DARK: [Color32; 8] = [
    rgb(0x5BA6EA),
    rgb(0xF0605A),
    rgb(0x5FC95F),
    rgb(0xB48FE0),
    rgb(0xFFA046),
    rgb(0x4FD4E4),
    rgb(0xC8907F),
    rgb(0xF08FD2),
];

impl Palette {
    /// The colour of the operation at `index` in the list, the preview's:
    /// the palette's eight in turn.
    #[must_use]
    pub fn operation(&self, index: usize) -> Color32 {
        let set = if self.dark {
            &OPERATION_DARK
        } else {
            &OPERATION_LIGHT
        };
        set.get(index % set.len())
            .copied()
            .unwrap_or(self.text_primary)
    }
}

/// The proportional face's family names: one family per weight, since
/// egui has no weight property (section 12, "Fonts").
pub const MEDIUM: &str = "medium";
/// The SemiBold family.
pub const SEMIBOLD: &str = "semibold";
/// The Lucide icon font's family.
pub const ICONS: &str = "icons";

/// The text style of a panel title or section header: 13 px SemiBold.
pub const TITLE: &str = "Title";
/// An emphasised row name: 13 px Medium.
pub const MEDIUM_STYLE: &str = "Medium";
/// The empty-state heading and sheet titles: 18 px SemiBold.
pub const DISPLAY: &str = "Display";

/// Icon sizes (section 6): 16 px in rows and the status bar, 20 px in the
/// toolbar and sheet headers.
pub const ICON_SMALL_PX: f32 = 16.0;
/// The toolbar's icon size.
pub const ICON_LARGE_PX: f32 = 20.0;

/// `FontFamily::Name(name)`.
fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

/// The bundled faces (section 4): Inter in three static weights with
/// tabular figures frozen in, JetBrains Mono, and Lucide's icon font.
/// egui's own fonts are not compiled in (`default_fonts` is off).
#[must_use]
pub fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::empty();
    let faces: [(&str, &'static [u8]); 5] = [
        (
            "Inter-Regular",
            include_bytes!("../../assets/fonts/inter/Inter-Regular.ttf"),
        ),
        (
            "Inter-Medium",
            include_bytes!("../../assets/fonts/inter/Inter-Medium.ttf"),
        ),
        (
            "Inter-SemiBold",
            include_bytes!("../../assets/fonts/inter/Inter-SemiBold.ttf"),
        ),
        (
            "JetBrainsMono-Regular",
            include_bytes!("../../assets/fonts/jetbrains-mono/JetBrainsMono-Regular.ttf"),
        ),
        (
            "lucide",
            include_bytes!("../../assets/icons/lucide/lucide.ttf"),
        ),
    ];
    for (name, bytes) in faces {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }
    // Lucide follows each text face, so an icon's code point renders
    // inside a label too; Inter backs up the mono face for symbols it lacks.
    let families = [
        (FontFamily::Proportional, ["Inter-Regular", "lucide"]),
        (family(MEDIUM), ["Inter-Medium", "lucide"]),
        (family(SEMIBOLD), ["Inter-SemiBold", "lucide"]),
        (
            FontFamily::Monospace,
            ["JetBrainsMono-Regular", "Inter-Regular"],
        ),
        (family(ICONS), ["lucide", "Inter-Regular"]),
    ];
    for (key, names) in families {
        fonts
            .families
            .insert(key, names.iter().map(|n| (*n).to_owned()).collect());
    }
    fonts
}

/// The type scale (section 4; section 12, "Fonts").
fn text_styles() -> std::collections::BTreeMap<TextStyle, FontId> {
    [
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (TextStyle::Heading, FontId::new(15.0, family(SEMIBOLD))),
        (
            TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        ),
        (
            TextStyle::Name(TITLE.into()),
            FontId::new(13.0, family(SEMIBOLD)),
        ),
        (
            TextStyle::Name(MEDIUM_STYLE.into()),
            FontId::new(13.0, family(MEDIUM)),
        ),
        (
            TextStyle::Name(DISPLAY.into()),
            FontId::new(18.0, family(SEMIBOLD)),
        ),
    ]
    .into_iter()
    .collect()
}

/// A widget state's look (section 12, "Visuals"): no expansion, so hover
/// never grows a control, and a 4-px radius.
fn widget(
    bg_fill: Color32,
    weak_bg_fill: Color32,
    bg_stroke: Stroke,
    fg: Color32,
) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill,
        weak_bg_fill,
        bg_stroke,
        corner_radius: CornerRadius::same(4),
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    }
}

/// `palette` as egui's `Visuals` (section 12, "Visuals").
#[must_use]
pub fn visuals(palette: &Palette) -> Visuals {
    let base = if palette.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    let shadow = |blur: u8| Shadow {
        offset: [0, 4],
        blur,
        spread: 0,
        color: palette.shadow,
    };
    Visuals {
        dark_mode: palette.dark,
        override_text_color: None,
        widgets: Widgets {
            noninteractive: widget(
                palette.surface_panel,
                palette.surface_panel,
                Stroke::new(1.0, palette.border_default),
                palette.text_primary,
            ),
            inactive: widget(
                palette.surface_sunken,
                Color32::TRANSPARENT,
                Stroke::new(1.0, palette.border_strong),
                palette.text_primary,
            ),
            hovered: widget(
                palette.surface_sunken,
                palette.surface_hover,
                Stroke::new(1.0, palette.text_secondary),
                palette.text_primary,
            ),
            active: widget(
                palette.surface_sunken,
                palette.surface_active,
                Stroke::new(1.0, palette.accent),
                palette.text_primary,
            ),
            open: widget(
                palette.surface_raised,
                palette.surface_active,
                Stroke::new(1.0, palette.border_strong),
                palette.text_primary,
            ),
        },
        selection: Selection {
            bg_fill: palette.accent_soft,
            stroke: Stroke::new(1.0, palette.accent),
        },
        hyperlink_color: palette.accent,
        faint_bg_color: palette.surface_stripe,
        extreme_bg_color: palette.surface_sunken,
        text_edit_bg_color: Some(palette.surface_sunken),
        code_bg_color: palette.surface_stripe,
        warn_fg_color: palette.status_warning.text,
        error_fg_color: palette.status_error.text,
        window_corner_radius: CornerRadius::same(8),
        window_shadow: shadow(16),
        window_fill: palette.surface_raised,
        window_stroke: Stroke::new(1.0, palette.border_default),
        menu_corner_radius: CornerRadius::same(6),
        panel_fill: palette.surface_panel,
        popup_shadow: shadow(12),
        text_cursor: TextCursorStyle {
            stroke: Stroke::new(1.5, palette.accent),
            ..base.text_cursor.clone()
        },
        button_frame: true,
        collapsing_header_frame: false,
        indent_has_left_vline: false,
        striped: true,
        slider_trailing_fill: true,
        ..base
    }
}

/// The style of `palette`: its visuals, the type scale, and section 12's
/// spacing, interaction and animation settings.
#[must_use]
pub fn style(palette: &Palette) -> Style {
    let mut style = Style {
        text_styles: text_styles(),
        visuals: visuals(palette),
        animation_time: 0.15,
        ..Style::default()
    };
    let spacing = &mut style.spacing;
    spacing.item_spacing = egui::vec2(8.0, 4.0);
    spacing.button_padding = egui::vec2(8.0, 4.0);
    spacing.window_margin = Margin::same(12);
    spacing.menu_margin = Margin::same(4);
    spacing.indent = 16.0;
    spacing.interact_size = egui::vec2(24.0, 26.0);
    spacing.icon_width = 16.0;
    spacing.icon_width_inner = 10.0;
    spacing.icon_spacing = 8.0;
    spacing.combo_width = 160.0;
    spacing.text_edit_width = 160.0;
    spacing.tooltip_width = 320.0;
    spacing.scroll = ScrollStyle {
        bar_width: 6.0,
        floating: true,
        dormant_background_opacity: 0.0,
        active_background_opacity: 0.4,
        interact_background_opacity: 0.6,
        ..ScrollStyle::thin()
    };
    style.interaction.tooltip_delay = 0.5;
    style.interaction.tooltip_grace_time = 0.2;
    style
}

/// What egui calls `preference`.
#[must_use]
pub fn preference(theme: dc_app::Theme) -> ThemePreference {
    match theme {
        dc_app::Theme::System => ThemePreference::System,
        dc_app::Theme::Light => ThemePreference::Light,
        dc_app::Theme::Dark => ThemePreference::Dark,
    }
}

/// Installs the fonts and both themes on `ctx` (section 12: "Set
/// everything once in a `theme::apply(ctx, Appearance)` call at startup"),
/// then [`follow`]s `theme`.
pub fn apply(ctx: &egui::Context, theme: dc_app::Theme) {
    ctx.set_fonts(fonts());
    ctx.set_style_of(Theme::Light, style(&LIGHT));
    ctx.set_style_of(Theme::Dark, style(&DARK));
    follow(ctx, theme);
}

/// Makes `theme` the appearance preference: the OS's choice for
/// [`dc_app::Theme::System`], else the one named. A no-op when it already
/// is, so it can run every frame.
pub fn follow(ctx: &egui::Context, theme: dc_app::Theme) {
    let wanted = preference(theme);
    if ctx.options(|o| o.theme_preference) != wanted {
        ctx.options_mut(|o| o.theme_preference = wanted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every token pair is distinct between the modes where the spec
    /// makes them so: light and dark are one design, not one palette
    /// (section 1, principle 6).
    #[test]
    fn light_and_dark_differ_where_the_spec_differs() {
        assert_ne!(LIGHT.surface_panel, DARK.surface_panel);
        assert_ne!(LIGHT.viewport_bg, DARK.viewport_bg);
        assert_eq!(LIGHT.text_on_accent, DARK.text_on_accent);
        const { assert!(!LIGHT.dark && DARK.dark) };
    }

    /// The ramp runs from its first stop to its last through the middle
    /// one, interpolated in linear light.
    #[test]
    fn ramp_interpolates_its_stops_in_linear_light() {
        let lin = |c: Color32| {
            let [r, g, b, _] = c.to_normalized_gamma_f32();
            [linear(r), linear(g), linear(b)]
        };
        for (t, stop) in [(0.0, 0), (0.5, 2), (1.0, 4)] {
            let got = ramp_linear(&LIGHT.depth_ramp, t);
            let want = lin(LIGHT.depth_ramp[stop]);
            for k in 0..3 {
                assert!(
                    (got[k] - want[k]).abs() < 1e-6,
                    "t {t}: {got:?} vs {want:?}"
                );
            }
            assert_eq!(LIGHT.ramp_at(t), LIGHT.depth_ramp[stop], "t {t}");
        }
        let a = lin(LIGHT.depth_ramp[0]);
        let b = lin(LIGHT.depth_ramp[1]);
        let mid = ramp_linear(&LIGHT.depth_ramp, 0.125);
        for k in 0..3 {
            assert!((mid[k] - (a[k] + b[k]) / 2.0).abs() < 1e-6);
        }
    }

    /// The type scale has no size under 12 px and only the spec's sizes
    /// (section 13, item 5).
    #[test]
    fn type_scale_is_the_spec_sizes() {
        for (style, font) in text_styles() {
            assert!(
                [12.0, 13.0, 15.0, 18.0]
                    .iter()
                    .any(|s| (font.size - s).abs() < 1e-6),
                "{style:?} is {} px",
                font.size
            );
        }
    }
}
