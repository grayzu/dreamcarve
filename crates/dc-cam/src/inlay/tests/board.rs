// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #495: a male band that runs past the board's edge is clipped to the
//! board (`docs/specs/phase-3/design.md`, "Algorithms → Inlay", steps 4 and
//! 6).
//!
//! The star of the inlay test part, centred at (32, 50) on 100 × 100 mm
//! stock, so its left tip is 2 mm from the stock's left edge and its 8 mm
//! band runs 6 mm past it. The stock-boundary rule holds a V-bit to its
//! full radius, so a design 2 mm from the edge needs small bits: a 60°
//! V-bit 3.175 mm across (radius 1.5875 mm, cone 2.75 mm), a 1.5 mm end
//! mill, `depth_mm` 2, `glue_gap_mm` 0.5, `plug_depth_mm` 0.6. The V-bit
//! runs along the plug's foot `0.6 · tan 30°` = 0.346 mm out, its edge
//! 1.934 mm out: inside the board.

use super::*;
use crate::stock::StockEdge;

/// How far the star's left tip is from the stock's left edge, mm.
const NEAR_MM: f64 = 2.0;
/// The small V-bit's diameter, mm.
const SMALL_V_BIT_MM: f64 = 3.175;
/// The small end mill's diameter, mm.
const SMALL_END_MILL_MM: f64 = 1.5;

/// The star with its circle, its left tip [`NEAR_MM`] from the left edge
/// of 100 × 100 mm stock, with the bench's bits ([`project_of`]).
fn near_project() -> Project {
    project_of(
        vec![
            polygon(&star_points()),
            Polyline::circle(Point::new(0.0, 0.0), CIRCLE_MM),
        ],
        (30.0 + NEAR_MM, CENTRE.1),
        100.0,
        100.0,
    )
}

/// [`near_project`] with the small bits: `t1` a 60° V-bit
/// [`SMALL_V_BIT_MM`] across, `t2` an end mill [`SMALL_END_MILL_MM`]
/// across.
fn small_bits_project() -> Project {
    with_small_bits(near_project())
}

/// The star with its circle centred at `at` on `width_mm` × `height_mm`
/// stock, with the small bits ([`small_bits_project`]). Each tip is 30 mm
/// from the centre, so a centre 32 mm from an edge leaves a tip
/// [`NEAR_MM`] from it and the 8 mm band 6 mm past it.
fn small_bits_star(at: (f64, f64), width_mm: f64, height_mm: f64) -> Project {
    with_small_bits(project_of(
        vec![
            polygon(&star_points()),
            Polyline::circle(Point::new(0.0, 0.0), CIRCLE_MM),
        ],
        at,
        width_mm,
        height_mm,
    ))
}

/// `p` with the small bits for its tools.
fn with_small_bits(mut p: Project) -> Project {
    p.tools = vec![
        Tool {
            diameter_mm: SMALL_V_BIT_MM,
            ..v_bit(60.0)
        },
        Tool {
            diameter_mm: SMALL_END_MILL_MM,
            name: "1.5 mm end mill".into(),
            ..end_mill()
        },
    ];
    p
}

/// The near star's Inlay for the small bits, side `side`: `depth_mm` 2,
/// `glue_gap_mm` 0.5, `plug_depth_mm` 0.6, margin 8, the end mill's
/// clearing pass, the V-bit's floor at step-over 0.2.
fn small_inlay(side: Board) -> InlayOp {
    InlayOp {
        depth_mm: Some(2.0),
        glue_gap_mm: Some(0.5),
        plug_depth_mm: Some(0.6),
        clearing: vec![clearing_pass()],
        step_over_fraction: Some(0.2),
        ..inlay(side)
    }
}

/// [`small_inlay`]'s male with a cut-out by the end mill, four rect tabs
/// 6 mm wide and 3 mm tall, 0.2 mm through.
fn small_cut_out_male() -> InlayOp {
    InlayOp {
        cut_out: cut_out_male().cut_out,
        ..small_inlay(Board::Male)
    }
}

/// [`small_cut_out_male`] with `placement` for its tab request.
fn small_cut_out_male_with(placement: dc_project::TabPlacement) -> InlayOp {
    let mut op = small_cut_out_male();
    if let Some(tabs) = op.cut_out.as_mut().and_then(|c| c.tabs.as_mut()) {
        tabs.placement = placement;
    }
    op
}

/// The cut-out of `op` in `project` as it is cut (#710): its tool-centre
/// paths, and where the tabs its request places are centred, mm in
/// project space.
fn cut_out_tabs(project: &Project, op: &InlayOp) -> (Vec<Polyline>, Vec<Point>) {
    let band = male_band(&with_op(project, Operation::Inlay(op.clone())), op).expect("a male band");
    let side = side(project, op, Board::Male, &mut Vec::new()).expect("the side resolves");
    let project = &on_board(project, op, Board::Male);
    let cut = op.cut_out.as_ref().expect("a cut-out");
    let diameter_mm = project
        .tools
        .iter()
        .find(|t| t.id == cut.tool)
        .expect("the cut-out's tool")
        .diameter_mm;
    let paths = cut_out_paths(
        &side.band_edge,
        &band,
        diameter_mm,
        band.board(&project.stock),
        project.flatten_tolerance_mm,
    );
    let thickness_mm = project.stock.thickness_mm.expect("a thickness");
    let (copy, profile) = cut_out_profile(project, op, cut, &paths, thickness_mm);
    let tabs = cut.tabs.as_ref().expect("tabs");
    let placed = crate::placed_tabs::place(&copy, &profile.id, tabs).expect("placed");
    let points = placed
        .tabs
        .iter()
        .map(|t| Point::new(t.x_mm, t.y_mm))
        .collect();
    (paths, points)
}

/// Where a Profile that carries `op`'s cut-out's tab request, On line on
/// the closed `paths`, has its tabs placed ([`crate::placed_tabs::place`],
/// a count for each path): what a cut-out placed before #710.
fn requested_tabs(project: &Project, op: &InlayOp, paths: &[Polyline]) -> Vec<Point> {
    let project = &on_board(project, op, Board::Male);
    let cut = op.cut_out.as_ref().expect("a cut-out");
    let thickness_mm = project.stock.thickness_mm.expect("a thickness");
    let (mut copy, mut profile) = cut_out_profile(project, op, cut, paths, thickness_mm);
    copy.tabs.clear();
    profile.tabs.clone_from(&cut.tabs);
    copy.operations = vec![Operation::Profile(profile.clone())];
    let tabs = cut.tabs.as_ref().expect("tabs");
    crate::placed_tabs::place(&copy, &profile.id, tabs)
        .expect("placed")
        .tabs
        .iter()
        .map(|t| Point::new(t.x_mm, t.y_mm))
        .collect()
}

/// How many of `tabs` sit on each of `paths`: each tab counts for the
/// path nearest it.
fn tabs_per_path(paths: &[Polyline], tabs: &[Point]) -> Vec<usize> {
    let mut counts = vec![0; paths.len()];
    for &tab in tabs {
        let (nearest, _) = paths
            .iter()
            .enumerate()
            .map(|(i, path)| (i, distance_to(std::slice::from_ref(path), tab)))
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .expect("a path");
        counts[nearest] += 1;
    }
    counts
}

/// Where the cut-out run of `runs` lifts over a tab: the middle of each
/// stretch it cuts level at `top_z_mm`, the tabs' top, one per place (a
/// tab is lifted over once by every pass below its top). A stretch longer
/// than 20 mm is a pass at that height, not a tab.
fn tab_lifts(runs: &[ToolRun], top_z_mm: f64) -> Vec<Point> {
    let toolpath = &runs
        .iter()
        .find(|r| matches!(r.label, RunLabel::CutOut(_)))
        .expect("a cut-out run")
        .toolpath;
    let mut stretches: Vec<(Point, Point, f64)> = Vec::new();
    let mut on_top = false;
    let mut from: Option<Point3> = None;
    for mv in &toolpath.moves {
        let start = from;
        from = Some(mv.to);
        let Some(a) = start else {
            continue;
        };
        let level = mv.is_cut()
            && (a.z - top_z_mm).abs() < 1e-9
            && (mv.to.z - top_z_mm).abs() < 1e-9
            && a.xy().distance_to(mv.to.xy()) > 1e-9;
        if level {
            let length_mm = a.xy().distance_to(mv.to.xy());
            match stretches.last_mut() {
                Some(last) if on_top => {
                    last.1 = mv.to.xy();
                    last.2 += length_mm;
                }
                _ => stretches.push((a.xy(), mv.to.xy(), length_mm)),
            }
        }
        on_top = level;
    }
    let mut places: Vec<Point> = Vec::new();
    for (a, b, length_mm) in stretches {
        let middle = Point::new(f64::midpoint(a.x, b.x), f64::midpoint(a.y, b.y));
        if length_mm < 20.0 && places.iter().all(|p| p.distance_to(middle) > 1.0) {
            places.push(middle);
        }
    }
    places
}

/// Every sample of every cutting move of `run` ([`cut_samples`]) whose
/// tool, `radius_mm` round it, reaches past the stock's edge by more than
/// `slack_mm`.
pub(super) fn off_stock(
    toolpath: &Toolpath,
    project: &Project,
    radius_mm: f64,
    slack_mm: f64,
) -> Vec<Point3> {
    let (w, h) = (project.stock.width_mm, project.stock.height_mm);
    cut_samples(toolpath)
        .into_iter()
        .filter(|p| {
            p.x - radius_mm < -slack_mm
                || p.y - radius_mm < -slack_mm
                || p.x + radius_mm > w + slack_mm
                || p.y + radius_mm > h + slack_mm
        })
        .collect()
}

/// The diameter of `run`'s tool in `project`, mm.
pub(super) fn diameter_of(run: &ToolRun, project: &Project) -> f64 {
    project
        .tools
        .iter()
        .find(|t| t.id == run.tool)
        .expect("the run's tool")
        .diameter_mm
}

/// The `InlayBandClipped` warnings among `warnings`: edges, open, and
/// narrowest band.
fn clipped_warnings(warnings: &[Warning]) -> Vec<(Vec<StockEdge>, bool, f64)> {
    warnings
        .iter()
        .filter_map(|w| match w {
            Warning::InlayBandClipped {
                edges,
                open,
                narrowest_mm,
                ..
            } => Some((edges.clone(), *open, *narrowest_mm)),
            _ => None,
        })
        .collect()
}

/// #495: the near star's 8 mm band runs 6 mm past the left edge, so it is
/// clipped to the board. The male generates, every run passing
/// `assert_run_safe`; no run's tool (its full radius) leaves the stock;
/// the clearing pass's floor reaches the board's edge, its end mill's
/// centre the radius inside it; and it warns `inlay_band_clipped`, the
/// left edge, an open edge, the band 2 mm wide at its narrowest.
#[test]
fn band_is_clipped_to_the_stock() {
    let project = small_bits_project();
    let op = small_inlay(Board::Male);
    let (runs, warnings) = inlay_runs(&project, &op);
    let tolerance_mm = project.flatten_tolerance_mm;
    for run in &runs {
        let radius_mm = diameter_of(run, &project) / 2.0;
        let off = off_stock(&run.toolpath, &project, radius_mm, EPS_MM);
        assert!(
            off.is_empty(),
            "{:?} leaves the stock at {off:?}",
            run.label
        );
    }
    let clearing = runs
        .iter()
        .find(|r| matches!(r.label, RunLabel::Clearing(_)))
        .expect("a clearing run");
    let leftmost = cut_samples(&clearing.toolpath)
        .iter()
        .map(|p| p.x)
        .fold(f64::INFINITY, f64::min);
    assert!(
        (leftmost - SMALL_END_MILL_MM / 2.0).abs() <= tolerance_mm,
        "the floor reaches the board's edge: the end mill runs {leftmost} mm from it"
    );
    let clipped = clipped_warnings(&warnings);
    assert_eq!(clipped.len(), 1, "{warnings:?}");
    let (edges, open, narrowest_mm) = &clipped[0];
    assert_eq!(edges, &[StockEdge::MinX]);
    assert!(open);
    assert!((narrowest_mm - NEAR_MM).abs() < 1e-6, "{narrowest_mm}");
}

/// #495: with a V wall (no clearing pass) the band is clipped the V-bit's
/// radius inside the board, so the V-bit, which runs down the wall to its
/// top, keeps its whole radius on it; the band is `2 − 1.5875` mm wide
/// at its narrowest.
#[test]
fn v_wall_band_is_clipped_inside_the_board() {
    let project = small_bits_project();
    let op = InlayOp {
        clearing: Vec::new(),
        ..small_inlay(Board::Male)
    };
    let (runs, warnings) = inlay_runs(&project, &op);
    let off = off_stock(main_run(&runs), &project, SMALL_V_BIT_MM / 2.0, EPS_MM);
    assert!(off.is_empty(), "the V-bit leaves the stock at {off:?}");
    let clipped = clipped_warnings(&warnings);
    assert_eq!(clipped.len(), 1, "{warnings:?}");
    let (edges, open, narrowest_mm) = &clipped[0];
    assert_eq!(edges, &[StockEdge::MinX]);
    assert!(!open);
    assert!(
        (narrowest_mm - (NEAR_MM - SMALL_V_BIT_MM / 2.0)).abs() < 1e-6,
        "{narrowest_mm}"
    );
}

/// #495: with a cut-out the band's clipped edge frees the plug at the
/// board's edge. The cut-out's paths are open, each from the board's edge
/// to the board's edge, their ends the end mill's radius inside the left
/// edge; no cut of the run, its tool's full radius, leaves the stock; and
/// every tab its request places sits on one of those paths.
#[test]
fn cut_out_runs_edge_to_edge_when_clipped() {
    let project = small_bits_project();
    let op = small_cut_out_male();
    let tolerance_mm = project.flatten_tolerance_mm;
    let radius_mm = SMALL_END_MILL_MM / 2.0;
    let band =
        male_band(&with_op(&project, Operation::Inlay(op.clone())), &op).expect("a male band");
    assert!(band.is_open());
    let side = side(&project, &op, board_of(&op), &mut Vec::new()).expect("the side resolves");
    let paths = cut_out_paths(
        &side.band_edge,
        &band,
        SMALL_END_MILL_MM,
        band.board(&project.stock),
        tolerance_mm,
    );
    assert!(!paths.is_empty());
    for path in &paths {
        assert!(
            !path.is_closed(),
            "the clipped band's path is open: {path:?}"
        );
        let ends = [
            path.vertices().first().expect("a start"),
            path.vertices().last().expect("an end"),
        ];
        for end in ends {
            assert!(
                (end.x - radius_mm).abs() <= tolerance_mm,
                "an end is the radius inside the left edge: {end:?}"
            );
        }
        for v in path.vertices() {
            assert!(v.x >= radius_mm - tolerance_mm, "{v:?} is off the stock");
        }
    }

    let (runs, _) = inlay_runs(&project, &op);
    let cut_out = runs
        .iter()
        .find(|r| matches!(r.label, RunLabel::CutOut(_)))
        .expect("a cut-out run");
    let off = off_stock(&cut_out.toolpath, &project, radius_mm, EPS_MM);
    assert!(off.is_empty(), "the cut-out leaves the stock at {off:?}");
    for p in cut_samples(&cut_out.toolpath) {
        let gap = distance_to(&paths, p.xy());
        assert!(
            gap <= tolerance_mm,
            "{p:?} is {gap} mm off the cut-out's paths"
        );
    }

    let thickness_mm = project.stock.thickness_mm.expect("a thickness");
    let cut = op.cut_out.as_ref().expect("a cut-out");
    let (copy, profile) = cut_out_profile(&project, &op, cut, &paths, thickness_mm);
    let tabs = cut.tabs.as_ref().expect("tabs");
    let placed = crate::placed_tabs::place(&copy, &profile.id, tabs).expect("placed");
    // #710: the request's four, on the one piece the left edge leaves.
    assert_eq!(paths.len(), 1, "one edge clips the band into one piece");
    assert_eq!(placed.tabs.len(), 4, "the four tabs asked for");
    for tab in &placed.tabs {
        let at = Point::new(tab.x_mm, tab.y_mm);
        let gap = distance_to(&paths, at);
        assert!(
            gap <= TAB_WIDTH_MM,
            "a tab at {at:?} is {gap} mm off the paths"
        );
        assert!(at.x >= radius_mm, "a tab at {at:?} is off the stock");
    }
}

/// #710: a cut-out is one cut, however many pieces the board's edge leaves
/// of it. The star 2 mm from the left, bottom and top edges of 100 × 64 mm
/// stock: its band is clipped at all three, so its cut-out is three open
/// paths, and a request for four tabs places four on the cut-out as a
/// whole (before, four were asked of each piece). Each is on one of the
/// paths and on the stock, and the run lifts over those four places and
/// no other.
#[test]
fn clipped_cut_out_places_the_tabs_it_was_asked_for() {
    let project = small_bits_star((30.0 + NEAR_MM, 30.0 + NEAR_MM), 100.0, 64.0);
    let op = small_cut_out_male();
    let tolerance_mm = project.flatten_tolerance_mm;
    let radius_mm = SMALL_END_MILL_MM / 2.0;
    let (paths, tabs) = cut_out_tabs(&project, &op);
    assert!(paths.len() >= 3, "{} pieces", paths.len());
    assert!(paths.iter().all(|p| !p.is_closed()), "open pieces");
    assert_eq!(tabs.len(), 4, "the four tabs asked for: {tabs:?}");
    let (w, h) = (project.stock.width_mm, project.stock.height_mm);
    for &tab in &tabs {
        let gap = distance_to(&paths, tab);
        assert!(gap <= tolerance_mm, "a tab at {tab:?} is {gap} mm off");
        assert!(
            tab.x >= radius_mm
                && tab.x <= w - radius_mm
                && tab.y >= radius_mm
                && tab.y <= h - radius_mm,
            "a tab at {tab:?} is off the stock"
        );
    }
    let (runs, _) = inlay_runs(&project, &op);
    let lifts = tab_lifts(&runs, TAB_TOP_MM);
    assert_eq!(lifts.len(), 4, "the run lifts over four tabs: {lifts:?}");
    for lift in &lifts {
        assert!(
            tabs.iter().any(|t| t.distance_to(*lift) <= TAB_WIDTH_MM),
            "the run lifts at {lift:?}, where no tab is placed: {tabs:?}"
        );
    }
}

/// #710: the pieces share the count by length. The star 2 mm from the
/// left and bottom edges of 100 × 100 mm stock: its cut-out is two open
/// paths, a short one between those two tips and a long one round the
/// other three quarters. Of four tabs the long one holds more than the
/// short one, and they hold four between them.
#[test]
fn clipped_cut_out_shares_its_tabs_by_length() {
    let project = small_bits_star((30.0 + NEAR_MM, 30.0 + NEAR_MM), 100.0, 100.0);
    let op = small_cut_out_male();
    let (paths, tabs) = cut_out_tabs(&project, &op);
    assert_eq!(paths.len(), 2, "two pieces");
    let lengths: Vec<f64> = paths.iter().map(Polyline::length_mm).collect();
    let (short, long) = if lengths[0] < lengths[1] {
        (0, 1)
    } else {
        (1, 0)
    };
    assert!(
        lengths[long] > 2.0 * lengths[short],
        "clearly different lengths: {lengths:?}"
    );
    let counts = tabs_per_path(&paths, &tabs);
    assert_eq!(counts.iter().sum::<usize>(), 4, "{counts:?}");
    assert!(counts[long] > counts[short], "{counts:?} on {lengths:?}");
    let (runs, _) = inlay_runs(&project, &op);
    assert_eq!(tab_lifts(&runs, TAB_TOP_MM).len(), 4);
}

/// #710: tabs at distances are placed once each too, the distances
/// counted along the pieces end to end: two distances on the three-piece
/// cut-out place two tabs (before, two on each piece).
#[test]
fn clipped_cut_out_places_tabs_at_distances_once() {
    let project = small_bits_star((30.0 + NEAR_MM, 30.0 + NEAR_MM), 100.0, 64.0);
    let op = small_cut_out_male_with(dc_project::TabPlacement::AtDistances {
        distances_mm: vec![15.0, 120.0],
    });
    let (paths, tabs) = cut_out_tabs(&project, &op);
    assert!(paths.len() >= 3, "{} pieces", paths.len());
    assert_eq!(tabs.len(), 2, "one tab for each distance: {tabs:?}");
    let (runs, _) = inlay_runs(&project, &op);
    assert_eq!(tab_lifts(&runs, TAB_TOP_MM).len(), 2);
}

/// #710: a cut-out the board does not clip places what it placed before:
/// the tabs `placed_tabs::place` lays for its request on its closed
/// path, the same count at the same points. The star's (V wall and open
/// edge) and the tutorial's "DC".
#[test]
fn unclipped_cut_out_tabs_are_unchanged() {
    let (dc, dc_op) = fixture(include_str!("../../../../../tests/fixtures/dc.dcv"));
    let star = star_project(THICKNESS_MM);
    let open = InlayOp {
        clearing: vec![clearing_pass()],
        ..cut_out_male()
    };
    let cases = [
        ("DC", dc, dc_op),
        ("star open", star.clone(), open),
        ("star V wall", star, cut_out_male()),
    ];
    for (name, project, op) in cases {
        let (paths, tabs) = cut_out_tabs(&project, &op);
        assert!(paths.iter().all(Polyline::is_closed), "{name}");
        let before = requested_tabs(&project, &op, &paths);
        assert_eq!(tabs.len(), before.len(), "{name}: {tabs:?} for {before:?}");
        assert_eq!(tabs.len(), 4 * paths.len(), "{name}: four on each outline");
        for (tab, was) in tabs.iter().zip(&before) {
            assert!(
                tab.distance_to(*was) <= 1e-9,
                "{name}: a tab at {tab:?} was at {was:?}"
            );
        }
    }
}

/// #495: the near star's female and its clipped male seat within
/// [`SEAT_MM`] (0.1 mm grid), with the cut-out.
#[test]
fn clipped_male_still_seats() {
    let project = small_bits_project();
    let fit = fit_of(&project, &small_cut_out_male(), 0.1);
    println!(
        "the plug stands {:.3} mm proud at {:?}; the male's share {:.3} mm at {:?}",
        fit.seat_mm, fit.seat_at, fit.male_mm, fit.male_at
    );
    assert!(
        fit.is_flush(),
        "the plug stands {:.3} mm proud at {:?} (the male's share {:.3} mm at {:?})",
        fit.seat_mm,
        fit.seat_at,
        fit.male_mm,
        fit.male_at
    );
}

/// #495: with the bench's bits (the 60° V-bit 6.35 mm across and the
/// 3.175 mm end mill) and the part's values, the V-bit along the plug's
/// foot, `2 · tan 30°` = 1.155 mm out, would reach `1.155 + 3.175` =
/// 4.330 mm out, past the left edge 2 mm away: `InlayTooNearEdge`, naming
/// the edge, the distance, the 4.330 mm needed, and the stock that fits
/// the 60 mm star centred, `60 + 2 · 4.330` mm wide. The design's own cut
/// is never clipped.
#[test]
fn design_too_near_the_edge_is_explained() {
    let project = near_project();
    let op = InlayOp {
        clearing: vec![clearing_pass()],
        ..inlay(Board::Male)
    };
    let needed_mm = PLUG_DEPTH_MM * slope() + 6.35 / 2.0;
    let err = try_inlay(&project, &op).expect_err("too near the edge");
    let CamError::InlayTooNearEdge {
        edge,
        distance_mm,
        needed_mm: got_mm,
        stock_mm,
        ..
    } = err.clone()
    else {
        panic!("{err:?}");
    };
    assert_eq!(edge, StockEdge::MinX);
    assert!((distance_mm - NEAR_MM).abs() < 1e-9, "{distance_mm}");
    assert!((got_mm - needed_mm).abs() < 1e-9, "{got_mm}");
    assert!(
        (stock_mm - (60.0 + 2.0 * needed_mm)).abs() < 1e-6,
        "{stock_mm}"
    );
    let message = err.to_string();
    for part in [
        "-X (left) edge",
        "2.000 mm",
        "4.330 mm",
        "2.330 mm",
        "68.660 mm wide",
    ] {
        assert!(message.contains(part), "{part:?} in {message}");
    }
    // Moved 2.33 mm right it fits.
    let mut moved = project.clone();
    moved.instances[0].transform = Transform::translate(30.0 + needed_mm, CENTRE.1);
    let (_, warnings) = inlay_runs(&moved, &op);
    assert_eq!(clipped_warnings(&warnings).len(), 1, "{warnings:?}");
}

/// #495: a band that stays on the board is unchanged: the side carves
/// what it carved before, the surround of the design and, with an open
/// edge, the band `B` itself, nothing is clipped, and every run is the
/// same on stock ten times the size. The tutorial's "DC" male (open,
/// rounded, cut-out) and the star's (open and V wall, with a cut-out).
#[test]
fn unclipped_band_is_unchanged() {
    let (dc, dc_op) = fixture(include_str!("../../../../../tests/fixtures/dc.dcv"));
    let star = star_project(THICKNESS_MM);
    let open = InlayOp {
        clearing: vec![clearing_pass()],
        ..cut_out_male()
    };
    let cases = [
        ("DC", dc, dc_op),
        ("star open", star.clone(), open),
        ("star V wall", star, cut_out_male()),
    ];
    for (name, project, op) in cases {
        let p = with_op(&project, Operation::Inlay(op.clone()));
        let side = side(&p, &op, board_of(&op), &mut Vec::new()).expect("the side resolves");
        assert_eq!(side.clipped, None, "{name}");
        let band = male_band(&p, &op).expect("a male band");
        let tolerance_mm = p.flatten_tolerance_mm;
        assert_eq!(
            side.targets.regions,
            surround(
                &side.design,
                band.margin_mm,
                band.edge_rounding_mm,
                tolerance_mm
            ),
            "{name}"
        );
        if let Some(edge) = &side.open_edge {
            assert_eq!(
                edge.band,
                rounded_band(
                    &side.design,
                    band.margin_mm,
                    band.edge_rounding_mm,
                    tolerance_mm
                ),
                "{name}"
            );
        }
        let (runs, warnings) = inlay_runs(&project, &op);
        assert!(clipped_warnings(&warnings).is_empty(), "{name}");
        let mut big = project.clone();
        big.stock.width_mm *= 10.0;
        big.stock.height_mm *= 10.0;
        let (big_runs, _) = inlay_runs(&big, &op);
        assert_eq!(runs, big_runs, "{name}");
    }
}
