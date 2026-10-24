// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/drilling.ts (the `helical` drill type)
//   src/engine/toolpaths/entry.ts (`emitCenterLockedCircularBore`)

//! Bore toolpaths (`docs/specs/phase-2/design.md`, "Algorithms → Bore";
//! functional spec capability 14a; format 18, maintainer decision
//! 2026-10-24): each circular target bigger than the end mill is cut to
//! its drawn size by a helix down inside the hole, then one full circle at
//! the depth, as Fusion's "2D Bore".
//!
//! What changed in the port: a Bore is its own operation, not a Drill
//! cycle. The helix falls a pitch per turn (Fusion's "Pitch", the tool's
//! step-down by default), not at a ramp angle. It starts
//! [`crate::entry::ENTRY_CLEARANCE_MM`] above the stock top over its own
//! start (#214), not at a retract height reached over the hole's centre.
//! It has at least [`HELIX_SEGMENTS_PER_REV`] chords a turn, more when a
//! chord would bow in by more than the flatten tolerance. The bottom
//! revolution is four exact arcs, not 48 chords, and the tool then moves to
//! the centre before it lifts (Fusion's "Lead To Center"). A radial
//! allowance is added. A hole is too small when the helix radius is under
//! [`BORE_MIN_HELIX_RADIUS_MM`], not when it is no wider than the tool.
//! The helix sense is [`helix_is_ccw`], re-derived for Y-up (ADR-0001),
//! not upstream's `helixAngularDirection`. Upstream's too-large bound,
//! twice the tool's diameter, is kept: past it the helix leaves a core.

use crate::block::CutBlock;
use crate::drill::circles_of;
use crate::entry::{
    CutSide, HELIX_SEGMENTS_PER_REV, MAX_DESCENT_MOVES, descent_start_z_mm, helix_is_ccw,
    plunge_limited_feed_mm_min,
};
use crate::error::{CamError, ShapeRef};
use crate::passes::pass_z_mm;
use crate::profile::rates_of;
use crate::sequence::{SequenceInput, clearance_z_mm, sequence};
use crate::stock::{stock_overhang, swept_bounds};
use crate::toolpath::{Move, MoveKind, Toolpath};
use crate::warning::Warning;
use dc_geometry::{EPS_MM, Point, Point3, Vec2};
use dc_project::{BoreOp, MachiningOrder, Project};
use std::f64::consts::{FRAC_PI_2, PI, TAU};

/// The least helix radius a Bore cuts, mm. At [`HELIX_SEGMENTS_PER_REV`]
/// chords a turn a helix this small has chords `2 · 0.05 · sin(3.75°)` =
/// 0.0065 mm long, more than √2 units of the coarsest place a
/// post-processor writes in inches (0.0001 in), so every chord's ends
/// round apart, as `entry` keeps its helix (#265). A hole that leaves less
/// is within 0.1 mm of the bit, which is a Drill's job.
pub const BORE_MIN_HELIX_RADIUS_MM: f64 = 0.05;

/// One hole a Bore cuts: its shape, centre and helix radius, mm.
struct Hole {
    shape: ShapeRef,
    centre: Point,
    helix_radius_mm: f64,
}

/// Generates the Bore toolpath for `op` in `project`, in project space
/// (Y-up, mm, stock bottom at Z = 0, safe Z at `thickness + op.safe_z_mm`).
///
/// Every closed profile of every target instance, placed by its
/// transform, whose outline is a circle within the project's flatten
/// tolerance is a hole (Drill's test); anything else warns
/// [`Warning::BoreTargetNotCircle`] and is skipped. With hole radius `ρ`,
/// tool radius `r` and allowance `a`, the helix radius is `R = ρ − r − a`:
///
/// - under [`BORE_MIN_HELIX_RADIUS_MM`] the hole is skipped with
///   [`Warning::BoreNoRoom`];
/// - when `R − r` is more than the flatten tolerance the helix would leave
///   a core of diameter `2(R − r)` standing, loose in a through hole, so the
///   hole is skipped with [`Warning::BoreLeavesCore`];
/// - with no hole left the toolpath is empty and [`Warning::BoreNoHoles`]
///   says so.
///
/// Each hole is cut from [`crate::entry::ENTRY_CLEARANCE_MM`] above the
/// stock top, reached by a rapid straight down over the helix start, to
/// `depth_mm` (S1 with the operation's through-cut allowance): a helix of
/// radius `R` about the hole's centre, starting on +X from it and turning
/// counter-clockwise for climb, clockwise for conventional
/// ([`helix_is_ccw`]), falls `pitch_mm` (the tool's step-down when unset)
/// per turn in straight chords, each at the cutting feed scaled so it falls
/// no faster than the plunge rate (S10). At the depth it cuts one full
/// circle as four 90° arcs at the cutting feed, and moves to the centre
/// before the lift to safe Z. Holes are visited nearest neighbour from the
/// first in drawing order, linked at safe Z ([`crate::sequence`](fn@crate::sequence)).
///
/// Fails without a toolpath when the tool is not a flat end mill, when a
/// feed, the plunge rate, the stock thickness, the depth, or a pitch
/// (operation or tool step-down) is missing; when the pitch is not
/// positive, above the tool's step-down, or so fine that a chord would
/// fall less than one unit of the program's last place or the helix need
/// more than [`MAX_DESCENT_MOVES`] chords; when the allowance is negative;
/// when the depth passes the stock bottom plus allowance; when an id does
/// not resolve or a target cannot be placed; or when the cutter would
/// leave the stock ([`CamError::OffStock`]).
#[must_use = "the toolpath is the whole point"]
pub fn generate_bore(project: &Project, op: &BoreOp) -> Result<(Toolpath, Vec<Warning>), CamError> {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.id.clone(),
            tool: op.tool.clone(),
        })?;
    // The helix radius is the hole less half the diameter, right only for
    // a flat end mill, as a Pocket's offset (#67).
    if !tool.cutter.is_flat() {
        return Err(CamError::CutterNotFlat {
            op: op.id.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            cutter: tool.cutter.describe(),
        });
    }
    let rates = rates_of(tool)?;
    if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
        return Err(CamError::InvalidToolDiameter {
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            diameter_mm: tool.diameter_mm,
        });
    }
    if !(op.safe_z_mm.is_finite() && op.safe_z_mm > 0.0) {
        return Err(CamError::InvalidSafeZ {
            op: op.id.clone(),
            safe_z_mm: op.safe_z_mm,
        });
    }
    if !(op.allowance_mm.is_finite() && op.allowance_mm >= 0.0) {
        return Err(CamError::InvalidBoreSetting {
            op: op.id.clone(),
            field: "allowance_mm",
            value: op.allowance_mm,
            expected: "it must be 0 or more",
        });
    }
    let pitch_mm = op
        .pitch_mm
        .or(tool.step_down_mm)
        .ok_or_else(|| CamError::MissingStepDown {
            op: op.id.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
        })?;
    let bad_pitch = |expected| CamError::InvalidBoreSetting {
        op: op.id.clone(),
        field: "pitch_mm",
        value: pitch_mm,
        expected,
    };
    if !(pitch_mm.is_finite() && pitch_mm > 0.0) {
        return Err(bad_pitch("it must be more than 0"));
    }
    if let Some(step_down_mm) = tool.step_down_mm
        && pitch_mm > step_down_mm + EPS_MM
    {
        return Err(bad_pitch("it must be at most the tool's step-down"));
    }
    let thickness_mm = project
        .stock
        .thickness_mm
        .ok_or(CamError::MissingThickness)?;
    let depth_mm = op
        .depth_mm
        .ok_or_else(|| CamError::MissingDepth { op: op.id.clone() })?;
    // The hole bottom, with the depth checked against the stock bottom
    // plus the allowance (S1) as for every generator.
    let bottom_z_mm = pass_z_mm(
        &op.id,
        thickness_mm,
        depth_mm,
        depth_mm,
        op.through_cut_allowance_mm,
    )?
    .last()
    .copied()
    .unwrap_or(thickness_mm - depth_mm);
    let safe_z_mm = thickness_mm + op.safe_z_mm;
    let start_z_mm = descent_start_z_mm(thickness_mm, safe_z_mm);
    let tolerance_mm = project.flatten_tolerance_mm;
    let quantum_mm = project
        .post_processor
        .decimal_places
        .quantum_mm(project.display_units);
    let ccw = helix_is_ccw(op.direction, CutSide::Internal);

    let mut warnings = Vec::new();
    let holes = resolve_holes(project, op, tool.diameter_mm, &mut warnings)?;
    let mut blocks = Vec::with_capacity(holes.len());
    for hole in holes {
        let helix = Helix {
            centre: hole.centre,
            radius_mm: hole.helix_radius_mm,
            ccw,
            start_z_mm,
            bottom_z_mm,
            pitch_mm,
        };
        let chords = helix.chords(tolerance_mm, quantum_mm).ok_or_else(|| {
            bad_pitch("it is so fine the helix would need more moves than a descent may have")
        })?;
        let drop_mm = (start_z_mm - bottom_z_mm) / chords as f64;
        if drop_mm < quantum_mm {
            return Err(bad_pitch(
                "it is so fine a helix move would fall less than the program's last decimal place",
            ));
        }
        blocks.push(helix.block(hole.shape, chords, rates.feed_mm_min, rates.plunge_mm_min));
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
    };
    let toolpath = sequence(&input)?;
    // #62: the cutter must stay on the stock.
    if let Some(overhang) = swept_bounds(&toolpath, tool.diameter_mm / 2.0)
        .and_then(|swept| stock_overhang(&project.stock, swept))
    {
        return Err(CamError::OffStock {
            op: op.id.clone(),
            overhang,
        });
    }
    Ok((toolpath, warnings))
}

/// The holes of `op` a helix can cut, in drawing order (target order,
/// then profile order), each with its helix radius. Other profiles warn
/// and are skipped: not circles, too small to spiral in, or so wide the
/// helix would leave a core. No hole left warns once.
fn resolve_holes(
    project: &Project,
    op: &BoreOp,
    tool_diameter_mm: f64,
    warnings: &mut Vec<Warning>,
) -> Result<Vec<Hole>, CamError> {
    let tool_radius_mm = tool_diameter_mm / 2.0;
    let tolerance_mm = project.flatten_tolerance_mm;
    let mut holes = Vec::new();
    for (shape, circle) in circles_of(project, &op.id, &op.targets)? {
        let Some((centre, radius_mm)) = circle else {
            warnings.push(Warning::BoreTargetNotCircle {
                op: op.id.clone(),
                shape,
            });
            continue;
        };
        let hole_mm = 2.0 * radius_mm;
        let helix_radius_mm = radius_mm - tool_radius_mm - op.allowance_mm;
        if helix_radius_mm < BORE_MIN_HELIX_RADIUS_MM {
            warnings.push(Warning::BoreNoRoom {
                op: op.id.clone(),
                shape,
                hole_mm,
                tool_mm: tool_diameter_mm,
                allowance_mm: op.allowance_mm,
                // The outline is a circle within the tolerance, so a hole
                // drawn at the bit's size is a Drill's (Drill's own rule).
                drill_fits: hole_mm + tolerance_mm >= tool_diameter_mm,
            });
            continue;
        }
        // The bit sweeps the ring `[R − r, R + r]` about the centre; past
        // the tolerance its inner edge misses the centre (design spec,
        // "Algorithms → Bore", step 1).
        let core_radius_mm = helix_radius_mm - tool_radius_mm;
        if core_radius_mm > tolerance_mm {
            warnings.push(Warning::BoreLeavesCore {
                op: op.id.clone(),
                shape,
                hole_mm,
                tool_mm: tool_diameter_mm,
                allowance_mm: op.allowance_mm,
                core_mm: 2.0 * core_radius_mm,
            });
            continue;
        }
        holes.push(Hole {
            shape,
            centre,
            helix_radius_mm,
        });
    }
    if holes.is_empty() {
        warnings.push(Warning::BoreNoHoles { op: op.id.clone() });
    }
    Ok(holes)
}

/// One hole's helix and the circle at its bottom.
#[derive(Debug, Clone, Copy)]
struct Helix {
    centre: Point,
    radius_mm: f64,
    /// Counter-clockwise seen from +Z.
    ccw: bool,
    start_z_mm: f64,
    bottom_z_mm: f64,
    pitch_mm: f64,
}

impl Helix {
    /// The point at `angle_rad` from +X about the centre, on the helix.
    fn at(&self, angle_rad: f64) -> Point {
        self.centre + Vec2::new(angle_rad.cos(), angle_rad.sin()) * self.radius_mm
    }

    /// The number of turns from the start to the bottom.
    fn turns(&self) -> f64 {
        (self.start_z_mm - self.bottom_z_mm).max(0.0) / self.pitch_mm
    }

    /// Chords of the whole descent: at least [`HELIX_SEGMENTS_PER_REV`] a
    /// turn, more when a chord would bow in from the circle by more than
    /// `tolerance_mm`, but never so many that a chord runs less than √2
    /// units of the last place `quantum_mm` in XY (#265's rule), and at
    /// least one; `None` over [`MAX_DESCENT_MOVES`].
    fn chords(&self, tolerance_mm: f64, quantum_mm: f64) -> Option<usize> {
        let per_rev = HELIX_SEGMENTS_PER_REV as f64;
        let bowing = if tolerance_mm > 0.0 && tolerance_mm < self.radius_mm {
            (PI / (1.0 - tolerance_mm / self.radius_mm).acos()).ceil()
        } else {
            per_rev
        };
        // A chord of `n` a turn runs `2R·sin(π/n)`.
        let run = std::f64::consts::SQRT_2 * quantum_mm / (2.0 * self.radius_mm);
        let writable = if run > 0.0 && run < 1.0 {
            (PI / run.asin()).floor()
        } else {
            per_rev
        };
        let per_turn = bowing.min(writable).max(per_rev);
        let chords = (self.turns() * per_turn).ceil().max(1.0);
        // Checked against the cap first, so the cast is of a whole number
        // in 1..=MAX_DESCENT_MOVES.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        (chords <= MAX_DESCENT_MOVES as f64).then_some(chords as usize)
    }

    /// The hole's block: the helix in `chords` chords from the start, at
    /// the feed scaled to the plunge rate (S10), then the circle at the
    /// bottom and the move to the centre at `feed_mm_min`.
    fn block(
        &self,
        shape: ShapeRef,
        chords: usize,
        feed_mm_min: f64,
        plunge_mm_min: f64,
    ) -> CutBlock {
        let sense = if self.ccw { 1.0 } else { -1.0 };
        let sweep_rad = sense * self.turns() * TAU;
        let start = self.at(0.0);
        let mut current = Point3::new(start.x, start.y, self.start_z_mm);
        let mut descent = Vec::with_capacity(chords);
        let steps = chords as f64;
        for k in 1..=chords {
            let t = k as f64 / steps;
            let p = self.at(sweep_rad * t);
            let z = if k == chords {
                self.bottom_z_mm
            } else {
                self.start_z_mm - (self.start_z_mm - self.bottom_z_mm) * t
            };
            let next = Point3::new(p.x, p.y, z);
            descent.push(Move::linear(
                next,
                plunge_limited_feed_mm_min(feed_mm_min, plunge_mm_min, current, next),
            ));
            current = next;
        }
        let entry = current.xy();
        // One full circle at the bottom, four quarter arcs, back to the
        // helix end: the wall to size and the floor flat.
        let mut cuts = Vec::with_capacity(5);
        for quarter in 1..=4 {
            let p = if quarter == 4 {
                entry
            } else {
                self.at(sweep_rad + sense * FRAC_PI_2 * f64::from(quarter))
            };
            let kind = if self.ccw {
                MoveKind::ArcCcw {
                    centre: self.centre,
                }
            } else {
                MoveKind::ArcCw {
                    centre: self.centre,
                }
            };
            cuts.push(Move {
                kind,
                to: Point3::new(p.x, p.y, self.bottom_z_mm),
                feed_mm_min: Some(feed_mm_min),
            });
        }
        // To the centre, off the wall, before the lift (Fusion's "Lead To
        // Center"). No core is left, so this runs in air.
        cuts.push(Move::linear(
            Point3::new(self.centre.x, self.centre.y, self.bottom_z_mm),
            feed_mm_min,
        ));
        CutBlock {
            shape,
            pass: 0,
            z_mm: self.bottom_z_mm,
            descent_from: start,
            descent_from_z_mm: self.start_z_mm,
            descent,
            entry,
            cuts,
            exit: self.centre,
            // Nothing comes loose: no core is left.
            frees: false,
            cleared: None,
            group: 0,
        }
    }
}

#[cfg(test)]
mod tests;
