// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The tool library file, `<config dir>/tools.json` (`docs/specs/phase-2/
//! design.md`, "Algorithms → Tool library CLI"; `docs/specs/phase-4/
//! functional.md`, capability 64). Moved from the `dreamcarve` CLI's
//! `tools` command so the app reads and writes the same file the same
//! way; `dc-library` parses and edits it in memory. A missing file is no
//! library yet ([`read`] gives `None`); the first write creates it and its
//! directory. Every write is staged and renamed into place, so a failure
//! leaves the old file.

use std::path::Path;

use dc_library::Library;

use crate::config_dir::Paths;
use crate::error::ShellError;
use crate::read::{MAX_FILE_BYTES, read_capped};
use crate::staged::StagedFile;

/// The library at `paths`, or `None` when its file does not exist.
///
/// # Errors
///
/// [`ShellError::Read`] when the file cannot be read,
/// [`ShellError::FileTooLarge`] when it is past [`MAX_FILE_BYTES`], and
/// [`ShellError::LoadLibrary`] when it is not a library.
pub fn read(paths: &Paths) -> Result<Option<Library>, ShellError> {
    let path = &paths.library_file;
    let bytes = match read_capped(path, MAX_FILE_BYTES) {
        Ok(bytes) => bytes,
        Err(ShellError::Read { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    dc_library::load(&bytes)
        .map(Some)
        .map_err(|source| ShellError::LoadLibrary {
            path: path.clone(),
            source: Box::new(source),
        })
}

/// Writes `lib` as the library file at `paths`, creating its directory if
/// needed.
///
/// # Errors
///
/// [`ShellError::SaveLibrary`] when the library cannot be serialized,
/// [`ShellError::Create`] when the directory cannot be made, and the
/// staged write's errors.
pub fn write(paths: &Paths, lib: &Library) -> Result<(), ShellError> {
    let text = dc_library::save(lib).map_err(|source| ShellError::SaveLibrary {
        source: Box::new(source),
    })?;
    std::fs::create_dir_all(&paths.dir).map_err(|source| ShellError::Create {
        path: paths.dir.clone(),
        source,
    })?;
    StagedFile::write(&paths.library_file, text.as_bytes())?.commit()
}

/// Writes `lib` as DreamCarve library JSON to `file` (`tools export`): the
/// library file's bytes, anywhere.
///
/// # Errors
///
/// [`ShellError::SaveLibrary`] when the library cannot be serialized, and
/// the staged write's errors.
pub fn export(lib: &Library, file: &Path) -> Result<(), ShellError> {
    let text = dc_library::save(lib).map_err(|source| ShellError::SaveLibrary {
        source: Box::new(source),
    })?;
    StagedFile::write(file, text.as_bytes())?.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::test_support::tool_6_35_upcut;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dreamcarve-shell-library-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// No file is no library; a write creates the directory and the file,
    /// which reads back equal and exports byte for byte.
    #[test]
    fn missing_is_none_and_a_write_round_trips() {
        let dir = scratch("round-trip");
        let paths = Paths::in_dir(dir.join("config"));
        assert_eq!(read(&paths).unwrap(), None);
        let mut lib = Library::default();
        dc_library::add(&mut lib, tool_6_35_upcut(), None).unwrap();
        write(&paths, &lib).unwrap();
        assert_eq!(read(&paths).unwrap(), Some(lib.clone()));
        let out = dir.join("out.json");
        export(&lib, &out).unwrap();
        assert_eq!(
            std::fs::read(&out).unwrap(),
            std::fs::read(&paths.library_file).unwrap()
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Review 1.6: a library file past the read limit is refused by name
    /// and size before it is read (a sparse file, which costs no disk).
    #[test]
    fn a_library_file_past_the_read_limit_is_refused() {
        let dir = scratch("too-large");
        let paths = Paths::in_dir(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        let file = std::fs::File::create(&paths.library_file).unwrap();
        file.set_len(crate::MAX_FILE_BYTES + 1).unwrap();
        drop(file);
        let error = read(&paths).unwrap_err();
        assert!(
            matches!(
                &error,
                ShellError::FileTooLarge { path, limit_bytes: crate::MAX_FILE_BYTES, .. }
                    if *path == paths.library_file
            ),
            "{error:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A file that is not a library is an error naming it, as `tools`
    /// printed it before the move.
    #[test]
    fn a_broken_file_names_itself() {
        let dir = scratch("broken");
        let paths = Paths::in_dir(dir.clone());
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&paths.library_file, b"not json").unwrap();
        let error = read(&paths).unwrap_err().to_string();
        assert!(
            error.starts_with(&format!("cannot load {}: ", paths.library_file.display())),
            "{error}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
