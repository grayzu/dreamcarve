# Phase 5: GUI, CAD — Design Spec

**Status:** draft, written 2026-11-22 with `functional.md`; a slice
amends it in the commit that changes what it describes
(`docs/phase-5-worklog.md`, "Process")
**Functional spec:** `functional.md`
**Decision record:** ADR-0006
**Visual design:** phase 4's `redesign.md` and `experience.md`; what this
phase adds to them is under "Visual design" below

## Overview

Phase 5 lets the app write what it could only read: a definition's
outline. Three things carry it.

- **The file says how a shape was made** (format 29, ADR-0006).
  `FeatureDefinition` gains `shape`; `profiles` stays what is cut. The
  outline generators are pure functions in `dc-geometry`
  (`shapes`, `path`), called by `dc-project` (to check a file) and by
  `dc-app` (to make and edit shapes). Text's generator is a new crate,
  `dc-text`, because it needs a font parser and font bytes, which
  `dc-project` must not carry.
- **A tool model in `dc-app`.** Phase 4 has no tools: the Tab tool is a
  flag in egui's memory. `dc_app::tool` holds the tool in use and its
  draft, takes pointer and key events in project millimetres, and
  answers with what to draw and, when a shape is done, the `Command` it
  ran. The window only feeds events and paints the answer, so every
  acceptance test but the window's own (130) runs headless, as phase
  4's do.
- **Commands that write geometry.** Phase 4's 37 commands never touch a
  definition's outline. This phase adds a small set that do, each
  carrying finished values (an outline, a transform), never a gesture.

Nothing in `dc-cam` or `dc-gcode` changes. `dc-job` changes in one
place: the artwork checks and the stock fit skip construction geometry.

Data flow for one drawn rectangle: the window sends `PointerDown` and
`PointerUp` at two project points → `dc_app::tool` resolves each through
`snap::resolve` against the `Scene` → the Rectangle tool's draft holds
the first corner and previews the second → on the second it builds
`Shape::Rectangle`, asks `dc_geometry::shapes` for the outline, and runs
`Command::AddShapes` → `App::commit` pushes the snapshot, the digest
finds nothing targets the new shape, the `Scene` is rebuilt and the
Viewport draws it.

## Reference sources

Behaviour is ported from PureCutCNC unless a row says Graphite. A Rust
module that transcribes a PureCutCNC file takes the ported-file header
(`AGENTS.md`); one that transcribes a Graphite file takes the same
header with `Copyright (c) Graphite contributors` and Graphite's path
and commit (`615b4050`), and `NOTICE` gains Graphite then. The surveys
are `docs/research/purecutcnc-drawing-digest.md` and
`docs/research/graphite-tools-digest.md`.

| DreamCarve module | Source | What is taken, what changes |
| --- | --- | --- |
| `dc-geometry::shapes` (rectangle, ellipse, polygon, slot) | `src/types/project.ts` (`rectProfile`, `ellipseProfile`, `slotProfile`, `ngonProfile`), `src/store/helpers/cannedRectProfiles.ts`, `src/cannedShapes/generators/` | The constructions and their tests (`slot.test.ts`, `ngon.test.ts`, `cannedRectProfiles.test.ts`, `shapes.test.ts`). Re-derived for Y-up: every outline is anticlockwise, where upstream's winding differs by shape (digest §10.2). Arcs are bulges, not centre and flag. The ellipse is flattened within a tolerance; upstream's is four Béziers. The polygon gains a radius to the flats. |
| `dc-geometry::shapes::gear` | `src/sketch/gearProfile.ts`, `gearProfile.test.ts`, `src/cannedShapes/generators/gear.ts` | Ported whole with its validation and tests. |
| `dc-geometry::path` (nodes, split, delete, break, fillet, chamfer, spline, arc through three points) | `src/store/helpers/profileEdit.ts`, `profileEdit.test.ts`, `splineProfile` | The edit operations and the Catmull-Rom construction, on nodes with a per-segment kind instead of upstream's segment list. Segment-kind conversion and the corner (non-colinear) node are Graphite's (`path_tool.rs`, `shape_editor.rs`), re-implemented from the digest, not transcribed. |
| `dc-geometry::intersect` | `src/store/helpers/segmentIntersection.ts`, `segmentIntersection.test.ts` | Line–line, line–arc, arc–arc with parameters, ported with its 42 tests, on `Segment` (bulge) instead of resolved centre and sweep. |
| `dc-geometry::trim` | `featureGeometrySlice.ts` (`trimFeatureSegment`) | The idea of trimming at crossings. The interior case and closed outlines, which upstream leaves undone, are designed here ("Algorithms → Trim"). |
| `dc-geometry::join` | `crates/dc-import/src/dxf/stitch.rs` (DreamCarve's own) and `derivedFeatures.ts` (`joinOpenProfiles`) | The DXF importer's end-matching, moved down so both call it. |
| `dc-app::snap` | `src/sketch/snapping.ts`, `src/components/canvas/snappingHelpers.ts`, `snappingHelpers.test.ts` | Kinds, ranks, the pixel radius, the indicator. Dropped: the snap gate (digest §5.4), dimension anchors. Added: targets are the real outlines; `Ctrl` suspends; `Shift` angle steps (Graphite). |
| `dc-app::tool` (drawing tools, typed sizes) | `src/store/slices/pendingAddSlice.ts`, `src/components/canvas/useClickPlacement.ts`, `manualEntry.ts`, `useCreationWorkflow.ts`, `triggerDimensionEdit.ts`, `stickyDrawing.test.ts` | The click sequences, the sticky tools, the fields per step. Press-drag-release as a second way to give two points, and `Shift`/`Alt` on the box tools, are Graphite's (`shape_tool.rs`, `resize.rs`). No upstream test drives a click sequence (digest §11); these tests are written from the behaviour. |
| `dc-app::tool::node` | `featureGeometrySlice.ts` (`moveFeatureControl`), `SketchCanvas.tsx` (`hitEditableControl`), `editInPlace.test.ts` | Hit radii and order, the arc rebuilt through its old middle, edits written through the instance's inverse transform. Several nodes selected and moved together is Graphite's. |
| `dc-app::array` | `src/sketch/featureDistribution.ts`, `featureDistribution.test.ts` | The grid and radial planners. Rows advance in +Y, which is up here. Path arrays and scale ramps are not ported. |
| `dc-app::arrange` (align, distribute) | `src/store/slices/featureSlice.ts` (1128–1284) | Ported; Top is the greatest Y. Align to stock is new. |
| `dc-app::tool::measure` | `dimensionToolSlice.ts`, `dimensionRendering.ts` | The two-click tape and its label. |
| `dc-text::stroke` | `src/text/index.ts` (`GLYPHS`, `layoutGlyphs`) | The Simple Stroke table, flipped to Y-up. |
| `dc-text::layout` | `src/text/index.ts` (`outlineProfilesFromFont`) | The steps (glyph outlines, advance, counters nested). Not ported: three.js typeface fonts, the fixed 0.18 spacing, the Clipper clean-up. Fonts are parsed by `ttf-parser`; curves are flattened by `dc_geometry::flatten`. |
| `dc-app::command` (Make Unique, definitions) | `src/store/helpers/featureDefinitions.ts` (`makeUnique`, `gcOrphanedDefinitions`), `definitionEditing.test.ts` | Ported. |
| `dc-app::tree` (folders) | `src/store/slices/` folder actions, `folderGroup.test.ts` | Flat folders and the grouped flag. |

Not ported, by the functional spec's "Out of scope": the constraint
solver and every call into it from the edit paths (digest §8), driving
and named dimensions, dimension annotations, regions and feature roles
other than construction, text layouts on arcs and paths, canned models,
the second face and rotary work.

## Data model

### dc-geometry

```rust
// path.rs: an outline with curves. The editable form of a Polyline.
/// How a path runs from one node to the next.
pub enum Span {                       // serde: "line" | { "arc": {..} } | { "cubic": {..} }
    Line,
    /// A circular arc; `bulge` as `Vertex::bulge`.
    Arc { bulge: f64 },
    /// A cubic Bézier; the two handles are points in the path's space, millimetres.
    Cubic { handle_out: Point, handle_in: Point },
}
pub struct PathNode { pub x: f64, pub y: f64, pub span: Span, pub smooth: bool }   // `x`, `y` as a `Vertex`'s
pub struct Path { nodes: Vec<PathNode>, closed: bool }      // an open path's last span is unused
impl Path {
    pub fn new(nodes: Vec<PathNode>, closed: bool) -> Self;
    pub fn from_polyline(polyline: &Polyline) -> Self;       // lines and arcs, exactly
    pub fn has_curves(&self) -> bool;
    /// Lines and arcs as they are; each cubic flattened within `tolerance_mm`.
    pub fn to_polyline(&self, tolerance_mm: f64) -> Polyline;
    pub fn spline(points: &[Point], closed: bool) -> Self;   // a smooth curve through the points
    // Editing: each returns a new path, or `None` (for a corner, the reason) when it cannot be done.
    pub fn nearest(&self, p: Point) -> Option<PathHit>;      // the span, how far along it, the gap
    pub fn split_span(&self, span: usize, t: f64) -> Option<Self>;
    pub fn delete_node(&self, node: usize) -> Option<Self>;
    pub fn break_at(&self, node: usize) -> Option<(Self, Option<Self>)>;
    pub fn delete_span(&self, span: usize) -> Option<(Self, Option<Self>)>;
    pub fn fillet(&self, node: usize, radius_mm: f64) -> Result<Self, CornerError>;
    pub fn chamfer(&self, node: usize, distance_mm: f64) -> Result<Self, CornerError>;
    pub fn with_span_kind(&self, span: usize, kind: SpanKind) -> Option<Self>;
    pub fn with_node_at(&self, node: usize, to: Point) -> Option<Self>;
    pub fn with_handle_at(&self, node: usize, side: HandleSide, to: Point, keep_smooth: bool) -> Option<Self>;
    pub fn with_node_smooth(&self, node: usize, smooth: bool) -> Option<Self>;
    pub fn with_arc_through(&self, span: usize, through: Point) -> Option<Self>;
    pub fn with_arc_radius(&self, span: usize, radius_mm: f64) -> Option<Self>;
    pub fn reversed(&self) -> Self;
    pub fn closed_up(&self, tolerance_mm: f64) -> Option<Self>;
}
pub fn bulge_through(start: Point, end: Point, through: Point) -> Option<f64>;   // the arc through three points
pub fn arc_middle(start: Point, end: Point, bulge: f64) -> Point;                // the handle an arc is dragged by
```

`smooth` says the node's two handles are kept in line when one is
dragged; it is `false` for a node with no cubic beside it. A saved file
writes each node on one line, as it writes a vertex.

### dc-project (format 29)

```rust
pub struct FeatureDefinition { pub id, pub name, pub shape: Option<Shape>, pub profiles: Vec<Polyline> }

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Shape {
    /// Centred on the definition's origin, sides along X and Y.
    Rectangle { width_mm: f64, height_mm: f64, corner: Corner },
    Circle { diameter_mm: f64 },
    /// Its bounding box's width and height.
    Ellipse { width_mm: f64, height_mm: f64 },
    /// Centred; the first corner on +X.
    Polygon { sides: u32, radius_mm: f64, measured: Measured },
    /// Centred; its centreline along X; `length_mm` centre to centre.
    Slot { length_mm: f64, width_mm: f64 },
    Gear { teeth: u32, outside_diameter_mm: f64, pressure_angle_deg: f64, flanks: GearFlanks,
           whole_depth_mm: f64, root: GearRoot, root_fillet_mm: f64, crest: GearCrest,
           crest_radius_mm: f64, bore_diameter_mm: Option<f64> },
    /// The baseline starts at the origin and runs along +X; lines follow downward.
    Text { text: String, font: FontRef, height_mm: f64, letter_spacing_fraction: f64,
           line_spacing_fraction: f64, align: Align },
    /// Outlines with curves; `profiles` is each flattened. Never holds a path without a cubic alone.
    Path { paths: Vec<Path> },
}
pub enum Corner { Square, Rounded { radius_mm: f64 }, Chamfered { distance_mm: f64 } }
pub enum Measured { Corners, Flats }
// GearFlanks { Involute, Straight }, GearRoot { Rounded, Flat, Sharp } and GearCrest { Flat, Rounded }
// are dc_geometry::shapes::gear's, re-exported, so the generator and the file share them.
pub enum Align { Left, Centre, Right }
pub struct FontRef { pub family: String, pub style: String }
impl Shape {
    /// The outline this shape's sizes give, in definition space, anticlockwise outers and clockwise
    /// holes. `None` for text, which needs a font; `Err` for a gear whose sizes give no gear.
    pub fn outline(&self, flatten_tolerance_mm: f64) -> Option<Result<Vec<Polyline>, GearProblem>>;
    pub fn outline_matches(&self, profiles: &[Polyline], flatten_tolerance_mm: f64) -> Option<bool>;
}

pub struct FeatureInstance { /* … */ pub construction: bool, pub folder: Option<FolderId>, /* z_top_mm, z_bottom_mm */ }
pub struct Folder { pub id: FolderId, pub name: String, pub group: bool }   // ids `f<n>`
pub struct Project { /* … tabs, */ pub folders: Vec<Folder>, /* instances, bitmaps, … */ }
pub struct Bitmap { /* … locked, */ pub opacity_fraction: f64, /* png_base64 */ }
```

`v28_to_v29` adds `"shape": null` to every definition,
`"construction": false` and `"folder": null` to every instance (after
`locked`), `"folders": []` after `tabs`, and `"opacity_fraction": 1.0`
to every bitmap (after `locked`), and refuses a file that already has
any of them. Fixtures: `dcv/v29/sample.dcv` and `drawing.dcv`
(`test_support::drawing_project`: parametric shapes, a path with a
curve, a folder, a group, a construction instance and a faded picture;
it gains a definition of each kind as the slices that generate their
outlines land).

A folder's rows are its instances in file order, shown where its first
instance is; an empty folder is shown after the shapes, in `folders`
order. Whether a folder is collapsed is view state.

New `Problem`s:

| Code | Tier | When |
| --- | --- | --- |
| `shape_outline_differs` | warning | `Shape::outline` at the project's tolerance does not equal `profiles` (count, closedness, each vertex within `EPS_MM`); not asked of text |
| `construction_targeted` | error | an enabled operation's `targets`, or an Inlay's `boundary`, names a construction instance |
| `dangling_folder` | error | an instance's `folder` names no folder |
| `duplicate_folder_id` | error | two folders share an id |

A size that cannot be is phase 1's `out_of_range` (or `non_finite`) at
its own field, `definitions[d2].shape.width_mm`, so the app marks the
field as it marks any other: a size not above 0, a corner past half the
shorter side, sides outside 3–50, teeth outside 6–200, a pressure angle
outside 10–35°; so is a bitmap's `opacity_fraction` outside 0.1–1, and
every coordinate of a path's nodes and handles past the 6000 mm limit.

`closed_outlines` and every other reader of `profiles` are unchanged.

### dc-text (new crate)

```rust
pub struct Font { /* parsed face and its bytes */ }
impl Font {
    pub fn parse(bytes: Arc<[u8]>) -> Result<Font, TextError>;      // .ttf, .otf; first face of a collection
    pub fn family(&self) -> &str;  pub fn style(&self) -> &str;     // name table ids 16/17, else 1/2
    pub fn kind(&self) -> FontKind;                                  // Outline | SingleLine
}
pub struct Bundled { pub family: &'static str, pub style: &'static str, pub licence: &'static str }
pub fn bundled() -> &'static [Bundled];                              // bytes by include_bytes!, no I/O
pub fn bundled_font(family: &str, style: &str) -> Option<Font>;
pub struct TextSpec<'a> { pub text: &'a str, pub height_mm: f64, pub letter_spacing_fraction: f64,
                          pub line_spacing_fraction: f64, pub align: Align }
/// Outers anticlockwise with their counters clockwise after them; open paths for a single-line font.
pub fn layout(font: &Font, spec: &TextSpec<'_>, flatten_tolerance_mm: f64) -> Result<Vec<Polyline>, TextError>;
```

It depends on `dc-geometry` and `ttf-parser` (0.25, Apache-2.0 OR MIT,
no dependencies, no unsafe in its public use, the crate
`docs/rust-guidelines.md` names for fonts). It does not depend on
`dc-project`; `dc-app` maps `Shape::Text` to a `TextSpec`. The crate
graph gains `dc-geometry → dc-text → dc-app`.

### dc-app

```rust
pub enum Command { /* phase 4's, and: */
    /// New instances, each with a new definition. Ids and names are `apply`'s.
    AddShapes(Vec<NewShape>),
    /// The outline of the definition `instance` places replaced, for every linked copy: a typed
    /// size, a choice, a node edit, a fillet. Tabs on its instances are re-seated on the new
    /// outline. Named for `instance`, the copy the change was made through.
    SetOutline { instance: InstanceId, shape: Option<Shape>, profiles: Vec<Polyline>, what: Edit },
    /// Shapes that keep their sizes made plain paths with the same outlines.
    ConvertToPath(Vec<InstanceId>),
    /// Shapes made from others: a broken path, a join, an offset, weld, subtract, intersect, trim.
    /// `from` go unless `keep`; each pair of `inherit` hands a removed shape's operations,
    /// boundaries and tabs to the new shape at that index.
    ReplaceShapes { from: Vec<InstanceId>, keep: bool, add: Vec<NewShape>,
                    inherit: Vec<(InstanceId, usize)>, what: Edit },
    MakeUnique(Vec<InstanceId>),
    /// Linked copies of `ids`, one set per transform, optionally filed in a new grouped folder.
    CopyInstances { ids: Vec<InstanceId>, placements: Vec<Transform>, folder: Option<String> },
    /// One transform per instance: align, distribute.
    PlaceInstances { moves: Vec<(InstanceId, Transform)>, what: Edit },
    SetConstruction { ids: Vec<InstanceId>, construction: bool },
    NewFolder { name: String, ids: Vec<InstanceId>, group: bool },
    RenameFolder { id: FolderId, name: String }, DeleteFolder(FolderId), Ungroup(FolderId),
    /// Rows moved to `before` (an index into `instances`, or the end) and into `folder`.
    MoveRows { ids: Vec<InstanceId>, before: Option<InstanceId>, folder: Option<FolderId> },
    SetBitmapOpacity { ids: Vec<BitmapId>, opacity_fraction: f64 },
    PlaceBitmap { id: BitmapId, x_mm: f64, y_mm: f64, width_mm: f64 },   // scale by two points
}
pub struct NewShape { pub name: String, pub shape: Option<Shape>, pub profiles: Vec<Polyline>,
                      pub transform: Transform, pub construction: bool, pub folder: Option<FolderId> }
pub enum Edit { Size(SizeField), Corners, Measured, Flanks, Root, Crest,   // slice 8
                MoveNodes { count }, Reshape, AddNode, DeleteNodes { count }, Break, DeleteSegment,
                CloseEnds, JoinEnds,                                      // slice 10
                Fillet { count }, Chamfer { count }, SegmentKind(SpanKind), // slices 11, 12
                Offset, Weld, Subtract, Intersect,                          // slices 13, 14
                Trim, Join,                                                 // slices 15, 16
                Weld, Subtract, Intersect, Trim, Join, JoinEnds,
                Align(Side), Distribute(Axis), Regenerate }   // the undo label's words
```

`SetOutline` refuses an outline with a non-finite or out-of-range
vertex, an empty one, and a definition one of whose instances is
locked (a locked copy holds the shape for them all). A stretch that
changes sizes is not a command of its own: `TransformInstances` does it
("Sizes as placed, and stretching"), so a stretch of several shapes
stays one undo step. Re-seating a tab (`command::shapes::reseat_tabs`)
carries its point to the same share of the new outline's box as it had
of the old one's, then takes `dc_geometry::nearest_point` on the
profile of its index, or on the nearest profile when that index is
gone; a tab is deleted only when its definition has no profile left.

```rust
// tool.rs: the Viewport's tool in use. Not project data, not in the history.
pub enum Tool { Select, Nodes, Tab, Trim, Measure, Draw(Drawing) }
pub enum Drawing { Line, Arc, Spline, Rectangle, Circle, Ellipse, Polygon, Slot, Gear, Text }
pub enum Event { Move(Pointer), Press(Pointer), Release(Pointer), DoubleClick(Pointer), Key(Key), Leave,
                 Field { field: SizeField, text: String }, Option(ToolOption), Done }
pub enum Key { Enter, Escape, Backspace, Delete }
pub enum SizeField { X, Y, Width, Height, Diameter, Radius, Angle, Length, SlotWidth,
                     Teeth, PressureAngle, WholeDepth, RootFillet, CrestRadius, Bore }
pub enum Quantity { Length, Angle, Count }
pub enum ToolOption { Corners(CornerStyle), CornerSize(String), Sides(u32), Measured(Measured), Construction(bool),
                      Flanks(GearFlanks), Root(GearRoot), Crest(GearCrest) }
pub struct Reply { pub ran: Option<String>,            // the undo label of what this event committed
                   pub refused: Option<String> }       // a reason for the status bar
impl App {
    pub fn tool(&self) -> Tool;
    pub fn arm(&mut self, tool: Tool);                 // drops any draft; refused while targets are edited
    pub fn tool_event(&mut self, event: Event) -> Reply;
    /// What the tool wants drawn this frame, in project millimetres.
    pub fn draft(&self) -> &Draft;
    /// The tool bar: the prompt and the fields of the step in hand.
    pub fn tool_bar(&self) -> Option<ToolBar>;
    pub fn drawing_construction(&self) -> bool;        // the tool bar's Construction switch
}
pub struct Draft { pub outlines: Vec<Polyline>,        // the shape as it would be made now
                   pub snap: Option<Snapped>,          // the marker and its guide
                   pub labels: Vec<Label> }            // 83a: a size and where to write it
                                                       // (nodes and highlights join it with their tools)
pub struct ToolBar { pub tool: Tool, pub prompt: String, pub fields: Vec<SizeRow>,
                     pub choices: Vec<Choice>,         // one of a few: a gear's flanks, root and crest
                     pub note: Option<String>,         // read, not typed: a gear's module
                     pub can_finish: bool }            // whether Done would make a shape now
pub struct Choice { pub label: String, pub help: String, pub options: Vec<ChoiceOption>, pub selected: usize }
pub struct ChoiceOption { pub label: String, pub option: ToolOption }
pub struct SizeRow { pub field: SizeField, pub label: String, pub help: String, pub text: String,
                     pub quantity: Quantity, pub pinned: bool, pub problem: Option<String>,
                     pub option: bool }               // a tool's option, not a size of the step
```

A tool's options are preferences (`Prefs::drawing`: the rectangle's
corners and their size, the polygon's sides and what its radius reaches)
and, for the Construction switch, the session's; the window reads them
there and sets them with `Event::Option`.

`snap`:

```rust
pub enum SnapKind { Endpoint, Midpoint, Centre, Intersection, OnOutline, Perpendicular, Grid }
pub struct SnapSettings { pub enabled: bool, pub kinds: BTreeSet<SnapKind>, pub radius_px: f64 }  // Prefs
pub struct Pointer { pub at_mm: Point, pub shift: bool, pub alt: bool, pub command: bool }
pub struct Targets<'a> { pub scene: &'a Scene, pub stock: &'a Stock, pub origin: Point, pub px_per_mm: f64,
                         pub grid_step_mm: Option<f64>, pub skip: &'a [InstanceId] }
pub struct Snapped { pub at_mm: Point, pub kind: Option<SnapKind>, pub guide: Option<(Point, Point)> }
pub fn resolve(targets: &Targets<'_>, settings: &SnapSettings, pointer: &Pointer, reference: Option<Point>) -> Snapped;
pub fn grid_step_mm(px_per_mm: f64, units: Units, pinned_mm: Option<f64>) -> f64;
pub fn constrain_angle(from: Point, at: Point) -> Point;
impl App { pub fn snap_at(&self, pointer: &Pointer, reference: Option<Point>, skip: &[InstanceId]) -> Snapped;
           pub fn grid_step_mm(&self) -> f64; }
```

`Prefs` gains `snap: SnapSettings`, `grid_spacing_mm: Option<f64>`
(`None`: automatic) and `drawing_tool: Drawing` (the Draw button's),
and with the drawing tools their last-used options (corner style and
size, polygon sides and measure, the text defaults). `gui.toml` holds
them in two tables, `[snap]` (`enabled`, `kinds` by name, `radius_px`)
and `[drawing]` (`tool` by name, `grid_spacing_mm`, the options); a
name this build does not know is left out, a radius is held to 6–32 px,
and changing any of them is a preference the user set
(`shell::window::user_set`).

`ReplaceShapes` (slice 10, where a broken path first needs it) puts the
new shapes where the first of `from` was in the Design Tree, or, with
`keep`, after the last of them, and names its undo step for `from`.
For each pair of `inherit`, an operation's targets and an Inlay's
boundary have that new shape in the removed one's place, and the
removed one's tabs move to it and are re-seated on its outline (it is
placed as the old one was, so their points are in the same space). The
other removed shapes leave every target list and their tabs go. A
locked shape is not removed.

**Offset and the booleans** (`dc_app::edit`, slices 13 and 14) are pure
functions of the project giving one `ReplaceShapes`. `offset` works in
each shape's own space, by the distance divided by its placement's even
scale: its closed outlines are nested into regions (`dc_geometry::nest`)
so a hole is known as a hole, each outer is offset by the signed
distance and each hole by its negative (`dc_geometry::offset`, round
joins, arcs kept), and each open outline is moved left or right
(`offset_open`); *Outward* is the left of an open path's direction. The
result is one plain path per source, `<name> offset`, placed, filed and
marked as its source; a source whose offset is empty is returned in
`Offsets::vanished`, and an offset that makes nothing at all is
refused. `offset_outlines` is the same in project space for the
tool bar's preview. `boolean` places every shape's outlines in project
space, nests them, folds `dc_geometry::boolean` over them in selection
order at the project's flatten tolerance, and brings the result back
into the first shape's space: one path, `<name> weld` (`subtract`,
`intersect`), with the first shape's placement and its operations. A
shape with an open outline, fewer than two shapes and an empty result
are refused with the reason.

`Selection` is unchanged; the nodes selected in Edit nodes are the
tool's state. `Scene::Shape` gains `construction: bool`, the
definition's id and `centre`, where a parametric shape's own origin is
placed; `pick` skips a locked picture.

## Algorithms

Coordinates are project space, Y-up, millimetres, unless a subsection
says definition space. Angles are radians in code and degrees at the
user's edge, anticlockwise positive.

### Shape outlines (`dc_geometry::shapes`)

All closed outlines are anticlockwise, and each starts where its
subsection says, so the same sizes give the same vertices. The functions
take sizes the caller has checked; a size that cannot be gives a
degenerate outline, never a panic.

- **Rectangle.** Square: `Polyline::rect` from (−w/2, −h/2). Rounded,
  radius `r` held to `min(w, h)/2`: from the left end of the bottom
  side, four lines and four arcs of bulge `tan(π/8)`; at exactly half
  the shorter side the two arcs of that end meet and the side between
  them, having no length, is left out. Chamfered: the same eight
  vertices with no arcs. (Upstream stops a micrometre short of half the
  side and builds the rounded one by filleting a rectangle four times.)
- **Circle.** `Polyline::circle`: two half-circle arcs from its point on
  +X.
- **Ellipse.** Straight segments between points of the true ellipse,
  from its point on +X: `n = ⌈2π / (2·acos(1 − t/r))⌉` of them, `r` the
  greater radius and `t` the tolerance, rounded up to a multiple of
  four and held to 16–16 384. The ellipse is the circle of radius `r`
  squeezed along one axis, which moves no chord further from its curve,
  so every chord is within `t`. The first quadrant's points are
  computed and the other three mirrored, so the four axis points are
  vertices, the box is exactly the sizes and the outline is symmetric
  to the last bit. (Upstream's ellipse is four Béziers.)
- **Polygon.** `sides` vertices at `radius · (cos, sin)(2πi/sides)`;
  measured to the flats, the corner radius is `radius / cos(π/sides)`.
- **Slot.** Vertices (−l/2, −w/2) line to (l/2, −w/2), bulge 1 to
  (l/2, w/2), line to (−l/2, w/2), bulge 1 back: two lines, two exact
  half circles centred (±l/2, 0).
- **Gear.** `gearProfile.ts`'s `buildGearProfile`: the tooth outline
  sampled at five points per flank and per transition, repeated
  `teeth` times from the left root shoulder of the tooth on +X, lines
  only; the bore, when set, a clockwise circle after it. Its sizes are
  checked, not clamped as upstream clamps them: `Gear::problems` lists
  what is wrong, each a `GearProblem` naming its size. Validation
  reports each as `out_of_range` at that size's field; teeth cut past
  the centre are said once, without the fillet and bore that then
  cannot fit either.

`Shape::outline_matches` is what `shape_outline_differs` asks: the same
number of outlines, each with the same vertices in the same order
within `EPS_MM` (bulges within 1e-9). An outline flattened from a curve
(an ellipse, a path with curves) may have been flattened by another
build into other vertices of the same curve, so for those it is enough
that neither outline has a vertex further than the tolerance from the
other.

### Paths (`dc_geometry::path`)

- **Arc through three points** (`buildArcSegmentFromThreePoints`):
  the circumcentre of start, through and end; the bulge is
  `tan(sweep/4)` with the sweep's sign from `cross(through − start,
  end − start)`: negative cross is anticlockwise in Y-up, the reverse
  of upstream's `clockwise` flag. Collinear within 1e-9 is `None`.
- **Spline through points** (`splineProfile`, `appendSplineDraftSegment`):
  closed, each span is `out = p1 + (p2 − p0)/6`, `in = p2 − (p3 − p1)/6`
  with indices wrapping; open, the same with the end points repeated
  (`p0 = p1` at the start, `p3 = p2` at the end), which is what
  upstream's incremental build gives. Every node is smooth.
- **Split** at parameter `t` of a span: a line at the point; an arc
  into two arcs of the same circle (bulges `tan(t·θ/4)` and
  `tan((1−t)·θ/4)`); a cubic by de Casteljau.
- **Delete a node**: its neighbours are joined by a line, or by a cubic
  taking the outer handles when either span was a cubic; an arc beside
  a deleted node becomes a line (upstream's rule).
- **Fillet** at a node between two lines, interior angle `θ`: refused
  when `θ` or `π − θ` is under 1e-3, or `trim = r / tan(θ/2)` reaches
  either side's length; else two nodes at `trim` along each side joined
  by the minor arc. **Chamfer**: the same two nodes at `distance`,
  joined by a line.
- **Segment kind**: a cubic to the arc through its point at `t = ½`; a
  line, which has no middle to bow through, to an arc of bulge ¼ (it
  bows out by an eighth of its length, to be dragged from there); a line
  to a cubic with handles a third of the way along it; an arc to the
  cubic with the standard `4/3·tan(θ/4)·r` handles, one for each quarter
  turn or part of one, with a node between each; anything to a line by
  dropping the bulge or handles.
- **Handles**: dragging one at a smooth node turns the other to stay in
  line, keeping its length; dragged alone (`keep_smooth` off) the node
  becomes a corner; `with_node_smooth` brings the two back into line
  along the line between them.
- **Arc radius**: a typed radius keeps the ends, the side the arc bows
  to and whether it is over a half turn; under half the chord is
  refused (`arcHandleFromRadius`).
- **Moving a node beside an arc**: the arc's middle point is taken
  first, and the arc is rebuilt through the moved end, the other end
  and that point.

### Intersections and Trim

`intersect::crossings(a: &Segment, b: &Segment) -> Vec<Crossing { at,
along_a, along_b }>` for line–line, line–arc and arc–arc, each `along`
the fraction of the segment's length, within `EPS_MM` at the ends;
collinear and concentric overlaps give none.

**Trim** of profile `p` at clicked point `c`, against `others`:

1. Collect every crossing of `p` with each profile of `others` and with
   its own non-adjacent segments, as distances along `p`, sorted, with
   duplicates within `EPS_MM` merged.
2. None: nothing to trim. Report it.
3. `d` = the distance along `p` nearest `c`. On an open path the piece
   is from the last crossing before `d` (or the start) to the first
   after (or the end); on a closed one, between the two crossings
   round `d`, wrapping; a closed path with one crossing is opened at it
   and nothing is removed.
4. Remove the piece: an open path leaves zero, one or two open paths; a
   closed one leaves one open path from the far crossing round to the
   near one. A remainder shorter than `EPS_MM` is dropped.

The pieces are cut from the outline's own segments
(`Segment::piece`), so arcs stay arcs. A path with curves is trimmed on
its flattened outline and the result is a plain path. An open
outline's own two ends are no crossing: an end that only touches
another outline has nothing past it to trim to.
`dc_geometry::trim(path, at, others) -> Option<Trimmed { removed, kept }>`;
`None` when nothing crosses.

The Trim tool (`dc_app::tool::trim`) takes the outline nearest the
pointer within the pick tolerance, among the Scene's outlines as they
are placed, and trims it against all the others; `removed` is the
draft's highlight as the pointer moves. What is kept goes back through
the instance's inverse transform. One piece, or a shape with other
outlines or linked copies, is a `SetOutline` with `shape` `None` (so a
shape that keeps its sizes becomes a plain path in the same step); two
pieces of a shape that is one outline of its own are `ReplaceShapes`,
the first with what cut it.

### Join

`join_open(paths, tolerance_mm) -> Joined { chains: Vec<Chain { outline,
from }>, alone }`: a chain grows from the first free outline in order,
at either end, taking each time the nearest free end within the
tolerance (ties by input order), reversing an outline where needed,
with a line between ends more than `EPS_MM` apart; a chain, or one
outline, whose own ends are within the tolerance closes. It is the DXF
importer's stitch rule, but not its code: the importer moves a joined
piece by the gap where this bridges it, and is built for a file of
thousands of entities, on a grid and under a work budget
(`dc-import`'s `dxf/stitch.rs`, 1,400 lines with its hatch and
duplicate handling). Moving that to `dc-geometry` to serve a selection
of a few paths would have put the importer's golden corpus at risk for
no gain, so the importer keeps its own and this is a second, small
implementation of the rule.

`dc_app::edit::join` places the selected shapes that are one open
outline of their own (no linked copy, not locked), joins them in
project space, and makes each chain one shape in its first path's
space with that path's name, placement and operations
(`ReplaceShapes`); the rest are counted as left alone.

### Snapping

`snap::resolve` gathers candidates within `radius_px / px_per_mm` of the
pointer, from the outlines the `Scene` holds, the stock rectangle and
the origin, then takes the best by (rank, distance): rank 1 endpoint,
midpoint, centre, intersection; 2 perpendicular; 3 on outline; 4 grid. A
box test on each shape's bounds skips shapes out of reach, and
intersections (`dc_geometry::crossings`) are sought only between
segments whose boxes are in reach, as upstream does; neighbours of one
outline share their vertex, which is an endpoint and not a crossing.
`Centre` comes from arcs' centres, from a parametric shape's placed
origin, and from `as_circle` for a circle that came in as straight
segments; a slot's two end centres are its arcs'. `Perpendicular` is the
nearest point of a segment to `reference` where the line to it meets the
segment square.

The grid candidate is the pointer rounded to the grid step on each axis,
with no reach test (Graphite), so it wins only when nothing else is in
reach. `grid_step_mm` is the pinned spacing, or the smallest of 1, 2, 5
× 10ⁿ project units whose spacing is at least 8 px: between 8 and 20 px
apart, so no point is more than the default radius from a grid point,
and a whole fraction of phase 4's drawn grid (1, 10, 100 mm; 0.1, 1,
10 in), which stays as it is.

`Shift` first turns the pointer onto the ray from `reference` at the
nearest multiple of 15°, its distance kept; candidates are then kept
only within half a pixel of that ray, and the grid sets the length
along it in whole steps. `command` returns the pointer, turned onto the
ray when `Shift` is down too.

### The tool state machines

Two-point tools (Rectangle, Circle, Ellipse, Polygon) share one
machine: `Idle → First(p1)` on a press's point, then a `Release` more
than 4 px from the press is the second point (a drag), else the next
`Press` is. Slot is `Idle → First → Second → width`. Line, Arc and
Spline share `PathDraft { points, spans, arc_end }` (`tool/path.rs`):
the points clicked, what runs between each two (a line, an arc with its
bulge, or a curve), and the end of an arc that waits for its through
point. The three are one path's three kinds of span: `App::arm` keeps
the draft when it changes among them, so `A`, `S` and `P` change what
the next span is. A point is taken on the press. `Enter` takes the
point the fields give while a size is typed, and finishes the path
open when none is; **Done** and a double-click finish it (a
double-click's second press lands on the first and adds nothing).
Finishing drops an arc's end that still waits. A typed radius gives
the minor arc, `bulge = sagitta / half chord`, signed by the pointer's
side of the chord. When the path is made, each run of curves is
`Path::spline` through its points, its two ends corners; a path that
is all curves is one spline, open or closed. Gear is
`Idle → Centre → Sizes` (`tool/gear.rs`), the last waiting for **Done**
or `Enter`. The radius's click gives `Gear::with_defaults(radius)`;
after it each size is its own, held in the state as it was typed, and
`Gear::problems` is read every time the bar is: each problem is said at
the field of the size at fault (`GearProblem::size`), in project units
where it names a limit, and `can_finish` is false while there is one.
Text that is no number leaves the size and is said at the field too. A
field shows only while the gear's form reads it (the pressure angle for
involute flanks, the fillet for a rounded root, the radius for a
rounded crest). The preview is the gear's outline, or its outside
circle while the sizes give no gear. A click in the Viewport in `Sizes`
does nothing, as upstream's does. The bar's choices are data
(`ToolBar::choices`), each option carrying the `ToolOption` that
chooses it. The Rectangle's corners and the Polygon's measure are
choices of their bars too, and the corner size (while the corners are
not square) and the sides are rows after the step's sizes, in every
step, marked `option`: typing one sets the preference
(`Prefs::drawing`) for the shapes that follow, pins nothing, and
`Enter` in its field commits nothing. Every machine
answers `Field` events by pinning that size and recomputing the preview
point from the fields (`manualEntry.ts`'s `computeDimensionEditPreviewPoint`),
and `Key::Enter` by committing with the fields as they read. `Escape`
steps back: an open field, then the draft, then the tool.

**Edit nodes** (`tool/node.rs`). What is edited is the app's selection
when it is one instance whose definition is a path: `shape` `None`
(each profile read as a path, exactly) or `Shape::Path`. A shape that
keeps its sizes shows the bar's *Convert to Path*
(`NodeAction::Convert`). The tool holds the selected nodes
(`NodeRef { path, node }`), the selected span, and what the pointer has
hold of. A press hits, in order, the nearest node within 9 px, a
curve's handle or an arc's middle within 7 px, a span within 9 px
(`hitEditableControl`), all measured where the shape is placed; on
nothing it starts a box, or takes the shape under the pointer to edit
instead. A press that moves 4 px is a drag: the paths are recomputed
from the snapshot each move (`dragged`), drawn as the draft, and
committed on release as one `SetOutline`. The dragged point is the
snapped pointer through the instance's inverse transform, so it lands
under the pointer however the shape is placed (`editInPlace.test.ts`);
the other selected nodes move as far the same way. The outline written
is each path flattened at the project's tolerance, and `shape` is
`Shape::Path` only when a path has a curve.

A single open end dropped within 14 px of its own other end closes the
path; within 14 px of an end of another shape that is one open path
(visible, unlocked, not a linked copy), the two become one shape
through `ReplaceShapes`, the other's path brought into this shape's
space (`Path::transformed`, `Path::joined`). Break and Delete segment
on an open path make two shapes the same way when the shape is one
path of its own; a path of a shape with other outlines, or with linked
copies, stays in its definition as two outlines, so the copies follow.
`Draft` carries what the overlay paints: `nodes`, `handles`,
`highlights` (the selected span) and `marquee`.

**Fillet, Chamfer, segment kinds** (slices 11 and 12). The bar's
buttons are data (`ToolBar::actions`, each a `NodeAction`): *Fillet*
and *Chamfer* cut the selected corners by the tool's corner size, an
option row of the bar that is the Rectangle tool's own preference
(`Prefs::drawing.corner_mm`), read as the shape is placed. Corners are
cut from each path's last node back, since a cut corner is two nodes;
one that cannot take the size is left, and the reply carries both the
undo label and the reason. With a span selected the bar offers *Make
Straight*, *Make Arc* and *Make Curve*, the one it is disabled
(`Path::with_span_kind`). **Shape ▸ Fillet Corners…** is
`dc_app::edit::fillet_corners`, a pure function giving one
`SetOutline`: every corner of the one shape that can take the radius.
`dc_app::edit` also holds what a path shape is (`paths_of`,
`outline_of`), which the node tool reads.

A committed shape is `AddShapes` with the shape's outline in definition
space and the instance transform that places it: a translation to the
centre for the box shapes, with a rotation for a Slot (its axis) and a
Polygon (its first corner). A path is saved in project coordinates
under an identity transform, as an import is: with only lines and
arcs it is its outline and has no `shape`; with a curve it is
`Shape::Path` and its flattened outline. It is named for its spans
when they are all of a kind (*Line*, *Arc*, *Spline*), else *Path*.

### Sizes as placed, and stretching

`dc_app::shape` is the Shape section and its edits, pure functions of
the project as `dc_app::properties` is: `section(project, strings, id)`
gives the rows (`SizeRow`s), the choices and the note the tool bar's
types already carry; `set_size` and `set_option` give the `SetOutline`
that a typed size or a choice makes.

Properties shows a length `s` as `s · k`, where `k` (`even_scale`) is
the length of the instance's linear part's columns when that part is a
similarity, and exactly 1 within 1e-9 of it, so a size typed for a
shape that is only turned is saved as typed; typing `v` writes `v / k`.
A linear part that is not a similarity, which only a file can hold,
shows the drawn sizes read-only with *Stretched: convert to a path to
edit it*. A typed corner size, and a corner under a side that shrinks,
are held to half the shorter side. Changing what a polygon's radius
reaches re-reads the radius (× or ÷ `cos(π/n)`) and leaves the polygon
as it is. A gear's sizes are checked by `Gear::problems`, the first
reason given.

`TransformInstances` with a delta that is not a similarity asks each
instance whose shape keeps sizes to take it (`shape::stretch`):

- a Polygon, a Gear and a Text refuse (`StretchRefused`);
- an instance whose definition another instance places refuses
  (`StretchLinked`: *Make Unique to stretch this copy*), also when both
  copies are in the selection;
- the delta must be along X and Y, and the instance's linear part a
  similarity whose X column lies along X or Y within 1e-9, else it
  refuses as not square to the axes;
- otherwise the delta's two factors, taken along and across the shape's
  own X axis, become sizes: a Rectangle's and an Ellipse's width and
  height, a Circle's two diameters as an Ellipse, and for a Slot the
  width across and, along, what is left of its box
  (`(length + width) · along − width · across`, refused under 1 µm);
  the definition gets the shape and the outline it gives, its tabs are
  re-seated, and the instance keeps its linear part and moves so that
  its centre goes where the delta sends it.

A shape that keeps no sizes follows phase 4's rule: lines stretch
through the transform, arcs are refused with the geometry's reason.

### Text layout (`dc_text::layout`)

1. Scale: `height_mm / cap_height`, the font's `capHeight` (OS/2), or
   the height of `H` when the table lacks it.
2. For each line, for each character: the glyph by `cmap`, a missing
   one by the font's `.notdef` and a note; advance by `hmtx`, plus the
   `kern` table's pair value when the font has one, plus
   `letter_spacing_fraction · height_mm`.
3. Each glyph's outline through `ttf_parser::OutlineBuilder` into a
   `kurbo::BezPath` (quadratics and cubics), flattened by
   `dc_geometry::flatten`.
4. Lines are offset down by `line_spacing_fraction ·
   (ascender − descender + line_gap)` and shifted by `align` against
   the widest line.
5. Contours are oriented by containment: depth 0 anticlockwise, depth 1
   clockwise, and so on, so a counter is a hole whatever the font's own
   winding; each outer is followed by its holes.

A single-line font skips steps 3 and 5 and emits its strokes as open
polylines.

### Arrays, align, distribute

Grid: copy `(c, r)` is the selection translated by `(c · px, r · py)`,
`px` the pitch, or the selection's width plus the gap. Circular: copy
`k` is the selection rotated by `k · total / n` about the centre (`n`
copies for a full turn, `n − 1` steps for less), or translated to that
position when the copies are not turned. Align and distribute compute
one translation per instance from the instances' placed boxes, as
`featureSlice.ts` does.

## Module layout

```
crates/dc-geometry/src/{shapes.rs, shapes/gear.rs (port), path.rs, path/{edit.rs (port), spline.rs (port)},
                        intersect.rs (port), trim.rs, join.rs}
crates/dc-project/src/{shape.rs (Shape, its outline, its checks), model.rs (the fields), migrate.rs (v28_to_v29),
                       validate.rs (the six problems), ids.rs (FolderId)}
crates/dc-text/{Cargo.toml, README.md, fonts/ (the bundled files and their licences),
                src/{lib.rs, font.rs, layout.rs, stroke.rs (port), bundled.rs}}
crates/dc-job/src/checks.rs, project_from.rs            — skip construction instances
crates/dc-app/src/{tool.rs, tool/{draw.rs (box shapes, slot), gear.rs, path.rs (line, arc, spline), text.rs,
                   node.rs, trim.rs, adjust.rs (Offset…, Fillet Corners…, Join… in the tool bar), measure.rs,
                   fields.rs (typed sizes), bar.rs}, snap.rs, shape.rs (the Shape
                   section, sizes as placed, stretch), edit.rs (offset, weld, subtract, intersect, join, fillet
                   corners as commands), array.rs (port), fonts.rs (the fonts on hand), clipboard.rs,
                   command/{shapes.rs, folders.rs}, arrange.rs (align, distribute), tree.rs (folders)}
crates/dreamcarve-shell/src/fonts.rs                     — the user's fonts folder: list, add
crates/dreamcarve-app/src/{viewport/{tools.rs (events to dc-app), tool_bar.rs, draw_menu.rs, draft.rs (the
                           overlay: preview, nodes, snap marker, labels)}, panels/{shape.rs (the Shape section),
                           array.rs (popover)}, shell/menu.rs (Draw, Shape), keyboard.rs}
docs/book/src/{quick-start/first-design.md, user-guide/{drawing-shapes.md, snapping.md, text.md, editing-nodes.md,
               changing-shapes.md, copies-and-arrays.md, folders-and-construction.md, tracing.md, measuring.md}}
```

`dc-app`'s manifest gains `dc-text`; `dreamcarve-app`'s gains nothing.
The Tab tool's armed flag moves from egui's memory to `Tool::Tab`.

### The window (slice 9)

The frame reads the app and pushes `Action`s, as phase 4's does; four
are new: `Arm(Tool)`, `Tool(tool::Event)`, `SetSnap`, `SetGridSpacing`.
Arming any tool but Select opens the Design step and hides the start
screen, which also offers **Draw from scratch**.

- `viewport/mod.rs`: while `Tool::Draw` is armed, `tool_gestures` has
  the pointer in place of phase 4's `gestures`: the middle button and
  Space still pan; the primary button's press and release, a
  double-click, and each move are `tool::Event`s in project millimetres
  with Shift, Alt and Ctrl/⌘. A move is sent only when the pointer or a
  held key changes; over a pill or the tool bar the tool keeps its last
  point, so a size is typed with the preview where the pointer left it;
  off the pane it is told `Leave`. Enter, Escape, Backspace and Delete
  go to the tool (`keys`), so Delete deletes nothing while drawing. The
  selection's handles are not drawn and nothing under the pointer is
  picked.
- `viewport/draft.rs` paints `App::draft`: outlines flattened at the
  zoom band's tolerance, the size labels in label pills, the snap
  marker by kind with the kind's name, the dashed guide.
- `viewport/tool_bar.rs` draws `App::tool_bar` under the pills,
  centred. It measures its parts each frame and breaks into rows by the
  widths of the frame before (`rows`), since egui's own wrapping splits
  a label from its field. A field commits on leaving it
  (`Event::Field`); Enter then sends `Key::Enter`; Escape in a pinned
  field unpins it. A field's text is selected as it takes the keyboard.
  Tab from the drawing goes to the first field: `shell::focus` says
  what had the focus when Tab was pressed (`tab_from`) and is told the
  press was taken (`tab_taken`), so its walk does not correct it.
- `viewport/controls.rs`: the pill gains the Draw button (the icon of
  `Prefs::drawing_tool`) and a 24 px chevron that opens the drawing
  tools (`Drawing::is_ready`: all but Text until slice 24), each row
  its icon, name and key. `Fit::of` drops, in order, *Layers*' label,
  the zoom percentage, the two zoom buttons. `ways` takes two rows of
  the tool bar off Fit's top while a drawing tool is armed.
- `shell/menu.rs`: **Draw** (the tools, the armed one checked),
  **Shape** (*Convert to Path*), View ▸ **Snapping** (the switch, with
  ⇧S), **Snap To** (the kinds), **Grid** (*Show Grid*, *Automatic
  Spacing*, and the spacing typed). A checked row's accessible name
  ends with its keys, as a plain row's does.
- `keyboard.rs`: `SelectTool` (V), `DrawLine` (P), `DrawArc` (A),
  `DrawSpline` (S), `DrawRectangle` (M), `DrawCircle` (C),
  `DrawEllipse` (E), `DrawPolygon` (G), `DrawSlot` (O),
  `ToggleSnapping` (⇧S), and the menu `Draw`; the Keyboard chapter's
  table follows.
- `panels/properties.rs`: `shape_section` draws `shape::section` above
  the placement fields for the one shape selected; a choice takes the
  panel's width under its label. `panels/side.rs`: no Inspector for
  shapes while a drawing tool is armed.
- Every size and choice has a help string (`SizeField::help`,
  `Choice::help`), held to the rule every field's help is
  (`dc-app` `tests/help.rs`).
- `tests/strings.rs` `every_string_has_its_glyphs`: each character of
  each string of the table has a glyph in the bundled fonts. egui stops
  a frame that lays out a character with none, which a size label's
  diameter sign did; the label reads `Ø 40`.

### The window (slice 17)

Editing adds no panel and no popover. `Tool` has `Nodes`, `Trim` and
`Adjust(Adjust)`, with `Adjust` one of `Offset`, `FilletCorners` and
`Join`: a Shape command that takes a value is armed as a tool is, and
the tool bar of slice 9 is its form.

- `dc_app::tool::adjust`: `AdjustPrefs` (the distance, side and
  original of an offset, the fillet radius, the join tolerance; in
  `Prefs`, kept for the session and not written to `gui.toml`).
  `adjust_bar` is one field (`Distance`, `Radius` or `Tolerance`, an
  option row, so Enter in it is not a click), Offset's two choices
  (*Side*, whose first two options read *Left* and *Right* when every
  selected outline is open, and *Original*), and `can_finish` when
  `adjust_command` makes a command; the reason it cannot is the
  field's problem. `Done` or Enter runs the command and arms Select,
  the note (an offset that vanished, paths left alone) going to the
  status bar; Escape arms Select. `refresh_adjust_draft` puts
  `edit::offset_outlines` in the draft as the values change.
- `viewport/mod.rs`: `takes_pointer` is true for `Draw`, `Nodes`,
  `Trim` and `Adjust`, so `tool_gestures` has the pointer and the
  selection's handles are not drawn. In Edit nodes `Ctrl`/`⌘ A` is
  `NodeAction::SelectAll` and the arrows nudge the selected nodes by
  the steps they nudge shapes by with Select (`Shift` and `Alt` too). With
  Select, a double-click on a shape selects it alone and arms Edit
  nodes (`a_double_click_shows_nodes`).
- `viewport/draft.rs`: a node is a 9 px square, filled when selected,
  a smooth node a circle; a handle a 7 px circle on a line from its
  node; the piece Trim would take is 3 px in `accent`; the node
  marquee as the selection's.
- `viewport/tool_bar.rs`: the bar's `actions` are buttons after its
  fields (*Fillet*, *Chamfer*, the kinds the selected segment can be
  made, *Break*; or *Convert to Path* alone for a shape that keeps its
  sizes), each with its help, off while no node is selected. Done is
  drawn only when `can_finish`.
- `viewport/controls.rs`: the pill is Select, Edit nodes, Draw and its
  chevron, Tab. Trim is the last row of the chevron's popover, under a
  rule, and the Draw button shows its scissors pressed while it is
  armed: a fifth button cost the zoom buttons at the Viewport's least
  width.
- `shell/menu.rs`, **Shape**: *Convert to Path*, *Fillet Corners…*,
  *Offset…* (⇧O), *Weld*, *Subtract*, *Intersect*, *Join…* (⌘J). Weld,
  Subtract and Intersect act at once on two or more shapes and say why
  when they cannot; the three with an ellipsis arm their mode.
- `keyboard.rs`: `EditNodes` (N), `BreakPath` (B), `Offset` (⇧O),
  `Join` (⌘J).
- The context menu the functional spec names (Break, the segment
  kinds, Fillet, Convert to Path) comes with slice 18, where cut, copy,
  paste and Make Unique give the Viewport one; until then every row of
  it is on the tool bar or in the Shape menu.

## Visual design

Phase 4's tokens and components hold. Added to the palette, in both
themes, with the spec tables' tests extended: `viewport.construction`
(the dashed construction outline), `viewport.draft` (a shape being
drawn; the accent), `viewport.node` and `viewport.node_selected`,
`viewport.handle` (a curve's handle and its stem), `viewport.snap`
(the marker and its label), `viewport.measure`. Slice 9 adds none of
them yet: the draft is drawn in `accent` and the snap marker in
`viewport.shape.hover`, which are the colours `viewport.draft` and
`viewport.snap` were to alias; the tokens come with the first slice
that needs a colour of its own (construction geometry, nodes). The renderer gains two
styles, `Construction` and `ConstructionSelected`; the draft, the
nodes, the snap marker and the size labels are egui painter overlays,
as phase 4's handles are. A node is a 9 px square, a handle a 7 px
circle, hit within 9 px and 7 px (upstream's radii). The snap marker is
a square (endpoint), a triangle (midpoint), a circle with a cross
(centre), a cross (intersection), a circle on the line (on outline), a
right-angle mark (perpendicular) or a small plus (grid). Drawing-tool
icons are Lucide's: `slash` (Line), `radius` (Arc), `spline`, `square`,
`circle`, `ellipse`, `hexagon` (Polygon), `pill` (Slot), `cog` (Gear),
`type` (Text); `ruler` and `scissors` come with Measure and Trim. The
bundled Lucide has an ellipse and a pill, so no glyph is painter-drawn.

## Bundled fonts

Under `crates/dc-text/fonts/`, each with its licence file, each named in
`NOTICE`, `about.toml` and the Licensing chapter. All are SIL OFL 1.1,
which permits bundling and embedding; none is renamed or modified. The
choice is the maintainer's (functional open question 2); this is the
first proposal, one family per job a carved sign has:

| Family | Styles | For |
| --- | --- | --- |
| Inter | Regular, Bold | A plain sans; already bundled for the interface |
| Source Serif 4 | Regular, Bold | A serif |
| Roboto Slab | Bold | A slab, for sturdy lettering |
| Pacifico | Regular | A script |
| Allerta Stencil | Regular | A stencil: letters whose counters do not fall out when cut through |
| Simple Stroke | (single line) | Engraving; PureCutCNC's, Apache-2.0 |

Each file is fetched from its publisher's repository at slice time, its
licence and reserved names read then, and its checksum recorded in
`crates/dc-text/fonts/README.md`.

## Test plan

Names are `crate::module::test`; "(port)" marks a PureCutCNC test ported
under its upstream name; "(acc N)" the acceptance test it carries.

**Format 29 (slice 1)** `dc_project::migrate::v28_to_v29_adds_the_drawing_fields`,
`a_file_claiming_28_with_a_format_29_field_is_refused`, `version_29_fixtures_load`,
`a_v28_project_exports_the_same_programs` (acc 107);
`shape::every_shape_kind_round_trips`, `corners_and_lengths_read_as_the_file_writes_them`;
`validate::a_drawing_project_is_valid`,
`an_operation_targeting_construction_geometry_is_an_error`,
`an_inlay_boundary_of_construction_geometry_is_an_error`,
`a_folder_that_is_not_there_and_a_folder_id_used_twice_are_errors`,
`a_shapes_sizes_are_checked_at_their_fields`,
`a_texts_sizes_and_a_paths_nodes_are_checked`,
`a_bitmaps_opacity_is_held_to_its_range`; the problem snapshot gains
three codes; `dc_geometry::path::a_polyline_round_trips_through_a_path`,
`flattened_cubics_stay_within_tolerance`, `a_path_round_trips_through_json`;
`dreamcarve::cli::v28_project_exports_the_same_programs` (acc 107) and
`a_construction_target_fails_check_and_export` (acc 125).

**Shape outlines (slice 2)** `dc_geometry::shapes::` the ports of
`slot.test.ts` (7), `ngon.test.ts` (7) and `cannedRectProfiles.test.ts`
(15), under names that say what each shows (`rounded_segment_count`,
`slot_arcs_are_half_circles_about_the_end_centres`, …), and
`every_outline_is_closed_and_counter_clockwise`,
`rounded_corners_are_tangent_arcs`, `an_ellipse_stays_within_tolerance`,
`an_ellipses_segment_count_follows_the_tolerance`, `polygon_to_the_flats`;
`shapes::gear::` the ports of `gearProfile.test.ts` (9), and
`each_size_out_of_range_is_named`, `the_default_gear_has_24_fold_symmetry`,
`a_bore_is_a_clockwise_circle_after_the_teeth`; the properties
`a_shapes_outline_is_simple_and_its_box_is_its_sizes` and
`a_gears_outline_is_simple_and_symmetric`; `dc_project::shape::`
`outline_of_every_kind`, `an_outlines_box_is_its_sizes`,
`a_gear_that_cannot_be_has_no_outline`, `a_changed_outline_does_not_match`;
`validate::a_hand_edited_outline_is_shape_outline_differs`,
`a_gears_sizes_are_checked_together` (acc 108–111).

**Paths (slice 3)** `dc_geometry::path::` the ports of
`profileEdit.test.ts` (`insert_point_splits_a_line`,
`split_cubic_returns_the_de_casteljau_halves`, `close_open_path`,
`fillet_a_line_corner`, `chamfer_a_line_corner`,
`delete_node_bridges_across_it`, `delete_span_opens_a_closed_path`,
`break_opens_a_closed_path_at_the_node`,
`arc_through_three_points_bulges_toward_the_third`), and
`a_polyline_round_trips_through_a_path`, `flattened_cubics_stay_within_tolerance`,
`collinear_points_make_no_arc`, `the_arc_passes_through_the_third_point`,
`a_closed_spline_passes_through_its_points`, `an_open_spline_has_short_end_tangents`,
`split_arc_keeps_the_circle`, `a_corner_that_cannot_be_cut_says_why`,
`fillet_any_corner_of_a_closed_path`, `deleting_beside_a_curve_keeps_the_outer_handles`,
`nearest_finds_the_span_and_how_far_along`, `span_kinds`,
`moving_a_node_carries_its_handles_and_rebuilds_its_arcs`,
`a_smooth_handle_carries_its_twin`, `an_arc_is_reshaped_by_its_middle_or_its_radius`,
`reversed_runs_the_other_way`; the properties `splitting_a_span_keeps_the_outline`,
`a_path_reversed_twice_is_itself`, `an_arc_through_three_points_passes_through_them`.
The path geometry of slices 10 to 12 is here too; those slices add the
tool that drives it.

**Tool model and snapping (slice 4)** `dc_geometry::intersect::` the
ports of `segmentIntersection.test.ts` but its four `rayA` cases, which
serve Extend (`line_line_crossing`, `line_arc_two_hits`, `line_arc_tangent`,
`arc_arc_two_hits`, `arc_arc_tangent`, …), and
`a_clockwise_arc_counts_along_from_its_own_start`; `dc_app::snap` (in
`tests/snap.rs`) the ports of `snappingHelpers.test.ts`
(`line_line_intersection_snap`, `intersection_snap_respects_pixel_radius`,
`point_snap_keeps_priority_at_shared_endpoint`, `stock_intersection_snap`,
`every_kind_but_perpendicular_is_on_by_default`), and
`the_nearest_of_the_highest_rank_wins`, `each_kind_is_found_on_the_fixture`,
`perpendicular_needs_a_reference_point`, `a_kind_turned_off_is_not_offered`,
`nothing_in_reach_takes_the_pointer`, `the_grid_takes_what_nothing_else_does`,
`the_grid_follows_the_zoom_or_the_pin`, `shift_steps_the_angle_by_fifteen_degrees`,
`what_is_and_is_not_a_target`, `centres_of_parametric_shapes_and_flattened_circles`,
`the_radius_is_held_to_its_range` (acc 113); `tests/tool.rs`
`arming_a_tool_makes_it_the_tool_in_use`,
`escape_returns_to_select_and_arming_drops_the_draft`,
`target_editing_keeps_the_drawing_to_itself`,
`the_app_snaps_to_its_project_at_its_zoom`; `dreamcarve_shell::gui_prefs::`
`round_trip`, `snap_and_drawing_default_when_missing`; the window's
`files::prefs_round_trip_through_the_file_record`,
`snap_and_drawing_from_a_file_are_held_to_what_the_app_can_use`.

**Drawing tools (slices 5–7)** `dc_app` `tests/draw.rs`:
`rectangle_by_two_clicks`, `rectangle_by_a_drag`, `shift_squares_alt_centres`,
`rounded_and_chamfered_corners`, `rectangle_by_typed_sizes`,
`the_first_point_and_units_can_be_typed`, `a_refused_size_marks_its_field`,
`circle`, `ellipse`, `polygon_sides_and_flats`, `slot_by_three_clicks`,
`slot_by_typed_sizes`, `a_tool_stays_armed_with_its_options` (the cases of
`stickyDrawing.test.ts`), `too_small_is_not_made_and_construction_is_marked`,
`points_snap_and_sizes_are_shown`, `a_new_shape_joins_the_selections_folder`;
`tests/command.rs` `add_shapes_takes_the_next_ids_and_refuses_what_cannot_be`,
with `AddShapes` in `every_command_round_trips_through_the_file` and
`labels_name_the_thing`; for slice 6, in `tests/draw_path.rs`,
`line_open_and_closed`, `done_double_click_and_escape`,
`backspace_takes_back_a_point`, `arc_segment_through_a_point`,
`arc_by_a_typed_radius`, `line_by_typed_sizes`,
`spline_is_a_path_with_cubics`, `a_line_can_go_on_as_a_curve`; for slice 7, in `tests/draw_gear.rs` (the tool's cases of
`gearCreation.test.ts`), `gear_previews_and_commits`,
`gear_sizes_are_typed`,
`a_size_the_gear_cannot_have_is_refused_at_its_field`,
`gear_steps_back`
(acc 108–112, 114, 129).

**Parametric edits (slice 8)** `dc_app` `tests/shape.rs`: `rows_per_kind`,
`a_size_is_one_undo_step_named_for_it`, `a_choice_is_one_undo_step_too`,
`a_size_a_shape_cannot_have_is_refused`, `tabs_follow_the_outline`,
`linked_copies_follow`,
`sizes_read_as_placed`, `stretching_a_rectangle_changes_its_sizes`,
`a_circle_stretched_is_an_ellipse`, `a_polygon_refuses_a_stretch`,
`a_linked_copy_refuses_a_stretch`, `convert_to_path_keeps_the_outline`;
`command::every_command_round_trips_through_the_file` and
`labels_name_the_thing` gain every new command (acc 115).

**Edit nodes (slices 10–12)** `dc_app` `tests/nodes.rs`: `a_click_selects_a_node`,
`a_box_selects_several`, `dragging_moves_one_vertex`,
`editing_through_a_placed_copy_lands_under_the_pointer` (port:
`editInPlace.test.ts`), `an_arc_handle_changes_the_bulge_only`,
`a_smooth_handle_carries_its_twin_alt_does_not`, `add_and_delete_a_node`,
`break_and_delete_segment`, `dropping_an_end_on_an_end_joins`,
`tabs_follow_an_edited_outline`,
`a_sized_shape_is_converted_and_the_tool_is_left`;
`tests/command.rs` `replace_shapes_hands_targets_and_tabs_to_the_first`;
`dc_geometry::path::edit` `a_path_follows_a_transform`,
`open_paths_join_end_to_end`;
`fillet_and_chamfer_a_corner`, `fillet_corners_of_a_shape`,
`segment_kinds`,
`an_edit_regenerates_only_what_targets_it` (acc 117, 118, 127).

**Changing shapes (slices 13–16)** `dc_app` `tests/edit.rs`: `offset_outward_rounds_corners`,
`offset_inward_past_the_middle_makes_nothing`, `offset_of_an_open_path`,
`weld_subtract_intersect_areas`, `a_result_inherits_the_first_shapes_operations`,
`join_makes_one_path_of_paths_that_meet`; `tests/trim.rs`
`a_click_removes_the_piece_between_crossings`,
`a_circle_opens_and_becomes_a_path`;
`dc_geometry::intersect::` the ports (42), `dc_geometry::trim::`
`an_arm_up_to_the_crossing`, `a_middle_piece_splits_the_line`,
`a_circle_opens_between_two_crossings`, `nothing_crosses_is_reported`,
`a_loop_is_trimmed_at_its_own_crossing`,
`dc_geometry::join::` `ends_within_tolerance_join`, `a_chain_closes`,
`far_ends_are_left_alone`, and the DXF importer's stitch tests unchanged
(acc 119–121).

**Organising (slices 18–22)** `dc_app::command::` `make_unique_clones_the_definition`
(port: `definitionEditing.test.ts`), `clipboard::paste_is_unlinked_and_crosses_projects`,
`array::` the ports of `featureDistribution.test.ts` (grid and radial),
`array::copies_are_linked_and_grouped`, `arrange::align_and_distribute`,
`arrange::top_is_the_greatest_y`, `arrange::to_stock`, `tree::folders_file_rows`,
`tree::rows_reorder_the_file`, `tree::a_group_selects_as_one`,
`command::construction_leaves_every_operation`, `operations::pickers_skip_construction`,
`dc_job::checks::construction_is_not_checked` (acc 122–125).

**Text (slices 23–24)** `dc_text::` `capital_height_is_the_asked_height`,
`counters_nest_as_holes`, `lines_align`, `letter_spacing_adds_per_gap`,
`kerning_pairs_apply`, `layout_is_deterministic`, `every_bundled_font_parses`,
`a_file_that_is_not_a_font_is_refused`, `simple_stroke_is_open_paths`
(port of the glyph table's coverage); `dc_app::tool::text_places_and_edits`,
`fonts::a_missing_font_keeps_the_outline_and_refuses_edits` (acc 116).

**Tracing and measuring (slices 25–26)** `dc_app::pictures::opacity_is_saved_and_cuts_nothing`,
`scale_by_two_points`, `pick::a_locked_picture_is_not_picked`,
`tool::measure::distance_deltas_angle`, `length_and_area_of_a_shape` (acc 126).

**The window (slices 9, 17, 24, 27)** `dreamcarve-app` `tests/drawing.rs`
(slice 9): `the_draw_popover_lists_the_tools`,
`arming_a_tool_shows_its_bar`, `two_clicks_draw_a_rectangle`,
`typed_sizes_commit_on_enter`, `the_snapping_menu_toggles_a_kind`,
`draw_from_scratch_opens_design`, `tool_keys_arm_and_escape_disarms`,
`the_shape_section_edits_a_size`, `the_gear_tool_shows_its_sizes`;
`kittest::` `keyboard_shortcuts_reach_their_commands`,
`file_menu_labels_in_order` and `floating_tools_select_fit_and_zoom`
extended; `review::phase5_drawing_screenshots` writes
`docs/specs/phase-5/review/drawing/`; `tests/editing.rs` (slice 17):
`a_double_click_shows_nodes`,
`a_node_is_dragged_and_a_corner_filleted`, `trim_from_the_popover`,
`offset_and_weld_from_the_shape_menu`, `join_from_the_shape_menu`, and
`review::phase5_editing_screenshots` writing
`docs/specs/phase-5/review/editing/`; later slices
`folders_drag_and_drop`,
`the_text_tool_previews_as_typed` (acc 129, 130); `strings::` and
`theme::` scans as phase 4. `gui_equality::the_hook_plate_drawn_equals_the_cli`
and `drawn_equals_imported` (acc 128).

## Slices

Ordered; brackets are dependencies. Each ends with the gate green and a
row in `docs/phase-5-worklog.md`. Tests first, as phase 4's.

**5.1 Drawing**
1. **Format 29.** The fields, `Shape` (data only), `FolderId`, the
   migration and fixtures, the problems that need no generator, every
   reader updated, the CLI's tests. [—]
2. **Shape outlines.** `dc_geometry::shapes` with the ports;
   `Shape::outline`; `shape_outline_differs` and `invalid_shape`. [1]
3. **Paths.** `dc_geometry::path`: nodes, spans, flattening, the arc
   and spline constructions; `Shape::Path`'s outline. [1]
4. **Tool model and snapping.** `dc_geometry::intersect` (brought
   forward from slice 15, since the Intersection snap reads it);
   `dc_app::tool` (the enum, events, the draft), `snap`, the snap
   grid's spacing, `Prefs` and `gui.toml`. The window's Tab tool flag
   becomes `Tool::Tab` in slice 9, with the rest of the pill. [—]
5. **Box tools and Slot.** `AddShapes`; Rectangle, Circle, Ellipse,
   Polygon, Slot with typed sizes; naming. [2, 4]
6. **Line, Arc, Spline.** The path draft and its modes. [3, 4]
7. **Gear.** [2, 5]
8. **Sizes.** The Shape section's rows, `SetOutline`, sizes as placed,
   the stretch rule inside `TransformInstances`, `ConvertToPath`. [5]
9. **The window, drawing.** The pill and the Draw popover, the tool
   bar, the draft overlay with the snap marker and size labels, the
   Draw and Shape menus, View ▸ Snapping and Grid, the keys, *Draw from
   scratch*, the Shape section. [5–8]

**5.2 Editing**
10. **Edit nodes.** The path edit operations, the node tool, tabs
    re-seated. [3, 8]
11. **Fillet and chamfer.** [10]
12. **Segment kinds.** [10]
13. **Offset.** On `ReplaceShapes`, which slice 10 brought. [8]
14. **Weld, Subtract, Intersect.** [13]
15. **Intersections and Trim.** [10]
16. **Join.** `dc_geometry::join`, the stitch's rule; the importer keeps
    its own. [10]
17. **The window, editing.** Nodes and handles on the overlay, Offset…,
    Fillet Corners… and Join… in the tool bar, the Shape menu. [10–16]

**5.3 Organising**
18. **Make Unique, cut, copy, paste, Alt-drag.** [8]
19. **Arrays.** [18, 21]
20. **Align and distribute.** [—]
21. **Folders and groups.** The model's commands, the tree, the
    window's drag and drop. [1]
22. **Construction geometry.** The flag's commands, the pickers, the
    checks, the renderer's style. [1]

**5.4 Text**
23. **`dc-text`.** The crate, the bundled fonts with their licences,
    Simple Stroke, layout. [3]
24. **The Text tool and user fonts.** `Shape::Text` end to end, the
    fonts folder, the missing-font state, the window. [23, 8, 9]

**5.5 Tracing and measuring**
25. **Backdrops.** Opacity, scale by two points, the locked picture. [4]
26. **Measure and live sizes.** [4, 9]

**5.6 Release**
27. **The hook plate and the book.** The test part through the app,
    `gui_equality`, the quick start, the nine chapters and the updated
    ones, the screenshots, `CONTEXT.md`, the changelog, the review
    pictures, the exit-criteria audit. [1–26]

## Open questions

The functional spec's table holds the product questions. Design
questions still open:

| # | Question | Taken for now |
| --- | --- | --- |
| 1 | The artwork checks run on the window's thread at every shape change (phase 4 design, "dc-job"), which drawing makes frequent. | Left as it is until a drawing makes it felt; then to the worker, as that section says. |
| 2 | A node drag previews changed geometry, which phase 4's transform preview cannot carry. | The draft overlay draws the edited outline; the committed shape is hidden while its draft is drawn. |
| 3 | Booleans return polygons. PureCutCNC keeps source arcs through its union by annotating vertices. | Polygons, as the engine's boolean gives; arc fitting restores arcs at export. Revisit if drawings suffer. |
| 4 | `Shape::Path` stores handles as absolute points; a relative form would survive a hand edit of a node better. | Absolute, as upstream stores them and as kurbo takes them. |
