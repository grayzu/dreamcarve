// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Runs LinuxCNC's `rs274` on a bridged program, natively or in a
//! container, as a separate process. LinuxCNC is GPL-2.0; nothing of it is
//! linked or copied here.
//!
//! Where `rs274` comes from, in order:
//!
//! 1. `DREAMCARVE_RS274`: a path to the binary, or `podman:<image>` or
//!    `docker:<image>` for an image that has `rs274` on its `PATH` (the
//!    `Containerfile` beside this crate builds one).
//! 2. `rs274` on `PATH` (Debian and Ubuntu's `linuxcnc-uspace`).
//!
//! The run reads a parameter file that sets a G54 work offset of
//! [`WORK_OFFSET_INCH`]. With no offset, work and machine coordinates are
//! the same and a `G53` move looks like any other; with one, the
//! comparison sees `rs274` move to the machine Z the program names.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The environment variable naming the `rs274` to run.
pub const ENV: &str = "DREAMCARVE_RS274";

/// The G54 offset the parameter file sets: X, Y, Z. `rs274` with no INI
/// file reads the parameter file in inches, so this is (25.4, 50.8,
/// -76.2) mm. A machine-Z lift to Z -5 mm is work Z 71.2 under it.
pub const WORK_OFFSET_INCH: [f64; 3] = [1.0, 2.0, -3.0];

/// The flags `rs274` runs with. `-g` is batch mode: without it, `rs274`
/// asks on standard input what to do before it starts
/// (`src/emc/sai/driver.cc`). `-v` names the parameter file.
const FLAGS: &[&str] = &["-g", "-v", PARAMETERS];

const PARAMETERS: &str = "rs274.var";
const PROGRAM: &str = "program.ngc";

/// The banner `rs274` writes to standard error before every run.
const BANNER: &str = "executing";

/// How to start `rs274`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Rs274 {
    /// The binary itself.
    Native(PathBuf),
    /// An image run with `podman` or `docker`.
    Container {
        /// `podman` or `docker`.
        engine: String,
        /// The image, with `rs274` on its `PATH`.
        image: String,
    },
}

impl Rs274 {
    /// Reads a [`ENV`] value: `podman:<image>`, `docker:<image>`, or a
    /// path.
    #[must_use]
    pub fn from_spec(spec: &str) -> Self {
        for engine in ["podman", "docker"] {
            if let Some(image) = spec.strip_prefix(engine).and_then(|r| r.strip_prefix(':')) {
                return Self::Container {
                    engine: engine.to_owned(),
                    image: image.to_owned(),
                };
            }
        }
        Self::Native(PathBuf::from(spec))
    }

    /// The `rs274` [`ENV`] names, else `rs274` on `PATH`, else `None`.
    #[must_use]
    pub fn detect() -> Option<Self> {
        if let Some(spec) = std::env::var_os(ENV).filter(|s| !s.is_empty()) {
            return Some(Self::from_spec(&spec.to_string_lossy()));
        }
        on_path("rs274").map(Self::Native)
    }

    /// Runs `program` through `rs274` in `dir`, which must exist and is
    /// mounted into a container read-write (`rs274` writes its parameter
    /// file back).
    ///
    /// # Errors
    ///
    /// The OS error when a file cannot be written or `rs274` cannot be
    /// started.
    pub fn run(&self, program: &str, dir: &Path) -> io::Result<Run> {
        fs::write(dir.join(PROGRAM), program)?;
        fs::write(dir.join(PARAMETERS), parameters())?;
        let mut command = match self {
            Self::Native(rs274) => {
                let mut c = Command::new(rs274);
                c.current_dir(dir).args(FLAGS).arg(PROGRAM);
                c
            }
            Self::Container { engine, image } => {
                let mut mount = OsString::from(dir.as_os_str());
                mount.push(":/work");
                let mut c = Command::new(engine);
                c.args(["run", "--rm", "--network", "none", "-v"])
                    .arg(mount)
                    .args(["-w", "/work", image, "rs274"])
                    .args(FLAGS)
                    .arg(PROGRAM);
                c
            }
        };
        let out = command.stdin(Stdio::null()).output()?;
        Ok(Run {
            status: out.status.code(),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

impl std::fmt::Display for Rs274 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native(path) => write!(f, "{}", path.display()),
            Self::Container { engine, image } => write!(f, "rs274 in {engine} image {image}"),
        }
    }
}

/// One run of `rs274`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    /// The exit code; `None` when a signal ended it.
    pub status: Option<i32>,
    /// The canonical calls.
    pub stdout: String,
    /// The banner, then any error or warning.
    pub stderr: String,
}

impl Run {
    /// Why `rs274` did not accept the program, or `None` if it did: it ran
    /// to `M2`/`M30`, exited 0, and wrote nothing to standard error but
    /// its banner. The rule PR #264's `gcode-conformance` judges by.
    #[must_use]
    pub fn rejection(&self) -> Option<String> {
        let messages: Vec<&str> = self
            .stderr
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && *l != BANNER)
            .collect();
        if self.status != Some(0) {
            return Some(format!(
                "exited {:?}: {}",
                self.status,
                messages.join(" | ")
            ));
        }
        if !messages.is_empty() {
            return Some(format!("warning: {}", messages.join(" | ")));
        }
        if !self
            .stdout
            .lines()
            .any(|l| l.trim_end().ends_with("PROGRAM_END()"))
        {
            return Some("never reached the program end (M2 or M30)".to_owned());
        }
        None
    }
}

/// The parameter file: G54 (5220 = 1) with [`WORK_OFFSET_INCH`]
/// (5221-5223), the parameters LinuxCNC's documentation lists for the
/// first coordinate system
/// (<https://linuxcnc.org/docs/html/gcode/overview.html#sub:numbered-parameters>).
fn parameters() -> String {
    let [x, y, z] = WORK_OFFSET_INCH;
    format!("5220\t1.000000\n5221\t{x:.6}\n5222\t{y:.6}\n5223\t{z:.6}\n")
}

fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_container_and_path_specs() {
        assert_eq!(
            Rs274::from_spec("podman:dreamcarve-rs274"),
            Rs274::Container {
                engine: "podman".to_owned(),
                image: "dreamcarve-rs274".to_owned()
            }
        );
        assert_eq!(
            Rs274::from_spec("docker:ghcr.io/x/y:1"),
            Rs274::Container {
                engine: "docker".to_owned(),
                image: "ghcr.io/x/y:1".to_owned()
            }
        );
        assert_eq!(
            Rs274::from_spec("/usr/bin/rs274"),
            Rs274::Native(PathBuf::from("/usr/bin/rs274"))
        );
    }

    fn run(status: Option<i32>, stdout: &str, stderr: &str) -> Run {
        Run {
            status,
            stdout: stdout.to_owned(),
            stderr: stderr.to_owned(),
        }
    }

    #[test]
    fn a_clean_run_to_the_end_is_accepted() {
        let r = run(Some(0), "   73 N18    PROGRAM_END()\n", "executing\n");
        assert_eq!(r.rejection(), None);
    }

    #[test]
    fn an_error_a_warning_or_no_end_is_rejected() {
        let error = run(Some(1), "", "executing\nUnknown g code used\nN40 G50\n");
        assert!(
            error
                .rejection()
                .is_some_and(|r| r.contains("Unknown g code used")),
            "{error:?}"
        );
        let warning = run(Some(0), "PROGRAM_END()\n", "executing\nodd\n");
        assert!(
            warning
                .rejection()
                .is_some_and(|r| r.starts_with("warning"))
        );
        let unended = run(Some(0), "    1 N1     ON_RESET()\n", "executing\n");
        assert!(unended.rejection().is_some());
    }

    #[test]
    fn parameter_file_sets_g54() {
        assert_eq!(
            parameters(),
            "5220\t1.000000\n5221\t1.000000\n5222\t2.000000\n5223\t-3.000000\n"
        );
    }
}
