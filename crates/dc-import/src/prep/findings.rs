// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/import/classifier.ts

//! The import checks (functional 49a; design, "Import checks"): one
//! measure over the walk's painted paths each, one warning per kind found
//! with a count and the first example. Nothing is changed or refused.
//!
//! PureCutCNC's classifier assigns roles for its GUI; only its ambiguity
//! detection is reused, as findings: outlines that cross are an overlap, a
//! filled path inside a filled path is a separate-path hole. The nesting
//! itself stays [`dc_geometry::nest`]'s.

use super::DEFAULT_MIN_DETAIL_PCT;
use super::stroke::{self, Budget};
use super::walk::{HiddenReason, PaintedPath, walk};
use crate::svg::{self, SvgImportOptions};
use crate::{ImportError, ImportWarning};
use dc_geometry::{
    EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Rect, Region, area_mm2, intersection, merge_fills,
    resolve_fill, union,
    width::{hole_is_narrower_than, is_narrower_than},
};
use std::collections::BTreeMap;

/// The 49a checks on the SVG in `bytes`, in a fixed order: overlap, hole as
/// a separate path, stroke only, hidden, not convertible (text, images,
/// clip paths, masks), below size. Flattens at the importer's tolerance;
/// costs one more parse and flatten of the SVG `init` already read.
///
/// # Errors
///
/// [`ImportError`] when the SVG cannot be read, or `options` are refused,
/// as for the importer.
pub fn findings(
    bytes: &[u8],
    options: &SvgImportOptions,
) -> Result<Vec<ImportWarning>, ImportError> {
    let tolerance_mm = options.flatten_tolerance_mm;
    if !(tolerance_mm.is_finite() && tolerance_mm >= svg::MIN_FLATTEN_TOLERANCE_MM) {
        return Err(ImportError::InvalidTolerance { tolerance_mm });
    }
    let walked = walk(bytes)?;

    let mut budget = Budget::new(0, options.max_vertices);
    let mut shapes: Vec<Shape> = Vec::new();
    for path in &walked.painted {
        budget.spend(svg::estimated_vertices(&path.geometry.bez, tolerance_mm))?;
        let filled = path.fill.map_or_else(Vec::new, |fill| {
            let profiles = svg::build_profiles(&path.geometry, tolerance_mm);
            resolve_fill(&profiles, fill.rule, tolerance_mm)
        });
        // The checks only report; a failed stroke is `image-prep`'s warning.
        let (stroked, _) = stroke::regions(path, tolerance_mm, &mut budget)?;
        shapes.push(Shape {
            name: path.name.clone(),
            filled,
            stroked,
        });
    }

    let mut out = Vec::new();
    let named: Vec<(&str, Vec<&Region>)> = shapes
        .iter()
        .map(|s| (s.name.as_str(), s.filled.iter().chain(&s.stroked).collect()))
        .collect();
    out.extend(overlap(&named, tolerance_mm));
    out.extend(separate_holes(&shapes, tolerance_mm));
    out.extend(stroke_only(&walked.painted, &shapes));
    out.extend(hidden(&walked.hidden));
    out.extend(
        walked
            .not_convertible
            .iter()
            .map(|u| ImportWarning::SvgObjectNotConvertible {
                kind: u.kind,
                count: u.count,
                group: u.group.clone(),
            }),
    );

    // The ink: fills nested as `init` reads them, strokes united on top.
    let height_mm = painted_height_mm(&walked.painted);
    if height_mm.is_finite() && height_mm > 0.0 {
        let fills: Vec<Vec<Region>> = shapes.iter().map(|s| s.filled.clone()).collect();
        let strokes: Vec<Region> = shapes.iter().flat_map(|s| s.stroked.clone()).collect();
        if let Ok(nested) = merge_fills(fills, tolerance_mm) {
            let ink = union(&nested, &strokes, tolerance_mm);
            out.extend(below_size(&ink, height_mm, tolerance_mm));
        }
    }
    Ok(out)
}

/// The two checks that make sense on a project's shapes, for `check`:
/// `shapes_overlap` and `feature_below_size`. Regions are named
/// `region-N` (their position, from 1); `height_mm` is the design's, from
/// the regions' joint bounding box. See [`design_findings_named`].
#[must_use]
pub fn design_findings(regions: &[Region], height_mm: f64) -> Vec<ImportWarning> {
    let shapes: Vec<(String, Vec<Region>)> = regions
        .iter()
        .enumerate()
        .map(|(i, r)| (format!("region-{}", i + 1), vec![r.clone()]))
        .collect();
    design_findings_named(&shapes, height_mm)
}

/// [`design_findings`] with the shapes' own names: one `(name, regions)`
/// per shape, in order, so `shapes_overlap` can name the pair as the
/// project does. Pieces and holes are measured as given (a project's
/// nesting decides its holes); flattening is at the default tolerance.
#[must_use]
pub fn design_findings_named(
    shapes: &[(String, Vec<Region>)],
    height_mm: f64,
) -> Vec<ImportWarning> {
    let tolerance_mm = FLATTEN_TOLERANCE_DEFAULT_MM;
    let named: Vec<(&str, Vec<&Region>)> = shapes
        .iter()
        .map(|(name, regions)| (name.as_str(), regions.iter().collect()))
        .collect();
    let mut out: Vec<ImportWarning> = overlap(&named, tolerance_mm).into_iter().collect();
    let all: Vec<Region> = shapes
        .iter()
        .flat_map(|(_, regions)| regions.clone())
        .collect();
    out.extend(below_size(&all, height_mm, tolerance_mm));
    out
}

/// One painted path's areas.
pub(super) struct Shape {
    pub(super) name: String,
    /// The fill, resolved under the path's rule; empty without a visible
    /// fill (none, or on a path that encloses nothing).
    pub(super) filled: Vec<Region>,
    /// The stroke's area, resolved nonzero.
    pub(super) stroked: Vec<Region>,
}

/// Every pair `(i, j)`, `i < j`, of `boxes` that overlap or touch, by a
/// sweep over their left edges.
fn box_pairs(boxes: &[Option<Rect>]) -> Vec<(usize, usize)> {
    let mut order: Vec<(usize, Rect)> = boxes
        .iter()
        .enumerate()
        .filter_map(|(i, b)| b.map(|b| (i, b)))
        .collect();
    order.sort_by(|(i, a), (j, b)| a.min.x.total_cmp(&b.min.x).then(i.cmp(j)));
    let mut pairs = Vec::new();
    for (k, &(i, a)) in order.iter().enumerate() {
        for &(j, b) in order.iter().skip(k + 1) {
            if b.min.x > a.max.x {
                break;
            }
            if b.min.y <= a.max.y && a.min.y <= b.max.y {
                pairs.push((i.min(j), i.max(j)));
            }
        }
    }
    pairs.sort_unstable();
    pairs
}

pub(super) fn bounds(region: &Region) -> Option<Rect> {
    region.outer().polyline().bounds()
}

fn perimeter_mm(region: &Region) -> f64 {
    std::iter::once(region.outer())
        .chain(region.holes())
        .map(|c| c.polyline().length_mm())
        .sum()
}

/// How far a region's exact area (its arcs kept) may differ from the area of
/// its flattening, mm²: the chord error times the perimeter. Areas the
/// boolean returns are of flattened shapes, so they are compared with this
/// much room.
fn slack_mm2(region: &Region, tolerance_mm: f64) -> f64 {
    tolerance_mm.max(EPS_MM) * perimeter_mm(region)
}

/// The area `a` and `b` share, mm², and a slack under which it is noise.
fn shared(a: &Region, b: &Region, tolerance_mm: f64) -> (Vec<Region>, f64) {
    let both = intersection(
        std::slice::from_ref(a),
        std::slice::from_ref(b),
        tolerance_mm,
    );
    let mm2 = area_mm2(&both);
    (both, mm2)
}

/// `shapes_overlap`: pairs of shapes with a region of one crossing a
/// region of the other: they share area and neither lies wholly in the
/// other's material (`nest`'s test).
fn overlap(shapes: &[(&str, Vec<&Region>)], tolerance_mm: f64) -> Option<ImportWarning> {
    let items: Vec<(usize, &Region)> = shapes
        .iter()
        .enumerate()
        .flat_map(|(s, (_, regions))| regions.iter().map(move |r| (s, *r)))
        .collect();
    let boxes: Vec<Option<Rect>> = items.iter().map(|(_, r)| bounds(r)).collect();
    // Per pair of shapes, the box of everything where they cross.
    let mut crossings: BTreeMap<(usize, usize), Rect> = BTreeMap::new();
    for (i, j) in box_pairs(&boxes) {
        let (Some(&(si, a)), Some(&(sj, b))) = (items.get(i), items.get(j)) else {
            continue;
        };
        if si == sj {
            continue;
        }
        let (both, shared_mm2) = shared(a, b, tolerance_mm);
        let (slack_a, slack_b) = (slack_mm2(a, tolerance_mm), slack_mm2(b, tolerance_mm));
        let crosses = shared_mm2 > slack_a.min(slack_b)
            && (a.area_mm2() - shared_mm2).abs() > slack_a
            && (b.area_mm2() - shared_mm2).abs() > slack_b;
        if !crosses {
            continue;
        }
        let Some(extent) = both
            .iter()
            .filter_map(bounds)
            .reduce(|acc, b| acc.including(b.min).including(b.max))
        else {
            continue;
        };
        crossings
            .entry((si.min(sj), si.max(sj)))
            .and_modify(|acc| *acc = acc.including(extent.min).including(extent.max))
            .or_insert(extent);
    }
    let (&(first, second), extent) = crossings.iter().next()?;
    Some(ImportWarning::ShapesOverlap {
        count: crossings.len(),
        first: shapes.get(first)?.0.to_owned(),
        second: shapes.get(second)?.0.to_owned(),
        x_mm: f64::midpoint(extent.min.x, extent.max.x),
        y_mm: f64::midpoint(extent.min.y, extent.max.y),
    })
}

/// `svg_hole_as_separate_path`: a path with a visible fill all of whose
/// regions lie wholly inside the material of one other path's fill.
pub(super) fn separate_holes(shapes: &[Shape], tolerance_mm: f64) -> Option<ImportWarning> {
    let items: Vec<(usize, &Region)> = shapes
        .iter()
        .enumerate()
        .flat_map(|(s, shape)| shape.filled.iter().map(move |r| (s, r)))
        .collect();
    let boxes: Vec<Option<Rect>> = items.iter().map(|(_, r)| bounds(r)).collect();
    // For each region, the shapes whose material holds it, with the
    // container's area (the smallest names the outer).
    let mut inside: BTreeMap<usize, Vec<Option<(usize, f64)>>> = BTreeMap::new();
    for (s, shape) in shapes.iter().enumerate() {
        if !shape.filled.is_empty() {
            inside.insert(s, vec![None; shape.filled.len()]);
        }
    }
    let mut local: Vec<usize> = Vec::with_capacity(items.len());
    let mut seen: BTreeMap<usize, usize> = BTreeMap::new();
    for &(s, _) in &items {
        let n = seen.entry(s).or_insert(0);
        local.push(*n);
        *n += 1;
    }
    for (i, j) in box_pairs(&boxes) {
        let (Some(&(si, a)), Some(&(sj, b))) = (items.get(i), items.get(j)) else {
            continue;
        };
        if si == sj {
            continue;
        }
        for (inner, outer, s_in, s_out, k) in
            [(a, b, si, sj, local.get(i)), (b, a, sj, si, local.get(j))]
        {
            let (_, shared_mm2) = shared(inner, outer, tolerance_mm);
            let slack = slack_mm2(inner, tolerance_mm);
            let wholly = (inner.area_mm2() - shared_mm2).abs() <= slack
                && outer.area_mm2() > inner.area_mm2() + slack;
            if !wholly {
                continue;
            }
            if let (Some(slots), Some(&k)) = (inside.get_mut(&s_in), k) {
                let area = outer.area_mm2();
                if let Some(slot) = slots.get_mut(k)
                    && slot.is_none_or(|(_, best)| area < best)
                {
                    *slot = Some((s_out, area));
                }
            }
        }
    }
    let mut count = 0;
    let mut first = None;
    for (s, slots) in &inside {
        // Every region inside one and the same other path.
        let owners: Vec<Option<usize>> = slots.iter().map(|o| o.map(|(owner, _)| owner)).collect();
        let Some(Some(owner)) = owners.first().copied() else {
            continue;
        };
        if owners.iter().all(|o| *o == Some(owner)) {
            count += 1;
            if first.is_none() {
                first = Some((*s, owner));
            }
        }
    }
    let (inner, outer) = first?;
    Some(ImportWarning::SvgHoleAsSeparatePath {
        count,
        inner: shapes.get(inner)?.name.clone(),
        outer: shapes.get(outer)?.name.clone(),
    })
}

/// `svg_stroke_only_path`: painted with a stroke and no visible fill.
pub(super) fn stroke_only(painted: &[PaintedPath], shapes: &[Shape]) -> Option<ImportWarning> {
    let mut found = painted
        .iter()
        .zip(shapes)
        .filter(|(path, shape)| path.stroke.is_some() && shape.filled.is_empty());
    let (first, _) = found.next()?;
    Some(ImportWarning::SvgStrokeOnlyPath {
        count: 1 + found.count(),
        first: first.name.clone(),
        width_mm: first.stroke.as_ref()?.width_mm,
    })
}

/// `svg_hidden_object`: the walk's objects that are not drawn, the first
/// named by the order `display_none`, `hidden`, `zero_opacity`,
/// `unpainted`, then document order.
pub(super) fn hidden(objects: &[super::walk::HiddenObject]) -> Option<ImportWarning> {
    let first = [
        HiddenReason::DisplayNone,
        HiddenReason::Hidden,
        HiddenReason::ZeroOpacity,
        HiddenReason::Unpainted,
    ]
    .into_iter()
    .find_map(|reason| objects.iter().find(|o| o.reason == reason))?;
    Some(ImportWarning::SvgHiddenObject {
        count: objects.iter().map(|o| o.shapes).sum(),
        first: first.name.clone(),
        reason: first.reason,
    })
}

/// The height of the painted paths' joint box, mm, strokes grown by half
/// their width (a bound: a mitre can reach further).
pub(super) fn painted_height_mm(painted: &[PaintedPath]) -> f64 {
    let mut low = f64::INFINITY;
    let mut high = f64::NEG_INFINITY;
    for path in painted {
        let Some(b) = path.bounds_mm else { continue };
        let grow = path.stroke.as_ref().map_or(0.0, |s| s.width_mm / 2.0);
        low = low.min(b.min.y - grow);
        high = high.max(b.max.y + grow);
    }
    high - low
}

/// True for a region or hole too thin to be a feature at all: an average
/// width under ten times [`EPS_MM`], the slivers a union leaves where two
/// outlines nearly coincide. They are numerical noise, not detail, and
/// the width tests are not meant for them.
fn is_noise(area_mm2: f64, perimeter_mm: f64) -> bool {
    area_mm2 < 10.0 * EPS_MM * perimeter_mm
}

/// `feature_below_size`: pieces and holes of `ink` whose widest point is
/// under [`DEFAULT_MIN_DETAIL_PCT`] of `height_mm`.
fn below_size(ink: &[Region], height_mm: f64, tolerance_mm: f64) -> Option<ImportWarning> {
    if !(height_mm.is_finite() && height_mm > 0.0) {
        return None;
    }
    let threshold_mm = DEFAULT_MIN_DETAIL_PCT / 100.0 * height_mm;
    let mut count = 0;
    let mut smallest_mm = threshold_mm;
    let mut narrower = |is_narrower: &dyn Fn(f64) -> bool| {
        if !is_narrower(threshold_mm) {
            return;
        }
        count += 1;
        // Only a feature narrower than the smallest so far can lower it:
        // bisect the widest point below that.
        if is_narrower(smallest_mm) {
            let (mut low, mut high) = (0.0, smallest_mm);
            for _ in 0..16 {
                let mid = f64::midpoint(low, high);
                if is_narrower(mid) {
                    high = mid;
                } else {
                    low = mid;
                }
            }
            smallest_mm = f64::midpoint(low, high);
        }
    };
    for region in ink {
        if is_noise(region.area_mm2(), perimeter_mm(region)) {
            continue;
        }
        narrower(&|w| is_narrower_than(region, w, tolerance_mm));
        for hole in region.holes() {
            let outline = hole.polyline();
            if is_noise(hole.signed_area_mm2().abs(), outline.length_mm()) {
                continue;
            }
            narrower(&|w| hole_is_narrower_than(hole, w, tolerance_mm));
        }
    }
    (count > 0).then_some(ImportWarning::FeatureBelowSize {
        count,
        smallest_mm,
        threshold_mm,
        height_mm,
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing,
        reason = "tests"
    )]

    use super::*;
    use crate::{HiddenReason, NotConvertible};
    use dc_geometry::{FillRule, Point, Polyline, resolve_fill};

    const MESSY: &[u8] = include_bytes!("../../../../tests/fixtures/prep/messy.svg");
    const CLEAN: &[u8] = include_bytes!("../../../../tests/fixtures/prep/clean.svg");
    const FIX: &str = "; `dreamcarve image-prep` would fix this";

    fn check(bytes: &[u8]) -> Vec<ImportWarning> {
        findings(bytes, &SvgImportOptions::new()).expect("findings")
    }

    fn drawing(body: &str) -> Vec<u8> {
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 100 100">{body}</svg>"#
        )
        .into_bytes()
    }

    fn codes(warnings: &[ImportWarning]) -> Vec<&'static str> {
        warnings.iter().map(ImportWarning::code).collect()
    }

    #[test]
    fn overlap_names_the_pair_and_where() {
        let found = check(MESSY);
        let overlap = found
            .iter()
            .find_map(|w| match w {
                ImportWarning::ShapesOverlap {
                    count,
                    first,
                    second,
                    x_mm,
                    y_mm,
                } => Some((*count, first.as_str(), second.as_str(), *x_mm, *y_mm)),
                _ => None,
            })
            .expect("shapes_overlap");
        assert_eq!((overlap.0, overlap.1, overlap.2), (1, "sq-a", "sq-b"));
        // The squares meet at x 16..20; SVG y 0..10 is Y-up 90..100.
        assert!((overlap.3 - 18.0).abs() < 0.01, "x {}", overlap.3);
        assert!((overlap.4 - 95.0).abs() < 0.01, "y {}", overlap.4);
    }

    #[test]
    fn separate_hole_needs_both_paths_filled() {
        let found = check(MESSY);
        assert!(
            found.contains(&ImportWarning::SvgHoleAsSeparatePath {
                count: 1,
                inner: "inner".into(),
                outer: "outer".into(),
            }),
            "{found:?}"
        );
        // The inner path is only stroked: no visible fill, so no hole.
        let stroked = check(&drawing(
            r#"<rect id="o" x="10" y="10" width="40" height="40"/>
               <rect id="i" x="20" y="20" width="10" height="10" fill="none" stroke="black"/>"#,
        ));
        assert!(
            !codes(&stroked).contains(&"svg_hole_as_separate_path"),
            "{stroked:?}"
        );
    }

    #[test]
    fn an_unfilled_frame_is_not_reported() {
        let found = check(&drawing(
            r#"<rect id="frame" x="5" y="5" width="90" height="90" fill="none" stroke="black"/>
               <rect id="shape" x="20" y="20" width="30" height="30"/>"#,
        ));
        assert!(
            !codes(&found).contains(&"svg_hole_as_separate_path"),
            "{found:?}"
        );
    }

    #[test]
    fn stroke_only_gives_the_width_in_mm() {
        let found = check(MESSY);
        assert!(
            found.iter().any(
                |w| matches!(w, ImportWarning::SvgStrokeOnlyPath { count: 1, first, width_mm }
                if first == "centreline" && (width_mm - 1.2).abs() < 1e-3)
            ),
            "{found:?}"
        );
        // 50 user units over 100 mm: the stroke is twice as wide in mm.
        let scaled = findings(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="100mm" height="100mm" viewBox="0 0 50 50"><path id="p" d="M5,5 L40,40" fill="none" stroke="black" stroke-width="1"/></svg>"#,
            &SvgImportOptions::new(),
        )
        .unwrap();
        assert!(
            scaled.iter().any(
                |w| matches!(w, ImportWarning::SvgStrokeOnlyPath { width_mm, .. }
                if (width_mm - 2.0).abs() < 1e-3)
            ),
            "{scaled:?}"
        );
    }

    #[test]
    fn hidden_object_names_the_layer() {
        let found = check(MESSY);
        assert!(
            found.contains(&ImportWarning::SvgHiddenObject {
                count: 1,
                first: "old-layer".into(),
                reason: HiddenReason::DisplayNone,
            }),
            "{found:?}"
        );
    }

    #[test]
    fn not_convertible_names_the_fix() {
        let found = check(MESSY);
        let kinds: Vec<_> = found
            .iter()
            .filter_map(|w| match w {
                ImportWarning::SvgObjectNotConvertible { kind, count, group } => {
                    Some((*kind, *count, group.as_str()))
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            [
                (NotConvertible::Text, 1, ""),
                (NotConvertible::Image, 1, ""),
                (NotConvertible::ClipPath, 1, "clipped"),
            ]
        );
        let text: Vec<String> = found.iter().map(ToString::to_string).collect();
        assert!(
            text.iter()
                .any(|m| m.contains("convert text to paths in your drawing program"))
        );
        assert!(
            text.iter()
                .any(|m| m.contains("trace images in Inkscape first"))
        );
        assert!(text.iter().any(|m| m.contains("expand clips and masks")));
    }

    #[test]
    fn feature_below_size_counts_and_names_the_smallest() {
        let found = check(MESSY);
        let (count, smallest, threshold, height) = found
            .iter()
            .find_map(|w| match w {
                ImportWarning::FeatureBelowSize {
                    count,
                    smallest_mm,
                    threshold_mm,
                    height_mm,
                } => Some((*count, *smallest_mm, *threshold_mm, *height_mm)),
                _ => None,
            })
            .expect("feature_below_size");
        assert_eq!(count, 1);
        // The 0.1 mm dot; flattened at 0.01 mm its widest point is a little
        // under its diameter.
        assert!((0.07..=0.101).contains(&smallest), "smallest {smallest}");
        assert!((threshold - 0.2).abs() < 1e-3, "threshold {threshold}");
        assert!((height - 100.0).abs() < 1e-3, "height {height}");
    }

    /// A stroke whose outline would pass the vertex cap is refused before
    /// the outline is built.
    #[test]
    fn a_huge_dashed_stroke_is_refused_not_built() {
        let bytes = drawing(
            r#"<path d="M0,0 L50000,0" fill="none" stroke="black" stroke-width="50" stroke-dasharray="0.001"/>"#,
        );
        let result = findings(&bytes, &SvgImportOptions::new());
        assert!(
            matches!(result, Err(ImportError::TooManyVertices { .. })),
            "{result:?}"
        );
    }

    /// The sign's stroked outlines union into slivers a few nanometres
    /// wide; the width tests must not be given them.
    #[test]
    fn numerical_slivers_are_not_measured() {
        let sign = include_bytes!("../../../../tests/fixtures/sign.svg");
        let found = check(sign);
        assert!(!codes(&found).contains(&"feature_below_size"), "{found:?}");
        check(include_bytes!("../../../../tests/fixtures/dc-inlay.svg"));
    }

    #[test]
    fn a_clean_drawing_has_none() {
        assert_eq!(check(CLEAN), []);
    }

    #[test]
    fn codes_are_stable() {
        assert_eq!(
            codes(&check(MESSY)),
            [
                "shapes_overlap",
                "svg_hole_as_separate_path",
                "svg_stroke_only_path",
                "svg_hidden_object",
                "svg_object_not_convertible",
                "svg_object_not_convertible",
                "svg_object_not_convertible",
                "feature_below_size",
            ]
        );
    }

    #[test]
    fn messages_that_image_prep_fixes_name_it() {
        for warning in check(MESSY) {
            let message = warning.to_string();
            let fixed = !matches!(warning, ImportWarning::SvgObjectNotConvertible { .. });
            assert_eq!(message.ends_with(FIX), fixed, "{message}");
        }
    }

    fn rect_regions(x: f64, y: f64, w: f64, h: f64) -> Vec<Region> {
        resolve_fill(
            &[Polyline::rect(Point::new(x, y), w, h)],
            FillRule::NonZero,
            0.01,
        )
    }

    #[test]
    fn design_findings_read_a_projects_regions() {
        let shapes = vec![
            ("a".to_owned(), rect_regions(0.0, 0.0, 10.0, 10.0)),
            ("b".to_owned(), rect_regions(6.0, 0.0, 10.0, 10.0)),
            ("far".to_owned(), rect_regions(50.0, 90.0, 10.0, 10.0)),
            ("speck".to_owned(), rect_regions(80.0, 50.0, 0.1, 0.1)),
        ];
        let found = design_findings_named(&shapes, 100.0);
        assert_eq!(codes(&found), ["shapes_overlap", "feature_below_size"]);
        assert!(
            matches!(&found[0], ImportWarning::ShapesOverlap { count: 1, first, second, .. }
            if first == "a" && second == "b")
        );
        assert!(
            matches!(&found[1], ImportWarning::FeatureBelowSize { count: 1, height_mm, .. }
            if (*height_mm - 100.0).abs() < 1e-9)
        );
        // Unnamed regions are named by position.
        let flat: Vec<Region> = shapes.into_iter().flat_map(|(_, r)| r).collect();
        let found = design_findings(&flat, 100.0);
        assert!(
            matches!(&found[0], ImportWarning::ShapesOverlap { first, second, .. }
            if first == "region-1" && second == "region-2")
        );
        // No height, no threshold: nothing is measured against it.
        assert_eq!(codes(&design_findings(&flat, 0.0)), ["shapes_overlap"]);
    }
}
