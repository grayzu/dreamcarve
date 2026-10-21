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
    /// The peck depth is at least the hole depth, so the hole is drilled in
    /// one plunge. Params: `op`, `instance`, `instance_name`, `profile`,
    /// `peck_mm`, `depth_mm`.
    DrillPeckDeeperThanHole,
    /// The retract height is below the stock top, so it was raised to the
    /// top: the tool must never rapid into material. Params: `op`,
    /// `requested_mm` (the height above the stock top as set, negative),
    /// `used_mm` (0).
    DrillRetractBelowStockTop,
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
            Self::EngravePathDegenerate => "engrave_path_degenerate",
            Self::CutInsideFreedPart => "cut_inside_freed_part",
            Self::ToolChangeAvoidable => "tool_change_avoidable",
            Self::DrillTargetNotCircle => "drill_target_not_circle",
            Self::DrillToolWiderThanHole => "drill_tool_wider_than_hole",
            Self::DrillPeckDeeperThanHole => "drill_peck_deeper_than_hole",
            Self::DrillRetractBelowStockTop => "drill_retract_below_stock_top",
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
                "{} is an open path; Profile cuts only closed shapes, so it is not cut",
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
                "the {} mm peck is at least the {} mm depth of {}, so it is drilled in one plunge",
                self.mm("peck_mm"),
                self.mm("depth_mm"),
                self.shape()
            ),
            WarningCode::DrillRetractBelowStockTop => write!(
                f,
                "retract_mm is {} mm, below the stock top; the tool retracts to the stock top instead, because it must never rapid into material",
                self.mm("requested_mm")
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
}
