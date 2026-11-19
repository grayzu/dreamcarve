// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Profile generator's lookups that cannot miss (code review finding
//! 1.2, `docs/code-review-plan.md`): each helper, given the input its
//! callers never build, refuses with [`CamError::Internal`]. Before, a
//! miss dropped a cutting pass, left a shape uncut, or cut from an
//! invented point or depth.

use super::waste::{self, WallPath};
use super::*;
use dc_geometry::{Point, Polyline};
use dc_project::test_support::sample_project;
use dc_project::{
    CornerRelief, CutDirection, Entry, InstanceId, MachiningOrder, OperationId, StartAt, ToolId,
};

fn square() -> Polyline {
    Polyline::rect(Point::new(0.0, 0.0), 40.0, 40.0)
}

fn shape(name: &str) -> ShapeRef {
    ShapeRef {
        instance: InstanceId::new(name),
        instance_name: name.to_owned(),
        profile: 0,
    }
}

/// One region nested from one square.
fn one_region() -> Region {
    let (regions, _) = nest(vec![Contour::new(square()).unwrap()]).unwrap();
    regions.into_iter().next().unwrap()
}

fn is_internal(err: &CamError, what: &str) -> bool {
    matches!(err, CamError::Internal { what: found, .. } if *found == what)
}

/// The first pass begins at safe Z, and every later pass at the Z of the
/// pass above it. A pass past the last has none above it: it used to be
/// taken for a first pass, its descent starting at safe Z.
#[test]
fn a_pass_past_the_last_has_no_pass_above_it() {
    let pass_z = [4.0, 2.0, 0.0];
    assert_eq!(pass_above_z_mm(&pass_z, 0), Ok(None));
    assert_eq!(pass_above_z_mm(&pass_z, 2), Ok(Some(2.0)));
    let err = pass_above_z_mm(&pass_z, 4).unwrap_err();
    assert!(
        is_internal(&err, "a pass after the first has no pass above it"),
        "{err:?}"
    );
}

/// Left and Right cut open paths only. A closed outline placed for one
/// used to be cut on its line, as On line cuts it.
#[test]
fn a_closed_outline_under_left_or_right_is_an_internal_defect() {
    assert_eq!(
        placement(Side::OnLine, Role::Hole),
        Ok((Offset::None, Wall::Inside))
    );
    for side in [Side::Left, Side::Right] {
        for role in [Role::Outer, Role::Hole] {
            let err = placement(side, role).unwrap_err();
            assert!(
                is_internal(
                    &err,
                    "a Left or Right Profile was asked to cut a closed outline"
                ),
                "{side:?} {role:?}: {err:?}"
            );
        }
    }
}

/// A tool-centre path with no vertex has no start; an entry was aimed at
/// the origin for one.
#[test]
fn a_path_with_no_vertex_has_no_start() {
    assert_eq!(path_start(&square()), Ok(Point::new(0.0, 0.0)));
    let err = path_start(&Polyline::new(Vec::new(), true)).unwrap_err();
    assert!(
        is_internal(&err, "a tool-centre path has no vertex to start at"),
        "{err:?}"
    );
}

/// A region is nested from the shapes' contours, so its outline is one of
/// them. Asked of other shapes, the outline used to be left out: not cut,
/// and in no containment pair.
#[test]
fn a_region_of_no_shape_is_an_internal_defect() {
    let region = one_region();
    let shapes = [(shape("a"), Contour::new(square()).unwrap())];
    assert_eq!(region_outer(&region, &shapes), Ok(&shapes[0].0));
    assert_eq!(region_shapes(&region, &shapes).unwrap().len(), 1);
    let what = "a region's outline is of no target shape";
    for err in [
        region_outer(&region, &[]).unwrap_err(),
        region_shapes(&region, &[]).unwrap_err(),
        containment_pairs(std::slice::from_ref(&region), &[]).unwrap_err(),
    ] {
        assert!(is_internal(&err, what), "{err:?}");
    }
}

/// A path's waste domain is found from its region. A path naming a region
/// the Profile does not have used to get no domain, so a plunge.
#[test]
fn a_wall_path_of_no_region_is_an_internal_defect() {
    let path = square();
    let paths = [WallPath {
        path: &path,
        region: 3,
        stock_to_leave_mm: 0.0,
    }];
    let stock = sample_project().stock;
    for side in [Side::Inside, Side::Outside] {
        let err = waste::domains(side, &[one_region()], &paths, &stock, 3.0, 0.01).unwrap_err();
        assert!(
            is_internal(
                &err,
                "a Profile path cuts the wall of a region the Profile does not have"
            ),
            "{side:?}: {err:?}"
        );
    }
    // On line has no waste domain to find.
    assert_eq!(
        waste::domains(Side::OnLine, &[], &paths, &stock, 3.0, 0.01),
        Ok(vec![Vec::new()])
    );
}

/// `start: first_vertex` re-seams each closed path nearest its shape's
/// first vertex as drawn. For a shape that is not in the project the
/// path used to keep its own start, and the setting was ignored.
#[test]
fn a_start_at_the_first_vertex_of_a_missing_shape_is_an_internal_defect() {
    let project = sample_project();
    let op = ProfileOp {
        id: OperationId::new("o1"),
        name: "Profile".into(),
        enabled: true,
        targets: Vec::new(),
        tool: ToolId::new("t1"),
        side: Side::Outside,
        depth_mm: Some(3.0),
        step_down_mm: None,
        direction: CutDirection::Climb,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
        tabs: None,
        machining_order: MachiningOrder::FullDepthFirst,
        entry: Entry::Plunge,
        lead: None,
        corner_relief: CornerRelief::None,
        last_pass: None,
        start: StartAt::FirstVertex,
    };
    let missing = shape("not in the project");
    let placed = || {
        vec![CentrePath {
            path: square(),
            shape: &missing,
            wall: Wall::Outside,
            region: Some(0),
            hole: false,
            twin: None,
        }]
    };
    let err = start::seat(&project, &op, placed()).err().unwrap();
    assert!(
        is_internal(
            &err,
            "a shape a Profile cuts has no first vertex in the project"
        ),
        "{err:?}"
    );
    // With no start set, nothing is looked up.
    let auto = ProfileOp {
        start: StartAt::Auto,
        ..op
    };
    let (seated, fixed) = start::seat(&project, &auto, placed()).ok().unwrap();
    assert_eq!((seated.len(), fixed), (1, vec![false]));
}
