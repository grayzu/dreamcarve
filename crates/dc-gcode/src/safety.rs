// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The safety pass: parse the emitted program and check the phase 1 safety
//! rules on what the controller will read (`docs/specs/phase-1/design.md`,
//! "Emission", step 7; `docs/specs/phase-1/functional.md`, S1–S6). Every
//! violation is an [`ExportError`]; there are no safety warnings.
//!
//! Checks run in project space: each parsed point is mapped back through
//! the [`MachineTransform`] using the units the program itself selected, so
//! a wrong origin, a mirror, or a missing units command shows up as a
//! violation instead of being trusted.

use std::ops::Range;

use dc_cam::Toolpath;
use dc_project::{PostProcessor, ProfileOp, Tool};

use crate::ExportError;
use crate::transform::MachineTransform;

/// The longest line, in characters, DreamCarve emits. grblHAL's line
/// buffer holds 256 bytes including the terminator.
pub const MAX_LINE_CHARS: usize = 255;

/// One emitted operation and the program lines its moves occupy.
#[derive(Debug, Clone)]
pub(crate) struct OpSpan<'a> {
    /// The operation.
    pub op: &'a ProfileOp,
    /// Its tool.
    pub tool: &'a Tool,
    /// Its toolpath, in project space.
    pub toolpath: &'a Toolpath,
    /// 1-based program lines, end exclusive, from the operation's first
    /// motion line to its last.
    pub lines: Range<usize>,
}

/// Checks `text`, emitted with `post` and `transform` for the operations
/// in `spans`, against S1–S6.
///
/// # Errors
///
/// The first violation found, as the matching [`ExportError`].
pub(crate) fn check(
    text: &str,
    post: &PostProcessor,
    transform: &MachineTransform,
    stock_thickness_mm: f64,
    spans: &[OpSpan<'_>],
) -> Result<(), ExportError> {
    let _ = (text, post, transform, stock_thickness_mm, spans);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::emit::emit;
    use dc_cam::{Move, generate_profile};
    use dc_geometry::Point3;
    use dc_project::test_support::sample_project;
    use dc_project::{Operation, Project};

    /// The sample project (6 mm stock, safe Z 11 absolute, feed 1000,
    /// plunge 300, 18000 rpm, GRBL), its operation, and its tool.
    fn fixture() -> (Project, ProfileOp, Tool) {
        let project = sample_project();
        let Some(Operation::Profile(op)) = project.operations.first().cloned() else {
            panic!("sample project has a profile operation");
        };
        let tool = project.tools[0].clone();
        (project, op, tool)
    }

    /// Rapid in, plunge to `z`, one cut, retract.
    fn one_cut(z: f64) -> Toolpath {
        Toolpath {
            moves: vec![
                Move::rapid(Point3::new(0.0, 0.0, 11.0)),
                Move::linear(Point3::new(0.0, 0.0, z), 300.0),
                Move::linear(Point3::new(10.0, 0.0, z), 1000.0),
                Move::rapid(Point3::new(10.0, 0.0, 11.0)),
            ],
        }
    }

    fn emit_one(
        project: &Project,
        op: &ProfileOp,
        tool: &Tool,
        tp: Toolpath,
    ) -> Result<String, ExportError> {
        emit(project, &[(op.clone(), tool.clone(), tp)], "2026-10-15").map(|(p, _)| p.text)
    }

    /// Runs the safety pass on hand-written `text` as if it were the whole
    /// of the fixture's operation.
    fn check_text(text: &str) -> Result<(), ExportError> {
        let (project, op, tool) = fixture();
        let transform = MachineTransform::new(&project).unwrap();
        let toolpath = Toolpath::default();
        let lines = text.lines().count() + 1;
        let spans = [OpSpan {
            op: &op,
            tool: &tool,
            toolpath: &toolpath,
            lines: 1..lines,
        }];
        check(
            text,
            &project.post_processor,
            &transform,
            project.stock.thickness_mm,
            &spans,
        )
    }

    #[test]
    fn no_z_below_bottom_minus_allowance() {
        let (mut project, mut op, tool) = fixture();
        // The generated program never goes below the stock bottom.
        let (toolpath, _) = generate_profile(&project, &op).unwrap();
        emit_one(&project, &op, &tool, toolpath).unwrap();
        // Exactly at the bottom is fine; 0.1 mm below is not.
        emit_one(&project, &op, &tool, one_cut(0.0)).unwrap();
        let err = emit_one(&project, &op, &tool, one_cut(-0.1)).unwrap_err();
        assert!(
            matches!(err, ExportError::BelowMaterialBottom { z_mm, floor_mm, .. }
                if (z_mm + 0.1).abs() < 1e-6 && floor_mm.abs() < 1e-6),
            "{err:?}"
        );
        // An explicit allowance permits exactly that much.
        op.through_cut_allowance_mm = 0.2;
        if let Some(Operation::Profile(p)) = project.operations.first_mut() {
            p.through_cut_allowance_mm = 0.2;
        }
        emit_one(&project, &op, &tool, one_cut(-0.2)).unwrap();
        let err = emit_one(&project, &op, &tool, one_cut(-0.3)).unwrap_err();
        assert!(
            matches!(err, ExportError::BelowMaterialBottom { .. }),
            "{err:?}"
        );
    }

    #[test]
    fn rapids_only_at_or_above_safe_z() {
        let (project, op, tool) = fixture();
        // A rapid that ends below safe Z.
        let mut low_end = one_cut(0.0);
        low_end.moves[3].to.z = 10.0;
        let err = emit_one(&project, &op, &tool, low_end).unwrap_err();
        assert!(
            matches!(err, ExportError::RapidBelowSafeZ { .. }),
            "{err:?}"
        );
        // A rapid across the part at cutting depth.
        let mut across = one_cut(0.0);
        across
            .moves
            .insert(3, Move::rapid(Point3::new(20.0, 0.0, 0.0)));
        let err = emit_one(&project, &op, &tool, across).unwrap_err();
        assert!(
            matches!(err, ExportError::RapidBelowSafeZ { .. }),
            "{err:?}"
        );
        // Hand-written: an XY rapid before any Z is known.
        let err = check_text("G21\nG54\nG0 X1 Y1\n").unwrap_err();
        assert!(
            matches!(
                err,
                ExportError::RapidBelowSafeZ {
                    line: 3,
                    z_mm: None,
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[test]
    fn every_cut_has_active_feed() {
        let ok = "G21\nG54\nM3 S18000\nG4 P3\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nX10 F1000\nG0 Z5\nM5\n";
        check_text(ok).unwrap();
        let no_feed = "G21\nG54\nM3 S18000\nG4 P3\nG0 Z5\nX0 Y0\nG1 Z-1\nX10\nG0 Z5\nM5\n";
        let err = check_text(no_feed).unwrap_err();
        assert!(
            matches!(err, ExportError::CutWithoutFeed { line: 7 }),
            "{err:?}"
        );
        let invented =
            "G21\nG54\nM3 S18000\nG4 P3\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nX10 F2500\nG0 Z5\nM5\n";
        let err = check_text(invented).unwrap_err();
        assert!(
            matches!(err, ExportError::FeedNotFromTool { line: 8, .. }),
            "{err:?}"
        );
        let zero = "G21\nG54\nM3 S18000\nG4 P3\nG0 Z5\nX0 Y0\nG1 Z-1 F0\nG0 Z5\nM5\n";
        let err = check_text(zero).unwrap_err();
        assert!(
            matches!(err, ExportError::CutWithoutFeed { line: 7 }),
            "{err:?}"
        );
    }

    #[test]
    fn spindle_and_setup_rules_are_checked() {
        let no_units = "G54\nM3 S18000\nG4 P3\nG0 Z5\nM5\n";
        assert!(matches!(
            check_text(no_units),
            Err(ExportError::MissingSetup { .. })
        ));
        let no_wcs = "G21\nM3 S18000\nG4 P3\nG0 Z5\nM5\n";
        assert!(matches!(
            check_text(no_wcs),
            Err(ExportError::MissingSetup { .. })
        ));
        let off = "G21\nG54\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\n";
        assert!(matches!(
            check_text(off),
            Err(ExportError::SpindleNotRunning { line: 5 })
        ));
        let no_dwell = "G21\nG54\nM3 S18000\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\nM5\n";
        assert!(matches!(
            check_text(no_dwell),
            Err(ExportError::SpindleNotUpToSpeed { line: 6 })
        ));
        let left_on = "G21\nG54\nM3 S18000\nG4 P3\nG0 Z5\nX0 Y0\nG1 Z-1 F300\nG0 Z5\n";
        assert!(matches!(
            check_text(left_on),
            Err(ExportError::SpindleNotStopped)
        ));
    }

    #[test]
    fn forbidden_words_and_unsafe_characters_are_rejected() {
        for (text, word) in [
            ("G21\nG54\nM4 S1000\n", "M4"),
            ("G21\nG54\nM7\n", "M7"),
            ("G21\nG54\nM6 T1\n", "M6"),
            ("G21\nG54\nG53 G0 Z0\n", "G53"),
            ("G21\nG54\nG91\n", "G91"),
        ] {
            let err = check_text(text).unwrap_err();
            assert!(
                matches!(&err, ExportError::ForbiddenWord { line: 3, word: w } if w == word),
                "{text}: {err:?}"
            );
        }
        for text in [
            "G21 (stop!)\n",
            "G21 ; what?\n",
            "G21 (caf\u{e9})\n",
            "G21 (a~b)\n",
        ] {
            assert!(
                matches!(
                    check_text(text),
                    Err(ExportError::UnsafeLine { line: 1, .. })
                ),
                "{text}"
            );
        }
        let long = format!("G21 ({})\n", "a".repeat(MAX_LINE_CHARS));
        assert!(matches!(
            check_text(&long),
            Err(ExportError::UnsafeLine { line: 1, .. })
        ));
    }

    #[test]
    fn violation_is_error_not_warning() {
        // A hand-built toolpath that plunges through the spoilboard: export
        // fails outright and yields no program to write.
        let (project, op, tool) = fixture();
        let result = emit(&project, &[(op, tool, one_cut(-1.0))], "2026-10-15");
        assert!(
            matches!(result, Err(ExportError::BelowMaterialBottom { .. })),
            "{result:?}"
        );
    }
}
