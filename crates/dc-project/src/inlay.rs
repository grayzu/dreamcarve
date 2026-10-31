// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A male Inlay's band (#448; `docs/specs/phase-3/design.md`, "Algorithms
//! → Inlay", steps 4 and 6).

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
    /// Open, its rounding is `max(R, r)`; with a V wall it is `R`. A
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
        dc_geometry::assert_approx_eq!(band.cut_out_offset_mm(), 3.75, 1e-12);
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
        assert_eq!(
            band.edge,
            BandEdge::VWall {
                margin_needed_mm: Some(5.25)
            }
        );
        dc_geometry::assert_approx_eq!(band.edge_rounding_mm, 0.0, 1e-12);
        dc_geometry::assert_approx_eq!(band.cut_out_offset_mm(), 2.5, 1e-12);
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
