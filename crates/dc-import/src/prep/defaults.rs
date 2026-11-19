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
use dc_geometry::{Point, Region, area_mm2, intersection};

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

/// The marks `prepared` decided differently from the contract at
/// `min_detail_pct`: gone although at or above the threshold, or still
/// there although under it.
fn contract_breaks(marks: &[Mark], prepared: &Prepared, min_detail_pct: f64) -> usize {
    let threshold_mm = min_detail_pct / 100.0 * prepared.report.design_height_mm;
    marks
        .iter()
        .filter(|m| (m.diameter_mm < threshold_mm) == survives(m, prepared))
        .count()
}

fn detail_row(min_detail_pct: f64) -> DetailRow {
    let marks = marks(SPECKLED);
    let prepared = run(SPECKLED, min_detail_pct, 0.0);
    DetailRow {
        min_detail_pct,
        noise: gone(&marks, &prepared, Band::Noise),
        small: gone(&marks, &prepared, Band::Small),
        intended: gone(&marks, &prepared, Band::Intended),
        contract_breaks: contract_breaks(&marks, &prepared, min_detail_pct),
    }
}

/// The area in one of `original` and `result` and not in the other, mm².
fn changed_mm2(original: &[Region], result: &[Region]) -> f64 {
    let both = intersection(original, result, 1.0e-4);
    area_mm2(original) + area_mm2(result) - 2.0 * area_mm2(&both)
}

/// The area of the design's box, mm²: its height by its width.
fn box_mm2(pieces: &[Region], height_mm: f64) -> f64 {
    let (mut lo, mut hi) = (f64::INFINITY, f64::NEG_INFINITY);
    for v in pieces
        .iter()
        .flat_map(|r| r.outer().polyline().vertices().iter())
    {
        lo = lo.min(v.x);
        hi = hi.max(v.x);
    }
    height_mm * (hi - lo)
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
    /// Share of the design's box changed: the share that is scored.
    changed: f64,
    /// Share of the ink's own area changed: reported beside it, not
    /// scored (the README says what it would decide).
    changed_of_ink: f64,
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
    let changed_mm2 = changed_mm2(&original.pieces, &smoothed.pieces);
    SmoothRow {
        smooth_pct,
        reduction: 1.0 - after as f64 / before as f64,
        furthest_mm: furthest_vertex_mm(&original.pieces, &smoothed.pieces),
        eps_mm: smooth_pct / 100.0 * height_mm,
        changed: changed_mm2 / box_mm2(&original.pieces, height_mm),
        changed_of_ink: changed_mm2 / area_mm2(&original.pieces),
    }
}

/// A fixture prepared at both defaults together, as `image-prep` runs
/// them, against itself with both steps off.
#[derive(Debug, Clone)]
struct Together {
    prepared: Prepared,
    /// Share of the vertices removed, 0 to 1.
    reduction: f64,
    /// Share of the design's box changed.
    changed: f64,
    /// Share of the ink's own area changed.
    changed_of_ink: f64,
}

#[allow(clippy::cast_precision_loss, reason = "counts far below 2^52")]
fn together(svg: &str) -> Together {
    let original = run(svg, 0.0, 0.0);
    let prepared = run(svg, DEFAULT_MIN_DETAIL_PCT, DEFAULT_SMOOTH_PCT);
    let changed_mm2 = changed_mm2(&original.pieces, &prepared.pieces);
    Together {
        reduction: 1.0
            - vertex_count(&prepared.pieces) as f64 / vertex_count(&original.pieces) as f64,
        changed: changed_mm2 / box_mm2(&original.pieces, original.report.design_height_mm),
        changed_of_ink: changed_mm2 / area_mm2(&original.pieces),
        prepared,
    }
}

#[test]
fn fixtures_keep_their_intent_at_the_defaults() {
    const {
        assert!(
            DEFAULT_SMOOTH_PCT <= DEFAULT_MIN_DETAIL_PCT / 2.0,
            "smoothing never fights the speck threshold"
        );
    }

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

    // Both defaults together, as `image-prep` runs them (the sweep takes
    // each with the other off). The serifs and the counter are in the
    // letters, not marks: the D's counter survives, every mark is still
    // decided by the threshold alone with the smoothing on, and what
    // changes stays within the allowed area.
    let specks = together(SPECKLED);
    let large_holes = specks
        .prepared
        .pieces
        .iter()
        .flat_map(|r| r.holes())
        .filter(|h| h.signed_area_mm2().abs() > 1.0)
        .count();
    assert_eq!(large_holes, 1, "the D's counter");
    assert_eq!(
        contract_breaks(&marks(SPECKLED), &specks.prepared, DEFAULT_MIN_DETAIL_PCT),
        0,
        "smoothing moved a mark across the threshold"
    );
    assert!(specks.changed <= MAX_CHANGED_AREA, "{}", specks.changed);

    // jagged-dc.svg: the jitter goes, the letters stay.
    let smooth = smooth_row(DEFAULT_SMOOTH_PCT);
    assert!(smooth.reduction >= MIN_REDUCTION, "{smooth:?}");
    assert!(
        smooth.furthest_mm <= smooth.eps_mm + SLACK_MM,
        "a vertex moved more than eps: {smooth:?}"
    );
    assert!(smooth.changed <= MAX_CHANGED_AREA, "{smooth:?}");

    // And with the speck threshold on beside it: the two letters and the
    // counter, nothing dropped or filled, the same reduction.
    let jitter = together(JAGGED);
    let report = &jitter.prepared.report;
    assert_eq!(
        (
            report.pieces,
            report.holes,
            report.dropped_pieces,
            report.filled_holes
        ),
        (2, 1, 0, 0),
        "{report:?}"
    );
    assert!(jitter.reduction >= MIN_REDUCTION, "{}", jitter.reduction);
    assert!(jitter.changed <= MAX_CHANGED_AREA, "{}", jitter.changed);
}

/// The defaults are what the sweep picks: the smallest grid value that
/// passes, `smooth` also at most half of `min_detail`.
#[test]
fn defaults_are_the_smallest_grid_values_that_pass() {
    let detail = MIN_DETAIL_GRID_PCT
        .into_iter()
        .find(|&t| detail_row(t).passes())
        .expect("some min_detail passes");
    assert!(
        (detail - DEFAULT_MIN_DETAIL_PCT).abs() < 1.0e-12,
        "{detail}"
    );
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
    println!(
        "| smooth % | vertices removed | furthest vertex / eps | changed, of the box | of the ink | pass |"
    );
    println!("| --- | --- | --- | --- | --- | --- |");
    for s in SMOOTH_GRID_PCT {
        let r = smooth_row(s);
        println!(
            "| {s} | {:.1} % | {:.3} | {:.4} % | {:.4} % | {} |",
            r.reduction * 100.0,
            if r.eps_mm > 0.0 {
                r.furthest_mm / r.eps_mm
            } else {
                0.0
            },
            r.changed * 100.0,
            r.changed_of_ink * 100.0,
            if r.passes() { "yes" } else { "no" }
        );
    }
    println!();
    println!(
        "| both defaults | pieces | holes | dropped | filled | vertices removed | changed, of the box | of the ink |"
    );
    println!("| --- | --- | --- | --- | --- | --- | --- | --- |");
    for (name, svg) in [("speckled-dc.svg", SPECKLED), ("jagged-dc.svg", JAGGED)] {
        let t = together(svg);
        let r = &t.prepared.report;
        println!(
            "| {name} | {} | {} | {} | {} | {:.1} % | {:.4} % | {:.4} % |",
            r.pieces,
            r.holes,
            r.dropped_pieces,
            r.filled_holes,
            t.reduction * 100.0,
            t.changed * 100.0,
            t.changed_of_ink * 100.0
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
