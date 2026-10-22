// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Property tests for generation and emission (issue #72): a random valid
//! project with one Profile, Pocket, Drill or Engrave either fails to
//! generate with a typed error that names a problem of the drawing, or
//! generates a toolpath that passes `assert_safe` and emits through the
//! full safety pass (S1–S12); nothing panics. And sequencing is a
//! permutation (O2 in `docs/research/toolpath-ordering.md`): both
//! machining orders cut the same moves.
//!
//! They live here, not in dc-cam, because only this crate sees both the
//! generators and the safety pass.
//!
//! The seed is fixed so every run, in CI and locally, tries the same
//! cases; set `PROPTEST_RNG_SEED` to explore others. Failures proptest
//! finds are kept in `proptest-regressions/properties.txt` and replayed
//! first.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod cases;
mod projects;

use dc_cam::test_support::assert_safe;
use dc_cam::{
    CamError, MoveKind, Toolpath, generate_drill, generate_engrave, generate_pocket,
    generate_profile,
};
use dc_gcode::{EmitOp, emit};
use dc_project::{MachiningOrder, Operation, Problem, Project, validate};
use proptest::prelude::*;
use proptest::test_runner::{FileFailurePersistence, RngSeed};

/// The seed every run starts from unless `PROPTEST_RNG_SEED` is set.
const SEED: u64 = 0x0072_d0ca_4e00_0072;

const DATE: &str = "2026-01-01";

/// `cases` cases from the fixed seed, or from `PROPTEST_RNG_SEED` when set.
/// Failures are saved to, and replayed from, the crate's
/// `proptest-regressions/properties.txt` (`cargo test` runs integration
/// tests in the crate directory).
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

/// Generator errors a valid project may still get, because they depend on
/// how the shapes and the tool meet: the cutter would leave the stock, a
/// band between two lines is narrower than the tool, an On line part is
/// nowhere as wide as the tool (#223), or a Left or Right cut reaches what
/// a line keeps (#207), with its last pass allowance too (#311), or an
/// end segment has no room for it (#342). Any other error from a project
/// that validates is a defect.
fn rejects_drawing(error: &CamError) -> bool {
    matches!(
        error,
        CamError::OffStock { .. }
            | CamError::BandNarrowerThanTool { .. }
            | CamError::PartNarrowerThanTool { .. }
            | CamError::CutsIntoKeptSide { .. }
            | CamError::BandNarrowerThanAllowance { .. }
            | CamError::ShapeNarrowerThanAllowance { .. }
            | CamError::AllowanceCutsIntoKeptSide { .. }
            | CamError::EndSegmentHasNoRoom { .. }
    )
}

/// Generates the project's one operation, ready to emit.
fn generate(project: &Project) -> Result<EmitOp, CamError> {
    let op = project.operations[0].clone();
    let tool = project.tools[0].clone();
    Ok(match &op {
        Operation::Profile(p) => {
            let (toolpath, _) = generate_profile(project, p)?;
            EmitOp::from((op, tool, toolpath))
        }
        Operation::Pocket(p) => {
            let (toolpath, _) = generate_pocket(project, p)?;
            EmitOp::from((op, tool, toolpath))
        }
        Operation::Drill(d) => {
            let (drilled, _) = generate_drill(project, d)?;
            EmitOp::drill(op, tool, drilled)
        }
        Operation::Engrave(e) => {
            let (toolpath, _) = generate_engrave(project, e)?;
            EmitOp::from((op, tool, toolpath))
        }
    })
}

/// The property every generator shares: `project` validates; generation
/// either refuses the drawing ([`rejects_drawing`]) or gives a toolpath
/// that `assert_safe` accepts and `emit` writes through the safety pass.
fn exports_safely(project: &Project) -> Result<(), TestCaseError> {
    let errors: Vec<Problem> = validate(project)
        .into_iter()
        .filter(Problem::is_error)
        .collect();
    prop_assert!(errors.is_empty(), "the project is not valid: {errors:?}");
    let emit_op = match generate(project) {
        Ok(emit_op) => emit_op,
        Err(error) if rejects_drawing(&error) => return Ok(()),
        Err(error) => {
            return Err(TestCaseError::fail(format!(
                "a valid project fails to generate: {error}\n{error:?}"
            )));
        }
    };
    assert_safe(&emit_op.toolpath, project, &emit_op.op);
    if let Err(error) = emit(project, &[emit_op], DATE) {
        return Err(TestCaseError::fail(format!(
            "the safety pass rejects a generated toolpath: {error}\n{error:?}"
        )));
    }
    Ok(())
}

proptest! {
    #![proptest_config(config(64))]

    /// Profile: any side, direction, depth, step-down, tabs, entry, lead,
    /// corner relief and machining order.
    #[test]
    fn random_valid_profiles_never_panic_and_pass_safety(project in projects::profile_project()) {
        exports_safely(&project)?;
    }

    /// Left and Right (#207): open paths with arcs offset to either side,
    /// any direction, depth, step-down, tabs, ramp, lead and corner relief
    /// (both warned), beside closed outlines it skips.
    #[test]
    fn random_valid_left_right_profiles_never_panic_and_pass_safety(
        project in projects::open_side_profile_project()
    ) {
        exports_safely(&project)?;
    }

    /// Pocket: offset and raster at any angle, any wall pass, allowance,
    /// step-over, entry (plunge, ramp, helix) and corner relief.
    #[test]
    fn random_valid_pockets_never_panic_and_pass_safety(project in projects::pocket_project()) {
        exports_safely(&project)?;
    }

    /// Drill: simple and peck cycles, full and chip-break retracts, with
    /// and without a dwell, canned cycles where the post-processor has
    /// them; rectangles in the targets are skipped with a warning.
    #[test]
    fn random_valid_drills_never_panic_and_pass_safety(project in projects::drill_project()) {
        exports_safely(&project)?;
    }

    /// Engrave: closed and open paths with arcs, any cutter, alternating
    /// or not.
    #[test]
    fn random_valid_engraves_never_panic_and_pass_safety(project in projects::engrave_project()) {
        exports_safely(&project)?;
    }
}

/// The cutting moves of `toolpath` that move in XY, as text, sorted: the
/// multiset O2 compares. Moves in Z alone (plunges, tab lifts) depend on
/// where the tool comes from, so they are left out.
fn cutting_moves(toolpath: &Toolpath) -> Vec<String> {
    let mut out = Vec::new();
    let mut at = None;
    for mv in &toolpath.moves {
        let moves_xy = at.is_none_or(|p: dc_geometry::Point3| {
            p.xy().distance_to(mv.to.xy()) > dc_geometry::EPS_MM
        }) || !matches!(mv.kind, MoveKind::Linear | MoveKind::Rapid);
        if !mv.is_rapid() && moves_xy {
            out.push(format!("{mv:?}"));
        }
        at = Some(mv.to);
    }
    out.sort();
    out
}

proptest! {
    #![proptest_config(config(64))]

    /// O2: a multi-shape Profile cut full depth first and pass first makes
    /// the same cutting moves, or fails the same way under both.
    #[test]
    fn sequence_is_a_permutation(project in projects::profile_project()) {
        let Operation::Profile(op) = &project.operations[0] else {
            panic!("a profile project has a Profile");
        };
        let mut full = op.clone();
        full.machining_order = MachiningOrder::FullDepthFirst;
        let mut pass = op.clone();
        pass.machining_order = MachiningOrder::PassFirst;
        match (generate_profile(&project, &full), generate_profile(&project, &pass)) {
            (Ok((a, _)), Ok((b, _))) => {
                let (a, b) = (cutting_moves(&a), cutting_moves(&b));
                prop_assert!(!a.is_empty() || project.instances.is_empty());
                prop_assert_eq!(a, b);
            }
            (Err(a), Err(b)) => prop_assert_eq!(a, b),
            (a, b) => {
                return Err(TestCaseError::fail(format!(
                    "the orders disagree: full depth first {:?}, pass first {:?}",
                    a.err(),
                    b.err()
                )));
            }
        }
    }
}
