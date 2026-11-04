// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Program file names, as the CLI writes them (`docs/specs/phase-4/design.md`,
//! "Data model → dc-job"; `export`, #95, #498). A project whose operations
//! use one tool is one program, `<stem>.<ext>`. Programs of tool runs go
//! beside it, named `<stem>-<k>-T<n>.<ext>`: `<k>` the run's place, `<n>`
//! its tool's number. With an enabled Inlay ([`by_board`]) every program
//! is named after its board, `<stem>-female-<k>-T<n>.<ext>` and
//! `<stem>-male-<k>-T<n>.<ext>`, `<k>` counting from 1 within each board,
//! however many there are.

use dc_gcode::Program;
use dc_project::Project;

use crate::summary::by_board;

/// What each program's name adds to the stem, in program order: nothing
/// for a project's one program, `-<k>-T<n>` for a tool run's, and
/// `-<board>-<k>-T<n>` with an Inlay (#498). The caller adds the
/// extension, so a shell that writes under another extension (`-o`)
/// names its files the same way.
#[must_use]
pub fn program_suffixes(project: &Project, programs: &[Program]) -> Vec<String> {
    let boards = by_board(project);
    if programs.len() < 2 && !boards {
        return programs.iter().map(|_| String::new()).collect();
    }
    programs
        .iter()
        .enumerate()
        .map(|(k, program)| {
            if boards {
                let k = programs
                    .iter()
                    .take(k)
                    .filter(|p| p.board == program.board)
                    .count();
                format!(
                    "-{}-{}-T{}",
                    program.board.as_str(),
                    k + 1,
                    program.first_tool.number
                )
            } else {
                format!("-{}-T{}", k + 1, program.first_tool.number)
            }
        })
        .collect()
}

/// The file name of each program under `stem`, with its post-processor's
/// extension: `coaster.nc`; `coaster-1-T1.nc`, `coaster-2-T2.nc`;
/// `dc-female-1-T2.nc`, `dc-male-1-T2.nc`.
#[must_use]
pub fn program_names(project: &Project, stem: &str, programs: &[Program]) -> Vec<String> {
    program_suffixes(project, programs)
        .into_iter()
        .zip(programs)
        .map(|(suffix, program)| format!("{stem}{suffix}.{}", program.extension))
        .collect()
}
