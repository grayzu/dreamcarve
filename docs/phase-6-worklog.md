# Phase 6 worklog

The local record of phase 6 (3D preview and simulation), planned on
2026-11-23 while phase 5 waited for its tag. It stands in for GitHub
issues and pull requests, as `docs/phase-5-worklog.md` did for phase 5.
One line per slice; details live in the commits and in
`docs/specs/phase-6/`.

## Process

Confirmed by the maintainer on 2026-11-24: "for this phase we will use
the same phase 5 local process."

- All work is on the branch `Phase-6`. It was cut from `main` at
  `5f75ce9b` (`0.4.0-alpha.2`), before phase 5 reached `main`, and the
  specs were written against `Phase-5` at `a2eee9b9`. On 2026-11-24, at
  the maintainer's word ("proceed using the latest bits from phase 5 as
  a starting point"), the branch was rebased onto `Phase-5` at
  `d75c61ef` and the workspace version set to `0.6.0-alpha.1`. Phase 5
  is not on `main` or tagged yet: what it gains after `d75c61ef` has to
  be merged into this branch.
- No pull requests, no issues, no CI waits. It merges to `main` only
  when the whole phase is done.
- Spec first: `functional.md` and `design.md` were written before any
  code, by the agent, for the maintainer to review. This time they are
  reviewed before slice 1, not with the working build: sub-phase 6.3
  has no reference to port and its shape is the maintainer's to settle
  (functional open questions 2 and 3). A spec changes in the commit that
  changes what it describes.
- Test first. The gate before a slice is recorded as done, on macOS:
  `cargo fmt --all -- --check`, `cargo clippy --all-targets
  --all-features -- -D warnings`, `cargo nextest run --workspace` with
  `cargo test --workspace --doc`, and `cargo run -p check-headers`.
- Windows and Linux are tested by CI when the branch reaches `main`. The
  code stays portable meanwhile. Before that merge, the checks CI runs
  and the local gate does not are run here too (`cargo doc`, `cargo
  deny`, `mdbook build`, the licence and dist checks, clippy for the
  Windows and Linux targets). This phase adds one to watch: the window's
  rendering tests need a GPU adapter in CI (phase 4 design, "Test
  harness", still "not yet so").
- The maintainer's own steps wait for them: the review of the specs and
  of the build, the bench check's table, the decisions listed below, and
  the tags.

Reference checkout, beside this worktree: `../purecutcnc` at `d04ae2f`.
Its survey is `docs/research/purecutcnc-simulation-digest.md`.

## Where it stands

Baseline before the first slice: 3,766 tests pass on macOS at
`0.6.0-alpha.1`, on `Phase-5`'s `d75c61ef`. After slice 4: 3,838. After
slices 5, 10 and 11: 3,865. After the window (slices 6 to 9, 12 to 14):
3,919.

### Exit criteria (functional spec)

| Criterion | Where it stands |
| --- | --- |
| Acceptance tests 131–158 green on macOS | Green, with 149 withdrawn. Each is named in the design spec's test plan beside the test that carries it |
| S22's two tests (142, 156) on all six post-processors and both program splits | Green: `dc_job` `simulate::the_plan_is_the_runs_export_writes` and `dreamcarve` `gui_equality::simulating_changes_no_program`, each on the plaque, the sign and the inlay pair |
| The bench check's table filled for every part that has been cut | **The maintainer's.** The coaster, the plaque and the lettered inlay pair are cut; the sign in part; the line-art inlay, the photo, the tapered ball nose parts and the hook plate are not |
| Quick start "Check the cut in 3D" and the Simulation chapter, with screenshots from the build | Written, with nine pictures taken by `book::simulation_screenshots`; every other picture of the book taken again for the six-step rail |
| `CONTEXT.md` updated; every user-visible string keyed | Done: a Simulation section (Simulation, Heightfield, Cell, Quality, Bed, Playback, Design Surface, Uncut Material), and the string scans pass |
| The maintainer's review, and the open questions settled | **Open.** "Decisions waiting for the maintainer", below |
| `v0.6.0-alpha.1`, then `v0.6.0` | **The maintainer's** |

## Decisions waiting for the maintainer

Settled by the maintainer on 2026-11-24:

- **A sixth step, Simulate**, between Operations and Export, not a
  2D/3D switch on every step's Viewport (functional 1).
- **Uncut material only; gouges wait for phase 7**, "when we need to
  bring in 3d support". Uncut material is compared with a *design
  surface* built from each operation's own shapes and depths
  (functional 2; capability 119). Capability 121 and acceptance 149 are
  withdrawn, and with them part and waste, the gouge threshold and the
  step's attention state. The roadmap's phase 7 gains gouge
  highlighting.
- **Uncut material is not a warning code** (functional 3): accepted
  with "Ok... I think", so it may be looked at again with the build.
- **The exit does not wait for parts that are not cut** (functional 9).
- **SolidCore** (`tangency-labs/solidcore`, Apache-2.0, a Rust port of
  NVIDIA's SMLib, experimental at 0.0.x) was raised as a help with 3D
  modelling. Not for this phase: the simulation is a height grid and
  the design surface needs no solid. It is a candidate for phase 7's
  re-examination of ADR-0003 and for STEP import after 1.0.

Still open: the functional spec's other open questions and the design
spec's eight. Each has a choice taken so the work can proceed:

- **`dreamcarve simulate`** is in: a report and a 16-bit heightmap PNG
  (functional 7).
- **A drill and an engraving bit are plain cylinders**, as PureCutCNC
  draws a drill; no point angle is invented (functional 5).
- **PureCutCNC's two rendering refinements are not in the first build**
  (issues #973 and #1209, about 1,400 lines); slice 13 decides on #973
  from pictures (functional 13).

Decisions made while building are added here.

- Slices 1 to 4, the engine. **Heights are `f64`**, not the `f32` the
  first draft had: `docs/rust-guidelines.md` keeps `f32` to the
  renderer's edge. A grid's fields are private and a height is read as
  an `Option`; the cut is `Heightfield::cut`, and `build` takes the base
  stock by value, since the guidelines give the engine no `&mut` to a
  caller's data. The design spec's signatures are the code's now.
- Slice 2. **Phase 3's closed form could lose a cut at the rim**: a move
  that falls as it leaves a cell is lowest over it exactly on the
  cutter's rim, where rounding put the distance a hair past the radius.
  The reference test found it on a 90° V; a pose inside a covered
  stretch is now read no further out than the rim. `dc-cam`'s seat and
  carve tests are unchanged by it. **An arc that falls** is cut along
  the whole arc's chords however it is split, so that playing it in
  parts is the whole to rounding (chords of each part differed by half
  a micron). A line is split into pieces only when its box runs long
  across both axes. Upstream's kernel test is ported on moves of my own
  in its frame: the survey names the kinds of move, not their numbers.
- Slice 3. The cut order is the order of the generated runs, the
  stock's then the male board's: `emit_programs` only divides that
  sequence into programs where the tool changes. Acceptance 142 holds
  it on six post-processors and both splits by the programs' tool runs
  and operations, and move for move against the generated toolpaths; it
  does not read the written G-code back, which the safety pass already
  holds to the toolpath (S11, S12). **Acceptance 138 is tested on the
  plaque**, not the coaster, which has no committed project. Besides
  its four tabs the plaque's Profile leaves a fifth patch standing in
  its own run's cells: the lead-in arc at the back, which only the first
  pass cuts, 4 mm deep, beside the trench. It is in the waste; the
  simulation shows it, and the test says so. A run's key is an FNV-1a
  hash in `dc-sim` (`SimRun::key`), mixed with its board's size in
  `dc-job`.
- Slice 4. Playback is placed by a cursor, and a time is turned into
  one, so a move that takes no time (a rapid with no rapid rate set) is
  passed as soon as it is reached and the tool is never drawn on it.
  **At the job's end the finished build is put in place of the played
  stock**: parts of a move agree with the whole to rounding, not to the
  bit, and the end should be the build itself (acceptance 139). A step
  back from part-way along a move goes to that move's start. Upstream's
  two subdivision tests are not ported: the cut splits moves here.
- Slices 5, 10 and 11. **`simulate` follows the CLI as it is**: `--board
  female|male` as `export` has it (the spec had `stock|male`), and no
  `--force`, which no command has: `--out` writes over nothing or over
  a picture it wrote before, known by a `Software` text in the PNG, as
  `check --preview` knows its SVG. `--uncut-over` sets the threshold.
  The report's time is the moves' at their feeds: 3 min 33 s for the
  plaque, well under `export`'s estimate, which counts the spindle's
  spin-up and reads fitted arcs.
- **What the uncut check finds on real projects.** The plaque as
  committed, the sign and the inlay pair report nothing. The plaque
  with its square recess's dogbones off reports four corners, 5.1 mm²
  in all at Standard (the corners are 8.7 mm²; a cell along each wall
  is the tolerance's). **A V-Carve has little to report**: one too wide
  for its bit is refused by S17 before it is cut, and with no clearing
  pass the V-bit clears its own floor, so the spec's scenario of an
  uncut plateau could not happen and is replaced by a Pocket's necks
  and corners. A Profile's trench is found on the grid, by distance
  from the shape's edge, with no polygon offset; a tab is kept a tool's
  radius longer at each end than stands.
- Slice 6, the app core. **The last simulation of a board is kept
  whatever snapshot it was made for**, where the design said a result
  for an old snapshot is dropped: it is what stays on screen, dimmed,
  while the next is made, and what the next one builds on. Whether it
  is the current one is asked of it (`Simulation::state`), by the
  snapshot and the settings it was made with, so Quality and the uncut
  threshold are part of what "current" means. A job that stopped
  because a newer one was wanted is not held as Stop holds one. **A job
  smaller than one chunk of work is never asked whether to go on**: the
  plaque at Draft is cut before the engine first asks, so Stop takes on
  larger jobs only, which is where it matters. **Continue leads through
  Simulate from any step**, as the first step that is not complete; a
  project never opens on it (functional question 11). The simulation
  reads `Generation::current_runs`, every enabled operation's runs once
  none is waiting, an error or not: an operation an error stopped has
  none and is left out, and the panel says so.
- Slice 7, the renderer. As the design now says it: no vertex or index
  buffers, gradients as `Rg32Float` (a steep V's slope does not fit a
  normalised texture, and `Rg16Snorm` is a wgpu feature), and an sRGB
  target, since egui decodes what it samples. **The step rule's edge
  has a nanometre of slack**: two heights half a cell apart on paper
  differ by a rounding once they are `f64`s, and upstream's own test
  asks that exactly half a cell be a slope. A wgpu debug label is an id
  (`sim.tops`), not words: the shell's string scan reads every literal.
- Slice 8, playback. The transport's keys and the views' are rows of
  **View ▸ Simulation**, a submenu: eleven more rows made View too long
  for a short window. **Space plays whenever the focus is only where a
  click left it**, and is taken before the window's controls see it, so
  it does not also press the rail's button; a control reached with Tab
  keeps Space. The drawing's keys (the tools, Delete, the arrows) are
  not read on the Simulate step: they would change shapes that are not
  in view. *Simulating… n %* and Stop are the status bar's generation
  cell, which the header's chip reads.
- Slice 12. **Uncut material is off until turned on**, as capability
  120 says; the first draft of the preferences had it on.
- **What the pictures and the window's tests found**, each put right:
  the stock drawn far too dark (an sRGB target, where egui reads a
  texture as holding encoded colours); a step back from the job's end
  that changed nothing (it undid only the rapids after the last cut);
  Play ending at once from the job's last moment, a step having brought
  it there with those rapids still to pass; an uncut material line too
  long for the panel, which pushed the rest of the section out of it;
  Colour's three choices cramped beside their label; a framed place
  filling the card under the pills. A through cut showed no colour By
  operation, having no floor: a wall now takes the colour of the cut
  that made it.
- **The app quick start goes through Simulate.** *Continue* from
  Operations opens it, so the chapter says so, shows the coaster there
  and goes on to Export; its test follows. Every picture of the book
  was taken again, since each shows the rail.
- **The parity capture is the maintainer's to unblock.** `../purecutcnc`
  has no `node_modules`, and installing them changes that checkout and
  fetches from the network. With them, `tools/parity/sim-capture.ts`
  would write PureCutCNC's grid for the golden cases whose moves are
  level, to compare with `dc-sim`'s.
- The environment, not the code: on this machine a newly built program
  can wait minutes to start while another session runs its tests (the
  build volume is shared, and launches seem to queue system-wide). A
  gate run that takes 15 minutes alone took far longer beside the
  Phase-5 worktree's.

## Slices

Status is one of: todo, doing, done, owner (waits for the maintainer).
The numbers are the design spec's.

### 6.1 The engine

| Slice | What | Status |
| --- | --- | --- |
| 0 | The specs: `functional.md` (capabilities 107–127 with 121 withdrawn, acceptance 131–158 with 149 withdrawn, S22, the bench check's table), `design.md` (14 slices), the survey `docs/research/purecutcnc-simulation-digest.md`, the roadmap's phase 6 brought up to date. No ADR: the design surface is display only | owner (review) |
| 1 | `dc-sim`, a new crate below `dc-cam`: `Grid` (square cells counted along the longer side, a least cell and a cap of 2²⁴; upstream's 24-cell floor not ported), `Heightfield` (an `f64` height and the last run to lower it, a cell; never below the bottom), `Cutter` and `CutterShape::of` (flat, ball, V, tapered ball nose; a drill and an engraving bit as plain cylinders), `SimError`; `tests/manifest.rs` for S22; the crate graph in the guidelines and the book's crate map | done |
| 2 | The cut: `dc_sim::sweep`, phase 3's closed forms moved from `dc-cam` with their test and held to a brute force for every shape (the ball and the capped V added; the rim fix); `Heightfield::cut` for a line, a level arc (whole, by its true distance) and a falling arc (fixed chords within a micron), by a span of the move, with the changed rectangle and the count of cells tested; `SimMove`; two properties (a move in parts is the move; moves in any order leave the same bits); `dc-cam`'s test heightfield on `dc-sim`'s forms | done |
| 3 | Runs in order: `SimRun`, `build` (from any run's checkpoint, the stock handed back after each run, progress and stop between chunks of cells tested), `Checkpoints` within a byte budget, `facts` (the deepest cut and its run, the area cut through, the volume removed, the plain cylinders), `SimRun::key`; `dc_job::simulate::plan` (each board's runs in the order export writes them, each move with its seconds) and `dc_job::sim` | done |
| 4 | `Timeline` (every move's place in time) and `Playback` (a cursor over the whole job, seeks forward by cutting and back from the nearest checkpoint, bounded by cells tested; a step of one cutting move; the end of a run; the tip's pose; the dirty rectangle; the finished build put in place at the end). Gate for slices 1 to 4 together, on macOS: fmt and clippy clean, 3,838 tests of which one failed for a reason of the run's own (`version_flag_prints_dev_build_time_and_commit_hash` compares the binary's commit with `HEAD`, which the version commit moved while the tests ran; it passes on a build of the commit), doc tests and headers pass | done |
| 5 | `dreamcarve simulate <dcv>`: the report (the board and its cells, the runs and their time, the deepest cut, where it goes through, the volume removed, the plain cylinders, the uncut material by operation), `--out` a 16-bit heightmap PNG that knows itself by a `Software` text, `--quality`, `--cell`, `--board`, `--until`, `--uncut-over`; the committed reports and pictures of the plaque, the sign and the inlay's male board; the Overview chapter's section. **Not done: the parity capture**, which needs `npm install` in `../purecutcnc` | done, but for the capture |

### 6.2 The 3D view

| Slice | What | Status |
| --- | --- | --- |
| 6 | The app core: `Step::Simulate`, sixth of six, with *Continue* leading through it and a project never opening on it; `dc_app::sim` (the job and `execute`, which cuts from the first run that changed; `Simulation`, which says whether what is shown is current; what blocks a simulation; the transport; the orbit camera and its matrices; the probe; the rows, the summary and the read-out as text); `Generation::current_runs`; `SimPrefs` and `[simulation]` in `gui.toml` | done |
| 7 | The window, the stock: the simulation thread (`shell/sim_worker.rs`); `StockRenderer`, off screen with a depth buffer into a texture egui shows, the bed, a top a cell and a wall an edge from `stock.wgsl` with no vertex buffers, only the changed rectangle sent; the step rule and gradients (`shade.rs`, ported with its tests); drag, slide and zoom, the four views and Fit; the step's panel with Quality and its cell size; the tokens `sim.stock`, `sim.bed`, `sim.tool`, `sim.tool.shank`; the first pictures | done |
| 8 | The window, playback: the transport (Previous, Play or Pause, Next, the scrubber with a tick an operation, the two times, Speed and its five choices), the rows' clicks and their progress, the tool as one turned profile of the cutter's own shape (`tool.rs`), the keys of capability 126 as rows of View ▸ Simulation, *Simulating… n %* and Stop in the status bar | done |
| 9 | Colour (Material, By operation with a wall coloured by the cut that made it, By depth with phase 4's legend), Layers (the tool, the toolpath and rapids of the operation shown and the origin's axes as lines against the stock's depth, the bed), the male board's choice, the read-out from the machine origin with the operation's name | done |

### 6.3 Checking the result

| Slice | What | Status |
| --- | --- | --- |
| 10 | The design surface (done before slice 6, with slice 5, since the report lists uncut material): `dc_sim::Design` (regions, a trench beside an edge, a trench along a path, tabs, a V's cone) on `dc_sim::edt`, a distance transform written from the paper; `dc_job::simulate::design` for each operation of a project | done |
| 11 | `dc_sim::compare`: uncut material against the highest asked in a cell's neighbourhood, patches of four cells and more as places, by operation; the report's list. On the fixtures: the plaque, the sign and the inlay pair report nothing; the plaque with its square recess's dogbones off, its four corners. Gate for slices 5, 10 and 11 together, on macOS: fmt and clippy clean, doc tests and headers pass, 3,865 tests of which two failed for a reason outside these slices (`dreamcarve-app`'s two source scans, for string and colour literals, read the working tree, and found a draft of slice 7's renderer written while the tests ran; the draft was put right, and both scans are part of the next gate) | done |
| 12 | The window, the check: the flag folded into the classes texture and tinted in the warning colour on the finished stock; Result with the report's facts, the Uncut material switch (off until turned on), its threshold, a line an operation that frames its places from the top, and what the check cannot find | done |

### 6.4 Release

| Slice | What | Status |
| --- | --- | --- |
| 13 | Looking at it: 19 scenes of the plaque, the sign and the inlay pair in both appearances, written by `review::phase6_review` into `docs/specs/phase-6/review/`, with its README: what was looked at, **#973 not ported** (walls on cell edges read well at Standard), and the measured cost, 4 ms to 554 ms a build, so no optimisation | done |
| 14 | The book and the exit: the quick start "Check the cut in 3D" and the chapter "Simulation", with nine pictures taken by `book::simulation_screenshots`; the app quick start taken through Simulate on its way to Export; every picture of the book taken again, the rail having six steps; the Keyboard chapter's table and its section on the step; the six steps in The app and the Overview; the crate map; `CONTEXT.md`'s Simulation terms; the changelog. Gate for slices 6 to 9 and 12 to 14 together, on macOS: fmt and clippy clean, 3,919 tests pass, doc tests and headers pass. The checks CI runs that the gate does not, run on 2026-11-25: rustdoc with warnings denied, with and without private items (five links in `dc-sim`'s comments were ambiguous or to a private item, and are put right); `cargo deny check licenses advisories`; `mdbook build`; the third-party licences test; `dist plan` and `dist generate --check`; clippy for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu`: all pass. `gcode-conformance` needs LinuxCNC and runs in CI only. The exit audit went capability by capability and added two tests (`an_operation_with_an_error_is_left_out`, `space_plays_and_pauses`) | done |
