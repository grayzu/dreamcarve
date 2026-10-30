// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The programs the comparison runs on: every committed LinuxCNC, Mach3,
//! UCCNC and generic program, and the quick-start plaque exported
//! in-process for each of those controllers, with the variants that
//! exercise what a coaster does not: canned cycles (`G81`, `G82` with
//! `G98`), a `G53` machine-Z lift, the same with manual spindle control
//! (`M0` pauses, one right after the lift), and an inch program.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use dc_cam::{
    RunLabel, generate_bore, generate_drill, generate_engrave, generate_pocket, generate_profile,
    generate_vcarve,
};
use dc_gcode::{EmitOp, ProgramSplit, emit_programs};
use dc_project::{
    DrillCycle, DrillOp, Heights, Operation, ParkXy, PostProcessor, PostProcessorRef, Project,
    SpindleControl, ToolId, Units, bundled_definition, latest_definition, load,
};

/// The definitions whose programs `rs274` can read, with the bridge.
pub const POSTS: &[&str] = &["linuxcnc", "generic", "mach3", "uccnc"];

/// The date written into exported programs, so a run is reproducible.
pub const EXPORT_DATE: &str = "2026-10-15";

/// One program to compare.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// A short name for the report.
    pub name: String,
    /// The definition it was written with.
    pub post: PostProcessor,
    /// The units the controller is set up in, for a definition with no
    /// units word; the program's own units otherwise.
    pub units: Units,
    /// The program.
    pub text: String,
}

/// Why the corpus could not be built.
#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    /// A committed file could not be read.
    #[error("{path}: {source}")]
    Io {
        /// The file.
        path: PathBuf,
        /// The OS error.
        source: io::Error,
    },
    /// The plaque fixture did not load.
    #[error("{path}: {message}")]
    Load {
        /// The file.
        path: PathBuf,
        /// What `dc-project` said.
        message: String,
    },
    /// A bundled definition is missing.
    #[error("definition {0}: {1}")]
    Definition(String, String),
    /// An exported variant did not generate or emit.
    #[error("{case}: {message}")]
    Export {
        /// The case.
        case: String,
        /// What failed.
        message: String,
    },
}

/// The committed programs, relative to the workspace root: (file,
/// definition id, version or `None` for the latest).
const COMMITTED: &[(&str, &str, Option<u32>)] = &[
    (
        "tests/golden/post-linuxcnc-coaster/program.ngc",
        "linuxcnc",
        None,
    ),
    (
        "tests/golden/post-generic-coaster/program.nc",
        "generic",
        None,
    ),
    ("tests/golden/post-mach3-coaster/program.tap", "mach3", None),
    ("tests/golden/post-uccnc-coaster/program.txt", "uccnc", None),
    (
        "crates/dreamcarve/tests/expected/coaster-linuxcnc-v1.ngc",
        "linuxcnc",
        Some(1),
    ),
    (
        "crates/dreamcarve/tests/expected/coaster-mach3-v1.tap",
        "mach3",
        Some(1),
    ),
    (
        "crates/dreamcarve/tests/expected/coaster-mach3-v2.tap",
        "mach3",
        Some(2),
    ),
    (
        "crates/dreamcarve/tests/expected/coaster-uccnc-v1.txt",
        "uccnc",
        Some(1),
    ),
    (
        "crates/dreamcarve/tests/expected/coaster-uccnc-v2.txt",
        "uccnc",
        Some(2),
    ),
];

/// The project every exported variant starts from.
const PLAQUE: &str = "tests/fixtures/plaque.dcv";

/// Builds the corpus from the workspace at `root`.
///
/// # Errors
///
/// [`CorpusError`] when a committed program or the fixture is missing, or
/// a variant does not export.
pub fn build_corpus(root: &Path) -> Result<Vec<Case>, CorpusError> {
    let mut cases = Vec::new();
    for (file, id, version) in COMMITTED {
        let path = root.join(file);
        let text = fs::read_to_string(&path).map_err(|source| CorpusError::Io {
            path: path.clone(),
            source,
        })?;
        let definition = match version {
            Some(v) => bundled_definition(id, *v),
            None => latest_definition(id),
        }
        .map_err(|e| CorpusError::Definition((*id).to_owned(), e.to_string()))?;
        cases.push(Case {
            name: (*file).to_owned(),
            post: definition.post_processor().clone(),
            units: Units::Mm,
            text,
        });
    }

    let path = root.join(PLAQUE);
    let bytes = fs::read(&path).map_err(|source| CorpusError::Io {
        path: path.clone(),
        source,
    })?;
    let plaque = load(&bytes)
        .map_err(|e| CorpusError::Load {
            path: path.clone(),
            message: e.to_string(),
        })?
        .project;
    for id in POSTS {
        for (variant, project) in variants(&plaque, id)? {
            let post = project
                .post_processor
                .resolve()
                .map_err(|e| CorpusError::Definition((*id).to_owned(), e.to_string()))?;
            let name = format!("plaque/{id}/{variant}");
            let programs = export(&project).map_err(|message| CorpusError::Export {
                case: name.clone(),
                message,
            })?;
            for (k, text) in programs.into_iter().enumerate() {
                cases.push(Case {
                    name: format!("{name}/program-{}", k + 1),
                    post: post.clone(),
                    units: project.display_units,
                    text,
                });
            }
        }
    }
    Ok(cases)
}

/// The plaque on definition `id`, in each variant that definition can
/// write.
fn variants(plaque: &Project, id: &str) -> Result<Vec<(&'static str, Project)>, CorpusError> {
    let reference = PostProcessorRef::latest(id)
        .map_err(|e| CorpusError::Definition(id.to_owned(), e.to_string()))?;
    let post = reference
        .resolve()
        .map_err(|e| CorpusError::Definition(id.to_owned(), e.to_string()))?;
    let mut base = plaque.clone();
    base.post_processor = reference;
    let mut out = vec![("plain", base.clone())];

    if post.canned_cycles.is_some() {
        // The plaque bores its holes (maintainer decision 2026-10-24), so
        // these variants drill them as it did before: a simple Drill with
        // the 6.35 mm bit is `G81`, and one with a dwell `G82`.
        for (variant, dwell_s) in [("canned-g81", 0.0), ("canned-g82", 0.5)] {
            let mut p = base.clone();
            p.post_processor.use_canned_cycles = true;
            for op in &mut p.operations {
                if let Operation::Bore(bore) = op {
                    *op = Operation::Drill(DrillOp {
                        id: bore.id.clone(),
                        name: bore.name.clone(),
                        enabled: bore.enabled,
                        targets: bore.targets.clone(),
                        tool: ToolId::new("t2"),
                        cycle: DrillCycle::Simple,
                        dwell_s,
                        depth_mm: bore.depth_mm,
                        retract_mm: 1.0,
                        through_cut_allowance_mm: bore.through_cut_allowance_mm,
                        safe_z_mm: bore.safe_z_mm,
                        allow_tool_wider_than_hole: true,
                    });
                }
            }
            out.push((variant, p));
        }
    }
    if post.work_coordinates.machine_command.is_some() {
        // Acceptance 45's heights: a G53 lift to machine Z -5 mm.
        let mut p = base.clone();
        p.heights = Heights {
            clearance_mm: Some(20.0),
            tool_change_machine_z_mm: Some(-5.0),
            park: Some(ParkXy {
                x_mm: 0.0,
                y_mm: 0.0,
            }),
        };
        out.push(("machine-z", p.clone()));
        // With manual spindle control, `M0` pauses: one to turn the
        // router on, and one to turn it off right after the lift, so a
        // full simulation sees the tool at the lift's machine Z while
        // paused (issue #328).
        p.post_processor.spindle_control = SpindleControl::Manual;
        out.push(("manual-machine-z", p));
    }
    if matches!(id, "linuxcnc" | "uccnc") {
        let mut p = base;
        p.display_units = Units::Inch;
        out.push(("inch", p));
    }
    Ok(out)
}

/// Generates every enabled operation of `project` and emits its programs,
/// as `dreamcarve export` does (`crates/dreamcarve/src/export.rs`,
/// `build`), without its validation and reporting.
///
/// # Errors
///
/// What failed, when an operation has no tool or does not generate or
/// emit.
pub fn export(project: &Project) -> Result<Vec<String>, String> {
    let mut ops = Vec::new();
    for operation in project.operations.iter().filter(|o| o.enabled()) {
        let tool = project
            .tools
            .iter()
            .find(|t| &t.id == operation.tool())
            .ok_or_else(|| format!("operation {} has no tool", operation.id()))?
            .clone();
        let (toolpath, holes) = match operation {
            Operation::Profile(op) => (generate_profile(project, op).map_err(err)?.0, None),
            Operation::Pocket(op) => (generate_pocket(project, op).map_err(err)?.0, None),
            Operation::Engrave(op) => (generate_engrave(project, op).map_err(err)?.0, None),
            Operation::Bore(op) => (generate_bore(project, op).map_err(err)?.0, None),
            // Format 20: each tool run is written with its own tool.
            Operation::VCarve(op) => {
                for run in generate_vcarve(project, op).map_err(err)?.0 {
                    let tool = project
                        .tools
                        .iter()
                        .find(|t| t.id == run.tool)
                        .ok_or_else(|| format!("operation {} has no tool", operation.id()))?
                        .clone();
                    ops.push(EmitOp {
                        op: operation.clone(),
                        tool,
                        toolpath: run.toolpath,
                        holes: None,
                        label: run.label,
                    });
                }
                continue;
            }
            Operation::Drill(op) => {
                let (drilled, _) = generate_drill(project, op).map_err(err)?;
                (drilled.toolpath, Some(drilled.holes))
            }
            // Format 22: the Inlay's generator lands in #436.
            Operation::Inlay(_) => {
                return Err(format!(
                    "operation {} is an Inlay, which this build does not generate yet",
                    operation.id()
                ));
            }
        };
        ops.push(EmitOp {
            op: operation.clone(),
            tool,
            toolpath,
            holes,
            label: RunLabel::Main,
        });
    }
    let (programs, _) =
        emit_programs(project, &ops, ProgramSplit::ProjectDefault, EXPORT_DATE).map_err(err)?;
    Ok(programs.into_iter().map(|p| p.text).collect())
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
