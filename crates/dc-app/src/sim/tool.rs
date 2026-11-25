// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/engine/simulation/toolMesh.ts

//! The tool as it is drawn (phase 6 functional spec, capability 115;
//! `docs/specs/phase-6/design.md`, "The renderer", draw 4): a solid
//! turned about the tool's axis, its tip at the origin and its axis +Z,
//! with a plain shank above the cutting length.
//!
//! From upstream: the proportions. The cutting part is three diameters
//! long from the tip, the shank is 1.05 times the radius and two
//! diameters long, and a turn is 24 segments. Changed: upstream builds a
//! cylinder, a hemisphere or a cone by the tool's kind; here one profile
//! is turned for every kind, and its underside is the simulation's own
//! [`Cutter::height_mm`], so the tool drawn is the shape that cuts,
//! the tapered ball nose included. A length is a multiple of the
//! diameter and never a fixed number of millimetres, which upstream's
//! least lengths are. The triangles are `f64`; the renderer narrows them.

use std::f64::consts::TAU;

use dc_geometry::Point3;
use dc_job::sim::Cutter;

use super::mat::Vec3;

/// Segments in a turn about the axis: upstream's.
pub const TURN_SEGMENTS: u32 = 24;
/// Steps across the radius along the cutter's underside.
pub const UNDERSIDE_STEPS: u32 = 16;
/// The cutting part's length from the tip, in diameters: upstream's
/// when the tool gives no cutting depth, and no tool here does.
pub const CUT_LENGTH_DIAMETERS: f64 = 3.0;
/// The shank's length, in diameters.
pub const SHANK_LENGTH_DIAMETERS: f64 = 2.0;
/// The shank's radius over the cutter's.
pub const SHANK_RADIUS_RATIO: f64 = 1.05;

/// Which part of the tool a triangle belongs to: each has its colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPart {
    /// The part that cuts.
    Cutter,
    /// The plain shank above it.
    Shank,
}

/// One corner of a triangle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ToolVertex {
    /// Where it is, mm from the tip.
    pub position: Point3,
    /// The surface's outward direction there, one long.
    pub normal: Vec3,
    /// The part it belongs to.
    pub part: ToolPart,
}

/// The tool's surface as triangles, three corners each.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ToolMesh {
    /// The corners, three a triangle.
    pub vertices: Vec<ToolVertex>,
    /// The top of the shank, mm above the tip.
    pub height_mm: f64,
}

/// One stretch of the profile: from one (radius, height) to the next,
/// going up the outside of the tool.
struct Stretch {
    from: (f64, f64),
    to: (f64, f64),
    part: ToolPart,
}

/// The mesh of `cutter`. A cutter of no size has no mesh.
#[must_use]
pub fn tool_mesh(cutter: &Cutter) -> ToolMesh {
    let radius = cutter.radius_mm();
    if !(radius.is_finite() && radius > 0.0) {
        return ToolMesh::default();
    }
    let diameter = 2.0 * radius;
    // The underside, from the tip out to the radius.
    let mut profile: Vec<(f64, f64)> = (0..=UNDERSIDE_STEPS)
        .filter_map(|k| {
            let d = radius * f64::from(k) / f64::from(UNDERSIDE_STEPS);
            cutter.height_mm(d).map(|z| (d, z))
        })
        .collect();
    let rim = profile.last().map_or(0.0, |p| p.1);
    // A cutter whose underside is taller than three diameters (a slim
    // V-bit) still has a little wall above it.
    let cut_top = (CUT_LENGTH_DIAMETERS * diameter).max(rim + 0.5 * diameter);
    let shank_radius = SHANK_RADIUS_RATIO * radius;
    let top = cut_top + SHANK_LENGTH_DIAMETERS * diameter;
    profile.push((radius, cut_top));
    let cutter_points = profile.len();
    profile.extend([(shank_radius, cut_top), (shank_radius, top), (0.0, top)]);

    let stretches = profile.windows(2).enumerate().filter_map(|(k, pair)| {
        let (&from, &to) = (pair.first()?, pair.get(1)?);
        Some(Stretch {
            from,
            to,
            // The step out to the shank is the shank's underside.
            part: if k + 1 < cutter_points {
                ToolPart::Cutter
            } else {
                ToolPart::Shank
            },
        })
    });

    let mut vertices = Vec::new();
    for stretch in stretches {
        let (r0, z0) = stretch.from;
        let (r1, z1) = stretch.to;
        let (dr, dz) = (r1 - r0, z1 - z0);
        let length = dr.hypot(dz);
        if length <= 0.0 {
            continue;
        }
        // Going up the outside, outward is to the right of the way.
        let (nr, nz) = (dz / length, -dr / length);
        for segment in 0..TURN_SEGMENTS {
            let angle = |s: u32| TAU * f64::from(s) / f64::from(TURN_SEGMENTS);
            let corner = |r: f64, z: f64, a: f64| ToolVertex {
                position: Point3::new(r * a.cos(), r * a.sin(), z),
                normal: Vec3::new(nr * a.cos(), nr * a.sin(), nz),
                part: stretch.part,
            };
            let (a0, a1) = (angle(segment), angle(segment + 1));
            let quad = [
                corner(r0, z0, a0),
                corner(r0, z0, a1),
                corner(r1, z1, a1),
                corner(r1, z1, a0),
            ];
            let [a, b, c, d] = quad;
            // A stretch that starts or ends on the axis is a fan: one of
            // the two triangles has no area and is left out.
            if r0 > 0.0 {
                vertices.extend([a, b, c]);
            }
            if r1 > 0.0 {
                vertices.extend([a, c, d]);
            }
        }
    }
    ToolMesh {
        vertices,
        height_mm: top,
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

    fn cutters() -> Vec<Cutter> {
        vec![
            Cutter::Flat { radius_mm: 3.175 },
            Cutter::Ball { radius_mm: 3.0 },
            Cutter::Vee {
                slope: (30.0_f64).to_radians().tan(),
                radius_mm: 6.35,
            },
            Cutter::Vee {
                slope: (5.0_f64).to_radians().tan(),
                radius_mm: 3.0,
            },
            Cutter::TaperedBall {
                tip_radius_mm: 0.5,
                slope: (5.0_f64).to_radians().tan(),
                radius_mm: 3.0,
            },
        ]
    }

    /// Capability 115: the tool drawn is the shape that cuts. Every
    /// corner of the cutting part is on or above the cutter's own
    /// profile, the lowest is the tip at the origin, and none is wider
    /// than the cutter.
    #[test]
    fn the_cutter_is_drawn_in_the_shape_that_cuts() {
        for cutter in cutters() {
            let mesh = tool_mesh(&cutter);
            assert!(!mesh.vertices.is_empty(), "{cutter:?}");
            assert_eq!(mesh.vertices.len() % 3, 0);
            let radius = cutter.radius_mm();
            let mut lowest = f64::INFINITY;
            for v in mesh.vertices.iter().filter(|v| v.part == ToolPart::Cutter) {
                let d = v.position.x.hypot(v.position.y);
                assert!(d <= radius + 1e-9, "{cutter:?}: {d}");
                let floor = cutter.height_mm(d.min(radius)).unwrap();
                assert!(v.position.z >= floor - 1e-9, "{cutter:?} at {d}");
                lowest = lowest.min(v.position.z);
            }
            assert_approx_eq!(lowest, 0.0, 1e-9);
        }
    }

    /// Upstream's proportions: three diameters of cutter, then a shank
    /// 1.05 times as wide and two diameters long.
    #[test]
    fn the_shank_is_a_touch_wider_and_two_diameters_long() {
        let mesh = tool_mesh(&Cutter::Flat { radius_mm: 3.0 });
        assert_approx_eq!(mesh.height_mm, 30.0, 1e-9);
        let shank: Vec<_> = mesh
            .vertices
            .iter()
            .filter(|v| v.part == ToolPart::Shank)
            .collect();
        let widest = shank
            .iter()
            .map(|v| v.position.x.hypot(v.position.y))
            .fold(0.0, f64::max);
        assert_approx_eq!(widest, 3.15, 1e-9);
        let (low, high) = shank.iter().fold((f64::INFINITY, 0.0_f64), |(l, h), v| {
            (l.min(v.position.z), h.max(v.position.z))
        });
        assert_approx_eq!(low, 18.0, 1e-9);
        assert_approx_eq!(high, 30.0, 1e-9);
        // A slim V-bit's cone is taller than three diameters: it keeps
        // a wall above the cone.
        let slim = tool_mesh(&Cutter::Vee {
            slope: (5.0_f64).to_radians().tan(),
            radius_mm: 3.0,
        });
        let cone = 3.0 / (5.0_f64).to_radians().tan();
        assert_approx_eq!(slim.height_mm, cone + 3.0 + 12.0, 1e-9);
    }

    /// Every normal is one long and points away from the axis or along
    /// it: out on a wall, down under the cutter, up on the top.
    #[test]
    fn normals_are_unit_and_point_out() {
        for cutter in cutters() {
            let mesh = tool_mesh(&cutter);
            for v in &mesh.vertices {
                assert_approx_eq!(v.normal.length(), 1.0, 1e-9);
                let radial = v.normal.x * v.position.x + v.normal.y * v.position.y;
                assert!(radial >= -1e-9, "{cutter:?}: a normal points at the axis");
            }
            let top = mesh
                .vertices
                .iter()
                .filter(|v| (v.position.z - mesh.height_mm).abs() < 1e-9)
                .filter(|v| v.normal.z > 0.5)
                .count();
            assert!(top > 0, "{cutter:?}: the shank has a top");
            assert!(
                mesh.vertices
                    .iter()
                    .filter(|v| v.part == ToolPart::Cutter && v.position.z < 1e-9)
                    .all(|v| v.normal.z <= 1e-9),
                "{cutter:?}: the underside faces down"
            );
        }
    }

    #[test]
    fn a_cutter_of_no_size_has_no_mesh() {
        assert!(
            tool_mesh(&Cutter::Flat { radius_mm: 0.0 })
                .vertices
                .is_empty()
        );
        assert!(
            tool_mesh(&Cutter::Ball {
                radius_mm: f64::NAN
            })
            .vertices
            .is_empty()
        );
    }
}
