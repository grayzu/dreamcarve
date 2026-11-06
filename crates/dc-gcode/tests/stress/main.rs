// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Stress tests of V-Carve and Inlay on generated difficult designs
//! (#464). The 3.3 bench check found generator bugs on valid geometry the
//! fixtures never exercised (#452, #463, and #465 to #475 after them):
//! line art, thin ridges, many small regions, sharp notches. These tests
//! carve many such designs, drawn at random but deterministic from a seed
//! ([`designs`], [`lettering`]), with every V-Carve and Inlay setting
//! that matters, and hold each to the property of [`property`].
//!
//! They live here, not in dc-cam, because only this crate sees both the
//! generators and the safety pass (as `tests/properties`); the seat check
//! is dc-cam's (`dc_cam::test_support::seat`, the `test-support`
//! feature).
//!
//! **Cost.** The CI tests carve [`CI_CASES`] small designs of each family
//! through the whole property (two line-art motifs, or two or three
//! letters up to 10 mm tall, a narrower band), each by three of the seven
//! operations in turn, with arc fitting off (#480), and
//! [`OWN_MOVE_CASES`] small designs of each by all seven through
//! generation alone. In a debug build they take about 50 s of wall time
//! and 100 s of CPU (the lettering test is the longest, 48 s), run in
//! parallel with the crate's other tests. The large sweep is ignored; run
//! it with a release build, on every core:
//!
//! ```sh
//! cargo test --release -p dc-gcode --test stress -- --ignored sweep --nocapture
//! ```
//!
//! `STRESS_CASES` (default [`SWEEP_CASES`]) sets how many designs of each
//! family it carves, and `STRESS_SEED` (default 0) the first seed;
//! `STRESS_VERBOSE` prints each step's time. A failure names its family
//! and seed, which [`designs::line_art`] or [`lettering::lettering`]
//! redraw, and [`designs::line_art_keeping`] narrows to a motif. Seat
//! failures of an open issue are reported as known
//! (`property::known_issue`), not failed, and so is a 15° or 30° bit's in
//! an inch program by a corner too sharp for its written places, the
//! limit the maintainer accepted on #438 (`property::inch_sharp_corner`).
//! No regression file is written:
//! a failing seed becomes a named test, with its minimal design, once its
//! bug is understood.
//!
//! What the sweep found is on #464. While #484 is open, the sweep still
//! fails: seat failures with 60° and 90° bits away from a near touch
//! (#484). Their reproductions are the ignored tests at the end of this
//! file and in dc-cam's inlay tests.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod designs;
mod lettering;
mod property;
mod rng;

use designs::Design;
use property::{Kind, Outcome, Seats, carve, case};

/// Designs per family the CI tests carve through the whole property.
const CI_CASES: u64 = 3;
/// Designs per family `generator_never_refuses_its_own_move` generates.
const OWN_MOVE_CASES: u64 = 2;
/// Designs per family the sweep carves by default.
const SWEEP_CASES: u64 = 200;

/// The first seed of each CI test's designs, apart so each test draws its
/// own.
const LINE_ART_SEED: u64 = 0x0464_0000;
const LETTERING_SEED: u64 = 0x0464_1000;
const OWN_MOVE_SEED: u64 = 0x0464_2000;

/// A family of designs: its name and how a seed draws one.
type Family = (&'static str, fn(u64) -> Design);

const LINE_ART: Family = ("line art", designs::line_art);
const LETTERING: Family = ("lettering", lettering::lettering);
const LINE_ART_SMALL: Family = ("small line art", designs::line_art_small);
const LETTERING_SMALL: Family = ("small lettering", lettering::lettering_small);

/// Carves `count` designs of each family from `first` ([`carve`]). A
/// small family's design is carved by three of the seven operations, in
/// turn by seed: the female, one male, one V-Carve; and with arc fitting
/// off (#480: it can take minutes on a V-bit's long level runs in a
/// debug build).
fn run(families: &[Family], first: u64, count: u64, seats: Seats, full: bool) -> Outcome {
    let mut all = Outcome::default();
    for &(family, draw) in families {
        for seed in first..first + count {
            let mut c = case(family, seed, draw(seed));
            if full && family.starts_with("small") {
                c.project.arc_fit_tolerance_mm = None;
                let male = MALES[usize::try_from(seed % 4).unwrap_or(0)];
                let carve = [Kind::VCarveTop, Kind::VCarveStart][usize::from(seed % 2 == 1)];
                c.ops
                    .retain(|(kind, _)| [Kind::Female, male, carve].contains(kind));
            }
            all.absorb(carve(&c, seats, full));
        }
    }
    all
}

/// The male operations, for [`run`]'s turns.
const MALES: [Kind; 4] = [
    Kind::MaleVWall,
    Kind::MaleOpenEdge,
    Kind::MaleRounded,
    Kind::MaleCutOut,
];

/// Fails with every failure of `outcome`, listed; prints the explained
/// errors and the known issues' seat failures.
fn assert_none_failed(outcome: &Outcome) {
    println!(
        "{} operations carved, {} explained, {} pairs seated, {} known, {} failures",
        outcome.carved,
        outcome.explained.len(),
        outcome.seated,
        outcome.known.len(),
        outcome.failures.len()
    );
    for line in &outcome.explained {
        println!("  explained: {line}");
    }
    for line in &outcome.known {
        println!("  known {line}");
    }
    assert!(
        outcome.failures.is_empty(),
        "{} failures:\n{}",
        outcome.failures.len(),
        outcome.failures.join("\n")
    );
}

/// The generators draw valid designs: every outline a closed contour
/// that crosses neither itself nor another (`designs::valid`, what
/// `check` accepts), over 64 seeds of each family and size.
#[test]
fn designs_are_valid() {
    for seed in 0..64 {
        for (family, draw) in [LINE_ART, LETTERING, LINE_ART_SMALL, LETTERING_SMALL] {
            let design = draw(seed);
            assert!(
                designs::valid(&design.profiles),
                "{family} seed {seed}: {:?}",
                design.notes
            );
        }
    }
}

/// The property over [`CI_CASES`] small line-art designs: female and the
/// four males (V wall, open edge, rounded, cut-out), V-Carve from the
/// stock top and from a cleared start depth, each carving safely through
/// S1–S19 or refused with an allowed error; one male case per design
/// (#498: one Inlay, both boards) seats within 0.05 mm, or fails as an
/// open issue does.
#[test]
fn line_art_designs_carve_or_explain() {
    assert_none_failed(&run(
        &[LINE_ART_SMALL],
        LINE_ART_SEED,
        CI_CASES,
        Seats::One,
        true,
    ));
}

/// The property over [`CI_CASES`] small lettering designs, overlapping
/// glyphs merged by union.
#[test]
fn lettering_designs_carve_or_explain() {
    assert_none_failed(&run(
        &[LETTERING_SMALL],
        LETTERING_SEED,
        CI_CASES,
        Seats::One,
        true,
    ));
}

/// The generator never refuses its own move ("would cut … past a wall",
/// #463) on [`OWN_MOVE_CASES`] small designs of each family, carved by
/// every operation: no
/// `ConeOffMaterial`, `ConeArcMove` or `OffMaterial` from any operation.
#[test]
fn generator_never_refuses_its_own_move() {
    assert_none_failed(&run(
        &[LINE_ART_SMALL, LETTERING_SMALL],
        OWN_MOVE_SEED,
        OWN_MOVE_CASES,
        Seats::None,
        false,
    ));
}

/// The large sweep: `STRESS_CASES` full-size designs of each family from
/// `STRESS_SEED`, every male case's two boards seated (#498), on every
/// core. Prints
/// each design's failures as it finishes, then the cases run, the known
/// issues' seat failures and the failures found.
#[test]
#[ignore = "the large sweep (#464): run with --release -- --ignored"]
fn sweep() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let count = number("STRESS_CASES", SWEEP_CASES);
    let first = number("STRESS_SEED", 0);
    let jobs: Vec<(Family, u64)> = [LINE_ART, LETTERING]
        .into_iter()
        .flat_map(|family| (first..first + count).map(move |seed| (family, seed)))
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let all = std::sync::Mutex::new(Outcome::default());
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&((family, draw), seed)) = jobs.get(k) else {
                        break;
                    };
                    let started = std::time::Instant::now();
                    let outcome = carve(&case(family, seed, draw(seed)), Seats::All, true);
                    println!(
                        "{family} seed {seed}: {} failures, {} known, in {:.1?}",
                        outcome.failures.len(),
                        outcome.known.len(),
                        started.elapsed()
                    );
                    for f in &outcome.failures {
                        println!("FAIL {f}");
                    }
                    all.lock()
                        .expect("no thread panics holding it")
                        .absorb(outcome);
                }
            });
        }
    });
    println!("sweep: {count} designs of each family from seed {first}");
    assert_none_failed(&all.into_inner().expect("the threads are done"));
}

/// Carves `design` of `family` and `seed` with the operation `kind` of
/// its case alone, every male seated: a sweep failure's reproduction.
fn reproduce(family: &str, seed: u64, design: Design, kind: Kind) -> Outcome {
    let mut c = case(family, seed, design);
    c.ops.retain(|(k, _)| *k == kind || *k == Kind::Female);
    carve(&c, Seats::All, true)
}

/// #482 (found by the sweep): the male of line-art seed `0x34`'s first
/// motif (a 13 mm square of line art with two rings, butt and round line
/// ends; 30° bit, inch program) with the end mill's clearing pass, its
/// band's edge open, generates. The V-bit's inset closes the mouth of a
/// bay of the band, which becomes a piece in the island of the band's
/// floor, so the
/// clearing pass's floor is two pieces of the band's one shape, one in
/// the other's island; the pass paired the shape with itself and failed
/// "shape `Design` (instance `i1`, profile 1) encloses itself through
/// other shapes; no cut order exists". The pair also seats (#500): the
/// female's groove into the motif's 125° corner stopped 0.025 mm in, at
/// a stub of the corner's branch, and left the plug 0.092 mm proud; since
/// #484 the branch runs on to the axis.
#[test]
fn male_of_line_art_with_rings_orders_its_cuts() {
    let outcome = reproduce(
        "line art",
        0x34,
        designs::line_art_keeping(0x34, &|k| k == 0),
        Kind::MaleOpenEdge,
    );
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
}

/// #483 (found by the sweep): the V-Carve from a cleared start depth of
/// line-art seed `0x54` (a near touch 0.0012 mm wide and a framed mesh;
/// 15° bit, millimetre program, arc fitting on) exports. The safety pass
/// refused it: "the program does not reproduce operation `o2` move
/// 8696". A fitted arc ends where three vertices and a 0.0007 mm spike
/// between them round to one point, so the emitter writes nothing for
/// them; S6 matched the arc's end to the first, the next line to the
/// second, and found no line for the spike. Further on, S12 refused a
/// chain fitted to a tip on a circle 0.003 mm across, whose last piece is
/// shorter than a unit in the last place. About four minutes in a
/// release build, far too long for the debug test run, so it runs with
/// the sweep.
#[test]
#[ignore = "slow: the sweep's full-size design; run with --release -- --ignored"]
fn v_carve_of_line_art_reproduces_when_written() {
    let outcome = reproduce("line art", 0x54, designs::line_art(0x54), Kind::VCarveStart);
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
}

/// #484 (found by the stress tests): the pair of small lettering seed
/// `0x4641000` ("ere", 90° bit) with the male's V wall seats within
/// 0.05 mm. When #484 was filed the plug stood 0.36 mm proud beside the
/// last "e"'s lower terminal, where the male's surround narrows into the
/// letter's mouth; the fixes merged since (#478 to #505) seat it.
#[test]
fn lettering_seats_beside_a_narrow_mouth() {
    let seed = 0x0464_1000;
    let outcome = reproduce(
        "small lettering",
        seed,
        lettering::lettering_small(seed),
        Kind::MaleVWall,
    );
    assert!(
        outcome.failures.is_empty() && outcome.known.is_empty(),
        "{:?}",
        outcome.failures
    );
}

/// #481 (found by the sweep): the V-Carve from the stock top of line-art
/// seed `0x40`'s first motif, a crescent 5.03 mm across (arcs of bulge
/// 0.790 and 0.643, so its horns are acute tips drawn with arcs), with the
/// 15° bit (millimetre program, flat depth 4.9 mm), exports. The safety
/// pass refuses it: "emitted line 55 takes operation `o2`'s V-bit 0.002 mm
/// past the material it may cut", at Z 11.983, just below the stock top.
/// The female of the same design fails the same way. The chain into a
/// horn kept `quantum / √2` from the chords the inner arc is flattened to,
/// which lie beyond that arc, so it ran 0.0001 mm from the arc as drawn,
/// and written to three places it crossed it.
#[test]
fn v_carve_beside_a_hair_thin_pinch_passes_s17() {
    let outcome = reproduce(
        "line art",
        0x40,
        designs::line_art_keeping(0x40, &|k| k == 0),
        Kind::VCarveTop,
    );
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
}

/// #490 (found by the sweep): the male of lettering seed `0x44` ("Dm",
/// "me", 60° bit, inch program) with `outline_rounding_mm` 5.53 and the
/// end mill's clearing pass generates, and every program passes S1–S19.
/// The pass's S9 check refused it, "would cut 1332.519 mm² outside the
/// material": one ring of the band's floor comes within 8.5 µm of itself,
/// and its swept band, the ring grown by the end mill's radius, came back
/// from `dc_geometry::offset` without the loop round its inside, so the
/// footprint covered the plugs the ring runs round. The toolpath itself
/// stayed in the band.
#[test]
fn male_lettering_with_rounding_clears_inside_the_band() {
    let outcome = reproduce(
        "lettering",
        0x44,
        lettering::lettering(0x44),
        Kind::MaleRounded,
    );
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
}

/// #502 (found by the sweep): the males of line-art seed `0xc5` (a
/// 6.54 mm crescent and four circles; 30° bit, millimetre program, arc
/// fitting on) with the end mill's clearing pass, rounding or a cut-out
/// export. The safety pass refused them: "the program does not reproduce
/// operation `o1` move 344". The V-bit joins a ring of the band three chords
/// before the point it laps from, so 55 moves go round one circle 21°
/// past a full turn, and the arc fitter took them as one run. Its sub-arcs
/// take their sweep from the run's ends, modulo a turn, so the program cut
/// the last 21° and skipped the lap before them.
#[test]
fn males_of_line_art_keep_each_lap_when_fitted() {
    for kind in [Kind::MaleOpenEdge, Kind::MaleRounded, Kind::MaleCutOut] {
        let outcome = reproduce("line art", 0xc5, designs::line_art(0xc5), kind);
        assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
    }
}

/// #508 (found by the sweep): the male of line-art seed `0xb3` (a mesh,
/// two spiky motifs and a near touch; 60° bit, millimetre program) with
/// the end mill's clearing pass, its band's edge open, generates, every
/// program passes S1–S19 and the pair seats. The generator refused its
/// own move, "would cut 0.0954 mm past a wall": a stay-down link 7 mm long
/// between two medial chains on the band's floor, held to the cone at the
/// start plane against the region's walls but not to the open edge, where
/// the board stands to the stock top and the cone is wider. The pair's
/// seat is #484's (`KNOWN_CASES`): the female's groove leaves the plug
/// 0.274 mm proud 23 mm from the near touch, as with every male of the
/// design.
#[test]
fn male_of_line_art_0xb3_generates() {
    let outcome = reproduce(
        "line art",
        0xb3,
        designs::line_art(0xb3),
        Kind::MaleOpenEdge,
    );
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
}

/// #513 (found by the sweep): the male cut-out of lettering seed `0xc0`
/// ("CrC", 90° bit, millimetre program, V-walled band) generates, every
/// program passes S1–S19 and the pair seats within 0.05 mm. Its cut-out
/// path, the band's edge shrunk by 2.382 mm, crossed itself: where one of
/// the edge's chords beside a "C" is 0.052 mm long, cavalier_contours
/// left a spike 0.0007 mm long in the inset, and the Profile refused it,
/// "the outline crosses itself". `dc_geometry::offset` drops such spikes.
#[test]
fn male_cut_out_of_lettering_0xc0_generates() {
    let outcome = reproduce(
        "lettering",
        0xc0,
        lettering::lettering(0xc0),
        Kind::MaleCutOut,
    );
    assert!(
        outcome.failures.is_empty() && outcome.known.is_empty(),
        "{:?} {:?}",
        outcome.failures,
        outcome.known
    );
}

/// #511 (found by the sweep): the male of lettering seed `0x47` ("mm",
/// 60° bit, millimetre program) with a V wall seats within 0.05 mm. Two
/// serif feet leave a channel 0.44 mm wide in the band; its exit, between
/// the two feet's corners, got no circumcentre, so the axis down the
/// channel ended there, and spur pruning, whose test it passes (its
/// clearance does not fall), unzipped it back to the counter above. The
/// male cut nothing in the channel, and the plug stood 0.140 mm proud.
#[test]
fn lettering_male_cuts_the_channel_between_two_feet() {
    let outcome = reproduce(
        "lettering",
        0x47,
        lettering::lettering(0x47),
        Kind::MaleVWall,
    );
    assert!(
        outcome.failures.is_empty() && outcome.known.is_empty(),
        "{:?} {:?}",
        outcome.failures,
        outcome.known
    );
}

/// #515 (found by the sweep): the male of lettering seed `0xaf` (60° bit)
/// with a V wall seats within 0.05 mm. Its "m" and "e" overlap, so they
/// are one outline, which comes 0.02 mm from itself across the outside
/// between the "m"'s leg and the "e"'s curve, leaving two acute curved
/// bays either side; #479 merges two parts that near, not one outline,
/// and the male's groove stopped short in the bays: the plug stood
/// 1.263 mm proud, 0.430 after #511.
#[test]
fn lettering_outline_near_itself_is_merged() {
    let outcome = reproduce(
        "lettering",
        0xaf,
        lettering::lettering(0xaf),
        Kind::MaleVWall,
    );
    assert!(
        outcome.failures.is_empty() && outcome.known.is_empty(),
        "{:?} {:?}",
        outcome.failures,
        outcome.known
    );
}

/// #438 (the maintainer's decision on #499's limit): a narrow bit's seat
/// failure in an inch program is excused only near a corner sharper than
/// the written margin lets it seat ([`property::inch_sharp_corner`]). A
/// 10° wedge 20 mm long, under the 15° bit's 24.8°: a binding cell
/// 0.05 mm from its tip is excused; one 5 mm in along it is not, nor is
/// the cell by the tip in a millimetre program or with the 60° bit.
#[test]
fn inch_limit_excuses_only_a_narrow_bit_by_a_sharp_corner() {
    let half = 5.0_f64.to_radians().tan() * 20.0;
    let wedge = dc_geometry::Polyline::new(
        vec![
            dc_geometry::Vertex::line(0.0, 0.0),
            dc_geometry::Vertex::line(20.0, -half),
            dc_geometry::Vertex::line(20.0, half),
        ],
        true,
    );
    let design = Design {
        profiles: vec![wedge],
        notes: Vec::new(),
    };
    let with = |units: dc_project::Units, angle_deg: f64| {
        let mut c = case("line art", 0, design.clone());
        c.project.display_units = units;
        c.project.tools[0].cutter = dc_project::ToolKind::VBit {
            included_angle_deg: angle_deg,
        };
        c
    };
    let female = |c: &property::Case| match c.ops.iter().find(|(k, _)| *k == Kind::Female) {
        Some((_, dc_project::Operation::Inlay(op))) => op.clone(),
        _ => panic!("the case has a female"),
    };
    let inch = with(dc_project::Units::Inch, 15.0);
    let place = |p: dc_geometry::Point| inch.project.instances[0].transform.apply(p);
    let (by_tip, far) = (
        place(dc_geometry::Point::new(0.05, 0.0)),
        place(dc_geometry::Point::new(5.0, 0.0)),
    );
    let excused = |c: &property::Case, at| property::inch_sharp_corner(c, &female(c), at);
    assert!(excused(&inch, by_tip).is_some(), "by the tip");
    assert!(excused(&inch, far).is_none(), "5 mm in");
    let mm = with(dc_project::Units::Mm, 15.0);
    assert!(excused(&mm, by_tip).is_none(), "in millimetres");
    let wide = with(dc_project::Units::Inch, 60.0);
    assert!(excused(&wide, by_tip).is_none(), "with the 60° bit");
}

/// #506 (found by the sweep): the male of lettering seed `0x2d` (15° bit,
/// millimetre program) with a V wall seats within 0.05 mm. A letter's
/// curve leaves the male's surround a bay whose last medial node, 0.239 mm
/// from the walls, leaves part of the bay's wall 0.009 mm beyond its disc.
/// The medial axis gives a convex vertex no disc reaches a tip of its own,
/// but only beyond a fixed slack of 0.01 mm, which the 15° bit's cone
/// turns into 0.076 mm: the plug stood 0.069 mm proud on the bay's wood.
/// The slack now scales with the bit, as the wall tolerance does (#478).
#[test]
fn lettering_curve_bay_seats_with_a_narrow_bit() {
    let outcome = reproduce(
        "lettering",
        0x2d,
        lettering::lettering(0x2d),
        Kind::MaleVWall,
    );
    assert!(
        outcome.failures.is_empty() && outcome.known.is_empty(),
        "{:?} {:?}",
        outcome.failures,
        outcome.known
    );
}

/// #506 (found by the sweep): the male of line-art seed `0x55` (15° bit;
/// the sweep's inch program, and the same in millimetres) with a V wall
/// seats within 0.05 mm. Simplification and the off-axis fit let the
/// carved edge stop up to `EDGE_MM`, 0.005 mm, short of the walls (#457),
/// which the 15° bit's cone turns into 0.038 mm; the female stood that
/// shallow over a stretch of wall and, with the male's 0.027, the plug
/// 0.062 mm proud. The allowance now scales with the bit, as the wall
/// tolerance does (#478).
#[test]
fn line_art_seats_with_the_edge_allowance_scaled() {
    let outcome = reproduce("line art", 0x55, designs::line_art(0x55), Kind::MaleVWall);
    assert!(
        outcome.failures.is_empty() && outcome.known.is_empty(),
        "{:?} {:?}",
        outcome.failures,
        outcome.known
    );
}

/// #555 (found by the confirmation sweep, #438): the male of lettering
/// seed `0xc4` (90° bit, millimetre program) with a V wall generates,
/// every program passes S1–S19 and the pair seats within 0.05 mm. The
/// generator refused its own move, "would cut 0.0161 mm past a wall": a
/// letter's hole, grown by the floor's inset to find the band's floor,
/// came back from `dc_geometry::offset` with its bay's loop closed by a
/// straight line across the round join at a sharp point of the letter,
/// 0.016 mm nearer it than the inset, and the floor's ring ran along it.
#[test]
fn male_of_lettering_0xc4_generates() {
    let outcome = reproduce(
        "lettering",
        0xc4,
        lettering::lettering(0xc4),
        Kind::MaleVWall,
    );
    assert!(
        outcome.failures.is_empty() && outcome.known.is_empty(),
        "{:?} {:?}",
        outcome.failures,
        outcome.known
    );
}

/// #553 (found by the confirmation sweep on #438): the males of
/// lettering seeds `0x8f` and `0x81` (30° bit, millimetre programs) with a
/// V wall seat within 0.05 mm. The band round the letters is sampled every
/// 0.33 and 0.36 mm, and where two letters, or two lobes of one, pass
/// 0.05 to 0.11 mm apart the band has a neck narrower than the step, with
/// no medial node in it (`0x8f`) or only a stub of the branch (`0x81`).
/// The male cut nothing across it, and the plug stood 3.586 and 0.890 mm
/// proud on the wood left there, `0x8f`'s its whole height `D − G`. The
/// band is now sampled again where it has a neck narrower than its step.
/// About 160 s in a debug build, so it runs with the sweep; dc-cam's
/// `male_of_a_curve_beside_an_edge_cuts_the_neck` is its minimal design.
#[test]
#[ignore = "slow: the sweep's full-size designs; run with --release -- --ignored"]
fn lettering_male_cuts_a_neck_narrower_than_its_step() {
    for seed in [0x8f, 0x81] {
        let outcome = reproduce(
            "lettering",
            seed,
            lettering::lettering(seed),
            Kind::MaleVWall,
        );
        assert!(
            outcome.failures.is_empty() && outcome.known.is_empty(),
            "{seed:#x}: {:?} {:?}",
            outcome.failures,
            outcome.known
        );
    }
}

/// #522 (found by the sweep): line-art seed `0xa` and lettering seed
/// `0x3c` (15° bit, millimetres) seat within 0.05 mm. Their glue gaps,
/// drawn at 0.12 of the depth, stood above the floor's straight-run
/// ridge, `0.1 · D`, but not above the peaks where the V-bit's floor rings
/// turn a corner: line art `0xa`'s plug rested on one 0.161 mm proud. The
/// export now reports the corner ridge too, and the sweep draws the glue
/// gap above it, as a user following the Inlay chapter would set it.
#[test]
fn glue_gap_stands_above_the_floor_corner_ridge() {
    for (family, seed, design) in [
        ("line art", 0xa, designs::line_art(0xa)),
        ("lettering", 0x3c, lettering::lettering(0x3c)),
    ] {
        let outcome = reproduce(family, seed, design, Kind::MaleVWall);
        assert!(
            outcome.failures.is_empty() && outcome.known.is_empty(),
            "{family} {seed:#x}: {:?} {:?}",
            outcome.failures,
            outcome.known
        );
    }
}

/// #554 (found by the confirmation sweep on #438): the males of lettering
/// seed `0x97` (15° bit, inch program) and line-art seed `0x63` (15° bit,
/// millimetre program) with a V wall seat within 0.05 mm. Each has a
/// corner whose medial tip is joined to a node beside one of its walls:
/// in `0x97` the slit where an "e"'s tail meets its bowl, in `0x63` a
/// pointed line end. The stretch to the tip ran along that wall, and
/// fitting split it on the wall where the wall bends into the region, at
/// the start plane; it reads a point's axis from its nearest wall point,
/// which for a point on the wall is under it, so the rest of the way lay on
/// the wall, and the written margin (#469) cut the groove back to where it
/// left it, 0.022 and 0.051 mm from the tips: the plugs stood 0.051 and
/// 0.340 mm proud past them. A point on a wall now seeks the axis along
/// the wall's normal.
/// Over ten minutes in a debug build, so it runs with the sweep; dc-cam's
/// `chain_along_a_wall_into_a_corner_is_fitted_to_its_axis` is its minimal
/// design.
#[test]
#[ignore = "slow: the sweep's full-size designs; run with --release -- --ignored"]
fn males_reach_a_corner_their_axis_meets_along_a_wall() {
    for (family, seed, design) in [
        ("lettering", 0x97, lettering::lettering(0x97)),
        ("line art", 0x63, designs::line_art(0x63)),
    ] {
        let outcome = reproduce(family, seed, design, Kind::MaleVWall);
        assert!(
            outcome.failures.is_empty() && outcome.known.is_empty(),
            "{family} {seed:#x}: {:?} {:?}",
            outcome.failures,
            outcome.known
        );
    }
}

/// #554 (found by the confirmation sweep on #438): the males of line-art
/// seed `0xa7` (15° bit, millimetre program) with a V wall and with an
/// open edge seat within 0.05 mm. Two groups of its motifs have bands of
/// their own whose outer edges pass 0.07 mm apart. The board between
/// them, a ridge narrower than the seat check's 0.1 mm cells, fell apart
/// in the check into pieces, and one, joined to the board through no
/// cell, was counted as plug: 0.167 mm proud, 0.033 mm on a 0.05 mm grid.
/// The check now counts every cell beyond the band's outer edges as
/// board. About 160 s in a debug build, so it runs with the sweep; dc-cam's
/// `sliver_of_board_between_two_bands_is_not_plug` is its minimal case.
#[test]
#[ignore = "slow: the sweep's full-size design; run with --release -- --ignored"]
fn board_between_two_bands_is_not_plug() {
    for kind in [Kind::MaleVWall, Kind::MaleOpenEdge] {
        let outcome = reproduce("line art", 0xa7, designs::line_art(0xa7), kind);
        assert!(
            outcome.failures.is_empty() && outcome.known.is_empty(),
            "{kind:?}: {:?} {:?}",
            outcome.failures,
            outcome.known
        );
    }
}

/// #565 (found by the confirmation sweep on #438, behind #554): the males
/// of line-art seed `0x63` (15° bit, millimetre program) with the end
/// mill's clearing pass, an open edge or rounding, seat within 0.05 mm,
/// and every program passes S1–S19. Two causes held them up:
///
/// - **The grid start refused every candidate** at a 3.8° pointed line
///   end, 0.177 mm proud. The margin's point lay on a fitted stretch whose
///   cone reached 0.000025 mm past a wall, within fitting's slack, and the
///   grid start held each move to it to `−EPS_MM`. It now holds them to
///   fitting's slack (dc-cam's
///   `chain_end_past_a_wall_within_the_fit_slack_starts_on_the_grid`).
/// - **A floor piece a hair wide went uncut** in a channel between two
///   strokes, 0.101 mm proud. Fitting dropped its chains, but with an open
///   edge the floor cut only pieces thicker than twice the project's
///   tolerance. It now cuts every piece fitting dropped the chains for
///   (dc-cam's `open_edge_male_cuts_a_floor_piece_a_hair_wide`).
///
/// The full design through S1–S19 takes most of an hour in a release
/// build, so it runs with the sweep.
#[test]
#[ignore = "slow: the sweep's full-size design; run with --release -- --ignored"]
fn males_of_line_art_0x63_with_a_clearing_pass_seat() {
    for kind in [Kind::MaleOpenEdge, Kind::MaleRounded] {
        let outcome = reproduce("line art", 0x63, designs::line_art(0x63), kind);
        assert!(
            outcome.failures.is_empty() && outcome.known.is_empty(),
            "{kind:?}: {:?} {:?}",
            outcome.failures,
            outcome.known
        );
    }
}
