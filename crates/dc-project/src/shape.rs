// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! How a drawn shape's outline was made (format 29; ADR-0006;
//! `docs/specs/phase-5/design.md`, "Data model → dc-project").
//!
//! A [`crate::FeatureDefinition`]'s `profiles` are what is cut. Its
//! `shape` says how the app made them, so the app can edit them by their
//! sizes: nothing in `dc-cam` or `dc-gcode` reads a `Shape`.
//!
//! Every shape lies in definition space, millimetres, Y-up (ADR-0001),
//! centred on the origin unless its variant says otherwise; the
//! instance's transform places it.

use crate::units_input;
use dc_geometry::Path;
use serde::{Deserialize, Serialize};

/// The fewest sides a [`Shape::Polygon`] can have.
pub const MIN_POLYGON_SIDES: u32 = 3;
/// The most sides a [`Shape::Polygon`] can have (PureCutCNC's
/// `NGON_MAX_SIDES`).
pub const MAX_POLYGON_SIDES: u32 = 50;
/// The fewest teeth a [`Shape::Gear`] can have (PureCutCNC's
/// `gearProfile.ts`).
pub const MIN_GEAR_TEETH: u32 = 6;
/// The most teeth a [`Shape::Gear`] can have.
pub const MAX_GEAR_TEETH: u32 = 200;
/// The smallest pressure angle of a [`Shape::Gear`], degrees.
pub const MIN_PRESSURE_ANGLE_DEG: f64 = 10.0;
/// The largest pressure angle of a [`Shape::Gear`], degrees.
pub const MAX_PRESSURE_ANGLE_DEG: f64 = 35.0;

/// The sizes a shape was drawn with. `kind` names the variant in a file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Shape {
    /// A rectangle, its sides along X and Y.
    Rectangle {
        /// Extent along X, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        width_mm: f64,
        /// Extent along Y, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        height_mm: f64,
        /// How its four corners are finished.
        corner: Corner,
    },
    /// A circle.
    Circle {
        /// Its diameter, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        diameter_mm: f64,
    },
    /// An ellipse, its axes along X and Y.
    Ellipse {
        /// Its full extent along X, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        width_mm: f64,
        /// Its full extent along Y, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        height_mm: f64,
    },
    /// A regular polygon, its first corner on +X.
    Polygon {
        /// How many sides, [`MIN_POLYGON_SIDES`] to [`MAX_POLYGON_SIDES`].
        sides: u32,
        /// The radius `measured` says, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        radius_mm: f64,
        /// What `radius_mm` reaches.
        measured: Measured,
    },
    /// A slot: two straight sides and two half circles, its centreline
    /// along X (CONTEXT: Slot).
    Slot {
        /// From one end's centre to the other's, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        length_mm: f64,
        /// Across the slot, millimetres, above 0: the half circles'
        /// diameter.
        #[serde(deserialize_with = "units_input::mm")]
        width_mm: f64,
    },
    /// A spur gear, a tooth on +X (PureCutCNC's `GearProfileParams`).
    Gear {
        /// How many teeth, [`MIN_GEAR_TEETH`] to [`MAX_GEAR_TEETH`].
        teeth: u32,
        /// Across the tips of the teeth, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        outside_diameter_mm: f64,
        /// The pressure angle, degrees, [`MIN_PRESSURE_ANGLE_DEG`] to
        /// [`MAX_PRESSURE_ANGLE_DEG`].
        pressure_angle_deg: f64,
        /// The form of a tooth's sides.
        flanks: Flanks,
        /// From a tooth's tip to its root, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        whole_depth_mm: f64,
        /// The form of the gap's floor.
        root: Root,
        /// The radius of a rounded root's corners, millimetres, 0 or
        /// more.
        #[serde(deserialize_with = "units_input::mm")]
        root_fillet_mm: f64,
        /// The form of a tooth's tip.
        crest: Crest,
        /// The radius of a rounded crest's corners, millimetres, 0 or
        /// more.
        #[serde(deserialize_with = "units_input::mm")]
        crest_radius_mm: f64,
        /// The diameter of the centre hole, millimetres, above 0; `None`
        /// for a gear with no bore.
        #[serde(deserialize_with = "units_input::opt_mm")]
        bore_diameter_mm: Option<f64>,
    },
    /// Lines of text. The first line's baseline starts at the origin and
    /// runs along +X; later lines follow below it.
    Text {
        /// The words; a newline starts a line.
        text: String,
        /// The font the outline was made with.
        font: FontRef,
        /// The height of a capital letter, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        height_mm: f64,
        /// Space added between letters, as a fraction of `height_mm`;
        /// 0 is the font's own spacing.
        letter_spacing_fraction: f64,
        /// The distance from one baseline to the next, as a fraction of
        /// the font's own line height, above 0; 1 is the font's own.
        line_spacing_fraction: f64,
        /// How several lines are lined up.
        align: Align,
    },
    /// Outlines with curves, one path for each of the definition's
    /// `profiles`, which are these flattened. A definition whose outlines
    /// have no curve has no `shape`.
    Path {
        /// The paths, in the order of `profiles`.
        paths: Vec<Path>,
    },
}

impl Shape {
    /// The variant's name in a file, and in messages: `rectangle`,
    /// `slot`, …
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Rectangle { .. } => "rectangle",
            Self::Circle { .. } => "circle",
            Self::Ellipse { .. } => "ellipse",
            Self::Polygon { .. } => "polygon",
            Self::Slot { .. } => "slot",
            Self::Gear { .. } => "gear",
            Self::Text { .. } => "text",
            Self::Path { .. } => "path",
        }
    }
}

/// How a [`Shape::Rectangle`]'s corners are finished.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Corner {
    /// Sharp.
    Square,
    /// Rounded with an arc tangent to both sides.
    Rounded {
        /// The arc's radius, millimetres, above 0 and at most half the
        /// shorter side.
        #[serde(deserialize_with = "units_input::mm")]
        radius_mm: f64,
    },
    /// Cut off straight.
    Chamfered {
        /// How far along each side the cut starts, millimetres, above 0
        /// and at most half the shorter side.
        #[serde(deserialize_with = "units_input::mm")]
        distance_mm: f64,
    },
}

/// What a [`Shape::Polygon`]'s radius reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Measured {
    /// Its corners: the circle through them.
    Corners,
    /// The middle of its sides: the circle inside it, half a hexagon's
    /// spanner size.
    Flats,
}

/// The form of a gear tooth's sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Flanks {
    /// An involute of the base circle, as cut gears have.
    Involute,
    /// Straight from root to tip.
    Straight,
}

/// The form of the floor between two gear teeth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Root {
    /// Rounded into the flanks.
    Rounded,
    /// Flat, meeting the flanks at a corner.
    Flat,
    /// The flanks meet at a point.
    Sharp,
}

/// The form of a gear tooth's tip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Crest {
    /// Flat, on the outside circle.
    Flat,
    /// Rounded into the flanks.
    Rounded,
}

/// How several lines of a [`Shape::Text`] are lined up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    /// Every line starts at the origin's X.
    Left,
    /// Every line is centred on the widest line's middle.
    Centre,
    /// Every line ends where the widest line ends.
    Right,
}

/// Names a font by what its own name table says, so a project never
/// holds a path on someone's computer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontRef {
    /// The family: `Inter`.
    pub family: String,
    /// The style within the family: `Regular`, `Bold`.
    pub style: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::{PathNode, Point, Span};

    /// One shape of every kind, with sizes validation accepts.
    pub(crate) fn every_kind() -> Vec<Shape> {
        vec![
            Shape::Rectangle {
                width_mm: 60.0,
                height_mm: 30.0,
                corner: Corner::Square,
            },
            Shape::Rectangle {
                width_mm: 60.0,
                height_mm: 30.0,
                corner: Corner::Rounded { radius_mm: 5.0 },
            },
            Shape::Rectangle {
                width_mm: 60.0,
                height_mm: 30.0,
                corner: Corner::Chamfered { distance_mm: 5.0 },
            },
            Shape::Circle { diameter_mm: 20.0 },
            Shape::Ellipse {
                width_mm: 40.0,
                height_mm: 20.0,
            },
            Shape::Polygon {
                sides: 6,
                radius_mm: 10.0,
                measured: Measured::Flats,
            },
            Shape::Slot {
                length_mm: 40.0,
                width_mm: 12.0,
            },
            Shape::Gear {
                teeth: 24,
                outside_diameter_mm: 40.0,
                pressure_angle_deg: 20.0,
                flanks: Flanks::Involute,
                whole_depth_mm: 3.4615,
                root: Root::Rounded,
                root_fillet_mm: 0.23,
                crest: Crest::Flat,
                crest_radius_mm: 0.3,
                bore_diameter_mm: Some(6.0),
            },
            Shape::Text {
                text: "Dream\nCarve".to_owned(),
                font: FontRef {
                    family: "Inter".to_owned(),
                    style: "Bold".to_owned(),
                },
                height_mm: 10.0,
                letter_spacing_fraction: 0.0,
                line_spacing_fraction: 1.0,
                align: Align::Centre,
            },
            Shape::Path {
                paths: vec![Path::new(
                    vec![
                        PathNode {
                            x: 0.0,
                            y: 0.0,
                            span: Span::Cubic {
                                handle_out: Point::new(10.0, 20.0),
                                handle_in: Point::new(30.0, 20.0),
                            },
                            smooth: true,
                        },
                        PathNode::line(40.0, 0.0),
                    ],
                    true,
                )],
            },
        ]
    }

    /// Every kind reads back as it was written, says its `kind` in the
    /// file as [`Shape::kind`] does, and refuses a field it does not have.
    #[test]
    fn every_shape_kind_round_trips() {
        let mut kinds = std::collections::BTreeSet::new();
        for shape in every_kind() {
            let text = serde_json::to_string(&shape).unwrap();
            assert!(
                text.starts_with(&format!(r#"{{"kind":"{}""#, shape.kind())),
                "{text}"
            );
            assert_eq!(serde_json::from_str::<Shape>(&text).unwrap(), shape);
            let typo = text.replacen(r#"","#, r#"","nonsense":1,"#, 1);
            assert!(serde_json::from_str::<Shape>(&typo).is_err(), "{typo}");
            kinds.insert(shape.kind());
        }
        assert_eq!(kinds.len(), 8, "a shape of every kind");
    }

    /// A unit variant reads as a plain string and one with data as a
    /// one-key object (`docs/rust-guidelines.md`, "Serialization"); a
    /// length may carry its unit, as any length in a file may.
    #[test]
    fn corners_and_lengths_read_as_the_file_writes_them() {
        let text = r#"{"kind":"rectangle","width_mm":"2 in","height_mm":30.0,"corner":{"rounded":{"radius_mm":"1/4 in"}}}"#;
        let shape: Shape = serde_json::from_str(text).unwrap();
        assert_eq!(
            shape,
            Shape::Rectangle {
                width_mm: 50.8,
                height_mm: 30.0,
                corner: Corner::Rounded { radius_mm: 6.35 },
            }
        );
        let square = r#"{"kind":"rectangle","width_mm":1.0,"height_mm":1.0,"corner":"square"}"#;
        assert!(matches!(
            serde_json::from_str::<Shape>(square).unwrap(),
            Shape::Rectangle {
                corner: Corner::Square,
                ..
            }
        ));
        let gear = serde_json::to_string(&every_kind()[7]).unwrap();
        assert!(gear.contains(r#""bore_diameter_mm":6.0"#), "{gear}");
        // An unset bore is written, as `null`, and must be there to read.
        let no_bore = gear.replace(r#""bore_diameter_mm":6.0"#, r#""bore_diameter_mm":null"#);
        assert!(serde_json::from_str::<Shape>(&no_bore).is_ok());
        let missing = gear.replace(r#","bore_diameter_mm":6.0"#, "");
        assert!(serde_json::from_str::<Shape>(&missing).is_err());
    }
}
