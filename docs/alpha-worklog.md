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
| A1 | An Inlay's schema error names its field (GitHub #712; review 1.3) | doing |
| A2 | A clipped Inlay cut-out places the tabs it was asked for (#710; review 1.13) | doing |
| A3 | The safety pass fails closed on a lookup miss (#711; review 1.1) | todo |
| A4 | Boundary sweep: the plug 0.182 mm proud at lettering seed 0x47 (#713) | doing |
| A5 | Import robustness: DXF stitch and dedupe budget, warning names, read caps, control characters, `init` no-overwrite (review 1.5, 1.6, 1.8, 1.9, 1.12) | doing |
| A6 | The digest hashes bitmaps; `Rates` keeps feed and plunge; cuts outside a span refused; wall-clock assertions removed (review 1.4, 1.7, 1.10, 1.11) | todo |
| A7 | A generator's lookup miss is an internal-defect error, not a skipped pass (review 1.2) | todo |

### B. Phase 3

| Slice | What | Status |
| --- | --- | --- |
| B1 | Calibrate the image-prep defaults from the sweep (#599), without the maintainer's private art | todo |
| B2 | Exit-criteria audit and CHANGELOG for 3.7 (#701) | todo |
| B3 | Bench checks 3.4, 3.5, 3.6 (#606, #649, #684) and the `v0.3.0` tag | owner |

### C. Phase 4 features

| Slice | What | Status |
| --- | --- | --- |
| C1 | Import drawings and pictures (#535) | todo |
| C2 | Photo V-Carve editor and bitmaps (#544) | todo |
| C3 | Tapered ball nose in the app (#685) | todo |

### D. UI redesign

| Slice | What | Status |
| --- | --- | --- |
| D1 | The redesign brief, `docs/specs/phase-4/redesign.md`: principles, the window, the steps, tokens, components | done |
| D2 | Theme tokens and widgets: dark default, teal accent, cards, pills | todo |
| D3 | The shell: icon rail, steps, start screen | todo |
| D4 | The steps' panels (Design, Stock, Tools, Operations, Export), warnings, viewport overlays | todo |
| D5 | Window tests and snapshots for the new shell | todo |

### E. Docs and release preparation

| Slice | What | Status |
| --- | --- | --- |
| E1 | Book chapters and the quick start "From SVG to G-code in the app", with screenshots from the build (#550) | todo |
| E2 | macOS bundle and the phase 4 audit (#551) | todo |
| E3 | Bench check in the app and the `v0.4.0` tag | owner |

### F. Review plan, stage 2

Taken where a slice above already touches the code; the rest after E.
