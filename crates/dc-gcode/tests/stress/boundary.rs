// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The drawn boundary in the stress harness (#604, slice 21k;
//! `docs/specs/phase-3/design.md`, "Drawn boundary", "Stress").
//!
//! [`boundary_case`] turns a case of [`super::property`] into one whose
//! male Inlays take their band from drawn boundary instances: an oval or a
//! rounded rectangle 2 to 12 mm clear of the design's bounding box, or a
//! contour following the design 3 to 8 mm out; one shape, or two whose
//! union is the band. The three males are the V wall (no clearing pass),
//! the open edge (the end mill's pass) and the cut-out, so a boundary is
//! cut with and without clearing and a cut-out, with the same bit,
//! depths, units and tools as the case's computed bands. The boundary is
//! drawn from a stream of its own (`seed ^ 0xb0d0_0604`), so no seeded
//! case of the other tests changes.
//!
//! A boundary that does not fit (too tight for the cut-out's end mill, or
//! crossing the design) is refused by validation with
//! `inlay_cut_out_no_room` or `inlay_boundary_crosses_design`, which the
//! property counts as explained, not as a failure.

use super::designs::{bounds, transformed};
use super::property::{Case, Kind};
use super::rng::Rng;
use dc_geometry::{Contour, Point, Polyline, Vertex, nest, rounded_band};
use dc_project::{
    DefinitionId, FeatureDefinition, FeatureInstance, InlayOp, InstanceId, Operation,
};
use std::f64::consts::{FRAC_PI_2, SQRT_2, TAU};

/// The male kinds a boundary is cut with, and the kind each becomes.
const MALES: [(Kind, Kind); 3] = [
    (Kind::MaleVWall, Kind::BoundaryVWall),
    (Kind::MaleOpenEdge, Kind::BoundaryOpenEdge),
    (Kind::MaleCutOut, Kind::BoundaryCutOut),
];

/// What a boundary shape is.
#[derive(Debug, Clone, Copy)]
enum Shape {
    Oval,
    RoundedRect,
    Contour,
}

/// An ellipse `rx_mm` × `ry_mm` round `centre`, as 96 chords.
fn oval(centre: Point, rx_mm: f64, ry_mm: f64) -> Polyline {
    let n = 96;
    Polyline::new(
        (0..n)
            .map(|k| {
                let a = TAU * f64::from(k) / f64::from(n);
                Vertex::line(centre.x + rx_mm * a.cos(), centre.y + ry_mm * a.sin())
            })
            .collect(),
        true,
    )
}

/// The rectangle `min` to `max` with corners rounded by `radius_mm`, 8
/// chords a corner.
fn rounded_rect(min: Point, max: Point, radius_mm: f64) -> Polyline {
    let mut points = Vec::new();
    let corners = [
        (Point::new(max.x - radius_mm, min.y + radius_mm), -FRAC_PI_2),
        (Point::new(max.x - radius_mm, max.y - radius_mm), 0.0),
        (Point::new(min.x + radius_mm, max.y - radius_mm), FRAC_PI_2),
        (
            Point::new(min.x + radius_mm, min.y + radius_mm),
            2.0 * FRAC_PI_2,
        ),
    ];
    for (c, from) in corners {
        for k in 0..=8 {
            let a = from + FRAC_PI_2 * f64::from(k) / 8.0;
            points.push(Vertex::line(
                c.x + radius_mm * a.cos(),
                c.y + radius_mm * a.sin(),
            ));
        }
    }
    Polyline::new(points, true)
}

/// A description of what was drawn, for the case's label.
struct Drawn {
    profiles: Vec<Polyline>,
    note: String,
}

/// One shape of kind `shape`, `clear_mm` out from the design placed in
/// the project.
fn draw(shape: Shape, clear_mm: f64, rng: &mut Rng, placed: &[Polyline], tol_mm: f64) -> Drawn {
    let b = bounds(placed);
    let (w, h) = (b.max.x - b.min.x, b.max.y - b.min.y);
    let centre = Point::new((b.min.x + b.max.x) / 2.0, (b.min.y + b.max.y) / 2.0);
    match shape {
        Shape::Oval => Drawn {
            profiles: vec![oval(
                centre,
                w / 2.0 * SQRT_2 + clear_mm,
                h / 2.0 * SQRT_2 + clear_mm,
            )],
            note: format!("oval {clear_mm:.2} mm clear"),
        },
        Shape::RoundedRect => {
            let min = Point::new(b.min.x - clear_mm, b.min.y - clear_mm);
            let max = Point::new(b.max.x + clear_mm, b.max.y + clear_mm);
            let radius_mm = rng.range(0.1, 0.5) * (max.x - min.x).min(max.y - min.y) / 2.0;
            Drawn {
                profiles: vec![rounded_rect(min, max, radius_mm)],
                note: format!("rounded rectangle {clear_mm:.2} mm clear, corners {radius_mm:.2}"),
            }
        }
        Shape::Contour => {
            let contours: Vec<Contour> = placed
                .iter()
                .filter_map(|p| Contour::new(p.clone()).ok())
                .collect();
            let design = nest(contours).map(|(r, _)| r).unwrap_or_default();
            let band = rounded_band(&design, clear_mm, 0.0, tol_mm);
            Drawn {
                profiles: band.iter().map(|r| r.outer().polyline().clone()).collect(),
                note: format!("contour {clear_mm:.2} mm out"),
            }
        }
    }
}

/// `base` with its male Inlays (V wall, open edge, cut-out) taking their
/// band from boundary instances drawn round its design; the female and
/// the V-Carves are dropped. See the module docs.
pub fn boundary_case(base: &Case) -> Case {
    let mut rng = Rng::new(base.seed ^ 0xb0d0_0604);
    let mut project = base.project.clone();
    let placed: Vec<Polyline> = {
        let instance = project.instances.first().expect("the design instance");
        project
            .definitions
            .get(&instance.definition)
            .expect("its definition")
            .profiles
            .iter()
            .map(|p| transformed(p, instance.transform))
            .collect()
    };
    let shapes = if rng.chance(0.5) { 1 } else { 2 };
    let mut notes = Vec::new();
    let mut ids = Vec::new();
    for k in 0..shapes {
        let shape = *rng.pick(&[Shape::Oval, Shape::RoundedRect, Shape::Contour]);
        let clear_mm = match shape {
            Shape::Contour => rng.range(3.0, 8.0),
            Shape::Oval | Shape::RoundedRect => rng.range(2.0, 12.0),
        };
        let drawn = draw(
            shape,
            clear_mm,
            &mut rng,
            &placed,
            project.flatten_tolerance_mm,
        );
        notes.push(drawn.note);
        let definition = FeatureDefinition {
            id: DefinitionId::new(format!("bd{}", k + 1)),
            name: format!("Boundary {}", k + 1),
            profiles: drawn.profiles,
        };
        let id = InstanceId::new(format!("b{}", k + 1));
        project.instances.push(FeatureInstance {
            id: id.clone(),
            definition: definition.id.clone(),
            transform: dc_geometry::Transform::IDENTITY,
            name: format!("Boundary {}", k + 1),
            visible: true,
            locked: false,
            z_top_mm: None,
            z_bottom_mm: None,
        });
        project
            .definitions
            .insert(definition.id.clone(), definition);
        ids.push(id);
    }
    let ops = MALES
        .iter()
        .filter_map(|&(from, to)| {
            let (_, op) = base.ops.iter().find(|(kind, _)| *kind == from)?;
            let Operation::Inlay(inlay) = op else {
                return None;
            };
            Some((
                to,
                Operation::Inlay(InlayOp {
                    boundary: ids.clone(),
                    margin_mm: None,
                    outline_rounding_mm: None,
                    ..inlay.clone()
                }),
            ))
        })
        .collect();
    Case {
        label: format!("{} + boundary: {}", base.label, notes.join(" + ")),
        family: base.family.clone(),
        seed: base.seed,
        project,
        ops,
    }
}
