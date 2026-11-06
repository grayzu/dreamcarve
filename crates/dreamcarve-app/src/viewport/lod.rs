// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/canvas/toolpathDisplay.ts

//! Display-only coalescing of toolpath segments
//! (`docs/specs/phase-4/design.md`, "The Viewport renderer", `lod`):
//! connected segments of one style whose summed travel stays under a
//! pixel draw as one. Emitted motion never passes through here: the input
//! is the renderer's segments, built from the Scene's runs, and the
//! output goes to the GPU only.
//!
//! From PureCutCNC's `displaySegments`: only connected runs coalesce (each
//! segment starting exactly where the last ended), of the same style
//! (there, the same feed scale; here, the same style word, so a depth step
//! or an operation colour is never blurred), and the limit is the summed
//! *travel*, not the distance between the ends, so a tight reversal does
//! not vanish: every original point stays within half the limit of the
//! chord. PureCutCNC allows three canvas pixels; DreamCarve one, as the
//! design spec says, and the caller rebuilds per zoom band
//! (`layers::lod_mm`), so zooming in restores detail. Its spatial index
//! is not ported: the GPU draws every segment of a buffer, and panning
//! re-uploads nothing.

use crate::viewport::renderer::Segment;

/// The XY length of `s`, mm.
fn length_mm(s: &Segment) -> f64 {
    let dx = f64::from(s.b_mm[0]) - f64::from(s.a_mm[0]);
    let dy = f64::from(s.b_mm[1]) - f64::from(s.a_mm[1]);
    dx.hypot(dy)
}

/// Whether `next` starts exactly where `pending` ends: bit for bit, as
/// upstream compares, so only truly connected runs join.
fn connected(pending: &Segment, next: &Segment) -> bool {
    pending.b_mm[0].to_bits() == next.a_mm[0].to_bits()
        && pending.b_mm[1].to_bits() == next.a_mm[1].to_bits()
}

/// `segments` with each connected run of one style coalesced while its
/// summed travel stays within `max_travel_mm`, in order. A segment longer
/// than the limit passes through whole.
#[must_use]
pub(crate) fn coalesce(segments: &[Segment], max_travel_mm: f64) -> Vec<Segment> {
    let mut out = Vec::with_capacity(segments.len());
    let mut pending: Option<(Segment, f64)> = None;
    for next in segments {
        let length = length_mm(next);
        match pending.as_mut() {
            Some((p, travel))
                if connected(p, next)
                    && p.style == next.style
                    && *travel + length <= max_travel_mm =>
            {
                p.b_mm = next.b_mm;
                *travel += length;
            }
            _ => {
                if let Some((p, _)) = pending.take() {
                    out.push(p);
                }
                pending = Some((*next, length));
            }
        }
    }
    if let Some((p, _)) = pending {
        out.push(p);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::viewport::renderer::{StyleId, StyleWord};

    fn seg(a: (f32, f32), b: (f32, f32), style: StyleWord) -> Segment {
        Segment {
            a_mm: [a.0, a.1],
            b_mm: [b.0, b.1],
            along_mm: 0.0,
            style,
        }
    }

    fn cut() -> StyleWord {
        StyleWord::toolpath(StyleId::Cut, Some(0), None, false)
    }

    /// Upstream's rule: connected same-style runs coalesce up to the
    /// limit of summed travel, then a new one starts.
    #[test]
    fn connected_runs_coalesce_under_the_limit() {
        let tiny: Vec<Segment> = (0..10u8)
            .map(|i| {
                // Eighths of a millimetre, exact in binary, so each move
                // starts bit for bit where the last ended.
                let x = f32::from(i) * 0.125;
                seg((x, 0.0), (x + 0.125, 0.0), cut())
            })
            .collect();
        let out = coalesce(&tiny, 0.5);
        assert_eq!(
            out.len(),
            3,
            "1.25 mm of 0.125 mm moves in runs of 0.5 mm: {out:?}"
        );
        assert_eq!(out[0].a_mm, [0.0, 0.0]);
        assert_eq!(out[2].b_mm, tiny[9].b_mm, "the run still ends where it did");
    }

    /// A tight reversal is travel, not displacement: it does not vanish.
    #[test]
    fn a_reversal_counts_its_travel() {
        let there = seg((0.0, 0.0), (0.4, 0.0), cut());
        let back = seg((0.4, 0.0), (0.0, 0.0), cut());
        let out = coalesce(&[there, back], 0.5);
        assert_eq!(out.len(), 2, "0.8 mm of travel is over the 0.5 mm limit");
    }

    /// Different styles (a depth step, a colour, a rapid) and gaps never
    /// join, and nothing over the limit changes.
    #[test]
    fn styles_and_gaps_keep_segments_apart() {
        let deeper = StyleWord::toolpath(StyleId::Cut, Some(0), Some(9), false);
        let a = seg((0.0, 0.0), (0.1, 0.0), cut());
        let b = seg((0.1, 0.0), (0.2, 0.0), deeper);
        let c = seg((0.3, 0.0), (0.4, 0.0), deeper);
        let long = seg((0.4, 0.0), (9.0, 0.0), deeper);
        let out = coalesce(&[a, b, c, long], 0.5);
        assert_eq!(out, vec![a, b, c, long]);
    }
}
