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
    /// in cut order), `path_length_mm`.
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
    /// `tab_span_mm` (one tab's length of path: width plus tool diameter),
    /// `tab_total_mm` (count times span), `max_count` (most tabs of this
    /// width that cover at most half), and `max_width_mm` (widest tab at
    /// this count that covers at most half; absent when none fits).
    TabsCoverMostOfContour,
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

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let params: Vec<String> = self
            .params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        write!(f, "{} ({})", self.code, params.join(", "))
    }
}
