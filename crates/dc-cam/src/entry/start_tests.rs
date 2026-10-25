// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Where a ramp or helix starts (#214; `docs/specs/phase-2/design.md`,
//! "Algorithms → Entry", descent start Z; functional spec S2). A descent
//! that would start at safe Z starts one entry clearance above the stock
//! top instead, and a Pocket's later pass one entry clearance above the
//! floor of the pass before (PureCutCNC `pocket.ts`, `levelEntryStartZ`).
//! The tool gets there by a rapid straight down from safe Z, so it no
//! longer ramps or spirals through the air at the feed rate.
//!
//! The fixture is the sample project: a 50 × 30 mm rectangle at (10, 10)
//! on 6 mm stock, safe Z 5 mm above it (Z 11 above the stock bottom), the
//! 6.35 mm tool cutting 2 mm a pass.

use crate::pocket::generate_pocket;
use crate::profile::generate_profile;
use crate::test_support::assert_safe;
use crate::toolpath::{Move, Toolpath};
use dc_geometry::{EPS_MM, Point3};
use dc_project::test_support::{profile_mut, sample_project};
use dc_project::{
    ClearanceStrategy, CornerRelief, CutDirection, Entry, MachiningOrder, Operation, OperationId,
    PocketOp, Project, Side, ToolId, WallPass,
};

const THICKNESS_MM: f64 = 6.0;
const SAFE_Z_MM: f64 = THICKNESS_MM + 5.0;
/// The entry clearance: how far above the stock top, or above the floor
/// the pass before cut, a ramp or helix starts.
const CLEARANCE_MM: f64 = 1.0;

/// An offset Pocket of the sample's rectangle, 4 mm deep in two passes
/// (Z 4 and 2), with `entry`.
fn pocket(project: &Project, entry: Entry) -> Operation {
    Operation::Pocket(PocketOp {
        id: OperationId::new("o2"),
        name: "Pocket".into(),
        enabled: true,
        targets: project.instances.iter().map(|i| i.id.clone()).collect(),
        tool: ToolId::new("t1"),
        strategy: ClearanceStrategy::Offset,
        wall_pass: WallPass::Last,
        depth_mm: Some(4.0),
        step_down_mm: None,
        step_over_fraction: None,
        direction: CutDirection::Climb,
        stock_to_leave_mm: 0.0,
        rest_of: None,
        entry,
        corner_relief: CornerRelief::None,
        machining_order: MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
    })
}

/// The sample's outside Profile, 6 mm deep in three passes, without tabs,
/// with `entry`.
fn profile(side: Side, entry: Entry) -> Operation {
    let mut op = profile_mut(&mut sample_project()).clone();
    op.side = side;
    op.tabs = None;
    op.entry = entry;
    Operation::Profile(op)
}

/// Generates `op` in `project` and checks it with [`assert_safe`].
fn generate(project: &Project, op: &Operation) -> Toolpath {
    let (toolpath, _) = match op {
        Operation::Pocket(pocket) => generate_pocket(project, pocket).unwrap(),
        Operation::Profile(profile) => generate_profile(project, profile).unwrap(),
        _ => unreachable!("only Pocket and Profile have an entry"),
    };
    assert_safe(&toolpath, project, op);
    toolpath
}

/// Every rapid that ends below safe Z, with the moves either side of it.
fn approaches(toolpath: &Toolpath) -> Vec<(Move, Move, Move)> {
    toolpath
        .moves
        .windows(3)
        .filter(|w| w[1].is_rapid() && w[1].to.z < SAFE_Z_MM - EPS_MM)
        .map(|w| (w[0], w[1], w[2]))
        .collect()
}

/// Asserts `approach` is a rapid straight down from safe Z to `z_mm`,
/// followed by a descending move that also moves in XY: the ramp or helix
/// starts there.
fn assert_descent_starts_at(approach: (Move, Move, Move), z_mm: f64, what: &str) {
    let (before, rapid, first) = approach;
    let at = rapid.to.xy();
    assert!(
        before.is_rapid()
            && before.to.xy().distance_to(at) <= EPS_MM
            && (before.to.z - SAFE_Z_MM).abs() <= EPS_MM,
        "{what}: the rapid down to the descent does not start at safe Z above it: {before:?} -> {rapid:?}"
    );
    assert!(
        (rapid.to.z - z_mm).abs() <= EPS_MM,
        "{what}: the descent starts at Z {}, not {z_mm}: {rapid:?}",
        rapid.to.z
    );
    assert!(
        !first.is_rapid() && first.to.z < z_mm - EPS_MM && first.to.xy().distance_to(at) > EPS_MM,
        "{what}: no ramp or helix follows the rapid: {rapid:?} -> {first:?}"
    );
}

/// Asserts no move that descends while it moves in XY starts above
/// `z_mm`: nothing ramps or spirals through the air above it.
fn assert_no_descent_above(toolpath: &Toolpath, z_mm: f64, what: &str) {
    for w in toolpath.moves.windows(2) {
        let (from, mv) = (w[0].to, w[1]);
        let sloped = !mv.is_rapid()
            && mv.to.z < from.z - EPS_MM
            && from.xy().distance_to(mv.to.xy()) > EPS_MM;
        assert!(
            !sloped || from.z <= z_mm + EPS_MM,
            "{what}: {mv:?} descends from Z {} in the air above {z_mm}",
            from.z
        );
    }
}

/// #214: a Pocket's helix or ramp and a Profile's ramp or helix on the
/// first pass start 1 mm above the stock top (Z 7), reached by a rapid
/// straight down from safe Z (Z 11), not at safe Z: no descending move
/// starts higher.
#[test]
fn first_pass_descent_starts_just_above_the_stock() {
    let project = sample_project();
    let helix = Entry::Helix {
        diameter_fraction: 0.8,
        angle_deg: 15.0,
    };
    let ramp = Entry::Ramp { angle_deg: 15.0 };
    let start_mm = THICKNESS_MM + CLEARANCE_MM;
    for (what, op) in [
        ("pocket helix", pocket(&project, helix.clone())),
        ("pocket ramp", pocket(&project, ramp.clone())),
        ("outside profile ramp", profile(Side::Outside, ramp.clone())),
        ("inside profile helix", profile(Side::Inside, helix)),
        ("on line profile ramp", profile(Side::OnLine, ramp)),
    ] {
        let toolpath = generate(&project, &op);
        let approaches = approaches(&toolpath);
        let first = *approaches
            .first()
            .unwrap_or_else(|| panic!("{what}: no rapid down to a descent"));
        assert_descent_starts_at(first, start_mm, what);
        assert_no_descent_above(&toolpath, start_mm, what);
    }
}

/// #214: a Pocket's later pass starts its helix 1 mm above the floor the
/// pass before cut (Z 4 + 1), reached by a rapid straight down from safe
/// Z: the helix's start lies on the flat revolution the pass before cut
/// at Z 4, so the rapid stops in air the operation has cleared. The first
/// pass starts 1 mm above the stock top, as before.
#[test]
fn later_pass_descent_starts_just_above_the_previous_floor() {
    let project = sample_project();
    let op = pocket(
        &project,
        Entry::Helix {
            diameter_fraction: 0.8,
            angle_deg: 15.0,
        },
    );
    let toolpath = generate(&project, &op);
    let approaches = approaches(&toolpath);
    assert_eq!(approaches.len(), 2, "one descent per pass: {approaches:?}");
    assert_descent_starts_at(approaches[0], THICKNESS_MM + CLEARANCE_MM, "pass 0");
    assert_descent_starts_at(approaches[1], 4.0 + CLEARANCE_MM, "pass 1");
    let pass_1 = toolpath
        .moves
        .iter()
        .position(|m| *m == approaches[1].1)
        .unwrap();
    assert_no_descent_above(
        &Toolpath {
            moves: toolpath.moves[pass_1..].to_vec(),
        },
        4.0 + CLEARANCE_MM,
        "pass 1",
    );
    // The helix's start lies on the circle the pass before cut at its
    // floor.
    let start = approaches[1].1.to;
    let floor = Point3::new(start.x, start.y, 4.0);
    assert!(
        toolpath.moves[..pass_1].iter().any(|m| !m.is_rapid()
            && m.to.z <= 4.0 + EPS_MM
            && m.to.xy().distance_to(floor.xy()) <= 0.5),
        "pass 0 cut its floor near {floor:?}"
    );
}
