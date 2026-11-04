// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The staged write (#71): every file of an export is written in full to a
//! temporary file beside its destination, then all are renamed into place
//! at the end, or none of them. Moved from `crates/dreamcarve/src/cli.rs`
//! (`docs/specs/phase-4/design.md`, "Module layout") so the app writes as
//! the CLI does.

use std::ffi::OsString;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::ShellError;

/// How many temporary names [`StagedFile::write`] tries before giving up.
const TEMP_ATTEMPTS: u32 = 64;

/// A file ready to be moved into place: written in full to a temporary
/// file beside its destination, so a failure never leaves a partial file
/// and the final [`StagedFile::commit`] is a rename.
///
/// The temporary file is created with `create_new` (`O_EXCL`), which fails
/// on any existing file or symlink instead of following it, under a name
/// with a random suffix. A file planted at a candidate name is therefore
/// never written through; the next candidate is tried instead.
#[derive(Debug)]
pub struct StagedFile {
    /// The temporary file; `None` once committed.
    temp: Option<PathBuf>,
    dest: PathBuf,
}

impl StagedFile {
    /// Writes `contents` to a new temporary file in `dest`'s directory.
    ///
    /// # Errors
    ///
    /// [`ShellError::NotAFilePath`] for a destination with no file name,
    /// [`ShellError::Write`] when the file system refuses, and
    /// [`ShellError::NoTemporaryName`] when every candidate name is taken.
    pub fn write(dest: &Path, contents: &[u8]) -> Result<Self, ShellError> {
        let name = dest.file_name().ok_or_else(|| ShellError::NotAFilePath {
            path: dest.to_path_buf(),
        })?;
        let candidates = (0..TEMP_ATTEMPTS).map(|_| {
            let mut temp_name = OsString::from(".");
            temp_name.push(name);
            temp_name.push(format!(".{:016x}.tmp", random_u64()));
            dest.with_file_name(temp_name)
        });
        Self::write_via(dest, contents, candidates)
    }

    /// [`StagedFile::write`] with the temporary names to try, in order.
    fn write_via(
        dest: &Path,
        contents: &[u8],
        candidates: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, ShellError> {
        let fail = |source: std::io::Error| ShellError::Write {
            path: dest.to_path_buf(),
            source,
        };
        for candidate in candidates {
            let mut file = match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(file) => file,
                // Someone else's file or symlink: leave it alone.
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(fail(e)),
            };
            // From here on the file is ours, and `Drop` removes it on failure.
            let staged = Self {
                temp: Some(candidate),
                dest: dest.to_path_buf(),
            };
            file.write_all(contents)
                .and_then(|()| file.sync_all())
                .map_err(fail)?;
            return Ok(staged);
        }
        Err(ShellError::NoTemporaryName {
            path: dest.to_path_buf(),
        })
    }

    /// Where the file goes once committed.
    #[must_use]
    pub fn destination(&self) -> &Path {
        &self.dest
    }

    /// Moves the file into place, replacing any file already there.
    ///
    /// # Errors
    ///
    /// [`ShellError::Write`] when the rename fails; the temporary file is
    /// then removed.
    pub fn commit(mut self) -> Result<(), ShellError> {
        let Some(temp) = self.temp.take() else {
            return Ok(());
        };
        match std::fs::rename(&temp, &self.dest) {
            Ok(()) => Ok(()),
            Err(source) => {
                // Not moved: put it back so `Drop` removes it.
                self.temp = Some(temp);
                Err(ShellError::Write {
                    path: self.dest.clone(),
                    source,
                })
            }
        }
    }
}

/// Moves every staged file into place, in order, or none of them: export
/// writes its programs and the project as one change (#71). Before each
/// rename, whatever is at the destination is kept under a temporary name
/// beside it, without reading or following it: a regular file by a second
/// hard link, so it stays in place with its mode, or (where the file
/// system has no hard links, and for a symlink) by moving it aside. A
/// symlink at a destination is replaced by the new file, as a rename
/// replaces it, and its target is never opened. If a rename fails, every
/// destination already changed is put back as it was, or removed where
/// there was nothing, before the error is returned.
///
/// Once every file is in place, each destination's directory is synced
/// (Unix), so the renames survive a power cut. That is best effort: the
/// files are already written, and failing then would say they were not.
///
/// # Errors
///
/// The error of the first rename, or backup, that failed.
pub fn commit_all(staged: Vec<StagedFile>) -> Result<(), ShellError> {
    let mut moved: Vec<(PathBuf, Option<Backup>)> = Vec::with_capacity(staged.len());
    for file in staged {
        let dest = file.dest.clone();
        let result = Backup::take(&dest).and_then(|backup| match file.commit() {
            Ok(()) => Ok(backup),
            Err(e) => {
                if let Some(backup) = backup {
                    backup.undo_unchanged();
                }
                Err(e)
            }
        });
        match result {
            Ok(backup) => moved.push((dest, backup)),
            Err(e) => {
                // Undoing is best effort; the error reported is the step
                // that failed.
                for (dest, backup) in moved.into_iter().rev() {
                    match backup {
                        Some(backup) => backup.restore(),
                        None => {
                            let _ = std::fs::remove_file(&dest);
                        }
                    }
                }
                return Err(e);
            }
        }
    }
    for (dest, backup) in moved {
        sync_parent(&dest);
        if let Some(backup) = backup {
            backup.discard();
        }
    }
    Ok(())
}

/// What was at an export destination before [`commit_all`] replaced it,
/// kept under a temporary name in the same directory.
struct Backup {
    /// The temporary name.
    path: PathBuf,
    /// The destination it is put back to.
    dest: PathBuf,
    /// Whether it was moved away from the destination; otherwise it is a
    /// second hard link and the destination is still there.
    moved_aside: bool,
}

impl Backup {
    /// Keeps what is at `dest`: `None` when there is nothing, or a
    /// directory, which no file can replace, so the rename fails and
    /// nothing needs keeping. Never reads `dest` or follows it.
    fn take(dest: &Path) -> Result<Option<Self>, ShellError> {
        let fail = |source: std::io::Error| ShellError::Write {
            path: dest.to_path_buf(),
            source,
        };
        let meta = match std::fs::symlink_metadata(dest) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(fail(e)),
        };
        if meta.is_dir() {
            return Ok(None);
        }
        let name = dest.file_name().ok_or_else(|| ShellError::NotAFilePath {
            path: dest.to_path_buf(),
        })?;
        let mut linkable = meta.file_type().is_file();
        for _ in 0..TEMP_ATTEMPTS {
            let mut backup_name = OsString::from(".");
            backup_name.push(name);
            backup_name.push(format!(".{:016x}.bak", random_u64()));
            let path = dest.with_file_name(backup_name);
            if linkable {
                // A second name for the same file: `dest` stays in place,
                // and the rename over it leaves this one as it was.
                match std::fs::hard_link(dest, &path) {
                    Ok(()) => {
                        return Ok(Some(Self {
                            path,
                            dest: dest.to_path_buf(),
                            moved_aside: false,
                        }));
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                    // No hard links here: move it aside instead.
                    Err(_) => linkable = false,
                }
            }
            // Claim the name with `create_new`, which never follows a
            // planted file or link, then move `dest` over our own file.
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(fail(e)),
            }
            if let Err(e) = std::fs::rename(dest, &path) {
                let _ = std::fs::remove_file(&path);
                return Err(fail(e));
            }
            return Ok(Some(Self {
                path,
                dest: dest.to_path_buf(),
                moved_aside: true,
            }));
        }
        Err(ShellError::NoTemporaryName {
            path: dest.to_path_buf(),
        })
    }

    /// Puts the kept file back at its destination. Best effort: if the
    /// rename fails the file stays under its temporary name, not lost.
    fn restore(self) {
        let _ = std::fs::rename(&self.path, &self.dest);
    }

    /// Undoes [`Backup::take`] when the destination was never replaced. A
    /// rename between two links of one file does nothing (POSIX
    /// `rename`), so a linked copy is dropped instead.
    fn undo_unchanged(self) {
        if self.moved_aside {
            self.restore();
        } else {
            self.discard();
        }
    }

    /// Drops the kept file once the export is in place.
    fn discard(self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Syncs the directory holding `path`, so a rename into it is on disk.
fn sync_parent(path: &Path) {
    #[cfg(unix)]
    {
        let dir = match path.parent() {
            Some(dir) if !dir.as_os_str().is_empty() => dir,
            _ => Path::new("."),
        };
        if let Ok(dir) = std::fs::File::open(dir) {
            let _ = dir.sync_all();
        }
    }
    #[cfg(not(unix))]
    let _ = path;
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        // Removes the temporary file when the stage is abandoned.
        if let Some(temp) = &self.temp {
            let _ = std::fs::remove_file(temp);
        }
    }
}

/// 64 unpredictable bits from std's randomly keyed hasher, mixed with the
/// clock, process id, and a counter. Not cryptographic: `create_new` is
/// what makes the temporary file safe; this only makes collisions rare.
fn random_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    hasher.write_u32(std::process::id());
    if let Ok(now) = SystemTime::now().duration_since(UNIX_EPOCH) {
        hasher.write_u128(now.as_nanos());
    }
    hasher.finish()
}

/// The CLI's tests of the staged write, moved here unchanged.
#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty directory for test `name`.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dc-cli-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn staged_file_is_removed_when_not_committed() {
        let dir = scratch("abandon");
        let dest = dir.join("out.nc");
        drop(StagedFile::write(&dest, b"G21\n").unwrap());
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nothing left");
        StagedFile::write(&dest, b"G21\n")
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"G21\n");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "only out.nc");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A file already at a candidate temporary name (planted, or left by
    /// another process) is skipped: never truncated, written, or removed.
    #[test]
    fn staged_file_never_writes_through_an_existing_file() {
        let dir = scratch("planted-file");
        let planted = dir.join(".out.nc.planted.tmp");
        std::fs::write(&planted, b"theirs").unwrap();
        let dest = dir.join("out.nc");
        let next = dir.join(".out.nc.next.tmp");
        let staged = StagedFile::write_via(&dest, b"ours", [planted.clone(), next]).unwrap();
        assert_eq!(std::fs::read(&planted).unwrap(), b"theirs", "untouched");
        staged.commit().unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"ours");
        assert_eq!(std::fs::read(&planted).unwrap(), b"theirs", "kept");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A symlink at a candidate temporary name is not followed: its target
    /// is never created or written. Unix only, because creating a symlink
    /// on Windows needs a privilege CI runners may not have.
    #[cfg(unix)]
    #[test]
    fn staged_file_never_writes_through_a_symlink() {
        let dir = scratch("planted-link");
        let target = dir.join("victim");
        let link = dir.join(".out.nc.link.tmp");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let dest = dir.join("out.nc");
        let next = dir.join(".out.nc.next.tmp");
        StagedFile::write_via(&dest, b"ours", [link.clone(), next])
            .unwrap()
            .commit()
            .unwrap();
        assert!(!target.exists(), "the symlink target was created");
        assert!(
            std::fs::symlink_metadata(&link).unwrap().is_symlink(),
            "link kept"
        );
        assert_eq!(std::fs::read(&dest).unwrap(), b"ours");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// The names in `dir`, sorted.
    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// #71: export moves every staged file into place or none. When a
    /// later rename fails (here its destination is a directory, which no
    /// file can replace), the files already moved are put back as they
    /// were, a file that did not exist is gone again, and no temporary
    /// file is left behind.
    #[test]
    fn failed_second_rename_leaves_no_partial_export() {
        let dir = scratch("all-or-none");
        let replaced = dir.join("coaster.nc");
        std::fs::write(&replaced, b"old program\n").unwrap();
        let created = dir.join("coaster-2-T2.nc");
        let blocked = dir.join("coaster.dcv");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("inside"), b"x").unwrap();
        let staged = vec![
            StagedFile::write(&replaced, b"new program\n").unwrap(),
            StagedFile::write(&created, b"second program\n").unwrap(),
            StagedFile::write(&blocked, b"project\n").unwrap(),
        ];
        assert!(commit_all(staged).is_err(), "the last rename must fail");
        assert_eq!(std::fs::read(&replaced).unwrap(), b"old program\n");
        assert!(!created.exists(), "a new file was left in place");
        assert_eq!(names_in(&dir), ["coaster.dcv", "coaster.nc"]);

        // Without the obstacle every file moves, and nothing else is left.
        std::fs::remove_dir_all(&blocked).unwrap();
        let staged = vec![
            StagedFile::write(&replaced, b"new program\n").unwrap(),
            StagedFile::write(&created, b"second program\n").unwrap(),
            StagedFile::write(&blocked, b"project\n").unwrap(),
        ];
        commit_all(staged).unwrap();
        assert_eq!(std::fs::read(&replaced).unwrap(), b"new program\n");
        assert_eq!(std::fs::read(&created).unwrap(), b"second program\n");
        assert_eq!(std::fs::read(&blocked).unwrap(), b"project\n");
        assert_eq!(
            names_in(&dir),
            ["coaster-2-T2.nc", "coaster.dcv", "coaster.nc"]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A file put back after a failed export is the same file: its mode
    /// is kept, not reset to a new file's default (#71).
    #[cfg(unix)]
    #[test]
    fn failed_export_restores_the_original_file_and_mode() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = scratch("restore-mode");
        let replaced = dir.join("coaster.nc");
        std::fs::write(&replaced, b"old program\n").unwrap();
        std::fs::set_permissions(&replaced, std::fs::Permissions::from_mode(0o640)).unwrap();
        let blocked = dir.join("coaster.dcv");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("inside"), b"x").unwrap();
        let staged = vec![
            StagedFile::write(&replaced, b"new program\n").unwrap(),
            StagedFile::write(&blocked, b"project\n").unwrap(),
        ];
        assert!(commit_all(staged).is_err());
        assert_eq!(std::fs::read(&replaced).unwrap(), b"old program\n");
        let mode = std::fs::metadata(&replaced).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640, "mode {mode:o}");
        assert_eq!(names_in(&dir), ["coaster.dcv", "coaster.nc"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Security review of #71: a symlink at an output path is never
    /// followed. Export replaces the link itself, as a rename does; a
    /// failed export puts the link back. Either way the link's target is
    /// not read, copied, or written, so its contents reach no new file.
    /// Unix only, as creating a symlink on Windows needs a privilege.
    #[cfg(unix)]
    #[test]
    fn export_does_not_follow_a_symlinked_destination() {
        let dir = scratch("linked-dest");
        let target = dir.join("secret");
        std::fs::write(&target, b"secret\n").unwrap();
        let link = dir.join("coaster.nc");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let blocked = dir.join("coaster.dcv");
        std::fs::create_dir(&blocked).unwrap();
        std::fs::write(blocked.join("inside"), b"x").unwrap();
        let no_copy_of_target = |dir: &Path| {
            for name in names_in(dir).iter().filter(|n| *n != "secret") {
                let path = dir.join(name);
                if std::fs::symlink_metadata(&path).unwrap().is_file() {
                    assert_ne!(std::fs::read(&path).unwrap(), b"secret\n", "{name}");
                }
            }
        };

        let staged = vec![
            StagedFile::write(&link, b"new program\n").unwrap(),
            StagedFile::write(&blocked, b"project\n").unwrap(),
        ];
        assert!(commit_all(staged).is_err());
        assert!(
            std::fs::symlink_metadata(&link).unwrap().is_symlink(),
            "link put back"
        );
        assert_eq!(std::fs::read_link(&link).unwrap(), target);
        assert_eq!(std::fs::read(&target).unwrap(), b"secret\n");
        assert_eq!(names_in(&dir), ["coaster.dcv", "coaster.nc", "secret"]);
        no_copy_of_target(&dir);

        std::fs::remove_dir_all(&blocked).unwrap();
        let staged = vec![StagedFile::write(&link, b"new program\n").unwrap()];
        commit_all(staged).unwrap();
        assert!(
            std::fs::symlink_metadata(&link).unwrap().is_file(),
            "link replaced"
        );
        assert_eq!(std::fs::read(&link).unwrap(), b"new program\n");
        assert_eq!(std::fs::read(&target).unwrap(), b"secret\n");
        assert_eq!(names_in(&dir), ["coaster.nc", "secret"]);
        no_copy_of_target(&dir);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn staged_file_fails_when_every_name_is_taken() {
        let dir = scratch("all-taken");
        let taken = dir.join(".out.nc.taken.tmp");
        std::fs::write(&taken, b"theirs").unwrap();
        let result = StagedFile::write_via(&dir.join("out.nc"), b"ours", [taken.clone()]);
        assert!(result.is_err(), "no free name must be an error");
        assert_eq!(std::fs::read(&taken).unwrap(), b"theirs");
        assert!(!dir.join("out.nc").exists(), "nothing written");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
