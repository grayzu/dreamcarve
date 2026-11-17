// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The #464 sweep for an Inlay cut with a tapered ball nose (#681, slice
//! 27e): the line-art and lettering designs, each carved with both bench
//! bits (Ultra-High Detail, tip 0.762 mm, and Fine Detail, tip 2.286 mm;
//! both 6.7° per side, 19.05 mm of flute), as the female and the four
//! males of [`property::case`], every program through S1–S19 and the
//! pairs seated within the seat check's 0.05 mm. The Inlay rounds each
//! design for the bit; a design it cannot round (a part or a gap too
//! narrow, `InlayTooNarrowForBit`) is a pass of that refusal, counted on
//! its own, not a seat.
//!
//! The depths are drawn for the bit, not the V-bit's cone: `D` 1.5 to
//! 5 mm, `G` from the bit's tangent height `h_t` to 0.8 mm above it, `P`
//! from `h_t` to 3 mm; the rest of the case (margins, roundings, clearing
//! passes, cut-outs, units) is the V-bit case's of the same seed.
//!
//! CI carves [`super::CI_CASES`] small designs of each family with each
//! bit; the large sweep is ignored, run as the V-bit's:
//!
//! ```sh
//! cargo test --release -p dc-gcode --test stress -- --ignored tapered_sweep --nocapture
//! ```

use super::designs::{self, Design};
use super::lettering;
use super::property::{Case, Kind, Outcome, Seats, carve, case};
use super::rng::Rng;
use super::{CI_CASES, SWEEP_CASES, assert_none_failed};
use dc_project::test_support::{tool_tbn_fine, tool_tbn_ultra};
use dc_project::{Operation, Tool, ToolId};

/// The first seed of the CI test's designs.
const TAPERED_SEED: u64 = 0x0681_0000;

/// A family of designs: its name (the V-bit case's, which sizes the band)
/// and how a seed draws one.
type Family = (&'static str, fn(u64) -> Design);

const LINE_ART: Family = ("line art", designs::line_art);
const LETTERING: Family = ("lettering", lettering::lettering);
const LINE_ART_SMALL: Family = ("small line art", designs::line_art_small);
const LETTERING_SMALL: Family = ("small lettering", lettering::lettering_small);

/// A bench tapered ball nose: its name and the tool.
#[derive(Debug, Clone, Copy)]
enum Bit {
    Ultra,
    Fine,
}

impl Bit {
    const BOTH: [Self; 2] = [Self::Ultra, Self::Fine];

    fn tool(self) -> Tool {
        let tool = match self {
            Self::Ultra => tool_tbn_ultra(),
            Self::Fine => tool_tbn_fine(),
        };
        Tool {
            id: ToolId::new("t1"),
            step_down_mm: None,
            step_over_fraction: Some(0.1),
            ..tool
        }
    }

    /// How much the bit's designs are drawn larger than the V-bit's, so
    /// most keep their parts and gaps wider than the bit rounds away
    /// (`2 R_min`, `2 w_t`) and are carved rather than refused; their
    /// points and notches the Inlay still rounds.
    fn scale(self) -> f64 {
        std::env::var("TAPERED_SCALE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(match self {
                Self::Ultra => 3.0,
                Self::Fine => 6.0,
            })
    }

    /// The tangent height `h_t = ρ (1 − sin β)`, mm, by hand.
    fn tangent_height_mm(self) -> f64 {
        let rho = match self {
            Self::Ultra => 0.381,
            Self::Fine => 1.143,
        };
        rho * (1.0 - 6.7_f64.to_radians().sin())
    }
}

/// The V-bit case of `family` and `seed` cut with `bit` (module docs): its
/// Inlay operations only, their depths drawn for the bit.
fn tapered_case(family: &str, seed: u64, design: Design, bit: Bit) -> Case {
    let k = bit.scale();
    let grown = Design {
        profiles: design
            .profiles
            .iter()
            .map(|p| {
                dc_geometry::Transform::scale(k, k)
                    .apply_polyline(p)
                    .expect("a uniform scale places every outline")
            })
            .collect(),
        notes: design.notes,
    };
    let mut c = case(family, seed, grown);
    let mut rng = Rng::new(seed ^ 0x7a9e_0681);
    let h_t = bit.tangent_height_mm();
    let depth_mm = rng.range(1.5, 5.0);
    let glue_gap_mm = (h_t + rng.range(0.0, 0.8)).min(depth_mm - 0.5);
    let plug_depth_mm = rng.range(h_t, 3.0);
    c.project.tools[0] = bit.tool();
    c.ops.retain(|(kind, _)| {
        matches!(
            kind,
            Kind::Female
                | Kind::MaleVWall
                | Kind::MaleOpenEdge
                | Kind::MaleRounded
                | Kind::MaleCutOut
        )
    });
    for (_, op) in &mut c.ops {
        if let Operation::Inlay(inlay) = op {
            inlay.depth_mm = Some(depth_mm);
            inlay.glue_gap_mm = Some(glue_gap_mm);
            inlay.plug_depth_mm = Some(plug_depth_mm);
        }
    }
    c.label = format!(
        "{} with the {bit:?} tapered ball nose, drawn {k}× larger: D {depth_mm:.3} G {glue_gap_mm:.3} P {plug_depth_mm:.3}",
        c.label
    );
    // The bit in the family's name, so a known issue names it
    // (`property::KNOWN_CASES`).
    c.family = format!("tapered {family} ({bit:?})");
    c
}

/// How many of `outcome`'s explained refusals are the design refused as
/// too narrow for the bit.
fn refused_too_narrow(outcome: &Outcome) -> usize {
    outcome
        .explained
        .iter()
        .filter(|line| line.contains("its tapered ball nose needs"))
        .count()
}

/// Carves `count` designs of each family from `first`, each with both
/// bits. A small family's design is carved as the female and one male,
/// in turn by seed, as the V-bit's CI tests carve it.
fn run(families: &[Family], first: u64, count: u64, seats: Seats) -> Outcome {
    let mut all = Outcome::default();
    for &(family, draw) in families {
        for seed in first..first + count {
            for bit in Bit::BOTH {
                let mut c = tapered_case(family, seed, draw(seed), bit);
                if family.starts_with("small") {
                    let male = MALES[usize::try_from(seed % 4).unwrap_or(0)];
                    c.ops
                        .retain(|(kind, _)| [Kind::Female, male].contains(kind));
                }
                all.absorb(carve(&c, seats, true));
            }
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

/// Prints what `outcome` holds of the refusals, then fails with every
/// failure.
fn report(outcome: &Outcome) {
    println!(
        "{} refused as too narrow for the bit (a pass of the refusal, not a seat)",
        refused_too_narrow(outcome)
    );
    assert_none_failed(outcome);
}

/// The #464 property for an Inlay cut with a tapered ball nose, on
/// [`CI_CASES`] small line-art and lettering designs, each with both bench
/// bits: every program generated passes S1–S19, one pair per design and
/// bit seats within 0.05 mm, or the design is refused as too narrow for
/// the bit (`InlayTooNarrowForBit`), a pass of the refusal.
#[test]
fn tapered_inlays_seat() {
    let outcome = run(
        &[LINE_ART_SMALL, LETTERING_SMALL],
        TAPERED_SEED,
        CI_CASES,
        Seats::One,
    );
    assert!(
        outcome.carved + refused_too_narrow(&outcome) > 0,
        "{outcome:?}"
    );
    report(&outcome);
}

/// #704 (found by the tapered sweep, #681): lettering seed `0x5b` with
/// the Ultra bit, drawn 3× larger (30° V-bit case, inch program), as the
/// female and the male with a cut-out, generates, passes S1–S19 and
/// seats. The cut-out's path, the band's edge inset 1.5875 mm, came back
/// from the offset with segments nanometres long where the edge's curve
/// has about that radius; the outline test took one and its neighbour,
/// going on the same way 2° apart, to overlap, and the generator refused
/// the path as crossing itself. dc-geometry's
/// `a_tiny_segment_going_on_the_same_way_is_no_crossing` is its minimal
/// case. About a minute in a release build, so it runs with the sweep.
#[test]
#[ignore = "slow: the sweep's full-size design; run with --release -- --ignored"]
fn cut_out_of_lettering_0x5b_with_the_ultra_bit_is_cut() {
    let mut c = tapered_case("lettering", 0x5b, lettering::lettering(0x5b), Bit::Ultra);
    c.ops
        .retain(|(kind, _)| [Kind::Female, Kind::MaleCutOut].contains(kind));
    let outcome = carve(&c, Seats::All, true);
    assert_eq!(outcome.carved, 2, "{outcome:?}");
    assert!(
        outcome.failures.is_empty() && outcome.known.is_empty(),
        "{:?} {:?}",
        outcome.failures,
        outcome.known
    );
}

/// The large sweep for a tapered ball nose: `STRESS_CASES` (default
/// [`SWEEP_CASES`]) full-size designs of each family from `STRESS_SEED`,
/// each with both bench bits, every male's two boards seated, on every
/// core.
#[test]
#[ignore = "the tapered ball nose sweep (#681): run with --release -- --ignored"]
fn tapered_sweep() {
    let number = |name: &str, default: u64| {
        std::env::var(name)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    let count = number("STRESS_CASES", SWEEP_CASES);
    let first = number("STRESS_SEED", 0);
    let jobs: Vec<(Family, u64, Bit)> = [LINE_ART, LETTERING]
        .into_iter()
        .flat_map(|family| {
            (first..first + count)
                .flat_map(move |seed| Bit::BOTH.into_iter().map(move |bit| (family, seed, bit)))
        })
        .collect();
    let next = std::sync::atomic::AtomicUsize::new(0);
    let all = std::sync::Mutex::new(Outcome::default());
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..threads {
            scope.spawn(|| {
                loop {
                    let k = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(&((family, draw), seed, bit)) = jobs.get(k) else {
                        break;
                    };
                    let started = std::time::Instant::now();
                    let outcome = carve(&tapered_case(family, seed, draw(seed), bit), Seats::All, true);
                    println!(
                        "{family} seed {seed} {bit:?}: {} carved, {} seated, {} refused as too narrow, {} failures, in {:.1?}",
                        outcome.carved,
                        outcome.seated,
                        refused_too_narrow(&outcome),
                        outcome.failures.len(),
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
    println!("tapered sweep: {count} designs of each family from seed {first}, both bits");
    report(&all.into_inner().expect("the threads are done"));
}

/// #705 (found by the sweep): lettering `0x60` with the Fine bit, drawn
/// 6× larger (`D` 2.129, `G` 1.629, `P` 2.908, inch program), its female
/// and V-wall male, seats. The plug stood 0.107 mm proud 0.03 mm inside a
/// point the Inlay rounded to `R_min` (1.194 mm): the letter's medial axis
/// is sampled 0.93 mm apart, which merged the node at the round end's
/// centre into the spine's, and fitting found no point of the stretch
/// past it whose nearest wall was the arc, so the female's carve stopped
/// 0.0126 mm short of it (dc-cam's
/// `tapered_carve_reaches_a_rounded_point_past_its_centre`). The
/// full-size design through S1–S19 and the seat check takes about a
/// minute and a half in a release build, so it runs with the sweep.
#[test]
#[ignore = "slow: the sweep's full-size design; run with --release -- --ignored"]
fn lettering_0x60_seats_at_a_rounded_point() {
    let mut c = tapered_case("lettering", 0x60, lettering::lettering(0x60), Bit::Fine);
    c.ops
        .retain(|(kind, _)| [Kind::Female, Kind::MaleVWall].contains(kind));
    let outcome = carve(&c, Seats::All, true);
    assert!(
        outcome.seated == 1 && outcome.failures.is_empty() && outcome.known.is_empty(),
        "{outcome:?}"
    );
}
