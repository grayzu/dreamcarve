// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S18 (`docs/specs/phase-3/design.md`, "Safety pass"; functional S18): a
//! countersink Drill's program is a plunge at each hole's centre to the
//! depth its mouth and its V-bit's angle derive, written as plain moves.
//!
//! Everything is re-derived from the project, never from the generator
//! or its hole records:
//!
//! - the **holes**: the target circles a Drill finds (closed profiles,
//!   placed, that are circles within the flatten tolerance), less those
//!   no narrower than the mouth, which the generator skips
//!   (`countersink_mouth_not_wider_than_hole`, within
//!   [`COUNTERSINK_TOLERANCE_MM`]);
//! - the **bottom**: the start plane, `start_depth_mm` below the stock top,
//!   less [`countersink_depth_mm`] of the mouth and the tool's angle;
//! - the **retract plane**: `min(safe Z, stock top + max(retract_mm, 0))`,
//!   from the stock top whatever the start plane (security review H3);
//! - the **engagement**: from the start plane when it is the stock top or
//!   an enabled earlier Pocket, or V-Carve's flat floor (#426), at the
//!   start depth holds the bit at every hole ([`cleared_start_plane`], as
//!   validation and the generator decide it), else from the stock top
//!   (security review H4); the cone is
//!   [`countersink_depth_mm`] of the tool's own diameter.
//!
//! In the operation's span:
//!
//! - no line is a canned cycle: no `G73`, `G81`, `G82` or `G83` word and
//!   no move a cycle makes (a modal cycle line has no word);
//! - every cut (`G1`, `G2`, `G3`) is a `G1` whose start and end are both
//!   at one hole's centre, that starts no higher than the retract plane
//!   and ends at the bottom;
//! - every rapid ends at or above the retract plane, and one that starts
//!   below it moves in Z alone (the retract from the bottom);
//! - anything else is a dwell or no motion.
//!
//! Each failure is [`ExportError::CountersinkDepth`]; then every cut's
//! engagement is held to the cone ([`ExportError::ConeHeightExceeded`]),
//! which a mouth wider than the bit, or a start plane nothing cleared,
//! fails.
//!
//! **Tolerance.** Rounding only: one unit in the last written place of the
//! move's units plus [`EPS_MM`]. The holes and the depth are computed here
//! from the same project values, with the same functions, as the generator
//! computes them, and a vertical plunge is never flattened or fitted, so
//! the flatten and arc-fit slack S9 allows elsewhere does not apply.

use dc_geometry::{EPS_MM, Point, Point3, as_circle};
use dc_project::{
    COUNTERSINK_TOLERANCE_MM, DrillCycle, DrillOp, Operation, PostProcessor, Project, ToolKind,
    cleared_start_plane, countersink_depth_mm,
};

use super::placed;
use crate::ExportError;
use crate::parse::{Block, MachineMove, MotionKind};
use crate::safety::{OpSpan, differs, quantum_mm};
use crate::transform::MachineTransform;

/// A countersink Drill's material (module docs), project space, mm.
#[derive(Debug, Clone)]
pub(crate) struct Sink {
    /// The centres of the holes it countersinks.
    pub centres: Vec<Point>,
    /// Where every plunge ends: the start plane less the derived depth.
    pub bottom_z_mm: f64,
    /// The highest a plunge may start, and the lowest a rapid may end.
    pub retract_z_mm: f64,
    /// Where the flank's engagement counts from: the start plane when it
    /// is cleared, else the stock top.
    pub engaged_from_z_mm: f64,
    /// The bit's cone height: its diameter's countersink depth.
    pub cone_mm: f64,
}

impl Sink {
    /// The material of `drill` in `project`, when its cycle is a
    /// countersink, its tool a V-bit of a usable angle, its mouth, start
    /// depth, retract and safe Z usable and its targets placeable; else
    /// `None`, which the caller treats as unresolved.
    pub(crate) fn of(project: &Project, drill: &DrillOp) -> Option<Self> {
        let DrillCycle::Countersink {
            diameter_mm: mouth_mm,
            start_depth_mm,
            ..
        } = drill.cycle
        else {
            return None;
        };
        let thickness_mm = project.stock.thickness_mm.filter(|t| t.is_finite())?;
        let tool = project.tools.iter().find(|t| t.id == drill.tool)?;
        let ToolKind::VBit { included_angle_deg } = tool.cutter else {
            return None;
        };
        let depth_mm = countersink_depth_mm(mouth_mm, included_angle_deg)?;
        let cone_mm = countersink_depth_mm(tool.diameter_mm, included_angle_deg)?;
        let usable = start_depth_mm.is_finite()
            && (0.0..=thickness_mm).contains(&start_depth_mm)
            && drill.retract_mm.is_finite()
            && drill.safe_z_mm.is_finite()
            && drill.safe_z_mm > 0.0;
        if !usable {
            return None;
        }
        let start_plane_z_mm = thickness_mm - start_depth_mm;
        let safe_z_mm = thickness_mm + drill.safe_z_mm;
        let retract_z_mm = (thickness_mm + drill.retract_mm.max(0.0)).min(safe_z_mm);
        let centres = placed(project, &drill.targets)?
            .iter()
            .filter(|p| p.is_closed())
            .filter_map(|p| as_circle(p, project.flatten_tolerance_mm))
            .filter(|(_, radius_mm)| mouth_mm - 2.0 * radius_mm > COUNTERSINK_TOLERANCE_MM)
            .map(|(centre, _)| centre)
            .collect();
        // H4: the stock top, unless the start plane is it or an enabled
        // earlier Pocket or V-Carve floor at the start depth cleared it.
        let cleared = start_depth_mm <= 0.0
            || cleared_start_plane(project, &Operation::Drill(drill.clone()))
                .is_some_and(|plane| plane.is_at(start_depth_mm));
        Some(Self {
            centres,
            bottom_z_mm: start_plane_z_mm - depth_mm,
            retract_z_mm,
            engaged_from_z_mm: if cleared {
                start_plane_z_mm
            } else {
                thickness_mm
            },
            cone_mm,
        })
    }

    /// S18 on the lines of `span` (module docs).
    ///
    /// # Errors
    ///
    /// At the first line that breaks it: [`ExportError::CountersinkDepth`],
    /// or [`ExportError::ConeHeightExceeded`] for a plunge whose flank
    /// engagement passes the cone.
    pub(crate) fn check(
        &self,
        blocks: &[Block],
        post: &PostProcessor,
        transform: &MachineTransform,
        span: &OpSpan<'_>,
    ) -> Result<(), ExportError> {
        let in_span: Vec<&Block> = blocks
            .iter()
            .filter(|b| span.lines.contains(&b.line))
            .collect();
        let fault = |line: usize, found_mm: f64| ExportError::CountersinkDepth {
            op: span.op.id().clone(),
            line,
            expected_mm: self.bottom_z_mm,
            found_mm,
        };
        // No canned cycle, first: its moves may well be the right ones.
        if let Some(cycle) = in_span.iter().find(|b| is_cycle(b)) {
            let deepest_mm = in_span
                .iter()
                .filter(|b| b.line == cycle.line)
                .filter_map(|b| b.motion.as_ref())
                .filter_map(|mv| mv.to.z.map(|z| transform.project_z(z, mv.units)))
                .reduce(f64::min)
                .unwrap_or(f64::NAN);
            return Err(fault(cycle.line, deepest_mm));
        }
        for mv in in_span.iter().filter_map(|b| b.motion.as_ref()) {
            let tolerance_mm = quantum_mm(post, mv.units) + EPS_MM;
            let z_of = |z: Option<f64>| z.map(|z| transform.project_z(z, mv.units));
            let (from_z, Some(to_z)) = (z_of(mv.from.z), z_of(mv.to.z)) else {
                return Err(fault(mv.line, f64::NAN));
            };
            if mv.kind == MotionKind::Rapid {
                // Z is unknown only before the first Z word, where S2
                // holds a rapid that moves in XY.
                let moves_xy = differs(mv.from.x, mv.to.x) || differs(mv.from.y, mv.to.y);
                let below = |z: f64| z < self.retract_z_mm - tolerance_mm;
                if below(to_z) || (moves_xy && from_z.is_none_or(below)) {
                    return Err(fault(mv.line, to_z));
                }
                continue;
            }
            let Some(from_z) = from_z else {
                return Err(fault(mv.line, to_z));
            };
            let plunge = mv.kind == MotionKind::Linear
                && self.at_a_centre(mv, transform, tolerance_mm)
                && from_z <= self.retract_z_mm + tolerance_mm
                && (to_z - self.bottom_z_mm).abs() <= tolerance_mm;
            if !plunge {
                return Err(fault(mv.line, to_z));
            }
            let engaged_mm = self.engaged_from_z_mm - to_z;
            if engaged_mm > self.cone_mm + tolerance_mm {
                return Err(ExportError::ConeHeightExceeded {
                    op: span.op.id().clone(),
                    line: mv.line,
                    engaged_mm,
                    cone_mm: self.cone_mm,
                });
            }
        }
        Ok(())
    }

    /// Whether `mv` starts and ends at one hole's centre, in XY, within
    /// `tolerance_mm`.
    fn at_a_centre(
        &self,
        mv: &MachineMove,
        transform: &MachineTransform,
        tolerance_mm: f64,
    ) -> bool {
        let point = |x: Option<f64>, y: Option<f64>| {
            let (x, y) = (x?, y?);
            Some(transform.to_project(Point3::new(x, y, 0.0), mv.units).xy())
        };
        let (Some(from), Some(to)) = (point(mv.from.x, mv.from.y), point(mv.to.x, mv.to.y)) else {
            return false;
        };
        self.centres
            .iter()
            .any(|c| from.distance_to(*c) <= tolerance_mm && to.distance_to(*c) <= tolerance_mm)
    }
}

/// Whether `block` is a drilling canned cycle: a `G73`, `G81`, `G82` or
/// `G83` word, or a move or dwell the controller makes for one.
fn is_cycle(block: &Block) -> bool {
    block.synthetic || [73.0, 81.0, 82.0, 83.0].iter().any(|&g| block.has('G', g))
}
