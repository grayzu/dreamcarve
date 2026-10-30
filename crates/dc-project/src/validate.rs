// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Project validation (`docs/specs/phase-1/design.md`, "Data model →
//! dc-project"). [`validate`] reports every problem it finds rather than
//! stopping at the first, so the CLI `check` command can print them all.
//!
//! Error-class problems make a project unsafe or impossible to machine
//! (dangling ids, non-positive dimensions, non-finite numbers, coordinates,
//! transforms and stock sides beyond [`MAX_COORDINATE_MM`], and every value
//! the user must supply that is unset: the stock thickness, each operation's
//! depth, each tool's feed, plunge rate and, under automatic spindle control,
//! RPM; nothing may default them, #64, #76). A tool's rates count only when
//! an enabled operation uses it: an unused tool is one warning instead, and
//! its unset rates come back as errors once an operation that uses it is
//! enabled. Warning-class problems still allow a correct result but need the
//! user's attention (disabled operations, unused tools, open paths).
//!
//! Sanity checks (#67) catch values that are valid but dangerous, and look
//! only at enabled operations and the tools they use. Errors: a feed or
//! plunge rate above [`MAX_RATE_MM_MIN`], an Outside, Inside, Left or
//! Right Profile cut with a cutter other than a flat end mill, and an
//! enabled operation with
//! no targets. Warnings: a step-down deeper than the
//! tool's diameter, a plunge rate above the feed rate, a spindle speed
//! above [`MAX_RPM`], and a diameter above [`MAX_TOOL_DIAMETER_MM`]. None
//! changes a value; they only report.
//!
//! Phase 2 (#117; `docs/specs/phase-2/design.md`, "Tool numbers" and
//! "Validation") adds the rules for tool numbers and for the Pocket, Drill
//! and Engrave operations, entries, leads and corner relief. Errors: two
//! used tools resolving to one `T` number, or a number of 0; a `rest_of`
//! that is not an earlier enabled Pocket with the same targets; a
//! step-over fraction outside (0, 1]; a Pocket, Engrave or peck Drill with
//! no step-down anywhere; lead, entry and raster values out of range; a
//! helix on an On line, Left or Right Profile; canned cycles on a definition without
//! them; a Pocket with a cutter that is not a flat end mill. Warnings: a
//! Drill with a cutter other than a drill or end mill; corner relief on a
//! tool with no step-down; a V-bit engraving deeper than its cone.
//!
//! #283 (design addendum A7) adds the heights. Errors: a clearance height
//! below an enabled operation's safe Z; a tool-change machine Z of 0 or
//! more, or on a definition that documents no `G53`; a park point beyond
//! the coordinate limit.
//!
//! Format 20 (`docs/specs/phase-3/design.md`, "Validation") adds the
//! V-Carve. Errors: a V-Carve whose tool is not a V-bit, or whose V-bit's
//! angle is not in (0, 180); a flat depth (plus the start depth, unless an
//! enabled earlier Pocket at that depth contains the targets) deeper than
//! the bit's cone; clearing passes without a flat depth, or naming a
//! missing tool or one that is not a flat end mill; a negative start
//! depth; a start depth below the stock top that no earlier Pocket (nor,
//! since #426, an earlier V-Carve's flat floor) clears and the user does
//! not vouch for; a flatten tolerance above
//! [`MAX_FLATTEN_TOLERANCE_MM`]. Warning: a clearing pass wider than the
//! one before.
//!
//! [`Problem::field`] names where in the file each problem is, so the CLI
//! can turn it into a `file:line:column` with [`crate::SourceMap`].

use crate::countersink::{COUNTERSINK_TOLERANCE_MM, countersink_depth_mm};
use crate::ids::{DefinitionId, InstanceId, OperationId, TabId, ToolId};
use crate::model::{
    BoreOp, ClearanceStrategy, ClearingPass, CornerRelief, DrillCycle, DrillOp, EngraveOp, Entry,
    InlayCutOut, InlayOp, InlaySide, LastPass, Lead, Operation, OriginXy, PeckRetract, PlacedTab,
    PocketOp, ProfileOp, Project, Side, TabPlacement, TabSpec, Tool, ToolKind, VCarveOp,
    VCarveStrategy,
};
use crate::post_processor::{
    MAX_FILE_EXTENSION_CHARS, SpindleControl, ToolChangeMode, is_safe_file_extension,
};
use crate::project::cleared_start_plane;
use dc_geometry::{EPS_MM, GeometryError, MAX_COORDINATE_MM};
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt;

/// How serious a [`Problem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// The project cannot be machined safely as it stands.
    Error,
    /// The user should know, but the project is still usable.
    Warning,
}

/// One finding of [`validate`].
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// A definition's `id` differs from its key in `definitions`.
    DefinitionKeyMismatch {
        /// The map key.
        key: DefinitionId,
        /// The `id` field inside the definition.
        id: DefinitionId,
    },
    /// Two instances share an id.
    DuplicateInstanceId {
        /// The repeated id.
        id: InstanceId,
    },
    /// Two tools share an id.
    DuplicateToolId {
        /// The repeated id.
        id: ToolId,
    },
    /// Two operations share an id.
    DuplicateOperationId {
        /// The repeated id.
        id: OperationId,
    },
    /// An instance places a definition that does not exist.
    DanglingDefinition {
        /// The instance holding the reference.
        instance: InstanceId,
        /// The missing definition.
        definition: DefinitionId,
    },
    /// An operation cuts with a tool that does not exist.
    DanglingTool {
        /// The operation holding the reference.
        operation: OperationId,
        /// The missing tool.
        tool: ToolId,
    },
    /// An operation targets an instance that does not exist.
    DanglingInstance {
        /// The operation holding the reference.
        operation: OperationId,
        /// The missing instance.
        instance: InstanceId,
    },
    /// Two placed tabs share an id (#105).
    DuplicateTabId {
        /// The repeated id.
        id: TabId,
    },
    /// A placed tab is on an instance that does not exist (#105).
    TabDanglingInstance {
        /// The tab.
        tab: TabId,
        /// The missing instance.
        instance: InstanceId,
    },
    /// A placed tab is on a profile its instance's definition does not
    /// have (#105).
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
    OutOfRange {
        /// Path to the field, such as `tools[t1].diameter_mm`.
        field: String,
        /// The value found.
        value: f64,
        /// The allowed range, in words.
        expected: &'static str,
    },
    /// A number is NaN or infinite.
    NonFinite {
        /// Path to the field.
        field: String,
    },
    /// An operation is disabled and will not be exported.
    OperationDisabled {
        /// The disabled operation.
        operation: OperationId,
    },
    /// A tool has no value for a field export needs (feed, plunge or RPM),
    /// or no step-down while an enabled operation sets none either (#98).
    /// An error since #76, so `check` lists every unset value in one run.
    ToolValueUnset {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The unset field, such as `feed_mm_min`.
        field: &'static str,
    },
    /// A tool no enabled operation uses. Its unset rates are not reported
    /// (#84): nothing cuts with it, and they are never defaulted.
    UnusedTool {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
    },
    /// The stock thickness is unset. It sets depth, tab heights and, with
    /// Z zero at the stock bottom, safe Z, so it is never defaulted (#64).
    StockThicknessUnset,
    /// An operation has no value for a field export needs (its depth).
    OperationValueUnset {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
        /// The unset field, such as `depth_mm`.
        field: &'static str,
    },
    /// A tool's number is outside the range its field allows.
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
    /// cut it (#175, #207).
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
    UnsafeFileExtension {
        /// The extension found.
        extension: String,
    },
    /// The tool-change mode is `"m6"`, but the post-processor definition's
    /// tool change has no `M6`, so it cannot change tools within a program
    /// (#95). GRBL 1.1 has none.
    ToolChangeModeNeedsM6 {
        /// The post-processor's id.
        post: String,
    },
    /// An enabled operation cuts deeper per pass than its tool is wide
    /// (#67). A warning: the program is still what was asked for.
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
    /// that is not a flat end mill (#67, #207). Those sides offset by half
    /// the diameter, so any
    /// other cutter makes the part the wrong size: an error. On the line the
    /// tool centre follows the line, so any cutter is allowed. So is a male
    /// Inlay's cut-out (format 23, #443), at `cut_out.tool`.
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
    /// and stop the spindle without moving (#67). An error.
    OperationHasNoTargets {
        /// The operation.
        operation: OperationId,
        /// The operation's display name.
        name: String,
    },
    /// A used tool's spindle speed is above [`MAX_RPM`] (#67). A warning:
    /// GRBL and grblHAL clamp `S` to the machine's maximum (`$30`), and
    /// under manual control the speed is only a hint.
    SpindleSpeedHigh {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The speed set, RPM.
        rpm: u32,
    },
    /// A used tool plunges faster than it cuts (#67), which usually means
    /// the two rates are swapped. A warning.
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
    /// A used tool's diameter is above [`MAX_TOOL_DIAMETER_MM`] (#67). A
    /// warning: a shape such a bit does not fit is already skipped with a
    /// warning, and around a part it cuts valid geometry.
    LargeDiameter {
        /// The tool.
        tool: ToolId,
        /// The tool's display name.
        name: String,
        /// The diameter, mm.
        diameter_mm: f64,
    },
    /// Two tools of enabled operations resolve to the same `T` number,
    /// set or positional (#117, functional spec capability 21a): the
    /// program would change to the wrong bit. An error at the later tool.
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
    /// targets (#117, functional spec capability 13). An error.
    RestOfInvalid {
        /// The Pocket holding the reference.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The operation named.
        rest_of: OperationId,
    },
    /// An On line, Left or Right Profile asks for a helix entry (#117,
    /// functional spec capability 16; #207). A helix sits in material the
    /// operation removes, and those cuts remove only their kerf. An error.
    HelixNeedsWasteSide {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
    },
    /// Canned cycles are on, but the post-processor definition has none
    /// (#117, functional spec capability 14), so the choice could only be
    /// ignored. An error.
    CannedCyclesUnsupported {
        /// The post-processor's id.
        post: String,
    },
    /// An enabled Drill cuts with a cutter other than a drill or flat end
    /// mill (#117): a V-bit or ball nose makes a cone, not a hole. A
    /// warning.
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
    /// step-down is unset (#117): the relief pass steps down at the
    /// tool's step-down, so it is skipped. A warning.
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
    /// The clearance height is below an enabled operation's safe Z (#283):
    /// a link between parts would run lower than that operation's links
    /// between passes. An error.
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
    MachineZNotBelowHome {
        /// The value, mm.
        machine_z_mm: f64,
    },
    /// `heights.tool_change_machine_z_mm` is set, but the post-processor
    /// documents no machine-coordinate move (`work_coordinates.
    /// machine_command` is `null`), so the lift cannot be written (#283).
    /// An error.
    MachineZNeedsG53 {
        /// The post-processor's id.
        post: String,
    },
    /// An enabled Engrave with a V-bit cuts deeper than the bit's cone is
    /// tall (#117), so the shank, not the cone, would cut. A warning.
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
    /// An enabled Profile's last pass stock to leave is not more than 0 and at
    /// most a third of its bit's diameter (#284; VCarve advises keeping it
    /// well below a third). An error.
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
    /// A Profile has a last pass but cuts On line (#284): it cuts with
    /// both flanks of the bit, so it leaves no wall for the last pass to
    /// take the stock to leave off. An error. (changed in #311: Left and Right,
    /// which cut one flank against the part, take a last pass.)
    LastPassNeedsSide {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The side it cuts.
        side: Side,
    },
    /// An enabled Profile's last pass is thicker than its step-down in
    /// effect, the operation's or else the tool's (#310 review): it would
    /// cut that much in one pass, where every other pass steps down less.
    /// An error.
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
    /// (format 18; maintainer decision 2026-10-24): one turn of the helix
    /// would cut deeper than a pass. An error.
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
    /// (format 20; phase 3 security review L6): the safety checks' slack
    /// and the medial axis's chords scale with it. An error.
    FlattenToleranceRange {
        /// The value set, mm.
        flatten_tolerance_mm: f64,
    },
    /// An enabled V-Carve or Inlay (format 22) cuts with a tool that is
    /// not a V-bit (format 20; functional spec, capability 38): its depth
    /// follows the bit's angle. An error.
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
    },
    /// A V-bit an enabled V-Carve, countersink (format 21) or Inlay (format
    /// 22) cuts with has an included angle not above 0 degrees and below
    /// 180 (format 20). Reported instead of
    /// `tool_out_of_range` for such a tool. An error.
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
    },
    /// A V-Carve has clearing passes but no flat depth (format 20;
    /// capability 40): with no floor there is nothing to clear. An error.
    ClearingNeedsFlatDepth {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
    },
    /// An enabled V-Carve's clearing pass cuts with a tool that is not a
    /// flat end mill (format 20; capability 40): it is cut as a Pocket,
    /// which offsets by half the diameter. An error.
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
    /// An enabled V-Carve or countersink Drill starts below the stock top,
    /// no enabled earlier Pocket, nor the flat floor of an enabled earlier
    /// V-Carve (#426), holds its footprint, and it does not
    /// say another program cleared the start plane (format 20; capability
    /// 41; phase 3 security review M2). An error.
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
    },
    /// An enabled countersink Drill cuts with a tool that is not a V-bit
    /// (format 21; capability 44): its depth follows the bit's angle. An
    /// error.
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
    InlayMarginPositive {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
        /// The margin set, mm.
        margin_mm: f64,
    },
    /// A male Inlay leaves less than [`INLAY_MIN_PLUG_BASE_MM`] of board
    /// under its surround's floor (format 22; capability 45): the board
    /// the plug stands on may flex or crack when it is pressed into the
    /// glue. It still cuts. A warning.
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
    /// A female Inlay has a cut-out (format 23, #443): only the male has a
    /// band to cut its plug out of. An error.
    InlayCutOutNeedsMale {
        /// The operation.
        operation: OperationId,
        /// Its display name.
        name: String,
    },
    /// A male Inlay's cut-out tool does not fit the band's flat floor
    /// (format 23, #443): the floor ring, between the mirrored design
    /// grown by `P·tan(α/2)` and by `M − P·tan(α/2)`, is narrower than the
    /// tool, so a cut-out half the margin out would cut into the plug's
    /// wall or the band's outer wall. An error.
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
        /// The least margin with room, mm: `2·P·tan(α/2)` plus the
        /// diameter.
        needed_mm: f64,
    },
}

/// The fastest feed or plunge rate a used tool may have, mm/min (#67).
/// Above it export fails: no hobby machine moves that fast, so the value is
/// almost surely a typo. Raise it if users report real machines beyond it.
pub const MAX_RATE_MM_MIN: f64 = 20_000.0;

/// Above this spindle speed a used tool gets a warning, RPM (#67).
pub const MAX_RPM: u32 = 60_000;

/// Above this diameter a used tool gets a warning, mm (#67).
pub const MAX_TOOL_DIAMETER_MM: f64 = 50.0;

/// The thinnest board a male Inlay may leave under its surround's floor
/// without a warning, mm (format 22; `docs/specs/phase-3/design.md`,
/// "Algorithms → Inlay", step 4): `inlay_plug_base_thin` below it.
pub const INLAY_MIN_PLUG_BASE_MM: f64 = 6.0;

/// The coarsest `flatten_tolerance_mm` a project may set, mm (format 20;
/// phase 3 security review L6): the safety checks' slack and the medial
/// axis's chords scale with it.
pub const MAX_FLATTEN_TOLERANCE_MM: f64 = 0.1;

impl Problem {
    /// The stable `snake_case` code of the problem, the variant's name:
    /// what `check` and `export` print before a warning's message
    /// (phase 2 functional spec, capability 25).
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::DefinitionKeyMismatch { .. } => "definition_key_mismatch",
            Self::DuplicateInstanceId { .. } => "duplicate_instance_id",
            Self::DuplicateToolId { .. } => "duplicate_tool_id",
            Self::DuplicateOperationId { .. } => "duplicate_operation_id",
            Self::DanglingDefinition { .. } => "dangling_definition",
            Self::DanglingTool { .. } => "dangling_tool",
            Self::DanglingInstance { .. } => "dangling_instance",
            Self::DuplicateTabId { .. } => "duplicate_tab_id",
            Self::TabDanglingInstance { .. } => "tab_dangling_instance",
            Self::TabProfileMissing { .. } => "tab_profile_missing",
            Self::OutOfRange { .. } => "out_of_range",
            Self::NonFinite { .. } => "non_finite",
            Self::OperationDisabled { .. } => "operation_disabled",
            Self::ToolValueUnset { .. } => "tool_value_unset",
            Self::UnusedTool { .. } => "unused_tool",
            Self::StockThicknessUnset => "stock_thickness_unset",
            Self::OperationValueUnset { .. } => "operation_value_unset",
            Self::ToolOutOfRange { .. } => "tool_out_of_range",
            Self::OpenProfile { .. } => "open_profile",
            Self::UnsafeFileExtension { .. } => "unsafe_file_extension",
            Self::ToolChangeModeNeedsM6 { .. } => "tool_change_mode_needs_m6",
            Self::StepDownAboveDiameter { .. } => "step_down_above_diameter",
            Self::CutterNotFlat { .. } => "cutter_not_flat",
            Self::OperationHasNoTargets { .. } => "operation_has_no_targets",
            Self::SpindleSpeedHigh { .. } => "spindle_speed_high",
            Self::PlungeAboveFeed { .. } => "plunge_above_feed",
            Self::LargeDiameter { .. } => "large_diameter",
            Self::DuplicateToolNumber { .. } => "duplicate_tool_number",
            Self::RestOfInvalid { .. } => "rest_of_invalid",
            Self::HelixNeedsWasteSide { .. } => "helix_needs_waste_side",
            Self::CannedCyclesUnsupported { .. } => "canned_cycles_unsupported",
            Self::DrillCutterUnusual { .. } => "drill_cutter_unusual",
            Self::ReliefNeedsStepDown { .. } => "relief_needs_step_down",
            Self::EngraveDeeperThanCone { .. } => "engrave_deeper_than_cone",
            Self::LastPassStockToLeave { .. } => "last_pass_stock_to_leave",
            Self::LastPassNeedsSide { .. } => "last_pass_needs_side",
            Self::LastPassThickerThanStepDown { .. } => "last_pass_thicker_than_step_down",
            Self::BorePitchOverStepDown { .. } => "bore_pitch_over_step_down",
            Self::FlattenToleranceRange { .. } => "flatten_tolerance_range",
            Self::VCarveNeedsVBit { .. } => "v_carve_needs_v_bit",
            Self::VBitAngleRange { .. } => "v_bit_angle_range",
            Self::VBitConeExceeded { .. } => "v_bit_cone_exceeded",
            Self::ClearingNeedsFlatDepth { .. } => "clearing_needs_flat_depth",
            Self::ClearingNeedsEndMill { .. } => "clearing_needs_end_mill",
            Self::ClearingDanglingTool { .. } => "clearing_dangling_tool",
            Self::ClearingNotDescending { .. } => "clearing_not_descending",
            Self::StartDepthNegative { .. } => "start_depth_negative",
            Self::StartDepthUncleared { .. } => "start_depth_uncleared",
            Self::CountersinkNeedsVBit { .. } => "countersink_needs_v_bit",
            Self::CountersinkDiameterPositive { .. } => "countersink_diameter_positive",
            Self::CountersinkMouthExceedsTool { .. } => "countersink_mouth_exceeds_tool",
            Self::CountersinkIgnoresToolWidth { .. } => "countersink_ignores_tool_width",
            Self::InlayGlueGapNotBelowDepth { .. } => "inlay_glue_gap_not_below_depth",
            Self::InlayMarginPositive { .. } => "inlay_margin_positive",
            Self::InlayPlugBaseThin { .. } => "inlay_plug_base_thin",
            Self::InlayCutOutNeedsMale { .. } => "inlay_cut_out_needs_male",
            Self::InlayCutOutNoRoom { .. } => "inlay_cut_out_no_room",
            Self::ClearanceBelowSafeZ { .. } => "clearance_below_safe_z",
            Self::MachineZNotBelowHome { .. } => "machine_z_not_below_home",
            Self::MachineZNeedsG53 { .. } => "machine_z_needs_g53",
        }
    }

    /// Whether this problem is an error or a warning.
    pub fn severity(&self) -> Severity {
        match self {
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
            | Self::InlayPlugBaseThin { .. } => Severity::Warning,
            Self::CutterNotFlat { .. }
            | Self::InlayCutOutNeedsMale { .. }
            | Self::InlayCutOutNoRoom { .. }
            | Self::OperationHasNoTargets { .. }
            | Self::DuplicateToolNumber { .. }
            | Self::RestOfInvalid { .. }
            | Self::HelixNeedsWasteSide { .. }
            | Self::CannedCyclesUnsupported { .. }
            | Self::LastPassStockToLeave { .. }
            | Self::LastPassNeedsSide { .. }
            | Self::LastPassThickerThanStepDown { .. }
            | Self::BorePitchOverStepDown { .. }
            | Self::FlattenToleranceRange { .. }
            | Self::VCarveNeedsVBit { .. }
            | Self::VBitAngleRange { .. }
            | Self::VBitConeExceeded { .. }
            | Self::ClearingNeedsFlatDepth { .. }
            | Self::ClearingNeedsEndMill { .. }
            | Self::ClearingDanglingTool { .. }
            | Self::StartDepthNegative { .. }
            | Self::StartDepthUncleared { .. }
            | Self::CountersinkNeedsVBit { .. }
            | Self::CountersinkDiameterPositive { .. }
            | Self::CountersinkMouthExceedsTool { .. }
            | Self::InlayGlueGapNotBelowDepth { .. }
            | Self::InlayMarginPositive { .. }
            | Self::ClearanceBelowSafeZ { .. }
            | Self::MachineZNotBelowHome { .. }
            | Self::MachineZNeedsG53 { .. } => Severity::Error,
            Self::DefinitionKeyMismatch { .. }
            | Self::DuplicateInstanceId { .. }
            | Self::DuplicateToolId { .. }
            | Self::DuplicateOperationId { .. }
            | Self::DanglingDefinition { .. }
            | Self::DanglingTool { .. }
            | Self::DanglingInstance { .. }
            | Self::DuplicateTabId { .. }
            | Self::TabDanglingInstance { .. }
            | Self::TabProfileMissing { .. }
            | Self::OutOfRange { .. }
            | Self::NonFinite { .. }
            | Self::ToolValueUnset { .. }
            | Self::StockThicknessUnset
            | Self::OperationValueUnset { .. }
            | Self::ToolOutOfRange { .. }
            | Self::UnsafeFileExtension { .. }
            | Self::ToolChangeModeNeedsM6 { .. } => Severity::Error,
        }
    }

    /// Whether this problem is error-class.
    pub fn is_error(&self) -> bool {
        self.severity() == Severity::Error
    }

    /// Whether this problem is a value the user must supply and has not:
    /// the list `init` prints (#76).
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
            Self::ToolValueUnset { tool, field, .. } | Self::ToolOutOfRange { tool, field, .. } => {
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
            Self::StepDownAboveDiameter { field, .. } => Some(field.clone()),
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
            Self::InlayPlugBaseThin { operation, .. } => {
                Some(format!("operations[{operation}].{INLAY_PLUG_DEPTH}"))
            }
            Self::InlayCutOutNeedsMale { operation, .. } => {
                Some(format!("operations[{operation}].cut_out"))
            }
            Self::InlayCutOutNoRoom { operation, .. } => {
                Some(format!("operations[{operation}].margin_mm"))
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
        _ => "mm",
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DefinitionKeyMismatch { key, id } => {
                write!(f, "definition keyed `{key}` has id `{id}`")
            }
            Self::DuplicateInstanceId { id } => write!(f, "instance id `{id}` is used twice"),
            Self::DuplicateToolId { id } => write!(f, "tool id `{id}` is used twice"),
            Self::DuplicateOperationId { id } => write!(f, "operation id `{id}` is used twice"),
            Self::DanglingDefinition {
                instance,
                definition,
            } => write!(
                f,
                "instance `{instance}` places definition `{definition}`, which does not exist"
            ),
            Self::DanglingTool { operation, tool } => write!(
                f,
                "operation `{operation}` uses tool `{tool}`, which does not exist"
            ),
            Self::DanglingInstance {
                operation,
                instance,
            } => write!(
                f,
                "operation `{operation}` targets instance `{instance}`, which does not exist"
            ),
            Self::DuplicateTabId { id } => write!(f, "two tabs have the id `{id}`"),
            Self::TabDanglingInstance { tab, instance } => write!(
                f,
                "tab `{tab}` is on instance `{instance}`, which does not exist"
            ),
            Self::TabProfileMissing {
                tab,
                instance,
                profile,
                count,
            } => write!(
                f,
                "tab `{tab}` is on profile {profile} of instance `{instance}`, whose definition has {count} profile(s), numbered from 0"
            ),

            Self::OutOfRange {
                field,
                value,
                expected,
            } => write!(f, "{field} is {value}, expected {expected}"),
            Self::NonFinite { field } => write!(f, "{field} is not a finite number"),
            Self::OperationDisabled { operation } => {
                write!(
                    f,
                    "operation `{operation}` is disabled and will not be exported"
                )
            }
            Self::ToolValueUnset { tool, name, field } => write!(
                f,
                "tools[{tool}].{field} is not set ({}) for tool `{name}`",
                unset_unit(field)
            ),
            Self::UnusedTool { tool, name } => write!(
                f,
                "tool `{tool}` (`{name}`) is defined but not used by any enabled operation"
            ),
            Self::StockThicknessUnset => write!(f, "{STOCK_THICKNESS} is not set (mm)"),
            Self::OperationValueUnset {
                operation,
                name,
                field,
            } => write!(
                f,
                "operations[{operation}].{field} is not set ({}) for operation `{name}`",
                unset_unit(field)
            ),
            Self::ToolOutOfRange {
                tool,
                name,
                field,
                value,
                expected,
            } => write!(
                f,
                "tool `{name}` (`{tool}`): {field} is {value}, expected {expected}"
            ),
            Self::OpenProfile {
                definition,
                name,
                profile,
            } => write!(
                f,
                "`{name}` (definition `{definition}`, profile {profile}) is an open path and no operation cuts it; only an On line, Left or Right Profile or an Engrave cuts an open path"
            ),
            Self::UnsafeFileExtension { extension } => write!(
                f,
                "the post-processor's file extension `{}` is not allowed: it must be 1 to {MAX_FILE_EXTENSION_CHARS} letters or digits and not `dcv`",
                extension.escape_default()
            ),
            Self::ToolChangeModeNeedsM6 { post } => write!(
                f,
                "tool_change_mode is \"m6\", but post-processor `{post}` has no M6 in its tool change, so it cannot change tools within one program; use \"separate_programs\""
            ),
            Self::StepDownAboveDiameter {
                operation,
                name,
                tool_name,
                step_down_mm,
                diameter_mm,
                ..
            } => write!(
                f,
                "operation `{name}` (`{operation}`) cuts {step_down_mm} mm per pass, deeper than the {diameter_mm} mm diameter of tool `{tool_name}`; passes that deep can break the bit, so use a step-down of at most {diameter_mm} mm"
            ),
            Self::CutterNotFlat {
                operation,
                name,
                tool,
                tool_name,
                cutter,
                field,
            } if *field != "tool" => write!(
                f,
                "operation `{name}` (`{operation}`) cuts its plug's cut-out with tool `{tool_name}` (`{tool}`), {cutter}; the cut-out runs through the board in the band's flat floor, which only a flat end mill cuts with a straight wall at every depth. Use an end mill"
            ),
            Self::CutterNotFlat {
                operation,
                name,
                tool,
                tool_name,
                cutter,
                ..
            } => write!(
                f,
                "operation `{name}` (`{operation}`) cuts with tool `{tool_name}` (`{tool}`), {cutter}; an Inside, Outside, Left or Right Profile and a Pocket offset the path by half the diameter, which only a flat end mill cuts at every depth, so the part would come out the wrong size. Use an end mill"
            ),
            Self::OperationHasNoTargets { operation, name } => write!(
                f,
                "operation `{name}` (`{operation}`) is enabled but has no targets, so it would cut nothing; add targets or disable it"
            ),
            Self::SpindleSpeedHigh { tool, name, rpm } => write!(
                f,
                "tool `{name}` (`{tool}`) is set to {rpm} RPM, faster than the {MAX_RPM} RPM a hobby spindle or router turns; check it for a typo"
            ),
            Self::PlungeAboveFeed {
                tool,
                name,
                plunge_mm_min,
                feed_mm_min,
            } => write!(
                f,
                "tool `{name}` (`{tool}`) plunges at {plunge_mm_min} mm/min, faster than it cuts ({feed_mm_min} mm/min); a plunge rate is usually well below the feed rate, so check the two are not swapped"
            ),
            Self::LargeDiameter {
                tool,
                name,
                diameter_mm,
            } => write!(
                f,
                "tool `{name}` (`{tool}`) is {diameter_mm} mm in diameter, larger than the {MAX_TOOL_DIAMETER_MM} mm of a hobby router bit; check it for a typo"
            ),
            Self::DuplicateToolNumber {
                tool,
                name,
                number,
                other,
            } => write!(
                f,
                "tool `{name}` (`{tool}`) and tool `{other}` would both be T{number} in the program; a tool with no `number` is its position in the list, so give one of them another number"
            ),
            Self::RestOfInvalid {
                operation,
                name,
                rest_of,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) rests on `{rest_of}`, which is not an earlier enabled Pocket with the same targets; rest machining clears what such a Pocket left"
            ),
            Self::LastPassStockToLeave {
                operation,
                name,
                tool_name,
                stock_to_leave_mm,
                diameter_mm,
                limit_mm,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) has last_pass.stock_to_leave_mm {stock_to_leave_mm} mm with tool `{tool_name}` ({diameter_mm} mm); it must be more than 0 and at most {limit_mm:.3} mm, a third of the bit's diameter, as VCarve advises"
            ),
            Self::LastPassNeedsSide {
                operation,
                name,
                side,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) cuts {} but has a last_pass; a last pass takes the stock to leave off a wall the other passes leave, so it needs an Inside, Outside, Left or Right Profile. Set last_pass to null, or cut on one side of the line",
                match side {
                    Side::OnLine => "on the line",
                    Side::Left => "to the left of open paths",
                    Side::Right => "to the right of open paths",
                    Side::Inside => "inside",
                    Side::Outside => "outside",
                }
            ),
            Self::LastPassThickerThanStepDown {
                operation,
                name,
                thickness_mm,
                step_down_mm,
                step_down_field,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) has last_pass.thickness_mm {thickness_mm} mm, more than its {step_down_mm} mm step-down ({step_down_field}); the last pass would cut that deep in one go. Make it at most the step-down, or set it to null for the usual last pass"
            ),
            Self::BorePitchOverStepDown {
                operation,
                name,
                pitch_mm,
                step_down_mm,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) has pitch_mm {pitch_mm} mm, more than its tool's {step_down_mm} mm step-down; each turn of the helix would cut deeper than a pass. Make it at most the step-down, or set it to null to use the step-down"
            ),
            Self::FlattenToleranceRange {
                flatten_tolerance_mm,
            } => write!(
                f,
                "{FLATTEN_TOLERANCE} is {flatten_tolerance_mm} mm, above the {MAX_FLATTEN_TOLERANCE_MM} mm limit; the safety checks allow the program that far off the true curves, so a coarser tolerance could hide a cut through a wall. Use at most {MAX_FLATTEN_TOLERANCE_MM} mm"
            ),
            Self::VCarveNeedsVBit {
                operation,
                name,
                tool,
                tool_name,
                cutter,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) carves with tool `{tool_name}` (`{tool}`), {cutter}; a carve's depth follows the angle of a V-bit, so use a V-bit"
            ),
            Self::VBitAngleRange {
                tool,
                name,
                included_angle_deg,
            } => write!(
                f,
                "tool `{name}` (`{tool}`) has an included angle of {included_angle_deg} degrees; a V-Carve, an Inlay or a countersink needs a V-bit angle above 0 and below 180 degrees to work out its depth"
            ),
            Self::VBitConeExceeded {
                operation,
                name,
                tool_name,
                engagement_mm,
                cone_height_mm,
                from_stock_top,
                field,
            } => {
                write!(
                    f,
                    "operation `{name}` (`{operation}`) engages tool `{tool_name}` {engagement_mm} mm deep"
                )?;
                if *from_stock_top && *field == INLAY_PLUG_DEPTH {
                    write!(
                        f,
                        " from the stock top (its start plane, depth_mm − glue_gap_mm below the stock top, plus its plug_depth_mm; the plug above the start plane is never cleared)"
                    )?;
                } else if *from_stock_top {
                    let own = if *field == COUNTERSINK_DIAMETER {
                        "its countersink's depth"
                    } else {
                        "its flat depth"
                    };
                    write!(
                        f,
                        " from the stock top (its start depth, which no Pocket or V-Carve floor of the project clears, plus {own})"
                    )?;
                }
                write!(
                    f,
                    ", but the bit's cone is only {cone_height_mm:.3} mm tall; below that the shank cuts, so use a shallower depth or a wider bit"
                )
            }
            Self::ClearingNeedsFlatDepth { operation, name } => write!(
                f,
                "operation `{name}` (`{operation}`) has clearing passes but no flat_depth_mm; with no flat floor there is nothing for them to clear, so set a flat depth or remove the passes"
            ),
            Self::ClearingNeedsEndMill {
                operation,
                name,
                pass,
                tool,
                tool_name,
                cutter,
            } => write!(
                f,
                "clearing pass {pass} of operation `{name}` (`{operation}`) cuts with tool `{tool_name}` (`{tool}`), {cutter}; a clearing pass is cut as a Pocket, which only a flat end mill cuts to size. Use an end mill"
            ),
            Self::ClearingDanglingTool {
                operation,
                pass,
                tool,
            } => write!(
                f,
                "clearing pass {pass} of operation `{operation}` uses tool `{tool}`, which does not exist"
            ),
            Self::ClearingNotDescending {
                operation,
                name,
                pass,
                tool_name,
                diameter_mm,
                previous_tool_name,
                previous_diameter_mm,
            } => write!(
                f,
                "clearing pass {pass} of operation `{name}` (`{operation}`) uses tool `{tool_name}` ({diameter_mm} mm), wider than the pass before it (`{previous_tool_name}`, {previous_diameter_mm} mm); it still runs, but list the largest tool first so each pass clears only what the one before could not reach"
            ),
            Self::StartDepthNegative {
                operation,
                name,
                start_depth_mm,
                ..
            } => write!(
                f,
                "operation `{name}` (`{operation}`) has start_depth_mm {start_depth_mm} mm; the start plane is a depth below the stock top, 0 or more"
            ),
            Self::StartDepthUncleared {
                operation,
                name,
                start_depth_mm,
                field,
            } => {
                let what = if *field == COUNTERSINK_START_DEPTH {
                    "the whole of its V-bit at each hole"
                } else {
                    "its targets"
                };
                write!(
                    f,
                    "operation `{name}` (`{operation}`) starts {start_depth_mm} mm below the stock top, but no enabled Pocket, nor the flat floor of an enabled V-Carve, before it contains {what}; add a Pocket of that depth before it or, if another program clears the start plane, set start_plane_cleared_externally to true"
                )
            }
            Self::CountersinkNeedsVBit {
                operation,
                name,
                tool,
                tool_name,
                cutter,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) countersinks with tool `{tool_name}` (`{tool}`), {cutter}; a countersink's depth follows the angle of a V-bit, so use a V-bit"
            ),
            Self::CountersinkDiameterPositive {
                operation,
                name,
                diameter_mm,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) has a countersink diameter_mm of {diameter_mm} mm; the mouth must be wider than 0"
            ),
            Self::CountersinkMouthExceedsTool {
                operation,
                name,
                tool_name,
                diameter_mm,
                tool_diameter_mm,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) asks for a {diameter_mm} mm countersink, wider than the {tool_diameter_mm} mm tool `{tool_name}`; past its diameter a V-bit has no cutting edge, so use a mouth at most the bit's diameter or a wider bit"
            ),
            Self::InlayGlueGapNotBelowDepth {
                operation,
                name,
                glue_gap_mm,
                depth_mm,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) has glue_gap_mm {glue_gap_mm} mm, not below its depth_mm {depth_mm} mm; the male starts depth_mm − glue_gap_mm below the stock top, so its plug would have no wall to seat. Make the glue gap smaller than the depth (0 or 0.25 mm to start)"
            ),
            Self::InlayMarginPositive {
                operation,
                name,
                margin_mm,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) is a male inlay with margin_mm {margin_mm} mm; its surround is carved that far beyond the design, so it must be above 0 (5 to 7.5 mm leaves room to clamp the plug board)"
            ),
            Self::InlayPlugBaseThin {
                operation,
                name,
                floor_depth_mm,
                thickness_mm,
                base_mm,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) cuts its plug's surround {floor_depth_mm} mm below the stock top, leaving {base_mm} mm of the {thickness_mm} mm board under it, less than {INLAY_MIN_PLUG_BASE_MM} mm; the base may flex or crack when the plug is pressed into the glue. Use thicker stock, or a smaller depth_mm or plug_depth_mm"
            ),
            Self::InlayCutOutNeedsMale { operation, name } => write!(
                f,
                "operation `{name}` (`{operation}`) is a female inlay with a cut_out; only the male's plug is cut out of its board, the female's pocket stays in it. Set cut_out to null, or side to \"male\""
            ),
            Self::InlayCutOutNoRoom {
                operation,
                name,
                tool_name,
                diameter_mm,
                margin_mm,
                needed_mm,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) cuts its plug out with tool `{tool_name}`, {diameter_mm} mm across, half its margin_mm {margin_mm} mm out, in the band's flat floor; that floor is narrower than the tool, so the cut-out would cut into the plug's wall or the band's outer wall. Raise margin_mm to at least {needed_mm:.3} mm in both projects, or use a narrower end mill"
            ),
            Self::CountersinkIgnoresToolWidth { operation, name } => write!(
                f,
                "operation `{name}` (`{operation}`) countersinks, so allow_tool_wider_than_hole has no effect: a countersink's V-bit is wider than the hole by design, and a hole at least as wide as the mouth is skipped instead; set it to false"
            ),
            Self::HelixNeedsWasteSide { operation, name } => write!(
                f,
                "operation `{name}` (`{operation}`) cuts on the line or beside an open path but asks for a helix entry; a helix sits in material the cut removes, and such a cut removes only its kerf. Use \"plunge\" or a ramp"
            ),
            Self::CannedCyclesUnsupported { post } => write!(
                f,
                "use_canned_cycles is true, but post-processor `{post}` has no canned cycles, so Drill cannot write them; set it to false"
            ),
            Self::DrillCutterUnusual {
                operation,
                name,
                tool,
                tool_name,
                cutter,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) drills with tool `{tool_name}` (`{tool}`), {cutter}, which makes a cone rather than a flat-bottomed hole; a drill or an end mill is usual"
            ),
            Self::ReliefNeedsStepDown {
                operation,
                name,
                tool,
                tool_name,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) asks for corner relief, but tool `{tool_name}` (`{tool}`) has no step_down_mm, which the relief pass steps down at; the relief is skipped until it is set"
            ),
            Self::EngraveDeeperThanCone {
                operation,
                name,
                tool_name,
                depth_mm,
                cone_height_mm,
            } => write!(
                f,
                "operation `{name}` (`{operation}`) engraves {depth_mm} mm deep with tool `{tool_name}`, whose cone is only {cone_height_mm:.3} mm tall; below that the shank cuts, so use a shallower depth or a wider bit"
            ),
            Self::ClearanceBelowSafeZ {
                operation,
                name,
                clearance_mm,
                safe_z_mm,
            } => write!(
                f,
                "{CLEARANCE} is {clearance_mm} mm, below the {safe_z_mm} mm safe_z_mm of operation `{name}` (`{operation}`); links between parts would run lower than its links between passes, so raise the clearance to at least {safe_z_mm} mm"
            ),
            Self::MachineZNotBelowHome { machine_z_mm } => write!(
                f,
                "{MACHINE_Z} is {machine_z_mm} mm, which is not below 0 as the program writes it; grblHAL and GRBL, and LinuxCNC as its configuration tools set it up, home Z at the top of its travel, where machine Z is 0 or just below, so the lift must be below 0, at least the homing pull-off below it"
            ),
            Self::MachineZNeedsG53 { post } => write!(
                f,
                "{MACHINE_Z} is set, but post-processor `{post}` does not document a machine-coordinate move (G53), so the lift cannot be written; set it to null"
            ),
        }
    }
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
// Spells out `MAX_RATE_MM_MIN`; a test checks they agree.
const RATE: &str = "a value greater than 0 and at most 20000 mm/min";

#[derive(Default)]
struct Validator {
    problems: Vec<Problem>,
}

impl Validator {
    fn project(&mut self, p: &Project) {
        self.positive(FLATTEN_TOLERANCE, p.flatten_tolerance_mm);
        // Format 20 (security review L6): the safety checks' slack scales
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

        // In the order the file holds them (#76): settings, then geometry.
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
                _ => None,
            })
            .collect();
        // Tools an enabled operation takes its step-down from (#98): a
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
                Operation::VCarve(_) | Operation::Inlay(_) => false,
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
        // #117: the `T` number each used tool resolves to, set or
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
            // #117: `T0` is no tool; a set number must be positive, used
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
            self.tool_positive(tool, "diameter_mm", tool.diameter_mm);
            // #67: the sanity limits apply only to a tool something cuts
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
            // the pause message (#34), so it may be unset.
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
            // it; one error per tool, however many operations do (#98).
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
            }
        }

        // #105: placed tabs, after the operations as the file holds them.
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
        // Engrave and On line, Left and Right Profile cut open paths
        // (#175, #207), so a
        // definition an enabled one targets is not reported as having
        // paths that are never cut.
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
                    // fields only for one that fails (#71).
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
            // Unset depths are fine: phase 1 does not use them (#99).
            if let Some(z_top_mm) = inst.z_top_mm {
                self.coordinate(&format!("{at}.z_top_mm"), z_top_mm);
            }
            if let Some(z_bottom_mm) = inst.z_bottom_mm {
                let field = format!("{at}.z_bottom_mm");
                // Nothing reads them yet, but a top below the bottom is
                // never meant (#71).
                if self.coordinate(&field, z_bottom_mm)
                    && inst
                        .z_top_mm
                        .is_some_and(|top| top.is_finite() && top < z_bottom_mm)
                {
                    self.out_of_range(field, z_bottom_mm, BELOW_Z_TOP);
                }
            }
        }

        if !is_safe_file_extension(&p.post_processor.file_extension) {
            self.problems.push(Problem::UnsafeFileExtension {
                extension: p.post_processor.file_extension.clone(),
            });
        }

        // The definition's own values are checked by tests on every
        // bundled version, since a project cannot change them (#106). A
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
        // #117: canned cycles can only be written where the definition
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
    /// with `tool` when it exists (#67, #117): it has targets; a Pocket, a
    /// Bore or an Inside or Outside Profile cuts with a flat end mill; a
    /// Bore's pitch is at most the tool's step-down; a Drill
    /// cuts with a drill or end mill (a warning otherwise); the step-down
    /// in effect is at most the tool's diameter; corner relief has a tool
    /// step-down to step at; a V-bit engraves no deeper than its cone.
    fn enabled_operation(&mut self, op: &Operation, tool: Option<&Tool>, at: &str) {
        if op.targets().is_empty() {
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
            Operation::VCarve(_) | Operation::Inlay(_) => true,
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
            | Operation::Inlay(_) => false,
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
            Operation::Bore(_) | Operation::VCarve(_) | Operation::Inlay(_) => None,
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
        // #310 review: the last pass is at most one step-down thick.
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
            | Operation::Inlay(_) => CornerRelief::None,
        };
        if relief != CornerRelief::None && tool.step_down_mm.is_none() && own_step_down.is_some() {
            self.problems.push(Problem::ReliefNeedsStepDown {
                operation: op.id().clone(),
                name: op.name().to_owned(),
                tool: tool.id.clone(),
                tool_name: tool.name.clone(),
            });
        }
        // #284: VCarve advises a stock to leave well below a third of the bit.
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
        // #117: a helix needs removed material to sit in (functional spec,
        // capability 16); an On line, Left or Right cut removes only its
        // kerf (#207).
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

    /// A last pass's values that need no tool (#284): a finite stock to leave
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
        // #117: rest machining clears what an earlier Pocket of the same
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

    /// The rules of an enabled countersink Drill that need its tool and
    /// the project (format 21; `docs/specs/phase-3/design.md`,
    /// "Validation"): it cuts with a V-bit; its mouth is no wider than the
    /// bit; a start depth below the stock top is cleared by an earlier
    /// floor that holds the bit at every hole, a Pocket's or a V-Carve's
    /// flat floor (#426; [`cleared_start_plane`]), or vouched for; and the
    /// bit's flanks engage no deeper than its cone, counting the start
    /// depth unless such a floor at that depth clears the plane (security
    /// review H4). `op` is `drill` as the project holds it. A
    /// Drill of another cycle has none of these.
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
        // is the bit's disc at each hole (#425).
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
        // targets clears it, a Pocket's or a V-Carve's flat floor (#426);
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
            });
        }
        // The flanks engage from the start plane only when a floor at
        // that depth clears it (design spec, "V-Carve generator", step 3;
        // security review H4); from the stock top otherwise.
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
        let flat_depth_mm = carve.flat_depth_mm.filter(|f| f.is_finite() && *f > 0.0);
        // Without a flat depth the carve's depth follows from the shapes,
        // which only the generator knows; the start depth alone may
        // already be too much.
        let engagement_mm =
            flat_depth_mm.unwrap_or(0.0) + if from_stock_top { start_depth_mm } else { 0.0 };
        let cone_height_mm = tool.diameter_mm / 2.0 / (included_angle_deg.to_radians() / 2.0).tan();
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
            });
        }
    }

    /// The tool rules of enabled carve `operation` (`name`), a V-Carve
    /// (format 20) or an Inlay (format 22): it carves with a V-bit, and
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
        if let Some(tool) = tool
            && !matches!(tool.cutter, ToolKind::VBit { .. })
        {
            self.problems.push(Problem::VCarveNeedsVBit {
                operation: operation.clone(),
                name: name.to_owned(),
                tool: tool.id.clone(),
                tool_name: tool.name.clone(),
                cutter: tool.cutter.describe(),
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
    /// required on either side, the margin on a male only. An Inlay has
    /// no start depth, so no `start_depth_*` rule applies, and it always
    /// has a flat depth, so its clearing passes always have a floor.
    fn inlay(&mut self, p: &Project, inlay: &InlayOp, tool: Option<&Tool>, at: &str) {
        let male = inlay.side == InlaySide::Male;
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
        match (male, inlay.margin_mm) {
            // The female does not use it.
            (false, Some(margin_mm)) => {
                self.finite(&format!("{at}.margin_mm"), margin_mm);
            }
            (false, None) => {}
            (true, Some(margin_mm)) => {
                if self.finite(&format!("{at}.margin_mm"), margin_mm) && margin_mm <= 0.0 {
                    self.problems.push(Problem::InlayMarginPositive {
                        operation: inlay.id.clone(),
                        name: inlay.name.clone(),
                        margin_mm,
                    });
                }
            }
            (true, None) => unset(self, "margin_mm"),
        }
        self.clearing(p, &inlay.id, &inlay.clearing, at);
        self.v_bit_floor(
            inlay.step_over_fraction,
            inlay.through_cut_allowance_mm,
            inlay.safe_z_mm,
            at,
        );
        // Format 23 (#443): the cut-out's own fields.
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
    /// (format 23, #443): only a male has one; its tool is a flat end
    /// mill; and the band's flat floor ring, `M − 2·P·tan(α/2)` wide, holds
    /// the tool's diameter. The ring is compared only when the values it
    /// uses passed their own checks.
    fn enabled_cut_out(&mut self, p: &Project, inlay: &InlayOp, v_bit: Option<&Tool>) {
        let Some(cut_out) = &inlay.cut_out else {
            return;
        };
        if inlay.side != InlaySide::Male {
            self.problems.push(Problem::InlayCutOutNeedsMale {
                operation: inlay.id.clone(),
                name: inlay.name.clone(),
            });
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
        let Some(ToolKind::VBit { included_angle_deg }) = v_bit.map(|t| &t.cutter) else {
            return;
        };
        let (Some(plug_depth_mm), Some(margin_mm)) = (inlay.plug_depth_mm, inlay.margin_mm) else {
            return;
        };
        let usable = included_angle_deg.is_finite()
            && *included_angle_deg > 0.0
            && *included_angle_deg < 180.0
            && plug_depth_mm.is_finite()
            && plug_depth_mm > 0.0
            && margin_mm.is_finite()
            && margin_mm > 0.0
            && tool.diameter_mm.is_finite()
            && tool.diameter_mm > 0.0;
        if !usable {
            return;
        }
        let needed_mm =
            InlayCutOut::margin_needed_mm(plug_depth_mm, *included_angle_deg, tool.diameter_mm);
        if margin_mm + EPS_MM < needed_mm {
            self.problems.push(Problem::InlayCutOutNoRoom {
                operation: inlay.id.clone(),
                name: inlay.name.clone(),
                tool_name: tool.name.clone(),
                diameter_mm: tool.diameter_mm,
                margin_mm,
                needed_mm,
            });
        }
    }

    /// The rules of an enabled Inlay that need its tools and the project
    /// (format 22): its tools as a V-Carve's; its flank engagement from
    /// the stock top ([`InlayOp::engagement_mm`]; security review H4)
    /// within the V-bit's cone; and a male's board under its surround's
    /// floor at least [`INLAY_MIN_PLUG_BASE_MM`] thick (a warning). Only
    /// values that passed their own checks are compared.
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
        let male = inlay.side == InlaySide::Male;
        let usable = depth_ok && (!male || (glue_gap_ok && plug_depth_ok));
        let Some(engagement_mm) = inlay.engagement_mm().filter(|_| usable) else {
            return;
        };
        if let Some(tool) = tool
            && let ToolKind::VBit { included_angle_deg } = tool.cutter
            && included_angle_deg > 0.0
            && included_angle_deg < 180.0
            && tool.diameter_mm.is_finite()
            && tool.diameter_mm > 0.0
        {
            let cone_height_mm =
                tool.diameter_mm / 2.0 / (included_angle_deg.to_radians() / 2.0).tan();
            if engagement_mm > cone_height_mm + EPS_MM {
                self.problems.push(Problem::VBitConeExceeded {
                    operation: inlay.id.clone(),
                    name: inlay.name.clone(),
                    tool_name: tool.name.clone(),
                    engagement_mm,
                    cone_height_mm,
                    from_stock_top: male,
                    field: if male { INLAY_PLUG_DEPTH } else { "depth_mm" },
                });
            }
        }
        if male
            && let Some(thickness_mm) = p.stock.thickness_mm.filter(|t| t.is_finite() && *t > 0.0)
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

    /// #283, design addendum A7: the clearance height is positive and at
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
            // in its decimal places (#304 review).
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
                // #67: far past any hobby machine, so almost surely a typo.
                if in_use && v.is_finite() && v > MAX_RATE_MM_MIN {
                    self.tool_out_of_range(tool, field, v, RATE);
                }
            }
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
mod tests {
    use super::*;
    use crate::model::{
        ClearanceStrategy, CornerRelief, DrillCycle, Entry, LastPass, Lead, Operation, PeckRetract,
        ToolKind,
    };
    use crate::test_support::{
        profile_mut, sample_bore, sample_drill, sample_engrave, sample_pocket, sample_project,
    };
    use dc_geometry::{MAX_COORDINATE_MM, Polyline, Transform, Vertex, approx_eq};

    /// #105: a placed tab must name an instance and one of its
    /// definition's profiles, and its id must be its own.
    #[test]
    fn placed_tab_on_a_missing_outline_is_an_error() {
        use crate::test_support::sample_tab;
        let mut p = sample_project();
        p.tabs = vec![
            sample_tab("tab1", 25.0, 0.0),
            PlacedTab {
                instance: InstanceId::new("nope"),
                ..sample_tab("tab2", 0.0, 0.0)
            },
            PlacedTab {
                profile: 3,
                ..sample_tab("tab3", 0.0, 0.0)
            },
            sample_tab("tab1", 50.0, 15.0),
        ];
        let problems = validate(&p);
        assert!(
            problems.contains(&Problem::TabDanglingInstance {
                tab: TabId::new("tab2"),
                instance: InstanceId::new("nope"),
            }),
            "{problems:?}"
        );
        assert!(
            problems.contains(&Problem::TabProfileMissing {
                tab: TabId::new("tab3"),
                instance: InstanceId::new("i1"),
                profile: 3,
                count: 1,
            }),
            "{problems:?}"
        );
        assert!(
            problems.contains(&Problem::DuplicateTabId {
                id: TabId::new("tab1")
            }),
            "{problems:?}"
        );
        let dangling = problems
            .iter()
            .find(|q| matches!(q, Problem::TabDanglingInstance { .. }))
            .unwrap();
        assert!(dangling.is_error());
        assert_eq!(dangling.field().as_deref(), Some("tabs[tab2].instance"));
        assert_eq!(
            dangling.to_string(),
            "tab `tab2` is on instance `nope`, which does not exist"
        );
    }

    /// #105: a placed tab's size must be positive and its point a finite
    /// coordinate within the limit.
    #[test]
    fn placed_tab_size_and_point_are_checked() {
        use crate::test_support::sample_tab;
        let mut p = sample_project();
        p.tabs = vec![
            PlacedTab {
                width_mm: 0.0,
                height_mm: -1.0,
                ..sample_tab("tab1", 25.0, 0.0)
            },
            PlacedTab {
                x_mm: 1.0e7,
                ..sample_tab("tab2", 25.0, 0.0)
            },
        ];
        let fields: Vec<String> = errors(&validate(&p))
            .iter()
            .filter_map(|q| q.field())
            .collect();
        for field in [
            "tabs[tab1].width_mm",
            "tabs[tab1].height_mm",
            "tabs[tab2].x_mm",
        ] {
            assert!(fields.iter().any(|f| f == field), "{field}: {fields:?}");
        }
        p.tabs = vec![sample_tab("tab1", 25.0, 0.0)];
        assert_eq!(errors(&validate(&p)), Vec::<&Problem>::new());
    }

    fn errors(problems: &[Problem]) -> Vec<&Problem> {
        problems.iter().filter(|p| p.is_error()).collect()
    }

    #[test]
    fn sample_project_has_no_problems() {
        assert_eq!(validate(&sample_project()), vec![]);
    }

    #[test]
    fn dangling_tool_id_is_error() {
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.tool = ToolId::new("missing");
        let problems = validate(&p);
        // t1 is now used by nothing, which is a warning of its own.
        assert_eq!(
            problems,
            vec![
                Problem::UnusedTool {
                    tool: ToolId::new("t1"),
                    name: "6.35 mm upcut".into(),
                },
                Problem::DanglingTool {
                    operation: OperationId::new("o1"),
                    tool: ToolId::new("missing"),
                },
            ]
        );
        assert_eq!(problems[1].severity(), Severity::Error);
    }

    /// #76: an unset feed is an error, so `check` lists it with the other
    /// unset values in one run instead of failing later in export.
    #[test]
    fn unset_feed_is_an_error() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = None;
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ToolValueUnset {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
                field: "feed_mm_min",
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(
            problems[0].to_string(),
            "tools[t1].feed_mm_min is not set (mm/min) for tool `6.35 mm upcut`"
        );
    }

    #[test]
    fn unset_plunge_and_rpm_are_errors() {
        let mut p = sample_project();
        p.tools[0].plunge_mm_min = None;
        p.tools[0].rpm = None;
        let problems = validate(&p);
        assert_eq!(problems.len(), 2);
        assert_eq!(errors(&problems).len(), 2);
        assert_eq!(
            problems[1].to_string(),
            "tools[t1].rpm is not set (RPM) for tool `6.35 mm upcut`"
        );
    }

    /// #34, S3 (manual): with the router switched by hand, the speed is
    /// only a hint in the pause message, so an unset one is not reported.
    /// Feed and plunge still are.
    #[test]
    fn unset_rpm_is_not_reported_with_manual_spindle_control() {
        let mut p = sample_project();
        p.post_processor.spindle_control = SpindleControl::Manual;
        p.tools[0].rpm = None;
        assert_eq!(validate(&p), vec![]);
        p.tools[0].plunge_mm_min = None;
        assert_eq!(
            validate(&p),
            vec![Problem::ToolValueUnset {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
                field: "plunge_mm_min",
            }]
        );
    }

    #[test]
    fn dangling_definition_id_is_error() {
        let mut p = sample_project();
        p.instances[0].definition = DefinitionId::new("missing");
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::DanglingDefinition {
                instance: InstanceId::new("i1"),
                definition: DefinitionId::new("missing"),
            }]
        );
        assert!(problems[0].is_error());
    }

    #[test]
    fn dangling_instance_id_is_error() {
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.targets.push(InstanceId::new("missing"));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::DanglingInstance {
                operation: OperationId::new("o1"),
                instance: InstanceId::new("missing"),
            }]
        );
    }

    #[test]
    fn non_positive_stock_dimension_is_error() {
        let mut p = sample_project();
        p.stock.thickness_mm = Some(0.0);
        let problems = validate(&p);
        assert_eq!(problems.len(), 1);
        assert!(
            matches!(&problems[0], Problem::OutOfRange { field, .. } if field == "stock.thickness_mm")
        );
        assert!(problems[0].is_error());
    }

    /// #64: thickness drives depth, tab Z and, with the origin at the stock
    /// bottom, safe Z itself, so an unset one is an error like a missing
    /// feed is at export, never a default. The message says export refuses.
    #[test]
    fn missing_thickness_is_an_error() {
        let mut p = sample_project();
        p.stock.thickness_mm = None;
        let problems = validate(&p);
        assert_eq!(problems, vec![Problem::StockThicknessUnset]);
        assert!(problems[0].is_error());
        assert_eq!(
            problems[0].to_string(),
            "stock.thickness_mm is not set (mm)"
        );
    }

    /// #64: an unset Profile depth is an error naming the operation.
    #[test]
    fn missing_depth_is_an_error() {
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.depth_mm = None;
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::OperationValueUnset {
                operation: OperationId::new("o1"),
                name: "Cut out".into(),
                field: "depth_mm",
            }]
        );
        assert!(problems[0].is_error());
        assert_eq!(
            problems[0].to_string(),
            "operations[o1].depth_mm is not set (mm) for operation `Cut out`"
        );
    }

    /// #64: one run reports the unset thickness, depth and tool rates
    /// together, so the user fixes them in one edit.
    #[test]
    fn missing_thickness_depth_and_feeds_are_reported_together() {
        let mut p = sample_project();
        p.stock.thickness_mm = None;
        p.tools[0].feed_mm_min = None;
        let op = profile_mut(&mut p);
        op.depth_mm = None;
        let problems = validate(&p);
        assert_eq!(problems.len(), 3, "{problems:?}");
        assert!(problems.contains(&Problem::StockThicknessUnset));
        assert!(problems.iter().any(
            |p| matches!(p, Problem::OperationValueUnset { field, .. } if *field == "depth_mm")
        ));
        assert!(problems.iter().any(
            |p| matches!(p, Problem::ToolValueUnset { field, .. } if *field == "feed_mm_min")
        ));
    }

    /// #76: every unset value is listed, in the order the file holds them,
    /// each with the field that locates it.
    #[test]
    fn unset_values_are_listed_in_file_order_with_fields() {
        let mut p = sample_project();
        p.stock.thickness_mm = None;
        p.tools[0].feed_mm_min = None;
        p.tools[0].plunge_mm_min = None;
        p.tools[0].rpm = None;
        let op = profile_mut(&mut p);
        op.depth_mm = None;
        let problems = validate(&p);
        assert!(problems.iter().all(Problem::is_unset_value), "{problems:?}");
        assert!(problems.iter().all(Problem::is_error), "{problems:?}");
        let fields: Vec<_> = problems.iter().filter_map(Problem::field).collect();
        assert_eq!(
            fields,
            [
                "stock.thickness_mm",
                "tools[t1].feed_mm_min",
                "tools[t1].plunge_mm_min",
                "tools[t1].rpm",
                "operations[o1].depth_mm",
            ]
        );
    }

    /// #76: a problem's field is where it sits in the saved file.
    #[test]
    fn problem_fields_locate_in_the_saved_file() {
        let mut p = sample_project();
        p.tools[0].diameter_mm = 0.0;
        p.tools[0].cutter = ToolKind::VBit {
            included_angle_deg: 200.0,
        };
        p.instances[0].definition = DefinitionId::new("missing");
        p.post_processor.file_extension = "dcv".into();
        let op = profile_mut(&mut p);
        op.safe_z_mm = 0.0;
        op.enabled = false;
        let problems = validate(&p);
        assert_eq!(problems.len(), 7, "{problems:?}");
        let text = crate::save(&p).unwrap();
        let map = crate::SourceMap::parse(&text).unwrap();
        for problem in &problems {
            let field = problem.field().unwrap();
            assert!(map.locate(&field).is_some(), "{field} in {text}");
        }
        assert!(
            Problem::DuplicateToolId {
                id: ToolId::new("t1")
            }
            .field()
            .is_none()
        );
        assert!(!problems.iter().any(Problem::is_unset_value));
    }

    /// A second tool with no rates set.
    fn unset_tool(id: &str) -> Tool {
        Tool {
            id: ToolId::new(id),
            name: "3 mm downcut".into(),
            feed_mm_min: None,
            plunge_mm_min: None,
            rpm: None,
            ..crate::test_support::tool_6_35_upcut()
        }
    }

    /// PR #84: a tool nothing cuts with is one warning, located at the
    /// tool; its unset rates are not errors, and nothing defaults them.
    #[test]
    fn unused_tool_with_unset_values_is_a_warning_not_an_error() {
        let mut p = sample_project();
        p.tools.push(unset_tool("t2"));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::UnusedTool {
                tool: ToolId::new("t2"),
                name: "3 mm downcut".into(),
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
        assert_eq!(
            problems[0].to_string(),
            "tool `t2` (`3 mm downcut`) is defined but not used by any enabled operation"
        );
        let text = crate::save(&p).unwrap();
        let map = crate::SourceMap::parse(&text).unwrap();
        let at = map.locate(&problems[0].field().unwrap()).unwrap();
        let line = text.lines().nth(at.line - 1).unwrap();
        assert_eq!(line.trim(), "{", "points at the tool object: {line}");
        let next = text.lines().nth(at.line).unwrap();
        assert_eq!(next.trim(), "\"id\": \"t2\",");
    }

    /// PR #84: a tool only disabled operations use is unused, like the
    /// operations themselves; enabling one brings its errors back.
    #[test]
    fn tool_used_only_by_disabled_operation_is_unused() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = None;
        p.tools[0].rpm = None;
        let op = profile_mut(&mut p);
        op.enabled = false;
        let problems = validate(&p);
        assert!(errors(&problems).is_empty(), "{problems:?}");
        assert!(problems.contains(&Problem::UnusedTool {
            tool: ToolId::new("t1"),
            name: "6.35 mm upcut".into(),
        }));
        let op = profile_mut(&mut p);
        op.enabled = true;
        let problems = validate(&p);
        assert_eq!(errors(&problems).len(), 2, "{problems:?}");
        assert!(
            !problems
                .iter()
                .any(|p| matches!(p, Problem::UnusedTool { .. }))
        );
    }

    /// PR #84: a tool an enabled operation uses still reports every unset
    /// rate as an error, beside an unused tool's warning.
    #[test]
    fn used_tool_with_unset_values_is_still_an_error() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = None;
        p.tools[0].plunge_mm_min = None;
        p.tools[0].rpm = None;
        p.tools.push(unset_tool("t2"));
        let problems = validate(&p);
        let fields: Vec<_> = errors(&problems)
            .into_iter()
            .filter_map(Problem::field)
            .collect();
        assert_eq!(
            fields,
            [
                "tools[t1].feed_mm_min",
                "tools[t1].plunge_mm_min",
                "tools[t1].rpm"
            ]
        );
        assert_eq!(problems.len(), 4, "one warning for t2: {problems:?}");
    }

    /// A zero feed is wrong even on an unused tool.
    #[test]
    fn unused_tool_still_rejects_bad_values() {
        let mut p = sample_project();
        let mut t2 = unset_tool("t2");
        t2.feed_mm_min = Some(0.0);
        p.tools.push(t2);
        let problems = validate(&p);
        assert!(
            problems.iter().any(|p| matches!(p, Problem::ToolOutOfRange { tool, field, .. } if tool.as_str() == "t2" && *field == "feed_mm_min")),
            "{problems:?}"
        );
    }

    #[test]
    fn non_positive_tool_diameter_is_error() {
        let mut p = sample_project();
        p.tools[0].diameter_mm = -1.0;
        let problems = validate(&p);
        assert!(
            matches!(&problems[..], [Problem::ToolOutOfRange { tool, field, .. }] if tool.as_str() == "t1" && *field == "diameter_mm"),
            "{problems:?}"
        );
    }

    #[test]
    fn non_positive_depth_is_error() {
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.depth_mm = Some(0.0);
        let problems = validate(&p);
        assert!(
            matches!(&problems[..], [Problem::OutOfRange { field, .. }] if field == "operations[o1].depth_mm"),
            "{problems:?}"
        );
    }

    #[test]
    fn disabled_operation_is_warning() {
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.enabled = false;
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![
                Problem::UnusedTool {
                    tool: ToolId::new("t1"),
                    name: "6.35 mm upcut".into(),
                },
                Problem::OperationDisabled {
                    operation: OperationId::new("o1"),
                },
            ]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
    }

    #[test]
    fn non_finite_value_is_error() {
        let mut p = sample_project();
        p.stock.width_mm = f64::NAN;
        p.tools[0].cutter = ToolKind::VBit {
            included_angle_deg: f64::INFINITY,
        };
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![
                Problem::NonFinite {
                    field: "stock.width_mm".into()
                },
                Problem::NonFinite {
                    field: "tools[t1].cutter.v_bit.included_angle_deg".into()
                },
                // #67: a V-bit of any angle cannot cut a Profile.
                Problem::CutterNotFlat {
                    operation: OperationId::new("o1"),
                    name: "Cut out".into(),
                    tool: ToolId::new("t1"),
                    tool_name: "6.35 mm upcut".into(),
                    cutter: "a V-bit",
                    field: "tool",
                },
            ]
        );
    }

    #[test]
    fn duplicate_tool_id_is_error() {
        let mut p = sample_project();
        p.tools.push(p.tools[0].clone());
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::DuplicateToolId {
                id: ToolId::new("t1")
            }]
        );
    }

    #[test]
    fn definition_key_mismatch_is_error() {
        let mut p = sample_project();
        if let Some(def) = p.definitions.values_mut().next() {
            def.id = DefinitionId::new("other");
        }
        let problems = validate(&p);
        assert_eq!(problems.len(), 1);
        assert!(matches!(problems[0], Problem::DefinitionKeyMismatch { .. }));
    }

    #[test]
    fn problem_messages_name_the_subject() {
        let problem = Problem::ToolValueUnset {
            tool: ToolId::new("t1"),
            name: "Set me".into(),
            field: "feed_mm_min",
        };
        assert_eq!(
            problem.to_string(),
            "tools[t1].feed_mm_min is not set (mm/min) for tool `Set me`"
        );
    }

    const DWELL: &str = "post_processor.spindle_on_dwell_s";

    #[test]
    fn negative_spindle_dwell_is_error() {
        let mut p = sample_project();
        p.post_processor.spindle_on_dwell_s = -1.0;
        let problems = validate(&p);
        let field = DWELL.to_owned();
        assert!(
            matches!(&problems[..], [Problem::OutOfRange { field: f, .. }] if *f == field),
            "{problems:?}"
        );
    }

    #[test]
    fn non_finite_spindle_dwell_is_error() {
        let mut p = sample_project();
        p.post_processor.spindle_on_dwell_s = f64::NAN;
        let field = DWELL.to_owned();
        assert_eq!(validate(&p), vec![Problem::NonFinite { field }]);
    }

    #[test]
    fn zero_spindle_dwell_is_allowed() {
        let mut p = sample_project();
        p.post_processor.spindle_on_dwell_s = 0.0;
        assert!(validate(&p).is_empty());
    }

    /// Functional spec acceptance test 2 (#40): a tool problem names the
    /// tool as the user sees it, not only by id.
    #[test]
    fn tool_problems_name_the_tool() {
        let mut p = sample_project();
        p.tools[0].name = "Set me".into();
        p.tools[0].feed_mm_min = Some(0.0);
        p.tools[0].plunge_mm_min = None;
        let messages: Vec<String> = validate(&p).iter().map(ToString::to_string).collect();
        assert_eq!(messages.len(), 2, "{messages:?}");
        for (message, field) in messages.iter().zip(["feed_mm_min", "plunge_mm_min"]) {
            for part in ["Set me", "t1", field] {
                assert!(message.contains(part), "`{message}` names {part}");
            }
        }
    }

    /// Functional spec capability 8 (#40): open paths are imported but
    /// Profile never cuts them, and `check` reports each one.
    #[test]
    fn open_profile_is_warning_naming_the_definition() {
        let mut p = sample_project();
        assert!(
            !validate(&p)
                .iter()
                .any(|q| matches!(q, Problem::OpenProfile { .. })),
            "the sample's closed rectangle is not reported"
        );
        let def = p
            .definitions
            .get_mut(&DefinitionId::new("d1"))
            .expect("the sample has d1");
        def.profiles.push(Polyline::new(
            vec![Vertex::new(0.0, 40.0, 0.0), Vertex::new(50.0, 40.0, 0.0)],
            false,
        ));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::OpenProfile {
                definition: DefinitionId::new("d1"),
                name: "Rectangle".into(),
                profile: 1,
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
        let message = problems[0].to_string();
        assert!(
            message.contains("Rectangle") && message.contains("open"),
            "{message}"
        );
    }

    /// Capability 15 (#135): Engrave cuts open paths, so an open profile of
    /// a definition an enabled Engrave targets is not reported as never
    /// cut. A disabled Engrave cuts nothing, so the warning stays.
    #[test]
    fn open_profile_engraved_is_not_reported() {
        let mut p = sample_project();
        p.definitions
            .get_mut(&DefinitionId::new("d1"))
            .expect("the sample has d1")
            .profiles
            .push(Polyline::new(
                vec![Vertex::new(0.0, 40.0, 0.0), Vertex::new(50.0, 40.0, 0.0)],
                false,
            ));
        p.operations.push(Operation::Engrave(sample_engrave()));
        let open = |p: &Project| {
            validate(p)
                .iter()
                .any(|q| matches!(q, Problem::OpenProfile { .. }))
        };
        assert!(!open(&p), "{:?}", validate(&p));
        if let Some(Operation::Engrave(op)) = p.operations.last_mut() {
            op.enabled = false;
        }
        assert!(open(&p), "a disabled Engrave cuts nothing");
    }

    /// #175: an On line Profile cuts open paths too, so an open profile of
    /// a definition an enabled On line Profile targets is not reported.
    /// Inside and Outside skip open paths, so the warning stays for them,
    /// and its message names what does cut an open path.
    #[test]
    fn open_profile_cut_on_line_is_not_reported() {
        let mut p = sample_project();
        p.definitions
            .get_mut(&DefinitionId::new("d1"))
            .expect("the sample has d1")
            .profiles
            .push(Polyline::new(
                vec![Vertex::new(0.0, 40.0, 0.0), Vertex::new(50.0, 40.0, 0.0)],
                false,
            ));
        let open = |p: &Project| {
            validate(p)
                .into_iter()
                .find(|q| matches!(q, Problem::OpenProfile { .. }))
        };
        let message = open(&p).expect("Outside skips it").to_string();
        assert!(
            message.contains("On line") && message.contains("Engrave"),
            "{message}"
        );
        profile_mut(&mut p).side = Side::OnLine;
        assert!(open(&p).is_none(), "{:?}", validate(&p));
        // #207: Left and Right cut open paths too.
        for side in [Side::Left, Side::Right] {
            profile_mut(&mut p).side = side;
            assert!(open(&p).is_none(), "{side:?}: {:?}", validate(&p));
        }
        profile_mut(&mut p).enabled = false;
        assert!(open(&p).is_some(), "a disabled Profile cuts nothing");
    }

    /// The fields of every error-class problem, in order.
    fn error_fields(problems: &[Problem]) -> Vec<String> {
        problems
            .iter()
            .filter(|p| p.is_error())
            .map(|p| match p {
                Problem::OutOfRange { field, .. } | Problem::NonFinite { field } => field.clone(),
                other => panic!("unexpected problem {other:?}"),
            })
            .collect()
    }

    /// #59: a finite but huge transform passed validation and then
    /// panicked `cavalier_contours`. Scale and translation are each
    /// rejected, naming the instance's transform.
    #[test]
    fn huge_transform_is_rejected() {
        for transform in [
            Transform::scale(1e15, 1e15),
            Transform::translate(1e12, 0.0),
            Transform::from([1.0, 0.0, 0.0, 1.0, 0.0, -1e12]),
        ] {
            let mut p = sample_project();
            p.instances[0].transform = transform;
            let fields = error_fields(&validate(&p));
            assert!(!fields.is_empty(), "{transform:?} is rejected");
            assert!(
                fields
                    .iter()
                    .all(|f| f.starts_with("instances[i1].transform")),
                "{transform:?}: {fields:?}"
            );
        }
    }

    /// #59: an instance whose placed shape reaches just past the limit is
    /// rejected even though each coefficient is in range; one that reaches
    /// exactly the limit is accepted. The sample rectangle is 50 × 30 mm.
    #[test]
    fn placement_past_coordinate_limit_is_rejected() {
        let mut p = sample_project();
        p.stock.width_mm = MAX_COORDINATE_MM;
        p.instances[0].transform = Transform::translate(MAX_COORDINATE_MM - 50.0, 10.0);
        assert_eq!(validate(&p), vec![], "reaching the limit is accepted");
        p.instances[0].transform = Transform::translate(MAX_COORDINATE_MM - 49.5, 10.0);
        let problems = validate(&p);
        assert!(
            matches!(&problems[..], [Problem::OutOfRange { field, value, .. }]
                if field == "instances[i1].transform"
                    && approx_eq(*value, MAX_COORDINATE_MM + 0.5, 1e-9)),
            "{problems:?}"
        );
    }

    /// #99: phase 1 does not use the instance depths, so leaving them unset
    /// is neither an error nor a warning.
    #[test]
    fn unset_instance_depths_are_valid() {
        let mut p = sample_project();
        p.instances[0].z_top_mm = None;
        p.instances[0].z_bottom_mm = None;
        assert_eq!(validate(&p), vec![]);
    }

    /// #71: an instance's bottom may not lie above its top; equal is a
    /// flat feature and allowed. Nothing reads them yet, so the order is
    /// all there is to check.
    #[test]
    fn instance_z_bottom_above_z_top_is_error() {
        let mut p = sample_project();
        p.instances[0].z_top_mm = Some(2.0);
        p.instances[0].z_bottom_mm = Some(3.0);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::OutOfRange {
                field: "instances[i1].z_bottom_mm".into(),
                value: 3.0,
                expected: BELOW_Z_TOP,
            }]
        );
        assert!(problems[0].is_error());
        p.instances[0].z_bottom_mm = Some(2.0);
        assert_eq!(validate(&p), vec![], "equal is allowed");
        p.instances[0].z_top_mm = None;
        p.instances[0].z_bottom_mm = Some(3.0);
        assert_eq!(validate(&p), vec![], "one unset is not compared");
    }

    /// #99: a set instance depth is still checked against the coordinate
    /// limit.
    #[test]
    fn instance_z_top_out_of_range_is_error() {
        let mut p = sample_project();
        p.instances[0].z_top_mm = Some(7000.0);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::OutOfRange {
                field: "instances[i1].z_top_mm".into(),
                value: 7000.0,
                expected: COORDINATE,
            }]
        );
        assert!(problems[0].is_error());
        p.instances[0].z_top_mm = Some(MAX_COORDINATE_MM);
        p.instances[0].z_bottom_mm = Some(-MAX_COORDINATE_MM);
        assert_eq!(validate(&p), vec![], "the limit is inclusive");
    }

    /// #59: vertex coordinates are bounded by the limit, inclusive.
    #[test]
    fn huge_vertex_is_rejected() {
        let mut p = sample_project();
        p.instances[0].transform = Transform::IDENTITY;
        let def = p
            .definitions
            .get_mut(&DefinitionId::new("d1"))
            .expect("the sample has d1");
        def.profiles = vec![Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(MAX_COORDINATE_MM, 0.0),
                Vertex::line(MAX_COORDINATE_MM, 10.0),
            ],
            true,
        )];
        assert!(errors(&validate(&p)).is_empty(), "the limit is accepted");
        let def = p
            .definitions
            .get_mut(&DefinitionId::new("d1"))
            .expect("the sample has d1");
        def.profiles = vec![Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(1e12, 0.0),
                Vertex::line(1e12, -10.0),
            ],
            true,
        )];
        assert_eq!(
            error_fields(&validate(&p)),
            vec![
                "definitions[d1].profiles[0].vertices[1].x",
                "definitions[d1].profiles[0].vertices[2].x",
            ]
        );
    }

    /// #59: an arc whose vertices are in range can still bow past the
    /// limit; the profile is reported.
    #[test]
    fn arc_bulging_past_coordinate_limit_is_rejected() {
        let mut p = sample_project();
        p.instances[0].transform = Transform::IDENTITY;
        let def = p
            .definitions
            .get_mut(&DefinitionId::new("d1"))
            .expect("the sample has d1");
        def.profiles = vec![Polyline::circle(
            dc_geometry::Point::new(0.0, MAX_COORDINATE_MM - 0.5),
            1.0,
        )];
        let fields = error_fields(&validate(&p));
        assert!(
            fields.contains(&"definitions[d1].profiles[0]".to_owned()),
            "{fields:?}"
        );
    }

    /// #59: stock sides are bounded by the same limit, inclusive.
    #[test]
    fn huge_stock_is_rejected() {
        let mut p = sample_project();
        p.stock.width_mm = MAX_COORDINATE_MM;
        p.stock.height_mm = MAX_COORDINATE_MM;
        p.stock.thickness_mm = Some(MAX_COORDINATE_MM);
        assert_eq!(validate(&p), vec![], "the limit is accepted");
        p.stock.width_mm = MAX_COORDINATE_MM + 0.5;
        p.stock.height_mm = 1e15;
        p.stock.thickness_mm = Some(1e300);
        assert_eq!(
            error_fields(&validate(&p)),
            vec!["stock.width_mm", "stock.height_mm", "stock.thickness_mm"]
        );
    }

    /// The limit phrases spell out `MAX_COORDINATE_MM` by hand.
    #[test]
    fn limit_phrases_match_the_constant() {
        let limit = format!("{MAX_COORDINATE_MM}");
        for phrase in [STOCK_SIDE, COORDINATE, COEFFICIENT, REACH] {
            assert!(phrase.contains(&limit), "`{phrase}` states {limit}");
        }
    }

    /// #59: the message states the limit, so the user knows what to aim for.
    #[test]
    fn limit_messages_state_the_limit() {
        let mut p = sample_project();
        p.stock.width_mm = 1e15;
        p.instances[0].transform = Transform::translate(1e12, 0.0);
        let limit = MAX_COORDINATE_MM.to_string();
        let problems = validate(&p);
        assert_eq!(problems.len(), 2, "{problems:?}");
        for problem in problems {
            let message = problem.to_string();
            assert!(message.contains(&limit), "`{message}` states {limit}");
        }
    }

    /// #36 security review (#40): a shared `.dcv` chooses its output
    /// extension, so it must not name the project file or a path.
    #[test]
    fn unsafe_file_extension_is_error() {
        for extension in ["dcv", "DCV", "", "/../x", "n c", "toolong12"] {
            let mut p = sample_project();
            p.post_processor.file_extension = extension.into();
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::UnsafeFileExtension {
                    extension: extension.into()
                }],
                "`{extension}` is refused"
            );
            assert!(problems[0].is_error(), "`{extension}` is an error");
        }
        for extension in ["nc", "gcode", "tap", "NGC"] {
            let mut p = sample_project();
            p.post_processor.file_extension = extension.into();
            assert_eq!(validate(&p), vec![], "`{extension}` is allowed");
        }
    }

    /// #95: `"m6"` needs a definition whose tool change has `M6`. GRBL 1.1
    /// has none (`definitions/README.md`, "Tool changes"), so the mode is an
    /// error located at the field, whatever the operations. With an `M6`
    /// line, as grblHAL has, it is fine.
    #[test]
    fn m6_mode_without_m6_command_is_an_error() {
        let mut p = sample_project();
        p.post_processor.tool_change_mode = crate::ToolChangeMode::M6;
        let problems = validate(&p);
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert!(problems[0].is_error(), "{problems:?}");
        let field = problems[0].field().unwrap();
        assert_eq!(field, "post_processor.tool_change_mode");
        let message = problems[0].to_string();
        assert!(
            message.contains("M6") && message.contains("separate_programs"),
            "{message}"
        );
        let text = crate::save(&p).unwrap();
        let map = crate::SourceMap::parse(&text).unwrap();
        assert!(map.locate(&field).is_some(), "{field} in {text}");

        p.post_processor = crate::PostProcessorRef {
            tool_change_mode: crate::ToolChangeMode::M6,
            ..crate::bundled_definition("grblhal", 1)
                .unwrap()
                .default_ref()
        };
        assert_eq!(validate(&p), vec![]);
    }

    // #67: sanity checks on dangerous but valid values.

    fn op_mut(p: &mut Project) -> &mut crate::model::ProfileOp {
        profile_mut(p)
    }

    /// Asserts `problems` is exactly one problem with `severity`, whose
    /// field locates in the saved `p`; returns its message.
    fn only_located(p: &Project, problems: &[Problem], severity: Severity) -> String {
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].severity(), severity, "{problems:?}");
        let text = crate::save(p).unwrap();
        let map = crate::SourceMap::parse(&text).unwrap();
        let field = problems[0].field().unwrap();
        assert!(map.locate(&field).is_some(), "{field} in {text}");
        problems[0].to_string()
    }

    /// #67: a 50 mm step-down with a 6.35 mm end mill is one full-depth
    /// pass; it is a warning naming the operation and the tool, located at
    /// the step-down in effect.
    #[test]
    fn step_down_above_diameter_warns() {
        let mut p = sample_project();
        op_mut(&mut p).step_down_mm = Some(50.0);
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Warning);
        assert_eq!(problems[0].field().unwrap(), "operations[o1].step_down_mm");
        assert_eq!(
            message,
            "operation `Cut out` (`o1`) cuts 50 mm per pass, deeper than the 6.35 mm diameter of tool `6.35 mm upcut`; passes that deep can break the bit, so use a step-down of at most 6.35 mm"
        );

        // The tool's step-down, when the operation sets none.
        let mut p = sample_project();
        p.tools[0].step_down_mm = Some(7.0);
        let problems = validate(&p);
        only_located(&p, &problems, Severity::Warning);
        assert_eq!(problems[0].field().unwrap(), "tools[t1].step_down_mm");

        // Equal to the diameter is fine.
        let mut p = sample_project();
        op_mut(&mut p).step_down_mm = Some(6.35);
        assert_eq!(validate(&p), vec![]);
    }

    /// #67: Profile offsets by half the diameter, which is right only for
    /// a flat end mill; any other cutter makes the part the wrong size.
    #[test]
    fn v_bit_in_profile_is_an_error() {
        let mut p = sample_project();
        p.tools[0].cutter = ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Error);
        assert_eq!(problems[0].field().unwrap(), "operations[o1].tool");
        assert_eq!(
            message,
            "operation `Cut out` (`o1`) cuts with tool `6.35 mm upcut` (`t1`), a V-bit; an Inside, Outside, Left or Right Profile and a Pocket offset the path by half the diameter, which only a flat end mill cuts at every depth, so the part would come out the wrong size. Use an end mill"
        );
    }

    #[test]
    fn ball_nose_and_engraver_in_profile_are_errors() {
        for (cutter, words) in [
            (ToolKind::BallNose, "a ball-nose bit"),
            (ToolKind::Engraver, "an engraving bit"),
        ] {
            for side in [crate::model::Side::Outside, crate::model::Side::Inside] {
                let mut p = sample_project();
                p.tools[0].cutter = cutter.clone();
                op_mut(&mut p).side = side;
                let problems = validate(&p);
                let message = only_located(&p, &problems, Severity::Error);
                assert!(message.contains(words), "{message}");
            }
        }
    }

    /// #207: Left and Right offset the path by half the diameter, as
    /// Inside and Outside do, so they need a flat end mill too.
    #[test]
    fn v_bit_left_or_right_is_an_error() {
        for side in [crate::model::Side::Left, crate::model::Side::Right] {
            let mut p = sample_project();
            p.tools[0].cutter = ToolKind::VBit {
                included_angle_deg: 60.0,
            };
            op_mut(&mut p).side = side;
            let problems = validate(&p);
            assert!(
                problems
                    .iter()
                    .any(|q| matches!(q, Problem::CutterNotFlat { .. })),
                "{side:?}: {problems:?}"
            );
        }
    }

    /// #67 (maintainer, PR #96): on the line the tool centre follows the
    /// line itself, so the cut does not depend on the cutter's shape.
    #[test]
    fn v_bit_on_line_is_allowed() {
        let mut p = sample_project();
        p.tools[0].cutter = ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        op_mut(&mut p).side = crate::model::Side::OnLine;
        assert_eq!(validate(&p), vec![]);
    }

    #[test]
    fn ball_nose_and_engraver_on_line_are_allowed() {
        for cutter in [ToolKind::BallNose, ToolKind::Engraver] {
            let mut p = sample_project();
            p.tools[0].cutter = cutter;
            op_mut(&mut p).side = crate::model::Side::OnLine;
            assert_eq!(validate(&p), vec![]);
        }
    }

    /// #67 (maintainer comment): a feed over the limit fails export. The
    /// error names the tool, the field and the limit.
    #[test]
    fn feed_above_limit_is_an_error() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = Some(1e9);
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Error);
        assert_eq!(problems[0].field().unwrap(), "tools[t1].feed_mm_min");
        assert_eq!(
            message,
            "tool `6.35 mm upcut` (`t1`): feed_mm_min is 1000000000, expected a value greater than 0 and at most 20000 mm/min"
        );
        p.tools[0].feed_mm_min = Some(MAX_RATE_MM_MIN);
        assert_eq!(validate(&p), vec![]);
    }

    #[test]
    fn plunge_above_limit_is_an_error() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = Some(MAX_RATE_MM_MIN);
        p.tools[0].plunge_mm_min = Some(MAX_RATE_MM_MIN + 1.0);
        let problems = validate(&p);
        // Also faster than the feed, which is a warning of its own.
        let errors = errors(&problems);
        assert_eq!(errors.len(), 1, "{problems:?}");
        assert_eq!(errors[0].field().unwrap(), "tools[t1].plunge_mm_min");
    }

    /// The rate limit phrase spells out `MAX_RATE_MM_MIN` by hand.
    #[test]
    fn rate_limit_phrase_matches_the_constant() {
        assert!(
            RATE.contains(&format!("{MAX_RATE_MM_MIN} mm/min")),
            "{RATE}"
        );
    }

    /// #67: GRBL and grblHAL clamp `S` to `$30`, so a huge speed is a
    /// warning, not an error.
    #[test]
    fn rpm_above_limit_warns() {
        let mut p = sample_project();
        p.tools[0].rpm = Some(1_000_000);
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Warning);
        assert_eq!(problems[0].field().unwrap(), "tools[t1].rpm");
        assert_eq!(
            message,
            "tool `6.35 mm upcut` (`t1`) is set to 1000000 RPM, faster than the 60000 RPM a hobby spindle or router turns; check it for a typo"
        );
        p.tools[0].rpm = Some(MAX_RPM);
        assert_eq!(validate(&p), vec![]);
    }

    #[test]
    fn plunge_above_feed_warns() {
        let mut p = sample_project();
        p.tools[0].plunge_mm_min = Some(1200.0);
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Warning);
        assert_eq!(problems[0].field().unwrap(), "tools[t1].plunge_mm_min");
        assert_eq!(
            message,
            "tool `6.35 mm upcut` (`t1`) plunges at 1200 mm/min, faster than it cuts (1000 mm/min); a plunge rate is usually well below the feed rate, so check the two are not swapped"
        );
        p.tools[0].plunge_mm_min = Some(1000.0);
        assert_eq!(validate(&p), vec![]);
    }

    #[test]
    fn diameter_above_limit_warns() {
        let mut p = sample_project();
        p.tools[0].diameter_mm = 200.0;
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Warning);
        assert_eq!(problems[0].field().unwrap(), "tools[t1].diameter_mm");
        assert_eq!(
            message,
            "tool `6.35 mm upcut` (`t1`) is 200 mm in diameter, larger than the 50 mm of a hobby router bit; check it for a typo"
        );
        p.tools[0].diameter_mm = MAX_TOOL_DIAMETER_MM;
        assert_eq!(validate(&p), vec![]);
    }

    /// #67: an enabled operation with no targets would write a program
    /// that starts the spindle and stops it without moving.
    #[test]
    fn empty_operation_is_an_error() {
        let mut p = sample_project();
        op_mut(&mut p).targets.clear();
        let problems = validate(&p);
        let message = only_located(&p, &problems, Severity::Error);
        assert_eq!(problems[0].field().unwrap(), "operations[o1].targets");
        assert_eq!(
            message,
            "operation `Cut out` (`o1`) is enabled but has no targets, so it would cut nothing; add targets or disable it"
        );
        // Disabled, it is only the disabled warning (and its unused tool).
        op_mut(&mut p).enabled = false;
        assert!(errors(&validate(&p)).is_empty());
    }

    /// #67: only tools an enabled operation uses are checked; an unused
    /// tool is already a warning of its own.
    #[test]
    fn unused_tool_gets_no_sanity_findings() {
        let mut p = sample_project();
        p.tools.push(Tool {
            id: ToolId::new("t2"),
            name: "Odd".into(),
            cutter: ToolKind::VBit {
                included_angle_deg: 60.0,
            },
            diameter_mm: 200.0,
            feed_mm_min: Some(1e9),
            plunge_mm_min: Some(2e9),
            rpm: Some(1_000_000),
            step_down_mm: Some(500.0),
            ..crate::test_support::tool_6_35_upcut()
        });
        assert_eq!(
            validate(&p),
            vec![Problem::UnusedTool {
                tool: ToolId::new("t2"),
                name: "Odd".into(),
            }]
        );
    }

    /// #67: every sanity problem is reported in one run, in file order.
    #[test]
    fn sanity_problems_are_reported_together() {
        let mut p = sample_project();
        p.tools[0].feed_mm_min = Some(1e9);
        p.tools[0].plunge_mm_min = Some(2e9);
        p.tools[0].rpm = Some(1_000_000);
        op_mut(&mut p).targets.clear();
        let problems = validate(&p);
        let fields: Vec<_> = problems.iter().filter_map(Problem::field).collect();
        assert_eq!(
            fields,
            [
                "tools[t1].feed_mm_min",
                "tools[t1].plunge_mm_min",
                "tools[t1].rpm",
                "tools[t1].plunge_mm_min",
                "operations[o1].targets",
            ],
            "{problems:?}"
        );
        assert_eq!(errors(&problems).len(), 3, "{problems:?}");
    }

    /// #98: with neither the operation nor its tool giving a step-down,
    /// the passes are unknown, so it is an error located at the tool, the
    /// one value every operation using it falls back on.
    #[test]
    fn step_down_unset_on_operation_and_tool_is_error() {
        let mut p = sample_project();
        p.tools[0].step_down_mm = None;
        op_mut(&mut p).step_down_mm = None;
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ToolValueUnset {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
                field: "step_down_mm",
            }]
        );
        assert!(problems[0].is_error());
        assert!(problems[0].is_unset_value());
        assert_eq!(problems[0].field().unwrap(), "tools[t1].step_down_mm");
        assert_eq!(
            problems[0].to_string(),
            "tools[t1].step_down_mm is not set (mm) for tool `6.35 mm upcut`"
        );
    }

    /// #98: the operation's own step-down is enough.
    #[test]
    fn step_down_on_operation_only_is_valid() {
        let mut p = sample_project();
        p.tools[0].step_down_mm = None;
        op_mut(&mut p).step_down_mm = Some(2.0);
        assert_eq!(validate(&p), vec![]);
    }

    /// #98: the tool's step-down applies when the operation sets none.
    #[test]
    fn step_down_on_tool_only_is_valid() {
        let mut p = sample_project();
        p.tools[0].step_down_mm = Some(2.0);
        op_mut(&mut p).step_down_mm = None;
        assert_eq!(validate(&p), vec![]);
    }

    /// #98: two operations falling back on the same unset tool value give
    /// one error, not one per operation.
    #[test]
    fn step_down_unset_reported_once_per_tool() {
        let mut p = sample_project();
        p.tools[0].step_down_mm = None;
        op_mut(&mut p).step_down_mm = None;
        let mut second = op_mut(&mut p).clone();
        second.id = OperationId::new("o2");
        p.operations.push(Operation::Profile(second));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ToolValueUnset {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
                field: "step_down_mm",
            }]
        );
    }

    /// #98: a disabled operation cuts nothing, so it needs no step-down.
    #[test]
    fn disabled_operation_needs_no_step_down() {
        let mut p = sample_project();
        p.tools[0].step_down_mm = None;
        let op = op_mut(&mut p);
        op.step_down_mm = None;
        op.enabled = false;
        let problems = validate(&p);
        assert!(errors(&problems).is_empty(), "{problems:?}");
        assert!(
            !problems
                .iter()
                .any(|p| matches!(p, Problem::ToolValueUnset { .. })),
            "{problems:?}"
        );
    }

    /// #67: the quick start's coaster, as the chapter fills it in (feed
    /// 1500, plunge 500, 18000 RPM, 4 mm step-down, 6.35 mm end mill),
    /// draws no sanity finding. `dreamcarve`'s CLI test
    /// `quick_start_project_has_no_sanity_warnings` checks the real file.
    #[test]
    fn quick_start_values_have_no_sanity_findings() {
        let mut p = sample_project();
        let tool = &mut p.tools[0];
        tool.cutter = ToolKind::EndMill;
        tool.diameter_mm = 6.35;
        tool.feed_mm_min = Some(1500.0);
        tool.plunge_mm_min = Some(500.0);
        tool.rpm = Some(18000);
        // On the tool since #98; the operation's is left unset.
        tool.step_down_mm = Some(4.0);
        p.stock.thickness_mm = Some(11.8);
        let op = op_mut(&mut p);
        op.step_down_mm = None;
        op.depth_mm = Some(12.0);
        op.through_cut_allowance_mm = 0.2;
        assert_eq!(validate(&p), vec![]);
    }

    // Phase 2, #117: tool numbers and the new operations' rules
    // (`docs/specs/phase-2/design.md`, "Tool numbers", "Validation").

    /// The sample project with `ops` after its Profile.
    fn with_ops(ops: Vec<Operation>) -> Project {
        let mut p = sample_project();
        p.operations.extend(ops);
        p
    }

    /// The sample tool copied as `id`, every value set.
    fn tool(id: &str) -> Tool {
        Tool {
            id: ToolId::new(id),
            name: format!("tool {id}"),
            ..crate::test_support::tool_6_35_upcut()
        }
    }

    /// Design spec, "Tool numbers": two tools of enabled operations that
    /// resolve to the same `T` number would make the program change to
    /// the wrong bit. An error at the second tool's `number`.
    #[test]
    fn duplicate_tool_number_is_error() {
        let mut p = sample_project();
        p.tools[0].number = Some(2);
        let mut t2 = tool("t2");
        t2.number = Some(2);
        p.tools.push(t2);
        let mut second = sample_pocket();
        second.tool = ToolId::new("t2");
        p.operations.push(Operation::Pocket(second));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::DuplicateToolNumber {
                tool: ToolId::new("t2"),
                name: "tool t2".into(),
                number: 2,
                other: ToolId::new("t1"),
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].field().as_deref(), Some("tools[t2].number"));
        assert!(problems[0].to_string().contains("T2"), "{}", problems[0]);
    }

    /// An unset number is the tool's 1-based position, which a set number
    /// on another tool may clash with.
    #[test]
    fn set_number_clashing_with_positional_is_error() {
        let mut p = sample_project();
        let mut t2 = tool("t2");
        t2.number = Some(1);
        p.tools.push(t2);
        let mut second = sample_pocket();
        second.tool = ToolId::new("t2");
        p.operations.push(Operation::Pocket(second));
        assert_eq!(
            validate(&p),
            vec![Problem::DuplicateToolNumber {
                tool: ToolId::new("t2"),
                name: "tool t2".into(),
                number: 1,
                other: ToolId::new("t1"),
            }]
        );
        // The other way round: the positional tool is the second one.
        let mut p = sample_project();
        p.tools[0].number = Some(2);
        p.tools.push(tool("t2"));
        let mut second = sample_pocket();
        second.tool = ToolId::new("t2");
        p.operations.push(Operation::Pocket(second));
        assert_eq!(
            validate(&p),
            vec![Problem::DuplicateToolNumber {
                tool: ToolId::new("t2"),
                name: "tool t2".into(),
                number: 2,
                other: ToolId::new("t1"),
            }]
        );
    }

    /// `T0` is no tool on most controllers.
    #[test]
    fn tool_number_zero_is_error() {
        let mut p = sample_project();
        p.tools[0].number = Some(0);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ToolOutOfRange {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
                field: "number",
                value: 0.0,
                expected: POSITIVE,
            }]
        );
        assert_eq!(problems[0].field().as_deref(), Some("tools[t1].number"));
    }

    /// Only tools of enabled operations are written to a program, so only
    /// they can clash.
    #[test]
    fn unused_tools_do_not_clash() {
        let mut p = sample_project();
        p.tools[0].number = Some(2);
        let mut t2 = tool("t2");
        t2.number = Some(2);
        p.tools.push(t2);
        assert_eq!(
            validate(&p),
            vec![Problem::UnusedTool {
                tool: ToolId::new("t2"),
                name: "tool t2".into(),
            }]
        );
    }

    /// Functional spec, capability 13: rest machining clears what an
    /// earlier Pocket of the same shapes left, so `rest_of` must name one.
    #[test]
    fn rest_of_must_be_an_earlier_enabled_pocket_with_the_same_targets() {
        let rough = sample_pocket();
        let mut rest = sample_pocket();
        rest.id = OperationId::new("o5");
        rest.name = "Rest".into();
        rest.rest_of = Some(OperationId::new("o2"));
        let p = with_ops(vec![
            Operation::Pocket(rough.clone()),
            Operation::Pocket(rest.clone()),
        ]);
        assert_eq!(validate(&p), vec![], "an earlier enabled Pocket");

        let invalid = Problem::RestOfInvalid {
            operation: OperationId::new("o5"),
            name: "Rest".into(),
            rest_of: OperationId::new("o2"),
        };
        assert_eq!(invalid.severity(), Severity::Error);
        assert_eq!(invalid.field().as_deref(), Some("operations[o5].rest_of"));

        // Later, not earlier.
        let p = with_ops(vec![
            Operation::Pocket(rest.clone()),
            Operation::Pocket(rough.clone()),
        ]);
        assert_eq!(validate(&p), vec![invalid.clone()], "a later Pocket");

        // Disabled.
        let mut off = rough.clone();
        off.enabled = false;
        let p = with_ops(vec![
            Operation::Pocket(off),
            Operation::Pocket(rest.clone()),
        ]);
        let problems = validate(&p);
        assert!(
            problems.contains(&invalid),
            "a disabled Pocket: {problems:?}"
        );

        // A Profile, not a Pocket.
        let mut rest_of_profile = rest.clone();
        rest_of_profile.rest_of = Some(OperationId::new("o1"));
        let p = with_ops(vec![Operation::Pocket(rest_of_profile)]);
        assert_eq!(
            validate(&p),
            vec![Problem::RestOfInvalid {
                operation: OperationId::new("o5"),
                name: "Rest".into(),
                rest_of: OperationId::new("o1"),
            }],
            "a Profile"
        );

        // Other targets.
        let mut other_targets = rough.clone();
        other_targets.targets = vec![InstanceId::new("i2")];
        let mut p = with_ops(vec![
            Operation::Pocket(other_targets),
            Operation::Pocket(rest.clone()),
        ]);
        p.instances.push(crate::FeatureInstance {
            id: InstanceId::new("i2"),
            ..p.instances[0].clone()
        });
        assert_eq!(validate(&p), vec![invalid.clone()], "other targets");

        // Nothing of that id at all.
        let mut dangling = rest.clone();
        dangling.rest_of = Some(OperationId::new("nope"));
        let p = with_ops(vec![Operation::Pocket(dangling)]);
        assert_eq!(
            validate(&p),
            vec![Problem::RestOfInvalid {
                operation: OperationId::new("o5"),
                name: "Rest".into(),
                rest_of: OperationId::new("nope"),
            }]
        );
    }

    #[test]
    fn step_over_fraction_outside_unit_interval_is_error() {
        for (fraction, ok) in [(0.0, false), (1.5, false), (1.0, true), (0.4, true)] {
            let mut pocket = sample_pocket();
            pocket.step_over_fraction = Some(fraction);
            let p = with_ops(vec![Operation::Pocket(pocket)]);
            let problems = validate(&p);
            if ok {
                assert_eq!(problems, vec![], "{fraction}");
            } else {
                assert_eq!(
                    problems,
                    vec![Problem::OutOfRange {
                        field: "operations[o2].step_over_fraction".into(),
                        value: fraction,
                        expected: FRACTION,
                    }],
                    "{fraction}"
                );
            }
        }
    }

    /// A Pocket or Engrave has no single-pass form: with no step-down on
    /// the operation or its tool nothing says how deep each pass is, and
    /// nothing defaults it (#98). Reported once, at the tool, as Profile
    /// reports it.
    #[test]
    fn pocket_without_step_down_is_error() {
        for op in [
            Operation::Pocket(sample_pocket()),
            Operation::Engrave(sample_engrave()),
        ] {
            let mut p = with_ops(vec![op.clone()]);
            p.tools[0].step_down_mm = None;
            // The Profile falls back on the tool too; take it out of the way.
            profile_mut(&mut p).step_down_mm = Some(2.0);
            assert_eq!(
                validate(&p),
                vec![Problem::ToolValueUnset {
                    tool: ToolId::new("t1"),
                    name: "6.35 mm upcut".into(),
                    field: "step_down_mm",
                }],
                "{op:?}"
            );
            // Set on the operation: fine.
            let mut p = with_ops(vec![op.clone()]);
            p.tools[0].step_down_mm = None;
            profile_mut(&mut p).step_down_mm = Some(2.0);
            match p.operations.get_mut(1) {
                Some(Operation::Pocket(pocket)) => pocket.step_down_mm = Some(1.0),
                Some(Operation::Engrave(engrave)) => engrave.step_down_mm = Some(1.0),
                _ => panic!("the added operation"),
            }
            assert_eq!(validate(&p), vec![], "{op:?}");
        }
    }

    /// Functional spec, capability 14: a peck's depth is the tool's
    /// step-down by default, so one of the two must be set. A simple
    /// cycle needs neither.
    #[test]
    fn peck_without_depth_or_tool_step_down_is_error() {
        let mut drill = sample_drill();
        drill.cycle = DrillCycle::Peck {
            peck_depth_mm: None,
            retract: PeckRetract::Full,
        };
        let mut p = with_ops(vec![Operation::Drill(drill.clone())]);
        p.tools[0].step_down_mm = None;
        profile_mut(&mut p).step_down_mm = Some(2.0);
        assert_eq!(
            validate(&p),
            vec![Problem::ToolValueUnset {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
                field: "step_down_mm",
            }]
        );
        drill.cycle = DrillCycle::Peck {
            peck_depth_mm: Some(1.5),
            retract: PeckRetract::Full,
        };
        let mut p = with_ops(vec![Operation::Drill(drill)]);
        p.tools[0].step_down_mm = None;
        profile_mut(&mut p).step_down_mm = Some(2.0);
        assert_eq!(validate(&p), vec![], "peck depth set");
        let mut p = with_ops(vec![Operation::Drill(sample_drill())]);
        p.tools[0].step_down_mm = None;
        profile_mut(&mut p).step_down_mm = Some(2.0);
        assert_eq!(validate(&p), vec![], "simple cycle");
    }

    /// Functional spec, capability 16: a helix sits in material the
    /// operation removes, and an On line Profile removes only its kerf.
    #[test]
    fn helix_on_on_line_profile_is_error() {
        let helix = Entry::Helix {
            diameter_fraction: 0.8,
            angle_deg: 5.0,
        };
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.side = Side::OnLine;
        op.entry = helix.clone();
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::HelixNeedsWasteSide {
                operation: OperationId::new("o1"),
                name: "Cut out".into(),
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].field().as_deref(), Some("operations[o1].entry"));
        // #207: Left and Right ramp along the offset path and have no
        // waste area for a helix either.
        for side in [Side::Left, Side::Right] {
            let mut p = sample_project();
            let op = profile_mut(&mut p);
            op.side = side;
            op.entry = helix.clone();
            assert_eq!(
                validate(&p),
                vec![Problem::HelixNeedsWasteSide {
                    operation: OperationId::new("o1"),
                    name: "Cut out".into(),
                }],
                "{side:?}"
            );
        }
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.side = Side::Inside;
        op.entry = helix;
        assert_eq!(validate(&p), vec![]);
        // A Pocket always has waste inside.
        let mut pocket = sample_pocket();
        pocket.entry = Entry::Helix {
            diameter_fraction: 0.8,
            angle_deg: 5.0,
        };
        assert_eq!(validate(&with_ops(vec![Operation::Pocket(pocket)])), vec![]);
    }

    /// Functional spec, capability 17: a lead is an arc of a positive
    /// radius sweeping at most a half circle, with a non-negative overcut.
    #[test]
    fn lead_radius_must_be_positive() {
        let good = Lead {
            radius_mm: 2.0,
            sweep_deg: 90.0,
            lead_out: true,
            overcut_mm: 0.0,
        };
        let mut p = sample_project();
        profile_mut(&mut p).lead = Some(good.clone());
        assert_eq!(validate(&p), vec![]);
        for (lead, field, value, expected) in [
            (
                Lead {
                    radius_mm: 0.0,
                    ..good.clone()
                },
                "radius_mm",
                0.0,
                POSITIVE,
            ),
            (
                Lead {
                    sweep_deg: 0.0,
                    ..good.clone()
                },
                "sweep_deg",
                0.0,
                SWEEP,
            ),
            (
                Lead {
                    sweep_deg: 181.0,
                    ..good.clone()
                },
                "sweep_deg",
                181.0,
                SWEEP,
            ),
            (
                Lead {
                    overcut_mm: -1.0,
                    ..good.clone()
                },
                "overcut_mm",
                -1.0,
                NON_NEGATIVE,
            ),
        ] {
            let mut p = sample_project();
            profile_mut(&mut p).lead = Some(lead);
            assert_eq!(
                validate(&p),
                vec![Problem::OutOfRange {
                    field: format!("operations[o1].lead.{field}"),
                    value,
                    expected,
                }],
                "{field} {value}"
            );
        }
    }

    /// The last pass acceptance 46 asks for, on the sample's Outside
    /// Profile (#284).
    fn last_pass(stock_to_leave_mm: f64) -> LastPass {
        LastPass {
            stock_to_leave_mm,
            thickness_mm: None,
            reverse: false,
        }
    }

    /// #284 (capability 34, acceptance 46): the stock to leave is more than 0
    /// and at most a third of the bit's diameter, as VCarve advises. 0, a
    /// negative value and 2.2 mm with the 6.35 mm bit are errors naming the
    /// diameter and the limit; 0.5 and exactly a third are valid.
    #[test]
    fn last_pass_stock_to_leave_limit() {
        let mut p = sample_project();
        for stock_to_leave_mm in [0.5, 6.35 / 3.0] {
            profile_mut(&mut p).last_pass = Some(last_pass(stock_to_leave_mm));
            assert_eq!(validate(&p), vec![], "{stock_to_leave_mm}");
        }
        for stock_to_leave_mm in [0.0, -0.5, 2.2] {
            profile_mut(&mut p).last_pass = Some(last_pass(stock_to_leave_mm));
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::LastPassStockToLeave {
                    operation: OperationId::new("o1"),
                    name: "Cut out".into(),
                    tool_name: "6.35 mm upcut".into(),
                    stock_to_leave_mm,
                    diameter_mm: 6.35,
                    limit_mm: 6.35 / 3.0,
                }],
                "{stock_to_leave_mm}"
            );
            let problem = &problems[0];
            assert!(problem.is_error());
            assert_eq!(problem.code(), "last_pass_stock_to_leave");
            assert_eq!(
                problem.field().as_deref(),
                Some("operations[o1].last_pass.stock_to_leave_mm")
            );
            let message = problem.to_string();
            for part in ["6.35 mm", "2.117 mm", "a third"] {
                assert!(message.contains(part), "{message}");
            }
        }
        profile_mut(&mut p).last_pass = Some(last_pass(f64::NAN));
        assert_eq!(
            validate(&p),
            vec![Problem::NonFinite {
                field: "operations[o1].last_pass.stock_to_leave_mm".into()
            }]
        );
    }

    /// #284: On line has no wall to leave material on: it cuts with both
    /// flanks. (changed in #311: Left and Right, which cut one flank
    /// against the part, take a last pass too.)
    #[test]
    fn last_pass_on_line_is_error() {
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.side = Side::OnLine;
        op.last_pass = Some(last_pass(0.5));
        let problems = validate(&p);
        let expected = Problem::LastPassNeedsSide {
            operation: OperationId::new("o1"),
            name: "Cut out".into(),
            side: Side::OnLine,
        };
        assert!(problems.contains(&expected), "{problems:?}");
        assert!(expected.is_error());
        assert_eq!(expected.code(), "last_pass_needs_side");
        assert_eq!(
            expected.field().as_deref(),
            Some("operations[o1].last_pass")
        );
        let message = expected.to_string();
        assert!(
            message.contains("Inside, Outside, Left or Right"),
            "{message}"
        );
        for side in [Side::Inside, Side::Outside] {
            let mut p = sample_project();
            let op = profile_mut(&mut p);
            op.side = side;
            op.last_pass = Some(last_pass(0.5));
            assert_eq!(validate(&p), vec![], "{side:?}");
        }
    }

    /// #311: a Left or Right Profile cuts one flank of the bit against the
    /// part, as Inside and Outside do, so it takes a last pass: no
    /// `LastPassNeedsSide`, and the stock to leave and thickness are checked
    /// as on Inside and Outside. On line still errors.
    #[test]
    fn last_pass_left_right_is_valid() {
        for side in [Side::Left, Side::Right] {
            let mut p = sample_project();
            let op = profile_mut(&mut p);
            op.side = side;
            op.tabs = None;
            op.last_pass = Some(last_pass(0.5));
            assert_eq!(validate(&p), vec![], "{side:?}");

            profile_mut(&mut p).last_pass = Some(last_pass(2.2));
            let problems = validate(&p);
            assert!(
                problems
                    .iter()
                    .any(|q| matches!(q, Problem::LastPassStockToLeave { .. })),
                "{side:?}: {problems:?}"
            );
            assert!(
                !problems
                    .iter()
                    .any(|q| matches!(q, Problem::LastPassNeedsSide { .. })),
                "{side:?}: {problems:?}"
            );
        }
        let mut p = sample_project();
        let op = profile_mut(&mut p);
        op.side = Side::OnLine;
        op.last_pass = Some(last_pass(0.5));
        assert!(
            validate(&p)
                .iter()
                .any(|q| matches!(q, Problem::LastPassNeedsSide { .. }))
        );
    }

    /// #284: `thickness_mm`, the last pass's depth, is more than 0 and at
    /// most the operation's `depth_mm`.
    #[test]
    fn last_pass_thickness_range() {
        let mut p = sample_project();
        for thickness_mm in [0.5, 2.0] {
            profile_mut(&mut p).last_pass = Some(LastPass {
                thickness_mm: Some(thickness_mm),
                ..last_pass(0.5)
            });
            assert_eq!(validate(&p), vec![], "{thickness_mm}");
        }
        for thickness_mm in [0.0, -1.0, 6.5] {
            profile_mut(&mut p).last_pass = Some(LastPass {
                thickness_mm: Some(thickness_mm),
                ..last_pass(0.5)
            });
            assert_eq!(
                validate(&p),
                vec![Problem::OutOfRange {
                    field: "operations[o1].last_pass.thickness_mm".into(),
                    value: thickness_mm,
                    expected: LAST_PASS_THICKNESS,
                }],
                "{thickness_mm}"
            );
        }
        // With the depth unset, only the depth is reported.
        let op = profile_mut(&mut p);
        op.depth_mm = None;
        op.last_pass = Some(LastPass {
            thickness_mm: Some(6.5),
            ..last_pass(0.5)
        });
        assert!(
            validate(&p)
                .iter()
                .all(|problem| !matches!(problem, Problem::OutOfRange { .. })),
            "{:?}",
            validate(&p)
        );
    }

    /// #310 review: the last pass is at most one step-down thick, the
    /// operation's when it sets one, else the tool's: the sample's 2 mm
    /// tool step-down allows 2 mm and refuses 2.5 and 6, at the thickness;
    /// with the operation's 4 mm, 3 is fine.
    #[test]
    fn last_pass_thickness_at_most_step_down() {
        let mut p = sample_project();
        for (thickness_mm, step_down_mm) in [(2.5, None), (6.0, None), (4.5, Some(4.0))] {
            let op = profile_mut(&mut p);
            op.step_down_mm = step_down_mm;
            op.last_pass = Some(LastPass {
                thickness_mm: Some(thickness_mm),
                ..last_pass(0.5)
            });
            let problems = validate(&p);
            let (field, step) = match step_down_mm {
                Some(step) => ("operations[o1].step_down_mm", step),
                None => ("tools[t1].step_down_mm", 2.0),
            };
            let expected = Problem::LastPassThickerThanStepDown {
                operation: OperationId::new("o1"),
                name: "Cut out".into(),
                thickness_mm,
                step_down_mm: step,
                step_down_field: field.into(),
            };
            assert_eq!(problems, vec![expected.clone()], "{thickness_mm}");
            assert!(expected.is_error());
            assert_eq!(expected.code(), "last_pass_thicker_than_step_down");
            assert_eq!(
                expected.field().as_deref(),
                Some("operations[o1].last_pass.thickness_mm")
            );
            let message = expected.to_string();
            assert!(
                message.contains(field) && message.contains("step-down"),
                "{message}"
            );
        }
        let op = profile_mut(&mut p);
        op.step_down_mm = Some(4.0);
        op.last_pass = Some(LastPass {
            thickness_mm: Some(3.0),
            ..last_pass(0.5)
        });
        assert_eq!(validate(&p), vec![]);
    }

    /// Functional spec, capability 16: a ramp steeper than 45° is a
    /// plunge in all but name; a helix is at most the tool's diameter.
    #[test]
    fn ramp_angle_range() {
        for (entry, field, value, expected) in [
            (
                Entry::Ramp { angle_deg: 0.0 },
                "entry.ramp.angle_deg",
                0.0,
                ENTRY_ANGLE,
            ),
            (
                Entry::Ramp { angle_deg: 46.0 },
                "entry.ramp.angle_deg",
                46.0,
                ENTRY_ANGLE,
            ),
            (
                Entry::Helix {
                    diameter_fraction: 0.0,
                    angle_deg: 5.0,
                },
                "entry.helix.diameter_fraction",
                0.0,
                FRACTION,
            ),
            (
                Entry::Helix {
                    diameter_fraction: 1.1,
                    angle_deg: 5.0,
                },
                "entry.helix.diameter_fraction",
                1.1,
                FRACTION,
            ),
            (
                Entry::Helix {
                    diameter_fraction: 0.8,
                    angle_deg: 50.0,
                },
                "entry.helix.angle_deg",
                50.0,
                ENTRY_ANGLE,
            ),
        ] {
            let mut pocket = sample_pocket();
            pocket.entry = entry.clone();
            let p = with_ops(vec![Operation::Pocket(pocket)]);
            assert_eq!(
                validate(&p),
                vec![Problem::OutOfRange {
                    field: format!("operations[o2].{field}"),
                    value,
                    expected,
                }],
                "{entry:?}"
            );
        }
        for entry in [
            Entry::Ramp { angle_deg: 45.0 },
            Entry::Ramp { angle_deg: 15.0 },
            Entry::Helix {
                diameter_fraction: 1.0,
                angle_deg: 45.0,
            },
        ] {
            let mut p = sample_project();
            profile_mut(&mut p).entry = entry.clone();
            assert_eq!(validate(&p), vec![], "{entry:?}");
        }
    }

    /// Functional spec, capability 14: canned cycles are written only
    /// where the definition has them; asking for them elsewhere would be
    /// silently ignored, so it is an error.
    #[test]
    fn canned_cycles_on_definition_without_them_is_error() {
        let mut p = sample_project();
        p.post_processor.use_canned_cycles = true;
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::CannedCyclesUnsupported {
                post: "grbl".into()
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(
            problems[0].field().as_deref(),
            Some("post_processor.use_canned_cycles")
        );
    }

    /// Functional spec, capability 14: Drill plunges any cutter, but a
    /// V-bit or ball nose makes a cone, not a hole, so it is a warning.
    #[test]
    fn drill_with_v_bit_warns() {
        let mut p = with_ops(vec![Operation::Drill(sample_drill())]);
        // The Profile needs an end mill; let the drill have its own tool.
        let mut v_bit = tool("t2");
        v_bit.cutter = ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        p.tools.push(v_bit);
        if let Some(Operation::Drill(drill)) = p.operations.get_mut(1) {
            drill.tool = ToolId::new("t2");
        }
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::DrillCutterUnusual {
                operation: OperationId::new("o3"),
                name: "Drill".into(),
                tool: ToolId::new("t2"),
                tool_name: "tool t2".into(),
                cutter: "a V-bit",
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
        assert_eq!(problems[0].field().as_deref(), Some("operations[o3].tool"));
        for cutter in [ToolKind::EndMill, ToolKind::Drill] {
            if let Some(t2) = p.tools.get_mut(1) {
                t2.cutter = cutter;
            }
            assert_eq!(validate(&p), vec![]);
        }
    }

    /// Functional spec, capability 18: the relief pass steps down at the
    /// tool's step-down; without one it is skipped, which the user should
    /// know. The Profile's own step-down keeps the main passes valid.
    #[test]
    fn relief_without_step_down_warns() {
        let mut p = sample_project();
        p.tools[0].step_down_mm = None;
        let op = profile_mut(&mut p);
        op.step_down_mm = Some(2.0);
        op.corner_relief = CornerRelief::Dogbone;
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ReliefNeedsStepDown {
                operation: OperationId::new("o1"),
                name: "Cut out".into(),
                tool: ToolId::new("t1"),
                tool_name: "6.35 mm upcut".into(),
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o1].corner_relief")
        );
        // A Pocket, likewise.
        let mut pocket = sample_pocket();
        pocket.step_down_mm = Some(2.0);
        pocket.corner_relief = CornerRelief::TBone;
        let mut p = with_ops(vec![Operation::Pocket(pocket)]);
        p.tools[0].step_down_mm = None;
        profile_mut(&mut p).step_down_mm = Some(2.0);
        let problems = validate(&p);
        assert!(
            matches!(problems.as_slice(), [Problem::ReliefNeedsStepDown { operation, .. }]
                if operation == &OperationId::new("o2")),
            "{problems:?}"
        );
        // With the tool's step-down set, nothing.
        p.tools[0].step_down_mm = Some(2.0);
        assert_eq!(validate(&p), vec![]);
    }

    /// Functional spec, capability 15: a V-bit engraving deeper than its
    /// cone is tall cuts with the shank. A 60° bit 6 mm across has a
    /// cone 3 / tan 30° = 5.196 mm tall.
    #[test]
    fn engrave_deeper_than_v_bit_cone_warns() {
        let mut p = with_ops(vec![Operation::Engrave(sample_engrave())]);
        let mut v_bit = tool("t2");
        v_bit.diameter_mm = 6.0;
        v_bit.cutter = ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        p.tools.push(v_bit);
        if let Some(Operation::Engrave(engrave)) = p.operations.get_mut(1) {
            engrave.tool = ToolId::new("t2");
            engrave.depth_mm = Some(6.0);
        }
        let problems = validate(&p);
        assert_eq!(problems.len(), 1, "{problems:?}");
        let Problem::EngraveDeeperThanCone {
            operation,
            name,
            tool_name,
            depth_mm,
            cone_height_mm,
        } = &problems[0]
        else {
            panic!("{problems:?}");
        };
        assert_eq!(operation, &OperationId::new("o4"));
        assert_eq!(name, "Engrave");
        assert_eq!(tool_name, "tool t2");
        assert!(approx_eq(*depth_mm, 6.0, 1e-9));
        assert!(approx_eq(*cone_height_mm, 5.196, 1e-3), "{cone_height_mm}");
        assert_eq!(problems[0].severity(), Severity::Warning);
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o4].depth_mm")
        );
        if let Some(Operation::Engrave(engrave)) = p.operations.get_mut(1) {
            engrave.depth_mm = Some(5.0);
        }
        assert_eq!(validate(&p), vec![]);
    }

    /// #67 applies to Pocket as to Profile: its rings are the wall offset
    /// by half the diameter, which only a flat end mill cuts at every
    /// depth.
    #[test]
    fn pocket_with_v_bit_is_error() {
        let mut p = with_ops(vec![Operation::Pocket(sample_pocket())]);
        let mut v_bit = tool("t2");
        v_bit.cutter = ToolKind::VBit {
            included_angle_deg: 60.0,
        };
        p.tools.push(v_bit);
        if let Some(Operation::Pocket(pocket)) = p.operations.get_mut(1) {
            pocket.tool = ToolId::new("t2");
        }
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::CutterNotFlat {
                operation: OperationId::new("o2"),
                name: "Pocket".into(),
                tool: ToolId::new("t2"),
                tool_name: "tool t2".into(),
                cutter: "a V-bit",
                field: "tool",
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
    }

    /// Format 18: a Bore's pitch is at most its tool's step-down, which
    /// is an error; at the step-down it is fine.
    #[test]
    fn bore_pitch_over_step_down_is_error() {
        let mut bore = sample_bore();
        bore.pitch_mm = Some(2.5);
        let p = with_ops(vec![Operation::Bore(bore)]);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::BorePitchOverStepDown {
                operation: OperationId::new("o5"),
                name: "Bore".into(),
                pitch_mm: 2.5,
                step_down_mm: 2.0,
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "bore_pitch_over_step_down");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o5].pitch_mm")
        );
        let mut bore = sample_bore();
        bore.pitch_mm = Some(2.0);
        assert_eq!(validate(&with_ops(vec![Operation::Bore(bore)])), vec![]);
        let mut bore = sample_bore();
        bore.pitch_mm = Some(0.0);
        assert_eq!(
            validate(&with_ops(vec![Operation::Bore(bore)])),
            vec![Problem::OutOfRange {
                field: "operations[o5].pitch_mm".into(),
                value: 0.0,
                expected: POSITIVE,
            }]
        );
    }

    /// Format 18: with no pitch a Bore takes the tool's step-down, so one
    /// of the two must be set, as for a Pocket.
    #[test]
    fn bore_without_pitch_or_step_down_is_error() {
        let mut p = with_ops(vec![Operation::Bore(sample_bore())]);
        p.tools[0].step_down_mm = None;
        profile_mut(&mut p).step_down_mm = Some(2.0);
        assert_eq!(
            validate(&p),
            vec![Problem::ToolValueUnset {
                tool: ToolId::new("t1"),
                name: "6.35 mm upcut".into(),
                field: "step_down_mm",
            }]
        );
        if let Some(Operation::Bore(bore)) = p.operations.get_mut(1) {
            bore.pitch_mm = Some(1.0);
        }
        assert_eq!(validate(&p), vec![]);
    }

    /// Format 18: the stock to leave is a distance of 0 or more.
    #[test]
    fn bore_negative_stock_to_leave_is_error() {
        let mut bore = sample_bore();
        bore.stock_to_leave_mm = -0.1;
        assert_eq!(
            validate(&with_ops(vec![Operation::Bore(bore)])),
            vec![Problem::OutOfRange {
                field: "operations[o5].stock_to_leave_mm".into(),
                value: -0.1,
                expected: NON_NEGATIVE,
            }]
        );
    }

    /// Format 18: a Bore cuts its wall with the side of the bit, so it
    /// needs a flat end mill, as a Pocket does.
    #[test]
    fn bore_needs_flat_end_mill() {
        for cutter in [ToolKind::Drill, ToolKind::BallNose] {
            let mut p = with_ops(vec![Operation::Bore(sample_bore())]);
            let mut t2 = tool("t2");
            t2.cutter = cutter.clone();
            p.tools.push(t2);
            if let Some(Operation::Bore(bore)) = p.operations.get_mut(1) {
                bore.tool = ToolId::new("t2");
            }
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::CutterNotFlat {
                    operation: OperationId::new("o5"),
                    name: "Bore".into(),
                    tool: ToolId::new("t2"),
                    tool_name: "tool t2".into(),
                    cutter: cutter.describe(),
                    field: "tool",
                }],
                "{cutter:?}"
            );
            assert_eq!(problems[0].severity(), Severity::Error);
        }
    }

    /// Functional spec, capability 14: the dwell and retract are
    /// distances and times of 0 or more; a chip-break gap of 0 would
    /// never clear the chip.
    #[test]
    fn drill_ranges() {
        let mut dwell = sample_drill();
        dwell.dwell_s = -1.0;
        let mut retract = sample_drill();
        retract.retract_mm = -0.5;
        let mut gap = sample_drill();
        gap.cycle = DrillCycle::Peck {
            peck_depth_mm: Some(1.0),
            retract: PeckRetract::ChipBreak { gap_mm: 0.0 },
        };
        let mut peck = sample_drill();
        peck.cycle = DrillCycle::Peck {
            peck_depth_mm: Some(0.0),
            retract: PeckRetract::Full,
        };
        for (drill, field, value, expected) in [
            (dwell, "dwell_s", -1.0, NON_NEGATIVE),
            (retract, "retract_mm", -0.5, NON_NEGATIVE),
            (gap, "cycle.peck.retract.chip_break.gap_mm", 0.0, POSITIVE),
            (peck, "cycle.peck.peck_depth_mm", 0.0, POSITIVE),
        ] {
            let p = with_ops(vec![Operation::Drill(drill)]);
            assert_eq!(
                validate(&p),
                vec![Problem::OutOfRange {
                    field: format!("operations[o3].{field}"),
                    value,
                    expected,
                }],
                "{field}"
            );
        }
        let mut stock = sample_pocket();
        stock.stock_to_leave_mm = -0.1;
        let p = with_ops(vec![Operation::Pocket(stock)]);
        assert_eq!(
            validate(&p),
            vec![Problem::OutOfRange {
                field: "operations[o2].stock_to_leave_mm".into(),
                value: -0.1,
                expected: NON_NEGATIVE,
            }]
        );
    }

    /// Functional spec, capability 12: the raster angle is from the X
    /// axis, 0 to 180 exclusive.
    #[test]
    fn raster_angle_range() {
        for (angle_deg, ok) in [(-1.0, false), (180.0, false), (0.0, true), (179.0, true)] {
            let mut pocket = sample_pocket();
            pocket.strategy = ClearanceStrategy::Raster { angle_deg };
            let p = with_ops(vec![Operation::Pocket(pocket)]);
            let problems = validate(&p);
            if ok {
                assert_eq!(problems, vec![], "{angle_deg}");
            } else {
                assert_eq!(
                    problems,
                    vec![Problem::OutOfRange {
                        field: "operations[o2].strategy.raster.angle_deg".into(),
                        value: angle_deg,
                        expected: RASTER_ANGLE,
                    }],
                    "{angle_deg}"
                );
            }
        }
    }

    /// Every new problem names a field the saved file has, so `check` can
    /// point at a line and column.
    #[test]
    fn phase_2_problem_fields_locate_in_the_saved_file() {
        let mut p = sample_project();
        p.tools[0].step_over_fraction = Some(2.0);
        p.post_processor.use_canned_cycles = true;
        let op = profile_mut(&mut p);
        op.side = Side::OnLine;
        op.entry = Entry::Helix {
            diameter_fraction: 1.5,
            angle_deg: 5.0,
        };
        op.lead = Some(Lead {
            radius_mm: -1.0,
            sweep_deg: 90.0,
            lead_out: true,
            overcut_mm: 0.0,
        });
        let mut pocket = sample_pocket();
        pocket.rest_of = Some(OperationId::new("nope"));
        pocket.step_over_fraction = Some(2.0);
        pocket.strategy = ClearanceStrategy::Raster { angle_deg: 180.0 };
        pocket.corner_relief = CornerRelief::Dogbone;
        pocket.step_down_mm = Some(1.0);
        let mut t2 = tool("t2");
        t2.step_down_mm = None;
        t2.number = Some(1);
        p.tools.push(t2);
        pocket.tool = ToolId::new("t2");
        let mut drill = sample_drill();
        drill.dwell_s = -1.0;
        let mut engrave = sample_engrave();
        engrave.depth_mm = Some(0.0);
        p.operations.extend([
            Operation::Pocket(pocket),
            Operation::Drill(drill),
            Operation::Engrave(engrave),
        ]);
        let problems = validate(&p);
        let text = crate::save(&p).unwrap();
        let map = crate::SourceMap::parse(&text).unwrap();
        let mut kinds: Vec<String> = Vec::new();
        for problem in &problems {
            let field = problem.field().unwrap();
            assert!(map.locate(&field).is_some(), "{field} in {text}");
            kinds.push(
                format!("{problem:?}")
                    .split([' ', '{'])
                    .next()
                    .unwrap()
                    .to_owned(),
            );
        }
        for expected in [
            "ToolOutOfRange",
            "DuplicateToolNumber",
            "CannedCyclesUnsupported",
            "HelixNeedsWasteSide",
            "RestOfInvalid",
            "ReliefNeedsStepDown",
            "OutOfRange",
        ] {
            assert!(
                kinds.contains(&expected.to_owned()),
                "{expected} in {kinds:?}"
            );
        }
    }

    // #283: the clearance, tool-change and park heights.

    /// Design addendum A7: the clearance height is for links between
    /// parts, so it may not be below an enabled operation's retract
    /// between passes. `clearance_mm: 3` with the sample Profile's
    /// `safe_z_mm: 5` names the operation, at the clearance field; 5 is
    /// valid, and a disabled operation does not count.
    #[test]
    fn clearance_below_safe_z_is_error() {
        let mut p = sample_project();
        p.heights.clearance_mm = Some(3.0);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ClearanceBelowSafeZ {
                operation: OperationId::new("o1"),
                name: "Cut out".into(),
                clearance_mm: 3.0,
                safe_z_mm: 5.0,
            }]
        );
        assert!(problems[0].is_error());
        assert_eq!(problems[0].field().as_deref(), Some("heights.clearance_mm"));
        assert!(
            problems[0].to_string().contains("Cut out"),
            "{}",
            problems[0]
        );
        let text = crate::save(&p).unwrap();
        let map = crate::SourceMap::parse(&text).unwrap();
        assert!(map.locate("heights.clearance_mm").is_some(), "{text}");

        p.heights.clearance_mm = Some(5.0);
        assert_eq!(validate(&p), vec![]);
        p.heights.clearance_mm = Some(3.0);
        profile_mut(&mut p).enabled = false;
        assert!(
            !validate(&p)
                .iter()
                .any(|q| matches!(q, Problem::ClearanceBelowSafeZ { .. })),
            "a disabled operation is not cut"
        );
    }

    /// Design addendum A7: grblHAL and GRBL home Z at the top of its
    /// travel, to a negative machine position (grblHAL `machine_limits.c`
    /// 116-140, GRBL `limits.c` 382), so a lift to machine Z 0 or above
    /// goes past the limit switch. `0` and `2` are errors; `-5` is valid.
    #[test]
    fn machine_z_must_be_below_zero() {
        for machine_z_mm in [0.0, 2.0] {
            let mut p = sample_project();
            p.heights.tool_change_machine_z_mm = Some(machine_z_mm);
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::MachineZNotBelowHome { machine_z_mm }],
                "{machine_z_mm}"
            );
            assert!(problems[0].is_error());
            assert_eq!(
                problems[0].field().as_deref(),
                Some("heights.tool_change_machine_z_mm")
            );
        }
        let mut p = sample_project();
        p.heights.tool_change_machine_z_mm = Some(-5.0);
        assert_eq!(validate(&p), vec![]);
    }

    /// #304 review: the program writes the machine Z to its decimal places
    /// (3 in mm, 4 in inches), so one that rounds to 0 there is the same
    /// lift past the limit switch: −0.0004 mm is refused in a mm program,
    /// −0.001 mm in an inch program (under half of 0.0001 in, 0.00127
    /// mm), while −0.001 mm and −0.0013 mm write −0.001 and −0.0001.
    #[test]
    fn machine_z_rounding_to_zero_is_error() {
        for (units, machine_z_mm, refused) in [
            (crate::Units::Mm, -0.0004, true),
            (crate::Units::Mm, -0.001, false),
            (crate::Units::Inch, -0.001, true),
            (crate::Units::Inch, -0.0013, false),
        ] {
            let mut p = sample_project();
            p.display_units = units;
            p.heights.tool_change_machine_z_mm = Some(machine_z_mm);
            let problems = validate(&p);
            let expected = if refused {
                vec![Problem::MachineZNotBelowHome { machine_z_mm }]
            } else {
                vec![]
            };
            assert_eq!(problems, expected, "{units:?} {machine_z_mm}");
        }
    }

    /// Design addendum A7: the lift is `G53 G0 Z…`, which only a
    /// definition that documents `G53` may write (`definitions/README.md`,
    /// "Machine-coordinate moves"). On LinuxCNC, whose documents were not
    /// checked for it, the setting is an error naming the post-processor;
    /// on GRBL (the sample's) it is valid.
    #[test]
    fn machine_z_needs_g53_definition() {
        // #306: Mach3, UCCNC and generic document no `G53` that DreamCarve
        // could check, on any version.
        for definition in crate::bundled_definitions()
            .unwrap()
            .iter()
            .filter(|d| ["mach3", "uccnc", "generic"].contains(&d.id))
        {
            let mut p = sample_project();
            p.heights.tool_change_machine_z_mm = Some(-5.0);
            p.post_processor = definition.default_ref();
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::MachineZNeedsG53 {
                    post: definition.id.into()
                }],
                "{}",
                definition.path
            );
            assert!(problems[0].is_error());
            assert!(
                problems[0].to_string().contains(definition.id),
                "{}",
                problems[0]
            );
            assert_eq!(
                problems[0].field().as_deref(),
                Some("heights.tool_change_machine_z_mm")
            );
        }
    }

    /// #306: LinuxCNC documents `G53` (definitions README,
    /// "Machine-coordinate moves"), so both versions accept a machine Z,
    /// with the same rule as grblHAL and GRBL: below 0 as written.
    #[test]
    fn machine_z_accepted_on_linuxcnc() {
        for version in [1, 2] {
            let mut p = sample_project();
            p.post_processor = crate::PostProcessorRef::latest("linuxcnc").unwrap();
            p.post_processor.definition_version = version;
            p.heights.tool_change_machine_z_mm = Some(-5.0);
            assert_eq!(validate(&p), vec![], "v{version}");
            for machine_z_mm in [0.0, 5.0, -0.0004] {
                p.heights.tool_change_machine_z_mm = Some(machine_z_mm);
                assert_eq!(
                    validate(&p),
                    vec![Problem::MachineZNotBelowHome { machine_z_mm }],
                    "v{version} {machine_z_mm}"
                );
            }
        }
    }

    /// #283: the park point is a coordinate, within the 6000 mm limit
    /// (#75); the clearance height is positive.
    #[test]
    fn park_and_clearance_are_in_range() {
        let mut p = sample_project();
        p.heights.park = Some(crate::ParkXy {
            x_mm: 7000.0,
            y_mm: 0.0,
        });
        let problems = validate(&p);
        assert!(
            matches!(problems.as_slice(), [Problem::OutOfRange { field, .. }] if field == "heights.park.x_mm"),
            "{problems:?}"
        );
        p.heights.park = Some(crate::ParkXy {
            x_mm: 0.0,
            y_mm: 0.0,
        });
        assert_eq!(validate(&p), vec![]);
        profile_mut(&mut p).enabled = false;
        p.heights.clearance_mm = Some(-1.0);
        let problems = validate(&p);
        assert!(
            problems.iter().any(
                |q| matches!(q, Problem::OutOfRange { field, .. } if field == "heights.clearance_mm")
            ),
            "{problems:?}"
        );
    }

    // Format 20: the V-Carve (`docs/specs/phase-3/design.md`, "Validation").

    /// The sample project with the 90°, 6.35 mm V-bit `t2`, a 10 mm
    /// square `i2` inside the sample rectangle, and a V-Carve `o6` of the
    /// square, as `edit` leaves it.
    fn carve_project(edit: impl FnOnce(&mut crate::model::VCarveOp)) -> Project {
        let mut p = sample_project();
        p.tools.push(crate::test_support::tool_v_bit_90());
        let square = DefinitionId::new("d2");
        p.definitions.insert(
            square.clone(),
            crate::model::FeatureDefinition {
                id: square.clone(),
                name: "Square".into(),
                profiles: vec![Polyline::rect(
                    dc_geometry::Point { x: 0.0, y: 0.0 },
                    10.0,
                    10.0,
                )],
            },
        );
        p.instances.push(crate::model::FeatureInstance {
            id: InstanceId::new("i2"),
            definition: square,
            transform: Transform::translate(20.0, 20.0),
            name: "Square 1".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(6.0),
            z_bottom_mm: Some(0.0),
        });
        let mut carve = crate::test_support::sample_v_carve();
        carve.targets = vec![InstanceId::new("i2")];
        edit(&mut carve);
        p.operations.push(Operation::VCarve(carve));
        p
    }

    /// The carve of [`carve_project`], to edit.
    fn carve_mut(p: &mut Project) -> &mut crate::model::VCarveOp {
        p.operations
            .iter_mut()
            .find_map(|op| match op {
                Operation::VCarve(op) => Some(op),
                _ => None,
            })
            .unwrap()
    }

    /// An enabled Pocket `o2` of the sample rectangle, which contains the
    /// square of [`carve_project`], `depth_mm` deep.
    fn rectangle_pocket(depth_mm: f64) -> Operation {
        Operation::Pocket(PocketOp {
            depth_mm: Some(depth_mm),
            ..sample_pocket()
        })
    }

    /// A tool `id` as the sample's, `diameter_mm` wide and cutting with
    /// `cutter`.
    fn tool_of(id: &str, diameter_mm: f64, cutter: ToolKind) -> Tool {
        Tool {
            diameter_mm,
            cutter,
            ..tool(id)
        }
    }

    /// The one problem `problems` holds, which must be a
    /// [`Problem::VBitConeExceeded`]: its engagement, cone height,
    /// whether it counts from the stock top, and its field.
    fn cone_exceeded(problems: &[Problem]) -> (f64, f64, bool, Option<String>) {
        match problems {
            [
                problem @ Problem::VBitConeExceeded {
                    engagement_mm,
                    cone_height_mm,
                    from_stock_top,
                    ..
                },
            ] => {
                assert_eq!(problem.severity(), Severity::Error);
                assert_eq!(problem.code(), "v_bit_cone_exceeded");
                (
                    *engagement_mm,
                    *cone_height_mm,
                    *from_stock_top,
                    problem.field(),
                )
            }
            _ => panic!("expected one v_bit_cone_exceeded: {problems:?}"),
        }
    }

    /// Capability 38: a V-Carve's depth follows a V-bit's angle, so any
    /// other cutter is an error at the operation's tool.
    #[test]
    fn v_carve_with_end_mill_is_error() {
        assert_eq!(validate(&carve_project(|_| {})), vec![]);
        for cutter in [
            ToolKind::EndMill,
            ToolKind::BallNose,
            ToolKind::Engraver,
            ToolKind::Drill,
        ] {
            let mut p = carve_project(|_| {});
            p.tools[1].cutter = cutter.clone();
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::VCarveNeedsVBit {
                    operation: OperationId::new("o6"),
                    name: "V-Carve".into(),
                    tool: ToolId::new("t2"),
                    tool_name: "90° V-bit".into(),
                    cutter: cutter.describe(),
                }],
                "{cutter:?}"
            );
            assert_eq!(problems[0].severity(), Severity::Error);
            assert_eq!(problems[0].code(), "v_carve_needs_v_bit");
            assert_eq!(problems[0].field().as_deref(), Some("operations[o6].tool"));
        }
    }

    /// A V-bit a V-Carve uses must have an angle above 0° and below 180°:
    /// `v_bit_angle_range` at the tool's angle, instead of the
    /// `tool_out_of_range` any other V-bit gets.
    #[test]
    fn v_bit_angle_out_of_range_is_error() {
        for angle in [0.0, 180.0, 200.0, -10.0] {
            let mut p = carve_project(|carve| carve.flat_depth_mm = Some(1.0));
            p.tools[1].cutter = ToolKind::VBit {
                included_angle_deg: angle,
            };
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::VBitAngleRange {
                    tool: ToolId::new("t2"),
                    name: "90° V-bit".into(),
                    included_angle_deg: angle,
                }],
                "{angle}"
            );
            assert_eq!(problems[0].severity(), Severity::Error);
            assert_eq!(problems[0].code(), "v_bit_angle_range");
            assert_eq!(
                problems[0].field().as_deref(),
                Some("tools[t2].cutter.v_bit.included_angle_deg")
            );
        }
        // A V-bit no V-Carve uses keeps the general range check.
        let mut p = carve_project(|_| {});
        p.operations.pop();
        p.operations.push(Operation::Engrave(EngraveOp {
            tool: ToolId::new("t2"),
            step_down_mm: Some(1.0),
            ..sample_engrave()
        }));
        p.tools[1].cutter = ToolKind::VBit {
            included_angle_deg: 180.0,
        };
        assert_eq!(
            validate(&p),
            vec![Problem::ToolOutOfRange {
                tool: ToolId::new("t2"),
                name: "90° V-bit".into(),
                field: "cutter.v_bit.included_angle_deg",
                value: 180.0,
                expected: ANGLE,
            }]
        );
    }

    /// Capability 39: a flat depth deeper than the bit's cone is tall
    /// would cut with the shank. The 90°, 6.35 mm bit's cone is 3.175 mm.
    #[test]
    fn flat_depth_beyond_cone_is_error() {
        let p = carve_project(|carve| carve.flat_depth_mm = Some(3.5));
        let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&validate(&p));
        assert!(approx_eq(engagement_mm, 3.5, 1e-9), "{engagement_mm}");
        assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
        assert!(!from_stock_top);
        assert_eq!(field.as_deref(), Some("operations[o6].flat_depth_mm"));
        let message = validate(&p)[0].to_string();
        assert!(message.contains("3.175 mm"), "{message}");

        for (flat_depth_mm, angle) in [(3.0, 90.0), (3.175, 90.0), (3.5, 60.0)] {
            let mut p = carve_project(|carve| carve.flat_depth_mm = Some(flat_depth_mm));
            p.tools[1].cutter = ToolKind::VBit {
                included_angle_deg: angle,
            };
            assert_eq!(validate(&p), vec![], "{flat_depth_mm} mm, {angle}°");
        }
        // The flat depth is a depth, more than 0.
        let p = carve_project(|carve| carve.flat_depth_mm = Some(0.0));
        assert_eq!(
            validate(&p),
            vec![Problem::OutOfRange {
                field: "operations[o6].flat_depth_mm".into(),
                value: 0.0,
                expected: POSITIVE,
            }]
        );
    }

    fn pass(tool: &str) -> crate::model::ClearingPass {
        crate::model::ClearingPass {
            tool: ToolId::new(tool),
            strategy: ClearanceStrategy::Offset,
            direction: crate::model::CutDirection::Climb,
            entry: Entry::Plunge,
        }
    }

    /// Capability 40: clearing passes clear the flat floor, so a carve with
    /// none may not have any.
    #[test]
    fn clearing_needs_a_flat_depth() {
        let p = carve_project(|carve| carve.clearing = vec![pass("t1")]);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ClearingNeedsFlatDepth {
                operation: OperationId::new("o6"),
                name: "V-Carve".into(),
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "clearing_needs_flat_depth");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o6].clearing")
        );
        let p = carve_project(|carve| {
            carve.clearing = vec![pass("t1")];
            carve.flat_depth_mm = Some(1.0);
        });
        assert_eq!(validate(&p), vec![]);
    }

    /// Capability 40: a clearing pass is cut as a Pocket, so its tool must
    /// be a flat end mill; the carve's own V-bit is not one.
    #[test]
    fn clearing_pass_needs_a_flat_end_mill() {
        for cutter in [
            ToolKind::BallNose,
            ToolKind::Drill,
            ToolKind::VBit {
                included_angle_deg: 60.0,
            },
        ] {
            let mut p = carve_project(|carve| {
                carve.clearing = vec![pass("t1"), pass("t3")];
                carve.flat_depth_mm = Some(1.0);
            });
            p.tools.push(tool_of("t3", 3.175, cutter.clone()));
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::ClearingNeedsEndMill {
                    operation: OperationId::new("o6"),
                    name: "V-Carve".into(),
                    pass: 1,
                    tool: ToolId::new("t3"),
                    tool_name: "tool t3".into(),
                    cutter: cutter.describe(),
                }],
                "{cutter:?}"
            );
            assert_eq!(problems[0].severity(), Severity::Error);
            assert_eq!(problems[0].code(), "clearing_needs_end_mill");
            assert_eq!(
                problems[0].field().as_deref(),
                Some("operations[o6].clearing[1].tool")
            );
        }
        // The carve's own V-bit, given the step-down a pass steps at.
        let mut p = carve_project(|carve| {
            carve.clearing = vec![pass("t2")];
            carve.flat_depth_mm = Some(1.0);
        });
        p.tools[1].step_down_mm = Some(1.0);
        assert!(
            matches!(
                validate(&p).as_slice(),
                [Problem::ClearingNeedsEndMill {
                    pass: 0,
                    cutter: "a V-bit",
                    ..
                }]
            ),
            "{:?}",
            validate(&p)
        );
    }

    /// Capability 40: a pass whose tool is wider than the one before still
    /// runs, so it is a warning. A clearing tool is a used tool: its rates
    /// and step-down, which the pass steps down at, are required.
    #[test]
    fn clearing_wider_than_the_pass_before_warns() {
        let mut p = carve_project(|carve| {
            carve.clearing = vec![pass("t3"), pass("t1")];
            carve.flat_depth_mm = Some(1.0);
        });
        p.tools.push(tool_of("t3", 3.175, ToolKind::EndMill));
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ClearingNotDescending {
                operation: OperationId::new("o6"),
                name: "V-Carve".into(),
                pass: 1,
                tool_name: "6.35 mm upcut".into(),
                diameter_mm: 6.35,
                previous_tool_name: "tool t3".into(),
                previous_diameter_mm: 3.175,
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
        assert_eq!(problems[0].code(), "clearing_not_descending");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o6].clearing[1].tool")
        );
        // Largest first, or the same size twice, is quiet.
        carve_mut(&mut p).clearing = vec![pass("t1"), pass("t3"), pass("t3")];
        assert_eq!(validate(&p), vec![]);

        p.tools[2].step_down_mm = None;
        p.tools[2].feed_mm_min = None;
        assert_eq!(
            validate(&p),
            vec![
                Problem::ToolValueUnset {
                    tool: ToolId::new("t3"),
                    name: "tool t3".into(),
                    field: "feed_mm_min",
                },
                Problem::ToolValueUnset {
                    tool: ToolId::new("t3"),
                    name: "tool t3".into(),
                    field: "step_down_mm",
                },
            ]
        );
        // A clearing pass's strategy and entry follow the Pocket's rules.
        let p = carve_project(|carve| {
            carve.clearing = vec![crate::model::ClearingPass {
                strategy: ClearanceStrategy::Raster { angle_deg: 180.0 },
                entry: Entry::Ramp { angle_deg: 60.0 },
                ..pass("t1")
            }];
            carve.flat_depth_mm = Some(1.0);
        });
        assert_eq!(
            validate(&p),
            vec![
                Problem::OutOfRange {
                    field: "operations[o6].clearing[0].strategy.raster.angle_deg".into(),
                    value: 180.0,
                    expected: RASTER_ANGLE,
                },
                Problem::OutOfRange {
                    field: "operations[o6].clearing[0].entry.ramp.angle_deg".into(),
                    value: 60.0,
                    expected: ENTRY_ANGLE,
                },
            ]
        );
    }

    /// A clearing pass names a tool of the project, as an operation does.
    #[test]
    fn clearing_pass_naming_no_tool_is_error() {
        let p = carve_project(|carve| {
            carve.clearing = vec![pass("t9")];
            carve.flat_depth_mm = Some(1.0);
        });
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::ClearingDanglingTool {
                operation: OperationId::new("o6"),
                pass: 0,
                tool: ToolId::new("t9"),
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "clearing_dangling_tool");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o6].clearing[0].tool")
        );
    }

    /// Capability 41: the start plane is a depth below the stock top.
    #[test]
    fn negative_start_depth_is_error() {
        let p = carve_project(|carve| carve.start_depth_mm = -1.0);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::StartDepthNegative {
                operation: OperationId::new("o6"),
                name: "V-Carve".into(),
                start_depth_mm: -1.0,
                field: "start_depth_mm",
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "start_depth_negative");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o6].start_depth_mm")
        );
        let p = carve_project(|carve| carve.start_depth_mm = f64::NAN);
        assert_eq!(
            validate(&p),
            vec![Problem::NonFinite {
                field: "operations[o6].start_depth_mm".into(),
            }]
        );
    }

    /// Capability 41, security review M2: a carve below the stock top
    /// needs an enabled earlier Pocket containing its targets, or the
    /// user's word that another program cleared the start plane. A
    /// disabled Pocket, or one after the carve, does not count.
    #[test]
    fn uncleared_start_depth_is_an_error_unless_acknowledged() {
        let uncleared = |p: &Project| {
            validate(p)
                .into_iter()
                .filter(|problem| matches!(problem, Problem::StartDepthUncleared { .. }))
                .collect::<Vec<_>>()
        };
        let p = carve_project(|carve| {
            carve.start_depth_mm = 1.0;
            carve.flat_depth_mm = Some(1.0);
        });
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::StartDepthUncleared {
                operation: OperationId::new("o6"),
                name: "V-Carve".into(),
                start_depth_mm: 1.0,
                field: "start_depth_mm",
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "start_depth_uncleared");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o6].start_depth_mm")
        );

        let mut acknowledged = p.clone();
        carve_mut(&mut acknowledged).start_plane_cleared_externally = true;
        assert_eq!(validate(&acknowledged), vec![]);

        // A containing Pocket at the depth, before the carve.
        let mut pocketed = p.clone();
        pocketed.operations.insert(1, rectangle_pocket(1.0));
        assert_eq!(validate(&pocketed), vec![]);

        let mut disabled = pocketed.clone();
        if let Some(Operation::Pocket(pocket)) = disabled.operations.get_mut(1) {
            pocket.enabled = false;
        }
        assert_eq!(uncleared(&disabled).len(), 1, "{:?}", validate(&disabled));

        let mut after = p.clone();
        after.operations.push(rectangle_pocket(1.0));
        assert_eq!(uncleared(&after).len(), 1, "{:?}", validate(&after));

        // A Pocket that does not contain the square: a 10 mm square
        // elsewhere.
        let mut elsewhere = pocketed.clone();
        elsewhere.instances[0].transform = Transform::translate(40.0, 50.0);
        assert_eq!(uncleared(&elsewhere).len(), 1, "{:?}", validate(&elsewhere));

        // From the stock top, nothing needs clearing.
        let p = carve_project(|carve| carve.flat_depth_mm = Some(1.0));
        assert_eq!(validate(&p), vec![]);
    }

    /// Security review H4: with the start plane only vouched for, the bit's
    /// flanks engage from the stock top, so the start depth counts toward
    /// the cone: 4 + 1 mm with the 90° bit's 3.175 mm cone fails, while
    /// a Pocket of the start depth makes it 1 mm and passes.
    #[test]
    fn acknowledged_start_depth_counts_toward_the_cone() {
        let p = carve_project(|carve| {
            carve.start_depth_mm = 4.0;
            carve.start_plane_cleared_externally = true;
            carve.flat_depth_mm = Some(1.0);
        });
        let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&validate(&p));
        assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
        assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
        assert!(from_stock_top);
        assert_eq!(field.as_deref(), Some("operations[o6].flat_depth_mm"));
        let message = validate(&p)[0].to_string();
        assert!(message.contains("stock top"), "{message}");

        // Cleared by a Pocket at the start depth: the flat depth alone.
        let mut pocketed = p.clone();
        carve_mut(&mut pocketed).start_plane_cleared_externally = false;
        pocketed.operations.insert(1, rectangle_pocket(4.0));
        assert_eq!(validate(&pocketed), vec![]);

        // A Pocket at another depth leaves the start plane in material the
        // project does not clear: from the stock top again.
        let mut shallower = pocketed.clone();
        shallower.operations[1] = rectangle_pocket(3.0);
        let (engagement_mm, _, from_stock_top, _) = cone_exceeded(&validate(&shallower));
        assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
        assert!(from_stock_top);

        // With no flat depth the start depth alone already exceeds it.
        let mut full = p.clone();
        carve_mut(&mut full).flat_depth_mm = None;
        let (engagement_mm, _, from_stock_top, field) = cone_exceeded(&validate(&full));
        assert!(approx_eq(engagement_mm, 4.0, 1e-9), "{engagement_mm}");
        assert!(from_stock_top);
        assert_eq!(field.as_deref(), Some("operations[o6].start_depth_mm"));
    }

    /// Security review L6: the flatten tolerance is at most 0.1 mm.
    #[test]
    fn flatten_tolerance_above_0_1_mm_is_error() {
        let mut p = sample_project();
        p.flatten_tolerance_mm = 0.2;
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::FlattenToleranceRange {
                flatten_tolerance_mm: 0.2,
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "flatten_tolerance_range");
        assert_eq!(problems[0].field().as_deref(), Some("flatten_tolerance_mm"));
        assert!(approx_eq(MAX_FLATTEN_TOLERANCE_MM, 0.1, 1e-9));
        p.flatten_tolerance_mm = MAX_FLATTEN_TOLERANCE_MM;
        assert_eq!(validate(&p), vec![]);
        p.flatten_tolerance_mm = 0.0;
        assert_eq!(
            validate(&p),
            vec![Problem::OutOfRange {
                field: "flatten_tolerance_mm".into(),
                value: 0.0,
                expected: POSITIVE,
            }]
        );
    }

    // Format 21: the countersink Drill (functional spec, capability 44).

    /// The sample project with the 90°, 6.35 mm V-bit `t2` (a 3.175 mm
    /// cone) and a 3.175 mm circle `i3` at (35, 25), inside the sample
    /// rectangle, countersunk 6 mm wide from the stock top by Drill `o3`
    /// with no `depth_mm`, edited by `edit`.
    fn countersink_project(edit: impl FnOnce(&mut crate::model::DrillOp)) -> Project {
        let mut p = sample_project();
        p.tools.push(crate::test_support::tool_v_bit_90());
        let circle = DefinitionId::new("d3");
        p.definitions.insert(
            circle.clone(),
            crate::model::FeatureDefinition {
                id: circle.clone(),
                name: "Hole".into(),
                profiles: vec![Polyline::circle(
                    dc_geometry::Point { x: 0.0, y: 0.0 },
                    3.175 / 2.0,
                )],
            },
        );
        p.instances.push(crate::model::FeatureInstance {
            id: InstanceId::new("i3"),
            definition: circle,
            transform: Transform::translate(35.0, 25.0),
            name: "Hole 1".into(),
            visible: true,
            locked: false,
            z_top_mm: Some(6.0),
            z_bottom_mm: Some(0.0),
        });
        let mut drill = sample_drill();
        drill.targets = vec![InstanceId::new("i3")];
        drill.tool = ToolId::new("t2");
        drill.depth_mm = None;
        drill.cycle = DrillCycle::Countersink {
            diameter_mm: 6.0,
            start_depth_mm: 0.0,
            start_plane_cleared_externally: false,
        };
        edit(&mut drill);
        p.operations.push(Operation::Drill(drill));
        p
    }

    /// The countersink's settings of `drill`, to edit.
    fn countersink_of(drill: &mut crate::model::DrillOp) -> (&mut f64, &mut f64, &mut bool) {
        match &mut drill.cycle {
            DrillCycle::Countersink {
                diameter_mm,
                start_depth_mm,
                start_plane_cleared_externally,
            } => (diameter_mm, start_depth_mm, start_plane_cleared_externally),
            cycle => panic!("not a countersink: {cycle:?}"),
        }
    }

    /// Capability 44: the mouth can be no wider than the V-bit, past
    /// whose diameter there is no cutting edge; a mouth the bit's own
    /// diameter is the most it cuts. A mouth that is not above 0 is its
    /// own error.
    #[test]
    fn countersink_mouth_wider_than_tool_is_error() {
        let p = countersink_project(|drill| *countersink_of(drill).0 = 7.0);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::CountersinkMouthExceedsTool {
                operation: OperationId::new("o3"),
                name: "Drill".into(),
                tool_name: "90° V-bit".into(),
                diameter_mm: 7.0,
                tool_diameter_mm: 6.35,
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "countersink_mouth_exceeds_tool");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o3].cycle.countersink.diameter_mm")
        );
        let p = countersink_project(|drill| *countersink_of(drill).0 = 6.35);
        assert_eq!(validate(&p), vec![]);

        for diameter_mm in [0.0, -6.0] {
            let p = countersink_project(|drill| *countersink_of(drill).0 = diameter_mm);
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::CountersinkDiameterPositive {
                    operation: OperationId::new("o3"),
                    name: "Drill".into(),
                    diameter_mm,
                }],
                "{diameter_mm}"
            );
            assert_eq!(problems[0].severity(), Severity::Error);
            assert_eq!(problems[0].code(), "countersink_diameter_positive");
            assert_eq!(
                problems[0].field().as_deref(),
                Some("operations[o3].cycle.countersink.diameter_mm")
            );
        }
        let p = countersink_project(|drill| *countersink_of(drill).0 = f64::NAN);
        assert_eq!(
            validate(&p),
            vec![Problem::NonFinite {
                field: "operations[o3].cycle.countersink.diameter_mm".into(),
            }]
        );
    }

    /// Capabilities 41 and 44, security review H4: the flanks engage
    /// from the stock top unless an enabled earlier Pocket at the start
    /// depth contains the bit at every hole, so a 6 mm mouth (3 mm deep
    /// with the 90° bit) 2 mm down a start plane only vouched for engages
    /// 5 mm of a 3.175 mm cone. The same countersink on the floor of a
    /// 2 mm Pocket engages 3 mm, which fits; so does one from the top.
    #[test]
    fn countersink_depth_beyond_cone_is_error() {
        let vouched = countersink_project(|drill| {
            let (_, start_depth_mm, cleared_externally) = countersink_of(drill);
            *start_depth_mm = 2.0;
            *cleared_externally = true;
        });
        let problems = validate(&vouched);
        let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&problems);
        assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
        assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
        assert!(from_stock_top);
        assert_eq!(
            field.as_deref(),
            Some("operations[o3].cycle.countersink.diameter_mm")
        );
        let message = problems[0].to_string();
        assert!(
            message.contains("from the stock top") && message.contains("countersink's depth"),
            "{message}"
        );

        let mut pocketed = countersink_project(|drill| *countersink_of(drill).1 = 2.0);
        pocketed.operations.insert(1, rectangle_pocket(2.0));
        assert_eq!(validate(&pocketed), vec![]);

        // A Pocket at another depth does not clear the start plane: the
        // flanks engage from the stock top again.
        let mut shallower = countersink_project(|drill| *countersink_of(drill).1 = 2.0);
        shallower.operations.insert(1, rectangle_pocket(1.0));
        let (engagement_mm, _, from_stock_top, _) = cone_exceeded(&validate(&shallower));
        assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
        assert!(from_stock_top);

        assert_eq!(validate(&countersink_project(|_| {})), vec![]);
    }

    /// Capability 44, security review M2: a countersink below the stock
    /// top needs an earlier Pocket that holds its whole bit at every
    /// hole, or the user's word, as a V-Carve does; located at its own
    /// start depth. A negative start depth is an error there too.
    #[test]
    fn uncleared_countersink_start_depth_is_an_error() {
        // 0.1 mm down, so the flanks engaged from the stock top (3.1 mm)
        // fit the 3.175 mm cone and the plane is the one problem.
        let p = countersink_project(|drill| *countersink_of(drill).1 = 0.1);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::StartDepthUncleared {
                operation: OperationId::new("o3"),
                name: "Drill".into(),
                start_depth_mm: 0.1,
                field: "cycle.countersink.start_depth_mm",
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o3].cycle.countersink.start_depth_mm")
        );
        assert!(
            problems[0]
                .to_string()
                .contains("the whole of its V-bit at each hole"),
            "{}",
            problems[0]
        );
        let mut pocketed = p.clone();
        pocketed.operations.insert(1, rectangle_pocket(0.1));
        assert_eq!(validate(&pocketed), vec![]);
        // Deeper, the uncleared plane counts toward the cone as well.
        let deeper = countersink_project(|drill| *countersink_of(drill).1 = 1.0);
        let codes: Vec<&str> = validate(&deeper).iter().map(Problem::code).collect();
        assert_eq!(codes, ["start_depth_uncleared", "v_bit_cone_exceeded"]);
        let vouched = countersink_project(|drill| {
            let (_, start_depth_mm, cleared_externally) = countersink_of(drill);
            *start_depth_mm = 0.1;
            *cleared_externally = true;
        });
        assert_eq!(validate(&vouched), vec![]);

        let p = countersink_project(|drill| *countersink_of(drill).1 = -1.0);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::StartDepthNegative {
                operation: OperationId::new("o3"),
                name: "Drill".into(),
                start_depth_mm: -1.0,
                field: "cycle.countersink.start_depth_mm",
            }]
        );
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o3].cycle.countersink.start_depth_mm")
        );
    }

    /// An enabled V-Carve `o6` of the sample rectangle with the 90° V-bit
    /// `t2` and a 2.5 mm flat depth from the stock top: its floor is the
    /// rectangle inset 2.5 mm, x 12.5..57.5, y 12.5..37.5.
    fn rectangle_v_carve() -> Operation {
        let mut carve = crate::test_support::sample_v_carve();
        carve.flat_depth_mm = Some(2.5);
        Operation::VCarve(carve)
    }

    /// Issue #426: the countersink's hole at (35, 25) sits on the 2.5 mm
    /// floor of a V-Carve before it, which holds the whole 6.35 mm bit,
    /// so a countersink 2.5 mm down is neither uncleared nor past its
    /// cone: the flanks engage its own 3 mm depth from the floor. At
    /// 2 mm the floor is not its plane (the order check's mismatch), so
    /// the flanks engage from the stock top, 5 mm of a 3.175 mm cone. A
    /// hole on the carve's slope, outside the floor, is uncleared.
    #[test]
    fn countersink_on_a_v_carve_floor_is_not_uncleared() {
        let mut floored = countersink_project(|drill| *countersink_of(drill).1 = 2.5);
        floored.operations.insert(1, rectangle_v_carve());
        assert_eq!(validate(&floored), vec![]);

        let mut mismatched = countersink_project(|drill| *countersink_of(drill).1 = 2.0);
        mismatched.operations.insert(1, rectangle_v_carve());
        let (engagement_mm, _, from_stock_top, _) = cone_exceeded(&validate(&mismatched));
        assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
        assert!(from_stock_top);

        let mut on_the_slope = countersink_project(|drill| *countersink_of(drill).1 = 0.1);
        on_the_slope.operations.insert(1, rectangle_v_carve());
        on_the_slope.instances[1].transform = Transform::translate(11.5, 25.0);
        let codes: Vec<&str> = validate(&on_the_slope).iter().map(Problem::code).collect();
        assert_eq!(codes, ["start_depth_uncleared"]);
    }

    /// The design spec, "Validation": a countersink's V-bit is no
    /// `drill_cutter_unusual`, and its unset `depth_mm` is not an unset
    /// value (the depth is derived). Any cutter but a V-bit is
    /// `countersink_needs_v_bit` at the tool, and a V-bit angle out of
    /// range is `v_bit_angle_range`, as for a V-Carve.
    #[test]
    fn countersink_drill_with_v_bit_is_not_unusual() {
        let p = countersink_project(|_| {});
        assert_eq!(validate(&p), vec![]);
        let p = countersink_project(|drill| drill.depth_mm = Some(3.0));
        assert_eq!(validate(&p), vec![]);

        for cutter in [ToolKind::Drill, ToolKind::EndMill, ToolKind::BallNose] {
            let mut p = countersink_project(|_| {});
            p.tools[1].cutter = cutter.clone();
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::CountersinkNeedsVBit {
                    operation: OperationId::new("o3"),
                    name: "Drill".into(),
                    tool: ToolId::new("t2"),
                    tool_name: "90° V-bit".into(),
                    cutter: cutter.describe(),
                }],
                "{cutter:?}"
            );
            assert_eq!(problems[0].severity(), Severity::Error);
            assert_eq!(problems[0].code(), "countersink_needs_v_bit");
            assert_eq!(problems[0].field().as_deref(), Some("operations[o3].tool"));
        }

        for angle in [0.0, 180.0] {
            let mut p = countersink_project(|_| {});
            p.tools[1].cutter = ToolKind::VBit {
                included_angle_deg: angle,
            };
            assert_eq!(
                validate(&p),
                vec![Problem::VBitAngleRange {
                    tool: ToolId::new("t2"),
                    name: "90° V-bit".into(),
                    included_angle_deg: angle,
                }],
                "{angle}"
            );
        }
    }

    /// Design spec, "Countersink" step 2: Drill's narrower-hole rule does
    /// not apply to a countersink, so `allow_tool_wider_than_hole: true`
    /// does nothing, and validation says so at the field. A warning.
    #[test]
    fn countersink_ignores_tool_width_warns() {
        let p = countersink_project(|drill| drill.allow_tool_wider_than_hole = true);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::CountersinkIgnoresToolWidth {
                operation: OperationId::new("o3"),
                name: "Drill".into(),
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
        assert_eq!(problems[0].code(), "countersink_ignores_tool_width");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o3].allow_tool_wider_than_hole")
        );
        // A simple Drill uses the field.
        let mut p = with_ops(vec![Operation::Drill(sample_drill())]);
        if let Some(Operation::Drill(drill)) = p.operations.get_mut(1) {
            drill.allow_tool_wider_than_hole = true;
        }
        assert_eq!(validate(&p), vec![]);
    }

    // Format 22: the Inlay (`docs/specs/phase-3/design.md`, "Validation"
    // and "Algorithms → Inlay", step 1).

    /// The project of [`carve_project`] on a 19 mm board, with the
    /// V-Carve replaced by the Inlay `o7` of the square `i2`
    /// ([`crate::test_support::sample_inlay`] with a 0.5 mm plug depth:
    /// female, `D 2.5, G 0.25, P 0.5`, margin 6, the 90° 6.35 mm V-bit
    /// `t2`, so either side fits the cone), as `edit` leaves it.
    fn inlay_project(edit: impl FnOnce(&mut crate::model::InlayOp)) -> Project {
        let mut p = carve_project(|_| {});
        p.stock.thickness_mm = Some(19.0);
        p.operations.pop();
        let mut inlay = crate::test_support::sample_inlay();
        inlay.targets = vec![InstanceId::new("i2")];
        inlay.plug_depth_mm = Some(0.5);
        edit(&mut inlay);
        p.operations.push(Operation::Inlay(inlay));
        p
    }

    /// The star test part's values (functional spec, "Inlay pair"): `D 4,
    /// G 1, P 2`, margin 8, on `side`.
    fn star(inlay: &mut crate::model::InlayOp, side: crate::model::InlaySide) {
        inlay.side = side;
        inlay.depth_mm = Some(4.0);
        inlay.glue_gap_mm = Some(1.0);
        inlay.plug_depth_mm = Some(2.0);
        inlay.margin_mm = Some(8.0);
    }

    /// Sets the V-bit `t2` of `p` to `angle` degrees.
    fn v_bit_angle(p: &mut Project, angle: f64) {
        p.tools[1].cutter = ToolKind::VBit {
            included_angle_deg: angle,
        };
    }

    const SIDES: [crate::model::InlaySide; 2] = [
        crate::model::InlaySide::Female,
        crate::model::InlaySide::Male,
    ];

    /// Capability 45: the glue gap is left under the seated plug, so it
    /// is 0 or more and below the depth, on either side; at or above it
    /// the male's start plane would be at or above the stock top.
    #[test]
    fn inlay_glue_gap_must_be_below_depth() {
        for side in SIDES {
            assert_eq!(
                validate(&inlay_project(|inlay| inlay.side = side)),
                vec![],
                "{side:?}"
            );
            for glue_gap_mm in [2.5, 3.0] {
                let p = inlay_project(|inlay| {
                    inlay.side = side;
                    inlay.glue_gap_mm = Some(glue_gap_mm);
                });
                let problems = validate(&p);
                assert_eq!(
                    problems,
                    vec![Problem::InlayGlueGapNotBelowDepth {
                        operation: OperationId::new("o7"),
                        name: "Inlay".into(),
                        glue_gap_mm,
                        depth_mm: 2.5,
                    }],
                    "{side:?} {glue_gap_mm}"
                );
                assert_eq!(problems[0].severity(), Severity::Error);
                assert_eq!(problems[0].code(), "inlay_glue_gap_not_below_depth");
                assert_eq!(
                    problems[0].field().as_deref(),
                    Some("operations[o7].glue_gap_mm")
                );
                let message = problems[0].to_string();
                assert!(
                    message.contains(&format!("glue_gap_mm {glue_gap_mm} mm"))
                        && message.contains("depth_mm 2.5 mm"),
                    "{message}"
                );
            }
            // 0 is a plug that bottoms exactly; just below the depth fits.
            for glue_gap_mm in [0.0, 2.4] {
                let p = inlay_project(|inlay| {
                    inlay.side = side;
                    inlay.glue_gap_mm = Some(glue_gap_mm);
                });
                assert_eq!(validate(&p), vec![], "{side:?} {glue_gap_mm}");
            }
            let p = inlay_project(|inlay| {
                inlay.side = side;
                inlay.glue_gap_mm = Some(-0.25);
            });
            assert_eq!(
                validate(&p),
                vec![Problem::OutOfRange {
                    field: "operations[o7].glue_gap_mm".into(),
                    value: -0.25,
                    expected: NON_NEGATIVE,
                }],
                "{side:?}"
            );
        }
    }

    /// Capability 45: the depth, glue gap and plug depth describe both
    /// sides, so each is required on either; unset, each is
    /// `operation_value_unset` at its field, in file order, and set, the
    /// depths are above 0.
    #[test]
    fn inlay_values_must_be_set() {
        for side in SIDES {
            let p = inlay_project(|inlay| {
                inlay.side = side;
                inlay.depth_mm = None;
                inlay.glue_gap_mm = None;
                inlay.plug_depth_mm = None;
            });
            let problems = validate(&p);
            let unset = |field| Problem::OperationValueUnset {
                operation: OperationId::new("o7"),
                name: "Inlay".into(),
                field,
            };
            assert_eq!(
                problems,
                vec![
                    unset("depth_mm"),
                    unset("glue_gap_mm"),
                    unset("plug_depth_mm")
                ],
                "{side:?}"
            );
            assert!(problems.iter().all(Problem::is_unset_value));
            assert_eq!(
                problems[2].field().as_deref(),
                Some("operations[o7].plug_depth_mm")
            );
            let p = inlay_project(|inlay| {
                inlay.side = side;
                inlay.depth_mm = Some(0.0);
                inlay.plug_depth_mm = Some(-1.0);
            });
            assert_eq!(
                validate(&p),
                vec![
                    Problem::OutOfRange {
                        field: "operations[o7].depth_mm".into(),
                        value: 0.0,
                        expected: POSITIVE,
                    },
                    Problem::OutOfRange {
                        field: "operations[o7].plug_depth_mm".into(),
                        value: -1.0,
                        expected: POSITIVE,
                    },
                ],
                "{side:?}"
            );
        }
    }

    /// Capability 45: the male's surround is the design grown by
    /// `margin_mm`, so a male needs one above 0; the female ignores it.
    #[test]
    fn inlay_male_needs_margin() {
        use crate::model::InlaySide;
        let p = inlay_project(|inlay| {
            inlay.side = InlaySide::Male;
            inlay.margin_mm = None;
        });
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::OperationValueUnset {
                operation: OperationId::new("o7"),
                name: "Inlay".into(),
                field: "margin_mm",
            }]
        );
        assert!(problems[0].is_unset_value());
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o7].margin_mm")
        );
        for margin_mm in [0.0, -1.0] {
            let p = inlay_project(|inlay| {
                inlay.side = InlaySide::Male;
                inlay.margin_mm = Some(margin_mm);
            });
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::InlayMarginPositive {
                    operation: OperationId::new("o7"),
                    name: "Inlay".into(),
                    margin_mm,
                }],
                "{margin_mm}"
            );
            assert_eq!(problems[0].severity(), Severity::Error);
            assert_eq!(problems[0].code(), "inlay_margin_positive");
            assert_eq!(
                problems[0].field().as_deref(),
                Some("operations[o7].margin_mm")
            );
            // The female does not use it.
            let p = inlay_project(|inlay| inlay.margin_mm = Some(margin_mm));
            assert_eq!(validate(&p), vec![], "female {margin_mm}");
        }
        let p = inlay_project(|inlay| inlay.margin_mm = None);
        assert_eq!(validate(&p), vec![], "female, no margin");
    }

    /// Design spec, "Inlay" step 4: a male leaving less than 6 mm of
    /// board under its surround's floor warns. The star's values put the
    /// floor 4 − 1 + 2 = 5 mm down: 5 mm left on a 10 mm board, 6 on an
    /// 11 mm one, 7 on 12. The female leaves no plug base.
    #[test]
    fn thin_plug_base_warns() {
        use crate::model::InlaySide;
        let mut p = inlay_project(|inlay| star(inlay, InlaySide::Male));
        v_bit_angle(&mut p, 60.0);
        p.stock.thickness_mm = Some(10.0);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::InlayPlugBaseThin {
                operation: OperationId::new("o7"),
                name: "Inlay".into(),
                floor_depth_mm: 5.0,
                thickness_mm: 10.0,
                base_mm: 5.0,
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Warning);
        assert_eq!(problems[0].code(), "inlay_plug_base_thin");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o7].plug_depth_mm")
        );
        assert!(approx_eq(INLAY_MIN_PLUG_BASE_MM, 6.0, 0.0));
        for thickness_mm in [11.0, 12.0] {
            p.stock.thickness_mm = Some(thickness_mm);
            assert_eq!(validate(&p), vec![], "{thickness_mm} mm");
        }
        let mut female = inlay_project(|inlay| star(inlay, InlaySide::Female));
        v_bit_angle(&mut female, 60.0);
        female.stock.thickness_mm = Some(10.0);
        assert_eq!(validate(&female), vec![]);
    }

    /// Security review H4: the male's flank engages from the stock top,
    /// `(D − G) + P`, since the plug above its start plane is never
    /// cleared. Corbin's `D 2.5, G 0.25, P 1.5` engages 3.75 mm, past the
    /// 90° 6.35 mm bit's 3.175 mm cone, though `P` alone fits; the star's
    /// `4, 1, 2` engages 5 mm, within the 60° bit's 5.5 mm.
    #[test]
    fn male_inlay_engagement_beyond_the_cone_is_error() {
        use crate::model::InlaySide;
        let corbin = |inlay: &mut crate::model::InlayOp| {
            inlay.depth_mm = Some(2.5);
            inlay.glue_gap_mm = Some(0.25);
            inlay.plug_depth_mm = Some(1.5);
        };
        let p = inlay_project(|inlay| {
            corbin(inlay);
            inlay.side = InlaySide::Male;
        });
        let problems = validate(&p);
        let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&problems);
        assert!(approx_eq(engagement_mm, 3.75, 1e-9), "{engagement_mm}");
        assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
        assert!(from_stock_top);
        assert_eq!(field.as_deref(), Some("operations[o7].plug_depth_mm"));
        let message = problems[0].to_string();
        assert!(
            message.contains("3.75 mm")
                && message.contains("3.175 mm")
                && message.contains("plug_depth_mm")
                && !message.contains("Pocket"),
            "{message}"
        );

        let mut p = inlay_project(|inlay| star(inlay, InlaySide::Male));
        v_bit_angle(&mut p, 60.0);
        assert_eq!(validate(&p), vec![]);
        // The star on the 90° bit engages 5 mm, past its 3.175 mm cone.
        let p = inlay_project(|inlay| star(inlay, InlaySide::Male));
        let (engagement_mm, ..) = cone_exceeded(&validate(&p));
        assert!(approx_eq(engagement_mm, 5.0, 1e-9), "{engagement_mm}");
    }

    /// Design spec, "Inlay" step 1: the female starts at the stock top,
    /// so its engagement is its depth alone, whatever the glue gap and
    /// plug depth its male uses; located at `depth_mm`.
    #[test]
    fn female_inlay_engagement_is_its_depth() {
        // Its male would engage 3.5 − 0.25 + 1.5 = 4.75 mm.
        let p = inlay_project(|inlay| {
            inlay.depth_mm = Some(3.5);
            inlay.plug_depth_mm = Some(1.5);
        });
        let problems = validate(&p);
        let (engagement_mm, cone_height_mm, from_stock_top, field) = cone_exceeded(&problems);
        assert!(approx_eq(engagement_mm, 3.5, 1e-9), "{engagement_mm}");
        assert!(approx_eq(cone_height_mm, 3.175, 1e-9), "{cone_height_mm}");
        assert!(!from_stock_top);
        assert_eq!(field.as_deref(), Some("operations[o7].depth_mm"));
        // 3 mm fits, though its male would engage 3 − 0.25 + 3 mm.
        let p = inlay_project(|inlay| {
            inlay.depth_mm = Some(3.0);
            inlay.plug_depth_mm = Some(3.0);
        });
        assert_eq!(validate(&p), vec![]);
        let mut p = inlay_project(|inlay| star(inlay, crate::model::InlaySide::Female));
        v_bit_angle(&mut p, 60.0);
        assert_eq!(validate(&p), vec![]);
    }

    /// Capability 45: an Inlay carves with a V-bit, as a V-Carve does:
    /// any other cutter is `v_carve_needs_v_bit` at its tool, and its
    /// bit's angle out of range is `v_bit_angle_range`.
    #[test]
    fn inlay_with_end_mill_is_error() {
        for side in SIDES {
            for cutter in [
                ToolKind::EndMill,
                ToolKind::BallNose,
                ToolKind::Engraver,
                ToolKind::Drill,
            ] {
                let mut p = inlay_project(|inlay| inlay.side = side);
                p.tools[1].cutter = cutter.clone();
                let problems = validate(&p);
                assert_eq!(
                    problems,
                    vec![Problem::VCarveNeedsVBit {
                        operation: OperationId::new("o7"),
                        name: "Inlay".into(),
                        tool: ToolId::new("t2"),
                        tool_name: "90° V-bit".into(),
                        cutter: cutter.describe(),
                    }],
                    "{side:?} {cutter:?}"
                );
                assert_eq!(problems[0].severity(), Severity::Error);
                assert_eq!(problems[0].code(), "v_carve_needs_v_bit");
                assert_eq!(problems[0].field().as_deref(), Some("operations[o7].tool"));
                assert!(
                    !problems[0].to_string().contains("V-Carve"),
                    "{}",
                    problems[0]
                );
            }
            for angle in [0.0, 180.0] {
                let mut p = inlay_project(|inlay| inlay.side = side);
                v_bit_angle(&mut p, angle);
                assert_eq!(
                    validate(&p),
                    vec![Problem::VBitAngleRange {
                        tool: ToolId::new("t2"),
                        name: "90° V-bit".into(),
                        included_angle_deg: angle,
                    }],
                    "{side:?} {angle}"
                );
            }
        }
    }

    // Format 23 (#443): a male Inlay's cut-out.

    /// [`inlay_project`] with the 3.175 mm end mill `t3` added and a
    /// cut-out with it, four 6 mm tabs 3 mm tall, 0.2 mm through, as
    /// `edit` leaves the Inlay.
    fn cut_out_project(edit: impl FnOnce(&mut crate::model::InlayOp)) -> Project {
        use crate::model::{InlayCutOut, TabPlacement, TabShape, TabSpec};
        let mut p = inlay_project(|inlay| {
            inlay.side = crate::model::InlaySide::Male;
            inlay.cut_out = Some(InlayCutOut {
                tool: ToolId::new("t3"),
                tabs: Some(TabSpec {
                    placement: TabPlacement::Count { count: 4 },
                    width_mm: 6.0,
                    height_mm: 3.0,
                    shape: TabShape::Rect,
                    on_holes: false,
                    avoid_corners: true,
                }),
                through_cut_allowance_mm: 0.2,
            });
            edit(inlay);
        });
        p.tools.push(Tool {
            id: ToolId::new("t3"),
            name: "1/8 end mill".into(),
            diameter_mm: 3.175,
            ..crate::test_support::tool_6_35_upcut()
        });
        p
    }

    /// The male of [`cut_out_project`] is valid: its 6 mm band, 0.5 mm
    /// deep on the 90° bit, has a floor ring 5 mm wide.
    #[test]
    fn cut_out_on_a_male_is_valid() {
        assert_eq!(validate(&cut_out_project(|_| {})), vec![]);
    }

    /// #443: only the male has a band to cut the plug out of; a female's
    /// pocket is in the board it stays in. A cut-out on a female is
    /// `inlay_cut_out_needs_male`, at `cut_out`, an error.
    #[test]
    fn cut_out_on_a_female_is_an_error() {
        let p = cut_out_project(|inlay| inlay.side = crate::model::InlaySide::Female);
        let problems = validate(&p);
        assert_eq!(
            problems,
            vec![Problem::InlayCutOutNeedsMale {
                operation: OperationId::new("o7"),
                name: "Inlay".into(),
            }]
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "inlay_cut_out_needs_male");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o7].cut_out")
        );
        assert!(
            problems[0].to_string().contains("female"),
            "{}",
            problems[0]
        );
    }

    /// #443: the cut-out's tool runs On line round the plug and through
    /// the board; it must be a flat end mill, `cutter_not_flat` at
    /// `cut_out.tool` otherwise.
    #[test]
    fn cut_out_needs_a_flat_end_mill() {
        for cutter in [
            ToolKind::BallNose,
            ToolKind::Engraver,
            ToolKind::VBit {
                included_angle_deg: 60.0,
            },
        ] {
            let mut p = cut_out_project(|_| {});
            p.tools[2].cutter = cutter.clone();
            let problems = validate(&p);
            assert_eq!(
                problems,
                vec![Problem::CutterNotFlat {
                    operation: OperationId::new("o7"),
                    name: "Inlay".into(),
                    tool: ToolId::new("t3"),
                    tool_name: "1/8 end mill".into(),
                    cutter: cutter.describe(),
                    field: "cut_out.tool",
                }],
                "{cutter:?}"
            );
            assert_eq!(problems[0].code(), "cutter_not_flat");
            assert_eq!(
                problems[0].field().as_deref(),
                Some("operations[o7].cut_out.tool")
            );
            assert!(
                problems[0].to_string().contains("cut-out"),
                "{}",
                problems[0]
            );
        }
    }

    /// #443: the tool centre runs half the margin out, so the floor ring,
    /// `M − 2·P·tan(α/2)` wide, must hold the tool's diameter. The star
    /// with `margin_mm: 4` on the 60° bit (`P·s = 1.155`) leaves 1.69 mm
    /// for the 3.175 mm end mill: `inlay_cut_out_no_room`, at
    /// `margin_mm`, naming the margin needed, 2.309 + 3.175 = 5.484 mm.
    #[test]
    fn cut_out_with_no_room_in_the_floor_ring_is_an_error() {
        let mut p = cut_out_project(|inlay| {
            star(inlay, crate::model::InlaySide::Male);
            inlay.margin_mm = Some(4.0);
        });
        v_bit_angle(&mut p, 60.0);
        let problems = validate(&p);
        assert_eq!(problems.len(), 1, "{problems:?}");
        let Problem::InlayCutOutNoRoom {
            operation,
            margin_mm,
            needed_mm,
            diameter_mm,
            ..
        } = &problems[0]
        else {
            panic!("{problems:?}");
        };
        assert_eq!(operation, &OperationId::new("o7"));
        dc_geometry::assert_approx_eq!(*margin_mm, 4.0, 1e-12);
        dc_geometry::assert_approx_eq!(*diameter_mm, 3.175, 1e-12);
        dc_geometry::assert_approx_eq!(
            *needed_mm,
            4.0 * (30.0_f64).to_radians().tan() + 3.175,
            1e-9
        );
        assert_eq!(problems[0].severity(), Severity::Error);
        assert_eq!(problems[0].code(), "inlay_cut_out_no_room");
        assert_eq!(
            problems[0].field().as_deref(),
            Some("operations[o7].margin_mm")
        );
        assert!(problems[0].to_string().contains("5.484"), "{}", problems[0]);

        // The star's own 8 mm margin has room.
        let mut p = cut_out_project(|inlay| star(inlay, crate::model::InlaySide::Male));
        v_bit_angle(&mut p, 60.0);
        assert_eq!(validate(&p), vec![]);
    }

    /// #443: the cut-out's tool is a used tool: its feed, plunge rate and
    /// RPM are required, and so is its step-down, which sets the passes.
    #[test]
    fn cut_out_tool_needs_its_rates() {
        for field in ["feed_mm_min", "plunge_mm_min", "rpm", "step_down_mm"] {
            let mut p = cut_out_project(|_| {});
            let tool = &mut p.tools[2];
            match field {
                "feed_mm_min" => tool.feed_mm_min = None,
                "plunge_mm_min" => tool.plunge_mm_min = None,
                "rpm" => tool.rpm = None,
                _ => tool.step_down_mm = None,
            }
            assert_eq!(
                validate(&p),
                vec![Problem::ToolValueUnset {
                    tool: ToolId::new("t3"),
                    name: "1/8 end mill".into(),
                    field,
                }],
                "{field}"
            );
        }
    }

    /// #443: the cut-out's fields follow their Profile counterparts': its
    /// tool must exist, its allowance be 0 or more, and its tabs as a
    /// Profile's tab request.
    #[test]
    fn cut_out_fields_are_checked() {
        let p = cut_out_project(|inlay| {
            let cut_out = inlay.cut_out.as_mut().unwrap();
            cut_out.through_cut_allowance_mm = -0.1;
            cut_out.tabs.as_mut().unwrap().width_mm = 0.0;
        });
        let fields: Vec<Option<String>> = validate(&p).iter().map(Problem::field).collect();
        assert_eq!(
            fields,
            [
                Some("operations[o7].cut_out.tabs.width_mm".to_owned()),
                Some("operations[o7].cut_out.through_cut_allowance_mm".to_owned()),
            ]
        );
        let p = cut_out_project(|inlay| inlay.cut_out.as_mut().unwrap().tool = ToolId::new("t9"));
        assert_eq!(
            validate(&p),
            vec![
                Problem::UnusedTool {
                    tool: ToolId::new("t3"),
                    name: "1/8 end mill".into(),
                },
                Problem::DanglingTool {
                    operation: OperationId::new("o7"),
                    tool: ToolId::new("t9"),
                },
            ]
        );
    }

    /// Capability 40: an Inlay's clearing passes follow the V-Carve's
    /// rules, on either side, and an Inlay always has a flat depth, so it
    /// never needs one for them. Each clearing list gives an Inlay the
    /// problems it gives a floored V-Carve of the same id and name.
    #[test]
    fn inlay_clearing_rules_match_v_carve() {
        use crate::model::ClearingPass;
        let raster = ClearingPass {
            strategy: ClearanceStrategy::Raster { angle_deg: 180.0 },
            entry: Entry::Ramp { angle_deg: 60.0 },
            ..pass("t1")
        };
        let lists = [
            vec![pass("t1")],
            vec![pass("t1"), pass("t3")],
            vec![pass("t3"), pass("t1")],
            vec![pass("t1"), pass("t4")],
            vec![pass("t2")],
            vec![pass("t9")],
            vec![raster],
        ];
        // `t3` a 3.175 mm end mill with no rates or step-down: a clearing
        // tool is a used tool, so they are required. `t4` a V-bit.
        let tools = |p: &mut Project| {
            p.tools.push(Tool {
                feed_mm_min: None,
                step_down_mm: None,
                ..tool_of("t3", 3.175, ToolKind::EndMill)
            });
            p.tools.push(tool_of(
                "t4",
                6.35,
                ToolKind::VBit {
                    included_angle_deg: 60.0,
                },
            ));
        };
        for side in SIDES {
            for clearing in &lists {
                let mut inlay = inlay_project(|inlay| {
                    inlay.side = side;
                    inlay.depth_mm = Some(2.0);
                    inlay.glue_gap_mm = Some(0.5);
                    inlay.plug_depth_mm = Some(1.0);
                    inlay.clearing.clone_from(clearing);
                });
                tools(&mut inlay);
                let mut carve = carve_project(|carve| {
                    carve.id = OperationId::new("o7");
                    carve.name = "Inlay".into();
                    carve.flat_depth_mm = Some(1.0);
                    carve.clearing.clone_from(clearing);
                });
                carve.stock.thickness_mm = Some(19.0);
                tools(&mut carve);
                assert_eq!(validate(&inlay), validate(&carve), "{side:?} {clearing:?}");
            }
        }
        // The clearing passes' problems are the V-Carve's own.
        let mut p = inlay_project(|inlay| inlay.clearing = vec![pass("t1"), pass("t4")]);
        tools(&mut p);
        let codes: Vec<&str> = validate(&p).iter().map(Problem::code).collect();
        assert!(codes.contains(&"clearing_needs_end_mill"), "{codes:?}");
        assert!(!codes.contains(&"clearing_needs_flat_depth"), "{codes:?}");
    }
}
