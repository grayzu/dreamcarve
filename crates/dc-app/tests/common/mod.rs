// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The committed projects and drawings the slice 2 tests run over
//! (`docs/specs/phase-4/design.md`, "Test plan → dc-app core (slice 2)"),
//! read from where they live: `tests/fixtures/`, `tests/golden/` and the
//! CLI's fixtures. Nothing is copied here.

#![allow(dead_code, reason = "each test binary uses the helpers it needs")]

use std::path::{Path, PathBuf};

use dc_import::SvgImportOptions;
use dc_job::Drawing;
use dc_project::Project;

/// The repository root.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The bytes of the file at `path`.
pub fn read(path: &Path) -> Vec<u8> {
    std::fs::read(path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// `tests/fixtures/<name>`.
pub fn fixture(name: &str) -> PathBuf {
    repo_root().join("tests/fixtures").join(name)
}

/// The project in the `.dcv` at `path`, migrated.
pub fn load_dcv(path: &Path) -> Project {
    let bytes = read(path);
    dc_project::load(&bytes)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .project
}

/// The quick-start plaque: ten shapes, five operations of four kinds, a
/// Profile with a tab request, three tools of which one is unused.
pub fn plaque_bytes() -> Vec<u8> {
    read(&fixture("plaque.dcv"))
}

/// The quick-start plaque, loaded.
pub fn plaque() -> Project {
    load_dcv(&fixture("plaque.dcv"))
}

/// `tests/fixtures/<name>.svg` imported at the default tolerance.
pub fn svg_drawing(name: &str) -> Drawing {
    let bytes = read(&fixture(&format!("{name}.svg")));
    let (import, _warnings) =
        dc_import::import_svg(&bytes, &SvgImportOptions::default()).expect("fixture imports");
    Drawing::Svg(import)
}

/// One committed project: its file stem and path.
pub type Fixture = (String, PathBuf);

/// The quick start chapters' projects, `tests/fixtures/*.dcv`.
pub fn quick_start_projects() -> Vec<Fixture> {
    ["plaque", "plaque-svg", "sign", "dc"]
        .into_iter()
        .map(|name| (name.to_owned(), fixture(&format!("{name}.dcv"))))
        .collect()
}

/// The golden cases that have a DreamCarve project,
/// `tests/golden/<case>/dreamcarve.dcv`, sorted.
pub fn golden_projects() -> Vec<Fixture> {
    let mut cases: Vec<Fixture> = std::fs::read_dir(repo_root().join("tests/golden"))
        .expect("tests/golden readable")
        .map(|e| e.expect("entry").path())
        .filter(|p| p.join("dreamcarve.dcv").is_file())
        .map(|p| {
            let name = p.file_name().expect("case").to_string_lossy().into_owned();
            (name, p.join("dreamcarve.dcv"))
        })
        .collect();
    cases.sort();
    assert!(cases.len() >= 20, "{} golden projects", cases.len());
    cases
}

/// The version 9 corpus projects with tab requests
/// (`crates/dreamcarve/tests/fixtures/tabs-migration/`), which exercise
/// the migrations.
pub fn corpus_projects() -> Vec<Fixture> {
    [
        "rect-outside-climb",
        "square-with-hole-on-holes",
        "circle-rotated",
        "star-with-hole",
        "two-parts-pass-first",
    ]
    .into_iter()
    .map(|name| {
        (
            name.to_owned(),
            repo_root()
                .join("crates/dreamcarve/tests/fixtures/tabs-migration")
                .join(format!("{name}.dcv")),
        )
    })
    .collect()
}

/// The three groups above.
pub fn checked_projects() -> Vec<Fixture> {
    let mut all = quick_start_projects();
    all.extend(golden_projects());
    all.extend(corpus_projects());
    all
}

/// The phase 1 coaster as `dreamcarve init fixtures/coaster.svg` writes
/// it: the stock round the drawing with `init`'s margin, the placeholder
/// tool and the Profile.
pub fn coaster() -> Project {
    let bytes = read(&repo_root().join("fixtures/coaster.svg"));
    let (import, _warnings) =
        dc_import::import_svg(&bytes, &SvgImportOptions::default()).expect("coaster imports");
    let post = dc_project::latest_definition("grblhal")
        .expect("grblHAL is bundled")
        .default_ref();
    dc_job::project_from::drawing(
        "coaster",
        Drawing::Svg(import),
        post,
        dc_project::Units::Mm,
        true,
    )
    .expect("init makes the coaster")
}

/// Asserts two transforms agree coefficient by coefficient within `tol`.
pub fn assert_same_transform(
    got: dc_geometry::Transform,
    want: dc_geometry::Transform,
    tol: f64,
    what: &str,
) {
    for (k, (a, b)) in got.coeffs().iter().zip(want.coeffs()).enumerate() {
        assert!(
            (a - b).abs() <= tol,
            "{what}: coefficient {k} is {a}, want {b} ({got:?} vs {want:?})"
        );
    }
}

/// Asserts two points agree within `tol` mm.
pub fn assert_near(got: dc_geometry::Point, want: dc_geometry::Point, tol: f64, what: &str) {
    assert!(
        got.distance_to(want) <= tol,
        "{what}: {got:?}, want {want:?}"
    );
}

/// The instance `id` of `project`.
pub fn instance_of<'a>(project: &'a Project, id: &str) -> &'a dc_project::FeatureInstance {
    project
        .instances
        .iter()
        .find(|i| i.id.as_str() == id)
        .unwrap_or_else(|| panic!("the project has instance {id}"))
}
