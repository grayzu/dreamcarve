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

/// The [`Source`] of one bundled definition, compiled in from
/// `definitions/<id>/v<version>.json`.
macro_rules! source {
    ($id:literal, $version:literal) => {
        (
            $id,
            $version,
            concat!($id, "/v", $version, ".json"),
            include_str!(concat!("definitions/", $id, "/v", $version, ".json")),
        )
    };
}

/// Every bundled definition version, compiled in, in listing order: by
/// id (grblHAL, GRBL, LinuxCNC, Mach3, UCCNC, generic), then oldest
/// version first. A released version is
/// never edited or removed (ADR-0004, `definitions/README.md`); a change
/// that would change emitted bytes is a new version file.
/// `tests::released_definition_versions_are_unchanged` pins each one.
const SOURCES: [Source; 16] = [
    source!("grblhal", 1),
    source!("grblhal", 2),
    source!("grblhal", 3),
    source!("grblhal", 4),
    source!("grbl", 1),
    source!("grbl", 2),
    source!("grbl", 3),
    source!("linuxcnc", 1),
    source!("linuxcnc", 2),
    source!("mach3", 1),
    source!("mach3", 2),
    source!("mach3", 3),
    source!("uccnc", 1),
    source!("uccnc", 2),
    source!("uccnc", 3),
    source!("generic", 1),
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
mod tests;
