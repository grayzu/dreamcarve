// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Acceptance 91's first half (functional spec, capability 80): "a source
//! scan finds no user-visible string literal outside the string table".
//! `dc-app`'s `strings::every_key_is_used` is the second half, that every
//! key of the table is used. This is the scan, over the window's crate,
//! in the manner of `theme::no_colour_literal_outside_theme`.
//!
//! The scan reads every string literal of `src/` outside comments and
//! test modules, as Rust lexes them, and asks of each whether a person at
//! the window could read it as words. A literal is **wording** when, with
//! its escapes and its `{placeholders}` taken out, it has a capital
//! letter or two words. What is left unflagged by that rule is what the
//! window never shows as prose: ids and key paths (`export.details`,
//! `cut_out.tool`), placeholder names (`count`), file extensions and
//! thread names, and patterns that only arrange values (`{label}
//! ({keys})`, ` · {count}`). The rule's one blind spot is a single
//! lower-case word, which reads the same as a key; the app writes no
//! label that way.
//!
//! A wording literal must be accounted for, or the test fails naming it:
//!
//! - by what it is passed to ([`DEVELOPER_CONTEXTS`]): a lint's reason, a
//!   panic's message, `Debug` output, a compile-time path or name, the
//!   binary's command-line attributes;
//! - by name ([`NAMES`]): the fonts and text styles the theme registers
//!   with egui, which are identifiers that happen to be capitalised;
//! - as the book's text ([`BOOK_ITEMS`]): `keyboard.rs` carries the
//!   Keyboard chapter's table, which a test holds to the chapter. The
//!   book is English only, and the window shows none of it, which
//!   [`the_keyboard_chapters_text_is_not_shown_in_the_window`] checks.
//!
//! Every entry of the three lists must match a literal, so a list cannot
//! outlive what it excuses.
//!
//! What this does not cover: `dc-app`'s own refusals (`CommandError`'s
//! `Display` and its reasons), which the window shows as it shows the
//! engine's warnings and errors, in the CLI's words. The design spec's
//! "Strings" leaves error text to phase 8.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Every `.rs` file under `dir`, recursively, sorted.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("source directory")
        .map(|entry| entry.expect("entry").path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// One string literal of a source file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Literal {
    /// The line it starts on, from 1.
    line: usize,
    /// What is between its quotes, as written.
    text: String,
    /// The code before it, comments left out and each run of white space
    /// one space, cut to its last 48 characters: what it is passed to.
    before: String,
    /// Whether it is a raw string, whose backslashes are themselves.
    raw: bool,
}

/// How many characters of the code before a literal are kept.
const BEFORE_CHARS: usize = 48;

/// The string literals of `source`, in order, as Rust lexes them: line
/// and nested block comments skipped, a character literal (`'"'`) and a
/// lifetime (`'a`) not mistaken for a string's start, escapes kept as
/// written, raw strings (`r"…"`, `r#"…"#`) read to their own closing
/// quote.
fn literals(source: &str) -> Vec<Literal> {
    let chars: Vec<char> = source.chars().collect();
    let at = |i: usize| chars.get(i).copied();
    let mut out = Vec::new();
    let mut code = String::new();
    let (mut i, mut line) = (0, 1);
    let push = |code: &mut String, c: char| {
        if c.is_whitespace() {
            if !code.ends_with(' ') {
                code.push(' ');
            }
        } else {
            code.push(c);
        }
    };
    let before = |code: &str| -> String {
        let trimmed = code.trim_end();
        let count = trimmed.chars().count();
        trimmed
            .chars()
            .skip(count.saturating_sub(BEFORE_CHARS))
            .collect()
    };
    while let Some(c) = at(i) {
        match c {
            '\n' => {
                line += 1;
                push(&mut code, c);
                i += 1;
            }
            '/' if at(i + 1) == Some('/') => {
                while at(i).is_some_and(|c| c != '\n') {
                    i += 1;
                }
            }
            '/' if at(i + 1) == Some('*') => {
                let mut depth = 0;
                loop {
                    match (at(i), at(i + 1)) {
                        (Some('/'), Some('*')) => {
                            depth += 1;
                            i += 2;
                        }
                        (Some('*'), Some('/')) => {
                            depth -= 1;
                            i += 2;
                            if depth == 0 {
                                break;
                            }
                        }
                        (Some(c), _) => {
                            line += usize::from(c == '\n');
                            i += 1;
                        }
                        (None, _) => break,
                    }
                }
            }
            '\'' => {
                // A character literal is closed two or more characters
                // on; a lifetime is not closed at all.
                let end = match (at(i + 1), at(i + 2)) {
                    (Some('\\'), _) => (i + 2..chars.len()).find(|k| at(*k) == Some('\'')),
                    (Some(_), Some('\'')) => Some(i + 2),
                    _ => None,
                };
                i = end.map_or(i + 1, |end| end + 1);
                push(&mut code, '\'');
            }
            'r' | 'b' | '"' => {
                // An identifier that ends in `r` or `b` is not a prefix.
                let word_before =
                    i > 0 && at(i - 1).is_some_and(|p| p.is_alphanumeric() || p == '_') && c != '"';
                let mut k = i;
                if !word_before {
                    if at(k) == Some('b') {
                        k += 1;
                    }
                    if at(k) == Some('r') {
                        k += 1;
                    }
                }
                let raw = k > i && at(k - 1) == Some('r');
                let mut hashes = 0;
                if raw {
                    while at(k + hashes) == Some('#') {
                        hashes += 1;
                    }
                }
                if at(k + hashes) != Some('"') || (!raw && hashes > 0) {
                    push(&mut code, c);
                    i += 1;
                    continue;
                }
                let start = k + hashes + 1;
                let mut end = start;
                loop {
                    match at(end) {
                        None => break,
                        Some('\\') if !raw => end += 2,
                        Some('"') if (1..=hashes).all(|h| at(end + h) == Some('#')) => {
                            break;
                        }
                        Some(_) => end += 1,
                    }
                }
                let end = end.min(chars.len());
                let text: String = chars[start..end].iter().collect();
                out.push(Literal {
                    line,
                    before: before(&code),
                    raw,
                    text: text.clone(),
                });
                line += text.matches('\n').count();
                code.push('"');
                i = end + 1 + hashes;
            }
            _ => {
                push(&mut code, c);
                i += 1;
            }
        }
    }
    out
}

/// `literal` as a person would read it on the screen: its escapes and its
/// `{placeholders}` each a space (`{{` and `}}` are braces, and words
/// too little to matter).
fn shown(literal: &Literal) -> String {
    let mut out = String::new();
    let mut chars = literal.text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if !literal.raw => {
                // `\n`, `\"`, `\u{e084}`, and a line continuation.
                if chars.next() == Some('u') {
                    for c in chars.by_ref() {
                        if c == '}' {
                            break;
                        }
                    }
                }
                out.push(' ');
            }
            '{' => {
                for c in chars.by_ref() {
                    if c == '}' {
                        break;
                    }
                }
                out.push(' ');
            }
            c => out.push(c),
        }
    }
    out
}

/// Whether `literal` reads as words: it has a capital letter, or two
/// words with white space between them.
fn is_wording(literal: &Literal) -> bool {
    let shown = shown(literal);
    let words = shown
        .split_whitespace()
        .filter(|word| word.chars().any(char::is_alphabetic))
        .count();
    shown.chars().any(char::is_uppercase) || words > 1
}

/// What a literal is passed to when it is the developer's, never the
/// window's: the end of the code before it, and why it is not shown.
const DEVELOPER_CONTEXTS: &[(&str, &str)] = &[
    ("reason =", "a lint attribute's reason, read in the source"),
    ("debug_struct(", "`Debug` output, for a developer"),
    ("include_bytes!(", "a path read when the app is compiled"),
    ("env!(", "a variable read when the app is compiled"),
    (
        "value_name =",
        "the binary's command line, whose help is clap's English",
    ),
];

/// Wording literals that are names, by file (from `src/`) and text, and
/// why each is not a string of the window.
const NAMES: &[(&str, &str, &str)] = &[
    (
        "shell/theme.rs",
        "Inter-Regular",
        "a font, by the name egui registers it under",
    ),
    ("shell/theme.rs", "Inter-Medium", "a font's name"),
    ("shell/theme.rs", "Inter-SemiBold", "a font's name"),
    ("shell/theme.rs", "JetBrainsMono-Regular", "a font's name"),
    (
        "shell/theme.rs",
        "Title",
        "a text style, by the name egui keeps it under",
    ),
    ("shell/theme.rs", "Medium", "a text style's name"),
    ("shell/theme.rs", "Display", "a text style's name"),
    ("shell/theme.rs", "Summary", "a text style's name"),
    ("shell/theme.rs", "RailLabel", "a text style's name"),
];

/// The file that carries the book's Keyboard chapter.
const BOOK_FILE: &str = "keyboard.rs";

/// The items of [`BOOK_FILE`] whose literals are the Keyboard chapter's
/// text (what each shortcut does, the keys' names, the table), each by
/// the start of its first line. An item runs to the first later line that
/// is `}` or `];` alone.
const BOOK_ITEMS: &[&str] = &[
    "pub const SHORTCUTS: &[Shortcut] = &[",
    "fn key_names(",
    "pub fn keys_text(",
    "pub fn chapter_table(",
];

/// What reads the book's text, which nothing outside [`BOOK_FILE`] may
/// use: a shortcut's `does`, and the two functions that word the table.
const BOOK_READERS: &[&str] = &[".does", "keys_text", "chapter_table"];

/// The line ranges of [`BOOK_ITEMS`] in `source`, from 1, inclusive.
fn book_lines(source: &str) -> Vec<(usize, usize)> {
    let lines: Vec<&str> = source.lines().collect();
    BOOK_ITEMS
        .iter()
        .map(|start| {
            let first = lines
                .iter()
                .position(|line| line.starts_with(start))
                .unwrap_or_else(|| panic!("{BOOK_FILE} has no item `{start}`"));
            let last = (first..lines.len())
                .find(|k| matches!(lines[*k], "}" | "];"))
                .unwrap_or_else(|| panic!("`{start}` never ends"));
            (first + 1, last + 1)
        })
        .collect()
}

/// `source` without its test module: everything before `#[cfg(test)]`.
fn shipped(source: &str) -> &str {
    source
        .find("#[cfg(test)]")
        .map_or(source, |cut| &source[..cut])
}

/// The sources of the window's crate: each file's path from `src/`, with
/// forward slashes, and its text.
fn sources() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 20, "the scan found the sources: {files:?}");
    files
        .into_iter()
        .map(|file| {
            let name = file
                .strip_prefix(&src)
                .expect("under src")
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(&file).expect("source readable");
            (name, text)
        })
        .collect()
}

/// Acceptance 91, first half: no literal of the window's sources reads as
/// words unless it is accounted for (the module's text says how), and
/// each line of the three lists accounts for one that is there.
#[test]
fn no_user_visible_string_literal_outside_the_table() {
    let mut offences = Vec::new();
    let mut contexts_met: BTreeSet<&str> = BTreeSet::new();
    let mut names_met: BTreeSet<(&str, &str)> = BTreeSet::new();
    let mut book_literals = 0;
    let mut scanned = 0;
    for (name, text) in sources() {
        let code = shipped(&text);
        let book = if name == BOOK_FILE {
            book_lines(code)
        } else {
            Vec::new()
        };
        for literal in literals(code) {
            scanned += 1;
            if !is_wording(&literal) {
                continue;
            }
            if let Some((context, _)) = DEVELOPER_CONTEXTS
                .iter()
                .find(|(context, _)| literal.before.ends_with(context))
            {
                contexts_met.insert(context);
                continue;
            }
            if let Some((file, known, _)) = NAMES
                .iter()
                .find(|(file, known, _)| *file == name && *known == literal.text)
            {
                names_met.insert((file, known));
                continue;
            }
            if book
                .iter()
                .any(|(first, last)| (*first..=*last).contains(&literal.line))
            {
                book_literals += 1;
                continue;
            }
            offences.push(format!("src/{name}:{}: {:?}", literal.line, literal.text));
        }
    }
    assert!(scanned > 300, "the scan read the literals: {scanned}");
    assert!(
        offences.is_empty(),
        "string literals a user could read, outside crates/dc-app/strings/en.toml (key each, or \
         account for it in this test's lists):\n{}",
        offences.join("\n")
    );
    for (context, why) in DEVELOPER_CONTEXTS {
        assert!(
            contexts_met.contains(context),
            "no wording literal follows `{context}` ({why}): delete the entry"
        );
    }
    for (file, known, why) in NAMES {
        assert!(
            names_met.contains(&(*file, *known)),
            "src/{file} has no literal {known:?} ({why}): delete the entry"
        );
    }
    assert!(
        book_literals > 30,
        "the Keyboard chapter's text is in {BOOK_FILE}: {book_literals} literals"
    );
}

/// The exemption of the book's text holds only while the window shows
/// none of it: nothing outside `keyboard.rs` reads a shortcut's `does` or
/// calls the functions that word the chapter's table. (The menus take a
/// shortcut's keys from egui's own formatting of them.)
#[test]
fn the_keyboard_chapters_text_is_not_shown_in_the_window() {
    let mut readers = Vec::new();
    for (name, text) in sources() {
        if name == BOOK_FILE {
            for reader in BOOK_READERS {
                assert!(
                    text.contains(reader.trim_start_matches('.')),
                    "{BOOK_FILE} no longer has `{reader}`: bring BOOK_READERS up to date"
                );
            }
            continue;
        }
        for (n, line) in shipped(&text).lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            if BOOK_READERS.iter().any(|reader| code.contains(reader)) {
                readers.push(format!("src/{name}:{}: {}", n + 1, line.trim()));
            }
        }
    }
    assert!(
        readers.is_empty(),
        "the Keyboard chapter's English is read outside {BOOK_FILE}; if the window shows it, \
         key it:\n{}",
        readers.join("\n")
    );
}

/// The scan's own reading, on what it must flag and what it must let
/// through, so a rule loosened by mistake fails here and not silently.
#[test]
fn the_scan_reads_literals_as_rust_does() {
    let source = r##"
        // A comment's "Quoted Words" are not a literal.
        /* Nor a block's "Other Words", /* nested "Too" */ still out. */
        fn draw<'a>(ui: &'a mut Ui) {
            let quote = '"';
            ui.button("Cancel");
            ui.label("save the project");
            ui.label(format!("{count} shapes selected"));
            ui.label(r#"Raw "Text""#);
            ui.label("tab\tstop");
            let id = Id::new("status_bar");
            let key = "cut_out.tool";
            let pattern = format!("{label} ({keys})");
            let dot = format!(" · {count}");
            let unit = "mm";
            let glyph = "\u{e084}";
            thing.expect(
                "The Thing Is There",
            );
        }
    "##;
    let found = literals(source);
    let texts: Vec<&str> = found.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "Cancel",
            "save the project",
            "{count} shapes selected",
            "Raw \"Text\"",
            "tab\\tstop",
            "status_bar",
            "cut_out.tool",
            "{label} ({keys})",
            " · {count}",
            "mm",
            "\\u{e084}",
            "The Thing Is There",
        ]
    );
    let wording: Vec<&str> = found
        .iter()
        .filter(|l| is_wording(l))
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(
        wording,
        [
            "Cancel",
            "save the project",
            "{count} shapes selected",
            "Raw \"Text\"",
            "tab\\tstop",
            "The Thing Is There",
        ],
        "a capital or two words is wording; an id, a key, a pattern, a unit and a glyph are not"
    );
    let expect = found.last().expect("the last literal");
    assert!(
        expect.before.ends_with("expect("),
        "a literal on its own line follows what it is passed to: {:?}",
        expect.before
    );
    assert_eq!(found[0].line, 6, "lines count from 1");
    assert_eq!(expect.line, 18);
    // A test module is not shipped.
    assert_eq!(
        shipped("fn a() {}\n#[cfg(test)]\nmod tests { const T: &str = \"Words Here\"; }"),
        "fn a() {}\n"
    );
}

/// Every string of the table can be drawn: each of its characters has a
/// glyph in the fonts the app bundles. A character with none stops the
/// frame that lays it out (egui's missing-glyph policy), which a table
/// entry no test happens to draw would do in a user's hands; the
/// diameter sign `⌀` in a size label did (phase 5, slice 9). A
/// placeholder's braces are not drawn, and what fills it is a number, a
/// name the user typed, or a file's text.
#[test]
fn every_string_has_its_glyphs() {
    let ctx = eframe::egui::Context::default();
    dreamcarve_app::theme::apply(&ctx, dc_app::Theme::Dark);
    // Fonts are loaded at the first frame.
    let mut frame = ctx.run_ui(eframe::egui::RawInput::default(), |_| {});
    frame.textures_delta.clear();
    let font = eframe::egui::TextStyle::Body.resolve(&ctx.global_style());
    // The scan can tell: the sign that was the defect has no glyph.
    assert!(!ctx.fonts_mut(|fonts| fonts.has_glyph(&font, '\u{2300}')));
    let mut missing = Vec::new();
    for &key in dc_app::StrKey::ALL {
        let text = key.english();
        let lacking: Vec<char> = text
            .chars()
            .filter(|c| !c.is_control())
            .filter(|c| !ctx.fonts_mut(|fonts| fonts.has_glyph(&font, *c)))
            .collect();
        if !lacking.is_empty() {
            missing.push(format!("{}: {lacking:?} in `{text}`", key.key()));
        }
    }
    assert!(
        missing.is_empty(),
        "these strings have characters no bundled font draws:\n{}",
        missing.join("\n")
    );
}
