// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Placed tabs for a shell (`docs/specs/phase-4/design.md`, "Test plan →
//! dc-job (slice 1)"; functional spec, acceptance 78, first half).

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_job::JobError;
use dc_job::tabs::place_request;
use dc_project::{Operation, OperationId};

use common::{expected, first_difference, load_dcv, read, repo_root};

/// For each corpus project with a tab request, `place_request` on its
/// Profile gives the project `tabs place <dcv> o1` saved before the move
/// (`expected/tabs/`), byte for byte: the request placed as tabs and
/// cleared. A Profile with no request, and an operation that is no
/// Profile, are refused.
#[test]
fn place_request_equals_tabs_place() {
    let op = OperationId::new("o1");
    for name in [
        "rect-outside-climb",
        "square-with-hole-on-holes",
        "circle-rotated",
        "star-with-hole",
    ] {
        let path = repo_root()
            .join("crates/dreamcarve/tests/fixtures/tabs-migration")
            .join(format!("{name}.dcv"));
        let project = load_dcv(&path);
        let (placed, _warnings) =
            place_request(&project, &op).unwrap_or_else(|e| panic!("{name}: {e}"));
        let text = dc_project::save(&placed).unwrap();
        let committed = read(&expected("tabs", &format!("{name}.dcv")));
        assert!(
            text == committed,
            "{name} differs from what `tabs place` saved before the move: {}",
            first_difference(&text, &committed)
        );
        assert!(!placed.tabs.is_empty(), "{name}");
        assert!(
            placed.operations.iter().all(|o| match o {
                Operation::Profile(p) => p.tabs.is_none(),
                _ => true,
            }),
            "{name}: the request is cleared"
        );
        // Placed once, there is nothing left to place.
        assert!(
            matches!(
                place_request(&placed, &op),
                Err(JobError::NoTabRequest { op: ref o }) if o == &op
            ),
            "{name}"
        );
        assert!(matches!(
            place_request(&project, &OperationId::new("o99")),
            Err(JobError::Cam(dc_job::CamError::UnknownOperation { .. }))
        ));
    }
}
