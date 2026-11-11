// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the Viewport draws over its toolpaths (functional spec,
//! capabilities 73a and 74; `experience.md`, section 9, "Depth legend"
//! and "Program extents"; mock-up `09-preview-legend.svg`): in *By depth*,
//! the depth legend at the pane's bottom-left, and the extents' caption
//! pill. The toolpath view's controls are a popover from the status bar
//! (`shell::toolpaths`), so the Viewport's corner holds no card (#616).

use dc_app::view::{DepthLegend, ProgramExtents, range_label};
use dc_app::{App, StrKey};
use eframe::egui::{self, Align2, Rect, Stroke, TextStyle, Ui};

use crate::shell::theme::{self, Palette};
use crate::viewport::renderer::gpu_f32;

/// The legend's inset from the pane's corner, px.
const INSET_PX: f32 = 12.0;
/// Its inner margin, px.
const PAD_PX: f32 = 8.0;
/// The cutting ramp's bar, px.
const RAMP_BAR: egui::Vec2 = egui::vec2(12.0, 160.0);
/// The non-cutting band's height above it, px.
const BAND_PX: f32 = 40.0;
/// Two legend labels closer than this stack side by side, px.
const LABEL_GAP_PX: f32 = 14.0;

/// The room the legend takes from the pane's left edge, px, inset
/// included: what Fit leaves free when the legend is on (`experience.md`,
/// section 9: "It never overlaps the stock at fit"). Measured as the
/// legend lays itself out.
pub(crate) fn reserve_px(ui: &Ui, app: &App, legend: &DepthLegend) -> f32 {
    let painter = ui.painter();
    let layout = Layout::of(ui, painter, app, legend);
    INSET_PX + layout.size.x + INSET_PX
}

/// The legend's parts, laid out against an origin at its top-left inside
/// the padding.
struct Layout {
    title: std::sync::Arc<egui::Galley>,
    caption: Option<std::sync::Arc<egui::Galley>>,
    /// Each tick: its y, its label's x and its label.
    rows: Vec<(f32, f32, std::sync::Arc<egui::Galley>)>,
    /// Where the non-cutting band starts, and its height (0 without one).
    band_top: f32,
    band_px: f32,
    /// The size of the whole, padding included.
    size: egui::Vec2,
}

impl Layout {
    fn of(ui: &Ui, painter: &egui::Painter, app: &App, legend: &DepthLegend) -> Self {
        let palette = Palette::current(ui);
        let strings = app.strings();
        let small = TextStyle::Small.resolve(ui.style());
        let unit = dc_app::units::suffix(legend.units);
        let title = painter.layout_no_wrap(
            strings.fmt(StrKey::AppToolpathsLegend, &[("unit", unit)]),
            TextStyle::Name(theme::TITLE.into()).resolve(ui.style()),
            palette.text_primary,
        );
        let band_px = if legend.ticks.iter().any(|t| t.fraction < 0.0) {
            BAND_PX
        } else {
            0.0
        };
        let band_top = title.size().y + 6.0;
        let ramp_top = band_top + band_px;
        let mut rows = Vec::with_capacity(legend.ticks.len());
        let mut last: Option<(f32, f32)> = None;
        let mut width = RAMP_BAR.x.max(title.size().x);
        for tick in &legend.ticks {
            let galley = painter.layout_no_wrap(
                format!("{}  {}", tick.label, tick.names_text(strings)),
                small.clone(),
                palette.text_secondary,
            );
            let y = ramp_top
                + RAMP_BAR.y * gpu_f32(tick.fraction).max(0.0)
                + band_px * gpu_f32(tick.fraction).min(0.0);
            let mut x = RAMP_BAR.x + 8.0;
            if let Some((last_y, last_right)) = last
                && (y - last_y).abs() < LABEL_GAP_PX
            {
                x = last_right + 8.0;
            }
            let right = x + galley.size().x;
            width = width.max(right);
            last = Some((y, right));
            rows.push((y, x, galley));
        }
        let caption = (band_px > 0.0).then(|| {
            painter.layout_no_wrap(
                strings.get(StrKey::AppToolpathsLegendBand).to_owned(),
                small.clone(),
                palette.text_secondary,
            )
        });
        let mut height = ramp_top + RAMP_BAR.y + 6.0;
        if let Some(caption) = &caption {
            width = width.max(caption.size().x);
            height += caption.size().y + 4.0;
        }
        Self {
            title,
            caption,
            rows,
            band_top,
            band_px,
            size: egui::vec2(width + 2.0 * PAD_PX, height + 2.0 * PAD_PX),
        }
    }
}

/// The depth legend at `pane`'s bottom-left: the band where the bit moves
/// without cutting, in the rapid grey with diagonal hatching, ticked at
/// the clearance height and each safe Z; the stock top; the cutting ramp
/// as a 12 × 160 px bar, ticked at each operation's depth and the deepest
/// cut; labels that would overlap stacked to the right; on a
/// `viewport.label.bg` card so it reads over the toolpaths. Drawn in its
/// own non-interactive area rather than a bare layer: egui orders areas,
/// so a sheet opened later stacks above it (#549), where a bare layer
/// would paint over the sheet. Returns the legend's rectangle.
pub(crate) fn draw_legend(ui: &Ui, pane: Rect, app: &App, legend: &DepthLegend) -> Rect {
    egui::Area::new(egui::Id::unique("viewport.depth_legend"))
        .order(egui::Order::Foreground)
        .fixed_pos(pane.min)
        .interactable(false)
        .show(ui.ctx(), |ui| {
            let painter = ui.painter().clone();
            paint_legend(ui, &painter, pane, app, legend)
        })
        .inner
}

/// [`draw_legend`]'s painting, on `painter`; returns the legend's frame.
fn paint_legend(
    ui: &Ui,
    painter: &egui::Painter,
    pane: Rect,
    app: &App,
    legend: &DepthLegend,
) -> Rect {
    let palette = Palette::current(ui);
    let layout = Layout::of(ui, painter, app, legend);
    let frame = Rect::from_min_size(
        egui::pos2(
            pane.left() + INSET_PX,
            (pane.bottom() - INSET_PX - layout.size.y).max(pane.top() + INSET_PX),
        ),
        layout.size,
    );
    painter.rect_filled(frame, 8.0, palette.viewport_label_bg);
    let origin = frame.min + egui::vec2(PAD_PX, PAD_PX);
    painter.galley(origin, layout.title, palette.text_primary);
    if layout.band_px > 0.0 {
        let band = Rect::from_min_size(
            origin + egui::vec2(0.0, layout.band_top),
            egui::vec2(RAMP_BAR.x, layout.band_px),
        );
        painter.rect_filled(band, 0.0, palette.viewport_rapid.gamma_multiply(0.35));
        // Diagonal 1-px hatching, every 4 px.
        let hatch = painter.with_clip_rect(band);
        let mut x = band.left() - band.height();
        while x < band.right() {
            hatch.line_segment(
                [
                    egui::pos2(x, band.bottom()),
                    egui::pos2(x + band.height(), band.top()),
                ],
                Stroke::new(1.0, palette.viewport_rapid),
            );
            x += 4.0;
        }
        painter.rect_stroke(
            band,
            0.0,
            Stroke::new(1.0, palette.border_default),
            egui::StrokeKind::Outside,
        );
    }
    let bar = Rect::from_min_size(
        origin + egui::vec2(0.0, layout.band_top + layout.band_px),
        RAMP_BAR,
    );
    let steps = 64_u8;
    for i in 0..steps {
        let (t0, t1) = (
            f32::from(i) / f32::from(steps),
            f32::from(i + 1) / f32::from(steps),
        );
        let slice = Rect::from_min_max(
            egui::pos2(bar.left(), bar.top() + bar.height() * t0),
            egui::pos2(bar.right(), bar.top() + bar.height() * t1),
        );
        painter.rect_filled(slice, 0.0, palette.ramp_at((t0 + t1) / 2.0));
    }
    painter.rect_stroke(
        bar,
        0.0,
        Stroke::new(1.0, palette.border_default),
        egui::StrokeKind::Outside,
    );
    for (y, x, galley) in layout.rows {
        let y = origin.y + y;
        painter.line_segment(
            [egui::pos2(bar.right(), y), egui::pos2(bar.right() + 4.0, y)],
            Stroke::new(1.0, palette.text_secondary),
        );
        let rect = Align2::LEFT_CENTER.anchor_size(egui::pos2(origin.x + x, y), galley.size());
        painter.galley(rect.min, galley, palette.text_secondary);
    }
    if let Some(caption) = layout.caption {
        painter.galley(
            egui::pos2(origin.x, bar.bottom() + 10.0),
            caption,
            palette.text_secondary,
        );
    }
    frame
}
/// The extents' caption (`experience.md`, section 9: *Program extents
/// (bit edge, dashed)* and its ranges) in a `viewport.label.bg` pill at
/// the bit-edge box's top-left, `edge` on the window. Where that place is
/// under the depth legend (`covers`), it sits under the box's
/// bottom-left instead, so the caption stays readable at the minimum
/// window (#549).
pub(crate) fn extents_caption(
    ui: &Ui,
    painter: &egui::Painter,
    app: &App,
    extents: &ProgramExtents,
    edge: Rect,
    covers: &[Rect],
) {
    let palette = Palette::current(ui);
    let text = app.strings().fmt(
        StrKey::AppToolpathsExtentsCaption,
        &[
            ("x", &range_label(extents.edge_x, extents.units)),
            ("y", &range_label(extents.edge_y, extents.units)),
        ],
    );
    let galley = painter.layout_no_wrap(
        text,
        TextStyle::Small.resolve(ui.style()),
        palette.text_secondary,
    );
    let size = galley.size() + egui::vec2(12.0, 4.0);
    let above = Rect::from_min_size(edge.left_top() + egui::vec2(0.0, -size.y - 4.0), size);
    let below = Rect::from_min_size(edge.left_bottom() + egui::vec2(0.0, 4.0), size);
    // Kept inside the pane: shifted left where it would run off its right.
    let pane = painter.clip_rect();
    let inside = |r: Rect| r.translate(egui::vec2((pane.right() - 4.0 - r.right()).min(0.0), 0.0));
    let (above, below) = (inside(above), inside(below));
    let rect = [above, below]
        .into_iter()
        .find(|r| !covers.iter().any(|c| c.intersects(*r)))
        .unwrap_or(above);
    painter.rect_filled(rect, 6.0, palette.viewport_label_bg);
    painter.galley(
        rect.min + egui::vec2(6.0, 2.0),
        galley,
        palette.text_secondary,
    );
}
