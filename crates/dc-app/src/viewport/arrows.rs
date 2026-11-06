// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/canvas/toolpathArrows.ts

//! Where the direction arrows go (`docs/specs/phase-4/design.md`,
//! "PureCutCNC sources", `arrows`; functional spec, capability 72: "Direction
//! arrows along cuts are a toggle"), decided once per toolpath and view
//! scale.
//!
//! From PureCutCNC's `toolpathArrows.ts`: the spacing and force-length
//! thresholds in screen pixels from the toolpath's span, a running travel
//! per kind since the last arrow, the forced arrow on a long move, and the
//! arrow on a short connector that turns sharply against both neighbours.
//! Positions are stored in *scaled world* space (world × scale, no pan
//! offset), so panning reuses them and only a zoom rebuilds, which is
//! right: the thresholds are in pixels, so which moves earn an arrow
//! changes with zoom and not with pan. [`ArrowCache`] keeps one answer per
//! toolpath, keyed on everything that changes it.
//!
//! What changes here: DreamCarve's toolpaths carry arcs, so the pass runs
//! on [`moves_of`]'s lines, each arc flattened for display; a cutting
//! move has no `from` of its own, so a move starts where the last one
//! ended. PureCutCNC's separate retraction layer and its Z-level filter
//! do not exist (capability 73 has one *Rapids* toggle), so a rapid up and
//! a level rapid are gated together. Y-up changes nothing: lengths and the
//! angle between two directions are the same either way up.

use std::collections::BTreeMap;

use dc_geometry::Point3;
use dc_job::Toolpath;

use crate::scene::{StrokeKind, strokes};
use crate::view::Visibility;

/// Which layer a move belongs to, and so which arrows it can carry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArrowKind {
    /// A cutting move.
    Cut,
    /// A rapid.
    Rapid,
}

/// One straight move the pass reads: a toolpath's line, or a chord of one
/// of its arcs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrowMove {
    /// Its layer.
    pub kind: ArrowKind,
    /// Where it starts, mm.
    pub from: Point3,
    /// Where it ends, mm.
    pub to: Point3,
}

/// The visibility flags the pass reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArrowVisibility {
    /// Cutting moves are drawn.
    pub cuts: bool,
    /// Rapids are drawn.
    pub rapids: bool,
}

impl ArrowVisibility {
    /// Both layers drawn.
    pub const ALL: Self = Self {
        cuts: true,
        rapids: true,
    };
}

impl From<&Visibility> for ArrowVisibility {
    fn from(visibility: &Visibility) -> Self {
        Self {
            cuts: visibility.cuts,
            rapids: visibility.rapids,
        }
    }
}

/// The arrows of one toolpath at one scale, in scaled-world space: each is
/// `[from_x, from_y, to_x, to_y]`, the move it sits on, multiplied by the
/// scale and not offset. Add the pan offset to draw.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Placements {
    /// Arrows on cutting moves.
    pub cut: Vec<[f64; 4]>,
    /// Arrows on rapids.
    pub rapid: Vec<[f64; 4]>,
}

impl Placements {
    /// The arrows of `kind`.
    #[must_use]
    pub fn of(&self, kind: ArrowKind) -> &[[f64; 4]] {
        match kind {
            ArrowKind::Cut => &self.cut,
            ArrowKind::Rapid => &self.rapid,
        }
    }
}

/// `toolpath`'s moves as the pass reads them: every move a line from where
/// the last ended, each arc flattened to chords within `tolerance_mm`. The
/// first move only places the tool and a dwell does not move it.
#[must_use]
pub fn moves_of(toolpath: &Toolpath, tolerance_mm: f64) -> Vec<ArrowMove> {
    let mut out = Vec::with_capacity(toolpath.moves.len());
    for stroke in strokes(toolpath) {
        let kind = match stroke.kind {
            StrokeKind::Rapid => ArrowKind::Rapid,
            StrokeKind::Cut | StrokeKind::Plunge => ArrowKind::Cut,
        };
        let points = stroke.points(tolerance_mm);
        out.extend(points.windows(2).filter_map(|w| match w {
            [from, to] => Some(ArrowMove {
                kind,
                from: *from,
                to: *to,
            }),
            _ => None,
        }));
    }
    out
}

/// `dx, dy`'s length. PureCutCNC avoids `Math.hypot` here for speed;
/// coordinates are nowhere near where the difference matters.
fn distance(dx: f64, dy: f64) -> f64 {
    (dx * dx + dy * dy).sqrt()
}

/// The longest side of the box around `moves`' ends, all three axes, mm;
/// `None` when there are none.
fn span_mm(moves: &[ArrowMove]) -> Option<f64> {
    let mut points = moves.iter().flat_map(|m| [m.from, m.to]);
    let first = points.next()?;
    let (mut min, mut max) = (first, first);
    for p in points {
        min = Point3::new(min.x.min(p.x), min.y.min(p.y), min.z.min(p.z));
        max = Point3::new(max.x.max(p.x), max.y.max(p.y), max.z.max(p.z));
    }
    Some((max.x - min.x).max(max.y - min.y).max(max.z - min.z))
}

/// The arrow-placement pass (PureCutCNC `computeToolpathArrowPlacements`).
/// Pure; a render path wants [`ArrowCache`]. `scale` is the view's pixels
/// per millimetre; positions come back multiplied by it but not offset.
#[must_use]
pub fn compute(moves: &[ArrowMove], scale: f64, visibility: ArrowVisibility) -> Placements {
    let mut out = Placements::default();
    let Some(span) = span_mm(moves) else {
        return out;
    };
    let preferred_spacing = (span * scale * 0.09).clamp(12.0, 40.0);
    let preferred_arrow_length = (span * scale * 0.03).clamp(8.5, 18.0);
    let mut since_cut = 0.0;
    let mut since_rapid = 0.0;

    // The normalised screen direction of a neighbour move, if it has one.
    let neighbour = |m: Option<&ArrowMove>| -> Option<(f64, f64)> {
        let m = m?;
        let dx = (m.to.x - m.from.x) * scale;
        let dy = (m.to.y - m.from.y) * scale;
        let length = distance(dx, dy);
        (length > 0.001).then(|| (dx / length, dy / length))
    };

    for (index, m) in moves.iter().enumerate() {
        match m.kind {
            ArrowKind::Cut if !visibility.cuts => continue,
            ArrowKind::Rapid if !visibility.rapids => continue,
            _ => {}
        }
        let (from_x, from_y) = (m.from.x * scale, m.from.y * scale);
        let (to_x, to_y) = (m.to.x * scale, m.to.y * scale);
        let (dx, dy) = (to_x - from_x, to_y - from_y);
        let length = distance(dx, dy);
        if length < 0.5 {
            continue;
        }
        let since = match m.kind {
            ArrowKind::Cut => &mut since_cut,
            ArrowKind::Rapid => &mut since_rapid,
        };
        *since += length;

        let force = length >= preferred_arrow_length * 1.1;
        let by_spacing = *since >= preferred_spacing;
        // A short cut that turns sharply against both neighbours gets an
        // arrow even when spacing says no; the neighbours are read only
        // when neither cheap test placed it.
        let mut connector = false;
        if !force && !by_spacing && m.kind == ArrowKind::Cut && length <= preferred_spacing * 0.8 {
            let (ux, uy) = (dx / length, dy / length);
            let previous = index.checked_sub(1).and_then(|i| moves.get(i));
            if let Some((px, py)) = neighbour(previous)
                && let Some((nx, ny)) = neighbour(moves.get(index + 1))
            {
                let previous_turn = (ux * px + uy * py).clamp(-1.0, 1.0).acos();
                let next_turn = (ux * nx + uy * ny).clamp(-1.0, 1.0).acos();
                connector = previous_turn.min(next_turn) >= std::f64::consts::PI / 10.0;
            }
        }
        if !force && !by_spacing && !connector {
            continue;
        }
        let arrow = [from_x, from_y, to_x, to_y];
        match m.kind {
            ArrowKind::Cut => out.cut.push(arrow),
            ArrowKind::Rapid => out.rapid.push(arrow),
        }
        *since = 0.0;
    }
    out
}

/// One cached answer and what it was computed from.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    scale_bits: u64,
    visibility: ArrowVisibility,
    placements: Placements,
}

/// [`compute`] cached per toolpath (PureCutCNC `toolpathArrowPlacements`):
/// one entry per toolpath key, reused while the scale and the flags the
/// pass reads are unchanged, so a pan hits and a zoom rebuilds. The key
/// names the toolpath (its operation and result); a new result needs a
/// new key.
#[derive(Debug, Clone, Default)]
pub struct ArrowCache {
    entries: BTreeMap<u64, Entry>,
    builds: usize,
}

impl ArrowCache {
    /// An empty cache.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The arrows of the toolpath `key`, whose moves are `moves`, at
    /// `scale` with `visibility`: the cached answer when those match the
    /// last call for `key`, else a fresh [`compute`], stored.
    pub fn placements(
        &mut self,
        key: u64,
        moves: &[ArrowMove],
        scale: f64,
        visibility: ArrowVisibility,
    ) -> &Placements {
        let scale_bits = scale.to_bits();
        let builds = &mut self.builds;
        let mut build = || {
            *builds += 1;
            Entry {
                scale_bits,
                visibility,
                placements: compute(moves, scale, visibility),
            }
        };
        let entry = self.entries.entry(key).or_insert_with(&mut build);
        if entry.scale_bits != scale_bits || entry.visibility != visibility {
            *entry = build();
        }
        &entry.placements
    }

    /// Forgets every toolpath not in `keys` (a result replaced, an
    /// operation deleted).
    pub fn retain(&mut self, keys: &[u64]) {
        self.entries.retain(|k, _| keys.contains(k));
    }

    /// How many times [`ArrowCache::placements`] has run the pass: a test's
    /// window on hits and misses.
    #[must_use]
    pub fn builds(&self) -> usize {
        self.builds
    }
}
