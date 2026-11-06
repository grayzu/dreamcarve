// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The theme module is the one place colours are written
//! (`docs/specs/phase-4/experience.md`, sections 5, 12 and 13, item 1), and
//! its tokens are the spec's.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use std::path::{Path, PathBuf};

use dreamcarve_app::theme;
use eframe::egui::Color32;

/// Every `.rs` file under `dir`, recursively.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("source directory") {
        let path = entry.expect("entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// `experience.md` section 12: "no widget sets a colour literal
/// (`Color32::from_rgb` outside `theme.rs` and the painter-glyph module is
/// a bug)". A source scan of the app for colour constructors and named
/// colours outside `shell/theme.rs`; the painter-glyph module arrives with
/// the operation glyphs (slice 10) and is not exempt until then.
#[test]
fn no_colour_literal_outside_theme() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(files.len() > 5, "the scan found the sources: {files:?}");
    let theme_file = src.join("shell").join("theme.rs");
    assert!(
        files.contains(&theme_file),
        "the theme module is where the spec puts it"
    );

    let mut offences = Vec::new();
    for file in files.iter().filter(|f| **f != theme_file) {
        let text = std::fs::read_to_string(file).expect("source readable");
        for (n, line) in text.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            let literal = code.match_indices("Color32::").any(|(at, _)| {
                let rest = &code[at + "Color32::".len()..];
                // A constructor (`from_rgb`, `from_gray`, `from_hex`, …) or
                // a named colour (`WHITE`, `TRANSPARENT`, …).
                rest.starts_with("from_") || rest.starts_with(|c: char| c.is_ascii_uppercase())
            }) || code.contains("Rgba::")
                || code.contains("Hsva::")
                || code.contains("ecolor::");
            if literal {
                offences.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
            }
        }
    }
    assert!(
        offences.is_empty(),
        "colour literals outside theme.rs:\n{}",
        offences.join("\n")
    );
}

/// `#RRGGBB` or `#RRGGBBAA` as egui stores it (premultiplied).
fn parse_hex(hex: &str) -> Color32 {
    let digits = hex.trim_start_matches('#');
    let byte = |i: usize| u8::from_str_radix(&digits[i..i + 2], 16).expect("hex digits");
    match digits.len() {
        6 => Color32::from_rgb(byte(0), byte(2), byte(4)),
        8 => Color32::from_rgba_unmultiplied(byte(0), byte(2), byte(4), byte(6)),
        _ => panic!("not a colour: {hex}"),
    }
}

/// The first two backticked hex values of each row of `experience.md`
/// whose first cell is a token name: `(token, light, dark)`. Rows whose
/// cells are not plain colours (the status triples, "as shape") are left
/// to the next test.
fn experience_tokens() -> Vec<(String, Color32, Color32)> {
    let text =
        std::fs::read_to_string(common::repo_root().join("docs/specs/phase-4/experience.md"))
            .expect("experience.md readable");
    let mut rows = Vec::new();
    for line in text.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() < 4 {
            continue;
        }
        // `viewport.stock (outline, dashed)` names `viewport.stock`.
        let token = cells[1].split_whitespace().next().unwrap_or_default();
        let hex = |cell: &str| -> Option<Color32> {
            let inner = cell.strip_prefix('`')?;
            let value = inner.split('`').next()?;
            (value.starts_with('#') && !cell.contains(" / ")).then(|| parse_hex(value))
        };
        if let (Some(light), Some(dark)) = (hex(cells[2]), hex(cells[3])) {
            rows.push((token.to_owned(), light, dark));
        }
    }
    rows
}

/// Section 5's tables, light and dark, are the theme's tokens, value for
/// value: surfaces, borders, text, accent and the Viewport's colours, and
/// the status colours' text, stroke and tint.
#[test]
fn tokens_match_experience_md() {
    let spec = experience_tokens();
    let light = theme::LIGHT.tokens();
    let dark = theme::DARK.tokens();
    let mut checked = 0;
    for (token, want_light, want_dark) in &spec {
        let Some((_, got_light)) = light.iter().find(|(name, _)| name == token) else {
            continue;
        };
        let (_, got_dark) = dark
            .iter()
            .find(|(name, _)| name == token)
            .expect("both palettes name the same tokens");
        assert_eq!(got_light, want_light, "light {token}");
        assert_eq!(got_dark, want_dark, "dark {token}");
        checked += 1;
    }
    for required in [
        "surface.window",
        "surface.panel",
        "surface.raised",
        "surface.sunken",
        "surface.stripe",
        "surface.hover",
        "surface.active",
        "border.default",
        "border.strong",
        "text.primary",
        "text.secondary",
        "text.disabled",
        "text.on-accent",
        "accent",
        "accent.fill",
        "accent.soft",
        "focus.ring",
        "viewport.bg",
        "viewport.axis",
        "viewport.stock",
        "viewport.stock.fill",
    ] {
        assert!(
            spec.iter().any(|(t, _, _)| t == required),
            "experience.md has no row {required}"
        );
        assert!(
            light.iter().any(|(t, _)| *t == required),
            "the theme has no token {required}"
        );
    }
    assert!(checked >= 21, "{checked} tokens checked");

    // The status rows: `text / stroke / tint` in each mode.
    let text =
        std::fs::read_to_string(common::repo_root().join("docs/specs/phase-4/experience.md"))
            .unwrap();
    let status = [
        (
            "status.error",
            theme::LIGHT.status_error,
            theme::DARK.status_error,
        ),
        (
            "status.warning",
            theme::LIGHT.status_warning,
            theme::DARK.status_warning,
        ),
        (
            "status.info",
            theme::LIGHT.status_info,
            theme::DARK.status_info,
        ),
    ];
    for (token, light, dark) in status {
        let line = text
            .lines()
            .find(|l| l.starts_with(&format!("| {token} ")))
            .unwrap_or_else(|| panic!("experience.md has no row {token}"));
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        for (cell, want) in [(cells[2], light), (cells[3], dark)] {
            let values: Vec<Color32> = cell
                .split('/')
                .map(|v| parse_hex(v.trim().trim_matches('`')))
                .collect();
            assert_eq!(values, [want.text, want.stroke, want.tint], "{token}");
        }
    }
}

/// The rows of the `experience.md` table under `heading` whose first cell
/// is a number: each row's cells.
fn numbered_rows(heading: &str) -> Vec<Vec<String>> {
    let text =
        std::fs::read_to_string(common::repo_root().join("docs/specs/phase-4/experience.md"))
            .unwrap();
    let section = text.split(heading).nth(1).expect("the section");
    let section = section.split("\n#").next().unwrap_or_default();
    section
        .lines()
        .map(|l| {
            l.split('|')
                .map(|c| c.trim().to_owned())
                .collect::<Vec<_>>()
        })
        .filter(|cells| {
            cells
                .get(1)
                .and_then(|c| c.split_whitespace().next())
                .is_some_and(|c| c.parse::<f64>().is_ok())
        })
        .collect()
}

/// The first backticked hex in `cell`.
fn hex_in(cell: &str) -> Color32 {
    parse_hex(cell.split('`').nth(1).expect("a backticked colour"))
}

/// Section 5, "Operation colours" and "Depth ramp" (design open question
/// 5, accepted; #545): the light set is the proposed column and is the
/// CLI's `dc_job::preview::PALETTE` (one table, so `--preview` and the
/// light Viewport agree), the dark set is the app's own, and the ramp's
/// five stops are the spec's in both modes.
#[test]
fn operation_colours_and_depth_ramp_match_experience_md() {
    let operations = numbered_rows("### Operation colours");
    assert_eq!(operations.len(), 8, "{operations:?}");
    for (k, row) in operations.iter().enumerate() {
        assert_eq!(theme::LIGHT.operations[k], hex_in(&row[4]), "light {k}");
        assert_eq!(theme::DARK.operations[k], hex_in(&row[6]), "dark {k}");
        assert_eq!(
            theme::LIGHT.operations[k],
            parse_hex(dc_job::preview::PALETTE[k]),
            "light {k} is the preview's"
        );
        assert_eq!(theme::LIGHT.operation(k + 8), theme::LIGHT.operations[k]);
    }
    let ramp = numbered_rows("### Depth ramp");
    assert_eq!(ramp.len(), 5, "{ramp:?}");
    for (k, row) in ramp.iter().enumerate() {
        assert_eq!(
            theme::LIGHT.depth_ramp[k],
            hex_in(&row[2]),
            "light stop {k}"
        );
        assert_eq!(theme::DARK.depth_ramp[k], hex_in(&row[3]), "dark stop {k}");
    }
}
