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
use dc_geometry::shapes::gear::{Gear, GearCrest, GearFlanks, GearProblem, GearRoot};
use dc_geometry::{EPS_MM, Path, Polyline, nearest_point, shapes};
use serde::{Deserialize, Serialize};

/// The fewest sides a [`Shape::Polygon`] can have.
pub const MIN_POLYGON_SIDES: u32 = 3;
/// The most sides a [`Shape::Polygon`] can have (PureCutCNC's
/// `NGON_MAX_SIDES`).
pub const MAX_POLYGON_SIDES: u32 = 50;
/// The fewest teeth a [`Shape::Gear`] can have (PureCutCNC's
/// `gearProfile.ts`).
pub const MIN_GEAR_TEETH: u32 = shapes::gear::MIN_TEETH;
/// The most teeth a [`Shape::Gear`] can have.
pub const MAX_GEAR_TEETH: u32 = shapes::gear::MAX_TEETH;
/// The smallest pressure angle of a [`Shape::Gear`], degrees.
pub const MIN_PRESSURE_ANGLE_DEG: f64 = shapes::gear::MIN_PRESSURE_ANGLE_DEG;
/// The largest pressure angle of a [`Shape::Gear`], degrees.
pub const MAX_PRESSURE_ANGLE_DEG: f64 = shapes::gear::MAX_PRESSURE_ANGLE_DEG;

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
        flanks: GearFlanks,
        /// From a tooth's tip to its root, millimetres, above 0.
        #[serde(deserialize_with = "units_input::mm")]
        whole_depth_mm: f64,
        /// The form of the gap's floor.
        root: GearRoot,
        /// The radius of a rounded root's corners, millimetres, 0 or
        /// more.
        #[serde(deserialize_with = "units_input::mm")]
        root_fillet_mm: f64,
        /// The form of a tooth's tip.
        crest: GearCrest,
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

    /// The outline these sizes give, in definition space: closed outers
    /// counter-clockwise, each followed by its holes, clockwise; a path's
    /// outlines as they were drawn
    /// (`docs/specs/phase-5/design.md`, "Algorithms → Shape outlines").
    /// A curve is flattened within `flatten_tolerance_mm`, the project's.
    ///
    /// `None` for text, whose outline needs a font, which this crate
    /// does not have (ADR-0006). A gear whose sizes give no gear is the
    /// `Err`; every other kind gives an outline whatever its sizes, a
    /// degenerate one for sizes validation refuses.
    #[must_use]
    pub fn outline(&self, flatten_tolerance_mm: f64) -> Option<Result<Vec<Polyline>, GearProblem>> {
        let one = |outline: Polyline| Some(Ok(vec![outline]));
        match self {
            Self::Rectangle {
                width_mm,
                height_mm,
                corner,
            } => one(match *corner {
                Corner::Square => shapes::rectangle(*width_mm, *height_mm),
                Corner::Rounded { radius_mm } => {
                    shapes::rounded_rectangle(*width_mm, *height_mm, radius_mm)
                }
                Corner::Chamfered { distance_mm } => {
                    shapes::chamfered_rectangle(*width_mm, *height_mm, distance_mm)
                }
            }),
            Self::Circle { diameter_mm } => one(shapes::circle(*diameter_mm)),
            Self::Ellipse {
                width_mm,
                height_mm,
            } => one(shapes::ellipse(*width_mm, *height_mm, flatten_tolerance_mm)),
            Self::Polygon {
                sides,
                radius_mm,
                measured,
            } => {
                let corner_radius_mm = match measured {
                    Measured::Corners => *radius_mm,
                    Measured::Flats => shapes::corner_radius_of_flats(*sides, *radius_mm),
                };
                one(shapes::regular_polygon(*sides, corner_radius_mm))
            }
            Self::Slot {
                length_mm,
                width_mm,
            } => one(shapes::slot(*length_mm, *width_mm)),
            Self::Gear { .. } => self.gear().map(|gear| gear.outline()),
            Self::Text { .. } => None,
            Self::Path { paths } => Some(Ok(paths
                .iter()
                .map(|path| path.to_polyline(flatten_tolerance_mm))
                .collect())),
        }
    }

    /// Whether `profiles` are the outline these sizes give at
    /// `flatten_tolerance_mm`: what validation's `shape_outline_differs`
    /// asks. `None` when there is nothing to compare with: a text, whose
    /// outline needs a font, or a gear whose sizes give no gear.
    ///
    /// The outlines must agree vertex for vertex within [`EPS_MM`]. An
    /// outline that was flattened from a curve (an ellipse, a path with
    /// curves) may also have been flattened by another build, into other
    /// vertices of the same curve, so for those it is enough that neither
    /// outline strays from the other by more than the tolerance.
    #[must_use]
    pub fn outline_matches(
        &self,
        profiles: &[Polyline],
        flatten_tolerance_mm: f64,
    ) -> Option<bool> {
        let outline = self.outline(flatten_tolerance_mm)?.ok()?;
        if outline.len() != profiles.len() {
            return Some(false);
        }
        let flattened = match self {
            Self::Ellipse { .. } => true,
            Self::Path { paths } => paths.iter().any(Path::has_curves),
            _ => false,
        };
        Some(outline.iter().zip(profiles).all(|(made, saved)| {
            same_vertices(made, saved)
                || (flattened
                    && made.is_closed() == saved.is_closed()
                    && stays_near(made, saved, flatten_tolerance_mm)
                    && stays_near(saved, made, flatten_tolerance_mm))
        }))
    }

    /// A gear's sizes as `dc-geometry` takes them; `None` for any other
    /// kind.
    #[must_use]
    pub fn gear(&self) -> Option<Gear> {
        let Self::Gear {
            teeth,
            outside_diameter_mm,
            pressure_angle_deg,
            flanks,
            whole_depth_mm,
            root,
            root_fillet_mm,
            crest,
            crest_radius_mm,
            bore_diameter_mm,
        } = *self
        else {
            return None;
        };
        Some(Gear {
            teeth,
            outside_radius_mm: outside_diameter_mm / 2.0,
            whole_depth_mm,
            flanks,
            pressure_angle_deg,
            root,
            root_fillet_mm,
            crest,
            crest_radius_mm,
            bore_diameter_mm,
        })
    }
}

/// How far apart two bulges may be and still be the same arc;
/// dimensionless.
const SAME_BULGE: f64 = 1e-9;

/// Whether two outlines have the same vertices in the same order, each
/// within [`EPS_MM`] and [`SAME_BULGE`].
fn same_vertices(a: &Polyline, b: &Polyline) -> bool {
    a.is_closed() == b.is_closed()
        && a.vertices().len() == b.vertices().len()
        && a.vertices().iter().zip(b.vertices()).all(|(v, w)| {
            (v.x - w.x).abs() <= EPS_MM
                && (v.y - w.y).abs() <= EPS_MM
                && (v.bulge - w.bulge).abs() <= SAME_BULGE
        })
}

/// Whether every vertex of `outline` is within `tolerance_mm` (and
/// [`EPS_MM`]) of `other`.
fn stays_near(outline: &Polyline, other: &Polyline, tolerance_mm: f64) -> bool {
    outline.vertices().iter().all(|vertex| {
        nearest_point(other, vertex.point())
            .is_some_and(|nearest| nearest.gap_mm <= tolerance_mm + EPS_MM)
    })
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
                flanks: GearFlanks::Involute,
                whole_depth_mm: 3.4615,
                root: GearRoot::Rounded,
                root_fillet_mm: 0.23,
                crest: GearCrest::Flat,
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

    /// Every kind but text gives an outline: closed, the outers
    /// counter-clockwise, as many outlines as the kind has. The fixture's
    /// kinds are all here (phase 5 acceptance 108 to 111).
    #[test]
    fn outline_of_every_kind() {
        let mut texts = 0;
        for shape in every_kind() {
            let Some(outline) = shape.outline(0.01) else {
                assert!(matches!(shape, Shape::Text { .. }), "{}", shape.kind());
                texts += 1;
                continue;
            };
            let outline = outline.unwrap();
            let expected = if shape.kind() == "gear" { 2 } else { 1 };
            assert_eq!(outline.len(), expected, "{}", shape.kind());
            for (i, profile) in outline.iter().enumerate() {
                assert!(profile.is_closed(), "{}", shape.kind());
                // A path winds as it was drawn; the rest are made
                // counter-clockwise, a hole after its outer clockwise.
                if shape.kind() == "path" {
                    continue;
                }
                let outer = i == 0;
                assert_eq!(
                    profile.signed_area_mm2() > 0.0,
                    outer,
                    "{} outline {i}",
                    shape.kind()
                );
            }
            assert_eq!(shape.outline_matches(&outline, 0.01), Some(true));
        }
        assert_eq!(texts, 1);
    }

    /// The sizes are the outline's: a rectangle's box, a slot's overall
    /// length, a polygon across its flats.
    #[test]
    fn an_outlines_box_is_its_sizes() {
        let cases: [(Shape, f64, f64); 6] = [
            (every_kind()[0].clone(), 60.0, 30.0),
            (every_kind()[1].clone(), 60.0, 30.0),
            (every_kind()[2].clone(), 60.0, 30.0),
            (every_kind()[3].clone(), 20.0, 20.0),
            (every_kind()[4].clone(), 40.0, 20.0),
            // A slot's length is centre to centre: 40 + 12 overall.
            (every_kind()[6].clone(), 52.0, 12.0),
        ];
        for (shape, width_mm, height_mm) in cases {
            let outline = shape.outline(0.01).unwrap().unwrap();
            let bounds = outline[0].bounds().unwrap();
            assert!(
                (bounds.width_mm() - width_mm).abs() < 1e-9,
                "{}",
                shape.kind()
            );
            assert!(
                (bounds.height_mm() - height_mm).abs() < 1e-9,
                "{}",
                shape.kind()
            );
        }
        // A hexagon of radius 10 to its flats is 20 across them.
        let hexagon = every_kind()[5].outline(0.01).unwrap().unwrap();
        assert!((hexagon[0].bounds().unwrap().height_mm() - 20.0).abs() < 1e-9);
    }

    /// A gear whose sizes give no gear has no outline, and nothing to
    /// compare a saved one with.
    #[test]
    fn a_gear_that_cannot_be_has_no_outline() {
        let Shape::Gear { .. } = &every_kind()[7] else {
            panic!("a gear");
        };
        let mut shape = every_kind()[7].clone();
        if let Shape::Gear {
            bore_diameter_mm, ..
        } = &mut shape
        {
            *bore_diameter_mm = Some(100.0);
        }
        assert!(matches!(
            shape.outline(0.01),
            Some(Err(GearProblem::BoreTooLarge { .. }))
        ));
        assert_eq!(shape.outline_matches(&[], 0.01), None);
    }

    /// An outline with a vertex moved past a micrometre, or one too many
    /// or too few, is not the shape's. An ellipse flattened into other
    /// vertices of the same curve still is; one that strays is not.
    #[test]
    fn a_changed_outline_does_not_match() {
        use dc_geometry::Vertex;
        let slot = every_kind()[6].clone();
        let outline = slot.outline(0.01).unwrap().unwrap();
        let moved = |by_mm: f64| {
            let mut vertices = outline[0].vertices().to_vec();
            vertices[2].y += by_mm;
            vec![Polyline::new(vertices, true)]
        };
        assert_eq!(slot.outline_matches(&moved(1e-7), 0.01), Some(true));
        assert_eq!(slot.outline_matches(&moved(1e-5), 0.01), Some(false));
        assert_eq!(slot.outline_matches(&[], 0.01), Some(false));
        let mut straight = outline[0].vertices().to_vec();
        straight[1].bulge = 0.0;
        let straight = vec![Polyline::new(straight, true)];
        assert_eq!(slot.outline_matches(&straight, 0.01), Some(false));
        let open = vec![Polyline::new(outline[0].vertices().to_vec(), false)];
        assert_eq!(slot.outline_matches(&open, 0.01), Some(false));

        let ellipse = every_kind()[4].clone();
        // The same ellipse flattened finer: other vertices, the same curve.
        let finer = ellipse.outline(0.001).unwrap().unwrap();
        assert_ne!(
            finer[0].vertices().len(),
            ellipse.outline(0.01).unwrap().unwrap()[0].vertices().len()
        );
        assert_eq!(ellipse.outline_matches(&finer, 0.01), Some(true));
        // A different ellipse.
        let other = Shape::Ellipse {
            width_mm: 40.0,
            height_mm: 21.0,
        };
        let other = other.outline(0.01).unwrap().unwrap();
        assert_eq!(ellipse.outline_matches(&other, 0.01), Some(false));
        // A slot is not flattened, so only its own vertices are its.
        let square = vec![Polyline::new(
            vec![
                Vertex::line(-26.0, -6.0),
                Vertex::line(26.0, -6.0),
                Vertex::line(26.0, 6.0),
                Vertex::line(-26.0, 6.0),
            ],
            true,
        )];
        assert_eq!(slot.outline_matches(&square, 0.01), Some(false));
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
