// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Emitted-motion safety assertions for tests in this crate and, with the
//! `test-support` feature, in crates that depend on it
//! (`docs/rust-guidelines.md`, "Tests"; `docs/specs/phase-1/functional.md`,
//! safety rules S1–S3).

#![allow(
    clippy::panic,
    clippy::expect_used,
    reason = "a test assertion helper reports failure by panicking"
)]

use crate::toolpath::{Move, MoveKind, Toolpath};
use dc_geometry::{EPS_MM, approx_eq};
use dc_project::{ProfileOp, Project};

/// Asserts that `toolpath`, generated for `op` in `project`, is safe to run.
/// Z is absolute: stock bottom 0, stock top `thickness`, safe Z
/// `thickness + op.safe_z_mm`. Tolerances are [`EPS_MM`]. Checks:
///
/// 1. **Depth (S1).** No move ends below `−op.through_cut_allowance_mm`.
/// 2. **Start and end.** A non-empty toolpath starts with a rapid that ends
///    at or above safe Z, and its last move ends at or above safe Z.
/// 3. **Rapids (S2).** Every rapid ends at or above safe Z and carries no
///    feed. A rapid that moves in XY must also start at or above safe Z, so
///    the only rapid that may begin lower is a straight vertical retract.
///    (The first move's start is wherever the machine is; the
///    post-processor raises Z before moving in XY.)
/// 4. **Feeds (S3).** Every linear and arc move carries `Some` positive
///    feed. A move that lowers Z carries the tool's plunge rate; a move that
///    changes XY without lowering Z carries the tool's feed rate.
///
/// Panics with the offending move index on the first violation.
pub fn assert_safe(toolpath: &Toolpath, project: &Project, op: &ProfileOp) {
    let tool = project
        .tools
        .iter()
        .find(|t| t.id == op.tool)
        .expect("operation's tool is in the project");
    let feed = tool.feed_mm_min.expect("tool has a feed");
    let plunge = tool.plunge_mm_min.expect("tool has a plunge rate");
    let safe_z = project.stock.thickness_mm + op.safe_z_mm;
    let floor_z = -op.through_cut_allowance_mm;
    let at_or_above = |z: f64, limit: f64| z >= limit - EPS_MM;

    let Some(first) = toolpath.moves.first() else {
        return;
    };
    assert!(
        first.is_rapid() && at_or_above(first.to.z, safe_z),
        "move 0 must be a rapid at safe Z {safe_z}: {first:?}"
    );

    let mut previous: Option<&Move> = None;
    for (i, mv) in toolpath.moves.iter().enumerate() {
        assert!(
            at_or_above(mv.to.z, floor_z),
            "move {i} goes below stock bottom − allowance ({floor_z}): {mv:?}"
        );
        match mv.kind {
            MoveKind::Rapid => {
                assert!(mv.feed_mm_min.is_none(), "rapid {i} carries a feed: {mv:?}");
                assert!(
                    at_or_above(mv.to.z, safe_z),
                    "rapid {i} ends below safe Z {safe_z}: {mv:?}"
                );
                if let Some(prev) = previous {
                    let moves_xy = prev.to.xy().distance_to(mv.to.xy()) > EPS_MM;
                    assert!(
                        !moves_xy || at_or_above(prev.to.z, safe_z),
                        "rapid {i} moves in XY from below safe Z {safe_z}: {prev:?} -> {mv:?}"
                    );
                }
            }
            MoveKind::Linear | MoveKind::ArcCw { .. } | MoveKind::ArcCcw { .. } => {
                let Some(f) = mv.feed_mm_min else {
                    panic!("cutting move {i} has no feed: {mv:?}");
                };
                assert!(f > 0.0, "cutting move {i} has non-positive feed: {mv:?}");
                let prev = previous.expect("a cutting move follows the first rapid");
                let descends = mv.to.z < prev.to.z - EPS_MM;
                let moves_xy = prev.to.xy().distance_to(mv.to.xy()) > EPS_MM
                    || !matches!(mv.kind, MoveKind::Linear);
                if descends {
                    assert!(
                        approx_eq(f, plunge, EPS_MM),
                        "descending move {i} feeds at {f}, not plunge {plunge}: {mv:?}"
                    );
                } else if moves_xy {
                    assert!(
                        approx_eq(f, feed, EPS_MM),
                        "XY cutting move {i} feeds at {f}, not feed {feed}: {mv:?}"
                    );
                }
            }
        }
        previous = Some(mv);
    }

    if let Some(last) = toolpath.moves.last() {
        assert!(
            at_or_above(last.to.z, safe_z),
            "toolpath ends below safe Z {safe_z}: {last:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::Point3;
    use dc_project::Operation;
    use dc_project::test_support::sample_project;

    /// The sample project (6 mm stock, safe Z 11 absolute, feed 1000,
    /// plunge 300) and its profile operation.
    fn fixture() -> (Project, ProfileOp) {
        let project = sample_project();
        let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
            panic!("sample project has a profile operation");
        };
        (project, op)
    }

    /// Rapid in, plunge to `z`, one cut, retract: safe when `z` ≥ 0.
    fn one_cut(z: f64) -> Vec<Move> {
        vec![
            Move::rapid(Point3::new(0.0, 0.0, 11.0)),
            Move::linear(Point3::new(0.0, 0.0, z), 300.0),
            Move::linear(Point3::new(10.0, 0.0, z), 1000.0),
            Move::rapid(Point3::new(10.0, 0.0, 11.0)),
        ]
    }

    fn check(moves: Vec<Move>) {
        let (project, op) = fixture();
        assert_safe(&Toolpath { moves }, &project, &op);
    }

    #[test]
    fn accepts_a_safe_cut() {
        check(one_cut(0.0));
    }

    #[test]
    #[should_panic(expected = "below stock bottom")]
    fn rejects_cut_below_bottom() {
        check(one_cut(-0.1));
    }

    #[test]
    #[should_panic(expected = "must be a rapid at safe Z")]
    fn rejects_first_move_below_safe_z() {
        let mut moves = one_cut(0.0);
        moves[0].to.z = 10.0;
        check(moves);
    }

    #[test]
    #[should_panic(expected = "moves in XY from below safe Z")]
    fn rejects_rapid_across_from_depth() {
        let mut moves = one_cut(0.0);
        moves[3].to.x = 20.0;
        check(moves);
    }

    #[test]
    #[should_panic(expected = "has no feed")]
    fn rejects_cut_without_feed() {
        let mut moves = one_cut(0.0);
        moves[2].feed_mm_min = None;
        check(moves);
    }

    #[test]
    #[should_panic(expected = "not plunge")]
    fn rejects_descent_at_cutting_feed() {
        let mut moves = one_cut(0.0);
        moves[1].feed_mm_min = Some(1000.0);
        check(moves);
    }

    #[test]
    #[should_panic(expected = "carries a feed")]
    fn rejects_rapid_with_feed() {
        let mut moves = one_cut(0.0);
        moves[3].feed_mm_min = Some(1000.0);
        check(moves);
    }

    #[test]
    #[should_panic(expected = "ends below safe Z")]
    fn rejects_ending_in_the_stock() {
        let mut moves = one_cut(0.0);
        moves.pop();
        check(moves);
    }
}
