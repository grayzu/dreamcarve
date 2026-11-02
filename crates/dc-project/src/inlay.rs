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

use crate::ids::ToolId;
use crate::model::{InlayOp, InlaySide, Project, ToolKind};
use dc_geometry::EPS_MM;

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

/// A male Inlay's band, as the generator, validation and the safety pass
/// derive it (#448).
#[derive(Debug, Clone, PartialEq)]
pub struct MaleBand {
    /// `margin_mm`, `M`.
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
}

/// `tan(α/2)` of Inlay `op`'s V-bit, when it is one with an angle in (0°,
/// 180°).
fn slope_of(project: &Project, op: &InlayOp) -> Option<f64> {
    let tool = project.tools.iter().find(|t| t.id == op.tool)?;
    let ToolKind::VBit { included_angle_deg } = tool.cutter else {
        return None;
    };
    let slope = (included_angle_deg.to_radians() / 2.0).tan();
    (included_angle_deg > 0.0 && included_angle_deg < 180.0 && slope.is_finite() && slope > 0.0)
        .then_some(slope)
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

/// The edge pass of male Inlay `op` (#448): its first clearing pass whose
/// flat end mill fits the band's floor ring, `M − P·tan(α/2) ≥ 2r` (within
/// [`EPS_MM`]). `None` on a female, without such a pass, or when the
/// margin, the plug depth or the V-bit is not usable.
#[must_use]
pub fn edge_pass(project: &Project, op: &InlayOp) -> Option<EdgePass> {
    if op.side != InlaySide::Male {
        return None;
    }
    let margin_mm = positive(op.margin_mm)?;
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

/// Male Inlay `op`'s band (#448; module docs): its edge, open or a V
/// wall, and its roundings. `None` on a female, or when a value the band
/// needs is unusable: the margin, depth, glue gap or plug depth, an
/// `outline_rounding_mm` that is set and not above 0, or the V-bit.
#[must_use]
pub fn male_band(project: &Project, op: &InlayOp) -> Option<MaleBand> {
    if op.side != InlaySide::Male {
        return None;
    }
    let slope = slope_of(project, op)?;
    let margin_mm = positive(op.margin_mm)?;
    let plug_depth_mm = positive(op.plug_depth_mm)?;
    let depth_mm = positive(op.depth_mm)?;
    let glue_gap_mm = op
        .glue_gap_mm
        .filter(|g| g.is_finite() && *g >= 0.0 && *g < depth_mm)?;
    let outline_rounding_mm = match op.outline_rounding_mm {
        None => 0.0,
        some => positive(some)?,
    };
    let foot_mm = plug_depth_mm * slope;
    // The V-bit's flank at the plug's foot reaches `(D − G + P)·s` from its
    // tip at the stock top, the tip `P·s` from the design.
    let flank_mm = (depth_mm - glue_gap_mm + 2.0 * plug_depth_mm) * slope;
    let flank_fits = flank_mm <= margin_mm + EPS_MM;
    let edge = match edge_pass(project, op) {
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
        margin_mm,
        foot_mm,
        outline_rounding_mm,
        edge_rounding_mm,
        edge,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::ToolId;
    use crate::model::{ClearanceStrategy, ClearingPass, CutDirection, Entry, InlaySide, Tool};
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

    /// The sample male ([`sample_inlay`] on its side: the 90° V-bit `t2`,
    /// slope 1, `D 2.5, G 0.25, P 1.5`, margin 6, so the floor ring is
    /// `M − P·s` = 4.5 mm wide) with clearing passes by `tools`, in a
    /// project holding the 6.35 mm end mill `t1`, `t2`, a 3.175 mm end
    /// mill `t3` and a 1 mm one `t4`.
    fn male(tools: &[&str]) -> (Project, InlayOp) {
        let mut p = sample_project();
        p.tools.push(tool_v_bit_90());
        p.tools.push(end_mill("t3", 3.175));
        p.tools.push(end_mill("t4", 1.0));
        let op = InlayOp {
            side: InlaySide::Male,
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
    /// or on a female, there is no edge pass.
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
        let female = InlayOp {
            side: InlaySide::Female,
            ..op
        };
        assert_eq!(edge_pass(&p, &female), None);
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
        let female = InlayOp {
            side: InlaySide::Female,
            ..op
        };
        assert_eq!(male_band(&p, &female), None);
    }
}
