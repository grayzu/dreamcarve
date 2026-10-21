// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve init <svg|dxf>`: imports the drawing with `dc-import` and
//! writes a `.dcv` project with the defaults of `docs/specs/phase-1/design.md`,
//! "CLI (dreamcarve)": stock = drawing bounds plus a 10 mm margin on each
//! side; origin bottom-left at the stock top; one placeholder tool "Set me"
//! (6.35 mm, 2 flutes, no feeds or step-down); one outside climb Profile of
//! every closed shape, safe Z 5 mm, four 8 × 3 mm rectangular tabs. The
//! format is the file's extension; a DXF takes the DXF-only `--units`
//! override, `--join-tolerance` and `--layer` (`docs/specs/phase-2/design.md`,
//! "CLI additions").
//!
//! The stock thickness, the Profile depth, the feeds, the spindle speed and
//! the step-down are left unset (`null`), so `export` refuses the project
//! until the user fills them in (S3, #64, #98): `init` cannot know the
//! material or the machine. The step-down is the tool's: the operation's,
//! which would override it, stays `null` too.
//! `init` lists each of them as `coaster.dcv:line:column: …`, a link an
//! editor or terminal opens at the `null` to replace (#76).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, MAX_COORDINATE_MM, Rect, Transform};
use dc_import::{
    DxfImportOptions, ImportError, ImportWarning, JOIN_TOLERANCE_DEFAULT_MM, SvgImportOptions,
    import_dxf, import_svg,
};
use dc_project::{
    CURRENT_VERSION, CornerRelief, CutDirection, Entry, FeatureDefinition, FeatureInstance,
    MachineOrigin, MachiningOrder, Operation, OperationId, OriginXy, OriginZ, PostProcessorRef,
    ProfileOp, Project, Side, SourceMap, Stock, TabPlacement, TabShape, TabSpec, Tool, ToolId,
    ToolKind, Units, validate,
};

use crate::cli::{Failure, InitArgs, StagedFile, UnitsArg, lookup_post, read_file};
use crate::config;
use crate::output::{errln, outln};

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
/// The placeholder tool: a 1/4" two-flute end mill.
const TOOL_ID: &str = "t1";
const TOOL_NAME: &str = "Set me";
const TOOL_DIAMETER_MM: f64 = 6.35;
const TOOL_FLUTES: u8 = 2;
/// The one operation `init` writes, a Profile, and its defaults.
const OPERATION_ID: &str = "op1";
/// Arc-fitting tolerance a new project starts with, mm (functional spec,
/// capability 24).
const ARC_FIT_TOLERANCE_MM: f64 = 0.01;
const SAFE_Z_MM: f64 = 5.0;
const TAB_COUNT: u8 = 4;
const TAB_WIDTH_MM: f64 = 8.0;
const TAB_HEIGHT_MM: f64 = 3.0;
/// The ramp angle a new project's Profile enters each pass at, degrees
/// (functional spec, capability 16; #119).
const INIT_RAMP_ANGLE_DEG: f64 = dc_cam::entry::INIT_RAMP_ANGLE_DEG;

/// What an importer produced, whichever format it read.
struct Imported {
    definitions: Vec<FeatureDefinition>,
    instances: Vec<FeatureInstance>,
    bounds: Option<Rect>,
}

/// Runs `init`.
pub fn run(args: &InitArgs) -> Result<ExitCode, Failure> {
    let mut post = lookup_post(&args.post)?;
    // The user's configured defaults (`tool_change_mode`) go on top of the
    // definition's, before anything is written (capability 19).
    config::seed_post(&mut post)?;
    let is_dxf = args
        .drawing
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dxf"));
    if !is_dxf && (args.join_tolerance.is_some() || !args.layers.is_empty()) {
        return Err(Failure::usage(
            "--join-tolerance and --layer apply to DXF drawings only",
        ));
    }
    let output = args
        .output
        .clone()
        .unwrap_or_else(|| args.drawing.with_extension("dcv"));
    if output.exists() {
        // The .dcv is where the user's edits live; never overwrite one.
        return Err(Failure::error(format!(
            "{} already exists; remove it or choose another file with -o",
            output.display()
        )));
    }

    let bytes = read_file(&args.drawing)?;
    let (import, warnings) = if is_dxf {
        import_dxf_drawing(args, &bytes)?
    } else {
        import_svg_drawing(args, &bytes)?
    };
    for warning in &warnings {
        errln!("warning: {warning}");
    }
    let Some(bounds) = import.bounds else {
        return Err(Failure::error(format!(
            "{} contains no shapes to import",
            args.drawing.display()
        )));
    };

    let name = project_name(&args.drawing);
    let display_units = args.units.unwrap_or(UnitsArg::Mm).into();
    let project = default_project(name, import, bounds, post, display_units);
    // The drawing imported within the limit, but the margin can push the
    // stock past it; refuse rather than write a project `check` rejects.
    let stock = &project.stock;
    if stock.width_mm > MAX_COORDINATE_MM || stock.height_mm > MAX_COORDINATE_MM {
        return Err(Failure::error(format!(
            "{} with a {STOCK_MARGIN_MM} mm margin needs a {:.2} × {:.2} mm stock, \
             larger than the {MAX_COORDINATE_MM} mm limit; make the drawing smaller",
            args.drawing.display(),
            stock.width_mm,
            stock.height_mm,
        )));
    }
    let text = dc_project::save(&project).map_err(Failure::error)?;
    StagedFile::write(&output, text.as_bytes())?.commit()?;

    report(&project, &output, &text)?;
    Ok(ExitCode::SUCCESS)
}

/// Imports an SVG and prints what was read.
fn import_svg_drawing(
    args: &InitArgs,
    bytes: &[u8],
) -> Result<(Imported, Vec<ImportWarning>), Failure> {
    let (import, warnings) = import_svg(bytes, &SvgImportOptions::new())
        .map_err(|e| Failure::error(format!("cannot import {}: {e}", args.drawing.display())))?;
    outln!(
        "Imported {} shape(s) from {}: page units {}, 1 user unit = {:.4} mm.",
        import.definitions.len(),
        args.drawing.display(),
        import.units,
        import.mm_per_user_unit
    )?;
    Ok((
        Imported {
            definitions: import.definitions,
            instances: import.instances,
            bounds: import.bounds,
        },
        warnings,
    ))
}

/// Imports a DXF with the DXF-only flags and prints what was read. A file
/// with no usable `$INSUNITS` is refused with the flag to pass.
fn import_dxf_drawing(
    args: &InitArgs,
    bytes: &[u8],
) -> Result<(Imported, Vec<ImportWarning>), Failure> {
    let options = DxfImportOptions {
        units: args.units.map(Into::into),
        join_tolerance_mm: args.join_tolerance.unwrap_or(JOIN_TOLERANCE_DEFAULT_MM),
        layers: (!args.layers.is_empty()).then(|| args.layers.clone()),
        ..DxfImportOptions::new()
    };
    let (import, warnings) = import_dxf(bytes, &options).map_err(|e| {
        let hint = match e {
            ImportError::UnitsUnknown { .. } => "; pass `--units mm|inch`",
            _ => "",
        };
        Failure::error(format!(
            "cannot import {}: {e}{hint}",
            args.drawing.display()
        ))
    })?;
    let units = match (args.units, import.units) {
        (Some(units), _) => format!("units {units} (from --units)"),
        (None, Some(units)) => format!("units {units} (from $INSUNITS)"),
        // Unreachable: without either, the import fails above.
        (None, None) => "units unknown".to_owned(),
    };
    outln!(
        "Imported {} shape(s) from {}: {units}, 1 drawing unit = {:.4} mm.",
        import.definitions.len(),
        args.drawing.display(),
        import.mm_per_unit
    )?;
    Ok((
        Imported {
            definitions: import.definitions,
            instances: import.instances,
            bounds: import.bounds,
        },
        warnings,
    ))
}

/// The project name: the drawing's file name without its extension.
fn project_name(drawing: &Path) -> String {
    drawing.file_stem().map_or_else(
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
    import: Imported,
    bounds: Rect,
    post_processor: PostProcessorRef,
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
        number: None,
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
        step_down_mm: None,
        direction: CutDirection::Climb,
        through_cut_allowance_mm: 0.0,
        safe_z_mm: SAFE_Z_MM,
        tabs: Some(TabSpec {
            placement: TabPlacement::Count { count: TAB_COUNT },
            width_mm: TAB_WIDTH_MM,
            height_mm: TAB_HEIGHT_MM,
            shape: TabShape::Rect,
        }),
        machining_order: MachiningOrder::FullDepthFirst,
        entry: Entry::Ramp {
            angle_deg: INIT_RAMP_ANGLE_DEG,
        },
        lead: None,
        corner_relief: CornerRelief::None,
    };
    Project {
        version: CURRENT_VERSION,
        name,
        display_units,
        flatten_tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
        arc_fit_tolerance_mm: Some(ARC_FIT_TOLERANCE_MM),
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

/// Prints what was written and what the user must do next: one line per
/// value to set, located in `text`, the file as written.
fn report(project: &Project, output: &Path, text: &str) -> Result<(), Failure> {
    let targets = project
        .operations
        .iter()
        .map(|op| op.targets().len())
        .sum::<usize>();
    let untargeted = project.instances.len() - targets.min(project.instances.len());
    outln!(
        "Wrote {}: stock {:.2} × {:.2} mm, post-processor {}, {} shape(s) targeted by Profile.",
        output.display(),
        project.stock.width_mm,
        project.stock.height_mm,
        project.post_processor.id,
        targets,
    )?;
    if untargeted > 0 {
        outln!(
            "{untargeted} shape(s) are open paths and are not cut; `dreamcarve check` lists them."
        )?;
    }
    outln!("Values to set; export refuses until every one is set:")?;
    let map = SourceMap::parse(text).ok();
    for problem in validate(project).iter().filter(|p| p.is_unset_value()) {
        let at = problem
            .field()
            .and_then(|field| map.as_ref()?.locate(&field));
        match at {
            Some(at) => outln!("{}:{at}: {problem}", output.display())?,
            None => outln!("{problem}")?,
        }
    }
    outln!(
        "Next: edit {} and replace each `null` listed above. To cut through, `depth_mm` is \
         the thickness plus `through_cut_allowance_mm`. Then run `dreamcarve check {}`.",
        output.display(),
        output.display()
    )
}
