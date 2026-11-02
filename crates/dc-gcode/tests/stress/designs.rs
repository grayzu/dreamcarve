// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Difficult but valid line-art designs (#464), deterministic from a seed.
//!
//! A design is a few motifs side by side, each drawn in its own box and
//! turned by a random angle, the boxes a ridge apart (0.1 to 3 mm), or a
//! near touch (0.001 to 0.05 mm). The motifs are the shapes the 3.3
//! bench check and its follow-ups found bugs in:
//!
//! - **strokes:** line art, a square less thickened polylines and rings
//!   0.1 to 3 mm wide, some crossing it, some ending inside a region,
//!   butt, round or pointed (3° to 30°): ridges, small enclosed regions,
//!   dangling line ends (#452, #463, #465, #469);
//! - **spiky:** a polygon with acute spikes and notches from 3° to 30°,
//!   tilted thin strips standing into it (line ends, butt, round or
//!   pointed), arcs, corners up to 175°, and holes inside holes in its
//!   middle, the first sometimes 0.001 to 0.05 mm from a notch's tip
//!   (#452, #456, #469);
//! - **mesh:** many small regions, 0.5 to 2.5 mm across, 0.1 to 1 mm
//!   apart, as rectangles, triangles, diamonds and circles, sometimes
//!   holes of one frame with islands in them (#466, #475);
//! - **near:** two shapes whose edges, corner, curve or hole come 0.001 to
//!   0.05 mm from each other;
//! - **bulgy:** circles inside circles, slots, rounded rectangles and
//!   crescents, drawn with arcs (bulges).
//!
//! Every outline is closed and crosses neither itself nor another: a
//! motif whose outlines would (a boolean can leave a ring touching
//! itself) is drawn again from the next draw of its seed, so a design is
//! still a function of its seed. [`valid`] is the test; `check` accepts
//! what it accepts.

use super::rng::Rng;
use dc_geometry::{
    Contour, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Polyline, Rect, Region, Transform, Vertex,
    difference, nest,
};
use std::f64::consts::{PI, TAU};

/// A design: its outlines in design space and what was drawn, to name a
/// failing case.
#[derive(Debug, Clone)]
pub struct Design {
    /// The outlines.
    pub profiles: Vec<Polyline>,
    /// One line per motif, with its random values.
    pub notes: Vec<String>,
}

/// One motif, in its own frame.
struct Motif {
    profiles: Vec<Polyline>,
    note: String,
}

/// The line-art design of `seed`: two to four motifs.
pub fn line_art(seed: u64) -> Design {
    line_art_keeping(seed, &|_| true)
}

/// [`line_art`]'s first two motifs, for the CI tests' budget.
pub fn line_art_small(seed: u64) -> Design {
    line_art_keeping(seed, &|k| k < 2)
}

/// [`line_art`] with only the motifs whose places `keep` accepts, each as
/// it is in the whole design, laid out in turn: a failing design's motifs
/// one at a time, or two side by side, find the one that fails.
pub fn line_art_keeping(seed: u64, keep: &dyn Fn(usize) -> bool) -> Design {
    let mut rng = Rng::new(seed);
    let count = rng.int(2, 4);
    let mut motifs = Vec::new();
    for _ in 0..count {
        let mut fork = rng.fork();
        motifs.push(motif(&mut fork));
    }
    lay_out(&mut rng, motifs, keep)
}

/// One motif of a random kind, valid ([`valid`]): redrawn from the next
/// draw until it is.
fn motif(rng: &mut Rng) -> Motif {
    for _ in 0..32 {
        let mut fork = rng.fork();
        let kind = fork.int(0, 4);
        let m = match kind {
            0 => strokes(&mut fork),
            1 => spiky(&mut fork),
            2 => mesh(&mut fork),
            3 => near(&mut fork),
            _ => bulgy(&mut fork),
        };
        if !m.profiles.is_empty() && valid(&m.profiles) {
            return m;
        }
    }
    // A plain square, which is always valid; 32 failures in a row would
    // mean the motifs above are wrong, which `designs_are_valid` reports.
    Motif {
        profiles: vec![rect(0.0, 0.0, 4.0, 4.0)],
        note: "square (fallback)".into(),
    }
}

/// Whether `profiles` are a valid design: each a closed contour that
/// crosses neither itself nor any other, none repeated.
pub fn valid(profiles: &[Polyline]) -> bool {
    let contours: Result<Vec<Contour>, _> = profiles.iter().cloned().map(Contour::new).collect();
    let Ok(contours) = contours else {
        return false;
    };
    match nest(contours) {
        Ok((_, warnings)) => warnings.is_empty(),
        Err(_) => false,
    }
}

/// `motifs` each turned by a random angle about its box's centre and laid
/// out in rows of up to two, a random ridge or near touch between boxes.
fn lay_out(rng: &mut Rng, motifs: Vec<Motif>, keep: &dyn Fn(usize) -> bool) -> Design {
    let mut profiles = Vec::new();
    let mut notes = Vec::new();
    let mut x_mm = 0.0;
    let mut y_mm = 0.0;
    let mut row_top_mm: f64 = 0.0;
    let mut k = 0;
    for (place, motif) in motifs.into_iter().enumerate() {
        let angle = rng.range(0.0, TAU);
        let turned: Vec<Polyline> = motif
            .profiles
            .iter()
            .map(|p| transformed(p, Transform::rotate(angle)))
            .collect();
        let b = bounds(&turned);
        let gap_mm = gap(rng);
        if !keep(place) {
            continue;
        }
        if k == 2 {
            x_mm = 0.0;
            y_mm = row_top_mm + gap_mm;
        } else if k > 0 {
            x_mm += gap_mm;
        }
        let shift = Transform::translate(x_mm - b.min.x, y_mm - b.min.y);
        profiles.extend(turned.iter().map(|p| transformed(p, shift)));
        x_mm += b.max.x - b.min.x;
        row_top_mm = row_top_mm.max(y_mm + b.max.y - b.min.y);
        notes.push(format!(
            "{} (turned {:.1}°, then {gap_mm:.4} mm on)",
            motif.note,
            angle.to_degrees()
        ));
        k += 1;
    }
    Design { profiles, notes }
}

/// The space between two motifs' boxes: a ridge 0.1 to 3 mm, or one time
/// in five a near touch 0.001 to 0.05 mm.
fn gap(rng: &mut Rng) -> f64 {
    if rng.chance(0.2) {
        rng.log_range(0.001, 0.05)
    } else {
        rng.log_range(0.1, 3.0)
    }
}

// Line art.

/// How a stroke ends.
#[derive(Debug, Clone, Copy)]
enum End {
    Butt,
    Round,
    /// To a point of this included angle, degrees.
    Pointed(f64),
}

impl End {
    fn random(rng: &mut Rng) -> Self {
        match rng.int(0, 2) {
            0 => Self::Butt,
            1 => Self::Round,
            _ => Self::Pointed(rng.log_range(3.0, 30.0)),
        }
    }
}

/// A stroke width, 0.1 to 3 mm, small ones as likely as large.
fn width(rng: &mut Rng) -> f64 {
    rng.log_range(0.1, 3.0)
}

/// Line art: an `S` mm square less one to three strokes across it, one to
/// three that end inside it, and up to two rings.
fn strokes(rng: &mut Rng) -> Motif {
    let s = rng.range(6.0, 16.0);
    let mut pieces = Vec::new();
    let mut note = format!("strokes {s:.2} mm:");
    let crossing = rng.int(1, 3);
    for _ in 0..crossing {
        let a = outside_point(rng, s);
        let m = Point::new(rng.range(0.1, 0.9) * s, rng.range(0.1, 0.9) * s);
        let b = outside_point(rng, s);
        let w = width(rng);
        note += &format!(" across w {w:.3};");
        pieces.extend(stroke(&[a, m, b], w, End::Butt));
    }
    for _ in 0..rng.int(1, 3) {
        let a = outside_point(rng, s);
        let mut path = vec![a];
        if rng.chance(0.4) {
            path.push(Point::new(rng.range(0.1, 0.9) * s, rng.range(0.1, 0.9) * s));
        }
        path.push(Point::new(
            rng.range(0.15, 0.85) * s,
            rng.range(0.15, 0.85) * s,
        ));
        let w = width(rng);
        let end = End::random(rng);
        note += &format!(" line end w {w:.3} {end:?};");
        pieces.extend(stroke(&path, w, end));
    }
    for _ in 0..rng.int(0, 2) {
        let c = Point::new(rng.range(0.1, 0.9) * s, rng.range(0.1, 0.9) * s);
        let r = rng.range(0.4, s / 3.0);
        let w = width(rng).min(0.8 * r);
        note += &format!(" ring r {r:.2} w {w:.3};");
        pieces.extend(regions(vec![
            Polyline::circle(c, r + w / 2.0),
            Polyline::circle(c, r - w / 2.0),
        ]));
    }
    let frame = regions(vec![rect(0.0, 0.0, s, s)]);
    let left = difference(&frame, &pieces, FLATTEN_TOLERANCE_DEFAULT_MM);
    Motif {
        profiles: outlines(&left),
        note,
    }
}

/// A point 1 mm outside the `s` mm square, on a random side.
fn outside_point(rng: &mut Rng, s: f64) -> Point {
    let t = rng.range(0.0, s);
    match rng.int(0, 3) {
        0 => Point::new(t, -1.0),
        1 => Point::new(s + 1.0, t),
        2 => Point::new(t, s + 1.0),
        _ => Point::new(-1.0, t),
    }
}

/// The area a stroke along `path`, `w` wide, covers: a quad per segment,
/// a disc at each bend (round joins), and its last end as `end`; its
/// first end is butt.
fn stroke(path: &[Point], w: f64, end: End) -> Vec<Region> {
    let h = w / 2.0;
    let mut out = Vec::new();
    for pair in path.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let Some((_, n)) = frame_of(a, b) else {
            continue;
        };
        out.push(closed(&[
            add(a, n, -h),
            add(b, n, -h),
            add(b, n, h),
            add(a, n, h),
        ]));
    }
    for &p in &path[1..path.len() - 1] {
        out.push(Polyline::circle(p, h));
    }
    let (a, b) = (path[path.len() - 2], path[path.len() - 1]);
    if let Some((u, n)) = frame_of(a, b) {
        match end {
            End::Butt => {}
            End::Round => out.push(Polyline::circle(b, h)),
            End::Pointed(angle_deg) => {
                let tip = add(b, u, h / (angle_deg.to_radians() / 2.0).tan());
                out.push(closed(&[add(b, n, -h), tip, add(b, n, h)]));
            }
        }
    }
    regions(out)
}

// Spiky polygons.

/// A polygon of `K` slots round a circle of radius `R`, each slot plain
/// (with up to 12 corners, so as obtuse as 175°), an arc, an acute spike,
/// an acute notch, or a thin strip standing in (a line end), notches and
/// strips tilted up to 35° off the radius; holes inside holes in its
/// middle.
fn spiky(rng: &mut Rng) -> Motif {
    let r = rng.range(4.0, 12.0);
    let k_slots = rng.int(5, 12);
    let phase = rng.range(0.0, TAU);
    let mut vertices: Vec<Vertex> = Vec::new();
    let mut note = format!("spiky R {r:.2}, {k_slots} slots:");
    #[allow(clippy::cast_precision_loss, reason = "a small count")]
    let slot = TAU / k_slots as f64;
    let inner_mm = r * (slot / 2.0).cos();
    for k in 0..k_slots {
        #[allow(clippy::cast_precision_loss, reason = "a small count")]
        let (t0, t1) = (phase + slot * k as f64, phase + slot * (k + 1) as f64);
        let p0 = Point::new(r * t0.cos(), r * t0.sin());
        let p1 = Point::new(r * t1.cos(), r * t1.sin());
        let m = Point::new((p0.x + p1.x) / 2.0, (p0.y + p1.y) / 2.0);
        let chord = p0.distance_to(p1);
        let t = Point::new((p1.x - p0.x) / chord, (p1.y - p0.y) / chord);
        // Outward: the chord's right, travelling counter-clockwise.
        let n = Point::new(t.y, -t.x);
        let mut first = Vertex::line(p0.x, p0.y);
        let mut rest: Vec<Vertex> = Vec::new();
        match rng.int(0, 5) {
            0 => {
                let corners = rng.int(1, 12);
                for i in 1..corners {
                    #[allow(clippy::cast_precision_loss, reason = "small counts")]
                    let a = t0 + slot * i as f64 / corners as f64;
                    rest.push(Vertex::line(r * a.cos(), r * a.sin()));
                }
                note += &format!(" plain {corners};");
            }
            1 => {
                let bulge = rng.range(-0.2, 0.6);
                first.bulge = bulge;
                note += &format!(" arc {bulge:.3};");
            }
            2 => {
                let angle = rng.log_range(3.0, 30.0);
                let tan = (angle.to_radians() / 2.0).tan();
                let mut length = rng.range(1.0, 10.0);
                let half = (length * tan).min(0.45 * chord);
                length = half / tan;
                for p in [add(m, t, -half), add(m, n, length), add(m, t, half)] {
                    rest.push(Vertex::line(p.x, p.y));
                }
                note += &format!(" spike {angle:.2}° {length:.2} mm;");
            }
            3 => {
                let angle = rng.log_range(3.0, 30.0);
                let tilt = rng.range(-35.0, 35.0);
                let depth = rng.range(0.2, 0.55) * inner_mm;
                if let Some(points) = notch(m, t, n, chord, angle, tilt, depth) {
                    rest.extend(points.iter().map(|p| Vertex::line(p.x, p.y)));
                    note += &format!(" notch {angle:.2}° tilted {tilt:.1}° {depth:.2} mm;");
                }
            }
            4 => {
                let w = width(rng);
                let tilt = rng.range(-35.0, 35.0);
                let depth = rng.range(0.2, 0.55) * inner_mm;
                let end = End::random(rng);
                if let Some(points) = strip(m, t, n, chord, w, tilt, depth, end) {
                    rest.extend(points);
                    note += &format!(" line end w {w:.3} {end:?} tilted {tilt:.1}° {depth:.2} mm;");
                }
            }
            _ => note += " plain;",
        }
        vertices.push(first);
        vertices.extend(rest);
    }
    let outer = Polyline::new(vertices, true);
    // How near the middle the outline comes.
    let reach_mm = reach(&outer);
    let mut profiles = vec![outer];
    if rng.chance(0.8) && reach_mm > 0.5 {
        let first_mm = if rng.chance(0.35) {
            let g = rng.log_range(0.001, 0.05);
            note += &format!(" hole {g:.4} mm from the outline;");
            reach_mm - g
        } else {
            reach_mm * rng.range(0.3, 0.85)
        };
        let (nested, nested_note) = nested_rings(rng, Point::new(0.0, 0.0), first_mm);
        profiles.extend(nested);
        note += &nested_note;
    }
    Motif { profiles, note }
}

/// The least distance from the origin to `outline`, its arcs included.
fn reach(outline: &Polyline) -> f64 {
    let origin = Point::new(0.0, 0.0);
    dc_geometry::nearest_point(outline, origin).map_or(0.0, |n| n.gap_mm)
}

/// A notch of `angle_deg` into the slot whose chord has midpoint `m`,
/// direction `t` and outward normal `n`: its tip `depth` in from `m`
/// along the inward normal turned by `tilt_deg`, its sides crossing the
/// chord. The mouth, tip, mouth points in travel order, or `None` if the
/// mouth does not fit the chord.
fn notch(
    m: Point,
    t: Point,
    n: Point,
    chord: f64,
    angle_deg: f64,
    tilt_deg: f64,
    depth: f64,
) -> Option<[Point; 3]> {
    let u = rotate(Point::new(-n.x, -n.y), tilt_deg.to_radians());
    let tip = add(m, u, depth);
    let back = Point::new(-u.x, -u.y);
    let half = angle_deg.to_radians() / 2.0;
    let mut mouth = [
        on_chord(tip, rotate(back, half), m, t)?,
        on_chord(tip, rotate(back, -half), m, t)?,
    ];
    mouth.sort_by(|a, b| a.total_cmp(b));
    if mouth[0] < -0.45 * chord || mouth[1] > 0.45 * chord {
        return None;
    }
    Some([add(m, t, mouth[0]), tip, add(m, t, mouth[1])])
}

/// A strip `w` wide standing into the slot (see [`notch`]) `depth` in,
/// along the inward normal turned by `tilt_deg`, ending `end`. Its
/// vertices in travel order, or `None` if it does not fit.
#[allow(
    clippy::too_many_arguments,
    reason = "the slot's frame and the strip's values"
)]
fn strip(
    m: Point,
    t: Point,
    n: Point,
    chord: f64,
    w: f64,
    tilt_deg: f64,
    depth: f64,
    end: End,
) -> Option<Vec<Vertex>> {
    let u = rotate(Point::new(-n.x, -n.y), tilt_deg.to_radians());
    // `p` is the strip's left, looking along `u`; the chord runs along
    // `t`, so the side nearer `p0` comes first.
    let p = Point::new(-u.y, u.x);
    let h = w / 2.0;
    let sides = [add(m, p, -h), add(m, p, h)];
    let mouths = [on_chord(sides[0], u, m, t)?, on_chord(sides[1], u, m, t)?];
    if mouths.iter().any(|s| s.abs() > 0.45 * chord) {
        return None;
    }
    let (end_depth, apex) = match end {
        End::Butt => (depth, None),
        End::Round => (depth - h, None),
        End::Pointed(angle_deg) => (
            depth - h / (angle_deg.to_radians() / 2.0).tan(),
            Some(add(m, u, depth)),
        ),
    };
    if end_depth <= 0.05 {
        return None;
    }
    // Each side from the chord down to the end.
    let foot = |k: usize| add(m, t, mouths[k]);
    let bottom = |k: usize| add(sides[k], u, end_depth);
    let order = if mouths[0] < mouths[1] {
        [0, 1]
    } else {
        [1, 0]
    };
    let mut out = vec![
        Vertex::line(foot(order[0]).x, foot(order[0]).y),
        Vertex::line(bottom(order[0]).x, bottom(order[0]).y),
    ];
    match (end, apex) {
        (End::Round, _) => {
            // A half circle round the end, away from the chord: on the
            // left of travel from one bottom corner to the other when
            // that travel is clockwise about the end's centre.
            let a = bottom(order[0]);
            let b = bottom(order[1]);
            let towards = add(m, u, depth);
            let cross = (b.x - a.x) * (towards.y - a.y) - (b.y - a.y) * (towards.x - a.x);
            // A positive bulge is a counter-clockwise arc, which bulges
            // to the right of its chord.
            if let Some(last) = out.last_mut() {
                last.bulge = if cross > 0.0 { -1.0 } else { 1.0 };
            }
        }
        (_, Some(apex)) => out.push(Vertex::line(apex.x, apex.y)),
        _ => {}
    }
    out.push(Vertex::line(bottom(order[1]).x, bottom(order[1]).y));
    out.push(Vertex::line(foot(order[1]).x, foot(order[1]).y));
    Some(out)
}

/// Where the ray from `from` along `dir` crosses the line through `m`
/// along `t`, as the distance along `t` from `m`; `None` if it runs away
/// from or along it.
fn on_chord(from: Point, dir: Point, m: Point, t: Point) -> Option<f64> {
    // from + s·dir = m + τ·t.
    let det = dir.x * (-t.y) - dir.y * (-t.x);
    if det.abs() < 1e-9 {
        return None;
    }
    let (rx, ry) = (m.x - from.x, m.y - from.y);
    let s = (rx * (-t.y) - ry * (-t.x)) / det;
    let tau = (dir.x * ry - dir.y * rx) / det;
    (s > 0.0).then_some(tau)
}

/// Rings inside rings about `centre`, the first `first_mm` out: a hole,
/// an island in it, a hole in that, and so on, each a circle or a
/// polygon, the bands between them 0.1 to 3 mm. The outlines and a note.
fn nested_rings(rng: &mut Rng, centre: Point, first_mm: f64) -> (Vec<Polyline>, String) {
    let mut out = Vec::new();
    let mut note = String::from(" nested");
    let mut r = first_mm;
    while r > 0.2 && out.len() < 5 {
        let (outline, inscribed) = if rng.chance(0.5) {
            (Polyline::circle(centre, r), r)
        } else {
            let sides = rng.int(3, 8);
            let turn = rng.range(0.0, TAU);
            #[allow(clippy::cast_precision_loss, reason = "a small count")]
            let points: Vec<Point> = (0..sides)
                .map(|i| {
                    let a = turn + TAU * i as f64 / sides as f64;
                    Point::new(centre.x + r * a.cos(), centre.y + r * a.sin())
                })
                .collect();
            #[allow(clippy::cast_precision_loss, reason = "a small count")]
            let inscribed = r * (PI / sides as f64).cos();
            (closed(&points), inscribed)
        };
        note += &format!(" {r:.3}");
        out.push(outline);
        r = inscribed - width(rng);
    }
    note += ";";
    (out, note)
}

// Many small regions.

/// A grid of small cells, 0.5 to 2.5 mm, 0.1 to 1 mm apart, each a
/// rectangle, triangle, diamond or circle; sometimes all holes in one
/// frame, with islands in some of them.
fn mesh(rng: &mut Rng) -> Motif {
    let (nx, ny) = (rng.int(2, 6), rng.int(1, 5));
    let line = |rng: &mut Rng, count: usize| {
        let mut at = Vec::new();
        let mut x = 0.0;
        for _ in 0..count {
            let size = rng.log_range(0.5, 2.5);
            at.push((x, size));
            x += size + rng.log_range(0.1, 1.0);
        }
        (at, x)
    };
    let (xs, width_mm) = line(rng, nx);
    let (ys, height_mm) = line(rng, ny);
    let framed = rng.chance(0.4);
    let mut profiles = Vec::new();
    let mut note = format!("mesh {nx} × {ny}");
    for &(x, cw) in &xs {
        for &(y, ch) in &ys {
            let kind = rng.int(0, 4);
            let cell = match kind {
                0 | 1 => rect(x, y, x + cw, y + ch),
                2 => {
                    let corners = [
                        Point::new(x, y),
                        Point::new(x + cw, y),
                        Point::new(x + cw, y + ch),
                        Point::new(x, y + ch),
                    ];
                    let skip = rng.int(0, 3);
                    let points: Vec<Point> =
                        (0..4).filter(|&i| i != skip).map(|i| corners[i]).collect();
                    closed(&points)
                }
                3 => closed(&[
                    Point::new(x + cw / 2.0, y),
                    Point::new(x + cw, y + ch / 2.0),
                    Point::new(x + cw / 2.0, y + ch),
                    Point::new(x, y + ch / 2.0),
                ]),
                _ => Polyline::circle(Point::new(x + cw / 2.0, y + ch / 2.0), cw.min(ch) / 2.0),
            };
            profiles.push(cell);
            let inset = rng.log_range(0.1, 0.6);
            if framed && kind <= 1 && rng.chance(0.3) && cw.min(ch) > 2.0 * inset + 0.3 {
                profiles.push(rect(x + inset, y + inset, x + cw - inset, y + ch - inset));
            }
        }
    }
    if framed {
        let f = rng.log_range(0.1, 1.5);
        let last = |v: &[(f64, f64)], total: f64| v.last().map_or(total, |&(a, s)| a + s);
        profiles.push(rect(
            -f,
            -f,
            last(&xs, width_mm) + f,
            last(&ys, height_mm) + f,
        ));
        note += &format!(", framed {f:.3} mm out");
    }
    Motif { profiles, note }
}

// Near touches.

/// Two shapes, or a shape and its hole, 0.001 to 0.05 mm apart: parallel
/// edges, a corner at an edge, a circle at an edge, a hole at its
/// outline, two corners.
fn near(rng: &mut Rng) -> Motif {
    let g = rng.log_range(0.001, 0.05);
    let a = rng.range(1.0, 8.0);
    let b = rng.range(1.0, 8.0);
    let kind = rng.int(0, 4);
    let profiles = match kind {
        0 => {
            let dy = rng.range(-0.5, 0.5) * b;
            vec![
                rect(0.0, 0.0, a, b),
                rect(a + g, dy, a + g + rng.range(1.0, 8.0), dy + b),
            ]
        }
        1 => {
            let angle = rng.range(20.0, 120.0).to_radians();
            let half_mm = a * (angle / 2.0).tan();
            vec![
                closed(&[
                    Point::new(0.0, -half_mm),
                    Point::new(a, 0.0),
                    Point::new(0.0, half_mm),
                ]),
                rect(a + g, -b, a + g + rng.range(1.0, 8.0), b),
            ]
        }
        2 => {
            let r = a / 2.0;
            vec![
                Polyline::circle(Point::new(r, 0.0), r),
                rect(a + g, -b, a + g + rng.range(1.0, 8.0), b),
            ]
        }
        3 => {
            let hole = rng.range(0.2, 0.8) * a;
            let (y0, y1) = (rng.range(0.1, 0.4) * b, rng.range(0.6, 0.9) * b);
            let inner = if rng.chance(0.5) {
                rect(a - g - hole, y0, a - g, y1)
            } else {
                let r = (y1 - y0) / 2.0;
                Polyline::circle(Point::new(a - g - r, (y0 + y1) / 2.0), r)
            };
            vec![rect(0.0, 0.0, a, b), inner]
        }
        _ => {
            let d = g / std::f64::consts::SQRT_2;
            vec![
                rect(0.0, 0.0, a, b),
                rect(a + d, b + d, a + d + rng.range(1.0, 8.0), b + d + a),
            ]
        }
    };
    Motif {
        profiles,
        note: format!("near {kind} gap {g:.4} mm ({a:.2} × {b:.2})"),
    }
}

// Arcs.

/// Shapes drawn with arcs: circles inside circles, a slot, a rounded
/// rectangle, a crescent.
fn bulgy(rng: &mut Rng) -> Motif {
    let kind = rng.int(0, 3);
    let (profiles, note) = match kind {
        0 => {
            let r = rng.range(2.0, 7.0);
            let mut out = vec![Polyline::circle(Point::new(0.0, 0.0), r)];
            let mut note = format!("circles {r:.3}");
            let mut at = r - width(rng);
            while at > 0.15 && out.len() < 6 {
                out.push(Polyline::circle(Point::new(0.0, 0.0), at));
                note += &format!(" {at:.3}");
                at -= width(rng);
            }
            (out, note)
        }
        1 => {
            let length = rng.range(2.0, 12.0);
            let h = rng.log_range(0.15, 2.5);
            let mut out = vec![slot(length, h)];
            if h > 0.6 && rng.chance(0.5) {
                out.push(slot(length * 0.6, h - rng.range(0.1, 0.4) * h));
            }
            (out, format!("slot {length:.2} × {:.3}", 2.0 * h))
        }
        2 => {
            let (a, b) = (rng.range(2.0, 10.0), rng.range(2.0, 10.0));
            let rho = rng.range(0.05, 0.5) * a.min(b);
            (
                vec![rounded_rect(a, b, rho)],
                format!("rounded {a:.2} × {b:.2} r {rho:.3}"),
            )
        }
        _ => {
            let c = rng.range(2.0, 10.0);
            let b1 = rng.range(0.3, 1.0);
            let b2 = b1 * rng.range(0.2, 0.9);
            (
                vec![Polyline::new(
                    vec![Vertex::new(0.0, 0.0, b1), Vertex::new(c, 0.0, -b2)],
                    true,
                )],
                format!("crescent {c:.2} bulges {b1:.3} {b2:.3}"),
            )
        }
    };
    Motif { profiles, note }
}

/// A slot `length` long between its ends' centres, `2·h` wide.
fn slot(length: f64, h: f64) -> Polyline {
    let x0 = -length / 2.0;
    let x1 = length / 2.0;
    Polyline::new(
        vec![
            Vertex::new(x0, -h, 0.0),
            Vertex::new(x1, -h, 1.0),
            Vertex::new(x1, h, 0.0),
            Vertex::new(x0, h, 1.0),
        ],
        true,
    )
}

/// An `a` × `b` rectangle with its corners rounded by `rho`.
fn rounded_rect(a: f64, b: f64, rho: f64) -> Polyline {
    let q = (PI / 8.0).tan();
    Polyline::new(
        vec![
            Vertex::new(rho, 0.0, 0.0),
            Vertex::new(a - rho, 0.0, q),
            Vertex::new(a, rho, 0.0),
            Vertex::new(a, b - rho, q),
            Vertex::new(a - rho, b, 0.0),
            Vertex::new(rho, b, q),
            Vertex::new(0.0, b - rho, 0.0),
            Vertex::new(0.0, rho, q),
        ],
        true,
    )
}

// Helpers.

/// A closed polyline through `points`, straight edges.
pub fn closed(points: &[Point]) -> Polyline {
    Polyline::new(
        points.iter().map(|p| Vertex::line(p.x, p.y)).collect(),
        true,
    )
}

/// The rectangle from (`x0`, `y0`) to (`x1`, `y1`).
fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Polyline {
    closed(&[
        Point::new(x0, y0),
        Point::new(x1, y0),
        Point::new(x1, y1),
        Point::new(x0, y1),
    ])
}

/// `p + d·k`.
fn add(p: Point, d: Point, k: f64) -> Point {
    Point::new(p.x + d.x * k, p.y + d.y * k)
}

/// `v` turned by `angle` radians.
fn rotate(v: Point, angle: f64) -> Point {
    let (s, c) = angle.sin_cos();
    Point::new(v.x * c - v.y * s, v.x * s + v.y * c)
}

/// The unit direction from `a` to `b` and its left normal; `None` if they
/// coincide.
fn frame_of(a: Point, b: Point) -> Option<(Point, Point)> {
    let d = a.distance_to(b);
    if d < 1e-9 {
        return None;
    }
    let u = Point::new((b.x - a.x) / d, (b.y - a.y) / d);
    Some((u, Point::new(-u.y, u.x)))
}

/// Each of `polylines` as a region of its own (they may overlap: a
/// boolean takes their union).
fn regions(polylines: Vec<Polyline>) -> Vec<Region> {
    polylines
        .into_iter()
        .filter_map(|p| Contour::new(p).ok())
        .filter_map(|c| nest(vec![c]).ok())
        .flat_map(|(r, _)| r)
        .collect()
}

/// Every outline of `regions`.
pub fn outlines(regions: &[Region]) -> Vec<Polyline> {
    regions
        .iter()
        .flat_map(|r| std::iter::once(r.outer()).chain(r.holes()))
        .map(|c| c.polyline().clone())
        .collect()
}

/// `p` moved by `t`.
pub fn transformed(p: &Polyline, t: Transform) -> Polyline {
    t.apply_polyline(p).expect("a finite transform")
}

/// The box round `profiles`.
pub fn bounds(profiles: &[Polyline]) -> Rect {
    let mut all: Option<Rect> = None;
    for p in profiles {
        if let Some(b) = p.bounds() {
            all = Some(all.map_or(b, |a| a.including(b.min).including(b.max)));
        }
    }
    all.expect("a design has an outline")
}
