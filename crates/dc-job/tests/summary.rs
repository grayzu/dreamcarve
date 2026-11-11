// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 28 of phase 4 (`docs/specs/phase-4/design.md`, "Warnings", carve
//! notes; #620): the carve summary `check` prints is built from
//! [`dc_job::summary::carve_facts`], and its text does not change by a
//! byte. The expected text under `tests/fixtures/carves/` is what
//! `describe_carves` printed for each committed project before the facts
//! existed; the CLI's own tests (`crates/dreamcarve/tests/cli.rs`) are the
//! second guard.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::PathBuf;

use dc_job::summary::{carve_facts, describe_carves, format_carves};
use dc_job::{ProgramSplit, build};
use dc_project::{Project, Units};

use common::{DATE, checked_projects, load_dcv, repo_root};

/// Every committed project `check` runs on, read where it lives, each also
/// in inches (the summary states lengths in the display units): the
/// quick-start projects, the golden cases, the tab corpus, the Inlay
/// migration pairs and the format 26 drawn-boundary Inlay.
fn projects() -> Vec<(String, Project)> {
    let mut fixtures = checked_projects();
    let migration = repo_root().join("crates/dreamcarve/tests/fixtures/inlay-migration");
    for stem in ["dc-female", "dc-male", "star-female", "star-male"] {
        fixtures.push((
            format!("inlay-migration-{stem}"),
            migration.join(format!("{stem}.dcv")),
        ));
    }
    fixtures.push((
        "dc-boundary".to_owned(),
        repo_root().join("tests/fixtures/dc-boundary.dcv"),
    ));
    let mut out = Vec::new();
    for (stem, path) in fixtures {
        let project = load_dcv(&path);
        let mut inch = project.clone();
        inch.display_units = Units::Inch;
        out.push((stem.clone(), project));
        out.push((format!("{stem}-in"), inch));
    }
    out
}

/// What `check` hands `describe_carves`: the runs of a full build.
fn generated(project: &Project) -> Vec<dc_job::EmitOp> {
    build(project, ProgramSplit::ProjectDefault, DATE)
        .generated
        .unwrap_or_default()
}

/// The expected text of `stem`, or `None` when the project carves nothing.
fn expected(stem: &str) -> Option<String> {
    let path: PathBuf = repo_root()
        .join("crates/dc-job/tests/fixtures/carves")
        .join(format!("{stem}.txt"));
    std::fs::read_to_string(path).ok()
}

/// Design spec, slice 28: for every committed project `check` runs on,
/// `describe_carves` prints exactly what it printed before it became a
/// formatter over `carve_facts` (the files were written by the code before
/// the change). A project with no file carves nothing. The projects that
/// carve include a V-Carve with no flat depth and one with clearing (the
/// sign), the one-file Inlay with its male cut-out (`dc`), the drawn
/// boundary (`dc-boundary`) and the two-file pairs of format 24.
#[test]
fn describe_carves_is_unchanged() {
    let mut carving = 0;
    for (stem, project) in projects() {
        let lines = describe_carves(&project, &generated(&project));
        let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
        match expected(&stem) {
            Some(want) => {
                carving += 1;
                assert_eq!(text, want, "{stem}: check's carve lines changed");
            }
            None => assert!(lines.is_empty(), "{stem}: {lines:?}"),
        }
    }
    assert!(carving >= 20, "{carving} carving projects");
}

/// Design spec, slice 28: `carve_facts` is the structure, and formatting
/// it gives `describe_carves`' strings, line for line; each carving
/// operation has one entry, with a board per line it prints (two for an
/// Inlay that cuts both), and every board leads with its deepest cut.
#[test]
fn carve_facts_format_to_describe_carves() {
    for (stem, project) in projects() {
        let generated = generated(&project);
        let facts = carve_facts(&project, &generated);
        assert_eq!(
            format_carves(&facts),
            describe_carves(&project, &generated),
            "{stem}"
        );
        for carve in &facts {
            assert!(!carve.boards.is_empty(), "{stem}: {}", carve.op);
            if carve.kind == dc_job::summary::CarveKind::Photo {
                continue;
            }
            for board in &carve.boards {
                assert_eq!(
                    board.facts.first().map(dc_job::summary::Fact::key),
                    Some(dc_job::summary::FactKey::DeepestCut),
                    "{stem}: {}",
                    carve.op
                );
            }
        }
    }
    let sign = load_dcv(&repo_root().join("tests/fixtures/sign.dcv"));
    let facts = carve_facts(&sign, &generated(&sign));
    let ops: Vec<&str> = facts.iter().map(|c| c.op.as_str()).collect();
    assert_eq!(ops, ["op2", "op3"], "the sign's two V-Carves");
    let dc = load_dcv(&repo_root().join("tests/fixtures/dc.dcv"));
    let facts = carve_facts(&dc, &generated(&dc));
    let boards: Vec<Option<dc_project::Board>> = facts
        .iter()
        .flat_map(|c| c.boards.iter().map(|b| b.board))
        .collect();
    assert_eq!(
        boards,
        [
            Some(dc_project::Board::Female),
            Some(dc_project::Board::Male)
        ]
    );
}

/// The project of `tests/fixtures/photo/gradient.png` as *New Project from
/// Picture…* makes it, 100 mm wide, with what export needs set: the
/// stock's thickness, the carve's maximum depth and the V-bit's rates.
fn photo_project() -> Project {
    let bytes = std::fs::read(repo_root().join("tests/fixtures/photo/gradient.png")).unwrap();
    let (pixels, _) = dc_import::bitmap::import(&bytes, dc_import::ImageFormat::Png).unwrap();
    let mut project = dc_job::project_from::picture(
        "gradient",
        &pixels,
        dc_job::picture::PictureSize::Width { width_mm: 100.0 },
        dc_project::PostProcessorRef::latest("grblhal").unwrap(),
        Units::Mm,
    )
    .unwrap();
    project.stock.thickness_mm = Some(12.0);
    let tool = &mut project.tools[0];
    tool.feed_mm_min = Some(1000.0);
    tool.plunge_mm_min = Some(300.0);
    tool.rpm = Some(18000);
    let Some(dc_project::Operation::PhotoVCarve(op)) = project.operations.first_mut() else {
        panic!("one Photo V-Carve");
    };
    op.max_depth_mm = Some(2.0);
    project
}

/// Slice 28 after #646: the Photo V-Carve's line, built from its facts, is
/// the line #646 printed, word for word: that line's format, filled from
/// the same layout (`dc_cam::photo::layout`), is the expectation.
#[test]
fn photo_line_is_unchanged() {
    let project = photo_project();
    let generated = generated(&project);
    assert!(!generated.is_empty(), "the picture generates");
    let Some(dc_project::Operation::PhotoVCarve(photo)) = project.operations.first() else {
        panic!("one Photo V-Carve");
    };
    let layout = dc_cam::photo::layout(&project, photo).expect("its layout");
    let bitmap = &project.bitmaps[0];
    let length = |mm: f64| dc_job::summary::describe_length(&project, mm);
    let footprint = layout.footprint;
    let want = format!(
        "Photo V-Carve {} `{}`: picture `{}` ({}), {} by {} at ({}, {}), {} grooves {} wide at the full depth of {}.",
        photo.id,
        photo.name,
        bitmap.name,
        bitmap.id,
        length(footprint.width_mm()),
        length(footprint.height_mm()),
        length(footprint.min.x),
        length(footprint.min.y),
        layout.grooves,
        length(layout.width_mm),
        length(2.0),
    );
    assert_eq!(describe_carves(&project, &generated), [want]);
    let facts = carve_facts(&project, &generated);
    assert_eq!(format_carves(&facts), describe_carves(&project, &generated));
}
