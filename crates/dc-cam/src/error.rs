// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Errors from toolpath generation (`docs/specs/phase-1/design.md`,
//! "Algorithms → Profile contour"). An error means the toolpath cannot be
//! trusted, so none is produced.

use dc_geometry::GeometryError;
use dc_project::{DefinitionId, InstanceId, OperationId, ToolId};
use std::fmt;

use crate::Overhang;

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

/// What is wrong with a [`crate::CutBlock`] a generator handed to
/// [`crate::sequence`].
#[derive(Debug, Clone, PartialEq)]
pub enum BlockProblem {
    /// A cut or descent move ends below the block's pass Z.
    MoveBelowPass {
        /// The block's pass Z, mm.
        z_mm: f64,
        /// The lowest move end, mm.
        lowest_mm: f64,
    },
    /// The shape's passes are not the consecutive integers from 0.
    PassesNotConsecutive {
        /// The pass indices the shape's blocks carry, sorted.
        passes: Vec<usize>,
    },
    /// The block has no descent moves, so it plunges, but its descent
    /// starts away from its entry.
    PlungeAwayFromEntry,
}

impl fmt::Display for BlockProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MoveBelowPass { z_mm, lowest_mm } => {
                write!(
                    f,
                    "a move ends at Z {lowest_mm} mm, below the pass Z {z_mm} mm"
                )
            }
            Self::PassesNotConsecutive { passes } => {
                write!(f, "its passes {passes:?} are not consecutive from 0")
            }
            Self::PlungeAwayFromEntry => f.write_str("a plunge starts away from the entry point"),
        }
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
        /// The tool's id: names need not be unique (#71).
        tool_id: ToolId,
        /// Which rate is missing.
        field: FeedField,
    },
    /// The tool is not a flat end mill and the operation offsets by half
    /// the diameter: an Outside or Inside Profile, or a Pocket. Only a flat
    /// end mill cuts that offset at every depth, so any other cutter makes
    /// the part the wrong size (#67). Validation reports this first.
    #[error(
        "operation `{op}` cuts with tool `{tool}`, {cutter}; offsetting by half the diameter needs a flat end mill, or the part comes out the wrong size"
    )]
    CutterNotFlat {
        /// The operation.
        op: OperationId,
        /// The tool's name.
        tool: String,
        /// The tool's id: names need not be unique (#71).
        tool_id: ToolId,
        /// The cutter in words, such as `a V-bit`.
        cutter: &'static str,
    },
    /// Neither the operation nor its tool sets a step-down.
    #[error("operation `{op}` has no step-down and tool `{tool}` sets none")]
    MissingStepDown {
        /// The operation.
        op: OperationId,
        /// The tool's name.
        tool: String,
        /// The tool's id: names need not be unique (#71).
        tool_id: ToolId,
    },
    /// Neither the Pocket nor its tool sets a step-over fraction.
    #[error("operation `{op}` has no step_over_fraction and tool `{tool}` sets none")]
    MissingStepOver {
        /// The operation.
        op: OperationId,
        /// The tool's name.
        tool: String,
        /// The tool's id: names need not be unique (#71).
        tool_id: ToolId,
    },
    /// The step-over fraction in effect is not in (0, 1], or is so small
    /// that one pass would need more than [`crate::pocket::MAX_RINGS`]
    /// rings.
    #[error(
        "operation `{op}` has step-over fraction {step_over_fraction}; it must be more than 0 and at most 1"
    )]
    InvalidStepOver {
        /// The operation.
        op: OperationId,
        /// The fraction in effect.
        step_over_fraction: f64,
    },
    /// The operation's entry has an angle outside (0, 45] degrees or a
    /// helix diameter fraction outside (0, 1]. Validation rejects both; the
    /// generator does not trust its callers.
    #[error("operation `{op}` has entry {field} {value}; it must be {range}")]
    InvalidEntry {
        /// The operation.
        op: OperationId,
        /// `angle_deg` or `diameter_fraction`.
        field: &'static str,
        /// The value set.
        value: f64,
        /// The allowed range, as the user reads it.
        range: &'static str,
    },
    /// The operation's lead has a radius that is not more than 0, a sweep
    /// outside (0, 180] degrees, or a negative overcut. Validation rejects
    /// each; the generator does not trust its callers.
    #[error("operation `{op}` has lead {field} {value}; it must be {range}")]
    InvalidLead {
        /// The operation.
        op: OperationId,
        /// `radius_mm`, `sweep_deg` or `overcut_mm`.
        field: &'static str,
        /// The value set.
        value: f64,
        /// The allowed range, as the user reads it.
        range: &'static str,
    },
    /// A raster Pocket's angle is not a number of degrees in [0, 180).
    /// Validation reports it at `strategy.raster.angle_deg`; the generator
    /// refuses it too rather than guess what was meant.
    #[error(
        "operation `{op}` has raster angle {angle_deg}°; it must be 0 or more and less than 180"
    )]
    InvalidRasterAngle {
        /// The operation.
        op: OperationId,
        /// The angle given, degrees.
        angle_deg: f64,
    },
    /// A helix entry on an On line, Left or Right Profile, which removes
    /// nothing but its own kerf, so no material is there to helix in.
    /// Validation reports it (`helix_needs_waste_side`); the generator
    /// refuses it too.
    #[error(
        "operation `{op}` cuts on the line or beside an open path and asks for a helix entry; a helix needs waste beside the cut, so use ramp or plunge"
    )]
    HelixNeedsWasteSide {
        /// The operation.
        op: OperationId,
    },
    /// The operation asks for something this build does not generate yet.
    /// Validation accepts it, so a project may carry it; export refuses it
    /// rather than cutting something else.
    #[error("operation `{op}` uses {feature}, which is not supported by this build yet")]
    NotSupportedYet {
        /// The operation.
        op: OperationId,
        /// The feature, as the user would name it (`a raster strategy`).
        feature: String,
    },
    /// A Pocket's `allowance_mm` is negative or not a number. Validation
    /// reports it too; a negative allowance would cut into the walls.
    #[error("operation `{op}` has allowance_mm {allowance_mm}; it must be 0 or more")]
    InvalidAllowance {
        /// The operation.
        op: OperationId,
        /// The allowance given, mm.
        allowance_mm: f64,
    },
    /// A rest Pocket's `rest_of` is not an earlier enabled Pocket with the
    /// same targets. Validation reports it too; the generator refuses it
    /// rather than guess what the reference left.
    #[error(
        "operation `{op}` rests on `{rest_of}`, which is not an earlier enabled Pocket with the same targets; no toolpath was produced"
    )]
    RestOfInvalid {
        /// The rest operation.
        op: OperationId,
        /// The operation it names.
        rest_of: OperationId,
    },
    /// A Pocket's cuts would leave its region or enter an island (S9): the
    /// tool's footprint along the toolpath covers `area_mm2` of material
    /// the operation must not touch. A defect in the generator, never in
    /// the user's project.
    #[error(
        "operation `{op}` would cut {area_mm2:.3} mm² outside the material of {shape}; no toolpath was produced"
    )]
    OffMaterial {
        /// The operation.
        op: OperationId,
        /// The pocket's outline.
        shape: ShapeRef,
        /// How much of the footprint lies off the material, mm².
        area_mm2: f64,
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
        /// The tool's id: names need not be unique (#71).
        tool_id: ToolId,
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
    /// The cutter would leave the stock: the operation's swept extent,
    /// toolpath plus tool radius, runs past a stock edge (#62).
    #[error("operation `{op}` cuts off the stock: {overhang}")]
    OffStock {
        /// The operation.
        op: OperationId,
        /// How far past each edge, mm.
        overhang: Overhang,
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
    /// A Profile's cut along one line of a region comes too close to
    /// another line of the same region: the band between them (an outline
    /// and an island or hole in it, or two of those) is narrower than the
    /// tool. Inside, the band is waste and the cutter would cut into the
    /// material the Profile keeps beyond the other line (#192); On line,
    /// the band is the part, and the two lines' cuts would cut through it
    /// (#199), as they would beside an open path drawn in the part, away
    /// from where the two lines meet (#210). `shape` is the one drawn first; `other` is boxed to keep
    /// the error small.
    #[error(
        "operation `{op}` cuts {shape} and {other}, but the band between them is {band_mm:.3} mm wide, narrower than its {diameter_mm} mm tool, so the bit would cut into material the Profile keeps; no toolpath was produced"
    )]
    BandNarrowerThanTool {
        /// The operation.
        op: OperationId,
        /// One line of the band, the one drawn first.
        shape: ShapeRef,
        /// The other line.
        other: Box<ShapeRef>,
        /// The narrowest distance between the two lines, mm.
        band_mm: f64,
        /// The tool's diameter, mm.
        diameter_mm: f64,
    },
    /// An On line Profile's part is narrower than the tool everywhere: every
    /// point of it lies within the tool radius of its lines (the part
    /// shrunk by the radius is empty), so the cut along them removes the
    /// whole part (#223). `shape` is the part's outline.
    #[error(
        "operation `{op}` cuts {shape} On line, but the part is nowhere as wide as its {diameter_mm} mm tool, so the bit would remove the whole part; cut it Outside, or use Engrave to mark it; no toolpath was produced"
    )]
    PartNarrowerThanTool {
        /// The operation.
        op: OperationId,
        /// The part's outline.
        shape: ShapeRef,
        /// The tool's diameter, mm.
        diameter_mm: f64,
    },
    /// A Profile's last pass has an allowance that is not more than 0 and
    /// at most a third of the bit's diameter, or a thickness that is not
    /// more than 0 and at most the depth (#284). Validation rejects both;
    /// the generator does not trust its callers.
    #[error("operation `{op}` has last_pass.{field} {value} mm; it must be {range}")]
    InvalidLastPass {
        /// The operation.
        op: OperationId,
        /// `allowance_mm` or `thickness_mm`.
        field: &'static str,
        /// The value set, mm.
        value: f64,
        /// The allowed range, as the user reads it.
        range: String,
    },
    /// A Profile that cuts On line has a last pass (#284): it cuts with
    /// both flanks, so it leaves no wall to take the allowance off.
    /// Validation rejects it; the generator refuses it too. (changed in
    /// #311: Left and Right take a last pass.)
    #[error(
        "operation `{op}` has a last_pass but cuts on the line; a last pass needs a wall to take the allowance off, so set last_pass to null or cut Inside, Outside, Left or Right"
    )]
    LastPassNeedsSide {
        /// The operation.
        op: OperationId,
    },
    /// #284: a Profile with a last pass allowance cuts two lines closer
    /// than its passes before the last need, `needed_mm`. Inside, two
    /// lines of one region need the tool and the allowance on both,
    /// `diameter + 2 · allowance`: those passes, `allowance_mm` farther
    /// from each line, would cut into the skin the other line's passes
    /// leave, or into the part. Between two regions (a part and another
    /// part, or one inside another's hole) the passes need
    /// `diameter + allowance`, or they would come within the radius of the
    /// other part (#310 review). `shape` is the one drawn first; `other`
    /// is boxed to keep the error small.
    #[error(
        "operation `{op}` cuts {shape} and {other} with a {allowance_mm} mm last pass allowance, but the gap between them is {band_mm:.3} mm, narrower than the {needed_mm:.3} mm its {diameter_mm} mm tool and the allowance need there, so the passes before the last would cut into material the Profile keeps; make last_pass.allowance_mm smaller or set last_pass to null; no toolpath was produced"
    )]
    BandNarrowerThanAllowance {
        /// The operation.
        op: OperationId,
        /// One line of the band, the one drawn first.
        shape: ShapeRef,
        /// The other line.
        other: Box<ShapeRef>,
        /// The narrowest distance between the two lines, mm.
        band_mm: f64,
        /// The tool's diameter, mm.
        diameter_mm: f64,
        /// The last pass allowance, mm.
        allowance_mm: f64,
        /// The gap the passes before the last need, mm.
        needed_mm: f64,
    },
    /// #284, #310 review, changed in #324: somewhere the passes before the
    /// last on `shape` skip material the last pass would then cut whole at
    /// full depth: a neck, slot, notch, inlet or small hole wider than the
    /// tool but narrower than the tool plus the allowance on both sides,
    /// which they bridge (refused once the last pass would take off more
    /// than a square corner's `√2` times the allowance there), or a tip so
    /// narrow that the last pass would take off more than a third of the
    /// tool (or `√2` times the allowance, if more) where they stop short.
    /// A corner whose wedge is thinner than that is cut.
    #[error(
        "operation `{op}` cuts {shape} with a {allowance_mm} mm last pass allowance, but somewhere the passes before the last cannot follow it and the last pass would cut what they skip at full depth: a notch, slot, neck or hole that fits its {diameter_mm} mm tool but not the tool plus the allowance on both sides, or a tip so narrow that the last pass would take off more than a third of the tool there; make last_pass.allowance_mm smaller or set last_pass to null; no toolpath was produced"
    )]
    ShapeNarrowerThanAllowance {
        /// The operation.
        op: OperationId,
        /// The shape the allowance passes cannot follow.
        shape: ShapeRef,
        /// The tool's diameter, mm.
        diameter_mm: f64,
        /// The last pass allowance, mm.
        allowance_mm: f64,
    },
    /// A Left or Right Profile's cut beside one open path comes within the
    /// tool radius of another line of the operation, or of the strip one
    /// radius wide that a line keeps on the side away from its own cut
    /// (#207): the bit would cut into material the drawing keeps. `shape`
    /// and `other` are in drawing order and may be the same path, whose
    /// cut reaches what it keeps further along. `other` is boxed to keep
    /// the error small.
    #[error(
        "operation `{op}` cuts beside {shape} and {other}, but a cut comes within the {diameter_mm} mm tool's radius of one of them or of the material it keeps, so the bit would cut into the part; join lines that meet into one path, move them apart, or check which side each is cut on; no toolpath was produced"
    )]
    CutsIntoKeptSide {
        /// The operation.
        op: OperationId,
        /// One of the two paths, the one drawn first.
        shape: ShapeRef,
        /// The other path.
        other: Box<ShapeRef>,
        /// The tool's diameter, mm.
        diameter_mm: f64,
    },
    /// #311: a Left or Right Profile's passes before the last, `allowance_mm`
    /// farther from each open path than its last pass, come within the
    /// tool radius plus the allowance of another line of the operation,
    /// or of the strip one radius wide that a line keeps on its part side:
    /// they would take that line's skin, which its own last pass is to cut,
    /// or cut into the part. This is [`CamError::CutsIntoKeptSide`]'s rule
    /// with the radius grown by the allowance, as Inside's band rule needs
    /// `diameter + 2 · allowance` ([`CamError::BandNarrowerThanAllowance`]).
    /// `shape` and `other` are in drawing order and may be the same path;
    /// `other` is boxed to keep the error small.
    #[error(
        "operation `{op}` cuts beside {shape} and {other} with a {allowance_mm} mm last pass allowance, but a pass before the last comes within the {diameter_mm} mm tool's radius plus the allowance of one of them or of the material it keeps, so it would cut the skin that line's last pass takes off, or the part; move the lines apart, make last_pass.allowance_mm smaller or set last_pass to null; no toolpath was produced"
    )]
    AllowanceCutsIntoKeptSide {
        /// The operation.
        op: OperationId,
        /// One of the two paths, the one drawn first.
        shape: ShapeRef,
        /// The other path.
        other: Box<ShapeRef>,
        /// The tool's diameter, mm.
        diameter_mm: f64,
        /// The last pass allowance, mm.
        allowance_mm: f64,
    },
    /// #342: a Left or Right Profile's cut beside an open path has no room
    /// at one of the path's ends. The segment that ends there is too short
    /// for the tool, plus the last pass allowance when `allowance_mm` is
    /// set, to cut beside it: on the inside of the turn before it, the cut
    /// beside the earlier segment reaches past the normal through the end
    /// vertex, where every cut must stop (A8, "Ends"), and into the strip
    /// the path keeps or the part past its end. `shape` is boxed to keep
    /// the error small.
    #[error(
        "operation `{op}` cuts beside {shape}, but the segment ending at its vertex {vertex} is {length_mm:.3} mm long, too short for its {diameter_mm} mm tool{} to cut beside it: the cut would reach past the path's end into the part; lengthen or remove that segment, cut the other side, or use a smaller tool; no toolpath was produced",
        with_allowance(*.allowance_mm)
    )]
    EndSegmentHasNoRoom {
        /// The operation.
        op: OperationId,
        /// The open path.
        shape: Box<ShapeRef>,
        /// The end vertex, 0-based in the path as drawn: 0 for its first,
        /// one less than its vertex count for its last.
        vertex: usize,
        /// The length of the segment that ends there, mm.
        length_mm: f64,
        /// The tool's diameter, mm.
        diameter_mm: f64,
        /// The last pass allowance, mm, when only the passes before the
        /// last have no room.
        allowance_mm: Option<f64>,
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
    /// Tabs were asked for an operation the project does not have (#105).
    #[error("operation `{op}` is not in the project")]
    UnknownOperation {
        /// The operation asked for.
        op: OperationId,
    },
    /// Tabs were asked for an operation that is not a Profile, which is
    /// the only operation that cuts round tabs (#105).
    #[error("operation `{op}` is a {kind} operation; only a Profile cuts tabs")]
    TabsNeedProfile {
        /// The operation.
        op: OperationId,
        /// Its kind, as the file tags it.
        kind: &'static str,
    },
    /// A Drill setting cannot be cut: a chip-break gap that is not a
    /// positive number, or a dwell that is negative or not finite.
    #[error("operation `{op}` has {field} {value}; it must be a non-negative number")]
    InvalidDrillSetting {
        /// The operation.
        op: OperationId,
        /// Which setting: `gap_mm` or `dwell_s`.
        field: &'static str,
        /// The value as set.
        value: f64,
    },
    /// A generator produced a cut block the sequencer cannot trust. This
    /// is a defect in the generator, never in the user's project.
    #[error("shape {shape}, pass {pass}: {problem}")]
    InvalidBlock {
        /// The block's shape.
        shape: ShapeRef,
        /// The block's pass index.
        pass: usize,
        /// What is wrong.
        problem: BlockProblem,
    },
    /// The containment pairs given to the sequencer form a cycle, so no
    /// shape can be cut first. `nest` never produces one.
    #[error("shape {shape} encloses itself through other shapes; no cut order exists")]
    ContainmentCycle {
        /// A shape on the cycle.
        shape: ShapeRef,
    },
    /// Any other geometry failure.
    #[error(transparent)]
    Geometry(#[from] GeometryError),
}

/// The words [`CamError::EndSegmentHasNoRoom`] adds for a last pass
/// allowance: none without one.
fn with_allowance(allowance_mm: Option<f64>) -> String {
    allowance_mm.map_or_else(String::new, |a| {
        format!(" and its {a} mm last pass allowance")
    })
}
