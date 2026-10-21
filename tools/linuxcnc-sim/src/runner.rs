// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Runs `linuxcnc -r <ini>` on a session directory, natively or in a
//! container, as a separate process. LinuxCNC is GPL-2.0; nothing of it is
//! linked or copied here.
//!
//! LinuxCNC comes only from [`ENV`]: a path to the `linuxcnc` script, or
//! `podman:<image>` or `docker:<image>` for an image that has it on its
//! `PATH`. The image `tools/rs274-motion/Containerfile` builds has all of
//! LinuxCNC. Unlike `rs274-motion`, a `linuxcnc` on `PATH` is not used
//! unasked: a session starts a whole LinuxCNC, the runs take minutes,
//! LinuxCNC refuses to run as root (as in a CI container), and a native
//! session shares the host's HAL memory with any LinuxCNC already running.

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::{RESULT_FILE, Session};
use crate::report::{ReportError, SessionReport, parse_report};

/// The environment variable naming the LinuxCNC to run.
pub const ENV: &str = "DREAMCARVE_LINUXCNC";

/// How long one session may take, seconds, before `timeout` ends it in a
/// container. The UI aborts any one program after its own `TIMEOUT`.
const SESSION_TIMEOUT_S: u32 = 1800;

/// How to start LinuxCNC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxCnc {
    /// The `linuxcnc` script itself.
    Native(PathBuf),
    /// An image run with `podman` or `docker`.
    Container {
        /// `podman` or `docker`.
        engine: String,
        /// The image, with `linuxcnc` on its `PATH`.
        image: String,
    },
}

/// Why a session gave no report.
#[derive(Debug, thiserror::Error)]
pub enum RunError {
    /// A file could not be written, or LinuxCNC could not be started.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// LinuxCNC ran but the UI left no complete record: the output says
    /// why.
    #[error("LinuxCNC exited {status:?} without a complete report ({why}):\n{output}")]
    NoReport {
        /// The exit code.
        status: Option<i32>,
        /// What was wrong with the record.
        why: String,
        /// LinuxCNC's standard output and error.
        output: String,
    },
}

/// What one session left: the UI's record and the motion logs.
#[derive(Debug, Clone, PartialEq)]
pub struct SessionRun {
    /// The UI's record.
    pub report: SessionReport,
    /// For each program, in order, its motion-logger log; empty unless
    /// the machine is [`crate::config::Machine::Logger`].
    pub logs: Vec<String>,
    /// LinuxCNC's standard output and error.
    pub output: String,
}

impl LinuxCnc {
    /// Reads an [`ENV`] value: `podman:<image>`, `docker:<image>`, or a
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

    /// The LinuxCNC [`ENV`] names, or `None`.
    #[must_use]
    pub fn detect() -> Option<Self> {
        std::env::var(ENV)
            .ok()
            .filter(|s| !s.is_empty())
            .map(|s| Self::from_spec(&s))
    }

    /// Writes `session` into `dir`, which must exist, runs it, and reads
    /// what it left.
    ///
    /// # Errors
    ///
    /// [`RunError`] when the files cannot be written, LinuxCNC does not
    /// start, or the UI did not finish its record.
    pub fn run(&self, session: &Session, dir: &Path) -> Result<SessionRun, RunError> {
        session.write(dir)?;
        let ini = session.machine.ini_file();
        let mut command = match self {
            Self::Native(linuxcnc) => {
                let mut c = Command::new(linuxcnc);
                c.current_dir(dir).args(["-r", ini]);
                c
            }
            Self::Container { engine, image } => {
                let mut mount = OsString::from(dir.as_os_str());
                mount.push(":/work");
                let mut c = Command::new(engine);
                // `rtapi_app` is setuid root. With the bit in force it
                // creates HAL's shared memory as root, and the user's
                // `halcmd` is refused it. `no-new-privileges` leaves it
                // running as the user, in LinuxCNC's non-realtime POSIX
                // mode, as LinuxCNC's own tests run from a build tree.
                c.args([
                    "run",
                    "--rm",
                    "--network",
                    "none",
                    "--security-opt",
                    "no-new-privileges",
                    "-v",
                ])
                .arg(mount)
                .args(["-w", "/work", image, "timeout"])
                .arg(SESSION_TIMEOUT_S.to_string())
                .args(["linuxcnc", "-r", ini]);
                c
            }
        };
        let out = command.stdin(Stdio::null()).output()?;
        let output = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let no_report = |why: String| RunError::NoReport {
            status: out.status.code(),
            why,
            output: output.clone(),
        };
        let text = fs::read_to_string(dir.join(RESULT_FILE))
            .map_err(|e| no_report(format!("{RESULT_FILE}: {e}")))?;
        let report = parse_report(&text).map_err(|e: ReportError| no_report(e.to_string()))?;
        if !report.done || report.programs.len() != session.programs.len() {
            return Err(no_report(format!(
                "{} of {} program(s) reported{}",
                report.programs.len(),
                session.programs.len(),
                if report.done { "" } else { ", never DONE" }
            )));
        }
        let mut logs = Vec::new();
        if session.machine == crate::config::Machine::Logger {
            for (name, _) in &session.programs {
                let log = dir.join(format!("{name}.log"));
                logs.push(
                    fs::read_to_string(&log)
                        .map_err(|e| no_report(format!("{}: {e}", log.display())))?,
                );
            }
        }
        Ok(SessionRun {
            report,
            logs,
            output,
        })
    }
}

impl std::fmt::Display for LinuxCnc {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Native(path) => write!(f, "{}", path.display()),
            Self::Container { engine, image } => {
                write!(f, "linuxcnc in {engine} image {image}")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_container_and_path_specs() {
        assert_eq!(
            LinuxCnc::from_spec("podman:dreamcarve-rs274"),
            LinuxCnc::Container {
                engine: "podman".to_owned(),
                image: "dreamcarve-rs274".to_owned()
            }
        );
        assert_eq!(
            LinuxCnc::from_spec("/usr/bin/linuxcnc"),
            LinuxCnc::Native(PathBuf::from("/usr/bin/linuxcnc"))
        );
    }
}
