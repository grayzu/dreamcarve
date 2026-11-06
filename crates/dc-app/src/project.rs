// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Project panel's model (`docs/specs/phase-4/functional.md`,
//! capabilities 60, 61 and 62; mock-up `03-stock-origin.svg`): every
//! field the panel shows, the `.dcv` field it writes, the text it shows,
//! the command a typed value becomes, the validation message located on
//! it, and what the post-processor's definition offers. The shell draws
//! the rows and runs the commands; nothing here holds state.
//!
//! A field shows the project's value in its display units and parses what
//! the file accepts ([`crate::units`]). Its message is the file's own:
//! validation's finding whose [`Finding::field`] names the field, an error
//! before a warning (`experience.md`, section 7, "Inline validation").
//! What the definition offers is decided as `validate` decides it: the
//! tool-change machine Z where `work_coordinates.machine_command` is set
//! (`G53`), one program with `M6` where the tool change has `M6`, canned
//! cycles where the definition has them.

use dc_geometry::Point;
use dc_job::{Finding, Tier};
use dc_project::{
    DecimalPlaces, Heights, MachineOrigin, OriginXy, OriginZ, ParkXy, PostProcessorRef, Problem,
    Project, ResetReason, SettingReset, SpindleControl, Stock, ToolChangeMode, Units,
};

use crate::app::App;
use crate::command::Command;
use crate::error::CommandError;
use crate::strings::{StrKey, Strings};
use crate::units;

/// The margin *Fit to shapes* starts with, mm: `init`'s (capability 61).
pub const DEFAULT_FIT_MARGIN_MM: f64 = dc_job::project_from::STOCK_MARGIN_MM;

/// The arc-fitting tolerance switching arc fitting on writes, mm: the one
/// `init` and *New* write.
pub const DEFAULT_ARC_FIT_MM: f64 = dc_job::project_from::ARC_FIT_TOLERANCE_MM;

/// A field the user types into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Field {
    /// `stock.width_mm`.
    StockWidth,
    /// `stock.height_mm`.
    StockHeight,
    /// `stock.thickness_mm`; empty is unset.
    StockThickness,
    /// A custom origin's X.
    OriginX,
    /// A custom origin's Y.
    OriginY,
    /// `heights.clearance_mm`; empty is unset.
    Clearance,
    /// `heights.tool_change_machine_z_mm`; empty is unset.
    ToolChangeZ,
    /// `heights.park`, typed `X, Y`; empty is unset.
    Park,
    /// `flatten_tolerance_mm`.
    FlattenTolerance,
    /// `arc_fit_tolerance_mm`; empty switches arc fitting off.
    ArcFitTolerance,
    /// `post_processor.file_extension`, with or without its dot.
    FileExtension,
    /// `post_processor.spindle_on_dwell_s`, seconds.
    SpindleDwell,
    /// `post_processor.decimal_places.mm`.
    DecimalsMm,
    /// `post_processor.decimal_places.inch`.
    DecimalsInch,
}

/// What a field's number is, for its unit suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quantity {
    /// A length in the project's display units.
    Length,
    /// Seconds.
    Seconds,
    /// A count or a name: no unit.
    Plain,
}

impl Field {
    /// The `.dcv` field it writes, as validation names it: what the help
    /// tooltip shows and what a located finding is matched against.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::StockWidth => "stock.width_mm",
            Self::StockHeight => "stock.height_mm",
            Self::StockThickness => "stock.thickness_mm",
            Self::OriginX => "origin.xy.custom.x_mm",
            Self::OriginY => "origin.xy.custom.y_mm",
            Self::Clearance => "heights.clearance_mm",
            Self::ToolChangeZ => "heights.tool_change_machine_z_mm",
            Self::Park => "heights.park",
            Self::FlattenTolerance => "flatten_tolerance_mm",
            Self::ArcFitTolerance => "arc_fit_tolerance_mm",
            Self::FileExtension => "post_processor.file_extension",
            Self::SpindleDwell => "post_processor.spindle_on_dwell_s",
            Self::DecimalsMm => "post_processor.decimal_places.mm",
            Self::DecimalsInch => "post_processor.decimal_places.inch",
        }
    }

    /// The label.
    #[must_use]
    pub const fn label(self) -> StrKey {
        match self {
            Self::StockWidth => StrKey::ProjectStockWidth,
            Self::StockHeight => StrKey::ProjectStockHeight,
            Self::StockThickness => StrKey::ProjectStockThickness,
            Self::OriginX => StrKey::ProjectOriginX,
            Self::OriginY => StrKey::ProjectOriginY,
            Self::Clearance => StrKey::ProjectHeightsClearance,
            Self::ToolChangeZ => StrKey::ProjectHeightsToolChangeZ,
            Self::Park => StrKey::ProjectHeightsPark,
            Self::FlattenTolerance => StrKey::ProjectTolerancesFlatten,
            Self::ArcFitTolerance => StrKey::ProjectTolerancesArcFit,
            Self::FileExtension => StrKey::ProjectPostExtension,
            Self::SpindleDwell => StrKey::ProjectPostDwell,
            Self::DecimalsMm => StrKey::ProjectPostDecimalsMm,
            Self::DecimalsInch => StrKey::ProjectPostDecimalsInch,
        }
    }

    /// The one-sentence help its label shows on hover.
    #[must_use]
    pub const fn help(self) -> StrKey {
        match self {
            Self::StockWidth => StrKey::ProjectStockWidthHelp,
            Self::StockHeight => StrKey::ProjectStockHeightHelp,
            Self::StockThickness => StrKey::ProjectStockThicknessHelp,
            Self::OriginX => StrKey::ProjectOriginXHelp,
            Self::OriginY => StrKey::ProjectOriginYHelp,
            Self::Clearance => StrKey::ProjectHeightsClearanceHelp,
            Self::ToolChangeZ => StrKey::ProjectHeightsToolChangeZHelp,
            Self::Park => StrKey::ProjectHeightsParkHelp,
            Self::FlattenTolerance => StrKey::ProjectTolerancesFlattenHelp,
            Self::ArcFitTolerance => StrKey::ProjectTolerancesArcFitHelp,
            Self::FileExtension => StrKey::ProjectPostExtensionHelp,
            Self::SpindleDwell => StrKey::ProjectPostDwellHelp,
            Self::DecimalsMm => StrKey::ProjectPostDecimalsMmHelp,
            Self::DecimalsInch => StrKey::ProjectPostDecimalsInchHelp,
        }
    }

    /// What its number is.
    #[must_use]
    pub const fn quantity(self) -> Quantity {
        match self {
            Self::FileExtension | Self::DecimalsMm | Self::DecimalsInch => Quantity::Plain,
            Self::SpindleDwell => Quantity::Seconds,
            _ => Quantity::Length,
        }
    }

    /// The unit drawn after the value: `mm` or `in` for a length, `s`
    /// for seconds, nothing otherwise.
    #[must_use]
    pub const fn suffix(self, units: Units) -> &'static str {
        match self.quantity() {
            Quantity::Length => units::suffix(units),
            Quantity::Seconds => "s",
            Quantity::Plain => "",
        }
    }

    /// Whether a finding located at `path` is about this field. The park
    /// point is one field for its two coordinates.
    #[must_use]
    pub fn locates(self, path: &str) -> bool {
        path == self.path()
            || (self == Self::Park
                && path
                    .strip_prefix(self.path())
                    .is_some_and(|rest| rest.starts_with('.')))
    }

    /// The message to show under the field: the first error located on
    /// it, else the first warning, else the first hint.
    #[must_use]
    pub fn status(self, findings: &[Finding]) -> Option<FieldStatus<'_>> {
        status(findings, |path| self.locates(path))
    }

    /// The field's text for `project`: the value in the display units with
    /// no unit, or empty when the file has `null`.
    #[must_use]
    pub fn text(self, project: &Project) -> String {
        let units = project.display_units;
        let length = |mm: f64| units::field_text(mm, units);
        let optional = |mm: Option<f64>| mm.map(length).unwrap_or_default();
        let post = &project.post_processor;
        match self {
            Self::StockWidth => length(project.stock.width_mm),
            Self::StockHeight => length(project.stock.height_mm),
            Self::StockThickness => optional(project.stock.thickness_mm),
            Self::OriginX | Self::OriginY => {
                let point = project.origin.xy.point_mm(&project.stock);
                length(if self == Self::OriginX {
                    point.x
                } else {
                    point.y
                })
            }
            Self::Clearance => optional(project.heights.clearance_mm),
            Self::ToolChangeZ => optional(project.heights.tool_change_machine_z_mm),
            Self::Park => project.heights.park.map_or_else(String::new, |park| {
                format!("{}, {}", length(park.x_mm), length(park.y_mm))
            }),
            Self::FlattenTolerance => length(project.flatten_tolerance_mm),
            Self::ArcFitTolerance => optional(project.arc_fit_tolerance_mm),
            Self::FileExtension => post.file_extension.clone(),
            // Seconds are not a length: shown as the file holds them.
            Self::SpindleDwell => units::field_text(post.spindle_on_dwell_s, Units::Mm),
            Self::DecimalsMm => post.decimal_places.mm.to_string(),
            Self::DecimalsInch => post.decimal_places.inch.to_string(),
        }
    }

    /// The command `text`, typed into this field and committed, makes:
    /// `None` when it changes nothing, so leaving a field unchanged is no
    /// undo step.
    ///
    /// # Errors
    ///
    /// [`CommandError::NotAQuantity`] when `text` is not what the field
    /// takes (not a length, not `X, Y`, not a whole number), with the
    /// reason; the file never sees it.
    pub fn set(self, project: &Project, text: &str) -> Result<Option<Command>, CommandError> {
        let units = project.display_units;
        let trimmed = text.trim();
        let length = |t: &str| units::parse_length_mm(t, units);
        let optional = |t: &str| {
            if t.trim().is_empty() {
                Ok(None)
            } else {
                length(t).map(Some)
            }
        };
        let stock = |f: &dyn Fn(&mut Stock)| {
            let mut stock = project.stock.clone();
            f(&mut stock);
            Command::SetStock(stock)
        };
        let heights = |f: &dyn Fn(&mut Heights)| {
            let mut heights = project.heights.clone();
            f(&mut heights);
            Command::SetHeights(heights)
        };
        let post = |f: &dyn Fn(&mut PostProcessorRef)| {
            let mut post = project.post_processor.clone();
            f(&mut post);
            Command::SetPostSettings(post)
        };
        let command = match self {
            Self::StockWidth => {
                let mm = length(text)?;
                stock(&|s| s.width_mm = mm)
            }
            Self::StockHeight => {
                let mm = length(text)?;
                stock(&|s| s.height_mm = mm)
            }
            Self::StockThickness => {
                let mm = optional(text)?;
                stock(&|s| s.thickness_mm = mm)
            }
            Self::OriginX | Self::OriginY => {
                let mm = length(text)?;
                let point = project.origin.xy.point_mm(&project.stock);
                let (x_mm, y_mm) = if self == Self::OriginX {
                    (mm, point.y)
                } else {
                    (point.x, mm)
                };
                Command::SetOrigin(MachineOrigin {
                    xy: OriginXy::Custom { x_mm, y_mm },
                    z: project.origin.z,
                })
            }
            Self::Clearance => {
                let mm = optional(text)?;
                heights(&|h| h.clearance_mm = mm)
            }
            Self::ToolChangeZ => {
                let mm = optional(text)?;
                heights(&|h| h.tool_change_machine_z_mm = mm)
            }
            Self::Park => {
                let park = parse_park(text, units)?;
                heights(&|h| h.park = park)
            }
            Self::FlattenTolerance => Command::SetTolerances {
                flatten_mm: length(text)?,
                arc_fit_mm: project.arc_fit_tolerance_mm,
            },
            Self::ArcFitTolerance => Command::SetTolerances {
                flatten_mm: project.flatten_tolerance_mm,
                arc_fit_mm: optional(text)?,
            },
            Self::FileExtension => {
                let extension = trimmed.strip_prefix('.').unwrap_or(trimmed).to_owned();
                post(&|p| p.file_extension.clone_from(&extension))
            }
            Self::SpindleDwell => {
                let seconds = parse_seconds(text)?;
                post(&|p| p.spindle_on_dwell_s = seconds)
            }
            Self::DecimalsMm | Self::DecimalsInch => {
                let places = parse_places(text)?;
                let mm = self == Self::DecimalsMm;
                post(&|p| {
                    p.decimal_places = DecimalPlaces {
                        mm: if mm { places } else { p.decimal_places.mm },
                        inch: if mm { p.decimal_places.inch } else { places },
                    };
                })
            }
        };
        Ok(changes(project, command))
    }
}

/// A row that is chosen rather than typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Choice {
    /// `origin.xy`: a corner, the centre, or custom.
    OriginXy,
    /// `origin.z`: the stock's top or bottom.
    OriginZ,
    /// `post_processor.id`: the bundled definition.
    Post,
    /// `post_processor.spindle_control`.
    SpindleControl,
    /// `post_processor.tool_change_mode`.
    ToolChangeMode,
    /// `post_processor.use_canned_cycles`.
    CannedCycles,
}

impl Choice {
    /// The `.dcv` field it writes.
    #[must_use]
    pub const fn path(self) -> &'static str {
        match self {
            Self::OriginXy => "origin.xy",
            Self::OriginZ => "origin.z",
            Self::Post => "post_processor.id",
            Self::SpindleControl => "post_processor.spindle_control",
            Self::ToolChangeMode => "post_processor.tool_change_mode",
            Self::CannedCycles => "post_processor.use_canned_cycles",
        }
    }

    /// The label.
    #[must_use]
    pub const fn label(self) -> StrKey {
        match self {
            Self::OriginXy => StrKey::ProjectOriginXy,
            Self::OriginZ => StrKey::ProjectOriginZ,
            Self::Post => StrKey::ProjectPostController,
            Self::SpindleControl => StrKey::ProjectPostSpindle,
            Self::ToolChangeMode => StrKey::ProjectPostToolChange,
            Self::CannedCycles => StrKey::ProjectPostCanned,
        }
    }

    /// The one-sentence help its label shows on hover.
    #[must_use]
    pub const fn help(self) -> StrKey {
        match self {
            Self::OriginXy => StrKey::ProjectOriginXyHelp,
            Self::OriginZ => StrKey::ProjectOriginZHelp,
            Self::Post => StrKey::ProjectPostControllerHelp,
            Self::SpindleControl => StrKey::ProjectPostSpindleHelp,
            Self::ToolChangeMode => StrKey::ProjectPostToolChangeHelp,
            Self::CannedCycles => StrKey::ProjectPostCannedHelp,
        }
    }

    /// The message to show under the row, as for [`Field::status`]. A
    /// finding on any of the origin's or the reference's own fields is
    /// one of these rows'.
    #[must_use]
    pub fn status(self, findings: &[Finding]) -> Option<FieldStatus<'_>> {
        status(findings, |path| {
            path == self.path() || (self == Self::Post && path == "post_processor")
        })
    }
}

/// A field's validation state: the file's message and how much it matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FieldStatus<'a> {
    /// Error, warning or hint.
    pub tier: Tier,
    /// The file's message, verbatim.
    pub message: &'a str,
}

/// The first finding `locates` accepts, errors before warnings before
/// hints.
fn status<'a>(findings: &'a [Finding], locates: impl Fn(&str) -> bool) -> Option<FieldStatus<'a>> {
    [Tier::Error, Tier::Warning, Tier::Hint]
        .into_iter()
        .find_map(|tier| {
            findings
                .iter()
                .find(|f| f.tier == tier && f.field.as_deref().is_some_and(&locates))
        })
        .map(|f| FieldStatus {
            tier: f.tier,
            message: &f.message,
        })
}

/// The XY origin's choices (capability 60): the four corners, the centre,
/// or a custom point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OriginPreset {
    /// The stock corner at (0, 0).
    BottomLeft,
    /// The stock corner at (width, 0).
    BottomRight,
    /// The stock corner at (0, height).
    TopLeft,
    /// The stock corner at (width, height).
    TopRight,
    /// The centre of the stock.
    Centre,
    /// A point the user types.
    Custom,
}

impl OriginPreset {
    /// Every choice, in the order the dropdown lists them.
    pub const ALL: [Self; 6] = [
        Self::BottomLeft,
        Self::BottomRight,
        Self::TopLeft,
        Self::TopRight,
        Self::Centre,
        Self::Custom,
    ];

    /// The choice `xy` is.
    #[must_use]
    pub const fn of(xy: &OriginXy) -> Self {
        match xy {
            OriginXy::BottomLeft => Self::BottomLeft,
            OriginXy::BottomRight => Self::BottomRight,
            OriginXy::TopLeft => Self::TopLeft,
            OriginXy::TopRight => Self::TopRight,
            OriginXy::Centre => Self::Centre,
            OriginXy::Custom { .. } => Self::Custom,
        }
    }

    /// Its name in the dropdown.
    #[must_use]
    pub const fn label(self) -> StrKey {
        match self {
            Self::BottomLeft => StrKey::ProjectOriginBottomLeft,
            Self::BottomRight => StrKey::ProjectOriginBottomRight,
            Self::TopLeft => StrKey::ProjectOriginTopLeft,
            Self::TopRight => StrKey::ProjectOriginTopRight,
            Self::Centre => StrKey::ProjectOriginCentre,
            Self::Custom => StrKey::ProjectOriginCustom,
        }
    }
}

/// `command` when it changes `project`; `None` when applying it gives
/// the same project, or it cannot be applied (the shell would only report
/// the refusal for a value the panel built itself).
fn changes(project: &Project, command: Command) -> Option<Command> {
    match command.apply(project) {
        Ok(next) if next == *project => None,
        _ => Some(command),
    }
}

/// The command that sets the XY origin to `preset`. *Custom* starts at
/// the point the origin is at now, so choosing it moves nothing until X
/// or Y is typed.
#[must_use]
pub fn set_origin_xy(project: &Project, preset: OriginPreset) -> Option<Command> {
    let xy = match preset {
        OriginPreset::BottomLeft => OriginXy::BottomLeft,
        OriginPreset::BottomRight => OriginXy::BottomRight,
        OriginPreset::TopLeft => OriginXy::TopLeft,
        OriginPreset::TopRight => OriginXy::TopRight,
        OriginPreset::Centre => OriginXy::Centre,
        OriginPreset::Custom => {
            let point = project.origin.xy.point_mm(&project.stock);
            OriginXy::Custom {
                x_mm: point.x,
                y_mm: point.y,
            }
        }
    };
    changes(
        project,
        Command::SetOrigin(MachineOrigin {
            xy,
            z: project.origin.z,
        }),
    )
}

/// The command that sets Z zero to the stock's top or bottom.
#[must_use]
pub fn set_origin_z(project: &Project, z: OriginZ) -> Option<Command> {
    changes(
        project,
        Command::SetOrigin(MachineOrigin {
            xy: project.origin.xy.clone(),
            z,
        }),
    )
}

/// The command that switches arc fitting on (at [`DEFAULT_ARC_FIT_MM`])
/// or off (`null`).
#[must_use]
pub fn set_arc_fit(project: &Project, on: bool) -> Option<Command> {
    let arc_fit_mm = match (on, project.arc_fit_tolerance_mm) {
        (true, Some(mm)) => Some(mm),
        (true, None) => Some(DEFAULT_ARC_FIT_MM),
        (false, _) => None,
    };
    changes(
        project,
        Command::SetTolerances {
            flatten_mm: project.flatten_tolerance_mm,
            arc_fit_mm,
        },
    )
}

/// The command that sets who switches the spindle.
#[must_use]
pub fn set_spindle_control(project: &Project, control: SpindleControl) -> Option<Command> {
    let mut post = project.post_processor.clone();
    post.spindle_control = control;
    changes(project, Command::SetPostSettings(post))
}

/// The command that sets the tool-change mode; `None` for `M6` on a
/// definition that cannot write it (capability 62).
#[must_use]
pub fn set_tool_change_mode(project: &Project, mode: ToolChangeMode) -> Option<Command> {
    if !tool_change_modes(project).contains(&mode) {
        return None;
    }
    let mut post = project.post_processor.clone();
    post.tool_change_mode = mode;
    changes(project, Command::SetPostSettings(post))
}

/// The command that switches canned cycles on or off; `None` for on with
/// a definition that has none (capability 62).
#[must_use]
pub fn set_canned_cycles(project: &Project, on: bool) -> Option<Command> {
    if on && offers(project).canned_cycles != Offer::Offered {
        return None;
    }
    let mut post = project.post_processor.clone();
    post.use_canned_cycles = on;
    changes(project, Command::SetPostSettings(post))
}

/// The command that switches to the latest version of bundled
/// definition `id`, keeping the settings it can use
/// ([`dc_project::switch_post`]); `None` when the project is already on
/// it. Picking the project's own post-processor at an older version
/// upgrades it.
#[must_use]
pub fn switch_post(project: &Project, id: &str) -> Option<Command> {
    changes(project, Command::SwitchPost { id: id.to_owned() })
}

/// The margin field's starting text, `init`'s margin in `units`.
#[must_use]
pub fn fit_margin_text(units: Units) -> String {
    units::field_text(DEFAULT_FIT_MARGIN_MM, units)
}

/// *Fit to shapes* with the margin `text` (capability 61).
///
/// # Errors
///
/// [`CommandError::NotAQuantity`] when `text` is not a length.
pub fn fit_stock(project: &Project, text: &str) -> Result<Command, CommandError> {
    Ok(Command::FitStockToShapes {
        margin_mm: units::parse_length_mm(text, project.display_units)?,
    })
}

/// Whether the definition offers a setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Offer {
    /// The definition can write it.
    Offered,
    /// It cannot; `reason` is the CLI's message for the setting on this
    /// definition, the hover text of the disabled control.
    Refused {
        /// Why, as `check` says it.
        reason: String,
    },
}

/// What the project's post-processor definition offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offers {
    /// The tool-change machine Z: needs `G53`.
    pub tool_change_z: Offer,
    /// One program with `M6`: needs `M6` in the tool change.
    pub m6: Offer,
    /// Canned drill cycles: need the definition's cycles.
    pub canned_cycles: Offer,
}

/// What the definition `project` names offers (capabilities 60 and 62),
/// by the rules `validate` applies, refused with its messages. A
/// reference that names no bundled definition offers nothing; only a
/// project built in code has one.
#[must_use]
pub fn offers(project: &Project) -> Offers {
    let post = &project.post_processor;
    let definition = post.definition().ok().map(|d| d.post_processor());
    let offer = |offered: bool, problem: Problem| {
        if offered {
            Offer::Offered
        } else {
            Offer::Refused {
                reason: problem.to_string(),
            }
        }
    };
    let name = || post.id.clone();
    Offers {
        tool_change_z: offer(
            definition.is_some_and(|d| d.work_coordinates.machine_command.is_some()),
            Problem::MachineZNeedsG53 { post: name() },
        ),
        m6: offer(
            definition.is_some_and(|d| d.tool_change.has_m6()),
            Problem::ToolChangeModeNeedsM6 { post: name() },
        ),
        canned_cycles: offer(
            definition.is_some_and(|d| d.canned_cycles.is_some()),
            Problem::CannedCyclesUnsupported { post: name() },
        ),
    }
}

/// The tool-change modes the dropdown lists: one program per tool
/// always, one program with `M6` where the definition has it, and the
/// project's own mode even where it does not, so a hand-edited file shows
/// what it holds (with the file's error under it).
#[must_use]
pub fn tool_change_modes(project: &Project) -> Vec<ToolChangeMode> {
    let mut modes = vec![ToolChangeMode::SeparatePrograms];
    if offers(project).m6 == Offer::Offered
        || project.post_processor.tool_change_mode == ToolChangeMode::M6
    {
        modes.push(ToolChangeMode::M6);
    }
    modes
}

/// One bundled post-processor in the Controller list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PostChoice {
    /// Its id, for example `grblhal`.
    pub id: &'static str,
    /// Its name, for example `grblHAL`.
    pub name: String,
    /// Its latest bundled version.
    pub version: u32,
}

/// The bundled post-processors at their latest version, in the order
/// `dreamcarve posts` lists them (capability 62).
///
/// # Errors
///
/// [`CommandError::PostProcessor`] if a bundled definition cannot be
/// read, which no release ships.
pub fn posts() -> Result<Vec<PostChoice>, CommandError> {
    Ok(dc_project::latest_definitions()?
        .into_iter()
        .map(|d| PostChoice {
            id: d.id,
            name: d.post_processor().name.clone(),
            version: d.version,
        })
        .collect())
}

/// The latest version of the project's post-processor when the project
/// is on an older one, for the note under the list.
#[must_use]
pub fn newer_version(project: &Project) -> Option<u32> {
    let post = &project.post_processor;
    dc_project::latest_definition(&post.id)
        .ok()
        .map(|d| d.version)
        .filter(|&latest| latest > post.definition_version)
}

/// The Controller list's text for the project's post-processor:
/// `grblHAL v4 (latest)`, or `grblHAL v1` on an older version.
#[must_use]
pub fn post_label(strings: &Strings, project: &Project) -> String {
    let post = &project.post_processor;
    let name = post
        .definition()
        .map_or_else(|_| post.id.clone(), |d| d.post_processor().name.clone());
    let version = post.definition_version.to_string();
    let key = if newer_version(project).is_some() {
        StrKey::ProjectPostVersion
    } else {
        StrKey::ProjectPostVersionLatest
    };
    strings.fmt(key, &[("name", &name), ("version", &version)])
}

/// One line per setting the last switch reset, as `export --post` prints
/// it: *Reset tool_change_mode "m6" → "separate_programs": post-processor
/// linuxcnc has no M6 tool change.*
#[must_use]
pub fn reset_messages(
    strings: &Strings,
    project: &Project,
    resets: &[SettingReset],
) -> Vec<String> {
    resets
        .iter()
        .map(|reset| {
            let why = strings.get(match reset.reason {
                ResetReason::NoM6 => StrKey::ProjectPostResetNoM6,
                ResetReason::NoCannedCycles => StrKey::ProjectPostResetNoCycles,
            });
            strings.fmt(
                StrKey::ProjectPostReset,
                &[
                    ("setting", reset.setting),
                    ("from", &reset.from),
                    ("to", &reset.to),
                    ("post", &project.post_processor.id),
                    ("why", why),
                ],
            )
        })
        .collect()
}

/// What the Viewport draws for the panel's fields, project space, mm
/// (Y-up).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Marks {
    /// The work zero, where the crosshair is drawn.
    pub origin_mm: Point,
    /// The park point, when set.
    pub park_mm: Option<Point>,
}

/// Where the origin crosshair and the park mark are for `project`.
#[must_use]
pub fn marks(project: &Project) -> Marks {
    Marks {
        origin_mm: project.origin.xy.point_mm(&project.stock),
        park_mm: project
            .heights
            .park
            .map(|park| Point::new(park.x_mm, park.y_mm)),
    }
}

/// The command a click on the Viewport at `at_mm` (project space) makes
/// while the park point is being picked.
#[must_use]
pub fn set_park(project: &Project, at_mm: Point) -> Option<Command> {
    let mut heights = project.heights.clone();
    heights.park = Some(ParkXy {
        x_mm: at_mm.x,
        y_mm: at_mm.y,
    });
    changes(project, Command::SetHeights(heights))
}

/// Why export is refused, when it is (S20: on the CLI's conditions): the
/// first error among the current snapshot's findings. The thickness
/// unset is one (`stock_thickness_unset`).
#[must_use]
pub fn export_refusal(app: &App) -> Option<&Finding> {
    app.warnings().iter().find(|f| f.tier == Tier::Error)
}

/// `X, Y` as a park point; empty is none.
fn parse_park(text: &str, units: Units) -> Result<Option<ParkXy>, CommandError> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let parts: Vec<&str> = text.split(',').collect();
    let [x, y] = parts.as_slice() else {
        return Err(CommandError::NotAQuantity {
            text: text.to_owned(),
            reason: format!("type the park point as X, Y, not `{}`", text.trim()),
        });
    };
    Ok(Some(ParkXy {
        x_mm: units::parse_length_mm(x, units)?,
        y_mm: units::parse_length_mm(y, units)?,
    }))
}

/// Seconds, with or without an `s`.
fn parse_seconds(text: &str) -> Result<f64, CommandError> {
    let trimmed = text.trim();
    let number = trimmed.strip_suffix('s').unwrap_or(trimmed).trim();
    let refused = |reason: String| CommandError::NotAQuantity {
        text: text.to_owned(),
        reason,
    };
    if number.contains(',') {
        return Err(refused(format!("`{trimmed}` has a comma; use a dot")));
    }
    match number.parse::<f64>() {
        Ok(seconds) if seconds.is_finite() => Ok(seconds),
        _ => Err(refused(format!("`{trimmed}` is not a number of seconds"))),
    }
}

/// A whole number of decimal places, 0 to 255 as the file reads it.
fn parse_places(text: &str) -> Result<u8, CommandError> {
    text.trim()
        .parse::<u8>()
        .map_err(|_| CommandError::NotAQuantity {
            text: text.to_owned(),
            reason: format!(
                "`{}` is not a whole number of decimal places from 0 to 255",
                text.trim()
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_project::test_support::sample_project;

    #[test]
    fn park_locates_both_coordinates_and_nothing_else() {
        assert!(Field::Park.locates("heights.park.x_mm"));
        assert!(Field::Park.locates("heights.park.y_mm"));
        assert!(Field::Park.locates("heights.park"));
        assert!(!Field::Park.locates("heights.parked"));
        assert!(!Field::Clearance.locates("heights.clearance_mm.x"));
    }

    #[test]
    fn leaving_a_field_unchanged_is_no_command() {
        let project = sample_project();
        for field in [
            Field::StockWidth,
            Field::StockHeight,
            Field::StockThickness,
            Field::Clearance,
            Field::FlattenTolerance,
            Field::FileExtension,
            Field::SpindleDwell,
            Field::DecimalsMm,
        ] {
            let text = field.text(&project);
            assert!(
                matches!(field.set(&project, &text), Ok(None)),
                "{field:?} = {text:?}"
            );
        }
    }

    #[test]
    fn seconds_take_an_s_and_refuse_a_comma() {
        assert!(parse_seconds("6 s").is_ok_and(|s| (s - 6.0).abs() < 1e-12));
        assert!(parse_seconds("1.5").is_ok_and(|s| (s - 1.5).abs() < 1e-12));
        assert!(parse_seconds("1,5").is_err());
        assert!(parse_seconds("soon").is_err());
    }
}
