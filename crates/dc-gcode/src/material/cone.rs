// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! S17, the cone, on the parsed program (`docs/specs/phase-3/design.md`,
//! "Safety pass" and "Algorithms → Tapered ball nose", step 7; functional
//! S17; security review H3, H4 and M1).
//!
//! A carving bit is a stack of discs about its axis that widen with
//! height ([`BitProfile`]): a V-bit's of radius `d · slope` at height `d`
//! above its tip (`slope = tan(α/2)`), a tapered ball nose's (TBN) of
//! radius `w(d)`, its ball, then its cone, then its cylinder at the cap
//! `R`. A bit whose tip is `d` below the start plane cuts the disc of
//! radius `w(d)` there, and it cuts nothing below the carve's final
//! surface exactly when that disc stays inside the carved regions: the
//! final surface at a point `q` is `w⁻¹(dist(q, wall))` deep, and the bit
//! at `p` reaches `q` no lower than `d − w⁻¹(|q − p|)`, which the triangle
//! inequality keeps above it wherever `w(d) ≤ dist(p, wall)` (`w` is
//! increasing and, below the cap, concave). So each cut is held to its
//! **room**, the tip's distance to the walls less `w(d)`, at every point
//! along it, measured exactly:
//!
//! - **A cut at constant Z, or an arc:** segment against segment, arcs as
//!   arcs ([`segment_distance_mm`]), less the radius at its deeper end.
//!   That is exact at constant Z; an arc whose Z varies (no V toolpath has
//!   one) is held to its deeper end all along, which can only refuse more,
//!   since `w` never narrows with depth.
//! - **A straight cut whose Z varies:** for each wall segment, the least of
//!   `g(τ) = dist(p(τ), wall) − w(d(τ))` over the cut, exactly, by the
//!   shared [`least_room`] (#678), which the generator calls too. For a
//!   V-bit the radius is linear in `τ` and the least is in closed form. For
//!   a TBN the cut is split where `d(τ)` crosses `h_t` and `h_R`: the
//!   cylinder stretch is the end mill's constant `R`, the cone stretch is
//!   the virtual V-bit's `(d + δ) · tan β`, both in that closed form, and
//!   the ball stretch's least is at an end or a stationary point, a root of
//!   a polynomial of degree at most four. No sampling step.
//!
//! The walls are the carved regions' outlines as drawn, arcs and all. The
//! slack is S9's (`material`'s module docs): one unit in the last written
//! place, the flatten tolerance (the generator measures its chains against
//! chords, which near a concave arc lie up to that far beyond the true
//! wall, #407; the floor trace runs at exactly `w(F)` from the walls,
//! #405, and the floor's chords where grown islands touch are within it),
//! the arc-fit tolerance when arcs are fitted (S12), and [`EPS_MM`].

use dc_geometry::profile::{BitProfile, WallPiece, least_room};
use dc_geometry::{
    EPS_MM, Point, Point3, Segment, closest_points, point_segment_distance_mm, segment_distance_mm,
};

use super::{Area, bounds_of, near, on_line, points_of};

/// A V-Carve's material for S17, every value re-derived from the project:
/// a V-bit's or a tapered ball nose's.
#[derive(Debug, Clone)]
pub(crate) struct Cone {
    /// The carved regions, outer and holes, from the targets' closed
    /// profiles.
    pub(super) areas: Vec<Area>,
    /// The start plane, mm, project space.
    pub(super) start_plane_z_mm: f64,
    /// The bit's cross-section (`Tool::carve_profile`).
    pub(super) profile: BitProfile,
    /// The flat depth's floor, mm, project space; `None` without one.
    pub(super) floor_z_mm: Option<f64>,
    /// The Z the flank is engaged from, mm: the stock top, or the start
    /// plane when an enabled earlier Pocket at its depth contains the
    /// targets (security review H4).
    pub(super) engaged_from_z_mm: f64,
    /// The cutting height, the deepest the flank may be engaged, mm
    /// (`Tool::cutting_height_mm`): a V-bit's cone height, `diameter / (2
    /// · slope)`, or a TBN's `min(flute_length_mm, h_R)`.
    pub(super) cone_mm: f64,
    /// A male Inlay's open band edge (#448): the same cone held to the
    /// band `B` with the stock top as its plane, since beyond `B` the
    /// board stands to the top, so no disc below the stock top may cross
    /// it. `None` for every other carve.
    pub(super) edge: Option<Box<Cone>>,
}

/// Why a cut fails S17.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Fault {
    /// The cone, the tip or the cut reaches `overlap_mm` past the material
    /// at `at`, project space.
    Off { at: Point3, overlap_mm: f64 },
    /// The flank is engaged `engaged_mm` deep, past the cutting height.
    Shank { engaged_mm: f64 },
}

impl Cone {
    /// The radius of the disc the bit cuts at the start plane for a tip at
    /// `z_mm`, mm: a V-bit's `d · slope`, negative above the start plane,
    /// as it always was; a TBN's `w(d)`, 0 above it.
    pub(super) fn radius_mm(&self, z_mm: f64) -> f64 {
        let depth_mm = self.start_plane_z_mm - z_mm;
        match self.profile {
            BitProfile::VBit { slope } => depth_mm * slope,
            BitProfile::TaperedBall { .. } => self.profile.half_width_mm(depth_mm),
        }
    }

    /// S17 for one cutting move: `path` its tip path in XY, `from_z_mm` and
    /// `to_z_mm` its ends' Z, project space, within `slack_mm`.
    ///
    /// - Its tip stays in the regions: its ends and middle are inside one
    ///   or within the slack of an outline, and it meets no outline away
    ///   from its ends.
    /// - **A straight descent** (no XY motion beyond the slack) that stays
    ///   above the start plane runs through the air over the carve: the
    ///   start plane is the stock top, or a Pocket or the user cleared it.
    ///   Any other descent is held at its lowest point, the one point
    ///   where its cone is widest, which is stricter than H3's exemption
    ///   (`docs/specs/phase-3/design.md`, "Safety pass", "Descents").
    /// - **Any other cut** runs no higher than the start plane.
    /// - No cut reaches below the flat depth, nor engages the flank deeper
    ///   than the cone (the shank, H4).
    /// - Its room is at least `−slack_mm` all along (module docs).
    /// - With an open band edge (#448), the same holds of the edge's cone,
    ///   whose plane is the stock top: the cone's disc there stays inside
    ///   the band.
    pub(super) fn check(
        &self,
        path: Segment,
        z_mm: (f64, f64),
        slack_mm: f64,
    ) -> Result<(), Fault> {
        self.check_own(path, z_mm, slack_mm)?;
        match &self.edge {
            Some(edge) => edge.check(path, z_mm, slack_mm),
            None => Ok(()),
        }
    }

    /// [`Cone::check`] against this cone's own regions and plane.
    fn check_own(
        &self,
        path: Segment,
        (from_z_mm, to_z_mm): (f64, f64),
        slack_mm: f64,
    ) -> Result<(), Fault> {
        // The tip's Z at `p` on the path: along a line, as far as `p` is
        // along it; on an arc (no V toolpath has one with varying Z), the
        // deeper end's, whose cone is the wider.
        let z_at = |p: Point| {
            let chord_mm = path.start.distance_to(path.end);
            if !path.is_line() || chord_mm <= EPS_MM {
                return from_z_mm.min(to_z_mm);
            }
            let t = (path.start.distance_to(p) / chord_mm).clamp(0.0, 1.0);
            from_z_mm + (to_z_mm - from_z_mm) * t
        };
        let (low, high) = if from_z_mm <= to_z_mm {
            (
                Point3::new(path.start.x, path.start.y, from_z_mm),
                Point3::new(path.end.x, path.end.y, to_z_mm),
            )
        } else {
            (
                Point3::new(path.end.x, path.end.y, to_z_mm),
                Point3::new(path.start.x, path.start.y, from_z_mm),
            )
        };
        if let Some((p, outside_mm)) = self.tip_off(path, slack_mm) {
            let z_mm = z_at(p);
            return Err(Fault::Off {
                at: Point3::new(p.x, p.y, z_mm),
                overlap_mm: outside_mm + self.radius_mm(z_mm).max(0.0),
            });
        }
        let vertical = path.is_line() && path.start.distance_to(path.end) <= slack_mm;
        let start_plane_z_mm = self.start_plane_z_mm;
        if vertical && low.z > start_plane_z_mm + slack_mm {
            return Ok(());
        }
        if !vertical && high.z > start_plane_z_mm + slack_mm {
            return Err(Fault::Off {
                at: high,
                overlap_mm: high.z - start_plane_z_mm,
            });
        }
        if let Some(floor_z_mm) = self.floor_z_mm
            && low.z < floor_z_mm - slack_mm
        {
            return Err(Fault::Off {
                at: low,
                overlap_mm: floor_z_mm - low.z,
            });
        }
        let engaged_mm = self.engaged_from_z_mm - low.z;
        if engaged_mm > self.cone_mm + slack_mm {
            return Err(Fault::Shank { engaged_mm });
        }
        match self.least_room(
            path,
            (from_z_mm, to_z_mm),
            vertical.then_some(low),
            slack_mm,
        ) {
            Some((room_mm, at)) if room_mm < -slack_mm => Err(Fault::Off {
                at,
                overlap_mm: -room_mm,
            }),
            _ => Ok(()),
        }
    }

    /// Where tip path `path` leaves the regions, and how far outside it is
    /// there, mm (0 where it crosses an outline): an end or its middle
    /// outside every region beyond `slack_mm`, or a point away from its
    /// ends where it meets an outline. `None` when it stays in.
    fn tip_off(&self, path: Segment, slack_mm: f64) -> Option<(Point, f64)> {
        let bounds = bounds_of(path);
        let near_areas = || {
            self.areas
                .iter()
                .filter(move |a| near(bounds, a.bounds, slack_mm))
        };
        for p in points_of(path) {
            let held = self.areas.iter().any(|a| a.region.contains_point(p))
                || near_areas().any(|a| on_line(&a.edges, p, slack_mm));
            if !held {
                let outside_mm = self
                    .areas
                    .iter()
                    .flat_map(|a| &a.edges)
                    .map(|e| point_segment_distance_mm(e.segment, p))
                    .fold(f64::INFINITY, f64::min);
                return Some((
                    p,
                    if outside_mm.is_finite() {
                        outside_mm
                    } else {
                        0.0
                    },
                ));
            }
        }
        near_areas()
            .flat_map(|a| &a.edges)
            .filter(|e| near(bounds, e.bounds, slack_mm))
            .find_map(|e| {
                let (p, q) = closest_points(path, e.segment);
                (p.distance_to(q) <= EPS_MM
                    && p.distance_to(path.start) > slack_mm
                    && p.distance_to(path.end) > slack_mm)
                    .then_some((p, 0.0))
            })
    }

    /// The least room along the cut (module docs), mm, and where it is
    /// least, among the walls within reach of the cut; `None` when no wall
    /// is, or no wall can bring the room below `−slack_mm`. A straight
    /// descent (`descent` its lowest point) is read at that point.
    fn least_room(
        &self,
        path: Segment,
        (from_z_mm, to_z_mm): (f64, f64),
        descent: Option<Point3>,
        slack_mm: f64,
    ) -> Option<(f64, Point3)> {
        let low_z_mm = from_z_mm.min(to_z_mm);
        let widest_mm = self.radius_mm(low_z_mm);
        let reach_mm = widest_mm.max(0.0) + slack_mm;
        let bounds = bounds_of(path);
        let level = (to_z_mm - from_z_mm).abs() <= EPS_MM;
        let mut least: Option<(f64, Point3)> = None;
        let mut keep = |room_mm: f64, at: Point3| {
            if least.is_none_or(|(r, _)| room_mm < r) {
                least = Some((room_mm, at));
            }
        };
        let walls = self
            .areas
            .iter()
            .filter(|a| near(bounds, a.bounds, reach_mm))
            .flat_map(|a| &a.edges)
            .filter(|e| near(bounds, e.bounds, reach_mm));
        for wall in walls {
            if let Some(low) = descent {
                let p = Point::new(low.x, low.y);
                keep(point_segment_distance_mm(wall.segment, p) - widest_mm, low);
                continue;
            }
            // At constant Z, or on an arc, the room at the deeper end all
            // along: exact at constant Z, and a bound below the room for an
            // arc whose Z varies.
            let bound_mm = segment_distance_mm(path, wall.segment) - widest_mm;
            if level || !path.is_line() {
                let (p, _) = closest_points(path, wall.segment);
                keep(bound_mm, Point3::new(p.x, p.y, low_z_mm));
                continue;
            }
            if bound_mm >= -slack_mm {
                // This wall cannot fail the cut.
                continue;
            }
            let from = Point3::new(path.start.x, path.start.y, from_z_mm);
            let to = Point3::new(path.end.x, path.end.y, to_z_mm);
            let (room_mm, t) = least_room(
                from,
                to,
                WallPiece::from(wall.segment),
                &self.profile,
                self.start_plane_z_mm,
            );
            let p = path.start + (path.end - path.start) * t;
            keep(
                room_mm,
                Point3::new(p.x, p.y, from_z_mm + (to_z_mm - from_z_mm) * t),
            );
        }
        least
    }
}
