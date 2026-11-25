// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/simulation/tools.ts (`cutterSurfaceZ`)

//! The shapes cutters have (`docs/specs/phase-6/design.md`, "Algorithms
//! → Cutter shapes"; functional capability 110).
//!
//! A cutter is its lower surface: the height above the tip at each
//! distance from the axis, out to half the cutting diameter, past which
//! nothing is cut. Upstream's flat, ball and V profiles are kept, and its
//! drill, a flat cylinder. Added: the tapered ball nose, which upstream
//! does not have and phase 3's [`BitProfile`] defines, and the engraving
//! bit, whose kind holds no shape and so is a plain cylinder too. No
//! angle is invented (S22). The profiles have no handedness, so nothing
//! here changes for Y-up.

use dc_geometry::profile::BitProfile;
use dc_project::{Tool, ToolKind};

use crate::SimError;

/// How far past the radius a point still counts as under the cutter, mm:
/// upstream's slack, which keeps a cell exactly on the rim.
pub(crate) const RIM_SLACK_MM: f64 = 1e-9;

/// A cutter's lower surface. `radius_mm` is half the cutting diameter;
/// nothing is cut beyond it, and above the surface the cutter is a
/// cylinder of that radius.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cutter {
    /// A flat end: an end mill, and the plain cylinder a drill or an
    /// engraving bit is shown as.
    Flat {
        /// The radius, mm.
        radius_mm: f64,
    },
    /// A hemisphere under a cylinder: a ball nose.
    Ball {
        /// The radius, mm.
        radius_mm: f64,
    },
    /// A cone, point down, up to the radius: a V-bit.
    Vee {
        /// `tan(α/2)` for an included angle `α`.
        slope: f64,
        /// The radius, mm.
        radius_mm: f64,
    },
    /// A ball tangent to a cone, capped at the radius: a tapered ball
    /// nose, as [`BitProfile::TaperedBall`] has it.
    TaperedBall {
        /// The ball's radius, mm.
        tip_radius_mm: f64,
        /// `tan β`, `β` the taper per side.
        slope: f64,
        /// Half the cutting diameter, mm.
        radius_mm: f64,
    },
}

impl Cutter {
    /// Half the cutting diameter, mm.
    #[must_use]
    pub const fn radius_mm(&self) -> f64 {
        match *self {
            Self::Flat { radius_mm }
            | Self::Ball { radius_mm }
            | Self::Vee { radius_mm, .. }
            | Self::TaperedBall { radius_mm, .. } => radius_mm,
        }
    }

    /// The surface's height above the tip `d_mm` from the axis, mm;
    /// `None` past the radius, where nothing is cut.
    #[must_use]
    pub fn height_mm(&self, d_mm: f64) -> Option<f64> {
        let radius_mm = self.radius_mm();
        // A distance that is no number is under nothing.
        if d_mm.is_nan() || d_mm > radius_mm + RIM_SLACK_MM {
            return None;
        }
        let d_mm = d_mm.clamp(0.0, radius_mm);
        Some(match *self {
            Self::Flat { .. } => 0.0,
            Self::Ball { radius_mm } => {
                radius_mm - ((radius_mm - d_mm) * (radius_mm + d_mm)).max(0.0).sqrt()
            }
            Self::Vee { slope, .. } => d_mm / slope,
            Self::TaperedBall { .. } => {
                let profile = self.tapered()?;
                // At the cap the profile has no one height: the cone's
                // top edge is the least.
                profile
                    .height_mm(d_mm)
                    .or_else(|| profile.cap_height_mm())?
            }
        })
    }

    /// A tapered ball nose's profile; `None` for any other cutter.
    pub(crate) const fn tapered(&self) -> Option<BitProfile> {
        match *self {
            Self::TaperedBall {
                tip_radius_mm,
                slope,
                radius_mm,
            } => Some(BitProfile::TaperedBall {
                tip_radius_mm,
                slope,
                cap_radius_mm: radius_mm,
            }),
            Self::Flat { .. } | Self::Ball { .. } | Self::Vee { .. } => None,
        }
    }
}

/// The shape a tool cuts with in a simulation, and whether it is the
/// tool's own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutterShape {
    /// The surface.
    pub cutter: Cutter,
    /// Whether the tool's kind holds no shape, so that it is shown as a
    /// plain cylinder: a drill, whose point angle the project does not
    /// hold, and an engraving bit. The Result says so (capability 110).
    pub plain: bool,
}

impl CutterShape {
    /// The shape `tool` cuts with.
    ///
    /// # Errors
    ///
    /// [`SimError::NoShape`] when the tool's diameter is no size, a
    /// V-bit's angle is out of range, or a tapered ball nose has a value
    /// unset or out of range. Validation refuses each for a tool an
    /// enabled operation uses.
    pub fn of(tool: &Tool) -> Result<Self, SimError> {
        let no_shape = || SimError::NoShape {
            tool: tool.name.clone(),
        };
        let radius_mm = tool.diameter_mm / 2.0;
        if !(radius_mm.is_finite() && radius_mm > 0.0) {
            return Err(no_shape());
        }
        let (cutter, plain) = match tool.cutter {
            ToolKind::EndMill => (Cutter::Flat { radius_mm }, false),
            ToolKind::Drill | ToolKind::Engraver => (Cutter::Flat { radius_mm }, true),
            ToolKind::BallNose => (Cutter::Ball { radius_mm }, false),
            ToolKind::VBit { .. } | ToolKind::TaperedBallNose { .. } => {
                let cutter = match tool.carve_profile().ok_or_else(no_shape)? {
                    BitProfile::VBit { slope } => Cutter::Vee { slope, radius_mm },
                    BitProfile::TaperedBall {
                        tip_radius_mm,
                        slope,
                        cap_radius_mm,
                    } => Cutter::TaperedBall {
                        tip_radius_mm,
                        slope,
                        radius_mm: cap_radius_mm,
                    },
                };
                (cutter, false)
            }
        };
        Ok(Self { cutter, plain })
    }
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
    use dc_project::test_support::{tool_6_35_upcut, tool_tbn_ultra, tool_v_bit_90};

    fn tool_of(cutter: ToolKind, diameter_mm: f64) -> Tool {
        Tool {
            cutter,
            diameter_mm,
            ..tool_6_35_upcut()
        }
    }

    /// Capability 110's table: each kind of tool cuts with its shape.
    #[test]
    fn each_tool_kind_has_its_shape() {
        let end_mill = CutterShape::of(&tool_6_35_upcut()).unwrap();
        assert_eq!(end_mill.cutter, Cutter::Flat { radius_mm: 3.175 });
        assert!(!end_mill.plain);

        let ball = CutterShape::of(&tool_of(ToolKind::BallNose, 6.0)).unwrap();
        assert_eq!(ball.cutter, Cutter::Ball { radius_mm: 3.0 });
        assert!(!ball.plain);

        let v = tool_v_bit_90();
        let shape = CutterShape::of(&v).unwrap();
        let Cutter::Vee { slope, radius_mm } = shape.cutter else {
            panic!("a V-bit is a cone, not {:?}", shape.cutter);
        };
        assert_approx_eq!(slope, 1.0, 1e-12);
        assert_approx_eq!(radius_mm, v.diameter_mm / 2.0, 0.0);
        assert!(!shape.plain);

        let tbn = tool_tbn_ultra();
        let shape = CutterShape::of(&tbn).unwrap();
        assert_eq!(shape.cutter.tapered(), tbn.carve_profile());
        assert!(!shape.plain);
    }

    /// Acceptance 137: a drill has no point angle in the project and an
    /// engraving bit no shape, so both are flat cylinders and say so.
    /// Upstream draws its drill the same way (`tools.ts`, the `drill`
    /// case).
    #[test]
    fn a_drill_and_an_engraver_are_plain_cylinders() {
        for kind in [ToolKind::Drill, ToolKind::Engraver] {
            let shape = CutterShape::of(&tool_of(kind.clone(), 5.0)).unwrap();
            assert_eq!(shape.cutter, Cutter::Flat { radius_mm: 2.5 }, "{kind:?}");
            assert!(shape.plain, "{kind:?}");
        }
    }

    /// Ported from `cutterSurfaceZ`'s table (the survey, section 3): with
    /// the tip at 0, a flat end is 0 everywhere, a ball `r − √(r² − d²)`,
    /// a V `d / tan(α/2)`.
    #[test]
    fn heights_are_the_profiles() {
        let flat = Cutter::Flat { radius_mm: 1.5 };
        let ball = Cutter::Ball { radius_mm: 2.0 };
        let v60 = Cutter::Vee {
            slope: 30.0_f64.to_radians().tan(),
            radius_mm: 3.0,
        };
        let v90 = Cutter::Vee {
            slope: 1.0,
            radius_mm: 3.0,
        };
        for d in [0.0, 0.3, 1.0, 1.5] {
            assert_approx_eq!(flat.height_mm(d).unwrap(), 0.0, 0.0);
            assert_approx_eq!(
                ball.height_mm(d).unwrap(),
                2.0 - (4.0 - d * d).sqrt(),
                1e-12
            );
            assert_approx_eq!(
                v60.height_mm(d).unwrap(),
                d / 30.0_f64.to_radians().tan(),
                1e-12
            );
            assert_approx_eq!(v90.height_mm(d).unwrap(), d, 1e-12);
        }
        // A ball is its radius high at its rim.
        assert_approx_eq!(ball.height_mm(2.0).unwrap(), 2.0, 1e-12);
    }

    /// Acceptance 135: a tapered ball nose is its ball inside the tangent
    /// circle and its cone outside, at the heights phase 3's profile
    /// gives, up to the top edge of the cone at the cutting radius.
    #[test]
    fn a_tapered_ball_nose_is_ball_then_cone() {
        let cutter = CutterShape::of(&tool_tbn_ultra()).unwrap().cutter;
        let profile = cutter.tapered().unwrap();
        let (rho, w_t) = (0.381, profile.tangent_half_width_mm());
        let in_ball = 0.2;
        assert!(in_ball < w_t);
        assert_approx_eq!(
            cutter.height_mm(in_ball).unwrap(),
            rho - (rho * rho - in_ball * in_ball).sqrt(),
            1e-12
        );
        let on_cone = 1.0;
        assert_approx_eq!(
            cutter.height_mm(on_cone).unwrap(),
            on_cone / profile.slope() - profile.apex_offset_mm(),
            1e-12
        );
        assert_approx_eq!(
            cutter.height_mm(cutter.radius_mm()).unwrap(),
            profile.cap_height_mm().unwrap(),
            1e-9
        );
    }

    /// Upstream returns nothing past `radius + 1e-9`; so does every shape
    /// here, and a distance that is no number is under nothing.
    #[test]
    fn nothing_is_cut_past_the_radius() {
        let shapes = [
            Cutter::Flat { radius_mm: 3.0 },
            Cutter::Ball { radius_mm: 3.0 },
            Cutter::Vee {
                slope: 1.0,
                radius_mm: 3.0,
            },
            CutterShape::of(&tool_tbn_ultra()).unwrap().cutter,
        ];
        for cutter in shapes {
            let r = cutter.radius_mm();
            assert!(cutter.height_mm(r).is_some(), "{cutter:?} at its rim");
            assert!(cutter.height_mm(r + 1e-6).is_none(), "{cutter:?} past it");
            assert!(cutter.height_mm(f64::NAN).is_none(), "{cutter:?}");
        }
    }

    #[test]
    fn a_tool_with_no_size_has_no_shape() {
        let unset = tool_of(
            ToolKind::TaperedBallNose {
                tip_diameter_mm: None,
                taper_angle_deg: Some(6.7),
                flute_length_mm: Some(19.05),
            },
            6.35,
        );
        let flat_v = tool_of(
            ToolKind::VBit {
                included_angle_deg: 180.0,
            },
            6.35,
        );
        for tool in [unset, flat_v, tool_of(ToolKind::EndMill, 0.0)] {
            assert!(
                matches!(CutterShape::of(&tool), Err(SimError::NoShape { .. })),
                "{:?} of {} mm",
                tool.cutter,
                tool.diameter_mm
            );
        }
    }
}
