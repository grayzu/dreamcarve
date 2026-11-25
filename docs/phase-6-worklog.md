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
  specs were written against `Phase-5` at `a2eee9b9`. **Before slice 1,
  the branch is rebased onto `main` with phase 5 in it** and the
  workspace version is set to `0.6.0-alpha.1`. Nothing but documents is
  on the branch until then.
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

Planned. No code. The baseline test count is taken when the branch is
rebased.

### Exit criteria (functional spec)

| Criterion | Where it stands |
| --- | --- |
| Acceptance tests 131–158 green on macOS | Not started |
| S22's two tests (142, 156) on all six post-processors and both program splits | Not started |
| The bench check's table filled for every part that has been cut | **The maintainer's.** The coaster, the plaque and the lettered inlay pair are cut; the sign in part; the line-art inlay, the photo, the tapered ball nose parts and the hook plate are not |
| Quick start "Check the cut in 3D" and the Simulation chapter, with screenshots from the build | Not started |
| `CONTEXT.md` updated; every user-visible string keyed | Not started. `CONTEXT.md` is left alone until the rebase: phase 5 rewrote much of it |
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

## Slices

Status is one of: todo, doing, done, owner (waits for the maintainer).
The numbers are the design spec's.

### 6.1 The engine

| Slice | What | Status |
| --- | --- | --- |
| 0 | The specs: `functional.md` (capabilities 107–127 with 121 withdrawn, acceptance 131–158 with 149 withdrawn, S22, the bench check's table), `design.md` (14 slices), the survey `docs/research/purecutcnc-simulation-digest.md`, the roadmap's phase 6 brought up to date. No ADR: the design surface is display only | owner (review) |
| 1 | `dc-sim`: the grid and the cutters | todo |
| 2 | The cut: `sweep` moved from `dc-cam` with its tests, the ball and the level arc, `cut` | todo |
| 3 | Runs in order: `build`, checkpoints, progress and stop, `facts`, `dc_job::simulate::plan` | todo |
| 4 | The timeline and playback | todo |
| 5 | `dreamcarve simulate`: the report, the PNG, the fixtures' results, the parity capture | todo |

### 6.2 The 3D view

| Slice | What | Status |
| --- | --- | --- |
| 6 | The app core: `Step::Simulate`, `dc_app::sim`, the preferences | todo |
| 7 | The window, the stock: the worker, the renderer, the camera, the step panel, Quality | todo |
| 8 | The window, playback: the transport, the rows, the tool, the keys | todo |
| 9 | Colour, layers, the male board, the read-out | todo |

### 6.3 Checking the result

| Slice | What | Status |
| --- | --- | --- |
| 10 | The design surface | todo |
| 11 | Compare: uncut material, the report's list | todo |
| 12 | The window, the check | todo |

### 6.4 Release

| Slice | What | Status |
| --- | --- | --- |
| 13 | Looking at it: the test parts' pictures at each quality, the decision on #973, the measured cost | todo |
| 14 | The book and the exit | todo |
