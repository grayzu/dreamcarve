// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The user config directory (`docs/specs/phase-2/design.md`, "User config
//! (CLI only)"), moved from `crates/dreamcarve/src/config.rs`. It holds
//! `config.toml` ([`crate::config`]), `tools.json` (the tool library) and
//! the app's `gui.toml` ([`crate::gui_prefs`]). `<config dir>` is
//! `DREAMCARVE_CONFIG_DIR` when that is set; otherwise the platform's
//! usual place, read from the environment with std alone (no crate for
//! it: the `directories` crate would bring in an MPL-2.0 dependency):
//! macOS `$HOME/Library/Application Support/dreamcarve`, Windows
//! `%APPDATA%\dreamcarve`, Linux and everything else
//! `$XDG_CONFIG_HOME/dreamcarve`, or `$HOME/.config/dreamcarve` when
//! `XDG_CONFIG_HOME` is unset. A missing `HOME` or `APPDATA` is an error
//! naming the variable.

use std::ffi::OsStr;
use std::path::PathBuf;

use crate::error::ShellError;

/// The environment variable that overrides the config directory.
pub const CONFIG_DIR_VAR: &str = "DREAMCARVE_CONFIG_DIR";
/// The config file's name in the config directory.
pub const CONFIG_FILE_NAME: &str = "config.toml";
/// The tool library's name in the config directory.
pub const LIBRARY_FILE_NAME: &str = "tools.json";
/// The app's preferences file's name in the config directory (functional
/// spec, open question 11).
pub const GUI_PREFS_FILE_NAME: &str = "gui.toml";

/// Where the config directory and its files are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// The config directory.
    pub dir: PathBuf,
    /// `config.toml` in it.
    pub config_file: PathBuf,
    /// `tools.json` in it.
    pub library_file: PathBuf,
    /// `gui.toml` in it.
    pub gui_prefs_file: PathBuf,
}

impl Paths {
    /// The paths under config directory `dir`.
    #[must_use]
    pub fn in_dir(dir: PathBuf) -> Self {
        let config_file = dir.join(CONFIG_FILE_NAME);
        let library_file = dir.join(LIBRARY_FILE_NAME);
        let gui_prefs_file = dir.join(GUI_PREFS_FILE_NAME);
        Self {
            dir,
            config_file,
            library_file,
            gui_prefs_file,
        }
    }

    /// The paths for this run: under `override_dir` (the value of
    /// `DREAMCARVE_CONFIG_DIR`) when it is set and not empty, else under
    /// the platform's config directory for `dreamcarve`.
    ///
    /// # Errors
    ///
    /// [`ShellError::ConfigDirUnset`] when the platform's base variable is
    /// unset or empty.
    pub fn resolve(override_dir: Option<&OsStr>) -> Result<Self, ShellError> {
        match override_dir {
            Some(dir) if !dir.is_empty() => Ok(Self::in_dir(PathBuf::from(dir))),
            _ => platform_dir().map(Self::in_dir),
        }
    }

    /// [`Paths::resolve`] from the process environment.
    ///
    /// # Errors
    ///
    /// As [`Paths::resolve`].
    pub fn from_env() -> Result<Self, ShellError> {
        Self::resolve(std::env::var_os(CONFIG_DIR_VAR).as_deref())
    }
}

/// The directory name under the platform's config directory.
const APP_DIR_NAME: &str = "dreamcarve";

/// The platform's config directory for DreamCarve, from the environment
/// (module doc). An unset or empty base variable is an error naming it.
fn platform_dir() -> Result<PathBuf, ShellError> {
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
fn base_dir(var: &'static str) -> Result<PathBuf, ShellError> {
    env_path(var).ok_or(ShellError::ConfigDirUnset { var })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `DREAMCARVE_CONFIG_DIR` replaces the platform directory, and the
    /// four paths hang off whichever directory is in force.
    #[test]
    fn env_override_wins() {
        let custom = PathBuf::from("/custom/dir");
        let paths = Paths::resolve(Some(custom.as_os_str())).unwrap();
        assert_eq!(paths.dir, custom);
        assert_eq!(paths.config_file, custom.join(CONFIG_FILE_NAME));
        assert_eq!(paths.library_file, custom.join(LIBRARY_FILE_NAME));
        assert_eq!(paths.gui_prefs_file, custom.join(GUI_PREFS_FILE_NAME));
        let platform = Paths::resolve(None).unwrap();
        assert_ne!(platform.dir, custom);
        assert!(
            platform.dir.ends_with("dreamcarve"),
            "{}",
            platform.dir.display()
        );
    }
}
