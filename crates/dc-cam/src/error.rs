// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Errors from toolpath generation (`docs/specs/phase-1/design.md`,
//! "Algorithms → Profile contour"). An error means the toolpath cannot be
//! trusted, so none is produced.

use dc_geometry::GeometryError;
use dc_project::{DefinitionId, InstanceId, OperationId, ToolId};
use std::fmt;

/// Which rate a tool is missing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedField {
    /// `feed_mm_min`, the cutting feed.
    Feed,
    /// `plunge_mm_min`, the descent feed.
    Plunge,
}

impl fmt::Display for FeedField {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Feed => "feed rate (feed_mm_min)",
            Self::Plunge => "plunge rate (plunge_mm_min)",
        })
    }
}

/// One target shape: profile `profile` (0-based, in the definition's
/// order) of the feature instance `instance`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShapeRef {
    /// The instance the shape belongs to.
    pub instance: InstanceId,
    /// The instance's name, as the user sees it.
    pub instance_name: String,
    /// Index of the profile within the instance's definition.
    pub profile: usize,
}

impl fmt::Display for ShapeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "`{}` (instance `{}`, profile {})",
            self.instance_name, self.instance, self.profile
        )
    }
}

/// Why a toolpath could not be generated.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum CamError {
    /// The operation names a tool the project does not contain.
    #[error("operation `{op}` uses tool `{tool}`, which is not in the project")]
    UnknownTool {
        /// The operation.
        op: OperationId,
        /// The missing tool id.
        tool: ToolId,
    },
    /// The operation targets an instance the project does not contain.
    #[error("operation `{op}` targets instance `{instance}`, which is not in the project")]
    UnknownInstance {
        /// The operation.
        op: OperationId,
        /// The missing instance id.
        instance: InstanceId,
    },
    /// An instance refers to a definition the project does not contain.
    #[error("instance `{instance}` uses definition `{definition}`, which is not in the project")]
    UnknownDefinition {
        /// The instance.
        instance: InstanceId,
        /// The missing definition id.
        definition: DefinitionId,
    },
    /// The tool lacks a rate the cut needs: unset, or not a positive
    /// number. Rates are never invented.
    #[error("tool `{tool}` has no usable {field}")]
    MissingFeed {
        /// The tool's name.
        tool: String,
        /// Which rate is missing.
        field: FeedField,
    },
    /// Neither the operation nor its tool sets a step-down.
    #[error("operation `{op}` has no step-down and tool `{tool}` sets none")]
    MissingStepDown {
        /// The operation.
        op: OperationId,
        /// The tool's name.
        tool: String,
    },
    /// The step-down in effect is not a positive number, or is so small
    /// that the depth would need more than
    /// [`crate::passes::MAX_PASSES`] passes.
    #[error(
        "operation `{op}` has step-down {step_down_mm} mm; it must be more than 0 and give at most {max} passes",
        max = crate::passes::MAX_PASSES
    )]
    InvalidStepDown {
        /// The operation.
        op: OperationId,
        /// The step-down in effect, mm.
        step_down_mm: f64,
    },
    /// The stock thickness is unset. Validation reports this first; it is
    /// never defaulted (#64).
    #[error("the stock has no thickness_mm; export fails until it is set")]
    MissingThickness,
    /// The operation's depth is unset. Validation reports this first; it is
    /// never defaulted (#64).
    #[error("operation `{op}` has no depth_mm; export fails until it is set")]
    MissingDepth {
        /// The operation.
        op: OperationId,
    },
    /// The cut depth is not a positive number.
    #[error("operation `{op}` has depth {depth_mm} mm; it must be more than 0")]
    InvalidDepth {
        /// The operation.
        op: OperationId,
        /// The depth from stock top, mm.
        depth_mm: f64,
    },
    /// Safe Z is not above the stock top.
    #[error("operation `{op}` has safe Z {safe_z_mm} mm above the stock; it must be more than 0")]
    InvalidSafeZ {
        /// The operation.
        op: OperationId,
        /// Safe Z above the stock top, mm.
        safe_z_mm: f64,
    },
    /// The tool's diameter is not a positive number.
    #[error("tool `{tool}` has diameter {diameter_mm} mm; it must be more than 0")]
    InvalidToolDiameter {
        /// The tool's name.
        tool: String,
        /// The diameter, mm.
        diameter_mm: f64,
    },
    /// The cut would go deeper than the stock plus the operation's
    /// through-cut allowance.
    #[error(
        "operation `{op}` cuts {depth_mm} mm deep, but the stock and through-cut allowance permit {limit_mm} mm"
    )]
    BelowMaterialBottom {
        /// The operation.
        op: OperationId,
        /// Requested depth from stock top, mm.
        depth_mm: f64,
        /// Stock thickness plus through-cut allowance, mm.
        limit_mm: f64,
    },
    /// Two target shapes cross: they overlap without one containing the
    /// other, so neither part-and-hole nor separate parts describes them.
    /// This is `GeometryError::ContoursCross` with its input indices mapped
    /// back to the shapes the user drew.
    #[error(
        "shapes {first} and {second} cross each other; separate them or merge them into one shape"
    )]
    TargetsCross {
        /// The earlier shape in target order.
        first: ShapeRef,
        /// The later shape in target order.
        second: ShapeRef,
    },
    /// A target shape cannot be used: it encloses no area, crosses itself, or its
    /// instance transform collapses it or distorts its arcs.
    #[error("shape {shape} cannot be cut: {source}")]
    InvalidTarget {
        /// The shape.
        shape: ShapeRef,
        /// What is wrong with it.
        source: GeometryError,
    },
    /// The operation's tab settings cannot be cut: a width or height that
    /// is not a positive number, or a distance that is not finite.
    #[error("operation `{op}` has tab {field} {value_mm} mm; it must be a positive number")]
    InvalidTab {
        /// The operation.
        op: OperationId,
        /// Which setting: `width_mm`, `height_mm`, or `distances_mm[i]`.
        field: String,
        /// The value, mm.
        value_mm: f64,
    },
    /// Any other geometry failure.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}
