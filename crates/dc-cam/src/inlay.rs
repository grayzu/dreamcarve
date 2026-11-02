// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Inlay (`docs/specs/phase-3/design.md`, "Algorithms → Inlay
//! (dc-cam::inlay)"; functional capability 45, acceptance 56 and 57). No
//! PureCutCNC reference: the method is Corbin's Workshop's V-bit inlay
//! (cited in the functional spec), which VCarve's "V-Carve Inlay" also
//! follows.
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
//!   stock top, since the plug above the start plane is never cleared
//!   (security review H4).
//!
//! The two mate: the female's pocket `h` below the stock top is the
//! design inset by `h · tan(α/2)`, and the plug `h` above its start plane
//! is the mirrored design inset by the same, for `h` from 0 to `D − G`
//! (design step 5, the fit property). The plug seats with its start plane
//! at the female's surface and its top `G` above the pocket's floor.

#[cfg(test)]
mod tests;

use crate::error::{CamError, ShapeRef};
use crate::profile::{generate_profile_below, rates_of};
use crate::run::{RunLabel, ToolRun};
use crate::vcarve::{
    self, Carve, Clearing, OpenEdge, Targets, carve_regions, carve_regions_then, checked_v_bit,
    clearing, floor::Floor,
};
use crate::warning::Warning;
use dc_geometry::{
    Contour, EPS_MM, Polyline, Rect, Region, Transform, inset_outers, nearest_point, rounded_band,
    surround,
};
use dc_project::{
    BandEdge, CornerRelief, CutDirection, DefinitionId, Entry, FeatureDefinition, FeatureInstance,
    INLAY_MIN_PLUG_BASE_MM, InlayCutOut, InlayOp, InlaySide, InstanceId, MaleBand, Operation,
    OperationId, ProfileOp, Project, Side as ProfileSide, StartAt, VCarveStrategy, male_band,
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
    for instance_id in targets {
        let instance = project
            .instances
            .iter()
            .find(|i| &i.id == instance_id)
            .ok_or_else(|| CamError::UnknownInstance {
                op: op.clone(),
                instance: instance_id.clone(),
            })?;
        let definition = project
            .definitions
            .get(&instance.definition)
            .ok_or_else(|| CamError::UnknownDefinition {
                instance: instance.id.clone(),
                definition: instance.definition.clone(),
            })?;
        for polyline in definition.profiles.iter().filter(|p| p.is_closed()) {
            let placed = instance.transform.apply_polyline(polyline)?;
            if let Some(b) = placed.bounds() {
                bounds = Some(match bounds {
                    Some(all) => all.including(b.min).including(b.max),
                    None => b,
                });
            }
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

impl Values {
    /// The values of `op`, as validation checks them.
    fn of(op: &InlayOp) -> Result<Self, CamError> {
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
        let margin_mm = match op.side {
            InlaySide::Female => None,
            InlaySide::Male => {
                let margin_mm = op.margin_mm.ok_or_else(|| unset("margin_mm"))?;
                if !(margin_mm.is_finite() && margin_mm > 0.0) {
                    return Err(CamError::InlayMargin {
                        op: op.id.clone(),
                        margin_mm,
                    });
                }
                Some(margin_mm)
            }
        };
        // #448: as validation's `inlay_outline_rounding`, on either side.
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
    /// cut-out runs round (#443); empty for the female.
    pub design: Vec<Region>,
    /// The male's band (#448): its edge and roundings, from
    /// [`dc_project::male_band`]; `None` for the female.
    pub band: Option<MaleBand>,
    /// The male's band edge when a clearing pass cuts it upright (#448).
    pub open_edge: Option<OpenEdge>,
}

/// The side `op` of `project` carves (design steps 2 to 4). Warnings of
/// the design's outlines (an open path, a duplicate) go to `warnings`.
///
/// # Errors
///
/// As [`Values`] (an unset or out-of-range value, `InlayGlueGap`,
/// `InlayMargin`), [`CamError::MissingThickness`], and what
/// [`carve_regions`] refuses.
pub(crate) fn side(
    project: &Project,
    op: &InlayOp,
    warnings: &mut Vec<Warning>,
) -> Result<Side, CamError> {
    let values = Values::of(op)?;
    side_of(project, op, values, warnings)
}

fn side_of(
    project: &Project,
    op: &InlayOp,
    values: Values,
    warnings: &mut Vec<Warning>,
) -> Result<Side, CamError> {
    let thickness_mm = project
        .stock
        .thickness_mm
        .ok_or(CamError::MissingThickness)?;
    let start_plane_z_mm = thickness_mm - values.start_depth_mm();
    let Some(margin_mm) = values.margin_mm else {
        return Ok(Side {
            targets: carve_regions(project, &op.id, &op.targets, warnings)?,
            start_plane_z_mm,
            engaged_from_z_mm: thickness_mm,
            flat_depth_mm: values.flat_depth_mm(),
            mirror_x_mm: None,
            design: Vec::new(),
            band: None,
            open_edge: None,
        });
    };
    let mirror_x_mm = axis_x_mm(project, &op.id, &op.targets)?;
    let mirror = mirror_x_mm.map(Transform::mirror_x_about);
    let design = carve_regions_then(project, &op.id, &op.targets, mirror, warnings)?;
    // #448: the band's edge, closed by `R'`, and whether it is open.
    let band = male_band(project, op);
    let edge_rounding_mm = band.as_ref().map_or(0.0, |b| b.edge_rounding_mm);
    let tolerance_mm = project.flatten_tolerance_mm;
    let regions = surround(&design.regions, margin_mm, edge_rounding_mm, tolerance_mm);
    let open_edge = band.as_ref().filter(|b| b.is_open()).map(|_| OpenEdge {
        band: rounded_band(&design.regions, margin_mm, edge_rounding_mm, tolerance_mm),
        design: design.regions.clone(),
        top_z_mm: thickness_mm,
    });
    let shapes = regions
        .iter()
        .map(|region| bordering_shape(region, &design))
        .collect();
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
        open_edge,
    })
}

/// The design outline surround region `region` borders, to name it by:
/// the one whose first vertex is nearest the region's outlines (the
/// band round a shape has the shape as a hole, a hole of the design is a
/// region of its own).
fn bordering_shape(region: &Region, design: &Targets) -> ShapeRef {
    let outlines: Vec<_> = std::iter::once(region.outer())
        .chain(region.holes())
        .map(dc_geometry::Contour::polyline)
        .collect();
    design
        .contours
        .iter()
        .filter_map(|(shape, contour)| {
            let probe = contour.polyline().vertices().first()?.point();
            let gap_mm = outlines
                .iter()
                .filter_map(|o| nearest_point(o, probe))
                .map(|n| n.gap_mm)
                .reduce(f64::min)?;
            Some((shape, gap_mm))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map_or_else(
            || ShapeRef {
                instance: InstanceId::new(""),
                instance_name: String::new(),
                profile: 0,
            },
            |(shape, _)| shape.clone(),
        )
}

/// The height, mm, of the ridges the V-bit leaves between its floor rings
/// on side `op`, `step_over_fraction · F` as for a V-Carve
/// ([`vcarve::floor_ridge_height_mm`]); `None` when the side has no floor
/// wide enough, no usable V-bit or step-over, or does not resolve.
#[must_use]
pub fn floor_ridge_height_mm(project: &Project, op: &InlayOp) -> Option<f64> {
    let tool = project.tools.iter().find(|t| t.id == op.tool)?;
    let bit = checked_v_bit(&op.id, tool).ok()?;
    let fraction = op
        .step_over_fraction
        .or(tool.step_over_fraction)
        .filter(|f| f.is_finite() && *f > 0.0 && *f <= 1.0)?;
    let side = side(project, op, &mut Vec::new()).ok()?;
    let floor = Floor {
        z_mm: side.start_plane_z_mm - side.flat_depth_mm,
        inset_mm: side.flat_depth_mm * bit.slope,
        step_over_fraction: Some(fraction),
    };
    let tolerance_mm = project.flatten_tolerance_mm;
    let has_floor = side
        .targets
        .regions
        .iter()
        .any(|r| !floor.regions(r, tolerance_mm).is_empty());
    has_floor.then_some(fraction * side.flat_depth_mm)
}

/// Generates one side of the Inlay `op` in `project`, in project space
/// (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness +
/// op.safe_z_mm`): one [`ToolRun`] per clearing pass that reaches some of
/// the floor, then [`RunLabel::Main`] with the operation's V-bit, as a
/// V-Carve's ([`vcarve::generate_vcarve`]).
///
/// The female carves the design (the targets' closed outlines nested
/// even-odd) from the stock top with a flat depth of `depth_mm`. The male
/// carves the surround of the design mirrored about its targets' centre
/// line ([`male_mirror`]), `margin_mm` wide, from a start plane
/// `depth_mm − glue_gap_mm` below the stock top with a flat depth of
/// `plug_depth_mm`, engaged from the stock top; it warns
/// [`Warning::InlayMaleMirrored`] with the line's X, and
/// [`Warning::InlayPlugBaseThin`] when less than
/// [`INLAY_MIN_PLUG_BASE_MM`] of board is left under its floor.
///
/// # Errors
///
/// An unknown tool, a tool that is not a usable V-bit, a missing rate or
/// thickness, a safe Z that is not positive; [`CamError::InlayValueUnset`],
/// [`CamError::InlayValueRange`], [`CamError::InlayGlueGap`],
/// [`CamError::InlayMargin`]; [`CamError::VBitConeExceeded`] when the
/// flank's engagement from the stock top, `D` or `D − G + P`, is past the
/// bit's cone (security review H4); a clearing pass that cannot run; and
/// what [`vcarve::blocks`] and the V-Carve's sequencing and cone test
/// refuse.
#[must_use = "the toolpath is the whole point"]
pub fn generate_inlay(
    project: &Project,
    op: &InlayOp,
) -> Result<(Vec<ToolRun>, Vec<Warning>), CamError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: op.tool.clone(),
        })?;
    let bit = checked_v_bit(&op.id, tool)?;
    rates_of(tool)?;
    if !(op.safe_z_mm.is_finite() && op.safe_z_mm > 0.0) {
        return Err(CamError::InvalidSafeZ {
            op: op.id.clone(),
            safe_z_mm: op.safe_z_mm,
        });
    }
    let values = Values::of(op)?;
    let cut_out = checked_cut_out(project, op, values)?;
    let passes = clearing::Passes::of_inlay(op, values.start_depth_mm());
    let clearing_tools = clearing::checked_tools(project, &passes, true)?;
    let mut warnings = Vec::new();
    let side = side_of(project, op, values, &mut warnings)?;
    // Step 1: the flank is engaged from the stock top, so the plug above
    // the male's start plane counts (security review H4).
    let engaged_mm = values.engagement_mm();
    if engaged_mm > bit.cone_mm + EPS_MM {
        return Err(CamError::VBitConeExceeded {
            op: op.id.clone(),
            shape: Box::new(
                side.targets
                    .shapes
                    .first()
                    .cloned()
                    .unwrap_or_else(|| ShapeRef {
                        instance: InstanceId::new(""),
                        instance_name: String::new(),
                        profile: 0,
                    }),
            ),
            depth_mm: engaged_mm,
            cone_mm: bit.cone_mm,
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
    // #448: clearing passes that leave the band's edge a V wall.
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
        // #452: the male's walls along the plug reach every corner.
        design: values.margin_mm.map(|_| design.clone()),
    };
    let mut runs = clearing::runs(
        project,
        &passes,
        &carve,
        &clearing_tools,
        bit.slope,
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
    });
    // #443: the cut-out last, so the piece is freed only once carved.
    if let (Some((cut_out, band, diameter_mm)), Some(thickness_mm)) =
        (cut_out, project.stock.thickness_mm)
    {
        let paths: Vec<Polyline> =
            cut_out_paths(&design, &band, diameter_mm, project.flatten_tolerance_mm)
                .into_iter()
                .map(Contour::into_polyline)
                .collect();
        let (cut_project, profile) = cut_out_profile(project, op, cut_out, &paths, thickness_mm);
        let (toolpath, cut_warnings) =
            generate_profile_below(&cut_project, &profile, values.engagement_mm())?;
        warnings.extend(cut_warnings);
        runs.push(ToolRun {
            label: RunLabel::CutOut(cut_out.tool.clone()),
            tool: cut_out.tool.clone(),
            toolpath,
        });
    }
    Ok((runs, warnings))
}

/// The cut-out of `op` (#443), checked as validation checks it, with the
/// male's band ([`dc_project::male_band`], #448) and its tool's diameter,
/// mm: `None` without one.
///
/// # Errors
///
/// [`CamError::InlayCutOutNeedsMale`] on a female;
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
        return Err(CamError::InlayCutOutNeedsMale { op: op.id.clone() });
    };
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == cut_out.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: cut_out.tool.clone(),
        })?;
    if !tool.cutter.is_flat() {
        return Err(CamError::CutterNotFlat {
            op: op.id.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            cutter: tool.cutter.describe(),
        });
    }
    if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
        return Err(CamError::InvalidToolDiameter {
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            diameter_mm: tool.diameter_mm,
        });
    }
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

/// A male Inlay's cut-out paths for an end mill `diameter_mm` wide (#443,
/// #448, #461; design "Inlay", step 6): the outer outlines of the band's
/// edge `B` round its mirrored `design` ([`dc_geometry::rounded_band`],
/// closed by the edge's rounding) inset by [`MaleBand::cut_out_inset_mm`]
/// ([`dc_geometry::inset_outers`]), so the tool's edge runs along the
/// outer edge of the band's flat floor: on `B` when the edge is open,
/// `P·tan(α/2)` inside it, at the V wall's foot, otherwise. Above the
/// floor it cuts only what the carve removed. A hole of the design is not
/// cut out: it stays in the plug.
#[must_use]
pub fn cut_out_paths(
    design: &[Region],
    band: &MaleBand,
    diameter_mm: f64,
    tolerance_mm: f64,
) -> Vec<Contour> {
    let edge = rounded_band(design, band.margin_mm, band.edge_rounding_mm, tolerance_mm);
    inset_outers(&edge, band.cut_out_inset_mm(diameter_mm), tolerance_mm)
}

/// The On line Profile a male Inlay's cut-out is (#443), and a copy of
/// `project` that holds it: `paths` (closed, project space) as the
/// profiles of one definition placed in place, which the Profile targets
/// with the cut-out's tool, tabs and through-cut allowance, through the
/// stock `thickness_mm` thick, climb, plunging, with the Inlay's id,
/// name, machining order and safe Z. The Profile is the copy's only
/// operation, so its tab request is placed as `dreamcarve tabs place`
/// places it ([`crate::placed_tabs::place`]); the safety pass builds the
/// same Profile itself, on paths it derives itself, to place its tabs.
#[must_use]
pub fn cut_out_profile(
    project: &Project,
    op: &InlayOp,
    cut_out: &InlayCutOut,
    paths: &[Polyline],
    thickness_mm: f64,
) -> (Project, ProfileOp) {
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
            profiles: paths.to_vec(),
        },
    );
    copy.instances.push(FeatureInstance {
        id: instance.clone(),
        definition,
        transform: Transform::IDENTITY,
        name: format!("{} cut-out", op.name),
        visible: true,
        locked: false,
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
        tabs: cut_out.tabs.clone(),
        machining_order: op.machining_order,
        entry: Entry::Plunge,
        lead: None,
        corner_relief: CornerRelief::None,
        last_pass: None,
        start: StartAt::Auto,
    };
    copy.operations = vec![Operation::Profile(profile.clone())];
    (copy, profile)
}
