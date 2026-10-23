// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #362: a Profile's lap rises over every tab its bit reaches, not only
//! its own outline's (`docs/specs/phase-2/design.md`, "Profile tabs").
//! Where a band between two outlines of one Profile is narrower than the
//! bit (a hole round an island, the overlap `ToolpathsOverlap` warns of),
//! one outline's lap runs within the radius of the other's tab and would
//! cut it away below its top. The tab is the material the other outline's
//! last pass leaves standing; the lap's raised stretches over it are
//! found as an allowance path's are from its own last pass
//! ([`last_pass::twin_tabs`]): every stretch whose cutter reaches that
//! material, at the tab's top and shape, merged with the lap's own.

use super::last_pass;
use crate::tabs::PathTabs;
use dc_geometry::{Polyline, Rect};

/// `layout`, the raised stretches of the lap `path`, raised also over
/// every tab of `sources` (other outlines' last-pass paths, each with its
/// own tabs) that a cutter of `radius_mm` on `path` reaches, the tabs
/// standing from `floor_z_mm`. `None` when it reaches none, so the lap
/// is cut as before.
pub(super) fn over_reached_tabs<'a>(
    path: &Polyline,
    layout: &PathTabs,
    sources: impl IntoIterator<Item = (&'a Polyline, &'a PathTabs)>,
    radius_mm: f64,
    floor_z_mm: f64,
) -> Option<PathTabs> {
    let bounds = path.bounds()?;
    // A tab stands within the radius of its last pass, and the cutter
    // reaches a radius, plus the lattice's step and slack, at most
    // [`REACH_SLACK_MM`].
    let reach_mm = 2.0 * radius_mm + REACH_SLACK_MM;
    let mut raised: Option<PathTabs> = None;
    for (source, tabs) in sources {
        if tabs.is_untabbed() || !within(bounds, source.bounds(), reach_mm) {
            continue;
        }
        let reached = last_pass::twin_tabs(source, tabs, path, radius_mm, floor_z_mm);
        if reached.is_untabbed() {
            continue;
        }
        let base = raised.as_ref().unwrap_or(layout);
        raised = Some(base.merged(&reached));
    }
    raised
}

/// More than [`last_pass::twin_tabs`] reaches past twice the radius, mm:
/// twice its largest lattice step.
const REACH_SLACK_MM: f64 = 0.5;

/// Whether `a` and `b` come within `reach_mm` of each other.
fn within(a: Rect, b: Option<Rect>, reach_mm: f64) -> bool {
    b.is_some_and(|b| {
        a.min.x - reach_mm <= b.max.x
            && b.min.x - reach_mm <= a.max.x
            && a.min.y - reach_mm <= b.max.y
            && b.min.y - reach_mm <= a.max.y
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tabs::{TabCut, Tabs};
    use dc_geometry::{Point, Vertex};
    use dc_project::TabShape;

    /// A closed axis-aligned rectangle from `(x0, y0)` to `(x1, y1)`,
    /// counter-clockwise from its lower left.
    fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Polyline {
        Polyline::new(
            vec![
                Vertex::line(x0, y0),
                Vertex::line(x1, y0),
                Vertex::line(x1, y1),
                Vertex::line(x0, y1),
            ],
            true,
        )
    }

    const R: f64 = 3.0;
    const TOP: f64 = 2.0;

    /// The lap round a 20 × 20 mm island cut Outside by a 6 mm bit, with
    /// one 2 mm rect tab centred on its right side, 2 mm tall.
    fn island_lap() -> (Polyline, PathTabs) {
        let lap = rect(-R, -R, 20.0 + R, 20.0 + R);
        // Along the lap from (-R, -R): the bottom, then up the right side
        // to its middle.
        let centre_mm = (20.0 + 2.0 * R) + (10.0 + R);
        let tabs = Tabs::new(TOP, 2.0 * R, 0.01);
        let cut = TabCut {
            centre_mm,
            width_mm: 2.0,
            top_z_mm: TOP,
            shape: TabShape::Rect,
        };
        let (layout, _) = tabs.layout(&[cut], lap.length_mm(), crate::tabs::PathEnds::Closed);
        (lap, layout)
    }

    /// The distance along `path` of its point nearest `p`.
    fn along(path: &Polyline, p: Point) -> f64 {
        dc_geometry::nearest_point(path, p).map_or(f64::NAN, |n| n.along_mm)
    }

    #[test]
    fn lap_rises_over_another_outlines_reached_tab() {
        let (island, island_tabs) = island_lap();
        // A hole's lap whose left side runs on the island's right edge,
        // the band between them narrower than the bit: it has no tab of
        // its own.
        let hole = rect(20.0, -10.0, 40.0, 30.0);
        let untabbed = PathTabs::from_spans(hole.length_mm(), 0, &[], true);
        let raised = over_reached_tabs(&hole, &untabbed, [(&island, &island_tabs)], R, 0.0)
            .expect("the hole's lap reaches the island's tab");
        // Over the island's tab, at its centre, the hole's lap holds the
        // top; at both ends of the tab's material (1 mm either side) and a
        // radius beyond, where its cutter still reaches it, too.
        for y in [10.0, 9.0, 11.0, 9.0 - R + 0.1, 11.0 + R - 0.1] {
            let d = along(&hole, Point::new(20.0, y));
            assert!(
                (raised.z_at_mm(d, 0.0) - TOP).abs() < 1e-9,
                "the hole's lap at y = {y} runs at {} mm, not over the tab",
                raised.z_at_mm(d, 0.0)
            );
        }
        // Clear of the tab it cuts at its pass Z.
        for y in [0.0, 20.0, 25.0] {
            let d = along(&hole, Point::new(20.0, y));
            assert!(raised.z_at_mm(d, 0.0).abs() < 1e-9, "y = {y} is raised");
        }
        let d = along(&hole, Point::new(30.0, 30.0));
        assert!(
            raised.z_at_mm(d, 0.0).abs() < 1e-9,
            "the far side is raised"
        );
    }

    #[test]
    fn lap_out_of_reach_of_every_other_tab_is_unchanged() {
        let (island, island_tabs) = island_lap();
        // A lap 0.3 mm more than the diameter from the island's, within
        // the bounds' reach: its cutter stops short of the tab's material,
        // which stands within the radius of the island's lap.
        let far = rect(20.0 + 3.0 * R + 0.3, -10.0, 60.0, 30.0);
        let untabbed = PathTabs::from_spans(far.length_mm(), 0, &[], true);
        assert!(over_reached_tabs(&far, &untabbed, [(&island, &island_tabs)], R, 0.0).is_none());
    }

    #[test]
    fn reached_tab_merges_with_the_laps_own() {
        let (island, island_tabs) = island_lap();
        let hole = rect(20.0, -10.0, 40.0, 30.0);
        // The hole's own tab, centred 1 mm above the island's on the side
        // they share, overlaps the reached stretch: one raised stretch.
        let tabs = Tabs::new(TOP, 2.0 * R, 0.01);
        let own = TabCut {
            centre_mm: along(&hole, Point::new(20.0, 11.0)),
            width_mm: 2.0,
            top_z_mm: TOP,
            shape: TabShape::Rect,
        };
        let (layout, _) = tabs.layout(&[own], hole.length_mm(), crate::tabs::PathEnds::Closed);
        let raised = over_reached_tabs(&hole, &layout, [(&island, &island_tabs)], R, 0.0)
            .expect("the hole's lap reaches the island's tab");
        assert_eq!(raised.tab_count(), 1, "the lap keeps its own tab count");
        assert_eq!(raised.gaps_mm().len(), 1, "one raised stretch");
        assert!(raised.covered_mm() > layout.covered_mm());
    }
}
