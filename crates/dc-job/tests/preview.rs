// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The preview's marks (`docs/specs/phase-4/design.md`, "Test plan → dc-job
//! (slice 1)"): geometry the Viewport can draw, equal to what the SVG
//! draws. That the SVG itself is unchanged is
//! `crates/dreamcarve/tests/pipeline_move.rs`, against the binary.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_geometry::Point;
use dc_job::preview::{Run, marks, render};
use dc_job::{EmitOp, ProgramSplit};
use dc_project::{Board, Project};

use common::{DATE, load_dcv};

/// For every run of every committed project, the tab marks, start points
/// and holes [`marks`] returns are what the SVG of that run alone draws:
/// one `tab` path per tab mark, from its first to its last vertex, an arc
/// where the mark follows an arc; one `start` circle per start point, with
/// a tick where it has a direction; one `hole` circle per hole; all at the
/// mark's place, Y flipped once, with the bit's radius (functional spec,
/// acceptance 79).
#[test]
fn marks_equal_what_the_svg_draws() {
    let mut tab_marks = 0;
    let mut arcs = 0;
    let mut starts = 0;
    let mut holes = 0;
    for (stem, path) in common::checked_projects() {
        let project = load_dcv(&path);
        let built = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
        let generated = built
            .generated
            .unwrap_or_else(|| panic!("{stem}: {:?}", built.findings));
        for entry in &generated {
            let run = Run::from(entry);
            let marks = marks(&run);
            let svg = render(&project, &[run]);
            let frame = frame_of(&project, entry);
            let case = format!("{stem} {}", entry.label.naming(entry.op.id().as_str()));

            let tabs = elements(&svg, "tab");
            assert_eq!(tabs.len(), marks.tab_marks.len(), "{case}: tab marks");
            for (element, polyline) in tabs.iter().zip(&marks.tab_marks) {
                let d = attr(element, "d");
                let numbers: Vec<&str> = d
                    .split(' ')
                    .filter(|t| !matches!(*t, "M" | "L" | "A" | "Z"))
                    .collect();
                let first = polyline.vertices().first().unwrap().point();
                let last = polyline.vertices().last().unwrap().point();
                assert_eq!(
                    (numbers[0].to_owned(), numbers[1].to_owned()),
                    frame.xy(first),
                    "{case}: tab mark starts at {d}"
                );
                assert_eq!(
                    (
                        numbers[numbers.len() - 2].to_owned(),
                        numbers[numbers.len() - 1].to_owned()
                    ),
                    frame.xy(last),
                    "{case}: tab mark ends at {d}"
                );
                if polyline.segments().any(|s| s.arc().is_some()) {
                    assert!(d.contains(" A "), "{case}: an arc mark is an arc: {d}");
                    arcs += 1;
                }
                tab_marks += 1;
            }

            let circles = elements(&svg, "start");
            assert_eq!(
                circles.len(),
                marks.start_points.len(),
                "{case}: start points"
            );
            for (element, start) in circles.iter().zip(&marks.start_points) {
                let (cx, cy) = frame.xy(start.at);
                assert_eq!(
                    (
                        attr(element, "cx").to_owned(),
                        attr(element, "cy").to_owned()
                    ),
                    (cx, cy),
                    "{case}"
                );
                assert_eq!(attr(element, "r"), fmt(start.radius_mm), "{case}");
                starts += 1;
            }
            let ticks = elements(&svg, "start-tick");
            let with_direction: Vec<_> = marks
                .start_points
                .iter()
                .filter_map(|s| s.direction.map(|d| (s.at, d, s.radius_mm)))
                .collect();
            assert_eq!(ticks.len(), with_direction.len(), "{case}: ticks");
            for (element, (at, direction, radius_mm)) in ticks.iter().zip(with_direction) {
                let end = at + direction * (2.0 * radius_mm);
                let (x0, y0) = frame.xy(at);
                let (x1, y1) = frame.xy(end);
                assert_eq!(
                    attr(element, "d"),
                    format!("M {x0} {y0} L {x1} {y1}"),
                    "{case}"
                );
            }

            let drilled = elements(&svg, "hole");
            assert_eq!(drilled.len(), marks.holes.len(), "{case}: holes");
            for (element, (at, radius_mm)) in drilled.iter().zip(&marks.holes) {
                let (cx, cy) = frame.xy(*at);
                assert_eq!(
                    (
                        attr(element, "cx").to_owned(),
                        attr(element, "cy").to_owned()
                    ),
                    (cx, cy),
                    "{case}"
                );
                assert_eq!(attr(element, "r"), fmt(*radius_mm), "{case}");
                holes += 1;
            }
        }
    }
    assert!(
        tab_marks > 0 && arcs > 0 && starts > 0 && holes > 0,
        "{tab_marks} tab marks ({arcs} on arcs), {starts} start points, {holes} holes"
    );
}

/// The generated entries of committed project `stem`, with the project.
fn generated(stem: &str) -> (Project, Vec<EmitOp>) {
    let (_, path) = common::checked_projects()
        .into_iter()
        .find(|(name, _)| name == stem)
        .unwrap_or_else(|| panic!("{stem} is a committed project"));
    let project = load_dcv(&path);
    let built = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
    let generated = built
        .generated
        .unwrap_or_else(|| panic!("{stem}: {:?}", built.findings));
    (project, generated)
}

/// A Photo V-Carve's marks are its grooves' (slice D7): one start point
/// where the bit plunges into each groove, and no tab mark, since it
/// leaves no tab. A groove's depth follows the picture, and the bit goes
/// from one groove to the next at feed above the stock (`dc_cam::photo`,
/// "Moves"); read as level passes, every step deeper along a groove was a
/// start circle and every rise a tab mark. The groove count is the one
/// `check` prints (`Fact::Grooves`).
#[test]
fn a_photo_v_carve_has_one_start_point_per_groove_and_no_tab_mark() {
    use dc_job::summary::{Fact, carve_facts};

    for stem in ["photo-raster-metric", "photo-raster-inch"] {
        let (project, generated) = generated(stem);
        let grooves: usize = carve_facts(&project, &generated)
            .iter()
            .flat_map(|carve| &carve.boards)
            .flat_map(|board| &board.facts)
            .filter_map(|fact| match fact {
                Fact::Grooves { count, .. } => Some(*count),
                _ => None,
            })
            .sum();
        assert!(grooves > 1, "{stem}: {grooves} grooves");
        let photo: Vec<&EmitOp> = generated
            .iter()
            .filter(|e| matches!(e.op, dc_project::Operation::PhotoVCarve(_)))
            .collect();
        assert_eq!(photo.len(), 1, "{stem}: one Photo V-Carve run");
        let entry = photo[0];
        let marks = marks(&Run::from(entry));
        assert!(
            marks.tab_marks.is_empty(),
            "{stem}: {} tab marks on a cut that leaves no tab",
            marks.tab_marks.len()
        );
        assert_eq!(
            marks.start_points.len(),
            grooves,
            "{stem}: a start point per groove"
        );
        // Each is where a groove's plunge starts: a cutting move straight
        // down from there.
        for start in &marks.start_points {
            let plunges = entry.toolpath.moves.windows(2).any(|pair| {
                let (from, m) = (pair[0].to, &pair[1]);
                m.is_cut()
                    && m.to.z < from.z
                    && Point::new(from.x, from.y).distance_to(start.at) < 1e-9
                    && Point::new(m.to.x, m.to.y).distance_to(start.at) < 1e-9
            });
            assert!(plunges, "{stem}: no plunge at {:?}", start.at);
        }
    }
}

/// Only a cut that leaves tabs draws tab marks: a Profile, and an Inlay's
/// cut-out round its plug. A carve's depth follows its art, so its climbs
/// are not tabs (the sign's Lettering drew 98); a Pocket, a Bore, an
/// Engrave and a clearing pass leave none. And a V-Carve or an Inlay
/// still has one start point per place the bit comes down from a rapid.
#[test]
fn only_a_cut_that_leaves_tabs_has_tab_marks() {
    use dc_job::RunLabel;
    use dc_project::Operation;

    let mut tabbed = 0;
    let mut carved = 0;
    for (stem, path) in common::checked_projects() {
        let project = load_dcv(&path);
        let built = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
        let generated = built
            .generated
            .unwrap_or_else(|| panic!("{stem}: {:?}", built.findings));
        for entry in &generated {
            let case = format!("{stem} {}", entry.label.naming(entry.op.id().as_str()));
            let marks = marks(&Run::from(entry));
            let leaves_tabs = matches!(
                (&entry.op, &entry.label),
                (Operation::Profile(_), _) | (Operation::Inlay(_), RunLabel::CutOut(_))
            );
            if leaves_tabs {
                tabbed += marks.tab_marks.len();
            } else {
                assert!(
                    marks.tab_marks.is_empty(),
                    "{case}: {} tab marks",
                    marks.tab_marks.len()
                );
            }
            let carve = matches!(
                (&entry.op, &entry.label),
                (Operation::VCarve(_) | Operation::Inlay(_), RunLabel::Main)
            );
            if carve {
                // Each place the bit comes down: over at safe Z, down in
                // the air, then the plunge.
                let rapids = entry.toolpath.moves.iter().filter(|m| m.is_rapid()).count();
                assert_eq!(marks.start_points.len(), rapids / 3, "{case}");
                carved += marks.start_points.len();
            }
        }
    }
    assert!(
        tabbed > 0 && carved > 0,
        "{tabbed} tab marks, {carved} carve starts"
    );
}

/// Where a run's board is drawn: the stock at X 0, the male board to its
/// right (#498), both with Y flipped at the picture's height.
struct Frame {
    height_mm: f64,
    x_mm: f64,
}

impl Frame {
    fn xy(&self, p: Point) -> (String, String) {
        (fmt(self.x_mm + p.x), fmt(self.height_mm - p.y))
    }
}

fn frame_of(project: &Project, entry: &EmitOp) -> Frame {
    match (entry.board, project.male_board()) {
        (Board::Male, Some((_, board))) => Frame {
            height_mm: project.stock.height_mm.max(board.height_mm),
            x_mm: project.stock.width_mm + 10.0,
        },
        _ => Frame {
            height_mm: project.stock.height_mm,
            x_mm: 0.0,
        },
    }
}

/// A number as the preview writes it: three decimals, no `-0.000`.
fn fmt(value: f64) -> String {
    let s = format!("{value:.3}");
    match s.strip_prefix('-') {
        Some(unsigned) if unsigned.chars().all(|c| c == '0' || c == '.') => unsigned.to_owned(),
        _ => s,
    }
}

/// The text of every element whose `class` is `class`.
fn elements<'a>(svg: &'a str, class: &str) -> Vec<&'a str> {
    let wanted = format!(" class=\"{class}\"");
    svg.split('<').filter(|e| e.contains(&wanted)).collect()
}

/// The value of attribute `name` in `element`.
fn attr<'a>(element: &'a str, name: &str) -> &'a str {
    let key = format!(" {name}=\"");
    let start = element.find(&key).unwrap() + key.len();
    let rest = &element[start..];
    &rest[..rest.find('"').unwrap()]
}
