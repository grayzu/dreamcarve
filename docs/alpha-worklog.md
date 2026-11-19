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
| A7 | A generator's lookup miss is an internal-defect error, `CamError::Internal`, not a skipped pass (review 1.2). Done for the 2.5D generators (about 100 lookups now error, 55 removed). Still to do: `pocket/rings.rs` `RingTree`, `vcarve*`, `inlay.rs`, `relief.rs`, `photo.rs`, dc-geometry | doing |

| A8 | A tab at the end of an open path on a Left or Right Profile: the safety pass's tab spine there fails a property seed (kept in `crates/dc-gcode/proptest-regressions/properties.txt`). Either the generator plunges into a tab at an open path's end or the spine measures the span differently; `docs/specs/phase-3/design.md` has the seed | todo |

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
| C1 | Import drawings and pictures (#535): the headless half is done; the window half (picture sheet, tree rows, textured quads, drop) is slice D6 | doing |
| C2 | Photo V-Carve editor and bitmaps (#544): the headless half is done, with selecting, picking, moving and resizing a picture; the window half is slice D6 | doing |
| C3 | Tapered ball nose in the app (#685): the headless half is done; the window half is slice D6 | doing |

### D. UI redesign

| Slice | What | Status |
| --- | --- | --- |
| D1 | The redesign brief, `docs/specs/phase-4/redesign.md`: principles, the window, the steps, tokens, components | done |
| D2 | Theme tokens and widgets: dark default, teal accent, cards, pills | done |
| D3 | The shell: `dc_app::steps`, the header, the rail, the step panel, the Inspector, the start screen; capabilities 50, 50a, 50b rewritten | done |
| D4 | Export inlined into its step, the Viewport's floating controls, the drawer and sheets restyled. Also from the D3 review: a blank project shows "1 error" in the status bar and on the Warnings badge; with By depth on and the Inspector open at 1280 px the fitted stock is small; stale comments in dc-app still name the toolbar and the CAM panel; the snapshot pair needs re-rendering on Linux | todo |
| D5 | Window tests and snapshots for the new shell | todo |

| D6 | Wire the features built headless into the new window (contracts below) | doing |

Where things plug in after D3: the step panel is `panels/side.rs`
`step()`, matching on `Step`; the Inspector is `inspecting()` returning
`Inspecting::{Shapes, Tabs, Tool, Operation}` and `body()` drawing it (a
picture's Properties is a new variant on Design); the Export step is
`panels/export_step.rs` and `panels::project::output`; tool fields are
`tools::tool_fields`; the floating pill is `viewport::tools_pill`.

D3 changed these step rules from the brief, all for good reason: a blank
project has no marks; Export needs attention only when an operation is
enabled; an unused tool without rates leaves Tools complete; marks hold
still while toolpaths regenerate; rail buttons are 56 px wide so
"Operations" fits; panels no longer collapse to strips (View ▸ Step Panel
hides the step panel).

#### What the window must draw for slice C1 (pictures)

- **Add picture sheet:** `PictureImport::open(ImageFormat, name, &bytes,
  Purpose)`; read `size_px()`, `pixels()`, `warnings()`, `notes(&Strings)`,
  `side()`, `size()`, `footprint_mm()`, `blocked(&Strings)`; write
  `set_width_mm`, `set_height_mm`, `clear_size` (mm; parse typed text with
  `units::parse_length_mm`); run `App::import_picture(&sheet)`.
- **New from…:** open either sheet with `Purpose::New`, then
  `App::new_from_drawing(&sheet)` or `App::new_from_picture(&sheet)`.
- **A dropped file:** `FileKind::of(name)`; `App::drop_file(&path, bytes)`
  returns `Dropped::Project`, `Drawing(sheet)` or `Picture(sheet)`. Run
  the unsaved-changes sheet before passing a `.dcv`.
- **Tree:** `Tree::rows()`, `Tree::bitmaps`, `bitmaps_header()`,
  `bitmap(&id)`; row actions `tree::bitmap_menu`, `bitmap_menu_command`,
  `toggle_bitmap_visible`, `toggle_bitmap_locked`, `rename_bitmap`. The
  legacy `PictureRow`, `Line` and `Tree::lines()` are deleted once the
  window uses `rows()`.
- **Viewport:** `Scene::bitmaps() -> &[SceneBitmap]` (`id`, `name`,
  `locked`, `footprint`, `width_px`, `height_px`, `pixels`, and `picture`,
  the texture cache key). The Scene decodes a picture on the UI thread
  when it first appears; move that to the worker if it stutters.
- **Not built yet:** selecting, picking and moving a picture (new
  `Selection` and `Hit` variants); *Add ▸ Photo V-Carve* is still disabled.
  Both belong to C2.

#### What the window must draw for slice C3 (tapered ball nose)

- **Tool editor:** draw `tools::rows(tool, units)` (`FieldRow { field,
  label, text, unit, required }`), ordered by `tools::fields_for(kind)`,
  in place of the hard-coded field list. Until then the type dropdown
  offers Tapered ball nose but its three fields (`ToolField::TipDiameter`,
  `Taper`, `FluteLength`) are not drawn. `KindChoice::ALL` has six
  entries.
- **Required marks:** `FieldRow.required`; `tools::lacks_value(tool)` is
  the row's red dot. The message goes in the tooltip (capability 66b).
- **Tables:** `library::Column::ALL` (Name, Type, Ø, Tip, Angle, Flutes,
  Feed, Plunge, RPM, Product) with `heading()` and `numeric()`;
  `library::row`/`cell` return `Cell::{Text, Empty, Unset}`. The Tools
  list's second line is `tools::row_detail`.
- **`KindFilter`** has no tapered ball nose entry yet: the window matches
  it exhaustively. Switch the window to `KindFilter::label()`, then add
  the variant.
- **Pickers:** `editor::offered_for` and `offered_tools`, unchanged
  signatures. **Notes:** `editor::tool_note`, `editor::notes` on the
  Inlay's `male_board` row, `editor::seat_line`.
- **Import review:** `library::review_status`, `review_unset`, `skipped`.
- Still open: `operations::new_operation` defaults a new carve to the
  first V-bit even when a tapered ball nose is the only carving tool.

#### What the window must draw for slice C2 (Photo V-Carve, pictures as objects)

- **Editor rows** come from the fields table like every kind. The Picture
  row is `Row::picks_picture()`: a dropdown from
  `editor::offered_pictures`, shown text `editor::picture_text`, commit
  `editor::set_picture`. Replace its placeholder
  `FieldKind::Choice(&[])` with a `FieldKind::PicturePicker`.
- **Add menu:** `operations::add_menu_with(project, strings, shapes,
  app.selected_pictures())` and `add_with`; fold the pictures parameter
  into `add_menu`/`add`.
- **Selection:** `App::selected_pictures`, `select_pictures`,
  `click_picture(&id, ClickMode)`; `Tree::bitmap_range`;
  `BitmapRow.target`. Fold `App.pictures` into `Selection::Bitmaps`.
- **Picking:** `pick::picture_under(scene, camera, at, &hit)` after
  `pick`; fold into `Hit::Bitmap`.
- **Properties:** `pictures::properties` gives `PictureProperties {
  placement, name, pixels }`, drawn with `properties::{FIELDS, field_text,
  parse}`; also `pictures::{title, PIXELS_LABEL, pixels_text, read_only,
  unlock, bounds}`.
- **Move and resize:** `pictures::edit`, `move_by`, `resize` return
  `Command::MoveBitmaps` or `ResizeBitmap`.
- **A spec change made here, for the maintainer to confirm:** capability
  56 said a picture does not scale. A picture can now be resized, aspect
  kept, about its centre (commit `2ddb4837`; reverting it restores the
  old rule).
- Importing a picture does not select it yet; it should.

### E. Docs and release preparation

| Slice | What | Status |
| --- | --- | --- |
| E1 | Book chapters and the quick start "From SVG to G-code in the app", with screenshots from the build (#550) | todo |
| E2 | macOS bundle and the phase 4 audit (#551) | todo |
| E3 | Bench check in the app and the `v0.4.0` tag | owner |

### F. Review plan, stage 2

Taken where a slice above already touches the code; the rest after E.
