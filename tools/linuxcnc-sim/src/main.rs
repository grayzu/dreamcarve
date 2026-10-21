// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `cargo run -p linuxcnc-sim -- [--linuxcnc SPEC] [--post ID[@VERSION]]
//! [--units mm|inch] [FILE...]`: runs DreamCarve programs through
//! LinuxCNC's full simulation, headless, and checks them against
//! DreamCarve's reading (issue #328).
//!
//! With no FILE it runs the LinuxCNC and generic programs of
//! `rs274-motion`'s corpus. With FILEs, `--post` names the definition
//! they were written with (default `linuxcnc`, latest version) and
//! `--units` the units before the program selects any (default `mm`).
//!
//! `--linuxcnc` is a path to the `linuxcnc` script or `podman:<image>` /
//! `docker:<image>`; it defaults to `$DREAMCARVE_LINUXCNC`.
//!
//! Exits 0 when every program agrees, 1 when one does not, and 2 when
//! nothing could be checked (no LinuxCNC, a bad option, a session that
//! left no record).

use std::path::PathBuf;
use std::process::ExitCode;

use dc_project::{Units, bundled_definition, latest_definition};
use linuxcnc_sim::runner::{ENV, LinuxCnc};
use linuxcnc_sim::{check_cases, linuxcnc_cases};
use rs274_motion::corpus::Case;

const USAGE: &str =
    "usage: linuxcnc-sim [--linuxcnc SPEC] [--post ID[@VERSION]] [--units mm|inch] [FILE...]";

struct Options {
    linuxcnc: Option<LinuxCnc>,
    post: String,
    units: Units,
    files: Vec<PathBuf>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        linuxcnc: None,
        post: "linuxcnc".to_owned(),
        units: Units::Mm,
        files: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "--linuxcnc" => options.linuxcnc = Some(LinuxCnc::from_spec(&value("--linuxcnc")?)),
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
                eprintln!("linuxcnc-sim: {why}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let Some(linuxcnc) = options.linuxcnc.clone().or_else(LinuxCnc::detect) else {
        eprintln!(
            "linuxcnc-sim: no LinuxCNC: set {ENV} or --linuxcnc to podman:<image> (after \
             `podman build -t dreamcarve-rs274 tools/rs274-motion`) or to the linuxcnc script \
             (see tools/linuxcnc-sim/README.md)"
        );
        return ExitCode::from(2);
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cases = if options.files.is_empty() {
        linuxcnc_cases(&root).map_err(|e| e.to_string())
    } else {
        given_files(&options)
    };
    let cases = match cases {
        Ok(c) => c,
        Err(why) => {
            eprintln!("linuxcnc-sim: {why}");
            return ExitCode::from(2);
        }
    };
    let work = std::env::temp_dir().join(format!("linuxcnc-sim-{}", std::process::id()));
    println!("LinuxCNC: {linuxcnc}\n");
    let outcomes = check_cases(&cases, &linuxcnc, &work);
    let _ = std::fs::remove_dir_all(&work);
    let outcomes = match outcomes {
        Ok(o) => o,
        Err(why) => {
            eprintln!("linuxcnc-sim: {why}");
            return ExitCode::from(2);
        }
    };
    let mut failed = 0_usize;
    for (case, outcome) in cases.iter().zip(&outcomes) {
        let label = format!("{} [{}]", case.name, case.post.id);
        if outcome.agrees() {
            println!(
                "  ok   {label}: {} moves, {} M0 pause(s)",
                outcome.moves, outcome.pauses
            );
        } else {
            failed += 1;
            println!("  DIFF {label}: {} problem(s)", outcome.problems.len());
            for p in &outcome.problems {
                println!("       {}", p.replace('\n', "\n       "));
            }
        }
    }
    println!(
        "\n{} of {} program(s) agree with LinuxCNC's full simulation \
         ({} logged moves and {} M0 pauses checked).",
        cases.len() - failed,
        cases.len(),
        outcomes.iter().map(|o| o.moves).sum::<usize>(),
        outcomes.iter().map(|o| o.pauses).sum::<usize>()
    );
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}
