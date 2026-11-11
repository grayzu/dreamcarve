// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/designPrint/svg.ts

//! The toolpath preview SVG of `export --preview` and `check --preview`
//! (functional spec, capability 31; `docs/specs/phase-2/design.md`, "Phase
//! 2 addendum (#73) → A5"), moved from `crates/dreamcarve/src/preview.rs`
//! with its bytes unchanged (`docs/specs/phase-4/design.md`, "Data model →
//! dc-job"). [`marks`] is new: the tab marks, start points and drill
//! holes as geometry, which [`render`] draws and the app's Viewport draws
//! the same way (functional spec, acceptance 79).
//!
//! [`render`] draws the generated toolpath, not the G-code, so arc fitting
//! does not show. It is a pure function of the project and the toolpaths:
//! numbers have three decimals and nothing depends on the date or a random
//! id, so the same project gives the same bytes. The shell writes the
//! file.
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
//! - **Pictures** (format 27): each bitmap as an `<image>` at its
//!   footprint, under everything else drawn on the stock.

use std::f64::consts::PI;
use std::fmt::Write as _;

use dc_cam::{Move, MoveKind, RunLabel, Toolpath};
use dc_gcode::EmitOp;
use dc_geometry::{Arc, EPS_MM, Point, Point3, Polyline, Vec2, Vertex};
use dc_project::{Board, Operation, OperationId, Project, Tool};

/// Operation colours, by the run's place in the preview, repeating after
/// eight: `experience.md`'s light operation set (section 5, "Operation
/// colours"; design open question 5), Matplotlib's tab10 hues in their
/// order with green, purple, orange, cyan and pink darkened to at least
/// 3.3:1 on the light Viewport. One table: the app's light Viewport draws
/// these, so the screen and this file agree (acceptance 79). Distinct on
/// white and from the black shapes.
pub const PALETTE: [&str; 8] = [
    "#1f77b4", "#d62728", "#2a8f2a", "#8e5fb8", "#d96a00", "#0e98a8", "#8c564b", "#c94fa8",
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

/// One tool run to draw: an operation's toolpath with one tool, and which
/// of the operation's runs it is (a carve's clearing pass, #406).
#[derive(Debug, Clone, Copy)]
pub struct Run<'a> {
    /// The operation.
    pub op: &'a Operation,
    /// Which of its runs.
    pub label: &'a RunLabel,
    /// The run's tool.
    pub tool: &'a Tool,
    /// The run's toolpath, in its board's project space.
    pub toolpath: &'a Toolpath,
    /// The board the run cuts (#498): an Inlay male's is drawn on the male
    /// board, beside the stock.
    pub board: Board,
}

impl<'a> From<&'a EmitOp> for Run<'a> {
    /// A generated entry as the run it draws.
    fn from(entry: &'a EmitOp) -> Self {
        Self {
            op: &entry.op,
            label: &entry.label,
            tool: &entry.tool,
            toolpath: &entry.toolpath,
            board: entry.board,
        }
    }
}

/// The gap between the stock and the male board in the picture, mm
/// (#498).
const BOARD_GAP_MM: f64 = 10.0;
/// The origin mark's radius, mm.
const ORIGIN_MARK_MM: f64 = 1.5;

/// A deterministic SVG of the project's stock, shapes and each enabled
/// operation's tool runs, in mm, Y flipped for display, each run in a
/// colour of its own. When a run is on an Inlay's male board (#498), the
/// male board is drawn beside the stock, to its right `BOARD_GAP_MM`
/// away, their bottom edges level, with its own outline and its Inlays'
/// mirrored shapes, its male runs drawn on it, and each board its origin
/// mark.
#[must_use]
pub fn render(project: &Project, ops: &[Run<'_>]) -> String {
    let width = project.stock.width_mm;
    let male = project
        .male_board()
        .filter(|_| ops.iter().any(|r| r.board == Board::Male));
    let height = male.as_ref().map_or(project.stock.height_mm, |(_, board)| {
        project.stock.height_mm.max(board.height_mm)
    });
    let total = male
        .as_ref()
        .map_or(width, |(_, board)| width + BOARD_GAP_MM + board.width_mm);
    let frame = Frame {
        height_mm: height,
        x_mm: 0.0,
    };
    let male_frame = Frame {
        height_mm: height,
        x_mm: width + BOARD_GAP_MM,
    };
    let mut svg = String::new();
    let _ = writeln!(svg, r#"<?xml version="1.0" encoding="UTF-8"?>"#);
    let _ = writeln!(svg, "{MARKER}");
    let _ = writeln!(
        svg,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">"#,
        w = fmt(total),
        h = fmt(height),
    );
    let _ = writeln!(
        svg,
        "<title>{}: toolpath preview</title>",
        escape_xml(&project.name)
    );
    let _ = writeln!(
        svg,
        r#"<rect class="stock" x="0.000" y="{}" width="{}" height="{}" fill="none" stroke="{STOCK_COLOUR}" stroke-width="{}" stroke-dasharray="{STOCK_DASH}"/>"#,
        fmt(height - project.stock.height_mm),
        fmt(width),
        fmt(project.stock.height_mm),
        fmt(STOCK_STROKE_MM),
    );
    if let Some((_, board)) = &male {
        let _ = writeln!(
            svg,
            r#"<rect class="male-board" x="{}" y="{}" width="{}" height="{}" fill="none" stroke="{STOCK_COLOUR}" stroke-width="{}" stroke-dasharray="{STOCK_DASH}"/>"#,
            fmt(male_frame.x_mm),
            fmt(height - board.height_mm),
            fmt(board.width_mm),
            fmt(board.height_mm),
            fmt(STOCK_STROKE_MM),
        );
    }
    write_pictures(&mut svg, project, &frame);
    write_shapes(&mut svg, project, &frame);
    if let Some((_, board)) = &male {
        write_male_shapes(&mut svg, project, &male_frame);
        write_male_boundaries(&mut svg, project, &male_frame);
        write_origin(&mut svg, project, &project.stock, &frame);
        write_origin(&mut svg, project, board, &male_frame);
    }
    for (k, run) in ops.iter().enumerate() {
        let colour = colour(k);
        let on = if run.board == Board::Male && male.is_some() {
            &male_frame
        } else {
            &frame
        };
        let drawn = Drawn::of(
            run.toolpath,
            matches!(run.op, Operation::Drill(_)),
            on,
            run.tool.diameter_mm / 2.0,
        );
        write_operation(&mut svg, on, run, colour, &drawn);
    }
    let boundary = male.is_some() && !male_boundaries(project).is_empty();
    write_legend(&mut svg, width, height, ops, boundary);
    svg.push_str("</svg>\n");
    svg
}

/// [`render`] of every generated entry, in order (addendum A5).
#[must_use]
pub fn render_generated(project: &Project, generated: &[EmitOp]) -> String {
    let ops: Vec<Run<'_>> = generated.iter().map(Run::from).collect();
    render(project, &ops)
}

/// The marks a run's toolpath draws besides its cuts and rapids, as
/// geometry in its board's project space, mm, Y-up, so the app's Viewport
/// draws the same ones [`render`] does (functional spec, acceptance 79).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Marks {
    /// Each stretch of cutting moves over a tab, as the moves trace it:
    /// one open polyline per tab per pass that rises over it.
    pub tab_marks: Vec<Polyline>,
    /// Where each descent starts, with the direction of the first cut
    /// after it. Empty for a Drill, whose descents are its holes.
    pub start_points: Vec<StartMark>,
    /// A Drill's holes in order, each once, with the bit's radius, mm.
    pub holes: Vec<(Point, f64)>,
}

/// Where a descent starts: a circle of the bit's radius, with a tick one
/// diameter long in the direction of the first cut after it, when there
/// is one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StartMark {
    /// Where the tool comes down, mm.
    pub at: Point,
    /// The unit direction of the first cutting move after the descent,
    /// `None` when none moves in XY.
    pub direction: Option<Vec2>,
    /// The bit's radius, mm: the circle's, and half the tick's length.
    pub radius_mm: f64,
}

/// The marks of `run`: its tab marks, start points and holes, as
/// [`render`] draws them.
#[must_use]
pub fn marks(run: &Run<'_>) -> Marks {
    // The cuts are built as path data in a frame; the marks are geometry
    // and need none.
    let frame = Frame {
        height_mm: 0.0,
        x_mm: 0.0,
    };
    Drawn::of(
        run.toolpath,
        matches!(run.op, Operation::Drill(_)),
        &frame,
        run.tool.diameter_mm / 2.0,
    )
    .marks
}

/// One enabled Inlay's design as its male cuts it (#498).
#[derive(Debug, Clone, PartialEq)]
pub struct MaleShapes {
    /// The Inlay.
    pub op: OperationId,
    /// Its targets' closed outlines placed and mirrored about their centre
    /// line ([`dc_cam::inlay::male_mirror`]), in the male board's project
    /// space, mm, Y-up.
    pub outlines: Vec<Polyline>,
}

/// Every enabled Inlay's design as its male cuts it (#498), in list order:
/// what [`render`] draws on the male board, and what the app's Viewport
/// draws there. An Inlay whose male is not cut (its board's thickness
/// unset) has none; one whose design cannot be mirrored (a missing
/// instance, which validation reports) is left out, and so is an outline
/// its transform cannot place.
#[must_use]
pub fn male_shapes(project: &Project) -> Vec<MaleShapes> {
    let mut out = Vec::new();
    for op in &project.operations {
        let Operation::Inlay(inlay) = op else {
            continue;
        };
        if !(inlay.enabled && inlay.cuts_male()) {
            continue;
        }
        let Ok(mirror) = dc_cam::inlay::male_mirror(project, &inlay.targets) else {
            continue;
        };
        let mut outlines = Vec::new();
        for target in &inlay.targets {
            let Some(instance) = project.instances.iter().find(|i| &i.id == target) else {
                continue;
            };
            let Some(definition) = project.definitions.get(&instance.definition) else {
                continue;
            };
            for profile in definition.profiles.iter().filter(|p| p.is_closed()) {
                if let Ok(placed) = instance
                    .transform
                    .apply_polyline(profile)
                    .and_then(|p| mirror.apply_polyline(&p))
                {
                    outlines.push(placed);
                }
            }
        }
        out.push(MaleShapes {
            op: inlay.id.clone(),
            outlines,
        });
    }
    out
}

/// One enabled Inlay's drawn boundary as its male cuts it (#603).
#[derive(Debug, Clone, PartialEq)]
pub struct MaleBoundary {
    /// The Inlay.
    pub op: OperationId,
    /// The outlines of its boundary instances (`boundary`),
    /// placed and mirrored with the design about the targets' centre line
    /// ([`dc_cam::inlay::male_mirror`]), in the male board's project
    /// space, mm, Y-up; every outline, a hole's included, since the
    /// picture shows what was drawn.
    pub outlines: Vec<Polyline>,
}

/// Every enabled Inlay's drawn boundary as its male cuts it (#603), in
/// list order: what [`render`] draws on the male board. An Inlay with no
/// boundary, or whose male is not cut, has none; an instance that does
/// not resolve (validation reports it) is left out.
#[must_use]
pub fn male_boundaries(project: &Project) -> Vec<MaleBoundary> {
    let mut out = Vec::new();
    for op in &project.operations {
        let Operation::Inlay(inlay) = op else {
            continue;
        };
        if !(inlay.enabled && inlay.cuts_male() && inlay.has_boundary()) {
            continue;
        }
        let Ok(mirror) = dc_cam::inlay::male_mirror(project, &inlay.targets) else {
            continue;
        };
        let mut outlines = Vec::new();
        for id in &inlay.boundary {
            let Ok(instance) = dc_project::boundary_instance(project, inlay, id) else {
                continue;
            };
            for region in &instance.regions {
                for contour in std::iter::once(region.outer()).chain(region.holes()) {
                    if let Ok(placed) = mirror.apply_polyline(contour.polyline()) {
                        outlines.push(placed);
                    }
                }
            }
        }
        out.push(MaleBoundary {
            op: inlay.id.clone(),
            outlines,
        });
    }
    out
}

/// Every enabled male Inlay's drawn boundary ([`male_boundaries`]),
/// dashed grey as the board's outline is, on the male board, under the
/// male's runs.
fn write_male_boundaries(svg: &mut String, project: &Project, frame: &Frame) {
    for boundary in male_boundaries(project) {
        for placed in &boundary.outlines {
            if let Some(d) = polyline_path(placed, frame) {
                let _ = writeln!(
                    svg,
                    r#"<path class="male-boundary" d="{d}" fill="none" stroke="{STOCK_COLOUR}" stroke-width="{}" stroke-dasharray="{STOCK_DASH}"/>"#,
                    fmt(STOCK_STROKE_MM)
                );
            }
        }
    }
}

/// Every enabled Inlay's design as its male cuts it ([`male_shapes`]),
/// thin black, on the male board.
fn write_male_shapes(svg: &mut String, project: &Project, frame: &Frame) {
    let _ = writeln!(
        svg,
        r##"<g class="male-shapes" fill="none" stroke="#000000" stroke-width="{}">"##,
        fmt(SHAPE_STROKE_MM)
    );
    for shapes in male_shapes(project) {
        for placed in &shapes.outlines {
            if let Some(d) = polyline_path(placed, frame) {
                let _ = writeln!(svg, r#"<path class="male-shape" d="{d}"/>"#);
            }
        }
    }
    svg.push_str("</g>\n");
}

/// Each bitmap as an `<image>` of class `picture` at its footprint, under
/// the shapes and the toolpaths, so the grooves can be checked against
/// the picture (`docs/specs/phase-3/design.md`, "CLI"). The footprint's
/// top edge is the picture's top row (Y-up), so the image's corner is the
/// footprint's top-left, flipped; `preserveAspectRatio="none"` stretches
/// it to the footprint exactly. The data URI is the decoded pixels encoded
/// again ([`dc_import::bitmap::stored`]), never the file's own string, and
/// it is escaped like every attribute (security review L5). A bitmap whose
/// pixels cannot be read is not drawn: what reads the pixels to cut them
/// reports it.
fn write_pictures(svg: &mut String, project: &Project, frame: &Frame) {
    for bitmap in &project.bitmaps {
        let Some(png_base64) = bitmap
            .pixels()
            .ok()
            .and_then(|pixels| dc_import::bitmap::stored(&pixels).ok())
        else {
            continue;
        };
        let _ = writeln!(
            svg,
            r#"<image class="picture" id="{}" x="{}" y="{}" width="{}" height="{}" preserveAspectRatio="none" href="{}"/>"#,
            escape_xml(bitmap.id.as_str()),
            fmt(frame.x_mm + bitmap.x_mm),
            fmt(frame.height_mm - (bitmap.y_mm + bitmap.height_mm)),
            fmt(bitmap.width_mm),
            fmt(bitmap.height_mm),
            escape_xml(&format!("data:image/png;base64,{png_base64}")),
        );
    }
}

/// The origin mark of a board `stock` drawn in `frame` (#498): a circle at
/// the XY work zero the project's origin preset puts on that board.
fn write_origin(svg: &mut String, project: &Project, stock: &dc_project::Stock, frame: &Frame) {
    let at = project.origin.xy.point_mm(stock);
    let _ = writeln!(
        svg,
        r##"<circle class="origin" {} r="{}" fill="none" stroke="#000000" stroke-width="{}"/>"##,
        frame.centre_attrs(at),
        fmt(ORIGIN_MARK_MM),
        fmt(MARK_STROKE_MM),
    );
}

/// The colour of the `k`th operation.
fn colour(k: usize) -> &'static str {
    PALETTE.get(k % PALETTE.len()).copied().unwrap_or("#000000")
}

/// Where a board's project points are drawn.
struct Frame {
    /// The picture's height, mm: `y_svg = height_mm − y`.
    height_mm: f64,
    /// Where the board's X = 0 is in the picture, mm: 0 for the stock,
    /// right of it for the male board (#498).
    x_mm: f64,
}

impl Frame {
    /// `p` as `x y` in SVG coordinates. The one place Y is flipped.
    fn xy(&self, p: Point) -> String {
        format!("{} {}", fmt(self.x_mm + p.x), fmt(self.height_mm - p.y))
    }

    /// `p` as the attributes `cx="…" cy="…"`.
    fn centre_attrs(&self, p: Point) -> String {
        format!(
            r#"cx="{}" cy="{}""#,
            fmt(self.x_mm + p.x),
            fmt(self.height_mm - p.y)
        )
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

/// What one toolpath draws: its cuts and rapids as path data in a frame,
/// and its marks as geometry in project space.
#[derive(Default)]
struct Drawn {
    /// Rapids that move in XY, as `(from, to)`.
    rapids: Vec<(Point, Point)>,
    /// One path of each run of cuts, as path data.
    cuts: Vec<String>,
    /// The tab marks, start points and holes.
    marks: Marks,
}

impl Drawn {
    /// Walks `toolpath` once, building its paths in `frame` and its marks
    /// for a bit of `radius_mm`. A move whose start is unknown, the first,
    /// only places the tool.
    fn of(toolpath: &Toolpath, drill: bool, frame: &Frame, radius_mm: f64) -> Self {
        Walk::new(drill, frame, radius_mm).run(toolpath)
    }
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

/// A tab mark being built from moves, as a polyline with bulge arcs, and
/// whether any of them drew. It traces what [`PathBuilder`] would write:
/// a straight move with no XY extent adds nothing, an arc is a bulge, and
/// a full turn is two half turns, as [`push_arc`] splits it.
struct TabBuilder {
    vertices: Vec<Vertex>,
    drawn: bool,
}

impl TabBuilder {
    fn starting_at(p: Point) -> Self {
        Self {
            vertices: vec![Vertex::line(p.x, p.y)],
            drawn: false,
        }
    }

    /// Sets the bulge of the segment leaving the last vertex.
    fn set_bulge(&mut self, bulge: f64) {
        if let Some(last) = self.vertices.last_mut() {
            last.bulge = bulge;
        }
    }

    /// Adds the cutting move `m` from `from`.
    fn push(&mut self, from: Point3, m: &Move) {
        let (from, to) = (xy(from), xy(m.to));
        match m.kind {
            MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                let ccw = matches!(m.kind, MoveKind::ArcCcw { .. });
                if from.distance_to(centre) <= XY_EPS_MM {
                    // No radius: a line, as `push_arc` draws it.
                    self.vertices.push(Vertex::line(to.x, to.y));
                } else if from.distance_to(to) <= EPS_MM {
                    // A full turn: two half turns through the point
                    // opposite the start, since a polyline segment cannot
                    // end where it begins.
                    let bulge = if ccw { 1.0 } else { -1.0 };
                    let mid = Point::new(2.0 * centre.x - from.x, 2.0 * centre.y - from.y);
                    self.set_bulge(bulge);
                    self.vertices.push(Vertex::new(mid.x, mid.y, bulge));
                    self.vertices.push(Vertex::line(to.x, to.y));
                } else {
                    let sweep = move_sweep(from, to, centre, ccw);
                    self.set_bulge((sweep / 4.0).tan());
                    self.vertices.push(Vertex::line(to.x, to.y));
                }
                self.drawn = true;
            }
            _ if from.distance_to(to) > XY_EPS_MM => {
                self.vertices.push(Vertex::line(to.x, to.y));
                self.drawn = true;
            }
            _ => {}
        }
    }

    /// The polyline, if anything was drawn.
    fn finish(self) -> Option<Polyline> {
        self.drawn.then(|| Polyline::new(self.vertices, false))
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

/// The state of a walk along a toolpath.
struct Walk<'a> {
    frame: &'a Frame,
    /// A Drill: its descents are holes, and it has no tabs.
    drill: bool,
    /// The bit's radius, mm: the start circles' and the holes'.
    radius_mm: f64,
    out: Drawn,
    /// Where the tool is, once known.
    at: Option<Point3>,
    /// The current run of cuts.
    run: Option<PathBuilder>,
    /// The current stretch over a tab.
    tab: Option<TabBuilder>,
    /// The Z the last descent reached, or where the last rapid left the
    /// tool.
    floor_z: f64,
    /// Whether the last cutting move went down.
    descending: bool,
    /// The start in `out.starts` still waiting for its first cut.
    pending_tick: Option<usize>,
}

impl<'a> Walk<'a> {
    fn new(drill: bool, frame: &'a Frame, radius_mm: f64) -> Self {
        Self {
            frame,
            drill,
            radius_mm,
            out: Drawn::default(),
            at: None,
            run: None,
            tab: None,
            floor_z: f64::INFINITY,
            descending: false,
            pending_tick: None,
        }
    }

    fn run(mut self, toolpath: &Toolpath) -> Drawn {
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
                    .marks
                    .holes
                    .last()
                    .is_none_or(|(last, _)| last.distance_to(hole) > XY_EPS_MM)
                {
                    self.out.marks.holes.push((hole, self.radius_mm));
                }
            } else {
                self.out.marks.start_points.push(StartMark {
                    at: xy(from),
                    direction: None,
                    radius_mm: self.radius_mm,
                });
                self.pending_tick = Some(self.out.marks.start_points.len() - 1);
            }
        }
        if !going_down
            && let Some(k) = self.pending_tick
            && let Some(direction) = start_direction(from, m)
        {
            if let Some(start) = self.out.marks.start_points.get_mut(k) {
                start.direction = Some(direction);
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
                .get_or_insert_with(|| TabBuilder::starting_at(xy(from)))
                .push(from, m);
        } else {
            self.end_tab();
        }
        if going_down {
            self.floor_z = m.to.z;
        }
        self.descending = going_down;
    }

    fn end_tab(&mut self) {
        if let Some(polyline) = self.tab.take().and_then(TabBuilder::finish) {
            self.out.marks.tab_marks.push(polyline);
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
/// start points or holes, the marks drawn from [`Marks`].
fn write_operation(svg: &mut String, frame: &Frame, run: &Run<'_>, colour: &str, drawn: &Drawn) {
    // #498: a male board run is marked as such.
    let class = match run.board {
        Board::Female => "operation",
        Board::Male => "operation male",
    };
    let _ = writeln!(
        svg,
        r#"<g id="{}" class="{class}" fill="none" stroke="{colour}">"#,
        escape_xml(run.op.id().as_str())
    );
    if !drawn.rapids.is_empty() {
        let d: Vec<String> = drawn
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
    for d in &drawn.cuts {
        let _ = writeln!(
            svg,
            r#"<path class="cut" d="{d}" stroke-width="{}"/>"#,
            fmt(CUT_STROKE_MM)
        );
    }
    let marks = &drawn.marks;
    for tab in &marks.tab_marks {
        if let Some(d) = polyline_path(tab, frame) {
            let _ = writeln!(
                svg,
                r#"<path class="tab" d="{d}" stroke-width="{}" stroke-linecap="butt" stroke-opacity="0.6"/>"#,
                fmt(TAB_STROKE_MM)
            );
        }
    }
    let mark = fmt(MARK_STROKE_MM);
    for start in &marks.start_points {
        let radius = fmt(start.radius_mm);
        let _ = writeln!(
            svg,
            r#"<circle class="start" {} r="{radius}" stroke-width="{mark}"/>"#,
            frame.centre_attrs(start.at)
        );
        if let Some(direction) = start.direction {
            let end = start.at + direction * (2.0 * start.radius_mm);
            let _ = writeln!(
                svg,
                r#"<path class="start-tick" d="M {} L {}" stroke-width="{mark}"/>"#,
                frame.xy(start.at),
                frame.xy(end)
            );
        }
    }
    for (hole, radius_mm) in &marks.holes {
        let _ = writeln!(
            svg,
            r#"<circle class="hole" {} r="{}" stroke-width="{mark}"/>"#,
            frame.centre_attrs(*hole),
            fmt(*radius_mm)
        );
    }
    svg.push_str("</g>\n");
}

/// The legend in the stock's top-left corner: a swatch of each run's
/// colour, its operation's id and name; a carve's clearing run as
/// `o6 (clearing t1) Name` (#406).
fn write_legend(svg: &mut String, width_mm: f64, height_mm: f64, ops: &[Run<'_>], boundary: bool) {
    if ops.is_empty() {
        return;
    }
    // Lettering scaled to the stock, so it reads at any size.
    let font = (width_mm.min(height_mm) / 40.0).clamp(2.0, 10.0);
    let line = font * 1.5;
    let mut labels: Vec<String> = ops
        .iter()
        .map(|run| {
            format!(
                "{} {}{}",
                run.label.naming(run.op.id().as_str()),
                run.op.name(),
                match run.board {
                    Board::Female => "",
                    Board::Male => " (male board)",
                }
            )
        })
        .collect();
    // #603: the drawn boundary's dashed outline, in the board's grey.
    if boundary {
        labels.push("boundary (male board)".to_owned());
    }
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
            if k < ops.len() {
                colour(k)
            } else {
                STOCK_COLOUR
            },
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

    /// `toolpath` as the main run of the sample project's one operation,
    /// with its 6.35 mm tool.
    fn main_run<'a>(project: &'a Project, toolpath: &'a Toolpath) -> Run<'a> {
        Run {
            op: &project.operations[0],
            label: &RunLabel::Main,
            tool: &project.tools[0],
            toolpath,
            board: Board::Female,
        }
    }

    /// `toolpath` rendered as the sample project's one operation, with
    /// its 6.35 mm tool. The stock is 100 mm high, so `y_svg = 100 − y`.
    fn render_one(toolpath: &Toolpath) -> String {
        let project = sample_project();
        render(&project, &[main_run(&project, toolpath)])
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

    /// A tab mark over an arc is drawn as the arc, as a bulge polyline
    /// (so the Viewport draws the same curve), and a full-turn arc over a
    /// tab is two half turns, as a cut's is.
    #[test]
    fn tab_marks_keep_arcs() {
        // Down at (60, 50) to Z 0 (the floor), then a quarter turn about
        // (50, 50) at Z 2, over a tab, counter-clockwise to (50, 60).
        let toolpath = Toolpath {
            moves: vec![
                rapid(60.0, 50.0, 10.0),
                cut(60.0, 50.0, 0.0),
                cut(60.0, 50.0, 2.0),
                arc(true, 50.0, 60.0, 2.0, (50.0, 50.0)),
                cut(50.0, 60.0, 0.0),
            ],
        };
        let svg = render_one(&toolpath);
        let tabs = elements(&svg, "tab");
        assert_eq!(tabs.len(), 1, "{svg}");
        assert_eq!(
            attr(tabs[0], "d"),
            "M 60.000 50.000 A 10.000 10.000 0 0 0 50.000 40.000"
        );
        let project = sample_project();
        let marks = marks(&main_run(&project, &toolpath));
        assert_eq!(marks.tab_marks.len(), 1);
        let quarter = marks.tab_marks[0].segments().next().unwrap().arc().unwrap();
        assert!((quarter.radius_mm - 10.0).abs() < 1e-9, "{quarter:?}");
        assert!((quarter.sweep_rad - PI / 2.0).abs() < 1e-9, "{quarter:?}");

        // A full circle over a tab.
        let toolpath = Toolpath {
            moves: vec![
                rapid(60.0, 50.0, 10.0),
                cut(60.0, 50.0, 0.0),
                cut(60.0, 50.0, 2.0),
                arc(false, 60.0, 50.0, 2.0, (50.0, 50.0)),
                cut(60.0, 50.0, 0.0),
            ],
        };
        let svg = render_one(&toolpath);
        let tabs = elements(&svg, "tab");
        assert_eq!(tabs.len(), 1, "{svg}");
        assert_eq!(
            attr(tabs[0], "d"),
            "M 60.000 50.000 A 10.000 10.000 0 0 1 40.000 50.000 A 10.000 10.000 0 0 1 60.000 50.000"
        );
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
        // The marks say the same, in project space.
        let project = sample_project();
        let marks = marks(&main_run(&project, &toolpath));
        assert_eq!(marks.start_points.len(), 4);
        assert_eq!(marks.start_points[3].at, Point::new(20.0, 50.0));
        assert_eq!(marks.start_points[3].direction, Some(Vec2::new(0.0, 1.0)));
        assert!((marks.start_points[3].radius_mm - 3.175).abs() < 1e-12);
        assert!(marks.holes.is_empty());
    }

    #[test]
    fn escapes_names() {
        let mut project = sample_project();
        if let Operation::Profile(op) = &mut project.operations[0] {
            op.name = "<a & \"b\">".into();
        }
        let toolpath = one_cut();
        let svg = render(&project, &[main_run(&project, &toolpath)]);
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
        let ops = [main_run(&project, &toolpath)];
        assert_eq!(render(&project, &ops), render(&project, &ops));
    }
}
