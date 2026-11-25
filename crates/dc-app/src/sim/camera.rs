// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/components/viewport3d/orbitControls.ts
//   src/components/viewport3d/viewPresets.ts

//! The 3D view's camera (phase 6 functional spec, capability 111;
//! `docs/specs/phase-6/design.md`, "The camera and the read-out").
//!
//! From upstream: an orbit about a target point by two angles and a
//! distance, in perspective at 45° up and down; a drag turns it 0.01 rad
//! a pixel, another slides it in the view's own plane, the wheel zooms
//! about the point under the pointer; the presets; a fit that keeps the
//! angles and takes the least distance at which a box's eight corners
//! are in view, with a tenth to spare; clip planes that follow the
//! distance.
//!
//! Re-derived for this world. Upstream draws its sheet `(x, y, z)` at
//! world `(x, z, y)` with Y up and its Y-down sheet running toward the
//! viewer; here the world is project space itself, X to the right, Y
//! away from the operator, Z up (ADR-0001). So the eye of yaw 0 stands
//! on the operator's side, at −Y, looking toward +Y; Front is that view,
//! Right looks from +X, and Top looks straight down with +Y up the
//! screen, as the 2D Viewport draws the stock. Upstream's polar angle
//! from the vertical is an elevation here, held from level with the bed
//! to straight down, so the eye is never under the bed.

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, FRAC_PI_6};

use dc_geometry::Point3;

use super::mat::{Mat4, Vec3};

/// The field of view, up and down, radians: upstream's 45°.
pub const FOV_Y_RAD: f64 = FRAC_PI_4;
/// How far a drag of one pixel turns the view, radians.
pub const ORBIT_RAD_PER_PX: f64 = 0.01;
/// How much room a fit leaves round what it frames.
pub const FIT_MARGIN: f64 = 1.1;
/// The nearest and the furthest the eye goes from its target, mm.
pub const DISTANCE_RANGE_MM: (f64, f64) = (1.0, 100_000.0);

/// A view by name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View3 {
    /// From the front right, 30° above the bed: upstream's isometric.
    Home,
    /// Straight down, +Y up the screen.
    Top,
    /// From the operator's side, level with the bed.
    Front,
    /// From the +X side, level with the bed.
    Right,
}

impl View3 {
    /// The view's yaw and pitch, radians.
    #[must_use]
    pub const fn angles(self) -> (f64, f64) {
        match self {
            Self::Home => (FRAC_PI_4, FRAC_PI_6),
            Self::Top => (0.0, FRAC_PI_2),
            Self::Front => (0.0, 0.0),
            Self::Right => (FRAC_PI_2, 0.0),
        }
    }
}

/// A box in space, mm: what a fit frames.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Box3 {
    /// Its corner of least X, Y and Z.
    pub min: Point3,
    /// Its corner of greatest X, Y and Z.
    pub max: Point3,
}

impl Box3 {
    /// Its centre.
    #[must_use]
    pub fn centre(&self) -> Point3 {
        Point3::new(
            f64::midpoint(self.min.x, self.max.x),
            f64::midpoint(self.min.y, self.max.y),
            f64::midpoint(self.min.z, self.max.z),
        )
    }

    /// Its eight corners.
    #[must_use]
    pub fn corners(&self) -> [Point3; 8] {
        let (a, b) = (self.min, self.max);
        [
            Point3::new(a.x, a.y, a.z),
            Point3::new(b.x, a.y, a.z),
            Point3::new(a.x, b.y, a.z),
            Point3::new(b.x, b.y, a.z),
            Point3::new(a.x, a.y, b.z),
            Point3::new(b.x, a.y, b.z),
            Point3::new(a.x, b.y, b.z),
            Point3::new(b.x, b.y, b.z),
        ]
    }

    /// The length of its diagonal, mm.
    #[must_use]
    pub fn diagonal_mm(&self) -> f64 {
        Vec3::between(self.min, self.max).length()
    }
}

/// A half line in space: where the pointer looks.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ray {
    /// Where it starts: the eye.
    pub origin: Point3,
    /// Its direction, one long.
    pub direction: Vec3,
}

impl Ray {
    /// The point `t` mm along it.
    #[must_use]
    pub fn at(&self, t: f64) -> Point3 {
        (self.direction * t).from(self.origin)
    }
}

/// An orbit about a target: where the 3D view looks from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrbitCamera {
    /// The point the eye turns about and looks at, mm.
    pub target: Point3,
    /// The eye's turn about the vertical, radians: 0 looks from the
    /// operator's side toward +Y, a quarter turn from +X toward −X.
    pub yaw_rad: f64,
    /// The eye's height as an angle, radians: 0 level with the target,
    /// a quarter turn straight above it.
    pub pitch_rad: f64,
    /// The eye's distance from the target, mm.
    pub distance_mm: f64,
    /// The picture's width and height, pixels.
    pub viewport_px: (f64, f64),
}

impl OrbitCamera {
    /// The Home view of `bounds` in a picture of `viewport_px`.
    #[must_use]
    pub fn framing(bounds: Box3, viewport_px: (f64, f64)) -> Self {
        let (yaw_rad, pitch_rad) = View3::Home.angles();
        let mut camera = Self {
            target: bounds.centre(),
            yaw_rad,
            pitch_rad,
            distance_mm: 250.0,
            viewport_px,
        };
        camera.fit(bounds);
        camera
    }

    /// The eye's right, up and forward directions, each one long.
    #[must_use]
    pub fn basis(&self) -> (Vec3, Vec3, Vec3) {
        let (sy, cy) = self.yaw_rad.sin_cos();
        let (sp, cp) = self.pitch_rad.sin_cos();
        let right = Vec3::new(cy, sy, 0.0);
        let forward = Vec3::new(-cp * sy, cp * cy, -sp);
        (right, right.cross(forward), forward)
    }

    /// Where the eye is.
    #[must_use]
    pub fn eye(&self) -> Point3 {
        let (_, _, forward) = self.basis();
        (-forward * self.distance_mm).from(self.target)
    }

    /// The picture's width over its height; 1 for a picture of no size.
    #[must_use]
    pub fn aspect(&self) -> f64 {
        let (w, h) = self.viewport_px;
        if w > 0.0 && h > 0.0 { w / h } else { 1.0 }
    }

    /// Takes up a named view, the distance and the target kept.
    pub fn preset(&mut self, view: View3) {
        (self.yaw_rad, self.pitch_rad) = view.angles();
    }

    /// The named view it is in, if it is in one exactly.
    #[must_use]
    pub fn view(&self) -> Option<View3> {
        [View3::Home, View3::Top, View3::Front, View3::Right]
            .into_iter()
            .find(|view| {
                let (yaw, pitch) = view.angles();
                (yaw - self.yaw_rad).abs() < 1e-9 && (pitch - self.pitch_rad).abs() < 1e-9
            })
    }

    /// Turns the view by a drag of (`dx_px`, `dy_px`): the stock follows
    /// the pointer sideways, and a drag down raises the eye. The eye
    /// stays between level with the target and straight above it.
    pub fn orbit(&mut self, dx_px: f64, dy_px: f64) {
        self.yaw_rad = (self.yaw_rad - dx_px * ORBIT_RAD_PER_PX).rem_euclid(std::f64::consts::TAU);
        self.pitch_rad = (self.pitch_rad + dy_px * ORBIT_RAD_PER_PX).clamp(0.0, FRAC_PI_2);
    }

    /// How many millimetres a pixel is at the target's depth.
    #[must_use]
    pub fn mm_per_px(&self) -> f64 {
        let h = self.viewport_px.1.max(1.0);
        2.0 * self.distance_mm * (FOV_Y_RAD / 2.0).tan() / h
    }

    /// Slides the view by a drag of (`dx_px`, `dy_px`) in its own
    /// plane: what is under the pointer stays under it.
    pub fn pan(&mut self, dx_px: f64, dy_px: f64) {
        let (right, up, _) = self.basis();
        let k = self.mm_per_px();
        self.target = (right * (-dx_px * k) + up * (dy_px * k)).from(self.target);
    }

    /// Zooms by `factor` (under 1 comes closer) about the pixel `at_px`,
    /// which keeps showing what it showed at the target's depth.
    pub fn zoom_about(&mut self, at_px: (f64, f64), factor: f64) {
        if !(factor.is_finite() && factor > 0.0) {
            return;
        }
        let before = self.distance_mm;
        let after = (before * factor).clamp(DISTANCE_RANGE_MM.0, DISTANCE_RANGE_MM.1);
        let k = after / before;
        let (right, up, _) = self.basis();
        let (ndc_x, ndc_y) = self.ndc(at_px);
        let half = before * (FOV_Y_RAD / 2.0).tan();
        let offset = right * (ndc_x * half * self.aspect()) + up * (ndc_y * half);
        self.target = (offset * (1.0 - k)).from(self.target);
        self.distance_mm = after;
    }

    /// Frames `bounds`: the angles kept, the target at the box's centre,
    /// and the least distance at which all eight corners are in the
    /// picture, with [`FIT_MARGIN`] to spare.
    pub fn fit(&mut self, bounds: Box3) {
        self.target = bounds.centre();
        let (right, up, forward) = self.basis();
        let tan_y = (FOV_Y_RAD / 2.0).tan();
        let tan_x = tan_y * self.aspect();
        let mut distance: f64 = 0.0;
        for corner in bounds.corners() {
            let c = Vec3::between(self.target, corner);
            // A corner `depth` nearer the eye than the target needs the
            // eye that much further back.
            let depth = -c.dot(forward);
            distance = distance
                .max(c.dot(right).abs() / tan_x + depth)
                .max(c.dot(up).abs() / tan_y + depth);
        }
        self.distance_mm = (distance * FIT_MARGIN).clamp(DISTANCE_RANGE_MM.0, DISTANCE_RANGE_MM.1);
    }

    /// The near and far clip planes, mm from the eye, for a scene whose
    /// box is `scene`: near is a hundredth of the distance, as
    /// upstream's, and far reaches past the scene.
    #[must_use]
    pub fn clip(&self, scene: Box3) -> (f64, f64) {
        let near = (self.distance_mm * 0.01).max(0.01);
        let reach = Vec3::between(self.eye(), scene.centre()).length() + scene.diagonal_mm();
        (near, (reach * 1.1).max(near * 2.0))
    }

    /// World to clip space, for a scene whose box is `scene`.
    #[must_use]
    pub fn view_projection(&self, scene: Box3) -> Mat4 {
        let (right, up, forward) = self.basis();
        let (near, far) = self.clip(scene);
        Mat4::perspective(FOV_Y_RAD, self.aspect(), near, far).after(&Mat4::view(
            self.eye(),
            right,
            up,
            forward,
        ))
    }

    /// A pixel as the picture's own coordinates: −1 to 1 across and up.
    fn ndc(&self, at_px: (f64, f64)) -> (f64, f64) {
        let (w, h) = (self.viewport_px.0.max(1.0), self.viewport_px.1.max(1.0));
        (2.0 * at_px.0 / w - 1.0, 1.0 - 2.0 * at_px.1 / h)
    }

    /// The ray from the eye through the pixel `at_px`.
    #[must_use]
    pub fn ray(&self, at_px: (f64, f64)) -> Ray {
        let (right, up, forward) = self.basis();
        let (ndc_x, ndc_y) = self.ndc(at_px);
        let tan_y = (FOV_Y_RAD / 2.0).tan();
        let direction = forward + right * (ndc_x * tan_y * self.aspect()) + up * (ndc_y * tan_y);
        Ray {
            origin: self.eye(),
            direction: direction.normalized().unwrap_or(forward),
        }
    }

    /// The pixel `p` is drawn at; `None` for a point behind the eye.
    #[must_use]
    pub fn to_screen(&self, p: Point3) -> Option<(f64, f64)> {
        let (right, up, forward) = self.basis();
        let c = Vec3::between(self.eye(), p);
        let depth = c.dot(forward);
        if depth <= 0.0 {
            return None;
        }
        let tan_y = (FOV_Y_RAD / 2.0).tan();
        let (ndc_x, ndc_y) = (
            c.dot(right) / (depth * tan_y * self.aspect()),
            c.dot(up) / (depth * tan_y),
        );
        let (w, h) = self.viewport_px;
        Some(((ndc_x + 1.0) / 2.0 * w, (1.0 - ndc_y) / 2.0 * h))
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

    /// A stock 170 × 120 × 12 mm on its bed.
    fn stock() -> Box3 {
        Box3 {
            min: Point3::new(0.0, 0.0, 0.0),
            max: Point3::new(170.0, 120.0, 12.0),
        }
    }

    fn camera(view: View3) -> OrbitCamera {
        let mut camera = OrbitCamera::framing(stock(), (800.0, 600.0));
        camera.preset(view);
        camera.fit(stock());
        camera
    }

    /// Acceptance 152: Top looks straight down with +X to the right and
    /// +Y up the screen; Front looks from the operator's side, with +X
    /// to the right and +Z up; Right looks from +X, with +Y to the
    /// right; Home looks from the front right, above.
    #[test]
    fn the_presets_look_the_way_their_names_say() {
        let centre = stock().centre();
        let on_screen = |camera: &OrbitCamera, p: Point3| camera.to_screen(p).unwrap();
        let moved = |camera: &OrbitCamera, by: Vec3| {
            let (a, b) = (
                on_screen(camera, centre),
                on_screen(camera, by.from(centre)),
            );
            (b.0 - a.0, b.1 - a.1)
        };

        let top = camera(View3::Top);
        assert!(top.eye().z > 100.0);
        assert_approx_eq!(top.eye().x, centre.x, 1e-9);
        assert_approx_eq!(top.eye().y, centre.y, 1e-9);
        let (dx, dy) = moved(&top, Vec3::new(10.0, 0.0, 0.0));
        assert!(
            dx > 1.0 && dy.abs() < 1e-9,
            "+X is to the right: {dx}, {dy}"
        );
        let (dx, dy) = moved(&top, Vec3::new(0.0, 10.0, 0.0));
        assert!(
            dy < -1.0 && dx.abs() < 1e-9,
            "+Y is up the screen: {dx}, {dy}"
        );

        let front = camera(View3::Front);
        assert!(front.eye().y < 0.0, "the operator's side");
        assert_approx_eq!(front.eye().z, centre.z, 1e-9);
        let (dx, _) = moved(&front, Vec3::new(10.0, 0.0, 0.0));
        assert!(dx > 1.0);
        let (dx, dy) = moved(&front, Vec3::new(0.0, 0.0, 5.0));
        assert!(dy < -1.0 && dx.abs() < 1e-9, "+Z is up");

        let right = camera(View3::Right);
        assert!(right.eye().x > 170.0);
        let (dx, _) = moved(&right, Vec3::new(0.0, 10.0, 0.0));
        assert!(dx > 1.0, "+Y is to the right, seen from +X");

        let home = camera(View3::Home);
        let eye = home.eye();
        assert!(
            eye.x > centre.x && eye.y < centre.y && eye.z > centre.z,
            "{eye:?}"
        );
        assert_eq!(home.view(), Some(View3::Home));
    }

    /// Acceptance 152: Fit frames the stock in every preset: every
    /// corner is in the picture with room to spare, and a tenth nearer
    /// (the least distance, without the margin) the corner that reaches
    /// furthest is on the picture's edge.
    #[test]
    fn fit_frames_the_eight_corners() {
        let reach = |camera: &OrbitCamera, viewport: (f64, f64)| {
            let mut reach: f64 = 0.0;
            for corner in stock().corners() {
                let (x, y) = camera.to_screen(corner).unwrap();
                reach = reach
                    .max((2.0 * x / viewport.0 - 1.0).abs())
                    .max((1.0 - 2.0 * y / viewport.1).abs());
            }
            reach
        };
        for view in [View3::Home, View3::Top, View3::Front, View3::Right] {
            for viewport in [(800.0, 600.0), (400.0, 900.0), (1600.0, 300.0)] {
                let mut camera = OrbitCamera::framing(stock(), viewport);
                camera.preset(view);
                camera.fit(stock());
                let fitted = reach(&camera, viewport);
                assert!(
                    fitted < 1.0 && fitted > 0.5,
                    "{view:?} in {viewport:?}: the furthest corner reaches {fitted}"
                );
                let mut tight = camera;
                tight.distance_mm /= FIT_MARGIN;
                assert_approx_eq!(reach(&tight, viewport), 1.0, 1e-9);
                assert_eq!(camera.target, stock().centre());
            }
        }
    }

    /// Acceptance 152: no drag takes the eye under the bed, and a drag
    /// down from level raises it; a full turn comes back.
    #[test]
    fn no_drag_goes_under_the_bed() {
        let mut camera = camera(View3::Home);
        camera.orbit(0.0, -10_000.0);
        assert_approx_eq!(camera.pitch_rad, 0.0, 0.0);
        assert!(camera.eye().z >= camera.target.z - 1e-9);
        camera.orbit(0.0, 10.0);
        assert_approx_eq!(camera.pitch_rad, 0.1, 1e-12);
        camera.orbit(0.0, 10_000.0);
        assert_approx_eq!(camera.pitch_rad, FRAC_PI_2, 0.0);
        let yaw = camera.yaw_rad;
        camera.orbit(std::f64::consts::TAU / ORBIT_RAD_PER_PX, 0.0);
        assert_approx_eq!(camera.yaw_rad, yaw, 1e-9);
        // A drag to the right turns the eye to the left of where it was:
        // the stock's near side follows the pointer.
        let mut camera = super::tests::camera(View3::Front);
        camera.orbit(10.0, 0.0);
        assert!(camera.eye().x < camera.target.x);
        assert_eq!(camera.view(), None);
    }

    /// A pan keeps what was under the pointer under it; a zoom keeps the
    /// pixel it is about showing the same point.
    #[test]
    fn pan_and_zoom_keep_what_is_under_the_pointer() {
        let mut camera = camera(View3::Home);
        let p = camera.target;
        let (x0, y0) = camera.to_screen(p).unwrap();
        camera.pan(37.0, -21.0);
        let (x1, y1) = camera.to_screen(p).unwrap();
        assert_approx_eq!(x1 - x0, 37.0, 1e-6);
        assert_approx_eq!(y1 - y0, -21.0, 1e-6);

        let mut camera = super::tests::camera(View3::Top);
        // A point of the plane through the target, under a pixel off
        // the centre.
        let at = (620.0, 140.0);
        let ray = camera.ray(at);
        let t = (camera.target.z - ray.origin.z) / ray.direction.z;
        let under = ray.at(t);
        let before = camera.distance_mm;
        camera.zoom_about(at, 0.5);
        assert_approx_eq!(camera.distance_mm, before / 2.0, 1e-9);
        let (x, y) = camera.to_screen(under).unwrap();
        assert_approx_eq!(x, at.0, 1e-6);
        assert_approx_eq!(y, at.1, 1e-6);
        // The distance is held to its range, and a zoom by nothing is none.
        camera.zoom_about(at, 1e-9);
        assert_approx_eq!(camera.distance_mm, DISTANCE_RANGE_MM.0, 0.0);
        camera.zoom_about(at, f64::NAN);
        camera.zoom_about(at, 0.0);
        assert_approx_eq!(camera.distance_mm, DISTANCE_RANGE_MM.0, 0.0);
    }

    /// The ray through a pixel passes through the point drawn there, and
    /// the view and perspective put that point at the same place.
    #[test]
    fn a_ray_and_the_matrix_agree_with_the_screen() {
        let camera = camera(View3::Home);
        let scene = stock();
        let vp = camera.view_projection(scene);
        for corner in scene.corners() {
            let (x, y) = camera.to_screen(corner).unwrap();
            let ray = camera.ray((x, y));
            let to_corner = Vec3::between(ray.origin, corner);
            let along = to_corner.dot(ray.direction);
            let off = Vec3::between(ray.at(along), corner).length();
            assert!(off < 1e-6, "the ray misses the corner by {off} mm");
            let clip = vp.apply(corner);
            let (w, h) = camera.viewport_px;
            assert_approx_eq!((clip[0] / clip[3] + 1.0) / 2.0 * w, x, 1e-6);
            assert_approx_eq!((1.0 - clip[1] / clip[3]) / 2.0 * h, y, 1e-6);
            let depth = clip[2] / clip[3];
            assert!((0.0..=1.0).contains(&depth), "depth {depth}");
        }
        assert_eq!(camera.to_screen(camera.ray((400.0, 300.0)).at(-5.0)), None);
    }
}
