// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Phase 6 functional spec, S22: the simulation is a reader. No
//! generator and no post-processor can read one, and Cargo holds them to
//! it: `dc-sim` knows nothing above `dc-project`, `dc-gcode` does not
//! list it, and `dc-cam` lists it only for its tests (`dev-dependencies`
//! and the optional dependency its `test-support` feature turns on),
//! naming it in the one module those gate.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code reports failure by panicking"
)]

use std::path::{Path, PathBuf};

fn crates_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/dc-sim has a parent")
        .to_owned()
}

/// Every dependency line of a crate's manifest as `(table, key, rest)`.
fn dependencies(krate: &str) -> Vec<(String, String, String)> {
    let path = crates_dir().join(krate).join("Cargo.toml");
    let manifest = std::fs::read_to_string(&path).expect("the manifest is readable");
    let mut found = Vec::new();
    let mut table = String::new();
    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            table = line.trim_matches(['[', ']']).to_owned();
            continue;
        }
        if table.contains("dependencies")
            && let Some((key, rest)) = line.split_once('=')
        {
            let key = key.trim().trim_matches('"');
            let key = key.split('.').next().unwrap_or(key);
            found.push((table.clone(), key.to_owned(), rest.trim().to_owned()));
        }
    }
    found
}

#[test]
fn dc_sim_knows_nothing_above_the_project() {
    let keys: Vec<String> = dependencies("dc-sim").into_iter().map(|d| d.1).collect();
    for needed in ["dc-geometry", "dc-project"] {
        assert!(keys.iter().any(|k| k == needed), "{needed} in {keys:?}");
    }
    for forbidden in ["dc-cam", "dc-gcode", "dc-job", "dc-app", "dc-import"] {
        assert!(
            !keys.iter().any(|k| k == forbidden),
            "crates/dc-sim/Cargo.toml lists {forbidden}: {keys:?}"
        );
    }
}

#[test]
fn the_post_processor_does_not_list_the_simulation() {
    let listed = dependencies("dc-gcode")
        .into_iter()
        .any(|d| d.1 == "dc-sim");
    assert!(!listed, "crates/dc-gcode/Cargo.toml lists dc-sim");
}

/// `dc-cam` may name `dc-sim` in two places: an optional dependency (the
/// `test-support` feature's) and a dev-dependency. A plain dependency
/// would let a generator read a simulation.
#[test]
fn the_generators_list_the_simulation_for_tests_only() {
    let entries: Vec<_> = dependencies("dc-cam")
        .into_iter()
        .filter(|d| d.1 == "dc-sim")
        .collect();
    assert_eq!(entries.len(), 2, "{entries:?}");
    for (table, _, rest) in &entries {
        match table.as_str() {
            "dependencies" => assert!(
                rest.contains("optional = true"),
                "dc-cam's dependency on dc-sim must be optional: {rest}"
            ),
            "dev-dependencies" => {}
            other => panic!("dc-cam names dc-sim under [{other}]"),
        }
    }
}

/// The one module of `dc-cam` that names `dc_sim` is its test
/// heightfield, which `lib.rs` compiles for tests and `test-support`
/// alone.
#[test]
fn only_the_test_heightfield_names_the_simulation() {
    fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("a source directory") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                walk(&path, found);
            } else if path.extension().is_some_and(|e| e == "rs")
                && std::fs::read_to_string(&path)
                    .expect("a source file")
                    .contains("dc_sim")
            {
                found.push(path);
            }
        }
    }
    let src = crates_dir().join("dc-cam").join("src");
    let mut found = Vec::new();
    walk(&src, &mut found);
    assert_eq!(found, vec![src.join("heightfield.rs")]);
    let lib = std::fs::read_to_string(src.join("lib.rs")).expect("dc-cam's lib.rs");
    assert!(
        lib.contains("#[cfg(any(test, feature = \"test-support\"))]\nmod heightfield;"),
        "dc-cam's heightfield module must stay gated to tests"
    );
}
