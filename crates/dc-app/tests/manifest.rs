// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Phase 4 functional spec, S20: the app's program is the CLI's, built by
//! the same `dc-job` call. Cargo enforces it: `dc-app` lists neither
//! `dc-cam` nor `dc-gcode`, in any dependency table.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test code reports failure by panicking"
)]

/// Every dependency key of `crates/dc-app/Cargo.toml`, from each
/// `[dependencies]`, `[dev-dependencies]`, `[build-dependencies]` and
/// target-specific table.
fn dependency_keys() -> Vec<String> {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("the manifest is readable");
    let mut keys = Vec::new();
    let mut in_dependencies = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            in_dependencies = line.contains("dependencies");
            continue;
        }
        if in_dependencies && let Some((key, _)) = line.split_once('=') {
            keys.push(key.trim().trim_matches('"').to_owned());
        }
    }
    keys
}

/// S20: neither `dc-cam` nor `dc-gcode` is a dependency of `dc-app`; the
/// types it needs come re-exported from `dc-job`.
#[test]
fn manifest_lists_neither_dc_cam_nor_dc_gcode() {
    let keys = dependency_keys();
    assert!(
        keys.iter().any(|k| k == "dc-job"),
        "dc-app depends on dc-job"
    );
    for forbidden in ["dc-cam", "dc-gcode"] {
        assert!(
            !keys.iter().any(|k| k == forbidden),
            "crates/dc-app/Cargo.toml lists {forbidden}: {keys:?}"
        );
    }
    for gui in ["egui", "eframe", "egui_wgpu", "wgpu", "winit"] {
        assert!(
            !keys.iter().any(|k| k == gui),
            "crates/dc-app/Cargo.toml lists {gui}: nothing GUI-specific in a dc-* crate"
        );
    }
}
