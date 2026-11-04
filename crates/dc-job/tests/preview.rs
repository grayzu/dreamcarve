// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The preview after the move (`docs/specs/phase-4/design.md`, "Test plan →
//! dc-job (slice 1)"): the same bytes, and marks the Viewport can draw.

#![allow(
    clippy::panic,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    reason = "test code reports failure by panicking"
)]

mod common;

use dc_geometry::Point;
use dc_job::preview::{Run, marks, render, render_generated};
use dc_job::{EmitOp, ProgramSplit};
use dc_project::{Board, Project};

use common::{DATE, expected, first_difference, load_dcv, read};

/// Every committed project with a committed preview, rendered from
/// [`dc_job::build`]'s runs, is byte for byte the SVG `check --preview`
/// wrote before the move (`expected/quick-start/`, `expected/golden/`,
/// `expected/corpus/`): straight and arc tab marks, start points, holes,
/// the male board, the legend.
#[test]
fn render_is_unchanged_after_the_move() {
    let groups = [
        ("quick-start", common::quick_start_projects()),
        ("golden", common::golden_projects()),
        ("corpus", common::corpus_projects()),
    ];
    let mut compared = 0;
    for (group, fixtures) in groups {
        for (stem, path) in fixtures {
            let project = load_dcv(&path);
            let built = dc_job::build(&project, ProgramSplit::ProjectDefault, DATE);
            let generated = built
                .generated
                .unwrap_or_else(|| panic!("{group}/{stem}: {:?}", built.findings));
            let svg = render_generated(&project, &generated);
            let committed = read(&expected(group, &format!("{stem}.svg")));
            assert!(
                svg == committed,
                "{group}/{stem}: the preview differs from the one the CLI wrote before the move: {}",
                first_difference(&svg, &committed)
            );
            compared += 1;
        }
    }
    assert!(compared >= 25, "{compared} previews compared");
}

/// For every run of every committed project, the tab marks, start points
/// and holes [`marks`] returns are what the SVG of that run alone draws:
/// one `tab` path per tab mark, from its first to its last vertex; one
/// `start` circle per start point, with a tick where it has a direction;
/// one `hole` circle per hole; all at the mark's place, Y flipped once,
/// with the bit's radius (functional spec, acceptance 79).
#[test]
fn marks_equal_what_the_svg_draws() {
    let mut tab_marks = 0;
    let mut arcs = 0;
    let mut starts = 0;
    let mut holes = 0;
    for (stem, path) in common::quick_start_projects()
        .into_iter()
        .chain(common::golden_projects())
        .chain(common::corpus_projects())
    {
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
