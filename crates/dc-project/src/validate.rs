// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Project validation (`docs/specs/phase-1/design.md`, "Data model →
//! dc-project"; the "Validation" sections of the phase 2 and phase 3 design
//! specs for the later operations). [`validate`] reports every problem it
//! finds rather than stopping at the first, so the CLI `check` command can
//! print them all.
//!
//! An error makes a project unsafe or impossible to machine: a dangling
//! id, a number out of range or not finite, a coordinate beyond
//! [`MAX_COORDINATE_MM`], and every value the user must supply that is
//! unset, since nothing may default one. A warning still allows a correct
//! result but needs the user's attention. Each [`Problem`] variant says
//! which it is and why.
//!
//! A tool's rates, and the checks for values that are valid but dangerous,
//! count only for enabled operations and the tools they use: an unused
//! tool is one warning instead, and its unset rates come back as errors
//! once an operation that uses it is enabled. No check changes a value;
//! they only report.
//!
//! [`Problem::field`] names where in the file each problem is, so the CLI
//! can turn it into a `file:line:column` with [`crate::SourceMap`].

use crate::bitmap::{BitmapColour, BitmapError, MAX_BITMAP_SIDE_PX};
use crate::countersink::{COUNTERSINK_TOLERANCE_MM, countersink_depth_mm};
use crate::ids::{BitmapId, DefinitionId, InstanceId, OperationId, TabId, ToolId};
use crate::model::{
    Bitmap, Board, BoreOp, ClearanceStrategy, ClearingPass, CornerRelief, DrillCycle, DrillOp,
    EngraveOp, Entry, InlayOp, LastPass, Lead, Operation, OriginXy, PeckRetract, PhotoVCarveOp,
    PlacedTab, PocketOp, ProfileOp, Project, Side, TabPlacement, TabSpec, Tool, ToolKind, VCarveOp,
    VCarveStrategy,
};
use crate::post_processor::{
    MAX_FILE_EXTENSION_CHARS, SpindleControl, ToolChangeMode, is_safe_file_extension,
};
use crate::project::cleared_start_plane;
use crate::tool::CuttingLimit;
use dc_geometry::{EPS_MM, GeometryError, MAX_COORDINATE_MM};
use std::collections::BTreeMap;
use std::collections::BTreeSet;

/// How serious a [`Problem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The project cannot be machined safely as it stands.
    Error,
    /// The user should know, but the project is still usable.
    Warning,
}

/// What an earlier floor must hold to clear an operation's start plane
/// ([`Problem::StartDepthUncleared`]), as its message names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartFootprint {
    /// A V-Carve's targets.
    Targets,
    /// A countersink's V-bit, its whole diameter, at each hole.
    BitAtEachHole,
    /// A Photo V-Carve's picture's footprint rectangle.
    Picture,
}

impl StartFootprint {
    /// The footprint as [`Problem::StartDepthUncleared`]'s message names it.
    const fn in_words(self) -> &'static str {
        match self {
            Self::Targets => "its targets",
            Self::BitAtEachHole => "the whole of its V-bit at each hole",
            Self::Picture => "its picture's footprint",
        }
    }
}

/// One finding of [`validate`].
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Problem {
    /// A definition's `id` differs from its key in `definitions`.
    #[error("definition keyed `{key}` has id `{id}`")]
    DefinitionKeyMismatch {
        /// The map key.
        key: DefinitionId,
        /// The `id` field inside the definition.
        id: DefinitionId,
    },
    /// Two instances share an id.
    #[error("instance id `{id}` is used twice")]
    DuplicateInstanceId {
        /// The repeated id.
        id: InstanceId,
    },
    /// Two tools share an id.
    #[error("tool id `{id}` is used twice")]
    DuplicateToolId {
        /// The repeated id.
        id: ToolId,
    },
    /// Two operations share an id.
    #[error("operation id `{id}` is used twice")]
    DuplicateOperationId {
        /// The repeated id.
        id: OperationId,
    },
    /// An instance places a definition that does not exist.
    #[error("instance `{instance}` places definition `{definition}`, which does not exist")]
    DanglingDefinition {
        /// The instance holding the reference.
        instance: InstanceId,
        /// The missing definition.
        definition: DefinitionId,
    },
    /// An operation cuts with a tool that does not exist.
    #[error("operation `{operation}` uses tool `{tool}`, which does not exist")]
    DanglingTool {
        /// The operation holding the reference.
        operation: OperationId,
        /// The missing tool.
        tool: ToolId,
    },
    /// An operation targets an instance that does not exist.
    #[error("operation `{operation}` targets instance `{instance}`, which does not exist")]
    DanglingInstance {
        /// The operation holding the reference.
        operation: OperationId,
        /// The missing instance.
        instance: InstanceId,
    },
    /// Two placed tabs share an id.
    #[error("two tabs have the id `{id}`")]
    DuplicateTabId {
        /// The repeated id.
        id: TabId,
    },
    /// A placed tab is on an instance that does not exist.
    #[error("tab `{tab}` is on instance `{instance}`, which does not exist")]
    TabDanglingInstance {
        /// The tab.
        tab: TabId,
        /// The missing instance.
        instance: InstanceId,
    },
    /// A placed tab is on a profile its instance's definition does not
    /// have.
    #[error(
        "tab `{tab}` is on profile {profile} of instance `{instance}`, whose definition has {count} profile(s), numbered from 0"
    )]
    TabProfileMissing {
        /// The tab.
        tab: TabId,
        /// The instance.
        instance: InstanceId,
        /// The profile index asked for.
        profile: usize,
        /// How many profiles the definition has.
        count: usize,
    },
    /// A number is outside the range its field allows.
    #[error("{field} is {value}, expected {expected}")]
    OutOfRange {
        /// Path to the field, such as `tools[t1].diameter_mm`.
        field: String,
        /// The value found.
        value: f64,
        /// The allowed range, in words.
        expected: &'static str,
    },
    /// A number is NaN or infinite.
    #[error("{field} is not a finite number")]
    NonFinite {
        /// Path to the field.
        field: String,
    },
    /// An operation is disabled and will not be exported.
    #[error("operation `{operation}` is disabled and will not be exported")]
    OperationDisabled {
        /// The disabled operation.
        operation: OperationId,
    },
    /// A tool has no value for a field export needs (feed, plunge or RPM),
    /// or no step-down while an enabled operation sets none either.
    /// An error, so `check` lists every unset value in one run.
    #[error(
        "tools[{tool}].{field} is not set ({}) for tool `{name}`",
        unset_unit(.field)
    )]
    ToolValueUnset {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The unset field, such as `feed_mm_min`.
        field: &'static str,
    },
    /// A tool no enabled operation uses. Its unset rates are not reported:
    /// nothing cuts with it, and they are never defaulted.
    #[error("tool `{tool}` (`{name}`) is defined but not used by any enabled operation")]
    UnusedTool {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
    },
    /// The stock thickness is unset. It sets depth, tab heights and, with
    /// Z zero at the stock bottom, safe Z, so it is never defaulted.
    #[error("{STOCK_THICKNESS} is not set (mm)")]
    StockThicknessUnset,
    /// An operation has no value for a field export needs (its depth).
    #[error(
        "operations[{operation}].{field} is not set ({}) for operation `{name}`",
        unset_unit(.field)
    )]
    OperationValueUnset {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
        /// The unset field, such as `depth_mm`.
        field: &'static str,
    },
    /// A tool's number is outside the range its field allows.
    #[error("tool `{name}` (`{tool}`): {field} is {value}, expected {expected}")]
    ToolOutOfRange {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The field within the tool, such as `feed_mm_min`.
        field: &'static str,
        /// The value found.
        value: f64,
        /// The allowed range, in words.
        expected: &'static str,
    },
    /// A definition's profile is open. Pocket and Inside and Outside
    /// Profile cut only closed shapes, so it is never cut (functional
    /// spec, capability 8). Not reported for a definition an enabled
    /// Engrave or an On line, Left or Right Profile targets, since those
    /// cut it.
    #[error(
        "`{name}` (definition `{definition}`, profile {profile}) is an open path and no operation cuts it; only an On line, Left or Right Profile or an Engrave cuts an open path"
    )]
    OpenProfile {
        /// The definition.
        definition: DefinitionId,
        /// The definition's display name.
        name: String,
        /// Index of the profile within the definition.
        profile: usize,
    },
    /// The post-processor's file extension could name the project file or a
    /// path (see [`is_safe_file_extension`]).
    #[error(
        "the post-processor's file extension `{}` is not allowed: it must be 1 to {MAX_FILE_EXTENSION_CHARS} letters or digits and not `dcv`",
        .extension.escape_default()
    )]
    UnsafeFileExtension {
        /// The extension found.
        extension: String,
    },
    /// The tool-change mode is `"m6"`, but the post-processor definition's
    /// tool change has no `M6`, so it cannot change tools within a program
    /// (#95). GRBL 1.1 has none.
    #[error(
        "tool_change_mode is \"m6\", but post-processor `{post}` has no M6 in its tool change, so it cannot change tools within one program; use \"separate_programs\""
    )]
    ToolChangeModeNeedsM6 {
        /// The post-processor's id.
        post: String,
    },
    /// An enabled operation cuts deeper per pass than its tool is wide. A
    /// warning: the program is still what was asked for.
    #[error(
        "operation `{name}` (`{operation}`) cuts {step_down_mm} mm per pass, deeper than the {diameter_mm} mm diameter of tool `{tool_name}`; passes that deep can break the bit, so use a step-down of at most {diameter_mm} mm"
    )]
    StepDownAboveDiameter {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
        /// The tool's display name.
        tool_name: String,
        /// Where the step-down in effect is set: the operation's
        /// `step_down_mm`, or else the tool's.
        field: String,
        /// The step-down in effect, mm.
        step_down_mm: f64,
        /// The tool's diameter, mm.
        diameter_mm: f64,
    },
    /// An enabled Outside, Inside, Left or Right Profile cuts with a tool
    /// that is not a flat end mill. Those sides offset by half
    /// the diameter, so any
    /// other cutter makes the part the wrong size: an error. On the line the
    /// tool centre follows the line, so any cutter is allowed. So is a male
    /// Inlay's cut-out (format 23), at `cut_out.tool`.
    #[error(
        "operation `{name}` (`{operation}`) {} tool `{tool_name}` (`{tool}`), {cutter}; {}. Use an end mill",
        flat_cut(.field).0,
        flat_cut(.field).1
    )]
    CutterNotFlat {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        tool_name: String,
        /// The cutter in words, such as `a V-bit`.
        cutter: &'static str,
        /// Where the tool is named under the operation: `tool`, or an
        /// Inlay's `cut_out.tool`.
        field: &'static str,
    },
    /// An enabled operation targets nothing, so its program would start
    /// and stop the spindle without moving. An error.
    #[error(
        "operation `{name}` (`{operation}`) is enabled but has no targets, so it would cut nothing; add targets or disable it"
    )]
    OperationHasNoTargets {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
    },
    /// A used tool's spindle speed is above [`MAX_RPM`] (#67). A warning:
    /// GRBL and grblHAL clamp `S` to the machine's maximum (`$30`), and
    /// under manual control the speed is only a hint.
    #[error(
        "tool `{name}` (`{tool}`) is set to {rpm} RPM, faster than the {MAX_RPM} RPM a hobby spindle or router turns; check it for a typo"
    )]
    SpindleSpeedHigh {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The speed set, RPM.
        rpm: u32,
    },
    /// A used tool plunges faster than it cuts, which usually means
    /// the two rates are swapped. A warning.
    #[error(
        "tool `{name}` (`{tool}`) plunges at {plunge_mm_min} mm/min, faster than it cuts ({feed_mm_min} mm/min); a plunge rate is usually well below the feed rate, so check the two are not swapped"
    )]
    PlungeAboveFeed {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The plunge rate, mm/min.
        plunge_mm_min: f64,
        /// The feed rate, mm/min.
        feed_mm_min: f64,
    },
    /// A used tool's diameter is above [`MAX_TOOL_DIAMETER_MM`]. A
    /// warning: a shape such a bit does not fit is already skipped with a
    /// warning, and around a part it cuts valid geometry.
    #[error(
        "tool `{name}` (`{tool}`) is {diameter_mm} mm in diameter, larger than the {MAX_TOOL_DIAMETER_MM} mm of a hobby router bit; check it for a typo"
    )]
    LargeDiameter {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The diameter, mm.
        diameter_mm: f64,
    },
    /// Two tools of enabled operations resolve to the same `T` number,
    /// set or positional (functional spec capability 21a): the
    /// program would change to the wrong bit. An error at the later tool.
    #[error(
        "tool `{name}` (`{tool}`) and tool `{other}` would both be T{number} in the program; a tool with no `number` is its position in the list, so give one of them another number"
    )]
    DuplicateToolNumber {
        /// The later tool.
        tool: ToolId,
        /// Its display name.
        name: String,
        /// The number both resolve to.
        number: u32,
        /// The earlier tool with that number.
        other: ToolId,
    },
    /// A Pocket's `rest_of` is not an earlier enabled Pocket with the same
    /// targets (functional spec capability 13). An error.
    #[error(
        "operation `{name}` (`{operation}`) rests on `{rest_of}`, which is not an earlier enabled Pocket with the same targets; rest machining clears what such a Pocket left"
    )]
    RestOfInvalid {
        /// The Pocket holding the reference.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The operation named.
        rest_of: OperationId,
    },
    /// An On line, Left or Right Profile asks for a helix entry (functional
    /// spec capability 16). A helix sits in material the operation removes,
    /// and those cuts remove only their kerf. An error.
    #[error(
        "operation `{name}` (`{operation}`) cuts on the line or beside an open path but asks for a helix entry; a helix sits in material the cut removes, and such a cut removes only its kerf. Use \"plunge\" or a ramp"
    )]
    HelixNeedsWasteSide {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
    },
    /// Canned cycles are on, but the post-processor definition has none
    /// (functional spec capability 14), so the choice could only be
    /// ignored. An error.
    #[error(
        "use_canned_cycles is true, but post-processor `{post}` has no canned cycles, so Drill cannot write them; set it to false"
    )]
    CannedCyclesUnsupported {
        /// The post-processor's id.
        post: String,
    },
    /// An enabled Drill cuts with a cutter other than a drill or flat end
    /// mill: a V-bit or ball nose makes a cone, not a hole. A
    /// warning.
    #[error(
        "operation `{name}` (`{operation}`) drills with tool `{tool_name}` (`{tool}`), {cutter}, which makes a cone rather than a flat-bottomed hole; a drill or an end mill is usual"
    )]
    DrillCutterUnusual {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        tool_name: String,
        /// The cutter in words, such as `a V-bit`.
        cutter: &'static str,
    },
    /// An enabled Pocket or Profile asks for corner relief on a tool whose
    /// step-down is unset: the relief pass steps down at the
    /// tool's step-down, so it is skipped. A warning.
    #[error(
        "operation `{name}` (`{operation}`) asks for corner relief, but tool `{tool_name}` (`{tool}`) has no step_down_mm, which the relief pass steps down at; the relief is skipped until it is set"
    )]
    ReliefNeedsStepDown {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        tool_name: String,
    },
    /// The clearance height is below an enabled operation's safe Z:
    /// a link between parts would run lower than that operation's links
    /// between passes. An error.
    #[error(
        "{CLEARANCE} is {clearance_mm} mm, below the {safe_z_mm} mm safe_z_mm of operation `{name}` (`{operation}`); links between parts would run lower than its links between passes, so raise the clearance to at least {safe_z_mm} mm"
    )]
    ClearanceBelowSafeZ {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// `heights.clearance_mm`.
        clearance_mm: f64,
        /// The operation's `safe_z_mm`.
        safe_z_mm: f64,
    },
    /// `heights.tool_change_machine_z_mm` is 0 or above, or rounds to 0 in
    /// the program's decimal places (#283). grblHAL
    /// and GRBL home Z to the top of its travel at a negative machine
    /// position (grblHAL `machine_limits.c`, GRBL `limits.c`), so a Z of 0
    /// or more is past the limit switch. An error.
    #[error(
        "{MACHINE_Z} is {machine_z_mm} mm, which is not below 0 as the program writes it; grblHAL and GRBL, and LinuxCNC as its configuration tools set it up, home Z at the top of its travel, where machine Z is 0 or just below, so the lift must be below 0, at least the homing pull-off below it"
    )]
    MachineZNotBelowHome {
        /// The value, mm.
        machine_z_mm: f64,
    },
    /// `heights.tool_change_machine_z_mm` is set, but the post-processor
    /// documents no machine-coordinate move (`work_coordinates.
    /// machine_command` is `null`), so the lift cannot be written.
    /// An error.
    #[error(
        "{MACHINE_Z} is set, but post-processor `{post}` does not document a machine-coordinate move (G53), so the lift cannot be written; set it to null"
    )]
    MachineZNeedsG53 {
        /// The post-processor's id.
        post: String,
    },
    /// An enabled Engrave with a V-bit cuts deeper than the bit's cone is
    /// tall, so the shank, not the cone, would cut. A warning.
    #[error(
        "operation `{name}` (`{operation}`) engraves {depth_mm} mm deep with tool `{tool_name}`, whose cone is only {cone_height_mm:.3} mm tall; below that the shank cuts, so use a shallower depth or a wider bit"
    )]
    EngraveDeeperThanCone {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The tool's display name.
        tool_name: String,
        /// The depth asked for, mm.
        depth_mm: f64,
        /// The cone's height, mm: half the diameter over the tangent of
        /// half the included angle.
        cone_height_mm: f64,
    },
    /// An enabled Profile's last pass stock to leave is not more than 0 and
    /// at most a third of its bit's diameter (VCarve advises keeping it
    /// well below a third). An error.
    #[error(
        "operation `{name}` (`{operation}`) has last_pass.stock_to_leave_mm {stock_to_leave_mm} mm with tool `{tool_name}` ({diameter_mm} mm); it must be more than 0 and at most {limit_mm:.3} mm, a third of the bit's diameter, as VCarve advises"
    )]
    LastPassStockToLeave {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The tool's display name.
        tool_name: String,
        /// The stock to leave set, mm.
        stock_to_leave_mm: f64,
        /// The bit's diameter, mm.
        diameter_mm: f64,
        /// The largest stock to leave allowed, mm: a third of the diameter.
        limit_mm: f64,
    },
    /// A Profile has a last pass but cuts On line: it cuts with
    /// both flanks of the bit, so it leaves no wall for the last pass to
    /// take the stock to leave off. An error. (Left and Right, which cut
    /// one flank against the part, take a last pass.)
    #[error(
        "operation `{name}` (`{operation}`) cuts {} but has a last_pass; a last pass takes the stock to leave off a wall the other passes leave, so it needs an Inside, Outside, Left or Right Profile. Set last_pass to null, or cut on one side of the line",
        side_cut(*.side)
    )]
    LastPassNeedsSide {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The side it cuts.
        side: Side,
    },
    /// An enabled Profile's last pass is thicker than its step-down in
    /// effect, the operation's or else the tool's: it would
    /// cut that much in one pass, where every other pass steps down less.
    /// An error.
    #[error(
        "operation `{name}` (`{operation}`) has last_pass.thickness_mm {thickness_mm} mm, more than its {step_down_mm} mm step-down ({step_down_field}); the last pass would cut that deep in one go. Make it at most the step-down, or set it to null for the usual last pass"
    )]
    LastPassThickerThanStepDown {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// `last_pass.thickness_mm`.
        thickness_mm: f64,
        /// The step-down in effect, mm.
        step_down_mm: f64,
        /// Where that step-down is set: `operations[o1].step_down_mm` or
        /// `tools[t1].step_down_mm`.
        step_down_field: String,
    },
    /// An enabled Bore's `pitch_mm` is more than its tool's step-down
    /// (format 18): one turn of the helix
    /// would cut deeper than a pass. An error.
    #[error(
        "operation `{name}` (`{operation}`) has pitch_mm {pitch_mm} mm, more than its tool's {step_down_mm} mm step-down; each turn of the helix would cut deeper than a pass. Make it at most the step-down, or set it to null to use the step-down"
    )]
    BorePitchOverStepDown {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The pitch asked for, mm.
        pitch_mm: f64,
        /// The tool's step-down, mm.
        step_down_mm: f64,
    },
    /// `flatten_tolerance_mm` is above [`MAX_FLATTEN_TOLERANCE_MM`]
    /// (format 20): the safety checks' slack
    /// and the medial axis's chords scale with it. An error.
    #[error(
        "{FLATTEN_TOLERANCE} is {flatten_tolerance_mm} mm, above the {MAX_FLATTEN_TOLERANCE_MM} mm limit; the safety checks allow the program that far off the true curves, so a coarser tolerance could hide a cut through a wall. Use at most {MAX_FLATTEN_TOLERANCE_MM} mm"
    )]
    FlattenToleranceRange {
        /// The value set, mm.
        flatten_tolerance_mm: f64,
    },
    /// An enabled V-Carve or Inlay (format 22) cuts with a tool that is
    /// neither a V-bit (format 20; functional spec, capability 38) nor a
    /// tapered ball nose (format 28): its depth follows
    /// the bit's profile. A Photo V-Carve (format 27) needs a V-bit, for
    /// good. An error.
    #[error(
        "operation `{name}` (`{operation}`) carves with tool `{tool_name}` (`{tool}`), {cutter}; {}",
        carving_bit_advice(.cutter, *.photo)
    )]
    VCarveNeedsVBit {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        tool_name: String,
        /// The cutter in words, such as `an end mill`.
        cutter: &'static str,
        /// Whether the operation is a Photo V-Carve.
        photo: bool,
    },
    /// A tapered ball nose's value is out of range (format 28): a
    /// tip diameter not above 0, a taper per side not in (0, 45], flutes
    /// not finite or ending inside the ball, or a cutting diameter not
    /// above the tip. Reported instead of `tool_out_of_range` or
    /// `non_finite` for those fields, on every tool, used or not. An error.
    #[error("tool `{name}` (`{tool}`): {field} is {value}, expected {expected}")]
    TaperedBallNoseRange {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The field within the tool, such as
        /// `cutter.tapered_ball_nose.taper_angle_deg` or `diameter_mm`.
        field: &'static str,
        /// The value found.
        value: f64,
        /// What it must be, in words.
        expected: String,
    },
    /// A V-bit an enabled V-Carve, countersink (format 21) or Inlay (format
    /// 22) cuts with has an included angle not above 0 degrees and below
    /// 180 (format 20). Reported instead of
    /// `tool_out_of_range` for such a tool. An error.
    #[error(
        "tool `{name}` (`{tool}`) has an included angle of {included_angle_deg} degrees; a V-Carve, an Inlay, a Photo V-Carve or a countersink needs a V-bit angle above 0 and below 180 degrees to work out its depth"
    )]
    VBitAngleRange {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The angle set, degrees.
        included_angle_deg: f64,
    },
    /// An enabled V-Carve engages its V-bit deeper than the bit's cone is
    /// tall (format 20; capabilities 39 and 41), so the shank would cut.
    /// So may a countersink (format 21) or an Inlay (format 22), whose
    /// engagement is [`crate::InlayOp::engagement_mm`].
    /// The engagement is the flat depth, plus the start depth unless an
    /// earlier floor at the start depth holds the targets, an enabled
    /// Pocket's or an enabled V-Carve's flat floor
    /// ([`crate::cleared_start_plane`]). An error.
    #[error(
        "operation `{name}` (`{operation}`) engages tool `{tool_name}`{}",
        cone_exceeded(*.engagement_mm, *.cone_height_mm, *.from_stock_top, .field, .limit)
    )]
    VBitConeExceeded {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The tool's display name.
        tool_name: String,
        /// How deep the flanks engage, mm.
        engagement_mm: f64,
        /// The cone's height, mm: half the diameter over the tangent of
        /// half the included angle.
        cone_height_mm: f64,
        /// Whether the engagement counts the start depth, the start
        /// plane not being cleared by a Pocket or a V-Carve floor of the
        /// project.
        from_stock_top: bool,
        /// The field at fault: `flat_depth_mm`, or `start_depth_mm` for a
        /// carve with no flat depth; a countersink's
        /// `cycle.countersink.diameter_mm` (format 21); a female Inlay's
        /// `depth_mm`, and a male's `plug_depth_mm`, its engagement
        /// counted from the stock top (format 22).
        field: &'static str,
        /// What sets `cone_height_mm`: a V-bit's cone,
        /// or a tapered ball nose's flutes or cutting diameter, its cutting
        /// height ([`Tool::cutting_height_mm`]).
        limit: CuttingLimit,
    },
    /// A V-Carve has clearing passes but no flat depth (format 20;
    /// capability 40): with no floor there is nothing to clear. An error.
    #[error(
        "operation `{name}` (`{operation}`) has clearing passes but no flat_depth_mm; with no flat floor there is nothing for them to clear, so set a flat depth or remove the passes"
    )]
    ClearingNeedsFlatDepth {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
    },
    /// An enabled V-Carve's clearing pass cuts with a tool that is not a
    /// flat end mill (format 20; capability 40): it is cut as a Pocket,
    /// which offsets by half the diameter. An error.
    #[error(
        "clearing pass {pass} of operation `{name}` (`{operation}`) cuts with tool `{tool_name}` (`{tool}`), {cutter}; a clearing pass is cut as a Pocket, which only a flat end mill cuts to size. Use an end mill"
    )]
    ClearingNeedsEndMill {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The pass's position in `clearing`, from 0.
        pass: usize,
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        tool_name: String,
        /// The cutter in words, such as `a V-bit`.
        cutter: &'static str,
    },
    /// A clearing pass names a tool that does not exist (format 20). An
    /// error.
    #[error(
        "clearing pass {pass} of operation `{operation}` uses tool `{tool}`, which does not exist"
    )]
    ClearingDanglingTool {
        /// The operation.
        operation: OperationId,
        /// The pass's position in `clearing`, from 0.
        pass: usize,
        /// The missing tool.
        tool: ToolId,
    },
    /// An enabled V-Carve's clearing pass cuts with a tool wider than the
    /// pass before it (format 20; capability 40). It still runs and
    /// clears what it can reach. A warning.
    #[error(
        "clearing pass {pass} of operation `{name}` (`{operation}`) uses tool `{tool_name}` ({diameter_mm} mm), wider than the pass before it (`{previous_tool_name}`, {previous_diameter_mm} mm); it still runs, but list the largest tool first so each pass clears only what the one before could not reach"
    )]
    ClearingNotDescending {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The pass's position in `clearing`, from 0.
        pass: usize,
        /// Its tool's display name.
        tool_name: String,
        /// Its tool's diameter, mm.
        diameter_mm: f64,
        /// The tool's display name of the pass before.
        previous_tool_name: String,
        /// That tool's diameter, mm.
        previous_diameter_mm: f64,
    },
    /// A start depth is below 0 (format 20; capability 41). An error.
    #[error(
        "operation `{name}` (`{operation}`) has start_depth_mm {start_depth_mm} mm; the start plane is a depth below the stock top, 0 or more"
    )]
    StartDepthNegative {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The value set, mm.
        start_depth_mm: f64,
        /// The field, under the operation: `start_depth_mm`, or a
        /// countersink's `cycle.countersink.start_depth_mm` (format 21).
        field: &'static str,
    },
    /// An enabled V-Carve, countersink Drill or Photo V-Carve starts below
    /// the stock top, no enabled earlier Pocket, nor the flat floor of an
    /// enabled earlier V-Carve, holds its footprint, and it does not say
    /// another program cleared the start plane (format 20; capability 41).
    /// An error.
    #[error(
        "operation `{name}` (`{operation}`) starts {start_depth_mm} mm below the stock top, but no enabled Pocket, nor the flat floor of an enabled V-Carve, before it contains {}; add a Pocket of that depth before it or, if another program clears the start plane, set start_plane_cleared_externally to true",
        .footprint.in_words()
    )]
    StartDepthUncleared {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The start depth, mm.
        start_depth_mm: f64,
        /// The field, under the operation, as for
        /// [`Problem::StartDepthNegative`].
        field: &'static str,
        /// What a floor must hold for it.
        footprint: StartFootprint,
    },
    /// An enabled countersink Drill cuts with a tool that is not a V-bit
    /// (format 21; capability 44): its depth follows the bit's angle. An
    /// error.
    #[error(
        "operation `{name}` (`{operation}`) countersinks with tool `{tool_name}` (`{tool}`), {cutter}; a countersink's depth follows the angle of a V-bit, so use a V-bit"
    )]
    CountersinkNeedsVBit {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        tool_name: String,
        /// The cutter in words, such as `a drill`.
        cutter: &'static str,
    },
    /// A countersink's mouth diameter is not above 0 (format 21). An
    /// error.
    #[error(
        "operation `{name}` (`{operation}`) has a countersink diameter_mm of {diameter_mm} mm; the mouth must be wider than 0"
    )]
    CountersinkDiameterPositive {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The value set, mm.
        diameter_mm: f64,
    },
    /// An enabled countersink asks for a mouth wider than its V-bit
    /// (format 21; capability 44): past its diameter the bit has no
    /// cutting edge, and the shank would rub. An error.
    #[error(
        "operation `{name}` (`{operation}`) asks for a {diameter_mm} mm countersink, wider than the {tool_diameter_mm} mm tool `{tool_name}`; past its diameter a V-bit has no cutting edge, so use a mouth at most the bit's diameter or a wider bit"
    )]
    CountersinkMouthExceedsTool {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The tool's display name.
        tool_name: String,
        /// The mouth asked for, mm.
        diameter_mm: f64,
        /// The tool's diameter, mm.
        tool_diameter_mm: f64,
    },
    /// A countersink Drill sets `allow_tool_wider_than_hole` to `true`
    /// (format 21): the field has no effect on a countersink, whose bit is
    /// wider than the hole by design. A warning.
    #[error(
        "operation `{name}` (`{operation}`) countersinks, so allow_tool_wider_than_hole has no effect: a countersink's V-bit is wider than the hole by design, and a hole at least as wide as the mouth is skipped instead; set it to false"
    )]
    CountersinkIgnoresToolWidth {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
    },
    /// An Inlay's `glue_gap_mm` is not below its `depth_mm` (format 22;
    /// capability 45): the male's start plane, `depth_mm − glue_gap_mm`
    /// below the stock top, would be at or above the stock top, so the
    /// plug would have no wall to seat. An error.
    #[error(
        "operation `{name}` (`{operation}`) has glue_gap_mm {glue_gap_mm} mm, not below its depth_mm {depth_mm} mm; the male starts depth_mm − glue_gap_mm below the stock top, so its plug would have no wall to seat. Make the glue gap smaller than the depth (0 or 0.25 mm to start)"
    )]
    InlayGlueGapNotBelowDepth {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The glue gap set, mm.
        glue_gap_mm: f64,
        /// The depth set, mm.
        depth_mm: f64,
    },
    /// A male Inlay's `margin_mm` is not above 0 (format 22; capability
    /// 45): its surround is the design grown by the margin, so with none
    /// there is no band to carve round the plug. An error.
    #[error(
        "operation `{name}` (`{operation}`) is a male inlay with margin_mm {margin_mm} mm; its surround is carved that far beyond the design, so it must be above 0 (5 to 7.5 mm leaves room to clamp the plug board)"
    )]
    InlayMarginPositive {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The margin set, mm.
        margin_mm: f64,
    },
    /// An Inlay's `outline_rounding_mm` is set and not a number above 0
    /// (format 24): it is the radius the male's band is closed by,
    /// so 0 or less rounds nothing and is written `null`. An error.
    #[error(
        "operation `{name}` (`{operation}`) has outline_rounding_mm {rounding_mm} mm; the male's band is rounded by that radius, so it must be above 0. Set it to null for no rounding, or to a radius such as 10"
    )]
    InlayOutlineRounding {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The rounding set, mm.
        rounding_mm: f64,
    },
    /// A male Inlay leaves less than [`INLAY_MIN_PLUG_BASE_MM`] of board
    /// under its surround's floor (format 22; capability 45): the board
    /// the plug stands on may flex or crack when it is pressed into the
    /// glue. It still cuts. A warning.
    #[error(
        "operation `{name}` (`{operation}`) cuts its plug's surround {floor_depth_mm} mm below the male board's top, leaving {base_mm} mm of the {thickness_mm} mm male board under it, less than {INLAY_MIN_PLUG_BASE_MM} mm; the base may flex or crack when the plug is pressed into the glue. Use a thicker male board, or a smaller depth_mm or plug_depth_mm"
    )]
    InlayPlugBaseThin {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// How deep the surround's floor is below the stock top, mm:
        /// `depth_mm − glue_gap_mm + plug_depth_mm`.
        floor_depth_mm: f64,
        /// The stock thickness, mm.
        thickness_mm: f64,
        /// What is left under the floor, mm.
        base_mm: f64,
    },
    /// An Inlay cut with a tapered ball nose has a `glue_gap_mm` below the
    /// bit's tangent height `h_t`: the plug's top edge
    /// rides on the pocket's floor fillet, so it stands `lift_mm` proud
    /// and its walls open by `wall_gap_mm` at the surface. A warning.
    #[error(
        "operation `{name}` (`{operation}`) has glue_gap_mm {glue_gap_mm} mm, below its tapered ball nose's tangent height {tangent_height_mm:.3} mm, so the plug's top edge rides on the pocket's rounded floor: it stands about {lift_mm:.3} mm proud with its walls {wall_gap_mm:.3} mm open at the surface. Set glue_gap_mm to at least {tangent_height_mm:.3} mm, and test-fit a coupon first"
    )]
    InlayGlueGapBelowBall {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// `glue_gap_mm`.
        glue_gap_mm: f64,
        /// The bit's tangent height `h_t`, mm.
        tangent_height_mm: f64,
        /// How far the plug stands proud, mm.
        lift_mm: f64,
        /// How far its walls open at the surface, mm.
        wall_gap_mm: f64,
    },
    /// An Inlay cut with a tapered ball nose has a `plug_depth_mm` below
    /// the bit's tangent height `h_t`: the plug's wall
    /// just below the surface is the ball's, narrower than the pocket by up
    /// to `wall_gap_mm`. A warning.
    #[error(
        "operation `{name}` (`{operation}`) has plug_depth_mm {plug_depth_mm} mm, below its tapered ball nose's tangent height {tangent_height_mm:.3} mm, so the plug's wall just below the surface is cut by the ball, up to {wall_gap_mm:.3} mm narrower than the pocket. Set plug_depth_mm to at least {tangent_height_mm:.3} mm"
    )]
    InlayPlugDepthBelowBall {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// `plug_depth_mm`.
        plug_depth_mm: f64,
        /// The bit's tangent height `h_t`, mm.
        tangent_height_mm: f64,
        /// The widest gap at the plug's wall, mm.
        wall_gap_mm: f64,
    },
    /// An Inlay's male board has a width or height set that is not above 0
    /// (format 25): `null` is the stock's, a number is the board's
    /// size. An error.
    #[error(
        "operation `{name}` (`{operation}`) has male_board.{field} {value_mm} mm; the male board's size must be above 0. Set it to null to use the stock's"
    )]
    InlayMaleBoardSize {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// `width_mm` or `height_mm`.
        field: &'static str,
        /// The value set, mm.
        value_mm: f64,
    },
    /// An Inlay's male board has a thickness set that is not above 0
    /// (format 25). An error.
    #[error(
        "operation `{name}` (`{operation}`) has male_board.thickness_mm {thickness_mm} mm; measure the male board and set its thickness, above 0"
    )]
    InlayMaleBoardThickness {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The thickness set, mm.
        thickness_mm: f64,
    },
    /// Two enabled Inlays that cut their males name different male boards:
    /// every male program of a project is cut on the one male board, the
    /// first such Inlay's. An error, on the later Inlay.
    #[error(
        "operation `{name}` (`{operation}`) cuts its male on a male_board other than operation `{first}`'s; every Inlay of a project cuts its male on the one male board, so give them the same male_board"
    )]
    InlayMaleBoardsDiffer {
        /// The later operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The first Inlay that cuts its male, whose board is the project's
        /// male board.
        first: OperationId,
    },
    /// A male Inlay's cut-out tool does not fit the band's flat floor
    /// (format 23): the floor ring, from the mirrored design
    /// grown by `P·tan(α/2)` out to the band's edge (`M` out when a
    /// clearing pass cuts the edge, `M − P·tan(α/2)` with a V-walled
    /// edge), is narrower than the tool, so a cut-out along its outer edge
    /// would cut into the plug's wall. An error.
    #[error(
        "operation `{name}` (`{operation}`) cuts its plug out with tool `{tool_name}`, {diameter_mm} mm across, along the outer edge of the band's flat floor; {}, or use a narrower end mill",
        cut_out_room(*.margin_mm, *.needed_mm, *.at_boundary)
    )]
    InlayCutOutNoRoom {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The cut-out tool's display name.
        tool_name: String,
        /// Its diameter, mm.
        diameter_mm: f64,
        /// The margin set, mm.
        margin_mm: f64,
        /// The least margin with room, mm: `P·tan(α/2)` plus the diameter
        /// with an open edge, `2·P·tan(α/2)` plus it with a V wall
        /// ([`crate::MaleBand::cut_out_margin_needed_mm`]).
        needed_mm: f64,
        /// Whether the band is a drawn boundary (format 26): `margin_mm`
        /// is then the narrowest band, and the problem is located at
        /// `boundary`.
        at_boundary: bool,
    },
    /// A male Inlay with a drawn boundary has `margin_mm` or
    /// `outline_rounding_mm` set (format 26): the boundary replaces
    /// both, so the value does nothing. A warning, at the field.
    #[error(
        "operation `{name}` (`{operation}`) draws its male's edge with a boundary, so {field} does nothing; the boundary replaces both margin_mm and outline_rounding_mm. Set {field} to null, or empty the boundary list to use it"
    )]
    InlayMarginIgnored {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// `margin_mm` or `outline_rounding_mm`.
        field: &'static str,
    },
    /// An Inlay's `boundary` lists an id that is no instance (format 26).
    /// An error, at the entry.
    #[error(
        "operation `{name}` (`{operation}`) lists boundary[{index}] `{instance}`, which is no instance of the project; list the id of a drawn shape that is in it"
    )]
    InlayBoundaryUnknown {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The entry's place in `boundary`.
        index: usize,
        /// The id listed.
        instance: InstanceId,
    },
    /// An Inlay's boundary instance has a profile that is not a closed
    /// outline (format 26): an open profile is an error, not
    /// skipped. At the entry.
    #[error(
        "operation `{name}` (`{operation}`) lists boundary[{index}] `{instance}`, whose profile {profile} is not a closed outline; a boundary must be closed, so close that profile or list another shape"
    )]
    InlayBoundaryOpen {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The entry's place in `boundary`.
        index: usize,
        /// The instance.
        instance: InstanceId,
        /// The profile's place in its definition.
        profile: usize,
    },
    /// An Inlay's boundary instance is also one of its targets (format 26).
    /// An error, at the entry.
    #[error(
        "operation `{name}` (`{operation}`) lists boundary[{index}] `{instance}`, which is also one of its targets; the design cannot be its own boundary, so draw a separate shape round it"
    )]
    InlayBoundaryIsTarget {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The entry's place in `boundary`.
        index: usize,
        /// The instance.
        instance: InstanceId,
    },
    /// An Inlay's drawn edge touches or crosses its design, or part of
    /// the design lies outside it (format 26): the plug's wall would
    /// have no band beyond it. The point is the worst one, on the design
    /// after near parts are merged. An error, at `boundary`.
    #[error(
        "operation `{name}` (`{operation}`) has a boundary that touches or crosses its design, or leaves part of it outside: the worst point is at ({x_mm:.3}, {y_mm:.3}), {distance_mm:.3} mm past the boundary's edge; the design must lie strictly inside the boundary, so draw the boundary wider there"
    )]
    InlayBoundaryCrossesDesign {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// X of the worst point, mm.
        x_mm: f64,
        /// Y of the worst point, mm.
        y_mm: f64,
        /// How far past the edge it is, mm; 0 for a touch.
        distance_mm: f64,
    },
    /// A boundary instance none of whose shapes holds any part of the
    /// design (format 26). It still counts in the union. A warning,
    /// at the entry.
    #[error(
        "operation `{name}` (`{operation}`) lists boundary[{index}] `{instance}` (`{instance_name}`), but no part of the design is inside it; it still widens the band, so remove it if you did not mean that"
    )]
    InlayBoundaryEmpty {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The entry's place in `boundary`.
        index: usize,
        /// The instance.
        instance: InstanceId,
        /// The instance's display name.
        instance_name: String,
    },
    /// A boundary shape has a hole (format 26): board standing in
    /// the band would hold the plug off the female, so the band
    /// ignores it. A warning, at the entry.
    #[error(
        "operation `{name}` (`{operation}`) lists boundary[{index}] `{instance}` (`{instance_name}`), whose profile {profile} is a hole; the band ignores holes, since board left standing in it would hold the plug off the female, so the hole is cut as band too"
    )]
    InlayBoundaryHoleIgnored {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The entry's place in `boundary`.
        index: usize,
        /// The instance.
        instance: InstanceId,
        /// The instance's display name.
        instance_name: String,
        /// The hole's place among its definition's profiles.
        profile: usize,
    },
    /// A Photo V-Carve's `line_angle_deg` is not in [0, 180) (format 27;
    /// capability 47). An error.
    #[error(
        "operation `{name}` (`{operation}`) has line_angle_deg {line_angle_deg}; the grooves' angle is 0 degrees or more and less than 180"
    )]
    PhotoLineAngleRange {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The angle set, degrees.
        line_angle_deg: f64,
    },
    /// A Photo V-Carve's `line_spacing_fraction` is not above 0 (format
    /// 27). An error.
    #[error(
        "operation `{name}` (`{operation}`) has line_spacing_fraction {line_spacing_fraction}; the spacing of the grooves is a fraction of a groove's width, above 0"
    )]
    PhotoLineSpacingPositive {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The fraction set.
        line_spacing_fraction: f64,
    },
    /// A Photo V-Carve's `line_spacing_fraction` is below 1, so its
    /// grooves overlap at full depth (format 27). A warning.
    #[error(
        "operation `{name}` (`{operation}`) has line_spacing_fraction {line_spacing_fraction}, less than 1, so its grooves overlap where the picture is darkest and no ridge is left between them; use 1 or more to keep the ridges"
    )]
    PhotoLinesOverlap {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The fraction set.
        line_spacing_fraction: f64,
    },
    /// A Photo V-Carve's `sample_mm` is not above 0 (format 27). An
    /// error.
    #[error(
        "operation `{name}` (`{operation}`) has sample_mm {sample_mm} mm; the distance between samples along a groove is above 0"
    )]
    PhotoSamplePositive {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The value set, mm.
        sample_mm: f64,
    },
    /// A Photo V-Carve's `stepover_retract_mm` is not above 0 (format
    /// 27). An error.
    #[error(
        "operation `{name}` (`{operation}`) has stepover_retract_mm {stepover_retract_mm} mm; the lift between grooves is above 0"
    )]
    PhotoRetractPositive {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The value set, mm.
        stepover_retract_mm: f64,
    },
    /// A Photo V-Carve's `image` names no bitmap of the project (format
    /// 27). An error.
    #[error("operation `{operation}` carves bitmap `{bitmap}`, which does not exist")]
    PhotoUnknownBitmap {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The id it names.
        bitmap: BitmapId,
    },
    /// A bitmap's footprint does not have its picture's proportions:
    /// `height_mm / width_mm` is off `height_px / width_px` by more than
    /// 1e-6, so the picture would be stretched (format 27). An error.
    #[error(
        "bitmap `{name}` (`{bitmap}`) is {width_mm} × {height_mm} mm, which is not the shape of its {width_px} × {height_px} px picture; set height_mm to {} mm, or width_mm to {} mm, so the picture is not stretched",
        scaled_mm(*.width_mm, *.height_px, *.width_px),
        scaled_mm(*.height_mm, *.width_px, *.height_px)
    )]
    BitmapFootprintAspect {
        /// The bitmap.
        bitmap: BitmapId,
        /// Its display name.
        name: String,
        /// The footprint's width, mm.
        width_mm: f64,
        /// The footprint's height, mm.
        height_mm: f64,
        /// The picture's width, pixels.
        width_px: u32,
        /// The picture's height, pixels.
        height_px: u32,
    },
    /// A bitmap's `width_px` × `height_px` is not its PNG's size, read
    /// from the PNG's header (format 27). An error.
    #[error(
        "bitmap `{name}` (`{bitmap}`) says it is {width_px} × {height_px} px, but its PNG is {png_width_px} × {png_height_px} px; set width_px and height_px to the PNG's size"
    )]
    BitmapPixelsMismatch {
        /// The bitmap.
        bitmap: BitmapId,
        /// Its display name.
        name: String,
        /// The bitmap's `width_px`.
        width_px: u32,
        /// The bitmap's `height_px`.
        height_px: u32,
        /// The PNG's width, pixels.
        png_width_px: u32,
        /// The PNG's height, pixels.
        png_height_px: u32,
    },
    /// A bitmap's PNG is not 8-bit grayscale, read from its header
    /// (format 27): DreamCarve wrote it, so a hand edit is the only way.
    /// An error.
    #[error(
        "the PNG of bitmap `{name}` (`{bitmap}`) is {bit_depth}-bit {colour}, not 8-bit grayscale; import the picture again so that DreamCarve converts it"
    )]
    BitmapNotGrayscale8 {
        /// The bitmap.
        bitmap: BitmapId,
        /// Its display name.
        name: String,
        /// The PNG's colour type.
        colour: BitmapColour,
        /// The PNG's bits per sample.
        bit_depth: u8,
    },
    /// Two bitmaps share an id (format 27). An error.
    #[error("bitmap id `{id}` is used twice")]
    DuplicateBitmapId {
        /// The id used twice.
        id: BitmapId,
    },
    /// A bitmap's `width_px` or `height_px` is 0 (format 27). An error.
    #[error(
        "bitmap `{name}` (`{bitmap}`) is {width_px} × {height_px} px; a picture has at least one pixel each way"
    )]
    BitmapEmpty {
        /// The bitmap.
        bitmap: BitmapId,
        /// Its display name.
        name: String,
        /// The bitmap's `width_px`.
        width_px: u32,
        /// The bitmap's `height_px`.
        height_px: u32,
    },
    /// A bitmap's `width_px` or `height_px` is above
    /// [`MAX_BITMAP_SIDE_PX`] (format 27). An
    /// error.
    #[error(
        "bitmap `{name}` (`{bitmap}`) is {width_px} × {height_px} px; a picture is at most {limit_px} px on a side, so scale it down before adding it"
    )]
    BitmapTooLarge {
        /// The bitmap.
        bitmap: BitmapId,
        /// Its display name.
        name: String,
        /// The bitmap's `width_px`.
        width_px: u32,
        /// The bitmap's `height_px`.
        height_px: u32,
        /// [`MAX_BITMAP_SIDE_PX`].
        limit_px: u32,
    },
    /// A bitmap's `width_mm` or `height_mm` is not above 0 (format 27). An
    /// error.
    #[error(
        "bitmap `{name}` (`{bitmap}`) has {field} {value} mm; a picture's footprint has a width and a height above 0"
    )]
    BitmapFootprintPositive {
        /// The bitmap.
        bitmap: BitmapId,
        /// Its display name.
        name: String,
        /// The field at fault: `width_mm` or `height_mm`.
        field: &'static str,
        /// The value set, mm.
        value: f64,
    },
    /// A bitmap's `png_base64` is not base64 or does not start with a PNG
    /// header (format 27). [`crate::load`] refuses such a file, so
    /// only a project built in code gets here. An error.
    #[error("the picture of bitmap `{name}` (`{bitmap}`) cannot be read: {reason}")]
    BitmapUnreadable {
        /// The bitmap.
        bitmap: BitmapId,
        /// Its display name.
        name: String,
        /// What is wrong with the picture.
        reason: BitmapError,
    },
    /// A stepover in effect puts a used tool's or an enabled operation's
    /// passes closer together than [`MIN_PASS_SPACING_MM`]. A fraction
    /// that small is nearly always a slip, a percentage typed as a
    /// fraction (0.08 % where 8 % was meant), and its program cuts the
    /// same wood for hours. A warning: the program is still what was
    /// asked for.
    #[error("{}", step_over_too_fine(.passes, *.fraction, *.width_mm))]
    StepOverTooFine {
        /// Where the fraction in effect is set: a tool's or an
        /// operation's `step_over_fraction`, or a Photo V-Carve's
        /// `line_spacing_fraction`.
        field: String,
        /// Whose passes they are, and what the fraction is of.
        passes: FinePasses,
        /// The fraction.
        fraction: f64,
        /// What the generator multiplies the fraction by to space the
        /// passes, mm.
        width_mm: f64,
    },
}

/// Whose passes [`Problem::StepOverTooFine`] is about, and what their
/// stepover is a fraction of: each generator multiplies it by a width of
/// its own.
#[derive(Debug, Clone, PartialEq)]
pub enum FinePasses {
    /// A used tool's own `step_over_fraction`, of its diameter: the
    /// spacing of a Pocket's or a clearing pass's passes that take it,
    /// and the most any pass that does can be apart.
    Tool {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
    },
    /// An enabled Pocket's own `step_over_fraction`, of its tool's
    /// diameter.
    Pocket {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
        /// The tool's display name.
        tool_name: String,
    },
    /// The floor passes of an enabled V-Carve or Inlay: the stepover in
    /// effect, the operation's own or else its tool's, is a fraction of
    /// the width the bit cuts at the flat depth, which is narrower than
    /// its diameter.
    Floor {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
        /// The flat depth below the start plane, mm: a V-Carve's
        /// `flat_depth_mm`, or the shallower of an Inlay's `depth_mm` and,
        /// once the male is cut, `plug_depth_mm`.
        depth_mm: f64,
        /// The tool's display name when the fraction is the tool's;
        /// `None` when it is the operation's own.
        from_tool: Option<String>,
    },
    /// The grooves of an enabled Photo V-Carve: its
    /// `line_spacing_fraction` is of a groove's width at `max_depth_mm`.
    Grooves {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
        /// The Photo V-Carve's `max_depth_mm`.
        max_depth_mm: f64,
    },
}

/// The least distance between neighbouring passes that validation takes
/// as meant, mm: a stepover that puts them closer is `step_over_too_fine`.
/// It bounds the distance, not the fraction, because each generator
/// multiplies the fraction by a width of its own (a Pocket by the tool's
/// diameter, a carve's floor by the bit's width at the flat depth).
///
/// 0.02 mm is no more than a hobby machine can place a cut to: a
/// belt-driven axis steps 0.0125 mm, and a screw-driven one, which steps
/// finer, still repeats to a few hundredths of a millimetre at best, as
/// the wood does. It is a third of the finest stepover found in a tool
/// vendor's published library (0.061 mm, the finishing stepover of a
/// 0.8 mm tapered ball nose), so no value a vendor means is caught, and
/// four times the slip it was written for (8 % of a 6.35 mm bit typed as
/// 0.08 %: 0.005 mm between passes where 0.508 mm was meant).
pub const MIN_PASS_SPACING_MM: f64 = 0.02;

/// Whether `fraction` of `width_mm` puts passes closer than
/// [`MIN_PASS_SPACING_MM`]. `false` for a fraction or a width out of
/// range, which has an error of its own.
fn passes_too_fine(fraction: f64, width_mm: f64) -> bool {
    fraction.is_finite()
        && fraction > 0.0
        && fraction <= 1.0
        && width_mm.is_finite()
        && width_mm > 0.0
        && fraction * width_mm < MIN_PASS_SPACING_MM
}

/// `value` to at most `places` decimals, without trailing zeros: `6.35`,
/// `0.08`, `8`.
fn trimmed(value: f64, places: usize) -> String {
    let text = format!("{value:.places$}");
    if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    }
}

/// [`Problem::StepOverTooFine`]'s message: whose stepover, as a
/// percentage of what, the distance it leaves between passes, and, when
/// a hundred times the fraction is a stepover that cuts, what that would
/// be: the percentage the user most likely meant. A floor that takes its
/// tool's fraction gets no such guess, because that fraction is fine of
/// the tool's diameter and so is no slip: the floor is narrower, and the
/// operation needs a fraction of its own.
fn step_over_too_fine(passes: &FinePasses, fraction: f64, width_mm: f64) -> String {
    let percent = trimmed(fraction * 100.0, 4);
    let width = trimmed(width_mm, 3);
    let (what, between, own) = match passes {
        FinePasses::Tool { tool, name } => (
            format!(
                "tool `{name}` (`{tool}`) has a stepover of {percent} % of its {width} mm diameter"
            ),
            "passes",
            true,
        ),
        FinePasses::Pocket {
            operation,
            name,
            tool_name,
        } => (
            format!(
                "operation `{name}` (`{operation}`) has a stepover of {percent} % of the {width} mm diameter of tool `{tool_name}`"
            ),
            "passes",
            true,
        ),
        FinePasses::Floor {
            operation,
            name,
            depth_mm,
            from_tool,
        } => {
            let depth = trimmed(*depth_mm, 3);
            let whose = match from_tool {
                Some(tool_name) => format!("takes tool `{tool_name}`'s stepover for its floor,"),
                None => "has a floor stepover of".to_owned(),
            };
            (
                format!(
                    "operation `{name}` (`{operation}`) {whose} {percent} % of the {width} mm its bit cuts at the flat depth, {depth} mm"
                ),
                "passes",
                from_tool.is_none(),
            )
        }
        FinePasses::Grooves {
            operation,
            name,
            max_depth_mm,
        } => {
            let depth = trimmed(*max_depth_mm, 3);
            (
                format!(
                    "operation `{name}` (`{operation}`) has a line spacing of {percent} % of the {width} mm a groove is wide at its max_depth_mm, {depth} mm"
                ),
                "grooves",
                true,
            )
        }
    };
    let spacing_mm = fraction * width_mm;
    let spacing = if spacing_mm < 0.0005 {
        "under 0.001".to_owned()
    } else {
        format!("{spacing_mm:.3}")
    };
    let meant = fraction * 100.0;
    let advice = if !own {
        "; give the operation a step_over_fraction of its own".to_owned()
    } else if meant <= 1.0 && meant * width_mm >= MIN_PASS_SPACING_MM {
        format!(
            "; {} % is {:.3} mm",
            trimmed(meant * 100.0, 4),
            meant * width_mm
        )
    } else {
        String::new()
    };
    format!("{what}: {spacing} mm between {between}, too fine to cut{advice}")
}

/// The fastest feed or plunge rate a used tool may have, mm/min.
/// Above it export fails: no hobby machine moves that fast, so the value is
/// almost surely a typo. Raise it if users report real machines beyond it.
pub const MAX_RATE_MM_MIN: f64 = 20_000.0;

/// Above this spindle speed a used tool gets a warning, RPM.
pub const MAX_RPM: u32 = 60_000;

/// Above this diameter a used tool gets a warning, mm.
pub const MAX_TOOL_DIAMETER_MM: f64 = 50.0;

/// The thinnest board a male Inlay may leave under its surround's floor
/// without a warning, mm (format 22; `docs/specs/phase-3/design.md`,
/// "Algorithms → Inlay", step 4): `inlay_plug_base_thin` below it.
pub const INLAY_MIN_PLUG_BASE_MM: f64 = 6.0;

/// The coarsest `flatten_tolerance_mm` a project may set, mm (format 20):
/// the safety checks' slack and the medial axis's chords scale with it.
pub const MAX_FLATTEN_TOLERANCE_MM: f64 = 0.1;

/// `name`, a variant's CamelCase name, in snake case, and how many bytes
/// of the array that takes: a `_` goes before each capital but the first,
/// and before a digit that follows a lowercase letter (`Grayscale8` is
/// `grayscale_8`, `M6` is `m6`). `N` is twice the name's length, which
/// always holds it.
const fn snake_case<const N: usize>(name: &str) -> ([u8; N], usize) {
    let mut out = [0; N];
    let mut free: &mut [u8] = &mut out;
    let mut bytes = name.as_bytes();
    let mut previous = b'_';
    while let [byte, rest @ ..] = bytes {
        let starts_word =
            byte.is_ascii_uppercase() || (byte.is_ascii_digit() && previous.is_ascii_lowercase());
        if starts_word
            && previous != b'_'
            && let [slot, tail @ ..] = free
        {
            *slot = b'_';
            free = tail;
        }
        if let [slot, tail @ ..] = free {
            *slot = byte.to_ascii_lowercase();
            free = tail;
        }
        previous = *byte;
        bytes = rest;
    }
    let len = N - free.len();
    (out, len)
}

/// The first `len` bytes of `snake` as text. Falls back to `name` were
/// they not UTF-8, which [`snake_case`] of an ASCII name always is.
const fn code_text(snake: &'static [u8], len: usize, name: &'static str) -> &'static str {
    match snake.split_at_checked(len) {
        Some((code, _)) => match core::str::from_utf8(code) {
            Ok(code) => code,
            Err(_) => name,
        },
        None => name,
    }
}

/// A variant's name in snake case, worked out when the crate is compiled.
macro_rules! variant_code {
    ($variant:ident) => {{
        const NAME: &str = stringify!($variant);
        const SNAKE: ([u8; 2 * NAME.len()], usize) = snake_case(NAME);
        code_text(&SNAKE.0, SNAKE.1, NAME)
    }};
}

/// Writes [`Problem::code`] from the variants' names. The match has no
/// wildcard, so a variant missing from the list does not compile.
macro_rules! problem_codes {
    ($($variant:ident),+ $(,)?) => {
        impl Problem {
            /// The stable `snake_case` code of the problem, the variant's name:
            /// what `check` and `export` print before a warning's message
            /// (phase 2 functional spec, capability 25).
            #[must_use]
            pub const fn code(&self) -> &'static str {
                match self {
                    $(Self::$variant { .. } => variant_code!($variant),)+
                }
            }

            /// Every code, in the order the variants are declared.
            #[cfg(test)]
            pub(crate) const CODES: &[&str] = &[$(variant_code!($variant)),+];
        }
    };
}

problem_codes! {
    DefinitionKeyMismatch, DuplicateInstanceId, DuplicateToolId,
    DuplicateOperationId, DanglingDefinition, DanglingTool,
    DanglingInstance, DuplicateTabId, TabDanglingInstance,
    TabProfileMissing, OutOfRange, NonFinite, OperationDisabled,
    ToolValueUnset, UnusedTool, StockThicknessUnset, OperationValueUnset,
    ToolOutOfRange, OpenProfile, UnsafeFileExtension, ToolChangeModeNeedsM6,
    StepDownAboveDiameter, CutterNotFlat, OperationHasNoTargets,
    SpindleSpeedHigh, PlungeAboveFeed, LargeDiameter, DuplicateToolNumber,
    RestOfInvalid, HelixNeedsWasteSide, CannedCyclesUnsupported,
    DrillCutterUnusual, ReliefNeedsStepDown, ClearanceBelowSafeZ,
    MachineZNotBelowHome, MachineZNeedsG53, EngraveDeeperThanCone,
    LastPassStockToLeave, LastPassNeedsSide, LastPassThickerThanStepDown,
    BorePitchOverStepDown, FlattenToleranceRange, VCarveNeedsVBit,
    TaperedBallNoseRange, VBitAngleRange, VBitConeExceeded,
    ClearingNeedsFlatDepth, ClearingNeedsEndMill, ClearingDanglingTool,
    ClearingNotDescending, StartDepthNegative, StartDepthUncleared,
    CountersinkNeedsVBit, CountersinkDiameterPositive,
    CountersinkMouthExceedsTool, CountersinkIgnoresToolWidth,
    InlayGlueGapNotBelowDepth, InlayMarginPositive, InlayOutlineRounding,
    InlayPlugBaseThin, InlayGlueGapBelowBall, InlayPlugDepthBelowBall,
    InlayMaleBoardSize, InlayMaleBoardThickness, InlayMaleBoardsDiffer,
    InlayCutOutNoRoom, InlayMarginIgnored, InlayBoundaryUnknown,
    InlayBoundaryOpen, InlayBoundaryIsTarget, InlayBoundaryCrossesDesign,
    InlayBoundaryEmpty, InlayBoundaryHoleIgnored, PhotoLineAngleRange,
    PhotoLineSpacingPositive, PhotoLinesOverlap, PhotoSamplePositive,
    PhotoRetractPositive, PhotoUnknownBitmap, BitmapFootprintAspect,
    BitmapPixelsMismatch, BitmapNotGrayscale8, DuplicateBitmapId,
    BitmapEmpty, BitmapTooLarge, BitmapFootprintPositive, BitmapUnreadable,
    StepOverTooFine,
}

impl Problem {
    /// Whether this problem is an error or a warning. The warnings are
    /// listed; every other problem is an error, and so is a new variant
    /// until it is listed.
    pub fn severity(&self) -> Severity {
        let warning = matches!(
            self,
            Self::OperationDisabled { .. }
                | Self::UnusedTool { .. }
                | Self::OpenProfile { .. }
                | Self::StepDownAboveDiameter { .. }
                | Self::SpindleSpeedHigh { .. }
                | Self::PlungeAboveFeed { .. }
                | Self::LargeDiameter { .. }
                | Self::DrillCutterUnusual { .. }
                | Self::ReliefNeedsStepDown { .. }
                | Self::EngraveDeeperThanCone { .. }
                | Self::ClearingNotDescending { .. }
                | Self::CountersinkIgnoresToolWidth { .. }
                | Self::InlayPlugBaseThin { .. }
                | Self::InlayGlueGapBelowBall { .. }
                | Self::InlayPlugDepthBelowBall { .. }
                | Self::InlayMarginIgnored { .. }
                | Self::InlayBoundaryEmpty { .. }
                | Self::InlayBoundaryHoleIgnored { .. }
                | Self::PhotoLinesOverlap { .. }
                | Self::StepOverTooFine { .. }
        );
        if warning {
            Severity::Warning
        } else {
            Severity::Error
        }
    }

    /// Whether this problem is error-class.
    pub fn is_error(&self) -> bool {
        self.severity() == Severity::Error
    }

    /// Whether this problem is a value the user must supply and has not:
    /// the list `init` prints.
    pub fn is_unset_value(&self) -> bool {
        matches!(
            self,
            Self::ToolValueUnset { .. }
                | Self::StockThicknessUnset
                | Self::OperationValueUnset { .. }
        )
    }

    /// The field this problem is about, written as the messages write
    /// fields (`stock.thickness_mm`, `tools[t1].feed_mm_min`), for
    /// [`crate::SourceMap::locate`]. `None` when no one value is at fault,
    /// as with an id used twice.
    #[must_use]
    pub fn field(&self) -> Option<String> {
        match self {
            Self::DefinitionKeyMismatch { key, .. } => Some(format!("definitions[{key}].id")),
            Self::DuplicateInstanceId { .. }
            | Self::DuplicateToolId { .. }
            | Self::DuplicateOperationId { .. } => None,
            Self::DanglingDefinition { instance, .. } => {
                Some(format!("instances[{instance}].definition"))
            }
            Self::DanglingTool { operation, .. } => Some(format!("operations[{operation}].tool")),
            Self::DanglingInstance { operation, .. } => {
                Some(format!("operations[{operation}].targets"))
            }
            Self::DuplicateTabId { .. } => None,
            Self::TabDanglingInstance { tab, .. } => Some(format!("tabs[{tab}].instance")),
            Self::TabProfileMissing { tab, .. } => Some(format!("tabs[{tab}].profile")),

            Self::OutOfRange { field, .. } | Self::NonFinite { field } => Some(field.clone()),
            Self::OperationDisabled { operation } => {
                Some(format!("operations[{operation}].enabled"))
            }
            Self::ToolValueUnset { tool, field, .. }
            | Self::ToolOutOfRange { tool, field, .. }
            | Self::TaperedBallNoseRange { tool, field, .. } => {
                Some(format!("tools[{tool}].{field}"))
            }
            Self::UnusedTool { tool, .. } => Some(format!("tools[{tool}]")),
            Self::StockThicknessUnset => Some(STOCK_THICKNESS.to_owned()),
            Self::OperationValueUnset {
                operation, field, ..
            } => Some(format!("operations[{operation}].{field}")),
            Self::OpenProfile {
                definition,
                profile,
                ..
            } => Some(format!("definitions[{definition}].profiles[{profile}]")),
            Self::UnsafeFileExtension { .. } => Some("post_processor.file_extension".to_owned()),
            Self::ToolChangeModeNeedsM6 { .. } => {
                Some("post_processor.tool_change_mode".to_owned())
            }
            Self::StepDownAboveDiameter { field, .. } | Self::StepOverTooFine { field, .. } => {
                Some(field.clone())
            }
            Self::CutterNotFlat {
                operation, field, ..
            } => Some(format!("operations[{operation}].{field}")),
            Self::OperationHasNoTargets { operation, .. } => {
                Some(format!("operations[{operation}].targets"))
            }
            Self::SpindleSpeedHigh { tool, .. } => Some(format!("tools[{tool}].rpm")),
            Self::PlungeAboveFeed { tool, .. } => Some(format!("tools[{tool}].plunge_mm_min")),
            Self::LargeDiameter { tool, .. } => Some(format!("tools[{tool}].diameter_mm")),
            Self::DuplicateToolNumber { tool, .. } => Some(format!("tools[{tool}].number")),
            Self::RestOfInvalid { operation, .. } => {
                Some(format!("operations[{operation}].rest_of"))
            }
            Self::HelixNeedsWasteSide { operation, .. } => {
                Some(format!("operations[{operation}].entry"))
            }
            Self::CannedCyclesUnsupported { .. } => {
                Some("post_processor.use_canned_cycles".to_owned())
            }
            Self::DrillCutterUnusual { operation, .. } => {
                Some(format!("operations[{operation}].tool"))
            }
            Self::ReliefNeedsStepDown { operation, .. } => {
                Some(format!("operations[{operation}].corner_relief"))
            }
            Self::EngraveDeeperThanCone { operation, .. } => {
                Some(format!("operations[{operation}].depth_mm"))
            }
            Self::LastPassStockToLeave { operation, .. } => Some(format!(
                "operations[{operation}].last_pass.stock_to_leave_mm"
            )),
            Self::LastPassNeedsSide { operation, .. } => {
                Some(format!("operations[{operation}].last_pass"))
            }
            Self::LastPassThickerThanStepDown { operation, .. } => {
                Some(format!("operations[{operation}].last_pass.thickness_mm"))
            }
            Self::BorePitchOverStepDown { operation, .. } => {
                Some(format!("operations[{operation}].pitch_mm"))
            }
            Self::FlattenToleranceRange { .. } => Some(FLATTEN_TOLERANCE.to_owned()),
            Self::VCarveNeedsVBit { operation, .. } => {
                Some(format!("operations[{operation}].tool"))
            }
            Self::VBitAngleRange { tool, .. } => {
                Some(format!("tools[{tool}].cutter.v_bit.included_angle_deg"))
            }
            Self::VBitConeExceeded {
                operation, field, ..
            } => Some(format!("operations[{operation}].{field}")),
            Self::ClearingNeedsFlatDepth { operation, .. } => {
                Some(format!("operations[{operation}].clearing"))
            }
            Self::ClearingNeedsEndMill {
                operation, pass, ..
            }
            | Self::ClearingDanglingTool {
                operation, pass, ..
            }
            | Self::ClearingNotDescending {
                operation, pass, ..
            } => Some(format!("operations[{operation}].clearing[{pass}].tool")),
            Self::StartDepthNegative {
                operation, field, ..
            }
            | Self::StartDepthUncleared {
                operation, field, ..
            } => Some(format!("operations[{operation}].{field}")),
            Self::CountersinkNeedsVBit { operation, .. } => {
                Some(format!("operations[{operation}].tool"))
            }
            Self::CountersinkDiameterPositive { operation, .. }
            | Self::CountersinkMouthExceedsTool { operation, .. } => {
                Some(format!("operations[{operation}].{COUNTERSINK_DIAMETER}"))
            }
            Self::CountersinkIgnoresToolWidth { operation, .. } => Some(format!(
                "operations[{operation}].allow_tool_wider_than_hole"
            )),
            Self::InlayGlueGapNotBelowDepth { operation, .. } => {
                Some(format!("operations[{operation}].glue_gap_mm"))
            }
            Self::InlayMarginPositive { operation, .. } => {
                Some(format!("operations[{operation}].margin_mm"))
            }
            Self::InlayOutlineRounding { operation, .. } => {
                Some(format!("operations[{operation}].outline_rounding_mm"))
            }
            Self::InlayPlugBaseThin { operation, .. } => {
                Some(format!("operations[{operation}].{INLAY_PLUG_DEPTH}"))
            }
            Self::InlayGlueGapBelowBall { operation, .. } => {
                Some(format!("operations[{operation}].glue_gap_mm"))
            }
            Self::InlayPlugDepthBelowBall { operation, .. } => {
                Some(format!("operations[{operation}].{INLAY_PLUG_DEPTH}"))
            }
            Self::InlayMaleBoardSize {
                operation, field, ..
            } => Some(format!("operations[{operation}].male_board.{field}")),
            Self::InlayMaleBoardThickness { operation, .. } => {
                Some(format!("operations[{operation}].male_board.thickness_mm"))
            }
            Self::InlayMaleBoardsDiffer { operation, .. } => {
                Some(format!("operations[{operation}].male_board"))
            }
            Self::InlayCutOutNoRoom {
                operation,
                at_boundary,
                ..
            } => Some(if *at_boundary {
                format!("operations[{operation}].boundary")
            } else {
                format!("operations[{operation}].margin_mm")
            }),
            Self::InlayMarginIgnored {
                operation, field, ..
            } => Some(format!("operations[{operation}].{field}")),
            Self::InlayBoundaryUnknown {
                operation, index, ..
            }
            | Self::InlayBoundaryOpen {
                operation, index, ..
            }
            | Self::InlayBoundaryIsTarget {
                operation, index, ..
            }
            | Self::InlayBoundaryEmpty {
                operation, index, ..
            }
            | Self::InlayBoundaryHoleIgnored {
                operation, index, ..
            } => Some(format!("operations[{operation}].boundary[{index}]")),
            Self::InlayBoundaryCrossesDesign { operation, .. } => {
                Some(format!("operations[{operation}].boundary"))
            }
            Self::PhotoLineAngleRange { operation, .. } => {
                Some(format!("operations[{operation}].line_angle_deg"))
            }
            Self::PhotoLineSpacingPositive { operation, .. }
            | Self::PhotoLinesOverlap { operation, .. } => {
                Some(format!("operations[{operation}].line_spacing_fraction"))
            }
            Self::PhotoSamplePositive { operation, .. } => {
                Some(format!("operations[{operation}].sample_mm"))
            }
            Self::PhotoRetractPositive { operation, .. } => {
                Some(format!("operations[{operation}].stepover_retract_mm"))
            }
            Self::PhotoUnknownBitmap { operation, .. } => {
                Some(format!("operations[{operation}].image"))
            }
            Self::BitmapFootprintAspect { bitmap, .. } => {
                Some(format!("bitmaps[{bitmap}].height_mm"))
            }
            Self::BitmapPixelsMismatch { bitmap, .. } => {
                Some(format!("bitmaps[{bitmap}].width_px"))
            }
            Self::BitmapNotGrayscale8 { bitmap, .. } | Self::BitmapUnreadable { bitmap, .. } => {
                Some(format!("bitmaps[{bitmap}].png_base64"))
            }
            Self::DuplicateBitmapId { .. } => None,
            // At the side at fault, the width when both are.
            Self::BitmapEmpty {
                bitmap, width_px, ..
            } => Some(if *width_px == 0 {
                format!("bitmaps[{bitmap}].width_px")
            } else {
                format!("bitmaps[{bitmap}].height_px")
            }),
            Self::BitmapTooLarge {
                bitmap,
                width_px,
                limit_px,
                ..
            } => Some(if width_px > limit_px {
                format!("bitmaps[{bitmap}].width_px")
            } else {
                format!("bitmaps[{bitmap}].height_px")
            }),
            Self::BitmapFootprintPositive { bitmap, field, .. } => {
                Some(format!("bitmaps[{bitmap}].{field}"))
            }
            Self::ClearanceBelowSafeZ { .. } => Some(CLEARANCE.to_owned()),
            Self::MachineZNotBelowHome { .. } | Self::MachineZNeedsG53 { .. } => {
                Some(MACHINE_Z.to_owned())
            }
        }
    }
}

const STOCK_THICKNESS: &str = "stock.thickness_mm";
/// A countersink's mouth, under its operation (format 21).
pub(crate) const COUNTERSINK_DIAMETER: &str = "cycle.countersink.diameter_mm";
/// A countersink's start depth, under its operation (format 21).
pub(crate) const COUNTERSINK_START_DEPTH: &str = "cycle.countersink.start_depth_mm";
/// A male Inlay's plug depth, under its operation (format 22), where its
/// `v_bit_cone_exceeded` and `inlay_plug_base_thin` are located.
const INLAY_PLUG_DEPTH: &str = "plug_depth_mm";
const FLATTEN_TOLERANCE: &str = "flatten_tolerance_mm";
const CLEARANCE: &str = "heights.clearance_mm";
const MACHINE_Z: &str = "heights.tool_change_machine_z_mm";

/// The unit of a value the user must supply, for "is not set" messages.
fn unset_unit(field: &str) -> &'static str {
    match field {
        "feed_mm_min" | "plunge_mm_min" => "mm/min",
        "rpm" => "RPM",
        crate::tool::TBN_TAPER => "degrees per side",
        _ => "mm",
    }
}

/// How [`Problem::CutterNotFlat`] names the cut and says why it needs a
/// flat end mill: a male Inlay's cut-out, or else an offset Profile or a
/// Pocket.
fn flat_cut(field: &str) -> (&'static str, &'static str) {
    if field == "tool" {
        (
            "cuts with",
            "an Inside, Outside, Left or Right Profile and a Pocket offset the path by half the diameter, which only a flat end mill cuts at every depth, so the part would come out the wrong size",
        )
    } else {
        (
            "cuts its plug's cut-out with",
            "the cut-out runs through the board in the band's flat floor, which only a flat end mill cuts with a straight wall at every depth",
        )
    }
}

/// Where a Profile cuts, as [`Problem::LastPassNeedsSide`] says it.
const fn side_cut(side: Side) -> &'static str {
    match side {
        Side::OnLine => "on the line",
        Side::Left => "to the left of open paths",
        Side::Right => "to the right of open paths",
        Side::Inside => "inside",
        Side::Outside => "outside",
    }
}

/// What [`Problem::VCarveNeedsVBit`] tells the user to cut with instead of
/// `cutter`.
fn carving_bit_advice(cutter: &str, photo: bool) -> &'static str {
    let tapered = cutter == "a tapered ball nose";
    match (tapered, photo) {
        (true, true) => "a Photo V-Carve needs a V-bit; a tapered ball nose is not supported there",
        (false, true) => "a Photo V-Carve's depth follows the angle of a V-bit, so use a V-bit",
        (_, false) => {
            "a carve's depth follows the bit's profile, so it needs a V-bit or a tapered ball nose; use a V-bit or a tapered ball nose"
        }
    }
}

/// The rest of [`Problem::VBitConeExceeded`]'s message after the tool's
/// name: how deep the bit engages, from where, and the height it passes.
fn cone_exceeded(
    engagement_mm: f64,
    cone_height_mm: f64,
    from_stock_top: bool,
    field: &str,
    limit: &CuttingLimit,
) -> String {
    let from_top = if from_stock_top {
        "from the stock top, "
    } else {
        ""
    };
    match limit {
        CuttingLimit::CuttingDiameter { diameter_mm } => format!(
            ", a tapered ball nose, {from_top}engaged {engagement_mm:.3} mm, past {cone_height_mm:.3} mm, where it reaches its {diameter_mm:.3} mm cutting diameter; above that the bit is a cylinder, whose walls follow no carve, so use a shallower depth"
        ),
        CuttingLimit::Flutes => format!(
            ", a tapered ball nose, {from_top}engaged {engagement_mm:.3} mm, past the bit's {cone_height_mm:.3} mm flutes; above them the shank rubs, so use a shallower depth or a bit with longer flutes"
        ),
        CuttingLimit::Cone => {
            let counted_from = if !from_stock_top {
                String::new()
            } else if field == INLAY_PLUG_DEPTH {
                " from the stock top (its start plane, depth_mm − glue_gap_mm below the stock top, plus its plug_depth_mm; the plug above the start plane is never cleared)".to_owned()
            } else {
                let own = match field {
                    COUNTERSINK_DIAMETER => "its countersink's depth",
                    "max_depth_mm" => "its max_depth_mm",
                    _ => "its flat depth",
                };
                format!(
                    " from the stock top (its start depth, which no Pocket or V-Carve floor of the project clears, plus {own})"
                )
            };
            format!(
                " {engagement_mm} mm deep{counted_from}, but the bit's cone is only {cone_height_mm:.3} mm tall; below that the shank cuts, so use a shallower depth or a wider bit"
            )
        }
    }
}

/// Why [`Problem::InlayCutOutNoRoom`]'s floor is too narrow and what to
/// widen: the drawn boundary, or else the margin.
fn cut_out_room(margin_mm: f64, needed_mm: f64, at_boundary: bool) -> String {
    if at_boundary {
        format!(
            "the drawn boundary is only {margin_mm:.3} mm from the design at its narrowest, so that floor is narrower than the tool and the cut-out would cut into the plug's wall. Draw the boundary at least {needed_mm:.3} mm from the design all round"
        )
    } else {
        format!(
            "with margin_mm {margin_mm} mm that floor is narrower than the tool, so the cut-out would cut into the plug's wall. Raise margin_mm to at least {needed_mm:.3} mm"
        )
    }
}

/// A bitmap's side that fits `side_mm` of the other one, for a picture
/// `to_px` along this side and `from_px` along the other
/// ([`Problem::BitmapFootprintAspect`]).
fn scaled_mm(side_mm: f64, to_px: u32, from_px: u32) -> f64 {
    side_mm * f64::from(to_px) / f64::from(from_px)
}

/// Checks `project` and returns every problem found, errors and warnings,
/// in a deterministic order: project fields in declaration order, which is
/// the order a saved file holds them.
#[must_use]
pub fn validate(project: &Project) -> Vec<Problem> {
    let mut v = Validator::default();
    v.project(project);
    v.problems
}

const POSITIVE: &str = "a value greater than 0";
const NON_NEGATIVE: &str = "a value of 0 or more";
const FRACTION: &str = "a value greater than 0 and at most 1";
const ANGLE: &str = "an angle greater than 0 and less than 180 degrees";
const SWEEP: &str = "an angle greater than 0 and at most 180 degrees";
const ENTRY_ANGLE: &str = "an angle greater than 0 and at most 45 degrees";
const RASTER_ANGLE: &str = "an angle of 0 or more and less than 180 degrees";
const BELOW_Z_TOP: &str = "a value at most the instance's z_top_mm";
const LAST_PASS_THICKNESS: &str = "a value greater than 0 and at most the operation's depth_mm";
// The limits below spell out `MAX_COORDINATE_MM`; a test checks they agree.
const STOCK_SIDE: &str = "a value greater than 0 and at most 6000 mm";
const COORDINATE: &str = "a value from -6000 to 6000 mm";
const COEFFICIENT: &str = "a value from -6000 to 6000";
const REACH: &str = "a shape that stays within 6000 mm of the origin";
const BITMAP_REACH: &str = "a footprint that stays within 6000 mm of the origin";
/// How far a bitmap's `height_mm / width_mm` may be from its `height_px /
/// width_px` (format 27): a ratio, so no unit. Wide enough for a
/// footprint written to six decimals, far below one pixel of stretch.
const BITMAP_ASPECT_TOLERANCE: f64 = 1e-6;
// Spells out `MAX_RATE_MM_MIN`; a test checks they agree.
const RATE: &str = "a value greater than 0 and at most 20000 mm/min";

#[derive(Default)]
struct Validator {
    problems: Vec<Problem>,
}

impl Validator {
    fn project(&mut self, p: &Project) {
        self.positive(FLATTEN_TOLERANCE, p.flatten_tolerance_mm);
        // Format 20: the safety checks' slack scales
        // with the tolerance, so it has a ceiling.
        if p.flatten_tolerance_mm.is_finite() && p.flatten_tolerance_mm > MAX_FLATTEN_TOLERANCE_MM {
            self.problems.push(Problem::FlattenToleranceRange {
                flatten_tolerance_mm: p.flatten_tolerance_mm,
            });
        }
        self.stock_side("stock.width_mm", p.stock.width_mm);
        self.stock_side("stock.height_mm", p.stock.height_mm);
        match p.stock.thickness_mm {
            Some(thickness_mm) => self.stock_side("stock.thickness_mm", thickness_mm),
            None => self.problems.push(Problem::StockThicknessUnset),
        }
        if let OriginXy::Custom { x_mm, y_mm } = p.origin.xy {
            self.finite("origin.xy.custom.x_mm", x_mm);
            self.finite("origin.xy.custom.y_mm", y_mm);
        }
        self.heights(p);

        // In the order the file holds them: settings, then geometry.
        // Tools an enabled operation cuts with; only their rates are required.
        // A V-Carve's clearing passes cut with their own tools (format 20).
        let clearing_tools = || {
            p.operations
                .iter()
                .filter_map(|op| match op {
                    Operation::VCarve(carve) if carve.enabled => Some(&carve.clearing),
                    Operation::Inlay(inlay) if inlay.enabled => Some(&inlay.clearing),
                    _ => None,
                })
                .flatten()
                .map(|pass| &pass.tool)
                // A male Inlay's cut-out (format 23) too: it steps down
                // by its tool's step-down.
                .chain(p.operations.iter().filter_map(|op| match op {
                    Operation::Inlay(inlay) if inlay.enabled => {
                        inlay.cut_out.as_ref().map(|c| &c.tool)
                    }
                    _ => None,
                }))
        };
        let used: BTreeSet<&ToolId> = p
            .operations
            .iter()
            .filter(|op| op.enabled())
            .map(Operation::tool)
            .chain(clearing_tools())
            .collect();
        // V-bits an enabled V-Carve carves with (format 20): their angle
        // out of range is `v_bit_angle_range`.
        // So are those of an enabled countersink (format 21).
        let v_carve_tools: BTreeSet<&ToolId> = p
            .operations
            .iter()
            .filter_map(|op| match op {
                Operation::VCarve(carve) if carve.enabled => Some(&carve.tool),
                Operation::Inlay(inlay) if inlay.enabled => Some(&inlay.tool),
                Operation::Drill(drill) if drill.enabled && drill.cycle.is_countersink() => {
                    Some(&drill.tool)
                }
                // And of an enabled Photo V-Carve (format 27).
                Operation::PhotoVCarve(photo) if photo.enabled => Some(&photo.tool),
                _ => None,
            })
            .collect();
        // Tools an enabled operation takes its step-down from: a
        // Profile, Pocket or Engrave with no step-down of its own, a peck
        // Drill with no peck depth (functional spec, capability 14), and a
        // V-Carve's clearing pass (format 20). The V-bit cuts in one pass.
        let step_down_from_tool: BTreeSet<&ToolId> = p
            .operations
            .iter()
            .filter(|op| op.enabled())
            .filter(|op| match op {
                Operation::Profile(op) => op.step_down_mm.is_none(),
                Operation::Pocket(op) => op.step_down_mm.is_none(),
                Operation::Engrave(op) => op.step_down_mm.is_none(),
                Operation::Bore(op) => op.pitch_mm.is_none(),
                Operation::VCarve(_) | Operation::Inlay(_) | Operation::PhotoVCarve(_) => false,
                Operation::Drill(op) => matches!(
                    op.cycle,
                    DrillCycle::Peck {
                        peck_depth_mm: None,
                        ..
                    }
                ),
            })
            .map(Operation::tool)
            .chain(clearing_tools())
            .collect();
        // The `T` number each used tool resolves to, set or
        // positional, and the first tool with it.
        let mut numbers: BTreeMap<u32, &ToolId> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        for (position, tool) in p.tools.iter().enumerate() {
            let in_use = used.contains(&tool.id);
            if !in_use {
                self.problems.push(Problem::UnusedTool {
                    tool: tool.id.clone(),
                    name: tool.name.clone(),
                });
            }
            if !seen.insert(&tool.id) {
                self.problems.push(Problem::DuplicateToolId {
                    id: tool.id.clone(),
                });
            }
            let at = format!("tools[{}]", tool.id);
            // `T0` is no tool; a set number must be positive, used
            // or not. Only used tools are written, so only they clash.
            match tool.number {
                Some(0) => self.tool_out_of_range(tool, "number", 0.0, POSITIVE),
                number if in_use => {
                    // The position is a list index, far below u32::MAX.
                    let resolved =
                        number.unwrap_or_else(|| u32::try_from(position + 1).unwrap_or(u32::MAX));
                    match numbers.get(&resolved) {
                        Some(other) if *other != &tool.id => {
                            self.problems.push(Problem::DuplicateToolNumber {
                                tool: tool.id.clone(),
                                name: tool.name.clone(),
                                number: resolved,
                                other: (*other).clone(),
                            });
                        }
                        Some(_) => {}
                        None => {
                            numbers.insert(resolved, &tool.id);
                        }
                    }
                }
                _ => {}
            }
            if let ToolKind::VBit { included_angle_deg } = tool.cutter {
                let field = "cutter.v_bit.included_angle_deg";
                if self.finite(&format!("{at}.{field}"), included_angle_deg)
                    && !(included_angle_deg > 0.0 && included_angle_deg < 180.0)
                {
                    if v_carve_tools.contains(&tool.id) {
                        self.problems.push(Problem::VBitAngleRange {
                            tool: tool.id.clone(),
                            name: tool.name.clone(),
                            included_angle_deg,
                        });
                    } else {
                        self.tool_out_of_range(tool, field, included_angle_deg, ANGLE);
                    }
                }
            }
            if let ToolKind::TaperedBallNose {
                tip_diameter_mm,
                taper_angle_deg,
                flute_length_mm,
            } = tool.cutter
            {
                self.tapered_ball_nose(
                    tool,
                    in_use,
                    [tip_diameter_mm, taper_angle_deg, flute_length_mm],
                );
            }
            self.tool_positive(tool, "diameter_mm", tool.diameter_mm);
            // The sanity limits apply only to a tool something cuts
            // with; an unused tool is one warning already.
            if in_use && tool.diameter_mm.is_finite() && tool.diameter_mm > MAX_TOOL_DIAMETER_MM {
                self.problems.push(Problem::LargeDiameter {
                    tool: tool.id.clone(),
                    name: tool.name.clone(),
                    diameter_mm: tool.diameter_mm,
                });
            }
            if tool.flutes == 0 {
                self.tool_out_of_range(tool, "flutes", 0.0, POSITIVE);
            }
            self.required_positive(tool, in_use, "feed_mm_min", tool.feed_mm_min);
            self.required_positive(tool, in_use, "plunge_mm_min", tool.plunge_mm_min);
            // With the router switched by hand the speed is only a hint in
            // the pause message, so it may be unset.
            let rpm_required =
                in_use && p.post_processor.spindle_control == SpindleControl::Automatic;
            match tool.rpm {
                None if rpm_required => self.problems.push(Problem::ToolValueUnset {
                    tool: tool.id.clone(),
                    name: tool.name.clone(),
                    field: "rpm",
                }),
                Some(0) => self.tool_out_of_range(tool, "rpm", 0.0, POSITIVE),
                Some(rpm) if in_use && rpm > MAX_RPM => {
                    self.problems.push(Problem::SpindleSpeedHigh {
                        tool: tool.id.clone(),
                        name: tool.name.clone(),
                        rpm,
                    });
                }
                None | Some(_) => {}
            }
            if let (true, Some(feed_mm_min), Some(plunge_mm_min)) =
                (in_use, tool.feed_mm_min, tool.plunge_mm_min)
                && feed_mm_min.is_finite()
                && feed_mm_min > 0.0
                && plunge_mm_min.is_finite()
                && plunge_mm_min > feed_mm_min
            {
                self.problems.push(Problem::PlungeAboveFeed {
                    tool: tool.id.clone(),
                    name: tool.name.clone(),
                    plunge_mm_min,
                    feed_mm_min,
                });
            }
            // Unset is an error only when an enabled operation falls back on
            // it; one error per tool, however many operations do.
            match tool.step_down_mm {
                Some(step) => self.tool_positive(tool, "step_down_mm", step),
                None if step_down_from_tool.contains(&tool.id) => {
                    self.problems.push(Problem::ToolValueUnset {
                        tool: tool.id.clone(),
                        name: tool.name.clone(),
                        field: "step_down_mm",
                    });
                }
                None => {}
            }
            if let Some(fraction) = tool.step_over_fraction {
                let field = "step_over_fraction";
                if self.finite(&format!("{at}.{field}"), fraction)
                    && !(fraction > 0.0 && fraction <= 1.0)
                {
                    self.tool_out_of_range(tool, field, fraction, FRACTION);
                }
                // A sanity check, so of a used tool only. The diameter is
                // the widest any generator multiplies the fraction by.
                if in_use && passes_too_fine(fraction, tool.diameter_mm) {
                    self.problems.push(Problem::StepOverTooFine {
                        field: format!("{at}.{field}"),
                        passes: FinePasses::Tool {
                            tool: tool.id.clone(),
                            name: tool.name.clone(),
                        },
                        fraction,
                        width_mm: tool.diameter_mm,
                    });
                }
            }
        }

        let tool_ids: BTreeSet<_> = p.tools.iter().map(|t| &t.id).collect();
        let instance_ids: BTreeSet<_> = p.instances.iter().map(|i| &i.id).collect();
        let mut seen = BTreeSet::new();
        for (index, op) in p.operations.iter().enumerate() {
            if !seen.insert(op.id()) {
                self.problems.push(Problem::DuplicateOperationId {
                    id: op.id().clone(),
                });
            }
            if !op.enabled() {
                self.problems.push(Problem::OperationDisabled {
                    operation: op.id().clone(),
                });
            }
            if !tool_ids.contains(op.tool()) {
                self.problems.push(Problem::DanglingTool {
                    operation: op.id().clone(),
                    tool: op.tool().clone(),
                });
            }
            for target in op.targets() {
                if !instance_ids.contains(target) {
                    self.problems.push(Problem::DanglingInstance {
                        operation: op.id().clone(),
                        instance: target.clone(),
                    });
                }
            }
            let at = format!("operations[{}]", op.id());
            let tool = p.tools.iter().find(|t| &t.id == op.tool());
            if op.enabled() {
                self.enabled_operation(op, tool, &at);
            }
            // A V-Carve has no depth of its own: its flat depth may be
            // unset, which carves to full depth (format 20). A
            // countersink's depth is derived, so its `depth_mm` is not
            // used (format 21).
            match op.depth_mm() {
                _ if matches!(op, Operation::VCarve(_)) => {}
                // An Inlay's values are its own (format 22): checked with
                // the rest of its fields, in file order.
                _ if matches!(op, Operation::Inlay(_)) => {}
                // A Photo V-Carve's depth is its `max_depth_mm` (format
                // 27), checked with the rest of its fields too.
                _ if matches!(op, Operation::PhotoVCarve(_)) => {}
                _ if matches!(op, Operation::Drill(drill) if drill.cycle.is_countersink()) => {}
                Some(depth_mm) => self.positive(&format!("{at}.depth_mm"), depth_mm),
                None => self.problems.push(Problem::OperationValueUnset {
                    operation: op.id().clone(),
                    name: op.name().to_owned(),
                    field: "depth_mm",
                }),
            }
            // The rest of each kind's fields, in the order the file holds
            // them.
            match op {
                Operation::Profile(op) => self.profile(op, &at),
                Operation::Pocket(op) => self.pocket(op, index, &p.operations, &at),
                Operation::Drill(drill) => {
                    self.drill(drill, &at);
                    if drill.enabled {
                        self.enabled_countersink(p, op, drill, tool);
                    }
                }
                Operation::Bore(op) => self.bore(op, &at),
                Operation::Engrave(op) => self.engrave(op, &at),
                Operation::VCarve(carve) => self.v_carve(p, op, carve, tool, &at),
                Operation::Inlay(inlay) => self.inlay(p, inlay, tool, &at),
                Operation::PhotoVCarve(photo) => self.photo_v_carve(p, op, photo, tool, &at),
            }
            if let (true, Some(tool)) = (op.enabled(), tool) {
                self.fine_passes(op, tool, &at);
            }
        }

        // Placed tabs, after the operations as the file holds them.
        let mut seen_tabs = BTreeSet::new();
        for tab in &p.tabs {
            if !seen_tabs.insert(&tab.id) {
                self.problems
                    .push(Problem::DuplicateTabId { id: tab.id.clone() });
            }
            self.placed_tab(p, tab);
        }

        // Definitions with a coordinate out of range; their instances are
        // not checked again, so each mistake is reported once.
        let mut out_of_range = BTreeSet::new();
        // Engrave and On line, Left and Right Profile cut open paths, so a
        // definition an enabled one targets is not reported as having paths
        // that are never cut.
        let engraved: BTreeSet<&DefinitionId> = p
            .operations
            .iter()
            .filter_map(|op| match op {
                Operation::Engrave(op) if op.enabled => Some(&op.targets),
                Operation::Profile(op) if op.enabled && op.side.cuts_open_paths() => {
                    Some(&op.targets)
                }
                _ => None,
            })
            .flatten()
            .filter_map(|id| p.instances.iter().find(|i| &i.id == id))
            .map(|i| &i.definition)
            .collect();
        for (key, def) in &p.definitions {
            if *key != def.id {
                self.problems.push(Problem::DefinitionKeyMismatch {
                    key: key.clone(),
                    id: def.id.clone(),
                });
            }
            for (i, profile) in def.profiles.iter().enumerate() {
                if !profile.is_closed() && !engraved.contains(&def.id) {
                    self.problems.push(Problem::OpenProfile {
                        definition: def.id.clone(),
                        name: def.name.clone(),
                        profile: i,
                    });
                }
                let mut ok = true;
                for (j, vertex) in profile.vertices().iter().enumerate() {
                    // Every load and save checks every vertex: name the
                    // fields only for one that fails.
                    let in_range = |v: f64| v.is_finite() && v.abs() <= MAX_COORDINATE_MM;
                    if in_range(vertex.x) && in_range(vertex.y) && vertex.bulge.is_finite() {
                        continue;
                    }
                    let at = format!("definitions[{key}].profiles[{i}].vertices[{j}]");
                    ok &= self.coordinate(&format!("{at}.x"), vertex.x);
                    ok &= self.coordinate(&format!("{at}.y"), vertex.y);
                    ok &= self.finite(&format!("{at}.bulge"), vertex.bulge);
                }
                // An arc can bow past the limit between vertices inside it.
                let reach_mm = profile.reach_mm();
                if ok && reach_mm > MAX_COORDINATE_MM {
                    self.out_of_range(format!("definitions[{key}].profiles[{i}]"), reach_mm, REACH);
                    ok = false;
                }
                if !ok {
                    out_of_range.insert(key);
                }
            }
        }

        let mut seen = BTreeSet::new();
        for inst in &p.instances {
            if !seen.insert(&inst.id) {
                self.problems.push(Problem::DuplicateInstanceId {
                    id: inst.id.clone(),
                });
            }
            if !p.definitions.contains_key(&inst.definition) {
                self.problems.push(Problem::DanglingDefinition {
                    instance: inst.id.clone(),
                    definition: inst.definition.clone(),
                });
            }
            let at = format!("instances[{}]", inst.id);
            let coeffs = inst.transform.coeffs();
            let mut ok = true;
            for (i, c) in coeffs.iter().enumerate() {
                let field = format!("{at}.transform[{i}]");
                if !self.finite(&field, *c) {
                    ok = false;
                } else if c.abs() > MAX_COORDINATE_MM {
                    self.out_of_range(field, *c, COEFFICIENT);
                    ok = false;
                }
            }
            // Each coefficient can be in range and still carry a shape past
            // the limit. Other transform errors (singular, arcs under a
            // non-uniform scale) are the generators' to report.
            let def = p.definitions.get(&inst.definition);
            if let Some(def) = def.filter(|_| ok && !out_of_range.contains(&inst.definition)) {
                let reach_mm = def
                    .profiles
                    .iter()
                    .filter_map(|profile| match inst.transform.apply_polyline(profile) {
                        Err(GeometryError::CoordinateOutOfRange { reach_mm, .. }) => Some(reach_mm),
                        _ => None,
                    })
                    .reduce(f64::max);
                if let Some(reach_mm) = reach_mm {
                    self.out_of_range(format!("{at}.transform"), reach_mm, REACH);
                }
            }
            // Unset depths are fine: phase 1 does not use them.
            if let Some(z_top_mm) = inst.z_top_mm {
                self.coordinate(&format!("{at}.z_top_mm"), z_top_mm);
            }
            if let Some(z_bottom_mm) = inst.z_bottom_mm {
                let field = format!("{at}.z_bottom_mm");
                // Nothing reads them yet, but a top below the bottom is
                // never meant.
                if self.coordinate(&field, z_bottom_mm)
                    && inst
                        .z_top_mm
                        .is_some_and(|top| top.is_finite() && top < z_bottom_mm)
                {
                    self.out_of_range(field, z_bottom_mm, BELOW_Z_TOP);
                }
            }
        }

        // Format 27: the pictures, after the instances as the file holds
        // them.
        let mut seen = BTreeSet::new();
        for bitmap in &p.bitmaps {
            if !seen.insert(&bitmap.id) {
                self.problems.push(Problem::DuplicateBitmapId {
                    id: bitmap.id.clone(),
                });
            }
            self.bitmap(bitmap);
        }

        if !is_safe_file_extension(&p.post_processor.file_extension) {
            self.problems.push(Problem::UnsafeFileExtension {
                extension: p.post_processor.file_extension.clone(),
            });
        }

        // The definition's own values are checked by tests on every
        // bundled version, since a project cannot change them. A
        // reference that names no bundled definition fails to load, so
        // only a project built in code gets here without one; export
        // refuses it.
        let post = &p.post_processor;
        let definition_has_m6 = post
            .definition()
            .is_ok_and(|d| d.post_processor().tool_change.has_m6());
        if post.tool_change_mode == ToolChangeMode::M6 && !definition_has_m6 {
            self.problems.push(Problem::ToolChangeModeNeedsM6 {
                post: post.id.clone(),
            });
        }

        self.non_negative("post_processor.spindle_on_dwell_s", post.spindle_on_dwell_s);
        // Canned cycles can only be written where the definition
        // has them; asking elsewhere would be ignored, so it is an error.
        let definition_has_cycles = post
            .definition()
            .is_ok_and(|d| d.post_processor().canned_cycles.is_some());
        if post.use_canned_cycles && !definition_has_cycles {
            self.problems.push(Problem::CannedCyclesUnsupported {
                post: post.id.clone(),
            });
        }
    }

    /// The sanity checks of an enabled operation `op` at `at`, cutting
    /// with `tool` when it exists: it has targets; a Pocket, a
    /// Bore or an Inside or Outside Profile cuts with a flat end mill; a
    /// Bore's pitch is at most the tool's step-down; a Drill
    /// cuts with a drill or end mill (a warning otherwise); the step-down
    /// in effect is at most the tool's diameter; corner relief has a tool
    /// step-down to step at; a V-bit engraves no deeper than its cone.
    fn enabled_operation(&mut self, op: &Operation, tool: Option<&Tool>, at: &str) {
        // A Photo V-Carve carves a bitmap, not instances (format 27).
        if op.targets().is_empty() && !matches!(op, Operation::PhotoVCarve(_)) {
            self.problems.push(Problem::OperationHasNoTargets {
                operation: op.id().clone(),
                name: op.name().to_owned(),
            });
        }
        // A V-Carve's tool rules are its own (format 20,
        // [`Self::enabled_v_carve`]): it cuts in one pass, so it has no
        // step-down to check. So are a countersink's (format 21,
        // [`Self::enabled_countersink`]): its V-bit is no unusual drill.
        let own_rules = match op {
            Operation::VCarve(_) | Operation::Inlay(_) | Operation::PhotoVCarve(_) => true,
            Operation::Drill(drill) => drill.cycle.is_countersink(),
            Operation::Profile(_)
            | Operation::Pocket(_)
            | Operation::Bore(_)
            | Operation::Engrave(_) => false,
        };
        let Some(tool) = tool.filter(|_| !own_rules) else {
            return;
        };
        let offsets_by_radius = match op {
            Operation::Profile(op) => op.side != Side::OnLine,
            Operation::Pocket(_) | Operation::Bore(_) => true,
            Operation::Drill(_)
            | Operation::Engrave(_)
            | Operation::VCarve(_)
            | Operation::Inlay(_)
            | Operation::PhotoVCarve(_) => false,
        };
        if offsets_by_radius && !tool.cutter.is_flat() {
            self.problems.push(Problem::CutterNotFlat {
                operation: op.id().clone(),
                name: op.name().to_owned(),
                tool: tool.id.clone(),
                tool_name: tool.name.clone(),
                cutter: tool.cutter.describe(),
                field: "tool",
            });
        }
        if let Operation::Drill(_) = op
            && !matches!(tool.cutter, ToolKind::Drill | ToolKind::EndMill)
        {
            self.problems.push(Problem::DrillCutterUnusual {
                operation: op.id().clone(),
                name: op.name().to_owned(),
                tool: tool.id.clone(),
                tool_name: tool.name.clone(),
                cutter: tool.cutter.describe(),
            });
        }
        // A Bore's pitch is held to the tool's step-down (format 18).
        if let Operation::Bore(bore) = op
            && let (Some(pitch_mm), Some(step_down_mm)) = (bore.pitch_mm, tool.step_down_mm)
            && pitch_mm.is_finite()
            && step_down_mm.is_finite()
            && pitch_mm > step_down_mm
        {
            self.problems.push(Problem::BorePitchOverStepDown {
                operation: op.id().clone(),
                name: op.name().to_owned(),
                pitch_mm,
                step_down_mm,
            });
        }
        let own_step_down = match op {
            Operation::Profile(op) => op.step_down_mm,
            Operation::Bore(_)
            | Operation::VCarve(_)
            | Operation::Inlay(_)
            | Operation::PhotoVCarve(_) => None,
            Operation::Pocket(op) => op.step_down_mm,
            Operation::Engrave(op) => op.step_down_mm,
            Operation::Drill(op) => match op.cycle {
                DrillCycle::Peck { peck_depth_mm, .. } => peck_depth_mm,
                DrillCycle::Simple | DrillCycle::Countersink { .. } => None,
            },
        };
        let step = match (own_step_down, tool.step_down_mm) {
            (Some(step), _) => Some((step, format!("{at}.step_down_mm"))),
            (None, Some(step)) => Some((step, format!("tools[{}].step_down_mm", tool.id))),
            (None, None) => None,
        };
        // The last pass is at most one step-down thick.
        if let Operation::Profile(profile) = op
            && let Some(thickness_mm) = profile.last_pass.as_ref().and_then(|l| l.thickness_mm)
            && thickness_mm.is_finite()
            && thickness_mm > 0.0
            && profile
                .depth_mm
                .is_none_or(|depth_mm| thickness_mm <= depth_mm)
            && let Some((step_down_mm, field)) = &step
            && step_down_mm.is_finite()
            && thickness_mm > *step_down_mm
        {
            self.problems.push(Problem::LastPassThickerThanStepDown {
                operation: op.id().clone(),
                name: op.name().to_owned(),
                thickness_mm,
                step_down_mm: *step_down_mm,
                step_down_field: field.clone(),
            });
        }
        if let Some((step_down_mm, field)) = step
            && step_down_mm.is_finite()
            && tool.diameter_mm.is_finite()
            && tool.diameter_mm > 0.0
            && step_down_mm > tool.diameter_mm
        {
            self.problems.push(Problem::StepDownAboveDiameter {
                operation: op.id().clone(),
                name: op.name().to_owned(),
                tool_name: tool.name.clone(),
                field,
                step_down_mm,
                diameter_mm: tool.diameter_mm,
            });
        }
        // The relief pass steps down at the tool's step-down (functional
        // spec, capability 18). With no step-down anywhere the operation
        // is already an error at the tool; the warning is for the case
        // the main passes are fine and only the relief is lost.
        let relief = match op {
            Operation::Profile(op) => op.corner_relief,
            Operation::Pocket(op) => op.corner_relief,
            Operation::Drill(_)
            | Operation::Bore(_)
            | Operation::Engrave(_)
            | Operation::VCarve(_)
            | Operation::Inlay(_)
            | Operation::PhotoVCarve(_) => CornerRelief::None,
        };
        if relief != CornerRelief::None && tool.step_down_mm.is_none() && own_step_down.is_some() {
            self.problems.push(Problem::ReliefNeedsStepDown {
                operation: op.id().clone(),
                name: op.name().to_owned(),
                tool: tool.id.clone(),
                tool_name: tool.name.clone(),
            });
        }
        // VCarve advises a stock to leave well below a third of the bit.
        if let Operation::Profile(profile) = op
            && let Some(last_pass) = &profile.last_pass
            && last_pass.stock_to_leave_mm.is_finite()
            && tool.diameter_mm.is_finite()
            && tool.diameter_mm > 0.0
        {
            let limit_mm = tool.diameter_mm / 3.0;
            let stock_to_leave_mm = last_pass.stock_to_leave_mm;
            if stock_to_leave_mm <= 0.0 || stock_to_leave_mm > limit_mm {
                self.problems.push(Problem::LastPassStockToLeave {
                    operation: op.id().clone(),
                    name: op.name().to_owned(),
                    tool_name: tool.name.clone(),
                    stock_to_leave_mm,
                    diameter_mm: tool.diameter_mm,
                    limit_mm,
                });
            }
        }
        if let (Operation::Engrave(engrave), ToolKind::VBit { included_angle_deg }) =
            (op, &tool.cutter)
            && let included_angle_deg = *included_angle_deg
            && let Some(depth_mm) = engrave.depth_mm
            && included_angle_deg > 0.0
            && included_angle_deg < 180.0
            && tool.diameter_mm.is_finite()
            && tool.diameter_mm > 0.0
        {
            let cone_height_mm =
                tool.diameter_mm / 2.0 / (included_angle_deg.to_radians() / 2.0).tan();
            if depth_mm.is_finite() && depth_mm > cone_height_mm {
                self.problems.push(Problem::EngraveDeeperThanCone {
                    operation: op.id().clone(),
                    name: op.name().to_owned(),
                    tool_name: tool.name.clone(),
                    depth_mm,
                    cone_height_mm,
                });
            }
        }
    }

    /// A tab request's fields at `at`, in file order: a Profile's, or a
    /// male Inlay's cut-out's (format 23).
    fn tab_spec(&mut self, tabs: &TabSpec, at: &str) {
        match &tabs.placement {
            TabPlacement::Count { count } => {
                if *count == 0 {
                    self.out_of_range(format!("{at}.placement.count"), 0.0, POSITIVE);
                }
            }
            TabPlacement::AtDistances { distances_mm } => {
                for (i, d) in distances_mm.iter().enumerate() {
                    self.non_negative(&format!("{at}.placement.distances_mm[{i}]"), *d);
                }
            }
        }
        self.positive(&format!("{at}.width_mm"), tabs.width_mm);
        self.positive(&format!("{at}.height_mm"), tabs.height_mm);
    }

    /// A Profile's own fields after `depth_mm`, in file order.
    fn profile(&mut self, op: &ProfileOp, at: &str) {
        if let Some(step) = op.step_down_mm {
            self.positive(&format!("{at}.step_down_mm"), step);
        }
        self.non_negative(
            &format!("{at}.through_cut_allowance_mm"),
            op.through_cut_allowance_mm,
        );
        self.positive(&format!("{at}.safe_z_mm"), op.safe_z_mm);
        if let Some(tabs) = &op.tabs {
            self.tab_spec(tabs, &format!("{at}.tabs"));
        }
        self.entry(&op.entry, at);
        // A helix needs removed material to sit in (functional spec,
        // capability 16); an On line, Left or Right cut removes only its
        // kerf.
        if !op.side.has_waste_area() && matches!(op.entry, Entry::Helix { .. }) {
            self.problems.push(Problem::HelixNeedsWasteSide {
                operation: op.id.clone(),
                name: op.name.clone(),
            });
        }
        if let Some(lead) = &op.lead {
            self.lead(lead, at);
        }
        if let Some(last_pass) = &op.last_pass {
            self.last_pass(op, last_pass, at);
        }
    }

    /// A last pass's values that need no tool: a finite stock to leave
    /// (its range needs the bit, [`Self::enabled_operation`]), a thickness
    /// in (0, depth], and an Inside or Outside side.
    fn last_pass(&mut self, op: &ProfileOp, last_pass: &LastPass, at: &str) {
        self.finite(
            &format!("{at}.last_pass.stock_to_leave_mm"),
            last_pass.stock_to_leave_mm,
        );
        if let Some(thickness_mm) = last_pass.thickness_mm {
            let field = format!("{at}.last_pass.thickness_mm");
            let above_depth = op
                .depth_mm
                .is_some_and(|depth_mm| depth_mm.is_finite() && thickness_mm > depth_mm);
            if self.finite(&field, thickness_mm) && (thickness_mm <= 0.0 || above_depth) {
                self.out_of_range(field, thickness_mm, LAST_PASS_THICKNESS);
            }
        }
        if !op.side.has_wall() {
            self.problems.push(Problem::LastPassNeedsSide {
                operation: op.id.clone(),
                name: op.name.clone(),
                side: op.side,
            });
        }
    }

    /// `step_over_too_fine` for enabled operation `op` at `at`, cut with
    /// `tool`: the stepover in effect, times the width its generator
    /// multiplies it by, is under [`MIN_PASS_SPACING_MM`]. A Pocket
    /// (`dc-cam`'s `pocket.rs`) steps over a fraction of the tool's
    /// diameter, so only its own fraction is judged here: the tool's is
    /// judged at the tool, by the same product, and so is a clearing
    /// pass's, which has no fraction of its own. A V-Carve's and an
    /// Inlay's floor (`vcarve/floor.rs`, `Floor::stepover`) step over a
    /// fraction of the bit's width at the flat depth, a V-bit's and a
    /// tapered ball nose's alike. A Photo V-Carve (`photo.rs`) spaces its
    /// grooves by a fraction of a groove's width at `max_depth_mm`.
    fn fine_passes(&mut self, op: &Operation, tool: &Tool, at: &str) {
        let usable = |depth_mm: Option<f64>| depth_mm.filter(|d| d.is_finite() && *d > 0.0);
        match op {
            Operation::Pocket(pocket) => {
                if let Some(fraction) = pocket.step_over_fraction
                    && passes_too_fine(fraction, tool.diameter_mm)
                {
                    self.problems.push(Problem::StepOverTooFine {
                        field: format!("{at}.step_over_fraction"),
                        passes: FinePasses::Pocket {
                            operation: pocket.id.clone(),
                            name: pocket.name.clone(),
                            tool_name: tool.name.clone(),
                        },
                        fraction,
                        width_mm: tool.diameter_mm,
                    });
                }
            }
            Operation::VCarve(carve) => {
                if let Some(depth_mm) = usable(carve.flat_depth_mm) {
                    self.fine_floor(op, tool, carve.step_over_fraction, depth_mm, at);
                }
            }
            Operation::Inlay(inlay) => {
                // The female's floor is `depth_mm` down; the male's, once
                // its board is measured, `plug_depth_mm` below its start
                // plane. The shallower floor is the narrower, so the finer.
                let male = usable(inlay.plug_depth_mm).filter(|_| inlay.cuts_male());
                let depth_mm = match (usable(inlay.depth_mm), male) {
                    (Some(female_mm), Some(male_mm)) => Some(female_mm.min(male_mm)),
                    (female, male) => female.or(male),
                };
                if let Some(depth_mm) = depth_mm {
                    self.fine_floor(op, tool, inlay.step_over_fraction, depth_mm, at);
                }
            }
            Operation::PhotoVCarve(photo) => {
                if let Some(max_depth_mm) = usable(photo.max_depth_mm)
                    && let Some(profile) = tool.carve_profile()
                {
                    let width_mm = 2.0 * profile.half_width_mm(max_depth_mm);
                    let fraction = photo.line_spacing_fraction;
                    if passes_too_fine(fraction, width_mm) {
                        self.problems.push(Problem::StepOverTooFine {
                            field: format!("{at}.line_spacing_fraction"),
                            passes: FinePasses::Grooves {
                                operation: photo.id.clone(),
                                name: photo.name.clone(),
                                max_depth_mm,
                            },
                            fraction,
                            width_mm,
                        });
                    }
                }
            }
            Operation::Profile(_)
            | Operation::Drill(_)
            | Operation::Bore(_)
            | Operation::Engrave(_) => {}
        }
    }

    /// [`Self::fine_passes`] for the floor of carve `op`, `depth_mm` below
    /// its start plane: the stepover in effect is the operation's `own`,
    /// else its tool's, of the width `tool` cuts at that depth. A tool's
    /// fraction that is too fine of its whole diameter is reported at the
    /// tool and not again here.
    fn fine_floor(
        &mut self,
        op: &Operation,
        tool: &Tool,
        own: Option<f64>,
        depth_mm: f64,
        at: &str,
    ) {
        let Some(profile) = tool.carve_profile() else {
            return;
        };
        let width_mm = 2.0 * profile.half_width_mm(depth_mm);
        let (fraction, field, from_tool) = match (own, tool.step_over_fraction) {
            (Some(fraction), _) => (fraction, format!("{at}.step_over_fraction"), None),
            (None, Some(fraction)) if !passes_too_fine(fraction, tool.diameter_mm) => (
                fraction,
                format!("tools[{}].step_over_fraction", tool.id),
                Some(tool.name.clone()),
            ),
            (None, _) => return,
        };
        if passes_too_fine(fraction, width_mm) {
            self.problems.push(Problem::StepOverTooFine {
                field,
                passes: FinePasses::Floor {
                    operation: op.id().clone(),
                    name: op.name().to_owned(),
                    depth_mm,
                    from_tool,
                },
                fraction,
                width_mm,
            });
        }
    }

    /// A Pocket's own fields, in file order, and its `rest_of` against the
    /// operations before it (`index`).
    fn pocket(&mut self, op: &PocketOp, index: usize, operations: &[Operation], at: &str) {
        if let ClearanceStrategy::Raster { angle_deg } = op.strategy {
            let field = format!("{at}.strategy.raster.angle_deg");
            if self.finite(&field, angle_deg) && !(0.0..180.0).contains(&angle_deg) {
                self.out_of_range(field, angle_deg, RASTER_ANGLE);
            }
        }
        if let Some(step) = op.step_down_mm {
            self.positive(&format!("{at}.step_down_mm"), step);
        }
        if let Some(fraction) = op.step_over_fraction {
            let field = format!("{at}.step_over_fraction");
            if self.finite(&field, fraction) && !(fraction > 0.0 && fraction <= 1.0) {
                self.out_of_range(field, fraction, FRACTION);
            }
        }
        self.non_negative(&format!("{at}.stock_to_leave_mm"), op.stock_to_leave_mm);
        // Rest machining clears what an earlier Pocket of the same
        // shapes left (functional spec, capability 13).
        if let Some(rest_of) = &op.rest_of {
            let earlier = operations
                .iter()
                .take(index)
                .find(|other| other.id() == rest_of);
            let valid = matches!(
                earlier,
                Some(Operation::Pocket(rough)) if rough.enabled && rough.targets == op.targets
            );
            if !valid {
                self.problems.push(Problem::RestOfInvalid {
                    operation: op.id.clone(),
                    name: op.name.clone(),
                    rest_of: rest_of.clone(),
                });
            }
        }
        self.entry(&op.entry, at);
        self.non_negative(
            &format!("{at}.through_cut_allowance_mm"),
            op.through_cut_allowance_mm,
        );
        self.positive(&format!("{at}.safe_z_mm"), op.safe_z_mm);
    }

    /// A Drill's own fields, in file order.
    fn drill(&mut self, op: &DrillOp, at: &str) {
        match &op.cycle {
            DrillCycle::Simple => {}
            DrillCycle::Peck {
                peck_depth_mm,
                retract,
            } => {
                if let Some(peck_depth_mm) = peck_depth_mm {
                    self.positive(&format!("{at}.cycle.peck.peck_depth_mm"), *peck_depth_mm);
                }
                if let PeckRetract::ChipBreak { gap_mm } = retract {
                    self.positive(
                        &format!("{at}.cycle.peck.retract.chip_break.gap_mm"),
                        *gap_mm,
                    );
                }
            }
            DrillCycle::Countersink {
                diameter_mm,
                start_depth_mm,
                ..
            } => {
                if self.finite(&format!("{at}.{COUNTERSINK_DIAMETER}"), *diameter_mm)
                    && *diameter_mm <= 0.0
                {
                    self.problems.push(Problem::CountersinkDiameterPositive {
                        operation: op.id.clone(),
                        name: op.name.clone(),
                        diameter_mm: *diameter_mm,
                    });
                }
                if self.finite(&format!("{at}.{COUNTERSINK_START_DEPTH}"), *start_depth_mm)
                    && *start_depth_mm < 0.0
                {
                    self.problems.push(Problem::StartDepthNegative {
                        operation: op.id.clone(),
                        name: op.name.clone(),
                        start_depth_mm: *start_depth_mm,
                        field: COUNTERSINK_START_DEPTH,
                    });
                }
            }
        }
        self.non_negative(&format!("{at}.dwell_s"), op.dwell_s);
        self.non_negative(&format!("{at}.retract_mm"), op.retract_mm);
        self.non_negative(
            &format!("{at}.through_cut_allowance_mm"),
            op.through_cut_allowance_mm,
        );
        self.positive(&format!("{at}.safe_z_mm"), op.safe_z_mm);
        // Format 21: a countersink's bit is wider than its hole by design.
        if op.cycle.is_countersink() && op.allow_tool_wider_than_hole {
            self.problems.push(Problem::CountersinkIgnoresToolWidth {
                operation: op.id.clone(),
                name: op.name.clone(),
            });
        }
    }

    /// The rules of an enabled countersink Drill that need its tool and the
    /// project (format 21; `docs/specs/phase-3/design.md`, "Validation"):
    /// it cuts with a V-bit; its mouth is no wider than the bit; a start
    /// depth below the stock top is cleared by an earlier floor that holds
    /// the bit at every hole, a Pocket's or a V-Carve's flat floor
    /// ([`cleared_start_plane`]), or vouched for; and the bit's flanks
    /// engage no deeper than its cone, counting the start depth unless such
    /// a floor at that depth clears the plane. `op` is `drill` as the
    /// project holds it. A Drill of another cycle has none of these.
    fn enabled_countersink(
        &mut self,
        p: &Project,
        op: &Operation,
        drill: &DrillOp,
        tool: Option<&Tool>,
    ) {
        let DrillCycle::Countersink {
            diameter_mm,
            start_depth_mm,
            start_plane_cleared_externally,
        } = drill.cycle
        else {
            return;
        };
        let angle_deg = match tool.map(|t| &t.cutter) {
            Some(ToolKind::VBit { included_angle_deg }) => Some(*included_angle_deg),
            Some(cutter) => {
                if let Some(tool) = tool {
                    self.problems.push(Problem::CountersinkNeedsVBit {
                        operation: drill.id.clone(),
                        name: drill.name.clone(),
                        tool: tool.id.clone(),
                        tool_name: tool.name.clone(),
                        cutter: cutter.describe(),
                    });
                }
                None
            }
            None => None,
        };
        let mouth_ok = diameter_mm.is_finite() && diameter_mm > 0.0;
        let mut fits_tool = mouth_ok;
        if let Some(tool) = tool
            && mouth_ok
            && tool.diameter_mm.is_finite()
            && diameter_mm - tool.diameter_mm > COUNTERSINK_TOLERANCE_MM
        {
            fits_tool = false;
            self.problems.push(Problem::CountersinkMouthExceedsTool {
                operation: drill.id.clone(),
                name: drill.name.clone(),
                tool_name: tool.name.clone(),
                diameter_mm,
                tool_diameter_mm: tool.diameter_mm,
            });
        }

        // The start plane (capability 41), as for a V-Carve; the footprint
        // is the bit's disc at each hole.
        if !(start_depth_mm.is_finite() && start_depth_mm >= 0.0) {
            return;
        }
        let below_top = start_depth_mm > 0.0;
        let cleared = if below_top {
            cleared_start_plane(p, op)
        } else {
            None
        };
        if below_top && cleared.is_none() && !start_plane_cleared_externally {
            self.problems.push(Problem::StartDepthUncleared {
                operation: drill.id.clone(),
                name: drill.name.clone(),
                start_depth_mm,
                field: COUNTERSINK_START_DEPTH,
                footprint: StartFootprint::BitAtEachHole,
            });
        }
        let cleared_at_start = cleared.is_some_and(|plane| plane.is_at(start_depth_mm));
        let from_stock_top = below_top && !cleared_at_start;
        let (Some(tool), Some(angle_deg), true) = (tool, angle_deg, fits_tool) else {
            return;
        };
        if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
            return;
        }
        // The angle out of range is `v_bit_angle_range`, at the tool.
        let (Some(depth_mm), Some(cone_height_mm)) = (
            countersink_depth_mm(diameter_mm, angle_deg),
            countersink_depth_mm(tool.diameter_mm, angle_deg),
        ) else {
            return;
        };
        let engagement_mm = depth_mm + if from_stock_top { start_depth_mm } else { 0.0 };
        if engagement_mm - cone_height_mm > COUNTERSINK_TOLERANCE_MM {
            self.problems.push(Problem::VBitConeExceeded {
                operation: drill.id.clone(),
                name: drill.name.clone(),
                tool_name: tool.name.clone(),
                engagement_mm,
                cone_height_mm,
                from_stock_top,
                field: COUNTERSINK_DIAMETER,
                limit: CuttingLimit::Cone,
            });
        }
    }

    /// A Bore's own fields, in file order (format 18).
    fn bore(&mut self, op: &BoreOp, at: &str) {
        if let Some(pitch_mm) = op.pitch_mm {
            self.positive(&format!("{at}.pitch_mm"), pitch_mm);
        }
        self.non_negative(&format!("{at}.stock_to_leave_mm"), op.stock_to_leave_mm);
        self.non_negative(
            &format!("{at}.through_cut_allowance_mm"),
            op.through_cut_allowance_mm,
        );
        self.positive(&format!("{at}.safe_z_mm"), op.safe_z_mm);
    }

    /// A V-Carve's own fields, in file order, then, when it is enabled,
    /// its tool, clearing and start-depth rules (format 20;
    /// `docs/specs/phase-3/design.md`, "Validation"). `op` is `carve` as
    /// the project holds it.
    fn v_carve(
        &mut self,
        p: &Project,
        op: &Operation,
        carve: &VCarveOp,
        tool: Option<&Tool>,
        at: &str,
    ) {
        if let VCarveStrategy::Offset { step_over_mm, .. } = carve.strategy {
            self.positive(&format!("{at}.strategy.offset.step_over_mm"), step_over_mm);
        }
        self.clearing(p, &carve.id, &carve.clearing, at);
        if !carve.clearing.is_empty() && carve.flat_depth_mm.is_none() {
            self.problems.push(Problem::ClearingNeedsFlatDepth {
                operation: carve.id.clone(),
                name: carve.name.clone(),
            });
        }
        if let Some(flat_depth_mm) = carve.flat_depth_mm {
            self.positive(&format!("{at}.flat_depth_mm"), flat_depth_mm);
        }
        let start_depth_ok = self.finite(&format!("{at}.start_depth_mm"), carve.start_depth_mm);
        if start_depth_ok && carve.start_depth_mm < 0.0 {
            self.problems.push(Problem::StartDepthNegative {
                operation: carve.id.clone(),
                name: carve.name.clone(),
                start_depth_mm: carve.start_depth_mm,
                field: "start_depth_mm",
            });
        }
        self.v_bit_floor(
            carve.step_over_fraction,
            carve.through_cut_allowance_mm,
            carve.safe_z_mm,
            at,
        );
        if carve.enabled {
            self.enabled_v_carve(p, op, carve, tool);
        }
    }

    /// The clearing passes of carve `operation` at `at`, in file order
    /// (format 20; an Inlay's too, format 22): each names a tool of the
    /// project, and its strategy and entry follow the Pocket's rules.
    fn clearing(
        &mut self,
        p: &Project,
        operation: &OperationId,
        clearing: &[ClearingPass],
        at: &str,
    ) {
        for (i, pass) in clearing.iter().enumerate() {
            let pass_at = format!("{at}.clearing[{i}]");
            if !p.tools.iter().any(|t| t.id == pass.tool) {
                self.problems.push(Problem::ClearingDanglingTool {
                    operation: operation.clone(),
                    pass: i,
                    tool: pass.tool.clone(),
                });
            }
            if let ClearanceStrategy::Raster { angle_deg } = pass.strategy {
                let field = format!("{pass_at}.strategy.raster.angle_deg");
                if self.finite(&field, angle_deg) && !(0.0..180.0).contains(&angle_deg) {
                    self.out_of_range(field, angle_deg, RASTER_ANGLE);
                }
            }
            self.entry(&pass.entry, &pass_at);
        }
    }

    /// The fields a V-Carve and an Inlay end with, in file order: the
    /// V-bit floor's step-over fraction, the through-cut allowance and
    /// safe Z.
    fn v_bit_floor(
        &mut self,
        step_over_fraction: Option<f64>,
        through_cut_allowance_mm: f64,
        safe_z_mm: f64,
        at: &str,
    ) {
        if let Some(fraction) = step_over_fraction {
            let field = format!("{at}.step_over_fraction");
            if self.finite(&field, fraction) && !(fraction > 0.0 && fraction <= 1.0) {
                self.out_of_range(field, fraction, FRACTION);
            }
        }
        self.non_negative(
            &format!("{at}.through_cut_allowance_mm"),
            through_cut_allowance_mm,
        );
        self.positive(&format!("{at}.safe_z_mm"), safe_z_mm);
    }

    /// The rules of an enabled V-Carve that need its tools and the
    /// project (format 20): it carves with a V-bit; each clearing pass
    /// cuts with a flat end mill no wider than the pass before (a
    /// warning); a start depth below the stock top is cleared by an
    /// earlier Pocket or vouched for; and the bit's flanks engage no deeper
    /// than its cone.
    fn enabled_v_carve(
        &mut self,
        p: &Project,
        op: &Operation,
        carve: &VCarveOp,
        tool: Option<&Tool>,
    ) {
        self.carving_tools(p, &carve.id, &carve.name, tool, &carve.clearing);

        // The start plane (capability 41): an earlier floor that holds the
        // targets clears it, a Pocket's or a V-Carve's flat floor;
        // otherwise, below the stock top, the user must vouch for it.
        let start_depth_mm = carve.start_depth_mm;
        if !(start_depth_mm.is_finite() && start_depth_mm >= 0.0) {
            return;
        }
        let below_top = start_depth_mm > 0.0;
        let cleared = if below_top {
            cleared_start_plane(p, op)
        } else {
            None
        };
        if below_top && cleared.is_none() && !carve.start_plane_cleared_externally {
            self.problems.push(Problem::StartDepthUncleared {
                operation: carve.id.clone(),
                name: carve.name.clone(),
                start_depth_mm,
                field: "start_depth_mm",
                footprint: StartFootprint::Targets,
            });
        }
        // The flanks engage from the start plane only when a floor at that
        // depth clears it (design spec, "V-Carve generator", step 3); from
        // the stock top otherwise.
        let cleared_at_start = cleared.is_some_and(|plane| plane.is_at(start_depth_mm));
        let from_stock_top = below_top && !cleared_at_start;
        let Some(tool) = tool else {
            return;
        };
        // A V-bit's cone height, or a tapered ball nose's cutting height;
        // `None` for any other cutter or a value out of range, which other
        // rules name.
        let (Some(cone_height_mm), Some(limit)) = (tool.cutting_height_mm(), tool.cutting_limit())
        else {
            return;
        };
        let flat_depth_mm = carve.flat_depth_mm.filter(|f| f.is_finite() && *f > 0.0);
        // Without a flat depth the carve's depth follows from the shapes,
        // which only the generator knows; the start depth alone may
        // already be too much.
        let engagement_mm =
            flat_depth_mm.unwrap_or(0.0) + if from_stock_top { start_depth_mm } else { 0.0 };
        if engagement_mm > cone_height_mm + EPS_MM {
            self.problems.push(Problem::VBitConeExceeded {
                operation: carve.id.clone(),
                name: carve.name.clone(),
                tool_name: tool.name.clone(),
                engagement_mm,
                cone_height_mm,
                from_stock_top,
                field: if carve.flat_depth_mm.is_some() {
                    "flat_depth_mm"
                } else {
                    "start_depth_mm"
                },
                limit,
            });
        }
    }

    /// A Photo V-Carve's own fields, in file order, then, when it is
    /// enabled, its tool, start plane and cone rules (format 27;
    /// `docs/specs/phase-3/design.md`, "Validation"). It has no targets:
    /// `image` names a bitmap of the project.
    fn photo_v_carve(
        &mut self,
        p: &Project,
        op: &Operation,
        photo: &PhotoVCarveOp,
        tool: Option<&Tool>,
        at: &str,
    ) {
        let (operation, name) = (&photo.id, &photo.name);
        if !p.bitmaps.iter().any(|b| b.id == photo.image) {
            self.problems.push(Problem::PhotoUnknownBitmap {
                operation: operation.clone(),
                name: name.clone(),
                bitmap: photo.image.clone(),
            });
        }
        // Never defaulted: unset is an error, as any operation's depth.
        match photo.max_depth_mm {
            Some(max_depth_mm) => self.positive(&format!("{at}.max_depth_mm"), max_depth_mm),
            None => self.problems.push(Problem::OperationValueUnset {
                operation: operation.clone(),
                name: name.clone(),
                field: "max_depth_mm",
            }),
        }
        let start_depth_ok = self.finite(&format!("{at}.start_depth_mm"), photo.start_depth_mm);
        if start_depth_ok && photo.start_depth_mm < 0.0 {
            self.problems.push(Problem::StartDepthNegative {
                operation: operation.clone(),
                name: name.clone(),
                start_depth_mm: photo.start_depth_mm,
                field: "start_depth_mm",
            });
        }
        let line_angle_deg = photo.line_angle_deg;
        if self.finite(&format!("{at}.line_angle_deg"), line_angle_deg)
            && !(0.0..180.0).contains(&line_angle_deg)
        {
            self.problems.push(Problem::PhotoLineAngleRange {
                operation: operation.clone(),
                name: name.clone(),
                line_angle_deg,
            });
        }
        let line_spacing_fraction = photo.line_spacing_fraction;
        if self.finite(
            &format!("{at}.line_spacing_fraction"),
            line_spacing_fraction,
        ) {
            if line_spacing_fraction <= 0.0 {
                self.problems.push(Problem::PhotoLineSpacingPositive {
                    operation: operation.clone(),
                    name: name.clone(),
                    line_spacing_fraction,
                });
            } else if line_spacing_fraction < 1.0 {
                self.problems.push(Problem::PhotoLinesOverlap {
                    operation: operation.clone(),
                    name: name.clone(),
                    line_spacing_fraction,
                });
            }
        }
        if self.finite(&format!("{at}.sample_mm"), photo.sample_mm) && photo.sample_mm <= 0.0 {
            self.problems.push(Problem::PhotoSamplePositive {
                operation: operation.clone(),
                name: name.clone(),
                sample_mm: photo.sample_mm,
            });
        }
        if self.finite(
            &format!("{at}.stepover_retract_mm"),
            photo.stepover_retract_mm,
        ) && photo.stepover_retract_mm <= 0.0
        {
            self.problems.push(Problem::PhotoRetractPositive {
                operation: operation.clone(),
                name: name.clone(),
                stepover_retract_mm: photo.stepover_retract_mm,
            });
        }
        self.non_negative(
            &format!("{at}.through_cut_allowance_mm"),
            photo.through_cut_allowance_mm,
        );
        self.positive(&format!("{at}.safe_z_mm"), photo.safe_z_mm);
        if photo.enabled {
            self.enabled_photo_v_carve(p, op, photo, tool);
        }
    }

    /// The rules of an enabled Photo V-Carve that need its tool and the
    /// project (format 27), as a V-Carve's ([`Self::enabled_v_carve`]):
    /// it carves with a V-bit; a start depth below the stock top is
    /// cleared by an earlier floor that holds the picture's footprint, or
    /// vouched for; and the bit's flanks engage no deeper than its cone,
    /// counting the start depth unless such a floor is at it.
    fn enabled_photo_v_carve(
        &mut self,
        p: &Project,
        op: &Operation,
        photo: &PhotoVCarveOp,
        tool: Option<&Tool>,
    ) {
        self.carving_tools(p, &photo.id, &photo.name, tool, &[]);

        let start_depth_mm = photo.start_depth_mm;
        if !(start_depth_mm.is_finite() && start_depth_mm >= 0.0) {
            return;
        }
        let below_top = start_depth_mm > 0.0;
        let cleared = if below_top {
            cleared_start_plane(p, op)
        } else {
            None
        };
        if below_top && cleared.is_none() && !photo.start_plane_cleared_externally {
            self.problems.push(Problem::StartDepthUncleared {
                operation: photo.id.clone(),
                name: photo.name.clone(),
                start_depth_mm,
                field: "start_depth_mm",
                footprint: StartFootprint::Picture,
            });
        }
        let cleared_at_start = cleared.is_some_and(|plane| plane.is_at(start_depth_mm));
        let from_stock_top = below_top && !cleared_at_start;
        let Some(tool) = tool else {
            return;
        };
        let ToolKind::VBit { included_angle_deg } = tool.cutter else {
            return;
        };
        if !(included_angle_deg > 0.0
            && included_angle_deg < 180.0
            && tool.diameter_mm.is_finite()
            && tool.diameter_mm > 0.0)
        {
            return;
        }
        let Some(max_depth_mm) = photo.max_depth_mm.filter(|d| d.is_finite() && *d > 0.0) else {
            return;
        };
        let engagement_mm = max_depth_mm + if from_stock_top { start_depth_mm } else { 0.0 };
        let cone_height_mm = tool.diameter_mm / 2.0 / (included_angle_deg.to_radians() / 2.0).tan();
        if engagement_mm > cone_height_mm + EPS_MM {
            self.problems.push(Problem::VBitConeExceeded {
                operation: photo.id.clone(),
                name: photo.name.clone(),
                tool_name: tool.name.clone(),
                engagement_mm,
                cone_height_mm,
                from_stock_top,
                field: "max_depth_mm",
                limit: CuttingLimit::Cone,
            });
        }
    }

    /// A bitmap's fields, in file order (format 27;
    /// `docs/specs/phase-3/design.md`, "Validation"): a size of at least
    /// one pixel each way and at most [`MAX_BITMAP_SIDE_PX`]; a footprint
    /// above 0 each way, on the coordinate range at both corners, with the
    /// picture's proportions; and a PNG that is 8-bit grayscale of that
    /// size. Only the PNG's header is read, never its image data, so the
    /// checks cost nothing per pixel.
    fn bitmap(&mut self, bitmap: &Bitmap) {
        let at = format!("bitmaps[{}]", bitmap.id);
        let (id, name) = (&bitmap.id, &bitmap.name);
        let (width_px, height_px) = (bitmap.width_px, bitmap.height_px);
        // The size as the bitmap states it: one mistake, reported once.
        let empty = width_px == 0 || height_px == 0;
        let too_large = width_px > MAX_BITMAP_SIDE_PX || height_px > MAX_BITMAP_SIDE_PX;
        if empty {
            self.problems.push(Problem::BitmapEmpty {
                bitmap: id.clone(),
                name: name.clone(),
                width_px,
                height_px,
            });
        } else if too_large {
            self.problems.push(Problem::BitmapTooLarge {
                bitmap: id.clone(),
                name: name.clone(),
                width_px,
                height_px,
                limit_px: MAX_BITMAP_SIDE_PX,
            });
        }
        let size_ok = !empty && !too_large;

        let x_ok = self.coordinate(&format!("{at}.x_mm"), bitmap.x_mm);
        let y_ok = self.coordinate(&format!("{at}.y_mm"), bitmap.y_mm);
        let mut sides_ok = true;
        for (field, value, corner_ok, corner) in [
            ("width_mm", bitmap.width_mm, x_ok, bitmap.x_mm),
            ("height_mm", bitmap.height_mm, y_ok, bitmap.y_mm),
        ] {
            let field_at = format!("{at}.{field}");
            if !self.finite(&field_at, value) {
                sides_ok = false;
            } else if value <= 0.0 {
                self.problems.push(Problem::BitmapFootprintPositive {
                    bitmap: id.clone(),
                    name: name.clone(),
                    field,
                    value,
                });
                sides_ok = false;
            } else if corner_ok && corner + value > MAX_COORDINATE_MM {
                // The far corner; the near one is reported at `x_mm` or
                // `y_mm`, and not again here.
                self.out_of_range(field_at, corner + value, BITMAP_REACH);
            }
        }
        if size_ok && sides_ok {
            let footprint = bitmap.height_mm / bitmap.width_mm;
            let pixels = f64::from(height_px) / f64::from(width_px);
            if (footprint - pixels).abs() > BITMAP_ASPECT_TOLERANCE {
                self.problems.push(Problem::BitmapFootprintAspect {
                    bitmap: id.clone(),
                    name: name.clone(),
                    width_mm: bitmap.width_mm,
                    height_mm: bitmap.height_mm,
                    width_px,
                    height_px,
                });
            }
        }

        // The stored picture, from its header alone.
        match bitmap.read_header() {
            Err(reason) => self.problems.push(Problem::BitmapUnreadable {
                bitmap: id.clone(),
                name: name.clone(),
                reason,
            }),
            Ok(header) => {
                if size_ok && (header.width_px, header.height_px) != (width_px, height_px) {
                    self.problems.push(Problem::BitmapPixelsMismatch {
                        bitmap: id.clone(),
                        name: name.clone(),
                        width_px,
                        height_px,
                        png_width_px: header.width_px,
                        png_height_px: header.height_px,
                    });
                }
                if !header.is_grayscale_8() {
                    self.problems.push(Problem::BitmapNotGrayscale8 {
                        bitmap: id.clone(),
                        name: name.clone(),
                        colour: header.colour,
                        bit_depth: header.bit_depth,
                    });
                }
            }
        }
    }

    /// The tool rules of enabled carve `operation` (`name`), a V-Carve
    /// (format 20), an Inlay (format 22) or a Photo V-Carve (format 27,
    /// which has no clearing passes): it carves with a V-bit, and
    /// each clearing pass cuts with a flat end mill no wider than the pass
    /// before (a warning).
    fn carving_tools(
        &mut self,
        p: &Project,
        operation: &OperationId,
        name: &str,
        tool: Option<&Tool>,
        clearing: &[ClearingPass],
    ) {
        let photo = p
            .operations
            .iter()
            .any(|o| matches!(o, Operation::PhotoVCarve(photo) if &photo.id == operation));
        // A V-Carve and an Inlay carve with a tapered
        // ball nose too; a Photo V-Carve only with a V-bit.
        if let Some(tool) = tool
            && !(matches!(tool.cutter, ToolKind::VBit { .. })
                || !photo && matches!(tool.cutter, ToolKind::TaperedBallNose { .. }))
        {
            self.problems.push(Problem::VCarveNeedsVBit {
                operation: operation.clone(),
                name: name.to_owned(),
                tool: tool.id.clone(),
                tool_name: tool.name.clone(),
                cutter: tool.cutter.describe(),
                photo,
            });
        }
        let mut previous: Option<&Tool> = None;
        for (i, pass) in clearing.iter().enumerate() {
            let Some(pass_tool) = p.tools.iter().find(|t| t.id == pass.tool) else {
                continue;
            };
            if !pass_tool.cutter.is_flat() {
                self.problems.push(Problem::ClearingNeedsEndMill {
                    operation: operation.clone(),
                    name: name.to_owned(),
                    pass: i,
                    tool: pass_tool.id.clone(),
                    tool_name: pass_tool.name.clone(),
                    cutter: pass_tool.cutter.describe(),
                });
            }
            if let Some(before) = previous
                && pass_tool.diameter_mm.is_finite()
                && before.diameter_mm.is_finite()
                && pass_tool.diameter_mm > before.diameter_mm
            {
                self.problems.push(Problem::ClearingNotDescending {
                    operation: operation.clone(),
                    name: name.to_owned(),
                    pass: i,
                    tool_name: pass_tool.name.clone(),
                    diameter_mm: pass_tool.diameter_mm,
                    previous_tool_name: before.name.clone(),
                    previous_diameter_mm: before.diameter_mm,
                });
            }
            previous = Some(pass_tool);
        }
    }

    /// An Inlay's own fields, in file order, then, when it is enabled, its
    /// tool, clearing, cone and plug-base rules (format 22;
    /// `docs/specs/phase-3/design.md`, "Validation" and "Algorithms →
    /// Inlay", steps 1 and 4). The depth, glue gap and plug depth are
    /// required on both boards; the margin, and the male's cone, cut-out
    /// and plug base, once the male is cut, its board's thickness set. An
    /// Inlay has no start depth, so no `start_depth_*` rule applies, and it
    /// always has a flat depth, so its clearing passes always have a floor.
    fn inlay(&mut self, p: &Project, inlay: &InlayOp, tool: Option<&Tool>, at: &str) {
        let male = inlay.cuts_male();
        self.male_board(p, inlay, at);
        let unset = |v: &mut Self, field: &'static str| {
            v.problems.push(Problem::OperationValueUnset {
                operation: inlay.id.clone(),
                name: inlay.name.clone(),
                field,
            });
        };
        match inlay.depth_mm {
            Some(depth_mm) => self.positive(&format!("{at}.depth_mm"), depth_mm),
            None => unset(self, "depth_mm"),
        }
        match inlay.glue_gap_mm {
            Some(glue_gap_mm) => {
                self.non_negative(&format!("{at}.glue_gap_mm"), glue_gap_mm);
                if let Some(depth_mm) = inlay.depth_mm
                    && depth_mm.is_finite()
                    && depth_mm > 0.0
                    && glue_gap_mm.is_finite()
                    && glue_gap_mm >= depth_mm
                {
                    self.problems.push(Problem::InlayGlueGapNotBelowDepth {
                        operation: inlay.id.clone(),
                        name: inlay.name.clone(),
                        glue_gap_mm,
                        depth_mm,
                    });
                }
            }
            None => unset(self, "glue_gap_mm"),
        }
        match inlay.plug_depth_mm {
            Some(plug_depth_mm) => {
                self.positive(&format!("{at}.{INLAY_PLUG_DEPTH}"), plug_depth_mm);
            }
            None => unset(self, INLAY_PLUG_DEPTH),
        }
        // Format 26: a drawn boundary replaces the margin and the
        // rounding, so the male needs no margin and ignores one.
        let drawn = male && inlay.has_boundary();
        match (male, inlay.margin_mm) {
            // The female does not use it.
            (false, Some(margin_mm)) => {
                self.finite(&format!("{at}.margin_mm"), margin_mm);
            }
            (false, None) => {}
            (true, Some(margin_mm)) => {
                if self.finite(&format!("{at}.margin_mm"), margin_mm) && margin_mm <= 0.0 && !drawn
                {
                    self.problems.push(Problem::InlayMarginPositive {
                        operation: inlay.id.clone(),
                        name: inlay.name.clone(),
                        margin_mm,
                    });
                }
            }
            (true, None) if drawn => {}
            (true, None) => unset(self, "margin_mm"),
        }
        // Format 24: on either side, so a pair shares it.
        if let Some(rounding_mm) = inlay.outline_rounding_mm
            && !(rounding_mm.is_finite() && rounding_mm > 0.0)
        {
            self.problems.push(Problem::InlayOutlineRounding {
                operation: inlay.id.clone(),
                name: inlay.name.clone(),
                rounding_mm,
            });
        }
        if drawn {
            for (field, set) in [
                ("margin_mm", inlay.margin_mm.is_some()),
                ("outline_rounding_mm", inlay.outline_rounding_mm.is_some()),
            ] {
                if set {
                    self.problems.push(Problem::InlayMarginIgnored {
                        operation: inlay.id.clone(),
                        name: inlay.name.clone(),
                        field,
                    });
                }
            }
            self.boundary(p, inlay);
        }
        self.clearing(p, &inlay.id, &inlay.clearing, at);
        self.v_bit_floor(
            inlay.step_over_fraction,
            inlay.through_cut_allowance_mm,
            inlay.safe_z_mm,
            at,
        );
        // Format 23: the cut-out's own fields.
        if let Some(cut_out) = &inlay.cut_out {
            if !p.tools.iter().any(|t| t.id == cut_out.tool) {
                self.problems.push(Problem::DanglingTool {
                    operation: inlay.id.clone(),
                    tool: cut_out.tool.clone(),
                });
            }
            if let Some(tabs) = &cut_out.tabs {
                self.tab_spec(tabs, &format!("{at}.cut_out.tabs"));
            }
            self.non_negative(
                &format!("{at}.cut_out.through_cut_allowance_mm"),
                cut_out.through_cut_allowance_mm,
            );
        }
        if inlay.enabled {
            self.enabled_inlay(p, inlay, tool);
            self.enabled_cut_out(p, inlay, tool);
        }
    }

    /// The rules of an enabled Inlay's cut-out that need its tools
    /// (format 23), once its male is cut: its tool is a flat
    /// end mill; and the band's flat floor ring holds the tool's diameter:
    /// `M − P·tan(α/2)` wide when a clearing pass cuts the band's edge, `M
    /// − 2·P·tan(α/2)` with a V-walled edge ([`crate::male_band`]).
    /// The ring is compared only when the values it uses passed their own
    /// checks.
    fn enabled_cut_out(&mut self, p: &Project, inlay: &InlayOp, v_bit: Option<&Tool>) {
        let Some(cut_out) = &inlay.cut_out else {
            return;
        };
        if !inlay.cuts_male() {
            return;
        }
        let Some(tool) = p.tools.iter().find(|t| t.id == cut_out.tool) else {
            return;
        };
        if !tool.cutter.is_flat() {
            self.problems.push(Problem::CutterNotFlat {
                operation: inlay.id.clone(),
                name: inlay.name.clone(),
                tool: tool.id.clone(),
                tool_name: tool.name.clone(),
                cutter: tool.cutter.describe(),
                field: "cut_out.tool",
            });
            return;
        }
        if !matches!(v_bit.map(|t| &t.cutter), Some(ToolKind::VBit { .. })) {
            return;
        }
        let Some(band) = crate::male_band(p, inlay) else {
            return;
        };
        if !(tool.diameter_mm.is_finite() && tool.diameter_mm > 0.0) {
            return;
        }
        let margin_mm = band.margin_mm;
        let needed_mm = band.cut_out_margin_needed_mm(tool.diameter_mm);
        if margin_mm + EPS_MM < needed_mm {
            self.problems.push(Problem::InlayCutOutNoRoom {
                operation: inlay.id.clone(),
                name: inlay.name.clone(),
                tool_name: tool.name.clone(),
                diameter_mm: tool.diameter_mm,
                margin_mm,
                needed_mm,
                at_boundary: matches!(band.source, crate::BandSource::Drawn { .. }),
            });
        }
    }

    /// A male Inlay's drawn boundary (format 26; design spec, "Validation",
    /// "Drawn boundary"): each entry resolves (an instance, not a target,
    /// closed outlines only), and its holes are ignored with a warning;
    /// then, if every entry resolved, the design lies strictly inside the
    /// union of the entries (`inlay_boundary_crosses_design`, with the
    /// worst point), and an entry that holds no part of the design warns.
    /// Nothing is checked before the male is cut, or without a boundary.
    fn boundary(&mut self, p: &Project, inlay: &InlayOp) {
        let mut resolved = true;
        for (index, id) in inlay.boundary.iter().enumerate() {
            let operation = inlay.id.clone();
            let name = inlay.name.clone();
            match crate::boundary_instance(p, inlay, id) {
                Ok(shape) => {
                    for &profile in &shape.holes {
                        self.problems.push(Problem::InlayBoundaryHoleIgnored {
                            operation: operation.clone(),
                            name: name.clone(),
                            index,
                            instance: id.clone(),
                            instance_name: shape.name.clone(),
                            profile,
                        });
                    }
                }
                Err(problem) => {
                    resolved = false;
                    self.problems.push(match problem {
                        crate::BoundaryProblem::Unknown { instance } => {
                            Problem::InlayBoundaryUnknown {
                                operation,
                                name,
                                index,
                                instance,
                            }
                        }
                        crate::BoundaryProblem::Open { instance, profile } => {
                            Problem::InlayBoundaryOpen {
                                operation,
                                name,
                                index,
                                instance,
                                profile,
                            }
                        }
                        crate::BoundaryProblem::IsTarget { instance } => {
                            Problem::InlayBoundaryIsTarget {
                                operation,
                                name,
                                index,
                                instance,
                            }
                        }
                        // `boundary_instance` finds no crossing: that is
                        // the whole list's.
                        crate::BoundaryProblem::CrossesDesign { .. } => continue,
                    });
                }
            }
        }
        if !resolved {
            return;
        }
        match crate::drawn_boundary(p, inlay) {
            Ok(Some(drawn)) => {
                for &index in &drawn.empty {
                    let Some(shape) = drawn.instances.get(index) else {
                        continue;
                    };
                    self.problems.push(Problem::InlayBoundaryEmpty {
                        operation: inlay.id.clone(),
                        name: inlay.name.clone(),
                        index,
                        instance: shape.instance.clone(),
                        instance_name: shape.name.clone(),
                    });
                }
            }
            Err(crate::BoundaryProblem::CrossesDesign {
                x_mm,
                y_mm,
                distance_mm,
            }) => self.problems.push(Problem::InlayBoundaryCrossesDesign {
                operation: inlay.id.clone(),
                name: inlay.name.clone(),
                x_mm,
                y_mm,
                distance_mm,
            }),
            Ok(None) | Err(_) => {}
        }
    }

    /// The rules of an enabled Inlay that need its tools and the project
    /// (format 22): its tools as a V-Carve's; each board's flank engagement
    /// from its top ([`InlayOp::engagement_mm`]) within the V-bit's cone,
    /// the female's `D` at `depth_mm` and, once the male is cut, the male's
    /// `D − G + P` at `plug_depth_mm`; and the male board under its
    /// surround's floor at least [`INLAY_MIN_PLUG_BASE_MM`] thick (a
    /// warning; the male board's thickness, not the stock's). Only values
    /// that passed their own checks are compared.
    fn enabled_inlay(&mut self, p: &Project, inlay: &InlayOp, tool: Option<&Tool>) {
        self.carving_tools(p, &inlay.id, &inlay.name, tool, &inlay.clearing);
        let depth_ok = inlay.depth_mm.is_some_and(|d| d.is_finite() && d > 0.0);
        let glue_gap_ok = inlay
            .glue_gap_mm
            .zip(inlay.depth_mm)
            .is_some_and(|(g, d)| g.is_finite() && g >= 0.0 && g < d);
        let plug_depth_ok = inlay
            .plug_depth_mm
            .is_some_and(|p| p.is_finite() && p > 0.0);
        // A V-bit's cone height, or a tapered ball nose's cutting height.
        let cone_height_mm =
            tool.and_then(|tool| Some((tool, tool.cutting_height_mm()?, tool.cutting_limit()?)));
        let mut boards = vec![(Board::Female, depth_ok)];
        if inlay.cuts_male() {
            boards.push((Board::Male, depth_ok && glue_gap_ok && plug_depth_ok));
        }
        for (board, usable) in boards {
            let Some(engagement_mm) = inlay.engagement_mm(board).filter(|_| usable) else {
                continue;
            };
            let male = board == Board::Male;
            if let Some((tool, cone_height_mm, limit)) = cone_height_mm
                && engagement_mm > cone_height_mm + EPS_MM
            {
                self.problems.push(Problem::VBitConeExceeded {
                    operation: inlay.id.clone(),
                    name: inlay.name.clone(),
                    tool_name: tool.name.clone(),
                    engagement_mm,
                    cone_height_mm,
                    from_stock_top: male,
                    field: if male { INLAY_PLUG_DEPTH } else { "depth_mm" },
                    limit,
                });
            }
            // A tapered ball nose's glue gap and plug depth below its
            // tangent height (warnings), once, with the female.
            if !male && let Some(profile) = tool.and_then(Tool::carve_profile) {
                self.inlay_ball(inlay, &profile, glue_gap_ok, plug_depth_ok);
            }
            if male
                && let Some(thickness_mm) = inlay
                    .male_board
                    .thickness_mm
                    .filter(|t| t.is_finite() && *t > 0.0)
            {
                let base_mm = thickness_mm - engagement_mm;
                if base_mm < INLAY_MIN_PLUG_BASE_MM {
                    self.problems.push(Problem::InlayPlugBaseThin {
                        operation: inlay.id.clone(),
                        name: inlay.name.clone(),
                        floor_depth_mm: engagement_mm,
                        thickness_mm,
                        base_mm,
                    });
                }
            }
        }
    }

    /// An Inlay cut with a tapered ball nose of `profile`:
    /// a `glue_gap_mm` below the bit's tangent height is
    /// `inlay_glue_gap_below_ball`, and a `plug_depth_mm` below it
    /// `inlay_plug_depth_below_ball`, each a warning naming what it costs.
    /// Only values that passed their own checks are compared.
    fn inlay_ball(
        &mut self,
        inlay: &InlayOp,
        profile: &dc_geometry::profile::BitProfile,
        glue_gap_ok: bool,
        plug_depth_ok: bool,
    ) {
        if let Some(glue_gap_mm) = inlay.glue_gap_mm.filter(|_| glue_gap_ok)
            && let Some(cost) = crate::glue_gap_below_ball(profile, glue_gap_mm)
        {
            self.problems.push(Problem::InlayGlueGapBelowBall {
                operation: inlay.id.clone(),
                name: inlay.name.clone(),
                glue_gap_mm,
                tangent_height_mm: cost.tangent_height_mm,
                lift_mm: cost.lift_mm,
                wall_gap_mm: cost.wall_gap_mm,
            });
        }
        if let Some(plug_depth_mm) = inlay.plug_depth_mm.filter(|_| plug_depth_ok)
            && let Some(cost) = crate::plug_depth_below_ball(profile, plug_depth_mm)
        {
            self.problems.push(Problem::InlayPlugDepthBelowBall {
                operation: inlay.id.clone(),
                name: inlay.name.clone(),
                plug_depth_mm,
                tangent_height_mm: cost.tangent_height_mm,
                wall_gap_mm: cost.wall_gap_mm,
            });
        }
    }

    /// An Inlay's male board (format 25): its width and height,
    /// when set, above 0 (`inlay_male_board_size`) and within
    /// [`MAX_COORDINATE_MM`]; its thickness, when set, above 0
    /// (`inlay_male_board_thickness`) and within it too. An enabled Inlay
    /// that cuts its male shares the one male board of the project: one
    /// whose board differs from the first such Inlay's is
    /// `inlay_male_boards_differ`.
    fn male_board(&mut self, p: &Project, inlay: &InlayOp, at: &str) {
        let board = &inlay.male_board;
        for (field, value) in [("width_mm", board.width_mm), ("height_mm", board.height_mm)] {
            let Some(value_mm) = value else {
                continue;
            };
            let path = format!("{at}.male_board.{field}");
            if !self.finite(&path, value_mm) {
                continue;
            }
            if value_mm <= 0.0 {
                self.problems.push(Problem::InlayMaleBoardSize {
                    operation: inlay.id.clone(),
                    name: inlay.name.clone(),
                    field,
                    value_mm,
                });
            } else if value_mm > MAX_COORDINATE_MM {
                self.out_of_range(path, value_mm, STOCK_SIDE);
            }
        }
        if let Some(thickness_mm) = board.thickness_mm {
            let path = format!("{at}.male_board.thickness_mm");
            if self.finite(&path, thickness_mm) {
                if thickness_mm <= 0.0 {
                    self.problems.push(Problem::InlayMaleBoardThickness {
                        operation: inlay.id.clone(),
                        name: inlay.name.clone(),
                        thickness_mm,
                    });
                } else if thickness_mm > MAX_COORDINATE_MM {
                    self.out_of_range(path, thickness_mm, STOCK_SIDE);
                }
            }
        }
        if !(inlay.enabled && inlay.cuts_male()) {
            return;
        }
        let Some((first, first_board)) = p.male_board() else {
            return;
        };
        if first.id == inlay.id {
            return;
        }
        let this_board = board.stock(&p.stock);
        let same = |a: Option<f64>, b: Option<f64>| match (a, b) {
            (Some(a), Some(b)) => (a - b).abs() <= EPS_MM || !(a.is_finite() && b.is_finite()),
            (None, None) => true,
            _ => false,
        };
        if !(same(Some(this_board.width_mm), Some(first_board.width_mm))
            && same(Some(this_board.height_mm), Some(first_board.height_mm))
            && same(this_board.thickness_mm, first_board.thickness_mm))
        {
            self.problems.push(Problem::InlayMaleBoardsDiffer {
                operation: inlay.id.clone(),
                name: inlay.name.clone(),
                first: first.id.clone(),
            });
        }
    }

    /// An Engrave's own fields, in file order.
    fn engrave(&mut self, op: &EngraveOp, at: &str) {
        if let Some(step) = op.step_down_mm {
            self.positive(&format!("{at}.step_down_mm"), step);
        }
        self.non_negative(
            &format!("{at}.through_cut_allowance_mm"),
            op.through_cut_allowance_mm,
        );
        self.positive(&format!("{at}.safe_z_mm"), op.safe_z_mm);
    }

    /// An entry's values (functional spec, capability 16): a ramp or helix
    /// angle in (0, 45], a helix diameter fraction in (0, 1].
    fn entry(&mut self, entry: &Entry, at: &str) {
        match entry {
            Entry::Plunge => {}
            Entry::Ramp { angle_deg } => {
                self.entry_angle(&format!("{at}.entry.ramp.angle_deg"), *angle_deg);
            }
            Entry::Helix {
                diameter_fraction,
                angle_deg,
            } => {
                let field = format!("{at}.entry.helix.diameter_fraction");
                if self.finite(&field, *diameter_fraction)
                    && !(*diameter_fraction > 0.0 && *diameter_fraction <= 1.0)
                {
                    self.out_of_range(field, *diameter_fraction, FRACTION);
                }
                self.entry_angle(&format!("{at}.entry.helix.angle_deg"), *angle_deg);
            }
        }
    }

    fn entry_angle(&mut self, field: &str, angle_deg: f64) {
        if self.finite(field, angle_deg) && !(angle_deg > 0.0 && angle_deg <= 45.0) {
            self.out_of_range(field.to_owned(), angle_deg, ENTRY_ANGLE);
        }
    }

    /// A lead's values (functional spec, capability 17): a positive radius,
    /// a sweep in (0, 180], a non-negative overcut.
    fn lead(&mut self, lead: &Lead, at: &str) {
        self.positive(&format!("{at}.lead.radius_mm"), lead.radius_mm);
        let field = format!("{at}.lead.sweep_deg");
        if self.finite(&field, lead.sweep_deg) && !(lead.sweep_deg > 0.0 && lead.sweep_deg <= 180.0)
        {
            self.out_of_range(field, lead.sweep_deg, SWEEP);
        }
        self.non_negative(&format!("{at}.lead.overcut_mm"), lead.overcut_mm);
    }

    /// Design addendum A7: the clearance height is positive and at
    /// least every enabled operation's safe Z; the tool-change machine Z is
    /// below 0 and only on a definition that documents `G53`; the park
    /// point is within the coordinate limit.
    fn heights(&mut self, p: &Project) {
        let heights = &p.heights;
        if let Some(clearance_mm) = heights.clearance_mm
            && self.coordinate(CLEARANCE, clearance_mm)
        {
            if clearance_mm <= 0.0 {
                self.out_of_range(CLEARANCE.to_owned(), clearance_mm, POSITIVE);
            }
            for op in p.operations.iter().filter(|op| op.enabled()) {
                let safe_z_mm = op.safe_z_mm();
                if safe_z_mm.is_finite() && clearance_mm < safe_z_mm {
                    self.problems.push(Problem::ClearanceBelowSafeZ {
                        operation: op.id().clone(),
                        name: op.name().to_owned(),
                        clearance_mm,
                        safe_z_mm,
                    });
                }
            }
        }
        if let Some(machine_z_mm) = heights.tool_change_machine_z_mm
            && self.coordinate(MACHINE_Z, machine_z_mm)
        {
            // Below 0 as the program writes it: not one that rounds to 0
            // in its decimal places.
            let half_quantum_mm = p.post_processor.decimal_places.quantum_mm(p.display_units) / 2.0;
            if machine_z_mm >= -half_quantum_mm {
                self.problems
                    .push(Problem::MachineZNotBelowHome { machine_z_mm });
            }
            // A reference that names no bundled definition fails to load;
            // only a project built in code gets here without one.
            let documents_g53 = p.post_processor.definition().is_ok_and(|d| {
                d.post_processor()
                    .work_coordinates
                    .machine_command
                    .is_some()
            });
            if !documents_g53 {
                self.problems.push(Problem::MachineZNeedsG53 {
                    post: p.post_processor.id.clone(),
                });
            }
        }
        if let Some(park) = heights.park {
            self.coordinate("heights.park.x_mm", park.x_mm);
            self.coordinate("heights.park.y_mm", park.y_mm);
        }
    }

    /// Reports `field` if `value` is not finite; returns whether it is.
    fn finite(&mut self, field: &str, value: f64) -> bool {
        let ok = value.is_finite();
        if !ok {
            self.problems.push(Problem::NonFinite {
                field: field.to_owned(),
            });
        }
        ok
    }

    /// Reports `field` if `value` is not finite or its magnitude exceeds
    /// [`MAX_COORDINATE_MM`]; returns whether it is in range.
    fn coordinate(&mut self, field: &str, value: f64) -> bool {
        if !self.finite(field, value) {
            return false;
        }
        let ok = value.abs() <= MAX_COORDINATE_MM;
        if !ok {
            self.out_of_range(field.to_owned(), value, COORDINATE);
        }
        ok
    }

    /// A stock side: positive and at most [`MAX_COORDINATE_MM`].
    fn stock_side(&mut self, field: &str, value: f64) {
        if self.finite(field, value) && (value <= 0.0 || value > MAX_COORDINATE_MM) {
            self.out_of_range(field.to_owned(), value, STOCK_SIDE);
        }
    }

    /// A placed tab: on an instance and profile that exist, at a point
    /// within the coordinate limit, with a positive width and height.
    fn placed_tab(&mut self, p: &Project, tab: &PlacedTab) {
        let at = format!("tabs[{}]", tab.id);
        let instance = p.instances.iter().find(|i| i.id == tab.instance);
        let profiles = instance
            .and_then(|i| p.definitions.get(&i.definition))
            .map(|d| d.profiles.as_slice());
        match (instance, profiles) {
            (None, _) => self.problems.push(Problem::TabDanglingInstance {
                tab: tab.id.clone(),
                instance: tab.instance.clone(),
            }),
            // A missing definition is reported for the instance.
            (Some(_), None) => {}
            (Some(_), Some(profiles)) => {
                if profiles.get(tab.profile).is_none() {
                    self.problems.push(Problem::TabProfileMissing {
                        tab: tab.id.clone(),
                        instance: tab.instance.clone(),
                        profile: tab.profile,
                        count: profiles.len(),
                    });
                }
            }
        }
        self.coordinate(&format!("{at}.x_mm"), tab.x_mm);
        self.coordinate(&format!("{at}.y_mm"), tab.y_mm);
        self.positive(&format!("{at}.width_mm"), tab.width_mm);
        self.positive(&format!("{at}.height_mm"), tab.height_mm);
    }

    fn positive(&mut self, field: &str, value: f64) {
        if self.finite(field, value) && value <= 0.0 {
            self.out_of_range(field.to_owned(), value, POSITIVE);
        }
    }

    fn non_negative(&mut self, field: &str, value: f64) {
        if self.finite(field, value) && value < 0.0 {
            self.out_of_range(field.to_owned(), value, NON_NEGATIVE);
        }
    }

    /// A tool value export needs: unset is an error when the tool is
    /// `in_use`, and a set one must be positive either way.
    fn required_positive(
        &mut self,
        tool: &Tool,
        in_use: bool,
        field: &'static str,
        value: Option<f64>,
    ) {
        match value {
            None if !in_use => {}
            None => self.problems.push(Problem::ToolValueUnset {
                tool: tool.id.clone(),
                name: tool.name.clone(),
                field,
            }),
            Some(v) => {
                self.tool_positive(tool, field, v);
                // Far past any hobby machine, so almost surely a typo.
                if in_use && v.is_finite() && v > MAX_RATE_MM_MIN {
                    self.tool_out_of_range(tool, field, v, RATE);
                }
            }
        }
    }

    /// A tapered ball nose's values, `[tip, taper, flutes]` (format 28):
    /// each unset one is `tool_value_unset` when the tool is `in_use`, and
    /// each set one out of range, or not a number, is
    /// `tapered_ball_nose_range` at its field, used or not, as is a cutting
    /// diameter not above the tip. A cutting diameter not above 0 is left
    /// to the rule every tool has.
    fn tapered_ball_nose(&mut self, tool: &Tool, in_use: bool, values: [Option<f64>; 3]) {
        use crate::tool::{
            MAX_TAPER_ANGLE_DEG, TBN_FLUTES, TBN_TAPER, TBN_TIP, tapered_tangent_height_mm,
        };
        let [tip, taper, flutes] = values;
        if in_use {
            for (field, value) in [(TBN_TIP, tip), (TBN_TAPER, taper), (TBN_FLUTES, flutes)] {
                if value.is_none() {
                    self.problems.push(Problem::ToolValueUnset {
                        tool: tool.id.clone(),
                        name: tool.name.clone(),
                        field,
                    });
                }
            }
        }
        let mut range = |field: &'static str, value: f64, expected: String| {
            self.problems.push(Problem::TaperedBallNoseRange {
                tool: tool.id.clone(),
                name: tool.name.clone(),
                field,
                value,
                expected,
            });
        };
        let tip = tip.filter(|&tip_mm| {
            let ok = tip_mm.is_finite() && tip_mm > 0.0;
            if !ok {
                range(TBN_TIP, tip_mm, "a tip diameter above 0 mm".into());
            }
            ok
        });
        let taper = taper.filter(|&taper_deg| {
            let ok = taper_deg > 0.0 && taper_deg <= MAX_TAPER_ANGLE_DEG;
            if !ok {
                range(
                    TBN_TAPER,
                    taper_deg,
                    format!(
                        "a taper above 0 and at most {MAX_TAPER_ANGLE_DEG} degrees per side (half the included angle)"
                    ),
                );
            }
            ok
        });
        if let Some(flutes_mm) = flutes {
            let tangent_mm = tip
                .zip(taper)
                .and_then(|(t, a)| tapered_tangent_height_mm(t, a));
            if !(flutes_mm.is_finite() && flutes_mm > 0.0) {
                range(TBN_FLUTES, flutes_mm, "a flute length above 0 mm".into());
            } else if let Some(tangent_mm) = tangent_mm
                && flutes_mm <= tangent_mm
            {
                range(
                    TBN_FLUTES,
                    flutes_mm,
                    format!(
                        "a flute length above {tangent_mm:.3} mm, the height where the ball meets the taper"
                    ),
                );
            }
        }
        if let Some(tip_mm) = tip
            && tool.diameter_mm.is_finite()
            && tool.diameter_mm > 0.0
            && tool.diameter_mm <= tip_mm
        {
            range(
                "diameter_mm",
                tool.diameter_mm,
                format!(
                    "a cutting diameter above the {tip_mm} mm tip diameter; diameter_mm is the widest the bit cuts, where its taper ends"
                ),
            );
        }
    }

    /// [`Self::positive`] for a tool's field, naming the tool.
    fn tool_positive(&mut self, tool: &Tool, field: &'static str, value: f64) {
        if self.finite(&format!("tools[{}].{field}", tool.id), value) && value <= 0.0 {
            self.tool_out_of_range(tool, field, value, POSITIVE);
        }
    }

    fn tool_out_of_range(
        &mut self,
        tool: &Tool,
        field: &'static str,
        value: f64,
        expected: &'static str,
    ) {
        self.problems.push(Problem::ToolOutOfRange {
            tool: tool.id.clone(),
            name: tool.name.clone(),
            field,
            value,
            expected,
        });
    }

    fn out_of_range(&mut self, field: String, value: f64, expected: &'static str) {
        self.problems.push(Problem::OutOfRange {
            field,
            value,
            expected,
        });
    }
}

#[cfg(test)]
mod tests;
