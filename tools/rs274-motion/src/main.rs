// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `cargo run -p rs274-motion -- [--rs274 SPEC] [--post ID[@VERSION]]
//! [--units mm|inch] [FILE...]`: runs DreamCarve programs through
//! LinuxCNC's `rs274` and compares its motion with DreamCarve's reading
//! (issue #299).
//!
//! With no FILE it runs the corpus ([`rs274_motion::corpus`]). With FILEs,
//! `--post` names the definition they were written with (default
//! `linuxcnc`, latest version) and `--units` what a controller with no
//! units word is set up in (default `mm`).
//!
//! `--rs274` is a path or `podman:<image>` / `docker:<image>`; it defaults
//! to `$DREAMCARVE_RS274`, then `rs274` on `PATH`.
//!
//! Exits 0 when every program agrees, 1 when one differs or `rs274`
//! rejects one, and 2 when nothing could be compared (no `rs274`, a bad
//! option, a corpus that does not build).

use std::path::PathBuf;
use std::process::ExitCode;

use dc_project::{Units, bundled_definition, latest_definition};
use rs274_motion::corpus::{Case, build_corpus};
use rs274_motion::runner::{ENV, Rs274};
use rs274_motion::{Outcome, Verdict, check};

const USAGE: &str =
    "usage: rs274-motion [--rs274 SPEC] [--post ID[@VERSION]] [--units mm|inch] [FILE...]";

struct Options {
    rs274: Option<Rs274>,
    post: String,
    units: Units,
    files: Vec<PathBuf>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        rs274: None,
        post: "linuxcnc".to_owned(),
        units: Units::Mm,
        files: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "--rs274" => options.rs274 = Some(Rs274::from_spec(&value("--rs274")?)),
            "--post" => options.post = value("--post")?,
            "--units" => {
                options.units = match value("--units")?.as_str() {
                    "mm" => Units::Mm,
                    "inch" => Units::Inch,
                    other => return Err(format!("--units is mm or inch, not {other}")),
                }
            }
            "-h" | "--help" => return Err(String::new()),
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            _ => options.files.push(PathBuf::from(arg)),
        }
    }
    Ok(options)
}

fn given_files(options: &Options) -> Result<Vec<Case>, String> {
    let (id, version) = match options.post.split_once('@') {
        Some((id, v)) => (
            id,
            Some(v.parse::<u32>().map_err(|_| format!("bad version {v}"))?),
        ),
        None => (options.post.as_str(), None),
    };
    let definition = match version {
        Some(v) => bundled_definition(id, v),
        None => latest_definition(id),
    }
    .map_err(|e| e.to_string())?;
    options
        .files
        .iter()
        .map(|file| {
            Ok(Case {
                name: file.display().to_string(),
                post: definition.post_processor().clone(),
                units: options.units,
                text: std::fs::read_to_string(file)
                    .map_err(|e| format!("{}: {e}", file.display()))?,
            })
        })
        .collect()
}

fn main() -> ExitCode {
    let options = match options() {
        Ok(o) => o,
        Err(why) => {
            if !why.is_empty() {
                eprintln!("rs274-motion: {why}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let Some(rs274) = options.rs274.clone().or_else(Rs274::detect) else {
        eprintln!(
            "rs274-motion: no rs274: install linuxcnc-uspace, or set {ENV} to its path or to \
             podman:<image> (see tools/rs274-motion/README.md)"
        );
        return ExitCode::from(2);
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cases = if options.files.is_empty() {
        build_corpus(&root).map_err(|e| e.to_string())
    } else {
        given_files(&options)
    };
    let cases = match cases {
        Ok(c) => c,
        Err(why) => {
            eprintln!("rs274-motion: {why}");
            return ExitCode::from(2);
        }
    };
    let work = std::env::temp_dir().join(format!("rs274-motion-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&work) {
        eprintln!("rs274-motion: {}: {e}", work.display());
        return ExitCode::from(2);
    }
    println!("rs274: {rs274}\n");
    let mut failed = 0_usize;
    for case in &cases {
        let outcome = check(&case.text, &case.post, case.units, &rs274, &work);
        if !outcome.agrees() {
            failed += 1;
        }
        report(case, &outcome);
    }
    let _ = std::fs::remove_dir_all(&work);
    println!(
        "\n{} of {} program(s) agree with rs274.",
        cases.len() - failed,
        cases.len()
    );
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn report(case: &Case, outcome: &Outcome) {
    let label = format!("{} [{}]", case.name, case.post.id);
    match &outcome.verdict {
        Verdict::Compared(c) if c.differences.is_empty() => {
            println!("  ok   {label}: {} moves on {} lines", c.moves, c.lines);
        }
        Verdict::Compared(c) => {
            println!("  DIFF {label}: {} difference(s)", c.differences.len());
            for d in &c.differences {
                let line = d.line.map_or_else(|| "-".to_owned(), |l| l.to_string());
                println!("       line {line}: {}", d.what.replace('\n', "\n       "));
            }
        }
        Verdict::Rejected(why) => println!("  FAIL {label}: rs274 rejected it: {why}"),
        Verdict::Failed(why) => println!("  FAIL {label}: {why}"),
    }
    for r in &outcome.rewrites {
        println!(
            "       bridged line {}: {} -> {} ({})",
            r.line,
            r.from.trim(),
            r.to,
            r.why
        );
    }
}
