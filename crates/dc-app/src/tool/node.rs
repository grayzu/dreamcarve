// Copyright 2026 Franja (Frank) Povazanj (PureCutCNC)
// Copyright 2026 The DreamCarve Contributors
// SPDX-License-Identifier: Apache-2.0
//
// Ported to Rust and modified from PureCutCNC:
//   src/store/slices/featureGeometrySlice.ts (moveFeatureControl)
//   src/components/canvas/SketchCanvas.tsx (hitEditableControl)
//   src/components/canvas/usePointerGestures.ts (tryJoinDraggedOpenEndpoint)
//   src/store/editInPlace.test.ts

//! The Edit nodes tool (`docs/specs/phase-5/design.md`, "The tool state
//! machines"; functional capability 92): the one selected path shows its
//! nodes, the handles of its curves and the middles of its arcs, and the
//! pointer moves them.
//!
//! What is edited is the app's selection, when it is one shape whose
//! definition is a path: a plain outline, or a path with curves
//! (`Shape::Path`). A shape that keeps its sizes is converted first, at
//! the user's word. Every edit is made on the definition, through the
//! instance's inverse transform, so the node lands under the pointer
//! however the shape is placed, and every linked copy follows
//! (`editInPlace.test.ts`). A drag is previewed and is one undo step on
//! release; a click, a key and a typed coordinate are one each.
//!
//! Changed from upstream: several nodes are selected (a click, `Shift`,
//! a box) and move together, which is Graphite's; upstream moves one.
//! `Alt` moves a curve's handle alone and makes its node a corner.
//! Upstream's edit session, with its Apply and Cancel, is not ported:
//! each edit is in the undo history as it is made.

use std::collections::BTreeSet;

use dc_geometry::{HandleSide, Path, Point, Polyline, Rect, Span, SpanKind, Transform, Vec2};
use dc_project::{FeatureInstance, InstanceId};

use super::draw::DRAG_PX;
use super::{Draft, Event, Key, Quantity, Reply, SizeField, SizeRow, ToolBar};
use crate::command::{Edit, NewShape, copy_name};
use crate::edit::{corner_reason, outline_of, paths_of};
use crate::snap::{Pointer, Snapped};
use crate::strings::StrKey;
use crate::viewport::pick::{self, Hit, PICK_TOLERANCE_PX};
use crate::{App, Command, Selection, Tool, units};

/// How near a node, an arc's middle or a span the pointer takes hold of
/// it, logical pixels (`NODE_HIT_RADIUS`).
pub const NODE_HIT_PX: f64 = 9.0;
/// How near a curve's handle, logical pixels (`HANDLE_HIT_RADIUS`).
pub const HANDLE_HIT_PX: f64 = 7.0;
/// How near another open end a dragged end is dropped to join it, logical
/// pixels (`OPEN_ENDPOINT_JOIN_HIT_RADIUS`).
pub const JOIN_PX: f64 = 14.0;
/// Ends this near are one node when two paths join, mm.
const JOIN_TOLERANCE_MM: f64 = 1e-6;

/// A node of the edited shape: which of its paths, and which node of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeRef {
    /// The path, an index into the definition's outlines.
    pub path: usize,
    /// The node of that path.
    pub node: usize,
}

/// What the pointer can take hold of on the edited shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    /// A node.
    Node(NodeRef),
    /// A handle of a curve at a node: the one of the span that leaves it,
    /// or of the span that arrives.
    Handle {
        /// The node.
        node: NodeRef,
        /// Which of its two handles.
        side: HandleSide,
    },
    /// The middle of an arc, by which it is bowed.
    ArcMiddle {
        /// The path.
        path: usize,
        /// The arc: the span that leaves this node.
        span: usize,
    },
    /// A span, at `t` along it.
    Span {
        /// The path.
        path: usize,
        /// The span: the one that leaves this node.
        span: usize,
        /// How far along it, 0 to 1.
        t: f64,
    },
}

/// Something the Edit nodes tool is asked to do that is no pointer event:
/// a key, a context menu's row, a button of the tool bar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeAction {
    /// Select every node (`Ctrl`/`⌘ A`).
    SelectAll,
    /// Break the path at the one selected node (`B`).
    Break,
    /// Move the selected nodes by this much, mm in project space (the
    /// arrow keys).
    Nudge {
        /// Along X.
        dx_mm: f64,
        /// Along Y.
        dy_mm: f64,
    },
    /// Make the selected shape, which keeps its sizes, a path, and edit
    /// it.
    Convert,
    /// Round the selected corners by the bar's corner size (capability
    /// 93).
    Fillet,
    /// Cut the selected corners back by the bar's corner size.
    Chamfer,
    /// Make the selected span a line, an arc or a curve.
    SpanKind(SpanKind),
}

/// A button of the tool bar.
#[derive(Debug, Clone, PartialEq)]
pub struct BarAction {
    /// What it reads.
    pub label: String,
    /// What it does, for its tooltip.
    pub help: String,
    /// What it asks of the tool.
    pub action: NodeAction,
    /// Whether it can be asked now.
    pub enabled: bool,
}

/// A node as the overlay draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeMark {
    /// Where, mm in project space.
    pub at_mm: Point,
    /// Whether it is selected.
    pub selected: bool,
    /// Whether it is a smooth node of a curve: drawn round, a corner
    /// square.
    pub smooth: bool,
}

/// A handle as the overlay draws it: a curve's, on a stem from its node,
/// or the middle of an arc.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HandleMark {
    /// Where, mm in project space.
    pub at_mm: Point,
    /// The node its stem starts at; an arc's middle has no stem.
    pub from_mm: Option<Point>,
}

/// What the pointer has hold of.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Grab {
    /// A control, pressed at this project point: a drag once it has
    /// moved.
    Control {
        control: Control,
        pressed_mm: Point,
        moved: bool,
    },
    /// Nothing: a box round nodes once it has moved. `add` keeps what was
    /// selected (`Shift`).
    Marquee { from_mm: Point, add: bool },
}

/// What the Edit nodes tool holds.
#[derive(Debug, Clone, PartialEq, Default)]
pub(super) struct NodeDraft {
    /// The selected nodes.
    selected: BTreeSet<NodeRef>,
    /// The selected span, as (path, the node it leaves).
    span: Option<(usize, usize)>,
    grab: Option<Grab>,
}

/// The shape being edited, as the tool reads it.
#[derive(Debug, Clone, PartialEq)]
struct Edited {
    instance: FeatureInstance,
    /// Definition space to project space.
    to_project: Transform,
    /// Project space to definition space.
    to_definition: Transform,
    /// The definition's outlines as paths.
    paths: Vec<Path>,
    /// Whether another instance places the same definition.
    linked: bool,
}

/// What the selection gives the tool.
#[derive(Debug, Clone, PartialEq)]
enum Subject {
    /// Nothing to edit: no one shape is selected.
    Nothing,
    /// A shape that keeps its sizes, by name: to be converted first.
    Sized(String),
    /// A path.
    Path(Box<Edited>),
}

impl Edited {
    fn node(&self, at: NodeRef) -> Option<Point> {
        Some(self.paths.get(at.path)?.nodes().get(at.node)?.point())
    }

    /// A definition-space point in project space.
    fn placed(&self, p: Point) -> Point {
        self.to_project.apply(p)
    }

    /// Every node, with where it is placed.
    fn nodes(&self) -> impl Iterator<Item = (NodeRef, Point)> + '_ {
        self.paths.iter().enumerate().flat_map(move |(path, p)| {
            p.nodes()
                .iter()
                .enumerate()
                .map(move |(node, n)| (NodeRef { path, node }, self.placed(n.point())))
        })
    }

    /// Every curve handle and arc middle, with the control it is and
    /// where it is placed (a handle with its node's place too).
    fn handles(&self) -> Vec<(Control, Point, Option<Point>)> {
        let mut out = Vec::new();
        for (path, p) in self.paths.iter().enumerate() {
            let count = p.nodes().len();
            for span in 0..p.span_count() {
                let Some(node) = p.nodes().get(span) else {
                    continue;
                };
                let end = (span + 1) % count;
                match node.span {
                    Span::Line => {}
                    Span::Arc { bulge } => {
                        if let Some((a, b)) = p.span_ends(span) {
                            out.push((
                                Control::ArcMiddle { path, span },
                                self.placed(dc_geometry::arc_middle(a, b, bulge)),
                                None,
                            ));
                        }
                    }
                    Span::Cubic {
                        handle_out,
                        handle_in,
                    } => {
                        out.push((
                            Control::Handle {
                                node: NodeRef { path, node: span },
                                side: HandleSide::Out,
                            },
                            self.placed(handle_out),
                            Some(self.placed(node.point())),
                        ));
                        if let Some(end_node) = p.nodes().get(end) {
                            out.push((
                                Control::Handle {
                                    node: NodeRef { path, node: end },
                                    side: HandleSide::In,
                                },
                                self.placed(handle_in),
                                Some(self.placed(end_node.point())),
                            ));
                        }
                    }
                }
            }
        }
        out
    }

    /// What is under project point `at`, `px_per_mm` to the pixel: the
    /// nearest node first, then a curve's handle or an arc's middle, then
    /// a span (`hitEditableControl`).
    fn hit(&self, at: Point, px_per_mm: f64) -> Option<Control> {
        let px = |p: Point| p.distance_to(at) * px_per_mm;
        let nearest_node = self
            .nodes()
            .map(|(node, placed)| (node, px(placed)))
            .filter(|(_, d)| *d <= NODE_HIT_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((node, _)) = nearest_node {
            return Some(Control::Node(node));
        }
        let nearest_handle = self
            .handles()
            .into_iter()
            .map(|(control, placed, _)| (control, px(placed)))
            .filter(|(_, d)| *d <= HANDLE_HIT_PX)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((control, _)) = nearest_handle {
            return Some(control);
        }
        let local = self.to_definition.apply(at);
        self.paths
            .iter()
            .enumerate()
            .filter_map(|(path, p)| {
                let hit = p.nearest(local)?;
                Some((path, hit, px(self.placed(hit.point))))
            })
            .filter(|(_, _, d)| *d <= NODE_HIT_PX)
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(path, hit, _)| Control::Span {
                path,
                span: hit.span,
                t: hit.t,
            })
    }

    /// The open ends of path `path`: its first and last nodes.
    fn ends(&self, path: usize) -> Option<(NodeRef, NodeRef)> {
        let p = self.paths.get(path)?;
        (!p.is_closed() && p.nodes().len() >= 2).then(|| {
            (
                NodeRef { path, node: 0 },
                NodeRef {
                    path,
                    node: p.nodes().len() - 1,
                },
            )
        })
    }
}

impl App {
    /// What the selection gives the Edit nodes tool.
    fn node_subject(&self) -> Subject {
        let [id] = self.selection().instances() else {
            return Subject::Nothing;
        };
        let project = self.project();
        let Some(instance) = project.instances.iter().find(|i| &i.id == id) else {
            return Subject::Nothing;
        };
        let Some(definition) = project.definitions.get(&instance.definition) else {
            return Subject::Nothing;
        };
        let Some(paths) = paths_of(definition) else {
            return Subject::Sized(instance.name.clone());
        };
        let Some(to_definition) = instance.transform.inverse() else {
            return Subject::Nothing;
        };
        Subject::Path(Box::new(Edited {
            to_project: instance.transform,
            to_definition,
            paths,
            linked: project
                .instances
                .iter()
                .filter(|i| i.definition == instance.definition)
                .count()
                > 1,
            instance: instance.clone(),
        }))
    }

    /// The shape whose nodes are being edited.
    fn edited(&self) -> Option<Edited> {
        match self.node_subject() {
            Subject::Path(edited) => Some(*edited),
            _ => None,
        }
    }

    /// The instance whose nodes the Edit nodes tool shows, while it is
    /// armed on a path.
    #[must_use]
    pub fn node_editing(&self) -> Option<InstanceId> {
        (self.tool.tool == Tool::Nodes)
            .then(|| self.edited().map(|edited| edited.instance.id))
            .flatten()
    }

    /// The nodes selected in the Edit nodes tool.
    #[must_use]
    pub fn selected_nodes(&self) -> Vec<NodeRef> {
        self.tool.node.selected.iter().copied().collect()
    }

    /// What is under project point `at` on the shape being edited.
    #[must_use]
    pub fn node_control_at(&self, at: Point) -> Option<Control> {
        self.edited()?.hit(at, self.view().camera.px_per_mm)
    }

    /// Hands `event` to the Edit nodes tool.
    pub(super) fn node_event(&mut self, event: Event) -> Reply {
        let reply = match event {
            Event::Move(pointer) => {
                self.tool.pointer = Some(pointer);
                self.node_moved(&pointer);
                Reply::default()
            }
            Event::Leave => {
                self.tool.pointer = None;
                Reply::default()
            }
            Event::Press(pointer) => {
                self.tool.pointer = Some(pointer);
                self.node_press(&pointer)
            }
            Event::Release(pointer) => {
                self.tool.pointer = Some(pointer);
                self.node_release(&pointer)
            }
            Event::DoubleClick(pointer) => self.node_double_click(&pointer),
            Event::Key(Key::Enter | Key::Escape) | Event::Done => {
                // The shape stays selected.
                self.arm(Tool::Select);
                return Reply::default();
            }
            Event::Key(Key::Backspace | Key::Delete) => self.delete_selected(),
            Event::Field { field, text } => self.type_node_size(field, &text),
            Event::Node(action) => self.node_action(action),
            Event::Option(_) | Event::Text(_) => Reply::default(),
        };
        self.refresh_node_draft();
        reply
    }

    /// The pointer moved: a press that has gone far enough is a drag.
    fn node_moved(&mut self, pointer: &Pointer) {
        let px_per_mm = self.view().camera.px_per_mm;
        if let Some(Grab::Control {
            pressed_mm, moved, ..
        }) = &mut self.tool.node.grab
            && pressed_mm.distance_to(pointer.at_mm) * px_per_mm > DRAG_PX
        {
            *moved = true;
        }
    }

    /// The button went down: on a control, which is selected and may be
    /// dragged; on another shape, which is edited instead; on nothing,
    /// where a box starts.
    fn node_press(&mut self, pointer: &Pointer) -> Reply {
        let px_per_mm = self.view().camera.px_per_mm;
        let hit = self
            .edited()
            .and_then(|edited| edited.hit(pointer.at_mm, px_per_mm));
        let Some(control) = hit else {
            // Another shape under the pointer is the one to edit.
            let camera = self.view().camera;
            let at = camera.to_screen(pointer.at_mm);
            if let Hit::Instance(id) = pick::pick(self.scene(), &camera, at, PICK_TOLERANCE_PX, 0)
                && self.selection().instances() != [id.clone()]
            {
                self.select(Selection::Instances(vec![id]));
                self.tool.node = NodeDraft::default();
                return Reply::default();
            }
            if !pointer.shift {
                self.tool.node.selected.clear();
            }
            self.tool.node.span = None;
            self.tool.node.grab = Some(Grab::Marquee {
                from_mm: pointer.at_mm,
                add: pointer.shift,
            });
            return Reply::default();
        };
        let draft = &mut self.tool.node;
        match control {
            Control::Node(node) => {
                draft.span = None;
                if pointer.shift {
                    // Shift takes a node out of the selection, or adds it.
                    if !draft.selected.remove(&node) {
                        draft.selected.insert(node);
                    }
                } else if !draft.selected.contains(&node) {
                    draft.selected.clear();
                    draft.selected.insert(node);
                }
            }
            Control::Span { path, span, t } => {
                if pointer.command {
                    // Ctrl or ⌘ on a span adds a node there.
                    return self.add_node(path, span, t);
                }
                draft.selected.clear();
                draft.span = Some((path, span));
            }
            Control::Handle { .. } | Control::ArcMiddle { .. } => {}
        }
        draft.grab = Some(Grab::Control {
            control,
            pressed_mm: pointer.at_mm,
            moved: false,
        });
        Reply::default()
    }

    /// The paths as the drag in hand has them now, with where the dragged
    /// point snapped, and what the drag is for its undo step.
    fn dragged(
        &self,
        edited: &Edited,
        control: Control,
        pressed_mm: Point,
        pointer: &Pointer,
    ) -> Option<(Vec<Path>, Snapped, Edit)> {
        let snapped = self.snap_at(pointer, None, std::slice::from_ref(&edited.instance.id));
        let to = edited.to_definition.apply(snapped.at_mm);
        let mut paths = edited.paths.clone();
        let what = match control {
            Control::Node(grabbed) => {
                // The grabbed node goes under the pointer, and the others
                // selected go as far the same way.
                let from = edited.node(grabbed)?;
                let delta = to - from;
                let moving: Vec<(NodeRef, Point)> = self
                    .tool
                    .node
                    .selected
                    .iter()
                    .filter_map(|node| Some((*node, edited.node(*node)? + delta)))
                    .collect();
                for (node, target) in &moving {
                    let path = paths.get_mut(node.path)?;
                    *path = path.with_node_at(node.node, *target)?;
                }
                Edit::MoveNodes {
                    count: moving.len(),
                }
            }
            Control::Handle { node, side } => {
                let path = paths.get_mut(node.path)?;
                *path = path.with_handle_at(node.node, side, to, !pointer.alt)?;
                Edit::Reshape
            }
            Control::ArcMiddle { path, span } => {
                let edited_path = paths.get_mut(path)?;
                *edited_path = edited_path.with_arc_through(span, to)?;
                Edit::Reshape
            }
            Control::Span { path, span, .. } => {
                // A straight span moves with its two nodes; an arc and a
                // curve are reshaped by their handles.
                let edited_path = paths.get_mut(path)?;
                let node = *edited_path.nodes().get(span)?;
                if node.span != Span::Line {
                    return None;
                }
                let delta = edited.to_definition.apply(pointer.at_mm)
                    - edited.to_definition.apply(pressed_mm);
                let end = (span + 1) % edited_path.nodes().len();
                let (a, b) = edited_path.span_ends(span)?;
                *edited_path = edited_path.with_node_at(span, a + delta)?;
                *edited_path = edited_path.with_node_at(end, b + delta)?;
                Edit::MoveNodes { count: 2 }
            }
        };
        Some((paths, snapped, what))
    }

    /// The button came up: a drag is committed, one undo step; a box
    /// selects the nodes in it.
    fn node_release(&mut self, pointer: &Pointer) -> Reply {
        let Some(grab) = self.tool.node.grab.take() else {
            return Reply::default();
        };
        let Some(edited) = self.edited() else {
            return Reply::default();
        };
        match grab {
            Grab::Marquee { from_mm, add } => {
                let px_per_mm = self.view().camera.px_per_mm;
                if from_mm.distance_to(pointer.at_mm) * px_per_mm > DRAG_PX {
                    let rect = Rect::from_point(from_mm).including(pointer.at_mm);
                    let inside = |p: Point| {
                        (rect.min.x..=rect.max.x).contains(&p.x)
                            && (rect.min.y..=rect.max.y).contains(&p.y)
                    };
                    if !add {
                        self.tool.node.selected.clear();
                    }
                    let boxed: Vec<NodeRef> = edited
                        .nodes()
                        .filter(|(_, placed)| inside(*placed))
                        .map(|(node, _)| node)
                        .collect();
                    self.tool.node.selected.extend(boxed);
                }
                Reply::default()
            }
            Grab::Control { moved: false, .. } => Reply::default(),
            Grab::Control {
                control,
                pressed_mm,
                moved: true,
            } => {
                let Some((paths, _, what)) = self.dragged(&edited, control, pressed_mm, pointer)
                else {
                    return Reply::default();
                };
                // An open end dropped on another open end joins them.
                if let Control::Node(node) = control
                    && self.tool.node.selected.len() == 1
                    && let Some(command) = self.join_command(&edited, &paths, node)
                {
                    return self.run_node_command(command);
                }
                let command = self.outline_command(&edited, paths, what);
                self.run_node_command(command)
            }
        }
    }

    /// The `SetOutline` that gives the edited shape `paths`: its outline
    /// flattened from them at the project's tolerance, and the paths kept
    /// where one has a curve.
    fn outline_command(&self, edited: &Edited, paths: Vec<Path>, what: Edit) -> Command {
        let (shape, profiles) = outline_of(paths, self.project().flatten_tolerance_mm);
        Command::SetOutline {
            instance: edited.instance.id.clone(),
            shape,
            profiles,
            what,
        }
    }

    /// Runs a node edit: its undo label, or why it was refused.
    fn run_node_command(&mut self, command: Command) -> Reply {
        match self.run(command) {
            Ok(label) => Reply {
                ran: Some(label),
                refused: None,
            },
            Err(error) => Reply {
                ran: None,
                refused: Some(error.to_string()),
            },
        }
    }

    /// The command that joins the dragged open end `node`, now where
    /// `paths` have it, to the open end it was dropped on: its own path's
    /// other end, which closes the path, or another shape's, which makes
    /// the two one shape. `None` when it was dropped on neither.
    fn join_command(&self, edited: &Edited, paths: &[Path], node: NodeRef) -> Option<Command> {
        let (first, last) = edited.ends(node.path)?;
        if node != first && node != last {
            return None;
        }
        let px_per_mm = self.view().camera.px_per_mm;
        let path = paths.get(node.path)?;
        let dropped = edited.placed(path.nodes().get(node.node)?.point());
        let near = |p: Point| p.distance_to(dropped) * px_per_mm <= JOIN_PX;
        // Its own other end: the path closes, the two ends one node.
        let other = if node == first { last } else { first };
        let other_at = path.nodes().get(other.node)?.point();
        if path.nodes().len() >= 3 && near(edited.placed(other_at)) {
            let mut closing = path.with_node_at(node.node, other_at)?;
            closing = closing.closed_up(JOIN_TOLERANCE_MM)?;
            let mut joined = paths.to_vec();
            *joined.get_mut(node.path)? = closing;
            return Some(self.outline_command(edited, joined, Edit::CloseEnds));
        }
        // Another shape's open end: both must be one open path of their
        // own, since the two become one shape.
        if edited.linked || edited.paths.len() != 1 {
            return None;
        }
        let project = self.project();
        for instance in &project.instances {
            if instance.id == edited.instance.id || !instance.visible || instance.locked {
                continue;
            }
            let Some(definition) = project.definitions.get(&instance.definition) else {
                continue;
            };
            let shared = project
                .instances
                .iter()
                .filter(|i| i.definition == instance.definition)
                .count()
                > 1;
            let Some(their_paths) = paths_of(definition) else {
                continue;
            };
            let [theirs] = their_paths.as_slice() else {
                continue;
            };
            if shared || theirs.is_closed() || theirs.span_count() == 0 {
                continue;
            }
            // Their path in this shape's own space.
            let Some(theirs) = theirs.transformed(instance.transform.then(edited.to_definition))
            else {
                continue;
            };
            let (start, end) = (theirs.nodes().first()?, theirs.nodes().last()?);
            let at_start = near(edited.placed(start.point()));
            let at_end = near(edited.placed(end.point()));
            if !at_start && !at_end {
                continue;
            }
            // This path runs to the dragged end, and theirs on from the
            // end it was dropped on, which the dragged end is moved to.
            let target = if at_start { start.point() } else { end.point() };
            let mine = path.with_node_at(node.node, target)?;
            let mine = if node == last { mine } else { mine.reversed() };
            let theirs = if at_start { theirs } else { theirs.reversed() };
            let joined = mine.joined(&theirs, JOIN_TOLERANCE_MM)?;
            let (shape, profiles) = outline_of(vec![joined], project.flatten_tolerance_mm);
            return Some(Command::ReplaceShapes {
                from: vec![edited.instance.id.clone(), instance.id.clone()],
                keep: false,
                add: vec![NewShape {
                    name: edited.instance.name.clone(),
                    shape,
                    profiles,
                    transform: edited.instance.transform,
                    construction: edited.instance.construction,
                    folder: edited.instance.folder.clone(),
                }],
                inherit: vec![(edited.instance.id.clone(), 0)],
                what: Edit::JoinEnds,
            });
        }
        None
    }

    /// A second click in the same place: on a span it adds a node there;
    /// on empty stock it leaves the tool.
    fn node_double_click(&mut self, pointer: &Pointer) -> Reply {
        self.tool.node.grab = None;
        let px_per_mm = self.view().camera.px_per_mm;
        match self
            .edited()
            .and_then(|edited| edited.hit(pointer.at_mm, px_per_mm))
        {
            Some(Control::Span { path, span, t }) => self.add_node(path, span, t),
            Some(_) => Reply::default(),
            None => {
                let camera = self.view().camera;
                let at = camera.to_screen(pointer.at_mm);
                if pick::pick(self.scene(), &camera, at, PICK_TOLERANCE_PX, 0) == Hit::Nothing {
                    self.arm(Tool::Select);
                }
                Reply::default()
            }
        }
    }

    /// Splits span `span` of path `path` at `t`: a node more, the outline
    /// the same. The new node is selected.
    fn add_node(&mut self, path: usize, span: usize, t: f64) -> Reply {
        let Some(edited) = self.edited() else {
            return Reply::default();
        };
        let Some(split) = edited.paths.get(path).and_then(|p| p.split_span(span, t)) else {
            return Reply::default();
        };
        let mut paths = edited.paths.clone();
        if let Some(slot) = paths.get_mut(path) {
            *slot = split;
        }
        let reply = self.run_node_command(self.outline_command(&edited, paths, Edit::AddNode));
        if reply.ran.is_some() {
            self.tool.node.span = None;
            self.tool.node.selected = BTreeSet::from([NodeRef {
                path,
                node: span + 1,
            }]);
        }
        reply
    }

    /// `Delete`: the selected span goes, or the selected nodes do, their
    /// neighbours joined. A node an outline cannot lose is left.
    fn delete_selected(&mut self) -> Reply {
        let Some(edited) = self.edited() else {
            return Reply::default();
        };
        if let Some((path, span)) = self.tool.node.span {
            let Some((first, second)) = edited.paths.get(path).and_then(|p| p.delete_span(span))
            else {
                return self.refuse(StrKey::NodeRefusedLastSegment);
            };
            let reply = self.split(&edited, path, first, second, Edit::DeleteSegment);
            if reply.ran.is_some() {
                self.tool.node = NodeDraft::default();
            }
            return reply;
        }
        if self.tool.node.selected.is_empty() {
            return Reply::default();
        }
        // From each path's last node back, so the nodes before keep
        // their numbers.
        let mut paths = edited.paths.clone();
        let mut deleted = 0;
        for node in self.tool.node.selected.iter().rev() {
            if let Some(slot) = paths.get_mut(node.path)
                && let Some(without) = slot.delete_node(node.node)
            {
                *slot = without;
                deleted += 1;
            }
        }
        if deleted == 0 {
            return self.refuse(StrKey::NodeRefusedTooFew);
        }
        let reply = self.run_node_command(self.outline_command(
            &edited,
            paths,
            Edit::DeleteNodes { count: deleted },
        ));
        if reply.ran.is_some() {
            self.tool.node.selected.clear();
        }
        reply
    }

    /// A refusal with the reason `key`.
    fn refuse(&self, key: StrKey) -> Reply {
        Reply {
            ran: None,
            refused: Some(self.strings().get(key).to_owned()),
        }
    }

    /// Path `path` of the edited shape replaced by `first` and, when the
    /// edit split it, `second`. A shape that is one path of its own
    /// becomes two shapes; a path of a shape with other outlines, or with
    /// linked copies, which share its definition, stays in it as two
    /// outlines.
    fn split(
        &mut self,
        edited: &Edited,
        path: usize,
        first: Path,
        second: Option<Path>,
        what: Edit,
    ) -> Reply {
        let mut paths = edited.paths.clone();
        let Some(slot) = paths.get_mut(path) else {
            return Reply::default();
        };
        *slot = first.clone();
        let Some(second) = second else {
            return self.run_node_command(self.outline_command(edited, paths, what));
        };
        if edited.linked || edited.paths.len() != 1 {
            paths.insert(path + 1, second);
            return self.run_node_command(self.outline_command(edited, paths, what));
        }
        let project = self.project();
        let tolerance_mm = project.flatten_tolerance_mm;
        let taken: Vec<&str> = project.instances.iter().map(|i| i.name.as_str()).collect();
        let part = |name: String, path: Path| {
            let (shape, profiles) = outline_of(vec![path], tolerance_mm);
            NewShape {
                name,
                shape,
                profiles,
                transform: edited.instance.transform,
                construction: edited.instance.construction,
                folder: edited.instance.folder.clone(),
            }
        };
        let command = Command::ReplaceShapes {
            from: vec![edited.instance.id.clone()],
            keep: false,
            add: vec![
                part(edited.instance.name.clone(), first),
                part(copy_name(&taken, &edited.instance.name), second),
            ],
            inherit: vec![(edited.instance.id.clone(), 0)],
            what,
        };
        self.run_node_command(command)
    }

    /// Something asked of the tool that is no pointer event.
    fn node_action(&mut self, action: NodeAction) -> Reply {
        match action {
            NodeAction::Convert => {
                let ids = self.selection().instances().to_vec();
                self.run_node_command(Command::ConvertToPath(ids))
            }
            NodeAction::SelectAll => {
                if let Some(edited) = self.edited() {
                    self.tool.node.span = None;
                    self.tool.node.selected = edited.nodes().map(|(node, _)| node).collect();
                }
                Reply::default()
            }
            NodeAction::Break => {
                let Some(edited) = self.edited() else {
                    return Reply::default();
                };
                let selected: Vec<NodeRef> = self.tool.node.selected.iter().copied().collect();
                let [node] = selected.as_slice() else {
                    return self.refuse(StrKey::NodeRefusedBreakOne);
                };
                let Some((first, second)) = edited
                    .paths
                    .get(node.path)
                    .and_then(|p| p.break_at(node.node))
                else {
                    return self.refuse(StrKey::NodeRefusedBreakEnd);
                };
                let reply = self.split(&edited, node.path, first, second, Edit::Break);
                if reply.ran.is_some() {
                    self.tool.node = NodeDraft::default();
                }
                reply
            }
            NodeAction::Fillet => self.cut_corners(true),
            NodeAction::Chamfer => self.cut_corners(false),
            NodeAction::SpanKind(kind) => {
                let Some(edited) = self.edited() else {
                    return Reply::default();
                };
                let Some((path, span)) = self.tool.node.span else {
                    return self.refuse(StrKey::NodeRefusedNoSegment);
                };
                let Some(changed) = edited
                    .paths
                    .get(path)
                    .and_then(|p| p.with_span_kind(span, kind))
                else {
                    return Reply::default();
                };
                if edited.paths.get(path) == Some(&changed) {
                    return Reply::default();
                }
                let mut paths = edited.paths.clone();
                if let Some(slot) = paths.get_mut(path) {
                    *slot = changed;
                }
                self.run_node_command(self.outline_command(&edited, paths, Edit::SegmentKind(kind)))
            }
            NodeAction::Nudge { dx_mm, dy_mm } => {
                let Some(edited) = self.edited() else {
                    return Reply::default();
                };
                if self.tool.node.selected.is_empty() {
                    return Reply::default();
                }
                let origin = edited.to_definition.apply(Point::new(0.0, 0.0));
                let delta = edited.to_definition.apply(Point::new(dx_mm, dy_mm)) - origin;
                self.move_selected(&edited, delta)
            }
        }
    }

    /// **Fillet** or **Chamfer**: each selected corner rounded by an arc,
    /// or cut back straight, by the bar's corner size as the shape is
    /// placed. A corner that cannot take it is left as it is, and the
    /// reply says why; one undo step for those that could.
    fn cut_corners(&mut self, fillet: bool) -> Reply {
        let Some(edited) = self.edited() else {
            return Reply::default();
        };
        if self.tool.node.selected.is_empty() {
            return self.refuse(StrKey::NodeRefusedNoCorner);
        }
        let units = self.project().display_units;
        let Some(scale) = crate::shape::even_scale(edited.to_project) else {
            return Reply {
                ran: None,
                refused: Some(
                    crate::CommandError::Stretched {
                        name: edited.instance.name.clone(),
                    }
                    .to_string(),
                ),
            };
        };
        let size_mm = self.prefs().drawing.corner_size_mm(units) / scale;
        let mut paths = edited.paths.clone();
        let mut cut = 0;
        let mut left = None;
        // From each path's last node back: a cut corner is two nodes.
        for node in self.tool.node.selected.iter().rev() {
            let Some(slot) = paths.get_mut(node.path) else {
                continue;
            };
            let made = if fillet {
                slot.fillet(node.node, size_mm)
            } else {
                slot.chamfer(node.node, size_mm)
            };
            match made {
                Ok(path) => {
                    *slot = path;
                    cut += 1;
                }
                Err(error) => left = left.or(Some(error)),
            }
        }
        let why = left.map(|error| corner_reason(self.strings(), units, error, scale));
        if cut == 0 {
            return Reply {
                ran: None,
                refused: why,
            };
        }
        let what = if fillet {
            Edit::Fillet { count: cut }
        } else {
            Edit::Chamfer { count: cut }
        };
        let mut reply = self.run_node_command(self.outline_command(&edited, paths, what));
        if reply.ran.is_some() {
            // The nodes are renumbered: nothing stays selected.
            self.tool.node.selected.clear();
            // Some corners were cut and some were not: say which were
            // left, and why.
            reply.refused = why.map(|why| {
                self.strings()
                    .fmt(StrKey::NodeRefusedSomeCorners, &[("reason", &why)])
            });
        }
        reply
    }

    /// Moves the selected nodes by `delta`, in definition space: one
    /// undo step.
    fn move_selected(&mut self, edited: &Edited, delta: Vec2) -> Reply {
        let moving: Vec<(NodeRef, Point)> = self
            .tool
            .node
            .selected
            .iter()
            .filter_map(|node| Some((*node, edited.node(*node)? + delta)))
            .collect();
        let mut paths = edited.paths.clone();
        for (node, target) in &moving {
            if let Some(slot) = paths.get_mut(node.path)
                && let Some(moved) = slot.with_node_at(node.node, *target)
            {
                *slot = moved;
            }
        }
        self.run_node_command(self.outline_command(
            edited,
            paths,
            Edit::MoveNodes {
                count: moving.len(),
            },
        ))
    }

    /// The one selected node, when there is one.
    fn one_selected(&self) -> Option<NodeRef> {
        let mut selected = self.tool.node.selected.iter();
        match (selected.next(), selected.next()) {
            (Some(node), None) => Some(*node),
            _ => None,
        }
    }

    /// The selected span, when it is an arc.
    fn selected_arc(&self, edited: &Edited) -> Option<(usize, usize)> {
        let (path, span) = self.tool.node.span?;
        let node = edited.paths.get(path)?.nodes().get(span)?;
        matches!(node.span, Span::Arc { .. }).then_some((path, span))
    }

    /// A coordinate typed for the one selected node, from the origin, or
    /// a radius for the selected arc.
    fn type_node_size(&mut self, field: SizeField, text: &str) -> Reply {
        // The corner size is the tool's option, as the Rectangle's is.
        if field == SizeField::CornerSize {
            self.type_size(field, text);
            return Reply::default();
        }
        self.tool.problems.remove(&field);
        let Some(edited) = self.edited() else {
            return Reply::default();
        };
        if text.trim().is_empty() {
            return Reply::default();
        }
        let value = match self.read_size(field, text) {
            Ok(value) => value,
            Err(problem) => {
                self.tool.problems.insert(field, problem);
                return Reply::default();
            }
        };
        let project = self.project();
        let origin = project.origin.xy.point_mm(&project.stock);
        match field {
            SizeField::X | SizeField::Y => {
                let Some(node) = self.one_selected() else {
                    return Reply::default();
                };
                let Some(at) = edited.node(node).map(|p| edited.placed(p)) else {
                    return Reply::default();
                };
                let to = if field == SizeField::X {
                    Point::new(origin.x + value, at.y)
                } else {
                    Point::new(at.x, origin.y + value)
                };
                let delta = edited.to_definition.apply(to) - edited.to_definition.apply(at);
                if delta.length() <= f64::EPSILON {
                    return Reply::default();
                }
                self.move_selected(&edited, delta)
            }
            SizeField::Radius => {
                let Some((path, span)) = self.selected_arc(&edited) else {
                    return Reply::default();
                };
                // The radius is typed as the arc is placed.
                let scale = crate::shape::even_scale(edited.to_project).unwrap_or(1.0);
                let Some(reshaped) = edited
                    .paths
                    .get(path)
                    .and_then(|p| p.with_arc_radius(span, value / scale))
                else {
                    let problem = self.strings().get(StrKey::NodeProblemRadius).to_owned();
                    self.tool.problems.insert(field, problem);
                    return Reply::default();
                };
                let mut paths = edited.paths.clone();
                if let Some(slot) = paths.get_mut(path) {
                    *slot = reshaped;
                }
                self.run_node_command(self.outline_command(&edited, paths, Edit::Reshape))
            }
            _ => Reply::default(),
        }
    }

    /// The tool bar for the Edit nodes tool.
    pub(super) fn node_bar(&self) -> ToolBar {
        let strings = self.strings();
        let units = self.project().display_units;
        let action = |label: StrKey, help: StrKey, action: NodeAction, enabled: bool| BarAction {
            label: strings.get(label).to_owned(),
            help: strings.get(help).to_owned(),
            action,
            enabled,
        };
        let mut bar = ToolBar {
            tool: Tool::Nodes,
            prompt: String::new(),
            fields: Vec::new(),
            choices: Vec::new(),
            actions: Vec::new(),
            note: None,
            can_finish: true,
            text: None,
        };
        let edited = match self.node_subject() {
            Subject::Nothing => {
                strings
                    .get(StrKey::NodePromptPick)
                    .clone_into(&mut bar.prompt);
                return bar;
            }
            Subject::Sized(name) => {
                bar.prompt = strings.fmt(StrKey::NodePromptSized, &[("name", &name)]);
                bar.actions.push(action(
                    StrKey::AppMenuConvertToPath,
                    StrKey::NodeConvertHelp,
                    NodeAction::Convert,
                    true,
                ));
                return bar;
            }
            Subject::Path(edited) => edited,
        };
        strings
            .get(StrKey::NodePromptEdit)
            .clone_into(&mut bar.prompt);
        let project = self.project();
        let origin = project.origin.xy.point_mm(&project.stock);
        let row = |field: SizeField, value: f64| SizeRow {
            field,
            label: strings.get(field.label()).to_owned(),
            help: strings.get(field.help()).to_owned(),
            text: match field.quantity() {
                Quantity::Length => units::field_text(value, units),
                Quantity::Angle | Quantity::Count | Quantity::Percent => String::new(),
            },
            quantity: field.quantity(),
            pinned: false,
            problem: self.tool.problems.get(&field).cloned(),
            option: false,
        };
        let one = self.one_selected();
        if let Some(at) = one
            .and_then(|node| edited.node(node))
            .map(|p| edited.placed(p))
        {
            bar.fields.push(row(SizeField::X, at.x - origin.x));
            bar.fields.push(row(SizeField::Y, at.y - origin.y));
        }
        if let Some((path, span)) = self.selected_arc(&edited)
            && let Some(radius) = edited.paths.get(path).and_then(|p| p.arc_radius_mm(span))
        {
            let scale = crate::shape::even_scale(edited.to_project).unwrap_or(1.0);
            bar.fields.push(row(SizeField::Radius, radius * scale));
        }
        // The corner size Fillet and Chamfer cut by: the tool's option.
        let mut corner = row(
            SizeField::CornerSize,
            self.prefs().drawing.corner_size_mm(units),
        );
        corner.option = true;
        bar.fields.push(corner);
        let any = !self.tool.node.selected.is_empty();
        bar.actions.push(action(
            StrKey::NodeFillet,
            StrKey::NodeFilletHelp,
            NodeAction::Fillet,
            any,
        ));
        bar.actions.push(action(
            StrKey::NodeChamfer,
            StrKey::NodeChamferHelp,
            NodeAction::Chamfer,
            any,
        ));
        // What the selected span can be made.
        if let Some(kind) = self.tool.node.span.and_then(|(path, span)| {
            let node = edited.paths.get(path)?.nodes().get(span)?;
            Some(match node.span {
                Span::Line => SpanKind::Line,
                Span::Arc { .. } => SpanKind::Arc,
                Span::Cubic { .. } => SpanKind::Cubic,
            })
        }) {
            for (label, help, to) in [
                (
                    StrKey::NodeMakeStraight,
                    StrKey::NodeMakeStraightHelp,
                    SpanKind::Line,
                ),
                (StrKey::NodeMakeArc, StrKey::NodeMakeArcHelp, SpanKind::Arc),
                (
                    StrKey::NodeMakeCurve,
                    StrKey::NodeMakeCurveHelp,
                    SpanKind::Cubic,
                ),
            ] {
                bar.actions
                    .push(action(label, help, NodeAction::SpanKind(to), to != kind));
            }
        }
        let can_break = one.is_some_and(|node| {
            edited
                .paths
                .get(node.path)
                .is_some_and(|p| p.break_at(node.node).is_some())
        });
        bar.actions.push(action(
            StrKey::NodeBreak,
            StrKey::NodeBreakHelp,
            NodeAction::Break,
            can_break,
        ));
        bar
    }

    /// What the tool wants drawn: the nodes, the handles, the box being
    /// dragged, and while a control is dragged the outline as the release
    /// will leave it.
    fn refresh_node_draft(&mut self) {
        let mut draft = Draft::default();
        let Some(mut edited) = self.edited() else {
            self.tool.node = NodeDraft::default();
            self.tool.draft = draft;
            return;
        };
        // Nodes that are gone (an undo, another shape) are not selected.
        let exists = |node: &NodeRef| edited.node(*node).is_some();
        let stale: Vec<NodeRef> = self
            .tool
            .node
            .selected
            .iter()
            .filter(|node| !exists(node))
            .copied()
            .collect();
        for node in stale {
            self.tool.node.selected.remove(&node);
        }
        if self.tool.node.span.is_some_and(|(path, span)| {
            edited
                .paths
                .get(path)
                .is_none_or(|p| span >= p.span_count())
        }) {
            self.tool.node.span = None;
        }
        let tolerance_mm = self.project().flatten_tolerance_mm;
        match (self.tool.node.grab, self.tool.pointer) {
            (
                Some(Grab::Control {
                    control,
                    pressed_mm,
                    moved: true,
                }),
                Some(pointer),
            ) => {
                if let Some((paths, snapped, _)) =
                    self.dragged(&edited, control, pressed_mm, &pointer)
                {
                    draft.snap = Some(snapped);
                    draft.outlines = paths
                        .iter()
                        .filter_map(|path| {
                            edited
                                .to_project
                                .apply_polyline(&path.to_polyline(tolerance_mm))
                                .ok()
                        })
                        .collect();
                    edited.paths = paths;
                }
            }
            (Some(Grab::Marquee { from_mm, .. }), Some(pointer)) => {
                draft.marquee = Some((from_mm, pointer.at_mm));
            }
            _ => {}
        }
        draft.nodes = edited
            .nodes()
            .map(|(node, at_mm)| NodeMark {
                at_mm,
                selected: self.tool.node.selected.contains(&node),
                smooth: edited
                    .paths
                    .get(node.path)
                    .and_then(|p| p.nodes().get(node.node))
                    .is_some_and(|n| n.smooth),
            })
            .collect();
        draft.handles = edited
            .handles()
            .into_iter()
            .map(|(_, at_mm, from_mm)| HandleMark { at_mm, from_mm })
            .collect();
        if let Some((path, span)) = self.tool.node.span
            && let Some(highlight) = span_outline(&edited, path, span, tolerance_mm)
        {
            draft.highlights.push(highlight);
        }
        self.tool.draft = draft;
    }

    /// Shows the edited shape's nodes as the tool is armed.
    pub(super) fn begin_nodes(&mut self) {
        self.refresh_node_draft();
    }
}

/// Span `span` of path `path`, placed, as an outline of its own: the
/// selected span's highlight.
fn span_outline(edited: &Edited, path: usize, span: usize, tolerance_mm: f64) -> Option<Polyline> {
    let p = edited.paths.get(path)?;
    let count = p.nodes().len();
    let start = *p.nodes().get(span)?;
    let end = *p.nodes().get((span + 1) % count)?;
    let one = Path::new(vec![start, end], false);
    edited
        .to_project
        .apply_polyline(&one.to_polyline(tolerance_mm))
        .ok()
}
