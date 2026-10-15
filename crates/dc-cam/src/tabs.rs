// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/tabs.ts

//! Holding tabs on Profile passes (`docs/specs/phase-1/design.md`,
//! "Algorithms → Profile contour", step 7).
//!
//! PureCutCNC places tabs as rectangles in the world and grows each by the
//! tool radius before clipping the path against it (`toolCentreContours`,
//! `expandedTabFootprints`). DreamCarve places tabs by distance along the
//! tool-centre contour instead, so that growth becomes a span of
//! `width + diameter` along the path: the cutter's full width must clear
//! the tab. World-placed tabs arrive with the GUI in phase 5.

use crate::error::CamError;
use crate::toolpath::Move;
use crate::warning::Warning;
use dc_geometry::Polyline;
use dc_project::{OperationId, TabPlacement, TabShape, TabSpec};

/// Distances along a closed path of length `path_length_mm` at which tabs
/// are centred, each in `[0, path_length_mm)`, in placement order.
///
/// `Count(n)` spaces `n` tabs `L/n` apart starting at `L/(2n)`, so no tab
/// straddles the path start unless tabs overlap. `AtDistances` takes each
/// value modulo `L`. A non-positive `L` gives no tabs.
#[must_use]
pub fn tab_centres_mm(placement: &TabPlacement, path_length_mm: f64) -> Vec<f64> {
    let _ = (placement, path_length_mm);
    todo!()
}

/// The feed rates a lap is cut at.
#[derive(Debug, Clone, Copy)]
pub(crate) struct LapRates {
    /// XY cutting feed, mm/min.
    pub feed_mm_min: f64,
    /// Descent feed, mm/min; also used for tab lifts.
    pub plunge_mm_min: f64,
}

/// One operation's tab settings, checked.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Tabs {
    placement: TabPlacement,
    shape: TabShape,
    top_z_mm: f64,
    span_mm: f64,
    chord_tolerance_mm: f64,
}

impl Tabs {
    /// Checks `spec` for operation `op` cut with a tool `diameter_mm` wide.
    /// `chord_tolerance_mm` bounds how far a smooth ramp's chords stray from
    /// the raised cosine in Z.
    pub(crate) fn new(
        op: &OperationId,
        spec: &TabSpec,
        diameter_mm: f64,
        chord_tolerance_mm: f64,
    ) -> Result<Self, CamError> {
        let _ = (op, spec, diameter_mm, chord_tolerance_mm);
        todo!()
    }

    /// Tab top, mm above the stock bottom.
    pub(crate) fn top_z_mm(&self) -> f64 {
        self.top_z_mm
    }

    /// The raised spans on a closed path `path_length_mm` long, and whether
    /// any two of them overlapped.
    pub(crate) fn layout(&self, path_length_mm: f64) -> (PathTabs, bool) {
        let _ = path_length_mm;
        todo!()
    }
}

/// The merged tab spans on one closed tool-centre path.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PathTabs {
    _private: (),
}

/// The warnings about where the tab top lies relative to the cut
/// (PureCutCNC `applyTabWarnings`): [`crate::WarningCode::TabsOutsideCutRange`]
/// when no pass of `pass_z_mm` lies below the tab top, or when the tab top is
/// above the stock top `thickness_mm`.
pub(crate) fn cut_range_warnings(
    op: &OperationId,
    tabs: &Tabs,
    pass_z_mm: &[f64],
    thickness_mm: f64,
) -> Vec<Warning> {
    let _ = (op, tabs, pass_z_mm, thickness_mm);
    todo!()
}

/// Appends one lap of `path` at `z_mm`, starting with the descent from the
/// tool's current position (vertically above the path start) and ending at
/// the path start. Where `tabs` is given and `z_mm` lies below the tab top,
/// the lap rises over each tab span.
pub(crate) fn emit_lap(
    moves: &mut Vec<Move>,
    path: &Polyline,
    z_mm: f64,
    tabs: Option<(&Tabs, &PathTabs)>,
    rates: LapRates,
) {
    let _ = (moves, path, z_mm, tabs, rates);
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::generate_profile;
    use crate::tab_profile::{
        smooth_tab_height_fraction, smooth_tab_sample_count, smooth_tab_sample_fractions,
        smooth_tab_z_mm,
    };
    use crate::test_support::assert_safe;
    use crate::toolpath::{MoveKind, Toolpath};
    use crate::warning::WarningCode;
    use dc_geometry::{EPS_MM, Point, Point3, Transform, assert_approx_eq};
    use dc_project::test_support::sample_project;
    use dc_project::{
        CutDirection, DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, Operation,
        ProfileOp, Project, Side, Stock, ToolId,
    };
    use std::collections::BTreeMap;
    use std::f64::consts::TAU;

    const TOL: f64 = 1e-6;
    const THICKNESS_MM: f64 = 12.0;
    const DIAMETER_MM: f64 = 6.35;
    const TAB_WIDTH_MM: f64 = 8.0;
    const TAB_Z_MM: f64 = 3.0;
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
        let mut p = sample_project();
        p.stock = Stock {
            width_mm: 120.0,
            height_mm: 120.0,
            thickness_mm: THICKNESS_MM,
        };
        let def = FeatureDefinition {
            id: DefinitionId::new("d1"),
            name: "Coaster".into(),
            profiles: vec![Polyline::circle(Point::new(0.0, 0.0), radius_mm)],
        };
        p.definitions = BTreeMap::from([(def.id.clone(), def)]);
        p.instances = vec![FeatureInstance {
            id: InstanceId::new("i1"),
            definition: DefinitionId::new("d1"),
            transform: Transform::translate(60.0, 60.0),
            name: "Coaster 1".into(),
            visible: true,
            locked: false,
            z_top_mm: THICKNESS_MM,
            z_bottom_mm: 0.0,
        }];
        let op = ProfileOp {
            id: OperationId::new("o1"),
            name: "Cut out".into(),
            enabled: true,
            targets: vec![InstanceId::new("i1")],
            tool: ToolId::new("t1"),
            side,
            depth_mm: THICKNESS_MM,
            step_down_mm: Some(4.0),
            direction: CutDirection::Climb,
            through_cut_allowance_mm: 0.0,
            safe_z_mm: 5.0,
            tabs,
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
        assert_safe(&toolpath, project, op);
        (toolpath, warnings)
    }

    fn codes(warnings: &[Warning]) -> Vec<WarningCode> {
        warnings.iter().map(|w| w.code).collect()
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

        // Four tabs all but cover it (port of "four tabs cover a circle"),
        // yet leave a gap to cut through, so nothing overlaps.
        let (p, op) = small(4);
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let lap = final_lap(&tp);
        let free: f64 = runs(&lap, |m| m.at_z(0.0))
            .iter()
            .map(|r| r.length_mm)
            .sum();
        assert_approx_eq!(free, path_mm - 4.0 * span_mm, TOL);
        assert!(free / path_mm < 0.05 && free > 0.0, "free {free}");

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

    #[test]
    fn final_pass_has_exactly_four_raised_segments_at_tab_z() {
        let (p, op) = coaster(Some(four_rect_tabs()));
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
        assert_eq!(warnings[0].param("op"), Some("o1"));
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
        assert_approx_eq!(lifted[0].length_mm, 8.0 + TAB_WIDTH_MM + DIAMETER_MM, TOL);
        let held: f64 = raised_runs(&final_lap(&tp))
            .iter()
            .map(|r| r.length_mm)
            .sum();
        assert_approx_eq!(held, 8.0, TOL);

        // Tabs wide enough to cover the whole path merge into one: the
        // final pass rides the tab top all the way round.
        let mut spec = four_rect_tabs();
        spec.width_mm = 80.0;
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOverlap]);
        let lap = final_lap(&tp);
        assert!(lap.iter().all(|m| m.at_z(TAB_Z_MM)), "{lap:?}");

        // Spaced-out tabs do not warn.
        let (p, op) = coaster(Some(four_rect_tabs()));
        let (_, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
    }

    #[test]
    fn smooth_profile_is_raised_cosine() {
        // Exact endpoints and peak (port: testExactEndpointsAndPeak).
        assert_approx_eq!(smooth_tab_height_fraction(0.0), 0.0, 0.0);
        assert_approx_eq!(smooth_tab_height_fraction(1.0), 0.0, 0.0);
        assert_approx_eq!(smooth_tab_height_fraction(0.5), 1.0, 0.0);
        assert_approx_eq!(smooth_tab_z_mm(0.0, -5.0, 3.0), -5.0, 0.0);
        assert_approx_eq!(smooth_tab_z_mm(1.0, -5.0, 3.0), -5.0, 0.0);
        assert_approx_eq!(smooth_tab_z_mm(0.5, -5.0, 3.0), 3.0, 0.0);

        // Zero-slope joins (port: testZeroSlopeJoins).
        let h = 1e-6;
        let slope = |u: f64| {
            (smooth_tab_height_fraction(u + h) - smooth_tab_height_fraction(u - h)) / (2.0 * h)
        };
        assert_approx_eq!(slope(h), 0.0, 1e-4);
        assert_approx_eq!(slope(1.0 - h), 0.0, 1e-4);
        assert_approx_eq!(slope(0.5), 0.0, 1e-4);
        assert!(slope(0.25) > 1.0);
        assert!(slope(0.75) < -1.0);

        // Monotonic halves, bounded (port: testMonotonicAndBounded).
        let steps = 2000_u32;
        let at = |i: u32| smooth_tab_height_fraction(f64::from(i) / f64::from(steps));
        for i in 1..=steps / 2 {
            assert!(at(i) >= at(i - 1) - 1e-12, "rising at {i}");
        }
        for i in steps / 2 + 1..=steps {
            assert!(at(i) <= at(i - 1) + 1e-12, "falling at {i}");
        }
        assert!((0..=steps).all(|i| (0.0..=1.0).contains(&at(i))));

        // Never below the cut Z, never above the top (port:
        // testNeverBelowCutZ).
        for i in 0..=500_u32 {
            let z = smooth_tab_z_mm(f64::from(i) / 500.0, -4.25, -1.0);
            assert!((-4.25 - 1e-12..=-1.0 + 1e-12).contains(&z), "{z}");
        }

        // A top at or below the base never descends (port:
        // testMalformedRanges).
        assert_approx_eq!(smooth_tab_z_mm(0.5, -2.0, -2.0), -2.0, 0.0);
        assert_approx_eq!(smooth_tab_z_mm(0.5, -2.0, -6.0), -2.0, 0.0);
        assert_approx_eq!(smooth_tab_z_mm(0.25, -2.0, -6.0), -2.0, 0.0);

        // The sample count honours the chord tolerance, measured (port:
        // testSampleCountMeetsTolerance).
        for (rise, tol) in [
            (3.0, 0.01),
            (0.5, 0.01),
            (12.0, 0.01),
            (3.0, 0.001),
            (0.12, 0.0004),
        ] {
            let n = smooth_tab_sample_count(rise, tol);
            assert_eq!(n % 2, 0, "even for rise {rise} tol {tol}");
            let nf = f64::from(n);
            let mut worst: f64 = 0.0;
            for i in 0..n {
                let (u0, u1) = (f64::from(i) / nf, f64::from(i + 1) / nf);
                let (z0, z1) = (
                    smooth_tab_z_mm(u0, 0.0, rise),
                    smooth_tab_z_mm(u1, 0.0, rise),
                );
                for step in 1..32_u32 {
                    let t = f64::from(step) / 32.0;
                    let chord = z0 + (z1 - z0) * t;
                    let curve = smooth_tab_z_mm(u0 + (u1 - u0) * t, 0.0, rise);
                    worst = worst.max((curve - chord).abs());
                }
            }
            assert!(
                worst <= tol,
                "chord error {worst} > {tol} for rise {rise} (n {n})"
            );
        }

        // Count bounds (port: testSampleCountBounds).
        assert!(smooth_tab_sample_count(12.0, 0.01) > smooth_tab_sample_count(3.0, 0.01));
        assert!(smooth_tab_sample_count(3.0, 0.001) > smooth_tab_sample_count(3.0, 0.01));
        assert_eq!(smooth_tab_sample_count(0.0, 0.01), 4);
        assert_eq!(smooth_tab_sample_count(3.0, 0.0), 4);
        assert_eq!(smooth_tab_sample_count(1e9, 1e-9), 512);

        // Fractions (port: testSampleFractions).
        let fractions = smooth_tab_sample_fractions(3.0, 0.01);
        assert_eq!(
            fractions.len(),
            smooth_tab_sample_count(3.0, 0.01) as usize + 1
        );
        assert_approx_eq!(fractions[0], 0.0, 0.0);
        assert_approx_eq!(*fractions.last().unwrap(), 1.0, 0.0);
        assert!(fractions.iter().any(|f| (f - 0.5).abs() <= 0.0));
        assert!(fractions.windows(2).all(|w| w[1] > w[0]));

        // On the coaster: four ramps, each rising from the pass Z to the
        // tab top at its centre and back, over width + diameter, with no
        // vertical step.
        let (p, op) = coaster(Some(tab_spec(
            TabPlacement::Count { count: 4 },
            TabShape::Smooth,
        )));
        let (tp, warnings) = generate(&p, &op);
        assert!(warnings.is_empty(), "{warnings:?}");
        let lap = final_lap(&tp);
        let lifted = runs(&lap, |m| m.z_max() > TOL);
        assert_eq!(lifted.len(), 4, "{lifted:?}");
        let span_mm = TAB_WIDTH_MM + DIAMETER_MM;
        for run in &lifted {
            assert_approx_eq!(run.length_mm, span_mm, TOL);
        }
        let peaks = lap
            .iter()
            .filter(|m| (m.mv.to.z - TAB_Z_MM).abs() <= 1e-9)
            .count();
        assert_eq!(peaks, 4, "the top is reached exactly once per tab");
        assert!(
            lap.iter()
                .all(|m| m.from.z >= -1e-12 && m.mv.to.z <= TAB_Z_MM + 1e-12)
        );
        // Every lap point lies on the raised cosine for its distance along
        // its span.
        let mut at_mm = 0.0;
        for m in &lap {
            at_mm += m.length_mm;
            let Some(run) = lifted
                .iter()
                .find(|r| at_mm > r.start_mm + TOL && at_mm < r.start_mm + r.length_mm - TOL)
            else {
                continue;
            };
            let u = (at_mm - run.start_mm) / span_mm;
            assert_approx_eq!(m.mv.to.z, smooth_tab_z_mm(u, 0.0, TAB_Z_MM), 1e-6);
            if m.mv.to.z < m.from.z - EPS_MM {
                assert_approx_eq!(m.mv.feed_mm_min.unwrap(), PLUNGE_MM_MIN, EPS_MM);
            }
        }
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
    fn tabs_above_cut_depth_warn() {
        // The cut stops at Z 6, above a 3 mm tab: the tabs never apply.
        let (p, mut op) = coaster(Some(four_rect_tabs()));
        op.depth_mm = 6.0;
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOutsideCutRange]);
        assert_eq!(warnings[0].param("op"), Some("o1"));
        assert_eq!(warnings[0].param("reason"), Some("cut_stops_above_tab"));
        let (plain, _) = generate(&p, &ProfileOp { tabs: None, ..op });
        assert_eq!(tp, plain, "tabs out of reach change nothing");

        // A tab top above the stock top is reported, and still applied to
        // every pass, in the air above the stock.
        let mut spec = four_rect_tabs();
        spec.height_mm = THICKNESS_MM + 2.0;
        let (p, op) = coaster(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        assert_eq!(codes(&warnings), vec![WarningCode::TabsOutsideCutRange]);
        assert_eq!(warnings[0].param("reason"), Some("above_stock_top"));
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
