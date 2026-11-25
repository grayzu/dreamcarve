# Phase 6: 3D preview and simulation — Design Spec

**Status:** draft, written 2026-11-23 with `functional.md`, ahead of the
phase; a slice amends it in the commit that changes what it describes
(`docs/phase-6-worklog.md`, "Process"). Written against the `Phase-5`
branch at `a2eee9b9`.
**Functional spec:** `functional.md`
**Decision record:** ADR-0003; no new ADR (open question 1)
**Visual design:** phase 4's `redesign.md` and `experience.md`; what this
phase adds to them is under "Visual design" below

## Overview

Phase 6 adds a reader to the pipeline: something that takes the runs
export writes and shows what they leave. Three things carry it.

- **`dc-sim`, a new library crate.** The stock as a grid of heights, the
  cutter shapes, the cut of one move into the grid, the runs in order
  with a checkpoint after each, a timeline and a playback cursor, and the
  comparison of a finished grid with a design surface. It knows
  `dc-geometry` and `dc-project` and nothing above them: its input is its
  own plain move type, so `dc-cam`'s tests can use it. The exact per-cell
  heights phase 3 wrote for its tests (`dc_cam::heightfield`) move here
  and become the production kernel.
- **A simulation job in the app core.** `dc-job` turns generated runs
  into a plan (`dc_job::simulate`), builds the design surface from the
  project, and writes the report and the PNG; the CLI's `simulate`
  command is those calls. `dc-app` keeps a simulation as it keeps
  toolpaths: a digest per run, a job for the worker, a result that is
  stale until the next one lands, and the playback, the 3D camera and
  the step's rows as data with no window.
- **A 3D renderer in the window.** `dreamcarve-app` draws the grid from a
  height texture into an off-screen target with a depth buffer and shows
  that in the Viewport's card. The technique is PureCutCNC's, on wgpu.

Nothing in `dc-cam`'s generators or in `dc-gcode` changes (S22). `dc-cam`
loses its test heightfield's closed forms to `dc-sim` and calls them
from its tests.

Data flow: every operation is current and `finish` is in → on the
Simulate step, `dc_app::sim` digests each run (its result's key, its
tool's shape, the board, the cell size) and posts a `SimJob` naming the
first run whose digest changed → the simulation worker calls
`dc_job::simulate::plan` and `dc_sim::build` from that run's checkpoint,
in chunks, reporting progress and reading a stop flag → the checkpoints,
the finished grid, the design surface and the comparison come back keyed
by snapshot → `dc-app` stores them and builds a `Playback` at the end of
the job → the window uploads the grid's heights to a texture and draws.
Play moves the `Playback`'s cursor on the window's thread each frame,
cuts the moves it passes into the live grid, and uploads the rectangle
that changed.

## PureCutCNC sources

Ported from PureCutCNC at `d04ae2f` unless a row says otherwise; the
survey is `docs/research/purecutcnc-simulation-digest.md` (sections in
brackets). A Rust file that transcribes a PureCutCNC file takes the
ported-file header (`AGENTS.md`).

| DreamCarve module | Source | What is taken, what changes |
| --- | --- | --- |
| `dc-sim::grid` | `src/engine/simulation/grid.ts`, `types.ts` [2] | Square cells counted along the stock's longer side, one height a cell (an `f64` here, upstream's is an `f32`), row-major from the lowest Y, the bottom as "cut through". Not taken: the 24-cell floor on each axis, which makes a narrow stock's grid reach past it [2.2]; stock-profile masking; `wrap`, `initialTopZ`. Added: a least cell size, a cell cap, the run that cut each cell. |
| `dc-sim::cutter` | `tools.ts` (`cutterSurfaceZ`) [3] | The flat, ball and V profiles, and the drill as a flat cylinder. Added: the tapered ball nose (DreamCarve's own, `dc_geometry::profile::BitProfile`), the engraving bit as a plain cylinder. |
| `dc-sim::sweep` | `crates/dc-cam/src/heightfield.rs` (DreamCarve's own, phase 3) and `replay.ts` (`applyMoveKernel`) [5] | The box of cells a move can reach, cells only ever lowered, nothing below the stock bottom, the inclusive changed rectangle: PureCutCNC's. The height itself is DreamCarve's: the lowest the cutter's surface reaches over the cell anywhere along the move, in closed form, where upstream takes the tip's height at the nearest point and so leaves a ramp up to `slope × radius` too high [5.3]. Arcs are moves here; upstream has only chords. |
| `dc-sim::build` | `replay.ts` (`replayItemsIntoGrid`, the chunked variant), `src/app/simulationCompute.ts` [5.6, 5.7, 7] | Items in cut order onto one grid, cancellable between chunks, never a part-built result. Added: a checkpoint after each run, progress, a build that starts at a given run. Not taken: Selected mode's lone operation on fresh stock [7.2]. |
| `dc-sim::playback` | `playback.ts` (`PlaybackController`, `subdivideMoves`) [8] | A cursor over the path and a live grid cloned from a base; pieces no longer than twice the cutter's radius; forward seeks cut from where they are, backward seeks restore and replay; the tip's pose; the dirty rectangle kept until taken. Changed: the cursor runs over the whole job and is placed by time, not by path length; a backward seek restores the nearest checkpoint, not the start; an advance is bounded by work done, not by a distance a frame. |
| `dc-app::sim::camera` | `src/components/viewport3d/orbitControls.ts`, `viewPresets.ts` [10.5] | The orbit about a target (two angles and a distance), the drag, pan and wheel rules, the presets, the fit of a box's eight corners, the clip planes that follow the distance. Re-derived for Z-up, Y-up: upstream renders sheet `(x, y, z)` at world `(x, z, y)`; here the world is project space. |
| `dc-app::sim::shade` | `heightfieldShader.ts` (`edgeIsStep`, the surface normal), `stepThreshold.ts` [9.4, 9.5] | The step-or-slope rule with its half-cell threshold and its factor of 4, and the gradient that leaves steps out, as Rust functions with upstream's tests. Upstream runs them in the fragment shader and tests the shader's text through a transpiler; here they run on the CPU into two small textures (open question 3). The zoomed-out widening is not taken. |
| `dreamcarve-app::sim::renderer` | `gpuMesh.ts`, `instancedBoundary.ts`, `simulationScene.ts` [9.1–9.3, 9.6, 9.8] | One flat quad a cell with no shared vertex, drawn a row at a time by instance; a wall on every grid edge, placed in the vertex stage from the height texture; the stock's sides as the edge walls; holes where a cell is cut through; the light rig; 12 % depth darkening; uploads of the changed rectangle only; drawing only when something changed. |
| `dreamcarve-app::sim::tool` | `toolMesh.ts` [9.7] | The cutter as a cylinder, a hemisphere or a cone with a shank 1.05 times as wide above it. Added: the tapered ball nose, turned from its profile. |

Not ported: rotary and wrapped grids, bar stock, plasma, the Bottom face
and setups, the CSG model view (digest, section 15); the main-thread
chunking and the reference-identity cache, which exist for the browser;
edge crossings and outline corners (issue #973) and winner-move shading
(issue #1209), which the grid's `cut_by` and sub-phase 6.4 stand in for
(functional open question 13).

Designed here, with no upstream: playing the whole job, the timeline in
seconds, colour by operation and by depth, the read-out, the design
surface and the check for uncut material, the report and the PNG.
Gouges are left to phase 7 (functional 121).

## Data model

### dc-sim (new crate)

```rust
/// How many cells: a named quality, or a size the CLI gives.
pub enum CellSpec { Quality(Quality), CellMm(f64) }
pub enum Quality { Draft, Standard, Fine }          // 512, 1024, 2048 cells on the longer side

pub const MIN_CELL_MM: f64 = 0.02;
pub const MAX_CELLS: usize = 1 << 24;               // 16 777 216; eight bytes of height each

/// Square cells over a board: cell (i, j) covers
/// [i·c, (i+1)·c) × [j·c, (j+1)·c) from the board's bottom-left corner,
/// row 0 at the lowest Y. The last column or row may reach past the
/// board by part of a cell. Fields are private: `Grid::of` holds them to
/// their ranges.
pub struct Grid { /* cell_mm, nx, ny, width_mm, height_mm */ }
impl Grid {
    pub fn of(width_mm: f64, height_mm: f64, spec: CellSpec) -> Result<Self, SimError>;
    pub fn centre(&self, i: u32, j: u32) -> Point;
    pub fn cell_at(&self, p: Point) -> Option<(u32, u32)>;
    pub fn on_board(&self, i: u32, j: u32) -> bool;   // its centre is on the board
    pub fn index(&self, i: u32, j: u32) -> Option<usize>;
}

/// The owner of a cell no run has cut, in `Heightfield::owners`.
pub const UNCUT: u16 = u16::MAX;

/// The heights a board's runs leave. A height is from the board's
/// bottom, never below 0 and never above `top_z_mm`.
pub struct Heightfield { /* grid, top_z_mm, z_mm: Vec<f64>, cut_by: Vec<u16> */ }
impl Heightfield {
    pub fn uncut(grid: Grid, top_z_mm: f64) -> Result<Self, SimError>;
    pub fn z_mm(&self, i: u32, j: u32) -> Option<f64>;
    pub fn cut_by(&self, i: u32, j: u32) -> Option<u16>;    // None: uncut, or no such cell
    pub fn is_through(&self, i: u32, j: u32) -> bool;        // at the bottom
    pub fn heights(&self) -> &[f64];  pub fn owners(&self) -> &[u16];   // what the renderer reads
    /// Cuts the part of `mv` between two fractions of its length, as run
    /// `owner`.
    pub fn cut(&mut self, mv: &SimMove, span: (f64, f64), cutter: &Cutter, owner: u16) -> Stamp;
    pub fn restore(&mut self, from: &Self) -> bool;          // a copy of a checkpoint on the same grid
}

/// Cells [i0, i1) × [j0, j1).
pub struct CellRect { pub i0: u32, pub i1: u32, pub j0: u32, pub j1: u32 }
/// What a cut did: the cells that changed and how many were tested.
pub struct Stamp { pub changed: Option<CellRect>, pub tested: u64 }

/// A cutter's surface. `radius_mm` is half the cutting diameter; nothing
/// is cut beyond it.
pub enum Cutter {
    Flat { radius_mm: f64 },
    Ball { radius_mm: f64 },
    Vee { slope: f64, radius_mm: f64 },                              // tan(α/2)
    TaperedBall { tip_radius_mm: f64, slope: f64, radius_mm: f64 },  // phase 3's profile
}
pub struct CutterShape { pub cutter: Cutter, pub plain: bool }       // plain: the kind holds no shape
impl CutterShape { pub fn of(tool: &Tool) -> Result<Self, SimError>; }
impl Cutter {
    /// The surface's height above the tip at `d_mm` from the axis; None past the radius.
    pub fn height_mm(&self, d_mm: f64) -> Option<f64>;
    /// The lowest the surface reaches over `q` while the tip runs a → b; None when never over it.
    pub fn low_over_mm(&self, a: Point3, b: Point3, q: Point) -> Option<f64>;
}

/// One move as the simulation reads it.
pub enum Motion { Cut, Travel }                     // Travel: a rapid or a dwell
pub enum Turn { Cw, Ccw }                           // seen from +Z, Y up
pub struct SimArc { pub centre: Point, pub turn: Turn }
pub struct SimMove {
    pub from: Point3, pub to: Point3,
    pub arc: Option<SimArc>,
    pub motion: Motion,
    pub seconds: f64,                 // on the timeline; 0 allowed
}
pub struct SimRun { pub cutter: CutterShape, pub moves: Vec<SimMove> }
impl SimRun { pub fn key(&self) -> u64; }           // FNV-1a of everything the simulation reads of it

pub enum Outcome { Finished(Heightfield), Stopped }
pub struct Progress { pub run: usize, pub moves_done: usize, pub moves: usize }
/// Cuts runs[from..] into `base`, which holds runs[..from]. Calls
/// `after_run` with the number of runs cut and the stock they leave (once
/// before anything, then after each run) and `keep_going` between chunks
/// of `CHUNK_CELLS` cell tests.
pub fn build(base: Heightfield, runs: &[SimRun], from: usize,
             after_run: &mut dyn FnMut(usize, &Heightfield),
             keep_going: &mut dyn FnMut(Progress) -> bool) -> Result<Outcome, SimError>;

pub struct Facts { pub deepest_mm: f64, pub deepest_run: Option<u16>, pub through_mm2: f64,
                   pub through_runs: Vec<u16>, pub removed_mm3: f64, pub plain_runs: Vec<usize> }
pub fn facts(field: &Heightfield, runs: &[SimRun]) -> Facts;
```

```rust
/// Where each move starts and ends in time.
pub struct Timeline { /* the time each move ends, per run */ }
/// Every move before (run, mv) is done; that move as far as `fraction`.
/// The end of the job is run `runs.len()`.
pub struct Cursor { pub run: usize, pub mv: usize, pub fraction: f64 }
impl Timeline {
    pub fn of(runs: &[SimRun]) -> Self;
    pub fn total_s(&self) -> f64;  pub fn run_span_s(&self, run: usize) -> Option<(f64, f64)>;
    pub fn at(&self, t_s: f64) -> Cursor;  pub fn time_of(&self, c: Cursor) -> f64;
}

/// The stock kept after runs, by how many runs have cut it, within a
/// byte budget (CHECKPOINT_BYTES = 256 MiB).
pub struct Checkpoints { /* BTreeMap<usize, Arc<Heightfield>> */ }
impl Checkpoints {
    pub fn keep(&mut self, runs_cut: usize, field: &Heightfield);
    pub fn get(&self, runs_cut: usize) -> Option<&Arc<Heightfield>>;
    pub fn at_or_before(&self, runs_cut: usize) -> Option<(usize, &Arc<Heightfield>)>;
    pub fn truncate(&mut self, runs_cut: usize);
}

pub struct Pose { pub tip: Point3, pub run: usize, pub cuts: bool }
pub struct Playback { /* runs, timeline, checkpoints, the finished stock, the live stock, the cursor, the dirty rect */ }
impl Playback {
    pub fn at_end(runs: Arc<[SimRun]>, checkpoints: Checkpoints, finished: Arc<Heightfield>) -> Self;
    /// Moves the cursor toward `t_s`, cutting what it passes, until it
    /// is there or `budget_cells` cell tests are spent; returns the time reached.
    pub fn seek(&mut self, t_s: f64, budget_cells: u64) -> f64;
    pub fn step_moves(&mut self, n: i64);  pub fn to_run_end(&mut self, run: usize);
    pub fn time_s(&self) -> f64;  pub fn cursor(&self) -> Cursor;
    pub fn is_at_start(&self) -> bool;  pub fn is_finished(&self) -> bool;
    pub fn pose(&self) -> Option<Pose>;                      // None at the start and at the end
    pub fn field(&self) -> &Heightfield;  pub fn take_dirty(&mut self) -> Option<CellRect>;
}
```

```rust
/// What the operations' shapes ask for (functional 119).
pub struct Design { /* grid, top_z_mm, z_mm: Vec<f64>, asked_by: Vec<u16> */ }
pub enum BandSide { Inside, Outside }
impl Design {
    pub fn uncut(grid: Grid, top_z_mm: f64) -> Self;
    pub fn z_mm(&self, i: u32, j: u32) -> Option<f64>;  pub fn asked_by(&self, i: u32, j: u32) -> Option<u16>;
    pub fn lower_regions(&mut self, regions: &[Region], z_mm: f64, op: u16);            // Pocket, Bore
    pub fn lower_band(&mut self, regions: &[Region], width_mm: f64, side: BandSide, z_mm: f64, op: u16);  // a trench beside an edge
    pub fn lower_near(&mut self, paths: &[Polyline], reach_mm: f64, z_mm: f64, op: u16);  // a trench along a path
    pub fn raise_near(&mut self, paths: &[Polyline], reach_mm: f64, z_mm: f64, op: u16);  // tabs in a trench
    pub fn lower_cone(&mut self, regions: &[Region], start_z_mm: f64, slope: f64, flat_mm: Option<f64>, op: u16);
}

pub const UNCUT_MM: f64 = 0.3;                      // the threshold unless the user changes it
pub const UNCUT_RANGE_MM: (f64, f64) = (0.05, 5.0);
pub const UNCUT_FLAG: u8 = 1;                       // a bit, so phase 7's gouge can be the next
pub const MIN_PLACE_CELLS: u32 = 4;
pub struct Place { pub op: Option<u16>, pub cells: u32, pub area_mm2: f64, pub height_mm: f64, pub bounds: Rect }
pub struct Comparison { pub flags: Vec<u8>, pub places: Vec<Place> }
pub fn compare(field: &Heightfield, design: &Design, uncut_mm: f64) -> Comparison;
```

`SimError` is the crate's one error: a cell size that passes `MAX_CELLS`
or is under `MIN_CELL_MM`, a board with no size or no thickness, more
runs than a cell can name, a tool with no shape (a tapered ball nose
with an unset value; validation refuses that project first, so a shell
never sees it).

Heights are `f64`, as `docs/rust-guidelines.md` asks of all geometry
("`f32` appears only in GPU vertex buffers at the renderer edge"): the
first draft of this spec had `f32`. A grid of the most cells is 134 MB
of heights. The engine's functions take what they change by value or as
`&mut self` of a value the caller owns, per the same guidelines. The crate has no warnings: what it finds is `Facts` and
`Comparison`.

### dc-job

```rust
// dc_job::simulate
pub struct Plan { pub boards: Vec<BoardPlan> }                 // the stock, then the male board if it is cut
pub struct BoardPlan { pub board: Board, pub width_mm: f64, pub height_mm: f64, pub thickness_mm: f64,
                       pub runs: Vec<PlannedRun> }
pub struct PlannedRun { pub op: OperationId, pub op_index: usize, pub label: RunLabel, pub tool: ToolId,
                        pub sim: SimRun, pub key: u64 }        // key: what `dc_app` digests
pub fn plan(project: &Project, generated: &[EmitOp], estimate: &EstimateConfig) -> Result<Plan, SimError>;
pub fn sim_moves(toolpath: &Toolpath, estimate: &EstimateConfig) -> Vec<SimMove>;
pub fn design(project: &Project, board: &BoardPlan, generated: &[EmitOp], grid: Grid) -> Design;

pub fn describe(project: &Project, board: &BoardPlan, field: &Heightfield, runs_cut: usize, facts: &Facts) -> Vec<String>;
pub fn describe_uncut(project: &Project, comparison: &Comparison, uncut_mm: f64, cell_mm: f64) -> Vec<String>;
pub fn heightmap_png(field: &Heightfield) -> Result<Vec<u8>, JobError>;   // 16-bit gray, top row the highest Y
pub fn is_heightmap(bytes: &[u8]) -> bool;                               // a picture `heightmap_png` wrote
```

`dc_job` re-exports `dc_sim` as `dc_job::sim`, as it re-exports the
toolpath types, so the GUI crates' manifests gain nothing.

### dc-app

```rust
// dc_app::sim
pub struct SimJob { pub snapshot: SnapshotId, pub board: Board, pub spec: CellSpec, pub from_run: usize,
                    pub base: Option<Arc<Heightfield>>, pub uncut_mm: f64 }
pub enum SimResult { Built { job: SimJob, built: Arc<Built> }, Stopped { job: SimJob } }
pub struct Built { pub plan: Arc<BoardPlan>, pub checkpoints: Checkpoints, pub finished: Arc<Heightfield>,
                   pub facts: Facts, pub design: Arc<Design>, pub comparison: Arc<Comparison> }

pub struct Simulation { /* per board: run digests, the current Built, stale and stopped flags, progress */ }
impl Simulation {
    pub fn next_job(&mut self, app: &App) -> Option<SimJob>;   // None unless the step is open and generation is idle
    pub fn accept(&mut self, result: SimResult) -> bool;       // dropped unless its snapshot is current
    pub fn state(&self, board: Board) -> SimState;             // Empty | Building(Progress) | Current | Stale | Stopped | Blocked
    pub fn stop(&mut self);  pub fn again(&mut self);
}
pub fn execute(project: &Project, generated: &[EmitOp], job: &SimJob, estimate: &EstimateConfig,
               keep_going: &mut dyn FnMut(Progress) -> bool) -> SimResult;   // the worker's one call

pub struct Transport { pub playing: bool, pub speed: Speed, pub playback: Playback }
impl Transport { pub fn frame(&mut self, dt_s: f64) -> bool; /* true while it wants another frame */ }
pub enum Speed { X1, X5, X25, X100, X500 }

pub struct OrbitCamera { pub target: Point3, pub yaw_rad: f64, pub pitch_rad: f64, pub distance_mm: f64,
                         pub viewport_px: Vec2 }
impl OrbitCamera {
    pub fn preset(&mut self, view: View3);  pub fn fit(&mut self, bounds: Box3);
    pub fn orbit(&mut self, d_px: Vec2);  pub fn pan(&mut self, d_px: Vec2);  pub fn zoom_about(&mut self, at: Pos, factor: f64);
    pub fn view_projection(&self) -> [[f32; 4]; 4];  pub fn ray(&self, at: Pos) -> Ray;
}
pub enum View3 { Home, Top, Front, Right }
pub fn probe(field: &Heightfield, ray: Ray) -> Option<(Point3, u16)>;      // the read-out: where the ray meets the stock

pub struct Rows { /* one per operation in cut order: colour, name, tool, time, state, its runs */ }
pub fn rows(app: &App, board: Board) -> Rows;
pub fn result(app: &App, board: Board) -> ResultView;          // the facts and the uncut list, as text and bounds
```

`Step` gains `Simulate` between `Operations` and `Export`; `Step::ALL`
has six. `steps::states` reads `Simulation::state`: Simulate is empty
or complete, never in need of attention. `Prefs` gains `sim: SimPrefs {
quality, colour, layers, uncut, uncut_mm, speed }`, written under `[simulation]` in `gui.toml`.
`View` is untouched: the 3D camera is the Simulate step's own and is not
saved. `Work` and `JobResult` are untouched: the simulation has its own
job type and its own worker.

No project type changes. `CURRENT_VERSION` stays 29.

## Algorithms

Conventions for all of them: project space, Y-up, Z-up, millimetres, Z
= 0 at the board's bottom (ADR-0001, and the toolpaths' own frame). The
grid has no flip and no handedness rule; the camera and the lights are
the only places a direction is chosen.

### The grid

`c = max(longer side / N, MIN_CELL_MM)` for a quality's `N`, or the
`--cell` value; `nx = ceil(width / c)`, `ny = ceil(height / c)`, each at
least 1; `nx · ny > MAX_CELLS` is `SimError::TooManyCells` naming the
smallest cell that fits. A cell's height is the stock's at its centre.
The last column and row may hang past the board; a cell whose centre is
off the board is cut like any other, is left out of `facts` and
`compare`, and is trimmed to the board's edge by the renderer.

### Cutter shapes

`CutterShape::of(tool)`: `EndMill` → `Flat`; `BallNose` → `Ball`;
`VBit` → `Vee` with `slope = tan(α/2)`; `TaperedBallNose` →
`TaperedBall` from `Tool::tip_radius_mm`, the taper's tangent and
`Tool::cut_radius_mm`; `Drill` and `Engraver` → `Flat` with `plain:
true`. `radius_mm` is `diameter_mm / 2` throughout.

`height_mm(d)`, `None` past the radius: flat `0`; ball `r − √(r² − d²)`;
V `d / slope`; tapered ball `BitProfile`'s inverse width (`ρ − √(ρ² −
d²)` inside the tangent circle, `d / slope − δ` on the cone).

### The cut of one move

For a straight move `a → b` and a cell centre `q`, the new height is

    low(q) = min over t in [0, 1] of  z(t) + h(|q − p(t)|)

with `h` the cutter's `height_mm` and `∞` past the radius. `h` is convex
and increasing, the distance is convex in `t`, so the least is at an end
of the move or at one stationary point. `Cutter::low_over_mm` is that
least in closed form:

- **Flat:** the lower end of the stretch of the move whose disc covers
  `q` (`disc_low_mm`).
- **V:** the stationary point of `z(t) + |q − p(t)| / slope`, clamped to
  the stretch within the radius (`cone_low_mm`, with the cap added).
- **Tapered ball:** the move split where the distance crosses the
  tangent circle and the radius, the ball's and the cone's stationary
  points on each piece (`tapered_low_mm`).
- **Ball:** the tapered ball's ball piece alone, with the tangent circle
  at the radius.

These are `dc_cam::heightfield`'s functions, moved with their tests. For
a level move every one reduces to `z + h(distance from q to the
segment)`, which is PureCutCNC's kernel.

`cut(field, mv, span, cutter, owner)`:

1. Take the part of the move between the two fractions of `span`.
2. **A level arc** is cut whole: the distance from `q` to the arc is
   `| |q − centre| − R |` where the angle of `q` about the centre is in
   the arc's sweep, else the distance to the nearer end. **An arc whose
   Z changes** is cut as chords within `CHORD_MM` (0.001 mm), each a
   straight move. The chords are the whole arc's, at fixed fractions of
   it, and a part of the arc is cut along them: a falling arc played in
   parts is then the whole to rounding, where chords of each part would
   differ from the whole's by up to the chord tolerance.
3. The box of cells: the move's XY box grown by the radius, as cell
   ranges clamped to the grid. A move whose lower end is at or above
   `top_z_mm`, or whose box misses the grid, returns `None` before any
   cell is read. For a V, the radius is first cut down to `(top −
   lowest tip) · slope` when that is smaller.
4. For each cell of the box: `z = max(low(q), 0)`; if it is below the
   cell's height, write it, set `cut_by` to `owner`, and grow the changed
   rectangle. A cell is never raised and never written below 0.

A pose inside a stretch the cutter covers is read no further out than
the rim. A move that falls as it leaves a cell is lowest over it at its
last pose, exactly on the rim, where rounding would otherwise put the
distance a hair past the radius and lose the cut; phase 3's closed form
had that flaw and slice 2's reference test found it.

A straight move's part is a straight move and an arc's part is an arc,
and `low` over a move is the least of `low` over its parts, so a move cut
in any number of pieces, in any order, leaves what the whole move leaves
(to rounding: a part is worked out from other end points). That is what
makes playback exact (acceptance 140), and upstream's part-played ramps
are not (digest, item 4 at its head). At the job's end `Playback` puts
the finished build in place of its own stock, so the end is the build to
the bit and the owner (acceptance 139).

`build` cuts each run's moves in order. The cut itself splits a line
whose box runs further than `max(2 · radius, 8 cells)` across both axes
into pieces, as upstream's playback splits moves, so a long diagonal's
box does not cover a square of cells it never touches; a move along an
axis has a tight box and is cut whole. It counts cell tests and calls `keep_going` every `CHUNK_CELLS`
(2²²); `false` returns `Stopped` with the field unusable, which the
caller drops.

**Determinism.** The kernel is a pure function of the field, the move
and the cutter. Within one platform a build is the same bytes whatever
its chunking or the run it starts from, and whole moves in any order
leave the same heights, because each cell's final height is a minimum of
the same set of values. Across platforms `tan`, `hypot`,
`sin` and `cos` (a tool's angle, a chord's end) may differ in their last
bit, so heights are compared within 0.0005 mm there (acceptance 143),
well under the 16-bit PNG's step.

### Runs in order, and what is reused

`dc_job::simulate::plan` walks `generated`, which is every enabled
operation's runs in the project's order with each operation's runs in
its own cut order. That is the order `dc_gcode::emit_programs` writes:
the program split only divides the sequence where the tool changes and
never reorders it ("Operations are never regrouped by tool, which would
change the cut order"), and it writes the stock's runs, then the male
board's. Each `EmitOp` becomes a `PlannedRun` on its `board`, the male
board's size and thickness read from `Project::on_male_board` as the
emitter reads them:
its toolpath's moves become `SimMove`s (`Rapid` and `Dwell` with `cuts:
false`), each with its seconds:

- a cut: its length (an arc's arc length, with its Z change as a helix)
  ÷ its `feed_mm_min`, × 60;
- a dwell: its seconds;
- a rapid: its length ÷ `EstimateConfig::rapid_mm_min` when that is set,
  else 0;
- all × `EstimateConfig::scale_factor`.

This is the estimator's arithmetic on the toolpath where the estimator
reads the written program, so a run's seconds differ from its
`RunSummary::estimate` by what only the program has: the spin-up dwell
and the effect of arc fitting on a length. The rows show the
estimate's time; the transport shows the timeline's.

A run's `key` hashes its moves, its cutter shape and its board's size.
`Simulation` keeps the keys of the last build by position; the first
position whose key differs is `from_run`, and the job carries the
checkpoint before it as `base`. A changed cell size or board changes
every key.

`Checkpoints` keeps the field after each run as an `Arc`. Past
`CHECKPOINT_BYTES` it drops every other one, then again, keeping the
first and the last; a seek that needs a dropped one restores the nearest
kept before it and cuts forward.

### The timeline and playback

`Timeline` is the running sum of the moves' seconds. `Playback::seek(t,
budget)`:

- **Forward:** cut from the cursor to the target, a move at a time, the
  move under way by its part `[fraction, new fraction]`; non-cutting
  moves only advance the cursor. Stop when `budget` cell tests are spent
  and return the time reached.
- **Backward:** copy the heights and owners of the checkpoint at or
  before the target's run into the live field, mark the whole grid
  dirty, then go forward.

`Transport::frame(dt)` asks for `time + dt × speed` with a budget of
`FRAME_CELLS` (2²¹, about 4 ms of kernel work), and sets its time to
what came back: on a job too heavy to keep up, the transport's clock
slows and the window does not. Guards on this count cell tests, never
milliseconds: upstream's timing ratio tracked machine load and failed
four of its builds, and it now counts work instead
(`src/engine/simulation/INDEX.md`, on `squarePlayback.test.ts`).

`step_moves(n)` moves the cursor to the end of the `n`-th cutting move
on; `to_run_end(run)` to the end of that run; both through `seek` with
no budget. `pose()` is the tip on the move under way; `None` before the
first move and after the last.

### The design surface

`dc_job::simulate::design` starts from `Design::uncut` and lowers it for
each enabled operation on the board, in any order (a cell keeps the
lowest asked of it and the operation that asked):

| Operation | Regions and height |
| --- | --- |
| Pocket | `lower_regions`: its targets' closed outlines as placed, nested into regions with their islands as holes, as the safety pass finds them, at `top − depth` |
| Profile, Inside or Outside | `lower_band`: a trench the tool's diameter wide beside the regions' edges, on the cut side, at `top − depth` (the bottom when it goes through). The band is found on the grid: the cells of that side whose centres are within the width of the edge, by the distance transform below, so the shape's own corners stay sharp |
| Profile, On line, Left or Right | `lower_near`: within the tool's radius of the paths (a Left or Right Profile's offset by the radius to its side first): what the tool sweeps |
| Profile's tabs | `raise_near`: the trench is kept at the tallest tab's top within the tool's radius, and a cell, of each stretch of the tool's path that rides over a tab. Those stretches are the preview's tab marks (`dc_job::preview::marks`), the one thing read from a generated run |
| Bore | `lower_regions`: each circle's disc at `top − depth` |
| V-Carve | `lower_cone` over its regions: `start − min(flat, d / slope)` with `d` the distance from the cell's centre to the regions' edge and `start` the top less the operation's start depth |
| Drill, Engrave, Inlay, Photo V-Carve | Nothing. The design stays as it is under them, so nothing they cut or leave is reported |

A tab is kept a tool's radius longer at each end than the 8 mm that
stands, where the tool's round end is still coming down: more than
stands, never less, so a tab is never reported. A Pocket's corner relief
is not asked for: it is a cut past the shape, which only a gouge check
would look at. A V-Carve seldom has anything to report: a carve too wide
for its bit is refused by S17 before it is cut, and with no clearing
pass the V-bit clears its own floor; what is left to find is a floor's
ridges over the threshold.

`lower_cone` needs each inside cell's distance to the boundary. It
rasterises the area to the grid (a cell is inside when its centre is),
then takes the exact Euclidean distance transform of the mask
(Felzenszwalb and Huttenlocher's two-pass lower-envelope method, O(cells),
written from the paper; `dc_sim::edt`), less half a cell: the mask's edge
is half a cell from the first inside centre on average. The error is
under a cell, which `compare`'s neighbourhood absorbs. A shape is laid on
the grid by scan lines over its outlines, arcs flattened to an eighth of
a cell.

The design is only ever at or below the uncut top, and the check reads
it one way: stock standing above it. Where no operation asks for a cut
it is the top, the stock is never above the top, and so nothing is
reported there. That is why uncut material needs no notion of part and
waste; a gouge check would, and phase 7 has a model to take it from.

### Compare

A cell on the board, with `N` its 3 × 3 neighbourhood and `H` the
finished field, is **uncut** when `H − max over N of design >
uncut_mm`.

The neighbourhood is the cell tolerance of functional 112: a wall that
the two grids place one cell apart is not a place. Flagged cells are
grouped into 8-connected components; one of fewer than `MIN_PLACE_CELLS`
(4) is dropped. A `Place` has its cells, its area (`cells · c²`), its
greatest height over the design, its box, and its operation, the
`asked_by` of its highest cell. Places sort by operation, then by area,
largest first.

### Shading data (`dc_app::sim::shade`)

Two things the renderer reads a cell at a time, computed on the CPU for
the changed rectangle grown by two cells:

- **Edge classes**, 2 bits a cell: whether the edge to the east
  neighbour and the edge to the north neighbour is a step.
  `edge_is_step(h_near, h_far, h_near_beyond, h_far_beyond, cell)` is
  upstream's rule: equal heights, no edge; either side cut through, a
  step; a change of at most half a cell, a slope; else a slope when the
  next edge on either side changes the same way and this one is at most
  4 times as steep; else a step.
- **Gradients**, two `i16` a cell: the height's change per cell along X
  and Y at the cell's centre, a central difference in which an edge that
  is a step contributes nothing.

The fragment stage lights a top from the gradients of the four cells
round it, blended by position, taking a neighbour's only across an edge
that is a slope; a wall that is a step from its own face; a riser inside
a slope from the surface's gradient. So a flat top stays flat to its
rim, and a V-carve's walls and a ball's scallops shade smoothly.

### The camera and the read-out

`OrbitCamera` is upstream's spherical orbit in a Z-up world: the eye is
at `target + distance · (cos pitch · sin yaw, −cos pitch · cos yaw, sin
pitch)`, so yaw 0 looks from the operator's side (−Y) toward +Y. Pitch
is held to `[0, π/2]`: level with the bed to straight down. Perspective,
45° vertical. Presets: Home `(yaw π/4, pitch π/6)`, from the front
right and 30° above the bed, upstream's isometric; Top `pitch π/2` with
+Y up the screen; Front `yaw 0,
pitch 0`; Right `yaw π/2, pitch 0`. A preset keeps the distance; `fit`
keeps the angles and takes the least distance at which the box's eight
corners are inside the view, times 1.1, and clear of the floating
controls as phase 4's Fit is. Orbit is 0.01 rad a pixel; the wheel is
`exp(Δ · 0.0015)` about the point under the pointer; near is 1 % of the
distance, far reaches past the scene.

The matrix arithmetic (look-at, perspective, a product, an inverse for
the ray) is a module of about 150 lines in `dc-app`, in `f64`, with no
new dependency (open question 2).

`probe` walks the ray's XY shadow across the grid a cell at a time (a
2D DDA) from where the ray enters the board's box, and returns the first
cell whose height the ray is at or below, with the point and the cell's
`cut_by`. The read-out shows that point from the machine origin and the
run's operation.

### The renderer

`dreamcarve_app::sim::renderer::StockRenderer` owns an off-screen colour
target with a `Depth32Float` buffer, 4× multisampled and resolved, the
size of the Viewport's card in physical pixels. The resolved texture is
`Rgba8Unorm` holding sRGB-encoded colours, which is what egui 0.37
expects of a texture it samples (its shader reads one as "not
sRGB-aware"; the first build used an sRGB target and drew the stock a
good deal too dark). It is registered with egui by
`egui_wgpu::Renderer::register_native_texture` and painted as the card's
image. The scene is drawn into it from the
frame, with the window's own device and queue (a clone of eframe's
`RenderState`), and only when the camera, the grid, a preference or the
theme changed: the renderer keeps what it last drew and skips a frame
that would draw the same. The 2D pipelines and the egui pass are
untouched; turning on eframe's `depth_buffer` instead would put a depth
attachment on every pipeline in that pass, the 2D Viewport's included.

Textures, all `nx × ny`, read with `textureLoad` and never filtered:
heights `R32Float`; owners `R16Uint`; edge classes and the check's flag
packed in one `R8Uint`; gradients `Rg32Float` (a 60° V's slope is 1.7,
past what a signed-normalised texture holds, and `Rg16Snorm` is a wgpu
feature, not a format every device has). Each is written with
`Queue::write_texture` for the changed rectangle only. A grid with more
cells along a side than the device's largest texture is not drawn, and
the card says to choose a coarser Quality.

There are no vertex or index buffers: every corner comes from the
vertex's number and the instance's. Draws, in order, in one shader
(`stock.wgsl`):

1. **The bed**: a quad just under Z = 0, a margin wider than the board
   (5 % of its longer side, at least 10 mm). Left out when Layers ▸ Bed
   is off.
2. **Tops**: `nx × 6` vertices, drawn `ny` times by instance. The vertex
   stage loads the cell's height and places its corner, trimmed to the
   board where the last column or row hangs past it; a cell that is cut
   through is placed outside the clip volume. The fragment stage takes
   its colour from the mode (material; the owner's operation colour from
   a table a run, which grows with the plan; the depth ramp's texture,
   from the top to the job's deepest cut as phase 4's ramp is), lays the
   check's tint over it (uncut in the warning colour, 60 %), lights it
   and darkens it by depth.
3. **Walls**: the edges between east–west neighbours (`nx + 1` a row,
   `ny` instances) and between north–south ones (`nx` a row, `ny + 1`
   instances). The vertex stage loads the two cells either side (outside
   the grid reads as 0, which makes the stock's own sides) and spans the
   wall from the higher to the lower; equal heights give no area. A wall
   that is a step is lit by its own face; a riser inside a slope by its
   cell's gradient. A wall takes the colour of the cut that made it,
   the lower cell's owner, so a through cut's walls show By operation
   though it has no floor; By depth colours every point by its own
   depth, so a wall shades down its height.
4. **The tool**, when the playback has a pose: `dc_app::sim::tool_mesh`,
   one profile turned about the axis for every cutter (24 segments), its
   underside the simulation's own `Cutter::height_mm`, with a shank of
   1.05 times the radius and two diameters long above a cutting length
   of 3 diameters.
5. **Lines** (`overlay.wgsl`, with the tool): the toolpath and rapids of
   the operation being shown, and the origin's three axes, 5 % of the
   board's longer side. A quad a segment from an instance buffer,
   widened on the screen to 1.5 px, clipped to the plane in front of
   the eye, dashed for a rapid, and lifted a hair toward the eye so a
   cut's path is not lost in its own floor. The buffer is made again
   only when the operation shown, the layers or the theme change.

Light: upstream's rig turned to this world, ambient 0.7, a white key of
0.9 from `(120, −120, 180)` and a blue-white fill of 0.35 from `(−120,
80, 80)`, diffuse only; darkening `1 − 0.12 · depth / thickness`. The
light is worked in the theme's sRGB-encoded colours, as upstream's is,
and written as it is.

A wgpu object's debug label is an id (`sim.tops`): the shell's string
scan reads every literal as something a user might read.

### The worker

`dreamcarve-app` gains a second worker thread for simulation, so a long
build never holds up a toolpath. It takes a `SimJob`, the snapshot's
project and its generated runs, calls `dc_app::sim::execute`, and sends
the `SimResult` back. `keep_going` reads an `AtomicBool` the shell sets
on *Stop*, on a newer job, and on any change to the project.
`Simulation::next_job` gives a job only while the Simulate step is open,
`Generation::busy()` is false and `finished()` has no error.

### `dreamcarve simulate`

`crates/dreamcarve/src/simulate.rs`: read and validate the project as
`export` does; generate every enabled operation (`dc_job`'s build path,
stopping on an error as `check` reports it); `plan`; for the chosen
board `Grid::of`, `build` (to `--until`'s last run when given),
`facts`, `design`, `compare`, the report's lines; print them; with
`--out`, `heightmap_png` through `dreamcarve-shell`'s staged write. The
path is checked before anything is read: a `.png`, not the project, and
holding nothing or a picture `is_heightmap` knows as its own (a
`Software` text chunk, as the preview SVG is known by its marker). A
pixel is `round(z / thickness · 65535)`; the PNG's
first row is the grid's last, since a picture's top is the highest Y.

## Module layout

```
crates/dc-sim/src/{lib.rs, error.rs, grid.rs (port), field.rs, cutter.rs (port), sweep.rs (moved from dc-cam; the level-arc distance), cut.rs (port: the box, the write, the rectangle), build.rs (port), timeline.rs, checkpoints.rs, playback.rs (port), design.rs, edt.rs, compare.rs, facts.rs}
crates/dc-cam/src/heightfield.rs — keeps `Heightfield`'s mirror and the seat check for its tests, on dc-sim's `Cutter::low_over_mm`
crates/dc-job/src/simulate.rs (plan, design, report, heightmap_png) and `pub use dc_sim as sim`
crates/dc-app/src/sim.rs and sim/{job.rs (SimJob, Simulation, execute), session.rs (the app's hand-over of jobs and results, what blocks a simulation), transport.rs, camera.rs (port), mat.rs, probe.rs, shade.rs (port), tool.rs (port: the tool's mesh), rows.rs (the rows, the summary, the Result's lines, the read-out), prefs.rs}; steps.rs, prefs.rs, generation.rs (`current_runs`), strings/en.toml
crates/dreamcarve-app/src/sim/{mod.rs (the step's Viewport: the view's state, the gestures, the keys), renderer.rs (port), stock.wgsl (port: the bed, the tops and the walls), controls.rs (the views, Colour, the transport, the read-out)}, panels/simulate.rs (the step panel), shell/sim_worker.rs, shell/rail.rs, keyboard.rs
crates/dreamcarve/src/simulate.rs; cli.rs
crates/dreamcarve-shell/src/gui_prefs.rs ([simulation])
tools/parity/sim-capture.ts and tests/golden/sim-*/ (slice 5)
tests/fixtures/simulate/{coaster,plaque,sign,hook-plate}.{png,txt}
docs/specs/phase-6/{functional.md, design.md, review/}
docs/book/src/{quick-start/simulate.md, user-guide/simulation.md}
```

The crate graph in `docs/rust-guidelines.md` gains `dc-sim`:

```
dc-geometry → dc-project → dc-sim → dc-job → dc-app → dreamcarve-app
dc-sim ⇢ dc-cam   (dev-dependency and `test-support` only)
```

`dc-cam` lists `dc-sim` as an optional dependency its `test-support`
feature turns on, and as a dev-dependency; a manifest test holds that no
other entry names it and that `dc-gcode` does not list it (S22).
`dc-sim` follows every `dc-*` rule: no I/O, no threads, no panic, units
in names, `Result` with its own error.

Dependencies: no crate new to the workspace. `dc-job` lists `png`,
which `dc-project` and `dc-import` already use, for the 16-bit picture;
`rayon` is not used (open question 5).

## Visual design

`experience.md` is the authority and gains, in the slices that draw
them:

- **Tokens** (rows of its Viewport table, which the theme's test
  reads): `sim.stock` (the material), `sim.bed`, `sim.tool` and
  `sim.tool.shank`, their values chosen from slice 7's pictures in both
  themes. Upstream's stock is a grey-blue (`#C2CAD4` light, `#B5BECA`
  dark) and its tool `#D9DDE3` over a `#7D8591` shank. Uncut material uses `status.warning`;
  By operation uses the operation colours; By depth the depth ramp.
- **The rail** has six steps. Simulate's icon is Lucide's `rotate-3d`.
  Section 2's "A sixth step" is this.
- **The transport** is a 40-px `surface.raised` pill, 12 px above the
  card's bottom edge, centred: Previous, Play or Pause, Next, the
  scrubber (at least 240 px, operations marked on it as ticks), the two
  times in tabular figures, and Speed as a quiet button that opens its
  five choices. It eases to 60 % while the stock is dragged, as the other
  floating controls do.
- **The top-left pill** holds Home, Top, Front, Right and Fit; the
  top-right pill, Colour's three-way control. The bottom-left read-out
  is phase 4's with a Z and the operation's name.
- **The step panel** follows section 2's rule for a step: the summary
  line, then the rows, then two sections, Result and Display, with
  their values as summaries when closed.

The review checklist of `experience.md` is applied to every slice's
pictures.

## Test plan

Names are `crate::module::test`; "(port)" marks a PureCutCNC test ported
under a name that says what it shows, with its source; "(acc N)" the
acceptance test it carries; "(moved)" a `dc-cam` test that moves with its
function.

**The grid and the cutters (slice 1)** `dc_sim::grid::`
`cells_are_counted_along_the_longer_side`,
`a_small_stock_is_held_to_the_least_cell`,
`a_narrow_stock_has_no_cells_beyond_it` (upstream's 24-cell floor is not
ported), `a_part_cell_is_on_the_board_only_by_its_centre`,
`too_many_cells_name_the_cell_that_fits` (acc 131),
`a_board_or_a_cell_that_is_no_size_is_refused`, `centres_and_cells_agree`,
`a_span_is_the_cells_whose_centres_it_holds`; `field::`
`uncut_stock_is_at_its_top_and_cut_by_nothing`,
`cells_only_go_down_and_never_below_the_bottom` (acc 138),
`a_board_with_no_thickness_is_refused`; `cutter::`
`each_tool_kind_has_its_shape`,
`a_drill_and_an_engraver_are_plain_cylinders` (acc 137),
`heights_are_the_profiles` (port: `cutterSurfaceZ`'s table),
`a_tapered_ball_nose_is_ball_then_cone`, `nothing_is_cut_past_the_radius`,
`a_tool_with_no_size_has_no_shape`; `tests/manifest.rs` (S22): `dc-sim`
knows nothing above the project, the post-processor does not list it,
the generators list it for tests only and name it in one gated module.

**The cut (slice 2)** `dc_sim::sweep::`
`tapered_descending_move_is_exact` (moved from `dc-cam`),
`every_shape_is_cut_to_its_lowest_pose` (acc 136; each shape against a
10⁵-pose brute force), `a_ramp_is_cut_to_the_lowest_pose_not_the_nearest`
(upstream's ramp, with the exact answer),
`a_level_move_is_the_profile_at_the_nearest_point`,
`a_v_deeper_than_its_cone_reaches_no_further_than_its_radius`;
`cut::tests::` `a_flat_end_mill_cuts_a_slot_at_any_angle` (acc 132),
`a_ball_nose_leaves_its_arc` (acc 133),
`a_v_bit_leaves_its_cone_and_a_wall_past_it` (acc 134),
`a_tapered_ball_nose_leaves_ball_cone_and_wall` (acc 135),
`the_kernel_matches_the_reference_profile` (port:
`testKernelMatchesReferenceProfile`, for a flat r 1.5, a ball r 2, a V
r 3 at 60° and 90°, a drill r 1.25, over a ramp, an axis cut, a plunge,
a lead and a cut below the bottom),
`the_changed_rectangle_holds_every_changed_cell` (port:
`testDirtyRegionCoversChangedCells`),
`a_move_off_the_grid_or_above_the_stock_reads_no_cell`,
`a_level_arc_is_cut_at_its_true_distance`,
`a_helix_is_cut_within_a_micron_and_leaves_a_round_hole` (acc 136),
`a_part_of_a_move_is_cut_to_where_the_tip_is`,
`a_long_diagonal_is_cut_in_pieces`, `a_moves_length_and_points`; the
properties (`tests/properties.rs`) `a_move_in_pieces_equals_the_move`
and `moves_in_any_order_leave_the_same_field`; `dc_cam`'s seat and carve
tests unchanged and green.

**Runs and reuse (slice 3)** `dc_sim::build::`
`a_build_from_a_checkpoint_equals_a_build_from_nothing` (acc 144),
`the_owner_is_the_last_run_to_lower_a_cell` (acc 151),
`a_stopped_build_stops_within_a_chunk` (acc 145),
`a_build_of_nothing_is_the_base`; `checkpoints::`
`the_latest_at_or_before_is_found`,
`dropping_keeps_the_first_and_the_last`; `facts::`
`uncut_stock_has_no_facts`, `removed_volume_of_a_slot`,
`the_deepest_cut_and_the_through_cut_name_their_runs`; `digest::`
`the_hash_is_fnv_1a`, `every_input_changes_the_key`;
`dc_job` `tests/simulate.rs`: `the_plan_is_the_runs_export_writes` on
all six post-processors and both splits, for the plaque, the sign and
the inlay pair (acc 142), `a_rapid_and_a_dwell_cut_nothing`,
`a_runs_seconds_are_its_lengths_over_its_feeds`,
`each_board_has_its_own_runs` (acc 150),
`a_runs_key_follows_what_the_simulation_reads` (acc 144),
`the_plaques_tabs_stand` (port: `tabbedEdgeSimulation.test.ts`, on the
plaque at two qualities; acc 138; the plaque stands in for the coaster,
which has no committed project).

**Playback (slice 4)** `dc_sim::timeline::`
`a_runs_span_is_its_moves_seconds`, `time_and_cursor_round_trip`,
`a_zero_second_move_is_passed`; `playback::tests::` the ports of
`playback.test.ts`: `a_slow_move_takes_longer`,
`forward_seeks_equal_one_seek`,
`a_backward_seek_restores_the_earlier_field`,
`the_dirty_rectangle_grows_until_taken` (a backward seek dirties the
grid); upstream's two subdivision tests are not ported, since the cut
splits moves here; and `a_job_opens_at_its_end`,
`playing_in_any_steps_ends_at_the_build` for steps of the job over 7,
53, 197 and 1000 (acc 139; upstream's step counts, on ramps, a helix
and arcs), `part_way_is_the_build_of_the_moves_so_far` (acc 140),
`to_run_end_is_the_runs_checkpoint`, `a_step_is_one_cutting_move`,
`a_spent_budget_returns_the_time_reached`,
`the_pose_is_none_at_both_ends`, `an_empty_job_plays`.

**The command (slice 5)** `dreamcarve` `tests/cli/simulate.rs`:
`the_report_and_the_picture_of_each_fixture` (acc 153; the plaque, the
sign, the inlay's male board), `a_file_that_is_not_its_picture_is_kept`,
`a_refused_project_is_refused_as_check_refuses_it`,
`until_stops_after_the_operation`, `the_male_board_is_its_own_picture`
(acc 150), `a_cell_too_small_is_refused` (acc 131),
`uncut_material_is_listed_by_operation` (acc 147); `dc_job`
`tests/simulate.rs`: `a_heightmap_is_sixteen_bit_top_row_first`,
`the_plaque_measures_as_it_is_drawn` (acc 154). The parity capture of
PureCutCNC's grids (`tools/parity/sim-capture.ts`) is not done: it needs
`npm install` in the reference checkout, which is the maintainer's to
run (the worklog).

**The app core (slice 6)** `dc_app` `tests/steps.rs`:
`there_are_six_steps`, and *Continue* and the opening step with a sixth
(`continue_goes_to_the_next_step_that_is_not_complete`,
`continue_wraps_to_an_earlier_step_and_ends_on_export`,
`a_project_opens_on_its_first_step_that_is_not_complete`);
`tests/sim.rs`: `a_job_is_given_only_with_generation_idle`,
`a_change_starts_from_the_first_changed_run` (acc 144),
`an_old_snapshots_result_is_kept_but_is_not_current` (acc 145; the
result is kept to show and to build on, see the worklog),
`undo_brings_the_earlier_result_back`, `stop_holds_until_simulate_again`,
`another_quality_is_another_simulation`,
`nothing_enabled_and_a_refused_project_are_blocked`,
`simulate_is_empty_then_complete_and_never_needs_attention` (acc 155),
`rows_are_in_cut_order_and_follow_the_playback`; `sim::camera::`
`the_presets_look_the_way_their_names_say`,
`fit_frames_the_eight_corners`, `no_drag_goes_under_the_bed` (acc 152),
`pan_and_zoom_keep_what_is_under_the_pointer`,
`a_ray_and_the_matrix_agree_with_the_screen`; `sim::mat::` four on the
view, the perspective and the product; `sim::probe::`
`reads_a_slots_floor_and_its_run`,
`a_slanting_ray_meets_what_is_in_front`, `nothing_is_read_off_the_stock`;
`sim::transport::` `a_job_opens_at_the_speed_that_plays_it_in_half_a_minute`,
`play_runs_from_the_start_to_the_build`,
`the_scrubber_and_the_rows_place_the_job`; `sim::rows::`
`a_row_is_ahead_cutting_or_done`, `the_clock_reads_minutes_and_seconds`,
`a_fraction_is_a_whole_percentage_that_never_reads_done_early`;
`sim::prefs::` its three; `dreamcarve_shell::gui_prefs::`
`simulation_defaults_when_missing` and `round_trip`;
`dreamcarve_app::files::` `prefs_round_trip_through_the_file_record`,
`simulation_from_a_file_is_held_to_what_the_app_can_use`;
`dreamcarve::gui_equality::simulating_changes_no_program` (acc 156: the
plaque, the sign and the inlay pair, built, stopped and never opened).

**Shading and the tool (slices 7 and 8)** `dc_app::sim::shade::` the
ports of `heightfieldShader.test.ts`:
`a_tab_bridge_and_a_cut_through_rim_are_steps`,
`walls_and_slopes_keep_their_class` (0.4 and 0.5 of a cell are slopes,
0.6 a step, at cells of 0.1, 1, 2 and 2.5), `a_shallow_scallop_is_a_slope`,
`the_gradient_is_the_slope_and_leaves_steps_out`,
`shading_a_changed_rectangle_equals_shading_all`; `sim::tool::`
`the_cutter_is_drawn_in_the_shape_that_cuts`,
`the_shank_is_a_touch_wider_and_two_diameters_long`,
`normals_are_unit_and_point_out`, `a_cutter_of_no_size_has_no_mesh`.

**The window (slices 7–9, 12)** `dreamcarve_app::sim::renderer::`
`the_globals_match_the_shader`, `the_class_bits_match_the_shader`,
`one_top_a_cell_and_a_wall_an_edge` (the counts upstream's
`gpuMesh.test.ts` and `instancedBoundary.test.ts` hold, here the draws'
vertex and instance counts), `only_the_changed_rectangle_is_sent`;
`tests/simulate.rs`: `simulate_lists_the_operations_in_cut_order`,
`play_advances_and_pause_holds`, `a_row_click_dims_the_rows_after_it`,
`quality_colour_and_layers_are_preferences`,
`the_male_board_is_offered_with_an_inlay`,
`the_uncut_list_frames_its_places` (acc 147 in the window),
`a_frame_of_the_plaque_is_not_blank_and_changes` (acc 157, through
wgpu); `tests/kittest.rs`:
`the_rail_has_six_steps_and_a_project_opens_on_its_first_incomplete_one`,
`continue_goes_to_the_next_step_and_export_is_the_primary_on_export`
(acc 155 in the window), `keyboard_shortcuts_reach_their_commands` (acc
158: every key of 126), `file_menu_labels_in_order`,
`focus_order_and_visible_ring`; `keyboard::tests::`
`chapter_table_matches_the_shortcut_table`. The review pictures are
`tests/review.rs`'s `phase6_review`, the book's `tests/book.rs`'s
`simulation_screenshots`.

**The check (slices 10–11)** `dc_sim::edt::`
`distances_equal_brute_force_on_random_masks`,
`a_bars_cells_count_from_its_edge`, `a_full_mask_is_far_from_any_edge`;
`design::` `a_region_is_lowered_and_its_island_left`,
`the_lowest_asked_wins_and_stays_in_the_board`,
`a_band_is_the_tools_width_beside_the_edge`,
`a_path_is_lowered_within_reach`, `a_tab_is_raised_in_its_own_trench`,
`lower_cone_is_distance_over_slope_capped`,
`the_design_is_never_above_the_top`; `compare::`
`a_square_pocket_has_four_corners`,
`nothing_is_reported_where_no_shape_asks`,
`a_wall_one_cell_off_is_not_a_place`, `ridges_follow_the_threshold`,
`small_patches_are_dropped_and_places_name_their_operations`; `dc_job`
`tests/simulate.rs`: `each_operation_asks_for_its_table_row` (acc 146,
on the plaque), `a_v_carve_asks_for_its_cone_to_the_flat_depth` (acc
146, on the sign), `a_square_pocket_has_four_corners_and_none_with_dogbones`
(acc 147 and 148: the plaque, the sign and the inlay pair report
nothing; the plaque with its relief off, four corners).

## Slices

Ordered; brackets are dependencies. Each ends with the gate green and a
row in `docs/phase-6-worklog.md`. Tests first.

**6.1 The engine**
1. **`dc-sim`: the grid and the cutters.** The crate, `Grid`,
   `Heightfield`, `Cutter`, `CutterShape::of`, the guidelines' crate
   graph. [—]
2. **The cut.** `sweep` moved from `dc-cam` with its tests and the ball
   and level-arc cases added; `cut`; `dc-cam`'s test heightfield on top
   of it. [1]
3. **Runs in order.** `SimMove`, `SimRun`, `build` with checkpoints,
   progress and stop; `facts`; `dc_job::simulate::plan` and the
   re-export. [2]
4. **The timeline and playback.** [3]
5. **`dreamcarve simulate`.** The command, the report, the PNG, the four
   fixtures' committed results; `tools/parity/sim-capture.ts` writing
   PureCutCNC's grid for a handful of the golden cases whose moves are
   level, with each case's tolerance in its README. [3]

**6.2 The 3D view**
6. **The app core.** `Step::Simulate` and every place that counts five
   steps, `dc_app::sim` (the job, the transport, the camera, the probe,
   the rows), the preferences. [3, 4]
7. **The window, the stock.** The simulation worker, the off-screen
   target, the height texture, tops and walls, the shading data, the
   bed, the camera's gestures and presets, the step panel with Quality,
   the tokens, the first review pictures. [6]
8. **The window, playback.** The transport, the scrubber, Speed, the
   rows' clicks, Previous and Next, the tool, the rectangle uploads, the
   keys. [7]
9. **Colour, layers, the male board, the read-out.** [7]

**6.3 Checking the result**
10. **The design surface.** `Design`, `edt`, `dc_job::simulate::design`
    for each operation. [3]
11. **Compare.** `compare`, the places, the report's lists, the facts in
    the report. [10]
12. **The window, the check.** The flags texture and its tint, the
    Result section, a row's click framing its places. [9, 11]

**6.4 Release**
13. **Looking at it.** The review pictures of the test parts at each
    quality; from them, whether walls on cell edges are good enough or
    upstream's outline corners (#973) are ported as a slice of their
    own; a count of cell tests and a wall-clock time for each test part
    at each quality, recorded in the worklog; an optimisation only with
    the benchmark the guidelines ask for. [7–12]
14. **The book and the exit.** The quick start, the Simulation chapter
    and the updated ones, the screenshots, `CONTEXT.md`, the changelog,
    the bench check's table as far as it can be filled, the checks CI
    runs that the gate does not, the exit-criteria audit. [1–13]

## Open questions

The functional spec's table holds the product questions. Design
questions still open:

| # | Question | Taken for now |
| --- | --- | --- |
| 1 | Does the design surface deserve an ADR? | No. It is display only, nothing is saved from it, and it can change without a migration. ADR-0003's consequence that resolution is a user setting with a budget is met by Quality and `MAX_CELLS`. |
| 2 | Matrix arithmetic: a module of our own, or `glam`? | Our own, about 150 lines in `f64`, tested against the presets. `glam` is not in the tree. |
| 3 | The step rule and the gradient on the CPU into textures, where upstream runs them in the fragment shader. | The CPU: the ported tests then exercise the code that ships, at 3 more bytes a cell and a small pass over each changed rectangle. Revisit if the upload shows in a profile. |
| 4 | The off-screen target, or eframe's `depth_buffer`? | Off-screen: the 2D pipelines, the egui pass and the test harness stay as they are, and a test can read the target back. |
| 5 | Should `build` cut a run's rows in parallel? The result is order-free, so it could. | Serial. `docs/rust-guidelines.md` asks for a benchmark and a same-output test first; slice 13 measures. |
| 6 | Phase 4's design left "a cancellation point in `dc-cam`" to this phase, "when simulation adds longer jobs". | Not taken up: the simulation has its own worker and its own stop, so it adds no wait to a generator. Stop on a generator stays as phase 4 left it. |
| 7 | `dc-sim` below `dc-cam`, with its own move type, or above it, reading `Toolpath`? | Below: `dc-cam`'s tests then use the one kernel, and the conversion from a `Toolpath` is a dozen lines in `dc-job`. |
| 8 | The parity capture of slice 5 needs Node and the PureCutCNC checkout, as the other goldens do, and upstream's ramps differ from ours by design. | Level-move cases only, at upstream's default detail, with the tolerance each case's README states. |
