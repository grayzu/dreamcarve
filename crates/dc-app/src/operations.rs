// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The Operations list (`docs/specs/phase-4/functional.md`, capability 65;
//! mock-up `01-workspace.svg`; `experience.md`, section 7, "List row"):
//! the eight kinds, the *Add* menu partitioned by what the selection can
//! take, a new operation of a kind, and the rows the list draws.
//!
//! The Add menu's partition is PureCutCNC's (`OperationAddMenu.tsx`,
//! behaviour only): the kinds every selected shape can take first, the
//! rest below a divider with the reason each cannot; with nothing selected
//! one sentence says to select shapes. A shape takes a Pocket, a V-Carve or
//! an Inlay when it has a closed outline, a Drill or a Bore when it has a
//! circle (`dc_geometry::as_circle` on the placed outline, as the Drill
//! generator decides), a Profile or an Engrave always, and a Photo V-Carve
//! needs a picture, which no project holds before phase 3.5.
//!
//! A new operation targets the selection, takes the project's first tool
//! whose type fits (a V-bit for V-Carve, Inlay and Photo V-Carve; an end
//! mill or a drill for Drill; an end mill otherwise), or none, is named
//! `<Kind> <n>` with the first `n` no operation has, and gets the CLI's
//! defaults: `init`'s Profile ([`dc_job::project_from::default_profile`]);
//! for the other kinds the settings the chapters' JSON blocks give, with
//! `init`'s safe Z, direction, entry, order and allowance. Every depth the
//! CLI leaves unset stays `null` (S20): the app never fills one in. The
//! model's `tool` is required, so "none" is the empty id, which validation
//! reports (`dangling_tool`) under the Tool row until one is picked.

use dc_geometry::as_circle;
use dc_project::{
    BoreOp, ClearanceStrategy, DrillCycle, DrillOp, EngraveOp, InlayOp, InstanceId, MaleBoard,
    Operation, OperationId, PocketOp, Project, Tool, ToolId, ToolKind, VCarveOp, VCarveStrategy,
    WallPass,
};

use crate::app::App;
use crate::command::Command;
use crate::strings::{StrKey, Strings};

/// `init`'s Drill retract, mm ("`init` writes 1": the Drill chapter).
const DRILL_RETRACT_MM: f64 = 1.0;

/// An operation kind, as the Add menu lists it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Kind {
    /// Cut along outlines.
    Profile,
    /// Clear the inside of closed outlines.
    Pocket,
    /// Plunge at circles' centres.
    Drill,
    /// Cut circles to size on a helix.
    Bore,
    /// Trace paths on the line.
    Engrave,
    /// Carve along medial lines with a V-bit.
    VCarve,
    /// A V-bit inlay, both boards.
    Inlay,
    /// Carve a picture in grooves (phase 3.5; no record before it).
    PhotoVCarve,
}

impl Kind {
    /// The eight kinds, in the Add menu's order.
    pub const ALL: [Self; 8] = [
        Self::Profile,
        Self::Pocket,
        Self::Drill,
        Self::Bore,
        Self::Engrave,
        Self::VCarve,
        Self::Inlay,
        Self::PhotoVCarve,
    ];

    /// The kind of `operation`.
    #[must_use]
    pub const fn of(operation: &Operation) -> Self {
        match operation {
            Operation::Profile(_) => Self::Profile,
            Operation::Pocket(_) => Self::Pocket,
            Operation::Drill(_) => Self::Drill,
            Operation::Bore(_) => Self::Bore,
            Operation::Engrave(_) => Self::Engrave,
            Operation::VCarve(_) => Self::VCarve,
            Operation::Inlay(_) => Self::Inlay,
            Operation::PhotoVCarve(_) => Self::PhotoVCarve,
        }
    }

    /// Its name, as the chapters give it.
    #[must_use]
    pub const fn label(self) -> StrKey {
        match self {
            Self::Profile => StrKey::OperationsKindProfile,
            Self::Pocket => StrKey::OperationsKindPocket,
            Self::Drill => StrKey::OperationsKindDrill,
            Self::Bore => StrKey::OperationsKindBore,
            Self::Engrave => StrKey::OperationsKindEngrave,
            Self::VCarve => StrKey::OperationsKindVCarve,
            Self::Inlay => StrKey::OperationsKindInlay,
            Self::PhotoVCarve => StrKey::OperationsKindPhotoVCarve,
        }
    }

    /// Whether `tool` fits the kind, for a new operation's tool.
    fn fits(self, tool: &Tool) -> bool {
        match self {
            Self::VCarve | Self::Inlay | Self::PhotoVCarve => {
                matches!(tool.cutter, ToolKind::VBit { .. })
            }
            Self::Drill => matches!(tool.cutter, ToolKind::EndMill | ToolKind::Drill),
            Self::Profile | Self::Pocket | Self::Bore | Self::Engrave => {
                matches!(tool.cutter, ToolKind::EndMill)
            }
        }
    }
}

/// The Add menu for a selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddMenu {
    /// The one sentence shown above a list that offers nothing, when
    /// nothing is selected.
    pub prompt: Option<String>,
    /// The kinds the selection can take, in [`Kind::ALL`]'s order.
    pub available: Vec<Kind>,
    /// The rest, below the divider, each with why it cannot (`None` when
    /// the prompt says it for all).
    pub unavailable: Vec<(Kind, Option<String>)>,
}

/// What one instance's outlines are, for the menu's rules.
#[derive(Debug, Clone, Copy, Default)]
struct Outlines {
    closed: bool,
    circle: bool,
}

/// Instance `id`'s outlines placed as the generators place them.
fn outlines(project: &Project, id: &InstanceId) -> Outlines {
    let Some(instance) = project.instances.iter().find(|i| i.id == *id) else {
        return Outlines::default();
    };
    let Some(definition) = project.definitions.get(&instance.definition) else {
        return Outlines::default();
    };
    let mut found = Outlines::default();
    for profile in &definition.profiles {
        if !profile.is_closed() {
            continue;
        }
        found.closed = true;
        if !found.circle
            && let Ok(placed) = instance.transform.apply_polyline(profile)
        {
            found.circle = as_circle(&placed, project.flatten_tolerance_mm).is_some();
        }
    }
    found
}

/// Why `kind` cannot take the shapes `of`, or `None` when it can.
fn refusal(kind: Kind, of: &[Outlines]) -> Option<StrKey> {
    match kind {
        Kind::Profile | Kind::Engrave => None,
        Kind::Pocket => {
            (!of.iter().all(|o| o.closed)).then_some(StrKey::OperationsReasonOpenPocket)
        }
        Kind::VCarve => {
            (!of.iter().all(|o| o.closed)).then_some(StrKey::OperationsReasonOpenVCarve)
        }
        Kind::Inlay => (!of.iter().all(|o| o.closed)).then_some(StrKey::OperationsReasonOpenInlay),
        Kind::Drill => {
            (!of.iter().all(|o| o.circle)).then_some(StrKey::OperationsReasonCircleDrill)
        }
        Kind::Bore => (!of.iter().all(|o| o.circle)).then_some(StrKey::OperationsReasonCircleBore),
        Kind::PhotoVCarve => Some(StrKey::OperationsReasonPicture),
    }
}

/// The Add menu for the shapes `selected` (capability 65; acceptance 72).
#[must_use]
pub fn add_menu(project: &Project, strings: &Strings, selected: &[InstanceId]) -> AddMenu {
    if selected.is_empty() {
        return AddMenu {
            prompt: Some(strings.get(StrKey::OperationsAddSelectShapes).to_owned()),
            available: Vec::new(),
            unavailable: Kind::ALL.iter().map(|k| (*k, None)).collect(),
        };
    }
    let of: Vec<Outlines> = selected.iter().map(|id| outlines(project, id)).collect();
    let mut menu = AddMenu {
        prompt: None,
        available: Vec::new(),
        unavailable: Vec::new(),
    };
    for kind in Kind::ALL {
        match refusal(kind, &of) {
            None => menu.available.push(kind),
            Some(reason) => menu
                .unavailable
                .push((kind, Some(strings.get(reason).to_owned()))),
        }
    }
    menu
}

/// The project's first tool that fits `kind`, as a new operation takes it.
#[must_use]
pub fn fitting_tool(project: &Project, kind: Kind) -> Option<ToolId> {
    project
        .tools
        .iter()
        .find(|t| kind.fits(t))
        .map(|t| t.id.clone())
}

/// `<Kind> <n>` with the first `n` from 1 that no operation is named.
fn next_name(project: &Project, strings: &Strings, kind: Kind) -> String {
    let kind_name = strings.get(kind.label());
    (1_usize..)
        .map(|n| {
            strings.fmt(
                StrKey::OperationsName,
                &[("kind", kind_name), ("n", &n.to_string())],
            )
        })
        .find(|name| !project.operations.iter().any(|o| o.name() == name))
        .unwrap_or_else(|| kind_name.to_owned())
}

/// The first `op<n>` no operation has.
fn next_id(project: &Project) -> OperationId {
    let taken: Vec<&str> = project.operations.iter().map(|o| o.id().as_str()).collect();
    OperationId::new(crate::command::next_free_id(&taken, "op"))
}

/// A new operation of `kind` on `targets` with the CLI's defaults, the
/// first fitting tool (or none, the empty id) and the next free name and
/// id; `None` for a Photo V-Carve, which has no record before phase 3.5.
#[must_use]
pub fn new_operation(
    project: &Project,
    strings: &Strings,
    kind: Kind,
    targets: &[InstanceId],
) -> Option<Operation> {
    let tool = fitting_tool(project, kind).unwrap_or_else(|| ToolId::new(""));
    let id = next_id(project);
    let name = next_name(project, strings, kind);
    let targets = targets.to_vec();
    // `init`'s Profile carries the settings every kind shares.
    let mut init = dc_job::project_from::default_profile(targets.clone(), tool.clone());
    let (safe_z_mm, through_cut_allowance_mm) = (init.safe_z_mm, init.through_cut_allowance_mm);
    let (direction, machining_order) = (init.direction, init.machining_order);
    Some(match kind {
        Kind::Profile => {
            init.id = id;
            init.name = name;
            Operation::Profile(init)
        }
        Kind::Pocket => Operation::Pocket(PocketOp {
            id,
            name,
            enabled: true,
            targets,
            tool,
            strategy: ClearanceStrategy::Offset,
            wall_pass: WallPass::Last,
            depth_mm: None,
            step_down_mm: None,
            step_over_fraction: None,
            direction,
            stock_to_leave_mm: 0.0,
            rest_of: None,
            entry: init.entry,
            corner_relief: init.corner_relief,
            machining_order,
            through_cut_allowance_mm,
            safe_z_mm,
        }),
        Kind::Drill => Operation::Drill(DrillOp {
            id,
            name,
            enabled: true,
            targets,
            tool,
            cycle: DrillCycle::Simple,
            dwell_s: 0.0,
            depth_mm: None,
            retract_mm: DRILL_RETRACT_MM,
            through_cut_allowance_mm,
            safe_z_mm,
            allow_tool_wider_than_hole: false,
        }),
        Kind::Bore => Operation::Bore(BoreOp {
            id,
            name,
            enabled: true,
            targets,
            tool,
            depth_mm: None,
            pitch_mm: None,
            direction,
            stock_to_leave_mm: 0.0,
            through_cut_allowance_mm,
            safe_z_mm,
        }),
        Kind::Engrave => Operation::Engrave(EngraveOp {
            id,
            name,
            enabled: true,
            targets,
            tool,
            depth_mm: None,
            step_down_mm: None,
            alternate_direction: false,
            machining_order,
            through_cut_allowance_mm,
            safe_z_mm,
        }),
        Kind::VCarve => Operation::VCarve(VCarveOp {
            id,
            name,
            enabled: true,
            targets,
            tool,
            strategy: VCarveStrategy::Medial,
            clearing: Vec::new(),
            flat_depth_mm: None,
            start_depth_mm: 0.0,
            start_plane_cleared_externally: false,
            step_over_fraction: None,
            machining_order,
            through_cut_allowance_mm,
            safe_z_mm,
        }),
        Kind::Inlay => Operation::Inlay(InlayOp {
            id,
            name,
            enabled: true,
            targets,
            tool,
            male_board: MaleBoard::default(),
            depth_mm: None,
            glue_gap_mm: None,
            plug_depth_mm: None,
            margin_mm: None,
            outline_rounding_mm: None,
            boundary: Vec::new(),
            clearing: Vec::new(),
            step_over_fraction: None,
            machining_order,
            through_cut_allowance_mm,
            safe_z_mm,
            cut_out: None,
        }),
        Kind::PhotoVCarve => return None,
    })
}

/// *Add ▸ `kind`* on the shapes `selected`: `None` when the menu does not
/// offer the kind for them.
#[must_use]
pub fn add(
    project: &Project,
    strings: &Strings,
    kind: Kind,
    selected: &[InstanceId],
) -> Option<Command> {
    if !add_menu(project, strings, selected)
        .available
        .contains(&kind)
    {
        return None;
    }
    new_operation(project, strings, kind, selected).map(Command::AddOperation)
}

/// Moving operation `id` to position `to`, or `None` when it is there.
#[must_use]
pub fn reorder(project: &Project, id: &OperationId, to: usize) -> Option<Command> {
    let from = project.operations.iter().position(|o| o.id() == id)?;
    (from != to && to < project.operations.len())
        .then(|| Command::ReorderOperation { id: id.clone(), to })
}

/// A row's status cell (`experience.md`, section 7, "Status cell").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Nothing to say.
    Clean,
    /// Waiting for its result: the spinner.
    Generating,
    /// This many warnings (hints aside).
    Warnings(usize),
    /// An error stops it.
    Error,
}

/// One row of the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    /// The operation.
    pub id: OperationId,
    /// Its kind, for the glyph.
    pub kind: Kind,
    /// Its place in the list, which picks its colour.
    pub index: usize,
    /// Its name.
    pub name: String,
    /// Whether it is cut.
    pub enabled: bool,
    /// The second line: the kind and the tool's name.
    pub detail: String,
    /// Whether its toolpath is hidden (the eye; view state).
    pub hidden: bool,
    /// Whether the editor shows it.
    pub editing: bool,
    /// The status cell.
    pub status: Status,
    /// What hovering the status cell lists: the operation's errors, then
    /// its warnings, as the Warnings panel words them.
    pub messages: Vec<String>,
}

/// The list's rows, in cut order (capability 65).
#[must_use]
pub fn rows(app: &App) -> Vec<ListRow> {
    let project = app.project();
    let strings = app.strings();
    let findings = app.warnings();
    let generation = app.generation();
    project
        .operations
        .iter()
        .enumerate()
        .map(|(index, op)| {
            let kind = Kind::of(op);
            let tool = project
                .tools
                .iter()
                .find(|t| t.id == *op.tool())
                .map_or_else(
                    || strings.get(StrKey::OperationsNoTool),
                    |t| t.name.as_str(),
                );
            let mine = || findings.iter().filter(|f| f.op.as_ref() == Some(op.id()));
            let status = if generation.is_regenerating(op.id()) {
                Status::Generating
            } else if mine().any(|f| f.tier == dc_job::Tier::Error) {
                Status::Error
            } else {
                match mine().filter(|f| f.tier == dc_job::Tier::Warning).count() {
                    0 => Status::Clean,
                    n => Status::Warnings(n),
                }
            };
            ListRow {
                id: op.id().clone(),
                kind,
                index,
                name: op.name().to_owned(),
                enabled: op.enabled(),
                detail: strings.fmt(
                    StrKey::OperationsDetail,
                    &[("kind", strings.get(kind.label())), ("tool", tool)],
                ),
                hidden: app.view().hidden_ops.contains(op.id()),
                editing: app.editing() == Some(op.id()),
                status,
                messages: [dc_job::Tier::Error, dc_job::Tier::Warning]
                    .into_iter()
                    .flat_map(|tier| mine().filter(move |f| f.tier == tier))
                    .map(|f| f.message.clone())
                    .collect(),
            }
        })
        .collect()
}
