// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The user's fonts folder (phase 5 functional spec, capability 91):
//! `fonts/` in the config directory. *Add font…* copies a `.ttf` or
//! `.otf` file there, and the app reads the folder when it starts. The
//! app never lists the system's fonts; what is in this folder is what
//! the user put there.
//!
//! Whether a file is a font the app can read is `dc-text`'s to say, from
//! its bytes; this module lists, reads and copies files.

use std::path::{Path, PathBuf};

use crate::config_dir::Paths;
use crate::error::ShellError;
use crate::read::read_capped;

/// The folder's name in the config directory.
pub const FONTS_DIR_NAME: &str = "fonts";

/// The most a font file may hold, bytes: a large font with every script
/// is a few tens of megabytes.
pub const MAX_FONT_BYTES: u64 = 64 * 1024 * 1024;

/// The user's fonts folder under `paths`. It may not be there yet.
#[must_use]
pub fn folder(paths: &Paths) -> PathBuf {
    paths.dir.join(FONTS_DIR_NAME)
}

/// Whether `path` is named as a font file is: `.ttf` or `.otf`, in any
/// case.
#[must_use]
pub fn is_font_name(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("ttf") || extension.eq_ignore_ascii_case("otf")
        })
}

/// The font files in the user's fonts folder, by name. None when the
/// folder is not there or cannot be read: the app starts with the
/// bundled fonts alone.
#[must_use]
pub fn list(paths: &Paths) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(folder(paths)) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_font_name(path))
        .collect();
    files.sort();
    files
}

/// The bytes of the font file at `path`.
///
/// # Errors
///
/// [`ShellError::Read`], or [`ShellError::FileTooLarge`] past
/// [`MAX_FONT_BYTES`].
pub fn read(path: &Path) -> Result<Vec<u8>, ShellError> {
    read_capped(path, MAX_FONT_BYTES)
}

/// Writes `bytes`, a font file named `file_name`, to the user's fonts
/// folder, making the folder when it is not there, and says where. A
/// file of that name already there is replaced: it is the same font,
/// added again.
///
/// # Errors
///
/// [`ShellError::Write`] when the folder or the file cannot be written.
pub fn add(paths: &Paths, file_name: &str, bytes: &[u8]) -> Result<PathBuf, ShellError> {
    let dir = folder(paths);
    // Only the name: a path in it would write outside the folder.
    let name = Path::new(file_name)
        .file_name()
        .map_or_else(|| PathBuf::from("font.ttf"), PathBuf::from);
    let path = dir.join(name);
    let failed = |source| ShellError::Write {
        path: path.clone(),
        source,
    };
    std::fs::create_dir_all(&dir).map_err(failed)?;
    std::fs::write(&path, bytes).map_err(failed)?;
    Ok(path)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code reports failure by panicking"
)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> Paths {
        let dir =
            std::env::temp_dir().join(format!("dc-shell-fonts-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Paths::in_dir(dir)
    }

    /// A folder that is not there lists nothing; a font added is copied
    /// into it under its own name and listed, with the other font files
    /// and nothing else.
    #[test]
    fn a_font_added_is_copied_and_listed() {
        let paths = scratch("add");
        assert!(list(&paths).is_empty(), "no folder yet");
        let added = add(&paths, "Signs-Bold.ttf", b"font bytes").unwrap();
        assert_eq!(added, folder(&paths).join("Signs-Bold.ttf"));
        assert_eq!(read(&added).unwrap(), b"font bytes");
        add(&paths, "another.OTF", b"more").unwrap();
        std::fs::write(folder(&paths).join("notes.txt"), b"not a font").unwrap();
        std::fs::create_dir_all(folder(&paths).join("old.ttf")).unwrap();
        let names: Vec<String> = list(&paths)
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Signs-Bold.ttf", "another.OTF"]);
        // Added again, it is replaced, not doubled.
        add(&paths, "Signs-Bold.ttf", b"newer").unwrap();
        assert_eq!(read(&added).unwrap(), b"newer");
        assert_eq!(list(&paths).len(), 2);
        std::fs::remove_dir_all(&paths.dir).unwrap();
    }

    /// A name that carries a path is written by its last part, inside
    /// the folder.
    #[test]
    fn a_name_with_a_path_stays_in_the_folder() {
        let paths = scratch("escape");
        let added = add(&paths, "../../elsewhere/x.ttf", b"x").unwrap();
        assert_eq!(added, folder(&paths).join("x.ttf"));
        assert!(is_font_name(Path::new("a/B.TtF")));
        assert!(!is_font_name(Path::new("a/B.woff")));
        std::fs::remove_dir_all(&paths.dir).unwrap();
    }
}
