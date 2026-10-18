// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/gcode/utils.ts

//! The machine transform: project space to the numbers a program carries
//! (`docs/specs/phase-1/design.md`, "Emission", steps 2 and 3; safety rule
//! S6).
//!
//! Ported from `projectToMachinePoint` and `machineToProjectPoint`.
//! Re-derived for Y-up (ADR-0001): upstream mirrors Y because its project
//! space is Y-down; DreamCarve's project and machine are both Y-up, so the
//! transform is a translation to the machine origin, then the definition's
//! axis mapping and signs, then the units scale. Nothing else mirrors.
//!
//! `coordinate_system.x_axis` names the machine axis that project X drives
//! (`dc_project::CoordinateSystem`); upstream reads the field the other way
//! round. The two agree for every mapping without an axis swap, which
//! covers both bundled definitions.

use dc_geometry::Point3;
use dc_project::{Axis, CoordinateSystem, OriginXy, OriginZ, Project, Units};

use crate::ExportError;

/// Millimetres per inch.
pub const MM_PER_INCH: f64 = 25.4;

/// Maps project points (mm, Y-up, stock bottom at Z = 0) to machine
/// coordinates in the output units, and back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MachineTransform {
    origin_mm: Point3,
    x: Signed,
    y: Signed,
    z: Signed,
    output_units: Units,
}

/// A machine axis without sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MachineAxis {
    X,
    Y,
    Z,
}

/// The machine axis a project axis drives, and the sign it drives it with.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Signed {
    axis: MachineAxis,
    sign: f64,
}

impl Signed {
    const fn of(axis: Axis) -> Self {
        let (axis, sign) = match axis {
            Axis::X => (MachineAxis::X, 1.0),
            Axis::Y => (MachineAxis::Y, 1.0),
            Axis::Z => (MachineAxis::Z, 1.0),
            Axis::NegX => (MachineAxis::X, -1.0),
            Axis::NegY => (MachineAxis::Y, -1.0),
            Axis::NegZ => (MachineAxis::Z, -1.0),
        };
        Self { axis, sign }
    }

    /// The coordinate of `m` on this machine axis.
    const fn pick(self, m: Point3) -> f64 {
        match self.axis {
            MachineAxis::X => m.x,
            MachineAxis::Y => m.y,
            MachineAxis::Z => m.z,
        }
    }
}

impl MachineTransform {
    /// The transform for `project`: its machine origin, its embedded
    /// post-processor's axis mapping, and its display units as output units.
    ///
    /// # Errors
    ///
    /// [`ExportError::InvalidAxisMapping`] if two project axes map to the
    /// same machine axis or project Z does not map to machine Z.
    pub fn new(project: &Project) -> Result<Self, ExportError> {
        let axes = project.post_processor.coordinate_system;
        let (x, y, z) = (
            Signed::of(axes.x_axis),
            Signed::of(axes.y_axis),
            Signed::of(axes.z_axis),
        );
        // Arcs are written in the XY plane (G17) and safe Z is a machine Z
        // height, so project Z must drive machine Z, and X and Y the other
        // two. Machine +Z is up on GRBL and grblHAL; an inverted Z would turn
        // every retract into a plunge, and the safety pass, which reads the
        // program back through this same transform, could not tell.
        if z.axis != MachineAxis::Z || z.sign < 0.0 || x.axis == y.axis || x.axis == MachineAxis::Z
        {
            return Err(invalid(&project.post_processor.id, axes));
        }
        let stock = &project.stock;
        let (ox, oy) = match project.origin.xy {
            OriginXy::BottomLeft => (0.0, 0.0),
            OriginXy::BottomRight => (stock.width_mm, 0.0),
            OriginXy::TopLeft => (0.0, stock.height_mm),
            OriginXy::TopRight => (stock.width_mm, stock.height_mm),
            OriginXy::Centre => (stock.width_mm / 2.0, stock.height_mm / 2.0),
            OriginXy::Custom { x_mm, y_mm } => (x_mm, y_mm),
        };
        let oz = match project.origin.z {
            OriginZ::StockTop => stock.thickness_mm.ok_or(ExportError::MissingThickness)?,
            OriginZ::StockBottom => 0.0,
        };
        Ok(Self {
            origin_mm: Point3::new(ox, oy, oz),
            x,
            y,
            z,
            output_units: project.display_units,
        })
    }

    /// The units machine coordinates are written in.
    #[must_use]
    pub const fn output_units(&self) -> Units {
        self.output_units
    }

    /// Millimetres per output unit: 1 or 25.4.
    #[must_use]
    pub const fn mm_per_unit(&self) -> f64 {
        mm_per_unit(self.output_units)
    }

    /// The machine origin in project space, mm.
    #[must_use]
    pub const fn origin_mm(&self) -> Point3 {
        self.origin_mm
    }

    /// True when the axis mapping mirrors the XY plane (an odd number of
    /// inversions or an X/Y swap), which turns a clockwise arc into a
    /// counter-clockwise one.
    #[must_use]
    pub fn mirrors_xy(&self) -> bool {
        // The planar map's determinant is sx·sy, negated by an X/Y swap.
        let swap = if self.x.axis == MachineAxis::Y {
            -1.0
        } else {
            1.0
        };
        self.x.sign * self.y.sign * swap < 0.0
    }

    /// `p`, a project point in mm, in machine coordinates and output units.
    #[must_use]
    pub fn to_machine(&self, p: Point3) -> Point3 {
        let scale = self.mm_per_unit();
        let deltas = [
            (self.x, p.x - self.origin_mm.x),
            (self.y, p.y - self.origin_mm.y),
            (self.z, p.z - self.origin_mm.z),
        ];
        let on = |axis: MachineAxis| {
            deltas
                .iter()
                .find(|(s, _)| s.axis == axis)
                .map_or(0.0, |(s, delta)| s.sign * delta / scale)
        };
        Point3::new(on(MachineAxis::X), on(MachineAxis::Y), on(MachineAxis::Z))
    }

    /// `m`, a machine point in `units`, back in project space, mm.
    #[must_use]
    pub fn to_project(&self, m: Point3, units: Units) -> Point3 {
        let scale = mm_per_unit(units);
        // Each sign is ±1, so it is its own inverse.
        Point3::new(
            self.x.sign * self.x.pick(m) * scale + self.origin_mm.x,
            self.y.sign * self.y.pick(m) * scale + self.origin_mm.y,
            self.z.sign * self.z.pick(m) * scale + self.origin_mm.z,
        )
    }

    /// Project Z, mm, of machine Z `z` in `units`. Defined because project
    /// Z drives machine Z alone.
    #[must_use]
    pub fn project_z(&self, z: f64, units: Units) -> f64 {
        self.z.sign * z * mm_per_unit(units) + self.origin_mm.z
    }
}

fn invalid(post: &str, axes: CoordinateSystem) -> ExportError {
    let name = |a: Axis| {
        match a {
            Axis::X => "X",
            Axis::Y => "Y",
            Axis::Z => "Z",
            Axis::NegX => "-X",
            Axis::NegY => "-Y",
            Axis::NegZ => "-Z",
        }
        .to_owned()
    };
    ExportError::InvalidAxisMapping {
        post: post.to_owned(),
        x: name(axes.x_axis),
        y: name(axes.y_axis),
        z: name(axes.z_axis),
    }
}

/// Millimetres per unit of `units`.
#[must_use]
pub const fn mm_per_unit(units: Units) -> f64 {
    match units {
        Units::Mm => 1.0,
        Units::Inch => MM_PER_INCH,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dc_geometry::assert_approx_eq;
    use dc_project::test_support::sample_project;
    use dc_project::{Axis, OriginXy, OriginZ};

    const TOL: f64 = 1e-9;

    fn assert_point(actual: Point3, expected: Point3) {
        assert_approx_eq!(actual.x, expected.x, TOL);
        assert_approx_eq!(actual.y, expected.y, TOL);
        assert_approx_eq!(actual.z, expected.z, TOL);
    }

    #[test]
    fn origin_bottom_left_stock_top_translates_only() {
        // 100 × 100 × 6 mm stock, origin bottom-left, Z zero at stock top.
        let project = sample_project();
        assert_eq!(project.origin.xy, OriginXy::BottomLeft);
        assert_eq!(project.origin.z, OriginZ::StockTop);
        let t = MachineTransform::new(&project).unwrap();
        assert!(!t.mirrors_xy());
        for p in [
            Point3::new(0.0, 0.0, 6.0),
            Point3::new(10.0, 20.0, 3.0),
            Point3::new(90.0, 75.5, 11.0),
            Point3::new(-3.0, -4.0, 0.0),
        ] {
            let m = t.to_machine(p);
            // X and Y pass through unchanged, Y is not mirrored, and Z only
            // moves down by the thickness: no coordinate changes sign
            // except by translation.
            assert_point(m, Point3::new(p.x, p.y, p.z - 6.0));
            assert_point(t.to_project(m, Units::Mm), p);
        }
        // Moving up in project Y moves up in machine Y.
        let a = t.to_machine(Point3::new(0.0, 10.0, 0.0));
        let b = t.to_machine(Point3::new(0.0, 20.0, 0.0));
        assert!(b.y > a.y);
    }

    #[test]
    fn other_origins_translate() {
        let mut project = sample_project();
        project.origin.z = OriginZ::StockBottom;
        let cases = [
            (OriginXy::BottomRight, Point3::new(-90.0, 20.0, 3.0)),
            (OriginXy::TopLeft, Point3::new(10.0, -80.0, 3.0)),
            (OriginXy::TopRight, Point3::new(-90.0, -80.0, 3.0)),
            (OriginXy::Centre, Point3::new(-40.0, -30.0, 3.0)),
            (
                OriginXy::Custom {
                    x_mm: 5.0,
                    y_mm: 7.0,
                },
                Point3::new(5.0, 13.0, 3.0),
            ),
        ];
        for (xy, expected) in cases {
            project.origin.xy = xy;
            let t = MachineTransform::new(&project).unwrap();
            assert_point(t.to_machine(Point3::new(10.0, 20.0, 3.0)), expected);
        }
    }

    #[test]
    fn axis_inversion_from_definition_applies() {
        let mut project = sample_project();
        project.post_processor.coordinate_system = CoordinateSystem {
            x_axis: Axis::NegX,
            y_axis: Axis::Y,
            z_axis: Axis::Z,
        };
        let t = MachineTransform::new(&project).unwrap();
        let p = Point3::new(10.0, 20.0, 3.0);
        assert_point(t.to_machine(p), Point3::new(-10.0, 20.0, -3.0));
        assert_point(t.to_project(t.to_machine(p), Units::Mm), p);
        assert!(t.mirrors_xy(), "one inverted axis mirrors the plane");

        project.post_processor.coordinate_system = CoordinateSystem {
            x_axis: Axis::NegX,
            y_axis: Axis::NegY,
            z_axis: Axis::Z,
        };
        let t = MachineTransform::new(&project).unwrap();
        assert_point(t.to_machine(p), Point3::new(-10.0, -20.0, -3.0));
        assert!(!t.mirrors_xy(), "two inversions are a rotation");

        project.post_processor.coordinate_system = CoordinateSystem {
            x_axis: Axis::Y,
            y_axis: Axis::X,
            z_axis: Axis::Z,
        };
        let t = MachineTransform::new(&project).unwrap();
        assert_point(t.to_machine(p), Point3::new(20.0, 10.0, -3.0));
        assert_point(t.to_project(t.to_machine(p), Units::Mm), p);
        assert!(t.mirrors_xy(), "an X/Y swap mirrors the plane");
    }

    #[test]
    fn invalid_axis_mappings_are_rejected() {
        let mut project = sample_project();
        for (x, y, z) in [
            (Axis::X, Axis::X, Axis::Z),
            (Axis::X, Axis::NegX, Axis::Z),
            (Axis::Z, Axis::Y, Axis::X),
            (Axis::X, Axis::Z, Axis::Y),
            // Machine +Z is up on GRBL and grblHAL; an inverted Z would
            // send every retract into the work.
            (Axis::X, Axis::Y, Axis::NegZ),
        ] {
            project.post_processor.coordinate_system = CoordinateSystem {
                x_axis: x,
                y_axis: y,
                z_axis: z,
            };
            assert!(matches!(
                MachineTransform::new(&project),
                Err(ExportError::InvalidAxisMapping { .. })
            ));
        }
    }

    /// #64: with Z zero at the stock top, the origin is the thickness, so an
    /// unset one is refused rather than defaulted.
    #[test]
    fn stock_top_origin_without_thickness_is_rejected() {
        let mut project = sample_project();
        project.stock.thickness_mm = None;
        assert!(matches!(
            MachineTransform::new(&project),
            Err(ExportError::MissingThickness)
        ));
    }

    #[test]
    fn inch_output_scales_after_the_transform() {
        let mut project = sample_project();
        project.display_units = Units::Inch;
        let t = MachineTransform::new(&project).unwrap();
        assert_eq!(t.output_units(), Units::Inch);
        let p = Point3::new(25.4, 50.8, 6.0 + 25.4);
        assert_point(t.to_machine(p), Point3::new(1.0, 2.0, 1.0));
        assert_point(t.to_project(Point3::new(1.0, 2.0, 1.0), Units::Inch), p);
        assert_approx_eq!(t.project_z(1.0, Units::Inch), 31.4, TOL);
        assert_approx_eq!(t.project_z(-6.0, Units::Mm), 0.0, TOL);
    }
}
