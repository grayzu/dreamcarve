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
| `dc-sim::grid` | `src/engine/simulation/grid.ts`, `types.ts` [2] | Square cells counted along the stock's longer side, one `f32` height a cell, row-major from the lowest Y, the bottom as "cut through". Not taken: the 24-cell floor on each axis, which makes a narrow stock's grid reach past it [2.2]; stock-profile masking; `wrap`, `initialTopZ`. Added: a least cell size, a cell cap, the run that cut each cell. |
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
pub const MAX_CELLS: usize = 1 << 24;               // 16 777 216; four bytes of height each

/// Square cells over a board: cell (i, j) covers
/// [i·c, (i+1)·c) × [j·c, (j+1)·c) from the board's bottom-left corner,
/// row 0 at the lowest Y. The last column or row may reach past the
/// board by part of a cell.
pub struct Grid { pub cell_mm: f64, pub nx: u32, pub ny: u32, pub width_mm: f64, pub height_mm: f64 }
impl Grid {
    pub fn of(width_mm: f64, height_mm: f64, spec: CellSpec) -> Result<Self, SimError>;
    pub fn centre(&self, i: u32, j: u32) -> Point;
    pub fn cell_at(&self, p: Point) -> Option<(u32, u32)>;
    pub fn on_board(&self, i: u32, j: u32) -> bool;   // its centre is on the board
}

/// The run that cut a cell last, as its place in the plan.
pub const UNCUT: u16 = u16::MAX;

/// The heights a board's runs leave. A height is from the board's
/// bottom, never below 0 and never above `top_z_mm`.
pub struct Heightfield {
    grid: Grid, top_z_mm: f64,
    z_mm: Vec<f32>,        // row-major; `top_z_mm` where nothing has cut
    cut_by: Vec<u16>,      // UNCUT, or the run that lowered the cell last
}
impl Heightfield {
    pub fn uncut(grid: Grid, top_z_mm: f64) -> Self;
    pub fn z_mm(&self, i: u32, j: u32) -> f32;  pub fn cut_by(&self, i: u32, j: u32) -> u16;
    pub fn is_through(&self, i: u32, j: u32) -> bool;        // z == 0
    pub fn heights(&self) -> &[f32];  pub fn owners(&self) -> &[u16];   // what the renderer uploads
}

/// Cells [i0, i1) × [j0, j1).
pub struct CellRect { pub i0: u32, pub i1: u32, pub j0: u32, pub j1: u32 }

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
    /// The lowest the surface reaches over `q` while the tip runs a → b; ∞ when never over it.
    pub fn low_over_mm(&self, a: Point3, b: Point3, q: Point) -> f64;
}

/// One move as the simulation reads it.
pub struct SimMove {
    pub from: Point3, pub to: Point3,
    pub arc: Option<SimArc>,          // centre and direction, Y-up
    pub cuts: bool,                   // false: a rapid or a dwell
    pub seconds: f64,                 // on the timeline; 0 allowed
}
pub struct SimRun { pub cutter: CutterShape, pub moves: Vec<SimMove> }

/// Cuts `mv` between two fractions of its length into `field`, as run
/// `owner`. Returns the cells that changed.
pub fn cut(field: &mut Heightfield, mv: &SimMove, span: (f64, f64), cutter: &Cutter, owner: u16) -> Option<CellRect>;

pub enum Outcome { Finished, Stopped }
pub struct Progress { pub run: usize, pub moves_done: usize, pub moves: usize }
/// Cuts runs[from..] into `field`, which holds runs[..from]. Calls
/// `after_run` with the field after each run and `keep_going` between
/// chunks of `CHUNK_CELLS` cell tests.
pub fn build(field: &mut Heightfield, runs: &[SimRun], from: usize,
             after_run: &mut dyn FnMut(usize, &Heightfield),
             keep_going: &mut dyn FnMut(Progress) -> bool) -> Outcome;

pub struct Facts { pub deepest_mm: f64, pub deepest_run: Option<u16>, pub through_cells: u32,
                   pub removed_mm3: f64, pub plain_tools: Vec<usize> }
pub fn facts(field: &Heightfield, runs: &[SimRun]) -> Facts;
```

```rust
/// Where each move starts and ends in time.
pub struct Timeline { /* cumulative seconds per run and per move */ }
pub struct Cursor { pub run: usize, pub mv: usize, pub fraction: f64 }
impl Timeline {
    pub fn of(runs: &[SimRun]) -> Self;
    pub fn total_s(&self) -> f64;  pub fn run_span_s(&self, run: usize) -> (f64, f64);
    pub fn at(&self, t_s: f64) -> Cursor;  pub fn time_of(&self, c: Cursor) -> f64;
}

/// The grids kept after runs, within a byte budget.
pub struct Checkpoints { /* Arc<Heightfield> by run; CHECKPOINT_BYTES = 256 MiB */ }

pub struct Pose { pub tip: Point3, pub run: usize, pub cuts: bool }
pub struct Playback { /* runs, timeline, checkpoints, the live field, the cursor, the dirty rect */ }
impl Playback {
    pub fn at_end(runs: Arc<[SimRun]>, checkpoints: Checkpoints, finished: &Heightfield) -> Self;
    /// Moves the cursor toward `t_s`, cutting what it passes, until it
    /// is there or `budget_cells` cell tests are spent; returns the time reached.
    pub fn seek(&mut self, t_s: f64, budget_cells: u64) -> f64;
    pub fn step_moves(&mut self, n: i64);  pub fn to_run_end(&mut self, run: usize);
    pub fn time_s(&self) -> f64;  pub fn cursor(&self) -> Cursor;
    pub fn pose(&self) -> Option<Pose>;                      // None at the start and at the end
    pub fn field(&self) -> &Heightfield;  pub fn take_dirty(&mut self) -> Option<CellRect>;
}
```

```rust
/// What the operations' shapes ask for (functional 119).
pub struct Design {
    z_mm: Vec<f32>,        // the surface; `top_z_mm` where nothing asks
    asked_by: Vec<u16>,    // the operation that asks for the lowest, or NONE
}
impl Design {
    pub fn uncut(grid: &Grid, top_z_mm: f64) -> Self;
    pub fn lower_regions(&mut self, regions: &[Region], z_mm: f64, op: u16);            // Pocket, Bore, a trench
    pub fn raise_in(&mut self, regions: &[Region], z_mm: f64, within: &[Region]);        // tabs in a trench
    pub fn lower_cone(&mut self, regions: &[Region], start_z_mm: f64, slope: f64, flat_mm: Option<f64>, op: u16);
}

pub const UNCUT_MM: f64 = 0.3;                      // the threshold unless the user changes it
pub const UNCUT_FLAG: u8 = 1;                       // a bit, so phase 7's gouge can be the next
pub struct Place { pub op: u16, pub cells: u32, pub area_mm2: f64, pub height_mm: f64, pub bounds: Rect }
pub struct Comparison { pub flags: Vec<u8>, pub places: Vec<Place> }
pub fn compare(field: &Heightfield, design: &Design, uncut_mm: f64) -> Comparison;
```

`SimError` is the crate's one error: a cell size that passes `MAX_CELLS`
or is not finite and positive, a board with no size, a tapered ball nose
with an unset value (validation refuses that project first, so a shell
never sees it). The crate has no warnings: what it finds is `Facts` and
`Comparison`.

### dc-job

```rust
// dc_job::simulate
pub struct Plan { pub boards: Vec<BoardPlan> }                 // the stock, then the male board if it is cut
pub struct BoardPlan { pub board: Board, pub width_mm: f64, pub height_mm: f64, pub thickness_mm: f64,
                       pub runs: Vec<PlannedRun> }
pub struct PlannedRun { pub op: OperationId, pub op_index: usize, pub label: RunLabel, pub tool: ToolId,
                        pub sim: SimRun, pub key: u64 }        // key: what `dc_app` digests
pub fn plan(project: &Project, generated: &[EmitOp], estimate: &EstimateConfig) -> Plan;
pub fn design(project: &Project, board: &BoardPlan, grid: &Grid) -> Design;

pub struct Report { /* the facts, the places by operation, the plain tools; `Display` is the CLI's text */ }
pub fn report(project: &Project, board: &BoardPlan, facts: &Facts, comparison: &Comparison) -> Report;
pub fn heightmap_png(field: &Heightfield) -> Vec<u8>;          // 16-bit gray, top row the highest Y
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
   straight move.
3. The box of cells: the move's XY box grown by the radius, as cell
   ranges clamped to the grid. A move whose lower end is at or above
   `top_z_mm`, or whose box misses the grid, returns `None` before any
   cell is read. For a V, the radius is first cut down to `(top −
   lowest tip) · slope` when that is smaller.
4. For each cell of the box: `z = max(low(q), 0)`, rounded to `f32` by
   the one helper that documents the range; if it is below the cell's
   height, write it, set `cut_by` to `owner`, and grow the changed
   rectangle. A cell is never raised and never written below 0.

A straight move's part is a straight move and an arc's part is an arc,
and `low` over a move is the least of `low` over its parts, so a move cut
in any number of pieces, in any order, leaves what the whole move leaves.
That is what makes playback exact (acceptance 139), and upstream's
part-played ramps are not (digest, item 4 at its head).

`build` cuts each run's moves in order and, as upstream's playback does,
splits a move longer than `max(2 · radius, 8 cells)` into pieces first,
so a long diagonal's box does not cover a square of cells it never
touches. It counts cell tests and calls `keep_going` every `CHUNK_CELLS`
(2²²); `false` returns `Stopped` with the field unusable, which the
caller drops.

**Determinism.** The kernel is a pure function of the field, the move
and the cutter. Within one platform the result is the same bytes
whatever the piece lengths or chunking, because each cell's final height
is a minimum of the same set of values. Across platforms `tan`, `hypot`,
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
| Pocket | The regions its generator clears, from the same target resolution (`dc_cam`'s public region helper for a Pocket, added in slice 10 if none is public), at `top − depth`; with corner relief, the notches' discs joined to them |
| Profile | The trench: between each target outline and its offset by the tool's diameter on the cut side (`dc_geometry::offset`, sharp corners kept on the outline's side), or by the radius each way for On; at `top − depth`, 0 when it goes through. Then `raise_in` each placed tab's footprint (its width along the outline, the trench's width across) to the tab's top |
| Bore | Each circle's disc at `top − depth` |
| V-Carve | `lower_cone` over its targets' area: `start − min(flat, d / slope)` with `d` the distance from the cell's centre to the area's boundary and `start` the operation's start plane |
| Drill, Engrave, Inlay, Photo V-Carve | Nothing. The design stays as it is under them, so nothing they cut or leave is reported |

`lower_cone` needs each inside cell's distance to the boundary. It
rasterises the area to the grid (a cell is inside when its centre is),
then takes the exact Euclidean distance transform of the mask
(Felzenszwalb and Huttenlocher's two-pass lower-envelope method, O(cells),
written from the paper), less half a cell: the mask's edge is half a cell
from the first inside centre on average. The error is under a cell,
which `compare`'s neighbourhood absorbs.

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

The window owns an off-screen colour target (the egui surface's format)
with a `Depth32Float` buffer, 4× multisampled and resolved, the size of
the Viewport's card in physical pixels, registered with egui by
`egui_wgpu::Renderer::register_native_texture` and drawn as the card's
image. The scene is rendered into it from `App::update`, through
`Frame::wgpu_render_state`, only on a frame where the camera, the grid,
a preference or the theme changed. The 2D pipelines and the egui pass
are untouched; turning on eframe's `depth_buffer` instead would put a
depth attachment on every pipeline in that pass, the 2D Viewport's
included.

Textures, all `nx × ny`, nearest: heights `R32Float`; owners `R16Uint`;
edge classes and check flags packed in one `R8Uint`; gradients
`Rg16Snorm`. Each is written with `Queue::write_texture` for the changed
rectangle only.

Draws, in order:

1. **The bed**: a quad at Z = 0, a margin wider than the board (5 % of
   its longer side, at least 10 mm).
2. **Tops**: one row template of `nx × 4` vertices and `nx × 6` indices,
   drawn `ny` times by instance. The vertex stage loads the cell's
   height and places its corner; the fragment stage discards a cell that
   is cut through, takes its colour from the mode (material; the owner's
   operation colour from a 256-entry table; the depth ramp's texture),
   lays the check's tint over it (uncut in the warning colour, 60 %),
   lights it and darkens it by depth.
3. **Walls**: two more row templates, the edges between east–west
   neighbours (`nx + 1` a row, `ny` instances) and between north–south
   ones (`nx` a row, `ny + 1` instances). The vertex stage loads the two
   cells either side (outside the grid reads as 0, which makes the
   stock's own sides) and spans the wall from the higher to the lower;
   equal heights give no area.
4. **The tool**, when the playback has a pose: a mesh turned from the
   cutter's profile (24 segments), with a shank of 1.05 times the radius
   above a cutting length of 3 diameters.
5. **Lines**: the toolpath and rapids of the operation being shown, and
   the origin's axes, as screen-space quads from segments with a Z, the
   2D renderer's line shader given a depth test.

Light: upstream's rig turned to this world, ambient 0.7, a white key of
0.9 from `(120, −120, 180)` and a blue-white fill of 0.35 from `(−120,
80, 80)`, diffuse only; darkening `1 − 0.12 · depth / thickness`.

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
`facts`, `design`, `compare`, `report`; print the report; with `--out`,
`heightmap_png` through `dreamcarve-shell`'s staged write, `commit_new`
unless `--force`. A pixel is `round(z / thickness · 65535)`; the PNG's
first row is the grid's last, since a picture's top is the highest Y.

## Module layout

```
crates/dc-sim/src/{lib.rs, error.rs, grid.rs (port), field.rs, cutter.rs (port), sweep.rs (moved from dc-cam; the level-arc distance), cut.rs (port: the box, the write, the rectangle), build.rs (port), timeline.rs, checkpoints.rs, playback.rs (port), design.rs, edt.rs, compare.rs, facts.rs}
crates/dc-cam/src/heightfield.rs — keeps `Heightfield`'s mirror and the seat check for its tests, on dc-sim's `Cutter::low_over_mm`
crates/dc-job/src/simulate.rs (plan, design, report, heightmap_png) and `pub use dc_sim as sim`
crates/dc-app/src/sim.rs and sim/{job.rs (SimJob, Simulation, execute), transport.rs, camera.rs (port), mat.rs, probe.rs, shade.rs (port), rows.rs, result.rs}; steps.rs, prefs.rs, strings/en.toml
crates/dreamcarve-app/src/sim/{mod.rs (the step's Viewport), renderer.rs, heightfield.wgsl, walls.wgsl, tool.rs (port), lines.rs, transport.rs (the floating bar), controls.rs}, panels/simulate.rs (the step panel), shell/sim_worker.rs, shell/rail.rs, keyboard.rs
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
ported), `too_many_cells_name_the_cell_that_fits` (acc 131);
`cutter::` `each_tool_kind_has_its_shape`,
`a_drill_and_an_engraver_are_plain_cylinders` (acc 137),
`heights_are_the_profiles` (port: `cutterSurfaceZ`'s table),
`nothing_is_cut_past_the_radius`.

**The cut (slice 2)** `dc_sim::sweep::` the tests of `cone_low_mm`,
`disc_low_mm` and `tapered_low_mm` (moved), and
`a_ball_is_the_tapered_balls_ball`,
`a_level_arc_is_cut_at_its_true_distance`; `cut::`
`a_level_move_matches_the_profile_at_the_nearest_point` (port:
`replay.test.ts` `testKernelMatchesReferenceProfile`, for a flat r 1.5,
a ball r 2, a V r 3 at 60° and 90°, a drill r 1.25, over the axis cut,
the plunge, the lead and the cut below the bottom),
`a_ramp_is_cut_to_the_lowest_pose_not_the_nearest` (upstream's diagonal
ramp, with the exact answer),
`the_changed_rectangle_holds_every_changed_cell` (port:
`testDirtyRegionCoversChangedCells`), `cells_only_go_down_and_never_below_the_bottom`,
`a_move_off_the_grid_or_above_the_stock_reads_no_cell`,
`a_helical_arc_is_cut_within_a_micron`,
`a_v_deeper_than_its_cone_leaves_a_wall`; the properties
`a_move_in_pieces_equals_the_move` and
`moves_in_any_order_leave_the_same_field` (acc 132–136);
`dc_cam`'s seat and carve tests unchanged and green.

**Runs and reuse (slice 3)** `dc_job::simulate::`
`the_plan_is_the_runs_export_writes` on all six post-processors and both
splits (acc 142), `a_rapid_and_a_dwell_cut_nothing`,
`a_runs_seconds_are_its_lengths_over_its_feeds`,
`each_board_has_its_own_runs` (acc 150);
`dc_sim::build::` `a_build_from_a_checkpoint_equals_a_build_from_nothing`
(acc 144), `a_stopped_build_stops_within_a_chunk` (acc 145),
`the_owner_is_the_last_run_to_lower_a_cell` (acc 151);
`checkpoints::dropping_keeps_the_first_and_the_last`;
`facts::` `the_deepest_cut_names_its_run`, `removed_volume_of_a_pocket`;
`dc_job::simulate::` `the_coasters_tabs_stand` (port:
`tabbedEdgeSimulation.test.ts`, on the coaster at two qualities; acc
138), `a_v_carve_equals_the_reference` (acc 141),
`the_same_bytes_twice` (acc 143).

**Playback (slice 4)** `dc_sim::playback::` the ports of
`playback.test.ts`: `pieces_keep_their_moves_seconds`,
`short_moves_are_not_split`, `a_slow_move_takes_longer`,
`forward_seeks_equal_one_seek`, `a_backward_seek_restores_the_earlier_field`,
`the_dirty_rectangle_grows_until_taken` and
`a_backward_seek_dirties_the_grid`; and
`playing_in_any_steps_ends_at_the_build` for steps of the job over 7,
53, 197 and 1000 (acc 139; upstream's step counts, on ramps too),
`part_way_is_the_build_of_the_moves_so_far` (acc 140),
`a_spent_budget_returns_the_time_reached`,
`to_run_end_is_the_runs_checkpoint`, `the_pose_is_none_at_both_ends`;
`timeline::` `time_and_cursor_round_trip`, `a_zero_second_move_is_passed`.

**The command (slice 5)** `dreamcarve::cli::simulate::`
`the_report_and_the_picture_of_each_fixture` (acc 153, 154),
`an_existing_file_is_kept_without_force`,
`a_refused_project_is_refused_as_check_refuses_it`,
`until_stops_after_the_operation`, `the_male_board_is_its_own_picture`,
`a_cell_too_small_is_refused` (acc 131);
`dc_job::simulate::heightmap_png_is_sixteen_bit_top_row_first`;
`golden::sim_*` each `tests/golden/sim-<case>/heights.bin` within the
case's stated tolerance.

**The app core (slice 6)** `dc_app::steps::` `there_are_six_steps`,
`simulate_is_empty_then_complete_then_attention` (acc 155),
`export_is_as_it_was`; `sim::job::`
`a_job_is_given_only_on_the_step_with_generation_idle`,
`a_change_starts_from_the_first_changed_run` (acc 144),
`a_result_for_an_old_snapshot_is_dropped` (acc 145),
`undo_brings_the_earlier_result_back`; `sim::camera::` the presets'
directions, `fit_frames_the_eight_corners`, `no_drag_goes_under_the_bed`
(acc 152); `sim::probe::reads_a_pockets_floor_and_its_operation`;
`sim::rows::` `rows_are_in_cut_order_with_their_runs`,
`rows_after_the_cursor_are_dimmed`; `prefs::simulation_round_trips`;
`dreamcarve::gui_equality::simulating_changes_no_program` (acc 156).

**Shading (slice 7)** `dc_app::sim::shade::` the ports of
`heightfieldShader.test.ts`:
`a_tab_bridge_and_a_cut_through_rim_are_steps`,
`walls_and_slopes_keep_their_class` (0.4 and 0.5 of a cell are slopes,
0.6 a step, at cells of 0.1, 1, 2 and 2.5), `a_shallow_scallop_is_a_slope`,
`the_gradient_is_a_central_difference_at_a_centre`,
`the_gradient_leaves_steps_out`.

**The window (slices 7–9, 12)** `dreamcarve_app::sim::renderer::`
`one_private_top_a_cell` (port: `gpuMesh.test.ts`),
`wall_counts` (port: `instancedBoundary.test.ts`),
`only_the_changed_rectangle_is_uploaded`; `kittest::`
`the_rail_has_six_steps`, `simulate_lists_the_operations`,
`play_advances_and_pause_holds`, `a_row_click_dims_the_rest`,
`quality_colour_and_layers_change_the_picture`,
`the_male_board_is_offered_with_an_inlay`,
`a_frame_of_the_plaque_is_not_blank_and_changes` (acc 157),
`the_keys` (acc 158), `the_uncut_list_frames_its_places`.

**The check (slices 10–11)** `dc_sim::edt::`
`distances_equal_brute_force_on_random_masks` (a property);
`design::` `lower_cone_is_distance_over_slope_capped`,
`the_design_is_never_above_the_top`;
`dc_job::simulate::design::` `each_operation_asks_for_its_table_row`
(acc 146), `tabs_are_raised_in_the_trench`;
`dc_sim::compare::` `a_wall_one_cell_off_is_not_a_place`,
`small_components_are_dropped`, `a_place_names_its_operation`;
`dc_job::simulate::` `a_square_pocket_has_four_corners_and_none_with_dogbones`,
`a_sliver_one_cell_wide_is_not_a_place`, `a_wide_v_stroke_without_clearing_has_a_plateau`,
`ridges_follow_the_threshold` (acc 147),
`nothing_is_reported_where_no_shape_asks` (a lead beside a trench, a
Drill wider than its circle, an Engrave, an Inlay, a Photo V-Carve, the
uncut stock; acc 148).

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
