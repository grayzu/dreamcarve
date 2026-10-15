// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Figures for the user guide, drawn from what the engine actually returns.
//! Each nesting figure runs [`dc_geometry::nest`] on the same shapes as a
//! unit test (or on an untested edge case) and draws the result, so a figure
//! can only show behaviour the code really has.

use dc_geometry::{Contour, EPS_MM, Point, Polyline, Rect, Region, nest};
use std::fmt::Write as _;

/// One generated figure: where it lives under the repo root and its SVG.
pub struct Figure {
    /// Path relative to the repository root.
    pub path: &'static str,
    /// The SVG document.
    pub svg: String,
}

/// Every generated figure, in a fixed order.
#[must_use]
pub fn all() -> Vec<Figure> {
    nesting_cases()
        .into_iter()
        .map(|case| Figure {
            path: case.path,
            svg: render_nesting(&case.contours),
        })
        .collect()
}

struct Case {
    path: &'static str,
    contours: Vec<Contour>,
}

fn circle(x: f64, y: f64, r: f64) -> Contour {
    contour(Polyline::circle(Point::new(x, y), r))
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Contour {
    contour(Polyline::rect(Point::new(x, y), w, h))
}

#[allow(clippy::expect_used, reason = "fixed, known-good figure geometry")]
fn contour(p: Polyline) -> Contour {
    Contour::new(p).expect("figure geometry is a valid contour")
}

macro_rules! case {
    ($file:literal, $contours:expr) => {
        Case {
            path: concat!("docs/book/src/user-guide/figures/nesting/", $file),
            contours: $contours,
        }
    };
}

/// The nesting cases. The first group mirrors `dc_geometry::region` unit
/// tests shape for shape; the second shows edge cases no test pins down yet.
fn nesting_cases() -> Vec<Case> {
    vec![
        // Mirrors region::tests.
        case!(
            "ring.svg",
            vec![circle(0.0, 0.0, 10.0), circle(0.0, 0.0, 5.0)]
        ),
        case!(
            "disjoint.svg",
            vec![rect(0.0, 0.0, 10.0, 10.0), rect(20.0, 0.0, 10.0, 5.0)]
        ),
        case!(
            "island.svg",
            vec![
                circle(0.0, 0.0, 10.0),
                circle(0.0, 0.0, 7.0),
                circle(0.0, 0.0, 3.0)
            ]
        ),
        case!(
            "four-levels.svg",
            vec![
                rect(0.0, 0.0, 40.0, 40.0),
                rect(5.0, 5.0, 30.0, 30.0),
                rect(10.0, 10.0, 20.0, 20.0),
                rect(15.0, 15.0, 10.0, 10.0),
            ]
        ),
        case!(
            "inside-arc-bulge.svg",
            vec![circle(0.0, 0.0, 10.0), circle(0.0, 6.0, 2.0)]
        ),
        case!(
            "outside-arc-bulge.svg",
            vec![circle(0.0, 0.0, 10.0), circle(9.0, 9.0, 1.0)]
        ),
        case!(
            "several-holes.svg",
            vec![
                rect(0.0, 0.0, 100.0, 50.0),
                circle(20.0, 25.0, 5.0),
                circle(50.0, 25.0, 3.0),
                circle(80.0, 25.0, 4.0),
            ]
        ),
        case!(
            "region-order.svg",
            vec![
                circle(50.0, 0.0, 2.0),
                rect(100.0, 0.0, 5.0, 5.0),
                circle(50.0, 0.0, 8.0),
            ]
        ),
        case!(
            "order-independent.svg",
            vec![
                circle(0.0, 0.0, 10.0),
                circle(0.0, 0.0, 7.0),
                circle(0.0, 0.0, 3.0),
                rect(30.0, 0.0, 5.0, 5.0),
            ]
        ),
        case!(
            "touching-edge.svg",
            vec![rect(0.0, 0.0, 10.0, 10.0), rect(0.0, 2.0, 5.0, 2.0)]
        ),
        case!(
            "clockwise-input.svg",
            vec![
                rect(0.0, 0.0, 10.0, 10.0).reversed(),
                rect(2.0, 2.0, 2.0, 2.0)
            ]
        ),
        // Edge cases without a unit test.
        case!(
            "two-parts-two-holes.svg",
            vec![
                rect(0.0, 0.0, 30.0, 30.0),
                circle(55.0, 15.0, 8.0),
                rect(40.0, 0.0, 30.0, 30.0),
                circle(15.0, 15.0, 8.0),
            ]
        ),
        case!(
            "identical.svg",
            vec![rect(0.0, 0.0, 20.0, 10.0), rect(0.0, 0.0, 20.0, 10.0)]
        ),
        case!(
            "crossing.svg",
            vec![circle(0.0, 0.0, 10.0), circle(8.0, 0.0, 5.0)]
        ),
    ]
}

/// Fills for regions R1, R2, …; light enough to read strokes over.
const REGION_FILLS: [&str; 4] = ["#cfe3ff", "#ffe2b8", "#d4f2cc", "#f1d3f5"];
const OUTER_STROKE: &str = "#0b5cd5";
const HOLE_STROKE: &str = "#c62828";
const INK: &str = "#1f2328";

const DRAW_W_PX: f64 = 380.0;
const DRAW_H_MAX_PX: f64 = 240.0;
const MARGIN_PX: f64 = 24.0;
const LINE_PX: f64 = 18.0;

/// Maps project millimetres (Y-up) to SVG pixels (Y-down).
struct View {
    min: Point,
    max_y: f64,
    scale: f64,
    offset_x: f64,
}

impl View {
    fn fit(bounds: Rect) -> Self {
        let w = bounds.width_mm().max(EPS_MM);
        let h = bounds.height_mm().max(EPS_MM);
        let scale = (DRAW_W_PX / w).min(DRAW_H_MAX_PX / h);
        Self {
            min: bounds.min,
            max_y: bounds.max.y,
            scale,
            offset_x: MARGIN_PX + (DRAW_W_PX - w * scale) / 2.0,
        }
    }

    fn x(&self, x_mm: f64) -> f64 {
        self.offset_x + (x_mm - self.min.x) * self.scale
    }

    fn y(&self, y_mm: f64) -> f64 {
        MARGIN_PX + (self.max_y - y_mm) * self.scale
    }
}

/// Fixed two-decimal formatting with negative zero normalised.
fn num(v: f64) -> String {
    let s = format!("{v:.2}");
    if s == "-0.00" { "0.00".to_owned() } else { s }
}

fn path_data(c: &Contour, v: &View) -> String {
    let pl = c.polyline();
    let mut d = String::new();
    if let Some(first) = pl.vertices().first() {
        let _ = write!(d, "M{} {}", num(v.x(first.x)), num(v.y(first.y)));
    }
    for seg in pl.segments() {
        let (x, y) = (num(v.x(seg.end.x)), num(v.y(seg.end.y)));
        match seg.arc() {
            Some(arc) => {
                let r = num(arc.radius_mm * v.scale);
                let large = u8::from(arc.sweep_rad.abs() > std::f64::consts::PI);
                // Y-up counter-clockwise reads clockwise on a Y-down screen,
                // which is SVG's positive sweep direction.
                let sweep = u8::from(arc.sweep_rad > 0.0);
                let _ = write!(d, " A{r} {r} 0 {large} {sweep} {x} {y}");
            }
            None => {
                let _ = write!(d, " L{x} {y}");
            }
        }
    }
    d.push_str(" Z");
    d
}

/// A small arrowhead showing travel direction a quarter of the way round.
fn arrow(c: &Contour, v: &View, colour: &str) -> String {
    let pl = c.polyline();
    let Some((p, t)) = pl.point_at(pl.length_mm() * 0.2) else {
        return String::new();
    };
    // Screen-space tangent (flip Y), then a triangle pointing along it.
    let (tx, ty) = (t.x, -t.y);
    let (cx, cy) = (v.x(p.x), v.y(p.y));
    let (len, half) = (9.0, 4.5);
    let tip = (cx + tx * len / 2.0, cy + ty * len / 2.0);
    let base = (cx - tx * len / 2.0, cy - ty * len / 2.0);
    let left = (base.0 - ty * half, base.1 + tx * half);
    let right = (base.0 + ty * half, base.1 - tx * half);
    format!(
        "<path d=\"M{} {} L{} {} L{} {} Z\" fill=\"{colour}\"/>",
        num(tip.0),
        num(tip.1),
        num(left.0),
        num(left.1),
        num(right.0),
        num(right.1)
    )
}

/// The input index of each output contour, matched by bounds and area.
fn input_index(c: &Contour, inputs: &[Contour], used: &mut [bool]) -> usize {
    let same = |a: &Contour, b: &Contour| {
        let (Some(ba), Some(bb)) = (a.polyline().bounds(), b.polyline().bounds()) else {
            return false;
        };
        let close = |x: f64, y: f64| (x - y).abs() <= 1e-6;
        close(ba.min.x, bb.min.x)
            && close(ba.min.y, bb.min.y)
            && close(ba.max.x, bb.max.x)
            && close(ba.max.y, bb.max.y)
            && close(a.signed_area_mm2().abs(), b.signed_area_mm2().abs())
    };
    let found = inputs
        .iter()
        .zip(used.iter())
        .position(|(input, &u)| !u && same(c, input));
    if let Some(i) = found
        && let Some(flag) = used.get_mut(i)
    {
        *flag = true;
    }
    found.unwrap_or(usize::MAX)
}

fn winding(c: &Contour) -> &'static str {
    if c.is_ccw() { "CCW" } else { "CW" }
}

fn render_nesting(inputs: &[Contour]) -> String {
    let regions: Vec<Region> = nest(inputs.to_vec());

    let bounds = inputs
        .iter()
        .filter_map(|c| c.polyline().bounds())
        .reduce(|a, b| a.including(b.min).including(b.max))
        .unwrap_or(Rect::from_point(Point::new(0.0, 0.0)));
    let view = View::fit(bounds);
    let draw_h = bounds.height_mm() * view.scale;

    let mut used = vec![false; inputs.len()];
    let mut shapes = String::new();
    let mut strokes = String::new();
    let mut labels: Vec<(f64, f64, String)> = Vec::new();
    let mut legend: Vec<(usize, String)> = Vec::new();

    for (k, region) in regions.iter().enumerate() {
        let fill = REGION_FILLS.get(k % REGION_FILLS.len()).unwrap_or(&"#ddd");
        let mut d = path_data(region.outer(), &view);
        for hole in region.holes() {
            d.push(' ');
            d.push_str(&path_data(hole, &view));
        }
        let _ = write!(
            shapes,
            "<path d=\"{d}\" fill=\"{fill}\" fill-rule=\"evenodd\" stroke=\"none\"/>"
        );

        let mut describe = |c: &Contour, colour: &str, dashed: bool| -> usize {
            let i = input_index(c, inputs, &mut used);
            let dash = if dashed {
                " stroke-dasharray=\"6 3\""
            } else {
                ""
            };
            let _ = write!(
                strokes,
                "<path d=\"{}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"2\"{dash}/>{}",
                path_data(c, &view),
                arrow(c, &view, colour)
            );
            if let Some(b) = c.polyline().bounds() {
                let (x, y) = (view.x((b.min.x + b.max.x) / 2.0), view.y(b.max.y) - 4.0);
                match labels
                    .iter_mut()
                    .find(|(lx, ly, _)| (lx - x).abs() < 1.0 && (ly - y).abs() < 1.0)
                {
                    Some((_, _, text)) => {
                        let _ = write!(text, ", #{i}");
                    }
                    None => labels.push((x, y, format!("#{i}"))),
                }
            }
            i
        };

        let outer = describe(region.outer(), OUTER_STROKE, false);
        let mut line = format!("R{}: outer #{outer} ({})", k + 1, winding(region.outer()));
        let holes: Vec<String> = region
            .holes()
            .iter()
            .map(|h| format!("#{} ({})", describe(h, HOLE_STROKE, true), winding(h)))
            .collect();
        if holes.is_empty() {
            line.push_str(", no holes");
        } else {
            let _ = write!(line, ", holes {}", holes.join(", "));
        }
        legend.push((k, line));
    }

    let legend_top = MARGIN_PX + draw_h + 28.0;
    let count = regions.len();
    let total_h = legend_top + LINE_PX * (count as f64 + 1.0) + 6.0;
    let total_w = DRAW_W_PX + 2.0 * MARGIN_PX;

    let mut svg = String::new();
    let _ = write!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"system-ui, sans-serif\" font-size=\"12\">\n\
         <!-- Generated by tools/doc-figures from dc_geometry::nest. Do not edit; run `cargo run -p doc-figures`. -->\n\
         <rect width=\"{w}\" height=\"{h}\" fill=\"#ffffff\"/>\n",
        w = num(total_w),
        h = num(total_h)
    );
    svg.push_str(&shapes);
    svg.push('\n');
    svg.push_str(&strokes);
    svg.push('\n');
    for (x, y, text) in &labels {
        let _ = writeln!(
            svg,
            "<text x=\"{}\" y=\"{}\" text-anchor=\"middle\" fill=\"{INK}\" stroke=\"#ffffff\" stroke-width=\"3\" paint-order=\"stroke\">{text}</text>",
            num(*x),
            num(*y)
        );
    }
    let plural = if count == 1 { "region" } else { "regions" };
    let _ = writeln!(
        svg,
        "<text x=\"{}\" y=\"{}\" font-weight=\"600\" fill=\"{INK}\">nest() result: {count} {plural}</text>",
        num(MARGIN_PX),
        num(legend_top)
    );
    for (row, (k, line)) in legend.iter().enumerate() {
        let y = legend_top + LINE_PX * (row as f64 + 1.0);
        let fill = REGION_FILLS.get(k % REGION_FILLS.len()).unwrap_or(&"#ddd");
        let _ = writeln!(
            svg,
            "<rect x=\"{}\" y=\"{}\" width=\"12\" height=\"12\" fill=\"{fill}\" stroke=\"{OUTER_STROKE}\"/><text x=\"{}\" y=\"{}\" fill=\"{INK}\">{line}</text>",
            num(MARGIN_PX),
            num(y - 10.0),
            num(MARGIN_PX + 18.0),
            num(y)
        );
    }
    svg.push_str("</svg>\n");
    svg
}
