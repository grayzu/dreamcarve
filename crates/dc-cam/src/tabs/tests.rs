// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/tabs.ts

//! Tests of holding tabs: where a request lays them along a path, and
//! the moves a Profile cuts over them.

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
    let square =
        |side_mm: f64| Polyline::rect(Point::new(-side_mm / 2.0, -side_mm / 2.0), side_mm, side_mm);
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
        shape: None,
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
        construction: false,
        folder: None,
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
        last_pass: None,
        start: dc_project::StartAt::Auto,
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
        avoid_corners: false,
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

/// A count is shared among the pieces of one outline by length,
/// the largest remainders rounded up, so the shares add up to it; the
/// earlier piece takes the odd tab where two are the same length.
#[test]
fn shares_split_a_count_by_length() {
    let room = [9, 9, 9];
    assert_eq!(shares(4, &[40.0, 40.0, 100.0], &room), [1, 1, 2]);
    assert_eq!(shares(4, &[50.0, 150.0], &room), [1, 3]);
    assert_eq!(shares(4, &[10.0, 10.0], &room), [2, 2]);
    assert_eq!(shares(5, &[10.0, 10.0, 10.0], &room), [2, 2, 1]);
    assert_eq!(shares(1, &[10.0, 30.0, 20.0], &room), [0, 1, 0]);
    assert_eq!(shares(0, &[10.0, 30.0], &room), [0, 0]);
    // Lengths equal but for rounding share as equal ones do.
    assert_eq!(shares(3, &[10.0, 10.0 + 1e-12], &room), [2, 1]);
    for count in 0..=12 {
        let split = shares(count, &[13.0, 7.5, 61.0, 0.2], &[99; 4]);
        assert_eq!(split.iter().sum::<u32>(), count, "{split:?}");
    }
}

/// No piece is asked for more than it holds while another has
/// room, so the whole count is placed whenever the pieces can hold it.
/// When they cannot, the tabs left over are asked of them by length,
/// for each piece to cut down with its coverage warning.
#[test]
fn shares_keep_to_what_each_piece_holds() {
    // By length the short piece's share is 1, which it cannot hold.
    assert_eq!(shares(5, &[19.0, 100.0], &[0, 5]), [0, 5]);
    // The long piece is full at 2: the others take the rest by length.
    assert_eq!(shares(6, &[100.0, 30.0, 20.0], &[2, 9, 9]), [2, 2, 2]);
    assert_eq!(shares(7, &[100.0, 30.0, 20.0], &[2, 9, 9]), [2, 3, 2]);
    // Three pieces that hold one each cannot hold four: the fourth is
    // asked of the first, which cuts it down.
    assert_eq!(shares(4, &[10.0, 10.0, 10.0], &[1, 1, 1]), [2, 1, 1]);
    // Pieces with no length take none, and no pieces none.
    assert_eq!(shares(3, &[0.0, 0.0], &[5, 5]), [0, 0]);
    assert_eq!(shares(3, &[0.0, 10.0], &[5, 5]), [0, 3]);
    assert!(shares(3, &[], &[]).is_empty());
}

/// A request shared among the pieces of one outline. A piece
/// holds as many tabs as cover at most half of it, the test `fit`
/// cuts a count down by: with the 8 mm tab and a 6 mm bit a tab keeps
/// 14 mm raised, so 56 mm holds two and 55.9 mm one.
/// Distances count along the pieces end to end, modulo their whole
/// length, each on its piece from that piece's start.
#[test]
fn request_is_shared_among_the_pieces_of_an_outline() {
    let op = OperationId::new("op1");
    let diameter_mm = 6.0;
    let request = |placement| {
        Request::new(&op, &tab_spec(placement, TabShape::Rect), diameter_mm).expect("a request")
    };
    let span_mm = TAB_WIDTH_MM + diameter_mm;
    let counted = request(TabPlacement::Count { count: 4 });
    assert_eq!(counted.most(4.0 * span_mm), 2);
    assert_eq!(counted.most(4.0 * span_mm - 0.1), 1);
    assert_eq!(counted.most(1.0), 0);
    let placements = |shared: Vec<Request>| -> Vec<TabPlacement> {
        shared.into_iter().map(|r| r.placement).collect()
    };
    assert_eq!(
        placements(counted.shared(&[40.0, 40.0, 100.0])),
        [1, 1, 2].map(|count| TabPlacement::Count { count })
    );
    // The first piece holds none: the second, which holds four, takes
    // them all.
    assert_eq!(
        placements(counted.shared(&[20.0, 8.0 * span_mm])),
        [0, 4].map(|count| TabPlacement::Count { count })
    );
    let at = request(TabPlacement::AtDistances {
        distances_mm: vec![5.0, 45.0, 130.0, 40.0],
    });
    let shared = placements(at.shared(&[40.0, 60.0]));
    let [
        TabPlacement::AtDistances {
            distances_mm: first,
        },
        TabPlacement::AtDistances {
            distances_mm: second,
        },
    ] = &shared[..]
    else {
        panic!("{shared:?}");
    };
    // 130 mm is 30 mm round again; 40 mm is the second piece's start.
    for (got, want) in first.iter().zip([5.0, 30.0]) {
        assert_approx_eq!(*got, want, TOL);
    }
    for (got, want) in second.iter().zip([5.0, 0.0]) {
        assert_approx_eq!(*got, want, TOL);
    }
    assert_eq!((first.len(), second.len()), (2, 2));
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
                avoid_corners: false,
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
    // in half of it are cut, and that warns.
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
    // one; only the one that fits in half of it is cut, so
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
    // One 14.35 mm tab covers at most half of 54.6 mm, so one is cut;
    // four tabs fit in half only if each is at most
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

/// A hole's slug is waste, so by default only the part's
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

/// With `on_holes` the hole is held too, by the same four tabs
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

/// An outline too short for its tabs gets the most that keep
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

/// A part that asked for tabs keeps at least one, however short
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
    // (S10), with no vertical step.
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

/// S10: a smooth tab's down slope descends no faster than the
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

// ── #285: tabs off corners ──

/// A request for rect tabs `width_mm` wide at `placement`, off corners
/// when `avoid_corners`.
fn corner_spec(placement: TabPlacement, width_mm: f64, avoid_corners: bool) -> TabSpec {
    TabSpec {
        width_mm,
        avoid_corners,
        ..tab_spec(placement, TabShape::Rect)
    }
}

/// Acceptance 47's part: a 50 mm square rotated 45° about its centre
/// at (60, 60), cut outside.
fn rotated_square(tabs: Option<TabSpec>) -> (Project, ProfileOp) {
    let (mut p, op) = job(
        "Square",
        vec![Polyline::rect(Point::new(-25.0, -25.0), 50.0, 50.0)],
        Side::Outside,
        tabs,
    );
    p.instances[0].transform =
        Transform::rotate(std::f64::consts::FRAC_PI_4).then(Transform::translate(60.0, 60.0));
    (p, op)
}

/// A 50 mm square cut on the line, so its tool path has four sharp
/// 90° corners 50 mm apart.
fn on_line_square(tabs: Option<TabSpec>) -> (Project, ProfileOp) {
    job(
        "Square",
        vec![Polyline::rect(Point::new(-25.0, -25.0), 50.0, 50.0)],
        Side::OnLine,
        tabs,
    )
}

/// Where each raised run of `lap` is centred, mm along it, in order.
fn run_centres(lap: &[XyMove]) -> Vec<f64> {
    let l: f64 = lap.iter().map(|m| m.length_mm).sum();
    let mut centres: Vec<f64> = raised_runs(lap)
        .iter()
        .map(|r| (r.start_mm + r.length_mm / 2.0).rem_euclid(l))
        .collect();
    centres.sort_by(f64::total_cmp);
    centres
}

/// Where `lap`, made of straight moves, turns: the distance along it
/// of each move's start whose direction differs from the move
/// before's (the lap's start counts when it turns there).
fn turns_at(lap: &[XyMove]) -> Vec<f64> {
    let dir = |m: &XyMove| (m.mv.to.xy() - m.from.xy()).normalized().unwrap();
    let mut at = Vec::new();
    let mut walked = 0.0;
    for (k, m) in lap.iter().enumerate() {
        let before = &lap[(k + lap.len() - 1) % lap.len()];
        if dir(before).cross(dir(m)).abs() > 1e-9 {
            at.push(walked);
        }
        walked += m.length_mm;
    }
    at
}

/// The `tab_on_corner` warnings, as `(instance name, distance)`.
fn on_corner(warnings: &[Warning]) -> Vec<(String, f64)> {
    warnings
        .iter()
        .filter_map(|w| match w {
            Warning::TabOnCorner {
                shape, distance_mm, ..
            } => Some((shape.instance_name.clone(), *distance_mm)),
            _ => None,
        })
        .collect()
}

/// Acceptance 47: a 50 mm square rotated 45°, cut outside with
/// four evenly spaced rect tabs, its cut starting nearest the origin
/// (mid-side, so each tab is centred 27.5 mm along, in the middle of
/// the arc round the next corner). Without `avoid_corners` the tabs sit
/// on those arcs. With it, every tab stretch lies on a straight side:
/// no move at the tab top is an arc, and each of the four stretches is
/// still its tab's width plus the bit's diameter, at the tab's height,
/// with the cut at full depth between them. Three tabs from the
/// path's own start put one on a corner too, and it moves off.
#[test]
fn rotated_square_tabs_leave_corners() {
    let span_mm = TAB_WIDTH_MM + DIAMETER_MM;
    let near_origin = dc_project::StartAt::Near {
        x_mm: 0.0,
        y_mm: 0.0,
    };
    for (count, start) in [(4, near_origin), (3, dc_project::StartAt::Auto)] {
        let placement = || TabPlacement::Count { count };
        let (p, mut op) = rotated_square(Some(corner_spec(placement(), TAB_WIDTH_MM, false)));
        op.start = start;
        let (before, _) = generate(&p, &op);
        let lap = final_lap(&before);
        assert!(
            lap.iter()
                .any(|m| m.at_z(TAB_Z_MM) && !matches!(m.mv.kind, MoveKind::Linear)),
            "{count}: without avoid_corners a tab sits on a corner arc: {lap:?}"
        );

        let (p, mut op) = rotated_square(Some(corner_spec(placement(), TAB_WIDTH_MM, true)));
        op.start = start;
        let (tp, warnings) = generate(&p, &op);
        assert!(on_corner(&warnings).is_empty(), "{count}: {warnings:?}");
        let lap = final_lap(&tp);
        for m in lap.iter().filter(|m| m.at_z(TAB_Z_MM)) {
            assert!(
                matches!(m.mv.kind, MoveKind::Linear),
                "{count}: on a corner: {m:?}"
            );
        }
        let raised = raised_runs(&lap);
        assert_eq!(raised.len(), usize::from(count), "{raised:?}");
        for run in &raised {
            assert_approx_eq!(run.length_mm, span_mm, TOL);
        }
        // Between the tabs the last pass cuts at full depth, every arc
        // round a corner among it.
        let arcs: Vec<&XyMove> = lap
            .iter()
            .filter(|m| !matches!(m.mv.kind, MoveKind::Linear))
            .collect();
        assert!(arcs.len() >= 4, "{count}: {arcs:?}");
        assert!(arcs.iter().all(|m| m.at_z(0.0)), "{count}: {arcs:?}");
        assert!(lap.iter().all(|m| m.at_z(0.0) || m.at_z(TAB_Z_MM)));
    }
}

/// A circle turns `D / R` per bit diameter, under 30° for a
/// radius above 1.91 D, so the coaster's tabs never move: its
/// toolpath is the same with `avoid_corners` on and off, with no
/// `tab_on_corner`. A circle whose path radius is 1.5 D turns 38° per
/// diameter, a corner everywhere: its one tab stays, and warns.
#[test]
fn circle_tabs_never_move() {
    let toolpath = |avoid: bool| {
        let spec = corner_spec(TabPlacement::Count { count: 4 }, TAB_WIDTH_MM, avoid);
        let (p, op) = coaster(Some(spec));
        generate(&p, &op)
    };
    let ((off, _), (on, warnings)) = (toolpath(false), toolpath(true));
    assert_eq!(on, off);
    assert!(warnings.is_empty(), "{warnings:?}");

    let spec = corner_spec(TabPlacement::Count { count: 1 }, 2.0, true);
    let (p, op) = circle_job(DIAMETER_MM, Side::Outside, Some(spec));
    let (tp, warnings) = generate(&p, &op);
    assert_eq!(on_corner(&warnings).len(), 1, "{warnings:?}");
    assert!(
        warnings.iter().any(|w| matches!(
            w,
            Warning::TabOnCorner {
                reason: crate::warning::TabOnCornerReason::Everywhere,
                ..
            }
        )),
        "{warnings:?}"
    );
    assert_eq!(raised_runs(&final_lap(&tp)).len(), 1);
}

/// A tab on a corner moves at most half the gap to each of its
/// neighbours. On the line, a 50 mm square's path turns 90° at each
/// corner; a 2 mm tab's stretch, 8.35 mm with the 6.35 mm bit, is off
/// the corner at 50 mm once its centre is 7.35 mm or more away. A tab
/// at 49 mm would come off it soonest going back, to 42.65 mm, but its
/// neighbour at 40 mm lets it go back only 4.5 mm, so it goes forward
/// to 57.35 mm, within half its 31 mm gap to the tab at 80. Those two
/// stay. The cut keeps every tab's width and height.
#[test]
fn tab_never_moves_past_half_gap() {
    let at = |avoid: bool| {
        let spec = corner_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![40.0, 49.0, 80.0],
            },
            2.0,
            avoid,
        );
        let (p, op) = on_line_square(Some(spec));
        let (tp, warnings) = generate(&p, &op);
        (final_lap(&tp), warnings)
    };
    let (lap, _) = at(false);
    assert_eq!(turns_at(&lap), vec![0.0, 50.0, 100.0, 150.0]);
    let before = run_centres(&lap);
    let (lap, warnings) = at(true);
    assert!(on_corner(&warnings).is_empty(), "{warnings:?}");
    let after = run_centres(&lap);
    assert_eq!(before.len(), 3, "{before:?}");
    assert_eq!(after.len(), 3, "{after:?}");
    for (c, e) in before.iter().zip([40.0, 49.0, 80.0]) {
        assert_approx_eq!(*c, e, TOL);
    }
    for (c, e) in after.iter().zip([40.0, 57.35, 80.0]) {
        assert_approx_eq!(*c, e, 1e-3);
    }
    // Half the gap to each neighbour, both ways round.
    let l = 200.0;
    for (k, (b, a)) in before.iter().zip(&after).enumerate() {
        let prev = before[(k + 2) % 3];
        let next = before[(k + 1) % 3];
        let back = (b - prev).rem_euclid(l) / 2.0;
        let ahead = (next - b).rem_euclid(l) / 2.0;
        let moved = a - b;
        assert!(
            moved <= ahead + TOL && -moved <= back + TOL,
            "{k}: {b} → {a}"
        );
    }
    for run in raised_runs(&lap) {
        assert_approx_eq!(run.length_mm, 2.0 + DIAMETER_MM, TOL);
    }
}

/// A tab on a corner with nowhere to go, its neighbours too
/// close both ways, stays where it is with `tab_on_corner` naming the
/// outline and its distance along the path, and is cut at its full
/// width and height. The #109 floor holds too: an 8 mm square cut
/// outside, too short for its four tabs and a corner everywhere,
/// keeps the one tab #109 leaves it, warned both ways.
#[test]
fn blocked_tab_warns_tab_on_corner() {
    let spec = corner_spec(
        TabPlacement::AtDistances {
            distances_mm: vec![40.0, 49.0, 60.0],
        },
        2.0,
        true,
    );
    let (p, op) = on_line_square(Some(spec));
    let (tp, warnings) = generate(&p, &op);
    let blocked = on_corner(&warnings);
    assert_eq!(blocked.len(), 1, "{warnings:?}");
    assert_eq!(blocked[0].0, "Square 1");
    assert_approx_eq!(blocked[0].1, 49.0, TOL);
    let text = warnings
        .iter()
        .find(|w| w.code() == WarningCode::TabOnCorner)
        .map(ToString::to_string)
        .unwrap_or_default();
    assert!(text.contains("49 mm along"), "{text}");
    assert!(text.contains("half way to the next tab"), "{text}");
    let lap = final_lap(&tp);
    let centres = run_centres(&lap);
    assert_eq!(centres.len(), 3, "{centres:?}");
    for (c, e) in centres.iter().zip([40.0, 49.0, 60.0]) {
        assert_approx_eq!(*c, e, TOL);
    }
    for run in raised_runs(&lap) {
        assert_approx_eq!(run.length_mm, 2.0 + DIAMETER_MM, TOL);
    }

    let spec = corner_spec(TabPlacement::Count { count: 4 }, TAB_WIDTH_MM, true);
    let (p, op) = job(
        "Small",
        vec![Polyline::rect(Point::new(-4.0, -4.0), 8.0, 8.0)],
        Side::Outside,
        Some(spec),
    );
    let (tp, warnings) = generate(&p, &op);
    let (asked, cut, _) = cover_of(&warnings);
    assert_eq!((asked, cut), (4, 1));
    assert_eq!(on_corner(&warnings).len(), 1, "{warnings:?}");
    let raised = raised_runs(&final_lap(&tp));
    assert_eq!(raised.len(), 1, "the part keeps its tab: {raised:?}");
    assert_approx_eq!(raised[0].length_mm, TAB_WIDTH_MM + DIAMETER_MM, TOL);
}

/// On an open path a tab never moves past an end. A line 50 mm
/// along, then 6 mm up, cut on the line: a 2 mm tab at 53 mm is on the
/// corner at 50. Off it ahead would be 57.35 mm, past the 56 mm end,
/// so it goes back to 42.65 mm, whichever end the cut starts at.
#[test]
fn open_path_tab_stops_at_end() {
    for direction in [CutDirection::Conventional, CutDirection::Climb] {
        let spec = corner_spec(
            TabPlacement::AtDistances {
                distances_mm: vec![53.0],
            },
            2.0,
            true,
        );
        let (p, mut op) = job(
            "Hook",
            vec![Polyline::new(
                vec![
                    dc_geometry::Vertex::line(-25.0, 0.0),
                    dc_geometry::Vertex::line(25.0, 0.0),
                    dc_geometry::Vertex::line(25.0, 6.0),
                ],
                false,
            )],
            Side::OnLine,
            Some(spec),
        );
        op.direction = direction;
        let (tp, warnings) = generate(&p, &op);
        assert!(
            on_corner(&warnings).is_empty(),
            "{direction:?}: {warnings:?}"
        );
        // The tab top is held 4.175 mm either side of 42.65 mm along
        // the line as drawn, which starts at x = 35.
        let held: Vec<Point3> = final_lap(&tp)
            .iter()
            .filter(|m| m.at_z(TAB_Z_MM))
            .flat_map(|m| [m.from, m.mv.to])
            .collect();
        assert!(!held.is_empty(), "{direction:?}");
        assert!(held.iter().all(|q| (q.y - 60.0).abs() <= TOL), "{held:?}");
        let lo = held.iter().map(|q| q.x).fold(f64::MAX, f64::min);
        let hi = held.iter().map(|q| q.x).fold(f64::MIN, f64::max);
        let half_mm = (2.0 + DIAMETER_MM) / 2.0;
        assert_approx_eq!(lo, 35.0 + 42.65 - half_mm, 1e-3);
        assert_approx_eq!(hi, 35.0 + 42.65 + half_mm, 1e-3);
    }
}

/// A smooth span an end of an open path cuts short holds the tab top
/// right to that end (`layout_open`): the path leaves no room for the
/// slope there. A 4 mm tab cut with a 6 mm bit raises 5 mm either side
/// of its centre: the top for the 3 mm radius, then down over 2 mm.
/// Laid at 0, 1 and 4 mm along a 50 mm open path, its span runs past
/// the path's start; at 49 mm, past its end; at 5 mm it fits, and
/// keeps both slopes. `reversed`, the path is cut from its last
/// vertex, and the layout is measured from there.
///
/// Before, the slope was laid inside what the end left of the span,
/// so a pass was at its own depth at the path's end, at or beside the
/// tab's centre, and cut the tab through.
#[test]
fn smooth_span_cut_short_by_an_open_end_holds_its_top_there() {
    let tabs = Tabs::new(3.0, 6.0, 0.01);
    let cut = |centre_mm: f64| TabCut {
        centre_mm,
        width_mm: 4.0,
        top_z_mm: 3.0,
        shape: TabShape::Smooth,
    };
    // (centre, [(distance along the path as drawn, Z of a pass at 0)]).
    // A span is asked just inside the path's end it is held to: it is
    // half open, so the far end of the path as cut is past it.
    let cases: [(f64, &[(f64, f64)]); 5] = [
        (0.0, &[(0.001, 3.0), (3.0, 3.0), (4.0, 1.5), (5.5, 0.0)]),
        (1.0, &[(0.001, 3.0), (4.0, 3.0), (5.0, 1.5), (6.5, 0.0)]),
        // The path's start is on the tab's slope: held, so no lower.
        (4.0, &[(0.001, 3.0), (7.0, 3.0), (8.0, 1.5), (9.5, 0.0)]),
        (
            49.0,
            &[(43.5, 0.0), (45.0, 1.5), (46.0, 3.0), (49.999, 3.0)],
        ),
        (
            5.0,
            &[(0.0, 0.0), (1.0, 1.5), (2.0, 3.0), (8.0, 3.0), (9.0, 1.5)],
        ),
    ];
    for reversed in [false, true] {
        for (centre_mm, expected) in cases {
            let (layout, overlapped) =
                tabs.layout(&[cut(centre_mm)], 50.0, PathEnds::Open { reversed });
            assert!(!overlapped);
            for &(drawn_mm, z_mm) in expected {
                let along_mm = if reversed { 50.0 - drawn_mm } else { drawn_mm };
                assert_approx_eq!(layout.z_at_mm(along_mm, 0.0), z_mm, TOL);
            }
        }
    }
    // A span that starts with one the end cuts short is held with it,
    // whichever is laid first.
    for cuts in [[cut(1.0), cut(5.0)], [cut(5.0), cut(1.0)]] {
        let (layout, overlapped) = tabs.layout(&cuts, 50.0, PathEnds::Open { reversed: false });
        assert!(overlapped);
        assert_approx_eq!(layout.z_at_mm(0.0, 0.0), 3.0, TOL);
        assert_approx_eq!(layout.z_at_mm(9.0, 0.0), 1.5, TOL);
    }
}

/// #285: `avoid_corners: false` lays the tabs as before: on the
/// rotated square, four tabs centred an eighth, three eighths, five
/// eighths and seven eighths of the way along the path from its
/// start, whatever the corners.
#[test]
fn avoid_corners_false_is_byte_identical() {
    let (p, op) = rotated_square(Some(corner_spec(
        TabPlacement::Count { count: 4 },
        TAB_WIDTH_MM,
        false,
    )));
    let (tp, warnings) = generate(&p, &op);
    assert!(warnings.is_empty(), "{warnings:?}");
    let lap = final_lap(&tp);
    let l: f64 = lap.iter().map(|m| m.length_mm).sum();
    assert_approx_eq!(l, 200.0 + TAU * DIAMETER_MM / 2.0, 1e-6);
    let centres = run_centres(&lap);
    assert_eq!(centres.len(), 4);
    for (k, c) in centres.iter().enumerate() {
        assert_approx_eq!(*c, l * (2.0 * k as f64 + 1.0) / 8.0, TOL);
    }
}
