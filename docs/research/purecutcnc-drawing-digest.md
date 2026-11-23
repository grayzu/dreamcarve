# PureCutCNC 2D drawing and editing: digest for the DreamCarve phase 5 spec

Source: `/Users/markg/orca/workspaces/dreamcarve/purecutcnc`, HEAD `d04ae2f` (merge of PR #1240).
All paths are relative to that root. Line numbers are from that commit.

Conventions used here:

- **[read]** is the default: the claim comes from the file and lines cited.
- **[inferred]** marks anything deduced rather than read. Treat it as a lead, not a fact.
- "Y-down" always means PureCutCNC's internal convention (`ARCHITECTURE.md` §8, lines 107-110).

Things the spec author should know before reading on:

1. PureCutCNC has grown well past "2D drawing". It now has parametric **canned shapes**
   (ellipse, slot, regular polygon, rounded rect, chamfered rect, gear), a
   **fixed-distance constraint** system, **driving dimensions**, persistent dimension
   annotations, text on arc/path, grid/radial/path arrays, sheet nesting, a second (Bottom)
   face and a rotary "Around" face. Sections 8 and 12 list what the spec should exclude.
2. `planning/INDEX.md` routes to **no current design reference** for sketch, text, snapping
   or constraints. The only planning material on those topics is in `planning/archive/`
   (for example `AXIS_LOCK_Implementation_Plan.md`, `BACKDROP_Implementation_Plan.md`,
   `COMPOSITE_FEATURE_Implementation_Plan.md`, `CHAMFER_TOOL_Plan.md`,
   `ELLIPSE_Implementation_Plan.md`, `FEATURE_EDIT_Implementation_Plan.md`,
   `FOLDER_GROUP_Plan.md`, `CONSTRAINT_PRESERVATION_Design.md`), which the index says must
   not be used as current authority (`planning/INDEX.md` lines 22-25). I did not read the
   archive; the code is the authority for everything below.
3. All drawing is **click-click**, never press-drag. Dragging on empty canvas is marquee
   select (mouse) or pan (touch).
4. I did not read these files in full, only the parts cited: `SketchCanvas.tsx` (3,839
   lines), `usePointerGestures.ts`, `featureSlice.ts`, `selectionSlice.ts`,
   `pendingCompletionSlice.ts`, `constraintSolver.ts`, `drivingDimensionResolver.ts`,
   `dimensions.ts`, `EditSketchPanel.tsx`, `CanvasCreationPanel.tsx`,
   `TextToolDialog.tsx`, `textPlacement.ts`, `arcReconstruction.ts`.

---

## 1. Data model

### 1.1 Geometry primitives — `src/types/project.ts`

```ts
// 24-61
export interface Point { x: number; y: number }
export interface LineSegment   { type: 'line';   to: Point }
export interface ArcSegment    { type: 'arc';    to: Point; center: Point; clockwise: boolean }
export interface BezierSegment { type: 'bezier'; to: Point; control1: Point; control2: Point }
export type CircleSegment =    { type: 'circle'; center: Point; to: Point; clockwise: boolean }
export type Segment = LineSegment | ArcSegment | BezierSegment | CircleSegment
export interface SketchProfile { start: Point; segments: Segment[]; closed: boolean }
```

Facts that follow from the code, not the type:

- A profile is one path: `start`, then each segment runs from the previous end to `to`.
  There is **no bulge** representation. Arcs are centre + end point + direction flag.
- An arc's radius is implicit: `hypot(segmentStart - center)` (`project.ts:1972`). Nothing
  enforces that `to` is at the same radius.
- A full circle is a **single `circle` segment** whose `to` equals `profile.start`; the
  profile start is the radius handle (`circleProfile`, 1388-1397; `profileVertices`,
  1911-1915, returns one vertex for it; `segmentEndPoint`, 1906-1908).
- A closed profile repeats its start as the last segment's `to`; `profileVertices` drops
  that duplicate (1917-1925).
- Curves are **cubic Béziers only**. There is no B-spline, NURBS, quadratic or ellipse-arc
  segment type.
- `clockwise: true` means the **atan2 angle decreases** from start to end
  (`sampleProfilePoints`, 1974-1979). See section 10: on PureCutCNC's own screen that
  looks counter-clockwise.

Profile constructors (all in `project.ts`):

| Function | Lines | Output |
|---|---|---|
| `rectProfile(x, y, w, h)` | 1375-1386 | 4 lines, start `(x,y)`, then `(x+w,y)`, `(x+w,y+h)`, `(x,y+h)`, back |
| `circleProfile(cx, cy, r)` | 1388-1397 | start `(cx+r, cy)`, one `circle` segment, `clockwise: true` |
| `ellipseProfile(cx, cy, rx, ry)` | 1399-1420 | 4 Béziers, `KAPPA = 0.5523`, start at rightmost point, anchors right → `(cx, cy+ry)` → left → `(cx, cy-ry)` |
| `polygonProfile(points)` | 1422-1436 | lines through the points plus a closing line |
| `slotProfile(p1, p2, width)` | 1438-1459 | line, arc(centre p2), line, arc(centre p1), both arcs `clockwise: true` |
| `ngonProfile(cx, cy, n, circumradius, firstVertexAngle)` | 1461-1473 | vertices at `firstVertexAngle + i·2π/n`, then `polygonProfile` |
| `splineProfile(points)` | 1583-1616 | closed uniform Catmull-Rom as Béziers (section 2.5); fewer than 3 points falls back to `polygonProfile` |

`slotProfile` verbatim, because the test file is on the roadmap's read list:

```ts
export function slotProfile(p1: Point, p2: Point, width: number): SketchProfile {
  const r = width / 2
  const angle = Math.atan2(p2.y - p1.y, p2.x - p1.x)
  const px = -Math.sin(angle)
  const py = Math.cos(angle)
  const A: Point = { x: p1.x + r * px, y: p1.y + r * py }
  const B: Point = { x: p2.x + r * px, y: p2.y + r * py }
  const C: Point = { x: p2.x - r * px, y: p2.y - r * py }
  const D: Point = { x: p1.x - r * px, y: p1.y - r * py }
  return {
    start: A,
    segments: [
      { type: 'line', to: B },
      { type: 'arc', center: p2, to: C, clockwise: true },
      { type: 'line', to: D },
      { type: 'arc', center: p1, to: A, clockwise: true },
    ],
    closed: true,
  }
}
```

Sampling and bounds:

- `sampleProfilePoints(profile, curveSamples = 16, arcStepRadians = π/18)` (1928-1999):
  Béziers get `curveSamples` uniform-t samples; arcs and circles get
  `max(8, ceil(|sweep| / arcStep))` chords; a closed profile drops a duplicate last point
  within `1e-6`.
- Engine flattening uses different defaults: `DEFAULT_FLATTEN_CURVE_SAMPLES = 24`,
  `DEFAULT_FLATTEN_ARC_STEP = π/36` (`src/engine/toolpaths/geometry.ts:50-51`,
  `flattenProfile` 200-206). Flattening is **by fixed count / fixed angle, not by chord
  tolerance**.
- `getProfileBounds` (2001-2036) is exact for a detected ellipse and for a single circle,
  sampled otherwise.
- `profileHasSelfIntersection` (2070-2099) samples at 24 per curve and tests all chord
  pairs.

`FeatureKind` and its inference:

```ts
// 253
export type FeatureKind = 'rect' | 'circle' | 'ellipse' | 'polygon' | 'spline' | 'composite' | 'text' | 'stl'
```

`inferFeatureKind(profile)` (1491-1559) re-derives the kind from geometry after every edit:
one `circle` segment, or four concentric equal-radius arcs → `circle`; all lines, 4
segments, axis-aligned, distinct corners → `rect`; all lines otherwise → `polygon`; four
Béziers matching `ellipseProfile` within `1e-4` → `ellipse`; four Béziers otherwise →
`spline`; anything else → `composite`. So `kind` is a **derived label, not an authored
type**, except `text` and `stl`, which every edit path preserves
(for example `featureGeometrySlice.ts:429`).

### 1.2 Feature definition / instance split

```ts
// 219-220
export type FeatureOperation = 'add' | 'subtract' | 'region' | 'model' | 'line' | 'construction'
export type RegionMaskMode = 'include' | 'exclude'

// 545-562
export interface Matrix2D { a: number; b: number; c: number; d: number; e: number; f: number }
export const IDENTITY_MATRIX: Matrix2D = { a: 1, b: 0, c: 0, d: 1, e: 0, f: 0 }

// 568-583
export interface FeatureDefinition {
  id: string
  kind: FeatureKind
  profile: SketchProfile
  dimensions: LocalDimension[]
  text?: TextFeatureData | null
  canned?: CannedShapeData | null   // parametric shape; `profile` is then only the placement frame
  stl?: STLFeatureData | null
  operation: FeatureOperation
  regionMaskMode?: RegionMaskMode
}

// 588-623
export interface FeatureInstance {
  id: string
  name: string
  definitionId: string
  transform: Matrix2D
  textLayout?: TextLayout | null    // curved baseline, per instance
  constraints: LocalConstraint[]
  z_top: DimensionRef               // number | named dimension key
  z_bottom: DimensionRef
  authoringFace: SetupFace          // 'top' | 'bottom' | 'around'
  folderId: string | null
  visible: boolean
  locked: boolean
}

// 625-635
export interface FeatureFolder {
  id: string
  name: string
  collapsed: boolean
  section?: 'features' | 'regions' | 'construction'
  grouped?: boolean
}
export type FeatureTreeEntry =
  | { type: 'folder'; folderId: string }
  | { type: 'feature'; featureId: string }
```

Matrix convention (`src/geometry/profileMatrix.ts:32-42`): `x' = a·x + c·y + e`,
`y' = b·x + d·y + f`.

The runtime/editing shape that every tool actually works on:

```ts
// 199-205, 518-538
export interface Sketch {
  profile: SketchProfile
  origin: Point               // position on stock
  orientationAngle: number    // local +Y axis angle in degrees, relative to project +X
  dimensions: LocalDimension[]
  constraints: LocalConstraint[]
}
export interface SketchFeature {
  id: string; name: string; kind: FeatureKind
  text?: TextFeatureData | null
  textLayout?: TextLayout | null
  canned?: CannedShapeData | null
  stl?: STLFeatureData | null
  folderId: string | null
  sketch: Sketch
  operation: FeatureOperation
  regionMaskMode?: RegionMaskMode
  z_top: DimensionRef; z_bottom: DimensionRef
  authoringFace?: SetupFace
  visible: boolean; locked: boolean
}
```

`SketchFeature` is not serialized. `resolveFeatureRow` (`src/store/helpers/resolveFeatures.ts:206-274`)
composes definition + instance transform into a `ResolvedSketchFeature`; edits are made on
that world-space view and folded back.

How a transform reaches the geometry (`transformProfileByMatrix`,
`src/geometry/profileMatrix.ts:185-262`):

- Lines and Béziers: points transformed.
- Arcs and circles under a **similarity** (`isCirclePreservingTransform`, 68-73:
  `a²+b² ≈ c²+d²` and `a·c+b·d ≈ 0`, epsilon `1e-9`): stay arcs/circles, centre and end
  transformed, and `clockwise` is flipped when the determinant is negative (mirror).
- Arcs and circles under shear or non-uniform scale: converted to Béziers first (one per
  ≤ 90° of sweep, handle length `(4/3)·tan(step/4)·r`; circle → 4 Béziers with
  `KAPPA = 0.5522847498`).

So non-uniform resize of a circle silently turns it into four Béziers.

### 1.3 Linked copies and Make Unique

- A linked copy is another `FeatureInstance` with the same `definitionId`
  (`ARCHITECTURE.md` §4, lines 56-58).
- Copy mode is project-wide: `meta.copyMode: 'reference' | 'independent'`
  (`project.ts:1278-1279`), default `'reference'` (`newProject`, `project.ts:2151`).
  A single gesture can override it
  (`PendingMoveTool.copyMode`, `src/store/types.ts:208-209`).
- Editing the sketch of one instance writes the profile back to the shared definition
  through the **inverse of that instance's transform**, so every linked copy re-resolves
  under its own transform (`syncEditedFeatureDefinition`,
  `src/store/slices/featureGeometrySlice.ts:175-230`).
- `makeUnique(project, instanceId)` (`src/store/helpers/featureDefinitions.ts:339-367`):
  deep-clones the definition under a new id `def-clone-N` (289-298) and repoints that one
  instance. The store action is one undo step (`featureGeometrySlice.ts:1279-1306`).
- `gcOrphanedDefinitions` (255-281) removes definitions no instance (or stock source)
  references; it runs after join/cut when originals are not kept.
- UI entry: `featureTree.contextMenu.makeUnique` and "select linked instances" in
  `src/components/feature-tree/FeatureContextMenu.tsx:401-408`.

### 1.4 Folders, groups, tree sections

- Folders belong to one of three tree sections: `features`, `regions`, `construction`.
- `grouped: true` makes a folder a **transform group**: selecting a member selects the
  whole folder (`selectFeature(id, additive, expandGroup = true)`, `types.ts:574`;
  `SelectionState.groupFolderId`, `types.ts:73-74`); copying every member of a grouped
  folder creates a new grouped folder named `"<name> Copy"`
  (`src/store/slices/pendingCompletionSlice.ts:269-309`).
- Store actions: `addFeatureFolder`, `updateFeatureFolder`, `deleteFeatureFolder`,
  `assignFeaturesToFolder`, `moveFeatureTreeFeature`, `reorderFeatureTreeEntries`,
  `groupSelectedFeaturesIntoNewFolder`, `toggleFolderGrouped`, `selectFolderFeatures`,
  folder visibility toggles (`types.ts:462-477`).
- There is **no nested folder**: `FeatureFolder` has no parent field.
- There is **no separate "group" object**; grouping is a folder flag.

### 1.5 Construction geometry

- It is a role, not a flag: `operation: 'construction'` (`ARCHITECTURE.md` line 34).
  It lives in its own tree section, renders muted/dashed, takes part in snapping,
  mirroring, cutting and dimensions, and is excluded from CSG, simulation, toolpaths and
  CAM targets through `isMachinable` / `modelFeatures()`
  (`src/store/helpers/featureRoles.ts`, described in `src/store/INDEX.md` line 64; the
  file itself was not read).
- It is created by choosing the **Construction creation target** before drawing
  (`CreationTarget = 'feature' | 'line' | 'region' | 'construction'`, `types.ts:176`).
- A feature's role can be changed afterwards in Properties
  (`DRAWING_MODES_TUTORIAL_VOICEOVER.md` line 21; conversion tests in
  `src/store/constructionWorkflows.test.ts`).
- There are **no infinite guide lines, no ruler-dragged guides, no construction points**
  as a distinct entity: construction geometry is ordinary profiles with a role.

### 1.6 Text data

```ts
// 221-252, 336-341
export type TextFontStyle = 'skeleton' | 'outline'
export type TextFontId = 'simple_stroke' | 'simple_stroke_italic' | ... | 'droid_serif_bold_italic'  // 30 ids
export interface TextFeatureData { text: string; style: TextFontStyle; fontId: TextFontId; size: number }
```

`TextLayout` (294-334) is a tagged union, stored on the **instance**:

```ts
| { kind: 'arc'; center: Point; radius: number; angleDegrees: number; sweepDegrees: number;
    anchor: 'start' | 'center' | 'end'; fit: 'natural' | 'fill';
    direction: 'cw' | 'ccw'; orientation: 'fixed' | 'follow' }
| { kind: 'path'; path: SketchProfile; startOffset: number; endOffset: number;
    anchor: 'start' | 'center' | 'end'; fit: 'natural' | 'fill';
    reversed: boolean; orientation: 'fixed' | 'follow'; sourceFeatureId?: string }
```

A text definition's `profile` is the run's **frame rectangle**; glyph geometry is never
stored (section 3).

### 1.7 Canned (parametric) shapes

```ts
// 381-410
export type CannedShapeParamValue = number | string | boolean | null
  | CannedShapeParamValue[] | { [key: string]: CannedShapeParamValue }
export type CannedShapeParams = Record<string, CannedShapeParamValue>
export interface CannedShapeData { kind: string; params: CannedShapeParams }
```

The generator contract is `CannedShapeGenerator` in `src/cannedShapes/types.ts:96-122`:
`kind`, `labelKey`, `generate(params) → CannedShapeOutline[]` in definition-local
coordinates, optional `normalizeParams`, `convertParams` (unit switch), `summary`.
Registry keys: `'ellipse'`, `'slot'`, `'ngon'`, `'roundrect'`, `'chamferrect'`
(`src/cannedShapes/generators/kinds.ts:24-28`), `'gear'`
(`generators/gear.ts:45`), plus rotary kinds out of scope.

Rules (`src/cannedShapes/INDEX.md`, "Rules"; `src/cannedShapes/index.ts:194-216`):

- The stored `definition.profile` is a placement frame derived from the parameters;
  rendering and hit testing read the generated outlines.
- Closed outlines nest by containment: depth 0 takes the feature's operation, depth 1 the
  opposite, depth 2 the operation again. Open outlines keep the operation.
- Placement is the instance transform; under a mirror the generated arcs' `clockwise`
  flags flip (`index.ts:166-185`).
- **Expand to features** turns a canned shape into plain rows in one grouped folder
  (`expandCannedFeature`, `src/store/helpers/cannedExpansion.ts`, not read).
- A canned shape refuses sketch edit, trim/extend, join, cut and offset until expanded
  (`selectionSlice.ts:986-988`; `featureSlice.ts:858-862, 957-961, 1041-1045`;
  `featureGeometrySlice.ts:811-816`).

### 1.8 Backdrop, grid, annotations

```ts
// 1143-1155
export interface BackdropImage {
  name: string; mimeType: string; imageDataUrl: string
  intrinsicWidth: number; intrinsicHeight: number
  center: Point; width: number; height: number
  orientationAngle: number; opacity: number; visible: boolean
}
// 690-697
export interface GridSettings {
  extent: number; majorSpacing: number; minorSpacing: number
  snapEnabled: boolean; snapIncrement: number; visible: boolean
}
```

`Project` holds exactly **one** backdrop: `backdrop: BackdropImage | null`
(`project.ts:1333`), with the image embedded as a data URL.

`defaultGrid` (1782-1802): mm → extent 200, major 10, minor 2, snap increment 1;
inch → extent 8, major 1, minor 0.25, snap increment 0.125; `snapEnabled: true`,
`visible: true`. Note that the snap code reads `grid.snapIncrement` but never
`grid.snapEnabled` (`src/components/canvas/snappingHelpers.ts:437-443`); the live toggle
is the app-local `SnapSettings` (section 5).

Dimension annotations (`project.ts:88-139`): `DimensionAnnotation { id, type, a, b?, c?,
offset, labelOffset?, textOverride?, precisionOverride?, visible, locked }` with
`DimensionType = 'aligned' | 'horizontal' | 'vertical' | 'radius' | 'diameter' | 'angle'`
and `DimensionAnchor` kinds `free | vertex | midpoint | center | circleEdge |
segmentPoint | origin`. Stored in `project.annotations`.

---

## 2. Drawing tools

### 2.1 What exists

The single list is `CREATION_SHAPE_OPTIONS` (`src/commands/creationShapes.ts:23-36`):

```
rect, circle, ellipse, polygon, spline, composite, text, slot, ngon, gear, roundrect, chamferrect
```

`shapeEnabledForTarget` (67-74): **text and gear are available only for the `feature`
creation target**; every other shape is available for all four targets.

Mapping to the brief's list:

| Brief asks for | PureCutCNC |
|---|---|
| line | **No dedicated tool.** Use `polygon` and finish open (Enter / double-click), or `composite` in line mode |
| polyline | `polygon` finished open (names it "Polyline N") |
| rectangle | `rect` |
| rounded rect | `roundrect` (canned) |
| chamfered rect | `chamferrect` (canned) |
| circle | `circle` (centre + radius only) |
| ellipse | `ellipse` (canned) |
| polygon | `polygon` (closed by clicking the first point) |
| regular polygon | `ngon` (canned) |
| slot | `slot` (canned) |
| gear | `gear` (canned) |
| arc | **No dedicated tool.** Arc is a segment mode inside `composite` (3 clicks) |
| spline / Bézier | `spline` (click-through-points, auto tangents), and spline mode inside `composite` |
| text | `text` |

### 2.2 Shared mechanics

State lives in `pendingAdd: PendingAddTool | null` (`src/store/types.ts:122-167`):

```ts
| { shape: 'rect' | 'circle' | 'ellipse'; anchor: Point | null; session: number }
| { shape: 'text'; config: TextToolConfig; session: number }
| { shape: 'polygon' | 'spline'; points: Point[]; session: number }
| { shape: 'composite'; start: Point | null; lastPoint: Point | null; segments: Segment[];
    currentMode: CompositeSegmentMode; pendingArcEnd: Point | null; closed: boolean; session: number }
| { shape: 'slot'; points: Point[]; session: number }
| { shape: 'ngon'; anchor: Point | null; sides: number; session: number }
| { shape: 'gear'; anchor: Point | null; outsideRadius: number | null; params: GearCreationParams; session: number }
| { shape: 'roundrect' | 'chamferrect'; anchor: Point | null; corner: number; session: number }
```

(also `origin`, `tab`, `clamp`, `canned-edit`, `rotary-solid`, `rotary-pattern`).

- Arming a tool: `startAdd…Placement` (`src/store/slices/pendingAddSlice.ts:176-343`)
  clears `pendingMove`, `pendingTransform`, `pendingOffset`, the sketch-edit session, and
  sets selection mode to `feature`. Clicking the active tool's button again cancels
  (`creationShapes.ts:107-114`).
- Click dispatch: `handleClick` in `src/components/canvas/useClickPlacement.ts:821-906`.
- Live preview: `handleCanvasPointerMove` in
  `src/components/canvas/usePointerGestures.ts:712-754`.
- **Sticky**: after each creation the same tool re-arms with a fresh draft, keeping the
  n-gon side count, the corner value, the composite mode, the gear parameters and the
  text config (`rearmPendingAdd`, `pendingAddSlice.ts:132-167`). Escape leaves the tool.
- **Snap gate**: every click is taken at `pickedPoint`, which is `null` — and the click
  is ignored — when snapping is enabled, at least one mode is on, and nothing snapped
  (`useClickPlacement.ts:400` and `822-824`). See section 5.4.
- **Axis lock**: tapping Alt cycles none → X → Y (`src/sketch/useAxisLock.ts:42-59,
  100-104`). `applyLock(point, origin)`: X lock keeps `origin.y`, Y lock keeps `origin.x`
  (77-82). It is applied to polygon, spline, composite and slot points, to move targets
  and to node drags — **not** to the second click of rect, circle, ellipse, n-gon, gear
  (`useClickPlacement.ts:842, 855, 901, 919`).
- Default depth of a new feature: `Math.min(stock.thickness, 10)`, the literal `10` being
  in project units whatever they are (`pendingAddSlice.ts:426, 636, 659, 906, 937, 1030,
  1068, 1080`).
- Minimum size: `convertLength(0.01, 'mm', units)` (427).
- Role and name: `buildShapeFeature` (`src/store/helpers/buildShapeFeature.ts:32-77`).
  Target `region` → `'region'`; `construction` → `'construction'`; `line` → `'line'`;
  `feature` + closed → `inferManualFeatureOperation` (Add when outside every solid,
  alternating by nesting; `src/store/helpers/manualFeatureOperation.ts:83-107`);
  `feature` + open → `'line'`. Regions and construction features are named
  `Region N` / `Construction N`; others `<Shape> <features.length + 1>`.
  New sketches get `origin {0,0}` and `orientationAngle: 90`.
- A new Line inherits `z_top` from the smallest enclosing solid
  (`inferLineTopZFromEnclosingFeature`, same file, 120 onward).

Keys while a tool is armed (`src/components/canvas/useCanvasKeyboard.ts`; names in
`src/components/canvas/canvasShortcuts.ts:26-36`):

| Key | Effect | Lines |
|---|---|---|
| Tab | open / advance / close typed-dimension entry | 283-520 |
| Enter | polygon/spline with ≥ 2 points: finish open; composite with ≥ 1 segment and no pending arc end: finish open | 662-673, 696-703 |
| Backspace | remove the last point / composite step (not on key repeat) | 652-660, 688-695 |
| L / A / S | composite segment mode line / arc / spline | 675-687 |
| Escape | cancel the tool | 723-729 |
| Alt (tap) | cycle axis lock | `useAxisLock.ts` |

Finishing an open path is refused when the target is `region`
(`useCanvasKeyboard.ts:667-669`, `pendingAddSlice.ts:656, 929`).
Double-click also finishes a polygon/spline open path
(`usePointerGestures.ts:1234-1243`).

### 2.3 Typed-dimension entry ("manual entry")

Files: `src/components/canvas/manualEntry.ts` (state type and preview-point maths),
`src/components/canvas/triggerDimensionEdit.ts` (opens the entry from the panel's
"Dimensions" button), `useCanvasKeyboard.ts:283-520` (Tab does the same thing),
`src/components/canvas/useCreationWorkflow.ts:168-261` (commit),
`src/components/canvas/CanvasCreationPanel.tsx` (inputs; Enter in any input calls
`commitCreationDimensionEdit`, lines 241-375).

```ts
// manualEntry.ts 23-37
export interface DimensionEditState {
  shape: 'rect' | 'circle' | 'ellipse' | 'tab' | 'clamp' | 'polygon' | 'spline' | 'composite' | 'slot' | 'ngon' | 'gear'
  anchor: Point
  arcStart?: Point
  arcEnd?: Point
  arcClockwise?: boolean
  signX: number
  signY: number
  activeField: 'width' | 'height' | 'radius' | 'length' | 'angle'
  width: string; height: string; radius: string; length: string; angle: string
}
```

Behaviour:

- Entry is available only **after the first click** (an anchor, a first point, or a
  composite start) — `creationCanDimEdit`, `useCreationWorkflow.ts:127-129`. The first
  point itself cannot be typed; there is **no absolute X/Y coordinate entry**.
- Fields are pre-filled from the current cursor preview, so Tab then Enter commits what
  the cursor showed.
- While a field is open, the preview point is recomputed from the fields by
  `computeDimensionEditPreviewPoint` (`manualEntry.ts:112-218`).
- Parsing: `parseLengthInput` strips commas and calls `Number()` — **no expressions, no
  fractions, no unit suffixes** (`src/utils/units.ts:74-83`). Formatting: 3 decimals in
  mm, 4 in inch, trailing zeros stripped (85-93). Angles: 2 decimals
  (`formatAngleField`, `manualEntry.ts:46-49`).
- Typed angles are headings `atan2(dy, dx)` in degrees, passed through
  `faceAngles(project).direction`. On an unturned (Top) stock `faceAngles` returns the
  constant `STOCK_ANGLES` (`src/store/helpers/activeFace.ts:632-634`); that constant was
  not read, and I take it to be the identity map **[inferred]**. On Y-down, +90° points
  down the screen.

Per shape (Tab order, then what Enter commits):

| Shape, phase | Fields | Preview point from fields |
|---|---|---|
| rect / roundrect / chamferrect, anchor set | width → height | `anchor + (signX·w, signY·h)`; signs come from which side of the anchor the cursor was on |
| ellipse, centre set | width → height (these are **rx, ry**) | same formula |
| circle, centre set | radius | `(anchor.x + r, anchor.y)` |
| polygon / spline, ≥ 1 point | length → angle | `last + len·(cos θ, sin θ)`; Enter appends the point and stays in the tool |
| composite, line or spline mode | length → angle | same; Enter appends a segment |
| composite, arc mode, end not yet picked | length → angle (+ a radius, default = current length, or 0.5 when the length is 0) | Enter adds the end point, then a through-point at the sagitta on the `(-dy, dx)` side, producing a minor arc in one step (`useCreationWorkflow.ts:179-205`) |
| composite, arc mode, end picked | radius | through-point on the chord's perpendicular bisector, on the side the cursor was on (`manualEntry.ts:117-148`; opened by `useCanvasKeyboard.ts:366-406`) |
| slot, first centre set | length → angle → width (the width lives in the `radius` field) | Enter derives p2 and commits the whole slot (`useCreationWorkflow.ts:219-240`) |
| slot, both centres set | width | point at `midpoint + (w/2)·perp` |
| ngon, centre set | radius (angle carried from the cursor direction) | `anchor + r·(cos θ, sin θ)` |
| gear, centre set | radius | same; Enter moves to the parameter phase |

Whether the n-gon and gear panels expose the angle as an editable input was not
verified; Tab only cycles `radius` for them (`useCanvasKeyboard.ts:490-519`).

Defaults used when there is no cursor preview: slot length 20 mm / 1 in, slot width
6 mm / 0.25 in (`triggerDimensionEdit.ts:160-162, 174`).

### 2.4 Rectangle

- Clicks: first corner → opposite corner (`useClickPlacement.ts:847-852`).
- Result (`pendingAddSlice.ts:431-438, 533`; `featureSlice.ts:1286-1288`):
  `x = min(x1,x2)`, `y = min(y1,y2)`, `w = max(|dx|, minSize)`, `h = max(|dy|, minSize)`,
  `rectProfile(x, y, w, h)`, kind `'rect'`.
- **Baked**: a plain 4-line profile with an identity transform. Width and height are not
  stored parameters. Afterwards it is edited by node editing, the Resize transform, or a
  driving dimension.
- No centre-out rectangle, no 3-point (rotated) rectangle, no square constraint key.
- Tests: `src/store/featureLifecycle.test.ts`, `src/store/creationDefinitions.test.ts`,
  `src/store/stickyDrawing.test.ts`, `src/store/geometryFidelity.test.ts`.

### 2.5 Circle

- Clicks: centre → any point on the circumference.
- Result (`pendingAddSlice.ts:540-541`; `featureSlice.ts:1290-1292`):
  `r = max(minSize, hypot(dx, dy))`, `circleProfile(cx, cy, r)`.
- **Baked** as one `circle` segment, kind `'circle'`. The radius is editable afterwards
  as a node (the start point is the radius handle; the centre is a separate
  `circle_center` control), or by the radius field in sketch edit.
- No 2-point (diameter) or 3-point circle.

### 2.6 Ellipse (canned)

- Clicks: centre → a point whose **X offset is rx and Y offset is ry**
  (`pendingAddSlice.ts:535-538`): `rx = max(minSize, |dx|)`, `ry = max(minSize, |dy|)`.
  It is always axis-aligned at creation.
- Result (`featureSlice.ts:1294-1296`): canned `{ kind: 'ellipse', params: { rx, ry } }`,
  instance transform `translate(cx, cy)`; generator
  `ellipseProfile(0, 0, rx, ry)` (`src/cannedShapes/generators/ellipse.ts:52`).
- Geometry: four cubic Béziers with `KAPPA = 0.5523` — an approximation, not an exact
  ellipse.
- **Parametric**: "Edit ellipse…" opens a side panel with Radius X / Radius Y
  (`ELLIPSE_EDITOR_REQUEST`, ellipse.ts:66-74); while it is open the canvas draws two
  radius handles that can be dragged (`src/cannedShapes/ellipseHandles.ts`,
  `src/components/canvas/useCannedRadiusDrag.ts`, hit radius 9 px). Rotation comes from
  the Rotate transform, not a parameter.
- Tests: `src/cannedShapes/generators/shapes.test.ts`,
  `src/cannedShapes/ellipseHandles.test.ts`, `src/components/canvas/cannedRadiusDrag.test.ts`,
  `src/store/cannedShapeCreation.test.ts`, `src/store/cannedShapeEditPanel.test.ts`.

### 2.7 Polygon and polyline

- Clicks append points (`useClickPlacement.ts:835-846`); a click identical to the last
  point is ignored.
- **Close**: with ≥ 3 points, clicking within `POLYGON_CLOSE_RADIUS = 12` px of the first
  point (`useClickPlacement.ts:82, 837`; `isLoopCloseCandidate`,
  `src/components/canvas/draftGeometry.ts:105-114`) → `completePendingPolygon` →
  `polygonProfile(points)` (`pendingAddSlice.ts:631-651`).
- **Open**: Enter or double-click with ≥ 2 points → `completePendingOpenPath`
  (653-741): a `closed: false` profile of lines, kind `'polygon'`, operation `'line'`
  (or `'construction'`), named `Polyline N`.
- **Baked** lines. Kind is re-inferred, so a 4-point axis-aligned polygon becomes `rect`.
- Backspace removes the last point; Tab gives length → angle for the next point.

### 2.8 Spline

- Same click handling as polygon (shared `points` array).
- **Closed** (click near the first point, ≥ 3 points): `splineProfile(points)`
  (`project.ts:1583-1616`). For each `i`, with `p0 = P[i-1]`, `p1 = P[i]`, `p2 = P[i+1]`,
  `p3 = P[i+2]` (indices wrap):
  `control1 = p1 + (p2 − p0)/6`, `control2 = p2 − (p3 − p1)/6`, `to = p2`.
  That is the uniform Catmull-Rom → Bézier conversion.
- **Open** (Enter / double-click): built incrementally with `appendSplineDraftSegment`
  (`src/store/helpers/profileEdit.ts:694-726`, duplicated in
  `draftGeometry.ts:54-86`). For a new end `to`, with `current` the last anchor and
  `previous` the one before it (or `current` if none): `tangent = (to − previous)/6`;
  new segment `control1 = current + tangent`, `control2 = to − (to − current)/6`;
  and the previous Bézier's `control2` is rewritten to `current − tangent`.
  **The open and closed constructions are different formulas**; an open spline's first
  and last segments have short end tangents.
- **Baked** Béziers, kind `'spline'` (or re-inferred). Edited afterwards through anchor
  and handle nodes.
- There is **no click-drag "pen tool"** for placing handles while drawing, and no
  fit-point/control-point spline entity.

### 2.9 Composite (the only way to draw an arc)

State: `start`, `lastPoint`, `segments`, `currentMode: 'line' | 'arc' | 'spline'`,
`pendingArcEnd`, `closed`. Reducer: `addPendingCompositePoint`
(`pendingAddSlice.ts:755-828`).

- First click sets `start` and `lastPoint`.
- **Line mode**: each click appends `{ type: 'line', to }`.
- **Spline mode**: each click appends through `appendSplineDraftSegment`.
- **Arc mode** takes two clicks per arc: the first is the arc's **end point**
  (`pendingArcEnd`), the second a **point the arc passes through**. The arc is
  `buildArcSegmentFromThreePoints(lastPoint, pendingArcEnd, through)`
  (`profileEdit.ts:760-788`): circumcentre of the three points, `clockwise = cross < 0`
  with `cross = (through − start) × (end − start)`; collinear points
  (`|denominator| < 1e-9`) return `null` and the click is ignored.
- Modes can be switched between segments (L / A / S, or the panel). Switching away from
  arc mode drops a pending arc end (`setPendingCompositeMode`, 743-753).
- **Close**: clicking within 12 px of the start, when not in arc mode, with no pending arc
  end and ≥ 3 draft points (`useClickPlacement.ts:887-898`) → `completePendingComposite`
  (894-922), which closes with a line, or with a spline segment when the current mode is
  spline (`resolveCompositeDraftSegments`, `profileEdit.ts:728-746`; it needs ≥ 2
  segments). An arc whose end lands exactly on the start also closes the draft
  (`pendingAddSlice.ts:809`).
- **Open**: Enter with ≥ 1 segment → `completePendingOpenComposite` (924-973), kind
  `'composite'`, operation `'line'` or `'construction'`.
- Backspace: clears a pending arc end first, then removes the last segment, then the
  start (`undoPendingCompositeStep`, 830-871).
- **Baked**: lines, true arcs and Béziers in one profile.
- There is no tangent-arc mode, no centre-start-end arc, no start-end-radius click mode
  (radius can only be typed).
- `closePendingCompositeDraft` exists in the store (873-892) but nothing outside the
  slice calls it.

### 2.10 Slot (canned)

- Clicks: centre of the first end → centre of the second end → a third click whose
  perpendicular distance from the axis is the half-width
  (`useClickPlacement.ts:853-862`; `placePendingSlotAt`, `pendingAddSlice.ts:1052-1071`):
  `width = 2·|((p3 − p1) × axis)| / |axis|`; refused when the axis or the width is below
  `1e-10`.
- Result (`featureSlice.ts:1306-1313`): canned `{ kind: 'slot', params: { length, width } }`
  with `length = |p2 − p1|` (centre to centre), instance transform
  `translate(midpoint) · rotate(atan2(dy, dx))`; generator
  `slotProfile((−length/2, 0), (length/2, 0), width)`
  (`src/cannedShapes/generators/slot.ts:46-52`).
- Geometry: two lines and two **true semicircular arcs**.
- **Parametric**: "Edit slot…" with Length and Width (slot.ts:66-74).
- Tests: `src/types/slot.test.ts` (7 functions: segment count and types, arc centres,
  connectivity, arc radii, closure, horizontal slot start point, 45° rotation), plus the
  canned tests above.

### 2.11 Regular polygon (canned)

- The side count is set in the creation panel, default **6**, clamped to 3-50, rounded
  (`pendingAddSlice.ts:301, 975-981`; `NGON_MIN_SIDES`/`NGON_MAX_SIDES`,
  `src/cannedShapes/generators/ngon.ts:33-34`).
- Clicks: centre → a point that becomes the **first vertex**
  (`placePendingNgonAt`, 1073-1090): `circumradius = hypot`, `firstVertexAngle = atan2`.
- Result (`featureSlice.ts:1315-1318`): canned
  `{ kind: 'ngon', params: { sides, size: circumradius, rotation: degrees } }`, transform
  `translate(cx, cy)`.
- Named Triangle, Square, Pentagon, … Dodecagon for 3-12 sides, else `Polygon<N>`.
- **Parametric**: Sides, Size (circumradius), Rotation ° (ngon.ts:69-78).
- Sizing is by **circumradius only**; there is no inscribed (across-flats) option and no
  star.
- Tests: `src/types/ngon.test.ts` (7 functions).

### 2.12 Rounded and chamfered rectangle (canned)

- Corner value default **5 mm / 0.2 in** (`pendingAddSlice.ts:327, 337`), editable in
  the creation panel before the second click (`setPendingRectCorner`, clamped `≥ 0`).
- Clicks: as rectangle.
- Result (`featureSlice.ts:1326-1332`): canned
  `{ kind: 'roundrect', params: { width, height, radius } }` or
  `{ kind: 'chamferrect', params: { width, height, chamfer } }`, transform
  `translate(x, y)` of the min corner.
- Geometry (`src/store/helpers/cannedRectProfiles.ts`):
  - `roundedRectProfile` (25-61): start from `rectProfile`, clamp the radius to
    `min(w,h)/2 − 1e-6`, then apply `applyLineCornerFillet` up to four times, each time to
    the first remaining line-line corner. Result: 4 lines + 4 true arcs. Radius `≤ 0`
    returns the plain rectangle.
  - `chamferedRectProfile` (72-98): 8 lines built directly, chamfer clamped the same way,
    start `(x, y+d)`.
- One value for all four corners; no per-corner values.
- **Parametric**: Width, Height, Corner radius / Chamfer.
- Tests: `src/store/helpers/cannedRectProfiles.test.ts` (15 tests, 34 assertions).

### 2.13 Gear (canned)

- Clicks: centre → outside radius (click or typed) → parameter panel → Confirm
  (`useClickPlacement.ts:871-878`; `setPendingGearRadiusAt` and `completePendingGear`,
  `pendingAddSlice.ts:1003-1042`). The first radius pick replaces the parameters with
  `defaultGearCreationParams(outsideRadius)`.
- Parameters (`src/sketch/gearProfile.ts:28-38`): `teeth`, `wholeDepth`,
  `flankProfile: 'involute' | 'straight'`, `pressureAngleDeg`,
  `rootForm: 'rounded' | 'flat' | 'sharp'`, `rootFilletRadius`,
  `crestForm: 'flat' | 'rounded'`, `crestRadius`, `boreDiameter`; plus `outsideRadius`.
- Limits and defaults (19-22, 62-76, 109-126): teeth 6-200 (default 24), pressure angle
  10-35° (default 20), `module = 2·outsideRadius/(teeth + 2)`,
  `wholeDepth = 2.25·module`, `rootFilletRadius = 0.15·module`,
  `crestRadius = 0.2·module`, bore 0, involute flanks, rounded root, flat crest.
- Geometry: **a polygon of line segments only** (`profileFromPoints`, 351-363).
  Involute flanks are sampled at 5 points per flank, root and crest transitions at 5
  samples each (46-48). The bore, when > 0, is a second outline (a true `circle`) that
  the nesting rule turns into the opposite operation
  (`src/cannedShapes/generators/gear.ts:94-108`).
- Validation messages: `validateGearProfileParams` (128-171).
- Tests: `src/sketch/gearProfile.test.ts` (9), `src/cannedShapes/generators/gear.test.ts`
  (8), `src/store/gearCreation.test.ts` (10),
  `src/components/canvas/gearEditorWiring.test.ts`.

### 2.14 Text

See section 3. Interaction: the text button opens a dialog
(`src/components/project/TextToolDialog.tsx`), its config arms
`{ shape: 'text', config }`, and **one click places the run**
(`useClickPlacement.ts:879-885`; `placePendingTextAt`, `pendingAddSlice.ts:547-608`).

### 2.15 Creation panel and prompts

`src/components/canvas/CanvasCreationPanel.tsx` shows a floating panel with the step
prompt and buttons. Prompt strings (`src/i18n/locales/en/canvas.ts:104-125`) include
"Click first corner", "Click center point", "Click opposite corner or enter dimensions",
"Click to set radius or enter dimensions", "Click first end center",
"Click second end center or enter dimensions",
"Move cursor to set width, click to commit", "Click arc curvature point",
"Add points or close". Phase keys: `useCreationWorkflow.ts:134-140`.

Preview drawing: `buildPendingProfile` and `drawCompositeDraft`
(`src/components/canvas/draftHelpers.ts:202-323`): dashed `[8, 5]` stroke, 2 px, with a
live length label on each line/Bézier segment and a radius label on each arc.

---

## 3. Text

### 3.1 Fonts

All in `src/text/index.ts`.

- **Outline fonts** are the eleven typeface-JSON files that ship inside the `three`
  package's examples (imports at 20-30): Helvetiker regular/bold, Optimer regular/bold,
  Gentilis regular/bold, Droid Sans regular/bold, Droid Sans Mono regular, Droid Serif
  regular/bold. They are parsed with three's `FontLoader`
  (`src/text/fontData.ts:17-30`).
- **Stroke font**: one hand-written single-line font, "Simple Stroke", defined inline as
  the `GLYPHS` table (167-210). It covers `A-Z`, `0-9`, `. - _ / ?` and space. Lower case
  is upper-cased, anything else renders as `?` (`glyphFor`, 428-431). Each glyph is a
  list of polylines on a unit box with **y pointing down** (feet at `y = 1`) and an
  `advance`.
- **Variants are synthetic**: italic is a shear (`ITALIC_SLANT = 0.22`), condensed is an
  X scale (`CONDENSED_WIDTH_SCALE = 0.82`) (115-116), applied by
  `applyFontVariantToProfile` (314-324): `x' = x·widthScale − y·slant`. That gives the 30
  `TextFontId` values (118-149).
- **There is no user font loading**: no TTF/OTF/system-font path exists anywhere in
  `src/text/`. (`@pdf-lib/fontkit` in `package.json` is for PDF output.)
- **Licences [inferred, not verifiable in this checkout]**: `node_modules` is absent, so
  I could not read three's `examples/fonts/LICENSE`. From memory: Helvetiker, Optimer
  and Gentilis are typeface.js conversions of the MgOpen fonts (MgOpen licence, a
  Bitstream-Vera-style permissive licence), and the Droid fonts are Apache-2.0. Verify
  against the three.js repository before bundling anything. PureCutCNC's `NOTICE` does
  **not** attribute any font (it lists only occt-import-js / Open CASCADE).
- The stroke font is original PureCutCNC data under Apache-2.0 (it is in a file with the
  project header); it is **not** a Hershey font.

### 3.2 Glyphs to profiles

Constants: `LETTER_SPACING_RATIO = 0.18`, `LINE_HEIGHT_RATIO = 1.45`, default text
`'TEXT'` (97-100).

Stroke style (`layoutGlyphs`, 453-475): each polyline point is
`(cursorX + x·size, baselineY + y·size)`; the cursor advances by
`size·(advance + 0.18)`. Each polyline becomes an **open line profile**
(`lineProfile`, 248-257).

Outline style (`outlineProfilesFromFont`, 326-418):

1. For each character, `font.generateShapes(char, size)` then
   `shape.extractPoints(20)` — three's curve sampler with 20 divisions per curve. Curves
   are **flattened to polylines here; no arcs or Béziers survive**.
2. Outer contours get depth 0, holes depth 1.
3. The cursor advances by `glyph.ha · (size / resolution) + size·0.18`
   (`resolution` default 1000). **No kerning.**
4. The whole run is flipped in Y (`flipProfileY`, the font data being y-up) and
   translated so its bounding box starts at `(0, 0)`.
5. The font variant shear/scale is applied.
6. Each contour goes through `cleanOutlineContour`
   (`src/text/outlineContours.ts:82-110`): Clipper `SimplifyPolygon` (non-zero) to
   resolve self-intersections, drop components with area
   `≤ size²·0.001`, restore the source winding sign.
7. Operation per contour: even depth → the run's operation, odd depth → the inverse
   (516), so a letter's counter is an island inside the cut.

Text is **single-line**: `normalizeText` (420-426) turns every newline into a space, so
the multi-line code in `layoutGlyphs` never sees more than one line.

Templates are cached by style, font, operation, size, layout and text (584-600).

### 3.3 What is stored and how it resolves

`createTextFeatureAt` (`src/store/helpers/naming.ts:73-109`): the feature is
`kind: 'text'` with `text: { text, style, fontId, size }`, and its profile is the
**frame**: `getTextFrameProfile(config, anchor)` = a rectangle at the anchor sized to the
template bounds (`index.ts:677-680`). The anchor is the frame's min corner (top-left on
PureCutCNC's screen). `z_top = stock.thickness`, `z_bottom = 0`.

`resolveTextFeatureShapes` (752-795) regenerates the template and maps it onto the
frame: the frame's vertex 0 is the origin, vertex 1 − vertex 0 the X axis, vertex 3 −
vertex 0 the Y axis; a template point maps by its normalized `(u, v)` in the template
bounds. Consequences:

- Moving, rotating, mirroring or **non-uniformly resizing the frame transforms the
  letters** — a stretched frame gives stretched text.
- Editing the string or size re-fits the frame width so the previous stretch ratio is
  preserved (`refitTextFrameProfile`, 705-738).

### 3.4 Parameters

- Dialog config (`TextToolConfig`, 46-54): `text`, `style`, `fontId`, `size`,
  `operation`, `layout`.
- Defaults (`defaultTextToolConfig`, 217-226): style `skeleton`, font `simple_stroke`,
  size 10 mm / 0.4 in, operation `subtract`. Default outline font `helvetiker_bold`
  (241-246).
- `size` is the glyph box height for the stroke font and the font em size passed to
  three for outline fonts.
- **Not present**: letter-spacing control (fixed 0.18 × size), line spacing, alignment
  (left/centre/right), multi-line, kerning, bold/italic as real font faces, text box
  wrapping.

### 3.5 Text on an arc or a path

A post-create edit on an existing run (`src/store/slices/textLayoutSlice.ts`,
`src/sketch/textPlacement.ts`, `src/components/canvas/TextLayoutPanel.tsx`; only
`src/text/baseline.ts` was read).

- `arcBaseline(center, radius, angleDegrees, sweepDegrees, direction)`: point at
  `angle = anchorAngle ± distance/radius`. Angles are **0 = 3 o'clock, positive clockwise
  on the Y-down screen; 12 o'clock is 270**.
- `pathBaseline(profile, startOffset, endOffset, anchor, reversed)` uses
  `measureProfilePath` (arc-length parameterisation; Béziers via a 128-step table).
- `bendShapesToBaseline`: each **glyph** (grouped by `glyphIndex`) moves under one
  similarity transform — scale, rotation, translation — positioned by its centre's
  fraction along the run. Letterforms are never sheared.
  `fit: 'fill'` scales the whole run uniformly by `span / naturalWidth`;
  `fit: 'natural'` keeps the size and reports overflow.
  `orientation: 'follow'` rotates each glyph to the tangent; `'fixed'` keeps it upright.
- Defaults in `textPlacement.ts`: `MIN_ARC_RADIUS = 1e-3`, default sweep 120°, top of
  circle 270°, bottom 90° (lines 34, 102-108).
- A path layout bakes a snapshot of the guide into the instance; `sourceFeatureId` only
  enables a manual Refresh.

### 3.6 Expand

`expandTextFeature` (`src/store/helpers/textExpansion.ts:55 onward`): one grouped folder
per glyph, one plain feature per contour or stroke, each with a fresh definition.
Stroke fonts expand to open Line features.

### 3.7 Tests

`src/text/baseline.test.ts` (15 functions), `src/text/outlineContours.test.ts` (5),
`src/text/fontData.test.ts` (1), `src/store/textExpansion.test.ts` (14),
`src/store/textFrameRefit.test.ts`, `src/store/textLayout.test.ts` (14),
`src/store/textLayoutPersistence.test.ts`, `src/store/textPathRefresh.test.ts`,
`src/store/textReference.test.ts`, `src/components/project/textToolDirty.test.ts`.

---

## 4. Node (sketch) editing

### 4.1 Edit session

- Enter: double-click a feature (`usePointerGestures.ts:1224-1280`) or "Edit sketch" in
  the context menu → `enterSketchEdit(id)` (`src/store/slices/selectionSlice.ts:982-1011`).
  It sets `selection.mode = 'sketch_edit'` and records

  ```ts
  // types.ts 301-306
  export interface SketchEditSession {
    entityType: 'feature' | 'clamp' | 'tab'
    entityId: string
    snapshot: Project
    pastLength: number
  }
  ```

  Refused for ghost (other-face) features and for canned shapes. Exactly one feature is
  edited at a time (`editableFeature`, `SketchCanvas.tsx:2256-2266`).
- **Apply** (Enter with no tool active, or the panel): `applySketchEdit` (1061-1115)
  simply leaves the mode. Each edit made during the session is already its own undo
  step, and stays that way. (Only the stock-source special case collapses to one step.)
- **Cancel** (Escape with no tool active): `cancelSketchEdit` (1117-1148) restores the
  snapshot and truncates the undo stack to `pastLength`.
- Escape ladder (`useCanvasKeyboard.ts:796-889`): picked trim/extend subject → active
  tool → the session. Enter/Escape while a tool is active only exits the tool.
- Tools: `SketchEditTool = 'add_point' | 'delete_point' | 'delete_segment' | 'disconnect'
  | 'fillet' | 'chamfer' | 'trim' | 'extend'` (`types.ts:83`). With no tool, the mode is
  "move". Panel: `src/components/canvas/EditSketchPanel.tsx`.
- Linked copies: the edit is written to the shared definition
  (`syncEditedFeatureDefinition`), so every copy changes.

### 4.2 Controls and hit radii

```ts
// types.ts 77-81
export interface SketchControlRef {
  kind: 'anchor' | 'in_handle' | 'out_handle' | 'arc_handle' | 'segment' | 'circle_center'
  index: number
  t?: number
}
```

`hitEditableControl` (`SketchCanvas.tsx:2357-2453`), constants in
`src/components/canvas/SketchCanvas.types.ts:23-27`:

- `NODE_HIT_RADIUS = 9` px for anchors, circle centres and segments.
- `HANDLE_HIT_RADIUS = 7` px for Bézier handles and arc handles.
- Order: nearest anchor first; then Bézier `out_handle` (the outgoing segment's
  `control1`) and `in_handle` (the incoming segment's `control2`); then `arc_handle` (the
  arc's **midpoint**, `arcControlPoint`) and `circle_center`; a `segment` hit only when
  nothing else matched.
- Drawn sizes: `NODE_RADIUS = 5`, `HANDLE_RADIUS = 4`
  (`src/components/canvas/scenePrimitives.ts:34-35`).

### 4.3 Moving things — `moveFeatureControl`

`src/store/slices/featureGeometrySlice.ts:233-471`. A drag opens a history transaction
on pointer-down and commits it on pointer-up, so one drag is one undo step
(`usePointerGestures.ts:541, 1045-1119`).

- **Anchor**: the anchor moves; an adjacent Bézier's near control point moves by the
  same delta (so handles travel with the node); an adjacent **arc is rebuilt through its
  old midpoint** — `arcControlPoint` is captured before the move, then
  `buildArcSegmentFromThreePoints(newStart, newEnd, oldMidpoint)` (255-336). The arc
  therefore changes radius and centre rather than staying concentric.
- **Line segment drag** (`kind: 'segment'`, lines only): both end anchors translate by
  the cursor delta from the grab point `t` (340-353). Arc and Bézier segments cannot be
  dragged as a whole.
- **Bézier handle**: sets that control point; if the anchor's other side is also a
  Bézier, the opposite handle is **rotated to stay collinear while keeping its own
  length** (354-374, 399-420). All Bézier nodes are therefore smooth-with-independent-
  lengths; there is no corner/cusp mode and no symmetric mode.
- **Arc handle**: rebuilds the arc through the dragged point with the same end points
  (375-389).
- **Circle centre**: translates centre and start together (390-398). Dragging the
  circle's single anchor changes the radius.
- After every edit: `normalizeEditableProfileClosure` (`profileEdit.ts:65-83`) sets
  `closed` to whether the last anchor equals the start, and the kind is re-inferred.
- Snapping applies to the drag, with the edited feature excluded from snap targets
  (`usePointerGestures.ts:685-688`); axis lock applies relative to the drag start
  (837-844). If snapping is required and nothing snaps, the node does not move, except a
  segment drag which follows the raw cursor (692-697).
- Dropping an open profile's end node within `OPEN_ENDPOINT_JOIN_HIT_RADIUS = 14` px of
  another open end joins them (`tryJoinDraggedOpenEndpoint`,
  `usePointerGestures.ts:984-1025`).

### 4.4 Typed edits in sketch edit

Clicking (or Tab on) an anchor, a segment or an arc handle opens a small inspector
(`useClickPlacement.ts:795-811`; `useDimensionEditWorkflow.ts`;
`computeEditDimSteps`, `draftHelpers.ts:161-200`):

- a line/Bézier segment → length then angle of its end anchor relative to its start
  anchor (the end anchor moves);
- an arc → radius (`arcHandleFromRadius`, `manualEntry.ts:79-110`: keeps the end points
  and the side of the chord the centre is on; refuses a radius smaller than the half
  chord);
- a circle → radius;
- an anchor → its incoming segment, then its outgoing segment, as successive Tab steps.

Values apply live while typing; Enter and Escape both keep them
(`useCanvasKeyboard.ts:838-848`).

### 4.5 Add / delete / break

- **Add point** (`insertPointIntoProfile`, `profileEdit.ts:461-498`;
  `findSketchInsertTarget`, `draftGeometry.ts:356-384`, within 9 px of a segment and
  `t` in `(0.001, 0.999)`): a line splits into two lines; a Bézier splits by de
  Casteljau at `t` (`splitBezierSegment`, 93-115); an arc or circle splits into two arcs
  with the same centre and direction (`splitArcSegment`, 117-132).
- **Extend an open profile**: with the Add point tool, click an open end (within
  `EXTEND_HIT_RADIUS = 14` px), then click the new point
  (`useClickPlacement.ts:615-660`). The new segment is a line, or a Bézier continuing the
  tangent if the adjacent segment is a Bézier (`extendOpenProfileAtStart/End`,
  134-185). Clicking another open end instead joins the two profiles.
- **Delete point** (`deleteAnchorFromProfile`, 500-586): refused below 3 anchors
  (closed) or 2 (open). The two neighbours are bridged by a line, or by a Bézier reusing
  the outer control points if either neighbour was a Bézier (`buildBridgeSegment`,
  278-303). **An arc next to a deleted point becomes a line.**
- **Delete segment** (`deleteSegmentFromProfile`, 605-646): a closed profile becomes
  open, starting at the deleted segment's end; an open profile splits into two features.
  Refused for a circle and for a single-segment profile.
- **Disconnect** (`disconnectProfileAtAnchor`, 648-692): a closed profile opens at that
  anchor; an open profile splits there into two features.

### 4.6 Fillet and chamfer

`applyLineCornerFillet` (`profileEdit.ts:316-392`) and `applyLineCornerChamfer`
(394-459). **Both require a line-line corner**; arcs and Béziers are refused.

Fillet, with `θ` the interior angle between the two edges at the corner:

- refused when `θ ≤ 1e-3` or `|π − θ| ≤ 1e-3`;
- `trim = radius / tan(θ/2)`; refused when `trim ≥` either edge length;
- tangent points at `corner + direction·trim` on each edge;
- centre at `corner + bisector · radius / sin(θ/2)`;
- the arc is `createFilletArcSegment` (305-314): `clockwise = cross(start − center,
  end − center) < 0`, i.e. always the minor arc.

Chamfer: both edges are cut back by the same `distance` (refused when `distance ≥` an
edge length) and joined by a line. One distance; no angle or two-distance chamfer.

Interaction (`useClickPlacement.ts:747-792`; `useFilletWorkflow.ts`): click a corner
anchor, then move — the radius follows the cursor
(`filletRadiusFromPoint`, `src/store/helpers/referenceTransforms.ts:345-389`:
`trim = max(0, cursor·incomingDir, cursor·outgoingDir)`, `radius = trim·tan(θ/2)`) — and
click to commit, or Tab to type the radius and Enter.

A fillet is **baked geometry**: there is no stored radius and no way to re-edit or
remove a fillet except by editing nodes.

### 4.7 Trim and extend (inside sketch edit)

`trimFeatureSegment` (`featureGeometrySlice.ts:803-1052`) and `extendFeatureEndpoint`
(1054-1277). Both are explicitly "MVP":

- Only for **open** subject profiles (the tools are disabled while editing a closed
  feature, `src/commands/sketchCommands.ts:167-168, 290-297`).
- Lines and arcs only; a Bézier subject or reference is refused
  (`resolveProfileSegments`, `src/store/helpers/resolveProfileSegments.ts:31-104`,
  returns `null` for Béziers).
- Two picks: the subject segment (on any open feature), then one reference segment on
  any feature.
- **Trim** removes the part between the picked side's free end and the first crossing
  with the reference. An interior trim (removing a middle piece) is "not yet
  implemented" (957).
- **Extend** grows a free end along its line, or along its circle, to the nearest
  forward hit on the reference; if the reference segment is not reached, it retries
  against the reference's supporting infinite line (1183-1211). Only the first or last
  segment can be extended.
- Intersections: `segmentIntersections` in
  `src/store/helpers/segmentIntersection.ts` (line-line, line-arc, arc-arc; 42 tests).

### 4.8 Converting segments

**Does not exist.** There is no command to convert a line to an arc or a curve, an arc
to a line or Béziers, or to toggle a node between smooth and corner. The only
conversions are side effects: deleting a point next to an arc yields a line; a
non-similarity transform turns arcs into Béziers; join/cut/offset re-derive geometry.

### 4.9 Tests

`src/store/helpers/profileEdit.test.ts` (10 tests, 44 assertions),
`src/store/editOpFidelity.test.ts` (5), `src/store/editInPlace.test.ts` (17),
`src/store/openProfileJoin.test.ts` (7),
`src/store/helpers/segmentIntersection.test.ts` (42),
`src/store/helpers/resolveProfileSegments.test.ts` (24),
`src/store/definitionEditing.test.ts` (6).

---

## 5. Snapping

### 5.1 Kinds and settings — `src/sketch/snapping.ts`

```ts
export type SnapMode = 'grid' | 'point' | 'line' | 'midpoint' | 'center' | 'intersection' | 'perpendicular'
export interface SnapSettings { enabled: boolean; modes: SnapMode[]; pixelRadius: number }
export const DEFAULT_SNAP_SETTINGS: SnapSettings = {
  enabled: true,
  modes: ['grid', 'point', 'line', 'midpoint', 'center'],
  pixelRadius: 14,
}
```

- `pixelRadius` is clamped to 6-32 on load (`normalizeSnapSettings`, 48-68).
- Settings are an **application preference**, not project data: local storage key
  `camcam.snapSettings` (`src/app/useSnapSettings.ts:21`). One master toggle and one
  toggle per mode (31-40). No UI to change `pixelRadius` was found **[inferred: not
  searched exhaustively]**.
- Intersection and perpendicular are **off by default**.

### 5.2 What each kind snaps to — `src/components/canvas/snappingHelpers.ts`

| Mode | Candidate | Lines |
|---|---|---|
| `grid` | `round(v / grid.snapIncrement) · grid.snapIncrement` on each axis | 105-107, 437-443 |
| `point` | every profile vertex (`profileVertices`), and the machine origin when visible | 291-298, 482-485 |
| `midpoint` | line midpoint; Bézier at `t = 0.5`; arc midpoint | 304-309 (`segmentMidpoint`, `draftGeometry.ts:123-133`) |
| `center` | centre of every arc and circle segment | 311-316 |
| `line` | nearest point on a segment: exact projection for lines, nearest point on a sampled polyline for arcs and Béziers (12 samples per Bézier; arcs at `π/36`) | 322-345, 362-389 |
| `perpendicular` | foot of the perpendicular from the **current reference point** onto a segment | 346-358, 391-402 |
| `intersection` | crossings between segments of any two profiles, and non-adjacent segments of one profile; lines and arcs only | 199-278 |

Notes:

- "Endpoint" is not a separate kind: `point` covers every vertex, end or interior.
- `perpendicular` only exists while there is a reference point: the first corner/centre
  of rect, circle, ellipse; the last polygon/spline/composite point; a move's from-point;
  a rotate/mirror reference start; a constraint anchor
  (`currentSnapReferencePoint`, `src/components/canvas/useSnapPreview.ts:127-158`).
  Slot, n-gon, gear and rounded/chamfered rect anchors are not in that list.
- Snap targets: the stock outline, every **visible** feature on the active face
  (including construction and regions), tabs, clamps, the origin (445-485).
  Hidden features do not snap.
- For **text and canned shapes the snap target is the frame profile, not the generated
  outline**: the loop reads `feature.sketch.profile` (458-459), which
  `resolveFeatureRow` replaces with the frame for those kinds
  (`resolveFeatures.ts:223-245`). So you cannot snap to an ellipse's curve, a slot's
  arc centre, a rounded rectangle's corner arc or a gear tooth.
- Intersection with a Bézier is not found (`resolveProfileSegments` returns `null`).

### 5.3 Priority and tolerance

A candidate is kept only if it is within `pixelRadius` screen pixels of the raw cursor
(`pushSnapCandidate`, 170-197). Candidates sort by priority, then by pixel distance
(495-498):

```ts
function snapPriority(mode: SnapMode): number {
  if (mode === 'point' || mode === 'center' || mode === 'midpoint' || mode === 'intersection') return 1
  if (mode === 'perpendicular') return 2
  if (mode === 'line') return 3
  return 4   // grid
}
```

So a vertex 13 px away beats an on-line point 1 px away, and grid is always last.

### 5.4 The snap gate

`requiresResolvedSnapForPointPick()` is true whenever snapping is enabled with at least
one mode (`useSnapPreview.ts:160-163`). Then:

- a **click with no snap candidate is discarded** (`useClickPlacement.ts:400`, and each
  branch's `if (!pickedPoint) return`);
- a node drag with no candidate does not move the node
  (`usePointerGestures.ts:692-697, 833-836`).

With grid snapping on, a candidate exists only when a grid point is within 14 px, so
when zoomed in far enough that grid points are more than about 28 px apart, clicks in
the gaps do nothing unless another mode catches them. This is real behaviour, not an
inference; whether it is intended is not stated anywhere I read.

### 5.5 Feedback

`drawSnapIndicator` (`draftHelpers.ts:371-438`): a marker per kind — diamond (midpoint),
circle with cross-hair (center), square (perpendicular), X (intersection), circle
(everything else) — plus a dashed guide line for `line` (cursor → projected point) and
`perpendicular` (reference point → foot), and a text label ("Grid", "Point", "Line",
"Midpoint", "Center", "Intersection", "Perpendicular") shown for
`SNAP_LABEL_VISIBLE_MS = 2500` ms (`useSnapPreview.ts:35`).

### 5.6 Not present

No tangent snap, no quadrant snap, no nearest-on-curve distinct from `line`, no
extension/alignment tracking (inferred horizontal/vertical alignment with other
points), no polar/angle snap, no parallel snap, no snap to the grid lines (only grid
points), no object-snap override keys, no guides. Angle constraint while drawing is only
the X/Y axis lock (section 2.2).

### 5.7 Tests

`src/components/canvas/snappingHelpers.test.ts` (5 functions, 24 assertions),
`src/sketch/useAxisLock.test.ts` (9 functions).

---

## 6. Editing commands

Command surface: `src/commands/sketchCommands.ts:92-104` —
transform `copy | move | delete | resize | rotate | mirror`;
boolean `join | cut | offset`; arrange `align | distribute`; the eight sketch-edit tools;
constraint; dimension tools.

### 6.1 Move and Copy

`PendingMoveTool { mode: 'move' | 'copy'; entityType: 'feature' | 'clamp' | 'tab' |
'backdrop'; entityIds; fromPoint; toPoint; copyMode? }` (`types.ts:201-210`).

- Clicks: a **from** point, then a **to** point. After the second click a distance field
  opens pre-filled with the distance; Enter confirms (the direction stays that of the two
  clicks, the length is the typed value). Tab opens the same field before the second
  click (`useClickPlacement.ts:908-926`; `src/components/canvas/useMoveWorkflow.ts:113-164`).
- Move commits `transform = translate(dx, dy) · transform` on each selected instance
  (`pendingCompletionSlice.ts:314-333`). Locked features do not move.
- **Copy** then asks for a count; copy `k` is placed at `k·(dx, dy)` — a **linear array**
  (`buildCopiedFeatures`, `src/store/helpers/copyFeatures.ts:134-241`). Enter commits
  (`useCanvasKeyboard.ts:809-820`).
- Reference vs independent: `pendingMove.copyMode ?? project.meta.copyMode`
  (`pendingCompletionSlice.ts:229`). Reference copies share the `definitionId`;
  independent copies clone the definition with id `f-<newId>`.
- Names: `duplicateFeatureName` (in `naming.ts`, not read).
- There is **no one-key Duplicate in place**; "Copy" is always the two-point gesture.

### 6.2 Clipboard

`src/platform/featureClipboard.ts`; wired in `src/platform/useDesktopIntegration.ts:109-140`.

- The clipboard is an **in-memory ref inside the app**, not the OS clipboard.
- Copy takes the selected **visible** features plus their definitions (56-71). Cut is
  copy + delete (164-172).
- Paste enters a placement mode: the payload follows the cursor and one click places it
  with its **bounding-box centre** at the click (`featureClipboardAnchor`, 91-99;
  `pasteClipboardFeatures`, 174-206). It honours `meta.copyMode`, so a paste is a linked
  copy by default. One undo step.
- Tests: `src/platform/featureClipboard.test.ts` (7 functions).

### 6.3 Resize, Rotate, Mirror (reference-based transforms)

`PendingTransformTool { mode: 'resize' | 'rotate' | 'mirror'; entityType: 'feature' |
'backdrop'; referenceStart; referenceEnd; keepOriginals }` (`types.ts:212-220`).
Maths in `src/store/helpers/referenceTransforms.ts`.

- **Resize** (99-184): click a reference start (the fixed point), a reference end, then
  a target point which is projected onto the reference line. The scale factor is
  `projected length / reference length`, or typed (Tab → "Scale" field,
  `computeScaleFactorFromPreview`, `manualEntry.ts:241-256`).
  - It is expressed in the feature's own basis: `v = direction(orientationAngle)`,
    `u = direction(orientationAngle − 90°)`.
  - **Axis snap**: if the reference vector is within **12°** of `u` or `v`
    (`cos 12°` test, 69-79), only that axis scales — a one-directional stretch.
    Otherwise both axes scale by their own projected ratios.
  - Imported models always scale uniformly, Z included.
  - Refused when a scale is `≤ 1e-6` or not finite.
- **Rotate** (186-222): click the pivot, a reference end, then a target; the angle is
  `atan2(cross, dot)` between the two rays, or typed (Tab → "Angle"). With
  `keepOriginals` on, it asks for a count and creates copies at `k·angle` — a **circular
  array about the pivot** (`buildRotatedCopies`, `copyFeatures.ts:58-99`).
- **Mirror** (224-262): click two points defining the axis; it commits on the second
  click. `keepOriginals` (key K) creates mirrored linked copies instead
  (`buildMirroredCopies`, 101-132). Arc flags flip (`mirrorProfile`,
  `src/store/helpers/transform.ts:142-174`).
- All three compose a delta matrix onto the instance transform
  (`rotateDelta`, `mirrorDelta`, `src/store/helpers/instanceTransforms.ts:116-156`), so
  definitions are untouched and copies stay linked.
- There are **no bounding-box handles** to drag for scale or rotate; every transform is
  the pick-reference-points workflow.
- Tests: `src/store/projectStoreTransform.test.ts` (11),
  `src/store/instanceTransforms.test.ts` (22), `src/store/geometryFidelity.test.ts`
  (48 — every kind × every transform class), `src/store/folderGroupTransforms.test.ts`
  (20).

### 6.4 Offset

- Eligible: every selected feature closed, none canned, none text
  (`sketchCommands.ts:155-156`). **Open paths cannot be offset.**
- Interaction (`useClickPlacement.ts:958-973`; `resolveOffsetPreview`,
  `draftGeometry.ts:291-354`): move the cursor; the distance is the cursor's distance to
  the nearest boundary point; **negative (inward) when the cursor is inside** a source
  profile, positive outside. Click to commit, or Tab to type a signed distance.
- Algorithm (`previewOffsetFeaturesInternal`,
  `src/store/helpers/derivedFeatures.ts:404-443`): the selected features are **unioned
  first**, then offset with Clipper `ClipperOffset`, `jtMiter`, `etClosedPolygon`
  (`src/store/helpers/clipping.ts:291-300`; default miter limit and arc tolerance, none
  set). Corners are therefore **mitred, not rounded**.
- Clean-up (`simplifyOffsetContour`,
  `src/engine/toolpaths/arcReconstruction.ts:1313-1344`): merge collinear chords, re-fit
  chord runs to arcs **only when the fitted centre matches a source arc/circle centre**,
  Ramer-Douglas-Peucker on the remaining line runs with tolerance
  `0.001 × bounding-box diagonal`, promote a closed arc to a `circle`. Béziers in the
  source come back as polylines.
- Result: new independent features named `"<base> Offset"`, originals kept, operation
  copied from the last selected feature (a `model` becomes `add`), same Z range.
- Tests: `src/store/helpers/offsetSimplify.test.ts` (45 assertions).

### 6.5 Join (union) and Cut (difference / split)

- **Join** (`mergeSelectedFeatures`, `src/store/slices/featureSlice.ts:855-951`): ≥ 2
  closed features → Clipper union (non-zero). Reconstruction tries
  `clipperContourToProfilePreserving` — it maps output vertices back to the source
  segments through an integer-coordinate annotation map, so **original arcs and Béziers
  survive** and only new intersection vertices become line joints — and falls back to a
  plain polygon. Name `"<stem> Join"`. Originals are replaced unless Keep originals (K).
  - The interactive workflow only lets you add features that form a **connected overlap
    group**: overlapping area or a shared boundary segment of positive length; corner
    contact does not count (`selectionSlice.ts:262-306`; `clipping.ts:151-289`).
  - A selection that already qualifies skips the confirmation panel
    (`src/store/helpers/shapeActionEligibility.ts`, not read).
- **Cut** (`cutFeaturesByCutterGrouped`, `derivedFeatures.ts:237-340`): two phases —
  pick cutters, Tab, pick targets, Enter.
  - closed target − closed cutters → Clipper difference; the result can be several
    pieces, with holes;
  - closed target split by an **open** cutter → `splitClosedByOpen`
    (`src/store/helpers/polygonSplit.ts`, not read) into ≥ 2 closed pieces, only when
    the cutter fully crosses;
  - open target trimmed by closed cutters → Clipper open-path difference; the result is
    polylines (arcs are lost);
  - open target with only open cutters → not allowed.
  - Cutters are kept; targets are replaced unless Keep originals.
- **There is no intersect and no XOR** command. `ctIntersection` is used only as an
  overlap test.
- Join and cut are 2D boolean operations **on the outlines** and produce new independent
  definitions; they are unrelated to the Add/Subtract 3D model roles.
- Open-profile join (end to end) is in section 4.3/4.5
  (`joinOpenProfiles`, `derivedFeatures.ts:485-513`: inserts a bridging line if the ends
  do not coincide; closing a Line turns its operation into `subtract`,
  `featureGeometrySlice.ts:557-558`).
- Tests: `src/store/joinSharedEdge.test.ts` (7), `src/store/joinImmediate.test.ts` (17),
  `src/store/helpers/clipping.test.ts` (12), `src/store/polygonSplit.test.ts` (10),
  `src/store/snapshotOps.test.ts` (14), `src/store/second_cut_test.ts`.

### 6.6 Align and Distribute

`featureSlice.ts:1128-1284`. Both translate instances; locked features are skipped.

- `FeatureAlignment = 'left' | 'center_horizontal' | 'right' | 'top' | 'center_vertical'
  | 'bottom'`. The reference is the **combined bounding box of the selection** (not the
  first-selected item, not the stock). `top` aligns `minY`, `bottom` aligns `maxY` —
  correct for Y-down, **inverted for Y-up**. Needs ≥ 2 features.
- `FeatureDistribution = 'horizontal_gaps' | 'horizontal_centers' | 'vertical_gaps' |
  'vertical_centers'`. Needs ≥ 3. The first and last (after sorting) stay put; `centers`
  spaces centres evenly, `gaps` makes the gaps equal:
  `gap = (span − Σ sizes) / (n − 1)`.
- There is no "align to stock/material" and no "centre in stock" command.
- Tests: `src/store/featureLifecycleOps.test.ts`.

### 6.7 Arrays — "feature distribution"

`src/sketch/featureDistribution.ts` (planner), `src/store/slices/featureDistributionSlice.ts`
(commit), `src/components/canvas/FeatureDistributionPanel.tsx`.

```ts
export type FeatureDistributionSpec =
  | { mode: 'grid'; rows; columns; spacingX; spacingY; startScale; endScale }
  | { mode: 'radial'; center; copyCount; sweepDegrees; orientation: 'fixed' | 'follow'; startScale; endScale }
  | { mode: 'path'; copyCount; startOffset; endOffset; orientation: 'fixed' | 'follow'; startScale; endScale }
```

- Defaults (116-157): grid 1 row × 2 columns, spacing 20 mm / 2 in; radial 4 copies,
  360°, follow; path 4 copies, offsets 0, follow; scales 100 %.
- The pivot of the source set is its bounding-box centre (160-168).
- **Grid**: copies at `pivot + (column·spacingX, row·spacingY)`; the source is cell
  (0, 0); spacing is **pitch (centre to centre), not gap**; needs `rows·columns ≥ 2`.
- **Radial**: radius and start angle come from the source pivot relative to the picked
  centre; a full circle divides by `copyCount`, a partial sweep by `copyCount − 1`;
  `follow` rotates each copy by its angle offset.
- **Path**: copies at equal arc-length steps along a picked guide feature between two
  offsets; a closed guide divides by `copyCount`, an open one by `copyCount − 1`; the
  source itself is moved onto the path start; `follow` aligns to the tangent.
- `startScale`/`endScale` interpolate a uniform scale across the copies.
- `copyCount` **includes the source**.
- It is **one-shot**: the result is ordinary linked (or independent) instances; no array
  object remembers the parameters.
- Tests: `src/sketch/featureDistribution.test.ts` (7),
  `src/store/featureDistribution.test.ts` (5).

### 6.8 Selection

- Click: `resolveFeatureSelectionHit` (`src/components/canvas/hitTest.ts:402-448`). A
  feature is hit when the point is within **8 px** of its outline (`pointNearProfile`,
  126) or inside it. Several interior hits resolve to the innermost shape (or the
  topmost); two or more coincident outlines open an overlap picker.
- Ctrl / Cmd / Shift-click is additive (`useClickPlacement.ts:1020`).
- **Shift-drag pans**, as do the middle and right buttons
  (`usePointerGestures.ts:389-397`).
- Drag on empty canvas: marquee; it selects features **fully inside** the rectangle and
  adds to the current selection (1087-1114). There is no crossing-selection direction.
  The marquee needs ≥ 6 px of movement.
- Wheel zoom about the cursor: `zoom · exp(−deltaY · 0.0015)`, minimum 0.02 (1196-1222).
  Zoom-window tool exists.
- Tests: `src/components/canvas/hitTest.test.ts` (51 assertions),
  `src/store/bulkSelection.test.ts` (94 tests).

### 6.9 Undo

Whole-project snapshots: `history.past` is capped at **100** entries
(`.slice(-100)` at every write, for example `pendingAddSlice.ts:477`).
`beginHistoryTransaction` / `commitHistoryTransaction` group a drag into one step.

---

## 7. Backdrop, measurement, dimension display

### 7.1 Backdrop

`src/store/slices/backdropSlice.ts`.

- One image per project, embedded as a data URL.
- On load (`createBackdropFromImage`, 55-71): fitted, keeping aspect ratio, inside 90 %
  of the stock bounds (minimum 10 mm), centred on the stock, `orientationAngle: 90`,
  `opacity: 0.6`, visible. Loading a second image replaces the first and keeps its
  placement (`replaceBackdropImage`, 73-87).
- Properties: centre, width, height, orientation angle, opacity (0-1), visibility
  (`normalizeBackdrop`, 89-120).
- It is selectable (only when nothing else is hit, `useClickPlacement.ts:1049-1050`) and
  supports Move, Resize and Rotate through the same reference-point workflows
  (`startMoveBackdrop`, `startResizeBackdrop`, `startRotateBackdrop`;
  `resizeBackdropFromReference`, `rotateBackdropFromReference`,
  `referenceTransforms.ts:264-343`). **Resize-by-reference is the calibration method**:
  click two points on the image a known distance apart, then type the scale or click the
  target. There is no "set this distance to X" calibration and no mirror.
- **No bitmap tracing / vectorisation exists** (no match for vectorize, potrace or
  similar anywhere in `src/`). Tracing is manual drawing over the image.
- The backdrop is not a snap target (it is not in `resolveSketchSnap`).
- Supported formats were not checked (the loader UI was not read).

### 7.2 Tape measure

`src/store/slices/dimensionToolSlice.ts:67-89`; drawing in
`src/components/canvas/dimensionRendering.ts:242-282`.

- Transient, not saved, not in undo.
- Click A (snapped), move (live dashed span), click B → the measurement freezes until
  the next click starts a new one. Escape leaves the tool.
- Label: `distance · Δx … Δy … · angle`, with the angle shown as
  `|atan2(dy, dx)|` in degrees (so 0-180, unsigned).
- Point-to-point only. No measuring of an object's length, area or radius.

### 7.3 Dimension display while drawing and editing

`src/components/canvas/measurements.ts`: `drawLineLengthMeasurement`,
`drawArcRadiusMeasurement`, `drawRadiusMeasurement`, `drawProfileLineMeasurements`,
`drawAngleMeasurement`, `drawActiveEditMeasurements`. Live length labels on draft
segments and radius labels on draft arcs are drawn from `draftHelpers.ts:292-302`.
`meta.showFeatureInfo` and `meta.showDimensions` are project-level display toggles
(`project.ts:1276-1277`).

### 7.4 Persistent dimension annotations (exclude or defer)

`project.annotations`, six types (section 1.8). Placement: pick 2 anchors (3 for an
angle), then a click sets the offset; radius/diameter must start on a centre snap and
commit on the edge click (`useClickPlacement.ts:456-525`). Values are never stored; they
are recomputed from anchors that follow the geometry (`src/sketch/dimensions.ts`).
Annotations can be dragged, deleted (armed delete mode, or Delete on a selected one),
and — see section 8 — clicked to drive geometry.

---

## 8. Constraints and parametrics (note for exclusion)

These exist. A spec that wants "no constraints" must say so, because the port's
reference has them woven into the edit paths.

1. **Fixed-distance constraints between features.** The type unions are wide
   (`LocalConstraintType` = `horizontal | vertical | equal_length | equal_radius |
   tangent | fixed_distance | fixed_angle | fixed_radius`; `GlobalConstraintType` =
   `concentric | equal_spacing | symmetric | coincident_edge`; `project.ts:145-159`), but
   **only `fixed_distance` is implemented**: every branch in
   `src/sketch/constraintSolver.ts` and `src/store/slices/constraintsSlice.ts` filters on
   `c.type !== 'fixed_distance'`. `global_constraints` is carried in the file but nothing
   solves it.
   - Workflow: key `c` in sketch edit, or the Constraint button → pick an anchor on the
     feature → pick a reference (snap point, perpendicular segment, intersection) → type
     a distance (`PendingConstraint`, `types.ts:308-322`).
   - Effect: the constrained feature is **translated rigidly** to satisfy the distance
     (`solveFeatureTranslation`, `propagateRigidTransforms`,
     `propagateConstraintsOnTranslate`, `propagateConstraintsOnRotate`). It is not a
     general geometric solver: no degrees-of-freedom analysis, no shape change.
   - Every move, node edit, join and transform calls into it
     (`featureGeometrySlice.ts:434-453`; `pendingCompletionSlice.ts:334-343`), which is
     why it cannot be ignored when porting those paths — the calls have to be dropped
     deliberately.
2. **Driving dimensions.** Clicking a dimension annotation's label opens an edit that
   changes the underlying geometry (`src/sketch/drivingDimensionResolver.ts`,
   `src/components/canvas/useDrivingDimensionWorkflow.ts`); the stock's width/height
   labels work the same way.
3. **Named dimensions with formulas** (`NamedDimension { id, name, value, formula }`,
   `project.ts:70-75`; `Project.dimensions`). `DimensionRef = number | string` is used
   for feature `z_top`/`z_bottom`. Whether formulas are evaluated anywhere was not
   checked.
4. **`LocalDimension`** (`project.ts:207-213`) and `FeatureDefinition.dimensions`: the
   field is copied around everywhere but I found no code that creates one
   **[inferred: vestigial]**.
5. **Canned shapes** are the real parametric layer for 2D shapes (section 1.7).
6. Tests: `src/sketch/constraintSolver.test.ts` (16 functions, 1,417 lines),
   `src/sketch/drivingDimensionResolver.test.ts` (718 lines),
   `src/sketch/dimensions.test.ts`, `src/store/linkedConstraintResolve.test.ts`.

---

## 9. Tool-library editor UI (brief)

- Project tools live in `project.tools`; CRUD actions `addTool`, `importTools`,
  `updateTool`, `deleteTool`, `duplicateTool` (`src/store/slices/toolsSlice.ts`).
- The tool list and per-tool editing fields are in `src/components/cam/CAMPanel.tsx`.
- The library browser/import dialog is `src/components/cam/ToolLibraryDialog.tsx`, with
  its pure model in `src/components/cam/toolLibraryDialogModel.ts` (+ test).
- The bundled library is `public/tool-library.json`, loaded by `src/toolLibrary.ts`
  (`parseToolLibraryFile`, 130; fetch at 147-163), structural test
  `src/toolLibrary.test.ts`.
- Tool type and defaults: `Tool`, `ToolType`, `defaultTool` in `project.ts:703-731,
  1804`.

---

## 10. Coordinate-convention gotchas

PureCutCNC's internal 2D space is Y-down with no flip between world and canvas
(`worldToCanvas`, `src/components/canvas/viewTransform.ts:59-66`). Every item below has
to be re-derived for DreamCarve's Y-up space (ADR-0001), not copied.

### 10.1 Two "clockwise" vocabularies in one codebase

1. **Segment flag and engine winding: the mathematical sense on raw numbers.**
   - `ArcSegment.clockwise: true` ⇒ the sweep is negative, the atan2 angle **decreases**
     (`project.ts:1974-1979`, repeated in `profilePrimitives.ts:33-41`,
     `draftGeometry.ts:209-218`, `featureDistribution.ts:379-381`).
   - The flag is passed unchanged as `ctx.arc`'s `counterclockwise` argument
     (`src/components/canvas/profilePrimitives.ts:83, 93`). On the Y-down canvas a
     `clockwise: true` arc is therefore drawn **counter-clockwise as seen on screen**.
   - `isClockwise(points) = signedArea < 0` with the standard shoelace sum
     (`src/engine/toolpaths/geometry.ts:216-230`): also the Y-up sense applied to raw
     numbers, so also visually reversed on PureCutCNC's screen.
   - `ArcSeg.ccw = !seg.clockwise` (`resolveProfileSegments.ts:64`).
2. **User-facing angles and some comments: the on-screen sense.**
   - `TextLayout.angleDegrees`: "0 = 3 o'clock, positive = clockwise on screen"
     (`project.ts:300`); arc `direction: 'cw'` is `sign = +1`, increasing angle
     (`src/text/baseline.ts`, `arcBaseline`).
   - `ellipseProfile`'s comment "go clockwise (screen coords: +Y down)" describes an
     **increasing**-angle traversal (`project.ts:1405`), and
     `circleToBezierSegments` says the same (`profileMatrix.ts:111`).

Porting rule: decide per item whether DreamCarve keeps the **numbers** or the **picture**.

- If a PureCutCNC coordinate is converted by negating Y (as the golden corpus must),
  every arc and circle flag inverts — the same rule the code applies for a mirror
  (`profileMatrix.ts:214, 237`; `transform.ts:155`; `cannedShapes/index.ts:176-184`).
- If a constructor is ported formula-for-formula into Y-up, the numbers and flags are
  self-consistent and `clockwise: true` becomes a true clockwise arc; but the picture is
  the vertical mirror of PureCutCNC's.

### 10.2 Winding of the constructors is not uniform

Signed area with the standard shoelace formula on raw numbers:

| Constructor | Sign | On PureCutCNC's screen |
|---|---|---|
| `rectProfile` | positive | clockwise |
| `chamferedRectProfile` | positive (its comment says "CCW") | clockwise |
| `roundedRectProfile` | positive, fillet arcs `clockwise: false` | clockwise |
| `ngonProfile`, gear | positive (increasing angle) | clockwise |
| `ellipseProfile` | positive (increasing angle) | clockwise |
| `circleProfile` | negative (`clockwise: true`) | counter-clockwise |
| `slotProfile` | negative (arcs `clockwise: true`) | counter-clockwise |
| polygon / spline / composite | whatever the user clicked | — |
| text outlines | whatever three.js gives after the Y flip; `cleanOutlineContour` preserves the source sign | — |

The model never normalizes winding. Consumers do it at the point of use
(`flattenFeatureToClipperPath` → `normalizeWinding(points, false)`,
`clipping.ts:46-49`). **This table is my derivation, not something the code states**:
the rect, chamfered-rect and slot signs were worked out by hand from the vertex order,
the rounded-rect arc flag from the fillet rule applied to that order, and the rest from
the direction of angle increase. No test was run to confirm it.

### 10.3 Other Y-dependent rules

- `rectProfile(x, y, w, h)`: `(x, y)` is the minimum corner — top-left on screen in
  PureCutCNC, bottom-left in Y-up.
- `slotProfile`: the start point A is on the `(-sin θ, cos θ)` side of the axis, which is
  below the axis on screen in Y-down and above it in Y-up.
- `buildArcSegmentFromThreePoints`: `clockwise = cross < 0` where
  `cross = (through − start) × (end − start)` — numerically invariant, visually mirrored.
- Fillet arcs: `clockwise = cross(start − c, end − c) < 0` — same remark.
- Typed direction angles are `atan2(dy, dx)`: +90° is **down** the screen in PureCutCNC.
  In Y-up it is up. The n-gon `rotation` parameter and the resize basis
  (`orientationAngle`, default 90 = "local +Y") are the same quantity.
- Rotation angle sign: `atan2(cross, dot)` (`referenceTransforms.ts:200`) — positive
  turns +X toward +Y: clockwise on screen in Y-down, counter-clockwise in Y-up.
- Align `top` = `minY`, `bottom` = `maxY` (`featureSlice.ts:1168-1173`); distribute
  "vertical" sorts by Y ascending = top to bottom on screen.
- Grid arrays: rows advance in +Y = down the screen.
- Text: the stroke glyph table is y-down (feet at `y = 1`); outline fonts arrive y-up
  and are flipped (`flipProfileY`, `text/index.ts:302-304`). In Y-up the stroke table
  needs flipping and the outline flip must be removed. The text frame's anchor is its
  minimum corner; `bendShapesToBaseline` assumes `maxY` is the run's bottom
  (`baseline.ts`, "the run is y-down").
- Text-on-arc: top of circle is 270°, bottom is 90° (`textPlacement.ts:105-108`).
- `KAPPA` is `0.5523` in `ellipseProfile` but `0.5522847498` in `profileMatrix.ts:91`;
  `inferFeatureKind` compares ellipse control points with tolerance `1e-4`, which is why
  the mismatch is harmless there.
- `Matrix2D` is the SVG/canvas layout (`x' = a·x + c·y + e`), and a positive rotation
  matrix (`rotateMatrix`) rotates +X toward +Y.
- Offset sign: negative = inward is decided by point-in-profile, not by winding
  (`draftGeometry.ts:315-321`), so it is convention-independent.
- Bottom-face and Around-face view mirroring (`viewTransform.ts:38-105`,
  `faceAngles`) is a second layer of flipping that phase 5 can ignore if it is
  single-face.

---

## 11. Tests worth porting as red tests

PureCutCNC's tests are mostly **plain `tsx` scripts**: `function testX()` bodies with a
local `assert`, run with `npx tsx <file>`; a minority use a `test('name', fn)` helper.
Counts below are test functions or named cases, with assertion-line counts where the
file has no named cases. They are approximate (grep-based).

Pure geometry — direct ports:

| File | Count | Asserts |
|---|---|---|
| `src/types/slot.test.ts` | 7 | 4 segments typed line/arc/line/arc; arc centres are p1/p2; radii = width/2; closure; start point for a horizontal slot; 45° case |
| `src/types/ngon.test.ts` | 7 | vertex count, circumradius, first-vertex angle, closure |
| `src/types/project.test.ts` | 3 | project-level helpers |
| `src/geometry/profile.test.ts` | 8 | clone/translate/transform of each segment kind |
| `src/store/helpers/cannedRectProfiles.test.ts` | 15 | rounded and chamfered rectangle outlines, clamping, zero corner |
| `src/store/helpers/profileEdit.test.ts` | 10 | fillet, chamfer, insert, delete, disconnect, spline draft |
| `src/store/helpers/segmentIntersection.test.ts` | 42 | line-line, line-arc, arc-arc, ray mode, tangency, endpoints |
| `src/store/helpers/resolveProfileSegments.test.ts` | 24 | 1:1 segment resolution, sweep sign, circle, Bézier → null |
| `src/store/helpers/offsetSimplify.test.ts` | 45 asserts | arcs re-fitted after offset, collinear merge, circle promotion |
| `src/store/helpers/clipping.test.ts` | 12 | overlap / shared-edge connectivity for join |
| `src/store/polygonSplit.test.ts` | 10 | closed shape split by an open cutter |
| `src/sketch/gearProfile.test.ts` | 9 | module/pitch maths, validation messages, point counts, closure |
| `src/sketch/featureDistribution.test.ts` | 7 | grid, radial (full and partial), path placement, validation codes |
| `src/store/instanceTransforms.test.ts` | 22 | matrix builders, inverse, pivot, mirror |
| `src/text/baseline.test.ts` | 15 | arc and path baselines, anchor, fit, similarity-only glyph transforms |
| `src/text/outlineContours.test.ts` | 5 | sliver removal, winding preserved, disconnected components kept |
| `src/cannedShapes/generators/shapes.test.ts` | 11 | local outline equals the plain constructor, parameter refusal, unit conversion of lengths only, determinism |
| `src/cannedShapes/generators/gear.test.ts` | 8 | generated outline equals `buildGearProfile`, bore nesting |
| `src/cannedShapes/cannedShapes.test.ts` | 13 | nesting depth → operation, placement, mirror winding |
| `src/cannedShapes/ellipseHandles.test.ts` | 31 asserts | handle positions under transforms, axis-locked drag, minimum clamp |

Interaction and snapping — portable as behaviour tests:

| File | Count | Asserts |
|---|---|---|
| `src/components/canvas/snappingHelpers.test.ts` | 5 | snap priority and candidates, intersection snap, anchors |
| `src/sketch/useAxisLock.test.ts` | 9 | lock cycle, `applyLock`, Alt-tap rules |
| `src/components/canvas/hitTest.test.ts` | 51 asserts | outline tolerance, innermost-first selection, ambiguity |
| `src/components/canvas/useClickPlacement.test.ts` | 4 asserts | Around-sheet band picking only — **not** a test of the drawing state machine |
| `src/components/canvas/previewPrimitives.test.ts` | 7 | Line/Construction stroke-only previews, pending gear bore |
| `src/commands/sketchCommands.test.ts` | 7 | enable/active predicates for every command |
| `src/platform/featureClipboard.test.ts` | 7 | copy/cut/paste, anchor, definitions carried |

Store-level — the behaviour is portable, the harness is not:

| File | Count | Asserts |
|---|---|---|
| `src/store/stickyDrawing.test.ts` | 7 | tool re-arms after each shape; parameters persist |
| `src/store/cannedShapeCreation.test.ts` | 14 | each canned shape through the real creation path equals the plain outline; save/load; one edit = one undo |
| `src/store/gearCreation.test.ts` | 10 | gear + bore as one feature |
| `src/store/featureLifecycle.test.ts` | — | create → definition, save/load, undo/redo, delete → GC, per kind |
| `src/store/creationDefinitions.test.ts` | — | every creation path mints a definition |
| `src/store/manualNestingDefaults.test.ts` | 6 | Add/Subtract alternation by nesting |
| `src/store/constructionWorkflows.test.ts` | 11 | construction target, role conversion, open profiles |
| `src/store/duplicateReference.test.ts` | 14 asserts | reference copies share a definition; independent copies clone |
| `src/store/definitionEditing.test.ts` | 6 | shared edit propagation; make unique |
| `src/store/editInPlace.test.ts` | 17 | editing a transformed linked instance round-trips through the inverse transform |
| `src/store/editOpFidelity.test.ts` | 5 | insert/delete/disconnect/arc-handle keep segment kinds |
| `src/store/geometryFidelity.test.ts` | 48 | every kind × every transform class |
| `src/store/openProfileJoin.test.ts` | 7 | joining open ends, bridging line, closing |
| `src/store/joinSharedEdge.test.ts`, `joinImmediate.test.ts` | 7, 17 | join grouping rules |
| `src/store/snapshotOps.test.ts` | 14 | join/cut/offset produce fresh definitions |
| `src/store/projectStoreTransform.test.ts` | 11 | resize/rotate/mirror from reference |
| `src/store/folderGroup.test.ts`, `folderGroupTransforms.test.ts` | 31, 20 | grouped folders select and transform as a unit |
| `src/store/copyFeatureOrder.test.ts` | 5 | copies keep authored order |
| `src/store/featureDistribution.test.ts` | 5 | array commit, one undo step |
| `src/store/featureLifecycleOps.test.ts` | — | align and distribute with undo |
| `src/store/textExpansion.test.ts`, `textFrameRefit.test.ts`, `textLayout.test.ts` | 14, 40 asserts, 14 | text expansion, frame refit, baseline workflow |
| `src/store/bulkSelection.test.ts` | 94 | selection families, additive toggling |

**Coverage gap to plan for:** I found no unit test that drives the click sequence of a
drawing tool (`handleClick` → store) or the typed-dimension entry
(`manualEntry.ts`, `triggerDimensionEdit.ts`, `useCreationWorkflow.ts`). Those are
covered, if at all, by the Playwright suite under `e2e/`, which I did not read. The
phase 5 red tests for the interaction state machines will have to be written from the
behaviour described in section 2, not ported.

---

## 12. What does not exist (consolidated)

Each of these needs a second reference or an original design if phase 5 wants it.

Drawing tools:

- a dedicated **line** tool and a dedicated **arc** tool;
- arc by centre-start-end, tangent arc, arc by start-end-radius clicks;
- circle by 2 points (diameter) or 3 points;
- rectangle from centre, rotated/3-point rectangle;
- star, polygon by inscribed radius (across flats);
- ellipse arc; exact ellipse (it is four Béziers);
- pen tool with click-drag handles; freehand; B-spline / control-point spline;
- point entity;
- absolute coordinate entry for any point, relative `@dx,dy` entry, expressions or
  fractions in fields;
- press-drag drawing.

Text:

- user fonts (TTF/OTF/system), kerning, letter-spacing control, multi-line, alignment,
  text box, true bold/italic faces, lower case in the stroke font, Hershey fonts.

Node editing:

- convert segment (line ↔ arc ↔ curve);
- node smooth/corner/symmetric toggle (Bézier handles are always kept collinear);
- drag an arc or curve segment as a whole;
- fillet or chamfer on anything but a line-line corner; editable/removable fillets;
  two-distance or angle chamfers; per-corner values on the canned rectangles;
- multi-node selection and move; node alignment; editing more than one feature at once;
- interior trim; trim/extend on closed shapes or Béziers.

Snapping:

- tangent, quadrant, extension/alignment tracking, polar/angle increments, parallel;
- guides (draggable or infinite);
- snapping to the generated outline of text and canned shapes (only their frames);
- a per-project snap setting (it is an app preference).

Editing commands:

- boolean intersect and XOR; 2D subtract that consumes the cutter;
- offset of open paths; rounded or bevelled offset corners; offset that keeps Béziers;
- duplicate in place; nudge with arrow keys **[inferred: `useCanvasKeyboard.ts` has no
  arrow-key branch; the only `ArrowLeft` handling under `components/canvas` and
  `components/layout` is in `AppShell.tsx` (lines 545-734), which I did not read and
  take to be menu/toolbar navigation]**;
- scale/rotate handles on a bounding box;
- align to stock, centre in stock, align to the first-selected object;
- persistent (re-editable) arrays; nested folders; a group entity separate from folders;
- OS clipboard interchange.

Backdrop and measurement:

- bitmap tracing; more than one backdrop; calibration by typed distance; backdrop
  mirror; measuring length/area of an object.

Parametrics:

- geometric constraints other than fixed distance (horizontal, vertical, tangent,
  equal, concentric, symmetric are type names only);
- a general constraint solver.
