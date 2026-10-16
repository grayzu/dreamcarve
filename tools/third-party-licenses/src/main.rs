// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `cargo run -p third-party-licenses -- generate [OUTPUT]`: writes
//! THIRD-PARTY-LICENSES (default: at the workspace root) with cargo-about,
//! then checks it covers every crate shipped in the `dreamcarve` binary.
//!
//! `cargo run -p third-party-licenses -- check [FILE]`: the check alone.
//!
//! Both exit non-zero when a shipped crate has no license text. The release
//! workflow runs `generate` before `dist build` (`.github/build-setup.yml`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use third_party_licenses::{Coverage, Error, LICENSES_FILE, check, generate};

const USAGE: &str = "usage: third-party-licenses <generate [OUTPUT] | check [FILE]>";

fn main() -> ExitCode {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut args = std::env::args_os().skip(1);
    let command = args.next();
    let file = args
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(LICENSES_FILE));

    let result = match command.as_ref().and_then(|c| c.to_str()) {
        Some("generate") => generate(&root, &file).and_then(|()| check(&root, &file)),
        Some("check") => check(&root, &file),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    report(&file, result)
}

fn report(file: &Path, result: Result<Coverage, Error>) -> ExitCode {
    match result {
        Ok(coverage) if coverage.uncovered.is_empty() => {
            println!(
                "third-party-licenses: OK ({} shipped crates covered by {})",
                coverage.shipped.len(),
                file.display()
            );
            ExitCode::SUCCESS
        }
        Ok(coverage) => {
            eprintln!(
                "third-party-licenses: FAIL ({} shipped crate(s) without a license text in {}):",
                coverage.uncovered.len(),
                file.display()
            );
            for id in &coverage.uncovered {
                eprintln!("  {id}");
            }
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("third-party-licenses: {e}");
            ExitCode::FAILURE
        }
    }
}
