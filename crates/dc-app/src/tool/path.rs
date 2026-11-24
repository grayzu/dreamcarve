// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/slices/pendingAddSlice.ts (addPendingCompositePoint,
//     undoPendingCompositeStep, completePendingComposite, completePendingOpenComposite)
//   src/components/canvas/useClickPlacement.ts
//   src/components/canvas/manualEntry.ts
//   src/components/canvas/draftGeometry.ts (isLoopCloseCandidate)

//! The Line, Arc and Spline tools (`docs/specs/phase-5/design.md`, "The
//! tool state machines"; functional capabilities 87 to 89): one path
//! drawn a point at a time, each span a line, an arc or a curve.
//!
//! The three are upstream's *composite* tool and its three segment
//! modes, with its polygon and spline tools folded in: arming Arc or
//! Spline while a path is being drawn changes what the next span is and
//! keeps the path. A line takes one click; an arc two, its end and then a
//! point it passes through; a curve one, and passes smoothly through its
//! points (`Path::spline`). A click on the first point closes the path;
//! `Enter`, **Done** or a double-click finishes it open; `Backspace`
//! takes back the last point.
//!
//! A path of lines and arcs is saved as its outline, with no `shape`; a
//! path with a curve keeps its nodes in `shape` and its flattened
//! outline in `profiles` (ADR-0006).
//!
//! Changed from upstream: a typed radius bows the arc to the pointer's
//! side, where upstream's bows to a fixed side when it is typed with the
//! end; the direction of a typed angle is anticlockwise from +X (Y-up);
//! a double-click finishes any of the three, not only a polygon.

use dc_geometry::{Path, PathNode, Point, Polyline, Span, Vec2, arc_middle, bulge_through};
use dc_project::Shape;

use super::draw::{MIN_SIZE_MM, size_text};
use super::{Draft, Drawing, Event, Key, Label, Reply, SizeField, SizeRow, ToolBar};
use crate::command::{NewShape, next_name};
use crate::snap::Snapped;
use crate::strings::StrKey;
use crate::{App, Command, Tool, units};

/// How near the first point a click closes the path, logical pixels
/// (`POLYGON_CLOSE_RADIUS`).
const CLOSE_PX: f64 = 12.0;

/// What a span of the path being drawn is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum DraftSpan {
    Line,
    Arc {
        bulge: f64,
    },
    /// A curve through its ends; its handles are made when the path is.
    Curve,
}

/// The path being drawn.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct PathDraft {
    /// The points clicked, in order.
    points: Vec<Point>,
    /// What runs between each point and the next: one fewer than the
    /// points.
    spans: Vec<DraftSpan>,
    /// The end of the arc being drawn, once it is clicked: the next
    /// click is a point the arc passes through.
    arc_end: Option<Point>,
}

/// Whether `drawing` is one of this module's tools.
pub(super) const fn draws(drawing: Drawing) -> bool {
    matches!(drawing, Drawing::Line | Drawing::Arc | Drawing::Spline)
}

impl PathDraft {
    /// Whether a point has been taken.
    pub(super) fn begun(&self) -> bool {
        !self.points.is_empty()
    }

    /// Changing to another of the three tools: an arc's end that waits
    /// for its through point is dropped (`setPendingCompositeMode`).
    pub(super) fn changed_tool(&mut self) {
        self.arc_end = None;
    }

    /// What the path is called: for what its spans are when they are all
    /// of a kind, else a *Path*.
    fn name(&self, closing: Option<DraftSpan>) -> StrKey {
        let mut spans = self.spans.iter().chain(closing.iter());
        let Some(first) = spans.next() else {
            return StrKey::DrawNameLine;
        };
        let kind = |span: &DraftSpan| std::mem::discriminant(span);
        if spans.any(|span| kind(span) != kind(first)) {
            return StrKey::DrawNamePath;
        }
        match first {
            DraftSpan::Line => StrKey::DrawNameLine,
            DraftSpan::Arc { .. } => StrKey::DrawNameArc,
            DraftSpan::Curve => StrKey::DrawNameSpline,
        }
    }

    /// The path the draft is, with `closing` from its last point back to
    /// its first when it is closed. Each run of curves passes smoothly
    /// through its points; a path that is all curves is one spline.
    fn path(&self, closing: Option<DraftSpan>) -> Path {
        let count = self.points.len();
        let mut spans = self.spans.clone();
        if let Some(closing) = closing {
            spans.push(closing);
        }
        if spans.iter().all(|span| *span == DraftSpan::Curve) {
            return Path::spline(&self.points, closing.is_some());
        }
        let mut nodes: Vec<PathNode> = self
            .points
            .iter()
            .map(|p| PathNode::line(p.x, p.y))
            .collect();
        let mut index = 0;
        while index < spans.len() {
            match spans.get(index).copied() {
                Some(DraftSpan::Arc { bulge }) => {
                    if let Some(node) = nodes.get_mut(index) {
                        node.span = Span::Arc { bulge };
                    }
                    index += 1;
                }
                Some(DraftSpan::Curve) => {
                    // The run of curves from here: its points, wrapping to
                    // the first for the closing span.
                    let run = spans
                        .iter()
                        .skip(index)
                        .take_while(|span| **span == DraftSpan::Curve)
                        .count();
                    let through: Vec<Point> = (index..=index + run)
                        .filter_map(|i| self.points.get(i % count).copied())
                        .collect();
                    let curve = Path::spline(&through, false);
                    for (offset, made) in curve.nodes().iter().take(run).enumerate() {
                        if let Some(node) = nodes.get_mut(index + offset) {
                            node.span = made.span;
                            node.smooth = offset > 0;
                        }
                    }
                    // The nodes the run starts and ends at are corners.
                    index += run;
                }
                Some(DraftSpan::Line) | None => index += 1,
            }
        }
        Path::new(nodes, closing.is_some())
    }
}

impl App {
    /// Hands `event` to the Line, Arc or Spline tool `drawing`.
    pub(super) fn path_event(&mut self, drawing: Drawing, event: Event) -> Reply {
        let reply = match event {
            Event::Move(pointer) => {
                self.tool.pointer = Some(pointer);
                Reply::default()
            }
            Event::Leave => {
                self.tool.pointer = None;
                Reply::default()
            }
            Event::Press(pointer) => {
                self.tool.pointer = Some(pointer);
                self.path_point(drawing)
            }
            Event::Release(_) | Event::Node(_) => Reply::default(),
            // The first click of the pair added its point already.
            Event::DoubleClick(_) | Event::Done => self.finish_path(None),
            // With a size typed, Enter takes the point the fields give;
            // with none, it finishes the path.
            Event::Key(Key::Enter) if self.tool.pins.is_empty() => self.finish_path(None),
            Event::Key(Key::Enter) => self.path_point(drawing),
            Event::Key(Key::Escape) => {
                if self.tool.path.begun() {
                    self.tool.path = PathDraft::default();
                    self.clear_path_step();
                } else {
                    self.arm(Tool::Select);
                    return Reply::default();
                }
                Reply::default()
            }
            Event::Key(Key::Backspace | Key::Delete) => {
                self.take_back();
                Reply::default()
            }
            Event::Field { field, text } => {
                self.type_path_size(field, &text);
                Reply::default()
            }
            Event::Option(option) => {
                self.set_option(option);
                Reply::default()
            }
            // Words are the Text tool's.
            Event::Text(_) => Reply::default(),
        };
        self.refresh_path_draft(drawing);
        reply
    }

    /// The tool bar for the Line, Arc or Spline tool.
    pub(super) fn path_bar(&self, drawing: Drawing) -> ToolBar {
        let strings = self.strings();
        let units = self.project().display_units;
        let draft = &self.tool.path;
        let prompt = match (drawing, draft.begun(), draft.arc_end) {
            (_, false, _) => StrKey::DrawPromptPathFirst,
            (Drawing::Arc, true, None) => StrKey::DrawPromptArcEnd,
            (Drawing::Arc, true, Some(_)) => StrKey::DrawPromptArcThrough,
            (Drawing::Spline, true, _) => StrKey::DrawPromptSplineNext,
            (_, true, _) => StrKey::DrawPromptLineNext,
        };
        let values = self.path_values(drawing);
        let fields = self
            .path_fields(drawing)
            .iter()
            .map(|&field| {
                let pinned = self.tool.pins.get(&field).copied();
                let value = pinned.or_else(|| {
                    values
                        .iter()
                        .find(|(shown, _)| *shown == field)
                        .map(|(_, value)| *value)
                });
                SizeRow {
                    field,
                    label: strings.get(super::draw::field_label(field)).to_owned(),
                    help: strings.get(field.help()).to_owned(),
                    text: value.map_or_else(String::new, |value| size_text(field, value, units)),
                    quantity: field.quantity(),
                    pinned: pinned.is_some(),
                    problem: self.tool.problems.get(&field).cloned(),
                    option: false,
                }
            })
            .collect();
        ToolBar {
            readings: Vec::new(),
            tool: Tool::Draw(drawing),
            prompt: strings.get(prompt).to_owned(),
            fields,
            choices: Vec::new(),
            actions: Vec::new(),
            note: None,
            can_finish: draft.points.len() >= 2,
            text: None,
        }
    }

    /// The fields of the step in hand (capability 83).
    fn path_fields(&self, drawing: Drawing) -> &'static [SizeField] {
        let draft = &self.tool.path;
        match (draft.begun(), drawing, draft.arc_end) {
            (false, _, _) => &[SizeField::X, SizeField::Y],
            (true, Drawing::Arc, Some(_)) => &[SizeField::Radius],
            (true, _, _) => &[SizeField::Length, SizeField::Angle],
        }
    }

    fn clear_path_step(&mut self) {
        self.tool.pins.clear();
        self.tool.problems.clear();
    }

    /// The pointer's point for the step in hand, snapped, the last point
    /// its reference.
    fn path_pointer(&self) -> Option<Snapped> {
        let pointer = self.tool.pointer?;
        let reference = match self.tool.path.arc_end {
            // The through point turns about nothing.
            Some(_) => None,
            None => self.tool.path.points.last().copied(),
        };
        Some(self.snap_at(&pointer, reference, &[]))
    }

    /// The next point as the step would take it: the pointer's, or, once
    /// a length or an angle is typed, that far that way from the last
    /// point; the first point from typed X and Y. `None` while nothing
    /// gives one.
    fn next_point(&self) -> Option<Point> {
        let pointer = self.path_pointer().map(|snapped| snapped.at_mm);
        let pin = |field: SizeField| self.tool.pins.get(&field).copied();
        let Some(last) = self.tool.path.points.last().copied() else {
            let project = self.project();
            let origin = project.origin.xy.point_mm(&project.stock);
            let x = pin(SizeField::X)
                .map(|x| origin.x + x)
                .or(pointer.map(|p| p.x))?;
            let y = pin(SizeField::Y)
                .map(|y| origin.y + y)
                .or(pointer.map(|p| p.y))?;
            return Some(Point::new(x, y));
        };
        // With nothing typed the point is the pointer's own, to the bit: a
        // snapped point stays on what it snapped to.
        if pin(SizeField::Length).is_none() && pin(SizeField::Angle).is_none() {
            return pointer;
        }
        let offset = pointer.map(|p| p - last);
        let length = pin(SizeField::Length).or(offset.map(Vec2::length))?;
        let angle_deg = pin(SizeField::Angle)
            .or(offset
                .filter(|o| o.length() > 0.0)
                .map(|o| o.y.atan2(o.x).to_degrees()))
            .unwrap_or(0.0);
        let (sin, cos) = angle_deg.to_radians().sin_cos();
        Some(last + Vec2::new(cos, sin) * length)
    }

    /// The bulge of the arc from the last point to `end`: through the
    /// pointer, or, with a radius typed, of that radius, bowing to the
    /// pointer's side of the chord (`manualEntry.ts`, the arc's radius).
    fn arc_bulge(&self, end: Point) -> Option<f64> {
        let start = self.tool.path.points.last().copied()?;
        let pointer = self.path_pointer().map(|snapped| snapped.at_mm);
        if let Some(radius) = self.tool.pins.get(&SizeField::Radius).copied() {
            let half_chord = start.distance_to(end) / 2.0;
            if radius < half_chord || half_chord <= 0.0 {
                return None;
            }
            // The short way round: the arc's height over its chord.
            let height = radius - (radius * radius - half_chord * half_chord).max(0.0).sqrt();
            // A positive bulge bows to the right of the way from start to
            // end; the pointer says which side.
            let right = pointer.is_none_or(|p| (p - start).cross(end - start) >= 0.0);
            let bulge = height / half_chord;
            return Some(if right { bulge } else { -bulge });
        }
        bulge_through(start, end, pointer?)
    }

    /// What the fields of the step in hand read.
    fn path_values(&self, drawing: Drawing) -> Vec<(SizeField, f64)> {
        let draft = &self.tool.path;
        let Some(last) = draft.points.last().copied() else {
            let project = self.project();
            let origin = project.origin.xy.point_mm(&project.stock);
            return self
                .path_pointer()
                .map(|snapped| {
                    vec![
                        (SizeField::X, snapped.at_mm.x - origin.x),
                        (SizeField::Y, snapped.at_mm.y - origin.y),
                    ]
                })
                .unwrap_or_default();
        };
        if let (Drawing::Arc, Some(end)) = (drawing, draft.arc_end) {
            return self
                .arc_bulge(end)
                .and_then(|bulge| {
                    dc_geometry::Segment {
                        start: last,
                        end,
                        bulge,
                    }
                    .arc()
                })
                .map(|arc| vec![(SizeField::Radius, arc.radius_mm)])
                .unwrap_or_default();
        }
        self.next_point()
            .map(|next| {
                let offset = next - last;
                let angle = if offset.length() > 0.0 {
                    offset.y.atan2(offset.x).to_degrees()
                } else {
                    0.0
                };
                vec![
                    (SizeField::Length, offset.length()),
                    (SizeField::Angle, angle),
                ]
            })
            .unwrap_or_default()
    }

    /// Whether the pointer is on the first point, to close the path: three
    /// points or more, within 12 px (`isLoopCloseCandidate`).
    fn closes(&self, at: Point) -> bool {
        let draft = &self.tool.path;
        draft.points.len() >= 3
            && draft.points.first().is_some_and(|first| {
                first.distance_to(at) * self.view().camera.px_per_mm <= CLOSE_PX
            })
    }

    /// Takes the step's point (`addPendingCompositePoint`).
    fn path_point(&mut self, drawing: Drawing) -> Reply {
        let Some(at) = self.next_point() else {
            return Reply::default();
        };
        let Some(last) = self.tool.path.points.last().copied() else {
            self.tool.path.points.push(at);
            self.clear_path_step();
            return Reply::default();
        };
        if drawing == Drawing::Arc {
            let Some(end) = self.tool.path.arc_end else {
                // The arc's end; a click on the last point is no arc.
                if at.distance_to(last) >= MIN_SIZE_MM {
                    self.tool.path.arc_end = Some(at);
                    self.clear_path_step();
                }
                return Reply::default();
            };
            // A second click on the end is a double-click's, not a point
            // of the arc.
            if !self.tool.pins.contains_key(&SizeField::Radius) && at.distance_to(end) < MIN_SIZE_MM
            {
                return Reply::default();
            }
            let Some(bulge) = self.arc_bulge(end) else {
                return Reply {
                    ran: None,
                    refused: Some(self.strings().get(StrKey::DrawRefusedNoArc).to_owned()),
                };
            };
            self.tool.path.arc_end = None;
            self.clear_path_step();
            // An arc that ends on the first point closes the path.
            if self.closes(end) {
                return self.finish_path(Some(DraftSpan::Arc { bulge }));
            }
            self.tool.path.points.push(end);
            self.tool.path.spans.push(DraftSpan::Arc { bulge });
            return Reply::default();
        }
        let span = if drawing == Drawing::Spline {
            DraftSpan::Curve
        } else {
            DraftSpan::Line
        };
        if self.closes(at) {
            return self.finish_path(Some(span));
        }
        // A second click in the same place adds nothing.
        if at.distance_to(last) >= MIN_SIZE_MM {
            self.tool.path.points.push(at);
            self.tool.path.spans.push(span);
            self.clear_path_step();
        }
        Reply::default()
    }

    /// `Backspace` (`undoPendingCompositeStep`): an arc's end that waits
    /// for its through point goes first, then the last point.
    fn take_back(&mut self) {
        let draft = &mut self.tool.path;
        if draft.arc_end.take().is_none() {
            draft.points.pop();
            draft.spans.pop();
        }
        self.clear_path_step();
    }

    /// Makes the path a shape: open, or closed by `closing`. An arc's
    /// end that waits for its through point is dropped. A path of one
    /// point, or of no length, is not made, and the draft stays.
    fn finish_path(&mut self, closing: Option<DraftSpan>) -> Reply {
        let draft = &self.tool.path;
        if draft.points.len() < 2 {
            return Reply::default();
        }
        let path = draft.path(closing);
        let tolerance_mm = self.project().flatten_tolerance_mm;
        let outline = path.to_polyline(tolerance_mm);
        if outline.length_mm() < MIN_SIZE_MM {
            return Reply::default();
        }
        let name = draft.name(closing);
        let shape = path.has_curves().then(|| Shape::Path { paths: vec![path] });
        let command = Command::AddShapes(vec![NewShape {
            name: next_name(self.project(), self.strings().get(name)),
            shape,
            profiles: vec![outline],
            // Drawn where it lies, as an imported outline is.
            transform: dc_geometry::Transform::IDENTITY,
            construction: self.tool.construction,
            group: self.selection_group(),
        }]);
        match self.run(command) {
            Ok(label) => {
                self.tool.path = PathDraft::default();
                self.clear_path_step();
                Reply {
                    ran: Some(label),
                    refused: None,
                }
            }
            Err(error) => Reply {
                ran: None,
                refused: Some(error.to_string()),
            },
        }
    }

    /// Reads a typed size for the path's step.
    fn type_path_size(&mut self, field: SizeField, text: &str) {
        self.type_size(field, text);
        // A radius under half the chord is no arc's.
        if field == SizeField::Radius
            && let (Some(radius), Some(start), Some(end)) = (
                self.tool.pins.get(&field).copied(),
                self.tool.path.points.last().copied(),
                self.tool.path.arc_end,
            )
            && radius < start.distance_to(end) / 2.0
        {
            self.tool.pins.remove(&field);
            let least =
                units::field_text(start.distance_to(end) / 2.0, self.project().display_units);
            let problem = self
                .strings()
                .fmt(StrKey::DrawProblemRadiusTooSmall, &[("least", &least)]);
            self.tool.problems.insert(field, problem);
        }
    }

    /// What the tool wants drawn: the path so far, the span to the
    /// pointer, and its length or radius.
    pub(super) fn refresh_path_draft(&mut self, drawing: Drawing) {
        let snap = self.path_pointer();
        let units = self.project().display_units;
        let tolerance_mm = self.project().flatten_tolerance_mm;
        let mut draft = Draft {
            snap,
            ..Draft::default()
        };
        let path = &self.tool.path;
        if let Some(last) = path.points.last().copied() {
            // The path with the span that would be added now.
            let mut preview = path.clone();
            let mut label = None;
            if let (Drawing::Arc, Some(end)) = (drawing, path.arc_end) {
                let bulge = self.arc_bulge(end);
                preview.points.push(end);
                preview
                    .spans
                    .push(bulge.map_or(DraftSpan::Line, |bulge| DraftSpan::Arc { bulge }));
                if let Some(bulge) = bulge
                    && let Some(arc) = (dc_geometry::Segment {
                        start: last,
                        end,
                        bulge,
                    })
                    .arc()
                {
                    label = Some(Label {
                        at_mm: arc_middle(last, end, bulge),
                        text: self.strings().fmt(
                            StrKey::DrawLabelRadius,
                            &[("radius", &units::summary_text(arc.radius_mm, units))],
                        ),
                    });
                }
            } else if let Some(next) = self.next_point()
                && next.distance_to(last) > 0.0
            {
                preview.points.push(next);
                preview.spans.push(match drawing {
                    Drawing::Spline => DraftSpan::Curve,
                    _ => DraftSpan::Line,
                });
                label = Some(Label {
                    at_mm: last + (next - last) * 0.5,
                    text: self.strings().fmt(
                        StrKey::DrawLabelLength,
                        &[(
                            "length",
                            &units::summary_text(last.distance_to(next), units),
                        )],
                    ),
                });
            }
            if preview.points.len() >= 2 {
                let outline: Polyline = preview.path(None).to_polyline(tolerance_mm);
                draft.outlines.push(outline);
            }
            draft.labels.extend(label);
        }
        self.tool.draft = draft;
    }
}
