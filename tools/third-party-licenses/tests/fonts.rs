// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `FONT-LICENSES`, which every release archive carries, holds the licence
//! text of every font and icon set the programs bundle, as the source tree
//! has it. The Open Font License asks that its text travel with the font.

// Test helpers may panic on setup failure (docs/rust-guidelines.md, "Toolchain").
#![allow(clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};

use third_party_licenses::{ARCHIVE_FILES, FONT_LICENSES_FILE};

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Every licence file under a folder of bundled fonts or icons.
fn licence_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("the asset folder is readable");
    for entry in entries {
        let path = entry.expect("a folder entry").path();
        if path.is_dir() {
            licence_files(&path, found);
        } else {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("LICENSE") || name.starts_with("OFL") {
                found.push(path);
            }
        }
    }
}

#[test]
fn font_licenses_hold_every_bundled_licence_text() {
    let root = workspace_root();
    let held = fs::read_to_string(root.join(FONT_LICENSES_FILE)).expect("FONT-LICENSES is read");
    let mut files = Vec::new();
    for dir in [
        "crates/dc-text/fonts",
        "crates/dreamcarve-app/assets/fonts",
        "crates/dreamcarve-app/assets/icons",
    ] {
        licence_files(&root.join(dir), &mut files);
    }
    assert!(files.len() >= 8, "found the licence files: {files:?}");
    for file in files {
        let text = fs::read_to_string(&file).expect("a licence file is read");
        assert!(
            held.contains(&text),
            "{FONT_LICENSES_FILE} holds the text of {}",
            file.display()
        );
    }
}

#[test]
fn every_archive_carries_the_font_licences() {
    assert!(ARCHIVE_FILES.contains(&FONT_LICENSES_FILE));
}
