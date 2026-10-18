// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The project data model (`docs/specs/phase-1/design.md`, "Data model →
//! dc-project"). Every type here is persisted in a `.dcv` file, so each one
//! denies unknown fields and serializes `Option` as `null`
//! (`docs/rust-guidelines.md`, "Serialization"). `Option` fields are also
//! required on load (`deserialize_with = "Option::deserialize"` turns off
//! serde's implicit `None` for a missing field), so a schema change can never
//! slip in without a migration.
//!
//! Geometry is Y-up, Z-up, millimetres (ADR-0001). Stock occupies
//! `[0, width] × [0, height] × [0, thickness]` in project space.

use crate::ids::{DefinitionId, InstanceId, OperationId, ToolId};
use crate::post_processor::PostProcessor;
use dc_geometry::{Polyline, Transform};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A DreamCarve project: stock, features, tools, operations and machine
/// setup. The root of a `.dcv` file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Project {
    /// Schema version of the file this project was read from or will be
    /// written as. [`crate::load`] migrates it to [`crate::CURRENT_VERSION`].
    pub version: u32,
    /// Project name, used as the G-code program name.
    pub name: String,
    /// Units the CLI shows and `init` uses. Internal values are always
    /// millimetres.
    pub display_units: Units,
    /// Maximum deviation, millimetres, when curves are flattened to
    /// polylines. Default [`dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM`].
    pub flatten_tolerance_mm: f64,
    /// The material block.
    pub stock: Stock,
    /// Where the machine's work zero sits relative to the stock.
    pub origin: MachineOrigin,
    /// Canonical shapes, keyed by id.
    pub definitions: BTreeMap<DefinitionId, FeatureDefinition>,
    /// Placed copies of definitions.
    pub instances: Vec<FeatureInstance>,
    /// Tools embedded in the project (ADR-0002).
    pub tools: Vec<Tool>,
    /// Machining operations, in cutting order.
    pub operations: Vec<Operation>,
    /// Embedded copy of the post-processor definition (ADR-0002).
    pub post_processor: PostProcessor,
}

/// Display units for the CLI and new projects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Units {
    /// Millimetres.
    Mm,
    /// Inches.
    Inch,
}

/// The stock block, millimetres. It occupies
/// `[0, width_mm] × [0, height_mm] × [0, thickness_mm]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stock {
    /// Extent along X, millimetres.
    pub width_mm: f64,
    /// Extent along Y, millimetres.
    pub height_mm: f64,
    /// Extent along Z, millimetres. The material bottom is Z = 0. `None`
    /// until the user measures the material; like a feed, it is never
    /// defaulted, and validation reports it as an error until set (#64).
    #[serde(deserialize_with = "Option::deserialize")]
    pub thickness_mm: Option<f64>,
}

/// Where the machine's work zero sits relative to the stock.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineOrigin {
    /// XY zero.
    pub xy: OriginXy,
    /// Z zero.
    pub z: OriginZ,
}

/// XY work zero, relative to the stock footprint (Y-up). Saved as a plain
/// string, except `Custom`, which is `{ "custom": { "x_mm": …, "y_mm": … } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OriginXy {
    /// The stock corner at (0, 0).
    BottomLeft,
    /// The stock corner at (width, 0).
    BottomRight,
    /// The stock corner at (0, height).
    TopLeft,
    /// The stock corner at (width, height).
    TopRight,
    /// The centre of the stock footprint.
    Centre,
    /// An arbitrary point in project space.
    Custom {
        /// X, millimetres.
        x_mm: f64,
        /// Y, millimetres.
        y_mm: f64,
    },
}

/// Z work zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginZ {
    /// Top face of the stock (Z = thickness in project space).
    StockTop,
    /// Bottom face of the stock (Z = 0 in project space).
    StockBottom,
}

/// A canonical, untransformed shape shared by one or more instances.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureDefinition {
    /// This definition's id; equals its key in [`Project::definitions`].
    pub id: DefinitionId,
    /// Display name.
    pub name: String,
    /// Outline polylines in definition space, millimetres, Y-up.
    pub profiles: Vec<Polyline>,
}

/// A placed copy of a feature definition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeatureInstance {
    /// This instance's id.
    pub id: InstanceId,
    /// The definition this instance places.
    pub definition: DefinitionId,
    /// Maps definition space to project space.
    pub transform: Transform,
    /// Display name.
    pub name: String,
    /// Shown in previews.
    pub visible: bool,
    /// Protected from edits.
    pub locked: bool,
    /// Top of the feature, millimetres above the material bottom.
    pub z_top_mm: f64,
    /// Bottom of the feature, millimetres above the material bottom.
    pub z_bottom_mm: f64,
}

/// A cutting tool embedded in the project. Feeds, plunge and RPM stay `None`
/// until the user supplies them; export fails without them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tool {
    /// This tool's id.
    pub id: ToolId,
    /// Display name.
    pub name: String,
    /// Cutter geometry. Named `cutter`, not `kind`, so the file does not
    /// read `"kind": {"kind": ...}` (the variant tag is `kind`).
    pub cutter: ToolKind,
    /// Cutting diameter, millimetres.
    pub diameter_mm: f64,
    /// Number of flutes.
    pub flutes: u8,
    /// Cutting feed rate, millimetres per minute.
    #[serde(deserialize_with = "Option::deserialize")]
    pub feed_mm_min: Option<f64>,
    /// Plunge feed rate, millimetres per minute.
    #[serde(deserialize_with = "Option::deserialize")]
    pub plunge_mm_min: Option<f64>,
    /// Spindle speed, revolutions per minute.
    #[serde(deserialize_with = "Option::deserialize")]
    pub rpm: Option<u32>,
    /// Default depth per pass, millimetres.
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_down_mm: Option<f64>,
    /// Default step-over as a fraction of the diameter, in (0, 1].
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_over_fraction: Option<f64>,
}

/// Cutter geometry. Saved as a plain string, except `VBit`, which is
/// `{ "v_bit": { "included_angle_deg": … } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ToolKind {
    /// Flat end mill.
    EndMill,
    /// Ball-nose end mill.
    BallNose,
    /// V-bit with the given included angle.
    VBit {
        /// Included angle of the V, degrees.
        included_angle_deg: f64,
    },
    /// Engraving bit.
    Engraver,
}

/// A machining operation. Phase 1 has only Profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    /// Cut along closed outlines.
    Profile(ProfileOp),
}

impl Operation {
    /// This operation's id.
    pub fn id(&self) -> &OperationId {
        match self {
            Self::Profile(op) => &op.id,
        }
    }

    /// This operation's display name.
    pub fn name(&self) -> &str {
        match self {
            Self::Profile(op) => &op.name,
        }
    }

    /// Whether the operation is included in export.
    pub fn enabled(&self) -> bool {
        match self {
            Self::Profile(op) => op.enabled,
        }
    }

    /// The tool the operation cuts with.
    pub fn tool(&self) -> &ToolId {
        match self {
            Self::Profile(op) => &op.tool,
        }
    }

    /// The instances the operation machines.
    pub fn targets(&self) -> &[InstanceId] {
        match self {
            Self::Profile(op) => &op.targets,
        }
    }
}

/// A profile operation: cuts inside, outside or on closed outlines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileOp {
    /// This operation's id.
    pub id: OperationId,
    /// Display name.
    pub name: String,
    /// Whether the operation is included in export.
    pub enabled: bool,
    /// Instances whose outlines are cut.
    pub targets: Vec<InstanceId>,
    /// The tool to cut with.
    pub tool: ToolId,
    /// Which side of the outline the cutter runs on.
    pub side: Side,
    /// Final depth below the stock top, millimetres. `None` until the user
    /// sets it; never defaulted, and an error in validation until set (#64).
    #[serde(deserialize_with = "Option::deserialize")]
    pub depth_mm: Option<f64>,
    /// Depth per pass, millimetres; overrides [`Tool::step_down_mm`].
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_down_mm: Option<f64>,
    /// Climb or conventional milling.
    pub direction: CutDirection,
    /// How far below the stock bottom the cut may go, millimetres. 0 forbids
    /// cutting below the material bottom.
    pub through_cut_allowance_mm: f64,
    /// Retract height above the stock top, millimetres.
    pub safe_z_mm: f64,
    /// Holding tabs, if any.
    #[serde(deserialize_with = "Option::deserialize")]
    pub tabs: Option<TabSpec>,
}

/// Which side of an outline a profile cuts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// Tool centre inside the outline.
    Inside,
    /// Tool centre outside the outline.
    Outside,
    /// Tool centre on the outline.
    OnLine,
}

/// Milling direction relative to the clockwise spindle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CutDirection {
    /// Climb milling.
    Climb,
    /// Conventional milling.
    Conventional,
}

/// Holding tabs left by a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabSpec {
    /// Where the tabs go along the outline.
    pub placement: TabPlacement,
    /// Tab length along the outline, millimetres.
    pub width_mm: f64,
    /// Tab top above the stock bottom, millimetres.
    pub height_mm: f64,
    /// Tab cross-section.
    pub shape: TabShape,
}

/// Where tabs go along the tool-centre contour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TabPlacement {
    /// `count` tabs evenly spaced.
    Count {
        /// Number of tabs.
        count: u8,
    },
    /// Tabs centred at the given distances along the contour.
    AtDistances {
        /// Distances along the contour, millimetres, taken modulo its length.
        distances_mm: Vec<f64>,
    },
}

/// Tab cross-section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabShape {
    /// Square-shouldered tab.
    Rect,
    /// Raised-cosine ramp.
    Smooth,
}
