// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/multiFeature.ts
//   src/engine/toolpaths/geometry.ts (greedyNearestNeighbor)
//   src/engine/toolpaths/pocket.ts (transitionToCutEntry)

//! Tests of [`sequence`] and [`plan`]: the order blocks are cut in and
//! the links between them.

use super::*;
use crate::error::BlockProblem;
use crate::test_support::{Limits, assert_safe_within};
use crate::toolpath::{Move, MoveKind};
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
        entry_z_mm: z,
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
        clearance_z_mm: SAFE_Z_MM,
        stock_top_mm: THICKNESS_MM,
        tool_diameter_mm: DIAMETER_MM,
        stepover_mm: 0.0,
        feed_mm_min: FEED_MM_MIN,
        plunge_mm_min: PLUNGE_MM_MIN,
        cut_before: &[],
        groove_link_z_mm: None,
    }
}

fn limits() -> Limits {
    Limits {
        safe_z_mm: SAFE_Z_MM,
        stock_top_mm: THICKNESS_MM,
        floor_z_mm: 0.0,
        feed_mm_min: FEED_MM_MIN,
        plunge_mm_min: PLUNGE_MM_MIN,
        level_floor_mm_min: FEED_MM_MIN,
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
                MachiningOrder::PassFirst => [("hole", 0), ("outer", 0), ("hole", 1), ("outer", 1)],
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
/// again: a vertical rapid, an XY rapid at safe Z, a rapid straight
/// down to 1 mm above the stock, then the plunge. (changed in #227: the
/// plunge started at safe Z.)
#[test]
fn links_between_shapes_retract_to_safe_z() {
    let blocks = [
        squares("a", 0.0, 0.0, 20.0, 1),
        squares("b", 50.0, 0.0, 20.0, 1),
    ]
    .concat();
    let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
    let last_cut_of_a = 2 + 4;
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
        Move::rapid(Point3::new(50.0, 0.0, THICKNESS_MM + 1.0))
    );
    assert_eq!(
        m[last_cut_of_a + 4],
        Move::linear(Point3::new(50.0, 0.0, 4.0), PLUNGE_MM_MIN)
    );
    assert_eq!(m.iter().filter(|m| m.is_rapid()).count(), 6);
    for (i, mv) in m.iter().enumerate().filter(|(_, m)| m.is_rapid()) {
        let down = i > 0 && mv.to.z < m[i - 1].to.z - EPS_MM;
        let z = if down { THICKNESS_MM + 1.0 } else { SAFE_Z_MM };
        assert!((mv.to.z - z).abs() <= EPS_MM, "{mv:?}");
    }
}

/// The clearance Z of the #283 tests: 20 mm above the stock top, well
/// above `SAFE_Z_MM`.
const CLEARANCE_Z_MM: f64 = THICKNESS_MM + 20.0;

/// #283, design addendum A7: a link between blocks of different
/// instances rises to the clearance Z and crosses there; S2 still
/// holds (the rapid down stops 1 mm above the stock, then the plunge).
/// Two squares, each its own instance: the link from `a` to `b` is a
/// rapid up to the clearance Z, a rapid in XY at it, then the descent.
#[test]
fn links_between_instances_rise_to_clearance() {
    let blocks = [
        squares("a", 0.0, 0.0, 20.0, 1),
        squares("b", 50.0, 0.0, 20.0, 1),
    ]
    .concat();
    let mut sequenced = input(blocks, MachiningOrder::FullDepthFirst, &[]);
    sequenced.clearance_z_mm = CLEARANCE_Z_MM;
    let tp = run(&sequenced);
    let last_cut_of_a = 2 + 4;
    let m = &tp.moves;
    assert_eq!(m[last_cut_of_a].to, Point3::new(0.0, 0.0, 4.0));
    assert_eq!(
        m[last_cut_of_a + 1],
        Move::rapid(Point3::new(0.0, 0.0, CLEARANCE_Z_MM))
    );
    assert_eq!(
        m[last_cut_of_a + 2],
        Move::rapid(Point3::new(50.0, 0.0, CLEARANCE_Z_MM))
    );
    assert_eq!(
        m[last_cut_of_a + 3],
        Move::rapid(Point3::new(50.0, 0.0, THICKNESS_MM + 1.0))
    );
    assert_eq!(
        m[last_cut_of_a + 4],
        Move::linear(Point3::new(50.0, 0.0, 4.0), PLUNGE_MM_MIN)
    );
    // Every XY rapid between the two runs at the clearance Z; the
    // first approach and the last retract stay at safe Z, for the
    // emitter to rise from (design addendum A7, "Emitter").
    let xy_rapids: Vec<&Move> = m
        .iter()
        .zip(m.iter().skip(1))
        .filter(|(a, b)| b.is_rapid() && a.to.xy().distance_to(b.to.xy()) > EPS_MM)
        .map(|(_, b)| b)
        .collect();
    assert_eq!(xy_rapids.len(), 1, "{m:?}");
    assert!(
        (xy_rapids[0].to.z - CLEARANCE_Z_MM).abs() <= EPS_MM,
        "{m:?}"
    );
    assert_eq!(m[0], Move::rapid(Point3::new(0.0, 0.0, SAFE_Z_MM)));
    assert_eq!(
        m.last(),
        Some(&Move::rapid(Point3::new(50.0, 0.0, SAFE_Z_MM)))
    );
}

/// One instance's square outline (profile 0) and a square hole in it
/// (profile 1).
fn part_square(profile: usize, x: f64, y: f64, size: f64, passes: usize) -> Vec<CutBlock> {
    squares("part", x, y, size, passes)
        .into_iter()
        .map(|mut b| {
            b.shape.profile = profile;
            b
        })
        .collect()
}

/// #283, design addendum A7: links within one instance, between its
/// passes and between its hole and its outline, stay at safe Z, as
/// Fusion's retract height does, even with a clearance Z set. The hole
/// is cut first (O1), each square in two passes with the second
/// starting where the first ended (no link), and the one retract from
/// the hole to the outline crosses at safe Z.
#[test]
fn passes_of_one_instance_link_at_safe_z() {
    let outline = part_square(0, 0.0, 0.0, 100.0, 2);
    let hole = part_square(1, 40.0, 40.0, 20.0, 2);
    let containment = vec![(hole[0].shape.clone(), outline[0].shape.clone())];
    let blocks = [outline, hole].concat();
    let mut sequenced = input(blocks, MachiningOrder::FullDepthFirst, &containment);
    sequenced.clearance_z_mm = CLEARANCE_Z_MM;
    let tp = run(&sequenced);
    assert!(
        tp.moves.iter().all(|mv| mv.to.z <= SAFE_Z_MM + EPS_MM),
        "nothing rises above safe Z: {:?}",
        tp.moves
    );
    let crossings: Vec<&Move> = tp
        .moves
        .iter()
        .zip(tp.moves.iter().skip(1))
        .filter(|(a, b)| b.is_rapid() && a.to.xy().distance_to(b.to.xy()) > EPS_MM)
        .map(|(_, b)| b)
        .collect();
    assert_eq!(crossings.len(), 1, "{:?}", tp.moves);
    assert_eq!(crossings[0].to, Point3::new(0.0, 0.0, SAFE_Z_MM));
    // Against the same blocks as two instances, which do rise.
    let mut apart = sequenced.clone();
    for block in &mut apart.blocks {
        if block.shape.profile == 1 {
            block.shape.instance = InstanceId::new("other");
        }
    }
    let other_containment = vec![(apart.blocks[2].shape.clone(), apart.blocks[0].shape.clone())];
    apart.containment = &other_containment;
    let tp = run(&apart);
    assert!(
        tp.moves
            .iter()
            .any(|mv| (mv.to.z - CLEARANCE_Z_MM).abs() <= EPS_MM),
        "{:?}",
        tp.moves
    );
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

/// The move right after `a`'s last cut (index 6: a rapid in, a rapid
/// down, the plunge, four cuts).
fn link_move(tp: &Toolpath) -> Move {
    tp.moves[7]
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
        3,
        "{:?}",
        tp.moves
    );
    // The link is followed straight by b's cuts: no plunge at the same Z.
    assert_eq!(
        tp.moves[8],
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

/// Slice 9: a block whose descent is a ramp or helix keeps it for when
/// the tool arrives from above. Reached by a stay-down link, the tool
/// is already at the block's Z in cleared material, so the link runs
/// straight to the block's entry and the descent is not cut: replaying
/// it from `descent_from` would climb back up through cleared air.
#[test]
fn stay_down_link_skips_the_descent() {
    let mut blocks = cleared_pair(10.0, 5.0);
    // b ramps in from (14, 5) at Z 4.4 down to its entry (10, 5) at Z 4.
    blocks[1].descent_from = Point::new(14.0, 5.0);
    blocks[1].descent_from_z_mm = 4.4;
    blocks[1].descent = vec![
        Move::linear(Point3::new(12.0, 5.0, 4.2), FEED_MM_MIN),
        Move::linear(Point3::new(10.0, 5.0, 4.0), FEED_MM_MIN),
    ];
    let mut stay = input(blocks.clone(), MachiningOrder::FullDepthFirst, &[]);
    stay.stepover_mm = 10.0;
    let tp = run(&stay);
    assert_eq!(
        link_move(&tp),
        Move::linear(Point3::new(10.0, 5.0, 4.0), FEED_MM_MIN),
        "{:?}",
        tp.moves
    );
    assert_eq!(
        tp.moves[8],
        Move::linear(Point3::new(30.0, 5.0, 4.0), FEED_MM_MIN),
        "b's cuts follow the link: {:?}",
        tp.moves
    );
    assert!(
        tp.moves
            .iter()
            .all(|m| m.to.z <= 4.0 + EPS_MM || m.is_rapid()),
        "no descent move is replayed: {:?}",
        tp.moves
    );

    // Reached by a retract, the descent is cut as given, after a rapid
    // straight down to 1 mm above the stock top (nothing was cut at
    // (14, 5)) and a plunge to where it starts (#214).
    let mut retract = input(blocks, MachiningOrder::FullDepthFirst, &[]);
    retract.stepover_mm = 2.0;
    let tp = run(&retract);
    let at = tp
        .moves
        .iter()
        .position(|m| *m == Move::linear(Point3::new(12.0, 5.0, 4.2), FEED_MM_MIN))
        .expect("the descent is cut");
    assert_eq!(
        tp.moves[at - 1],
        Move::linear(Point3::new(14.0, 5.0, 4.4), PLUNGE_MM_MIN)
    );
    assert_eq!(
        tp.moves[at - 2],
        Move::rapid(Point3::new(14.0, 5.0, THICKNESS_MM + 1.0))
    );
    assert_eq!(
        tp.moves[at - 3],
        Move::rapid(Point3::new(14.0, 5.0, SAFE_Z_MM))
    );
}

/// #214, S2: after a lift, the tool reaches a descent that starts below
/// safe Z by a rapid straight down, which stops 1 mm above the lowest
/// floor a cut written so far passed through that point, or 1 mm above
/// the stock top where none did, but never below the descent's start;
/// a plunge at the plunge rate covers the rest.
#[test]
fn rapid_down_to_a_descent_stops_a_clearance_above_the_cut_or_the_stock() {
    let ramp_in = |name: &str, from: Point, entry: Point| {
        let mut b = square(name, entry.x, entry.y, 5.0, 0, 1);
        b.descent_from = from;
        b.descent_from_z_mm = 4.5;
        // 0.5 mm down over 5 mm: the feed is slow enough (S10).
        b.descent = vec![Move::linear(
            Point3::new(entry.x, entry.y, 4.0),
            FEED_MM_MIN,
        )];
        b
    };
    let blocks = vec![
        square("a", 0.0, 0.0, 20.0, 0, 1),
        // (10, 0) is on a's first side, cut at Z 4.
        ramp_in("b", Point::new(10.0, 0.0), Point::new(15.0, 0.0)),
        // (60, 60) was never cut.
        ramp_in("c", Point::new(60.0, 60.0), Point::new(65.0, 60.0)),
    ];
    let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
    for (at, rapid_z) in [(Point::new(10.0, 0.0), 5.0), (Point::new(60.0, 60.0), 7.0)] {
        let i = tp
            .moves
            .iter()
            .position(|m| m.is_rapid() && m.to.xy() == at && m.to.z < SAFE_Z_MM)
            .unwrap_or_else(|| panic!("a rapid down at {at:?}: {:?}", tp.moves));
        assert_eq!(
            tp.moves[i - 1],
            Move::rapid(Point3::new(at.x, at.y, SAFE_Z_MM))
        );
        assert_eq!(tp.moves[i], Move::rapid(Point3::new(at.x, at.y, rapid_z)));
        assert_eq!(
            tp.moves[i + 1],
            Move::linear(Point3::new(at.x, at.y, 4.5), PLUNGE_MM_MIN)
        );
    }
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

/// #215: two shapes nearer each other than [`NEAREST_TIE_MM`] in their
/// distance from the current exit are a tie, kept in drawing order, so
/// the 1e-6 mm noise of an `f32` import does not pick the later one.
#[test]
fn equidistant_shapes_within_tolerance_keep_drawing_order() {
    let a = squares("a", 0.0, 0.0, 10.0, 1);
    for noise_mm in [1e-6, 1e-4, 0.9 * NEAREST_TIE_MM] {
        let b = squares("b", 30.0, 0.0, 10.0, 1);
        let c = squares("c", -30.0 + noise_mm, 0.0, 10.0, 1);
        for (blocks, expected) in [
            ([a.clone(), b.clone(), c.clone()].concat(), ["a", "b", "c"]),
            ([a.clone(), c.clone(), b.clone()].concat(), ["a", "c", "b"]),
        ] {
            for order in BOTH {
                let tp = run(&input(blocks.clone(), order, &[]));
                assert_eq!(
                    names(&visits(&tp, &blocks)),
                    expected,
                    "{order:?}, c {noise_mm} mm nearer"
                );
            }
        }
    }
    // Nearer by more than the tolerance is nearer.
    let b = squares("b", 30.0, 0.0, 10.0, 1);
    let c = squares("c", -30.0 + 2.0 * NEAREST_TIE_MM, 0.0, 10.0, 1);
    let blocks = [a, b, c].concat();
    for order in BOTH {
        let tp = run(&input(blocks.clone(), order, &[]));
        assert_eq!(names(&visits(&tp, &blocks)), ["a", "c", "b"], "{order:?}");
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
/// rapid out. (changed in #227: a rapid straight down to 1 mm above
/// the stock comes before the first plunge.)
#[test]
fn single_shape_is_identical_in_both_orders() {
    let blocks = squares("only", 10.0, 10.0, 20.0, 3);
    let full = run(&input(blocks.clone(), MachiningOrder::FullDepthFirst, &[]));
    let pass = run(&input(blocks.clone(), MachiningOrder::PassFirst, &[]));
    assert_eq!(full, pass);
    let mut expected = vec![
        Move::rapid(Point3::new(10.0, 10.0, SAFE_Z_MM)),
        Move::rapid(Point3::new(10.0, 10.0, THICKNESS_MM + 1.0)),
    ];
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

/// A relief-notch-like block of shape `name` at `pass`: a plunge at
/// (`x`, `y`) from safe Z, out 1 mm down and left, and back.
fn notch(name: &str, x: f64, y: f64, pass: usize) -> CutBlock {
    let z = PASS_Z_MM[pass];
    let at = Point::new(x, y);
    CutBlock {
        shape: shape(name),
        pass,
        z_mm: z,
        descent_from: at,
        descent_from_z_mm: SAFE_Z_MM,
        descent: Vec::new(),
        entry: at,
        entry_z_mm: z,
        cuts: vec![
            Move::linear(Point3::new(x - 1.0, y - 1.0, z), FEED_MM_MIN),
            Move::linear(Point3::new(x, y, z), FEED_MM_MIN),
        ],
        exit: at,
        frees: false,
        cleared: None,
        group: 0,
    }
}

/// #189: nothing of a shape is cut after its freeing pass. Each pass of
/// an outline and of its hole is followed, in generator order, by a
/// notch block at that pass, as `relief::profile_notches` hands them
/// over. The sequencer cuts a unit's freeing blocks last, so the
/// notches at the last pass come before the lap that frees, under both
/// orders and both drawing orders, and O1 still holds: the hole is
/// finished before the outline's freeing lap.
#[test]
fn nothing_is_cut_after_a_shapes_freeing_pass() {
    let with_notches = |name: &str, x: f64, y: f64, size: f64| -> Vec<CutBlock> {
        let mut blocks = squares(name, x, y, size, 3);
        blocks.extend((0..3).map(|pass| notch(name, x, y, pass)));
        blocks
    };
    let outer = with_notches("outer", 0.0, 0.0, 100.0);
    let hole = with_notches("hole", 40.0, 40.0, 20.0);
    let pairs = pairs(&[("hole", "outer")]);
    for order in BOTH {
        for blocks in [
            [outer.clone(), hole.clone()].concat(),
            [hole.clone(), outer.clone()].concat(),
        ] {
            let tp = run(&input(blocks.clone(), order, &pairs));
            // Where each block's first cut is in the toolpath.
            let at: Vec<usize> = blocks
                .iter()
                .map(|b| {
                    tp.moves
                        .iter()
                        .position(|m| *m == b.cuts[0])
                        .unwrap_or_else(|| panic!("{:?} pass {} not cut", b.shape, b.pass))
                })
                .collect();
            let freeing = |name: &str| {
                blocks
                    .iter()
                    .zip(&at)
                    .find(|(b, _)| b.frees && b.shape.instance_name == name)
                    .map(|(_, &i)| i)
                    .unwrap()
            };
            for (b, &i) in blocks.iter().zip(&at) {
                let name = b.shape.instance_name.as_str();
                assert!(
                    i <= freeing(name),
                    "{order:?}: {name} pass {} is cut after its freeing pass",
                    b.pass
                );
                if name == "hole" {
                    assert!(i < freeing("outer"), "{order:?}: O1");
                }
            }
        }
    }
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
    assert_eq!(m[6].to, Point3::new(0.0, 0.0, 4.0), "last cut of pass 0");
    assert_eq!(
        m[7],
        Move::linear(Point3::new(0.0, 0.0, 2.0), PLUNGE_MM_MIN)
    );
    assert_eq!(m.len(), 2 + 2 * 5 + 1);
}

/// Link rule 2: a next pass whose descent starts elsewhere (a moved
/// seam) is reached by a retract to safe Z, an XY rapid, a rapid down
/// to 1 mm above the floor pass 0 cut there, and a plunge (changed in
/// #227: the plunge started at safe Z).
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
    assert_eq!(m[7], Move::rapid(Point3::new(0.0, 0.0, SAFE_Z_MM)));
    assert_eq!(m[8], Move::rapid(Point3::new(20.0, 0.0, SAFE_Z_MM)));
    assert_eq!(m[9], Move::rapid(Point3::new(20.0, 0.0, 5.0)));
    assert_eq!(
        m[10],
        Move::linear(Point3::new(20.0, 0.0, 2.0), PLUNGE_MM_MIN)
    );
}

/// Link rule 1 (#240): a block of the same shape and pass whose
/// descent is a straight plunge starting where the tool is, inside the
/// column the previous block just cut to at least the plunge's end,
/// has no link. The plunge goes from where the tool is (dropped when
/// it is already there, a move up at the plunge rate when the block
/// starts higher), not via safe Z. A block of another shape, or one
/// that would plunge below what was cut there, still retracts.
#[test]
fn block_starting_where_the_tool_is_in_its_own_cut_does_not_lift() {
    let square = square("s", 0.0, 0.0, 20.0, 0, 1);
    let lap_end = Point3::new(0.0, 0.0, 4.0);
    // A relief notch block as `relief` builds it: its descent is one
    // plunge at the descend point from safe Z.
    let relief_notch = |name: &str, z: f64| {
        let mut block = notch(name, 0.0, 0.0, 0);
        block.z_mm = z;
        block.entry_z_mm = z;
        block.descent = vec![Move::linear(Point3::new(0.0, 0.0, z), PLUNGE_MM_MIN)];
        for cut in &mut block.cuts {
            cut.to.z = z;
        }
        block
    };
    let sequenced = |next: CutBlock| {
        let first_cut = next.cuts[0];
        let tp = run(&input(
            vec![square.clone(), next],
            MachiningOrder::FullDepthFirst,
            &[],
        ));
        assert_eq!(tp.moves[6].to, lap_end, "the square's last cut");
        let notch_at = tp.moves.iter().position(|m| *m == first_cut).unwrap();
        tp.moves[7..notch_at].to_vec()
    };

    // At the lap's Z: nothing between the two blocks.
    assert_eq!(sequenced(notch("s", 0.0, 0.0, 0)), vec![]);
    assert_eq!(sequenced(relief_notch("s", 4.0)), vec![]);
    // Above it: straight up in the open column.
    assert_eq!(
        sequenced(relief_notch("s", 5.0)),
        vec![Move::linear(Point3::new(0.0, 0.0, 5.0), PLUNGE_MM_MIN)]
    );
    // Another shape, or below the cut: up to safe Z and back down.
    for next in [relief_notch("t", 4.0), relief_notch("s", 2.0)] {
        let link = sequenced(next);
        assert_eq!(
            link.first(),
            Some(&Move::rapid(Point3::new(0.0, 0.0, SAFE_Z_MM))),
            "{link:?}"
        );
    }
}

#[test]
fn first_and_last_moves_are_rapids_at_safe_z() {
    let blocks = [
        squares("a", 5.0, 5.0, 10.0, 2),
        squares("b", 40.0, 0.0, 10.0, 2),
    ]
    .concat();
    // Full depth first ends at b; pass first walks back and ends at a.
    for (order, last_xy) in [
        (MachiningOrder::FullDepthFirst, Point::new(40.0, 0.0)),
        (MachiningOrder::PassFirst, Point::new(5.0, 5.0)),
    ] {
        let tp = run(&input(blocks.clone(), order, &[]));
        assert_eq!(tp.moves[0], Move::rapid(Point3::new(5.0, 5.0, SAFE_Z_MM)));
        assert_eq!(
            *tp.moves.last().unwrap(),
            Move::rapid(Point3::new(last_xy.x, last_xy.y, SAFE_Z_MM)),
            "{order:?}"
        );
    }
}

/// A block's own descent moves are written as given; a descent move
/// that ends where the tool already is (the previous pass's exit) is
/// dropped rather than written as a zero-length move. A descent that
/// opens with a straight plunge from safe Z is reached by a rapid
/// straight down to 1 mm above the stock first (#227).
#[test]
fn generator_descent_is_used_and_zero_length_moves_are_dropped() {
    let mut blocks = squares("s", 5.0, 0.0, 20.0, 2);
    // Pass 0 ramps in from (0, 0): plunge to the stock top, ramp to
    // the entry at Z 4, 2 mm down over 5 mm at the feed scaled so it
    // falls at the plunge rate (S10).
    let ramp_mm_min = PLUNGE_MM_MIN * 29f64.sqrt() / 2.0;
    blocks[0].descent_from = Point::new(0.0, 0.0);
    blocks[0].descent = vec![
        Move::linear(Point3::new(0.0, 0.0, THICKNESS_MM), PLUNGE_MM_MIN),
        Move::linear(Point3::new(5.0, 0.0, 4.0), ramp_mm_min),
    ];
    // Pass 1's descent starts with a move to where pass 0 ended.
    blocks[1].descent = vec![
        Move::linear(Point3::new(5.0, 0.0, 4.0), PLUNGE_MM_MIN),
        Move::linear(Point3::new(5.0, 0.0, 2.0), PLUNGE_MM_MIN),
    ];
    let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
    let m = &tp.moves;
    assert_eq!(m[0], Move::rapid(Point3::new(0.0, 0.0, SAFE_Z_MM)));
    assert_eq!(m[1], Move::rapid(Point3::new(0.0, 0.0, THICKNESS_MM + 1.0)));
    assert_eq!(
        m[2],
        Move::linear(Point3::new(0.0, 0.0, THICKNESS_MM), PLUNGE_MM_MIN)
    );
    assert_eq!(m[3], Move::linear(Point3::new(5.0, 0.0, 4.0), ramp_mm_min));
    assert_eq!(m[7].to, Point3::new(5.0, 0.0, 4.0), "last cut of pass 0");
    assert_eq!(
        m[8],
        Move::linear(Point3::new(5.0, 0.0, 2.0), PLUNGE_MM_MIN)
    );
    assert_eq!(m.len(), 2 + 2 + 4 + 1 + 4 + 1);
}

/// A dwell ends where the tool already is, but it is a pause, not a
/// zero-length move: it is written.
#[test]
fn dwell_moves_are_kept() {
    let mut blocks = squares("s", 5.0, 0.0, 20.0, 1);
    let at = blocks[0].cuts[0].to;
    blocks[0].cuts.insert(1, Move::dwell(at, 0.5));
    let tp = run(&input(blocks, MachiningOrder::FullDepthFirst, &[]));
    assert!(
        tp.moves
            .iter()
            .any(|m| m.kind == MoveKind::Dwell { seconds: 0.5 }),
        "{:?}",
        tp.moves
    );
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

/// Code review finding 1.2: a unit with no block has no start to
/// travel to. The sequencer refuses it; it used to start it at the
/// origin.
#[test]
fn a_unit_with_no_block_is_an_internal_defect() {
    assert_eq!(
        Unit::new(&[]).unwrap_err(),
        CamError::internal("a pass of a shape has no block to cut")
    );
}

/// Code review finding 1.2: a shape that encloses an index the
/// sequencer has no shape for used to count as finished, which let
/// its encloser be cut free first. Both walks refuse it.
#[test]
fn enclosing_a_shape_that_is_not_there_is_an_internal_defect() {
    for order in [MachiningOrder::FullDepthFirst, MachiningOrder::PassFirst] {
        let blocks = squares("s", 0.0, 0.0, 10.0, 1);
        let mut shapes = Shapes::new(&input(blocks, order, &[])).unwrap();
        shapes.shapes[0].enclosed.push(7);
        assert_eq!(
            shapes.walk(order).unwrap_err(),
            CamError::internal("a shape encloses one the sequencer does not have"),
            "{order:?}"
        );
    }
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

/// Security review H1 (`docs/specs/phase-3/design.md`): a block whose
/// cuts start above its deepest Z, as a V-Carve chain from a corner tip
/// at the surface does, is plunged to its entry Z, not to `z_mm`. The
/// chain here starts at Z 4 at (0, 0) and sinks to its pass Z 1 at
/// (10, 0); a plunge to Z 1 would cut 3 mm below the surface it starts
/// on.
#[test]
fn a_plunge_ends_at_the_blocks_entry_z_not_its_deepest_z() {
    let at = Point::new(0.0, 0.0);
    let chain = CutBlock {
        shape: shape("v"),
        pass: 0,
        z_mm: 1.0,
        descent_from: at,
        descent_from_z_mm: SAFE_Z_MM,
        descent: Vec::new(),
        entry: at,
        entry_z_mm: 4.0,
        cuts: vec![
            Move::linear(Point3::new(10.0, 0.0, 1.0), FEED_MM_MIN),
            Move::linear(Point3::new(20.0, 0.0, 1.0), FEED_MM_MIN),
        ],
        exit: Point::new(20.0, 0.0),
        frees: false,
        cleared: None,
        group: 0,
    };
    let first_cut = chain.cuts[0];
    let tp = run(&input(vec![chain], MachiningOrder::FullDepthFirst, &[]));
    let cut_at = tp.moves.iter().position(|m| *m == first_cut).unwrap();
    let before = &tp.moves[..cut_at];
    assert_eq!(
        before.last(),
        Some(&Move::linear(Point3::new(0.0, 0.0, 4.0), PLUNGE_MM_MIN)),
        "the plunge ends at the entry Z: {:?}",
        tp.moves
    );
    assert!(
        before.iter().all(|m| m.to.z >= 4.0 - EPS_MM),
        "nothing goes below the entry Z before the first cut: {:?}",
        tp.moves
    );
}

/// Security review H1: `entry_z_mm` is where cutting begins, so it is
/// never below the pass Z, the block's deepest.
#[test]
fn validate_block_rejects_an_entry_below_the_deepest_z() {
    let mut blocks = squares("s", 0.0, 0.0, 10.0, 1);
    blocks[0].entry_z_mm = 3.5;
    let err = sequence(&input(blocks, MachiningOrder::FullDepthFirst, &[])).unwrap_err();
    assert_eq!(
        err,
        CamError::InvalidBlock {
            shape: shape("s"),
            pass: 0,
            problem: BlockProblem::EntryBelowPass {
                z_mm: 4.0,
                entry_z_mm: 3.5
            }
        }
    );
}

/// Security review H1: the first cut starts at `(entry, entry_z_mm)`,
/// so a descent that ends anywhere else, away in XY or at another Z,
/// is refused.
#[test]
fn validate_block_rejects_a_first_cut_away_from_the_entry() {
    for end in [Point3::new(3.0, 0.0, 4.0), Point3::new(0.0, 0.0, 5.0)] {
        let mut blocks = squares("s", 0.0, 0.0, 10.0, 1);
        blocks[0].descent = vec![Move::linear(end, PLUNGE_MM_MIN)];
        let err = sequence(&input(blocks, MachiningOrder::FullDepthFirst, &[]))
            .expect_err("a descent ending away from the entry is refused");
        assert_eq!(
            err,
            CamError::InvalidBlock {
                shape: shape("s"),
                pass: 0,
                problem: BlockProblem::FirstCutAwayFromEntry { start: end }
            },
            "descent ending at {end:?}"
        );
    }
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

/// #232: a link between two points on a cleared area's boundary that
/// bridges a dent of the material into it, shorter than the old
/// 0.5 mm sample step. A 20 mm square with a diamond hole whose two
/// tips (10, 10.15) and (10, 9.85) are 0.3 mm apart: the straight link
/// between them runs through the diamond, crossing none of its edges
/// properly (it touches them only at its ends), and no sample point
/// was tested. It is refused; a link of the same length beside the
/// diamond, in the material, is still inside.
#[test]
fn short_chord_between_boundary_points_is_not_inside() {
    let outer = Contour::new(Polyline::rect(Point::new(0.0, 0.0), 20.0, 20.0)).unwrap();
    let diamond = Contour::new(Polyline::new(
        [(10.0, 10.15), (9.0, 10.0), (10.0, 9.85), (10.05, 10.0)]
            .into_iter()
            .map(|(x, y)| dc_geometry::Vertex::line(x, y))
            .collect(),
        true,
    ))
    .unwrap();
    let cleared = nest(vec![outer, diamond]).unwrap().0.remove(0);
    assert_eq!(cleared.holes().len(), 1);
    let (top, bottom) = (Point::new(10.0, 10.15), Point::new(10.0, 9.85));
    assert!(!link_inside_cleared(&cleared, &cleared, top, bottom));
    assert!(!link_inside_cleared(&cleared, &cleared, bottom, top));
    let (a, b) = (Point::new(12.0, 10.15), Point::new(12.0, 9.85));
    assert!(link_inside_cleared(&cleared, &cleared, a, b));
}

/// #464, S2: a rapid straight down over a steep cut (a 15° V-bit's
/// descent, here falling 4.4 mm per mm) stops 1 mm above the cut's
/// highest Z within [`CLEARED_POINT_TOLERANCE_MM`] along it of where it
/// passes nearest, not 1 mm above that nearest point. The safety pass
/// finds the nearest point on the cut as written, rounded, which on so
/// steep a cut can stand a few thousandths higher: a stress test's
/// male stopped its rapid at 5.844 over a floor the program put at
/// 4.846, and S2 refused it.
#[test]
fn rapid_down_over_a_steep_cut_clears_the_floor_as_written() {
    let mut steep = square("a", 10.0, 0.0, 5.0, 0, 1);
    let end = Point3::new(10.048, 0.0, 4.7888);
    steep.entry_z_mm = 5.0;
    steep.z_mm = end.z;
    // S10: the descent's feed holds it to the plunge rate.
    let (run_mm, drop_mm) = (0.048_f64, 5.0 - end.z);
    let feed = PLUNGE_MM_MIN * run_mm.hypot(drop_mm) / drop_mm;
    steep.cuts = vec![Move::linear(end, feed)];
    steep.exit = end.xy();
    let at = Point::new(10.0364, 0.0004);
    let mut ramp = square("b", 30.0, 0.0, 5.0, 0, 1);
    ramp.descent_from = at;
    ramp.descent_from_z_mm = 4.5;
    ramp.descent = vec![Move::linear(Point3::new(30.0, 0.0, 4.0), FEED_MM_MIN)];
    let tp = run(&input(
        vec![steep, ramp],
        MachiningOrder::FullDepthFirst,
        &[],
    ));
    let rapid = tp
        .moves
        .iter()
        .find(|m| m.is_rapid() && m.to.xy() == at && m.to.z < SAFE_Z_MM)
        .unwrap_or_else(|| panic!("a rapid down at {at:?}: {:?}", tp.moves));
    // The cut's Z 0.01 mm back from the nearest point, 0.0364 along.
    let floor_mm = 5.0 - (5.0 - end.z) * (0.0364 - CLEARED_POINT_TOLERANCE_MM) / 0.048;
    assert!(
        (rapid.to.z - (floor_mm + ENTRY_CLEARANCE_MM)).abs() <= 1e-9,
        "the rapid stops at {}, not {}",
        rapid.to.z,
        floor_mm + ENTRY_CLEARANCE_MM
    );
}
