// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The crate's error type. Every safety violation found in an emitted
//! program is an [`ExportError`], never a warning
//! (`docs/specs/phase-1/design.md`, "Emission", step 7).

use dc_cam::FeedField;
use dc_project::{OperationId, PostProcessorError, ToolId};

/// Why G-code could not be exported. When `emit` returns one of these, no
/// program is produced.
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    /// The project's post-processor names no bundled definition version.
    /// Loading a project already refuses that, so only a project built in
    /// code gets here.
    #[error(transparent)]
    PostProcessor(#[from] PostProcessorError),
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
    /// Two tools of one program resolve to the same `T` number: a set
    /// `number`, or the 1-based place in the tool list of a tool with none
    /// (#120). The program would fit the wrong bit. Validation refuses
    /// such a project first.
    #[error(
        "tools `{other}` and `{tool}` would both be T{number} in the program; give one of them another number"
    )]
    DuplicateToolNumber {
        /// The later tool.
        tool: ToolId,
        /// The earlier tool with that number.
        other: ToolId,
        /// The number both resolve to.
        number: u32,
    },
    /// A tool's `number` is 0: `T0` is no tool. Validation refuses it
    /// first.
    #[error("tool `{tool}` has number 0; a tool number is 1 or more")]
    ToolNumberZero {
        /// The tool.
        tool: ToolId,
    },
    /// A tool lacks a feed or plunge rate: unset, or not a positive number.
    /// Rates are never invented.
    #[error("tool `{tool}` has no usable {field}")]
    MissingFeed {
        /// The tool's name.
        tool: String,
        /// The tool's id: names need not be unique (#71).
        tool_id: ToolId,
        /// Which rate is missing.
        field: FeedField,
    },
    /// The stock thickness is unset, so neither safe Z nor, with Z zero at
    /// the stock top, the work origin is known. It is never defaulted
    /// (#64).
    #[error("the stock has no thickness_mm; export fails until it is set")]
    MissingThickness,
    /// A tool lacks a spindle speed, or it is 0. Speeds are never invented.
    #[error("tool `{tool}` has no spindle speed (rpm)")]
    MissingRpm {
        /// The tool's name.
        tool: String,
        /// The tool's id: names need not be unique (#71).
        tool_id: ToolId,
    },
    /// One program was asked for operations that use more than one tool,
    /// but the post-processor's tool-change mode is not `"m6"`, or its tool
    /// change has no `M6`, so the program cannot stop for the user to change
    /// the bit (#57, #95). GRBL 1.1 has no `M6`, and its `M0` pause is a
    /// feed hold, in which it refuses to jog. `emit_programs` writes one
    /// program per tool run instead.
    #[error(
        "post-processor `{post}` does not change tools within one program (that needs tool_change.mode \"m6\" and an M6 command), but the enabled operations use {}; export one program per tool run instead",
        .tools.iter().map(|t| format!("`{t}`")).collect::<Vec<_>>().join(", ")
    )]
    ToolChangeNotSupported {
        /// The post-processor's id.
        post: String,
        /// The tools' names, in the order the program would use them.
        tools: Vec<String>,
    },
    /// One program with tool changes was asked for (`export --one-program`,
    /// `ProgramSplit::OneProgram`), but the post-processor's tool change has
    /// no `M6` line, so no program of its own can stop for the user to
    /// change the bit (#57). GRBL 1.1 has no `M6`. Checked before anything
    /// is emitted, whatever the operations' tools.
    #[error(
        "post-processor `{post}` has no M6 tool change, so it cannot write one program with tool changes; export one program per tool run instead"
    )]
    OneProgramNeedsM6 {
        /// The post-processor's id.
        post: String,
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
    /// #312: a setup code the definition's header writes (`G50`, `G69`)
    /// appears where `emit` never writes it: with other words on its line,
    /// or outside the header and the setup after a stop.
    #[error("emitted line {line} has `{word}` where DreamCarve never sends it: {reason}")]
    MisplacedSetupWord {
        /// 1-based line number in the program.
        line: usize,
        /// The word.
        word: String,
        /// Why it is refused.
        reason: &'static str,
    },
    /// S5: the program does not select units, a work-coordinate system, and
    /// absolute distance mode before its first motion or spindle start.
    #[error("the program does not set {what} before it moves")]
    MissingSetup {
        /// What is missing: units, work coordinates, or absolute distance
        /// mode.
        what: &'static str,
    },
    /// S5: after a pause the program moves, or starts the spindle, before
    /// it sets units, work coordinates, and absolute distance mode again.
    /// The user may change any of them at the controller during a pause.
    #[error(
        "emitted line {line} moves before the program sets {what} again after the pause on line {pause_line}"
    )]
    MissingSetupAfterPause {
        /// 1-based line number of the move or spindle start.
        line: usize,
        /// 1-based line number of the pause.
        pause_line: usize,
        /// What is missing.
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
    /// S7: a cut, widened by its tool's radius, leaves the stock
    /// rectangle (#62).
    #[error("emitted line {line} cuts {overhang_mm:.3} mm past the stock's {side} edge")]
    OffStock {
        /// The operation the line belongs to, if any.
        op: Option<OperationId>,
        /// 1-based line number in the program.
        line: usize,
        /// The edge, in project axes: `-X`, `+X`, `-Y`, or `+Y`.
        side: &'static str,
        /// How far past it the cutter's edge reaches, mm.
        overhang_mm: f64,
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
    /// S15: inside a Drill, a rapid in Z alone descends below the stock
    /// top deeper than any cut has reached at that XY, into uncut material.
    #[error(
        "emitted line {line} rapids down to {z_mm:.4} mm above stock bottom, into material no cut has opened at that point"
    )]
    RapidIntoUncut {
        /// 1-based line number in the program.
        line: usize,
        /// Where the rapid ends, above stock bottom, mm.
        z_mm: f64,
        /// The deepest a cut has reached at that XY, mm above stock bottom;
        /// `None` when no cut has been made there.
        cut_mm: Option<f64>,
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
    /// S4: an `M0` where Z is unknown or below safe Z. The user fits tools
    /// and switches the router at a pause, so the bit must be clear of the
    /// work.
    #[error("emitted line {line} pauses below safe Z, or where Z is not known")]
    PauseNotAtSafeZ {
        /// 1-based line number of the pause.
        line: usize,
    },
    /// S4: the spindle starts with no retract to safe Z since the last
    /// pause. The user may have fitted a tool and set Z zero at the pause,
    /// so Z is not known until the program moves to it again.
    #[error(
        "emitted line {line} starts the spindle without a retract to safe Z after the last pause"
    )]
    SpindleStartNotAtSafeZ {
        /// 1-based line number of the spindle start.
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
    /// A G2/G3 line whose end is its start within the written precision.
    /// The controller cuts a full circle, which DreamCarve never emits on
    /// purpose, and whose reach S7 and the extents would have to guess.
    #[error(
        "emitted line {line} is an arc that ends where it starts, which the controller cuts as a full circle"
    )]
    FullCircleArc {
        /// 1-based line number in the program.
        line: usize,
    },
    /// S12: a `G2`/`G3` written in place of straight toolpath moves does
    /// not follow them: its start or end is not a toolpath vertex, a vertex
    /// between lies off the arc by more than the arc-fit tolerance, it
    /// sweeps more than 90°, or it turns against the vertices.
    #[error(
        "emitted line {line} is an arc in place of operation `{op}`'s straight moves that does not follow them"
    )]
    ArcFitViolation {
        /// The operation.
        op: OperationId,
        /// 1-based line number in the program.
        line: usize,
    },
    /// S10: a cutting move descends faster than its tool's plunge rate:
    /// its feed times the sine of its slope is above the plunge rate. A
    /// ramp, a helix, a tab's down slope and a straight plunge are all
    /// held to it.
    #[error("emitted line {line} of operation `{op}` descends faster than its tool's plunge rate")]
    EntryTooSteep {
        /// The operation.
        op: OperationId,
        /// 1-based line number in the program.
        line: usize,
    },
    /// S9: a cutting move takes the tool out of the material its operation
    /// may cut: past a Pocket's wall, its stock to leave, or into an island; off
    /// a Drill's holes; or off an Engrave's lines.
    #[error("emitted line {line} takes operation `{op}`'s tool outside the material it may cut")]
    OffMaterial {
        /// The operation.
        op: OperationId,
        /// 1-based line number in the program.
        line: usize,
    },
    /// S17 (`docs/specs/phase-3/design.md`, "Safety pass"): a V-bit's cone
    /// crosses a wall of its carved region below the start plane, its tip
    /// leaves the region or is below the flat depth, or a cut above the
    /// start plane is not a vertical descent over the carve. The point is
    /// where the overlap is worst, project space, mm.
    #[error(
        "emitted line {line} takes operation `{op}`'s V-bit {overlap_mm:.3} mm past the material it may cut, at X {x_mm:.3} Y {y_mm:.3} Z {z_mm:.3} (project space)"
    )]
    ConeOffMaterial {
        /// The operation.
        op: OperationId,
        /// 1-based line number in the program.
        line: usize,
        /// X of the worst point, project space, mm.
        x_mm: f64,
        /// Y of the worst point, project space, mm.
        y_mm: f64,
        /// Z of the worst point, project space, mm.
        z_mm: f64,
        /// How far past the material the cone, the tip or the cut reaches
        /// there, mm.
        overlap_mm: f64,
    },
    /// S17, the shank (security review H4): the V-bit's flank is engaged
    /// deeper than its cone is tall, measured from the stock top (or from
    /// a start plane an enabled earlier Pocket has cleared), so the shank
    /// would cut. A tip below the cone height is this too.
    #[error(
        "emitted line {line} engages operation `{op}`'s V-bit {engaged_mm:.3} mm deep, past its {cone_mm:.3} mm cone, so its shank would cut"
    )]
    ConeHeightExceeded {
        /// The operation.
        op: OperationId,
        /// 1-based line number in the program.
        line: usize,
        /// The engagement, mm: from the stock top (or the cleared start
        /// plane) down to the tip.
        engaged_mm: f64,
        /// The cone's height, mm: `diameter / (2 · tan(α/2))`.
        cone_mm: f64,
    },
    /// S6: reading the program back does not reproduce the toolpath: a
    /// move is missing, ends elsewhere, is the wrong kind, turns the wrong
    /// way, has the wrong centre, or has moves the toolpath does not.
    #[error("the program does not reproduce operation `{op}` move {index}")]
    MotionMismatch {
        /// The operation.
        op: OperationId,
        /// Index of the first toolpath move not reproduced; the number of
        /// moves if extra motion follows the last.
        index: usize,
    },
    /// #283: the project's clearance height is below an operation's safe
    /// Z, so a link between parts would run lower than one between passes.
    /// Validation refuses such a project first.
    #[error(
        "the clearance height {clearance_mm} mm is below operation `{op}`'s safe Z of {safe_z_mm} mm"
    )]
    ClearanceBelowSafeZ {
        /// The operation.
        op: OperationId,
        /// `heights.clearance_mm`, mm above the stock top.
        clearance_mm: f64,
        /// The operation's `safe_z_mm`, mm above the stock top.
        safe_z_mm: f64,
    },
    /// #283: the tool-change machine Z is 0 or above, or not a number.
    /// grblHAL and GRBL, and LinuxCNC as its configuration tools set it up
    /// (#306), home Z to machine Z 0 or a negative position, so the lift
    /// would go past the limit switch. Validation refuses it first.
    #[error("the tool-change machine Z {machine_z_mm} mm is not below 0, where Z homes")]
    MachineZNotBelowHome {
        /// `heights.tool_change_machine_z_mm`.
        machine_z_mm: f64,
    },
    /// #283: the project sets a tool-change machine Z, but the definition
    /// documents no machine-coordinate move. Validation refuses it first.
    #[error(
        "post-processor `{post}` documents no machine-coordinate move (G53), so it cannot lift to the tool-change machine Z"
    )]
    MachineZNeedsG53 {
        /// The post-processor's id.
        post: String,
    },
    /// #283: the park point is not a finite number within the coordinate
    /// limit. Validation refuses it first.
    #[error("the park point ({x_mm}, {y_mm}) mm is not a point the program can move to")]
    InvalidPark {
        /// X, mm.
        x_mm: f64,
        /// Y, mm.
        y_mm: f64,
    },
    /// S2 as changed for #283: with a clearance height, an operation's
    /// first move across the work, or one outside every operation such as
    /// the move to the park point, runs below it.
    #[error(
        "emitted line {line} moves across the work below the clearance height ({clearance_z_mm} mm above the stock bottom)"
    )]
    RapidBelowClearance {
        /// 1-based line number in the program.
        line: usize,
        /// The height above stock bottom, mm; `None` when not known.
        z_mm: Option<f64>,
        /// The clearance height above stock bottom, mm.
        clearance_z_mm: f64,
    },
    /// S16: a machine-coordinate lift (`G53 G0 Z`) comes with the tool not
    /// known to be at or above safe Z in work coordinates.
    #[error(
        "emitted line {line} lifts in machine coordinates (G53) without a retract to safe Z first"
    )]
    MachineLiftNotAtSafeZ {
        /// 1-based line number of the lift.
        line: usize,
    },
    /// S16: a machine-coordinate lift is to another Z than the project's
    /// `tool_change_machine_z_mm`, or to one not below 0.
    #[error(
        "emitted line {line} lifts to machine Z {z_mm} mm; the project's tool-change machine Z is {expected_mm} mm"
    )]
    MachineLiftWrongZ {
        /// 1-based line number of the lift.
        line: usize,
        /// The machine Z written, mm.
        z_mm: f64,
        /// `heights.tool_change_machine_z_mm`.
        expected_mm: f64,
    },
    /// S16: the program moves after a machine-coordinate lift before a
    /// tool-change stop or the end; a lift comes only right before one.
    #[error(
        "emitted line {line} moves after the machine-coordinate lift on line {lift_line} without a stop between"
    )]
    MachineLiftNotBeforeStop {
        /// 1-based line number of the move.
        line: usize,
        /// 1-based line number of the lift.
        lift_line: usize,
    },
}
