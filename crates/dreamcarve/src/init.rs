// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve init <svg>`: imports the drawing with `dc-import` and writes
//! a `.dcv` project with the defaults of `docs/specs/phase-1/design.md`,
//! "CLI (dreamcarve)": stock = drawing bounds plus a 10 mm margin on each
//! side; origin bottom-left at the stock top; one placeholder tool "Set me"
//! (6.35 mm, 2 flutes, no feeds); one outside climb Profile of every closed
//! shape in 4 mm passes, safe Z 5 mm, four 8 × 3 mm rectangular tabs.
//!
//! The stock thickness, the Profile depth, the feeds and the spindle speed
//! are left unset (`null`), so `export` refuses the project until the user
//! fills them in (S3, #64): `init` cannot know the material or the machine.

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, MAX_COORDINATE_MM, Rect, Transform};
use dc_import::{SvgImport, SvgImportOptions, import_svg};
use dc_project::{
    CURRENT_VERSION, CutDirection, MachineOrigin, Operation, OperationId, OriginXy, OriginZ,
    PostProcessor, ProfileOp, Project, Side, Stock, TabPlacement, TabShape, TabSpec, Tool, ToolId,
    ToolKind, Units,
};

use crate::cli::{Failure, InitArgs, StagedFile, lookup_post, read_file};

/// Margin between the drawing's bounds and each stock edge, mm.
const STOCK_MARGIN_MM: f64 = 10.0;
/// Stock sizes and instance offsets are rounded up to this many steps per
/// millimetre (0.01 mm), so the file reads cleanly.
const ROUND_STEPS_PER_MM: f64 = 100.0;
/// How far a value may sit above a 0.01 mm step and still round down to it.
/// The imported bounds come from flattened curves and carry noise of a few
/// micrometres (the coaster's circle spans 4.9999981..95.0000026 mm), which
/// would otherwise push a 110 mm stock to 110.01 and shift the drawing by
/// 0.01 mm. The margin can fall short by less than this, which is below the
/// 0.001 mm resolution programs are written with.
const ROUND_SLACK_MM: f64 = 0.001;
/// The instances' `z_top_mm`, which the importer needs. Nothing in phase 1
/// reads it: depth and tab heights come from the stock thickness and the
/// operation, which `init` leaves unset (#64).
const FEATURE_TOP_MM: f64 = 12.0;
/// The placeholder tool: a 1/4" two-flute end mill.
const TOOL_ID: &str = "t1";
const TOOL_NAME: &str = "Set me";
const TOOL_DIAMETER_MM: f64 = 6.35;
const TOOL_FLUTES: u8 = 2;
/// The one operation `init` writes, a Profile, and its defaults.
const OPERATION_ID: &str = "op1";
const STEP_DOWN_MM: f64 = 4.0;
const SAFE_Z_MM: f64 = 5.0;
const TAB_COUNT: u8 = 4;
const TAB_WIDTH_MM: f64 = 8.0;
const TAB_HEIGHT_MM: f64 = 3.0;

/// Runs `init`.
pub fn run(args: &InitArgs) -> Result<ExitCode, Failure> {
    let post = lookup_post(&args.post)?;
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| args.svg.with_extension("dcv"));
    if output.exists() {
        // The .dcv is where the user's edits live; never overwrite one.
        return Err(Failure::error(format!(
            "{} already exists; remove it or choose another file with -o",
            output.display()
        )));
    }

    let bytes = read_file(&args.svg)?;
    let (import, warnings) = import_svg(&bytes, &SvgImportOptions::new(FEATURE_TOP_MM))
        .map_err(|e| Failure::error(format!("cannot import {}: {e}", args.svg.display())))?;
    for warning in &warnings {
        eprintln!("warning: {warning}");
    }
    let Some(bounds) = import.bounds else {
        return Err(Failure::error(format!(
            "{} contains no shapes to import",
            args.svg.display()
        )));
    };
    println!(
        "Imported {} shape(s) from {}: page units {}, 1 user unit = {:.4} mm.",
        import.definitions.len(),
        args.svg.display(),
        import.units,
        import.mm_per_user_unit
    );

    let name = project_name(&args.svg);
    let project = default_project(name, import, bounds, post, args.units.into());
    // The drawing imported within the limit, but the margin can push the
    // stock past it; refuse rather than write a project `check` rejects.
    let stock = &project.stock;
    if stock.width_mm > MAX_COORDINATE_MM || stock.height_mm > MAX_COORDINATE_MM {
        return Err(Failure::error(format!(
            "{} with a {STOCK_MARGIN_MM} mm margin needs a {:.2} × {:.2} mm stock, \
             larger than the {MAX_COORDINATE_MM} mm limit; make the drawing smaller",
            args.svg.display(),
            stock.width_mm,
            stock.height_mm,
        )));
    }
    let text = dc_project::save(&project).map_err(Failure::error)?;
    StagedFile::write(&output, text.as_bytes())?.commit()?;

    report(&project, &output);
    Ok(ExitCode::SUCCESS)
}

/// The project name: the SVG's file name without its extension.
fn project_name(svg: &Path) -> String {
    svg.file_stem().map_or_else(
        || "untitled".to_owned(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// `value_mm` rounded up to the next multiple of 0.01 mm, ignoring up to
/// `ROUND_SLACK_MM` of noise above a multiple.
fn round_up_mm(value_mm: f64) -> f64 {
    ((value_mm - ROUND_SLACK_MM) * ROUND_STEPS_PER_MM).ceil() / ROUND_STEPS_PER_MM
}

/// Builds the project with the `init` defaults. Instances are moved so the
/// drawing's bounds sit at least `STOCK_MARGIN_MM` in from the stock's
/// bottom-left corner; definitions keep the imported coordinates. The
/// imported bounds carry float noise from flattening, so the offset and the
/// stock size are rounded up to 0.01 mm: the drawing stays at least
/// `STOCK_MARGIN_MM` (less under `ROUND_SLACK_MM`) from every edge, and the
/// file reads cleanly.
fn default_project(
    name: String,
    import: SvgImport,
    bounds: Rect,
    post_processor: PostProcessor,
    display_units: Units,
) -> Project {
    let dx_mm = round_up_mm(STOCK_MARGIN_MM - bounds.min.x);
    let dy_mm = round_up_mm(STOCK_MARGIN_MM - bounds.min.y);
    let shift = Transform::translate(dx_mm, dy_mm);
    let mut instances = import.instances;
    for instance in &mut instances {
        instance.transform = instance.transform.then(shift);
    }
    let definitions: BTreeMap<_, _> = import
        .definitions
        .into_iter()
        .map(|d| (d.id.clone(), d))
        .collect();
    // Profile cuts closed shapes only; open paths stay in the project for
    // the user to see, untargeted (functional spec, capability 8).
    let targets = instances
        .iter()
        .filter(|i| {
            definitions
                .get(&i.definition)
                .is_some_and(|d| d.profiles.iter().any(|p| p.is_closed()))
        })
        .map(|i| i.id.clone())
        .collect();
    let tool = Tool {
        id: ToolId::new(TOOL_ID),
        name: TOOL_NAME.into(),
        cutter: ToolKind::EndMill,
        diameter_mm: TOOL_DIAMETER_MM,
        flutes: TOOL_FLUTES,
        feed_mm_min: None,
        plunge_mm_min: None,
        rpm: None,
        step_down_mm: None,
        step_over_fraction: None,
    };
    let profile = ProfileOp {
        id: OperationId::new(OPERATION_ID),
        name: "Profile".into(),
        enabled: true,
        targets,
        tool: tool.id.clone(),
        side: Side::Outside,
        depth_mm: None,
        step_down_mm: Some(STEP_DOWN_MM),
        direction: CutDirection::Climb,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: SAFE_Z_MM,
        tabs: Some(TabSpec {
            placement: TabPlacement::Count { count: TAB_COUNT },
            width_mm: TAB_WIDTH_MM,
            height_mm: TAB_HEIGHT_MM,
            shape: TabShape::Rect,
        }),
    };
    Project {
        version: CURRENT_VERSION,
        name,
        display_units,
        flatten_tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
        stock: Stock {
            width_mm: round_up_mm(bounds.max.x + dx_mm + STOCK_MARGIN_MM),
            height_mm: round_up_mm(bounds.max.y + dy_mm + STOCK_MARGIN_MM),
            thickness_mm: None,
        },
        origin: MachineOrigin {
            xy: OriginXy::BottomLeft,
            z: OriginZ::StockTop,
        },
        definitions,
        instances,
        tools: vec![tool],
        operations: vec![Operation::Profile(profile)],
        post_processor,
    }
}

/// Prints what was written and what the user must do next.
fn report(project: &Project, output: &Path) {
    let targets = project
        .operations
        .iter()
        .map(|op| op.targets().len())
        .sum::<usize>();
    let untargeted = project.instances.len() - targets.min(project.instances.len());
    println!(
        "Wrote {}: stock {:.2} × {:.2} mm, post-processor {}, {} shape(s) targeted by Profile.",
        output.display(),
        project.stock.width_mm,
        project.stock.height_mm,
        project.post_processor.id,
        targets,
    );
    if untargeted > 0 {
        println!(
            "{untargeted} shape(s) are open paths and are not cut; `dreamcarve check` lists them."
        );
    }
    println!(
        "Next: edit {} to set `stock.thickness_mm` to your material's measured thickness, \
         the Profile depth `operations[{OPERATION_ID}].depth_mm` (to cut through: the thickness \
         plus `through_cut_allowance_mm`), and tool `{TOOL_ID}` ({TOOL_NAME}): feed_mm_min, \
         plunge_mm_min and rpm. Export refuses until they are set.",
        output.display()
    );
}
