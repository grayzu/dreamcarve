// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/toolpathVisibility.ts

//! The view settings (`docs/specs/phase-4/design.md`, "Data model →
//! dc-app"): the camera, what the Viewport draws of a toolpath and how it
//! colours it, and which operations are hidden. View state, not project
//! state (functional spec, acceptance 81): nothing here is saved in the
//! `.dcv` or undone. The camera is [`crate::viewport::camera`]'s.
//!
//! From PureCutCNC's `toolpathVisibility.ts` comes the visibility record
//! as view state, one flag per layer. DreamCarve's layers are functional
//! capability 73's: cuts, rapids, plunges, direction arrows, start points
//! and tab marks; PureCutCNC's lead-ins, retractions and feed colours do
//! not exist here (a rapid upward is a rapid), and its levels are not
//! ported.
//!
//! *By depth* (capability 73; `experience.md`, section 5, "Depth ramp"):
//! every cutting move takes its colour from a ramp running from pale at
//! the stock top to dark at the deepest cut in the project. [`DepthRamp`]
//! places a Z on it; [`depth_legend`] labels it in work coordinates: the
//! clearance height and each safe Z in the band above the stock top
//! (capability 73a), then the stock top, each operation's depth and the
//! deepest cut, in project units. [`operation_key`] is *By operation*'s
//! key.

use std::collections::BTreeSet;

use dc_geometry::Rect;
use dc_job::{AxisRange, Finished};
use dc_project::{Board, OperationId, OriginZ, Project, Units};

use crate::strings::{StrKey, Strings};

use crate::scene::Scene;
use crate::selection::Selection;
pub use crate::viewport::camera::Camera;

/// What the user is looking at and how.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct View {
    /// Where the Viewport looks and how closely.
    pub camera: Camera,
    /// What the Viewport draws of a toolpath.
    pub visibility: Visibility,
    /// How the Viewport colours toolpaths.
    pub colour: ColourMode,
    /// Operations whose toolpath is hidden (the eye in the list).
    pub hidden_ops: BTreeSet<OperationId>,
}

impl View {
    /// Whether operation `op`'s toolpath is drawn: its eye is open and,
    /// with *Only the selected operation* on, it is the selected
    /// operation. With that on and no operation selected, none is.
    #[must_use]
    pub fn shows(&self, op: &OperationId, selection: &Selection) -> bool {
        if self.hidden_ops.contains(op) {
            return false;
        }
        !self.visibility.only_selected || matches!(selection, Selection::Operation(id) if id == op)
    }

    /// *Show all*: every operation's eye open.
    pub fn show_all(&mut self) {
        self.hidden_ops.clear();
    }

    /// *Hide all*: every operation of `scene` hidden.
    pub fn hide_all(&mut self, scene: &Scene) {
        self.hidden_ops
            .extend(scene.operations().iter().map(|o| o.id.clone()));
    }
}

/// What the Viewport draws of a toolpath. Everything but the arrows and
/// the extents is on by default, as in `gui.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visibility {
    /// Cutting moves.
    pub cuts: bool,
    /// Rapid moves.
    pub rapids: bool,
    /// Plunges: a mark where the tool feeds straight down.
    pub plunges: bool,
    /// Direction arrows along the cuts.
    pub arrows: bool,
    /// Where each descent starts.
    pub start_points: bool,
    /// The stretches over tabs.
    pub tab_marks: bool,
    /// The grid.
    pub grid: bool,
    /// The program extents boxes (capability 74).
    pub extents: bool,
    /// Draw only the selected operation's toolpath.
    pub only_selected: bool,
}

impl Default for Visibility {
    fn default() -> Self {
        Self {
            cuts: true,
            rapids: true,
            plunges: true,
            arrows: false,
            start_points: true,
            tab_marks: true,
            grid: true,
            extents: false,
            only_selected: false,
        }
    }
}

/// How the Viewport colours toolpaths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ColourMode {
    /// One colour per operation, the preview's palette.
    #[default]
    ByOperation,
    /// By cutting depth.
    ByDepth,
}

/// How many steps the depth ramp has: the renderer's ramp texture is this
/// many texels wide, and a cutting move's style word carries its step in
/// eight bits.
pub const DEPTH_STEPS: usize = 256;

/// Where a Z sits between the stock top and the deepest cut in the
/// project, for *By depth*.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DepthRamp {
    /// The stock top, project Z, mm: the ramp's pale end.
    pub top_z_mm: f64,
    /// The deepest cut's Z, mm: the ramp's dark end.
    pub deepest_z_mm: f64,
}

impl DepthRamp {
    /// The ramp of `scene`'s cutting moves on the stock (an Inlay's male
    /// board is cut from its own top and is left out); `None` when the
    /// stock has no thickness or nothing cuts below its top.
    #[must_use]
    pub fn of(project: &Project, scene: &Scene) -> Option<Self> {
        let top_z_mm = project.stock.thickness_mm?;
        let deepest_z_mm = scene
            .runs()
            .filter(|run| run.board == Board::Female)
            .flat_map(|run| cutting_z_mm(&run.toolpath))
            .reduce(f64::min)?;
        (deepest_z_mm < top_z_mm - Z_EPS_MM).then_some(Self {
            top_z_mm,
            deepest_z_mm,
        })
    }

    /// How far down the ramp `z_mm` is: 0 at the stock top or above, 1 at
    /// the deepest cut or below.
    #[must_use]
    pub fn fraction(&self, z_mm: f64) -> f64 {
        let span = self.top_z_mm - self.deepest_z_mm;
        if span <= Z_EPS_MM {
            return 1.0;
        }
        ((self.top_z_mm - z_mm) / span).clamp(0.0, 1.0)
    }

    /// The ramp step of `z_mm`, `0..DEPTH_STEPS`.
    #[must_use]
    pub fn step(&self, z_mm: f64) -> u8 {
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "a fraction in [0, 1] times 255, rounded"
        )]
        let step = (self.fraction(z_mm) * (DEPTH_STEPS - 1) as f64).round() as u8;
        step
    }
}

/// Two Z values closer than this are one depth, mm.
const Z_EPS_MM: f64 = 1e-6;

/// The Z of every cutting move's end in `toolpath`: the depths it cuts
/// at. A rapid never cuts, and the first move only places the tool.
fn cutting_z_mm(toolpath: &dc_job::Toolpath) -> impl Iterator<Item = f64> + '_ {
    toolpath
        .moves
        .iter()
        .skip(1)
        .filter(|m| !m.is_rapid() && !m.is_dwell())
        .map(|m| m.to.z)
}

/// What a tick of the depth legend names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TickName {
    /// The clearance height (`heights.clearance_mm`).
    Clearance,
    /// An operation's safe Z, each distinct height once.
    SafeZ,
    /// The stock top.
    StockTop,
    /// An operation cutting to this depth, by name.
    Operation(String),
}

impl TickName {
    /// The name as the legend writes it.
    #[must_use]
    pub fn text<'a>(&'a self, strings: &'a Strings) -> &'a str {
        match self {
            Self::Clearance => strings.get(StrKey::AppToolpathsClearance),
            Self::SafeZ => strings.get(StrKey::AppToolpathsSafeZ),
            Self::StockTop => strings.get(StrKey::AppToolpathsStockTop),
            Self::Operation(name) => name,
        }
    }
}

/// One labelled tick of the depth legend.
#[derive(Debug, Clone, PartialEq)]
pub struct LegendTick {
    /// Where on the legend: on the cutting ramp, 0 at the stock top and 1
    /// at the deepest cut; in the band above it, negative, −1 at the
    /// highest non-cutting height.
    pub fraction: f64,
    /// The depth below the stock top, mm: negative above it.
    pub depth_mm: f64,
    /// The height as the legend writes it: work-coordinate Z in project
    /// units, signed (`+10`, `0`, `−6`, `+11.8`); the unit is in the
    /// legend's title.
    pub label: String,
    /// What sits at this height, top to bottom of the legend's text.
    pub names: Vec<TickName>,
    /// The stock top.
    pub top: bool,
    /// The deepest cut in the project.
    pub deepest: bool,
}

impl LegendTick {
    /// The operations cutting to this depth, by name, in list order.
    pub fn operations(&self) -> impl Iterator<Item = &str> {
        self.names.iter().filter_map(|n| match n {
            TickName::Operation(name) => Some(name.as_str()),
            _ => None,
        })
    }

    /// The names as the legend writes them: `Screw holes, Cut out`.
    #[must_use]
    pub fn names_text(&self, strings: &Strings) -> String {
        let names: Vec<&str> = self.names.iter().map(|n| n.text(strings)).collect();
        names.join(", ")
    }

    /// The tick as one line: `−6 Square recess`.
    #[must_use]
    pub fn text(&self, strings: &Strings) -> String {
        format!("{} {}", self.label, self.names_text(strings))
    }
}

/// The depth legend of *By depth* (functional spec, capability 73a;
/// `experience.md`, section 9): above the stock top, the band where the
/// bit moves without cutting, with ticks at the clearance height and each
/// distinct safe Z; then ticks at the stock top, at each operation's
/// depth and at the deepest cut, top first. Heights that coincide share a
/// tick; the deepest cut is always some operation's depth, so it is that
/// tick. Labels are work-coordinate Z: with Z zero at the stock bottom
/// the stock top reads its thickness.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthLegend {
    /// The ramp the cutting ticks sit on.
    pub ramp: DepthRamp,
    /// The ticks, top first.
    pub ticks: Vec<LegendTick>,
    /// The project units the labels are in.
    pub units: Units,
}

/// The legend of `scene`'s toolpaths in `project`, or `None` when nothing
/// cuts below the stock top.
#[must_use]
pub fn depth_legend(project: &Project, scene: &Scene) -> Option<DepthLegend> {
    let ramp = DepthRamp::of(project, scene)?;
    let units = project.display_units;
    // Work Z zero, as a depth below the stock top (mm).
    let zero_depth_mm = match project.origin.z {
        OriginZ::StockTop => 0.0,
        OriginZ::StockBottom => ramp.top_z_mm,
    };
    let label = |depth_mm: f64| signed_label(zero_depth_mm - depth_mm, units);

    // The non-cutting heights above the stock top, highest first.
    let mut above: Vec<(f64, TickName)> = Vec::new();
    if let Some(clearance) = project.heights.clearance_mm {
        above.push((clearance, TickName::Clearance));
    }
    for op in scene.operations() {
        let Some(safe) = project
            .operations
            .iter()
            .find(|o| *o.id() == op.id)
            .map(dc_project::Operation::safe_z_mm)
            .filter(|z| z.is_finite() && *z > 0.0)
        else {
            continue;
        };
        let seen = above
            .iter()
            .any(|(h, n)| *n == TickName::SafeZ && (h - safe).abs() <= LEGEND_MERGE_MM);
        if !seen {
            above.push((safe, TickName::SafeZ));
        }
    }
    above.sort_by(|a, b| b.0.total_cmp(&a.0));
    let highest = above.first().map_or(0.0, |a| a.0);
    let mut ticks: Vec<LegendTick> = Vec::new();
    for (height_mm, name) in above {
        match ticks.last_mut() {
            Some(last) if (last.depth_mm + height_mm).abs() <= LEGEND_MERGE_MM => {
                last.names.push(name);
            }
            _ => ticks.push(LegendTick {
                fraction: -height_mm / highest,
                depth_mm: -height_mm,
                label: label(-height_mm),
                names: vec![name],
                top: false,
                deepest: false,
            }),
        }
    }

    ticks.push(LegendTick {
        fraction: 0.0,
        depth_mm: 0.0,
        label: label(0.0),
        names: vec![TickName::StockTop],
        top: true,
        deepest: false,
    });
    let mut depths: Vec<(f64, String)> = Vec::new();
    for op in scene.operations() {
        let deepest = op
            .runs
            .iter()
            .filter(|r| r.run.board == Board::Female)
            .flat_map(|r| cutting_z_mm(&r.run.toolpath))
            .reduce(f64::min);
        if let Some(z) = deepest.filter(|z| *z < ramp.top_z_mm - Z_EPS_MM) {
            depths.push((ramp.top_z_mm - z, op.name.clone()));
        }
    }
    depths.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (depth_mm, name) in depths {
        match ticks.last_mut() {
            Some(last) if !last.top && (last.depth_mm - depth_mm).abs() <= LEGEND_MERGE_MM => {
                last.names.push(TickName::Operation(name));
            }
            _ => ticks.push(LegendTick {
                fraction: ramp.fraction(ramp.top_z_mm - depth_mm),
                depth_mm,
                label: label(depth_mm),
                names: vec![TickName::Operation(name)],
                top: false,
                deepest: false,
            }),
        }
    }
    if let Some(last) = ticks.last_mut().filter(|t| !t.top) {
        last.deepest = true;
    }
    Some(DepthLegend { ramp, ticks, units })
}

/// One entry of the toolpath view's by-operation key (capability 73).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyEntry {
    /// The operation.
    pub id: OperationId,
    /// Its name, from the file.
    pub name: String,
    /// The preview palette's index the Viewport draws its first run in
    /// ([`crate::scene::SceneOperation::colour`]); the shell takes it
    /// modulo the palette's eight.
    pub colour: usize,
}

/// The by-operation key: every enabled operation in list order with the
/// colour the Viewport draws it in.
#[must_use]
pub fn operation_key(scene: &Scene) -> Vec<KeyEntry> {
    scene
        .operations()
        .iter()
        .map(|op| KeyEntry {
            id: op.id.clone(),
            name: op.name.clone(),
            colour: op.colour,
        })
        .collect()
}

/// Two operations whose depths differ by less than this share a tick,
/// mm: under what the label's decimals show.
const LEGEND_MERGE_MM: f64 = 1e-3;

/// The program extents the Viewport draws (capability 74): every stock
/// program's, joined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgramExtents {
    /// Where the tool centre goes, project space, mm.
    pub centre_mm: Rect,
    /// Where the cutter's edge reaches, project space, mm.
    pub edge_mm: Rect,
    /// The edge's X range in machine coordinates, `units`, as `check`
    /// prints it.
    pub edge_x: AxisRange,
    /// The edge's Y range likewise.
    pub edge_y: AxisRange,
    /// The tool centre's Z range in machine coordinates, `units`.
    pub z: Option<AxisRange>,
    /// The programs' units.
    pub units: Units,
}

/// The joint box of `a` and `b`.
fn join_rect(a: Rect, b: Rect) -> Rect {
    a.including(b.min).including(b.max)
}

/// The joint range of `a` and `b`.
fn join_range(a: Option<AxisRange>, b: Option<AxisRange>) -> Option<AxisRange> {
    match (a, b) {
        (Some(a), Some(b)) => Some(AxisRange {
            min: a.min.min(b.min),
            max: a.max.max(b.max),
        }),
        (a, None) => a,
        (None, b) => b,
    }
}

/// The extents of `finished`'s programs that cut the stock, joined, as
/// `check` prints them (`dc_job::summary::describe_extents`) and in
/// project space for the boxes; `None` before `finish` has run or when no
/// program moves in X and Y. The male board's (#498) are left for slice
/// 14, which draws that board.
#[must_use]
pub fn program_extents(project: &Project, finished: &Finished) -> Option<ProgramExtents> {
    let mut out: Option<ProgramExtents> = None;
    for summary in finished
        .programs
        .iter()
        .filter(|s| s.program.board == Board::Female)
    {
        let Some(boxes) = dc_job::project_extents(project, summary) else {
            continue;
        };
        let (Some(edge_x), Some(edge_y)) = (summary.edge_extents.x, summary.edge_extents.y) else {
            continue;
        };
        let next = ProgramExtents {
            centre_mm: boxes.centre,
            edge_mm: boxes.edge,
            edge_x,
            edge_y,
            z: summary.program.extents.z,
            units: summary.program.extents.units,
        };
        out = Some(match out {
            None => next,
            Some(o) => ProgramExtents {
                centre_mm: join_rect(o.centre_mm, next.centre_mm),
                edge_mm: join_rect(o.edge_mm, next.edge_mm),
                edge_x: join_range(Some(o.edge_x), Some(next.edge_x)).unwrap_or(o.edge_x),
                edge_y: join_range(Some(o.edge_y), Some(next.edge_y)).unwrap_or(o.edge_y),
                z: join_range(o.z, next.z),
                units: o.units,
            },
        });
    }
    out
}

/// A machine coordinate as the extents caption and the status bar write
/// it: up to three decimals in mm and four in inches, trailing zeros
/// dropped, a true minus sign.
#[must_use]
pub fn coordinate_label(value: f64, units: Units) -> String {
    let decimals = match units {
        Units::Mm => 3,
        Units::Inch => 4,
    };
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    };
    match text.strip_prefix('-') {
        Some(digits) if digits.chars().all(|c| c == '0' || c == '.') => "0".to_owned(),
        Some(digits) => format!("\u{2212}{digits}"),
        None => text,
    }
}

/// A range as `min..max` in [`coordinate_label`]'s form.
#[must_use]
pub fn range_label(range: AxisRange, units: Units) -> String {
    format!(
        "{}..{}",
        coordinate_label(range.min, units),
        coordinate_label(range.max, units)
    )
}

/// A work-coordinate Z (`z_mm`) as the legend writes it, in `units`: two
/// decimals in mm and three in inches at most, trailing zeros dropped,
/// with an explicit `+` above zero and a true minus below; `0` at zero.
#[must_use]
pub fn signed_label(z_mm: f64, units: Units) -> String {
    let decimals = match units {
        Units::Mm => 2,
        Units::Inch => 3,
    };
    let value = crate::units::to_display(z_mm, units);
    let text = format!("{:.decimals$}", value.abs());
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        text
    };
    if text.chars().all(|c| c == '0' || c == '.') {
        "0".to_owned()
    } else if value < 0.0 {
        format!("\u{2212}{text}")
    } else {
        format!("+{text}")
    }
}
