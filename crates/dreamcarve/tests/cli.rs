// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Behaviour tests for the `dreamcarve` binary. Phase 0: it exists and
//! reports its version (`docs/specs/phase-0/functional.md`, capability 1).

use std::process::Command;

fn dreamcarve() -> Command {
    Command::new(env!("CARGO_BIN_EXE_dreamcarve"))
}

#[test]
fn version_flag_prints_name_and_cargo_version() {
    let out = dreamcarve().arg("--version").output().expect("binary runs");
    assert!(out.status.success(), "exit status: {:?}", out.status);
    let stdout = String::from_utf8(out.stdout).expect("utf-8 stdout");
    assert_eq!(
        stdout.trim(),
        format!("dreamcarve {}", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn no_arguments_prints_usage_and_fails() {
    let out = dreamcarve().output().expect("binary runs");
    assert!(!out.status.success(), "no subcommand must be an error");
    let stderr = String::from_utf8(out.stderr).expect("utf-8 stderr");
    assert!(stderr.contains("Usage"), "stderr was: {stderr}");
}
