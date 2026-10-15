// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/types.ts

//! Post-processor definition: how a controller dialect spells G-code.
//!
//! Mirrors PureCutCNC's `MachineDefinitionSchema` field for field, with keys
//! in `snake_case`. A project embeds its own copy so an app upgrade never
//! changes what a saved project exports. The bundled definitions live in
//! `dc-gcode`; the type lives here because `dc-project` cannot depend on
//! `dc-gcode` (crate direction `dc-geometry → dc-project → dc-gcode`).
//!
//! Upstream fills five fields with zod defaults when absent (`builtin`,
//! `program.operation_header`, `motion.arc_interpolation`,
//! `canned_cycles.chip_break_drill_command`, `canned_cycles.cancel_command`).
//! Here every field is required, so a saved file shows every setting.

use serde::{Deserialize, Serialize};

/// A post-processor definition. Text fields are G-code words or templates
/// whose `{placeholder}` tokens are filled at export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostProcessor {
    /// Stable identifier, for example `grblhal`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Controller vendor, if any.
    pub vendor: Option<String>,
    /// True for definitions shipped with DreamCarve.
    pub builtin: bool,
    /// Output file extension without the dot, for example `nc`.
    pub file_extension: String,
    /// Mapping from project axes to machine axes.
    pub coordinate_system: CoordinateSystem,
    /// How numbers are printed.
    pub number_format: NumberFormat,
    /// Unit-selection commands.
    pub units: UnitCommands,
    /// Program header, footer, and comment syntax.
    pub program: ProgramFormat,
    /// Work-coordinate-system selection.
    pub work_coordinates: WorkCoordinates,
    /// Motion words.
    pub motion: Motion,
    /// Feed and spindle words.
    pub feed_speed: FeedSpeed,
    /// Tool-change block.
    pub tool_change: ToolChange,
    /// Drilling canned cycles, or `None` if the controller has none.
    /// Parsed but unused until phase 2.
    pub canned_cycles: Option<CannedCycles>,
    /// Coolant commands, or `None` if the controller has none.
    /// Parsed but unused until phase 2.
    pub coolant: Option<Coolant>,
    /// Program-end command.
    pub stop: Stop,
}

/// A machine axis, optionally inverted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Axis {
    /// Machine X.
    #[serde(rename = "X")]
    X,
    /// Machine Y.
    #[serde(rename = "Y")]
    Y,
    /// Machine Z.
    #[serde(rename = "Z")]
    Z,
    /// Machine X, inverted.
    #[serde(rename = "-X")]
    NegX,
    /// Machine Y, inverted.
    #[serde(rename = "-Y")]
    NegY,
    /// Machine Z, inverted.
    #[serde(rename = "-Z")]
    NegZ,
}

/// Which machine axis each project axis drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CoordinateSystem {
    /// Machine axis for project X.
    pub x_axis: Axis,
    /// Machine axis for project Y.
    pub y_axis: Axis,
    /// Machine axis for project Z.
    pub z_axis: Axis,
}

/// Number formatting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NumberFormat {
    /// Decimal places per output unit.
    pub decimal_places: DecimalPlaces,
    /// Keep trailing zeros (`1.500` rather than `1.5`).
    pub trailing_zeros: bool,
    /// Keep the leading zero (`0.5` rather than `.5`).
    pub leading_zero: bool,
}

/// Decimal places for millimetre and inch output. Reads either one number
/// for both units or an object; always writes the object, as upstream
/// normalises it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "DecimalPlacesRepr")]
pub struct DecimalPlaces {
    /// Decimal places when output is in millimetres.
    pub mm: u8,
    /// Decimal places when output is in inches.
    pub inch: u8,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DecimalPlacesRepr {
    Both(u8),
    PerUnit(DecimalPlacesPerUnit),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecimalPlacesPerUnit {
    mm: u8,
    inch: u8,
}

impl From<DecimalPlacesRepr> for DecimalPlaces {
    fn from(repr: DecimalPlacesRepr) -> Self {
        match repr {
            DecimalPlacesRepr::Both(n) => Self { mm: n, inch: n },
            DecimalPlacesRepr::PerUnit(DecimalPlacesPerUnit { mm, inch }) => Self { mm, inch },
        }
    }
}

/// Unit-selection commands; `None` emits nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitCommands {
    /// Command selecting millimetres, for example `G21`.
    pub mm_command: Option<String>,
    /// Command selecting inches, for example `G20`.
    pub inch_command: Option<String>,
}

/// Program framing and comment syntax.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProgramFormat {
    /// Header template lines.
    pub header: Vec<String>,
    /// Template lines emitted before each operation.
    pub operation_header: Vec<String>,
    /// Footer lines, before the program-end command.
    pub footer: Vec<String>,
    /// Text that opens a comment, for example `(` or `;`.
    pub comment_prefix: String,
    /// Text that closes a comment; empty for `;` comments.
    pub comment_suffix: String,
    /// Emit `N` line numbers.
    pub line_numbers: bool,
    /// Step between line numbers.
    pub line_number_increment: u32,
}

/// Work-coordinate-system selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkCoordinates {
    /// Command selecting the WCS, for example `G54`; `None` emits nothing.
    pub select_command: Option<String>,
}

/// How arc centres are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArcFormat {
    /// Centre offsets `I` and `J`.
    Ij,
    /// Radius `R`.
    R,
}

/// Motion words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Motion {
    /// Rapid move, for example `G0`.
    pub rapid_command: String,
    /// Linear feed move, for example `G1`.
    pub linear_command: String,
    /// Clockwise arc, for example `G2`.
    pub cw_arc_command: String,
    /// Counter-clockwise arc, for example `G3`.
    pub ccw_arc_command: String,
    /// How arc centres are written.
    pub arc_format: ArcFormat,
    /// Omit a motion word that repeats the active one.
    pub modal_motion: bool,
    /// Emit arcs as arc moves; otherwise flatten them.
    pub arc_interpolation: bool,
}

/// Feed and spindle words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FeedSpeed {
    /// Feed word, for example `F`.
    pub feed_command: String,
    /// Spindle-speed word, for example `S`.
    pub rpm_command: String,
    /// Spindle on clockwise, for example `M3`.
    pub spindle_on_cw: String,
    /// Spindle on counter-clockwise, for example `M4`.
    pub spindle_on_ccw: String,
    /// Spindle off, for example `M5`.
    pub spindle_off: String,
    /// Put the feed word on the motion line.
    pub inline_with_motion: bool,
    /// Emit feed and speed only when they change.
    pub modal_feed_speed: bool,
}

/// Tool-change block.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolChange {
    /// Template lines of the tool change.
    pub commands: Vec<String>,
    /// Stop the spindle before the change.
    pub stop_spindle_first: bool,
    /// Pause after the change.
    pub pause_after_change: bool,
    /// Pause command, for example `M0`.
    pub pause_command: String,
}

/// Canned-cycle retract mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RetractMode {
    /// Retract to the initial Z.
    #[serde(rename = "G98")]
    G98,
    /// Retract to the R plane.
    #[serde(rename = "G99")]
    G99,
}

/// Drilling canned cycles. Parsed but unused until phase 2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CannedCycles {
    /// Plain drill cycle, for example `G81`.
    pub drill_command: Option<String>,
    /// Drill with dwell, for example `G82`.
    pub drill_with_dwell_command: Option<String>,
    /// Peck drill, for example `G83`.
    pub peck_drill_command: Option<String>,
    /// Chip-break drill, for example `G73`.
    pub chip_break_drill_command: Option<String>,
    /// Word carrying the peck step, for example `Q`.
    pub peck_step_word: String,
    /// Retract mode, or `None` to emit none.
    pub retract_mode: Option<RetractMode>,
    /// Cycle cancel, for example `G80`.
    pub cancel_command: String,
}

/// Coolant commands. Parsed but unused until phase 2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coolant {
    /// Flood on, for example `M8`.
    pub flood_on_command: String,
    /// Mist on, for example `M7`.
    pub mist_on_command: String,
    /// Coolant off, for example `M9`.
    pub coolant_off_command: String,
}

/// Program end.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Stop {
    /// Program-end command, for example `M30`.
    pub program_end_command: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PostProcessor {
        PostProcessor {
            id: "sample".into(),
            name: "Sample".into(),
            description: "Every optional part filled".into(),
            vendor: None,
            builtin: false,
            file_extension: "nc".into(),
            coordinate_system: CoordinateSystem {
                x_axis: Axis::X,
                y_axis: Axis::NegY,
                z_axis: Axis::Z,
            },
            number_format: NumberFormat {
                decimal_places: DecimalPlaces { mm: 3, inch: 4 },
                trailing_zeros: false,
                leading_zero: true,
            },
            units: UnitCommands {
                mm_command: Some("G21".into()),
                inch_command: None,
            },
            program: ProgramFormat {
                header: vec!["( {programName} )".into(), "G90".into()],
                operation_header: vec![],
                footer: vec!["M5".into()],
                comment_prefix: "(".into(),
                comment_suffix: ")".into(),
                line_numbers: true,
                line_number_increment: 10,
            },
            work_coordinates: WorkCoordinates {
                select_command: Some("G54".into()),
            },
            motion: Motion {
                rapid_command: "G0".into(),
                linear_command: "G1".into(),
                cw_arc_command: "G2".into(),
                ccw_arc_command: "G3".into(),
                arc_format: ArcFormat::R,
                modal_motion: true,
                arc_interpolation: false,
            },
            feed_speed: FeedSpeed {
                feed_command: "F".into(),
                rpm_command: "S".into(),
                spindle_on_cw: "M3".into(),
                spindle_on_ccw: "M4".into(),
                spindle_off: "M5".into(),
                inline_with_motion: true,
                modal_feed_speed: true,
            },
            tool_change: ToolChange {
                commands: vec!["M6 T{toolNumber}".into()],
                stop_spindle_first: true,
                pause_after_change: false,
                pause_command: "M0".into(),
            },
            canned_cycles: Some(CannedCycles {
                drill_command: Some("G81".into()),
                drill_with_dwell_command: None,
                peck_drill_command: Some("G83".into()),
                chip_break_drill_command: None,
                peck_step_word: "Q".into(),
                retract_mode: Some(RetractMode::G98),
                cancel_command: "G80".into(),
            }),
            coolant: Some(Coolant {
                flood_on_command: "M8".into(),
                mist_on_command: "M7".into(),
                coolant_off_command: "M9".into(),
            }),
            stop: Stop {
                program_end_command: "M30".into(),
            },
        }
    }

    #[test]
    fn post_processor_round_trips_through_json() {
        let post = sample();
        let text = serde_json::to_string_pretty(&post).unwrap();
        let back: PostProcessor = serde_json::from_str(&text).unwrap();
        assert_eq!(back, post);
    }

    #[test]
    fn keys_are_snake_case_and_none_serializes_as_null() {
        let value = serde_json::to_value(sample()).unwrap();
        assert_eq!(value["file_extension"], "nc");
        assert_eq!(value["feed_speed"]["spindle_on_cw"], "M3");
        assert_eq!(value["coordinate_system"]["y_axis"], "-Y");
        assert_eq!(value["motion"]["arc_format"], "r");
        assert!(value["vendor"].is_null());
        assert!(value["units"]["inch_command"].is_null());
    }

    #[test]
    fn unknown_field_is_rejected() {
        let mut value = serde_json::to_value(sample()).unwrap();
        value["motion"]["arcInterpolation"] = serde_json::Value::Bool(true);
        let err = serde_json::from_value::<PostProcessor>(value).unwrap_err();
        assert!(err.to_string().contains("arcInterpolation"), "{err}");
    }

    #[test]
    fn missing_field_is_rejected() {
        let mut value = serde_json::to_value(sample()).unwrap();
        value.as_object_mut().unwrap().remove("builtin");
        assert!(serde_json::from_value::<PostProcessor>(value).is_err());
    }

    #[test]
    fn decimal_places_accepts_one_number_for_both_units() {
        let places: DecimalPlaces = serde_json::from_str("3").unwrap();
        assert_eq!(places, DecimalPlaces { mm: 3, inch: 3 });
        let text = serde_json::to_string(&places).unwrap();
        assert_eq!(text, r#"{"mm":3,"inch":3}"#);
    }
}
