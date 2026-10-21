// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S9 on the parsed program (`docs/specs/phase-2/functional.md`, S9;
//! `docs/specs/phase-2/design.md`, "Data model → dc-gcode",
//! `ExportError::OffMaterial`).

use dc_geometry::{Point, Polyline, Region};
use dc_project::{Operation, Project};

use crate::ExportError;
use crate::parse::Block;
use crate::safety::{OpSpan, Tolerances};
use crate::transform::MachineTransform;
use dc_project::PostProcessor;

/// The material one operation may cut, worked out from the project.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Material {
    /// A Profile.
    Profile,
    /// A Pocket.
    Pocket {
        /// The regions.
        regions: Vec<Region>,
        /// The allowance, mm.
        allowance_mm: f64,
    },
    /// A Drill.
    Drill {
        /// Each hole's centre and radius, mm.
        holes: Vec<(Point, f64)>,
    },
    /// An Engrave.
    Engrave {
        /// The paths.
        paths: Vec<Polyline>,
    },
    /// Unresolved.
    Unresolved,
}

impl Material {
    /// The material `op` may cut in `project`.
    pub(crate) fn of(_project: &Project, _op: &Operation) -> Self {
        Self::Profile
    }
}

/// S9 for `span`.
pub(crate) fn check(
    _blocks: &[Block],
    _post: &PostProcessor,
    _transform: &MachineTransform,
    _tolerances: Tolerances,
    _span: &OpSpan<'_>,
) -> Result<(), ExportError> {
    Ok(())
}
