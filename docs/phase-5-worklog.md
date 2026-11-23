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
`0.5.0-alpha.1`.

## Decisions waiting for the maintainer

The functional spec's "Open questions" (ten) and the design spec's
(four). Each has a choice taken so the work can proceed; none blocks.
Decisions made while building are added here.

## Slices

Status is one of: todo, doing, done, owner (waits for the maintainer).
The numbers are the design spec's.

### 5.1 Drawing

| Slice | What | Status |
| --- | --- | --- |
| 0 | The specs: `functional.md` (capabilities 81–106, acceptance 107–130, S21, the hook plate), `design.md` (27 slices), ADR-0006; `NOTES.md` folded in; the roadmap's phase 5 brought up to date | done |
| 1 | Format 29: `shape`, `construction`, `folder`, `folders`, `opacity_fraction`; the migration, fixtures and problems | todo |
| 2 | Shape outlines in `dc-geometry`, `Shape::outline`, `shape_outline_differs` | todo |
| 3 | Paths with curves | todo |
| 4 | The tool model and snapping | todo |
| 5 | Rectangle, Circle, Ellipse, Polygon, Slot | todo |
| 6 | Line, Arc, Spline | todo |
| 7 | Gear | todo |
| 8 | Sizes: the Shape section, stretching, Convert to Path | todo |
| 9 | The window, drawing | todo |

### 5.2 Editing

| Slice | What | Status |
| --- | --- | --- |
| 10 | Edit nodes | todo |
| 11 | Fillet and chamfer | todo |
| 12 | Segment kinds | todo |
| 13 | Offset | todo |
| 14 | Weld, Subtract, Intersect | todo |
| 15 | Intersections and Trim | todo |
| 16 | Join | todo |
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
