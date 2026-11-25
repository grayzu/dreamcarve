// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Properties of the cut (`docs/specs/phase-6/design.md`, "Algorithms →
//! The cut of one move"): the two facts playback and the build rest on.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code reports failure by panicking"
)]

use dc_geometry::{Point, Point3};
use dc_sim::{CellSpec, Cutter, Grid, Heightfield, SimMove, Turn};
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};

/// The seed every run uses unless `PROPTEST_RNG_SEED` is set.
const SEED: u64 = 0x0073_696d_0000_0006;

/// `cases` cases from the fixed seed, or from `PROPTEST_RNG_SEED` when
/// set, as `dc-geometry`'s properties are configured.
fn config(cases: u32) -> ProptestConfig {
    let mut config = ProptestConfig::with_cases(cases);
    if std::env::var_os("PROPTEST_RNG_SEED").is_none() {
        config.rng_seed = RngSeed::Fixed(SEED);
    }
    config.failure_persistence = Some(Box::new(FileFailurePersistence::Direct(
        "proptest-regressions/properties.txt",
    )));
    config
}

const TOP_MM: f64 = 10.0;

fn field() -> Heightfield {
    let grid = Grid::of(24.0, 20.0, CellSpec::CellMm(0.5)).unwrap();
    Heightfield::uncut(grid, TOP_MM).unwrap()
}

/// A tip position on or a little off the board, from above the stock to
/// below its bottom.
fn tip() -> impl Strategy<Value = Point3> {
    (-3.0..27.0_f64, -3.0..23.0_f64, -1.0..11.0_f64).prop_map(|(x, y, z)| Point3::new(x, y, z))
}

/// A straight cut, or an arc through its ends that falls or rises
/// between them.
fn cutting_move() -> impl Strategy<Value = SimMove> {
    (
        tip(),
        tip(),
        prop::option::of((0.6..3.0_f64, any::<bool>())),
    )
        .prop_map(|(a, b, arc)| {
            let mv = SimMove::cut(a, b, 1.0);
            match arc {
                None => mv,
                Some((stretch, ccw)) => {
                    // A centre on the chord's perpendicular bisector, so that
                    // both ends are on the circle.
                    let mid = Point::new((a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
                    let centre = Point::new(
                        mid.x - (b.y - a.y) * (stretch - 0.5),
                        mid.y + (b.x - a.x) * (stretch - 0.5),
                    );
                    mv.along(centre, if ccw { Turn::Ccw } else { Turn::Cw })
                }
            }
        })
}

fn cutter() -> impl Strategy<Value = Cutter> {
    prop_oneof![
        (0.4..4.0_f64).prop_map(|radius_mm| Cutter::Flat { radius_mm }),
        (0.4..4.0_f64).prop_map(|radius_mm| Cutter::Ball { radius_mm }),
        (0.3..2.0_f64, 0.5..4.0_f64)
            .prop_map(|(slope, radius_mm)| Cutter::Vee { slope, radius_mm }),
        (0.2..0.8_f64, 0.05..0.5_f64, 1.0..4.0_f64).prop_map(
            |(tip_radius_mm, slope, radius_mm)| {
                Cutter::TaperedBall {
                    tip_radius_mm,
                    slope,
                    radius_mm,
                }
            }
        ),
    ]
}

proptest! {
    #![proptest_config(config(256))]

    /// A move cut in parts, in any order, leaves what the whole move
    /// leaves: a part of a line is a line and a part of an arc an arc,
    /// and the lowest over a move is the least of the lowest over its
    /// parts. The parts are worked out from other end points, so the
    /// heights agree to rounding, not to the bit. An arc that falls is
    /// the same chords however it is split.
    #[test]
    fn a_move_in_pieces_equals_the_move(
        mv in cutting_move(),
        cutter in cutter(),
        mut cuts in prop::collection::vec(0.0..1.0_f64, 0..5),
        order in any::<prop::sample::Index>(),
    ) {
        let mut whole = field();
        whole.cut(&mv, (0.0, 1.0), &cutter, 0);

        cuts.push(0.0);
        cuts.push(1.0);
        cuts.sort_by(f64::total_cmp);
        let mut spans: Vec<(f64, f64)> = cuts.windows(2).map(|w| (w[0], w[1])).collect();
        let turn = order.index(spans.len());
        spans.rotate_left(turn);
        let mut parts = field();
        for span in spans {
            parts.cut(&mv, span, &cutter, 0);
        }
        let worst = whole
            .heights()
            .iter()
            .zip(parts.heights())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        prop_assert!(
            worst <= 1e-9,
            "heights differ by {worst} mm for {mv:?} with {cutter:?}"
        );
    }

    /// Whole moves in any order leave the same heights, to the bit: a
    /// cell's height is the least of the same values whatever the order.
    #[test]
    fn moves_in_any_order_leave_the_same_field(
        (moves, shuffled) in prop::collection::vec(cutting_move(), 1..6)
            .prop_flat_map(|m| (Just(m.clone()), Just(m).prop_shuffle())),
        cutter in cutter(),
    ) {
        let (mut first, mut second) = (field(), field());
        for mv in &moves {
            first.cut(mv, (0.0, 1.0), &cutter, 0);
        }
        for mv in &shuffled {
            second.cut(mv, (0.0, 1.0), &cutter, 0);
        }
        let bits = |f: &Heightfield| f.heights().iter().map(|z| z.to_bits()).collect::<Vec<_>>();
        prop_assert_eq!(bits(&first), bits(&second));
        // No cell is above the top or below the bottom.
        prop_assert!(first.heights().iter().all(|&z| (0.0..=TOP_MM).contains(&z)));
    }
}
