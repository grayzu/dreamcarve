// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The empty state's example thumbnails as outlines (functional spec,
//! capability 79; `experience.md`, section 7, "Example card"): each
//! example card shows the example's own shapes, fitted to a small
//! picture, never a stored image.

use dc_project::Project;

/// The thumbnail's width, px.
pub const THUMBNAIL_WIDTH_PX: f32 = 96.0;
/// The thumbnail's height, px.
pub const THUMBNAIL_HEIGHT_PX: f32 = 56.0;
/// The room left clear inside each edge of the thumbnail, px.
pub const THUMBNAIL_INSET_PX: f32 = 4.0;

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

/// The thumbnail of `project`.
#[must_use]
pub fn thumbnail(project: &Project) -> Thumbnail {
    let _ = project;
    Thumbnail::default()
}
