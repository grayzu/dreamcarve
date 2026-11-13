// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! HATCH boundary paths (#668): the loops a HATCH fills, read from its
//! groups into lines, arcs, elliptic arcs, splines and bulge polylines in
//! the hatch's object coordinate system. PureCutCNC does not read HATCH;
//! the group codes are those of Autodesk's DXF Reference ("HATCH",
//! "Boundary Path Data"). Two points the reference leaves open follow the
//! MIT-licensed ezdxf's reading of the format, which is checked against
//! AutoCAD's own files: the start and end angles (50/51) of a clockwise
//! arc or elliptic-arc edge (73 = 0) are stored as their complements
//! (360° − angle), and an elliptic arc's angles are angles from the major
//! axis, not ellipse parameters.
//!
//! A DXF is untrusted: every count the file declares (boundary paths,
//! edges, polyline vertices, spline knots and control points) is checked
//! against [`MAX_HATCH_BOUNDARIES`] and [`MAX_HATCH_EDGES`] and charged to
//! the import's vertex budget before anything is read for it, and no
//! buffer is sized from a declared count.

use dc_geometry::{Point, Vec2, Vertex};

use super::HatchProblem;
use super::groups::Group;
use crate::ImportError;

/// Most boundary paths (loops) one HATCH may declare; more is
/// [`ImportError::DxfHatchTooLarge`].
pub const MAX_HATCH_BOUNDARIES: usize = 10_000;

/// Most edges, polyline vertices, spline knots and spline control points
/// one HATCH may declare across its boundary paths; more is
/// [`ImportError::DxfHatchTooLarge`].
pub const MAX_HATCH_EDGES: usize = 100_000;

/// Boundary path type flag (group 92) bit: the path is a polyline.
const PATH_IS_POLYLINE: i64 = 2;

/// One boundary path, in the hatch's OCS and drawing units.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Boundary {
    /// A polyline path (92 bit 2): vertices with bulges, always closed.
    Polyline(Vec<Vertex>),
    /// An edge path: edges end to end.
    Edges(Vec<Edge>),
}

/// One edge of an edge path (72 edge type 1 to 4).
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Edge {
    /// 72 = 1: 10/20 start, 11/21 end.
    Line { start: Point, end: Point },
    /// 72 = 2: 10/20 centre, 40 radius, 50/51 start and end angle in
    /// degrees, 73 counter-clockwise.
    Arc {
        centre: Point,
        radius: f64,
        start_deg: f64,
        end_deg: f64,
        ccw: bool,
    },
    /// 72 = 3: 10/20 centre, 11/21 major axis end relative to the centre,
    /// 40 minor-to-major ratio, 50/51 start and end angle in degrees, 73
    /// counter-clockwise.
    Ellipse {
        centre: Point,
        major: Vec2,
        ratio: f64,
        start_deg: f64,
        end_deg: f64,
        ccw: bool,
    },
    /// 72 = 4: 94 degree, 73 rational, 74 periodic, 95/96 knot and control
    /// point counts, 40 knots, 10/20 control points, 42 weights, then
    /// optionally 97 fit point count, 11/21 fit points and 12/22, 13/23
    /// tangents.
    Spline {
        degree: usize,
        knots: Vec<f64>,
        control: Vec<Point>,
        weights: Vec<f64>,
        fit: Vec<Point>,
    },
}

/// Why a HATCH's boundaries were not read.
#[derive(Debug)]
pub(super) enum Failure {
    /// The whole import fails: a cap was passed.
    Import(ImportError),
    /// This hatch is skipped with a warning.
    Unreadable(HatchProblem),
}

impl From<ImportError> for Failure {
    fn from(e: ImportError) -> Self {
        Self::Import(e)
    }
}

impl From<HatchProblem> for Failure {
    fn from(p: HatchProblem) -> Self {
        Self::Unreadable(p)
    }
}

/// The boundary paths of the HATCH whose own groups are `groups`, read
/// from its boundary path count (91) to its last path. `hatch` names it in
/// errors; `charge` takes each declared count from the import's vertex
/// budget before the items it counts are read.
pub(super) fn boundaries(
    groups: &[Group],
    hatch: &str,
    charge: &mut dyn FnMut(usize) -> Result<(), ImportError>,
) -> Result<Vec<Boundary>, Failure> {
    // The path count follows the AcDbHatch header (elevation, extrusion,
    // pattern name, solid fill and associativity flags); no code before it
    // is 91.
    let Some(start) = groups.iter().position(|g| g.code == 91) else {
        return Err(HatchProblem::NoBoundary.into());
    };
    let mut r = Reader {
        groups,
        at: start,
        boundary: 0,
    };
    let declared = r.int(91)?;
    let count = r.count(declared, 91, MAX_HATCH_BOUNDARIES, "boundary paths", hatch)?;
    charge(count)?;
    let mut budget = EdgeBudget {
        used: 0,
        hatch,
        charge,
    };
    let mut out = Vec::new();
    for index in 0..count {
        r.boundary = index + 1;
        let flags = r.int(92)?;
        let boundary = if flags & PATH_IS_POLYLINE != 0 {
            r.polyline(&mut budget)?
        } else {
            r.edges(&mut budget)?
        };
        r.source_objects();
        out.push(boundary);
    }
    Ok(out)
}

/// The edges, vertices, knots and control points of one HATCH so far,
/// against [`MAX_HATCH_EDGES`] and the import's vertex budget.
struct EdgeBudget<'a, 'b> {
    used: usize,
    hatch: &'a str,
    charge: &'b mut dyn FnMut(usize) -> Result<(), ImportError>,
}

impl EdgeBudget<'_, '_> {
    fn take(&mut self, n: usize, declared: i64) -> Result<(), Failure> {
        self.used = self.used.saturating_add(n);
        if self.used > MAX_HATCH_EDGES {
            return Err(ImportError::DxfHatchTooLarge {
                hatch: self.hatch.to_owned(),
                what: "edges and vertices",
                declared,
                limit: MAX_HATCH_EDGES,
            }
            .into());
        }
        (self.charge)(n)?;
        Ok(())
    }
}

/// A cursor over a HATCH's groups. Boundary data is positional: each read
/// expects the next group to have the code the reference puts there.
struct Reader<'a> {
    groups: &'a [Group],
    at: usize,
    /// The boundary path being read, from 1; 0 before the first.
    boundary: usize,
}

impl<'a> Reader<'a> {
    fn peek_code(&self) -> Option<i32> {
        self.groups.get(self.at).map(|g| g.code)
    }

    /// The next group, which must have `code`.
    fn take(&mut self, code: i32) -> Result<&'a Group, Failure> {
        match self.groups.get(self.at) {
            Some(g) if g.code == code => {
                self.at += 1;
                Ok(g)
            }
            _ => Err(self.malformed(code)),
        }
    }

    /// The next group when it has `code`.
    fn optional(&mut self, code: i32) -> Option<&'a Group> {
        let g = self.groups.get(self.at).filter(|g| g.code == code)?;
        self.at += 1;
        Some(g)
    }

    fn malformed(&self, code: i32) -> Failure {
        HatchProblem::Malformed {
            boundary: self.boundary,
            code,
        }
        .into()
    }

    fn real(&mut self, code: i32) -> Result<f64, Failure> {
        let g = self.take(code)?;
        g.real().ok_or_else(|| self.malformed(code))
    }

    fn int(&mut self, code: i32) -> Result<i64, Failure> {
        let g = self.take(code)?;
        g.int().ok_or_else(|| self.malformed(code))
    }

    fn point(&mut self, x_code: i32) -> Result<Point, Failure> {
        Ok(Point::new(self.real(x_code)?, self.real(x_code + 10)?))
    }

    /// A declared count as a `usize`: negative is malformed, above `limit`
    /// is [`ImportError::DxfHatchTooLarge`].
    fn count(
        &self,
        declared: i64,
        code: i32,
        limit: usize,
        what: &'static str,
        hatch: &str,
    ) -> Result<usize, Failure> {
        let n = usize::try_from(declared).map_err(|_| self.malformed(code))?;
        if n > limit {
            return Err(ImportError::DxfHatchTooLarge {
                hatch: hatch.to_owned(),
                what,
                declared,
                limit,
            }
            .into());
        }
        Ok(n)
    }

    /// A count of edges, vertices, knots or control points, charged to
    /// `budget` before any is read.
    fn charged_count(
        &mut self,
        code: i32,
        budget: &mut EdgeBudget<'_, '_>,
    ) -> Result<usize, Failure> {
        let declared = self.int(code)?;
        let n = self.count(
            declared,
            code,
            MAX_HATCH_EDGES,
            "edges and vertices",
            budget.hatch,
        )?;
        budget.take(n, declared)?;
        Ok(n)
    }

    /// A polyline path: 72 has-bulge, 73 is-closed, 93 vertex count, then
    /// 10/20 and 42 per vertex. A bulge is read wherever it is written,
    /// whatever 72 says.
    fn polyline(&mut self, budget: &mut EdgeBudget<'_, '_>) -> Result<Boundary, Failure> {
        self.optional(72);
        self.optional(73);
        let n = self.charged_count(93, budget)?;
        let mut vertices = Vec::new();
        for _ in 0..n {
            let p = self.point(10)?;
            let bulge = match self.optional(42) {
                Some(g) => g.real().ok_or_else(|| self.malformed(42))?,
                None => 0.0,
            };
            vertices.push(Vertex::new(p.x, p.y, bulge));
        }
        Ok(Boundary::Polyline(vertices))
    }

    /// An edge path: 93 edge count, then each edge from its 72 type.
    fn edges(&mut self, budget: &mut EdgeBudget<'_, '_>) -> Result<Boundary, Failure> {
        let n = self.charged_count(93, budget)?;
        let mut edges = Vec::new();
        for _ in 0..n {
            let edge = match self.int(72)? {
                1 => Edge::Line {
                    start: self.point(10)?,
                    end: self.point(11)?,
                },
                2 => Edge::Arc {
                    centre: self.point(10)?,
                    radius: self.real(40)?,
                    start_deg: self.real(50)?,
                    end_deg: self.real(51)?,
                    ccw: self.counter_clockwise(),
                },
                3 => {
                    let centre = self.point(10)?;
                    let major = self.point(11)?;
                    Edge::Ellipse {
                        centre,
                        major: Vec2::new(major.x, major.y),
                        ratio: self.real(40)?,
                        start_deg: self.real(50)?,
                        end_deg: self.real(51)?,
                        ccw: self.counter_clockwise(),
                    }
                }
                4 => self.spline(budget)?,
                edge_type => {
                    return Err(HatchProblem::UnknownEdge {
                        boundary: self.boundary,
                        edge_type,
                    }
                    .into());
                }
            };
            edges.push(edge);
        }
        Ok(Boundary::Edges(edges))
    }

    /// The counter-clockwise flag (73) of an arc or elliptic-arc edge;
    /// counter-clockwise when absent.
    fn counter_clockwise(&mut self) -> bool {
        self.optional(73)
            .and_then(Group::int)
            .is_none_or(|flag| flag != 0)
    }

    /// A spline edge, after its 72.
    fn spline(&mut self, budget: &mut EdgeBudget<'_, '_>) -> Result<Edge, Failure> {
        let degree = self.int(94)?;
        let degree = usize::try_from(degree).map_err(|_| self.malformed(94))?;
        self.optional(73);
        self.optional(74);
        let knot_count = self.charged_count(95, budget)?;
        let control_count = self.charged_count(96, budget)?;
        let mut knots = Vec::new();
        for _ in 0..knot_count {
            knots.push(self.real(40)?);
        }
        // Weights follow each control point, or all of them; either way
        // they are read in order.
        let mut control = Vec::new();
        let mut weights = Vec::new();
        for _ in 0..control_count {
            control.push(self.point(10)?);
            if let Some(w) = self.optional(42) {
                weights.push(w.real().ok_or_else(|| self.malformed(42))?);
            }
        }
        while weights.len() < control_count
            && let Some(w) = self.optional(42)
        {
            weights.push(w.real().ok_or_else(|| self.malformed(42))?);
        }
        let mut fit = Vec::new();
        if self.fit_data_follows() {
            let fit_count = self.charged_count(97, budget)?;
            for _ in 0..fit_count {
                fit.push(self.point(11)?);
            }
            for code in [12, 13] {
                if self.optional(code).is_some() {
                    self.real(code + 10)?;
                }
            }
        }
        Ok(Edge::Spline {
            degree,
            knots,
            control,
            weights,
            fit,
        })
    }

    /// Whether the next group, a 97, is a spline edge's fit point count
    /// (written from AutoCAD 2010 on) rather than its boundary path's
    /// source object count, which is also 97. A fit count is followed by
    /// fit points (11), tangents (12, 13), the next edge (72) or the source
    /// count (97); a source count by handles (330) or the next path (92) or
    /// the hatch's style (75).
    fn fit_data_follows(&self) -> bool {
        self.peek_code() == Some(97)
            && self
                .groups
                .get(self.at + 1)
                .is_some_and(|g| matches!(g.code, 11 | 12 | 13 | 72 | 97))
    }

    /// The source boundary objects that end a path: 97 count, then that
    /// many 330 handles. Absent in some files; nothing is kept.
    fn source_objects(&mut self) {
        if self.optional(97).is_some() {
            while self.optional(330).is_some() {}
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::panic, reason = "tests")]

    use super::*;
    use crate::dxf::code_page::CodePage;
    use crate::dxf::groups;

    fn read(text: &str) -> Result<Vec<Boundary>, Failure> {
        let groups = groups::read(text.as_bytes(), CodePage::WINDOWS_1252)
            .unwrap()
            .groups;
        boundaries(&groups, "1F", &mut |_| Ok(()))
    }

    /// A spline edge with R2010 fit data (97 0) is followed by the path's
    /// own source count (97 0): the first is the fit count, the second the
    /// sources. An older file without fit data has only the second.
    #[test]
    fn spline_fit_count_is_told_from_the_source_count() {
        let spline = "72\n4\n94\n1\n73\n0\n74\n0\n95\n4\n96\n2\n\
            40\n0.0\n40\n0.0\n40\n1.0\n40\n1.0\n10\n0.0\n20\n0.0\n10\n5.0\n20\n0.0\n";
        let with_fit = format!("91\n1\n92\n0\n93\n1\n{spline}97\n0\n97\n0\n75\n0\n");
        let without = format!("91\n1\n92\n0\n93\n1\n{spline}97\n1\n330\nAB\n75\n0\n");
        for text in [with_fit, without] {
            let boundaries = read(&text).unwrap();
            let [Boundary::Edges(edges)] = boundaries.as_slice() else {
                panic!("{boundaries:?}");
            };
            assert!(matches!(&edges[0], Edge::Spline { control, .. } if control.len() == 2));
        }
    }

    #[test]
    fn missing_group_is_malformed_and_names_the_boundary() {
        let Err(Failure::Unreadable(problem)) =
            read("91\n2\n92\n2\n93\n1\n10\n0.0\n20\n0.0\n92\n0\n93\n1\n72\n1\n10\n0.0\n")
        else {
            panic!("malformed");
        };
        assert_eq!(
            problem,
            HatchProblem::Malformed {
                boundary: 2,
                code: 20
            }
        );
    }
}
