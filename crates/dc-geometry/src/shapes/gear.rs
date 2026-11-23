// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/sketch/gearProfile.ts
//   src/sketch/gearProfile.test.ts
//   src/cannedShapes/generators/gear.ts

//! The outline of an external spur gear (`docs/specs/phase-5/design.md`,
//! "Algorithms → Shape outlines"; functional capability 86).
//!
//! The outline is straight segments only: each flank and each rounded
//! root or crest is sampled at five points, as upstream samples it. A
//! tooth is centred on +X and the outline runs counter-clockwise from
//! that tooth's left root shoulder, which in Y-up is the shoulder below
//! the X axis. The bore, when the gear has one, is a second outline: a
//! clockwise circle, a hole to every operation.
//!
//! Re-derived for Y-up: upstream's angles increase clockwise on its
//! Y-down screen and counter-clockwise here; the numbers are the same,
//! so the outline is upstream's mirrored top to bottom, which for a gear
//! with a tooth on +X is the same gear.
//!
//! Changed from upstream: sizes are checked, not clamped
//! ([`Gear::problems`] says what is wrong; upstream first forces the
//! tooth count, the pressure angle and negative sizes into range, then
//! validates); the gear is centred on the origin, since an instance's
//! transform places it; an absent bore is `None`, not zero.

use crate::point::Point;
use crate::polyline::{Polyline, Vertex};
use serde::{Deserialize, Serialize};
use std::f64::consts::{PI, TAU};

/// The fewest teeth a gear can have (`GEAR_MIN_TEETH`).
pub const MIN_TEETH: u32 = 6;
/// The most teeth a gear can have (`GEAR_MAX_TEETH`).
pub const MAX_TEETH: u32 = 200;
/// The tooth count of a new gear (`DEFAULT_GEAR_TEETH`).
pub const DEFAULT_TEETH: u32 = 24;
/// The smallest pressure angle, degrees.
pub const MIN_PRESSURE_ANGLE_DEG: f64 = 10.0;
/// The largest pressure angle, degrees.
pub const MAX_PRESSURE_ANGLE_DEG: f64 = 35.0;
/// The pressure angle of a new gear, degrees
/// (`DEFAULT_GEAR_PRESSURE_ANGLE_DEG`).
pub const DEFAULT_PRESSURE_ANGLE_DEG: f64 = 20.0;

/// The points an involute flank is sampled at, past its start.
const INVOLUTE_FLANK_SAMPLES: u32 = 5;
/// The points a rounded root is sampled at, between its shoulders.
const ROOT_ARC_SAMPLES: u32 = 5;
/// The points a rounded crest is sampled at, between its shoulders.
const CREST_ARC_SAMPLES: u32 = 5;
/// Points closer than this are one point, millimetres (upstream's
/// `addDistinctPoint`).
const DISTINCT_MM: f64 = 1e-9;

/// The form of a gear tooth's sides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GearFlanks {
    /// An involute of the base circle, as cut gears have.
    Involute,
    /// Straight from root to tip.
    Straight,
}

/// The form of the floor between two gear teeth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GearRoot {
    /// Rounded into the flanks.
    Rounded,
    /// Flat, meeting the flanks at a corner.
    Flat,
    /// The flanks meet at a point on the root circle.
    Sharp,
}

/// The form of a gear tooth's tip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GearCrest {
    /// Flat, on the outside circle.
    Flat,
    /// Rounded into the flanks.
    Rounded,
}

/// The sizes of a gear (`GearProfileParams`, less its centre).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Gear {
    /// How many teeth, [`MIN_TEETH`] to [`MAX_TEETH`].
    pub teeth: u32,
    /// The radius of the circle through the tips of the teeth,
    /// millimetres.
    pub outside_radius_mm: f64,
    /// From a tooth's tip to its root, millimetres.
    pub whole_depth_mm: f64,
    /// The form of the flanks.
    pub flanks: GearFlanks,
    /// The pressure angle, degrees; read for involute flanks only.
    pub pressure_angle_deg: f64,
    /// The form of the root.
    pub root: GearRoot,
    /// How far a rounded root's shoulders stand above the root circle,
    /// millimetres.
    pub root_fillet_mm: f64,
    /// The form of the crest.
    pub crest: GearCrest,
    /// How far a rounded crest's shoulders sit below the outside circle,
    /// millimetres.
    pub crest_radius_mm: f64,
    /// The diameter of the centre hole, millimetres; `None` for no bore.
    pub bore_diameter_mm: Option<f64>,
}

/// Why a [`Gear`]'s sizes give no gear (`validateGearProfileParams`).
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum GearProblem {
    /// The outside radius is not a positive number.
    #[error("the outside diameter must be greater than 0")]
    OutsideRadius,
    /// The tooth count is out of range.
    #[error("the tooth count must be {MIN_TEETH} to {MAX_TEETH}")]
    Teeth,
    /// The whole depth is not a positive number.
    #[error("the whole depth must be greater than 0")]
    WholeDepth,
    /// The teeth are deeper than the gear's radius.
    #[error("the whole depth must be smaller than the outside radius")]
    WholeDepthPastCentre,
    /// The pressure angle of involute flanks is out of range.
    #[error(
        "the pressure angle must be {MIN_PRESSURE_ANGLE_DEG} to {MAX_PRESSURE_ANGLE_DEG} degrees"
    )]
    PressureAngle,
    /// The two involute flanks of a tooth would meet below its crest.
    #[error(
        "the pressure angle is too high for this tooth count; the involute flanks would cross at the crest"
    )]
    FlanksCross,
    /// A rounded root's fillet is negative or not a number.
    #[error("the root fillet must be 0 or greater")]
    RootFillet,
    /// A rounded root's fillet does not fit between the teeth.
    #[error("the root fillet is too large for the tooth spacing; at most {max_mm:.3} mm fits")]
    RootFilletTooLarge {
        /// The largest fillet that fits, millimetres.
        max_mm: f64,
    },
    /// A rounded crest's radius is negative or not a number.
    #[error("the crest radius must be 0 or greater")]
    CrestRadius,
    /// A rounded crest's radius does not fit on the tooth.
    #[error("the crest radius is too large for the tooth spacing; at most {max_mm:.3} mm fits")]
    CrestRadiusTooLarge {
        /// The largest radius that fits, millimetres.
        max_mm: f64,
    },
    /// The bore is not a positive number.
    #[error("the bore diameter must be greater than 0, or left out for no bore")]
    Bore,
    /// The bore reaches the roots of the teeth.
    #[error("the bore diameter must be smaller than the root diameter, {root_diameter_mm:.3} mm")]
    BoreTooLarge {
        /// The root circle's diameter, millimetres.
        root_diameter_mm: f64,
    },
}

/// Which of a gear's sizes a [`GearProblem`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GearSize {
    /// The outside radius or diameter.
    Outside,
    /// The tooth count.
    Teeth,
    /// The whole depth.
    WholeDepth,
    /// The pressure angle.
    PressureAngle,
    /// The root fillet.
    RootFillet,
    /// The crest radius.
    CrestRadius,
    /// The bore diameter.
    Bore,
}

impl GearProblem {
    /// The size at fault.
    #[must_use]
    pub const fn size(&self) -> GearSize {
        match self {
            Self::OutsideRadius => GearSize::Outside,
            Self::Teeth => GearSize::Teeth,
            Self::WholeDepth | Self::WholeDepthPastCentre => GearSize::WholeDepth,
            Self::PressureAngle | Self::FlanksCross => GearSize::PressureAngle,
            Self::RootFillet | Self::RootFilletTooLarge { .. } => GearSize::RootFillet,
            Self::CrestRadius | Self::CrestRadiusTooLarge { .. } => GearSize::CrestRadius,
            Self::Bore | Self::BoreTooLarge { .. } => GearSize::Bore,
        }
    }
}

impl Gear {
    /// A gear of `outside_radius_mm` with upstream's defaults
    /// (`defaultGearCreationParams`): 24 teeth, involute flanks at 20°, a
    /// whole depth of 2.25 modules, a rounded root with a fillet of 0.15
    /// modules, a flat crest (its radius, unused, 0.2 modules), no bore.
    #[must_use]
    pub fn with_defaults(outside_radius_mm: f64) -> Self {
        let module = module_mm(outside_radius_mm, DEFAULT_TEETH);
        Self {
            teeth: DEFAULT_TEETH,
            outside_radius_mm,
            whole_depth_mm: module * 2.25,
            flanks: GearFlanks::Involute,
            pressure_angle_deg: DEFAULT_PRESSURE_ANGLE_DEG,
            root: GearRoot::Rounded,
            root_fillet_mm: module * 0.15,
            crest: GearCrest::Flat,
            crest_radius_mm: module * 0.2,
            bore_diameter_mm: None,
        }
    }

    /// The module: the pitch diameter per tooth, millimetres
    /// (`gearModule`).
    #[must_use]
    pub fn module_mm(&self) -> f64 {
        module_mm(self.outside_radius_mm, self.teeth)
    }

    /// The pitch circle's radius, millimetres (`gearPitchRadius`).
    #[must_use]
    pub fn pitch_radius_mm(&self) -> f64 {
        self.module_mm() * f64::from(self.teeth) / 2.0
    }

    /// The root circle's radius, millimetres (`gearRootRadius`).
    #[must_use]
    pub fn root_radius_mm(&self) -> f64 {
        self.outside_radius_mm - self.whole_depth_mm
    }

    /// The largest root fillet that fits between the teeth, millimetres
    /// (`maxGearRootFilletRadius`).
    #[must_use]
    pub fn max_root_fillet_mm(&self) -> f64 {
        let root_radius = self.root_radius_mm();
        if !is_positive(root_radius) || self.teeth == 0 {
            return 0.0;
        }
        let pitch_at_root = TAU * root_radius / f64::from(self.teeth);
        (self.whole_depth_mm * 0.45)
            .min(pitch_at_root * 0.2)
            .max(0.0)
    }

    /// The largest crest radius that fits on a tooth, millimetres
    /// (`maxGearCrestRadius`).
    #[must_use]
    pub fn max_crest_radius_mm(&self) -> f64 {
        if !is_positive(self.outside_radius_mm) || self.teeth == 0 {
            return 0.0;
        }
        let pitch_at_outside = TAU * self.outside_radius_mm / f64::from(self.teeth);
        (self.whole_depth_mm * 0.35)
            .min(pitch_at_outside * 0.12)
            .max(0.0)
    }

    /// Everything wrong with these sizes, in upstream's order; empty for
    /// a gear that can be drawn.
    #[must_use]
    pub fn problems(&self) -> Vec<GearProblem> {
        let mut problems = Vec::new();
        let positive = is_positive;
        if !positive(self.outside_radius_mm) {
            problems.push(GearProblem::OutsideRadius);
        }
        if !(MIN_TEETH..=MAX_TEETH).contains(&self.teeth) {
            problems.push(GearProblem::Teeth);
        }
        if !positive(self.whole_depth_mm) {
            problems.push(GearProblem::WholeDepth);
        }
        let root_radius = self.root_radius_mm();
        if self.outside_radius_mm.is_finite()
            && self.whole_depth_mm.is_finite()
            && root_radius <= 0.0
        {
            problems.push(GearProblem::WholeDepthPastCentre);
        }
        if self.flanks == GearFlanks::Involute
            && !(MIN_PRESSURE_ANGLE_DEG..=MAX_PRESSURE_ANGLE_DEG).contains(&self.pressure_angle_deg)
        {
            problems.push(GearProblem::PressureAngle);
        }
        if self.involute_crest_would_cross() {
            problems.push(GearProblem::FlanksCross);
        }
        if self.root == GearRoot::Rounded {
            let max_mm = self.max_root_fillet_mm();
            if !(self.root_fillet_mm.is_finite() && self.root_fillet_mm >= 0.0) {
                problems.push(GearProblem::RootFillet);
            } else if self.root_fillet_mm > max_mm {
                problems.push(GearProblem::RootFilletTooLarge { max_mm });
            }
        }
        if self.crest == GearCrest::Rounded {
            let max_mm = self.max_crest_radius_mm();
            if !(self.crest_radius_mm.is_finite() && self.crest_radius_mm >= 0.0) {
                problems.push(GearProblem::CrestRadius);
            } else if self.crest_radius_mm > max_mm {
                problems.push(GearProblem::CrestRadiusTooLarge { max_mm });
            }
        }
        if let Some(bore_diameter_mm) = self.bore_diameter_mm {
            if !positive(bore_diameter_mm) {
                problems.push(GearProblem::Bore);
            } else if bore_diameter_mm >= root_radius * 2.0 {
                problems.push(GearProblem::BoreTooLarge {
                    root_diameter_mm: root_radius * 2.0,
                });
            }
        }
        problems
    }

    /// The gear's outline, and after it its bore when it has one
    /// (`buildGearProfile`, and the bore of `generators/gear.ts`).
    ///
    /// # Errors
    ///
    /// The first of [`Gear::problems`], when the sizes give no gear.
    pub fn outline(&self) -> Result<Vec<Polyline>, GearProblem> {
        if let Some(problem) = self.problems().into_iter().next() {
            return Err(problem);
        }
        let mut points: Vec<Point> = Vec::new();
        for tooth in 0..self.teeth {
            let built = match self.flanks {
                GearFlanks::Straight => self.straight_tooth(tooth),
                GearFlanks::Involute => self.involute_tooth(tooth),
            };
            for point in built.points {
                add_distinct(&mut points, point);
            }
            self.add_root_transition(&mut points, built.root_right_rad, built.next_root_left_rad);
        }
        // The last root's end can land on the first tooth's start.
        if let (Some(first), Some(last)) = (points.first(), points.last())
            && points.len() > 1
            && first.distance_to(*last) < DISTINCT_MM
        {
            points.pop();
        }
        let vertices = points.iter().map(|p| Vertex::line(p.x, p.y)).collect();
        let mut outlines = vec![Polyline::new(vertices, true)];
        if let Some(bore_diameter_mm) = self.bore_diameter_mm {
            outlines
                .push(Polyline::circle(Point::new(0.0, 0.0), bore_diameter_mm / 2.0).reversed());
        }
        Ok(outlines)
    }

    /// Whether a tooth's two involute flanks would meet before they
    /// reach its crest (`involuteCrestWouldCross`).
    fn involute_crest_would_cross(&self) -> bool {
        if self.flanks != GearFlanks::Involute
            || self.teeth == 0
            || !is_positive(self.outside_radius_mm)
        {
            return false;
        }
        let pitch_radius = self.pitch_radius_mm();
        let base_radius = pitch_radius * self.pressure_angle_deg.to_radians().cos();
        let flank_end_radius = match self.crest {
            GearCrest::Rounded => self
                .root_shoulder_radius_mm()
                .max(self.crest_shoulder_radius_mm()),
            GearCrest::Flat => self.outside_radius_mm,
        };
        let roll_at_crest = involute_angle(base_radius, flank_end_radius)
            - involute_angle(base_radius, pitch_radius);
        let half_tooth_at_pitch = PI / (2.0 * f64::from(self.teeth));
        roll_at_crest > half_tooth_at_pitch + 1e-9
    }

    /// How far the root's shoulders stand above the root circle
    /// (`rootShoulderLift`).
    fn root_shoulder_lift_mm(&self) -> f64 {
        match self.root {
            GearRoot::Flat => 0.0,
            GearRoot::Sharp => self.max_root_fillet_mm(),
            GearRoot::Rounded => self.root_fillet_mm.min(self.max_root_fillet_mm()),
        }
    }

    fn root_shoulder_radius_mm(&self) -> f64 {
        self.root_radius_mm() + self.root_shoulder_lift_mm()
    }

    /// How far the crest's shoulders sit below the outside circle
    /// (`crestShoulderDrop`).
    fn crest_shoulder_drop_mm(&self) -> f64 {
        match self.crest {
            GearCrest::Flat => 0.0,
            GearCrest::Rounded => self.crest_radius_mm.min(self.max_crest_radius_mm()),
        }
    }

    fn crest_shoulder_radius_mm(&self) -> f64 {
        self.outside_radius_mm - self.crest_shoulder_drop_mm()
    }

    /// The crest from its left shoulder, already in `points`, to its
    /// right one (`addCrestTransition`).
    fn add_crest_transition(
        &self,
        points: &mut Vec<Point>,
        left_rad: f64,
        centre_rad: f64,
        right_rad: f64,
    ) {
        if self.crest == GearCrest::Flat {
            add_distinct(points, point_at(self.outside_radius_mm, right_rad));
            return;
        }
        let shoulder_radius = self.crest_shoulder_radius_mm();
        for step in 1..=CREST_ARC_SAMPLES {
            let t = f64::from(step) / f64::from(CREST_ARC_SAMPLES + 1);
            let angle = if t <= 0.5 {
                lerp(left_rad, centre_rad, t * 2.0)
            } else {
                lerp(centre_rad, right_rad, (t - 0.5) * 2.0)
            };
            let radius =
                shoulder_radius + (self.outside_radius_mm - shoulder_radius) * (PI * t).sin();
            add_distinct(points, point_at(radius, angle));
        }
        add_distinct(points, point_at(shoulder_radius, right_rad));
    }

    /// One tooth with involute flanks (`buildInvoluteToothPoints`).
    fn involute_tooth(&self, tooth: u32) -> Tooth {
        let pitch = TAU / f64::from(self.teeth);
        let centre = f64::from(tooth) * pitch;
        let next_centre = f64::from(tooth + 1) * pitch;
        let root_shoulder_radius = self.root_shoulder_radius_mm();
        let crest_shoulder_radius = self.crest_shoulder_radius_mm();
        let pitch_radius = self.pitch_radius_mm();
        let base_radius = pitch_radius * self.pressure_angle_deg.to_radians().cos();
        let flank_start_radius = root_shoulder_radius.max(base_radius.min(self.outside_radius_mm));
        let flank_end_radius = match self.crest {
            GearCrest::Rounded => flank_start_radius.max(crest_shoulder_radius),
            GearCrest::Flat => self.outside_radius_mm,
        };
        let pitch_involute = involute_angle(base_radius, pitch_radius);
        let half_tooth_at_pitch = PI / (2.0 * f64::from(self.teeth));
        let flank = |side: Side, radius: f64, centre: f64| -> f64 {
            let roll = involute_angle(base_radius, radius) - pitch_involute;
            match side {
                Side::Left => centre - half_tooth_at_pitch + roll,
                Side::Right => centre + half_tooth_at_pitch - roll,
            }
        };
        let left_root = flank(Side::Left, flank_start_radius, centre);
        let right_root = flank(Side::Right, flank_start_radius, centre);
        let left_crest = flank(Side::Left, flank_end_radius, centre);
        let right_crest = flank(Side::Right, flank_end_radius, centre);
        let next_root_left = flank(Side::Left, flank_start_radius, next_centre);
        let along = |step: u32| -> f64 {
            let t = f64::from(step) / f64::from(INVOLUTE_FLANK_SAMPLES);
            flank_start_radius + (flank_end_radius - flank_start_radius) * t
        };

        let mut points = Vec::new();
        add_distinct(&mut points, point_at(root_shoulder_radius, left_root));
        let undercut = flank_start_radius > root_shoulder_radius + 1e-9;
        if undercut {
            add_distinct(&mut points, point_at(flank_start_radius, left_root));
        }
        for step in 1..=INVOLUTE_FLANK_SAMPLES {
            let radius = along(step);
            add_distinct(
                &mut points,
                point_at(radius, flank(Side::Left, radius, centre)),
            );
        }
        self.add_crest_transition(&mut points, left_crest, centre, right_crest);
        for step in (0..INVOLUTE_FLANK_SAMPLES).rev() {
            let radius = along(step);
            add_distinct(
                &mut points,
                point_at(radius, flank(Side::Right, radius, centre)),
            );
        }
        if undercut {
            add_distinct(&mut points, point_at(root_shoulder_radius, right_root));
        }
        Tooth {
            points,
            root_right_rad: right_root,
            next_root_left_rad: next_root_left,
        }
    }

    /// One tooth with straight flanks (`buildStraightToothPoints`).
    fn straight_tooth(&self, tooth: u32) -> Tooth {
        let pitch = TAU / f64::from(self.teeth);
        let centre = f64::from(tooth) * pitch;
        let next_centre = f64::from(tooth + 1) * pitch;
        let root_shoulder_radius = self.root_shoulder_radius_mm();
        let crest_shoulder_radius = self.crest_shoulder_radius_mm();
        let root_half = pitch * 0.32;
        let crest_half = pitch * 0.18;
        let mut points = Vec::new();
        add_distinct(
            &mut points,
            point_at(root_shoulder_radius, centre - root_half),
        );
        add_distinct(
            &mut points,
            point_at(crest_shoulder_radius, centre - crest_half),
        );
        self.add_crest_transition(
            &mut points,
            centre - crest_half,
            centre,
            centre + crest_half,
        );
        add_distinct(
            &mut points,
            point_at(root_shoulder_radius, centre + root_half),
        );
        Tooth {
            points,
            root_right_rad: centre + root_half,
            next_root_left_rad: next_centre - root_half,
        }
    }

    /// The root from one tooth's right shoulder to the next tooth's left
    /// one (`addRootTransition`).
    fn add_root_transition(&self, points: &mut Vec<Point>, from_rad: f64, to_rad: f64) {
        let root_radius = self.root_radius_mm();
        let shoulder_radius = self.root_shoulder_radius_mm();
        match self.root {
            GearRoot::Flat => {}
            GearRoot::Sharp => {
                add_distinct(points, point_at(root_radius, lerp(from_rad, to_rad, 0.5)));
            }
            GearRoot::Rounded => {
                for step in 1..=ROOT_ARC_SAMPLES {
                    let t = f64::from(step) / f64::from(ROOT_ARC_SAMPLES + 1);
                    let radius = shoulder_radius - (shoulder_radius - root_radius) * (PI * t).sin();
                    add_distinct(points, point_at(radius, lerp(from_rad, to_rad, t)));
                }
            }
        }
    }
}

/// A flank of a tooth, looking outward from the gear's centre.
#[derive(Clone, Copy)]
enum Side {
    /// The flank at the smaller angle.
    Left,
    /// The flank at the larger angle.
    Right,
}

/// One tooth's points and where the root after it starts and ends.
struct Tooth {
    points: Vec<Point>,
    root_right_rad: f64,
    next_root_left_rad: f64,
}

/// Whether `value` is a finite number above zero.
fn is_positive(value: f64) -> bool {
    value.is_finite() && value > 0.0
}

/// `gearModule`: the outside diameter over the tooth count plus two.
fn module_mm(outside_radius_mm: f64, teeth: u32) -> f64 {
    outside_radius_mm * 2.0 / (f64::from(teeth) + 2.0)
}

fn point_at(radius_mm: f64, angle_rad: f64) -> Point {
    Point::new(radius_mm * angle_rad.cos(), radius_mm * angle_rad.sin())
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

fn add_distinct(points: &mut Vec<Point>, point: Point) {
    if points
        .last()
        .is_some_and(|last| last.distance_to(point) < DISTINCT_MM)
    {
        return;
    }
    points.push(point);
}

/// `involuteParam`: the roll parameter at `radius` of the involute of a
/// circle of `base_radius`; 0 at or inside the base circle.
fn involute_param(base_radius: f64, radius: f64) -> f64 {
    if base_radius <= 0.0 || radius <= base_radius {
        return 0.0;
    }
    ((radius * radius) / (base_radius * base_radius) - 1.0).sqrt()
}

/// `involuteAngle`: the involute function, `t − atan t`.
fn involute_angle(base_radius: f64, radius: f64) -> f64 {
    let t = involute_param(base_radius, radius);
    t - t.atan()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f64 = 1e-6;

    /// Upstream's `baseParams`: an outside radius of 20 with the
    /// defaults. (Its centre of (3, −2) is the instance's transform
    /// here.)
    fn base() -> Gear {
        Gear::with_defaults(20.0)
    }

    fn radii(outline: &Polyline) -> Vec<f64> {
        outline
            .vertices()
            .iter()
            .map(|v| v.point().distance_to(Point::new(0.0, 0.0)))
            .collect()
    }

    /// The radii of the outline's points whose angle, 0 to 2π, is in the
    /// window (`radiiInAngleWindow`).
    fn radii_in_window(outline: &Polyline, from_rad: f64, to_rad: f64) -> Vec<f64> {
        outline
            .vertices()
            .iter()
            .filter(|v| {
                let angle = v.y.atan2(v.x).rem_euclid(TAU);
                angle >= from_rad - EPSILON && angle <= to_rad + EPSILON
            })
            .map(|v| v.point().distance_to(Point::new(0.0, 0.0)))
            .collect()
    }

    fn max(values: &[f64]) -> f64 {
        values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
    }

    fn min(values: &[f64]) -> f64 {
        values.iter().copied().fold(f64::INFINITY, f64::min)
    }

    /// `testDefaultProfileIsClosed`: a closed outline with more than six
    /// sampled segments a tooth.
    #[test]
    fn default_profile_is_closed() {
        let gear = Gear {
            teeth: 24,
            ..base()
        };
        let outlines = gear.outline().unwrap();
        assert_eq!(outlines.len(), 1, "no bore");
        let outline = &outlines[0];
        assert!(outline.is_closed());
        assert!(
            outline.segments().count() > 24 * 6,
            "{} segments",
            outline.segments().count()
        );
        assert!(outline.segments().all(|s| s.is_line()));
        assert!(outline.signed_area_mm2() > 0.0, "counter-clockwise");
    }

    /// `testProfileRadiiStayInsideOutsideDiameter`: the outline reaches
    /// the outside circle and the root circle and passes neither.
    #[test]
    fn profile_radii_stay_between_the_root_and_outside_circles() {
        let gear = Gear {
            teeth: 18,
            ..base()
        };
        let radii = radii(&gear.outline().unwrap()[0]);
        assert!((max(&radii) - gear.outside_radius_mm).abs() <= EPSILON);
        assert!((min(&radii) - gear.root_radius_mm()).abs() <= EPSILON);
    }

    /// `testStraightFlankProfileBuildsTrapezoidalTeeth`.
    #[test]
    fn straight_flank_profile_builds_trapezoidal_teeth() {
        let gear = Gear {
            teeth: 12,
            flanks: GearFlanks::Straight,
            crest: GearCrest::Rounded,
            root: GearRoot::Flat,
            ..base()
        };
        let outline = &gear.outline().unwrap()[0];
        assert!(outline.is_closed());
        assert!(outline.segments().count() >= 12 * 4);
    }

    /// `testSharpRootCreatesApexBelowShoulders`: between two teeth the
    /// outline dips to a point on the root circle, its shoulders above.
    #[test]
    fn sharp_root_creates_an_apex_below_its_shoulders() {
        let gear = Gear {
            teeth: 12,
            flanks: GearFlanks::Straight,
            root: GearRoot::Sharp,
            crest: GearCrest::Flat,
            ..base()
        };
        let outline = &gear.outline().unwrap()[0];
        let root_radius = gear.root_radius_mm();
        let shoulder = root_radius + gear.max_root_fillet_mm();
        let pitch = TAU / 12.0;
        let roots = radii_in_window(outline, pitch * 0.31, pitch * 0.69);
        assert!(roots.iter().any(|r| (r - root_radius).abs() < EPSILON));
        assert!(max(&roots) >= shoulder - EPSILON);
    }

    /// `testRootFilletRadiusChangesRoundedRoot`.
    #[test]
    fn root_fillet_radius_changes_the_rounded_root() {
        let small = Gear {
            teeth: 12,
            flanks: GearFlanks::Straight,
            root: GearRoot::Rounded,
            root_fillet_mm: 0.1,
            crest: GearCrest::Flat,
            ..base()
        };
        let large = Gear {
            root_fillet_mm: small.max_root_fillet_mm() * 0.8,
            ..small
        };
        let pitch = TAU / 12.0;
        let window =
            |gear: &Gear| radii_in_window(&gear.outline().unwrap()[0], pitch * 0.31, pitch * 0.69);
        let (small_radii, large_radii) = (window(&small), window(&large));
        assert!(max(&large_radii) > max(&small_radii) + 0.5);
        assert!(min(&large_radii) <= small.root_radius_mm() + EPSILON);
    }

    /// `testCrestRadiusChangesRoundedCrest`.
    #[test]
    fn crest_radius_changes_the_rounded_crest() {
        let small = Gear {
            teeth: 12,
            flanks: GearFlanks::Straight,
            root: GearRoot::Flat,
            crest: GearCrest::Rounded,
            crest_radius_mm: 0.1,
            ..base()
        };
        let large = Gear {
            crest_radius_mm: small.max_crest_radius_mm() * 0.9,
            ..small
        };
        let pitch = TAU / 12.0;
        let window =
            |gear: &Gear| radii_in_window(&gear.outline().unwrap()[0], pitch * 0.82, pitch * 1.18);
        let (small_radii, large_radii) = (window(&small), window(&large));
        assert!(min(&large_radii) < min(&small_radii) - 0.5);
        assert!(max(&large_radii) >= large.outside_radius_mm - EPSILON);
    }

    /// `testValidationRejectsOversizedBore`.
    #[test]
    fn validation_rejects_an_oversized_bore() {
        let gear = Gear {
            bore_diameter_mm: Some(40.0),
            ..base()
        };
        let problems = gear.problems();
        assert!(
            matches!(problems.as_slice(), [GearProblem::BoreTooLarge { .. }]),
            "{problems:?}"
        );
        assert_eq!(problems[0].size(), GearSize::Bore);
        assert!(gear.outline().is_err());
    }

    /// `testValidationRejectsOversizedCrestRadius`.
    #[test]
    fn validation_rejects_an_oversized_crest_radius() {
        let rounded = Gear {
            crest: GearCrest::Rounded,
            ..base()
        };
        let gear = Gear {
            crest_radius_mm: rounded.max_crest_radius_mm() + 0.01,
            ..rounded
        };
        let problems = gear.problems();
        assert!(
            matches!(
                problems.as_slice(),
                [GearProblem::CrestRadiusTooLarge { .. }]
            ),
            "{problems:?}"
        );
    }

    /// `testValidationRejectsCrossedInvoluteCrest`.
    #[test]
    fn validation_rejects_a_crossed_involute_crest() {
        for teeth in [6, 8, 12] {
            let gear = Gear {
                teeth,
                pressure_angle_deg: 35.0,
                crest: GearCrest::Flat,
                ..base()
            };
            assert!(
                gear.problems().contains(&GearProblem::FlanksCross),
                "{teeth} teeth: {:?}",
                gear.problems()
            );
            assert!(gear.outline().is_err(), "{teeth} teeth");
        }
    }

    /// Sizes are checked, where upstream clamps them first: each is its
    /// own problem at its own size.
    #[test]
    fn each_size_out_of_range_is_named() {
        let cases: [(Gear, GearProblem); 7] = [
            (
                Gear {
                    outside_radius_mm: 0.0,
                    ..base()
                },
                GearProblem::OutsideRadius,
            ),
            (Gear { teeth: 5, ..base() }, GearProblem::Teeth),
            (
                Gear {
                    teeth: 201,
                    ..base()
                },
                GearProblem::Teeth,
            ),
            (
                Gear {
                    whole_depth_mm: 0.0,
                    ..base()
                },
                GearProblem::WholeDepth,
            ),
            (
                Gear {
                    whole_depth_mm: 20.0,
                    ..base()
                },
                GearProblem::WholeDepthPastCentre,
            ),
            (
                Gear {
                    pressure_angle_deg: 9.9,
                    ..base()
                },
                GearProblem::PressureAngle,
            ),
            (
                Gear {
                    bore_diameter_mm: Some(0.0),
                    ..base()
                },
                GearProblem::Bore,
            ),
        ];
        for (gear, expected) in cases {
            assert!(
                gear.problems().contains(&expected),
                "{expected:?}: {:?}",
                gear.problems()
            );
        }
        // A straight-flanked gear does not read its pressure angle.
        let straight = Gear {
            flanks: GearFlanks::Straight,
            pressure_angle_deg: 80.0,
            ..base()
        };
        assert_eq!(straight.problems(), vec![]);
        let fillet = Gear {
            root_fillet_mm: 5.0,
            ..base()
        };
        assert!(matches!(
            fillet.problems().as_slice(),
            [GearProblem::RootFilletTooLarge { .. }]
        ));
    }

    /// The default gear on a 40 mm outside diameter (phase 5 acceptance
    /// 111): module 40 ÷ 26, and 24-fold symmetry, a tooth on +X.
    #[test]
    fn the_default_gear_has_24_fold_symmetry() {
        let gear = base();
        assert!((gear.module_mm() - 40.0 / 26.0).abs() < 1e-12);
        assert!((gear.pitch_radius_mm() - 40.0 / 26.0 * 12.0).abs() < 1e-12);
        let outline = &gear.outline().unwrap()[0];
        let vertices = outline.vertices();
        assert_eq!(vertices.len() % 24, 0);
        let per_tooth = vertices.len() / 24;
        let turn = crate::Transform::rotate(TAU / 24.0);
        for (i, v) in vertices.iter().enumerate() {
            let expected = turn.apply(v.point());
            let next = vertices[(i + per_tooth) % vertices.len()].point();
            assert!(expected.distance_to(next) < 1e-9, "vertex {i}");
        }
        // The tooth on +X: its flat crest's two corners are on the outside
        // circle, one above the X axis and one as far below it.
        let rightmost = vertices.iter().max_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
        assert!((rightmost.point().distance_to(Point::new(0.0, 0.0)) - 20.0).abs() < EPSILON);
        let mirrored = Point::new(rightmost.x, -rightmost.y);
        assert!(
            vertices
                .iter()
                .any(|v| v.point().distance_to(mirrored) < 1e-9),
            "the tooth on +X is symmetric about the X axis"
        );
        assert!(rightmost.y.abs() < 2.0, "{rightmost:?}");
        // The same sizes give the same vertices.
        assert_eq!(gear.outline().unwrap(), gear.outline().unwrap());
    }

    /// A bore is a second outline: a clockwise circle of its diameter
    /// (`generators/gear.ts`), a hole inside the teeth.
    #[test]
    fn a_bore_is_a_clockwise_circle_after_the_teeth() {
        let gear = Gear {
            bore_diameter_mm: Some(6.0),
            ..base()
        };
        let outlines = gear.outline().unwrap();
        assert_eq!(outlines.len(), 2);
        let bore = &outlines[1];
        assert!(bore.signed_area_mm2() < 0.0, "a hole");
        let (centre, radius) = crate::as_circle(bore, 1e-9).unwrap();
        assert!(centre.distance_to(Point::new(0.0, 0.0)) < 1e-9);
        assert!((radius - 3.0).abs() < 1e-9);
        assert_eq!(
            outlines[0],
            Gear {
                bore_diameter_mm: None,
                ..gear
            }
            .outline()
            .unwrap()[0]
        );
    }
}
