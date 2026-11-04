// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The user config file (`docs/specs/phase-2/design.md`, "User config (CLI
//! only)"), moved from `crates/dreamcarve/src/config.rs`.
//!
//! `<config dir>/config.toml` holds the defaults a user sets once:
//! `[export] tool_change_mode`, which `init` and `export --post
//! --reset-settings` write into a new post-processor reference (functional
//! spec, capability 19), and a plain `export --post` only where it resets
//! the project's mode (capability 27, #277); and `[estimate]`, the
//! machine's rapid rate and a scale factor for the cycle-time estimate
//! `export` and `check` print (capability 30). No program depends on the
//! file: a plain `export` reads only `[estimate]`, for the printed
//! estimate, so a project exports the same bytes on every machine.
//!
//! A missing file is the defaults. A file that does not parse, or has a
//! key DreamCarve does not know, is an error naming the file, line and
//! column, as `config.toml:2:19: …` (#76): a typo must never fall back to
//! a default silently.

use std::path::Path;

use dc_job::EstimateConfig;
use dc_project::{PostProcessorRef, ToolChangeMode};
use serde::Deserialize;

use crate::config_dir::Paths;
use crate::error::ShellError;

/// The user config file. Every table and key is optional and defaults to
/// what the bundled definitions default to; unknown ones are errors.
#[derive(Debug, Clone, PartialEq, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Config {
    /// Defaults for new post-processor settings.
    pub export: ExportConfig,
    /// The machine facts the printed cycle-time estimate needs.
    pub estimate: EstimateConfig,
}

/// The `[export]` table.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExportConfig {
    /// The `tool_change_mode` `init` and `export --post` write into a new
    /// post-processor reference. `"separate_programs"` unless set.
    pub tool_change_mode: ToolChangeMode,
}

/// Reads the config file of `paths`; a missing file is the defaults.
///
/// # Errors
///
/// [`ShellError::Read`] when the file exists but cannot be read, and
/// [`ShellError::Config`] when it does not parse ([`parse`]).
pub fn load_from(paths: &Paths) -> Result<Config, ShellError> {
    let path = &paths.config_file;
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(source) => Err(ShellError::Read {
            path: path.clone(),
            source,
        }),
    }
}

/// Parses `text`, the contents of the config file at `path`, which the
/// error names with the line and column of the problem.
///
/// # Errors
///
/// [`ShellError::Config`] for a syntax error, an unknown key or a bad
/// value.
pub fn parse(text: &str, path: &Path) -> Result<Config, ShellError> {
    toml::from_str(text).map_err(|e: toml::de::Error| ShellError::Config {
        path: path.to_path_buf(),
        at: e.span().map(|span| line_column(text, span.start)),
        message: e.message().to_owned(),
    })
}

/// 1-based line and column (in characters) of byte `offset` in `text`.
pub(crate) fn line_column(text: &str, offset: usize) -> (usize, usize) {
    let before = text.get(..offset).unwrap_or(text);
    let line = before.matches('\n').count() + 1;
    let column = before.rsplit('\n').next().map_or(0, |l| l.chars().count()) + 1;
    (line, column)
}

/// Puts the user's configured defaults on top of `post`, a reference with
/// the definition's default settings, as `init` and `export --post` write
/// it. A configured `"m6"` cannot apply to a definition whose tool change
/// has no `M6` (GRBL): that is an error naming the config file, so a bad
/// default never lands in a project.
///
/// # Errors
///
/// [`ShellError::M6Unavailable`], or [`ShellError::Post`] when `post`
/// does not resolve.
pub fn apply(
    config: &Config,
    paths: &Paths,
    post: &mut PostProcessorRef,
) -> Result<(), ShellError> {
    let mode = config.export.tool_change_mode;
    if mode == ToolChangeMode::M6 {
        let definition = post.resolve()?;
        if !definition.tool_change.has_m6() {
            return Err(ShellError::M6Unavailable {
                config: paths.config_file.clone(),
                post: post.id.clone(),
            });
        }
    }
    post.tool_change_mode = mode;
    Ok(())
}

/// Loads the user config from the process environment's config directory
/// and applies it to `post` ([`apply`]).
///
/// # Errors
///
/// As [`Paths::from_env`], [`load_from`] and [`apply`].
pub fn seed_post(post: &mut PostProcessorRef) -> Result<(), ShellError> {
    let paths = Paths::from_env()?;
    let config = load_from(&paths)?;
    apply(&config, &paths, post)
}

/// Reads the `[estimate]` table of the user config from the process
/// environment's config directory: the only part of the file a plain
/// `export` or `check` reads, for the printed estimate alone (design
/// spec, addendum A4). A bad value anywhere in the file is an error, as
/// for `init`.
///
/// # Errors
///
/// As [`Paths::from_env`] and [`load_from`].
pub fn estimate_settings() -> Result<EstimateConfig, ShellError> {
    let paths = Paths::from_env()?;
    Ok(load_from(&paths)?.estimate)
}

/// The CLI's tests of the config file, moved here with `err.message`
/// read through `Display` and the exit code, which is the CLI's, dropped.
#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::ToolChangeMode;
    use std::path::{Path, PathBuf};

    /// A fresh, empty directory for test `name`.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dc-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A missing config file is the defaults: one program per tool run,
    /// as the bundled definitions default to.
    #[test]
    fn missing_file_means_defaults() {
        let dir = scratch("missing");
        let paths = Paths::in_dir(dir.join("nowhere"));
        let config = load_from(&paths).unwrap();
        assert_eq!(config, Config::default());
        assert_eq!(
            config.export.tool_change_mode,
            ToolChangeMode::SeparatePrograms
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A syntax error names the file and the line, as `file:line:column:`,
    /// so the user can go straight to it (#76).
    #[test]
    fn malformed_file_names_line() {
        let path = Path::new("/somewhere/config.toml");
        let err = parse("[export]\ntool_change_mode = m6\n", path).unwrap_err();
        let message = err.to_string();
        assert!(
            message.starts_with("/somewhere/config.toml:2:"),
            "{message}"
        );
        // A value outside the enum is located too.
        let message = parse("[export]\ntool_change_mode = \"both\"\n", path)
            .unwrap_err()
            .to_string();
        assert!(
            message.starts_with("/somewhere/config.toml:2:"),
            "{message}"
        );
        assert!(message.contains("separate_programs"), "{message}");
    }

    /// A key DreamCarve does not know is a typo, never a silent default:
    /// the error names the key, at its line.
    #[test]
    fn unknown_key_is_error() {
        let path = Path::new("config.toml");
        let message = parse("[export]\ntool_change_mod = \"m6\"\n", path)
            .unwrap_err()
            .to_string();
        assert!(message.contains("tool_change_mod"), "{message}");
        assert!(message.starts_with("config.toml:2:"), "{message}");
        let message = parse("[unknown]\nkey = 1\n", path).unwrap_err().to_string();
        assert!(message.contains("unknown"), "{message}");
        // The documented file parses, and an empty one is the defaults.
        let config = parse("[export]\ntool_change_mode = \"m6\"\n", path).unwrap();
        assert_eq!(config.export.tool_change_mode, ToolChangeMode::M6);
        assert_eq!(parse("", path).unwrap(), Config::default());
    }

    /// `[estimate]` (functional spec, capability 30): `rapid_mm_min` and
    /// `scale_factor` parse, integers too; unset, there is no rapid rate
    /// and the scale factor is 1. A rate or factor that is 0, negative or
    /// not finite is an error at the value's line and column, never a
    /// silent default.
    #[test]
    fn estimate_rejects_non_positive_rate() {
        let path = Path::new("/somewhere/config.toml");
        let config = parse(
            "[estimate]\nrapid_mm_min = 3000\nscale_factor = 1.5\n",
            path,
        )
        .unwrap();
        assert_eq!(config.estimate.rapid_mm_min, Some(3000.0));
        assert!((config.estimate.scale_factor - 1.5).abs() < 1e-12);
        let defaults = parse("", path).unwrap().estimate;
        assert_eq!(defaults.rapid_mm_min, None);
        assert!((defaults.scale_factor - 1.0).abs() < 1e-12);
        for (text, key) in [
            ("[estimate]\nrapid_mm_min = 0\n", "rapid_mm_min"),
            ("[estimate]\nrapid_mm_min = -3000.0\n", "rapid_mm_min"),
            ("[estimate]\nrapid_mm_min = inf\n", "rapid_mm_min"),
            ("[estimate]\nscale_factor = 0.0\n", "scale_factor"),
            ("[estimate]\nscale_factor = nan\n", "scale_factor"),
        ] {
            let message = parse(text, path).unwrap_err().to_string();
            assert!(
                message.starts_with("/somewhere/config.toml:2:"),
                "{text}: {message}"
            );
            assert!(message.contains("above 0"), "{text}: {message}");
            let column: usize = message
                .trim_start_matches("/somewhere/config.toml:2:")
                .split(':')
                .next()
                .and_then(|c| c.parse().ok())
                .unwrap_or_else(|| panic!("{text}: no column in {message}"));
            assert_eq!(column, key.len() + 4, "{text}: at the value");
        }
        let message = parse("[estimate]\nrapid_rate = 3000\n", path)
            .unwrap_err()
            .to_string();
        assert!(message.contains("rapid_rate"), "{message}");
    }
}
