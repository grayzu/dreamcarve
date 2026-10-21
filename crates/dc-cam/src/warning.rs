// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Non-fatal findings of toolpath generation (`docs/specs/phase-1/design.md`,
//! "dc-cam"). A warning still yields a safe toolpath; anything that would
//! make motion wrong is a [`crate::CamError`] instead.

use std::fmt;

use dc_geometry::EPS_MM;
use dc_project::{CornerRelief, OperationId, ToolId};

use crate::entry::{EntryKind, FallbackReason};
use crate::error::ShapeRef;

/// What a [`Warning`] is about: one code per [`Warning`] variant, whose
/// fields name the things involved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WarningCode {
    /// A target shape is an open path, which a Pocket and an Inside or
    /// Outside Profile cannot cut, or an open path with no length on an On
    /// line Profile (#175); it was skipped.
    OpenPathSkipped,
    /// The tool does not fit: the tool-centre offset of a shape vanished,
    /// so that shape was not cut.
    ToolTooLarge,
    /// A target shape duplicates an earlier one and was dropped so it is
    /// not cut twice.
    DuplicateShapeDropped,
    /// Two or more tab spans on one tool-centre contour overlap, so they
    /// were merged into one raised span.
    TabsOverlap,
    /// The tab top lies outside the range the cut passes through: no pass
    /// goes below it (the tabs change nothing), or it is above the stock
    /// top (the tabs still apply).
    TabsOutsideCutRange,
    /// The tab spans on one tool-centre contour would cover more than half
    /// of its length, so the part or hole would not cut free. The contour
    /// gets the most tabs that cover at most half, down to none (#109).
    TabsCoverMostOfContour,
    /// The tool path of one shape cuts into material another shape in the
    /// same operation keeps. The shapes are cut as drawn.
    ToolpathsOverlap,
    /// The tool is wider than a pocket region somewhere it must pass: the
    /// tool-centre region of that shape is empty, so the shape is not cut.
    PocketNoRoom,
    /// Two of a pocket's boundaries, an island and the wall or two
    /// islands, are closer than the tool needs to pass between them (its
    /// diameter plus twice the allowance), so the material between them
    /// stays.
    PocketIslandTooClose,
    /// The entry asked for did not fit a shape, so its passes start with
    /// the next one in the chain helix → ramp → plunge
    /// (`docs/specs/phase-2/design.md`, "Algorithms → Entry"): the shape
    /// leaves no room for it, or at this angle and depth it would need more
    /// than the descent move limit.
    EntryFallback,
    /// A helix entry fits a shape only smaller than asked, so it is cut at
    /// the largest diameter that fits.
    HelixDiameterReduced,
    /// A lead-in or lead-out arc fits a shape only smaller than asked, so
    /// it is cut at the largest radius and widest sweep that fit
    /// (`docs/specs/phase-2/design.md`, "Algorithms → Lead-in and
    /// lead-out").
    LeadShrunk,
    /// A lead asked for is not cut ([`LeadDroppedReason`]).
    LeadDropped,
    /// The operation's corner relief was not cut at all
    /// (`docs/specs/phase-2/design.md`, "Algorithms → Corner relief";
    /// [`ReliefSkippedReason`]).
    ReliefSkipped,
    /// A corner of the cleared area is not relieved because the notch
    /// does not fit it ([`ReliefCornerReason`]).
    ReliefCornerTooTight,
    /// A corner of the cleared area is not relieved because the main
    /// passes did not cut to full depth where the notch would start (a
    /// tab over it, or no wall pass), so the notch would plunge into
    /// uncut material.
    ReliefCornerNotCut,
    /// An Engrave target path has fewer than two distinct points, so there
    /// is nothing to trace; it was skipped.
    EngravePathDegenerate,
    /// A closed target of this operation lies inside an outline an earlier
    /// operation cuts free of the stock, so the cut would run in a loose
    /// part (`docs/specs/phase-2/design.md`, "Operation-order warnings";
    /// research O8).
    CutInsideFreedPart,
    /// A tool is used by operations that do not run one after another, so
    /// it is fitted more than once. Operations are never reordered (D7);
    /// this is a hint.
    ToolChangeAvoidable,
    /// A Drill target is not a circle (or is an open path), so it was
    /// skipped.
    DrillTargetNotCircle,
    /// The drilling tool is wider than the hole it drills; the hole comes
    /// out the tool's size.
    DrillToolWiderThanHole,
    /// The peck depth is at least the depth of the operation's holes, so
    /// each is drilled in one plunge. Reported once per operation: every
    /// hole has the same depth.
    DrillPeckDeeperThanHole,
    /// The retract height is below the stock top, so it was raised to the
    /// top: the tool must never rapid into material.
    DrillRetractBelowStockTop,
    /// A rest Pocket's tool reaches nothing the Pocket it rests on left
    /// (`docs/specs/phase-2/design.md`, "Algorithms → Pocket", step 3), so
    /// it emits no toolpath.
    RestOfClearsNothing,
}

impl WarningCode {
    /// The stable `snake_case` name of the code.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::OpenPathSkipped => "open_path_skipped",
            Self::ToolTooLarge => "tool_too_large",
            Self::DuplicateShapeDropped => "duplicate_shape_dropped",
            Self::TabsOverlap => "tabs_overlap",
            Self::TabsOutsideCutRange => "tabs_outside_cut_range",
            Self::TabsCoverMostOfContour => "tabs_cover_most_of_contour",
            Self::ToolpathsOverlap => "toolpaths_overlap",
            Self::PocketNoRoom => "pocket_no_room",
            Self::PocketIslandTooClose => "pocket_island_too_close",
            Self::EntryFallback => "entry_fallback",
            Self::HelixDiameterReduced => "helix_diameter_reduced",
            Self::LeadShrunk => "lead_shrunk",
            Self::LeadDropped => "lead_dropped",
            Self::ReliefSkipped => "relief_skipped",
            Self::ReliefCornerTooTight => "relief_corner_too_tight",
            Self::ReliefCornerNotCut => "relief_corner_not_cut",
            Self::EngravePathDegenerate => "engrave_path_degenerate",
            Self::CutInsideFreedPart => "cut_inside_freed_part",
            Self::ToolChangeAvoidable => "tool_change_avoidable",
            Self::DrillTargetNotCircle => "drill_target_not_circle",
            Self::DrillToolWiderThanHole => "drill_tool_wider_than_hole",
            Self::DrillPeckDeeperThanHole => "drill_peck_deeper_than_hole",
            Self::DrillRetractBelowStockTop => "drill_retract_below_stock_top",
            Self::RestOfClearsNothing => "rest_of_clears_nothing",
        }
    }
}

impl fmt::Display for WarningCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why an open path was skipped ([`Warning::OpenPathSkipped`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenPathReason {
    /// A Pocket or an Inside or Outside Profile cannot cut an open path:
    /// it has no inside or outside.
    NoSide,
    /// An On line Profile's open path has no length (#175).
    NoLength,
}

/// Where the tab top lies outside the cut ([`Warning::TabsOutsideCutRange`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabsRangeReason {
    /// No pass goes below the tab top, so the tabs change nothing.
    CutStopsAboveTab,
    /// The tab top is above the stock top; the tabs still apply.
    AboveStockTop,
}

/// What an island is too close to ([`Warning::PocketIslandTooClose`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandNeighbour {
    /// The pocket's wall.
    Wall,
    /// Another island, by its profile index.
    Island(usize),
}

/// Why a lead was not cut ([`Warning::LeadDropped`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeadDroppedReason {
    /// No lead-in fits the shape even at a quarter of the radius, so its
    /// passes start on the outline (below the tab top only, when only the
    /// tabs are in the way).
    NoRoom,
    /// No lead-out fits where the cut ends, so it stops on the outline.
    Exit,
    /// An On line Profile has no waste side for a lead, so the operation
    /// cuts without one.
    OnLine,
}

/// Why an operation's corner relief was not cut at all
/// ([`Warning::ReliefSkipped`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReliefSkippedReason {
    /// The tool has no step-down, which the relief pass steps down at.
    NoStepDown,
    /// An On line Profile has no waste side to put a notch in.
    OnLine,
}

/// Why a corner's notch does not fit it ([`Warning::ReliefCornerTooTight`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReliefCornerReason {
    /// An edge beside the corner is shorter than the tool is wide.
    ShortEdge,
    /// The tool-centre path does not turn the corner within reach.
    NoWallPath,
}

/// Something the user should know about that still yields a safe toolpath:
/// one variant per [`WarningCode`], with the things involved. Its
/// [`Display`](fmt::Display) is a plain sentence, values rounded for
/// display; the operation is not named, the caller says which operation
/// the warning came from.
#[derive(Debug, Clone, PartialEq)]
pub enum Warning {
    /// [`WarningCode::OpenPathSkipped`].
    OpenPathSkipped {
        /// The open path.
        shape: ShapeRef,
        /// Why it cannot be cut.
        reason: OpenPathReason,
    },
    /// [`WarningCode::ToolTooLarge`].
    ToolTooLarge {
        /// The shape not cut.
        shape: ShapeRef,
        /// The tool's name.
        tool: String,
        /// The tool's id.
        tool_id: ToolId,
        /// The tool's diameter.
        diameter_mm: f64,
    },
    /// [`WarningCode::DuplicateShapeDropped`].
    DuplicateShapeDropped {
        /// The shape dropped.
        shape: ShapeRef,
        /// The earlier shape it duplicates, which is cut.
        duplicate_of: ShapeRef,
    },
    /// [`WarningCode::TabsOverlap`].
    TabsOverlap {
        /// The operation.
        op: OperationId,
        /// The tool-centre contour, 0-based in cut order.
        contour: usize,
        /// The shape the contour is cut for, when it is known.
        shape: Option<ShapeRef>,
        /// The contour's length.
        path_length_mm: f64,
    },
    /// [`WarningCode::TabsOutsideCutRange`].
    TabsOutsideCutRange {
        /// The operation.
        op: OperationId,
        /// Which side of the cut the tab top lies on.
        reason: TabsRangeReason,
        /// The tab top, above the stock bottom.
        tab_top_mm: f64,
        /// The deepest pass, above the stock bottom.
        deepest_pass_z_mm: f64,
        /// The stock top, above the stock bottom.
        stock_top_mm: f64,
    },
    /// [`WarningCode::TabsCoverMostOfContour`].
    TabsCoverMostOfContour {
        /// The operation.
        op: OperationId,
        /// The tool-centre contour, 0-based in cut order.
        contour: usize,
        /// The shape the contour is cut for, when it is known.
        shape: Option<ShapeRef>,
        /// The contour's length.
        path_length_mm: f64,
        /// How many tabs the placement asked for on it.
        tab_count: usize,
        /// The tab width as set.
        tab_width_mm: f64,
        /// The tool diameter.
        diameter_mm: f64,
        /// One tab's length of path: width plus tool diameter.
        tab_span_mm: f64,
        /// `tab_count` times `tab_span_mm`.
        tab_total_mm: f64,
        /// How many tabs are cut on it: the most that cover at most half,
        /// fewer than `tab_count`, possibly none.
        cut_count: usize,
        /// The widest tab at `tab_count` that covers at most half; `None`
        /// when none fits.
        max_width_mm: Option<f64>,
    },
    /// [`WarningCode::ToolpathsOverlap`].
    ToolpathsOverlap {
        /// The shape cut first.
        shape: ShapeRef,
        /// The shape whose material it cuts into.
        other: ShapeRef,
        /// The smallest distance between the two shapes' lines.
        gap_mm: f64,
        /// The tool's name.
        tool: String,
        /// The tool's id.
        tool_id: ToolId,
        /// The tool's diameter.
        diameter_mm: f64,
        /// The gap this side needs: the diameter for Outside, the radius
        /// for On line.
        needed_mm: f64,
    },
    /// [`WarningCode::PocketNoRoom`].
    PocketNoRoom {
        /// The operation.
        op: OperationId,
        /// The shape not cut.
        shape: ShapeRef,
        /// The tool's name.
        tool: String,
        /// The tool's id.
        tool_id: ToolId,
        /// The tool's diameter.
        diameter_mm: f64,
    },
    /// [`WarningCode::PocketIslandTooClose`].
    PocketIslandTooClose {
        /// The operation.
        op: OperationId,
        /// The pocket's outline.
        shape: ShapeRef,
        /// The island's profile index.
        island: usize,
        /// What the island is too close to.
        other: IslandNeighbour,
        /// The distance between them.
        gap_mm: f64,
        /// The distance the tool needs: `diameter_mm + 2 × allowance_mm`.
        needed_mm: f64,
        /// The operation's allowance.
        allowance_mm: f64,
        /// The tool's name.
        tool: String,
        /// The tool's id.
        tool_id: ToolId,
        /// The tool's diameter.
        diameter_mm: f64,
    },
    /// [`WarningCode::EntryFallback`].
    EntryFallback {
        /// The operation.
        op: OperationId,
        /// The shape.
        shape: ShapeRef,
        /// The entry asked for: a ramp or a helix.
        requested: EntryKind,
        /// The entry used: a ramp or a plunge.
        used: EntryKind,
        /// Why the one asked for was not used.
        reason: FallbackReason,
    },
    /// [`WarningCode::HelixDiameterReduced`].
    HelixDiameterReduced {
        /// The operation.
        op: OperationId,
        /// The shape.
        shape: ShapeRef,
        /// The helix diameter asked for.
        requested_mm: f64,
        /// The smallest helix diameter cut in the shape.
        used_mm: f64,
    },
    /// [`WarningCode::LeadShrunk`].
    LeadShrunk {
        /// The operation.
        op: OperationId,
        /// The shape.
        shape: ShapeRef,
        /// The lead radius asked for.
        requested_mm: f64,
        /// The smallest radius cut on the shape.
        used_mm: f64,
        /// The sweep asked for.
        requested_sweep_deg: f64,
        /// The smallest sweep cut.
        used_sweep_deg: f64,
    },
    /// [`WarningCode::LeadDropped`].
    LeadDropped {
        /// The operation.
        op: OperationId,
        /// The shape, for [`LeadDroppedReason::NoRoom`] and
        /// [`LeadDroppedReason::Exit`].
        shape: Option<ShapeRef>,
        /// Why the lead is not cut.
        reason: LeadDroppedReason,
    },
    /// [`WarningCode::ReliefSkipped`].
    ReliefSkipped {
        /// The operation.
        op: OperationId,
        /// The relief asked for.
        requested: CornerRelief,
        /// Why none was cut.
        reason: ReliefSkippedReason,
    },
    /// [`WarningCode::ReliefCornerTooTight`].
    ReliefCornerTooTight {
        /// The operation.
        op: OperationId,
        /// The shape.
        shape: ShapeRef,
        /// The corner's X.
        x_mm: f64,
        /// The corner's Y.
        y_mm: f64,
        /// Why the notch does not fit.
        reason: ReliefCornerReason,
    },
    /// [`WarningCode::ReliefCornerNotCut`].
    ReliefCornerNotCut {
        /// The operation.
        op: OperationId,
        /// The shape.
        shape: ShapeRef,
        /// The corner's X.
        x_mm: f64,
        /// The corner's Y.
        y_mm: f64,
    },
    /// [`WarningCode::EngravePathDegenerate`].
    EngravePathDegenerate {
        /// The operation.
        op: OperationId,
        /// The path skipped.
        shape: ShapeRef,
    },
    /// [`WarningCode::CutInsideFreedPart`].
    CutInsideFreedPart {
        /// This operation.
        op: OperationId,
        /// The earlier operation that frees the part.
        earlier_op: OperationId,
        /// The shape this operation cuts.
        shape: ShapeRef,
        /// The outline the earlier operation frees.
        freed: ShapeRef,
    },
    /// [`WarningCode::ToolChangeAvoidable`].
    ToolChangeAvoidable {
        /// The tool's id.
        tool_id: ToolId,
        /// The tool's name, or `?` when the project has no such tool.
        tool: String,
        /// Every enabled operation using the tool, in order.
        ops: Vec<OperationId>,
    },
    /// [`WarningCode::DrillTargetNotCircle`].
    DrillTargetNotCircle {
        /// The operation.
        op: OperationId,
        /// The shape skipped.
        shape: ShapeRef,
    },
    /// [`WarningCode::DrillToolWiderThanHole`].
    DrillToolWiderThanHole {
        /// The operation.
        op: OperationId,
        /// The hole.
        shape: ShapeRef,
        /// The hole's diameter.
        hole_mm: f64,
        /// The tool's diameter.
        tool_mm: f64,
    },
    /// [`WarningCode::DrillPeckDeeperThanHole`].
    DrillPeckDeeperThanHole {
        /// The operation.
        op: OperationId,
        /// The peck depth.
        peck_mm: f64,
        /// The holes' depth.
        depth_mm: f64,
    },
    /// [`WarningCode::DrillRetractBelowStockTop`].
    DrillRetractBelowStockTop {
        /// The operation.
        op: OperationId,
        /// The height above the stock top as set, negative.
        requested_mm: f64,
        /// The height used: 0.
        used_mm: f64,
    },
    /// [`WarningCode::RestOfClearsNothing`].
    RestOfClearsNothing {
        /// The operation.
        op: OperationId,
        /// The operation it rests on.
        rest_of: OperationId,
        /// This operation's tool name.
        tool: String,
        /// This operation's tool id.
        tool_id: ToolId,
    },
}

impl Warning {
    /// What happened.
    #[must_use]
    pub const fn code(&self) -> WarningCode {
        match self {
            Self::OpenPathSkipped { .. } => WarningCode::OpenPathSkipped,
            Self::ToolTooLarge { .. } => WarningCode::ToolTooLarge,
            Self::DuplicateShapeDropped { .. } => WarningCode::DuplicateShapeDropped,
            Self::TabsOverlap { .. } => WarningCode::TabsOverlap,
            Self::TabsOutsideCutRange { .. } => WarningCode::TabsOutsideCutRange,
            Self::TabsCoverMostOfContour { .. } => WarningCode::TabsCoverMostOfContour,
            Self::ToolpathsOverlap { .. } => WarningCode::ToolpathsOverlap,
            Self::PocketNoRoom { .. } => WarningCode::PocketNoRoom,
            Self::PocketIslandTooClose { .. } => WarningCode::PocketIslandTooClose,
            Self::EntryFallback { .. } => WarningCode::EntryFallback,
            Self::HelixDiameterReduced { .. } => WarningCode::HelixDiameterReduced,
            Self::LeadShrunk { .. } => WarningCode::LeadShrunk,
            Self::LeadDropped { .. } => WarningCode::LeadDropped,
            Self::ReliefSkipped { .. } => WarningCode::ReliefSkipped,
            Self::ReliefCornerTooTight { .. } => WarningCode::ReliefCornerTooTight,
            Self::ReliefCornerNotCut { .. } => WarningCode::ReliefCornerNotCut,
            Self::EngravePathDegenerate { .. } => WarningCode::EngravePathDegenerate,
            Self::CutInsideFreedPart { .. } => WarningCode::CutInsideFreedPart,
            Self::ToolChangeAvoidable { .. } => WarningCode::ToolChangeAvoidable,
            Self::DrillTargetNotCircle { .. } => WarningCode::DrillTargetNotCircle,
            Self::DrillToolWiderThanHole { .. } => WarningCode::DrillToolWiderThanHole,
            Self::DrillPeckDeeperThanHole { .. } => WarningCode::DrillPeckDeeperThanHole,
            Self::DrillRetractBelowStockTop { .. } => WarningCode::DrillRetractBelowStockTop,
            Self::RestOfClearsNothing { .. } => WarningCode::RestOfClearsNothing,
        }
    }

    /// The operation the warning is about, for the warnings that name one.
    #[must_use]
    pub const fn op(&self) -> Option<&OperationId> {
        match self {
            Self::TabsOverlap { op, .. }
            | Self::TabsOutsideCutRange { op, .. }
            | Self::TabsCoverMostOfContour { op, .. }
            | Self::PocketNoRoom { op, .. }
            | Self::PocketIslandTooClose { op, .. }
            | Self::EntryFallback { op, .. }
            | Self::HelixDiameterReduced { op, .. }
            | Self::LeadShrunk { op, .. }
            | Self::LeadDropped { op, .. }
            | Self::ReliefSkipped { op, .. }
            | Self::ReliefCornerTooTight { op, .. }
            | Self::ReliefCornerNotCut { op, .. }
            | Self::EngravePathDegenerate { op, .. }
            | Self::CutInsideFreedPart { op, .. }
            | Self::DrillTargetNotCircle { op, .. }
            | Self::DrillToolWiderThanHole { op, .. }
            | Self::DrillPeckDeeperThanHole { op, .. }
            | Self::DrillRetractBelowStockTop { op, .. }
            | Self::RestOfClearsNothing { op, .. } => Some(op),
            Self::OpenPathSkipped { .. }
            | Self::ToolTooLarge { .. }
            | Self::DuplicateShapeDropped { .. }
            | Self::ToolpathsOverlap { .. }
            | Self::ToolChangeAvoidable { .. } => None,
        }
    }
}

/// `value` rounded to `decimals` places, trailing zeros dropped.
fn rounded(value: f64, decimals: usize) -> String {
    let text = format!("{value:.decimals$}");
    let text = if text.contains('.') {
        text.trim_end_matches('0').trim_end_matches('.')
    } else {
        &text
    };
    if text == "-0" { "0" } else { text }.to_owned()
}

/// Lengths are shown to 0.01 mm, tool diameters to 0.001 mm (so a 1/8"
/// bit reads 3.175 mm), shares to 0.1%.
const MM_DECIMALS: usize = 2;
const DIAMETER_DECIMALS: usize = 3;
const PERCENT_DECIMALS: usize = 1;

/// `value_mm` as a length, rounded for display.
fn mm(value_mm: f64) -> String {
    rounded(value_mm, MM_DECIMALS)
}

/// `value_mm` as a tool or hole diameter, rounded for display.
fn diameter(value_mm: f64) -> String {
    rounded(value_mm, DIAMETER_DECIMALS)
}

/// The shape a tab warning is about: `shape` when it is known, else the
/// contour `contour` (0-based, in cut order).
fn shape_or_contour(shape: Option<&ShapeRef>, contour: usize) -> String {
    match shape {
        Some(shape) => shape.to_string(),
        None => format!("contour {} in cut order", contour + 1),
    }
}

/// The name the project file uses for `relief`.
const fn relief_name(relief: CornerRelief) -> &'static str {
    match relief {
        CornerRelief::None => "none",
        CornerRelief::Dogbone => "dogbone",
        CornerRelief::TBone => "t_bone",
    }
}

/// What is cut instead of `tab_count` tabs that would cover most of a
/// contour (`cut_count` of them; `kept_covers_most` when that is the one
/// tab a part keeps and it still covers more than half), and the width at
/// which all of them would fit, `max_width_mm`.
fn coverage_advice(
    tab_count: usize,
    cut_count: usize,
    kept_covers_most: bool,
    max_width_mm: Option<f64>,
) -> String {
    let cut = match (cut_count, kept_covers_most) {
        (0, _) => "No tab is cut there".to_owned(),
        (1, true) if tab_count == 1 => "Its one tab is kept, as a part keeps at least one, so the part may need cutting free by hand".to_owned(),
        (1, true) => "Only 1 of them is cut there, as a part keeps at least one, so the part may need cutting free by hand".to_owned(),
        (1, false) => "Only 1 of them is cut there".to_owned(),
        (n, _) => format!("Only {n} of them are cut there"),
    };
    match max_width_mm {
        Some(width_mm) if tab_count == 1 && cut_count == 1 => format!(
            "{cut}. To let it cut free, make it at most {} mm wide.",
            mm(width_mm)
        ),
        Some(width_mm) if tab_count == 1 => format!(
            "{cut}. To keep it, make it at most {} mm wide.",
            mm(width_mm)
        ),
        Some(width_mm) => format!(
            "{cut}. To keep all {tab_count}, make them at most {} mm wide.",
            mm(width_mm)
        ),
        None if cut_count == 0 => {
            format!("{cut}: this outline is too short for tabs with this bit.")
        }
        None => format!("{cut}."),
    }
}

impl fmt::Display for Warning {
    #[allow(clippy::too_many_lines)] // one sentence per variant
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OpenPathSkipped {
                shape,
                reason: OpenPathReason::NoLength,
            } => write!(
                f,
                "{shape} is an open path with no length, so there is nothing to cut; it is skipped"
            ),
            Self::OpenPathSkipped {
                shape,
                reason: OpenPathReason::NoSide,
            } => write!(
                f,
                "{shape} is an open path, which has no inside or outside, so it is not cut; an On line Profile or an Engrave cuts along it"
            ),
            Self::ToolTooLarge {
                shape,
                tool,
                diameter_mm,
                ..
            } => write!(
                f,
                "tool `{tool}` ({} mm) does not fit {shape}, so that shape is not cut",
                mm(*diameter_mm)
            ),
            Self::DuplicateShapeDropped {
                shape,
                duplicate_of,
            } => write!(
                f,
                "{shape} is the same outline in the same place as instance `{}` profile {}, so it is cut only once",
                duplicate_of.instance, duplicate_of.profile
            ),
            Self::TabsOverlap {
                contour,
                shape,
                path_length_mm,
                ..
            } => write!(
                f,
                "tabs on {} touch or overlap on its {} mm tool path, so they were joined into longer tabs; use fewer or narrower tabs",
                shape_or_contour(shape.as_ref(), *contour),
                mm(*path_length_mm)
            ),
            Self::TabsOutsideCutRange {
                reason: TabsRangeReason::AboveStockTop,
                tab_top_mm,
                stock_top_mm,
                ..
            } => write!(
                f,
                "the tab top is {} mm above the stock bottom, higher than the {} mm stock top; over each tab every pass lifts clear of the stock",
                mm(*tab_top_mm),
                mm(*stock_top_mm)
            ),
            Self::TabsOutsideCutRange {
                reason: TabsRangeReason::CutStopsAboveTab,
                tab_top_mm,
                deepest_pass_z_mm,
                ..
            } => write!(
                f,
                "the tab top is {} mm above the stock bottom, but the deepest pass stops at {} mm, so the tabs change nothing; lower the tabs or cut deeper",
                mm(*tab_top_mm),
                mm(*deepest_pass_z_mm)
            ),
            Self::TabsCoverMostOfContour {
                contour,
                shape,
                path_length_mm,
                tab_count,
                tab_width_mm,
                diameter_mm,
                tab_span_mm,
                tab_total_mm,
                cut_count,
                max_width_mm,
                ..
            } => {
                let share = if *path_length_mm > 0.0 {
                    format!(
                        " ({}%)",
                        rounded(100.0 * tab_total_mm / path_length_mm, PERCENT_DECIMALS)
                    )
                } else {
                    String::new()
                };
                let width = mm(*tab_width_mm);
                let diameter = diameter(*diameter_mm);
                let span = mm(*tab_span_mm);
                let path = mm(*path_length_mm);
                let tabs = if *tab_count == 1 {
                    format!(
                        "its one tab is {width} mm wide, and with the {diameter} mm bit it keeps {span} mm of the tool path raised on a {path} mm path{share}"
                    )
                } else {
                    format!(
                        "each of the {tab_count} tabs is {width} mm wide, and with the {diameter} mm bit each one keeps {span} mm of the tool path raised, {} mm in total on a {path} mm path{share}",
                        mm(*tab_total_mm)
                    )
                };
                write!(
                    f,
                    "tabs on {} would cover more than half of its tool path: {tabs}, so the part or hole would not cut free. {}",
                    shape_or_contour(shape.as_ref(), *contour),
                    coverage_advice(
                        *tab_count,
                        *cut_count,
                        *cut_count == 1 && 2.0 * tab_span_mm > *path_length_mm + EPS_MM,
                        *max_width_mm
                    )
                )
            }
            Self::ToolpathsOverlap {
                shape,
                other,
                gap_mm,
                tool,
                diameter_mm,
                needed_mm,
                ..
            } => write!(
                f,
                "the tool paths of {shape} and {other} cut into each other: the shapes are {} mm apart, closer than the {} mm tool `{tool}` ({} mm) needs between them, so cutting one gouges the other. Move them at least {} mm apart or use a smaller bit.",
                mm(*gap_mm),
                mm(*needed_mm),
                mm(*diameter_mm),
                mm(*needed_mm)
            ),
            Self::PocketNoRoom {
                shape,
                tool,
                diameter_mm,
                ..
            } => write!(
                f,
                "tool `{tool}` ({} mm) is wider than the pocket of {shape} somewhere it must pass, so that shape is not cut; use a smaller bit",
                diameter(*diameter_mm)
            ),
            Self::PocketIslandTooClose {
                shape,
                island,
                other,
                gap_mm,
                needed_mm,
                allowance_mm,
                tool,
                diameter_mm,
                ..
            } => {
                let other = match other {
                    IslandNeighbour::Wall => "the pocket wall".to_owned(),
                    IslandNeighbour::Island(index) => format!("island profile {index}"),
                };
                let needs = if *allowance_mm > 0.0 {
                    format!(
                        "tool `{tool}` needs with a {} mm allowance ({} mm)",
                        mm(*allowance_mm),
                        diameter(*needed_mm)
                    )
                } else {
                    format!("the {} mm tool `{tool}` is wide", diameter(*diameter_mm))
                };
                write!(
                    f,
                    "in the pocket of {shape}, island profile {island} is {} mm from {other}, closer than {needs}, so the material between them is left uncut",
                    mm(*gap_mm),
                )
            }
            Self::EntryFallback {
                shape,
                requested,
                used,
                reason,
                ..
            } => {
                let requested = requested.as_str();
                let why = match reason {
                    FallbackReason::TooManyMoves => format!(
                        "a `{requested}` entry at this angle would take too many moves to reach depth in"
                    ),
                    FallbackReason::NoRoom => {
                        format!("there is no room for a `{requested}` entry in")
                    }
                };
                let (used, advice) = match used {
                    EntryKind::Ramp => ("a zigzag ramp", ""),
                    EntryKind::Plunge | EntryKind::Helix => (
                        "a straight plunge at the plunge rate",
                        "; set the operation's entry to `plunge` to stop this warning",
                    ),
                };
                write!(
                    f,
                    "{why} {shape}, so its passes start with {used} instead{advice}"
                )
            }
            Self::HelixDiameterReduced {
                shape,
                requested_mm,
                used_mm,
                ..
            } => write!(
                f,
                "the helix entry in {shape} is {} mm across, smaller than the {} mm asked for, because a larger one does not fit there",
                diameter(*used_mm),
                diameter(*requested_mm)
            ),
            Self::LeadShrunk {
                shape,
                requested_mm,
                used_mm,
                requested_sweep_deg,
                used_sweep_deg,
                ..
            } => write!(
                f,
                "the lead arc on {shape} is {} mm in radius over {}°, smaller than the {} mm over {}° asked for, because a larger one does not fit there",
                mm(*used_mm),
                rounded(*used_sweep_deg, 0),
                mm(*requested_mm),
                rounded(*requested_sweep_deg, 0)
            ),
            Self::LeadDropped { op, shape, reason } => {
                let shape = shape
                    .as_ref()
                    .map_or_else(|| "a shape".to_owned(), ShapeRef::to_string);
                match reason {
                    LeadDroppedReason::OnLine => write!(
                        f,
                        "operation `{op}` cuts on the line, which has no waste side for a lead, so it cuts without one; remove the lead or cut Inside or Outside"
                    ),
                    LeadDroppedReason::Exit => write!(
                        f,
                        "there is no room for a lead-out arc where the cut on {shape} ends, so it stops on the outline"
                    ),
                    LeadDroppedReason::NoRoom => write!(
                        f,
                        "there is no room for a lead-in arc on {shape}, even at a quarter of the radius, so its passes start on the outline without one"
                    ),
                }
            }
            Self::ReliefSkipped {
                requested, reason, ..
            } => {
                let requested = relief_name(*requested);
                match reason {
                    ReliefSkippedReason::OnLine => write!(
                        f,
                        "corner relief `{requested}` is skipped: an On line Profile has no waste side to cut a notch into, so no corners were relieved"
                    ),
                    ReliefSkippedReason::NoStepDown => write!(
                        f,
                        "corner relief `{requested}` is skipped because the tool has no step_down_mm, which the relief pass steps down at, so no corners were relieved; set the tool's step-down"
                    ),
                }
            }
            Self::ReliefCornerTooTight {
                shape,
                x_mm,
                y_mm,
                reason,
                ..
            } => {
                let why = match reason {
                    ReliefCornerReason::NoWallPath => "the tool path does not turn it within reach",
                    ReliefCornerReason::ShortEdge => {
                        "an edge beside it is shorter than the bit is wide"
                    }
                };
                write!(
                    f,
                    "the corner of {shape} at ({}, {}) mm is not relieved: {why}, so it keeps the bit's radius",
                    mm(*x_mm),
                    mm(*y_mm)
                )
            }
            Self::ReliefCornerNotCut {
                shape, x_mm, y_mm, ..
            } => write!(
                f,
                "the corner of {shape} at ({}, {}) mm is not relieved because the main passes did not cut to full depth where its notch starts (a tab over it, or no wall pass), and a notch must not plunge into uncut material; it keeps the bit's radius",
                mm(*x_mm),
                mm(*y_mm)
            ),
            Self::EngravePathDegenerate { shape, .. } => write!(
                f,
                "{shape} has fewer than two distinct points, so there is nothing to engrave; it is skipped"
            ),
            Self::CutInsideFreedPart {
                earlier_op,
                shape,
                freed,
                ..
            } => write!(
                f,
                "{shape} lies inside {freed}, which operation `{earlier_op}` cuts free of the stock first, so this cut would run in a loose part; move this operation before `{earlier_op}`, or hold that part with tabs"
            ),
            Self::ToolChangeAvoidable { tool_id, tool, ops } => {
                let ops = ops
                    .iter()
                    .map(|id| format!("`{id}`"))
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "tool `{tool_id}` (`{tool}`) is used by operations {ops}, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)"
                )
            }
            Self::DrillTargetNotCircle { shape, .. } => write!(
                f,
                "{shape} is not a circle; Drill plunges only at the centre of circles, so it is not drilled"
            ),
            Self::DrillToolWiderThanHole {
                shape,
                hole_mm,
                tool_mm,
                ..
            } => write!(
                f,
                "the {} mm tool is wider than the {} mm hole {shape}, so the hole comes out the tool's size",
                diameter(*tool_mm),
                mm(*hole_mm)
            ),
            Self::DrillPeckDeeperThanHole {
                peck_mm, depth_mm, ..
            } => write!(
                f,
                "the {} mm peck is at least the {} mm depth of the holes, so each is drilled in one plunge",
                mm(*peck_mm),
                mm(*depth_mm)
            ),
            Self::DrillRetractBelowStockTop { requested_mm, .. } => write!(
                f,
                "retract_mm is {} mm, below the stock top; the tool retracts to the stock top instead, because it must never rapid into material",
                mm(*requested_mm)
            ),
            Self::RestOfClearsNothing { rest_of, tool, .. } => write!(
                f,
                "operation `{rest_of}`, which this one rests on, already reaches everything tool `{tool}` can, so this operation cuts nothing; use a smaller tool, or remove it"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Shape` (instance `i1`, profile 2).
    fn shape() -> ShapeRef {
        ShapeRef {
            instance: "i1".into(),
            instance_name: "Shape".to_owned(),
            profile: 2,
        }
    }

    /// Operation `o1`.
    fn op() -> OperationId {
        "o1".into()
    }

    /// One warning for every message form, each with the message it printed
    /// before warnings were typed (#248). The values carry float noise so
    /// the rounding is checked too.
    #[allow(clippy::too_many_lines)] // one entry per message form
    fn message_fixtures() -> Vec<(Warning, &'static str)> {
        vec![
            (
                Warning::OpenPathSkipped {
                    shape: shape(),
                    reason: OpenPathReason::NoSide,
                },
                "`Shape` (instance `i1`, profile 2) is an open path, which has no inside or outside, so it is not cut; an On line Profile or an Engrave cuts along it",
            ),
            (
                Warning::OpenPathSkipped {
                    shape: shape(),
                    reason: OpenPathReason::NoLength,
                },
                "`Shape` (instance `i1`, profile 2) is an open path with no length, so there is nothing to cut; it is skipped",
            ),
            (
                Warning::ToolTooLarge {
                    shape: shape(),
                    tool: "1/4\" upcut".to_owned(),
                    tool_id: "t1".into(),
                    diameter_mm: 6.35,
                },
                "tool `1/4\" upcut` (6.35 mm) does not fit `Shape` (instance `i1`, profile 2), so that shape is not cut",
            ),
            (
                Warning::DuplicateShapeDropped {
                    shape: shape(),
                    duplicate_of: ShapeRef {
                        instance: "i0".into(),
                        instance_name: "Kept".to_owned(),
                        profile: 1,
                    },
                },
                "`Shape` (instance `i1`, profile 2) is the same outline in the same place as instance `i0` profile 1, so it is cut only once",
            ),
            (
                Warning::TabsOverlap {
                    op: op(),
                    contour: 3,
                    shape: Some(shape()),
                    path_length_mm: 99.996_000_1,
                },
                "tabs on `Shape` (instance `i1`, profile 2) touch or overlap on its 100 mm tool path, so they were joined into longer tabs; use fewer or narrower tabs",
            ),
            (
                Warning::TabsOverlap {
                    op: op(),
                    contour: 3,
                    shape: None,
                    path_length_mm: 12.345,
                },
                "tabs on contour 4 in cut order touch or overlap on its 12.35 mm tool path, so they were joined into longer tabs; use fewer or narrower tabs",
            ),
            (
                Warning::TabsOutsideCutRange {
                    op: op(),
                    reason: TabsRangeReason::AboveStockTop,
                    tab_top_mm: 20.000_000_01,
                    deepest_pass_z_mm: 0.0,
                    stock_top_mm: 18.0,
                },
                "the tab top is 20 mm above the stock bottom, higher than the 18 mm stock top; over each tab every pass lifts clear of the stock",
            ),
            (
                Warning::TabsOutsideCutRange {
                    op: op(),
                    reason: TabsRangeReason::CutStopsAboveTab,
                    tab_top_mm: 3.0,
                    deepest_pass_z_mm: 4.499_999,
                    stock_top_mm: 18.0,
                },
                "the tab top is 3 mm above the stock bottom, but the deepest pass stops at 4.5 mm, so the tabs change nothing; lower the tabs or cut deeper",
            ),
            (
                Warning::TabsCoverMostOfContour {
                    op: op(),
                    contour: 0,
                    shape: Some(shape()),
                    path_length_mm: 100.0,
                    tab_count: 4,
                    tab_width_mm: 10.0,
                    diameter_mm: 6.35,
                    tab_span_mm: 16.35,
                    tab_total_mm: 65.4,
                    cut_count: 3,
                    max_width_mm: Some(6.15),
                },
                "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: each of the 4 tabs is 10 mm wide, and with the 6.35 mm bit each one keeps 16.35 mm of the tool path raised, 65.4 mm in total on a 100 mm path (65.4%), so the part or hole would not cut free. Only 3 of them are cut there. To keep all 4, make them at most 6.15 mm wide.",
            ),
            (
                Warning::TabsCoverMostOfContour {
                    op: op(),
                    contour: 1,
                    shape: None,
                    path_length_mm: 30.0,
                    tab_count: 2,
                    tab_width_mm: 5.0,
                    diameter_mm: 8.0,
                    tab_span_mm: 13.0,
                    tab_total_mm: 26.0,
                    cut_count: 1,
                    max_width_mm: None,
                },
                "tabs on contour 2 in cut order would cover more than half of its tool path: each of the 2 tabs is 5 mm wide, and with the 8 mm bit each one keeps 13 mm of the tool path raised, 26 mm in total on a 30 mm path (86.7%), so the part or hole would not cut free. Only 1 of them is cut there.",
            ),
            (
                Warning::TabsCoverMostOfContour {
                    op: op(),
                    contour: 0,
                    shape: Some(shape()),
                    path_length_mm: 10.0,
                    tab_count: 1,
                    tab_width_mm: 3.0,
                    diameter_mm: 3.175,
                    tab_span_mm: 6.175,
                    tab_total_mm: 6.175,
                    cut_count: 0,
                    max_width_mm: None,
                },
                "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: its one tab is 3 mm wide, and with the 3.175 mm bit it keeps 6.17 mm of the tool path raised on a 10 mm path (61.8%), so the part or hole would not cut free. No tab is cut there: this outline is too short for tabs with this bit.",
            ),
            (
                Warning::TabsCoverMostOfContour {
                    op: op(),
                    contour: 0,
                    shape: Some(shape()),
                    path_length_mm: 6.5,
                    tab_count: 1,
                    tab_width_mm: 3.0,
                    diameter_mm: 1.5,
                    tab_span_mm: 4.5,
                    tab_total_mm: 4.5,
                    cut_count: 0,
                    max_width_mm: Some(1.75),
                },
                "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: its one tab is 3 mm wide, and with the 1.5 mm bit it keeps 4.5 mm of the tool path raised on a 6.5 mm path (69.2%), so the part or hole would not cut free. No tab is cut there. To keep it, make it at most 1.75 mm wide.",
            ),
            (
                Warning::TabsCoverMostOfContour {
                    op: op(),
                    contour: 0,
                    shape: Some(shape()),
                    path_length_mm: 20.0,
                    tab_count: 1,
                    tab_width_mm: 8.0,
                    diameter_mm: 6.35,
                    tab_span_mm: 14.35,
                    tab_total_mm: 14.35,
                    cut_count: 1,
                    max_width_mm: Some(3.65),
                },
                "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: its one tab is 8 mm wide, and with the 6.35 mm bit it keeps 14.35 mm of the tool path raised on a 20 mm path (71.8%), so the part or hole would not cut free. Its one tab is kept, as a part keeps at least one, so the part may need cutting free by hand. To let it cut free, make it at most 3.65 mm wide.",
            ),
            (
                Warning::TabsCoverMostOfContour {
                    op: op(),
                    contour: 0,
                    shape: Some(shape()),
                    path_length_mm: 20.0,
                    tab_count: 4,
                    tab_width_mm: 8.0,
                    diameter_mm: 6.35,
                    tab_span_mm: 14.35,
                    tab_total_mm: 57.4,
                    cut_count: 1,
                    max_width_mm: None,
                },
                "tabs on `Shape` (instance `i1`, profile 2) would cover more than half of its tool path: each of the 4 tabs is 8 mm wide, and with the 6.35 mm bit each one keeps 14.35 mm of the tool path raised, 57.4 mm in total on a 20 mm path (287%), so the part or hole would not cut free. Only 1 of them is cut there, as a part keeps at least one, so the part may need cutting free by hand.",
            ),
            (
                Warning::ToolpathsOverlap {
                    shape: shape(),
                    other: ShapeRef {
                        instance: "i9".into(),
                        instance_name: "Other".to_owned(),
                        profile: 0,
                    },
                    gap_mm: 1.004_999,
                    tool: "3 mm".to_owned(),
                    tool_id: "t1".into(),
                    diameter_mm: 3.0,
                    needed_mm: 1.5,
                },
                "the tool paths of `Shape` (instance `i1`, profile 2) and `Other` (instance `i9`, profile 0) cut into each other: the shapes are 1 mm apart, closer than the 1.5 mm tool `3 mm` (3 mm) needs between them, so cutting one gouges the other. Move them at least 1.5 mm apart or use a smaller bit.",
            ),
            (
                Warning::PocketNoRoom {
                    op: op(),
                    shape: shape(),
                    tool: "1/8\"".to_owned(),
                    tool_id: "t1".into(),
                    diameter_mm: 3.175,
                },
                "tool `1/8\"` (3.175 mm) is wider than the pocket of `Shape` (instance `i1`, profile 2) somewhere it must pass, so that shape is not cut; use a smaller bit",
            ),
            (
                Warning::PocketIslandTooClose {
                    op: op(),
                    shape: shape(),
                    island: 3,
                    other: IslandNeighbour::Wall,
                    gap_mm: 4.999_9,
                    needed_mm: 6.35,
                    allowance_mm: 0.0,
                    tool: "t".to_owned(),
                    tool_id: "t1".into(),
                    diameter_mm: 6.35,
                },
                "in the pocket of `Shape` (instance `i1`, profile 2), island profile 3 is 5 mm from the pocket wall, closer than the 6.35 mm tool `t` is wide, so the material between them is left uncut",
            ),
            (
                Warning::PocketIslandTooClose {
                    op: op(),
                    shape: shape(),
                    island: 3,
                    other: IslandNeighbour::Island(4),
                    gap_mm: 7.0,
                    needed_mm: 7.350_000_1,
                    allowance_mm: 0.5,
                    tool: "t".to_owned(),
                    tool_id: "t1".into(),
                    diameter_mm: 6.35,
                },
                "in the pocket of `Shape` (instance `i1`, profile 2), island profile 3 is 7 mm from island profile 4, closer than tool `t` needs with a 0.5 mm allowance (7.35 mm), so the material between them is left uncut",
            ),
            (
                Warning::EntryFallback {
                    op: op(),
                    shape: shape(),
                    requested: EntryKind::Helix,
                    used: EntryKind::Ramp,
                    reason: FallbackReason::NoRoom,
                },
                "there is no room for a `helix` entry in `Shape` (instance `i1`, profile 2), so its passes start with a zigzag ramp instead",
            ),
            (
                Warning::EntryFallback {
                    op: op(),
                    shape: shape(),
                    requested: EntryKind::Ramp,
                    used: EntryKind::Plunge,
                    reason: FallbackReason::TooManyMoves,
                },
                "a `ramp` entry at this angle would take too many moves to reach depth in `Shape` (instance `i1`, profile 2), so its passes start with a straight plunge at the plunge rate instead; set the operation's entry to `plunge` to stop this warning",
            ),
            (
                Warning::HelixDiameterReduced {
                    op: op(),
                    shape: shape(),
                    requested_mm: 5.08,
                    used_mm: 2.539_999_9,
                },
                "the helix entry in `Shape` (instance `i1`, profile 2) is 2.54 mm across, smaller than the 5.08 mm asked for, because a larger one does not fit there",
            ),
            (
                Warning::LeadShrunk {
                    op: op(),
                    shape: shape(),
                    requested_mm: 30.0,
                    used_mm: 22.499_999,
                    requested_sweep_deg: 90.0,
                    used_sweep_deg: 44.6,
                },
                "the lead arc on `Shape` (instance `i1`, profile 2) is 22.5 mm in radius over 45°, smaller than the 30 mm over 90° asked for, because a larger one does not fit there",
            ),
            (
                Warning::LeadDropped {
                    op: op(),
                    shape: Some(shape()),
                    reason: LeadDroppedReason::NoRoom,
                },
                "there is no room for a lead-in arc on `Shape` (instance `i1`, profile 2), even at a quarter of the radius, so its passes start on the outline without one",
            ),
            (
                Warning::LeadDropped {
                    op: op(),
                    shape: Some(shape()),
                    reason: LeadDroppedReason::Exit,
                },
                "there is no room for a lead-out arc where the cut on `Shape` (instance `i1`, profile 2) ends, so it stops on the outline",
            ),
            (
                Warning::LeadDropped {
                    op: op(),
                    shape: None,
                    reason: LeadDroppedReason::OnLine,
                },
                "operation `o1` cuts on the line, which has no waste side for a lead, so it cuts without one; remove the lead or cut Inside or Outside",
            ),
            (
                Warning::ReliefSkipped {
                    op: op(),
                    requested: CornerRelief::Dogbone,
                    reason: ReliefSkippedReason::NoStepDown,
                },
                "corner relief `dogbone` is skipped because the tool has no step_down_mm, which the relief pass steps down at, so no corners were relieved; set the tool's step-down",
            ),
            (
                Warning::ReliefSkipped {
                    op: op(),
                    requested: CornerRelief::TBone,
                    reason: ReliefSkippedReason::OnLine,
                },
                "corner relief `t_bone` is skipped: an On line Profile has no waste side to cut a notch into, so no corners were relieved",
            ),
            (
                Warning::ReliefCornerTooTight {
                    op: op(),
                    shape: shape(),
                    x_mm: 10.004,
                    y_mm: -0.001,
                    reason: ReliefCornerReason::ShortEdge,
                },
                "the corner of `Shape` (instance `i1`, profile 2) at (10, 0) mm is not relieved: an edge beside it is shorter than the bit is wide, so it keeps the bit's radius",
            ),
            (
                Warning::ReliefCornerTooTight {
                    op: op(),
                    shape: shape(),
                    x_mm: 1.5,
                    y_mm: 2.25,
                    reason: ReliefCornerReason::NoWallPath,
                },
                "the corner of `Shape` (instance `i1`, profile 2) at (1.5, 2.25) mm is not relieved: the tool path does not turn it within reach, so it keeps the bit's radius",
            ),
            (
                Warning::ReliefCornerNotCut {
                    op: op(),
                    shape: shape(),
                    x_mm: -3.0,
                    y_mm: 40.125,
                },
                "the corner of `Shape` (instance `i1`, profile 2) at (-3, 40.12) mm is not relieved because the main passes did not cut to full depth where its notch starts (a tab over it, or no wall pass), and a notch must not plunge into uncut material; it keeps the bit's radius",
            ),
            (
                Warning::EngravePathDegenerate {
                    op: op(),
                    shape: shape(),
                },
                "`Shape` (instance `i1`, profile 2) has fewer than two distinct points, so there is nothing to engrave; it is skipped",
            ),
            (
                Warning::CutInsideFreedPart {
                    op: op(),
                    earlier_op: "o0".into(),
                    shape: shape(),
                    freed: ShapeRef {
                        instance: "i0".into(),
                        instance_name: "Outline".to_owned(),
                        profile: 0,
                    },
                },
                "`Shape` (instance `i1`, profile 2) lies inside `Outline` (instance `i0`, profile 0), which operation `o0` cuts free of the stock first, so this cut would run in a loose part; move this operation before `o0`, or hold that part with tabs",
            ),
            (
                Warning::ToolChangeAvoidable {
                    tool_id: "t1".into(),
                    tool: "6.35 mm upcut".to_owned(),
                    ops: vec!["o1".into(), "o3".into()],
                },
                "tool `t1` (`6.35 mm upcut`) is used by operations `o1`, `o3`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)",
            ),
            (
                Warning::DrillTargetNotCircle {
                    op: op(),
                    shape: shape(),
                },
                "`Shape` (instance `i1`, profile 2) is not a circle; Drill plunges only at the centre of circles, so it is not drilled",
            ),
            (
                Warning::DrillToolWiderThanHole {
                    op: op(),
                    shape: shape(),
                    hole_mm: 4.999_999_9,
                    tool_mm: 6.35,
                },
                "the 6.35 mm tool is wider than the 5 mm hole `Shape` (instance `i1`, profile 2), so the hole comes out the tool's size",
            ),
            (
                Warning::DrillPeckDeeperThanHole {
                    op: op(),
                    peck_mm: 20.0,
                    depth_mm: 12.000_000_1,
                },
                "the 20 mm peck is at least the 12 mm depth of the holes, so each is drilled in one plunge",
            ),
            (
                Warning::DrillRetractBelowStockTop {
                    op: op(),
                    requested_mm: -2.5,
                    used_mm: 0.0,
                },
                "retract_mm is -2.5 mm, below the stock top; the tool retracts to the stock top instead, because it must never rapid into material",
            ),
            (
                Warning::RestOfClearsNothing {
                    op: "o2".into(),
                    rest_of: "o1".into(),
                    tool: "12.7 mm end mill".to_owned(),
                    tool_id: "t2".into(),
                },
                "operation `o1`, which this one rests on, already reaches everything tool `12.7 mm end mill` can, so this operation cuts nothing; use a smaller tool, or remove it",
            ),
        ]
    }

    #[test]
    fn warning_messages_are_unchanged() {
        for (warning, message) in message_fixtures() {
            assert_eq!(warning.to_string(), message, "{warning:?}");
        }
    }

    /// The code after `code`, in declaration order. A match, so a new code
    /// does not compile until it is listed here.
    const fn next_code(code: WarningCode) -> Option<WarningCode> {
        use WarningCode as C;
        Some(match code {
            C::OpenPathSkipped => C::ToolTooLarge,
            C::ToolTooLarge => C::DuplicateShapeDropped,
            C::DuplicateShapeDropped => C::TabsOverlap,
            C::TabsOverlap => C::TabsOutsideCutRange,
            C::TabsOutsideCutRange => C::TabsCoverMostOfContour,
            C::TabsCoverMostOfContour => C::ToolpathsOverlap,
            C::ToolpathsOverlap => C::PocketNoRoom,
            C::PocketNoRoom => C::PocketIslandTooClose,
            C::PocketIslandTooClose => C::EntryFallback,
            C::EntryFallback => C::HelixDiameterReduced,
            C::HelixDiameterReduced => C::LeadShrunk,
            C::LeadShrunk => C::LeadDropped,
            C::LeadDropped => C::ReliefSkipped,
            C::ReliefSkipped => C::ReliefCornerTooTight,
            C::ReliefCornerTooTight => C::ReliefCornerNotCut,
            C::ReliefCornerNotCut => C::EngravePathDegenerate,
            C::EngravePathDegenerate => C::CutInsideFreedPart,
            C::CutInsideFreedPart => C::ToolChangeAvoidable,
            C::ToolChangeAvoidable => C::DrillTargetNotCircle,
            C::DrillTargetNotCircle => C::DrillToolWiderThanHole,
            C::DrillToolWiderThanHole => C::DrillPeckDeeperThanHole,
            C::DrillPeckDeeperThanHole => C::DrillRetractBelowStockTop,
            C::DrillRetractBelowStockTop => C::RestOfClearsNothing,
            C::RestOfClearsNothing => return None,
        })
    }

    #[test]
    fn every_warning_code_has_a_variant() {
        let chapter = include_str!("../../../docs/book/src/user-guide/warnings.md");
        let fixtures = message_fixtures();
        let mut code = Some(WarningCode::OpenPathSkipped);
        let mut count = 0;
        while let Some(c) = code {
            assert!(
                fixtures.iter().any(|(w, _)| w.code() == c),
                "no variant has code `{c}`"
            );
            // The warnings that name an operation return it from `op`.
            let names_op = !matches!(
                c,
                WarningCode::OpenPathSkipped
                    | WarningCode::ToolTooLarge
                    | WarningCode::DuplicateShapeDropped
                    | WarningCode::ToolpathsOverlap
                    | WarningCode::ToolChangeAvoidable
            );
            for (w, _) in fixtures.iter().filter(|(w, _)| w.code() == c) {
                assert_eq!(w.op().is_some(), names_op, "{w:?}");
            }
            assert!(
                chapter.contains(&format!("| `{c}` |")),
                "the Warnings chapter does not list `{c}`"
            );
            count += 1;
            code = next_code(c);
        }
        assert_eq!(count, 24);
    }

    #[test]
    fn rounded_drops_noise_and_trailing_zeros() {
        assert_eq!(rounded(54.600_002_225_240_075, 2), "54.6");
        assert_eq!(rounded(14.35, 2), "14.35");
        assert_eq!(rounded(12.0, 2), "12");
        assert_eq!(rounded(-0.001, 2), "0");
        assert_eq!(rounded(105.128, 1), "105.1");
    }
}
