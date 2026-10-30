// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/types.ts

//! Post-processors: how a controller dialect spells G-code, which bundled
//! definitions DreamCarve ships, and what a project stores (#106,
//! ADR-0004).
//!
//! [`PostProcessor`] mirrors PureCutCNC's `MachineDefinitionSchema` field
//! for field, with keys in `snake_case`. It is the effective definition
//! that `dc-gcode` emits with. A project does not hold one: it holds a
//! [`PostProcessorRef`], the id and version of a bundled definition plus the
//! few settings a user may change, and [`PostProcessorRef::resolve`] puts
//! the two together.
//!
//! The bundled definitions are versioned files under `definitions/`,
//! compiled in with `include_str!` and parsed on first use (see
//! `definitions/README.md`). A released version never changes, so a
//! project that names it exports the same bytes after any upgrade. They
//! live here, not in `dc-gcode`, because loading and validating a project
//! needs them (an unknown id, the `M6` check, the version 6 migration) and
//! `dc-project` cannot depend on `dc-gcode` (crate direction
//! `dc-geometry → dc-project → dc-gcode`).
//!
//! Upstream fills five fields with zod defaults when absent (`builtin`,
//! `program.operation_header`, `motion.arc_interpolation`,
//! `canned_cycles.chip_break_drill_command`, `canned_cycles.cancel_command`).
//! Here every field is required, so a definition file shows every value. An
//! `Option` field is required too, `null` when unset (#65): serde would
//! otherwise read a missing one as `None`.

use crate::Units;
use serde::{Deserialize, Deserializer, Serialize, de};
use serde_json::Value;
use std::fmt;
use std::sync::OnceLock;

/// Longest [`PostProcessor::file_extension`] accepted.
pub const MAX_FILE_EXTENSION_CHARS: usize = 8;

/// Whether `extension` is safe to append to a project's file name. It is a
/// setting in the `.dcv`, so a shared project chooses this value: it
/// must be 1 to [`MAX_FILE_EXTENSION_CHARS`] ASCII letters or digits (never
/// a path) and not `dcv` in any case, which would replace the project.
#[must_use]
pub fn is_safe_file_extension(extension: &str) -> bool {
    !extension.is_empty()
        && extension.len() <= MAX_FILE_EXTENSION_CHARS
        && extension.chars().all(|c| c.is_ascii_alphanumeric())
        && !extension.eq_ignore_ascii_case("dcv")
}

/// A post-processor definition, complete: a bundled definition version with
/// a project's settings in it ([`PostProcessorRef::resolve`]), which is what
/// `dc-gcode` emits with. Text fields are G-code words or templates whose
/// `{placeholder}` tokens are filled at export.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostProcessor {
    /// Stable identifier, for example `grblhal`.
    pub id: String,
    /// Display name.
    pub name: String,
    /// One-line description.
    pub description: String,
    /// Controller vendor, if any.
    #[serde(deserialize_with = "Option::deserialize")]
    pub vendor: Option<String>,
    /// True for definitions shipped with DreamCarve.
    pub builtin: bool,
    /// Output file extension without the dot, for example `nc`. See
    /// [`is_safe_file_extension`].
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
    /// Whether Drill writes canned cycles (`G81`–`G83`, `G73`) where
    /// `canned_cycles` has them, or plain moves everywhere (functional
    /// spec, capability 14). A project setting
    /// ([`PostProcessorRef::use_canned_cycles`]); off by default on every
    /// controller.
    pub use_canned_cycles: bool,
    /// Drilling canned cycles, or `None` if the controller has none.
    #[serde(deserialize_with = "Option::deserialize")]
    pub canned_cycles: Option<CannedCycles>,
    /// Coolant commands, or `None` if the controller has none.
    /// Parsed but unused until phase 2.
    #[serde(deserialize_with = "Option::deserialize")]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DecimalPlaces {
    /// Decimal places when output is in millimetres.
    pub mm: u8,
    /// Decimal places when output is in inches.
    pub inch: u8,
}

impl DecimalPlaces {
    /// One unit in the last place a program in `units` writes, mm:
    /// `10^−places` mm, or `10^−places` in (25.4 times as many mm). The
    /// safety pass compares written numbers within it, and an entry helix
    /// whose moves are shorter is not cut (#265).
    #[must_use]
    pub fn quantum_mm(self, units: Units) -> f64 {
        let (places, mm_per_unit) = match units {
            Units::Mm => (self.mm, 1.0),
            Units::Inch => (self.inch, 25.4),
        };
        10f64.powi(-i32::from(places)) * mm_per_unit
    }
}

/// The object form of [`DecimalPlaces`], read strictly.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DecimalPlacesPerUnit {
    mm: u8,
    inch: u8,
}

/// Reads either form of [`DecimalPlaces`]. A visitor rather than an
/// untagged enum, so a value of neither form is reported as what was
/// expected (#65), and an error inside the object keeps its key.
struct DecimalPlacesVisitor;

impl<'de> de::Visitor<'de> for DecimalPlacesVisitor {
    type Value = DecimalPlaces;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a whole number from 0 to 255 for both units, or an object with `mm` and `inch`",
        )
    }

    fn visit_u64<E: de::Error>(self, n: u64) -> Result<DecimalPlaces, E> {
        let n =
            u8::try_from(n).map_err(|_| E::invalid_value(de::Unexpected::Unsigned(n), &self))?;
        Ok(DecimalPlaces { mm: n, inch: n })
    }

    fn visit_i64<E: de::Error>(self, n: i64) -> Result<DecimalPlaces, E> {
        match u64::try_from(n) {
            Ok(n) => self.visit_u64(n),
            Err(_) => Err(E::invalid_value(de::Unexpected::Signed(n), &self)),
        }
    }

    fn visit_map<A: de::MapAccess<'de>>(self, map: A) -> Result<DecimalPlaces, A::Error> {
        let DecimalPlacesPerUnit { mm, inch } =
            DecimalPlacesPerUnit::deserialize(de::value::MapAccessDeserializer::new(map))?;
        Ok(DecimalPlaces { mm, inch })
    }
}

impl<'de> Deserialize<'de> for DecimalPlaces {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(DecimalPlacesVisitor)
    }
}

/// Unit-selection commands; `None` emits nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitCommands {
    /// Command selecting millimetres, for example `G21`.
    #[serde(deserialize_with = "Option::deserialize")]
    pub mm_command: Option<String>,
    /// Command selecting inches, for example `G20`.
    #[serde(deserialize_with = "Option::deserialize")]
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
    #[serde(deserialize_with = "Option::deserialize")]
    pub select_command: Option<String>,
    /// The non-modal command that makes one move in machine coordinates,
    /// `G53`, where the controller's documentation or source shows it
    /// (#283; `definitions/README.md`, "Machine-coordinate moves"). The
    /// program writes it only as `G53 G0 Z…`, for a project's
    /// `heights.tool_change_machine_z_mm`; `None` refuses that height.
    #[serde(deserialize_with = "Option::deserialize")]
    pub machine_command: Option<String>,
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
    /// Pause after every spindle start so the spindle reaches speed before
    /// it cuts. Neither GRBL nor grblHAL waits on its own.
    pub spindle_on_dwell: SpindleDwell,
    /// Whether the program switches the spindle, or pauses for the user to
    /// switch the router by hand.
    pub spindle_control: SpindleControl,
}

/// Who switches the spindle on and off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpindleControl {
    /// The program starts the spindle with `spindle_on_cw` and the tool's
    /// speed, dwells for spin-up, and stops it with `spindle_off`.
    #[default]
    Automatic,
    /// The spindle is a router the user switches by hand. The program sends
    /// no spindle words; it retracts to safe Z and pauses with `M0` before
    /// each tool's first cut and after its last, with a message saying what
    /// to do. The tool's speed, if set, appears only in the message.
    Manual,
}

/// A dwell after the spindle starts, and the dwell word a Drill's `dwell_s`
/// is written with.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpindleDwell {
    /// Template for the dwell line; `{seconds}` is replaced by
    /// `duration_s` converted to `units`. `G4 P{seconds}` on GRBL and
    /// grblHAL, where P is in seconds.
    pub command: String,
    /// How long to wait, seconds. `0` emits no dwell.
    pub duration_s: f64,
    /// The unit of the number written into `command`, and into a `G82`'s
    /// `P`: seconds on GRBL, grblHAL, LinuxCNC, Mach3 (as its manual
    /// shows it set up) and RS274/NGC, milliseconds on UCCNC
    /// (`definitions/README.md`).
    pub units: DwellUnits,
}

/// The unit a controller's dwell word takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DwellUnits {
    /// `P` is seconds.
    Seconds,
    /// `P` is milliseconds.
    Milliseconds,
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
    /// How a project whose operations use more than one tool is exported
    /// (#95). Added in `.dcv` version 5.
    pub mode: ToolChangeMode,
}

impl ToolChange {
    /// Whether a line of [`ToolChange::commands`] has an `M6` (or `M06`)
    /// code word, outside `( )` and `;` comments: whether the definition
    /// can change tools within one program. GRBL 1.1 has no `M6` (#57).
    #[must_use]
    pub fn has_m6(&self) -> bool {
        self.commands.iter().any(|line| is_m6_line(line))
    }
}

/// Whether template `line` has an `M6` code word, comments skipped.
#[must_use]
pub fn is_m6_line(line: &str) -> bool {
    let mut code = String::new();
    let mut in_comment = false;
    for c in line.chars() {
        match c {
            ';' if !in_comment => break,
            '(' => in_comment = true,
            ')' => in_comment = false,
            c if !in_comment => code.push(c),
            _ => {}
        }
    }
    code.split_whitespace()
        .any(|w| w.eq_ignore_ascii_case("M6") || w.eq_ignore_ascii_case("M06"))
}

/// How a project whose operations use more than one tool is exported (#95).
/// A machine setting, so it is a post-processor setting
/// ([`PostProcessorRef::tool_change_mode`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolChangeMode {
    /// One complete program per run of consecutive operations with the
    /// same tool, each with no tool change. The user changes the bit and
    /// sets Z zero between programs. Works on every controller and sender.
    #[default]
    SeparatePrograms,
    /// One program that stops with `T<n> M6` at each tool change. Needs a
    /// definition whose tool change has `M6` (grblHAL) and a sender that
    /// stops there.
    M6,
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

/// Drilling canned cycles (design spec, "Canned cycles"). Written only
/// when the project's [`PostProcessor::use_canned_cycles`] is on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CannedCycles {
    /// Plain drill cycle, for example `G81`.
    #[serde(deserialize_with = "Option::deserialize")]
    pub drill_command: Option<String>,
    /// Drill with dwell, for example `G82`.
    #[serde(deserialize_with = "Option::deserialize")]
    pub drill_with_dwell_command: Option<String>,
    /// Peck drill, for example `G83`.
    #[serde(deserialize_with = "Option::deserialize")]
    pub peck_drill_command: Option<String>,
    /// Chip-break drill, for example `G73`.
    #[serde(deserialize_with = "Option::deserialize")]
    pub chip_break_drill_command: Option<String>,
    /// Word carrying the peck step, for example `Q`.
    pub peck_step_word: String,
    /// Retract mode, or `None` to emit none.
    #[serde(deserialize_with = "Option::deserialize")]
    pub retract_mode: Option<RetractMode>,
    /// Cycle cancel, for example `G80`.
    pub cancel_command: String,
    /// How far above the previous depth the controller re-enters after a
    /// full peck retract (`G83`), and how far it lifts between chip-break
    /// pecks (`G73`), millimetres, as its documentation states; `None`
    /// where it states none, and then neither cycle is written. The safety
    /// pass expands each cycle with this distance and compares the
    /// expansion with the Drill toolpath to it as the tolerance.
    #[serde(deserialize_with = "Option::deserialize")]
    pub reentry_gap_mm: Option<f64>,
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

/// What a project stores about its post-processor (#106, ADR-0004): which
/// bundled definition version it uses, and the user's settings. Everything
/// else in the effective [`PostProcessor`] comes from that definition
/// version, which is compiled in and never changes once released, so a
/// saved project exports the same bytes after any upgrade.
///
/// [`resolve`](Self::resolve) puts the two together. Every key is
/// required, and they are written in this order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PostProcessorRef {
    /// The bundled definition's id, for example `grblhal`.
    pub id: String,
    /// The bundled definition's version, `<n>` in
    /// `definitions/<id>/v<n>.json`.
    pub definition_version: u32,
    /// Output file extension without the dot, for example `nc`. See
    /// [`is_safe_file_extension`].
    pub file_extension: String,
    /// Whether the program switches the spindle, or pauses for the user to
    /// switch the router by hand.
    pub spindle_control: SpindleControl,
    /// Seconds to wait after every spindle start, so the spindle reaches
    /// speed before it cuts. `0` emits no dwell.
    pub spindle_on_dwell_s: f64,
    /// How a project whose operations use more than one tool is exported
    /// (#95).
    pub tool_change_mode: ToolChangeMode,
    /// Decimal places per output unit.
    pub decimal_places: DecimalPlaces,
    /// Whether Drill writes canned cycles where the definition has them
    /// (functional spec, capability 14). Off by default; an error when on
    /// for a definition without them. Added in `.dcv` version 8.
    pub use_canned_cycles: bool,
}

impl PostProcessorRef {
    /// A reference to the latest bundled version of `id` with that
    /// version's default settings: what `init` and `export --post` write.
    ///
    /// # Errors
    ///
    /// As for [`latest_definition`].
    pub fn latest(id: &str) -> Result<Self, PostProcessorError> {
        Ok(latest_definition(id)?.default_ref())
    }

    /// The bundled definition version this reference names.
    ///
    /// # Errors
    ///
    /// As for [`bundled_definition`].
    pub fn definition(&self) -> Result<&'static BundledDefinition, PostProcessorError> {
        bundled_definition(&self.id, self.definition_version)
    }

    /// The effective post-processor: the definition, with this reference's
    /// settings in place of the definition's defaults.
    ///
    /// # Errors
    ///
    /// As for [`bundled_definition`].
    pub fn resolve(&self) -> Result<PostProcessor, PostProcessorError> {
        let mut post = self.definition()?.post.clone();
        post.file_extension.clone_from(&self.file_extension);
        post.feed_speed.spindle_control = self.spindle_control;
        post.feed_speed.spindle_on_dwell.duration_s = self.spindle_on_dwell_s;
        post.tool_change.mode = self.tool_change_mode;
        post.number_format.decimal_places = self.decimal_places;
        post.use_canned_cycles = self.use_canned_cycles;
        Ok(post)
    }
}

/// Where each setting of a [`PostProcessorRef`] sits in a full
/// [`PostProcessor`]: its key in the reference and in a definition's
/// `default_settings`, and its path in the full definition. Up to `.dcv`
/// version 6 a project held its whole definition, with the first
/// [`V6_SETTING_COUNT`] settings at these paths; `use_canned_cycles`
/// came with version 8 (#117).
pub(crate) const SETTING_PATHS: [(&str, &[&str]); 6] = [
    ("file_extension", &["file_extension"]),
    ("spindle_control", &["feed_speed", "spindle_control"]),
    (
        "spindle_on_dwell_s",
        &["feed_speed", "spindle_on_dwell", "duration_s"],
    ),
    ("tool_change_mode", &["tool_change", "mode"]),
    ("decimal_places", &["number_format", "decimal_places"]),
    ("use_canned_cycles", &["use_canned_cycles"]),
];

/// How many of [`SETTING_PATHS`] a version 6 file's embedded copy holds.
pub(crate) const V6_SETTING_COUNT: usize = 5;

/// Why a post-processor reference names no bundled definition.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PostProcessorError {
    /// No bundled definition has this id.
    #[error("post-processor `{id}` is not bundled; available: {}", known.join(", "))]
    UnknownId {
        /// The id asked for.
        id: String,
        /// The bundled ids, sorted.
        known: Vec<String>,
    },
    /// The definition is bundled, but not at this version.
    #[error(
        "post-processor `{id}` has no definition version {version}; available: {}",
        available.iter().map(u32::to_string).collect::<Vec<_>>().join(", ")
    )]
    UnknownVersion {
        /// The definition's id.
        id: String,
        /// The version asked for.
        version: u32,
        /// The bundled versions of `id`, oldest first.
        available: Vec<u32>,
    },
    /// A bundled definition file is invalid. Only a broken build can
    /// produce this; `bundled_definitions_parse` guards it.
    #[error("bundled post-processor definition `{path}` is invalid: {message}")]
    BundledDefinition {
        /// The file under `definitions/`.
        path: &'static str,
        /// What is wrong.
        message: String,
    },
}

/// One bundled definition file: id, version, path under `definitions/`,
/// and text.
type Source = (&'static str, u32, &'static str, &'static str);

/// Every bundled definition version, compiled in, in listing order: by
/// id (grblHAL, GRBL, LinuxCNC, Mach3, UCCNC, generic), then oldest
/// version first. A released version is
/// never edited or removed (ADR-0004, `definitions/README.md`); a change
/// that would change emitted bytes is a new version file.
/// `tests::released_definition_versions_are_unchanged` pins each one.
const SOURCES: [Source; 16] = [
    (
        "grblhal",
        1,
        "grblhal/v1.json",
        include_str!("definitions/grblhal/v1.json"),
    ),
    (
        "grblhal",
        2,
        "grblhal/v2.json",
        include_str!("definitions/grblhal/v2.json"),
    ),
    (
        "grblhal",
        3,
        "grblhal/v3.json",
        include_str!("definitions/grblhal/v3.json"),
    ),
    (
        "grblhal",
        4,
        "grblhal/v4.json",
        include_str!("definitions/grblhal/v4.json"),
    ),
    (
        "grbl",
        1,
        "grbl/v1.json",
        include_str!("definitions/grbl/v1.json"),
    ),
    (
        "grbl",
        2,
        "grbl/v2.json",
        include_str!("definitions/grbl/v2.json"),
    ),
    (
        "grbl",
        3,
        "grbl/v3.json",
        include_str!("definitions/grbl/v3.json"),
    ),
    (
        "linuxcnc",
        1,
        "linuxcnc/v1.json",
        include_str!("definitions/linuxcnc/v1.json"),
    ),
    (
        "linuxcnc",
        2,
        "linuxcnc/v2.json",
        include_str!("definitions/linuxcnc/v2.json"),
    ),
    (
        "mach3",
        1,
        "mach3/v1.json",
        include_str!("definitions/mach3/v1.json"),
    ),
    (
        "mach3",
        2,
        "mach3/v2.json",
        include_str!("definitions/mach3/v2.json"),
    ),
    (
        "mach3",
        3,
        "mach3/v3.json",
        include_str!("definitions/mach3/v3.json"),
    ),
    (
        "uccnc",
        1,
        "uccnc/v1.json",
        include_str!("definitions/uccnc/v1.json"),
    ),
    (
        "uccnc",
        2,
        "uccnc/v2.json",
        include_str!("definitions/uccnc/v2.json"),
    ),
    (
        "uccnc",
        3,
        "uccnc/v3.json",
        include_str!("definitions/uccnc/v3.json"),
    ),
    (
        "generic",
        1,
        "generic/v1.json",
        include_str!("definitions/generic/v1.json"),
    ),
];

/// [`SOURCES`] parsed, once per process. The one exception to "no global
/// state in `dc-*`" (`docs/rust-guidelines.md`): a cache of data compiled
/// into the binary, written once and read-only after. It is not state a
/// caller can change, and every call sees the same value, so results stay
/// pure functions of their inputs. It lets [`bundled_definitions`] hand
/// out `&'static` definitions without parsing six files on every project
/// load (#71).
static BUNDLED: OnceLock<Vec<BundledDefinition>> = OnceLock::new();

/// One version of a bundled definition: `definitions/<id>/v<version>.json`.
///
/// The file is a [`PostProcessor`] without the settings a project holds
/// (see [`PostProcessorRef`]), plus a `default_settings` object with them,
/// which a new project starts from.
#[derive(Debug)]
pub struct BundledDefinition {
    /// The definition's id, for example `grblhal`.
    pub id: &'static str,
    /// Its version.
    pub version: u32,
    /// Its file under `definitions/`, for example `grblhal/v1.json`.
    pub path: &'static str,
    /// The file's text.
    pub source: &'static str,
    /// The definition with its default settings.
    post: PostProcessor,
    /// The file without `default_settings`: the part a project cannot
    /// change, as JSON.
    dialect: Value,
}

impl BundledDefinition {
    /// The effective post-processor with this version's default settings.
    #[must_use]
    pub fn post_processor(&self) -> &PostProcessor {
        &self.post
    }

    /// A reference to this version with its default settings.
    #[must_use]
    pub fn default_ref(&self) -> PostProcessorRef {
        let post = &self.post;
        PostProcessorRef {
            id: self.id.to_owned(),
            definition_version: self.version,
            file_extension: post.file_extension.clone(),
            spindle_control: post.feed_speed.spindle_control,
            spindle_on_dwell_s: post.feed_speed.spindle_on_dwell.duration_s,
            tool_change_mode: post.tool_change.mode,
            decimal_places: post.number_format.decimal_places,
            use_canned_cycles: post.use_canned_cycles,
        }
    }

    /// The file without its `default_settings`, as JSON: every field of the
    /// definition that a project cannot change.
    pub(crate) fn dialect(&self) -> &Value {
        &self.dialect
    }

    /// Parses one bundled file. Its default settings go where
    /// [`SETTING_PATHS`] says, and the result must be a valid
    /// [`PostProcessor`]: a file that also sets one of those fields itself,
    /// or has any other extra field, is invalid.
    fn parse((id, version, path, text): Source) -> Result<Self, PostProcessorError> {
        let (post, dialect) = parse_definition(text)
            .map_err(|message| PostProcessorError::BundledDefinition { path, message })?;
        if post.id != id {
            return Err(PostProcessorError::BundledDefinition {
                path,
                message: format!("its id is `{}`, not `{id}`", post.id),
            });
        }
        Ok(Self {
            id,
            version,
            path,
            source: text,
            post,
            dialect,
        })
    }
}

/// Reads definition file `text`: the definition with its default settings,
/// and the file without them. See [`BundledDefinition::parse`].
fn parse_definition(text: &str) -> Result<(PostProcessor, Value), String> {
    let mut dialect: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let Some(Value::Object(mut defaults)) = dialect
        .as_object_mut()
        .and_then(|root| root.shift_remove("default_settings"))
    else {
        return Err("`default_settings` is missing or not an object".into());
    };
    let mut full = dialect.clone();
    for (key, at) in SETTING_PATHS {
        let value = defaults
            .shift_remove(key)
            .ok_or_else(|| format!("`default_settings.{key}` is missing"))?;
        place(&mut full, at, value)?;
    }
    if let Some(key) = defaults.keys().next() {
        return Err(format!("unknown field `default_settings.{key}`"));
    }
    let post: PostProcessor = serde_json::from_value(full).map_err(|e| e.to_string())?;
    Ok((post, dialect))
}

/// Puts `value` at `path` in `root`, which must have every object on the
/// way and nothing at the end.
fn place(root: &mut Value, path: &[&str], value: Value) -> Result<(), String> {
    let Some((last, parents)) = path.split_last() else {
        return Err("empty setting path".into());
    };
    let mut at = root;
    for key in parents {
        at = at
            .get_mut(*key)
            .ok_or_else(|| format!("`{}` is missing", parents.join(".")))?;
    }
    let object = at
        .as_object_mut()
        .ok_or_else(|| format!("`{}` is not an object", parents.join(".")))?;
    if object.contains_key(*last) {
        return Err(format!(
            "`{}` is a project setting; it belongs in `default_settings`",
            path.join(".")
        ));
    }
    object.insert((*last).to_owned(), value);
    Ok(())
}

/// Every bundled definition version, in listing order: by id, grblHAL
/// first, then oldest version first.
///
/// # Errors
///
/// [`PostProcessorError::BundledDefinition`] if a bundled file is invalid.
/// A test proves none is, so this only fires on a broken build.
pub fn bundled_definitions() -> Result<&'static [BundledDefinition], PostProcessorError> {
    if let Some(bundled) = BUNDLED.get() {
        return Ok(bundled);
    }
    let parsed = SOURCES
        .into_iter()
        .map(BundledDefinition::parse)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(BUNDLED.get_or_init(|| parsed))
}

/// The latest version of every bundled definition, in listing order,
/// grblHAL first: what `dreamcarve posts` lists.
///
/// # Errors
///
/// As for [`bundled_definitions`].
pub fn latest_definitions() -> Result<Vec<&'static BundledDefinition>, PostProcessorError> {
    let mut latest: Vec<&'static BundledDefinition> = Vec::new();
    for definition in bundled_definitions()? {
        match latest.iter_mut().find(|d| d.id == definition.id) {
            Some(slot) if slot.version < definition.version => *slot = definition,
            Some(_) => {}
            None => latest.push(definition),
        }
    }
    Ok(latest)
}

/// The bundled definition `id` at `version`.
///
/// # Errors
///
/// [`PostProcessorError::UnknownId`] or
/// [`PostProcessorError::UnknownVersion`], each listing what is bundled;
/// [`PostProcessorError::BundledDefinition`] as for
/// [`bundled_definitions`].
pub fn bundled_definition(
    id: &str,
    version: u32,
) -> Result<&'static BundledDefinition, PostProcessorError> {
    let versions: Vec<&'static BundledDefinition> = bundled_definitions()?
        .iter()
        .filter(|d| d.id == id)
        .collect();
    if versions.is_empty() {
        return Err(unknown_id(id));
    }
    versions
        .iter()
        .find(|d| d.version == version)
        .copied()
        .ok_or_else(|| PostProcessorError::UnknownVersion {
            id: id.to_owned(),
            version,
            available: versions.iter().map(|d| d.version).collect(),
        })
}

/// The latest bundled version of `id`.
///
/// # Errors
///
/// [`PostProcessorError::UnknownId`], listing the bundled ids;
/// [`PostProcessorError::BundledDefinition`] as for
/// [`bundled_definitions`].
pub fn latest_definition(id: &str) -> Result<&'static BundledDefinition, PostProcessorError> {
    latest_definitions()?
        .into_iter()
        .find(|d| d.id == id)
        .ok_or_else(|| unknown_id(id))
}

/// A setting [`switch_post`] could not keep: the user had changed it, but
/// the new definition cannot use the value, so it took the new default.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingReset {
    /// The setting's key in the reference, for example `tool_change_mode`.
    pub setting: &'static str,
    /// The value it had, as the `.dcv` writes it (`"m6"`, `true`).
    pub from: String,
    /// The value it has now, the new definition's default, written the
    /// same way.
    pub to: String,
    /// Why the new definition cannot use `from`.
    pub reason: ResetReason,
}

/// Why [`switch_post`] reset a setting: the rule `validate` would have
/// broken with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResetReason {
    /// `tool_change_mode: "m6"` on a definition whose tool change has no
    /// `M6` line.
    NoM6,
    /// `use_canned_cycles: true` on a definition without canned cycles.
    NoCannedCycles,
}

/// The latest version of `id` with `old`'s settings carried over
/// (`docs/specs/phase-2/functional.md`, capability 27): what
/// `export --post <id>` writes. Each setting still at the default of the
/// version `old` names takes the new version's default; one the user
/// changed is kept, unless the new definition cannot use it, and then it
/// takes the new default and is reported. So switching to the project's
/// own id changes only `definition_version`.
///
/// # Errors
///
/// As for [`bundled_definition`] for `old`'s definition, and as for
/// [`latest_definition`] for `id`.
pub fn switch_post(
    old: &PostProcessorRef,
    id: &str,
) -> Result<(PostProcessorRef, Vec<SettingReset>), PostProcessorError> {
    let old_default = old.definition()?.default_ref();
    let target = latest_definition(id)?;
    let post = target.post_processor();
    let mut new = target.default_ref();
    let mut resets = Vec::new();

    if old.file_extension != old_default.file_extension {
        new.file_extension.clone_from(&old.file_extension);
    }
    if old.spindle_control != old_default.spindle_control {
        new.spindle_control = old.spindle_control;
    }
    // Bit equality, not a tolerance: the question is whether the user
    // changed the number, and an unchanged one is the default's own bits,
    // read from the same JSON text.
    if old.spindle_on_dwell_s.to_bits() != old_default.spindle_on_dwell_s.to_bits() {
        new.spindle_on_dwell_s = old.spindle_on_dwell_s;
    }
    if old.decimal_places != old_default.decimal_places {
        new.decimal_places = old.decimal_places;
    }
    if old.tool_change_mode != old_default.tool_change_mode {
        if old.tool_change_mode == ToolChangeMode::M6 && !post.tool_change.has_m6() {
            resets.push(SettingReset {
                setting: "tool_change_mode",
                from: setting_text(&old.tool_change_mode),
                to: setting_text(&new.tool_change_mode),
                reason: ResetReason::NoM6,
            });
        } else {
            new.tool_change_mode = old.tool_change_mode;
        }
    }
    if old.use_canned_cycles != old_default.use_canned_cycles {
        if old.use_canned_cycles && post.canned_cycles.is_none() {
            resets.push(SettingReset {
                setting: "use_canned_cycles",
                from: setting_text(&old.use_canned_cycles),
                to: setting_text(&new.use_canned_cycles),
                reason: ResetReason::NoCannedCycles,
            });
        } else {
            new.use_canned_cycles = old.use_canned_cycles;
        }
    }
    Ok((new, resets))
}

/// `value` as the `.dcv` writes it: `"m6"`, `true`.
fn setting_text(value: &impl Serialize) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

/// [`PostProcessorError::UnknownId`] for `id`, listing the bundled ids
/// sorted.
pub(crate) fn unknown_id(id: &str) -> PostProcessorError {
    let mut known: Vec<String> = Vec::new();
    for (bundled, ..) in SOURCES {
        if !known.iter().any(|k| k == bundled) {
            known.push(bundled.to_owned());
        }
    }
    known.sort();
    PostProcessorError::UnknownId {
        id: id.to_owned(),
        known,
    }
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
                machine_command: None,
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
                spindle_on_dwell: SpindleDwell {
                    command: "G4 P{seconds}".into(),
                    duration_s: 3.0,
                    units: DwellUnits::Seconds,
                },
                spindle_control: SpindleControl::Automatic,
            },
            tool_change: ToolChange {
                commands: vec!["M6 T{toolNumber}".into()],
                stop_spindle_first: true,
                pause_after_change: false,
                pause_command: "M0".into(),
                mode: ToolChangeMode::M6,
            },
            use_canned_cycles: false,
            canned_cycles: Some(CannedCycles {
                drill_command: Some("G81".into()),
                drill_with_dwell_command: None,
                peck_drill_command: Some("G83".into()),
                chip_break_drill_command: None,
                peck_step_word: "Q".into(),
                retract_mode: Some(RetractMode::G98),
                cancel_command: "G80".into(),
                reentry_gap_mm: None,
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
    fn spindle_dwell_round_trips() {
        let post = sample();
        let json = serde_json::to_value(&post).unwrap();
        assert_eq!(
            json["feed_speed"]["spindle_on_dwell"],
            serde_json::json!({ "command": "G4 P{seconds}", "duration_s": 3.0, "units": "seconds" })
        );
        let back: PostProcessor = serde_json::from_value(json).unwrap();
        assert_eq!(back, post);
    }

    /// #117, design spec "Post-processor definitions": the dwell's `P`
    /// word is seconds on GRBL, grblHAL and LinuxCNC and milliseconds on
    /// UCCNC, so the definition says which. A definition without it is
    /// rejected like any missing field (#65).
    #[test]
    fn dwell_units_round_trip_as_snake_case() {
        let mut post = sample();
        for (units, text) in [
            (DwellUnits::Seconds, "seconds"),
            (DwellUnits::Milliseconds, "milliseconds"),
        ] {
            post.feed_speed.spindle_on_dwell.units = units;
            let json = serde_json::to_value(&post).unwrap();
            assert_eq!(
                json["feed_speed"]["spindle_on_dwell"]["units"], text,
                "{units:?}"
            );
            let back: PostProcessor = serde_json::from_value(json).unwrap();
            assert_eq!(back, post, "{units:?}");
        }
        let mut value = serde_json::to_value(&post).unwrap();
        value["feed_speed"]["spindle_on_dwell"]
            .as_object_mut()
            .unwrap()
            .remove("units");
        let err = serde_json::from_value::<PostProcessor>(value).unwrap_err();
        assert!(err.to_string().contains("units"), "{err}");
    }

    /// #117, design spec "Canned cycles": the re-entry gap of a chip-break
    /// cycle is a definition fact, `null` where the controller retracts
    /// fully, and required like every `Option` field.
    #[test]
    fn reentry_gap_is_required_and_null_when_unset() {
        let post = sample();
        let json = serde_json::to_value(&post).unwrap();
        assert!(json["canned_cycles"]["reentry_gap_mm"].is_null());
        let mut value = json.clone();
        value["canned_cycles"]["reentry_gap_mm"] = serde_json::json!(0.5);
        let back: PostProcessor = serde_json::from_value(value).unwrap();
        assert!(
            matches!(back.canned_cycles, Some(CannedCycles { reentry_gap_mm: Some(gap), .. })
                if (gap - 0.5).abs() < 1e-12)
        );
        let mut value = json;
        value["canned_cycles"]
            .as_object_mut()
            .unwrap()
            .remove("reentry_gap_mm");
        let err = serde_json::from_value::<PostProcessor>(value).unwrap_err();
        assert!(err.to_string().contains("reentry_gap_mm"), "{err}");
    }

    /// #117: whether Drill writes canned cycles is the user's choice per
    /// project (functional spec, open question 2: off everywhere), so it
    /// is a setting in the reference block and in `default_settings`, and
    /// `resolve` carries it onto the definition.
    #[test]
    fn use_canned_cycles_is_a_project_setting() {
        for definition in bundled_definitions().unwrap() {
            let mut reference = definition.default_ref();
            assert!(!reference.use_canned_cycles, "{}", definition.path);
            assert!(!definition.post_processor().use_canned_cycles);
            reference.use_canned_cycles = true;
            assert!(reference.resolve().unwrap().use_canned_cycles);
            let json = serde_json::to_value(&reference).unwrap();
            assert_eq!(json["use_canned_cycles"], serde_json::json!(true));
        }
        let (_, dialect) = parse_definition(include_str!("definitions/grblhal/v1.json")).unwrap();
        assert!(
            dialect.get("use_canned_cycles").is_none(),
            "a setting is not part of the dialect"
        );
    }

    #[test]
    fn spindle_control_round_trips_as_snake_case() {
        let mut post = sample();
        for (control, text) in [
            (SpindleControl::Automatic, "automatic"),
            (SpindleControl::Manual, "manual"),
        ] {
            post.feed_speed.spindle_control = control;
            let json = serde_json::to_value(&post).unwrap();
            assert_eq!(json["feed_speed"]["spindle_control"], text, "{control:?}");
            let back: PostProcessor = serde_json::from_value(json).unwrap();
            assert_eq!(back, post, "{control:?}");
        }
    }

    /// #95: the tool-change mode is a snake_case string in the file.
    #[test]
    fn tool_change_mode_round_trips_as_snake_case() {
        let mut post = sample();
        for (mode, text) in [
            (ToolChangeMode::SeparatePrograms, "separate_programs"),
            (ToolChangeMode::M6, "m6"),
        ] {
            post.tool_change.mode = mode;
            let json = serde_json::to_value(&post).unwrap();
            assert_eq!(json["tool_change"]["mode"], text, "{mode:?}");
            let back: PostProcessor = serde_json::from_value(json).unwrap();
            assert_eq!(back, post, "{mode:?}");
        }
    }

    /// #95, #65: a definition without `tool_change.mode` is rejected, like
    /// any other missing field.
    #[test]
    fn missing_tool_change_mode_is_rejected() {
        let mut value = serde_json::to_value(sample()).unwrap();
        value["tool_change"].as_object_mut().unwrap().remove("mode");
        let err = serde_json::from_value::<PostProcessor>(value).unwrap_err();
        assert!(err.to_string().contains("mode"), "{err}");
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
    fn m6_is_found_in_code_not_comments() {
        let mut change = sample().tool_change;
        for (commands, has) in [
            (vec!["M5", "T{toolNumber} M6"], true),
            (vec!["m06 t{toolNumber}"], true),
            (vec!["M5", "M0 ; Tool change: M6"], false),
            (vec!["M0 (M6)"], false),
            (vec!["M61 Q1"], false),
            (vec![], false),
        ] {
            change.commands = commands.iter().map(|&c| c.to_owned()).collect();
            assert_eq!(change.has_m6(), has, "{commands:?}");
        }
    }

    #[test]
    fn decimal_places_accepts_one_number_for_both_units() {
        let places: DecimalPlaces = serde_json::from_str("3").unwrap();
        assert_eq!(places, DecimalPlaces { mm: 3, inch: 3 });
        let text = serde_json::to_string(&places).unwrap();
        assert_eq!(text, r#"{"mm":3,"inch":3}"#);
    }

    /// One unit in the last place, mm: 0.001 mm at 3 places, 0.0001 in
    /// (0.00254 mm) at 4.
    #[test]
    fn quantum_is_one_unit_in_the_last_place_in_mm() {
        let places = DecimalPlaces { mm: 3, inch: 4 };
        assert!((places.quantum_mm(Units::Mm) - 0.001).abs() < 1e-15);
        assert!((places.quantum_mm(Units::Inch) - 0.002_54).abs() < 1e-15);
    }

    // #106: a project stores a reference to a bundled definition version
    // and the user's settings, not the definition.

    /// A current-version project whose `post_processor` is the GRBL
    /// reference with the default settings.
    const CURRENT_FIXTURE: &str = include_str!("../tests/fixtures/dcv/v22/sample.dcv");

    /// The reference block as the project writes it, keys in order.
    const REFERENCE_BLOCK: &str = r#"{
  "id": "grbl",
  "definition_version": 1,
  "file_extension": "nc",
  "spindle_control": "automatic",
  "spindle_on_dwell_s": 3.0,
  "tool_change_mode": "separate_programs",
  "decimal_places": {
    "mm": 3,
    "inch": 4
  },
  "use_canned_cycles": false
}"#;

    #[test]
    fn project_post_processor_round_trips() {
        let loaded = crate::load(CURRENT_FIXTURE.as_bytes()).unwrap();
        let block = serde_json::to_string_pretty(&loaded.project.post_processor).unwrap();
        assert_eq!(block, REFERENCE_BLOCK);
        assert_eq!(crate::save(&loaded.project).unwrap(), CURRENT_FIXTURE);
    }

    /// `CURRENT_FIXTURE` with `from` replaced by `to`, loaded.
    fn load_edited(from: &str, to: &str) -> Result<crate::Loaded, crate::LoadError> {
        assert!(CURRENT_FIXTURE.contains(from), "the fixture has `{from}`");
        crate::load(CURRENT_FIXTURE.replacen(from, to, 1).as_bytes())
    }

    /// The line and column of the first `needle` in `CURRENT_FIXTURE`.
    fn position_of(needle: &str) -> crate::TextPosition {
        let (line, text) = CURRENT_FIXTURE
            .lines()
            .enumerate()
            .find(|(_, text)| text.contains(needle))
            .unwrap();
        let column = text.find(needle).unwrap() + 1;
        crate::TextPosition {
            line: line + 1,
            column,
        }
    }

    #[test]
    fn missing_post_processor_setting_fails_to_load() {
        let err = load_edited("    \"spindle_on_dwell_s\": 3.0,\n", "").unwrap_err();
        let crate::LoadError::Schema { path, at, message } = &err else {
            panic!("a schema error: {err:?}");
        };
        assert_eq!(path, "post_processor", "{err}");
        // Located at the object it is missing from.
        let mut object = position_of("\"post_processor\": {");
        object.column += "\"post_processor\": ".len();
        assert_eq!(*at, Some(object), "{err}");
        assert!(message.contains("spindle_on_dwell_s"), "{err}");
    }

    #[test]
    fn unknown_post_id_fails_to_load() {
        let err = load_edited("\"id\": \"grbl\"", "\"id\": \"fanuc\"").unwrap_err();
        let crate::LoadError::Schema { path, at, message } = &err else {
            panic!("a schema error: {err:?}");
        };
        assert_eq!(path, "post_processor.id", "{err}");
        let value = position_of("\"grbl\",");
        assert_eq!(*at, Some(value), "{err}");
        assert!(message.contains("`fanuc`"), "{err}");
        assert!(
            message.contains("generic, grbl, grblhal, linuxcnc, mach3, uccnc"),
            "{err}"
        );
    }

    #[test]
    fn unknown_definition_version_fails_to_load() {
        let err =
            load_edited("\"definition_version\": 1", "\"definition_version\": 99").unwrap_err();
        let crate::LoadError::Schema { path, at, message } = &err else {
            panic!("a schema error: {err:?}");
        };
        assert_eq!(path, "post_processor.definition_version", "{err}");
        let mut value = position_of("\"definition_version\": 1");
        value.column += "\"definition_version\": ".len();
        assert_eq!(*at, Some(value), "{err}");
        assert!(message.contains("99"), "{err}");
        assert!(message.contains("`grbl`"), "{err}");
        assert!(message.ends_with("available: 1, 2, 3"), "{err}");
    }

    /// The definition that `id`.json on main parsed to, before #106, kept as
    /// a fixture, plus the fields the schema has gained since with the
    /// values the migration gives a file of that time (#117): the dwell
    /// in seconds, canned cycles off, no re-entry gap.
    fn old_embedded(id: &str) -> PostProcessor {
        let text = match id {
            "grblhal" => include_str!("../tests/fixtures/post/grblhal.json"),
            "grbl" => include_str!("../tests/fixtures/post/grbl.json"),
            _ => panic!("no fixture for {id}"),
        };
        let mut value: Value = serde_json::from_str(text).unwrap();
        value["feed_speed"]["spindle_on_dwell"]["units"] = Value::from("seconds");
        value["use_canned_cycles"] = Value::from(false);
        // #283: both document `G53` (`definitions/README.md`,
        // "Machine-coordinate moves"); nothing writes it unless a project
        // sets the height.
        value["work_coordinates"]["machine_command"] = Value::from("G53");
        if let Some(cycles) = value["canned_cycles"].as_object_mut() {
            cycles.insert("reentry_gap_mm".into(), Value::Null);
        }
        serde_json::from_value(value).unwrap()
    }

    /// Version 1 of each definition is today's file with the settings moved
    /// out, so it must resolve, with the default settings, to exactly what
    /// the file parsed to before: no controller-facing value changed.
    #[test]
    fn resolve_matches_old_embedded_definition() {
        for id in ["grblhal", "grbl"] {
            let definition = bundled_definition(id, 1).unwrap();
            let resolved = definition.default_ref().resolve().unwrap();
            assert_eq!(resolved, old_embedded(id), "{id}");
        }
    }

    /// Every bundled version parses, and has the id and path it is listed
    /// under.
    #[test]
    fn bundled_definitions_parse() {
        let bundled = bundled_definitions().unwrap();
        assert_eq!(bundled.len(), SOURCES.len());
        for d in bundled {
            assert_eq!(d.post_processor().id, d.id, "{}", d.path);
            assert_eq!(d.path, format!("{}/v{}.json", d.id, d.version));
        }
    }

    /// The grblHAL version 1 file with `from` replaced by `to`, parsed.
    fn parse_edited(from: &str, to: &str) -> Result<(PostProcessor, Value), String> {
        let text = include_str!("definitions/grblhal/v1.json");
        assert!(text.contains(from), "the file has `{from}`");
        parse_definition(&text.replacen(from, to, 1))
    }

    /// A definition leaves the project's settings to `default_settings`:
    /// one that sets one itself is invalid, rather than overridden.
    #[test]
    fn definition_setting_a_project_setting_is_invalid() {
        let err = parse_edited("{\n", "{\n  \"file_extension\": \"tap\",\n").unwrap_err();
        assert!(
            err.contains("`file_extension` is a project setting"),
            "{err}"
        );
    }

    #[test]
    fn definition_default_settings_are_strict() {
        let err = parse_edited("\"tool_change_mode\": \"separate_programs\",\n", "").unwrap_err();
        assert!(err.contains("default_settings.tool_change_mode"), "{err}");
        let err = parse_edited(
            "\"default_settings\": {\n",
            "\"default_settings\": {\n    \"line_numbers\": true,\n",
        )
        .unwrap_err();
        assert!(err.contains("default_settings.line_numbers"), "{err}");
    }

    // #277, functional capability 27: `export --post` keeps the settings
    // the user changed.

    /// The default reference of `id` at `version`.
    fn default_at(id: &str, version: u32) -> PostProcessorRef {
        bundled_definition(id, version).unwrap().default_ref()
    }

    /// The latest version of `id`.
    fn latest_version(id: &str) -> u32 {
        latest_definition(id).unwrap().version
    }

    /// A raised spin-up dwell, the #73 case, survives an upgrade of grblHAL
    /// v1 to its latest version, and nothing is reported reset.
    #[test]
    fn switch_keeps_changed_dwell() {
        let mut old = default_at("grblhal", 1);
        old.spindle_on_dwell_s = 6.0;
        let (new, resets) = switch_post(&old, "grblhal").unwrap();
        assert_eq!(new.definition_version, latest_version("grblhal"));
        assert!(
            (new.spindle_on_dwell_s - 6.0).abs() < 1e-12,
            "{}",
            new.spindle_on_dwell_s
        );
        assert_eq!(resets, []);
    }

    /// A setting still at the old definition's default follows the new
    /// one: grblHAL's `nc` becomes LinuxCNC's `ngc`.
    #[test]
    fn switch_takes_new_default_for_default_extension() {
        let old = latest_definition("grblhal").unwrap().default_ref();
        assert_eq!(old.file_extension, "nc");
        let (new, resets) = switch_post(&old, "linuxcnc").unwrap();
        assert_eq!(new, latest_definition("linuxcnc").unwrap().default_ref());
        assert_eq!(new.file_extension, "ngc");
        assert_eq!(resets, []);
    }

    /// An extension the user chose is theirs, whatever the new
    /// definition's default.
    #[test]
    fn switch_keeps_changed_extension() {
        let mut old = latest_definition("grblhal").unwrap().default_ref();
        old.file_extension = "gcode".into();
        for id in ["grblhal", "grbl", "linuxcnc", "mach3", "uccnc", "generic"] {
            let (new, resets) = switch_post(&old, id).unwrap();
            assert_eq!(new.id, id);
            assert_eq!(new.file_extension, "gcode", "{id}");
            assert_eq!(resets, [], "{id}");
        }
    }

    /// GRBL's tool change has no `M6` (#57), so a grblHAL project's `"m6"`
    /// cannot move with it: it is reset to GRBL's default and reported,
    /// never kept to fail validation.
    #[test]
    fn switch_resets_m6_without_m6_line() {
        let mut old = latest_definition("grblhal").unwrap().default_ref();
        old.tool_change_mode = ToolChangeMode::M6;
        let (new, resets) = switch_post(&old, "grbl").unwrap();
        assert_eq!(new.tool_change_mode, ToolChangeMode::SeparatePrograms);
        assert_eq!(
            resets,
            [SettingReset {
                setting: "tool_change_mode",
                from: "\"m6\"".into(),
                to: "\"separate_programs\"".into(),
                reason: ResetReason::NoM6,
            }]
        );
        // grblHAL has M6, so the mode stays on an upgrade.
        let (same, resets) = switch_post(&old, "grblhal").unwrap();
        assert_eq!(same.tool_change_mode, ToolChangeMode::M6);
        assert_eq!(resets, []);
    }

    /// grblHAL's definition has no canned cycles (#117), so LinuxCNC's
    /// `use_canned_cycles: true` is reset to `false` and reported.
    #[test]
    fn switch_resets_canned_cycles_without_them() {
        let mut old = latest_definition("linuxcnc").unwrap().default_ref();
        old.use_canned_cycles = true;
        let (new, resets) = switch_post(&old, "grblhal").unwrap();
        assert!(!new.use_canned_cycles);
        assert_eq!(
            resets,
            [SettingReset {
                setting: "use_canned_cycles",
                from: "true".into(),
                to: "false".into(),
                reason: ResetReason::NoCannedCycles,
            }]
        );
        // Mach3 has them, so the setting moves there.
        let (mach3, resets) = switch_post(&old, "mach3").unwrap();
        assert!(mach3.use_canned_cycles);
        assert_eq!(resets, []);
    }

    /// `export --post` with the project's own id, to upgrade the version,
    /// changes only the version: from every bundled version, with every
    /// setting changed to a value that definition accepts, and with every
    /// setting at its default.
    #[test]
    fn switch_same_id_changes_only_version() {
        for definition in bundled_definitions().unwrap() {
            let post = definition.post_processor();
            let mut changed = definition.default_ref();
            changed.file_extension = "gcode".into();
            changed.spindle_control = SpindleControl::Manual;
            changed.spindle_on_dwell_s = 6.5;
            changed.decimal_places = DecimalPlaces { mm: 2, inch: 3 };
            changed.tool_change_mode = if post.tool_change.has_m6() {
                ToolChangeMode::M6
            } else {
                ToolChangeMode::SeparatePrograms
            };
            changed.use_canned_cycles = post.canned_cycles.is_some();
            for old in [definition.default_ref(), changed] {
                let (new, resets) = switch_post(&old, definition.id).unwrap();
                let mut expected = old.clone();
                expected.definition_version = latest_version(definition.id);
                assert_eq!(new, expected, "{}", definition.path);
                assert_eq!(resets, [], "{}", definition.path);
            }
        }
    }

    /// FNV-1a, 64 bit: enough to notice an edit, which is all the pin is
    /// for. It is not a security measure.
    fn fnv1a(bytes: &[u8]) -> u64 {
        bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &b| {
            (hash ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    }

    /// A released definition version never changes: a project that names it
    /// must export the same bytes after any upgrade. A change that would
    /// change emitted bytes is a new version file (`definitions/README.md`,
    /// "Versions"); pin it here when it is added.
    #[test]
    fn released_definition_versions_are_unchanged() {
        // Re-pinned in #117 when `spindle_on_dwell.units` and
        // `default_settings.use_canned_cycles` reached every version, and
        // in #283 when `work_coordinates.machine_command` did (schema-wide
        // fields; see `definitions/README.md`, "Versions"). LinuxCNC's
        // were re-pinned in #306, when its `machine_command` became
        // `"G53"`: that writes nothing unless a project sets the machine
        // Z, which both versions refused before.
        const PINNED: [(&str, u64); 16] = [
            ("grblhal/v1.json", 0x4069_e336_d88c_fe81),
            // #171: the project name no longer opens a comment.
            ("grblhal/v2.json", 0xabe7_6c89_c5f8_6bf8),
            // #279: the header cancels canned-cycle motion and cutter
            // compensation (`G80`, `G40`).
            ("grblhal/v3.json", 0x341b_511e_68ea_cdde),
            // #316: the header cancels scaling (`G50`).
            ("grblhal/v4.json", 0xeb50_edfd_1a5d_8aa4),
            ("grbl/v1.json", 0xfa5f_26f9_ddd0_790f),
            ("grbl/v2.json", 0x3970_4e64_8e3c_4d04),
            ("grbl/v3.json", 0x5696_f0ba_8779_d54e),
            // Slice 17 (#164).
            ("linuxcnc/v1.json", 0x155d_d13d_9406_23a9),
            // #286: no `G49` in the header.
            ("linuxcnc/v2.json", 0x0622_7196_706c_b93f),
            ("mach3/v1.json", 0xeff3_be07_97a2_28bf),
            ("mach3/v2.json", 0x7604_07ca_afcf_53d5),
            // #312: the header cancels scaling and rotation (`G50`, `G69`).
            ("mach3/v3.json", 0xc840_8d2b_7fd4_33a9),
            ("uccnc/v1.json", 0x1c80_d6fb_1d78_ab36),
            ("uccnc/v2.json", 0xfae6_efe4_6cee_ccbc),
            ("uccnc/v3.json", 0x7822_1085_2958_badc),
            ("generic/v1.json", 0x0fa6_f809_7b33_a95c),
        ];
        let bundled = bundled_definitions().unwrap();
        let paths: Vec<&str> = bundled.iter().map(|d| d.path).collect();
        let pinned: Vec<&str> = PINNED.iter().map(|&(path, _)| path).collect();
        assert_eq!(paths, pinned, "every bundled version is pinned");
        for (definition, (path, hash)) in bundled.iter().zip(PINNED) {
            assert_eq!(
                fnv1a(definition.source.as_bytes()),
                hash,
                "{path} changed; add a new version instead"
            );
        }
    }
}
