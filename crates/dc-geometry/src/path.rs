// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Paths: outlines that may hold curves, the form a shape is edited in
//! node by node (`docs/specs/phase-5/design.md`, "Data model →
//! dc-geometry"; ADR-0006).
//!
//! A [`Polyline`] holds lines and circular arcs and is what is cut. A
//! [`Path`] holds the same two and cubic Béziers besides. Every polyline
//! is a path ([`Path::from_polyline`]); a path becomes a polyline by
//! flattening each curve within a tolerance ([`Path::to_polyline`]),
//! with its lines and arcs passed through as they are.
//!
//! Coordinates are millimetres, Y-up (ADR-0001). A [`Span::Arc`]'s bulge
//! is a [`Vertex`]'s: positive is counter-clockwise.

use crate::flatten::flatten;
use crate::point::{EPS_MM, Point};
use crate::polyline::{BULGE_EPS, Polyline, Vertex};
use serde::{Deserialize, Serialize};

/// How a path runs from one node to the next.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Span {
    /// A straight line.
    #[default]
    Line,
    /// A circular arc.
    Arc {
        /// tan(θ/4) of the arc's signed sweep, as [`Vertex::bulge`];
        /// dimensionless.
        bulge: f64,
    },
    /// A cubic Bézier.
    Cubic {
        /// The handle leaving this node, a point in the path's space,
        /// millimetres.
        handle_out: Point,
        /// The handle arriving at the next node, millimetres.
        handle_in: Point,
    },
}

impl Span {
    /// Whether this span is a cubic Bézier.
    #[must_use]
    pub const fn is_cubic(&self) -> bool {
        matches!(self, Self::Cubic { .. })
    }
}

/// One node of a [`Path`], millimetres, Y-up, and the span that leaves
/// it.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathNode {
    /// X, millimetres.
    pub x: f64,
    /// Y, millimetres.
    pub y: f64,
    /// How the path runs from this node to the next. Unused on the last
    /// node of an open path.
    pub span: Span,
    /// Whether the handles either side of this node are kept in line when
    /// one is moved. Says nothing of a node with no curve beside it.
    pub smooth: bool,
}

impl PathNode {
    /// A node at (`x`, `y`) mm whose outgoing span is a line.
    #[must_use]
    pub const fn line(x: f64, y: f64) -> Self {
        Self {
            x,
            y,
            span: Span::Line,
            smooth: false,
        }
    }

    /// The node's position.
    #[must_use]
    pub const fn point(&self) -> Point {
        Point::new(self.x, self.y)
    }
}

/// An open or closed sequence of lines, arcs and curves, millimetres,
/// Y-up. On an open path the last node's span is unused.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Path {
    nodes: Vec<PathNode>,
    closed: bool,
}

impl Path {
    /// A path through `nodes`; `closed` adds the span from the last node
    /// back to the first.
    #[must_use]
    pub fn new(nodes: Vec<PathNode>, closed: bool) -> Self {
        Self { nodes, closed }
    }

    /// The path a polyline is: each vertex a node, each line a line and
    /// each arc an arc with its bulge as it is. A bulge too small to be an
    /// arc (as [`crate::Segment::is_line`] reads it) is a line.
    #[must_use]
    pub fn from_polyline(polyline: &Polyline) -> Self {
        let nodes = polyline
            .vertices()
            .iter()
            .map(|v| PathNode {
                x: v.x,
                y: v.y,
                span: if v.bulge.abs() < BULGE_EPS {
                    Span::Line
                } else {
                    Span::Arc { bulge: v.bulge }
                },
                smooth: false,
            })
            .collect();
        Self::new(nodes, polyline.is_closed())
    }

    /// The nodes, in order.
    #[must_use]
    pub fn nodes(&self) -> &[PathNode] {
        &self.nodes
    }

    /// Whether the last node joins back to the first.
    #[must_use]
    pub const fn is_closed(&self) -> bool {
        self.closed
    }

    /// How many spans the path has: one per node when closed, one fewer
    /// when open.
    #[must_use]
    pub fn span_count(&self) -> usize {
        if self.closed {
            self.nodes.len()
        } else {
            self.nodes.len().saturating_sub(1)
        }
    }

    /// The node span `index` ends at: the next node, the first for a
    /// closed path's last span. `None` past the last span.
    #[must_use]
    pub fn span_end(&self, index: usize) -> Option<&PathNode> {
        if index >= self.span_count() {
            return None;
        }
        self.nodes.get((index + 1) % self.nodes.len().max(1))
    }

    /// Whether any span is a curve. A path without one is a polyline and
    /// needs no flattening.
    #[must_use]
    pub fn has_curves(&self) -> bool {
        (0..self.span_count())
            .filter_map(|i| self.nodes.get(i))
            .any(|node| node.span.is_cubic())
    }

    /// The path as a polyline: lines and arcs as they are, each curve as
    /// straight segments no further than `tolerance_mm` from it. The
    /// path's nodes are vertices of the result, in order.
    #[must_use]
    pub fn to_polyline(&self, tolerance_mm: f64) -> Polyline {
        let mut vertices: Vec<Vertex> = Vec::with_capacity(self.nodes.len());
        for (index, node) in self.nodes.iter().enumerate() {
            let end = self.span_end(index);
            match (node.span, end) {
                (Span::Arc { bulge }, Some(_)) => vertices.push(Vertex::new(node.x, node.y, bulge)),
                (
                    Span::Cubic {
                        handle_out,
                        handle_in,
                    },
                    Some(end),
                ) => {
                    vertices.push(Vertex::line(node.x, node.y));
                    vertices.extend(cubic_interior(
                        node.point(),
                        handle_out,
                        handle_in,
                        end.point(),
                        tolerance_mm,
                    ));
                }
                // A line, or the unused span of an open path's last node.
                _ => vertices.push(Vertex::line(node.x, node.y)),
            }
        }
        Polyline::new(vertices, self.closed)
    }
}

/// The vertices a flattened cubic adds between its two ends, which the
/// caller already has.
fn cubic_interior(
    start: Point,
    handle_out: Point,
    handle_in: Point,
    end: Point,
    tolerance_mm: f64,
) -> Vec<Vertex> {
    let mut curve = kurbo::BezPath::new();
    curve.move_to((start.x, start.y));
    curve.curve_to(
        (handle_out.x, handle_out.y),
        (handle_in.x, handle_in.y),
        (end.x, end.y),
    );
    let Some(flat) = flatten(&curve, tolerance_mm).into_iter().next() else {
        // A curve shorter than `EPS_MM` flattens to nothing.
        return Vec::new();
    };
    flat.vertices()
        .iter()
        .filter(|v| v.point().distance_to(start) > EPS_MM && v.point().distance_to(end) > EPS_MM)
        .copied()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flatten::FLATTEN_TOLERANCE_DEFAULT_MM;
    use kurbo::ParamCurveNearest;

    fn cubic_path(closed: bool) -> Path {
        Path::new(
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
                PathNode {
                    x: 40.0,
                    y: 0.0,
                    span: Span::Arc { bulge: -1.0 },
                    smooth: false,
                },
                PathNode::line(40.0, -20.0),
                PathNode::line(0.0, -20.0),
            ],
            closed,
        )
    }

    /// A polyline's lines and arcs are a path's, vertex for vertex, and
    /// come back unchanged at any tolerance: nothing is flattened.
    #[test]
    fn a_polyline_round_trips_through_a_path() {
        for closed in [true, false] {
            let polyline = Polyline::new(
                vec![
                    Vertex::line(0.0, 0.0),
                    Vertex::new(40.0, 0.0, 1.0),
                    Vertex::new(40.0, 30.0, -0.25),
                    Vertex::line(0.0, 30.0),
                ],
                closed,
            );
            let path = Path::from_polyline(&polyline);
            assert!(!path.has_curves());
            assert_eq!(path.is_closed(), closed);
            assert_eq!(path.span_count(), if closed { 4 } else { 3 });
            assert_eq!(path.to_polyline(1.0), polyline, "closed: {closed}");
        }
    }

    /// Each curve becomes straight segments within the tolerance of it;
    /// the lines and the arc beside it stay as they are, and every node
    /// is still a vertex.
    #[test]
    fn flattened_cubics_stay_within_tolerance() {
        let path = cubic_path(true);
        assert!(path.has_curves());
        let tolerance = FLATTEN_TOLERANCE_DEFAULT_MM;
        let flat = path.to_polyline(tolerance);
        assert!(flat.is_closed());
        let vertices = flat.vertices();
        for node in path.nodes() {
            assert!(
                vertices.iter().any(|v| v.point() == node.point()),
                "node {node:?} is not a vertex"
            );
        }
        // The curve's vertices are the first run, up to the arc's start.
        let arc_start = vertices
            .iter()
            .position(|v| v.point() == Point::new(40.0, 0.0))
            .unwrap();
        assert!(arc_start > 4, "the curve was flattened: {arc_start}");
        let curve = kurbo::CubicBez::new((0.0, 0.0), (10.0, 20.0), (30.0, 20.0), (40.0, 0.0));
        for (i, v) in vertices[..=arc_start].iter().enumerate() {
            let bulge = if i == arc_start { -1.0 } else { 0.0 };
            assert_eq!(v.bulge.to_bits(), f64::to_bits(bulge), "vertex {i}");
            let off = curve
                .nearest(kurbo::Point::new(v.x, v.y), 1e-9)
                .distance_sq
                .sqrt();
            assert!(off <= tolerance, "vertex {i} is {off} mm off the curve");
        }
        assert_eq!(vertices.len(), arc_start + 3);
    }

    /// An open path's last span is not drawn, whatever it says.
    #[test]
    fn an_open_paths_last_span_is_unused() {
        let mut nodes = cubic_path(false).nodes().to_vec();
        nodes[3].span = Span::Arc { bulge: 1.0 };
        let path = Path::new(nodes, false);
        assert_eq!(path.span_count(), 3);
        assert!(path.span_end(3).is_none());
        let flat = path.to_polyline(0.01);
        assert!(!flat.is_closed());
        assert_eq!(flat.vertices().last().unwrap().bulge.to_bits(), 0);
    }

    #[test]
    fn a_path_round_trips_through_json() {
        let path = cubic_path(true);
        let text = serde_json::to_string(&path).unwrap();
        assert!(text.contains(r#""span":"line""#), "{text}");
        assert!(text.contains(r#""span":{"arc":{"bulge":-1.0}}"#), "{text}");
        assert!(
            text.contains(r#""span":{"cubic":{"handle_out":{"x":10.0,"y":20.0},"handle_in":{"x":30.0,"y":20.0}}}"#),
            "{text}"
        );
        assert_eq!(serde_json::from_str::<Path>(&text).unwrap(), path);
        // A typo in a hand-edited file fails loudly.
        let typo = text.replace("handle_in", "handle_inn");
        assert!(serde_json::from_str::<Path>(&typo).is_err());
    }
}
