// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A project from a drawing (`docs/specs/phase-4/design.md`, "Test plan →
//! dc-job (slice 1)"; functional spec, acceptance 63).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_import::{DxfImportOptions, SvgImportOptions, import_dxf, import_svg};
use dc_job::import::Drawing;
use dc_job::project_from;
use dc_project::{PostProcessorRef, Units};

use common::{expected, first_difference, read, repo_root};

/// The committed drawings `init` was run on before the move, and the file
/// name of each project it wrote (`expected/init/`).
fn drawings() -> Vec<(&'static str, std::path::PathBuf, &'static str)> {
    vec![
        (
            "coaster",
            repo_root().join("fixtures/coaster.svg"),
            "coaster-svg.dcv",
        ),
        (
            "plaque",
            repo_root().join("tests/fixtures/plaque.dxf"),
            "plaque-dxf.dcv",
        ),
        (
            "plaque",
            repo_root().join("tests/fixtures/plaque.svg"),
            "plaque-svg.dcv",
        ),
        (
            "sign",
            repo_root().join("tests/fixtures/sign.svg"),
            "sign-svg.dcv",
        ),
        (
            "dc-inlay",
            repo_root().join("tests/fixtures/dc-inlay.svg"),
            "dc-inlay-svg.dcv",
        ),
    ]
}

/// The drawing at `path` imported as `init` imports it, with no flags.
fn import(path: &std::path::Path) -> Drawing {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    if path.extension().is_some_and(|x| x == "dxf") {
        let (import, _) = import_dxf(&bytes, &DxfImportOptions::new())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Drawing::Dxf(import)
    } else {
        let (import, _) = import_svg(&bytes, &SvgImportOptions::new())
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        Drawing::Svg(import)
    }
}

/// With defaults, the project equals the file `dreamcarve init` wrote for
/// the same drawing before the move, byte for byte once saved: the
/// coaster, the plaque from DXF and from SVG, the sign and the inlay
/// letters.
#[test]
fn drawing_with_defaults_equals_init() {
    for (name, path, written) in drawings() {
        let post = PostProcessorRef::latest("grblhal").unwrap();
        let project = project_from::drawing(name, import(&path), post, Units::Mm, true)
            .unwrap_or_else(|e| panic!("{written}: {e}"));
        let text = dc_project::save(&project).unwrap();
        let committed = read(&expected("init", written));
        assert!(
            text == committed,
            "{written} differs from what `init` wrote before the move: {}",
            first_difference(&text, &committed)
        );
    }
}

/// Acceptance 63 (functional spec, open question 1): without defaults the
/// project is `init`'s with the placeholder tool and the Profile removed:
/// same stock, same instances and definitions, same post-processor, no
/// tools, no operations, no tabs.
#[test]
fn drawing_without_defaults_has_no_tool_and_no_operation() {
    for (name, path, written) in drawings() {
        let post = PostProcessorRef::latest("grblhal").unwrap();
        let with = project_from::drawing(name, import(&path), post.clone(), Units::Mm, true)
            .unwrap_or_else(|e| panic!("{written}: {e}"));
        let without = project_from::drawing(name, import(&path), post, Units::Mm, false)
            .unwrap_or_else(|e| panic!("{written}: {e}"));
        assert!(without.tools.is_empty(), "{written}");
        assert!(without.operations.is_empty(), "{written}");
        assert!(without.tabs.is_empty(), "{written}");
        assert_eq!(without.stock, with.stock, "{written}");
        assert_eq!(without.origin, with.origin, "{written}");
        assert_eq!(without.instances, with.instances, "{written}");
        assert_eq!(without.definitions, with.definitions, "{written}");
        assert_eq!(without.post_processor, with.post_processor, "{written}");
        assert_eq!(without.name, with.name, "{written}");
        assert_eq!(without.display_units, with.display_units, "{written}");
        assert!(
            !with.tools.is_empty() && !with.operations.is_empty(),
            "{written}"
        );
        // It is a valid project with nothing to cut: no errors about unset
        // values of a tool or operation it does not have.
        let findings = dc_job::validation(&without);
        assert!(
            findings
                .errors
                .iter()
                .all(|f| f.field.as_deref() == Some("stock.thickness_mm")),
            "{written}: {:?}",
            findings.errors
        );
    }
}
