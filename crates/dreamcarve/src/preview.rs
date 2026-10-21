// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/designPrint/svg.ts

//! The toolpath preview SVG of `export --preview` and `check --preview`
//! (functional spec, capability 31; `docs/specs/phase-2/design.md`, "Phase
//! 2 addendum (#73) → A5").
//!
//! [`render`] draws the generated toolpath, not the G-code, so arc fitting
//! does not show. It is a pure function of the project and the toolpaths:
//! numbers have three decimals and nothing depends on the date or a random
//! id, so the same project gives the same bytes. The binary writes the
//! file. Here until the phase 4 GUI draws toolpaths itself.
//!
//! The picture is the stock at 1:1 in millimetres, seen from above. Project
//! space is Y-up and SVG is Y-down, so, as ADR-0001 asks of a renderer,
//! every Y is flipped here, once, as `y_svg = H − y` with `H` the stock
//! height; no `transform` is used, so the file's numbers read in drawing
//! terms. A counter-clockwise arc in Y-up still turns counter-clockwise on
//! the page, which is SVG's negative direction: sweep flag 0.
//!
//! From PureCutCNC's `svg.ts` come the frame, the stock outline, the shape
//! paths with arcs as `A`, the dashed rapids, `fmt` and `escapeXml`; its
//! page, print scaling, dimensions and labels are dropped. What a toolpath
//! shows is DreamCarve's:
//!
//! - **Cuts**: one solid path per run of cutting moves between rapids, in
//!   the operation's colour.
//! - **Tab marks**: within a run, the *floor* is the Z the last descent
//!   reached. A cutting move that starts or ends above the floor is over a
//!   tab, and each stretch of them is drawn again with a heavy stroke: one
//!   mark per tab per pass that rises over it.
//! - **Start points**: where each descent begins, from a rapid or from the
//!   floor of the pass before, a circle the size of the bit, with a tick
//!   one diameter long in the direction of the first cut after it.
//! - **Drill holes**: a Drill draws a circle of the bit at each hole
//!   instead.

use std::f64::consts::PI;
use std::fmt::Write as _;

use dc_cam::{Move, MoveKind, Toolpath};
use dc_geometry::{Arc, Point, Point3, Polyline, Vec2};
use dc_project::{Operation, Project, Tool};

/// Operation colours, by the operation's place in the preview, repeating
/// after eight. Distinct on white and from the black shapes.
const PALETTE: [&str; 8] = [
    "#1f77b4", "#d62728", "#2ca02c", "#9467bd", "#ff7f0e", "#17becf", "#8c564b", "#e377c2",
];
/// The stock outline, as PureCutCNC draws it: grey, dashed.
const STOCK_COLOUR: &str = "#7f7f7f";
const STOCK_STROKE_MM: f64 = 0.35;
const STOCK_DASH: &str = "2.5 1.5";
/// The shapes, thin black.
const SHAPE_STROKE_MM: f64 = 0.2;
/// Cutting moves.
const CUT_STROKE_MM: f64 = 0.3;
/// Rapids: grey, dashed, thinner than a cut.
const RAPID_COLOUR: &str = "#808080";
const RAPID_STROKE_MM: f64 = 0.18;
const RAPID_DASH: &str = "1.6 1.2";
/// A tab mark, heavy enough to read over its cut.
const TAB_STROKE_MM: f64 = 1.2;
/// Start circles, ticks and drill holes.
const MARK_STROKE_MM: f64 = 0.2;
/// Two Z values closer than this are one level, mm.
const Z_EPS_MM: f64 = 1e-6;
/// A move shorter than this in XY draws nothing, mm.
const XY_EPS_MM: f64 = 1e-9;

/// The comment on the second line of every preview. `--preview` writes
/// over an existing file only if it has this near its start, so a
/// mistyped path never replaces the user's drawing.
pub const MARKER: &str = "<!-- Toolpath preview written by DreamCarve -->";

/// A deterministic SVG of the project's stock, shapes and each enabled
/// operation's toolpath, in mm, Y flipped for display.
pub fn render(project: &Project, ops: &[(&Operation, &Tool, &Toolpath)]) -> String {
    let width = project.stock.width_mm;
    let height = project.stock.height_mm;
    let frame = Frame { height_mm: height };
    let mut svg = String::new();
    let _ = writeln!(svg, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(svg, "{MARKER}");
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">"#,
        w = fmt(width),
        h = fmt(height),
    );
    let _ = writeln!(
        svg,
        "<title>{}: toolpath preview</title>",
        escape_xml(&project.name)
    );
    let _ = writeln!(
        svg,
        r#"<rect class="stock" x="0.000" y="0.000" width="{}" height="{}" fill="none" stroke="{STOCK_COLOUR}" stroke-width="{}" stroke-dasharray="{STOCK_DASH}"/>"#,
        fmt(width),
        fmt(height),
        fmt(STOCK_STROKE_MM),
    );
    write_shapes(&mut svg, project, &frame);
    for (k, (op, tool, toolpath)) in ops.iter().enumerate() {
        let colour = colour(k);
        let drawing = Drawing::of(toolpath, matches!(op, Operation::Drill(_)), &frame);
        write_operation(&mut svg, &frame, op, tool, colour, &drawing);
    }
    write_legend(&mut svg, width, height, ops);
    svg.push_str("</svg>\n");
    svg
}

/// The colour of the `k`th operation.
fn colour(k: usize) -> &'static str {
    PALETTE.get(k % PALETTE.len()).copied().unwrap_or("#000000")
}

/// Where a project point is drawn.
struct Frame {
    /// The stock height, mm: `y_svg = height_mm − y`.
    height_mm: f64,
}

impl Frame {
    /// `p` as `x y` in SVG coordinates. The one place Y is flipped.
    fn xy(&self, p: Point) -> String {
        format!("{} {}", fmt(p.x), fmt(self.height_mm - p.y))
    }

    /// `p` as the attributes `cx="…" cy="…"`.
    fn centre_attrs(&self, p: Point) -> String {
        format!(r#"cx="{}" cy="{}""#, fmt(p.x), fmt(self.height_mm - p.y))
    }
}

/// `value` with three decimals, and never `-0.000` (PureCutCNC `fmt`,
/// which rounds to four places; three are a micron, finer than any
/// machine here).
fn fmt(value: f64) -> String {
    let s = format!("{value:.3}");
    match s.strip_prefix('-') {
        Some(unsigned) if unsigned.chars().all(|c| c == '0' || c == '.') => unsigned.to_owned(),
        _ => s,
    }
}

/// `text` safe in XML text and attribute values (PureCutCNC `escapeXml`).
fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// Appends the arc about `centre` from `from` (the current point) to `to`
/// sweeping `sweep_rad` (positive counter-clockwise, Y-up) as `A`
/// commands, split so no piece sweeps more than a half turn: an SVG arc
/// over 180° needs the large-arc flag, and one of a full turn draws
/// nothing.
fn push_arc(d: &mut String, frame: &Frame, from: Point, to: Point, centre: Point, sweep_rad: f64) {
    let radius = from.distance_to(centre);
    if radius <= XY_EPS_MM {
        let _ = write!(d, " L {}", frame.xy(to));
        return;
    }
    let start = (from.y - centre.y).atan2(from.x - centre.x);
    let pieces: u8 = if sweep_rad.abs() > PI + 1e-9 { 2 } else { 1 };
    // Counter-clockwise in Y-up is counter-clockwise on the page too,
    // which is SVG's negative-angle direction.
    let sweep_flag = if sweep_rad > 0.0 { 0 } else { 1 };
    for k in 1..=pieces {
        let end = if k == pieces {
            to
        } else {
            let angle = start + sweep_rad * f64::from(k) / f64::from(pieces);
            Point::new(
                centre.x + radius * angle.cos(),
                centre.y + radius * angle.sin(),
            )
        };
        let r = fmt(radius);
        let _ = write!(d, " A {r} {r} 0 0 {sweep_flag} {}", frame.xy(end));
    }
}

/// The signed sweep of an arc move from `from` to `to` about `centre`.
/// One that ends where it starts is a full turn, as G-code reads it.
fn move_sweep(from: Point, to: Point, centre: Point, ccw: bool) -> f64 {
    let sweep = Arc::from_ends(from, to, centre, ccw).sweep_rad;
    if from.distance_to(to) <= XY_EPS_MM {
        if ccw {
            std::f64::consts::TAU
        } else {
            -std::f64::consts::TAU
        }
    } else {
        sweep
    }
}

/// `polyline` as path data: `M`, then `L` and `A`, and `Z` when it
/// is closed.
fn polyline_path(polyline: &Polyline, frame: &Frame) -> Option<String> {
    let first = polyline.vertices().first()?;
    let mut d = format!("M {}", frame.xy(first.point()));
    for segment in polyline.segments() {
        match segment.arc() {
            Some(arc) => push_arc(
                &mut d,
                frame,
                segment.start,
                segment.end,
                arc.centre,
                arc.sweep_rad,
            ),
            None => {
                let _ = write!(d, " L {}", frame.xy(segment.end));
            }
        }
    }
    if polyline.is_closed() {
        d.push_str(" Z");
    }
    Some(d)
}

/// Every instance's outlines, placed, thin black.
fn write_shapes(svg: &mut String, project: &Project, frame: &Frame) {
    let _ = writeln!(
        svg,
        r##"<g class="shapes" fill="none" stroke="#000000" stroke-width="{}">"##,
        fmt(SHAPE_STROKE_MM)
    );
    for instance in &project.instances {
        let Some(definition) = project.definitions.get(&instance.definition) else {
            continue;
        };
        for profile in &definition.profiles {
            // Validation refuses a transform that cannot place a shape.
            let Ok(placed) = instance.transform.apply_polyline(profile) else {
                continue;
            };
            if let Some(d) = polyline_path(&placed, frame) {
                let _ = writeln!(svg, r#"<path class="shape" d="{d}"/>"#);
            }
        }
    }
    svg.push_str("</g>\n");
}

/// What one toolpath draws, in project space.
#[derive(Default)]
struct Drawing {
    /// Rapids that move in XY, as `(from, to)`.
    rapids: Vec<(Point, Point)>,
    /// One path of each run of cuts, as path data.
    cuts: Vec<String>,
    /// One path per stretch over a tab, as path data.
    tabs: Vec<String>,
    /// Where each descent starts, with the direction of the first cut
    /// after it, when there is one.
    starts: Vec<(Point, Option<Vec2>)>,
    /// A Drill's holes, in order, each once.
    holes: Vec<Point>,
}

/// Path data being built from moves, and whether any of them drew.
struct PathBuilder {
    d: String,
    drawn: bool,
}

impl PathBuilder {
    fn starting_at(p: Point, frame: &Frame) -> Self {
        Self {
            d: format!("M {}", frame.xy(p)),
            drawn: false,
        }
    }

    /// Adds the cutting move `m` from `from`; a move with no XY extent
    /// adds nothing.
    fn push(&mut self, from: Point3, m: &Move, frame: &Frame) {
        let (from, to) = (xy(from), xy(m.to));
        match m.kind {
            MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                let ccw = matches!(m.kind, MoveKind::ArcCcw { .. });
                push_arc(
                    &mut self.d,
                    frame,
                    from,
                    to,
                    centre,
                    move_sweep(from, to, centre, ccw),
                );
                self.drawn = true;
            }
            _ if from.distance_to(to) > XY_EPS_MM => {
                let _ = write!(self.d, " L {}", frame.xy(to));
                self.drawn = true;
            }
            _ => {}
        }
    }

    /// The path data, if anything was drawn.
    fn finish(self) -> Option<String> {
        self.drawn.then_some(self.d)
    }
}

/// The XY of `p`.
const fn xy(p: Point3) -> Point {
    Point::new(p.x, p.y)
}

/// The direction the cutting move `m` from `from` starts in, in XY, or
/// `None` for a move with no XY extent.
fn start_direction(from: Point3, m: &Move) -> Option<Vec2> {
    let (from, to) = (xy(from), xy(m.to));
    match m.kind {
        MoveKind::ArcCcw { centre } => {
            let r = from - centre;
            Vec2::new(-r.y, r.x).normalized()
        }
        MoveKind::ArcCw { centre } => {
            let r = from - centre;
            Vec2::new(r.y, -r.x).normalized()
        }
        _ => (to - from).normalized(),
    }
}

impl Drawing {
    /// Walks `toolpath` once, building its paths in `frame`. A move whose
    /// start is unknown, the first, only places the tool.
    fn of(toolpath: &Toolpath, drill: bool, frame: &Frame) -> Self {
        Walk::new(drill, frame).run(toolpath)
    }
}

/// The state of a walk along a toolpath.
struct Walk<'a> {
    frame: &'a Frame,
    /// A Drill: its descents are holes, and it has no tabs.
    drill: bool,
    out: Drawing,
    /// Where the tool is, once known.
    at: Option<Point3>,
    /// The current run of cuts.
    run: Option<PathBuilder>,
    /// The current stretch over a tab.
    tab: Option<PathBuilder>,
    /// The Z the last descent reached, or where the last rapid left the
    /// tool.
    floor_z: f64,
    /// Whether the last cutting move went down.
    descending: bool,
    /// The start in `out.starts` still waiting for its first cut.
    pending_tick: Option<usize>,
}

impl<'a> Walk<'a> {
    fn new(drill: bool, frame: &'a Frame) -> Self {
        Self {
            frame,
            drill,
            out: Drawing::default(),
            at: None,
            run: None,
            tab: None,
            floor_z: f64::INFINITY,
            descending: false,
            pending_tick: None,
        }
    }

    fn run(mut self, toolpath: &Toolpath) -> Drawing {
        for m in &toolpath.moves {
            let Some(from) = self.at else {
                if !m.is_dwell() {
                    self.at = Some(m.to);
                    self.floor_z = m.to.z;
                }
                continue;
            };
            if m.is_dwell() {
                continue;
            }
            if m.is_rapid() {
                self.end_run();
                if xy(from).distance_to(xy(m.to)) > XY_EPS_MM {
                    self.out.rapids.push((xy(from), xy(m.to)));
                }
                self.floor_z = m.to.z;
                self.descending = false;
            } else {
                self.cut(from, m);
            }
            self.at = Some(m.to);
        }
        self.end_run();
        self.out
    }

    /// Records the cutting move `m` from `from`.
    fn cut(&mut self, from: Point3, m: &Move) {
        let frame = self.frame;
        let going_down = m.to.z < from.z - Z_EPS_MM;
        if going_down && !self.descending && from.z <= self.floor_z + Z_EPS_MM {
            // A new descent: from a rapid, or from the pass before.
            if self.drill {
                let hole = xy(from);
                if self
                    .out
                    .holes
                    .last()
                    .is_none_or(|last| last.distance_to(hole) > XY_EPS_MM)
                {
                    self.out.holes.push(hole);
                }
            } else {
                self.out.starts.push((xy(from), None));
                self.pending_tick = Some(self.out.starts.len() - 1);
            }
        }
        if !going_down
            && let Some(k) = self.pending_tick
            && let Some(direction) = start_direction(from, m)
        {
            if let Some(start) = self.out.starts.get_mut(k) {
                start.1 = Some(direction);
            }
            self.pending_tick = None;
        }
        self.run
            .get_or_insert_with(|| PathBuilder::starting_at(xy(from), frame))
            .push(from, m, frame);
        let raised =
            !self.drill && (from.z > self.floor_z + Z_EPS_MM || m.to.z > self.floor_z + Z_EPS_MM);
        if raised {
            self.tab
                .get_or_insert_with(|| PathBuilder::starting_at(xy(from), frame))
                .push(from, m, frame);
        } else {
            self.end_tab();
        }
        if going_down {
            self.floor_z = m.to.z;
        }
        self.descending = going_down;
    }

    fn end_tab(&mut self) {
        if let Some(d) = self.tab.take().and_then(PathBuilder::finish) {
            self.out.tabs.push(d);
        }
    }

    fn end_run(&mut self) {
        self.end_tab();
        if let Some(d) = self.run.take().and_then(PathBuilder::finish) {
            self.out.cuts.push(d);
        }
        self.pending_tick = None;
    }
}

/// One operation's group: rapids under its cuts, then tab marks, then
/// start points or holes.
fn write_operation(
    svg: &mut String,
    frame: &Frame,
    op: &Operation,
    tool: &Tool,
    colour: &str,
    drawing: &Drawing,
) {
    let _ = writeln!(
        svg,
        r#"<g id="{}" class="operation" fill="none" stroke="{colour}">"#,
        escape_xml(op.id().as_str())
    );
    if !drawing.rapids.is_empty() {
        let d: Vec<String> = drawing
            .rapids
            .iter()
            .map(|(from, to)| format!("M {} L {}", frame.xy(*from), frame.xy(*to)))
            .collect();
        let _ = writeln!(
            svg,
            r#"<path class="rapid" d="{}" stroke="{RAPID_COLOUR}" stroke-width="{}" stroke-dasharray="{RAPID_DASH}"/>"#,
            d.join(" "),
            fmt(RAPID_STROKE_MM)
        );
    }
    for d in &drawing.cuts {
        let _ = writeln!(
            svg,
            r#"<path class="cut" d="{d}" stroke-width="{}"/>"#,
            fmt(CUT_STROKE_MM)
        );
    }
    for d in &drawing.tabs {
        let _ = writeln!(
            svg,
            r#"<path class="tab" d="{d}" stroke-width="{}" stroke-linecap="butt" stroke-opacity="0.6"/>"#,
            fmt(TAB_STROKE_MM)
        );
    }
    let diameter = tool.diameter_mm;
    let radius = fmt(diameter / 2.0);
    let mark = fmt(MARK_STROKE_MM);
    for (at, direction) in &drawing.starts {
        let _ = writeln!(
            svg,
            r#"<circle class="start" {} r="{radius}" stroke-width="{mark}"/>"#,
            frame.centre_attrs(*at)
        );
        if let Some(direction) = direction {
            let end = *at + *direction * diameter;
            let _ = writeln!(
                svg,
                r#"<path class="start-tick" d="M {} L {}" stroke-width="{mark}"/>"#,
                frame.xy(*at),
                frame.xy(end)
            );
        }
    }
    for hole in &drawing.holes {
        let _ = writeln!(
            svg,
            r#"<circle class="hole" {} r="{radius}" stroke-width="{mark}"/>"#,
            frame.centre_attrs(*hole)
        );
    }
    svg.push_str("</g>\n");
}

/// The legend in the stock's top-left corner: a swatch of each
/// operation's colour, its id and its name.
fn write_legend(
    svg: &mut String,
    width_mm: f64,
    height_mm: f64,
    ops: &[(&Operation, &Tool, &Toolpath)],
) {
    if ops.is_empty() {
        return;
    }
    // Lettering scaled to the stock, so it reads at any size.
    let font = (width_mm.min(height_mm) / 40.0).clamp(2.0, 10.0);
    let line = font * 1.5;
    let labels: Vec<String> = ops
        .iter()
        .map(|(op, _, _)| format!("{} {}", op.id(), op.name()))
        .collect();
    let longest = labels.iter().map(|l| l.chars().count()).max().unwrap_or(0);
    #[allow(
        clippy::cast_precision_loss,
        reason = "a label's length in characters, far below 2^52"
    )]
    let (box_width, box_height) = (
        font * (3.0 + 0.6 * longest as f64),
        line * labels.len() as f64 + font,
    );
    let _ = writeln!(
        svg,
        r##"<g class="legend" font-family="sans-serif" font-size="{}">"##,
        fmt(font)
    );
    let _ = writeln!(
        svg,
        r##"<rect class="legend-box" x="0.000" y="0.000" width="{}" height="{}" fill="#ffffff" fill-opacity="0.5"/>"##,
        fmt(box_width),
        fmt(box_height)
    );
    let mut baseline = font * 0.5;
    for (k, label) in labels.iter().enumerate() {
        baseline += line;
        let swatch = font * 0.8;
        let _ = writeln!(
            svg,
            r#"<rect class="legend-swatch" x="{}" y="{}" width="{s}" height="{s}" fill="{}"/>"#,
            fmt(font),
            fmt(baseline - swatch),
            colour(k),
            s = fmt(swatch),
        );
        let _ = writeln!(
            svg,
            r##"<text class="legend-entry" x="{}" y="{}" fill="#000000">{}</text>"##,
            fmt(font * 2.2),
            fmt(baseline),
            escape_xml(label)
        );
    }
    svg.push_str("</g>\n");
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
            let tab = |moves: &mut Vec<Move>, x0: f64, x1: f64, y: f64| {
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
