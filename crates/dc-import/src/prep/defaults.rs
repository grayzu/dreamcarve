// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The calibration of the two defaults (slice 21f; `docs/specs/phase-3/
//! design.md`, "Calibration of the defaults"). The sweep the README of
//! `tests/fixtures/prep/` records is run here, on the synthetic
//! `speckled-dc.svg` and `jagged-dc.svg`, and the tests pin the constants to
//! its result: a later change to either fails the sweep's own scoring.
//!
//! `cargo test -p dc-import --lib print_the_sweep -- --ignored --nocapture`
//! prints the README's tables.

#![cfg(test)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::print_stdout,
    reason = "tests"
)]

use super::{DEFAULT_MIN_DETAIL_PCT, DEFAULT_SMOOTH_PCT, PrepMode, PrepOptions, Prepared, prepare};
use dc_geometry::{Point, Region, area_mm2, intersection, union};

const SPECKLED: &str = include_str!("../../../../tests/fixtures/prep/speckled-dc.svg");
const JAGGED: &str = include_str!("../../../../tests/fixtures/prep/jagged-dc.svg");
const RING_CROSS: &str = include_str!("../../../../tests/fixtures/prep/ring-cross.svg");

/// The sweep's grids (design spec, "Calibration of the defaults").
const MIN_DETAIL_GRID_PCT: [f64; 6] = [0.05, 0.1, 0.2, 0.3, 0.5, 1.0];
const SMOOTH_GRID_PCT: [f64; 5] = [0.0, 0.02, 0.05, 0.1, 0.2];
/// Scoring: the smoothed jitter loses at least this share of its vertices.
const MIN_REDUCTION: f64 = 0.80;
/// Scoring: smoothing changes at most this share of the design's area.
const MAX_CHANGED_AREA: f64 = 0.001;
/// Slack on distances, mm (the importer's rounding).
const SLACK_MM: f64 = 1.0e-6;

/// Which kind of mark a fixture's header lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// A filled dot, a piece of its own.
    Speck,
    /// A hole in a letter.
    Pinhole,
}

/// Which population a mark belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Band {
    /// Trace noise: the widest point under 0.2 % of the height.
    Noise,
    /// Small but not noise: 0.25 to 0.8 % of the height.
    Small,
    /// Intended detail: from 0.9 % of the height.
    Intended,
}

/// One seeded mark, from a `mark` line of the fixture's header comment.
#[derive(Debug, Clone)]
struct Mark {
    kind: Kind,
    band: Band,
    x_mm: f64,
    /// SVG, Y-down.
    y_down_mm: f64,
    diameter_mm: f64,
}

fn marks(svg: &str) -> Vec<Mark> {
    let mut found = Vec::new();
    for line in svg.lines() {
        let words: Vec<&str> = line.split_whitespace().collect();
        let ["mark", kind, band, _id, x_mm, y_down_mm, diameter_mm] = words[..] else {
            continue;
        };
        found.push(Mark {
            kind: match kind {
                "speck" => Kind::Speck,
                "pinhole" => Kind::Pinhole,
                other => panic!("mark kind {other}"),
            },
            band: match band {
                "noise" => Band::Noise,
                "small" => Band::Small,
                "intended" => Band::Intended,
                other => panic!("mark band {other}"),
            },
            x_mm: x_mm.parse().unwrap(),
            y_down_mm: y_down_mm.parse().unwrap(),
            diameter_mm: diameter_mm.parse().unwrap(),
        });
    }
    found
}

fn options(min_detail_pct: f64, smooth_pct: f64) -> PrepOptions {
    PrepOptions {
        mode: PrepMode::Shapes,
        min_detail_pct,
        smooth_pct,
        ..PrepOptions::default()
    }
}

fn run(svg: &str, min_detail_pct: f64, smooth_pct: f64) -> Prepared {
    prepare(svg.as_bytes(), &options(min_detail_pct, smooth_pct))
        .expect("prepare")
        .0
}

/// Whether `mark` is still in the result: a speck is a piece covering its
/// centre, a pinhole is a centre that is no longer open.
fn survives(mark: &Mark, prepared: &Prepared) -> bool {
    let at = Point::new(mark.x_mm, prepared.page_height_mm - mark.y_down_mm);
    let covered = prepared.pieces.iter().any(|r| r.contains_point(at));
    match mark.kind {
        Kind::Speck => covered,
        Kind::Pinhole => !covered,
    }
}

fn vertex_count(regions: &[Region]) -> usize {
    regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(|c| c.polyline().vertices().len())
        .sum()
}

/// One row of the `min_detail` sweep on `speckled-dc.svg`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct DetailRow {
    min_detail_pct: f64,
    noise: (usize, usize),
    small: (usize, usize),
    intended: (usize, usize),
    /// Marks that went although at or above the threshold, or stayed
    /// although under it: the contract's own check.
    contract_breaks: usize,
}

impl DetailRow {
    /// Every noise mark gone and no intended detail gone.
    fn passes(&self) -> bool {
        self.noise.0 == self.noise.1 && self.intended.0 == 0 && self.contract_breaks == 0
    }
}

/// `(gone, total)` of the marks in `band`.
fn gone(marks: &[Mark], prepared: &Prepared, band: Band) -> (usize, usize) {
    let in_band: Vec<&Mark> = marks.iter().filter(|m| m.band == band).collect();
    (
        in_band.iter().filter(|m| !survives(m, prepared)).count(),
        in_band.len(),
    )
}

fn detail_row(min_detail_pct: f64) -> DetailRow {
    let marks = marks(SPECKLED);
    let prepared = run(SPECKLED, min_detail_pct, 0.0);
    let threshold_mm = min_detail_pct / 100.0 * prepared.report.design_height_mm;
    let contract_breaks = marks
        .iter()
        .filter(|m| (m.diameter_mm < threshold_mm) == survives(m, &prepared))
        .count();
    DetailRow {
        min_detail_pct,
        noise: gone(&marks, &prepared, Band::Noise),
        small: gone(&marks, &prepared, Band::Small),
        intended: gone(&marks, &prepared, Band::Intended),
        contract_breaks,
    }
}

/// Shortest distance from `p` to the segment `a`-`b`, mm.
fn to_segment_mm(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let length2 = dx * dx + dy * dy;
    let t = if length2 > 0.0 {
        (((p.x - a.x) * dx + (p.y - a.y) * dy) / length2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.distance_to(Point::new(a.x + t * dx, a.y + t * dy))
}

/// The largest distance from a vertex of `smoothed` to the outlines of
/// `original`, mm.
fn furthest_vertex_mm(original: &[Region], smoothed: &[Region]) -> f64 {
    let segments: Vec<(Point, Point)> = original
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .flat_map(|c| c.polyline().segments())
        .map(|s| (s.start, s.end))
        .collect();
    smoothed
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .flat_map(|c| c.polyline().vertices().iter())
        .map(|v| {
            let p = Point::new(v.x, v.y);
            segments
                .iter()
                .map(|&(a, b)| to_segment_mm(p, a, b))
                .fold(f64::INFINITY, f64::min)
        })
        .fold(0.0, f64::max)
}

/// One row of the `smooth` sweep on `jagged-dc.svg`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct SmoothRow {
    smooth_pct: f64,
    /// Share of the vertices removed, 0 to 1.
    reduction: f64,
    /// The furthest an output vertex lies from the input, mm.
    furthest_mm: f64,
    eps_mm: f64,
    /// Share of the design's box changed.
    changed: f64,
}

impl SmoothRow {
    fn passes(&self) -> bool {
        self.reduction >= MIN_REDUCTION
            && self.furthest_mm <= self.eps_mm + SLACK_MM
            && self.changed <= MAX_CHANGED_AREA
    }
}

#[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
fn smooth_row(smooth_pct: f64) -> SmoothRow {
    let original = run(JAGGED, 0.0, 0.0);
    let smoothed = run(JAGGED, 0.0, smooth_pct);
    let height_mm = original.report.design_height_mm;
    let before = vertex_count(&original.pieces);
    let after = vertex_count(&smoothed.pieces);
    let both = intersection(&original.pieces, &smoothed.pieces, 1.0e-4);
    let changed_mm2 =
        area_mm2(&original.pieces) + area_mm2(&smoothed.pieces) - 2.0 * area_mm2(&both);
    // The design's box: its height by its width.
    let all = union(&original.pieces, &[], 1.0e-4);
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in all
        .iter()
        .flat_map(|r| r.outer().polyline().vertices().iter())
    {
        lo = lo.min(v.x);
        hi = hi.max(v.x);
    }
    SmoothRow {
        smooth_pct,
        reduction: 1.0 - after as f64 / before as f64,
        furthest_mm: furthest_vertex_mm(&original.pieces, &smoothed.pieces),
        eps_mm: smooth_pct / 100.0 * height_mm,
        changed: changed_mm2 / (height_mm * (hi - lo)),
    }
}

#[test]
fn fixtures_keep_their_intent_at_the_defaults() {
    assert!(
        DEFAULT_SMOOTH_PCT <= DEFAULT_MIN_DETAIL_PCT / 2.0,
        "smoothing never fights the speck threshold"
    );

    // speckled-dc.svg: every noise speck and pinhole gone, no intended
    // detail gone, and nothing else decided differently from the contract
    // (a mark is gone exactly when under the threshold).
    let row = detail_row(DEFAULT_MIN_DETAIL_PCT);
    assert_eq!(
        row.noise.0, row.noise.1,
        "noise left at the default: {row:?}"
    );
    assert_eq!(row.intended.0, 0, "intended detail lost: {row:?}");
    assert_eq!(row.contract_breaks, 0, "{row:?}");
    assert!(row.noise.1 >= 12 && row.intended.1 >= 6, "{row:?}");

    // The serifs and the counter are in the letters, not marks: the D's
    // counter survives the default run.
    let kept = run(SPECKLED, DEFAULT_MIN_DETAIL_PCT, DEFAULT_SMOOTH_PCT);
    let large_holes = kept
        .pieces
        .iter()
        .flat_map(|r| r.holes())
        .filter(|h| h.signed_area_mm2().abs() > 1.0)
        .count();
    assert_eq!(large_holes, 1, "the D's counter");

    // jagged-dc.svg: the jitter goes, the letters stay.
    let smooth = smooth_row(DEFAULT_SMOOTH_PCT);
    assert!(smooth.reduction >= MIN_REDUCTION, "{smooth:?}");
    assert!(
        smooth.furthest_mm <= smooth.eps_mm + SLACK_MM,
        "a vertex moved more than eps: {smooth:?}"
    );
    assert!(smooth.changed <= MAX_CHANGED_AREA, "{smooth:?}");
}

/// The defaults are what the sweep picks: the smallest grid value that
/// passes, `smooth` also at most half of `min_detail`.
#[test]
fn defaults_are_the_smallest_grid_values_that_pass() {
    let detail = MIN_DETAIL_GRID_PCT
        .into_iter()
        .find(|&t| detail_row(t).passes())
        .expect("some min_detail passes");
    assert!((detail - DEFAULT_MIN_DETAIL_PCT).abs() < 1.0e-12, "{detail}");
    let smooth = SMOOTH_GRID_PCT
        .into_iter()
        .filter(|&s| s > 0.0)
        .find(|&s| smooth_row(s).passes())
        .expect("some smooth passes");
    assert!((smooth - DEFAULT_SMOOTH_PCT).abs() < 1.0e-12, "{smooth}");
    assert!(smooth <= detail / 2.0);
}

/// Prints the README's tables.
#[test]
#[ignore = "prints the sweep for tests/fixtures/prep/README.md"]
fn print_the_sweep() {
    let fraction = |(gone, total): (usize, usize)| format!("{gone} / {total}");
    println!("| min_detail % | noise gone | small gone | intended gone | pass |");
    println!("| --- | --- | --- | --- | --- |");
    for t in MIN_DETAIL_GRID_PCT {
        let r = detail_row(t);
        println!(
            "| {t} | {} | {} | {} | {} |",
            fraction(r.noise),
            fraction(r.small),
            fraction(r.intended),
            if r.passes() { "yes" } else { "no" }
        );
    }
    println!();
    println!("| smooth % | vertices removed | furthest vertex / eps | changed area | pass |");
    println!("| --- | --- | --- | --- | --- |");
    for s in SMOOTH_GRID_PCT {
        let r = smooth_row(s);
        println!(
            "| {s} | {:.1} % | {:.3} | {:.4} % | {} |",
            r.reduction * 100.0,
            if r.eps_mm > 0.0 {
                r.furthest_mm / r.eps_mm
            } else {
                0.0
            },
            r.changed * 100.0,
            if r.passes() { "yes" } else { "no" }
        );
    }
    println!();
    println!("| min_detail % | ring-cross pieces | holes | dropped | filled |");
    println!("| --- | --- | --- | --- | --- |");
    for t in MIN_DETAIL_GRID_PCT {
        let p = run(RING_CROSS, t, 0.0);
        println!(
            "| {t} | {} | {} | {} | {} |",
            p.report.pieces, p.report.holes, p.report.dropped_pieces, p.report.filled_holes
        );
    }
}
