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
    /// A target shape is an open path, which Profile cannot cut; it was
    /// skipped. Params: `instance`, `instance_name`, `profile`.
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
    /// islands, are closer than the tool is wide, so the material between
    /// them stays. Params: `op`, `instance`, `instance_name`, `profile`
    /// (the pocket's outline), `island` (the island's profile index),
    /// `other` (`wall`, or the other island's profile index), `gap_mm`,
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
            WarningCode::OpenPathSkipped => write!(
                f,
                "{} is an open path; Profile and Pocket cut only closed shapes, so it is not cut",
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
                write!(
                    f,
                    "in the pocket of {}, island profile {} is {} mm from {other}, closer than the {} mm tool `{}` is wide, so the material between them is left uncut",
                    self.shape(),
                    self.text("island"),
                    self.mm("gap_mm"),
                    self.number("diameter_mm", DIAMETER_DECIMALS),
                    self.text("tool")
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
