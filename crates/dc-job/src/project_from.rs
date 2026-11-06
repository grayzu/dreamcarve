// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! A new project from a drawing (`docs/specs/phase-4/design.md`, "Data
//! model → dc-job"), moved from `crates/dreamcarve/src/init.rs` with its
//! defaults unchanged (`docs/specs/phase-1/design.md`, "CLI (dreamcarve)"):
//! stock = drawing bounds plus a 10 mm margin on each side; origin
//! bottom-left at the stock top; and, with defaults, one placeholder tool
//! "Set me" (6.35 mm, 2 flutes, no feeds or step-down) and one outside
//! climb Profile of every closed shape, safe Z 5 mm, asking for four
//! 8 × 3 mm rectangular tabs on each part it cuts and none on its holes
//! (#105, ADR-0005). The app passes `with_defaults: false` (functional
//! spec, open question 1): shapes and stock only, since its empty
//! Operations list and *Add* do what the placeholder does for a CLI user.
//!
//! The stock thickness, the Profile depth, the feeds, the spindle speed and
//! the step-down are left unset (`null`), so `export` refuses the project
//! until the user fills them in (S3, #64, #98): a drawing cannot say what
//! the material or the machine is. The step-down is the tool's: the
//! operation's, which would override it, stays `null` too.

use std::collections::BTreeMap;

use dc_geometry::{FLATTEN_TOLERANCE_DEFAULT_MM, MAX_COORDINATE_MM, Rect, Transform};
use dc_project::{
    CURRENT_VERSION, CornerRelief, CutDirection, Entry, Heights, MachineOrigin, MachiningOrder,
    Operation, OperationId, OriginXy, OriginZ, PostProcessorRef, ProfileOp, Project, Side, StartAt,
    Stock, TabPlacement, TabShape, TabSpec, Tool, ToolId, ToolKind, Units,
};

use crate::error::JobError;
use crate::import::Drawing;

/// Margin between the drawing's bounds and each stock edge, mm.
pub const STOCK_MARGIN_MM: f64 = 10.0;
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
/// The one operation the defaults write, a Profile, and its settings.
const OPERATION_ID: &str = "op1";
/// Arc-fitting tolerance a new project starts with, mm (functional spec,
/// capability 24). The app's *New* starts from it too.
pub const ARC_FIT_TOLERANCE_MM: f64 = 0.01;
const SAFE_Z_MM: f64 = 5.0;
const TAB_COUNT: u8 = 4;
const TAB_WIDTH_MM: f64 = 8.0;
const TAB_HEIGHT_MM: f64 = 3.0;
/// The ramp angle a new project's Profile enters each pass at, degrees
/// (functional spec, capability 16; #119).
const INIT_RAMP_ANGLE_DEG: f64 = dc_cam::entry::INIT_RAMP_ANGLE_DEG;

/// `value_mm` rounded up to the next multiple of 0.01 mm, ignoring up to
/// `ROUND_SLACK_MM` of noise above a multiple.
fn round_up_mm(value_mm: f64) -> f64 {
    ((value_mm - ROUND_SLACK_MM) * ROUND_STEPS_PER_MM).ceil() / ROUND_STEPS_PER_MM
}

/// A stock sized round some shapes ([`fit_stock`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StockFit {
    /// The translation that moves the shapes so their bounds sit the
    /// margin in from the stock's bottom-left corner.
    pub shift: Transform,
    /// The stock width, mm.
    pub width_mm: f64,
    /// The stock height, mm.
    pub height_mm: f64,
}

/// The stock `init` sizes round shapes with `bounds` (mm, project space)
/// at `margin_mm` from every edge, and the shift that places them: the
/// rule [`drawing`] applies with [`STOCK_MARGIN_MM`], for the app's *Fit
/// stock to shapes* with the margin the user types (functional spec,
/// capability 61). Offsets and sizes are rounded up to 0.01 mm, so the
/// shapes stay at least the margin (less `ROUND_SLACK_MM`) from every edge
/// and the file reads cleanly.
#[must_use]
pub fn fit_stock(bounds: Rect, margin_mm: f64) -> StockFit {
    let dx_mm = round_up_mm(margin_mm - bounds.min.x);
    let dy_mm = round_up_mm(margin_mm - bounds.min.y);
    StockFit {
        shift: Transform::translate(dx_mm, dy_mm),
        width_mm: round_up_mm(bounds.max.x + dx_mm + margin_mm),
        height_mm: round_up_mm(bounds.max.y + dy_mm + margin_mm),
    }
}

/// The project `dreamcarve init` writes for `import`, named `name` (the
/// drawing's file stem), with post-processor `post_processor` and
/// `display_units`. Instances are moved so the drawing's bounds sit at
/// least [`STOCK_MARGIN_MM`] in from the stock's bottom-left corner;
/// definitions keep the imported coordinates. The imported bounds carry
/// float noise from flattening, so the offset and the stock size are
/// rounded up to 0.01 mm: the drawing stays at least [`STOCK_MARGIN_MM`]
/// (less under `ROUND_SLACK_MM`) from every edge, and the file reads
/// cleanly. With `with_defaults` the project also has the placeholder tool
/// and the Profile of every closed shape; without, it has no tool and no
/// operation (functional spec, acceptance 63).
///
/// # Errors
///
/// [`JobError::NoShapes`] for a drawing with nothing to import, and
/// [`JobError::StockTooLarge`] when the drawing imported within
/// [`MAX_COORDINATE_MM`] but the margin pushes the stock past it: better
/// refused than written as a project `check` rejects.
pub fn drawing(
    name: &str,
    import: Drawing,
    post_processor: PostProcessorRef,
    display_units: Units,
    with_defaults: bool,
) -> Result<Project, JobError> {
    let bounds = import.bounds().ok_or(JobError::NoShapes)?;
    let (definitions, instances) = import.into_shapes();
    let project = default_project(
        name,
        definitions,
        instances,
        bounds,
        post_processor,
        display_units,
        with_defaults,
    );
    let stock = &project.stock;
    if stock.width_mm > MAX_COORDINATE_MM || stock.height_mm > MAX_COORDINATE_MM {
        return Err(JobError::StockTooLarge {
            width_mm: stock.width_mm,
            height_mm: stock.height_mm,
            margin_mm: STOCK_MARGIN_MM,
            limit_mm: MAX_COORDINATE_MM,
        });
    }
    Ok(project)
}

/// Builds the project with the `init` defaults ([`drawing`]).
fn default_project(
    name: &str,
    definitions: Vec<dc_project::FeatureDefinition>,
    mut instances: Vec<dc_project::FeatureInstance>,
    bounds: Rect,
    post_processor: PostProcessorRef,
    display_units: Units,
    with_defaults: bool,
) -> Project {
    let fit = fit_stock(bounds, STOCK_MARGIN_MM);
    for instance in &mut instances {
        instance.transform = instance.transform.then(fit.shift);
    }
    let definitions: BTreeMap<_, _> = definitions.into_iter().map(|d| (d.id.clone(), d)).collect();
    let (tools, operations) = if with_defaults {
        // Profile cuts closed shapes only; open paths stay in the project
        // for the user to see, untargeted (functional spec, capability 8).
        let targets = instances
            .iter()
            .filter(|i| {
                definitions
                    .get(&i.definition)
                    .is_some_and(|d| d.profiles.iter().any(|p| p.is_closed()))
            })
            .map(|i| i.id.clone())
            .collect();
        let tool = placeholder_tool();
        let profile = default_profile(targets, tool.id.clone());
        (vec![tool], vec![Operation::Profile(profile)])
    } else {
        (Vec::new(), Vec::new())
    };
    Project {
        version: CURRENT_VERSION,
        name: name.to_owned(),
        display_units,
        flatten_tolerance_mm: FLATTEN_TOLERANCE_DEFAULT_MM,
        arc_fit_tolerance_mm: Some(ARC_FIT_TOLERANCE_MM),
        stock: Stock {
            width_mm: fit.width_mm,
            height_mm: fit.height_mm,
            thickness_mm: None,
        },
        origin: MachineOrigin {
            xy: OriginXy::BottomLeft,
            z: OriginZ::StockTop,
        },
        // #283: the clearance, tool-change and park heights are the
        // user's to set, like the thickness; unset, the program is as
        // before they existed.
        heights: Heights::unset(),
        definitions,
        instances,
        tools,
        operations,
        tabs: Vec::new(),
        post_processor,
    }
}

/// The placeholder tool `t1`, "Set me": a 1/4" two-flute end mill with no
/// feeds, speed or step-down.
fn placeholder_tool() -> Tool {
    Tool {
        id: ToolId::new(TOOL_ID),
        name: TOOL_NAME.into(),
        vendor: None,
        product_id: None,
        number: None,
        cutter: ToolKind::EndMill,
        diameter_mm: TOOL_DIAMETER_MM,
        flutes: TOOL_FLUTES,
        feed_mm_min: None,
        plunge_mm_min: None,
        rpm: None,
        step_down_mm: None,
        step_over_fraction: None,
    }
}

/// The Profile `op1` of `targets` with `tool`: outside, climb, no depth, a
/// tab request of four 8 × 3 mm rect tabs off the corners and not on
/// holes, a 15° ramp into each pass, safe Z 5 mm. The app's *Add ▸
/// Profile* starts from it too (functional spec, capability 65), so a new
/// Profile is `init`'s whichever shell made it.
#[must_use]
pub fn default_profile(targets: Vec<dc_project::InstanceId>, tool: ToolId) -> ProfileOp {
    ProfileOp {
        id: OperationId::new(OPERATION_ID),
        name: "Profile".into(),
        enabled: true,
        targets,
        tool,
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
            // #109: a hole's slug is waste.
            on_holes: false,
            // #285: tabs are weakest, and hardest to clean up, on a corner.
            avoid_corners: true,
        }),
        machining_order: MachiningOrder::FullDepthFirst,
        entry: Entry::Ramp {
            angle_deg: INIT_RAMP_ANGLE_DEG,
        },
        lead: None,
        corner_relief: CornerRelief::None,
        last_pass: None,
        start: StartAt::Auto,
    }
}
