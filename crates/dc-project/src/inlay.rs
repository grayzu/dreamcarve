// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A male Inlay's band (#448; `docs/specs/phase-3/design.md`, "Algorithms
//! → Inlay", steps 4 and 6): which clearing pass, if any, cuts the band's
//! outer edge upright, how far the edge is rounded, and where the
//! cut-out runs. The generator, validation and the safety pass all need
//! the rule, and neither validation nor the safety pass may call
//! `dc-cam`, so it lives here, as [`crate::cleared_start_plane`] does.
//!
//! With `s = tan(α/2)` of the Inlay's V-bit, `P` its `plug_depth_mm`, `M`
//! its `margin_mm` and `r` an end mill's radius, the band's flat floor
//! runs from the plug's foot, the design grown by `P·s`, out to the band's
//! edge, at least `M` out. A clearing pass **fits** that ring when `M −
//! P·s ≥ 2r`. The edge is **open** when one fits (the first, the **edge
//! pass**) and the V-bit's flank at the plug's foot, `(D − G + P)·s` wide
//! at the stock top, stays inside the band: `M ≥ (D − G + 2P)·s`.
//!
//! Where the band runs past the board's edge it is clipped to the board
//! (#495, [`MaleBand::board`]): an open edge to the board itself, a V
//! wall to the board less the V-bit's radius. The design must still
//! leave the band's own room there ([`MaleBand::board_room_mm`]).

use crate::ids::{InstanceId, ToolId};
use crate::model::{InlayOp, Operation, Project, Stock, ToolKind};
use crate::project::closed_outlines;
use crate::validate::MAX_FLATTEN_TOLERANCE_MM;
use dc_geometry::{
    BandClearance, Contour, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Rect, Region, area_mm2,
    band_clearance, drawn_band, intersection, merge_near_parts, nest, wall_tolerance_mm,
};

impl Project {
    /// This project as Inlay `inlay`'s male board cuts it (#498;
    /// `docs/specs/phase-3/design.md`, "Algorithms → Inlay", "Boards"): a
    /// copy whose stock is the male board ([`crate::MaleBoard::stock`]),
    /// everything else the same, so the origin preset, heights and
    /// tolerances apply to the male board as they do to the stock. The
    /// generator cuts the male on it and the safety pass checks the male's
    /// programs against it.
    #[must_use]
    pub fn on_male_board(&self, inlay: &InlayOp) -> Self {
        let mut project = self.clone();
        project.stock = inlay.male_board.stock(&self.stock);
        project
    }

    /// The project's male board (#498): the first enabled Inlay that cuts
    /// its male, and its board as stock. Validation holds every other
    /// such Inlay to the same board (`inlay_male_boards_differ`). `None`
    /// when no Inlay cuts a male.
    #[must_use]
    pub fn male_board(&self) -> Option<(&InlayOp, Stock)> {
        self.operations.iter().find_map(|op| match op {
            Operation::Inlay(inlay) if inlay.enabled && inlay.cuts_male() => {
                Some((inlay, inlay.male_board.stock(&self.stock)))
            }
            _ => None,
        })
    }
}

/// The clearing pass that cuts a male Inlay's band edge upright (#448):
/// the first whose end mill fits the band's floor ring.
#[derive(Debug, Clone, PartialEq)]
pub struct EdgePass {
    /// Its place in the Inlay's `clearing` list.
    pub index: usize,
    /// Its end mill.
    pub tool: ToolId,
    /// The end mill's radius, mm.
    pub radius_mm: f64,
}

/// What a male Inlay's band's outer edge is (#448).
#[derive(Debug, Clone, PartialEq)]
pub enum BandEdge {
    /// Cut upright by the edge pass's end mill, out to the band's edge.
    Open(EdgePass),
    /// A V wall, cut by the V-bit as the plug's walls are.
    VWall {
        /// The least margin that would open the edge, mm, when the Inlay
        /// has clearing passes but none fits (or the V-bit's flank would
        /// reach past the band); `None` without clearing passes.
        margin_needed_mm: Option<f64>,
    },
}

/// Where a male Inlay's band edge comes from (#449).
#[derive(Debug, Clone, PartialEq)]
pub enum BandSource {
    /// The design grown by `margin_mm` and closed by
    /// `outline_rounding_mm` (0 when unset).
    Computed {
        /// `margin_mm`, mm.
        margin_mm: f64,
        /// `outline_rounding_mm`, mm; 0 when unset.
        outline_rounding_mm: f64,
    },
    /// The drawn boundary ([`InlayOp::boundary`]); `margin_mm` and
    /// `outline_rounding_mm` are ignored.
    Drawn {
        /// The least distance, mm, from the design's outlines to the
        /// drawn edge ([`DrawnBoundary::narrowest_mm`]).
        narrowest_mm: f64,
    },
}

/// A male Inlay's band, as the generator, validation and the safety pass
/// derive it (#448).
#[derive(Debug, Clone, PartialEq)]
pub struct MaleBand {
    /// Where the edge comes from (#449).
    pub source: BandSource,
    /// `margin_mm`, `M`: with a drawn boundary, the narrowest band
    /// ([`BandSource::Drawn`]), which every rule reads in its place.
    pub margin_mm: f64,
    /// `plug_depth_mm · tan(α/2)`, `P·s`: how far the plug's foot stands
    /// out from the design at the floor, mm.
    pub foot_mm: f64,
    /// `outline_rounding_mm`, `R`, 0 when unset.
    pub outline_rounding_mm: f64,
    /// The band edge's rounding, `R'`, mm: `max(R, r)` of the edge pass
    /// when the edge is open, so its end mill reaches every inward corner;
    /// `R` with a V wall.
    pub edge_rounding_mm: f64,
    /// Open or a V wall.
    pub edge: BandEdge,
    /// The V-bit's radius, `R_v`, mm: the stock-boundary rule (S7) holds
    /// a V-bit to its full radius, wherever its cone reaches (#495).
    pub v_bit_radius_mm: f64,
    /// How far the V-bit's flank at the plug's foot reaches from the
    /// design at the stock top, `(D − G + 2P)·s`, mm.
    pub flank_mm: f64,
}

impl MaleBand {
    /// The edge pass, when the edge is open.
    #[must_use]
    pub fn edge_pass(&self) -> Option<&EdgePass> {
        match &self.edge {
            BandEdge::Open(pass) => Some(pass),
            BandEdge::VWall { .. } => None,
        }
    }

    /// Whether the edge is open.
    #[must_use]
    pub fn is_open(&self) -> bool {
        self.edge_pass().is_some()
    }

    /// How far inside the band's edge `B` the centre of a cut-out
    /// `diameter_mm` wide runs, mm (design step 6; #461): as far out as the
    /// floor reaches, so the tool's edge runs along the floor's outer edge
    /// and above the floor it cuts only what the carve removed. With an
    /// open edge the floor reaches `B`, which the edge pass cuts upright:
    /// the radius `r`. With a V wall the floor ends at the slope's foot,
    /// `P·s` inside `B`: `P·s + r`, so the freed plug has no knife edge.
    #[must_use]
    pub fn cut_out_inset_mm(&self, diameter_mm: f64) -> f64 {
        let radius_mm = diameter_mm / 2.0;
        if self.is_open() {
            radius_mm
        } else {
            self.foot_mm + radius_mm
        }
    }

    /// The least margin whose floor ring holds a cut-out `diameter_mm`
    /// wide, mm: `P·s + d` with an open edge, `2·P·s + d` with a V wall.
    /// With it the cut-out's path ([`Self::cut_out_inset_mm`] inside the
    /// band's edge, at least `M` out from the design) keeps the tool its
    /// radius outside the plug's foot, the design grown by `P·s`.
    #[must_use]
    pub fn cut_out_margin_needed_mm(&self, diameter_mm: f64) -> f64 {
        if self.is_open() {
            self.foot_mm + diameter_mm
        } else {
            2.0 * self.foot_mm + diameter_mm
        }
    }

    /// How far inside the board's edge the band is clipped, mm (#495):
    /// 0 with an open edge, whose end mill clears to the board's edge, its
    /// cutting edge on it; the V-bit's radius `R_v` with a V wall, so the
    /// V-bit, which runs down the wall to its top, keeps its full radius
    /// on the board.
    #[must_use]
    pub fn board_inset_mm(&self) -> f64 {
        if self.is_open() {
            0.0
        } else {
            self.v_bit_radius_mm
        }
    }

    /// The rectangle the band is clipped to (#495): `stock`'s, `[0,
    /// width] × [0, height]`, inset by [`Self::board_inset_mm`].
    #[must_use]
    pub fn board(&self, stock: &Stock) -> Rect {
        let inset_mm = self.board_inset_mm();
        Rect {
            min: Point::new(inset_mm, inset_mm),
            max: Point::new(stock.width_mm - inset_mm, stock.height_mm - inset_mm),
        }
    }

    /// The least distance, mm, from the design to a board edge that
    /// clips the band (#495): the band left there must still hold what
    /// the margin holds. The V-bit runs along the plug's foot, `P·s` out,
    /// and keeps its full radius on the board: `P·s + R_v` (which holds
    /// its flank too, as the cone check keeps `(D − G + P)·s ≤ R_v`). With
    /// an open edge the edge pass's end mill fits the floor ring, `P·s +
    /// 2r`, and the flank stays inside the band, `(D − G + 2P)·s`. A
    /// cut-out `cut_out_diameter_mm` wide needs
    /// [`Self::cut_out_margin_needed_mm`] of band, from the clipped edge,
    /// [`Self::board_inset_mm`] inside the board's.
    #[must_use]
    pub fn board_room_mm(&self, cut_out_diameter_mm: Option<f64>) -> f64 {
        let v_bit_mm = self.foot_mm + self.v_bit_radius_mm;
        let band_mm = match &self.edge {
            BandEdge::Open(pass) => (self.foot_mm + 2.0 * pass.radius_mm).max(self.flank_mm),
            BandEdge::VWall { .. } => 0.0,
        };
        let cut_out_mm = cut_out_diameter_mm.map_or(0.0, |d| self.cut_out_margin_needed_mm(d));
        v_bit_mm.max(self.board_inset_mm() + band_mm.max(cut_out_mm))
    }
}

/// `tan(α/2)` of Inlay `op`'s V-bit, when it is one with an angle in (0°,
/// 180°).
fn slope_of(project: &Project, op: &InlayOp) -> Option<f64> {
    v_bit_of(project, op).map(|(slope, _)| slope)
}

/// `tan(α/2)` of Inlay `op`'s V-bit, when it is one with an angle in (0°,
/// 180°), and its radius, mm, as the tool gives it (`dc-cam` and the
/// safety pass refuse a diameter that is not above 0 before they use it).
fn v_bit_of(project: &Project, op: &InlayOp) -> Option<(f64, f64)> {
    let tool = project.tools.iter().find(|t| t.id == op.tool)?;
    let ToolKind::VBit { included_angle_deg } = tool.cutter else {
        return None;
    };
    let slope = (included_angle_deg.to_radians() / 2.0).tan();
    (included_angle_deg > 0.0 && included_angle_deg < 180.0 && slope.is_finite() && slope > 0.0)
        .then_some((slope, tool.diameter_mm / 2.0))
}

/// A positive finite length.
fn positive(mm: Option<f64>) -> Option<f64> {
    mm.filter(|v| v.is_finite() && *v > 0.0)
}

/// The radii of Inlay `op`'s clearing passes' end mills, in list order;
/// `None` for a pass whose tool is missing, not flat, or has no usable
/// diameter.
fn pass_radii<'p>(
    project: &'p Project,
    op: &'p InlayOp,
) -> impl Iterator<Item = (usize, &'p ToolId, Option<f64>)> + 'p {
    op.clearing.iter().enumerate().map(|(index, pass)| {
        let radius_mm = project
            .tools
            .iter()
            .find(|t| t.id == pass.tool)
            .filter(|t| t.cutter.is_flat())
            .and_then(|t| positive(Some(t.diameter_mm)))
            .map(|d| d / 2.0);
        (index, &pass.tool, radius_mm)
    })
}

/// The edge pass of Inlay `op`'s male (#448): its first clearing pass
/// whose flat end mill fits the band's floor ring, `M − P·tan(α/2) ≥ 2r`
/// (within [`EPS_MM`]). `None` without such a pass, or when the margin,
/// the plug depth or the V-bit is not usable.
#[must_use]
pub fn edge_pass(project: &Project, op: &InlayOp) -> Option<EdgePass> {
    edge_pass_at(project, op, band_source(project, op)?.margin_mm())
}

/// [`edge_pass`] for a band whose margin, or narrowest band, is
/// `margin_mm`.
fn edge_pass_at(project: &Project, op: &InlayOp, margin_mm: f64) -> Option<EdgePass> {
    let foot_mm = positive(op.plug_depth_mm)? * slope_of(project, op)?;
    let ring_mm = margin_mm - foot_mm;
    pass_radii(project, op).find_map(|(index, tool, radius_mm)| {
        let radius_mm = radius_mm?;
        (2.0 * radius_mm <= ring_mm + EPS_MM).then(|| EdgePass {
            index,
            tool: tool.clone(),
            radius_mm,
        })
    })
}

impl BandSource {
    /// `M`: the margin, or the narrowest band, mm.
    #[must_use]
    pub const fn margin_mm(&self) -> f64 {
        match self {
            Self::Computed { margin_mm, .. } => *margin_mm,
            Self::Drawn { narrowest_mm } => *narrowest_mm,
        }
    }
}

/// Where Inlay `op`'s band edge comes from (#449): the drawn boundary
/// when it has one that resolves with the design strictly inside, else
/// `margin_mm` and `outline_rounding_mm`. `None` when a value it needs is
/// unusable: a boundary with a problem ([`drawn_boundary`]), a margin or
/// a set rounding that is not above 0.
fn band_source(project: &Project, op: &InlayOp) -> Option<BandSource> {
    if op.has_boundary() {
        let drawn = drawn_boundary(project, op).ok()??;
        return positive(Some(drawn.narrowest_mm))
            .map(|narrowest_mm| BandSource::Drawn { narrowest_mm });
    }
    Some(BandSource::Computed {
        margin_mm: positive(op.margin_mm)?,
        outline_rounding_mm: match op.outline_rounding_mm {
            None => 0.0,
            some => positive(some)?,
        },
    })
}

/// Inlay `op`'s male band (#448; module docs): its edge, open or a V
/// wall, and its roundings. `None` when a value the band needs is
/// unusable: the margin (or, with a drawn boundary, the boundary: it must
/// resolve with the design inside), depth, glue gap or plug depth, an
/// `outline_rounding_mm` that is set and not above 0 (without a boundary),
/// or the V-bit. It does not read the male board, which only clips it
/// ([`MaleBand::board`]).
#[must_use]
pub fn male_band(project: &Project, op: &InlayOp) -> Option<MaleBand> {
    let (slope, v_bit_radius_mm) = v_bit_of(project, op)?;
    let source = band_source(project, op)?;
    let margin_mm = source.margin_mm();
    let plug_depth_mm = positive(op.plug_depth_mm)?;
    let depth_mm = positive(op.depth_mm)?;
    let glue_gap_mm = op
        .glue_gap_mm
        .filter(|g| g.is_finite() && *g >= 0.0 && *g < depth_mm)?;
    let outline_rounding_mm = match &source {
        BandSource::Computed {
            outline_rounding_mm,
            ..
        } => *outline_rounding_mm,
        BandSource::Drawn { .. } => 0.0,
    };
    let foot_mm = plug_depth_mm * slope;
    // The V-bit's flank at the plug's foot reaches `(D − G + P)·s` from its
    // tip at the stock top, the tip `P·s` from the design.
    let flank_mm = (depth_mm - glue_gap_mm + 2.0 * plug_depth_mm) * slope;
    let flank_fits = flank_mm <= margin_mm + EPS_MM;
    let edge = match edge_pass_at(project, op, margin_mm) {
        Some(pass) if flank_fits => BandEdge::Open(pass),
        _ => BandEdge::VWall {
            margin_needed_mm: pass_radii(project, op)
                .filter_map(|(_, _, r)| r)
                .reduce(f64::min)
                .map(|r| (foot_mm + 2.0 * r).max(flank_mm)),
        },
    };
    let edge_rounding_mm = match &edge {
        BandEdge::Open(pass) => outline_rounding_mm.max(pass.radius_mm),
        BandEdge::VWall { .. } => outline_rounding_mm,
    };
    Some(MaleBand {
        source,
        margin_mm,
        foot_mm,
        outline_rounding_mm,
        edge_rounding_mm,
        edge,
        v_bit_radius_mm,
        flank_mm,
    })
}

/// Placeholder until the drawn band is cut (#603).
#[must_use]
pub fn band_edge(
    _project: &Project,
    _op: &InlayOp,
    _mirror: Option<dc_geometry::Transform>,
    _design: &[Region],
) -> Option<Vec<Region>> {
    None
}

/// What is wrong with one of an Inlay's boundary instances (#449; design
/// spec, "Drawn boundary"); validation reports each under its own code
/// (`inlay_boundary_*`), at the entry it names.
#[derive(Debug, Clone, PartialEq)]
pub enum BoundaryProblem {
    /// The id is no instance of the project (`inlay_boundary_unknown`).
    Unknown {
        /// The id listed.
        instance: InstanceId,
    },
    /// The instance has a profile that cannot be a closed outline: it is
    /// open, or it does not make a contour (`inlay_boundary_open`). An
    /// open profile is an error, not skipped, because a boundary the user
    /// meant and lost would silently fall back to nothing.
    Open {
        /// The instance.
        instance: InstanceId,
        /// The profile's place in its definition.
        profile: usize,
    },
    /// The instance is also one of the Inlay's targets
    /// (`inlay_boundary_is_target`).
    IsTarget {
        /// The instance.
        instance: InstanceId,
    },
    /// The drawn edge touches or crosses the design, or part of the
    /// design lies outside it (`inlay_boundary_crosses_design`); on the
    /// design after near parts are merged.
    CrossesDesign {
        /// X of the worst point, mm.
        x_mm: f64,
        /// Y of the worst point, mm.
        y_mm: f64,
        /// How far past the edge it is, mm; 0 for a touch.
        distance_mm: f64,
    },
}

/// One boundary instance of an Inlay, in project space (#449).
#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryInstance {
    /// The instance.
    pub instance: InstanceId,
    /// Its display name.
    pub name: String,
    /// Its definition's closed profiles under its transform, nested
    /// even-odd as the targets are ([`nest`]).
    pub regions: Vec<Region>,
    /// The places, in its definition's profiles, of the profiles that are
    /// holes: ignored by the band (`inlay_boundary_hole_ignored`).
    pub holes: Vec<usize>,
}

/// Boundary instance `id` of Inlay `op`, resolved (#449): its closed
/// profiles under its transform, nested as the targets are. A problem
/// when `id` is no instance (or its definition is missing), is a target,
/// or has a profile that is not a closed outline.
///
/// # Errors
///
/// The [`BoundaryProblem`] of the entry.
pub fn boundary_instance(
    project: &Project,
    op: &InlayOp,
    id: &InstanceId,
) -> Result<BoundaryInstance, BoundaryProblem> {
    let unknown = || BoundaryProblem::Unknown {
        instance: id.clone(),
    };
    let instance = project
        .instances
        .iter()
        .find(|i| &i.id == id)
        .ok_or_else(unknown)?;
    if op.targets.contains(id) {
        return Err(BoundaryProblem::IsTarget {
            instance: id.clone(),
        });
    }
    let definition = project
        .definitions
        .get(&instance.definition)
        .ok_or_else(unknown)?;
    let open = |profile: usize| BoundaryProblem::Open {
        instance: id.clone(),
        profile,
    };
    let mut contours = Vec::new();
    for (index, profile) in definition.profiles.iter().enumerate() {
        if !profile.is_closed() {
            return Err(open(index));
        }
        let placed = instance
            .transform
            .apply_polyline(profile)
            .map_err(|_| open(index))?;
        contours.push(Contour::new(placed).map_err(|_| open(index))?);
    }
    if contours.is_empty() {
        return Err(open(0));
    }
    // Contours and profiles are in step: every profile made one.
    let (regions, _) = nest(contours).map_err(|_| open(0))?;
    let holes = regions
        .iter()
        .flat_map(|r| r.hole_indices().iter().copied())
        .collect();
    Ok(BoundaryInstance {
        instance: id.clone(),
        name: instance.name.clone(),
        regions,
        holes,
    })
}

/// The regions of every instance of Inlay `op`'s `boundary`, in list
/// order and in project space (#449; design spec, "Drawn boundary", step
/// 1): the function the targets' regions are collected by, per entry.
///
/// # Errors
///
/// The first entry's [`BoundaryProblem`] (`Unknown`, `Open` or
/// `IsTarget`); an open profile is an error, not skipped.
pub fn boundary_regions(
    project: &Project,
    op: &InlayOp,
) -> Result<Vec<BoundaryInstance>, BoundaryProblem> {
    op.boundary
        .iter()
        .map(|id| boundary_instance(project, op, id))
        .collect()
}

/// Inlay `op`'s drawn boundary, derived (#449; design spec, "Drawn
/// boundary", steps 1 to 3), in project space: not mirrored, which is an
/// isometry that moves the design with it.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawnBoundary {
    /// The drawn band `B`: the boundary instances' outer contours united,
    /// holes dropped ([`dc_geometry::drawn_band`]).
    pub band: Vec<Region>,
    /// The least distance, mm, from the design's outlines to the band's
    /// edge: the narrowest band, `M`.
    pub narrowest_mm: f64,
    /// X of the design's point nearest the edge, mm.
    pub x_mm: f64,
    /// Y of the design's point nearest the edge, mm.
    pub y_mm: f64,
    /// The instances, in list order.
    pub instances: Vec<BoundaryInstance>,
    /// The places, in the list, of the instances none of whose shapes
    /// holds any part of the design (`inlay_boundary_empty`).
    pub empty: Vec<usize>,
}

/// Inlay `op`'s drawn boundary, derived and checked against its design
/// (#449): the instances' regions ([`boundary_regions`]), their drawn
/// band, and the band's clearance from the design after near parts are
/// merged (the design the female and the male both use). `Ok(None)` when
/// `op` has no boundary. A design that does not resolve (a target that
/// is not an instance) is empty here; validation reports that.
///
/// # Errors
///
/// A [`BoundaryProblem`]: one of an entry, or `CrossesDesign` when the
/// edge touches or crosses the design or part of it lies outside.
pub fn drawn_boundary(
    project: &Project,
    op: &InlayOp,
) -> Result<Option<DrawnBoundary>, BoundaryProblem> {
    if !op.has_boundary() {
        return Ok(None);
    }
    let instances = boundary_regions(project, op)?;
    let tolerance_mm = tolerance_mm(project);
    let all: Vec<Region> = instances
        .iter()
        .flat_map(|i| i.regions.iter().cloned())
        .collect();
    let band = drawn_band(&all, tolerance_mm);
    let design = merged_design_of(project, op);
    let (narrowest_mm, x_mm, y_mm) = match band_clearance(&design, &band, tolerance_mm) {
        BandClearance::Inside {
            narrowest_mm,
            x_mm,
            y_mm,
        } => (narrowest_mm, x_mm, y_mm),
        BandClearance::Crosses {
            x_mm,
            y_mm,
            depth_mm,
        } => {
            return Err(BoundaryProblem::CrossesDesign {
                x_mm,
                y_mm,
                distance_mm: depth_mm,
            });
        }
    };
    let empty = instances
        .iter()
        .enumerate()
        .filter(|(_, instance)| {
            // A shape that holds no part of the design shares no area with
            // it.
            let shapes = drawn_band(&instance.regions, tolerance_mm);
            !design.is_empty()
                && area_mm2(&intersection(&shapes, &design, tolerance_mm)) <= EPS_MM * EPS_MM
        })
        .map(|(index, _)| index)
        .collect();
    Ok(Some(DrawnBoundary {
        band,
        narrowest_mm,
        x_mm,
        y_mm,
        instances,
        empty,
    }))
}

/// The flatten tolerance the geometry here is drawn within: the
/// project's, held to what validation allows (a bad value is reported
/// there, not trusted here).
fn tolerance_mm(project: &Project) -> f64 {
    if project.flatten_tolerance_mm.is_finite() && project.flatten_tolerance_mm > 0.0 {
        project.flatten_tolerance_mm.min(MAX_FLATTEN_TOLERANCE_MM)
    } else {
        FLATTEN_TOLERANCE_DEFAULT_MM
    }
}

/// Inlay `op`'s design in project space: its targets' closed outlines
/// nested even-odd, with the parts that come closer than
/// [`INLAY_MERGE_MM`] merged into one shape, drawn within its V-bit's wall
/// tolerance ([`wall_tolerance_mm`], #478), as the generator and the
/// safety pass merge them (#479). Empty when the targets do not resolve.
fn merged_design_of(project: &Project, op: &InlayOp) -> Vec<Region> {
    let Some(outlines) = closed_outlines(project, &op.targets) else {
        return Vec::new();
    };
    let Ok((design, _)) = nest(outlines) else {
        return Vec::new();
    };
    let tolerance_mm = tolerance_mm(project);
    let plug_tolerance_mm =
        slope_of(project, op).map_or(tolerance_mm, |slope| wall_tolerance_mm(tolerance_mm, slope));
    merge_near_parts(&design, INLAY_MERGE_MM, plug_tolerance_mm).regions
}

/// How close two parts of an Inlay's design may come before both sides
/// cut them as one shape (#479; `docs/specs/phase-3/design.md`,
/// "Algorithms → Inlay", step 2), mm: 0.05, the most the maintainer
/// allows. The generator, and the safety pass that re-derives the
/// design, both use it.
///
/// A gap narrower than twice the flatten tolerance plus `√2` units in the
/// last place written (0.0214 mm by default) cannot be carved at all:
/// each outline is carved as chords up to the tolerance off it, and a
/// V-bit's tip path keeps `quantum / √2` from each wall (#469). But that
/// distance is not enough. Near touches up to 0.044 mm wide still held
/// the plug up to 1.6 mm proud, where the carve does not reach into the
/// narrow wedge either side of the gap (the stress sweep's line art seed
/// `0x32`, 0.0436 mm; a square's corner beside a circle, 0.022 to
/// 0.035 mm), and merged at 0.0214 mm, the wedge the closing leaves past
/// the fill was still too narrow at some gaps (the corner, 0.016 mm). At
/// 0.05 mm every one of them, the issue's two squares and the circle
/// beside the square seat at every gap tried.
pub const INLAY_MERGE_MM: f64 = 0.05;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ToolId;
    use crate::model::{ClearanceStrategy, ClearingPass, CutDirection, Entry, MaleBoard, Tool};
    use crate::test_support::{sample_inlay, sample_project, tool_6_35_upcut, tool_v_bit_90};

    /// An end mill `id` `diameter_mm` across.
    fn end_mill(id: &str, diameter_mm: f64) -> Tool {
        Tool {
            id: ToolId::new(id),
            name: format!("{diameter_mm} mm end mill"),
            diameter_mm,
            ..tool_6_35_upcut()
        }
    }

    /// The sample Inlay's male ([`sample_inlay`] with its male board
    /// measured, 6 mm: the 90° V-bit `t2`, slope 1, `D 2.5, G 0.25, P
    /// 1.5`, margin 6, so the floor ring is `M − P·s` = 4.5 mm wide) with
    /// clearing passes by `tools`, in a project holding the 6.35 mm end
    /// mill `t1`, `t2`, a 3.175 mm end mill `t3` and a 1 mm one `t4`.
    fn male(tools: &[&str]) -> (Project, InlayOp) {
        let mut p = sample_project();
        p.tools.push(tool_v_bit_90());
        p.tools.push(end_mill("t3", 3.175));
        p.tools.push(end_mill("t4", 1.0));
        let op = InlayOp {
            male_board: MaleBoard {
                width_mm: None,
                height_mm: None,
                thickness_mm: Some(6.0),
            },
            clearing: tools
                .iter()
                .map(|t| ClearingPass {
                    tool: ToolId::new(*t),
                    strategy: ClearanceStrategy::Offset,
                    direction: CutDirection::Climb,
                    entry: Entry::Plunge,
                })
                .collect(),
            ..sample_inlay()
        };
        (p, op)
    }

    /// #448: the edge pass is the first clearing pass whose end mill fits
    /// the floor ring, `M − P·tan(α/2) ≥ 2r`: of the 6.35 mm and 3.175 mm
    /// passes in a 4.5 mm ring, the second, though a 1 mm pass follows;
    /// a 4.5 mm end mill just fits. With only the 6.35 mm pass, or none,
    /// there is no edge pass; the male board does not change it (#498).
    #[test]
    fn edge_pass_is_the_first_clearing_pass_that_fits() {
        let (p, op) = male(&["t1", "t3", "t4"]);
        let pass = edge_pass(&p, &op).expect("an edge pass");
        assert_eq!(pass.index, 1);
        assert_eq!(pass.tool, ToolId::new("t3"));
        dc_geometry::assert_approx_eq!(pass.radius_mm, 3.175 / 2.0, 1e-12);

        let (mut p, op) = male(&["t3"]);
        p.tools[2].diameter_mm = 4.5;
        assert_eq!(edge_pass(&p, &op).map(|e| e.index), Some(0), "just fits");

        for tools in [&["t1"][..], &[]] {
            let (p, op) = male(tools);
            assert_eq!(edge_pass(&p, &op), None, "{tools:?}");
        }
        let (p, op) = male(&["t3"]);
        let unmeasured = InlayOp {
            male_board: MaleBoard::default(),
            ..op.clone()
        };
        assert_eq!(edge_pass(&p, &unmeasured), edge_pass(&p, &op));
    }

    /// #448: the band's outer edge is open when an edge pass fits and the
    /// V-bit's flank at the plug's foot, `(D − G + P)·s` wide at the stock
    /// top, stays inside the band: `M ≥ (D − G + 2P)·s`, 5.25 mm here.
    /// Open, its rounding is `max(R, r)` and a cut-out's centre runs its
    /// radius inside the edge (#461); with a V wall the rounding is `R` and
    /// the centre runs `P·s` farther in, inside the slope's foot. A
    /// margin of 5 has room for the pass but not the flank: a V wall,
    /// naming the 5.25 mm needed; with no pass that fits, the margin that
    /// fits the narrowest; with no clearing, nothing.
    #[test]
    fn open_edge_needs_the_pass_and_the_flank_inside_the_band() {
        let (p, op) = male(&["t1", "t3"]);
        let band = male_band(&p, &op).expect("a male band");
        assert_eq!(band.edge_pass().map(|e| e.index), Some(1));
        dc_geometry::assert_approx_eq!(band.margin_mm, 6.0, 1e-12);
        dc_geometry::assert_approx_eq!(band.foot_mm, 1.5, 1e-12);
        dc_geometry::assert_approx_eq!(band.outline_rounding_mm, 0.0, 1e-12);
        dc_geometry::assert_approx_eq!(band.edge_rounding_mm, 3.175 / 2.0, 1e-12);
        dc_geometry::assert_approx_eq!(band.cut_out_inset_mm(3.175), 3.175 / 2.0, 1e-12);
        dc_geometry::assert_approx_eq!(band.cut_out_margin_needed_mm(3.175), 4.675, 1e-12);

        let rounded = InlayOp {
            outline_rounding_mm: Some(4.0),
            ..op.clone()
        };
        let band = male_band(&p, &rounded).expect("a male band");
        dc_geometry::assert_approx_eq!(band.outline_rounding_mm, 4.0, 1e-12);
        dc_geometry::assert_approx_eq!(band.edge_rounding_mm, 4.0, 1e-12);

        let narrow = InlayOp {
            margin_mm: Some(5.0),
            ..op.clone()
        };
        let band = male_band(&p, &narrow).expect("a male band");
        let BandEdge::VWall {
            margin_needed_mm: Some(needed_mm),
        } = band.edge
        else {
            panic!("{band:?}");
        };
        dc_geometry::assert_approx_eq!(needed_mm, 5.25, 1e-12);
        dc_geometry::assert_approx_eq!(band.edge_rounding_mm, 0.0, 1e-12);
        dc_geometry::assert_approx_eq!(band.cut_out_inset_mm(1.0), 1.5 + 0.5, 1e-12);
        dc_geometry::assert_approx_eq!(band.cut_out_margin_needed_mm(1.0), 4.0, 1e-12);

        let (p, op) = male(&["t1"]);
        let band = male_band(&p, &op).expect("a male band");
        let BandEdge::VWall {
            margin_needed_mm: Some(needed_mm),
        } = band.edge
        else {
            panic!("{band:?}");
        };
        dc_geometry::assert_approx_eq!(needed_mm, 1.5 + 6.35, 1e-12);

        let (p, op) = male(&[]);
        let band = male_band(&p, &op).expect("a male band");
        assert_eq!(
            band.edge,
            BandEdge::VWall {
                margin_needed_mm: None
            }
        );
        let unmeasured = InlayOp {
            male_board: MaleBoard::default(),
            ..op.clone()
        };
        assert_eq!(male_band(&p, &unmeasured), male_band(&p, &op));
    }

    /// The sample project with the V-bit `t2`, an Inlay `o7` of the
    /// sample rectangle `i1` (10 to 60 by 10 to 40 mm) listing `boundary`
    /// (male board 6 mm, no margin), and the shapes `shapes` as instances
    /// `i2`, `i3`, … each a closed rectangle (corner x, y, width, height)
    /// from the origin.
    fn bounded(boundary: &[&str], shapes: &[[f64; 4]]) -> (Project, InlayOp) {
        use crate::ids::{DefinitionId, InstanceId};
        use crate::model::{FeatureDefinition, FeatureInstance};
        let mut p = sample_project();
        p.tools.push(tool_v_bit_90());
        for (k, [x, y, w, h]) in shapes.iter().enumerate() {
            let id = format!("i{}", k + 2);
            let definition = DefinitionId::new(format!("d_{id}"));
            p.definitions.insert(
                definition.clone(),
                FeatureDefinition {
                    id: definition.clone(),
                    name: id.clone(),
                    profiles: vec![dc_geometry::Polyline::rect(Point::new(0.0, 0.0), *w, *h)],
                },
            );
            p.instances.push(FeatureInstance {
                id: InstanceId::new(&id),
                definition,
                transform: dc_geometry::Transform::translate(*x, *y),
                name: format!("Shape {id}"),
                visible: true,
                locked: false,
                z_top_mm: None,
                z_bottom_mm: None,
            });
        }
        let op = InlayOp {
            male_board: MaleBoard {
                width_mm: None,
                height_mm: None,
                thickness_mm: Some(6.0),
            },
            margin_mm: None,
            targets: vec![InstanceId::new("i1")],
            boundary: boundary.iter().map(|b| InstanceId::new(*b)).collect(),
            ..sample_inlay()
        };
        (p, op)
    }

    /// `boundary_regions` collects each listed instance's regions in
    /// project space, in list order: the rectangle placed at (0, 0), 80
    /// by 50 mm, and the one at (5, 5), 30 by 30 mm.
    #[test]
    fn boundary_regions_collects_each_instance_in_project_space() {
        let (p, op) = bounded(
            &["i3", "i2"],
            &[[0.0, 0.0, 80.0, 50.0], [5.0, 5.0, 30.0, 30.0]],
        );
        let found = boundary_regions(&p, &op).expect("resolves");
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].instance, crate::ids::InstanceId::new("i3"));
        assert_eq!(found[1].instance, crate::ids::InstanceId::new("i2"));
        let bounds = found[0].regions[0].outer().polyline().bounds().unwrap();
        assert_eq!((bounds.min.x, bounds.max.x), (5.0, 35.0));
        assert_eq!((bounds.min.y, bounds.max.y), (5.0, 35.0));
        assert_eq!(found[1].regions.len(), 1);
    }

    /// The problems of an entry: an id that is no instance, a target, a
    /// definition with an open profile (the profile's index).
    #[test]
    fn boundary_regions_names_what_is_wrong() {
        use crate::ids::InstanceId;
        let (p, op) = bounded(&["nope"], &[]);
        assert_eq!(
            boundary_regions(&p, &op),
            Err(BoundaryProblem::Unknown {
                instance: InstanceId::new("nope")
            })
        );
        let (p, op) = bounded(&["i1"], &[]);
        assert_eq!(
            boundary_regions(&p, &op),
            Err(BoundaryProblem::IsTarget {
                instance: InstanceId::new("i1")
            })
        );
        let (mut p, op) = bounded(&["i2"], &[[0.0, 0.0, 80.0, 50.0]]);
        p.definitions
            .get_mut(&crate::ids::DefinitionId::new("d_i2"))
            .unwrap()
            .profiles
            .insert(
                0,
                dc_geometry::Polyline::new(
                    vec![
                        dc_geometry::Vertex::line(0.0, 0.0),
                        dc_geometry::Vertex::line(5.0, 0.0),
                    ],
                    false,
                ),
            );
        assert_eq!(
            boundary_regions(&p, &op),
            Err(BoundaryProblem::Open {
                instance: InstanceId::new("i2"),
                profile: 0
            })
        );
    }

    /// The drawn boundary of an 80 × 50 rectangle round the design (10 to
    /// 60 by 10 to 40): 10 mm at the narrowest. A rectangle that cuts the
    /// design is `CrossesDesign` at its farthest point; no boundary is no
    /// drawn band.
    #[test]
    fn drawn_boundary_reads_the_narrowest_band() {
        let (p, op) = bounded(&["i2"], &[[0.0, 0.0, 80.0, 50.0]]);
        let drawn = drawn_boundary(&p, &op)
            .expect("resolves")
            .expect("a drawn boundary");
        dc_geometry::assert_approx_eq!(drawn.narrowest_mm, 10.0, 1e-9);
        assert_eq!(drawn.band.len(), 1);
        assert!(drawn.empty.is_empty());

        let (p, op) = bounded(&["i2"], &[[30.0, 0.0, 80.0, 50.0]]);
        let Err(BoundaryProblem::CrossesDesign {
            x_mm, distance_mm, ..
        }) = drawn_boundary(&p, &op)
        else {
            panic!("the design is cut");
        };
        dc_geometry::assert_approx_eq!(x_mm, 10.0, 1e-9);
        dc_geometry::assert_approx_eq!(distance_mm, 20.0, 1e-9);

        let (p, op) = bounded(&[], &[]);
        assert!(!op.has_boundary());
        assert!(drawn_boundary(&p, &op).is_ok_and(|d| d.is_none()));
    }

    /// With a boundary the male band reads the narrowest band as its
    /// margin, with no rounding of its own (`BandSource::Drawn`); the
    /// same Inlay with a margin and no boundary reads the margin.
    #[test]
    fn male_band_reads_the_drawn_band_for_its_margin() {
        let (p, op) = bounded(&["i2"], &[[0.0, 0.0, 80.0, 50.0]]);
        let band = male_band(&p, &op).expect("a male band");
        dc_geometry::assert_approx_eq!(band.margin_mm, 10.0, 1e-9);
        dc_geometry::assert_approx_eq!(band.outline_rounding_mm, 0.0, 1e-12);
        assert!(
            matches!(band.source, BandSource::Drawn { narrowest_mm } if (narrowest_mm - 10.0).abs() < 1e-9)
        );

        let ignored = InlayOp {
            margin_mm: Some(6.0),
            outline_rounding_mm: Some(4.0),
            ..op.clone()
        };
        assert_eq!(male_band(&p, &ignored), Some(band));
        let computed = InlayOp {
            boundary: Vec::new(),
            ..ignored
        };
        let band = male_band(&p, &computed).expect("a male band");
        dc_geometry::assert_approx_eq!(band.margin_mm, 6.0, 1e-12);
        assert_eq!(
            band.source,
            BandSource::Computed {
                margin_mm: 6.0,
                outline_rounding_mm: 4.0
            }
        );
    }

    /// #495: an open band is clipped to the stock itself, a V wall the
    /// V-bit's radius inside it (the 90° bit `t2`, 6.35 mm across). The
    /// room a clipping edge needs: the V-bit along the plug's foot with
    /// its radius, `1.5 + 3.175`; with an open edge also the edge pass's
    /// ring, `1.5 + 3.175`, and the flank, `(2.25 + 3)·1` = 5.25; a 1 mm
    /// cut-out's ring, `1.5 + 1` open and `3.175 + 3 + 1` with a V wall.
    #[test]
    fn band_is_clipped_to_the_board() {
        let (p, op) = male(&["t3"]);
        let band = male_band(&p, &op).expect("a male band");
        assert!(band.is_open());
        dc_geometry::assert_approx_eq!(band.v_bit_radius_mm, 3.175, 1e-12);
        dc_geometry::assert_approx_eq!(band.flank_mm, 5.25, 1e-12);
        dc_geometry::assert_approx_eq!(band.board_inset_mm(), 0.0, 1e-12);
        let board = band.board(&p.stock);
        assert_eq!(board.min, dc_geometry::Point::new(0.0, 0.0));
        assert_eq!(
            board.max,
            dc_geometry::Point::new(p.stock.width_mm, p.stock.height_mm)
        );
        dc_geometry::assert_approx_eq!(band.board_room_mm(None), 5.25, 1e-12);
        dc_geometry::assert_approx_eq!(band.board_room_mm(Some(1.0)), 5.25, 1e-12);

        let (p, op) = male(&[]);
        let band = male_band(&p, &op).expect("a male band");
        assert!(!band.is_open());
        dc_geometry::assert_approx_eq!(band.board_inset_mm(), 3.175, 1e-12);
        let board = band.board(&p.stock);
        dc_geometry::assert_approx_eq!(board.min.x, 3.175, 1e-12);
        dc_geometry::assert_approx_eq!(board.max.y, p.stock.height_mm - 3.175, 1e-12);
        dc_geometry::assert_approx_eq!(band.board_room_mm(None), 4.675, 1e-12);
        dc_geometry::assert_approx_eq!(band.board_room_mm(Some(1.0)), 7.175, 1e-12);
    }
}
