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
`0.5.0-alpha.1`. After slice 2: 3,490.

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

## Slices

Status is one of: todo, doing, done, owner (waits for the maintainer).
The numbers are the design spec's.

### 5.1 Drawing

| Slice | What | Status |
| --- | --- | --- |
| 0 | The specs: `functional.md` (capabilities 81–106, acceptance 107–130, S21, the hook plate), `design.md` (27 slices), ADR-0006; `NOTES.md` folded in; the roadmap's phase 5 brought up to date | done |
| 1 | Format 29: a definition's `shape` (`dc_project::Shape`, eight kinds, data only), `dc_geometry::Path` (nodes with line, arc and cubic spans; exact from a polyline, flattened to one), an instance's `construction` and `folder`, `folders`, a bitmap's `opacity_fraction`; `v28_to_v29`; fixtures `v29/sample.dcv` and `drawing.dcv`; the problems `construction_targeted`, `duplicate_folder_id`, `dangling_folder`, and `out_of_range` at a shape's size or a bitmap's opacity; a path's nodes one per line in a saved file; the quick-start projects and the book's Project file, Images and first-cut chapters at 29. No program changed: a version 28 project exports byte for byte. Gate: 3,449 tests | done |
| 2 | Shape outlines: `dc_geometry::shapes` (rectangle, rounded and chamfered, circle, ellipse, regular polygon to corners or flats, slot) with PureCutCNC's slot, n-gon and canned-rectangle tests ported, and `shapes::gear`, `gearProfile.ts` ported with its tests; every outline counter-clockwise, which upstream's are not as a set; `Shape::outline` and `outline_matches`; the warning `shape_outline_differs`; a gear's sizes checked together as `out_of_range` at the size at fault; the drawing fixture now holds every kind but text. Two properties: an outline is simple and its box is its sizes; a gear is simple and symmetric. Gate: 3,490 tests | done |
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
