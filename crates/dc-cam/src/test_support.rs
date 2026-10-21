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
use dc_geometry::{EPS_MM, Point, approx_eq};
use dc_project::{Operation, Project};

/// Asserts that `toolpath`, generated for `op` in `project`, is safe to run.
/// Z is absolute: stock bottom 0, stock top `thickness`, safe Z
/// `thickness + op.safe_z_mm`. Tolerances are [`EPS_MM`]. Checks:
///
/// 1. **Depth (S1).** No move ends below `−op.through_cut_allowance_mm`.
/// 2. **Start and end.** A non-empty toolpath starts with a rapid that ends
///    at or above safe Z, and its last move ends at or above safe Z.
/// 3. **Rapids (S2, S15).** Every rapid carries no feed. A rapid that moves
///    in XY starts and ends at or above safe Z. A rapid in Z alone may end
///    below safe Z only in air: at or above the stock top, or at or above
///    the deepest Z a cutting move has already reached at that XY (a
///    drill's retract and peck re-entry, S15). (The first move's start is
///    wherever the machine is; the post-processor raises Z before moving
///    in XY.)
/// 4. **Feeds (S3).** Every linear and arc move carries `Some` positive
///    feed. A straight plunge (Z falls, XY stays) carries the tool's plunge
///    rate. A straight move that falls while it moves in XY (a ramp or
///    helix descent, a tab's down slope) carries the feed rate scaled so
///    its vertical component is at most the plunge rate, `min(feed,
///    plunge × length / drop)` (S10's feed scale, #157): the cutting feed
///    where that is slow enough, else exactly the plunge-limited feed.
///    Every other move that changes XY carries the tool's feed rate.
/// 5. **Dwells.** A dwell ends where the tool already is and carries no
///    feed.
///
/// Panics with the offending move index on the first violation.
pub fn assert_safe(toolpath: &Toolpath, project: &Project, op: &Operation) {
    let tool = project
        .tools
        .iter()
        .find(|t| &t.id == op.tool())
        .expect("operation's tool is in the project");
    let thickness_mm = project.stock.thickness_mm.expect("stock has a thickness");
    // A Drill never cuts in XY, so it needs no cutting feed; with none set,
    // any XY cutting move fails the feed check.
    let feed_mm_min = match op {
        Operation::Drill(_) => tool.feed_mm_min.unwrap_or(f64::NAN),
        _ => tool.feed_mm_min.expect("tool has a feed"),
    };
    assert_safe_within(
        toolpath,
        &Limits {
            safe_z_mm: thickness_mm + op.safe_z_mm(),
            stock_top_mm: thickness_mm,
            floor_z_mm: -op.through_cut_allowance_mm(),
            feed_mm_min,
            plunge_mm_min: tool.plunge_mm_min.expect("tool has a plunge rate"),
        },
    );
}

/// The limits [`assert_safe_within`] checks a toolpath against, for tests
/// that build moves or cut blocks without a project.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Absolute safe Z, mm.
    pub safe_z_mm: f64,
    /// The stock top, mm: a rapid in Z alone may end at or above it.
    pub stock_top_mm: f64,
    /// Lowest permitted move end, mm: stock bottom minus the through-cut
    /// allowance.
    pub floor_z_mm: f64,
    /// The tool's cutting feed, mm/min. `NaN` when the operation has none,
    /// which then rejects every XY cutting move.
    pub feed_mm_min: f64,
    /// The tool's plunge rate, mm/min.
    pub plunge_mm_min: f64,
}

/// [`assert_safe`] with the limits given directly instead of read from a
/// project and operation. The checks are the same.
pub fn assert_safe_within(toolpath: &Toolpath, limits: &Limits) {
    let feed = limits.feed_mm_min;
    let plunge = limits.plunge_mm_min;
    let safe_z = limits.safe_z_mm;
    let floor_z = limits.floor_z_mm;
    let at_or_above = |z: f64, limit: f64| z >= limit - EPS_MM;

    let Some(first) = toolpath.moves.first() else {
        return;
    };
    assert!(
        first.is_rapid() && at_or_above(first.to.z, safe_z),
        "move 0 must be a rapid at safe Z {safe_z}: {first:?}"
    );

    // The deepest Z a cutting move has reached at the current XY (S15).
    let mut cut_to: Option<(Point, f64)> = None;
    let mut previous: Option<&Move> = None;
    for (i, mv) in toolpath.moves.iter().enumerate() {
        assert!(
            at_or_above(mv.to.z, floor_z),
            "move {i} goes below stock bottom − allowance ({floor_z}): {mv:?}"
        );
        let moves_xy = previous.is_some_and(|prev| prev.to.xy().distance_to(mv.to.xy()) > EPS_MM);
        match mv.kind {
            MoveKind::Rapid => {
                assert!(mv.feed_mm_min.is_none(), "rapid {i} carries a feed: {mv:?}");
                if let Some(prev) = previous {
                    assert!(
                        !moves_xy || at_or_above(prev.to.z, safe_z),
                        "rapid {i} moves in XY from below safe Z {safe_z}: {prev:?} -> {mv:?}"
                    );
                }
                if moves_xy || previous.is_none() {
                    assert!(
                        at_or_above(mv.to.z, safe_z),
                        "rapid {i} ends below safe Z {safe_z}: {mv:?}"
                    );
                } else if !at_or_above(mv.to.z, safe_z)
                    && !at_or_above(mv.to.z, limits.stock_top_mm)
                {
                    let cut_z = cut_to
                        .filter(|(at, _)| at.distance_to(mv.to.xy()) <= EPS_MM)
                        .map(|(_, z)| z);
                    assert!(
                        cut_z.is_some_and(|z| at_or_above(mv.to.z, z)),
                        "rapid {i} descends into uncut material: ends at Z {} where the deepest cut is {cut_z:?}: {mv:?}",
                        mv.to.z
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
                let moves_xy = moves_xy || !matches!(mv.kind, MoveKind::Linear);
                if descends && !moves_xy {
                    assert!(
                        approx_eq(f, plunge, EPS_MM),
                        "plunge {i} feeds at {f}, not plunge {plunge}: {mv:?}"
                    );
                } else if descends && matches!(mv.kind, MoveKind::Linear) {
                    let (dx, dy) = (mv.to.x - prev.to.x, mv.to.y - prev.to.y);
                    let drop = prev.to.z - mv.to.z;
                    let length = (dx * dx + dy * dy + drop * drop).sqrt();
                    let limited = feed.min(plunge * length / drop);
                    assert!(
                        approx_eq(f, limited, EPS_MM * limited.max(1.0)),
                        "descending XY move {i} feeds at {f}, not the plunge-limited {limited} (feed {feed}, plunge {plunge}): {prev:?} -> {mv:?}"
                    );
                } else if moves_xy {
                    assert!(
                        approx_eq(f, feed, EPS_MM),
                        "XY cutting move {i} feeds at {f}, not feed {feed}: {mv:?}"
                    );
                }
                let z = match cut_to {
                    Some((at, z)) if at.distance_to(mv.to.xy()) <= EPS_MM => z.min(mv.to.z),
                    _ => mv.to.z,
                };
                cut_to = Some((mv.to.xy(), z));
            }
            MoveKind::Dwell { seconds } => {
                assert!(mv.feed_mm_min.is_none(), "dwell {i} carries a feed: {mv:?}");
                assert!(seconds > 0.0, "dwell {i} waits {seconds} s: {mv:?}");
                let prev = previous.expect("a dwell follows a move");
                assert!(
                    !moves_xy && approx_eq(prev.to.z, mv.to.z, EPS_MM),
                    "dwell {i} is not where the tool is: {prev:?} -> {mv:?}"
                );
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
    use dc_project::test_support::sample_project;

    /// The sample project (6 mm stock, safe Z 11 absolute, feed 1000,
    /// plunge 300) and its profile operation.
    fn fixture() -> (Project, Operation) {
        let project = sample_project();
        let op = project
            .operations
            .first()
            .cloned()
            .expect("sample project has an operation");
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

    /// Rapid in, plunge to `drop`, ramp down to 0 over 5 mm of X, cut,
    /// retract.
    fn ramp_cut_by(drop: f64, ramp_feed: f64) -> Vec<Move> {
        vec![
            Move::rapid(Point3::new(0.0, 0.0, 11.0)),
            Move::linear(Point3::new(0.0, 0.0, drop), 300.0),
            Move::linear(Point3::new(5.0, 0.0, 0.0), ramp_feed),
            Move::linear(Point3::new(10.0, 0.0, 0.0), 1000.0),
            Move::rapid(Point3::new(10.0, 0.0, 11.0)),
        ]
    }

    /// A 1 mm drop over 5 mm: at feed 1000 the vertical rate is 196 mm/min,
    /// under the 300 plunge, so the ramp runs at the cutting feed.
    fn ramp_cut(ramp_feed: f64) -> Vec<Move> {
        ramp_cut_by(1.0, ramp_feed)
    }

    #[test]
    fn accepts_ramp_down_at_cutting_feed() {
        check(ramp_cut(1000.0));
    }

    /// S10's feed scale: a 2 mm drop over 5 mm at feed 1000 would descend
    /// at 371 mm/min, faster than the 300 plunge; scaled to
    /// `300 × length / drop` it is a feed derived from the tool.
    #[test]
    fn accepts_steep_ramp_at_plunge_limited_feed() {
        let length = (25.0_f64 + 4.0).sqrt();
        check(ramp_cut_by(2.0, 300.0 * length / 2.0));
    }

    /// Any other feed on a descending XY move is invented.
    #[test]
    #[should_panic(expected = "not the plunge-limited")]
    fn rejects_steep_ramp_at_an_invented_feed() {
        check(ramp_cut_by(2.0, 500.0));
    }

    #[test]
    #[should_panic(expected = "not the plunge-limited")]
    fn rejects_ramp_down_at_plunge_rate() {
        check(ramp_cut(300.0));
    }

    /// S10 (#157): the steep ramp at the cutting feed would descend at 371
    /// mm/min, past the 300 plunge, whatever made it: an entry or a tab's
    /// down slope.
    #[test]
    #[should_panic(expected = "not the plunge-limited")]
    fn rejects_steep_ramp_at_cutting_feed() {
        check(ramp_cut_by(2.0, 1000.0));
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

    /// Rapid in, rapid down to 1 mm above the 6 mm stock, peck to 2, rapid
    /// up to the retract height, rapid back down to `reentry_z`, peck to
    /// 0, retract: a drill's moves (S15).
    fn peck_cycle(reentry_z: f64) -> Vec<Move> {
        vec![
            Move::rapid(Point3::new(0.0, 0.0, 11.0)),
            Move::rapid(Point3::new(0.0, 0.0, 7.0)),
            Move::linear(Point3::new(0.0, 0.0, 2.0), 300.0),
            Move::rapid(Point3::new(0.0, 0.0, 7.0)),
            Move::rapid(Point3::new(0.0, 0.0, reentry_z)),
            Move::linear(Point3::new(0.0, 0.0, 0.0), 300.0),
            Move::rapid(Point3::new(0.0, 0.0, 11.0)),
        ]
    }

    /// S15: a rapid in Z alone may end above the stock, or inside the air
    /// a cut has already opened at that XY.
    #[test]
    fn z_only_rapid_into_cut_air_is_accepted() {
        check(peck_cycle(2.5));
        check(peck_cycle(2.0));
    }

    /// S15: below the deepest cut at that XY is uncut material.
    #[test]
    #[should_panic(expected = "descends into uncut material")]
    fn z_only_rapid_below_cut_depth_is_rejected() {
        check(peck_cycle(1.5));
    }

    /// A cut at another XY does not open this hole.
    #[test]
    #[should_panic(expected = "descends into uncut material")]
    fn cut_elsewhere_does_not_open_this_hole() {
        let moves = vec![
            Move::rapid(Point3::new(10.0, 0.0, 11.0)),
            Move::rapid(Point3::new(10.0, 0.0, 7.0)),
            Move::linear(Point3::new(10.0, 0.0, 2.0), 300.0),
            Move::rapid(Point3::new(10.0, 0.0, 11.0)),
            Move::rapid(Point3::new(0.0, 0.0, 11.0)),
            Move::rapid(Point3::new(0.0, 0.0, 7.0)),
            Move::rapid(Point3::new(0.0, 0.0, 2.5)),
            Move::linear(Point3::new(0.0, 0.0, 0.0), 300.0),
            Move::rapid(Point3::new(0.0, 0.0, 11.0)),
        ];
        check(moves);
    }

    #[test]
    fn dwell_at_the_tool_position_is_accepted() {
        let mut moves = one_cut(0.0);
        moves.insert(3, Move::dwell(Point3::new(10.0, 0.0, 0.0), 0.5));
        check(moves);
    }

    #[test]
    #[should_panic(expected = "not where the tool is")]
    fn dwell_away_from_the_tool_is_rejected() {
        let mut moves = one_cut(0.0);
        moves.insert(3, Move::dwell(Point3::new(10.0, 0.0, 1.0), 0.5));
        check(moves);
    }
}
