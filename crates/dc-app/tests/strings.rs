// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Slice 2 of phase 4: the string table (functional acceptance 91). Every
//! key in `strings/en.toml` is used by a GUI crate, every used key exists
//! (the compiler: `StrKey` is generated from the table), and placeholders
//! are filled.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::collections::BTreeSet;
use std::path::Path;

use dc_app::{StrKey, Strings};

/// The dotted keys of `strings/en.toml`, in table order.
fn table_keys() -> Vec<String> {
    let text = std::fs::read_to_string(common::repo_root().join("crates/dc-app/strings/en.toml"))
        .expect("the string table is readable");
    let mut keys = Vec::new();
    let mut section = String::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            section = name.trim().to_owned();
        } else if let Some((key, _)) = line.split_once('=') {
            let key = key.trim();
            keys.push(if section.is_empty() {
                key.to_owned()
            } else {
                format!("{section}.{key}")
            });
        }
    }
    keys
}

/// `command.label.set_stock` as `build.rs` names it: `CommandLabelSetStock`.
fn variant_name(key: &str) -> String {
    key.split(['.', '_'])
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                None => String::new(),
            }
        })
        .collect()
}

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Every `StrKey::<Variant>` token in the two GUI crates' sources, `dc-app`
/// and the shell, `dreamcarve-app` (#532).
fn used_variants() -> BTreeSet<String> {
    let mut files = Vec::new();
    for crate_dir in ["crates/dc-app/src", "crates/dreamcarve-app/src"] {
        let before = files.len();
        rust_files(&common::repo_root().join(crate_dir), &mut files);
        assert!(files.len() > before, "no sources scanned in {crate_dir}");
    }
    let mut used = BTreeSet::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("source is readable");
        for (k, _) in text.match_indices("StrKey::") {
            let rest = &text[k + "StrKey::".len()..];
            let variant: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if !variant.is_empty() && variant != "ALL" {
                used.insert(variant);
            }
        }
    }
    used
}

/// Acceptance 91: the table and the generated enum agree, and every key is
/// used somewhere in a GUI crate.
#[test]
fn every_key_is_used() {
    let keys = table_keys();
    assert!(!keys.is_empty(), "the table is empty");
    let generated: BTreeSet<&str> = StrKey::ALL.iter().map(|k| k.key()).collect();
    let table: BTreeSet<&str> = keys.iter().map(String::as_str).collect();
    assert_eq!(generated, table, "StrKey does not match strings/en.toml");
    assert_eq!(StrKey::ALL.len(), keys.len(), "duplicate keys in the table");

    let used = used_variants();
    let unused: Vec<&String> = keys
        .iter()
        .filter(|key| !used.contains(&variant_name(key)))
        .collect();
    assert!(unused.is_empty(), "keys nothing uses: {unused:?}");
}

/// Acceptance 91 for the setup sheet (#625; capability 77b): its labels
/// are the string table's `[setup_sheet]` entries, one per
/// `dc_job::setup_sheet::Label` and no other, and the app hands the
/// renderer the table's text, a translation's included.
#[test]
fn setup_sheet_strings_are_keyed() {
    use dc_job::setup_sheet::Label;

    let table: BTreeSet<String> = table_keys()
        .into_iter()
        .filter(|k| k.starts_with("setup_sheet."))
        .collect();
    let labels: BTreeSet<String> = Label::ALL.iter().map(|l| l.key().to_owned()).collect();
    assert!(!labels.is_empty());
    assert_eq!(labels, table, "the labels are the [setup_sheet] keys");

    let keys: Vec<StrKey> = StrKey::ALL
        .iter()
        .copied()
        .filter(|k| k.key().starts_with("setup_sheet."))
        .collect();
    let english = Strings::english();
    let marked = Strings::with(keys.iter().map(|&k| (k, format!("<<{}>>", k.key()))));
    for &label in Label::ALL {
        let key = keys
            .iter()
            .copied()
            .find(|k| k.key() == label.key())
            .unwrap_or_else(|| panic!("no StrKey for {}", label.key()));
        assert_eq!(
            dc_app::export::setup_sheet_text(&english, label),
            english.get(key),
            "{}",
            label.key()
        );
        assert_eq!(
            dc_app::export::setup_sheet_text(&marked, label),
            format!("<<{}>>", label.key()),
            "a translation reaches the sheet"
        );
    }
}

/// `Strings::fmt` fills every `{name}` placeholder, leaves text without
/// one alone, and every value in the table has only placeholders that can
/// be named.
#[test]
fn placeholders_are_filled() {
    let strings = Strings::default();
    assert_eq!(
        strings.fmt(StrKey::CommandSubjectOne, &[("name", "Oval")]),
        "“Oval”"
    );
    assert_eq!(
        strings.fmt(StrKey::CommandLabelMove, &[("subject", "“Oval”")]),
        "Move “Oval”"
    );
    assert_eq!(
        strings.fmt(StrKey::CommandSubjectMany, &[("count", "3")]),
        "3 shapes"
    );
    assert_eq!(
        strings.get(StrKey::CommandLabelReorderOperations),
        "Reorder operations"
    );
    assert_eq!(
        strings.fmt(StrKey::CommandLabelReorderOperations, &[("name", "x")]),
        "Reorder operations"
    );

    for &key in StrKey::ALL {
        let text = strings.get(key);
        let names: Vec<String> = text
            .match_indices('{')
            .map(|(k, _)| {
                let rest = &text[k + 1..];
                let end = rest
                    .find('}')
                    .unwrap_or_else(|| panic!("{}: unclosed brace", key.key()));
                rest[..end].to_owned()
            })
            .collect();
        for name in &names {
            assert!(
                !name.is_empty() && name.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{}: placeholder {{{name}}} is not a name",
                key.key()
            );
        }
        let args: Vec<(&str, &str)> = names.iter().map(|n| (n.as_str(), "X")).collect();
        let filled = strings.fmt(key, &args);
        assert!(
            !filled.contains('{') && !filled.contains('}'),
            "{}: {filled:?} keeps a placeholder",
            key.key()
        );
    }
}
