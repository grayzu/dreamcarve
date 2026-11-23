# Phase 5 worklog

The local record of phase 5 (GUI, CAD), started 2026-11-22. It stands in
for GitHub issues and pull requests, as `docs/alpha-worklog.md` did for
phases 3 and 4. One line per slice; details live in the commits and in
`docs/specs/phase-5/`.

## Process

Asked by the maintainer on 2026-11-22: the approach of the alpha push,
for phase 5.

- All work is on the branch `Phase-5`, cut from `main` at `5f75ce9b` and
  pushed to GitHub as a backup. No pull requests, no issues, no CI waits.
  It merges to `main` only when the whole phase is done.
- The branch is version `0.5.0-alpha.1`.
- Spec first: `functional.md`, `design.md` and ADR-0006 were written
  before any code, by the agent, for the maintainer to review with the
  working build. A spec changes in the commit that changes what it
  describes.
- Test first. The gate before a slice is recorded as done, on macOS:
  `cargo fmt --all -- --check`, `cargo clippy --all-targets
  --all-features -- -D warnings`, `cargo nextest run --workspace` with
  `cargo test --workspace --doc`, and `cargo run -p check-headers`.
- Windows and Linux are tested by CI when the branch reaches `main`. The
  code stays portable meanwhile: no macOS-only API without a `cfg`, no
  path or line-ending assumptions. Before that merge, the checks CI runs
  and the local gate does not are run here too (`cargo doc`, `cargo
  deny`, `mdbook build`, the licence and dist checks, clippy for the
  Windows and Linux targets).
- The maintainer's own steps wait for them: the review of the build, the
  bench check, the decisions listed below, and the tags.

Reference checkouts, beside this worktree: `../purecutcnc` at `d04ae2f`
and `../graphite` at `615b4050` (sparse). Their surveys are
`docs/research/purecutcnc-drawing-digest.md` and
`docs/research/graphite-tools-digest.md`.

## Where it stands

Baseline before the first slice: 3,430 tests pass on macOS at
`0.5.0-alpha.1`. After slice 16: 3,641.

## Decisions waiting for the maintainer

The functional spec's "Open questions" (ten) and the design spec's
(four). Each has a choice taken so the work can proceed; none blocks.
Decisions made while building are added here.

- Slice 1: a size that cannot be is `out_of_range` at the shape's own
  field, not a new `invalid_shape` code, so the app marks the field as it
  marks any other; a bitmap's opacity the same. The design spec says so
  now.
- Slice 2: a gear's sizes are checked, not clamped as PureCutCNC clamps
  them before it validates; an ellipse is drawn as chords of the true
  curve (its four axis points among the vertices), not as PureCutCNC's
  four Béziers flattened; a rounded corner of exactly half the shorter
  side is allowed, the side between the two arcs left out, where
  upstream stops a micrometre short.
- Slice 4, snapping, where it leaves PureCutCNC: a click with no snap
  is taken at the pointer (upstream discards it); the grid has no reach,
  so with grid snap on a point with nothing better near lands on the
  grid (Graphite's rule); the snap grid's spacing follows the zoom in
  steps of 1, 2 and 5, at least 8 px apart, or is pinned by the user,
  while the grid phase 4 draws (1, 10, 100 mm) stays as it is; `Shift`
  holds a direction to 15° steps and `Ctrl`/`⌘` suspends snapping
  (upstream cycles an axis lock on an `Alt` tap); Intersection is on by
  default (upstream: opt-in); a shape's real outline snaps, not its
  frame.
- Slice 5: on Rectangle and Ellipse `Shift` squares the box, so it
  does not also hold the corner to a 15° direction; a polygon measured
  to its flats takes the click as the middle of a side, not a corner; a
  slot whose width is typed with its length and angle is made at once,
  as PureCutCNC makes it.
- Slice 6: Line, Arc and Spline are one path's three kinds of segment,
  so changing among them keeps the path being drawn (upstream's
  composite tool; its separate polygon and spline tools are folded in).
  A path is named *Line*, *Arc* or *Spline* when its segments are all
  of that kind, else *Path*. A double-click finishes any of the three
  (upstream: a polygon only). A typed radius bows the arc to the
  pointer's side and is the short way round; an arc past half a turn
  is drawn through a point. `Enter` takes the typed point while a
  length or an angle is typed, and finishes the path when none is. A
  closed path is saved in the direction it was drawn, not turned
  counter-clockwise as the sized shapes are.
- Slice 7: a gear's size is held as it was typed and the reason said at
  its field, the gear not made while one stands (upstream's way); a
  typed size is not thrown back. A radius click on the centre is not
  taken (upstream raises it to the least size). The reasons that name a
  limit give it in project units; the rest are `dc-geometry`'s words.
  The module is shown in project units in an inch project too; a
  diametral pitch is not offered.
- Slice 8: the design named a `Reshape` command for a stretch that
  changes sizes; it is done inside `TransformInstances` instead, so a
  stretch of several shapes is still one undo step and every caller
  (Properties, the handles) gets it. `SetOutline` names the instance,
  not the definition, so its undo step can name the copy that was
  edited. Stretching a polygon or a gear is refused outright, as the
  functional spec says; the design's "lines stretch through the
  transform" now holds only for shapes that keep no sizes. A stretched
  circle keeps the name *Circle 1*. A locked linked copy makes the
  sizes of all its copies read-only. Convert to Path is allowed on a
  locked shape, since nothing moves.
- Slice 9: the tool bar floats under the pills, not across the top as
  the target-editing bar does, and breaks into rows when it is wide
  (the Gear's). Fit leaves room for two rows of it while a drawing tool
  is armed, so the view refits when a tool is taken up or put down
  (phase 4's target bar deliberately does not). The Inspector stays
  closed while a drawing tool is armed. The Snapping switch is a row of
  the View menu and the kinds are under *Snap To*, where the spec had
  both under one submenu: a shortcut's key must show in its menu's own
  row. The zoom percentage now goes before the zoom buttons in a
  narrow Viewport; at 1280 wide with an Inspector open the percentage
  is hidden. Icons are all Lucide's (it has an ellipse and a pill); the
  Arc's is `radius`, which may deserve a drawn glyph. No palette token
  was added: the draft is `accent`, the snap marker
  `viewport.shape.hover`. The Construction switch is left for slice 22
  with the rest of construction geometry. The pill's Edit
  nodes, Trim and Measure buttons come with their tools. The diameter
  sign in size labels is `Ø`: the bundled Inter has no `⌀`.
- Slice 10: the selected nodes and the edited shape are the tool's,
  the shape being the app's one selected instance; there is no edit
  session to apply or cancel (upstream's), each edit being its own undo
  step. Breaking an open path makes two shapes only when the shape is
  one path of its own with no linked copy; otherwise the definition
  gains an outline. The context menu rows and the `B` key are the
  window's (slice 17); the app core takes them as `NodeAction`s.
- Slices 11 and 12: a fillet's radius is typed (the bar's corner size),
  not dragged out from the corner as upstream also allows. One size
  serves Fillet, Chamfer and the Rectangle tool's corners. Fillet
  Corners takes one shape at a time: several would need several
  definitions changed in one undo step, which no command does yet.
- Slices 13 and 14: the design's `ReplaceShapes { remove, inherit:
  Option }` became `{ from, keep, inherit: Vec<(shape, index)> }`, so
  an offset of several shapes without their originals hands each one's
  operations to its own result, an offset that keeps its originals is
  still named for them, and its results stand beside them. For an open
  path *Outward* is the left of its direction and *Inward* the right;
  the window is to label them Left and Right.
- Slices 15 and 16: the DXF importer's stitch was not moved to
  `dc-geometry` as the design said. It is 1,400 lines built for files of
  thousands of entities (a grid, a work budget, hatches, duplicates) and
  it moves a joined piece by the gap, where Join bridges the gap; a
  second, small implementation of the same rule serves a selection, and
  the importer and its golden corpus are untouched. Trim treats an
  outline's own end that touches another outline as no crossing.

## Slices

Status is one of: todo, doing, done, owner (waits for the maintainer).
The numbers are the design spec's.

### 5.1 Drawing

| Slice | What | Status |
| --- | --- | --- |
| 0 | The specs: `functional.md` (capabilities 81–106, acceptance 107–130, S21, the hook plate), `design.md` (27 slices), ADR-0006; `NOTES.md` folded in; the roadmap's phase 5 brought up to date | done |
| 1 | Format 29: a definition's `shape` (`dc_project::Shape`, eight kinds, data only), `dc_geometry::Path` (nodes with line, arc and cubic spans; exact from a polyline, flattened to one), an instance's `construction` and `folder`, `folders`, a bitmap's `opacity_fraction`; `v28_to_v29`; fixtures `v29/sample.dcv` and `drawing.dcv`; the problems `construction_targeted`, `duplicate_folder_id`, `dangling_folder`, and `out_of_range` at a shape's size or a bitmap's opacity; a path's nodes one per line in a saved file; the quick-start projects and the book's Project file, Images and first-cut chapters at 29. No program changed: a version 28 project exports byte for byte. Gate: 3,449 tests | done |
| 2 | Shape outlines: `dc_geometry::shapes` (rectangle, rounded and chamfered, circle, ellipse, regular polygon to corners or flats, slot) with PureCutCNC's slot, n-gon and canned-rectangle tests ported, and `shapes::gear`, `gearProfile.ts` ported with its tests; every outline counter-clockwise, which upstream's are not as a set; `Shape::outline` and `outline_matches`; the warning `shape_outline_differs`; a gear's sizes checked together as `out_of_range` at the size at fault; the drawing fixture now holds every kind but text. Two properties: an outline is simple and its box is its sizes; a gear is simple and symmetric. Gate: 3,490 tests | done |
| 3 | Paths: `dc_geometry::path` edits a path node by node, `profileEdit.ts` ported with its tests on nodes that each hold the span leaving them: the arc through three points (`bulge_through`), a spline through points (Catmull-Rom, closed or open), split a span, delete a node, break, delete a span, fillet and chamfer a corner with the reason when one is refused, change a span's kind, move a node (an arc beside it rebuilt through its old middle), move a handle (smooth or alone), reshape an arc by its middle or its radius, reverse, close. This is the path geometry of slices 10 to 12 as well; those add the tool. Three properties. Gate: 3,517 tests | done |
| 4 | The tool model and snapping, in the app core: `dc_app::tool` (the tool in use, a drawing tool the Draw button holds, events, the draft the window paints), `dc_app::snap` ported from PureCutCNC's `resolveSketchSnap` with its tests (endpoint, midpoint, centre, intersection, on outline, perpendicular, grid; rank then distance), `dc_geometry::intersect` ported from `segmentIntersection.ts` (brought forward from slice 15: the Intersection snap reads it), a scene shape's centre and construction flag, and the snapping and drawing preferences in `gui.toml` (`[snap]`, `[drawing]`). No window yet: that is slice 9. Gate: 3,550 tests | done |
| 5 | Rectangle, Circle, Ellipse, Polygon and Slot, in the app core: `Command::AddShapes`; the tools' state machine (`dc_app::tool::draw`) with PureCutCNC's click sequences, a drag as two points, `Shift` for a square and `Alt` from the centre, every step typed (the first point as X and Y from the origin, then the sizes; a typed size is pinned and the rest follows the pointer), a refused size marked at its field, the tool bar's prompt and fields as data, the size label, sticky tools, names by the next free number, the Construction switch, the selection's folder; the tools' options in `gui.toml`. Gate: 3,569 tests | done |
| 6 | Line, Arc and Spline, in the app core (`dc_app::tool::path`): one path drawn a point at a time, each span a line, an arc (its end, then a point it passes through, or a typed radius) or a curve (Catmull-Rom through its points); changing among the three keeps the path; a click on the first point closes, `Enter`, Done or a double-click finishes open, `Backspace` takes back a point; the next point by typed length and angle; lines and arcs saved as their outline, a path with a curve as `Shape::Path` and its flattened outline; three points in a line refused with the reason; the tool bar's `can_finish`. Gate: 3,577 tests | done |
| 7 | The Gear tool, in the app core (`dc_app::tool::gear`): the centre, the outside radius, then the sizes in the tool bar (teeth, outside diameter, pressure angle, whole depth, root fillet, crest radius, bore; flanks, root and crest as choices; the module as a note), previewed, until Done; what the gear cannot be said at the field at fault; the bore a second outline; `Shape::from_gear`; `ToolBar::choices` and `note`, `Quantity::Count`. Gate: 3,581 tests | done |
| 8 | Sizes (`dc_app::shape`): the Properties Shape section as data (each kind's sizes as placed, its choices, a gear's module), a typed size or a choice as one `SetOutline` named for it, held and refused as each kind needs, linked copies following; a stretch taken as new sizes by a rectangle, an ellipse, a slot and a circle inside `TransformInstances`, refused by a polygon, a gear, a turned shape and a linked copy; `ConvertToPath`; tabs re-seated on a changed outline. Gate: 3,595 tests | done |
| 9 | The window, drawing: the pill's Draw button and its popover, the tool bar (prompt, choices, typed sizes, Done; wraps into rows), the draft overlay (preview, size labels, snap marker and guide), the tools' keys and Enter, Escape, Backspace through to the tool, the Draw and Shape menus, View ▸ Snapping, Snap To and Grid, *Draw from scratch*, the Properties Shape section, the prompt in the status bar; the Rectangle's corners and corner size and the Polygon's sides and measure in the bar; the Tab tool is `Tool::Tab`; help on every size and choice; a font-coverage test over the strings table; review pictures in `docs/specs/phase-5/review/drawing/`. Gate: 3,608 tests | done |

### 5.2 Editing

| Slice | What | Status |
| --- | --- | --- |
| 10 | Edit nodes, in the app core (`dc_app::tool::node`): the one selected path's nodes, handles and arc middles; select by click, Shift and a box; drag a node, several, a straight side, a curve's handle (Alt for a corner), an arc's middle, each previewed and one undo step, through the instance's inverse transform so linked copies follow; a node's X and Y and an arc's radius typed; add a node (double-click, Ctrl-click), delete nodes or a segment, Break; an end dropped on an end closes the path or joins two shapes; a sized shape converted first; `ReplaceShapes`; `Path::transformed` and `Path::joined`. The window's side is slice 17. Gate: 3,622 tests | done |
| 11 | Fillet and chamfer, in the app core: the Edit nodes bar's *Fillet* and *Chamfer* on the selected corners by its corner size, those that cannot take it left and named; `dc_app::edit::fillet_corners` for Shape ▸ Fillet Corners… | done |
| 12 | Segment kinds, in the app core: the selected segment made straight, an arc or a curve from the bar. Gate: 3,625 tests | done |
| 13 | Offset, in the app core (`dc_app::edit::offset`, `offset_outlines` for the preview): each shape by itself, outward, inward or both, holes the other way, open paths to a side; the original kept or replaced with its operations handed on; `ReplaceShapes` keeps its sources or hands each one's operations and tabs to its own result | done |
| 14 | Weld, Subtract, Intersect, in the app core (`dc_app::edit::boolean`): the selection in order, one path named for the first, placed as it is, with what cut it. Gate: 3,630 tests | done |
| 15 | Trim: `dc_geometry::trim` (the piece between the crossings either side of a point, on the outline's own segments, with itself too) and the Trim tool in the app core (`dc_app::tool::trim`): the piece highlighted, a click taking it out, a sized shape made a path in the same step | done |
| 16 | Join: `dc_geometry::join_open` (chains of open outlines whose ends are within a tolerance, gaps bridged, a chain that meets itself closed) and `dc_app::edit::join`. Gate: 3,641 tests | done |
| 17 | The window, editing | todo |

### 5.3 Organising

| Slice | What | Status |
| --- | --- | --- |
| 18 | Make Unique, cut, copy, paste, Alt-drag | todo |
| 19 | Arrays | todo |
| 20 | Align and distribute | todo |
| 21 | Folders and groups | todo |
| 22 | Construction geometry | todo |

### 5.4 Text

| Slice | What | Status |
| --- | --- | --- |
| 23 | `dc-text`: fonts, layout, Simple Stroke, the bundled fonts | todo |
| 24 | The Text tool and user fonts | todo |

### 5.5 Tracing and measuring

| Slice | What | Status |
| --- | --- | --- |
| 25 | Backdrops: opacity, scale by two points | todo |
| 26 | Measure and live sizes | todo |

### 5.6 Release

| Slice | What | Status |
| --- | --- | --- |
| 27 | The hook plate, the quick start, the chapters, the review pictures, the audit | todo |
