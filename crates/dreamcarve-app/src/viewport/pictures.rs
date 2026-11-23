// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Pictures on the Viewport (functional spec, capabilities 56 to 58;
//! `experience.md`, sections 5 and 9): each visible bitmap is a textured
//! quad at its footprint, under the shapes and over the grid
//! ([`quads`], drawn by `renderer`'s picture pipeline), at 60 % while a
//! shape is selected so the selection reads over it.
//!
//! A picture is a thing to select and place, as a shape is. A click
//! inside its footprint where nothing above takes the click selects it
//! (`dc_app::viewport::pick::Hit::Bitmap`); a drag from it moves the
//! selected pictures; the one selected picture's corner handles resize it
//! about its centre with its proportions kept, since a picture is never
//! stretched ([`Grip`], [`dragged`]). While the button is down the quad
//! and the handles are drawn where the release will leave them
//! ([`Preview`]), and the release is one `MoveBitmaps` or `ResizeBitmap`.
//! A locked picture refuses both, with the reason in the status bar. The
//! outline, the handles, the padlock and the marked Photo V-Carve's
//! colour are the overlay's ([`overlay`]).
//!
//! The texture's samples are the Scene's (`SceneBitmap::pixels`, decoded
//! by `dc-app` once per stored picture); nothing is decoded here. The mip
//! chain that keeps a picture shown small from shimmering is built once
//! per stored picture ([`Mips`]): in the frame for a picture of up to
//! [`MIPS_INLINE_SAMPLES`] samples, on a thread of its own for a larger
//! one, which draws through its own samples until the chain lands.
//!
//! Positions go through the camera's one Y flip, as everything on the
//! Viewport does; the quad's own orientation is the shader's
//! (`picture.wgsl`).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use dc_app::scene::SceneBitmap;
use dc_app::viewport::camera::{Camera, Pos};
use dc_app::viewport::handles::{HANDLE_HIT_PX, MIN_SCALE};
use dc_app::{Command, CommandError, StrKey};
use dc_geometry::{Point, Rect as MmRect};
use dc_job::picture::PictureSize;
use dc_project::{BitmapId, Grayscale, Project};
use eframe::egui::{self, Pos2, Rect, Stroke, Ui};

use super::renderer::{self, MipLevel, PictureQuad};
use crate::shell::theme::{self, Palette};
use crate::shell::window::FrameState;
use crate::widgets::icon::Icon;

/// The largest picture whose mip chain is built in the frame, samples: a
/// megapixel takes a few milliseconds. A larger one is built on a thread.
pub(super) const MIPS_INLINE_SAMPLES: usize = 1 << 20;
/// A selected picture's outline, px: a selected shape's.
const SELECTED_PX: f32 = 2.0;
/// A hovered picture's outline, px: a shape's.
const HOVER_PX: f32 = 1.25;

/// What a press on a picture took hold of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Grip {
    /// The picture itself: a drag moves the selected pictures.
    Move,
    /// A corner handle of the one selected picture, by its place in
    /// [`corners`]: a drag resizes it about its centre.
    Corner(usize),
}

/// The selected pictures drawn where the drag in progress will put them.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Preview {
    /// The pictures dragged.
    pub ids: Vec<BitmapId>,
    /// What the drag holds.
    pub grip: Grip,
    /// A move's offset, mm.
    pub dx_mm: f64,
    /// A move's offset, mm.
    pub dy_mm: f64,
    /// A resize's footprint for the one picture, about its centre.
    pub resized: Option<MmRect>,
}

impl Preview {
    /// Where picture `id`, whose footprint is `footprint`, draws while
    /// the drag is on.
    pub(super) fn placed(&self, id: &BitmapId, footprint: MmRect) -> MmRect {
        if !self.ids.contains(id) {
            return footprint;
        }
        match (self.grip, self.resized) {
            (Grip::Corner(_), Some(resized)) => resized,
            _ => MmRect {
                min: Point::new(footprint.min.x + self.dx_mm, footprint.min.y + self.dy_mm),
                max: Point::new(footprint.max.x + self.dx_mm, footprint.max.y + self.dy_mm),
            },
        }
    }

    /// The command the release runs: one `MoveBitmaps`, or one
    /// `ResizeBitmap` to the previewed width.
    ///
    /// # Errors
    ///
    /// What `dc_app::pictures::move_by` and `resize` refuse;
    /// [`CommandError::NothingToDo`] for a drag that ended where it began.
    pub(super) fn command(&self, project: &Project) -> Result<Command, CommandError> {
        match (self.grip, self.resized, self.ids.as_slice()) {
            (Grip::Corner(_), Some(resized), [id]) => dc_app::pictures::resize(
                project,
                id,
                PictureSize::Width {
                    width_mm: resized.width_mm(),
                },
            ),
            (Grip::Corner(_), _, _) => Err(CommandError::NothingToDo),
            (Grip::Move, _, _) => {
                dc_app::pictures::move_by(project, &self.ids, self.dx_mm, self.dy_mm)
            }
        }
    }
}

/// `rect`'s corners: bottom-left, bottom-right, top-right, top-left, as
/// `dc_app`'s handles order a selection's.
pub(super) const fn corners(rect: MmRect) -> [Point; 4] {
    let (min, max) = (rect.min, rect.max);
    [min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)]
}

/// The footprint of the one picture selected, when exactly one is and the
/// Scene draws it: the picture whose corners are handles.
pub(super) fn handled<'a>(state: &FrameState<'a>) -> Option<&'a SceneBitmap> {
    match state.app.selected_pictures() {
        [one] => state.app.scene().bitmap(one),
        _ => None,
    }
}

/// The corner handle of the one selected picture under pane point `at`,
/// within the handles' hit distance.
pub(super) fn corner_at(state: &FrameState<'_>, camera: &Camera, at: Pos) -> Option<usize> {
    let bitmap = handled(state)?;
    corners(bitmap.footprint)
        .iter()
        .position(|corner| camera.to_screen(*corner).distance_to(at) <= HANDLE_HIT_PX)
}

/// The preview of a drag of `grip` on the pictures `ids` from project
/// point `from` to `to`: the offset for a move; for a corner, the one
/// picture's footprint scaled about its centre by how far the pointer has
/// gone along the corner's own direction, its proportions kept and never
/// under [`MIN_SCALE`].
pub(super) fn dragged(
    project: &Project,
    ids: &[BitmapId],
    grip: Grip,
    from: Point,
    to: Point,
) -> Preview {
    let mut preview = Preview {
        ids: ids.to_vec(),
        grip,
        dx_mm: to.x - from.x,
        dy_mm: to.y - from.y,
        resized: None,
    };
    let (Grip::Corner(corner), Some(bounds)) = (grip, dc_app::pictures::bounds(project, ids))
    else {
        return preview;
    };
    let centre = Point::new(
        f64::midpoint(bounds.min.x, bounds.max.x),
        f64::midpoint(bounds.min.y, bounds.max.y),
    );
    let Some(grabbed) = corners(bounds).get(corner).copied() else {
        return preview;
    };
    // The pointer's reach along the centre-to-corner line, as a share of
    // the corner's own: 1 where the drag began.
    let (ax, ay) = (grabbed.x - centre.x, grabbed.y - centre.y);
    let reach = ax * ax + ay * ay;
    if reach <= f64::EPSILON {
        return preview;
    }
    let moved = (
        to.x - from.x + grabbed.x - centre.x,
        to.y - from.y + grabbed.y - centre.y,
    );
    let scale = ((moved.0 * ax + moved.1 * ay) / reach).max(MIN_SCALE);
    let (half_w, half_h) = (
        bounds.width_mm() * scale / 2.0,
        bounds.height_mm() * scale / 2.0,
    );
    preview.resized = Some(MmRect {
        min: Point::new(centre.x - half_w, centre.y - half_h),
        max: Point::new(centre.x + half_w, centre.y + half_h),
    });
    preview
}

/// The status bar's text for `preview`: the offset of a move, the scale
/// of a resize, as a shape's drag reads.
pub(super) fn readout(state: &FrameState<'_>, preview: &Preview) -> String {
    let strings = state.app.strings();
    let project = state.app.project();
    let units = project.display_units;
    match (
        preview.resized,
        dc_app::pictures::bounds(project, &preview.ids),
    ) {
        (Some(resized), Some(bounds)) if bounds.width_mm() > 0.0 => {
            let percent = format!("{:.0}", resized.width_mm() / bounds.width_mm() * 100.0);
            strings.fmt(StrKey::PlacementScale, &[("x", &percent), ("y", &percent)])
        }
        _ => strings.fmt(
            StrKey::PlacementMove,
            &[
                ("dx", &super::readout_length(preview.dx_mm, units)),
                ("dy", &super::readout_length(preview.dy_mm, units)),
            ],
        ),
    }
}

/// A picture's mip chain, built or on its way.
#[derive(Debug)]
enum Chain {
    Ready(Arc<Vec<MipLevel>>),
    Building(Receiver<Vec<MipLevel>>),
}

/// The mip chains of the Scene's pictures, by the Scene's key of the
/// stored picture: built once each, and dropped with the picture.
#[derive(Debug, Default)]
pub(super) struct Mips {
    chains: BTreeMap<u64, Chain>,
}

impl Mips {
    /// Starts the chain of each of `bitmaps` that has none, takes those
    /// that have landed, and forgets those of pictures that have gone.
    /// Returns whether one is still being built, so the caller looks
    /// again soon.
    pub(super) fn follow(&mut self, bitmaps: &[SceneBitmap]) -> bool {
        self.chains
            .retain(|key, _| bitmaps.iter().any(|b| b.picture == *key));
        let mut building = false;
        for bitmap in bitmaps {
            let Some(pixels) = &bitmap.pixels else {
                continue;
            };
            let chain = self
                .chains
                .remove(&bitmap.picture)
                .unwrap_or_else(|| start(pixels));
            let chain = match chain {
                Chain::Building(landing) => match landing.try_recv() {
                    Ok(levels) => Chain::Ready(Arc::new(levels)),
                    Err(TryRecvError::Empty) => Chain::Building(landing),
                    // The thread ended without its chain: build it here.
                    Err(TryRecvError::Disconnected) => {
                        Chain::Ready(Arc::new(renderer::mips(pixels)))
                    }
                },
                ready @ Chain::Ready(_) => ready,
            };
            building |= matches!(chain, Chain::Building(_));
            self.chains.insert(bitmap.picture, chain);
        }
        building
    }

    /// The chain of the picture `key`, once built.
    fn of(&self, key: u64) -> Option<Arc<Vec<MipLevel>>> {
        match self.chains.get(&key) {
            Some(Chain::Ready(levels)) => Some(Arc::clone(levels)),
            _ => None,
        }
    }
}

/// The chain of `pixels`: built now for a picture of up to
/// [`MIPS_INLINE_SAMPLES`], else on a thread of its own, so a camera's
/// picture never holds a frame.
fn start(pixels: &Arc<Grayscale>) -> Chain {
    if pixels.bytes.len() > MIPS_INLINE_SAMPLES {
        let (send, landing) = mpsc::channel();
        let shared = Arc::clone(pixels);
        let spawned = std::thread::Builder::new()
            .name("dreamcarve-mips".to_owned())
            .spawn(move || {
                // Nobody listens once the picture has gone: fine.
                let _ = send.send(renderer::mips(&shared));
            });
        if spawned.is_ok() {
            return Chain::Building(landing);
        }
    }
    Chain::Ready(Arc::new(renderer::mips(pixels)))
}

/// The frame's picture quads, bottom first: every visible bitmap the
/// Scene holds pixels for, at its footprint, or where `preview` puts it,
/// drawn at `opacity` times its own (a backdrop's, phase 5 capability
/// 101).
pub(super) fn quads(
    bitmaps: &[SceneBitmap],
    mips: &Mips,
    preview: Option<&Preview>,
    opacity: f32,
) -> Vec<PictureQuad> {
    bitmaps
        .iter()
        .filter_map(|bitmap| {
            let pixels = bitmap.pixels.as_ref()?;
            let rect = preview.map_or(bitmap.footprint, |p| p.placed(&bitmap.id, bitmap.footprint));
            Some(PictureQuad {
                key: bitmap.picture,
                pixels: Arc::clone(pixels),
                mips: mips.of(bitmap.picture),
                rect_mm: [
                    renderer::gpu_f32(rect.min.x),
                    renderer::gpu_f32(rect.min.y),
                    renderer::gpu_f32(rect.max.x),
                    renderer::gpu_f32(rect.max.y),
                ],
                opacity: opacity * renderer::gpu_f32(bitmap.opacity_fraction),
            })
        })
        .collect()
}

/// What the overlay marks of the pictures, besides the selection.
#[derive(Debug, Clone, Copy)]
pub(super) struct Marks<'a> {
    /// The picture under the pointer.
    pub hover: Option<&'a BitmapId>,
    /// The drag in progress.
    pub preview: Option<&'a Preview>,
}

/// `rect` (mm) on the window.
fn on_window(pane: Rect, camera: &Camera, rect: MmRect) -> Rect {
    Rect::from_two_pos(
        super::window_pos(pane, camera, rect.min),
        super::window_pos(pane, camera, rect.max),
    )
}

/// The pictures' part of the overlay (`experience.md`, section 8,
/// "Transform handles"): the outline of the picture under the pointer; a
/// 2-px `accent` outline round each selected picture, grey when it is
/// locked, with the four 8-px corner handles of the one selected; the
/// outline of the picture the marked Photo V-Carve carves, in that
/// operation's colour (capability 67); and a padlock by each locked
/// picture, as by a locked shape.
pub(super) fn overlay(
    ui: &Ui,
    painter: &egui::Painter,
    pane: Rect,
    state: &FrameState<'_>,
    camera: &Camera,
    marks: Marks<'_>,
) {
    let palette = Palette::current(ui);
    let app = state.app;
    let selected = app.selected_pictures();
    // Nothing is selected or moved in target editing.
    let editing_targets = app.target_editing().is_some();
    let carved = app.scene().targets().and_then(|m| {
        m.picture
            .as_ref()
            .map(|picture| (picture, palette.operation(m.colour)))
    });
    for bitmap in app.scene().bitmaps() {
        let footprint = marks
            .preview
            .map_or(bitmap.footprint, |p| p.placed(&bitmap.id, bitmap.footprint));
        let rect = on_window(pane, camera, footprint);
        let outline = |width: f32, colour: egui::Color32| {
            painter.rect_stroke(
                rect,
                0.0,
                Stroke::new(width, colour),
                egui::StrokeKind::Middle,
            );
        };
        let is_selected = selected.contains(&bitmap.id) && !editing_targets;
        if let Some((_, colour)) = carved.filter(|(picture, _)| **picture == bitmap.id) {
            outline(SELECTED_PX, colour);
        }
        if is_selected {
            let colour = if bitmap.locked {
                palette.viewport_shape_locked
            } else {
                palette.accent
            };
            outline(SELECTED_PX, colour);
            if selected.len() == 1 {
                let stroke = Stroke::new(1.0, colour);
                for corner in corners(footprint) {
                    let at: Pos2 = super::window_pos(pane, camera, corner);
                    painter.rect(
                        Rect::from_center_size(at, egui::vec2(super::HANDLE_PX, super::HANDLE_PX)),
                        0.0,
                        palette.viewport_handle_fill,
                        stroke,
                        egui::StrokeKind::Middle,
                    );
                }
            }
        } else if marks.hover == Some(&bitmap.id) && !editing_targets {
            outline(HOVER_PX, palette.viewport_shape_hover);
        }
        if bitmap.locked {
            let glyph = Icon::Lock.galley(ui, theme::ICON_SMALL_PX, palette.viewport_shape_locked);
            painter.galley(
                rect.right_top() + egui::vec2(2.0, -glyph.size().y - 2.0),
                glyph,
                palette.viewport_shape_locked,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> MmRect {
        MmRect {
            min: Point::new(x0, y0),
            max: Point::new(x1, y1),
        }
    }

    /// A move's preview shifts the dragged pictures and no other; a
    /// resize's replaces the one picture's footprint.
    #[test]
    fn a_preview_places_only_the_pictures_it_drags() {
        let (b1, b2) = (BitmapId::new("b1"), BitmapId::new("b2"));
        let footprint = rect(10.0, 10.0, 110.0, 60.0);
        let moving = Preview {
            ids: vec![b1.clone()],
            grip: Grip::Move,
            dx_mm: 5.0,
            dy_mm: -2.0,
            resized: None,
        };
        assert_eq!(moving.placed(&b1, footprint), rect(15.0, 8.0, 115.0, 58.0));
        assert_eq!(moving.placed(&b2, footprint), footprint);
        let resizing = Preview {
            grip: Grip::Corner(2),
            resized: Some(rect(35.0, 22.5, 85.0, 47.5)),
            ..moving
        };
        assert_eq!(
            resizing.placed(&b1, footprint),
            rect(35.0, 22.5, 85.0, 47.5)
        );
    }

    /// The corners are in the handles' order, so corner 2 is the top
    /// right in a Y-up project.
    #[test]
    fn corners_run_anticlockwise_from_the_bottom_left() {
        let c = corners(rect(0.0, 0.0, 4.0, 2.0));
        assert_eq!(
            c,
            [
                Point::new(0.0, 0.0),
                Point::new(4.0, 0.0),
                Point::new(4.0, 2.0),
                Point::new(0.0, 2.0)
            ]
        );
    }
}
