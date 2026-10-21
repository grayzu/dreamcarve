// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/tabs.ts

//! Holding tabs on Profile passes (`docs/specs/phase-1/design.md`,
//! "Algorithms → Profile contour", step 7; `docs/specs/phase-2/design.md`,
//! "Placed tabs").
//!
//! PureCutCNC places tabs as rectangles in the world and grows each by the
//! tool radius before clipping the path against it (`toolCentreContours`,
//! `expandedTabFootprints`). DreamCarve places a tab on one outline of
//! one instance (#105, ADR-0005, [`crate::placed_tabs`]), and centres it
//! on the tool path at the path's point nearest the tab, so the growth
//! becomes a span of `width + diameter` along the path: the cutter's full
//! width must clear the tab.
//!
//! A smooth tab leaves a triangular bridge: base `width`, peak `height`
//! above the cut floor (#66). For a flat bit of radius r the tool centre
//! holds the tab top for `2r` centred on the tab and ramps straight down
//! over `width / 2` on each side, so the span is still `width + 2r`.
//! PureCutCNC's raised cosine (`tabProfile.ts`) over the same span touches
//! the tab top at a single point, which leaves only about 60% of the height
//! under a 6.35 mm bit; it is not ported.
//!
//! Each tab has its own width, height and shape, as PureCutCNC's do; a
//! pass rises over a tab only when it is below that tab's top. Spans that
//! overlap merge into one, which holds the highest top and is smooth only
//! when all its tabs are.
//!
//! A tab request ([`TabSpec`], [`Request`]) lays tabs per tool-centre
//! path ([`Request::fit`], #109): a hole gets none unless
//! [`TabSpec::on_holes`], and a path whose spans would cover more than
//! half of it gets the most that cover at most half, so it cuts free
//! between its tabs. A part's outline keeps at least one tab even then
//! ([`Least::One`]): a loose part can be thrown by the bit. PureCutCNC
//! sizes tabs per feature in its auto-tab command (`autoTabs.ts`, a 15%
//! free-path rule); that command writes placed tabs and waits for the
//! GUI. The half-path threshold here is the coverage warning's.

use crate::entry::plunge_limited_feed_mm_min;
use crate::error::{CamError, ShapeRef};
use crate::toolpath::{Move, MoveKind};
use crate::warning::{TabsRangeReason, Warning};
use dc_geometry::{EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Point3, Polyline, Segment};
use dc_project::{OperationId, TabPlacement, TabShape, TabSpec};

/// Most chords one arc piece of a smooth ramp is split into. Bounds the
/// work a tiny chord tolerance can cause.
const MAX_ARC_CHORDS: u32 = 1024;

/// Distances along a closed path of length `path_length_mm` at which tabs
/// are centred, each in `[0, path_length_mm)`, in placement order.
///
/// `Count(n)` spaces `n` tabs `L/n` apart starting at `L/(2n)`, so no tab
/// straddles the path start unless tabs overlap. `AtDistances` takes each
/// value modulo `L`. A non-positive `L` gives no tabs.
#[must_use]
pub fn tab_centres_mm(placement: &TabPlacement, path_length_mm: f64) -> Vec<f64> {
    if path_length_mm.is_nan() || path_length_mm <= EPS_MM {
        return Vec::new();
    }
    match placement {
        TabPlacement::Count { count } => {
            let n = f64::from(*count);
            (0..*count)
                .map(|k| path_length_mm * (2.0 * f64::from(k) + 1.0) / (2.0 * n))
                .collect()
        }
        TabPlacement::AtDistances { distances_mm } => distances_mm
            .iter()
            .map(|d| d.rem_euclid(path_length_mm))
            .collect(),
    }
}

/// The feed rates a lap is cut at, exactly as the user set them on the
/// tool.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LapRates {
    /// XY cutting feed, mm/min.
    pub feed_mm_min: f64,
    /// Descent feed, mm/min; also used for tab lifts.
    pub plunge_mm_min: f64,
}

/// One tab as one tool path cuts it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TabCut {
    /// Where the tab is centred along the path, mm: from the start of a
    /// closed path, from the first vertex as drawn of an open one.
    pub centre_mm: f64,
    /// Tab width, mm.
    pub width_mm: f64,
    /// Tab top above the stock bottom, mm.
    pub top_z_mm: f64,
    /// Tab cross-section.
    pub shape: TabShape,
}

/// What every tab one operation cuts shares: the tool that cuts them, the
/// chord tolerance of smooth ramps on arcs, and the highest tab top, below
/// which a pass is "below the tabs" for entries and leads.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tabs {
    /// The highest tab top of the operation's tabs.
    top_z_mm: f64,
    /// Diameter of the tool that cuts the tabs.
    diameter_mm: f64,
    chord_tolerance_mm: f64,
}

/// One operation's tab request, checked (#105: a [`TabSpec`] places
/// tabs; it is not cut data).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Request {
    placement: TabPlacement,
    shape: TabShape,
    top_z_mm: f64,
    /// Width of one tab as left on the part.
    width_mm: f64,
    /// Length of path one tab raises: tab width plus tool diameter.
    span_mm: f64,
    /// Holes get tabs too (#109).
    on_holes: bool,
}

/// Which way a path runs, for laying tabs on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathEnds {
    /// A closed path: tabs are measured from its start, and a span may
    /// run through it.
    Closed,
    /// An open path, tabs measured from its first vertex as drawn and
    /// each span cut short at its ends (#175). `reversed`: it is cut from
    /// its last vertex.
    Open {
        /// The path is cut from its last vertex.
        reversed: bool,
    },
}

/// The fewest tabs [`Request::fit`] may leave on a path that asked for
/// some.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Least {
    /// A part's outline: cut free, a small part can be thrown by the bit,
    /// so it keeps one tab even when that covers most of it.
    One,
    /// A hole (with `on_holes`) or an open path: its slug or slot may
    /// come loose, so it may get none.
    Zero,
}

/// The tabs one path asked for and the tabs cut on it, when they cover
/// more than half of it ([`Request::fit`], #109).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reduced {
    /// How many tabs the placement puts on the path.
    pub asked: usize,
    /// How many are cut.
    pub cut: usize,
}

/// Fails with [`CamError::InvalidTab`] for operation `op` when a tab's
/// width or height, named `field` and `value_mm`, is not a positive
/// number.
pub(crate) fn check_size(op: &OperationId, field: &str, value_mm: f64) -> Result<(), CamError> {
    if value_mm.is_finite() && value_mm > 0.0 {
        Ok(())
    } else {
        Err(CamError::InvalidTab {
            op: op.clone(),
            field: field.to_owned(),
            value_mm,
        })
    }
}

impl Tabs {
    /// The tabs of an operation cut with a tool `diameter_mm` wide whose
    /// highest tab top is `top_z_mm`. `chord_tolerance_mm` bounds how far
    /// the chords of a smooth ramp on an arc stray from the path in XY; a
    /// value that is not positive falls back to
    /// [`FLATTEN_TOLERANCE_DEFAULT_MM`].
    pub(crate) fn new(top_z_mm: f64, diameter_mm: f64, chord_tolerance_mm: f64) -> Self {
        Self {
            top_z_mm,
            diameter_mm,
            chord_tolerance_mm: if chord_tolerance_mm > 0.0 {
                chord_tolerance_mm
            } else {
                FLATTEN_TOLERANCE_DEFAULT_MM
            },
        }
    }

    /// The raised spans of `cuts` on a path `path_length_mm` long, and
    /// whether any of them overlapped (or touched, or one alone covers the
    /// whole path), which merges them.
    ///
    /// Each tab raises its `width` plus the tool diameter of path centred
    /// on it (PureCutCNC grows each tab footprint by the tool radius on
    /// every side; along the path that is the same length). On a closed
    /// path spans are circular: one may run through the path start. On an
    /// open path ([`PathEnds::Open`], #175) they are measured from its
    /// first vertex as drawn and cut short at its ends; `reversed`, the
    /// path is cut from its last vertex, and the spans are measured from
    /// there, so the tabs stay where they were drawn.
    pub(crate) fn layout(
        &self,
        cuts: &[TabCut],
        path_length_mm: f64,
        ends: PathEnds,
    ) -> (PathTabs, bool) {
        match ends {
            PathEnds::Closed => self.layout_closed(cuts, path_length_mm),
            PathEnds::Open { reversed } => self.layout_open(cuts, path_length_mm, reversed),
        }
    }

    /// The span `cut` raises on a closed path `path_length_mm` long, its
    /// start in `[0, L)` and its end possibly beyond `L`.
    fn closed_span(&self, cut: &TabCut, path_length_mm: f64) -> Raw {
        let span_mm = cut.width_mm + self.diameter_mm;
        let start_mm = (cut.centre_mm - span_mm / 2.0).rem_euclid(path_length_mm);
        Raw::of(cut, start_mm, start_mm + span_mm)
    }

    fn layout_closed(&self, cuts: &[TabCut], path_length_mm: f64) -> (PathTabs, bool) {
        let l = path_length_mm;
        let mut layout = PathTabs {
            length_mm: l,
            spans: Vec::new(),
            full: false,
            closed: true,
            tab_count: cuts.len(),
        };
        if cuts.is_empty() {
            return (layout, false);
        }
        let mut overlap = cuts
            .iter()
            .any(|cut| cut.width_mm + self.diameter_mm >= l - EPS_MM);
        let mut spans: Vec<Raw> = cuts.iter().map(|cut| self.closed_span(cut, l)).collect();
        spans.sort_by(|a, b| a.start_mm.total_cmp(&b.start_mm));

        // Spans whose end may lie beyond L, merged in order.
        let mut merged: Vec<Raw> = Vec::new();
        for span in spans {
            match merged.last_mut() {
                Some(last) if span.start_mm <= last.end_mm + EPS_MM => {
                    overlap = true;
                    last.absorb(&span);
                }
                _ => merged.push(span),
            }
        }
        // The last span may run past L into the first.
        while merged.len() > 1 {
            let (Some(first), Some(last)) = (merged.first().copied(), merged.last()) else {
                break;
            };
            if last.end_mm - l < first.start_mm - EPS_MM {
                break;
            }
            overlap = true;
            merged.remove(0);
            if let Some(last) = merged.last_mut() {
                last.absorb(&Raw {
                    start_mm: first.start_mm + l,
                    end_mm: first.end_mm + l,
                    ..first
                });
            }
        }
        layout.full = merged.iter().any(|s| s.end_mm - s.start_mm >= l - EPS_MM);
        layout.spans = merged.iter().map(Raw::span).collect();
        (layout, overlap)
    }

    fn layout_open(
        &self,
        cuts: &[TabCut],
        path_length_mm: f64,
        reversed: bool,
    ) -> (PathTabs, bool) {
        let l = path_length_mm;
        let mut layout = PathTabs {
            length_mm: l,
            spans: Vec::new(),
            full: false,
            closed: false,
            tab_count: cuts.len(),
        };
        let mut spans: Vec<Raw> = cuts
            .iter()
            .map(|cut| {
                let half = (cut.width_mm + self.diameter_mm) / 2.0;
                Raw::of(
                    cut,
                    (cut.centre_mm - half).max(0.0),
                    (cut.centre_mm + half).min(l),
                )
            })
            .collect();
        spans.sort_by(|a, b| a.start_mm.total_cmp(&b.start_mm));
        let mut overlap = false;
        let mut merged: Vec<Raw> = Vec::new();
        for span in spans {
            match merged.last_mut() {
                Some(last) if span.start_mm <= last.end_mm + EPS_MM => {
                    overlap = true;
                    last.absorb(&span);
                }
                _ => merged.push(span),
            }
        }
        layout.full = merged
            .iter()
            .any(|s| s.start_mm <= EPS_MM && s.end_mm >= l - EPS_MM);
        overlap |= layout.full;
        layout.spans = merged
            .iter()
            .map(|s| {
                let span = s.span();
                if reversed {
                    Span {
                        start_mm: l - s.end_mm,
                        ramp_in_mm: span.ramp_out_mm,
                        ramp_out_mm: span.ramp_in_mm,
                        ..span
                    }
                } else {
                    span
                }
            })
            .collect();
        if reversed {
            layout.spans.reverse();
        }
        (layout, overlap)
    }

    /// The highest tab top, mm above the stock bottom: a pass below it
    /// rises over some tab.
    pub(crate) fn top_z_mm(&self) -> f64 {
        self.top_z_mm
    }
}

impl Request {
    /// Checks `spec` for operation `op` cut with a tool `diameter_mm` wide.
    ///
    /// Fails with [`CamError::InvalidTab`] when the width or height is not
    /// a positive number or a distance is not finite.
    pub(crate) fn new(
        op: &OperationId,
        spec: &TabSpec,
        diameter_mm: f64,
    ) -> Result<Self, CamError> {
        check_size(op, "width_mm", spec.width_mm)?;
        check_size(op, "height_mm", spec.height_mm)?;
        if let TabPlacement::AtDistances { distances_mm } = &spec.placement
            && let Some((i, d)) = distances_mm
                .iter()
                .enumerate()
                .find(|(_, d)| !d.is_finite())
        {
            return Err(CamError::InvalidTab {
                op: op.clone(),
                field: format!("distances_mm[{i}]"),
                value_mm: *d,
            });
        }
        Ok(Self {
            placement: spec.placement.clone(),
            shape: spec.shape,
            top_z_mm: spec.height_mm,
            width_mm: spec.width_mm,
            span_mm: spec.width_mm + diameter_mm,
            on_holes: spec.on_holes,
        })
    }

    /// Whether holes get tabs too ([`TabSpec::on_holes`]).
    pub(crate) fn on_holes(&self) -> bool {
        self.on_holes
    }

    /// The tab top the request asks for, mm above the stock bottom.
    pub(crate) fn top_z_mm(&self) -> f64 {
        self.top_z_mm
    }

    /// The tab width the request asks for, mm.
    pub(crate) fn width_mm(&self) -> f64 {
        self.width_mm
    }

    /// A tab of this request centred at `centre_mm`.
    fn cut(&self, centre_mm: f64) -> TabCut {
        TabCut {
            centre_mm,
            width_mm: self.width_mm,
            top_z_mm: self.top_z_mm,
            shape: self.shape,
        }
    }

    /// The tabs this request puts on a path `path_length_mm` long, laid
    /// out by `tabs` ([`Tabs::layout`]). When their spans would cover more
    /// than half the path, so the part or hole would not cut free, the
    /// path gets the most tabs that cover at most half (#109), but never
    /// fewer than `least` allows: `count` tabs re-spaced evenly, or the
    /// first of `at_distances` in their order. What was asked and what is
    /// cut are in [`Reduced`], which is given whenever the asked tabs
    /// covered more than half, even when one kept tab is all that was
    /// asked.
    pub(crate) fn fit(
        &self,
        tabs: &Tabs,
        path_length_mm: f64,
        ends: PathEnds,
        least: Least,
    ) -> (Vec<TabCut>, Option<Reduced>) {
        let l = path_length_mm;
        let half_mm = l / 2.0 + EPS_MM;
        let cuts = |centres: Vec<f64>| -> Vec<TabCut> {
            centres.into_iter().map(|c| self.cut(c)).collect()
        };
        let covered = |cuts: &[TabCut]| tabs.layout(cuts, l, ends).0.covered_mm();
        let centres = tab_centres_mm(&self.placement, l);
        let asked = centres.len();
        let all = cuts(centres.clone());
        if covered(&all) <= half_mm {
            return (all, None);
        }
        // `asked` is at least 1 here: no tabs cover nothing.
        let fewest = match least {
            Least::One => 1,
            Least::Zero => 0,
        };
        let kept = match &self.placement {
            // Evenly spaced spans that fit in half the path never overlap.
            TabPlacement::Count { count } => {
                let fits = (0..*count)
                    .rev()
                    .find(|&n| f64::from(n) * self.span_mm <= half_mm)
                    .unwrap_or(0)
                    .max(fewest);
                cuts(tab_centres_mm(&TabPlacement::Count { count: fits }, l))
            }
            TabPlacement::AtDistances { .. } => {
                let first = |k: usize| cuts(centres.iter().take(k).copied().collect());
                (usize::from(fewest)..asked)
                    .rev()
                    .map(first)
                    .find(|kept| covered(kept) <= half_mm)
                    .unwrap_or_else(|| first(usize::from(fewest)))
            }
        };
        let cut = kept.len();
        (kept, Some(Reduced { asked, cut }))
    }
}

/// One raised stretch of a path, after merging.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    /// Distance along the path where the span starts, in `[0, L)`.
    start_mm: f64,
    /// Length of the span; `start_mm + length_mm` may exceed `L`.
    length_mm: f64,
    /// The highest tab top in the span.
    top_z_mm: f64,
    /// Smooth only when every tab in the span is.
    shape: TabShape,
    /// The length a smooth span rises over at its start: half the width
    /// of its first tab, so the bridge left is a triangle that wide at its
    /// base.
    ramp_in_mm: f64,
    /// The length a smooth span falls over at its end: half the width of
    /// its last tab.
    ramp_out_mm: f64,
}

impl Span {
    /// Where the span ends, unwrapped.
    fn end_mm(&self) -> f64 {
        self.start_mm + self.length_mm
    }
}

/// A span while spans are merged, by its ends.
#[derive(Debug, Clone, Copy)]
struct Raw {
    start_mm: f64,
    end_mm: f64,
    top_z_mm: f64,
    shape: TabShape,
    ramp_in_mm: f64,
    ramp_out_mm: f64,
}

impl Raw {
    /// The span of `cut` from `start_mm` to `end_mm`.
    fn of(cut: &TabCut, start_mm: f64, end_mm: f64) -> Self {
        let ramp_mm = cut.width_mm / 2.0;
        Self {
            start_mm,
            end_mm,
            top_z_mm: cut.top_z_mm,
            shape: cut.shape,
            ramp_in_mm: ramp_mm,
            ramp_out_mm: ramp_mm,
        }
    }

    /// Merges `next`, which starts at or before this span's end, into it.
    fn absorb(&mut self, next: &Raw) {
        if next.end_mm > self.end_mm {
            self.end_mm = next.end_mm;
            self.ramp_out_mm = next.ramp_out_mm;
        }
        self.top_z_mm = self.top_z_mm.max(next.top_z_mm);
        if next.shape == TabShape::Rect {
            self.shape = TabShape::Rect;
        }
    }

    fn span(&self) -> Span {
        Span {
            start_mm: self.start_mm,
            length_mm: self.end_mm - self.start_mm,
            top_z_mm: self.top_z_mm,
            shape: self.shape,
            ramp_in_mm: self.ramp_in_mm,
            ramp_out_mm: self.ramp_out_mm,
        }
    }
}

/// The merged tab spans on one tool-centre path.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PathTabs {
    length_mm: f64,
    spans: Vec<Span>,
    /// The spans cover the whole path.
    full: bool,
    /// The path is closed, so a span may run through its start. An open
    /// path's spans lie within it ([`Tabs::layout`]).
    closed: bool,
    /// How many tabs the spans were laid for, before merging.
    tab_count: usize,
}

/// Where a point along a tabbed path lies.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Zone {
    /// Outside every span, or in one whose top the pass is not below: the
    /// pass Z.
    Clear,
    /// On a smooth ramp, `f` of the way up from the pass Z (0) to the tab
    /// top `top_z_mm` (1).
    Ramp(f64, f64),
    /// At the tab top `top_z_mm`.
    Hold(f64),
}

impl PathTabs {
    /// The same spans measured from `start_mm` along the path instead of
    /// from its start: the layout of the path re-seamed there, so the
    /// tabs stay where they are along the outline.
    pub(crate) fn reseamed(&self, start_mm: f64) -> Self {
        let l = self.length_mm;
        Self {
            length_mm: l,
            spans: self
                .spans
                .iter()
                .map(|s| Span {
                    start_mm: if l > 0.0 {
                        (s.start_mm - start_mm).rem_euclid(l)
                    } else {
                        s.start_mm
                    },
                    ..*s
                })
                .collect(),
            full: self.full,
            closed: self.closed,
            tab_count: self.tab_count,
        }
    }

    /// True when no tab stands on the path: its last pass cuts free.
    pub(crate) fn is_untabbed(&self) -> bool {
        self.spans.is_empty() && !self.full
    }

    /// How many tabs the spans were laid for, before merging.
    pub(crate) fn tab_count(&self) -> usize {
        self.tab_count
    }

    /// How much of the path the spans raise, mm: at most its length.
    pub(crate) fn covered_mm(&self) -> f64 {
        if self.full {
            self.length_mm
        } else {
            self.spans
                .iter()
                .map(|s| s.length_mm)
                .sum::<f64>()
                .min(self.length_mm)
        }
    }

    /// How far along the path from its start the first raised span
    /// begins, mm: the stretch a pass below the tab top cuts at its own Z
    /// before the first tab. 0 when a span covers the start; the path
    /// length when there are no spans.
    pub(crate) fn clear_from_start_mm(&self) -> f64 {
        if self.full {
            return 0.0;
        }
        let mut clear = self.length_mm;
        for span in &self.spans {
            let wraps = self.closed && span.end_mm() >= self.length_mm - EPS_MM;
            if span.start_mm <= EPS_MM || wraps {
                return 0.0;
            }
            clear = clear.min(span.start_mm);
        }
        clear
    }

    /// The length of the path the spans are laid on, mm.
    pub(crate) fn length_mm(&self) -> f64 {
        self.length_mm
    }

    /// The stretches of a closed path no span raises, as `(start, length)`
    /// mm along it, the start in `[0, L)` and a stretch possibly running
    /// through the path start: longest first, then nearest the start.
    /// Empty when the spans cover the path, or the path is open; the whole
    /// path from its start when there are no spans.
    pub(crate) fn gaps_mm(&self) -> Vec<(f64, f64)> {
        let l = self.length_mm;
        if self.full || !self.closed || l <= EPS_MM {
            return Vec::new();
        }
        let mut spans = self.spans.clone();
        spans.sort_by(|a, b| a.start_mm.total_cmp(&b.start_mm));
        let mut gaps: Vec<(f64, f64)> = spans
            .iter()
            .zip(spans.iter().cycle().skip(1))
            .map(|(a, b)| {
                let end = a.end_mm();
                let length = (b.start_mm - end).rem_euclid(l);
                (end.rem_euclid(l), length)
            })
            .filter(|(_, length)| *length > EPS_MM)
            .collect();
        if spans.is_empty() {
            gaps.push((0.0, l));
        }
        gaps.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.total_cmp(&b.0)));
        gaps
    }

    /// Where along the path the tabs' material stands, mm, as `(from, to)`
    /// with `to` possibly beyond the path length (a stretch through the
    /// start): each raised span less a tool radius at either end, since the
    /// tool rises a radius before the material and comes down a radius
    /// after it. The whole path when the spans cover it.
    pub(crate) fn stretches_mm(&self, tabs: &Tabs) -> Vec<(f64, f64)> {
        if self.full {
            return vec![(0.0, self.length_mm)];
        }
        let radius = tabs.diameter_mm / 2.0;
        self.spans
            .iter()
            .filter(|s| s.length_mm > 2.0 * radius)
            .map(|s| (s.start_mm + radius, s.end_mm() - radius))
            .collect()
    }

    /// The zone at `distance_mm` along the path for a pass at `base_z_mm`.
    fn zone(&self, distance_mm: f64, base_z_mm: f64) -> Zone {
        if self.full {
            let top = self
                .spans
                .iter()
                .map(|s| s.top_z_mm)
                .fold(f64::NEG_INFINITY, f64::max);
            return if base_z_mm < top - EPS_MM {
                Zone::Hold(top)
            } else {
                Zone::Clear
            };
        }
        for span in &self.spans {
            let t = if self.closed {
                (distance_mm - span.start_mm).rem_euclid(self.length_mm)
            } else {
                distance_mm - span.start_mm
            };
            if !(0.0..span.length_mm).contains(&t) {
                continue;
            }
            if base_z_mm >= span.top_z_mm - EPS_MM {
                return Zone::Clear;
            }
            let top = span.top_z_mm;
            return match span.shape {
                TabShape::Rect => Zone::Hold(top),
                // A smooth span rises over its first half tab width and
                // falls over its last, holding the top between: one bit
                // diameter on a single tab, longer on a merged span.
                TabShape::Smooth if t < span.ramp_in_mm => Zone::Ramp(t / span.ramp_in_mm, top),
                TabShape::Smooth if t > span.length_mm - span.ramp_out_mm => {
                    Zone::Ramp((span.length_mm - t) / span.ramp_out_mm, top)
                }
                TabShape::Smooth => Zone::Hold(top),
            };
        }
        Zone::Clear
    }

    /// Z at `distance_mm` along the path for a pass at `base_z_mm`.
    fn z_at(&self, distance_mm: f64, base_z_mm: f64) -> f64 {
        match self.zone(distance_mm, base_z_mm) {
            Zone::Clear => base_z_mm,
            Zone::Ramp(f, top) => base_z_mm + (top - base_z_mm) * f,
            Zone::Hold(top) => top,
        }
    }

    /// Distances where a lap at `base_z_mm` must have a vertex: the
    /// boundaries of the spans it rises over and, for smooth ones, the
    /// ends of each straight ramp.
    fn breakpoints_mm(&self, base_z_mm: f64) -> Vec<f64> {
        if self.full {
            return Vec::new();
        }
        let mut out = Vec::new();
        for span in self
            .spans
            .iter()
            .filter(|s| base_z_mm < s.top_z_mm - EPS_MM)
        {
            let end = span.end_mm();
            out.push(span.start_mm);
            out.push(end);
            if span.shape == TabShape::Smooth {
                out.push(span.start_mm + span.ramp_in_mm);
                out.push(end - span.ramp_out_mm);
            }
        }
        if self.closed {
            for d in &mut out {
                *d = d.rem_euclid(self.length_mm);
            }
        }
        out
    }
}

/// The warnings about where the tab tops lie relative to the cut
/// (PureCutCNC `applyTabWarnings`), for an operation whose tabs have tops
/// `tops_z_mm`: [`Warning::TabsOutsideCutRange`] when no pass of
/// `pass_z_mm` lies below the lowest tab top, so that tab changes
/// nothing, or when the highest tab top is above the stock top
/// `thickness_mm`.
pub(crate) fn cut_range_warnings(
    op: &OperationId,
    tops_z_mm: &[f64],
    pass_z_mm: &[f64],
    thickness_mm: f64,
) -> Vec<Warning> {
    let Some(deepest_z_mm) = pass_z_mm.iter().copied().reduce(f64::min) else {
        return Vec::new();
    };
    let (Some(lowest_mm), Some(highest_mm)) = (
        tops_z_mm.iter().copied().reduce(f64::min),
        tops_z_mm.iter().copied().reduce(f64::max),
    ) else {
        return Vec::new();
    };
    let warning = |reason, tab_top_mm| Warning::TabsOutsideCutRange {
        op: op.clone(),
        reason,
        tab_top_mm,
        deepest_pass_z_mm: deepest_z_mm,
        stock_top_mm: thickness_mm,
    };
    let mut warnings = Vec::new();
    if highest_mm > thickness_mm + EPS_MM {
        warnings.push(warning(TabsRangeReason::AboveStockTop, highest_mm));
    }
    if deepest_z_mm >= lowest_mm - EPS_MM {
        warnings.push(warning(TabsRangeReason::CutStopsAboveTab, lowest_mm));
    }
    warnings
}

/// The warning that tab spans on contour `contour` (0-based, in cut order)
/// overlapped and were merged.
pub(crate) fn overlap_warning(
    op: &OperationId,
    contour: usize,
    shape: Option<&ShapeRef>,
    path_length_mm: f64,
) -> Warning {
    Warning::TabsOverlap {
        op: op.clone(),
        contour,
        shape: shape.cloned(),
        path_length_mm,
    }
}

/// [`Warning::TabsCoverMostOfContour`] for contour `contour` (0-based, in
/// cut order), `path_length_mm` long and cut with a tool `diameter_mm`
/// wide, on which `reduced.asked` tabs of widths `widths_mm` (one per tab
/// asked) would have covered more than half, so the part or hole would
/// not cut free, and `reduced.cut` are cut: fewer when a request was cut
/// down ([`Request::fit`], #109), all when placed tabs are cut as placed.
pub(crate) fn coverage_warning(
    op: &OperationId,
    contour: usize,
    shape: Option<&ShapeRef>,
    (diameter_mm, widths_mm): (f64, &[f64]),
    path_length_mm: f64,
    reduced: Reduced,
) -> Warning {
    let asked_f: f64 = (0..reduced.asked).map(|_| 1.0).sum();
    let width_mm = widths_mm.iter().copied().fold(0.0, f64::max);
    let total_mm: f64 = widths_mm.iter().map(|w| w + diameter_mm).sum();
    // Rounded down to 0.01 mm so the suggested width does fit.
    let max_width_mm = ((path_length_mm / 2.0 / asked_f - diameter_mm) * 100.0).floor() / 100.0;
    Warning::TabsCoverMostOfContour {
        op: op.clone(),
        contour,
        shape: shape.cloned(),
        path_length_mm,
        tab_count: reduced.asked,
        tab_width_mm: width_mm,
        diameter_mm,
        tab_span_mm: width_mm + diameter_mm,
        tab_total_mm: total_mm,
        cut_count: reduced.cut,
        max_width_mm: (max_width_mm > 0.0).then_some(max_width_mm),
    }
}

/// One move of a lap, before the vertical step that may precede it.
#[derive(Debug, Clone, Copy)]
struct Piece {
    /// Z the move must start at.
    from_z_mm: f64,
    kind: MoveKind,
    to: Point3,
}

/// One lap of a path at one pass, as the cuts of a [`crate::CutBlock`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LapCuts {
    /// The Z the lap starts at: the pass Z, or the tab top when a tab span
    /// covers the path start.
    pub start_z_mm: f64,
    /// The lap's moves from the start point at `start_z_mm` back to the
    /// start point (to the last vertex of an open path).
    pub moves: Vec<Move>,
}

/// One lap of `path` at `z_mm`, from the path start at
/// [`LapCuts::start_z_mm`] back to the path start (to its last vertex for
/// an open path, #175). Where `tabs` is given
/// and `z_mm` lies below a tab's top, the lap rises over that tab's span:
/// rect tabs with a vertical lift, a traverse at the tab top, and a
/// vertical descent; smooth tabs along straight ramps either side of a hold
/// at the tab top. Every vertical move is at the plunge rate; every move
/// along the path is at the feed rate, except a smooth ramp down, whose
/// feed is scaled so it descends no faster than the plunge rate (S10,
/// #157). `None` for a path with no vertices.
pub(crate) fn lap_cuts(
    path: &Polyline,
    z_mm: f64,
    tabs: Option<(&Tabs, &PathTabs)>,
    rates: LapRates,
) -> Option<LapCuts> {
    let start = path.vertices().first().map(|v| v.point())?;
    let tabs = tabs.filter(|(t, _)| z_mm < t.top_z_mm - EPS_MM);
    let pieces = lap_pieces(path, z_mm, tabs);
    let start_z_mm = pieces.first().map_or(z_mm, |p| p.from_z_mm);
    let mut current = Point3::new(start.x, start.y, start_z_mm);
    let mut moves = Vec::with_capacity(pieces.len());
    for piece in pieces {
        if (current.z - piece.from_z_mm).abs() > EPS_MM {
            current = Point3::new(current.x, current.y, piece.from_z_mm);
            moves.push(Move::linear(current, rates.plunge_mm_min));
        }
        // A ramp is straight (an arc is split into chords while Z
        // changes), so the straight-move scale covers every descent.
        let feed_mm_min = match piece.kind {
            MoveKind::Linear => plunge_limited_feed_mm_min(
                rates.feed_mm_min,
                rates.plunge_mm_min,
                current,
                piece.to,
            ),
            _ => rates.feed_mm_min,
        };
        moves.push(Move {
            kind: piece.kind,
            to: piece.to,
            feed_mm_min: Some(feed_mm_min),
        });
        current = piece.to;
    }
    Some(LapCuts { start_z_mm, moves })
}

/// The moves of one lap at `z_mm`, split at tab breakpoints.
fn lap_pieces(path: &Polyline, z_mm: f64, tabs: Option<(&Tabs, &PathTabs)>) -> Vec<Piece> {
    let segments: Vec<Segment> = path
        .segments()
        .filter(|s| s.start.distance_to(s.end) > EPS_MM)
        .collect();
    let Some((tabs, layout)) = tabs else {
        return segments
            .iter()
            .map(|s| Piece {
                from_z_mm: z_mm,
                kind: segment_kind(s),
                to: Point3::new(s.end.x, s.end.y, z_mm),
            })
            .collect();
    };
    let mut breakpoints = layout.breakpoints_mm(z_mm);
    breakpoints.sort_by(f64::total_cmp);

    let lap = Lap {
        tabs,
        layout,
        base_z_mm: z_mm,
    };
    let mut pieces = Vec::new();
    let mut segment_start_mm = 0.0;
    for segment in &segments {
        let length_mm = segment.length_mm();
        let mut local: Vec<f64> = vec![0.0];
        local.extend(
            breakpoints
                .iter()
                .map(|b| b - segment_start_mm)
                .filter(|d| *d > EPS_MM && *d < length_mm - EPS_MM),
        );
        local.push(length_mm);
        local.dedup_by(|b, a| *b - *a <= EPS_MM);
        for pair in local.windows(2) {
            if let &[a, b] = pair {
                lap.push_piece(&mut pieces, segment, segment_start_mm, a, b);
            }
        }
        segment_start_mm += length_mm;
    }
    pieces
}

/// One tabbed lap being split into pieces.
struct Lap<'a> {
    tabs: &'a Tabs,
    layout: &'a PathTabs,
    base_z_mm: f64,
}

impl Lap<'_> {
    /// Appends the piece of `segment` from local distance `a` to `b`, which
    /// lies in one zone. `segment_start_mm` is the segment's distance along
    /// the path.
    fn push_piece(
        &self,
        pieces: &mut Vec<Piece>,
        segment: &Segment,
        segment_start_mm: f64,
        a: f64,
        b: f64,
    ) {
        let mid = segment_start_mm + (a + b) / 2.0;
        let level_z_mm = match self.layout.zone(mid, self.base_z_mm) {
            Zone::Clear => Some(self.base_z_mm),
            Zone::Hold(top) => Some(top),
            Zone::Ramp(..) => None,
        };
        if let Some(z) = level_z_mm {
            let end = point_on(segment, b);
            pieces.push(Piece {
                from_z_mm: z,
                kind: segment_kind(segment),
                to: Point3::new(end.x, end.y, z),
            });
            return;
        }
        // A ramp: Z is linear in distance along the path, so a straight
        // piece is one move; an arc is split into chords that stay within
        // the tolerance in XY, Z exact at each chord end.
        let chords = match segment.arc() {
            Some(arc) => {
                let angle_rad = (b - a) / arc.radius_mm;
                let tol = self.tabs.chord_tolerance_mm;
                let max_rad = if tol < arc.radius_mm {
                    2.0 * (1.0 - tol / arc.radius_mm).acos()
                } else {
                    std::f64::consts::PI
                };
                (1..MAX_ARC_CHORDS)
                    .find(|&k| angle_rad / f64::from(k) <= max_rad)
                    .unwrap_or(MAX_ARC_CHORDS)
            }
            None => 1,
        };
        let z_at = |d: f64| self.layout.z_at(segment_start_mm + d, self.base_z_mm);
        let at = |k: u32| {
            if k == chords {
                b
            } else {
                a + (b - a) * f64::from(k) / f64::from(chords)
            }
        };
        for k in 0..chords {
            let (d0, d1) = (at(k), at(k + 1));
            let end = point_on(segment, d1);
            pieces.push(Piece {
                from_z_mm: z_at(d0),
                kind: MoveKind::Linear,
                to: Point3::new(end.x, end.y, z_at(d1)),
            });
        }
    }
}

/// The point `distance_mm` along `segment`; exactly its end at its length.
fn point_on(segment: &Segment, distance_mm: f64) -> Point {
    if distance_mm >= segment.length_mm() - EPS_MM {
        return segment.end;
    }
    segment
        .point_at(distance_mm)
        .map_or(segment.end, |(p, _)| p)
}

/// How a whole segment, or a constant-Z piece of it, is cut. A positive
/// bulge, hence a positive sweep, is counter-clockwise in Y-up.
pub(crate) fn segment_kind(segment: &Segment) -> MoveKind {
    match segment.arc() {
        Some(arc) if arc.sweep_rad > 0.0 => MoveKind::ArcCcw { centre: arc.centre },
        Some(arc) => MoveKind::ArcCw { centre: arc.centre },
        None => MoveKind::Linear,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::generate_profile;
    use crate::test_support::assert_safe;
    use crate::toolpath::{MoveKind, Toolpath};
    use crate::warning::WarningCode;
    use dc_geometry::{EPS_MM, Point, Point3, Transform, assert_approx_eq};
    use dc_project::test_support::sample_project;
    use dc_project::{
        CornerRelief, CutDirection, DefinitionId, Entry, FeatureDefinition, FeatureInstance,
        InstanceId, MachiningOrder, Operation, ProfileOp, Project, Side, Stock, ToolId,
    };
    use std::collections::BTreeMap;
    use std::f64::consts::TAU;

    const TOL: f64 = 1e-6;
    /// Smooth ramps are cut as chords of the arc, so lengths summed over
    /// them fall short of the arc length by at most this here (chord
    /// tolerance 0.01 mm on a 48 mm radius).
    const CHORD_TOL: f64 = 1e-3;
    const THICKNESS_MM: f64 = 12.0;
    const DIAMETER_MM: f64 = 6.35;
    const TAB_WIDTH_MM: f64 = 8.0;
    const TAB_Z_MM: f64 = 3.0;
    const FEED_MM_MIN: f64 = 1000.0;
    const PLUNGE_MM_MIN: f64 = 300.0;
    /// Passes of the coaster: 12 mm deep in 4 mm steps.
    const PASS_Z_MM: [f64; 3] = [8.0, 4.0, 0.0];
    const COASTER_RADIUS_MM: f64 = 45.0;

    /// The spec's coaster (`docs/specs/phase-1/functional.md`, criterion
    /// 11): a 90 mm circle centred at (60, 60) on 12 mm stock, cut outside,
    /// climb, 12 mm deep in 4 mm steps with the 6.35 mm tool.
    fn coaster(tabs: Option<TabSpec>) -> (Project, ProfileOp) {
        circle_job(COASTER_RADIUS_MM, Side::Outside, tabs)
    }

    fn circle_job(radius_mm: f64, side: Side, tabs: Option<TabSpec>) -> (Project, ProfileOp) {
        job(
            "Coaster",
            vec![Polyline::circle(Point::new(0.0, 0.0), radius_mm)],
            side,
            tabs,
        )
    }

    /// A 60 mm square part with a centred square hole `hole_mm` wide, cut
    /// outside (so the hole is cut inside), hole first.
    fn square_with_hole(hole_mm: f64, tabs: Option<TabSpec>) -> (Project, ProfileOp) {
        let square = |side_mm: f64| {
            Polyline::rect(Point::new(-side_mm / 2.0, -side_mm / 2.0), side_mm, side_mm)
        };
        job(
            "Square",
            vec![square(60.0), square(hole_mm)],
            Side::Outside,
            tabs,
        )
    }

    /// One definition named `name` holding `profiles`, placed once at
    /// (60, 60) as instance `i1` named "`name` 1", cut by Profile `o1`.
    fn job(
        name: &str,
        profiles: Vec<Polyline>,
        side: Side,
        tabs: Option<TabSpec>,
    ) -> (Project, ProfileOp) {
        let mut p = sample_project();
        p.stock = Stock {
            width_mm: 120.0,
            height_mm: 120.0,
            thickness_mm: Some(THICKNESS_MM),
        };
        let def = FeatureDefinition {
            id: DefinitionId::new("d1"),
            name: name.into(),
            profiles,
        };
        p.definitions = BTreeMap::from([(def.id.clone(), def)]);
        p.instances = vec![FeatureInstance {
            id: InstanceId::new("i1"),
            definition: DefinitionId::new("d1"),
            transform: Transform::translate(60.0, 60.0),
            name: format!("{name} 1"),
            visible: true,
            locked: false,
            z_top_mm: Some(THICKNESS_MM),
            z_bottom_mm: Some(0.0),
        }];
        let op = ProfileOp {
            id: OperationId::new("o1"),
            name: "Cut out".into(),
            enabled: true,
            targets: vec![InstanceId::new("i1")],
            tool: ToolId::new("t1"),
            side,
            depth_mm: Some(THICKNESS_MM),
            step_down_mm: Some(4.0),
            direction: CutDirection::Climb,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
            tabs,
            machining_order: MachiningOrder::FullDepthFirst,
            entry: Entry::Plunge,
            lead: None,
            corner_relief: CornerRelief::None,
        };
        p.operations = vec![Operation::Profile(op.clone())];
        (p, op)
    }

    fn tab_spec(placement: TabPlacement, shape: TabShape) -> TabSpec {
        TabSpec {
            placement,
            width_mm: TAB_WIDTH_MM,
            height_mm: TAB_Z_MM,
            shape,
            on_holes: false,
        }
    }

    fn four_rect_tabs() -> TabSpec {
        tab_spec(TabPlacement::Count { count: 4 }, TabShape::Rect)
    }

    /// Length of the coaster's tool-centre path.
    fn coaster_path_mm() -> f64 {
        TAU * (COASTER_RADIUS_MM + DIAMETER_MM / 2.0)
    }

    fn generate(project: &Project, op: &ProfileOp) -> (Toolpath, Vec<Warning>) {
        let (toolpath, warnings) = generate_profile(project, op).unwrap();
        assert_safe(&toolpath, project, &Operation::Profile(op.clone()));
        (toolpath, warnings)
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(|w| w.code()).collect()
    }

    /// A move that changes XY, with where it started.
    #[derive(Debug, Clone, Copy)]
    struct XyMove {
        from: Point3,
        mv: Move,
        /// Length along the path in XY, arcs measured as arcs.
        length_mm: f64,
    }

    impl XyMove {
        fn z_max(&self) -> f64 {
            self.from.z.max(self.mv.to.z)
        }

        fn at_z(&self, z_mm: f64) -> bool {
            (self.from.z - z_mm).abs() <= TOL && (self.mv.to.z - z_mm).abs() <= TOL
        }
    }

    /// Every non-rapid move that changes XY, in order.
    fn xy_moves(toolpath: &Toolpath) -> Vec<XyMove> {
        let mut out = Vec::new();
        let Some(first) = toolpath.moves.first() else {
            return out;
        };
        let mut pos = first.to;
        for mv in &toolpath.moves {
            let (a, b) = (pos.xy(), mv.to.xy());
            if !mv.is_rapid() && a.distance_to(b) > EPS_MM {
                let length_mm = match mv.kind {
                    MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                        let (u, v) = (a - centre, b - centre);
                        let mut sweep = u.cross(v).atan2(u.dot(v)).abs();
                        let ccw = matches!(mv.kind, MoveKind::ArcCcw { .. });
                        if (u.cross(v) < 0.0) == ccw {
                            sweep = TAU - sweep;
                        }
                        a.distance_to(centre) * sweep
                    }
                    _ => a.distance_to(b),
                };
                out.push(XyMove {
                    from: pos,
                    mv: *mv,
                    length_mm,
                });
            }
            pos = mv.to;
        }
        out
    }

    /// The XY moves of the final pass: every one that stays below the
    /// pass before it.
    fn final_lap(toolpath: &Toolpath) -> Vec<XyMove> {
        xy_moves(toolpath)
            .into_iter()
            .filter(|m| m.z_max() < PASS_Z_MM[1] - TOL)
            .collect()
    }

    /// A run of consecutive XY moves along a lap.
    #[derive(Debug, Clone, Copy)]
    struct Run {
        /// Distance along the lap from its start to the run's start.
        start_mm: f64,
        length_mm: f64,
    }

    /// The runs of `lap` whose moves satisfy `pred`; a run that wraps
    /// through the lap start is joined into one.
    fn runs(lap: &[XyMove], pred: impl Fn(&XyMove) -> bool) -> Vec<Run> {
        let mut runs: Vec<Run> = Vec::new();
        let mut at_mm = 0.0;
        let mut open = false;
        for m in lap {
            if pred(m) {
                if open {
                    runs.last_mut().unwrap().length_mm += m.length_mm;
                } else {
                    runs.push(Run {
                        start_mm: at_mm,
                        length_mm: m.length_mm,
                    });
                    open = true;
                }
            } else {
                open = false;
            }
            at_mm += m.length_mm;
        }
        let wraps = lap.first().is_some_and(&pred) && lap.last().is_some_and(&pred);
        if wraps && runs.len() > 1 {
            let first = runs.remove(0);
            runs.last_mut().unwrap().length_mm += first.length_mm;
        }
        runs
    }

    fn raised_runs(lap: &[XyMove]) -> Vec<Run> {
        runs(lap, |m| m.at_z(TAB_Z_MM))
    }

    /// After the descent into it, the final pass never moves Z alone: a
    /// smooth tab ramps rather than steps.
    fn assert_no_vertical_step_in_final_pass(tp: &Toolpath) {
        let final_start = tp
            .moves
            .iter()
            .position(|m| m.to.z < PASS_Z_MM[1] - TOL)
            .unwrap();
        for pair in tp.moves[final_start..].windows(2) {
            let (prev, mv) = (pair[0], pair[1]);
            let vertical = prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM;
            assert!(
                mv.is_rapid() || !vertical || (mv.to.z - prev.to.z).abs() <= TOL,
                "no vertical step inside the final pass: {prev:?} -> {mv:?}"
            );
        }
    }

    #[test]
    fn count_four_spaces_evenly() {
        let centres = tab_centres_mm(&TabPlacement::Count { count: 4 }, 100.0);
        assert_eq!(centres.len(), 4);
        for (c, e) in centres.iter().zip([12.5, 37.5, 62.5, 87.5]) {
            assert_approx_eq!(*c, e, TOL);
        }
        let at = TabPlacement::AtDistances {
            distances_mm: vec![-10.0, 110.0, 50.0],
        };
        let centres = tab_centres_mm(&at, 100.0);
        for (c, e) in centres.iter().zip([90.0, 10.0, 50.0]) {
            assert_approx_eq!(*c, e, TOL);
        }
        assert!(tab_centres_mm(&TabPlacement::Count { count: 0 }, 100.0).is_empty());

        // On the coaster, the raised runs are centred at L/8, 3L/8, 5L/8,
        // 7L/8 along the final lap.
        let (p, op) = coaster(Some(four_rect_tabs()));
        let (tp, _) = generate(&p, &op);
        let l = coaster_path_mm();
        let runs = raised_runs(&final_lap(&tp));
        assert_eq!(runs.len(), 4, "{runs:?}");
        for (k, run) in runs.iter().enumerate() {
            let centre = run.start_mm + run.length_mm / 2.0;
            assert_approx_eq!(centre, l * (2.0 * k as f64 + 1.0) / 8.0, TOL);
        }
    }

    #[test]
    fn span_is_width_plus_diameter() {
        // The tool-centre path, not the outline, is what tabs interrupt:
        // a 25.4 mm hole cut inside has a path π(25.4 − d) long (port of
        // "a circle's inside path is shorter than its bounding box
        // suggests").
        let width_mm = 7.9375;
        let small = |count| {
            circle_job(
                12.7,
                Side::Inside,
                Some(TabSpec {
                    placement: TabPlacement::Count { count },
                    width_mm,
                    height_mm: TAB_Z_MM,
                    shape: TabShape::Rect,
                    on_holes: false,
                }),
            )
        };
        let path_mm = std::f64::consts::PI * (25.4 - DIAMETER_MM);
        let span_mm = width_mm + DIAMETER_MM;

        // Two tabs leave most of the small circle free.
        let (p, op) = small(2);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let lap = final_lap(&tp);
        let total: f64 = lap.iter().map(|m| m.length_mm).sum();
        assert_approx_eq!(total, path_mm, TOL);
        let raised = raised_runs(&lap);
        assert_eq!(raised.len(), 2, "{raised:?}");
        for run in &raised {
            assert_approx_eq!(run.length_mm, span_mm, TOL);
        }

        // Four tabs would all but cover it (port of "four tabs cover a
        // circle"): they cover most of the path, so only the two that fit
        // in half of it are cut, and that warns (#109).
        let (p, op) = small(4);
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(
            codes(&warnings),
            vec![WarningCode::TabsCoverMostOfContour],
            "{warnings:?}"
        );
        let lap = final_lap(&tp);
        assert_eq!(raised_runs(&lap).len(), 2, "{lap:?}");
        let free: f64 = runs(&lap, |m| m.at_z(0.0))
            .iter()
            .map(|r| r.length_mm)
            .sum();
        assert_approx_eq!(free, path_mm - 2.0 * span_mm, TOL);

        // And on the coaster each tab spans 8 + 6.35 mm.
        let (p, op) = coaster(Some(four_rect_tabs()));
        let (tp, _) = generate(&p, &op);
        for run in raised_runs(&final_lap(&tp)) {
            assert_approx_eq!(run.length_mm, TAB_WIDTH_MM + DIAMETER_MM, TOL);
        }
    }

    #[test]
    fn passes_above_tab_height_untouched() {
        for (shape, top_z_mm) in [
            (TabShape::Rect, TAB_Z_MM),
            (TabShape::Smooth, TAB_Z_MM),
            // A pass exactly at the tab top is not below it.
            (TabShape::Rect, PASS_Z_MM[1]),
        ] {
            let mut spec = tab_spec(TabPlacement::Count { count: 4 }, shape);
            spec.height_mm = top_z_mm;
            let (p, op) = coaster(Some(spec));
            let (with_tabs, _) = generate(&p, &op);
            let (plain, _) = generate(&p, &ProfileOp { tabs: None, ..op });
            // Everything up to the descent into the final pass is identical.
            let final_descent = plain
                .moves
                .iter()
                .position(|m| m.to.z < PASS_Z_MM[1] - TOL)
                .unwrap();
            assert_eq!(
                with_tabs.moves[..final_descent],
                plain.moves[..final_descent],
                "{shape:?} at {top_z_mm}"
            );
            assert_ne!(with_tabs.moves, plain.moves, "the final pass is tabbed");
        }
    }

    /// O9: tabs survive reordering, so the check runs under both machining
    /// orders.
    #[test]
    fn final_pass_has_exactly_four_raised_segments_at_tab_z() {
        for order in [MachiningOrder::FullDepthFirst, MachiningOrder::PassFirst] {
            assert_four_raised_segments_at_tab_z(order);
        }
    }

    fn assert_four_raised_segments_at_tab_z(order: MachiningOrder) {
        let (p, mut op) = coaster(Some(four_rect_tabs()));
        op.machining_order = order;
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let lap = final_lap(&tp);
        let raised = raised_runs(&lap);
        assert_eq!(raised.len(), 4, "{raised:?}");
        for run in &raised {
            assert_approx_eq!(run.length_mm, TAB_WIDTH_MM + DIAMETER_MM, TOL);
        }
        // Evenly spaced: a quarter of the path between run starts.
        let quarter = coaster_path_mm() / 4.0;
        for pair in raised.windows(2) {
            assert_approx_eq!(pair[1].start_mm - pair[0].start_mm, quarter, TOL);
        }
        // Every other XY move of the final pass is at the pass Z, and the
        // whole lap is accounted for.
        assert!(lap.iter().all(|m| m.at_z(0.0) || m.at_z(TAB_Z_MM)));
        let total: f64 = lap.iter().map(|m| m.length_mm).sum();
        assert_approx_eq!(total, coaster_path_mm(), TOL);

        // Each tab is a vertical lift at plunge feed, a traverse, and a
        // vertical descent at plunge feed.
        let final_start = tp
            .moves
            .iter()
            .position(|m| m.to.z < PASS_Z_MM[1] - TOL)
            .unwrap();
        let mut lifts = 0;
        let mut drops = 0;
        for pair in tp.moves[final_start..].windows(2) {
            let (prev, mv) = (pair[0], pair[1]);
            if mv.is_rapid() || (mv.to.z - prev.to.z).abs() <= TOL {
                continue;
            }
            assert!(
                prev.to.xy().distance_to(mv.to.xy()) <= EPS_MM,
                "vertical: {mv:?}"
            );
            assert_eq!(mv.kind, MoveKind::Linear);
            assert_approx_eq!(mv.feed_mm_min.unwrap(), PLUNGE_MM_MIN, EPS_MM);
            if mv.to.z > prev.to.z {
                assert_approx_eq!(mv.to.z, TAB_Z_MM, TOL);
                lifts += 1;
            } else {
                drops += 1;
            }
        }
        assert_eq!(lifts, 4);
        assert_eq!(drops, 4, "one per tab");
        // Never below the stock bottom.
        assert!(tp.moves.iter().all(|m| m.to.z >= -EPS_MM));
    }

    #[test]
    fn tab_across_the_lap_start_never_plunges_into_it() {
        let spec = tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![0.0],
            },
            TabShape::Rect,
        );
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let raised = raised_runs(&final_lap(&tp));
        assert_eq!(raised.len(), 1, "{raised:?}");
        assert_approx_eq!(raised[0].length_mm, TAB_WIDTH_MM + DIAMETER_MM, TOL);
        // The descent into the final pass stops at the tab top: the lap
        // starts inside the tab.
        let descent = tp
            .moves
            .iter()
            .find(|m| m.to.z < PASS_Z_MM[1] - TOL)
            .unwrap();
        assert_approx_eq!(descent.to.z, TAB_Z_MM, TOL);
    }

    #[test]
    fn overlapping_tabs_merge_and_warn() {
        let spec = tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![100.0, 108.0],
            },
            TabShape::Rect,
        );
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOverlap]);
        assert_eq!(warnings[0].op().map(OperationId::as_str), Some("o1"));
        let raised = raised_runs(&final_lap(&tp));
        assert_eq!(raised.len(), 1, "merged: {raised:?}");
        assert_approx_eq!(raised[0].length_mm, 8.0 + TAB_WIDTH_MM + DIAMETER_MM, TOL);
        assert_approx_eq!(
            raised[0].start_mm,
            100.0 - (TAB_WIDTH_MM + DIAMETER_MM) / 2.0,
            TOL
        );

        // Two identical tabs overlap entirely: one span, still a warning.
        let spec = tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![100.0, 100.0],
            },
            TabShape::Rect,
        );
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOverlap]);
        let raised = raised_runs(&final_lap(&tp));
        assert_eq!(raised.len(), 1);
        assert_approx_eq!(raised[0].length_mm, TAB_WIDTH_MM + DIAMETER_MM, TOL);

        // Overlapping smooth tabs merge into one ramp that holds the top
        // between them.
        let spec = tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![100.0, 108.0],
            },
            TabShape::Smooth,
        );
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOverlap]);
        let lifted = runs(&final_lap(&tp), |m| m.z_max() > TOL);
        assert_eq!(lifted.len(), 1, "{lifted:?}");
        assert_approx_eq!(
            lifted[0].length_mm,
            8.0 + TAB_WIDTH_MM + DIAMETER_MM,
            CHORD_TOL
        );
        let held: f64 = raised_runs(&final_lap(&tp))
            .iter()
            .map(|r| r.length_mm)
            .sum();
        // From the first tab's one-diameter hold to the last's: the ramps
        // are half a tab width each.
        assert_approx_eq!(held, 8.0 + DIAMETER_MM, TOL);
        assert_no_vertical_step_in_final_pass(&tp);

        // Four tabs wide enough to cover the whole path would merge into
        // one; only the one that fits in half of it is cut (#109), so
        // nothing overlaps.
        let mut spec = four_rect_tabs();
        spec.width_mm = 80.0;
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsCoverMostOfContour]);
        let raised = raised_runs(&final_lap(&tp));
        assert_eq!(raised.len(), 1, "{raised:?}");
        assert_approx_eq!(raised[0].length_mm, 80.0 + DIAMETER_MM, TOL);

        // Distances so close the tabs overlap, but in less than half the
        // path, are cut as set: merged, with a warning.
        let spec = tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![100.0, 104.0, 108.0],
            },
            TabShape::Rect,
        );
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOverlap]);
        assert_eq!(raised_runs(&final_lap(&tp)).len(), 1);

        // Spaced-out tabs do not warn.
        let (p, op) = coaster(Some(four_rect_tabs()));
        let (_, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    /// The XY moves of each contour's final pass, in cut order: a contour
    /// runs from one rapid to the next.
    fn final_laps_by_contour(toolpath: &Toolpath) -> Vec<Vec<XyMove>> {
        let mut out: Vec<Vec<XyMove>> = Vec::new();
        let mut current: Vec<XyMove> = Vec::new();
        let Some(first) = toolpath.moves.first() else {
            return out;
        };
        let mut pos = first.to;
        for mv in &toolpath.moves {
            if mv.is_rapid() {
                if !current.is_empty() {
                    out.push(std::mem::take(&mut current));
                }
            } else if pos.xy().distance_to(mv.to.xy()) > EPS_MM {
                let m = XyMove {
                    from: pos,
                    mv: *mv,
                    length_mm: 0.0,
                };
                if m.z_max() < PASS_Z_MM[1] - TOL {
                    current.push(m);
                }
            }
            pos = mv.to;
        }
        if !current.is_empty() {
            out.push(current);
        }
        out
    }

    fn coverage_warnings(warnings: &[Warning]) -> Vec<&Warning> {
        warnings
            .iter()
            .filter(|w| w.code() == WarningCode::TabsCoverMostOfContour)
            .collect()
    }

    /// [`four_rect_tabs`] on the holes too.
    fn four_rect_tabs_on_holes() -> TabSpec {
        TabSpec {
            on_holes: true,
            ..four_rect_tabs()
        }
    }

    #[test]
    fn tabs_covering_most_of_contour_warn() {
        // Default tabs (4 × 8 mm, 6.35 mm bit) on a 20 mm square hole:
        // 57.4 mm of tab on a 54.6 mm tool path.
        let (p, op) = square_with_hole(20.0, Some(four_rect_tabs_on_holes()));
        let (_, warnings) = generate(&p, &op);
        let cover = coverage_warnings(&warnings);
        assert_eq!(cover.len(), 1, "only the hole: {warnings:?}");
        let w = cover[0];
        let Warning::TabsCoverMostOfContour {
            op,
            contour,
            shape: Some(shape),
            path_length_mm,
            tab_count,
            tab_span_mm,
            tab_total_mm,
            cut_count,
            max_width_mm: Some(max_width_mm),
            ..
        } = w
        else {
            panic!("{w:?}");
        };
        assert_eq!(op.as_str(), "o1");
        assert_eq!(*contour, 0, "the hole is cut first");
        assert_eq!(shape.instance.as_str(), "i1");
        assert_eq!(shape.instance_name, "Square 1");
        assert_eq!(shape.profile, 1);
        assert_eq!(*tab_count, 4);
        let path_mm = 4.0 * (20.0 - DIAMETER_MM);
        let span_mm = TAB_WIDTH_MM + DIAMETER_MM;
        assert_approx_eq!(*path_length_mm, path_mm, TOL);
        assert_approx_eq!(*tab_span_mm, span_mm, TOL);
        assert_approx_eq!(*tab_total_mm, 4.0 * span_mm, TOL);
        // One 14.35 mm tab covers at most half of 54.6 mm, so one is cut
        // (#109); four tabs fit in half only if each is at most
        // 27.3 / 4 − 6.35 = 0.475 mm wide.
        assert_eq!(*cut_count, 1);
        assert!(*max_width_mm <= path_mm / 2.0 / 4.0 - DIAMETER_MM);

        // The text names the shape, the path, the tab total and the share,
        // says what would happen and what is cut instead, and suggests a
        // width, in rounded values.
        let text = w.to_string();
        for needle in [
            "`Square 1`",
            "54.6 mm",
            "57.4 mm",
            "105.1%",
            "would not cut free",
            "Only 1 of them is cut",
            "0.47 mm",
        ] {
            assert!(text.contains(needle), "{needle:?} in {text:?}");
        }
        assert!(!text.contains('='), "{text}");

        // The outer contour's tabs cover a fifth of it: no warning. A
        // larger hole is fine too.
        let (p, op) = square_with_hole(40.0, Some(four_rect_tabs()));
        let (_, warnings) = generate(&p, &op);
        assert!(coverage_warnings(&warnings).is_empty(), "{warnings:?}");
    }

    /// The coverage warning for four 3 mm tabs cut with a 1/8" (3.175 mm)
    /// bit on a 13.175 mm square hole: a 40 mm tool path, 24.7 mm of it
    /// raised.
    fn small_bit_coverage_warning() -> Warning {
        let spec = TabSpec {
            width_mm: 3.0,
            ..four_rect_tabs()
        };
        let (mut p, op) = square_with_hole(
            13.175,
            Some(TabSpec {
                on_holes: true,
                ..spec
            }),
        );
        p.tools[0].diameter_mm = 3.175;
        let (_, warnings) = generate(&p, &op);
        let cover = coverage_warnings(&warnings);
        assert_eq!(cover.len(), 1, "only the hole: {warnings:?}");
        cover[0].clone()
    }

    #[test]
    fn coverage_warning_carries_tab_width_and_diameter() {
        let w = small_bit_coverage_warning();
        let Warning::TabsCoverMostOfContour {
            tab_width_mm,
            diameter_mm,
            tab_span_mm,
            tab_total_mm,
            path_length_mm,
            ..
        } = w
        else {
            panic!("{w:?}");
        };
        assert_approx_eq!(tab_width_mm, 3.0, TOL);
        assert_approx_eq!(diameter_mm, 3.175, TOL);
        assert_approx_eq!(tab_span_mm, 6.175, TOL);
        assert_approx_eq!(tab_total_mm, 4.0 * 6.175, TOL);
        assert_approx_eq!(path_length_mm, 40.0, TOL);
    }

    #[test]
    fn coverage_warning_text_names_the_tab_width() {
        let text = small_bit_coverage_warning().to_string();
        for needle in ["3 mm wide", "3.175 mm bit", "6.17 mm", "24.7 mm", "40 mm"] {
            assert!(text.contains(needle), "{needle:?} in {text:?}");
        }
        assert!(!text.contains("tab(s) of 6.17 mm each"), "{text}");
    }

    /// #109: a hole's slug is waste, so by default only the part's
    /// outline gets tabs: the 40 mm hole of the 60 mm square is cut free
    /// all the way round on its last pass, while the square rises over
    /// its four tabs. Nothing warns.
    #[test]
    fn holes_get_no_tabs_by_default() {
        let (p, op) = square_with_hole(40.0, Some(four_rect_tabs()));
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let laps = final_laps_by_contour(&tp);
        assert_eq!(laps.len(), 2, "the hole, then the square");
        assert!(
            laps[0].iter().all(|m| m.at_z(0.0)),
            "the hole has no tabs: {:?}",
            laps[0]
        );
        let square = runs(&laps[1], |m| m.at_z(TAB_Z_MM));
        assert_eq!(square.len(), 4, "the square keeps its tabs: {square:?}");
        assert!(laps[1].iter().all(|m| m.at_z(0.0) || m.at_z(TAB_Z_MM)));
    }

    /// #109: with `on_holes` the hole is held too, by the same four tabs
    /// as its part, as every outline was before version 9 (a 134.6 mm
    /// hole path; four 14.35 mm spans cover 43% of it, so all are cut).
    #[test]
    fn on_holes_opt_in_keeps_hole_tabs() {
        let (p, op) = square_with_hole(40.0, Some(four_rect_tabs_on_holes()));
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let laps = final_laps_by_contour(&tp);
        assert_eq!(laps.len(), 2, "the hole, then the square");
        for (name, lap) in ["hole", "square"].iter().zip(&laps) {
            let raised = runs(lap, |m| m.at_z(TAB_Z_MM));
            assert_eq!(raised.len(), 4, "{name}: {raised:?}");
        }
    }

    /// The tabs asked and cut, and the text, of the one coverage warning
    /// in `warnings`.
    fn cover_of(warnings: &[Warning]) -> (usize, usize, String) {
        let cover = coverage_warnings(warnings);
        assert_eq!(cover.len(), 1, "{warnings:?}");
        let Warning::TabsCoverMostOfContour {
            tab_count,
            cut_count,
            ..
        } = cover[0]
        else {
            panic!("{:?}", cover[0]);
        };
        (*tab_count, *cut_count, cover[0].to_string())
    }

    /// #109: an outline too short for its tabs gets the most that keep
    /// them to half of its tool path (the coverage threshold), and the
    /// coverage warning names the shape and says how many are cut.
    /// `count` tabs are re-spaced evenly; of `at_distances`, the first in
    /// the list are kept. A hole with `on_holes` may get none; a part
    /// keeps at least one ([`short_part_outline_keeps_at_least_one_tab`]).
    #[test]
    fn too_short_outline_gets_fewer_tabs_and_warns() {
        let span_mm = TAB_WIDTH_MM + DIAMETER_MM;

        // Eleven 14.35 mm spans would cover 52% of the coaster's 302.7 mm
        // path; ten (47%) are cut, evenly spaced.
        let spec = tab_spec(TabPlacement::Count { count: 11 }, TabShape::Rect);
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsCoverMostOfContour]);
        let (asked, cut, text) = cover_of(&warnings);
        assert_eq!((asked, cut), (11, 10));
        assert!(text.contains("`Coaster 1`"), "names the shape: {text}");
        assert!(text.contains("Only 10 of them are cut"), "{text}");
        let raised = raised_runs(&final_lap(&tp));
        assert_eq!(raised.len(), 10, "{raised:?}");
        let tenth = coaster_path_mm() / 10.0;
        for pair in raised.windows(2) {
            assert_approx_eq!(pair[1].start_mm - pair[0].start_mm, tenth, TOL);
        }
        for run in &raised {
            assert_approx_eq!(run.length_mm, span_mm, TOL);
        }

        // Twelve distances 25 mm apart would cover 57%; the first ten are
        // cut, the last two (250 and 275 mm) are not.
        let distances_mm: Vec<f64> = (0..12).map(|k| 25.0 * f64::from(k)).collect();
        let spec = tab_spec(TabPlacement::AtDistances { distances_mm }, TabShape::Rect);
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!((cover_of(&warnings).0, cover_of(&warnings).1), (12, 10));
        let l = coaster_path_mm();
        let mut centres: Vec<f64> = raised_runs(&final_lap(&tp))
            .iter()
            .map(|r| (r.start_mm + r.length_mm / 2.0).rem_euclid(l))
            .map(|c| if c > l - 1.0 { c - l } else { c })
            .collect();
        centres.sort_by(f64::total_cmp);
        assert_eq!(centres.len(), 10, "{centres:?}");
        for (k, c) in centres.iter().enumerate() {
            assert_approx_eq!(*c, 25.0 * k as f64, TOL);
        }

        // A 20 mm square hole with hole tabs on: one of the four fits
        // its 54.6 mm path. The square keeps all four.
        let (p, op) = square_with_hole(20.0, Some(four_rect_tabs_on_holes()));
        let (tp, warnings) = generate(&p, &op);
        let (asked, cut, text) = cover_of(&warnings);
        assert_eq!((asked, cut), (4, 1));
        assert!(text.contains("`Square 1`"), "{text}");
        let laps = final_laps_by_contour(&tp);
        assert_eq!(runs(&laps[0], |m| m.at_z(TAB_Z_MM)).len(), 1);
        assert_eq!(runs(&laps[1], |m| m.at_z(TAB_Z_MM)).len(), 4);

        // A 13 mm square hole with hole tabs on has a 26.6 mm path: even
        // one 14.35 mm span covers more than half, so the hole gets none
        // and its slug cuts free. The square keeps its four.
        let (p, op) = square_with_hole(13.0, Some(four_rect_tabs_on_holes()));
        let (tp, warnings) = generate(&p, &op);
        let (asked, cut, text) = cover_of(&warnings);
        assert_eq!((asked, cut), (4, 0));
        assert!(text.contains("No tab is cut"), "{text}");
        let laps = final_laps_by_contour(&tp);
        assert_eq!(laps.len(), 2);
        assert!(laps[0].iter().all(|m| m.at_z(0.0)), "{:?}", laps[0]);
        assert_eq!(runs(&laps[1], |m| m.at_z(TAB_Z_MM)).len(), 4);
    }

    /// #109: a part that asked for tabs keeps at least one, however short
    /// its outline: cut free, a small part can be thrown by the bit,
    /// which is worse than one held by a tab that covers most of it. A
    /// 12 mm circle cut inside has a 17.7 mm path, and one 14.35 mm span
    /// covers 81% of it: the one tab is cut, and the warning says the
    /// part may need cutting free by hand.
    #[test]
    fn short_part_outline_keeps_at_least_one_tab() {
        for (count, asked) in [(4, 4), (1, 1)] {
            let spec = tab_spec(TabPlacement::Count { count }, TabShape::Rect);
            let (p, op) = circle_job(6.0, Side::Inside, Some(spec));
            let (tp, warnings) = generate(&p, &op);
            let (tab_count, cut, text) = cover_of(&warnings);
            assert_eq!((tab_count, cut), (asked, 1), "count {count}");
            assert!(text.contains("by hand"), "{text}");
            let laps = final_laps_by_contour(&tp);
            assert_eq!(laps.len(), 1);
            let raised = runs(&laps[0], |m| m.at_z(TAB_Z_MM));
            assert_eq!(raised.len(), 1, "count {count}: {raised:?}");
        }

        // Of `at_distances`, the first is the one kept.
        let spec = tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![9.0, 0.0],
            },
            TabShape::Rect,
        );
        let (p, op) = circle_job(6.0, Side::Inside, Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!((cover_of(&warnings).0, cover_of(&warnings).1), (2, 1));
        let lap = final_lap(&tp);
        let raised = raised_runs(&lap);
        assert_eq!(raised.len(), 1, "{raised:?}");
        assert_approx_eq!(raised[0].start_mm + raised[0].length_mm / 2.0, 9.0, TOL);
    }

    #[test]
    fn tabs_covering_more_than_half_warn() {
        // The boundary is more than half: exactly half does not warn.
        let l = coaster_path_mm();
        let one_tab = |span_mm: f64| {
            let mut spec = tab_spec(
                TabPlacement::AtDistances {
                    distances_mm: vec![100.0],
                },
                TabShape::Rect,
            );
            spec.width_mm = span_mm - DIAMETER_MM;
            coaster(Some(spec))
        };
        let (p, op) = one_tab(l / 2.0);
        let (_, warnings) = generate(&p, &op);
        assert!(coverage_warnings(&warnings).is_empty(), "{warnings:?}");
        let (p, op) = one_tab(l / 2.0 + 0.01);
        let (_, warnings) = generate(&p, &op);
        assert_eq!(coverage_warnings(&warnings).len(), 1, "{warnings:?}");

        // By count: 10 spans (47%) do not warn, 11 (52%) do.
        for (count, warns) in [(10, false), (11, true)] {
            let spec = tab_spec(TabPlacement::Count { count }, TabShape::Rect);
            let (p, op) = coaster(Some(spec));
            let (_, warnings) = generate(&p, &op);
            assert_eq!(!coverage_warnings(&warnings).is_empty(), warns, "{count}");
        }
    }

    #[test]
    fn final_pass_reaches_full_depth_between_tabs() {
        // Every contour's last lap reaches the cut depth somewhere, unless
        // its tabs cover most of it and say so.
        let mut wide = four_rect_tabs();
        wide.width_mm = 80.0;
        let jobs = [
            coaster(Some(four_rect_tabs())),
            coaster(Some(tab_spec(
                TabPlacement::Count { count: 4 },
                TabShape::Smooth,
            ))),
            coaster(Some(tab_spec(
                TabPlacement::Count { count: 11 },
                TabShape::Rect,
            ))),
            coaster(Some(tab_spec(
                TabPlacement::AtDistances {
                    distances_mm: vec![0.0],
                },
                TabShape::Rect,
            ))),
            coaster(Some(wide)),
            circle_job(12.7, Side::Inside, Some(four_rect_tabs())),
            square_with_hole(20.0, Some(four_rect_tabs())),
            square_with_hole(40.0, Some(four_rect_tabs())),
        ];
        for (p, op) in &jobs {
            let (tp, warnings) = generate(p, op);
            let laps = final_laps_by_contour(&tp);
            assert!(!laps.is_empty());
            for (i, lap) in laps.iter().enumerate() {
                if lap.iter().any(|m| m.at_z(0.0)) {
                    continue;
                }
                assert!(
                    coverage_warnings(&warnings).iter().any(|w| matches!(
                        w,
                        Warning::TabsCoverMostOfContour { contour, .. } if *contour == i
                    )),
                    "contour {i} never reaches depth and nothing says so: {warnings:?}"
                );
            }
        }
    }

    /// A 100 × 40 mm bar cut outside, with one tab per `tabs`.
    fn bar(tabs: Option<TabSpec>) -> (Project, ProfileOp) {
        job(
            "Bar",
            vec![Polyline::rect(Point::new(-50.0, -20.0), 100.0, 40.0)],
            Side::Outside,
            tabs,
        )
    }

    /// One smooth tab centred `BAR_TAB_MM` along the bar's tool-centre
    /// path: the middle of a long straight edge.
    const BAR_TAB_MM: f64 = 60.0;

    fn bar_smooth_tab() -> (Project, ProfileOp) {
        bar(Some(tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![BAR_TAB_MM],
            },
            TabShape::Smooth,
        )))
    }

    /// The final lap as a Z profile along the path: (distance, Z) at each
    /// vertex, Z linear in distance between them.
    fn lap_profile(lap: &[XyMove]) -> Vec<(f64, f64)> {
        let mut out = Vec::new();
        let mut at_mm = 0.0;
        for m in lap {
            out.push((at_mm, m.from.z));
            at_mm += m.length_mm;
            out.push((at_mm, m.mv.to.z));
        }
        out
    }

    /// The material a flat bit of radius `DIAMETER_MM / 2` leaves above the
    /// stock bottom at `x_mm` along the path: the lowest the tool's
    /// bottom reaches over every centre position within one radius.
    fn material_mm(profile: &[(f64, f64)], x_mm: f64) -> f64 {
        let r = DIAMETER_MM / 2.0;
        let (lo, hi) = (x_mm - r, x_mm + r);
        let z_at = |s: f64| {
            profile
                .windows(2)
                .find(|w| w[0].0 <= s && s <= w[1].0 && w[1].0 > w[0].0)
                .map(|w| w[0].1 + (w[1].1 - w[0].1) * (s - w[0].0) / (w[1].0 - w[0].0))
                .unwrap()
        };
        profile
            .iter()
            .filter(|(s, _)| (lo..=hi).contains(s))
            .map(|(_, z)| *z)
            .chain([z_at(lo), z_at(hi)])
            .fold(f64::INFINITY, f64::min)
    }

    #[test]
    fn smooth_tab_plateau_is_bit_diameter() {
        let (p, op) = bar_smooth_tab();
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let lap = final_lap(&tp);
        // The tab sits on a straight edge.
        let lifted = runs(&lap, |m| m.z_max() > TOL);
        assert_eq!(lifted.len(), 1, "{lifted:?}");
        let mut at_mm = 0.0;
        for m in &lap {
            let inside = at_mm >= lifted[0].start_mm - TOL
                && at_mm + m.length_mm <= lifted[0].start_mm + lifted[0].length_mm + TOL;
            if inside {
                assert_eq!(m.mv.kind, MoveKind::Linear, "{m:?}");
            }
            at_mm += m.length_mm;
        }
        // The final pass holds the tab top for at least one bit diameter,
        // centred on the tab.
        let raised = raised_runs(&lap);
        assert_eq!(raised.len(), 1, "{raised:?}");
        assert!(
            raised[0].length_mm >= DIAMETER_MM - TOL,
            "plateau {} < {DIAMETER_MM}",
            raised[0].length_mm
        );
        assert_approx_eq!(
            raised[0].start_mm + raised[0].length_mm / 2.0,
            BAR_TAB_MM,
            TOL
        );
    }

    #[test]
    fn smooth_tab_bridge_is_full_height() {
        let (p, op) = bar_smooth_tab();
        let (tp, _) = generate(&p, &op);
        let profile = lap_profile(&final_lap(&tp));
        let peak = (-400..=400)
            .map(|i| material_mm(&profile, BAR_TAB_MM + f64::from(i) * 0.01))
            .fold(f64::NEG_INFINITY, f64::max);
        assert_approx_eq!(peak, TAB_Z_MM, 0.01);
    }

    #[test]
    fn smooth_tab_bridge_is_triangle() {
        let half_mm = TAB_WIDTH_MM / 2.0;
        let triangle = |x: f64| TAB_Z_MM * (1.0 - x.abs() / half_mm).max(0.0);
        let samples = [0.0, 1.0, -1.0, 2.0, -2.5, 3.0, -3.5, 4.0, -4.0, 5.0, -6.0];

        // On a straight edge.
        let (p, op) = bar_smooth_tab();
        let (tp, _) = generate(&p, &op);
        let profile = lap_profile(&final_lap(&tp));
        for x in samples {
            assert_approx_eq!(material_mm(&profile, BAR_TAB_MM + x), triangle(x), 0.01);
        }

        // On the coaster's arcs: four tabs, each the same triangle centred
        // on its distance, each lifted over width + diameter, every move
        // along the path at the feed rate or its plunge-limited scale
        // (S10, #157), with no vertical step.
        let (p, op) = coaster(Some(tab_spec(
            TabPlacement::Count { count: 4 },
            TabShape::Smooth,
        )));
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let lap = final_lap(&tp);
        let lifted = runs(&lap, |m| m.z_max() > TOL);
        assert_eq!(lifted.len(), 4, "{lifted:?}");
        for run in &lifted {
            assert_approx_eq!(run.length_mm, TAB_WIDTH_MM + DIAMETER_MM, CHORD_TOL);
        }
        let profile = lap_profile(&lap);
        let l = coaster_path_mm();
        for k in 0..4 {
            let centre = l * (2.0 * f64::from(k) + 1.0) / 8.0;
            for x in samples {
                assert_approx_eq!(material_mm(&profile, centre + x), triangle(x), 0.01);
            }
        }
        assert!(
            lap.iter()
                .all(|m| m.from.z >= -1e-12 && m.mv.to.z <= TAB_Z_MM + 1e-12)
        );
        for m in &lap {
            assert_approx_eq!(m.mv.feed_mm_min.unwrap(), plunge_limited(m), EPS_MM);
        }
        assert_no_vertical_step_in_final_pass(&tp);
    }

    /// The feed S10 asks of `m`: the cutting feed, scaled on a descending
    /// move so its vertical rate is at most the plunge rate, `min(feed,
    /// plunge × length / drop)`.
    fn plunge_limited(m: &XyMove) -> f64 {
        let drop_mm = m.from.z - m.mv.to.z;
        if drop_mm <= EPS_MM {
            return FEED_MM_MIN;
        }
        let length_mm = m.length_mm.hypot(drop_mm);
        FEED_MM_MIN.min(PLUNGE_MM_MIN * length_mm / drop_mm)
    }

    /// S10 (#157): a smooth tab's down slope descends no faster than the
    /// plunge rate. The bar's 3 mm tab over a 4 mm ramp (half the 8 mm
    /// width) would fall at 1000 × 3/5 = 600 mm/min at the cutting feed,
    /// twice the 300 plunge, so its feed is scaled to 300 × 5/3 = 500; the
    /// up slope and the hold stay at the cutting feed.
    #[test]
    fn smooth_tab_down_slope_is_plunge_limited() {
        let (p, op) = bar_smooth_tab();
        let (tp, _) = generate(&p, &op);
        let lap = final_lap(&tp);
        let mut falling = 0;
        for m in &lap {
            let f = m.mv.feed_mm_min.unwrap();
            let drop_mm = m.from.z - m.mv.to.z;
            if drop_mm > EPS_MM {
                falling += 1;
                let vertical = f * drop_mm / m.length_mm.hypot(drop_mm);
                assert!(
                    vertical <= PLUNGE_MM_MIN + 1e-9,
                    "down slope falls at {vertical} mm/min: {m:?}"
                );
                assert!(f < FEED_MM_MIN, "the scale applies: {m:?}");
            } else {
                assert_approx_eq!(f, FEED_MM_MIN, EPS_MM);
            }
            assert_approx_eq!(f, plunge_limited(m), EPS_MM);
        }
        assert!(falling > 0, "the final lap comes down off the tab");
    }

    #[test]
    fn tabs_above_cut_depth_warn() {
        // The cut stops at Z 6, above a 3 mm tab: the tabs never apply.
        let (p, mut op) = coaster(Some(four_rect_tabs()));
        op.depth_mm = Some(6.0);
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOutsideCutRange]);
        assert_eq!(warnings[0].op().map(OperationId::as_str), Some("o1"));
        assert!(
            matches!(
                warnings[0],
                Warning::TabsOutsideCutRange {
                    reason: TabsRangeReason::CutStopsAboveTab,
                    ..
                }
            ),
            "{warnings:?}"
        );
        let (plain, _) = generate(&p, &ProfileOp { tabs: None, ..op });
        assert_eq!(tp, plain, "tabs out of reach change nothing");

        // A tab top above the stock top is reported, and still applied to
        // every pass, in the air above the stock.
        let mut spec = four_rect_tabs();
        spec.height_mm = THICKNESS_MM + 2.0;
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOutsideCutRange]);
        assert!(
            matches!(
                warnings[0],
                Warning::TabsOutsideCutRange {
                    reason: TabsRangeReason::AboveStockTop,
                    ..
                }
            ),
            "{warnings:?}"
        );
        let raised = runs(&xy_moves(&tp), |m| m.at_z(THICKNESS_MM + 2.0));
        assert_eq!(raised.len(), 4 * PASS_Z_MM.len());

        // Tabs within the cut do not warn.
        let (p, op) = coaster(Some(four_rect_tabs()));
        let (_, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn invalid_tab_spec_is_error() {
        for (width_mm, height_mm) in [(0.0, 3.0), (-1.0, 3.0), (8.0, 0.0), (f64::NAN, 3.0)] {
            let mut spec = four_rect_tabs();
            spec.width_mm = width_mm;
            spec.height_mm = height_mm;
            let (p, op) = coaster(Some(spec));
            let err = generate_profile(&p, &op).unwrap_err();
            assert!(matches!(err, CamError::InvalidTab { .. }), "{err:?}");
        }
        let (p, op) = coaster(Some(tab_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![f64::INFINITY],
            },
            TabShape::Rect,
        )));
        let err = generate_profile(&p, &op).unwrap_err();
        assert!(matches!(err, CamError::InvalidTab { .. }), "{err:?}");
    }
}
