// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/helpers/profileEdit.ts
//   src/store/helpers/profileEdit.test.ts
//   src/store/slices/featureGeometrySlice.ts (moveFeatureControl)
//   src/components/canvas/manualEntry.ts (arcHandleFromRadius)

//! Editing a path node by node (`docs/specs/phase-5/design.md`,
//! "Algorithms → Paths"; functional capabilities 92 and 93): splitting a
//! span, deleting a node, breaking a path, rounding and chamfering a
//! corner, changing a span's kind, and moving nodes, handles and arcs.
//!
//! Every operation returns a new path and leaves `self` as it was. One
//! that cannot be done returns `None`, or for a corner the reason, and
//! changes nothing.
//!
//! Changed from upstream, whose profile is a start point and a list of
//! segments each ending at a point: a path here is its nodes, each with
//! the span that leaves it, so a closed path does not repeat its first
//! point and the closing span is the last node's. Arcs are bulges
//! (positive counter-clockwise in Y-up), where upstream keeps a centre
//! and a flag. Changing a span's kind, a corner (not smooth) node and the
//! reasons a corner is refused are not upstream's: the first two follow
//! Graphite's path tool, re-implemented from its behaviour
//! (`docs/research/graphite-tools-digest.md`, §2.6).

use super::spline::{arc_middle, bulge_through};
use super::{Path, PathNode, Span};
use crate::distance::nearest_point;
use crate::point::{EPS_MM, Point, Vec2};
use crate::polyline::{Polyline, Segment, Vertex};
use kurbo::{ParamCurve, ParamCurveNearest};
use std::f64::consts::{FRAC_PI_2, PI};

/// How close to its ends a span may be split, as a fraction of it
/// (upstream's `0.001` and `0.999`).
const SPLIT_MARGIN: f64 = 0.001;

/// A corner whose sides are within this of being in line, or of doubling
/// back, cannot be rounded or chamfered, radians (upstream's `1e-3`).
const FLAT_CORNER_RAD: f64 = 1e-3;

/// The bulge a straight span is given when it is made an arc: it bows
/// out by an eighth of its length, enough to see and to drag from.
const NEW_ARC_BULGE: f64 = 0.25;

/// Where on a path a point is nearest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathHit {
    /// The span, numbered by the node it leaves.
    pub span: usize,
    /// How far along the span, 0 at its start to 1 at its end: a fraction
    /// of a line's or an arc's length, a cubic's parameter.
    pub t: f64,
    /// The nearest point of the span, millimetres.
    pub point: Point,
    /// How far the given point is from it, millimetres.
    pub gap_mm: f64,
}

/// What a span can be made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanKind {
    /// A straight line.
    Line,
    /// A circular arc.
    Arc,
    /// A cubic Bézier.
    Cubic,
}

/// One of the two handles at a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleSide {
    /// The handle of the span that leaves the node.
    Out,
    /// The handle of the span that arrives at the node.
    In,
}

/// Why a corner cannot be rounded or chamfered.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum CornerError {
    /// The node is an end of an open path, or is not a node of it.
    #[error("the end of an open path is not a corner")]
    End,
    /// One of the two sides is an arc or a curve.
    #[error("a corner next to an arc or a curve cannot be rounded or chamfered")]
    NotStraight,
    /// The two sides are in line, or one doubles back on the other.
    #[error("the two sides are in line, so there is no corner")]
    Flat,
    /// The radius or the distance is not above zero.
    #[error("the size must be greater than 0")]
    NotPositive,
    /// A side is too short for the size.
    #[error("the sides are too short: less than {max_mm:.3} mm fits this corner")]
    TooLarge {
        /// The size the corner's shorter side runs out at, millimetres:
        /// a size must be under it.
        max_mm: f64,
    },
}

impl Path {
    /// The two ends of span `span`: the node it leaves and the node it
    /// arrives at.
    #[must_use]
    pub fn span_ends(&self, span: usize) -> Option<(Point, Point)> {
        let start = self.nodes.get(span)?.point();
        Some((start, self.span_end(span)?.point()))
    }

    /// The span arriving at `node`: the node before it, the last node for
    /// a closed path's first. `None` at the start of an open path.
    #[must_use]
    pub fn span_into(&self, node: usize) -> Option<usize> {
        let count = self.nodes.len();
        if node >= count {
            return None;
        }
        match (node, self.closed) {
            (0, true) => (count > 1).then(|| count - 1),
            (0, false) => None,
            _ => Some(node - 1),
        }
    }

    /// The span leaving `node`. `None` at the end of an open path.
    #[must_use]
    pub fn span_out_of(&self, node: usize) -> Option<usize> {
        (node < self.span_count()).then_some(node)
    }

    /// The point `t` of the way along span `span`, as [`PathHit::t`]
    /// counts it.
    #[must_use]
    pub fn point_on_span(&self, span: usize, t: f64) -> Option<Point> {
        let (start, end) = self.span_ends(span)?;
        Some(match self.nodes.get(span)?.span {
            Span::Line => start + (end - start) * t,
            Span::Arc { bulge } => {
                let segment = Segment { start, end, bulge };
                segment
                    .point_at(segment.length_mm() * t)
                    .map_or(start, |(point, _)| point)
            }
            Span::Cubic {
                handle_out,
                handle_in,
            } => {
                let p = cubic(start, handle_out, handle_in, end).eval(t);
                Point::new(p.x, p.y)
            }
        })
    }

    /// Where on the path `p` is nearest, the first span along it on a
    /// tie. `None` for a path with no span.
    #[must_use]
    pub fn nearest(&self, p: Point) -> Option<PathHit> {
        let mut best: Option<PathHit> = None;
        for span in 0..self.span_count() {
            let Some((start, end)) = self.span_ends(span) else {
                continue;
            };
            let hit = match self.nodes.get(span).map(|node| node.span) {
                Some(Span::Cubic {
                    handle_out,
                    handle_in,
                }) => {
                    let curve = cubic(start, handle_out, handle_in, end);
                    let nearest = curve.nearest(kurbo::Point::new(p.x, p.y), 1e-9);
                    let at = curve.eval(nearest.t);
                    Some(PathHit {
                        span,
                        t: nearest.t,
                        point: Point::new(at.x, at.y),
                        gap_mm: nearest.distance_sq.sqrt(),
                    })
                }
                Some(Span::Line) => line_or_arc_hit(span, start, end, 0.0, p),
                Some(Span::Arc { bulge }) => line_or_arc_hit(span, start, end, bulge, p),
                None => None,
            };
            if let Some(hit) = hit
                && best.is_none_or(|best| hit.gap_mm < best.gap_mm)
            {
                best = Some(hit);
            }
        }
        best
    }

    /// The path with a node added `t` of the way along span `span`
    /// (`insertPointIntoProfile`, `splitBezierSegment`, `splitArcSegment`).
    /// The outline does not change: a line becomes two lines, an arc two
    /// arcs of the same circle, a cubic its two de Casteljau halves. `t`
    /// is held a thousandth clear of each end.
    #[must_use]
    pub fn split_span(&self, span: usize, t: f64) -> Option<Self> {
        let (start, end) = self.span_ends(span)?;
        if !t.is_finite() {
            return None;
        }
        let t = t.clamp(SPLIT_MARGIN, 1.0 - SPLIT_MARGIN);
        let at = self.point_on_span(span, t)?;
        let mut nodes = self.nodes.clone();
        let first = nodes.get_mut(span)?;
        let added = match first.span {
            Span::Line => PathNode::line(at.x, at.y),
            Span::Arc { bulge } => {
                // tan(θ/4) of each part of the sweep θ.
                let quarter_sweep = bulge.atan();
                first.span = Span::Arc {
                    bulge: (quarter_sweep * t).tan(),
                };
                PathNode {
                    x: at.x,
                    y: at.y,
                    span: Span::Arc {
                        bulge: (quarter_sweep * (1.0 - t)).tan(),
                    },
                    smooth: true,
                }
            }
            Span::Cubic {
                handle_out,
                handle_in,
            } => {
                let p01 = lerp(start, handle_out, t);
                let p12 = lerp(handle_out, handle_in, t);
                let p23 = lerp(handle_in, end, t);
                let p012 = lerp(p01, p12, t);
                let p123 = lerp(p12, p23, t);
                let split = lerp(p012, p123, t);
                first.span = Span::Cubic {
                    handle_out: p01,
                    handle_in: p012,
                };
                PathNode {
                    x: split.x,
                    y: split.y,
                    span: Span::Cubic {
                        handle_out: p123,
                        handle_in: p23,
                    },
                    smooth: true,
                }
            }
        };
        nodes.insert(span + 1, added);
        Some(Self::new(nodes, self.closed))
    }

    /// The path without node `node`, its two neighbours joined
    /// (`deleteAnchorFromProfile`, `buildBridgeSegment`): by a line, or
    /// by a cubic that keeps the outer handles when either span beside
    /// the node was a cubic. An arc beside a deleted node becomes a line.
    /// Deleting an open path's end just shortens it.
    ///
    /// `None` when the path would be left with fewer than three nodes
    /// closed, or two open.
    #[must_use]
    pub fn delete_node(&self, node: usize) -> Option<Self> {
        let count = self.nodes.len();
        let least = if self.closed { 3 } else { 2 };
        if node >= count || count <= least {
            return None;
        }
        let mut nodes = self.nodes.clone();
        match (self.span_into(node), self.span_out_of(node)) {
            (Some(into), Some(out_of)) => {
                let before = self.nodes.get(into)?;
                let after = self.span_end(out_of)?;
                let removed = self.nodes.get(node)?;
                let bridge = bridge(before, after.point(), before.span, removed.span);
                nodes.get_mut(into)?.span = bridge;
            }
            // The last node of an open path: the node before it now ends
            // the path, and its span is unused.
            (Some(into), None) => nodes.get_mut(into)?.span = Span::Line,
            // The first node of an open path: nothing to join.
            (None, _) => {}
        }
        nodes.remove(node);
        Some(Self::new(nodes, self.closed))
    }

    /// The path broken at `node` (`disconnectProfileAtAnchor`). A closed
    /// path opens there: it starts at the node and ends at a second node
    /// in the same place. An open path splits there into two, the node
    /// ending the first and starting the second. `None` at the end of an
    /// open path, where there is nothing to break.
    #[must_use]
    pub fn break_at(&self, node: usize) -> Option<(Self, Option<Self>)> {
        let count = self.nodes.len();
        if node >= count || self.span_count() == 0 {
            return None;
        }
        if self.closed {
            let mut nodes: Vec<PathNode> = (0..count)
                .filter_map(|offset| self.nodes.get((node + offset) % count).copied())
                .collect();
            let start = self.nodes.get(node)?;
            nodes.push(PathNode::line(start.x, start.y));
            return Some((Self::new(nodes, false), None));
        }
        if node == 0 || node == count - 1 {
            return None;
        }
        let mut leading = self.nodes.get(..=node)?.to_vec();
        leading.last_mut()?.span = Span::Line;
        let trailing = self.nodes.get(node..)?.to_vec();
        Some((Self::new(leading, false), Some(Self::new(trailing, false))))
    }

    /// The path without span `span` (`deleteSegmentFromProfile`). A
    /// closed path opens there, starting where the span ended. An open
    /// path splits into the part before and the part after; a part with
    /// no span left is dropped. `None` when the path's only span would
    /// go.
    #[must_use]
    pub fn delete_span(&self, span: usize) -> Option<(Self, Option<Self>)> {
        let count = self.nodes.len();
        if span >= self.span_count() || self.span_count() <= 1 {
            return None;
        }
        if self.closed {
            let mut nodes: Vec<PathNode> = (1..=count)
                .filter_map(|offset| self.nodes.get((span + offset) % count).copied())
                .collect();
            nodes.last_mut()?.span = Span::Line;
            return Some((Self::new(nodes, false), None));
        }
        let mut leading = self.nodes.get(..=span)?.to_vec();
        leading.last_mut()?.span = Span::Line;
        let trailing = self.nodes.get(span + 1..)?.to_vec();
        let mut parts = [leading, trailing]
            .into_iter()
            .filter(|nodes| nodes.len() >= 2)
            .map(|nodes| Self::new(nodes, false));
        let first = parts.next()?;
        Some((first, parts.next()))
    }

    /// The path with the corner at `node` rounded by an arc of
    /// `radius_mm` tangent to both sides (`applyLineCornerFillet`): the
    /// node becomes two, each side shortened by `radius / tan(θ/2)` for a
    /// corner of interior angle `θ`, joined by the arc that turns the way
    /// the corner did.
    ///
    /// # Errors
    ///
    /// [`CornerError`]: the node is an open path's end, a side is not a
    /// straight line, the sides are in line, or a side is shorter than
    /// the arc needs.
    pub fn fillet(&self, node: usize, radius_mm: f64) -> Result<Self, CornerError> {
        let corner = self.corner(node, radius_mm)?;
        let trim_mm = radius_mm / (corner.interior_rad / 2.0).tan();
        // The largest radius whose trim the shorter side still has room
        // for.
        let max_mm = corner.shorter_mm * (corner.interior_rad / 2.0).tan();
        if trim_mm.is_nan() || trim_mm <= 0.0 || trim_mm >= corner.shorter_mm {
            return Err(CornerError::TooLarge { max_mm });
        }
        // The arc turns through what is left of the half turn, the way
        // the path turns at the corner: left is counter-clockwise.
        let bulge = ((PI - corner.interior_rad) / 4.0)
            .tan()
            .copysign(corner.turn);
        Ok(self.with_corner_cut(node, &corner, trim_mm, Span::Arc { bulge }))
    }

    /// The path with the corner at `node` cut off straight, from
    /// `distance_mm` along one side to `distance_mm` along the other
    /// (`applyLineCornerChamfer`).
    ///
    /// # Errors
    ///
    /// As [`Path::fillet`].
    pub fn chamfer(&self, node: usize, distance_mm: f64) -> Result<Self, CornerError> {
        let corner = self.corner(node, distance_mm)?;
        if distance_mm >= corner.shorter_mm {
            return Err(CornerError::TooLarge {
                max_mm: corner.shorter_mm,
            });
        }
        Ok(self.with_corner_cut(node, &corner, distance_mm, Span::Line))
    }

    /// The corner at `node`, checked: both sides straight, not in line.
    fn corner(&self, node: usize, size_mm: f64) -> Result<Corner, CornerError> {
        if !(size_mm.is_finite() && size_mm > 1e-9) {
            return Err(CornerError::NotPositive);
        }
        let (Some(into), Some(out_of)) = (self.span_into(node), self.span_out_of(node)) else {
            return Err(CornerError::End);
        };
        let (Some(before), Some(at), Some(after)) = (
            self.nodes.get(into),
            self.nodes.get(node),
            self.span_end(out_of),
        ) else {
            return Err(CornerError::End);
        };
        if before.span != Span::Line || at.span != Span::Line {
            return Err(CornerError::NotStraight);
        }
        let (back, on) = (before.point() - at.point(), after.point() - at.point());
        let (Some(back_dir), Some(on_dir)) = (back.normalized(), on.normalized()) else {
            return Err(CornerError::Flat);
        };
        let interior_rad = back_dir.dot(on_dir).clamp(-1.0, 1.0).acos();
        if !interior_rad.is_finite()
            || interior_rad <= FLAT_CORNER_RAD
            || (PI - interior_rad).abs() <= FLAT_CORNER_RAD
        {
            return Err(CornerError::Flat);
        }
        Ok(Corner {
            at: at.point(),
            back_dir,
            on_dir,
            interior_rad,
            shorter_mm: back.length().min(on.length()),
            // The way the path turns here: positive to the left.
            turn: (-back_dir).cross(on_dir),
        })
    }

    /// The path with the corner node replaced by two, `trim_mm` back
    /// along each side, joined by `join`.
    fn with_corner_cut(&self, node: usize, corner: &Corner, trim_mm: f64, join: Span) -> Self {
        let first = corner.at + corner.back_dir * trim_mm;
        let second = corner.at + corner.on_dir * trim_mm;
        let mut nodes = self.nodes.clone();
        let replacement = [
            PathNode {
                x: first.x,
                y: first.y,
                span: join,
                smooth: false,
            },
            PathNode::line(second.x, second.y),
        ];
        nodes.splice(node..=node, replacement);
        Self::new(nodes, self.closed)
    }

    /// The path with span `span` made a line, an arc or a cubic, its two
    /// ends where they were.
    ///
    /// - To a line: the bulge or the handles are dropped.
    /// - To an arc: a cubic becomes the arc through its middle point; a
    ///   straight span, which has no middle to bow through, bows out by
    ///   an eighth of its length.
    /// - To a cubic: a line gets handles a third of the way along it; an
    ///   arc becomes the cubic that follows it, or, when it turns more
    ///   than a quarter turn, as many cubics as it has quarter turns,
    ///   with a node between each.
    ///
    /// A span that is already of the kind is left as it is.
    #[must_use]
    pub fn with_span_kind(&self, span: usize, kind: SpanKind) -> Option<Self> {
        let (start, end) = self.span_ends(span)?;
        let current = self.nodes.get(span)?.span;
        let mut nodes = self.nodes.clone();
        match (kind, current) {
            (SpanKind::Line, _) => nodes.get_mut(span)?.span = Span::Line,
            (SpanKind::Arc, Span::Arc { .. }) | (SpanKind::Cubic, Span::Cubic { .. }) => {}
            (SpanKind::Arc, Span::Line) => {
                nodes.get_mut(span)?.span = Span::Arc {
                    bulge: NEW_ARC_BULGE,
                };
            }
            (SpanKind::Arc, Span::Cubic { .. }) => {
                let middle = self.point_on_span(span, 0.5)?;
                let bulge = bulge_through(start, end, middle).unwrap_or(NEW_ARC_BULGE);
                nodes.get_mut(span)?.span = Span::Arc { bulge };
            }
            (SpanKind::Cubic, Span::Line) => {
                nodes.get_mut(span)?.span = Span::Cubic {
                    handle_out: lerp(start, end, 1.0 / 3.0),
                    handle_in: lerp(start, end, 2.0 / 3.0),
                };
            }
            (SpanKind::Cubic, Span::Arc { bulge }) => {
                let cubics = arc_as_cubics(start, end, bulge);
                let mut replacement = cubics.into_iter();
                let first = replacement.next()?;
                nodes.get_mut(span)?.span = first.span;
                // The nodes between the cubics, where the arc is split.
                let added: Vec<PathNode> = replacement.collect();
                let tail = nodes.split_off(span + 1);
                nodes.extend(added);
                nodes.extend(tail);
            }
        }
        Some(Self::new(nodes, self.closed))
    }

    /// The path with node `node` moved to `to` (`moveFeatureControl`,
    /// anchor). The handles of a curve beside it move with it. An arc
    /// beside it is rebuilt through the point that was its middle, so it
    /// changes radius and centre rather than staying concentric; an arc
    /// the three points no longer span becomes a line.
    #[must_use]
    pub fn with_node_at(&self, node: usize, to: Point) -> Option<Self> {
        let old = self.nodes.get(node)?.point();
        let delta = to - old;
        let mut nodes = self.nodes.clone();
        // The span that arrives at the node.
        if let Some(into) = self.span_into(node) {
            let start = self.nodes.get(into)?.point();
            let arriving = nodes.get_mut(into)?;
            arriving.span = match arriving.span {
                Span::Line => Span::Line,
                Span::Arc { bulge } => rebuilt_arc(start, to, arc_middle(start, old, bulge)),
                Span::Cubic {
                    handle_out,
                    handle_in,
                } => Span::Cubic {
                    handle_out,
                    handle_in: handle_in + delta,
                },
            };
        }
        // The span that leaves it.
        let end = self
            .span_out_of(node)
            .and_then(|out_of| self.span_end(out_of));
        let moved = nodes.get_mut(node)?;
        moved.x = to.x;
        moved.y = to.y;
        if let Some(end) = end {
            let end = end.point();
            moved.span = match moved.span {
                Span::Line => Span::Line,
                Span::Arc { bulge } => rebuilt_arc(to, end, arc_middle(old, end, bulge)),
                Span::Cubic {
                    handle_out,
                    handle_in,
                } => Span::Cubic {
                    handle_out: handle_out + delta,
                    handle_in,
                },
            };
        }
        Some(Self::new(nodes, self.closed))
    }

    /// The path with one handle at `node` moved to `to`
    /// (`moveFeatureControl`, handles). With `keep_smooth`, at a smooth
    /// node between two cubics, the other handle is turned to stay in
    /// line with it, keeping its own length. Without, the handle moves
    /// alone and the node becomes a corner (Graphite's Alt). `None` when
    /// that side of the node is not a cubic.
    #[must_use]
    pub fn with_handle_at(
        &self,
        node: usize,
        side: HandleSide,
        to: Point,
        keep_smooth: bool,
    ) -> Option<Self> {
        let at = self.nodes.get(node)?.point();
        let (into, out_of) = (self.span_into(node), self.span_out_of(node));
        let mut nodes = self.nodes.clone();
        // The spans holding the moved handle and the one across the node.
        let (moved, other) = match side {
            HandleSide::Out => (out_of?, into),
            HandleSide::In => (into?, out_of),
        };
        set_handle(nodes.get_mut(moved)?, side, to)?;
        let node_mut = nodes.get_mut(node)?;
        if !keep_smooth {
            node_mut.smooth = false;
        } else if node_mut.smooth
            && let Some(other) = other
            && let Some(direction) = (at - to).normalized()
        {
            let across = match side {
                HandleSide::Out => HandleSide::In,
                HandleSide::In => HandleSide::Out,
            };
            if let Some(length) = handle(nodes.get(other)?, across).map(|h| h.distance_to(at)) {
                // A side that is not a cubic has no handle to turn.
                let _ = set_handle(nodes.get_mut(other)?, across, at + direction * length);
            }
        }
        Some(Self::new(nodes, self.closed))
    }

    /// The path with node `node` made smooth or a corner. Made smooth,
    /// its two handles are turned into line with each other, each
    /// keeping its length, along the line between them; a node with a
    /// curve on one side only just takes the mark.
    #[must_use]
    pub fn with_node_smooth(&self, node: usize, smooth: bool) -> Option<Self> {
        let at = self.nodes.get(node)?.point();
        let mut nodes = self.nodes.clone();
        nodes.get_mut(node)?.smooth = smooth;
        if smooth
            && let (Some(into), Some(out_of)) = (self.span_into(node), self.span_out_of(node))
            && let (Some(arriving), Some(leaving)) = (
                handle(self.nodes.get(into)?, HandleSide::In),
                handle(self.nodes.get(out_of)?, HandleSide::Out),
            )
            && let Some(direction) = (leaving - arriving).normalized()
        {
            let _ = set_handle(
                nodes.get_mut(out_of)?,
                HandleSide::Out,
                at + direction * leaving.distance_to(at),
            );
            let _ = set_handle(
                nodes.get_mut(into)?,
                HandleSide::In,
                at + (-direction) * arriving.distance_to(at),
            );
        }
        Some(Self::new(nodes, self.closed))
    }

    /// The path with span `span` made the arc through `through`, its ends
    /// where they were (`moveFeatureControl`, arc handle): what dragging
    /// an arc's middle does. A point in line with the ends makes the span
    /// straight.
    #[must_use]
    pub fn with_arc_through(&self, span: usize, through: Point) -> Option<Self> {
        let (start, end) = self.span_ends(span)?;
        let mut nodes = self.nodes.clone();
        nodes.get_mut(span)?.span = rebuilt_arc(start, end, through);
        Some(Self::new(nodes, self.closed))
    }

    /// The radius of span `span`, millimetres; `None` when it is not an
    /// arc.
    #[must_use]
    pub fn arc_radius_mm(&self, span: usize) -> Option<f64> {
        let (start, end) = self.span_ends(span)?;
        let Span::Arc { bulge } = self.nodes.get(span)?.span else {
            return None;
        };
        Segment { start, end, bulge }.arc().map(|arc| arc.radius_mm)
    }

    /// The path with the arc of span `span` given `radius_mm`
    /// (`arcHandleFromRadius`): its ends stay, it bows to the same side,
    /// and it stays under or over a half turn as it was. `None` when the
    /// span is not an arc, or the radius is under half the distance
    /// between its ends, which no circle through both can have.
    #[must_use]
    pub fn with_arc_radius(&self, span: usize, radius_mm: f64) -> Option<Self> {
        let (start, end) = self.span_ends(span)?;
        let Span::Arc { bulge } = self.nodes.get(span)?.span else {
            return None;
        };
        let half_chord = start.distance_to(end) / 2.0;
        if !(radius_mm.is_finite() && half_chord > EPS_MM) || radius_mm < half_chord {
            return None;
        }
        // The arc's height over its chord: the short way round for an
        // arc under a half turn, the long way for one over.
        let reach = (radius_mm * radius_mm - half_chord * half_chord)
            .max(0.0)
            .sqrt();
        let height = if bulge.abs() > 1.0 {
            radius_mm + reach
        } else {
            radius_mm - reach
        };
        let mut nodes = self.nodes.clone();
        nodes.get_mut(span)?.span = Span::Arc {
            bulge: (height / half_chord).copysign(bulge),
        };
        Some(Self::new(nodes, self.closed))
    }

    /// The path run the other way (`reverseOpenProfile`, and the same for
    /// a closed one): the same outline, its nodes in reverse order, each
    /// arc turning the other way and each cubic's handles exchanged.
    #[must_use]
    pub fn reversed(&self) -> Self {
        let count = self.nodes.len();
        let nodes = (0..count)
            .rev()
            .filter_map(|old| {
                let node = self.nodes.get(old)?;
                // The span that arrived at this node now leaves it.
                let span = self
                    .span_into(old)
                    .and_then(|into| self.nodes.get(into))
                    .map_or(Span::Line, |before| match before.span {
                        Span::Line => Span::Line,
                        Span::Arc { bulge } => Span::Arc { bulge: -bulge },
                        Span::Cubic {
                            handle_out,
                            handle_in,
                        } => Span::Cubic {
                            handle_out: handle_in,
                            handle_in: handle_out,
                        },
                    });
                Some(PathNode { span, ..*node })
            })
            .collect();
        Self::new(nodes, self.closed)
    }

    /// An open path closed (`closeOpenProfile`): by a straight line from
    /// its end to its start, or, when the two are within `tolerance_mm`,
    /// by making them one node. `None` for a path that is closed already
    /// or has fewer than two spans.
    #[must_use]
    pub fn closed_up(&self, tolerance_mm: f64) -> Option<Self> {
        if self.closed || self.span_count() < 2 {
            return None;
        }
        let mut nodes = self.nodes.clone();
        let (first, last) = (nodes.first()?.point(), nodes.last()?.point());
        if first.distance_to(last) <= tolerance_mm.max(EPS_MM) {
            nodes.pop();
            if nodes.len() < 3 {
                return None;
            }
        } else {
            nodes.last_mut()?.span = Span::Line;
        }
        Some(Self::new(nodes, true))
    }
}

/// A corner between two straight sides.
struct Corner {
    at: Point,
    /// From the corner back along the side that arrives.
    back_dir: Vec2,
    /// From the corner along the side that leaves.
    on_dir: Vec2,
    interior_rad: f64,
    /// The shorter side's length, millimetres.
    shorter_mm: f64,
    /// Positive when the path turns left at the corner.
    turn: f64,
}

fn lerp(a: Point, b: Point, t: f64) -> Point {
    a + (b - a) * t
}

fn cubic(start: Point, handle_out: Point, handle_in: Point, end: Point) -> kurbo::CubicBez {
    kurbo::CubicBez::new(
        (start.x, start.y),
        (handle_out.x, handle_out.y),
        (handle_in.x, handle_in.y),
        (end.x, end.y),
    )
}

/// Where a line or an arc is nearest `p`.
fn line_or_arc_hit(span: usize, start: Point, end: Point, bulge: f64, p: Point) -> Option<PathHit> {
    let one = Polyline::new(
        vec![
            Vertex::new(start.x, start.y, bulge),
            Vertex::line(end.x, end.y),
        ],
        false,
    );
    let nearest = nearest_point(&one, p)?;
    let length_mm = one.length_mm();
    Some(PathHit {
        span,
        t: if length_mm > 0.0 {
            (nearest.along_segment_mm / length_mm).clamp(0.0, 1.0)
        } else {
            0.0
        },
        point: nearest.point,
        gap_mm: nearest.gap_mm,
    })
}

/// The span that joins the neighbours of a deleted node
/// (`buildBridgeSegment`).
fn bridge(before: &PathNode, after: Point, arriving: Span, leaving: Span) -> Span {
    let start = before.point();
    match (arriving, leaving) {
        (Span::Cubic { handle_out, .. }, Span::Cubic { handle_in, .. }) => Span::Cubic {
            handle_out,
            handle_in,
        },
        (Span::Cubic { handle_out, .. }, _) => Span::Cubic {
            handle_out,
            handle_in: lerp(after, start, 1.0 / 3.0),
        },
        (_, Span::Cubic { handle_in, .. }) => Span::Cubic {
            handle_out: lerp(start, after, 1.0 / 3.0),
            handle_in,
        },
        _ => Span::Line,
    }
}

/// The arc from `start` to `end` through `through`, or a line when the
/// three are in line.
fn rebuilt_arc(start: Point, end: Point, through: Point) -> Span {
    bulge_through(start, end, through).map_or(Span::Line, |bulge| Span::Arc { bulge })
}

/// One handle of the cubic that leaves `node`: its own `Out`, or the
/// `In` that arrives at the next node. `None` when the span is no cubic.
fn handle(node: &PathNode, side: HandleSide) -> Option<Point> {
    match (node.span, side) {
        (Span::Cubic { handle_out, .. }, HandleSide::Out) => Some(handle_out),
        (Span::Cubic { handle_in, .. }, HandleSide::In) => Some(handle_in),
        _ => None,
    }
}

/// Sets one handle of the cubic that leaves `node`; `None` when the span
/// is no cubic.
fn set_handle(node: &mut PathNode, side: HandleSide, to: Point) -> Option<()> {
    let Span::Cubic {
        handle_out,
        handle_in,
    } = &mut node.span
    else {
        return None;
    };
    match side {
        HandleSide::Out => *handle_out = to,
        HandleSide::In => *handle_in = to,
    }
    Some(())
}

/// The arc from `start` to `end` as cubics, one for each quarter turn or
/// part of one: the node each starts at and its span. The standard
/// approximation, handles `4/3·tan(θ/4)·r` along the tangents, off the
/// arc by under 0.03 % of its radius for a quarter turn.
fn arc_as_cubics(start: Point, end: Point, bulge: f64) -> Vec<PathNode> {
    let segment = Segment { start, end, bulge };
    let Some(arc) = segment.arc() else {
        return vec![PathNode {
            x: start.x,
            y: start.y,
            span: Span::Cubic {
                handle_out: lerp(start, end, 1.0 / 3.0),
                handle_in: lerp(start, end, 2.0 / 3.0),
            },
            smooth: false,
        }];
    };
    // A sweep a rounding over a whole number of quarter turns is that
    // many.
    let parts = (arc.sweep_rad.abs() / FRAC_PI_2 - 1e-9).ceil().max(1.0);
    let part_sweep = arc.sweep_rad / parts;
    let reach = 4.0 / 3.0 * (part_sweep / 4.0).tan() * arc.radius_mm;
    let at = |angle: f64| -> (Point, Vec2) {
        let (sin, cos) = angle.sin_cos();
        (
            Point::new(
                arc.centre.x + arc.radius_mm * cos,
                arc.centre.y + arc.radius_mm * sin,
            ),
            // The direction of travel for a counter-clockwise arc; `reach`
            // carries the sign for a clockwise one.
            Vec2::new(-sin, cos),
        )
    };
    let mut nodes = Vec::new();
    let mut k = 0.0;
    while k < parts {
        let from = arc.start_angle_rad + part_sweep * k;
        let (from_point, from_tangent) = at(from);
        let (to_point, to_tangent) = at(from + part_sweep);
        // The arc's own ends, exactly, so the path does not move.
        let from_point = if k == 0.0 { start } else { from_point };
        let to_point = if k + 1.0 >= parts { end } else { to_point };
        nodes.push(PathNode {
            x: from_point.x,
            y: from_point.y,
            span: Span::Cubic {
                handle_out: from_point + from_tangent * reach,
                handle_in: to_point + to_tangent * (-reach),
            },
            smooth: k > 0.0,
        });
        k += 1.0;
    }
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assert_approx_eq;

    /// Upstream's `squareProfile`: (0,0), (10,0), (10,10), (0,10), closed.
    /// In Y-up numbers it is counter-clockwise.
    fn square() -> Path {
        Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode::line(10.0, 0.0),
                PathNode::line(10.0, 10.0),
                PathNode::line(0.0, 10.0),
            ],
            true,
        )
    }

    /// An open corner: (0,0) to (10,0) to (10,10).
    fn elbow() -> Path {
        Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode::line(10.0, 0.0),
                PathNode::line(10.0, 10.0),
            ],
            false,
        )
    }

    fn points(path: &Path) -> Vec<(f64, f64)> {
        path.nodes().iter().map(|n| (n.x, n.y)).collect()
    }

    /// Asserts the path's nodes are at `expected`, within a nanometre.
    #[track_caller]
    fn assert_nodes(path: &Path, expected: &[(f64, f64)]) {
        let nodes = points(path);
        assert_eq!(nodes.len(), expected.len(), "{nodes:?}");
        for (node, expected) in nodes.iter().zip(expected) {
            assert!(
                (node.0 - expected.0).abs() < 1e-9 && (node.1 - expected.1).abs() < 1e-9,
                "{nodes:?} is not {expected:?}"
            );
        }
    }

    /// What [`apart_mm`] can read between two outlines that are the same
    /// curve: each is flattened to within a tenth of a micrometre.
    const SAME_OUTLINE_MM: f64 = 3e-4;

    fn cubic_node(x: f64, y: f64, out: (f64, f64), into: (f64, f64)) -> PathNode {
        PathNode {
            x,
            y,
            span: Span::Cubic {
                handle_out: Point::new(out.0, out.1),
                handle_in: Point::new(into.0, into.1),
            },
            smooth: true,
        }
    }

    /// How far apart two outlines are: the furthest any vertex of either,
    /// flattened fine, is from the other.
    fn apart_mm(a: &Path, b: &Path) -> f64 {
        let (a, b) = (
            a.to_polyline(1e-4).with_arcs_flattened(1e-4),
            b.to_polyline(1e-4).with_arcs_flattened(1e-4),
        );
        let one_way = |from: &Polyline, to: &Polyline| {
            from.vertices()
                .iter()
                .filter_map(|v| nearest_point(to, v.point()))
                .map(|nearest| nearest.gap_mm)
                .fold(0.0, f64::max)
        };
        one_way(&a, &b).max(one_way(&b, &a))
    }

    /// `testInsertPointIntoProfile`: a line from (0,0) to (10,0) split at
    /// 0.4 is two lines meeting at (4,0).
    #[test]
    fn insert_point_splits_a_line() {
        let line = Path::new(
            vec![PathNode::line(0.0, 0.0), PathNode::line(10.0, 0.0)],
            false,
        );
        let split = line.split_span(0, 0.4).unwrap();
        assert_eq!(points(&split), [(0.0, 0.0), (4.0, 0.0), (10.0, 0.0)]);
        assert!(!split.is_closed() && !split.has_curves());
        assert_eq!(
            line.split_span(1, 0.5),
            None,
            "an open path's last node has no span"
        );
    }

    /// `testSplitBezierSegment`: the de Casteljau halves of the cubic
    /// (0,0) (0,6) (6,6) (6,0) at a half.
    #[test]
    fn split_cubic_returns_the_de_casteljau_halves() {
        let curve = Path::new(
            vec![
                cubic_node(0.0, 0.0, (0.0, 6.0), (6.0, 6.0)),
                PathNode::line(6.0, 0.0),
            ],
            false,
        );
        let split = curve.split_span(0, 0.5).unwrap();
        assert_eq!(
            split.nodes()[0].span,
            Span::Cubic {
                handle_out: Point::new(0.0, 3.0),
                handle_in: Point::new(1.5, 4.5),
            }
        );
        assert_eq!(split.nodes()[1].point(), Point::new(3.0, 4.5));
        assert_eq!(
            split.nodes()[1].span,
            Span::Cubic {
                handle_out: Point::new(4.5, 4.5),
                handle_in: Point::new(6.0, 3.0),
            }
        );
        assert!(
            split.nodes()[1].smooth,
            "the curve runs straight through it"
        );
        assert!(apart_mm(&curve, &split) < SAME_OUTLINE_MM);
    }

    /// `splitArcSegment`: an arc splits into two arcs of the same circle,
    /// wherever along it, and the outline does not move.
    #[test]
    fn split_arc_keeps_the_circle() {
        for bulge in [1.0, -0.3, 2.5] {
            let arc = Path::new(
                vec![
                    PathNode {
                        x: 0.0,
                        y: 0.0,
                        span: Span::Arc { bulge },
                        smooth: false,
                    },
                    PathNode::line(10.0, 0.0),
                ],
                false,
            );
            let whole = Segment {
                start: Point::new(0.0, 0.0),
                end: Point::new(10.0, 0.0),
                bulge,
            }
            .arc()
            .unwrap();
            for t in [0.25, 0.5, 0.9] {
                let split = arc.split_span(0, t).unwrap();
                assert_eq!(split.span_count(), 2);
                let flat = split.to_polyline(0.01);
                let mut sweep = 0.0;
                for part in flat.segments() {
                    let part = part.arc().unwrap();
                    assert_approx_eq!(part.radius_mm, whole.radius_mm, 1e-9);
                    assert_approx_eq!(part.centre.x, whole.centre.x, 1e-9);
                    assert_approx_eq!(part.centre.y, whole.centre.y, 1e-9);
                    sweep += part.sweep_rad;
                }
                assert_approx_eq!(sweep, whole.sweep_rad, 1e-9);
                let first = flat.segments().next().unwrap().arc().unwrap();
                assert_approx_eq!(first.sweep_rad, whole.sweep_rad * t, 1e-9);
            }
        }
    }

    /// `testCloseOpenProfile`: an open path of two lines closes with a
    /// third; one whose end is on its start closes by losing the repeat.
    #[test]
    fn close_open_path() {
        let open = Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode::line(3.0, 0.0),
                PathNode::line(3.0, 2.0),
            ],
            false,
        );
        let closed = open.closed_up(0.0).unwrap();
        assert!(closed.is_closed());
        assert_eq!(points(&closed), [(0.0, 0.0), (3.0, 0.0), (3.0, 2.0)]);
        assert_eq!(closed.span_count(), 3);
        assert_eq!(closed.closed_up(0.0), None, "closed already");

        let (broken, _) = square().break_at(2).unwrap();
        assert_eq!(broken.nodes().len(), 5);
        let again = broken.closed_up(0.0).unwrap();
        assert_eq!(again.nodes().len(), 4);
        assert!(apart_mm(&again, &square()) < SAME_OUTLINE_MM);
        // An end within the tolerance of the start joins it too.
        let mut nodes = broken.nodes().to_vec();
        nodes[4].x += 0.05;
        let near = Path::new(nodes, false);
        assert_eq!(near.closed_up(0.1).unwrap().nodes().len(), 4);
        assert_eq!(near.closed_up(0.0).unwrap().nodes().len(), 5);
    }

    /// `testApplyLineCornerFillet`: a 2 mm fillet at (10,0) trims the
    /// first side to (8,0), adds an arc to (10,2) about (8,2), and keeps
    /// the second side. Upstream's `!clockwise` is counter-clockwise.
    #[test]
    fn fillet_a_line_corner() {
        let filleted = elbow().fillet(1, 2.0).unwrap();
        assert_nodes(
            &filleted,
            &[(0.0, 0.0), (8.0, 0.0), (10.0, 2.0), (10.0, 10.0)],
        );
        let flat = filleted.to_polyline(0.01);
        let segments: Vec<Segment> = flat.segments().collect();
        assert!(segments[0].is_line() && segments[2].is_line());
        let arc = segments[1].arc().unwrap();
        assert_approx_eq!(arc.centre.x, 8.0, 1e-9);
        assert_approx_eq!(arc.centre.y, 2.0, 1e-9);
        assert_approx_eq!(arc.radius_mm, 2.0, 1e-9);
        assert!(
            arc.sweep_rad > 0.0,
            "counter-clockwise, as the corner turns"
        );
        assert_approx_eq!(arc.sweep_rad, FRAC_PI_2, 1e-9);
        // A corner turning the other way gets an arc turning the other way.
        let right = Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode::line(10.0, 0.0),
                PathNode::line(10.0, -10.0),
            ],
            false,
        );
        let flat = right.fillet(1, 2.0).unwrap().to_polyline(0.01);
        let arc = flat.segments().nth(1).unwrap().arc().unwrap();
        assert_approx_eq!(arc.sweep_rad, -FRAC_PI_2, 1e-9);
        assert_approx_eq!(arc.centre.y, -2.0, 1e-9);
    }

    /// A fillet of a closed path's first node works as any other, and a
    /// corner of any angle gets an arc tangent to both sides.
    #[test]
    fn fillet_any_corner_of_a_closed_path() {
        let filleted = square().fillet(0, 2.0).unwrap();
        assert!(filleted.is_closed());
        assert_eq!(filleted.nodes().len(), 5);
        assert_nodes(
            &filleted,
            &[
                (0.0, 2.0),
                (2.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
            ],
        );
        let triangle = Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode::line(30.0, 0.0),
                PathNode::line(10.0, 20.0),
            ],
            true,
        );
        for node in 0..3 {
            let rounded = triangle.fillet(node, 1.5).unwrap().to_polyline(0.01);
            let segments: Vec<Segment> = rounded.segments().collect();
            let count = segments.len();
            let (index, arc) = segments
                .iter()
                .enumerate()
                .find_map(|(i, s)| s.arc().map(|arc| (i, arc)))
                .unwrap();
            assert_approx_eq!(arc.radius_mm, 1.5, 1e-9);
            let segment = segments[index];
            let before = segments[(index + count - 1) % count];
            let after = segments[(index + 1) % count];
            let (_, leaving) = segment.point_at(0.0).unwrap();
            let (_, arriving) = segment.point_at(segment.length_mm()).unwrap();
            assert_approx_eq!(
                leaving.cross((before.end - before.start).normalized().unwrap()),
                0.0,
                1e-9
            );
            assert_approx_eq!(
                arriving.cross((after.end - after.start).normalized().unwrap()),
                0.0,
                1e-9
            );
        }
    }

    /// `testApplyLineCornerChamfer`, all its cases.
    #[test]
    fn chamfer_a_line_corner() {
        let chamfered = elbow().chamfer(1, 2.0).unwrap();
        assert_nodes(
            &chamfered,
            &[(0.0, 0.0), (8.0, 0.0), (10.0, 2.0), (10.0, 10.0)],
        );
        assert!(!chamfered.has_curves());
        assert!(chamfered.to_polyline(0.01).segments().all(|s| s.is_line()));

        // The closing node of a closed path: it starts at the trim point
        // on the arriving side.
        let closed = square().chamfer(0, 2.0).unwrap();
        assert_nodes(
            &closed,
            &[
                (0.0, 2.0),
                (2.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0),
                (0.0, 10.0),
            ],
        );
        assert_eq!(closed.nodes().len(), 5);

        // A corner that is not square: the same distance along each side.
        let angled = Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode::line(10.0, 0.0),
                PathNode::line(15.0, 10.0),
            ],
            false,
        );
        let cut = angled.chamfer(1, 2.0).unwrap();
        assert_approx_eq!(cut.nodes()[1].x, 8.0, 1e-9);
        assert_approx_eq!(cut.nodes()[2].x, 10.0 + 2.0 / 5f64.sqrt(), 1e-9);
        assert_approx_eq!(cut.nodes()[2].y, 4.0 / 5f64.sqrt(), 1e-9);
    }

    /// What a corner refuses, and why: upstream returns `null` for each;
    /// here each has its reason for the status bar.
    #[test]
    fn a_corner_that_cannot_be_cut_says_why() {
        let arc_neighbour = Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode {
                    x: 10.0,
                    y: 0.0,
                    span: Span::Arc { bulge: 1.0 },
                    smooth: false,
                },
                PathNode::line(10.0, 10.0),
            ],
            false,
        );
        assert_eq!(arc_neighbour.chamfer(1, 2.0), Err(CornerError::NotStraight));
        assert_eq!(arc_neighbour.fillet(1, 2.0), Err(CornerError::NotStraight));
        let chamfered = elbow().chamfer(1, 2.0).unwrap();
        assert_eq!(chamfered.chamfer(0, 1.0), Err(CornerError::End));
        assert_eq!(chamfered.chamfer(3, 1.0), Err(CornerError::End));
        assert_eq!(chamfered.chamfer(9, 1.0), Err(CornerError::End));
        assert_eq!(chamfered.chamfer(1, 0.0), Err(CornerError::NotPositive));
        assert_eq!(chamfered.fillet(1, f64::NAN), Err(CornerError::NotPositive));
        // The first side is 8 long now: a distance of 8 uses all of it.
        assert!(matches!(
            chamfered.chamfer(1, 8.0),
            Err(CornerError::TooLarge { .. })
        ));
        // A square corner takes any radius under its shorter side.
        let Err(CornerError::TooLarge { max_mm }) = elbow().fillet(1, 10.0) else {
            panic!("a radius that uses the whole side");
        };
        assert_approx_eq!(max_mm, 10.0, 1e-9);
        assert!(elbow().fillet(1, 9.99).is_ok());
        let straight = Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode::line(5.0, 0.0),
                PathNode::line(10.0, 0.0),
            ],
            false,
        );
        assert_eq!(straight.fillet(1, 1.0), Err(CornerError::Flat));
        let back = Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode::line(5.0, 0.0),
                PathNode::line(1.0, 0.0),
            ],
            false,
        );
        assert_eq!(back.chamfer(1, 1.0), Err(CornerError::Flat));
    }

    /// `testDeleteAnchorFromProfile`: the square without its second
    /// node is a triangle; no fewer than three nodes stay closed.
    #[test]
    fn delete_node_bridges_across_it() {
        let triangle = square().delete_node(1).unwrap();
        assert_eq!(points(&triangle), [(0.0, 0.0), (10.0, 10.0), (0.0, 10.0)]);
        assert!(triangle.is_closed());
        assert_eq!(triangle.delete_node(0), None, "three nodes are the least");
        // The first node of a closed path: the last node's span bridges.
        let without_first = square().delete_node(0).unwrap();
        assert_eq!(
            points(&without_first),
            [(10.0, 0.0), (10.0, 10.0), (0.0, 10.0)]
        );
        // An open path's ends just go; two nodes are the least.
        let open = elbow();
        assert_eq!(
            points(&open.delete_node(0).unwrap()),
            [(10.0, 0.0), (10.0, 10.0)]
        );
        assert_eq!(
            points(&open.delete_node(2).unwrap()),
            [(0.0, 0.0), (10.0, 0.0)]
        );
        assert_eq!(
            points(&open.delete_node(1).unwrap()),
            [(0.0, 0.0), (10.0, 10.0)]
        );
        assert_eq!(open.delete_node(1).unwrap().delete_node(0), None);
        assert_eq!(open.delete_node(7), None);
    }

    /// `buildBridgeSegment`: beside a curve the bridge is a curve that
    /// keeps the outer handles; an arc beside a deleted node is a line.
    #[test]
    fn deleting_beside_a_curve_keeps_the_outer_handles() {
        let curve = Path::spline(
            &[
                Point::new(0.0, 0.0),
                Point::new(10.0, 5.0),
                Point::new(20.0, 0.0),
                Point::new(30.0, 5.0),
            ],
            false,
        );
        let Span::Cubic { handle_out, .. } = curve.nodes()[0].span else {
            panic!("a cubic");
        };
        let Span::Cubic { handle_in, .. } = curve.nodes()[1].span else {
            panic!("a cubic");
        };
        let without = curve.delete_node(1).unwrap();
        assert_eq!(
            without.nodes()[0].span,
            Span::Cubic {
                handle_out,
                handle_in
            }
        );
        let with_arc = Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode {
                    x: 10.0,
                    y: 0.0,
                    span: Span::Arc { bulge: 0.5 },
                    smooth: false,
                },
                PathNode::line(20.0, 0.0),
                PathNode::line(20.0, 10.0),
            ],
            true,
        );
        assert_eq!(with_arc.delete_node(1).unwrap().nodes()[0].span, Span::Line);
    }

    /// `testDeleteSegmentFromProfile`: the square without its second
    /// side opens there, starting where that side ended.
    #[test]
    fn delete_span_opens_a_closed_path() {
        let (open, second) = square().delete_span(1).unwrap();
        assert_eq!(second, None);
        assert!(!open.is_closed());
        assert_eq!(
            points(&open),
            [(10.0, 10.0), (0.0, 10.0), (0.0, 0.0), (10.0, 0.0)]
        );
        // An open path splits in two; a part with no span is dropped.
        let line = Path::new(
            (0..5)
                .map(|i| PathNode::line(f64::from(i) * 10.0, 0.0))
                .collect(),
            false,
        );
        let (first, second) = line.delete_span(1).unwrap();
        assert_eq!(points(&first), [(0.0, 0.0), (10.0, 0.0)]);
        assert_eq!(
            points(&second.unwrap()),
            [(20.0, 0.0), (30.0, 0.0), (40.0, 0.0)]
        );
        let (first, second) = line.delete_span(0).unwrap();
        assert_eq!(first.nodes().len(), 4);
        assert_eq!(second, None);
        let one = Path::new(
            vec![PathNode::line(0.0, 0.0), PathNode::line(1.0, 0.0)],
            false,
        );
        assert_eq!(one.delete_span(0), None, "its only span");
    }

    /// `testDisconnectProfileAtAnchor`: the square broken at its third
    /// node opens there and ends where it starts.
    #[test]
    fn break_opens_a_closed_path_at_the_node() {
        let (open, second) = square().break_at(2).unwrap();
        assert_eq!(second, None);
        assert!(!open.is_closed());
        assert_eq!(
            points(&open),
            [
                (10.0, 10.0),
                (0.0, 10.0),
                (0.0, 0.0),
                (10.0, 0.0),
                (10.0, 10.0)
            ]
        );
        // An open path splits there; its ends have nothing to break.
        let (first, second) = elbow().break_at(1).unwrap();
        assert_eq!(points(&first), [(0.0, 0.0), (10.0, 0.0)]);
        assert_eq!(points(&second.unwrap()), [(10.0, 0.0), (10.0, 10.0)]);
        assert_eq!(elbow().break_at(0), None);
        assert_eq!(elbow().break_at(2), None);
    }

    /// Where a point is nearest: on a line, on an arc (by the arc, not
    /// its chord) and on a curve.
    #[test]
    fn nearest_finds_the_span_and_how_far_along() {
        let path = Path::new(
            vec![
                PathNode::line(0.0, 0.0),
                PathNode {
                    x: 10.0,
                    y: 0.0,
                    span: Span::Arc { bulge: 1.0 },
                    smooth: false,
                },
                cubic_node(10.0, 10.0, (10.0, 16.0), (4.0, 16.0)),
                PathNode::line(4.0, 10.0),
            ],
            false,
        );
        let hit = path.nearest(Point::new(4.0, 1.0)).unwrap();
        assert_eq!(hit.span, 0);
        assert_approx_eq!(hit.t, 0.4, 1e-9);
        assert_approx_eq!(hit.gap_mm, 1.0, 1e-9);
        // The half circle's rightmost point is (15, 5), halfway round.
        let hit = path.nearest(Point::new(17.0, 5.0)).unwrap();
        assert_eq!(hit.span, 1);
        assert_approx_eq!(hit.t, 0.5, 1e-9);
        assert_approx_eq!(hit.point.x, 15.0, 1e-9);
        assert_approx_eq!(hit.gap_mm, 2.0, 1e-9);
        let hit = path.nearest(Point::new(7.0, 20.0)).unwrap();
        assert_eq!(hit.span, 2);
        assert_approx_eq!(hit.t, 0.5, 1e-6);
        assert_approx_eq!(hit.point.y, 14.5, 1e-6);
        // Splitting where the point is nearest puts a node there.
        let split = path.split_span(hit.span, hit.t).unwrap();
        assert_approx_eq!(split.nodes()[3].y, 14.5, 1e-6);
        assert_eq!(Path::default().nearest(Point::new(0.0, 0.0)), None);
    }

    /// A span changes kind and keeps its ends. An arc made a curve stays
    /// within a hair of the arc; a curve made an arc passes through the
    /// curve's middle; anything made a line is the chord.
    #[test]
    fn span_kinds() {
        let line = elbow();
        let arc = line.with_span_kind(0, SpanKind::Arc).unwrap();
        assert_eq!(
            arc.nodes()[0].span,
            Span::Arc {
                bulge: NEW_ARC_BULGE
            }
        );
        assert_eq!(points(&arc), points(&line));
        let curve = line.with_span_kind(0, SpanKind::Cubic).unwrap();
        assert!(curve.has_curves());
        assert!(
            apart_mm(&curve, &line) < SAME_OUTLINE_MM,
            "a straight curve"
        );
        assert_eq!(curve.with_span_kind(0, SpanKind::Line).unwrap(), line);
        assert_eq!(arc.with_span_kind(0, SpanKind::Line).unwrap(), line);
        assert_eq!(arc.with_span_kind(0, SpanKind::Arc).unwrap(), arc);
        assert_eq!(line.with_span_kind(5, SpanKind::Arc), None);

        // A quarter-turn arc is one cubic; a three-quarter turn is three,
        // with two nodes added; each follows its arc closely.
        for (bulge, cubics) in [
            ((PI / 8.0).tan(), 1),
            (-1.0, 2),
            ((3.0 * PI / 8.0).tan(), 3),
        ] {
            let arc = Path::new(
                vec![
                    PathNode {
                        x: 0.0,
                        y: 0.0,
                        span: Span::Arc { bulge },
                        smooth: false,
                    },
                    PathNode::line(20.0, 0.0),
                ],
                false,
            );
            let curve = arc.with_span_kind(0, SpanKind::Cubic).unwrap();
            assert_eq!(curve.span_count(), cubics, "bulge {bulge}");
            assert_eq!(curve.nodes()[0].point(), Point::new(0.0, 0.0));
            assert_eq!(curve.nodes().last().unwrap().point(), Point::new(20.0, 0.0));
            let radius = arc.arc_radius_mm(0).unwrap();
            assert!(
                apart_mm(&curve, &arc) < radius * 5e-4 + SAME_OUTLINE_MM,
                "bulge {bulge}"
            );
            // And back: the arc through the cubic's middle is the arc.
            if cubics == 1 {
                let back = curve.with_span_kind(0, SpanKind::Arc).unwrap();
                let Span::Arc { bulge: again } = back.nodes()[0].span else {
                    panic!("an arc");
                };
                assert_approx_eq!(again, bulge, 1e-3);
            }
        }
    }

    /// Moving a node moves a curve's handles with it, and rebuilds an arc
    /// beside it through the point that was the arc's middle
    /// (`moveFeatureControl`).
    #[test]
    fn moving_a_node_carries_its_handles_and_rebuilds_its_arcs() {
        let moved = square().with_node_at(2, Point::new(12.0, 13.0)).unwrap();
        assert_eq!(
            points(&moved),
            [(0.0, 0.0), (10.0, 0.0), (12.0, 13.0), (0.0, 10.0)]
        );
        assert_eq!(square().with_node_at(4, Point::new(0.0, 0.0)), None);

        let curve = Path::spline(
            &[
                Point::new(0.0, 0.0),
                Point::new(10.0, 5.0),
                Point::new(20.0, 0.0),
            ],
            false,
        );
        let by = Vec2::new(3.0, -2.0);
        let moved = curve
            .with_node_at(1, curve.nodes()[1].point() + by)
            .unwrap();
        for (side, span) in [(HandleSide::In, 0), (HandleSide::Out, 1)] {
            let before = handle(&curve.nodes()[span], side).unwrap();
            let after = handle(&moved.nodes()[span], side).unwrap();
            assert_eq!(after, before + by, "{side:?}");
        }
        // The far handles stay.
        assert_eq!(
            handle(&moved.nodes()[0], HandleSide::Out),
            handle(&curve.nodes()[0], HandleSide::Out)
        );

        let with_arc = Path::new(
            vec![
                PathNode {
                    x: 0.0,
                    y: 0.0,
                    span: Span::Arc { bulge: 1.0 },
                    smooth: false,
                },
                PathNode {
                    x: 10.0,
                    y: 0.0,
                    span: Span::Arc { bulge: -0.5 },
                    smooth: false,
                },
                PathNode::line(20.0, 0.0),
            ],
            false,
        );
        let old_middles = [
            arc_middle(Point::new(0.0, 0.0), Point::new(10.0, 0.0), 1.0),
            arc_middle(Point::new(10.0, 0.0), Point::new(20.0, 0.0), -0.5),
        ];
        let to = Point::new(11.0, 2.0);
        let moved = with_arc.with_node_at(1, to).unwrap();
        let flat = moved.to_polyline(0.01);
        for (segment, middle) in flat.segments().zip(old_middles) {
            let gap = crate::point_segment_distance_mm(segment, middle);
            assert!(gap < 1e-9, "the arc passes through its old middle: {gap}");
        }
        // Moved into line with the other end and the old middle, an arc
        // has no circle left and is a line.
        let flatten = Path::new(
            vec![
                PathNode {
                    x: 0.0,
                    y: 0.0,
                    span: Span::Arc { bulge: 1.0 },
                    smooth: false,
                },
                PathNode::line(10.0, 0.0),
            ],
            false,
        );
        let straight = flatten.with_node_at(1, Point::new(10.0, -10.0)).unwrap();
        assert_eq!(straight.nodes()[0].span, Span::Line);
    }

    /// A smooth node's two handles stay in line when one is dragged, each
    /// keeping its length; dragged alone, the node becomes a corner.
    #[test]
    fn a_smooth_handle_carries_its_twin() {
        let curve = Path::spline(
            &[
                Point::new(0.0, 0.0),
                Point::new(10.0, 5.0),
                Point::new(20.0, 0.0),
            ],
            false,
        );
        let at = curve.nodes()[1].point();
        let twin_before = handle(&curve.nodes()[0], HandleSide::In).unwrap();
        let to = Point::new(10.0, 9.0);
        let dragged = curve.with_handle_at(1, HandleSide::Out, to, true).unwrap();
        assert_eq!(handle(&dragged.nodes()[1], HandleSide::Out), Some(to));
        let twin = handle(&dragged.nodes()[0], HandleSide::In).unwrap();
        // In line with the node and the dragged handle, on the far side.
        assert_approx_eq!((twin - at).cross(to - at), 0.0, 1e-9);
        assert!((twin - at).dot(to - at) < 0.0);
        assert_approx_eq!(twin.distance_to(at), twin_before.distance_to(at), 1e-9);
        assert!(dragged.nodes()[1].smooth);

        let alone = curve.with_handle_at(1, HandleSide::Out, to, false).unwrap();
        assert_eq!(handle(&alone.nodes()[0], HandleSide::In), Some(twin_before));
        assert!(!alone.nodes()[1].smooth, "a corner now");
        // A corner's handles move alone from then on.
        let again = alone
            .with_handle_at(1, HandleSide::In, Point::new(5.0, 9.0), true)
            .unwrap();
        assert_eq!(handle(&again.nodes()[1], HandleSide::Out), Some(to));
        // Made smooth again, the two come back into line.
        let smooth = again.with_node_smooth(1, true).unwrap();
        let (a, b) = (
            handle(&smooth.nodes()[0], HandleSide::In).unwrap(),
            handle(&smooth.nodes()[1], HandleSide::Out).unwrap(),
        );
        assert_approx_eq!((a - at).cross(b - at), 0.0, 1e-9);
        assert!((a - at).dot(b - at) < 0.0);
        // The end of an open path has one handle; a line has none.
        assert!(curve.with_handle_at(0, HandleSide::Out, to, true).is_some());
        assert_eq!(curve.with_handle_at(0, HandleSide::In, to, true), None);
        assert_eq!(elbow().with_handle_at(1, HandleSide::Out, to, true), None);
    }

    /// An arc dragged by its middle passes through where it is dragged
    /// to, its ends staying; typed, its radius is the radius, on the same
    /// side and the same way round.
    #[test]
    fn an_arc_is_reshaped_by_its_middle_or_its_radius() {
        let arc = elbow().with_span_kind(0, SpanKind::Arc).unwrap();
        let through = Point::new(5.0, -5.0);
        let dragged = arc.with_arc_through(0, through).unwrap();
        assert_eq!(points(&dragged), points(&arc));
        assert_approx_eq!(dragged.arc_radius_mm(0).unwrap(), 5.0, 1e-9);
        let Span::Arc { bulge } = dragged.nodes()[0].span else {
            panic!("an arc");
        };
        assert_approx_eq!(bulge, 1.0, 1e-9);
        assert_eq!(
            arc.with_arc_through(0, Point::new(3.0, 0.0))
                .unwrap()
                .nodes()[0]
                .span,
            Span::Line
        );
        assert_eq!(elbow().arc_radius_mm(0), None);
        assert_eq!(elbow().with_arc_radius(0, 10.0), None);

        for bulge in [0.3, -0.3, 1.8, -1.8] {
            let mut nodes = elbow().nodes().to_vec();
            nodes[0].span = Span::Arc { bulge };
            let arc = Path::new(nodes, false);
            let wide = arc.with_arc_radius(0, 20.0).unwrap();
            assert_approx_eq!(wide.arc_radius_mm(0).unwrap(), 20.0, 1e-9);
            let Span::Arc { bulge: after } = wide.nodes()[0].span else {
                panic!("an arc");
            };
            assert_eq!(after > 0.0, bulge > 0.0, "the same side");
            assert_eq!(after.abs() > 1.0, bulge.abs() > 1.0, "the same way round");
            // Half the chord is the least a radius can be.
            assert_eq!(arc.with_arc_radius(0, 4.99), None);
            assert!(arc.with_arc_radius(0, 5.0).is_some());
        }
    }

    /// Reversed, a path is the same outline run the other way, and
    /// reversed again it is itself.
    #[test]
    fn reversed_runs_the_other_way() {
        let mixed = Path::new(
            vec![
                cubic_node(0.0, 0.0, (5.0, 8.0), (15.0, 8.0)),
                PathNode {
                    x: 20.0,
                    y: 0.0,
                    span: Span::Arc { bulge: 0.6 },
                    smooth: false,
                },
                PathNode::line(20.0, -10.0),
                PathNode::line(0.0, -10.0),
            ],
            false,
        );
        for closed in [false, true] {
            let path = Path::new(mixed.nodes().to_vec(), closed);
            let back = path.reversed();
            assert_eq!(back.is_closed(), closed);
            assert_eq!(back.nodes()[0].point(), path.nodes()[3].point());
            assert!(apart_mm(&back, &path) < SAME_OUTLINE_MM, "closed: {closed}");
            let (a, b) = (
                path.to_polyline(0.001).signed_area_mm2(),
                back.to_polyline(0.001).signed_area_mm2(),
            );
            if closed {
                assert_approx_eq!(a, -b, 1e-3);
            }
            let again = back.reversed();
            assert!(apart_mm(&again, &path) < SAME_OUTLINE_MM);
            assert_eq!(points(&again), points(&path));
        }
    }
}
