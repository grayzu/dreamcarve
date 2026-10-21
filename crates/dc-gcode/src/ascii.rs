// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Transliteration of the text DreamCarve writes into comments to
//! printable ASCII (`docs/specs/phase-2/design.md`, "Phase 2 addendum
//! (#73)", A2; functional capability 28).
//!
//! The emitter's comment sanitizer (`emit::sanitize_comment`) maps each
//! non-ASCII character through [`transliterate`] before its filter, so
//! "Café" is written `Cafe`, not `Caf`. The table is DreamCarve's own,
//! written by hand and reviewed, with no dependency.

/// The printable ASCII text that stands for non-ASCII character `c` in a
/// comment, or `None` when the table has no entry for it (and for ASCII
/// characters, which the sanitizer keeps or drops itself).
pub(crate) fn transliterate(_c: char) -> Option<&'static str> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The characters a comment may not hold: `( ) ;` end one early,
    /// `! ? ~` are real-time commands on GRBL and grblHAL even inside a
    /// comment, `{ }` are placeholder syntax, `%` is a program delimiter to
    /// some senders (`emit::sanitize_comment`).
    const SPECIALS: [char; 9] = ['(', ')', ';', '!', '?', '~', '{', '}', '%'];

    /// Every entry, over every Unicode scalar value, is printable ASCII
    /// without a comment special, so a transliteration can never end a
    /// comment or send a real-time command. Spot checks that the table is
    /// not empty, and that ASCII has no entry.
    #[test]
    fn table_never_produces_comment_specials() {
        let mut entries = 0;
        for c in (0..=0x10_FFFF).filter_map(char::from_u32) {
            let Some(text) = transliterate(c) else {
                continue;
            };
            entries += 1;
            assert!(!c.is_ascii(), "ASCII {c:?} has an entry {text:?}");
            assert!(
                text.chars().all(|t| (' '..='~').contains(&t)),
                "{c:?} (U+{:04X}) → {text:?} is not printable ASCII",
                u32::from(c)
            );
            assert!(
                !text.contains(SPECIALS),
                "{c:?} (U+{:04X}) → {text:?} holds a comment special",
                u32::from(c)
            );
        }
        // Latin-1 letters and Latin Extended-A alone are more than 150.
        assert!(entries > 150, "only {entries} entries");
    }

    /// Letters with accents and strokes lose them; ligatures and sharp s
    /// spell out (functional capability 28: "Straße" → `Strasse`).
    #[test]
    fn latin1_letters_lose_accents() {
        for (c, ascii) in [
            ('é', "e"),
            ('É', "E"),
            ('Ø', "O"),
            ('ø', "o"),
            ('ß', "ss"),
            ('Æ', "AE"),
            ('æ', "ae"),
            ('Œ', "OE"),
            ('ñ', "n"),
            ('ç', "c"),
            ('ü', "u"),
            ('Å', "A"),
            ('Ł', "L"),
            ('ł', "l"),
            ('Č', "C"),
            ('ő', "o"),
            ('ž', "z"),
        ] {
            assert_eq!(transliterate(c), Some(ascii), "{c:?}");
        }
    }

    /// The symbols a workshop writes spell out as the design lists them,
    /// and dashes, quotes and the ellipsis become their ASCII forms.
    #[test]
    fn fractions_and_symbols_spell_out() {
        for (c, ascii) in [
            ('½', "1/2"),
            ('¼', "1/4"),
            ('¾', "3/4"),
            ('°', "deg"),
            ('×', "x"),
            ('±', "+-"),
            ('µ', "u"),
            ('²', "2"),
            ('³', "3"),
            ('™', "TM"),
            ('—', "-"),
            ('–', "-"),
            ('‘', "'"),
            ('’', "'"),
            ('“', "\""),
            ('”', "\""),
            ('…', "..."),
            ('\u{a0}', " "),
        ] {
            assert_eq!(transliterate(c), Some(ascii), "{c:?}");
        }
    }

    /// Characters with no safe ASCII form have no entry, so the sanitizer
    /// drops them as before: the inverted marks would become `!` and `?`,
    /// and scripts other than Latin are not in the table.
    #[test]
    fn unmapped_characters_have_no_entry() {
        for c in ['¡', '¿', 'a', '(', '木', 'Ж', '€', '\u{2603}'] {
            assert_eq!(transliterate(c), None, "{c:?}");
        }
    }
}
