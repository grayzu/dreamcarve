// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve check <dcv>`: runs the export pipeline ([`export::build`])
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

use dc_gcode::ProgramSplit;

use crate::cli::{CheckArgs, Failure, StagedFile};
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
    let export::Built {
        programs,
        findings,
        generated,
    } = export::build(&project, ProgramSplit::ProjectDefault, CHECK_DATE);
    if let Some(preview) = &args.preview {
        // Where a plain `export` would write its programs; with none, the
        // one program's default path.
        let mut outputs = export::output_paths(None, &args.dcv, &programs)?;
        if programs.is_empty() {
            outputs = vec![
                args.dcv
                    .with_extension(&project.post_processor.file_extension),
            ];
        }
        export::check_preview_not_program(preview, &outputs)?;
    }
    let estimates = export::Estimates::new(&project, &programs, estimate_settings);

    for error in &findings.errors {
        outln!("{}", file.describe("error", error))?;
    }
    for warning in &findings.warnings {
        outln!("{}", file.describe("warning", warning))?;
    }
    // Phase 3: a carve's depth follows its shapes, so its deepest cut is
    // stated.
    for line in export::describe_carves(&project, generated.as_deref().unwrap_or_default()) {
        outln!("{line}")?;
    }
    // #62: where the program would take the tool, and capability 30: how
    // long it would run, as `export` prints them; for each program when
    // there is one per tool run (#95), then the total.
    if let [program] = programs.as_slice() {
        let radius_mm = export::largest_tool_radius_mm(&project);
        for line in export::describe_extents(&project, program, radius_mm) {
            outln!("{line}")?;
        }
        outln!("{}", estimates.program_line(0))?;
    } else {
        for (k, program) in programs.iter().enumerate() {
            outln!("Program {} (T{}):", k + 1, program.first_tool.number)?;
            let radius_mm = export::tool_radius_mm(&project, program);
            for line in export::describe_extents(&project, program, radius_mm) {
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
    if let Some(preview) = &args.preview {
        match &generated {
            Some(generated) => {
                let svg = export::preview_svg(&project, generated);
                StagedFile::write(preview, svg.as_bytes())?.commit()?;
                outln!("Wrote preview {}.", preview.display())?;
            }
            None => outln!("Preview not written: not every operation generated a toolpath.")?,
        }
    }
    outln!(
        "{}: {} error(s), {} warning(s)",
        args.dcv.display(),
        findings.errors.len(),
        findings.warnings.len()
    )?;
    Ok(if findings.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
