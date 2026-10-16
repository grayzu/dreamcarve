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

use crate::{check, export, init};

/// DreamCarve: open source CAD/CAM for CNC hobbyists.
#[derive(Debug, Parser)]
#[command(version, about, arg_required_else_help = true)]
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
    Err(Failure::error("not implemented"))
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

/// A file ready to be moved into place: written in full to a temporary
/// file beside its destination, so a failure never leaves a partial file
/// and the final [`StagedFile::commit`] is a rename.
pub struct StagedFile {
    temp: PathBuf,
    dest: PathBuf,
}

impl StagedFile {
    /// Writes `contents` to a temporary file in `dest`'s directory.
    pub fn write(dest: &Path, contents: &[u8]) -> Result<Self, Failure> {
        let name = dest
            .file_name()
            .ok_or_else(|| Failure::error(format!("{} is not a file path", dest.display())))?;
        let mut temp_name = OsString::from(".");
        temp_name.push(name);
        temp_name.push(format!(".dreamcarve-{}.tmp", std::process::id()));
        let staged = Self {
            temp: dest.with_file_name(temp_name),
            dest: dest.to_path_buf(),
        };
        let written = std::fs::File::create(&staged.temp).and_then(|mut file| {
            file.write_all(contents)?;
            file.sync_all()
        });
        match written {
            Ok(()) => Ok(staged),
            Err(e) => Err(Failure::error(format!(
                "cannot write {}: {e}",
                staged.dest.display()
            ))),
        }
    }

    /// Moves the file into place, replacing any file already there.
    pub fn commit(self) -> Result<(), Failure> {
        std::fs::rename(&self.temp, &self.dest)
            .map_err(|e| Failure::error(format!("cannot write {}: {e}", self.dest.display())))
        // `self` drops here; the temporary file is gone after a rename, and
        // `Drop` ignores the missing file.
    }
}

impl Drop for StagedFile {
    fn drop(&mut self) {
        // Removes the temporary file when the stage is abandoned.
        let _ = std::fs::remove_file(&self.temp);
    }
}

/// Today's date in UTC as `YYYY-MM-DD`, for the program header.
pub fn today_utc() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let days = i64::try_from(secs / 86_400).unwrap_or(0);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Proleptic Gregorian date of `days` since 1970-01-01 (Howard Hinnant's
/// `civil_from_days`, public domain).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    // `d` is 1..=31 and `m` 1..=12 by construction.
    (
        y,
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_from_days_matches_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(20_741), (2026, 10, 15));
    }

    #[test]
    fn staged_file_is_removed_when_not_committed() {
        let dir = std::env::temp_dir().join(format!("dc-stage-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dest = dir.join("out.nc");
        let staged = StagedFile::write(&dest, b"G21\n").unwrap();
        drop(staged);
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nothing left");
        StagedFile::write(&dest, b"G21\n")
            .unwrap()
            .commit()
            .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"G21\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
