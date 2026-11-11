// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Photo V-Carve (format 27, #646; `docs/specs/phase-3/design.md`,
//! "Algorithms → Photo V-Carve"; functional capability 47): a picture
//! carved as parallel V-bit grooves, each as deep as the picture is dark
//! under it, up to `max_depth_mm`.
//!
//! Designed from RasterCarve's documented behaviour and VCarve's
//! PhotoVCarve form; nothing here is a port.
//!
//! - **Grooves** ([`grooves`]): with the bit's slope `tan(α/2)`, the
//!   full-depth width is `W = 2 · max_depth · slope` and the spacing
//!   `s = line_spacing_fraction · W`. The family is the lines
//!   `{ p : n·p = n·c + k·s }`, `k` an integer, with direction
//!   `u = (cos θ, sin θ)`, normal `n = (−sin θ, cos θ)`, through the
//!   picture's top-left corner `c`. Each is clipped to the footprint
//!   rectangle; a line that only touches it at a corner is a groove of
//!   one point. They are cut from the largest `k` to the smallest (for
//!   `θ` in [0°, 90°] that is from the line through `c` across the
//!   picture), even `k` along `+u`, odd along `−u`. `hatch` adds the
//!   family at `θ + 90°` after the first, laid the same way.
//! - **Depth**: along a groove, a sample every `sample_mm` from its start
//!   and one at its end; at each the depth is `(1 − g/255) · max_depth`,
//!   or `(g/255) · max_depth` with `invert`, where `g` is [`sample`], and
//!   `z = start_plane − depth`. Samples collinear in XYZ within
//!   [`EPS_MM`] merge into one move.
//! - **Moves**: one [`CutBlock`] per groove, a straight plunge at the
//!   plunge rate to its first sample, then its samples as straight cuts
//!   at the feed rate, slowed where one falls faster than the plunge rate
//!   (S10). Between two grooves of a family the bit rises to the **link
//!   height** at the feed rate, crosses there at the feed rate, and
//!   plunges ([`crate::SequenceInput::groove_link_z_mm`]): `start_plane +
//!   stepover_retract_mm` when the start plane is the stock top or an
//!   earlier floor cleared it at the start depth, else no lower than the
//!   stock top plus [`ENTRY_CLEARANCE_MM`] (security review L1). The
//!   first approach, the step between the families and the last retract
//!   are at safe Z.
//! - **Limits**: the depth is never past the bit's cone, counted from the
//!   stock top unless a floor at the start depth cleared it (security
//!   review H4); the start plane is cleared or vouched for (M2); the cut
//!   stays above the stock bottom less the allowance (S1); the grooves
//!   hold at most [`MAX_PHOTO_MOVES`] samples, counted before anything is
//!   allocated (L3); and every groove, widened at each point by its
//!   width at the stock top, stays on the stock (S7).
//!
//! [`Bitmap::pixels`] and [`sample`] are the only readers of pixel data.

use crate::block::CutBlock;
use crate::entry::{ENTRY_CLEARANCE_MM, plunge_limited_feed_mm_min};
use crate::error::{CamError, PhotoProblem, ShapeRef};
use crate::profile::rates_of;
use crate::sequence::{SequenceInput, clearance_z_mm, sequence};
use crate::stock::stock_overhang;
use crate::toolpath::{Move, MoveKind, Toolpath};
use crate::vcarve::checked_v_bit;
use crate::warning::Warning;
use dc_geometry::{EPS_MM, Point, Point3, Rect, Vec2};
use dc_project::{
    Bitmap, BitmapError, Grayscale, InstanceId, LoadError, MachiningOrder, Operation, OperationId,
    PhotoStrategy, PhotoVCarveOp, Project, cleared_start_plane,
};

/// The most samples a Photo V-Carve's grooves may hold, both families
/// together (security review L3): two million straight moves, about 60 MB
/// of toolpath. Counted before anything is allocated, as `MAX_RINGS`
/// bounds a Pocket. The design's example, a 100 × 50 mm picture at 30°,
/// 2 mm and 1.1 sampled every 0.25 mm, takes about 20 000.
pub const MAX_PHOTO_MOVES: usize = 2_000_000;

/// Two samples of a groove closer than this, mm, are one: its last regular
/// sample is moved onto its end rather than kept beside it.
const MIN_SAMPLE_GAP_MM: f64 = 1e-3;

/// One family of grooves ([`grooves`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Grooves {
    /// The family's direction, `(cos θ, sin θ)`; each groove runs along it
    /// or against it.
    pub direction: Vec2,
    /// The distance between neighbouring grooves, mm, measured across
    /// them.
    pub spacing_mm: f64,
    /// The grooves in cut order, each its sample points in cut order,
    /// project space, inside the footprint: every `sample_mm` from its
    /// start and its end. A groove that only touches the footprint at a
    /// corner is one point.
    pub lines: Vec<Vec<Point>>,
}

/// What `check`, `export` and the editor's status line say of a Photo
/// V-Carve, read from the project without sampling the picture.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layout {
    /// The picture's footprint, project space.
    pub footprint: Rect,
    /// How many grooves cut it, both families of a hatch.
    pub grooves: usize,
    /// A groove's width at the full depth, mm: `2 · max_depth_mm · tan(α/2)`.
    pub width_mm: f64,
}

/// The footprint rectangle of `bitmap`, project space: `x_mm` to `x_mm +
/// width_mm`, `y_mm` to `y_mm + height_mm`.
#[must_use]
pub fn footprint(bitmap: &Bitmap) -> Rect {
    Rect {
        min: Point::new(bitmap.x_mm, bitmap.y_mm),
        max: Point::new(
            bitmap.x_mm + bitmap.width_mm,
            bitmap.y_mm + bitmap.height_mm,
        ),
    }
}

/// The family of grooves at `angle_deg` (any finite angle; `θ + 90°` for a
/// hatch's second) and `spacing_mm` apart through the top-left corner of
/// `footprint`, clipped to it and sampled every `sample_mm` (module docs),
/// for operation `op`.
///
/// # Errors
///
/// [`CamError::PhotoSetting`] for a spacing or step that is not a positive
/// number, or that would hold more than [`MAX_PHOTO_MOVES`] samples
/// ([`PhotoProblem::TooManyMoves`], field `spacing_mm` or `sample_mm`),
/// found before anything is allocated.
pub fn grooves(
    op: &OperationId,
    footprint: Rect,
    angle_deg: f64,
    spacing_mm: f64,
    sample_mm: f64,
) -> Result<Grooves, CamError> {
    let setting = |field: &'static str, value: f64, problem: PhotoProblem| CamError::PhotoSetting {
        op: op.clone(),
        field,
        value: Some(value),
        problem,
    };
    let above_zero = PhotoProblem::OutOfRange { range: "above 0" };
    if !(spacing_mm.is_finite() && spacing_mm > 0.0) {
        return Err(setting("spacing_mm", spacing_mm, above_zero));
    }
    if !(sample_mm.is_finite() && sample_mm > 0.0) {
        return Err(setting("sample_mm", sample_mm, above_zero));
    }
    let family = Family::new(footprint, angle_deg, spacing_mm)
        .ok_or_else(|| setting("spacing_mm", spacing_mm, above_zero))?;
    match family.budget(sample_mm) {
        Budget::Within(_) => Ok(family.sampled(sample_mm)),
        Budget::TooManyLines(moves) => Err(setting("spacing_mm", spacing_mm, too_many(moves))),
        Budget::TooManySamples(moves) => Err(setting("sample_mm", sample_mm, too_many(moves))),
    }
}

/// [`PhotoProblem::TooManyMoves`] for `moves`.
const fn too_many(moves: f64) -> PhotoProblem {
    PhotoProblem::TooManyMoves {
        moves,
        limit: MAX_PHOTO_MOVES,
    }
}

/// The brightness of `bitmap`'s picture `pixels` at project point `p`,
/// 0 (black) to 255 (white): bilinear between the four nearest pixel
/// centres, clamped to the edge pixels outside them (and so outside the
/// footprint). Row 0 is the picture's top row, at the highest Y. Indices
/// come from `f64::floor` clamped into the picture and are read with
/// `get`, never from subtraction on a `u32` (security review M4); a point
/// that is not a number reads the top-left pixel, and a picture with no
/// pixels reads white.
#[must_use]
pub fn sample(bitmap: &Bitmap, pixels: &Grayscale, p: Point) -> f64 {
    let (width_px, height_px) = (pixels.width_px, pixels.height_px);
    if width_px == 0 || height_px == 0 {
        return 255.0;
    }
    let pixel_w = bitmap.width_mm / f64::from(width_px);
    let pixel_h = bitmap.height_mm / f64::from(height_px);
    let (i0, i1, tx) = axis((p.x - bitmap.x_mm) / pixel_w - 0.5, width_px);
    let (j0, j1, ty) = axis(
        (bitmap.y_mm + bitmap.height_mm - p.y) / pixel_h - 0.5,
        height_px,
    );
    let width = width_px as usize;
    let at = |i: usize, j: usize| {
        j.checked_mul(width)
            .and_then(|row| row.checked_add(i))
            .and_then(|k| pixels.bytes.get(k))
            .map_or(255.0, |&g| f64::from(g))
    };
    let top = at(i0, j0) * (1.0 - tx) + at(i1, j0) * tx;
    let bottom = at(i0, j1) * (1.0 - tx) + at(i1, j1) * tx;
    top * (1.0 - ty) + bottom * ty
}

/// The two pixel indices either side of `f`, a position in pixel centres
/// along an axis of `n` pixels, and how far `f` is from the first to the
/// second, 0 to 1: clamped to the edge pixels, and to the first for a
/// position that is not a number.
#[allow(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "the value is clamped to 0..n − 1 and floored first"
)]
fn axis(f: f64, n: u32) -> (usize, usize, f64) {
    let last = f64::from(n.saturating_sub(1));
    let f = if f.is_nan() { 0.0 } else { f.clamp(0.0, last) };
    let floor = f.floor();
    let i0 = floor as usize;
    let i1 = if floor < last { i0 + 1 } else { i0 };
    (i0, i1, f - floor)
}

/// What [`Layout`] says of Photo V-Carve `op` of `project`; `None` when
/// its bitmap, its tool's angle, its maximum depth, its line angle or its
/// spacing does not resolve, or its grooves are past [`MAX_PHOTO_MOVES`].
#[must_use]
pub fn layout(project: &Project, op: &PhotoVCarveOp) -> Option<Layout> {
    let bitmap = project.bitmaps.iter().find(|b| b.id == op.image)?;
    let tool = project.tools.iter().find(|t| t.id == op.tool)?;
    let slope = checked_v_bit(&op.id, tool).ok()?.slope;
    let max_depth_mm = op.max_depth_mm.filter(|d| d.is_finite() && *d > 0.0)?;
    let width_mm = 2.0 * max_depth_mm * slope;
    let spacing_mm = op.line_spacing_fraction * width_mm;
    let footprint = footprint(bitmap);
    let grooves = angles(op)
        .iter()
        .map(|&angle| {
            Family::new(footprint, angle, spacing_mm)
                .filter(|f| f.line_count() <= MAX_PHOTO_MOVES as f64)
                .map(|f| f.lines().count())
        })
        .sum::<Option<usize>>()?;
    Some(Layout {
        footprint,
        grooves,
        width_mm,
    })
}

/// The families' angles, degrees: `θ`, then `θ + 90°` for a hatch.
fn angles(op: &PhotoVCarveOp) -> Vec<f64> {
    match op.strategy {
        PhotoStrategy::Raster => vec![op.line_angle_deg],
        PhotoStrategy::Hatch => vec![op.line_angle_deg, op.line_angle_deg + 90.0],
    }
}

/// Generates the toolpath of Photo V-Carve `op` of `project` (module
/// docs): one run, its own, on the stock, which the caller tags
/// [`crate::RunLabel::Main`]. Warns [`Warning::PhotoLinesOverlap`] for a
/// spacing below 1.
///
/// # Errors
///
/// Every setting validation checks is checked again: an unknown tool, a
/// tool that is not a usable V-bit ([`CamError::VCarveNeedsVBit`],
/// [`CamError::VBitAngleRange`]), a missing rate, a safe Z not above 0,
/// an unset or out-of-range setting ([`CamError::PhotoSetting`]), an
/// unknown bitmap ([`CamError::PhotoBitmapUnknown`]), no stock
/// thickness, a start depth not from 0 to the thickness, an uncleared
/// start plane ([`CamError::StartDepthUncleared`]), flanks engaged past
/// the cone ([`CamError::VBitConeExceeded`]), a cut below the stock
/// bottom less the allowance ([`CamError::BelowMaterialBottom`]). Then
/// grooves past [`MAX_PHOTO_MOVES`] ([`CamError::PhotoSetting`]), a
/// picture that does not decode ([`CamError::BitmapDecode`]), and grooves
/// whose width takes the bit off the stock ([`CamError::OffStock`]).
pub fn generate_photo(
    project: &Project,
    op: &PhotoVCarveOp,
) -> Result<(Toolpath, Vec<Warning>), CamError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: op.tool.clone(),
        })?;
    let bit = checked_v_bit(&op.id, tool)?;
    let rates = rates_of(tool)?;
    if !(op.safe_z_mm.is_finite() && op.safe_z_mm > 0.0) {
        return Err(CamError::InvalidSafeZ {
            op: op.id.clone(),
            safe_z_mm: op.safe_z_mm,
        });
    }
    let max_depth_mm = checked_settings(op)?;
    let bitmap = project
        .bitmaps
        .iter()
        .find(|b| b.id == op.image)
        .ok_or_else(|| CamError::PhotoBitmapUnknown {
            op: op.id.clone(),
            bitmap: op.image.clone(),
        })?;
    let shape = ShapeRef {
        instance: InstanceId::new(bitmap.id.as_str()),
        instance_name: bitmap.name.clone(),
        profile: 0,
    };
    let plane = Plane::of(project, op, max_depth_mm)?;
    let engaged_mm = plane.engaged_from_z_mm - (plane.start_plane_z_mm - max_depth_mm);
    if engaged_mm > bit.cone_mm + EPS_MM {
        return Err(CamError::VBitConeExceeded {
            op: op.id.clone(),
            shape: Box::new(shape),
            depth_mm: engaged_mm,
            cone_mm: bit.cone_mm,
        });
    }

    // L3: the samples are counted, every family, before any is made.
    let width_mm = 2.0 * max_depth_mm * bit.slope;
    let spacing_mm = op.line_spacing_fraction * width_mm;
    let footprint = footprint(bitmap);
    let families: Vec<Family> = angles(op)
        .iter()
        .map(|&angle| Family::new(footprint, angle, spacing_mm))
        .collect::<Option<_>>()
        .ok_or_else(|| photo_setting(op, "line_spacing_fraction", op.line_spacing_fraction))?;
    let mut samples = 0.0;
    for family in &families {
        samples += match family.budget(op.sample_mm) {
            Budget::Within(n) => n,
            Budget::TooManyLines(moves) => {
                return Err(CamError::PhotoSetting {
                    op: op.id.clone(),
                    field: "line_spacing_fraction",
                    value: Some(op.line_spacing_fraction),
                    problem: too_many(moves),
                });
            }
            Budget::TooManySamples(moves) => moves,
        };
    }
    if samples > MAX_PHOTO_MOVES as f64 {
        return Err(CamError::PhotoSetting {
            op: op.id.clone(),
            field: "sample_mm",
            value: Some(op.sample_mm),
            problem: too_many(samples),
        });
    }

    let pixels = bitmap.pixels().map_err(|e| match e {
        LoadError::Bitmap { id, reason } => CamError::BitmapDecode { bitmap: id, reason },
        other => CamError::BitmapDecode {
            bitmap: bitmap.id.clone(),
            reason: BitmapError::Png {
                message: other.to_string(),
            },
        },
    })?;
    let depth_of = |p: Point| {
        let g = sample(bitmap, &pixels, p) / 255.0;
        let dark = if op.invert { g } else { 1.0 - g };
        dark.clamp(0.0, 1.0) * max_depth_mm
    };
    let thickness_mm = plane.thickness_mm;
    let safe_z_mm = thickness_mm + op.safe_z_mm;
    let mut blocks = Vec::new();
    for (group, family) in families.iter().enumerate() {
        for line in family.sampled(op.sample_mm).lines {
            let points: Vec<Point3> = line
                .iter()
                .map(|&p| Point3::new(p.x, p.y, plane.start_plane_z_mm - depth_of(p)))
                .collect();
            if let Some(block) = groove_block(&shape, group, &merged(&points), safe_z_mm, rates) {
                blocks.push(block);
            }
        }
    }
    let input = SequenceInput {
        blocks,
        order: MachiningOrder::FullDepthFirst,
        containment: &[],
        safe_z_mm,
        clearance_z_mm: clearance_z_mm(project, thickness_mm, safe_z_mm),
        stock_top_mm: thickness_mm,
        tool_diameter_mm: tool.diameter_mm,
        stepover_mm: 0.0,
        feed_mm_min: rates.feed_mm_min,
        plunge_mm_min: rates.plunge_mm_min,
        cut_before: &[],
        groove_link_z_mm: Some(plane.link_z_mm),
    };
    let toolpath = sequence(&input)?;
    // S7: each groove, as wide as it is at the stock top, on the stock.
    if let Some(overhang) = groove_bounds(&toolpath, thickness_mm, bit.slope, tool.diameter_mm)
        .and_then(|swept| stock_overhang(&project.stock, swept))
    {
        return Err(CamError::OffStock {
            op: op.id.clone(),
            overhang,
        });
    }
    let mut warnings = Vec::new();
    if op.line_spacing_fraction < 1.0 {
        warnings.push(Warning::PhotoLinesOverlap {
            op: op.id.clone(),
            fraction: op.line_spacing_fraction,
        });
    }
    Ok((toolpath, warnings))
}

/// [`CamError::PhotoSetting`] for `field` of `op` at `value`, out of its
/// range above 0.
fn photo_setting(op: &PhotoVCarveOp, field: &'static str, value: f64) -> CamError {
    CamError::PhotoSetting {
        op: op.id.clone(),
        field,
        value: Some(value),
        problem: PhotoProblem::OutOfRange { range: "above 0" },
    }
}

/// The settings of `op` validation checks, checked again: its maximum
/// depth, returned, set and above 0; its line angle from 0° to below
/// 180°; its spacing, sample step and step-over retract above 0.
fn checked_settings(op: &PhotoVCarveOp) -> Result<f64, CamError> {
    let Some(max_depth_mm) = op.max_depth_mm else {
        return Err(CamError::PhotoSetting {
            op: op.id.clone(),
            field: "max_depth_mm",
            value: None,
            problem: PhotoProblem::Unset,
        });
    };
    let positive = |v: f64| v.is_finite() && v > 0.0;
    if !positive(max_depth_mm) {
        return Err(photo_setting(op, "max_depth_mm", max_depth_mm));
    }
    if !(op.line_angle_deg.is_finite() && (0.0..180.0).contains(&op.line_angle_deg)) {
        return Err(CamError::PhotoSetting {
            op: op.id.clone(),
            field: "line_angle_deg",
            value: Some(op.line_angle_deg),
            problem: PhotoProblem::OutOfRange {
                range: "from 0 up to, not including, 180",
            },
        });
    }
    for (field, value) in [
        ("line_spacing_fraction", op.line_spacing_fraction),
        ("sample_mm", op.sample_mm),
        ("stepover_retract_mm", op.stepover_retract_mm),
    ] {
        if !positive(value) {
            return Err(photo_setting(op, field, value));
        }
    }
    Ok(max_depth_mm)
}

/// The planes a Photo V-Carve cuts from, project space, mm.
struct Plane {
    /// The stock top.
    thickness_mm: f64,
    /// The start plane, `start_depth_mm` below the stock top.
    start_plane_z_mm: f64,
    /// Where the flanks are engaged from: the start plane when it is the
    /// stock top or a floor at the start depth cleared it, else the stock
    /// top (security review H4).
    engaged_from_z_mm: f64,
    /// Where the bit crosses between grooves (security review L1).
    link_z_mm: f64,
}

impl Plane {
    /// The planes of `op` in `project`, cutting `max_depth_mm` deep at
    /// most.
    fn of(project: &Project, op: &PhotoVCarveOp, max_depth_mm: f64) -> Result<Self, CamError> {
        let thickness_mm = project
            .stock
            .thickness_mm
            .ok_or(CamError::MissingThickness)?;
        let start_depth_mm = op.start_depth_mm;
        if !(start_depth_mm.is_finite() && start_depth_mm >= 0.0 && start_depth_mm <= thickness_mm)
        {
            return Err(CamError::InvalidStartDepth {
                op: op.id.clone(),
                start_depth_mm,
            });
        }
        let start_plane_z_mm = thickness_mm - start_depth_mm;
        let cleared_at_start = start_depth_mm <= 0.0 || {
            let cleared = cleared_start_plane(project, &Operation::PhotoVCarve(op.clone()));
            if cleared.is_none() && !op.start_plane_cleared_externally {
                return Err(CamError::StartDepthUncleared {
                    op: op.id.clone(),
                    start_depth_mm,
                });
            }
            cleared.is_some_and(|plane| plane.is_at(start_depth_mm))
        };
        let depth_mm = start_depth_mm + max_depth_mm;
        let limit_mm = thickness_mm + op.through_cut_allowance_mm;
        if !(depth_mm <= limit_mm + EPS_MM) {
            return Err(CamError::BelowMaterialBottom {
                op: op.id.clone(),
                depth_mm,
                limit_mm,
            });
        }
        let over_plane_mm = start_plane_z_mm + op.stepover_retract_mm;
        Ok(Self {
            thickness_mm,
            start_plane_z_mm,
            engaged_from_z_mm: if cleared_at_start {
                start_plane_z_mm
            } else {
                thickness_mm
            },
            // L1: over material the project cannot see, never lower than
            // the stock top plus the entry clearance.
            link_z_mm: if cleared_at_start {
                over_plane_mm
            } else {
                over_plane_mm.max(thickness_mm + ENTRY_CLEARANCE_MM)
            },
        })
    }
}

/// One family of grooves before it is sampled: the lines `c + k·s·n +
/// t·u` (module docs).
struct Family {
    /// The footprint the lines are clipped to.
    footprint: Rect,
    /// The top-left corner every line's offset counts from.
    corner: Point,
    /// `(cos θ, sin θ)`.
    u: Vec2,
    /// `(−sin θ, cos θ)`.
    n: Vec2,
    /// The spacing, mm.
    spacing_mm: f64,
    /// The largest and smallest `k` whose line meets the footprint.
    k_high: i64,
    k_low: i64,
}

/// How many samples a family would hold ([`Family::budget`]).
enum Budget {
    /// At most this many, within [`MAX_PHOTO_MOVES`].
    Within(f64),
    /// More lines than [`MAX_PHOTO_MOVES`]: at least this many moves.
    TooManyLines(f64),
    /// More samples than [`MAX_PHOTO_MOVES`]: at least this many.
    TooManySamples(f64),
}

impl Family {
    /// The family at `angle_deg`, `spacing_mm` apart, over `footprint`;
    /// `None` when a number is not finite or the spacing not above 0.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "both ends are within MAX_PHOTO_MOVES spacings of 0, checked first"
    )]
    fn new(footprint: Rect, angle_deg: f64, spacing_mm: f64) -> Option<Self> {
        let theta = angle_deg.to_radians();
        let usable = theta.is_finite()
            && spacing_mm.is_finite()
            && spacing_mm > 0.0
            && [footprint.min, footprint.max]
                .iter()
                .all(|p| p.x.is_finite() && p.y.is_finite());
        if !usable {
            return None;
        }
        let u = Vec2::new(theta.cos(), theta.sin());
        let n = Vec2::new(-theta.sin(), theta.cos());
        let corner = Point::new(footprint.min.x, footprint.max.y);
        let offsets = [
            footprint.min,
            footprint.max,
            Point::new(footprint.min.x, footprint.max.y),
            Point::new(footprint.max.x, footprint.min.y),
        ]
        .map(|p| n.dot(p - corner));
        let high = offsets.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let low = offsets.iter().copied().fold(f64::INFINITY, f64::min);
        // A line within EPS_MM of the footprint counts as touching it.
        let (k_high, k_low) = (
            ((high + EPS_MM) / spacing_mm).floor(),
            ((low - EPS_MM) / spacing_mm).ceil(),
        );
        let limit = MAX_PHOTO_MOVES as f64;
        let k_high = k_high.clamp(-limit - 1.0, limit + 1.0) as i64;
        let k_low = k_low.clamp(-limit - 1.0, limit + 1.0) as i64;
        Some(Self {
            footprint,
            corner,
            u,
            n,
            spacing_mm,
            k_high,
            k_low,
        })
    }

    /// How many lines `k` the family spans, at most.
    fn line_count(&self) -> f64 {
        (self.k_high - self.k_low + 1).max(0) as f64
    }

    /// The family's lines that meet the footprint, in cut order: each its
    /// `k` and its clipped span `t0 ≤ t1` along `u`.
    fn lines(&self) -> impl Iterator<Item = (i64, f64, f64)> + '_ {
        (self.k_low..=self.k_high)
            .rev()
            .filter_map(|k| self.clip(k).map(|(t0, t1)| (k, t0, t1)))
    }

    /// The point at `t` along line `k`, clamped into the footprint.
    fn at(&self, k: i64, t: f64) -> Point {
        let offset = k as f64 * self.spacing_mm;
        let p = self.corner + self.n * offset + self.u * t;
        let Rect { min, max } = self.footprint;
        Point::new(p.x.clamp(min.x, max.x), p.y.clamp(min.y, max.y))
    }

    /// Line `k` clipped to the footprint: its span along `u`, `None` when
    /// it misses it (a line within [`EPS_MM`] of a corner touches it, a
    /// span of one point).
    fn clip(&self, k: i64) -> Option<(f64, f64)> {
        let base = self.corner + self.n * (k as f64 * self.spacing_mm);
        let Rect { min, max } = self.footprint;
        let mut t0 = f64::NEG_INFINITY;
        let mut t1 = f64::INFINITY;
        for (b, d, lo, hi) in [
            (base.x, self.u.x, min.x, max.x),
            (base.y, self.u.y, min.y, max.y),
        ] {
            if d.abs() <= 1e-12 {
                if b < lo - EPS_MM || b > hi + EPS_MM {
                    return None;
                }
                continue;
            }
            let (a, c) = ((lo - b) / d, (hi - b) / d);
            t0 = t0.max(a.min(c));
            t1 = t1.min(a.max(c));
        }
        if !(t0.is_finite() && t1.is_finite()) || t1 < t0 - EPS_MM {
            return None;
        }
        Some((t0, t1.max(t0)))
    }

    /// How many samples the family holds at `sample_mm`, counted from the
    /// lines' lengths alone (L3).
    fn budget(&self, sample_mm: f64) -> Budget {
        let limit = MAX_PHOTO_MOVES as f64;
        let lines = self.line_count();
        if lines > limit {
            return Budget::TooManyLines(lines);
        }
        let mut samples = 0.0;
        for (_, t0, t1) in self.lines() {
            samples += ((t1 - t0) / sample_mm).floor() + 2.0;
            if samples > limit {
                return Budget::TooManySamples(samples);
            }
        }
        Budget::Within(samples)
    }

    /// The family's grooves sampled every `sample_mm`, in cut order.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "the step count is below MAX_PHOTO_MOVES, which `budget` checked"
    )]
    fn sampled(&self, sample_mm: f64) -> Grooves {
        let lines = self
            .lines()
            .map(|(k, t0, t1)| {
                let steps = ((t1 - t0) / sample_mm).floor().max(0.0) as usize;
                let mut ts: Vec<f64> = (0..=steps).map(|i| t0 + i as f64 * sample_mm).collect();
                // The end, on its own or in place of a sample beside it.
                match ts.last_mut() {
                    Some(last) if t1 - *last <= MIN_SAMPLE_GAP_MM => *last = t1,
                    _ => ts.push(t1),
                }
                if ts.len() == 2 && t1 - t0 <= MIN_SAMPLE_GAP_MM {
                    ts.truncate(1);
                }
                let mut points: Vec<Point> = ts.into_iter().map(|t| self.at(k, t)).collect();
                if k.rem_euclid(2) == 1 {
                    points.reverse();
                }
                points
            })
            .collect();
        Grooves {
            direction: self.u,
            spacing_mm: self.spacing_mm,
            lines,
        }
    }
}

/// `points`, along one straight line in XY in order, with every point
/// that lies within [`EPS_MM`] in Z of the straight move between its
/// neighbours left out: a flat or evenly sloping stretch is one move. Each
/// candidate move is held to every point it would replace.
fn merged(points: &[Point3]) -> Vec<Point3> {
    let Some((&first, rest)) = points.split_first() else {
        return Vec::new();
    };
    let mut out = vec![first];
    let mut anchor = first;
    // The slopes, dZ per mm along the line from `anchor`, that keep every
    // point since it within EPS_MM.
    let (mut low, mut high) = (f64::NEG_INFINITY, f64::INFINITY);
    let mut pending: Option<Point3> = None;
    for &p in rest {
        let mut along = anchor.xy().distance_to(p.xy());
        if let Some(q) = pending {
            let slope = (p.z - anchor.z) / along;
            if !(slope >= low && slope <= high) {
                out.push(q);
                anchor = q;
                (low, high) = (f64::NEG_INFINITY, f64::INFINITY);
                along = anchor.xy().distance_to(p.xy());
            }
        }
        if along <= 0.0 {
            continue;
        }
        low = low.max((p.z - EPS_MM - anchor.z) / along);
        high = high.min((p.z + EPS_MM - anchor.z) / along);
        pending = Some(p);
    }
    out.extend(pending);
    out
}

/// The cut block of one groove, `points` its samples in cut order: a
/// straight plunge at the plunge rate to the first, then straight cuts at
/// the feed rate, each slowed so it falls no faster than the plunge rate
/// (S10). `None` for a groove with no point.
fn groove_block(
    shape: &ShapeRef,
    group: usize,
    points: &[Point3],
    safe_z_mm: f64,
    rates: crate::tabs::LapRates,
) -> Option<CutBlock> {
    let (&first, rest) = points.split_first()?;
    let mut from = first;
    let cuts: Vec<Move> = rest
        .iter()
        .map(|&to| {
            let feed = plunge_limited_feed_mm_min(rates.feed_mm_min, rates.plunge_mm_min, from, to);
            from = to;
            Move::linear(to, feed)
        })
        .collect();
    let lowest_mm = points.iter().map(|p| p.z).fold(f64::INFINITY, f64::min);
    Some(CutBlock {
        shape: shape.clone(),
        pass: 0,
        z_mm: lowest_mm,
        descent_from: first.xy(),
        descent_from_z_mm: safe_z_mm,
        descent: Vec::new(),
        entry: first.xy(),
        entry_z_mm: first.z,
        cuts,
        exit: from.xy(),
        frees: false,
        cleared: None,
        group,
    })
}

/// The XY bounds of `toolpath`'s cuts, each end grown by the groove's
/// half-width at the stock top there: `(thickness − z) · slope`, from 0
/// above the stock to the tool's radius (S7 as it reads a V-bit's
/// groove). `None` when nothing is cut.
fn groove_bounds(
    toolpath: &Toolpath,
    thickness_mm: f64,
    slope: f64,
    diameter_mm: f64,
) -> Option<Rect> {
    let radius = |z: f64| ((thickness_mm - z) * slope).clamp(0.0, diameter_mm / 2.0);
    let mut bounds: Option<Rect> = None;
    let mut include = |p: Point3| {
        let r = radius(p.z);
        for corner in [Point::new(p.x - r, p.y - r), Point::new(p.x + r, p.y + r)] {
            bounds = Some(bounds.map_or_else(|| Rect::from_point(corner), |b| b.including(corner)));
        }
    };
    let mut previous: Option<Point3> = None;
    for mv in &toolpath.moves {
        if mv.kind == MoveKind::Linear {
            if let Some(start) = previous {
                include(start);
            }
            include(mv.to);
        }
        previous = Some(mv.to);
    }
    bounds
}

#[cfg(test)]
mod tests;
