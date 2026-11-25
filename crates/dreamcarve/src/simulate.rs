// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve simulate <dcv>` (phase 6 functional spec, capability 125):
//! cuts the project's runs into its stock as the app's Simulate step
//! does, prints what the stock is like then, and with `--out` writes it
//! as a 16-bit grayscale PNG.
//!
//! It reads the runs `export` writes ([`dc_job::build()`], then
//! [`dc_job::simulate::plan`]) and changes nothing (S22). A project
//! `check` refuses is refused with `check`'s lines. The one file it may
//! write is the picture, and only over nothing or over a picture it
//! wrote before.

use std::path::Path;
use std::process::ExitCode;

use dc_job::sim::{
    CellSpec, Grid, Heightfield, Outcome, Quality, UNCUT_MM, UNCUT_RANGE_MM, build, compare, facts,
};
use dc_job::simulate::{describe, describe_uncut, design, heightmap_png, is_heightmap, plan};
use dc_job::{Built, ProgramSplit};
use dc_project::Board;
use dreamcarve_shell::StagedFile;

use crate::cli::{Failure, QualityArg, SimulateArgs};
use crate::config;
use crate::export;
use crate::output::outln;

/// The date filled into the discarded programs. It never reaches a file.
const SIMULATE_DATE: &str = "simulate";

/// Runs `simulate`.
pub fn run(args: &SimulateArgs) -> Result<ExitCode, Failure> {
    if let Some(out) = &args.out {
        check_out_path(out, &args.dcv)?;
    }
    let (project, file) = export::load_project(&args.dcv)?;
    let estimate_settings = config::estimate_settings()?;
    let Built {
        findings,
        generated,
        ..
    } = dc_job::build(&project, ProgramSplit::ProjectDefault, SIMULATE_DATE);
    // What `check` refuses, with its lines.
    for error in &findings.errors {
        outln!("{}", file.describe("error", error))?;
    }
    let Some(generated) = generated.filter(|_| findings.errors.is_empty()) else {
        return Err(Failure::error(format!(
            "{} has {} error(s); nothing was simulated",
            args.dcv.display(),
            findings.errors.len()
        )));
    };

    let plan = plan(&project, &generated, &estimate_settings).map_err(Failure::error)?;
    let wanted = args.board.map_or(Board::Female, Board::from);
    let Some(board) = plan.board(wanted) else {
        return Err(Failure::error(format!(
            "{} cuts no {} board",
            args.dcv.display(),
            wanted.as_str()
        )));
    };
    let runs_cut = match &args.until {
        None => board.runs.len(),
        Some(op) => board
            .runs
            .iter()
            .rposition(|run| run.op.as_str() == op)
            .map(|last| last + 1)
            .ok_or_else(|| {
                Failure::error(format!(
                    "--until {op}: no enabled operation of that id cuts the {} board",
                    wanted.as_str()
                ))
            })?,
    };

    let uncut_mm = match args.uncut_over {
        None => UNCUT_MM,
        Some(mm) if (UNCUT_RANGE_MM.0..=UNCUT_RANGE_MM.1).contains(&mm) => mm,
        Some(mm) => {
            return Err(Failure::error(format!(
                "--uncut-over must be from {} to {} mm, not {mm}",
                UNCUT_RANGE_MM.0, UNCUT_RANGE_MM.1
            )));
        }
    };
    let spec = match (args.cell, args.quality) {
        (Some(cell_mm), _) => CellSpec::CellMm(cell_mm),
        (None, quality) => CellSpec::Quality(quality.map_or(Quality::Standard, Quality::from)),
    };
    let grid = Grid::of(board.width_mm, board.height_mm, spec).map_err(Failure::error)?;
    let stock = Heightfield::uncut(grid, board.thickness_mm).map_err(Failure::error)?;
    let sim_runs = board.sim_runs();
    let cut = sim_runs.get(..runs_cut).unwrap_or(&sim_runs);
    let field = match build(stock, cut, 0, &mut |_, _| {}, &mut |_| true).map_err(Failure::error)? {
        Outcome::Finished(field) => field,
        Outcome::Stopped => return Err(Failure::error("the simulation was stopped")),
    };

    let found = facts(&field, cut);
    for line in describe(&project, board, &field, runs_cut, &found) {
        outln!("{line}")?;
    }
    // Capability 120: what the shapes ask for and the bits did not cut.
    // Only the finished stock is compared, and only the stock is asked
    // anything: part-way, every later operation's shape would be uncut.
    if runs_cut == board.runs.len() && board.board == Board::Female {
        let asked = design(&project, board, &generated, grid);
        let uncut = compare(&field, &asked, uncut_mm);
        for line in describe_uncut(&project, &uncut, uncut_mm, grid.cell_mm()) {
            outln!("{line}")?;
        }
    }
    if let Some(out) = &args.out {
        let png = heightmap_png(&field).map_err(Failure::error)?;
        StagedFile::write(out, &png)?.commit()?;
        outln!(
            "Wrote heightmap {} ({} × {} px, 16-bit: white is the board's top, black its bottom).",
            out.display(),
            grid.nx(),
            grid.ny()
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

impl From<QualityArg> for Quality {
    fn from(quality: QualityArg) -> Self {
        match quality {
            QualityArg::Draft => Self::Draft,
            QualityArg::Standard => Self::Standard,
            QualityArg::Fine => Self::Fine,
        }
    }
}

/// Refuses an `--out` path that is not a `.png`, is the project file, or
/// holds anything but a heightmap this command wrote: a picture of the
/// user's is never written over.
fn check_out_path(out: &Path, dcv: &Path) -> Result<(), Failure> {
    let png = out
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("png"));
    if !png {
        return Err(Failure::error(format!(
            "--out must name a .png file, not {}",
            out.display()
        )));
    }
    if export::same_path(out, dcv) {
        return Err(Failure::error(format!(
            "--out {} is the project file; refusing to write over it",
            out.display()
        )));
    }
    if std::fs::symlink_metadata(out).is_ok() {
        let ours = dreamcarve_shell::read_capped(out, dreamcarve_shell::MAX_FILE_BYTES)
            .is_ok_and(|bytes| is_heightmap(&bytes));
        if !ours {
            return Err(Failure::error(format!(
                "--out {} exists and is not a DreamCarve heightmap (your picture?); refusing to write over it",
                out.display()
            )));
        }
    }
    Ok(())
}
