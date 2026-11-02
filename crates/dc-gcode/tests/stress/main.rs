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
//! (`property::known_issue`), not failed. No regression file is written:
//! a failing seed becomes a named test, with its minimal design, once its
//! bug is understood.
//!
//! What the sweep found is on #464. While #481 to #484 are open, the
//! sweep still fails: S17 at export beside a pinch (#481), a containment
//! cycle (#482), a written program that does not reproduce its toolpath
//! (#483), and seat failures with 60° and 90° bits away from a near touch
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
/// S1–S19 or refused with an allowed error; one female–male pair per
/// design seats within 0.05 mm, or fails as an open issue does.
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
/// `STRESS_SEED`, every male seated on the female, on every core. Prints
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

/// #481 (found by the sweep): a V-Carve of a rotated 4.18 × 1.47 mm
/// rectangle with a 0.42 mm circular hole 0.001 mm from its edge, with
/// the 15° bit from the stock top (flat depth 3.611 mm, floor step-over
/// 0.293), generates and passes `assert_run_safe`, and its program
/// passes the safety pass. It fails S17 there: the written cone 0.007
/// mm past the material, at Z 11.948, beside the pinch between the hole
/// and the edge, narrower than the written precision allows a tip path
/// (#469).
#[test]
#[ignore = "#481: S17 at export beside a pinch finer than the written precision"]
fn v_carve_beside_a_hair_thin_pinch_passes_s17() {
    use dc_geometry::{Point, Polyline, Vertex};
    let rect = designs::closed(&[
        Point::new(3.0466759807674237, 3.9289314157286848),
        Point::new(0.0, 1.0720017271145235),
        Point::new(1.0052377016041247, 0.0),
        Point::new(4.0519136823715485, 2.8569296886141613),
    ]);
    let hole = Polyline::new(
        vec![
            Vertex::new(0.4744443774441627, 0.5675810359250852, 1.0),
            Vertex::new(0.779202155064878, 0.8533585679688382, 1.0),
        ],
        true,
    );
    let mut c = case(
        "line art",
        0,
        Design {
            profiles: vec![rect, hole],
            notes: Vec::new(),
        },
    );
    c.project.instances[0].transform =
        dc_geometry::Transform::translate(15.30403519021997, 15.30403519021997);
    c.project.stock.width_mm = 34.65998406281149;
    c.project.stock.height_mm = 34.537001796168624;
    c.project.display_units = dc_project::Units::Mm;
    c.project.arc_fit_tolerance_mm = None;
    c.project.tools[0].cutter = dc_project::ToolKind::VBit {
        included_angle_deg: 15.0,
    };
    c.angle_deg = 15.0;
    c.label = "#481's design: 15° V-bit, Mm".into();
    let carve_op = dc_project::VCarveOp {
        id: dc_project::OperationId::new("o2"),
        name: "V-Carve".into(),
        enabled: true,
        targets: vec![dc_project::InstanceId::new("i1")],
        tool: dc_project::ToolId::new("t1"),
        strategy: dc_project::VCarveStrategy::Medial,
        clearing: Vec::new(),
        flat_depth_mm: Some(3.6106976154393067),
        start_depth_mm: 0.0,
        start_plane_cleared_externally: false,
        step_over_fraction: Some(0.2933158362312681),
        machining_order: dc_project::MachiningOrder::FullDepthFirst,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 4.839984608265256,
    };
    c.ops = vec![(Kind::VCarveTop, dc_project::Operation::VCarve(carve_op))];
    assert_none_failed(&carve(&c, Seats::None, true));
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
/// band's edge open, generates. It fails "shape `Design` (instance `i1`,
/// profile 1) encloses itself through other shapes; no cut order
/// exists", an internal-consistency error.
#[test]
#[ignore = "#482: a containment cycle on a male's line art"]
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
/// 15° bit, millimetre program) exports. The safety pass refuses it:
/// "the program does not reproduce operation `o2` move 8697".
#[test]
#[ignore = "#483: a written program that does not reproduce its toolpath"]
fn v_carve_of_line_art_reproduces_when_written() {
    let outcome = reproduce("line art", 0x54, designs::line_art(0x54), Kind::VCarveStart);
    assert!(outcome.failures.is_empty(), "{:?}", outcome.failures);
}

/// #484 (found by the stress tests): the pair of small lettering seed
/// `0x4641000` ("ere", 90° bit) with the male's V wall seats within
/// 0.05 mm. The plug stands 0.36 mm proud beside the last "e"'s lower
/// terminal, where the male's surround narrows into the letter's mouth.
#[test]
#[ignore = "#484: the plug stands proud at 60° and 90° away from a near touch"]
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
