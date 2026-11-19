// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Questions about a whole project's geometry that validation and the
//! generators must answer the same way (`docs/specs/phase-3/design.md`,
//! "Algorithms → Start depth"). They live here, not in `dc-cam`, because
//! validation may not depend on `dc-cam` (`docs/rust-guidelines.md`,
//! "Crate boundaries"); `dc-cam` calls them.

use crate::ids::InstanceId;
use crate::model::{Bitmap, Operation, Project, VCarveOp};
use crate::validate::MAX_FLATTEN_TOLERANCE_MM;
use dc_geometry::{
    Contour, EPS_MM, FLATTEN_TOLERANCE_DEFAULT_MM, Point, Polyline, Region, area_mm2, as_circle,
    difference, floor_regions, nest, point_segment_distance_mm,
};

/// The plane an earlier operation cleared under the footprint of an
/// operation cut from a start plane ([`cleared_start_plane`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClearedPlane<'p> {
    /// The operation whose cut left the plane: an enabled Pocket, or an
    /// enabled V-Carve with a flat depth (its floor, #426).
    pub by: &'p Operation,
}

impl ClearedPlane<'_> {
    /// How far below the stock top the plane is, mm: the Pocket's depth
    /// (`None` when it has none set), or the V-Carve's start depth plus
    /// its flat depth ([`Operation::depth_mm`]).
    #[must_use]
    pub fn depth_mm(&self) -> Option<f64> {
        self.by.depth_mm()
    }

    /// Whether the plane is `start_depth_mm` below the stock top, within
    /// [`EPS_MM`]: then it is the start plane of the operation it holds,
    /// and that operation's flank is engaged from it (security review
    /// H4). At another depth it is `start_depth_mismatch`.
    #[must_use]
    pub fn is_at(&self, start_depth_mm: f64) -> bool {
        self.depth_mm()
            .is_some_and(|d| (d - start_depth_mm).abs() <= EPS_MM)
    }
}

/// The plane an earlier operation cleared under the start plane of `op`
/// (functional spec, capability 41): the last operation before `op` in
/// the operation list that is
///
/// - **an enabled Pocket** whose regions (its targets' closed outlines,
///   nested) hold the whole footprint of `op`; the plane is its depth; or
/// - **an enabled V-Carve with a flat depth `F`** (#426) whose floor
///   regions hold it: its targets' regions inset by `F · tan(α/2)` with
///   its own V-bit ([`dc_geometry::floor_regions`], less the pieces too
///   thin to cut, as the generator's floor and the safety pass derive
///   them). The plane is its start depth plus `F`. The floor is cut
///   whole by the V-bit's floor rings and any clearing passes, and above
///   it the carve's walls slope outward, so a footprint inside the floor
///   at floor level is in air all the way up.
///
/// Held means inside a region and outside its islands. `None` when there
/// is no such operation, when `op` is not in `project` or is not cut from
/// a start plane, or when its footprint does not resolve (a missing
/// instance or tool, a degenerate or crossing outline, no hole:
/// validation or the generator reports those). A V-Carve whose floor does
/// not resolve (no V-bit with an angle in (0°, 180°), a start depth below
/// 0) clears nothing.
///
/// The footprint is what `op` cuts at its start plane:
///
/// - **A V-Carve:** the closed outlines of its targets, as placed; open
///   paths have no area and are left out. Containment is checked two
///   ways, both needed: every vertex of every outline is in a region by
///   [`dc_geometry::Region::contains_point`], and the area the outlines
///   enclose, nested even-odd as the carve nests them, less the clearing
///   regions, is nothing within the flattening slack (the project's
///   flatten tolerance times the outlines' length). The second catches an
///   outline whose vertices are all inside but whose edges cross a gap or
///   an island; an outline crossing a region boundary does not count.
/// - **A countersink Drill** (format 21, #425): the disc of its tool's
///   radius at the centre of each target circle (closed profiles that
///   are circles within the flatten tolerance, as the Drill finds its
///   holes), not the hole circle. That is the bit's widest section above
///   the start plane, in the cleared air: a floor that holds the hole but
///   not the bit round it would let the bit cut its wall. A disc is held
///   when its centre is in a region and at least the radius, less the
///   flatten tolerance, from the region's outline and islands, arcs
///   measured as arcs. Every circle counts, a hole the countersink skips
///   (`countersink_mouth_not_wider_than_hole`) included; the targets
///   that are no circle are not cut, so they are left out.
///
/// The plane's depth is not compared with the start depth here: a floor
/// at another depth still holds the footprint, which
/// `start_depth_mismatch` reports ([`ClearedPlane::is_at`]).
#[must_use]
pub fn cleared_start_plane<'p>(project: &'p Project, op: &Operation) -> Option<ClearedPlane<'p>> {
    let index = project.operations.iter().position(|o| o.id() == op.id())?;
    let tolerance_mm =
        if project.flatten_tolerance_mm.is_finite() && project.flatten_tolerance_mm > 0.0 {
            project.flatten_tolerance_mm.min(MAX_FLATTEN_TOLERANCE_MM)
        } else {
            FLATTEN_TOLERANCE_DEFAULT_MM
        };
    let footprint = Footprint::of(project, op, tolerance_mm)?;
    project
        .operations
        .iter()
        .take(index)
        .rev()
        .find_map(|earlier| {
            let regions = match earlier {
                Operation::Pocket(pocket) if pocket.enabled => {
                    target_regions(project, &pocket.targets)?
                }
                Operation::VCarve(carve) if carve.enabled => {
                    v_carve_floor(project, carve, tolerance_mm)?
                }
                _ => return None,
            };
            footprint
                .held_by(&regions)
                .then_some(ClearedPlane { by: earlier })
        })
}

/// The closed outlines of `targets` nested even-odd, as a Pocket and a
/// V-Carve nest them; `None` when they do not resolve.
fn target_regions(project: &Project, targets: &[InstanceId]) -> Option<Vec<Region>> {
    let (regions, _) = nest(closed_outlines(project, targets)?).ok()?;
    Some(regions)
}

/// The floor regions of V-Carve `carve` (#426): its targets' regions
/// inset by its own bit's half-width at the flat depth, `w(F)` (`F ·
/// tan(α/2)` for a V-bit; #679), less those too thin to cut (area at most
/// `tolerance_mm` times the perimeter, the generator's noise rule).
/// `None` without a flat depth above 0, without a usable V-bit or tapered
/// ball nose ([`crate::Tool::carve_profile`]), with a start depth that is not 0 or more,
/// or when the targets do not resolve.
fn v_carve_floor(project: &Project, carve: &VCarveOp, tolerance_mm: f64) -> Option<Vec<Region>> {
    let flat_depth_mm = carve.flat_depth_mm.filter(|f| f.is_finite() && *f > 0.0)?;
    if !(carve.start_depth_mm.is_finite() && carve.start_depth_mm >= 0.0) {
        return None;
    }
    let tool = project.tools.iter().find(|t| t.id == carve.tool)?;
    // Slice 27c (#679): the bit's half-width at the flat depth, `w(F)`,
    // which for a V-bit is `F · tan(α/2)`.
    let inset_mm = tool.carve_profile()?.half_width_mm(flat_depth_mm);
    if !(inset_mm.is_finite() && inset_mm > 0.0) {
        return None;
    }
    let regions = target_regions(project, &carve.targets)?;
    Some(
        floor_regions(&regions, inset_mm, tolerance_mm)
            .into_iter()
            .filter(|floor| {
                let perimeter_mm: f64 = std::iter::once(floor.outer())
                    .chain(floor.holes())
                    .map(|c| c.polyline().length_mm())
                    .sum();
                floor.area_mm2() > tolerance_mm * perimeter_mm
            })
            .collect(),
    )
}

/// What an operation cut from a start plane occupies at that plane, which
/// the regions of an earlier operation must hold for its floor to be the
/// plane ([`cleared_start_plane`]).
enum Footprint {
    /// A V-Carve's closed target outlines.
    Outlines {
        /// The outlines, as placed.
        outlines: Vec<Contour>,
        /// The outlines nested even-odd, as the carve nests them.
        regions: Vec<Region>,
        /// The flatten tolerance, mm, which the area test is within.
        tolerance_mm: f64,
        /// The area the test lets stand outside, mm²: the tolerance times
        /// the outlines' length.
        slack_mm2: f64,
    },
    /// A countersink's discs: each hole's centre with the tool's radius.
    Discs {
        /// The centres, as placed.
        centres: Vec<Point>,
        /// The tool's radius, mm.
        radius_mm: f64,
        /// How far, mm, a disc may reach past a wall.
        tolerance_mm: f64,
    },
}

impl Footprint {
    /// The footprint of `op`; `None` for an operation not cut from a start
    /// plane, or one whose footprint does not resolve or is empty.
    fn of(project: &Project, op: &Operation, tolerance_mm: f64) -> Option<Self> {
        match op {
            Operation::VCarve(carve) => {
                Self::outlines(closed_outlines(project, &carve.targets)?, tolerance_mm)
            }
            // Format 27: a Photo V-Carve cuts inside its picture's
            // footprint rectangle (design spec, "Start depth", step 1).
            Operation::PhotoVCarve(photo) => {
                let bitmap = project.bitmaps.iter().find(|b| b.id == photo.image)?;
                Self::outlines(vec![bitmap_outline(bitmap)?], tolerance_mm)
            }
            Operation::Drill(drill) if drill.cycle.is_countersink() => {
                let tool = project.tools.iter().find(|t| t.id == drill.tool)?;
                let radius_mm = tool.diameter_mm / 2.0;
                if !(radius_mm.is_finite() && radius_mm > 0.0) {
                    return None;
                }
                let centres: Vec<Point> = closed_outlines(project, &drill.targets)?
                    .iter()
                    .filter_map(|c| as_circle(c.polyline(), project.flatten_tolerance_mm))
                    .map(|(centre, _)| centre)
                    .collect();
                (!centres.is_empty()).then_some(Self::Discs {
                    centres,
                    radius_mm,
                    tolerance_mm,
                })
            }
            // An Inlay has no start depth (format 22; design spec, "Start
            // depth"): the female starts at the stock top, and the male's
            // stock above its start plane is the plug, kept on purpose.
            Operation::Profile(_)
            | Operation::Pocket(_)
            | Operation::Drill(_)
            | Operation::Bore(_)
            | Operation::Engrave(_)
            | Operation::Inlay(_) => None,
        }
    }

    /// The footprint of closed `outlines`, nested even-odd; `None` when
    /// there are none or they do not nest.
    fn outlines(outlines: Vec<Contour>, tolerance_mm: f64) -> Option<Self> {
        if outlines.is_empty() {
            return None;
        }
        let (regions, _) = nest(outlines.clone()).ok()?;
        let slack_mm2 = tolerance_mm
            * outlines
                .iter()
                .map(|c| c.polyline().length_mm())
                .sum::<f64>();
        Some(Self::Outlines {
            outlines,
            regions,
            tolerance_mm,
            slack_mm2,
        })
    }

    /// True when `regions` hold the whole footprint, inside a region and
    /// outside its islands.
    fn held_by(&self, regions: &[Region]) -> bool {
        match self {
            Self::Outlines {
                outlines,
                regions: footprint,
                tolerance_mm,
                slack_mm2,
            } => {
                let vertices_inside = outlines.iter().all(|outline| {
                    outline.polyline().vertices().iter().all(|v| {
                        let p = Point::new(v.x, v.y);
                        regions.iter().any(|r| r.contains_point(p))
                    })
                });
                vertices_inside
                    && area_mm2(&difference(footprint, regions, *tolerance_mm)) <= *slack_mm2
            }
            Self::Discs {
                centres,
                radius_mm,
                tolerance_mm,
            } => centres.iter().all(|&centre| {
                regions.iter().any(|region| {
                    region.contains_point(centre)
                        && std::iter::once(region.outer())
                            .chain(region.holes())
                            .flat_map(|wall| wall.polyline().segments())
                            .all(|seg| {
                                point_segment_distance_mm(seg, centre) >= radius_mm - tolerance_mm
                            })
                })
            }),
        }
    }
}

/// The footprint rectangle of `bitmap` as a closed outline, in project
/// space (Y-up): from (`x_mm`, `y_mm`), `width_mm` wide and `height_mm`
/// tall. `None` when it is no rectangle: a value is not finite, or a side
/// is not above 0, which validation reports.
fn bitmap_outline(bitmap: &Bitmap) -> Option<Contour> {
    let values = [bitmap.x_mm, bitmap.y_mm, bitmap.width_mm, bitmap.height_mm];
    if !(values.iter().all(|v| v.is_finite()) && bitmap.width_mm > 0.0 && bitmap.height_mm > 0.0) {
        return None;
    }
    let corner = Point::new(bitmap.x_mm, bitmap.y_mm);
    Contour::new(Polyline::rect(corner, bitmap.width_mm, bitmap.height_mm)).ok()
}

/// The closed outlines of `targets`, placed in project space, in target
/// then profile order; `None` when one does not resolve.
pub(crate) fn closed_outlines(project: &Project, targets: &[InstanceId]) -> Option<Vec<Contour>> {
    let mut outlines = Vec::new();
    for id in targets {
        let instance = project.instances.iter().find(|i| &i.id == id)?;
        let definition = project.definitions.get(&instance.definition)?;
        for profile in definition.profiles.iter().filter(|p| p.is_closed()) {
            let placed = instance.transform.apply_polyline(profile).ok()?;
            outlines.push(Contour::new(placed).ok()?);
        }
    }
    Some(outlines)
}

#[cfg(test)]
mod tests;
