// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Argument parsing, exit codes, and the file and clock I/O the commands
//! share (`docs/specs/phase-1/design.md`, "CLI (dreamcarve)"). The staged
//! write and the config directory are `dreamcarve-shell`'s, shared with
//! the app (phase 4, slice 1).
//!
//! Exit codes: 0 success, 1 a validation, import, export, or file error,
//! 2 a usage error (clap's own code for bad arguments, and an unknown
//! post-processor id).

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use dc_gcode::ProgramSplit;
use dc_project::{PostProcessorError, PostProcessorRef, SettingReset};
use dreamcarve_shell::ShellError;

use crate::output::{self, errln, outln};
use crate::{check, config, export, image, init, prep, tabs, tools};

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
    /// Create a project (.dcv) from an SVG or DXF drawing, or from a PNG
    /// or JPEG picture to Photo V-Carve.
    Init(InitArgs),
    /// Add a PNG or JPEG picture to a project, and list a project's
    /// pictures.
    Image(ImageArgs),
    /// Prepare an SVG for inlay: turn strokes into areas, resolve fills,
    /// drop specks, and write a clean SVG of filled shapes.
    ImagePrep(ImagePrepArgs),
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
    /// With --count, leave tabs where the spacing puts them, on corners
    /// too. Without it each tab on a corner slides off it, as a request
    /// with `avoid_corners` does.
    #[arg(long, requires = "count")]
    pub allow_corners: bool,
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

/// Arguments of `image`.
#[derive(Debug, clap::Args)]
pub struct ImageArgs {
    /// What to do.
    #[command(subcommand)]
    pub command: ImageCommand,
}

/// The `image` subcommands (format 27, #645).
#[derive(Debug, Subcommand)]
pub enum ImageCommand {
    /// Add a picture to a project as a bitmap a Photo V-Carve can carve,
    /// converted to grayscale, and save the project.
    Add(ImageAddArgs),
    /// List the project's pictures and the Photo V-Carves that carve each.
    List {
        /// The project.
        dcv: PathBuf,
    },
}

/// Arguments of `image add`: exactly one of `--width` and `--height`.
#[derive(Debug, clap::Args)]
#[command(group(clap::ArgGroup::new("size").required(true).args(["width", "height"])))]
pub struct ImageAddArgs {
    /// The project to add the picture to.
    pub dcv: PathBuf,
    /// The picture: a PNG (.png) or JPEG (.jpg, .jpeg), by extension.
    pub picture: PathBuf,
    /// The picture's width on the stock, mm; its height follows the
    /// picture's proportions.
    #[arg(long, value_name = "MM", allow_negative_numbers = true)]
    pub width: Option<f64>,
    /// The picture's height on the stock, mm; its width follows the
    /// picture's proportions.
    #[arg(long, value_name = "MM", allow_negative_numbers = true)]
    pub height: Option<f64>,
    /// Where the picture's bottom-left corner goes, in mm from the stock's
    /// bottom-left corner: `--at 20,30` [default: 0,0].
    #[arg(long, value_name = "X,Y", value_parser = parse_at, allow_hyphen_values = true)]
    pub at: Option<(f64, f64)>,
    /// The picture's name in the project [default: the file's name
    /// without its extension].
    #[arg(long)]
    pub name: Option<String>,
}

/// `--at x,y`: two numbers, mm.
fn parse_at(text: &str) -> Result<(f64, f64), String> {
    let usage = || format!("`{text}` is not a position: give `x,y` in mm, such as `20,30`");
    let (x, y) = text.split_once(',').ok_or_else(usage)?;
    let number = |v: &str| v.trim().parse::<f64>().ok().filter(|v| v.is_finite());
    Ok((number(x).ok_or_else(usage)?, number(y).ok_or_else(usage)?))
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
    /// The drawing to import: an SVG (.svg) or a DXF (.dxf); or a picture
    /// to Photo V-Carve: a PNG (.png) or JPEG (.jpg, .jpeg). By extension.
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
    /// Picture only, and it or --height is required: the picture's width
    /// on the stock, mm; its height follows the picture's proportions.
    #[arg(long, value_name = "MM", allow_negative_numbers = true)]
    pub width: Option<f64>,
    /// Picture only, and it or --width is required: the picture's height
    /// on the stock, mm; its width follows the picture's proportions.
    #[arg(long, value_name = "MM", allow_negative_numbers = true)]
    pub height: Option<f64>,
}

/// Arguments of `image-prep`. It takes no tool, stock, depth, feed or size
/// option, by construction (functional 49b): every threshold is a share of
/// the design's height.
#[derive(Debug, clap::Args)]
pub struct ImagePrepArgs {
    /// The drawing to prepare: an SVG. A PNG or JPEG is refused; trace it
    /// in Inkscape (Path > Trace Bitmap) first.
    pub svg: PathBuf,
    /// Where to write the result [default: the drawing's name with
    /// -inlay added, beside it]. An existing file is never overwritten.
    #[arg(short, long, value_name = "SVG")]
    pub output: Option<PathBuf>,
    /// Make the enclosed spaces between the lines the inlay, not the ink
    /// itself: for line art.
    #[arg(long)]
    pub regions: bool,
    /// Drop pieces, and fill holes, whose widest point is under this
    /// percentage of the design's height, 0 to 5; 0 turns it off
    /// [default: 0.2].
    #[arg(long, value_name = "PCT", allow_negative_numbers = true)]
    pub min_detail: Option<f64>,
    /// Straighten runs of tiny segments within this percentage of the
    /// design's height, 0 to 5; 0 turns it off [default: 0.05].
    #[arg(long, value_name = "PCT", allow_negative_numbers = true)]
    pub smooth: Option<f64>,
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
    /// Write one program with `T<n> M6` tool changes, whatever the project's
    /// tool_change_mode says. Needs a post-processor with M6 (grblHAL).
    /// Not saved to the project.
    #[arg(long, conflicts_with = "programs_per_tool")]
    pub one_program: bool,
    /// Write one program per tool run, whatever the project's
    /// tool_change_mode says. Not saved to the project.
    #[arg(long)]
    pub programs_per_tool: bool,
    /// Also write a picture of every operation's toolpath over the stock
    /// and the shapes to this .svg file, with the programs.
    #[arg(long, value_name = "SVG")]
    pub preview: Option<PathBuf>,
    /// Write the programs of one board only: `female`, the stock, or
    /// `male`, the Inlays' male board (to re-cut the male after a fit
    /// change). Not saved to the project.
    #[arg(long, value_name = "BOARD")]
    pub board: Option<BoardArg>,
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
    /// Write a picture of every operation's toolpath over the stock and
    /// the shapes to this .svg file, when every operation generates.
    #[arg(long, value_name = "SVG")]
    pub preview: Option<PathBuf>,
}

/// `--board` values (#498).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum BoardArg {
    /// The stock: every operation but an Inlay's male.
    Female,
    /// The Inlays' male board.
    Male,
}

impl From<BoardArg> for dc_project::Board {
    fn from(board: BoardArg) -> Self {
        match board {
            BoardArg::Female => Self::Female,
            BoardArg::Male => Self::Male,
        }
    }
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

impl From<ShellError> for Failure {
    /// A file or config error of the shared shell: exit 1, its message.
    fn from(error: ShellError) -> Self {
        Self::error(error)
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
        Command::Image(args) => image::run(&args.command),
        Command::ImagePrep(args) => prep::run(&args),
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

/// Today's date in UTC as `YYYY-MM-DD`, for the program header.
pub fn today_utc() -> String {
    dreamcarve_shell::clock::today_utc()
}
