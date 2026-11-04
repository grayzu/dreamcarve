// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A project from a drawing (`docs/specs/phase-4/design.md`, "Test plan →
//! dc-job (slice 1)"; functional spec, acceptance 63). That the project
//! with defaults is byte for byte `init`'s is
//! `crates/dreamcarve/tests/pipeline_move.rs`, against the binary.

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

use common::repo_root;

/// The committed drawings `init` runs on, with the project name `init`
/// gives each.
fn drawings() -> Vec<(&'static str, std::path::PathBuf)> {
    vec![
        ("coaster", repo_root().join("fixtures/coaster.svg")),
        ("plaque", repo_root().join("tests/fixtures/plaque.dxf")),
        ("plaque", repo_root().join("tests/fixtures/plaque.svg")),
        ("sign", repo_root().join("tests/fixtures/sign.svg")),
        ("dc-inlay", repo_root().join("tests/fixtures/dc-inlay.svg")),
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

/// Acceptance 63 (functional spec, open question 1): without defaults the
/// project is `init`'s with the placeholder tool and the Profile removed:
/// same stock, same instances and definitions, same post-processor, no
/// tools, no operations, no tabs; and it validates with nothing to set but
/// the stock's thickness.
#[test]
fn drawing_without_defaults_has_no_tool_and_no_operation() {
    for (name, path) in drawings() {
        let case = path.display().to_string();
        let post = PostProcessorRef::latest("grblhal").unwrap();
        let with = project_from::drawing(name, import(&path), post.clone(), Units::Mm, true)
            .unwrap_or_else(|e| panic!("{case}: {e}"));
        let without = project_from::drawing(name, import(&path), post, Units::Mm, false)
            .unwrap_or_else(|e| panic!("{case}: {e}"));
        assert!(without.tools.is_empty(), "{case}");
        assert!(without.operations.is_empty(), "{case}");
        assert!(without.tabs.is_empty(), "{case}");
        assert_eq!(without.stock, with.stock, "{case}");
        assert_eq!(without.origin, with.origin, "{case}");
        assert_eq!(without.instances, with.instances, "{case}");
        assert_eq!(without.definitions, with.definitions, "{case}");
        assert_eq!(without.post_processor, with.post_processor, "{case}");
        assert_eq!(without.name, with.name, "{case}");
        assert_eq!(without.display_units, with.display_units, "{case}");
        assert!(
            !with.tools.is_empty() && !with.operations.is_empty(),
            "{case}"
        );
        let findings = dc_job::validation(&without);
        assert!(
            findings
                .errors
                .iter()
                .all(|f| f.field.as_deref() == Some("stock.thickness_mm")),
            "{case}: {:?}",
            findings.errors
        );
    }
}
