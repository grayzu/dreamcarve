// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/multiFeature.ts
//   src/engine/toolpaths/geometry.ts (greedyNearestNeighbor)
//   src/engine/toolpaths/pocket.ts (transitionToCutEntry)

//! Sequencing (`docs/specs/phase-2/design.md`, "Algorithms → Sequence";
//! `docs/research/toolpath-ordering.md` §5.5, decisions D5 and D10): the
//! one step every generator hands its [`CutBlock`]s to. It orders the
//! blocks by machining order, containment precedence and nearest neighbour,
//! writes the links between them, and produces the [`Toolpath`].
//!
//! Nothing here depends on Y-up or Y-down: distances and containment are
//! symmetric under the flip. What was re-derived rather than copied is the
//! start of the walk (the first shape in drawing order, D5, where upstream
//! starts a drilling walk at the origin) and the containment rule, which
//! upstream does not have.

use crate::block::CutBlock;
use crate::error::{CamError, ShapeRef};
use crate::toolpath::Toolpath;
use dc_project::MachiningOrder;

/// Everything [`sequence`] needs, in project space (mm, Y-up, Z-up).
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceInput<'a> {
    /// The blocks to order, in generator order. The order in which shapes
    /// first appear here is their drawing order.
    pub blocks: Vec<CutBlock>,
    /// Full depth first or pass first.
    pub order: MachiningOrder,
    /// `(inner, encloser)` pairs: `inner` is cut before `encloser` reaches
    /// its freeing pass, and under full depth first before `encloser`
    /// starts at all. Pairs naming shapes with no blocks are ignored.
    pub containment: &'a [(ShapeRef, ShapeRef)],
    /// Absolute safe Z, mm: every rapid that moves in XY runs here.
    pub safe_z_mm: f64,
    /// The tool's diameter, mm.
    pub tool_diameter_mm: f64,
    /// The operation's step-over, mm; 0 for a profile. A stay-down link
    /// may be at most `tool_diameter_mm + stepover_mm` long.
    pub stepover_mm: f64,
    /// The tool's cutting feed, mm/min, for stay-down links.
    pub feed_mm_min: f64,
    /// The tool's plunge rate, mm/min, for the straight plunges of blocks
    /// with an empty descent.
    pub plunge_mm_min: f64,
}

/// The order the blocks of `input` are cut in, as indices into
/// `input.blocks` (spec steps 1–3): every unit (one shape at one pass) is
/// a run of consecutive indices in generator order; units are walked by
/// `input.order` with containment precedence and nearest neighbour, ties by
/// drawing order. [`sequence`] emits exactly this order.
///
/// Fails with [`CamError::InvalidBlock`] when a block's cuts go below its
/// pass Z, a shape's passes are not consecutive from 0, or a plunging block
/// starts away from its entry, and with [`CamError::ContainmentCycle`]
/// when the containment pairs leave no shape cuttable.
pub fn plan(input: &SequenceInput<'_>) -> Result<Vec<usize>, CamError> {
    let _ = input;
    Ok(Vec::new())
}

/// Orders and links the blocks of `input` into one toolpath (spec steps
/// 4–5): the order is [`plan`]'s; between consecutive blocks `a → b` the
/// link is nothing when `b` is the next pass of `a`'s shape and its descent
/// starts where `a` ended; a stay-down `Linear` at feed when both blocks
/// have `cleared` areas at one Z and the straight link lies inside their
/// union and is at most `tool_diameter_mm + stepover_mm` long; otherwise a
/// rapid up to safe Z, a rapid in XY, and the descent. A block's empty
/// descent is a straight plunge at the plunge rate; its own descent moves
/// are written as given, after a plunge to `descent_from_z_mm` when that is
/// below the height the tool arrives at. Zero-length moves are never
/// written. The first block is reached by a rapid at safe Z; the last is
/// followed by a rapid up to safe Z.
///
/// Fails as [`plan`] does.
#[must_use = "the toolpath is the whole point"]
pub fn sequence(input: &SequenceInput<'_>) -> Result<Toolpath, CamError> {
    let _ = plan(input)?;
    Ok(Toolpath::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::BlockProblem;
    use crate::test_support::{Limits, assert_safe_within};
    use crate::toolpath::Move;
    use dc_geometry::{Contour, EPS_MM, Point, Point3, Polyline, Region, nest};
    use dc_project::InstanceId;

    const THICKNESS_MM: f64 = 6.0;
    const SAFE_Z_MM: f64 = THICKNESS_MM + 5.0;
    const FEED_MM_MIN: f64 = 1000.0;
    const PLUNGE_MM_MIN: f64 = 300.0;
    const DIAMETER_MM: f64 = 6.35;
    /// Pass Zs of a 6 mm cut in 2 mm steps on 6 mm stock, project space.
    const PASS_Z_MM: [f64; 3] = [4.0, 2.0, 0.0];

    fn shape(name: &str) -> ShapeRef {
        ShapeRef {
            instance: InstanceId::new(name),
            instance_name: name.into(),
            profile: 0,
        }
    }

    /// A square `size` mm wide with its lower-left corner at (`x`, `y`),
    /// cut counter-clockwise from that corner at `pass` (Z `PASS_Z_MM[pass]`),
    /// plunging. `frees` on the last of `passes`.
    fn square(name: &str, x: f64, y: f64, size: f64, pass: usize, passes: usize) -> CutBlock {
        let z = PASS_Z_MM[pass];
        let start = Point::new(x, y);
        let corner = |px: f64, py: f64| Move::linear(Point3::new(px, py, z), FEED_MM_MIN);
        CutBlock {
            shape: shape(name),
            pass,
            z_mm: z,
            descent_from: start,
            descent_from_z_mm: if pass == 0 {
                SAFE_Z_MM
            } else {
                PASS_Z_MM[pass - 1]
            },
            descent: Vec::new(),
            entry: start,
            cuts: vec![
                corner(x + size, y),
                corner(x + size, y + size),
                corner(x, y + size),
                corner(x, y),
            ],
            exit: start,
            frees: pass + 1 == passes && PASS_Z_MM[pass] <= EPS_MM,
            cleared: None,
            group: 0,
        }
    }

    /// Every pass of [`square`], top first.
    fn squares(name: &str, x: f64, y: f64, size: f64, passes: usize) -> Vec<CutBlock> {
        (0..passes)
            .map(|pass| square(name, x, y, size, pass, passes))
            .collect()
    }

    fn pairs(pairs: &[(&str, &str)]) -> Vec<(ShapeRef, ShapeRef)> {
        pairs
            .iter()
            .map(|(inner, outer)| (shape(inner), shape(outer)))
            .collect()
    }

    fn input<'a>(
        blocks: Vec<CutBlock>,
        order: MachiningOrder,
        containment: &'a [(ShapeRef, ShapeRef)],
    ) -> SequenceInput<'a> {
        SequenceInput {
            blocks,
            order,
            containment,
            safe_z_mm: SAFE_Z_MM,
            tool_diameter_mm: DIAMETER_MM,
            stepover_mm: 0.0,
            feed_mm_min: FEED_MM_MIN,
            plunge_mm_min: PLUNGE_MM_MIN,
        }
    }

    fn limits() -> Limits {
        Limits {
            safe_z_mm: SAFE_Z_MM,
            floor_z_mm: 0.0,
            feed_mm_min: FEED_MM_MIN,
            plunge_mm_min: PLUNGE_MM_MIN,
        }
    }

    /// Sequences, and checks the result with `assert_safe_within`.
    fn run(input: &SequenceInput<'_>) -> Toolpath {
        let toolpath = sequence(input).unwrap();
        assert_safe_within(&toolpath, &limits());
        toolpath
    }

    /// The blocks in the order their first cut appears in `toolpath`, as
    /// `(shape name, pass)`.
    fn visits(toolpath: &Toolpath, blocks: &[CutBlock]) -> Vec<(String, usize)> {
        let mut found: Vec<(usize, &CutBlock)> = blocks
            .iter()
            .map(|b| {
                let first = b.cuts[0];
                let at = toolpath
                    .moves
                    .iter()
                    .position(|m| *m == first)
                    .unwrap_or_else(|| panic!("block {:?} pass {} not emitted", b.shape, b.pass));
                (at, b)
            })
            .collect();
        found.sort_by_key(|(at, _)| *at);
        found
            .into_iter()
            .map(|(_, b)| (b.shape.instance_name.clone(), b.pass))
            .collect()
    }

    fn names(visits: &[(String, usize)]) -> Vec<&str> {
        visits.iter().map(|(n, _)| n.as_str()).collect()
    }

    /// Port of `cutZTransitions` (PureCutCNC `toolpaths.test.ts`): the Z of
    /// each non-rapid move, with repeats collapsed.
    fn cut_z_transitions(toolpath: &Toolpath) -> Vec<f64> {
        let mut out: Vec<f64> = Vec::new();
        for mv in toolpath.moves.iter().filter(|m| !m.is_rapid()) {
            if out.last().is_none_or(|z| (z - mv.to.z).abs() > EPS_MM) {
                out.push(mv.to.z);
            }
        }
        out
    }

    fn assert_z(actual: &[f64], expected: &[f64]) {
        assert_eq!(actual.len(), expected.len(), "{actual:?} vs {expected:?}");
        for (a, e) in actual.iter().zip(expected) {
            assert!((a - e).abs() <= EPS_MM, "{actual:?} vs {expected:?}");
        }
    }

    const BOTH: [MachiningOrder; 2] = [MachiningOrder::FullDepthFirst, MachiningOrder::PassFirst];

    /// O1: with a (hole, outer) pair every pass of the hole precedes the
    /// outer's freeing pass, whichever was drawn first, under both orders.
    #[test]
    fn hole_is_cut_before_its_outline() {
        let outer = squares("outer", 0.0, 0.0, 100.0, 2);
        let hole = squares("hole", 40.0, 40.0, 20.0, 2);
        let pairs = pairs(&[("hole", "outer")]);
        for order in BOTH {
            for blocks in [
                [outer.clone(), hole.clone()].concat(),
                [hole.clone(), outer.clone()].concat(),
            ] {
                let tp = run(&input(blocks.clone(), order, &pairs));
                let expected: Vec<(String, usize)> = match order {
                    MachiningOrder::FullDepthFirst => {
                        [("hole", 0), ("hole", 1), ("outer", 0), ("outer", 1)]
                    }
                    MachiningOrder::PassFirst => {
                        [("hole", 0), ("outer", 0), ("hole", 1), ("outer", 1)]
                    }
                }
                .map(|(n, p)| (n.to_string(), p))
                .to_vec();
                assert_eq!(visits(&tp, &blocks), expected, "{order:?}");
            }
        }
    }

    /// O1 (`docs/research/toolpath-ordering.md`): a frame (outline A, hole
    /// B) with a coaster C inside the hole. Whatever the drawing order, the
    /// coaster is cut first, then the hole that frees the waste around it,
    /// then the outline.
    #[test]
    fn island_is_cut_before_the_hole_around_it() {
        let a = squares("A", 0.0, 0.0, 200.0, 2);
        let b = squares("B", 20.0, 20.0, 160.0, 2);
        let c = squares("C", 70.0, 45.0, 60.0, 2);
        let pairs = pairs(&[("B", "A"), ("C", "B")]);
        for order in BOTH {
            for blocks in [
                [a.clone(), b.clone(), c.clone()].concat(),
                [c.clone(), b.clone(), a.clone()].concat(),
                [b.clone(), c.clone(), a.clone()].concat(),
            ] {
                let tp = run(&input(blocks.clone(), order, &pairs));
                let got = visits(&tp, &blocks);
                let expected = match order {
                    MachiningOrder::FullDepthFirst => ["C", "C", "B", "B", "A", "A"],
                    MachiningOrder::PassFirst => ["C", "B", "A", "C", "B", "A"],
                };
                assert_eq!(names(&got), expected, "{order:?}: {got:?}");
            }
        }
    }

    /// Four nested contours drawn outermost first: the innermost is cut
    /// first, and each hole still comes before its own outline.
    #[test]
    fn four_levels_cut_innermost_first() {
        let blocks = [
            squares("outer", 0.0, 0.0, 200.0, 2),
            squares("hole", 20.0, 20.0, 160.0, 2),
            squares("island", 40.0, 40.0, 120.0, 2),
            squares("island hole", 60.0, 60.0, 80.0, 2),
        ]
        .concat();
        let pairs = pairs(&[
            ("hole", "outer"),
            ("island", "hole"),
            ("island hole", "island"),
        ]);
        let tp = run(&input(
            blocks.clone(),
            MachiningOrder::FullDepthFirst,
            &pairs,
        ));
        assert_eq!(
            names(&visits(&tp, &blocks)),
            [
                "island hole",
                "island hole",
                "island",
                "island",
                "hole",
                "hole",
                "outer",
                "outer"
            ]
        );
    }

    /// O2: sequencing is a permutation. Three parts with holes, one of them
    /// with a tab-raised cut, give the same multiset of cutting moves at
    /// every Z under both orders.
    #[test]
    fn orders_cut_the_same_moves() {
        let mut blocks = Vec::new();
        for (i, x) in [0.0, 60.0, 120.0].into_iter().enumerate() {
            blocks.extend(squares(&format!("part {i}"), x, 0.0, 50.0, 3));
            blocks.extend(squares(&format!("hole {i}"), x + 20.0, 20.0, 10.0, 3));
        }
        // A tab on the first part's final pass: the third edge rises.
        let tabbed = &mut blocks[2];
        assert_eq!(tabbed.pass, 2);
        tabbed.cuts[2].to.z = 3.0;
        tabbed
            .cuts
            .insert(2, Move::linear(Point3::new(50.0, 50.0, 3.0), PLUNGE_MM_MIN));
        tabbed
            .cuts
            .insert(4, Move::linear(Point3::new(0.0, 50.0, 0.0), PLUNGE_MM_MIN));
        let pairs = pairs(&[
            ("hole 0", "part 0"),
            ("hole 1", "part 1"),
            ("hole 2", "part 2"),
        ]);
        let cutting = |tp: &Toolpath| {
            let mut out: Vec<String> = Vec::new();
            let mut pos = tp.moves[0].to;
            for mv in &tp.moves {
                if !mv.is_rapid() && pos.xy().distance_to(mv.to.xy()) > EPS_MM {
                    out.push(format!("{:?}", mv));
                }
                pos = mv.to;
            }
            out.sort();
            out
        };
        let full = run(&input(
            blocks.clone(),
            MachiningOrder::FullDepthFirst,
            &pairs,
        ));
        let pass = run(&input(blocks.clone(), MachiningOrder::PassFirst, &pairs));
        assert_ne!(full.moves, pass.moves, "the orders differ");
        assert_eq!(cutting(&full), cutting(&pass));
        assert_eq!(
            cutting(&full).len(),
            6 * 3 * 4,
            "the tab lifts are vertical"
        );
    }

    /// O3: between two shapes the tool goes up to safe Z, across, and down
    /// again: a vertical rapid, an XY rapid at safe Z, then the plunge.
    #[test]
    fn links_between_shapes_retract_to_safe_z() {
        let blocks = [
            squares("a", 0.0, 0.0, 20.0, 1),
            squares("b", 50.0, 0.0, 20.0, 1),
        ]
        .concat();
        let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
        let last_cut_of_a = 1 + 4;
        let m = &tp.moves;
        assert_eq!(m[last_cut_of_a].to, Point3::new(0.0, 0.0, 4.0));
        assert_eq!(
            m[last_cut_of_a + 1],
            Move::rapid(Point3::new(0.0, 0.0, SAFE_Z_MM))
        );
        assert_eq!(
            m[last_cut_of_a + 2],
            Move::rapid(Point3::new(50.0, 0.0, SAFE_Z_MM))
        );
        assert_eq!(
            m[last_cut_of_a + 3],
            Move::linear(Point3::new(50.0, 0.0, 4.0), PLUNGE_MM_MIN)
        );
        assert_eq!(m.iter().filter(|m| m.is_rapid()).count(), 4);
        for mv in m.iter().filter(|m| m.is_rapid()) {
            assert!((mv.to.z - SAFE_Z_MM).abs() <= EPS_MM, "{mv:?}");
        }
    }

    fn region(x: f64, y: f64, size: f64) -> Region {
        let contour = Contour::new(Polyline::rect(Point::new(x, y), size, size)).unwrap();
        nest(vec![contour]).unwrap().0.remove(0)
    }

    /// Two cleared blocks at one Z whose starts are `link_mm` apart in X.
    fn cleared_pair(b_x: f64, b_y: f64) -> Vec<CutBlock> {
        let mut a = square("a", 0.0, 0.0, 20.0, 0, 1);
        a.cleared = Some(region(0.0, 0.0, 20.0));
        let mut b = square("b", b_x, b_y, 20.0, 0, 1);
        b.cleared = Some(region(b_x, b_y, 20.0));
        vec![a, b]
    }

    /// The move right after `a`'s last cut (index 5).
    fn link_move(tp: &Toolpath) -> Move {
        tp.moves[6]
    }

    /// O3: a short link inside the union of two cleared areas stays down as
    /// a `Linear` at feed; a link outside the union, longer than
    /// `tool_diameter + stepover`, or from a block without a cleared area
    /// retracts to safe Z.
    #[test]
    fn stay_down_link_only_inside_cleared_area() {
        // b starts at (10, 5), inside a's cleared square; 11.18 mm away.
        let mut inside = input(cleared_pair(10.0, 5.0), MachiningOrder::FullDepthFirst, &[]);
        inside.stepover_mm = 10.0;
        let tp = run(&inside);
        assert_eq!(
            link_move(&tp),
            Move::linear(Point3::new(10.0, 5.0, 4.0), FEED_MM_MIN)
        );
        assert_eq!(
            tp.moves.iter().filter(|m| m.is_rapid()).count(),
            2,
            "{:?}",
            tp.moves
        );
        // The link is followed straight by b's cuts: no plunge at the same Z.
        assert_eq!(
            tp.moves[7],
            Move::linear(Point3::new(30.0, 5.0, 4.0), FEED_MM_MIN)
        );

        // Too long for the threshold: 6.35 + 2 < 11.18.
        let mut long = input(cleared_pair(10.0, 5.0), MachiningOrder::FullDepthFirst, &[]);
        long.stepover_mm = 2.0;
        assert!(link_move(&run(&long)).is_rapid());

        // Outside the union: b starts at (10, 30), above both squares.
        let mut outside = input(
            cleared_pair(10.0, 30.0),
            MachiningOrder::FullDepthFirst,
            &[],
        );
        outside.stepover_mm = 30.0;
        assert!(link_move(&run(&outside)).is_rapid());

        // No cleared area on a: a profile never stays down.
        let mut none = input(cleared_pair(10.0, 5.0), MachiningOrder::FullDepthFirst, &[]);
        none.stepover_mm = 10.0;
        none.blocks[0].cleared = None;
        assert!(link_move(&run(&none)).is_rapid());
    }

    /// Port of `testPocketFeatureFirstOrder` (PureCutCNC
    /// `src/engine/toolpaths/toolpaths.test.ts`): two disjoint shapes, two
    /// passes, full depth first. Upstream reads `[-2, -4, -2, -4]` below the
    /// stock top; here Z is above the stock bottom of 6 mm stock, so the
    /// same passes are `[4, 2, 4, 2]` (O4).
    #[test]
    fn full_depth_first_z_transitions() {
        let blocks = [
            squares("left", 0.0, 0.0, 10.0, 2),
            squares("right", 40.0, 0.0, 10.0, 2),
        ]
        .concat();
        let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
        assert_z(&cut_z_transitions(&tp), &[4.0, 2.0, 4.0, 2.0]);
        // Each half of the move list holds one shape at both depths.
        let mid = tp.moves.len() / 2;
        for half in [&tp.moves[..mid], &tp.moves[mid..]] {
            let zs: Vec<f64> = half
                .iter()
                .filter(|m| !m.is_rapid())
                .map(|m| m.to.z)
                .collect();
            assert!(zs.iter().any(|z| (z - 4.0).abs() <= EPS_MM), "{zs:?}");
            assert!(zs.iter().any(|z| (z - 2.0).abs() <= EPS_MM), "{zs:?}");
        }
    }

    /// Port of `testPocketLevelFirstOrder` (PureCutCNC
    /// `src/engine/toolpaths/toolpaths.test.ts`): the same two shapes under
    /// pass first cut one depth across both, then the next: `[4, 2]`
    /// (upstream `[-2, -4]`) (O4).
    #[test]
    fn pass_first_z_transitions() {
        let blocks = [
            squares("left", 0.0, 0.0, 10.0, 2),
            squares("right", 40.0, 0.0, 10.0, 2),
        ]
        .concat();
        let tp = run(&input(blocks.clone(), MachiningOrder::PassFirst, &[]));
        assert_z(&cut_z_transitions(&tp), &[4.0, 2.0]);
        let got = visits(&tp, &blocks);
        for name in ["left", "right"] {
            for pass in 0..2 {
                assert!(got.contains(&(name.to_string(), pass)), "{got:?}");
            }
        }
    }

    /// Under pass first each layer's walk starts from the previous layer's
    /// last exit, so the second layer runs back the other way.
    #[test]
    fn pass_first_walk_starts_from_the_previous_pass_exit() {
        let blocks = [
            squares("a", 0.0, 0.0, 10.0, 2),
            squares("b", 30.0, 0.0, 10.0, 2),
            squares("c", 60.0, 0.0, 10.0, 2),
        ]
        .concat();
        let tp = run(&input(blocks.clone(), MachiningOrder::PassFirst, &[]));
        assert_eq!(names(&visits(&tp, &blocks)), ["a", "b", "c", "c", "b", "a"]);
    }

    /// O5: two shapes at the same distance from the current exit are
    /// visited in drawing order.
    #[test]
    fn equidistant_shapes_keep_drawing_order() {
        let a = squares("a", 0.0, 0.0, 10.0, 1);
        let b = squares("b", 30.0, 0.0, 10.0, 1);
        let c = squares("c", -30.0, 0.0, 10.0, 1);
        for (blocks, expected) in [
            ([a.clone(), b.clone(), c.clone()].concat(), ["a", "b", "c"]),
            ([a.clone(), c.clone(), b.clone()].concat(), ["a", "c", "b"]),
        ] {
            for order in BOTH {
                let tp = run(&input(blocks.clone(), order, &[]));
                assert_eq!(names(&visits(&tp, &blocks)), expected, "{order:?}");
            }
        }
    }

    /// Port of `testPocketFeatureFirstNearestBlockOrder` (PureCutCNC
    /// `src/engine/toolpaths/toolpaths.test.ts`): shapes at X 0, 30 and 200
    /// targeted as a, c, b are cut a, b, c, each with both passes (O5).
    #[test]
    fn nearest_neighbour_orders_a_c_b_as_a_b_c() {
        let blocks = [
            squares("a", 0.0, 0.0, 10.0, 2),
            squares("c", 200.0, 0.0, 10.0, 2),
            squares("b", 30.0, 0.0, 10.0, 2),
        ]
        .concat();
        let tp = run(&input(blocks.clone(), MachiningOrder::FullDepthFirst, &[]));
        assert_eq!(names(&visits(&tp, &blocks)), ["a", "a", "b", "b", "c", "c"]);
        assert_eq!(cut_z_transitions(&tp).len(), 6, "two passes per shape");
    }

    /// O5: the same input twice gives the same toolpath.
    #[test]
    fn same_input_twice_is_identical() {
        let blocks = [
            squares("a", 0.0, 0.0, 10.0, 3),
            squares("c", 200.0, 0.0, 10.0, 3),
            squares("b", 30.0, 0.0, 10.0, 3),
        ]
        .concat();
        for order in BOTH {
            let first = run(&input(blocks.clone(), order, &[]));
            let second = run(&input(blocks.clone(), order, &[]));
            assert_eq!(first, second, "{order:?}");
        }
    }

    /// O6: a one-shape operation is unchanged by the ordering setting, and
    /// is the phase 1 recipe: rapid in, per pass a plunge and the cuts,
    /// rapid out.
    #[test]
    fn single_shape_is_identical_in_both_orders() {
        let blocks = squares("only", 10.0, 10.0, 20.0, 3);
        let full = run(&input(blocks.clone(), MachiningOrder::FullDepthFirst, &[]));
        let pass = run(&input(blocks.clone(), MachiningOrder::PassFirst, &[]));
        assert_eq!(full, pass);
        let mut expected = vec![Move::rapid(Point3::new(10.0, 10.0, SAFE_Z_MM))];
        for block in &blocks {
            expected.push(Move::linear(
                Point3::new(10.0, 10.0, block.z_mm),
                PLUNGE_MM_MIN,
            ));
            expected.extend(block.cuts.iter().copied());
        }
        expected.push(Move::rapid(Point3::new(10.0, 10.0, SAFE_Z_MM)));
        assert_eq!(full.moves, expected);
    }

    /// Under pass first an encloser's freeing pass waits until the shape it
    /// encloses has finished entirely, even when that shape has more
    /// passes: the encloser's pass 1 (which frees) comes after the enclosed
    /// shape's pass 2.
    #[test]
    fn pass_first_waits_for_enclosed_shapes_freeing_pass() {
        let mut encloser = squares("E", 0.0, 0.0, 100.0, 2);
        encloser[1].frees = true;
        let enclosed = squares("I", 40.0, 40.0, 20.0, 3);
        let blocks = [encloser, enclosed].concat();
        let pairs = pairs(&[("I", "E")]);
        let tp = run(&input(blocks.clone(), MachiningOrder::PassFirst, &pairs));
        let expected: Vec<(String, usize)> = [("I", 0), ("E", 0), ("I", 1), ("I", 2), ("E", 1)]
            .map(|(n, p)| (n.to_string(), p))
            .to_vec();
        assert_eq!(visits(&tp, &blocks), expected);
        let tp = run(&input(
            blocks.clone(),
            MachiningOrder::FullDepthFirst,
            &pairs,
        ));
        assert_eq!(names(&visits(&tp, &blocks)), ["I", "I", "I", "E", "E"]);
    }

    /// Link rule 1: consecutive passes of one shape whose descent starts
    /// where the last pass ended have no link; the plunge goes straight
    /// from the previous pass Z.
    #[test]
    fn same_shape_next_pass_stays_down() {
        let tp = run(&input(
            squares("s", 0.0, 0.0, 20.0, 2),
            MachiningOrder::FullDepthFirst,
            &[],
        ));
        let m = &tp.moves;
        assert_eq!(m[5].to, Point3::new(0.0, 0.0, 4.0), "last cut of pass 0");
        assert_eq!(
            m[6],
            Move::linear(Point3::new(0.0, 0.0, 2.0), PLUNGE_MM_MIN)
        );
        assert_eq!(m.len(), 1 + 2 * 5 + 1);
    }

    /// Link rule 2: a next pass whose descent starts elsewhere (a moved
    /// seam) is reached by a retract to safe Z, an XY rapid and a plunge.
    #[test]
    fn moved_entry_retracts_between_passes() {
        let mut blocks = squares("s", 0.0, 0.0, 20.0, 2);
        let moved = &mut blocks[1];
        moved.descent_from = Point::new(20.0, 0.0);
        moved.entry = Point::new(20.0, 0.0);
        moved.exit = Point::new(20.0, 0.0);
        moved.cuts.rotate_left(1);
        let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
        let m = &tp.moves;
        assert_eq!(m[6], Move::rapid(Point3::new(0.0, 0.0, SAFE_Z_MM)));
        assert_eq!(m[7], Move::rapid(Point3::new(20.0, 0.0, SAFE_Z_MM)));
        assert_eq!(
            m[8],
            Move::linear(Point3::new(20.0, 0.0, 2.0), PLUNGE_MM_MIN)
        );
    }

    #[test]
    fn first_and_last_moves_are_rapids_at_safe_z() {
        let blocks = [
            squares("a", 5.0, 5.0, 10.0, 2),
            squares("b", 40.0, 0.0, 10.0, 2),
        ]
        .concat();
        for order in BOTH {
            let tp = run(&input(blocks.clone(), order, &[]));
            assert_eq!(tp.moves[0], Move::rapid(Point3::new(5.0, 5.0, SAFE_Z_MM)));
            assert_eq!(
                *tp.moves.last().unwrap(),
                Move::rapid(Point3::new(40.0, 0.0, SAFE_Z_MM))
            );
        }
    }

    /// A block's own descent moves are written as given; a descent move
    /// that ends where the tool already is (the previous pass's exit) is
    /// dropped rather than written as a zero-length move.
    #[test]
    fn generator_descent_is_used_and_zero_length_moves_are_dropped() {
        let mut blocks = squares("s", 5.0, 0.0, 20.0, 2);
        // Pass 0 ramps in from (0, 0): plunge to the stock top, ramp to
        // the entry at Z 4.
        blocks[0].descent_from = Point::new(0.0, 0.0);
        blocks[0].descent = vec![
            Move::linear(Point3::new(0.0, 0.0, THICKNESS_MM), PLUNGE_MM_MIN),
            Move::linear(Point3::new(5.0, 0.0, 4.0), FEED_MM_MIN),
        ];
        // Pass 1's descent starts with a move to where pass 0 ended.
        blocks[1].descent = vec![
            Move::linear(Point3::new(5.0, 0.0, 4.0), PLUNGE_MM_MIN),
            Move::linear(Point3::new(5.0, 0.0, 2.0), PLUNGE_MM_MIN),
        ];
        let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
        let m = &tp.moves;
        assert_eq!(m[0], Move::rapid(Point3::new(0.0, 0.0, SAFE_Z_MM)));
        assert_eq!(
            m[1],
            Move::linear(Point3::new(0.0, 0.0, THICKNESS_MM), PLUNGE_MM_MIN)
        );
        assert_eq!(m[2], Move::linear(Point3::new(5.0, 0.0, 4.0), FEED_MM_MIN));
        assert_eq!(m[6].to, Point3::new(5.0, 0.0, 4.0), "last cut of pass 0");
        assert_eq!(
            m[7],
            Move::linear(Point3::new(5.0, 0.0, 2.0), PLUNGE_MM_MIN)
        );
        assert_eq!(m.len(), 1 + 2 + 4 + 1 + 4 + 1);
    }

    /// Two blocks of one shape at one pass are written in generator order
    /// under both machining orders, even when the nearer one is second.
    #[test]
    fn blocks_of_one_unit_keep_generator_order() {
        let mut near = square("s", 0.0, 0.0, 10.0, 0, 2);
        let mut far = square("s", 50.0, 0.0, 10.0, 0, 2);
        near.group = 1;
        far.group = 1;
        let mut near_1 = square("s", 0.0, 0.0, 10.0, 1, 2);
        let mut far_1 = square("s", 50.0, 0.0, 10.0, 1, 2);
        near_1.group = 1;
        far_1.group = 1;
        // Pass 0 ends at the far loop; pass 1 lists the near loop first.
        let blocks = vec![near, far, near_1, far_1];
        for order in BOTH {
            let tp = run(&input(blocks.clone(), order, &[]));
            let starts: Vec<f64> = tp
                .moves
                .iter()
                .filter(|m| !m.is_rapid() && m.to.z < SAFE_Z_MM)
                .filter(|m| {
                    m.to.y.abs() <= EPS_MM
                        && ((m.to.x - 10.0).abs() <= EPS_MM || (m.to.x - 60.0).abs() <= EPS_MM)
                })
                .map(|m| m.to.x)
                .collect();
            assert_eq!(starts, [10.0, 60.0, 10.0, 60.0], "{order:?}");
        }
    }

    #[test]
    fn cut_below_pass_z_is_an_error() {
        let mut blocks = squares("s", 0.0, 0.0, 10.0, 1);
        blocks[0].cuts[1].to.z = 3.5;
        let err = sequence(&input(blocks, MachiningOrder::FullDepthFirst, &[])).unwrap_err();
        assert_eq!(
            err,
            CamError::InvalidBlock {
                shape: shape("s"),
                pass: 0,
                problem: BlockProblem::MoveBelowPass {
                    z_mm: 4.0,
                    lowest_mm: 3.5
                }
            }
        );
    }

    #[test]
    fn non_consecutive_passes_are_an_error() {
        let blocks = vec![
            square("s", 0.0, 0.0, 10.0, 0, 3),
            square("s", 0.0, 0.0, 10.0, 2, 3),
        ];
        let err = sequence(&input(blocks, MachiningOrder::FullDepthFirst, &[])).unwrap_err();
        assert!(
            matches!(
                &err,
                CamError::InvalidBlock {
                    problem: BlockProblem::PassesNotConsecutive { passes },
                    ..
                } if passes == &[0, 2]
            ),
            "{err:?}"
        );
    }

    #[test]
    fn plunge_away_from_entry_is_an_error() {
        let mut blocks = squares("s", 0.0, 0.0, 10.0, 1);
        blocks[0].descent_from = Point::new(3.0, 0.0);
        let err = sequence(&input(blocks, MachiningOrder::FullDepthFirst, &[])).unwrap_err();
        assert!(
            matches!(
                err,
                CamError::InvalidBlock {
                    problem: BlockProblem::PlungeAwayFromEntry,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn containment_cycle_is_an_error() {
        let blocks = [
            squares("a", 0.0, 0.0, 10.0, 1),
            squares("b", 30.0, 0.0, 10.0, 1),
        ]
        .concat();
        let pairs = pairs(&[("a", "b"), ("b", "a")]);
        for order in BOTH {
            let err = sequence(&input(blocks.clone(), order, &pairs)).unwrap_err();
            assert!(matches!(err, CamError::ContainmentCycle { .. }), "{err:?}");
        }
    }

    /// `plan` is the order `sequence` writes: each unit a run of generator
    /// order indices.
    #[test]
    fn plan_matches_the_emitted_order() {
        let blocks = [
            squares("a", 0.0, 0.0, 10.0, 2),
            squares("c", 200.0, 0.0, 10.0, 2),
            squares("b", 30.0, 0.0, 10.0, 2),
        ]
        .concat();
        let input = input(blocks, MachiningOrder::PassFirst, &[]);
        assert_eq!(plan(&input).unwrap(), [0, 4, 2, 3, 5, 1]);
    }
}
