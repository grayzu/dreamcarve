// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The user config file and directory (`docs/specs/phase-2/design.md`,
//! "User config (CLI only)") and `dreamcarve config path`.

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
