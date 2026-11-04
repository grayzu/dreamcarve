// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Generates `StrKey` from `strings/en.toml` (`docs/specs/phase-4/design.md`,
//! "Strings"), so a key that is not in the table fails to compile.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=strings/en.toml");
}
