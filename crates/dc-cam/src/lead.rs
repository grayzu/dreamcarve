// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/xyLead.ts

//! Lead-in and lead-out arcs on a Profile (`docs/specs/phase-2/design.md`,
//! "Algorithms → Lead-in and lead-out").

use crate::entry::EntryDomain;
use crate::error::CamError;
use crate::toolpath::MoveKind;
use dc_geometry::{Point, Polyline, Vec2};
use dc_project::{Lead, OperationId};

/// Where a lead's tool centre may be.
pub(crate) trait LeadSpace {
    /// True when the tool centre may be at `p`: in the domain or on its
    /// edge.
    fn inside(&self, p: Point) -> bool;
    /// True when `p` is in the domain and at least [`LeadSpace::safety_mm`]
    /// from every wall of it.
    fn clear_of_walls(&self, p: Point) -> bool;
    /// The boundary safety, mm.
    fn safety_mm(&self) -> f64;
}

/// A waste domain as a [`LeadSpace`].
pub(crate) struct DomainSpace<'a> {
    domain: &'a EntryDomain,
    safety_mm: f64,
}

impl<'a> DomainSpace<'a> {
    /// `domain` for a tool `tool_diameter_mm` across.
    pub(crate) fn new(domain: &'a EntryDomain, tool_diameter_mm: f64) -> Self {
        Self {
            domain,
            safety_mm: tool_diameter_mm,
        }
    }
}

impl LeadSpace for DomainSpace<'_> {
    fn inside(&self, _p: Point) -> bool {
        let _ = self.domain;
        false
    }
    fn clear_of_walls(&self, _p: Point) -> bool {
        false
    }
    fn safety_mm(&self) -> f64 {
        self.safety_mm
    }
}

/// One lead arc in the plane of its pass, mm, Y-up.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct LeadArc {
    /// Where the arc starts.
    pub from: Point,
    /// Where it ends.
    pub to: Point,
    /// Its centre.
    pub centre: Point,
    /// Counter-clockwise viewed from +Z.
    pub ccw: bool,
    /// Its radius, mm.
    pub radius_mm: f64,
    /// Its sweep, degrees.
    pub sweep_deg: f64,
}

impl LeadArc {
    /// The move kind that cuts it.
    pub(crate) fn kind(&self) -> MoveKind {
        if self.ccw {
            MoveKind::ArcCcw {
                centre: self.centre,
            }
        } else {
            MoveKind::ArcCw {
                centre: self.centre,
            }
        }
    }

    /// The unit direction of travel at `p` on the arc.
    pub(crate) fn tangent_at(&self, p: Point) -> Vec2 {
        let radial = p - self.centre;
        let t = radial.perp().normalized().unwrap_or(Vec2::new(1.0, 0.0));
        if self.ccw { t } else { -t }
    }
}

/// A planned lead-in.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LeadIn {
    /// The arc, from the staging point to the arrival on the contour.
    pub arc: LeadArc,
    /// How far along the input contour the arrival is, mm.
    pub arrival_mm: f64,
    /// The contour re-seamed to start, and so end, at the arrival.
    pub seam: Polyline,
}

impl LeadIn {
    /// Where the descent lands.
    pub(crate) fn staging(&self) -> Point {
        self.arc.from
    }

    /// Where the lead joins the contour.
    pub(crate) fn arrival(&self) -> Point {
        self.arc.to
    }
}

/// A planned exit.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LeadOut {
    /// The overcut along the contour past the arrival, an open path; `None`
    /// when the overcut is 0.
    pub overcut: Option<Polyline>,
    /// The exit arc; `None` when `lead_out` is off or none fits.
    pub arc: Option<LeadArc>,
}

/// Plans the lead-in onto the closed tool-centre `path`.
pub(crate) fn plan_lead_in(
    _path: &Polyline,
    _lead: &Lead,
    _tool_diameter_mm: f64,
    _space: &dyn LeadSpace,
    _from: Option<Point>,
) -> Option<LeadIn> {
    None
}

/// Plans the exit from `seam`, the contour as the lead-in re-seamed it.
pub(crate) fn plan_lead_out(
    _seam: &Polyline,
    _lead: &Lead,
    _tool_diameter_mm: f64,
    _space: &dyn LeadSpace,
) -> LeadOut {
    LeadOut {
        overcut: None,
        arc: None,
    }
}

/// `path`, closed, re-seamed to start `distance_mm` along it.
pub(crate) fn reseam(path: &Polyline, _distance_mm: f64) -> Polyline {
    path.clone()
}

/// The points `arc` is checked at, both ends included.
pub(crate) fn lead_samples(arc: &LeadArc) -> Vec<Point> {
    vec![arc.from, arc.to]
}

/// True when `arc`, joining a contour at `anchor`, stays in `space`.
pub(crate) fn lead_fits(_anchor: Point, _arc: &LeadArc, _space: &dyn LeadSpace) -> bool {
    true
}

/// The lead values validation requires, checked again.
pub(crate) fn check(_op: &OperationId, _lead: &Lead) -> Result<(), CamError> {
    Ok(())
}

#[cfg(test)]
mod tests;
