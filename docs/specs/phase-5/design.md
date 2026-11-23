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
    pub fn transformed(&self, t: Transform) -> Result<Path, GeometryError>;   // arcs under uneven scale become cubics
}
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
    /// A definition's outline replaced: a typed size, a node edit, a fillet, Convert to Path.
    /// Tabs on its instances are re-seated on the new outline.
    SetOutline { definition: DefinitionId, shape: Option<Shape>, profiles: Vec<Polyline>, what: Edit },
    /// A definition's outline replaced and its one instance re-placed: a stretch that changes sizes.
    Reshape { instance: InstanceId, shape: Shape, profiles: Vec<Polyline>, transform: Transform },
    /// Shapes replaced by others: offset without its original, weld, subtract, intersect, trim, join.
    /// Operations and boundaries naming `inherit` name the first new shape instead.
    ReplaceShapes { remove: Vec<InstanceId>, add: Vec<NewShape>, inherit: Option<InstanceId>, what: Edit },
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
pub enum Edit { Size(SizeField), MoveNodes, AddNode, DeleteNodes, Break, DeleteSegment, SegmentKind,
                Fillet, Chamfer, ConvertToPath, Offset, Weld, Subtract, Intersect, Trim, Join, JoinEnds,
                Align(Side), Distribute(Axis), Regenerate }   // the undo label's words
```

`SetOutline` and `Reshape` refuse an outline with a non-finite or
out-of-range vertex and a definition that is not there. Re-seating a tab
is `dc_geometry::nearest_point` on the profile of its index, or on the
nearest profile when that index is gone; a tab is deleted only when its
definition has no profile left.

```rust
// tool.rs: the Viewport's tool in use. Not project data, not in the history.
pub enum Tool { Select, Nodes, Tab, Trim, Measure, Draw(Drawing) }
pub enum Drawing { Line, Arc, Spline, Rectangle, Circle, Ellipse, Polygon, Slot, Gear, Text }
pub struct Pointer { pub at_mm: Point, pub px_per_mm: f64, pub shift: bool, pub alt: bool, pub command: bool }
pub enum Event { Move(Pointer), Press(Pointer), Release(Pointer), DoubleClick(Pointer),
                 Key(Key), Field { field: SizeField, text: String }, Option(ToolOption), Done }
pub enum Key { Enter, Escape, Backspace, Delete, Tab, Letter(char), Nudge { dx_mm: f64, dy_mm: f64 } }
pub struct Reply { pub ran: Option<String>,            // the undo label of what this event committed
                   pub refused: Option<String> }       // a reason for the status bar
impl App {
    pub fn tool(&self) -> &Tool;
    pub fn arm(&mut self, tool: Tool);                 // drops any draft
    pub fn tool_event(&mut self, event: Event) -> Reply;
    /// What the tool wants drawn this frame, in project millimetres.
    pub fn draft(&self) -> &Draft;
    /// The tool bar: the prompt, the options, the fields of the step in hand.
    pub fn tool_bar(&self) -> Option<ToolBar>;
}
pub struct Draft { pub outlines: Vec<Polyline>,        // dashed preview
                   pub nodes: Vec<NodeMark>,           // Edit nodes: anchors, handles, their state
                   pub snap: Option<Snapped>,          // the marker and its guide
                   pub labels: Vec<Label>,             // 83a: a size and where to write it
                   pub highlight: Vec<Polyline> }      // Trim's part under the pointer; Measure's line
pub struct ToolBar { pub tool: Tool, pub prompt: String, pub options: Vec<OptionRow>,
                     pub fields: Vec<SizeRow>, pub can_finish: bool }
pub struct SizeRow { pub field: SizeField, pub text: String, pub pinned: bool, pub problem: Option<String> }
```

`snap`:

```rust
pub enum SnapKind { Endpoint, Midpoint, Centre, Intersection, OnOutline, Perpendicular, Grid }
pub struct SnapSettings { pub enabled: bool, pub kinds: BTreeSet<SnapKind>, pub radius_px: f64 }  // Prefs
pub struct Snapped { pub at_mm: Point, pub kind: Option<SnapKind>, pub guide: Option<(Point, Point)> }
pub fn resolve(scene: &Scene, stock: &Stock, origin: Point, grid: Option<f64>, settings: &SnapSettings,
               pointer: &Pointer, reference: Option<Point>, skip: &[InstanceId]) -> Snapped;
```

`Prefs` gains `snap: SnapSettings`, `grid_spacing_mm: Option<f64>`
(`None`: automatic), and `drawing: DrawingPrefs` (the last drawing
tool, corner style and size, polygon sides and measure, the text
defaults), each mapped to `gui.toml` as `Prefs::tab` is.

`Selection` is unchanged; the nodes selected in Edit nodes are the
tool's state. `Scene::Shape` gains `construction: bool` and the
definition's id; `pick` skips a locked picture.

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
- **Segment kind**: to an arc through the span's point at `t = ½`; to a
  cubic with handles a third of the chord (a line) or the standard
  `4/3·tan(θ/4)·r` handles (an arc, split at 90°); to a line by
  dropping the bulge or handles.
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

The pieces are built by splitting spans at the crossings, so arcs stay
arcs. A path with curves is trimmed on its flattened outline and the
result is a plain path.

### Join

`join::join_open(paths, tolerance_mm) -> Joined { paths, joined: usize,
alone: usize }`: repeatedly take the pair of free ends nearest each
other within the tolerance (ties by input order), reverse a path where
needed, and concatenate, with a line between ends more than `EPS_MM`
apart; a path whose own ends are within the tolerance closes. It is the
DXF importer's stitch rule, moved to `dc-geometry` and called from both.

### Snapping

`snap::resolve` gathers candidates within `radius_px / px_per_mm` of the
pointer, from the flattened outlines the `Scene` already holds for
picking, the stock rectangle and the origin, then takes the best by
(rank, distance): rank 1 endpoint, midpoint, centre, intersection; 2
perpendicular; 3 on outline; 4 grid. A box test on each shape's bounds
skips shapes out of reach, and intersections are sought only between
segments whose boxes hold the pointer's disc, as upstream does. `Centre`
comes from arcs' centres and from `as_circle`; a parametric shape's
instance origin is a centre too, and a slot's two end centres come from
its arcs. `Shift` first projects the pointer onto the nearest ray at a
multiple of 15° from `reference`; candidates are then kept only within
half a pixel of that ray. `command` returns the pointer.

### The tool state machines

Two-point tools (Rectangle, Circle, Ellipse, Polygon) share one
machine: `Idle → First(p1)` on a press's point, then a `Release` more
than 4 px from the press is the second point (a drag), else the next
`Press` is. Slot is `Idle → First → Second → width`. Line, Arc and
Spline share `PathDraft { nodes, mode, pending_arc_end }`. Gear is
`Idle → Centre → Sizes`, the last waiting for **Done**. Every machine
answers `Field` events by pinning that size and recomputing the preview
point from the fields (`manualEntry.ts`'s `computeDimensionEditPreviewPoint`),
and `Key::Enter` by committing with the fields as they read. `Escape`
steps back: an open field, then the draft, then the tool.

A committed shape is `AddShapes` with the shape's outline in definition
space and the instance transform that places it: a translation to the
centre for the box shapes, with a rotation for a Slot (its axis) and a
Polygon (its first corner). A path is saved in project coordinates
under an identity transform, as an import is.

### Sizes as placed, and stretching

Properties shows a size `s` as `s · k`, where `k = √|det|` of the
instance's linear part, when that part is a similarity; typing `v`
writes `v / k`. A linear part that is not a similarity (a stretched
polygon of lines) shows the sizes read-only with *Stretched: convert to
a path to edit it*.

`TransformInstances` with a delta that scales X and Y differently asks
each parametric instance to absorb it (`shape::stretch`): when the
instance's rotation is a multiple of 90° within 1e-9 and its definition
is used by no instance outside the command, the delta becomes new sizes
(Rectangle, Ellipse and Slot as the functional spec says; Circle to
Ellipse) and a transform with the scale taken out, as `Reshape`. A
shape that cannot absorb it follows phase 4's rule: lines stretch
through the transform; arcs are refused, now with the reason.

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
crates/dc-app/src/{tool.rs, tool/{draw.rs (box shapes, slot, gear), path.rs (line, arc, spline), text.rs,
                   node.rs, trim.rs, measure.rs, fields.rs (typed sizes), bar.rs}, snap.rs, shape.rs (the Shape
                   section, sizes as placed, stretch), edit.rs (offset, weld, subtract, intersect, join, fillet
                   corners as commands), array.rs (port), fonts.rs (the fonts on hand), clipboard.rs,
                   command/{shapes.rs, folders.rs}, arrange.rs (align, distribute), tree.rs (folders)}
crates/dreamcarve-shell/src/fonts.rs                     — the user's fonts folder: list, add
crates/dreamcarve-app/src/{viewport/{tools.rs (events to dc-app), tool_bar.rs, draw_menu.rs, draft.rs (the
                           overlay: preview, nodes, snap marker, labels)}, panels/{shape.rs (the Shape section),
                           offset.rs, array.rs (popovers)}, shell/menu.rs (Draw, Shape), keyboard.rs}
docs/book/src/{quick-start/first-design.md, user-guide/{drawing-shapes.md, snapping.md, text.md, editing-nodes.md,
               changing-shapes.md, copies-and-arrays.md, folders-and-construction.md, tracing.md, measuring.md}}
```

`dc-app`'s manifest gains `dc-text`; `dreamcarve-app`'s gains nothing.
The Tab tool's armed flag moves from egui's memory to `Tool::Tab`.

## Visual design

Phase 4's tokens and components hold. Added to the palette, in both
themes, with the spec tables' tests extended: `viewport.construction`
(the dashed construction outline), `viewport.draft` (a shape being
drawn; the accent), `viewport.node` and `viewport.node_selected`,
`viewport.handle` (a curve's handle and its stem), `viewport.snap`
(the marker and its label), `viewport.measure`. The renderer gains two
styles, `Construction` and `ConstructionSelected`; the draft, the
nodes, the snap marker and the size labels are egui painter overlays,
as phase 4's handles are. A node is a 9 px square, a handle a 7 px
circle, hit within 9 px and 7 px (upstream's radii). The snap marker is
a square (endpoint), a triangle (midpoint), a circle with a cross
(centre), a cross (intersection), a circle on the line (on outline), a
right-angle mark (perpendicular) or a small plus (grid). Drawing-tool
icons are Lucide's where one reads as the shape (`Square`, `Circle`,
`Spline`, `Type`, `Slash`, `Hexagon`, `Cog`, `Ruler`, `Scissors`), and
painter-drawn glyphs for Slot, Arc and Ellipse, as the operation kinds
are.

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
`profileEdit.test.ts` (10); `a_polyline_round_trips_through_a_path`,
`arc_through_three_points_bulges_toward_the_third`,
`collinear_points_make_no_arc`, `a_spline_passes_through_its_points`,
`flattened_cubics_stay_within_tolerance`,
`splitting_a_span_keeps_the_outline` (property), and for slice 12
`make_arc_keeps_the_middle_point`, `make_curve_then_straight_is_the_chord`.

**Tool model and snapping (slice 4)** `dc_app::snap::` the ports of
`snappingHelpers.test.ts` (5); `the_nearest_of_the_highest_rank_wins`,
`each_kind_is_found_on_the_fixture`, `a_kind_turned_off_is_not_offered`,
`nothing_in_reach_takes_the_pointer`, `command_suspends`,
`shift_steps_the_angle_by_fifteen_degrees`,
`the_grid_snaps_to_what_is_shown`, `a_dragged_shape_is_not_its_own_target`,
`hidden_shapes_do_not_snap_construction_does` (acc 113);
`tool::arming_drops_the_draft`, `tool::escape_steps_back`,
`prefs::snap_settings_round_trip` (in `dreamcarve-shell` too).

**Drawing tools (slices 5–7)** `dc_app::tool::` `rectangle_by_two_clicks`,
`rectangle_by_a_drag`, `shift_squares_alt_centres`,
`rounded_and_chamfered_corners`, `circle`, `ellipse`, `polygon_sides_and_flats`,
`slot_by_three_clicks`, `slot_by_typed_width`, `gear_previews_and_commits`,
`line_open_and_closed`, `backspace_takes_back_a_point`,
`arc_segment_through_a_point`, `spline_is_a_path_with_cubics`,
`a_tool_stays_armed_with_its_options` (port of `stickyDrawing.test.ts`'s
cases), `a_shape_too_small_is_not_made`, `new_shapes_are_named_and_numbered`,
`typed_sizes_per_tool` (table-driven over capability 83),
`a_pinned_size_leaves_the_rest_to_the_pointer`, `a_refused_size_marks_its_field`
(acc 108–112, 114, 129).

**Parametric edits (slice 8)** `dc_app::shape::` `rows_per_kind`,
`a_size_is_one_undo_step_named_for_it`, `linked_copies_follow`,
`sizes_read_as_placed`, `stretching_a_rectangle_changes_its_sizes`,
`a_circle_stretched_is_an_ellipse`, `a_polygon_refuses_a_stretch`,
`a_linked_copy_refuses_a_stretch`, `convert_to_path_keeps_the_outline`;
`command::every_command_round_trips_through_the_file` and
`labels_name_the_thing` gain every new command (acc 115).

**Edit nodes (slices 10–12)** `dc_app::tool::node::` `a_click_selects_a_node`,
`a_box_selects_several`, `dragging_moves_one_vertex`,
`editing_through_a_placed_copy_lands_under_the_pointer` (port:
`editInPlace.test.ts`), `an_arc_handle_changes_the_bulge_only`,
`a_smooth_handle_carries_its_twin_alt_does_not`, `add_and_delete_a_node`,
`break_and_delete_segment`, `dropping_an_end_on_an_end_joins`,
`fillet_and_chamfer_a_corner`, `fillet_corners_of_a_shape`,
`segment_kinds`, `tabs_follow_an_edited_outline`,
`an_edit_regenerates_only_what_targets_it` (acc 117, 118, 127).

**Changing shapes (slices 13–16)** `dc_app::edit::` `offset_outward_rounds_corners`,
`offset_inward_past_the_middle_makes_nothing`, `offset_of_an_open_path`,
`weld_subtract_intersect_areas`, `a_result_inherits_the_first_shapes_operations`,
`dc_geometry::intersect::` the ports (42), `dc_geometry::trim::`
`an_arm_up_to_the_crossing`, `a_middle_piece_splits_the_line`,
`a_circle_opens_between_two_crossings`, `nothing_crosses_is_reported`,
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

**The window (slices 9, 17, 24, 27)** `kittest::` `the_draw_popover_lists_the_tools`,
`arming_a_tool_shows_its_bar`, `two_clicks_draw_a_rectangle`,
`typed_sizes_commit_on_enter`, `a_double_click_shows_nodes`,
`the_snapping_menu_toggles_a_kind`, `draw_from_scratch_opens_design`,
`tool_keys_arm_and_escape_disarms`, `folders_drag_and_drop`,
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
4. **Tool model and snapping.** `dc_app::tool` (the enum, events, the
   draft, the bar), `snap`, the grid's snap spacing, `Prefs` and
   `gui.toml`. The Tab tool becomes `Tool::Tab`. [—]
5. **Box tools and Slot.** `AddShapes`; Rectangle, Circle, Ellipse,
   Polygon, Slot with typed sizes; naming. [2, 4]
6. **Line, Arc, Spline.** The path draft and its modes. [3, 4]
7. **Gear.** [2, 5]
8. **Sizes.** The Shape section's rows, `SetOutline`, sizes as placed,
   `Reshape` and the stretch rule, Convert to Path. [5]
9. **The window, drawing.** The pill and the Draw popover, the tool
   bar, the draft overlay with the snap marker and size labels, the
   Draw and Shape menus, View ▸ Snapping and Grid, the keys, *Draw from
   scratch*, the Shape section. [5–8]

**5.2 Editing**
10. **Edit nodes.** The path edit operations, the node tool, tabs
    re-seated. [3, 8]
11. **Fillet and chamfer.** [10]
12. **Segment kinds.** [10]
13. **Offset.** `ReplaceShapes`. [8]
14. **Weld, Subtract, Intersect.** [13]
15. **Intersections and Trim.** [10]
16. **Join.** The stitch moved to `dc-geometry`. [10]
17. **The window, editing.** Nodes and handles on the overlay, the
    popovers, the menus. [10–16]

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
