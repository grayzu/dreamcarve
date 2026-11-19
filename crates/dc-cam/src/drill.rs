// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/drilling.ts
//   src/engine/toolpaths/drillingCountersink.test.ts
//   src/engine/toolpaths/drillingRetract.test.ts
//   src/engine/toolpaths/toolpaths.test.ts (drilling ordering tests)
//   src/engine/toolpaths/camOperationSmoke.test.ts (drilling cases)

//! Drill toolpaths (`docs/specs/phase-2/design.md`, "Algorithms → Drill";
//! functional spec capability 14, safety rule S15).
//!
//! What changed in the port: a hole is any closed target whose outline is
//! a circle within the project's flatten tolerance
//! ([`dc_geometry::as_circle`]), not a `circle` feature kind; the
//! chip-break gap is the operation's `gap_mm`, not a constant in project
//! units; a dwell is kept as a [`crate::MoveKind::Dwell`] move when the cycle is
//! written as plain moves (upstream loses it); pecks count from the
//! retract plane, as a controller's `G83`/`G73` counts them from `R`,
//! not from the stock top (#153); holes are visited nearest
//! neighbour from the first circle in drawing order (D5), not from the
//! origin, by [`crate::sequence`](fn@crate::sequence); there is no clamp
//! check, and helical boring is its own operation ([`crate::bore`]).
//!
//! The countersink cycle (format 21; `docs/specs/phase-3/design.md`,
//! "Algorithms → Countersink"; upstream `countersinkTipDepth`,
//! `resolveCountersinkDepth`, `emitCountersinkPlunge` and the
//! mouth-versus-hole skip) is a simple plunge whose depth is derived from
//! the V-bit's angle, measured from a start plane `start_depth_mm` below
//! the stock top rather than from a feature's own top face; upstream's
//! tool `maxCutDepth` check is the cone-height check, from the start
//! plane only where an earlier Pocket at that depth has cleared it.
//! Nothing here depends on Y-up or Y-down: a drill moves only
//! in Z at each centre, and distances are symmetric under the flip.

use crate::block::CutBlock;
use crate::context::{self, OpContext, check_diameter, check_safe_z, plunge_rate_of, tool_of};
use crate::error::{CamError, ShapeRef, at};
use crate::passes::pass_z_mm;
use crate::sequence::{SequenceInput, plan, sequence};
use crate::tabs::LapRates;
use crate::toolpath::{Move, Toolpath};
use crate::warning::Warning;
use dc_geometry::{Point, Point3, as_circle};
use dc_project::{
    COUNTERSINK_TOLERANCE_MM, DrillCycle, DrillOp, InstanceId, Operation, OperationId, PeckRetract,
    Project, Tool, ToolKind, cleared_start_plane, countersink_depth_mm,
};

/// How far above the depth already drilled a full-retract peck re-enters
/// the hole by rapid, mm. The tool re-enters only through air it has
/// already cut (S15); the next peck feeds from there.
pub const PECK_REENTRY_MM: f64 = 0.5;

/// The canned cycle a hole's plain moves are equivalent to, for the
/// emitter (`docs/specs/phase-2/design.md`, "Canned cycles").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CannedKind {
    /// Simple, no dwell.
    G81,
    /// Simple with a dwell at the bottom.
    G82,
    /// Peck with a full retract, no dwell.
    G83,
    /// Peck with a chip-break retract, no dwell.
    G73,
    /// No canned cycle describes it (a peck with a dwell): always written
    /// as plain moves.
    Expanded,
}

/// One drilled hole, as the emitter may write it as a canned cycle. Z
/// values are absolute project Z, mm (stock bottom at 0).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DrillHole {
    /// The hole centre, mm, project space.
    pub centre: Point,
    /// The hole top: the stock top, or a countersink's start plane.
    pub top_z_mm: f64,
    /// The hole bottom.
    pub bottom_z_mm: f64,
    /// The retract plane (`R`): `min(safe Z, stock top + retract_mm)`.
    pub retract_z_mm: f64,
    /// The equivalent canned cycle; always [`CannedKind::Expanded`] for a
    /// countersink, which no canned cycle describes.
    pub cycle: CannedKind,
    /// The peck depth, mm, for a peck cycle.
    pub peck_mm: Option<f64>,
    /// The dwell at the bottom of every plunge, seconds, when set.
    pub dwell_s: Option<f64>,
}

/// Drill output: the toolpath, which is the authority (S11), and one
/// record per hole in the order the toolpath drills them.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DrillToolpath {
    /// The moves.
    pub toolpath: Toolpath,
    /// The holes, in cut order.
    pub holes: Vec<DrillHole>,
}

/// Generates the Drill toolpath for `op` in `project`, in project space
/// (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness + op.safe_z_mm`).
///
/// Every closed profile of every target instance, placed by its
/// transform, whose outline is a circle within the project's flatten
/// tolerance is a hole; anything else warns
/// [`Warning::DrillTargetNotCircle`] and is skipped. A hole narrower than
/// the tool by more than the flatten tolerance is skipped with
/// [`Warning::DrillHoleTooSmall`], since a Drill never moves sideways and
/// the hole would come out the tool's size (maintainer decision
/// 2026-10-24); with `op.allow_tool_wider_than_hole` it is drilled anyway
/// and warns [`Warning::DrillToolWiderThanHole`]. With no hole left the
/// toolpath is empty and [`Warning::DrillNoHoles`] says so. Each hole is
/// drilled from the stock top to `depth_mm` (S1 with the operation's
/// through-cut allowance):
///
/// - the tool arrives at safe Z and rapids down to the retract plane
///   `min(safe Z, top + retract_mm)` (a negative `retract_mm` is raised to
///   the top with [`Warning::DrillRetractBelowStockTop`]);
/// - it plunges at the tool's plunge rate, in one plunge for a simple
///   cycle or in pecks of the peck depth (the operation's, else the tool's
///   step-down), waiting `dwell_s` at the bottom of every plunge as a
///   [`crate::MoveKind::Dwell`] move. Pecks count from the retract plane,
///   as a controller's `G83`/`G73` counts them from `R` (#153): peck `k`
///   ends at `max(bottom, retract plane − k·peck)`, so the first removes
///   the peck less the retract height (nothing, if the retract height is
///   the larger), the last may be shorter, and none cuts more than a peck
///   below the depth already drilled;
/// - between pecks it rapids up to the retract plane and back down to
///   [`PECK_REENTRY_MM`] above the depth reached (full retract), or up by
///   the chip-break gap (capped at the retract plane), and feeds on from
///   there; every rapid down ends in air already cut (S15);
/// - it ends with a rapid to safe Z, where the `G98` return of a canned
///   cycle puts it.
///
/// Holes are visited by [`crate::sequence`](fn@crate::sequence): nearest neighbour from the
/// first hole in drawing order, ties by drawing order, linked at safe Z.
/// A peck that reaches the bottom from the retract plane drills the hole
/// in one plunge, with [`Warning::DrillPeckDeeperThanHole`].
///
/// A countersink (format 21) plunges once at each circle's centre, from
/// the same retract plane above the stock top, to `diameter_mm / (2 ·
/// tan(α/2))` below its start plane, `start_depth_mm` below the stock top;
/// `depth_mm` is not used. A circle at least as wide as the mouth is
/// skipped with [`Warning::CountersinkMouthNotWiderThanHole`], and the
/// narrower-hole rule above does not apply. Its holes are recorded as
/// [`CannedKind::Expanded`]. It fails without a toolpath when the tool is
/// not a V-bit with an angle in (0°, 180°), the mouth is not above 0 or
/// is wider than the bit, the start depth is not from 0 to the stock
/// thickness, the start plane below the stock top is neither cleared by
/// an earlier Pocket or V-Carve floor ([`dc_project::cleared_start_plane`])
/// nor vouched for,
/// or the flanks would engage past the cone.
///
/// Fails without a toolpath when the plunge rate, the stock thickness, the
/// depth, or a peck's step-down is missing; when the depth passes the
/// stock bottom plus allowance; when the chip-break gap is not positive or
/// the dwell is negative; when an id does not resolve or a target cannot
/// be placed; or when the cutter would leave the stock
/// ([`CamError::OffStock`]).
#[must_use = "the toolpath is the whole point"]
pub fn generate_drill(
    project: &Project,
    op: &DrillOp,
) -> Result<(DrillToolpath, Vec<Warning>), CamError> {
    drill(project, op).map_err(|e| e.for_op(&op.id))
}

/// [`generate_drill`], before a defect it finds ([`CamError::Internal`])
/// names `op`.
fn drill(project: &Project, op: &DrillOp) -> Result<(DrillToolpath, Vec<Warning>), CamError> {
    let tool = tool_of(project, &op.id, &op.tool)?;
    let plunge_mm_min = plunge_rate_of(tool)?;
    check_diameter(tool)?;
    check_safe_z(&op.id, op.safe_z_mm)?;
    if !(op.dwell_s.is_finite() && op.dwell_s >= 0.0) {
        return Err(CamError::InvalidDrillSetting {
            op: op.id.clone(),
            field: "dwell_s",
            value: op.dwell_s,
        });
    }
    let thickness_mm = context::thickness_mm(project)?;
    let cycle = Cycle::of(project, op, tool, thickness_mm)?;
    // The hole's top and its depth below it: the stock top and the
    // operation's depth, or a countersink's start plane and derived depth.
    let (top_z_mm, depth_mm) = match &cycle.countersink {
        Some(sink) => (thickness_mm - sink.start_depth_mm, sink.depth_mm),
        None => (thickness_mm, context::depth_mm(&op.id, op.depth_mm)?),
    };
    // The hole bottom, with the depth checked against the stock bottom
    // plus the allowance (S1) as for every generator.
    let below_stock_top_mm = thickness_mm - top_z_mm + depth_mm;
    let bottom_z_mm = pass_z_mm(
        &op.id,
        thickness_mm,
        below_stock_top_mm,
        below_stock_top_mm,
        op.through_cut_allowance_mm,
    )?
    .last()
    .copied()
    .ok_or_else(|| CamError::internal("a hole's checked depth has no pass"))?;
    // Holes have no cleared area, so no stay-down link is ever cut at the
    // feed rate; the plunge rate is the only rate a Drill uses.
    let rates = LapRates {
        feed_mm_min: plunge_mm_min,
        plunge_mm_min,
    };
    let ctx = OpContext::new(project, &op.id, tool, rates, thickness_mm, op.safe_z_mm);
    let safe_z_mm = ctx.safe_z_mm;
    let mut warnings = Vec::new();
    if op.retract_mm < 0.0 {
        warnings.push(Warning::DrillRetractBelowStockTop {
            op: op.id.clone(),
            requested_mm: op.retract_mm,
            used_mm: 0.0,
        });
    }
    // Measured from the stock top whatever the hole's top, so the rapid
    // down to it never ends in the stock: a countersink on a Pocket's
    // floor plunges through the Pocket's air at the plunge rate (security
    // review H3).
    let retract_z_mm = (thickness_mm + op.retract_mm.max(0.0)).min(safe_z_mm);
    // Plunge levels, shallowest first; a simple cycle is one level. Pecks
    // count from the retract plane, as a controller's `G83`/`G73` counts
    // them from `R` (#153), so a canned cycle and these moves are the
    // same motion for any retract height. The pass rule is the one every
    // generator uses, from `R` instead of the stock top; `R` is at or
    // above the top, so no peck cuts deeper than a peck into material.
    let mut levels = match cycle.peck_mm {
        None => vec![bottom_z_mm],
        Some(peck_mm) => pass_z_mm(
            &op.id,
            retract_z_mm,
            retract_z_mm - bottom_z_mm,
            peck_mm,
            op.through_cut_allowance_mm,
        )?,
    };
    // The last level is the hole's bottom exactly.
    *levels
        .last_mut()
        .ok_or_else(|| CamError::internal("a hole has no level to drill to"))? = bottom_z_mm;
    if let Some(peck_mm) = cycle.peck_mm
        && levels.len() == 1
    {
        warnings.push(Warning::DrillPeckDeeperThanHole {
            op: op.id.clone(),
            peck_mm,
            depth_mm,
        });
    }
    let dwell_s = (op.dwell_s > 0.0).then_some(op.dwell_s);

    let fits = match &cycle.countersink {
        Some(sink) => Fits::Mouth(sink.mouth_mm),
        None => Fits::Tool(tool.diameter_mm),
    };
    let holes = resolve_holes(project, op, fits, &mut warnings)?;
    let blocks: Vec<CutBlock> = holes
        .iter()
        .map(|(shape, centre)| {
            let at = |z: f64| Point3::new(centre.x, centre.y, z);
            let mut cuts = Vec::with_capacity(levels.len() * 4);
            for (k, &z) in levels.iter().enumerate() {
                cuts.push(Move::linear(at(z), plunge_mm_min));
                if let Some(seconds) = dwell_s {
                    cuts.push(Move::dwell(at(z), seconds));
                }
                if k + 1 == levels.len() {
                    break;
                }
                // The next level, if any, feeds on from where these leave
                // the tool, always at or above `z`: air already cut.
                match &cycle.retract {
                    PeckRetract::Full => {
                        cuts.push(Move::rapid(at(retract_z_mm)));
                        cuts.push(Move::rapid(at((z + PECK_REENTRY_MM).min(retract_z_mm))));
                    }
                    PeckRetract::ChipBreak { gap_mm } => {
                        cuts.push(Move::rapid(at((z + *gap_mm).min(retract_z_mm))));
                    }
                }
            }
            cuts.push(Move::rapid(at(safe_z_mm)));
            CutBlock {
                shape: shape.clone(),
                pass: 0,
                z_mm: bottom_z_mm,
                descent_from: *centre,
                descent_from_z_mm: safe_z_mm,
                // The rapid down to the retract plane, above the stock:
                // the one rapid below safe Z the sequencer passes through
                // (design spec, "Algorithms → Sequence", step 5).
                descent: vec![Move::rapid(at(retract_z_mm))],
                entry: *centre,
                // The descent ends at the retract plane, where the first
                // feed down begins.
                entry_z_mm: retract_z_mm,
                cuts,
                exit: *centre,
                frees: false,
                cleared: None,
                group: 0,
            }
        })
        .collect();
    let input = SequenceInput::new(&ctx, blocks);
    let records = plan(&input)?
        .into_iter()
        .map(|i| {
            let block = at(
                &input.blocks,
                i,
                "the cut order names a hole that is not there",
            )?;
            Ok(DrillHole {
                centre: block.entry,
                top_z_mm,
                bottom_z_mm: block.z_mm,
                retract_z_mm,
                cycle: cycle.canned(dwell_s.is_some()),
                peck_mm: cycle.peck_mm,
                dwell_s,
            })
        })
        .collect::<Result<_, CamError>>()?;
    let toolpath = sequence(&input)?;
    // #62: the cutter must stay on the stock.
    ctx.check_on_stock(&toolpath, tool.cut_radius_mm())?;
    Ok((
        DrillToolpath {
            toolpath,
            holes: records,
        },
        warnings,
    ))
}

/// The operation's cycle, resolved: the peck depth in effect, how the
/// tool retracts between pecks, and a countersink's settings.
#[derive(Debug, Clone)]
struct Cycle {
    /// `None` for a simple cycle or a countersink.
    peck_mm: Option<f64>,
    /// How a peck retracts; unused for a simple cycle or a countersink.
    retract: PeckRetract,
    /// A countersink's mouth and derived depth (format 21).
    countersink: Option<Countersink>,
}

/// A countersink, resolved and checked (`docs/specs/phase-3/design.md`,
/// "Algorithms → Countersink", step 1).
#[derive(Debug, Clone, Copy)]
struct Countersink {
    /// The mouth's diameter at the start plane, mm.
    mouth_mm: f64,
    /// The start plane's depth below the stock top, mm.
    start_depth_mm: f64,
    /// The tip's depth below the start plane, mm.
    depth_mm: f64,
}

impl Countersink {
    /// Port of upstream `resolveCountersinkDepth`, with the start plane
    /// and the cone check of DreamCarve: the tool is a V-bit with an
    /// angle in (0°, 180°); the mouth is above 0 and no wider than the
    /// bit (within [`COUNTERSINK_TOLERANCE_MM`]); the depth is
    /// [`countersink_depth_mm`]; the start depth is from 0 to the stock
    /// thickness, and below the stock top an earlier floor holds the bit
    /// at every hole, an enabled Pocket's or an enabled V-Carve's flat
    /// floor (#426; [`cleared_start_plane`]), or the operation vouches for
    /// the plane (security review M2); and the flanks engage no deeper
    /// than the cone, counting the start depth unless that floor is at it
    /// (H4). Upstream's tool `maxCutDepth` check is this
    /// cone check: DreamCarve tools have none. Every check fails the
    /// whole operation, so nothing is cut at a guessed depth.
    fn of(
        project: &Project,
        op: &DrillOp,
        tool: &Tool,
        thickness_mm: f64,
        (mouth_mm, start_depth_mm, cleared_externally): (f64, f64, bool),
    ) -> Result<Self, CamError> {
        let ToolKind::VBit { included_angle_deg } = tool.cutter else {
            return Err(CamError::CountersinkNeedsVBit {
                op: op.id.clone(),
                tool: tool.name.clone(),
                tool_id: tool.id.clone(),
                cutter: tool.cutter.describe(),
            });
        };
        let angle_range = || CamError::VBitAngleRange {
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            angle_deg: included_angle_deg,
        };
        if !(included_angle_deg > 0.0 && included_angle_deg < 180.0) {
            return Err(angle_range());
        }
        if !(mouth_mm.is_finite() && mouth_mm > 0.0) {
            return Err(CamError::CountersinkDiameterPositive {
                op: op.id.clone(),
                diameter_mm: mouth_mm,
            });
        }
        if mouth_mm - tool.diameter_mm > COUNTERSINK_TOLERANCE_MM {
            return Err(CamError::CountersinkMouthExceedsTool {
                op: op.id.clone(),
                mouth_mm,
                tool_mm: tool.diameter_mm,
            });
        }
        let depth_mm =
            countersink_depth_mm(mouth_mm, included_angle_deg).ok_or_else(angle_range)?;
        let cone_mm =
            countersink_depth_mm(tool.diameter_mm, included_angle_deg).ok_or_else(angle_range)?;
        if !(start_depth_mm.is_finite() && (0.0..=thickness_mm).contains(&start_depth_mm)) {
            return Err(CamError::InvalidStartDepth {
                op: op.id.clone(),
                start_depth_mm,
            });
        }
        let mut engaged_mm = depth_mm;
        if start_depth_mm > 0.0 {
            let cleared = cleared_start_plane(project, &Operation::Drill(op.clone()));
            if cleared.is_none() && !cleared_externally {
                return Err(CamError::StartDepthUncleared {
                    op: op.id.clone(),
                    start_depth_mm,
                });
            }
            if !cleared.is_some_and(|plane| plane.is_at(start_depth_mm)) {
                engaged_mm += start_depth_mm;
            }
        }
        if engaged_mm - cone_mm > COUNTERSINK_TOLERANCE_MM {
            return Err(CamError::CountersinkConeExceeded {
                op: op.id.clone(),
                engaged_mm,
                cone_mm,
            });
        }
        Ok(Self {
            mouth_mm,
            start_depth_mm,
            depth_mm,
        })
    }
}

impl Cycle {
    fn of(
        project: &Project,
        op: &DrillOp,
        tool: &Tool,
        thickness_mm: f64,
    ) -> Result<Self, CamError> {
        match &op.cycle {
            DrillCycle::Simple => Ok(Self {
                peck_mm: None,
                retract: PeckRetract::Full,
                countersink: None,
            }),
            DrillCycle::Countersink {
                diameter_mm,
                start_depth_mm,
                start_plane_cleared_externally,
            } => Ok(Self {
                peck_mm: None,
                retract: PeckRetract::Full,
                countersink: Some(Countersink::of(
                    project,
                    op,
                    tool,
                    thickness_mm,
                    (
                        *diameter_mm,
                        *start_depth_mm,
                        *start_plane_cleared_externally,
                    ),
                )?),
            }),
            DrillCycle::Peck {
                peck_depth_mm,
                retract,
            } => {
                if let PeckRetract::ChipBreak { gap_mm } = retract
                    && !(gap_mm.is_finite() && *gap_mm > 0.0)
                {
                    return Err(CamError::InvalidDrillSetting {
                        op: op.id.clone(),
                        field: "gap_mm",
                        value: *gap_mm,
                    });
                }
                let peck_mm = context::step_down_mm(&op.id, *peck_depth_mm, tool)?;
                Ok(Self {
                    peck_mm: Some(peck_mm),
                    retract: retract.clone(),
                    countersink: None,
                })
            }
        }
    }

    /// The canned cycle these moves are equivalent to (design spec,
    /// "Algorithms → Drill", step 6). A countersink has none, so it is
    /// always written as its moves (format 21, S18): no `G8x` cycle can
    /// say its depth is derived.
    fn canned(&self, dwells: bool) -> CannedKind {
        if self.countersink.is_some() {
            return CannedKind::Expanded;
        }
        match (self.peck_mm, &self.retract, dwells) {
            (None, _, false) => CannedKind::G81,
            (None, _, true) => CannedKind::G82,
            (Some(_), PeckRetract::Full, false) => CannedKind::G83,
            (Some(_), PeckRetract::ChipBreak { .. }, false) => CannedKind::G73,
            (Some(_), _, true) => CannedKind::Expanded,
        }
    }
}

/// A target profile and its circle, centre and radius, if it is one.
pub(crate) type TargetCircle = (ShapeRef, Option<(Point, f64)>);

/// Every profile of every target of `op_id`, in drawing order (target
/// order, then profile order), with its circle: the centre and radius of
/// a closed profile whose outline, placed by its instance's transform, is
/// a circle within the project's flatten tolerance, and `None` for any
/// other profile. Drill and Bore find their holes this way.
pub(crate) fn circles_of(
    project: &Project,
    op_id: &OperationId,
    targets: &[InstanceId],
) -> Result<Vec<TargetCircle>, CamError> {
    let mut out = Vec::new();
    for instance_id in targets {
        let instance = project
            .instances
            .iter()
            .find(|i| &i.id == instance_id)
            .ok_or_else(|| CamError::UnknownInstance {
                op: op_id.clone(),
                instance: instance_id.clone(),
            })?;
        let definition = project
            .definitions
            .get(&instance.definition)
            .ok_or_else(|| CamError::UnknownDefinition {
                instance: instance.id.clone(),
                definition: instance.definition.clone(),
            })?;
        for (profile, polyline) in definition.profiles.iter().enumerate() {
            let shape = ShapeRef {
                instance: instance.id.clone(),
                instance_name: instance.name.clone(),
                profile,
            };
            let circle = if polyline.is_closed() {
                let placed = instance
                    .transform
                    .apply_polyline(polyline)
                    .map_err(|source| CamError::InvalidTarget {
                        shape: shape.clone(),
                        source,
                    })?;
                as_circle(&placed, project.flatten_tolerance_mm)
            } else {
                None
            };
            out.push((shape, circle));
        }
    }
    Ok(out)
}

/// Which circles a Drill cuts, by their size.
#[derive(Debug, Clone, Copy)]
enum Fits {
    /// A simple or peck Drill with a tool this wide, mm: a narrower hole
    /// is skipped unless the operation allows it.
    Tool(f64),
    /// A countersink with a mouth this wide, mm: a hole at least as wide
    /// is skipped (upstream's mouth-versus-hole rule). Its bit is wider
    /// than the hole by design, so the tool's width is no rule.
    Mouth(f64),
}

/// The holes of `op`, in drawing order (target order, then profile order):
/// each circular closed profile, placed by its instance's transform, with
/// its centre. Other profiles warn and are skipped. A hole narrower than
/// the tool by more than the flatten tolerance warns and is skipped
/// (maintainer decision 2026-10-24), unless the operation allows it, when
/// it is drilled at the tool's size and warns. For a countersink, a hole
/// at least as wide as the mouth, less upstream's `1e-9`, warns and is
/// skipped instead. No hole left warns once.
fn resolve_holes(
    project: &Project,
    op: &DrillOp,
    fits: Fits,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<(ShapeRef, Point)>, CamError> {
    let mut holes = Vec::new();
    for (shape, circle) in circles_of(project, &op.id, &op.targets)? {
        let Some((centre, radius_mm)) = circle else {
            warnings.push(Warning::DrillTargetNotCircle {
                op: op.id.clone(),
                shape,
            });
            continue;
        };
        let hole_mm = 2.0 * radius_mm;
        let tool_diameter_mm = match fits {
            Fits::Tool(tool_diameter_mm) => tool_diameter_mm,
            Fits::Mouth(mouth_mm) => {
                // Port of upstream's skip: the cone's rim would sit inside
                // the bore, with no seat to cut.
                if mouth_mm - hole_mm <= COUNTERSINK_TOLERANCE_MM {
                    warnings.push(Warning::CountersinkMouthNotWiderThanHole {
                        op: op.id.clone(),
                        shape,
                        mouth_mm,
                        hole_mm,
                    });
                } else {
                    holes.push((shape, centre));
                }
                continue;
            }
        };
        // The outline was taken for a circle within the flatten
        // tolerance, so its diameter is known no closer than that: a
        // hole drawn at the tool's size is drilled.
        if tool_diameter_mm > hole_mm + project.flatten_tolerance_mm {
            if !op.allow_tool_wider_than_hole {
                warnings.push(Warning::DrillHoleTooSmall {
                    op: op.id.clone(),
                    shape,
                    hole_mm,
                    tool_mm: tool_diameter_mm,
                });
                continue;
            }
            warnings.push(Warning::DrillToolWiderThanHole {
                op: op.id.clone(),
                shape: shape.clone(),
                hole_mm,
                tool_mm: tool_diameter_mm,
            });
        }
        holes.push((shape, centre));
    }
    if holes.is_empty() {
        warnings.push(Warning::DrillNoHoles { op: op.id.clone() });
    }
    Ok(holes)
}

#[cfg(test)]
mod tests;
