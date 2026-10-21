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
use dc_gcode::ProgramSplit;
use dc_project::{PostProcessorError, PostProcessorRef, SettingReset};

use crate::output::{self, errln, outln};
use crate::{check, clock, config, export, init, tabs, tools};

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
    /// Create a project (.dcv) from an SVG or DXF drawing.
    Init(InitArgs),
    /// Write G-code for a project.
    Export(ExportArgs),
    /// Report the problems export would find, without writing G-code.
    Check(CheckArgs),
    /// List the available post-processors.
    Posts,
    /// Show the user configuration.
    Config(ConfigArgs),
    /// Manage the tool library and copy its tools into projects.
    Tools(ToolsArgs),
    /// Place, list and remove a project's holding tabs.
    Tabs(TabsArgs),
}

/// Arguments of `tabs`.
#[derive(Debug, clap::Args)]
pub struct TabsArgs {
    /// What to do.
    #[command(subcommand)]
    pub command: TabsCommand,
}

/// The `tabs` subcommands. Each one that changes the project first places
/// the tab request of every enabled Profile, as `tabs place` would, and
/// saves the project through `dc_project::save`.
#[derive(Debug, Subcommand)]
pub enum TabsCommand {
    /// Place tabs on the outlines a Profile operation cuts, and save the
    /// project. With neither --count nor --at, places the operation's own
    /// tab request. Adds to the tabs already there unless --replace.
    Place(TabsPlaceArgs),
    /// List the project's tabs, and the operations that cut round each.
    List {
        /// The project.
        dcv: PathBuf,
    },
    /// Remove tabs and save the project: every tab, those one Profile
    /// operation cuts round, or those named with --tab.
    Clear {
        /// The project.
        dcv: PathBuf,
        /// Remove only the tabs this Profile operation cuts round.
        op: Option<String>,
        /// Remove this tab; repeat for several.
        #[arg(long = "tab", value_name = "ID", conflicts_with = "op")]
        tabs: Vec<String>,
    },
}

/// Arguments of `tabs place`.
#[derive(Debug, clap::Args)]
pub struct TabsPlaceArgs {
    /// The project.
    pub dcv: PathBuf,
    /// The Profile operation whose outlines get the tabs.
    pub op: String,
    /// Space this many tabs evenly along each tool path, as phase 2 laid
    /// `count` tabs: fewer on an outline too short for them.
    #[arg(long, conflicts_with = "at", value_parser = clap::value_parser!(u8).range(1..))]
    pub count: Option<u8>,
    /// Put a tab at each of these distances along every outline, in mm
    /// from its first vertex as drawn: `--at 10,55.5`.
    #[arg(long, value_delimiter = ',', value_name = "MM,...")]
    pub at: Option<Vec<f64>>,
    /// Tab width along the outline, mm [default: 8].
    #[arg(long, value_name = "MM")]
    pub width: Option<f64>,
    /// Tab top above the stock bottom, mm [default: 3].
    #[arg(long, value_name = "MM")]
    pub height: Option<f64>,
    /// Tab cross-section [default: rect].
    #[arg(long, value_enum)]
    pub shape: Option<TabShapeArg>,
    /// Put tabs on holes too, to hold their slugs.
    #[arg(long)]
    pub on_holes: bool,
    /// Remove the tabs the operation cuts round (those `tabs clear <dcv>
    /// <op>` removes) and place the new ones instead, in one save.
    #[arg(long)]
    pub replace: bool,
}

/// `--shape` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum TabShapeArg {
    /// Square-shouldered.
    Rect,
    /// A triangular bridge.
    Smooth,
}

impl From<TabShapeArg> for dc_project::TabShape {
    fn from(shape: TabShapeArg) -> Self {
        match shape {
            TabShapeArg::Rect => Self::Rect,
            TabShapeArg::Smooth => Self::Smooth,
        }
    }
}

/// Arguments of `tools`.
#[derive(Debug, clap::Args)]
pub struct ToolsArgs {
    /// What to do.
    #[command(subcommand)]
    pub command: ToolsCommand,
}

/// The `tools` subcommands.
#[derive(Debug, Subcommand)]
pub enum ToolsCommand {
    /// List every tool in the library.
    List {
        /// Print the library file's JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Copy a library tool into a project, under a new project tool id.
    Add {
        /// The project to add the tool to.
        dcv: PathBuf,
        /// The library tool: its id (`lt1`) or its name, if no other tool
        /// has that name.
        tool: String,
    },
    /// Add the tools of a DreamCarve tool library file, or of a Fusion 360
    /// library (`.tools` or `.json`), to the library.
    Import {
        /// The file to read.
        file: PathBuf,
        /// Overwrite a library tool that has the same source as an
        /// imported one, instead of adding the imported one beside it.
        #[arg(long)]
        replace: bool,
    },
    /// Write the library to a file, as DreamCarve tool library JSON.
    Export {
        /// The file to write.
        file: PathBuf,
    },
    /// Remove a tool from the library. Projects keep their copies.
    Remove {
        /// The library tool: its id (`lt1`) or its unique name.
        tool: String,
    },
}

/// Arguments of `config`.
#[derive(Debug, clap::Args)]
pub struct ConfigArgs {
    /// What to do.
    #[command(subcommand)]
    pub command: ConfigCommand,
}

/// The `config` subcommands.
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Print where the config file and the tool library live, and whether
    /// each exists.
    Path,
}

/// Arguments of `init`.
#[derive(Debug, clap::Args)]
pub struct InitArgs {
    /// The drawing to import: an SVG (.svg) or a DXF (.dxf), by extension.
    pub drawing: PathBuf,
    /// Where to write the project [default: the drawing's name with .dcv].
    #[arg(short, long, value_name = "DCV")]
    pub output: Option<PathBuf>,
    /// Post-processor to use, at its latest version (see `dreamcarve posts`).
    #[arg(long, value_name = "ID", default_value = "grblhal")]
    pub post: String,
    /// Display units of the project; G-code is written in these units
    /// [default: mm]. For a DXF, also the units the drawing is in: required
    /// when the file declares none ($INSUNITS), overriding it when it does.
    #[arg(long, value_enum)]
    pub units: Option<UnitsArg>,
    /// DXF only: join open segments whose ends are within this distance,
    /// in mm, and close a chain whose ends meet [default: 0.01].
    #[arg(long, value_name = "MM")]
    pub join_tolerance: Option<f64>,
    /// DXF only: import only this layer; repeat for several layers.
    #[arg(long = "layer", value_name = "NAME")]
    pub layers: Vec<String>,
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
    /// Switch the project to the latest version of this post-processor,
    /// keeping the settings you changed that it can use, and save the
    /// project, before exporting. The project's own id upgrades it to the
    /// latest version.
    #[arg(long, value_name = "ID")]
    pub post: Option<String>,
    /// With --post: take every setting from the new post-processor's
    /// defaults (and your configured ones) instead of keeping yours.
    #[arg(long, requires = "post")]
    pub reset_settings: bool,
    /// Write one program with T<n> M6 tool changes, whatever the project's
    /// tool_change_mode says. Needs a post-processor with M6 (grblHAL).
    /// Not saved to the project.
    #[arg(long, conflicts_with = "programs_per_tool")]
    pub one_program: bool,
    /// Write one program per tool run, whatever the project's
    /// tool_change_mode says. Not saved to the project.
    #[arg(long)]
    pub programs_per_tool: bool,
    /// The date written in the program header instead of today's (UTC).
    /// For reproducible output in tests.
    #[arg(long, hide = true, value_name = "YYYY-MM-DD")]
    pub date: Option<String>,
}

impl ExportArgs {
    /// How the programs are split, as the flags say; clap refuses both.
    pub fn split(&self) -> ProgramSplit {
        if self.one_program {
            ProgramSplit::OneProgram
        } else if self.programs_per_tool {
            ProgramSplit::ProgramPerTool
        } else {
            ProgramSplit::ProjectDefault
        }
    }
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

impl fmt::Display for UnitsArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Mm => "mm",
            Self::Inch => "inch",
        })
    }
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
        Command::Config(args) => config::run(&args.command),
        Command::Tools(args) => tools::run(&args.command),
        Command::Tabs(args) => tabs::run(&args.command),
    };
    // The command's own exit code stands even when stdout was a pipe its
    // reader closed (#216, `output`).
    match result.and_then(|code| output::flush().map(|()| code)) {
        Ok(code) => code,
        Err(failure) => {
            errln!("error: {}", failure.message);
            ExitCode::from(failure.code)
        }
    }
}

/// `dreamcarve posts`: one line per bundled post-processor, id first, from
/// its latest version.
fn posts() -> Result<ExitCode, Failure> {
    let posts = dc_project::latest_definitions().map_err(Failure::error)?;
    let width = posts.iter().map(|d| d.id.len()).max().unwrap_or(0);
    for definition in posts {
        outln!(
            "{:width$}  {}",
            definition.id,
            definition.post_processor().name
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

/// A reference to the latest bundled version of post-processor `id`, with
/// its default settings (#106); an unknown id is a usage error that lists
/// the known ones.
pub fn lookup_post(id: &str) -> Result<PostProcessorRef, Failure> {
    match PostProcessorRef::latest(id) {
        Ok(reference) => Ok(reference),
        Err(err @ PostProcessorError::UnknownId { .. }) => Err(Failure::usage(err.to_string())),
        Err(err) => Err(Failure::error(err)),
    }
}

/// [`PostProcessorRef`] `old` switched to the latest version of `id`,
/// keeping the user's settings (#277, [`dc_project::switch_post`]), with
/// the settings it reset; an unknown id is a usage error, as for
/// [`lookup_post`].
pub fn switch_post(
    old: &PostProcessorRef,
    id: &str,
) -> Result<(PostProcessorRef, Vec<SettingReset>), Failure> {
    match dc_project::switch_post(old, id) {
        Ok(switched) => Ok(switched),
        Err(err @ PostProcessorError::UnknownId { .. }) => Err(Failure::usage(err.to_string())),
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
pub fn commit_all(staged: Vec<StagedFile>) -> Result<(), Failure> {
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
    fn take(dest: &Path) -> Result<Option<Self>, Failure> {
        let fail =
            |e: std::io::Error| Failure::error(format!("cannot write {}: {e}", dest.display()));
        let meta = match std::fs::symlink_metadata(dest) {
            Ok(meta) => meta,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(fail(e)),
        };
        if meta.is_dir() {
            return Ok(None);
        }
        let name = dest
            .file_name()
            .ok_or_else(|| Failure::error(format!("{} is not a file path", dest.display())))?;
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
        Err(Failure::error(format!(
            "cannot write {}: no free temporary file name in its directory",
            dest.display()
        )))
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
