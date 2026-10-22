// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `cargo run -p grbl-sim-motion -- [--sims SPEC] [--post ID[@VERSION]]
//! [FILE...]`: runs DreamCarve's grblHAL and GRBL programs through the
//! grblHAL Simulator and grbl-sim, and compares their motion with
//! DreamCarve's reading (issue #329).
//!
//! With no FILE it runs the corpus ([`grbl_sim_motion::corpus`]). With
//! FILEs, `--post` names the definition they were written with (`grblhal`
//! or `grbl`, default `grblhal`, latest version).
//!
//! `--sims` is a directory or `podman:<image>` / `docker:<image>`; it
//! defaults to `$DREAMCARVE_GRBL_SIM`, then `grblHAL_sim` on `PATH`.
//!
//! For each program it runs the validator, `$C` check mode and the motion
//! comparison. Exits 0 when every program passes all three, 1 when one
//! does not, and 2 when nothing could be checked (no simulators, a probe
//! that fails, a bad option, a corpus that does not build).

use std::path::PathBuf;
use std::process::ExitCode;

use dc_project::{Units, bundled_definition, latest_definition};
use grbl_sim_motion::corpus::{Case, build_corpus, program_units};
use grbl_sim_motion::runner::{ENV, Firmware, KEEP, Sims};
use grbl_sim_motion::{Verdict, check_mode, check_motion, probe, validate};

const USAGE: &str = "usage: grbl-sim-motion [--sims SPEC] [--post ID[@VERSION]] [FILE...]";

struct Options {
    sims: Option<Sims>,
    post: String,
    files: Vec<PathBuf>,
}

fn options() -> Result<Options, String> {
    let mut options = Options {
        sims: None,
        post: "grblhal".to_owned(),
        files: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| args.next().ok_or_else(|| format!("{flag} needs a value"));
        match arg.as_str() {
            "--sims" => options.sims = Some(Sims::from_spec(&value("--sims")?)),
            "--post" => options.post = value("--post")?,
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
    let firmware = Firmware::of(id).ok_or_else(|| format!("--post is grblhal or grbl, not {id}"))?;
    let definition = match version {
        Some(v) => bundled_definition(id, v),
        None => latest_definition(id),
    }
    .map_err(|e| e.to_string())?;
    options
        .files
        .iter()
        .map(|file| {
            let text =
                std::fs::read_to_string(file).map_err(|e| format!("{}: {e}", file.display()))?;
            Ok(Case {
                name: file.display().to_string(),
                post: definition.post_processor().clone(),
                firmware,
                units: program_units(&text, Units::Mm),
                text,
            })
        })
        .collect()
}

fn main() -> ExitCode {
    let options = match options() {
        Ok(o) => o,
        Err(why) => {
            if !why.is_empty() {
                eprintln!("grbl-sim-motion: {why}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let Some(sims) = options.sims.clone().or_else(Sims::detect) else {
        eprintln!(
            "grbl-sim-motion: no simulators: set {ENV} to a directory with them or to \
             podman:<image> (see tools/grbl-sim-motion/README.md)"
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
            eprintln!("grbl-sim-motion: {why}");
            return ExitCode::from(2);
        }
    };
    let work = std::env::temp_dir().join(format!("grbl-sim-motion-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&work) {
        eprintln!("grbl-sim-motion: {}: {e}", work.display());
        return ExitCode::from(2);
    }
    println!("{sims}\n");
    for firmware in [Firmware::GrblHal, Firmware::Grbl] {
        if cases.iter().any(|c| c.firmware == firmware)
            && let Err(why) = probe(firmware, &sims, &work)
        {
            eprintln!("grbl-sim-motion: {why}");
            let _ = std::fs::remove_dir_all(&work);
            return ExitCode::from(2);
        }
    }
    let mut failed = 0_usize;
    for case in &cases {
        if !report(case, &sims, &work) {
            failed += 1;
        }
    }
    if std::env::var_os(KEEP).is_none_or(|v| v.is_empty()) {
        let _ = std::fs::remove_dir_all(&work);
    }
    println!(
        "\n{} of {} program(s) pass.",
        cases.len() - failed,
        cases.len()
    );
    if failed > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

/// Checks and reports one program; true if it passes.
fn report(case: &Case, sims: &Sims, work: &std::path::Path) -> bool {
    let label = format!("{} [{} on {}]", case.name, case.post.id, case.firmware);
    let mut ok = true;
    // grblHAL_validator crashes on every input at the pinned commit
    // (README, "What runs"); check mode covers what it would.
    let validated = match case.firmware {
        Firmware::Grbl => validate(&case.text, case.firmware, sims, work),
        Firmware::GrblHal => Ok(None),
    };
    match validated {
        Ok(None) => {}
        Ok(Some(why)) => {
            ok = false;
            println!("  FAIL {label}: the validator rejected it: {why}");
        }
        Err(why) => {
            ok = false;
            println!("  FAIL {label}: {why}");
        }
    }
    match check_mode(case, sims, work) {
        Ok(problems) if problems.is_empty() => {}
        Ok(problems) => {
            ok = false;
            println!("  FAIL {label}: check mode: {}", problems.join("; "));
        }
        Err(why) => {
            ok = false;
            println!("  FAIL {label}: check mode: {why}");
        }
    }
    match check_motion(case, sims, work) {
        Verdict::Compared(c) if c.differences.is_empty() => {
            if ok {
                println!(
                    "  ok   {label}: {} moves, {} checked against the simulator",
                    c.moves, c.checked
                );
            }
        }
        Verdict::Compared(c) => {
            ok = false;
            println!("  DIFF {label}: {} difference(s)", c.differences.len());
            for d in &c.differences {
                let line = d.line.map_or_else(|| "-".to_owned(), |l| l.to_string());
                println!("       line {line}: {}", d.what.replace('\n', "\n       "));
            }
        }
        Verdict::Rejected(why) => {
            ok = false;
            println!("  FAIL {label}: the firmware rejected it: {why}");
        }
        Verdict::Failed(why) => {
            ok = false;
            println!("  FAIL {label}: {why}");
        }
    }
    ok
}
