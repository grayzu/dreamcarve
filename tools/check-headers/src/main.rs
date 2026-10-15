// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `cargo run -p check-headers [ROOT]`: fails with a list of files when any
//! `.rs` file under ROOT (default: the workspace root) lacks a license header.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));

    match check_headers::check_tree(&root) {
        Ok(Ok(count)) => {
            println!("check-headers: OK ({count} .rs files)");
            ExitCode::SUCCESS
        }
        Ok(Err(failures)) => {
            eprintln!(
                "check-headers: FAIL ({} file(s) without an accepted header):",
                failures.len()
            );
            for failure in &failures {
                eprintln!("  {failure}");
            }
            eprintln!("See AGENTS.md, \"Licensing rules\", for the two accepted headers.");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("check-headers: cannot read {}: {e}", root.display());
            ExitCode::FAILURE
        }
    }
}
