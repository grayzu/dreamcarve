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
//! one instance (ADR-0005, [`crate::placed_tabs`]), and centres it
//! on the tool path at the path's point nearest the tab, so the growth
//! becomes a span of `width + diameter` along the path: the cutter's full
//! width must clear the tab.
//!
//! A smooth tab leaves a triangular bridge: base `width`, peak `height`
//! above the cut floor. For a flat bit of radius r the tool centre
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
//! path ([`Request::fit`]): a hole gets none unless
//! [`TabSpec::on_holes`], and a path whose spans would cover more than
//! half of it gets the most that cover at most half, so it cuts free
//! between its tabs. A part's outline keeps at least one tab even then
//! ([`Least::One`]): a loose part can be thrown by the bit. PureCutCNC
//! sizes tabs per feature in its auto-tab command (`autoTabs.ts`, a 15%
//! free-path rule); that command writes placed tabs and waits for the
//! GUI. The half-path threshold here is the coverage warning's.
//!
//! A request with [`TabSpec::avoid_corners`] moves each tab it lays on a
//! corner off it before the half-path rule is applied ([`corners`]).

mod corners;

pub use corners::TAB_CORNER_TURN_DEG;

use crate::entry::plunge_limited_feed_mm_min;
use crate::error::{CamError, ShapeRef};
use crate::toolpath::{Move, MoveKind};
use crate::warning::{TabOnCornerReason, TabsRangeReason, Warning};
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

/// One operation's tab request, checked (a [`TabSpec`] places
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
    /// Holes get tabs too.
    on_holes: bool,
    /// Tabs move off corners.
    avoid_corners: bool,
    /// Diameter of the tool that cuts the tabs.
    diameter_mm: f64,
}

/// The tabs a request lays on one path ([`Request::fit`]).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Fit {
    /// The tabs cut on it.
    pub cuts: Vec<TabCut>,
    /// What was asked and what is cut, when they would cover more than
    /// half of it.
    pub reduced: Option<Reduced>,
    /// Where each tab left on a corner is centred, mm along the
    /// path as tabs are measured on it, and why it stays.
    pub on_corners_mm: Vec<(f64, TabOnCornerReason)>,
}

/// Which way a path runs, for laying tabs on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathEnds {
    /// A closed path: tabs are measured from its start, and a span may
    /// run through it.
    Closed,
    /// An open path, tabs measured from its first vertex as drawn and
    /// each span cut short at its ends, where it holds its top
    /// right to the end. `reversed`: it is cut from its last vertex.
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
/// more than half of it ([`Request::fit`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Reduced {
    /// How many tabs the placement puts on the path.
    pub asked: usize,
    /// How many are cut.
    pub cut: usize,
    /// They are placed tabs, cut as placed, not a request cut down.
    pub placed: bool,
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
    /// open path ([`PathEnds::Open`]) they are measured from its
    /// first vertex as drawn and cut short at its ends; `reversed`, the
    /// path is cut from its last vertex, and the spans are measured from
    /// there, so the tabs stay where they were drawn. A smooth span an
    /// end cuts short has no slope at that end: it holds the tab top
    /// right to it, as a rect span does.
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
        // One span alone over the whole path overlaps another only when
        // there is another; a single tab covering it warns its coverage.
        let mut overlap = cuts.len() > 1
            && cuts
                .iter()
                .any(|cut| cut.width_mm + self.diameter_mm >= l - EPS_MM);
        let spans: Vec<Raw> = cuts.iter().map(|cut| self.closed_span(cut, l)).collect();
        let (merged, merges) = merge_closed(spans, l);
        overlap |= merges;
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
                let (from_mm, to_mm) = (cut.centre_mm - half, cut.centre_mm + half);
                let mut span = Raw::of(cut, from_mm.max(0.0), to_mm.min(l));
                // An end of the path cuts the span short: no room is left
                // for a smooth tab's slope there, so it holds its top
                // right to that end, as a stock path's stretch does
                // (`last_pass::raised_span`). A slope laid inside
                // what is left would start at the pass depth at the
                // path's end, within the tool radius of the tab's centre
                // or on its slope, and cut the tab through.
                if from_mm < -EPS_MM {
                    span.ramp_in_mm = 0.0;
                }
                if to_mm > l + EPS_MM {
                    span.ramp_out_mm = 0.0;
                }
                span
            })
            .collect();
        spans.sort_by(|a, b| a.start_mm.total_cmp(&b.start_mm));
        let mut overlap = false;
        let mut merged: Vec<Raw> = Vec::new();
        for span in spans {
            match merged.last_mut() {
                Some(last) if span.start_mm <= last.end_mm + EPS_MM => {
                    overlap = true;
                    last.absorb_open(&span);
                }
                _ => merged.push(span),
            }
        }
        layout.full = merged
            .iter()
            .any(|s| s.start_mm <= EPS_MM && s.end_mm >= l - EPS_MM);
        overlap |= layout.full && cuts.len() > 1;
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
            avoid_corners: spec.avoid_corners,
            diameter_mm,
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

    /// The tabs this request puts on `path`, laid out by `tabs`
    /// ([`Tabs::layout`]). With `avoid_corners` each one on a corner moves
    /// off it first, where it can ([`corners`]); one that cannot
    /// stays, and is in [`Fit::on_corners_mm`]. When their spans would cover
    /// more than half the path, so the part or hole would not cut free,
    /// the path gets the most tabs that cover at most half, but
    /// never fewer than `least` allows: `count` tabs re-spaced evenly, or
    /// the first of `at_distances` in their order, moved off corners as
    /// before. What was asked and what is cut are in [`Reduced`], which is
    /// given whenever the asked tabs covered more than half, even when one
    /// kept tab is all that was asked.
    pub(crate) fn fit(&self, tabs: &Tabs, path: &Polyline, ends: PathEnds, least: Least) -> Fit {
        let l = path.length_mm();
        let half_mm = l / 2.0 + EPS_MM;
        // Tabs on an open path are measured from its first vertex as
        // drawn, so its corners are found along the path as drawn.
        let corners = self.avoid_corners.then(|| {
            let drawn = match ends {
                PathEnds::Open { reversed: true } => path.reversed(),
                PathEnds::Closed | PathEnds::Open { reversed: false } => path.clone(),
            };
            corners::Corners::of(&drawn, self.diameter_mm, self.span_mm)
        });
        let lay = |centres: Vec<f64>| -> (Vec<TabCut>, Vec<(f64, TabOnCornerReason)>) {
            let (centres, on_corners) = match &corners {
                Some(corners) => {
                    let (moved, blocked) = corners::avoid(&centres, corners, self.span_mm);
                    let on: Vec<(f64, TabOnCornerReason)> = blocked
                        .iter()
                        .filter_map(|&(i, why)| moved.get(i).map(|&c| (c, why)))
                        .collect();
                    (moved, on)
                }
                None => (centres, Vec::new()),
            };
            (
                centres.into_iter().map(|c| self.cut(c)).collect(),
                on_corners,
            )
        };
        let covered = |cuts: &[TabCut]| tabs.layout(cuts, l, ends).0.covered_mm();
        let centres = tab_centres_mm(&self.placement, l);
        let asked = centres.len();
        let (all, on_corners) = lay(centres.clone());
        if covered(&all) <= half_mm {
            return Fit {
                cuts: all,
                reduced: None,
                on_corners_mm: on_corners,
            };
        }
        // `asked` is at least 1 here: no tabs cover nothing.
        let fewest = match least {
            Least::One => 1,
            Least::Zero => 0,
        };
        let (kept, on_corners) = match &self.placement {
            // Evenly spaced spans that fit in half the path never overlap.
            TabPlacement::Count { count } => {
                let fits = (0..*count)
                    .rev()
                    .find(|&n| f64::from(n) * self.span_mm <= half_mm)
                    .unwrap_or(0)
                    .max(fewest);
                lay(tab_centres_mm(&TabPlacement::Count { count: fits }, l))
            }
            TabPlacement::AtDistances { .. } => {
                let first = |k: usize| lay(centres.iter().take(k).copied().collect());
                (usize::from(fewest)..asked)
                    .rev()
                    .map(first)
                    .find(|(kept, _)| covered(kept) <= half_mm)
                    .unwrap_or_else(|| first(usize::from(fewest)))
            }
        };
        let cut = kept.len();
        Fit {
            cuts: kept,
            reduced: Some(Reduced {
                asked,
                cut,
                placed: false,
            }),
            on_corners_mm: on_corners,
        }
    }

    /// This request as each piece of one outline takes it, the
    /// pieces `lengths_mm` long, in the outline's order: where the board's
    /// edge leaves a male Inlay's cut-out as several open paths, the
    /// request is for the cut-out, not for each of them. One request for
    /// each piece, to lay on it as on any path ([`Request::fit`]).
    ///
    /// `count` tabs are shared among the pieces by length ([`shares`]), no
    /// piece asked for more than cover half of it ([`Request::most`]) while
    /// another has room. Tabs `at_distances` go where their distances fall
    /// along the pieces laid end to end, each taken modulo their whole
    /// length and counted on its piece from that piece's first vertex.
    pub(crate) fn shared(&self, lengths_mm: &[f64]) -> Vec<Self> {
        let placements: Vec<TabPlacement> = match &self.placement {
            TabPlacement::Count { count } => {
                let most: Vec<u32> = lengths_mm.iter().map(|&l| self.most(l)).collect();
                shares(u32::from(*count), lengths_mm, &most)
                    .into_iter()
                    .map(|share| TabPlacement::Count {
                        // No share is more than the count shared.
                        count: u8::try_from(share).unwrap_or(*count),
                    })
                    .collect()
            }
            TabPlacement::AtDistances { distances_mm } => {
                let total_mm: f64 = lengths_mm.iter().sum();
                let mut on: Vec<Vec<f64>> = vec![Vec::new(); lengths_mm.len()];
                let last = lengths_mm.len().saturating_sub(1);
                // Pieces with no length between them hold no tab.
                let distances_mm: &[f64] = if total_mm > EPS_MM { distances_mm } else { &[] };
                for d in distances_mm {
                    let mut along_mm = d.rem_euclid(total_mm);
                    for (i, (piece, &length_mm)) in on.iter_mut().zip(lengths_mm).enumerate() {
                        // Rounding may leave a distance just past the
                        // last piece: it stays on that piece, which
                        // takes it modulo its own length.
                        if along_mm < length_mm || i == last {
                            piece.push(along_mm.min(length_mm));
                            break;
                        }
                        along_mm -= length_mm;
                    }
                }
                on.into_iter()
                    .map(|distances_mm| TabPlacement::AtDistances { distances_mm })
                    .collect()
            }
        };
        placements
            .into_iter()
            .map(|placement| Self {
                placement,
                ..self.clone()
            })
            .collect()
    }

    /// The most tabs of this request whose spans cover at most half of a
    /// path `length_mm` long: what [`Request::fit`] cuts a larger count
    /// down to on a path that may get none.
    fn most(&self, length_mm: f64) -> u32 {
        let half_mm = length_mm / 2.0 + EPS_MM;
        let fits = |n: u32| f64::from(n) * self.span_mm <= half_mm;
        let about = whole(half_mm / self.span_mm);
        // The division may round across a whole number; `fit`'s own test
        // settles it.
        if !fits(about) {
            about.saturating_sub(1)
        } else if about < u32::MAX && fits(about + 1) {
            about + 1
        } else {
            about
        }
    }
}

/// How many of `count` tabs each piece of one outline takes, the
/// pieces `lengths_mm` long and each able to hold `most` of them: shared by
/// length, the largest remainders rounded up so the shares add up to
/// `count` ([`by_length`]). A piece whose share is more than it holds
/// takes what it holds, and the others share the rest the same way, so the
/// whole count is placed whenever the pieces can hold it. When they
/// cannot, the tabs left over are asked of them all by length, and each
/// piece cuts its own down as any path does, with its coverage warning.
pub(crate) fn shares(count: u32, lengths_mm: &[f64], most: &[u32]) -> Vec<u32> {
    let room = |i: usize| most.get(i).copied().unwrap_or(0);
    let mut given = vec![0_u32; lengths_mm.len()];
    // The pieces not yet full, and the tabs not yet given.
    let mut open: Vec<usize> = (0..lengths_mm.len()).collect();
    let mut left = count;
    while left > 0 && !open.is_empty() {
        let lengths: Vec<f64> = open
            .iter()
            .map(|&i| lengths_mm.get(i).copied().unwrap_or(0.0))
            .collect();
        let round = by_length(left, &lengths);
        let full: Vec<usize> = open
            .iter()
            .zip(&round)
            .filter(|&(&i, &n)| n > room(i))
            .map(|(&i, _)| i)
            .collect();
        if full.is_empty() {
            for (&i, &n) in open.iter().zip(&round) {
                if let Some(share) = given.get_mut(i) {
                    *share = n;
                }
            }
            return given;
        }
        for &i in &full {
            if let Some(share) = given.get_mut(i) {
                *share = room(i);
            }
            left = left.saturating_sub(room(i));
        }
        open.retain(|i| !full.contains(i));
    }
    for (share, extra) in given.iter_mut().zip(by_length(left, lengths_mm)) {
        *share = share.saturating_add(extra);
    }
    given
}

/// `count` shared among pieces `lengths_mm` long in proportion to their
/// lengths, by the largest remainder method: each piece takes the whole
/// part of its share, and the tabs then left go one each to the pieces
/// with the largest fractions, the earlier piece first where two are
/// equal to a millionth. The shares add up to `count`; pieces with no
/// length take none, and all zeros when none has any.
fn by_length(count: u32, lengths_mm: &[f64]) -> Vec<u32> {
    let usable = |l: f64| if l.is_finite() && l > 0.0 { l } else { 0.0 };
    let total_mm: f64 = lengths_mm.iter().map(|&l| usable(l)).sum();
    if total_mm <= EPS_MM {
        return vec![0; lengths_mm.len()];
    }
    let exact: Vec<f64> = lengths_mm
        .iter()
        .map(|&l| f64::from(count) * usable(l) / total_mm)
        .collect();
    let mut shares: Vec<u32> = exact.iter().map(|&share| whole(share)).collect();
    // Fractions are compared to a millionth, so pieces equal but for
    // rounding keep their order on every machine.
    let fraction = |i: usize| {
        exact
            .get(i)
            .map_or(0.0, |share| ((share - share.floor()) * 1e6).round())
    };
    let mut order: Vec<usize> = (0..exact.len()).collect();
    order.sort_by(|&a, &b| fraction(b).total_cmp(&fraction(a)));
    let given = shares.iter().fold(0_u32, |sum, &n| sum.saturating_add(n));
    // Fewer than one tab for each piece is left: every whole part is
    // less than one short of its share.
    let short = usize::try_from(count.saturating_sub(given)).unwrap_or(usize::MAX);
    for &i in order.iter().take(short) {
        if let Some(share) = shares.get_mut(i) {
            *share = share.saturating_add(1);
        }
    }
    shares
}

/// `x` rounded down to a whole number: 0 for a negative `x` or NaN, and
/// `u32::MAX` for one past it (a float cast saturates).
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn whole(x: f64) -> u32 {
    x.floor() as u32
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
    /// base. 0 where an end of an open path cuts the span short: it holds
    /// its top from that end ([`Tabs::layout`]).
    ramp_in_mm: f64,
    /// The length a smooth span falls over at its end: half the width of
    /// its last tab; 0 where an end of an open path cuts it short.
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

    /// [`Raw::absorb`] on an open path, where a span an end of the path
    /// cuts short holds its top right to that end (a ramp of no length,
    /// [`Tabs::layout`]): of two spans that start together, or end
    /// together, the merged span keeps the shorter ramp, so the one held
    /// to the end stays held whichever was laid first.
    fn absorb_open(&mut self, next: &Raw) {
        let starts_with = next.start_mm <= self.start_mm + EPS_MM;
        let ends_with = (next.end_mm - self.end_mm).abs() <= EPS_MM;
        let ramp_in_mm = self.ramp_in_mm.min(next.ramp_in_mm);
        let ramp_out_mm = self.ramp_out_mm.min(next.ramp_out_mm);
        self.absorb(next);
        if starts_with {
            self.ramp_in_mm = ramp_in_mm;
        }
        if ends_with {
            self.ramp_out_mm = ramp_out_mm;
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

/// The spans of a closed path `l` long (each start in `[0, L)`, its end
/// possibly beyond `L`) merged where they overlap or touch, including the
/// last into the first through the path start, in start order; and
/// whether any merged.
fn merge_closed(mut spans: Vec<Raw>, l: f64) -> (Vec<Raw>, bool) {
    let mut overlap = false;
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
    (merged, overlap)
}

/// Spans of an open path, each within it, merged in start order where
/// they overlap or touch ([`PathTabs::from_spans`]).
fn merge_open(mut spans: Vec<Raw>) -> Vec<Raw> {
    spans.sort_by(|a, b| a.start_mm.total_cmp(&b.start_mm));
    let mut merged: Vec<Raw> = Vec::new();
    for span in spans {
        match merged.last_mut() {
            Some(last) if span.start_mm <= last.end_mm + EPS_MM => last.absorb_open(&span),
            _ => merged.push(span),
        }
    }
    merged
}

/// One raised stretch of a path for [`PathTabs::from_spans`]: from
/// `start_mm` along it, `length_mm` long (it may run through the start),
/// at `top_z_mm`; a smooth one rises over `ramp_in_mm` and falls over
/// `ramp_out_mm`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RaisedSpan {
    /// Where it starts, mm along the path.
    pub start_mm: f64,
    /// How long it is, mm.
    pub length_mm: f64,
    /// Its top, mm above the stock bottom.
    pub top_z_mm: f64,
    /// Rect holds the top over it all; smooth ramps.
    pub shape: TabShape,
    /// A smooth span's rise, mm.
    pub ramp_in_mm: f64,
    /// A smooth span's fall, mm.
    pub ramp_out_mm: f64,
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

    /// These spans, laid on a closed path, carried onto another closed
    /// path `length_mm` long that runs beside it (design A8 step 3):
    /// `along` maps a distance along this path to the distance along the
    /// other of the point nearest it. Each span runs on the other path
    /// from where its start lands to where its end lands (from its end to
    /// its start when `reversed`: the other path runs the other way, so
    /// its ramps swap too), widened by `widen_mm` at both ends, and its
    /// smooth ramps lengthened by as much, so that at every point it is
    /// at least as high as this one where it lands. Spans that then
    /// overlap merge; spans that cover the path cover the other. Fails as
    /// `along` does, where a span landing anywhere else would leave its
    /// tab to be cut through.
    pub(crate) fn carried(
        &self,
        length_mm: f64,
        along: impl Fn(f64) -> Result<f64, CamError>,
        reversed: bool,
        widen_mm: f64,
    ) -> Result<Self, CamError> {
        let l = length_mm;
        let mut carried = Self {
            length_mm: l,
            spans: Vec::new(),
            full: self.full,
            closed: true,
            tab_count: self.tab_count,
        };
        if self.full || l <= EPS_MM || self.length_mm <= EPS_MM {
            carried.spans = self
                .spans
                .iter()
                .map(|s| Span {
                    start_mm: 0.0,
                    length_mm: l,
                    ..*s
                })
                .collect();
            return Ok(carried);
        }
        let spans: Vec<Raw> = self
            .spans
            .iter()
            .map(|s| {
                let (from_mm, to_mm) = if reversed {
                    (along(s.end_mm())?, along(s.start_mm)?)
                } else {
                    (along(s.start_mm)?, along(s.end_mm())?)
                };
                let (ramp_in_mm, ramp_out_mm) = if reversed {
                    (s.ramp_out_mm, s.ramp_in_mm)
                } else {
                    (s.ramp_in_mm, s.ramp_out_mm)
                };
                // The other path is about as long as this one here: a
                // span that would wrap most of the way round is a short
                // one whose ends landed out of order.
                let expected_mm = s.length_mm * l / self.length_mm;
                let mut run_mm = (to_mm - from_mm).rem_euclid(l);
                if run_mm - expected_mm > l / 2.0 {
                    run_mm -= l;
                }
                let start_mm = (from_mm - widen_mm).rem_euclid(l);
                Ok(Raw {
                    start_mm,
                    end_mm: start_mm + run_mm.max(0.0) + 2.0 * widen_mm,
                    top_z_mm: s.top_z_mm,
                    shape: s.shape,
                    ramp_in_mm: ramp_in_mm + widen_mm,
                    ramp_out_mm: ramp_out_mm + widen_mm,
                })
            })
            .collect::<Result<_, CamError>>()?;
        let (merged, _) = merge_closed(spans, l);
        carried.full = merged.iter().any(|s| s.end_mm - s.start_mm >= l - EPS_MM);
        carried.spans = merged.iter().map(Raw::span).collect();
        Ok(carried)
    }

    /// The raised stretches `spans` on a path `length_mm` long, laid for
    /// `tab_count` tabs, merged where they overlap or touch
    /// (a stock path's spans, found from the tabs' material). On a
    /// `closed` path a span may run through its start; on an open one
    /// each is cut short at the path's ends.
    pub(crate) fn from_spans(
        length_mm: f64,
        tab_count: usize,
        spans: &[RaisedSpan],
        closed: bool,
    ) -> Self {
        let l = length_mm;
        let raw: Vec<Raw> = spans
            .iter()
            .map(|s| {
                let (start_mm, end_mm) = if !closed {
                    (s.start_mm.max(0.0), (s.start_mm + s.length_mm).min(l))
                } else if l > 0.0 {
                    let start_mm = s.start_mm.rem_euclid(l);
                    (start_mm, start_mm + s.length_mm.max(0.0))
                } else {
                    (s.start_mm, s.start_mm + s.length_mm.max(0.0))
                };
                Raw {
                    start_mm,
                    end_mm: end_mm.max(start_mm),
                    top_z_mm: s.top_z_mm,
                    shape: s.shape,
                    ramp_in_mm: s.ramp_in_mm,
                    ramp_out_mm: s.ramp_out_mm,
                }
            })
            .collect();
        let merged = if closed {
            merge_closed(raw, l).0
        } else {
            merge_open(raw)
        };
        let full = if closed {
            merged.iter().any(|s| s.end_mm - s.start_mm >= l - EPS_MM)
        } else {
            merged
                .iter()
                .any(|s| s.start_mm <= EPS_MM && s.end_mm >= l - EPS_MM)
        };
        Self {
            length_mm: l,
            full,
            spans: merged.iter().map(Raw::span).collect(),
            closed,
            tab_count,
        }
    }

    /// These spans and `other`'s, laid on the same path, merged where they
    /// overlap or touch (a lap's own tabs and the tabs of other
    /// outlines its cutter reaches). The tab count stays this layout's.
    pub(crate) fn merged(&self, other: &PathTabs) -> Self {
        let l = self.length_mm;
        let raw: Vec<Raw> = self
            .spans
            .iter()
            .chain(&other.spans)
            .map(|s| Raw {
                start_mm: s.start_mm,
                end_mm: s.end_mm(),
                top_z_mm: s.top_z_mm,
                shape: s.shape,
                ramp_in_mm: s.ramp_in_mm,
                ramp_out_mm: s.ramp_out_mm,
            })
            .collect();
        let merged = if self.closed {
            merge_closed(raw, l).0
        } else {
            merge_open(raw)
        };
        let covers = if self.closed {
            merged.iter().any(|s| s.end_mm - s.start_mm >= l - EPS_MM)
        } else {
            merged
                .iter()
                .any(|s| s.start_mm <= EPS_MM && s.end_mm >= l - EPS_MM)
        };
        Self {
            length_mm: l,
            spans: merged.iter().map(Raw::span).collect(),
            full: self.full || other.full || covers,
            closed: self.closed,
            tab_count: self.tab_count,
        }
    }

    /// These spans, laid on an open path, measured from its other end:
    /// the layout of the path run backwards (a reversed last pass),
    /// so the tabs stay where they are. Each span's ramps swap. A closed
    /// path's layout is returned as it is; [`PathTabs::carried`] reverses
    /// one.
    pub(crate) fn reversed_open(&self) -> Self {
        if self.closed {
            return self.clone();
        }
        let l = self.length_mm;
        let mut spans: Vec<Span> = self
            .spans
            .iter()
            .map(|s| Span {
                start_mm: (l - s.end_mm()).max(0.0),
                ramp_in_mm: s.ramp_out_mm,
                ramp_out_mm: s.ramp_in_mm,
                ..*s
            })
            .collect();
        spans.reverse();
        Self {
            spans,
            ..self.clone()
        }
    }

    /// The Z, mm, a pass at `base_z_mm` cuts at `distance_mm` along the
    /// path: `base_z_mm`, or higher over a tab.
    pub(crate) fn z_at_mm(&self, distance_mm: f64, base_z_mm: f64) -> f64 {
        self.z_at(distance_mm, base_z_mm)
    }

    /// The top and shape of the span over `distance_mm` along the path,
    /// or `None` where no span raises it.
    pub(crate) fn span_at(&self, distance_mm: f64) -> Option<(f64, TabShape)> {
        if self.full {
            let top = self
                .spans
                .iter()
                .map(|s| s.top_z_mm)
                .fold(f64::NEG_INFINITY, f64::max);
            let shape = if self.spans.iter().all(|s| s.shape == TabShape::Smooth) {
                TabShape::Smooth
            } else {
                TabShape::Rect
            };
            return Some((top, shape));
        }
        self.spans.iter().find_map(|span| {
            let t = if self.closed {
                (distance_mm - span.start_mm).rem_euclid(self.length_mm)
            } else {
                distance_mm - span.start_mm
            };
            (0.0..span.length_mm)
                .contains(&t)
                .then_some((span.top_z_mm, span.shape))
        })
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
/// down ([`Request::fit`]), all when placed tabs are cut as placed.
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
        placed: reduced.placed,
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
/// an open path). Where `tabs` is given
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
mod tests;
