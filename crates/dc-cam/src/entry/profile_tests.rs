// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/toolpaths/edgeEntryPolicy.test.ts

//! Entry on Profile (`docs/specs/phase-2/design.md`, "Algorithms → Entry",
//! "Test plan → dc-cam — entry"; functional spec capability 16, acceptance
//! 26, S9 and S10). Tests marked "port" follow PureCutCNC's
//! `edgeEntryPolicy.test.ts` on its fixture: a 6 mm tool, feed 800, plunge
//! 300, conventional, 2 mm steps through 6 mm, a round boss (r 15 at
//! (40, 40)) cut Outside and a 34 × 26 opening at (10, 10) cut Inside.
//! Upstream's features with `subtract` are DreamCarve's Inside targets and
//! `add` its Outside targets; upstream's obstacles are features in the Z
//! span, DreamCarve's are other targets of the operation (design spec,
//! "Waste domain for Profile"). Upstream's tabs are rectangles in XY;
//! DreamCarve's are distances along the path, so the tab port plants one
//! there.

use super::*;
use crate::profile::generate_profile;
use crate::test_support::assert_safe;
use crate::toolpath::{MoveKind, Toolpath};
use crate::warning::{Warning, WarningCode};
use crate::{CamError, Move};
use dc_geometry::{EPS_MM, Point, Point3, Polyline, Transform, Vertex, assert_approx_eq};
use dc_project::test_support::sample_project;
use dc_project::{
    CornerRelief, DefinitionId, FeatureDefinition, FeatureInstance, InstanceId, MachiningOrder,
    Operation, OperationId, ProfileOp, Project, Side, Stock, TabPlacement, TabShape, TabSpec, Tool,
    ToolId, ToolKind,
};
use std::collections::BTreeMap;

const TOOL_DIAMETER_MM: f64 = 6.0;
const TOOL_RADIUS_MM: f64 = TOOL_DIAMETER_MM / 2.0;
const FEED: f64 = 800.0;
const PLUNGE: f64 = 300.0;
const THICKNESS_MM: f64 = 6.0;
const SAFE_Z_MM: f64 = THICKNESS_MM + 5.0;
/// Every pass of the stepped fixtures, 2 mm steps through 6 mm.
const PASS_Z_MM: [f64; 3] = [4.0, 2.0, 0.0];
/// Upstream's `GEOMETRIC_TOLERANCE`, plus the flatten tolerance: the waste
/// domain is built from arcs flattened to chords within it, so a cutter
/// riding a curved boundary may stray that far.
const GEOMETRIC_TOLERANCE_MM: f64 = 1e-3 + dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM;

/// Upstream `BOSS`: centre and radius.
const BOSS: (f64, f64, f64) = (40.0, 40.0, 15.0);
/// Upstream `HOLE`: x, y, w, h.
const HOLE: (f64, f64, f64, f64) = (10.0, 10.0, 34.0, 26.0);

fn tool(diameter_mm: f64, feed: f64, plunge: f64) -> Tool {
    Tool {
        id: ToolId::new("t1"),
        name: "t1".into(),
        number: None,
        cutter: ToolKind::EndMill,
        diameter_mm,
        flutes: 2,
        feed_mm_min: Some(feed),
        plunge_mm_min: Some(plunge),
        rpm: Some(18000),
        step_down_mm: Some(2.0),
        step_over_fraction: Some(0.4),
    }
}

/// A 120 × 120 × 6 mm project with one instance per shape (each its own
/// definition, placed where it is drawn) and upstream's 6 mm tool.
fn project_of(shapes: &[(&str, Vec<Polyline>)]) -> Project {
    let mut p = sample_project();
    p.stock = Stock {
        width_mm: 120.0,
        height_mm: 120.0,
        thickness_mm: Some(THICKNESS_MM),
    };
    p.tools = vec![tool(TOOL_DIAMETER_MM, FEED, PLUNGE)];
    p.definitions = BTreeMap::new();
    p.instances = Vec::new();
    for (id, profiles) in shapes {
        let def = DefinitionId::new(format!("d{id}"));
        p.definitions.insert(
            def.clone(),
            FeatureDefinition {
                id: def.clone(),
                name: (*id).to_owned(),
                profiles: profiles.clone(),
            },
        );
        p.instances.push(FeatureInstance {
            id: InstanceId::new(*id),
            definition: def,
            transform: Transform::IDENTITY,
            name: (*id).to_owned(),
            visible: true,
            locked: false,
            z_top_mm: Some(THICKNESS_MM),
            z_bottom_mm: Some(0.0),
        });
    }
    p.operations.clear();
    p
}

fn circle(c: (f64, f64, f64)) -> Polyline {
    Polyline::circle(Point::new(c.0, c.1), c.2)
}

fn rect(r: (f64, f64, f64, f64)) -> Polyline {
    Polyline::rect(Point::new(r.0, r.1), r.2, r.3)
}

fn boss_project() -> Project {
    project_of(&[("boss", vec![circle(BOSS)])])
}

fn hole_project() -> Project {
    project_of(&[("hole", vec![rect(HOLE)])])
}

/// A Profile of every instance of `p`, upstream's conventional 2 mm
/// steps through the stock, no tabs.
fn op(p: &Project, side: Side, entry: Entry) -> ProfileOp {
    ProfileOp {
        id: OperationId::new("o1"),
        name: "Profile".into(),
        enabled: true,
        targets: p.instances.iter().map(|i| i.id.clone()).collect(),
        tool: ToolId::new("t1"),
        side,
        depth_mm: Some(THICKNESS_MM),
        step_down_mm: None,
        direction: CutDirection::Conventional,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: 5.0,
        tabs: None,
        machining_order: MachiningOrder::FullDepthFirst,
        entry,
        lead: None,
        corner_relief: CornerRelief::None,
    }
}

/// Upstream's "finish" route: the whole depth in one pass.
fn finish(mut o: ProfileOp) -> ProfileOp {
    o.step_down_mm = Some(THICKNESS_MM);
    o
}

fn ramp() -> Entry {
    Entry::Ramp { angle_deg: 15.0 }
}

fn helix() -> Entry {
    Entry::Helix {
        diameter_fraction: 0.8,
        angle_deg: 5.0,
    }
}

/// Upstream `RAMPED`.
fn ramped() -> [Entry; 2] {
    [helix(), ramp()]
}

fn generate(p: &Project, o: &ProfileOp) -> (Toolpath, Vec<Warning>) {
    let (tp, warnings) = generate_profile(p, o).unwrap();
    assert_safe(&tp, p, &Operation::Profile(o.clone()));
    (tp, warnings)
}

fn fallbacks(warnings: &[Warning]) -> Vec<&Warning> {
    warnings
        .iter()
        .filter(|w| w.code == WarningCode::EntryFallback)
        .collect()
}

/// True when `mv` from `from` falls while it moves in XY: a ramp leg or
/// helix chord.
fn sloped(from: Point3, mv: &Move) -> bool {
    !mv.is_rapid() && mv.to.z < from.z - EPS_MM && from.xy().distance_to(mv.to.xy()) > EPS_MM
}

/// Every sloped move with the point it starts from.
fn sloped_moves(tp: &Toolpath) -> Vec<(Point3, Move)> {
    tp.moves
        .windows(2)
        .filter(|w| sloped(w[0].to, &w[1]))
        .map(|w| (w[0].to, w[1]))
        .collect()
}

/// Upstream `descentsIntoCut`: straight-down feed moves that end below the
/// stock top, into material.
fn plunges_into_stock(tp: &Toolpath) -> Vec<(Point3, Move)> {
    tp.moves
        .windows(2)
        .filter(|w| {
            let (from, mv) = (w[0].to, w[1]);
            !mv.is_rapid()
                && mv.to.z < from.z - EPS_MM
                && from.xy().distance_to(mv.to.xy()) <= EPS_MM
                && mv.to.z < THICKNESS_MM - EPS_MM
        })
        .map(|w| (w[0].to, w[1]))
        .collect()
}

/// Upstream `entrySamples`: every point the cutter centre passes through
/// on a descending entry move, eight per move.
fn entry_samples(tp: &Toolpath) -> Vec<Point> {
    let mut out = Vec::new();
    for (from, mv) in sloped_moves(tp) {
        for step in 0..=8 {
            let t = f64::from(step) / 8.0;
            out.push(from.xy() + (mv.to.xy() - from.xy()) * t);
        }
    }
    out
}

/// Entry samples of descents at or below `z_mm` (upstream
/// `entryMovesBelow`).
fn entry_samples_below(tp: &Toolpath, z_mm: f64) -> Vec<Point> {
    let mut out = Vec::new();
    for (from, mv) in sloped_moves(tp) {
        if from.z > z_mm + EPS_MM && mv.to.z > z_mm + EPS_MM {
            continue;
        }
        for step in 0..=8 {
            let t = f64::from(step) / 8.0;
            out.push(from.xy() + (mv.to.xy() - from.xy()) * t);
        }
    }
    out
}

/// Distance from `p` to the circle `c`'s disc: negative inside.
fn outside_circle(p: Point, c: (f64, f64, f64)) -> f64 {
    p.distance_to(Point::new(c.0, c.1)) - c.2
}

/// Port: upstream `testNoDescentReachesDepthInOnePlunge`. With a ramp or a
/// helix no straight plunge leads into the stock, on either side and for
/// a one-pass or stepped route, and no entry falls back; the plunge
/// baseline does plunge.
#[test]
fn no_descent_reaches_depth_in_one_plunge() {
    for entry in ramped() {
        for (label, p, side, one_pass) in [
            ("outside finish", boss_project(), Side::Outside, true),
            ("outside rough", boss_project(), Side::Outside, false),
            ("inside finish", hole_project(), Side::Inside, true),
            ("inside rough", hole_project(), Side::Inside, false),
        ] {
            let mut asked = op(&p, side, entry.clone());
            if one_pass {
                asked = finish(asked);
            }
            let plunged = ProfileOp {
                entry: Entry::Plunge,
                ..asked.clone()
            };
            let (base, _) = generate(&p, &plunged);
            assert!(
                !plunges_into_stock(&base).is_empty(),
                "{label}: the plunge baseline plunges"
            );
            let (tp, warnings) = generate(&p, &asked);
            assert!(
                plunges_into_stock(&tp).is_empty(),
                "{label} {entry:?}: {:?}",
                plunges_into_stock(&tp)
            );
            assert!(
                fallbacks(&warnings).is_empty(),
                "{label} {entry:?}: {warnings:?}"
            );
            assert!(
                !sloped_moves(&tp).is_empty(),
                "{label} {entry:?}: an entry was cut"
            );
        }
    }
}

/// Port: upstream `testEveryLevelIsEnteredThroughThePolicy`. Every pass Z
/// of a stepped route is reached by a descending XY move, and none falls
/// back.
#[test]
fn every_level_is_entered_through_the_policy() {
    for entry in ramped() {
        for (label, p, side) in [
            ("outside", boss_project(), Side::Outside),
            ("inside", hole_project(), Side::Inside),
        ] {
            let (tp, warnings) = generate(&p, &op(&p, side, entry.clone()));
            for z in PASS_Z_MM {
                assert!(
                    sloped_moves(&tp)
                        .iter()
                        .any(|(_, mv)| (mv.to.z - z).abs() <= EPS_MM),
                    "{label} {entry:?}: pass {z} is reached by a ramped entry"
                );
            }
            assert!(plunges_into_stock(&tp).is_empty(), "{label} {entry:?}");
            assert!(fallbacks(&warnings).is_empty(), "{label}: {warnings:?}");
        }
    }
}

/// Port: upstream `testTheOutsideEntryStaysOffThePartAndItsObstacles`.
/// Around the boss, with a neighbouring part the same operation cuts, the
/// entry's cutter bites the boss no deeper than the cut does and never
/// enters the neighbour. Asserted on the cutter, not the tool centre.
#[test]
fn outside_profile_ramp_stays_off_part_and_obstacles() {
    let obstacle = (40.0, 72.0, 7.0);
    let p = project_of(&[
        ("boss", vec![circle(BOSS)]),
        ("obstacle", vec![circle(obstacle)]),
    ]);
    for entry in ramped() {
        let (tp, warnings) = generate(&p, &op(&p, Side::Outside, entry.clone()));
        assert!(fallbacks(&warnings).is_empty(), "{entry:?}: {warnings:?}");
        let samples = entry_samples(&tp);
        assert!(!samples.is_empty(), "{entry:?}: an entry to check");
        for s in samples {
            for part in [BOSS, obstacle] {
                let clearance = outside_circle(s, part) - TOOL_RADIUS_MM;
                assert!(
                    clearance >= -GEOMETRIC_TOLERANCE_MM,
                    "{entry:?}: the cutter at {s:?} enters {part:?} by {}",
                    -clearance
                );
            }
        }
    }
}

/// Port: upstream `testTheInsideEntryStaysInsideTheOpening`. The cutter
/// never leaves the opening it cuts, whether it ramps or helixes.
#[test]
fn inside_profile_ramp_zigzags_in_the_hole() {
    let (x, y, w, h) = HOLE;
    let p = hole_project();
    for entry in ramped() {
        let (tp, warnings) = generate(&p, &op(&p, Side::Inside, entry.clone()));
        assert!(fallbacks(&warnings).is_empty(), "{entry:?}: {warnings:?}");
        let samples = entry_samples(&tp);
        assert!(!samples.is_empty(), "{entry:?}: an entry to check");
        let mut closest = f64::INFINITY;
        for s in samples {
            closest = closest
                .min(s.x - (x + TOOL_RADIUS_MM))
                .min(x + w - TOOL_RADIUS_MM - s.x)
                .min(s.y - (y + TOOL_RADIUS_MM))
                .min(y + h - TOOL_RADIUS_MM - s.y);
        }
        assert!(
            closest > -GEOMETRIC_TOLERANCE_MM,
            "{entry:?}: the cutter leaves the opening by {}",
            -closest
        );
    }
    // A ramp zigzags: its legs alternate direction. At 5° a leg falls
    // less than the 3 mm from 1 mm above the stock to the first pass.
    // (changed in #214: the first pass started at safe Z, and a 15° ramp
    // turned back; from just above the stock one 15° leg reaches it.)
    let (tp, _) = generate(&p, &op(&p, Side::Inside, Entry::Ramp { angle_deg: 5.0 }));
    let legs: Vec<Vec2> = sloped_moves(&tp)
        .iter()
        .take(2)
        .map(|(from, mv)| mv.to.xy() - from.xy())
        .collect();
    assert_eq!(legs.len(), 2, "two legs into the first pass");
    assert!(
        legs[0].dot(legs[1]) < 0.0,
        "the second leg turns back: {legs:?}"
    );
}

/// The distance from `p` to the stretch of `path` between `from_mm` and
/// `to_mm` along it, sampled finely enough for a straight stretch.
fn distance_to_stretch(path: &[Point], from_mm: f64, to_mm: f64, p: Point) -> f64 {
    let mut best = f64::INFINITY;
    let mut walked = 0.0;
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.distance_to(b);
        let steps = (len / 0.05).ceil().max(1.0) as usize;
        for k in 0..=steps {
            let t = k as f64 / steps as f64;
            let at = walked + len * t;
            if at >= from_mm - EPS_MM && at <= to_mm + EPS_MM {
                best = best.min(p.distance_to(a + (b - a) * t));
            }
        }
        walked += len;
    }
    best
}

/// The tool-centre path of the first lap at `z_mm` of `tp`, a program
/// that plunges (so its first level moves are the lap), from the lap's
/// start, arcs sampled every degree.
fn plunged_lap_points(tp: &Toolpath, z_mm: f64) -> Vec<Point> {
    let m = &tp.moves;
    let Some(first) = (1..m.len()).find(|&i| {
        !m[i].is_rapid()
            && (m[i].to.z - z_mm).abs() <= EPS_MM
            && m[i - 1].to.xy().distance_to(m[i].to.xy()) <= EPS_MM
    }) else {
        return Vec::new();
    };
    let mut at = m[first].to.xy();
    let mut out = vec![at];
    for mv in &m[first + 1..] {
        if mv.is_rapid() || (mv.to.z - z_mm).abs() > EPS_MM {
            break;
        }
        let to = mv.to.xy();
        let arc = match mv.kind {
            MoveKind::ArcCcw { centre } => Some((centre, 1.0)),
            MoveKind::ArcCw { centre } => Some((centre, -1.0)),
            _ => None,
        };
        if let Some((c, sense)) = arc {
            let (u, v) = (at - c, to - c);
            let mut sweep = u.cross(v).atan2(u.dot(v));
            if sense * sweep <= 0.0 {
                sweep += sense * std::f64::consts::TAU;
            }
            let steps = (sweep.abs().to_degrees().ceil() as usize).max(1);
            let start = u.y.atan2(u.x);
            for k in 1..=steps {
                let a = start + sweep * k as f64 / steps as f64;
                out.push(c + Vec2::new(a.cos(), a.sin()) * u.length());
            }
        } else {
            out.push(to);
        }
        at = to;
    }
    out
}

/// Port: upstream `testTabsAreKeptOutOfTheEntry`. A tab planted on the
/// natural entry: without the keep-out the cutter sits over it; with it,
/// no descent below the tab top brings the cutter onto the tab.
#[test]
fn tabs_are_kept_out_of_the_entry() {
    let p = boss_project();
    let plain = op(&p, Side::Outside, helix());
    let (untabbed, warnings) = generate(&p, &plain);
    assert!(fallbacks(&warnings).is_empty(), "{warnings:?}");
    // The tool-centre path, from the lap start (the block entry).
    let (plunged, _) = generate(
        &p,
        &ProfileOp {
            entry: Entry::Plunge,
            ..plain.clone()
        },
    );
    let lap = plunged_lap_points(&plunged, PASS_Z_MM[0]);
    assert!(lap.len() > 90, "a lap: {lap:?}");
    // The lap runs counter-clockwise from (58, 40) and the untabbed helix
    // sits just before its start. A 10 mm tab whose span (width + D) ends
    // 3.5 mm before the lap start, so the start itself stays a radius
    // clear of it: the tab's material runs from L − 3.5 − r − 10 to
    // L − 3.5 − r along the path.
    let length_mm = std::f64::consts::TAU * (BOSS.2 + TOOL_RADIUS_MM);
    let width_mm = 10.0;
    let span_end_mm = length_mm - 3.5;
    let centre_mm = span_end_mm - (width_mm + TOOL_DIAMETER_MM) / 2.0;
    let (from_mm, to_mm) = (
        span_end_mm - TOOL_RADIUS_MM - width_mm,
        span_end_mm - TOOL_RADIUS_MM,
    );
    let tab_top_mm = 5.0;
    let mut tabbed = plain.clone();
    tabbed.tabs = Some(TabSpec {
        placement: TabPlacement::AtDistances {
            distances_mm: vec![centre_mm],
        },
        width_mm,
        height_mm: tab_top_mm,
        shape: TabShape::Rect,
    });
    // How far the cutter at `s` reaches into the tab: the tab is the kerf
    // over its stretch, the stretch grown by r; the cutter adds r more.
    let bite =
        |s: Point| (2.0 * TOOL_RADIUS_MM - distance_to_stretch(&lap, from_mm, to_mm, s)).max(0.0);
    let worst = |tp: &Toolpath| {
        entry_samples_below(tp, tab_top_mm)
            .into_iter()
            .map(bite)
            .fold(0.0, f64::max)
    };
    // Upstream plants its tab square on the helix; here the tab must end
    // a radius before the lap start, so the untabbed helix bites its end.
    assert!(
        worst(&untabbed) >= 0.5,
        "the fixture plants the tab on the natural entry: {}",
        worst(&untabbed)
    );
    let (tp, warnings) = generate(&p, &tabbed);
    assert!(fallbacks(&warnings).is_empty(), "{warnings:?}");
    assert!(
        worst(&tp) <= GEOMETRIC_TOLERANCE_MM,
        "the entry is placed clear of the tab below its top: {} mm inside",
        worst(&tp)
    );
}

/// Port: upstream `testTheOutsideHelixTurnsTheExternalWay`. An outside
/// helix turns the other way round a boss than inside a cavity: the
/// signed area of its chords has the sense [`helix_is_ccw`] gives for
/// [`CutSide::External`], not [`CutSide::Internal`].
#[test]
fn outside_profile_helix_turns_the_external_way() {
    let p = boss_project();
    for direction in [CutDirection::Conventional, CutDirection::Climb] {
        let mut o = finish(op(&p, Side::Outside, helix()));
        o.direction = direction;
        let (tp, _) = generate(&p, &o);
        let chords = sloped_moves(&tp);
        assert!(chords.len() > 48, "{direction:?}: a helix");
        let twice: f64 = chords
            .iter()
            .map(|(from, mv)| from.x * mv.to.y - mv.to.x * from.y)
            .sum();
        assert_eq!(
            twice > 0.0,
            helix_is_ccw(direction, CutSide::External),
            "{direction:?}"
        );
        assert_ne!(twice > 0.0, helix_is_ccw(direction, CutSide::Internal));
    }
}

/// A 60 × 60 frame with a 30 × 30 hole and a 15 × 15 part inside the
/// hole, all cut Outside: the waste between the hole and the inner part is
/// 7.5 mm wide, so both their kerfs fill it and neither entry has room.
fn pinched_project() -> Project {
    project_of(&[
        (
            "frame",
            vec![
                rect((20.0, 20.0, 60.0, 60.0)),
                rect((35.0, 35.0, 30.0, 30.0)),
            ],
        ),
        ("inner", vec![rect((42.5, 42.5, 15.0, 15.0))]),
    ])
}

/// Port: upstream `testAnUnplaceableEntryFallsBackOutLoud`. Where the
/// waste domain is too tight the fallback chain runs and says so with
/// `entry_fallback`; the rest of the route still ramps.
#[test]
fn unplaceable_entry_falls_back_out_loud() {
    let p = pinched_project();
    for entry in ramped() {
        let (tp, warnings) = generate(&p, &op(&p, Side::Outside, entry.clone()));
        assert!(
            !plunges_into_stock(&tp).is_empty(),
            "{entry:?}: this fixture is the pinched one"
        );
        let fell = fallbacks(&warnings);
        assert!(!fell.is_empty(), "{entry:?}: {warnings:?}");
        for w in &fell {
            assert_eq!(w.param("used"), Some("plunge"), "{w:?}");
            assert_eq!(w.param("op"), Some("o1"));
        }
        assert!(
            fell.iter()
                .all(|w| w.param("instance") != Some("frame") || w.param("profile") == Some("1")),
            "the frame's outer wall has room: {fell:?}"
        );
        assert!(
            !sloped_moves(&tp).is_empty(),
            "{entry:?}: the contours that fit still ramp in"
        );
    }
}

/// Upstream `testPlungeAndAbsentAreUnchangedAndIdentical`, for DreamCarve
/// where `plunge` is what a migrated project has: every descent straight
/// down at the plunge rate, no sloped move, nothing warned.
#[test]
fn plunge_is_default_and_unchanged_from_phase_1() {
    for (p, side) in [
        (boss_project(), Side::Outside),
        (hole_project(), Side::Inside),
    ] {
        let (tp, warnings) = generate(&p, &op(&p, side, Entry::Plunge));
        assert!(warnings.is_empty(), "{warnings:?}");
        assert!(sloped_moves(&tp).is_empty());
        let plunges = plunges_into_stock(&tp);
        assert_eq!(plunges.len(), PASS_Z_MM.len(), "one plunge per pass");
        for (_, mv) in plunges {
            assert_eq!(mv.feed_mm_min, Some(PLUNGE));
        }
    }
}

/// Every pass after the first descends from the previous pass Z at the
/// block's entry point, where the previous lap ended: no retract and no
/// descent through the air above the floor already cut. The only rapid
/// below safe Z is the first pass's, straight down to 1 mm above the
/// stock (#214).
#[test]
fn descent_reaching_a_pass_follows_the_previous_floor() {
    for entry in ramped() {
        for (p, side) in [
            (boss_project(), Side::Outside),
            (hole_project(), Side::Inside),
        ] {
            let (tp, _) = generate(&p, &op(&p, side, entry.clone()));
            for (from, mv) in sloped_moves(&tp) {
                if let Some(k) = PASS_Z_MM.iter().position(|z| mv.to.z < z - EPS_MM) {
                    assert!(
                        from.z <= PASS_Z_MM[k] + EPS_MM,
                        "{entry:?} {side:?}: a descent below pass {k} starts above it: {from:?} -> {mv:?}"
                    );
                }
            }
            let below_safe: Vec<&Move> = tp
                .moves
                .iter()
                .filter(|m| m.is_rapid() && m.to.z < SAFE_Z_MM - EPS_MM)
                .collect();
            assert!(
                below_safe.len() == 1
                    && (below_safe[0].to.z - (THICKNESS_MM + 1.0)).abs() <= EPS_MM,
                "{entry:?} {side:?}: one rapid below safe Z, to the first pass's start: {below_safe:?}"
            );
            let rapids_up = tp
                .moves
                .windows(2)
                .filter(|w| w[1].is_rapid() && w[1].to.z > w[0].to.z + EPS_MM)
                .count();
            assert_eq!(rapids_up, 1, "{entry:?} {side:?}: one retract, at the end");
        }
    }
}

/// Acceptance test 26's part and neighbour: centre and radius.
const ACCEPTANCE_26_PARTS: ((f64, f64, f64), (f64, f64, f64)) =
    ((40.0, 40.0, 20.0), (85.0, 85.0, 12.0));

/// Acceptance test 26's outside fixture: both parts, a 6.35 mm tool at
/// feed 1500 and plunge 500, an outside climb Profile ramping at 15° in
/// 2 mm steps through the 6 mm stock.
fn acceptance_26_outside() -> (Project, ProfileOp) {
    let (part, neighbour) = ACCEPTANCE_26_PARTS;
    let mut p = project_of(&[
        ("part", vec![circle(part)]),
        ("neighbour", vec![circle(neighbour)]),
    ]);
    p.tools = vec![tool(6.35, 1500.0, 500.0)];
    let mut o = op(&p, Side::Outside, ramp());
    o.direction = CutDirection::Climb;
    (p, o)
}

/// Acceptance test 26, outside part: a 15° ramp around a part and a
/// neighbouring part, 6.35 mm tool, feed 1500, plunge 500. Every descent
/// slopes at 15° within 0.1° at the feed rate with its vertical rate at
/// most the plunge rate; its footprint stays off both parts; no vertical
/// plunge happens below the stock top.
#[test]
fn acceptance_26_outside_ramp_stays_off_the_part() {
    let (part, neighbour) = ACCEPTANCE_26_PARTS;
    let (p, o) = acceptance_26_outside();
    let (tp, warnings) = generate(&p, &o);
    assert!(fallbacks(&warnings).is_empty(), "{warnings:?}");
    let legs = sloped_moves(&tp);
    assert!(!legs.is_empty());
    for (from, mv) in &legs {
        let run = from.xy().distance_to(mv.to.xy());
        let slope = (from.z - mv.to.z).atan2(run).to_degrees();
        assert!(
            (slope - 15.0).abs() <= 0.1,
            "slope {slope}: {from:?} -> {mv:?}"
        );
        let feed = mv.feed_mm_min.unwrap();
        assert!(feed <= 1500.0 + 1e-9, "{mv:?}");
        let length = (run * run + (from.z - mv.to.z).powi(2)).sqrt();
        let vertical = feed * (from.z - mv.to.z) / length;
        assert!(vertical <= 500.0 + 1e-9, "vertical {vertical}: {mv:?}");
        // 1500 × sin 15° is under 500, so the ramp runs at the feed.
        assert_approx_eq!(feed, 1500.0, 1e-9);
    }
    let r = 6.35 / 2.0;
    for s in entry_samples(&tp) {
        for c in [part, neighbour] {
            assert!(
                outside_circle(s, c) - r >= -GEOMETRIC_TOLERANCE_MM,
                "the ramp at {s:?} enters {c:?}"
            );
        }
    }
    assert!(
        plunges_into_stock(&tp).is_empty(),
        "{:?}",
        plunges_into_stock(&tp)
    );
}

/// One descent: a run of consecutive sloped moves, each with the point it
/// starts from, and the index of the first move after it.
struct Descent {
    legs: Vec<(Point3, Move)>,
    next: usize,
}

/// Every descent of `tp`, in cut order.
fn descents(tp: &Toolpath) -> Vec<Descent> {
    let m = &tp.moves;
    let mut out: Vec<Descent> = Vec::new();
    let mut open = false;
    for i in 1..m.len() {
        if sloped(m[i - 1].to, &m[i]) {
            if !open {
                out.push(Descent {
                    legs: Vec::new(),
                    next: i,
                });
            }
            let d = out.last_mut().expect("a descent is open");
            d.legs.push((m[i - 1].to, m[i]));
            d.next = i + 1;
            open = true;
        } else {
            open = false;
        }
    }
    out
}

/// Acceptance test 26 (#250): "the first move into each pass is a zigzag".
/// On the outside fixture every pass of both parts is entered by a descent
/// of at least two legs, each turning back on the one before (XY dot
/// product < 0), each at 15° within 0.1°; the descent, with its level
/// handoff if it has one, ends at the lap's start point at the pass Z (an
/// outside lap is closed, so its start is where it ends).
#[test]
#[ignore = "#257"]
fn acceptance_26_outside_ramp_zigzags_at_every_pass() {
    let (p, o) = acceptance_26_outside();
    let (tp, warnings) = generate(&p, &o);
    assert!(fallbacks(&warnings).is_empty(), "{warnings:?}");
    let m = &tp.moves;
    let all = descents(&tp);
    let reached: Vec<f64> = all
        .iter()
        .map(|d| d.legs.last().expect("a descent has a leg").1.to.z)
        .collect();
    let want: Vec<f64> = PASS_Z_MM.iter().chain(PASS_Z_MM.iter()).copied().collect();
    assert_eq!(
        reached.len(),
        want.len(),
        "one descent per pass and part: {reached:?}"
    );
    for (k, (got, z)) in reached.iter().zip(&want).enumerate() {
        assert_approx_eq!(*got, *z, EPS_MM);
        let d = &all[k];
        assert!(
            d.legs.len() >= 2,
            "descent {k} into Z {z} has {} leg(s): {:?}",
            d.legs.len(),
            d.legs
        );
        for (j, (from, mv)) in d.legs.iter().enumerate() {
            let run = from.xy().distance_to(mv.to.xy());
            let slope = (from.z - mv.to.z).atan2(run).to_degrees();
            assert!(
                (slope - 15.0).abs() <= 0.1,
                "descent {k} leg {j} slopes {slope}°: {from:?} -> {mv:?}"
            );
        }
        for (j, w) in d.legs.windows(2).enumerate() {
            let a = w[0].1.to.xy() - w[0].0.xy();
            let b = w[1].1.to.xy() - w[1].0.xy();
            assert!(
                a.dot(b) < 0.0,
                "descent {k} leg {} does not turn back on leg {j}: {a:?} then {b:?}",
                j + 1
            );
        }
        // The lap: the level cutting moves at the pass Z after the descent.
        let lap: Vec<&Move> = m[d.next..]
            .iter()
            .take_while(|mv| !mv.is_rapid() && (mv.to.z - z).abs() <= EPS_MM)
            .collect();
        let lap_start = lap.last().expect("a lap follows the descent").to.xy();
        let mut end = d.legs.last().expect("a descent has a leg").1.to.xy();
        if end.distance_to(lap_start) > EPS_MM {
            // A level handoff from the ramp's end to the lap's start.
            end = lap[0].to.xy();
        }
        assert!(
            end.distance_to(lap_start) <= EPS_MM,
            "descent {k} ends at {end:?}, not the lap start {lap_start:?}"
        );
    }
}

/// The 100 × 50 rectangle at (10, 10) cut On line with a 6.35 mm tool.
fn on_line_project() -> Project {
    let mut p = project_of(&[("line", vec![rect((10.0, 10.0, 100.0, 50.0))])]);
    p.tools = vec![tool(6.35, 1500.0, 500.0)];
    p
}

/// True when `q` lies on the outline of the rectangle `r` (x, y, w, h).
fn on_rect(q: Point, r: (f64, f64, f64, f64)) -> bool {
    let (x0, y0, x1, y1) = (r.0, r.1, r.0 + r.2, r.1 + r.3);
    let within = q.x >= x0 - 1e-6 && q.x <= x1 + 1e-6 && q.y >= y0 - 1e-6 && q.y <= y1 + 1e-6;
    let on_edge = (q.x - x0).abs() <= 1e-6
        || (q.x - x1).abs() <= 1e-6
        || (q.y - y0).abs() <= 1e-6
        || (q.y - y1).abs() <= 1e-6;
    within && on_edge
}

/// Acceptance test 26, On line part, and the design spec's On line ramp:
/// the ramp runs back and forth along the cut path itself, from the start
/// vertex, `min(3·D, L)` out, at the entry's angle.
#[test]
fn on_line_ramp_runs_along_the_path_and_back() {
    let p = on_line_project();
    let angle_deg = 5.0;
    let (tp, warnings) = generate(&p, &op(&p, Side::OnLine, Entry::Ramp { angle_deg }));
    assert!(fallbacks(&warnings).is_empty(), "{warnings:?}");
    let legs = sloped_moves(&tp);
    assert!(legs.len() >= 2, "{legs:?}");
    let start = legs[0].0.xy();
    let run_mm = 3.0 * 6.35;
    let mut directions = Vec::new();
    for (from, mv) in &legs {
        assert!(
            on_rect(mv.to.xy(), (10.0, 10.0, 100.0, 50.0)),
            "{mv:?} leaves the line"
        );
        assert!(
            mv.to.xy().distance_to(start) <= run_mm + 1e-6,
            "{mv:?} runs past 3·D"
        );
        let slope = (from.z - mv.to.z)
            .atan2(from.xy().distance_to(mv.to.xy()))
            .to_degrees();
        assert!((slope - angle_deg).abs() <= 0.1, "slope {slope}");
        directions.push(mv.to.xy() - from.xy());
    }
    // Back and forth: the first pass descends 3 mm (1 mm above the stock
    // to 4) at 5°, more than one 19.05 mm leg's 1.67 mm, so it turns back.
    // (changed in #214: at 15° from safe Z it descended 7 mm, more than a
    // leg's 5.1 mm.)
    assert!(
        directions.windows(2).any(|w| w[0].dot(w[1]) < 0.0),
        "the ramp turns back: {directions:?}"
    );
    let furthest = legs
        .iter()
        .map(|(_, mv)| mv.to.xy().distance_to(start))
        .fold(0.0, f64::max);
    assert_approx_eq!(furthest, run_mm, 1e-6);
}

/// The design spec's On line lap: once the ramp reaches the pass Z the
/// tool returns along the path to the start vertex and cuts the whole
/// path from there, so the ramped stretch is cut again at full depth.
#[test]
fn on_line_lap_recuts_ramped_stretch_at_full_depth() {
    let p = on_line_project();
    let (tp, _) = generate(&p, &op(&p, Side::OnLine, ramp()));
    let start = sloped_moves(&tp)[0].0.xy();
    for z in PASS_Z_MM {
        // From the last move reaching this pass Z, the flat moves at it.
        let m = &tp.moves;
        let arrive = (1..m.len())
            .rfind(|&i| sloped(m[i - 1].to, &m[i]) && (m[i].to.z - z).abs() <= EPS_MM)
            .expect("the pass is ramped into");
        let mut at = m[arrive].to.xy();
        let mut back_home = (at.distance_to(start) <= EPS_MM).then_some(at);
        let mut lap_mm = 0.0;
        for mv in &m[arrive + 1..] {
            if (mv.to.z - z).abs() > EPS_MM || mv.is_rapid() {
                break;
            }
            let step = at.distance_to(mv.to.xy());
            if back_home.is_some() {
                lap_mm += step;
            }
            at = mv.to.xy();
            if back_home.is_none() && at.distance_to(start) <= EPS_MM {
                back_home = Some(at);
            }
            assert!(
                on_rect(at, (10.0, 10.0, 100.0, 50.0)),
                "{mv:?} leaves the line"
            );
        }
        assert!(back_home.is_some(), "pass {z}: back to the start vertex");
        assert_approx_eq!(lap_mm, 300.0, 1e-6);
        assert!(
            at.distance_to(start) <= EPS_MM,
            "the lap ends where it began"
        );
    }
}

/// Acceptance test 26, On line, on the open 100 mm line itself: the
/// along-the-path ramp on an open path runs out `3·D` and back.
#[test]
fn acceptance_26_on_line_ramps_along_a_100_mm_line() {
    let line = Polyline::new(
        vec![Vertex::line(10.0, 20.0), Vertex::line(110.0, 20.0)],
        false,
    );
    let t = EntryTool {
        diameter_mm: 6.35,
        feed_mm_min: 1500.0,
        plunge_mm_min: 500.0,
        direction: CutDirection::Climb,
        side: CutSide::Internal,
    };
    let d = along_path(&line, 15.0, f64::INFINITY, 11.0, 4.0, &t, 0.01);
    assert_eq!(d.used, EntryKind::Ramp);
    assert!(d.from.distance_to(Point::new(10.0, 20.0)) <= EPS_MM);
    let mut from = Point3::new(d.from.x, d.from.y, 11.0);
    let mut turned = false;
    let mut last_dx = 0.0;
    for mv in &d.moves {
        assert_approx_eq!(mv.to.y, 20.0, 1e-9);
        assert!(
            mv.to.x >= 10.0 - 1e-9 && mv.to.x <= 10.0 + 3.0 * 6.35 + 1e-9,
            "{mv:?}"
        );
        let dx = mv.to.x - from.x;
        if dx * last_dx < 0.0 {
            turned = true;
        }
        if dx.abs() > EPS_MM {
            last_dx = dx;
        }
        from = mv.to;
    }
    assert!(turned, "back and forth: {:?}", d.moves);
    let last = d.moves.last().unwrap().to;
    assert!(
        last.xy().distance_to(Point::new(10.0, 20.0)) <= EPS_MM,
        "ends at the start"
    );
    assert_approx_eq!(last.z, 4.0, EPS_MM);
}

/// The design spec's On line ramp on an open path shorter than one leg:
/// the legs reverse at the path's end, and the ramp never leaves it.
#[test]
fn on_line_ramp_reverses_at_the_end_of_a_short_open_path() {
    let line = Polyline::new(vec![Vertex::line(0.0, 0.0), Vertex::line(8.0, 0.0)], false);
    let t = EntryTool {
        diameter_mm: 6.35,
        feed_mm_min: 1500.0,
        plunge_mm_min: 500.0,
        direction: CutDirection::Climb,
        side: CutSide::Internal,
    };
    // 10 mm at 2.14 mm a leg: five legs, four turns.
    let d = along_path(&line, 15.0, f64::INFINITY, 10.0, 0.0, &t, 0.01);
    assert_eq!(d.used, EntryKind::Ramp);
    let xs: Vec<f64> = d.moves.iter().map(|m| m.to.x).collect();
    assert!(
        xs.iter().all(|x| (-1e-9..=8.0 + 1e-9).contains(x)),
        "{xs:?}"
    );
    assert!(
        xs.iter().filter(|x| (**x - 8.0).abs() <= 1e-9).count() >= 2,
        "turns at the far end more than once: {xs:?}"
    );
    // Every leg descends at 15°: 8 mm of run drops 8 · tan 15°.
    let mut from = Point3::new(0.0, 0.0, 10.0);
    for mv in &d.moves {
        if mv.to.z < from.z - EPS_MM {
            let slope = (from.z - mv.to.z)
                .atan2((mv.to.x - from.x).abs())
                .to_degrees();
            assert!((slope - 15.0).abs() <= 1e-6, "slope {slope}");
        }
        from = mv.to;
    }
}

/// An arc on the ramped stretch is split into chords within the flatten
/// tolerance while Z changes.
#[test]
fn on_line_ramp_splits_arcs_into_chords() {
    let p = project_of(&[("disc", vec![circle((60.0, 60.0, 30.0))])]);
    let mut p = p;
    p.tools = vec![tool(6.35, 1500.0, 500.0)];
    let (tp, _) = generate(&p, &op(&p, Side::OnLine, ramp()));
    let legs = sloped_moves(&tp);
    assert!(legs.len() > 4, "chords: {}", legs.len());
    for (from, mv) in legs {
        assert_eq!(mv.kind, MoveKind::Linear);
        for q in [
            from.xy(),
            mv.to.xy(),
            from.xy() + (mv.to.xy() - from.xy()) * 0.5,
        ] {
            let off = (q.distance_to(Point::new(60.0, 60.0)) - 30.0).abs();
            assert!(off <= 0.01 + 1e-9, "{q:?} is {off} mm off the circle");
        }
    }
}

/// Helix on an On line Profile is a validation error
/// (`helix_needs_waste_side`); the generator refuses it with a typed error
/// rather than guess.
#[test]
fn on_line_helix_is_an_error() {
    let p = on_line_project();
    let err = generate_profile(&p, &op(&p, Side::OnLine, helix())).unwrap_err();
    assert!(
        matches!(err, CamError::HelixNeedsWasteSide { .. }),
        "{err:?}"
    );
    assert!(err.to_string().contains("o1"), "{err}");
}

/// Acceptance test 26, last part: an inside Profile of a 7 mm hole with a
/// 6.35 mm tool has no room for a ramp, plunges, and warns
/// `entry_fallback`.
#[test]
fn acceptance_26_tight_hole_falls_back_to_plunge() {
    let mut p = project_of(&[("hole", vec![circle((50.0, 50.0, 3.5))])]);
    p.tools = vec![tool(6.35, 1500.0, 500.0)];
    let (tp, warnings) = generate(&p, &op(&p, Side::Inside, ramp()));
    let fell = fallbacks(&warnings);
    assert_eq!(fell.len(), 1, "{warnings:?}");
    assert_eq!(fell[0].param("requested"), Some("ramp"));
    assert_eq!(fell[0].param("used"), Some("plunge"));
    assert_eq!(fell[0].param("instance"), Some("hole"));
    assert!(sloped_moves(&tp).is_empty());
    assert_eq!(plunges_into_stock(&tp).len(), PASS_Z_MM.len());
}

/// The design spec's outside helix: it sits in the waste around the part,
/// and neither in a neighbouring part nor in its kerf (the neighbour's
/// tool-centre path grown by the radius).
#[test]
fn outside_profile_helix_avoids_neighbouring_part() {
    let neighbour = (64.0, 56.0, 7.0);
    let p = project_of(&[
        ("boss", vec![circle(BOSS)]),
        ("neighbour", vec![circle(neighbour)]),
    ]);
    let (tp, warnings) = generate(&p, &op(&p, Side::Outside, helix()));
    assert!(fallbacks(&warnings).is_empty(), "{warnings:?}");
    // Split the program into its two shapes at the retract between them.
    let m = &tp.moves;
    let split = (1..m.len())
        .find(|&i| m[i].is_rapid() && m[i].to.z > m[i - 1].to.z + EPS_MM)
        .unwrap();
    let first = Toolpath {
        moves: m[..split].to_vec(),
    };
    let second = Toolpath {
        moves: m[split..].to_vec(),
    };
    for half in [&first, &second] {
        let samples = entry_samples(half);
        assert!(!samples.is_empty());
        // Which shape this half cuts: its last lap ends on that shape's
        // tool-centre path, r from the outline.
        let end = half
            .moves
            .iter()
            .rev()
            .find(|mv| !mv.is_rapid())
            .unwrap()
            .to
            .xy();
        let cuts_boss = (outside_circle(end, BOSS) - TOOL_RADIUS_MM).abs() < 0.05;
        let (own, other) = if cuts_boss {
            (BOSS, neighbour)
        } else {
            (neighbour, BOSS)
        };
        for s in samples {
            assert!(outside_circle(s, own) - TOOL_RADIUS_MM >= -GEOMETRIC_TOLERANCE_MM);
            // Out of the other's kerf, the band r either side of its
            // tool-centre path: the centre is 2r from its outline.
            let clearance = outside_circle(s, other) - TOOL_DIAMETER_MM;
            assert!(
                clearance >= -GEOMETRIC_TOLERANCE_MM,
                "the helix at {s:?} enters the kerf round {other:?} by {}",
                -clearance
            );
        }
    }
}

/// The functional spec's default for new projects: a ramp at 15°, which
/// `init` writes, and which descends at 15°.
#[test]
fn ramp_default_angle_is_15_degrees() {
    assert_approx_eq!(INIT_RAMP_ANGLE_DEG, 15.0, 0.0);
    let p = hole_project();
    let o = op(
        &p,
        Side::Inside,
        Entry::Ramp {
            angle_deg: INIT_RAMP_ANGLE_DEG,
        },
    );
    let (tp, _) = generate(&p, &o);
    let legs = sloped_moves(&tp);
    assert!(!legs.is_empty());
    for (from, mv) in legs {
        let slope = (from.z - mv.to.z)
            .atan2(from.xy().distance_to(mv.to.xy()))
            .to_degrees();
        assert!((slope - 15.0).abs() <= 1e-6, "slope {slope}");
    }
}

/// Under `pass_first` the tool leaves each contour between passes and
/// comes back at safe Z: it goes straight down only through the kerf the
/// previous pass cut, to that pass's floor, and then ramps or helixes
/// into the new pass at the contour start.
#[test]
fn pass_first_entries_start_at_the_contour() {
    let p = project_of(&[
        ("a", vec![circle((30.0, 30.0, 12.0))]),
        ("b", vec![circle((85.0, 85.0, 12.0))]),
    ]);
    for entry in ramped() {
        let mut o = op(&p, Side::Outside, entry.clone());
        o.machining_order = MachiningOrder::PassFirst;
        let (tp, warnings) = generate(&p, &o);
        assert!(fallbacks(&warnings).is_empty(), "{entry:?}: {warnings:?}");
        for z in PASS_Z_MM {
            let arrivals = sloped_moves(&tp)
                .iter()
                .filter(|(_, mv)| (mv.to.z - z).abs() <= EPS_MM)
                .count();
            assert!(arrivals >= 2, "{entry:?}: both contours enter pass {z}");
        }
        for (from, mv) in plunges_into_stock(&tp) {
            assert!(
                [PASS_Z_MM[0], PASS_Z_MM[1]]
                    .iter()
                    .any(|z| (mv.to.z - z).abs() <= EPS_MM),
                "{entry:?}: only down to a floor already cut: {from:?} -> {mv:?}"
            );
        }
    }
}
