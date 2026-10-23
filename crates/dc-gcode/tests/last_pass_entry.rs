// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! #339, #348: how a Profile's last pass enters and comes off its tabs,
//! checked on the emitted G-code with a material heightfield
//! (`support/heightfield.rs`), independently of the generator. Each
//! program is parsed back into project space and its cuts are swept, in
//! order, through 12 mm stock on a 0.1 mm lattice; before each straight
//! plunge the lattice says how much uncut material lies under the cutter.

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

#[path = "support/heightfield.rs"]
mod heightfield;

use dc_cam::{CamError, generate_profile};
use dc_gcode::emit;
use dc_geometry::{Point, Polyline, Transform, Vertex, nearest_point};
use dc_project::test_support::sample_project;
use dc_project::{
    CutDirection, Entry, LastPass, Lead, Operation, Project, Side, Stock, TabPlacement, TabShape,
    TabSpec,
};
use heightfield::{Cut, Heights};

const STOCK_MM: f64 = 12.0;
const STEP_DOWN_MM: f64 = 4.0;
const RADIUS_MM: f64 = 6.35 / 2.0;
const LATTICE_MM: f64 = 0.1;
/// How far past the outline the lattice reaches: a ramp or lead beside
/// the wall, and the cutter round it.
const MARGIN_MM: f64 = 32.0;
/// How far a written move may depart from the path: the flatten tolerance,
/// a unit in the last place, and a little for the arc sampling.
const SLACK_MM: f64 = dc_geometry::FLATTEN_TOLERANCE_DEFAULT_MM + 0.002;
/// Rounding of the written numbers and of the lattice.
const DEPTH_TOL_MM: f64 = 0.01;

/// The entries the issue lists: plunge, ramp, helix, and a lead (with a
/// plunge and with a ramp).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Entering {
    Plunge,
    Ramp,
    Helix,
    LeadPlunge,
    LeadRamp,
}

const ENTERINGS: [Entering; 5] = [
    Entering::Plunge,
    Entering::Ramp,
    Entering::Helix,
    Entering::LeadPlunge,
    Entering::LeadRamp,
];

impl Entering {
    fn entry(self) -> Entry {
        match self {
            Self::Plunge | Self::LeadPlunge => Entry::Plunge,
            Self::Ramp | Self::LeadRamp => Entry::Ramp { angle_deg: 10.0 },
            Self::Helix => Entry::Helix {
                diameter_fraction: 0.8,
                angle_deg: 5.0,
            },
        }
    }

    fn lead(self) -> Option<Lead> {
        matches!(self, Self::LeadPlunge | Self::LeadRamp).then_some(Lead {
            radius_mm: 4.0,
            sweep_deg: 90.0,
            lead_out: true,
            overcut_mm: 1.0,
        })
    }
}

/// A drawing: its profile in definition space, placed at `at`.
#[derive(Debug, Clone)]
struct Drawing {
    name: &'static str,
    profile: Polyline,
    at: (f64, f64),
}

impl Drawing {
    /// The profile in project space.
    fn placed(&self) -> Polyline {
        Transform::translate(self.at.0, self.at.1)
            .apply_polyline(&self.profile)
            .unwrap()
    }
}

/// A 24 × 16 mm rectangle at (60, 50).
fn rect() -> Drawing {
    Drawing {
        name: "rect",
        profile: Polyline::rect(Point::new(0.0, 0.0), 24.0, 16.0),
        at: (60.0, 50.0),
    }
}

/// A round hole `diameter_mm` across, centred on (80, 60).
fn hole(name: &'static str, diameter_mm: f64) -> Drawing {
    let r = diameter_mm / 2.0;
    Drawing {
        name,
        profile: Polyline::new(
            vec![Vertex::new(r, 0.0, 1.0), Vertex::new(-r, 0.0, 1.0)],
            true,
        ),
        at: (80.0, 60.0),
    }
}

/// An open L, 30 mm along +X then 20 mm up, from (60, 50).
fn ell() -> Drawing {
    Drawing {
        name: "L",
        profile: Polyline::new(
            vec![
                Vertex::line(0.0, 0.0),
                Vertex::line(30.0, 0.0),
                Vertex::line(30.0, 20.0),
            ],
            false,
        ),
        at: (60.0, 50.0),
    }
}

/// One case: the drawing, the Profile's side, entry and last pass, and
/// its tabs.
#[derive(Debug, Clone)]
struct Case {
    drawing: Drawing,
    side: Side,
    direction: CutDirection,
    entering: Entering,
    allowance_mm: f64,
    thickness_mm: Option<f64>,
    reverse: bool,
    tabs: Option<TabSpec>,
}

impl Case {
    fn name(&self) -> String {
        format!(
            "{} {:?} {:?} {:?} a {} t {:?} reverse {} tabs {:?}",
            self.drawing.name,
            self.side,
            self.direction,
            self.entering,
            self.allowance_mm,
            self.thickness_mm,
            self.reverse,
            self.tabs
                .as_ref()
                .map(|t| (t.shape, t.height_mm, t.placement.clone()))
        )
    }

    /// The project: 200 × 150 × 12 mm stock, the drawing cut 12 mm deep
    /// in 4 mm passes with the 6.35 mm end mill.
    fn project(&self, last_pass: bool) -> Project {
        let mut project = sample_project();
        project.stock = Stock {
            width_mm: 200.0,
            height_mm: 150.0,
            thickness_mm: Some(STOCK_MM),
        };
        if let Some(definition) = project.definitions.values_mut().next() {
            definition.profiles = vec![self.drawing.profile.clone()];
        }
        project.instances[0].transform = Transform::translate(self.drawing.at.0, self.drawing.at.1);
        project.instances[0].z_top_mm = Some(STOCK_MM);
        let op = dc_project::test_support::profile_mut(&mut project);
        op.side = self.side;
        op.direction = self.direction;
        op.depth_mm = Some(STOCK_MM);
        op.step_down_mm = Some(STEP_DOWN_MM);
        op.entry = self.entering.entry();
        op.lead = self.entering.lead();
        op.tabs.clone_from(&self.tabs);
        op.last_pass = last_pass.then_some(LastPass {
            allowance_mm: self.allowance_mm,
            thickness_mm: self.thickness_mm,
            reverse: self.reverse,
        });
        project
    }
}

/// Two 6 × 3 mm tabs of `shape`, no taller than a step-down.
fn two_tabs(shape: TabShape) -> TabSpec {
    TabSpec {
        placement: TabPlacement::Count { count: 2 },
        width_mm: 6.0,
        height_mm: 3.0,
        shape,
        on_holes: false,
        avoid_corners: false,
    }
}

/// The program `project` exports and its cutting moves in project space,
/// in order; `None` when the generator refuses the drawing for the last
/// pass (or a helix on an open path).
fn program(project: &Project) -> Option<(String, Vec<Cut>)> {
    let Operation::Profile(op) = &project.operations[0] else {
        unreachable!("a Profile");
    };
    let (toolpath, _) = match generate_profile(project, op) {
        Ok(generated) => generated,
        Err(
            CamError::ShapeNarrowerThanAllowance { .. }
            | CamError::BandNarrowerThanAllowance { .. }
            | CamError::HelixNeedsWasteSide { .. },
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
        "2026-10-22",
    )
    .unwrap_or_else(|e| panic!("the safety pass: {e}"));
    let cuts = heightfield::cuts(project, &program.text, LATTICE_MM);
    Some((program.text, cuts))
}

/// A heightfield over the drawing `outline` and [`MARGIN_MM`] round it,
/// on the [`LATTICE_MM`] lattice, under the 12 mm stock.
fn heights(outline: &Polyline) -> Heights {
    Heights::with(outline, LATTICE_MM, MARGIN_MM, STOCK_MM)
}

/// What one program shows.
struct Measured {
    /// The deepest straight plunge into uncut material, mm.
    deepest_plunge_mm: f64,
}

/// The checks of the issue's tests on the emitted G-code of `case`:
///
/// - every straight plunge goes no deeper into uncut material than the
///   step-down;
/// - no tab material is lost: where the last pass's lap leaves stock
///   standing above its floor, nothing else cuts lower;
/// - nothing goes below the material bottom;
/// - every feed is the tool's, or derived from them and rounded down
///   (#325).
///
/// `None` when the generator refuses the drawing.
fn check(case: &Case) -> Option<Measured> {
    let name = case.name();
    let project = case.project(true);
    let (_, cuts) = program(&project)?;
    let tool = &project.tools[0];
    let (feed_mm_min, plunge_mm_min) = (tool.feed_mm_min.unwrap(), tool.plunge_mm_min.unwrap());
    let outline = case.drawing.placed();
    let mut heights = heights(&outline);
    let mut deepest_plunge_mm: f64 = 0.0;
    for cut in &cuts {
        assert!(cut.to.z >= -0.0005, "{name}: below the material bottom");
        let drop_mm = cut.from.z - cut.to.z;
        let length = (cut.to.xy().distance_to(cut.from.xy())).hypot(drop_mm);
        let tool_rate = [feed_mm_min, plunge_mm_min]
            .iter()
            .any(|r| (cut.feed - r).abs() <= 0.0005);
        let derived = cut.feed <= feed_mm_min + 0.0005
            && (drop_mm <= 0.0 || cut.feed * drop_mm / length <= plunge_mm_min + 0.0005);
        assert!(
            tool_rate || derived,
            "{name}: feed {} is not the tool's",
            cut.feed
        );
        deepest_plunge_mm = deepest_plunge_mm.max(heights.plunge_depth_mm(cut, RADIUS_MM));
        heights.sweep(cut, RADIUS_MM);
    }
    assert!(
        deepest_plunge_mm <= STEP_DOWN_MM + DEPTH_TOL_MM,
        "{name}: a plunge goes {deepest_plunge_mm} mm into uncut material, more than the {STEP_DOWN_MM} mm step-down"
    );
    if case.tabs.is_some() {
        check_tabs(&name, &outline, &cuts);
    }
    Some(Measured { deepest_plunge_mm })
}

/// No tab material is lost: the last pass's lap is every cut whose samples
/// all run `r` from the outline (the earlier passes run `r + a`); where it
/// leaves stock standing above its floor, no other cut goes lower.
fn check_tabs(name: &str, outline: &Polyline, cuts: &[Cut]) {
    let on_lap = |c: &Cut| {
        c.samples.iter().all(|p| {
            nearest_point(outline, p.xy()).is_some_and(|n| (n.gap_mm - RADIUS_MM).abs() <= SLACK_MM)
        })
    };
    let mut lap = heights(outline);
    let mut held = heights(outline);
    for cut in cuts {
        if on_lap(cut) {
            lap.sweep(cut, RADIUS_MM);
        } else {
            held.sweep(cut, RADIUS_MM - 0.03);
        }
    }
    let mut standing = 0;
    for j in 0..lap.ny {
        for i in 0..lap.nx {
            let tab = lap.at(i, j);
            if tab < STOCK_MM - 1e-9 && tab > 0.01 {
                standing += 1;
                assert!(
                    held.at(i, j) >= tab - 0.01,
                    "{name}: a cut other than the lap goes {} into a tab at {:?}",
                    tab - held.at(i, j),
                    lap.point(i, j)
                );
            }
        }
    }
    assert!(
        standing > 50,
        "{name}: the last pass leaves tabs: {standing}"
    );
}

/// Every case of the issue's matrix: Inside, Outside, Left and Right; a
/// plunge, a ramp, a helix and a lead; allowances of 0.2, 0.5 and 2 mm;
/// and the round holes of 8, 10 and 15 mm cut Inside.
fn matrix() -> Vec<Case> {
    let mut cases = Vec::new();
    let closed = [
        (rect(), Side::Inside),
        (rect(), Side::Outside),
        (hole("8 mm hole", 8.0), Side::Inside),
        (hole("10 mm hole", 10.0), Side::Inside),
        (hole("15 mm hole", 15.0), Side::Inside),
    ];
    let open = [(ell(), Side::Left), (ell(), Side::Right)];
    for (drawing, side) in closed.into_iter().chain(open) {
        for entering in ENTERINGS {
            for (k, allowance_mm) in [0.2, 0.5, 2.0].into_iter().enumerate() {
                cases.push(Case {
                    drawing: drawing.clone(),
                    side,
                    direction: if k == 1 {
                        CutDirection::Conventional
                    } else {
                        CutDirection::Climb
                    },
                    entering,
                    allowance_mm,
                    thickness_mm: (k == 2).then_some(1.0),
                    reverse: k == 1,
                    tabs: None,
                });
            }
        }
    }
    cases
}

/// #339: on the emitted G-code of every case of [`matrix`], no straight
/// plunge goes deeper into uncut material than the step-down, whatever the
/// entry; nothing goes below the material bottom; every feed is the
/// tool's or derived from them, rounded down. The cases a drawing cannot
/// take (the 8 mm hole at 2 mm, a helix on an open path) are refused, and
/// every other case is cut.
#[test]
fn last_pass_entry_never_plunges_deeper_than_step_down() {
    let mut refused = Vec::new();
    let mut cut = 0;
    let mut deepest_mm: f64 = 0.0;
    for case in matrix() {
        match check(&case) {
            Some(m) => {
                cut += 1;
                deepest_mm = deepest_mm.max(m.deepest_plunge_mm);
            }
            None => refused.push(case.name()),
        }
    }
    println!("the deepest plunge into uncut material: {deepest_mm:.3} mm");
    // D + 2a is 10.35 mm at 2 mm, more than the 8 and 10 mm holes; an
    // open path has no waste side for a helix.
    for name in &refused {
        assert!(
            name.contains("Helix") && name.starts_with("L ")
                || (name.starts_with("8 mm hole") || name.starts_with("10 mm hole"))
                    && name.contains(" a 2 "),
            "{name} is refused"
        );
    }
    assert_eq!(refused.len(), 16, "{refused:?}");
    assert_eq!(cut, 89);
}

/// #339 with tabs: the rectangle Inside and Outside and the L Left and
/// Right, with rect and smooth 6 × 3 mm tabs, every entry, at 0.5 mm:
/// besides the checks of
/// [`last_pass_entry_never_plunges_deeper_than_step_down`], no tab
/// material is lost (#337: a lap that starts on a tab starts at its
/// height, and the last pass's entry keeps off the tabs).
#[test]
fn last_pass_entry_keeps_the_tabs() {
    for (drawing, side) in [
        (rect(), Side::Inside),
        (rect(), Side::Outside),
        (ell(), Side::Left),
        (ell(), Side::Right),
    ] {
        for entering in ENTERINGS {
            for (shape, reverse) in [(TabShape::Rect, false), (TabShape::Smooth, true)] {
                let case = Case {
                    drawing: drawing.clone(),
                    side,
                    direction: CutDirection::Climb,
                    entering,
                    allowance_mm: 0.5,
                    thickness_mm: None,
                    reverse,
                    tabs: Some(two_tabs(shape)),
                };
                let refused = check(&case).is_none();
                assert!(
                    !refused || (entering == Entering::Helix && !side.has_waste_area()),
                    "{} is refused",
                    case.name()
                );
            }
        }
    }
}

/// #339 review: the step across the allowance is held to what it sweeps,
/// not to the waste less the tabs' entry keep-outs. The review's repro, a
/// 15 mm hole cut Inside with one 6 × 3 mm rect tab at 0.5 mm, and the
/// 24 × 16 mm rectangle cut Inside with two tabs at 1 mm and Outside with
/// four at 2 mm, each with a ramp, a helix and a lead, rect and smooth:
/// every one is cut, as before #339, and the checks of
/// [`last_pass_entry_keeps_the_tabs`] hold.
#[test]
fn last_pass_entry_steps_clear_of_tabs() {
    for (drawing, side, count, allowance_mm) in [
        (hole("15 mm hole", 15.0), Side::Inside, 1, 0.5),
        (rect(), Side::Inside, 2, 1.0),
        (rect(), Side::Outside, 4, 2.0),
    ] {
        for entering in [
            Entering::Ramp,
            Entering::Helix,
            Entering::LeadPlunge,
            Entering::LeadRamp,
        ] {
            for shape in [TabShape::Rect, TabShape::Smooth] {
                let case = Case {
                    drawing: drawing.clone(),
                    side,
                    direction: CutDirection::Climb,
                    entering,
                    allowance_mm,
                    thickness_mm: None,
                    reverse: false,
                    tabs: Some(TabSpec {
                        placement: TabPlacement::Count { count },
                        ..two_tabs(shape)
                    }),
                };
                assert!(check(&case).is_some(), "{} is refused", case.name());
            }
        }
    }
}

/// A 60 mm open line from (60, 50) along +X.
fn line() -> Drawing {
    Drawing {
        name: "line",
        profile: Polyline::new(vec![Vertex::line(0.0, 0.0), Vertex::line(60.0, 0.0)], false),
        at: (60.0, 50.0),
    }
}

/// #339 with #337: a last pass that starts on a tab. The 60 mm line cut
/// Left and Right with 8 × 3 mm tabs 3 mm from each end of the last
/// pass's path, rect and smooth, from either end, with a plunge and a
/// ramp: the allowance passes and the last pass both start on a tab, the
/// smooth one partway up its slope, where the last pass comes down from
/// the allowance pass's height after its step. The checks of
/// [`last_pass_entry_keeps_the_tabs`] hold, and every case is cut.
#[test]
fn last_pass_entry_starting_on_a_tab() {
    for side in [Side::Left, Side::Right] {
        for entering in [Entering::Plunge, Entering::Ramp] {
            for shape in [TabShape::Rect, TabShape::Smooth] {
                for reverse in [false, true] {
                    let case = Case {
                        drawing: line(),
                        side,
                        direction: CutDirection::Climb,
                        entering,
                        allowance_mm: 0.5,
                        thickness_mm: None,
                        reverse,
                        tabs: Some(TabSpec {
                            placement: TabPlacement::AtDistances {
                                distances_mm: vec![3.0, 57.0],
                            },
                            width_mm: 8.0,
                            ..two_tabs(shape)
                        }),
                    };
                    assert!(check(&case).is_some(), "{} is refused", case.name());
                }
            }
        }
    }
}

/// The layer the last pass's load is measured at, mm above the stock
/// bottom (#335): above every earlier pass's floor, so only the
/// allowance stands there for the last pass to take.
const LOAD_LAYER_MM: f64 = 6.0;

/// How hard the last pass's lap works in one program (#335).
struct Load {
    /// The fastest its cutting edge moves at the wall, as a multiple of
    /// the tool's feed: an arc's written feed times the wall's radius over
    /// the tool centre's.
    edge: f64,
    /// The material it removes per minute at [`LOAD_LAYER_MM`], as a
    /// multiple of what a straight wall's last pass removes there at the
    /// tool's feed, `feed × allowance` mm² per minute.
    removal: f64,
}

/// [`Load`] of `case`'s emitted G-code: its lap is every cut whose samples
/// all run `r` from the outline, the material on a 0.05 mm lattice;
/// `None` when the generator refuses the drawing.
fn load(case: &Case) -> Option<Load> {
    let project = case.project(true);
    let (_, cuts) = program(&project)?;
    let feed_mm_min = project.tools[0].feed_mm_min.unwrap();
    let outline = case.drawing.placed();
    let gap = |p: Point| nearest_point(&outline, p).map_or(f64::INFINITY, |n| n.gap_mm);
    let on_lap = |c: &Cut| {
        c.samples
            .iter()
            .all(|p| (gap(p.xy()) - RADIUS_MM).abs() <= SLACK_MM)
    };
    let first = cuts.iter().position(on_lap)?;
    let last = cuts.iter().rposition(on_lap)?;
    let mut before = Heights::with(&outline, 0.05, 8.0, STOCK_MM);
    for cut in &cuts[..first] {
        before.sweep(cut, RADIUS_MM);
    }
    let mut after = before.clone();
    let (mut edge, mut minutes): (f64, f64) = (0.0, 0.0);
    for cut in cuts[first..=last].iter().filter(|c| on_lap(c)) {
        after.sweep(cut, RADIUS_MM);
        let length_mm: f64 = cut
            .samples
            .windows(2)
            .map(|w| w[0].xy().distance_to(w[1].xy()))
            .sum();
        minutes += length_mm / cut.feed;
        if let (Some(rho), Some(centre)) = (cut.radius_mm, cut.centre)
            && (cut.from.z - cut.to.z).abs() <= 1e-9
        {
            // The wall the cut runs beside: round the centre, its radius
            // `r` more than the tool centre's inside a hole, less round a
            // part's corner.
            let wall_mm = gap(centre);
            edge = edge.max(cut.feed * wall_mm / rho / feed_mm_min);
        } else if cut.from.xy().distance_to(cut.to.xy()) > 1e-9 {
            edge = edge.max(cut.feed / feed_mm_min);
        }
    }
    let removed = (0..after.z.len())
        .filter(|&k| before.z[k] > LOAD_LAYER_MM && after.z[k] <= LOAD_LAYER_MM)
        .count();
    let area_mm2 = removed as f64 * 0.05 * 0.05;
    let removal = area_mm2 / minutes / (feed_mm_min * case.allowance_mm);
    Some(Load { edge, removal })
}

/// #335: in the round holes of 8, 10 and 15 mm cut Inside with a 6.35 mm
/// bit, with every entry and allowances of 0.2, 0.5 and 2 mm (the 8 and
/// 10 mm holes refuse 2 mm), the last pass's cutting edge never moves
/// faster than the feed, and its lap removes no more per minute at a
/// layer the earlier passes left the allowance on than a straight wall's
/// last pass does at the feed (within the lattice's 5%). Before #335 the
/// edge in the 8 mm hole moved at 4.8 times the feed and removed 4.5
/// times as much. The checks of
/// [`last_pass_entry_never_plunges_deeper_than_step_down`] hold too.
#[test]
fn last_pass_small_hole_load() {
    let mut measured = 0;
    for diameter_mm in [8.0, 10.0, 15.0] {
        for entering in ENTERINGS {
            for allowance_mm in [0.2, 0.5, 2.0] {
                let case = Case {
                    drawing: hole("hole", diameter_mm),
                    side: Side::Inside,
                    direction: CutDirection::Climb,
                    entering,
                    allowance_mm,
                    thickness_mm: None,
                    reverse: false,
                    tabs: None,
                };
                let name = format!("{diameter_mm} mm {}", case.name());
                let Some(load) = load(&case) else {
                    assert!(diameter_mm < 6.35 + 2.0 * allowance_mm, "{name} is refused");
                    continue;
                };
                assert!(check(&case).is_some(), "{name}");
                println!(
                    "{name}: edge {:.3} × feed, removal {:.3} × a straight wall's",
                    load.edge, load.removal
                );
                assert!(
                    load.edge <= 1.0 + 1e-6,
                    "{name}: the cutting edge moves at {} times the feed",
                    load.edge
                );
                assert!(
                    load.removal <= 1.05,
                    "{name}: the last pass removes {} times a straight wall's",
                    load.removal
                );
                measured += 1;
            }
        }
    }
    assert_eq!(measured, 5 * 7);
    // For reference: the rectangle, whose straight walls run at the feed.
    for side in [Side::Inside, Side::Outside] {
        let case = Case {
            drawing: rect(),
            side,
            direction: CutDirection::Climb,
            entering: Entering::Plunge,
            allowance_mm: 0.5,
            thickness_mm: None,
            reverse: false,
            tabs: None,
        };
        let load = load(&case).expect("cut");
        println!(
            "{}: edge {:.3} × feed, removal {:.3} × a straight wall's",
            case.name(),
            load.edge,
            load.removal
        );
        assert!(load.edge <= 1.0 + 1e-6, "{}", case.name());
    }
}

/// #339: projects without a last pass export what they did before: every
/// case of [`matrix`] with `last_pass` unset is compared with the program
/// the generator wrote before this change, through its fingerprint.
#[test]
fn no_last_pass_programs_are_unchanged() {
    let mut fingerprint: u64 = 0xcbf2_9ce4_8422_2325;
    for case in matrix()
        .iter()
        .filter(|c| (c.allowance_mm - 0.5).abs() < 1e-9)
    {
        let Some((text, _)) = program(&case.project(false)) else {
            continue;
        };
        for byte in case.name().bytes().chain(text.bytes()) {
            fingerprint ^= u64::from(byte);
            fingerprint = fingerprint.wrapping_mul(0x0100_0000_01b3);
        }
    }
    assert_eq!(fingerprint, NO_LAST_PASS_FINGERPRINT, "{fingerprint:#x}");
}

/// The fingerprint of [`no_last_pass_programs_are_unchanged`]'s programs
/// as the build before #339 wrote them.
const NO_LAST_PASS_FINGERPRINT: u64 = 0x25c8_2b67_09e7_5e4a;

/// #348: tabs taller than the step-down. The 60 mm line cut Left and Right
/// with 8 × 6 mm tabs 3 mm from each end, and the 24 × 16 mm rectangle cut
/// Inside and Outside with two 8 × 6 mm tabs, rect and smooth, plunged and
/// ramped, with `reverse` off and on, at 0.5 mm. Before #348 the last pass
/// came down a rect tab's whole 6 mm at its far end in one move, and a
/// smooth tab's at the start of a lap that starts on its slope, into
/// material no earlier pass cut. On the emitted G-code no straight plunge
/// goes deeper into uncut material than the 4 mm step-down, and no tab
/// material is lost ([`check`]); every case is cut.
#[test]
fn last_pass_entry_with_tall_tabs() {
    let mut deepest_mm: f64 = 0.0;
    for (drawing, side, placement) in [
        (
            line(),
            Side::Left,
            TabPlacement::AtDistances {
                distances_mm: vec![3.0, 57.0],
            },
        ),
        (
            line(),
            Side::Right,
            TabPlacement::AtDistances {
                distances_mm: vec![3.0, 57.0],
            },
        ),
        (rect(), Side::Inside, TabPlacement::Count { count: 2 }),
        (rect(), Side::Outside, TabPlacement::Count { count: 2 }),
    ] {
        for entering in [Entering::Plunge, Entering::Ramp] {
            for shape in [TabShape::Rect, TabShape::Smooth] {
                for reverse in [false, true] {
                    let case = Case {
                        drawing: drawing.clone(),
                        side,
                        direction: CutDirection::Climb,
                        entering,
                        allowance_mm: 0.5,
                        thickness_mm: None,
                        reverse,
                        tabs: Some(TabSpec {
                            placement: placement.clone(),
                            width_mm: 8.0,
                            height_mm: 6.0,
                            ..two_tabs(shape)
                        }),
                    };
                    let measured = check(&case);
                    assert!(measured.is_some(), "{} is refused", case.name());
                    if let Some(m) = measured {
                        deepest_mm = deepest_mm.max(m.deepest_plunge_mm);
                    }
                }
            }
        }
    }
    println!("the deepest plunge into uncut material: {deepest_mm:.3} mm");
}
