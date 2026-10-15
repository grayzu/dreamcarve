// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The `dreamcarve` command line. Phase 0: it reports its version. Phase 1
//! adds `init`, `export`, `check`, and `posts`
//! (`docs/specs/phase-1/functional.md`).

use clap::Command;

fn cli() -> Command {
    clap::command!().arg_required_else_help(true)
}

fn main() {
    // No subcommands yet: with `arg_required_else_help`, clap prints usage
    // and exits non-zero when nothing is given, and handles `--version`.
    let _matches = cli().get_matches();
}
