# Alpha worklog

The local record of the push to an early alpha, started 2026-11-18. It
stands in for GitHub issues and pull requests while that process is paused.
One line per slice; details live in the commits and the specs.

## Where it stands (2026-11-19)

Every slice below marked done is merged on `alpha` and pushed. The gate
passes on macOS: 3,346 tests, twice running, with fmt, clippy and
check-headers clean.

To try it:

```sh
cargo run --release -p dreamcarve-app          # or:
tools/bundle-macos.sh && open target/bundle/DreamCarve.app
```

Then follow `docs/book/src/quick-start/app.md`. The pictures of every
state of the app are in `docs/specs/phase-4/review/final/`.

Waiting for the maintainer:

- The bench checks: 3.4 (#606), 3.5 (#649), 3.6 (#684), and the coaster
  from the app for phase 4.
- The image-prep defaults on their own art
  (`tests/fixtures/prep/README.md`, "Still to confirm").
- The decisions under "Decisions made while wiring", checklist items 4,
  15, 34, 64, 65 and 99 in `docs/specs/phase-4/review/final/README.md`,
  and the rest of "Decisions waiting for the maintainer" in
  `docs/specs/phase-4/audit.md`.
- The result of the sign's full cut (#429), which was closed without one.
- A run on Windows and Linux, then the tags `v0.3.0` and `v0.4.0`.

Not done, by choice, until after that testing:

- F4: `Warning` and `CamError` restructured.
- A7's remainder: the fail-loud work inside dc-geometry.
- Review-plan stage 3: the Profile restructure, the `offset` retry
  ladder, undo sharing, the S6 search.
- The release slice (E4): signed bundles, an icon, a double-clicked
  `.dcv` on macOS.

Smaller items still open are rows B4, B6, D8 and the notes under A.

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

### G. The maintainer's smoke test (2026-11-20)

| Slice | What | Status |
| --- | --- | --- |
| G1 | A vendor's tapered ball noses import: when `RE` is under `DC/2` and the shaft is no wider than `DC`, the tip is `2·RE` and the cutting diameter is `DC`, with the taper as the file gives it (the maintainer's decision) and a note saying so. A bull tip is still skipped, now with its numbers | done |
| G2 | `step_over_too_fine`: a warning for a stepover under 0.02 mm between passes, on a tool, a Pocket, a V-Carve's or Inlay's floor and a Photo V-Carve's lines | done |
| G3 | The window's issues, each with a test: a field keeps what was typed when it is left by any means (Escape alone discards); the Tool Library's columns resize and the sheet resizes, remembered in `gui.toml`; a boundary clears and disables Band margin and Outline rounding; Board thickness is first in the Male board group; help for every field, as a tooltip and as a line at the foot of the panel holding the focused field (35 strings new, 64 rewritten); a stepover says its distance; one **Programs** control that sets the project (the export-only override and the Tool change row are gone; acceptance 85 rewritten); **Export G-code** at the foot of the Export panel; the result as a sheet. A quote in a name is stored and shown as typed: the backslash in the maintainer's tool names was typed or pasted | done |
| G5 | Left by G3, now done: the Import drawing sheet resizes; the Boards ticks and the import sheets' choices have help on the control; Band margin keeps its value across a boundary and comes back with *None* (held in the window, never in the project) | done |
| G6 | A field's problem takes no room (the maintainer's request): no message line under any field; the field's outline and glyph mark it; a tooltip on the glyph or the field lists its findings; a click on the glyph opens the Warnings drawer at the finding; the focused field's problem shows first in the help line, and in the field's accessible description. Notes that are not problems, and the Export step's list of what blocks export, stay where they are. `redesign.md`'s Field component still says the message goes under the field | done |
| G7 | A single program lists its tool runs: under the file, "2 tools, 1 tool change", then each tool with what it cuts and its time, in Program Details, the result sheet and the setup sheet. `dc_job::ProgramSummary.runs`; the runs' times add up to the program's. CLI output and G-code unchanged | done |
| G8 | An angle is said both ways in the tool editor and the library's pane: a tapered ball nose's Taper per side shows its included angle, a V-bit's Angle its angle per side (`dc_app::tools::angle_note`). From the maintainer's test cuts of 2026-11-21: the Fine Detail bit entered as 6.7° per side made a plug that would not enter its pocket; measured pockets matched 3.35° per side to within 0.1 mm, and a coaster cut at 3.35° fitted. The vendor's file gives 10° for the same bit | done |
| G9 | The app shows the artwork checks (asked 2026-11-21, with clip art in mind; image prep itself stays a command). The checks moved to `dc_job::checks`, so `check`, `init` and the app word them alike. The drawing sheet lists them before anything is added (all of them for an SVG; `shapes_overlap` and `feature_below_size` for a DXF, for the layers ticked), with a line that says to run `dreamcarve image-prep` and a link to *Preparing artwork*; after the import those two are rows in the Warnings drawer for as long as the shapes are that way, the overlap's row selects both shapes, and Design is marked. Before this the app showed none of them, so *Copy* was not `check` for a drawing with these faults. Pictures: `docs/specs/phase-4/review/import-checks/`. Two faults found on the way and fixed test-first: `svg_hidden_object` said a `display:none` layer "is imported and would be cut" when usvg drops it and nothing of it is in the project (now a note of its own: left out, show the layer to cut it; objects hidden by `visibility`, zero opacity or no paint, which are imported, keep the old words); and `dc_geometry::merge_fills` failed as `Unresolved` for two arcs overlapping by less than the flattening tolerance, found by its own property test on a new seed (it now also asks `nest`'s test of crossing; the seed is kept in `proptest-regressions/fill.txt`). Gate after it: 3,430 tests | done |
| G4 | gSender's *Invalid lines detected* on `M61 Q1`: gSender's own check at file load; grblHAL answers `ok` to the line and the program runs. Documented in the Post-processors chapter. The maintainer decided on 2026-11-20 to leave the program as it is for now: the `M61` line stays, no `T1` word is added (so gSender lists only the second tool), and a single program keeps its first tool in its file name | done |

What the smoke test showed: an Inlay estimated at 1 h 23 min, 60 minutes
of it a tapered ball nose flattening a few small floor areas at 0.005 mm
between passes, because the tool's Stepover was 0.08 % where 8 % was
meant. At the vendor's 2.88 % the same job is 22 minutes. The tools had
been made by hand because the Fusion import skipped the vendor's tapered
ball noses.

### B. Phase 3

| Slice | What | Status |
| --- | --- | --- |
| B1 | Image-prep defaults (#599). The calibration was already merged (#638); re-run and confirmed: `min_detail` 0.2, `smooth` 0.05. The maintainer's run on their own art is described under "Still to confirm" in `tests/fixtures/prep/README.md` | owner |
| B4 | `image-prep`'s report gives a `Narrowest gap` of 0.19 to 3.0 mm for the same letters depending on `--smooth` (dc-geometry) | todo |
| B2 | Exit-criteria audit for 3.7 (#701): `docs/specs/phase-3/audit.md`. All 40 acceptance tests have tests; 21 are covered clause for clause and 19 partly. The CHANGELOG's unreleased section is up to date | done |
| B5 | The audit's gaps closed: all 20 clauses asserted, the two order tests written, fixtures for formats 21, 22 and 25, the spec corrected where it disagreed with the code, #438's cut record copied from the issue. The audit now reads 33 acceptance tests covered and 7 partial. Two wrong figures in the spec were corrected (acceptance 51: 36 %, not 40 %; acceptance 55: the mouth is 4 mm above the floor). The sign's full cut (#429) was closed with no result recorded | done |
| B6 | Found by B5, not changed: messages say "V-bit" for a tapered ball nose (dc-gcode, dc-job); V-Carve codes are spelled two ways (`vcarve_*` on three generator warnings, `v_carve_needs_v_bit` in validation) | todo |
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
| D4 | Export inlined into its step (a 440-px panel on that step); the Viewport's floating controls (tools and zoom, By operation / By depth and Layers, the cursor read-out); the drawer as a card; the status bar's cells; a blank project shows no problem count; Fit clears the legend | done |
| D5 | The review. The nine defects the book's screenshots showed are fixed, each with a test (count forms in the string table, labels that wrap, one focus ring round the control, the import preview's arcs, the origin label clear of the read-out, lists that fade, the Cut-out group's summary, two-decimal lengths, the sheet that did not close). 170 pictures of every state in `docs/specs/phase-4/review/final/`, and what they showed fixed. `experience.md`'s checklist rewritten: 100 items, 94 met (`review/final/README.md`) | done |
| D8 | Left by the review: checklist items 4, 15, 34 and 64 are contradictions between `redesign.md` and `experience.md` for the maintainer to settle (six surface shades in view where the brief says three; two accent buttons on the start screen; operation 2's red on chrome; 13-px tooltips); item 65, a dropdown's current row has no check; item 99, motion is 150 ms and reduced motion is not read; the View menu's uneven check column; long library names cut at 120 px; `drop_of_png_opens_the_picture_sheet` failed once with "exceeded max_steps" and passed on rerun; the d2, d3, d4 and d6 picture sets predate the polish | todo |

| D6 | The three features wired into the new window: the Add picture sheet, pictures as textured quads under the shapes, Photo V-Carve's editor, the tapered ball nose's fields, the library's Tip column | done |
| D7 | Preview marks follow the kind of cut: a Photo V-Carve draws one start per groove and no tab marks, and V-Carve and Inlay lose their false tab marks (98 on the sign). Still open: importing a 12 MP picture holds the UI thread about 214 ms in dc-app | done |

Decisions made while wiring, for the maintainer to confirm:

- A picture can be resized, aspect kept, about its centre, by its
  Properties or its corner handles. Capability 56 said a picture does not
  scale; it now says it does (commit `2ddb4837` in dc-app, and the
  window's handles).
- The Tool Library sheet is 1100 px wide (was 960) so its ten columns fit.
- The Export step lists the blocking findings itself, starts its three
  settings groups closed with a one-line summary each, and widens its
  panel to 440 px. With no folder chosen, **Export G-code** opens the
  folder dialog first.
- Acceptance 90 holds with two caveats the quick start states: the app
  names a new operation "Profile 1" and the tool by its library name,
  where `init` writes "Profile" and "Set me", so the coaster's program
  differs from the CLI's in two comment lines; and the sign and the inlay
  match the CLI's programs only when their tools are added in the order
  the chapter gives, because a clearing run's comment names the tool's id
  in the file.
- The app colours by operation; the CLI's `--preview` still colours by
  tool run, so the two differ on a project with clearing passes or an
  Inlay (the sign's Lettering is purple in the CLI's SVG and green in the
  app). dc-job was not changed.
- An operation that validation blocks draws no toolpath at all (it used
  to keep its last one, faded). A tool losing its feed therefore makes
  every operation using it vanish from the Viewport until the feed is
  back.
- *Project ▸ Project Settings…* keeps its name and opens the Stock step.
- *Add to project* in the Tool Library now closes the sheet (capability
  64 changed to say so). It used to stay open, and its scrim swallowed
  the next click.
- The zoom percentage is of 96 px per inch, and a click on it sets 100 %.
- The colour mode (By operation, By depth) is the Viewport control only;
  it left the Layers popover.
- A new carve takes the first tool its picker offers, so a project whose
  only carving tool is a tapered ball nose gets it.
- The artwork checks' advice notes (a stroke with no fill, a hole drawn
  as a separate path, a hidden object) do not mark the Design step: the
  quick start's coaster, an outline drawn as a hairline, has the first.
  Notes that say part of the drawing was left out do mark it.
- *Duplicate* puts the copy 10 mm off, over the original, so it raises
  `shapes_overlap` in the Warnings drawer until the copy is moved clear.

### E. Docs and release preparation

| Slice | What | Status |
| --- | --- | --- |
| E1 | The quick start "From SVG to G-code in the app" (the coaster, then the plaque, the sign and the inlay pair), tested by `quick_start_parts_from_the_app`; 32 screenshots written by `cargo test -p dreamcarve-app --test book -- --ignored`; chapters The app, Importing, Stock and origin, Toolpath preview, Exporting; `docs::book_builds_and_every_screenshot_exists` (#550) | done |
| E5 | The rest of the book for the app: new chapters Placing shapes and Operations; an "In the app" part in each operation chapter, Images, Photo V-Carve, Tool library, Tapered ball nose, Tabs, Warnings, Configuration and Post-processors; Keyboard checked against `keyboard.rs`; Crate map and Porting brought up to date; 32 more pictures; an anchor for each of the 150 warning codes. `mdbook build` has still not been run: mdbook is not installed here | done |
| D9 | The defects the chapters found, fixed: one colour per operation on its row, its toolpaths on both boards and the Layers key (`dc_app::operations::colour_at`); an operation that cannot generate draws nothing; a Warnings row focuses a tool's, a stock's or a settings field; a tool's row carries its findings; a damaged `gui.toml` is said once and kept until a preference changes, and `config path` lists the file; findings shown with names (`operation \`Cut out\` (\`op4\`)`); eleven book pictures regenerated | done |
| E2 | A local macOS bundle (`tools/bundle-macos.sh`) and the phase 4 audit, `docs/specs/phase-4/audit.md`: of 43 acceptance tests 11 are covered clause for clause, 31 partly, 1 waits for the maintainer; 17 clauses have no assertion | done |
| E6 | The phase 4 audit's gaps closed. The Inlay's **Boundary** row, proven by `gui_equality::inlay_boundary_set_in_the_app_equals_the_clis_project`; on the way, the generation digest now covers boundary shapes and deleting a shape clears it from `boundary`. 15 of the 17 clauses asserted or implemented (a disabled button now shows its reason; the Viewport reads `project::marks`); acceptance 91's string scan (`tests/strings.rs`); 22 spec rows corrected; Help gained *Quick Start* and *Keyboard Shortcuts*; the app announced in the CHANGELOG; `drop_of_png_opens_the_picture_sheet` no longer flaky. The audit now reads 18 covered, 24 partial, 2 clauses unasserted | done |
| E6 | The first release for testers, `v0.4.0-alpha.1` (asked 2026-11-22: unsigned downloads, with instructions to build from source). The workspace is at `0.4.0-alpha.1`; `dreamcarve-app` is in the release archives for the four dist targets with its installer scripts (`dist plan`; `release.yml` did not change); `THIRD-PARTY-LICENSES` is generated from the app's manifest and checked against both programs' crates, with BSL-1.0 accepted for `clipboard-win` and `error-code` alone, as `deny.toml` has it; both ignored release tests pass here (`third_party_licenses_cover_lockfile`, `release_archive_contains_third_party_licenses`); the workspace type-checks and passes clippy for `x86_64-pc-windows-msvc` and `x86_64-unknown-linux-gnu` from the Mac. Installing chapter and README rewritten for it. What CI runs and the local gate does not was run here too, and two things it would have failed on were fixed: seven doc links `cargo doc -D warnings` rejects, and `url` 2.5.8 (RUSTSEC-2026-0592, now 2.5.9); `mdbook build` and `cargo deny check licenses advisories` pass. Not run here: the tests on Windows and Linux, and `gcode-conformance`, which needs LinuxCNC's `rs274`. The merge to `main` and the tag wait for the maintainer's word | prepared |
| E4 | For the release slice (#551), not done here: the signed and notarised bundles on three platforms through cargo-dist; an app icon; opening a double-clicked `.dcv` on macOS, which arrives as an Apple Event the app does not read yet; the licence texts for the app's crates | owner |
| E3 | Bench check in the app and the `v0.4.0` tag | owner |

### F. Review plan, stage 2

Taken where a slice above already touches the code; the rest after E.

| Slice | What | Status |
| --- | --- | --- |
| F1 | dc-project: tests out of seven source files (2.25); `Problem`'s messages as `#[error]` attributes, `severity()` one `matches!`, `code()` from the variant names (2.1); one migration helper and a `STEPS` array, three test families as tables (2.18); `each_kind!`, `source!`, `id_type!` shared with dc-library (2.19); the comment sweep (2.23, 2.24). `validate.rs` 10,867 → 4,177 lines, `migrate.rs` 4,053 → 985; production code 8,890 → 8,047. Guarded by `problem_snapshot.txt` (147 samples, all 87 codes), unchanged. Not done: folding the eleven range variants into `OutOfRange`, which would change messages | done |
| F2 | dc-gcode: tests out of `safety.rs`, `emit.rs`, `material.rs`, `arcfit.rs` (8,331 → 2,614; 7,538 → 3,206; 3,977 → 2,913; 3,062 → 1,388 lines); the test hooks `FLIP_ARCS` and `Material::Unchecked` removed from the production paths (3.10); `arcfit`, `canned`, `emit` private, one dead item deleted (2.22); one `MachineMove::arc_centre` (2.13); `material.rs`'s 341-line module doc moved into `phase-2/design.md`, `safety.rs` now says what it shares with the emitter. No program or expectation changed | done |
| F3 | dc-cam: 13 test modules out of the source files (`profile.rs` 8,446 → 2,831 lines, `pocket.rs` 4,758 → 911, `drill.rs` 2,431 → 610, `tabs.rs` 3,340 → 1,609, `relief.rs` 3,412 → 1,285); the generators' opening checks as shared functions in `context.rs`, each generator calling them in its own order (2.5); `targets::target_profiles` for five generators (2.6); 17 bare tolerances named (2.14); the three long doc comments cut to about 30 lines. No move, error text or check order changed | done |
| F4 | `Warning` and `CamError` restructured (2.2, 2.4), after D4: it changes what dc-job, dc-app and the CLI read | todo |
