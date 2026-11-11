// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve check <dcv>`: runs the export pipeline ([`dc_job::build()`])
//! without writing anything and prints every error and warning it finds,
//! including each open path, which validation reports because Profile never
//! cuts it (functional spec, capabilities 4 and 8), then, when the program
//! could be built, its extents (#62) and estimated run time (capability
//! 30) as `export` prints them, for each program when there is one per tool
//! run (#95). Exits 1 when any error was found.
//!
//! The one file it may write is `--preview`'s SVG (addendum A5), when
//! every operation generated its toolpath, whatever else was found.
//!
//! Every value the user has yet to set is an error, so one run lists all of
//! them, each as `coaster.dcv:line:column: error: …` (#76).

use std::process::ExitCode;

use dc_geometry::{Contour, Region, nest};
use dc_import::{ImportWarning, prep};
use dc_job::summary::{
    board_heading, by_board, describe_carves, describe_extents, largest_tool_radius_mm,
    tool_radius_mm,
};
use dc_job::{Built, Estimates, ProgramSplit, preview};
use dc_project::Project;
use dreamcarve_shell::StagedFile;

use crate::cli::{CheckArgs, Failure};
use crate::config;
use crate::export;
use crate::output::outln;

/// The date filled into the discarded program. It never reaches a file.
const CHECK_DATE: &str = "check";

/// Runs `check`.
pub fn run(args: &CheckArgs) -> Result<ExitCode, Failure> {
    if let Some(preview) = &args.preview {
        export::check_preview_path(preview, &args.dcv)?;
    }
    let (project, file) = export::load_project(&args.dcv)?;
    let estimate_settings = config::estimate_settings()?;
    // `check` has no split flags: it reports what a plain `export` would.
    let Built {
        programs,
        findings,
        generated,
    } = dc_job::build(&project, ProgramSplit::ProjectDefault, CHECK_DATE);
    // Where a plain `export` would write its programs; with none, the one
    // program's default path.
    let boards = by_board(&project);
    let mut outputs = export::output_paths(None, &args.dcv, &project, &programs)?;
    if programs.is_empty() {
        outputs = vec![
            args.dcv
                .with_extension(&project.post_processor.file_extension),
        ];
    }
    if let Some(preview) = &args.preview {
        export::check_preview_not_program(preview, &outputs)?;
    }
    let estimates = Estimates::new(&project, &programs, estimate_settings);

    // #498: what migrating the file changed that the user should know.
    for note in &file.notes {
        outln!("{}: note: {note}", args.dcv.display())?;
    }
    for error in &findings.errors {
        outln!("{}", file.describe("error", error))?;
    }
    for warning in &findings.warnings {
        outln!("{}", file.describe("warning", warning))?;
    }
    // Sub-phase 3.4 (49a): the import checks that still apply to a project.
    let design = design_findings(&project);
    for warning in &design {
        outln!("warning: {}: {warning}", warning.code())?;
    }
    // Phase 3: a carve's depth follows its shapes, so its deepest cut is
    // stated.
    for line in describe_carves(&project, generated.as_deref().unwrap_or_default()) {
        outln!("{line}")?;
    }
    // #62: where the program would take the tool, and capability 30: how
    // long it would run, as `export` prints them; for each program when
    // there is one per tool run (#95), then the total.
    if let [program] = programs.as_slice()
        && !boards
    {
        let radius_mm = largest_tool_radius_mm(&project);
        for line in describe_extents(&project, program, radius_mm) {
            outln!("{line}")?;
        }
        outln!("{}", estimates.program_line(0))?;
    } else {
        let mut board = None;
        for (k, program) in programs.iter().enumerate() {
            // #498: each board's programs under its heading, named as
            // `export` names them.
            if boards {
                if board != Some(program.board) {
                    board = Some(program.board);
                    outln!("{}", board_heading(&project, program.board))?;
                }
                let name = outputs
                    .get(k)
                    .and_then(|o| o.file_name())
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
                outln!("Program {name} (T{}):", program.first_tool.number)?;
            } else {
                outln!("Program {} (T{}):", k + 1, program.first_tool.number)?;
            }
            let radius_mm = tool_radius_mm(&project, program);
            for line in describe_extents(&project, program, radius_mm) {
                outln!("  {line}")?;
            }
            outln!("  {}", estimates.program_line(k))?;
        }
        if let Some(total) = estimates.total_line() {
            outln!("{total}")?;
        }
    }
    // Addendum A5: the preview is written when every operation generated,
    // whatever the warnings, and even if the programs then failed.
    if let Some(preview_path) = &args.preview {
        match &generated {
            Some(generated) => {
                let svg = preview::render_generated(&project, generated);
                StagedFile::write(preview_path, svg.as_bytes())?.commit()?;
                outln!("Wrote preview {}.", preview_path.display())?;
            }
            None => outln!("Preview not written: not every operation generated a toolpath.")?,
        }
    }
    outln!(
        "{}: {} error(s), {} warning(s)",
        args.dcv.display(),
        findings.errors.len(),
        findings.warnings.len() + design.len()
    )?;
    Ok(if findings.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// The two findings of the import checks that still apply to a project
/// (`shapes_overlap`, `feature_below_size`), read from the regions of its
/// visible shapes with the design's height from their joint bounding box
/// (functional 49a). Shapes that do not resolve into regions are left out;
/// validation reports them.
fn design_findings(project: &Project) -> Vec<ImportWarning> {
    let mut shapes: Vec<(String, Vec<Region>)> = Vec::new();
    for instance in project.instances.iter().filter(|i| i.visible) {
        let Some(definition) = project.definitions.get(&instance.definition) else {
            continue;
        };
        let contours: Option<Vec<Contour>> = definition
            .profiles
            .iter()
            .filter(|p| p.is_closed())
            .map(|p| {
                let placed = instance.transform.apply_polyline(p).ok()?;
                Contour::new(placed).ok()
            })
            .collect();
        if let Some(Ok((regions, _))) = contours.map(nest)
            && !regions.is_empty()
        {
            shapes.push((instance.name.clone(), regions));
        }
    }
    let bounds = shapes
        .iter()
        .flat_map(|(_, regions)| regions)
        .filter_map(|r| r.outer().polyline().bounds())
        .reduce(|a, b| a.including(b.min).including(b.max));
    let Some(bounds) = bounds else {
        return Vec::new();
    };
    prep::design_findings_named(&shapes, bounds.height_mm())
}
