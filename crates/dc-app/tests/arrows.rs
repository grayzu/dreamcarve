// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/canvas/toolpathArrows.test.ts

//! Slice 16 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan →
//! Viewport geometry", `arrows::*`; #545): where the direction arrows go,
//! ported from PureCutCNC's `toolpathArrows.test.ts` with its rules and
//! its oracle.
//!
//! Two claims, as upstream: panning must hit the cache and zooming must
//! miss it, since the pass reads screen-space thresholds; and storing
//! positions pan-independently must not move the arrows, checked against
//! an independent transcription of the original per-frame pass.
//!
//! What changed in the port:
//! - Upstream's `retractions` flag and its Z-level filter do not exist in
//!   DreamCarve (functional capability 73 has one *Rapids* toggle and no
//!   levels), so `a selected Z level filters arrows` is not ported and
//!   `rapid layers gate independently by Z direction` becomes
//!   `rapids_toggle_gates_level_and_rising_rapids`.
//! - Upstream's oracle runs on `trochoidal-249k.camj`, a PureCutCNC project
//!   whose trochoidal edge route DreamCarve does not generate, and a test
//!   here cannot read the PureCutCNC checkout in CI. The oracle runs
//!   instead on the toolpaths DreamCarve generates for the quick-start
//!   plaque (`tests/fixtures/plaque.dcv`): pockets, a profile with tabs,
//!   bores and engraving, arcs flattened, a few thousand moves, read in
//!   place.
//! - Y-up changes no expected value: every rule reads lengths and the
//!   angle between directions, the same either way up.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_app::viewport::arrows::{
    ArrowCache, ArrowKind, ArrowMove, ArrowVisibility, Placements, compute, moves_of,
};
use dc_geometry::Point3;

use common::plaque;

const ALL_ON: ArrowVisibility = ArrowVisibility::ALL;

fn cut(from: (f64, f64, f64), to: (f64, f64, f64)) -> ArrowMove {
    ArrowMove {
        kind: ArrowKind::Cut,
        from: Point3::new(from.0, from.1, from.2),
        to: Point3::new(to.0, to.1, to.2),
    }
}

fn rapid(from: (f64, f64, f64), to: (f64, f64, f64)) -> ArrowMove {
    ArrowMove {
        kind: ArrowKind::Rapid,
        ..cut(from, to)
    }
}

/// A long straight run: every move is far past the force-arrow length.
fn straight_run(count: usize) -> Vec<ArrowMove> {
    (0..count)
        .map(|i| {
            let x = i as f64 * 10.0;
            cut((x, 0.0, 0.0), (x + 10.0, 0.0, 0.0))
        })
        .collect()
}

/// Upstream `panning reuses the same placements; zooming rebuilds`: the
/// same (toolpath, scale, visibility) is a hit, a new scale a miss. A pan
/// changes none of the three, so it is the first case.
#[test]
fn panning_reuses_the_same_placements_zooming_rebuilds() {
    let moves = straight_run(40);
    let mut cache = ArrowCache::new();
    let first = cache.placements(1, &moves, 1.0, ALL_ON).clone();
    assert_eq!(cache.builds(), 1);
    let same_pan = cache.placements(1, &moves, 1.0, ALL_ON).clone();
    assert_eq!(cache.builds(), 1, "the same key, scale and flags must hit");
    assert_eq!(first, same_pan);
    let zoomed = cache.placements(1, &moves, 2.0, ALL_ON).clone();
    assert_eq!(cache.builds(), 2, "a different scale must rebuild");
    assert_ne!(zoomed, first);
}

/// Upstream `each visibility flag the pass reads rebuilds the cache`:
/// flipping cuts or rapids rebuilds, and restoring them agrees with the
/// first answer.
#[test]
fn each_visibility_flag_rebuilds_the_cache() {
    let moves = straight_run(20);
    let mut cache = ArrowCache::new();
    let base = cache.placements(7, &moves, 1.0, ALL_ON).clone();
    for flipped in [
        ArrowVisibility {
            cuts: false,
            ..ALL_ON
        },
        ArrowVisibility {
            rapids: false,
            ..ALL_ON
        },
    ] {
        let before = cache.builds();
        cache.placements(7, &moves, 1.0, flipped);
        assert_eq!(cache.builds(), before + 1, "{flipped:?} must rebuild");
    }
    let restored = cache.placements(7, &moves, 1.0, ALL_ON).clone();
    assert_eq!(restored.cut.len(), base.cut.len());
}

/// Upstream `positions are pan-independent: scaled world, no offset`:
/// every stored end point is exactly a world coordinate times the scale.
#[test]
fn positions_are_pan_independent() {
    let moves = straight_run(10);
    let scale = 3.0;
    let placements = compute(&moves, scale, ALL_ON);
    assert!(!placements.cut.is_empty(), "the run places arrows");
    let ends: Vec<f64> = moves
        .iter()
        .flat_map(|m| [m.from.x * scale, m.to.x * scale])
        .collect();
    for arrow in &placements.cut {
        assert!(
            ends.contains(&arrow[0]),
            "from x {} is not scaled world",
            arrow[0]
        );
        assert!(
            ends.contains(&arrow[2]),
            "to x {} is not scaled world",
            arrow[2]
        );
        assert!(arrow[1] == 0.0 && arrow[3] == 0.0, "Y is 0 on this run");
    }
}

/// Upstream `scaling the view scales the stored positions in lockstep`.
#[test]
fn scaling_the_view_scales_positions_in_lockstep() {
    let moves = straight_run(10);
    let at1 = compute(&moves, 1.0, ALL_ON);
    let at4 = compute(&moves, 4.0, ALL_ON);
    assert_eq!(at1.cut.len(), at4.cut.len(), "one arrow per move at both");
    for (i, (a, b)) in at1.cut.iter().zip(&at4.cut).enumerate() {
        for k in 0..4 {
            assert!(
                (b[k] - a[k] * 4.0).abs() < 1e-3,
                "arrow {i}[{k}]: {} vs {}",
                b[k],
                a[k] * 4.0
            );
        }
    }
}

/// Upstream `collinear moves are thinned by spacing, not one arrow each`:
/// 400 short collinear moves, none long enough to force an arrow and none
/// turning, so spacing alone decides.
#[test]
fn collinear_moves_are_thinned_by_spacing() {
    let moves: Vec<ArrowMove> = (0..400)
        .map(|i| {
            let x = f64::from(i);
            cut((x, 0.0, 0.0), (x + 1.0, 0.0, 0.0))
        })
        .collect();
    let arrows = compute(&moves, 1.0, ALL_ON).cut.len();
    assert!(arrows > 0, "spacing still places some arrows");
    assert!(
        arrows < 100,
        "spacing thins the run: {arrows} arrows for 400 moves"
    );
}

/// Upstream `a sharp short connector still gets an arrow the spacing rule
/// would skip`: the middle move of a zigzag turns against both neighbours
/// and gets an arrow; the straight run's does not.
#[test]
fn sharp_short_connector_still_gets_an_arrow() {
    let straight = [
        cut((0.0, 0.0, 0.0), (1.0, 0.0, 0.0)),
        cut((1.0, 0.0, 0.0), (2.0, 0.0, 0.0)),
        cut((2.0, 0.0, 0.0), (3.0, 0.0, 0.0)),
    ];
    let turning = [
        cut((0.0, 0.0, 0.0), (1.0, 0.0, 0.0)),
        cut((1.0, 0.0, 0.0), (1.0, 1.0, 0.0)),
        cut((1.0, 1.0, 0.0), (2.0, 1.0, 0.0)),
    ];
    let straight_arrows = compute(&straight, 1.0, ALL_ON).cut.len();
    let turning_arrows = compute(&turning, 1.0, ALL_ON).cut.len();
    assert!(
        turning_arrows > straight_arrows,
        "the turn adds an arrow: {turning_arrows} vs {straight_arrows}"
    );
}

/// Upstream `rapid layers gate independently by Z direction`, adapted:
/// DreamCarve has one *Rapids* toggle, so a level rapid and a rising one
/// each place an arrow with it on and neither does with it off, while the
/// cuts' arrows are untouched.
#[test]
fn rapids_toggle_gates_level_and_rising_rapids() {
    let moves = [
        rapid((0.0, 0.0, 5.0), (100.0, 0.0, 5.0)),
        rapid((100.0, 0.0, 5.0), (200.0, 0.0, 20.0)),
        cut((200.0, 0.0, 0.0), (300.0, 0.0, 0.0)),
    ];
    let both = compute(&moves, 1.0, ALL_ON);
    assert_eq!(both.rapid.len(), 2, "each rapid places an arrow");
    assert_eq!(both.cut.len(), 1);
    let no_rapids = compute(
        &moves,
        1.0,
        ArrowVisibility {
            rapids: false,
            ..ALL_ON
        },
    );
    assert!(no_rapids.rapid.is_empty(), "rapids off hides both");
    assert_eq!(no_rapids.cut.len(), 1, "and leaves the cut's");
}

/// Upstream `a toolpath with no bounds places nothing rather than
/// throwing`.
#[test]
fn no_moves_places_nothing() {
    let placements = compute(&[], 1.0, ALL_ON);
    assert_eq!(placements, Placements::default());
}

/// The arrows sit on the toolpath's moves as the Viewport draws them: a
/// generated arc is flattened to chords that start where the last move
/// ended, so a full circle's arrows lie on the circle.
#[test]
fn arcs_are_read_as_their_chords() {
    let project = plaque();
    let generated = dc_job::generate_operation(&project, &dc_project::OperationId::new("op3"));
    let toolpath = &generated.runs[0].toolpath;
    let moves = moves_of(toolpath, 0.01);
    assert!(
        moves.len() > toolpath.moves.len(),
        "the bore's arcs became chords"
    );
    for pair in moves.windows(2) {
        let gap = (pair[0].to.x - pair[1].from.x).hypot(pair[0].to.y - pair[1].from.y);
        assert!(gap < 1e-9, "a move starts where the last ended: {pair:?}");
    }
}

/// The original per-frame pass, transcribed from upstream's test
/// (`previewPrimitives.ts` at 9cd0325): canvas coordinates with the
/// offset added, `hypot`, both neighbours computed eagerly. Written
/// independently of `compute`, so the two cannot share a bug.
#[allow(
    clippy::manual_clamp,
    reason = "transcribed as upstream writes it, `max(12, min(40, …))`"
)]
fn original_arrow_pass(
    moves: &[ArrowMove],
    scale: f64,
    offset: (f64, f64),
    visibility: ArrowVisibility,
) -> Placements {
    let mut min = moves[0].from;
    let mut max = moves[0].from;
    for p in moves.iter().flat_map(|m| [m.from, m.to]) {
        min = Point3::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
        max = Point3::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
    }
    let span = (max.x - min.x).max(max.y - min.y).max(max.z - min.z);
    let spacing = (span * scale * 0.09).min(40.0).max(12.0);
    let arrow_length = (span * scale * 0.03).min(18.0).max(8.5);
    let to_canvas = |p: Point3| (offset.0 + p.x * scale, offset.1 + p.y * scale);
    let delta = |m: &ArrowMove| {
        let (from, to) = (to_canvas(m.from), to_canvas(m.to));
        let (dx, dy) = (to.0 - from.0, to.1 - from.1);
        (from, to, dx, dy, dx.hypot(dy))
    };
    let direction = |m: Option<&ArrowMove>| {
        let (_, _, dx, dy, length) = delta(m?);
        (length > 0.001).then(|| (dx / length, dy / length))
    };
    let mut since = [0.0, 0.0];
    let mut out = Placements::default();
    for (i, m) in moves.iter().enumerate() {
        let slot = match m.kind {
            ArrowKind::Cut if !visibility.cuts => continue,
            ArrowKind::Rapid if !visibility.rapids => continue,
            ArrowKind::Cut => 0,
            ArrowKind::Rapid => 1,
        };
        let (from, to, dx, dy, length) = delta(m);
        if length < 0.5 {
            continue;
        }
        since[slot] += length;
        let previous = if i == 0 {
            None
        } else {
            direction(moves.get(i - 1))
        };
        let next = direction(moves.get(i + 1));
        let dir = (dx / length, dy / length);
        let turn = previous.zip(next).map(|(p, n)| {
            let a = (dir.0 * p.0 + dir.1 * p.1).clamp(-1.0, 1.0).acos();
            let b = (dir.0 * n.0 + dir.1 * n.1).clamp(-1.0, 1.0).acos();
            a.min(b)
        });
        let connector = m.kind == ArrowKind::Cut
            && length <= spacing * 0.8
            && turn.is_some_and(|t| t >= std::f64::consts::PI / 10.0);
        let force = length >= arrow_length * 1.1;
        let by_spacing = since[slot] >= spacing;
        if !force && !by_spacing && !connector {
            continue;
        }
        let arrow = [from.0, from.1, to.0, to.1];
        match m.kind {
            ArrowKind::Cut => out.cut.push(arrow),
            ArrowKind::Rapid => out.rapid.push(arrow),
        }
        since[slot] = 0.0;
    }
    out
}

/// Upstream `places exactly the pre-#664 arrows on the real 249,663-move
/// fixture`, on DreamCarve's own toolpaths (see the module comment): every
/// run of the plaque, at upstream's scales and offsets, places the arrows
/// the original pass places, at the same canvas positions once the offset
/// is added back. Guarded against passing vacuously: the matrix sees
/// arrows of both kinds.
#[test]
fn places_exactly_the_original_arrows_on_generated_toolpaths() {
    let project = plaque();
    let mut seen = [0usize, 0usize];
    for op in &project.operations {
        let generated = dc_job::generate_operation(&project, op.id());
        assert!(!generated.findings.has_errors(), "{} generates", op.id());
        for run in &generated.runs {
            let moves = moves_of(&run.toolpath, 0.01);
            for scale in [1.0, 7.3, 40.0, 155.5] {
                for offset in [(0.0, 0.0), (137.5, -42.25)] {
                    let want = original_arrow_pass(&moves, scale, offset, ALL_ON);
                    let got = compute(&moves, scale, ALL_ON);
                    for (slot, kind) in [(0, ArrowKind::Cut), (1, ArrowKind::Rapid)] {
                        let (want, got) = (want.of(kind), got.of(kind));
                        assert_eq!(
                            want.len(),
                            got.len(),
                            "{} at scale {scale}: {kind:?} arrow count",
                            op.id()
                        );
                        seen[slot] += got.len();
                        let tolerance = 1e-9 * scale.max(1.0);
                        for (w, g) in want.iter().zip(got) {
                            let moved = [
                                g[0] + offset.0,
                                g[1] + offset.1,
                                g[2] + offset.0,
                                g[3] + offset.1,
                            ];
                            for k in 0..4 {
                                assert!(
                                    (w[k] - moved[k]).abs() <= tolerance,
                                    "{} at scale {scale}: {kind:?} arrow drifted: {w:?} vs {moved:?}",
                                    op.id()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        seen[0] > 100 && seen[1] > 20,
        "the matrix places arrows: {seen:?}"
    );
}
