// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The crate's error type. Every safety violation found in an emitted
//! program is an [`ExportError`], never a warning
//! (`docs/specs/phase-1/design.md`, "Emission", step 7).

use dc_cam::FeedField;
use dc_project::{OperationId, ToolId};

/// Why G-code could not be exported. When `emit` returns one of these, no
/// program is produced.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// No bundled post-processor has this id.
    #[error("post-processor `{id}` is not bundled")]
    UnknownPostProcessor {
        /// The id that was asked for.
        id: String,
    },
    /// A bundled definition file failed to parse. Only a broken build can
    /// produce this; `post::tests::bundled_definitions_validate` guards it.
    #[error("bundled post-processor definition `{file}` is invalid")]
    BundledDefinition {
        /// File name under `definitions/`.
        file: &'static str,
        /// The parse failure.
        #[source]
        source: serde_json::Error,
    },
    /// There is nothing to export: no operation is enabled.
    #[error("no enabled operation to export")]
    NoEnabledOperations,
    /// An operation's tool is not in the project's tool list.
    #[error("operation `{op}` uses tool `{tool}`, which is not in the project")]
    UnknownTool {
        /// The operation.
        op: OperationId,
        /// The missing tool id.
        tool: ToolId,
    },
    /// A tool lacks a feed or plunge rate: unset, or not a positive number.
    /// Rates are never invented.
    #[error("tool `{tool}` has no usable {field}")]
    MissingFeed {
        /// The tool's name.
        tool: String,
        /// Which rate is missing.
        field: FeedField,
    },
    /// A tool lacks a spindle speed, or it is 0. Speeds are never invented.
    #[error("tool `{tool}` has no spindle speed (rpm)")]
    MissingRpm {
        /// The tool's name.
        tool: String,
    },
    /// The post-processor lacks a command the safety rules need (S5): the
    /// units command for the output units, or the work-coordinate select.
    #[error("post-processor `{post}` has no `{field}`, which every program must start with")]
    MissingCommand {
        /// The post-processor's id.
        post: String,
        /// The missing field, as it is spelled in the definition.
        field: &'static str,
    },
    /// The post-processor's axis mapping is not usable: two project axes
    /// share a machine axis, or project Z does not drive machine Z.
    #[error(
        "post-processor `{post}` maps axes {x}, {y}, {z}; each project axis needs its own machine axis, and Z must stay +Z"
    )]
    InvalidAxisMapping {
        /// The post-processor's id.
        post: String,
        /// Machine axis for project X, as written in the definition.
        x: String,
        /// Machine axis for project Y.
        y: String,
        /// Machine axis for project Z.
        z: String,
    },
    /// The spindle dwell is negative or not a number.
    #[error("post-processor `{post}` has spindle dwell {duration_s} s; it must be 0 or more")]
    InvalidDwell {
        /// The post-processor's id.
        post: String,
        /// The dwell, seconds.
        duration_s: f64,
    },
    /// The post-processor writes too few decimal places for the output
    /// units, so rounding could hide a real error from the safety pass.
    #[error(
        "post-processor `{post}` writes {places} decimal places in {units}; at least {min} are needed"
    )]
    TooFewDecimalPlaces {
        /// The post-processor's id.
        post: String,
        /// `mm` or `inch`.
        units: &'static str,
        /// Decimal places the definition asks for.
        places: u8,
        /// The minimum DreamCarve accepts.
        min: u8,
    },
    /// A value to be written is not a finite number.
    #[error("cannot write {value} as a G-code number")]
    NonFiniteNumber {
        /// The value.
        value: f64,
    },
    /// A cutting move in an operation's toolpath carries no usable feed.
    #[error("operation `{op}` move {index} cuts without a feed rate")]
    MoveWithoutFeed {
        /// The operation.
        op: OperationId,
        /// Index of the move in the toolpath.
        index: usize,
    },
    /// An operation's toolpath starts with an arc, so the arc has no start.
    #[error("operation `{op}` move {index} is an arc with no known start point")]
    ArcWithoutStart {
        /// The operation.
        op: OperationId,
        /// Index of the move in the toolpath.
        index: usize,
    },
    /// A line of the emitted program could not be read back as G-code.
    #[error("emitted line {line} is not valid G-code: {reason}")]
    Unparseable {
        /// 1-based line number in the program.
        line: usize,
        /// What is wrong with it.
        reason: String,
    },
    /// A line of the emitted program contains characters the controller
    /// acts on immediately (`!`, `~`, `?`, control and non-ASCII bytes), or
    /// is longer than the controller's line buffer.
    #[error("emitted line {line} is unsafe to send: {reason}")]
    UnsafeLine {
        /// 1-based line number in the program.
        line: usize,
        /// What is wrong with it.
        reason: &'static str,
    },
    /// The emitted program contains a `G` or `M` code outside the set
    /// DreamCarve emits and its safety pass models.
    #[error("emitted line {line} contains `{word}`, which DreamCarve never sends")]
    ForbiddenWord {
        /// 1-based line number in the program.
        line: usize,
        /// The word.
        word: String,
    },
    /// S5: the program does not select units and a work-coordinate system
    /// before its first motion or spindle start.
    #[error("the program does not set {what} before it moves")]
    MissingSetup {
        /// What is missing: units or work coordinates.
        what: &'static str,
    },
    /// S1: a move ends below the stock bottom minus the operation's
    /// through-cut allowance.
    #[error(
        "emitted line {line} moves to Z {z_mm:.4} mm, below the lowest permitted {floor_mm:.4} mm (stock bottom minus through-cut allowance)"
    )]
    BelowMaterialBottom {
        /// The operation the line belongs to, if any.
        op: Option<OperationId>,
        /// 1-based line number in the program.
        line: usize,
        /// The move's Z above stock bottom, mm.
        z_mm: f64,
        /// The lowest permitted Z above stock bottom, mm.
        floor_mm: f64,
    },
    /// S2: a rapid ends below safe Z, or travels in XY from below safe Z
    /// or from an unknown height.
    #[error("emitted line {line} is a rapid below safe Z ({safe_z_mm:.4} mm above stock bottom)")]
    RapidBelowSafeZ {
        /// 1-based line number in the program.
        line: usize,
        /// The offending height above stock bottom, mm; `None` when the
        /// height is not yet known.
        z_mm: Option<f64>,
        /// Safe Z above stock bottom, mm.
        safe_z_mm: f64,
    },
    /// S3: a cutting move has no active feed rate.
    #[error("emitted line {line} cuts with no feed rate set")]
    CutWithoutFeed {
        /// 1-based line number in the program.
        line: usize,
    },
    /// S3: a cutting move's feed is neither the tool's feed nor its plunge
    /// rate.
    #[error("emitted line {line} cuts at {feed_mm_min:.3} mm/min, which is not a rate of its tool")]
    FeedNotFromTool {
        /// 1-based line number in the program.
        line: usize,
        /// The active feed, mm/min.
        feed_mm_min: f64,
    },
    /// S4: a cutting move happens while the spindle is off or without a
    /// speed.
    #[error("emitted line {line} cuts while the spindle is not running")]
    SpindleNotRunning {
        /// 1-based line number in the program.
        line: usize,
    },
    /// S4: a cutting move follows a spindle start with no spin-up dwell.
    #[error("emitted line {line} cuts before the spindle has had its spin-up dwell")]
    SpindleNotUpToSpeed {
        /// 1-based line number in the program.
        line: usize,
    },
    /// S4: the program ends without stopping the spindle after its last move.
    #[error("the program does not stop the spindle after its last move")]
    SpindleNotStopped,
    /// S4 (manual spindle control): a tool's first cut is not preceded by
    /// an `M0`, at safe Z and after the previous cut, for the user to turn
    /// the router on.
    #[error("emitted line {line} cuts before a pause, at safe Z, to turn the router on")]
    RouterNotStarted {
        /// 1-based line number of the cut.
        line: usize,
    },
    /// S4 (manual spindle control): the program has no `M0` at safe Z after
    /// its last cut for the user to turn the router off.
    #[error("the program does not pause, at safe Z, to turn the router off after its last cut")]
    RouterNotStopped,
    /// A G2/G3 line whose start and end radii differ by more than GRBL and
    /// grblHAL accept (0.005 mm and 0.1 % of the radius).
    #[error(
        "emitted line {line} is an arc the controller would reject (radius error {delta_mm:.4} mm)"
    )]
    ArcRejected {
        /// 1-based line number in the program.
        line: usize,
        /// Difference between start and end radius, mm.
        delta_mm: f64,
    },
    /// S6: reading the program back does not reproduce the toolpath: a
    /// toolpath point is missing or out of order in machine space.
    #[error("the program does not reproduce operation `{op}` move {index}")]
    MotionMismatch {
        /// The operation.
        op: OperationId,
        /// Index of the first move not found.
        index: usize,
    },
}
