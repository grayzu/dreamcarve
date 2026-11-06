// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve tools`: the tool library (`docs/specs/phase-2/design.md`,
//! "Algorithms → Tool library CLI"; functional spec, capability 21).
//!
//! The library is `<config dir>/tools.json` ([`crate::config::Paths`]), read and
//! written through `dreamcarve_shell::library`, which the app shares;
//! `dc-library` parses and edits it in memory. A missing
//! file is an empty library for `list`, and is created, with its
//! directory, by the first command that writes it (`import`). Every write
//! is staged and renamed into place, so a failure leaves the old file.
//!
//! `tools import` reads a DreamCarve library or a Fusion 360 library
//! (`dc_library::import`, by content), and adds each tool as
//! `dc_library::add` (or `replace` with `--replace`) decides; a Fusion
//! re-import is `already present` tool by tool.
//!
//! `tools add` copies a library tool into a project under the next free
//! project id (`t<n>`), keeping every value including its `number`
//! (`dc_library::project_copy`, which the app's *Add to project* uses),
//! and saves the project through `dc_project::save`. Projects embed copies
//! (ADR-0002): no later library command changes a project.

use std::path::Path;
use std::process::ExitCode;

use dc_library::{Library, LibraryTool};
use dc_project::ToolKind;

use dreamcarve_shell::StagedFile;

use crate::cli::{Failure, ToolsCommand, read_file};
use crate::config::Paths;
use crate::export::load_project;
use crate::output::{out, outln};

/// Runs `tools <command>`.
pub fn run(command: &ToolsCommand) -> Result<ExitCode, Failure> {
    let paths = Paths::from_env()?;
    match command {
        ToolsCommand::List { json } => list(&paths, *json),
        ToolsCommand::Add { dcv, tool } => add(&paths, dcv, tool),
        ToolsCommand::Import { file, replace } => import(&paths, file, *replace),
        ToolsCommand::Export { file } => export(&paths, file),
        ToolsCommand::Remove { tool } => remove(&paths, tool),
    }?;
    Ok(ExitCode::SUCCESS)
}

/// The library at `paths`, or `None` when its file does not exist.
fn read_library(paths: &Paths) -> Result<Option<Library>, Failure> {
    Ok(dreamcarve_shell::library::read(paths)?)
}

/// The library at `paths`; a missing file is an error saying how to
/// start one.
fn existing_library(paths: &Paths) -> Result<Library, Failure> {
    read_library(paths)?.ok_or_else(|| {
        Failure::error(format!(
            "there is no tool library yet ({} does not exist); add tools with \
             `dreamcarve tools import <file>`",
            paths.library_file.display()
        ))
    })
}

/// Writes `lib` as the library file, creating its directory if needed.
fn write_library(paths: &Paths, lib: &Library) -> Result<(), Failure> {
    Ok(dreamcarve_shell::library::write(paths, lib)?)
}

/// `tools list`: a table of every tool, or with `--json` the library file.
fn list(paths: &Paths, json: bool) -> Result<(), Failure> {
    let library = read_library(paths)?;
    if json {
        let lib = library.unwrap_or_default();
        out!("{}", dc_library::save(&lib).map_err(Failure::error)?)?;
        return Ok(());
    }
    let Some(lib) = library else {
        outln!(
            "No tool library yet: {} does not exist. Add tools with `dreamcarve tools import <file>`.",
            paths.library_file.display()
        )?;
        return Ok(());
    };
    if lib.tools.is_empty() {
        outln!(
            "The tool library {} is empty.",
            paths.library_file.display()
        )?;
        return Ok(());
    }
    let header = [
        "ID",
        "NAME",
        "VENDOR",
        "PRODUCT",
        "CUTTER",
        "DIAMETER_MM",
        "FEED_MM_MIN",
        "PLUNGE_MM_MIN",
        "RPM",
        "NUMBER",
        "SOURCE",
    ]
    .map(str::to_owned);
    let rows: Vec<[String; 11]> = lib.tools.iter().map(row).collect();
    let mut widths = header.clone().map(|h| h.chars().count());
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    for row in std::iter::once(&header).chain(&rows) {
        let line: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, &width)| format!("{cell:<width$}"))
            .collect();
        outln!("{}", line.join("  ").trim_end())?;
    }
    Ok(())
}

/// One `tools list` row: `-` for a value not set.
fn row(entry: &LibraryTool) -> [String; 11] {
    let tool = &entry.tool;
    let or_dash = |value: Option<String>| value.unwrap_or_else(|| "-".into());
    [
        entry.id.to_string(),
        tool.name.clone(),
        or_dash(tool.vendor.clone()),
        or_dash(tool.product_id.clone()),
        describe_cutter(&tool.cutter),
        tool.diameter_mm.to_string(),
        or_dash(tool.feed_mm_min.map(|v| v.to_string())),
        or_dash(tool.plunge_mm_min.map(|v| v.to_string())),
        or_dash(tool.rpm.map(|v| v.to_string())),
        or_dash(tool.number.map(|v| v.to_string())),
        or_dash(entry.source.clone()),
    ]
}

/// The cutter as `tools list` shows it.
fn describe_cutter(cutter: &ToolKind) -> String {
    match cutter {
        ToolKind::EndMill => "end mill".into(),
        ToolKind::BallNose => "ball nose".into(),
        ToolKind::VBit { included_angle_deg } => format!("V-bit {included_angle_deg}°"),
        ToolKind::Engraver => "engraver".into(),
        ToolKind::Drill => "drill".into(),
    }
}

/// `tools add <dcv> <tool>`: copies the tool into the project.
fn add(paths: &Paths, dcv: &Path, key: &str) -> Result<(), Failure> {
    let lib = existing_library(paths)?;
    let entry = dc_library::find(&lib, key).map_err(Failure::error)?;
    let (mut project, _) = load_project(dcv)?;
    let copy = dc_library::project_copy(&project.tools, &entry.tool);
    let id = copy.id.clone();
    project.tools.push(copy);
    let text = dc_project::save(&project).map_err(Failure::error)?;
    StagedFile::write(dcv, text.as_bytes())?.commit()?;
    outln!(
        "Added `{}` ({}) to {} as tool {id}.",
        entry.tool.name,
        entry.id,
        dcv.display()
    )?;
    Ok(())
}

/// `tools import <file> [--replace]`: adds each tool of a DreamCarve
/// library file or a Fusion 360 library (told apart by content, not
/// extension), reporting per tool what happened: imported, already
/// present, replaced, or for a Fusion item DreamCarve has no cutter for,
/// skipped with the reason (`dc_library::import`, the report the app
/// shows too). Values a Fusion preset left unset are noted on the tool's
/// line. A file that does not load, or a tool that is not valid, fails
/// the whole import and nothing is written.
fn import(paths: &Paths, file: &Path, replace: bool) -> Result<(), Failure> {
    let bytes = read_file(file)?;
    let fail = |e: dc_library::LibraryError| {
        Failure::error(format!("cannot import {}: {e}", file.display()))
    };
    let incoming = dc_library::import::read(&bytes).map_err(fail)?;
    let before = read_library(paths)?.unwrap_or_default();
    let merged = dc_library::import::merge(&before, incoming, replace).map_err(fail)?;
    for line in merged.report.lines() {
        outln!("{line}")?;
    }
    if merged.library != before {
        write_library(paths, &merged.library)?;
    }
    outln!(
        "{}",
        merged
            .report
            .summary(&paths.library_file.display().to_string())
    )?;
    Ok(())
}

/// `tools export <file>`: writes the library as DreamCarve JSON.
fn export(paths: &Paths, file: &Path) -> Result<(), Failure> {
    let lib = existing_library(paths)?;
    dreamcarve_shell::library::export(&lib, file)?;
    outln!("Wrote {} tool(s) to {}.", lib.tools.len(), file.display())?;
    Ok(())
}

/// `tools remove <tool>`: removes the tool from the library.
fn remove(paths: &Paths, key: &str) -> Result<(), Failure> {
    let mut lib = existing_library(paths)?;
    let removed = dc_library::remove(&mut lib, key)
        .map_err(|e| Failure::error(format!("cannot remove `{key}`: {e}")))?;
    write_library(paths, &lib)?;
    outln!(
        "Removed `{}` ({}) from the library. Projects keep their copies.",
        removed.tool.name,
        removed.id
    )?;
    Ok(())
}
