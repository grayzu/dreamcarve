// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #311: a Left or Right Profile with a last pass allowance, checked on
//! the emitted G-code with a material heightfield, independently of the
//! generator. The program of an open L, cut beside it, is parsed back into
//! project space and every cut is swept through the stock on a 0.1 mm
//! lattice: the passes before the last into one heightfield, the last pass
//! into another.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss,
    reason = "test code reports failure by panicking; lattice indices of coordinates within the stock"
)]

use dc_cam::generate_profile;
use dc_gcode::emit;
use dc_gcode::parse::MotionKind;
use dc_gcode::transform::MachineTransform;
use dc_geometry::{Arc, Point, Point3, Polyline, Transform, Vec2, Vertex};
use dc_project::test_support::sample_project;
use dc_project::{
    CutDirection, Entry, LastPass, Operation, ProfileOp, Project, Side, Stock, TabPlacement,
    TabShape, TabSpec,
};

const STOCK_MM: f64 = 12.0;
const RADIUS_MM: f64 = 6.35 / 2.0;
const ALLOWANCE_MM: f64 = 0.5;
const LATTICE_MM: f64 = 0.1;
/// How far a written move may depart from the path, as S9 lets it: the
/// project's flatten tolerance (a smooth tab's ramp on an arc is cut as
/// chords within it), a unit in the last place of the program, and a
/// little for the arc sampling.
const SLACK_MM: f64 = dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM + 0.002;

/// The L, in project space: 60 mm along +X from (40, 40), then 40 mm up.
fn ell() -> Vec<Point> {
    vec![
        Point::new(40.0, 40.0),
        Point::new(100.0, 40.0),
        Point::new(100.0, 80.0),
    ]
}

/// The sample project on 200 × 150 × 12 mm stock, its one definition the
/// open L, cut by `op` 12 mm deep in 4 mm passes with a 0.5 mm last pass
/// allowance after `edit`.
fn project(side: Side, direction: CutDirection, edit: impl FnOnce(&mut ProfileOp)) -> Project {
    let mut project = sample_project();
    project.stock = Stock {
        width_mm: 200.0,
        height_mm: 150.0,
        thickness_mm: Some(STOCK_MM),
    };
    if let Some(definition) = project.definitions.values_mut().next() {
        definition.profiles = vec![Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(60.0, 0.0),
                Vertex::line(60.0, 40.0),
            ],
            false,
        )];
    }
    project.instances[0].transform = Transform::translate(40.0, 40.0);
    project.instances[0].z_top_mm = Some(STOCK_MM);
    let op = dc_project::test_support::profile_mut(&mut project);
    op.side = side;
    op.direction = direction;
    op.depth_mm = Some(STOCK_MM);
    op.step_down_mm = Some(4.0);
    op.tabs = None;
    op.entry = Entry::Plunge;
    op.last_pass = Some(LastPass {
        allowance_mm: ALLOWANCE_MM,
        thickness_mm: None,
        reverse: false,
    });
    edit(op);
    project
}

/// Where a point lies relative to the L.
struct Place {
    /// Distance to the L, mm.
    gap_mm: f64,
    /// The distance signed: positive on the L's left, negative on its
    /// right, judged at a vertex by the two segments' mean direction.
    signed_mm: f64,
    /// Distance along the L to the nearest point, mm.
    along_mm: f64,
}

/// Where `q` lies relative to the open polyline `line` of straight
/// segments.
fn place(line: &[Point], q: Point) -> Place {
    let mut best: Option<(f64, f64, f64)> = None;
    let mut walked = 0.0;
    let n = line.len();
    for k in 0..n - 1 {
        let (a, b) = (line[k], line[k + 1]);
        let d = b - a;
        let length = d.length();
        let t = ((q - a).dot(d) / (length * length)).clamp(0.0, 1.0);
        let near = a + d * t;
        let gap = q.distance_to(near);
        // The direction that judges the side: at a vertex the mean of
        // the segments meeting there.
        let mut dir = d * (1.0 / length);
        if t <= 0.0 && k > 0 {
            let prev = (line[k] - line[k - 1]) * (1.0 / line[k].distance_to(line[k - 1]));
            dir = Vec2::new(prev.x + dir.x, prev.y + dir.y);
        } else if t >= 1.0 && k + 2 < n {
            let next = (line[k + 2] - line[k + 1]) * (1.0 / line[k + 2].distance_to(line[k + 1]));
            dir = Vec2::new(dir.x + next.x, dir.y + next.y);
        }
        if best.is_none_or(|(g, _, _)| gap < g - 1e-12) {
            best = Some((gap, walked + t * length, dir.cross(q - near).signum()));
        }
        walked += length;
    }
    let (gap_mm, along_mm, sign) = best.expect("the L has segments");
    Place {
        gap_mm,
        signed_mm: sign * gap_mm,
        along_mm,
    }
}

/// One cutting move in project space, sampled at most a lattice step
/// apart, Z interpolated, with its feed.
struct Cut {
    from: Point3,
    to: Point3,
    feed: f64,
    samples: Vec<Point3>,
}

/// The program `project` exports, its cutting moves split into passes:
/// each run of cuts between rapids.
fn passes(project: &Project) -> Vec<Vec<Cut>> {
    let Operation::Profile(op) = &project.operations[0] else {
        unreachable!("a Profile");
    };
    let (toolpath, _) = generate_profile(project, op).unwrap_or_else(|e| panic!("{e}"));
    let (program, _) = emit(
        project,
        &[(
            project.operations[0].clone(),
            project.tools[0].clone(),
            toolpath,
        )
            .into()],
        "2026-10-21",
    )
    .unwrap_or_else(|e| panic!("the safety pass: {e}"));
    let post = project.post_processor.resolve().expect("bundled");
    let transform = MachineTransform::new(project, &post).expect("a valid mapping");
    let mut out: Vec<Vec<Cut>> = Vec::new();
    let mut run: Vec<Cut> = Vec::new();
    for mv in dc_gcode::parse_motion(&program.text).expect("the program parses") {
        let (Some(fx), Some(fy), Some(fz), Some(tx), Some(ty), Some(tz)) =
            (mv.from.x, mv.from.y, mv.from.z, mv.to.x, mv.to.y, mv.to.z)
        else {
            continue;
        };
        let from = transform.to_project(Point3::new(fx, fy, fz), mv.units);
        let to = transform.to_project(Point3::new(tx, ty, tz), mv.units);
        if !mv.kind.is_cut() {
            if !run.is_empty() {
                out.push(std::mem::take(&mut run));
            }
            continue;
        }
        let arc = match (mv.kind, mv.arc.i, mv.arc.j) {
            (MotionKind::ArcCw | MotionKind::ArcCcw, Some(i), Some(j)) => {
                let centre = transform
                    .to_project(Point3::new(fx + i, fy + j, fz), mv.units)
                    .xy();
                Some(Arc::from_ends(
                    from.xy(),
                    to.xy(),
                    centre,
                    mv.kind == MotionKind::ArcCcw,
                ))
            }
            _ => None,
        };
        let length_mm = arc.as_ref().map_or_else(
            || from.xy().distance_to(to.xy()),
            |a| a.radius_mm * a.sweep_rad.abs(),
        );
        let steps = (length_mm / LATTICE_MM).ceil().max(1.0) as usize;
        let samples = (0..=steps)
            .map(|k| {
                let f = k as f64 / steps as f64;
                let xy = match &arc {
                    Some(a) => {
                        let angle = a.start_angle_rad + a.sweep_rad * f;
                        a.centre + Vec2::new(angle.cos(), angle.sin()) * a.radius_mm
                    }
                    None => from.xy() + (to.xy() - from.xy()) * f,
                };
                Point3::new(xy.x, xy.y, from.z + (to.z - from.z) * f)
            })
            .collect();
        run.push(Cut {
            from,
            to,
            feed: mv.feed.expect("a cut has a feed"),
            samples,
        });
    }
    if !run.is_empty() {
        out.push(run);
    }
    out
}

/// A heightfield over the L's neighbourhood: the lowest Z cut over each
/// lattice point, the stock top where nothing is.
struct Heights {
    origin: Point,
    nx: usize,
    ny: usize,
    z: Vec<f64>,
}

impl Heights {
    fn new() -> Self {
        let margin = 2.0 * RADIUS_MM + ALLOWANCE_MM + 1.0;
        let origin = Point::new(40.0 - margin, 40.0 - margin);
        let nx = ((60.0 + 2.0 * margin) / LATTICE_MM).ceil() as usize + 1;
        let ny = ((40.0 + 2.0 * margin) / LATTICE_MM).ceil() as usize + 1;
        Self {
            origin,
            nx,
            ny,
            z: vec![STOCK_MM; nx * ny],
        }
    }

    fn point(&self, i: usize, j: usize) -> Point {
        Point::new(
            self.origin.x + i as f64 * LATTICE_MM,
            self.origin.y + j as f64 * LATTICE_MM,
        )
    }

    /// Lowers every lattice point within `radius_mm` of each sample of
    /// `cuts` to the sample's Z.
    fn sweep(&mut self, cuts: &[Cut], radius_mm: f64) {
        for p in cuts.iter().flat_map(|c| &c.samples) {
            if p.z >= STOCK_MM {
                continue;
            }
            let j0 = ((p.y - radius_mm - self.origin.y) / LATTICE_MM)
                .ceil()
                .max(0.0) as usize;
            let j1 = (((p.y + radius_mm - self.origin.y) / LATTICE_MM).floor() as usize)
                .min(self.ny - 1);
            for j in j0..=j1 {
                let dy = self.origin.y + j as f64 * LATTICE_MM - p.y;
                let reach = radius_mm * radius_mm - dy * dy;
                if reach < 0.0 {
                    continue;
                }
                let half = reach.sqrt();
                let i0 = ((p.x - half - self.origin.x) / LATTICE_MM).ceil().max(0.0) as usize;
                let i1 =
                    (((p.x + half - self.origin.x) / LATTICE_MM).floor() as usize).min(self.nx - 1);
                for i in i0..=i1 {
                    let cell = &mut self.z[j * self.nx + i];
                    *cell = cell.min(p.z);
                }
            }
        }
    }

    fn at(&self, i: usize, j: usize) -> f64 {
        self.z[j * self.nx + i]
    }
}

/// What one program is checked for.
struct Expect {
    case: String,
    /// The side the cut is on: +1 Left, −1 Right.
    sign: f64,
    /// The cut has tabs, so the passes do not all reach the floor.
    tabbed: bool,
    /// The deepest pass before the last, mm above the stock bottom.
    rough_floor_mm: f64,
    /// How many passes.
    passes: usize,
}

/// The checks of the issue's safety tests on one program (see the test
/// docs), on the emitted G-code of `project`.
fn check(project: &Project, expect: &Expect) {
    let case = &expect.case;
    let line = ell();
    let length_mm = 100.0;
    let corner_mm = 60.0;
    let tool = &project.tools[0];
    let (feed_mm_min, plunge_mm_min) = (tool.feed_mm_min.unwrap(), tool.plunge_mm_min.unwrap());
    let passes = passes(project);
    assert_eq!(passes.len(), expect.passes, "{case}: one run per pass");
    let last_index = passes.len() - 1;
    for (k, pass) in passes.iter().enumerate() {
        let away_mm = if k < last_index {
            RADIUS_MM + ALLOWANCE_MM
        } else {
            RADIUS_MM
        };
        let (mut first_mm, mut last_mm) = (f64::MAX, f64::MIN);
        for cut in pass {
            // Nothing below the material bottom, or the cut's depth.
            assert!(cut.to.z >= -0.0005, "{case}: below the material bottom");
            // Feeds from the tool, or a ramp's derived from them and
            // rounded down (#325): never above the feed, and never
            // falling faster than the plunge rate.
            let drop_mm = cut.from.z - cut.to.z;
            let length = (cut.to.xy().distance_to(cut.from.xy())).hypot(drop_mm);
            let tool_rate = [feed_mm_min, plunge_mm_min]
                .iter()
                .any(|r| (cut.feed - r).abs() <= 0.0005);
            let derived = cut.feed <= feed_mm_min + 0.0005
                && (drop_mm <= 0.0 || cut.feed * drop_mm / length <= plunge_mm_min + 0.0005);
            assert!(
                tool_rate || derived,
                "{case}: feed {} is not the tool's",
                cut.feed
            );
            for p in &cut.samples {
                let at = place(&line, p.xy());
                assert!(
                    (at.gap_mm - away_mm).abs() <= SLACK_MM,
                    "{case}: pass {k} cuts {} mm from the L at {p:?}, not {away_mm}",
                    at.gap_mm
                );
                assert!(
                    at.signed_mm * expect.sign > 0.0,
                    "{case}: pass {k} cuts on the kept side at {p:?}"
                );
                // The ends: on the normals through the L's end vertices,
                // never past them.
                if at.along_mm <= 1e-9 {
                    assert!(
                        (p.x - line[0].x).abs() <= SLACK_MM,
                        "{case}: past the start {p:?}"
                    );
                }
                if at.along_mm >= length_mm - 1e-9 {
                    assert!(
                        (p.y - line[2].y).abs() <= SLACK_MM,
                        "{case}: past the end {p:?}"
                    );
                }
                first_mm = first_mm.min(at.along_mm);
                last_mm = last_mm.max(at.along_mm);
            }
        }
        // Every pass reaches both end normals: none is shorter.
        assert!(
            first_mm <= 0.01 && last_mm >= length_mm - 0.01,
            "{case}: pass {k} runs from {first_mm} to {last_mm} mm along the L"
        );
    }

    let (earlier, last) = passes.split_at(last_index);
    let earlier: Vec<Cut> = earlier.iter().flatten().map(clone_cut).collect();
    let mut before = Heights::new();
    before.sweep(&earlier, RADIUS_MM);
    let mut held = Heights::new();
    held.sweep(&earlier, RADIUS_MM - 0.03);
    let mut final_pass = Heights::new();
    final_pass.sweep(&last[0], RADIUS_MM);
    let (mut skin, mut ends, mut standing) = (0, 0, 0);
    for j in 0..before.ny {
        for i in 0..before.nx {
            let q = before.point(i, j);
            let at = place(&line, q);
            let s = at.signed_mm * expect.sign;
            let (b, l) = (before.at(i, j), final_pass.at(i, j));
            // No pass cuts the kept side, past the ends included.
            if s < -SLACK_MM {
                assert!(
                    b >= STOCK_MM && l >= STOCK_MM,
                    "{case}: the kept side at {q:?} is cut to {}",
                    b.min(l)
                );
                continue;
            }
            // No tab material is lost: where the last pass leaves stock
            // standing above its floor, no earlier pass cuts lower.
            if l < STOCK_MM && l > 0.01 {
                standing += 1;
                assert!(
                    held.at(i, j) >= l - 0.01,
                    "{case}: an earlier pass cuts {} into a tab at {q:?}",
                    l - held.at(i, j)
                );
            }
            // The skin, along the L away from its corner, where its
            // offsets are straight: up to each end normal the earlier
            // passes leave the allowance standing and clear the rest of
            // their kerf, and the last pass takes it off.
            let inside = at.along_mm > 0.0 && at.along_mm < length_mm;
            let off_corner = (at.along_mm - corner_mm).abs() > 2.0 * RADIUS_MM + ALLOWANCE_MM + 1.0;
            if expect.tabbed || !inside || !off_corner {
                continue;
            }
            let near_end = at.along_mm < 0.3 || at.along_mm > length_mm - 0.3;
            if s > 0.05 && s < ALLOWANCE_MM - 0.05 {
                skin += 1;
                ends += usize::from(near_end);
                assert!(
                    b >= STOCK_MM,
                    "{case}: an earlier pass cuts the skin at {q:?}"
                );
            }
            if s > ALLOWANCE_MM + 0.05 && s < 2.0 * RADIUS_MM + ALLOWANCE_MM - 0.05 {
                assert!(
                    b <= expect.rough_floor_mm + 1e-6,
                    "{case}: the earlier passes leave {b} at {q:?}"
                );
            }
            if s > 0.05 && s < 2.0 * RADIUS_MM - 0.05 {
                assert!(b.min(l) <= 1e-6, "{case}: {} left uncut at {q:?}", b.min(l));
            }
        }
    }
    if expect.tabbed {
        assert!(
            standing > 100,
            "{case}: the last pass leaves tabs: {standing}"
        );
    } else {
        assert!(
            skin > 1000 && ends > 10,
            "{case}: {skin} skin points, {ends} at the ends"
        );
    }
}

fn clone_cut(c: &Cut) -> Cut {
    Cut {
        from: c.from,
        to: c.to,
        feed: c.feed,
        samples: c.samples.clone(),
    }
}

/// #311, the issue's safety tests on the emitted G-code: an open L cut
/// Left (inside its corner) and Right (round it), climb and conventional,
/// with a plunge and a ramp, with `thickness_mm: 1` and with `reverse`:
///
/// - no pass cuts the kept side, past the L's ends included;
/// - every earlier pass keeps exactly `r + a` from the L and the last
///   exactly `r`;
/// - the ends: every pass starts and ends on the normals through the L's
///   end vertices, so none cuts past them or stops short; the earlier
///   passes leave the allowance standing right up to each end and the
///   last pass takes it off there too;
/// - nothing goes below the material bottom;
/// - every feed is the tool's, or derived from them and rounded down
///   (#325).
#[test]
fn left_right_last_pass_keeps_the_kept_side_and_cuts_to_the_ends() {
    let ramp = Entry::Ramp { angle_deg: 10.0 };
    for (side, direction, entry, thickness_mm, reverse) in [
        (Side::Left, CutDirection::Climb, Entry::Plunge, None, false),
        (
            Side::Left,
            CutDirection::Conventional,
            ramp.clone(),
            Some(1.0),
            true,
        ),
        (Side::Right, CutDirection::Climb, ramp.clone(), None, true),
        (
            Side::Right,
            CutDirection::Conventional,
            Entry::Plunge,
            Some(1.0),
            false,
        ),
    ] {
        let case = format!("{side:?} {direction:?} {entry:?} {thickness_mm:?} {reverse}");
        let project = project(side, direction, |op| {
            op.entry = entry.clone();
            op.last_pass = Some(LastPass {
                allowance_mm: ALLOWANCE_MM,
                thickness_mm,
                reverse,
            });
        });
        check(
            &project,
            &Expect {
                case,
                sign: if side == Side::Left { 1.0 } else { -1.0 },
                tabbed: false,
                rough_floor_mm: if thickness_mm.is_some() { 1.0 } else { 4.0 },
                passes: if thickness_mm.is_some() { 4 } else { 3 },
            },
        );
    }
}

/// #311: no tab material is lost on an open path with a last pass. The
/// L with three 8 × 6 mm tabs, rect and smooth, requested and placed,
/// Left and Right, so the pass at Z 4 runs below the tab top: wherever
/// the last pass leaves stock standing above its floor, no earlier pass
/// cuts lower; and the other checks of
/// [`left_right_last_pass_keeps_the_kept_side_and_cuts_to_the_ends`] hold.
#[test]
fn left_right_last_pass_keeps_the_tabs() {
    for (side, direction, shape, reverse) in [
        (Side::Left, CutDirection::Climb, TabShape::Rect, false),
        (
            Side::Right,
            CutDirection::Conventional,
            TabShape::Smooth,
            false,
        ),
        (
            Side::Left,
            CutDirection::Conventional,
            TabShape::Smooth,
            true,
        ),
        (Side::Right, CutDirection::Climb, TabShape::Rect, true),
    ] {
        for placed in [false, true] {
            let case = format!("{side:?} {direction:?} {shape:?} {reverse} {placed}");
            let mut project = project(side, direction, |op| {
                op.tabs = Some(TabSpec {
                    placement: TabPlacement::Count { count: 3 },
                    width_mm: 8.0,
                    height_mm: 6.0,
                    shape,
                    on_holes: false,
                    avoid_corners: false,
                });
                if let Some(last_pass) = &mut op.last_pass {
                    last_pass.reverse = reverse;
                }
            });
            if placed {
                project = dc_cam::placed_tabs::place_requests(&project).unwrap().0;
                assert_eq!(project.tabs.len(), 3, "{case}");
            }
            check(
                &project,
                &Expect {
                    case,
                    sign: if side == Side::Left { 1.0 } else { -1.0 },
                    tabbed: true,
                    rough_floor_mm: 4.0,
                    passes: 3,
                },
            );
        }
    }
}
