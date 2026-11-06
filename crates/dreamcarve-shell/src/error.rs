// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The crate's error type. Each message is one line, as the `dreamcarve`
//! CLI printed it before the move, so its output is unchanged: a shell
//! prints `error: ` and the message.

use std::path::PathBuf;

use dc_library::LibraryError;
use dc_project::PostProcessorError;

use crate::config_dir::CONFIG_DIR_VAR;

/// Why a shell step failed.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// A file could not be read.
    #[error("cannot read {}: {source}", path.display())]
    Read {
        /// The file.
        path: PathBuf,
        /// What the file system said.
        source: std::io::Error,
    },
    /// A file could not be written, or moved into place.
    #[error("cannot write {}: {source}", path.display())]
    Write {
        /// The file's destination.
        path: PathBuf,
        /// What the file system said.
        source: std::io::Error,
    },
    /// A directory could not be created.
    #[error("cannot create {}: {source}", path.display())]
    Create {
        /// The directory.
        path: PathBuf,
        /// What the file system said.
        source: std::io::Error,
    },
    /// A destination with no file name, such as `/` or `..`.
    #[error("{} is not a file path", path.display())]
    NotAFilePath {
        /// The path given.
        path: PathBuf,
    },
    /// Every temporary name beside the destination was taken.
    #[error("cannot write {}: no free temporary file name in its directory", path.display())]
    NoTemporaryName {
        /// The file's destination.
        path: PathBuf,
    },
    /// The platform's config directory cannot be found: its base
    /// environment variable is unset or empty.
    #[error(
        "cannot find the user config directory: {var} is not set; set it, or set {CONFIG_DIR_VAR} \
         to the directory to use"
    )]
    ConfigDirUnset {
        /// `HOME` or `APPDATA`.
        var: &'static str,
    },
    /// A config file does not parse, or has a key DreamCarve does not
    /// know, at `line:column` when the parser can say where (#76).
    #[error("{}{}: {message}", path.display(), at.map_or_else(String::new, |(line, column)| format!(":{line}:{column}")))]
    Config {
        /// The file.
        path: PathBuf,
        /// The 1-based line and column of the problem, when known.
        at: Option<(usize, usize)>,
        /// What the parser said.
        message: String,
    },
    /// `config.toml` asks for `m6` tool changes of a post-processor whose
    /// tool change has no `M6` (GRBL).
    #[error(
        "{} sets tool_change_mode = \"m6\", but post-processor `{post}` has no M6 tool change; \
         set \"separate_programs\" there or choose another post-processor",
        config.display()
    )]
    M6Unavailable {
        /// The config file.
        config: PathBuf,
        /// The post-processor's id.
        post: String,
    },
    /// The tool library file is not a library.
    #[error("cannot load {}: {source}", path.display())]
    LoadLibrary {
        /// The library file.
        path: PathBuf,
        /// Why it does not load.
        source: LibraryError,
    },
    /// A library could not be serialized.
    #[error("{source}")]
    SaveLibrary {
        /// The serializer's error.
        source: LibraryError,
    },
    /// The post-processor the config applies to could not be resolved.
    #[error(transparent)]
    Post(#[from] PostProcessorError),
}
