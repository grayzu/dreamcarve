// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! The grid a program writes a V-Carve's tip on, and the chain ends
//! started on it (#499; `docs/specs/phase-3/design.md`, "V-Carve
//! generator", step 5, the written-precision paragraph).
//!
//! A program writes every point to its decimal places, which can move it
//! by up to `w = quantum / √2`. So the medial chains keep their tip paths
//! `w` from the walls (#469, `medial::clear_of_walls`): written, a path no
//! nearer can't cross one. At a corner of angle `θ` that stops the groove
//! where the tip is `w` from both walls, `w / sin(θ/2)` short of the tip,
//! and a male Inlay's plug keeps wood in the corner up to
//! `w (1/sin(θ/2) − 1) / tan(α/2)` above its start plane: 0.12 mm with a
//! 15° bit in a 5° notch.
//!
//! Most of that margin isn't needed. A point of the grid the program
//! writes on is written exactly, so the groove may run on from where the
//! margin stops it to a grid point nearer the corner, as long as the move
//! as written stays inside: from the grid point, exact, to the margin's
//! point, moved by up to `w`. [`snapped_ends`] adds that move at each end
//! of a chain the margin cut short. Nothing a program writes crosses a
//! wall that the margin's rule would not let cross.
//!
//! The grid is coarse beside a sharp corner, so the corner is not always
//! reached: a 5° corner is about `q / 11` wide at `q` from its tip, and
//! the nearest grid point inside may be several units in. That is the
//! limit of the program's places, not of the generator.

use crate::vcarve::cone::{OpenEdgeWalls, Walls};
use crate::vcarve::medial::Depth;
use dc_geometry::{EPS_MM, Point, Point3};
use dc_project::Project;

/// How many grid units from a chain's end [`snapped_ends`] looks for a
/// start at most, each way: a 2° corner in a four-place inch program
/// needs about 41 (`w / sin 1°` is 0.103 mm, and a unit 0.00254 mm).
const MAX_STEPS: f64 = 64.0;

/// The grid a program writes XY on, in project space (#499): the work
/// zero, then whole units of the last written place.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct WrittenGrid {
    /// The work zero, mm, project space.
    pub origin: Point,
    /// One unit in the last written place, mm.
    pub quantum_mm: f64,
}

impl WrittenGrid {
    /// The grid `project`'s programs write on: its machine origin
    /// ([`dc_project::OriginXy::point_mm`]) and one unit in the last
    /// place of its post-processor, in its display units. Every axis
    /// mapping a post-processor may have only swaps or mirrors the axes
    /// about the origin, which leaves the grid as it is.
    pub(crate) fn of(project: &Project) -> Self {
        Self {
            origin: project.origin.xy.point_mm(&project.stock),
            quantum_mm: project
                .post_processor
                .decimal_places
                .quantum_mm(project.display_units),
        }
    }

    /// How far writing can move a point, mm: half a unit on each axis,
    /// `quantum / √2` (#469).
    pub(crate) fn margin_mm(&self) -> f64 {
        self.quantum_mm * std::f64::consts::FRAC_1_SQRT_2
    }

    /// The grid points within `radius_mm` of `centre`, at most
    /// [`MAX_STEPS`] units out each way.
    fn points_near(&self, centre: Point, radius_mm: f64) -> Vec<Point> {
        let q = self.quantum_mm;
        if !(q > 0.0 && radius_mm > 0.0) {
            return Vec::new();
        }
        let steps = (radius_mm / q).ceil().min(MAX_STEPS);
        let (i0, j0) = (
            ((centre.x - self.origin.x) / q).round(),
            ((centre.y - self.origin.y) / q).round(),
        );
        let mut out = Vec::new();
        let mut i = -steps;
        while i <= steps {
            let mut j = -steps;
            while j <= steps {
                let p = Point::new(self.origin.x + (i0 + i) * q, self.origin.y + (j0 + j) * q);
                if p.distance_to(centre) < radius_mm {
                    out.push(p);
                }
                j += 1.0;
            }
            i += 1.0;
        }
        out
    }
}

/// What a chain end is snapped against.
pub(crate) struct Snap<'a> {
    pub walls: &'a Walls,
    pub depth: &'a Depth,
    pub grid: WrittenGrid,
    /// A male Inlay's open band edge (#448), which every move must also
    /// keep the cone inside.
    pub edge: Option<&'a OpenEdgeWalls>,
}

/// `pieces`, the chain from `first` to `last` cut to the stretches that
/// keep the written margin from the walls (`medial::clear_of_walls`),
/// with a start on the grid added before the first piece where `first`
/// is nearer a wall than the margin, and after the last where `last` is
/// ([`Snap::start`]). A chain end the margin kept is left as it is. The
/// margin's own point is then dropped where it lies on the straight move
/// from the grid start to the point after it, within [`ON_MOVE_MM`], and
/// that move passes the same tests: a corner on the grid is one straight
/// cut in, as it was before the margin.
pub(crate) fn snapped_ends(
    first: Option<Point3>,
    last: Option<Point3>,
    mut pieces: Vec<Vec<Point3>>,
    snap: &Snap<'_>,
) -> Vec<Vec<Point3>> {
    if let (Some(end), Some(piece)) = (first, pieces.first_mut())
        && let Some(&q) = piece.first()
        && let Some(g) = snap.start(end, q)
    {
        piece.insert(0, g);
        if let (Some(&next), Some(&q)) = (piece.get(2), piece.get(1))
            && on_move(g, q, next)
            && snap.fits(g, next)
        {
            piece.remove(1);
        }
    }
    if let (Some(end), Some(piece)) = (last, pieces.last_mut())
        && let Some(&q) = piece.last()
        && let Some(g) = snap.start(end, q)
    {
        piece.push(g);
        let n = piece.len();
        if let (Some(&before), Some(&q)) = (
            n.checked_sub(3).and_then(|i| piece.get(i)),
            n.checked_sub(2).and_then(|i| piece.get(i)),
        ) && on_move(g, q, before)
            && snap.fits(g, before)
        {
            piece.remove(n - 2);
        }
    }
    pieces
}

/// How near the straight move past it the margin's point may lie and be
/// dropped, mm: a ten-thousandth of a millimetre, far below what a program
/// writes.
const ON_MOVE_MM: f64 = 1e-4;

/// Whether `p` lies on the straight move from `a` to `b`, in XYZ, within
/// [`ON_MOVE_MM`].
fn on_move(a: Point3, p: Point3, b: Point3) -> bool {
    let ab = (b.x - a.x, b.y - a.y, b.z - a.z);
    let ap = (p.x - a.x, p.y - a.y, p.z - a.z);
    let length2 = ab.0 * ab.0 + ab.1 * ab.1 + ab.2 * ab.2;
    if length2 <= 0.0 {
        return false;
    }
    let t = (ap.0 * ab.0 + ap.1 * ab.1 + ap.2 * ab.2) / length2;
    let off = (ap.0 - ab.0 * t, ap.1 - ab.1 * t, ap.2 - ab.2 * t);
    (0.0..=1.0).contains(&t) && (off.0 * off.0 + off.1 * off.1 + off.2 * off.2).sqrt() <= ON_MOVE_MM
}

impl Snap<'_> {
    /// For a chain whose end `end` (a corner's tip, most often) the
    /// written margin cut back to `q`, the grid point to start the groove
    /// at instead, at its own depth: of the grid points nearer `end` than
    /// `q` is, the one that leaves the least wood at `end`, `|end − g| −
    /// clearance(g)` (a male's plug keeps wood in its corner up to that
    /// over `tan(α/2)` above its start plane), among those whose move to
    /// `q` [`Snap::fits`], and that leave less wood than `q` itself,
    /// `|end − q| − margin`. `None` when `end` keeps the margin (the chain
    /// was not cut there) or no grid point will do.
    fn start(&self, end: Point3, q: Point3) -> Option<Point3> {
        let margin_mm = self.grid.margin_mm();
        let reach_mm = end.xy().distance_to(q.xy());
        if reach_mm <= EPS_MM
            || self.walls.path_clearance_mm(end.xy(), end.xy(), margin_mm) >= margin_mm
        {
            return None;
        }
        let clearance = |p: Point| self.walls.path_clearance_mm(p, p, reach_mm);
        let at_q_mm = reach_mm - margin_mm;
        let mut candidates: Vec<(f64, Point)> = self
            .grid
            .points_near(end.xy(), reach_mm)
            .into_iter()
            .map(|g| (g.distance_to(end.xy()) - clearance(g), g))
            .filter(|&(left_mm, g)| left_mm < at_q_mm && g.distance_to(q.xy()) > EPS_MM)
            .collect();
        candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
        candidates.into_iter().find_map(|(_, g)| {
            let start = Point3::new(g.x, g.y, self.depth.z_mm(clearance(g)));
            self.fits(start, q).then_some(start)
        })
    }

    /// Whether the move from `g`, a grid point written exactly, to `q`, a
    /// point that keeps the margin from the walls:
    ///
    /// - as written stays inside the walls, `q` moved by up to the margin
    ///   ([`Walls::widening_clearance_holds`]);
    /// - keeps the cone inside the walls ([`Walls::least_room`]) and the
    ///   open edge, if any.
    fn fits(&self, g: Point3, q: Point3) -> bool {
        self.walls
            .widening_clearance_holds(g.xy(), q.xy(), self.grid.margin_mm())
            && self
                .walls
                .least_room(g, q, self.depth.start_plane_z_mm, self.depth.slope)
                .0
                >= -EPS_MM
            && self.edge.is_none_or(|edge| edge.allows(g, q))
    }
}
