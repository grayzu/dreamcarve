// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The project data model (`docs/specs/phase-1/design.md`, "Data model →
//! dc-project"; `docs/specs/phase-2/design.md`, "Data model → dc-project"
//! for the Pocket, Drill and Engrave operations, entries, leads, corner
//! relief and tool numbers). Every type here is persisted in a `.dcv` file, so each one
//! denies unknown fields and serializes `Option` as `null`
//! (`docs/rust-guidelines.md`, "Serialization"). `Option` fields are also
//! required on load (`deserialize_with = "Option::deserialize"` turns off
//! serde's implicit `None` for a missing field), so a schema change can never
//! slip in without a migration.
//!
//! Every length (`*_mm`) and feed rate (`*_mm_min`) reads through
//! [`crate::units_input`], so a file may write it with its unit (`"0.5
//! in"`, `"60 in/min"`; addendum A6). Those helpers keep a field required,
//! as `Option::deserialize` does. The model holds millimetres either way.
//!
//! Geometry is Y-up, Z-up, millimetres (ADR-0001). Stock occupies
//! `[0, width] × [0, height] × [0, thickness]` in project space.

use crate::ids::{DefinitionId, InstanceId, OperationId, TabId, ToolId};
use crate::post_processor::PostProcessorRef;
use crate::units_input;
use dc_geometry::{Polyline, Transform};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// A DreamCarve project: stock, features, tools, operations and machine
/// setup. The root of a `.dcv` file.
///
/// Fields are declared, and so saved, in the order a person edits them
/// (#76): the settings first, the geometry last. Readers accept any order.
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
    #[serde(deserialize_with = "units_input::mm")]
    pub flatten_tolerance_mm: f64,
    /// Fit arcs to straight runs at export within this deviation,
    /// millimetres (functional spec, capability 24). `None` turns fitting
    /// off. `init` writes 0.01. Read by the arc fitter once it lands
    /// (#117 lists the follow-ups).
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub arc_fit_tolerance_mm: Option<f64>,
    /// The material block.
    pub stock: Stock,
    /// Where the machine's work zero sits relative to the stock.
    pub origin: MachineOrigin,
    /// The clearance, tool-change and park heights (#283; functional
    /// spec, capability 33). Each is optional; unset, the program is as
    /// before format version 12.
    pub heights: Heights,
    /// Tools embedded in the project (ADR-0002).
    pub tools: Vec<Tool>,
    /// Machining operations, in cutting order.
    pub operations: Vec<Operation>,
    /// Holding tabs placed on the outlines of instances (#105, ADR-0005).
    /// Every enabled Profile that cuts a tab's outline rises over it.
    pub tabs: Vec<PlacedTab>,
    /// Placed copies of definitions.
    pub instances: Vec<FeatureInstance>,
    /// The bundled post-processor definition version and the user's
    /// settings for it (ADR-0004).
    pub post_processor: PostProcessorRef,
    /// Canonical shapes, keyed by id. Last in the file, since it is the
    /// bulk of it and rarely edited by hand.
    pub definitions: BTreeMap<DefinitionId, FeatureDefinition>,
}

/// Heights the program moves to between parts, at a tool change and at
/// the end (#283; `docs/specs/phase-2/design.md`, "Phase 2 addendum
/// (#73)", A7). After VCarve's Material Setup ("Rapid Z Gaps above
/// Material", "Home / Start Position") and Fusion's clearance and retract
/// heights. Every field is `None` until the user sets it, and a project
/// with all three unset exports what it did before they existed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Heights {
    /// Above the stock top, millimetres: the height of links between parts
    /// (feature instances) within an operation, between operations, at a
    /// tool change and at the end. Each operation's `safe_z_mm` stays the
    /// retract between passes of one part. At least every enabled
    /// operation's `safe_z_mm`.
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub clearance_mm: Option<f64>,
    /// Machine Z, millimetres (written in the program's units), that
    /// `G53 G0 Z…` lifts to before every tool-change stop and at the end
    /// of every program. Below 0, since grblHAL and GRBL home Z to a
    /// negative machine position, and only on a definition whose
    /// `work_coordinates.machine_command` is set. Needs a homed machine.
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub tool_change_machine_z_mm: Option<f64>,
    /// Where the program ends in XY, project coordinates: the bit rises to
    /// the clearance height, rapids here, then makes the machine-Z lift if
    /// set. VCarve's Home position without its Z.
    #[serde(deserialize_with = "Option::deserialize")]
    pub park: Option<ParkXy>,
}

impl Heights {
    /// All three unset, as `init` writes them and the version 12
    /// migration adds them.
    #[must_use]
    pub const fn unset() -> Self {
        Self {
            clearance_mm: None,
            tool_change_machine_z_mm: None,
            park: None,
        }
    }
}

/// A point in project space, millimetres (Y-up): where a program parks.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParkXy {
    /// X, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub x_mm: f64,
    /// Y, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub y_mm: f64,
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
    #[serde(deserialize_with = "units_input::mm")]
    pub width_mm: f64,
    /// Extent along Y, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub height_mm: f64,
    /// Extent along Z, millimetres. The material bottom is Z = 0. `None`
    /// until the user measures the material; like a feed, it is never
    /// defaulted, and validation reports it as an error until set (#64).
    #[serde(deserialize_with = "units_input::opt_mm")]
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
        #[serde(deserialize_with = "units_input::mm")]
        x_mm: f64,
        /// Y, millimetres.
        #[serde(deserialize_with = "units_input::mm")]
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
    /// Shown in previews. Reserved for the GUI (phase 4): saved and
    /// loaded, but nothing reads it yet, and export cuts a hidden instance
    /// like any other (#71).
    pub visible: bool,
    /// Protected from edits. Reserved for the GUI (phase 4), like
    /// `visible`.
    pub locked: bool,
    /// Top of the feature, millimetres above the material bottom. `None`
    /// until the user sets it; nothing reads it yet, so it is never
    /// guessed (#99). Reserved: validation checks only its range and that
    /// it is not below `z_bottom_mm` (#71).
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub z_top_mm: Option<f64>,
    /// Bottom of the feature, millimetres above the material bottom. `None`
    /// until the user sets it, like `z_top_mm`.
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub z_bottom_mm: Option<f64>,
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
    /// Tool number, the `<n>` of `T<n>` in programs and of `-T<n>` in
    /// file names (functional spec, capability 21a). `None` means the
    /// tool's 1-based position in [`Project::tools`]. Never 0; validation
    /// rejects two used tools that resolve to the same number.
    #[serde(deserialize_with = "Option::deserialize")]
    pub number: Option<u32>,
    /// Cutter geometry. Named `cutter`, not `kind`, so the file does not
    /// read `"kind": {"kind": ...}` (the variant tag is `kind`).
    pub cutter: ToolKind,
    /// Cutting diameter, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub diameter_mm: f64,
    /// Number of flutes.
    pub flutes: u8,
    /// Cutting feed rate, millimetres per minute.
    #[serde(deserialize_with = "units_input::opt_mm_min")]
    pub feed_mm_min: Option<f64>,
    /// Plunge feed rate, millimetres per minute.
    #[serde(deserialize_with = "units_input::opt_mm_min")]
    pub plunge_mm_min: Option<f64>,
    /// Spindle speed, revolutions per minute.
    #[serde(deserialize_with = "Option::deserialize")]
    pub rpm: Option<u32>,
    /// Default depth per pass, millimetres.
    #[serde(deserialize_with = "units_input::opt_mm")]
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
    /// Twist drill. Phase 2 drills with any cutter; the kind drives
    /// warnings and Fusion import only.
    Drill,
}

impl ToolKind {
    /// Whether the cutter cuts its full diameter at every depth, as a flat
    /// end mill does. Profile and Pocket offset the path by half the
    /// diameter, which is right only for such a cutter (#67). An engraving
    /// bit usually narrows to a point, and its kind carries no shape, so it
    /// is not; nor is a drill, whose tip is a cone.
    #[must_use]
    pub const fn is_flat(&self) -> bool {
        matches!(self, Self::EndMill)
    }

    /// The cutter in words, with its article, as messages name it.
    #[must_use]
    pub const fn describe(&self) -> &'static str {
        match self {
            Self::EndMill => "an end mill",
            Self::BallNose => "a ball-nose bit",
            Self::VBit { .. } => "a V-bit",
            Self::Engraver => "an engraving bit",
            Self::Drill => "a drill",
        }
    }
}

/// A machining operation: Profile, Pocket, Drill or Engrave. Tagged by
/// `kind` in the file, so each one reads as a flat object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Operation {
    /// Cut along closed outlines.
    Profile(ProfileOp),
    /// Clear the inside of closed outlines.
    Pocket(PocketOp),
    /// Plunge at the centre of circular outlines.
    Drill(DrillOp),
    /// Trace open or closed paths with the tool centre on the line.
    Engrave(EngraveOp),
}

impl Operation {
    /// This operation's id.
    pub fn id(&self) -> &OperationId {
        match self {
            Self::Profile(op) => &op.id,
            Self::Pocket(op) => &op.id,
            Self::Drill(op) => &op.id,
            Self::Engrave(op) => &op.id,
        }
    }

    /// This operation's display name.
    pub fn name(&self) -> &str {
        match self {
            Self::Profile(op) => &op.name,
            Self::Pocket(op) => &op.name,
            Self::Drill(op) => &op.name,
            Self::Engrave(op) => &op.name,
        }
    }

    /// Whether the operation is included in export.
    pub fn enabled(&self) -> bool {
        match self {
            Self::Profile(op) => op.enabled,
            Self::Pocket(op) => op.enabled,
            Self::Drill(op) => op.enabled,
            Self::Engrave(op) => op.enabled,
        }
    }

    /// The tool the operation cuts with.
    pub fn tool(&self) -> &ToolId {
        match self {
            Self::Profile(op) => &op.tool,
            Self::Pocket(op) => &op.tool,
            Self::Drill(op) => &op.tool,
            Self::Engrave(op) => &op.tool,
        }
    }

    /// The instances the operation machines.
    pub fn targets(&self) -> &[InstanceId] {
        match self {
            Self::Profile(op) => &op.targets,
            Self::Pocket(op) => &op.targets,
            Self::Drill(op) => &op.targets,
            Self::Engrave(op) => &op.targets,
        }
    }

    /// Retract height above the stock top, millimetres.
    pub fn safe_z_mm(&self) -> f64 {
        match self {
            Self::Profile(op) => op.safe_z_mm,
            Self::Pocket(op) => op.safe_z_mm,
            Self::Drill(op) => op.safe_z_mm,
            Self::Engrave(op) => op.safe_z_mm,
        }
    }

    /// Final depth below the stock top, millimetres; `None` until set.
    pub fn depth_mm(&self) -> Option<f64> {
        match self {
            Self::Profile(op) => op.depth_mm,
            Self::Pocket(op) => op.depth_mm,
            Self::Drill(op) => op.depth_mm,
            Self::Engrave(op) => op.depth_mm,
        }
    }

    /// How far below the stock bottom the cut may go, millimetres (S1).
    pub fn through_cut_allowance_mm(&self) -> f64 {
        match self {
            Self::Profile(op) => op.through_cut_allowance_mm,
            Self::Pocket(op) => op.through_cut_allowance_mm,
            Self::Drill(op) => op.through_cut_allowance_mm,
            Self::Engrave(op) => op.through_cut_allowance_mm,
        }
    }

    /// The operation's kind as the file tags it: `profile`, `pocket`,
    /// `drill` or `engrave`.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Profile(_) => "profile",
            Self::Pocket(_) => "pocket",
            Self::Drill(_) => "drill",
            Self::Engrave(_) => "engrave",
        }
    }
}

/// The order an operation visits its shapes and passes (functional spec,
/// capability 11). Changes the order of cuts, never which cuts are made.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MachiningOrder {
    /// Each shape is cut to its full depth before the next.
    FullDepthFirst,
    /// Each pass is cut across every shape before the next, deeper pass.
    PassFirst,
}

/// How the tool reaches each pass depth (functional spec, capability 16).
/// Saved as `"plunge"`, or as a one-key object for the variants with
/// values: `{ "ramp": { "angle_deg": 15.0 } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Entry {
    /// Straight down at the plunge rate.
    Plunge,
    /// A zigzag ramp in removed material (Pocket, Inside and Outside
    /// Profile) or along the cut path (On line Profile).
    Ramp {
        /// Descent angle from the horizontal, degrees, in (0, 45].
        angle_deg: f64,
    },
    /// A helix in removed material. Pocket and Inside or Outside Profile
    /// only: an On line Profile removes nothing but its kerf.
    Helix {
        /// Helix diameter as a fraction of the tool diameter, in (0, 1].
        diameter_fraction: f64,
        /// Descent angle from the horizontal, degrees, in (0, 45].
        angle_deg: f64,
    },
}

/// A lead-in arc tangent to the cut, and optionally the matching lead-out
/// (functional spec, capability 17).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lead {
    /// Arc radius, millimetres, greater than 0.
    #[serde(deserialize_with = "units_input::mm")]
    pub radius_mm: f64,
    /// Arc sweep, degrees, in (0, 180]. `init` writes 90.
    pub sweep_deg: f64,
    /// Add the matching exit arc.
    pub lead_out: bool,
    /// Distance to continue past the start point before leading out,
    /// millimetres, 0 or more.
    #[serde(deserialize_with = "units_input::mm")]
    pub overcut_mm: f64,
}

/// Corner relief for internal corners a round tool cannot reach
/// (functional spec, capability 18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CornerRelief {
    /// No relief.
    None,
    /// A notch along the corner's bisector.
    Dogbone,
    /// A notch along the corner's longer edge.
    TBone,
}

/// A profile operation: cuts inside, outside or on closed outlines, along
/// open paths, or to the left or right of open paths.
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
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub depth_mm: Option<f64>,
    /// Depth per pass, millimetres; overrides [`Tool::step_down_mm`].
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub step_down_mm: Option<f64>,
    /// Climb or conventional milling.
    pub direction: CutDirection,
    /// How far below the stock bottom the cut may go, millimetres. 0 forbids
    /// cutting below the material bottom.
    #[serde(deserialize_with = "units_input::mm")]
    pub through_cut_allowance_mm: f64,
    /// Retract height above the stock top, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub safe_z_mm: f64,
    /// A tab request: tabs still to be placed on this operation's
    /// targets, as a format 10 file set them (#105, ADR-0005). Placed
    /// tabs are [`Project::tabs`]; a request is not cut data. Generation
    /// places a request before it cuts, as `dreamcarve tabs place` does,
    /// so an older file cuts the same; every `tabs` command that saves the
    /// project places the requests of its enabled Profiles and sets them
    /// to `null`. Kept for format 11, so older files cut the same.
    #[serde(deserialize_with = "Option::deserialize")]
    pub tabs: Option<TabSpec>,
    /// The order shapes and passes are visited. Migration: full depth
    /// first, as phase 1 cut.
    pub machining_order: MachiningOrder,
    /// How each pass depth is reached. Migration: plunge, as phase 1 cut.
    pub entry: Entry,
    /// Lead-in and lead-out arcs, if any. Migration: none.
    #[serde(deserialize_with = "Option::deserialize")]
    pub lead: Option<Lead>,
    /// Relief of the corners the tool cannot reach: the hole's corners on
    /// an Inside cut, the part's concave corners on an Outside cut.
    /// Migration: none.
    pub corner_relief: CornerRelief,
    /// A finishing pass (#284): every pass but the last leaves
    /// [`LastPass::allowance_mm`] on the wall and the last cuts it to
    /// size. Inside and Outside only. `init` and migration: none.
    #[serde(deserialize_with = "Option::deserialize")]
    pub last_pass: Option<LastPass>,
    /// Where each closed outline's cut starts (#285, functional spec
    /// capability 36). `init` and migration: [`StartAt::Auto`], as
    /// before.
    pub start: StartAt,
}

/// Where a Profile's cut starts on each closed outline (#285; VCarve's 2D
/// Profile "Use vector start point" and "Start At", Fusion's "Entry
/// Positions"). Saved as `"auto"`, `"first_vertex"`, or `{ "near": {
/// "x_mm": 0.0, "y_mm": 0.0 } }`. With an explicit start the tabs a
/// request places are measured from it, a lead arrives there or is
/// dropped, and an entry with no room there falls back instead of moving.
/// Open paths ignore it: their start is the end the cut direction sets
/// (#242).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum StartAt {
    /// As before: the start follows from the drawing's first vertex, and
    /// a lead, or an entry with no room beside a tab, may move it.
    Auto,
    /// At the point of the bit's path nearest the outline's first vertex
    /// as drawn, as VCarve's "Use vector start point" plunges at the
    /// shape's first point.
    FirstVertex,
    /// At the point of the bit's path nearest this project point, Y-up.
    Near {
        /// X, millimetres.
        #[serde(deserialize_with = "units_input::mm")]
        x_mm: f64,
        /// Y, millimetres.
        #[serde(deserialize_with = "units_input::mm")]
        y_mm: f64,
    },
}

/// A Profile's last pass allowance (#284, functional spec capability 34;
/// VCarve's 2D Profile "Last Pass Allowance", Fusion's 2D Contour stock to
/// leave and finishing pass): every pass but the last runs `allowance_mm`
/// farther from the part, and the last pass cuts on the part's path, so
/// it takes an even skin off the whole wall and leaves no step marks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LastPass {
    /// Material every pass but the last leaves on the wall, millimetres:
    /// more than 0 and at most a third of the bit's diameter.
    #[serde(deserialize_with = "units_input::mm")]
    pub allowance_mm: f64,
    /// How deep the last pass cuts, millimetres, more than 0 and at most
    /// `depth_mm` (VCarve's "Set Last Pass Thickness"): the other passes
    /// step down to `depth_mm − thickness_mm`. `None`: the last pass is the
    /// last of the usual depth passes.
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub thickness_mm: Option<f64>,
    /// Cut the last pass the other way round (VCarve's "Reverse
    /// direction"): conventional after climbing passes, and the reverse.
    pub reverse: bool,
}

/// How a Pocket clears its region (functional spec, capability 12). Saved
/// as `"offset"`, or `{ "raster": { "angle_deg": 45.0 } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ClearanceStrategy {
    /// Concentric passes stepping inward from the wall.
    Offset,
    /// Parallel straight passes.
    Raster {
        /// Angle of the passes from the X axis, degrees, in [0, 180).
        angle_deg: f64,
    },
}

/// Whether and when a Pocket runs a pass along its wall and islands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WallPass {
    /// After the clearing passes. `init` writes this.
    Last,
    /// Before the clearing passes.
    First,
    /// No wall pass: the wall keeps the clearing passes' scallops.
    None,
}

/// A pocket operation: clears the inside of closed outlines, leaving
/// shapes nested inside them as islands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PocketOp {
    /// This operation's id.
    pub id: OperationId,
    /// Display name.
    pub name: String,
    /// Whether the operation is included in export.
    pub enabled: bool,
    /// Instances whose insides are cleared.
    pub targets: Vec<InstanceId>,
    /// The tool to cut with. Offsets by half its diameter, so it must be
    /// a flat end mill.
    pub tool: ToolId,
    /// Offset rings or raster lines.
    pub strategy: ClearanceStrategy,
    /// Whether and when the wall pass runs.
    pub wall_pass: WallPass,
    /// Final depth below the stock top, millimetres. `None` until set.
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub depth_mm: Option<f64>,
    /// Depth per pass, millimetres; overrides [`Tool::step_down_mm`].
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub step_down_mm: Option<f64>,
    /// Step-over as a fraction of the tool diameter, in (0, 1]; overrides
    /// [`Tool::step_over_fraction`].
    #[serde(deserialize_with = "Option::deserialize")]
    pub step_over_fraction: Option<f64>,
    /// Climb or conventional milling, on every ring and the wall pass.
    pub direction: CutDirection,
    /// Material to leave on the walls, millimetres, 0 or more.
    #[serde(deserialize_with = "units_input::mm")]
    pub allowance_mm: f64,
    /// An earlier enabled Pocket with the same targets whose tool could
    /// not reach everything: this one clears only what it left
    /// (functional spec, capability 13).
    #[serde(deserialize_with = "Option::deserialize")]
    pub rest_of: Option<OperationId>,
    /// How each pass depth is reached.
    pub entry: Entry,
    /// Relief of the pocket's corners and the islands' concave corners.
    pub corner_relief: CornerRelief,
    /// The order shapes and passes are visited.
    pub machining_order: MachiningOrder,
    /// How far below the stock bottom the cut may go, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub through_cut_allowance_mm: f64,
    /// Retract height above the stock top, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub safe_z_mm: f64,
}

/// How a peck drill retracts between pecks. Saved as `"full"`, or
/// `{ "chip_break": { "gap_mm": 0.5 } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum PeckRetract {
    /// To the retract height above the hole top.
    Full,
    /// By a short gap, enough to break the chip.
    ChipBreak {
        /// How far to lift, millimetres, greater than 0.
        #[serde(deserialize_with = "units_input::mm")]
        gap_mm: f64,
    },
}

/// A drill cycle (functional spec, capability 14). Saved as `"simple"`,
/// or `{ "peck": { "peck_depth_mm": 2.0, "retract": "full" } }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum DrillCycle {
    /// One plunge at the plunge rate, then a retract.
    Simple,
    /// Plunges of a peck depth, retracting between them.
    Peck {
        /// Depth of each peck, millimetres, greater than 0. `None` takes
        /// the tool's step-down, which must then be set.
        #[serde(deserialize_with = "units_input::opt_mm")]
        peck_depth_mm: Option<f64>,
        /// How far to retract between pecks.
        retract: PeckRetract,
    },
}

/// A drill operation: plunges at the centre of each circular target.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrillOp {
    /// This operation's id.
    pub id: OperationId,
    /// Display name.
    pub name: String,
    /// Whether the operation is included in export.
    pub enabled: bool,
    /// Instances to drill at the centre of. A target that is not a circle
    /// is skipped with a warning at export.
    pub targets: Vec<InstanceId>,
    /// The tool to drill with. Any cutter; a drill or end mill is usual.
    pub tool: ToolId,
    /// Simple or peck.
    pub cycle: DrillCycle,
    /// Seconds to wait at the bottom of every plunge, 0 or more; 0 is no
    /// dwell. Combinable with any cycle.
    pub dwell_s: f64,
    /// Final depth below the stock top, millimetres. `None` until set.
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub depth_mm: Option<f64>,
    /// Height above the hole top to return to between pecks and between
    /// holes, millimetres, 0 or more. `init` writes 1.
    #[serde(deserialize_with = "units_input::mm")]
    pub retract_mm: f64,
    /// How far below the stock bottom the cut may go, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub through_cut_allowance_mm: f64,
    /// Retract height above the stock top, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub safe_z_mm: f64,
}

/// An engrave operation: follows each target path, open or closed, with
/// the tool centre on the line (functional spec, capability 15).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngraveOp {
    /// This operation's id.
    pub id: OperationId,
    /// Display name.
    pub name: String,
    /// Whether the operation is included in export.
    pub enabled: bool,
    /// Instances whose paths are traced.
    pub targets: Vec<InstanceId>,
    /// The tool to cut with. Any cutter.
    pub tool: ToolId,
    /// Final depth below the stock top, millimetres. `None` until set.
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub depth_mm: Option<f64>,
    /// Depth per pass, millimetres; overrides [`Tool::step_down_mm`].
    #[serde(deserialize_with = "units_input::opt_mm")]
    pub step_down_mm: Option<f64>,
    /// Cut successive passes of an open path in opposite directions,
    /// saving a retract per pass.
    pub alternate_direction: bool,
    /// The order shapes and passes are visited.
    pub machining_order: MachiningOrder,
    /// How far below the stock bottom the cut may go, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub through_cut_allowance_mm: f64,
    /// Retract height above the stock top, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub safe_z_mm: f64,
}

/// Which side of an outline a profile cuts on. Inside, Outside and On
/// line cut closed outlines; On line also cuts open paths along them, and
/// Left and Right cut open paths beside them (#207, as VCarve's Profile of
/// an open vector), skipping closed outlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    /// Tool centre inside the outline.
    Inside,
    /// Tool centre outside the outline.
    Outside,
    /// Tool centre on the outline.
    OnLine,
    /// Tool centre one tool radius to the left of an open path, looking
    /// along it from its first vertex to its last (in Y-up, the normal
    /// `(−dy, dx)` of a segment's direction `(dx, dy)`). Saved as
    /// `"left"`; from format version 10.
    Left,
    /// Tool centre one tool radius to the right of an open path: Left
    /// mirrored. Saved as `"right"`; from format version 10.
    Right,
}

impl Side {
    /// True for Left and Right, the sides of an open path: they cut open
    /// paths only and skip closed outlines.
    #[must_use]
    pub const fn is_open_path_side(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    /// True for the sides that cut open paths: On line, Left and Right.
    #[must_use]
    pub const fn cuts_open_paths(self) -> bool {
        matches!(self, Self::OnLine | Self::Left | Self::Right)
    }

    /// True for Inside and Outside, the sides whose cut has a waste area
    /// beside it for a helix entry, a lead or a relief notch. On line,
    /// Left and Right remove only their kerf.
    #[must_use]
    pub const fn has_waste_area(self) -> bool {
        matches!(self, Self::Inside | Self::Outside)
    }
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

/// A holding tab placed on one outline of one instance (#105, ADR-0005):
/// material a Profile that cuts the outline leaves standing, up to
/// `height_mm`. Its place is a point in definition space, so it follows
/// the instance when the instance moves, rotates, mirrors or scales; a
/// Profile centres the tab at its tool path's point nearest it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlacedTab {
    /// This tab's id.
    pub id: TabId,
    /// The instance whose outline holds the tab.
    pub instance: InstanceId,
    /// The outline: an index into the instance's definition's
    /// [`FeatureDefinition::profiles`].
    pub profile: usize,
    /// The tab's centre, X in definition space, millimetres: on the
    /// profile, or on the tool path of the Profile it was placed for.
    pub x_mm: f64,
    /// The tab's centre, Y in definition space, millimetres (Y-up).
    pub y_mm: f64,
    /// Tab length along the outline, millimetres. A pass below the tab top
    /// rises over `width_mm` plus the tool diameter of its tool path.
    pub width_mm: f64,
    /// Tab top above the stock bottom, millimetres.
    pub height_mm: f64,
    /// Tab cross-section.
    pub shape: TabShape,
}

/// Holding tabs to place on every outline a Profile cuts (#105, ADR-0005):
/// a generator of [`PlacedTab`]s with the phase 2 layout rules, as
/// `dreamcarve tabs place` and `init` use it, and the format 10 tab
/// setting a format 11 file may still hold as [`ProfileOp::tabs`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TabSpec {
    /// Where the tabs go along the outline.
    pub placement: TabPlacement,
    /// Tab length along the outline, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub width_mm: f64,
    /// Tab top above the stock bottom, millimetres.
    #[serde(deserialize_with = "units_input::mm")]
    pub height_mm: f64,
    /// Tab cross-section.
    pub shape: TabShape,
    /// Whether holes get tabs too: the outlines nested in another of the
    /// operation's outlines as holes. Without, a hole is cut free, since
    /// its slug is waste (#109). `init` writes `false`; the version 9
    /// migration writes `true`, as every outline had tabs before.
    pub on_holes: bool,
    /// Tabs slide off corners (#285, functional spec capability 35): a
    /// tab whose stretch would lie where the bit's path turns 30° or more
    /// within one bit diameter moves along the path to the nearest place
    /// where it does not. `init` writes `true`; the version 14 migration
    /// writes `false`, so older projects cut as before.
    pub avoid_corners: bool,
}

/// Where a [`TabSpec`] puts tabs along each tool-centre path.
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
        #[serde(deserialize_with = "units_input::vec_mm")]
        distances_mm: Vec<f64>,
    },
}

/// Tab cross-section.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabShape {
    /// Square-shouldered tab.
    Rect,
    /// Triangular bridge: straight ramps up to the tab top and back down.
    Smooth,
}
