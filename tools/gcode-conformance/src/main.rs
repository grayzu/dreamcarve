// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   scripts/gcode-conformance/run.ts

//! `cargo run -p gcode-conformance -- [--rs274 BIN] [--dreamcarve BIN]
//! [--out DIR] [FILE...]`: probes LinuxCNC's `rs274`, then runs the
//! corpus, and any extra FILEs, through it (issue #173).
//!
//! `--rs274` defaults to `$RS274_BIN`, then `rs274` on `PATH`.
//! `--dreamcarve` defaults to `$DREAMCARVE_BIN`, then `debug/dreamcarve`
//! under `$CARGO_TARGET_DIR` or the workspace's `target/`; build it first
//! with `cargo build -p dreamcarve`. `--out` (default
//! `target/gcode-conformance`) is wiped and refilled on every run.
//!
//! Exits 0 when every program is accepted, 1 when `rs274` rejects one, and
//! 2 when `rs274` is missing or misconfigured or the corpus cannot be
//! built: a verdict on the setup, not on the programs.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gcode_conformance::{
    ArcTier, Case, Misconfigured, ProbeVerdicts, Verdict, build_corpus, classify, judge, probe,
    run_rs274,
};

const USAGE: &str =
    "usage: gcode-conformance [--rs274 BIN] [--dreamcarve BIN] [--out DIR] [FILE...]";

/// The command line, defaults filled in.
struct Options {
    rs274: PathBuf,
    dreamcarve: PathBuf,
    out: PathBuf,
    extra: Vec<PathBuf>,
}

fn options(root: &Path) -> Result<Options, String> {
    let env = |name: &str| std::env::var_os(name).map(PathBuf::from);
    let target = env("CARGO_TARGET_DIR").unwrap_or_else(|| root.join("target"));
    let mut options = Options {
        rs274: env("RS274_BIN").unwrap_or_else(|| PathBuf::from("rs274")),
        dreamcarve: env("DREAMCARVE_BIN").unwrap_or_else(|| {
            target
                .join("debug")
                .join(format!("dreamcarve{}", std::env::consts::EXE_SUFFIX))
        }),
        out: target.join("gcode-conformance"),
        extra: Vec::new(),
    };
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |flag: &str| -> Result<PathBuf, String> {
            args.next()
                .map(PathBuf::from)
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.to_str() {
            Some("--rs274") => options.rs274 = value("--rs274")?,
            Some("--dreamcarve") => options.dreamcarve = value("--dreamcarve")?,
            Some("--out") => options.out = value("--out")?,
            Some("-h" | "--help") => return Err(String::new()),
            Some(flag) if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            _ => options.extra.push(PathBuf::from(arg)),
        }
    }
    Ok(options)
}

fn main() -> ExitCode {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let options = match options(&root) {
        Ok(options) => options,
        Err(why) => {
            if !why.is_empty() {
                eprintln!("gcode-conformance: {why}");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if options.out.exists()
        && let Err(e) = fs::remove_dir_all(&options.out)
    {
        eprintln!("gcode-conformance: {}: {e}", options.out.display());
        return ExitCode::from(2);
    }
    if let Err(e) = fs::create_dir_all(&options.out) {
        eprintln!("gcode-conformance: {}: {e}", options.out.display());
        return ExitCode::from(2);
    }

    println!("rs274: {}", options.rs274.display());
    let tier = match probe(&options.rs274, &options.out) {
        Ok(verdicts) => {
            report_probes(&verdicts);
            classify(&verdicts)
        }
        Err(e) => Err(e),
    };
    let tier = match tier {
        Ok(tier) => tier,
        Err(e) => return misconfigured(&e),
    };
    match tier {
        ArcTier::Strict => println!("rs274 checks arcs at least as strictly as GRBL."),
        ArcTier::Tolerant => println!(
            "rs274 checks arcs, more loosely than GRBL (PureCutCNC measured 0.028 mm);\n\
             DreamCarve's own arc check (dc-gcode arcfit) is the stricter one."
        ),
    }

    let mut cases = match build_corpus(&root, &options.dreamcarve, &options.out) {
        Ok(cases) => cases,
        Err(e) => {
            eprintln!("gcode-conformance: cannot build the corpus: {e}");
            return ExitCode::from(2);
        }
    };
    cases.extend(options.extra.iter().map(|file| Case {
        name: file.display().to_string(),
        file: file.clone(),
        covers: "given on the command line".to_owned(),
    }));

    println!("\nPrograms ({}):", cases.len());
    let mut rejected = 0_usize;
    for case in &cases {
        let verdict = match run_rs274(&options.rs274, &case.file, &options.out) {
            Ok(run) => judge(&run),
            Err(e) => Verdict::Rejected(format!("cannot run rs274: {e}")),
        };
        match verdict {
            Verdict::Accepted => println!("  ok   {}", case.name),
            Verdict::Rejected(why) => {
                rejected += 1;
                println!("  FAIL {}: {why}", case.name);
                println!("       covers: {}", case.covers);
                println!("       file:   {}", case.file.display());
            }
        }
    }
    if rejected > 0 {
        eprintln!(
            "\n{rejected} of {} program(s) rejected by LinuxCNC's rs274.",
            cases.len()
        );
        return ExitCode::FAILURE;
    }
    println!(
        "\nAll {} programs accepted by LinuxCNC's rs274.",
        cases.len()
    );
    ExitCode::SUCCESS
}

fn report_probes(verdicts: &ProbeVerdicts) {
    println!("Probes:");
    let line = |name: &str, verdict: &Verdict, want_accept: bool| {
        let (seen, why) = match verdict {
            Verdict::Accepted => ("accepted", String::new()),
            Verdict::Rejected(why) => ("rejected", format!(" ({why})")),
        };
        let mark = match (want_accept, verdict) {
            (true, Verdict::Accepted) | (false, Verdict::Rejected(_)) => "ok  ",
            _ => "BAD ",
        };
        println!("  {mark} {name}: {seen}{why}");
    };
    line("smoke program", &verdicts.smoke, true);
    line("valid arc", &verdicts.arc_valid, true);
    line("arc 1 mm off its circle", &verdicts.arc_gross, false);
    line(
        "G90.1, then an arc with incremental centres",
        &verdicts.absolute_centre_arc,
        false,
    );
    let (seen, why) = match &verdicts.arc_tight {
        Verdict::Accepted => ("accepted", String::new()),
        Verdict::Rejected(why) => ("rejected", format!(" ({why})")),
    };
    println!("  note arc 0.0106 mm off (PureCutCNC #447): {seen}{why}");
}

fn misconfigured(e: &Misconfigured) -> ExitCode {
    eprintln!("\ngcode-conformance: rs274 is not usable: {e}");
    eprintln!("This is a verdict on the interpreter setup, not on DreamCarve's programs.");
    ExitCode::from(2)
}
