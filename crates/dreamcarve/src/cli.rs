// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Argument parsing, exit codes, and the file and clock I/O the commands
//! share (`docs/specs/phase-1/design.md`, "CLI (dreamcarve)").
//!
//! Exit codes: 0 success, 1 a validation, import, export, or file error,
//! 2 a usage error (clap's own code for bad arguments, and an unknown
//! post-processor id).

use std::ffi::OsString;
use std::fmt;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Parser, Subcommand, ValueEnum};

use crate::{check, clock, export, init};

/// DreamCarve: open source CAD/CAM for CNC hobbyists.
#[derive(Debug, Parser)]
#[command(version = env!("DREAMCARVE_VERSION"), about, arg_required_else_help = true)]
pub struct Cli {
    /// What to do.
    #[command(subcommand)]
    pub command: Command,
}

/// The subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a project (.dcv) from an SVG drawing.
    Init(InitArgs),
    /// Write G-code for a project.
    Export(ExportArgs),
    /// Report the problems export would find, without writing G-code.
    Check(CheckArgs),
    /// List the available post-processors.
    Posts,
}

/// Arguments of `init`.
#[derive(Debug, clap::Args)]
pub struct InitArgs {
    /// The SVG drawing to import.
    pub svg: PathBuf,
    /// Where to write the project [default: the SVG's name with .dcv].
    #[arg(short, long, value_name = "DCV")]
    pub output: Option<PathBuf>,
    /// Post-processor to embed (see `dreamcarve posts`).
    #[arg(long, value_name = "ID", default_value = "grblhal")]
    pub post: String,
    /// Display units of the project; G-code is written in these units.
    #[arg(long, value_enum, default_value_t = UnitsArg::Mm)]
    pub units: UnitsArg,
}

/// Arguments of `export`.
#[derive(Debug, clap::Args)]
pub struct ExportArgs {
    /// The project to export.
    pub dcv: PathBuf,
    /// Where to write the program [default: the project's name with the
    /// post-processor's extension].
    #[arg(short, long, value_name = "NC")]
    pub output: Option<PathBuf>,
    /// Switch the embedded post-processor to this one, and save the
    /// project with it, before exporting.
    #[arg(long, value_name = "ID")]
    pub post: Option<String>,
    /// The date written in the program header instead of today's (UTC).
    /// For reproducible output in tests.
    #[arg(long, hide = true, value_name = "YYYY-MM-DD")]
    pub date: Option<String>,
}

/// Arguments of `check`.
#[derive(Debug, clap::Args)]
pub struct CheckArgs {
    /// The project to check.
    pub dcv: PathBuf,
}

/// `--units` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum UnitsArg {
    /// Millimetres.
    Mm,
    /// Inches.
    Inch,
}

impl From<UnitsArg> for dc_project::Units {
    fn from(units: UnitsArg) -> Self {
        match units {
            UnitsArg::Mm => Self::Mm,
            UnitsArg::Inch => Self::Inch,
        }
    }
}

/// Why a command failed: the exit code and what to print on stderr.
#[derive(Debug)]
pub struct Failure {
    /// The process exit code: 1 or 2.
    pub code: u8,
    /// The message, without the `error: ` prefix.
    pub message: String,
}

impl Failure {
    /// A validation, import, export, or file error (exit 1).
    pub fn error(message: impl fmt::Display) -> Self {
        Self {
            code: 1,
            message: message.to_string(),
        }
    }

    /// A usage error (exit 2).
    pub fn usage(message: impl fmt::Display) -> Self {
        Self {
            code: 2,
            message: message.to_string(),
        }
    }
}

/// Parses `args` and runs the command.
pub fn run(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let cli = match Cli::try_parse_from(args) {
        Ok(cli) => cli,
        Err(err) => {
            // Help and version print to stdout and exit 0; anything else is
            // a usage error, exit 2.
            let _ = err.print();
            return ExitCode::from(u8::try_from(err.exit_code()).unwrap_or(2));
        }
    };
    let result = match cli.command {
        Command::Init(args) => init::run(&args),
        Command::Export(args) => export::run(&args),
        Command::Check(args) => check::run(&args),
        Command::Posts => posts(),
    };
    match result {
        Ok(code) => code,
        Err(failure) => {
            eprintln!("error: {}", failure.message);
            ExitCode::from(failure.code)
        }
    }
}

/// `dreamcarve posts`: one line per bundled post-processor, id first.
fn posts() -> Result<ExitCode, Failure> {
    let posts = dc_gcode::post::bundled().map_err(Failure::error)?;
    let width = posts.iter().map(|p| p.id.len()).max().unwrap_or(0);
    for post in posts {
        println!("{:width$}  {}", post.id, post.name);
    }
    Ok(ExitCode::SUCCESS)
}

/// The bundled post-processor `id`; an unknown id is a usage error that
/// lists the known ones.
pub fn lookup_post(id: &str) -> Result<dc_project::PostProcessor, Failure> {
    match dc_gcode::post::lookup(id) {
        Ok(post) => Ok(post),
        Err(err @ dc_gcode::ExportError::UnknownPostProcessor { .. }) => {
            let known: Vec<&str> = dc_gcode::post::bundled()
                .map_err(Failure::error)?
                .iter()
                .map(|p| p.id.as_str())
                .collect();
            Err(Failure::usage(format!(
                "{err}; available: {}",
                known.join(", ")
            )))
        }
        Err(err) => Err(Failure::error(err)),
    }
}

/// Reads `path`, naming it in the error.
pub fn read_file(path: &Path) -> Result<Vec<u8>, Failure> {
    std::fs::read(path).map_err(|e| Failure::error(format!("cannot read {}: {e}", path.display())))
}

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
pub struct StagedFile {
    /// The temporary file; `None` once committed.
    temp: Option<PathBuf>,
    dest: PathBuf,
}

impl StagedFile {
    /// Writes `contents` to a new temporary file in `dest`'s directory.
    pub fn write(dest: &Path, contents: &[u8]) -> Result<Self, Failure> {
        let name = dest
            .file_name()
            .ok_or_else(|| Failure::error(format!("{} is not a file path", dest.display())))?;
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
    ) -> Result<Self, Failure> {
        let fail =
            |e: std::io::Error| Failure::error(format!("cannot write {}: {e}", dest.display()));
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
        Err(Failure::error(format!(
            "cannot write {}: no free temporary file name in its directory",
            dest.display()
        )))
    }

    /// Moves the file into place, replacing any file already there.
    pub fn commit(mut self) -> Result<(), Failure> {
        let Some(temp) = self.temp.take() else {
            return Ok(());
        };
        match std::fs::rename(&temp, &self.dest) {
            Ok(()) => Ok(()),
            Err(e) => {
                // Not moved: put it back so `Drop` removes it.
                self.temp = Some(temp);
                Err(Failure::error(format!(
                    "cannot write {}: {e}",
                    self.dest.display()
                )))
            }
        }
    }
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

/// Today's date in UTC as `YYYY-MM-DD`, for the program header.
pub fn today_utc() -> String {
    clock::date_utc(clock::now_secs())
}

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
