// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Tests of a male Inlay's band: its edge pass, its rounding, and where
//! the cut-out runs.

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

/// The edge pass is the first clearing pass whose end mill fits
/// the floor ring, `M − P·tan(α/2) ≥ 2r`: of the 6.35 mm and 3.175 mm
/// passes in a 4.5 mm ring, the second, though a 1 mm pass follows;
/// a 4.5 mm end mill just fits. With only the 6.35 mm pass, or none,
/// there is no edge pass; the male board does not change it.
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

/// The band's outer edge is open when an edge pass fits and the
/// V-bit's flank at the plug's foot, `(D − G + P)·s` wide at the stock
/// top, stays inside the band: `M ≥ (D − G + 2P)·s`, 5.25 mm here.
/// Open, its rounding is `max(R, r)` and a cut-out's centre runs its
/// radius inside the edge; with a V wall the rounding is `R` and
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
                shape: None,
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
            construction: false,
            folder: None,
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
    let narrowest = drawn.narrowest.expect("a design in the band");
    dc_geometry::assert_approx_eq!(narrowest.width_mm, 10.0, 1e-9);
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

/// An open band is clipped to the stock itself, a V wall the
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
