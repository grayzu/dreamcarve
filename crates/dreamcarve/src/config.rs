// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The user config file and directory (`docs/specs/phase-2/design.md`,
//! "User config (CLI only)") and `dreamcarve config path`.
//!
//! `<config dir>/config.toml` holds the defaults a user sets once:
//! `[export] tool_change_mode`, which `init` and `export --post` write
//! into a new post-processor reference (functional spec, capability 19),
//! and `[estimate]`, the machine's rapid rate and a scale factor for the
//! cycle-time estimate `export` and `check` print (capability 30). No
//! program depends on the file: a plain `export` reads only `[estimate]`,
//! for the printed estimate, so a project exports the same bytes on every
//! machine.
//! `<config dir>/tools.json` is the tool library (slice 20). `<config dir>`
//! is `DREAMCARVE_CONFIG_DIR` when that is set; otherwise the platform's
//! usual place, read from the environment with std alone (no crate for
//! it: the `directories` crate would bring in an MPL-2.0 dependency):
//! macOS `$HOME/Library/Application Support/dreamcarve`, Windows
//! `%APPDATA%\dreamcarve`, Linux and everything else
//! `$XDG_CONFIG_HOME/dreamcarve`, or `$HOME/.config/dreamcarve` when
//! `XDG_CONFIG_HOME` is unset. A missing `HOME` or `APPDATA` is an error
//! naming the variable.
//!
//! A missing file is the defaults. A file that does not parse, or has a
//! key DreamCarve does not know, is an error naming the file, line and
//! column, as `config.toml:2:19: …` (#76): a typo must never fall back to
//! a default silently.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use dc_project::{PostProcessorRef, ToolChangeMode};
use serde::Deserialize;

use crate::cli::{ConfigCommand, Failure};
use crate::output::outln;

/// The environment variable that overrides the config directory.
pub const CONFIG_DIR_VAR: &str = "DREAMCARVE_CONFIG_DIR";
/// The config file's name in the config directory.
pub const CONFIG_FILE_NAME: &str = "config.toml";
/// The tool library's name in the config directory.
pub const LIBRARY_FILE_NAME: &str = "tools.json";

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

/// The `[estimate]` table: facts about the user's machine that the
/// cycle-time estimate `export` and `check` print uses, and nothing else
/// (functional spec, capability 30). No program depends on it.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct EstimateConfig {
    /// The machine's rapid rate, mm/min, finite and above 0. Unset, rapids
    /// are reported as a distance and left out of the time, rather than
    /// guessed.
    #[serde(deserialize_with = "positive_some")]
    pub rapid_mm_min: Option<f64>,
    /// What every time is multiplied by, as VCarve's "Scale Factor": the
    /// user's correction for acceleration and the like. Finite and above
    /// 0; 1 unless set.
    #[serde(deserialize_with = "positive")]
    pub scale_factor: f64,
}

impl Default for EstimateConfig {
    fn default() -> Self {
        Self {
            rapid_mm_min: None,
            scale_factor: 1.0,
        }
    }
}

/// A number that is finite and above 0; anything else is an error at the
/// value, which [`parse`] locates.
fn positive<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
    let value = f64::deserialize(deserializer)?;
    if value.is_finite() && value > 0.0 {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(format!(
            "must be a number above 0, not {value}"
        )))
    }
}

/// [`positive`], for a key that may be left out.
fn positive_some<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<f64>, D::Error> {
    positive(deserializer).map(Some)
}

/// Reads the `[estimate]` table of the user config: the only part of the
/// file a plain `export` or `check` reads, for the printed estimate alone
/// (design spec, addendum A4). A bad value anywhere in the file is an
/// error, as for `init`.
pub fn estimate_settings() -> Result<EstimateConfig, Failure> {
    let paths = Paths::from_env()?;
    Ok(load_from(&paths)?.estimate)
}

/// The `[export]` table.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ExportConfig {
    /// The `tool_change_mode` `init` and `export --post` write into a new
    /// post-processor reference. `"separate_programs"` unless set.
    pub tool_change_mode: ToolChangeMode,
}

/// Where the config directory and its files are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The config directory.
    pub dir: PathBuf,
    /// `config.toml` in it.
    pub config_file: PathBuf,
    /// `tools.json` in it.
    pub library_file: PathBuf,
}

impl Paths {
    /// The paths under config directory `dir`.
    pub fn in_dir(dir: PathBuf) -> Self {
        let config_file = dir.join(CONFIG_FILE_NAME);
        let library_file = dir.join(LIBRARY_FILE_NAME);
        Self {
            dir,
            config_file,
            library_file,
        }
    }

    /// The paths for this run: under `override_dir` (the value of
    /// `DREAMCARVE_CONFIG_DIR`) when it is set and not empty, else under
    /// the platform's config directory for `dreamcarve`.
    pub fn resolve(override_dir: Option<&OsStr>) -> Result<Self, Failure> {
        match override_dir {
            Some(dir) if !dir.is_empty() => Ok(Self::in_dir(PathBuf::from(dir))),
            _ => platform_dir().map(Self::in_dir),
        }
    }

    /// [`Paths::resolve`] from the process environment.
    pub fn from_env() -> Result<Self, Failure> {
        Self::resolve(std::env::var_os(CONFIG_DIR_VAR).as_deref())
    }
}

/// The directory name under the platform's config directory.
const APP_DIR_NAME: &str = "dreamcarve";

/// The platform's config directory for DreamCarve, from the environment
/// (module doc). An unset or empty base variable is an error naming it.
fn platform_dir() -> Result<PathBuf, Failure> {
    if cfg!(target_os = "windows") {
        Ok(base_dir("APPDATA")?.join(APP_DIR_NAME))
    } else if cfg!(target_os = "macos") {
        Ok(base_dir("HOME")?
            .join("Library")
            .join("Application Support")
            .join(APP_DIR_NAME))
    } else {
        match env_path("XDG_CONFIG_HOME") {
            Some(xdg) => Ok(xdg.join(APP_DIR_NAME)),
            None => Ok(base_dir("HOME")?.join(".config").join(APP_DIR_NAME)),
        }
    }
}

/// Environment variable `var` as a path, `None` when unset or empty.
fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Environment variable `var` as a path; unset or empty is an error.
fn base_dir(var: &str) -> Result<PathBuf, Failure> {
    env_path(var).ok_or_else(|| {
        Failure::error(format!(
            "cannot find the user config directory: {var} is not set; set it, or set \
             {CONFIG_DIR_VAR} to the directory to use"
        ))
    })
}

/// Reads the config file of `paths`; a missing file is the defaults.
pub fn load_from(paths: &Paths) -> Result<Config, Failure> {
    let path = &paths.config_file;
    match std::fs::read_to_string(path) {
        Ok(text) => parse(&text, path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(e) => Err(Failure::error(format!(
            "cannot read {}: {e}",
            path.display()
        ))),
    }
}

/// Parses `text`, the contents of the config file at `path`, which the
/// error names with the line and column of the problem.
pub fn parse(text: &str, path: &Path) -> Result<Config, Failure> {
    toml::from_str(text).map_err(|e: toml::de::Error| {
        let message = match e.span() {
            Some(span) => {
                let (line, column) = line_column(text, span.start);
                format!("{}:{line}:{column}: {}", path.display(), e.message())
            }
            None => format!("{}: {}", path.display(), e.message()),
        };
        Failure::error(message)
    })
}

/// 1-based line and column (in characters) of byte `offset` in `text`.
fn line_column(text: &str, offset: usize) -> (usize, usize) {
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
pub fn apply(config: &Config, paths: &Paths, post: &mut PostProcessorRef) -> Result<(), Failure> {
    let mode = config.export.tool_change_mode;
    if mode == ToolChangeMode::M6 {
        let definition = post.resolve().map_err(Failure::error)?;
        if !definition.tool_change.has_m6() {
            return Err(Failure::error(format!(
                "{} sets tool_change_mode = \"m6\", but post-processor `{}` has no M6 tool \
                 change; set \"separate_programs\" there or choose another post-processor",
                paths.config_file.display(),
                post.id
            )));
        }
    }
    post.tool_change_mode = mode;
    Ok(())
}

/// Loads the user config and applies it to `post` ([`apply`]).
pub fn seed_post(post: &mut PostProcessorRef) -> Result<(), Failure> {
    let paths = Paths::from_env()?;
    let config = load_from(&paths)?;
    apply(&config, &paths, post)
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
        assert!(
            err.message.starts_with("/somewhere/config.toml:2:"),
            "{}",
            err.message
        );
        assert_eq!(err.code, 1);
        // A value outside the enum is located too.
        let err = parse("[export]\ntool_change_mode = \"both\"\n", path).unwrap_err();
        assert!(
            err.message.starts_with("/somewhere/config.toml:2:"),
            "{}",
            err.message
        );
        assert!(err.message.contains("separate_programs"), "{}", err.message);
    }

    /// A key DreamCarve does not know is a typo, never a silent default:
    /// the error names the key, at its line.
    #[test]
    fn unknown_key_is_error() {
        let path = Path::new("config.toml");
        let err = parse("[export]\ntool_change_mod = \"m6\"\n", path).unwrap_err();
        assert!(err.message.contains("tool_change_mod"), "{}", err.message);
        assert!(err.message.starts_with("config.toml:2:"), "{}", err.message);
        let err = parse("[unknown]\nkey = 1\n", path).unwrap_err();
        assert!(err.message.contains("unknown"), "{}", err.message);
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
            let err = parse(text, path).unwrap_err();
            assert!(
                err.message.starts_with("/somewhere/config.toml:2:"),
                "{text}: {}",
                err.message
            );
            assert!(err.message.contains("above 0"), "{text}: {}", err.message);
            let column: usize = err
                .message
                .trim_start_matches("/somewhere/config.toml:2:")
                .split(':')
                .next()
                .and_then(|c| c.parse().ok())
                .unwrap_or_else(|| panic!("{text}: no column in {}", err.message));
            assert_eq!(column, key.len() + 4, "{text}: at the value");
        }
        let err = parse("[estimate]\nrapid_rate = 3000\n", path).unwrap_err();
        assert!(err.message.contains("rapid_rate"), "{}", err.message);
    }

    /// `DREAMCARVE_CONFIG_DIR` replaces the platform directory, and the
    /// three paths hang off whichever directory is in force.
    #[test]
    fn env_override_wins() {
        let custom = PathBuf::from("/custom/dir");
        let paths = Paths::resolve(Some(custom.as_os_str())).unwrap();
        assert_eq!(paths.dir, custom);
        assert_eq!(paths.config_file, custom.join(CONFIG_FILE_NAME));
        assert_eq!(paths.library_file, custom.join(LIBRARY_FILE_NAME));
        let platform = Paths::resolve(None).unwrap();
        assert_ne!(platform.dir, custom);
        assert!(
            platform.dir.ends_with("dreamcarve"),
            "{}",
            platform.dir.display()
        );
    }
}
