// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The setup sheet (functional spec, capability 77b;
//! `docs/specs/phase-4/design.md`, "Architecture → Export";
//! `experience.md`, section 15): one self-contained HTML page the user
//! opens and prints from a browser, stating what the export wrote. The
//! project, the export date, the post-processor and its version, the
//! stock, the origin and the display units; then each program in run
//! order with its file, its tool, its operations, the step before it, its
//! estimated time, its extents (tool centre and bit edge) and Z range,
//! and its pauses; then the export's warnings and the controller notes.
//!
//! Every fact comes from the export's own rows ([`ProgramSummary`]) and
//! the CLI's text where the CLI has one (the step before, the notes, the
//! board headings, the extents' numbers); nothing is computed afresh, so
//! no feed or depth is invented. The labels come from the caller's string
//! table through [`Label`]. Pure and deterministic: the date and the
//! DreamCarve version are the caller's. Inline CSS only, no script, no
//! external resource and no image; every text goes through [`escape`].
//! The app's shell writes the page; a later `export --setup-sheet` would
//! print the same bytes (design open question 7).

use dc_gcode::format::format_number;
use dc_gcode::{AxisRange, ProgramSplit};
use dc_project::{OriginXy, OriginZ, Project, ToolKind, Units};

use crate::build::{Finding, ProgramSummary};
use crate::summary::{
    EstimateConfig, board_heading, by_board, describe_estimated_time, describe_length,
};

/// A label of the sheet: one entry of the string table's `[setup_sheet]`
/// section (functional spec, capability 80; acceptance 91). `{name}`
/// placeholders are filled here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Label {
    /// The page title: `{name}`.
    Title,
    /// The line under the title: `{date}`.
    Exported,
    /// The post-processor row's label.
    Post,
    /// The post-processor: `{name}`, `{version}`.
    PostValue,
    /// The units row's label.
    Units,
    /// Millimetres, as the units row says.
    UnitsMm,
    /// Inches, as the units row says.
    UnitsIn,
    /// A range in millimetres: `{range}`.
    RangeMm,
    /// A range in inches: `{range}`.
    RangeIn,
    /// The stock row's label.
    Stock,
    /// The stock: `{width}`, `{height}`, `{thickness}`.
    StockValue,
    /// A value the project leaves unset.
    NotSet,
    /// The XY zero row's label.
    XyZero,
    /// The Z zero row's label.
    ZZero,
    /// XY zero at the bottom-left corner.
    XyBottomLeft,
    /// XY zero at the bottom-right corner.
    XyBottomRight,
    /// XY zero at the top-left corner.
    XyTopLeft,
    /// XY zero at the top-right corner.
    XyTopRight,
    /// XY zero at the centre.
    XyCentre,
    /// XY zero elsewhere: `{x}`, `{y}`.
    XyCustom,
    /// Z zero on the stock top.
    ZStockTop,
    /// Z zero on the stock bottom.
    ZStockBottom,
    /// The programs' heading.
    Programs,
    /// One program's heading: `{number}`, `{file}`.
    Program,
    /// The tool row's label.
    Tool,
    /// The tool: `{number}`, `{name}`.
    ToolValue,
    /// The tool type row's label.
    ToolType,
    /// An end mill.
    TypeEndMill,
    /// A ball-nose bit.
    TypeBallNose,
    /// A V-bit.
    TypeVBit,
    /// An engraving bit.
    TypeEngraver,
    /// A drill.
    TypeDrill,
    /// The diameter row's label.
    Diameter,
    /// The angle row's label.
    Angle,
    /// A V-bit's included angle: `{angle}`.
    AngleValue,
    /// The vendor row's label.
    Vendor,
    /// The product row's label.
    Product,
    /// The operations row's label.
    Operations,
    /// The step-before row's label.
    Before,
    /// The estimated time row's label.
    Time,
    /// The tool centre's extents row's label.
    ExtentsCentre,
    /// The bit edge's extents row's label.
    ExtentsEdge,
    /// The Z range row's label.
    ZRange,
    /// The pauses row's label.
    Pauses,
    /// The warnings' heading.
    Warnings,
    /// One warning: `{code}`, `{message}`.
    Warning,
    /// The notes' heading.
    Notes,
    /// The footer: `{name}`, `{date}`, `{version}`.
    Footer,
}

impl Label {
    /// Every label.
    pub const ALL: &'static [Self] = &[
        Self::Title,
        Self::Exported,
        Self::Post,
        Self::PostValue,
        Self::Units,
        Self::UnitsMm,
        Self::UnitsIn,
        Self::RangeMm,
        Self::RangeIn,
        Self::Stock,
        Self::StockValue,
        Self::NotSet,
        Self::XyZero,
        Self::ZZero,
        Self::XyBottomLeft,
        Self::XyBottomRight,
        Self::XyTopLeft,
        Self::XyTopRight,
        Self::XyCentre,
        Self::XyCustom,
        Self::ZStockTop,
        Self::ZStockBottom,
        Self::Programs,
        Self::Program,
        Self::Tool,
        Self::ToolValue,
        Self::ToolType,
        Self::TypeEndMill,
        Self::TypeBallNose,
        Self::TypeVBit,
        Self::TypeEngraver,
        Self::TypeDrill,
        Self::Diameter,
        Self::Angle,
        Self::AngleValue,
        Self::Vendor,
        Self::Product,
        Self::Operations,
        Self::Before,
        Self::Time,
        Self::ExtentsCentre,
        Self::ExtentsEdge,
        Self::ZRange,
        Self::Pauses,
        Self::Warnings,
        Self::Warning,
        Self::Notes,
        Self::Footer,
    ];

    /// Its key in the string table.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::Title => "setup_sheet.title",
            Self::Exported => "setup_sheet.exported",
            Self::Post => "setup_sheet.post",
            Self::PostValue => "setup_sheet.post_value",
            Self::Units => "setup_sheet.units",
            Self::UnitsMm => "setup_sheet.units_mm",
            Self::UnitsIn => "setup_sheet.units_in",
            Self::RangeMm => "setup_sheet.range_mm",
            Self::RangeIn => "setup_sheet.range_in",
            Self::Stock => "setup_sheet.stock",
            Self::StockValue => "setup_sheet.stock_value",
            Self::NotSet => "setup_sheet.not_set",
            Self::XyZero => "setup_sheet.xy_zero",
            Self::ZZero => "setup_sheet.z_zero",
            Self::XyBottomLeft => "setup_sheet.xy_bottom_left",
            Self::XyBottomRight => "setup_sheet.xy_bottom_right",
            Self::XyTopLeft => "setup_sheet.xy_top_left",
            Self::XyTopRight => "setup_sheet.xy_top_right",
            Self::XyCentre => "setup_sheet.xy_centre",
            Self::XyCustom => "setup_sheet.xy_custom",
            Self::ZStockTop => "setup_sheet.z_stock_top",
            Self::ZStockBottom => "setup_sheet.z_stock_bottom",
            Self::Programs => "setup_sheet.programs",
            Self::Program => "setup_sheet.program",
            Self::Tool => "setup_sheet.tool",
            Self::ToolValue => "setup_sheet.tool_value",
            Self::ToolType => "setup_sheet.tool_type",
            Self::TypeEndMill => "setup_sheet.type_end_mill",
            Self::TypeBallNose => "setup_sheet.type_ball_nose",
            Self::TypeVBit => "setup_sheet.type_v_bit",
            Self::TypeEngraver => "setup_sheet.type_engraver",
            Self::TypeDrill => "setup_sheet.type_drill",
            Self::Diameter => "setup_sheet.diameter",
            Self::Angle => "setup_sheet.angle",
            Self::AngleValue => "setup_sheet.angle_value",
            Self::Vendor => "setup_sheet.vendor",
            Self::Product => "setup_sheet.product",
            Self::Operations => "setup_sheet.operations",
            Self::Before => "setup_sheet.before",
            Self::Time => "setup_sheet.time",
            Self::ExtentsCentre => "setup_sheet.extents_centre",
            Self::ExtentsEdge => "setup_sheet.extents_edge",
            Self::ZRange => "setup_sheet.z_range",
            Self::Pauses => "setup_sheet.pauses",
            Self::Warnings => "setup_sheet.warnings",
            Self::Warning => "setup_sheet.warning",
            Self::Notes => "setup_sheet.notes",
            Self::Footer => "setup_sheet.footer",
        }
    }
}

/// What one export wrote, for its setup sheet.
#[derive(Debug, Clone, Copy)]
pub struct Sheet<'a> {
    /// The project exported.
    pub project: &'a Project,
    /// The programs' rows, in run order, as [`crate::summarize`] lists
    /// them.
    pub programs: &'a [ProgramSummary],
    /// Each program's file name, in the same order.
    pub names: &'a [String],
    /// The export's warnings.
    pub warnings: &'a [Finding],
    /// The program split the export used.
    pub split: ProgramSplit,
    /// The user's `[estimate]` settings, as the export applied them.
    pub estimate: EstimateConfig,
    /// The export date, as the programs' headers have it.
    pub date: &'a str,
    /// The DreamCarve version, for the footer.
    pub version: &'a str,
}

/// The sheet's file name beside programs named under `stem`:
/// `<stem>-setup.html`.
#[must_use]
pub fn file_name(stem: &str) -> String {
    format!("{stem}-setup.html")
}

/// `text` safe in HTML text and attribute values: the one escape every
/// text on the page goes through.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// `template` with each `{name}` replaced by the value `args` gives; a
/// placeholder `args` does not name stays as written.
fn fill(template: &str, args: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let (before, from_brace) = rest.split_at(open);
        out.push_str(before);
        let Some(close) = from_brace.find('}') else {
            out.push_str(from_brace);
            return out;
        };
        let name = from_brace.get(1..close).unwrap_or_default();
        match args.iter().find(|(n, _)| *n == name) {
            Some((_, value)) => out.push_str(value),
            None => out.push_str(from_brace.get(..=close).unwrap_or_default()),
        }
        rest = from_brace.get(close + 1..).unwrap_or_default();
    }
    out.push_str(rest);
    out
}

/// `range` on `axis` as the CLI writes a program's extents (#62):
/// `X 15.000..155.000`, in the program's units with its post-processor's
/// decimal places.
fn describe_axis(project: &Project, units: Units, axis: &str, range: AxisRange) -> String {
    // The program was emitted with this post-processor, so it resolves.
    let post = project.post_processor.resolve().ok();
    let number = |value: f64| {
        post.as_ref()
            .and_then(|post| format_number(value, &post.number_format, units).ok())
            .unwrap_or_else(|| value.to_string())
    };
    format!("{axis} {}..{}", number(range.min), number(range.max))
}

/// Program `summary`'s extents as the sheet states them, each as the CLI
/// writes the numbers: the tool centre in X and Y, the bit's edge in X
/// and Y (its extents widened by its largest tool's radius, #62), and the
/// Z range of the tool centre. A range a program never moves on is left
/// out, and an empty one is an empty string.
#[must_use]
pub fn ranges(project: &Project, summary: &ProgramSummary) -> [String; 3] {
    let centre = &summary.program.extents;
    let edge = &summary.edge_extents;
    let xy = |extents: &dc_gcode::Extents| {
        [("X", extents.x), ("Y", extents.y)]
            .into_iter()
            .filter_map(|(axis, range)| {
                range.map(|r| describe_axis(project, extents.units, axis, r))
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let z = centre
        .z
        .map(|r| describe_axis(project, centre.units, "Z", r))
        .unwrap_or_default();
    [xy(centre), xy(edge), z]
}

/// The page.
struct Page<'t> {
    text: &'t dyn Fn(Label) -> String,
    html: String,
}

impl Page<'_> {
    /// `label`'s text, placeholders filled, escaped.
    fn label(&self, label: Label, args: &[(&str, &str)]) -> String {
        escape(&fill(&(self.text)(label), args))
    }

    fn push(&mut self, html: &str) {
        self.html.push_str(html);
    }

    /// One row of a facts table: the label, then the escaped value.
    fn row(&mut self, label: Label, value: &str) {
        let label = self.label(label, &[]);
        self.html.push_str(&format!(
            "<tr><th>{label}</th><td>{}</td></tr>\n",
            escape(value)
        ));
    }

    /// One row whose value is several lines.
    fn rows(&mut self, label: Label, values: &[String]) {
        let label = self.label(label, &[]);
        let values: Vec<String> = values.iter().map(|v| escape(v)).collect();
        self.html.push_str(&format!(
            "<tr><th>{label}</th><td>{}</td></tr>\n",
            values.join("<br>")
        ));
    }
}

/// The inline style sheet (`experience.md`, section 15): black on white,
/// a system sans-serif, 11 pt body, 16 pt title, 0.5-pt rules, one
/// program per block kept on one page, 15-mm margins, A4 and Letter.
const STYLE: &str = "@page { size: auto; margin: 15mm; }
body { margin: 0; color: #000; background: #fff; font-family: -apple-system, \"Segoe UI\", Inter, sans-serif; font-size: 11pt; line-height: 1.35; }
h1 { font-size: 16pt; margin: 0 0 2pt; }
h2 { font-size: 12pt; margin: 14pt 0 4pt; }
h3 { font-size: 11pt; margin: 0 0 4pt; }
p { margin: 0 0 6pt; }
table { border-collapse: collapse; width: 100%; }
th, td { border-bottom: 0.5pt solid #000; padding: 3pt 8pt 3pt 0; text-align: left; vertical-align: top; }
th { font-weight: 600; width: 32%; }
td { font-variant-numeric: tabular-nums; }
section { page-break-inside: avoid; break-inside: avoid; margin: 0 0 12pt; }
ul { margin: 0; padding-left: 14pt; }
footer { margin-top: 18pt; border-top: 0.5pt solid #000; padding-top: 4pt; font-size: 9pt; }
";

/// The setup sheet of `sheet`, its labels from `text`: one
/// self-contained HTML page, byte-identical for the same input.
#[must_use]
pub fn render(sheet: &Sheet<'_>, text: &dyn Fn(Label) -> String) -> String {
    let project = sheet.project;
    let mut page = Page {
        text,
        html: String::new(),
    };
    let title = page.label(Label::Title, &[("name", &project.name)]);
    page.push("<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n");
    page.push(&format!(
        "<title>{title}</title>\n<style>\n{STYLE}</style>\n"
    ));
    page.push("</head>\n<body>\n");
    page.push(&format!("<h1>{title}</h1>\n"));
    let exported = page.label(Label::Exported, &[("date", sheet.date)]);
    page.push(&format!("<p>{exported}</p>\n"));
    setup(&mut page, project);
    programs(&mut page, sheet);
    if !sheet.warnings.is_empty() {
        let heading = page.label(Label::Warnings, &[]);
        page.push(&format!("<h2>{heading}</h2>\n<ul>\n"));
        for warning in sheet.warnings {
            let line = page.label(
                Label::Warning,
                &[
                    ("code", warning.code.as_deref().unwrap_or_default()),
                    ("message", &warning.message),
                ],
            );
            page.push(&format!("<li>{line}</li>\n"));
        }
        page.push("</ul>\n");
    }
    let notes = crate::report::notes(project, sheet.programs, sheet.split);
    if !notes.is_empty() {
        let heading = page.label(Label::Notes, &[]);
        page.push(&format!("<h2>{heading}</h2>\n<ul>\n"));
        for note in &notes {
            page.push(&format!("<li>{}</li>\n", escape(note)));
        }
        page.push("</ul>\n");
    }
    let footer = page.label(
        Label::Footer,
        &[
            ("name", &project.name),
            ("date", sheet.date),
            ("version", sheet.version),
        ],
    );
    page.push(&format!("<footer>{footer}</footer>\n</body>\n</html>\n"));
    page.html
}

/// The setup: the post-processor, the units, the stock and the origin.
fn setup(page: &mut Page<'_>, project: &Project) {
    let post = &project.post_processor;
    let name = post
        .resolve()
        .map_or_else(|_| post.id.clone(), |resolved| resolved.name);
    page.push("<table>\n");
    let value = fill(
        &(page.text)(Label::PostValue),
        &[
            ("name", &name),
            ("version", &post.definition_version.to_string()),
        ],
    );
    page.row(Label::Post, &value);
    let units = match project.display_units {
        Units::Mm => Label::UnitsMm,
        Units::Inch => Label::UnitsIn,
    };
    page.row(Label::Units, &(page.text)(units));
    let stock = &project.stock;
    let thickness = stock.thickness_mm.map_or_else(
        || (page.text)(Label::NotSet),
        |t| describe_length(project, t),
    );
    let value = fill(
        &(page.text)(Label::StockValue),
        &[
            ("width", &describe_length(project, stock.width_mm)),
            ("height", &describe_length(project, stock.height_mm)),
            ("thickness", &thickness),
        ],
    );
    page.row(Label::Stock, &value);
    let xy = match &project.origin.xy {
        OriginXy::BottomLeft => (page.text)(Label::XyBottomLeft),
        OriginXy::BottomRight => (page.text)(Label::XyBottomRight),
        OriginXy::TopLeft => (page.text)(Label::XyTopLeft),
        OriginXy::TopRight => (page.text)(Label::XyTopRight),
        OriginXy::Centre => (page.text)(Label::XyCentre),
        OriginXy::Custom { x_mm, y_mm } => fill(
            &(page.text)(Label::XyCustom),
            &[
                ("x", &describe_length(project, *x_mm)),
                ("y", &describe_length(project, *y_mm)),
            ],
        ),
    };
    page.row(Label::XyZero, &xy);
    let z = match project.origin.z {
        OriginZ::StockTop => Label::ZStockTop,
        OriginZ::StockBottom => Label::ZStockBottom,
    };
    page.row(Label::ZZero, &(page.text)(z));
    page.push("</table>\n");
}

/// Each program in run order, by board with an Inlay (#498).
fn programs(page: &mut Page<'_>, sheet: &Sheet<'_>) {
    let project = sheet.project;
    let heading = page.label(Label::Programs, &[]);
    page.push(&format!("<h2>{heading}</h2>\n"));
    let boards = by_board(project);
    let mut board = None;
    let mut k_in_board = 0;
    for (summary, name) in sheet.programs.iter().zip(sheet.names) {
        let program = &summary.program;
        if boards && board != Some(program.board) {
            board = Some(program.board);
            k_in_board = 0;
            // The CLI's heading: the board and its size.
            let heading = escape(&board_heading(project, program.board));
            page.push(&format!("<h2>{heading}</h2>\n"));
        }
        k_in_board += 1;
        let heading = page.label(
            Label::Program,
            &[("number", &k_in_board.to_string()), ("file", name)],
        );
        page.push(&format!("<section>\n<h3>{heading}</h3>\n<table>\n"));
        tool(page, project, summary);
        let operations: Vec<&str> = summary
            .operations
            .iter()
            .filter_map(|id| project.operations.iter().find(|o| o.id() == id))
            .map(dc_project::Operation::name)
            .collect();
        page.row(Label::Operations, &operations.join(", "));
        page.row(Label::Before, &summary.before);
        // The CLI's time, or its line saying why there is none.
        let time = summary.estimate.map_or_else(
            || summary.estimate_line.clone(),
            |estimate| describe_estimated_time(&estimate, &sheet.estimate),
        );
        page.row(Label::Time, &time);
        let unit = match program.extents.units {
            Units::Mm => Label::RangeMm,
            Units::Inch => Label::RangeIn,
        };
        let [centre, edge, z] = ranges(project, summary);
        for (label, range) in [
            (Label::ExtentsCentre, centre),
            (Label::ExtentsEdge, edge),
            (Label::ZRange, z),
        ] {
            if !range.is_empty() {
                let value = fill(&(page.text)(unit), &[("range", &range)]);
                page.row(label, &value);
            }
        }
        if !program.pauses.is_empty() {
            let pauses: Vec<String> = program
                .pauses
                .iter()
                .map(|pause| {
                    let message = if pause.message.is_empty() {
                        "(no message)"
                    } else {
                        pause.message.as_str()
                    };
                    format!("line {}: {message}", pause.line)
                })
                .collect();
            page.rows(Label::Pauses, &pauses);
        }
        page.push("</table>\n</section>\n");
    }
}

/// The rows of `summary`'s tool: the one fitted before it starts.
fn tool(page: &mut Page<'_>, project: &Project, summary: &ProgramSummary) {
    let first = &summary.program.first_tool;
    let value = fill(
        &(page.text)(Label::ToolValue),
        &[("number", &first.number.to_string()), ("name", &first.name)],
    );
    page.row(Label::Tool, &value);
    let Some(tool) = project.tools.iter().find(|t| t.id == first.id) else {
        return;
    };
    let kind = match tool.cutter {
        ToolKind::EndMill => Label::TypeEndMill,
        ToolKind::BallNose => Label::TypeBallNose,
        ToolKind::VBit { .. } => Label::TypeVBit,
        ToolKind::Engraver => Label::TypeEngraver,
        ToolKind::Drill => Label::TypeDrill,
    };
    page.row(Label::ToolType, &(page.text)(kind));
    page.row(Label::Diameter, &describe_length(project, tool.diameter_mm));
    if let ToolKind::VBit { included_angle_deg } = tool.cutter {
        let value = fill(
            &(page.text)(Label::AngleValue),
            &[("angle", &included_angle_deg.to_string())],
        );
        page.row(Label::Angle, &value);
    }
    if let Some(vendor) = tool.vendor.as_deref().filter(|v| !v.is_empty()) {
        page.row(Label::Vendor, vendor);
    }
    if let Some(product) = tool.product_id.as_deref().filter(|p| !p.is_empty()) {
        page.row(Label::Product, product);
    }
}
