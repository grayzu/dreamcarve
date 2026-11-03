// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #484: the #464 stress sweep's seat failures with the 60° and 90° bits,
//! one minimal design per cause. Each pair is held to [`SEAT_MM`] on a
//! 0.1 mm grid, as [`plug_seats_flush`] holds its own.

use super::*;

/// The pair's female and male of `project`, each `side(…)`, seat within
/// [`SEAT_MM`]: the male's share and the whole seat.
fn assert_seats(name: &str, project: &Project, side: impl Fn(InlaySide) -> InlayOp) {
    let fit = fit_of(
        project,
        &side(InlaySide::Female),
        project,
        &side(InlaySide::Male),
        0.1,
    );
    println!(
        "{name}: the plug's start plane stops {:.3} mm above the female at {:?}; the male's share {:.3} mm at {:?}",
        fit.seat_mm, fit.seat_at, fit.male_mm, fit.male_at
    );
    assert!(
        fit.male_mm <= SEAT_MM,
        "{name}: the male stands {:.3} mm proud of the female's design at {:?}",
        fit.male_mm,
        fit.male_at
    );
    assert!(
        fit.seat_mm <= SEAT_MM,
        "{name}: the plug stands {:.3} mm proud, binding at {:?}",
        fit.seat_mm,
        fit.seat_at
    );
}

/// The star part's Inlay, the V-bit's floor at the step-over the user
/// guide asks for, 0.1.
fn fine(side: InlaySide) -> InlayOp {
    InlayOp {
        step_over_fraction: Some(0.1),
        ..inlay(side)
    }
}

/// A regular `n`-gon of circumradius `radius_mm` about the origin.
fn regular(n: u32, radius_mm: f64) -> Polyline {
    let points: Vec<(f64, f64)> = (0..n)
        .map(|k| {
            let a = std::f64::consts::TAU * f64::from(k) / f64::from(n);
            (radius_mm * a.cos(), radius_mm * a.sin())
        })
        .collect();
    polygon(&points)
}

/// #484 (line art `0x70`, `0x39` and eight more): a regular heptagon 8 mm
/// across, whose corners turn 51.4°. Its plug seats. A corner turning
/// more than 50° ([`dc_geometry::medial::CORNER_ANGLE_RAD`]) is sharp and
/// gets a tip, but one turning less than 60° has its two walls' contacts
/// closer than the contact-spread filter keeps, all along its branch but
/// for a stub at the tip; the tip joined the stub, and the branch from it
/// to the rest of the axis was missing. The heptagon's axis was four
/// nodes, and its female and male cut nothing between the stubs.
#[test]
fn heptagon_plug_seats_flush() {
    let project = project_of(vec![regular(7, 4.0)], CENTRE, 100.0, 100.0);
    assert_seats("heptagon", &project, fine);
}

/// Line art `0x4a`'s slot, centred on the origin: 7.9 mm between its
/// ends' centres, ends of radius 0.526 mm drawn as half circles, turned
/// 41.9°.
fn slot_0x4a() -> Polyline {
    let (cx, cy) = (32.6, 45.5);
    let v = |x: f64, y: f64, bulge: f64| Vertex {
        bulge,
        ..Vertex::line(x - cx, y - cy)
    };
    Polyline::new(
        vec![
            v(30.015_660, 42.509_001, 0.0),
            v(35.884_782, 47.775_436, 1.0),
            v(35.181_859, 48.558_802, 0.0),
            v(29.312_737, 43.292_366, 1.0),
        ],
        true,
    )
}

/// #484 (line art `0x4a`, `0xc6`): the sweep's slot, with its pair's
/// values (`D` 2.74, `G` 0.469, `P` 2.292, margin 3.53), narrower than
/// the groove at the floor (`D · tan 30°` = 1.58 mm), so the V-bit
/// carves it from its axis alone. Its plug seats. The axis keeps its
/// clearance to each end's centre, so spur pruning read its end as a
/// spur and unzipped it 32 nodes back, and no tip came back: the end's
/// flattened vertices lay off every bisector a tip could follow (here
/// none lies on the slot's axis), beyond every disc. The female's groove
/// stopped 1.2 mm short of each end, and the plug stood 0.91 mm proud.
#[test]
fn slot_plug_seats_flush() {
    let project = project_of(vec![slot_0x4a()], CENTRE, 100.0, 100.0);
    assert_seats("slot", &project, |side| InlayOp {
        depth_mm: Some(2.74),
        glue_gap_mm: Some(0.469),
        plug_depth_mm: Some(2.292),
        margin_mm: Some(3.53),
        ..fine(side)
    });
}

/// #484 (lettering `0x9`, `0x61`): two 16 mm squares joined by a bar
/// 5.5 mm wide. The female's floor in the bar is 0.88 mm wide, under two
/// step-overs (`0.1 · 2 · D · tan 30°` = 0.46 mm), between the squares'
/// floors, so the floor's inset by one step-over is noise in the bar and
/// not in the squares. The glue gap is `0.15 · D`, over the floor's
/// ridges (`0.1 · D`). Its plug seats. Before, a centre ring went only
/// inside an innermost ring, and the boundary trace, which has the
/// squares' rings inside it, left the bar's middle a whole step-over from
/// any groove: a ridge twice the reported one held the plug 0.3 mm up.
#[test]
fn dumbbell_floor_seats_flush() {
    let bar = 2.75;
    let project = project_of(
        vec![polygon(&[
            (-21.0, -8.0),
            (-5.0, -8.0),
            (-5.0, -bar),
            (5.0, -bar),
            (5.0, -8.0),
            (21.0, -8.0),
            (21.0, 8.0),
            (5.0, 8.0),
            (5.0, bar),
            (-5.0, bar),
            (-5.0, 8.0),
            (-21.0, 8.0),
        ])],
        CENTRE,
        100.0,
        100.0,
    );
    assert_seats("dumbbell", &project, |side| InlayOp {
        glue_gap_mm: Some(0.15 * DEPTH_MM),
        ..fine(side)
    });
}

/// #484 (lettering `0x1e`, `0x62`, `0xc1`, line art `0x59`, `0x8e`):
/// eight 3 mm squares on a circle of radius 10 mm, margin 3 mm, with a V
/// wall. Its plug seats. The margins ring round the middle without
/// covering it, so the band, their union, had a hole 11 mm across: board
/// the male never cut, which stood to the stock top inside the band's
/// floor and held the plug 3 mm off the female's surface.
#[test]
fn band_round_a_space_seats_flush() {
    let square = |x: f64, y: f64| {
        polygon(&[
            (x - 1.5, y - 1.5),
            (x + 1.5, y - 1.5),
            (x + 1.5, y + 1.5),
            (x - 1.5, y + 1.5),
        ])
    };
    let parts: Vec<Polyline> = (0..8)
        .map(|k| {
            let a = std::f64::consts::TAU * f64::from(k) / 8.0;
            square(10.0 * a.cos(), 10.0 * a.sin())
        })
        .collect();
    let project = project_of(parts, CENTRE, 100.0, 100.0);
    assert_seats("parts round a space", &project, |side| InlayOp {
        margin_mm: Some(3.0),
        ..fine(side)
    });
}

/// An L 3 mm thick, 16 mm tall and 10 mm wide, its inside corner filleted
/// with radius `fillet_mm` (an arc of bulge `tan(22.5°)`), its outside
/// corner at the origin.
fn filleted_l(fillet_mm: f64) -> Polyline {
    let t = 3.0;
    Polyline::new(
        vec![
            Vertex::line(0.0, 0.0),
            Vertex::line(10.0, 0.0),
            Vertex::line(10.0, t),
            Vertex {
                bulge: -(std::f64::consts::PI / 8.0).tan(),
                ..Vertex::line(t + fillet_mm, t)
            },
            Vertex::line(t, t + fillet_mm),
            Vertex::line(t, 16.0),
            Vertex::line(0.0, 16.0),
        ],
        true,
    )
}

/// #484 (lettering `0x71` and ten more): an L with a 0.3 mm fillet inside
/// its corner and a 4 mm square 1.5 mm off the corner, margin 20 mm, so
/// the band is sampled every 0.31 mm. Its plug seats. Between the L and
/// the square the band's axis has a short branch into the fillet, which
/// redundant-leaf pruning dropped at upstream's tolerance, `0.75` steps
/// (0.23 mm): its discs reached no farther than the junction's but for
/// that, and the male's groove stopped up to that short of the plug's
/// wall, which stood 0.42 mm proud. The tolerance is now at most the
/// carved edge's ([`vcarve::medial::EDGE_MM`]), as the lettering's
/// between an "m" and an "h".
#[test]
fn fillet_in_a_wide_band_seats_flush() {
    let c = 4.5;
    let project = project_of(
        vec![
            filleted_l(0.3),
            polygon(&[(c, c), (c + 4.0, c), (c + 4.0, c + 4.0), (c, c + 4.0)]),
        ],
        CENTRE,
        100.0,
        100.0,
    );
    assert_seats("filleted L", &project, |side| InlayOp {
        margin_mm: Some(20.0),
        ..fine(side)
    });
}

/// #511 (line art `0x59`): a regular octagon 8 mm across, whose corners
/// turn 45°, under the 50° a sharp corner turns. Its plug seats. The
/// contact-spread filter drops every spoke, so the axis is one node with
/// no edge, and a blunt corner's tip joined no node that had none: the
/// carve was a plunge at the middle, and the corners stood short.
#[test]
fn octagon_plug_seats_flush() {
    let project = project_of(vec![regular(8, 4.0)], CENTRE, 100.0, 100.0);
    assert_seats("octagon", &project, fine);
}

/// #511 (lettering `0x6f`): a 10 mm square with a slit 3 µm wide and 2 mm
/// long, in an inch program (half a unit in the last written place is
/// 0.0018 mm). Its plug seats, and both sides warn `inlay_hole_filled`.
/// No V-bit carves such a sliver: the female left a wall to the stock top
/// round it, the male's groove keeps half a written unit from each wall
/// and was dropped, and the plug stood 3 mm proud. A hole narrower than
/// the merge distance everywhere is filled, as near parts are merged.
#[test]
fn hairline_hole_is_filled_and_seats() {
    let mut project = project_of(
        vec![
            polygon(&[(-5.0, -5.0), (5.0, -5.0), (5.0, 5.0), (-5.0, 5.0)]),
            polygon(&[
                (-1.0, -0.0015),
                (1.0, -0.0015),
                (1.0, 0.0015),
                (-1.0, 0.0015),
            ]),
        ],
        CENTRE,
        100.0,
        100.0,
    );
    project.display_units = dc_project::Units::Inch;
    assert_seats("hairline hole", &project, fine);
    for side in [InlaySide::Female, InlaySide::Male] {
        let (_, warnings) = inlay_runs(&project, &fine(side));
        assert!(
            warnings
                .iter()
                .any(|w| w.code().as_str() == "inlay_hole_filled"),
            "{side:?}: {warnings:?}"
        );
    }
}

/// #511 (line art `0x17`): a 10 mm square with a hole of radius 2 mm 3 µm
/// from its right side, so the design pinches to 3 µm between them. Its
/// plug seats. Either side of the pinch the region is a wedge between the
/// side and the circle, narrower than the medial step for a millimetre or
/// more; its axis ran there as a chain whose clearance falls slower than
/// spur pruning asks, which unzipped it, and the female's groove stopped
/// that far short of the pinch.
#[test]
fn pinched_design_seats_flush() {
    let project = project_of(
        vec![
            polygon(&[(-5.0, -5.0), (5.0, -5.0), (5.0, 5.0), (-5.0, 5.0)]),
            Polyline::circle(Point::new(5.0 - 0.003 - 2.0, 0.0), 2.0),
        ],
        CENTRE,
        100.0,
        100.0,
    );
    assert_seats("pinch", &project, fine);
}

/// A pentagon whose corner at the origin is 125° (it turns 55°), as line
/// art `0x34`'s: walls 6 mm long at 62.5° either side of its bisector,
/// which points along −X, closed by a 12 mm wide end. Its other corners
/// are 117.5° and 90°.
fn obtuse_corner() -> Polyline {
    let (sin, cos) = 62.5_f64.to_radians().sin_cos();
    polygon(&[
        (0.0, 0.0),
        (-6.0 * cos, 6.0 * sin),
        (-12.0, 6.0 * sin),
        (-12.0, -6.0 * sin),
        (-6.0 * cos, -6.0 * sin),
    ])
}

/// #500 (line art `0x34`): the female's groove into a 125° corner runs
/// from the corner's tip along its bisector to the floor, and the plug
/// seats within [`SEAT_MM`] with the 15° bit (depths scaled to its cone
/// as the star's: `D` 4, `G` 1, `P` 1.6). Along the bisector, every
/// point's V surface, `s · sin 62.5° / tan 7.5°` deep at `s` from the
/// tip, is cut to within 0.01 mm sideways, up to where the floor (inset
/// `D · tan 7.5°`) begins. The corner turns between 50° and 60°: sharp,
/// so it gets a tip, but its walls' contacts are closer than the
/// contact-spread filter keeps all along its branch but for a stub at the
/// tip, and the tip joined the stub, so the bisector from the stub to the
/// axis was never cut: on line art `0x34`, 0.31 mm of it, which left the
/// female 0.025 mm shallow sideways at every bit and the plug 0.188 mm
/// proud with the 15° bit. #484's heptagon is the same cause with the
/// 60° bit.
#[test]
fn obtuse_corner_groove_runs_to_the_floor() {
    let mut project = project_of(vec![obtuse_corner()], CENTRE, 100.0, 100.0);
    project.tools[0] = v_bit(15.0);
    let slope = 7.5_f64.to_radians().tan();
    let half_sin = 62.5_f64.to_radians().sin();
    let side = |side| InlayOp {
        depth_mm: Some(4.0),
        glue_gap_mm: Some(1.0),
        plug_depth_mm: Some(1.6),
        ..fine(side)
    };
    let (runs, _) = inlay_runs(&project, &side(InlaySide::Female));
    let floor_s_mm = 4.0 * slope / half_sin;
    let mut short = Vec::new();
    let mut s = 0.02;
    while s < floor_s_mm {
        let q = Point::new(CENTRE.0 - s, CENTRE.1);
        let true_z_mm = THICKNESS_MM - s * half_sin / slope;
        let carved_z_mm = crate::heightfield::carved_z_mm(main_run(&runs), q, slope);
        let short_mm = (carved_z_mm - true_z_mm) * slope;
        if short_mm > 0.01 {
            short.push(format!("{s:.2} mm in: {short_mm:.4} mm short"));
        }
        s += 0.02;
    }
    assert!(
        short.is_empty(),
        "the groove stops short of the bisector: {short:#?}"
    );
    assert_seats("125° corner, 15° bit", &project, side);
}
