// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve check <dcv>`: runs the export pipeline ([`export::build`])
//! without writing anything and prints every error and warning it finds,
//! including each open path, which validation reports because Profile never
//! cuts it (functional spec, capabilities 4 and 8), then, when the program
//! could be built, its extents as `export` prints them (#62). Exits 1 when
//! any error was found.
//!
//! Every value the user has yet to set is an error, so one run lists all of
//! them, each as `coaster.dcv:line:column: error: …` (#76).

use std::process::ExitCode;

use crate::cli::{CheckArgs, Failure};
use crate::export;

/// The date filled into the discarded program. It never reaches a file.
const CHECK_DATE: &str = "check";

/// Runs `check`.
pub fn run(args: &CheckArgs) -> Result<ExitCode, Failure> {
    let (project, file) = export::load_project(&args.dcv)?;
    let export::Built { program, findings } = export::build(&project, CHECK_DATE);

    for error in &findings.errors {
        println!("{}", file.describe("error", error));
    }
    for warning in &findings.warnings {
        println!("{}", file.describe("warning", warning));
    }
    // #62: where the program would take the tool, as `export` prints it.
    if let Some(program) = &program {
        for line in export::describe_extents(&project, program) {
            println!("{line}");
        }
    }
    println!(
        "{}: {} error(s), {} warning(s)",
        args.dcv.display(),
        findings.errors.len(),
        findings.warnings.len()
    );
    Ok(if findings.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
