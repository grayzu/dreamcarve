// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Inlay (`docs/specs/phase-3/design.md`, "Algorithms → Inlay
//! (dc-cam::inlay)"; functional capability 45, acceptance 56 and 57). No
//! PureCutCNC reference: the method is Corbin's Workshop's V-bit inlay
//! (cited in the functional spec), which VCarve's "V-Carve Inlay" also
//! follows.
//!
//! One Inlay cuts two boards: the female on the project's stock,
//! and the male on the Inlay's male board ([`Project::on_male_board`]),
//! which shares the stock's bottom-left corner and has its own thickness;
//! [`generate_inlay`] gives both boards' runs, each tagged with its
//! [`Board`], and [`generate_inlay_board`] one. Every rule below that
//! reads "the stock" for the male reads the male board: the band's
//! clipping and `InlayTooNearEdge`, the stock top its flank is engaged
//! from, its floor and cut-out depths, and the plug base. Until the male
//! board's thickness is set, the male is not cut.
//!
//! Both sides of an inlay are a [`vcarve::Carve`] cut by the V-Carve
//! machinery ([`vcarve::blocks`], the clearing passes, `sequence` and the
//! generator's own cone test); this module only says what each side
//! carves:
//!
//! - **Female:** the design's regions from the stock top, flat at
//!   `depth_mm` `D`: the same carve as a V-Carve with `flat_depth_mm: D`
//!   and `start_depth_mm: 0`, move for move.
//! - **Male:** the design mirrored about the vertical line through the
//!   middle of its targets' joint bounding box ([`male_mirror`]), and the
//!   carve is its **surround** ([`dc_geometry::surround`]): the band
//!   `margin_mm` wide round the mirrored design, and every hole in it. Its
//!   start plane is `D − G` below the stock top (`glue_gap_mm` `G`), flat
//!   `plug_depth_mm` `P` below that, and its flank is engaged from the
//!   stock top, since the plug above the start plane is never cleared.
//!   A band that runs past the board's edge is
//!   clipped to the board ([`MaleBand::board`]), and its cut-out
//!   stops at the board's edge.
//!
//! On both sides, parts of the design that come closer than
//! [`INLAY_MERGE_MM`] are first merged into one shape ([`merge_near_parts`];
//! #479): no V-bit can carve the gap between them, so the female would
//! leave a wall standing in it and the plug would stand proud on it.
//!
//! The two mate: the female's pocket `h` below the stock top is the
//! design inset by `h · tan(α/2)`, and the plug `h` above its start plane
//! is the mirrored design inset by the same, for `h` from 0 to `D − G`
//! (design step 5, the fit property). The plug seats with its start plane
//! at the female's surface and its top `G` above the pocket's floor.
//!
//! **A tapered ball nose** (design spec, "Inlay fit")
//! cuts both sides on its profile: the walls mate wherever both are cut by
//! its cone. Its design, merged, is rounded for the bit on both boards
//! ([`dc_project::rounded_for_bit`]: opened by `R_min = (D − G)·tan β +
//! w_t`, closed by `w_t`), warning [`Warning::InlayRoundedForBit`], and a
//! part or gap the rounding would remove, split or join is
//! [`CamError::InlayTooNarrowForBit`]. A glue gap or plug depth below the
//! bit's tangent height warns [`Warning::InlayGlueGapBelowBall`] or
//! [`Warning::InlayPlugDepthBelowBall`]; the band reads the bit's profile
//! ([`dc_project::male_band`]), and the male's engagement is held to the
//! bit's cutting height.

#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::ops::Range;

use crate::context::{self, check_diameter, check_safe_z, rates_of, tool_of};
use crate::error::{CamError, ShapeRef, at};
use crate::placed_tabs;
use crate::profile::generate_profile_below;
use crate::run::{RunLabel, ToolRun};
use crate::stock::StockEdge;
use crate::targets::target_profiles;
use crate::vcarve::{
    self, Carve, Clearing, FloorRidges, OpenEdge, Targets, carve_regions, carve_regions_then,
    checked_bit, clearing, floor::Floor,
};
use crate::warning::Warning;
use dc_geometry::profile::BitProfile;
use dc_geometry::{
    Contour, EPS_MM, Point, Polyline, Rect, Region, Transform, band_on_board, difference_within,
    inset_outers, merge_near_parts, nearest_point, rounded_band, surround_within,
    wall_tolerance_mm, without_rect_sides,
};
use dc_project::{
    BandEdge, BandSource, Board, CornerRelief, CutDirection, DefinitionId, Entry,
    FeatureDefinition, FeatureInstance, INLAY_MERGE_MM, INLAY_MIN_PLUG_BASE_MM, InlayCutOut,
    InlayOp, InstanceId, MaleBand, Operation, OperationId, ProfileOp, Project, Side as ProfileSide,
    StartAt, VCarveStrategy, band_edge_of, male_band,
};

/// Both sides carve along the medial lines; an Inlay has no strategy of
/// its own.
const MEDIAL: VCarveStrategy = VCarveStrategy::Medial;

/// The male's mirror (design "Inlay" step 4): X reflected about the centre
/// line of the joint bounding box of `targets`' closed outlines, placed in
/// project space, arc extremes included; `[−1, 0, 0, 1, 2·cx, 0]`. The
/// safety pass derives the same line from the project. With no closed
/// outline there is nothing to mirror, and it is the identity.
///
/// # Errors
///
/// [`CamError::UnknownInstance`], [`CamError::UnknownDefinition`], or a
/// placement that fails ([`CamError::Geometry`]).
pub fn male_mirror(project: &Project, targets: &[InstanceId]) -> Result<Transform, CamError> {
    Ok(axis_x_mm(project, &OperationId::new(""), targets)?
        .map_or(Transform::IDENTITY, Transform::mirror_x_about))
}

/// The X of [`male_mirror`]'s line for operation `op`, mm; `None` with no
/// closed outline.
fn axis_x_mm(
    project: &Project,
    op: &OperationId,
    targets: &[InstanceId],
) -> Result<Option<f64>, CamError> {
    let mut bounds: Option<Rect> = None;
    for target in target_profiles(project, op, targets) {
        let target = target?;
        if !target.is_closed() {
            continue;
        }
        if let Some(b) = target.transformed()?.bounds() {
            bounds = Some(match bounds {
                Some(all) => all.including(b.min).including(b.max),
                None => b,
            });
        }
    }
    Ok(bounds.map(|b| (b.min.x + b.max.x) / 2.0))
}

/// An Inlay's four values, checked (design step 1).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Values {
    /// `depth_mm`, `D`, above 0.
    depth_mm: f64,
    /// `glue_gap_mm`, `G`, from 0 to below `D`.
    glue_gap_mm: f64,
    /// `plug_depth_mm`, `P`, above 0.
    plug_depth_mm: f64,
    /// `margin_mm`, `M`, above 0 on a male; `None` on a female, which
    /// ignores it.
    margin_mm: Option<f64>,
}

/// `margin_mm`, which must be a positive finite number.
fn positive_margin(op: &InlayOp, margin_mm: f64) -> Result<f64, CamError> {
    if margin_mm.is_finite() && margin_mm > 0.0 {
        Ok(margin_mm)
    } else {
        Err(CamError::InlayMargin {
            op: op.id.clone(),
            margin_mm,
        })
    }
}

impl Values {
    /// The values of `op` for `board`, as validation checks them: the
    /// margin only for the male.
    fn of(project: &Project, op: &InlayOp, board: Board) -> Result<Self, CamError> {
        let unset = |field| CamError::InlayValueUnset {
            op: op.id.clone(),
            field,
        };
        let positive = |field, value_mm: f64| {
            if value_mm.is_finite() && value_mm > 0.0 {
                Ok(value_mm)
            } else {
                Err(CamError::InlayValueRange {
                    op: op.id.clone(),
                    field,
                    value_mm,
                })
            }
        };
        let depth_mm = positive("depth_mm", op.depth_mm.ok_or_else(|| unset("depth_mm"))?)?;
        let glue_gap_mm = op.glue_gap_mm.ok_or_else(|| unset("glue_gap_mm"))?;
        let plug_depth_mm = positive(
            "plug_depth_mm",
            op.plug_depth_mm.ok_or_else(|| unset("plug_depth_mm"))?,
        )?;
        if !(glue_gap_mm.is_finite() && glue_gap_mm >= 0.0 && glue_gap_mm < depth_mm) {
            return Err(CamError::InlayGlueGap {
                op: op.id.clone(),
                glue_gap_mm,
                depth_mm,
            });
        }
        let margin_mm = match board {
            Board::Female => None,
            // The drawn boundary replaces `margin_mm`; the narrowest
            // band it leaves takes its place in every rule.
            Board::Male if op.has_boundary() => {
                let drawn = dc_project::drawn_boundary(project, op)
                    .map_err(|problem| CamError::InlayBoundary {
                        op: op.id.clone(),
                        problem,
                    })?
                    .ok_or_else(|| unset("margin_mm"))?;
                // A boundary round a design with no outline has no
                // narrowest band to stand for the margin.
                let narrowest = drawn.narrowest.ok_or_else(|| unset("margin_mm"))?;
                Some(positive_margin(op, narrowest.width_mm)?)
            }
            Board::Male => {
                let margin_mm = op.margin_mm.ok_or_else(|| unset("margin_mm"))?;
                Some(positive_margin(op, margin_mm)?)
            }
        };
        // As validation's `inlay_outline_rounding`, on either side.
        if let Some(rounding_mm) = op.outline_rounding_mm {
            positive("outline_rounding_mm", rounding_mm)?;
        }
        Ok(Self {
            depth_mm,
            glue_gap_mm,
            plug_depth_mm,
            margin_mm,
        })
    }

    /// The start plane's depth below the stock top, mm: 0 for the female,
    /// `D − G` for the male.
    fn start_depth_mm(self) -> f64 {
        match self.margin_mm {
            None => 0.0,
            Some(_) => self.depth_mm - self.glue_gap_mm,
        }
    }

    /// The flat depth below the start plane, mm: `D` for the female, `P`
    /// for the male.
    fn flat_depth_mm(self) -> f64 {
        match self.margin_mm {
            None => self.depth_mm,
            Some(_) => self.plug_depth_mm,
        }
    }

    /// How deep the flank engages from the stock top, mm: the start depth
    /// plus the flat depth, `D` or `D − G + P` ([`InlayOp::engagement_mm`]).
    fn engagement_mm(self) -> f64 {
        self.start_depth_mm() + self.flat_depth_mm()
    }
}

/// One side of an Inlay resolved from the project: what it carves, and
/// from where.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Side {
    /// The carved regions, their shapes, and the containment pairs for
    /// `sequence`: the design for the female, the mirrored design's
    /// surround for the male.
    pub targets: Targets,
    /// The start plane, mm, project space.
    pub start_plane_z_mm: f64,
    /// The Z the flank is engaged from, mm: always the stock top.
    pub engaged_from_z_mm: f64,
    /// The flat depth below the start plane, mm.
    pub flat_depth_mm: f64,
    /// The male's mirror line, mm; `None` for the female, or a male with
    /// no closed outline.
    pub mirror_x_mm: Option<f64>,
    /// The male's mirrored design, nested, whose outer outlines its
    /// cut-out runs round; empty for the female.
    pub design: Vec<Region>,
    /// The male's band: its edge and roundings, from
    /// [`dc_project::male_band`]; `None` for the female.
    pub band: Option<MaleBand>,
    /// The male's band edge `B` before it is clipped to the board
    /// ([`dc_project::band_edge`]), the cut-out's paths' source;
    /// empty for the female.
    pub band_edge: Vec<Region>,
    /// The male's band edge when a clearing pass cuts it upright.
    pub open_edge: Option<OpenEdge>,
    /// Where the board's edge clips the male's band; `None` when
    /// the band stays on the board, and for the female.
    pub clipped: Option<Clipped>,
}

/// Where the board's edge clips a male Inlay's band (design "Inlay"
/// step 4): the band `B` is clipped to [`MaleBand::board`], the stock's
/// rectangle with an open edge, inset by the V-bit's radius with a V
/// wall.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Clipped {
    /// The stock edges `B` runs past (inset as [`MaleBand::board`] is),
    /// each with the design's distance from the stock's edge there, mm, in
    /// [`StockEdge::ALL`]'s order.
    pub edges: Vec<(StockEdge, f64)>,
    /// The mirrored design's bounds, mm.
    pub design_bounds: Rect,
    /// How far inside the stock's edge the band is clipped, mm
    /// ([`MaleBand::board_inset_mm`]).
    pub inset_mm: f64,
}

impl Clipped {
    /// The edges `band`'s outlines run past on `board`, the design within
    /// `design_bounds`; `None` when there are none.
    fn of(
        band: &[Region],
        board: Rect,
        inset_mm: f64,
        design_bounds: Rect,
        stock: &dc_project::Stock,
    ) -> Option<Self> {
        let reach = bounds_of(band.iter().map(Region::outer))?;
        let edges: Vec<(StockEdge, f64)> = StockEdge::ALL
            .into_iter()
            .filter_map(|edge| {
                let (past, distance_mm) = match edge {
                    StockEdge::MinX => (board.min.x - reach.min.x, design_bounds.min.x),
                    StockEdge::MaxX => (
                        reach.max.x - board.max.x,
                        stock.width_mm - design_bounds.max.x,
                    ),
                    StockEdge::MinY => (board.min.y - reach.min.y, design_bounds.min.y),
                    StockEdge::MaxY => (
                        reach.max.y - board.max.y,
                        stock.height_mm - design_bounds.max.y,
                    ),
                };
                (past > EPS_MM).then_some((edge, distance_mm))
            })
            .collect();
        (!edges.is_empty()).then_some(Self {
            edges,
            design_bounds,
            inset_mm,
        })
    }

    /// The narrowest band left where it is clipped, mm: the least distance
    /// from the design to a clipping edge, less [`Self::inset_mm`].
    fn narrowest_mm(&self) -> f64 {
        self.edges
            .iter()
            .map(|(_, d)| d - self.inset_mm)
            .fold(f64::INFINITY, f64::min)
    }

    /// [`CamError::InlayTooNearEdge`] for the first clipping edge nearer
    /// the design than `needed_mm` ([`MaleBand::board_room_mm`]).
    fn check_room(&self, op: &OperationId, needed_mm: f64) -> Result<(), CamError> {
        let Some(&(edge, distance_mm)) = self.edges.iter().find(|(_, d)| d + EPS_MM < needed_mm)
        else {
            return Ok(());
        };
        let extent_mm = if edge.is_x() {
            self.design_bounds.width_mm()
        } else {
            self.design_bounds.height_mm()
        };
        Err(CamError::InlayTooNearEdge {
            op: op.clone(),
            edge,
            distance_mm,
            needed_mm,
            stock_mm: extent_mm + 2.0 * needed_mm,
        })
    }
}

/// The joint bounds of `contours`; `None` when there are none.
fn bounds_of<'c>(contours: impl Iterator<Item = &'c Contour>) -> Option<Rect> {
    contours
        .filter_map(|c| c.polyline().bounds())
        .reduce(|all, b| all.including(b.min).including(b.max))
}

/// `project` as `board` of Inlay `op` cuts it: the project itself
/// for the female, whose board is the stock, and
/// [`Project::on_male_board`] for the male.
fn on_board<'p>(project: &'p Project, op: &InlayOp, board: Board) -> Cow<'p, Project> {
    match board {
        Board::Female => Cow::Borrowed(project),
        Board::Male => Cow::Owned(project.on_male_board(op)),
    }
}

/// What `board` of Inlay `op` in `project` carves (design steps 2 to 4),
/// its near parts merged ([`merge_near`]), on that board
/// ([`on_board`]). Warnings of the design's outlines (an open path, a
/// duplicate, near parts merged) go to `warnings`.
///
/// The male's drawn boundary (format 26), as every generator
/// re-checks validation: the boundary's own problems are
/// [`CamError::InlayBoundary`]; on the male, what validation warns of it
/// ([`Warning::InlayMarginIgnored`], [`Warning::InlayBoundaryHoleIgnored`],
/// [`Warning::InlayBoundaryEmpty`]) is returned, from the same check
/// ([`dc_project::drawn_boundary`]).
///
/// # Errors
///
/// [`CamError::InlayBoundary`].
fn check_boundary(project: &Project, op: &InlayOp, board: Board) -> Result<Vec<Warning>, CamError> {
    if board != Board::Male || !op.has_boundary() {
        return Ok(Vec::new());
    }
    let drawn = match dc_project::drawn_boundary(project, op) {
        Err(problem) => {
            return Err(CamError::InlayBoundary {
                op: op.id.clone(),
                problem,
            });
        }
        Ok(None) => return Ok(Vec::new()),
        Ok(Some(drawn)) => drawn,
    };
    let mut warnings = Vec::new();
    for (field, set) in [
        ("margin_mm", op.margin_mm.is_some()),
        ("outline_rounding_mm", op.outline_rounding_mm.is_some()),
    ] {
        if set {
            warnings.push(Warning::InlayMarginIgnored {
                op: op.id.clone(),
                field,
            });
        }
    }
    for (index, instance) in drawn.instances.iter().enumerate() {
        for &profile in &instance.holes {
            warnings.push(Warning::InlayBoundaryHoleIgnored {
                op: op.id.clone(),
                index,
                instance: instance.instance.clone(),
                instance_name: instance.name.clone(),
                profile,
            });
        }
    }
    for &index in &drawn.empty {
        // An empty boundary instance is one of the boundary's instances.
        let instance = at(
            &drawn.instances,
            index,
            "an empty boundary instance is none of the boundary's",
        )
        .map_err(|e| e.for_op(&op.id))?;
        warnings.push(Warning::InlayBoundaryEmpty {
            op: op.id.clone(),
            index,
            instance: instance.instance.clone(),
            instance_name: instance.name.clone(),
        });
    }
    Ok(warnings)
}

/// As [`Values`] (an unset or out-of-range value, `InlayGlueGap`,
/// `InlayMargin`), [`CamError::MissingThickness`] (the board's), and what
/// [`carve_regions`] refuses.
pub(crate) fn side(
    project: &Project,
    op: &InlayOp,
    board: Board,
    warnings: &mut Vec<Warning>,
) -> Result<Side, CamError> {
    check_boundary(project, op, board)?;
    let values = Values::of(project, op, board)?;
    side_of(&on_board(project, op, board), op, values, warnings)
}

fn side_of(
    project: &Project,
    op: &InlayOp,
    values: Values,
    warnings: &mut Vec<Warning>,
) -> Result<Side, CamError> {
    let thickness_mm = context::thickness_mm(project)?;
    let start_plane_z_mm = thickness_mm - values.start_depth_mm();
    let Some(margin_mm) = values.margin_mm else {
        let design = carve_regions(project, &op.id, &op.targets, warnings)?;
        let design = merge_near(design, project, op, None, warnings)?;
        return Ok(Side {
            targets: round_for_bit(design, project, op, None, warnings)?,
            start_plane_z_mm,
            engaged_from_z_mm: thickness_mm,
            flat_depth_mm: values.flat_depth_mm(),
            mirror_x_mm: None,
            design: Vec::new(),
            band: None,
            band_edge: Vec::new(),
            open_edge: None,
            clipped: None,
        });
    };
    let mirror_x_mm = axis_x_mm(project, &op.id, &op.targets)?;
    let mirror = mirror_x_mm.map(Transform::mirror_x_about);
    let design = carve_regions_then(project, &op.id, &op.targets, mirror, warnings)?;
    let design = merge_near(design, project, op, mirror, warnings)?;
    // Rounded for a tapered ball nose, on the mirrored design, as the
    // female's is on the design.
    let design = round_for_bit(design, project, op, mirror, warnings)?;
    // The band's edge, closed by `R'`, and whether it is open.
    let band = male_band(project, op);
    let edge_rounding_mm = band.as_ref().map_or(0.0, |b| b.edge_rounding_mm);
    let tolerance_mm = project.flatten_tolerance_mm;
    // The plug's outline within the V-bit's wall tolerance; the
    // band's edge within the project's.
    let plug_tolerance_mm = plug_tolerance_mm(project, op);
    // The one function the safety pass derives the band by too.
    let edge = band
        .as_ref()
        .and_then(|b| band_edge_of(project, op, b, mirror, &design.regions))
        .unwrap_or_else(|| {
            rounded_band(&design.regions, margin_mm, edge_rounding_mm, tolerance_mm)
        });
    // A band that runs past the board's edge is clipped to it.
    let on_board = band.as_ref().and_then(|b| {
        let board = b.board(&project.stock);
        let clipped = band_on_board(&edge, board, tolerance_mm)?;
        let design_bounds = bounds_of(design.regions.iter().map(Region::outer))?;
        let at = Clipped::of(
            &edge,
            board,
            b.board_inset_mm(),
            design_bounds,
            &project.stock,
        )?;
        Some((clipped, at))
    });
    let drawn = matches!(
        band.as_ref().map(|b| &b.source),
        Some(BandSource::Drawn { .. })
    );
    let (regions, clipped_edge, clipped) = match on_board {
        // A computed band is the design's surround; a drawn one is its
        // edge less the design.
        None if drawn => (
            difference_within(&edge, &design.regions, tolerance_mm, plug_tolerance_mm),
            edge.clone(),
            None,
        ),
        None => (
            surround_within(
                &design.regions,
                margin_mm,
                edge_rounding_mm,
                tolerance_mm,
                plug_tolerance_mm,
            ),
            edge.clone(),
            None,
        ),
        Some((band, at)) => (
            difference_within(&band, &design.regions, tolerance_mm, plug_tolerance_mm),
            band,
            Some(at),
        ),
    };
    let open_edge = band.as_ref().filter(|b| b.is_open()).map(|_| OpenEdge {
        band: clipped_edge,
        design: design.regions.clone(),
        top_z_mm: thickness_mm,
    });
    let mut shapes = Vec::with_capacity(regions.len());
    for region in &regions {
        shapes.push(bordering_shape(region, &design)?);
    }
    Ok(Side {
        targets: Targets {
            regions,
            shapes,
            // The surround's regions lie side by side; none is cut before
            // another.
            containment: Vec::new(),
            contours: design.contours,
        },
        start_plane_z_mm,
        engaged_from_z_mm: thickness_mm,
        flat_depth_mm: values.flat_depth_mm(),
        mirror_x_mm,
        design: design.regions,
        band,
        band_edge: edge,
        open_edge,
        clipped,
    })
}

/// How near, mm, the plug's outline is drawn for Inlay `op`: the
/// project's flatten tolerance scaled for its bit's flank
/// ([`wall_tolerance_mm`]: a V-bit's `tan(α/2)`, a tapered ball nose's
/// `tan β`), or the tolerance itself when the tool is not a usable
/// carving bit.
fn plug_tolerance_mm(project: &Project, op: &InlayOp) -> f64 {
    let tolerance_mm = project.flatten_tolerance_mm;
    dc_project::inlay_profile(project, op).map_or(tolerance_mm, |profile| {
        wall_tolerance_mm(tolerance_mm, profile.slope())
    })
}

/// `design`, Inlay `op`'s design resolved and merged (the male's moved by
/// its `mirror`), rounded for its tapered ball nose
/// ([`dc_project::rounded_design`]; #681, design spec "Inlay fit"), as the
/// safety pass rounds it. Each kind of change warns
/// [`Warning::InlayRoundedForBit`] at its largest, the point as drawn: the
/// male's is mirrored back. A rounded region is named by the shape it was
/// rounded from. A design carved by a V-bit is returned as it is.
///
/// # Errors
///
/// [`CamError::InlayTooNarrowForBit`] where the rounding would remove,
/// split or join a part or a hole.
fn round_for_bit(
    design: Targets,
    project: &Project,
    op: &InlayOp,
    mirror: Option<Transform>,
    warnings: &mut Vec<Warning>,
) -> Result<Targets, CamError> {
    if !matches!(
        dc_project::inlay_profile(project, op),
        Some(BitProfile::TaperedBall { .. })
    ) {
        return Ok(design);
    }
    // A mirror is its own inverse.
    let as_drawn = |p: Point| mirror.map_or(p, |m| m.apply(p));
    let rounded = dc_project::rounded_design(project, op, &design.regions).map_err(|narrow| {
        let at = as_drawn(narrow.at);
        CamError::InlayTooNarrowForBit {
            op: op.id.clone(),
            kind: narrow.kind,
            x_mm: at.x,
            y_mm: at.y,
            width_mm: narrow.width_mm,
            needed_mm: narrow.needed_mm,
        }
    })?;
    for change in &rounded.changes {
        let at = as_drawn(change.at);
        warnings.push(Warning::InlayRoundedForBit {
            op: op.id.clone(),
            kind: change.kind,
            x_mm: at.x,
            y_mm: at.y,
            change_mm: change.change_mm,
            radius_mm: change.radius_mm,
        });
    }
    // Each rounded region is one of the design's, rounded.
    let mut shapes = Vec::with_capacity(rounded.sources.len());
    for &i in &rounded.sources {
        shapes.push(
            at(
                &design.shapes,
                i,
                "a rounded region is of no region of the design",
            )?
            .clone(),
        );
    }
    Ok(Targets {
        regions: rounded.regions,
        shapes,
        containment: design.containment,
        contours: design.contours,
    })
}

/// `design`, the targets of Inlay `op` resolved (the male's moved by its
/// `mirror`), with the parts that come closer than [`INLAY_MERGE_MM`]
/// merged into one shape ([`merge_near_parts`]; #479, design "Inlay",
/// step 2), as the safety pass merges them. Each pair of outlines that
/// come near warns [`Warning::InlayPartsMerged`] at the middle of their
/// gap as drawn: the male's point is mirrored back. A merged region is
/// named by the shape of its first part, and the containment pairs follow
/// the names, less those inside one merged region.
///
/// A merged group is drawn within the V-bit's wall tolerance, as
/// the plug's outline is.
///
/// # Errors
///
/// [`CamError::Internal`] when what [`merge_near_parts`] says it merged is
/// no outline or region of `design`, where a merged region would be named
/// by no shape.
fn merge_near(
    design: Targets,
    project: &Project,
    op: &InlayOp,
    mirror: Option<Transform>,
    warnings: &mut Vec<Warning>,
) -> Result<Targets, CamError> {
    let merge_mm = INLAY_MERGE_MM;
    let merged = merge_near_parts(&design.regions, merge_mm, plug_tolerance_mm(project, op));
    if merged.near.is_empty() && merged.filled.is_empty() {
        return Ok(design);
    }
    // The outlines are numbered as the design's regions number them.
    let shape_of = |index: usize| {
        at(
            &design.contours,
            index,
            "a merged outline is none of the design's",
        )
        .map(|(s, _)| s.clone())
    };
    let region_shape = |k: usize| {
        at(
            &design.shapes,
            k,
            "a merged region is of no region of the design",
        )
    };
    for near in &merged.near {
        let (first, second) = (shape_of(near.first)?, shape_of(near.second)?);
        // A mirror is its own inverse.
        let at = mirror.map_or(near.at, |m| m.apply(near.at));
        warnings.push(Warning::InlayPartsMerged {
            op: op.id.clone(),
            first,
            second,
            x_mm: at.x,
            y_mm: at.y,
            gap_mm: near.gap_mm,
            merge_mm,
        });
    }
    for filled in &merged.filled {
        let hole = shape_of(filled.index)?;
        let at = mirror.map_or(filled.at, |m| m.apply(filled.at));
        warnings.push(Warning::InlayHoleFilled {
            op: op.id.clone(),
            hole,
            x_mm: at.x,
            y_mm: at.y,
            width_mm: filled.width_mm,
            merge_mm,
        });
    }
    // Each input region's shape, and the shape it is now named by.
    let mut renamed: Vec<(&ShapeRef, &ShapeRef)> = Vec::new();
    let mut shapes: Vec<ShapeRef> = Vec::with_capacity(merged.sources.len());
    for sources in &merged.sources {
        // A merged region is made of one region of the design or more.
        let &first = sources
            .first()
            .ok_or_else(|| CamError::internal("a merged region is made of no region"))?;
        let name = region_shape(first)?;
        for &k in sources {
            renamed.push((region_shape(k)?, name));
        }
        shapes.push(name.clone());
    }
    let rename = |s: &ShapeRef| {
        renamed
            .iter()
            .find(|(from, _)| *from == s)
            .map_or_else(|| s.clone(), |(_, to)| (*to).clone())
    };
    let mut containment = Vec::new();
    for (inner, encloser) in &design.containment {
        let pair = (rename(inner), rename(encloser));
        if pair.0 != pair.1 && !containment.contains(&pair) {
            containment.push(pair);
        }
    }
    Ok(Targets {
        regions: merged.regions,
        shapes,
        containment,
        contours: design.contours,
    })
}

/// The design outline surround region `region` borders, to name it by:
/// the one whose first vertex is nearest the region's outlines (the
/// band round a shape has the shape as a hole, a hole of the design is a
/// region of its own).
///
/// # Errors
///
/// [`CamError::Internal`] when the design has no outline to name the
/// region by (a band stands round a plug, so a male with a band has one),
/// or an outline has no vertex or the region no edge to measure to. The
/// region was once named by a shape with an empty instance id.
fn bordering_shape(region: &Region, design: &Targets) -> Result<ShapeRef, CamError> {
    let outlines: Vec<_> = std::iter::once(region.outer())
        .chain(region.holes())
        .map(dc_geometry::Contour::polyline)
        .collect();
    let mut nearest: Option<(&ShapeRef, f64)> = None;
    for (shape, contour) in &design.contours {
        let probe = contour
            .polyline()
            .vertices()
            .first()
            .ok_or_else(|| CamError::internal("a design outline has no vertex"))?
            .point();
        let gap_mm = outlines
            .iter()
            .filter_map(|o| nearest_point(o, probe))
            .map(|n| n.gap_mm)
            .reduce(f64::min)
            .ok_or_else(|| CamError::internal("a male Inlay's band region has no edge"))?;
        // The first of the outlines as near.
        if nearest.is_none_or(|(_, best_mm)| gap_mm.total_cmp(&best_mm).is_lt()) {
            nearest = Some((shape, gap_mm));
        }
    }
    nearest
        .map(|(shape, _)| shape.clone())
        .ok_or_else(|| CamError::internal("a male Inlay's band borders no outline of its design"))
}

/// The ridges the V-bit leaves on the floor of `board` of Inlay `op`,
/// along its rings and at their corners (the same height since #527), as
/// for a V-Carve ([`vcarve::floor_ridges`]); `None` when that board has no floor
/// wide enough, no usable V-bit or step-over, or does not resolve (the
/// male's, while its board is not measured).
#[must_use]
pub fn floor_ridges(project: &Project, op: &InlayOp, board: Board) -> Option<FloorRidges> {
    let tool = project.tools.iter().find(|t| t.id == op.tool)?;
    let bit = checked_bit(&op.id, tool).ok()?;
    let fraction = op
        .step_over_fraction
        .or(tool.step_over_fraction)
        .filter(|f| f.is_finite() && *f > 0.0 && *f <= 1.0)?;
    let side = side(project, op, board, &mut Vec::new()).ok()?;
    let floor = Floor {
        z_mm: side.start_plane_z_mm - side.flat_depth_mm,
        inset_mm: bit.profile.half_width_mm(side.flat_depth_mm),
        step_over_fraction: Some(fraction),
        profile: bit.profile,
    };
    floor.ridges(
        &side.targets.regions,
        side.flat_depth_mm,
        project.flatten_tolerance_mm,
    )
}

/// Generates both boards of the Inlay `op` in `project`: the
/// female's runs on the stock, then the male's on its male board, each
/// [`ToolRun`] tagged with its [`Board`], as [`generate_inlay_board`]
/// generates each. While the male board has no thickness the male is not
/// cut: the runs are the female's, with [`Warning::InlayMaleBoardUnset`].
///
/// The warnings are the female's, then the male's that differ from them;
/// the male's [`Warning::InlayPartsMerged`], [`Warning::InlayHoleFilled`]
/// and [`Warning::InlayRoundedForBit`] are left out, since it merges, fills
/// and rounds the same design's parts, mirrored, and the female's already
/// name them where they are drawn.
///
/// # Errors
///
/// As [`generate_inlay_board`], for either board.
#[must_use = "the toolpath is the whole point"]
pub fn generate_inlay(
    project: &Project,
    op: &InlayOp,
) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError> {
    let (mut runs, mut warnings) = generate_inlay_board(project, op, Board::Female)?;
    let (male, male_warnings) = generate_inlay_board(project, op, Board::Male)?;
    runs.extend(male);
    for warning in male_warnings {
        let repeats = matches!(
            warning,
            Warning::InlayPartsMerged { .. }
                | Warning::InlayHoleFilled { .. }
                | Warning::InlayRoundedForBit { .. }
        ) || warnings.contains(&warning);
        if !repeats {
            warnings.push(warning);
        }
    }
    Ok((runs, warnings))
}

/// Generates `board` of the Inlay `op` in `project`, in that
/// board's project space (Y-up, mm, its bottom at Z = 0, safe Z at its
/// `thickness + op.safe_z_mm`; the male board shares the stock's
/// bottom-left corner, [`Project::on_male_board`]): one [`ToolRun`] per
/// clearing pass that reaches some of the floor, then [`RunLabel::Main`]
/// with the operation's V-bit, as a V-Carve's
/// ([`vcarve::generate_vcarve`]), and on the male its cut-out last; every
/// run tagged `board`.
///
/// The female carves the design (the targets' closed outlines nested
/// even-odd) from the stock top with a flat depth of `depth_mm`. The male
/// carves the surround of the design mirrored about its targets' centre
/// line ([`male_mirror`]), `margin_mm` wide, from a start plane
/// `depth_mm − glue_gap_mm` below the male board's top with a flat depth
/// of `plug_depth_mm`, engaged from that top; it warns
/// [`Warning::InlayMaleMirrored`] with the line's X, and
/// [`Warning::InlayPlugBaseThin`] when less than
/// [`INLAY_MIN_PLUG_BASE_MM`] of the male board is left under its floor.
/// While the male board has no thickness the male has no runs, and warns
/// [`Warning::InlayMaleBoardUnset`] instead. On either board, parts of the
/// design closer than [`INLAY_MERGE_MM`] are cut as one shape, each pair
/// warning [`Warning::InlayPartsMerged`]. The female ignores the
/// cut-out.
///
/// # Errors
///
/// An unknown tool, a tool that is not a usable V-bit or tapered ball nose
/// ([`CamError::TaperedBallNoseUnusable`]), a missing rate or
/// thickness, a safe Z that is not positive; [`CamError::InlayValueUnset`],
/// [`CamError::InlayValueRange`], [`CamError::InlayGlueGap`],
/// [`CamError::InlayMargin`]; [`CamError::VBitConeExceeded`] when the
/// flank's engagement from the board's top, `D` or `D − G + P`, is past
/// the bit's cone or a tapered ball nose's cutting height (security review
/// H4); [`CamError::InlayTooNarrowForBit`]; a clearing pass that
/// cannot run;
/// and what [`vcarve::blocks`] and the V-Carve's sequencing and cone test
/// refuse.
#[must_use = "the toolpath is the whole point"]
pub fn generate_inlay_board(
    project: &Project,
    op: &InlayOp,
    board: Board,
) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError> {
    if board == Board::Male && !op.cuts_male() {
        return Ok((
            Vec::new(),
            vec![Warning::InlayMaleBoardUnset { op: op.id.clone() }],
        ));
    }
    let (mut runs, warnings) =
        generate_on(&on_board(project, op, board), op, board).map_err(|e| e.for_op(&op.id))?;
    for run in &mut runs {
        run.board = board;
    }
    Ok((runs, warnings))
}

/// [`generate_inlay_board`] on `project`, already that board's.
fn generate_on(
    project: &Project,
    op: &InlayOp,
    board: Board,
) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError> {
    let tool = tool_of(project, &op.id, &op.tool)?;
    // A V-bit or a tapered ball nose.
    let bit = checked_bit(&op.id, tool)?;
    rates_of(tool)?;
    check_safe_z(&op.id, op.safe_z_mm)?;
    let mut warnings = check_boundary(project, op, board)?;
    let values = Values::of(project, op, board)?;
    let cut_out = checked_cut_out(project, op, values)?;
    let passes = clearing::Passes::of_inlay(op, values.start_depth_mm());
    let clearing_tools = clearing::checked_tools(project, &passes, true)?;
    let side = side_of(project, op, values, &mut warnings)?;
    // Step 1: the flank is engaged from the stock top, so the plug above
    // the male's start plane counts.
    let engaged_mm = values.engagement_mm();
    // The error names the first shape carved; with none, nothing is carved
    // and no flank is engaged.
    if engaged_mm > bit.cone_mm + EPS_MM
        && let Some(shape) = side.targets.shapes.first()
    {
        return Err(CamError::VBitConeExceeded {
            op: op.id.clone(),
            shape: Box::new(shape.clone()),
            depth_mm: engaged_mm,
            cone_mm: bit.cone_mm,
            limit: bit.limit,
        });
    }
    // A glue gap or plug depth below a tapered ball nose's tangent
    // height (validation reports both too).
    if let Some(cost) = dc_project::glue_gap_below_ball(&bit.profile, values.glue_gap_mm) {
        warnings.push(Warning::InlayGlueGapBelowBall {
            op: op.id.clone(),
            glue_gap_mm: values.glue_gap_mm,
            tangent_height_mm: cost.tangent_height_mm,
            lift_mm: cost.lift_mm,
            wall_gap_mm: cost.wall_gap_mm,
        });
    }
    if let Some(cost) = dc_project::plug_depth_below_ball(&bit.profile, values.plug_depth_mm) {
        warnings.push(Warning::InlayPlugDepthBelowBall {
            op: op.id.clone(),
            plug_depth_mm: values.plug_depth_mm,
            tangent_height_mm: cost.tangent_height_mm,
            wall_gap_mm: cost.wall_gap_mm,
        });
    }
    if let Some(axis_x_mm) = side.mirror_x_mm {
        warnings.push(Warning::InlayMaleMirrored {
            op: op.id.clone(),
            axis_x_mm,
        });
    }
    if values.margin_mm.is_some() {
        let thickness_mm = side.engaged_from_z_mm;
        let base_mm = thickness_mm - engaged_mm;
        if base_mm < INLAY_MIN_PLUG_BASE_MM {
            warnings.push(Warning::InlayPlugBaseThin {
                op: op.id.clone(),
                base_mm,
            });
        }
    }
    // Where the board's edge clips the band, the design must still
    // leave the band's room there.
    if let (Some(clipped), Some(band)) = (&side.clipped, &side.band) {
        let cut_out_diameter_mm = cut_out.as_ref().map(|(_, _, d)| *d);
        clipped.check_room(&op.id, band.board_room_mm(cut_out_diameter_mm))?;
        warnings.push(Warning::InlayBandClipped {
            op: op.id.clone(),
            edges: clipped.edges.iter().map(|(e, _)| *e).collect(),
            open: band.is_open(),
            narrowest_mm: clipped.narrowest_mm(),
        });
    }
    // Clearing passes that leave the band's edge a V wall.
    if let Some(BandEdge::VWall {
        margin_needed_mm: Some(margin_needed_mm),
    }) = side.band.as_ref().map(|b| &b.edge)
    {
        warnings.push(Warning::InlayBandEdgeVWall {
            op: op.id.clone(),
            margin_needed_mm: *margin_needed_mm,
        });
    }
    let Side {
        targets,
        start_plane_z_mm,
        engaged_from_z_mm,
        flat_depth_mm,
        design,
        band_edge,
        open_edge,
        ..
    } = side;
    let carve = Carve {
        regions: targets.regions,
        shapes: targets.shapes,
        start_plane_z_mm,
        engaged_from_z_mm,
        flat_depth_mm: Some(flat_depth_mm),
        strategy: &MEDIAL,
        step_over_fraction: op.step_over_fraction.or(tool.step_over_fraction),
        cleared_by: clearing_tools
            .iter()
            .map(|t| Clearing {
                radius_mm: t.diameter_mm / 2.0,
            })
            .collect(),
        open_edge,
        // The male's walls along the plug reach every corner.
        design: values.margin_mm.map(|_| design.clone()),
    };
    let mut runs = clearing::runs(
        project,
        &passes,
        &carve,
        &clearing_tools,
        &bit.profile,
        &mut warnings,
    )?;
    let blocks = vcarve::blocks(project, &op.id, tool, &carve, &mut warnings)?;
    let toolpath = vcarve::finish(
        project,
        &op.id,
        tool,
        &carve,
        blocks,
        &targets.containment,
        op.machining_order,
    )?;
    runs.push(ToolRun {
        label: RunLabel::Main,
        tool: tool.id.clone(),
        toolpath,
        board,
    });
    // The cut-out last, so the piece is freed only once carved.
    if let (Some((cut_out, band, diameter_mm)), Some(thickness_mm)) =
        (cut_out, project.stock.thickness_mm)
    {
        let outlines = cut_out_paths(
            &band_edge,
            &band,
            diameter_mm,
            band.board(&project.stock),
            project.flatten_tolerance_mm,
        );
        let (cut_project, profile, tab_warnings) =
            cut_out_profile(project, op, cut_out, &outlines, thickness_mm)?;
        let (toolpath, cut_warnings) =
            generate_profile_below(&cut_project, &profile, values.engagement_mm())?;
        warnings.extend(tab_warnings);
        // Every tab the Profile crosses is one its request placed, and
        // placing has said which outlines got fewer than asked.
        warnings.extend(
            cut_warnings
                .into_iter()
                .filter(|w| !matches!(w, Warning::TabsCoverMostOfContour { placed: true, .. })),
        );
        runs.push(ToolRun {
            label: RunLabel::CutOut(cut_out.tool.clone()),
            tool: cut_out.tool.clone(),
            toolpath,
            board,
        });
    }
    Ok((runs, warnings))
}

/// The cut-out of `op`, checked as validation checks it, with the
/// male's band ([`dc_project::male_band`]) and its tool's diameter,
/// mm: `None` without one, and on the female (no margin in `values`),
/// which has no plug to free.
///
/// # Errors
///
/// [`CamError::UnknownTool`], [`CamError::CutterNotFlat`] or
/// [`CamError::InvalidToolDiameter`] for its tool; and
/// [`CamError::InlayCutOutNoRoom`] when the band's flat floor ring is
/// narrower than the tool: `M − P·tan(α/2)` wide with an open edge, `M −
/// 2·P·tan(α/2)` with a V wall.
fn checked_cut_out<'o>(
    project: &Project,
    op: &'o InlayOp,
    values: Values,
) -> Result<Option<(&'o InlayCutOut, MaleBand, f64)>, CamError> {
    let Some(cut_out) = &op.cut_out else {
        return Ok(None);
    };
    let Some(margin_mm) = values.margin_mm else {
        // The female ignores the cut-out: only the male's plug is cut out.
        return Ok(None);
    };
    let tool = tool_of(project, &op.id, &cut_out.tool)?;
    if !tool.cutter.is_flat() {
        return Err(CamError::CutterNotFlat {
            op: op.id.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            cutter: tool.cutter.describe(),
        });
    }
    check_diameter(tool)?;
    // Values::of has checked every value the band needs, and
    // `generate_inlay` the V-bit, so the band resolves.
    let Some(band) = male_band(project, op) else {
        return Err(CamError::InlayMargin {
            op: op.id.clone(),
            margin_mm,
        });
    };
    let needed_mm = band.cut_out_margin_needed_mm(tool.diameter_mm);
    if margin_mm + EPS_MM < needed_mm {
        return Err(CamError::InlayCutOutNoRoom {
            op: op.id.clone(),
            diameter_mm: tool.diameter_mm,
            floor_mm: margin_mm - (needed_mm - tool.diameter_mm),
            needed_mm,
        });
    }
    Ok(Some((cut_out, band, tool.diameter_mm)))
}

/// A male Inlay's cut-out paths for an end mill `diameter_mm` wide
/// (design "Inlay", step 6): the outer outlines of
/// the band's edge `edge`, `B` before it is clipped
/// ([`dc_project::band_edge`]: the design grown by the margin and closed
/// by the edge's rounding, or the drawn boundary mirrored), clipped to
/// `board`, [`MaleBand::board`], where it runs past it, inset by
/// [`MaleBand::cut_out_inset_mm`] ([`dc_geometry::inset_outers`]), so
/// the tool's edge runs along the outer edge of the band's flat floor: on
/// `B` when the edge is open, `P·tan(α/2)` inside it, at the V wall's
/// foot, otherwise. Above the floor it cuts only what the carve removed.
/// A hole of the design is not cut out: it stays in the plug.
///
/// Where an open band is clipped to the board, the board's edge frees the
/// plug, and the stretches that run along it, the tool's radius inside
/// it, are dropped ([`dc_geometry::without_rect_sides`]): the rest are
/// open paths from the board's edge to the board's edge, their ends the
/// radius inside it. Every other path is closed.
///
/// The paths come back by outline: for each outline the one closed
/// path it is, or the open pieces the board's edge leaves of it, in its
/// order. An outline that runs only along the board's edge gives none.
/// The pieces of one outline are one cut to the user, and share its tabs
/// ([`cut_out_profile`]).
#[must_use]
pub fn cut_out_paths(
    edge: &[Region],
    band: &MaleBand,
    diameter_mm: f64,
    board: Rect,
    tolerance_mm: f64,
) -> Vec<Vec<Polyline>> {
    let inset_mm = band.cut_out_inset_mm(diameter_mm);
    let closed = |outlines: Vec<Contour>| -> Vec<Vec<Polyline>> {
        outlines
            .into_iter()
            .map(|outline| vec![outline.into_polyline()])
            .collect()
    };
    let Some(clipped) = band_on_board(edge, board, tolerance_mm) else {
        return closed(inset_outers(edge, inset_mm, tolerance_mm));
    };
    let outlines = inset_outers(&clipped, inset_mm, tolerance_mm);
    if !band.is_open() {
        return closed(outlines);
    }
    let within = Rect {
        min: Point::new(board.min.x + inset_mm, board.min.y + inset_mm),
        max: Point::new(board.max.x - inset_mm, board.max.y - inset_mm),
    };
    outlines
        .iter()
        .map(|outline| without_rect_sides(std::slice::from_ref(outline), within, tolerance_mm))
        .filter(|pieces| !pieces.is_empty())
        .collect()
}

/// The On line Profile a male Inlay's cut-out is, and a copy of
/// `project` that holds it: the paths of `outlines` ([`cut_out_paths`]:
/// project space, by outline; closed, or open from the board's edge to
/// the board's edge where the band is clipped) as the
/// profiles of one definition placed in place, which the Profile targets
/// with the cut-out's tool and through-cut allowance, through the
/// stock `thickness_mm` thick, climb, plunging, with the Inlay's id,
/// name, machining order and safe Z. The Profile is the copy's only
/// operation.
///
/// The cut-out's tab request is placed here, before anything is cut
/// (ADR-0005): the copy holds its tabs as placed tabs on that one
/// instance, and the Profile carries no request. Each outline takes the
/// request, as `dreamcarve tabs place` places a Profile's
/// ([`crate::placed_tabs::place`]); the pieces the board's edge leaves of
/// one outline take it between them ([`crate::placed_tabs::place_shared`]),
/// so a cut-out asked for four tabs gets four, however many pieces it is
/// cut in. The safety pass builds the same Profile itself, on paths it
/// derives itself, and places its tabs by the same rule.
///
/// The warnings are what placing found that the Profile cannot find again
/// from placed tabs: an outline or piece that got fewer tabs than its
/// share ([`Warning::TabsCoverMostOfContour`]) and a tab left on a corner
/// ([`Warning::TabOnCorner`]). Tabs that overlap are the Profile's to
/// report.
///
/// # Errors
///
/// As [`crate::placed_tabs::place`] when the request cannot be placed: a
/// tab size that is not a positive number, or a tool or path the Profile
/// cannot cut.
pub fn cut_out_profile(
    project: &Project,
    op: &InlayOp,
    cut_out: &InlayCutOut,
    outlines: &[Vec<Polyline>],
    thickness_mm: f64,
) -> Result<(Project, ProfileOp, Vec<Warning>), CamError> {
    let mut copy = project.clone();
    let mut stem = format!("{}-cut-out", op.id);
    while copy.instances.iter().any(|i| i.id.as_str() == stem)
        || copy.definitions.keys().any(|d| d.as_str() == stem)
    {
        stem.push('_');
    }
    let definition = DefinitionId::new(stem.clone());
    let instance = InstanceId::new(stem);
    copy.definitions.insert(
        definition.clone(),
        FeatureDefinition {
            id: definition.clone(),
            name: format!("{} cut-out", op.name),
            shape: None,
            profiles: outlines.iter().flatten().cloned().collect(),
        },
    );
    copy.instances.push(FeatureInstance {
        id: instance.clone(),
        definition,
        transform: Transform::IDENTITY,
        name: format!("{} cut-out", op.name),
        visible: true,
        locked: false,
        construction: false,
        folder: None,
        z_top_mm: None,
        z_bottom_mm: None,
    });
    let profile = ProfileOp {
        id: op.id.clone(),
        name: op.name.clone(),
        enabled: true,
        targets: vec![instance],
        tool: cut_out.tool.clone(),
        side: ProfileSide::OnLine,
        depth_mm: Some(thickness_mm + cut_out.through_cut_allowance_mm),
        step_down_mm: None,
        direction: CutDirection::Climb,
        through_cut_allowance_mm: cut_out.through_cut_allowance_mm,
        safe_z_mm: op.safe_z_mm,
        tabs: None,
        machining_order: op.machining_order,
        entry: Entry::Plunge,
        lead: None,
        corner_relief: CornerRelief::None,
        last_pass: None,
        start: StartAt::Auto,
    };
    copy.operations = vec![Operation::Profile(profile.clone())];
    let mut warnings = Vec::new();
    if let Some(spec) = &cut_out.tabs {
        // Each outline's paths, as the definition's profiles number them.
        let mut next = 0;
        let pieces: Vec<Range<usize>> = outlines
            .iter()
            .map(|paths| {
                let from = next;
                next += paths.len();
                from..next
            })
            .collect();
        let placement = placed_tabs::place_shared(&copy, &profile.id, spec, &pieces)?;
        copy.tabs.extend(placement.tabs);
        warnings.extend(
            placement
                .warnings
                .into_iter()
                .filter(|w| !matches!(w, Warning::TabsOverlap { .. })),
        );
    }
    Ok((copy, profile, warnings))
}
