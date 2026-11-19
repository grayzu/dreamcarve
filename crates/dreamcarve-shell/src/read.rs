// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The capped read (review 1.6): every file a DreamCarve binary reads
//! whole goes through [`read_capped`], so a file of any size, such as a
//! disk image given by mistake or an endless device, is refused by name
//! instead of being read into memory until the process dies.

use std::io::Read as _;
use std::path::Path;

use crate::error::ShellError;

/// The largest file DreamCarve reads into memory, in bytes: 256 MiB. A
/// drawing, a project, a picture or a tool library is far smaller (the
/// importers' own limits stop a drawing of a few megabytes, and a project
/// embeds at most a few pictures); the limit only has to sit well above
/// any of them and well below the memory of the machine.
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;

/// The bytes of the file at `path`, which may hold at most `limit_bytes`.
/// The size is checked before anything is read, and the read itself stops
/// one byte past the limit, so a file that grows meanwhile, or one whose
/// size is not known ahead (a pipe, a device), is refused too.
///
/// # Errors
///
/// [`ShellError::Read`] when the file cannot be opened or read, and
/// [`ShellError::FileTooLarge`] when it holds more than `limit_bytes`.
pub fn read_capped(path: &Path, limit_bytes: u64) -> Result<Vec<u8>, ShellError> {
    let fail = |source: std::io::Error| ShellError::Read {
        path: path.to_path_buf(),
        source,
    };
    let too_large = |size_bytes: u64| ShellError::FileTooLarge {
        path: path.to_path_buf(),
        size_bytes,
        limit_bytes,
    };
    let file = std::fs::File::open(path).map_err(fail)?;
    let size_bytes = file.metadata().map_err(fail)?.len();
    if size_bytes > limit_bytes {
        return Err(too_large(size_bytes));
    }
    // The size is a hint for the buffer only; the reader is what stops.
    let mut bytes = Vec::with_capacity(usize::try_from(size_bytes).unwrap_or(0));
    let read = file
        .take(limit_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(fail)?;
    match u64::try_from(read) {
        Ok(read) if read <= limit_bytes => Ok(bytes),
        Ok(read) => Err(too_large(read)),
        Err(_) => Err(too_large(u64::MAX)),
    }
}

/// [`read_capped`] for a text file, which must be UTF-8.
///
/// # Errors
///
/// As [`read_capped`]; text that is not UTF-8 is a [`ShellError::Read`]
/// that says so, as `std::fs::read_to_string` reports it.
pub fn read_capped_text(path: &Path, limit_bytes: u64) -> Result<String, ShellError> {
    String::from_utf8(read_capped(path, limit_bytes)?).map_err(|_| ShellError::Read {
        path: path.to_path_buf(),
        source: std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// A fresh, empty directory for test `name`.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dreamcarve-shell-read-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A file of exactly the limit reads whole; one byte more is refused
    /// with an error that names the file, its size and the limit.
    #[test]
    fn a_file_past_the_limit_is_refused_by_name_size_and_limit() {
        let dir = scratch("limit");
        let path = dir.join("drawing.dxf");
        std::fs::write(&path, b"0123456789").unwrap();
        assert_eq!(read_capped(&path, 10).unwrap(), b"0123456789");

        let error = read_capped(&path, 9).unwrap_err();
        assert!(
            matches!(
                &error,
                ShellError::FileTooLarge { path: p, size_bytes: 10, limit_bytes: 9 } if *p == path
            ),
            "{error:?}"
        );
        assert_eq!(
            error.to_string(),
            format!(
                "cannot read {}: it is 10 bytes, more than the limit of 9 bytes",
                path.display()
            )
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The size is checked before the file is read: a sparse file one
    /// byte past [`MAX_FILE_BYTES`], which costs no disk, is refused
    /// without a quarter of a gigabyte being read.
    #[test]
    fn a_file_past_the_built_in_limit_is_refused_unread() {
        let dir = scratch("sparse");
        let path = dir.join("huge.svg");
        let file = std::fs::File::create(&path).unwrap();
        file.set_len(MAX_FILE_BYTES + 1).unwrap();
        drop(file);
        let error = read_capped(&path, MAX_FILE_BYTES).unwrap_err();
        assert!(
            matches!(
                error,
                ShellError::FileTooLarge {
                    size_bytes,
                    limit_bytes: MAX_FILE_BYTES,
                    ..
                } if size_bytes == MAX_FILE_BYTES + 1
            ),
            "{error:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A device has no size to check, and never ends: the read stops one
    /// byte past the limit and the file is refused. Unix only, for
    /// `/dev/zero`.
    #[cfg(unix)]
    #[test]
    fn an_endless_file_is_refused_at_the_limit() {
        let error = read_capped(Path::new("/dev/zero"), 4096).unwrap_err();
        assert!(
            matches!(
                error,
                ShellError::FileTooLarge {
                    size_bytes: 4097,
                    limit_bytes: 4096,
                    ..
                }
            ),
            "{error:?}"
        );
    }

    /// A file that is not there is the read error it always was, with the
    /// OS's reason, so callers can still tell a missing file apart.
    #[test]
    fn a_missing_file_is_a_read_error() {
        let dir = scratch("missing");
        let path = dir.join("nothing.dcv");
        let error = read_capped(&path, MAX_FILE_BYTES).unwrap_err();
        assert!(
            matches!(
                &error,
                ShellError::Read { source, .. } if source.kind() == std::io::ErrorKind::NotFound
            ),
            "{error:?}"
        );
        assert!(
            error
                .to_string()
                .starts_with(&format!("cannot read {}: ", path.display())),
            "{error}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Text is read under the same limit, and must be UTF-8.
    #[test]
    fn text_is_capped_and_must_be_utf8() {
        let dir = scratch("text");
        let path = dir.join("config.toml");
        std::fs::write(&path, "a = \"é\"\n").unwrap();
        assert_eq!(read_capped_text(&path, 64).unwrap(), "a = \"é\"\n");
        assert!(matches!(
            read_capped_text(&path, 4),
            Err(ShellError::FileTooLarge { .. })
        ));
        std::fs::write(&path, b"a = \"\xff\"\n").unwrap();
        let error = read_capped_text(&path, 64).unwrap_err();
        assert!(
            matches!(
                &error,
                ShellError::Read { source, .. }
                    if source.kind() == std::io::ErrorKind::InvalidData
            ),
            "{error:?}"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
