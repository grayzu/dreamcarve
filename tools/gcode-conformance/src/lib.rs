// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   scripts/gcode-conformance/run.ts

//! Runs DreamCarve's LinuxCNC-dialect programs through LinuxCNC's own
//! standalone RS-274/NGC interpreter, `rs274` (issue #173).
//!
//! DreamCarve's `post` tests re-derive LinuxCNC's rules from its
//! documentation; `rs274` is the rules. A rejection here is LinuxCNC's
//! verdict, not a second opinion.
//!
//! Before any verdict is trusted, `rs274` faces the probes of PureCutCNC's
//! harness ([`probe`]): a known-good program it must accept (one that
//! rejects it is misconfigured, not strict), an arc it must accept, an arc
//! 1 mm off that it must reject, and DreamCarve's own bad program, `G90.1`
//! followed by an arc with incremental centres, that it must reject. The
//! tight arc probe only says how strict its arc check is.
//!
//! The corpus ([`build_corpus`]) is the committed LinuxCNC and generic
//! golden programs, the canned-cycle program `cli::canned_cycles_linuxcnc_opt_in`
//! pins, and the quick-start plaque exported for both posts by the
//! `dreamcarve` binary.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The flags `rs274` runs with: `-g`, batch mode. Without it `rs274` asks
/// on standard input what to do before it starts.
pub const RS274_FLAGS: &[&str] = &["-g"];

/// The line `rs274` writes to standard error before every run.
const BANNER: &str = "executing";

/// The canonical call `rs274` prints when a program reaches `M2` or `M30`.
const PROGRAM_END: &str = "PROGRAM_END()";

/// The date written into exported programs, so a corpus is the same bytes
/// on every run (the CLI tests' `FIXED_DATE`).
pub const EXPORT_DATE: &str = "2026-10-15";

/// One run of `rs274` on one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The exit code; `None` when a signal ended it.
    pub status: Option<i32>,
    /// The canonical machining calls, one per line.
    pub stdout: String,
    /// The banner, then any error or warning.
    pub stderr: String,
}

/// What `rs274` made of a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It ran to its end with no error or warning.
    Accepted,
    /// It stopped with an error, warned, or never reached its end. The
    /// text is what `rs274` said, or why the run does not count.
    Rejected(String),
}

/// Judges one run. Any line on standard error other than the banner is an
/// error or a warning, and fails the program even when `rs274` exits 0. A
/// run that exits 0 without reaching `M2`/`M30` fails too: the program was
/// not read to its end.
#[must_use]
pub fn judge(run: &Run) -> Verdict {
    let messages: Vec<&str> = run
        .stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && *line != BANNER)
        .collect();
    match run.status {
        Some(0) => {}
        Some(code) if messages.is_empty() => {
            return Verdict::Rejected(format!("exited {code}"));
        }
        Some(_) => return Verdict::Rejected(messages.join(" | ")),
        None => return Verdict::Rejected("ended by a signal".to_owned()),
    }
    if !messages.is_empty() {
        return Verdict::Rejected(format!("warning: {}", messages.join(" | ")));
    }
    if !run
        .stdout
        .lines()
        .any(|line| line.trim_end().ends_with(PROGRAM_END))
    {
        return Verdict::Rejected("never reached the program end (M2 or M30)".to_owned());
    }
    Verdict::Accepted
}

/// Runs `rs274` on `file`, from `dir` (anything it writes lands there).
///
/// # Errors
///
/// The OS error when `rs274` cannot be started.
pub fn run_rs274(rs274: &Path, file: &Path, dir: &Path) -> io::Result<Run> {
    let out = Command::new(rs274)
        .args(RS274_FLAGS)
        .arg(file)
        .current_dir(dir)
        .stdin(Stdio::null())
        .output()?;
    Ok(Run {
        status: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}

/// A minimal, unambiguously valid program every interpreter must accept.
pub const SMOKE: &str = "G21\nG90\nG0 X0 Y0\nG1 X1 Y0 F100\nM2\n";

/// A quarter circle of radius 10 with an incremental centre: valid.
pub const ARC_VALID: &str = "G21\nG90\nG0 X10 Y0\nG1 F600\nG3 X0 Y10 I-10 J0\nM2\n";

/// [`ARC_VALID`] with its end 1 mm off the circle. An interpreter that
/// accepts it is not checking arcs at all.
pub const ARC_GROSS: &str = "G21\nG90\nG0 X10 Y0\nG1 F600\nG3 X0 Y11 I-10 J0\nM2\n";

/// PureCutCNC's issue #447 block, whose radii disagree by 0.0106 mm. GRBL
/// (0.005 mm) rejects it; LinuxCNC's `rs274` accepts radius errors up to
/// about 0.028 mm (PureCutCNC's measurement).
pub const ARC_TIGHT: &str =
    "G21\nG90\nG0 X11.109 Y16.962\nG1 F600\nG3 X11.314 Y16.647 I0.339 J0.002\nM2\n";

/// [`ARC_VALID`] after `G90.1`: the arc's `I`, `J` are written from its
/// start, as DreamCarve writes them, but read as absolute, so the centre
/// is (-10, 0) and the radii are 20 and 14.1 mm. This is what a LinuxCNC
/// program without `G91.1` in its header would do after a program that
/// left `G90.1` on. It must be rejected.
pub const ABSOLUTE_CENTRE_ARC: &str =
    "G21\nG90\nG90.1\nG0 X10 Y0\nG1 F600\nG3 X0 Y10 I-10 J0\nM2\n";

/// How far `rs274`'s arc check goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArcTier {
    /// It rejects [`ARC_TIGHT`]: at least as strict as GRBL.
    Strict,
    /// It checks arcs but accepts [`ARC_TIGHT`]. LinuxCNC 2.9 is here.
    Tolerant,
}

/// The verdicts on the probe programs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeVerdicts {
    /// On [`SMOKE`].
    pub smoke: Verdict,
    /// On [`ARC_VALID`].
    pub arc_valid: Verdict,
    /// On [`ARC_GROSS`].
    pub arc_gross: Verdict,
    /// On [`ARC_TIGHT`].
    pub arc_tight: Verdict,
    /// On [`ABSOLUTE_CENTRE_ARC`].
    pub absolute_centre_arc: Verdict,
}

/// Why `rs274`'s verdicts cannot be trusted. None of these says anything
/// about DreamCarve's programs: the interpreter, or how it is run, is wrong.
#[derive(Debug, thiserror::Error)]
pub enum Misconfigured {
    /// `rs274` could not be started.
    #[error("cannot run `{rs274}`: {source} (install linuxcnc-uspace, or pass --rs274)")]
    Spawn {
        /// The binary.
        rs274: String,
        /// The OS error.
        source: io::Error,
    },
    /// A probe file could not be written.
    #[error("cannot write {path}: {source}")]
    Write {
        /// The file.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
    /// It rejected [`SMOKE`].
    #[error("rejected a known-good program ({0}): the binary or its flags are wrong")]
    SmokeRejected(String),
    /// It rejected [`ARC_VALID`].
    #[error("rejected a valid arc ({0})")]
    ValidArcRejected(String),
    /// It accepted [`ARC_GROSS`].
    #[error("accepted an arc 1 mm off its circle: it is not checking arcs")]
    GrossArcAccepted,
    /// It accepted [`ABSOLUTE_CENTRE_ARC`].
    #[error("accepted G90.1 followed by an arc with incremental centres")]
    AbsoluteCentreArcAccepted,
}

/// Decides from the probe verdicts whether `rs274` can be trusted, and how
/// strict its arc check is.
///
/// # Errors
///
/// [`Misconfigured`] when a probe went the wrong way.
pub fn classify(verdicts: &ProbeVerdicts) -> Result<ArcTier, Misconfigured> {
    if let Verdict::Rejected(why) = &verdicts.smoke {
        return Err(Misconfigured::SmokeRejected(why.clone()));
    }
    if let Verdict::Rejected(why) = &verdicts.arc_valid {
        return Err(Misconfigured::ValidArcRejected(why.clone()));
    }
    if verdicts.arc_gross == Verdict::Accepted {
        return Err(Misconfigured::GrossArcAccepted);
    }
    if verdicts.absolute_centre_arc == Verdict::Accepted {
        return Err(Misconfigured::AbsoluteCentreArcAccepted);
    }
    Ok(match verdicts.arc_tight {
        Verdict::Accepted => ArcTier::Tolerant,
        Verdict::Rejected(_) => ArcTier::Strict,
    })
}

/// Writes each probe program into `dir` and runs `rs274` on it.
///
/// # Errors
///
/// [`Misconfigured::Spawn`] or [`Misconfigured::Write`].
pub fn probe(rs274: &Path, dir: &Path) -> Result<ProbeVerdicts, Misconfigured> {
    let verdict = |name: &str, program: &str| -> Result<Verdict, Misconfigured> {
        let path = dir.join(format!("_probe-{name}.ngc"));
        fs::write(&path, program).map_err(|source| Misconfigured::Write {
            path: path.clone(),
            source,
        })?;
        let run = run_rs274(rs274, &path, dir).map_err(|source| Misconfigured::Spawn {
            rs274: rs274.display().to_string(),
            source,
        })?;
        Ok(judge(&run))
    };
    Ok(ProbeVerdicts {
        smoke: verdict("smoke", SMOKE)?,
        arc_valid: verdict("arc-valid", ARC_VALID)?,
        arc_gross: verdict("arc-gross", ARC_GROSS)?,
        arc_tight: verdict("arc-tight", ARC_TIGHT)?,
        absolute_centre_arc: verdict("absolute-centre-arc", ABSOLUTE_CENTRE_ARC)?,
    })
}

/// One program to run through `rs274`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    /// A short name for the report.
    pub name: String,
    /// The program.
    pub file: PathBuf,
    /// What the program exercises.
    pub covers: String,
}

/// The committed programs, relative to the workspace root.
const COMMITTED: &[(&str, &str, &str)] = &[
    (
        "post-linuxcnc-coaster",
        "tests/golden/post-linuxcnc-coaster/program.ngc",
        "LinuxCNC header (G40 G49 G80, G91.1), ramp entry, arcs, G4 dwell, M5 M2",
    ),
    (
        "post-generic-coaster",
        "tests/golden/post-generic-coaster/program.nc",
        "generic header, numbers without trailing zeros, arcs, M30",
    ),
    (
        "coaster-holes-linuxcnc-canned",
        "crates/dreamcarve/tests/expected/coaster-holes-linuxcnc-canned.ngc",
        "G98, G81 with R and F, G80 (cli::canned_cycles_linuxcnc_opt_in)",
    ),
];

/// The project exported for each post, relative to the workspace root.
const PLAQUE: &str = "tests/fixtures/plaque.dcv";

/// The posts whose programs `rs274` reads.
const POSTS: &[&str] = &["linuxcnc", "generic"];

/// Why the corpus could not be built.
#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    /// A file or directory could not be read or written.
    #[error("{path}: {source}")]
    Io {
        /// The file or directory.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
    /// The `dreamcarve` binary could not be started.
    #[error(
        "cannot run `{dreamcarve}`: {source} (run `cargo build -p dreamcarve`, or pass --dreamcarve)"
    )]
    Spawn {
        /// The binary.
        dreamcarve: String,
        /// The OS error.
        source: io::Error,
    },
    /// `dreamcarve export` failed.
    #[error("`dreamcarve export` for {post} failed:\n{stderr}")]
    Export {
        /// The post-processor.
        post: String,
        /// What it printed.
        stderr: String,
    },
    /// `dreamcarve export` wrote no program.
    #[error("`dreamcarve export` for {post} wrote no program in {dir}")]
    NoProgram {
        /// The post-processor.
        post: String,
        /// Where it should have written one.
        dir: PathBuf,
    },
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> CorpusError + '_ {
    move |source| CorpusError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// Builds the corpus: the committed programs under `root`, and the plaque
/// exported by `dreamcarve` for each post into `out`, which must exist.
/// `dreamcarve` reads an empty user config, so the programs are the
/// project's alone.
///
/// # Errors
///
/// [`CorpusError`] when a committed program is missing or an export fails.
pub fn build_corpus(root: &Path, dreamcarve: &Path, out: &Path) -> Result<Vec<Case>, CorpusError> {
    let mut cases = Vec::new();
    for (name, file, covers) in COMMITTED {
        let file = root.join(file);
        fs::metadata(&file).map_err(io_error(&file))?;
        cases.push(Case {
            name: (*name).to_owned(),
            file,
            covers: (*covers).to_owned(),
        });
    }

    let config = out.join("config");
    fs::create_dir_all(&config).map_err(io_error(&config))?;
    let plaque = root.join(PLAQUE);
    for post in POSTS {
        let dir = out.join(format!("plaque-{post}"));
        fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        let dcv = dir.join("plaque.dcv");
        fs::copy(&plaque, &dcv).map_err(io_error(&plaque))?;
        let export = Command::new(dreamcarve)
            .args([
                "export",
                "plaque.dcv",
                "--post",
                post,
                "--date",
                EXPORT_DATE,
            ])
            .current_dir(&dir)
            .env("DREAMCARVE_CONFIG_DIR", &config)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| CorpusError::Spawn {
                dreamcarve: dreamcarve.display().to_string(),
                source,
            })?;
        if !export.status.success() {
            return Err(CorpusError::Export {
                post: (*post).to_owned(),
                stderr: String::from_utf8_lossy(&export.stderr).into_owned(),
            });
        }
        let mut programs: Vec<PathBuf> = fs::read_dir(&dir)
            .map_err(io_error(&dir))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext != "dcv"))
            .collect();
        programs.sort();
        if programs.is_empty() {
            return Err(CorpusError::NoProgram {
                post: (*post).to_owned(),
                dir,
            });
        }
        for file in programs {
            let stem = file
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            cases.push(Case {
                name: format!("plaque-{post}/{stem}"),
                file,
                covers: format!(
                    "quick-start plaque on {post}: pockets, drill, profile, engrave, \
                     programs per tool"
                ),
            });
        }
    }
    Ok(cases)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(status: Option<i32>, stdout: &str, stderr: &str) -> Run {
        Run {
            status,
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
        }
    }

    const ENDED: &str = "   16 N..... SET_SPINDLE_MODE(0 0.0000)\n   17 N..... PROGRAM_END()\n   18 N..... ON_RESET()\n";

    #[test]
    fn accepts_a_run_that_ends_cleanly() {
        assert_eq!(
            judge(&run(Some(0), ENDED, "executing\n")),
            Verdict::Accepted
        );
    }

    #[test]
    fn rejects_an_error_with_rs274s_message() {
        let stderr = "executing\nRadius to end of arc differs from radius to start: r1=20.0000 r2=14.1421\nG3 X0 Y10 I-10 J0\n";
        let Verdict::Rejected(why) = judge(&run(Some(1), "", stderr)) else {
            panic!("an exit of 1 is a rejection");
        };
        assert!(why.contains("Radius to end of arc differs"), "{why}");
        assert!(why.contains("G3 X0 Y10 I-10 J0"), "{why}");
        assert!(!why.contains("executing"), "{why}");
    }

    #[test]
    fn rejects_a_failed_exit_with_no_message() {
        assert_eq!(
            judge(&run(Some(3), ENDED, "executing\n")),
            Verdict::Rejected("exited 3".to_owned())
        );
    }

    #[test]
    fn rejects_a_warning_even_when_rs274_exits_0() {
        let Verdict::Rejected(why) = judge(&run(Some(0), ENDED, "executing\nsomething odd\n"))
        else {
            panic!("a warning is a rejection");
        };
        assert!(why.starts_with("warning: something odd"), "{why}");
    }

    #[test]
    fn rejects_a_run_that_never_reaches_the_program_end() {
        let stdout = "    5 N..... SET_FEED_REFERENCE(CANON_XYZ)\n    6 N..... ON_RESET()\n";
        assert!(matches!(
            judge(&run(Some(0), stdout, "executing\n")),
            Verdict::Rejected(_)
        ));
    }

    #[test]
    fn rejects_a_run_ended_by_a_signal() {
        assert!(matches!(
            judge(&run(None, ENDED, "executing\n")),
            Verdict::Rejected(_)
        ));
    }

    /// The issue's bad program: `G90.1`, then the valid arc with its centre
    /// written from the start, so only the centre mode differs from
    /// [`ARC_VALID`].
    #[test]
    fn bad_program_is_g90_1_then_an_incremental_arc() {
        let lines: Vec<&str> = ABSOLUTE_CENTRE_ARC.lines().collect();
        let g90_1 = lines.iter().position(|l| *l == "G90.1");
        let arc = lines
            .iter()
            .position(|l| l.starts_with("G3 ") && l.contains('I'));
        assert!(
            matches!((g90_1, arc), (Some(m), Some(a)) if m < a),
            "{lines:?}"
        );
        assert_eq!(ABSOLUTE_CENTRE_ARC.replace("G90.1\n", ""), ARC_VALID);
    }

    fn verdicts() -> ProbeVerdicts {
        let rejected = || Verdict::Rejected("Radius to end of arc differs".to_owned());
        ProbeVerdicts {
            smoke: Verdict::Accepted,
            arc_valid: Verdict::Accepted,
            arc_gross: rejected(),
            arc_tight: Verdict::Accepted,
            absolute_centre_arc: rejected(),
        }
    }

    #[test]
    fn linuxcnc_2_9_is_tolerant() {
        assert_eq!(classify(&verdicts()).ok(), Some(ArcTier::Tolerant));
    }

    #[test]
    fn rejecting_the_tight_arc_is_strict() {
        let mut v = verdicts();
        v.arc_tight = Verdict::Rejected("radius".to_owned());
        assert_eq!(classify(&v).ok(), Some(ArcTier::Strict));
    }

    #[test]
    fn a_rejected_smoke_program_is_misconfigured() {
        let mut v = verdicts();
        v.smoke = Verdict::Rejected("exited 1".to_owned());
        assert!(matches!(classify(&v), Err(Misconfigured::SmokeRejected(_))));
    }

    #[test]
    fn a_rejected_valid_arc_is_misconfigured() {
        let mut v = verdicts();
        v.arc_valid = Verdict::Rejected("exited 1".to_owned());
        assert!(matches!(
            classify(&v),
            Err(Misconfigured::ValidArcRejected(_))
        ));
    }

    #[test]
    fn accepting_the_gross_arc_is_misconfigured() {
        let mut v = verdicts();
        v.arc_gross = Verdict::Accepted;
        assert!(matches!(classify(&v), Err(Misconfigured::GrossArcAccepted)));
    }

    #[test]
    fn accepting_the_bad_program_is_misconfigured() {
        let mut v = verdicts();
        v.absolute_centre_arc = Verdict::Accepted;
        assert!(matches!(
            classify(&v),
            Err(Misconfigured::AbsoluteCentreArcAccepted)
        ));
    }
}
