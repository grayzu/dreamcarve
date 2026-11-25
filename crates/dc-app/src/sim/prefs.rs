// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What the Simulate step remembers between sessions (phase 6 functional
//! spec, capability 124): Quality, Colour, Layers, the Uncut material
//! switch, its threshold and the last Speed. They are the app's, written
//! under `[simulation]` in `gui.toml`; the project file holds nothing of
//! the simulation.

use dc_job::sim::{Quality, UNCUT_MM, UNCUT_RANGE_MM};

use super::transport::Speed;

/// What colours a cut cell (capability 116). The uncut top keeps the
/// material's colour in all three.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Colouring {
    /// The one colour of the material.
    #[default]
    Material,
    /// The colour of the operation that cut the cell last.
    Operation,
    /// The cell's depth on the depth ramp.
    Depth,
}

impl Colouring {
    /// Every choice, in the control's order.
    pub const ALL: [Self; 3] = [Self::Material, Self::Operation, Self::Depth];

    /// Its name in `gui.toml`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Material => "material",
            Self::Operation => "operation",
            Self::Depth => "depth",
        }
    }

    /// The choice `name` names in `gui.toml`.
    #[must_use]
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == name)
    }
}

/// What is drawn besides the stock (capability 117).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layers {
    /// The tool, where it is.
    pub tool: bool,
    /// The cutting moves of the operation being shown, as lines.
    pub toolpath: bool,
    /// Its rapids, dashed.
    pub rapids: bool,
    /// The bed under the stock.
    pub bed: bool,
    /// The machine origin's axes.
    pub origin: bool,
}

impl Default for Layers {
    /// The tool, the bed and the origin; the toolpath and its rapids
    /// are off at first.
    fn default() -> Self {
        Self {
            tool: true,
            toolpath: false,
            rapids: false,
            bed: true,
            origin: true,
        }
    }
}

/// The Simulate step's preferences.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimPrefs {
    /// How finely the stock is divided.
    pub quality: Quality,
    /// What colours a cut cell.
    pub colouring: Colouring,
    /// What is drawn besides the stock.
    pub layers: Layers,
    /// Whether uncut material is highlighted and listed: off until it is
    /// turned on (capability 120).
    pub uncut_shown: bool,
    /// How far above the design the stock stands before it is uncut, mm.
    pub uncut_mm: f64,
    /// The last Speed chosen; `None` until one is, and then a job opens
    /// at [`Speed::opening`].
    pub speed: Option<Speed>,
}

impl Default for SimPrefs {
    fn default() -> Self {
        Self {
            quality: Quality::Standard,
            colouring: Colouring::default(),
            layers: Layers::default(),
            uncut_shown: false,
            uncut_mm: UNCUT_MM,
            speed: None,
        }
    }
}

impl SimPrefs {
    /// The threshold, held to the range the check takes.
    #[must_use]
    pub fn uncut_mm(&self) -> f64 {
        let (least, most) = UNCUT_RANGE_MM;
        if self.uncut_mm.is_nan() {
            UNCUT_MM
        } else {
            self.uncut_mm.clamp(least, most)
        }
    }
}

/// A quality's name in `gui.toml`.
#[must_use]
pub const fn quality_name(quality: Quality) -> &'static str {
    match quality {
        Quality::Draft => "draft",
        Quality::Standard => "standard",
        Quality::Fine => "fine",
    }
}

/// The quality `name` names in `gui.toml`.
#[must_use]
pub fn quality_named(name: &str) -> Option<Quality> {
    [Quality::Draft, Quality::Standard, Quality::Fine]
        .into_iter()
        .find(|q| quality_name(*q) == name)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "test code reports failure by panicking"
    )]

    use super::*;
    use dc_geometry::assert_approx_eq;

    /// Capability 124's defaults: Standard, the material's colour, the
    /// toolpath and rapids off, uncut material off, its threshold 0.3 mm.
    #[test]
    fn the_defaults_are_the_specs() {
        let prefs = SimPrefs::default();
        assert_eq!(prefs.quality, Quality::Standard);
        assert_eq!(prefs.colouring, Colouring::Material);
        assert!(prefs.layers.tool && prefs.layers.bed && prefs.layers.origin);
        assert!(!prefs.layers.toolpath && !prefs.layers.rapids);
        assert!(!prefs.uncut_shown);
        assert_approx_eq!(prefs.uncut_mm(), 0.3, 1e-9);
        assert_eq!(prefs.speed, None);
    }

    #[test]
    fn names_round_trip_and_a_stranger_is_none() {
        for colouring in Colouring::ALL {
            assert_eq!(Colouring::parse(colouring.as_str()), Some(colouring));
        }
        for quality in [Quality::Draft, Quality::Standard, Quality::Fine] {
            assert_eq!(quality_named(quality_name(quality)), Some(quality));
        }
        assert_eq!(Colouring::parse("wood"), None);
        assert_eq!(quality_named("ultra"), None);
    }

    #[test]
    fn a_threshold_out_of_range_is_held_to_it() {
        let mut prefs = SimPrefs::default();
        for (set, want) in [(0.001, 0.05), (99.0, 5.0), (f64::NAN, 0.3), (1.25, 1.25)] {
            prefs.uncut_mm = set;
            assert_approx_eq!(prefs.uncut_mm(), want, 1e-9);
        }
    }
}
