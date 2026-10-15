// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The committed figures must match what the engine produces today, so the
//! user guide cannot drift from the code. Regenerate with
//! `cargo run -p doc-figures` and review the change.

#![allow(clippy::unwrap_used, clippy::panic, reason = "test")]

use std::path::Path;

#[test]
fn committed_figures_match_engine_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut stale = Vec::new();
    for figure in doc_figures::all() {
        let committed = std::fs::read_to_string(root.join(figure.path)).unwrap_or_default();
        // A Windows checkout may convert line endings; compare content only.
        if committed.replace("\r\n", "\n") != figure.svg {
            stale.push(figure.path);
        }
    }
    assert!(
        stale.is_empty(),
        "figures out of date (run `cargo run -p doc-figures`): {stale:?}"
    );
}
