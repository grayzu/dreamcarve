// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve config path`, and the user config as the commands read it
//! (`docs/specs/phase-2/design.md`, "User config (CLI only)"). The
//! directory, the file and its parsing live in `dreamcarve-shell`
//! (`config_dir`, `config`), shared with the app; this module prints.

use std::process::ExitCode;

use dc_job::EstimateConfig;
use dc_project::PostProcessorRef;
pub use dreamcarve_shell::config_dir::Paths;

use crate::cli::{ConfigCommand, Failure};
use crate::output::outln;

/// Reads the `[estimate]` table of the user config: the only part of the
/// file a plain `export` or `check` reads, for the printed estimate alone
/// (design spec, addendum A4). A bad value anywhere in the file is an
/// error, as for `init`.
pub fn estimate_settings() -> Result<EstimateConfig, Failure> {
    Ok(dreamcarve_shell::config::estimate_settings()?)
}

/// Loads the user config and applies it to `post`: the configured
/// `tool_change_mode` on top of the definition's defaults, as `init` and
/// `export --post` write it (functional spec, capability 19).
pub fn seed_post(post: &mut PostProcessorRef) -> Result<(), Failure> {
    Ok(dreamcarve_shell::config::seed_post(post)?)
}

/// Runs `config <command>`.
pub fn run(command: &ConfigCommand) -> Result<ExitCode, Failure> {
    match command {
        ConfigCommand::Path => path(),
    }
}

/// `config path`: the directory and both files, each with whether it
/// exists, so a user can find or create them.
fn path() -> Result<ExitCode, Failure> {
    let paths = Paths::from_env()?;
    for (label, path) in [
        ("Config directory:", &paths.dir),
        ("Config file:", &paths.config_file),
        ("Tool library:", &paths.library_file),
    ] {
        let state = if path.exists() { "exists" } else { "not found" };
        outln!("{label:<17} {} ({state})", path.display())?;
    }
    Ok(ExitCode::SUCCESS)
}
