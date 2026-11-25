# PureCutCNC material-removal simulation: digest for the DreamCarve phase 6 spec

Source: `/Users/markg/orca/workspaces/dreamcarve/purecutcnc`, HEAD `d04ae2f`.
All paths are relative to that root. Line numbers are from that commit.

Conventions used here:

- **[read]** is the default: the claim comes from the file and lines cited.
- **[inferred]** marks anything deduced rather than read. Treat it as a lead, not a fact.
- "Y-down" always means PureCutCNC's internal convention (`ARCHITECTURE.md` §8, lines 107-110).
- "Sheet" means the stock's XY plane as the grid sees it. "World" means the Three.js scene.
- Scope: flat rectangular stock, 3-axis, top face. Rotary, bar stock, plasma and the Bottom
  face are named in section 15 and not digested.

Things the spec author should know before reading on:

1. **The simulation is a removal viewer and nothing else.** There is no rest-material
   highlight, no gouge or overcut detection, no rapid or holder collision check, no
   comparison against a target model, no colouring by operation, tool or depth (only a
   fixed 12 % depth darkening), no machining time, no measuring and no export of the
   result. Section 11 lists the searches behind each "not present".
2. **Only four milling cutter shapes exist, and two of the wanted ones do not.**
   `ToolType` is `'flat_endmill' | 'ball_endmill' | 'v_bit' | 'drill' | 'plasma'`
   (`src/types/project.ts:703`). There is **no tapered ball nose**. A V-bit is a **pointed
   cone only**: `Tool` has `vBitAngle` and no tip-diameter field (`project.ts:705-727`).
   A **drill is simulated as a flat-bottomed cylinder**, identical to a flat end mill
   (`src/engine/simulation/tools.ts:236-237`); there is no point angle anywhere.
3. **The flat kernel is exact only for constant-Z moves and vertical plunges.** For a move
   whose Z changes it takes the tool's Z at the point of the segment *nearest the cell*,
   not the lowest Z of any pose whose footprint covers the cell
   (`replay.ts:150-167`). **[inferred]** For a flat cutter of radius `r` on a ramp of
   slope `s = |dz| / xy-length` this leaves a cell up to `s · r` too high. The rotary
   kernel has a sweep for exactly this (issue #1048); the flat kernel does not.
4. **Playback and the static result can differ on ramps.** A part-played ramp is applied
   as shorter pieces, each using Z at the nearest point of the *piece*, so it "leaves
   heights a little below a whole-move replay"
   (`planning/SIMULATION_GPU_HEIGHTFIELD_DESIGN.md:131-134`;
   `winnerShading.test.ts:211-214`). **[inferred]** The amount depends on the per-frame
   step, which sits beside the plan's own invariant "Playback correctness must not depend
   on frame rate" (plan line 234).
5. **"Selected" shows one operation on fresh stock.** The static Selected result replays
   only the selected operation into an uncut grid (`src/app/simulationCompute.ts:241-245`).
   Pressing **Play tool** on the same operation starts from the stock left by the
   operations before it (`simulationCompute.ts:304-337`). The two views of one selection
   therefore show different stock.
6. **Playback is one operation at a time, and never a drill.** It exists only in Selected
   mode (`src/app/useSimulationModel.ts:389`) and returns no playback input when the
   tool type is `'drill'` (`useSimulationModel.ts:398-400`). There is no whole-job
   playback and no step-by-move, step-by-level or step-by-operation control.
7. **Rapids are skipped entirely.** Only `cut`, `plunge`, `lead_in` and `lead_out` moves
   touch the grid (`replay.ts:38-40`). A rapid through material removes nothing and
   reports nothing.
8. **Toolpath moves are straight segments.** There is no arc move at this level; arcs are
   fitted from chords at G-code export (`src/engine/gcode/arcFitting.ts:17-41`).
9. **`planning/SIMULATION_GPU_HEIGHTFIELD_DESIGN.md` is current authority**
   (`status: current`, `last-verified: 2026-11-13`, lines 1-5; listed in
   `planning/INDEX.md:40`). It covers rendering and playback updates. Four older
   simulation plans sit in `planning/archive/` and are not authority
   (`planning/INDEX.md:22-25`); I did not read them.
10. **The UI is small.** Mode (Selected / Visible), a Detail slider, a Play tool toggle,
    a camera view menu, and a playback bar (play/pause, stop, scrubber, speed, step,
    XYZ and feed readout). There is no toolpath overlay on the simulated stock, no
    show/hide-tool switch and no on-screen statistics. `SimulationStats` reaches the DOM
    only as `data-` attributes for the browser tests
    (`src/components/simulation/SimulationViewport.tsx:1350-1351`).
11. **Rendering is the large part.** `src/engine/simulation/` is 15,705 lines with its
    tests. **[inferred]** The flat grid, cutter profile, flat kernel, sequencing and
    playback come to roughly 1,500 lines of source. The rest is the GPU heightfield, the
    two visual refinements (issues #973 and #1209), rotary and tests.

Files I did not read in full, only the parts cited:

- `src/engine/simulation/replay.ts`: lines 1-560 and 1080-1581 read; 560-1080 (the wrapped
  kernel and its sweep schedule) skipped.
- `src/engine/simulation/tools.ts`: read in full, but `solveRayInterval` (61-176) is
  rotary only and not digested.
- `src/app/simulationSetup.ts`: lines 140-162, 258-280, 352-376 only.
- `src/engine/simulation/heightfieldShader.test.ts`, `outlineCorners.test.ts`,
  `instancedBoundary.test.ts`: case names and the assertions quoted in section 12 only.
- `e2e/viewportViews.smoke.spec.ts`: test titles and the two fixture snippets cited.
- `src/components/viewport3d/ViewPresetMenu.tsx`: structure only.
- `src/engine/toolpaths/clamps.ts`, `src/engine/operationBooklet/report.ts`,
  `src/engine/modelExport/stl.ts`, `src/engine/toolpaths/generateOperation.ts`: headers
  and the lines cited.
- Not read at all: every file listed in section 15, `planning/archive/*`,
  `src/app/simulation*.test.ts`, `src/app/useSimulationModel.test.ts`,
  `src/engine/simulation/glslScalar.testSupport.ts`.

---

## 1. Module map

`src/engine/simulation/INDEX.md` (lines 3-7) states the design: one heightfield, one
height per cell in `topZ`, rendered by a GPU heightfield mesh. "Pure logic + Three.js mesh
builders — no React."

| File | Lines | Role on the flat path |
|---|---|---|
| `types.ts` | 160 | `SimulationGrid`, `DirtyRegion`, `SimulationBuildOptions`, `SimulationStats`, `SimulationResult`, `SimulationReplayItem` |
| `grid.ts` | 333 | cell size and `createSimulationGrid` |
| `tools.ts` | 239 | `cutterSurfaceZ` (203-239); the rest is rotary |
| `replay.ts` | 1581 | `applyMoveKernel` (58-259), `applyMoveToGrid` (1096-1141), stats, static replay, chunked replay |
| `playback.ts` | 562 | `PlaybackController`, `cloneSimulationGrid`, `subdivideMoves` |
| `stepThreshold.ts` | 32 | `SHALLOW_SLOPE_CELLS = 0.5` |
| `edgeCrossings.ts` | 444 | issue #973 CPU side |
| `moveTable.ts` | 149 | issue #1209 CPU side |
| `gpuMesh.ts` | 167 | height texture, top-surface geometry, dirty-rectangle upload |
| `instancedBoundary.ts` | 648 | walls and underside |
| `heightfieldShader.ts` | 829 | surface material and all shared shader source |
| `outlineCorners.ts` | 262 | issue #973 GPU pass |
| `winnerShader.ts`, `winnerShading.ts` | 200, 144 | issue #1209 GPU side |
| `simulationScene.ts` | 129 | `buildSimulationStockScene`, the one scene builder |
| `toolMesh.ts` | 158 | the moving cutter mesh |
| `index.ts` | 34 | barrel |

App layer: `src/app/simulationCompute.ts` (372, pure), `src/app/useSimulationModel.ts`
(532, React hook), `src/app/simulationBuild.ts` (60), `src/app/simulationReuse.ts` (195),
`src/app/simulationSetup.ts` (377). UI: `src/components/simulation/SimulationViewport.tsx`
(1650), `SimulationSetupInfo.tsx` (57). Camera: `src/components/viewport3d/orbitControls.ts`
(467), `viewPresets.ts` (153), `ViewPresetMenu.tsx` (128).

---

## 2. Grid

### 2.1 Types — `src/engine/simulation/types.ts`

```ts
// 71-112 (comments trimmed)
export interface SimulationGrid {
  originX: number
  originY: number
  cellSize: number
  cols: number
  rows: number
  stockBottomZ: number
  stockTopZ: number
  wrap?: { axis: 'x' | 'y'; period: number }      // rotary only
  initialTopZ?: Float32Array                      // bar stock only
  topZ: Float32Array
  edgeCrossings?: Uint8Array                      // issue #973, 4 bytes per cell
  winners?: Uint32Array                           // issue #1209, 1 per cell
  moveTable?: import('./moveTable').SimulationMoveTable
}

// 114-119
export interface DirtyRegion { colMin: number; colMax: number; rowMin: number; rowMax: number }

// 121-137 (bar-only options trimmed)
export interface SimulationBuildOptions {
  wrap?: SimulationGrid['wrap']
  targetLongAxisCells?: number
  squareBar?: SquareBarDescriptor
  flatRoundBar?: FlatRoundBarDescriptor
}

// 139-144
export interface SimulationStats {
  removedCellCount: number
  minTopZ: number
  maxRemovedDepth: number
  processedMoveCount: number
}

// 146-150
export interface SimulationResult {
  grid: SimulationGrid
  stats: SimulationStats
  warnings: ToolpathWarning[]
}

// 152-160
export interface SimulationReplayItem {
  operationId: string
  operationName: string
  toolRef: string | null
  toolType: ToolType
  toolRadius: number
  vBitAngle: number | null
  toolpath: import('../toolpaths/types').ToolpathResult
}
```

`DirtyRegion` bounds are **inclusive** on both ends (`replay.ts:187-190`,
`gpuMesh.ts:152-153`).

### 2.2 Resolution — `grid.ts`

```ts
// 23-24
const DEFAULT_LONG_AXIS_CELLS = 180
const MIN_AXIS_CELLS = 24
```

`resolveSimulationGridSpec(project, options)` (70-111):

- `stockBounds = getStockBounds(project.stock)`, the 2D bounds of the stock profile
  (`project.ts:2101-2103`).
- `width = max(maxX - minX, 1e-6)`, `height = max(maxY - minY, 1e-6)` (75-76).
- `longAxisCells = max(24, round(options.targetLongAxisCells ?? 180))` (77).
- `cellSize = max(width, height) / longAxisCells` (78-79). **Cells are square.** The
  setting is a cell count along the longer stock side, not a size in mm.
- `cols = max(24, ceil(width / cellSize))`, `rows = max(24, ceil(height / cellSize))`
  (81-82). **[inferred]** Two consequences of the `ceil` and the floor, neither masked
  for a rectangular stock, whose cells all start at `stockTopZ` (279): the short axis can
  overhang the stock's max edge by part of a cell; and when the short side is less than
  24 cells the grid is still 24 cells across, so it extends past the stock. Worked
  example: a 1000 × 50 stock at detail 280 has `cellSize` 3.571, `ceil(50 / 3.571) = 14`,
  so `rows = 24` and the grid is 85.7 deep, 35.7 beyond the stock. That happens whenever
  the stock's aspect ratio exceeds `detail / 24` (11.7 : 1 at the default). I did not run
  it; no test covers it (section 12.1).
- `originX = stockBounds.minX`, `originY = stockBounds.minY`, `stockBottomZ = 0`,
  `stockTopZ = project.stock.thickness` (102-110, the non-bar branch).

There is **no cell cap, memory budget or adaptive limit in the engine**. The only upper
bound is the UI slider (section 10.2): 240 to 1500 cells, default 280. The plan states the
intent without a number: "Detail controls remain bounded by memory, upload, and rendering
cost; higher nominal resolution is not automatically safe on every device" (plan 232-233).

**[inferred]** Memory per flat grid is 12 bytes per cell (`topZ` 4, `edgeCrossings` 4,
`winners` 4). A square stock at detail 1500 is 2.25 M cells, so 27 MB per grid; the code's
own figure for `topZ` alone is "9 MB at detail 1500" (`gpuMesh.ts:100`). Playback holds a
base grid and a live clone, and the panel's replay cache keeps up to 6 results
(`simulationReuse.ts:128`).

### 2.3 Cell values

`createSimulationGrid(project, options)` (238-309), block branch:

- `topZ = new Float32Array(cols * rows)` filled with `stockTopZ` (244, 279). Heights are
  **32-bit floats**; several comparisons round with `Math.fround` because of it
  (`replay.ts:192`, `222`, `1247-1250`).
- If the stock has a non-rectangular profile from a source feature
  (`project.stock.sourceFeatureId`), each cell whose centre is outside the sampled profile
  polygon is set to **literal `0`** (285-300), by an even-odd point-in-polygon test
  (113-123). `sampleProfilePoints` is used so arcs and circles are tessellated (issue
  #644, comment 281-284).
- **"Empty" is `topZ <= stockBottomZ`.** There is no separate mask or sentinel. A cell cut
  through and a cell outside the stock profile are the same value. Shaders test
  `height <= stockBottomZ + 0.000001` (`heightfieldShader.ts:75`, `732`).
- A block grid gets `edgeCrossings = new Uint8Array(cols * rows * 4)` (331-333) and
  `winners` filled with `WINNER_NONE`, except cells already at or below the bottom, which
  are `WINNER_UNKNOWN` (317-328).
- `initialTopZ` is **not** set for a block, masked or not (302-308).

The cutter never reads `stockTopZ` (types.ts:79-83). It is the uniform starting height, the
reference for statistics, the top of the render bounds and the reference for depth
darkening.

### 2.4 Coordinates and units

- Cell `(col, row)` has centre `x = originX + (col + 0.5) * cellSize`,
  `y = originY + (row + 0.5) * cellSize` (`replay.ts:145-148`).
- Index is `row * cols + col`, row-major (`replay.ts:146`, `181`).
- Point to cell: `floor((v - origin) / cellSize)` clamped to `[0, count - 1]`
  (`replay.ts:32-36`).
- **No flip happens anywhere in the simulation.** Column index grows with sheet X, row
  index grows with sheet Y, and sheet Y is PureCutCNC's Y-down coordinate. The grid takes
  toolpath X/Y as they are.
- The renderer maps sheet `(x, y, z)` to world `(x, z, y)`: world Y is height, world Z is
  sheet Y (`heightfieldShader.ts:671-680`; `SimulationViewport.tsx:871-873`, "Toolpath
  (x, y, z) → world (x, z, y)"). **[inferred]** With the Top preset's
  `up = [0, 0, -1]` (`viewPresets.ts:60-64`) sheet +Y runs down the screen, matching the
  sketch, and world `(X, up, Z)` is a right-handed, unmirrored picture of the machine with
  world Z = minus machine Y.
- Z is up, `0` at the stock bottom, `stock.thickness` at the top (`grid.ts:108-109`).
  Toolpath Z is in the same frame; the kernel compares it to `topZ` directly.
- Everything is in **project units**, mm or inch. Tools are converted to project units by
  `normalizeToolForProject` (`src/engine/toolpaths/geometry.ts:158-189`), which also gives
  `radius = diameter / 2`.
- The one Y inversion in the feature is the playback readout: `y: origin.y - pose.y`
  (`SimulationViewport.tsx:1103-1109`), turning sheet Y into machine Y for display.

---

## 3. Cutter profiles — `src/engine/simulation/tools.ts`

```ts
// 203-239
export function cutterSurfaceZ(
  toolType: ToolType,
  toolRadius: number,
  toolCenterZ: number,
  radialDistance: number,
  vBitAngle: number | null = null,
): number | null {
  if (radialDistance > toolRadius + 1e-9) {
    return null
  }

  switch (toolType) {
    case 'plasma':
      return null
    case 'flat_endmill':
      return toolCenterZ
    case 'ball_endmill': {
      const clampedDistance = Math.max(0, Math.min(toolRadius, radialDistance))
      const dz = toolRadius - Math.sqrt(Math.max(0, toolRadius * toolRadius - clampedDistance * clampedDistance))
      return toolCenterZ + dz
    }
    case 'v_bit': {
      const includedAngle = Math.max(1, Math.min(179, vBitAngle ?? 60))
      const halfAngleRadians = (includedAngle * Math.PI) / 360
      const slope = Math.tan(halfAngleRadians)
      if (slope <= 1e-9) {
        return toolCenterZ
      }
      return toolCenterZ + radialDistance / slope
    }
    case 'drill':
      return toolCenterZ
  }
}
```

`toolCenterZ` is the **tool tip Z**, the Z the toolpath carries. With `d` the radial
distance from the axis, `r` the tool radius and `zt` the tip Z:

| Tool | Lower surface at `d` | Fields read |
|---|---|---|
| `flat_endmill` | `zt` | radius |
| `drill` | `zt` (same as flat) | radius |
| `ball_endmill` | `zt + r - sqrt(r² - d²)` | radius |
| `v_bit` | `zt + d / tan(angle / 2)`; `angle` is the included angle in degrees, clamped to `[1, 179]`, default 60 when null | radius, `vBitAngle` |
| `plasma` | `null` here; the kernel cuts a full-depth kerf instead | out of scope |

- Beyond `d > r + 1e-9` every profile returns `null`: nothing above the cutting diameter is
  modelled. There is no shank, holder or flute-length limit in the flat path.
- The V-bit cone runs from a point at the tip to the tool radius. `toolRadius` for a V-bit
  is half the tool's `diameter` (`geometry.ts:168`).
- `vBitAngle` is forced to `null` for every non-V tool and to `?? 60` for a V-bit when the
  tool is normalised (`geometry.ts:173`).
- **Unsupported tools:** there is no "unsupported" path. The `switch` has no `default`;
  `ToolType` has five members and all five are listed. A tool that is missing or
  incompatible with the operation never reaches this function (section 7.2).
- `cutterSurfaceZ` is **not called by the kernel**. The kernel inlines the same formulas
  (section 5.2); `replay.test.ts` holds the two together. The function is the readable
  reference and the test oracle.

---

## 4. Input: toolpath moves — `src/engine/toolpaths/types.ts`

```ts
// 22
export type ToolpathMoveKind = 'rapid' | 'plunge' | 'cut' | 'lead_in' | 'lead_out'

// 24-31
export interface ToolpathPoint {
  x: number
  y: number
  z: number
  a?: number            // rotary angle in degrees; absent on every flat job
}

// 33-49 (comments trimmed)
export interface ToolpathMove {
  kind: ToolpathMoveKind
  from: ToolpathPoint
  to: ToolpathPoint
  source?: string       // debug provenance tag
  feedScale?: number    // multiplier on the cut feed; absent means 1; never on a plunge
}

// 71-88 (comments trimmed)
export interface ToolpathResult {
  operationId: string
  moves: ToolpathMove[]
  warnings: ToolpathWarning[]
  bounds: ToolpathBounds | null
  collidingClampIds?: string[]
  collidingMoveIndices?: number[]
  debugToolpath?: boolean
  drillCycles?: DrillCycle[]
}
```

- Every move is a **straight segment** carrying both endpoints. There is no arc kind.
  Curves arrive as chords. Arc fitting is an export-stage step over constant-Z `cut`,
  `lead_in` and `lead_out` runs (`src/engine/gcode/arcFitting.ts:17-41`).
- The moves come from `computeOperationToolpath(project, operation, options)`
  (`src/engine/toolpaths/generateOperation.ts:205-209`), whose result is "always in stock
  space" (198-203). The simulation consumes the same optimised result as preview and
  export: `types.ts:101-102` calls it "the canonical result that caching, preview,
  simulation, and export consume", after adjacent collinear cut moves are merged.
- The simulation reads `kind`, `from`, `to` and, in playback only, `feedScale`. It ignores
  `drillCycles`: a drilling operation is replayed from its expanded `plunge` moves.
- **Tabs are not a simulation concept.** `replay.ts` and `playback.ts` contain no tab
  logic; a tab exists in the simulation only because the generated path rises over it.
  `tabbedEdgeSimulation.test.ts` checks that end to end (section 12).

---

## 5. The flat-sheet cut kernel — `src/engine/simulation/replay.ts`

### 5.1 Entry point and dispatch

```ts
// 38-40
export function moveIsMaterialRemoving(move: ToolpathMove): boolean {
  return move.kind === 'cut' || move.kind === 'plunge' || move.kind === 'lead_in' || move.kind === 'lead_out'
}

// 42-45
export interface ApplyMoveResult {
  changedCount: number
  dirtyRegion: DirtyRegion | null
}

// 1096-1105
export function applyMoveToGrid(
  grid: SimulationGrid,
  move: ToolpathMove,
  toolRadius: number,
  toolType: SimulationReplayItem['toolType'],
  vBitAngle: number | null,
  sweep: ToolpathMove = move,
  elapsed: number = 1,
  winnerIndex?: number,
): ApplyMoveResult
```

For a grid with no `wrap`, `applyMoveToGrid` calls
`applyMoveKernel(grid, move, toolRadius, toolType, vBitAngle, winnerIndex)` and returns its
result (1112-1113). **`sweep` and `elapsed` are not passed to the flat kernel**; they exist
for the rotary kernel. `applyMoveToGrid` itself does not check the move kind; the callers
filter with `moveIsMaterialRemoving` (1375, 1498) or `isCuttingMove` (`playback.ts:106-108`).

### 5.2 Per-move algorithm — `applyMoveKernel` (58-259)

1. **Bounding box.** XY bounds of the two endpoints grown by `toolRadius` (66-69), turned
   into inclusive cell ranges and clamped to the grid (71-72). There is no test for a
   move wholly outside the grid or wholly above the stock; a move outside the grid scans
   the clamped border cells and rejects each on distance. **[inferred]**
2. **Per-move constants** (79-103): `segD{x,y,z}`, `segLengthSq = dx² + dy²` (XY only),
   `xyStationary = segLengthSq <= 1e-12`, `stationaryZ = min(from.z, to.z)`,
   `radiusSqWithTolerance = (toolRadius + 1e-9)²`, and for a V-bit
   `vBitInvSlope = 1 / tan(angle / 2)` (1291-1295). The cutter shape is chosen once per
   move, not per cell (90-103).
3. **Per cell** in the box (144-238), with `(x, y)` the cell centre:
   - `t = clamp(((x - x0) * dx + (y - y0) * dy) / segLengthSq, 0, 1)`, or `0` when the move
     is XY-stationary (150-157).
   - `distanceSq` = squared XY distance from the cell centre to the segment point at `t`
     (158-160). If `distanceSq > radiusSqWithTolerance`, skip the cell (161-163). This is
     the **exact closest point on the segment**, so the footprint of one move is an exact
     capsule (stadium): no sampling along the move, no gaps at any step size.
   - `toolCenterZ = xyStationary ? stationaryZ : z0 + segDz * t` (165-167).
   - `cutZ`: flat and drill `toolCenterZ`; ball
     `toolCenterZ + r - sqrt(r² - min(r², distanceSq))`; V-bit
     `toolCenterZ + sqrt(distanceSq) * vBitInvSlope` (169-179). Flat cutters never take a
     square root (comment 90-93).
   - `nextZ = max(cutZ, stockBottomZ)` (182). **Nothing is ever written below the stock
     bottom.**
   - `lowered = nextZ < topZ[idx] - 1e-9` (183). If so write `topZ[idx] = nextZ`, count it,
     grow the dirty rectangle, and set `winners[idx]` (184-191). **Cells only ever go
     down.**
   - Otherwise the winner tie rule and the crossing stamp may still run (192-236; sections
     9.9 and 9.10).
4. **Result** (240-258): `changedCount` and the tight inclusive rectangle of cells that
   changed, widened to cover any crossing or winner change, or `null` when nothing
   changed.

### 5.3 What "Z-changing moves" get

- **Vertical plunge** (XY-stationary): a disc at the lower of the two Z values (88, 167).
  Exact.
- **Constant-Z move:** exact capsule at that Z.
- **Ramp or helix chord:** each cell gets the Z at its own nearest point on the segment
  (167). Beyond either end the Z is that endpoint's (the clamp at 155-156).
  **[inferred]** This is not the lower envelope of the swept cutter. For a flat cutter the
  true height at a cell at lateral distance `d` is the tool Z at the lowest pose still
  covering the cell, `sqrt(r² - d²)` further along the path than the nearest point, so the
  kernel is high by up to `slope · sqrt(r² - d²)`, at most `slope · r`. A ball is
  affected by less (on the path axis about `r · slope² / 2`), and a V-bit is exact on the
  axis while the ramp is shallower than the cone's flank. Cells never end too low.
- The source calls the flat kernel exact: "a flat grid is already exact: both keep the
  original kernel" (1106-1107) and "Flat (non-wrapped) grids never call this:
  `cutterSurfaceZ` is exact there" (`tools.ts:58-59`). **[inferred]** Both are said while
  contrasting it with the rotary solid, and are true of the profile at one pose, not of
  the sweep along a ramp. The sweep helpers (`wrappedNeedsSweep`, 421-431, and the
  schedule after it) are applied to wrapped grids only.

### 5.4 Rapids

Rapids are not applied (38-40; 1375-1377; 1498-1500; `playback.ts:421`, `452`, `479`). A
rapid that passes through uncut stock leaves the grid unchanged and produces no warning.
See section 11(c).

### 5.5 Statistics — `computeStats` (1143-1181)

For a grid without `initialTopZ` (every block):

- `removedCellCount` = cells with `z < stockTopZ - 1e-9` (1170-1172).
- `minTopZ` = lowest cell height, starting from `stockTopZ` (1165-1169).
- `maxRemovedDepth = max(0, stockTopZ - minTopZ)` (1178).
- `processedMoveCount` = number of material-removing moves applied (1380, 1503).

**[inferred]** For a stock with a non-rectangular profile the masked cells are `0` and
there is no baseline, so they count as removed, `minTopZ` is `0` and `maxRemovedDepth` is
the full thickness before any cut. The "nothing to replay" result avoids this by returning
zeros directly (`simulationCompute.ts:87-96`).

The statistics are counts of cells, not volumes; no volume or mass is computed anywhere.

### 5.6 Static replay

```ts
// 1321-1324
export function replayItemsIntoGrid(
  grid: SimulationGrid,
  items: readonly SimulationReplayItem[],
): { processedMoveCount: number; warnings: ToolpathWarning[] }

// 1386-1390
export function simulateReplayItemsHeightfield(
  project: Project,
  items: SimulationReplayItem[],
  options: SimulationBuildOptions = {},
): SimulationResult

// 1401-1406
export function simulateOperationHeightfield(
  project: Project,
  operation: Operation,
  toolpath: ToolpathResult,
  options: SimulationBuildOptions = {},
): SimulationResult
```

`replayItemsIntoGrid` (1321-1347): reserve move-table room for every cutting move
(1350-1355), then for each item in order, for each move in order, skip non-cutting moves
and call `applyMoveToGrid` (1370-1384). Edge-crossing recording is switched off for the
duration (`DEFERRING_CROSSINGS`, 53, 1328-1337) and done in one pass afterwards by
`CrossingFinalizer` (1338-1345; section 9.9). The `warnings` array it returns is always
empty (1371, 1383).

`simulateOperationHeightfield` builds a fresh grid, resolves the tool with
`findOperationTool`, and returns an uncut grid with the single warning
`{ code: 'replayNoTool' }` when there is none (1409-1414). That is the only warning the
simulation can produce (English: "No tool assigned to the selected operation.",
`src/i18n/locales/en/warnings.ts:300`). **[inferred]** The viewport never reads
`simulation.warnings`, so it is not shown.

### 5.7 Chunked, abortable replay

```ts
// 1451-1457
export interface SimulationChunkOptions {
  signal?: AbortSignal
  yieldToPaint?: () => Promise<void>
  sliceMs?: number
}
const DEFAULT_SLICE_MS = 16

// 1539-1545
export async function simulateOperationHeightfieldChunked(
  project, operation, toolpath, options = {}, chunk = {},
): Promise<SimulationResult | null>

// 1567-1572
export async function simulateReplayItemsHeightfieldChunked(
  project, items, options = {}, chunk = {},
): Promise<SimulationResult | null>
```

`replayItemsIntoGridChunked` (1474-1536) is the same loop on the main thread, cut into
slices:

- It yields once before the first move so the tab and its spinner can paint (1485-1488).
- After each move it reads `Date.now()`; once a slice has run `sliceMs` (16 ms) it awaits
  `yieldToPaint`, by default `setTimeout(…, 0)` (1459-1461, 1504-1508).
- It checks `signal.aborted` before each move and after each yield, and returns `null` on
  abort (1488, 1501, 1506). A cancelled build never returns a partial grid.
- The crossing pass yields the same way but reads the clock only every 256 moves
  (1515-1533).

There is **no worker**: "there is no separate square job and no worker"
(`simulationCompute.ts:20-23`). There is no progress value; the UI shows a spinner only.

---

## 6. How the app drives a build — `src/app/useSimulationModel.ts`

- Nothing is computed unless the centre tab is `'simulation'` (186, 203, 256, 316, 389).
- **Path acquisition** (172-238): the hook asks the generation service for each required
  operation's toolpath with `requestToolpath(operationId, 'simulation', signal)` in an
  effect, and stamps the answer with the project object and the id list. Until that
  lands, `paths` is `null`. An empty requirement is satisfied at once (236-238).
- **Build** (315-374): the input object
  `{ setupInput, project, simulationMode, simulationDetailCells, selectedInSetup, paths }`
  is passed to `computeSimulationResultAsync(input, { signal })` in an effect. A completed
  result is installed only if it was built from the *same input object* that is current
  (`simulationBuild.ts:39-60`); a superseded build is aborted (368-371).
- **Reuse** (277, 292-309, 348, 364-366): finished results are kept in a
  `SimulationReplayCache` of capacity 6, one document at a time
  (`simulationReuse.ts:127-175`). The key is `{ project, setup, detailCells, items }`
  compared by reference (`simulationReuse.ts:77-83`, `115-125`), where `items` is the list
  of `{ operationId, toolpath }` actually replayed, in cut order. Any project edit, a
  regenerated path or another detail value misses.
- `simulationInputPending` is true while the tab is open and no result exists for the
  current input (378); the viewport shows a spinner for it (`App.tsx:450`).
- There is **no debounce** on the Detail slider in the hook or in `App.tsx`
  (`App.tsx:71`, `449`). **[inferred]** The "debounced 150ms" remark at
  `SimulationViewport.tsx:1295` is stale; each slider value starts a build and aborts the
  previous one.

---

## 7. Operation sequencing

### 7.1 Order

"Cut order" is the order of `project.operations`, filtered to the simulated setup
(`simulationSetup.ts:364-376`). A single-setup project uses every operation.

### 7.2 Which operations are replayed — `src/app/simulationCompute.ts`

```ts
// 184-194
export function visibleReplayOperations(project: Project, operations: readonly Operation[]): Operation[] {
  return operations.filter((operation) =>
    operation.enabled
    && operation.showToolpath
    && operation.toolRef
    && findOperationTool(project, operation) !== null,
  )
}
```

| Mode | What is replayed | Lines |
|---|---|---|
| Selected | the selected operation alone, into a fresh grid; no `enabled` or `showToolpath` filter | 155-173, 241-245 |
| Visible | every operation passing `visibleReplayOperations`, in cut order, into one fresh grid | 197-223, 247-252 |
| Playback base | operations **before** the selected one that are `enabled && showToolpath && toolRef`, with a resolvable tool and an acquired path | 304-337 |

- **Disabled or hidden operations** are left out of Visible and of the playback base.
  Selected mode applies no `enabled` or `showToolpath` filter of its own. **[inferred]**
  Whether a disabled operation has a path to replay depends on the generation side, which
  I did not read.
- **Operations with errors:** an operation with no toolpath in the map, or no compatible
  tool, is dropped silently (207-209, 322-324). A toolpath that exists but has no moves
  replays nothing. Selected mode with nothing to draw returns the uncut blank
  (243, 145-152).
- **Tool per operation:** `findOperationTool(project, operation)` looks the tool up by
  `operation.toolRef` and returns `null` unless it is compatible with the operation kind
  (`src/toolPolicy.ts:30-33`). `normalizeToolForProject` then gives `radius` and
  `vBitAngle` in project units (163-171, 211-219).
- **One grid for all operations.** There is no per-operation grid or snapshot in a
  Visible replay; the items are applied in sequence to a single `topZ`. The only
  "per-operation base grid" is playback's: the prior operations replayed into a fresh
  grid by `priorReplayResult` (349-361), built lazily on the first Play tool click
  (`useSimulationModel.ts:439-449`) and shared through the replay cache with the Selected
  result of the same work (424-438).
- Operations after the selected one never appear in playback (comment 295-302).
- `simulationOperationCount` is computed (`useSimulationModel.ts:380-386`) and passed to
  the viewport as `operationCount`, but the viewport does not destructure or show it
  (`SimulationViewport.tsx:301-323`). The same holds for the `operation` and
  `stockHasProfile` props. **[inferred]** They are leftovers.

### 7.3 Setups

Simulation runs one machining setup at a time, each from fresh stock (plan 224-230). For
a project with more than one setup the toolbar shows an info button with the sentence
"{setup} is simulated on fresh stock: material removed by other setups is not shown."
(`src/i18n/locales/en/viewport.ts:47`; `SimulationSetupInfo.tsx:36-57`). A single-setup
project shows nothing (`useSimulationModel.ts:506-510`).

---

## 8. Playback — `src/engine/simulation/playback.ts`

### 8.1 Public API

```ts
// 24-28
export interface PlaybackToolInfo { toolType: ToolType; toolRadius: number; vBitAngle: number | null }

// 30-53 (comments trimmed)
export interface PlaybackOptions {
  maxSegmentLength?: number          // 0 or negative disables subdivision
  referenceFeedPerSecond?: number    // project units per second; omit for constant speed
  plungeFeedPerSecond?: number
}

// 55-66
export interface PlaybackPose {
  x: number
  y: number
  z: number
  moveKind: ToolpathMove['kind'] | null
  feedScale: number | undefined
  a?: number
}

// 75
export function cloneSimulationGrid(source: SimulationGrid): SimulationGrid
// 181
export function subdivideMoves(moves: ToolpathMove[], maxSegmentLength: number): ToolpathMove[]

// 185-562
export class PlaybackController {
  readonly baseGrid: SimulationGrid
  readonly liveGrid: SimulationGrid
  readonly moves: ToolpathMove[]          // the subdivided pieces
  readonly tool: PlaybackToolInfo
  readonly totalPathLength: number
  constructor(baseGrid, moves, tool, options: PlaybackOptions = {})
  getWinnerProgress(): { index: number; fraction: number }
  reset(): void
  getDirtyRegion(): DirtyRegion | null
  clearDirtyRegion(): void
  isFinished(): boolean
  getDistanceTraveled(): number
  getMoveIndex(): number
  getMoveCount(): number
  getPose(): PlaybackPose
  advance(budget: number): boolean
  seekToDistance(target: number): boolean
  seekToFraction(fraction: number): boolean
}
```

### 8.2 State

`moveIndex`, `moveFraction` (0..1 within the current piece), `distanceTraveled`,
`finished`, `lastMoveApplied`, `frameDirtyRegion` (200-205). There is no "playing" flag in
the controller: **play/pause lives in the viewport**, which calls `advance` each animation
frame while playing (section 10.4). The controller is a pure cursor over a path plus a
grid. An empty path is finished from construction (231-233).

### 8.3 Pieces (subdivision)

- `subdivideSegments` (148-178) splits any move longer than `maxSegmentLength` (3D length,
  85-90) into `ceil(length / max)` equal pieces, each a copy of the source move with
  interpolated endpoints so `kind`, `feedScale` and `source` carry over (164-172).
- Default `maxSegmentLength = max(toolRadius * 1.5, 0.5)` (217). The app passes
  `toolRadius * 2` (`useSimulationModel.ts:459-467`), with the measured reason: at
  `0.4 × radius` about 5/6 of cell tests were repeats of shared end caps; `2 × radius`
  cut that about four times.
- The stated purpose is to keep each move's cell bounding box close to the swept path;
  "Correctness doesn't depend on the length" (`useSimulationModel.ts:459-462`). See the
  ramp caveat in item 4 at the top.
- All moves are kept, rapids included. `totalPathLength` is the sum of the 3D lengths of
  **every** piece (223), so progress includes rapid travel.

### 8.4 Grid cloning and reset

`cloneSimulationGrid` (75-83) copies `topZ`, `edgeCrossings` and `winners`; every other
field, including the move table, is shared by reference. The constructor clones the base
grid into `liveGrid` and never writes `baseGrid` (215-216).

`reset()` (301-321) copies base `topZ`, winners and crossings back into the live arrays,
zeroes the cursor, and sets the dirty region to the **whole grid**, because restoring can
raise cells, which per-move tracking never reports (312-320).

### 8.5 Advancing — `step(amount, feedScaled)` (408-512)

Loop while budget remains and the path is not finished:

- **Zero-length piece** (`length <= 1e-9`): apply it once if it is a cutting move, then go
  to the next piece without using budget (420-442).
- `ratio = feedScaled ? feedRatioForMove(move) : 1` (444).
- `budgetToFinish = (length - length * moveFraction) / ratio` (445-449).
- **Enough budget to finish the piece** (451-473): if it is a cutting move and has not
  been applied as a whole yet (`lastMoveApplied !== moveIndex`), apply the **whole piece**
  to the live grid; add the remaining length to `distanceTraveled`; subtract the budget;
  go to the next piece.
- **Not enough** (474-508): advance `moveFraction` by `remaining * ratio / length`; if it
  is a cutting move, apply a **partial move** from the previous fraction to the new one
  (480-496); stop.
- Every application passes the piece's move-table index (`winnerIndexAt`, 284-286) and
  merges the returned dirty region into `frameDirtyRegion` (434-437, 336-345).
- Returns `true` if any cell, winner or crossing changed.

`advance(budget)` is `step(budget, true)` (396-398).

### 8.6 Feed, speed and progress

```ts
// src/engine/toolpaths/feed.ts:37-47
export function effectiveFeed(moveKind: FedMoveKind, feedScale: number | undefined, cutFeed: number, plungeFeed: number): number {
  if (moveKind === 'plunge') return plungeFeed
  return cutFeed * (feedScale ?? 1)
}
```

`feedRatioForMove` (244-255):

- `1` when no reference feed was given, for any **rapid**, and for a plunge when no plunge
  feed was given.
- Otherwise `max(effectiveFeed(kind, feedScale, reference, plunge) / reference, 1e-3)`.

So the budget is a distance at the reference (cut) feed. A cut at `feedScale 0.1` covers a
tenth of the budget; a plunge covers `plungeFeed / cutFeed` of it; **a rapid moves at the
cut feed**, not at a machine rapid rate. There is no clock in the controller: time enters
only as the `speed × dt` the caller multiplies out.

Progress is geometric: `distanceTraveled / totalPathLength`
(`SimulationViewport.tsx:1073-1075`). It is a fraction of path length, not of time.

### 8.7 Seeking

`seekToDistance(target)` (527-547), with `target` clamped to `[0, totalPathLength]`:

- **Forward** (`delta > 0`, not finished): `step(delta, false)`, so geometric, feed
  ignored, advancing from the current state. Justification in the comment (518-522): cuts
  only lower cells, so advancing from `d0` to `d` equals a reset and replay to `d`.
- **Within `1e-9` behind or at the target:** nothing.
- **Backward:** `reset()`, then `step(target, false)` from zero. Always returns `true`.

`seekToFraction(f)` is `seekToDistance(f * totalPathLength)` (549-551). A forward seek that
lands within a rounding step of the end finishes the path (comment 532-533; INDEX line 16).

There is no stepping API: no "next move", "next level" or "next operation".

### 8.8 Tool pose — `getPose()` (363-382)

- Empty path: `{0, 0, 0, moveKind: null}`.
- Finished: the last piece's `to` point and kind.
- Otherwise: linear interpolation of the current piece at `moveFraction`, with that
  piece's `kind` and `feedScale`.

The pose is the **tool tip**. The viewport places the tool mesh, whose tip is at its local
origin, at world `(pose.x, pose.z, pose.y)` (`SimulationViewport.tsx:873`;
`toolMesh.ts:50-54`).

### 8.9 Dirty region

`getDirtyRegion()` returns the union of every change since the last `clearDirtyRegion()`
(323-334). The caller decides when to upload; the viewport does it once per animation
frame (section 9.8).

---

## 9. Rendering

Everything here is Three.js on WebGL2 with GLSL ES 3.00 shaders
(`glslVersion: THREE.GLSL3`, `heightfieldShader.ts:826`). The technique is described, not
the API.

### 9.1 Scene pieces — `simulationScene.ts:96-129`

`buildSimulationStockScene(grid, stockColor)` builds, for a flat block:

1. a **height texture**: one 32-bit float per cell, `cols × rows`, nearest filtering,
   backed by the grid's own `topZ` array (`gpuMesh.ts:28-44`). Nearest is required because
   many mobile GPUs cannot linearly filter float textures (36-39);
2. the **top surface** (9.2);
3. the **boundary**: two wall meshes and one underside quad (9.3);
4. the **outline corner pass** (9.9) and the **winner shading data** (9.10).

The static view and the playback view both call this one builder
(`SimulationViewport.tsx:791`, `976`). All stock materials are drawn **double-sided**
(`heightfieldShader.ts:827`; `instancedBoundary.ts:550`, `572`), so triangle winding does
not matter.

### 9.2 Top surface — `gpuMesh.ts:52-81`, `heightfieldShader.ts:637-781`

- **One flat quad per cell, four private vertices, no vertex shared between cells.** The
  reason (`gpuMesh.ts:46-49`; plan 25-28): a shared corner samples one neighbouring texel,
  and on a one-cell-wide tab that drags the corners down to cut-through height and paints
  a false sloped hole.
- **Row instancing.** The vertex buffer holds one row: `cols × 4` vertices encoding
  `(column, corner x 0|1, corner y 0|1)` and `cols × 6` indices (16-bit while
  `cols × 4 <= 65535`). It is drawn `rows` times; the instance index is the row
  (`gpuMesh.ts:54-70`). Template memory is O(cols) at any detail.
- **Vertex stage** (661-694): fetch the cell's height by integer texel fetch, place the
  corner at `(originX + (col + cx) * cellSize, height, originY + (row + cy) * cellSize)`,
  moved by the corner offset when #973 is on (667-675).
- **Fragment stage** (731-780): discard when the cell's height is at or below the stock
  bottom (760-762); otherwise light with the smoothed surface normal (9.5), multiply by
  the stock colour and the depth darkening (773-779). The winner path (9.10) runs first
  when enabled.
- Bounding box for culling: the full stock box from bottom to `stockTopZ`
  (`gpuMesh.ts:71-74`).

### 9.3 Walls and underside — `instancedBoundary.ts`

- **Walls.** One quad for every grid edge, interior and perimeter:
  `(cols + 1) * rows + cols * (rows + 1)` (66-72). Two meshes: "vertical" edges between
  horizontally adjacent cells (`cols + 1` quads per row, `rows` instances) and
  "horizontal" edges (`cols` per row, `rows + 1` instances) (610-624). Each is a row-strip
  template instanced per row, for the same reason as the tops; per-edge instancing was
  measured to bottleneck on dispatch (74-80, 419-425).
- The vertex stage (155-295) derives everything from the edge index, the row and the
  height texture: the two cells either side (`nearCell` is the lower-index one), their
  heights, `top = max`, `bottom = max(min, stockBottomZ)` (198-201). A cell outside the
  grid reads as `stockBottomZ` (`heightfieldShader.ts:220-225`), which is what makes the
  stock's own perimeter a wall down to the bottom.
- An edge between equal heights has zero extent and is discarded in the fragment stage
  (342-345).
- Frustum culling is turned off for the walls (615, 623), since real positions come from
  the shader; the template carries a bounding sphere of the whole stock (409-417,
  449-452).
- **Predecessor, for scale:** the CPU-built boundary emitted about 18 vertices × 48 bytes
  per cell, "hundreds of MB of typed arrays at high detail, a multi-second build stall on
  'Play tool', and a hard cell cap" (21-24).
- **Underside.** One quad over the whole grid at `stockBottomZ`, normal straight down
  (455-489). Its fragment stage looks up the cell above each fragment and discards where
  that cell is cut through, so through cuts are holes (388-406).
- **Stock sides** are not a separate mesh: they are the perimeter wall edges.

### 9.4 Step or slope — `heightfieldShader.ts:68-89`, `stepThreshold.ts`

```ts
// stepThreshold.ts:32
export const SHALLOW_SLOPE_CELLS = 0.5
// heightfieldShader.ts:48
export const SLOPE_CONTINUATION_RATIO = 4
```

```glsl
// heightfieldShader.ts:69-88 (template values filled in)
bool edgeIsStep(float hNear, float hFar, float hNearBeyond, float hFarBeyond, float stockBottomZ, float cellSize) {
  float dCenter = hFar - hNear;
  if (dCenter == 0.0) { return false; }
  float cutThroughZ = stockBottomZ + 0.000001;
  if (min(hNear, hFar) <= cutThroughZ) { return true; }
  if (abs(dCenter) <= 0.5000 * cellSize) { return false; }
  float dNear = hNearBeyond > cutThroughZ ? hNear - hNearBeyond : 0.0;
  float dFar = hFarBeyond > cutThroughZ ? hFarBeyond - hFar : 0.0;
  bool slopeContinues =
    (dNear * dCenter > 0.0 && abs(dCenter) <= 4.0 * abs(dNear)) ||
    (dFar * dCenter > 0.0 && abs(dCenter) <= 4.0 * abs(dFar));
  return !slopeContinues;
}
```

In words, for the edge between two neighbouring cells, with the next cell out on each
side:

1. equal heights: nothing to draw;
2. either side cut through: **step**, always, however thin the skin;
3. height change at or below **half a cell**: **slope**, always;
4. otherwise a **slope** if the next edge on either side changes height in the same
   direction and this edge is at most 4 times as steep as it; a cut-through cell beyond
   the edge does not count as a continuation;
5. otherwise a **step**.

Both meshes call this one function, so "wall lit as a wall" and "top stays flat" are one
decision. When they disagreed the surface painted the riser's normal onto the flat top
beside a tab as a dark band (issue #829; plan 32-39). The half-cell rule exists because a
ball finish on large stock is sampled with two or three cells per pass, its gradient
reverses at nearly every edge, and without the rule the surface rendered as flat squares
outlined by wall-lit risers (issue #939; `stepThreshold.ts:17-31`; plan 40-49). Half a
cell covers ball finishing "up to roughly 45 % stepover". A real step that shallow gets
the same treatment and loses its crisp wall lighting.

A **riser inside a slope is still drawn** by the wall mesh but lit with the surface's
normal (`instancedBoundary.ts:234-247`, `332-341`). Leaving it out shows the underside
through a slit; lighting it as a wall draws the cell grid (plan 61-64).

### 9.5 Normals — `heightfieldShader.ts:123-213`, `568-633`

- **Step walls** use the face normal of the wall, pointing toward the lower cell
  (`instancedBoundary.ts:255-267`).
- **Tops and slope risers** use `surfaceNormal(cell, local, wide)` (579-625), computed per
  fragment from a **13-cell stencil**: the cell, two more each way on both axes, and the
  four diagonals.
  - If all 12 neighbours equal the centre, the normal is straight up (595-603). Most of a
    2.5D job takes this exit.
  - Otherwise the height change per cell along each axis is interpolated: along the axis
    it blends the change across the cell's two edges by position; across the axis it is a
    quadratic B-spline blend of the three neighbouring lines (`nearGradient`, 136-158).
    Lighting is therefore continuous across cell borders, and at a cell centre it reduces
    to a central difference (96-101).
  - An edge that `edgeIsStep` calls a step contributes zero and hides the cells beyond it,
    so a flat top stays flat up to its rim (109-113, 193-194).
  - Only the cell's own four edges get the full predicate; other stencil edges are used
    when shallow and otherwise repeat the cell's own edge (115-121, 126-132).
  - `normal = normalize(-dhdx, 1, -dhdz)` (624).
- **Zoomed out:** `wide = smoothstep(0.5, 1.0, cellsPerPixel)` from screen-space
  derivatives (630-633). At `wide = 1` the gradient uses the four edges around the point
  under a tent weight (`farGradient`, 162-175), which averages scallops narrower than a
  pixel instead of aliasing into moiré (plan 58-60).

### 9.6 Light, colour, depth darkening

```glsl
// heightfieldShader.ts:31-40
vec3 calcLighting(vec3 normal) {
  vec3 keyDir = normalize(vec3(120.0, 180.0, 120.0));
  vec3 fillDir = normalize(vec3(-120.0, 80.0, -80.0));
  vec3 fillColor = vec3(0.588, 0.714, 1.0);
  float diff1 = max(dot(normal, keyDir), 0.0);
  float diff2 = max(dot(normal, fillDir), 0.0);
  return vec3(0.7) + vec3(0.9) * diff1 + fillColor * 0.35 * diff2;
}
```

- Ambient 0.7, a white key light of 0.9 from world `(120, 180, 120)`, a blue-white fill
  (`0x96b6ff`) of 0.35 from `(-120, 80, -80)`. Diffuse only: no specular, no shadows, no
  tone mapping. The same three lights exist as scene lights for the tool and clamps
  (`SimulationViewport.tsx:639-646`). Directions are fixed in world space; they do not
  follow the camera.
- **Depth darkening** (490-496): `1 - 0.12 * clamp((stockTopZ - height) / max(stockTopZ - stockBottomZ, 0.001), 0, 1)`.
  The deepest cut is 12 % darker than the top. Applied to tops and slope risers; a block's
  step walls do not take it (`instancedBoundary.ts:346-349`).
- **Stock colour:** `project.stock.color` if set, else the theme's `stockDefault`
  (`SimulationViewport.tsx:783`, `969`; `App.tsx:461`), `0xb5beca` in the dark palette and
  `0xc2cad4` in the light one (`src/theme/palette.ts:307`, `434`). One colour for the
  whole stock: cut and uncut surfaces differ only by lighting and depth darkening.
- Background: theme `background`, `0x141820` dark, `0xeef2f7` light (`palette.ts:298`,
  `425`; `SimulationViewport.tsx:628`, `725-728`).

### 9.7 Tool mesh — `toolMesh.ts:55-145`

Built once per playback, tip at the local origin, axis up:

| Tool | Cutter geometry | Lines |
|---|---|---|
| `flat_endmill`, `drill`, `plasma` | cylinder of the tool radius, length `cutLength` | 120-131 |
| `ball_endmill` | lower hemisphere of the tool radius plus a cylinder of `max(cutLength - r, 0.5)` | 81-97 |
| `v_bit` | cone of height `max(r / tan(angle / 2), 0.2)` with its point down, plus a cylinder of `max(cutLength - coneHeight, 2)` | 98-119 |

- A shank cylinder of radius `1.05 × r` sits above the cutter (135-142).
- `radius = max(toolRadius, 0.05)` (56). 24 radial segments.
- `cutLength` is the tool's `maxCutDepth` when positive, else `3 × diameter`;
  `shankLength` is `2 × diameter` (`useSimulationModel.ts:451-458`).
- Materials are standard PBR: cutter `toolCutter` with emissive at 0.35, roughness 0.3,
  metalness 0.85; shank `toolShank`, roughness 0.45, metalness 0.7 (63-75). Dark palette
  `0xd9dde3` / `0x7d8591` (`palette.ts:327-329`).
- The tool is drawn only while Play tool is on, is opaque, and has no hide switch. There
  is no holder. **[inferred]** The fixed minimum lengths (0.5, 2, 0.2) are unit-blind and
  behave differently in inch projects.

Also in the scene: clamp meshes, highlighted when selected or when a toolpath collides
with them (`SimulationViewport.tsx:1220-1253`), and an origin triad sized at 5 % of the
largest stock dimension when `origin.visible` (1255-1285). Both come from
`src/engine/csg.ts`.

### 9.8 Uploads and the render loop

- **Dirty-rectangle upload.** `uploadHeightfieldRegion` pushes only the changed rectangle
  of `topZ` into the height texture (`gpuMesh.ts:110-167`). "A full-grid re-upload moves
  cols×rows×4 bytes every cutting frame (9 MB at detail 1500); the dirty rect during
  playback is typically just the tool's footprint" (99-101). It falls back to a full
  upload until the texture has been drawn once (`SimulationViewport.tsx:846-851`).
- **Per playback frame** (`flushPlaybackGridToGpu`, 828-858): set the winner progress
  uniforms; if the controller has a dirty region, upload it, invalidate the corner pass
  and the winner texture for the same rectangle, clear the region, request a draw.
- No mesh is rebuilt while cutting. Walls track the height texture in the vertex shader
  (comment 821-827).
- **Render on demand** (401-409, 691-703): the loop draws only when a flag is set or
  playback is running, and skips drawing entirely while the Simulation tab is not active.
  Camera input sets the flag rather than drawing inline (652-658). Before every draw the
  corner pass and winner textures are refreshed (159-164, 700).
- Renderer: antialiasing on, opaque, high-performance GPU preference
  (`src/components/viewport3d/webglRenderer.ts:43-55`); device pixel ratio capped at 2
  (`SimulationViewport.tsx:626`).
- WebGL2 unavailable: the scene is never built and a message replaces it. Context lost:
  playback pauses and an overlay shows until the context returns, after which the corner
  and winner textures are recomputed (666-689).

### 9.9 Refinement: edge crossings and outline corners (issue #973)

**Problem.** A cell height says what the cell centre sees, not where between two centres
the cut ended. Walls drawn on cell edges make any wall that is not grid-aligned a
staircase, and its faces light as alternating X-facing and Y-facing streaks
(`heightfieldShader.ts:244-252`). Measured: a stamped circular slot's wall corners sit
0.37 cell rms from the tool's true edge on cell edges and within 0.02 cell with this
refinement (plan 88-89; INDEX line 44).

**Data added, CPU** (`edgeCrossings.ts:29-48`): 4 bytes per cell.

| Byte | Meaning |
|---|---|
| 0 | east crossing position: `0` = none; `1..255` = fraction `(code - 1) / 254` from this cell's centre toward the east neighbour's centre |
| 1 | east boundary normal angle in the sheet plane, a full turn in 256 steps |
| 2 | south crossing position, toward the row + 1 neighbour |
| 3 | south boundary normal angle |

An edge is stored on its lower-index cell. `topZ` is untouched.

**How it is written.**

- *During playback* the kernel stamps as it cuts (`replay.ts:111-129`, `218-236`), only
  for cells this move brought to their current height that lie within one cell of the rim
  of the tool's reach (`distanceSq > (r - cellSize)²`) or, for a ball or V-bit, where the
  profile reaches the stock bottom. `stampEdgeCrossings` (`edgeCrossings.ts:271-348`)
  looks at the neighbours on the side facing away from the tool axis and skips any edge
  that could not be a step (one side on the bottom, or a rise over half a cell).
- *After a static replay* `CrossingFinalizer` (`replay.ts:1197-1289`) does it once the
  heights are final: build a mask of cells that sit on an outline
  (`outlineMasks`, `edgeCrossings.ts:362-391`), index them in 16 × 16-cell tiles with each
  tile's highest outline height, then run **every cutting move again**, visiting only
  tiles in its bounding box whose top is not below the move's lowest Z, and refine only
  cells that move cut to their final height.
- *Where the crossing goes:* the level is the stock bottom beside a through cut and
  halfway up the rise between two floors (`edgeCrossings.ts:312-316`). The crossing is
  where the line between the two centres leaves the move's swept capsule at that level,
  solved in closed form: a slab along the path and a disc at each end
  (`SweptCutter.exitFraction`, 153-245). For a ball or V the capsule's reach is narrowed
  to the radius at which the profile reaches the level (157-163).
- A move that lowers a cell replaces its crossings; one that only matches the depth keeps
  whichever reaches further (322-342). A crossing can move with no height changing, so the
  dirty region is widened to cover it (`replay.ts:247-254`).

**Data added, GPU** (`outlineCorners.ts`): the crossings as an 8-bit RGBA `cols × rows`
texture, and one 8-bit RGBA render target of `(cols + 1) × (rows + 1)`, one texel per grid
corner: R and G are the corner's X and Z offset in `1/254` cell steps around 127, B is the
wall's direction for lighting, `255` meaning "keep the face normal"
(`heightfieldShader.ts:227-242`, `311-315`).

**The solve** (`outlineCornerSolve`, `heightfieldShader.ts:316-404`), one fragment per
corner: for each of the four edges leaving the corner that `edgeIsStep` calls a wall, take
the boundary line through that wall's crossing (or the cell edge itself when no crossing
is recorded); place the corner at the least-squares point nearest those lines, with a weak
pull (`OUTLINE_CORNER_PULL = 0.05`) toward where it was. If the answer is more than half a
cell away, use the average of the crossings instead (376-383). This is dual contouring: a
curve comes out as a curve, and where two cuts' edges meet at an angle the corner lands on
their intersection and stays sharp.

**Who reads it.** The top quads, both wall meshes and the underside all read the same
per-corner offset through one function (`OUTLINE_CORNER_GLSL`, 438-448), so tops meet
walls with no gap or overlap. The walls also light from the stored direction, so a gently
curved wall shades smoothly across its jogs (`instancedBoundary.ts:259-266`).

**Cost.**

- Memory: 4 bytes per cell on the CPU, cloned for playback; the same again as a GPU
  texture, plus the corner target.
- Static build: a second pass over all moves. The plan says recording during the cut
  instead "made the static build ~50 % slower" (plan 74-76); the code comment says it
  "doubled the replay" (`replay.ts:1188-1190`). The deferred pass was the answer to both.
  No figure is given for the deferred pass itself.
- Playback: `stampEdgeCrossings` for rim cells of every piece.
- GPU: one extra pass, scissored to the dirty rectangle plus 2 cells of reach
  (`OUTLINE_REACH_CELLS = 2`, `outlineCorners.ts:50-56`, `130-144`), before each draw that
  follows a change. Doing the same rule per vertex instead "took the guitar render from
  12.4 to 32.3 ms on a Radeon Pro 560X" (`heightfieldShader.ts:270-273`).
- Code: `edgeCrossings.ts` (444) + `outlineCorners.ts` (262) + about 240 lines of shader
  source + `CrossingFinalizer` (about 110), and it is threaded through the kernel's inner
  loop.

**Without it** the scene is complete and correct at cell resolution: walls stand on cell
edges. A grid with no `edgeCrossings` array pays nothing in the kernel
(`replay.ts:111-115`), and the shaders compile without the corner lookups
(`heightfieldShader.ts:639-641`, `667-681`).

### 9.10 Refinement: winner-move shading (issue #1209)

**Problem.** Inside a cell the top is flat at the height sampled at the centre. A ball
scallop whose passes run along cell edges samples flat, and a through cut that ends inside
a cell either hides the cell's remaining material or shows none of the cut
(`e2e/viewportViews.smoke.spec.ts:888-893`). Measured on the reference "guitar" project:
the cell sample matches the exact height (within 1e-4 in) at 83 % of sub-cell points on
the finished surface and 33 % on the through-cut rim; with this refinement all but about
0.1 % and 1 % (plan 118-121).

**Data added, CPU** (`moveTable.ts`):

```ts
// 31-41
export const WINNER_NONE = 0xffffffff      // uncut cell
export const WINNER_UNKNOWN = 0xfffffffe   // lowered by a move not in the table
export const MOVE_TABLE_STRIDE = 8         // floats per move
export const MOVE_TABLE_MAX_TOOLS = 8

// 43-54
export interface MoveTableTool { toolType: SimulationReplayItem['toolType']; toolRadius: number; vBitAngle: number | null }
export interface SimulationMoveTable { data: Float32Array; count: number; tools: MoveTableTool[] }
```

- `grid.winners`: one unsigned 32-bit index per cell, naming the move that set the cell's
  height.
- `grid.moveTable`: 8 floats per cutting move, `from.xyz, toolSlot, to.xyz, 0` (23-24,
  96-106), and up to 8 distinct cutters. A ninth cutter gets no slot and its cells are
  marked unknown (76-87).
- **Kernel rule** (`replay.ts:131-142`, `191-217`): a lowered cell takes the move. A move
  that leaves a cell at the same height takes it only if the cell centre lies more deeply
  inside its footprint (`toolRadius - distance`), the lower index winning a tie, so the
  winner does not depend on move order. Heights are unchanged by any of this.
- Playback copies the base table and appends its own pieces (`extendMoveTable`,
  `moveTable.ts:65-74`; `playback.ts:263-281`), and reports how far through the table the
  cut has got (`getWinnerProgress`, 294-299).

**Data added, GPU** (`winnerShading.ts:62-104`): the winners as a 32-bit unsigned integer
texture, uploaded with the same dirty rectangle as the heights; the move table as a
4-channel 32-bit float texture, two texels per move, 2048 texels wide and at most 2048
rows (`winnerShader.ts:32-35`), so **at most 2,097,152 moves**. A longer program falls
back to cell-sampled shading and logs one line (`winnerShading.ts:67-74`).

**The shader** (`heightfieldShader.ts:733-759`; `winnerShader.ts:39-200`): only when a
cell covers two pixels or more (`cellsPerPixel < 0.5`), take the winners of the 2 × 2
cells whose centres surround the fragment, plus each winner's neighbour on either side in
cut order when it has the same tool (the adjacent segment of the same pass). For each
candidate evaluate the kernel's own formula at the fragment's exact sheet position, take
the minimum as the height and its analytic derivative as the slope, discard if that is the
stock bottom, and light from the slope. An unknown winner nearby makes the fragment fall
back to the stencil path (9.5). A cell whose centre is cut through still draws when a
neighbour has material, so a thin skin inside it shows (741-750). During playback entries
past the current one are ignored and the current one is clamped to its elapsed fraction,
so nothing shows ahead of the tool (`winnerShader.ts:131-134`, `145`, `188`).

**Cost.**

- Memory: 4 bytes per cell (cloned for playback) and 32 bytes per cutting move, each
  mirrored on the GPU.
- Kernel: one write per lowered cell; on equal-height cells one distance evaluation
  against the current winner's move.
- Fragment: up to 4 integer fetches and up to 12 move evaluations (4 winners × 3
  candidates), each 2 texel fetches plus a profile evaluation, then one slope evaluation.
  No frame-time figure is given.
- Requires integer and 32-bit float textures and screen-space derivatives.
- Coordinates in the table are 32-bit floats. **[inferred]** The shader's exact surface is
  therefore at single precision while the kernel computed in double.

**Without it** the surface is the stencil-shaded flat tops of 9.2 and 9.5. A grid without
`winners` compiles exactly the earlier shader (`heightfieldShader.ts:800-802`).

---

## 10. The UI

### 10.1 Where it lives

Simulation is the third of three centre tabs, `'sketch' | 'preview3d' | 'simulation'`
(`App.tsx:65`), labelled "Simulation" (`src/i18n/locales/en/appShell.ts:39`;
`AppShell.tsx:592-616`). All state is local React state: there is **no simulation state in
the store** and nothing is persisted. `simulationDetailCells` and `simulationMode` are
`useState` in `App.tsx:71-72`; playback state is `useState` inside the viewport
(`SimulationViewport.tsx:378-428`).

Which operation is simulated is the operation selected in the CAM panel; the simulation
panel has no operation picker. Visibility in Visible mode is each operation's own
`enabled` and `showToolpath` flags.

### 10.2 Toolbar controls (always shown) — `SimulationViewport.tsx:1427-1484`

| Control | Options, default | English strings (`src/i18n/locales/en/viewport.ts`) |
|---|---|---|
| Mode toggle (two buttons) | Selected, Visible; default Selected (`App.tsx:72`) | group "Simulation mode"; "Selected"; "Visible" (42-44) |
| Detail slider | 240 to 1500 in steps of 40; default 280 (`SimulationViewport.tsx:151`, `165-166`; `App.tsx:71`); the value is shown beside it | "Detail"; title "Simulation detail" (45-46) |
| Play tool toggle | on/off, default off; disabled unless mode is Selected, a playback input exists and WebGL is available (1462) | "Play tool"; titles "Switch to selected mode to use tool playback", "Select an operation with a valid toolpath to play", "Toggle tool playback" (48-51) |
| Camera view menu | Isometric (default), Top, Bottom, Front, Back, Right, Left; shows "Custom view" after free movement; actions Fit to model, Reset view | "Camera view"; "Standard views"; "Actions"; "Isometric view", "Top view", "Bottom view", "Front view", "Back view", "Right view", "Left view", "Custom view"; "Fit to model"; "Reset view" (25-37) |
| Fresh-stock info button | only when the project has more than one setup | "{setup} is simulated on fresh stock: material removed by other setups is not shown." (47) |

Turning Play tool on while the mode or selection stops qualifying turns it off again
(1145-1149).

Two application-level commands also act on this view: "Zoom to model" and "Zoom selected"
(a rubber-band zoom window) in the top command bar
(`src/components/layout/TopCommandBar.tsx:238-249`;
`src/i18n/locales/en/shell.ts:49-50`; `App.tsx:342-351`, `463-464`).

### 10.3 Playback bar (shown only while Play tool is on) — 1485-1647

A floating bar the user can drag by a handle (1486-1530).

| Control | Options, default | English strings |
|---|---|---|
| Play / Pause | toggles; pressing Play when finished resets first (1163-1177) | "Play", "Pause" (56-57) |
| Stop | pause, reset to the start (1179-1191) | "Stop & reset" (58) |
| Progress readout and scrubber | whole percent; range 0 to 1000 (1552-1564) | aria "Playback progress" (59) |
| Speed slider | 1× to 32× on a log scale over 100 positions; default 2× (174-180, 188-199) | "Speed"; tooltip "Speed multiplier of operation feed ({feed} = 1×). Current: {multiplier}" or "…of fallback feed…"; aria "Playback speed multiplier" (60-63) |
| Step select | mm: 0.05, 0.1, 0.25, 0.5, 1, 2.5, 5, 10, default 2.5; inch: 0.002, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, default 0.1 (213-217) | "Step"; tooltip "Maximum distance the tool advances per frame. Smaller = smoother motion, larger = faster playback." (64-65) |
| X, Y, Z readout | tool tip relative to the machine origin, Y inverted (1103-1109); 2 decimals in mm, 3 in inch (264-270) | labels "X", "Y", "Z" are literals |
| Feed readout | a dot coloured by move kind (cut and leads, plunge, rapid; `src/styles/layout.css:1418-1434`); the current move's authored feed as "N mm/min" or "N in/min", "Rapid" while playing a rapid, "—" otherwise; then the feed scale as a percent, "100%" when unscaled (1626-1645) | tooltip "Cutting feed of the current move. Reduced slotting pocket cuts show their scaled feed here; the dot colour marks the move kind (rapids have no feed)."; dot title "Idle" (66-67); "Rapid" is a hard-coded literal (1638) |

All bar controls are disabled while the playback scene is building or WebGL is not ok
(1161). A spinner overlay shows while a result is computing or the playback scene is
building (1372-1376). WebGL messages: "3D simulation isn't available", "3D graphics
context lost" and their bodies (`viewport.ts:52-55`).

There are no keyboard shortcuts in `SimulationViewport.tsx` (no key handlers in the file).

### 10.4 How the viewport drives playback

- **Start** (916-1035): one animation frame after the toggle, so the spinner paints
  first, it builds a `PlaybackController` from `getBaseGrid()`, the selected operation's
  moves, the tool, `maxSegmentLength`, the cut feed and the plunge feed; builds the scene
  on `controller.liveGrid`; builds the tool mesh. The static scene is disposed while
  playback is on (771-774).
- **1× speed** is the operation's own feed: `operation.feed / 60` project units per second
  (`useSimulationModel.ts:469-473`). With no feed it falls back to 50 mm/min or 2 in/min
  **read as units per second** (`SimulationViewport.tsx:181-182`, `368-374`).
  **[inferred]** The constant names say feed; the value is used as a per-second speed, so
  the fallback 1× is 50 mm/s.
- **Each frame while playing** (1051-1088): `dt = min(elapsed seconds, 0.1)`;
  `step = min(speed × multiplier × dt, Step)`; `controller.advance(step)`; move the tool;
  upload the dirty rectangle; stop when finished.
- **[inferred]** The Step cap bounds real speed: at 60 frames a second and the default
  2.5 mm it is 150 mm/s of cut-feed budget, whatever the multiplier. The code says as
  much: "Above ~30× the per-frame Step cap and the browser's RAF cadence both saturate"
  (175-178).
- **Scrub** (1193-1218): the slider updates at once; the engine seek is coalesced to one
  per animation frame. Forward drags are incremental; only backward drags replay.
- Progress, pose and playback statistics are flushed to React state about 10 times a
  second, not every frame (1124-1143).
- There is no elapsed-time or remaining-time display, no loop, and no way to play more
  than the selected operation.

### 10.5 Camera — `orbitControls.ts`, `viewPresets.ts`

- **Projection:** perspective, 45° vertical field of view
  (`SimulationViewport.tsx:648`). There is no orthographic mode; Top and the side views
  are perspective views along an axis.
- **Model:** a spherical orbit around a target point: `theta`, `phi`, `radius`
  (`orbitControls.ts:127-138`). Default `theta = π/4`, `phi = π/3`, `radius = 250`
  (`viewPresets.ts:40-44`). Radius limits 0.1 to 10000 (46-47).
- **Bindings** (`orbitControls.ts:182-310`):
  - left drag: orbit, 0.01 rad per pixel; `phi` clamped to `[0.05, π - 0.05]`; dragging
    down raises the camera (263-275);
  - Shift + left drag, middle drag or right drag: pan in the view plane (202-205,
    165-180); the context menu is suppressed (312-314);
  - wheel: zoom by `exp(deltaY × 0.0015)`, keeping the point under the pointer fixed
    (284-310);
  - one finger: orbit **[inferred from 187-205]**; two fingers: pinch zoom and pan
    together (187-200, 236-254).
- Any free movement sets the active preset to none ("Custom view") (197, 264, 277, 291).
- **Presets** (`viewPresets.ts:49-90`): iso `(π/4, π/3)`; top `phi = 0` with up
  `(0, 0, -1)`; bottom `phi = π` with up `(0, 0, 1)`; front `theta = 0`; back `π`; right
  `π/2`; left `3π/2`; the four side views at `phi = π/2`. A preset keeps the current
  radius (`orbitControls.ts:337-339`).
- **Fit** (`fitToBounds`, 351-422): keep the orientation, transform the 8 corners of the
  bounding box into view space, take the smallest distance at which all fit both
  horizontally and vertically, multiply by 1.1, and set the target to the box centre.
  "Fit to model" frames the stock surface, visible clamps and the origin triad
  (`SimulationViewport.tsx:576-609`).
- **Reset view:** isometric at the default radius, then fit (1477-1480).
- **First show:** the first mesh after a project change is framed once in isometric
  (806-816, 1289-1313). Later rebuilds keep the camera.
- **Zoom window:** drag a rectangle of at least 6 px; the view pans to its centre and
  divides the radius by the fit scale (`orbitControls.ts:423-465`).
- **Clip planes** follow the camera: near is 1 % of the radius; far is the largest of the
  initial 5000, twice the radius, and the distance to the far side of the scene plus 10 %
  (140-149). The stated reason: a 1220 × 2400 mm sheet needs the camera about 2900 mm out.

---

## 11. Presence or absence of the eight features asked about

Searches were case-insensitive `grep -r` over `src/` unless a narrower path is given.

**(a) Highlighting uncut or rest material against an intended shape — not present.**
Searched `uncut|rest material|remaining stock|leftover` in `src/engine/simulation`,
`src/components/simulation`, `src/app/simulation*.ts`, `src/app/useSimulationModel.ts`:
the only hits are comments about bar ends and the "uncut block" fallback. The simulation
holds no intended shape at all. "Rest" operations exist elsewhere but are **2D regions
derived from feature geometry and tool sizes**, not from the heightfield
(`src/engine/operations/camPlan/reconcileRest.ts:17-29` imports
`generatePocketRestRegionDrafts` and `generateEdgeRestRegionDrafts` from
`toolpaths/restRegions`; the English messages are at `warnings.ts:303-305`). The plan's
only mention of remaining stock is "Combined remaining-stock simulation across the flip is
out of scope" (plan 229-230).

**(b) Gouge or overcut detection — not present.** Searched `gouge|overcut|over-cut`: hits
are (1) the Edge Route "Vertical overcut" *parameter*, a deliberate cut below the stock
bottom with a CAM-panel notice (`src/components/cam/verticalOvercutNotice.ts:17-23`),
unrelated to simulation; (2) `src/test/rotaryCutterGouge.ts`, a rotary test helper. The
kernel clamps at the stock bottom (`replay.ts:182`) and records nothing when a cut would
have gone lower.

**(c) Collision checks — not in the simulation.**
- *Rapids through material:* none. Rapids are skipped (`replay.ts:38-40`).
- *Holder or shank:* none. Searched `holder|shank` in the simulation and its UI: every
  hit is the tool mesh's cosmetic shank (`toolMesh.ts:135-142`) or the rotary kernel's
  cutter solid.
- *What does exist, upstream:* a **toolpath-level clamp check**. Generation marks moves
  that cross a clamp's footprint (grown by the XY clearance and the tool radius) below the
  required Z (`src/engine/toolpaths/clamps.ts:25-42`), and returns `collidingClampIds`
  and `collidingMoveIndices` on the `ToolpathResult` (`toolpaths/types.ts:76-79`). The
  simulation viewport only **tints the colliding clamp meshes**
  (`SimulationViewport.tsx:1238-1241`; `src/engine/csg.ts:684-692`). The check runs
  against clamp rectangles, never against the simulated stock.

**(d) Comparison of the result against a target model — not present.** Searched
`deviation|compareTo|target model|targetModel|against the model|designSurface|excess material`
in the simulation, its UI and its app files: no hits. The CSG model preview and the
heightfield are separate tabs with no shared data.

**(e) Colouring the cut surface by operation, tool or depth — not present, except a fixed
depth darkening.** Searched `colorBy|colourBy|operationColor|toolColor|by operation` in
`src/engine/simulation` and `src/components/simulation`: no hits. The surface is one stock
colour (`heightfieldShader.ts:779`). `depthDarken` (490-496) darkens by at most 12 % with
depth and is not a user option. Note that the #1209 data (`grid.winners` plus the move
table's tool slot) records which move and which cutter set every cell, but nothing uses it
for colour.

**(f) Machining time tied to the simulation — not present.** The playback bar shows
progress as a percentage of path length (`SimulationViewport.tsx:1553`) and no time. A
feed-based time estimate exists in the **operation booklet** report:
`feedControlledTimeSeconds` sums `distance / feed × 60` over cut and plunge moves, reports
rapid distance separately without a time, and returns no time when a feed is invalid
(`src/engine/operationBooklet/report.ts:271-317`, used at 550-557, label
`booklet.label.estimatedFeedTime`). It shares `effectiveFeed` with playback
(`toolpaths/feed.ts:33-35`) and nothing else.

**(g) Measuring on the simulated stock — not present.** Searched `measure` in
`src/components/simulation`, `src/components/viewport3d`, `src/engine/simulation`: only
the verb in comments. The viewport has no picking of any kind; pointer input goes to the
camera only. The one readout is the tool-tip XYZ during playback.

**(h) Exporting the simulated result — not present.** Searched
`STLExporter|toDataURL|preserveDrawingBuffer|toBlob|exportStl|screenshot`: hits are the
STL **model** export of the designed features (`src/engine/modelExport/stl.ts`), the 2D
operation snapshot for the booklet (`src/components/canvas/operationSnapshot.ts:171`), and
top-view thumbnails of imported meshes (`src/import/stl.ts:246-379`). Nothing reads the
heightfield out, and the renderer is not created with a preserved drawing buffer
(`webglRenderer.ts:45-50`).

---

## 12. Tests

All are plain scripts run with `npx tsx <file>`: a local `assert`, top-level calls, an
exception on failure. Fixtures are hand-built `SimulationGrid` literals unless noted.
Notice that these fixtures use `stockTopZ: 0` with a **negative** `stockBottomZ` (`-6`,
`-10`), unlike the app, where the bottom is `0`.

### 12.1 Flat path

| File | Asserts | Cases worth porting as red tests |
|---|---|---|
| `src/engine/simulation/grid.test.ts` (297) | Stock-profile masking through real `Project` objects built with `newProject`, `circleProfile`, `rectProfile`, `stockFromFeature`, at `targetLongAxisCells: 48` | `testCircleStockMasksOutsideProfile` (15-30 % of cells are `0`; every cell is `0` or `stockTopZ`), `testCircleStockMasksCenter`, `testRectStockAllCellsFilled`, `testDefaultRectStockNoMasking`, `testArcProfileStockMasks`. **No test covers cell-size resolution, the 24-cell floor, or `cols`/`rows` rounding.** |
| `src/engine/simulation/replay.test.ts` (287) | The inlined kernel equals a reference loop built on `cutterSurfaceZ`, within `1e-6`, on a 60 × 50 grid with `cellSize 0.5`, origin `(-3, -2)`, top `0`, bottom `-6` | `testKernelMatchesReferenceProfile` for `flat_endmill` r 1.5, `ball_endmill` r 2, `v_bit` r 3 at 60° and 90°, `drill` r 1.25, over `TEST_MOVES`: a diagonal ramp, an axis-aligned cut, a stationary plunge, a short lead-in, and a cut below the stock bottom; plus "more than 50 cells cut". `testDirtyRegionCoversChangedCells` (every changed cell inside the region; `changedCount` equals the number of changed cells). The other two cases are rotary. **The reference uses the same nearest-point Z as the kernel, so this file does not test swept-ramp accuracy.** |
| `src/engine/simulation/playback.test.ts` (317) | Controller behaviour on a 40 × 4 grid, `cellSize 1`, flat r 1; `slotMoves()` is a plunge to `-2` then a 30-long cut | `testSubdividePreservesMoveMetadata` (a 10-long move at max 2 gives 5 contiguous pieces carrying `feedScale` and `source`), `testSubdivideLeavesShortMovesUntouched`, `testReducedFeedMoveAdvancesSlower` (budget 10 finishes a full-feed cut; the next 10 covers 1 unit of a `feedScale 0.1` cut), `testDisabledReferenceFeedIsConstantSpeed`, `testSeekIsGeometricDespiteFeedScaling` (seek 0.75 of 20 lands at 15), `testForwardSeekMatchesFreshReplay` (seeks to 8, 20, 27.5 equal one seek to 27.5), `testBackwardSeekRestoresEarlierState`, `testDirtyRegionAccumulatesUntilCleared` (grows across advances; a backward seek dirties the whole grid). Three cases are rotary or key-shape checks. |
| `src/engine/simulation/tabbedEdgeSimulation.test.ts` (79) | End to end through the real store and generator: a 50 × 40 × 5 rectangle at `(10, 10)`, a 6 mm flat tool, `edge_route_outside`, four automatic tabs, at detail 180 and 600 | At each of four tab points the height is at least 2.9 and both flanking points are exactly `0`; mid-playback never removes more tab stock than the final result; playback at fraction 1 equals the static height at the tab and keeps the through cut beside it. This is the one test tying generation, static replay and playback together. |
| `src/engine/simulation/gpuMesh.test.ts` (107) | Top-surface template: `cols × 4` vertices, `cols × 6` indices, `rows` instances, 16-bit indices, neighbouring cells share no vertex; one template at 280 × 280; bounds reach from `stockBottomZ` to `stockTopZ` | `testOneIndependentTopPerCell`; the rest are Three.js specifics. |
| `src/engine/simulation/instancedBoundary.test.ts` (218) | Wall count formula, template scales with row length and instances with row count, bounding sphere covers the stock, both meshes include the same `edgeIsStep` source, group wiring | `testWallInstanceCount`. The predicate check is a source-text check. |
| `src/engine/simulation/heightfieldShader.test.ts` (795) | Runs the shader's scalar functions, compiled to JavaScript by `glslScalar.testSupport.ts`, against height profiles and against cuts stamped by the real kernel | For the predicate, as 1D height rows: `testTabBridgeAndCutThroughStaySteps` (`[0,0,3,3,20,20]`; `[0,0,3,0,0]`; tabs 1 to 19.4 high beside 20 are steps; 19.5 and 19.8 are slopes; skins of 0.05 to 5 beside a through cut are steps; the stock perimeter is a step), `testWallsAndSlopesKeepTheirClass` (pocket wall, terraces, a one-cell wall; 0.4 and 0.5 cell deep are slopes, 0.6 is a step, at cell sizes 0.1, 1, 2 and 2.5; a V ridge and a roundover are slopes), `testShallowScallopIsSlope` (the "guitar" case: cell `20 / 1360` in, ball radius 0.125 in, stepover 0.04 in). For normals: `testGradientIsContinuousAndExact`, `testGradientLeavesStepsOut`, `testWideningAveragesNarrowScallopsOnly`. For #973: `testCircularCutFollowsTheTool` (0.02 cell against 0.37), `testStraightSlotAtAnyAngle`, `testBallBottomingOutEndsOnItsRealEdge`, `testMaterialCornersStaySharp`, `testTabBridgeKeepsItsWidth`, `testPassesAtOneDepthUnite`, `testWithoutCrossingsNothingMoves`. |
| `src/engine/simulation/outlineCorners.test.ts` (323) | #973 wiring: flat grids carry crossings and playback clones own theirs; a pass that only moves a crossing still reports a dirty region; one corner texture for all four readers; changes move corners only inside the refreshed rectangle; terrace walls keep crossings at depths 32-bit floats round down | `testCrossingOnlyChangeIsDirty` (two flat r 3 passes at y 20.1 and 20.3 on 40 × 40: no height changes, crossings do, `changedCount === 0` and the region is not null), `testTerraceWallsAtFloat32Depths` (depth pairs 11.4/7.33, 9.7/3.1, 12.3/6.1, static and playback). Only if #973 is ported. |
| `src/engine/simulation/winnerShading.test.ts` (373) | #1209: recording winners leaves every height identical at detail 120 and 333 on `syntheticJob` (ball raster, V groove, ramped flat through cut, plunge); tie rule independent of order; playback at 7, 53 and 197 steps, a backward seek and a reset end with a direct seek's winners and heights; shader formula equals the kernel; 99.5 % of 6000 random points within 1e-4; nothing shown ahead of the tool | `testHeightsUnchanged` and `testTieRuleIsOrderIndependent` if #1209 is ported. `testPlaybackWinnersMatchBatch` is worth reading even if it is not: it plays a **level** route on purpose, with the comment that a part-played ramp does not match a whole-move replay (211-214). |

### 12.2 App layer (not read; from `src/app/INDEX.md`)

`simulationSetup.test.ts` (INDEX line 10: real Top and Bottom pockets replayed on fresh
stock, mm and inch), `useSimulationModel.test.ts` (line 24: operation-count branches, the
loading render, a stale build ignored), `simulationReuse.test.ts` (line 22: key and cache
rules), `simulationTabSwitch.test.ts` (line 23), `simulationAcquisition.test.ts`
(line 21), `simulationSetupModel.test.ts` (line 11). These test React and cache wiring.

### 12.3 Browser tests that render the simulation

Playwright specs in `e2e/`. Several inject a small grid literal and render it through the
real shaders, then sample pixels.

| Spec, line | What it checks |
|---|---|
| `viewportViews.smoke.spec.ts:64` | switching the simulation view to Front updates the menu trigger |
| `:86` | a 3 × 3 grid with one cell at height 3 (`topZ = [0,0,0, 0,3,0, 0,0,0]`) keeps its tab top and cut-through rims at two camera angles |
| `:211` | a tab rim is shaded flat without flattening real slopes |
| `:305` | a shallow ball finish shades smoothly at any zoom |
| `:483` | a wall stays crisp above half a cell; a step at or below it shades as a slope |
| `:620` | a slanted wall draws straight; tops, walls and underside share its outline (#973) |
| `:798` | a gently sloped wall is lit evenly along its length (#973) |
| `:887` | the exact cut inside a cell from its winner moves, on a 20 × 20 grid (#1209) |
| `setupPreview.smoke.spec.ts:101`, `:189` | simulation follows the face in view; a single-setup project shows no info button |
| `webglContextLoss.smoke.spec.ts:57` | the context-lost message shows until the context returns |
| `panelHandles.smoke.spec.ts:99` | panel state survives Sketch → 3D → Simulation switching |
| `tablet1112.smoke.spec.ts:1408` | the fresh-stock info button is a 44 px target |

The browser tests read results through `data-simulation-*` attributes rather than pixels
where they can (`SimulationViewport.tsx:1345-1369`): `data-simulation-removed-cells`,
`data-simulation-min-z`, `data-simulation-surface`, and during playback
`data-simulation-playback-progress`, `-removed-cells`, `-min-z`.

---

## 13. Performance facts stated in the code or the plan

| Fact | Source |
|---|---|
| Detail range 240 to 1500 cells on the long axis, default 280; engine default 180, floor 24 | `SimulationViewport.tsx:151`, `165-166`; `App.tsx:71`; `grid.ts:23-24` |
| A full height upload is 9 MB at detail 1500 | `gpuMesh.ts:99-101` |
| Static replay of a 24k-move path: 86 ms (a rough) and 61 ms (a finish) per tab switch, 89 ms for the playback base, on the author's `turn-shape-test-01` project | `useSimulationModel.ts:273-275`; `simulationReuse.ts:25-27`; `simulationCompute.ts:346-347` |
| Chunked replay slice: 16 ms of synchronous work between yields | `replay.ts:1457` |
| Replay cache: 6 results, one document | `simulationReuse.ts:127-140` |
| Playback subdivision at 0.4 × radius made about 5/6 of cell tests repeats; 2 × radius cut redundant work about 4 times | `useSimulationModel.ts:459-467` |
| Playback speed 1× to 32×; above about 30× the Step cap and the frame rate saturate | `SimulationViewport.tsx:174-178` |
| Frame `dt` capped at 0.1 s | `SimulationViewport.tsx:1059` |
| Readout refresh about 10 Hz | `SimulationViewport.tsx:1124-1140` |
| Recording crossings during a static replay: "~50 % slower" (plan) or "doubled the replay" (code) | plan 74-76; `replay.ts:1188-1190` |
| Corner rule per vertex: 12.4 → 32.3 ms a frame on a Radeon Pro 560X ("guitar" project) | `heightfieldShader.ts:270-273`; `outlineCorners.ts:31-32` |
| #973 accuracy: 0.02 cell against 0.37 cell rms | plan 88-89 |
| #1209 accuracy: 83 % → about 99.9 % (surface), 33 % → about 99 % (through-cut rim), within 1e-4 in | plan 118-121 |
| Move table limit 2048 × 2048 texels, about 2.1 M moves; 8 tool slots | `winnerShader.ts:32-35`; plan 122-125; `moveTable.ts:41` |
| The CPU-built boundary it replaced: about 18 vertices × 48 B per cell, hundreds of MB, multi-second stall | `instancedBoundary.ts:21-24` |
| Per-edge instancing bottlenecked; "indexing the per-edge template away made no fps difference at 1480 detail" | `instancedBoundary.ts:74-78` |
| Idle redraw avoided: "a potentially multi-million-vertex scene at 60 fps while completely idle" | `SimulationViewport.tsx:404-408` |
| Pixel ratio capped at 2 | `SimulationViewport.tsx:626` |

No timing is given for the flat kernel per move, for a build at maximum detail, or for the
frame time of the shipped renderer. `docs/PERF_ASSERTIONS.md` mentions simulation only for
the rotary square-playback test (lines 78, 101).

**The "GPU suggestion" component is unrelated.** `src/components/ToolpathGpuSuggestion.tsx`
and `src/components/canvas/toolpathGpuSuggestion.ts` offer GPU rendering for the **2D
sketch canvas's toolpath lines** when drawing them is slow ("Canvas is taking longer to
draw toolpaths. GPU rendering may make panning and zooming smoother.",
`src/i18n/locales/en/appShell.ts:28`). The simulation has no CPU rendering fallback to
suggest away from.

---

## 14. Coordinate-convention facts for a port

These are the places where PureCutCNC's Y-down sheet shows in the simulation.

1. **Grid rows** run in increasing sheet Y, which is toward the operator on a Y-down sheet
   (section 2.4). The kernel's maths is symmetric in Y and contains no winding or
   handedness rule.
2. **World mapping** is `(x, z, y)`: sheet Y becomes world Z with height on world Y
   (`heightfieldShader.ts:671-680`). No sign is flipped.
3. **Compass names in the shaders** follow the row index: "north" is `row - 1`, "south" is
   `row + 1`, "west" is `col - 1`, "east" is `col + 1` (`heightfieldShader.ts:582-593`).
   `edgeCrossings` stores the **east and south** crossings, so "south" means the higher
   row (`edgeCrossings.ts:29-35`, `350-354`).
4. **Crossing normal angles** are `atan2(normalY, normalX)` in the sheet's own `(x, y)`
   plane (`edgeCrossings.ts:58-61`), and the corner solve uses them as `(cos, sin)` on
   world `(X, Z)` (`heightfieldShader.ts:358-360`).
5. **Camera presets** assume world Y up and name views by world axis: Front looks from
   world +Z, which is sheet +Y (`viewPresets.ts:70-74`; `orbitControls.ts:129-133`). Top
   uses up `(0, 0, -1)`.
6. **Light directions** are fixed world vectors (`heightfieldShader.ts:32-33`), so their
   position relative to the part depends on the mapping in item 2.
7. **The readout** inverts Y against the machine origin (`SimulationViewport.tsx:1103-1109`).
8. **Stock Z** is `0` at the bottom and `thickness` at the top (`grid.ts:108-109`).

---

## 15. Out-of-scope things that exist, and their files

Not digested. Listed so a port can skip them cleanly.

- **Rotary ("Around") / wrapped grids.** The unrolled periodic sheet, a second cutting
  kernel that resolves the cutter's true solid against a radial heightfield, a ramp sweep
  schedule, and a bar view that turns under the tool. Files:
  `src/engine/simulation/aroundWrap.ts`; `replay.ts:261-1080` (and the `wrap` branch at
  1108-1140); `tools.ts:19-201` (`cutterRayInterval`); `grid.wrap`;
  `src/components/simulation/aroundView.ts`; `src/engine/aroundFrame.ts`,
  `aroundFold.ts`; the `AROUND_WRAP` shader variants and `frame` parameters throughout
  section 9; the `bar` prop and A-axis readout in the viewport. Tests:
  `aroundReplay.test.ts`, `aroundWrap.test.ts`, `wrappedPlayback.test.ts` (+
  `wrappedPlayback.fixture.camj`), `wrappedCutterGeometry.test.ts`,
  `src/components/simulation/aroundView.test.ts`, `src/app/aroundSimulation.test.ts`,
  `e2e/around3d.smoke.spec.ts`, `e2e/aroundSketch.smoke.spec.ts`,
  `e2e/rotaryModel.smoke.spec.ts`; the wrapped cases inside `replay.test.ts` and
  `playback.test.ts`.
- **Square bar blanks.** A square cross-section seeded into the radial heightfield, with
  an `initialTopZ` baseline for statistics. Files: `grid.ts:29-68`, `125-165`, `263-277`;
  `SquareBarDescriptor` (`types.ts:21-40`); `SQUARE_BLANK_SILHOUETTE_GLSL`. Tests:
  `squareGrid.test.ts`, `squareReplay.test.ts`, `squarePlayback.test.ts`,
  `squareBlankBarScene.test.ts`, `src/app/squareSimulation.test.ts`,
  `src/test/squareSimulationFixture.ts`.
- **Round bar on a flat setup, and its lower half.** The grid seeded with the bar's upper
  half and a static half-cylinder mesh below. Files: `grid.ts:167-236`, `246-261`;
  `FlatRoundBarDescriptor` (`types.ts:42-69`); `flatRoundBarLowerHalf.ts`;
  `ROUND_BAR_SILHOUETTE_GLSL`, `ROUND_BAR_WALL_JOIN_GLSL`. Tests: `flatRoundBar.test.ts`,
  `flatRoundBarLowerHalf.test.ts`, `e2e/flatRoundBarLowerHalf.smoke.spec.ts`,
  `e2e/roundStock.smoke.spec.ts`.
- **Plasma torch.** A full-depth, one-kerf-wide cut along every torch-on move, inlined in
  the flat kernel as `isPlasma` (`replay.ts:98-101`, `165-166`) and drawn as a plain
  cylinder. Tests: `replayPlasma.test.ts`, `src/plasmaTool.test.ts`,
  `e2e/plasmaKerfPreview.smoke.spec.ts`, `e2e/plasmaMode.smoke.spec.ts`.
- **Bottom face and multiple setups.** `src/app/simulationSetup.ts` turns the project and
  paths into a setup's local frame so the grid always sees a top-down stock; each setup
  starts from fresh stock. Tests: `src/app/simulationSetup.test.ts`,
  `simulationSetupModel.test.ts`, `e2e/setupPreview.smoke.spec.ts`,
  `e2e/setupFaces.smoke.spec.ts`, `e2e/setupSimulationProbe.ts`,
  `src/components/simulation/SimulationSetupInfo.test.ts`.
- **CSG model preview.** `src/components/viewport3d/Viewport3D.tsx` (1276 lines) with
  `src/engine/csg.ts` is the separate "3D" tab: a CSG model of the designed features with
  toolpath overlays (`toolpathOverlay.ts`). It is not the simulation. The simulation
  borrows from that folder only `orbitControls.ts`, `viewPresets.ts`,
  `ViewPresetMenu.tsx`, `webglRenderer.ts`, `webglContextGuard.ts`,
  `WebglStatusOverlay.tsx`, and from `csg.ts` the clamp mesh and origin triad builders.

---

## 16. What a port can leave out

Each item is separable in the source; the reason it is separable is given.

1. **Everything in section 15.** A flat block never sets `wrap`, `initialTopZ`,
   `squareBar` or `flatRoundBar`, and every shader variant for them is compiled out when
   they are absent.
2. **Edge crossings and outline corners (#973).** A grid without `edgeCrossings` runs the
   same kernel and the shaders compile without the corner lookups (section 9.9, "Without
   it"). Leaving it out removes `edgeCrossings.ts`, `outlineCorners.ts`,
   `CrossingFinalizer`, the second pass over all moves, and the crossing branches of the
   kernel's inner loop.
3. **Winner-move shading (#1209).** A grid without `winners` compiles the earlier shader
   (section 9.10, "Without it"). Leaving it out removes `moveTable.ts`, `winnerShader.ts`,
   `winnerShading.ts`, the tie rule, and playback's table bookkeeping
   (`appendPiecesToMoveTable`, `getWinnerProgress`).
4. **The `sweep` and `elapsed` parameters of `applyMoveToGrid`** and
   `elapsedFractionOf` / the `parents` array in `playback.ts`. The flat kernel ignores
   them (section 5.1).
5. **The chunked main-thread replay** (section 5.7) and the input-identity stamp
   (`simulationBuild.ts`). They exist because the browser has one thread for UI and
   computation.
6. **The reference-identity replay cache** (`simulationReuse.ts`). It depends on React's
   immutable project objects as cache keys.
7. **The zoom-widening half of the surface normal** (`farGradient` and
   `gradientWidening`, section 9.5) is independent of the near gradient; the
   `wide = 0` path is the complete close-up shading.
8. **Stock-profile masking** (`grid.ts:281-300`), if stock is always a rectangle.
9. **`drillCycles`, `source`, `collidingMoveIndices`, `debugToolpath`** on the toolpath
   result: the simulation reads none of them.
10. **The unused viewport props** `operation`, `operationCount`, `stockHasProfile`
    (section 7.2) and the DOM `data-simulation-*` evidence attributes, which serve the
    Playwright tests.
11. **WebGL-specific handling:** the context-loss guard, the partial-upload fallback to a
    full upload, the 16-bit index choice, and the nearest-filter workaround for float
    textures.
