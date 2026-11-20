# Phase 4: exit-criteria audit for v0.4.0

Slice 22's audit (GitHub #551; worklog E2). It checks the phase 4 exit
criteria of `docs/plan.md` and every acceptance test, capability and safety
requirement of `docs/specs/phase-4/functional.md` against the branch `alpha`
at `558f53d5` ("Give a segment the track's full height as its click
target", 182 commits after `main` at `9e1049e9`), read on 2026-11-19. It
follows the form of phase 3's audit (`docs/specs/phase-3/audit.md`).

How it was made: by reading. Nothing was built or run for it: no `cargo
test`, no `mdbook`, no `dist`. Tests were found from an index of the 3,322
`#[test]` functions in the sources (518 of them this phase's: `dc-app` 288,
`dreamcarve-app` 147, `dc-job` 42, `dreamcarve-shell` 30, and 11 in the
app's files under `crates/dreamcarve/tests/`),
from the names in the design spec's test plan, and by searching for each
acceptance test's fixtures, codes and strings. "Covered" therefore means "a
test asserts it", not "the test passed today". A screenshot is never counted
as a test.

The clause-by-clause reading of acceptance tests 62 to 104 was split among
four readers (62 to 72, 73 to 83, 84 to 94, 95 to 104), each of whom opened
the test bodies. Fourteen of their findings, chosen because a status below
leans on them, were checked again against the sources and all held: the
string scan of acceptance 91, the CI workflow and the snapshot gate of 92,
Mach3's canned cycles (70), the plaque's operation kinds (74), the 24 of 33
golden directories that hold a project (62, 79), the 170 pictures and no 2×
file (104), the Inlay `boundary` exemption (73), the coaster's two
substituted comment lines (90), the `compared > 0` floor (83), the frame
limits (87), `lacks_rate` against `lacks_value` (71), `project::marks`'
callers (69), the five asks of the unsaved-changes sheet (88), and the
female-only export (76). The rest is as the readers reported it.

**Brought up to date by worklog E6**, on 2026-11-19, on a branch cut
from `alpha` at `11854373`. E6 closed what this audit found that could be
closed in the two GUI crates: the Inlay's drawn boundary, the clauses no
test asserted, the source scan of acceptance 91, the stale wording and
the rows where the spec and the code disagreed, and the changelog entry.
Where a status, a count or a row below changed, it says "E6"; everything
else is as the readers left it and was not read again. Unlike the audit,
E6 ran what it wrote: its section under "Evidence" has the commands and
their results. For the rows E6 touched, "covered" therefore also means the
test passed that day, on macOS, which is the one system this push
validates.

Three layers are named throughout:

- **headless**: `dc_app::test_support::Harness`, the harness the design spec
  names for the acceptance tests. It runs the app's model with the real
  `dc_job` functions and no window.
- **window**: `egui_kittest` on the real `Shell`, read through the
  accessibility tree.
- **binary**: the test runs the `dreamcarve` executable and compares bytes.

An acceptance test is **covered** when every clause is asserted as written,
at the headless layer or above. It is **partial** when a clause is asserted
on another fixture or with other values, more weakly than written, only on
the model where the clause is about what the window shows (a disabled
button, a status-bar cell, something drawn), or not at all.

Path shorthand: `app/` is `crates/dc-app/tests/`, `job/` is
`crates/dc-job/tests/`, `kittest::` is
`crates/dreamcarve-app/tests/kittest.rs::`, `window/library.rs` is
`crates/dreamcarve-app/tests/library.rs`, and `gui_equality::`,
`pipeline_move::` and `warnings_panel::` are files of that name under
`crates/dreamcarve/tests/`.

## Summary

- **Not ready to tag.** The app is built and the redesign is in. What
  blocks `v0.4.0` is listed under "What blocks the tag" below.
- **Exit criteria.** `docs/plan.md` gives three for phase 4: one is partly
  met, one is met, one waits for the maintainer. The functional spec gives
  eight: three are met (S20's six post-processors, `CONTEXT.md`, and,
  since E6, every string of the window keyed), two are not met (the
  chapters, the bundles), one is not shown (green on three operating
  systems in CI), one is half met and half waiting (the four parts, the
  bench cut), and one waits for the maintainer (the tag).
- **Acceptance tests.** There are 43: 62 to 104. All 43 have tests or, for
  104, a record behind them; none is without evidence. **18 are covered**
  (63, 67, 68, 70, 72, 74, 75, 78, 81, 83, 84, 85, 86, 88, 90, 91, 100,
  102); the audit found 11, and E6 brought 68, 70, 74, 84, 88, 90 and 91
  over, three of them (70, 90, 91) with the spec's wording corrected to
  what is true. **24 are partial** (31 before E6). **1 waits for the
  maintainer** (104, the visual review, now worded for the review that
  was done; its sign-off is the open step). No covering test is
  `#[ignore]`d or gated to one operating system by an attribute. Seven
  skip some or all of themselves at run time and still pass, listed
  under "Evidence": among them the rendered frame of acceptance 92
  without a GPU adapter, and the snapshot pair everywhere but Linux.
- **Clauses with no assertion at any layer: 2**, where the audit found
  17: that the smoke runs in CI on three operating systems (92), which
  this push cannot show, and the maintainer's sign-off (104), which is
  not a test. The other fifteen are listed with what became of each
  under "Clauses with no assertion".
- **Stale wording.** The six acceptance tests that named the old window
  or a state that has passed (64, 71, 76, 77, 82 and 104) are corrected
  (E6), with the sentences outside the acceptance tests that the audit
  listed. What is still old: the design spec's slices 23 to 35, as their
  own heading says, and seven test names.
- **Capabilities.** All 44 (50 to 80 with their lettered additions) are in
  the window. The two gaps the audit found are closed (E6): an Inlay's
  drawn `boundary` has its editor row (68), and Help opens the quick
  start, the Keyboard chapter and the user guide (79). The chapters were
  not counted again, since the book is being written beside this work;
  as the audit found them, 28 capabilities have their chapter, 7 are
  documented in part (57, 61, 62, 63, 65, 68a, 70), and 8 have none:
  58, 58a, 59, 66, 66a, 67, 68 and 69. One (80, strings) is not a
  user's matter.
- **Safety.** S20 is the one new rule. Its equality, refusal, no-invented-
  value and source-rule clauses are covered; "what the Viewport draws" is
  covered on the Scene, not on the pixels. E6 found and fixed one defect
  under it: the digest that says when an operation must be generated
  again did not cover the shapes of an Inlay's drawn boundary, so moving
  one left the Inlay's old toolpath standing as current (see "Safety
  requirements").
- **Slices.** Of the design spec's 37 slices (1 to 22, 23 to 36 and 31a),
  30 are closed on GitHub with their scope in the code. Three are open on
  GitHub and done on `alpha` (#535, #544, #685). Four are open and not
  done: the two visual reviews (#549, #628), the docs (#550) and the
  release (#551).

### What blocks the tag

1. **Three operating systems.** `alpha` has no CI run, and the worklog's
   gate ran on macOS only, by the maintainer's decision. Nothing shows the
   redesigned window on Windows or Linux. The functional spec's first exit
   criterion and acceptance 92 ask for all three in CI.
2. **The snapshot pair cannot pass where it is compared.** It was
   regenerated on macOS with the Linux-only gate lifted; the comparison
   runs on Linux only, and 22 commits have changed the window since. The
   CI workflow installs no software renderer, so on a runner without a GPU
   the test would print a line and pass having compared nothing.
3. **The release slice has not started** (#551, worklog E4): the app is
   kept out of the release archives (`dist = false`), there is no signed
   bundle on any platform, no icon, no licence texts for the app's crates,
   and a double-clicked `.dcv` does not open on macOS.
4. **The bench check** (worklog E3): the coaster cut from the app's
   program. The functional spec's "Test part" table is empty. It is the
   maintainer's.
5. **Documentation.** As the audit found it, one promised chapter does
   not exist (Placing shapes) and eight more are not brought up to date
   (worklog E5, under way beside this work and not read again here).
   `mdbook build` has not been run. AGENTS.md: a feature without a
   chapter is not done. The changelog's `[Unreleased]` section now
   announces the app (E6). New for the book from E6: the Inlay's Boundary
   row, and Help's two rows, are in no chapter.
6. **The visual review** (acceptance 104): six checklist items are not
   met and no sign-off is recorded. The spec now asks for the review
   that was done (1×, the README as its record), so the pictures are no
   longer short of it (E6).
7. **`v0.3.0` is not tagged.** The workspace is at `0.2.1`, and phase 3's
   audit lists what its tag still waits for.
8. **The tag itself**, the maintainer's.

Not blocking by themselves, for the maintainer to accept or turn into
slices: the 24 partial rows (31 before E6), the eight decisions made
while wiring, and the wording E6 changed that decides something, listed
under "Raised by this audit". The 17 clauses with no assertion are down
to the two in items 1 and 6, and the rows of "Where the spec and the code
disagree" are settled but for the one that needs CI.

## Evidence

- **Suite.** Not run for this audit. The worklog records the gate (`cargo
  fmt`, `clippy -D warnings`, `cargo nextest run --workspace`, the doc
  tests, `check-headers`) as run on macOS before each slice was marked
  done. No count for `alpha`'s head is recorded.
- **E6's run** (2026-11-19, macOS, on its branch from `alpha` at
  `11854373`). Everything E6 added or changed was run, and the worklog's
  gate was run whole at the end, the workspace's tests twice:

  | Command | Result |
  | --- | --- |
  | `cargo fmt --all -- --check` | clean |
  | `cargo clippy --all-targets --all-features -- -D warnings` | clean |
  | `cargo nextest run --workspace --no-fail-fast`, twice | 3,330 tests run, 3,330 passed, 24 skipped, both times (skipped are `#[ignore]`d: ten are the phase's picture writers, above; the rest are other crates') |
  | `cargo test --workspace --doc` | passes; the workspace has no doc test |
  | `cargo run -p check-headers` | OK, 462 `.rs` files |

  The audit's index counted 3,322 `#[test]` functions; E6 added 20 (six
  for the boundary, eleven for the clauses and Help, and the three of
  the string scan) and extended nine that were there. One flaky test was found, reproduced and fixed
  (`drop_of_png_opens_the_picture_sheet`; "Open items", `Harness::run`).
  On this machine there is a wgpu adapter, so the rendering tests
  rendered; the snapshot pair is still skipped off Linux. Nothing was
  run on Windows or Linux, and `mdbook` is not installed.
- **CI.** `gh run list --branch alpha` returns nothing. The workflow
  (`.github/workflows/ci.yml`) runs `cargo test --all` on `ubuntu-latest`,
  `macos-latest` and `windows-latest`, on a push to `main` and on a pull
  request, neither of which this push makes. `main`'s last run, at
  `9e1049e9`, is marked failed (the macOS job was cancelled); the last
  green one is `c8a83193` (2026-11-17). Both predate every commit of the
  redesign.
- **No software renderer in CI.** The design spec says "CI's Linux job
  installs Mesa's lavapipe", and `crates/dreamcarve-app/kittest.toml` and
  the header of `kittest.rs` assume it. The workflow has no such step: a
  search of `.github/workflows` for `lavapipe`, `mesa`, `WGPU`, `vulkan`
  and `xvfb` finds nothing.
- **`#[ignore]` in the phase's crates.** Ten tests, all picture writers,
  none an acceptance or safety test:

  | Test | Reason given |
  | --- | --- |
  | `crates/dreamcarve-app/tests/book.rs`: `quick_start_screenshots`, `the_app_screenshots`, `importing_screenshots`, `stock_screenshots`, `exporting_screenshots` | "writes the book's screenshots; run by hand" |
  | `crates/dreamcarve-app/tests/review.rs::review_screenshots` | "writes review pictures; run by hand with DC_REVIEW_DIR" |
  | `review.rs::redesign_d3_screenshots`, `redesign_d4_screenshots`, `redesign_d6_screenshots` | "writes the committed D3 (D4, D6) review pictures; run by hand" |
  | `review.rs::redesign_review` | "writes the committed final review pictures; run by hand" |

- **Tests that skip at run time**, and pass having asserted less:

  | Test | Skips | When |
  | --- | --- | --- |
  | `kittest::snapshot_workspace_light_and_dark` | Everything | On any system but Linux (`if !cfg!(target_os = "linux") { …; return; }`, "image snapshots are compared on Linux only (design question 2)"), and on Linux when no wgpu adapter is found |
  | `kittest::opens_plaque_renders_and_exports` | The rendered frame and its pixel assertions; it opens, saves and exports on the CPU harness all the same | No wgpu adapter ("no wgpu adapter on this machine, so the frame is not rendered…") |
  | `crates/dreamcarve-app/tests/renderer.rs`: `segment_quad_width_is_screen_constant`, `toolpaths_draw_in_operation_colours_and_by_depth`, `picture_draws_the_right_way_up_under_the_shapes`, and E6's `stale_toolpaths_render_faded` | Everything | No wgpu adapter |
  | `kittest::warnings_copy_reaches_the_clipboard` | The read-back from the operating system's clipboard | No clipboard ("this machine has no clipboard (no display server)…") |

- **Gated by attribute.** Four tests of `dreamcarve-shell` are
  `#[cfg(unix)]` (symlinks, file modes, an endless file): phase 1's staged
  writes, moved there by slice 1. No test of `dc-app`, `dc-job` or
  `dreamcarve-app` is gated by an attribute.
- **The test plan's names.** Of 223 backticked names in the design spec's
  test plan and slices, 12 are functions or modules, 202 are tests that
  exist as written, and 9 are not; see "Spec-named tests that do not
  exist".
- **GitHub**, read with `gh` on 2026-11-19, read-only: 41 issues carry the
  `phase-4` label, 34 closed and 7 open (#535, #544, #549, #550, #551,
  #628, #685). Pull request #666, "Second visual review (slice 36)", is
  open against `main` from `phase-4/issue-628-second-visual-review`, last
  updated 2026-11-12. #549 is open with five comments, the last on
  2026-11-12.

## Exit criteria

### `docs/plan.md`, phase 4

| Criterion | Status | Evidence |
| --- | --- | --- |
| A user can reproduce the phase 1–3 test parts from their SVGs without the CLI | **Partly met.** Met, headless, for the four parts the functional spec names; for the line-art inlay, the boundary can now be set in the app (E6) and the part as a whole is not shown; not shown for the photo and the tapered ball nose parts | `gui_equality::quick_start_parts_from_the_app` builds the coaster, the plaque, the sign and the inlay pair by the quick start's tables through the app's own functions, with no window, and compares the programs (acceptance 90). Phase 3 has five test parts. The line-art inlay needs a drawn boundary and `image-prep`, which is a command. The audit found the app could not set a boundary. It can now (capability 68), proved on the nearest committed project: the lettered pair given its frame as the boundary in the app is `dc-boundary.dcv` and exports the binary's programs for it, both boards (`gui_equality::inlay_boundary_set_in_the_app_equals_the_clis_project`). The compass itself is not built from the app by any test: its project is made by the chapter's run, not committed, and its drawing has to be prepared on the command line first. The photo V-carve can be made in the app (acceptance 77, partial); the tapered ball nose's fields are in the window (worklog C3); no test builds either part from the app. That a person at the window can do it rests on the 32 screenshots `tests/book.rs` writes when run by hand, and on the bench check, which is not done. |
| Quick start "From SVG to G-code in the app" | **Met in the repository; not published** | `docs/book/src/quick-start/app.md`, held to the build by `gui_equality::quick_start_parts_from_the_app` and `crates/dreamcarve/tests/docs.rs::book_builds_and_every_screenshot_exists`. The docs job deploys from `main`. |
| `v0.4.0` | **Waiting on the maintainer** | Not tagged (`git tag`: `v0.0.1`, `v0.1.0`, `v0.2.0`). The workspace version is `0.2.1`. |

### The functional spec's own list

| Criterion | Status | Evidence |
| --- | --- | --- |
| Acceptance tests 62–103 green on Windows, macOS and Linux in CI, and the second visual review (104) signed off on #549 | **Not shown** (three operating systems); **waiting on the maintainer** (the sign-off) | Every one of 62 to 103 has tests (below). Whether they pass on Linux and Windows is not known: `alpha` has no CI run and was validated on macOS only. What would show it: one green run of the merged branch on all three, with a software renderer installed on the Linux and Windows runners so that the rendering tests do not skip. This audit ran nothing. No sign-off of 104 is recorded anywhere in the repository. |
| S20's equality test (83) covers all six post-processors | **Met** | `gui_equality::app_export_equals_cli_export_on_six_posts`: `POSTS` is `generic`, `grbl`, `grblhal`, `linuxcnc`, `mach3`, `uccnc`, each on four projects and three program splits. Its only floor is `compared > 0` over the 72 cases; see acceptance 83. |
| The coaster, plaque, sign and inlay pair reproduced in the app from their drawings, programs identical to the CLI's (90); the coaster cut from the app's program and recorded above | **Partly met** (the four parts); **waiting on the maintainer** (the cut) | The parts are built headless from the chapter's tables; the coaster's program differs from the committed one in two comment lines; the other three are compared with what the binary writes for the committed projects. Acceptance 90 now says exactly that (E6) and is covered as reworded; "identical to the CLI's" in this criterion is therefore met but for the coaster's two names, which is decision 4 for the maintainer. The "Test part" table has no entry. |
| Quick start "From SVG to G-code in the app" and every chapter above published, with screenshots from the shipped build | **Not met** | The quick start and five of the seven new chapters exist with their screenshots (32 pictures written by `tests/book.rs`). **Placing shapes does not exist.** The operation chapters, Tabs, Post-processors, Configuration, Warnings and Inlay are not brought up to date, and Tool library only in part (worklog E5). Nothing is published from `alpha`, and `mdbook build` has not been run. See "Documentation deliverables". |
| `CONTEXT.md` updated with the three terms | **Met** | **Design Tree**, **Viewport** and **Toolpath Visibility** are defined (`CONTEXT.md` lines 542 to 555). `crates/dreamcarve/tests/quick_start.rs::context_terms_are_defined` lists all three. |
| Every user-visible string keyed (91) | **Met for the window's crate (E6)**; error and refusal text is left to phase 8, as the spec says | `app/strings.rs::every_key_is_used` proves the second half of acceptance 91, that every key of the table is used. The audit found no scan for the first half and saw two literals by eye. E6 wrote the scan (`crates/dreamcarve-app/tests/strings.rs`; acceptance 91's row says what it reads and what it lets through). Of the two: the window's first title, `with_title("DreamCarve")`, was unkeyed and is now `app.name`; the "does" column of `keyboard.rs` is the Keyboard chapter's English, which a test holds out of the window. `dc-app` is not scanned: its refusals are `Display` text like the engine's. |
| Binaries for all three platforms from cargo-dist; the app opens a `.dcv` by file association on each | **Not met** | `crates/dreamcarve-app/Cargo.toml` sets `[package.metadata.dist] dist = false`. `tools/bundle-macos.sh` writes a local `DreamCarve.app`: unsigned, no icon, this Mac's architecture. It declares the `.dcv` type, but a double-clicked file reaches a running macOS app as an Apple Event the app does not read. The binary takes a bare path (`main.rs`), which is how Windows and Linux pass an associated file; neither was tried. |
| `v0.4.0` tagged | **Waiting on the maintainer** | As above. |

## Acceptance tests 62 to 92

| # | Asserts | Covering tests | Status and notes |
| --- | --- | --- | --- |
| 62 Open, save, unchanged | Every golden fixture and the three quick-start projects saved to a new path equal the migrated original; the history is empty | `app/app.rs::save_unchanged_is_byte_identical`; `app/history.rs::open_clears_history`; `kittest::opens_plaque_renders_and_exports` | **Partial.** The per-fixture test compares `app.serialize()` in memory with `dc_project::save(dc_project::load(..))`; a file is written to a new path only for the plaque, in the window. No binary migrates anything. It opens the 24 of 33 golden directories that hold a `dreamcarve.dcv`, and `plaque`, `plaque-svg`, `sign`, `dc` and five tab-migration projects; no coaster project is committed, so the coaster is not among them. The empty history is asserted for every fixture. |
| 63 New Project from Drawing | `coaster.svg` gives `init`'s project without the tool and the Profile | `app/app.rs::new_from_drawing_equals_init_without_defaults`; `job/project_from.rs::drawing_without_defaults_has_no_tool_and_no_operation`; `pipeline_move::drawing_with_defaults_equals_init`; `kittest::new_from_drawing_through_the_sheet` | Covered. The app's project equals `project_from::drawing` with tools and operations cleared; `pipeline_move` ties that function to the `init` binary byte for byte; the window drives File ▸ New Project from Drawing… on the same file. |
| 64 Import DXF | `plaque.dxf` into an empty project: `init`'s shapes, named by layer, selected, one undo step; no units blocks; one layer; the unread-entity warning once | `app/import.rs::dxf_without_units_blocks_until_chosen`, `layer_subset`, `unread_entities_noted_once`, `one_import_is_one_undo_step`; `job/import.rs::drawing_into_places_as_drawn_and_returns_the_new_ids`; `kittest::drop_of_svg_opens_the_import_sheet` | **Partial.** No one test imports the whole file into an empty project: the layer test takes `holes` alone into an empty one, the undo test takes the whole file into the plaque. Names are compared with the importer's own, no layer name literally. The message is the importer's sentence without the CLI's `pass --units mm\|inch` hint, as the spec now says (E6). Ticking a layer, and choosing units to free the button, are headless only. The warning is asserted once in `App::notes()`, which feeds the drawer, not on the drawer. |
| 65 Add a picture | `gradient.png` at width 100 is `image add --width 100`'s bitmap at the origin, with the grayscale note | `app/import.rs::picture_at_width_equals_image_add`, `picture_size_follows_the_pixels`; `job/picture.rs::picture_into_appends_the_next_bitmap`; `kittest::drop_of_png_opens_the_picture_sheet`, `add_picture_and_see_its_tree_row` | **Partial.** Compared with `dc_job::picture::picture_into`, the function `image add` calls, not with the binary. The gradient is gray, so the note is asserted on `colour.jpg` at height 20; in the window it is asserted in the sheet, which is then closed without adding. |
| 66 Move, rotate, scale, mirror | Typed X and Y, rotate 90°, scale 50 %, the flips, a locked refusal in the status bar, each one undo step, the toolpath following | `app/properties.rs::typed_x_y_w_h_produce_the_expected_transforms`; `app/arrange.rs::rotate_90_and_flip_equal_the_typed_transforms`, `disabled_with_a_reason`, `partly_locked_turns_the_unlocked_and_names_the_rest`; `app/handles.rs` (three); `app/command.rs::locked_instances_refuse_transforms`; `kittest::drag_moves_on_release_as_one_step`, `locked_refuses_with_status_message`, `shortcuts_reach_the_arrange_commands`; E6: `kittest::locked_shape_has_read_only_fields_and_a_disabled_arrange_group` | **Partial.** The coaster's X already reads 10, so typing X 10 moves nothing; a typed X that moves is asserted on the plaque's square. The toolpath clause is asserted on that square and its operation, against `dc_job::generate_operation` on the reloaded file, and "translated by the same vector" is not asserted as such. Undo of the typed move is not asserted. Rotation and the flips are asserted absolutely on the plaque's square, headless; the window asserts only that a button and its shortcut agree. **E6:** the two clauses with no assertion are settled. The spec no longer names `locate` tests that never existed: it says the transforms are composed about the bounding-box centre, which is what is asserted. And it now says what a locked shape does: a drag and a nudge are refused in the status bar, and a typed value, Rotate and Flip cannot be asked for; the new window test asserts the read-only fields and the disabled group with its reason as the tooltip. That tooltip had never opened: see "Found while closing the gaps". |
| 67 Linked copy | Duplicate shares the definition; one definition, two instances; moving one leaves the other | `app/tree.rs::duplicate_is_a_linked_copy`; `app/command.rs::duplicate_is_a_linked_copy_offset_by_10_mm`; `crates/dc-app/src/app.rs::a_duplicate_becomes_the_selection`; `kittest::keyboard_shortcuts_reach_their_commands` | Covered, headless, on the plaque's square, through a saved and reloaded file. The window asserts only that Ctrl D adds an instance. |
| 68 Delete a target | Deleting the panel instance empties the Pocket's targets, gives `operation_has_no_targets`, removes its tabs; undo restores all three | `app/tree.rs::delete_removes_targets_and_tabs_and_warns`; `app/command.rs::delete_removes_targets_tabs_and_the_unused_definition`; E6: `kittest::deleting_a_target_lists_the_no_targets_error_in_the_drawer` | **Covered (E6).** The spec now names what the plaque has: `outline`, the Cut out Profile's target, with its tabs, and `square`, the one target of the Square recess Pocket; the test deletes both. The drawer's row is tied to the code: it is the error row at the operation's targets whose message is the one `dc_project::validate` gives under `operation_has_no_targets` (an error's row has no code, as the spec now says). The window test deletes the outline with the Delete key, reads the row in the open drawer, and undoes. Undo is whole-project equality. |
| 69 Stock, origin, heights | The fields `check` reads; the crosshair and park mark move; the tool-change height refused on generic; Fit stock equals `init`; thickness marked and export disabled | `app/project.rs::stock_origin_heights_write_the_fields`, `tool_change_z_offered_only_with_g53`, `thickness_unset_disables_export`; `app/commands.rs::fit_stock_to_shapes_equals_init`; `kittest::park_point_click_on_the_viewport`, `a_project_without_a_thickness_opens_on_stock`, `required_field_shows_required_not_a_message`; E6: `kittest::origin_crosshair_and_park_mark_move_on_the_viewport` | **Partial.** The fields are covered headless on the plaque. **E6:** "the crosshair and park mark move" is asserted on what the Viewport paints: the crosshair's four arms and its `0,0` label at the stock's corner and then at its centre, and the park mark's ring where the park point is set and then moved. The Viewport's overlay now reads `dc_app::project::marks()`, so the headless assertions are of what is drawn. The refused tool-change height, and *Fit to shapes*, are headless only. A disabled *Export G-code* is asserted in the window for a tool without a feed, never for an unset thickness. |
| 70 Post-processor switch | The dwell kept, `switch_post`'s resets; *one program* on grblHAL and not on GRBL; canned cycles on LinuxCNC and Mach3, the two whose definitions have them | `app/project.rs::post_switch_keeps_settings`, `m6_and_canned_cycles_offered_per_definition`; E6: `kittest::export_step_offers_m6_and_canned_cycles_where_the_post_processor_has_them` | **Covered (E6), with the spec corrected.** The spec said canned cycles is offered on LinuxCNC only. The bundled definitions say otherwise, and they are the authority: of the six, `linuxcnc/v2.json` and `mach3/v3.json` have `canned_cycles` (`G81`, `G82`), and `generic`, `grbl`, `grblhal` and `uccnc` have none. The spec now says so; the headless test names the two that offer and the four that refuse, and the window test reads the Export step's Post-processor group on each of the six: the checkbox or the line in its place, and *One program with M6* in the Tool change list or not. The switch itself is covered headless; in the window the post-processor is switched by its command, not picked from the Controller dropdown. |
| 71 Tools | Library add equals `tools add`; a cleared plunge rate marks the tool, lists the refusal and disables Export, and typing it back clears all three; Delete in use refused; the Fusion import's report and file | `app/tools.rs::add_from_library_equals_tools_add`, `missing_rate_is_an_error_and_blocks_export`, `remove_in_use_is_refused_naming_operations`; `app/library.rs::fusion_import_report_equals_tools_import`; `gui_equality::tools_add_equals_the_app`, `tools_import_report_equals_the_app`; `kittest::library_window_adds_ticked_rows`; `window/library.rs::import_review_counts_equal_tools_import`; E6: `kittest::delete_is_disabled_for_a_tool_in_use_and_names_its_operations` | **Partial.** The add and the import are covered at all three layers. The cleared rate is on the plaque's own tool, not the one just added. In the window, the disabled button and the drawer's row are asserted for a removed feed, not a plunge rate typed empty. Typing it back is headless only. **E6:** the red mark is now asserted through `tools::lacks_value`, which is what the Tools step draws its dot from, beside `lacks_rate`. *Delete* on a tool in use is asserted in the window: the button disabled, its tooltip the command's refusal naming the three operations, and enabled for an unused tool. The tooltip had never opened: see "Found while closing the gaps". The spec's wording is corrected to the Tools step's dot, the header's *Export G-code* and the library's review in place, whose counts and statuses are what the window shows of the report. |
| 72 Add an operation | *Add ▸ Profile* on the coaster: `init`'s defaults, the end mill, `Profile 1`; nothing selected; an open path | `app/operations.rs::add_targets_the_selection_with_cli_defaults`, `add_menu_partitions_by_eligibility` | Covered, headless. The end mill is added by a command, not from the library; the open path is the plaque's squiggle. No window test opens *Add* for a Profile: the window's *Add* is driven for Photo V-Carve and V-Carve only. |
| 73 Every field round-trips | Each field non-default, saved, read back by the CLI; default gives the default or `null`; no control outside the table; tool-derived placeholders | `app/editor.rs::fields_table_matches_the_records`, `set_and_get_by_key_path_round_trip`, `tool_derived_defaults_show_the_tools_value_and_write_null`, `fields::every_field_appears_once`, `profile_golden_render_order`, `pocket_and_engrave_golden_render_order`, `profile_tab_request_round_trips`, `drill_cycle_fields_follow_the_cycle` | **Partial.** "The CLI reads back" is `dc_project::load` in process. The table-driven write is `fields::set` on a detached operation, not a command through the app. Rows that are a choice, a toggle, a tab request or a point (`side`, `direction`, `strategy`, `entry`, `lead`, `corner_relief`, `machining_order`, `wall_pass`, `start`) get no save-and-load assertion there. "Set to its default" is compared in memory, not in a file. The table is compared with the record's leaves, not with the window: no window test lists the editor's widgets. **E6:** the Inlay's `boundary` is no longer exempt. It has its row, `still_to_come` is empty for every kind, and the row round-trips through the file with the rest. |
| 74 Reorder, enable, duplicate | The plaque's Bore dragged below its Panel pocket; unticking; Duplicate | `app/operations.rs::reorder_enable_duplicate`; E6: `gui_equality::reorder_and_untick_reach_the_saved_file_and_the_written_programs` | **Covered (E6), headless and binary.** The spec now names what the plaque has (a Bore, no Drill) and puts the copy below its original, as capability 65 does. The new test saves the app's project after the move and after the untick, has the binary export each, compares every program byte for byte with the app's, and reads the order of the operations in the written files and `"enabled": false` in the saved one. The toolpath's removal from the Viewport is asserted on the Scene. No window test drags a row, unticks one or presses Duplicate. |
| 75 Clearing passes | Two passes written in order; the smaller dragged above shows `clearing_not_descending` under the list | `app/editor.rs::clearing_list_orders_and_warns`, `add_pass_picks_the_next_smaller_end_mill`; `kittest::clearing_list_adds_and_drags` | Covered. The diameters are the sign's own tools. The window asserts the warning's text is somewhere in the window, not that it is under the list. |
| 76 Inlay, both boards | The values once and a Male board group; thickness cleared; set to 18 mm; both boards' programs; the male alone; the quick start's programs | `app/editor.rs::inlay_both_boards`; `app/scene.rs::male_board_sits_beside_the_stock_in_display_only`; `app/export.rs::board_tick_filters_without_saving`, `plan_lists_programs_as_the_cli_prints_them`; `app/pick.rs::male_board_shape_selects_the_inlay`; `kittest::inlay_editor_shows_the_male_board`; `gui_equality::app_export_equals_cli_export_on_six_posts`; `warnings_panel::copy_equals_check_on_the_binary`; E6: `app/export.rs::male_thickness_cleared_exports_the_female_board_only`, and for the boundary `app/editor.rs::inlay_boundary_is_one_of_the_projects_closed_shapes`, `app/scene.rs::male_band_follows_a_drawn_boundary`, `kittest::inlay_boundary_is_picked_in_the_editor_and_the_male_band_follows_it`, `gui_equality::inlay_boundary_set_in_the_app_equals_the_clis_project` | **Partial.** The mirror's X is compared with the binary at 12 mm. The derived line's red is asserted on the model. No window test opens the Export step on the inlay, or unticks a board. **E6:** with the thickness cleared, the plan lists the female board's programs only, under the CLI's names and with its bytes. The spec now says what is compared at which thickness (mark for mark at the fixture's 12 mm; at 18 mm, that the male is cut and drawn), and that the pair's programs are what the binary writes for the fixture, none being committed. The acceptance test gained the boundary: chosen in the app, the project is `dc-boundary.dcv`, the male's cuts end at the frame, and both boards' programs equal the binary's; that clause is covered at all three layers. |
| 77 Photo V-Carve | The gradient at `init`'s defaults draws `--preview`'s grooves; the groove count in the operation's status | `app/editor.rs::photo_v_carve_fields`, `photo_needs_a_picture_target`; `app/scene.rs::marks_equal_the_svg_marks`; `job/preview.rs::a_photo_v_carve_has_one_start_point_per_groove_and_no_tab_mark`; `kittest::photo_v_carve_is_added_and_its_editor_shows_its_rows` | **Partial.** The grooves are compared headless with the library's preview; no binary comparison includes a picture. The count is asserted on the status bar's message cell in the model (*Photo V-Carve · N grooves …*); no window test shows a groove count. **E6** corrected the spec: the maximum depth is typed, since `init` leaves it unset; the count is in the message cell and the drawer's note, not in "the operation's status"; phase 3.5 has landed. |
| 78 Tabs | *Place tabs* equals `tabs place --count 4`; drag; delete; the Tab tool; the program equals the CLI's | `app/tabs.rs::place_tabs_equals_tabs_place_and_clears_the_request`, `tab_drag_snaps_to_the_outline_in_definition_space`, `add_tab_at_clicked_point_with_last_size`, `hollow_when_no_profile_crosses`; `gui_equality::tabbed_coaster_exports_the_same_program`; `pipeline_move::place_request_equals_tabs_place`; `kittest::tab_tool_click_adds_a_tab`, `place_drag_and_delete_tabs` | Covered. The binary places `init`'s own request, which is a count of four. The saved file and the program are byte-equal to the binary's after placing and again after a drag, a delete and an add. The window tests use the plaque. |
| 79 Preview equals the toolpath | For each golden case the moves drawn are the generator's (count, endpoints to 1 µm); the SVG dump equals `--preview`'s | `app/scene.rs::marks_equal_the_svg_marks`, `svg_dump_equals_preview`; `job/preview.rs::marks_equal_what_the_svg_draws`; `pipeline_move::render_is_unchanged_after_the_move`; `gui_equality::app_export_equals_cli_export_on_six_posts` | **Partial.** The golden cases compared are the 24 of 33 that hold a project, and the plaque; the spec now says "each golden case that holds a DreamCarve project" (E6). "The moves the Viewport draws" are the Scene's strokes, the model the renderer reads, not what wgpu paints. "The Viewport's SVG dump" is `Scene::preview_svg`, which only tests call. It equals the library's render for the 24, and the binary's file for the coaster, plaque, sign and inlay. |
| 80 Depth colouring | *By depth* colours the Pocket's floor by Z; the legend's labels, positive and grey above the stock top; work coordinates | `app/view.rs::depth_legend_ticks_include_clearance_and_safe_z`, `depth_legend_labels_stock_top_each_depth_and_deepest`, `depth_legend_is_in_project_units`; `renderer.rs::toolpaths_draw_in_operation_colours_and_by_depth`; `kittest::floating_controls_never_cover_the_stock_at_fit`, `depth_legend_paints_under_a_sheet`, `depth_legend_leaves_the_extents_caption_readable`; E6: `kittest::depth_legend_puts_clearance_and_safe_z_in_the_rapid_grey_band` | **Partial.** The floor's colour is asserted as the ramp's step for its Z, not as the colour drawn; the renderer test counts pixels of the deepest stop only, and skips without an adapter. In inches only the first and last labels are asserted. **E6:** the grey band is asserted on what the legend paints: one bar in the Viewport's rapid colour at 35 %, the `+10 clearance` and `+5 safe Z` labels level with it and clearance above safe Z, the stock top's and the cuts' labels below it. The plaque is still one edited in the test, since the committed one has no clearance height. Z zero at the stock bottom is asserted in full. |
| 81 View state is not project state | Every visibility control, pan, zoom and panel size leave the file and the history alone | `app/prefs.rs::view_state_is_not_project_state`; `app/view.rs::visibility_toggles_change_no_file_and_no_history`; `kittest::eye_hides_one_operation_show_all_restores`, `floating_tools_select_fit_and_zoom`, `warnings_drawer_is_a_card_over_the_viewport` | Covered, headless: all nine visibility fields, both colour modes, the camera and the panel widths, then the saved bytes equal. The window drives a subset and asserts not dirty and no undo step, without comparing a file. |
| 82 Warnings drawer equals `check` | For each fixture with expected `check` output, the same codes, messages, operations and tiers; a located row selects and focuses; *Copy* | `app/warnings.rs::panel_equals_check_line_for_line`, `located_rows_name_the_field`, `copy_is_the_cli_text`, `tiers_match_the_cli`; `warnings_panel::copy_equals_check_on_the_binary`; `kittest::warnings_drawer_selects_and_copies`, `warnings_row_focuses_its_field`, `warnings_copy_reaches_the_clipboard`, `warnings_row_opens_the_step_that_owns_it` | **Partial.** All 20 fixtures with a `.warnings` file are compared, but 14 of those files are empty: 24 lines in six files carry the comparison, and none has an error. The click that selects an operation and focuses its field is asserted on other projects (a sample with its depth unset; the plaque with a depth cleared), not on a fixture of the 20. `check` prints a hint as `warning:`, so the hint tier comes from a list in the test; the spec now says the drawer tells a hint by its code (E6). *Copy* is asserted for all 20, and against the binary for eight files. |
| 83 Export equals the CLI | Coaster, plaque, sign and inlay, six post-processors, with and without the split override; the SVG dump | `gui_equality::app_export_equals_cli_export_on_six_posts`; `app/scene.rs::svg_dump_equals_preview`; `app/export.rs::plan_lists_programs_as_the_cli_prints_them`; `kittest::opens_plaque_renders_and_exports`, `export_sheet_disabled_on_errors_and_writes_all_or_none` | Covered, at the app's plan against the binary: 4 projects × 6 post-processors × 3 splits, each comparing the file names, every program's bytes, the preview and the report. Where the app refuses a case the test asserts the binary refuses it too and compares no bytes; the only floor is `compared > 0` over all 72, so nothing asserts that a given project, post-processor or split reached the byte comparison. The window writing those bytes is asserted for the plaque at the project's own split. |
| 84 Export is blocked by errors only | A tool lacking a feed: the error on the Export step, a click opens the tool, a disabled button; warnings alone export, and the status bar counts them | `app/export.rs::errors_disable_warnings_do_not`, `blocking_line_names_the_error_count`, `blocking_rows_are_the_drawers_errors`; `kittest::export_sheet_disabled_on_errors_and_writes_all_or_none`, `continue_goes_to_the_next_step_and_export_is_the_primary_on_export`; `app/steps.rs::export_needs_attention_when_an_error_blocks_it`; E6: `kittest::a_project_with_only_warnings_exports_and_the_status_bar_counts_them` | **Covered (E6).** The first three clauses were asserted in the window; the click on the finding now also asserts which tool the Inspector holds, by its name. The new window test opens the version 14 plaque, whose holes are too small for its drill: the status bar's counts cell reads the number of warnings the drawer lists and no error count, *Export G-code* is enabled, and pressed it writes every program the plan lists. |
| 85 Override not saved | *One program* overridden leaves `tool_change_mode` in the file | `app/export.rs::override_not_saved`, `the_program_split_applies_with_several_tools`; `gui_equality::app_export_equals_cli_export_on_six_posts` | Covered, headless: on the sign the one program holds `M6` and the saved bytes are those from before; repeated across the 72 cases. No window test touches the Export step's *Programs* control. |
| 86 Undo and redo | Twenty mixed edits back and forth, labelled; a new edit clears redo; three typed characters are one step | `app/history.rs::twenty_mixed_edits_undo_and_redo_in_order`, `new_edit_clears_redo`; `app/command.rs::labels_name_the_thing`; `kittest::field_commits_on_blur_not_per_keystroke` | Covered. The twenty include all seven kinds of edit the spec lists. The labels' words are pinned in `labels_name_the_thing`. The field clause is asserted in the window. |
| 87 The window never blocks | 500 shapes with a Pocket and a V-Carve: input within 100 ms, spinners, stale toolpaths faded, *Stop* within a second | `kittest::window_answers_input_while_generating`; `app/generation.rs::stop_clears_pending`; `app/scene.rs::stale_results_are_flagged_until_replaced`; `crates/dreamcarve-app/src/viewport/renderer.rs::style_words_pack_row_depth_stale_and_colour`; E6: `crates/dreamcarve-app/src/viewport/layers.rs::a_stale_operations_toolpath_is_drawn_faded`, `crates/dreamcarve-app/tests/renderer.rs::stale_toolpaths_render_faded` | **Partial.** The project is as written. "Within 100 ms" is asserted as: at least 60 frames timed by the wall clock, the 95th percentile within 100 ms, none over 250 ms, which is now the spec's own reading (E6); the frames that open the project are not timed. The spinners are asserted once. *Stop* is asserted by the wall clock on the window's state; the worker finishes the job it is on (design open question 4). **E6:** "stale toolpaths show faded" is asserted as drawn, on the plaque and not on the 500 shapes: every segment of a stale operation's toolpath carries the stale bit in the Viewport's draw list and none does once it is generated again; and, rendered, the Viewport holds under a tenth of the pixels it held in the runs' full colours while the toolpaths are still there. The rendered test renders nothing without an adapter. |
| 88 Unsaved changes | The sheet's three buttons; Cancel, Don't Save, Save; on close, quit, Open and each New | `kittest::unsaved_changes_prompt`, `unsaved_sheet_is_save_dont_save_cancel`, `save_on_untitled_asks_where_and_cancel_cancels_all`; `app/app.rs::dirty_tracks_the_saved_bytes` | **Covered (E6).** Asked on Close, Quit, Open, New, New Project from Drawing and now New Project from Picture: all six, in both tests. "Save writes the file before closing" is checked against the project's bytes for Close on a named project, and now for all six asks on an untitled project too: the file written is the changed project as the app serializes it, where it was only asserted non-empty. |
| 89 Units | An inch project shows every length in inches; `1/4 in` stores 6.35; `0.5` stores 12.7; the file unchanged | `app/units.rs::inch_project_displays_inches`, `unit_strings_parse_as_the_file_does`, `every_length_and_rate_field_accepts_unit_strings`; `app/view.rs::depth_legend_is_in_project_units`; `job/setup_sheet.rs::uses_display_units` | **Partial.** "Every length" is asserted for every length and rate row of every operation kind, nine Stock and Export fields, the park point and four tool fields, on the model. Not found in inches: the Viewport's read-out, the status bar's drag cell, the Stock step's summary line, and the carve notes. No window test renders an inch project's fields. The typed values go through `Slot::commit_text`, not the window. |
| 90 Quick starts without the CLI | The four parts built by the quick start's steps export the quick starts' committed programs | `gui_equality::quick_start_parts_from_the_app` (with `crates/dreamcarve/tests/app_steps/mod.rs` and the `<!-- app-test: … -->` tables of `docs/book/src/quick-start/app.md`) | **Covered as the spec is now worded (E6), headless and binary; the wording is decision 4, for the maintainer to confirm.** The chapter's tables are parsed and each value goes through the function the window calls; the chapter's prose steps are not run and no window is driven. The spec said every program is byte-identical to a committed one. It now says what is so and what the test asserts: the coaster's is the committed `coaster-grblhal.nc` but for two comment lines (`Profile 1` for `Profile`, the library tool's name for `Set me`) and is byte-identical once both are renamed; the plaque's, the sign's and the pair's are what the binary writes for the committed projects, no program of theirs being committed, with the tools added in the chapter's order. That another order gives another program is the worklog's statement, untested. |
| 91 Keyed strings | No user-visible literal outside the table; every key used | `app/strings.rs::every_key_is_used`, `setup_sheet_strings_are_keyed`, `placeholders_are_filled`; `crates/dc-app/src/strings.rs::keys_and_variants_agree`; E6: `crates/dreamcarve-app/tests/strings.rs::no_user_visible_string_literal_outside_the_table`, `the_keyboard_chapters_text_is_not_shown_in_the_window`, `the_scan_reads_literals_as_rust_does` | **Covered (E6), for the window's crate.** The second clause was covered. The first now has its scan: every string literal of `crates/dreamcarve-app/src` outside comments and test modules, read as Rust lexes it, must not read as words (a capital letter, or two words, once escapes and `{placeholders}` are out) unless it is accounted for: a lint's reason, `Debug` output, a compile-time path or name, a clap attribute, one of nine font and text-style names, or the Keyboard chapter's text in `keyboard.rs`, which a second test holds out of the window. Each entry of those lists must match a literal. It found one unkeyed string, the app's name (the window's first title), now the key `app.name`; the audit's other sighting, the Keyboard chapter's "does" column, is the book's English and is not shown in the window. What the scan does not do: one lower-case word reads as a key and passes; and `dc-app` is not scanned. Its refusals (`CommandError`'s `Display` and reasons) reach the window in English like the engine's errors, which the spec leaves to phase 8, and two other literals were seen there by eye: the unit `RPM` (`tools::field_unit`), and the part names `Status`, `Files`, `Run instructions` of `ExportResult::parts`, which only tests read. |
| 92 Three platforms | Launches, opens the plaque, renders a frame and exports, in CI on Windows, macOS and Linux | `kittest::opens_plaque_renders_and_exports` | **Partial; the three platforms are not shown.** "Launches" is the `Shell` in the test harness, not the binary as a process. The rendered frame is skipped where there is no adapter, and CI installs none. No run of `alpha` exists on any platform. |

## Acceptance tests 93 to 104

Added by the screenshot review, and reworded for the redesign.

| # | Asserts | Covering tests | Status and notes |
| --- | --- | --- | --- |
| 93 Window title, steps and Inspector | The title and the header's name; the five steps and the opening step; *Continue* and *Export G-code*; the Inspector; 1280 × 768 and narrower | `kittest::window_title_is_the_project_name_without_extension`, `rail_has_five_steps_and_a_project_opens_on_its_first_incomplete_one`, `continue_goes_to_the_next_step_and_export_is_the_primary_on_export`, `inspector_shows_properties_only_with_a_selection`, `a_project_without_a_thickness_opens_on_stock`, `inspector_lies_over_the_viewport_below_the_minimum`; `app/steps.rs` (24 tests) | **Partial**, on two clauses. "No widget shows `plaque.dcv`" scans accessibility labels in two states on one step; text-field values, the other steps and File ▸ Open Recent are not scanned. "With its error marked" and "the four steps complete" are asserted as the shell's step states, not as the mark the rail draws. Every other clause is asserted in the window as written. |
| 94 Status bar and floating controls | What the status bar never shows; the read-out; the colour mode and *Layers*; the generation cell and the estimate; *Regenerate all*; the units menu; cells that hold still; a blank project; Fit clear of the controls | `app/status.rs::cells_follow_the_state`, `stock_size_is_never_a_cell`; `app/steps.rs::a_new_project_counts_no_problem`; `kittest::toolpaths_button_opens_the_popover_and_escape_closes_it`, `status_bar_cells_hold_still`, `a_blank_project_shows_no_problem`, `regenerate_all_only_after_stop`, `units_menu_switches_display_units`, `floating_controls_never_cover_the_stock_at_fit`, `floating_tools_select_fit_and_zoom` | **Partial.** "Never shows the stock size" is asserted on the model's cells, not on the window's status bar. "No menu offers *Regenerate all*" is asserted for View, File and Project; Edit, Arrange and Help are never opened. The cells are asserted to hold when the generation cell and the pointer's cell come, not when they go. Fit's clearance leaves out the zoom percentage, and does not check the read-out again with By depth on. The rest is asserted in the window. |
| 95 Arrange | Rotate 90° clockwise, the popover's 30°, the flips, each one undo step; Properties are X, Y, W, H; the link broken; a locked shape disables the group | `app/arrange.rs::rotate_90_and_flip_equal_the_typed_transforms`, `disabled_with_a_reason`, `partly_locked_turns_the_unlocked_and_names_the_rest`; `app/properties.rs::placement_is_x_y_w_h_only`, `broken_link_scales_one_axis`; `kittest::shortcuts_reach_the_arrange_commands`, `inspector_shows_properties_only_with_a_selection` | **Partial.** Every transform, label and undo is asserted headless. In the window the buttons are pressed and asserted to make one undo step that equals the shortcut's; the direction and the value are not, and the popover is typed 90, not 30. "X, Y, W and H only" and the broken link are headless only. **E6:** the disabled group is asserted in the window, by `kittest::locked_shape_has_read_only_fields_and_a_disabled_arrange_group`: Rotate and both flips disabled with a locked shape selected, the reason in words (*Every selected shape is locked: unlock one to arrange it*) as each one's tooltip, and nothing turned by a press. |
| 96 Design Tree | The squiggle's open-path glyph and no `open` text; F2 and *Rename*; no *None* row | `app/tree.rs::rows_carry_a_shape_glyph`, `empty_sections_have_no_placeholder_row`, `rename_is_one_step`; `kittest::f2_and_context_menu_rename_in_place`, `design_tree_rows_select_hide_and_rename` | **Partial.** Renaming is asserted in the window. The glyph, the absent word and the absent *None* row are asserted on the row model, not on what the row draws. |
| 97 Editor layout | Targets, Tool and the Setup rows first; no Identity group; the header renames; closed groups with summaries; a located warning opens its group; every field once; the *Required* look | `app/editor.rs::fields::setup_rows_lead_each_kind`, `fields::every_field_appears_once`, `fields::no_identity_group`, `summary::closed_groups_summarise_their_values`, `summary::closed_cut_out_group_says_its_tool_and_tabs`, `summary::located_finding_opens_its_group`; `kittest::editor_header_renames_in_place`, `required_field_shows_required_not_a_message`, `editor_has_no_check_says_box` | **Partial.** The row order is asserted on the table for all eight kinds, not on the window. "Closed on first show" is asserted for the Pocket's groups; summaries' words for the Pocket and the Inlay's Cut-out, and elsewhere only as "differs from the title". The located warning is a made-up finding on the model. The header's rename, the absent name and enabled controls, *Required*, the absent message line and the tooltip are asserted in the window, on a Profile. **E6:** the glyph in an unset required field is asserted where the test asserts *Required*: `circle-x`, by its codepoint in the bundled icon font's own map, painted once on the field's line, inside its frame; for the tool's Feed, the Stock step's Thickness and a tapered ball nose's fields. |
| 98 Targets | The Pocket's targets in its colour, the rest faded, the tree rows marked; target editing; *Done* and *Cancel*; *Add to "Panel"* | `app/scene.rs::selected_operation_targets_are_marked`; `crates/dreamcarve-app/src/viewport/layers.rs::marked_targets_fade_the_rest`; `app/tree.rs::target_rows_marked_for_selected_operation`; `app/targets.rs` (four); `kittest::target_editing_bar_enter_is_done_escape_is_cancel`, `target_marquee_and_the_selection_shortcuts`, `hovering_an_operation_row_previews_its_targets`, `tree_context_menu_adds_and_removes_targets` | **Partial.** Editing, *Done*, *Cancel* and *Add to* are asserted in the window. The colour and the fade are asserted on the Scene and on the Viewport's draw list, not on pixels. The tree rows' bar is asserted on the row model only. |
| 99 Carve notes | The Lettering's note with *Deepest cut*; not counted in the status bar; *Copy* byte for byte; the two boards' sub-headings; one group, no heading | `app/warnings.rs::notes_follow_their_operation_and_are_not_counted`, `inlay_notes_have_board_subheadings`, `copy_includes_notes_as_check_prints_them`, `single_group_has_no_heading`; `app/status.rs::selected_operation_summary_in_message_cell`; `job/summary.rs::describe_carves_is_unchanged`, `carve_facts_format_to_describe_carves`; `warnings_panel::copy_equals_check_on_the_binary`; `kittest::message_cell_opens_the_drawer_at_the_note` | **Partial.** *Copy* is asserted against the binary on the sign and the inlay, for the lines the drawer lists (`check`'s extents and estimate lines are left out). The labelled fact, the sub-headings and the absent heading are asserted on the model; the window asserts one node holding `check`'s whole line. "Not counted in the status bar" is asserted on the drawer's counts, not on the status bar's cell. |
| 100 New tool | `tool1`, an end mill, 6.35 mm, 2 flutes, no rates, selected, its name focused; `tool2`; export refused | `app/tools.rs::new_tool_is_a_placeholder_named_by_number`, `new_tool_never_fills_a_rate`, `export_refuses_a_new_tool_until_its_rates_are_set`; `kittest::new_tool_focuses_its_name` | Covered. The geometry, the five unset values and `tool2` are headless; the focus and the typed rename are in the window. The refusal is asserted on the plaque with the new tool given to an operation, against `dc_job`'s findings, not the binary's output. |
| 101 Tool Library | Vendor groups; a feed edit writes the file, equal to the CLI's; the project and the history untouched; *Revert changes*; `0.25 in`; a second import shows *Changed*; *Replace with imported* | `app/library.rs::groups_by_vendor_with_no_vendor_last`, `revert_restores_the_tool_as_opened`, `new_and_duplicate`; `window/library.rs::edit_writes_the_library_and_not_the_project`, `units_are_display_only`, `import_review_counts_equal_tools_import`, `changed_tools_offer_keep_or_replace`, `no_choice_when_nothing_changed`; `kittest::import_review_replaces_the_panes_in_place`, `library_delete_asks_in_place_and_names_the_tools`; `gui_equality::tools_import_report_equals_the_app` | **Partial.** The groups are asserted on the model; no window test reads a group header. The file is compared with `dc_library::save` of the app's own edit, which is what the spec now asks (E6): the CLI has no command that edits a library tool, so "equal to the CLI's library after the same change" could not be asserted. The open project holds no copy of the edited tool, so the spec now says the project is left unchanged. The window shows `0.25` in the Diameter field; the string `0.25 in` is built in the test. Only the second import goes through the sheet. *Replace with imported* is compared with the library's `merge(replace: true)`; the binary's `--replace` is compared with the app only where nothing is *Changed*. |
| 102 Export step and result | No preview tick, warning list, total or note; Program Details closed; the blocking line; the folder asked for; the result; *Run instructions*; *Done*; the setup sheet | `kittest::export_sheet_has_no_preview_warnings_total_or_note`, `program_details_start_closed`, `export_sheet_disabled_on_errors_and_writes_all_or_none`, `export_asks_for_a_folder_when_none_is_chosen`, `export_result_shows_three_parts_and_saves_the_setup_sheet`; `app/export.rs::blocking_line_names_the_error_count`, `result_parts_are_status_files_and_instructions`, `result_holds_every_fact_of_the_cli_report`; `job/setup_sheet.rs` (four) | Covered, and in its new place: the window tests assert the Export step is open and no sheet is, and count nodes inside the step panel. Weaker than written in three spots: "over the finding" is not asserted as a position; "writes nothing when the dialog is cancelled" checks only the folder chosen later; the files' run order is asserted headless. The setup sheet's four properties are asserted on `dc_job::setup_sheet::render`, and the window asserts the written file is that render. |
| 103 Start screen and menus | The three actions, three example cards with thumbnails, the recent files, no sentence about phases; an example opens untitled; the import preview; the File menu; Appearance; the unsaved-changes sheet | `kittest::start_screen_offers_a_drawing_the_examples_and_recent_files`, `example_opens_untitled`, `file_menu_labels_in_order`, `appearance_has_three_checked_choices`, `unsaved_sheet_is_save_dont_save_cancel`; `app/import.rs::preview_is_the_drawing_alone`; `app/onboarding.rs::example_thumbnails_draw_the_examples_shapes` | **Partial.** The thumbnails are asserted as three non-empty outlines in the shell's state, and their geometry headless; not as drawn. "The import sheet's preview has no stock outline" is asserted on the model of the preview. *Save Setup Sheet…* is asserted to exist after an export, not its place in the File menu. The rest is asserted in the window. |
| 104 Visual review of the redesigned window | Section 13's 100 items, each ticked against a picture the app wrote or the test that holds it, or marked not met; both appearances at 1×; the record is the README; the maintainer's decision on the six and sign-off | None: a review, not a test. The record is `docs/specs/phase-4/review/final/README.md` | **Waiting on the maintainer.** The spec described the second review of the old layout (every panel, sheet and menu at 1× and 2×, a caption per picture, sign-off on #549). **E6** rewrote it for the review that was done, slice D5: 170 pictures of 85 scenes, dark and light, at 1×, which is what section 13 asks; the README says what each group of scenes holds and names the picture or the test beside each item; 94 items are met and 6 are not (4, 15, 34, 64, 65, 99). As reworded the review is done, and the open step is the maintainer's: to decide the six and sign off, in that README. #549 is open and pull request #666 carries the review of the old design; both are superseded. |

### Clauses with no assertion

A clause is listed here when no test asserts it at any layer. A clause
asserted on the model and not in the window is in the tables above, not
here. The audit found seventeen. E6 took each in turn: where the app did
it, the assertion was added; where the app did not, it was made to, test
first; where the clause was wrong about the app, the spec was corrected
to what is true and that is what is asserted. **Two remain**, neither of
which a test in this push can settle.

| # | Clause as the audit found it | What became of it (E6) |
| --- | --- | --- |
| 66 | Rotation and scale "produce the transforms the CLI's `locate` tests expect". | **Spec corrected.** No such tests ever existed. The spec now says the transforms are composed about the selection's bounding-box centre, which `app/arrange.rs` and `app/properties.rs` assert. |
| 66 | A locked instance refuses a typed value, Rotate and Flip "with a status-bar message". | **Spec corrected and asserted.** Those three cannot be asked of a locked shape: its fields are read-only and the Arrange group is disabled (58, 58a). The spec says so, and `kittest::locked_shape_has_read_only_fields_and_a_disabled_arrange_group` asserts it in the window. The group's tooltip did not open; fixed. |
| 68 | The Warnings panel shows `operation_has_no_targets`. | **Asserted.** The drawer's row is tied to the code through validation's own message, headless and in the window. The spec says an error's row has no code. |
| 69 | The Viewport's crosshair and park mark move. | **Asserted, and the code changed.** The Viewport reads `project::marks`; a window test reads the painted arms, label and ring before and after. |
| 70 | Canned cycles is offered on LinuxCNC only. | **Spec corrected and asserted.** LinuxCNC and Mach3, by the bundled definitions; headless by name, and in the window on all six. |
| 71 | *Delete* on a tool in use: the button disabled, its tooltip naming the operations. | **Asserted, and the code fixed.** The button was disabled, but its tooltip never opened. |
| 74 | The export's program order follows the reorder, in the written files. | **Asserted** on the files the binary writes for the app's saved project, with `enabled: false` in the saved file. |
| 76 | With the male thickness cleared, the export lists the female board only. | **Asserted** on the Export step's plan, against the CLI's names and bytes. |
| 80 | The clearance and safe-Z labels are in the rapid grey band. | **Asserted** on what the legend paints. |
| 84 | The status bar counts the warnings of a project that exports. | **Asserted** in the window, with the export pressed. |
| 87 | Stale toolpaths show faded. | **Asserted** on the Viewport's draw list, and on the rendered frame where there is an adapter; on the plaque, not on the 500 shapes. |
| 88 | *New Project from Picture…* with unsaved changes shows the sheet. | **Asserted**: the sixth ask, in both tests. |
| 91 | A source scan finds no user-visible string literal outside the table. | **Written**, for the window's crate; it found the app's name, now a key. |
| 92 | The smoke runs in CI on Windows, macOS and Linux. | **Open.** This push runs no CI and validates macOS only; the workflow is not this work's to change. It needs one run of the merged branch on all three, with a software renderer on the Linux and Windows runners. |
| 97 | The glyph in an unset Feed field. | **Asserted**, by the glyph's codepoint and its place in the field. |
| 104 | The pictures at 2×; a caption for each. | **Spec corrected.** Acceptance 104 now describes the review that was done: 1×, as section 13 asks, with the README naming the picture or the test beside each item. |
| 104 | Maintainer sign-off. | **Open**, and the maintainer's: the six items not met, and the sign-off, in `review/final/README.md`. |

### Acceptance tests whose wording is stale

The spec was rewritten for the redesign in capabilities 50, 50a, 50b, 60,
73, 77 and 79, and in acceptance 84, 93, 94, 102 and 103. These were
missed. In each, the tests assert the behaviour in its new place or not at
all, as the tables above say; the words are what is behind.

**E6 corrected all six**, each to what the "Now" column says, and with
them the sentences listed under the table: S20's "export dialog", 50a's
acceptance 92, capability 60's coordinates, the Properties panel of 69
and 75, capability 68's format, and "Warnings panel" wherever the spec
meant the drawer (the mock-up's file name and open question 13, which is
history, keep the old word). What was left as it is: the design spec's
slices 23 to 35, which their heading already says describe the old
window, with a note added to slices 24, 35, 36 and D3 where a sentence
was wrong about today's; and the seven test names, since the design
spec's test plan and this audit name them.

| # | Stale words | Now |
| --- | --- | --- |
| 64 | "disables *Import* until units are chosen" | The sheet is *Add drawing to project* and its button reads *Add shapes*. |
| 71 | "The library window's import … shows the report `tools import` prints"; "disables *Export*"; "marks the tool red" | The Tool Library is a sheet whose import is a review in place (64b), showing counts and a status per tool. The button is the header's *Export G-code* on the Export step. The mark is a red dot on the tool's row. |
| 76 | "the export dialog lists the female board only" | The Export step. The same paragraph says "the Export step" two sentences later. |
| 77 | "`check`'s groove count appears in the operation's status"; "Runs once phase 3.5 has landed" | The count is in the status bar's message cell and the drawer's note (76a); a row's status cell holds a spinner, a count or an error mark. Phase 3.5 has landed. |
| 82 | The title "Warnings panel equals `check`"; "the panel lists" | The Warnings drawer. Acceptance 64, 68 and 71 also say "in the Warnings panel", as capabilities 50, 59, 66, 71 and 76 still do; the book and the window say drawer. |
| 104 | "every panel, sheet and menu … at 1× and 2×"; "sign-off on #549" | The set is of steps, the Inspector, popovers, sheets and menus, at 1×, and `experience.md` section 13 asks for 1×. #549 is the review of the old design. |

Outside the acceptance tests: S20 says "the export dialog waits for
generation to finish"; capability 50a says "Acceptance test 92's
screenshots are reviewed against it" (92 has none; 104 does); capability
60 says "the status bar's coordinates count from the new point" where 50b
says the cursor is not in the status bar; capability 69 says a tab's size
is "in the Properties panel"; capability 68's Inlay row says "format 25".
In the design spec, slices 23 to 35 are written for the old window, as its
own heading says; slice D3's list says "*Export G-code* on Export opens
the sheet", which the test denies; slice 24 puts the depth legend "at the
Viewport's bottom-left". Seven tests and helpers keep old names for what
they now assert: `export_sheet_disabled_on_errors_and_writes_all_or_none`,
`export_sheet_has_no_preview_warnings_total_or_note`,
`plaque_export_sheet`, `toolpaths_button_opens_the_popover_and_escape_closes_it`
(it asserts there is no such button), `library_window_adds_ticked_rows`,
`tools_tab_holds_v_bit_until_an_angle_is_typed`, and
`app/warnings.rs::panel_equals_check_line_for_line`.

### Spec-named tests that do not exist

| Name in the design spec | What exists |
| --- | --- |
| `kittest::cam_panel_has_a_title_and_tools_first` | `kittest::rail_has_five_steps_and_a_project_opens_on_its_first_incomplete_one` (slice D3 records the rename) |
| `kittest::left_column_is_project_settings_then_properties` | `kittest::inspector_shows_properties_only_with_a_selection` (D3) |
| `kittest::project_settings_opens_on_a_project_error` | `kittest::a_project_without_a_thickness_opens_on_stock` (D3) |
| `kittest::units_is_the_first_row_of_project_settings` | `kittest::units_is_the_first_row_of_the_stock_step` (D3) |
| `kittest::empty_state_offers_two_paths` | `kittest::start_screen_offers_a_drawing_the_examples_and_recent_files` (D3) |
| `export::preview_svg_equals_the_cli` | `app/scene.rs::svg_dump_equals_preview` (slice 32 records it; the test plan's own list still names the old one) |
| `properties::typed_x_y_w_h_rotation_mirror_produce_the_expected_transforms` | `app/properties.rs::typed_x_y_w_h_produce_the_expected_transforms`, with rotation and mirror in `app/arrange.rs` |
| `editor::profile_tab_request_and_place_tabs` | `app/editor.rs::profile_tab_request_round_trips` and `app/tabs.rs::place_tabs_equals_tabs_place_and_clears_the_request` |
| `staged::existing_cli_tests_pass_unchanged` | Not one test: the moved tests of `crates/dreamcarve-shell/src/staged.rs` |

E6 brought the test plan's own list up to date for the last four (the
first five are the review slices', which record their renames): it now
names `scene::svg_dump_equals_preview`, the two tests each of the next
two became, and the moved tests of `staged.rs`, with the old name beside
each.

Five more exist under another crate than the plan gives.
`project_from::drawing_with_defaults_equals_init` and
`tabs::place_request_equals_tabs_place` are in `pipeline_move`, and the
plan lists them under `dc-job`. `library::edit_writes_the_library_and_not_the_project`,
`units_are_display_only` and `import_review_counts_equal_tools_import`
(with `changed_tools_offer_keep_or_replace` and
`no_choice_when_nothing_changed`) are in `window/library.rs`, not
`dc-app`.

## Capabilities and their chapters

`AGENTS.md`: a feature without a chapter is not done. "In the window" names
the model in `crates/dc-app/src/` and the panel in
`crates/dreamcarve-app/src/`; it means a module does it and at least one
test above exercises it, not that every sentence of the capability was
checked. A second documentation pass was running when this was read, so a
missing chapter is "not written at the time of this audit". Chapters are
under `docs/book/src/`.

| Capability | In the window | Chapter |
| --- | --- | --- |
| 50 One window, six regions | Yes: `steps.rs`; `shell/{header,rail,status,window}.rs`, `panels/side.rs` | `user-guide/the-app.md` |
| 50a Look and feel | Yes: `shell/theme.rs`, `widgets/` | `user-guide/the-app.md` ("Editing", "Appearance") |
| 50b Status bar | Yes: `status.rs`; `shell/status.rs` | `user-guide/the-app.md` ("The status bar") |
| 51 Project lifecycle | Yes: `app.rs`; `shell/files.rs`, `shell/menu.rs`, `panels/unsaved.rs`; `main.rs` for a path on the command line | `user-guide/the-app.md` ("Projects"), `user-guide/importing.md` |
| 52 Undo and redo | Yes: `history.rs`, `command.rs`; the header | `user-guide/the-app.md` ("Editing"), `user-guide/keyboard.md` |
| 53 Units | Yes: `units.rs`; the Stock step (`panels/project.rs`), the status bar's menu | `user-guide/stock-and-origin.md` ("Units"), `user-guide/keyboard.md` ("Typing values") |
| 54 Keyboard | Yes: `crates/dreamcarve-app/src/keyboard.rs` | `user-guide/keyboard.md`, whose table `keyboard.rs::chapter_table_matches_the_shortcut_table` holds to the code |
| 55 Add a drawing | Yes: `import.rs`; `panels/import.rs` | `user-guide/importing.md` |
| 56 Add a picture | Yes: `pictures.rs`; `panels/picture.rs`, `viewport/pictures.rs` | `user-guide/importing.md` ("The picture sheet", "A picture on the stock") |
| 57 Design Tree | Yes: `tree.rs`; `panels/tree.rs` | **In part.** `the-app.md` names it and says shapes are renamed, hidden and locked there. The rows' glyphs, the linked badge, the operation count, the context menu and the Tabs and Pictures sections are in no chapter. |
| 58 Placement | Yes: `properties.rs`, `viewport/handles.rs`; `panels/properties.rs` | **Not written.** Placing shapes does not exist. `the-app.md` has three sentences on the handles and X, Y, W, H. |
| 58a Arrange | Yes: `arrange.rs`; `panels/arrange.rs`, the Arrange menu | **Not written.** `keyboard.md` lists the shortcuts. |
| 59 Duplicate and delete | Yes: `command/instances.rs` | **Not written.** `keyboard.md` has the one line "Make linked copies of the selection". |
| 60 Project settings | Yes: `project.rs`; `panels/project.rs` on the Stock and Export steps | `user-guide/stock-and-origin.md`, `user-guide/exporting.md` ("Output settings") |
| 61 Fit stock to shapes, Centre on stock | Yes: `command.rs`; the Stock step, the Project and Arrange menus | **In part.** *Fit to shapes* is in `stock-and-origin.md`. *Centre on stock* is in no chapter. |
| 62 Post-processor | Yes: `project.rs` (`offers`, `switch_post`); `panels/project.rs` | **In part.** `exporting.md` describes the group and the switch. `user-guide/post-processors.md` does not mention the app. |
| 63 Project tools | Yes: `tools.rs`; `panels/tools.rs` | **In part.** `quick-start/app.md` adds tools from the library. No chapter describes the Tools step: its rows, the editor, *New tool*, *Duplicate*, *Delete*, *Save to library*, a tapered ball nose's fields. |
| 64 Tool Library | Yes: `library.rs`; `panels/library.rs` | `user-guide/tool-library.md` ("In the app"), in words, with no picture. It does not mention the Tip column, the labelled angles or the picker. |
| 64a Editing a library tool | Yes: `library.rs` (`edit`, `revert`) | `user-guide/tool-library.md` |
| 64b Importing into the library | Yes: `library.rs` (`review`) | `user-guide/tool-library.md` |
| 65 Operations list | Yes: `operations.rs`; `panels/operations.rs` | **In part.** `quick-start/app.md` ("How to add each operation") and a sentence in `the-app.md`. The grip, the checkbox, the eye, the status cell and the *Add* menu's two halves are in no chapter. |
| 66 Operation editor | Yes: `editor/`; `panels/editor.rs` | **Not written.** No operation chapter mentions the app, shows its editor or gives the field names (worklog E5). |
| 66a Setup, per kind | Yes: `editor/fields.rs` | **Not written**, as 66. The quick start's tables use the Setup rows of four kinds. |
| 66b Required values | Yes: `widgets/unit_field.rs`, `tools.rs` | `user-guide/the-app.md` ("Editing") |
| 67 Targets | Yes: `editor/targets.rs`, `scene.rs`; the Viewport's target bar | **Not written.** The book has no mention of target editing. |
| 68 Fields per kind | Yes. The audit found one setting with no row, an Inlay's drawn `boundary`; **E6 added it** (`editor::offered_boundaries`, `boundary_text`, `set_boundary`; `panels/editor.rs`), so every setting of every kind's record has its row | **Not written**, as 66. The Boundary row is in no chapter: `user-guide/inlay.md` sets a boundary by editing the file. |
| 68a Inlay: both boards | Yes: `editor/`, `scene.rs` (the male board); `panels/editor.rs` | **In part.** `quick-start/app.md` ("The inlay pair"). `user-guide/inlay.md` describes both boards from the command line and does not mention the app. |
| 69 Tabs | Yes: `viewport/tabs.rs`; `viewport/tab_tool.rs`, `panels/properties.rs` | **Not written.** `user-guide/tabs.md` does not mention the app. *Place tabs* is in the quick start and the Tab tool in one line of `toolpath-preview.md`. |
| 70 Clearing passes | Yes: `editor/clearing.rs`; `panels/clearing.rs` | **In part.** `quick-start/app.md` ("The sign"). `user-guide/v-carve.md` does not mention the app. |
| 71 Generation | Yes: `generation.rs`; `shell/worker.rs` | `user-guide/the-app.md`, `user-guide/toolpath-preview.md`. *Regenerate all* after a Stop is in neither. |
| 72 Viewport | Yes: `scene.rs`, `viewport/`; `viewport/{mod,renderer,controls}.rs` | `user-guide/toolpath-preview.md` |
| 73 Toolpath view | Yes: `view.rs`; `shell/toolpaths.rs` | `user-guide/toolpath-preview.md` ("Colours", "Layers") |
| 73a Depth legend | Yes: `view.rs`; `viewport/legend.rs` | `user-guide/toolpath-preview.md` |
| 74 Program extents | Yes: `view.rs`, `scene.rs` | `user-guide/toolpath-preview.md` ("Program extents") |
| 75 Selection on the Viewport | Yes: `selection.rs`, `viewport/pick.rs` | `user-guide/the-app.md` ("Selecting") |
| 76 Warnings panel | Yes: `warnings.rs`; `panels/warnings.rs` | `user-guide/the-app.md` ("Warnings"), `user-guide/exporting.md` ("The Warnings drawer"). `user-guide/warnings.md` does not mention the drawer. |
| 76a Carve notes | Yes: `warnings.rs`, `status.rs`; `dc_job::summary::carve_facts` | `user-guide/exporting.md` ("Notes") |
| 77 Export | Yes: `export.rs`; `panels/export_step.rs`, `panels/export.rs` | `user-guide/exporting.md` ("The Export step") |
| 77a Export result | Yes: `export.rs` (`result`); `panels/result.rs` | `user-guide/exporting.md` ("Exporting") |
| 77b Setup sheet | Yes: `dc_job::setup_sheet` | `user-guide/exporting.md` ("The setup sheet") |
| 78 Estimate and extents | Yes: `status.rs`, `export.rs` | `user-guide/the-app.md`, `user-guide/exporting.md` |
| 79 Start screen | Yes: `onboarding.rs`; `panels/empty.rs`. Help differed from the spec, offering the book's front page and About only; **E6 added** *Quick Start* and *Keyboard Shortcuts*, which open those two chapters of the published book (`shell/menu.rs::help_items`; `kittest::help_opens_the_quick_start_the_keyboard_chapter_and_about`). Until this phase's chapters are published the two pages are not there | `user-guide/the-app.md`, `quick-start/app.md` |
| 79a Menus | Yes: `shell/menu.rs` | `user-guide/the-app.md` ("Projects": the File menu), `user-guide/keyboard.md` |
| 80 Strings | Yes: `strings.rs`, `crates/dc-app/strings/en.toml` | Not a user's matter. `contributing/crate-map.md` names the table. |

Capabilities with no chapter, not written at the time of this audit: **58
Placement, 58a Arrange, 59 Duplicate and delete, 66 Operation editor, 66a
Setup, 67 Targets, 68 Fields per kind, 69 Tabs in the app.**

Documented in part: **57 Design Tree, 61 Centre on stock, 62
Post-processor, 63 Project tools, 65 Operations list, 68a Inlay in the
app, 70 Clearing passes in the app.** Two more have their chapter with
something left out: 64 (no picture, the Tip column, the picker) and 76
(the Warnings chapter does not mention the drawer).

## Safety requirements

S1 to S19 are phase 3's and are audited there; this phase changes no
generator, post-processor or check. S20 is the one new rule. Its clauses:

| Clause of S20 | Evidence | Status |
| --- | --- | --- |
| The program the app writes is `dc-gcode`'s output for the snapshot it holds, by the same generation call and safety pass as `dreamcarve export`, and equals the CLI's program for the saved file | `gui_equality::app_export_equals_cli_export_on_six_posts` (4 projects, 6 post-processors, 3 splits, against the binary); `job/build.rs::generate_operation_then_finish_equals_build`; `pipeline_move::cli_programs_are_byte_identical_after_the_move`; `kittest::opens_plaque_renders_and_exports` (the files on disk equal the plan's, for the plaque) | Covered, with acceptance 83's caveat: no floor says which cases reached the byte comparison. |
| No value reaches the engine that is not a field of the project; the app has no setting of its own that changes motion | `app/prefs.rs::view_state_is_not_project_state`; `app/view.rs::visibility_toggles_change_no_file_and_no_history`; `app/generation.rs::digest_covers_every_generator_input`; `app/export.rs::override_not_saved`, `board_tick_filters_without_saving` | Covered. The two per-export choices, the program split and the board tick, are the CLI's `--one-program`, `--programs-per-tool` and `--board`. |
| Export is refused on the CLI's conditions, and never offers a subset the file does not describe | `app/tools.rs::missing_rate_is_an_error_and_blocks_export`, `export_refuses_a_new_tool_until_its_rates_are_set`; `app/project.rs::thickness_unset_disables_export`; `app/export.rs::errors_disable_warnings_do_not`, `blocking_rows_are_the_drawers_errors`; the refusal branch of `gui_equality`'s `export_case` (the binary exits 1 with the same reason and writes nothing); `kittest::export_sheet_disabled_on_errors_and_writes_all_or_none` | Covered. In the window the disabled button is asserted for a missing feed, not for a missing thickness (acceptance 69). |
| The app never fills a feed, plunge rate, RPM or depth with a number of its own | `app/tools.rs::new_tool_never_fills_a_rate`, `no_default_v_bit_angle_is_written`, `switching_to_v_bit_holds_until_an_angle_is_typed`, `tapered_ball_nose_unset_fields_show_required`; `kittest::required_field_shows_required_not_a_message`, `tapered_ball_nose_fields_are_required_until_typed` | Covered for tools. A new operation's depth comes from the CLI's defaults for the kind (`app/operations.rs::add_targets_the_selection_with_cli_defaults`), as capability 65 says. |
| What the Viewport draws is the generator's toolpath for the current snapshot; a regenerating operation's old drawing is marked stale, and export waits | `app/scene.rs::marks_equal_the_svg_marks`, `svg_dump_equals_preview`, `stale_results_are_flagged_until_replaced`, `rebuilds_only_when_the_snapshot_changes`; `app/generation.rs::results_from_an_old_snapshot_are_dropped`; `app/export.rs::plan_waits_for_generation`; E6: `app/generation.rs::moving_an_inlays_boundary_shape_changes_its_digest`, `layers.rs::a_stale_operations_toolpath_is_drawn_faded`, `renderer.rs::stale_toolpaths_render_faded` | **Partial.** Asserted on the Scene, the model the renderer reads: nothing compares what wgpu paints with the toolpath. The fade of a stale run is now asserted as drawn (E6; acceptance 87). **One defect found and fixed by E6**, under the table. |
| The source rule: the GUI crates reach `dc-cam` and `dc-gcode` only through the CLI's entry points | `app/manifest.rs::manifest_lists_neither_dc_cam_nor_dc_gcode`; `crates/dreamcarve-app/tests/manifest.rs::manifest_lists_neither_dc_cam_nor_dc_gcode` | Covered. Both manifests list `dc-job` and neither forbidden crate. |
| "Checked by acceptance tests 71, 79, 83 and 84" | Above | 83 and 84 are covered; 71 and 79 are partial, for the reasons in their rows. None of the partial clauses is a way to get motion past the safety pass. |

**The digest did not cover an Inlay's drawn boundary** (found while
adding the Boundary row; fixed in the same commit). `generation::digest`
is what says a stored toolpath is still the current snapshot's. It
hashed an operation's record, its tools and the instances it targets,
and an Inlay's boundary shapes are not targets: with a boundary in the
file, moving or reshaping the shape that draws it left the Inlay's digest
as it was, so its old toolpath stood as current, was drawn unfaded, and
was what `finish` checked and the Export step planned. The app could not
set a boundary then, but it opens files that have one (the Inlay
chapter's, the line-art quick start's). By reading, not reproduced: the
safety pass derives the band from the project again (phase 3's
acceptance 61r) and so refuses old cuts that lie outside the new band,
which is a refusal the user could not have explained; old cuts that
still lie inside it would have been exported, a male cut to the edge as
it was, where the CLI cuts it to the edge as saved. The digest now
hashes each boundary instance and its definition. The test is on
`dc-boundary.dcv` with its frame moved 3 mm: the male generates
differently and the digest changes, and the same move in the pair
without a boundary changes neither. `digest_covers_every_generator_input`,
which mutates every leaf of three projects, could not see it: none of
the three has a boundary.

### Found while closing the gaps

Three things E6 met that the audit had not, all in the GUI crates and
all fixed with a test:

1. **The digest and an Inlay's boundary**, above.
2. **A disabled button never showed why.** `widgets/button.rs` draws a
   button the caller disables with a hover-only sense inside an enabled
   `Ui`, and then asked egui for the tooltip of a *disabled* widget,
   which egui opens only when the widget's `Ui` is disabled. So the
   reason on *Delete* for a tool in use, on Rotate and the flips for a
   locked selection, and on every other button the app disables with a
   reason, was never shown. It is now the button's hover text
   (`kittest::delete_is_disabled_for_a_tool_in_use_and_names_its_operations`,
   `locked_shape_has_read_only_fields_and_a_disabled_arrange_group`).
3. **Deleting a shape left it in an Inlay's `boundary`.**
   `DeleteInstances` took the shape off every target list and not off a
   boundary, which would have left the file naming a shape it no longer
   has (`inlay_boundary_unknown`, an error). It now leaves the boundary
   too (`app/command.rs::delete_takes_a_shape_off_an_inlays_boundary`).

## Slices

"In the code" was judged from the modules and from the test plan's names
(202 of the 211 test names exist; the nine others are in the table above).
The redesign (D3) moved what slices 3, 7, 8, 10, 18, 23, 24 and 34 built
to other parts of the window; the design spec's table "Redesign slices"
says where each went.

### The design spec's slices 1 to 22

| Slice | Issue | GitHub | In the code |
| --- | --- | --- | --- |
| 1 `dc-job` and `dreamcarve-shell` | #530 | Closed | Yes: both crates |
| 2 `dc-app` core | #531 | Closed | Yes |
| 3 Shell skeleton | #532 | Closed | Yes; the toolbar and the collapsing panels were replaced by D3 |
| 4 Viewport | #533 | Closed | Yes |
| 5 Design Tree and placement | #534 | Closed | Yes |
| 6 Import drawings and pictures | #535 | **Open** | Yes (worklog C1, D6): `import.rs`, `panels/import.rs`, `panels/picture.rs`; acceptance 64 and 65 |
| 7 Project panel | #536 | Closed | Yes; now the Stock step and the Export step's settings |
| 8 Tools tab and the Tool Library | #537 | Closed | Yes; now the Tools step and the sheet |
| 9 Generation and the Warnings panel | #538 | Closed | Yes |
| 10 Operations list and the editor framework | #539 | Closed | Yes |
| 11 Profile, Pocket and Engrave editors | #540 | Closed | Yes |
| 12 Drill and Bore editors | #541 | Closed | Yes |
| 13 V-Carve editor and clearing passes | #542 | Closed | Yes |
| 14 Inlay editor and the male board | #543 | Closed | Yes. It closed without the drawn boundary (format 26), which the editor's table listed as still to come; worklog E6 added the row |
| 15 Photo V-Carve editor and bitmaps | #544 | **Open** | Yes (worklog C2, D6, D7): `pictures.rs`, `viewport/pictures.rs`, the editor's rows; acceptance 77 |
| 16 Toolpath display | #545 | Closed | Yes; the view's controls moved to the Viewport (D4) |
| 17 Tabs | #546 | Closed | Yes |
| 18 Export dialog, result sheet, preview SVG, estimates | #547 | Closed | Yes; the dialog became the Export step (D4), and the preview tick went with slice 32 |
| 19 Keyboard and units polish | #548 | Closed | Yes |
| 20 Visual review against `experience.md` | #549 | **Open** | The first review became slices 23 to 36. Its exit is the sign-off of acceptance 104, not given. |
| 21 Docs, quick start and vocabulary | #550 | **Open** | **Partly** (worklog E1 done, E5 todo): the quick start, five chapters, `CONTEXT.md`. See "Documentation deliverables". |
| 22 Release: bundles, bench check, audit, `v0.4.0` | #551 | **Open** | **No**, but for this audit and a local macOS bundle (worklog E2, E3, E4). |

Follow-ups under the same label, all closed and in the code: #573 and #593
(the frame-time test of acceptance 87, made a percentile and freed of a
race), #576 (click a toolpath to select its operation:
`kittest::clicking_a_cut_opens_its_operation`), #581 (the V-bit's angle
held until typed, the library's Delete asked in place, the clipboard).

### The review slices 23 to 36

| Slice | Issue | GitHub | In the code |
| --- | --- | --- | --- |
| 23 Window title, panels and the CAM panel | #615 | Closed | The title, yes. The CAM panel and Project Settings were replaced by D3; four of its five tests were renamed. |
| 24 Status bar, toolpath view and the Warnings drawer | #616 | Closed | Yes; the Toolpaths button and the cursor cell left the status bar (D4) |
| 25 Design Tree and Arrange | #617 | Closed | Yes; the Arrange group moved from the toolbar to the Inspector |
| 26 Editor framework: header, Setup and summaries | #618 | Closed | Yes |
| 27 Targets on the drawing | #619 | Closed | Yes |
| 28 Carve notes | #620 | Closed | Yes |
| 29 Tools tab: New tool and Delete | #621 | Closed | Yes |
| 30 Tool Library: vendor groups, editing and units | #622 | Closed | Yes |
| 31 Library import review | #623 | Closed | Yes |
| 31a Tapered ball nose in the app | #685 | **Open** | Yes (worklog C3, D6): all seven tests the slice names exist, with four window tests in `kittest.rs` and three in `window/library.rs` |
| 32 Export sheet trimmed | #624 | Closed | Yes, on the Export step |
| 33 Export result and setup sheet | #625 | Closed | Yes |
| 34 Empty state | #626 | Closed | Yes, as the start screen |
| 35 Import sheet, menus and the unsaved-changes sheet | #627 | Closed | Yes |
| 36 Second visual review | #628 | **Open** | **Superseded.** Pull request #666 is the second review of the old window: its pictures and ticks describe a layout that no longer exists. Worklog D5 is the review of the new one. #666 is for the maintainer to close; the sign-off is still owed. |

### The redesign's slices, in the worklog only

| Slice | Worklog | In the code |
| --- | --- | --- |
| D1 The brief | done | `docs/specs/phase-4/redesign.md` |
| D2 Tokens and widgets | done | `shell/theme.rs`, `widgets/`; `crates/dreamcarve-app/tests/theme.rs` |
| D3 The shell | done | `steps.rs`; `shell/{header,rail}.rs`, `panels/side.rs`, `panels/empty.rs`; all twelve tests the slice names exist |
| D4 Export as a step; the floating controls | done | `panels/export_step.rs`, `viewport/controls.rs` |
| D5 The review | done | `review/final/` and its README; nine fixes, each with a test |
| D6 The three features in the new window | done | As slices 6, 15 and 31a |
| D7 Preview marks by kind of cut | done | `job/preview.rs::a_photo_v_carve_has_one_start_point_per_groove_and_no_tab_mark`, `only_a_cut_that_leaves_tabs_has_tab_marks` |
| D8 Left by the review | **todo** | See "Decisions waiting for the maintainer" |

## Decisions waiting for the maintainer

### Made while wiring (worklog), not yet confirmed

1. **A picture can be resized**, its proportions kept, about its centre,
   by its Properties or its corner handles. Capability 56 said a picture
   does not scale; it now says it does, "for the maintainer to confirm
   with the resize itself", and "Out of scope" was reworded to match.
   Tested: `app/pictures.rs::resizing_a_picture_keeps_its_proportions_and_its_centre`.
2. The Tool Library sheet is 1100 px wide (was 960) so its ten columns
   fit.
3. The Export step lists the blocking findings itself, starts its three
   settings groups closed with a line each, and is 440 px wide. With no
   folder chosen, *Export G-code* opens the folder dialog first.
4. **Acceptance 90 holds with two caveats** the quick start states: the
   coaster's program differs from the CLI's in two comment lines, and the
   sign and the inlay match only with their tools added in the chapter's
   order. The spec's text of 90 said byte-identical to the committed
   programs; E6 reworded it to these two caveats, so confirming this
   decision is confirming that text.
5. *Add to project* in the Tool Library closes the sheet (capability 64
   was changed to say so).
6. The zoom percentage is of 96 px per inch, and a click on it sets 100 %.
7. The colour mode is the Viewport's control only; it left the Layers
   popover.
8. A new carve takes the first tool its picker offers, so a project whose
   only carving tool is a tapered ball nose gets it. The design spec's
   tool-library section still says such a project "gets no tool there".

### The six checklist items not met (`review/final/README.md`)

| Item | What is wrong | Whose call |
| --- | --- | --- |
| 4 No more than three surface shades in view | Six are in view; `redesign.md`'s principle 2 says three and its own tables name seven tokens | The two disagree: the maintainer settles it |
| 15 One accent button in a view | The start screen has *New from drawing…* and the header's *Continue*; `redesign.md` rules out two and `experience.md` allows these | The two disagree |
| 34 Operation 2's red only as a line on the Viewport | The Operations list's glyph and the Layers swatch draw it on chrome, close to the error colour; two sections of `experience.md` disagree | The two disagree |
| 64 A tooltip | Its sentence is 13 px (`experience.md`), not `redesign.md`'s 12 | The two disagree |
| 65 A dropdown's current row ticked | No check: egui's `selectable_value` | A fix, or accept |
| 99 Motion, and none when the OS asks | 150 ms where `redesign.md` says 120; the OS's reduced-motion setting is not read | A fix, or accept |

The review's "Seen and left" adds four the checklist does not cover: a
blank project's status bar says *No problems* while its Export step says
*1 error*; the View menu's uneven check column; a library row's name cut
at 120 px; the dark stock's grid at 1.1:1. And the picture sets in
`review/d2/`, `d3/`, `d4/` and `d6/` predate the review's fixes.

### Raised by this audit

- **An Inlay's drawn boundary in the app.** Done (E6). Capability 68 says
  the editor exposes every setting of the kind's record, and the Inlay's
  `boundary` had no row. It has one, in Depth before the margin it
  replaces: a dropdown of *None (use the band margin)* and the shapes
  validation accepts as a boundary, with *Selected shapes* for a band
  drawn by several. Choosing writes `boundary` alone, one undo step; the
  margin and the rounding are for the user to clear, under validation's
  `inlay_margin_ignored`. What is left of it for the maintainer: whether
  choosing a boundary should clear those two itself, and that the shape
  is picked from a list, not on the drawing as targets are. The line-art
  part as a whole still needs `image-prep`, which is a command.
- **Which test parts the exit means.** `docs/plan.md` says "the phase 1–3
  test parts"; the functional spec and acceptance 90 name four. Phase 3
  has five.
- **Three operating systems.** The first exit criterion asks for CI on all
  three; this push validated macOS only. Whether `v0.4.0` waits for that
  run is the maintainer's to say.
- **Pull request #666 and #549.** Close #666 as superseded. Acceptance
  104 as E6 reworded it puts the sign-off in `review/final/README.md`;
  say so if it belongs elsewhere.
- **Design open question 7** (should the CLI get `export --setup-sheet`)
  is still "open until the maintainer answers".
- **`experience.md` still says "Status: draft".**
- **The 24 partial rows** (31 before E6): accept each, or name the ones
  to close. Most of what is left in them is of four kinds: a clause
  asserted on the model where it is about what the window draws (77, 79,
  93, 94, 96, 98, 99, 103); a comparison with the function the CLI calls
  where the spec says the binary (62, 65, 73); another fixture or other
  values than the spec's (64, 66, 82, 87, 101); and what needs three
  operating systems (92).
- **Wording E6 changed that decides something**, for the maintainer to
  confirm: acceptance 90 (decision 4 above, now the spec's text);
  acceptance 104 (the review that was done, 1×, the README as the
  record); acceptance 91 (error and refusal text left to phase 8, as the
  design spec already said of the engine's); acceptance 87 (the design
  spec's percentile reading of "within 100 ms").
- **Help's two new rows** open pages of the published book that do not
  exist until `alpha` is merged and the book deployed.

### Where the spec and the code disagree

None of these changes machine output.

| Where | The spec says | The code and tests say |
| --- | --- | --- |
| Capability 68; acceptance 73 | The editor exposes every setting of the kind's record | An Inlay's `boundary` has no row (`still_to_come(Kind::Inlay)`) |
| Capability 79 | Help opens the quick start for the app, the Keyboard chapter, and About | *User Guide* (the book's front page, as published from `main`) and *About DreamCarve* (`shell/menu.rs::help_items`) |
| Acceptance 70 | Canned cycles is offered on LinuxCNC only | Mach3's definitions have canned cycles too, and the offer follows the definition |
| Acceptance 66 | "The transforms the CLI's `locate` tests expect" | No such tests |
| Acceptance 66 | A locked instance refuses each transform with a status-bar message | A drag and a nudge do; typed fields are read-only and the Arrange group disabled, as 58 and 58a say |
| Acceptance 68 | Deleting "the plaque's panel instance" empties "the Pocket" | The plaque has no such instance; the tests delete `outline`, the Cut out Profile's target |
| Acceptance 68, capability 59 | The panel shows `operation_has_no_targets` | The panel prints an error without its code |
| Acceptance 74 | "The plaque's Drill"; Duplicate "appends" | The plaque has a Bore and no Drill; the copy goes below its original (capability 65) |
| Acceptance 76 | Set to 18 mm, the male board shows as `--preview` draws it | The fixture's male board is 12 mm, and the detailed comparison is at 12 |
| Acceptance 79 | "For each golden case" | 24 of the 33 golden directories hold a project |
| Acceptance 82, capability 76 | "Three tiers as `check` prints them" | `check` prints a hint as `warning:` |
| Acceptance 83 | "Inlay-pair projects" | One project, `dc.dcv`, since #498 |
| Acceptance 87 | "Within 100 ms" | The 95th percentile within 100 ms and no frame over 250 ms (the design spec, after #573) |
| Acceptance 90 | Byte-identical to "the quick starts' committed programs" | A committed program exists for the coaster only, and the app's differs from it in two comment lines |
| Acceptance 91 | "A source scan finds no user-visible string literal" | No such scan; the design spec describes the key scan only |
| Acceptance 92; design, "Test harness" | In CI on three platforms; the Linux job installs lavapipe | The workflow installs no renderer; `alpha` has no run |
| Acceptance 101 | A feed edit gives a file "equal to the CLI's library after the same change" | The CLI has no command that edits a library tool |
| Acceptance 104; design, slice 36 | 1× and 2×; "items 1–34"; a checklist comment on #549 | 1× only; 100 items; a README in the repository |
| Capability 52, 58a | *Move "Oval"*; *Rotate "square" 90°* | Curly quotes; `Rotate “square” 90° clockwise` |
| Capability 50b | *"square" is locked* | `shape “square” is locked` |
| Capability 79a | Project ▸ *Fit Stock to Shapes* | `Fit Stock to Shapes…` |
| Design, slice 35 | System is checked by default | Dark is, as capability 79a says |
| Design, slice 24 | With Z zero at the stock bottom the cuts' ticks are "positive or zero" | The test asserts `−0.2 Screw holes, Cut out` |
| Design, slice D3 | *Export G-code* on Export "opens the sheet" | It writes the programs; the test asserts no sheet opens |
| Design, "The Tool Library" | A project with only a tapered ball nose "gets no tool" on a new carve; the library's type filter "has no tapered ball nose entry yet" | It gets it (worklog decision 8; `app/operations.rs::a_new_carve_takes_the_first_tool_its_picker_offers`), and the filter has the entry (`app/library.rs::the_type_filter_has_a_tapered_ball_nose_entry`) |

**What E6 did with the 25 rows.** The table is kept as the audit wrote
it. Of its rows:

- **The code was brought to the spec in two**, both small and both in
  the GUI crates: the Inlay's `boundary` row (capability 68, acceptance
  73), and Help's quick start and Keyboard chapter (capability 79).
- **The spec was brought to the code in twenty-two**, in its own voice,
  the code and the tests being right: acceptance 66 (both rows), 68 (both
  rows, with capability 59), 70, 74, 76, 79, 82 (with capability 76), 83,
  87, 90, 91, 101 and 104 (with the design spec's slice 36); capabilities
  52 and 58a, 50b and 79a; and the design spec's slices 35, 24 and D3
  and its Tool Library section. For 70 the definitions were read first:
  LinuxCNC and Mach3 have canned cycles, the other four have none. For
  91 the scan was written, and the design spec now describes it.
- **One is recorded, not changed**: acceptance 92 and the design spec's
  "Test harness". The workflow installs no renderer and `alpha` has no
  run; the workflow is not this work's to edit, and the push runs no CI.
  The design spec now says the lavapipe step is not there yet.

## Open items that bear on the release

| Item | What is open | Checked |
| --- | --- | --- |
| The release slice (#551, worklog E4) | Signed and notarised bundles on three platforms through cargo-dist; the app in the release archives; an icon; a double-clicked `.dcv` on macOS; licence texts for the app's crates | `crates/dreamcarve-app/Cargo.toml`: `[package.metadata.dist] dist = false`, with the comment that `THIRD-PARTY-LICENSES` "covers the CLI's crates only" and that cargo-dist 0.33 has neither app bundles nor a file association. `dist-workspace.toml` has no bundle or signing setting. `tools/bundle-macos.sh` says what it leaves out: unsigned, no icon, one architecture, and the Apple Event unread. `NOTICE` names Inter, JetBrains Mono and Lucide. |
| The bench check in the app (worklog E3) | The coaster built in the app by the quick start, diffed against the phase 1 program, cut once | The "Test part" table of the functional spec has four empty rows. The diff half is what acceptance 90's test asserts (two comment lines). |
| The snapshot pair | `workspace_light.png` and `workspace_dark.png` were regenerated on macOS for the new layout | Commit `1fc9d5d6` (2026-11-18) says so: "rendered on macOS with the Linux-only gate lifted for the run … the pair needs rendering again by lavapipe before it can pass". 22 later commits touch `crates/dreamcarve-app/src`. The test returns at once on macOS and Windows, so the gate on macOS cannot see the pair is wrong. |
| Three operating systems | No run of `alpha` on Linux or Windows | See "Evidence". The rendering tests skip without an adapter, so a green run on CI as it is configured would still not show a rendered frame on Linux or Windows. |
| Timing-dependent window tests | `kittest::window_answers_input_while_generating` asserts wall-clock time: the 95th percentile of at least 60 frames within 100 ms, none over 250 ms, and *Stop* within one second. It failed at random on CI twice (#573, #593) and was reworked each time. | Present. It runs its own window and worker beside the 81 other tests of `kittest.rs`. |
| `Harness::run` and `max_steps` | **Found and fixed (E6).** The worklog (D8) records `kittest::drop_of_png_opens_the_picture_sheet` failing once with "exceeded max_steps" and passing on a rerun. Run 150 times beside a build it failed 3 times, each at `drop_file`'s `run()`: "exceeded max_steps (4). It would have settled after 6 steps". The picture is decoded on its own thread; when it landed between two frames of one `run`, the sheet it opens had too few of the four frames left. The tests now draw one frame at a time until the picture has landed and only then `run` (`common::settle_picture`, `drop_file`, and the two clicks that ask for a picture; `review.rs`'s three likewise). 800 runs under load and 900 without: no failure. `tests/book.rs` has the same pattern in `click_button` then `settle_picture`, and was not this work's to change. What the audit recorded: | `egui_kittest`'s `Harness::run` panics when the window still asks to repaint after `max_steps` frames, 4 by default. No test sets it, and `kittest.rs` calls `run()` 386 times. `common::settle_picture` calls it in a loop while a picture decodes on another thread. Whether that is the cause was not established. Other waits poll with 5 to 20 ms sleeps under deadlines of 10 to 300 s. |
| Wall-clock tests elsewhere (worklog A6) | Four in `dc-gcode` still assert time: `safety/tests/s17.rs::s17_check_is_fast_on_the_sign` and `safety/tests.rs::s9_check_is_fast_on_twenty_profile_parts`, `s9_check_is_fast_on_an_on_line_outline_of_many_chords`, `s9_check_is_fast_on_helix_pocket` | Present. They bear on the three-platform run. |
| A large picture on the UI thread (worklog D7) | Importing a 12 MP picture holds the UI thread about 214 ms in `dc-app` | Not measured here. Acceptance 87's 100 ms is asserted on generation, not on import. |
| The clipboard test | `kittest::warnings_copy_reaches_the_clipboard` reads the operating system's clipboard where there is one | It shares that clipboard with whatever else runs on the machine. |
| Documentation (worklog E5) | See the next section | |
| `mdbook build` | Not run: mdbook is not installed on the machine | `docs.rs::book_builds_and_every_screenshot_exists` runs it only "with `mdbook` installed". |
| The changelog | Done (E6). `[Unreleased]` had no entry under "Added" for the app | The first entry under "Added" now announces it: what it is, the five steps and what each does, preview, warnings and export, and that it is built from source and has been run on macOS only. |
| GitHub issues done on `alpha` | #535, #544 and #685 are open because the push is local-only | They close when `alpha` merges. |
| Help's link | *User Guide* opens `https://grayzu.github.io/dreamcarve/`, published from `main`; since E6, *Quick Start* opens `quick-start/app.html` and *Keyboard Shortcuts* `user-guide/keyboard.html` under it | Until `alpha` is merged and deployed, the app's Help shows a book without the app's chapters, and its two new rows open pages that are not published yet. |
| Worklog F4 | `Warning` and `CamError` are to be restructured, which changes what `dc-job` and `dc-app` read | todo. Better before the tag than after. |
| `v0.3.0` | Not tagged | `docs/specs/phase-3/audit.md`, "What blocks the tag". |

## Documentation deliverables

Every file is under `docs/book/src/`. Presence was checked by reading the
summary and searching each chapter for the app's terms.

| Deliverable | Where | Status |
| --- | --- | --- |
| Quick start: From SVG to G-code in the app: the coaster, then the plaque and the sign | `quick-start/app.md`, with 21 screenshots; the inlay pair as well | Present, held to the build by `gui_equality::quick_start_parts_from_the_app` and `docs.rs::book_builds_and_every_screenshot_exists` |
| The existing quick starts gain a note that each step has its equivalent in the app | `first-cut.md`, `plaque.md`, `sign.md`, `inlay.md` | **Four of seven.** `photo.md`, `line-art.md` and `tapered-ball-nose.md` have none. |
| New: The app | `user-guide/the-app.md` | Present |
| New: Importing drawings and pictures | `user-guide/importing.md` | Present. `importing-dxf.md` is kept beside it. |
| New: **Placing shapes** | | **Not written** |
| New: Stock, origin and heights | `user-guide/stock-and-origin.md` | Present |
| New: Toolpath preview | `user-guide/toolpath-preview.md` | Present |
| New: Checking and exporting | `user-guide/exporting.md` | Present |
| New: Keyboard shortcuts | `user-guide/keyboard.md` | Present, its table held to the code by a test |
| Updated: Overview, the app first | `user-guide/overview.md` | Present |
| Updated: every operation chapter with a screenshot of its editor, the field names and the Setup rows first | `profile.md`, `pocket.md`, `engrave.md`, `drill.md`, `bore.md`, `v-carve.md`, `inlay.md`, `photo-v-carve.md` | **Not written**: none mentions the app |
| Updated: Tabs (placing and dragging) | `user-guide/tabs.md` | **Not written** |
| Updated: Tool library | `user-guide/tool-library.md` | **In part**: the sheet, vendor groups, editing in place, library units and the import review, in words. No picture, no picker, no *New tool* on the Tools step, no Tip column, none of a tapered ball nose's fields in the app. |
| Updated: Post-processors (the Export step) | `user-guide/post-processors.md` | **Not written**; `exporting.md` covers the group |
| Updated: Configuration (app preferences and where they live) | `user-guide/configuration.md` | **Not written.** `the-app.md` sends the reader there for `gui.toml`, which the chapter does not name. |
| Updated: Warnings (the panel; "located at the field") | `user-guide/warnings.md` | **Not written** |
| Updated: Inlay (the male board, the per-board tick) | `user-guide/inlay.md` | **Not written** for the app; the one-file workflow is there |
| `CONTEXT.md`: Design Tree, Viewport, Toolpath Visibility | `CONTEXT.md` | Present |
| Contributing: Crate map (the GUI crate) | `contributing/crate-map.md` | Present, with the redesign's modules; it still says the app is "phase 4, in progress" |
| Contributing: How work flows (the GUI test harness, the screenshot rule) | `contributing/how-work-flows.md` | Present |
| Contributing: Porting from PureCutCNC (what of its React UI to port) | `contributing/porting.md` | **Not written**: the chapter does not mention the UI |

## Remaining steps

For the maintainer:

1. **Decide** the items under "Decisions waiting for the maintainer": the
   eight made while wiring, the six checklist items, the wording E6
   changed that decides something (acceptance 87, 90, 91 and 104), what
   is left of the Inlay's boundary, which test parts the exit means, and
   whether the tag waits for three operating systems.
2. **Sign off the visual review** (acceptance 104) in
   `review/final/README.md`, or say what it still needs: the six items.
   Close pull request #666 as superseded.
3. **The bench check**: build the coaster in the app by the quick start,
   export, cut, and fill the "Test part" table.
4. **The release slice** (#551): the bundles, the icon, the file
   association, the licence texts, `dist = true`.
5. **Tag `v0.3.0`, then `v0.4.0`**, per `docs/plan.md`, "Versioning".

For an agent, before the tag:

1. The chapters of "Documentation deliverables" marked not written or in
   part, Placing shapes first; then `mdbook build`. From E6, for the
   Inlay chapter: the Boundary row. The chapter as written on `alpha`
   beside this work says "The app has no row for a drawn boundary" and
   sends the reader to the project file; that is false once E6 is
   merged, and its picture of the Inlay's editor predates the row. And
   for the app's chapter: Help's *Quick Start* and *Keyboard Shortcuts*.
2. ~~A changelog entry for the app.~~ Done (E6).
3. ~~The tests for the 17 clauses with no assertion, the literal scan of
   acceptance 91 first.~~ Done (E6), but for the two that are not a
   test's to settle (92, 104).
4. ~~The stale wording of acceptance 64, 71, 76, 77, 82 and 104, and the
   rows of "Where the spec and the code disagree".~~ Done (E6): two rows
   by the code, twenty-two by the spec, one recorded.
5. Left by E6, each small: rename the seven tests that keep old names,
   with the design spec's test plan; a window test that picks the
   post-processor from the Controller dropdown (70 switches it by its
   command); `dc-app`'s unit `RPM` as a key; a mutation fixture with a
   drawn boundary for `digest_covers_every_generator_input`.
6. Once `alpha` is merged or pushed for CI: a software renderer on the
   Linux and Windows runners, the snapshot pair rendered again on Linux,
   and one green run on all three.
