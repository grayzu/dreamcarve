// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve image`: a project's pictures (format 27, #645;
//! `docs/specs/phase-3/design.md`, "CLI"; functional spec, capability 46).
//!
//! - `image add <dcv> <png|jpg|jpeg> (--width <mm> | --height <mm>) [--at
//!   x,y] [--name <name>]` decodes the picture to grayscale
//!   (`dc_import::bitmap`), appends it as the next bitmap with
//!   [`dc_job::picture::picture_into`], and saves the project, staged,
//!   after placing every enabled Profile's tab request, as every saving
//!   command does. It prints the bitmap's id, pixel size and footprint,
//!   and the import's warnings (`image_colour_converted` and the like).
//! - `image list <dcv>` prints a table of the bitmaps, read from each
//!   PNG's header, never its pixels.
//!
//! The decoding, the placement and the rows are the library's, so the
//! app's *Import picture* gets the same; this module reads, prints and
//! writes.

use std::path::Path;
use std::process::ExitCode;

use dc_cam::placed_tabs::place_requests;
use dc_geometry::Point;
use dc_import::ImageFormat;
use dc_import::bitmap;
use dc_job::JobError;
use dc_job::picture::{PictureSize, picture_into, picture_rows};
use dc_project::Grayscale;
use dreamcarve_shell::StagedFile;

use crate::cli::{Failure, ImageAddArgs, ImageCommand, read_file};
use crate::export::load_project;
use crate::output::{errln, outln};

/// Runs `image <command>`.
pub fn run(command: &ImageCommand) -> Result<ExitCode, Failure> {
    match command {
        ImageCommand::Add(args) => add(args),
        ImageCommand::List { dcv } => list(dcv),
    }?;
    Ok(ExitCode::SUCCESS)
}

/// The picture format of `path`, by its extension, or `None` when it
/// names no format DreamCarve reads as a picture.
pub fn picture_format(path: &Path) -> Option<ImageFormat> {
    path.extension()
        .and_then(|ext| ext.to_str())
        .and_then(ImageFormat::from_extension)
}

/// The size `--width` or `--height` gives, or a usage error naming both
/// flags when there is not exactly one.
pub fn picture_size(width: Option<f64>, height: Option<f64>) -> Result<PictureSize, Failure> {
    match (width, height) {
        (Some(width_mm), None) => Ok(PictureSize::Width { width_mm }),
        (None, Some(height_mm)) => Ok(PictureSize::Height { height_mm }),
        _ => Err(Failure::usage(
            "a picture needs exactly one of --width and --height, in mm; the other side \
             follows the picture's proportions",
        )),
    }
}

/// Reads and decodes the picture at `path` in `format`, printing each
/// warning as `warning: <code>: <message>`.
pub fn import_picture(path: &Path, format: ImageFormat) -> Result<Grayscale, Failure> {
    let bytes = read_file(path)?;
    let (pixels, warnings) = bitmap::import(&bytes, format)
        .map_err(|e| Failure::error(format!("cannot import {}: {e}", path.display())))?;
    for warning in &warnings {
        errln!("warning: {}: {warning}", warning.code());
    }
    Ok(pixels)
}

/// The name a picture gets: its file name without the extension.
pub fn picture_name(path: &Path) -> String {
    path.file_stem().map_or_else(
        || "picture".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// A placement error: a size that is not a length is the user's argument
/// (exit 2), anything else exit 1.
pub fn placement_failure(error: JobError) -> Failure {
    match error {
        JobError::PictureSize { .. } => Failure::usage(error),
        other => Failure::error(other),
    }
}

/// `image add`.
fn add(args: &ImageAddArgs) -> Result<(), Failure> {
    let format = picture_format(&args.picture).ok_or_else(|| {
        Failure::usage(format!(
            "{} is not a PNG or JPEG picture (.png, .jpg, .jpeg)",
            args.picture.display()
        ))
    })?;
    let size = picture_size(args.width, args.height)?;
    let (raw, _) = load_project(&args.dcv)?;
    let pixels = import_picture(&args.picture, format)?;
    let (placed, warnings) = place_requests(&raw).map_err(Failure::error)?;
    crate::tabs::print_warnings(&warnings);
    let name = args
        .name
        .clone()
        .unwrap_or_else(|| picture_name(&args.picture));
    let (x_mm, y_mm) = args.at.unwrap_or((0.0, 0.0));
    let (project, id) = picture_into(&placed, &name, &pixels, size, Point::new(x_mm, y_mm))
        .map_err(placement_failure)?;
    let text = dc_project::save(&project).map_err(Failure::error)?;
    StagedFile::write(&args.dcv, text.as_bytes())?.commit()?;
    let Some(bitmap) = project.bitmaps.iter().find(|b| b.id == id) else {
        return Err(Failure::error(format!("bitmap {id} was not added")));
    };
    outln!(
        "Added picture {id} `{name}` to {}: {} × {} px on {:.3} × {:.3} mm at ({:.3}, {:.3}).",
        args.dcv.display(),
        bitmap.width_px,
        bitmap.height_px,
        bitmap.width_mm,
        bitmap.height_mm,
        bitmap.x_mm,
        bitmap.y_mm,
    )?;
    Ok(())
}

/// `image list`: `ID NAME PX FOOTPRINT_MM AT_MM USED_BY`, one row per
/// bitmap, columns aligned; `USED_BY` lists the Photo V-Carves that carve
/// it, or `-`.
fn list(dcv: &Path) -> Result<(), Failure> {
    let (project, _) = load_project(dcv)?;
    let rows = picture_rows(&project)
        .map_err(|e| Failure::error(format!("cannot list {}: {e}", dcv.display())))?;
    if rows.is_empty() {
        outln!("{} has no pictures.", dcv.display())?;
        return Ok(());
    }
    let mut table =
        vec![["ID", "NAME", "PX", "FOOTPRINT_MM", "AT_MM", "USED_BY"].map(str::to_owned)];
    for row in rows {
        let used_by = if row.used_by.is_empty() {
            "-".to_owned()
        } else {
            row.used_by
                .iter()
                .map(|op| op.as_str())
                .collect::<Vec<_>>()
                .join(",")
        };
        table.push([
            row.id.as_str().to_owned(),
            row.name,
            format!("{}x{}", row.width_px, row.height_px),
            format!("{:.3}x{:.3}", row.width_mm, row.height_mm),
            format!("{:.3},{:.3}", row.x_mm, row.y_mm),
            used_by,
        ]);
    }
    let mut widths = [0_usize; 6];
    for cells in &table {
        for (width, cell) in widths.iter_mut().zip(cells) {
            *width = (*width).max(cell.chars().count());
        }
    }
    for cells in &table {
        let line = cells
            .iter()
            .zip(widths)
            .map(|(cell, width)| format!("{cell:width$}"))
            .collect::<Vec<_>>()
            .join("  ");
        outln!("{}", line.trim_end())?;
    }
    Ok(())
}
