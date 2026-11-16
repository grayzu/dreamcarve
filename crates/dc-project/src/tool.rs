// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! What a [`Tool`] derives from its stored values
//! (`docs/specs/phase-3/design.md`, "Data model → Tapered ball nose"), and
//! the values it still needs ([`Tool::unset_values`]).

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
