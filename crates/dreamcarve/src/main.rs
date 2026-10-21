// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The `dreamcarve` command line: `init`, `export`, `check`, and `posts`
//! (`docs/specs/phase-1/functional.md`, capabilities 1–5;
//! `docs/specs/phase-1/design.md`, "CLI (dreamcarve)"), and from phase 2
//! `config` and `tools` (`docs/specs/phase-2/design.md`, "Tool library
//! CLI").
//!
//! The binary owns argument parsing, file I/O, the clock, printing, and exit
//! codes. Every decision about geometry, toolpaths, and G-code is made by
//! the `dc-*` crates it calls.

use std::process::ExitCode;

mod check;
mod cli;
mod clock;
mod config;
mod export;
mod init;
mod tools;

fn main() -> ExitCode {
    cli::run(std::env::args_os())
}
