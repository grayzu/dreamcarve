# Alpha worklog

The local record of the push to an early alpha, started 2026-11-18. It
stands in for GitHub issues and pull requests while that process is paused.
One line per slice; details live in the commits and the specs.

## Process for this push

Decided by the maintainer on 2026-11-18. It overrides the "Git" and "How
work flows" rules of `AGENTS.md` and `docs/plan.md` until they say
otherwise; everything else in those files still holds.

- All work is on the branch `alpha`, cut from `main` at `9e1049e9`. It is
  pushed to GitHub as a backup. No pull requests, no issues, no CI waits.
- Still test-first. The gate before a slice is recorded as done, on macOS:
  `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features
  -- -D warnings`, `cargo nextest run --workspace` with `cargo test
  --workspace --doc`, and `cargo run -p check-headers`.
- Windows and Linux are not validated in this push. Code stays portable:
  no macOS-only API without a `cfg`, no path or line-ending assumptions.
- A spec changes in the same commit as the code it describes.
- The maintainer's own steps wait for them: bench checks (real cuts), the
  `v0.3.0` and `v0.4.0` tags, and anything that needs their private files.
- Scope: finish phases 3 and 4, redesign the UI, and carry on with
  `docs/code-review-plan.md`.

## UI decisions (2026-11-18)

- Dark by default; light kept and designed with it.
- One accent, green/teal. Toolpath colours stay distinct from it.
- A full redesign, not a restyle: a left icon rail and a step-by-step flow
  (Design, Stock, Tools, Operations, Export).
- Reference images: the maintainer's `ui_examples` folder (not in the
  repository). Their common language: rounded cards on a darker backdrop,
  pill tabs and chips, roomy spacing, one saturated accent, an inspector
  beside the canvas.

## Slices

Status is one of: todo, doing, done, owner (waits for the maintainer).

### A. Fixes

| Slice | What | Status |
| --- | --- | --- |
| A1 | An Inlay's schema error names its field (GitHub #712; review 1.3) | done |
| A2 | A clipped Inlay cut-out places the tabs it was asked for (#710; review 1.13) | done |
| A3 | The safety pass fails closed on a lookup miss (#711; review 1.1): 102 sites audited, 31 failed open; the audit table is in `docs/specs/phase-1/design.md` | done |
| A4 | Boundary sweep: the plug 0.182 mm proud at lettering seed 0x47 (#713). Fixed in `dc-geometry/src/medial.rs` (`restore_wall_reach`); all three sweeps pass with 0 failures | done |
| A5 | Import robustness: DXF stitch and dedupe budget, warning names, read caps, control characters, `init` no-overwrite (review 1.5, 1.6, 1.8, 1.9, 1.12) | done |
| A6 | `Rates` keeps feed and plunge; a cut outside every operation is refused; the digest hashes bitmaps; wall-clock assertions removed from dc-import and photo (review 1.4, 1.7, 1.10, 1.11). Still open: dc-gcode's `s17_check_is_fast_on_the_sign` and `s9_check_is_fast_*` assert time | done |
| A7 | A generator's lookup miss is an internal-defect error, `CamError::Internal`, not a skipped pass (review 1.2). Done for every generator in dc-cam (about 160 lookups now error, 110 removed, 16 sentinels replaced); the sweeps' totals are identical and none met the new error. Still to do: the interior of dc-geometry (about 275 unclassified pattern hits, mostly `medial.rs`, `merge.rs`, `simplify.rs`, `self_band.rs`, `polyline.rs`); making `medial_axis` and `boolean` fallible changes signatures in dc-import and about 36 call sites | doing |

| A8 | A smooth tab at the end of an open path was cut away: the generator laid the tab's slope inside the clamped span and plunged at the tab's centre. Fixed in `dc-cam/src/tabs.rs`; the safety pass's tab spine now covers Left and Right open paths | done |
| A9 | `PROPTEST_RNG_SEED=37` failed the Profile property: the checker, not the generator. Arc fitting merged a tiny helix's tail, the handoff and a dogbone into one arc, and S9's notch rule accepted straight cuts only. `Notch::allows` now judges an arc piece by piece. Seeds 38 to 47 pass | done |

Notes from the fixes:

- A2: the cut-out's request is per outline (per plug), shared among that
  outline's open pieces by length. `at_distances` had the same defect and
  the same fix. No expected program changed.
- A5: control characters from a DXF are replaced with U+FFFD at the
  reader. Still open: SVG ids and `.dcv` names are not sanitised before
  the CLI prints them; one filter in `crates/dreamcarve/src/output.rs`
  would cover every source.
- A1/A6: the digest hashes the whole bitmap record, base64 included, per
  operation that reads it on every snapshot. Not measured on a large
  picture; revisit with review item 3.3.
- D2: the CLI's preview palette now equals the light Viewport set.
- A3: review item 1.10 was real. A post-processor footer of `G1 Z-1 F5000`
  exported: a plunge into the stock at 5000 mm/min. Any `G1`, `G2` or `G3`
  outside every operation is now `ExportError::CutOutsideOperation`. The
  bundled definitions have no such line, so no program changed.
- A4: the cause was a medial-axis dead end pruned by a threshold that is
  relative to the sampling step, which grows with the band's size: in a
  larger drawn boundary a 0.6 mm dead end fell under it and the male cut
  nothing in the last 0.3 mm of a narrow channel. A pruned node is now put
  back when it reaches a wall the remaining axis misses. The restore fires
  on other designs too (about 50 nodes in an 80-design sample), so carves
  can gain short extra cuts; no program in the suite moved.
  `drop_small_components` uses the same kind of step-relative threshold
  and could drop a short channel open at both ends; not hit, not changed.
- A8: the verdict was the generator, not the check. Reduced case: a
  15 mm line, Right, a 2.578 mm bit, one smooth tab 1 mm wide asked for at
  distance 0. The last two passes plunged at the tab's centre, about 1 mm
  below its top, so the tab was cut away. Cut from the other end the lap
  sloped down to depth at the tab's centre, which the safety pass did not
  catch because it checks straight plunges only. No expected program
  changed.
- A7: no golden or expected program changed, and no test met the new
  error, so no dropped pass was found in the 2.5D generators.

### B. Phase 3

| Slice | What | Status |
| --- | --- | --- |
| B1 | Image-prep defaults (#599). The calibration was already merged (#638); re-run and confirmed: `min_detail` 0.2, `smooth` 0.05. The maintainer's run on their own art is described under "Still to confirm" in `tests/fixtures/prep/README.md` | owner |
| B4 | `image-prep`'s report gives a `Narrowest gap` of 0.19 to 3.0 mm for the same letters depending on `--smooth` (dc-geometry) | todo |
| B2 | Exit-criteria audit and CHANGELOG for 3.7 (#701) | todo |
| B3 | Bench checks 3.4, 3.5, 3.6 (#606, #649, #684) and the `v0.3.0` tag | owner |

### C. Phase 4 features

| Slice | What | Status |
| --- | --- | --- |
| C1 | Import drawings and pictures (#535), headless and in the window | done |
| C2 | Photo V-Carve editor and bitmaps (#544), headless and in the window; a picture is selected, moved and resized on the Viewport | done |
| C3 | Tapered ball nose in the app (#685), headless and in the window | done |

### D. UI redesign

| Slice | What | Status |
| --- | --- | --- |
| D1 | The redesign brief, `docs/specs/phase-4/redesign.md`: principles, the window, the steps, tokens, components | done |
| D2 | Theme tokens and widgets: dark default, teal accent, cards, pills | done |
| D3 | The shell: `dc_app::steps`, the header, the rail, the step panel, the Inspector, the start screen; capabilities 50, 50a, 50b rewritten | done |
| D4 | Export inlined into its step, the Viewport's floating controls, the drawer and sheets restyled. Also from the D3 review: a blank project shows "1 error" in the status bar and on the Warnings badge; with By depth on and the Inspector open at 1280 px the fitted stock is small; stale comments in dc-app still name the toolbar and the CAM panel; the snapshot pair needs re-rendering on Linux | todo |
| D5 | Window tests and snapshots for the new shell | todo |

| D6 | The three features wired into the new window: the Add picture sheet, pictures as textured quads under the shapes, Photo V-Carve's editor, the tapered ball nose's fields, the library's Tip column | done |
| D7 | From the D6 review: a Photo V-Carve's toolpath draws 453 start circles and 88 tab marks for 48 grooves, in the window and the CLI preview (dc-job's marks); importing a 12 MP picture holds the UI thread about 214 ms in dc-app (the PNG encode and the Scene's decode), and each later command about 10 ms | todo |

Decisions made while wiring, for the maintainer to confirm:

- A picture can be resized, aspect kept, about its centre, by its
  Properties or its corner handles. Capability 56 said a picture does not
  scale; it now says it does (commit `2ddb4837` in dc-app, and the
  window's handles).
- The Tool Library sheet is 1100 px wide (was 960) so its ten columns fit.
- A new carve takes the first tool its picker offers, so a project whose
  only carving tool is a tapered ball nose gets it.

### E. Docs and release preparation

| Slice | What | Status |
| --- | --- | --- |
| E1 | Book chapters and the quick start "From SVG to G-code in the app", with screenshots from the build (#550) | todo |
| E2 | macOS bundle and the phase 4 audit (#551) | todo |
| E3 | Bench check in the app and the `v0.4.0` tag | owner |

### F. Review plan, stage 2

Taken where a slice above already touches the code; the rest after E.

| Slice | What | Status |
| --- | --- | --- |
| F1 | dc-project: tests out of the source files (2.25), `Problem` through thiserror with a code macro (2.1), the migration helper and steps array (2.18), the accessor and `SOURCES` macros (2.19), the comment sweep (2.23, 2.24) | doing |
