// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Every change to the project (`docs/specs/phase-4/design.md`, "Data model
//! → dc-app"; functional spec, capability 52). A [`Command`] is applied to
//! the current snapshot and returns the next one, never touching the
//! caller's; [`App::run`](crate::App::run) pushes the result to the history
//! under the command's [`label`](Command::label), the text the Edit menu
//! shows. A command that cannot be applied returns a [`CommandError`] and
//! changes nothing.
//!
//! Commands that need geometry the Viewport computes (where a drag ends,
//! which outline a tab snaps to) take the finished value: a `Transform`, a
//! point in definition space. `TransformInstances` applies its delta about
//! the selection's joint bounding-box centre, as capability 58 says.

mod instances;
mod operations;

pub(crate) use instances::{
    about, bounds as selection_bounds, centre, copy_name, find as find_instance, next_free_id,
};

use dc_geometry::{Transform, approx_eq};
use dc_job::Drawing;
use dc_project::{
    Heights, InstanceId, MachineOrigin, Operation, OperationId, PlacedTab, PostProcessorRef,
    Project, Stock, TabId, Tool, ToolId, Units, switch_post,
};

use crate::arrange::Flip;
use crate::error::CommandError;
use crate::library::trimmed;
use crate::strings::{StrKey, Strings};

/// One whole turn, degrees.
const FULL_TURN_DEG: f64 = 360.0;
/// A turn within this many degrees of a whole one turns nothing.
const NO_TURN_DEG: f64 = 1e-9;
/// Decimals an undo label gives a turn's angle.
const ANGLE_DECIMALS: usize = 2;

/// Every change to the project. `apply` is pure; `label` is the undo text.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Replace the stock (size and thickness).
    SetStock(Stock),
    /// Replace the machine origin.
    SetOrigin(MachineOrigin),
    /// Replace the clearance, tool-change and park heights.
    SetHeights(Heights),
    /// Set the flattening tolerance and the arc-fitting tolerance (`None`
    /// writes no arcs).
    SetTolerances {
        /// Flattening tolerance, mm.
        flatten_mm: f64,
        /// Arc-fitting tolerance, mm, or `None` for lines only.
        arc_fit_mm: Option<f64>,
    },
    /// Change the display units; the stored values stay millimetres.
    SetDisplayUnits(Units),
    /// Switch to another bundled post-processor at its latest version,
    /// keeping the settings it can use ([`dc_project::switch_post`]).
    SwitchPost {
        /// The bundled definition's id.
        id: String,
    },
    /// Replace the post-processor settings (file extension, spindle
    /// control, tool-change mode, decimals, canned cycles).
    SetPostSettings(PostProcessorRef),
    /// Add tools under ids the project does not have.
    AddTools(Vec<Tool>),
    /// *New tool* (capability 63): add a tool made from scratch, under an
    /// id the project does not have; labelled *New tool “tool4”*.
    NewTool(Tool),
    /// Replace the tool with the same id.
    UpdateTool(Tool),
    /// Delete a tool no operation uses.
    DeleteTool(ToolId),
    /// Append a drawing's shapes, placed as drawn (capability 55). `name`
    /// is the file's, for the label.
    ImportDrawing {
        /// The imported drawing.
        drawing: Drawing,
        /// The drawing's file name.
        name: String,
    },
    /// Apply `delta` to each instance about the selection's joint
    /// bounding-box centre. Refused when any instance is locked.
    TransformInstances {
        /// The instances, in selection order.
        ids: Vec<InstanceId>,
        /// The change, about the selection's centre.
        delta: Transform,
    },
    /// Turn instances about their joint bounding-box centre (the Arrange
    /// group's Rotate, capability 58a). Refused when any is locked.
    RotateInstances {
        /// The instances, in selection order.
        ids: Vec<InstanceId>,
        /// The turn, degrees, positive anticlockwise (Y-up).
        angle_deg: f64,
    },
    /// Mirror instances about their joint bounding-box centre (the
    /// Arrange group's flips, capability 58a). Refused when any is locked.
    FlipInstances {
        /// The instances, in selection order.
        ids: Vec<InstanceId>,
        /// Which way.
        flip: Flip,
    },
    /// Set the `visible` and `locked` flags given.
    SetInstanceFlags {
        /// The instances.
        ids: Vec<InstanceId>,
        /// The new `visible`, when it changes.
        visible: Option<bool>,
        /// The new `locked`, when it changes.
        locked: Option<bool>,
    },
    /// Rename an instance.
    RenameInstance {
        /// The instance.
        id: InstanceId,
        /// Its new name.
        name: String,
    },
    /// Linked copies, offset by 10 mm and named `<name> (2)` (capability
    /// 59).
    DuplicateInstances(Vec<InstanceId>),
    /// Remove instances, their placed tabs, their entries in every
    /// operation's targets, and any definition no instance uses after.
    DeleteInstances(Vec<InstanceId>),
    /// Append an operation under an id the project does not have.
    AddOperation(Operation),
    /// Replace the operation with the same id.
    UpdateOperation(Operation),
    /// Move an operation to position `to` in the list.
    ReorderOperation {
        /// The operation.
        id: OperationId,
        /// Its new position, from 0.
        to: usize,
    },
    /// Enable or disable an operation.
    SetOperationEnabled {
        /// The operation.
        id: OperationId,
        /// Whether it runs.
        enabled: bool,
    },
    /// A copy of an operation under a fresh id, after the original.
    DuplicateOperation(OperationId),
    /// Remove an operation; its targets' placed tabs stay.
    DeleteOperation(OperationId),
    /// Place a Profile's tab request as tabs ([`dc_job::tabs::place_request`]).
    PlaceTabs {
        /// The Profile.
        op: OperationId,
    },
    /// Add a placed tab under an id the project does not have.
    AddTab(PlacedTab),
    /// Move a placed tab to a point of its outline, definition space, mm.
    MoveTab {
        /// The tab.
        id: TabId,
        /// Its new X, definition space, mm.
        x_mm: f64,
        /// Its new Y, definition space, mm.
        y_mm: f64,
    },
    /// Replace the tab with the same id.
    UpdateTab(PlacedTab),
    /// Replace each tab with the same id, one step (the Properties panel's
    /// width, height or shape over several selected tabs).
    UpdateTabs(Vec<PlacedTab>),
    /// Remove placed tabs.
    DeleteTabs(Vec<TabId>),
    /// Move instances so their joint bounding box is centred on the stock
    /// (capability 61). Refused when any is locked.
    CentreOnStock(Vec<InstanceId>),
    /// Size the stock to the visible shapes plus a margin, keeping the
    /// origin preset; the shapes keep their relative positions
    /// (capability 61).
    FitStockToShapes {
        /// The margin between the shapes and each edge, mm.
        margin_mm: f64,
    },
}

impl Command {
    /// The project after this command, `project` untouched.
    ///
    /// # Errors
    ///
    /// [`CommandError`] when the command cannot be applied: an id the
    /// project does not have, a locked instance, a value that cannot be
    /// stored, a tool in use, a `dc-job` step that refused.
    pub fn apply(&self, project: &Project) -> Result<Project, CommandError> {
        let mut next = project.clone();
        match self {
            Self::SetStock(stock) => {
                finite("stock.width_mm", stock.width_mm)?;
                finite("stock.height_mm", stock.height_mm)?;
                finite_opt("stock.thickness_mm", stock.thickness_mm)?;
                next.stock = stock.clone();
            }
            Self::SetOrigin(origin) => {
                if let dc_project::OriginXy::Custom { x_mm, y_mm } = origin.xy {
                    finite("origin.xy.x_mm", x_mm)?;
                    finite("origin.xy.y_mm", y_mm)?;
                }
                next.origin = origin.clone();
            }
            Self::SetHeights(heights) => {
                finite_opt("heights.clearance_mm", heights.clearance_mm)?;
                finite_opt(
                    "heights.tool_change_machine_z_mm",
                    heights.tool_change_machine_z_mm,
                )?;
                if let Some(park) = heights.park {
                    finite("heights.park.x_mm", park.x_mm)?;
                    finite("heights.park.y_mm", park.y_mm)?;
                }
                next.heights = heights.clone();
            }
            Self::SetTolerances {
                flatten_mm,
                arc_fit_mm,
            } => {
                finite("flatten_tolerance_mm", *flatten_mm)?;
                finite_opt("arc_fit_tolerance_mm", *arc_fit_mm)?;
                next.flatten_tolerance_mm = *flatten_mm;
                next.arc_fit_tolerance_mm = *arc_fit_mm;
            }
            Self::SetDisplayUnits(units) => next.display_units = *units,
            Self::SwitchPost { id } => {
                let (post, _resets) = switch_post(&project.post_processor, id)?;
                next.post_processor = post;
            }
            Self::SetPostSettings(post) => {
                finite("post_processor.spindle_on_dwell_s", post.spindle_on_dwell_s)?;
                next.post_processor = post.clone();
            }
            Self::AddTools(tools) => {
                if tools.is_empty() {
                    return Err(CommandError::NothingToDo);
                }
                for tool in tools {
                    add_tool(&mut next, tool)?;
                }
            }
            Self::NewTool(tool) => add_tool(&mut next, tool)?,
            Self::UpdateTool(tool) => {
                check_tool(tool)?;
                let slot = next
                    .tools
                    .iter_mut()
                    .find(|t| t.id == tool.id)
                    .ok_or_else(|| CommandError::UnknownTool {
                        id: tool.id.clone(),
                    })?;
                *slot = tool.clone();
            }
            Self::DeleteTool(id) => operations::delete_tool(&mut next, id)?,
            Self::ImportDrawing { drawing, .. } => {
                next = dc_job::drawing_into(project, drawing.clone()).0;
            }
            Self::TransformInstances { ids, delta } => {
                next = instances::transform(project, ids, *delta)?;
            }
            Self::RotateInstances { ids, angle_deg } => {
                finite("rotation", *angle_deg)?;
                if approx_eq(angle_deg.rem_euclid(FULL_TURN_DEG), 0.0, NO_TURN_DEG)
                    || approx_eq(
                        angle_deg.rem_euclid(FULL_TURN_DEG),
                        FULL_TURN_DEG,
                        NO_TURN_DEG,
                    )
                {
                    return Err(CommandError::NothingToDo);
                }
                let delta = Transform::rotate(angle_deg.to_radians());
                next = instances::transform(project, ids, delta)?;
            }
            Self::FlipInstances { ids, flip } => {
                let delta = match flip {
                    Flip::Horizontal => Transform::scale(-1.0, 1.0),
                    Flip::Vertical => Transform::scale(1.0, -1.0),
                };
                next = instances::transform(project, ids, delta)?;
            }
            Self::SetInstanceFlags {
                ids,
                visible,
                locked,
            } => {
                if ids.is_empty() {
                    return Err(CommandError::NothingSelected);
                }
                if visible.is_none() && locked.is_none() {
                    return Err(CommandError::NothingToDo);
                }
                for id in ids {
                    let instance = instances::find_mut(&mut next, id)?;
                    if let Some(visible) = visible {
                        instance.visible = *visible;
                    }
                    if let Some(locked) = locked {
                        instance.locked = *locked;
                    }
                }
            }
            Self::RenameInstance { id, name } => {
                instances::find_mut(&mut next, id)?.name.clone_from(name);
            }
            Self::DuplicateInstances(ids) => next = instances::duplicate(project, ids)?,
            Self::DeleteInstances(ids) => next = instances::delete(project, ids)?,
            Self::AddOperation(operation) => {
                if next.operations.iter().any(|o| o.id() == operation.id()) {
                    return Err(CommandError::DuplicateId {
                        kind: "an operation",
                        id: operation.id().as_str().to_owned(),
                    });
                }
                next.operations.push(operation.clone());
            }
            Self::UpdateOperation(operation) => {
                let at = operations::index_of(project, operation.id())?;
                if let Some(slot) = next.operations.get_mut(at) {
                    *slot = operation.clone();
                }
            }
            Self::ReorderOperation { id, to } => operations::reorder(&mut next, id, *to)?,
            Self::SetOperationEnabled { id, enabled } => {
                let at = operations::index_of(project, id)?;
                if let Some(operation) = next.operations.get_mut(at) {
                    *operations::enabled_mut(operation) = *enabled;
                }
            }
            Self::DuplicateOperation(id) => next = operations::duplicate(project, id)?,
            Self::DeleteOperation(id) => {
                let at = operations::index_of(project, id)?;
                next.operations.remove(at);
            }
            Self::PlaceTabs { op } => next = dc_job::tabs::place_request(project, op)?.0,
            Self::AddTab(tab) => {
                if next.tabs.iter().any(|t| t.id == tab.id) {
                    return Err(CommandError::DuplicateId {
                        kind: "a tab",
                        id: tab.id.as_str().to_owned(),
                    });
                }
                instances::find(project, &tab.instance)?;
                check_tab(tab)?;
                next.tabs.push(tab.clone());
            }
            Self::MoveTab { id, x_mm, y_mm } => {
                finite("tabs[].x_mm", *x_mm)?;
                finite("tabs[].y_mm", *y_mm)?;
                let tab = find_tab_mut(&mut next, id)?;
                tab.x_mm = *x_mm;
                tab.y_mm = *y_mm;
            }
            Self::UpdateTab(tab) => {
                check_tab(tab)?;
                instances::find(project, &tab.instance)?;
                *find_tab_mut(&mut next, &tab.id)? = tab.clone();
            }
            Self::UpdateTabs(tabs) => {
                if tabs.is_empty() {
                    return Err(CommandError::NothingSelected);
                }
                for tab in tabs {
                    check_tab(tab)?;
                    instances::find(project, &tab.instance)?;
                    *find_tab_mut(&mut next, &tab.id)? = tab.clone();
                }
            }
            Self::DeleteTabs(ids) => {
                if ids.is_empty() {
                    return Err(CommandError::NothingSelected);
                }
                for id in ids {
                    find_tab_mut(&mut next, id)?;
                }
                next.tabs.retain(|t| !ids.contains(&t.id));
            }
            Self::CentreOnStock(ids) => next = instances::centre_on_stock(project, ids)?,
            Self::FitStockToShapes { margin_mm } => {
                next = instances::fit_stock(project, *margin_mm)?;
            }
        }
        Ok(next)
    }

    /// The undo step's name, from `strings` (functional spec, capability
    /// 52): *Move “Oval”*, *Set depth of “Outside cut”*, *Reorder
    /// operations*, *Import plaque.dxf*, *Place 4 tabs*. `before` is the
    /// snapshot the command was applied to and `after` its result: a
    /// deletion names what `before` had, a placement counts what `after`
    /// gained.
    #[must_use]
    pub fn label(&self, strings: &Strings, before: &Project, after: &Project) -> String {
        let subject_of = |ids: &[InstanceId]| instances::subject(strings, before, ids);
        let op_subject = |id: &OperationId| operations::subject(strings, before, id);
        let plain = |key: StrKey| strings.get(key).to_owned();
        let with_subject = |key: StrKey, subject: &str| strings.fmt(key, &[("subject", subject)]);
        match self {
            Self::SetStock(_) => plain(StrKey::CommandLabelSetStock),
            Self::SetOrigin(_) => plain(StrKey::CommandLabelSetOrigin),
            Self::SetHeights(_) => plain(StrKey::CommandLabelSetHeights),
            Self::SetTolerances { .. } => plain(StrKey::CommandLabelSetTolerances),
            Self::SetDisplayUnits(_) => plain(StrKey::CommandLabelSetDisplayUnits),
            Self::SwitchPost { id } => strings.fmt(StrKey::CommandLabelSwitchPost, &[("name", id)]),
            Self::SetPostSettings(_) => plain(StrKey::CommandLabelSetPostSettings),
            Self::AddTools(tools) => match tools.as_slice() {
                [tool] => with_subject(StrKey::CommandLabelAddTool, &one(strings, &tool.name)),
                _ => strings.fmt(
                    StrKey::CommandLabelAddTools,
                    &[("count", &tools.len().to_string())],
                ),
            },
            Self::NewTool(tool) => {
                with_subject(StrKey::CommandLabelNewTool, &one(strings, &tool.name))
            }
            Self::UpdateTool(tool) => {
                with_subject(StrKey::CommandLabelUpdateTool, &one(strings, &tool.name))
            }
            Self::DeleteTool(id) => {
                let name = before
                    .tools
                    .iter()
                    .find(|t| t.id == *id)
                    .map_or_else(|| id.as_str().to_owned(), |t| t.name.clone());
                with_subject(StrKey::CommandLabelDeleteTool, &one(strings, &name))
            }
            Self::ImportDrawing { name, .. } => {
                strings.fmt(StrKey::CommandLabelImport, &[("name", name)])
            }
            Self::TransformInstances { ids, delta } => {
                let key = match instances::classify(*delta) {
                    instances::TransformKind::Move => StrKey::CommandLabelMove,
                    instances::TransformKind::Rotate => StrKey::CommandLabelRotate,
                    instances::TransformKind::Scale => StrKey::CommandLabelScale,
                    instances::TransformKind::Mirror => StrKey::CommandLabelMirror,
                    instances::TransformKind::Other => StrKey::CommandLabelTransform,
                };
                with_subject(key, &subject_of(ids))
            }
            Self::RotateInstances { ids, angle_deg } => {
                let key = if *angle_deg < 0.0 {
                    StrKey::CommandLabelRotateClockwise
                } else {
                    StrKey::CommandLabelRotateAnticlockwise
                };
                strings.fmt(
                    key,
                    &[
                        ("subject", &subject_of(ids)),
                        ("angle", &trimmed(angle_deg.abs(), ANGLE_DECIMALS)),
                    ],
                )
            }
            Self::FlipInstances { ids, flip } => {
                let key = match flip {
                    Flip::Horizontal => StrKey::CommandLabelFlipHorizontal,
                    Flip::Vertical => StrKey::CommandLabelFlipVertical,
                };
                with_subject(key, &subject_of(ids))
            }
            Self::SetInstanceFlags {
                ids,
                visible,
                locked,
            } => {
                let key = match (visible, locked) {
                    (Some(true), None) => StrKey::CommandLabelShow,
                    (Some(false), None) => StrKey::CommandLabelHide,
                    (None, Some(true)) => StrKey::CommandLabelLock,
                    (None, Some(false)) => StrKey::CommandLabelUnlock,
                    _ => StrKey::CommandLabelSetFlags,
                };
                with_subject(key, &subject_of(ids))
            }
            Self::RenameInstance { id, .. } => with_subject(
                StrKey::CommandLabelRename,
                &subject_of(std::slice::from_ref(id)),
            ),
            Self::DuplicateInstances(ids) => {
                with_subject(StrKey::CommandLabelDuplicate, &subject_of(ids))
            }
            Self::DeleteInstances(ids) => {
                with_subject(StrKey::CommandLabelDelete, &subject_of(ids))
            }
            Self::AddOperation(operation) => with_subject(
                StrKey::CommandLabelAddOperation,
                &one(strings, operation.name()),
            ),
            Self::UpdateOperation(operation) => {
                let subject = op_subject(operation.id());
                match operations::changed_field(before, operation) {
                    // The editor header's inline rename (#618), worded as
                    // the Design Tree's is.
                    Some(field) if field == "name" => {
                        with_subject(StrKey::CommandLabelRename, &subject)
                    }
                    Some(field) => strings.fmt(
                        StrKey::CommandLabelSetField,
                        &[("field", &field), ("subject", &subject)],
                    ),
                    None => with_subject(StrKey::CommandLabelEditOperation, &subject),
                }
            }
            Self::ReorderOperation { .. } => plain(StrKey::CommandLabelReorderOperations),
            Self::SetOperationEnabled { id, enabled } => {
                let key = if *enabled {
                    StrKey::CommandLabelEnable
                } else {
                    StrKey::CommandLabelDisable
                };
                with_subject(key, &op_subject(id))
            }
            Self::DuplicateOperation(id) => {
                with_subject(StrKey::CommandLabelDuplicate, &op_subject(id))
            }
            Self::DeleteOperation(id) => with_subject(StrKey::CommandLabelDelete, &op_subject(id)),
            Self::PlaceTabs { .. } => {
                let placed = after.tabs.len().saturating_sub(before.tabs.len());
                if placed == 1 {
                    plain(StrKey::CommandLabelPlaceTab)
                } else {
                    strings.fmt(
                        StrKey::CommandLabelPlaceTabs,
                        &[("count", &placed.to_string())],
                    )
                }
            }
            Self::AddTab(_) => plain(StrKey::CommandLabelAddTab),
            Self::MoveTab { .. } => plain(StrKey::CommandLabelMoveTab),
            Self::UpdateTab(_) => plain(StrKey::CommandLabelUpdateTab),
            Self::UpdateTabs(tabs) => {
                if tabs.len() == 1 {
                    plain(StrKey::CommandLabelUpdateTab)
                } else {
                    strings.fmt(
                        StrKey::CommandLabelUpdateTabs,
                        &[("count", &tabs.len().to_string())],
                    )
                }
            }
            Self::DeleteTabs(ids) => {
                if ids.len() == 1 {
                    plain(StrKey::CommandLabelDeleteTab)
                } else {
                    strings.fmt(
                        StrKey::CommandLabelDeleteTabs,
                        &[("count", &ids.len().to_string())],
                    )
                }
            }
            Self::CentreOnStock(ids) => {
                with_subject(StrKey::CommandLabelCentreOnStock, &subject_of(ids))
            }
            Self::FitStockToShapes { .. } => plain(StrKey::CommandLabelFitStock),
        }
    }
}

/// `“{name}”`, one named thing as a label's subject.
fn one(strings: &Strings, name: &str) -> String {
    strings.fmt(StrKey::CommandSubjectOne, &[("name", name)])
}

/// Rejects a value `dc_project::load` would reject at its boundary.
fn finite(field: &'static str, value: f64) -> Result<(), CommandError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(CommandError::InvalidValue { field, value })
    }
}

/// [`finite`] for an optional value.
fn finite_opt(field: &'static str, value: Option<f64>) -> Result<(), CommandError> {
    value.map_or(Ok(()), |v| finite(field, v))
}

/// Appends `tool` to `project`, refused when its id is taken or a number
/// cannot be stored.
fn add_tool(project: &mut Project, tool: &Tool) -> Result<(), CommandError> {
    if project.tools.iter().any(|t| t.id == tool.id) {
        return Err(CommandError::DuplicateId {
            kind: "a tool",
            id: tool.id.as_str().to_owned(),
        });
    }
    check_tool(tool)?;
    project.tools.push(tool.clone());
    Ok(())
}

/// A tool's numbers can be stored.
fn check_tool(tool: &Tool) -> Result<(), CommandError> {
    finite("tools[].diameter_mm", tool.diameter_mm)?;
    finite_opt("tools[].feed_mm_min", tool.feed_mm_min)?;
    finite_opt("tools[].plunge_mm_min", tool.plunge_mm_min)?;
    finite_opt("tools[].step_down_mm", tool.step_down_mm)?;
    finite_opt("tools[].step_over_fraction", tool.step_over_fraction)
}

/// A tab's numbers can be stored.
fn check_tab(tab: &PlacedTab) -> Result<(), CommandError> {
    finite("tabs[].x_mm", tab.x_mm)?;
    finite("tabs[].y_mm", tab.y_mm)?;
    finite("tabs[].width_mm", tab.width_mm)?;
    finite("tabs[].height_mm", tab.height_mm)
}

/// The placed tab `id` of `project`, to change.
fn find_tab_mut<'a>(
    project: &'a mut Project,
    id: &TabId,
) -> Result<&'a mut PlacedTab, CommandError> {
    project
        .tabs
        .iter_mut()
        .find(|t| t.id == *id)
        .ok_or_else(|| CommandError::UnknownTab { id: id.clone() })
}
