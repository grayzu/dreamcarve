// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/designPrint/svg.ts

//! The toolpath preview SVG of `export --preview` and `check --preview`
//! (functional spec, capability 31; `docs/specs/phase-2/design.md`, "Phase
//! 2 addendum (#73) → A5").

use dc_cam::Toolpath;
use dc_project::{Operation, Project, Tool};

/// A deterministic SVG of the project's stock, shapes and each enabled
/// operation's toolpath, in mm, Y flipped for display.
pub fn render(project: &Project, ops: &[(&Operation, &Tool, &Toolpath)]) -> String {
    let _ = (project, ops);
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_cam::{Move, MoveKind};
    use dc_geometry::{Point, Point3};
    use dc_project::test_support::sample_project;

    /// The feed of every cutting move in these tests, mm/min.
    const FEED_MM_MIN: f64 = 1000.0;

    fn rapid(x: f64, y: f64, z: f64) -> Move {
        Move::rapid(Point3::new(x, y, z))
    }

    fn cut(x: f64, y: f64, z: f64) -> Move {
        Move::linear(Point3::new(x, y, z), FEED_MM_MIN)
    }

    /// An arc to (`x`, `y`, `z`) about `centre`, counter-clockwise if `ccw`.
    fn arc(ccw: bool, x: f64, y: f64, z: f64, centre: (f64, f64)) -> Move {
        let centre = Point::new(centre.0, centre.1);
        Move {
            kind: if ccw {
                MoveKind::ArcCcw { centre }
            } else {
                MoveKind::ArcCw { centre }
            },
            to: Point3::new(x, y, z),
            feed_mm_min: Some(FEED_MM_MIN),
        }
    }

    /// `toolpath` rendered as the sample project's one operation, with
    /// its 6.35 mm tool. The stock is 100 mm high, so `y_svg = 100 − y`.
    fn render_one(toolpath: &Toolpath) -> String {
        let project = sample_project();
        render(
            &project,
            &[(&project.operations[0], &project.tools[0], toolpath)],
        )
    }

    /// The text of every element whose `class` is `class`, from its name
    /// to its closing `>`.
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

    /// Down at (10, 20) and one straight cut to (30, 25).
    fn one_cut() -> Toolpath {
        Toolpath {
            moves: vec![
                rapid(10.0, 20.0, 10.0),
                cut(10.0, 20.0, 0.0),
                cut(30.0, 25.0, 0.0),
                rapid(30.0, 25.0, 10.0),
            ],
        }
    }

    /// One square lap from (10, 10) to (90, 90) per pass at `pass_z`
    /// (counter-clockwise), descending straight down into each. Below
    /// `tab_top_z` the lap rises over two rect tabs: x 40..50 on the
    /// bottom side and x 50..40 on the top side.
    fn tab_lap(pass_z: &[f64], tab_top_z: f64) -> Toolpath {
        let mut moves = vec![rapid(10.0, 10.0, 10.0)];
        for &z in pass_z {
            moves.push(cut(10.0, 10.0, z));
            let raised = z < tab_top_z;
            let mut tab = |moves: &mut Vec<Move>, x0: f64, x1: f64, y: f64| {
                if raised {
                    moves.push(cut(x0, y, z));
                    moves.push(cut(x0, y, tab_top_z));
                    moves.push(cut(x1, y, tab_top_z));
                    moves.push(cut(x1, y, z));
                }
            };
            tab(&mut moves, 40.0, 50.0, 10.0);
            moves.push(cut(90.0, 10.0, z));
            moves.push(cut(90.0, 90.0, z));
            tab(&mut moves, 50.0, 40.0, 90.0);
            moves.push(cut(10.0, 90.0, z));
            moves.push(cut(10.0, 10.0, z));
        }
        moves.push(rapid(10.0, 10.0, 10.0));
        Toolpath { moves }
    }

    #[test]
    fn flips_y_once() {
        let svg = render_one(&one_cut());
        let cuts = elements(&svg, "cut");
        assert_eq!(cuts.len(), 1, "{svg}");
        // (10, 20) and (30, 25) at y_svg = 100 − y, written once each.
        assert_eq!(attr(cuts[0], "d"), "M 10.000 80.000 L 30.000 75.000");
        assert!(!svg.contains("transform"), "{svg}");
        // The frame is the stock, in millimetres.
        assert!(svg.contains(" viewBox=\"0 0 100.000 100.000\""), "{svg}");
        assert!(
            svg.contains(" width=\"100.000mm\" height=\"100.000mm\""),
            "{svg}"
        );
        // The sample's 50 × 30 rectangle at (10, 10): its corner (10, 10)
        // is drawn at (10, 90).
        let shapes = elements(&svg, "shape");
        assert_eq!(shapes.len(), 1, "{svg}");
        assert!(
            attr(shapes[0], "d").starts_with("M 10.000 90.000 L 60.000 90.000"),
            "{svg}"
        );
    }

    #[test]
    fn ccw_arc_has_sweep_zero() {
        let quarter = |ccw: bool, to: (f64, f64)| Toolpath {
            moves: vec![
                rapid(60.0, 50.0, 10.0),
                cut(60.0, 50.0, 0.0),
                arc(ccw, to.0, to.1, 0.0, (50.0, 50.0)),
            ],
        };
        // Counter-clockwise from (60, 50) to (50, 60), Y-up.
        let svg = render_one(&quarter(true, (50.0, 60.0)));
        let d = attr(elements(&svg, "cut")[0], "d").to_owned();
        assert_eq!(d, "M 60.000 50.000 A 10.000 10.000 0 0 0 50.000 40.000");
        // Clockwise from (60, 50) to (50, 40).
        let svg = render_one(&quarter(false, (50.0, 40.0)));
        let d = attr(elements(&svg, "cut")[0], "d").to_owned();
        assert_eq!(d, "M 60.000 50.000 A 10.000 10.000 0 0 1 50.000 60.000");
    }

    #[test]
    fn arc_over_180_is_split() {
        let path = |last: Move| Toolpath {
            moves: vec![rapid(60.0, 50.0, 10.0), cut(60.0, 50.0, 0.0), last],
        };
        // A full circle: an arc that ends where it starts.
        let svg = render_one(&path(arc(true, 60.0, 50.0, 0.0, (50.0, 50.0))));
        let d = attr(elements(&svg, "cut")[0], "d").to_owned();
        assert_eq!(
            d,
            "M 60.000 50.000 A 10.000 10.000 0 0 0 40.000 50.000 A 10.000 10.000 0 0 0 60.000 50.000"
        );
        // Three quarters clockwise, from (60, 50) to (50, 60): two halves of
        // 135°, neither with the large-arc flag.
        let svg = render_one(&path(arc(false, 50.0, 60.0, 0.0, (50.0, 50.0))));
        let d = attr(elements(&svg, "cut")[0], "d").to_owned();
        assert_eq!(d.matches(" A ").count(), 2, "{d}");
        assert_eq!(d.matches(" 0 0 1 ").count(), 2, "{d}");
        assert!(d.ends_with(" 50.000 40.000"), "{d}");
    }

    #[test]
    fn tab_marks_raised_level_moves() {
        // Tab top at Z 3: the pass at 4 is above it, the passes at 2 and 0
        // rise over both tabs.
        let svg = render_one(&tab_lap(&[4.0, 2.0, 0.0], 3.0));
        let tabs = elements(&svg, "tab");
        assert_eq!(
            tabs.len(),
            4,
            "two tabs on each of two raised passes: {svg}"
        );
        let ds: Vec<&str> = tabs.iter().map(|t| attr(t, "d")).collect();
        assert_eq!(
            ds,
            [
                "M 40.000 90.000 L 50.000 90.000",
                "M 50.000 10.000 L 40.000 10.000",
                "M 40.000 90.000 L 50.000 90.000",
                "M 50.000 10.000 L 40.000 10.000",
            ]
        );
        // Tab marks are heavier than the cut.
        let cut_width: f64 = attr(elements(&svg, "cut")[0], "stroke-width")
            .parse()
            .unwrap();
        let tab_width: f64 = attr(tabs[0], "stroke-width").parse().unwrap();
        assert!(tab_width > 2.0 * cut_width, "{tab_width} vs {cut_width}");
        // A lap with no tab gives no mark.
        let svg = render_one(&tab_lap(&[4.0], 3.0));
        assert!(elements(&svg, "tab").is_empty(), "{svg}");
    }

    #[test]
    fn start_circle_per_run() {
        let mut toolpath = tab_lap(&[4.0, 2.0, 0.0], 3.0);
        // A second run, after a rapid: down at (20, 50), cutting along +Y.
        toolpath.moves.extend([
            rapid(20.0, 50.0, 10.0),
            cut(20.0, 50.0, 0.0),
            cut(20.0, 70.0, 0.0),
            rapid(20.0, 70.0, 10.0),
        ]);
        let svg = render_one(&toolpath);
        let circles = elements(&svg, "start");
        // One per descent: three passes of the lap (the tab drops are not
        // descents), and the second run.
        assert_eq!(circles.len(), 4, "{svg}");
        for circle in &circles[..3] {
            assert_eq!(
                (attr(circle, "cx"), attr(circle, "cy"), attr(circle, "r")),
                ("10.000", "90.000", "3.175")
            );
        }
        assert_eq!(
            (attr(circles[3], "cx"), attr(circles[3], "cy")),
            ("20.000", "50.000")
        );
        // Each tick is one diameter long along the first cut: +X, then +Y
        // (up the page).
        let ticks: Vec<&str> = elements(&svg, "start-tick")
            .iter()
            .map(|t| attr(t, "d"))
            .collect();
        assert_eq!(ticks.len(), 4, "{svg}");
        assert_eq!(ticks[0], "M 10.000 90.000 L 16.350 90.000");
        assert_eq!(ticks[3], "M 20.000 50.000 L 20.000 43.650");
    }

    #[test]
    fn escapes_names() {
        let mut project = sample_project();
        if let Operation::Profile(op) = &mut project.operations[0] {
            op.name = "<a & \"b\">".into();
        }
        let toolpath = one_cut();
        let svg = render(
            &project,
            &[(&project.operations[0], &project.tools[0], &toolpath)],
        );
        assert!(svg.contains("&lt;a &amp; &quot;b&quot;&gt;"), "{svg}");
        assert!(!svg.contains("<a &"), "{svg}");
        // The legend names the operation by id and name, in its colour.
        let legend = elements(&svg, "legend-entry");
        assert_eq!(legend.len(), 1, "{svg}");
        assert!(legend[0].contains("o1 &lt;a &amp;"), "{svg}");
    }

    #[test]
    fn is_deterministic() {
        let toolpath = tab_lap(&[4.0, 2.0, 0.0], 3.0);
        let first = render_one(&toolpath);
        assert!(first.starts_with("<?xml"), "{first}");
        assert_eq!(first, render_one(&toolpath));
        // And the sample's real toolpath.
        let project = sample_project();
        let Operation::Profile(op) = &project.operations[0] else {
            panic!("the sample cuts a Profile");
        };
        let (toolpath, _) = dc_cam::generate_profile(&project, op).unwrap();
        let ops = [(&project.operations[0], &project.tools[0], &toolpath)];
        assert_eq!(render(&project, &ops), render(&project, &ops));
    }
}
