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
    project_of(
        &[(0.0, 0.0), (60.0, 0.0), (60.0, 40.0)],
        (40.0, 40.0),
        side,
        direction,
        edit,
    )
}

/// [`project`] with its one definition the open path through `points`,
/// placed at `at`.
fn project_of(
    points: &[(f64, f64)],
    at: (f64, f64),
    side: Side,
    direction: CutDirection,
    edit: impl FnOnce(&mut ProfileOp),
) -> Project {
    let mut project = sample_project();
    project.stock = Stock {
        width_mm: 200.0,
        height_mm: 150.0,
        thickness_mm: Some(STOCK_MM),
    };
    if let Some(definition) = project.definitions.values_mut().next() {
        definition.profiles = vec![Polyline::new(
            points.iter().map(|&(x, y)| Vertex::line(x, y)).collect(),
            false,
        )];
    }
    project.instances[0].transform = Transform::translate(at.0, at.1);
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
/// each run of cuts between rapids. `None` when the generator refuses
/// the drawing with an error that names it.
fn passes(project: &Project) -> Option<Vec<Vec<Cut>>> {
    let Operation::Profile(op) = &project.operations[0] else {
        unreachable!("a Profile");
    };
    let (toolpath, _) = match generate_profile(project, op) {
        Ok(generated) => generated,
        Err(
            dc_cam::CamError::ShapeNarrowerThanAllowance { .. }
            | dc_cam::CamError::AllowanceCutsIntoKeptSide { .. }
            | dc_cam::CamError::CutsIntoKeptSide { .. },
        ) => return None,
        Err(e) => panic!("{e}"),
    };
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
    Some(out)
}

/// A heightfield over an open path's neighbourhood: the lowest Z cut over
/// each lattice point, the stock top where nothing is.
struct Heights {
    origin: Point,
    nx: usize,
    ny: usize,
    z: Vec<f64>,
}

impl Heights {
    /// Over the points within the cuts' reach of `line`.
    fn new(line: &[Point]) -> Self {
        let margin = 2.0 * RADIUS_MM + ALLOWANCE_MM + 1.0;
        let (lo, hi) = line.iter().fold(
            (
                Point::new(f64::MAX, f64::MAX),
                Point::new(f64::MIN, f64::MIN),
            ),
            |(lo, hi), p| {
                (
                    Point::new(lo.x.min(p.x), lo.y.min(p.y)),
                    Point::new(hi.x.max(p.x), hi.y.max(p.y)),
                )
            },
        );
        let origin = Point::new(lo.x - margin, lo.y - margin);
        let nx = ((hi.x - lo.x + 2.0 * margin) / LATTICE_MM).ceil() as usize + 1;
        let ny = ((hi.y - lo.y + 2.0 * margin) / LATTICE_MM).ceil() as usize + 1;
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
    /// The open path, in project space.
    line: Vec<Point>,
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
/// docs), on the emitted G-code of `project`; false when the generator
/// refuses it.
fn check(project: &Project, expect: &Expect) -> bool {
    let case = &expect.case;
    let line = &expect.line;
    let lengths: Vec<f64> = line.windows(2).map(|w| w[0].distance_to(w[1])).collect();
    let length_mm: f64 = lengths.iter().sum();
    // Distances along the path to its inner vertices.
    let corners_mm: Vec<f64> = lengths
        .iter()
        .scan(0.0, |walked, l| {
            *walked += l;
            Some(*walked)
        })
        .take(lengths.len() - 1)
        .collect();
    let unit = |a: Point, b: Point| (b - a) * (1.0 / a.distance_to(b));
    let (first_dir, last_dir) = (
        unit(line[0], line[1]),
        unit(line[line.len() - 2], line[line.len() - 1]),
    );
    let tool = &project.tools[0];
    let (feed_mm_min, plunge_mm_min) = (tool.feed_mm_min.unwrap(), tool.plunge_mm_min.unwrap());
    let Some(passes) = passes(project) else {
        return false;
    };
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
                let at = place(line, p.xy());
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
                        (p.xy() - line[0]).dot(first_dir).abs() <= SLACK_MM,
                        "{case}: past the start {p:?}"
                    );
                }
                if at.along_mm >= length_mm - 1e-9 {
                    assert!(
                        (p.xy() - line[line.len() - 1]).dot(last_dir).abs() <= SLACK_MM,
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
    // The last pass's straight plunges (Z falls, XY stays) apart from the
    // rest of it: what the rest leaves standing over a tab is the tab, and
    // a plunge must not go below it either (#337: a reversed last pass
    // that starts on a tab).
    let plunge = |c: &Cut| c.to.z < c.from.z && c.from.xy().distance_to(c.to.xy()) <= 1e-9;
    let (plunges, lap): (Vec<Cut>, Vec<Cut>) = last[0].iter().map(clone_cut).partition(plunge);
    let mut before = Heights::new(line);
    before.sweep(&earlier, RADIUS_MM);
    let mut held = Heights::new(line);
    held.sweep(&earlier, RADIUS_MM - 0.03);
    held.sweep(&plunges, RADIUS_MM - 0.03);
    let mut final_lap = Heights::new(line);
    final_lap.sweep(&lap, RADIUS_MM);
    let mut final_pass = Heights::new(line);
    final_pass.sweep(&last[0], RADIUS_MM);
    let (mut skin, mut ends, mut standing) = (0, 0, 0);
    for j in 0..before.ny {
        for i in 0..before.nx {
            let q = before.point(i, j);
            let at = place(line, q);
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
            // No tab material is lost: where the last pass's lap leaves
            // stock standing above its floor, no earlier pass, and no
            // plunge of the last pass, cuts lower.
            let tab = final_lap.at(i, j);
            if tab < STOCK_MM && tab > 0.01 {
                standing += 1;
                assert!(
                    held.at(i, j) >= tab - 0.01,
                    "{case}: an earlier pass or a plunge cuts {} into a tab at {q:?}",
                    tab - held.at(i, j)
                );
            }
            // The skin, along the L away from its corner, where its
            // offsets are straight: up to each end normal the earlier
            // passes leave the allowance standing and clear the rest of
            // their kerf, and the last pass takes it off.
            let inside = at.along_mm > 0.0 && at.along_mm < length_mm;
            let off_corner = corners_mm
                .iter()
                .all(|c| (at.along_mm - c).abs() > 2.0 * RADIUS_MM + ALLOWANCE_MM + 1.0);
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
    true
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
        assert!(check(
            &project,
            &Expect {
                case,
                line: ell(),
                sign: if side == Side::Left { 1.0 } else { -1.0 },
                tabbed: false,
                rough_floor_mm: if thickness_mm.is_some() { 1.0 } else { 4.0 },
                passes: if thickness_mm.is_some() { 4 } else { 3 },
            },
        ));
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
            assert!(check(
                &project,
                &Expect {
                    case,
                    line: ell(),
                    sign: if side == Side::Left { 1.0 } else { -1.0 },
                    tabbed: true,
                    rough_floor_mm: 4.0,
                    passes: 3,
                },
            ));
        }
    }
}

/// The length of the last pass's path beside the open path through
/// `points`, placed at `at`, on `side`: the length its tabs are measured
/// along.
fn last_path_length_mm(points: &[(f64, f64)], at: (f64, f64), side: Side) -> f64 {
    let line = Polyline::new(
        points
            .iter()
            .map(|&(x, y)| Vertex::line(x + at.0, y + at.1))
            .collect(),
        false,
    );
    let left_mm = if side == Side::Left {
        RADIUS_MM
    } else {
        -RADIUS_MM
    };
    dc_geometry::offset_open(&line, left_mm)
        .iter()
        .map(Polyline::length_mm)
        .sum()
}

/// #311 review: tabs at an open path's ends. The review's two repros and
/// their variants: a 60 mm line at (60, 60) and a zigzag through (0, 0),
/// (5, 15), (10, 0), (15, 15), (20, 0), Left and Right, climb, with and
/// without `reverse` and `thickness_mm: 1`, rect and smooth 8 × 6 mm tabs
/// requested and placed, 12 mm deep in 4 mm passes at 0.5 mm. On the line
/// the tabs are 3 mm from each end of the last pass's path (the repro put
/// one 57 mm along); on the zigzag three are counted along it. Either way
/// a raised stretch reaches an end. On the emitted G-code no earlier pass,
/// and no plunge of the last pass, goes below what the last pass's lap
/// leaves standing at a tab, and the other checks of
/// [`left_right_last_pass_keeps_the_kept_side_and_cuts_to_the_ends`] hold.
/// The generator cuts every case.
#[test]
fn left_right_last_pass_keeps_tabs_at_the_ends() {
    let line: &[(f64, f64)] = &[(0.0, 0.0), (60.0, 0.0)];
    let zigzag: &[(f64, f64)] = &[
        (0.0, 0.0),
        (5.0, 15.0),
        (10.0, 0.0),
        (15.0, 15.0),
        (20.0, 0.0),
    ];
    let at = (60.0, 60.0);
    let (mut cut, mut cases) = (0, 0);
    for (name, points, count) in [("line", line, false), ("zigzag", zigzag, true)] {
        for side in [Side::Left, Side::Right] {
            let placement = if count {
                TabPlacement::Count { count: 3 }
            } else {
                let length_mm = last_path_length_mm(points, at, side);
                TabPlacement::AtDistances {
                    distances_mm: vec![3.0, length_mm - 3.0],
                }
            };
            for reverse in [false, true] {
                for thickness_mm in [None, Some(1.0)] {
                    for shape in [TabShape::Rect, TabShape::Smooth] {
                        for placed in [false, true] {
                            let case = format!(
                                "{name} {side:?} reverse {reverse} {thickness_mm:?} {shape:?} placed {placed}"
                            );
                            let mut project =
                                project_of(points, at, side, CutDirection::Climb, |op| {
                                    op.tabs = Some(TabSpec {
                                        placement: placement.clone(),
                                        width_mm: 8.0,
                                        height_mm: 6.0,
                                        shape,
                                        on_holes: false,
                                        avoid_corners: false,
                                    });
                                    op.last_pass = Some(LastPass {
                                        allowance_mm: ALLOWANCE_MM,
                                        thickness_mm,
                                        reverse,
                                    });
                                });
                            if placed {
                                project = dc_cam::placed_tabs::place_requests(&project).unwrap().0;
                            }
                            cases += 1;
                            let line: Vec<Point> = points
                                .iter()
                                .map(|&(x, y)| Point::new(x + at.0, y + at.1))
                                .collect();
                            let expect = Expect {
                                case,
                                line,
                                sign: if side == Side::Left { 1.0 } else { -1.0 },
                                tabbed: true,
                                rough_floor_mm: if thickness_mm.is_some() { 1.0 } else { 4.0 },
                                passes: if thickness_mm.is_some() { 4 } else { 3 },
                            };
                            cut += usize::from(check(&project, &expect));
                        }
                    }
                }
            }
        }
    }
    assert_eq!(cut, cases, "every case is cut");
}

/// The lattice points of `heights` on the kept side of `line`, cut on the
/// side of `sign` (+1 Left, −1 Right), that it has cut, with how deep: at
/// most a radius from the line on its other side, past its ends included
/// (judged there by the end segment's direction).
fn cuts_on_kept_side(heights: &Heights, line: &[Point], sign: f64) -> Vec<(Point, f64)> {
    let mut found = Vec::new();
    for j in 0..heights.ny {
        for i in 0..heights.nx {
            let q = heights.point(i, j);
            let at = place(line, q);
            if at.signed_mm * sign < -SLACK_MM
                && at.gap_mm <= RADIUS_MM
                && heights.at(i, j) < STOCK_MM
            {
                found.push((q, STOCK_MM - heights.at(i, j)));
            }
        }
    }
    found
}

/// #342: a V notch `angle_deg` across its tip, drawn down a 20 mm arm
/// from its first vertex to the tip at (0, 0) and up the other arm for
/// `last_mm`: it turns left at the tip, so Left is its inside and Right
/// its outside.
fn notch(angle_deg: f64, last_mm: f64) -> Vec<(f64, f64)> {
    let (sin, cos) = (angle_deg / 2.0).to_radians().sin_cos();
    vec![
        (-20.0 * sin, 20.0 * cos),
        (0.0, 0.0),
        (last_mm * sin, last_mm * cos),
    ]
}

/// #342: an open path whose last segment is shorter than the bit radius
/// never cuts the strip it keeps at its end. V notches of 30°, 45° and
/// 60° with last segments of 1, 2 and 3 mm, placed at (60, 40), cut 12 mm
/// deep in 4 mm passes with the 6.35 mm bit, Left (inside the V) and
/// Right (round it), with and without a 0.5 mm last pass, and without and
/// with 4 × 6 mm tabs 3 mm from each end of the last pass's path.
///
/// Inside the V the last segment has no room for the bit: a cut beside
/// it starts `r·cot(θ/2)` along it from the tip, 5.5 mm at 60°. The cut
/// beside the first arm then ran on until the circle of radius `r` round
/// the end vertex stopped it, past the normal through that vertex, and
/// cut into the strip the path keeps there (about 0.05 mm² at 30° and
/// 2 mm, the issue's repro) or the part past the end. Each is refused
/// with `EndSegmentHasNoRoom`, located at the last vertex, the bit (not
/// the allowance) having no room. Round the V every case is cut, and a
/// material heightfield of its emitted G-code shows no cut on the kept
/// side within a radius of the path, past either end included, while the
/// cut reaches both end normals.
#[test]
fn open_path_short_end_segment_keeps_the_kept_strip() {
    let at = (60.0, 40.0);
    let (mut refused, mut cut) = (0, 0);
    for angle_deg in [30.0, 45.0, 60.0] {
        for last_mm in [1.0, 2.0, 3.0] {
            let points = notch(angle_deg, last_mm);
            let line: Vec<Point> = points
                .iter()
                .map(|&(x, y)| Point::new(x + at.0, y + at.1))
                .collect();
            let total_mm: f64 = line.windows(2).map(|w| w[0].distance_to(w[1])).sum();
            for side in [Side::Left, Side::Right] {
                let inside = side == Side::Left;
                let length_mm = last_path_length_mm(&points, at, side);
                for last_pass in [false, true] {
                    for tabs in [false, true] {
                        let case = format!(
                            "{angle_deg}° {last_mm} mm {side:?} last pass {last_pass} tabs {tabs}"
                        );
                        let direction = if tabs {
                            CutDirection::Conventional
                        } else {
                            CutDirection::Climb
                        };
                        let project = project_of(&points, at, side, direction, |op| {
                            if !last_pass {
                                op.last_pass = None;
                            }
                            if tabs {
                                op.tabs = Some(TabSpec {
                                    placement: TabPlacement::AtDistances {
                                        distances_mm: vec![3.0, length_mm - 3.0],
                                    },
                                    width_mm: 4.0,
                                    height_mm: 6.0,
                                    shape: TabShape::Rect,
                                    on_holes: false,
                                    avoid_corners: false,
                                });
                            }
                        });
                        let Operation::Profile(op) = &project.operations[0] else {
                            unreachable!("a Profile");
                        };
                        match generate_profile(&project, op) {
                            Err(dc_cam::CamError::EndSegmentHasNoRoom {
                                vertex,
                                length_mm,
                                allowance_mm,
                                ..
                            }) if inside => {
                                assert_eq!(vertex, 2, "{case}: the last vertex");
                                assert!(
                                    (length_mm - last_mm).abs() < 1e-9,
                                    "{case}: {length_mm} mm"
                                );
                                assert_eq!(allowance_mm, None, "{case}: the bit has no room");
                                refused += 1;
                                continue;
                            }
                            Ok(_) if !inside => {}
                            Ok(_) => {
                                // Before #342: cut. Show what it takes.
                                let passes = passes(&project).expect("cut");
                                let all: Vec<Cut> =
                                    passes.iter().flatten().map(clone_cut).collect();
                                let mut heights = Heights::new(&line);
                                heights.sweep(&all, RADIUS_MM);
                                let kept = cuts_on_kept_side(&heights, &line, 1.0);
                                panic!(
                                    "{case}: cut, {} kept points cut, first {:?}",
                                    kept.len(),
                                    kept.first()
                                );
                            }
                            Err(e) => panic!("{case}: {e}"),
                        }
                        let passes = passes(&project).expect("cut");
                        let all: Vec<Cut> = passes.iter().flatten().map(clone_cut).collect();
                        let mut heights = Heights::new(&line);
                        heights.sweep(&all, RADIUS_MM);
                        let kept = cuts_on_kept_side(&heights, &line, -1.0);
                        assert!(
                            kept.is_empty(),
                            "{case}: {} kept points cut, first {:?}",
                            kept.len(),
                            kept.first()
                        );
                        let along: Vec<f64> = all
                            .iter()
                            .flat_map(|c| &c.samples)
                            .filter(|p| p.z < STOCK_MM)
                            .map(|p| place(&line, p.xy()).along_mm)
                            .collect();
                        assert!(
                            along.iter().any(|&a| a <= 1e-6)
                                && along.iter().any(|&a| a >= total_mm - 1e-6),
                            "{case}: the cut reaches both end normals"
                        );
                        cut += 1;
                    }
                }
            }
        }
    }
    assert_eq!((refused, cut), (36, 36), "refused and cut");
}
