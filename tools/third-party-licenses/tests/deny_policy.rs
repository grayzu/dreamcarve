// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The licence policy in `deny.toml` (#581): BSL-1.0 is admitted for the
//! app's clipboard crates by name, never globally. `cargo deny check`
//! (CI's `cargo-deny` job) enforces the policy; this test guards the
//! policy itself, so a later change cannot widen it unnoticed.

// Test helpers may panic on setup failure (docs/rust-guidelines.md, "Toolchain").
#![allow(clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

fn deny_toml() -> toml::Table {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deny.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    toml::from_str(&text).expect("deny.toml is TOML")
}

/// The strings in array `value`.
fn strings(value: &toml::Value) -> Vec<String> {
    value
        .as_array()
        .expect("an array")
        .iter()
        .map(|v| v.as_str().expect("a string").to_owned())
        .collect()
}

/// `[licenses] allow` never names BSL-1.0, and `[licenses] exceptions`
/// admits it for exactly `clipboard-win` and `error-code` (eframe
/// `clipboard` → egui-winit → arboard → clipboard-win → error-code,
/// Windows only) and nothing else.
#[test]
fn bsl_is_allowed_only_for_the_clipboard_crates() {
    let deny = deny_toml();
    let licenses = deny
        .get("licenses")
        .and_then(toml::Value::as_table)
        .expect("a [licenses] table");
    let allow = strings(licenses.get("allow").expect("an allow list"));
    assert!(
        !allow.iter().any(|l| l == "BSL-1.0"),
        "BSL-1.0 is never allowed globally: {allow:?}"
    );

    let exceptions = licenses
        .get("exceptions")
        .and_then(toml::Value::as_array)
        .expect("an exceptions list");
    let mut bsl: Vec<String> = exceptions
        .iter()
        .filter_map(|e| {
            let e = e.as_table().expect("an exception is a table");
            let allow = strings(e.get("allow").expect("an exception's allow"));
            allow.iter().any(|l| l == "BSL-1.0").then(|| {
                assert_eq!(
                    allow,
                    ["BSL-1.0"],
                    "a BSL-1.0 exception admits only BSL-1.0"
                );
                e.get("crate")
                    .and_then(toml::Value::as_str)
                    .expect("an exception names its crate")
                    .to_owned()
            })
        })
        .collect();
    bsl.sort();
    assert_eq!(
        bsl,
        ["clipboard-win", "error-code"],
        "BSL-1.0 is admitted for the clipboard crates by name, and only them"
    );
}
