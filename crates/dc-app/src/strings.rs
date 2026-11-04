// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The string table (`docs/specs/phase-4/design.md`, "Strings"; functional
//! spec, capability 80). Every user-visible string of the app is an entry
//! of `strings/en.toml`, keyed `panel.section.item`; `build.rs` generates
//! [`StrKey`] from it, so a key that is not in the table fails to compile.
//! English is the only language shipped; a [`Strings`] built with another
//! table's entries falls back to English for any key it lacks, so a
//! translation can be partial.

use std::collections::BTreeMap;

include!(concat!(env!("OUT_DIR"), "/str_key.rs"));

/// The strings the app shows: English, or another table over it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Strings {
    /// Entries that replace the English text.
    overrides: BTreeMap<StrKey, String>,
}

impl Strings {
    /// The English table compiled into the binary.
    #[must_use]
    pub fn english() -> Self {
        Self::default()
    }

    /// English with `entries` laid over it, for a translation (phase 8).
    #[must_use]
    pub fn with(entries: impl IntoIterator<Item = (StrKey, String)>) -> Self {
        Self {
            overrides: entries.into_iter().collect(),
        }
    }

    /// The text of `key`, placeholders as written.
    #[must_use]
    pub fn get(&self, key: StrKey) -> &str {
        self.overrides
            .get(&key)
            .map_or_else(|| key.english(), String::as_str)
    }

    /// The text of `key` with each `{name}` replaced by the value `args`
    /// gives for `name`. A placeholder `args` does not name stays as
    /// written, so a missing argument shows rather than vanishes.
    #[must_use]
    pub fn fmt(&self, key: StrKey, args: &[(&str, &str)]) -> String {
        let text = self.get(key);
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let (before, from_brace) = rest.split_at(open);
            out.push_str(before);
            match from_brace.find('}') {
                Some(close) => {
                    let name = from_brace.get(1..close).unwrap_or_default();
                    match args.iter().find(|(n, _)| *n == name) {
                        Some((_, value)) => out.push_str(value),
                        None => out.push_str(from_brace.get(..=close).unwrap_or_default()),
                    }
                    rest = from_brace.get(close + 1..).unwrap_or_default();
                }
                None => {
                    out.push_str(from_brace);
                    rest = "";
                }
            }
        }
        out.push_str(rest);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_returns_english_unless_overridden() {
        let strings = Strings::english();
        assert_eq!(strings.get(StrKey::CommandLabelSetStock), "Set stock");
        let french = Strings::with([(StrKey::CommandLabelSetStock, "Définir le brut".to_owned())]);
        assert_eq!(french.get(StrKey::CommandLabelSetStock), "Définir le brut");
        assert_eq!(
            french.get(StrKey::CommandLabelFitStock),
            "Fit stock to shapes"
        );
    }

    #[test]
    fn fmt_fills_named_placeholders_and_keeps_unknown_ones() {
        let strings = Strings::english();
        assert_eq!(
            strings.fmt(
                StrKey::CommandLabelSetField,
                &[("field", "depth"), ("subject", "“A”")]
            ),
            "Set depth of “A”"
        );
        assert_eq!(
            strings.fmt(StrKey::CommandLabelSetField, &[("field", "depth")]),
            "Set depth of {subject}"
        );
    }

    #[test]
    fn keys_and_variants_agree() {
        for &key in StrKey::ALL {
            assert!(!key.key().is_empty());
            assert!(!key.english().is_empty(), "{}", key.key());
        }
    }
}
