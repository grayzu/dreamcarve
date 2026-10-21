// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The machine configurations a session runs, and the files a session
//! directory holds: the INI file, its HAL file, the headless UI, a tool
//! table, the parameter file with the work offset, the programs and the
//! list of them.

use std::fs;
use std::io;
use std::path::Path;

use crate::WORK_OFFSET_MM;

const INI_TEMPLATE: &str = include_str!("../sim/machine.ini.in");
const MACHINE_HAL: &str = include_str!("../sim/machine.hal");
const LOGGER_HAL: &str = include_str!("../sim/logger.hal");
const UI: &str = include_str!("../sim/dreamcarve-ui.py");

/// The UI's file name; the INI names it as the display.
pub const UI_FILE: &str = "dreamcarve-ui.py";
/// The UI's record of the session.
pub const RESULT_FILE: &str = "result.txt";

/// Modes set by the INI's startup code that the header of a LinuxCNC
/// program must override: inches, incremental distance, absolute arc
/// centres, the XZ plane, inverse-time feed, the G55 work offset and
/// `G99` retract. LinuxCNC applies `[RS274NGC] RS274NGC_STARTUP_CODE` at
/// start and after every program end (INI configuration document,
/// `[RS274NGC]`).
pub const HOSTILE_STARTUP: &str = "G20 G91 G90.1 G18 G93 G55 G99";

/// Z travel of [`Machine::ShortZ`], machine millimetres: it ends at -20,
/// below the corpus's `G53 G0 Z-5` lift.
pub const SHORT_Z_MAX_MM: f64 = -20.0;

/// The bottom of Z travel on every machine, machine millimetres.
pub const Z_MIN_MM: f64 = -150.0;

/// A simulated machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Machine {
    /// motion-logger in place of the motion module: every motion command,
    /// with its end point and source line, goes to a log. Homes, but moves
    /// nothing and checks no limits.
    Logger,
    /// LinuxCNC's motion module, homed, Z travel [-150, 0] mm: the lift
    /// to machine Z -5 is inside it.
    Travel,
    /// As [`Machine::Travel`] with Z travel [-150, -20] mm: the lift is
    /// beyond the top.
    ShortZ,
    /// As [`Machine::Travel`], never homed.
    Unhomed,
}

impl Machine {
    /// The INI file's name.
    #[must_use]
    pub const fn ini_file(self) -> &'static str {
        match self {
            Self::Logger => "logger.ini",
            Self::Travel => "travel.ini",
            Self::ShortZ => "short-z.ini",
            Self::Unhomed => "unhomed.ini",
        }
    }

    /// The top of Z travel, machine millimetres. Z homes here.
    #[must_use]
    pub const fn z_max_mm(self) -> f64 {
        match self {
            Self::ShortZ => SHORT_Z_MAX_MM,
            _ => 0.0,
        }
    }

    /// The INI file, with `startup` as the startup code if given.
    #[must_use]
    pub fn ini(self, startup: Option<&str>) -> String {
        let logger = self == Self::Logger;
        INI_TEMPLATE
            .replace("@HOME@", if self == Self::Unhomed { "0" } else { "1" })
            .replace("@LOGGER@", if logger { "1" } else { "0" })
            .replace(
                "@STARTUP@",
                &startup.map_or_else(String::new, |s| format!("RS274NGC_STARTUP_CODE = {s}")),
            )
            // With no EMCMOT, `linuxcnc` loads no motion module, and the
            // logger answers task in its place (tests/motion-logger).
            .replace("@EMCMOT@", if logger { "" } else { "EMCMOT = motmod" })
            .replace(
                "@HALFILE@",
                if logger { "logger.hal" } else { "machine.hal" },
            )
            .replace("@Z_MIN@", &format!("{Z_MIN_MM}"))
            .replace("@Z_MAX@", &format!("{}", self.z_max_mm()))
    }
}

/// One run of LinuxCNC: a machine, an optional startup code, and the
/// programs it runs in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// The machine.
    pub machine: Machine,
    /// The startup code, if not LinuxCNC's default.
    pub startup: Option<String>,
    /// (file name, text) of each program.
    pub programs: Vec<(String, String)>,
}

impl Session {
    /// A session on `machine` running `texts`, named `p001.ngc` on.
    #[must_use]
    pub fn new<'a>(machine: Machine, texts: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            machine,
            startup: None,
            programs: texts
                .into_iter()
                .enumerate()
                .map(|(k, text)| (format!("p{:03}.ngc", k + 1), text.to_owned()))
                .collect(),
        }
    }

    /// Writes the session's files into `dir`, which must exist, after
    /// removing what an earlier session there left.
    ///
    /// # Errors
    ///
    /// The OS error when a file cannot be written or removed.
    pub fn write(&self, dir: &Path) -> io::Result<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            let stale = path.extension().is_some_and(|e| e == "log")
                || path.file_name().is_some_and(|n| n == RESULT_FILE);
            if stale {
                fs::remove_file(&path)?;
            }
        }
        fs::write(
            dir.join(self.machine.ini_file()),
            self.machine.ini(self.startup.as_deref()),
        )?;
        fs::write(dir.join("machine.hal"), MACHINE_HAL)?;
        fs::write(dir.join("logger.hal"), LOGGER_HAL)?;
        fs::write(dir.join(UI_FILE), UI)?;
        make_executable(&dir.join(UI_FILE))?;
        fs::write(dir.join("tool.tbl"), "T1 P1 D3.175 ;DreamCarve sim\n")?;
        fs::write(dir.join("sim.var"), parameters())?;
        let mut jobs = String::new();
        for (name, text) in &self.programs {
            fs::write(dir.join(name), text)?;
            jobs.push_str(name);
            jobs.push('\n');
        }
        fs::write(dir.join("jobs.txt"), jobs)
    }
}

/// The parameter file: G54 (5220 = 1) with [`WORK_OFFSET_MM`]
/// (5221-5223), in the INI's units, millimetres
/// (<https://linuxcnc.org/docs/html/gcode/overview.html#sub:numbered-parameters>).
fn parameters() -> String {
    let [x, y, z] = WORK_OFFSET_MM;
    format!("5220\t1.000000\n5221\t{x:.6}\n5222\t{y:.6}\n5223\t{z:.6}\n")
}

/// LinuxCNC runs the display as `<DISPLAY> -ini <file>`, so the UI must
/// be executable.
#[cfg(unix)]
fn make_executable(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<'a>(ini: &'a str, key: &str) -> Vec<&'a str> {
        ini.lines()
            .filter_map(|l| l.strip_prefix(key))
            .filter_map(|r| r.trim_start().strip_prefix('='))
            .map(str::trim)
            .collect()
    }

    #[test]
    fn every_placeholder_is_filled() {
        for machine in [
            Machine::Logger,
            Machine::Travel,
            Machine::ShortZ,
            Machine::Unhomed,
        ] {
            let ini = machine.ini(Some(HOSTILE_STARTUP));
            assert!(!ini.contains('@'), "{machine:?}: {ini}");
        }
    }

    #[test]
    fn short_z_travel_ends_below_the_lift_and_homes_inside_it() {
        let ini = Machine::ShortZ.ini(None);
        assert_eq!(
            value(&ini, "MAX_LIMIT"),
            ["1000", "1000", "-20", "1000", "1000", "-20"]
        );
        assert_eq!(value(&ini, "HOME"), ["1", "0", "0", "-20"]);
        let travel = Machine::Travel.ini(None);
        assert_eq!(value(&travel, "MAX_LIMIT")[2], "0");
    }

    #[test]
    fn only_the_logger_drops_the_motion_module() {
        assert!(value(&Machine::Logger.ini(None), "EMCMOT").is_empty());
        assert_eq!(value(&Machine::Travel.ini(None), "EMCMOT"), ["motmod"]);
        assert_eq!(value(&Machine::Unhomed.ini(None), "HOME")[0], "0");
    }

    #[test]
    fn parameter_file_sets_g54_in_millimetres() {
        assert_eq!(
            parameters(),
            "5220\t1.000000\n5221\t25.400000\n5222\t50.800000\n5223\t-76.200000\n"
        );
    }
}
