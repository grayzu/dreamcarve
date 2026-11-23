# Graphite vector-editing tools: behaviour digest for the DreamCarve phase 5 spec

Read-only research. Behaviour is described precisely enough to re-implement in a
different architecture; Graphite's message passing and node-graph plumbing are
left out on purpose.

## 0. Provenance, licence, method

### 0.1 Checkout

- Path: `/Users/markg/orca/workspaces/dreamcarve/graphite` (blobless, sparse clone of `https://github.com/GraphiteEditor/Graphite.git`)
- Commit: `615b4050ba4d247712dfd2fca745d87a9bba315f`
- Commit date and subject: 2026-11-22 23:22:03 +0000, "Fix read-only subgraphs offering edits, and add drag toggling to graph layer buttons (#4999)"
- All `path:line` references below are relative to the Graphite root at that commit.

Sparse-checkout paths added during this research (permitted by the brief; no
tracked file was modified, `git status` is clean):

- `node-graph/libraries/vector-types`
- `editor/src/messages/input_mapper`
- `editor/src/messages/portfolio/document/utility_types`
- `editor/src/messages/portfolio/document/overlays`

A few files outside the sparse set were read with `git show HEAD:<path>`
(`node-graph/nodes/vector/src/generator_nodes.rs`, `vector_nodes.rs`,
`editor/src/messages/preferences/utility_types.rs`,
`editor/src/messages/portfolio/document/document_message_handler.rs`); those
citations are marked where they occur. One `git grep ... HEAD` over the
blobless clone was started and stopped after it timed out; it may have fetched
extra blobs into the object store but changed nothing in the working tree.

### 0.2 Licence and attribution facts

- Workspace licence: `license = "MIT OR Apache-2.0"` (`Cargo.toml:74`); every crate including `libraries/snapping` inherits it (`libraries/snapping/Cargo.toml:5`, `license.workspace = true`).
- `LICENSE-MIT:1`, the only filled-in copyright line in the repository root: `Copyright (c) Graphite contributors`
- `LICENSE-APACHE` carries only the unfilled appendix boilerplate, `Copyright [yyyy] [name of copyright owner]` (`LICENSE-APACHE:189`). It has no Graphite-specific copyright line and no year.
- `Cargo.toml:71`: `authors = ["Graphite Authors <contact@graphite.art>"]`
- Source files carry no per-file copyright or SPDX header (grep of `libraries/snapping/src` and `pen_tool.rs` for `copyright|SPDX` finds nothing).
- There is no `NOTICE` file at the Graphite root (root listing: `Cargo.toml`, `LICENSE-APACHE`, `LICENSE-MIT`, `README.md`, `about.toml`, `deny.toml`, ...).

So an attribution header for a file ported from Graphite has to take its
copyright line from `LICENSE-MIT` ("Copyright (c) Graphite contributors", no
year given upstream) and name the source path and the commit above. Which
licence DreamCarve takes the code under (Apache-2.0 of the dual pair) is a
project decision, not something this digest settles.

### 0.3 What is and is not in the checkout

- The brief expected a `snapping/` folder or `snap_session.rs` under `common_functionality/`. This commit has `snap_session.rs` plus `snap_session/{presenter,scene_mirror,settings}.rs`, and the engine itself is a standalone crate, `libraries/snapping` (glam + kurbo only). The older `SnapManager` / `SnapSource` / `SnapTarget` design is gone.
- `libraries/` contains exactly: `control-protocol`, `dyn-any`, `ipsum`, `math-parser`, `rawkit`, `snapping`, `wgpu-sync`. There is **no `bezier-rs`** and **no `path-bool`** under `libraries/`. `path-bool` is at `node-graph/nodes/path-bool` (not read). Curve math is kurbo 0.13 (`Cargo.toml:214`), plus `lyon_geom = "1.0"` (`Cargo.toml:265`) and `linesweeper = "0.5"` (`Cargo.toml:288`).
- Bezier-rs-style helpers now live in `node-graph/libraries/vector-types/src/vector/algorithms/` and, for snapping, `libraries/snapping/src/curve.rs`.

### 0.4 Method and reliability

- Five parallel passes, one per group of areas, each reading the primary files in full (tool code; test bodies mostly by name only) and following calls into helpers. Each claim group is tagged **[read]** (taken from code or doc comments at the cited lines) or **[inferred]** (reasoning from that code, never executed).
- Nothing was built or run. No behaviour was confirmed in the running app.
- After the passes, about 40 quoted constants and line references were re-checked against the source by a separate read (tunables, `Kind` order, FSM enums, angle step, fit constants, grid constants, cage thresholds, shape constants, pen handle mirror, freehand insertion, measure constants). All matched. One contradiction between passes was found and corrected: `fit_handles_to_segment_pair` is used only when an anchor is slid along its path (`path_tool.rs:1419`), not when a point is deleted.
- Each section ends with its own "NOT present" and "not verified" lists; section 9 collects the open items.

### 0.5 Corrections to assumptions in the brief

| Assumed | What the code does |
|---|---|
| Shift = 15° angle lock | The step is a per-document setting, `ANGLE_STEP_DEFAULT = 15.`, clamped `ANGLE_STEP_MIN = 1.` to `ANGLE_STEP_MAX = 90.` (`editor/src/messages/portfolio/document/utility_types/misc.rs:11-15`). Pen, path handles, line shape, cage rotation and modal rotate all use it. Hints still say "15°". |
| Pen: Alt breaks the handle | Alt only switches between locked-length and equidistant colinear handles. **C** toggles colinear/free (cusp). |
| Pen: `CREATE_CURVE_THRESHOLD` separates click from drag | Defined (`editor/src/consts.rs:156`) but unused. Click vs drag is `DRAG_THRESHOLD = 1.` px (`editor/src/consts.rs:48`), applied in the snap session. |
| Snap tolerance is `SNAP_POINT_TOLERANCE` | That constant (`5.`, `editor/src/consts.rs:46`) is only the pen/freehand endpoint proximity test. Snapping uses `reach: 8.`, `release: 12.`, `switch_margin: 2.` screen px (`libraries/snapping/src/tunables.rs:26-28`). |
| Path intersections are a snap kind | They are not a `Kind`; they are a solver crossing of any two tracks. |
| Deleting a point refits the curve | It does not. The neighbours are reconnected with their outer handles unchanged. |
| Segment molding keeps the grabbed point under the cursor | It translates both control points by the raw pointer delta, so the grabbed point moves by `3t(1-t)·delta` [inferred]. |
| Transform cage is axis-aligned | It is a box in the local frame of the first selected layer; multi-selections are unioned in that frame. |
| Skew needs Ctrl | Skew starts by dragging the small triangles beside an edge midpoint, no modifier; Ctrl changes the constraint during the drag. |
| Double-click with Select enters node editing | It drills into groups or edits text. Tab toggles Select and Path. |
| Freehand fits curves | One straight segment per pointer-move event; no fitting or simplification. |
| Distribution buttons in the Select options bar | Six align buttons, flips and booleans exist; no distribute command. Equal spacing exists only as a snap (`libraries/snapping/src/spacing.rs`). |

### 0.6 Cross-cutting notes for a Y-up, arc-carrying port

- Everything in the editor layer is viewport or document space with **Y down**: "top" is `min.y`, positive angles turn clockwise on screen, ArrowUp is negative Y, a polygon's first vertex sits at −π/2. Re-derive each of these (AGENTS.md, ADR-0001); do not copy signs.
- Every Graphite segment is a line, quadratic or cubic Bezier. Nothing in the tools, the snapping crate or vector-types creates, keeps, fits or snaps to a true circular arc. Circles and arcs are cubic approximations (`HANDLE_OFFSET_FACTOR = 0.551784777779014`, at most 45° per cubic for arcs). There is no Bezier-to-arc or biarc fitting to port; DreamCarve must design that itself.
- Pixel tolerances are screen pixels applied through the viewport transform, with two exceptions that look like upstream slips and should not be copied: the pen's Ctrl lock radius (`20.` document units, `pen_tool.rs:1188, 1203`) and the path tool's `10.` neighbour test (`path_tool.rs:3485`, layer space) [inferred].
- Nudges are in document units (1 and 10), not zoom-scaled; with grid snapping on they land on the next grid line instead.

### 0.7 Section map

1. Pen tool
2. Path (node-editing) tool
3. Snapping system and grid
4. Select tool, transform cage and pivot
5. Measure overlay
6. Shape tool
7. Spline, freehand and text tools
8. Geometry helpers worth porting
9. Open items and unverified claims

---

## 1. Pen tool

Source: `editor/src/messages/tool/tool_messages/pen_tool.rs` (3,827 lines; lines 1-2499 are the tool, 2501-3827 are tests). Key bindings: `editor/src/messages/input_mapper/input_mappings.rs:277-290`. Paths below are relative to the Graphite root. Tags: **[read]** = taken from the code or its doc comments, **[inferred]** = my reading of how the pieces combine, not executed.

### 1.1 Constants used

| Constant | Value | Where defined | Where used by the pen |
|---|---|---|---|
| `SNAP_POINT_TOLERANCE` | `5.` (viewport px) | `editor/src/consts.rs:46` | every anchor/segment hit test: `pen_tool.rs:806, 833, 860, 917, 1294, 1381, 1535, 1794, 1918, 1941, 2030` |
| `SEGMENT_OVERLAY_SIZE` | `10.` | `editor/src/consts.rs:113` | half-length of the insert-on-segment tick, `pen_tool.rs:1803, 1924` |
| `HIDE_HANDLE_DISTANCE` | `3.` | `editor/src/consts.rs:111` | handle dots hidden when closer than this to their anchor (viewport px), `pen_tool.rs:1811` |
| `ANGLE_STEP_DEFAULT` | `15.` (degrees) | `editor/src/messages/portfolio/document/utility_types/misc.rs:11` | Shift angle step via `snapping_state.angle_step_radians()`, `pen_tool.rs:1320` |
| `ANGLE_STEP_MIN` / `ANGLE_STEP_MAX` | `1.` / `90.` | `.../utility_types/misc.rs:13, 15` | clamp of the per-document step (`misc.rs:103-107`) |
| literal `20.` | document units, not px | `pen_tool.rs:1188, 1203` | Ctrl lock-angle "near the anchor" radius |
| literal `1e-6` | | `pen_tool.rs:1049, 1261`; `utility_functions.rs:731` | zero-length handle test; colinearity tolerance (radians) |
| `f64::EPSILON * 100.` | | `pen_tool.rs:1318, 1347` | minimum squared length before an angle is taken |
| dashed guide | dash `4.`, gap `4.`, offset `0.5` | `pen_tool.rs:1873, 1878` | angle-lock / angle-snap guide line |
| `DRAG_THRESHOLD` | `1.` (viewport px) | `editor/src/consts.rs:48` | not referenced in `pen_tool.rs`; applied inside the snap session (`common_functionality/snap_session.rs:602-606`) |
| `CREATE_CURVE_THRESHOLD` | `5.` | `editor/src/consts.rs:156` | **unused**: no reference anywhere in the checked-out tree (`editor/`, `libraries/`, `node-graph/libraries/vector-types`). Dead constant under the `// PEN TOOL` heading. **[read, by grep of the sparse checkout only]** |

The Shift step is a per-document setting (`SnappingState.angle_step`, default 15°, clamped 1°-90°, NaN falls back to 15°), not a hard-coded 15°. The hint text still says "15° Increments" (`pen_tool.rs:2445, 2472`). **[read]**

### 1.2 Key and mouse bindings **[read]** (`input_mappings.rs:277-290`)

| Input | Message | Meaning |
|---|---|---|
| pointer move | `PointerMove { snap_angle: Shift, break_handle: Alt, lock_angle: Control, colinear: KeyC, move_anchor_with_handles: Space }` | refreshes on Control, Alt, Shift, C, Space, Q, W |
| LMB down | `DragStart { append_to_selected: Shift }` | |
| LMB up | `DragStop` | |
| RMB down, Escape | `Abort` | |
| Enter | `Confirm` | |
| Delete, Backspace | `RemovePreviousHandle` | |
| Tab | `SwapHandles` | only offered while dragging a handle (`pen_tool.rs:317-327`); otherwise Tab is the Select/Path tool toggle |
| G / R / S | `GRS { grab: KeyG, rotate: KeyR, scale: KeyS }` | |
| Q (held), W (tap) | read by the snap session, not the pen | "Quiet Snapping", "Cycle Snap" (`snap_session.rs:774-776`) |
| P | activate Pen tool | `input_mappings.rs:332` |

There is **no `DoubleClick` binding** for the pen (compare Path/Select/Gradient, which have one).

### 1.3 States **[read]** (`pen_tool.rs:113-120`)

```
Ready                      nothing in progress
DraggingHandle(HandleMode) mouse is down after placing (or pressing on) an anchor; pointer pulls a handle
PlacingAnchor              mouse is up; a preview segment follows the pointer from the last anchor
GRSHandle                  the last anchor's outgoing handle is being grabbed/rotated/scaled by keyboard
```

`HandleMode` (`pen_tool.rs:384-393`): `Free`, `ColinearLocked` (the default), `ColinearEquidistant`. The enum's doc comments are misleading; the hint text and `adjust_handle_length` (`pen_tool.rs:1221-1256`) are authoritative, see 1.6.

### 1.4 Tool data that matters **[read]** (`pen_tool.rs:362-489`)

- `latest_points: Vec<LastPoint>` + `point_index`: the anchors placed in this drawing, with a cursor so undo/redo can rewind/advance. `LastPoint { id, pos, in_segment: Option<SegmentId>, handle_start }`; `handle_start` is the **outgoing** handle of that anchor (absolute, layer space), equal to `pos` for a sharp anchor.
- `next_point`: the anchor being placed (follows the pointer in `PlacingAnchor`; fixed during `DraggingHandle`).
- `next_handle_start`: the outgoing handle of `next_point`, i.e. the handle under the pointer during a normal drag.
- `handle_end: Option<DVec2>`: the incoming handle of `next_point` (second control point of the preview cubic). `None` means "no segment is pending": the drawing has only its first anchor, or the user pressed on top of the previous anchor to bend it.
- `handle_type: TargetHandle`: which handle the pointer currently drags: `FuturePreviewOutHandle` (= `next_handle_start`), `PreviewInHandle` (= `handle_end`), `PriorOutHandle(seg)` / `PriorInHandle(seg)` (a handle of an already stored segment at the clicked anchor), `None`.
- `handle_mode`, `g1_continuous`, `angle` (last direction, radians, document space), `path_closed` (this press landed on an existing anchor/segment, so the pending segment closes or joins), `prior_segment_endpoint` / `prior_segment` / `prior_segments` / `prior_segment_layer` (the existing anchor that was pressed on and the stored segment(s) meeting it), `current_layer`.
- `before_press: Option<DrawingBeforePress>`: snapshot taken at each press after the first, so Escape mid-drag cancels only that segment.
- `handle_start_offset` / `handle_end_offset`: pointer-to-handle offsets (viewport) captured when Tab or Space re-targets the drag, so nothing jumps.
- `modifiers: ModifierState { snap_angle, lock_angle, break_handle, colinear, move_anchor_with_handles }`.
- `merging_from` + `held_input`: plumbing for the deferred layer merge (1.9); a different architecture that merges synchronously does not need it.

The preview segment is a cubic `(latest.pos, latest.handle_start, handle_end, next_point)` (`pen_tool.rs:1827-1833`). All of these are in the **layer's local space**; pointer positions are converted with `document_to_viewport * layer_to_document`.

### 1.5 Transitions

`[S]` = state before. Line numbers are the match arms in `transition` (`pen_tool.rs:1659-2426`).

**Ready**
- `PointerMove` (2291): reads modifiers; calls `snap_session.snap_point(view, pointer_document, PointOptions::default())` purely to show the snap indicator where a press would start. **[read]**
- `DragStart` (1979): opens a history step "Drew a Path Segment"; `handle_mode = Free`; start position = `snap_session.snap_press(...)`, i.e. *what the indicator was already showing*, else a fresh snap (`snap_session.rs:492`); then `create_initial_point` (1.8) picks extend / insert-on-segment / append / new layer; begins a snapping gesture; -> `DraggingHandle`. **[read]**
- `Abort` (258-262): no drawing to cancel; puts woken snap targets back to sleep. **[read]**

**DraggingHandle**
- `PointerMove` (2150): reads modifiers; C edge-toggles the handle mode (1.6); Space edge-captures the offset for anchor sliding; then `drag_handle` (1123) moves the handle; sets up auto-panning. **[read]**
- `DragStop` (2137): `finish_placing_handle` -> `place_handle_or_segment` (888), then closes the history step. Result:
  - if `handle_end` was `None` (first anchor, or a bend of the previous anchor): **no segment is stored**; the drag only recorded `latest.handle_start = next_handle_start`; `handle_end` becomes `Some`; -> `PlacingAnchor`.
  - otherwise a segment `latest -> next_point` is stored with handles `[handle_start - latest.pos, handle_end - next_point]` (relative to their anchors). If `next_point` is within 5 px of another anchor of the layer the segment ends on that existing anchor and the drawing ends -> `Ready`; else a new anchor is inserted, becomes `latest`, -> `PlacingAnchor`. **[read]**
- `Confirm` / Enter (2358): commits the handle/segment exactly as `DragStop` would, then ends the drawing -> `Ready`. If that leaves a lone first anchor, the anchor (or its whole new layer) is removed. Test `enter_while_pressing_the_first_anchor_closes_the_path` (3533): Enter while the closing press is still held closes the path. **[read]**
- `Abort` / Escape / RMB (2401): aborts the history step. First drag of the drawing -> whole drawing cancelled -> `Ready`. Later drag -> restores `before_press` and continues in `PlacingAnchor` from the previous anchor ("Cancel Segment"). **[read]**
- `SwapHandles` / Tab (2278): ignored on key repeat; swaps which handle the pointer drags (1.6). **[read]**
- `RemovePreviousHandle`, `GRS`: listed as available actions (318-327) but there is **no match arm** in this state, so they do nothing. **[read]**
- `Undo`: refused (test `undo_is_refused_while_a_segment_is_being_placed`, 2992). **[read test name; mechanism is outside this file]**

**PlacingAnchor**
- `PointerMove` (2239): `place_anchor` (1278) snaps `next_point` (1.7), collapses `handle_end` and `next_handle_start` onto it (so the preview shows a sharp arrival), auto-pans. **[read]**
- `DragStart` (2011): snapshots `before_press`; places the anchor at the shown preview position and tells the snap session the press landed there (`land_press`); opens a history step; if the anchor is within 5 px of an anchor on a *different* non-artboard layer, merges that layer into the current one (1.9); then `PlacePressedAnchor` (2054): `handle_mode = ColinearLocked`, `bend_from_previous_point` (795) classifies the press (below); -> `DraggingHandle`. **[read]**
- `Confirm` / Enter (2393) and `Abort` / Escape / RMB (2415): **both end the path and keep every stored segment** (each segment was already its own committed history step). A lone first anchor is taken back, with its layer if the layer holds nothing else. -> `Ready`. Hint: "RMB / Esc / Enter: End Path". **[read]**
- `RemovePreviousHandle` / Delete / Backspace (2077): sets `latest.handle_start = latest.pos`, i.e. retracts the outgoing handle of the last anchor so the next segment leaves it sharp. It does **not** delete the last anchor or segment. **[read]**
- `GRS` (1660): only if the last anchor has a non-zero outgoing handle; starts a grab/rotate/scale of that handle about the anchor -> `GRSHandle`. **[read]**
- `Undo` (2086) / `Redo` (2108): after the document has stepped, move `point_index` back/forward to the anchors that still exist and resume the preview from that anchor; undoing the first anchor ends the drawing. The Ctrl/Shift still held from the shortcut are ignored until released (`suppress_lock_angle`, `suppress_snap_angle`). **[read]**

**GRSHandle**
- `FinalPosition` (1693): updates `latest.handle_start`; if the handle was colinear with the opposite stored handle when the operation began (`is_almost_colinear`), the opposite handle is re-aimed to stay colinear, keeping its own length.
- `Confirm` (1733) -> `PlacingAnchor`; `Abort` (1748) restores both handles -> `PlacingAnchor`; `Undo`/`Redo` cancel the transform first.
- Quirk: the `PointerMove` re-sent on Confirm/Abort carries `snap_angle: Key::Control, lock_angle: Key::Shift` (`pen_tool.rs:1738-1744, 1764-1770`), swapped relative to the real binding. Looks like a bug; do not port. **[read]**

**How a press in `PlacingAnchor` is classified** (`bend_from_previous_point`, 795-854) **[read]**
1. Within 5 px (viewport) of the **previous anchor** -> "Bend Prev. Point": `handle_end = None`, `handle_mode = Free`; the drag re-pulls the previous anchor's outgoing handle and no new anchor is created. With Ctrl held the handle direction is locked to the prior segment's handle direction (`set_lock_angle`) until Ctrl is released.
2. Else within 5 px of **any other anchor of the same layer** (`close_path_on_point`, 856) -> closing/joining press: `path_closed = true`, `handle_type = PreviewInHandle` (the drag shapes the *incoming* handle of the closing segment), `next_handle_start = next_point`, `handle_mode = Free`.
3. Else within 5 px of a **segment** of a selected layer (`shape_editor.upper_closest_segment`) -> an anchor is inserted on that segment (`ClosestSegment::adjusted_insert`, `shape_editor.rs:264`, which splits the cubic at the stored parameter `t`) and the press is treated as case 2 on the new anchor.
4. Else an ordinary new anchor: `handle_type = FuturePreviewOutHandle`, `handle_mode = ColinearLocked`.

Not only the path's first point closes it: **any** anchor of the layer other than the last one does (`pen_tool.rs:914-921`). The loop keeps the *last* match, not the nearest. **[read]**

### 1.6 Handle placement and modifiers during a drag

`drag_handle` (`pen_tool.rs:1123-1218`), `adjust_handle_length` (1221), `apply_colinear_constraint` (1258). **[read]**

- **Click without moving** -> sharp anchor: both handles of the new anchor sit on it (zero-length). Hint: "LMB: Add Sharp Point, LMB Drag: Add Smooth Point" (2446). There is no pen-side drag threshold; the snap session holds the carried feature at the pressed position until the pointer has moved more than `DRAG_THRESHOLD = 1.` px (`snap_session.rs:485, 602-606`). **[read doc comments; hold mechanism not traced line by line]**
- **Drag on a new anchor (default `ColinearLocked`)**: pointer sets the outgoing handle `next_handle_start`; the incoming handle is mirrored through the anchor: `handle_end = 2 * next_point - next_handle_start` (1248). Symmetric, equal length.
- **Alt held** (`break_handle`):
  - in `ColinearLocked`: the opposite handle stays colinear but **keeps its own length** ("Non-Equidistant Handles", 2476): `apply_colinear_constraint` re-aims it at `anchor + |opposite - anchor| * dir`. So Alt pressed mid-drag freezes the incoming handle's length at whatever the mirror last gave it, and Alt held from the start of the drag leaves the incoming handle at zero length (a one-sided handle). **[read the formula; the two consequences are inferred]**
  - in `ColinearEquidistant`: the opposite handle is forced to the equal-length mirror while Alt is down, and its previous position is restored when Alt is released ("Equidistant Handles", 2480; `previous_handle_end_pos`, 1227-1243).
  - **Alt does not break the handles into a cusp.** That is C.
- **C** (edge-triggered, debounced; 2162-2174): toggles `Free` <-> colinear. `Free -> ColinearEquidistant`; either colinear mode `-> Free`. In `Free` only the dragged handle moves and the anchor is marked not G1. Hints: "C: Make Handles Colinear" / "C: Break Colinear Handles".
- **Shift** (`snap_angle`): the line anchor->handle is constrained to a multiple of the angle step (1.7).
- **Ctrl** (`lock_angle`): the line anchor->handle keeps the direction it had on the previous frame (`self.angle`); the pointer only changes the length. Extra behaviour when pressing with Ctrl on an existing anchor: `set_lock_angle` (1550) sets the direction to the existing segment's handle direction at that anchor (`calculate_segment_angle(.., prefer_handle_direction = true)`, `utility_functions.rs:115`), switches to `ColinearEquidistant`, and reverts to `Free` when Ctrl is released (`switch_to_free_on_ctrl_release`, 1183-1186). The same engages mid-drag if Ctrl goes down in `Free` mode while the handle is within `20.` document units of the anchor (1203-1213); beyond `20.` it just freezes the current direction (1188-1190).
- **Space** (`move_anchor_with_handles`): slides the anchor, the dragged handle and any opposite handle together (`space_anchor_handle_snap`, 993). All three are handed to the snapper as features, so any of them can land on a target (test 3462). For the very first anchor of a new path the stored point itself is moved. Releasing Space resumes handle dragging from the new place.
- **Tab** (`swap_handles`, 1018): switches the dragged handle to the opposite one (out <-> in, or to the stored prior handle when closing), storing a pointer offset so it does not jump; the mouse cursor is hidden while swapped. Not available if the opposite handle has zero length (1049).
- On release, colinearity is persisted as a per-anchor flag: `SetG1Continuous { handles, enabled: is_almost_colinear(..) }` for the pair meeting at the previous anchor (963-978) and, when closing, at the closing anchor (935-954). `is_almost_colinear` = angle between one handle and the negated other `< 1e-6` rad, and `true` when either handle has zero length (`utility_functions.rs:722-732`).

### 1.7 Anchor placement, angle snapping and snapping calls

`place_anchor` (1278) and `compute_snapped_angle` (1312). **[read]**

- `relative` point for the angle: while placing, the **previous anchor** (so Shift/Ctrl constrain the chord of the new segment; `None` once `path_closed`); while dragging a handle, **the anchor itself** (so they constrain the handle direction).
- Shift: `angle = round(angle / step) * step` with `step = snapping_state.angle_step_radians()`, measured from +X in **document space** (`DVec2::X.try_angle_to(relative - pointer)`), 1321-1322.
- Ctrl: `angle = self.angle`, the last unconstrained/settled direction (1323); `self.angle` is refreshed after every snap from the settled position (1345-1350). Ctrl wins over Shift (the `if !lock_angle` guard).
- The constraint is not applied by projecting the pointer. It is passed to the snapper as a **required track**: `PointOptions { required: Locus::line(relative, DVec2::from_angle(angle)), previous: relative (placing only), ..}`; the snapper then finds targets *along that line* (tests `a_segment_held_to_an_angle_step_still_stops_where_it_meets_an_edge`, 3192; `a_segment_drawn_to_the_side_of_an_ellipse_ends_where_it_touches_it`, 3165).
- `previous` is given only when placing an anchor (`neighbor = true`), so the snapper knows the straight segment being drawn (used for perpendicular/tangent/length constructions; see the snapping section).
- When the dragged handle is mirrored, both the handle and its mirror are offered as features: `snap_features(view, &[FeatureMap::IDENTITY, mirrored], 0, pointer, options)` with `mirrored = { matrix: -I, offset: 2 * anchor }` (1333-1341). Otherwise `snap_session.snap_point`.
- After the general snap, `next_point` is additionally pulled exactly onto any anchor of the current layer within 5 px (1289-1299).
- Exclusions: `begin_snapping_gesture` (556) calls `snap_session.begin_gesture_following_edits(view, Moving { edited: [layer], anchors: [...] })` where `anchors` = the latest anchor (only while `handle_end` is `None`) and the pressed existing anchor when closing. So the layer being drawn is still a snap source (each placed anchor becomes snappable), minus the anchor the handle is pulled from. The gesture is restarted after every placed segment (2145) and ended by `cleanup` (516).
- Snapping is silenced while Q is held or the document's master snapping switch is off (`snap_session.rs:210`).
- Y-down note: angles are taken in document space with Graphite's Y-down axes. A step grid and a "keep the current direction" lock are both mirror-symmetric, so nothing here depends on the sign of Y; only `calculate_segment_angle` returns a signed angle, and it is consumed as a direction. Port the directions, not the signs. **[inferred]**

### 1.8 Starting a path: new, extend, insert on segment, append

`create_initial_point` (`pen_tool.rs:1357-1476`), in priority order. **[read]**

1. **Extend an existing anchor**: `should_extend(document, start, 5 px, selected layers + all descendants of selected groups)`. Despite the name this is `closest_point` over **all anchors**, not only open endpoints (`utility_functions.rs:41-43, 53-64`); hidden layers are skipped (78). The drawing continues in that layer from that anchor (`extend_existing_path`, 1479). If the anchor has at most one segment attached, the new outgoing handle starts as the **mirror of the existing handle** (`handle_start = 2 * position - handle`), so a plain click-click continues smoothly (test 3783); with two or more segments attached it starts sharp (test 3750). `handle_mode = Free`.
2. **Start on a segment**: else the nearest segment of the selected layers within 5 px (`upper_closest_segment`, `shape_editor.rs:1902`) is split and the drawing continues from the inserted anchor (a branch: Graphite's vector model is a graph, not a list of subpaths).
3. **Shift = append to selected layer**: if exactly one non-artboard layer is selected, the first anchor is added to that layer as a new, unconnected sub-path (`AddPointLayerPosition`, 1580).
4. **New layer**: a layer with a Path node; the first anchor is at local `(0, 0)` and the layer is translated to the press position (1450-1472). Fill/stroke from the options bar are applied.

Only layers that are **selected** can be extended or split in steps 1-2. In the Ready state the pointer hovering a selected layer's segment (and no anchor) shows an anchor marker on the curve plus a perpendicular tick of half-length `SEGMENT_OVERLAY_SIZE` (1798-1804).

### 1.9 Closing and joining

- **Closing**: press within 5 px of another anchor of the same layer (1.5 case 2). While that press is held, the pointer shapes the incoming handle of the closing segment (`PreviewInHandle`); the existing handle on the other side of that anchor is untouched unless C makes the pair colinear. On release the segment is stored between the last anchor and the existing anchor and the tool returns to `Ready` (989). A click (no drag) closes with a sharp arrival. Whenever the pending segment would complete a closed loop, the enclosed region is previewed with a `COLOR_OVERLAY_BLUE_05` fill (1928-1968). **[read]**
- **Joining another path**: a press (in `PlacingAnchor`) within 5 px of an anchor on a *different* non-artboard layer first merges that layer into the current one (`merge_layers`, `graph_modification_utils.rs:40`, which re-bases the other layer's geometry into the first layer's space), then proceeds as a closing press on that anchor (2023-2052; tests 2603, 3249). Again any anchor qualifies, not only endpoints, and the target layer does not have to be selected. The `merging_from`/`held_input`/`PlacePressedAnchor{merged}` machinery only exists because the merge is asynchronous in Graphite. **[read]**
- There is **no explicit "close path" key** in the pen (Path tool has Ctrl+J, `input_mappings.rs:275`).

### 1.10 Overlays while drawing **[read]** (`pen_tool.rs:1807-1972`)

- Outline of the preview cubic (skipped if start == end).
- Handle lines: anchor-being-placed -> its outgoing handle; previous anchor -> its outgoing handle; anchor-being-placed -> its incoming handle. Handle dots are drawn only if more than `HIDE_HANDLE_DISTANCE` (3 px) from their anchor; the dragged one is drawn "selected".
- Anchor square at the anchor being placed, during a drag.
- Dashed line previous anchor -> pointer while Shift or Ctrl is held in `PlacingAnchor` and the previous anchor has a handle (1871-1879).
- Existing path overlays (`path_overlays`) with handle visibility per the option below.
- Insert-on-segment marker and tick as in Ready; closing-fill preview.
- Each is gated by the document's overlay visibility settings (`anchors()`, `handles()`).
- The snap indicator itself is drawn by the snap session presenter, not by the pen.

### 1.11 Options bar **[read]** (`pen_tool.rs:36-48, 160-247`)

Fill (on/off + colour; **enabled by default**, `DrawingToolState::new(true)`), swap fill/stroke, Stroke (on/off + colour), a stroke popover (`StrokeOptionsUpdate`: weight, align, cap, join, miter limit, paint order, dash lengths, dash offset; `stroke_options.rs:13-22`), and a two-way radio for handle overlays: "Show All Handles" vs "Show Frontier Handles" (default `FrontierHandles`: only handles of the segments meeting the last anchor). There is **no draw-mode option** (no polyline/arc/curve mode switch, no "rubber band" toggle).

### 1.12 Status-bar hints, verbatim **[read]** (`pen_tool.rs:2429-2494`)

- Ready: `LMB Draw Path` `+Shift Append to Selected Layer` | `Q Quiet Snapping` `W Cycle Snap`
- PlacingAnchor: `RMB / Esc / Enter End Path` | `Shift 15° Increments` `Ctrl Lock Angle` | `LMB Add Sharp Point` `LMB Drag Add Smooth Point` | `LMB / LMB Drag Bend Prev. Point` `+Ctrl Lock Angle` | snapping hints
- DraggingHandle: `RMB / Esc Cancel Segment` `Enter End Path` | `C Make Handles Colinear` (Free) or `C Break Colinear Handles` (colinear) `Tab Swap Dragged Handle` | `Shift 15° Increments` `Ctrl Lock Angle` [`Alt Non-Equidistant Handles` (ColinearLocked) / `Alt Equidistant Handles` (ColinearEquidistant)] `Space Drag Anchor` | snapping hints

### 1.13 History model **[read]**

Each segment, and the closing click, is its own undo step (test `each_segment_and_the_closing_click_are_their_own_history_steps`, 2896). Undo while drawing rewinds one segment and keeps drawing from the previous anchor (2922); redo advances (2949).

### 1.14 NOT present in the pen tool

- No double-click-to-finish. A second click on the last anchor is "Bend Prev. Point" (1.5 case 1) and adds nothing. **[read binding; consequence inferred]**
- Alt does not break tangents (it only controls handle length symmetry); cusps are made with C or by clicking the previous anchor again.
- Delete/Backspace does not remove the last anchor; it only retracts its outgoing handle. Removing the last anchor is done with Undo.
- No arc, line-only (polyline) or tangent-arc drawing mode; every segment is a cubic Bézier (possibly with zero-length handles). Nothing emits circular arcs.
- No numeric entry of length/angle/coordinates while drawing; no relative-coordinate input; no on-canvas length or angle readout drawn by the pen. (The G/R/S handle transform runs through the shared transform layer, which has typed-value bindings such as `TypeBackspace`/`TypeNegate`/`TypeDecimalPoint` at `input_mappings.rs:105-108`; whether typed values work for the pen handle case was **not verified**, and it only ever applies to the last outgoing handle.)
- No fixed 45°/90° ortho mode separate from the angle step; Shift is the only angular constraint and its step is one document-wide setting.
- No auto-smoothing or curve fitting of clicked points; no "rubber-band curvature" preview beyond the single pending segment.
- No dedicated close-path key or Enter-closes behaviour; Enter and Escape both leave the path open.
- No minimum-segment-length rejection (a click exactly on the last anchor is special-cased, nothing else is).
- `CREATE_CURVE_THRESHOLD` is defined and unused; there is no "small drags count as clicks" threshold other than the snap session's 1 px.
- Cannot extend or split an **unselected** layer at the start of a path (join at the end of a path works on any layer).

---

## 2. Path (node-editing) tool

Sources (paths relative to the Graphite root, commit `615b4050`):

- `editor/src/messages/tool/tool_messages/path_tool.rs` (abbrev. **PT**), read in full to line 3895 (tool code ends at 3784; the rest is tests, of which only the names were read).
- `editor/src/messages/tool/common_functionality/shape_editor.rs` (**SE**), read in full.
- `editor/src/messages/tool/common_functionality/utility_functions.rs` (**UF**), lines 110-250, 500-640, 715-735.
- `editor/src/messages/portfolio/document/overlays/utility_functions.rs` (**OF**) lines 18-330, `overlays/utility_types.rs` (**OT**) marker drawing.
- `node-graph/libraries/vector-types/src/vector/` (**VT**): `vector_types.rs` 505-660, `vector_modification.rs` 590-875, `misc.rs` 523-557 and 655-671, `algorithms/util.rs`, `algorithms/bezpath_algorithms.rs` 200-215.
- `editor/src/consts.rs` (**C**), `editor/src/messages/input_mapper/input_mappings.rs` (**IM**).

Tags: **[read]** = taken directly from code at the cited lines. **[inferred]** = my reasoning from that code, not stated by it.

### 2.0 Constants (verbatim)

| Constant | Value | Where | Used for |
|---|---|---|---|
| `SELECTION_THRESHOLD` | `10.` | C:109 | point (anchor/handle) hit radius, viewport px |
| `SEGMENT_INSERTION_DISTANCE` | `5.` | C:112 | segment hover/hit distance, viewport px (`+ 0.5` added at SE:1849) |
| `HIDE_HANDLE_DISTANCE` | `3.` | C:111 | handle closer than this to its anchor is neither drawn (OF:78) nor hit-testable (SE:1812) |
| `DRAG_THRESHOLD` | `1.` | C:48 | click vs drag, viewport px |
| `DRILL_THROUGH_THRESHOLD` | `10.` | C:110 | repeat double-click within this distance cycles overlapping layers |
| `DRAG_DIRECTION_MODE_DETERMINATION_THRESHOLD` | `15.` | C:61 | box diagonal at which directional select mode is fixed |
| `SELECTION_TOLERANCE` | `5.` | C:60 | lasso point thinning (`select_tool.rs:2377-2388`) |
| `SEGMENT_OVERLAY_SIZE` | `10.` | C:113 | half-length of the insertion tick / delete X |
| `MANIPULATOR_GROUP_MARKER_SIZE` | `6.` | C:108 | anchor square side / handle circle diameter |
| `SEGMENT_SELECTED_THICKNESS` | `3.` | C:114 | defined, but not referenced in the checked-out paths; selected segments are actually stroked `4.` (OT:1171) |
| `HANDLE_LENGTH_FACTOR` | `0.5` | C:115 | handle length when an anchor is made smooth (SE:1100) |
| `NUDGE_AMOUNT` / `BIG_NUDGE_AMOUNT` | `1.` / `10.` | C:53-54 | arrow-key nudge |
| `DOUBLE_CLICK_MILLISECONDS` | `500` | C:218 | window in which the pre-click selection is remembered (PT:779) |
| `MAX_SNAPPING_POINTS` | `16` | PT:574 | most dragged points that take part in snapping |
| `ANGLE_STEP_DEFAULT` / `MIN` / `MAX` | `15.` / `1.` / `90.` (degrees) | `portfolio/document/utility_types/misc.rs:11,13,15` | Shift angle step, per-document setting |
| unnamed | `25.` | PT:1204 | Ctrl-lock: handle shorter than this picks a tangent-derived angle |
| unnamed | `10.` | PT:3485 | Ctrl-lock: handle within this of an adjacent anchor (layer-space units, see 2.5) |
| `POSITION_TOLERANCE` | `1e-6` | PT:2720 | match anchors by position when joining after a layer merge |
| `SEGMENT_PAIR_FIT_SAMPLES` / `_PASSES` / `_TANGENTIAL_WEIGHT` | `16` / `12` / `0.01` | UF:523,525,527 | refit used by point sliding (2.10) |
| kurbo `DEFAULT_ACCURACY` | `1e-6` | kurbo 0.13.1 `param_curve.rs:19` | nearest-point accuracy for segment hover/insert |

### 2.1 Data model the tool edits [read]

- Geometry is points + segments (line / quadratic / cubic `kurbo::PathSeg`), addressed by stable `PointId` / `SegmentId`. A manipulator is `ManipulatorPointId::{Anchor(PointId), PrimaryHandle(SegmentId), EndHandle(SegmentId)}`: handles belong to segments (start-side and end-side), not to anchors. Handle positions are stored relative to their anchor (`SetHandlePosition { relative_position }`, VT `vector_modification.rs:604-607`), so moving an anchor carries its handles.
- "Colinear" (G1) is a per-anchor smooth flag, valid only where exactly two segment ends meet (`pairs_smoothly`, VT `vector_types.rs:533-536`; `colinear_pairs` 559-585). The editing API speaks in handle pairs: `SetG1Continuous { handles: [HandleId; 2], enabled }` (VT `vector_modification.rs:598-601`). Junctions with more than two segments (meshes) have no pair.
- `handle_pair_of(point)`: for an anchor, the first two connected handles; for a handle, itself plus the first other handle at the same anchor (VT `misc.rs:523-541`). `None` for the end handle of an open path.
- Selection state: per selected layer, a `HashSet<ManipulatorPointId>` plus a `HashSet<SegmentId>` (SE:49-58). Only selected, visible, unlocked layers are editable (PT:1602-1614). Points hidden by the overlay-visibility toggles are parked in "ignored" sets and skipped by moves (SE:121-151, 425-427).

### 2.2 State machine [read]

States (PT:562-571): `Ready`, `Dragging(DraggingState)`, `Drawing { selection_shape: Box | Lasso }`, `SlidingPoint`.

| From | Event | To | Effect |
|---|---|---|---|
| any | LMB down on a visible point within 10 px | Dragging | select (2.3), start snapping gesture (2.9) |
| any | LMB down on a segment within 5 px, segment mode without Ctrl | Dragging | select segment, drag moves it unsnapped |
| any | LMB down on a segment within 5 px, point mode (or segment mode + Ctrl) | Dragging | arm molding; release without moving inserts a point or (Alt) deletes the segment |
| any | LMB down on a layer's fill or on nothing | Drawing | box, or lasso if Ctrl held at press |
| Dragging | pointer move | Dragging | mold, or move points/segments (2.5, 2.7) |
| Dragging | LMB up | Ready | click-resolution of selection, insert/delete on segment, end transaction (PT:2498-2671) |
| Dragging | Esc or RMB | Ready | abort transaction (geometry restored), restore saved selection (PT:2393-2411) |
| Drawing | LMB up or Enter | Ready | apply box/lasso selection (PT:2339-2392, 2430-2497) |
| Drawing | Esc or RMB | Ready | discard (PT:2412-2415) |
| Ready | G pressed again during a Grab of a single colinear anchor | SlidingPoint | see 2.10 (`transform_layer_message_handler.rs:460-481`, PT:2764-2771) |
| SlidingPoint | pointer move | SlidingPoint | slide anchor along its two segments |
| SlidingPoint | LMB up | Ready | commit (falls to the generic DragStop arm, PT:2498) |
| SlidingPoint | Esc or RMB | Ready | abort (PT:2421-2428) |
| Ready | Esc | Ready | `snap_session.sleep_all()` (PT:2416-2420) |

Leaving `Dragging` by any route ends the snapping gesture (PT:439-446). Auto-panning runs in `Drawing` and `Dragging` when the pointer leaves the viewport, shifting `drag_start_pos` by the pan offset (PT:2281-2296).

### 2.3 Hit-testing order and tolerances [read]

On press (`mouse_down`, PT:756-967), in this order:

1. **Point** within `SELECTION_THRESHOLD` (10 px, viewport space). `closest_point_in_layer` (SE:1799-1842) scans handles first, then anchors, each with `<=`, so an anchor wins a tie with a handle. A handle is skipped when within `HIDE_HANDLE_DISTANCE` (3 px) of its own anchor; a quadratic's single handle is also skipped when within 3 px of the far anchor (SE:1812-1816). The hit must be visible under the current handle-overlay mode (`is_visible_point`, UF:202-242), otherwise the press falls through. With point-editing mode off, anchors are not hit, only handles (SE:636-639).
   - Layers are scanned in `HashMap` key order and the first layer whose nearest point is under the threshold wins (SE:1745-1753). It is not the globally nearest point across layers. **[inferred]** this is order-dependent when two selected layers have points under the cursor.
2. **Segment** within `SEGMENT_INSERTION_DISTANCE + 0.5` (5.5 px) (PT:892, SE:1849). Topmost selected layer first (`upper_closest_segment`, SE:1902-1909).
3. **Layer under the pointer** (fill hit): if no layer is selected, that layer becomes selected; then a box/lasso starts (PT:943-958).
4. **Nothing**: box/lasso starts (PT:960-966).

Hover uses the same tests each overlay frame (`update_closest_segment`, PT:1260-1292): a nearby visible point suppresses the segment hover when point mode is on; an already-hovered segment stays hovered until the pointer is more than 5 px away from its closest point.

### 2.4 Selection model [read]

Editing modes (PT:166-179, 1627-1729): `point_editing_mode` (default on) and `segment_editing_mode` (default off). At least one stays on. Clicking a mode button switches to that mode alone; Shift+click toggles it alongside the other. Switching to point-only selects the endpoints of selected segments and clears segments; switching to segment-only selects every segment whose two endpoints are selected.

Point click (PT:796-890 on press, 2550-2576 on release):

- Press, no Shift, point not selected: clear all, select it.
- Press, no Shift, point already selected: keep the whole selection (so a drag moves all). On release without a drag the selection collapses to that point; the previous selection is remembered for 500 ms so a double-click can convert all of them (PT:2560-2575, 779-782).
- Press with Shift on an unselected point: add it. Shift on a selected point: unchanged on press, deselected on release if no drag happened (PT:2552-2559).
- Dragging a handle while other things were selected: the earlier selection is saved on press and restored on release if a drag occurred (PT:827-837, 2131-2133, 2636-2648). So handle drags do not disturb the anchor selection.

Segment click, segment mode (PT:897-929, 2578-2625): same replace / Shift-add / Shift-toggle-on-release rules. In segment-only mode a selected segment's two anchors are also put in the point selection, and removed again on deselect unless another selected segment shares them.

Click on a selected layer's fill or on empty canvas without dragging: deselect all points and segments (PT:2457-2464, 2626-2630).

Double-click (PT:3080-3180):

- On a point: toggle smooth/sharp for the remembered selection (2.6).
- On a selected layer's fill: select all anchors (and/or segments, per mode) of the subpath(s) containing the click, found by `BezPath::contains` then flood-fill over connectivity; if no subpath contains the click, select everything in the layer (SE:693-745). Shift: deselect the layer. Alt: clear that layer's point and segment selection.
- On an unselected layer: select it (Shift adds). Repeated double-clicks within 10 px cycle through the overlapping layers (drill-through, PT:710-737).
- On background: deselect all layers.

Keyboard (IM:240-246, PT:3196-3233, SE:769-817):

- Accel+A: all anchors of the selected layers (plus all segments in segment mode). Line PT:3197 selects anchors unconditionally, even in segment-only mode.
- Accel+Shift+A or Alt+A: deselect all points and segments.
- Shift+Plus / Shift+Minus: grow / shrink the selection by one step along topology (neighbouring anchors; segments sharing an endpoint).
- Tab during a handle drag: swap the dragged handle for the next handle around the same anchor (smallest positive angle from the current one, SE:1946-2003). Requires a handle with a pair and no anchor selected; ignored on key repeat (PT:429-437). **[inferred]** the rotational sense depends on the Y direction of layer space.

Box and lasso (PT:1923-1998, 2430-2497; SE:2146-2335):

- Box by default; Ctrl held at press makes a freehand lasso (IM:232). Lasso points are thinned: when the last two points are closer than 5 px the last is replaced (`select_tool.rs:2377-2388`).
- Release modifiers: Shift = extend, Alt = subtract, neither = replace (PT:2434-2440). Enter commits the same way (PT:2339).
- **Points** (anchors and handles) are selected when strictly inside the rectangle / lasso polygon, in viewport space. Touched vs enclosed does not apply to points. Handles must be visible in the current overlay mode and have non-zero length (SE:2180-2184, 2303-2311).
- **Segments** (only when segment mode is on) depend on a user preference `SelectionMode { Touched (default), Enclosed, Directional }`:
  - Box, Enclosed: rectangle contains the segment's bounding box. Box, Touched: that, or any rectangle edge intersects the segment (`PathSeg::intersect_line`) (SE:2256-2270).
  - Lasso: 11 parametric samples of the segment (`steps = 10`, SE:2275); Enclosed = all inside the polygon, Touched = any inside.
  - Directional (PT:657-672): once the drag diagonal exceeds 15 px, a drag whose current x is left of the start x is Touched, otherwise Enclosed; fixed for the rest of the drag. Before the threshold it reads as Touched.
- Overlay: Enclosed draws a dashed outline (4 px dash, 4 px gap), Touched a solid one, both filled `COLOR_OVERLAY_BLUE_05`; points and segments that would be selected are shown in hover style while dragging (PT:1960-1997).
- The box's far corner is kept in document space, so it follows canvas panning (PT:675-678, 2075).

### 2.5 Dragging points [read unless tagged]

Common setup (`start_dragging_point`, PT:969-1001): the grab offset between the pointer and the hit point is removed, so the point itself, not the cursor position, is what snaps. A grey "ghost" outline of the pre-drag geometry is drawn once the drag exceeds 1 px (PT:1732-1741).

**Anchors, or more than one point** (`slide_selected_points`, PT:1038-1067; `prepare_slide`, PT:1004-1035):

- All selected points translate by one common vector, in document space: `pointer_document - slide.start`, then snapped.
- Snapping considers the grabbed point plus the 15 selected points nearest to it (`MAX_SNAPPING_POINTS - 1`), whichever lands best; spacing between them is preserved.
- **Shift** = axis lock to a screen axis through the grabbed point's start. The axis is whichever screen axis the pointer has travelled further along since the press; a tie keeps the current axis (PT:1449-1456). The lock is passed to the snapper as a required line, so the point can still stop where the axis crosses a snap target. Overlay: full-viewport red line for X or green for Y, the other axis at 25% (PT:2000-2016).
- Moving an anchor also moves a quadratic segment's lone handle when the anchor is the segment's end and its start is not selected (SE:930-955).
- A handle whose anchor is also selected is not moved separately (SE:1298-1301).

**Exactly one handle selected** (`drag`, PT:1463-1496; `calculate_handle_angle`, PT:1170-1241):

- Target = pointer, expressed relative to the handle's anchor, in document space.
- **Shift** (no Ctrl): the handle angle is rounded to the document's angle step, default 15° (`(angle / step).round() * step`, PT:1233-1236). The angle is measured from +X in document space. **[inferred]** with a tilted canvas the increments are relative to the document, not the screen; and Graphite's document space is Y-down, so the sign of angles flips in a Y-up port, though rounding is symmetric.
- **Ctrl** = lock angle. The angle is captured on the first frame Ctrl is seen and held until Ctrl is released (PT:2193-2196). Which angle is captured:
  1. If the handle sits within `10.` of an adjacent anchor (PT:3478-3489; measured between layer-space positions, so **[inferred]** not zoom-compensated), lock to the tangent at that neighbouring anchor and make the lock line pass through that neighbour (`adjacent_anchor_offset`, PT:1190-1201, 3490-3548).
  2. Else if the handle is shorter than `25.` document units (PT:1204): at a path end, the direction of its segment; otherwise, if the opposite handle is non-zero, make the pair colinear temporarily and lock to the opposite handle's line; otherwise the mean of the two adjacent segment angles, plus 90° when Alt is also held (PT:3383-3439).
  3. Else the handle's current angle (PT:1217-1224).
- With either modifier the handle is projected onto the constraint line through the anchor and that line is given to the snapper as `required`; the anchor is passed as `previous` so the snapper can offer lines from it (PT:1479-1495).
- Colinear partner update (`move_selected_points_and_segments`, SE:1312-1348), when the handle has a G1 partner:
  - Default: partner keeps its own length and is rotated to point exactly opposite.
  - **Alt** (equidistant): partner = exact mirror, `-(handle - anchor)`. The partner's original length is recorded when Alt goes down and restored when Alt is released (PT:1102-1134, 1501).
  - If the dragged handle collapses onto the anchor (`length² < f64::EPSILON * 1e5`), the partner returns to where it was at drag start (SE:1330-1332).
  - If both handles of a pair are selected and dragged without their anchor, the G1 flag is switched off (SE:1318-1324).
- **C** (edge-triggered, PT:1089-1099): toggle colinear for the selection mid-drag.
- **Space** (held): temporarily extends the selection to the handle's anchor and both its handles so the whole manipulator moves; restored on release of Space or of the mouse (PT:2158-2186, 2654-2659; SE:2037-2057). Only when one layer has a selection.
- **Tab**: swap to the other handle (2.4).

**Press modifiers on an anchor** (PT:839-884, 1506-1548):

- **Alt+click an anchor**: retract every handle at it to zero length and clear its colinear flags ("To Sharp").
- **Alt+drag from an anchor**: after that retraction, once the drag passes 1 px, the handle whose segment direction best matches the drag direction is selected (anchors with exactly two segments only) and, because Alt is held and the opposite handle has zero length, the pair is made colinear and pulled out symmetrically ("To Smooth").
- **Alt at press on a handle**: break colinearity before dragging.
- **Ctrl at press on an interior anchor that has exactly one zero-length handle**: select that handle and make the pair colinear, so the drag pulls out the missing handle opposite the existing one (PT:868-884).

**Segment drag in segment mode**: the endpoints of all selected segments translate together; no snapping ("Dragged segments slide unsnapped", PT:922-926), Shift axis lock still applies.

### 2.6 Smooth / sharp conversion and colinear toggling [read]

Triggers: double-click an anchor (`flip_smooth_sharp`, SE:2076-2143); "Colinear Handles" checkbox in the options bar (PT:3262-3274); C during a handle drag.

`flip_smooth_sharp`, per selected anchor:

- **To smooth** when any connected handle lies on the anchor (within `1e-5`) or any connected segment is a line. Uses `convert_manipulator_handles_to_colinear` (SE:1021-1113):
  - Take the first two connected handles and the two neighbouring anchors (neighbours coincident with this anchor within `1e-6` are ignored; if all are, do nothing).
  - Direction: with two usable neighbours, the mean of the two segment angles plus 90°; with one, that segment's angle. Flip the direction if it points away from the first neighbour or toward the second.
  - If one handle is already non-zero and no connected segment is a line: the zero handle becomes the mirror of the non-zero one (same length, opposite direction).
  - Otherwise each handle length = `|neighbour - anchor| * HANDLE_LENGTH_FACTOR * |direction · unit(neighbour - anchor)|`, first handle along `+direction`, second along `-direction`. A missing opposite handle on the far end of a line is created with zero length, turning the line into a cubic.
  - Set the G1 flag.
  - **[inferred]** the mean is a plain arithmetic mean of two `atan2` angles, not a circular mean, so it can be wrong across the ±π wrap; the flip step corrects the common cases.
- **To sharp** otherwise: cubic handles at the anchor are set to zero length and G1 cleared; a quadratic segment is converted to a line.

`convert_selected_manipulators_to_colinear_handles` (checkbox / C key, SE:1122-1244):

- One handle of the pair selected: the unselected handle is rotated to oppose it, keeping its length.
- Both or neither selected (anchor selected): both are set along the normalised difference of the two handle vectors, each keeping its own length. If a handle is ~zero, each handle vector defaults to one third of the way to its neighbouring anchor.
- Anchors with more than two segments are skipped; for a handle at such a junction, the partner chosen is the non-colinear handle most nearly opposite.

The checkbox is enabled when several points are selected, or one point with a handle pair at an anchor with at most two connections (PT:680-697). It shows unchecked for a mixed selection (TODO at PT:259).

### 2.7 Inserting a point, deleting a segment, molding [read unless tagged]

Hover: the closest point on the hovered segment is found with `PathSeg::nearest(pointer_in_layer_space, DEFAULT_ACCURACY)` (kurbo, accuracy `1e-6`), `t` clamped to `[0, 1]` (SE:229-239, 1857-1859). The 5 px distance test is done in viewport space on the evaluated point. **[inferred]** the nearest search runs in layer space, so under a non-uniform layer transform the result is not exactly the nearest point on screen. A cubic whose two handles sit on its anchors (within `f64::EPSILON * 100.`) is treated as a line (SE:1867-1874). Overlay: a tick perpendicular to the curve, ±10 px; with Alt held, an X at 45° (PT:1870-1888).

**Insert** (`adjusted_insert`, SE:264-320): on release without movement, in point-only mode with no modifier, or in segment mode with Ctrl (PT:2522).

- Split exactly at `t` with `PathSeg::subsegment(0..t)` and `(t..1)` (de Casteljau).
- New anchor at the split point; two new segments replace the old one, each with the sub-curve's own handles. A line splits into two lines with no handles.
- The two handles at the new anchor are flagged G1 when the original had an end handle (cubic). G1 pairings at the two original ends are carried over to the new segments, and the "begins a path" mark moves to the first half.
- The new anchor becomes the selection (Shift extends) (SE:322-332).
- The curve shape is unchanged by insertion.

**Delete segment**: Alt+click on a segment in point-only mode (PT:2523-2527; Alt state is the one sampled during the last hover move, PT:2253). `dissolve_segment` (SE:1394-1415) removes the segment, clears G1 pairs involving it, and removes an endpoint that had no other segment.

**Mold** (drag on a segment in point mode, or Ctrl+drag in segment mode; PT:931-939, 2137-2156; `mold_handle_positions`, SE:355-416):

- At press the two control points are recorded: the cubic's handles; for a line, the points at 1/3 and 2/3; for a quadratic, the handle and the end point (PT:933-937).
- While dragging, both control points are set to `original + delta`, where `delta` is the pointer displacement from the press-time closest point, converted to layer space. Both handles get the same full translation.
- **[inferred]** the curve point under the cursor therefore moves by `3t(1-t) * delta`, not by `delta`; this is not the "point follows cursor" (weighted) molding of some editors. A molded line becomes a cubic.
- Neighbours: by default, a G1 partner handle on each adjacent segment is rotated to stay opposite the new handle, keeping its length (`adjust_handle_colinearity`, UF:142-153). With **Alt** held, the partners are put back opposite the original handle directions and G1 is cleared (`restore_previous_handle_position`, UF:155-176); releasing Alt mid-drag re-establishes G1 (`restore_g1_continuity`, UF:178-199).
- No snapping while molding. A mold released where it began inserts nothing (test name PT:4275).

### 2.8 Deleting, cutting, joining [read]

| Keys | Action | Behaviour |
|---|---|---|
| Delete / Backspace | `Delete` (PT:2674-2695) | Dissolve. With nothing selected, deletes the selected layers instead. |
| Accel+Delete | `DeleteAndBreakPath` (SE:1664-1710) | Remove selected anchors and their segments, leaving a gap. Hint text: "No Dissolve". |
| Shift+Delete | `BreakPath` (SE:1611-1660) | Cut at each selected anchor: the first segment keeps the anchor, every other segment gets a new coincident anchor; G1 cleared. Hint: "Cut Anchor". |
| Accel+J | `ClosePath` (SE:491-568) | Join, see below. |

Dissolve (`delete_selected_points`, SE:1458-1566):

- Selected segments are removed first (`dissolve_segment`).
- A selected **handle** is retracted to zero length and its G1 pair cleared; nothing is removed.
- A selected **anchor** and its segments are removed. If it had exactly two connections and neither adjacent segment was selected, its two neighbours are reconnected by one new segment. Runs of consecutive deleted anchors are walked to the surviving neighbours on each side; a fully deleted loop is not reconnected.
- **The merged segment is not refit.** Its handles are simply the two outer handles of the removed segments, unchanged relative to their anchors (SE:1518-1532). The curve shape through the deleted anchor is lost.
- G1 pairings at the surviving ends are re-pointed at the new segment (SE:1542-1563); the path-beginning mark is preserved.
- In segment-only mode, Delete removes selected segments and then only those selected anchors whose segments were all selected (`delete_hanging_selected_anchors`, SE:1588-1607).

Join (`close_selected_path`, SE:491-568):

- Exactly two anchors selected: connect them with a straight segment (`handles: [None, None]`). In different layers, the layers are merged first and the anchors re-found by document position (tolerance `1e-6`, PT:2713-2763).
- Any other selection count: for each selected layer that has exactly two open endpoints in total, connect those two with a straight segment.
- **[inferred]** there is no check that the two selected anchors are path ends, so an interior anchor can gain a third segment. Coincident endpoints are joined by a zero-length segment, not welded into one anchor.

### 2.9 Snapping while dragging [read]

The tool calls the shared snap session; the snapping engine itself is section 3.

- At press: `snap_session.begin_gesture(view, Moving { edited, anchors, .. })` (PT:972-1000). `edited` = layers with selected points; `anchors` = the anchor of every selected point (a selected handle contributes its anchor). The drag therefore snaps to none of the moved anchors nor to segments ending at them, but does snap to the stationary anchors of the same path (test names PT:3878, 3985).
- Multi-point / anchor drag: `snap_features(view, features, lead = 0, free, PointOptions { previous, required })` with `features` = grabbed point first, then up to 15 nearest selected points, each as `FeatureMap::sliding_from(start)` (PT:1033, 1058-1063). `previous` = the single unselected neighbour of the grabbed anchor when the joining segment is straight (no handles), which lets the snapper offer angle/length constructions from it (PT:3442-3456). `required` = the Shift axis line.
- Single handle: `snap_point(view, target, PointOptions { previous: Some(anchor), required })` (PT:1490-1495).
- No snapping for: segment drags, molding, point sliding, nudging (except the grid rule in 2.11).
- Snap keys are shown as hints only while points are being dragged: "Q Quiet Snapping", "W Cycle Snap" (`snap_session.rs:774-776`, PT:3731-3735); the pointer-move mapping refreshes on Q and W (IM:238).
- The gesture ends on any exit from `Dragging` (PT:1070-1076).

### 2.10 Sliding an anchor along its path (G, G) [read]

- Trigger: with one colinear anchor selected, press G (grab) then G again (`transform_layer_message_handler.rs:460-481`). The anchor must have exactly two segments (PT:1306-1324).
- Each move (`slide_point`, PT:1336-1426): find the nearest point on each of the two original segments (`nearest`, accuracy `1e-6`), take the closer; move the anchor there.
- The segment the anchor moved into is replaced by its exact sub-curve from its far end to `t`.
- The other segment must now also cover the piece the first one gave up. It is refit with `fit_handles_to_segment_pair` (UF:531-636): handle directions are fixed (far handle's existing direction; path tangent at `t`), only the two handle lengths are solved. 16 samples per source segment weighted by arc-length share, a 2x2 weighted least-squares solve, then 12 Gauss-Newton passes that re-project each sample onto the fitted cubic (normal error weight 1, tangential weight `0.01`); negative lengths fall back to collapsing one handle. A pair that was split from one cubic is reproduced exactly (doc comment UF:529-530).
- This is the only refit in the tool. It is not used by Delete.

### 2.11 Keyboard nudge and typed coordinates [read]

- Arrow keys: `NUDGE_AMOUNT` (1); with Shift: `BIG_NUDGE_AMOUNT` (10). Two arrows held give a diagonal (IM:251-274).
- Space: the step is in document units, along the screen axes: `translation = rotate(nudge, -canvas_tilt)` (`snap_session.rs:746`). It is not scaled by zoom.
- With snapping enabled and a grid active, the nudge instead moves until the selected position furthest along the direction lands on the next grid line; a big nudge goes to the next major line, or 10 lines when only major lines are drawn (`snap_session.rs:728-770`; test name PT:4057).
- A nudged handle turns its colinear partner without changing the partner's length (PT:3191-3192). No resize modifiers apply to points.
- The mapping is Y-down: ArrowUp is `delta_y: -NUDGE_AMOUNT` (IM:257). A Y-up port must flip this.
- Options-bar **X / Y** fields: shown for exactly one selected point, in document space, unit " px" (PT:212-248, 3359-3362). Typing moves an anchor by delta; typing a handle position sets it and clears its G1 pair (SE:959-996).

### 2.12 G / R / S [read]

- The path tool's own `GRS` message is mapped (IM:235-237) but is absent from `actions()` (PT:455-521) and has no arm in the FSM, so it does nothing. The effective bindings are the global `TransformLayerMessage::BeginGrab / BeginRotate / BeginScale` on G / R / S (IM:409-411), which act on the selected points when the path tool is active.
- A lone anchor whose handles are both zero or absent accepts G but not R or S; a lone zero-length handle accepts none (`transform_layer_message_handler.rs:487-515`).
- During the operation: X / Y constrain axis, digits type a value, Shift slows, Ctrl snaps to increments, Enter / LMB apply, Esc / RMB cancel (IM:99-109). Detail belongs to the transform section.
- The pivot gizmo is force-disabled for the path tool (PT:1598-1599) and its options-bar widgets are commented out (PT:375-381).

### 2.13 Copy, cut, paste, duplicate [read]

- **Copy** (Accel+C, PT:2772-2845): anchors that are selected or are endpoints of a selected segment; segments that are selected or have both endpoints selected; G1 pairs whose two segments were both copied. Stored with the layer's transform.
- **Cut** (Accel+X): Copy, then delete without dissolving (PT:2846-2852, 2974-2980).
- **Paste** (PT:2853-2973): into the source layer if it is still selected; otherwise a new layer with black stroke of width `DEFAULT_STROKE_WIDTH` (`2.`, C:57) and white fill, centred in the viewport. New ids throughout; pasted points/segments become the selection.
- **Duplicate** (Accel+D, PT:2981-3079): same subset, duplicated in place in the same layer with new ids, and selected. It is not offset, so the copy sits exactly on the original.

### 2.14 Options bar and overlays [read]

Options bar, left to right (PT:359-382): X, Y, "Colinear Handles" checkbox, Point Editing Mode, Segment Editing Mode, handle-visibility radio, "Make Path Editable" button.

Handle visibility (`PathOverlayMode`, PT:159-164; `is_visible_point`, UF:202-242):

- `AllHandles`: every handle.
- `SelectedPointHandles` (default): handles of segments touching a selected anchor or owning a selected handle, plus the handle opposite a selected handle.
- `FrontierHandles`: only the handles at the outer ends of the run of selected segments; behaves like the previous mode when one point is selected.
- Hidden handles cannot be clicked or box-selected.

Drawn (OF:195-273; OT:746-786, 1166-1180):

- Path outline of each selected layer.
- Anchors: 6 px squares, white fill with blue stroke, blue fill when selected. Hover: 8 px 50%-blue halo behind.
- Handles: 6 px diameter circles with a line to their anchor, same colouring; not drawn within 3 px of the anchor.
- Selected segments: 4 px blue stroke. Hovered segment in segment mode: 4 px 50%-blue stroke.
- Insertion tick / delete X on a hovered segment; axis-lock lines; grey ghost outline during a drag; box / lasso; outlines of hovered layers (blue for the one a double-click would pick, grey for others, PT:1891-1921).
- Path direction markers, path / point index labels and a curvature comb also exist (C:117-147, OF:347-600); not studied.

### 2.15 Status-bar hints (PT:3550-3784) [read]

- Ready, hovering a segment, point mode: "LMB Insert Point on Segment" · "LMB drag Mold Segment" · "Alt+LMB Delete Segment".
- Ready, hovering a segment, segment mode: "LMB Select Segment, +Shift Extend" · "Ctrl+LMB Insert Point on Segment" · "Ctrl+LMB drag Mold Segment".
- Ready, hovering a point: "LMB Select Point, +Shift Extend".
- Ready, otherwise: "LMB drag Select Area, +Ctrl Lasso".
- With an anchor selected: "Double-click Convert Anchor Point" · "Alt+LMB To Sharp" · "Alt+LMB drag To Smooth".
- With a point selected: "LMB drag Drag Selected" · "G/R/S Grab/Rotate/Scale Selected" · (single colinear anchor) "G G Slide" · "Arrows Nudge Selected, +Shift 10x" · "Backspace Delete Selected, +Accel No Dissolve, +Shift Cut Anchor".
- With an anchor or segment selected: "Shift+Plus / Shift+Minus Grow/Shrink Selection".
- Dragging a handle: "RMB / Esc Cancel" · "Tab Swap Dragged Handle" (pair only) · "C Break Colinear Handles | Make Handles Colinear" · "Alt Equidistant Handles" (when colinear) · "Shift 15° Increments" · "Ctrl Lock Angle" · "Space Drag Anchor" · "Q Quiet Snapping" · "W Cycle Snap".
- Molding: "RMB / Esc Cancel" · "Alt Break Colinear Handles" (when a neighbour is colinear).
- Box / lasso: "RMB / Esc Cancel" · "LMB drag Select Area, +Shift Extend, +Alt Subtract".
- The "15° Increments" hint text is a literal (PT:3705); the actual step is the document setting.

### 2.16 NOT present (checked in PT and SE)

Each was checked by reading both files and a case-insensitive grep for `fillet|chamfer|align|distribut|simplif|reverse|arc|elliptic|segment.length|weld` over PT and SE (only unrelated hits).

- **No arc segments.** Segments are line / quadratic / cubic Bezier only; no bulge, no circular-arc editing or arc-preserving insert.
- **No fillet or chamfer** of a node, and no corner-radius handle.
- **No refit on delete.** Dissolving an anchor keeps the outer handles as they are (2.8).
- **No numeric segment length, angle, or handle length/angle fields.** Only X / Y of a single selected point.
- **No align or distribute nodes.**
- **No explicit "segment to line" / "segment to curve" command.** A line becomes a cubic only as a side effect of molding or smoothing; a quadratic becomes a line on sharp conversion.
- **No symmetric node type as stored state.** Equal-length handles exist only while Alt is held; the stored flag is colinear (G1) or not.
- **No weld of coincident endpoints** in Join; it adds a segment.
- **No reverse path direction, simplify, or "insert point at midpoint / at numeric t"** command in this tool.
- **No "point follows cursor" molding**; both handles are translated by the raw delta (2.7).
- **No snapping** for molding, segment drags, or point sliding.
- **No pivot / transform cage for a point selection** in this tool; R / S come from the global G/R/S operation.
- **No offset on Duplicate.**
- **No spatial index** for hit-testing; it is a linear scan per selected layer (TODO at SE:1795).
- Copy of selected segments is flagged TODO at PT:2773, though the code below it does include selected segments.

### 2.17 Porting notes (Y-up, arcs) [inferred]

- All hit tolerances are viewport pixels and all hit tests are done in viewport space; geometry edits are converted to layer space through the inverse layer-to-viewport transform. DreamCarve can keep the same split: test in screen space, edit in millimetres.
- Graphite document and viewport space are Y-down. Things that carry a sign: arrow-key mapping (2.11), the Tab "next handle" sense, the `+90°` rotation used when smoothing an anchor (`FRAC_PI_2` at SE:1056) and in Ctrl+Alt lock (PT:3429). Each is followed by an explicit flip test or is symmetric, but they should be re-derived, not copied.
- Handle logic assumes per-segment Bezier handles. For DreamCarve's line + bulge-arc polylines, the directly reusable parts are: the hit-test order and tolerances, the selection rules, box/lasso inclusion, the drag/axis-lock/snap contract, nudging, break / delete-and-break / join, and insert-at-nearest-point. Handle mirroring, molding, smooth/sharp, and point sliding apply only to Bezier spans.

---

## 3. Snapping system and grid

Sources (paths relative to the Graphite root, commit `615b4050ba4d247712dfd2fca745d87a9bba315f`):

- `libraries/snapping/` — a standalone crate (`snapping`), the geometry and the solver.
- `editor/src/messages/tool/common_functionality/snap_session.rs` + `snap_session/{settings,presenter,scene_mirror}.rs` — the editor-side session that every tool calls.
- `editor/src/messages/portfolio/document/utility_types/misc.rs` — `SnappingState`, `GridSnapping`, the options popover table.
- `editor/src/messages/portfolio/document/overlays/grid_overlays.rs` — grid drawing and the grid options panel.

Tags: **[read]** = taken directly from the code at the cited lines. **[inferred]** = my reading of how pieces combine, not stated in one place.

Note on vintage: this checkout does not contain the older `snapping/` folder with `SnapManager`, `LayerSnapper`, `GridSnapper`, `AlignmentSnapper`, `DistributionSnapper`, `SnapSource`/`SnapTarget` enums. Anything remembered from that design (e.g. `SNAP_POINT_TOLERANCE`) is superseded. `SNAP_POINT_TOLERANCE: f64 = 5.` still exists at `editor/src/consts.rs:46` but the snapping library does not use it — reach is `Tunables::reach = 8.` (`libraries/snapping/src/tunables.rs:26`). The const now serves only tool-local endpoint proximity tests: Pen close/continue-path checks (`tool_messages/pen_tool.rs:806,833,860,917,1294`), Freehand's `closest_open_path_endpoint` (`freehand_tool.rs:287`), and the endpoint hover overlay (`portfolio/document/overlays/utility_functions.rs:291`) **[read: grep of `editor/src`]**.

### 3.1 Architecture in one paragraph

**[read]** The crate "finds the relationships a moving point can take with the geometry around it. Geometry is solved in a work plane's coordinates and ranked by nearness on screen" (`libraries/snapping/src/lib.rs:1-3`). A gesture is described by *features* (points of the moving thing) that each follow two solved *parameters* through an affine `FeatureMap`. Every way a feature could snap is a `Candidate` whose shape is either a `Spot` (a point, fixes both freedoms) or a `Track` (a `Locus`: infinite line, circle, or bounded curve; leaves one freedom). The solver ranks spots, single tracks, and crossings of two tracks by screen distance, with hysteresis from the previously engaged snap, optional cycling, an optional required track (constraint), and a grid that takes up whatever freedom is left.

### 3.2 Crate facts and portability

**[read]** `libraries/snapping/Cargo.toml:17-22`: dependencies are only `glam` and `kurbo` (workspace versions: kurbo 0.13, glam 0.34 per root `Cargo.toml:214,225`); dev-dependency `criterion`. `license.workspace = true` → `MIT OR Apache-2.0` (root `Cargo.toml:74`). No editor types, no serde, no I/O. It is not published (`publish = false`).

Modules and sizes (non-test code ends where `#[cfg(test)]` starts):

| Module | Lines (code / total) | Content | Pure geometry? |
|---|---|---|---|
| `tunables.rs` | 33 | the five distances | yes |
| `settings.rs` | 44 | 15 per-kind booleans | yes |
| `candidate.rs` | 116 | `Kind`, `CandidateKey`, `Shape`, `Highlight`, `MatchedGaps`, `Candidate` | yes |
| `view.rs` | 103 / 197 | `ViewMapping` (homography plane↔screen, Jacobian, reach box) | yes (glam) |
| `frame.rs` | 54 / 98 | `Frame` (origin + axis directions; `nearest_axis`, `nearest_angle_step`) | yes |
| `bounds.rs`, `bounds_tree.rs` | 85 / 126, 99 / 186 | AABB and a static bounding-volume tree | yes |
| `scene.rs` | 276 / 532 | `Scene`, `SceneLayer`, `Exclusions`, spatial queries | yes |
| `curve.rs` | 786 / 1414 | `Curve` over `kurbo::PathSeg`: nearest, tangent/perpendicular feet, line/circle/curve intersections, polynomial root finder | yes (kurbo+glam) |
| `locus.rs` | 239 / 438 | `Locus::{Line, Circle, Curve}`, nearest, nearest-as-seen, crossings | yes |
| `tangency.rs` | 105 / 202 | `common_tangents(curve, curve)` by seeded Newton | yes |
| `grid.rs` | 166 / 316 | `Grid` of N line families; nearest point, crossings along a line | yes |
| `find.rs` | 202 / 464 | candidates near a probe | yes |
| `constructions.rs` | 251 / 635 | candidates from "awake" targets | yes |
| `spacing.rs` | 368 / 746 | equal gaps and equal sizes for boxes | yes |
| `solve.rs` | 648 / 1628 | the solver | yes |
| `snap.rs` | 213 | entry points `snap_point`, `snap_slide`, `snap_features` | yes |

**[inferred]** The whole crate is portable as-is to a kurbo-0.13 codebase; nothing in it knows about layers beyond opaque `u64` IDs. The only editor-specific code is `snap_session*.rs` (mirroring, dwell-to-wake, key steering, overlay drawing). For DreamCarve the crate would need arcs flattened to cubic `PathSeg`s or a new `Locus`/`Curve` variant, since `Curve` wraps only `kurbo::PathSeg` (line/quad/cubic) **[read: `curve.rs:41-70`]**.

Y-axis note: **[read]** the library is orientation-agnostic ("nothing here assumes the plane is the screen or the document", `lib.rs:3`); `Frame::default()` is axes `[X, Y]` (`frame.rs:12-19`). **[inferred]** The places where Y-down leaks in are editor-side: `Pivot::Start/End` label placement and "above" = `marked - (0, 4)` (`presenter.rs:73`), the artboard "top left corner" = bbox `min` (`misc.rs:330`), isometric slopes `[-tan(a), +tan(b)]` and the dot columns stepping by `(1, -tan_a)` (`settings.rs:52`, `grid_overlays.rs:135,157`), and the nudge rotated by `-tilt` (`snap_session.rs:747`). Angle steps are measured with `perp_dot`/`atan2` from the first frame axis (`frame.rs:49`), so sign conventions follow the plane.

### 3.3 Data model

**[read]**

- `Scene` (`scene.rs:130-137`): `BTreeMap<LayerId, IndexedLayer>`. Each `SceneLayer` (`scene.rs:52-59`) has `parent: Option<LayerId>`, `kind: LayerKind::{Artwork, Artboard}`, `quad: Option<[DVec2; 4]>` (oriented bounding-box corners "in order around it, oriented with the layer"), `anchors: Vec<SceneAnchor>`, `curves: Vec<SceneCurve>`.
- `SceneAnchor { position, id: Option<AnchorId>, role }`, `SceneCurve { curve, ends: [Option<AnchorId>; 2], role }` (`scene.rs:36-48`).
- `GeometryRole::{Source, Undrawn, Output}` (`scene.rs:20-27`): Source = edited directly and drawn as is; Undrawn = edited but drawn as something else (visible only while the layer is edited); Output = drawn but not what is edited.
- `Exclusions { layers, anchors: HashMap<LayerId, HashSet<AnchorId>>, edited }` (`scene.rs:73-80`).
- `Context { scene, view: &ViewMapping, frame: &Frame, tunables, settings, exclusions, awake: &[Awake] }` (`find.rs:34-42`).
- `Awake::{Anchor{layer,index}, Curve{layer,index}, Box{layer}, Position{id,position}}` (`find.rs:13-30`): "A target allowed to offer snaps that are far from its own geometry."
- `Probe { feature: u32, position, previous: Option<DVec2> }` (`find.rs:61-66`).
- `FeatureMap { matrix: DMat2, offset: DVec2 }`, `position = matrix * parameters + offset` (`solve.rs:18-23`). Constructors: `IDENTITY` (place a point), `sliding_from(start)` (translate), `stretching(start, pivot, axes)` (parameters are the two scale factors), `turning_about(start, lead_start, pivot)` (rotate+uniform-scale; parameters are where the lead goes), `following(start, lead_start, fraction)` (skew) (`solve.rs:25-95`).
- `Candidate { key: CandidateKey{kind, feature, identity: [u64;3]}, shape: Shape::{Spot(DVec2), Track(Locus)}, highlight: Option<Highlight>, reference: Option<DVec2>, gaps: Option<MatchedGaps> }` (`candidate.rs:38-82`).
- `Request { grid, required: Option<Locus>, incumbent, cycle: usize, quiet: bool, constructions, lead_constructions, particular, probed: Option<&[u32]>, crossings: Option<&CrossingCache> }` (`snap.rs:11-33`).
- `Outcome { parameters: DVec2, snap: Snap::{Free, Grid, Engaged(Engagement)}, candidates: Vec<Candidate> }` (`snap.rs:36-43`, `solve.rs:191-196`).
- `Engagement { key: EngagementKey::{Single(k), Crossing(k1,k2)}, tier: Tier::{Spot, Track}, candidates: Vec<usize>, distance, quantized: bool, pinned: bool, riding: Option<CandidateKey>, alternatives: usize }` (`solve.rs:198-215`).

Entry points (`snap.rs:73-89`):

```rust
pub fn snap_point(context: &Context, request: &Request, free: DVec2, previous: Option<DVec2>) -> Outcome
pub fn snap_slide(context: &Context, request: &Request, feature_starts: &[DVec2], lead: usize, free: DVec2) -> Outcome
pub fn snap_features(context: &Context, request: &Request, features: &[FeatureMap], lead: usize, free: DVec2, previous: Option<DVec2>) -> Outcome
```

There is no separate "snap source" enum. **[read]** What the moving thing contributes is a list of features (points); the library treats every feature identically, except that the `lead` feature (the one the pointer carries) alone gets `previous`-based candidates and lands on the grid (`snap.rs:79,110-118`, `solve.rs:173`). The *caller* decides which points are features (see 3.9).

### 3.4 Every snap kind (`Kind`, `candidate.rs:6-35`)

**[read]** The enum order is the specificity order used for tie-breaks: "The order is from most to least specific, and it settles ties between candidates that are equally near."

| # | `Kind` | Shape | Produced by | Needs awake? | Setting | Overlay label (`presenter.rs:163-191`) |
|---|---|---|---|---|---|---|
| 1 | `Anchor` | Spot | `find.rs:91-100` — every scene anchor within gathering distance | no | `anchors` | "Anchor" |
| 2 | `Midpoint` | Spot | `find.rs:114-117` — midpoint of a **straight** curve only (`(start+end)/2`) | no | `midpoints` | "Midpoint" |
| 3 | `Between` | Spot | `constructions.rs:164` — halfway between two awake points | yes (2 points) | `constructions` | "Halfway" |
| 4 | `TangentPoint` | Spot | `find.rs:182,187` — where a line from `previous` touches a curve; `constructions.rs:188,193` — touch points of a common tangent of two awake curves | previous point, or 2 awake curves | `tangents` | "Tangent" |
| 5 | `PerpendicularFoot` | Spot | `find.rs:182,190` — foot of the perpendicular from `previous` | previous point | `perpendiculars` | "Perpendicular" |
| 6 | `BoxCorner` | Spot | `find.rs:142-144` — the 4 quad corners | no | `box_corners` | "Corner" |
| 7 | `BoxEdgeMidpoint` | Spot | `find.rs:145-147` | no | `box_edge_midpoints` | "Edge Midpoint" |
| 8 | `BoxCenter` | Spot | `find.rs:155-158` — `(quad[0]+quad[2])/2` | no | `box_centers` | "Center" |
| 9 | `ArtboardCorner` | Spot | `find.rs:126-143` (artboard layers) | no | `artboards` | "Artboard Corner" |
| 10 | `ArtboardCenter` | Spot | same | no | `artboards` | "Artboard Center" |
| 11 | `OnCurve` | Track (bounded curve) | `find.rs:110-112` | no | `curves` | "On Path" |
| 12 | `OnBoxEdge` | Track (bounded line) | `find.rs:149-152` | no | `box_edges` | "On Edge" |
| 13 | `OnArtboardEdge` | Track | same, artboards | no | `artboards` | "On Artboard Edge" |
| 14 | `Extension` | Track (infinite line) | `constructions.rs:130-143` — tangent line from the end(s) of an awake curve; a straight curve gives one line, a bent one gives one per end | awake curve | `extensions` | "Extension" |
| 15 | `TangentLine` | Track (infinite line) | `find.rs:181` — line from `previous` through each tangent landing | previous | `tangents` | "Tangent" |
| 16 | `PerpendicularLine` | Track | `find.rs:181,198` — line from `previous` through each foot; plus, if `previous` lies on the curve, the normal leaving from there | previous | `perpendiculars` | "Perpendicular" |
| 17 | `Parallel` | Track | `constructions.rs:238-240` — through `previous`, parallel to an awake straight curve (skipped if `previous` is already on that curve's line) | previous + awake straight curve | `constructions` | "Parallel" |
| 18 | `RightAngle` | Track | `constructions.rs:241-243` — through `previous`, perpendicular to an awake straight curve | same | `constructions` | "Right Angle" |
| 19 | `Through` | Track | `constructions.rs:163` — infinite line through two awake points | 2 awake points | `constructions` | "In Line" |
| 20 | `CommonTangent` | Track | `constructions.rs:183` — line touching two awake curves | 2 awake curves | `tangents` | "Tangent to Both" |
| 21 | `Alignment` | Track | `constructions.rs:82-89,105-128,217-219` — a line along each frame axis through: each awake point; the 4 corners + centre of each awake box (lines that coincide offered once); and through `previous` | awake point/box, or previous | `point_alignment` / `box_alignment` | "Aligned" (two at once: "Aligned Both Ways") |
| 22 | `EqualLength` | Track (circle about `previous`) | `constructions.rs:221-225,244-246` — radius = length of the previous segment, or of an awake straight curve | previous (+ optional awake) | `equal_lengths` | "Equal Length" |
| 23 | `AngleStep` | Spot (1-D value) | editor gizmos only via `snap_value` (`gizmos/shape_gizmos/sweep_angle_gizmo.rs:384`, `spiral_turns_handle.rs:192`) | n/a | none | "Angle" |
| 24 | `Spacing` | Track (line across the axis) | `spacing.rs:251-316` | neighbours found automatically | editor `distribute_evenly` | "Evenly Spaced" |
| 25 | `EqualSize` | Track | `spacing.rs:319-367` | nearest neighbours + awake boxes | editor `equal_sizes` | "Equal Size" |
| 26 | `EqualRadius` | Spot (1-D value) | editor gizmo only (`circle_arc_radius_handle.rs:240`) | n/a | none | "Equal Radius" |
| 27 | `WholeValue` | Spot (1-D value) | editor gizmo only (`spiral_turns_handle.rs:192`) | n/a | none | "Whole Number" |

Path intersections are not a `Kind`: **[read]** they arise in the solver as a `Crossing` of two tracks (`solve.rs:530-566`), with `kind = first.kind.min(second.kind)` (`solve.rs:558`). The label for two `On*` tracks is "Intersection" (`presenter.rs:155`). Any two tracks can cross (e.g. `Alignment` × `OnCurve`, `Extension` × `Extension`, `EqualLength` circle × `Parallel`), label "`A and B`" (`presenter.rs:158`).

The "awake" mechanism: **[read]** candidates from `find_near_probe` exist only within the gathering distance of the probe. Relationships that act at a distance (alignment, extension, parallel, through, common tangent, equal length to a segment) come only from targets that are *awake*. A target wakes when the pointer rests on a snap to it (3.8).

### 3.5 Candidate generation

**[read]** `find_near_probe` (`find.rs:69-160`), run once per probed feature at the position the pointer asks for:

1. `gathering_distance = tunables.gathering_distance()` = `release * SQRT_2` = 12·√2 ≈ 16.97 px (`tunables.rs:18-20`). "Candidates are gathered from further than reach so that an engaged snap, or one along a ridden track, can still be recognized" (`find.rs:79`).
2. `area = view.plane_bounds_within_reach(probe.position, gathering_distance)` — a conservative plane-space AABB of the screen square (`view.rs:70-94`); the scene's trees are queried with it.
3. A point is `nearby` when `view.screen_distance(probe.position, p) <= gathering_distance` (`find.rs:82`). A curve `passes_nearby` when its nearest-as-seen point is nearby (`find.rs:83-84`).
4. Anchors → `Kind::Anchor` spots. Identity uses the anchor's own ID when it has one (stable across reordering) (`find.rs:95`).
5. Curves → `OnCurve` track; `Midpoint` spot if `curve.is_straight()`; and if the probe has a `previous`, `find_lines_from_previous` (`find.rs:163-201`): `curve.tangent_parameters_from(previous)` → `TangentLine` track + `TangentPoint` spot per root; `curve.perpendicular_parameters_from(previous)` → `PerpendicularLine` + `PerpendicularFoot`; landings within `1e-9 * bounds diagonal + 8·EPSILON·|previous|` of `previous` are skipped (`find.rs:170,176`).
6. Boxes → for each of 4 corners: corner spot, edge-midpoint spot (not for artboards: `find.rs:131-135`), edge track; then the centre spot.

`find_constructions(context)` (`constructions.rs:98-208`) and `find_constructions_from(context, previous, previous_length)` (`constructions.rs:213-250`) return position-independent candidates; the caller caches them and passes them in `Request::constructions` (offered to every probed feature) and `Request::lead_constructions` (lead only) (`snap.rs:118-123`). `find_spacing` / `find_equal_sizes` results go in `Request::particular` (already bound to a feature).

Then in `snap_features` (`snap.rs:89-192`):

- **[read]** The probe position is computed from `base = confine(view, lead, required, free)` — under a required track the candidates are gathered near where the pointer *falls on the track*, not near the pointer (`snap.rs:90-92`).
- `quiet` or a non-finite base returns `Snap::Free` with only the required track applied (`snap.rs:94-100`).
- De-duplication by key when constructions are present (`snap.rs:130-133`).
- Each candidate is transformed into parameter space through its feature's inverse map (`snap.rs:137-154`); a candidate that cannot be expressed becomes `Spot(NAN)` so indices stay aligned.
- Candidates of the same `(kind, identity)` for different features that land alike (within `1e-9` relative) are collapsed, keeping the lead's copy (`snap.rs:156-170`, `lands_alike` at `snap.rs:195-213`).

### 3.6 The solver (`solve.rs:320-407`)

**[read]** Steps, in order:

1. `base = confine(...)`. With `required`, `resolved = resolve_along(...)`; else `resolve_freely(...)` (`solve.rs:333-337`).
2. Drop anything that would put the lead on `avoid` (= `previous`; "where a segment is drawn from is no place for its other end") within `on_track` px (`solve.rs:339`, `297-300`).
3. `within_reach` = resolved with `reach_distance <= reach`, sorted by `listing_order` (`solve.rs:341-342`).
4. `challenger = preferred(within_reach, tie, lead)`.
5. `held = hold(problem, resolved)` — the incumbent, if it still exists within `release`.
6. Decide between held and challenger (hysteresis rule below).
7. Apply cycling (`W` key).
8. If nothing is chosen: land on the grid (`Snap::Grid`) or stay free.
9. If the chosen is a single straight track: quantise along it to the nearest grid-line crossing (`quantized = true`).

`resolve_freely` (`solve.rs:454-569`):

- Every Track candidate → `Resolved{tier: Track, parameters: nearest-as-seen point on it, distance}`; those within `gathering_distance` become `NearbyTrack`s; then "A track beyond the release distance can only matter where it crosses the one being ridden" (`solve.rs:490-492`).
- Every Spot candidate → `Resolved{tier: Spot}`. Its `reach_distance` is shortened to the distance *along a ridden track* if the spot lies on that track (within `on_track`) (`solve.rs:503-504`): a point riding a path catches a midpoint further along it.
- Crossings: nearby tracks sorted by distance, then capped per sort: `MAX_CROSSING_CURVES = 16` for curve loci, another 16 for `TangentLine`/`PerpendicularLine`, `MAX_CROSSING_LINES = 24` for all other lines/circles (`solve.rs:13,16,517-527`); ridden and held tracks are exempt. All pairs O(n²); pairs need both within `release` or one ridden (`solve.rs:532-535`). Two lines through the same point are skipped (`solve.rs:538-542`). A crossing that is only two curves joined end to end is skipped — that is an anchor (`joins_end_to_end`, `solve.rs:572-582`). Each crossing → `Resolved{tier: Spot, kind: min(kinds), key: Crossing(a,b)}`.
- Curve×curve crossings are memoised in `CrossingCache` keyed by the ordered key pair (`solve.rs:143-164`).

`resolve_along` (`solve.rs:585-610`) — constrained case: a Spot counts only if it lies on the required track within `on_track`; a Track contributes `required.crossings(&locus)`; everything is `Tier::Spot` and measured from `base`.

Ranking — quoted verbatim:

```rust
// solve.rs:256-264
fn listing_order(&self, other: &Self) -> Ordering {
	self.tier
		.cmp(&other.tier)
		.then(self.distance.total_cmp(&other.distance))
		.then(self.kind.cmp(&other.kind))
		.then(self.key.cmp(&other.key))
		.then(self.parameters.x.total_cmp(&other.parameters.x))
		.then(self.parameters.y.total_cmp(&other.parameters.y))
}
```

```rust
// solve.rs:409-417
/// The pick from a list in listing order: the nearest of the best tier, unless a more specific kind, or one of the same kind for the lead feature, lies within the tie distance of it.
fn preferred<'r>(listed: &[&'r Resolved], tie: f64, lead: u32) -> Option<&'r Resolved> {
	let nearest = *listed.first()?;
	let tied = listed.iter().take_while(|candidate| candidate.tier == nearest.tier && candidate.distance <= nearest.distance + tie);

	let not_for_the_lead = |candidate: &Resolved| !candidate.key.involves(lead);
	tied.min_by(|a, b| a.kind.cmp(&b.kind).then(not_for_the_lead(a).cmp(&not_for_the_lead(b))).then(a.listing_order(b)))
		.copied()
}
```

So the priority is: **(1) tier** — any Spot (point or crossing) within reach beats any single Track, regardless of distance; **(2) screen distance**; **(3) within `tie` = 0.5 px of the nearest, the lower `Kind` ordinal wins** (Anchor > Midpoint > Between > TangentPoint > … > OnCurve > … > Alignment …); **(4) then a candidate for the lead feature**; then deterministic key/parameter order. **[inferred]** Consequence: an anchor 7 px away loses to a box corner 3 px away; kind order matters only for near-coincident candidates (e.g. a rectangle's anchor vs its box corner at the same place → "Anchor").

Hysteresis — quoted:

```rust
// solve.rs:345-354
// Unless pinned, the engaged snap is unseated by a better tier or by anything of its own tier that is nearer by a clear margin
let held = hold(problem, &resolved);
let natural = match (held, challenger) {
	(Some((held, pinned)), Some(challenger)) => {
		let unseats = |rival: &&Resolved| rival.tier < held.tier || (rival.tier == held.tier && rival.distance + switch_margin < held.distance);
		if !pinned && within_reach.iter().any(unseats) { Some(challenger) } else { Some(held) }
	}
	(Some((held, _)), None) => Some(held),
	(None, challenger) => challenger,
};
```

**[read]** `hold` (`solve.rs:420-434`): the incumbent survives while a resolved candidate with the same `EngagementKey` has `reach_distance <= release` (12 px); among several crossings of the same pair, the one nearest the incumbent's last parameters. If the incumbent spot/crossing is gone, the point "carries on along the track it was riding" (not pinned). Engage at 8 px, release at 12 px, and a same-tier rival must be nearer by more than 2 px.

Riding: **[read]** when a single track is engaged it becomes `riding`; when a spot or crossing takes over, the previously ridden track stays `riding` as long as the pointer is within `release` of it (`solve.rs:383-392`, `472`). This is what lets spots along a path be caught from further off and released without flicker (scenario `a_point_riding_a_path_from_well_off_it_catches_the_midpoint_and_lets_it_go_without_flickering`, `libraries/snapping/tests/scenarios.rs:649`).

Cycling: **[read]** `cycle > 0` steps that many entries on through `within_reach` (wrapping) from the one that would be shown; the result is `pinned` and stays engaged against all rivals while it holds (`solve.rs:356-363`). `Engagement::alternatives` = how many are within reach.

Grid in the solver: **[read]** the grid is the lowest priority. "With nothing engaged, the grid decides where the feature lands along whatever freedom remains" (`solve.rs:366-375`): free → nearest grid *point* for the lead feature (`land_on_grid`, `solve.rs:622-631`); under a required track → nearest crossing of the track with a grid *line* (`land_on_grid_along`, `solve.rs:635-647`, straight tracks only). When a single straight Track is engaged, the position along it is also moved to the nearest grid-line crossing (`solve.rs:377-381`). **[inferred]** There is no reach test for the grid: with grid snapping on and nothing else within 8 px, the lead always lands on a grid point (scenario `with_the_grid_on_a_point_lands_on_it_unless_something_real_is_within_reach`, `scenarios.rs:495`). There is no snap to a single grid line in free placement (always the intersection point); the options tooltip says "Snaps to the points where the drawn grid lines cross" (`misc.rs:420`).

### 3.7 Tolerances and tunables (verbatim)

All distances are **screen (viewport) pixels**, compared through `ViewMapping::screen_distance` (`view.rs:46-49`), so they are zoom-independent by construction; scenario `the_same_gesture_on_screen_gives_the_same_snap_at_any_zoom` (`scenarios.rs:508`). **[read]**

| Constant | Value | Where | Meaning |
|---|---|---|---|
| `Tunables::reach` | `8.` | `libraries/snapping/src/tunables.rs:26` | px within which a snap can engage |
| `Tunables::release` | `12.` | `tunables.rs:27` | px beyond which an engaged snap lets go |
| `Tunables::switch_margin` | `2.` | `tunables.rs:28` | px a same-tier rival must be nearer by to take over |
| `Tunables::on_track` | `0.5` | `tunables.rs:29` | px a position may be off a track and still count as on it |
| `Tunables::tie` | `0.5` | `tunables.rs:30` | px within which a more specific kind is preferred to the nearest |
| `gathering_distance()` | `release * SQRT_2` (≈16.97) | `tunables.rs:18-20` | px radius candidates are gathered from |
| `MAX_CROSSING_CURVES` | `16` | `solve.rs:13` | nearest curve tracks (and, separately, tangent/perpendicular lines) tried for crossings |
| `MAX_CROSSING_LINES` | `24` | `solve.rs:16` | nearest other lines/circles tried for crossings |
| `CHANGES_BEFORE_REBUILD` | `32` | `scene.rs:8` | changed layers tolerated before the layer tree is rebuilt |
| `LEAF_SIZE` (tree) | `8` | `bounds_tree.rs:4` | items per leaf |
| `PARALLEL_SINE` | `1e-12` | `locus.rs:132` | lines treated as never meeting |
| `SAME_FAMILY_SINE` | `1e-6` | `grid.rs:4` | two grid families counted as one |
| `NEVER_CROSSING_SINE` | `1e-12` | `grid.rs:7` | line parallel to a grid family |
| `ON_A_LINE` | `1e-6` | `grid.rs:10` | fraction of spacing counted as on a grid line (nudging) |
| `SEEDS_PER_CURVE` / `MAX_ITERATIONS` | `12` / `30` | `tangency.rs:6-7` | common-tangent Newton search |
| `COINCIDENCE` | `1e-9` | `curve.rs:7` | relative zero distance |
| `NEGLIGIBLE` | `1e-12` | `curve.rs:10` | relative zero coefficient |
| `PARAMETER_SLACK` | `1e-9` | `curve.rs:13` | roots this far outside [0,1] count as the end |
| `ROOT_SEPARATION` | `1e-9` | `curve.rs:16` | |
| `INTERSECTION_SEPARATION` | `1e-6` | `curve.rs:19` | |
| `LEAF_SIZE` (curve) | `1e-6` | `curve.rs:22` | subdivision stop, fraction of combined diagonal |
| `MAX_SUBDIVISION_DEPTH` | `40` | `curve.rs:24` | |
| `MAX_LIVE_PAIRS` | `4096` | `curve.rs:27` | above this the curves are treated as coincident |
| `MAX_ROOT_REFINEMENT_STEPS` / `MAX_NEWTON_STEPS` / `NEWTON_MARGIN` | `100` / `50` / `1e-3` | `curve.rs:29,31,34` | |

Editor session (`editor/src/messages/tool/common_functionality/snap_session.rs`):

| Constant | Value | Line | Meaning |
|---|---|---|---|
| `WAKE_DWELL_IN_GESTURE` | `250` ms | `:27` | rest time to wake a target during a gesture |
| `WAKE_DWELL_ON_APPROACH` | `500` ms | `:30` | rest time to wake before any gesture |
| `REST_DRIFT` | `4.` px | `:33` | pointer drift still counted as resting |
| `AWAKE_LIMIT` | `5` | `:36` | woken targets kept (oldest dropped) |
| `NEAREST_SIBLINGS_AWAKE` | `3` | `:39` | sibling boxes auto-awake while a box moves |
| `DRAG_THRESHOLD` | `1.` px | `editor/src/consts.rs:48` | nothing snaps until the pointer has moved this far from the press (`snap_session.rs:602-606`) |
| `GAP_CAP_REACH` | `4.` px | `snap_session/presenter.rs:11` | half-length of the cap on an equal-gap marker |
| `MAX_SNAPPING_POINTS` | `16` | `tool_messages/path_tool.rs:574` | selected points used as features in a point drag |
| `MAX_SNAPPING_ANCHORS` | `8` | `common_functionality/utility_functions.rs:245` | dragged-layer anchors added as features in a layer drag |
| `MAX_LAYERS_WITH_SNAPPING_ANCHORS` | `8` | `utility_functions.rs:248` | above this many dragged layers, no anchors are added |
| `MIN_FEATURE_TRAVEL` | `1e-9` | `common_functionality/transformation_cage.rs:337` | features that barely move under a turn/skew are left out |
| `ANGLE_STEP_DEFAULT` | `15.`° | `portfolio/document/utility_types/misc.rs:11` | default constrained-angle step |
| `ANGLE_STEP_MIN` / `ANGLE_STEP_MAX` | `1.` / `90.`° | `misc.rs:13,15` | allowed range of the per-document angle step |
| `GRID_MIN_SPACING_IN_VIEWPORT` | `10.` px | `misc.rs:8` | minimum on-screen spacing of major grid lines |

`Tunables::default()` and `Frame::default()` are hard-coded at every call (`snap_session.rs:246`); **[read]** there is no user preference for snap distance.

### 3.8 Editor session behaviour (`SnapSession`)

**[read]** API a tool uses (`snap_session.rs:197-615`):

- `snap_point(view, free, PointOptions) -> DVec2` and `snap_features(view, features, lead, free, PointOptions) -> DVec2` (document space / parameters).
- `snap_value(view, position_of, free, on_screen_per_unit, &[ValueCandidate]) -> f64` — 1-D snapping of a gizmo value: solved as a point on the X axis with `required = line(0, X)` and a view scaled by pixels-per-unit, in an empty scene (`:347-403`).
- `begin_gesture(view, Moving)` — freezes the scene as it is; `begin_gesture_following_edits` — keeps mirroring document changes (the Pen uses this so each placed anchor becomes a target: `pen_tool.rs:575`; Select uses it when dragging copies: `select_tool.rs:657`).
- `snap_press(view, pointer)` — a press lands on what was *shown* on hover, else snaps now (`:492-496`); `land_press`.
- `end_gesture()`, `forget()`, `shown()`, `caught()`, `shown_position()`, `tick(now)`, `wants_ticks()`, `sleep_all()`.
- `PointOptions { previous, previous_length, required: Option<Locus>, snapping: Option<usize>, box_features: Option<BoxFeatures>, ignores_grid: bool }` (`:81-95`).
- `Moving { layers, copied, edited, anchors: Vec<(layer, PointId)>, drawn: Option<Drawn{parent, artboard}> }` (`:57-68`).

Steering keys (`:598-615`): **[read]**

- **Q held** = "Quiet Snapping": no candidates, only the required track applies. Hint text `"Quiet Snapping"` (`:775`).
- **W tapped** = "Cycle Snap": steps to the next candidate within reach; edge-triggered ("A tap of the cycling key counts once").
- Master switch `snapping_state.snapping_enabled`; toggled by Alt+S (`input_mappings.rs:353`). Grid visibility Alt+G (`input_mappings.rs:356`).
- Nothing snaps until the pointer is more than `DRAG_THRESHOLD` (1 px) from the press; until then the lead feature is held where the press landed (`:311-317`).
- Q and W are in the `refresh_keys` of the pointer-move mappings of Select, Artboard, Text, Gradient, Shape, Path, Pen, Spline, TransformLayer (`input_mappings.rs:109,112,122,170,180,194,238,278,300`). Freehand's has none.

Hover preview: **[read]** tools call `snap_point` on plain pointer moves before a press; with no gesture and `Snap::Free`/`Snap::Grid` the presenter draws a 4 px blue square where the press would land (`presenter.rs:24-31`).

Waking by dwell (`:531-556`, `:652-693`): **[read]** if the pointer stays within `REST_DRIFT` of where its rest began for the dwell time while engaged on a snap, the targets of the engaged candidates are added to `awake`: the anchor, the curve, or the box of each `Highlight`. A `Midpoint`, or a crossing of two `On*` tracks, wakes as an `Awake::Position` (a point of its own, ID = hash of the relationships and position) (`:662-682`). At most 5 awake; a rest is "spent" after waking. Ticks arrive on animation frames only while wanted (`tool_message_handler.rs:293,483`). Woken targets are put back to sleep when their layer changes (`:719-724`), on tool switch (`tool_message_handler.rs:191`), on Escape with nothing under way in Select/Pen/Path/Spline/Shape/Gradient (`select_tool.rs:2037`, `pen_tool.rs:260`, `path_tool.rs:2417`, `spline_tool.rs:583`, `shape_tool.rs:553`, `gradient_tool.rs:1683`).

Automatic awake boxes while a box moves (`:618-649`): **[read]** the 3 nearest sibling boxes (same parent, same kind, by AABB gap distance), any sibling box the current snap relies on, and the enclosing artboard are awake without dwell. This is what gives edge/centre alignment guides during a plain drag.

Scene mirroring (`snap_session/scene_mirror.rs`): **[read]**

- Hidden layers offer nothing (`:78-80`). Groups (layers with children) offer nothing themselves (`:101-103`). Locked layers: **not checked here [read: no lock test in `mirror_layer`]**.
- Artboard → kind `Artboard`, upright quad only, no anchors/curves (`:89-98`).
- Other layers: `quad` = local bounding box transformed to document (so it is *oriented with the layer*, not axis-aligned) (`:105-108`); curves = every segment of the layer's rendered outline; anchors = every segment end of the outline (`:112-119`, `:181-186`).
- If the layer has an editable path, its anchors (with `PointId`) and segments are added with role `Source` if they coincide with what is drawn (rounded to 1/1024 unit, `:189-191`), else `Undrawn`; the remaining drawn geometry is role `Output` (`:125-168`).
- Unchanged layers are detected (1e-9 relative) and not re-indexed (`:26`, `:43-63`).
- **Handles are never mirrored** — only anchors and segment curves. **[read]**

Exclusions built from `Moving` (`snap_session.rs:431-441`, `scene.rs:82-116`): **[read]**

- `layers`: moved layers and all descendants → skipped entirely.
- `anchors`: per layer, moving `AnchorId`s → those anchors are skipped, and so is every curve with a moving end; the layer counts as edited.
- `edited`: the layer keeps `Source` and `Undrawn` geometry as targets, loses `Output` geometry and its bounding box. (A dragged anchor snaps to the rest of its own path but not to itself: `scenarios.rs:407`.)
- `copied` layers stay put and are normal targets.
- Awake `Position`s found on moving/edited layers are dropped (`:249-259`).

### 3.9 What tools pass as features (snap "sources")

**[read]** from the call sites:

- **Single point** (Pen anchor/handle, Path insertion, line end, gradient stop): `snap_point` = one `IDENTITY` feature.
- **Mirrored pair**: Pen colinear handle (`pen_tool.rs:1333-1341`) and Line drawn from centre (`shapes/line_shape.rs:146-153`) pass `[IDENTITY, {matrix: -I, offset: 2·anchor}]` so either end may snap.
- **Layer drag** (Select, `select_tool.rs:566-604`): 4 cage corners + 4 edge midpoints + centre, then up to 8 of the dragged layers' own anchors nearest the grip (`push_nearest_anchors`), all `sliding_from`. `lead` = the corner nearest the grip. `box_features.sides` = for each document axis, the corner furthest toward each end. So there is no separate "bounding box snapping mode" — a box is 9 features plus `BoxFeatures`.
- **Point drag** (Path, `path_tool.rs:1025-1034`): the grabbed point first (lead), then up to 15 nearest other selected points; `previous` supplied for tangent/perpendicular lines.
- **Drawing a box-like shape** (`resize.rs:106-150`): corners + edge middles + centre as `stretching` features about the drag start (or centre with Alt), lead = the pointer's corner (index 2). For shapes with their own points (ellipse etc.) only the shape's points and centre snap (`snapping: Some(n)`), while the pointer corner still leads and lands on the grid.
- **Cage resize** (`transformation_cage.rs:205-268`): `stretching` about the pivot; an edge drag requires `line((1,0), Y)` or `line((0,1), X)` in scale space; Shift (constrain) requires `line(0, free.signum())`.
- **Cage rotate** (`transformation_cage.rs:806-834`): `turning_about(start, grip, pivot)`, `snapping: Some(lead)`, **`ignores_grid: true`**.
- **Cage skew** (`transformation_cage.rs:848-873`): `following(start, lead_start, fraction)`.
- **G/R/S transform** (`transform_layer_message_handler.rs:825-864`): same three maps, up to `MAX_SNAPPING_POINTS` starts.
- **Artboard drag** (`artboard_tool.rs:161-203`): corners + middles + centre, with `box_features`.

Constrained snapping: **[read]** a constraint is always `PointOptions::required: Option<Locus>` in the result's own terms (a position for a point; parameters for features). Examples: Shift angle step in Pen — `Locus::line(relative, from_angle(round(angle/step)·step))`, Ctrl lock angle keeps the last angle (`pen_tool.rs:1316-1326`); Line tool (`line_shape.rs:132-144`); Select axis-constrained drag — `Locus::line(ZERO, direction)` in slide space (`select_tool.rs:673`); Path axis lock (`path_tool.rs:1046-1052`). Under `required`, snaps are only spots lying on the line and crossings of the line with tracks (3.6), then grid-line crossings. The angle step is `document.snapping_state.angle_step_radians()` — per-document, default 15°, clamped to 1°–90° (`misc.rs:103-110`). `Frame::nearest_angle_step` exists (`frame.rs:43-52`) but the tools compute the step themselves; **[read]** `grep` finds no editor caller of `nearest_angle_step`.

Bounding-box spacing (`spacing.rs`, enabled per `snapping_state.bounding_box.distribute_evenly` / `equal_sizes`, `snap_session.rs:285-293`): **[read]**

- Needs the frame's first two axes at right angles (`spacing.rs:77-81`); tools only supply `box_features` when the box is upright in the document (`resize.rs:120`, `transformation_cage.rs:252`).
- Neighbours = boxes of the same `LayerKind`, wholly to one side along the axis, overlapping the moving box across the axis by more than a hair (`spacing.rs:152-186`); the nearest and the next beyond it on each side.
- Three rules per axis (`spacing.rs:270-314`): continue the gap of the two boxes on the low side; same on the high side; centre between the nearest on each side (rigid move, or stretch of the one moving side). Each is offered as a `Spacing` Track (a line across the axis through the carrying feature) only if the needed travel is within the gathering distance, and carries `MatchedGaps { gap, spans: [[DVec2;2];2] }` for drawing.
- `find_equal_sizes` (`spacing.rs:319-367`): only when exactly one side moves along an axis; references = nearest box on either side along both axes + every awake box; offers an `EqualSize` track where the stretched extent equals the reference's extent along that axis.
- **[inferred]** Spacing looks at most two neighbours deep per side; it does not find equal spacing among arbitrary sets of three or more objects, and does nothing for rotated boxes.

### 3.10 Per-kind toggles

Library `Settings` (`libraries/snapping/src/settings.rs:3-44`): 15 booleans, **all default `true`**: `anchors, midpoints, curves, box_corners, box_edge_midpoints, box_centers, box_edges, artboards, tangents, perpendiculars, point_alignment, box_alignment, extensions, constructions, equal_lengths`. **[read]**

Editor `SnappingState` (`misc.rs:72-99`) and defaults: **[read]**

| Field | Default | Line |
|---|---|---|
| `snapping_enabled` | `true` | `misc.rs:89` |
| `grid_snapping` | `false` | `:90` |
| `grid_visible` | `false` | `:91` |
| `artboards` | `true` | `:92` |
| `angle_step` | `15.` | `:96` |
| `bounding_box.center_point` | `true` | `:130` |
| `bounding_box.corner_point` | `true` | `:131` |
| `bounding_box.edge_midpoint` | `true` | `:132` |
| `bounding_box.align_with_edges` | `true` | `:133` |
| `bounding_box.distribute_evenly` | `true` | `:134` |
| `bounding_box.edges` | **`false`** | `:135` |
| `bounding_box.equal_sizes` | `true` | `:136` |
| `path.anchor_point` | `true` | `:155` |
| `path.line_midpoint` | `true` | `:156` |
| `path.along_path` | `true` | `:157` |
| `path.normal_to_path` | `true` | `:158` |
| `path.tangent_to_path` | `true` | `:159` |
| `path.align_with_anchor_point` | `true` | `:160` |

Mapping editor → library (`snap_session/settings.rs:7-26`): one-to-one, except `extensions`, `constructions`, `equal_lengths` are always `true` — "These come only from targets that have been woken, and have no options of their own" (`settings.rs:21-24`). **[read]**

Options popover grouping and labels (`misc.rs:356-421`, `SNAPPING_OPTION_GROUPS`), with tooltips verbatim: **[read]**

- **Points**: "Anchors" (Snaps to the anchor points of paths.) · "Midpoints" (Snaps to the middle of straight path segments.) · "Box Corners" · "Box Centers" · "Box Edge Midpoints" · "Artboards" (Snaps to the corners, centers, and edges of artboards.)
- **Curves**: "Along Paths" (Snaps to anywhere along paths.) · "Along Box Edges" · "Tangent to Paths" (Snaps a line to where it touches a path without crossing it.) · "Perpendicular to Paths" (Snaps a line to where it meets a path at a right angle.)
- **Alignment**: "With Anchors" (Snaps to the horizontal and vertical lines through anchor points.) · "With Box Edges" (…through the edges and centers of layers' bounding boxes.)
- **Spacing**: "Even Spacing" (Snaps a box to where its gaps to the boxes beside it are equal.) · "Equal Sizes" (Snaps a box being stretched to the width or height of a box beside it.)
- **Grid**: "Snap to Grid" (Snaps to the points where the drawn grid lines cross.)

There is no per-kind toggle for path intersections: they follow from "Along Paths" being on. **[inferred from `solve.rs` crossing logic + `settings.rs`]**

### 3.11 Snap indicator overlay (`snap_session/presenter.rs`)

**[read]** `draw_overlays` (`presenter.rs:16-75`), all in viewport pixels:

1. Awake targets, always drawn: anchor or position → circle radius `5.` stroke `COLOR_OVERLAY_BLUE_50` (`#00a8ff80`), no fill (`:82,96`); curve → outline of the segment (`:87`); box → dashed quad `COLOR_OVERLAY_BLUE_50`, dash 4 / gap 4 (`:92`).
2. No gesture and `Snap::Free` or `Snap::Grid`: a filled square, size `4.`, `COLOR_OVERLAY_BLUE` (`#00a8ff`) at the lead feature, then return (`:25-31`). (During a gesture, a grid or free result draws nothing.)
3. For each engaged candidate (one, or the two of a crossing):
   - highlight: curve → outlined; box → dashed quad `COLOR_OVERLAY_BLUE` 4/4; anchor → nothing extra (`:102-116`).
   - `Track(Line)` with a `reference` → a line from the reference to the snapped feature position plus a (non-selected) handle marker at the reference (`:46-50`). Colour `None` → `COLOR_OVERLAY_BLUE`, thickness `None` → `1.` px (`portfolio/document/overlays/utility_types.rs:736,743`).
   - `Track(Circle)` → the whole circle stroked `COLOR_OVERLAY_BLUE` (`:51-54`).
   - `gaps` → for each of the two spans: a line across the gap, a perpendicular cap of ±`GAP_CAP_REACH` (4 px) at each end, and the gap length via `format_rounded(gap, 2)` in document units, text `COLOR_OVERLAY_BLUE`, placed above a horizontal gap or right of a vertical one (`:131-147`).
4. A filled 4 px blue square at each snapped feature (deduplicated within 0.5 px) (`:66-70`).
5. One text label at the first engaged feature, 4 px above it, `COLOR_OVERLAY_WHITE` on `COLOR_OVERLAY_BLACK_75` (`#000000bf`) background, padding `4.` (`:72-74`). Text from `label()` (`:150-161`): one kind → its name; two `On*` kinds → "Intersection"; two `Alignment` → "Aligned Both Ways"; two equal kinds → the name once; otherwise "`{A} and {B}`". Names are in the table in 3.4.

### 3.12 The grid

State: `GridSnapping` (`misc.rs:214-247`). **[read]**

| Field | Default | Notes |
|---|---|---|
| `origin: DVec2` | `ZERO` | document position, or offset from artboard top-left |
| `grid_type` | `Rectangular { spacing: DVec2::ONE }` | or `Isometric { y_axis_spacing, angle_a, angle_b }` |
| `rectangular_spacing` | `DVec2::ONE` | remembered while isometric is active |
| `isometric_y_spacing` | `1.` | remembered while rectangular is active |
| `isometric_angle_a` / `_b` | `30.` / `30.` | degrees; UI range 1–89 (`grid_overlays.rs:415-416,423-424`) |
| `color` | `COLOR_OVERLAY_GRAY` = `#cccccc` | |
| `dot_display` | `false` | lines by default |
| `subdivisions: u32` | `1` | "How many parts minor lines split each space between major lines of a rectangular grid into, where 1 draws none"; UI 1–10 (`grid_overlays.rs:374-375`) |
| `angle` | `0.` | degrees, rotates a rectangular grid; UI −180…180 |
| `origin_follows_artboard` | `false` | origin measured from top-left of the artboard containing the first selected layer, else the first artboard (`misc.rs:317-332`) |

Spacing UI minimum is `0.01` (`grid_overlays.rs:350,358,405`); units shown as " px".

Types: **[read]** two — Rectangular (independent X and Y spacing, rotatable, with subdivisions) and Isometric (three line families: vertical, and two slanted at `-angle_a` and `+angle_b` from horizontal). Each can be displayed as lines or dots (`grid_overlays.rs:179-196`). There is no polar/radial grid.

Zoom adaptation (rectangular) — `compute_rectangle_spacing` (`misc.rs:279-293`): **[read]** "Double grid size until it takes up at least 10px": `while (size * zoom) < 10 on any axis { size *= 2 }` (both axes doubled together; gives up after 100 iterations). `drawn_levels` (`misc.rs:296-313`): `major` = that coarsened spacing; `minor = major / subdivisions`, drawn only if `subdivisions > 1` and `minor * zoom >= 5 px` on both axes ("Minor lines may come twice as close on screen as the major lines do"). The grid never gets finer than the configured spacing — it only coarsens by powers of two.

Zoom adaptation (isometric) — `compute_isometric_multiplier` (`misc.rs:335-347`): doubles a multiplier until `(|y_spacing| / max(|tan a + tan b|, 1)) * multiplier * zoom >= 10`. One level only (no minor lines).

**The grid snapped to is the grid drawn**: `settings::grid` uses `GridLevels::finest` — "The grid to land on, which is the finest one drawn at the document's present zoom" (`snap_session/settings.rs:28-31`). **[read]** So zooming out coarsens what you snap to. Rectangular → `Grid::new(origin, levels.families(finest))` = two families: lines along the grid's Y axis spaced `spacing.x`, lines along its X axis spaced `spacing.y` (`misc.rs:272-274`). Isometric → three families (`settings.rs:51-60`): vertical lines spaced `along_y / spread` where `spread = tan(b) + tan(a)`, and the two slanted families along `(1, slope)` spaced `along_y / hypot(slope, 1)` straight across.

Library `Grid` (`grid.rs:29-165`): **[read]**

- `Grid::new(origin, families)` needs ≥2 non-parallel families with positive finite spacing; `Grid::rectangular(origin, spacing)`.
- "A feature lands where the first two families meet" (`grid.rs:28`): grid points are the lattice generated by the first two families; `steps` matrix columns are the lattice basis (`grid.rs:59-62`). For isometric the third family passes through the same lattice **[inferred from the spacing formulas]**.
- `nearest_point_as_seen(position, seen)` (`grid.rs:87-111`): lattice reduction (up to 64 rounds of Gauss-style shortening, measured on screen), round to the reduced basis, then test the 3×3 neighbourhood and take the one that appears nearest.
- `crossings_along(point, direction, near)` (`grid.rs:115-139`): for each family not parallel to the line, the crossings with the family's lines just below and above `near` — used to quantise along a track or a required line. All families take part (including the third isometric one).
- `distance_to_next_line(point, direction)` (`grid.rs:142-164`): used for arrow-key nudging.

Nudging on the grid (`snap_session.rs:728-771`): **[read]** with snapping on and grid snapping on, an arrow nudge moves so that the position furthest along the nudge lands on the next grid line ahead. A big nudge (Shift, `BIG_NUDGE_AMOUNT = 10.`, `consts.rs:54`) lands on the next *major* line when minor lines are drawn, else 10 lines on. Without grid snapping a nudge is `NUDGE_AMOUNT = 1.` (`consts.rs:53`) rotated by the canvas tilt. A diagonal nudge = X nudge then Y nudge.

Drawing (`grid_overlays.rs`): **[read]**

- Lines: `draw_line_family` draws each line of a family across the visible quad (`:61-79`). Minor lines first at `MINOR_LINE_OPACITY: f64 = 0.5` of the grid colour's alpha (`:11`), skipping every `subdivisions`-th (those are major); then major lines at full colour (`:81-93`).
- Dots: one dot at each crossing of the finest lines; dots on two major lines at full colour, others at 50% (`:96-120`). `DOT_SIZE: f64 = 1.5` px (`:14`).
- `MAX_LINES_IN_VIEW: f64 = 10_000.` — a family with more steps than this across the view is not drawn (`:17,47-48`).
- Isometric lines/dots: `:122-169`; no minor level.
- The origin has no special marker; there are no axis-coloured lines. **[read: nothing drawn for it in `grid_overlay`]**

Grid options panel rows (`grid_overlays.rs:199-433`): "Grid" + "Snap to Grid" checkbox; "Type" radio Rectangular / Isometric; "Display" (dotted checkbox "Display as Dotted Grid" + colour); "Origin" ("Origin Follows Artboard" checkbox, X, Y); Rectangular: "Spacing" X/Y, "Subdiv.", "Angle"; Isometric: "Y Spacing", "Angles" (a, b). **[read]**

### 3.13 Performance structure

**[read]**

- `Scene` keeps, per layer, a `BoundsTree` over anchors and one over curve bounds, plus a tree over layer bounds (`scene.rs:118-137`). `BoundsTree` is a static binary BVH with `LEAF_SIZE = 8`, built O(n log n), queried O(log n + matches) (`bounds_tree.rs:4-83`).
- `Scene::prepare()` rebuilds the layer tree only when more than `CHANGES_BEFORE_REBUILD = 32` layers changed since the last build; changed layers are checked linearly meanwhile (`scene.rs:199-230`).
- Queries are restricted to the plane AABB of the gathering square around each probe (`find.rs:81`) — culling is to the pointer's neighbourhood, not to the viewport.
- Crossing pairs capped (16 + 16 + 24 tracks, 3.7); curve×curve crossings cached per gesture in `CrossingCache`, cleared when the scene, features or settings change (`solve.rs:139-164`, `snap_session.rs:217-221,716`).
- Constructions from awake targets are cached and recomputed only when `FoundFor { previous, previous_length, settings, awake }` changes (`snap_session.rs:128-134,271-282`); at most 5 woken + 3 siblings + artboard.
- Feature counts are capped by the tools (16 points, 8 anchors, 9 box features).
- The mirror is O(all geometry) per document change but skips unchanged layers (`scene_mirror.rs:14-40`); it is not run while Q is held or snapping is off (`snap_session.rs:209-215`), nor during a gesture that does not follow edits (`:708`).
- Benchmarks exist: `libraries/snapping/benches/scale.rs`; scenario `the_nearest_anchor_wins_among_many_thousands` (`scenarios.rs:712`).
- Perspective views are supported by the maths (homography `ViewMapping`, per-point Jacobian) though the editor only passes an affine (`snap_session.rs:233`).

### 3.14 NOT present

Checked against the code; each **[read]** unless marked.

- **Snap to handles** (bezier control points): not mirrored into the scene, no `Kind` for it.
- **Midpoint of a curved segment**: only straight segments (`find.rs:115`); `// TODO: Extend to the midpoints of curved segments` (`misc.rs:361`).
- **Arc / circle centre** and **quadrant points**: no `Kind`, no code. A circle's centre is reachable only as its `BoxCenter`.
- **Path "nearest point" as a spot**: `OnCurve` is a track; the point slides along it (and may be quantised by the grid if the segment is straight).
- **User guides / ruler guides**: none in `SnappingState` or the library (the only "guide" hits are commented-out `GuideMode`, `misc.rs:43-69`).
- **Polar tracking / automatic angle snapping from the last point**: angle steps apply only while Shift (snap angle) or Ctrl (lock angle) is held, as a required line. No implicit angular guides other than the axis `Alignment` lines through `previous` and `Parallel`/`RightAngle` to awake segments.
- **Object-snap overrides / one-shot "snap to X" modes**: none. The only steering is Q (quiet), W (cycle), and the per-kind checkboxes.
- **Configurable snap distance**: `Tunables::default()` hard-coded; no preference.
- **Snap to text baseline / glyph features**: none; text layers contribute only their outline curves/anchors and box, like any layer **[inferred from `mirror_layer` using `layer_outline`]**.
- **Snap to a single grid line in free placement**: only grid points; lines matter only along a track / required line.
- **Grid snapping for non-lead features**: only the lead lands on the grid (`solve.rs:622-631`).
- **Finer-than-configured adaptive grid, polar grid, grid origin marker**: none.
- **Minor lines on the isometric grid, rotation of the isometric grid**: none (`angle` applies to rectangular only, `misc.rs:225-226,296-312`).
- **Distribution among arbitrary object sets / rotated boxes**: only the two nearest neighbours per side, upright boxes, same parent and kind.
- **Alignment guides to unrelated far objects without waking**: far alignment needs an awake target (dwell) or one of the automatic three nearest siblings / the artboard.
- **Angle between two segments (e.g. 45° to an existing edge)**: only `Parallel` and `RightAngle`.
- **Snap sound/haptics, snap distance readout**: none; the only numeric readout is the equal-gap length.
- **Locked-layer exclusion**: not tested in `mirror_layer`; hidden layers are excluded. **[read of that function only — a lock filter elsewhere was not searched for]**

### 3.15 Porting notes for DreamCarve

**[inferred]**

- The library can be adopted nearly verbatim: it already uses kurbo 0.13 and separates plane coordinates from screen distance, so Y-up millimetre geometry works by supplying a `ViewMapping` whose affine includes the Y flip. Screen-pixel tolerances then need no change.
- DreamCarve's arc segments (bulge) have no `Locus`/`Curve` form. Options: convert each arc to cubics when mirroring (loses exact centre/quadrant semantics, which Graphite lacks anyway), or add an arc variant to `Curve` with exact nearest/tangent/intersection — the `Locus::Circle` crossing code (`locus.rs:213-238`) and `Curve::circle_intersections` (`curve.rs:228`) are a starting point. Arc centre and quadrant snaps would be new `Kind`s inserted near `Midpoint` in the order.
- The essential behaviours to keep: Spot-beats-Track tiering; 8/12/2 px engage/release/switch; `tie` kind-order preference; riding; the grid as the fallback for remaining freedom; "required track" as the single constraint mechanism; dwell-to-wake for long-range guides.
- The editor layer to re-implement (not port): mirroring the document into `Scene` with roles, building `Exclusions` per gesture, press-holding under the drag threshold, the tick/dwell timer, Q/W keys, and the presenter.
- For CNC use the grid coarsening rule matters: snapping follows the *drawn* level, so the snapped increment changes with zoom. A spec should decide deliberately whether to copy that.

---

## 4. Select tool, transform cage and pivot

Sources (paths relative to the Graphite root, commit `615b4050`):

- `editor/src/messages/tool/tool_messages/select_tool.rs` (`select_tool.rs`)
- `editor/src/messages/tool/common_functionality/transformation_cage.rs` (`transformation_cage.rs`)
- `editor/src/messages/tool/common_functionality/utility_functions.rs` (`utility_functions.rs`)
- `editor/src/messages/tool/common_functionality/pivot.rs`, `compass_rose.rs`, `auto_panning.rs`, `layer_origin_cross.rs`
- `editor/src/messages/tool/transform_layer/transform_layer_message_handler.rs` (`transform_layer_message_handler.rs`)
- `editor/src/messages/portfolio/document/utility_types/transformation.rs` (`transformation.rs`), `.../utility_types/misc.rs`
- `editor/src/messages/portfolio/document/document_message_handler.rs` (align, flip, nudge, click lists)
- `editor/src/messages/portfolio/document/overlays/utility_types.rs` (how the gizmos are drawn)
- `editor/src/consts.rs`, `editor/src/messages/input_mapper/input_mappings.rs`

Tags: **[read]** = taken directly from the code at the cited line. **[inferred]** = my reading of how several pieces combine; not stated in one place and not run.

Coordinate warning for the port: everything here is viewport space or document space with **Y down**. "top" means `min.y`. `DAffine2::from_angle(a)` with positive `a` turns **clockwise on screen**. DreamCarve is Y-up, so every "top/bottom", every angle sign and every `perp()` direction must be re-derived, not copied.

### 4.1 Constants (verbatim)

| Constant | Value | Where |
|---|---|---|
| `SELECTION_TOLERANCE` | `5.` | `editor/src/consts.rs:60` |
| `DRAG_DIRECTION_MODE_DETERMINATION_THRESHOLD` | `15.` | `editor/src/consts.rs:61` |
| `SELECTION_DRAG_ANGLE` | `90.` | `editor/src/consts.rs:62` |
| `LAYER_ORIGIN_CROSS_DIAMETER` / `_THICKNESS` | `10.` / `1.` | `editor/src/consts.rs:63-64` |
| `PIVOT_CROSSHAIR_THICKNESS` / `PIVOT_CROSSHAIR_LENGTH` / `PIVOT_DIAMETER` | `1.` / `9.` / `5.` | `editor/src/consts.rs:67-69` |
| `DOWEL_PIN_RADIUS` | `4.` | `editor/src/consts.rs:70` |
| `COMPASS_ROSE_RING_INNER_DIAMETER` / `_MAIN_RING_DIAMETER` / `_HOVER_RING_DIAMETER` | `13.` / `15.` / `23.` | `editor/src/consts.rs:73-75` |
| `COMPASS_ROSE_ARROW_SIZE` | `5.` | `editor/src/consts.rs:76` |
| `COMPASS_ROSE_ARROW_CLICK_TARGET_ANGLE` | `20.` (degrees) | `editor/src/consts.rs:78` |
| `ANGLE_MEASURE_RADIUS_FACTOR` | `0.04` | `editor/src/consts.rs:81` |
| `ARC_MEASURE_RADIUS_FACTOR_RANGE` | `(0.05, 0.15)` | `editor/src/consts.rs:82` |
| `RESIZE_HANDLE_SIZE` | `6.` | `editor/src/consts.rs:85` |
| `BOUNDS_SELECT_THRESHOLD` | `10.` | `editor/src/consts.rs:86` |
| `BOUNDS_ROTATE_THRESHOLD` | `20.` | `editor/src/consts.rs:87` |
| `MIN_LENGTH_FOR_MIDPOINT_VISIBILITY` | `20.` | `editor/src/consts.rs:88` |
| `MIN_LENGTH_FOR_CORNERS_VISIBILITY` | `12.` | `editor/src/consts.rs:89` |
| `MIN_LENGTH_FOR_EDGE_RESIZE_PRIORITY_OVER_CORNERS` | `10.` | `editor/src/consts.rs:91` |
| `MIN_LENGTH_FOR_RESIZE_TO_INCLUDE_INTERIOR` | `40.` | `editor/src/consts.rs:93` |
| `MAXIMUM_ALT_SCALE_FACTOR` | `25.` | `editor/src/consts.rs:98` |
| `MAX_LENGTH_FOR_NO_WIDTH_OR_HEIGHT` | `1e-4` | `editor/src/consts.rs:100` |
| `SKEW_TRIANGLE_SIZE` / `SKEW_TRIANGLE_OFFSET` | `7.` / `4.` | `editor/src/consts.rs:103-104` |
| `MIN_LENGTH_FOR_SKEW_TRIANGLE_VISIBILITY` | `48.` | `editor/src/consts.rs:105` |
| `SCALE_INCREMENT` | `0.1` | `editor/src/consts.rs:51` |
| `SLOWING_DIVISOR` | `10.` | `editor/src/consts.rs:52` |
| `NUDGE_AMOUNT` / `BIG_NUDGE_AMOUNT` | `1.` / `10.` | `editor/src/consts.rs:53-54` |
| `DRAG_BEYOND_VIEWPORT_MAX_OVEREXTENSION_PIXELS` | `50.` | `editor/src/consts.rs:40` |
| `DRAG_BEYOND_VIEWPORT_SPEED_FACTOR` | `20.` | `editor/src/consts.rs:41` |
| `ANGLE_STEP_DEFAULT` / `ANGLE_STEP_MIN` / `ANGLE_STEP_MAX` | `15.` / `1.` / `90.` (degrees) | `editor/src/messages/portfolio/document/utility_types/misc.rs:11,13,15` |
| `MIN_FEATURE_TRAVEL` | `1e-9` | `transformation_cage.rs:337` |
| `MAX_SNAPPING_ANCHORS` / `MAX_LAYERS_WITH_SNAPPING_ANCHORS` | `8` / `8` | `utility_functions.rs:245,248` |
| `NUDGE_RESIZE_MIN_EXTENT` | `1.` (document units) | `utility_functions.rs:669` |
| `MIN_ARROWS_TO_RESIZE_HANDLE_DISTANCE` | `4.` | `select_tool.rs:1151` |
| `MAX_SNAPPING_POINTS` (G/R/S on path points) | `16` | `editor/src/messages/tool/tool_messages/path_tool.rs:574` |

All pixel values above are viewport pixels unless marked otherwise.

### 4.2 Select tool state machine **[read]**

States (`select_tool.rs:486-508`): `Ready`, `Drawing { selection_shape: Box|Lasso, has_drawn }`, `Dragging { axis, using_compass, has_dragged, deepest, remove }`, `ResizingBounds`, `SkewingBounds { skew: Key }`, `RotatingBounds`, `DraggingPivot`.

Key bindings (`input_mappings.rs:112-118`): pointer move carries `axis_align: Shift, snap_angle: Shift, center: Alt, duplicate: Alt`; press carries `extend_selection: Shift, remove_from_selection: Alt, select_deepest: Accel, lasso_select: Control, skew: Control`; release carries `remove_from_selection: Alt`; `Enter`; double-click = `EditLayer`; right-click and `Escape` = `Abort`.

Press in `Ready` picks the gesture in this fixed order (`select_tool.rs:1415-1510`):

1. Pointer over the pivot dot (only when the gizmo type is Custom Pivot and enabled) → `DraggingPivot`.
2. Cage resize hit (`check_selected_edges`) → `ResizingBounds`, unless the pointer is on a skew triangle of the closest hit edge → `SkewingBounds` (`utility_functions.rs:476-488`).
3. Compass rose grabbable, or the deepest layer under the pointer is inside the current selection → `Dragging` (existing selection moves; nothing is reselected yet).
4. Cage rotate hit (`check_rotate`) → `RotatingBounds`.
5. Otherwise: unless Shift or Alt is held, deselect all. If a layer is under the pointer, select it (shallow or deep rule) and go straight to `Dragging`, so click-drag on an unselected layer selects and moves in one gesture. If nothing is under the pointer → `Drawing`, lasso when Ctrl is held, else box.

Note the order: resize beats drag, and drag of the selection beats rotate. **[read]**

Release / end transitions:

- `Dragging` + release (`select_tool.rs:1799-1886`): ends the transaction. If no drag happened and Alt is held and no layer was picked at press, the layers under a ±`SELECTION_TOLERANCE` box are removed from the selection. If no drag happened and the press landed on an already-selected layer, the selection narrows to that layer (or toggles it out when Shift was held at press).
- `ResizingBounds | SkewingBounds | RotatingBounds | DraggingPivot` + release or `Enter` (`select_tool.rs:1887-1919`): commit; if the pointer moved less than `10. * f64::EPSILON` from the press, abort the transaction instead.
- `Drawing` + release (`select_tool.rs:1920-1993`): apply the box or lasso selection (4.3).
- `Abort` (Escape or right-click) in any drag state: abort the transaction, which restores the layers; a pivot drag also restores the saved pivot because the pivot is tool state, not document state (`select_tool.rs:1520-1538`, `2009-2042`).
- Any change out of a dragging state ends the snapping gesture (`select_tool.rs:429-445`).

### 4.3 Selecting

**Click hit-test [read]**: `click_list` x-rays the exact pointer position in document space against layer click targets, artboards excluded (`document_message_handler.rs:2365-2399`). There is no pixel tolerance applied at this level. **[inferred]** any stroke-width slack lives in the click targets, which are outside the files read.

**Tolerance on a missed click [read + inferred]**: a press on empty canvas enters `Drawing`; when the pointer has not moved, the selection box is the press point ± `SELECTION_TOLERANCE` (5 px) (`select_tool.rs:735-742`), and with no direction established the mode falls back to Touched (`select_tool.rs:732`). So a click within 5 px of a layer still selects it.

**Shallow vs deep [read]** (`select_tool.rs:80-93`, `184-191`): options-bar dropdown, default tool data value Shallowest. Shallow: a click selects the least-nested ancestor, and a double-click steps one level deeper. Deep: a click selects the most-nested layer. Holding Accel at press forces deep for that click (`select_tool.rs:1491`).

**Modifiers [read]**: Shift at press keeps the existing selection and adds (and toggles a clicked selected layer off on release without drag). Alt at press also keeps the selection; Alt at release subtracts.

**Box and lasso [read]**: Ctrl at press on empty canvas gives a lasso (`select_tool.rs:1507`). Lasso points are appended per pointer move; if the last two points are closer than `SELECTION_TOLERANCE` the last is replaced rather than kept (`select_tool.rs:2377-2389`). The lasso polygon is closed implicitly.

**Touched / Enclosed / Directional [read]** (`editor/src/messages/preferences/utility_types.rs:3-8`, a user preference, default `Touched`):

- Touched: anything the area intersects.
- Enclosed: box → the layer's viewport bounding box lies fully inside the box (`document_message_handler.rs:2320-2341`); lasso → every click-target path lies inside the polygon (`document_message_handler.rs:2343-2362`).
- Directional (`select_tool.rs:718-733`): once the box diagonal exceeds `DRAG_DIRECTION_MODE_DETERMINATION_THRESHOLD` (15 px), the mode latches for the rest of the drag: current x left of start x → Touched, otherwise Enclosed. It does not flip back if the pointer crosses over.
- Drawn as: Enclosed = dashed outline (`4.`, `4.`, offset `0.5`), Touched = solid outline; both filled `COLOR_OVERLAY_BLUE_05` (`select_tool.rs:1339-1348`). Layers that would be selected are outlined live.
- In Deep mode a Touched box excludes group layers; the result is then collapsed so a group whose descendants are all selected is selected instead of them (`select_tool.rs:2391-2421`). In Shallow mode every hit maps to its topmost non-artboard ancestor (`select_tool.rs:1959-1966`).

**Double-click [read]** (`select_tool.rs:1368-1376`, `2350-2375`): Shallow → select the child-of-selected ancestor one level deeper; when there is nothing deeper, or in Deep mode, a text layer switches to the Text tool in edit mode. For any other layer nothing happens.
**Enter in Ready [read]** (`select_tool.rs:1994-2008`): exactly one selected text layer → Text tool edit. Otherwise nothing.
**Tab [read]**: toggles Select ↔ Path tool (`input_mappings.rs:345`). That, not double-click, is the route into node editing.

### 4.4 Drag-move **[read]**

- Movement is `pointer − grip` in document space, run through the snap session (`select_tool.rs:664-693`).
- Snap sources that travel with the drag (`select_tool.rs:567-604`): the 4 corners of the cage box, the 4 edge middles, the box centre, then up to `MAX_SNAPPING_ANCHORS` (8) path anchors nearest the grip. Anchors are only offered when at most `MAX_LAYERS_WITH_SNAPPING_ANCHORS` (8) visible leaf layers are dragged (`utility_functions.rs:251-289`). The "lead" feature, the one that lands on the grid, is the box corner nearest the grip.
- **Shift = axis lock** (`select_tool.rs:1585-1591`): the angle of `pointer − drag_start` in the viewport is rounded to the nearest `SELECTION_DRAG_ANGLE` (90°), so the drag is held to screen-horizontal or screen-vertical, whichever is nearer, re-evaluated on every move. The constraint is handed to the snapper as a required line, so the drag can still stop where that line crosses a snap target. Two guide lines through the dragged layers' start centre are drawn, red/green for the active axis and 25 % alpha for the other (`select_tool.rs:1269-1300`).
- **Alt = duplicate** (`select_tool.rs:1554-1568`, `761-853`): pressing Alt mid-drag puts the originals back at their start and drags fresh copies; releasing Alt deletes the copies and resumes moving the originals. It is a live toggle.
- A dashed "translation box" with dx and dy labels is drawn from drag start to current position while dragging (`select_tool.rs:1351-1357`; layout in 4.9).
- Q = "Quiet Snapping", W = "Cycle Snap" hints are shown wherever a drag snaps (`snap_session.rs:774-775`).

### 4.5 Transform cage

**Orientation [read]** (`select_tool.rs:894-903`, `983-1002`): the cage is **not** axis-aligned. It is an axis-aligned box in the local frame of the *first selected non-artboard layer* (its transform to viewport), and all selected layers' bounds are unioned in that frame. One rotated layer → rotated cage; several layers → cage aligned to the first layer. If that frame is singular, `1e-4 × identity` is added and the cage is flagged `transform_tampered`, which disables resizing.

**Handles drawn [read]** (`transformation_cage.rs:552-641`; handle = `RESIZE_HANDLE_SIZE` (6 px) white-filled square rotated with the cage, `overlays/utility_types.rs:845-848`; the outline colour is the overlay default, **[inferred]** blue). Let W and H be the cage's on-screen side lengths:

| Case | Condition | Handles |
|---|---|---|
| Point | both local extents ≤ `1e-4` | none |
| Flat | one local extent ≤ `1e-4` | 2, at the two ends |
| Narrow | W < 12 or H < 12 | 4 edge midpoints, no corners |
| otherwise | W ≥ 12 and H ≥ 12 | 4 corners; top/bottom midpoints when W > 20; left/right midpoints when H > 20 |

`Point`/`Flat` test local cage units against `MAX_LENGTH_FOR_NO_WIDTH_OR_HEIGHT`, not pixels (`transformation_cage.rs:644-651`).

**Resize hit zones [read]** (`check_selected_edges`, `transformation_cage.rs:677-746`). Pixel thresholds are converted per axis into cage-local units so the zones stay the same on-screen size under any affine cage transform (`transformation_cage.rs:660-670`).

1. The pointer must be within `BOUNDS_SELECT_THRESHOLD` (10 px) outside the box on every side.
2. An edge is hit when the pointer is within 10 px of it on either side (inside or outside). Two perpendicular edges hit = a corner. There is no separate test against the 6 px handle squares; the handles are visual only.
3. Corner vs edge when both are hit: if the pointer is within `MIN_LENGTH_FOR_EDGE_RESIZE_PRIORITY_OVER_CORNERS` (10 px) of the box's horizontal centre, left/right are dropped (top or bottom edge wins); else if within 10 px of the vertical centre, top/bottom are dropped. This only bites on cages small enough that corner zones reach the middle.
4. Small cage (W < 40 or H ≤ 40, `MIN_LENGTH_FOR_RESIZE_TO_INCLUDE_INTERIOR`): a tampered cage gives no hit; a pointer strictly inside the box gives no hit (only the exterior band resizes, so the interior stays available for dragging); if H < 12 and a left/right edge is hit, top/bottom are dropped; if W < 12 and a top/bottom edge is hit, left/right are dropped; a local width < `1e-4` disables left/right and a local height < `1e-4` disables top/bottom.

**Rotate hit zone [read]** (`check_rotate`, `transformation_cage.rs:749-766`): the pointer is outside the box and inside a square of half-side `BOUNDS_ROTATE_THRESHOLD` (20 px) centred on any of the **8** handle positions (corners and edge midpoints), or the 2 end points for a flat cage. A point cage cannot rotate. Because resize is tested first, the effective rotate band is roughly 10–20 px outside the box near a handle. **[inferred]** on a long edge the stretch between a corner zone and the midpoint zone is not a rotate target.

**Cursors [read]** (`transformation_cage.rs:769-796`): NS, EW, NWSE, NESW resize by which edges are hit; `Rotate` in the rotate zone; EW/NS over a skew triangle. The cursor choice does not account for cage rotation. **[inferred]** a 45°-rotated cage shows the cursor for the unrotated edge.

**Resize math [read]** (`SelectedEdges::new_size`, `transformation_cage.rs:99-175`; applied in `utility_functions.rs:292-325`):

- The pointer is mapped into the cage's local frame as it was at press. Each dragged edge is set to the pointer's coordinate directly. **[inferred]** there is no grab offset, so the edge jumps to the pointer by up to the 10 px hit slack.
- Fixed point ("pivot") = the opposite corner for a corner drag, the opposite edge's midpoint for an edge drag (`transformation_cage.rs:78-96`).
- **Alt** (`center: Alt`): scales about the *centre of transformation*, which is the pivot gizmo position (centre of the cage by default), not necessarily the geometric centre. Per dragged side, `scale_factor = (center − opposite) / (center − original_dragged)` and the opposite side becomes `center − scale_factor × (center − dragged)`. If that is non-finite or `|scale_factor| > MAXIMUM_ALT_SCALE_FACTOR` (25), which happens when the pivot sits on or near the dragged edge, Alt is ignored for this move and the edge resizes normally.
- **Shift** (`axis_align: Shift`): keeps the aspect ratio recorded at press (`width / height`). Corner drag: of the two candidate sizes (follow x, follow y) take the component-wise larger magnitude and keep the current signs. Edge drag: the other dimension follows and grows about the fixed point proportionally (`min_pivot = (pivot − min) / size`).
- **Flip through zero [inferred]**: there is no clamp, so dragging past the fixed point gives a negative size and hence a negative scale (a mirror).
- **Minimum size**: none for drag-resize **[read: no clamp present]**. In `bounds_to_scale_transform` an axis whose original size is below `f64::EPSILON * 1000.` or whose factor is non-finite gets factor 1 (`transformation_cage.rs:282-299`).
- The result is a non-uniform scale about a pivot in cage space, conjugated by the cage transform at press and applied to each layer's original transform.
- **Snapping while resizing** (`snap_stretch`, `transformation_cage.rs:178-279`): the moving features are the 4 corners, 4 edge middles, centre and the nearest ≤ 8 anchors of the *original* box, each stretched about the fixed point. An edge drag constrains the snap to keep the other axis's scale at 1; Shift constrains both scales to stay proportional. Width/gap matching ("same width as neighbour", "equal gaps") is only offered when the cage is upright in the document and neither Shift nor Alt is held.

**Rotation [read]** (`utility_functions.rs:328-380`, `transformation_cage.rs:799-840`):

- Centre = the pivot gizmo position at press (cage centre by default; mean of layer origins as fallback).
- Angle = signed angle from `press − centre` to `pointer − centre`, both in the viewport. It is a relative angle from where the drag began, not an absolute orientation.
- **Shift**: the angle is rounded to multiples of the document's angle step, default **15°**, user-settable 1–90° in the snapping popover (`misc.rs:11-15`, `103-110`). While Shift is held nothing else snaps. The hint text hard-codes "15° Increments" (`select_tool.rs:2181`).
- Without Shift: the cage corners, edge middles and nearest anchors snap as they travel on their circles about the centre; the grid is ignored (`ignores_grid: true`).
- Applied as `DAffine2::from_angle(angle)` about the centre in viewport space. Positive is clockwise on screen (Y down); a flipped canvas mirrors it.

**Skew [read]** (`transformation_cage.rs:301-332`, `438-458`, `503-542`, `842-899`):

- Trigger: **dragging one of the two small triangles** that appear either side of an edge midpoint when the pointer nears it. No modifier is needed to start. Triangles exist only on edges at least `MIN_LENGTH_FOR_SKEW_TRIANGLE_VISIBILITY` (48 px) long. The code also gates by cage size category (`transformation_cage.rs:460-488`), but **[inferred]** the 48 px rule already excludes every edge that gate would exclude, leaving full-size cages and the long edges of narrow ones.
- Triangle geometry: base at `midpoint ± edge_dir × (3 + SKEW_TRIANGLE_OFFSET)` = 7 px from the midpoint, size 7 px (`overlays/utility_types.rs:850-857`). Hit box: centre at `midpoint ± edge_dir × (3 + 4 + 3.5)` = 10.5 px, half-extent `SKEW_TRIANGLE_SIZE / 2` (3.5 px) along the edge and `BOUNDS_SELECT_THRESHOLD` (10 px) across it (`transformation_cage.rs:447-455`).
- Triangles are shown when the pointer is inside a rectangle around the midpoint of half-width `RESIZE_HANDLE_SIZE/2 + SKEW_TRIANGLE_OFFSET + SKEW_TRIANGLE_SIZE` = 14 px along the edge and 10 px across; on a compact cage (other side < 40 px) only the outer half of that rectangle plus a small inner strip counts (`transformation_cage.rs:503-542`).
- Math: the opposite edge's midpoint is fixed. Drag vector = pointer − dragged edge midpoint, taken into cage space and divided by the signed cage extent across the edge (sign −1 for top and left). For a top/bottom edge the matrix is `[[1, d.x], [0, 1]]` (x shear proportional to distance from the fixed edge).
- **Ctrl held during the drag = "Unlock Slide"** (`select_tool.rs:1436`, `2189`): the edge may also move across itself, adding `1 + d.y` on the diagonal (a stretch as well as a shear). Without Ctrl the edge only slides along its own direction.
- Snaps like rotation: every cage point follows the dragged edge midpoint by its fractional distance from the fixed edge.

**Arrow-key nudge [read]** (`input_mappings.rs:383-406`, `document_message_handler.rs:3770-3830`, `snap_session.rs:728-771`):

- Arrows move the selection by `NUDGE_AMOUNT` (1), Shift by `BIG_NUDGE_AMOUNT` (10), in document units; two arrows held give a diagonal. The direction is screen-relative (rotated by minus the canvas tilt).
- With snapping on and a grid active, a nudge instead lands the leading bounding-box corner on the next grid line; a big nudge goes to the next major line, or 10 lines on when only major lines are drawn.
- **Alt + arrow = resize**: the bottom-right corner moves by the nudge and the top-left stays; **Ctrl + Alt** moves the top-left and anchors the bottom-right. Each axis keeps at least `NUDGE_RESIZE_MIN_EXTENT` (1 unit); an original extent under 1 unit is left unscaled (`utility_functions.rs:682-720`). The box used is the document-space AABB of the selection, not the oriented cage.

### 4.6 Pivot

**[read]** (`pivot.rs`, `select_tool.rs:1161-1226`, `node-graph/libraries/vector-types/src/vector/reference_point.rs`)

- The pivot is the centre for cage rotation, Alt-resize and the modal R/S transforms.
- Options bar: a checkbox "Pivot Gizmo" and a dropdown with three types: `Custom Pivot`, `Origin (Average Point)` (mean of the selected layers' origins), `Origin (Active Object)` (origin of the most recently selected layer). Disabled → always the centre of the selection bounds (`pivot.rs:126-138`, `184-193`).
- Custom Pivot is stored as a normalised position in the cage box, (0,0) = top-left to (1,1) = bottom-right, default (0.5, 0.5) (`pivot.rs:196-225`). A **9-position reference-point widget** sets it to `TopLeft … Center … BottomRight`; a position is recognised as one of the nine within tolerance `1e-5`, anything else is `None` (`reference_point.rs`).
- Dragging: the pivot dot is grabbable within `PIVOT_DIAMETER / 2` = 2.5 px of its centre (`pivot.rs:306-308`) and this takes priority over every other gesture. The drag position goes through the general point snapper with default options (`select_tool.rs:1673-1690`), and the selected layers stay in the snap scene since they do not move. Tests confirm it lands on a corner and on the middle of its own layer (`select_tool.rs:3087`, `3102`). There is no dedicated "snap to the nine points" code: **[inferred]** landing on those comes from ordinary bounding-box snap targets.
- After a free drag the reference point reads `None` ("disconnected"); a pin button then appears. Unless pinned, the next fresh selection (press that deselects) resets the pivot to the last non-`None` reference point (`select_tool.rs:1478-1481`). A disconnected pivot is carried along when the selection is dragged or nudged (`select_tool.rs:1869-1878`, `2063-2075`).
- Storage: tool state only; it is not written to the document and aborting a transaction does not restore it, so the tool saves and restores it itself (`select_tool.rs:1523`).
- Drawn as a yellow 5 px filled circle with a 9 px crosshair, 1 px round-capped, rotated to the cage angle (`overlays/utility_types.rs:1063-1092`).
- **Dowel pin**: when the gizmo type is one of the Origin types, each selected layer's origin gets a 4 px-radius white circle with two opposite filled quadrants; the active layer's pin is `COLOR_OVERLAY_YELLOW`, others `COLOR_OVERLAY_YELLOW_DULL` (`select_tool.rs:1193-1204`, `overlays/utility_types.rs:1094-1125`). It is a marker; no drag code for it exists in the Select tool.
- **Layer origin cross**: a 10 px blue × at each selected layer's local origin, oriented by the layer's +X (`layer_origin_cross.rs`).

### 4.7 Compass rose

**[read]** (`compass_rose.rs`, `select_tool.rs:1099-1267`, `1408-1463`, `1572-1591`, `overlays/utility_types.rs:1010-1061`)

- A small ring with four arrows at the centre of the cage, rotated with the cage: ring 13–15 px diameter, arrows reaching to 23 px diameter, red arrows on the cage's X axis and green on its Y axis.
- Hit zone: an annulus between radius 6.5 px and 11.5 px. Within `COMPASS_ROSE_ARROW_CLICK_TARGET_ANGLE` (20°) either side of an arrow → that axis; elsewhere on the annulus → `Ring`.
- Pressing an arrow starts a drag constrained to the **cage's local axis** (not the screen axis), so a rotated object slides along its own axis. Pressing the ring starts an unconstrained drag. The ring is how an unfilled shape, whose interior is not a click target, can be grabbed and moved.
- Shown only when every cage side is at least `COMPASS_ROSE_HOVER_RING_DIAMETER + RESIZE_HANDLE_SIZE + 4` = 33 px and the pointer is not over a resize/rotate zone; grabbable only when every side is ≥ 23 px and the pointer is inside the cage.
- While hovering or dragging an arrow, a full-viewport line through the centre is drawn in the axis colour (faded on hover).

### 4.8 Options-bar commands

**[read]** (`select_tool.rs:194-240`, `333-345`)

- **Align** (6 buttons: left, horizontal centre, right, top, vertical centre, bottom; disabled with fewer than 2 layers). `document_message_handler.rs:373-416`: take the union of the selected visible layers' **viewport-space** AABBs (locked layers count toward the union but do not move); translate each unlocked layer along the one axis so its own AABB min / centre / max equals the union's. The reference is always the selection's union. It is computed in viewport space, so **[inferred]** on a tilted canvas it aligns to the screen, not the document.
- **Flip** horizontal / vertical: scale by (−1, 1) or (1, −1) about the centre of the selection's viewport AABB (`document_message_handler.rs:745-767`).
- **Boolean**: Union, Subtract Front, Subtract Back, Intersect, Difference (`node-graph/libraries/vector-types/src/vector/misc.rs:16-28`); each groups the selection into a boolean-operation folder (non-destructive).
- **Rotate ±90°**: menu bar only, about the centre of the selection's viewport AABB (`document_message_handler.rs:768-791`).
- Also in the bar: fill/stroke widgets, the Shallow/Deep dropdown, the pivot controls.

### 4.9 Modal Grab / Rotate / Scale (G, R, S)

**[read]** (`transform_layer_message_handler.rs`, `transformation.rs`, `input_mappings.rs:99-109`, `409-421`)

- `G`, `R`, `S` with a selection start a modal transform with **no mouse button held**; the pointer then drives it. It transforms whole layers in the Select and Shape tools (`transformation.rs:548-551`), selected points/segments in the Path tool, and the outgoing handle in the Pen tool. Which other tools let the keys through was not verified.
- Confirm: left click or `Enter`. Cancel: right click or `Escape` (undoes every chained operation). Pressing another of G/R/S while active applies the current one provisionally and switches.
- **Grab**: translation = accumulated pointer delta.
- **Rotate**: accumulates the signed angle between successive `pivot → pointer` vectors, so it can wind past 360° (`transform_layer_message_handler.rs:606-615`).
- **Scale**: factor starts at 1 and changes by the projection of pointer motion onto the `pivot → start` vector divided by its squared length; crossing the pivot makes it negative (a flip). Starting exactly on the pivot does nothing (`transform_layer_message_handler.rs:616-638`).
- **Axis constraint**: `X` / `Y`. First press = that global (viewport) axis; second press of the same key = the selection's **local** axis (the cage's axes); third press clears. Pressing the other key switches axis and resets to global (`transformation.rs:142-150`). Rotation ignores X/Y. For scale, a constraint scales one axis and leaves the other at 1.
- **Typed value** (`transformation.rs:716-804`): digits `0-9`, `.` or `,` for the decimal point, `-` to negate, `Backspace` to delete. Allowed for Rotate (degrees) and Scale (factor) at any time, and for Grab **only when axis-constrained** (distance along that axis in document units) (`transformation.rs:381-383`). A typed value overrides the pointer and is not snapped.
- **Shift = slow**: pointer deltas are divided by `SLOWING_DIVISOR` (10).
- **Ctrl = increments**: rotate rounds to the document angle step (15° default); scale rounds to `SCALE_INCREMENT` (0.1); grab rounds the displacement to whole document units (`transformation.rs:180`, `227-236`, `281-284`). Increments disable snapping.
- Snapping otherwise mirrors the cage gestures: box corners, edge middles, centre and nearest anchors (layers), or the nearest ≤ 16 selected points (Path tool); rotate and scale ignore the grid.
- Pivot: the pivot gizmo position for layers; for path points the average of the selected points (or the gizmo), and for a single selected handle its anchor (`transform_layer_message_handler.rs:961-1021`).
- **On-canvas readout** (shown when the "transform measurement" overlay setting is on, `transform_layer_message_handler.rs:218-320`):
  - Grab: a dashed L from the grab target, horizontal leg then vertical leg, each labelled with its length to 2 decimals (or the typed string); when both are non-zero the rectangle is completed with solid lines (`overlays/utility_types.rs:1378-1406`). The Select tool's ordinary drag draws the same box.
  - Rotate: a solid line from the pivot along the current direction, a dashed line along the start direction, and an arc between them of radius `ANGLE_MEASURE_RADIUS_FACTOR` (0.04) × the larger viewport dimension; the line length is the grip distance clamped to `ARC_MEASURE_RADIUS_FACTOR_RANGE` (0.05–0.15) × that dimension; label `"{angle}°"` to 2 decimals, wrapped into (−360, 360), placed just outside the arc at the bisector (`overlays/utility_types.rs:986-992`).
  - Scale: dashed line pivot → nearer point, solid line on to the further point, label `"{factor}x"` to 3 decimals, offset 24 px perpendicular.
  - In the Path tool a grey ghost outline of the pre-transform geometry is drawn.
  - Dashes are `2.` on, `2.` off, offset `0.5`; label text is 12 px `COLOR_OVERLAY_BLUE`.

### 4.10 Auto-panning

**[read]** (`auto_panning.rs`): while a drag is in progress and the pointer is outside the viewport, on each animation frame the canvas pans by `overshoot_fraction × DRAG_BEYOND_VIEWPORT_SPEED_FACTOR × viewport_size × frame_seconds`, rounded to whole pixels, with the pointer overshoot clamped to `DRAG_BEYOND_VIEWPORT_MAX_OVEREXTENSION_PIXELS` (50 px). **[inferred]** that is 20 px/s of pan per pixel of overshoot, capped at 1000 px/s. Each drag state then shifts its stored start/centre positions by the pan so the gesture stays anchored (`select_tool.rs:1740-1798`). Used by move, resize, skew, pivot drag and box/lasso; **[read]** `RotatingBounds` has no auto-pan arm.

### 4.11 NOT present (checked)

- **No numeric W/H/X/Y entry on the cage** and no size readout on the cage itself. The only numeric entry is the modal G/R/S typing. (A Properties panel may exist elsewhere; it is outside the tool code read.)
- **No typed rotation angle outside G/R/S.** Cage rotation is drag only; ±90° is a menu command.
- **No minimum-size clamp** on drag-resize; sizes can go through zero and flip.
- **No Ctrl+edge skew.** Skew starts only from the triangles; Ctrl during the skew unlocks cross-edge movement.
- **No separate rotate handle** (no lollipop); rotation is a zone outside the handles.
- **No hit test on the handle squares themselves**; zones are edge bands.
- **No mirror tool** (interactive mirror line); only flip H/V buttons about the selection centre.
- **No distribute command.** There is no `Distribute…` document message; "distribute evenly" exists only as a snapping kind (`misc.rs:120`).
- **No align to key object, to artboard, or to page**; alignment is always to the selection's own union box.
- **No transform cage in the Path tool** for selected nodes (`BoundingBoxManager` is used only by the Select, Shape, Text and Artboard tools). Node transforms go through modal G/R/S.
- **No double-click-to-edit-nodes**; double-click drills into groups or edits text.
- **No rotation snap to absolute angles**; Shift steps are relative to the drag start.
- **No shear by typed value, no "transform again", no transform-each** (the pivot type list carries a `TODO: Add "Individual"`, `pivot.rs:163`).
- **No persisted pivot** in the document.

## 5. Measure overlay

Sources: `editor/src/messages/tool/common_functionality/measure.rs` (`measure.rs`), trigger in `select_tool.rs:1004-1080` and `editor/src/messages/tool/tool_messages/artboard_tool.rs:360-394`.

### 5.1 What it is **[read]**

A transient "quick measurement" overlay that shows the **axis-aligned distances between two bounding boxes**: the selection's and the hovered object's. It measures boxes only, in document space, and draws nothing persistent.

### 5.2 Trigger **[read]**

All of these must hold (`select_tool.rs:1007-1018`):

- Select tool active and not in `Drawing` (box/lasso) and not in `ResizingBounds`.
- The middle mouse button is not down (not panning).
- **Alt is held.** The key is read directly, not through the key map (a `TODO` says so).
- The pointer is over a layer that is **not part of the selection**. Artboards count as hover targets here.
- The overlay setting `quick_measurement` is on (`overlays/utility_types.rs:139-141`).

Boxes used (`select_tool.rs:1056-1077`):

- Selected: union of the document-space AABBs of all selected, visible, unlocked, non-artboard layers.
- Hovered: document-space AABB of the hovered layer, resolved by the Shallow/Deep rule (the group in Shallow mode, the leaf in Deep mode); if that resolution lands on something already selected, the leaf under the pointer is used instead.
- Nothing is drawn if either box is missing.

The Artboard tool has the same overlay with Alt: selected artboard against another artboard, or against the topmost layer under the pointer.

**[inferred]** because Alt also means "duplicate" during a move and "subtract" on release, the overlay can appear during a `Dragging` gesture with Alt held; only `Drawing` and `ResizingBounds` are excluded.

### 5.3 What is drawn **[read]**

Always: a dashed outline of both boxes (`measure.rs:339-345`).

Then one set of marks chosen by how the boxes relate (`measurement_marks`, `measure.rs:267-318`). "Overlap along X" means the X spans intersect, touching included. A `Measured` mark is a solid line with a length label; a `Guide` is an unlabelled dashed line.

| Relation | Marks |
|---|---|
| **Apart diagonally** (no overlap on either axis) | Horizontal gap and vertical gap, both measured from the selection's nearest corner; two dashed guides complete the rectangle to the hovered box's nearest corner (`measure.rs:107-122`). |
| **Stacked, a centre inside the other's span** (overlap on X only, and either box's centre x lies within the other's x span) | One measured vertical line across the gap, at the selection's centre x if that lies over the hovered box, otherwise at the hovered box's centre x (`measure.rs:147-162`). |
| **Stacked, offset** (overlap on X only, neither centre within the other's span) | The measured width of the X overlap drawn along the selection's facing edge; the measured vertical gap at one end of the overlap; a dashed guide across the gap at the other end (`measure.rs:125-144`). |
| **Side by side** (overlap on Y only) | The two rows above with X and Y swapped (`measure.rs:285-286`). |
| **Overlapping at a corner** (each box has exactly one edge inside the other on both axes) | Four measured offsets between matching edges (left–left, right–right, top–top, bottom–bottom), drawn along the outside of the combined extent (`measure.rs:165-192`). |
| **Straddling an edge** (one box within the other's span on one axis, crossing one of its edges on the other) | Four measured lines. Two run along the crossing direction at the outer box's two side positions, each spanning the offset between the two boxes' matching edges on the crossed side. Two run the other way, giving the inner box's inset from the outer box on each side, drawn along the outermost of those two edges (`measure.rs:195-209`). |
| **Crossing** (each box is the longer one on a different axis, a plus shape) | Eight measured lines: the offset between each pair of matching edges, at both outer sides (`measure.rs:212-232`). |
| **Contained** (one box entirely within the other, either way round) | Four measured lines from the inner box's sides to the outer box's sides, through the inner box's centre: left, right, up, down (`measure.rs:235-244`). |
| **Sharing edges** | Shared edges produce zero-length marks, which are dropped (`measure.rs:75-77`). A span that shares one edge with the span containing it counts as strictly within; identical spans on one axis take the other axis's arrangement (`measure.rs:258-301`). |
| **Identical boxes** | Outlines only, no marks. |

The unit tests at `measure.rs:388-575` give exact expected marks for each case and are directly portable (they are pure functions of two rectangles).

### 5.4 Layout, formatting, colours **[read]**

- All marks are computed in document space and each end point is transformed to the viewport, so on a tilted canvas the lines tilt with the document.
- Dashed lines: dash `2.`, gap `2.`, offset `0.5` (`measure.rs:336`). Solid measured lines use the default overlay line. Default overlay colour is `COLOR_OVERLAY_BLUE` (`#00a8ff`); **[inferred]** default thickness is 1 px (the default lives in the overlay context, not read line by line).
- Label text: the segment length in **document units**, `format!("{length:.2}")` with trailing zeros and a trailing `.` trimmed, so `10`, `2.5`, `0.33`. **No unit suffix.** (`measure.rs:354-356`)
- Labels are omitted for lengths below `LABEL_MIN_LENGTH` = `0.01` document units (`measure.rs:7`); the line is still drawn.
- Label position: at the segment midpoint, offset `LABEL_PADDING` = `5.` viewport px (`measure.rs:9`) to a chosen side (left/right/top/bottom). The side is chosen per case so labels face away from the boxes, and is re-resolved through the document-to-viewport transform so it stays on the visually correct side on a rotated canvas (`measure.rs:321-330`). Text is 12 px, `COLOR_OVERLAY_BLUE`, no background (`overlays/utility_types.rs:1295-1352`).
- Y-down note: `Side::Top` is `−Y` (`measure.rs:42-49`). A Y-up port must swap top and bottom in `Side::direction` and re-check the "inner_lower" / "hovered_above" comparisons.

### 5.5 Related measurement readouts (not in `measure.rs`) **[read]**

- Drag-move and modal Grab: the dx/dy translation box (4.4, 4.9).
- Modal Rotate: the angle arc and degree label. Modal Scale: the factor label (4.9).
- These are the only angle and length readouts in the Select tool.

### 5.6 NOT present (checked)

- **No point-to-point measure tool** (no click-two-points ruler; there is no measure tool in `tool_messages/`).
- **No angle measurement** between objects or segments; angles appear only during a modal rotate.
- **No persistent dimensions or annotations**; the overlay exists only while Alt is held.
- **No measurement to path geometry** (nearest point on a curve, edge-to-edge true distance); only AABB to AABB.
- **No diagonal / Euclidean distance** between boxes; only horizontal and vertical components.
- **No unit conversion or unit label**; values are raw document units.
- **No width/height readout of a single object** from this overlay (two boxes are always required).
- **No measurement in the Path, Pen or Shape tools** via this module (only Select and Artboard call `measure::overlay`).

---

## 6. Shape tool

Paths are relative to the Graphite root, commit `615b4050ba4d247712dfd2fca745d87a9bba315f`. Short names used below:

- `ST` = `editor/src/messages/tool/tool_messages/shape_tool.rs`
- `SH/` = `editor/src/messages/tool/common_functionality/shapes/`
- `GZ/` = `editor/src/messages/tool/common_functionality/gizmos/shape_gizmos/`
- `RS` = `editor/src/messages/tool/common_functionality/resize.rs`
- `KM` = `editor/src/messages/input_mapper/input_mappings.rs`
- `GEN` = `node-graph/nodes/vector/src/generator_nodes.rs` (not in the sparse checkout; read with `git show HEAD:<path>`)
- `ALG/` = `node-graph/libraries/vector-types/src/vector/algorithms/`

Tags: **[read]** = taken from the source at the cited line; **[inferred]** = my conclusion from what I read, not stated by the code.

### 6.1 Shape list and how each is reached [read]

`ShapeType` (`SH/shape_utility.rs:32-45`), in dropdown order: `Polygon` (default), `Star`, `Heart`, `Circle`, `Arc`, `Spiral`, `Grid`, `Arrow`, then `Line`, `Rectangle`, `Ellipse` ("KEEP THIS AT THE END").

- One tool implements all eleven. Line, Rectangle and Ellipse are "alias" tools with their own toolbar entry (`SH/shape_utility.rs:114-121`) that send `SetShape` and hide the shape dropdown (`ST:1452-1460`, `ST:810-811`).
- Shortcuts (`KM:334-337`): `L` Line, `M` Rectangle, `E` Ellipse, `Y` Shape tool (whatever the dropdown holds). The other eight shapes have no key; they are picked from the dropdown (`ST:173-225`).
- Fill is on by default for the closed shapes and off for Spiral, Grid and Line (`SH/shape_utility.rs:65-70`). Fill/stroke/swap preference is remembered per shape mode (`ST:46-57`).
- Pressing a shape's own key mid-drag is ignored; pressing another shape's key mid-drag cancels the drag (`ST:1493-1505`).

### 6.2 State machine [read]

States (`ST:782-794`): `Ready(shape)`, `Drawing(shape)`, `DraggingLineEndpoints`, `ModifyingGizmo`, `ResizingBounds`, `RotatingBounds`, `SkewingBounds { skew: Key }`.

Press in `Ready` is resolved in this priority order (`ST:1062-1250`):

1. **Gizmo** under the pointer, unless Accel (Cmd/Ctrl) is held (`ST:1074`) -> `ModifyingGizmo`.
2. **End of a selected line or arrow**, unless Ctrl is held: nearest anchor within `SNAP_POINT_TOLERANCE` (5 px, `editor/src/consts.rs:46`) and then within `BOUNDS_SELECT_THRESHOLD` (10 px, `editor/src/consts.rs:86`) of an endpoint (`ST:1094-1114`, `SH/shape_utility.rs:203-204`) -> `DraggingLineEndpoints`.
3. **Transform cage** of the selection, unless Ctrl is held: resize / rotate / skew (`ST:1121-1164`). Selected lines and arrows have no cage (`ST:935-943`, `ST:1117-1119`).
4. Otherwise **draw a new shape**: the layer is created at once on press (`ST:1166-1249`) -> `Drawing`.

So Ctrl in `Ready` means "ignore what is selected, draw here": gizmos and cage are not drawn, anchors of the selection are shown instead (`ST:906`, `ST:928-930`), and the hover snap preview shows where the new shape would start (`ST:1370-1380`).

- Release (`DragStop`) in any drag state: `finish_transaction`, end the snap gesture, back to `Ready` (`ST:1407-1433`).
- Right-click or Escape (`KM:190-191`) in any drag state: abort the transaction (the new layer disappears), back to `Ready` (`ST:1434-1445`, `ST:1475-1491`). Escape in `Ready` puts snapping's woken targets back to sleep (`ST:551-555`).
- Enter does nothing in this tool (no mapping in `KM:187-220`).

**Click without drag** [read + inferred]: `finish_transaction` aborts when the pointer is within `DRAG_THRESHOLD` (`1.`, `editor/src/consts.rs:48`) of `drag_start`, otherwise commits (`editor/src/messages/input_mapper/utility_types/pointer.rs:68-72`). So a click leaves no shape. There is no "click to place a default-size shape" and no other minimum size. [inferred] The `drag_start` handed in is `Resize::drag_start`, which is a document-space point (`RS:10-11`), while the comparison is against the viewport mouse position, so the test is only exact at 100 % zoom with no pan. Do not copy that; compare in screen pixels.

### 6.3 The Resize helper: exact box math [read]

`Resize::snap_box` (`RS:75-158`) is used by Rectangle, Ellipse, Circle, Polygon, Star, Arc and Heart.

- **Start point**: taken on press from whatever snap the hover preview was already showing, or snapped now (`RS:23-26`, `snap_session.rs:492-496`). Stored in document space so panning mid-drag does not move it.
- **Frame**: the box is upright in the *window* (viewport axes), not in the document, so on a tilted canvas shapes are drawn screen-aligned and the layer gets a counter-rotation (`RS:82-87`, `RS:171-176`). Only artboards use document axes (`in_document`).
- **Free parameters**: `free = pointer - start` in viewport pixels (`RS:80`, `RS:96-100`). These are the box extents from the start, or half-extents when centred.
- **Shift (lock ratio)**: `free = max(|x|, |y|)` on both axes, keeping each axis' sign (`RS:102-105`). The larger extent wins. The snap is then *required* to stay on the diagonal line through the start (`Locus::line(ZERO, signum)`), so it still snaps, but only along the diagonal.
- **Alt (from centre)**: unit corners become `(-1,-1) .. (1,1)` instead of `(0,0) .. (1,1)` (`RS:108-112`); result is `[start - extents, start + extents]` instead of `[start, start + extents]` (`RS:156-157`).
- **Snapping features** for a plain box: 4 corners, 4 edge midpoints, centre, with the pointer's corner (index 2) leading (`RS:116-134`). Gap/size matching against other boxes (`BoxFeatures`) is offered only when the box is upright in the document, and only on the side the pointer moves, and never with Shift or Alt (`RS:120-132`).
- **Snapping features** for a shape that does not fill its box (polygon, star, arc ends, heart): the shape's own points plus the centre snap; the box corners do not, except that the pointer's corner still leads and lands on the grid (`RS:135-143`). At most `MAX_SNAPPING_SHAPE_POINTS = 16` points are offered (`SH/shape_utility.rs:28`).
- **No snap caught**: the pointer's extents are used exactly, without projecting onto the required track (`RS:153-154`).
- Result is two opposite corners in viewport space; each shape converts with `size = |start - end| / viewport_zoom` (`RS:167-169`) and places the layer at the midpoint.

### 6.4 Per-shape behaviour

Modifier array is `[Alt, Shift, Control]` (`KM:194`, `ST:843-845`). For box shapes it is `[center, lock_ratio, _]`; for line and arrow `[center, snap_angle, lock_angle]` (`SH/line_shape.rs:55`).

| Shape | Drag | Shift | Alt | Ctrl | Parameters and defaults |
|---|---|---|---|---|---|
| Rectangle | corner to corner | square | from centre | - | node width/height; created 1 x 1 (`SH/rectangle_shape.rs:19`) |
| Ellipse | corner to corner of bounding box | circle | from centre | - | `radius_x`, `radius_y`; created 0.5 (`SH/ellipse_shape.rs:19`) |
| Circle | corner to corner of bounding square; ratio always locked | no effect | from centre | - | `radius` = smaller box side / 2 (`SH/circle_shape.rs:99-107`, `RS:70-72`) |
| Polygon | bounding box | regular (no stretch) | from centre | - | sides 5 in tool (`ST:80`), node default 6, hard min 3 (`GEN:181-183`); radius centre-to-vertex |
| Star | bounding box | regular | from centre | - | sides 5; inner radius = 0.5 x outer while drawing (`SH/star_shape.rs:107`) |
| Arc | bounding box of the full circle | regular | from centre | - | start 0 deg, sweep 270 deg, type Open/Closed/PieSlice (`SH/arc_shape.rs:134-143`, `ST:82`) |
| Spiral | centre out: press = centre, distance = outer radius | - | - | - | Archimedean/Logarithmic, turns 5 (`ST:83-84`), inner radius 0 or 0.1, angular resolution 90 deg (`SH/spiral_shape.rs:97-113`) |
| Grid | bounding box | square cells / 30 deg isometric | from centre | - | lattice type; 10 x 10 points by default (`GEN:408-413`) |
| Line | end to end | angle step | from centre (mirrored) | lock current angle | `line_to` vector |
| Arrow | end to end | angle step | from centre | lock current angle | shaft 14, head width 32, head length 28 px (`ST:86-88`) |
| Heart | bounding box | regular | from centre | - | six shape proportions; not relevant to CNC |

Details [read]:

- **Rectangle / Ellipse**: `size = |start - end| / zoom`; ellipse radii are half that (`SH/rectangle_shape.rs:38`, `SH/ellipse_shape.rs:38`). Upstream test: drag (10,10) -> (19,0) gives `rx 4.5, ry 5`, centre (14.5, 5) (`SH/ellipse_shape.rs:88-104`).
- **Circle with Alt**: because ratio is locked by taking the larger extent, the radius is `max(|dx|, |dy|)` (Chebyshev distance), not the Euclidean distance from the centre. [inferred from `RS:102-105` + `SH/circle_shape.rs:107`]
- **Polygon / Star / Arc in a non-square box**: the node keeps a regular shape with radius = smaller side / 2, and the *layer transform* is stretched on the longer axis (`aspect`) (`SH/polygon_shape.rs:142-166`, `SH/star_shape.rs:146-175`, `SH/arc_shape.rs:166-185`). Shift ("Constrain Regular") makes the box square, so no stretch. There is a TODO admitting irregular polygons are not modelled. DreamCarve stores geometry, not transforms, so it would have to bake the stretch or forbid it.
- **Odd-sided polygon or star dragged upward** is drawn flipped (pointing down) by negating the radius (`SH/shape_utility.rs:217-220`). This test is `pointer.y < start.y` in Y-down viewport space.
- **Sides while idle**: `[` / `]` change the option by 1, min 3 (`ST:992-1020`, `KM:192-193`); the options-bar field is integer, min 3, max 1000 (`ST:141-147`). Changing it also rewrites the selected polygon/star layers (`ST:651-661`).
- **Sides while drawing**: `[` / `]` and also ArrowDown / ArrowUp change the shape being drawn, min 3 (`ST:1039-1061`, `SH/polygon_shape.rs:188`, `KM:219-220`). Arrow keys while idle nudge the selected layers instead (`ST:1021-1038`).
- **Spiral turns**: same keys, step 1, min 1 by key (`ST:1014-1017`, `SH/spiral_shape.rs:170-174`); the field allows min 0.5 (`ST:158-162`). Logarithmic outer radius is clamped to at least 0.1 (`SH/spiral_shape.rs:140-143`). The pointer position is snapped as a plain point (`SH/spiral_shape.rs:123-124`).
- **Grid**: spacing = drag size / 9, because 10 points make 9 cells (`SH/grid_shape.rs:154-261`). Rectangular: Shift uses the larger side for both; Alt centres and uses `2 * size / 9`. Isometric: spacing from height / 9; angle from `atan(h / (2 * dx))`, or fixed 30 deg with Shift. The far corner is snapped only for a rectangular grid with no modifiers (`SH/grid_shape.rs:121-129`).
- **Line** (`SH/line_shape.rs:126-162`): start is the snapped press. Shift rounds the angle of `pointer - start` (document space) to the nearest step and requires the end to stay on that line; the end still snaps along it (upstream test `a_line_held_to_an_angle_still_stops_where_it_meets_an_edge`, `SH/line_shape.rs:326-337`). Ctrl keeps the angle the line had on the previous update. Alt mirrors the far end through the start, and either end may be the one that snaps. The start is passed as `previous`, which lets snapping offer parallel / perpendicular / equal-length constructions.
- **Angle step** is a per-document snapping setting, not a constant: default `ANGLE_STEP_DEFAULT = 15.`, clamped to `ANGLE_STEP_MIN = 1.` .. `ANGLE_STEP_MAX = 90.` degrees (`editor/src/messages/portfolio/document/utility_types/misc.rs:11-15`, `:103-110`). Hints still say "15° Increments" (`ST:1544`, `ST:1550`).
- **Arrow**: same gesture as Line; no update while shorter than `1e-6` (`SH/arrow_shape.rs:50`). Options: shaft, head width, head length, each min 0.1 max 1000 px (`ST:237-283`).

### 6.5 How each primitive becomes Bezier geometry [read]

All in `ALG/shapes.rs`. There are no true arcs anywhere: the path model is `CurveType::{Linear, Quadratic, Cubic}` only (`node-graph/libraries/vector-types/src/vector/path.rs:12-20`).

- **Rectangle**: 4 line segments, order `corner1, (x2,y1), corner2, (x1,y2)`, closed (`ALG/shapes.rs:58-60`).
- **Rounded rectangle**: per corner two anchors and one cubic with handle length `radius * HANDLE_OFFSET_FACTOR`; radii `[top_left, top_right, bottom_right, bottom_left]` (`ALG/shapes.rs:63-94`). Overlapping radii are scaled down by the CSS corner-overlap rule when `clamped` (default true) (`GEN:140-155`).
- **Ellipse / Circle**: 4 cubic segments, anchors top, right, bottom, left; handle offset `size * HANDLE_OFFSET_FACTOR * 0.5` with `const HANDLE_OFFSET_FACTOR: f64 = 0.551784777779014;` (`ALG/shapes.rs:14`, `:97-115`). Note this is the Pomax least-max-error constant, not `0.5522847498`. Circle is `ellipse_bezpath(-r, r)` (`GEN:21-22`).
- **Arc**: `segments = ceil(|sweep| / (PI/4))`, min 1, so at most 45 deg per cubic; handle length `radius * 4/3 * sin(step/2) / (1 + cos(step/2))` along the tangent (`ALG/shapes.rs:144-146`, `:161-162`). Closed adds the chord; PieSlice adds the centre as a sharp anchor (`:170-174`). Sweeps beyond a full turn are folded (`:120-138`).
- **Polygon**: vertices at `center + radius * (cos a, sin a)`, `a = i * TAU/sides - PI/2`, `sides.max(3)` (`ALG/shapes.rs:179-192`).
- **Star**: `2 * sides` vertices alternating outer and inner radius, step `PI/sides`, same start angle, `sides.max(2)` (`ALG/shapes.rs:195-210`).
- **Line**: one line segment. **Arrow**: closed 8-point polygon (`ALG/shapes.rs:218-248`).
- **Spiral**: one cubic per `delta_theta` step (default 90 deg), handle length = exact arc length of the step / 3 along the analytic tangents (`ALG/shapes.rs:262-309`). Archimedean `r = a + b*theta`; logarithmic `r = a * e^(b*theta)`.
- **Grid**: points at `(spacing.x * col, spacing.y * row)`, joined according to a topology option (`GEN:399-470`).

Y-down assumptions to re-derive for DreamCarve [read + inferred]: the first polygon vertex at `-PI/2` is the *top* vertex only because Y points down; ellipse anchors go top -> right -> bottom -> left, which is clockwise on screen but positive signed area in raw coordinates; arcs with positive sweep run clockwise on screen; spiral points and the radius gizmo use `(r cos t, -r sin t)` (`ALG/shapes.rs:351`, `GZ/circle_arc_radius_handle.rs:254-256`); grid code comments say "Y increases downward" (`SH/grid_shape.rs:253`). The file header warns "Anchor order and winding direction are load-bearing" (`ALG/shapes.rs:3`).

### 6.6 Editing an existing shape with the Shape tool [read]

- **Line / arrow ends**: both ends of every selected line or arrow are drawn as 6 px squares, and as a hover anchor when the pointer is within `BOUNDS_SELECT_THRESHOLD` (10 px) (`SH/line_shape.rs:105-115`). Dragging an end keeps the other end fixed and reuses the drawing code, so Shift / Ctrl / Alt behave as when drawing (`SH/shape_utility.rs:207-212`, `ST:1277-1293`).
- **Everything else**: the standard transform cage (resize, rotate, skew). Resize modifiers: Alt "From Pivot", Shift "Preserve Aspect Ratio"; rotate: Shift "15° Increments"; skew: Ctrl "Unlock Slide" (`ST:1615-1629`). The cage itself is covered in area 4.
- A singular layer transform is nudged by `IDENTITY * 1e-4` so the cage can still be drawn (`SH/shape_utility.rs:231-236`).

### 6.7 On-canvas gizmos (parameter handles after creation) [read]

Shown only in `Ready` and only while Ctrl is not held (`ST:906-909`). Handlers exist for Star, Heart, Polygon, Arc, Circle, Grid, Spiral (`gizmos/gizmo_manager.rs:28-38`); **Rectangle, Ellipse, Line and Arrow have none**. A hovered gizmo wins over the cage. All are hidden when the shape is small on screen: `GIZMO_HIDE_THRESHOLD = 20.` px from centre to rim/vertex (`editor/src/consts.rs:177`).

| Gizmo | Controls | Hit zone | Drag mapping | Snaps to |
|---|---|---|---|---|
| Radius ring (Circle, Arc) | `radius` | a band around the rim: stroke width + up to 10 either side, shown as dashed ellipses (`GZ/circle_arc_radius_handle.rs:63-89`, `:148-149`) | pointer movement along the spoke, accumulated per move (`:177-185`) | radii of up to `MAX_EQUAL_RADII = 32` nearest circles/arcs/polygons/stars (`:21`, `:208-252`) |
| Sweep ends (Arc) | start angle and sweep | within 5 px of either end (`GZ/sweep_angle_gizmo.rs:74-76`) | angle of pointer about the centre in the arc's own space; dragging the start changes start and sweep, the end only sweep (`:196-335`) | multiples of 45 deg, 0..360; 90 and 270 flagged as right angles (`:380-392`) |
| Vertex handle (Polygon, Star) | radius; for a star the outer or inner radius depending on the vertex | within 5 px of a vertex (`GZ/point_radius_handle.rs:83`, `:110`) | projection of the drag onto the spoke (`:447-455`) | star only: the inner radius giving 90 deg tips, and radii that line tips up with other vertices (`:340-399`) |
| Sides dial (Polygon, Star) | number of sides | within `NUMBER_OF_POINTS_DIAL_SPOKE_LENGTH = 10.` px of the centre (`editor/src/consts.rs:174`, `GZ/number_of_points_dial.rs:63`) | `round(distance / 25)` sides, sign from horizontal direction, min 3 (`:192-208`) | integer by construction |
| Turns handle (Spiral) | turns (end) or start angle + turns (start); outer radius follows so tightness is kept | within `SPIRAL_END_HOVER_RADIUS = 8.` px of either end (`editor/src/consts.rs:171`, `GZ/spiral_turns_handle.rs:106-110`) | angle swept about the centre / 360; min 0.5 turns, outer radius min 0.1 (`:200-292`) | whole and half turns (`:187-197`) |
| Row/column bars (Grid) | rows or columns | a bar offset `GRID_ROW_COLUMN_GIZMO_OFFSET = 15.` px outside each of the 4 sides; hit when inside its rect or `distance_sq < 32.` (`editor/src/consts.rs:178`, `GZ/grid_rows_columns_gizmo.rs:162-177`) | `floor(delta / spacing)` added, min 1 (`:115-116`) | integer by construction |
| Heart handles | six proportions | `HEART_HANDLE_HOVER_RADIUS = 8.` px | direct or slider-style over `HEART_SLIDER_DRAG_DISTANCE = 200.` px | none |

All value snaps go through `SnapSession::snap_value(view, position_of, free, on_screen_per_unit, candidates)` (`editor/src/messages/tool/common_functionality/snap_session.rs:344-347`): a candidate is in reach when the *handle's on-screen travel* to it is within the snapping reach, so the tolerance is in pixels regardless of zoom or stretch. The radius ring is also drawn while a circle is being drawn (`ST:985-987`).

### 6.8 Options bar and hints [read]

Options bar (`ST:439-538`): Fill (hidden for Line), swap fill/stroke, Stroke, stroke options, then (not for the L/M/E aliases) the shape dropdown and per-shape fields: Sides (Polygon, Star); arc type radio (Arc); spiral type + Turns (Spiral); grid type (Grid); Shaft / Head W / Head L (Arrow). Selecting a layer of one of these shapes pulls its values into the bar (`ST:317-422`).

Hints (`ST:1507-1636`), compactly:

- Ready: "Draw <Shape>" + Shift "Constrain Square / Circular / Regular / Arc" + Alt "From Center"; Line/Arrow: Shift "15° Increments", Alt "From Center", Ctrl "Lock Angle"; Polygon/Star: `[` `]` "Decrease/Increase Sides"; Spiral: `[` `]` "Decrease/Increase Turns"; Circle: Alt only.
- Any drag: RMB / Esc "Cancel", then the same modifier list.
- Every state appends the shared `snapping_hints()` group.

### 6.9 How snapping is invoked [read]

- Hover in `Ready`: `snap_point` on every pointer move so the user sees where a press would start; suppressed (`forget`) when the press would grab a gizmo, cage or line end instead (`ST:1370-1380`).
- Press: `Resize::start` takes the shown snap as the start and begins a gesture marked as "drawing something not yet in the document" (`RS:17-35`).
- Move: `snap_features` for boxes (9 features or the shape's own points), `snap_point` for line / spiral / grid corner.
- Auto-pan while dragging outside the viewport (`ST:1271-1273`).

### 6.10 NOT present [read: absence checked in `ST`, `SH/`, `gizmos/`, `GEN`]

- Rounded-rectangle on-canvas corner-radius handles. Corner radius exists only as node inputs `corner_radius` / `clamped` (`GEN:129-130`); no Rectangle gizmo handler exists.
- Slot / obround, 3-point arc, 3-point circle, 2-point (diameter) circle, tangent circle or tangent arc, elliptical arc primitive.
- Centre + Euclidean-radius circle (Alt gives centre-out but the radius is the larger of |dx|, |dy|).
- Typed dimensions while dragging (no numeric-entry state in the tool; values are edited afterwards in the options bar).
- Polygon inscribed / circumscribed choice (radius is always centre-to-vertex) and polygon rotation parameter.
- Rotated rectangle by three points; drawing aligned to document axes on a tilted canvas.
- Angle snapping for any box shape; only Line and Arrow have it.
- Click-to-place with a default size; any minimum size while drawing.
- Multi-segment polyline in the Line tool (single segment only).
- Dimension read-outs while drawing, except the arc sweep angle text (`GZ/sweep_angle_gizmo.rs:185`).

### 6.11 Upstream test names worth mirroring [read]

`RS:236-381` and `ST:1722-2494` have behaviour-named tests, for example: `a_rectangle_drawn_with_shift_is_square_with_its_larger_extent_and_snaps_along_its_diagonal`, `a_rectangle_drawn_from_its_center_with_a_corner_near_a_corner_of_another_lands_on_it_and_stays_centered`, `a_rectangle_pressed_a_few_pixels_from_a_corner_of_another_starts_on_it`, `escape_while_drawing_a_rectangle_leaves_no_layer_and_ends_the_snapping_gesture`, `an_odd_polygon_or_star_dragged_upward_is_drawn_flipped_to_point_down`, `a_polygon_drawn_with_only_the_corner_of_its_box_near_a_corner_of_a_rectangle_does_not_snap`, `a_line_drawn_from_its_center_can_snap_at_the_end_away_from_the_pointer`, `the_shape_tools_own_key_mid_drag_is_ignored_and_leaves_nothing_to_undo_later`.

---

## 7. Spline, freehand and text tools

### 7.1 Spline tool

Source: `editor/src/messages/tool/tool_messages/spline_tool.rs` (tool: 1-693). Bindings `input_mappings.rs:299-305`. Solver: `node-graph/libraries/vector-types/src/vector/algorithms/spline.rs`. Spline node: `node-graph/nodes/vector/src/vector_nodes.rs:2609-2685` (read with `git show`, not in the sparse checkout).

**Model [read]**: a spline layer is a `Path` node feeding a `Spline` node (`spline_tool.rs:445-449`). The Path node stores only the clicked anchors joined by straight segments (`handles: [None, None]`, 664); the Spline node recomputes cubic handles on every evaluation. The curve therefore stays "live": moving an anchor later re-solves the whole run.

**States [read]** (`spline_tool.rs:68-73`): `Ready`, `Drawing`, `MergingEndpoints`.

**Bindings [read]**: LMB down `DragStart { append_to_selected: Shift }`; LMB up `DragStop`; **RMB, Escape and Enter all send `Confirm`** (`input_mappings.rs:303-305`). `Abort` only arrives from a tool switch. The Drawing-state hint nevertheless reads "RMB / Esc: Cancel" (607), which does not match the binding.

**Behaviour [read]**
- Ready, pointer move: `snap_point` to preview where a press would start (534-541).
- Press (386-465): start = `snap_session.snap_press`. If within `SNAP_POINT_TOLERANCE` (5 px) of any anchor of any non-artboard layer: when that layer already has a Spline node, extend it from that anchor (the release of this same click is swallowed, 468-471); otherwise remember that layer and anchor to be merged with the new spline's start when it is finished. With Shift and exactly one selected layer, the new points are added to that layer. Otherwise a new Path+Spline layer is created.
- **Points are added on mouse release**, not press (466-501), at the snapped pointer position, and only if farther than `DRAG_THRESHOLD` (`1.`, compared in layer-local units, 490) from the last point. A press-and-release without motion places the point where the press landed (`press` field, 312-323).
- Pointer move while drawing (502-533): if the pointer is within `PATH_JOIN_THRESHOLD = 5.` px (`consts.rs:159`) of another anchor of the *same* layer (excluding the preview point and the last point) the preview point lands exactly on it and general snapping is dropped (`snap_session.forget()`); otherwise general snapping via `snap_point(.., PointOptions::default())`. A temporary preview point + segment are inserted and removed each move (`extend_spline` 648, `delete_preview` 679).
- After each placed point, if it is within 5 px (`PATH_JOIN_THRESHOLD`) of an anchor on any non-artboard layer, the join is recorded and the spline is **confirmed automatically** (495-497, `try_merging_lastest_endpoint` 623). This is how a spline is closed: click on its first point.
- Confirm (558-568): with 0-1 points, abort; else remove the preview and run `MergingEndpoints`, which merges the recorded layers into the spline's layer (`merge_layers`) and welds the recorded endpoint pairs (`merge_points`, `graph_modification_utils.rs:171`), then ends the history step.
- Abort by tool switch (569-580): keeps what was drawn (if 2+ points) but skips the endpoint joins.
- Snapping exclusions: `Moving { layers: [current_layer] }` (302-310), i.e. the spline being drawn is not a snap source for itself (tests 1105, 1121).
- Overlays: only the open-path endpoint markers of selected layers (`open_path_endpoint_overlays`, `overlays/utility_functions.rs:278`), with the endpoint a press would continue from drawn in hover style.
- Options: fill (off by default, `DrawingToolState::new(false)`), stroke, stroke popover.

**How the curve is computed [read code; classification inferred]**
- Per run of anchors (`vector_nodes.rs:2632-2678`): fewer than 2 anchors -> skipped; `closed = closed && positions.len() > 2`.
- Open: `solve_spline_first_handle_open(points)` (`spline.rs:4-57`). Tridiagonal system solved with the Thomas algorithm for the **first (outgoing) handle at every anchor**: diagonal `b = [2, 4, ..., 4, 2]`, off-diagonals `1`, right side `d[0] = p0 + 2 p1`, `d[i] = 4 p_i + 2 p_{i+1}`, `d[n-1] = 3 p_{n-1}`. The incoming handle at each anchor is the mirror `2 p_i - h_i`. These are the C2 conditions with zero second derivative at both ends, i.e. a **natural cubic spline** through the anchors. **[the "natural" label is inferred from the end rows]**
- Closed: `solve_spline_first_handle_closed(points)` (`spline.rs:61-137`): periodic tridiagonal `[1, 4, 1]` system with right side `3 (p_{i+1} - p_{i-1})` solved by the Sherman-Morrison variant of Thomas (`gamma = -b[0]`), giving the derivative at each anchor; handle = `p_i + x_i / 3`. Returns empty for fewer than 3 points. C2 periodic.
- Each span `i` is the cubic `(p_i, h_i, 2 p_{i+1} - h_{i+1}, p_{i+1})` (2663-2667); interior anchors are flagged smooth (2645-2650, 2680).
- With exactly 2 points the open solver yields a straight line (handles at 1/3 and 2/3). **[inferred from the 2x2 system]**

**NOT present (spline)**: no tension/bias/alpha parameter, no Catmull-Rom or B-spline (control-polygon) mode, no per-point corner flag, no Shift angle constraint, no Ctrl lock, no way to cancel a spline with Escape (it confirms), no undo of the last point via Backspace (an `Undo` message exists in the enum but has no transition arm), no arc/biarc fitting, no conversion to arcs. Baking to a plain editable path is not in this tool.

### 7.2 Freehand tool (brief)

Source: `editor/src/messages/tool/tool_messages/freehand_tool.rs` (tool: 1-435). Bindings `input_mappings.rs:292-297`.

**[read]**
- States `Ready`, `Drawing` (71-76). Press starts, every `PointerMove` appends, release ends, Escape/RMB aborts the whole stroke.
- **Sampling**: one anchor per pointer-move event at the raw pointer position converted to layer space, joined to the previous anchor by a **straight segment** (`handles: [None, None]`), `extend_path_with_next_segment` (406-435). The only filter is "position differs from the last one and is finite" (407). No distance or time decimation.
- **No fitting, smoothing or simplification** anywhere in the tool; the hint literally says "Draw Polyline" (391). Any smoothing would have to be a separate node applied afterwards.
- **No snapping**: the tool never touches the snap session.
- Extending: pressing within `SNAP_POINT_TOLERANCE` (5 px) of an **open endpoint** (an anchor with exactly one segment, `closest_open_path_endpoint`, `utility_functions.rs:47-49`) of a selected visible layer continues that path in place (284-292); this takes priority over Shift-append (test 845). Middle anchors and closed paths never extend (tests 767, 786). Shift with exactly one selected non-artboard layer appends a new sub-path to it (294-309). Otherwise a new Path layer, fill off by default.
- A press and release with no movement on an endpoint changes nothing (test 866).

**NOT present (freehand)**: curve fitting (no Schneider/Ramer-Douglas-Peucker pass), pressure, stabiliser/lazy-mouse, minimum sample spacing, snapping, auto-close when ending near the start.

### 7.3 Text tool (brief)

Source: `editor/src/messages/tool/tool_messages/text_tool.rs` (tool: 1-1165). Bindings `input_mappings.rs:169-175`.

**[read]**
- States (434-447): `Ready`, `Editing`, `Placing` (dragging out a new text box), `Dragging` (moving an existing text layer), `ResizingBounds` (dragging the selected text layer's cage).
- Press in `Ready` (748-816), in priority order: on a resize edge/corner of the selected text layer's cage -> `ResizingBounds`; inside the bounding quad of any text layer (`check_click`, 616) -> select it and `Dragging`; otherwise -> `Placing`, with the start corner taken from the snap session (`Resize::start`, shared with the shape tool).
- Release from `Placing` (1004-1033): "dragged" means `|start - end|^2 > DRAG_THRESHOLD^2` with `DRAG_THRESHOLD = 1.` px.
  - **Click** -> point text: no width/height limit (`max_width`/`max_height` = `None`).
  - **Drag** -> box text: `max_width`/`max_height` = the dragged size divided by the viewport zoom; lines wrap to the width.
  - While dragging: **Shift = "Constrain Square"**, **Alt = "From Center"** (`PointerMove { center: Alt, lock_ratio: Shift }`, `input_mappings.rs:170`; hints 1131-1133), computed by `Resize::calculate_points_ignore_layer` (855).
- Release from `Dragging` with effectively no movement (`< 10. * f64::EPSILON`, 1035) -> enter `Editing` on that layer, i.e. click on existing text to edit it; a real drag moves the layer (raw pointer delta, **no snapping**, 868-891).
- `ResizingBounds` (892-959): Shift = lock aspect ratio, Alt = from centre; sets the Text node's `HasMaxWidth`/`MaxWidth`/`HasMaxHeight`/`MaxHeight` (both always, see the TODO at 926) and re-positions the layer; size snapping goes through the snap session (`SizeSnapData`, 906-909). It changes the wrap box, not the font size or scale.
- `Editing`: text entry is delegated to the **frontend** (`FrontendMessage::DisplayEditableTextbox`, 501: an HTML editable element positioned with a CSS matrix), not implemented in Rust. Commit on Escape, RMB, or Ctrl/Cmd+Enter (`input_mappings.rs:173-175` all map to `Abort`, which in `Editing` triggers a commit, 1097-1104) or on clicking elsewhere. Committing an **empty** string deletes the layer (482-494, 1077-1079, 1098-1100). There is no "cancel edits" path: Escape commits.
- Overlays (669-738): translucent box under the text being edited; the drag-out rectangle while placing; the selected text layer's cage; a red (`COLOR_OVERLAY_RED`, thickness `3.`) line along the bottom edge when lines are clipped by the max height (722-729).
- Options (36-58, 119-245): font family, font style, size (default `24.` px, min `1.`), fill colour, alignment. `letter_spacing` and `letter_tilt` exist in the options struct (defaults `0.`) but only the widgets listed are built in the bar. **[the "only those widgets" part is from a grep of the widget builder, not a full read]**
- New text is created as a text node layer with a deferred transform (568-614).

**NOT present (text)**: no convert-to-outlines / "create outlines" action in the tool (no match for `outline`, `to_path` or `flatten` in the file); no text on a path; no single-line (stick/engraving) fonts; no per-glyph selection or kerning edit; no in-Rust text editing or caret logic; no vertical text; no snapping when moving a text layer with this tool; no numeric box-size entry. Whether the Text node's output is already vector outlines consumable by other nodes is outside the files read. **[not verified]**

### 7.4 Viewport-space and Y-down notes for all four tools **[inferred unless marked]**

- Every hit tolerance above (`SNAP_POINT_TOLERANCE`, `PATH_JOIN_THRESHOLD`, `HIDE_HANDLE_DISTANCE`, `SEGMENT_OVERLAY_SIZE`) is measured in **viewport pixels** after `transform_to_viewport`, so it is zoom-independent. Exceptions that are measured in document/layer units and therefore scale with zoom: the pen's Ctrl `20.` radius (`pen_tool.rs:1181, 1188, 1203`) and the spline's `DRAG_THRESHOLD` minimum point spacing (`spline_tool.rs:490`). **[read]**
- Geometry is stored in layer-local space with a per-layer transform; the pen creates the first anchor at local `(0,0)` and translates the layer. DreamCarve, with no per-object transform, can store absolute coordinates and skip all the "deferred transform" handling (`new_layer_viewport_start`, `RecalculateLatestPointsPosition`, `merging_from`).
- Nothing in these four tools depends on winding or on which way Y points, except the text box (top-left origin, text flows toward +Y) and the perpendicular tick direction (symmetric). Angle steps and locks are mirror-symmetric.

---

## 8. Geometry helpers worth porting

### 8.1 What is under `libraries/` [read]

| Crate | What it is |
|---|---|
| `control-protocol` | Commands/results/scenario files for scripting and QA of the editor, one JSON line per message |
| `dyn-any` | "An Any trait that works for arbitrary lifetimes" |
| `ipsum` | Lorem Ipsum generator (Markov chain) |
| `math-parser` | "Parser for Graphite style mathematics expressions" (see 8.2) |
| `rawkit` | Camera raw image decoder |
| `snapping` | "Snapping engine that finds the relationships a moving point can take with the geometry around it"; depends only on `glam` and `kurbo` (`libraries/snapping/Cargo.toml:17-19`) |
| `wgpu-sync` | wgpu wrappers that synchronise against surface reconfiguration |

- There is **no `bezier-rs`** and **no `path-bool`** under `libraries/` at this commit.
- Boolean operations live at `node-graph/nodes/path-bool` (`Cargo.toml:111`; not in the sparse checkout, not read).
- Curve maths is delegated to external crates (root `Cargo.toml`): `kurbo = "0.13"` (`:214`), `lyon_geom = "1.0"` (`:265`, used only for cubic-cubic intersection), `linesweeper = "0.5"` (`:288`; [inferred] the boolean backend, not read), `glam 0.34` (`:225`).
- The reusable vector code is in `node-graph/libraries/vector-types/src/vector/` (`ALG/` plus `misc.rs`, `click_target.rs`).

### 8.2 `math-parser` in brief [read: `lib.rs`, `lexer.rs`, `ast.rs`, `constants.rs`, `context.rs` skimmed]

- Entry point: `evaluate(expression: &str) -> Result<Result<Object, EvalError>, ParseError>` (`libraries/math-parser/src/lib.rs:22`). Built on `chumsky 0.10` and `num-complex`.
- Operators: `+ - * / ^`, comparisons, logical and/or (`ast.rs:18-33`). Implicit multiplication (`2pi`, `2e`), scientific notation (`2.5e3`).
- Constants: `pi`/`π`, `tau`/`τ`, `phi`/`φ`, `e`, `inf`, `i j k`, `true`, `false` (`lexer.rs:173-188`).
- Around 100 functions (`constants.rs:468-964`): trig and inverse/hyperbolic, `sqrt cbrt root ln log exp`, `abs floor ceil round trunc fract sign`, `snap`, `mod`, `min max clamp remap lerp smoothstep`, statistics, vector (`dot cross normalize distance angle rotate`), matrices.
- Variables and host functions are supplied through `ValueProvider` / `FunctionProvider` on `EvalContext` (`context.rs:20-75`); `where` bindings exist in the grammar.
- Values can be complex numbers, vectors, quaternions and matrices, which is far more than a numeric field needs.
- **No physical units**: `grep -i unit` finds nothing in `lexer.rs`, `parser.rs`, `ast.rs`. "12mm + 1/4in" would not parse. Units in Graphite are a display suffix on the number widget (`.unit(" px")`, `ST:239`).

### 8.3 Answers per question

| Need | Where | Algorithm and constants | Hand-rolled or kurbo |
|---|---|---|---|
| Nearest point on a segment | Editor: `PathSeg::nearest(point, DEFAULT_ACCURACY)` (`editor/src/messages/tool/common_functionality/shape_editor.rs:233`, `:1858`; `tool_messages/path_tool.rs:1347`). Snapping: `Curve::nearest(point) -> NearestOnCurve { t, position, distance }` (`libraries/snapping/src/curve.rs:157`) | Snapping version is exact: roots of the perpendicularity polynomial (degree 5 for a cubic) plus both ends; ties go to the smallest t | Editor: kurbo. Snapping: hand-rolled polynomial root finder (`curve.rs:663-760`) |
| Perpendicular foot / tangent point from a point | `Curve::perpendicular_parameters_from` (`curve.rs:180`), `Curve::tangent_parameters_from` (`curve.rs:193`) | Polynomial roots in [0,1]; `PARAMETER_SLACK = 1e-9`, `ROOT_SEPARATION = 1e-9` (`curve.rs:13-16`) | Hand-rolled |
| Line - curve intersection | `Curve::line_intersections(point, direction)` (`curve.rs:211`); vector-types uses `PathSeg::intersect_line` (`ALG/intersection.rs:57-58`) | Substitute into the line equation, cubic roots | Snapping hand-rolled; vector-types kurbo |
| Circle - curve intersection | `Curve::circle_intersections(center, radius)` (`curve.rs:228`) | Degree-6 polynomial with a cancellation-safe constant term | Hand-rolled. Useful for arc segments |
| Curve - curve intersection | Snapping: `Curve::curve_intersections(&other) -> Vec<(t, u)>` (`curve.rs:245`). Vector-types: `segment_intersections` (`ALG/intersection.rs:53`), `filtered_segment_intersections` (`:160`), `bezpath_intersections` (`:42`) | Snapping: collinear and straight cases handled exactly, otherwise mutual subdivision to `LEAF_SIZE = 1e-6` of the combined diagonal, then Newton (`MAX_NEWTON_STEPS = 50`); duplicates merged at `INTERSECTION_SEPARATION = 1e-6`. Vector-types: cubic-cubic via `lyon_geom::cubic_intersections_t`; mixed quad cases by bounding-box subdivision to `DEFAULT_ACCURACY`, capped at 50 hits; neighbours closer than `MIN_SEPARATION_VALUE = 5. * 1e-3` in t are merged (`ALG/consts.rs:2`) | Snapping hand-rolled (the better one to port); vector-types lyon + hand-rolled |
| Self-intersection of one cubic | `pathseg_self_intersections` (`ALG/intersection.rs:246`) | Split into quads with `to_quads`, intersect non-adjacent pieces | Hand-rolled on kurbo |
| Line - line | `Curve::straight_intersection` (private, `curve.rs:366`); `miter_line_join` (`ALG/bezpath_algorithms.rs:445`) | Direct solve | Hand-rolled |
| Common tangent line of two curves | `common_tangents(first, second) -> Vec<(s, t)>` (`libraries/snapping/src/tangency.rs:12`) | Newton from `SEEDS_PER_CURVE = 12` seeds per curve, `MAX_ITERATIONS = 30` | Hand-rolled |
| Split at t / sub-segment | `PathSeg::subsegment(a..b)`, `subdivide()`; `split_bezpath(bezpath, TValue)` (`ALG/bezpath_algorithms.rs:62`), `trim_pathseg` (`:220`) | de Casteljau | kurbo; thin wrappers |
| Point at arc length | `eval_pathseg_euclidean(segment, fraction, accuracy)` (`ALG/bezpath_algorithms.rs:298`), `evaluate_bezpath(bezpath, TValue::Euclidean(f), ..)` (`:72`), `compute_sample_locations` (`:98`) | `arclen` then `inv_arclen` with accuracy `max(accuracy, length * 1e-14)` | kurbo |
| Length | `segment.arclen(accuracy)`, `segment.perimeter(DEFAULT_ACCURACY)`; `pathseg_length_centroid_and_length` (`ALG/bezpath_algorithms.rs:240`) | Centroid version: de Casteljau recursion, max depth 8 | kurbo; centroid hand-rolled |
| Tangent | `pathseg_tangent(segment, t)` (`ALG/util.rs:4`); `Curve::tangent(t)` (`curve.rs:98`) | The snapping version falls back to higher derivatives at retracted handles and cusps, so it never returns zero | kurbo derivative; fallback hand-rolled |
| Normal / curvature | No public helper. Private `signed_curvature(cubic, t)` in `ALG/concave_hull.rs:247` | - | Would use kurbo `ParamCurveCurvature` [inferred] |
| Offset | `offset_bezpath(bezpath, distance, join, miter_limit)` (`ALG/offset_bezpath.rs:23`) | Per segment `kurbo::offset::offset_cubic` with `CUBIC_REGULARIZATION_ACCURACY = 0.5` (`:7`); then clip overlapping neighbours at their intersection or add miter (default limit 4) / round joins. Does **not** remove global self-intersections | kurbo + hand-rolled joins. Not good enough for CAM offsets |
| Bounding box | `segment.bounding_box()` (tight), `Curve::bounds()` (`curve.rs:116`), `Vector::bounding_box_with_transform` (`vector_types.rs:239`) | Extrema | kurbo |
| Flatten to polyline | None in the code read. `flatten.rs` is about the vector *data model*, not polylines | - | kurbo's `flatten` would be used [inferred] |
| Fit one cubic to two consecutive segments (used only when an anchor is slid along its path, `tool_messages/path_tool.rs:1419`; deleting an anchor does NOT refit, see 2.8) | `fit_handles_to_segment_pair(start, end, start_dir, end_dir, first, second) -> (handle, handle)` (`editor/src/messages/tool/common_functionality/utility_functions.rs:531`) | 16 samples per segment, weighted least squares for the two handle lengths with fixed directions, 12 Gauss-Newton passes, tangential weight 0.01 (`:523-527`). Rebuilds an exactly split cubic exactly | Hand-rolled. Worth porting |
| Fit a curve through freehand points | Not in the tool: no `fit`/`simplify` call in `freehand_tool.rs` (grep) | - | Absent in the files read |
| Spline through points | `solve_spline_first_handle_open(points)`, `solve_spline_first_handle_closed(points)` (`ALG/spline.rs:4`, `:61`) | Tridiagonal solve for the first handle of each anchor; the other is its mirror | Hand-rolled, pure, easy to port |
| Segment molding | `ClosestSegment::mold_handle_positions` (`editor/src/messages/tool/common_functionality/shape_editor.rs:355`) | Adds the drag delta to both handles; neighbours' colinear handles are re-aimed, lengths kept (`utility_functions.rs:142`) | Hand-rolled, but tied to editor messages |
| Colinearity test | `is_almost_colinear(point, handle1, handle2)` (`utility_functions.rs:722`) | angle to the negated other handle `< 1e-6` rad | Hand-rolled |
| Is a segment really a line | `is_linear(segment)` (`node-graph/libraries/vector-types/src/vector/misc.rs:371`); `Curve::is_straight()` (`curve.rs:126`) | Cross product `< MAX_COINCIDENT_POINT_DISTANCE = 1e-7` (`ALG/consts.rs:8`); the snapping one is scale-relative (`COINCIDENCE = 1e-9`) and also checks handle order | Hand-rolled |
| Inside test | `BezPath::winding(point) != 0` / `contains` (`shape_utility.rs:553`, `vector_types.rs:288`); `bezpath_is_inside_bezpath` (`ALG/bezpath_algorithms.rs:534`) | Non-zero rule; path-in-path = bbox, all anchors inside, no intersections | kurbo + thin wrapper |
| Area, centroid, direction | `bezpath_area_centroid_and_area` (`ALG/bezpath_algorithms.rs:621`), `bezpath_length_centroid_and_length` (`:675`), `Vector::normalize_winding_directions` (`vector_attributes.rs:806`), `reverse_path_flows` (`path.rs:252`) | Green's theorem on the segment polynomials | Hand-rolled; kurbo `Shape::area()` gives signed area |
| Hit-test a shape with a point or lasso | `ClickTarget::intersect_point`, `intersect_path` (`click_target.rs:415`, `:365`) | Point is inflated to a stroke-width square, then outline intersection, then fill winding, then containment | Hand-rolled on kurbo |
| Arc to Bezier | `arc_bezpath` (`ALG/shapes.rs:118`), `ellipse_bezpath` (`:97`) | See 6.5 | Hand-rolled |
| **Bezier to arc / arc fitting / biarc** | **Absent.** grep for `biarc`, `to_arcs`, `kurbo::Arc` finds nothing in vector-types, the tools or snapping | - | DreamCarve must write this itself |
| Weld / join close points | `WeldExt::{weld_connected, weld_all, weld_within_pieces, weld_flow_ends, unweld}` (`ALG/weld.rs:14-25`) | Spatial grid + union-find | Hand-rolled, bound to Graphite's `Vector` model |

Other files, one line each [read: headers and signatures only]:

- `ALG/convex_hull.rs`: exact convex hull of curved geometry (`convex_hull_of_geometry`, `:89`).
- `ALG/concave_hull.rs`: rolling-ball concave hull (`concave_hull_of_geometry`, `:104`).
- `ALG/poisson_disk.rs`: Poisson-disk point sampling inside paths.
- `vector/misc.rs`: glam/kurbo conversions (`point_to_dvec2`, `dvec2_to_point`), `segment_to_handles` / `handles_to_segment` (`:266`, `:277`), `bezpath_from_anchors_and_handles` (`:303`), shape enums.
- `vector/flatten.rs`: `WorkingVector`, a flat point/segment working copy of the path model for topology edits.
- `vector/contour.rs`: walks the path model into contours (`contours`, `oriented_contours`, `contour_bezpaths`).
- `vector/connectivity.rs`: which paths reference each point (`Connectivity::build`, `spokes`).
- `libraries/snapping/src/constructions.rs`: turns "woken" anchors, boxes and curves into construction lines (axis lines through a point, curve extensions, line through two points, tangent/perpendicular lines); `find_constructions` (`:98`), `find_constructions_from(context, previous, previous_length)` (`:213`). Behaviour belongs to area 3.

### 8.4 Shared tolerances, verbatim [read]

`ALG/consts.rs`:
```rust
pub(crate) const MIN_SEPARATION_VALUE: f64 = 5. * 1e-3;          // :2
pub(crate) const MAX_ABSOLUTE_DIFFERENCE: f64 = 1e-3;            // :5
pub(crate) const MAX_COINCIDENT_POINT_DISTANCE: f64 = 1e-7;      // :8
pub(crate) const CLOSE_POINT_TOLERANCE: f64 = 1e-6;              // :12
```
`libraries/snapping/src/curve.rs:7-37`: `COINCIDENCE = 1e-9`, `NEGLIGIBLE = 1e-12`, `PARAMETER_SLACK = 1e-9`, `ROOT_SEPARATION = 1e-9`, `INTERSECTION_SEPARATION = 1e-6`, `LEAF_SIZE = 1e-6`, `MAX_SUBDIVISION_DEPTH = 40`, `MAX_LIVE_PAIRS = 4096`, `MAX_ROOT_REFINEMENT_STEPS = 100`, `MAX_NEWTON_STEPS = 50`, `NEWTON_MARGIN = 1e-3`, `COEFFICIENT_COUNT = 7`. All the fractional ones are relative to the size of the geometry, not absolute.

`kurbo::DEFAULT_ACCURACY` is the accuracy passed to `nearest`, `arclen` and `perimeter` throughout; its value was not read here (CLOSE_POINT_TOLERANCE's comment says it "matches Kurbo's default path accuracy", i.e. 1e-6).

### 8.5 Porting recommendation [inferred]

- `libraries/snapping/src/curve.rs` is the most valuable single file: self-contained (glam + kurbo), exact, scale-relative tolerances, and it already has `circle_intersections`, which maps onto DreamCarve's bulge arcs. Its test list (`curve.rs:900-1366`) is a ready-made behaviour spec.
- `fit_handles_to_segment_pair` and the two `solve_spline_*` functions are small, pure and portable.
- Do not port `offset_bezpath` for toolpaths; DreamCarve already has polyline/arc offsets and this one leaves global self-intersections.
- Everything here assumes cubic Beziers. Arc segments need their own nearest / split / intersect paths, and Bezier-to-arc fitting has no upstream reference at all.

### 8.6 Not verified

- `node-graph/nodes/path-bool` and `linesweeper` were not read.
- `kurbo` internals and the numeric value of `DEFAULT_ACCURACY` were not opened.
- `GZ/heart_handles.rs` and the overlay halves of the gizmo files were only skimmed.
- `math-parser` grammar (`parser.rs`) was not read end to end; the "no units" claim rests on grep.
- The `finish_transaction` unit mismatch in 6.2 is my reading of two files, not something upstream documents.

---

## 9. Open items and unverified claims

Collected from the per-section lists. None of these was resolved; treat each as
a question for the spec, not a fact.

### 9.1 Not verified

Pen, spline, freehand, text:

- `CREATE_CURVE_THRESHOLD` being unused rests on a grep of the sparse checkout (`editor/`, `libraries/`, `node-graph/libraries/vector-types`) only.
- Calling the open spline solver a natural cubic spline is an inference from its end-row equations.
- The mechanism that holds a pen handle at its anchor until 1 px of motion was read from doc comments, not traced.
- Whether typed numeric values work during the pen's G/R/S handle transform.
- Whether the Text node's output is usable vector outlines (outside the files read). The text tool itself has no convert-to-outlines action.
- The text options-bar widget list comes from a grep.

Path tool:

- `SEGMENT_SELECTED_THICKNESS = 3.` (`editor/src/consts.rs:114`) is not referenced in the checked-out paths; selected segments are stroked `4.` (`editor/src/messages/portfolio/document/overlays/utility_types.rs:1171`).
- Point hit-testing takes the first selected layer under threshold in `HashMap` order rather than the globally nearest point; the order dependence is inferred.
- The `10.` neighbour test at `path_tool.rs:3485` looks not zoom-compensated; inferred, not tested.
- The path tool's own `GRS` message is mapped (`input_mappings.rs:235-237`) but appears unhandled; the working bindings are the global ones (`input_mappings.rs:409-411`).
- Path direction markers, index labels and the curvature comb overlays exist but were not studied.

Snapping and grid:

- Whether locked layers are excluded from snap targets (`mirror_layer` has no lock test; a filter elsewhere was not searched for).
- Text layers contributing only outline and box is inferred.
- That the third isometric line family passes through the lattice of the first two is inferred from the spacing formulas.
- `libraries/snapping/tests/scenarios.rs` was skimmed by test name; `benches/scale.rs` was not read.
- `libraries/snapping/src/curve.rs` was read as signatures, doc comments and constants, not line by line.

Select, cage, measure:

- Whether click targets carry stroke-width slack (outside the files read).
- Default overlay line thickness and outline colour (assumed 1 px blue).
- Which tools besides Select, Shape, Path and Pen let modal G/R/S through.
- That the pivot lands on the nine reference points only through ordinary bounding-box snap targets; there is no dedicated code.
- Resize "jump to pointer" at an edge and flip-through-zero are read from the absence of an offset and of a clamp.

Shapes and geometry:

- `node-graph/nodes/path-bool` and `linesweeper` were not read; that linesweeper is the boolean backend is a guess.
- kurbo internals and the value of `DEFAULT_ACCURACY` were not opened; `1e-6` is implied by the comment at `node-graph/libraries/vector-types/src/vector/algorithms/consts.rs:10-12`.
- `gizmos/shape_gizmos/heart_handles.rs` and the overlay halves of the gizmo files were only skimmed.
- "No units" in `math-parser` rests on a grep, not a full read of `parser.rs`.
- The shape tool's click test in `finish_transaction` appears to compare a document-space start with a viewport-space pointer, exact only at 100 % zoom with no pan; a reading of two files.

### 9.2 Upstream oddities seen (do not port as behaviour)

- Pen `GRSHandle` Confirm/Abort re-sends `PointerMove` with Shift and Control swapped (`pen_tool.rs:1738-1744`).
- Spline tool: Escape and right-click are bound to `Confirm` (`input_mappings.rs:303-304`) although the hint says "Cancel".
- Pen hints say "15° Increments" although the step is a document setting.
- Dead constants: `CREATE_CURVE_THRESHOLD` (`editor/src/consts.rs:156`), `SEGMENT_SELECTED_THICKNESS` (`editor/src/consts.rs:114`).

### 9.3 Decisions the phase 5 spec has to make that Graphite cannot answer

- Arc segments in every tool: drawing them (3-point, tangent, centre), node-editing them (bulge handle), snapping to centre and quadrant points, splitting and nearest-point on an arc, and Bezier-to-arc fitting. No upstream reference exists for any of these.
- Whether snapping follows the drawn (zoom-coarsened) grid level, as Graphite does, or a fixed millimetre increment.
- Whether to allow non-uniform layer transforms on parametric shapes (Graphite stretches the layer transform for non-square polygon/star/arc drags) or bake them into geometry.
- Numeric entry while drawing, typed W/H on the cage, a point-to-point measure tool, angle measurement, persistent dimensions, distribute commands, node align, fillet and chamfer: all absent upstream.
