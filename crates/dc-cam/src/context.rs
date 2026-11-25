// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/edge.ts
//   src/engine/toolpaths/pocket.ts
//   src/engine/toolpaths/drilling.ts
//   src/engine/toolpaths/carving.ts

//! What a generator settles about its operation before it cuts
//! (`docs/specs/phase-1/design.md`, "Algorithms → Profile contour", steps
//! 5 and 8): the tool, its rates, the safe Z, the step-down, the stock's
//! thickness and the depth, each with the error that refuses the
//! operation, and [`OpContext`], what sequencing and the on-stock check
//! then need of them.
//!
//! The checks were lifted out of the generators, which each ran them by
//! hand, so this file keeps their files' upstream attribution. Generators
//! differ in which checks they run and in what order, and that order is
//! behaviour: the first problem found is the one reported. So each check
//! is its own function, called by each generator where it has always run
//! it; there is no one function that runs them all. Nothing here depends
//! on Y-up or Y-down.

use crate::error::{CamError, FeedField};
use crate::stock::{stock_overhang, swept_bounds};
use crate::tabs::LapRates;
use crate::toolpath::Toolpath;
use dc_geometry::Rect;
use dc_project::{OperationId, Project, Tool, ToolId};

/// The tool `tool` names in `project`, for operation `op`.
///
/// # Errors
///
/// [`CamError::UnknownTool`] when the project has no such tool.
pub(crate) fn tool_of<'p>(
    project: &'p Project,
    op: &OperationId,
    tool: &ToolId,
) -> Result<&'p Tool, CamError> {
    project
        .tools
        .iter()
        .find(|t| &t.id == tool)
        .ok_or_else(|| CamError::UnknownTool {
            op: op.clone(),
            tool: tool.clone(),
        })
}

/// A rate the user set to a positive number, or `None`.
fn usable_rate(rate_mm_min: Option<f64>) -> Option<f64> {
    rate_mm_min.filter(|r| r.is_finite() && *r > 0.0)
}

/// The error for a rate `tool` does not have. No rate is ever invented.
fn missing_rate(tool: &Tool, field: FeedField) -> CamError {
    CamError::MissingFeed {
        tool: tool.name.clone(),
        tool_id: tool.id.clone(),
        field,
    }
}

/// The plunge rate of `tool`, mm/min, exactly as the user set it.
///
/// # Errors
///
/// [`CamError::MissingFeed`] when it is unset or not a positive number.
pub(crate) fn plunge_rate_of(tool: &Tool) -> Result<f64, CamError> {
    usable_rate(tool.plunge_mm_min).ok_or_else(|| missing_rate(tool, FeedField::Plunge))
}

/// The rates every cut of an operation uses, exactly as the user set
/// them on the tool.
///
/// # Errors
///
/// [`CamError::MissingFeed`], naming the feed before the plunge rate,
/// when one is unset or not a positive number.
pub(crate) fn rates_of(tool: &Tool) -> Result<LapRates, CamError> {
    Ok(LapRates {
        feed_mm_min: usable_rate(tool.feed_mm_min)
            .ok_or_else(|| missing_rate(tool, FeedField::Feed))?,
        plunge_mm_min: plunge_rate_of(tool)?,
    })
}

/// Checks that the diameter of `tool` is a positive number.
///
/// # Errors
///
/// [`CamError::InvalidToolDiameter`] otherwise.
pub(crate) fn check_diameter(tool: &Tool) -> Result<(), CamError> {
    if tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0 {
        Ok(())
    } else {
        Err(CamError::InvalidToolDiameter {
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
            diameter_mm: tool.diameter_mm,
        })
    }
}

/// Checks that `safe_z_mm`, the safe Z of operation `op` above the stock
/// top, is a positive number.
///
/// # Errors
///
/// [`CamError::InvalidSafeZ`] otherwise.
pub(crate) fn check_safe_z(op: &OperationId, safe_z_mm: f64) -> Result<(), CamError> {
    if safe_z_mm.is_finite() && safe_z_mm > 0.0 {
        Ok(())
    } else {
        Err(CamError::InvalidSafeZ {
            op: op.clone(),
            safe_z_mm,
        })
    }
}

/// The step-down of operation `op`, mm: its own, `set_mm`, or else the
/// one of `tool`.
///
/// # Errors
///
/// [`CamError::MissingStepDown`] when neither is set.
pub(crate) fn step_down_mm(
    op: &OperationId,
    set_mm: Option<f64>,
    tool: &Tool,
) -> Result<f64, CamError> {
    set_mm
        .or(tool.step_down_mm)
        .ok_or_else(|| CamError::MissingStepDown {
            op: op.clone(),
            tool: tool.name.clone(),
            tool_id: tool.id.clone(),
        })
}

/// The thickness of the stock of `project`, mm.
///
/// # Errors
///
/// [`CamError::MissingThickness`] while it is unset.
pub(crate) fn thickness_mm(project: &Project) -> Result<f64, CamError> {
    project.stock.thickness_mm.ok_or(CamError::MissingThickness)
}

/// The depth of operation `op` below the stock top, mm, from its setting
/// `set_mm`.
///
/// # Errors
///
/// [`CamError::MissingDepth`] while it is unset.
pub(crate) fn depth_mm(op: &OperationId, set_mm: Option<f64>) -> Result<f64, CamError> {
    set_mm.ok_or_else(|| CamError::MissingDepth { op: op.clone() })
}

/// The depth operation `op` cuts to below the stock top, mm: its setting
/// `set_mm`, or, when that reaches the bottom of stock `thickness_mm`
/// thick, the through cut's, `allowance_mm` below the bottom
/// ([`dc_project::through_cut_depth_mm`]).
///
/// # Errors
///
/// [`CamError::MissingDepth`] while it is unset.
pub(crate) fn cut_depth_mm(
    op: &OperationId,
    set_mm: Option<f64>,
    thickness_mm: f64,
    allowance_mm: f64,
) -> Result<f64, CamError> {
    Ok(dc_project::through_cut_depth_mm(
        depth_mm(op, set_mm)?,
        thickness_mm,
        allowance_mm,
    ))
}

/// An operation whose checks have passed, as sequencing and the on-stock
/// check read it. Z values are absolute project Z, mm (stock bottom at 0).
#[derive(Debug, Clone, Copy)]
pub(crate) struct OpContext<'a> {
    /// The project the operation is in.
    pub project: &'a Project,
    /// The operation, for errors.
    pub op: &'a OperationId,
    /// Its tool.
    pub tool: &'a Tool,
    /// The tool's rates.
    pub rates: LapRates,
    /// The stock's thickness, mm: the Z of the stock top.
    pub thickness_mm: f64,
    /// Safe Z, mm: the stock top plus the operation's safe Z.
    pub safe_z_mm: f64,
}

impl<'a> OpContext<'a> {
    /// The context of `op` in `project` with `tool` at `rates`, on stock
    /// `thickness_mm` thick, its safe Z `safe_z_above_mm` above the stock
    /// top. It checks nothing: the caller has run its checks, in its order.
    pub(crate) fn new(
        project: &'a Project,
        op: &'a OperationId,
        tool: &'a Tool,
        rates: LapRates,
        thickness_mm: f64,
        safe_z_above_mm: f64,
    ) -> Self {
        Self {
            project,
            op,
            tool,
            rates,
            thickness_mm,
            safe_z_mm: thickness_mm + safe_z_above_mm,
        }
    }

    /// Checks that `toolpath`, cut by a tool of radius `radius_mm`, stays
    /// on the stock: the XY bounds of its cutting moves grown by the
    /// radius ([`swept_bounds`]).
    ///
    /// # Errors
    ///
    /// [`CamError::OffStock`] with the overhang.
    pub(crate) fn check_on_stock(
        &self,
        toolpath: &Toolpath,
        radius_mm: f64,
    ) -> Result<(), CamError> {
        self.check_swept_on_stock(swept_bounds(toolpath, radius_mm))
    }

    /// Checks that `swept`, the XY extent an operation's cutter sweeps,
    /// stays on the stock; `None`, an operation with no cutting move,
    /// does.
    ///
    /// # Errors
    ///
    /// [`CamError::OffStock`] with the overhang.
    pub(crate) fn check_swept_on_stock(&self, swept: Option<Rect>) -> Result<(), CamError> {
        match swept.and_then(|swept| stock_overhang(&self.project.stock, swept)) {
            Some(overhang) => Err(CamError::OffStock {
                op: self.op.clone(),
                overhang,
            }),
            None => Ok(()),
        }
    }
}
