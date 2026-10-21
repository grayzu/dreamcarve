// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Transliteration of the text DreamCarve writes into comments to
//! printable ASCII (`docs/specs/phase-2/design.md`, "Phase 2 addendum
//! (#73)", A2; functional capability 28).
//!
//! The emitter's comment sanitizer (`emit::sanitize_comment`) maps each
//! non-ASCII character through [`transliterate`] before its filter, so
//! "Café" is written `Cafe`, not `Caf`. The table is DreamCarve's own,
//! written by hand and reviewed, with no dependency (`deunicode` was
//! considered: its table covers scripts whose output nobody here can
//! check). It covers Latin-1 Supplement (U+00A0–U+00FF), Latin Extended-A
//! (U+0100–U+017F), the General Punctuation dashes, quotes and dots
//! (U+2010–U+2026), and `™`. A letter loses its accent or stroke (`é` →
//! `e`, `Ø` → `O`, `Ł` → `L`), a ligature or sharp s spells out (`Æ` →
//! `AE`, `ß` → `ss`), and a symbol takes its usual ASCII spelling (`½` →
//! `1/2`, `°` → `deg`, `×` → `x`, `—` → `-`, `…` → `...`).
//!
//! No entry holds a character a controller acts on in a comment (`( ) ; !
//! ? ~ { } %`): `©` is `C`, not `(C)`, and `¡` and `¿` have no entry, so
//! they are dropped, as is every character not in the table.

/// The printable ASCII text that stands for non-ASCII character `c` in a
/// comment, or `None` when the table has no entry for it (and for ASCII
/// characters, which the sanitizer keeps or drops itself). An entry may be
/// empty: a soft hyphen is removed, not turned into a space.
pub(crate) fn transliterate(c: char) -> Option<&'static str> {
    let ascii = match c {
        // Latin-1 Supplement: spaces and symbols.
        '\u{a0}' => " ",
        '¢' => "c",
        '£' => "GBP",
        '¥' => "JPY",
        '¦' => "|",
        '§' => "S",
        '©' => "C",
        'ª' => "a",
        '«' => "<<",
        '¬' => "-",
        '\u{ad}' => "",
        '®' => "R",
        '°' => "deg",
        '±' => "+-",
        '²' => "2",
        '³' => "3",
        '´' => "'",
        'µ' => "u",
        '¶' => "P",
        '·' => ".",
        '¸' => ",",
        '¹' => "1",
        'º' => "o",
        '»' => ">>",
        '¼' => "1/4",
        '½' => "1/2",
        '¾' => "3/4",
        '×' => "x",
        '÷' => "/",
        // Latin-1 Supplement: letters.
        'À' | 'Á' | 'Â' | 'Ã' | 'Ä' | 'Å' => "A",
        'Æ' => "AE",
        'Ç' => "C",
        'È' | 'É' | 'Ê' | 'Ë' => "E",
        'Ì' | 'Í' | 'Î' | 'Ï' => "I",
        'Ð' => "D",
        'Ñ' => "N",
        'Ò' | 'Ó' | 'Ô' | 'Õ' | 'Ö' | 'Ø' => "O",
        'Ù' | 'Ú' | 'Û' | 'Ü' => "U",
        'Ý' => "Y",
        'Þ' => "Th",
        'ß' => "ss",
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => "a",
        'æ' => "ae",
        'ç' => "c",
        'è' | 'é' | 'ê' | 'ë' => "e",
        'ì' | 'í' | 'î' | 'ï' => "i",
        'ð' => "d",
        'ñ' => "n",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' | 'ø' => "o",
        'ù' | 'ú' | 'û' | 'ü' => "u",
        'ý' | 'ÿ' => "y",
        'þ' => "th",
        // Latin Extended-A, in code point order.
        'Ā' | 'Ă' | 'Ą' => "A",
        'ā' | 'ă' | 'ą' => "a",
        'Ć' | 'Ĉ' | 'Ċ' | 'Č' => "C",
        'ć' | 'ĉ' | 'ċ' | 'č' => "c",
        'Ď' | 'Đ' => "D",
        'ď' | 'đ' => "d",
        'Ē' | 'Ĕ' | 'Ė' | 'Ę' | 'Ě' => "E",
        'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => "e",
        'Ĝ' | 'Ğ' | 'Ġ' | 'Ģ' => "G",
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => "g",
        'Ĥ' | 'Ħ' => "H",
        'ĥ' | 'ħ' => "h",
        'Ĩ' | 'Ī' | 'Ĭ' | 'Į' | 'İ' => "I",
        'ĩ' | 'ī' | 'ĭ' | 'į' | 'ı' => "i",
        'Ĳ' => "IJ",
        'ĳ' => "ij",
        'Ĵ' => "J",
        'ĵ' => "j",
        'Ķ' => "K",
        'ķ' | 'ĸ' => "k",
        'Ĺ' | 'Ļ' | 'Ľ' | 'Ŀ' | 'Ł' => "L",
        'ĺ' | 'ļ' | 'ľ' | 'ŀ' | 'ł' => "l",
        'Ń' | 'Ņ' | 'Ň' => "N",
        'ń' | 'ņ' | 'ň' => "n",
        'ŉ' => "'n",
        'Ŋ' => "NG",
        'ŋ' => "ng",
        'Ō' | 'Ŏ' | 'Ő' => "O",
        'ō' | 'ŏ' | 'ő' => "o",
        'Œ' => "OE",
        'œ' => "oe",
        'Ŕ' | 'Ŗ' | 'Ř' => "R",
        'ŕ' | 'ŗ' | 'ř' => "r",
        'Ś' | 'Ŝ' | 'Ş' | 'Š' => "S",
        'ś' | 'ŝ' | 'ş' | 'š' | 'ſ' => "s",
        'Ţ' | 'Ť' | 'Ŧ' => "T",
        'ţ' | 'ť' | 'ŧ' => "t",
        'Ũ' | 'Ū' | 'Ŭ' | 'Ů' | 'Ű' | 'Ų' => "U",
        'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => "u",
        'Ŵ' => "W",
        'ŵ' => "w",
        'Ŷ' | 'Ÿ' => "Y",
        'ŷ' => "y",
        'Ź' | 'Ż' | 'Ž' => "Z",
        'ź' | 'ż' | 'ž' => "z",
        // General Punctuation: dashes, quotes, dots.
        '\u{2010}'..='\u{2015}' => "-",
        '\u{2016}' => "||",
        '\u{2017}' => "_",
        '\u{2018}' | '\u{2019}' | '\u{201b}' => "'",
        '\u{201a}' => ",",
        '\u{201c}'..='\u{201f}' => "\"",
        '\u{2020}' => "+",
        '\u{2021}' => "++",
        '\u{2022}' => "*",
        '\u{2023}' => ">",
        '\u{2024}' => ".",
        '\u{2025}' => "..",
        '\u{2026}' => "...",
        '™' => "TM",
        _ => return None,
    };
    Some(ascii)
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
