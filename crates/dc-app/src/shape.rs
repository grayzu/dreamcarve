// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A drawn shape's sizes (`docs/specs/phase-5/design.md`, "Sizes as
//! placed, and stretching"; functional capability 84; ADR-0006): the
//! Properties panel's **Shape** section, a size typed there, and what a
//! stretch does to a shape that keeps its sizes.
//!
//! A definition's `shape` holds the sizes it was drawn with, in
//! definition space. The section shows them as the shape is placed: a
//! length times the instance's scale, so a shape scaled to twice its size
//! reads twice its drawn sizes, and a typed length is divided by it
//! ([`section`], [`set_size`]). A size is written with the outline it
//! gives, as one `SetOutline`, so every linked copy changes with it and
//! the outline in the file is always the one its sizes give.
//!
//! A stretch (a scale that is not the same along X and Y) cannot be kept
//! in a placement: it would make a rounded corner an ellipse and leave
//! the sizes meaning nothing. A rectangle, an ellipse, a slot and a
//! circle take it as new sizes instead ([`stretch`]); every other shape
//! that keeps sizes refuses it and scales evenly only.

use dc_geometry::shapes::gear::{Gear, GearCrest, GearFlanks, GearProblem, GearRoot, GearSize};
use dc_geometry::{Point, Polyline, Transform};
use dc_project::{
    Corner, FeatureInstance, InstanceId, MAX_POLYGON_SIDES, MIN_POLYGON_SIDES, Measured, Project,
    Shape, Units,
};

use crate::command::{Command, Edit, find_instance};
use crate::error::CommandError;
use crate::prefs::default_corner_mm;
use crate::strings::{StrKey, Strings};
use crate::tool::{
    Choice, ChoiceOption, CornerStyle, Quantity, SizeField, SizeRow, ToolOption, field_label,
    parse_angle_deg, polygon_name, size_text,
};
use crate::units;

/// How far a placement's two axes may differ in length or lean, as a
/// share of their length, and still be an even scale.
const EVEN_TOLERANCE: f64 = 1e-9;
/// A typed size within this of the one the shape has changes nothing:
/// millimetres or degrees.
const NO_CHANGE: f64 = 1e-9;
/// The shortest a stretched slot's centreline may be, mm.
const MIN_SLOT_LENGTH_MM: f64 = 1e-6;

/// The Properties panel's Shape section for one instance.
#[derive(Debug, Clone, PartialEq)]
pub struct Section {
    /// What the shape is: *Rectangle*, *Hexagon*.
    pub title: String,
    /// Its sizes, in project units, as it is placed.
    pub rows: Vec<SizeRow>,
    /// Its choices: a rectangle's corners, what a polygon's radius
    /// reaches, a gear's forms.
    pub choices: Vec<Choice>,
    /// A line that is read, not typed: a gear's module, or why the sizes
    /// are read-only.
    pub note: Option<String>,
    /// The sizes cannot be typed: the shape or a linked copy of it is
    /// locked, or its placement is not an even scale.
    pub read_only: bool,
}

/// The scale of `transform` when it scales X and Y alike (turned and
/// mirrored or not): the factor a drawn length is placed by, exactly 1
/// for a placement that only moves, turns or mirrors. `None` for a
/// stretch or a shear.
#[must_use]
pub fn even_scale(transform: Transform) -> Option<f64> {
    let [a, b, c, d, _, _] = transform.coeffs();
    let (x_squared, y_squared) = (a * a + b * b, c * c + d * d);
    let size = x_squared.max(y_squared);
    let even = (x_squared - y_squared).abs() <= EVEN_TOLERANCE * size
        && (a * c + b * d).abs() <= EVEN_TOLERANCE * size;
    (even && size.is_finite() && size > 0.0).then(|| {
        let scale = x_squared.sqrt();
        // A turn alone is a scale of 1 but for its sines' last digits:
        // a size typed for a turned shape is saved as it was typed.
        if (scale - 1.0).abs() <= EVEN_TOLERANCE {
            1.0
        } else {
            scale
        }
    })
}

/// The sizes `shape` has, in the section's order; none for a path, and
/// for a text until the Text tool is here.
fn fields(shape: &Shape) -> Vec<SizeField> {
    match shape {
        Shape::Rectangle { corner, .. } => {
            let mut fields = vec![SizeField::Width, SizeField::Height];
            if *corner != Corner::Square {
                fields.push(SizeField::CornerSize);
            }
            fields
        }
        Shape::Circle { .. } => vec![SizeField::Diameter],
        Shape::Ellipse { .. } => vec![SizeField::Width, SizeField::Height],
        Shape::Polygon { .. } => vec![SizeField::Sides, SizeField::Radius],
        Shape::Slot { .. } => vec![SizeField::Length, SizeField::SlotWidth],
        Shape::Gear { .. } => shape
            .gear()
            .map(|gear| gear_fields(&gear))
            .unwrap_or_default(),
        Shape::Text { .. } | Shape::Path { .. } => Vec::new(),
    }
}

/// A gear's sizes, in the bar's and the section's order; a size shows
/// only while the gear's form reads it.
pub(crate) fn gear_fields(gear: &Gear) -> Vec<SizeField> {
    let mut fields = vec![SizeField::Teeth, SizeField::Diameter];
    if gear.flanks == GearFlanks::Involute {
        fields.push(SizeField::PressureAngle);
    }
    fields.push(SizeField::WholeDepth);
    if gear.root == GearRoot::Rounded {
        fields.push(SizeField::RootFillet);
    }
    if gear.crest == GearCrest::Rounded {
        fields.push(SizeField::CrestRadius);
    }
    fields.push(SizeField::Bore);
    fields
}

/// What `field` of `shape` is: millimetres in definition space, degrees
/// or a count. `None` for a size the shape does not have, and for the
/// bore of a gear with none.
fn size_of(shape: &Shape, field: SizeField) -> Option<f64> {
    match (shape, field) {
        (Shape::Rectangle { width_mm, .. } | Shape::Ellipse { width_mm, .. }, SizeField::Width)
        | (Shape::Slot { width_mm, .. }, SizeField::SlotWidth) => Some(*width_mm),
        (
            Shape::Rectangle { height_mm, .. } | Shape::Ellipse { height_mm, .. },
            SizeField::Height,
        ) => Some(*height_mm),
        (Shape::Rectangle { corner, .. }, SizeField::CornerSize) => match *corner {
            Corner::Square => None,
            Corner::Rounded { radius_mm } => Some(radius_mm),
            Corner::Chamfered { distance_mm } => Some(distance_mm),
        },
        (Shape::Circle { diameter_mm }, SizeField::Diameter) => Some(*diameter_mm),
        (Shape::Polygon { sides, .. }, SizeField::Sides) => Some(f64::from(*sides)),
        (Shape::Polygon { radius_mm, .. }, SizeField::Radius) => Some(*radius_mm),
        (Shape::Slot { length_mm, .. }, SizeField::Length) => Some(*length_mm),
        (Shape::Gear { .. }, field) => gear_size(&shape.gear()?, field),
        _ => None,
    }
}

/// What `field` of `gear` is; `None` for a size a gear does not have and
/// for the bore of a gear with none.
pub(crate) fn gear_size(gear: &Gear, field: SizeField) -> Option<f64> {
    match field {
        SizeField::Teeth => Some(f64::from(gear.teeth)),
        SizeField::Diameter => Some(gear.outside_radius_mm * 2.0),
        SizeField::PressureAngle => Some(gear.pressure_angle_deg),
        SizeField::WholeDepth => Some(gear.whole_depth_mm),
        SizeField::RootFillet => Some(gear.root_fillet_mm),
        SizeField::CrestRadius => Some(gear.crest_radius_mm),
        SizeField::Bore => gear.bore_diameter_mm,
        _ => None,
    }
}

/// `gear` with `field` set to `value`: millimetres, degrees or a count.
/// A bore of nothing, or of 0, is no bore.
pub(crate) fn gear_with(mut gear: Gear, field: SizeField, value: Option<f64>) -> Gear {
    let Some(value) = value else {
        if field == SizeField::Bore {
            gear.bore_diameter_mm = None;
        }
        return gear;
    };
    match field {
        SizeField::Teeth => gear.teeth = count(value),
        SizeField::Diameter => gear.outside_radius_mm = value / 2.0,
        SizeField::PressureAngle => gear.pressure_angle_deg = value,
        SizeField::WholeDepth => gear.whole_depth_mm = value,
        SizeField::RootFillet => gear.root_fillet_mm = value,
        SizeField::CrestRadius => gear.crest_radius_mm = value,
        SizeField::Bore => gear.bore_diameter_mm = (value.abs() > 0.0).then_some(value),
        _ => {}
    }
    gear
}

/// A count read by [`read_size`], as the whole number it is.
fn count(value: f64) -> u32 {
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "read from a u32 by `read_size`; anything else saturates"
    )]
    let count = value.round().max(0.0) as u32;
    count
}

/// `corner_mm` held to half the shorter side of a `width_mm` ×
/// `height_mm` rectangle, where the corners of a short side meet.
fn held(corner_mm: f64, width_mm: f64, height_mm: f64) -> f64 {
    corner_mm.min(width_mm.min(height_mm) / 2.0)
}

/// `corner` with its size held to the rectangle's sides.
fn held_corner(corner: Corner, width_mm: f64, height_mm: f64) -> Corner {
    match corner {
        Corner::Square => Corner::Square,
        Corner::Rounded { radius_mm } => Corner::Rounded {
            radius_mm: held(radius_mm, width_mm, height_mm),
        },
        Corner::Chamfered { distance_mm } => Corner::Chamfered {
            distance_mm: held(distance_mm, width_mm, height_mm),
        },
    }
}

/// `shape` with `field` set to `value` (millimetres in definition space,
/// degrees or a count; `None` for a bore taken away). `None` for a size
/// the shape does not have.
fn with_size(shape: &Shape, field: SizeField, value: Option<f64>) -> Option<Shape> {
    if let Shape::Gear { .. } = shape {
        if !gear_fields(&shape.gear()?).contains(&field) {
            return None;
        }
        return Some(Shape::from_gear(&gear_with(shape.gear()?, field, value)));
    }
    let value = value?;
    Some(match (shape.clone(), field) {
        (
            Shape::Rectangle {
                height_mm, corner, ..
            },
            SizeField::Width,
        ) => Shape::Rectangle {
            width_mm: value,
            height_mm,
            corner: held_corner(corner, value, height_mm),
        },
        (
            Shape::Rectangle {
                width_mm, corner, ..
            },
            SizeField::Height,
        ) => Shape::Rectangle {
            width_mm,
            height_mm: value,
            corner: held_corner(corner, width_mm, value),
        },
        (
            Shape::Rectangle {
                width_mm,
                height_mm,
                corner,
            },
            SizeField::CornerSize,
        ) => Shape::Rectangle {
            width_mm,
            height_mm,
            corner: match corner {
                Corner::Square => return None,
                Corner::Rounded { .. } => Corner::Rounded {
                    radius_mm: held(value, width_mm, height_mm),
                },
                Corner::Chamfered { .. } => Corner::Chamfered {
                    distance_mm: held(value, width_mm, height_mm),
                },
            },
        },
        (Shape::Circle { .. }, SizeField::Diameter) => Shape::Circle { diameter_mm: value },
        (Shape::Ellipse { height_mm, .. }, SizeField::Width) => Shape::Ellipse {
            width_mm: value,
            height_mm,
        },
        (Shape::Ellipse { width_mm, .. }, SizeField::Height) => Shape::Ellipse {
            width_mm,
            height_mm: value,
        },
        (
            Shape::Polygon {
                radius_mm,
                measured,
                ..
            },
            SizeField::Sides,
        ) => Shape::Polygon {
            sides: count(value),
            radius_mm,
            measured,
        },
        (
            Shape::Polygon {
                sides, measured, ..
            },
            SizeField::Radius,
        ) => Shape::Polygon {
            sides,
            radius_mm: value,
            measured,
        },
        (Shape::Slot { width_mm, .. }, SizeField::Length) => Shape::Slot {
            length_mm: value,
            width_mm,
        },
        (Shape::Slot { length_mm, .. }, SizeField::SlotWidth) => Shape::Slot {
            length_mm,
            width_mm: value,
        },
        _ => return None,
    })
}

/// What a kind of shape is called, as its drawing tool names it.
fn title(shape: &Shape) -> StrKey {
    match shape {
        Shape::Rectangle { .. } => StrKey::DrawNameRectangle,
        Shape::Circle { .. } => StrKey::DrawNameCircle,
        Shape::Ellipse { .. } => StrKey::DrawNameEllipse,
        Shape::Polygon { sides, .. } => polygon_name(*sides),
        Shape::Slot { .. } => StrKey::DrawNameSlot,
        Shape::Gear { .. } => StrKey::DrawNameGear,
        Shape::Text { .. } | Shape::Path { .. } => StrKey::DrawNamePath,
    }
}

/// A row's label in the section: the tool bar's, but for a width and a
/// height, which Properties has beside the placement's W and H and so
/// writes in full, and a gear's diameter, which is its outside one.
const fn row_label(shape: &Shape, field: SizeField) -> StrKey {
    match (shape, field) {
        (Shape::Slot { .. }, _) => field_label(field),
        (_, SizeField::Width) => StrKey::ShapeFieldWidth,
        (_, SizeField::Height) => StrKey::ShapeFieldHeight,
        (Shape::Gear { .. }, SizeField::Diameter) => StrKey::DrawFieldOutsideDiameter,
        _ => field_label(field),
    }
}

/// The word an undo label has for a change: *Set **width** of “Slot 1”*.
///
/// Only a size or a choice is worded this way; a node edit's label is its
/// own sentence ([`crate::command::Command::label`]), and reads *size*
/// here.
pub(crate) const fn edit_word(shape: Option<&Shape>, edit: Edit) -> StrKey {
    match edit {
        Edit::MoveNodes { .. }
        | Edit::Reshape
        | Edit::AddNode
        | Edit::DeleteNodes { .. }
        | Edit::Break
        | Edit::DeleteSegment
        | Edit::CloseEnds
        | Edit::JoinEnds
        | Edit::Offset
        | Edit::Weld
        | Edit::Subtract
        | Edit::Intersect
        | Edit::Trim
        | Edit::Join
        | Edit::Fillet { .. }
        | Edit::Chamfer { .. }
        | Edit::SegmentKind(_) => StrKey::ShapeSizeSize,
        Edit::Size(field) => match field {
            SizeField::Width | SizeField::SlotWidth => StrKey::ShapeSizeWidth,
            SizeField::Height => StrKey::ShapeSizeHeight,
            SizeField::CornerSize => StrKey::ShapeSizeCornerSize,
            SizeField::Diameter => match shape {
                Some(Shape::Gear { .. }) => StrKey::ShapeSizeOutsideDiameter,
                _ => StrKey::ShapeSizeDiameter,
            },
            SizeField::Sides => StrKey::ShapeSizeSides,
            SizeField::Radius => StrKey::ShapeSizeRadius,
            SizeField::Length => StrKey::ShapeSizeLength,
            SizeField::Teeth => StrKey::ShapeSizeTeeth,
            SizeField::PressureAngle => StrKey::ShapeSizePressureAngle,
            SizeField::WholeDepth => StrKey::ShapeSizeWholeDepth,
            SizeField::RootFillet => StrKey::ShapeSizeRootFillet,
            SizeField::CrestRadius => StrKey::ShapeSizeCrestRadius,
            SizeField::Bore => StrKey::ShapeSizeBore,
            SizeField::X
            | SizeField::Y
            | SizeField::Angle
            | SizeField::Distance
            | SizeField::Tolerance
            | SizeField::Columns
            | SizeField::Rows
            | SizeField::SpacingX
            | SizeField::SpacingY
            | SizeField::Copies => StrKey::ShapeSizeSize,
        },
        Edit::Corners => StrKey::ShapeSizeCorners,
        Edit::Measured => StrKey::ShapeSizeMeasure,
        Edit::Flanks => StrKey::ShapeSizeFlanks,
        Edit::Root => StrKey::ShapeSizeRoot,
        Edit::Crest => StrKey::ShapeSizeCrest,
    }
}

/// One choice of a tool bar or a section: its label, its options and
/// the one that is chosen.
fn choice(
    strings: &Strings,
    (label, help): (StrKey, StrKey),
    options: &[(StrKey, ToolOption)],
    chosen: &ToolOption,
) -> Choice {
    Choice {
        label: strings.get(label).to_owned(),
        help: strings.get(help).to_owned(),
        selected: options
            .iter()
            .position(|(_, option)| option == chosen)
            .unwrap_or(0),
        options: options
            .iter()
            .map(|(label, option)| ChoiceOption {
                label: strings.get(*label).to_owned(),
                option: option.clone(),
            })
            .collect(),
    }
}

/// A gear's forms as choices: its flanks, its root and its crest.
pub(crate) fn gear_choices(strings: &Strings, gear: &Gear) -> Vec<Choice> {
    vec![
        choice(
            strings,
            (StrKey::DrawChoiceFlanks, StrKey::DrawChoiceFlanksHelp),
            &[
                (
                    StrKey::DrawChoiceFlanksInvolute,
                    ToolOption::Flanks(GearFlanks::Involute),
                ),
                (
                    StrKey::DrawChoiceFlanksStraight,
                    ToolOption::Flanks(GearFlanks::Straight),
                ),
            ],
            &ToolOption::Flanks(gear.flanks),
        ),
        choice(
            strings,
            (StrKey::DrawChoiceRoot, StrKey::DrawChoiceRootHelp),
            &[
                (
                    StrKey::DrawChoiceRootRounded,
                    ToolOption::Root(GearRoot::Rounded),
                ),
                (StrKey::DrawChoiceRootFlat, ToolOption::Root(GearRoot::Flat)),
                (
                    StrKey::DrawChoiceRootSharp,
                    ToolOption::Root(GearRoot::Sharp),
                ),
            ],
            &ToolOption::Root(gear.root),
        ),
        choice(
            strings,
            (StrKey::DrawChoiceCrest, StrKey::DrawChoiceCrestHelp),
            &[
                (
                    StrKey::DrawChoiceCrestFlat,
                    ToolOption::Crest(GearCrest::Flat),
                ),
                (
                    StrKey::DrawChoiceCrestRounded,
                    ToolOption::Crest(GearCrest::Rounded),
                ),
            ],
            &ToolOption::Crest(gear.crest),
        ),
    ]
}

/// A rectangle's corners as a choice: square, rounded or chamfered.
pub(crate) fn corners_choice(strings: &Strings, chosen: CornerStyle) -> Choice {
    choice(
        strings,
        (StrKey::DrawChoiceCorners, StrKey::DrawChoiceCornersHelp),
        &[
            (
                StrKey::DrawChoiceCornersSquare,
                ToolOption::Corners(CornerStyle::Square),
            ),
            (
                StrKey::DrawChoiceCornersRounded,
                ToolOption::Corners(CornerStyle::Rounded),
            ),
            (
                StrKey::DrawChoiceCornersChamfered,
                ToolOption::Corners(CornerStyle::Chamfered),
            ),
        ],
        &ToolOption::Corners(chosen),
    )
}

/// The drawing tools' Construction switch as a choice: off or on.
pub(crate) fn construction_choice(strings: &Strings, on: bool) -> Choice {
    choice(
        strings,
        (
            StrKey::DrawChoiceConstruction,
            StrKey::DrawChoiceConstructionHelp,
        ),
        &[
            (
                StrKey::DrawChoiceConstructionOff,
                ToolOption::Construction(false),
            ),
            (
                StrKey::DrawChoiceConstructionOn,
                ToolOption::Construction(true),
            ),
        ],
        &ToolOption::Construction(on),
    )
}

/// What a polygon's radius reaches as a choice: its corners or its
/// flats.
pub(crate) fn measure_choice(strings: &Strings, measured: Measured) -> Choice {
    choice(
        strings,
        (StrKey::DrawChoiceMeasure, StrKey::DrawChoiceMeasureHelp),
        &[
            (
                StrKey::DrawChoiceMeasureCorners,
                ToolOption::Measured(Measured::Corners),
            ),
            (
                StrKey::DrawChoiceMeasureFlats,
                ToolOption::Measured(Measured::Flats),
            ),
        ],
        &ToolOption::Measured(measured),
    )
}

/// The choices `shape` has.
fn choices(strings: &Strings, shape: &Shape) -> Vec<Choice> {
    match shape {
        Shape::Rectangle { corner, .. } => vec![corners_choice(
            strings,
            match corner {
                Corner::Square => CornerStyle::Square,
                Corner::Rounded { .. } => CornerStyle::Rounded,
                Corner::Chamfered { .. } => CornerStyle::Chamfered,
            },
        )],
        Shape::Polygon { measured, .. } => vec![measure_choice(strings, *measured)],
        Shape::Gear { .. } => shape
            .gear()
            .map(|gear| gear_choices(strings, &gear))
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// A gear's module as the note under its sizes, `scale` times as it is
/// placed; none while the sizes give no module.
pub(crate) fn module_note(
    strings: &Strings,
    units: Units,
    gear: &Gear,
    scale: f64,
) -> Option<String> {
    let module = gear.module_mm() * scale;
    (module.is_finite() && module > 0.0).then(|| {
        strings.fmt(
            StrKey::DrawNoteModule,
            &[("module", &units::summary_text(module, units))],
        )
    })
}

/// Why a gear cannot be, a limit in it in project units and `scale`
/// times as the gear is placed; upstream's words
/// (`validateGearProfileParams`).
pub(crate) fn gear_problem_text(
    strings: &Strings,
    units: Units,
    problem: &GearProblem,
    scale: f64,
) -> String {
    let limit = |mm: f64| units::limit_text(mm * scale, units);
    match *problem {
        GearProblem::RootFilletTooLarge { max_mm } => strings.fmt(
            StrKey::DrawProblemRootFilletTooLarge,
            &[("max", &limit(max_mm))],
        ),
        GearProblem::CrestRadiusTooLarge { max_mm } => strings.fmt(
            StrKey::DrawProblemCrestRadiusTooLarge,
            &[("max", &limit(max_mm))],
        ),
        GearProblem::BoreTooLarge { root_diameter_mm } => strings.fmt(
            StrKey::DrawProblemBoreTooLarge,
            &[("root", &limit(root_diameter_mm))],
        ),
        other => other.to_string(),
    }
}

/// The field a gear's problem is said at.
pub(crate) const fn gear_problem_field(size: GearSize) -> SizeField {
    match size {
        GearSize::Outside => SizeField::Diameter,
        GearSize::Teeth => SizeField::Teeth,
        GearSize::WholeDepth => SizeField::WholeDepth,
        GearSize::PressureAngle => SizeField::PressureAngle,
        GearSize::RootFillet => SizeField::RootFillet,
        GearSize::CrestRadius => SizeField::CrestRadius,
        GearSize::Bore => SizeField::Bore,
    }
}

/// What `text` typed in `field` is, millimetres, degrees or a count;
/// else why it is none of them.
pub(crate) fn read_size(
    strings: &Strings,
    units: Units,
    field: SizeField,
    text: &str,
) -> Result<f64, String> {
    let text = text.trim();
    match field.quantity() {
        Quantity::Length => units::parse_length_mm(text, units).map_err(|e| e.to_string()),
        Quantity::Angle => parse_angle_deg(text)
            .ok_or_else(|| strings.fmt(StrKey::DrawProblemNotAnAngle, &[("text", text)])),
        Quantity::Count => text
            .parse::<u32>()
            .map(f64::from)
            .map_err(|_| strings.fmt(StrKey::DrawProblemNotACount, &[("text", text)])),
    }
}

/// The instances that place `instance`'s definition, itself among them.
fn copies<'a>(project: &'a Project, instance: &FeatureInstance) -> Vec<&'a FeatureInstance> {
    project
        .instances
        .iter()
        .filter(|other| other.definition == instance.definition)
        .collect()
}

/// The Shape section for instance `id`: `None` for a shape that keeps no
/// sizes (an imported outline, a path) and for an id the project does not
/// have.
#[must_use]
pub fn section(project: &Project, strings: &Strings, id: &InstanceId) -> Option<Section> {
    let instance = find_instance(project, id).ok()?;
    let shape = project
        .definitions
        .get(&instance.definition)?
        .shape
        .as_ref()?;
    let fields = fields(shape);
    if fields.is_empty() {
        return None;
    }
    let units = project.display_units;
    let placed = even_scale(instance.transform);
    // A stretched shape reads the sizes it was drawn with.
    let scale = placed.unwrap_or(1.0);
    let rows = fields
        .into_iter()
        .map(|field| SizeRow {
            field,
            label: strings.get(row_label(shape, field)).to_owned(),
            help: strings.get(field.help()).to_owned(),
            text: size_of(shape, field).map_or_else(String::new, |value| {
                let shown = match field.quantity() {
                    Quantity::Length => value * scale,
                    Quantity::Angle | Quantity::Count => value,
                };
                size_text(field, shown, units)
            }),
            quantity: field.quantity(),
            pinned: false,
            problem: None,
            option: false,
        })
        .collect();
    let note = if placed.is_none() {
        Some(strings.get(StrKey::ShapeNoteStretched).to_owned())
    } else {
        shape
            .gear()
            .and_then(|gear| module_note(strings, units, &gear, scale))
    };
    Some(Section {
        title: strings.get(title(shape)).to_owned(),
        rows,
        choices: choices(strings, shape),
        note,
        read_only: placed.is_none() || copies(project, instance).iter().any(|copy| copy.locked),
    })
}

/// A refusal of `text`, said at its field.
fn refused(text: &str, reason: impl Into<String>) -> CommandError {
    CommandError::NotAQuantity {
        text: text.trim().to_owned(),
        reason: reason.into(),
    }
}

/// What is wrong with `shape`'s sizes, when something is; a limit in it
/// `scale` times as the shape is placed.
fn problem(strings: &Strings, units: Units, shape: &Shape, scale: f64) -> Option<String> {
    match shape {
        Shape::Polygon { sides, .. }
            if !(MIN_POLYGON_SIDES..=MAX_POLYGON_SIDES).contains(sides) =>
        {
            Some(strings.fmt(
                StrKey::ShapeProblemSides,
                &[
                    ("min", &MIN_POLYGON_SIDES.to_string()),
                    ("max", &MAX_POLYGON_SIDES.to_string()),
                ],
            ))
        }
        Shape::Gear { .. } => shape
            .gear()?
            .problems()
            .first()
            .map(|problem| gear_problem_text(strings, units, problem, scale)),
        _ => None,
    }
}

/// The `SetOutline` that makes instance `id`'s shape `shape`, with the
/// outline it gives.
fn outline_command(
    project: &Project,
    id: &InstanceId,
    shape: Shape,
    what: Edit,
) -> Result<Command, CommandError> {
    let profiles = shape
        .outline(project.flatten_tolerance_mm)
        .and_then(Result::ok)
        .ok_or(CommandError::EmptyOutline)?;
    Ok(Command::SetOutline {
        instance: id.clone(),
        shape: Some(shape),
        profiles,
        what,
    })
}

/// The command typing `text` into the section's `field` makes for
/// instance `id`: one `SetOutline`, the size divided by the instance's
/// scale.
///
/// # Errors
///
/// [`CommandError::NothingToDo`] for a size the shape does not have or
/// has already; [`CommandError::NotAQuantity`] with the reason when the
/// text is no size or the shape cannot have it;
/// [`CommandError::Stretched`] for a shape whose placement is not an even
/// scale. A locked shape is refused when the command is applied.
pub fn set_size(
    project: &Project,
    strings: &Strings,
    id: &InstanceId,
    field: SizeField,
    text: &str,
) -> Result<Command, CommandError> {
    let instance = find_instance(project, id)?;
    let shape = project
        .definitions
        .get(&instance.definition)
        .and_then(|definition| definition.shape.as_ref())
        .ok_or(CommandError::NothingToDo)?;
    if !fields(shape).contains(&field) {
        return Err(CommandError::NothingToDo);
    }
    let scale = even_scale(instance.transform).ok_or_else(|| CommandError::Stretched {
        name: instance.name.clone(),
    })?;
    let units = project.display_units;
    // A bore with nothing typed is no bore.
    let value = if field == SizeField::Bore && text.trim().is_empty() {
        None
    } else {
        let typed = read_size(strings, units, field, text).map_err(|why| refused(text, why))?;
        Some(match field.quantity() {
            Quantity::Length => typed / scale,
            Quantity::Angle | Quantity::Count => typed,
        })
    };
    // A gear's sizes are the gear's to refuse, each with its own reason.
    let gear = matches!(shape, Shape::Gear { .. });
    if !gear && value.is_some_and(|value| field.quantity() == Quantity::Length && value <= 0.0) {
        return Err(refused(text, strings.get(StrKey::DrawProblemNotPositive)));
    }
    let was = size_of(shape, field);
    let same = match (was, value) {
        (None, None) => true,
        (Some(was), Some(value)) => (was - value).abs() <= NO_CHANGE,
        // A bore of 0 where there is none.
        (None, Some(value)) => gear && value.abs() <= NO_CHANGE,
        (Some(_), None) => false,
    };
    if same {
        return Err(CommandError::NothingToDo);
    }
    let next = with_size(shape, field, value).ok_or(CommandError::NothingToDo)?;
    if let Some(why) = problem(strings, units, &next, scale) {
        return Err(refused(text, why));
    }
    outline_command(project, id, next, Edit::Size(field))
}

/// The command choosing `option` in the section makes for instance `id`.
///
/// # Errors
///
/// [`CommandError::NothingToDo`] for a choice the shape does not have or
/// has already; [`CommandError::NotAQuantity`] with the reason when the
/// shape cannot be that.
pub fn set_option(
    project: &Project,
    strings: &Strings,
    id: &InstanceId,
    option: &ToolOption,
) -> Result<Command, CommandError> {
    let instance = find_instance(project, id)?;
    let shape = project
        .definitions
        .get(&instance.definition)
        .and_then(|definition| definition.shape.as_ref())
        .ok_or(CommandError::NothingToDo)?;
    let units = project.display_units;
    let scale = even_scale(instance.transform).ok_or_else(|| CommandError::Stretched {
        name: instance.name.clone(),
    })?;
    let (next, what) = match (shape, option) {
        (
            Shape::Rectangle {
                width_mm,
                height_mm,
                corner,
            },
            ToolOption::Corners(style),
        ) => {
            // The size the corners have, or the tool's default, as the
            // rectangle is placed.
            let size = held(
                size_of(shape, SizeField::CornerSize)
                    .unwrap_or_else(|| default_corner_mm(units) / scale),
                *width_mm,
                *height_mm,
            );
            let next = match style {
                CornerStyle::Square => Corner::Square,
                CornerStyle::Rounded => Corner::Rounded { radius_mm: size },
                CornerStyle::Chamfered => Corner::Chamfered { distance_mm: size },
            };
            if std::mem::discriminant(&next) == std::mem::discriminant(corner) {
                return Err(CommandError::NothingToDo);
            }
            (
                Shape::Rectangle {
                    width_mm: *width_mm,
                    height_mm: *height_mm,
                    corner: next,
                },
                Edit::Corners,
            )
        }
        (
            Shape::Polygon {
                sides,
                radius_mm,
                measured,
            },
            ToolOption::Measured(to),
        ) => {
            if measured == to {
                return Err(CommandError::NothingToDo);
            }
            // The polygon stays as it is: its radius is read to where
            // it now reaches. A side's middle is cos(π/n) of a corner's
            // distance from the centre.
            let ratio = (std::f64::consts::PI / f64::from(*sides)).cos();
            let radius_mm = match to {
                Measured::Corners => radius_mm / ratio,
                Measured::Flats => radius_mm * ratio,
            };
            (
                Shape::Polygon {
                    sides: *sides,
                    radius_mm,
                    measured: *to,
                },
                Edit::Measured,
            )
        }
        (
            Shape::Gear { .. },
            ToolOption::Flanks(_) | ToolOption::Root(_) | ToolOption::Crest(_),
        ) => {
            let mut gear = shape.gear().ok_or(CommandError::NothingToDo)?;
            let was = gear;
            let what = match *option {
                ToolOption::Flanks(flanks) => {
                    gear.flanks = flanks;
                    Edit::Flanks
                }
                ToolOption::Root(root) => {
                    gear.root = root;
                    Edit::Root
                }
                ToolOption::Crest(crest) => {
                    gear.crest = crest;
                    Edit::Crest
                }
                _ => return Err(CommandError::NothingToDo),
            };
            if gear == was {
                return Err(CommandError::NothingToDo);
            }
            (Shape::from_gear(&gear), what)
        }
        _ => return Err(CommandError::NothingToDo),
    };
    if let Some(why) = problem(strings, units, &next, scale) {
        return Err(refused("", why));
    }
    outline_command(project, id, next, what)
}

/// A shape that took a stretch as new sizes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Stretched {
    /// The shape with its new sizes.
    pub(crate) shape: Shape,
    /// The outline they give.
    pub(crate) profiles: Vec<Polyline>,
    /// The instance's placement: as it was, but for where its centre is.
    pub(crate) transform: Transform,
}

/// What `instance` makes of a stretch: `delta`, which scales X and Y
/// differently, applied as `change` (the same about the selection's
/// centre). `None` for a shape that keeps no sizes, which is stretched
/// through its placement as it always was.
///
/// # Errors
///
/// [`CommandError::StretchRefused`] for a polygon, a gear or a text,
/// which keep their proportions, for a shape that is not square to the
/// axes, and for a stretch that is not along them;
/// [`CommandError::StretchLinked`] for a linked copy, whose shape is not
/// its own.
pub(crate) fn stretch(
    project: &Project,
    instance: &FeatureInstance,
    delta: Transform,
    change: Transform,
) -> Result<Option<Stretched>, CommandError> {
    let Some(shape) = project
        .definitions
        .get(&instance.definition)
        .and_then(|definition| definition.shape.as_ref())
    else {
        return Ok(None);
    };
    let refuse = |reason: &'static str| CommandError::StretchRefused {
        name: instance.name.clone(),
        reason,
    };
    match shape {
        Shape::Path { .. } => return Ok(None),
        Shape::Polygon { .. } => return Err(refuse("a polygon keeps its proportions")),
        Shape::Gear { .. } => return Err(refuse("a gear keeps its proportions")),
        Shape::Text { .. } => return Err(refuse("a text keeps its proportions")),
        Shape::Rectangle { .. }
        | Shape::Circle { .. }
        | Shape::Ellipse { .. }
        | Shape::Slot { .. } => {}
    }
    let others = copies(project, instance).len().saturating_sub(1);
    if others > 0 {
        return Err(CommandError::StretchLinked {
            name: instance.name.clone(),
            others,
        });
    }
    // The stretch is along X and Y, and so are the shape's own axes.
    let [sx, lean_y, lean_x, sy, _, _] = delta.coeffs();
    let size = sx.abs().max(sy.abs());
    if lean_x.abs() > EVEN_TOLERANCE * size || lean_y.abs() > EVEN_TOLERANCE * size {
        return Err(refuse("it stretches along X or Y only"));
    }
    let [a, b, _, _, _, _] = instance.transform.coeffs();
    let scale = even_scale(instance.transform).ok_or_else(|| refuse("it is stretched already"))?;
    // Where the shape's own X axis points: along X, or along Y.
    let (along, across) = if b.abs() <= EVEN_TOLERANCE * scale {
        (sx.abs(), sy.abs())
    } else if a.abs() <= EVEN_TOLERANCE * scale {
        (sy.abs(), sx.abs())
    } else {
        return Err(refuse("it is not square to the axes"));
    };
    let next = match *shape {
        Shape::Rectangle {
            width_mm,
            height_mm,
            corner,
        } => {
            let (width_mm, height_mm) = (width_mm * along, height_mm * across);
            Shape::Rectangle {
                width_mm,
                height_mm,
                corner: held_corner(corner, width_mm, height_mm),
            }
        }
        Shape::Circle { diameter_mm } => Shape::Ellipse {
            width_mm: diameter_mm * along,
            height_mm: diameter_mm * across,
        },
        Shape::Ellipse {
            width_mm,
            height_mm,
        } => Shape::Ellipse {
            width_mm: width_mm * along,
            height_mm: height_mm * across,
        },
        Shape::Slot {
            length_mm,
            width_mm,
        } => {
            // Its box is its length and width along, its width across:
            // the width takes the stretch across, and the centreline what
            // is left of the box along.
            let across_mm = width_mm * across;
            let along_mm = (length_mm + width_mm) * along - across_mm;
            if along_mm < MIN_SLOT_LENGTH_MM {
                return Err(refuse("a slot is longer than it is wide"));
            }
            Shape::Slot {
                length_mm: along_mm,
                width_mm: across_mm,
            }
        }
        _ => return Ok(None),
    };
    let profiles = next
        .outline(project.flatten_tolerance_mm)
        .and_then(Result::ok)
        .ok_or(CommandError::EmptyOutline)?;
    // Each of these is centred on its definition's origin and the same
    // either side of both axes, so its box goes where the stretch puts it
    // when its centre does.
    let centre = instance.transform.apply(Point::new(0.0, 0.0));
    let moved = change.apply(centre);
    Ok(Some(Stretched {
        shape: next,
        profiles,
        transform: instance
            .transform
            .then(Transform::translate(moved.x - centre.x, moved.y - centre.y)),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::FRAC_PI_3;

    #[test]
    fn an_even_scale_is_read_through_a_turn_and_a_mirror() {
        let near = |got: Option<f64>, want: f64| got.is_some_and(|k| (k - want).abs() < 1e-12);
        assert!(near(even_scale(Transform::IDENTITY), 1.0));
        assert!(near(even_scale(Transform::scale(2.0, 2.0)), 2.0));
        assert!(near(
            even_scale(Transform::rotate(FRAC_PI_3).then(Transform::scale(0.5, 0.5))),
            0.5
        ));
        assert!(near(even_scale(Transform::scale(-3.0, 3.0)), 3.0));
        assert!(near(
            even_scale(Transform::translate(40.0, -7.0).then(Transform::rotate(1.0))),
            1.0
        ));
        assert_eq!(even_scale(Transform::scale(2.0, 1.0)), None);
        assert_eq!(
            even_scale(Transform::from([1.0, 0.0, 0.5, 1.0, 0.0, 0.0])),
            None
        );
    }

    #[test]
    fn a_corner_is_held_to_half_the_shorter_side() {
        assert_eq!(held(5.0, 60.0, 30.0).to_bits(), 5.0f64.to_bits());
        assert_eq!(held(40.0, 60.0, 30.0).to_bits(), 15.0f64.to_bits());
        assert_eq!(
            held_corner(Corner::Chamfered { distance_mm: 9.0 }, 10.0, 80.0),
            Corner::Chamfered { distance_mm: 5.0 }
        );
        assert_eq!(held_corner(Corner::Square, 1.0, 1.0), Corner::Square);
    }
}
