// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The `dreamcarve` command line: `init`, `export`, `check`, and `posts`
//! (`docs/specs/phase-1/functional.md`, capabilities 1–5;
//! `docs/specs/phase-1/design.md`, "CLI (dreamcarve)"), and from phase 2
//! `config` and `tools` (`docs/specs/phase-2/design.md`, "Tool library
//! CLI") and `tabs` (#105, "Placed tabs").
//!
//! The binary owns argument parsing, the clock, printing, and exit codes,
//! and the file I/O it shares with the app through `dreamcarve-shell`.
//! Every decision about geometry, toolpaths, and G-code is made by the
//! `dc-*` crates it calls, through `dc-job`'s pipeline (phase 4, slice 1).
//! All printing goes through [`output`], which handles a closed pipe
//! (#216).

// `println!` and `eprintln!` panic when the stream fails; use `output`.
#![deny(clippy::print_stdout, clippy::print_stderr)]

use std::process::ExitCode;

mod check;
mod cli;
mod clock;
mod config;
mod export;
mod init;
mod output;
mod tabs;
mod tools;

fn main() -> ExitCode {
    cli::run(std::env::args_os())
}
