// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 1 of phase 4 (`docs/specs/phase-4/design.md`, "Test plan → dc-job
//! (slice 1)"): the pipeline moved out of the CLI produces the bytes the
//! CLI produced, in one call or in the app's two halves, and its findings
//! carry what a Warnings panel shows.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_job::{Built, EstimateConfig, Finding, ProgramSplit, Tier, program_names};
use dc_project::test_support::{profile_mut, sample_project};
use dc_project::{Operation, OperationId, ToolId};

use common::{DATE, expected, first_difference, load_dcv, read, repo_root};

/// Every quick-start project (`tests/fixtures/*.dcv`), every golden case
/// with a DreamCarve project and the tab-request corpus, through
/// [`dc_job::build`], gives byte for byte the programs the CLI wrote
/// before the move (`expected/quick-start/`, `expected/golden/`, and the
/// CLI's own `tests/expected/tabs-migration/`), under the names
/// [`program_names`] gives them.
#[test]
fn cli_programs_are_byte_identical_after_the_move() {
    let cli_expected = repo_root().join("crates/dreamcarve/tests/expected/tabs-migration");
    let groups: [(
        &str,
        Vec<common::Fixture>,
        Box<dyn Fn(&str) -> std::path::PathBuf>,
    ); 3] = [
        (
            "quick-start",
            common::quick_start_projects(),
            Box::new(|name| expected("quick-start", name)),
        ),
        (
            "golden",
            common::golden_projects(),
            Box::new(|name| expected("golden", name)),
        ),
        (
            "corpus",
            common::corpus_projects(),
            Box::new(move |name| cli_expected.join(name)),
        ),
    ];
    let mut compared = 0;
    for (group, fixtures, expected_path) in &groups {
        for (stem, path) in fixtures {
            let project = load_dcv(path);
            let built = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
            assert!(
                built.findings.errors.is_empty(),
                "{group}/{stem}: {:?}",
                built.findings.errors
            );
            let names = program_names(&project, stem, &built.programs);
            assert_eq!(names.len(), built.programs.len(), "{group}/{stem}");
            for (name, program) in names.iter().zip(&built.programs) {
                let committed = read(&expected_path(name));
                assert!(
                    program.text == committed,
                    "{group}/{stem}: {name} differs from the program the CLI wrote before the move: {}",
                    first_difference(&program.text, &committed)
                );
                compared += 1;
            }
        }
    }
    assert!(compared >= 40, "{compared} programs compared");
}

/// For every committed project (the quick-start projects, the golden
/// cases, the tab-request corpus): [`dc_job::validation`], then
/// [`dc_job::generate_operation`] for each enabled operation, then
/// [`dc_job::finish`], gives the same findings, in the same order, and the
/// same programs and names as [`dc_job::build`]; with every split the CLI
/// offers for the two-tool plaque. Where validation or generation finds an
/// error, both stop there with the same errors and no program.
#[test]
fn generate_operation_then_finish_equals_build() {
    for (stem, path) in common::checked_projects() {
        let project = load_dcv(&path);
        let splits: &[ProgramSplit] = if stem == "plaque" {
            &[
                ProgramSplit::ProjectDefault,
                ProgramSplit::ProgramPerTool,
                ProgramSplit::OneProgram,
            ]
        } else {
            &[ProgramSplit::ProjectDefault]
        };
        for &split in splits {
            let case = format!("{stem} ({split:?})");
            let built = dc_job::build(&project, split, DATE);

            let mut findings = dc_job::validation(&project);
            if findings.has_errors() {
                assert_eq!(built.findings, findings, "{case}: validation stops both");
                assert!(
                    built.programs.is_empty() && built.generated.is_none(),
                    "{case}"
                );
                continue;
            }
            let mut runs = Vec::new();
            for op in project.operations.iter().filter(|op| op.enabled()) {
                let generated = dc_job::generate_operation(&project, op.id());
                findings.extend(generated.findings);
                runs.extend(generated.runs);
            }
            if findings.has_errors() {
                assert_eq!(built.findings.errors, findings.errors, "{case}");
                assert!(
                    built.programs.is_empty() && built.generated.is_none(),
                    "{case}"
                );
                continue;
            }
            let finished = dc_job::finish(&project, &runs, split, DATE, &EstimateConfig::default());
            findings.extend(finished.findings);
            assert_eq!(findings, built.findings, "{case}: findings");
            assert_eq!(
                built.generated.as_deref(),
                Some(runs.as_slice()),
                "{case}: runs"
            );
            let programs: Vec<_> = finished.programs.iter().map(|s| &s.program).collect();
            let built_programs: Vec<_> = built.programs.iter().collect();
            assert_eq!(programs, built_programs, "{case}: programs");
            let names = program_names(&project, &project.name, &built.programs);
            for (k, summary) in finished.programs.iter().enumerate() {
                assert_eq!(summary.name, names[k], "{case}: program {k}");
                assert!(
                    !summary.operations.is_empty(),
                    "{case}: program {k} cuts nothing"
                );
                for op in &summary.operations {
                    assert!(
                        project
                            .operations
                            .iter()
                            .any(|o| o.id() == op && o.enabled()),
                        "{case}: program {k} names `{op}`"
                    );
                }
                assert!(
                    summary.before.starts_with("fit T"),
                    "{case}: {}",
                    summary.before
                );
                assert!(
                    summary.estimate_line.starts_with("Estimated time: "),
                    "{case}: {}",
                    summary.estimate_line
                );
                // The edge extents widen the tool centre's in X and Y only.
                let (centre, edge) = (&summary.program.extents, &summary.edge_extents);
                assert_eq!(edge.z, centre.z, "{case}");
                if let (Some(c), Some(e)) = (centre.x, edge.x) {
                    assert!(e.min <= c.min && e.max >= c.max, "{case}: {e:?} vs {c:?}");
                }
            }
            assert_eq!(
                finished.total_line.is_some(),
                finished.programs.len() > 1,
                "{case}: total line"
            );
        }
    }
}

/// Design "Data model → dc-job": `tool_change_avoidable` and `unused_tool`
/// are hints, every other warning a warning; an error has no code; a
/// located finding names its field and its operation; a finding about a
/// shape names it.
#[test]
fn findings_carry_tier_op_and_shape() {
    // `unused_tool`: a hint at the tool's field, about no operation.
    let mut project = sample_project();
    let mut spare = project.tools[0].clone();
    spare.id = ToolId::new("t9");
    spare.name = "spare".into();
    project.tools.push(spare);
    let findings = dc_job::validation(&project);
    let unused = findings
        .warnings
        .iter()
        .find(|f| f.code.as_deref() == Some("unused_tool"))
        .unwrap_or_else(|| panic!("{findings:?}"));
    assert_eq!(unused.tier, Tier::Hint);
    assert_eq!(unused.field.as_deref(), Some("tools[t9]"));
    assert_eq!(unused.op, None);

    // `tool_change_avoidable`: a hint from the order check, for Profiles
    // alternating t1, t2, t1.
    let mut project = sample_project();
    let mut second = project.tools[0].clone();
    second.id = ToolId::new("t2");
    second.name = "3 mm".into();
    second.diameter_mm = 3.0;
    project.tools.push(second);
    let first = profile_mut(&mut project).clone();
    let mut with_t2 = first.clone();
    with_t2.id = OperationId::new("o2");
    with_t2.tool = ToolId::new("t2");
    let mut again = first.clone();
    again.id = OperationId::new("o3");
    project.operations.push(Operation::Profile(with_t2));
    project.operations.push(Operation::Profile(again));
    let Built { findings, .. } = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
    let hint = findings
        .warnings
        .iter()
        .find(|f| f.code.as_deref() == Some("tool_change_avoidable"))
        .unwrap_or_else(|| panic!("{findings:?}"));
    assert_eq!(hint.tier, Tier::Hint);
    assert!(
        findings
            .warnings
            .iter()
            .filter(|f| f.code.as_deref() != Some("tool_change_avoidable"))
            .all(|f| f.tier == Tier::Warning),
        "{findings:?}"
    );
    assert!(findings.warnings.iter().all(|f| f.code.is_some()));

    // An error has no code, and one about an operation's value names both.
    let mut project = sample_project();
    profile_mut(&mut project).depth_mm = None;
    let Built { findings, .. } = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
    let error: &Finding = findings
        .errors
        .iter()
        .find(|f| f.field.as_deref() == Some("operations[o1].depth_mm"))
        .unwrap_or_else(|| panic!("{findings:?}"));
    assert_eq!(error.tier, Tier::Error);
    assert_eq!(error.code, None);
    assert_eq!(error.op, Some(OperationId::new("o1")));
    assert!(findings.errors.iter().all(|f| f.code.is_none()));

    // A generation warning about a shape names the shape and its
    // operation: the tab-request corpus's square with a hole, whose tabs
    // would cover most of the hole's path.
    let (_, path) = &common::corpus_projects()[1];
    let project = load_dcv(path);
    let Built { findings, .. } = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
    let about_shape = findings
        .warnings
        .iter()
        .find(|f| f.shape.is_some())
        .unwrap_or_else(|| panic!("{findings:?}"));
    assert_eq!(about_shape.op, Some(OperationId::new("o1")));
    assert_eq!(
        about_shape.code.as_deref(),
        Some("tabs_cover_most_of_contour")
    );
    let shape = about_shape.shape.as_ref().unwrap();
    assert!(
        about_shape.message.contains(&shape.to_string()),
        "{about_shape:?}"
    );
}
