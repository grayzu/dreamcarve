// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0

//! Strategies for random valid projects: one operation of each kind over
//! rectangles and circles, with holes and islands, and for Engrave open
//! paths, on 240 × 160 mm stock, with a random tool, depths, entries, tabs
//! and post-processor.
//!
//! Shapes sit one to a 80 × 80 mm cell of a 3 × 2 grid, each reaching at
//! most 28 mm from its cell's centre, so shapes of different cells never
//! meet and a cutter of up to 8 mm with a lead of up to 4 mm stays on the
//! stock. Within a cell, a hole's circumradius is at most 85 % of its
//! outline's inradius, and an island's of its hole's, so nested shapes
//! never cross (they may be closer than the tool, which Profile refuses
//! with a typed error).

use dc_geometry::{Point, Polyline, Transform, Vertex};
use dc_project::test_support::sample_project;
use dc_project::{
    ClearanceStrategy, CornerRelief, CutDirection, DefinitionId, DrillCycle, DrillOp, EngraveOp,
    Entry, FeatureDefinition, FeatureInstance, InstanceId, LastPass, Lead, MachineOrigin,
    MachiningOrder, Operation, OperationId, OriginXy, OriginZ, PeckRetract, PocketOp, ProfileOp,
    Project, Side, Stock, TabPlacement, TabShape, TabSpec, Tool, ToolId, ToolKind, Units, WallPass,
    latest_definitions,
};
use proptest::prelude::*;
use std::collections::BTreeMap;
use std::f64::consts::TAU;

/// Stock width, mm.
const STOCK_W_MM: f64 = 240.0;
/// Stock height, mm.
const STOCK_H_MM: f64 = 160.0;
/// Side of a grid cell, mm.
const CELL_MM: f64 = 80.0;
/// Cells across.
const CELLS_X: usize = 3;
/// Cells in the grid.
const CELLS: usize = 6;

/// A closed outline about its cell's centre, before rotation.
#[derive(Debug, Clone)]
enum Outline {
    /// A rectangle with this half-diagonal, at this angle from its width.
    Rect { circumradius_mm: f64, angle: f64 },
    /// A circle, as two arcs.
    Circle { radius_mm: f64 },
}

impl Outline {
    /// The radius of the largest circle about the centre inside it.
    fn inradius_mm(&self) -> f64 {
        match *self {
            Self::Rect {
                circumradius_mm,
                angle,
            } => circumradius_mm * angle.cos().min(angle.sin()),
            Self::Circle { radius_mm } => radius_mm,
        }
    }

    fn polyline(&self, reverse: bool) -> Polyline {
        let p = match *self {
            Self::Rect {
                circumradius_mm,
                angle,
            } => {
                let (hw, hh) = (circumradius_mm * angle.cos(), circumradius_mm * angle.sin());
                Polyline::rect(Point::new(-hw, -hh), 2.0 * hw, 2.0 * hh)
            }
            Self::Circle { radius_mm } => Polyline::circle(Point::new(0.0, 0.0), radius_mm),
        };
        if reverse { p.reversed() } else { p }
    }
}

/// A rectangle or circle of circumradius `circumradius_mm`.
fn outline(circumradius_mm: f64) -> impl Strategy<Value = Outline> {
    prop_oneof![
        // 30° to 60°: the shorter side is at least half the half-diagonal.
        (0.52..1.05_f64).prop_map(move |angle| Outline::Rect {
            circumradius_mm,
            angle
        }),
        Just(Outline::Circle {
            radius_mm: circumradius_mm
        }),
    ]
}

/// An outline, its hole and the hole's island, each optional below the
/// outline, the inner one at most 85 % of the outer's inradius.
fn nested() -> impl Strategy<Value = Vec<Outline>> {
    (10.0..28.0_f64)
        .prop_flat_map(outline)
        .prop_flat_map(|outer| {
            let r = outer.inradius_mm();
            (
                Just(outer),
                prop::option::of((0.3..0.85_f64).prop_flat_map(move |f| outline(f * r))),
            )
        })
        .prop_flat_map(|(outer, hole)| {
            let island = match &hole {
                Some(h) => {
                    let r = h.inradius_mm();
                    prop::option::of((0.3..0.85_f64).prop_flat_map(move |f| outline(f * r))).boxed()
                }
                None => Just(None).boxed(),
            };
            (Just(outer), Just(hole), island)
        })
        .prop_map(|(outer, hole, island)| {
            std::iter::once(outer).chain(hole).chain(island).collect()
        })
}

/// An open path of 2 to 5 vertices within 20 mm of the centre, its
/// segments lines or arcs of bulge up to 0.4, so it reaches at most 28 mm.
fn open_path() -> impl Strategy<Value = Polyline> {
    prop::collection::vec(
        (
            0.0..20.0_f64,
            0.0..TAU,
            prop_oneof![Just(0.0), -0.4..0.4_f64],
        ),
        2..=5,
    )
    .prop_map(|points| {
        let vertices = points
            .into_iter()
            .map(|(r, a, bulge)| Vertex::new(r * a.cos(), r * a.sin(), bulge))
            .collect();
        Polyline::new(vertices, false)
    })
}

/// One cell's drawing: its profiles, wound at random, and its rotation.
#[derive(Debug, Clone)]
pub struct Cell {
    profiles: Vec<Polyline>,
    rotation: f64,
}

fn cell(with_open_paths: bool) -> impl Strategy<Value = Cell> {
    let paths = if with_open_paths {
        prop::collection::vec(open_path(), 0..=2).boxed()
    } else {
        Just(Vec::new()).boxed()
    };
    (
        nested(),
        prop::collection::vec(any::<bool>(), 3),
        paths,
        0.0..TAU,
        any::<bool>(),
    )
        .prop_map(|(outlines, reverse, paths, rotation, closed)| {
            let mut profiles: Vec<Polyline> = outlines
                .iter()
                .zip(reverse)
                .map(|(o, r)| o.polyline(r))
                .collect();
            if !closed {
                // An Engrave drawing of open paths alone.
                profiles.clear();
            }
            profiles.extend(paths);
            if profiles.is_empty()
                && let Some(o) = outlines.first()
            {
                profiles.push(o.polyline(false));
            }
            Cell { profiles, rotation }
        })
}

/// 1 to 4 cells, in random grid positions.
fn cells(with_open_paths: bool) -> impl Strategy<Value = Vec<(usize, Cell)>> {
    (
        Just((0..CELLS).collect::<Vec<usize>>()).prop_shuffle(),
        prop::collection::vec(cell(with_open_paths), 1..=4),
    )
        .prop_map(|(slots, cells)| slots.into_iter().zip(cells).collect())
}

/// Everything but the operation.
#[derive(Debug, Clone)]
pub struct Setup {
    cells: Vec<(usize, Cell)>,
    thickness_mm: f64,
    tool: Tool,
    post: usize,
    canned: bool,
    arc_fit: bool,
    inch: bool,
    origin: MachineOrigin,
}

/// The operation's depth and allowance.
#[derive(Debug, Clone, Copy)]
pub struct Depth {
    /// Through the stock, or this fraction of it.
    through: bool,
    fraction: f64,
    allowance_mm: f64,
}

impl Depth {
    fn depth_mm(self, thickness_mm: f64) -> f64 {
        if self.through {
            thickness_mm + self.allowance_mm
        } else {
            self.fraction * thickness_mm
        }
    }
}

fn depth() -> impl Strategy<Value = Depth> {
    (
        any::<bool>(),
        0.05..0.95_f64,
        prop_oneof![Just(0.0), 0.0..1.0_f64],
    )
        .prop_map(|(through, fraction, allowance_mm)| Depth {
            through,
            fraction,
            allowance_mm,
        })
}

fn origin() -> impl Strategy<Value = MachineOrigin> {
    (
        prop_oneof![
            Just(OriginXy::BottomLeft),
            Just(OriginXy::BottomRight),
            Just(OriginXy::TopLeft),
            Just(OriginXy::TopRight),
            Just(OriginXy::Centre),
            (0.0..STOCK_W_MM, 0.0..STOCK_H_MM)
                .prop_map(|(x_mm, y_mm)| OriginXy::Custom { x_mm, y_mm }),
        ],
        prop_oneof![Just(OriginZ::StockTop), Just(OriginZ::StockBottom)],
    )
        .prop_map(|(xy, z)| MachineOrigin { xy, z })
}

/// A cutter of the given kinds with every rate set.
fn tool(kind: impl Strategy<Value = ToolKind>) -> impl Strategy<Value = Tool> {
    (
        kind,
        0.5..8.0_f64,
        100.0..5000.0_f64,
        30.0..3000.0_f64,
        6000_u32..24000,
        0.3..6.0_f64,
        0.1..1.0_f64,
    )
        .prop_map(
            |(cutter, diameter_mm, feed, plunge, rpm, step_down, step_over)| Tool {
                id: ToolId::new("t1"),
                name: "random".into(),
                number: None,
                cutter,
                diameter_mm,
                flutes: 2,
                feed_mm_min: Some(feed),
                plunge_mm_min: Some(plunge),
                rpm: Some(rpm),
                step_down_mm: Some(step_down),
                step_over_fraction: Some(step_over),
            },
        )
}

fn end_mill() -> impl Strategy<Value = ToolKind> {
    Just(ToolKind::EndMill)
}

fn any_cutter() -> impl Strategy<Value = ToolKind> {
    prop_oneof![
        Just(ToolKind::EndMill),
        Just(ToolKind::BallNose),
        (30.0..120.0_f64).prop_map(|included_angle_deg| ToolKind::VBit { included_angle_deg }),
        Just(ToolKind::Engraver),
        Just(ToolKind::Drill),
    ]
}

fn setup(
    kind: impl Strategy<Value = ToolKind>,
    with_open_paths: bool,
) -> impl Strategy<Value = Setup> {
    let posts = latest_definitions().map_or(1, |d| d.len());
    (
        cells(with_open_paths),
        3.0..25.0_f64,
        tool(kind),
        0..posts,
        any::<bool>(),
        any::<bool>(),
        any::<bool>(),
        origin(),
    )
        .prop_map(
            |(cells, thickness_mm, tool, post, canned, arc_fit, inch, origin)| Setup {
                cells,
                thickness_mm,
                tool,
                post,
                canned,
                arc_fit,
                inch,
                origin,
            },
        )
}

/// The project for `setup` with `op` as its one operation, targeting
/// every instance.
fn project(setup: Setup, op: impl FnOnce(&Setup, Vec<InstanceId>) -> Operation) -> Project {
    let mut project = sample_project();
    project.stock = Stock {
        width_mm: STOCK_W_MM,
        height_mm: STOCK_H_MM,
        thickness_mm: Some(setup.thickness_mm),
    };
    project.origin = setup.origin.clone();
    project.display_units = if setup.inch { Units::Inch } else { Units::Mm };
    project.arc_fit_tolerance_mm = setup.arc_fit.then_some(0.01);
    if let Ok(latest) = latest_definitions()
        && let Some(definition) = latest.get(setup.post)
    {
        let mut post = definition.default_ref();
        post.use_canned_cycles &= setup.canned;
        project.post_processor = post;
    }
    project.tools = vec![setup.tool.clone()];
    project.definitions = BTreeMap::new();
    project.instances = Vec::new();
    let mut targets = Vec::new();
    for (slot, cell) in &setup.cells {
        let (column, row) = (slot % CELLS_X, slot / CELLS_X);
        let centre = Point::new(
            CELL_MM * (column as f64 + 0.5),
            CELL_MM * (row as f64 + 0.5),
        );
        let definition = DefinitionId::new(format!("d{slot}"));
        let instance = InstanceId::new(format!("i{slot}"));
        project.definitions.insert(
            definition.clone(),
            FeatureDefinition {
                id: definition.clone(),
                name: format!("Cell {slot}"),
                profiles: cell.profiles.clone(),
            },
        );
        project.instances.push(FeatureInstance {
            id: instance.clone(),
            definition,
            transform: Transform::rotate(cell.rotation)
                .then(Transform::translate(centre.x, centre.y)),
            name: format!("Cell {slot}"),
            visible: true,
            locked: false,
            z_top_mm: Some(setup.thickness_mm),
            z_bottom_mm: Some(0.0),
        });
        targets.push(instance);
    }
    project.operations = vec![op(&setup, targets)];
    project
}

fn order() -> impl Strategy<Value = MachiningOrder> {
    prop_oneof![
        Just(MachiningOrder::FullDepthFirst),
        Just(MachiningOrder::PassFirst)
    ]
}

fn direction() -> impl Strategy<Value = CutDirection> {
    prop_oneof![Just(CutDirection::Climb), Just(CutDirection::Conventional)]
}

fn relief() -> impl Strategy<Value = CornerRelief> {
    prop_oneof![
        Just(CornerRelief::None),
        Just(CornerRelief::Dogbone),
        Just(CornerRelief::TBone)
    ]
}

/// An entry; a helix only when `helix`.
fn entry(helix: bool) -> BoxedStrategy<Entry> {
    let plunge = Just(Entry::Plunge);
    let ramp = (1.0..=45.0_f64).prop_map(|angle_deg| Entry::Ramp { angle_deg });
    if helix {
        prop_oneof![
            plunge,
            ramp,
            (0.05..=1.0_f64, 1.0..=45.0_f64).prop_map(|(diameter_fraction, angle_deg)| {
                Entry::Helix {
                    diameter_fraction,
                    angle_deg,
                }
            }),
        ]
        .boxed()
    } else {
        prop_oneof![plunge, ramp].boxed()
    }
}

fn step_down() -> impl Strategy<Value = Option<f64>> {
    prop::option::of(0.3..6.0_f64)
}

/// Tabs as (placement, width mm, height as a fraction of the stock, shape,
/// whether holes get them too).
fn tabs() -> impl Strategy<Value = Option<(TabPlacement, f64, f64, TabShape, bool)>> {
    prop::option::of((
        prop_oneof![
            (1_u8..=6).prop_map(|count| TabPlacement::Count { count }),
            prop::collection::vec(0.0..300.0_f64, 1..=4)
                .prop_map(|distances_mm| TabPlacement::AtDistances { distances_mm }),
        ],
        1.0..10.0_f64,
        0.05..0.8_f64,
        prop_oneof![Just(TabShape::Rect), Just(TabShape::Smooth)],
        any::<bool>(),
    ))
}

/// A last pass (#284) where the side has a wall to leave it on, as
/// (allowance as a fraction of a third of the bit, thickness as a fraction
/// of the depth, reverse).
fn last_pass(wall: bool) -> BoxedStrategy<Option<(f64, Option<f64>, bool)>> {
    if wall {
        prop::option::of((
            0.05..=1.0_f64,
            prop::option::of(0.05..=1.0_f64),
            any::<bool>(),
        ))
        .boxed()
    } else {
        Just(None).boxed()
    }
}

fn lead() -> impl Strategy<Value = Option<Lead>> {
    prop::option::of(
        (0.5..4.0_f64, 10.0..=180.0_f64, any::<bool>(), 0.0..3.0_f64).prop_map(
            |(radius_mm, sweep_deg, lead_out, overcut_mm)| Lead {
                radius_mm,
                sweep_deg,
                lead_out,
                overcut_mm,
            },
        ),
    )
}

/// A random valid Profile project.
pub fn profile_project() -> impl Strategy<Value = Project> {
    let side = prop_oneof![Just(Side::Inside), Just(Side::Outside), Just(Side::OnLine)];
    profile_project_of(side.boxed(), false)
}

/// A random valid Left or Right Profile project (#207), of closed outlines
/// (which it skips) and open paths with arcs.
pub fn open_side_profile_project() -> impl Strategy<Value = Project> {
    let side = prop_oneof![Just(Side::Left), Just(Side::Right)];
    profile_project_of(side.boxed(), true)
}

/// A random valid Profile project on one of `sides`, with open paths in
/// its drawing when `with_open_paths`.
fn profile_project_of(
    sides: BoxedStrategy<Side>,
    with_open_paths: bool,
) -> impl Strategy<Value = Project> {
    (setup(end_mill(), with_open_paths), sides)
        .prop_flat_map(|(setup, side)| {
            (
                Just(setup),
                Just(side),
                (depth(), step_down(), direction(), 1.0..15.0_f64),
                (
                    tabs(),
                    order(),
                    entry(side.has_waste_area()),
                    lead(),
                    relief(),
                    last_pass(side.has_waste_area()),
                ),
            )
        })
        .prop_map(
            |(
                setup,
                side,
                (depth, step_down_mm, direction, safe_z_mm),
                (tabs, order, entry, lead, relief, last_pass),
            )| {
                project(setup, |s, targets| {
                    let depth_mm = depth.depth_mm(s.thickness_mm);
                    Operation::Profile(ProfileOp {
                        id: OperationId::new("o1"),
                        name: "Profile".into(),
                        enabled: true,
                        targets,
                        tool: s.tool.id.clone(),
                        side,
                        depth_mm: Some(depth_mm),
                        step_down_mm,
                        direction,
                        through_cut_allowance_mm: depth.allowance_mm,
                        safe_z_mm,
                        tabs: tabs.map(|(placement, width_mm, height, shape, on_holes)| TabSpec {
                            placement,
                            width_mm,
                            height_mm: height * s.thickness_mm,
                            shape,
                            on_holes,
                        }),
                        machining_order: order,
                        entry,
                        lead,
                        corner_relief: relief,
                        last_pass: last_pass.map(|(allowance, thickness, reverse)| LastPass {
                            allowance_mm: allowance * s.tool.diameter_mm / 3.0,
                            // At most the depth and the step-down in
                            // effect (#310 review).
                            thickness_mm: thickness.map(|t| {
                                let step_mm =
                                    step_down_mm.or(s.tool.step_down_mm).unwrap_or(depth_mm);
                                t * depth_mm.min(step_mm)
                            }),
                            reverse,
                        }),
                    })
                })
            },
        )
}

/// A random valid Pocket project, offset or raster.
pub fn pocket_project() -> impl Strategy<Value = Project> {
    let strategy = prop_oneof![
        Just(ClearanceStrategy::Offset),
        (0.0..180.0_f64).prop_map(|angle_deg| ClearanceStrategy::Raster { angle_deg }),
    ];
    let wall = prop_oneof![
        Just(WallPass::Last),
        Just(WallPass::First),
        Just(WallPass::None)
    ];
    (
        setup(end_mill(), false),
        (
            strategy,
            wall,
            depth(),
            step_down(),
            prop::option::of(0.1..1.0_f64),
        ),
        (
            direction(),
            prop_oneof![Just(0.0), 0.0..2.0_f64],
            entry(true),
            relief(),
            order(),
        ),
        1.0..15.0_f64,
    )
        .prop_map(
            |(
                setup,
                (strategy, wall_pass, depth, step_down_mm, step_over_fraction),
                (direction, allowance_mm, entry, relief, order),
                safe_z_mm,
            )| {
                project(setup, |s, targets| {
                    Operation::Pocket(PocketOp {
                        id: OperationId::new("o1"),
                        name: "Pocket".into(),
                        enabled: true,
                        targets,
                        tool: s.tool.id.clone(),
                        strategy,
                        wall_pass,
                        depth_mm: Some(depth.depth_mm(s.thickness_mm)),
                        step_down_mm,
                        step_over_fraction,
                        direction,
                        allowance_mm,
                        rest_of: None,
                        entry,
                        corner_relief: relief,
                        machining_order: order,
                        through_cut_allowance_mm: depth.allowance_mm,
                        safe_z_mm,
                    })
                })
            },
        )
}

/// A random valid Drill project: every cycle, with and without a dwell.
pub fn drill_project() -> impl Strategy<Value = Project> {
    let cycle = prop_oneof![
        Just(DrillCycle::Simple),
        (
            prop::option::of(0.3..5.0_f64),
            prop_oneof![
                Just(PeckRetract::Full),
                (0.1..2.0_f64).prop_map(|gap_mm| PeckRetract::ChipBreak { gap_mm }),
            ],
        )
            .prop_map(|(peck_depth_mm, retract)| DrillCycle::Peck {
                peck_depth_mm,
                retract
            }),
    ];
    (
        setup(any_cutter(), false),
        cycle,
        prop_oneof![Just(0.0), 0.1..2.0_f64],
        depth(),
        prop_oneof![Just(0.0), 0.0..5.0_f64],
        1.0..15.0_f64,
    )
        .prop_map(|(setup, cycle, dwell_s, depth, retract_mm, safe_z_mm)| {
            project(setup, |s, targets| {
                Operation::Drill(DrillOp {
                    id: OperationId::new("o1"),
                    name: "Drill".into(),
                    enabled: true,
                    targets,
                    tool: s.tool.id.clone(),
                    cycle,
                    dwell_s,
                    depth_mm: Some(depth.depth_mm(s.thickness_mm)),
                    retract_mm,
                    through_cut_allowance_mm: depth.allowance_mm,
                    safe_z_mm,
                })
            })
        })
}

/// A random valid Engrave project, of closed and open paths.
pub fn engrave_project() -> impl Strategy<Value = Project> {
    (
        setup(any_cutter(), true),
        depth(),
        step_down(),
        any::<bool>(),
        order(),
        1.0..15.0_f64,
    )
        .prop_map(
            |(setup, depth, step_down_mm, alternate_direction, order, safe_z_mm)| {
                project(setup, |s, targets| {
                    Operation::Engrave(EngraveOp {
                        id: OperationId::new("o1"),
                        name: "Engrave".into(),
                        enabled: true,
                        targets,
                        tool: s.tool.id.clone(),
                        depth_mm: Some(depth.depth_mm(s.thickness_mm)),
                        step_down_mm,
                        alternate_direction,
                        machining_order: order,
                        through_cut_allowance_mm: depth.allowance_mm,
                        safe_z_mm,
                    })
                })
            },
        )
}
