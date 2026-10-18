// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Does a toolpath stay on the stock? (#62; `docs/specs/phase-1/design.md`,
//! "Algorithms → Profile contour", step 8.) The swept extent of an operation is the
//! XY bounds of its cutting moves, arcs' bulges included, grown by the tool
//! radius. Any part of it outside the stock rectangle is an overhang: the
//! cutter would leave the material, usually because a shape was moved or
//! scaled off the stock, and could run into clamps or the machine.

use std::fmt;

use dc_geometry::{Arc, EPS_MM, Point, Rect};
use dc_project::Stock;

use crate::{MoveKind, Toolpath};

/// How far an operation's swept extent runs past each stock edge, mm.
/// Each value is 0 or more.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Overhang {
    /// Past the left edge, X = 0.
    pub min_x_mm: f64,
    /// Past the right edge, X = stock width.
    pub max_x_mm: f64,
    /// Past the front edge, Y = 0.
    pub min_y_mm: f64,
    /// Past the back edge, Y = stock height.
    pub max_y_mm: f64,
}

impl Overhang {
    /// Each side's name and overhang, mm.
    fn sides(&self) -> [(&'static str, f64); 4] {
        [
            ("-X (left)", self.min_x_mm),
            ("+X (right)", self.max_x_mm),
            ("-Y (front)", self.min_y_mm),
            ("+Y (back)", self.max_y_mm),
        ]
    }
}

impl fmt::Display for Overhang {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for (side, overhang_mm) in self.sides() {
            if overhang_mm > OVERHANG_TOLERANCE_MM {
                if !first {
                    f.write_str(", ")?;
                }
                write!(f, "{overhang_mm:.3} mm past the {side} edge")?;
                first = false;
            }
        }
        Ok(())
    }
}

/// An overhang up to this is rounding, not a cut off the stock. A cut
/// that runs exactly along an edge stays on the stock.
pub const OVERHANG_TOLERANCE_MM: f64 = EPS_MM;

/// The XY bounds of `toolpath`'s cutting moves, arc bulges included,
/// grown by `tool_radius_mm`: everything the cutter touches. `None` when
/// the toolpath does not cut. Rapids are left out: S2 keeps them at safe
/// Z, above the stock.
#[must_use]
pub fn swept_bounds(toolpath: &Toolpath, tool_radius_mm: f64) -> Option<Rect> {
    let mut bounds: Option<Rect> = None;
    let mut include = |p: Point| {
        bounds = Some(bounds.map_or_else(|| Rect::from_point(p), |b| b.including(p)));
    };
    let mut previous: Option<Point> = None;
    for mv in &toolpath.moves {
        let end = mv.to.xy();
        match mv.kind {
            MoveKind::Rapid => {}
            MoveKind::Linear => {
                if let Some(start) = previous {
                    include(start);
                }
                include(end);
            }
            MoveKind::ArcCw { centre } | MoveKind::ArcCcw { centre } => {
                include(end);
                if let Some(start) = previous {
                    include(start);
                    let ccw = matches!(mv.kind, MoveKind::ArcCcw { .. });
                    Arc::from_ends(start, end, centre, ccw)
                        .axis_extremes()
                        .for_each(&mut include);
                }
            }
        }
        previous = Some(end);
    }
    bounds.map(|b| Rect {
        min: Point::new(b.min.x - tool_radius_mm, b.min.y - tool_radius_mm),
        max: Point::new(b.max.x + tool_radius_mm, b.max.y + tool_radius_mm),
    })
}

/// How far `swept` runs past `stock`'s edges, or `None` when it stays on
/// the stock within [`OVERHANG_TOLERANCE_MM`].
#[must_use]
pub fn stock_overhang(stock: &Stock, swept: Rect) -> Option<Overhang> {
    let overhang = Overhang {
        min_x_mm: (-swept.min.x).max(0.0),
        max_x_mm: (swept.max.x - stock.width_mm).max(0.0),
        min_y_mm: (-swept.min.y).max(0.0),
        max_y_mm: (swept.max.y - stock.height_mm).max(0.0),
    };
    overhang
        .sides()
        .iter()
        .any(|(_, mm)| *mm > OVERHANG_TOLERANCE_MM)
        .then_some(overhang)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Move;
    use dc_geometry::{Point3, assert_approx_eq};

    const TOL: f64 = 1e-9;

    fn stock() -> Stock {
        Stock {
            width_mm: 100.0,
            height_mm: 50.0,
            thickness_mm: 6.0,
        }
    }

    fn cut(moves: Vec<Move>) -> Toolpath {
        Toolpath { moves }
    }

    #[test]
    fn rapids_do_not_count() {
        let tp = cut(vec![
            Move::rapid(Point3::new(-50.0, -50.0, 11.0)),
            Move::rapid(Point3::new(10.0, 10.0, 11.0)),
            Move::linear(Point3::new(10.0, 10.0, 3.0), 300.0),
            Move::linear(Point3::new(20.0, 10.0, 3.0), 1000.0),
            Move::rapid(Point3::new(500.0, 10.0, 11.0)),
        ]);
        let swept = swept_bounds(&tp, 2.0).unwrap();
        assert_approx_eq!(swept.min.x, 8.0, TOL);
        assert_approx_eq!(swept.max.x, 22.0, TOL);
        assert_approx_eq!(swept.min.y, 8.0, TOL);
        assert_approx_eq!(swept.max.y, 12.0, TOL);
        assert!(stock_overhang(&stock(), swept).is_none());
        assert!(swept_bounds(&cut(vec![]), 2.0).is_none());
    }

    #[test]
    fn arc_bulge_counts() {
        // A clockwise half circle over the top, radius 10 about (50, 40):
        // its ends are at Y = 40, but it reaches Y = 50, and the 3 mm tool
        // 3 mm past the 50 mm stock.
        let tp = cut(vec![
            Move::rapid(Point3::new(40.0, 40.0, 11.0)),
            Move::linear(Point3::new(40.0, 40.0, 3.0), 300.0),
            Move {
                kind: MoveKind::ArcCw {
                    centre: Point::new(50.0, 40.0),
                },
                to: Point3::new(60.0, 40.0, 3.0),
                feed_mm_min: Some(1000.0),
            },
        ]);
        let swept = swept_bounds(&tp, 3.0).unwrap();
        assert_approx_eq!(swept.max.y, 53.0, TOL);
        let overhang = stock_overhang(&stock(), swept).unwrap();
        assert_approx_eq!(overhang.max_y_mm, 3.0, TOL);
        assert_approx_eq!(overhang.min_x_mm, 0.0, TOL);
        assert_eq!(overhang.to_string(), "3.000 mm past the +Y (back) edge");

        // The same ends counter-clockwise go under, to Y = 30.
        let mut ccw = tp;
        ccw.moves[2].kind = MoveKind::ArcCcw {
            centre: Point::new(50.0, 40.0),
        };
        let swept = swept_bounds(&ccw, 3.0).unwrap();
        assert_approx_eq!(swept.max.y, 43.0, TOL);
        assert_approx_eq!(swept.min.y, 27.0, TOL);
        assert!(stock_overhang(&stock(), swept).is_none());
    }

    #[test]
    fn edge_to_edge_is_on_the_stock() {
        let swept = Rect {
            min: Point::new(0.0, 0.0),
            max: Point::new(100.0, 50.0),
        };
        assert!(stock_overhang(&stock(), swept).is_none());
        let swept = Rect {
            min: Point::new(-1.5, 0.0),
            max: Point::new(102.0, 50.0),
        };
        let overhang = stock_overhang(&stock(), swept).unwrap();
        assert_eq!(
            overhang.to_string(),
            "1.500 mm past the -X (left) edge, 2.000 mm past the +X (right) edge"
        );
    }
}
