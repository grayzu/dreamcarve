// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What a [`Tool`] derives from its stored values
//! (`docs/specs/phase-3/design.md`, "Data model → Tapered ball nose"), and
//! the values it still needs ([`Tool::unset_values`]).
//!
//! A tapered ball nose (TBN) is a ball of radius `ρ = tip_diameter_mm / 2`
//! with a cone of half-angle `β` (`taper_angle_deg`, per side) tangent to
//! it, capped by the cutting diameter `2R = diameter_mm`. With `t = tan β`:
//! the tangent height `h_t = ρ (1 − sin β)`, the apex offset `δ = ρ (1/sin
//! β − 1)` (the cone, continued down, comes to a point `δ` below the tip),
//! the cap height `h_R = R/t − δ`, and the half-width at height `d` above
//! the tip `w(d) = √(2ρd − d²)` for `d ≤ h_t`, else `min((d + δ) t, R)`.
//!
//! These read the stored values directly; once `dc_geometry::profile`
//! lands (27b, #678) they may read its `BitProfile` instead.

use std::fmt;

use crate::ids::ToolId;
use crate::model::{Tool, ToolKind};

/// The location of a tapered ball nose's tip diameter within its tool.
pub const TBN_TIP: &str = "cutter.tapered_ball_nose.tip_diameter_mm";
/// The location of a tapered ball nose's taper within its tool.
pub const TBN_TAPER: &str = "cutter.tapered_ball_nose.taper_angle_deg";
/// The location of a tapered ball nose's flute length within its tool.
pub const TBN_FLUTES: &str = "cutter.tapered_ball_nose.flute_length_mm";

/// The steepest taper per side a tapered ball nose may have, degrees: a
/// sanity limit, not a physical one (the steepest catalogue TBN is about
/// 15°; a 45° one is a 90° V-bit with a round tip).
pub const MAX_TAPER_ANGLE_DEG: f64 = 45.0;

/// A value a tool still needs before an enabled operation may use it
/// ([`Tool::unset_values`]): `tools add`, `init` and the library's import
/// report name them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsetValue {
    /// The tool.
    pub tool: ToolId,
    /// The field within the tool, such as `feed_mm_min` or
    /// `cutter.tapered_ball_nose.flute_length_mm`.
    pub field: &'static str,
}

impl UnsetValue {
    /// Where the value sits in a project, as validation names it:
    /// `tools[t3].cutter.tapered_ball_nose.flute_length_mm`.
    #[must_use]
    pub fn location(&self) -> String {
        format!("tools[{}].{}", self.tool, self.field)
    }

    /// Whether the value is the cutter's, not a feed or the speed.
    #[must_use]
    pub fn is_cutter(&self) -> bool {
        self.field.starts_with("cutter.")
    }
}

impl fmt::Display for UnsetValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.location())
    }
}

/// A tapered ball nose's values when every one is set and usable: the tip
/// radius `ρ`, `sin β`, `tan β`, the cap radius `R` and the flute length.
#[derive(Debug, Clone, Copy)]
struct Taper {
    tip_radius_mm: f64,
    sin: f64,
    slope: f64,
    cap_radius_mm: f64,
    flute_length_mm: f64,
}

impl Taper {
    /// The values of `tool`, or `None` when it is not a tapered ball nose
    /// or a value is unset or out of range (validation names which).
    fn of(tool: &Tool) -> Option<Self> {
        let ToolKind::TaperedBallNose {
            tip_diameter_mm: Some(tip_mm),
            taper_angle_deg: Some(taper_deg),
            flute_length_mm: Some(flute_length_mm),
        } = tool.cutter
        else {
            return None;
        };
        let usable = tip_mm.is_finite()
            && tip_mm > 0.0
            && taper_deg > 0.0
            && taper_deg <= MAX_TAPER_ANGLE_DEG
            && tool.diameter_mm.is_finite()
            && tool.diameter_mm > tip_mm
            && flute_length_mm.is_finite();
        let beta = taper_deg.to_radians();
        let taper = Self {
            tip_radius_mm: tip_mm / 2.0,
            sin: beta.sin(),
            slope: beta.tan(),
            cap_radius_mm: tool.diameter_mm / 2.0,
            flute_length_mm,
        };
        (usable && flute_length_mm > taper.tangent_height_mm()).then_some(taper)
    }

    /// `h_t = ρ (1 − sin β)`, where the ball meets the cone.
    fn tangent_height_mm(&self) -> f64 {
        self.tip_radius_mm * (1.0 - self.sin)
    }

    /// `δ = ρ (1/sin β − 1)`.
    fn apex_offset_mm(&self) -> f64 {
        self.tip_radius_mm * (1.0 / self.sin - 1.0)
    }

    /// `h_R = R/t − δ`, where the cone reaches the cutting diameter.
    fn cap_height_mm(&self) -> f64 {
        self.cap_radius_mm / self.slope - self.apex_offset_mm()
    }

    /// `w(d)`, the half-width at `height_mm` above the tip, capped.
    fn half_width_mm(&self, height_mm: f64) -> f64 {
        let d = height_mm.max(0.0);
        if d <= self.tangent_height_mm() {
            (2.0 * self.tip_radius_mm * d - d * d).max(0.0).sqrt()
        } else {
            ((d + self.apex_offset_mm()) * self.slope).min(self.cap_radius_mm)
        }
    }
}

/// The tangent height of a tapered ball nose of tip diameter `tip_mm` and
/// taper `taper_deg` per side, mm: `ρ (1 − sin β)`. `None` when either is
/// out of range. Validation reads it to hold the flutes above the ball.
#[must_use]
pub fn tapered_tangent_height_mm(tip_mm: f64, taper_deg: f64) -> Option<f64> {
    let usable =
        tip_mm.is_finite() && tip_mm > 0.0 && taper_deg > 0.0 && taper_deg <= MAX_TAPER_ANGLE_DEG;
    usable.then(|| tip_mm / 2.0 * (1.0 - taper_deg.to_radians().sin()))
}

impl Tool {
    /// A tapered ball nose's tip radius, `tip_diameter_mm / 2`, mm; `None`
    /// for any other cutter or while the tip is unset.
    #[must_use]
    pub fn tip_radius_mm(&self) -> Option<f64> {
        match self.cutter {
            ToolKind::TaperedBallNose {
                tip_diameter_mm, ..
            } => tip_diameter_mm.map(|d| d / 2.0),
            _ => None,
        }
    }

    /// The deepest the bit's flank may be engaged, mm: a V-bit's cone
    /// height, `diameter / (2 tan(α/2))`, or a tapered ball nose's
    /// `min(flute_length_mm, h_R)`, past which the shank rubs or the bit
    /// is a cylinder. `None` for any other cutter, for a value unset, or
    /// for one out of range.
    #[must_use]
    pub fn cutting_height_mm(&self) -> Option<f64> {
        match self.cutter {
            ToolKind::VBit { included_angle_deg } => {
                let usable = included_angle_deg.is_finite()
                    && included_angle_deg > 0.0
                    && included_angle_deg < 180.0
                    && self.diameter_mm.is_finite()
                    && self.diameter_mm > 0.0;
                let slope = (included_angle_deg.to_radians() / 2.0).tan();
                (usable && slope.is_finite() && slope > 0.0).then(|| self.diameter_mm / 2.0 / slope)
            }
            ToolKind::TaperedBallNose { .. } => {
                Taper::of(self).map(|t| t.flute_length_mm.min(t.cap_height_mm()))
            }
            ToolKind::EndMill | ToolKind::BallNose | ToolKind::Engraver | ToolKind::Drill => None,
        }
    }

    /// The widest the bit cuts, as a radius, mm: the swept radius of a
    /// cut (S7, `swept_bounds`, the preview). `diameter_mm / 2` for every
    /// cutter but a tapered ball nose, whose is `min(w(flute_length_mm),
    /// diameter_mm / 2)`, less than the cap when the flutes end below it.
    /// `diameter_mm / 2`, the most it can be, while a value is unset.
    #[must_use]
    pub fn cut_radius_mm(&self) -> f64 {
        let cap_mm = self.diameter_mm / 2.0;
        Taper::of(self).map_or(cap_mm, |t| t.half_width_mm(t.flute_length_mm).min(cap_mm))
    }

    /// The values this tool still needs before an enabled operation may
    /// use it, in the order the file holds them: a tapered ball nose's
    /// unset tip, taper and flute length, then an unset feed, plunge rate
    /// and spindle speed. Not the step-down, which an operation may set
    /// instead, nor a value set but out of range, which validation names.
    #[must_use]
    pub fn unset_values(&self) -> Vec<UnsetValue> {
        let mut fields = Vec::new();
        if let ToolKind::TaperedBallNose {
            tip_diameter_mm,
            taper_angle_deg,
            flute_length_mm,
        } = self.cutter
        {
            for (field, value) in [
                (TBN_TIP, tip_diameter_mm),
                (TBN_TAPER, taper_angle_deg),
                (TBN_FLUTES, flute_length_mm),
            ] {
                if value.is_none() {
                    fields.push(field);
                }
            }
        }
        for (field, unset) in [
            ("feed_mm_min", self.feed_mm_min.is_none()),
            ("plunge_mm_min", self.plunge_mm_min.is_none()),
            ("rpm", self.rpm.is_none()),
        ] {
            if unset {
                fields.push(field);
            }
        }
        fields
            .into_iter()
            .map(|field| UnsetValue {
                tool: self.id.clone(),
                field,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use crate::ids::ToolId;
    use crate::model::{Tool, ToolKind};
    use crate::test_support::{tool_6_35_upcut, tool_tbn_fine, tool_tbn_ultra, tool_v_bit_90};
    use dc_geometry::assert_approx_eq;

    fn unset_tbn() -> Tool {
        Tool {
            id: ToolId::new("t3"),
            cutter: ToolKind::TaperedBallNose {
                tip_diameter_mm: Some(0.762),
                taper_angle_deg: None,
                flute_length_mm: None,
            },
            feed_mm_min: None,
            rpm: None,
            ..tool_tbn_ultra()
        }
    }

    #[test]
    fn tip_radius_is_half_the_tip_diameter() {
        assert_approx_eq!(tool_tbn_ultra().tip_radius_mm().unwrap(), 0.381, 1e-12);
        assert_approx_eq!(tool_tbn_fine().tip_radius_mm().unwrap(), 1.143, 1e-12);
        assert_eq!(tool_v_bit_90().tip_radius_mm(), None);
        assert_eq!(tool_6_35_upcut().tip_radius_mm(), None);
        let mut unset = tool_tbn_ultra();
        unset.cutter = ToolKind::TaperedBallNose {
            tip_diameter_mm: None,
            taper_angle_deg: Some(6.7),
            flute_length_mm: Some(19.05),
        };
        assert_eq!(unset.tip_radius_mm(), None);
    }

    /// The Fine bit's cone reaches the 6.35 mm cutting diameter below its
    /// flute top, so the cap holds; the Ultra's flutes end first, at
    /// `w(19.05) = (19.05 + δ) tan 6.7°`.
    #[test]
    fn cut_radius_is_capped_by_the_cutting_diameter() {
        assert_approx_eq!(tool_tbn_fine().cut_radius_mm(), 3.175, 1e-3);
        assert_approx_eq!(tool_tbn_ultra().cut_radius_mm(), 2.577, 1e-3);
        // A value unset: the cutting diameter's half, the widest it can be.
        assert_approx_eq!(unset_tbn().cut_radius_mm(), 3.175, 1e-12);
    }

    #[test]
    fn v_bit_cut_radius_is_half_its_diameter() {
        assert_eq!(tool_v_bit_90().cut_radius_mm(), 6.35 / 2.0);
        assert_eq!(tool_6_35_upcut().cut_radius_mm(), 6.35 / 2.0);
        for cutter in [ToolKind::BallNose, ToolKind::Engraver, ToolKind::Drill] {
            let tool = Tool {
                cutter,
                diameter_mm: 3.0,
                ..tool_6_35_upcut()
            };
            assert_eq!(tool.cut_radius_mm(), 1.5);
        }
    }

    #[test]
    fn cutting_height_is_the_lower_of_flutes_and_cap() {
        assert_approx_eq!(tool_tbn_fine().cutting_height_mm().unwrap(), 18.374, 1e-3);
        assert_approx_eq!(tool_tbn_ultra().cutting_height_mm().unwrap(), 19.05, 1e-3);
        // A V-bit's cone height, unchanged: diameter / (2 tan(α/2)).
        assert_approx_eq!(tool_v_bit_90().cutting_height_mm().unwrap(), 3.175, 1e-12);
        let v60 = Tool {
            cutter: ToolKind::VBit {
                included_angle_deg: 60.0,
            },
            ..tool_v_bit_90()
        };
        assert_approx_eq!(
            v60.cutting_height_mm().unwrap(),
            6.35 / (2.0 * 30f64.to_radians().tan()),
            1e-12
        );
        assert_eq!(tool_6_35_upcut().cutting_height_mm(), None);
        assert_eq!(unset_tbn().cutting_height_mm(), None);
    }

    #[test]
    fn describe_names_a_tapered_ball_nose() {
        assert_eq!(tool_tbn_ultra().cutter.describe(), "a tapered ball nose");
        assert!(!tool_tbn_ultra().cutter.is_flat());
    }

    /// The cutter's unset values first, then the feeds and speed, in the
    /// order the file holds them, each located by the tool's id.
    #[test]
    fn unset_values_lists_feeds_and_tapered_values() {
        let tool = Tool {
            cutter: ToolKind::TaperedBallNose {
                tip_diameter_mm: None,
                taper_angle_deg: None,
                flute_length_mm: None,
            },
            ..unset_tbn()
        };
        let at: Vec<String> = tool.unset_values().iter().map(|u| u.location()).collect();
        assert_eq!(
            at,
            [
                "tools[t3].cutter.tapered_ball_nose.tip_diameter_mm",
                "tools[t3].cutter.tapered_ball_nose.taper_angle_deg",
                "tools[t3].cutter.tapered_ball_nose.flute_length_mm",
                "tools[t3].feed_mm_min",
                "tools[t3].rpm",
            ]
        );
        let fields: Vec<&str> = unset_tbn().unset_values().iter().map(|u| u.field).collect();
        assert_eq!(
            fields,
            [
                "cutter.tapered_ball_nose.taper_angle_deg",
                "cutter.tapered_ball_nose.flute_length_mm",
                "feed_mm_min",
                "rpm",
            ]
        );
        assert!(unset_tbn().unset_values()[0].is_cutter());
        assert!(!unset_tbn().unset_values()[2].is_cutter());
        assert_eq!(tool_tbn_ultra().unset_values(), vec![]);
        assert_eq!(tool_v_bit_90().unset_values(), vec![]);
        // An end mill's are its feeds and speed only.
        let bare = Tool {
            feed_mm_min: None,
            plunge_mm_min: None,
            rpm: None,
            ..tool_6_35_upcut()
        };
        let fields: Vec<&str> = bare.unset_values().iter().map(|u| u.field).collect();
        assert_eq!(fields, ["feed_mm_min", "plunge_mm_min", "rpm"]);
    }
}
