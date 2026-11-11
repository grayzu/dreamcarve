// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S19 (format 27, #646; `docs/specs/phase-3/design.md`, "Safety pass",
//! the S17 and S19 bullets; functional S19): a Photo V-Carve's program
//! keeps its V-bit's tip inside the picture, between the start plane and
//! the maximum depth below it, and crosses between grooves fed at its
//! link height.
//!
//! Everything is re-derived from the project, never from the generator or
//! its toolpath:
//!
//! - the **footprint**: the bitmap's rectangle, `x_mm` to `x_mm +
//!   width_mm`, `y_mm` to `y_mm + height_mm`;
//! - the **start plane**, `start_depth_mm` below the stock top, and the
//!   **floor**, `max_depth_mm` below it;
//! - the **engagement**: from the start plane when it is the stock top or
//!   an enabled earlier floor at the start depth holds the footprint
//!   ([`cleared_start_plane`], as validation and the generator decide
//!   it), else from the stock top (security review H4); the cone is the
//!   bit's `diameter / (2 tan(α/2))`;
//! - the **link height**: `stepover_retract_mm` above the start plane
//!   when it is so cleared, else no lower than the stock top plus
//!   [`ENTRY_CLEARANCE_MM`] (security review L1). A start plane below the
//!   stock top that nothing clears and the user does not vouch for is
//!   unresolved: no cut passes.
//!
//! In the operation's span, every line that moves:
//!
//! - a rapid that moves in XY starts and ends at or above safe Z, else
//!   [`ExportError::GrooveLinkRapid`] (S2 refuses it too);
//! - every cut is a `G1` (arc fitting never applies to grooves; an arc is
//!   [`ExportError::GrooveOffPicture`]);
//! - a cut that ends at or below the start plane ends inside the
//!   footprint and no deeper than the floor ([`ExportError::GrooveOffPicture`]),
//!   with the flanks engaged no deeper than the cone
//!   ([`ExportError::ConeHeightExceeded`]); if it moves across, it starts
//!   at or below the start plane too, inside the footprint (the
//!   footprint is convex, so the whole cut is in it);
//! - a cut that moves across above the start plane is a link: both its
//!   ends at the link height and inside the footprint
//!   ([`ExportError::GrooveLinkRapid`] at another height);
//! - a cut straight up or down above the start plane is in the air.
//!
//! A groove's width may run past the footprint by up to half the
//! full-depth width: the picture's edge is not a wall. S7 keeps it on the
//! stock, widening each cut by its groove's width at the stock top.
//!
//! **Tolerance**: S9's slack ([`super::slack_mm`]): one unit in the last
//! written place, the flatten tolerance, the arc-fit tolerance when it is
//! on, and [`dc_geometry::EPS_MM`].

use dc_cam::entry::ENTRY_CLEARANCE_MM;
use dc_geometry::{Point, Point3, Rect};
use dc_project::{Operation, PhotoVCarveOp, PostProcessor, Project, cleared_start_plane};

use super::{slack_mm, v_bit_of};
use crate::ExportError;
use crate::parse::{Block, MachineMove, MotionKind};
use crate::safety::{OpSpan, Tolerances, differs};
use crate::transform::MachineTransform;

/// A Photo V-Carve's material (module docs), project space, mm.
#[derive(Debug, Clone)]
pub(crate) struct Picture {
    /// The picture's footprint.
    footprint: Rect,
    /// The start plane.
    start_plane_z_mm: f64,
    /// The lowest a groove may go: the start plane less the maximum depth.
    floor_z_mm: f64,
    /// Where the flanks are engaged from.
    engaged_from_z_mm: f64,
    /// The bit's cone height.
    pub(crate) cone_mm: f64,
    /// Where the bit crosses between grooves.
    link_z_mm: f64,
    /// Safe Z.
    safe_z_mm: f64,
}

impl Picture {
    /// The material of `photo` in `project`, when its bitmap, its tool (a
    /// V-bit of a usable angle), its maximum depth, start depth, step-over
    /// retract and safe Z are usable and its start plane is the stock top,
    /// cleared or vouched for; else `None`, which the caller treats as
    /// unresolved.
    pub(crate) fn of(project: &Project, photo: &PhotoVCarveOp) -> Option<Self> {
        let thickness_mm = project.stock.thickness_mm.filter(|t| t.is_finite())?;
        let (_, cone_mm) = v_bit_of(project, &photo.tool)?;
        let bitmap = project.bitmaps.iter().find(|b| b.id == photo.image)?;
        let footprint = Rect {
            min: Point::new(bitmap.x_mm, bitmap.y_mm),
            max: Point::new(
                bitmap.x_mm + bitmap.width_mm,
                bitmap.y_mm + bitmap.height_mm,
            ),
        };
        let max_depth_mm = photo.max_depth_mm?;
        let start_depth_mm = photo.start_depth_mm;
        let positive = |v: f64| v.is_finite() && v > 0.0;
        let usable = [footprint.min, footprint.max]
            .iter()
            .all(|p| p.x.is_finite() && p.y.is_finite())
            && positive(max_depth_mm)
            && positive(photo.stepover_retract_mm)
            && positive(photo.safe_z_mm)
            && start_depth_mm.is_finite()
            && (0.0..=thickness_mm).contains(&start_depth_mm);
        if !usable {
            return None;
        }
        let start_plane_z_mm = thickness_mm - start_depth_mm;
        let cleared_at_start = start_depth_mm <= 0.0 || {
            let cleared = cleared_start_plane(project, &Operation::PhotoVCarve(photo.clone()));
            if cleared.is_none() && !photo.start_plane_cleared_externally {
                return None;
            }
            cleared.is_some_and(|plane| plane.is_at(start_depth_mm))
        };
        let over_plane_mm = start_plane_z_mm + photo.stepover_retract_mm;
        Some(Self {
            footprint,
            start_plane_z_mm,
            floor_z_mm: start_plane_z_mm - max_depth_mm,
            engaged_from_z_mm: if cleared_at_start {
                start_plane_z_mm
            } else {
                thickness_mm
            },
            cone_mm,
            link_z_mm: if cleared_at_start {
                over_plane_mm
            } else {
                over_plane_mm.max(thickness_mm + ENTRY_CLEARANCE_MM)
            },
            safe_z_mm: thickness_mm + photo.safe_z_mm,
        })
    }

    /// S19 on the lines of `span` (module docs).
    ///
    /// # Errors
    ///
    /// At the first line that breaks it: [`ExportError::GrooveOffPicture`],
    /// [`ExportError::GrooveLinkRapid`] or
    /// [`ExportError::ConeHeightExceeded`]; [`ExportError::OffMaterial`]
    /// for a cut whose position is not known.
    pub(crate) fn check(
        &self,
        blocks: &[Block],
        post: &PostProcessor,
        transform: &MachineTransform,
        tolerances: Tolerances,
        span: &OpSpan<'_>,
    ) -> Result<(), ExportError> {
        let op = || span.op.id().clone();
        let moves = blocks
            .iter()
            .filter(|b| span.lines.contains(&b.line))
            .filter_map(|b| b.motion.as_ref());
        for mv in moves {
            let tol = slack_mm(post, mv.units, tolerances);
            let across = differs(mv.from.x, mv.to.x) || differs(mv.from.y, mv.to.y);
            let z_of = |z: Option<f64>| z.map(|z| transform.project_z(z, mv.units));
            let (from_z, to_z) = (z_of(mv.from.z), z_of(mv.to.z));
            if mv.kind == MotionKind::Rapid {
                let below = |z: Option<f64>| z.is_none_or(|z| z < self.safe_z_mm - tol);
                if across && (below(from_z) || below(to_z)) {
                    return Err(ExportError::GrooveLinkRapid {
                        op: op(),
                        line: mv.line,
                    });
                }
                continue;
            }
            let (Some(from), Some(to)) = (point(mv, true, transform), point(mv, false, transform))
            else {
                return Err(ExportError::OffMaterial {
                    op: op(),
                    line: mv.line,
                });
            };
            let off = |at: Point3| ExportError::GrooveOffPicture {
                op: op(),
                line: mv.line,
                x_mm: at.x,
                y_mm: at.y,
            };
            if mv.kind != MotionKind::Linear {
                return Err(off(to));
            }
            let in_groove = |p: Point3| p.z <= self.start_plane_z_mm + tol;
            if in_groove(to) {
                if across && !in_groove(from) {
                    return Err(ExportError::GrooveLinkRapid {
                        op: op(),
                        line: mv.line,
                    });
                }
                for p in [from, to] {
                    if in_groove(p) && !(self.inside(p, tol) && p.z >= self.floor_z_mm - tol) {
                        return Err(off(p));
                    }
                }
                let engaged_mm = self.engaged_from_z_mm - to.z.min(from.z);
                if engaged_mm > self.cone_mm + tol {
                    return Err(ExportError::ConeHeightExceeded {
                        op: op(),
                        line: mv.line,
                        engaged_mm,
                        cone_mm: self.cone_mm,
                    });
                }
            } else if across {
                let at_link = |p: Point3| (p.z - self.link_z_mm).abs() <= tol;
                if !(at_link(from) && at_link(to)) {
                    return Err(ExportError::GrooveLinkRapid {
                        op: op(),
                        line: mv.line,
                    });
                }
                for p in [from, to] {
                    if !self.inside(p, tol) {
                        return Err(off(p));
                    }
                }
            }
            // Straight up or down above the start plane: in the air.
        }
        Ok(())
    }

    /// Whether `p` lies inside the footprint, within `tol_mm`.
    fn inside(&self, p: Point3, tol_mm: f64) -> bool {
        let Rect { min, max } = self.footprint;
        p.x >= min.x - tol_mm
            && p.x <= max.x + tol_mm
            && p.y >= min.y - tol_mm
            && p.y <= max.y + tol_mm
    }
}

/// Where `mv` starts (`start`) or ends, project space; `None` when an
/// axis is not known.
fn point(mv: &MachineMove, start: bool, transform: &MachineTransform) -> Option<Point3> {
    let at = if start { mv.from } else { mv.to };
    Some(transform.to_project(Point3::new(at.x?, at.y?, at.z?), mv.units))
}
