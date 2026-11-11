// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve init <svg|dxf>`: imports the drawing with `dc-import` and
//! writes a `.dcv` project with the defaults of `docs/specs/phase-1/design.md`,
//! "CLI (dreamcarve)", which [`dc_job::project_from::drawing`] builds:
//! stock = drawing bounds plus a 10 mm margin on each side; origin
//! bottom-left at the stock top; one placeholder tool "Set me" (6.35 mm, 2
//! flutes, no feeds or step-down); one outside climb Profile of every
//! closed shape, safe Z 5 mm, asking for four 8 × 3 mm rectangular tabs on
//! each part it cuts and none on its holes: a tab request, cut as the tabs
//! it places, which `dreamcarve tabs` writes as placed tabs (#105,
//! ADR-0005). The format is the file's extension; a DXF takes the DXF-only
//! `--units` override, `--join-tolerance` and `--layer`
//! (`docs/specs/phase-2/design.md`, "CLI additions").
//!
//! The stock thickness, the Profile depth, the feeds, the spindle speed and
//! the step-down are left unset (`null`), so `export` refuses the project
//! until the user fills them in (S3, #64, #98): `init` cannot know the
//! material or the machine. `init` lists each of them as
//! `coaster.dcv:line:column: …`, a link an editor or terminal opens at the
//! `null` to replace (#76).
//!
//! On a picture (`.png`, `.jpg`, `.jpeg`; format 27, #645) `init` takes
//! exactly one of `--width` and `--height` and writes the project
//! [`dc_job::project_from::picture`] builds: the picture as bitmap `b1`
//! with the drawing's margin round it, a 60° V-bit and one Photo V-Carve,
//! the thickness, the maximum depth and the feeds unset.

use std::path::Path;
use std::process::ExitCode;

use dc_import::{
    DxfImportOptions, ImportError, ImportWarning, JOIN_TOLERANCE_DEFAULT_MM, SvgImportOptions,
    import_dxf, import_svg, prep,
};
use dc_job::JobError;
use dc_job::import::Drawing;
use dc_job::project_from;
use dc_project::{Project, SourceMap, validate};
use dreamcarve_shell::StagedFile;

use crate::cli::{Failure, InitArgs, UnitsArg, lookup_post, read_file};
use crate::config;
use crate::image::{import_picture, picture_format, picture_name, picture_size, placement_failure};
use crate::output::{errln, outln};

/// Runs `init`.
pub fn run(args: &InitArgs) -> Result<ExitCode, Failure> {
    let mut post = lookup_post(&args.post)?;
    // The user's configured defaults (`tool_change_mode`) go on top of the
    // definition's, before anything is written (capability 19).
    config::seed_post(&mut post)?;
    let is_dxf = args
        .drawing
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dxf"));
    if !is_dxf && (args.join_tolerance.is_some() || !args.layers.is_empty()) {
        return Err(Failure::usage(
            "--join-tolerance and --layer apply to DXF drawings only",
        ));
    }
    let picture = picture_format(&args.drawing);
    let size = match picture {
        Some(_) => Some(picture_size(args.width, args.height)?),
        None if args.width.is_some() || args.height.is_some() => {
            return Err(Failure::usage(
                "--width and --height apply to pictures (.png, .jpg, .jpeg) only",
            ));
        }
        None => None,
    };
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| args.drawing.with_extension("dcv"));
    if output.exists() {
        // The .dcv is where the user's edits live; never overwrite one.
        return Err(Failure::error(format!(
            "{} already exists; remove it or choose another file with -o",
            output.display()
        )));
    }
    if let (Some(format), Some(size)) = (picture, size) {
        let pixels = import_picture(&args.drawing, format)?;
        outln!(
            "Imported a {} × {} px picture from {}.",
            pixels.width_px,
            pixels.height_px,
            args.drawing.display()
        )?;
        let display_units = args.units.unwrap_or(UnitsArg::Mm).into();
        let project = project_from::picture(
            &picture_name(&args.drawing),
            &pixels,
            size,
            post,
            display_units,
        )
        .map_err(placement_failure)?;
        let text = dc_project::save(&project).map_err(Failure::error)?;
        StagedFile::write(&output, text.as_bytes())?.commit()?;
        report_picture(&project, &output, &text)?;
        return Ok(ExitCode::SUCCESS);
    }

    let bytes = read_file(&args.drawing)?;
    let (import, warnings) = if is_dxf {
        import_dxf_drawing(args, &bytes)?
    } else {
        import_svg_drawing(args, &bytes)?
    };
    if is_dxf {
        // A DXF's warnings print as they always did; the checks need an
        // SVG's paint and paths.
        for warning in &warnings {
            errln!("warning: {warning}");
        }
    } else {
        print_svg_warnings(&warnings, &bytes)?;
    }

    let name = project_name(&args.drawing);
    let display_units = args.units.unwrap_or(UnitsArg::Mm).into();
    let project =
        project_from::drawing(&name, import, post, display_units, true).map_err(|e| match e {
            JobError::NoShapes => Failure::error(format!(
                "{} contains no shapes to import",
                args.drawing.display()
            )),
            // The drawing imported within the limit, but the margin pushed
            // the stock past it; refused rather than written as a project
            // `check` rejects.
            JobError::StockTooLarge {
                width_mm,
                height_mm,
                margin_mm,
                limit_mm,
            } => Failure::error(format!(
                "{} with a {margin_mm} mm margin needs a {width_mm:.2} × {height_mm:.2} mm stock, \
                 larger than the {limit_mm} mm limit; make the drawing smaller",
                args.drawing.display(),
            )),
            other => Failure::error(other),
        })?;
    let text = dc_project::save(&project).map_err(Failure::error)?;
    StagedFile::write(&output, text.as_bytes())?.commit()?;

    report(&project, &output, &text)?;
    Ok(ExitCode::SUCCESS)
}

/// Prints an SVG import's `warnings` and the 49a import checks of `bytes`
/// as `warning: <code>: <message>`. The checks count the text, images,
/// clip paths and masks over the whole document, so they stand in for the
/// importer's own per-group warnings about them; the rest of the
/// importer's come first. Nothing here changes the project or the exit
/// status.
fn print_svg_warnings(warnings: &[ImportWarning], bytes: &[u8]) -> Result<(), Failure> {
    let counted_by_the_checks = |w: &ImportWarning| {
        matches!(
            w,
            ImportWarning::TextIgnored { .. }
                | ImportWarning::ImagesIgnored { .. }
                | ImportWarning::ClipPathIgnored { .. }
                | ImportWarning::MaskIgnored { .. }
        )
    };
    // The checks never change the outcome: a drawing they cannot measure
    // (they budget the strokes' outlines, which the import does not build)
    // is imported all the same, with a note.
    let checks = prep::findings(bytes, &SvgImportOptions::new()).unwrap_or_else(|e| {
        errln!("warning: import_checks_skipped: the drawing could not be checked: {e}");
        Vec::new()
    });
    for warning in warnings.iter().filter(|w| !counted_by_the_checks(w)) {
        errln!("warning: {}: {warning}", warning.code());
    }
    for warning in &checks {
        errln!("warning: {}: {warning}", warning.code());
    }
    Ok(())
}

/// Imports an SVG and prints what was read.
fn import_svg_drawing(
    args: &InitArgs,
    bytes: &[u8],
) -> Result<(Drawing, Vec<ImportWarning>), Failure> {
    let (import, warnings) = import_svg(bytes, &SvgImportOptions::new())
        .map_err(|e| Failure::error(format!("cannot import {}: {e}", args.drawing.display())))?;
    outln!(
        "Imported {} shape(s) from {}: page units {}, 1 user unit = {:.4} mm.",
        import.definitions.len(),
        args.drawing.display(),
        import.units,
        import.mm_per_user_unit
    )?;
    Ok((Drawing::Svg(import), warnings))
}

/// Imports a DXF with the DXF-only flags and prints what was read. A file
/// with no usable `$INSUNITS` is refused with the flag to pass.
fn import_dxf_drawing(
    args: &InitArgs,
    bytes: &[u8],
) -> Result<(Drawing, Vec<ImportWarning>), Failure> {
    let options = DxfImportOptions {
        units: args.units.map(Into::into),
        join_tolerance_mm: args.join_tolerance.unwrap_or(JOIN_TOLERANCE_DEFAULT_MM),
        layers: (!args.layers.is_empty()).then(|| args.layers.clone()),
        ..DxfImportOptions::new()
    };
    let (import, warnings) = import_dxf(bytes, &options).map_err(|e| {
        let hint = match e {
            ImportError::UnitsUnknown { .. } => "; pass `--units mm|inch`",
            _ => "",
        };
        Failure::error(format!(
            "cannot import {}: {e}{hint}",
            args.drawing.display()
        ))
    })?;
    let units = match (args.units, import.units) {
        (Some(units), _) => format!("units {units} (from --units)"),
        (None, Some(units)) => format!("units {units} (from $INSUNITS)"),
        // Unreachable: without either, the import fails above.
        (None, None) => "units unknown".to_owned(),
    };
    outln!(
        "Imported {} shape(s) from {}: {units}, 1 drawing unit = {:.4} mm.",
        import.definitions.len(),
        args.drawing.display(),
        import.mm_per_unit
    )?;
    Ok((Drawing::Dxf(import), warnings))
}

/// The project name: the drawing's file name without its extension.
fn project_name(drawing: &Path) -> String {
    drawing.file_stem().map_or_else(
        || "untitled".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// Prints what was written and what the user must do next: one line per
/// value to set, located in `text`, the file as written.
fn report(project: &Project, output: &Path, text: &str) -> Result<(), Failure> {
    let targets = project
        .operations
        .iter()
        .map(|op| op.targets().len())
        .sum::<usize>();
    let untargeted = project.instances.len() - targets.min(project.instances.len());
    outln!(
        "Wrote {}: stock {:.2} × {:.2} mm, post-processor {}, {} shape(s) targeted by Profile.",
        output.display(),
        project.stock.width_mm,
        project.stock.height_mm,
        project.post_processor.id,
        targets,
    )?;
    if untargeted > 0 {
        outln!(
            "{untargeted} shape(s) are open paths and are not cut; `dreamcarve check` lists them."
        )?;
    }
    values_to_set(project, output, text)?;
    outln!(
        "Next: edit {} and replace each `null` listed above. To cut through, `depth_mm` is \
         the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check {}`.",
        output.display(),
        output.display()
    )
}

/// Prints the values `export` refuses without, one a line, each located
/// in `text`, the file as written.
fn values_to_set(project: &Project, output: &Path, text: &str) -> Result<(), Failure> {
    outln!("Values to set; export refuses until every one is set:")?;
    let map = SourceMap::parse(text).ok();
    for problem in validate(project).iter().filter(|p| p.is_unset_value()) {
        let at = problem
            .field()
            .and_then(|field| map.as_ref()?.locate(&field));
        match at {
            Some(at) => outln!("{}:{at}: {problem}", output.display())?,
            None => outln!("{problem}")?,
        }
    }
    Ok(())
}

/// [`report`] for a picture's project: the bitmap and the Photo V-Carve
/// written, then the values to set (the thickness, the maximum depth, the
/// V-bit's feeds and speed).
fn report_picture(project: &Project, output: &Path, text: &str) -> Result<(), Failure> {
    for bitmap in &project.bitmaps {
        outln!(
            "Wrote {}: stock {:.2} × {:.2} mm, post-processor {}, picture {} ({} × {} px) \
             on {:.3} × {:.3} mm at ({:.3}, {:.3}), carved by one Photo V-Carve.",
            output.display(),
            project.stock.width_mm,
            project.stock.height_mm,
            project.post_processor.id,
            bitmap.id,
            bitmap.width_px,
            bitmap.height_px,
            bitmap.width_mm,
            bitmap.height_mm,
            bitmap.x_mm,
            bitmap.y_mm,
        )?;
    }
    values_to_set(project, output, text)?;
    outln!(
        "Next: edit {} and replace each `null` listed above; the tool is a 60° V-bit, so set \
         the one you have. Then run `dreamcarve check {}`.",
        output.display(),
        output.display()
    )
}
