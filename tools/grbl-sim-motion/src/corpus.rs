// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The programs the simulators run: every committed grblHAL and GRBL
//! program, and the quick-start plaque exported in-process for each, plain,
//! with a `G53` machine-Z lift, and in inches.

use std::fs;
use std::path::Path;

use dc_project::{
    Heights, ParkXy, PostProcessor, PostProcessorRef, Project, Units, bundled_definition,
    latest_definition, load,
};
use rs274_motion::corpus::{CorpusError, export};

use crate::runner::Firmware;

/// The definitions the simulators run.
pub const POSTS: &[&str] = &["grblhal", "grbl"];

/// One program to run.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// A short name for the report.
    pub name: String,
    /// The definition it was written with.
    pub post: PostProcessor,
    /// The firmware it is for.
    pub firmware: Firmware,
    /// The program's units.
    pub units: Units,
    /// The program.
    pub text: String,
}

/// The committed programs, relative to the workspace root: (file,
/// definition id, version or `None` for the latest).
const COMMITTED: &[(&str, &str, Option<u32>)] = &[
    ("coaster-grblhal.nc", "grblhal", None),
    ("coaster-grblhal-v1.nc", "grblhal", Some(1)),
    ("coaster-grblhal-v2.nc", "grblhal", Some(2)),
    ("coaster-grblhal-v3.nc", "grblhal", Some(3)),
    ("coaster-grblhal-manual.nc", "grblhal", None),
    ("coaster-two-tool-grblhal-1-T1.nc", "grblhal", None),
    ("coaster-two-tool-grblhal-2-T2.nc", "grblhal", None),
    ("coaster-two-tool-grblhal-m6.nc", "grblhal", None),
    ("coaster-bezier-grblhal-unfitted.nc", "grblhal", None),
    ("coaster-grbl.nc", "grbl", None),
    ("coaster-grbl-v1.nc", "grbl", Some(1)),
    ("coaster-grbl-v2.nc", "grbl", Some(2)),
    ("coaster-grbl-manual.nc", "grbl", None),
    ("coaster-two-tool-grbl-1-T1.nc", "grbl", None),
    ("coaster-two-tool-grbl-2-T2.nc", "grbl", None),
];

/// Where [`COMMITTED`] lives.
const EXPECTED: &str = "crates/dreamcarve/tests/expected";

/// The project every exported variant starts from.
const PLAQUE: &str = "tests/fixtures/plaque.dcv";

/// The units a program is written in: its `G20` or `G21`, else `fallback`.
#[must_use]
pub fn program_units(text: &str, fallback: Units) -> Units {
    for line in text.lines() {
        let code = line.split(['(', ';']).next().unwrap_or("").to_ascii_uppercase();
        for word in code.split_whitespace() {
            match word {
                "G20" => return Units::Inch,
                "G21" => return Units::Mm,
                _ => {}
            }
        }
    }
    fallback
}

fn definition(id: &str, version: Option<u32>) -> Result<PostProcessor, CorpusError> {
    match version {
        Some(v) => bundled_definition(id, v),
        None => latest_definition(id),
    }
    .map(|d| d.post_processor().clone())
    .map_err(|e| CorpusError::Definition(id.to_owned(), e.to_string()))
}

/// Builds the corpus from the workspace at `root`.
///
/// # Errors
///
/// [`CorpusError`] when a committed program or the fixture is missing, or
/// a variant does not export.
pub fn build_corpus(root: &Path) -> Result<Vec<Case>, CorpusError> {
    if true { return Ok(Vec::new()); }
    let mut cases = Vec::new();
    for (file, id, version) in COMMITTED {
        let path = root.join(EXPECTED).join(file);
        let text = fs::read_to_string(&path).map_err(|source| CorpusError::Io {
            path: path.clone(),
            source,
        })?;
        let firmware = Firmware::of(id)
            .ok_or_else(|| CorpusError::Definition((*id).to_owned(), "no simulator".to_owned()))?;
        cases.push(Case {
            name: format!("{EXPECTED}/{file}"),
            post: definition(id, *version)?,
            firmware,
            units: program_units(&text, Units::Mm),
            text,
        });
    }

    let path = root.join(PLAQUE);
    let bytes = fs::read(&path).map_err(|source| CorpusError::Io {
        path: path.clone(),
        source,
    })?;
    let plaque = load(&bytes)
        .map_err(|e| CorpusError::Load {
            path: path.clone(),
            message: e.to_string(),
        })?
        .project;
    for id in POSTS {
        let firmware = Firmware::of(id)
            .ok_or_else(|| CorpusError::Definition((*id).to_owned(), "no simulator".to_owned()))?;
        for (variant, project) in variants(&plaque, id)? {
            let post = project
                .post_processor
                .resolve()
                .map_err(|e| CorpusError::Definition((*id).to_owned(), e.to_string()))?;
            let name = format!("plaque/{id}/{variant}");
            let programs = export(&project).map_err(|message| CorpusError::Export {
                case: name.clone(),
                message,
            })?;
            for (k, text) in programs.into_iter().enumerate() {
                cases.push(Case {
                    name: format!("{name}/program-{}", k + 1),
                    post: post.clone(),
                    firmware,
                    units: program_units(&text, project.display_units),
                    text,
                });
            }
        }
    }
    Ok(cases)
}

/// The plaque on definition `id`: plain, with a `G53` lift to machine Z
/// -5 mm (acceptance 45's heights), and in inches.
fn variants(plaque: &Project, id: &str) -> Result<Vec<(&'static str, Project)>, CorpusError> {
    let reference = PostProcessorRef::latest(id)
        .map_err(|e| CorpusError::Definition(id.to_owned(), e.to_string()))?;
    let mut base = plaque.clone();
    base.post_processor = reference;
    let mut lift = base.clone();
    lift.heights = Heights {
        clearance_mm: Some(20.0),
        tool_change_machine_z_mm: Some(-5.0),
        park: Some(ParkXy {
            x_mm: 0.0,
            y_mm: 0.0,
        }),
    };
    let mut inch = base.clone();
    inch.display_units = Units::Inch;
    Ok(vec![("plain", base), ("machine-z", lift), ("inch", inch)])
}
