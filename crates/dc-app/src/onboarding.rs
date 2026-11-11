// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The empty state's example thumbnails as outlines (functional spec,
//! capability 79; `experience.md`, section 7, "Example card"): each
//! example card shows the example's own shapes, fitted to a small
//! picture, never a stored image. The outlines come from the [`Scene`],
//! so a thumbnail shows what the Viewport shows once the example opens.

use dc_project::Project;

use crate::generation::Generation;
use crate::scene::Scene;

/// The thumbnail's width, px.
pub const THUMBNAIL_WIDTH_PX: f32 = 96.0;
/// The thumbnail's height, px.
pub const THUMBNAIL_HEIGHT_PX: f32 = 56.0;
/// The room left clear inside each edge of the thumbnail, px.
pub const THUMBNAIL_INSET_PX: f32 = 4.0;

/// How far a chord may sit from the arc it stands for, px: a tenth of
/// the 1-px line the outlines are drawn with.
const CHORD_PX: f64 = 0.1;

/// One outline of a thumbnail.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Outline {
    /// Its points, px from the thumbnail's top-left corner, Y down, arcs
    /// as chords.
    pub points_px: Vec<[f32; 2]>,
    /// Whether the last point joins the first.
    pub closed: bool,
}

/// A project's visible shapes fitted to the thumbnail.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Thumbnail {
    /// One outline per profile of each visible instance, in the Scene's
    /// order.
    pub outlines: Vec<Outline>,
}

/// `x` px as the screen's `f32`.
#[allow(
    clippy::cast_possible_truncation,
    reason = "a position inside a 96 × 56 px picture"
)]
fn screen_px(x: f64) -> f32 {
    x as f32
}

/// The thumbnail of `project`: its visible instances' outlines, scaled
/// by one factor to fit [`THUMBNAIL_WIDTH_PX`] × [`THUMBNAIL_HEIGHT_PX`]
/// less [`THUMBNAIL_INSET_PX`] on each edge, centred, Y flipped for the
/// screen. A project with nothing visible has no outlines.
#[must_use]
pub fn thumbnail(project: &Project) -> Thumbnail {
    // No toolpaths are wanted, so nothing is generated for it.
    let scene = Scene::build(project, &Generation::new(project, false), 0);
    let Some(bounds) = scene.shapes_bounds() else {
        return Thumbnail::default();
    };
    let room_w_px = f64::from(THUMBNAIL_WIDTH_PX - 2.0 * THUMBNAIL_INSET_PX);
    let room_h_px = f64::from(THUMBNAIL_HEIGHT_PX - 2.0 * THUMBNAIL_INSET_PX);
    // A single point, or a line along an axis, has no extent to fit on
    // one axis or both: the other axis decides, or the scale is 1.
    let px_per_mm = [
        room_w_px / bounds.width_mm(),
        room_h_px / bounds.height_mm(),
    ]
    .into_iter()
    .filter(|s| s.is_finite() && *s > 0.0)
    .fold(None, |least: Option<f64>, s| {
        Some(least.map_or(s, |l| l.min(s)))
    })
    .unwrap_or(1.0);
    let centre_x_mm = f64::midpoint(bounds.min.x, bounds.max.x);
    let centre_y_mm = f64::midpoint(bounds.min.y, bounds.max.y);
    let tolerance_mm = CHORD_PX / px_per_mm;

    let outlines = scene
        .shapes()
        .iter()
        .flat_map(|shape| shape.outlines.iter())
        .map(|outline| Outline {
            points_px: outline
                .with_arcs_flattened(tolerance_mm)
                .vertices()
                .iter()
                .map(|v| {
                    [
                        THUMBNAIL_WIDTH_PX / 2.0 + screen_px((v.x - centre_x_mm) * px_per_mm),
                        THUMBNAIL_HEIGHT_PX / 2.0 - screen_px((v.y - centre_y_mm) * px_per_mm),
                    ]
                })
                .collect(),
            closed: outline.is_closed(),
        })
        .collect();
    Thumbnail { outlines }
}
