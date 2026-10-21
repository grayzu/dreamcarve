// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! `dreamcarve tabs`: a project's placed tabs (#105, ADR-0005;
//! `docs/specs/phase-2/design.md`, "Placed tabs").
//!
//! - `tabs place <dcv> <op> [--count N | --at d1,d2…]` places tabs on the
//!   outlines a Profile cuts with `dc_cam::placed_tabs`: `--count` with
//!   the phase 2 layout rules, `--at` at distances along each outline;
//!   with neither, the operation's own tab request.
//! - `tabs list <dcv>` prints every tab and the enabled Profiles that cut
//!   round it.
//! - `tabs clear <dcv> [<op> | --tab <id>…]` removes tabs.
//!
//! A command that changes the project first places the tab request of
//! every enabled Profile, so the file it saves holds placed tabs only, and
//! saves through `dc_project::save`, staged and renamed into place.
//! `list` places them in memory only, and says so.

use std::path::Path;
use std::process::ExitCode;

use dc_cam::Warning;
use dc_cam::placed_tabs::{
    TabSize, along_outline_mm, crossed_by, freed_parts, off_outline_mm, on_holes, place,
    place_on_outlines, place_requests,
};
use dc_project::{
    Operation, OperationId, PlacedTab, Project, TabId, TabPlacement, TabShape, TabSpec,
};

use crate::cli::{Failure, StagedFile, TabsCommand, TabsPlaceArgs};
use crate::export::load_project;
use crate::output::{errln, outln};

/// Tab width when `--width` is not given, mm: `init`'s.
const DEFAULT_WIDTH_MM: f64 = 8.0;
/// Tab top when `--height` is not given, mm above the stock bottom:
/// `init`'s.
const DEFAULT_HEIGHT_MM: f64 = 3.0;

/// Runs `tabs <command>`.
pub fn run(command: &TabsCommand) -> Result<ExitCode, Failure> {
    match command {
        TabsCommand::Place(args) => place_command(args),
        TabsCommand::List { dcv } => list(dcv),
        TabsCommand::Clear { dcv, op, tabs } => clear(dcv, op.as_deref(), tabs),
    }?;
    Ok(ExitCode::SUCCESS)
}

/// The project at `dcv` with every enabled Profile's tab request placed,
/// printing what placing found, with how many requests it placed and the
/// ids of the tabs the file already had.
fn load_placed(dcv: &Path) -> Result<(Project, usize, Vec<TabId>), Failure> {
    let (project, _) = load_project(dcv)?;
    let requests = requests_of(&project);
    let had = project.tabs.iter().map(|t| t.id.clone()).collect();
    let (placed, warnings) = place_requests(&project).map_err(Failure::error)?;
    print_warnings(&warnings);
    Ok((placed, requests, had))
}

/// How many enabled Profiles hold a tab request.
fn requests_of(project: &Project) -> usize {
    project
        .operations
        .iter()
        .filter(|op| matches!(op, Operation::Profile(p) if p.enabled && p.tabs.is_some()))
        .count()
}

/// Prints `warnings` as `warning: <code>: <message>` on stderr.
fn print_warnings(warnings: &[Warning]) {
    for w in warnings {
        let message = match w.op() {
            Some(op) => format!("operation `{op}`: {w}"),
            None => w.to_string(),
        };
        errln!("warning: {}: {message}", w.code().as_str());
    }
}

/// Saves `project` to `dcv`.
fn save(dcv: &Path, project: &Project) -> Result<(), Failure> {
    let text = dc_project::save(project).map_err(Failure::error)?;
    StagedFile::write(dcv, text.as_bytes())?.commit()
}

/// `tabs place`.
fn place_command(args: &TabsPlaceArgs) -> Result<(), Failure> {
    let (raw, _) = load_project(&args.dcv)?;
    let op = OperationId::new(args.op.as_str());
    let request = match raw.operations.iter().find(|o| o.id() == &op) {
        Some(Operation::Profile(p)) => p.tabs.clone(),
        Some(other) => {
            return Err(Failure::error(format!(
                "operation `{op}` is a {} operation; only a Profile cuts tabs",
                other.kind()
            )));
        }
        None => {
            return Err(Failure::error(format!(
                "operation `{op}` is not in {}",
                args.dcv.display()
            )));
        }
    };
    let (mut project, warnings) = place_requests(&raw).map_err(Failure::error)?;
    print_warnings(&warnings);
    // Size, shape and holes belong to the tabs --count or --at places; a
    // request brings its own.
    if args.count.is_none()
        && args.at.is_none()
        && (args.width.is_some() || args.height.is_some() || args.shape.is_some() || args.on_holes)
    {
        return Err(Failure::usage(
            "--width, --height, --shape and --on-holes need --count or --at",
        ));
    }
    let width_mm = args.width.unwrap_or(DEFAULT_WIDTH_MM);
    let height_mm = args.height.unwrap_or(DEFAULT_HEIGHT_MM);
    let shape = args.shape.map_or(TabShape::Rect, Into::into);
    let placement = match (&args.at, args.count) {
        (Some(at_mm), _) => Some(place_on_outlines(
            &project,
            &op,
            at_mm,
            TabSize {
                width_mm,
                height_mm,
                shape,
            },
            args.on_holes,
        )),
        (None, Some(count)) => Some(place(
            &project,
            &op,
            &TabSpec {
                placement: TabPlacement::Count { count },
                width_mm,
                height_mm,
                shape,
                on_holes: args.on_holes,
            },
        )),
        (None, None) => {
            let Some(spec) = request else {
                return Err(Failure::usage(format!(
                    "operation `{op}` has no tab request to place; give --count or --at"
                )));
            };
            // An enabled Profile's request is placed above; a disabled
            // one's only when asked for by name.
            let mut placed = None;
            if let Some(Operation::Profile(p)) =
                project.operations.iter_mut().find(|o| o.id() == &op)
                && p.tabs.take().is_some()
            {
                placed = Some(spec);
            }
            placed.map(|spec| place(&project, &op, &spec))
        }
    };
    if let Some(placement) = placement {
        let placement = placement.map_err(Failure::error)?;
        print_warnings(&placement.warnings);
        project.tabs.extend(placement.tabs);
    }
    let new: Vec<&PlacedTab> = project
        .tabs
        .iter()
        .filter(|t| !raw.tabs.iter().any(|r| r.id == t.id))
        .collect();
    for tab in &new {
        outln!("Placed {}", describe(&project, tab))?;
    }
    warn_on_holes(&project, &new);
    save(&args.dcv, &project)?;
    outln!(
        "{} tab(s) placed; {} saved with {} tab(s).",
        new.len(),
        args.dcv.display(),
        project.tabs.len()
    )?;
    Ok(())
}

/// Warns `tab_on_hole` once for each of `new` that an enabled Profile of
/// `project` cuts as a hole (#300). Generation does not warn again for a
/// tab placed on the hole for that cut, so this is where the user hears
/// that it holds the slug.
fn warn_on_holes(project: &Project, new: &[&PlacedTab]) {
    let mut warnings = Vec::new();
    for op in &project.operations {
        let Operation::Profile(profile) = op else {
            continue;
        };
        if !profile.enabled {
            continue;
        }
        for (tab, shape) in on_holes(project, profile).unwrap_or_default() {
            if new.iter().any(|t| t.id == tab) {
                warnings.push(Warning::TabOnHole {
                    op: profile.id.clone(),
                    tab,
                    shape,
                });
            }
        }
    }
    print_warnings(&warnings);
}

/// One tab as `place` and `list` show it.
fn describe(project: &Project, tab: &PlacedTab) -> String {
    let name = project
        .instances
        .iter()
        .find(|i| i.id == tab.instance)
        .map_or("?", |i| i.name.as_str());
    format!(
        "`{}` on `{name}` (instance `{}`, profile {}) {} mm along it: {} mm wide, top {} mm above the stock bottom, {}",
        tab.id,
        tab.instance,
        tab.profile,
        along_outline_mm(project, tab).map_or_else(|| "?".to_owned(), |(at, _)| mm(at)),
        mm(tab.width_mm),
        mm(tab.height_mm),
        shape_name(tab.shape)
    )
}

/// `value_mm` to 0.001 mm, trailing zeros dropped.
fn mm(value_mm: f64) -> String {
    let text = format!("{value_mm:.3}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" { "0" } else { text }.to_owned()
}

/// The shape as the file names it.
const fn shape_name(shape: TabShape) -> &'static str {
    match shape {
        TabShape::Rect => "rect",
        TabShape::Smooth => "smooth",
    }
}

/// `tabs list`: a table of every tab, with the enabled Profiles that cut
/// round it.
fn list(dcv: &Path) -> Result<(), Failure> {
    let (project, requests, _) = load_placed(dcv)?;
    if project.tabs.is_empty() {
        outln!("{} has no tabs.", dcv.display())?;
        return Ok(());
    }
    let crossings: Vec<(&OperationId, Vec<dc_project::TabId>)> = project
        .operations
        .iter()
        .filter_map(|op| match op {
            Operation::Profile(p) if p.enabled => {
                Some((&p.id, crossed_by(&project, p).unwrap_or_default()))
            }
            _ => None,
        })
        .collect();
    let header = [
        "ID",
        "INSTANCE",
        "PROFILE",
        "AT_MM",
        "OFF_MM",
        "X_MM",
        "Y_MM",
        "WIDTH_MM",
        "HEIGHT_MM",
        "SHAPE",
        "CUT_BY",
    ]
    .map(str::to_owned);
    let rows: Vec<[String; 11]> = project
        .tabs
        .iter()
        .map(|tab| {
            let cut_by: Vec<&str> = crossings
                .iter()
                .filter(|(_, ids)| ids.contains(&tab.id))
                .map(|(op, _)| op.as_str())
                .collect();
            [
                tab.id.to_string(),
                tab.instance.to_string(),
                tab.profile.to_string(),
                along_outline_mm(&project, tab).map_or_else(|| "-".to_owned(), |(at, _)| mm(at)),
                off_outline_mm(&project, tab).map_or_else(|| "-".to_owned(), mm),
                mm(tab.x_mm),
                mm(tab.y_mm),
                mm(tab.width_mm),
                mm(tab.height_mm),
                shape_name(tab.shape).to_owned(),
                if cut_by.is_empty() {
                    "-".to_owned()
                } else {
                    cut_by.join(",")
                },
            ]
        })
        .collect();
    let mut widths = header.clone().map(|h| h.chars().count());
    for row in &rows {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    for row in std::iter::once(&header).chain(&rows) {
        let line: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, &width)| format!("{cell:<width$}"))
            .collect();
        outln!("{}", line.join("  ").trim_end())?;
    }
    if requests > 0 {
        outln!(
            "{requests} operation(s) hold a tab request, listed as the tabs it places; \
             any `tabs` command that saves the project writes them."
        )?;
    }
    Ok(())
}

/// `tabs clear`: every tab, those the Profile `op` cuts round, or those
/// named in `ids`. Says which parts an enabled Profile then cuts through
/// the stock with no tab that one held before.
fn clear(dcv: &Path, op: Option<&str>, ids: &[String]) -> Result<(), Failure> {
    let (mut project, _, had) = load_placed(dcv)?;
    let before = project.tabs.len();
    let freed_before = freed_parts(&project);
    if let Some(op) = op {
        let op = OperationId::new(op);
        let profile = match project.operations.iter().find(|o| o.id() == &op) {
            Some(Operation::Profile(p)) => p.clone(),
            Some(other) => {
                return Err(Failure::error(format!(
                    "operation `{op}` is a {} operation; only a Profile cuts tabs",
                    other.kind()
                )));
            }
            None => {
                return Err(Failure::error(format!(
                    "operation `{op}` is not in {}",
                    dcv.display()
                )));
            }
        };
        let crossed = crossed_by(&project, &profile).map_err(Failure::error)?;
        project.tabs.retain(|t| !crossed.contains(&t.id));
    } else if !ids.is_empty() {
        if let Some(missing) = ids
            .iter()
            .find(|id| !project.tabs.iter().any(|t| t.id.as_str() == id.as_str()))
        {
            return Err(Failure::error(format!(
                "tab `{missing}` is not in {}",
                dcv.display()
            )));
        }
        project
            .tabs
            .retain(|t| !ids.iter().any(|id| id.as_str() == t.id.as_str()));
    } else {
        project.tabs.clear();
    }
    let removed = before - project.tabs.len();
    let new: Vec<&PlacedTab> = project
        .tabs
        .iter()
        .filter(|t| !had.contains(&t.id))
        .collect();
    warn_on_holes(&project, &new);
    save(dcv, &project)?;
    outln!(
        "Removed {removed} tab(s); {} saved with {} tab(s).",
        dcv.display(),
        project.tabs.len()
    )?;
    for (op, shape) in freed_parts(&project) {
        if !freed_before.contains(&(op.clone(), shape.clone())) {
            outln!(
                "Note: operation `{op}` now cuts {shape} through the stock with no tab, so the part comes loose on its last pass."
            )?;
        }
    }
    Ok(())
}
