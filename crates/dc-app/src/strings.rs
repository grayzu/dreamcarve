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
    ///
    /// A count's noun is written `{name|one|other}`: `one` where `args`
    /// gives `name` as `1`, `other` for every other value, so
    /// `{count} {count|tool|tools}` reads *1 tool* and *3 tools*. Every
    /// message that counts is written this way, and the table's test
    /// `no_message_reads_one_things` fails one that is not. Two forms are
    /// English's; a language with more takes its own rule here in phase 8.
    #[must_use]
    pub fn fmt(&self, key: StrKey, args: &[(&str, &str)]) -> String {
        let text = self.get(key);
        let value = |name: &str| args.iter().find(|(n, _)| *n == name).map(|(_, v)| *v);
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(open) = rest.find('{') {
            let (before, from_brace) = rest.split_at(open);
            out.push_str(before);
            match from_brace.find('}') {
                Some(close) => {
                    let inner = from_brace.get(1..close).unwrap_or_default();
                    let mut parts = inner.split('|');
                    let name = parts.next().unwrap_or_default();
                    match (value(name), parts.next(), parts.next()) {
                        (Some(count), Some(one), Some(other)) => {
                            out.push_str(if count == "1" { one } else { other });
                        }
                        (Some(value), None, _) => out.push_str(value),
                        _ => out.push_str(from_brace.get(..=close).unwrap_or_default()),
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
    fn fmt_picks_a_counts_noun_by_its_value() {
        let strings = Strings::english();
        let added = |n: &str| strings.fmt(StrKey::AppLibraryAdded, &[("count", n)]);
        assert_eq!(added("1"), "Added 1 tool from the library");
        assert_eq!(added("2"), "Added 2 tools from the library");
        assert_eq!(added("0"), "Added 0 tools from the library");
        let footer = |n: &str, ticked: &str| {
            strings.fmt(
                StrKey::AppLibraryFooter,
                &[("count", n), ("selected", ticked), ("name", "plaque")],
            )
        };
        assert_eq!(footer("1", "1"), "1 tool · 1 ticked · ✓ already in plaque");
        assert_eq!(
            footer("12", "0"),
            "12 tools · 0 ticked · ✓ already in plaque"
        );
        // A count the caller does not give stays as written.
        assert_eq!(
            strings.fmt(StrKey::AppLibraryAdded, &[]),
            "Added {count} {count|tool|tools} from the library"
        );
    }

    /// The names of the placeholders in `text`, a count's noun under its
    /// count's name.
    fn placeholders(text: &str) -> Vec<&str> {
        text.split('{')
            .skip(1)
            .filter_map(|part| part.split('}').next())
            .filter_map(|inner| inner.split('|').next())
            .collect()
    }

    /// Whether `word` reads as a plural after a number: it ends in `s` and
    /// is not one of the table's singular words that do.
    fn plural(word: &str) -> bool {
        const SINGULAR: [&str; 4] = ["pass", "blocks", "is", "has"];
        let word = word.trim_matches(|c: char| !c.is_alphabetic());
        word.len() > 1 && word.ends_with('s') && !SINGULAR.contains(&word)
    }

    /// No message reads "1 tools": every entry of the table, each of its
    /// placeholders given as `1`, has no plural in the two words after the
    /// number ("1 tools", "1 clearing passes", "1 locked shapes"). A
    /// number that is never a count (a pass's index, a version) has no
    /// noun after it, so it passes untouched.
    #[test]
    fn no_message_reads_one_things() {
        let strings = Strings::english();
        let mut wrong = Vec::new();
        for &key in StrKey::ALL {
            let names = placeholders(key.english());
            let args: Vec<(&str, &str)> = names.iter().map(|name| (*name, "1")).collect();
            let text = strings.fmt(key, &args);
            let words: Vec<&str> = text.split_whitespace().collect();
            for (i, word) in words.iter().enumerate() {
                if *word != "1" {
                    continue;
                }
                let first = words.get(i + 1).copied().unwrap_or_default();
                let second = words.get(i + 2).copied().unwrap_or_default();
                // The second word counts only as part of the first's
                // phrase: not past a separator or a punctuation mark.
                // A preposition ends it too: `1 mm between passes` counts
                // millimetres, not passes.
                let phrase_goes_on =
                    first.chars().all(char::is_alphabetic) && first != "tool" && first != "between";
                if plural(first) || (phrase_goes_on && plural(second)) {
                    wrong.push(format!("{}: {text}", key.key()));
                }
            }
        }
        assert!(
            wrong.is_empty(),
            "these read \"1 things\":\n{}",
            wrong.join("\n")
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
