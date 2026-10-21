// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Non-fatal findings of toolpath generation (`docs/specs/phase-1/design.md`,
//! "dc-cam"). A warning still yields a safe toolpath; anything that would
//! make motion wrong is a [`crate::CamError`] instead.

use std::collections::BTreeMap;
use std::fmt;

/// What a [`Warning`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum WarningCode {
    /// A target shape is an open path, which a Pocket and an Inside or
    /// Outside Profile cannot cut, or an open path with no length on an On
    /// line Profile (#175); it was skipped. Params: `instance`,
    /// `instance_name`, `profile`, and `reason: no_length` for the latter.
    OpenPathSkipped,
    /// The tool does not fit: the tool-centre offset of a shape vanished,
    /// so that shape was not cut. Params: `instance`, `instance_name`,
    /// `profile`, `tool`, `diameter_mm`.
    ToolTooLarge,
    /// A target shape duplicates an earlier one and was dropped so it is
    /// not cut twice. Params: `instance`, `instance_name`, `profile`,
    /// `duplicate_of_instance`, `duplicate_of_profile`.
    DuplicateShapeDropped,
    /// Two or more tab spans on one tool-centre contour overlap, so they
    /// were merged into one raised span. Params: `op`, `contour` (0-based,
    /// in cut order), `instance`, `instance_name`, `profile`,
    /// `path_length_mm`.
    TabsOverlap,
    /// The tab top lies outside the range the cut passes through: no pass
    /// goes below it (`reason` `cut_stops_above_tab`; the tabs change
    /// nothing), or it is above the stock top (`reason`
    /// `above_stock_top`; the tabs still apply). Params: `op`, `reason`,
    /// `tab_top_mm`, `deepest_pass_z_mm`, `stock_top_mm`.
    TabsOutsideCutRange,
    /// The tab spans on one tool-centre contour cover more than half of its
    /// length, so the part or hole may not cut free. The tabs are cut as
    /// set. Params: `op`, `contour` (0-based, in cut order), `instance`,
    /// `instance_name`, `profile`, `path_length_mm`, `tab_count`,
    /// `tab_width_mm` (the tab width as set), `diameter_mm` (the tool
    /// diameter), `tab_span_mm` (one tab's length of path: width plus tool
    /// diameter),
    /// `tab_total_mm` (count times span), `max_count` (most tabs of this
    /// width that cover at most half), and `max_width_mm` (widest tab at
    /// this count that covers at most half; absent when none fits).
    TabsCoverMostOfContour,
    /// The tool path of one shape cuts into material another shape in the
    /// same operation keeps. Params: `instance`, `instance_name`,
    /// `profile` (the shape cut first), `other_instance`,
    /// `other_instance_name`, `other_profile`, `gap_mm` (the smallest
    /// distance between the two shapes' lines), `tool`, `diameter_mm`, and
    /// `needed_mm` (the gap this side needs: the diameter for Outside, the
    /// radius for On line). The shapes are cut as drawn.
    ToolpathsOverlap,
    /// The tool is wider than a pocket region somewhere it must pass: the
    /// tool-centre region of that shape is empty, so the shape is not cut.
    /// Params: `op`, `instance`, `instance_name`, `profile`, `tool`,
    /// `diameter_mm`.
    PocketNoRoom,
    /// Two of a pocket's boundaries, an island and the wall or two
    /// islands, are closer than the tool needs to pass between them (its
    /// diameter plus twice the allowance), so the material between them
    /// stays. Params: `op`, `instance`, `instance_name`, `profile` (the
    /// pocket's outline), `island` (the island's profile index), `other`
    /// (`wall`, or the other island's profile index), `gap_mm`,
    /// `needed_mm` (`diameter_mm + 2 × allowance_mm`), `allowance_mm`,
    /// `tool`, `diameter_mm`.
    PocketIslandTooClose,
    /// The entry asked for did not fit a shape, so its passes start with
    /// the next one in the chain helix → ramp → plunge
    /// (`docs/specs/phase-2/design.md`, "Algorithms → Entry"). Params:
    /// `op`, `instance`, `instance_name`, `profile`, `requested` (`ramp`
    /// or `helix`), `used` (`ramp` or `plunge`), `reason` (`no_room`: the
    /// shape leaves no room for it; `too_many_moves`: at this angle and
    /// depth it would need more than the descent move limit).
    EntryFallback,
    /// A helix entry fits a shape only smaller than asked, so it is cut at
    /// the largest diameter that fits. Params: `op`, `instance`,
    /// `instance_name`, `profile`, `requested_mm`, `used_mm` (the smallest
    /// helix diameter cut in that shape).
    HelixDiameterReduced,
    /// A lead-in or lead-out arc fits a shape only smaller than asked, so
    /// it is cut at the largest radius and widest sweep that fit
    /// (`docs/specs/phase-2/design.md`, "Algorithms → Lead-in and
    /// lead-out"). Params: `op`, `instance`, `instance_name`, `profile`,
    /// `requested_mm`, `used_mm` (the smallest radius cut on that shape),
    /// `requested_sweep_deg`, `used_sweep_deg` (the smallest sweep cut).
    LeadShrunk,
    /// A lead asked for is not cut. Params: `op`, `reason`; for `no_room`
    /// and `exit` also `instance`, `instance_name`, `profile`. `no_room`:
    /// no lead-in fits the shape even at a quarter of the radius, so its
    /// passes start on the outline (below the tab top only, when only the
    /// tabs are in the way); `exit`: no lead-out fits where the cut ends,
    /// so it stops on the outline; `on_line`: an On line Profile has no
    /// waste side for a lead, so the operation cuts without one.
    LeadDropped,
    /// The operation's corner relief was not cut at all
    /// (`docs/specs/phase-2/design.md`, "Algorithms → Corner relief").
    /// Params: `op`, `requested` (`dogbone` or `t_bone`), `reason`
    /// (`no_step_down`: the tool has no step-down, which the relief pass
    /// steps down at; `on_line`: an On line Profile has no waste side to
    /// put a notch in).
    ReliefSkipped,
    /// A corner of the cleared area is not relieved because the notch
    /// does not fit it: an edge beside it is shorter than the tool is
    /// wide (`reason` `short_edge`), or the tool-centre path does not
    /// turn it within reach (`reason` `no_wall_path`). Params: `op`,
    /// `instance`, `instance_name`, `profile`, `x_mm`, `y_mm` (the
    /// corner), `reason`.
    ReliefCornerTooTight,
    /// A corner of the cleared area is not relieved because the main
    /// passes did not cut to full depth where the notch would start (a
    /// tab over it, or no wall pass), so the notch would plunge into
    /// uncut material. Params: `op`, `instance`, `instance_name`,
    /// `profile`, `x_mm`, `y_mm` (the corner).
    ReliefCornerNotCut,
    /// An Engrave target path has fewer than two distinct points, so there
    /// is nothing to trace; it was skipped. Params: `op`, `instance`,
    /// `instance_name`, `profile`.
    EngravePathDegenerate,
    /// A closed target of this operation lies inside an outline an earlier
    /// operation cuts free of the stock, so the cut would run in a loose
    /// part (`docs/specs/phase-2/design.md`, "Operation-order warnings";
    /// research O8). Params: `op`, `earlier_op`, `instance`,
    /// `instance_name`, `profile` (the shape this operation cuts),
    /// `freed_instance`, `freed_instance_name`, `freed_profile` (the
    /// outline the earlier operation frees).
    CutInsideFreedPart,
    /// A tool is used by operations that do not run one after another, so
    /// it is fitted more than once. Operations are never reordered (D7);
    /// this is a hint. Params: `tool`, `tool_name`, `ops` (the ids of
    /// every enabled operation using the tool, comma separated, in order).
    ToolChangeAvoidable,
    /// A Drill target is not a circle (or is an open path), so it was
    /// skipped. Params: `op`, `instance`, `instance_name`, `profile`.
    DrillTargetNotCircle,
    /// The drilling tool is wider than the hole it drills; the hole comes
    /// out the tool's size. Params: `op`, `instance`, `instance_name`,
    /// `profile`, `hole_mm`, `tool_mm`.
    DrillToolWiderThanHole,
    /// The peck depth is at least the depth of the operation's holes, so
    /// each is drilled in one plunge. Reported once per operation: every
    /// hole has the same depth. Params: `op`, `peck_mm`, `depth_mm`.
    DrillPeckDeeperThanHole,
    /// The retract height is below the stock top, so it was raised to the
    /// top: the tool must never rapid into material. Params: `op`,
    /// `requested_mm` (the height above the stock top as set, negative),
    /// `used_mm` (0).
    DrillRetractBelowStockTop,
    /// A rest Pocket's tool reaches nothing the Pocket it rests on left
    /// (`docs/specs/phase-2/design.md`, "Algorithms → Pocket", step 3), so
    /// it emits no toolpath. Params: `op`, `rest_of` (the reference
    /// operation), `tool` (this operation's tool name).
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

/// Something the user should know about that still yields a safe toolpath.
/// `params` names the things involved, keyed as documented on each
/// [`WarningCode`]; ordered so output is deterministic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// What happened.
    pub code: WarningCode,
    /// The details, by name.
    pub params: BTreeMap<String, String>,
}

impl Warning {
    /// A warning with no params.
    #[must_use]
    pub fn new(code: WarningCode) -> Self {
        Self {
            code,
            params: BTreeMap::new(),
        }
    }

    /// This warning with `key` set to `value`.
    #[must_use]
    pub fn with(mut self, key: &str, value: impl ToString) -> Self {
        self.params.insert(key.to_owned(), value.to_string());
        self
    }

    /// The value of param `key`, if set.
    #[must_use]
    pub fn param(&self, key: &str) -> Option<&str> {
        self.params.get(key).map(String::as_str)
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

impl Warning {
    /// Param `key`, or `?` when unset.
    fn text(&self, key: &str) -> &str {
        self.param(key).unwrap_or("?")
    }

    /// Param `key` as a number rounded to `decimals` places; unchanged when
    /// it is not a number.
    fn number(&self, key: &str, decimals: usize) -> String {
        let text = self.text(key);
        text.parse::<f64>()
            .map_or_else(|_| text.to_owned(), |v| rounded(v, decimals))
    }

    /// Param `key` as a length, rounded for display.
    fn mm(&self, key: &str) -> String {
        self.number(key, MM_DECIMALS)
    }

    /// The shape the warning is about, as the user knows it.
    fn shape(&self) -> String {
        match (self.param("instance_name"), self.param("instance")) {
            (Some(name), Some(instance)) => format!(
                "`{name}` (instance `{instance}`, profile {})",
                self.text("profile")
            ),
            _ => match self.param("contour").and_then(|c| c.parse::<usize>().ok()) {
                Some(c) => format!("contour {} in cut order", c + 1),
                None => "a shape".to_owned(),
            },
        }
    }

    /// The second shape a two-shape warning names, from its `other_*`
    /// params.
    fn other_shape(&self) -> String {
        format!(
            "`{}` (instance `{}`, profile {})",
            self.text("other_instance_name"),
            self.text("other_instance"),
            self.text("other_profile")
        )
    }

    /// The suggestion for tabs that cover most of a contour.
    fn coverage_advice(&self) -> String {
        let count = self.text("tab_count");
        let max_count = self.number("max_count", 0);
        let fewer = if max_count == "0" {
            "even one tab this wide covers more than half".to_owned()
        } else {
            format!("use at most {max_count} tab(s) this wide")
        };
        match self.param("max_width_mm") {
            Some(_) => format!(
                "To let it cut free, {fewer}, or make the {count} tab(s) at most {} mm wide.",
                self.mm("max_width_mm")
            ),
            None if max_count == "0" => format!(
                "To let it cut free, {fewer}: this outline is too short for tabs with this bit, so consider removing them."
            ),
            None => format!("To let it cut free, {fewer}."),
        }
    }
}

/// A plain sentence, values rounded for display. The operation is not
/// named; the caller says which operation the warning came from.
impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code {
            WarningCode::OpenPathSkipped if self.param("reason") == Some("no_length") => write!(
                f,
                "{} is an open path with no length, so there is nothing to cut; it is skipped",
                self.shape()
            ),
            WarningCode::OpenPathSkipped => write!(
                f,
                "{} is an open path, which has no inside or outside, so it is not cut; an On line Profile or an Engrave cuts along it",
                self.shape()
            ),
            WarningCode::ToolTooLarge => write!(
                f,
                "tool `{}` ({} mm) does not fit {}, so that shape is not cut",
                self.text("tool"),
                self.mm("diameter_mm"),
                self.shape()
            ),
            WarningCode::DuplicateShapeDropped => write!(
                f,
                "{} is the same outline in the same place as instance `{}` profile {}, so it is cut only once",
                self.shape(),
                self.text("duplicate_of_instance"),
                self.text("duplicate_of_profile")
            ),
            WarningCode::TabsOverlap => write!(
                f,
                "tabs on {} touch or overlap on its {} mm tool path, so they were joined into longer tabs; use fewer or narrower tabs",
                self.shape(),
                self.mm("path_length_mm")
            ),
            WarningCode::TabsOutsideCutRange => match self.param("reason") {
                Some("above_stock_top") => write!(
                    f,
                    "the tab top is {} mm above the stock bottom, higher than the {} mm stock top; over each tab every pass lifts clear of the stock",
                    self.mm("tab_top_mm"),
                    self.mm("stock_top_mm")
                ),
                _ => write!(
                    f,
                    "the tab top is {} mm above the stock bottom, but the deepest pass stops at {} mm, so the tabs change nothing; lower the tabs or cut deeper",
                    self.mm("tab_top_mm"),
                    self.mm("deepest_pass_z_mm")
                ),
            },
            WarningCode::ToolpathsOverlap => write!(
                f,
                "the tool paths of {} and {} cut into each other: the shapes are {} mm apart, closer than the {} mm tool `{}` ({} mm) needs between them, so cutting one gouges the other. Move them at least {} mm apart or use a smaller bit.",
                self.shape(),
                self.other_shape(),
                self.mm("gap_mm"),
                self.mm("needed_mm"),
                self.text("tool"),
                self.mm("diameter_mm"),
                self.mm("needed_mm")
            ),
            WarningCode::PocketNoRoom => write!(
                f,
                "tool `{}` ({} mm) is wider than the pocket of {} somewhere it must pass, so that shape is not cut; use a smaller bit",
                self.text("tool"),
                self.number("diameter_mm", DIAMETER_DECIMALS),
                self.shape()
            ),
            WarningCode::PocketIslandTooClose => {
                let other = match self.param("other") {
                    Some("wall") => "the pocket wall".to_owned(),
                    Some(index) => format!("island profile {index}"),
                    None => "another boundary".to_owned(),
                };
                let allowance = self
                    .param("allowance_mm")
                    .and_then(|a| a.parse::<f64>().ok())
                    .filter(|&a| a > 0.0);
                let needs = match allowance {
                    Some(_) => format!(
                        "tool `{}` needs with a {} mm allowance ({} mm)",
                        self.text("tool"),
                        self.mm("allowance_mm"),
                        self.number("needed_mm", DIAMETER_DECIMALS)
                    ),
                    None => format!(
                        "the {} mm tool `{}` is wide",
                        self.number("diameter_mm", DIAMETER_DECIMALS),
                        self.text("tool")
                    ),
                };
                write!(
                    f,
                    "in the pocket of {}, island profile {} is {} mm from {other}, closer than {needs}, so the material between them is left uncut",
                    self.shape(),
                    self.text("island"),
                    self.mm("gap_mm"),
                )
            }
            WarningCode::EntryFallback => {
                let requested = self.text("requested");
                let why = match self.param("reason") {
                    Some("too_many_moves") => format!(
                        "a `{requested}` entry at this angle would take too many moves to reach depth in"
                    ),
                    _ => format!("there is no room for a `{requested}` entry in"),
                };
                let (used, advice) = match self.param("used") {
                    Some("ramp") => ("a zigzag ramp", ""),
                    _ => (
                        "a straight plunge at the plunge rate",
                        "; set the operation's entry to `plunge` to stop this warning",
                    ),
                };
                write!(
                    f,
                    "{why} {}, so its passes start with {used} instead{advice}",
                    self.shape()
                )
            }
            WarningCode::HelixDiameterReduced => write!(
                f,
                "the helix entry in {} is {} mm across, smaller than the {} mm asked for, because a larger one does not fit there",
                self.shape(),
                self.number("used_mm", DIAMETER_DECIMALS),
                self.number("requested_mm", DIAMETER_DECIMALS)
            ),
            WarningCode::LeadShrunk => write!(
                f,
                "the lead arc on {} is {} mm in radius over {}°, smaller than the {} mm over {}° asked for, because a larger one does not fit there",
                self.shape(),
                self.mm("used_mm"),
                self.number("used_sweep_deg", 0),
                self.mm("requested_mm"),
                self.number("requested_sweep_deg", 0)
            ),
            WarningCode::LeadDropped => match self.param("reason") {
                Some("on_line") => write!(
                    f,
                    "operation `{}` cuts on the line, which has no waste side for a lead, so it cuts without one; remove the lead or cut Inside or Outside",
                    self.text("op")
                ),
                Some("exit") => write!(
                    f,
                    "there is no room for a lead-out arc where the cut on {} ends, so it stops on the outline",
                    self.shape()
                ),
                _ => write!(
                    f,
                    "there is no room for a lead-in arc on {}, even at a quarter of the radius, so its passes start on the outline without one",
                    self.shape()
                ),
            },
            WarningCode::ReliefSkipped => match self.param("reason") {
                Some("on_line") => write!(
                    f,
                    "corner relief `{}` is skipped: an On line Profile has no waste side to cut a notch into, so no corners were relieved",
                    self.text("requested")
                ),
                _ => write!(
                    f,
                    "corner relief `{}` is skipped because the tool has no step_down_mm, which the relief pass steps down at, so no corners were relieved; set the tool's step-down",
                    self.text("requested")
                ),
            },
            WarningCode::ReliefCornerTooTight => {
                let why = match self.param("reason") {
                    Some("no_wall_path") => "the tool path does not turn it within reach",
                    _ => "an edge beside it is shorter than the bit is wide",
                };
                write!(
                    f,
                    "the corner of {} at ({}, {}) mm is not relieved: {why}, so it keeps the bit's radius",
                    self.shape(),
                    self.mm("x_mm"),
                    self.mm("y_mm")
                )
            }
            WarningCode::ReliefCornerNotCut => write!(
                f,
                "the corner of {} at ({}, {}) mm is not relieved because the main passes did not cut to full depth where its notch starts (a tab over it, or no wall pass), and a notch must not plunge into uncut material; it keeps the bit's radius",
                self.shape(),
                self.mm("x_mm"),
                self.mm("y_mm")
            ),
            WarningCode::EngravePathDegenerate => write!(
                f,
                "{} has fewer than two distinct points, so there is nothing to engrave; it is skipped",
                self.shape()
            ),
            WarningCode::CutInsideFreedPart => write!(
                f,
                "{} lies inside `{}` (instance `{}`, profile {}), which operation `{}` cuts free of the stock first, so this cut would run in a loose part; move this operation before `{}`, or hold that part with tabs",
                self.shape(),
                self.text("freed_instance_name"),
                self.text("freed_instance"),
                self.text("freed_profile"),
                self.text("earlier_op"),
                self.text("earlier_op")
            ),
            WarningCode::ToolChangeAvoidable => write!(
                f,
                "tool `{}` (`{}`) is used by operations {}, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)",
                self.text("tool"),
                self.text("tool_name"),
                self.text("ops")
            ),
            WarningCode::DrillTargetNotCircle => write!(
                f,
                "{} is not a circle; Drill plunges only at the centre of circles, so it is not drilled",
                self.shape()
            ),
            WarningCode::DrillToolWiderThanHole => write!(
                f,
                "the {} mm tool is wider than the {} mm hole {}, so the hole comes out the tool's size",
                self.number("tool_mm", DIAMETER_DECIMALS),
                self.mm("hole_mm"),
                self.shape()
            ),
            WarningCode::DrillPeckDeeperThanHole => write!(
                f,
                "the {} mm peck is at least the {} mm depth of the holes, so each is drilled in one plunge",
                self.mm("peck_mm"),
                self.mm("depth_mm")
            ),
            WarningCode::DrillRetractBelowStockTop => write!(
                f,
                "retract_mm is {} mm, below the stock top; the tool retracts to the stock top instead, because it must never rapid into material",
                self.mm("requested_mm")
            ),
            WarningCode::RestOfClearsNothing => write!(
                f,
                "operation `{}`, which this one rests on, already reaches everything tool `{}` can, so this operation cuts nothing; use a smaller tool, or remove it",
                self.text("rest_of"),
                self.text("tool")
            ),
            WarningCode::TabsCoverMostOfContour => {
                let path_mm = self
                    .param("path_length_mm")
                    .and_then(|v| v.parse::<f64>().ok());
                let total_mm = self
                    .param("tab_total_mm")
                    .and_then(|v| v.parse::<f64>().ok());
                let share = match (path_mm, total_mm) {
                    (Some(p), Some(t)) if p > 0.0 => {
                        format!(" ({}%)", rounded(100.0 * t / p, PERCENT_DECIMALS))
                    }
                    _ => String::new(),
                };
                let width = self.mm("tab_width_mm");
                let diameter = self.number("diameter_mm", DIAMETER_DECIMALS);
                let span = self.mm("tab_span_mm");
                let path = self.mm("path_length_mm");
                let tabs = match self.text("tab_count") {
                    "1" => format!(
                        "its one tab is {width} mm wide, and with the {diameter} mm bit it keeps {span} mm of the tool path raised on a {path} mm path{share}"
                    ),
                    count => format!(
                        "each of the {count} tabs is {width} mm wide, and with the {diameter} mm bit each one keeps {span} mm of the tool path raised, {} mm in total on a {path} mm path{share}",
                        self.mm("tab_total_mm")
                    ),
                };
                write!(
                    f,
                    "tabs on {} cover more than half of its tool path: {tabs}, so the part or hole may not cut free. {} The tabs are cut as set.",
                    self.shape(),
                    self.coverage_advice()
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `w` about `Shape` (instance `i1`, profile 2) of operation `o1`.
    fn on_shape(w: Warning) -> Warning {
        w.with("op", "o1")
            .with("instance", "i1")
            .with("instance_name", "Shape")
            .with("profile", 2)
    }

    /// One warning for every message form, each with the message it printed
    /// before warnings were typed (#248). The values carry float noise so
    /// the rounding is checked too.
    fn message_fixtures() -> Vec<(Warning, &'static str)> {
        vec![
            (
                on_shape(Warning::new(WarningCode::OpenPathSkipped)),
                "`Shape` (instance `i1`, profile 2) is an open path, which has no inside or outside, so it is not cut; an On line Profile or an Engrave cuts along it",
            ),
            (
                on_shape(Warning::new(WarningCode::OpenPathSkipped)).with("reason", "no_length"),
                "`Shape` (instance `i1`, profile 2) is an open path with no length, so there is nothing to cut; it is skipped",
            ),
            (
                on_shape(Warning::new(WarningCode::ToolTooLarge))
                    .with("tool", "1/4\" upcut")
                    .with("diameter_mm", 6.35),
                "tool `1/4\" upcut` (6.35 mm) does not fit `Shape` (instance `i1`, profile 2), so that shape is not cut",
            ),
            (
                on_shape(Warning::new(WarningCode::DuplicateShapeDropped))
                    .with("duplicate_of_instance", "i0")
                    .with("duplicate_of_profile", 1),
                "`Shape` (instance `i1`, profile 2) is the same outline in the same place as instance `i0` profile 1, so it is cut only once",
            ),
            (
                on_shape(Warning::new(WarningCode::TabsOverlap))
                    .with("contour", 3)
                    .with("path_length_mm", 99.996_000_1),
                "tabs on `Shape` (instance `i1`, profile 2) touch or overlap on its 100 mm tool path, so they were joined into longer tabs; use fewer or narrower tabs",
            ),
            (
                Warning::new(WarningCode::TabsOverlap)
                    .with("op", "o1")
                    .with("contour", 3)
                    .with("path_length_mm", 12.345),
                "tabs on contour 4 in cut order touch or overlap on its 12.35 mm tool path, so they were joined into longer tabs; use fewer or narrower tabs",
            ),
            (
                Warning::new(WarningCode::TabsOutsideCutRange)
                    .with("op", "o1")
                    .with("reason", "above_stock_top")
                    .with("tab_top_mm", 20.000_000_01)
                    .with("deepest_pass_z_mm", 0.0)
                    .with("stock_top_mm", 18.0),
                "the tab top is 20 mm above the stock bottom, higher than the 18 mm stock top; over each tab every pass lifts clear of the stock",
            ),
            (
                Warning::new(WarningCode::TabsOutsideCutRange)
                    .with("op", "o1")
                    .with("reason", "cut_stops_above_tab")
                    .with("tab_top_mm", 3.0)
                    .with("deepest_pass_z_mm", 4.499_999)
                    .with("stock_top_mm", 18.0),
                "the tab top is 3 mm above the stock bottom, but the deepest pass stops at 4.5 mm, so the tabs change nothing; lower the tabs or cut deeper",
            ),
            (
                on_shape(Warning::new(WarningCode::TabsCoverMostOfContour))
                    .with("contour", 0)
                    .with("path_length_mm", 100.0)
                    .with("tab_count", 4)
                    .with("tab_width_mm", 10.0)
                    .with("diameter_mm", 6.35)
                    .with("tab_span_mm", 16.35)
                    .with("tab_total_mm", 65.4)
                    .with("max_count", 3.0)
                    .with("max_width_mm", 6.15),
                "tabs on `Shape` (instance `i1`, profile 2) cover more than half of its tool path: each of the 4 tabs is 10 mm wide, and with the 6.35 mm bit each one keeps 16.35 mm of the tool path raised, 65.4 mm in total on a 100 mm path (65.4%), so the part or hole may not cut free. To let it cut free, use at most 3 tab(s) this wide, or make the 4 tab(s) at most 6.15 mm wide. The tabs are cut as set.",
            ),
            (
                Warning::new(WarningCode::TabsCoverMostOfContour)
                    .with("op", "o1")
                    .with("contour", 1)
                    .with("path_length_mm", 30.0)
                    .with("tab_count", 2)
                    .with("tab_width_mm", 5.0)
                    .with("diameter_mm", 8.0)
                    .with("tab_span_mm", 13.0)
                    .with("tab_total_mm", 26.0)
                    .with("max_count", 1.0),
                "tabs on contour 2 in cut order cover more than half of its tool path: each of the 2 tabs is 5 mm wide, and with the 8 mm bit each one keeps 13 mm of the tool path raised, 26 mm in total on a 30 mm path (86.7%), so the part or hole may not cut free. To let it cut free, use at most 1 tab(s) this wide. The tabs are cut as set.",
            ),
            (
                on_shape(Warning::new(WarningCode::TabsCoverMostOfContour))
                    .with("contour", 0)
                    .with("path_length_mm", 10.0)
                    .with("tab_count", 1)
                    .with("tab_width_mm", 3.0)
                    .with("diameter_mm", 3.175)
                    .with("tab_span_mm", 6.175)
                    .with("tab_total_mm", 6.175)
                    .with("max_count", 0.0),
                "tabs on `Shape` (instance `i1`, profile 2) cover more than half of its tool path: its one tab is 3 mm wide, and with the 3.175 mm bit it keeps 6.17 mm of the tool path raised on a 10 mm path (61.8%), so the part or hole may not cut free. To let it cut free, even one tab this wide covers more than half: this outline is too short for tabs with this bit, so consider removing them. The tabs are cut as set.",
            ),
            (
                on_shape(Warning::new(WarningCode::ToolpathsOverlap))
                    .with("other_instance", "i9")
                    .with("other_instance_name", "Other")
                    .with("other_profile", 0)
                    .with("gap_mm", 1.004_999)
                    .with("tool", "3 mm")
                    .with("diameter_mm", 3.0)
                    .with("needed_mm", 1.5),
                "the tool paths of `Shape` (instance `i1`, profile 2) and `Other` (instance `i9`, profile 0) cut into each other: the shapes are 1 mm apart, closer than the 1.5 mm tool `3 mm` (3 mm) needs between them, so cutting one gouges the other. Move them at least 1.5 mm apart or use a smaller bit.",
            ),
            (
                on_shape(Warning::new(WarningCode::PocketNoRoom))
                    .with("tool", "1/8\"")
                    .with("diameter_mm", 3.175),
                "tool `1/8\"` (3.175 mm) is wider than the pocket of `Shape` (instance `i1`, profile 2) somewhere it must pass, so that shape is not cut; use a smaller bit",
            ),
            (
                on_shape(Warning::new(WarningCode::PocketIslandTooClose))
                    .with("island", 3)
                    .with("other", "wall")
                    .with("gap_mm", 4.999_9)
                    .with("needed_mm", 6.35)
                    .with("allowance_mm", 0.0)
                    .with("tool", "t")
                    .with("diameter_mm", 6.35),
                "in the pocket of `Shape` (instance `i1`, profile 2), island profile 3 is 5 mm from the pocket wall, closer than the 6.35 mm tool `t` is wide, so the material between them is left uncut",
            ),
            (
                on_shape(Warning::new(WarningCode::PocketIslandTooClose))
                    .with("island", 3)
                    .with("other", 4)
                    .with("gap_mm", 7.0)
                    .with("needed_mm", 7.350_000_1)
                    .with("allowance_mm", 0.5)
                    .with("tool", "t")
                    .with("diameter_mm", 6.35),
                "in the pocket of `Shape` (instance `i1`, profile 2), island profile 3 is 7 mm from island profile 4, closer than tool `t` needs with a 0.5 mm allowance (7.35 mm), so the material between them is left uncut",
            ),
            (
                on_shape(Warning::new(WarningCode::EntryFallback))
                    .with("requested", "helix")
                    .with("used", "ramp")
                    .with("reason", "no_room"),
                "there is no room for a `helix` entry in `Shape` (instance `i1`, profile 2), so its passes start with a zigzag ramp instead",
            ),
            (
                on_shape(Warning::new(WarningCode::EntryFallback))
                    .with("requested", "ramp")
                    .with("used", "plunge")
                    .with("reason", "too_many_moves"),
                "a `ramp` entry at this angle would take too many moves to reach depth in `Shape` (instance `i1`, profile 2), so its passes start with a straight plunge at the plunge rate instead; set the operation's entry to `plunge` to stop this warning",
            ),
            (
                on_shape(Warning::new(WarningCode::HelixDiameterReduced))
                    .with("requested_mm", 5.08)
                    .with("used_mm", 2.539_999_9),
                "the helix entry in `Shape` (instance `i1`, profile 2) is 2.54 mm across, smaller than the 5.08 mm asked for, because a larger one does not fit there",
            ),
            (
                on_shape(Warning::new(WarningCode::LeadShrunk))
                    .with("requested_mm", 30.0)
                    .with("used_mm", 22.499_999)
                    .with("requested_sweep_deg", 90.0)
                    .with("used_sweep_deg", 44.6),
                "the lead arc on `Shape` (instance `i1`, profile 2) is 22.5 mm in radius over 45°, smaller than the 30 mm over 90° asked for, because a larger one does not fit there",
            ),
            (
                on_shape(Warning::new(WarningCode::LeadDropped)).with("reason", "no_room"),
                "there is no room for a lead-in arc on `Shape` (instance `i1`, profile 2), even at a quarter of the radius, so its passes start on the outline without one",
            ),
            (
                on_shape(Warning::new(WarningCode::LeadDropped)).with("reason", "exit"),
                "there is no room for a lead-out arc where the cut on `Shape` (instance `i1`, profile 2) ends, so it stops on the outline",
            ),
            (
                Warning::new(WarningCode::LeadDropped)
                    .with("op", "o1")
                    .with("reason", "on_line"),
                "operation `o1` cuts on the line, which has no waste side for a lead, so it cuts without one; remove the lead or cut Inside or Outside",
            ),
            (
                Warning::new(WarningCode::ReliefSkipped)
                    .with("op", "o1")
                    .with("requested", "dogbone")
                    .with("reason", "no_step_down"),
                "corner relief `dogbone` is skipped because the tool has no step_down_mm, which the relief pass steps down at, so no corners were relieved; set the tool's step-down",
            ),
            (
                Warning::new(WarningCode::ReliefSkipped)
                    .with("op", "o1")
                    .with("requested", "t_bone")
                    .with("reason", "on_line"),
                "corner relief `t_bone` is skipped: an On line Profile has no waste side to cut a notch into, so no corners were relieved",
            ),
            (
                on_shape(Warning::new(WarningCode::ReliefCornerTooTight))
                    .with("x_mm", 10.004)
                    .with("y_mm", -0.001)
                    .with("reason", "short_edge"),
                "the corner of `Shape` (instance `i1`, profile 2) at (10, 0) mm is not relieved: an edge beside it is shorter than the bit is wide, so it keeps the bit's radius",
            ),
            (
                on_shape(Warning::new(WarningCode::ReliefCornerTooTight))
                    .with("x_mm", 1.5)
                    .with("y_mm", 2.25)
                    .with("reason", "no_wall_path"),
                "the corner of `Shape` (instance `i1`, profile 2) at (1.5, 2.25) mm is not relieved: the tool path does not turn it within reach, so it keeps the bit's radius",
            ),
            (
                on_shape(Warning::new(WarningCode::ReliefCornerNotCut))
                    .with("x_mm", -3.0)
                    .with("y_mm", 40.125),
                "the corner of `Shape` (instance `i1`, profile 2) at (-3, 40.12) mm is not relieved because the main passes did not cut to full depth where its notch starts (a tab over it, or no wall pass), and a notch must not plunge into uncut material; it keeps the bit's radius",
            ),
            (
                on_shape(Warning::new(WarningCode::EngravePathDegenerate)),
                "`Shape` (instance `i1`, profile 2) has fewer than two distinct points, so there is nothing to engrave; it is skipped",
            ),
            (
                on_shape(Warning::new(WarningCode::CutInsideFreedPart))
                    .with("earlier_op", "o0")
                    .with("freed_instance", "i0")
                    .with("freed_instance_name", "Outline")
                    .with("freed_profile", 0),
                "`Shape` (instance `i1`, profile 2) lies inside `Outline` (instance `i0`, profile 0), which operation `o0` cuts free of the stock first, so this cut would run in a loose part; move this operation before `o0`, or hold that part with tabs",
            ),
            (
                Warning::new(WarningCode::ToolChangeAvoidable)
                    .with("tool", "t1")
                    .with("tool_name", "6.35 mm upcut")
                    .with("ops", "`o1`, `o3`"),
                "tool `t1` (`6.35 mm upcut`) is used by operations `o1`, `o3`, which do not run one after another, so it is fitted more than once; put those operations next to each other to save a tool change (operations always run in the order listed)",
            ),
            (
                on_shape(Warning::new(WarningCode::DrillTargetNotCircle)),
                "`Shape` (instance `i1`, profile 2) is not a circle; Drill plunges only at the centre of circles, so it is not drilled",
            ),
            (
                on_shape(Warning::new(WarningCode::DrillToolWiderThanHole))
                    .with("hole_mm", 4.999_999_9)
                    .with("tool_mm", 6.35),
                "the 6.35 mm tool is wider than the 5 mm hole `Shape` (instance `i1`, profile 2), so the hole comes out the tool's size",
            ),
            (
                Warning::new(WarningCode::DrillPeckDeeperThanHole)
                    .with("op", "o1")
                    .with("peck_mm", 20.0)
                    .with("depth_mm", 12.000_000_1),
                "the 20 mm peck is at least the 12 mm depth of the holes, so each is drilled in one plunge",
            ),
            (
                Warning::new(WarningCode::DrillRetractBelowStockTop)
                    .with("op", "o1")
                    .with("requested_mm", -2.5)
                    .with("used_mm", 0.0),
                "retract_mm is -2.5 mm, below the stock top; the tool retracts to the stock top instead, because it must never rapid into material",
            ),
            (
                Warning::new(WarningCode::RestOfClearsNothing)
                    .with("op", "o2")
                    .with("rest_of", "o1")
                    .with("tool", "12.7 mm end mill"),
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

    #[test]
    fn rounded_drops_noise_and_trailing_zeros() {
        assert_eq!(rounded(54.600_002_225_240_075, 2), "54.6");
        assert_eq!(rounded(14.35, 2), "14.35");
        assert_eq!(rounded(12.0, 2), "12");
        assert_eq!(rounded(-0.001, 2), "0");
        assert_eq!(rounded(105.128, 1), "105.1");
    }

    #[test]
    fn coverage_warning_with_one_tab_reads_in_the_singular() {
        let w = Warning::new(WarningCode::TabsCoverMostOfContour)
            .with("instance", "i1")
            .with("instance_name", "Hole")
            .with("profile", 1)
            .with("path_length_mm", 10.0)
            .with("tab_count", 1)
            .with("tab_width_mm", 3.0)
            .with("diameter_mm", 3.175)
            .with("tab_span_mm", 6.175)
            .with("tab_total_mm", 6.175)
            .with("max_count", 0);
        assert_eq!(
            w.to_string(),
            "tabs on `Hole` (instance `i1`, profile 1) cover more than half of its tool path: its one tab is 3 mm wide, and with the 3.175 mm bit it keeps 6.17 mm of the tool path raised on a 10 mm path (61.8%), so the part or hole may not cut free. To let it cut free, even one tab this wide covers more than half: this outline is too short for tabs with this bit, so consider removing them. The tabs are cut as set."
        );
    }

    #[test]
    fn entry_warnings_name_the_shape_and_both_entries() {
        let shape = |w: Warning| {
            w.with("op", "o1")
                .with("instance", "i1")
                .with("instance_name", "Slot")
                .with("profile", 0)
        };
        let ramp = shape(Warning::new(WarningCode::EntryFallback))
            .with("requested", "helix")
            .with("used", "ramp")
            .with("reason", "no_room");
        assert_eq!(
            ramp.to_string(),
            "there is no room for a `helix` entry in `Slot` (instance `i1`, profile 0), so its passes start with a zigzag ramp instead"
        );
        let plunge = shape(Warning::new(WarningCode::EntryFallback))
            .with("requested", "ramp")
            .with("used", "plunge")
            .with("reason", "no_room");
        assert_eq!(
            plunge.to_string(),
            "there is no room for a `ramp` entry in `Slot` (instance `i1`, profile 0), so its passes start with a straight plunge at the plunge rate instead; set the operation's entry to `plunge` to stop this warning"
        );
        let reduced = shape(Warning::new(WarningCode::HelixDiameterReduced))
            .with("requested_mm", 5.08)
            .with("used_mm", 2.539_999_9);
        assert_eq!(
            reduced.to_string(),
            "the helix entry in `Slot` (instance `i1`, profile 0) is 2.54 mm across, smaller than the 5.08 mm asked for, because a larger one does not fit there"
        );
    }

    #[test]
    fn lead_warnings_name_the_shape_and_both_sizes() {
        let shape = |w: Warning| {
            w.with("op", "o1")
                .with("instance", "i1")
                .with("instance_name", "Hole")
                .with("profile", 0)
        };
        let shrunk = shape(Warning::new(WarningCode::LeadShrunk))
            .with("requested_mm", 30.0)
            .with("used_mm", 22.5)
            .with("requested_sweep_deg", 90.0)
            .with("used_sweep_deg", 45.0);
        assert_eq!(
            shrunk.to_string(),
            "the lead arc on `Hole` (instance `i1`, profile 0) is 22.5 mm in radius over 45°, smaller than the 30 mm over 90° asked for, because a larger one does not fit there"
        );
        let no_room = shape(Warning::new(WarningCode::LeadDropped)).with("reason", "no_room");
        assert_eq!(
            no_room.to_string(),
            "there is no room for a lead-in arc on `Hole` (instance `i1`, profile 0), even at a quarter of the radius, so its passes start on the outline without one"
        );
        let exit = shape(Warning::new(WarningCode::LeadDropped)).with("reason", "exit");
        assert_eq!(
            exit.to_string(),
            "there is no room for a lead-out arc where the cut on `Hole` (instance `i1`, profile 0) ends, so it stops on the outline"
        );
        let on_line = Warning::new(WarningCode::LeadDropped)
            .with("op", "o1")
            .with("reason", "on_line");
        assert_eq!(
            on_line.to_string(),
            "operation `o1` cuts on the line, which has no waste side for a lead, so it cuts without one; remove the lead or cut Inside or Outside"
        );
    }

    #[test]
    fn engrave_path_degenerate_names_the_shape() {
        let w = Warning::new(WarningCode::EngravePathDegenerate)
            .with("op", "o4")
            .with("instance", "i2")
            .with("instance_name", "Dot")
            .with("profile", 0);
        assert_eq!(
            w.to_string(),
            "`Dot` (instance `i2`, profile 0) has fewer than two distinct points, so there is nothing to engrave; it is skipped"
        );
    }

    #[test]
    fn toolpaths_overlap_names_both_shapes_and_the_gap() {
        let w = Warning::new(WarningCode::ToolpathsOverlap)
            .with("instance", "a")
            .with("instance_name", "Left")
            .with("profile", 0)
            .with("other_instance", "b")
            .with("other_instance_name", "Right")
            .with("other_profile", 1)
            .with("gap_mm", 2.000_000_000_1)
            .with("tool", "6 mm upcut")
            .with("diameter_mm", 6.0)
            .with("needed_mm", 6.0);
        assert_eq!(
            w.to_string(),
            "the tool paths of `Left` (instance `a`, profile 0) and `Right` (instance `b`, profile 1) cut into each other: the shapes are 2 mm apart, closer than the 6 mm tool `6 mm upcut` (6 mm) needs between them, so cutting one gouges the other. Move them at least 6 mm apart or use a smaller bit."
        );
    }

    #[test]
    fn drill_warnings_name_the_shape_and_sizes() {
        let shape = |code| {
            Warning::new(code)
                .with("op", "o3")
                .with("instance", "s")
                .with("instance_name", "Slot")
                .with("profile", 0)
        };
        assert_eq!(
            shape(WarningCode::DrillTargetNotCircle).to_string(),
            "`Slot` (instance `s`, profile 0) is not a circle; Drill plunges only at the centre of circles, so it is not drilled"
        );
        let wide = shape(WarningCode::DrillToolWiderThanHole)
            .with("hole_mm", 4.999_999_9)
            .with("tool_mm", 6.35);
        assert_eq!(
            wide.to_string(),
            "the 6.35 mm tool is wider than the 5 mm hole `Slot` (instance `s`, profile 0), so the hole comes out the tool's size"
        );
        let peck = Warning::new(WarningCode::DrillPeckDeeperThanHole)
            .with("op", "o3")
            .with("peck_mm", 20.0)
            .with("depth_mm", 12.0);
        assert_eq!(
            peck.to_string(),
            "the 20 mm peck is at least the 12 mm depth of the holes, so each is drilled in one plunge"
        );
    }
}
