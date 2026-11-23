// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/slices/dimensionToolSlice.ts
//   src/components/canvas/dimensionRendering.ts

//! The Measure tool (functional spec, capability 103; PureCutCNC's tape
//! measure). Two clicks, each snapping as any point does, are a tape:
//! the distance between them, its ΔX and ΔY and its angle from +X, in
//! the tool bar and beside a dashed line on the Viewport. It stays until
//! the next click begins another; `Escape` leaves the tool. With one
//! shape selected the bar also reads that shape's length and, when its
//! outlines are closed, the area they enclose.
//!
//! None of it is project data: the tape lives in the tool's session, and
//! is neither saved nor an undo step.
//!
//! Where it leaves PureCutCNC: the deltas are signed, from the first
//! point to the second, and the angle is anticlockwise from +X, 0 up to
//! 360 (Y is up here), where upstream shows both without their signs;
//! and a tape that is held is drawn as the one in progress is, dashed.

use dc_geometry::{Contour, Point, Polyline, Vertex};
use dc_project::{InstanceId, Project, Units};

use super::draw::angle_text;
use super::{Draft, Event, Key, Label, NodeMark, Reading, Reply, Tool, ToolBar};
use crate::app::App;
use crate::strings::StrKey;
use crate::units;

/// Two points nearer than this, mm, have no tape between them.
const NO_DISTANCE_MM: f64 = 1e-9;

/// What a tape between two points reads.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measurement {
    /// Where it is measured from, mm.
    pub from_mm: Point,
    /// Where it is measured to, mm.
    pub to_mm: Point,
    /// The distance between them, mm.
    pub distance_mm: f64,
    /// How far along X from the first to the second, mm.
    pub dx_mm: f64,
    /// How far along Y from the first to the second, mm (Y-up).
    pub dy_mm: f64,
    /// The direction from the first to the second: degrees anticlockwise
    /// from +X, 0 up to 360; 0 when the points are one.
    pub angle_deg: f64,
}

/// What a tape from `from` to `to` reads.
#[must_use]
pub fn measurement(from: Point, to: Point) -> Measurement {
    let (dx_mm, dy_mm) = (to.x - from.x, to.y - from.y);
    let distance_mm = dx_mm.hypot(dy_mm);
    let angle_deg = if distance_mm > 0.0 {
        let turned = dy_mm.atan2(dx_mm).to_degrees().rem_euclid(360.0);
        // A hair under zero folds to 360, which is 0.
        if turned >= 360.0 { 0.0 } else { turned }
    } else {
        0.0
    };
    Measurement {
        from_mm: from,
        to_mm: to,
        distance_mm,
        dx_mm,
        dy_mm,
        angle_deg,
    }
}

/// What the Measure tool reads of one shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShapeSize {
    /// The length of its outlines, every one, mm.
    pub length_mm: f64,
    /// The area its outlines enclose, holes left out, mm²; `None` when
    /// one of them is open, or they cross.
    pub area_mm2: Option<f64>,
}

/// The length and the area of shape `id` as it is placed; `None` when
/// the project has no such shape, or it has no outline.
#[must_use]
pub fn shape_size(project: &Project, id: &InstanceId) -> Option<ShapeSize> {
    let instance = project.instances.iter().find(|i| i.id == *id)?;
    let definition = project.definitions.get(&instance.definition)?;
    let placed: Vec<Polyline> = definition
        .profiles
        .iter()
        .map(|profile| instance.transform.apply_polyline(profile))
        .collect::<Result<_, _>>()
        .ok()?;
    if placed.is_empty() {
        return None;
    }
    let closed = placed.iter().all(Polyline::is_closed);
    let area_mm2 = closed
        .then(|| {
            let contours: Vec<Contour> = placed
                .iter()
                .cloned()
                .map(Contour::new)
                .collect::<Result<_, _>>()
                .ok()?;
            let (regions, _) = dc_geometry::nest(contours).ok()?;
            Some(dc_geometry::area_mm2(&regions))
        })
        .flatten();
    Some(ShapeSize {
        length_mm: placed.iter().map(Polyline::length_mm).sum(),
        area_mm2,
    })
}

/// A length with its unit, as a reading shows it: `140 mm`, `5.512 in`.
fn length_text(value_mm: f64, units: Units) -> String {
    format!(
        "{} {}",
        units::summary_text(value_mm, units),
        units::suffix(units)
    )
}

/// How far the Measure tool has got.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub(super) struct MeasureDraft {
    /// The first point of the tape being laid, mm.
    first: Option<Point>,
    /// The tape that was laid, which stays until the next click.
    frozen: Option<(Point, Point)>,
}

impl App {
    /// Where the pointer would put a point now, snapped.
    fn measure_pointer(&self) -> Option<Point> {
        let pointer = self.tool.pointer?;
        Some(self.snap_at(&pointer, self.tool.measure.first, &[]).at_mm)
    }

    /// The tape the Measure tool shows: the one laid, or the one from
    /// the first point to the pointer; `None` under another tool, before
    /// the first click, and while the pointer is on the first point.
    #[must_use]
    pub fn measurement(&self) -> Option<Measurement> {
        if self.tool.tool != Tool::Measure {
            return None;
        }
        let measure = self.tool.measure;
        if let Some((from, to)) = measure.frozen {
            return Some(measurement(from, to));
        }
        let (from, to) = (measure.first?, self.measure_pointer()?);
        (from.distance_to(to) > NO_DISTANCE_MM).then(|| measurement(from, to))
    }

    /// Hands `event` to the Measure tool.
    pub(super) fn measure_event(&mut self, event: Event) -> Reply {
        match event {
            Event::Key(Key::Escape) => {
                self.arm(Tool::Select);
                return Reply::default();
            }
            Event::Press(pointer) => {
                let at = self.snap_at(&pointer, self.tool.measure.first, &[]).at_mm;
                let measure = &mut self.tool.measure;
                match measure.first.take() {
                    // A click begins a tape, the one laid before gone.
                    None => {
                        measure.first = Some(at);
                        measure.frozen = None;
                    }
                    // The second lays it, to stay until the next click.
                    Some(first) => measure.frozen = Some((first, at)),
                }
                self.tool.pointer = Some(pointer);
            }
            Event::Move(pointer) => self.tool.pointer = Some(pointer),
            Event::Leave => self.tool.pointer = None,
            _ => {}
        }
        self.refresh_measure_draft();
        Reply::default()
    }

    /// The preview: the tape as a dashed line with a mark on each point
    /// clicked, what it reads beside its middle, and where the next
    /// click would be taken.
    pub(super) fn refresh_measure_draft(&mut self) {
        let mut draft = Draft::default();
        let measure = self.tool.measure;
        if let Some(pointer) = self.tool.pointer {
            draft.snap = Some(self.snap_at(&pointer, measure.first, &[]));
        }
        let mark = |at_mm: Point| NodeMark {
            at_mm,
            selected: false,
            smooth: true,
        };
        draft.nodes = match (measure.first, measure.frozen) {
            (Some(first), _) => vec![mark(first)],
            (None, Some((from, to))) => vec![mark(from), mark(to)],
            (None, None) => Vec::new(),
        };
        if let Some(read) = self.measurement() {
            let (from, to) = (read.from_mm, read.to_mm);
            draft.dashed.push(Polyline::new(
                vec![Vertex::line(from.x, from.y), Vertex::line(to.x, to.y)],
                false,
            ));
            let units = self.project().display_units;
            let length = |value_mm: f64| units::summary_text(value_mm, units);
            draft.labels.push(Label {
                at_mm: from + (to - from) * 0.5,
                text: self.strings().fmt(
                    StrKey::MeasureLabel,
                    &[
                        ("distance", &length(read.distance_mm)),
                        ("dx", &length(read.dx_mm)),
                        ("dy", &length(read.dy_mm)),
                        ("angle", &angle_text(read.angle_deg)),
                    ],
                ),
            });
        }
        self.tool.draft = draft;
    }

    /// The Measure tool's bar: what the next click does, the tape's
    /// readings, and the length and area of the one shape selected.
    pub(super) fn measure_bar(&self) -> ToolBar {
        let strings = self.strings();
        let units = self.project().display_units;
        let measure = self.tool.measure;
        let prompt = match (measure.first, measure.frozen) {
            (Some(_), _) => StrKey::MeasurePromptTo,
            (None, Some(_)) => StrKey::MeasurePromptAgain,
            (None, None) => StrKey::MeasurePromptFrom,
        };
        let reading = |label: StrKey, help: StrKey, text: String| Reading {
            label: strings.get(label).to_owned(),
            help: strings.get(help).to_owned(),
            text,
        };
        let mut readings = Vec::new();
        if let Some(read) = self.measurement() {
            readings.extend([
                reading(
                    StrKey::MeasureDistance,
                    StrKey::MeasureDistanceHelp,
                    length_text(read.distance_mm, units),
                ),
                reading(
                    StrKey::MeasureDx,
                    StrKey::MeasureDxHelp,
                    length_text(read.dx_mm, units),
                ),
                reading(
                    StrKey::MeasureDy,
                    StrKey::MeasureDyHelp,
                    length_text(read.dy_mm, units),
                ),
                reading(
                    StrKey::MeasureAngle,
                    StrKey::MeasureAngleHelp,
                    strings.fmt(
                        StrKey::MeasureAngleValue,
                        &[("angle", &angle_text(read.angle_deg))],
                    ),
                ),
            ]);
        }
        if let [id] = self.selection().instances()
            && let Some(size) = shape_size(self.project(), id)
        {
            readings.push(reading(
                StrKey::MeasureLength,
                StrKey::MeasureLengthHelp,
                length_text(size.length_mm, units),
            ));
            if let Some(area_mm2) = size.area_mm2 {
                readings.push(reading(
                    StrKey::MeasureArea,
                    StrKey::MeasureAreaHelp,
                    units::area_text(area_mm2, units),
                ));
            }
        }
        ToolBar {
            tool: Tool::Measure,
            prompt: strings.get(prompt).to_owned(),
            fields: Vec::new(),
            choices: Vec::new(),
            actions: Vec::new(),
            note: None,
            can_finish: false,
            text: None,
            readings,
        }
    }
}
