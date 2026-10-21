// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Rewrites a DreamCarve program so LinuxCNC's `rs274` reads it, changing
//! as little as it can and listing every change.
//!
//! Every rewrite comes from the definition, not from the controller's
//! name, so a new definition version gets the same treatment:
//!
//! - **Line numbers.** Every line that is not `%` or blank gets
//!   `N<line>`, its 1-based line in the original, replacing any `N` word it
//!   had (Mach3's). `rs274` prints the `N` of the block each canonical call
//!   comes from, so the comparison can line calls up with source lines. `N`
//!   is a label on every controller here; renumbering moves nothing.
//! - **`G50` and `G69`.** Mach3 and UCCNC's headers reset scaling and
//!   rotation (#312). LinuxCNC has neither mode, so the identity they
//!   restore is already in force, and `rs274` rejects both words ("Unknown
//!   g code used"). They are dropped.
//! - **Millisecond dwells.** Where the definition's dwell is in
//!   milliseconds (UCCNC), the `P` of a `G4` or `G82` is divided by 1000:
//!   LinuxCNC's `P` is seconds ([LinuxCNC G-code reference], `G4`).
//! - **No units word.** Where the definition has none (UCCNC reads lengths
//!   in the units it is set up in), `G21` or `G20`, as the caller says the
//!   controller is set up, is put on the first line with code.
//!
//! The bridge reads DreamCarve's output, which separates words with
//! spaces and puts comments after the code; it is not a general G-code
//! rewriter.
//!
//! [LinuxCNC G-code reference]: https://linuxcnc.org/docs/html/gcode/g-code.html

use dc_project::{DwellUnits, PostProcessor, Units};

/// Words LinuxCNC does not have that a DreamCarve header writes, with why
/// dropping them changes no motion under `rs274`.
const DROPPED: &[(&str, &str)] = &[
    (
        "G50",
        "scaling reset (Mach3, UCCNC): LinuxCNC has no scaling, so scale 1 is in force",
    ),
    (
        "G69",
        "rotation cancel (Mach3, UCCNC): LinuxCNC has no G68 rotation, so none is in force",
    ),
];

/// One change the bridge made, so the report can say what `rs274` did not
/// read as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rewrite {
    /// 1-based line in the original program.
    pub line: usize,
    /// The original line.
    pub from: String,
    /// The line `rs274` read.
    pub to: String,
    /// Why the change moves nothing, or what it assumes.
    pub why: String,
}

/// A program rewritten for `rs274`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bridged {
    /// The text `rs274` reads, one line per original line.
    pub text: String,
    /// Every change beyond the line numbers.
    pub rewrites: Vec<Rewrite>,
}

/// Rewrites `text`, a program written with definition `post`, for `rs274`.
/// `units` is what the controller is set up in, for a definition with no
/// units word; it is ignored otherwise.
#[must_use]
pub fn bridge(text: &str, post: &PostProcessor, units: Units) -> Bridged {
    let millis = post.feed_speed.spindle_on_dwell.units == DwellUnits::Milliseconds;
    let mut needs_units = post.units.mm_command.is_none();
    let mut out = String::with_capacity(text.len() + text.len() / 4);
    let mut rewrites = Vec::new();
    for (index, raw) in text.lines().enumerate() {
        let line = index + 1;
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed == "%" {
            out.push_str(raw);
            out.push('\n');
            continue;
        }
        let (code, comment) = split_comment(raw);
        let mut words: Vec<String> = code.split_whitespace().map(str::to_owned).collect();
        if words.first().is_some_and(|w| is_line_number(w)) {
            words.remove(0);
        }
        let mut why = Vec::new();
        for (word, reason) in DROPPED {
            let before = words.len();
            words.retain(|w| !w.eq_ignore_ascii_case(word));
            if words.len() != before {
                why.push(format!("dropped {word}: {reason}"));
            }
        }
        if millis && words.iter().any(|w| is_code(w, "G4") || is_code(w, "G82")) {
            for w in &mut words {
                if let Some(seconds) = millis_to_seconds(w) {
                    why.push(format!(
                        "{w} is milliseconds on this controller; rs274 reads seconds"
                    ));
                    *w = seconds;
                }
            }
        }
        if needs_units && !words.is_empty() {
            needs_units = false;
            let word = match units {
                Units::Mm => "G21",
                Units::Inch => "G20",
            };
            words.insert(0, word.to_owned());
            why.push(format!(
                "added {word}: the definition writes no units word, and the controller is \
                 taken to be set up in {}",
                match units {
                    Units::Mm => "millimetres",
                    Units::Inch => "inches",
                }
            ));
        }
        let mut rewritten = format!("N{line}");
        for w in &words {
            rewritten.push(' ');
            rewritten.push_str(w);
        }
        if !comment.is_empty() {
            rewritten.push(' ');
            rewritten.push_str(comment);
        }
        if !why.is_empty() {
            rewrites.push(Rewrite {
                line,
                from: raw.to_owned(),
                to: rewritten.clone(),
                why: why.join("; "),
            });
        }
        out.push_str(&rewritten);
        out.push('\n');
    }
    Bridged {
        text: out,
        rewrites,
    }
}

/// The code before the first comment, and the comment to the end of the
/// line.
fn split_comment(line: &str) -> (&str, &str) {
    match line.find(['(', ';']) {
        Some(at) => (line.get(..at).unwrap_or(line), line.get(at..).unwrap_or("")),
        None => (line, ""),
    }
}

/// True for an `N` word: `N` and digits.
fn is_line_number(word: &str) -> bool {
    let mut chars = word.chars();
    matches!(chars.next(), Some('N' | 'n'))
        && !word.get(1..).unwrap_or("").is_empty()
        && chars.all(|c| c.is_ascii_digit())
}

/// True if `word` is the code `code` (`G4` matches `G4`, `G04`, `g4.0`).
fn is_code(word: &str, code: &str) -> bool {
    let (Some(wl), Some(cl)) = (word.get(..1), code.get(..1)) else {
        return false;
    };
    if !wl.eq_ignore_ascii_case(cl) {
        return false;
    }
    match (
        word.get(1..).and_then(|n| n.parse::<f64>().ok()),
        code.get(1..).and_then(|n| n.parse::<f64>().ok()),
    ) {
        (Some(a), Some(b)) => (a - b).abs() < 1e-9,
        _ => false,
    }
}

/// `P<ms>` as `P<s>`, or `None` if `word` is not a `P` word.
fn millis_to_seconds(word: &str) -> Option<String> {
    let rest = word.strip_prefix(['P', 'p'])?;
    let ms: f64 = rest.parse().ok()?;
    let mut s = format!("{:.6}", ms / 1000.0);
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    Some(format!("P{s}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::latest_definition;

    fn post(id: &str) -> PostProcessor {
        latest_definition(id)
            .expect("bundled")
            .post_processor()
            .clone()
    }

    /// Every line keeps its place and gets its own number, so a canonical
    /// call's `N` is its source line; Mach3's own numbers are replaced.
    #[test]
    fn numbers_every_line_with_its_source_line() {
        let text = "%\nN10 ( Project: p )\nN20 G90 G94 G17 G40 G80\n\nN30 G0 Z5.000\n";
        let b = bridge(text, &post("mach3"), Units::Mm);
        assert_eq!(
            b.text,
            "%\nN2 ( Project: p )\nN3 G90 G94 G17 G40 G80\n\nN5 G0 Z5.000\n"
        );
        assert!(b.rewrites.is_empty(), "{:?}", b.rewrites);
    }

    /// #312's header words: `rs274` has neither, and each drop is listed.
    #[test]
    fn drops_g50_and_g69_and_says_so() {
        let text = "N40 G50\nN50 G69\nG0 X1\n";
        let b = bridge(text, &post("mach3"), Units::Mm);
        assert_eq!(b.text, "N1\nN2\nN3 G0 X1\n");
        assert_eq!(b.rewrites.len(), 2);
        assert!(
            b.rewrites[0].why.contains("dropped G50"),
            "{:?}",
            b.rewrites
        );
        assert!(
            b.rewrites[1].why.contains("dropped G69"),
            "{:?}",
            b.rewrites
        );
    }

    /// UCCNC: `G4 P3000` is 3 s (definitions README, "UCCNC"); LinuxCNC's
    /// `P` is seconds. The missing units word is supplied as set up.
    #[test]
    fn uccnc_dwell_becomes_seconds_and_units_are_stated() {
        let text = "( Project: p )\nG90 G17\nG4 P3000\nG4 P250\n";
        let b = bridge(text, &post("uccnc"), Units::Inch);
        assert_eq!(
            b.text,
            "N1 ( Project: p )\nN2 G20 G90 G17\nN3 G4 P3\nN4 G4 P0.25\n"
        );
        assert_eq!(b.rewrites.len(), 3, "{:?}", b.rewrites);
        assert!(b.rewrites[0].why.contains("added G20"), "{:?}", b.rewrites);
        assert!(
            b.rewrites[1].why.contains("milliseconds"),
            "{:?}",
            b.rewrites
        );
    }

    /// LinuxCNC and generic programs are read as written: only numbered.
    #[test]
    fn linuxcnc_and_generic_are_only_numbered() {
        let text = "G90 G17 G40 G80 G94\nG91.1\nG21\nG4 P3.000\nG53 G0 Z-5.000\nM2\n";
        for id in ["linuxcnc", "generic"] {
            let b = bridge(text, &post(id), Units::Mm);
            assert!(b.rewrites.is_empty(), "{id}: {:?}", b.rewrites);
            assert_eq!(
                b.text,
                "N1 G90 G17 G40 G80 G94\nN2 G91.1\nN3 G21\nN4 G4 P3.000\nN5 G53 G0 Z-5.000\nN6 M2\n",
                "{id}"
            );
        }
    }
}
