# Phase 3: exit-criteria audit for v0.3.0

Slice 3.7-2 (GitHub #701; worklog B2). This audit checks the phase 3 exit
criteria of `docs/plan.md` and every acceptance test and safety rule of
`docs/specs/phase-3/functional.md` against the branch `alpha` at `70c1a92a`
("Record the dc-project cleanup in the worklog", 103 commits after `main` at
`9e1049e9`). It follows the form of phase 2's audit (#255).

How it was made: by reading. Nothing was built or run for it, because the
machine was short of disk: no `cargo test`, no sweep, no `dist`. Tests were
found from an index of the 3,258 `#[test]` functions in the sources, from
the names in the design spec's test plan, and by searching for each
acceptance test's numbers and codes; each mapping below was confirmed by
reading the test's body. "Covered" therefore means "a test asserts it", not
"the test passed today". Test paths are `crate/file::test_fn` under
`crates/`.

**Updated 2026-11-19 (worklog B5).** The gaps this audit listed were then
closed on a branch from `alpha`: an assertion for each of the 20 clauses
that had none, the two tests the test plan named, a fixture for each of
formats 21, 22 and 25, the spec lines that disagreed with the code, and
the two cut records. The tables below say what each clause has now. This
part was run, on macOS, at `5e23d2ec`: `cargo fmt --all -- --check`, `cargo
clippy --all-targets --all-features -- -D warnings`, `cargo nextest run
--workspace --no-fail-fast` (3293 tests passed, 18 skipped, which are the
`#[ignore]`d ones listed under "Evidence"), `cargo test --workspace --doc`
(the workspace has no doc tests) and `cargo run -p check-headers`, all
clean. The sweeps, `dist` and the other two operating systems were still
not run. The rest of the audit is as it was read at `70c1a92a`.

## Summary

- **Not ready to tag.** The code and the documents are there. What blocks
  `v0.3.0` is listed under "What blocks the tag" below: three bench checks,
  one run of the suite on all three operating systems, and the tag
  procedure, which has not started.
- **Exit criteria.** `docs/plan.md` gives seven for phase 3, now that it
  names the fifth test part. Three are met (parity with PureCutCNC's
  V-carve tests, the sign cut, the inlay pair cut and fitted). Four wait
  for the maintainer (the line-art inlay, the photo V-carve, the tapered
  ball nose carve and inlay, the tag). The functional spec adds six of its
  own, checked in the second table: two are met, one is met in the
  repository and not yet published, one is not shown (green on all three
  OSes), and two wait for the maintainer.
- **Acceptance tests.** There are 40: 48 to 61, 61a to 61s and 61t to 61z.
  All 40 have tests behind them. 33 are covered clause for clause (21
  before the gaps were closed). 7 are partial (19 before): every clause is
  asserted, and at least one is asserted on another fixture, at another
  layer, or more weakly than written. They are 51, 53, 54, 61h, 61n, 61x
  and 61y. None is without evidence, none waits for the maintainer, and no
  covering test is `#[ignore]`d.
- **Clauses with no assertion: 0** (20 before, in 14 rows). Each has a
  test now, listed under "Clauses that had no assertion".
- **Test-plan names with no test: 0** (2 before):
  `order::clearing_pockets_are_not_containing_pockets` and
  `order::inlay_is_exempt` are in `dc-cam/src/order.rs`.
- **Format fixtures.** Formats 21, 22 and 25 each have a committed file
  that holds what the format added (none before).
- **Safety rules.** Of the seven entries under "Safety requirements", four
  are covered (S17, S19, the drawn boundary, the tapered ball nose) and
  three are partial (S18, "`image-prep` is not a safety layer", "S2, S6,
  S7 unchanged").
- **Documentation.** Every capability has its chapter, and every promised
  chapter and quick start exists. The one code the spec named differently
  from the product (`v_carve_target_open`) is `vcarve_target_open` in the
  spec now, as in the generator and the Warnings chapter.

### What blocks the tag

1. **Bench checks 3.4, 3.5 and 3.6** (#606, #649, #684), all open with no
   comment. They are the maintainer's.
2. **The image-prep defaults** (#599, worklog B1): calibrated on synthetic
   fixtures only; the maintainer's run on their own art has not been done.
3. **Green on all three OSes is not shown for `alpha`.** No CI run exists
   for the branch, and the worklog's gate ran on macOS only. The last run
   green on Ubuntu, macOS and Windows is `main` at `c8a83193`
   (2026-11-17).
4. **The release dry run** of #701's Tests section (`dist plan`, `dist
   build --artifacts=local`) was not run for this audit.
5. **The tag procedure**: the workspace is at `0.2.1`, and the changelog's
   section is still `[Unreleased]`. The tag is the maintainer's.

Not blocking by themselves, for the maintainer to accept or turn into
slices: the 7 partial rows, the `Narrowest gap` inconsistency (worklog
B4), the unfinished part of A7 in `dc-geometry`, the sign's full cut,
whose result is recorded nowhere (#429), and what is left under "Where
the spec and the code disagree".

## Evidence

- **Suite.** Not run for the audit as first written. The worklog records
  the gate (`cargo fmt`, `clippy -D warnings`, `cargo nextest run
  --workspace`, the doc tests, `check-headers`) as run on macOS before
  each slice was marked done. Run on macOS on 2026-11-19 with the gaps
  closed, at `5e23d2ec`: 3293 tests passed and 18 were
  skipped (the ignored ones below), the doc-test command was clean
  (the workspace has no doc tests), and so were the other three.
- **CI.** `alpha` has no run. `main`:
  - `c8a83193`, run 44051390962 (2026-11-17): every job passed, among them
    check on ubuntu-latest, macos-latest and windows-latest,
    gcode-conformance, cargo-deny, docs, third-party licenses.
  - `9e1049e9`, run 44385310413 (2026-11-18), the commit `alpha` was cut
    from: marked failed. Ubuntu and Windows passed; the macOS check was
    cancelled. Between the two commits only `docs/code-review-plan.md` and
    `tools/install-dev-tools.sh` were added.
- **`#[ignore]` in the sources** (`grep '#\[.*ignore' crates tools`). No
  acceptance or safety test is ignored. The sweeps that the generators'
  robustness leans on are, and no CI job runs them:

  | Test | Reason given | Run in CI? |
  | --- | --- | --- |
  | `dc-gcode/tests/stress/main.rs::sweep` | "the large sweep (#464): run with --release -- --ignored" | No |
  | `dc-gcode/tests/stress/main.rs::boundary_sweep` | "the boundary sweep (#604): run with --release -- --ignored" | No |
  | `dc-gcode/tests/stress/tapered.rs::tapered_sweep` | "the tapered ball nose sweep (#681): run with --release -- --ignored" | No |
  | `stress/main.rs::v_carve_of_line_art_reproduces_when_written`, `lettering_male_cuts_a_neck_narrower_than_its_step`, `males_reach_a_corner_their_axis_meets_along_a_wall`, `board_between_two_bands_is_not_plug`, `males_of_line_art_0x63_with_a_clearing_pass_seat`; `stress/tapered.rs::cut_out_of_lettering_0x5b_with_the_ultra_bit_is_cut`, `lettering_0x60_seats_at_a_rounded_point` | "slow: the sweep's full-size design(s); run with --release -- --ignored" | No |
  | `dc-import/src/prep/defaults.rs::print_the_sweep` | "prints the sweep for tests/fixtures/prep/README.md" | No (a printer, not a check) |
  | `dc-geometry/src/region.rs::nest_2000_shapes_under_500ms` | `cfg_attr(debug_assertions, ignore = "timing holds in release builds only (#71)")` | No |
  | `dreamcarve-app/tests/review.rs` (four tests; three when this was first written) | review pictures, run by hand (phase 4) | No |
  | `tools/third-party-licenses/tests/ci.rs` (two tests) | need cargo-about and dist | Yes, with `--ignored` |

  The worklog (A4, A7) records all three sweeps as passing with 0 failures
  on macOS this week, after the medial-axis fix, with identical totals
  after the generator changes.
- **Bench issues**, read with `gh issue view` on 2026-11-18:

  | Issue | State | Note |
  | --- | --- | --- |
  | #412 3.1 bench check | Closed 2026-10-29 | The sign without countersinks, cut on a LongMill MK3; recorded in the spec. |
  | #429 3.2 bench check | Closed 2026-10-30, completed | One comment, the instructions for the cut (2026-10-30). No result on the issue; the spec's "Sign" cut record now says so. |
  | #438 3.3 bench check | Closed 2026-11-10 | The maintainer cut inlays of two designs of their own ("a basic pacman image and a more complex shark image") and signed off: "Both came out acceptable so signing off as completed." No machine, bit, board or measurement is stated. Copied into the spec's cut record on 2026-11-19. |
  | #599 3.4 calibrate the defaults | Open | See "Open items", B1. |
  | #606 3.4 bench check | Open, no comment | Waits for the maintainer. |
  | #649 3.5 bench check | Open, no comment | Waits for the maintainer. |
  | #684 3.6 bench check | Open, no comment | Waits for the maintainer. |
  | #701 this audit and the tag | Open | Closes with the tag. |

## Exit criteria

### `docs/plan.md`, phase 3

| Criterion | Status | Evidence |
| --- | --- | --- |
| Parity with PureCutCNC's V-carve tests | **Met** | The upstream tests are ported with their headers: `dc-geometry/src/medial/tests.rs` (`rectangle_spine_has_exact_clearances_and_corner_tips`, `square_diagonals_meet_at_centre`, `circle_collapses_to_centre`, `ring_with_island_is_a_cycle_without_island_tips`, and the rest of the design spec's "(port)" list) and `dc-cam/src/vcarve/tests.rs`. Of about 730 backticked names in the design spec's test plan, 683 are tests that exist as written; the rest are field or code names, renames the plan itself records, or the names listed under "Spec-named tests that do not exist". Eight golden cases captured from PureCutCNC are in `tests/golden/` and compared in `dc-gcode/tests/golden.rs`: `vcarve_medial_rect`, `_square`, `_circle`, `_ring`, `_l`, `_text_rag`, `_triangle` and `vcarve_offset_square`, at 0.02 mm plus 0.0017 mm of rounding, with the differences each case declares. |
| Test part cut: a lettered sign imported as SVG outlines | **Met; the full cut's result is not recorded** | #412 (closed): cut without the countersinks on 2026-10-29, recorded in the functional spec's "Sign" cut record. #429 (closed as completed 2026-10-30): the full cut with the countersinks. The issue holds no result, so the spec's record says that the issue was closed with none and that the result is still to be recorded. The maintainer has it. |
| Test part cut: an inlay pair, female and male, cut and fitted | **Met** | #438 (closed 2026-11-10): the maintainer cut and fitted inlays of two of their own designs and signed off, as the spec's "a bench check may substitute its own design" allows. The spec's "Lettered inlay pair" cut record holds what the issue says: the date, the two designs, the verdict in the maintainer's words, and that no machine, bit, board or measurement was stated. |
| Test part cut: a line-art inlay prepared with `image-prep` inside a drawn boundary | **Waiting on the maintainer** | #606, open. The stand-in is tested: acceptance 61k and 61s. The defaults it would be prepared with are also unconfirmed (#599). |
| Test part cut: a photo V-carve whose toolpath matches RasterCarve's test cases within the tolerance the spec states | **Waiting on the maintainer** for the cut | #649, open. The match is tested: `dc-gcode/tests/golden.rs::photo_raster_inch` and `photo_raster_metric`, at 0.01 mm in XY and 0.05 mm in Z, the spec's tolerance, with the two departures the cases' READMEs declare (acceptance 60). |
| `v0.3.0` | **Waiting on the maintainer** | Not tagged (`git tag`: `v0.0.1`, `v0.1.0`, `v0.2.0`). The workspace version is `0.2.1`. |
| Test part cut: a carve and an inlay pair with a tapered ball nose, after a coupon fit | **Waiting on the maintainer** | #684, open. The projects are tested: `dreamcarve/tests/cli/tapered.rs::tapered_carve_exports`, `tapered_inlay_exports_both_boards`, `tapered_coupon_exports`. |
| Real cut: yes, all five parts | **Two of five** | The sign and the inlay pair, as above. The line-art inlay, the photo and the tapered ball nose part wait for #606, #649 and #684. |

The plan's exit named four parts when this audit was first written; it
names five now, as its sub-phase list and the functional spec do.

### The functional spec's own list

| Criterion | Status | Evidence |
| --- | --- | --- |
| All acceptance tests and safety tests green on all three OSes | **Not met: not shown for `alpha`** | Every acceptance test and safety rule has tests (below). Whether they pass on `alpha` on Linux and Windows is not known: the branch has no CI run and the worklog says "Windows and Linux are not validated in this push". The last all-green run is `main` at `c8a83193`. This audit ran nothing. |
| Golden parity cases captured and passing: the PureCutCNC V-carve cases and the RasterCarve photo cases | **Met** (captured); passing as of the last green CI | Eight V-Carve cases and two photo cases (`photo-raster-inch`, `photo-raster-metric`) in `tests/golden/`, each with its README, compared in `dc-gcode/tests/golden.rs`. The hatch case of the draft was dropped (security review M5), as acceptance 60 says. |
| The five test parts cut on grblHAL and recorded above, the inlay pairs fitted | **Waiting on the maintainer** | Two cut (the sign, an inlay pair); three not (#606, #649, #684). Of the five cut records in the spec, two are filled in: the sign's for the partial cut only, and the lettered pair's with what #438 says. |
| Quick start and every chapter above published | **Met in the repository; not published from `alpha`** | Every chapter and quick start exists (see "Documentation deliverables"). The docs job deploys from `main`; `alpha`'s chapters are not deployed. |
| `CONTEXT.md` updated | **Met** | Flat Depth, Start Depth, Clearing, Inlay, Glue Gap, Plug Depth, Photo V-Carve, Bitmap, Countersink, Image Prep, Ink, Piece, Speck, Boundary and Tapered Ball Nose are all defined. `dreamcarve/tests/quick_start.rs::context_terms_are_defined` checks it. |
| `v0.3.0` tagged with binaries | **Waiting on the maintainer** | As above. |

## Acceptance tests 48 to 61

Path shorthand: `vcarve/` is `dc-cam/src/vcarve/`, `inlay/` is
`dc-cam/src/inlay/`, `cli.rs` is `dreamcarve/tests/cli.rs`, `golden.rs` is
`dc-gcode/tests/golden.rs`.

| # | Asserts | Covering tests | Status and notes |
| --- | --- | --- | --- |
| 48 Medial depth | A 20 × 5 rectangle 4.330 deep on its centreline and up to the corners; a 6 mm square meets 3 mm deep; a 6 mm circle is one plunge 5.196 deep | `vcarve/tests.rs::rectangle_centreline_depth_is_half_width_over_tan`, `six_mm_square_with_90_degree_bit_meets_at_three_mm`, `six_mm_circle_is_one_plunge_at_5_196`; `dc-geometry/src/medial/tests.rs::square_diagonals_meet_at_centre`; `cli.rs::v_carve_exports_and_passes_safety` | Covered. The circle's depth is held to 0.02 mm (the spec gives it no tolerance). |
| 49 Nesting and corners | A ring carved along its centreline; "Rag" reaches every corner and matches the golden; a triangle meets at its incentre | `vcarve/tests.rs::ring_runs_along_its_centreline_and_rises_at_outer_corners`, `no_cut_is_nearer_a_wall_than_its_depth_allows`, `triangle_meets_at_the_incentre`; `golden.rs::vcarve_medial_text_rag` | Covered, with the declared differences of the golden (`DescentsHeldToPlunge`, `ChainDepthFitted`, `EveryCornerReached`, `ChainsOnTheAxis`). "No cut crosses an outline" is asserted through each cut's midpoint lying in a region and the cone test. The triangle is upstream's scaled by 0.4. |
| 50 Flat depth and V-bit floor | Nothing below Z −2; the 26 × 6 floor boundary traced; the floor covered within 0.05 | `vcarve/tests/floor.rs::floor_boundary_is_traced_at_the_flat_depth`, `v_bit_floor_covers_the_floor_region`, `floor_ridge_height_is_reported` | Covered. |
| 51 Clearing passes | Two runs; the end mill inside the floor; the V-bit's floor cutting under a third; a second pass; the four codes | `vcarve/tests/clearing_passes.rs::clearing_pass_takes_the_floor_region_and_depth`, `clearing_runs_come_before_the_v_bit_run_with_their_own_tools`, `floor_cutting_shrinks_to_what_the_end_mill_cannot_reach`, `floor_coverage_with_the_pass_still_reaches_the_floor`, `second_pass_clears_only_what_the_first_left`, `wider_second_pass_warns_not_descending_and_still_runs`, `clearing_without_a_flat_depth_is_an_error`, `clearing_pass_with_a_v_bit_is_an_error`, `removing_the_list_gives_the_v_bit_the_whole_floor`; `dc-gcode/src/emit/tests.rs::clearing_runs_become_programs_of_their_own_tools`; `dc-cam/tests/acceptance/clearing.rs::boundary_trace_alone_is_over_a_third_of_the_floor_cutting_at_0_4` | **Partial:** "export writes two runs" is asserted at the emit layer on the sample rectangle; on the 30 × 10 one `cli.rs::preview_legend_names_clearing_runs` asserts the two runs in the preview's legend and that `export` succeeds. The aside on 0.4 is asserted now, and the spec's figure corrected: the 64 mm trace is 36 % of the unassisted floor cutting, not 40 %. |
| 52 Cone and tool checks | `v_bit_cone_exceeded` naming 10 mm; an end mill refused; an open target skipped | `vcarve/tests.rs::cone_exceeded_is_an_error_naming_the_depth`, `flat_depth_within_the_cone_exports`, `end_mill_is_refused`, `open_path_is_skipped_with_warning`; `dc-project/src/validate/tests.rs::v_carve_with_end_mill_is_error` | Covered. The open-target code is `vcarve_target_open`, as the spec now says. |
| 53 Start depth | Every Z 4 mm lower; the mismatch and uncleared checks; one pass | `vcarve/tests.rs::start_depth_lowers_every_cut_and_plunges_from_the_stock_top`, `uncleared_start_depth_is_refused_and_acknowledged_counts_toward_the_cone`, `one_pass_whatever_the_depth`, `corner_tip_chain_plunges_to_the_surface_not_the_centreline`; `dc-cam/src/order.rs::start_depth_matching_an_earlier_pocket_is_quiet`, `start_depth_with_a_shallower_pocket_names_it`, `disabled_pocket_does_not_count`; `cli.rs::check_warns_start_depth_mismatch`, `check_refuses_an_uncleared_start_depth` | **Partial:** "plunges once per chain and never re-traces a line at a shallower Z" is asserted indirectly (the same toolpath with and without a step-down, fewer than nine plunges). "The rectangle outside it" is asserted only on `order.rs`'s 2 mm sample. The matching case is checked, never exported. The 90° refusal is asserted with the Pocket removed, not disabled. |
| 54 Offset strategy | Rings inset 0.5 to 2.5 at their own depth; both directions; the offset golden | `vcarve/tests/offset.rs::offset_rings_are_inset_by_their_depth`, `offset_climb_and_conventional_reverse_every_ring`; `golden.rs::vcarve_offset_square` | **Partial:** the rings are asserted on the 6 mm square. The reversal is asserted on a 16 mm square with a hole and a 60° bit. The golden is the only offset case, a 20 mm square, 60°, step-over 1 mm, clamped at 4 mm, which the spec now names. |
| 55 Countersink | Four plunges to Z −3; 82° to −3.451; three codes; a `G4`; the Pocket case to Z −7 | `dc-cam/src/drill.rs::acceptance_55_countersinks_four_holes`, `countersink_depth_is_mouth_over_two_tan_half_angle`, `countersink_needs_a_v_bit`, `countersink_dwell_is_at_the_bottom`, `countersink_depth_is_measured_from_the_start_plane`, `countersink_rapid_stops_above_the_stock_top_whatever_the_start_depth`, `uncleared_countersink_start_depth_is_refused`; `dc-cam/src/order.rs::countersink_start_depth_is_checked_too`; `cli.rs::countersink_exports_as_plain_moves_with_canned_cycles`; `dreamcarve/tests/cli/acceptance_3.rs::countersink_from_the_stock_top_in_a_pocket_warns_and_cuts_above_its_floor`; `dc-gcode/src/safety/tests/s18.rs::generated_countersinks_pass_s18` | Covered. The `G4 P0.500` after each plunge is asserted in the LinuxCNC program. The Pocket case is asserted through the CLI with four holes: quiet and Z −7 at `start_depth_mm: 4`; at 0 the warning naming the Pocket and plunges ending at Z −3, 1 mm above the floor. The spec said "the mouth would open 3 mm above the floor"; it is at the stock top, 4 mm above, and the spec is corrected. The refusal with the Pocket disabled is asserted at the generator and in validation. |
| 56 Inlay female | The female equals a V-Carve with `flat_depth_mm: 4`; unset male board | `inlay/tests.rs::female_equals_a_flat_depth_v_carve`; `inlay/tests/boards.rs::male_runs_wait_for_the_board_thickness`; `cli.rs::inlay_exports_and_passes_safety` | Covered. |
| 57 Inlay male | The surround, the mate within 0.02, the cut-out, the open edge, rounding, clipping to the board | `inlay/tests/boards.rs::both_boards_from_one_operation`, `plug_base_thin_reads_the_male_board`; `inlay/tests.rs::male_carves_the_surround_and_the_holes`, `male_floor_is_at_depth_minus_gap_plus_plug`, `male_flank_passes_through_the_outline_at_the_start_plane`, `female_and_male_walls_mate`, `clearing_pass_clears_the_male_floor`, `cut_out_hugs_the_slope_foot_with_a_v_wall`, `cut_out_hugs_the_band_edge`, `male_with_clearing_has_no_v_wall_at_the_band_edge`; `inlay/tests/board.rs::band_is_clipped_to_the_stock`, `cut_out_runs_edge_to_edge_when_clipped`, `clipped_male_still_seats`, `design_too_near_the_edge_is_explained` | Covered: every sentence has an assertion. Weaker than written: the plug-base warning runs on a rectangle; "shrinks as in 51" is asserted as under half, not under a third; the end mill's outermost path equal to the edge less its radius is asserted on a band rounded by 5 mm. |
| 58 Bitmap and placement | `init` on a picture; `image add` and `image list`; a moved `.dcv`; S7 past the edge | `dreamcarve/tests/cli/picture.rs::init_on_a_picture_writes_a_bitmap_and_a_photo_v_carve`, `image_add_places_a_picture_and_image_list_shows_it`, `image_add_converts_colour_and_says_so`, `moved_dcv_exports_the_same_program`; `cli.rs::picture_past_the_stock_edge_fails_s7` | Covered. |
| 59 Photo grooves | Angle, spacing 1.179, depths by tone, invert, hatch, the fed lift, the overlap warning | `dc-cam/src/photo/tests.rs::every_cut_is_inside_the_footprint`, `grooves_run_at_the_line_angle_and_alternate`, `grooves_are_spaced_by_the_fraction_of_the_full_depth_width`, `one_groove_passes_through_the_top_left_corner`, `black_is_max_depth_white_is_the_surface_and_grey_is_between`, `invert_swaps_black_and_white`, `zero_angle_grooves_are_parallel_to_x`, `hatch_adds_a_second_family_at_ninety_degrees`, `between_grooves_the_bit_rises_to_the_retract_and_feeds_across`, `spacing_below_one_warns` | Covered, with the spec's numbers and tolerances. |
| 60 Photo parity | Groove for groove against RasterCarve, 0.01 mm XY, 0.05 mm Z | `golden.rs::photo_raster_inch`, `photo_raster_metric` (`golden/photo.rs::check`) | Covered at the spec's tolerances. Declared departures: `Resampled` on the inch case, `LastLineUncut` on the metric case (DreamCarve cuts 111 grooves to RasterCarve's 110). The comparison reads the generated toolpath, not the emitted program. |
| 61 Sign end to end | Four programs in tool-run order; what the preview draws; S1 to S19 | `cli.rs::sign_exports_in_tool_run_order`, `sign_countersinks_sit_on_the_end_mill_cleared_floor`, `sign_lettering_is_within_the_cone`; `dreamcarve/tests/cli/acceptance_3.rs::sign_preview_draws_the_rings_the_serifs_the_countersinks_and_the_tabs`; `dreamcarve/tests/quick_start.rs::v_carved_sign_chapter_runs` | Covered. The preview is read: the clearing run's innermost ring 5.675 mm from the oval all the way round, a drawn line ending within 0.02 mm of each of the lettering's 57 outward corners, the two countersinks as the V-bit's circles, the cut-out's four sides with a tab mark on each. The safety clause is asserted as `export` succeeding, which runs the whole pass. |

## Acceptance tests 61a to 61s (sub-phase 3.4)

Path shorthand: `prep/` is `dc-import/src/prep/`, `boundary.rs` is
`dc-cam/src/inlay/tests/boundary.rs`, `validate` is
`dc-project/src/validate/tests.rs`.

| # | Asserts | Covering tests | Status and notes |
| --- | --- | --- | --- |
| 61a Import checks | One warning of each kind on `messy.svg`; the project unchanged; `check` repeats two | `cli.rs::init_prints_the_import_checks`, `init_project_is_unchanged_by_the_checks`, `check_repeats_overlap_and_size_findings`, `a_clean_drawing_prints_no_checks`; `prep/findings.rs::overlap_names_the_pair_and_where`, `feature_below_size_counts_and_names_the_smallest` | Covered. "Byte for byte what the build wrote before the checks" is equality with the committed `tests/fixtures/prep/messy.dcv`, a version 28 file, so it is a regression golden of today's build. |
| 61b Visible geometry only | Hidden, text, image and clip path reported; the hidden square not in the output | `prep/run.rs::messy_svg_reports_what_it_skipped_and_leaves_it_out`; `prep/walk.rs::text_images_clip_paths_and_masks_are_counted_with_their_fix`; `cli.rs::image_prep_prints_warnings_then_the_report` | Covered. The spec said the output holds none of the clip path's content; a clip path is ignored and its group imported whole (400 of the 920 mm²), as the warning and the chapter say, and the spec now says so. |
| 61c Strokes become areas | 43.14, 40.00, 44.00, 80.00 and 79.79 mm²; dashes; a non-uniform scale | `prep/stroke.rs::round_caps_add_pi`, `butt_caps`, `square_caps`, `miter_join_at_a_right_angle`, `round_join_at_a_right_angle`, `dashes_give_one_piece_each`, `a_non_uniform_scale_widens_unevenly` | Covered, at the stroke-outline layer. |
| 61d Fills are resolved | 150 and 300 mm²; nonzero against even-odd; order by position | `prep/run.rs::overlapping_squares_are_one_piece`, `an_island_path_becomes_a_hole`, `fill_rule_is_per_path`, `output_is_independent_of_document_order`; `prep/write.rs::piece_order_and_ids_follow_position_not_document_order` | Covered. Three squares in six document orders: the pieces and `piece-1` to `piece-3` come lowest then leftmost first. |
| 61e Regions mode | One piece of 584.3 mm² with four holes; four pieces of 2243 mm² in all; open ink | `prep/run.rs::shapes_mode_of_the_ring_and_cross_is_one_piece_with_four_holes`, `regions_mode_of_the_ring_and_cross`, `regions_mode_output_never_overlaps_ink`, `regions_mode_with_a_floating_cross_warns_open_ink`; `prep/report.rs::ridge_is_the_narrowest_gap_in_regions_mode` | Covered. |
| 61f Clean-up | Specks, holes, hairlines, the staircase, the range and the warning | `prep/run.rs::specks_and_hairlines_are_dropped`, `small_holes_are_filled`, `zero_min_detail_keeps_everything`, `smooth_removes_a_staircase_and_keeps_corners`, `smooth_zero_leaves_the_staircase`, `threshold_out_of_range_is_an_error`, `smooth_above_half_the_detail_warns`; `cli.rs::image_prep_threshold_range_exits_2` | Covered. |
| 61g The output file | Millimetres, filled paths only, no flip, never overwrites, a picture refused | `prep/write.rs::declares_millimetres_and_the_input_page`, `paths_are_filled_and_never_stroked`, `a_lone_rectangle_round_trips`, `y_is_flipped_back_so_an_f_is_an_f`; `cli.rs::image_prep_refuses_to_overwrite`, `image_prep_refuses_a_picture_and_names_inkscape`, `image_prep_malformed_svg_writes_nothing`, `image_prep_then_init_has_no_warnings` | Covered. |
| 61h The report | Height 60.00; gap 2.00 mm and its figures; thinnest piece; two warnings; counts | `prep/report.rs::design_height_and_counts`, `widths_scale_to_100_200_300`, `reaching_height_for_the_guide_width`, `thinnest_piece_is_the_least_median`; `dc-job/src/prep_report.rs::the_ring_and_cross_report_names_its_figures`; `cli.rs::image_prep_prints_the_report_of_the_ring_and_cross`; `prep/run.rs::regions_suggested_for_thin_ink_and_not_for_thick`, `background_shape_is_flagged` | **Partial:** the background test uses a rectangle over 100 % of the box, so the 98 % threshold is not pinned; the default-mode thinnest piece is asserted in the library only. The gap is pinned only at the default `--smooth` on clean strokes: no test computes it at two `--smooth` values, so none would catch the inconsistency of worklog B4. |
| 61i Determinism | Twice, reverse order, CRLF: the same bytes | `prep/write.rs::same_input_twice_reverse_order_and_crlf_give_identical_text`; `prep/run.rs::same_input_twice_is_identical`, `output_is_independent_of_document_order`, `crlf_input_gives_the_same_bytes` | Covered. |
| 61j No machining inputs | No machining option in `--help`; the output independent of the project | `cli.rs::image_prep_help_has_no_machining_options`; `dreamcarve/tests/cli/acceptance_3.rs::image_prep_output_is_the_same_whatever_project_is_beside_it` | Covered. The command takes no project; the same bytes come out beside a project of the drawing's own name and under a user config that changes `init`, and that project is untouched. |
| 61k Line art to inlay | Eight pieces, a 2.5 mm gap, one expected warning, both boards safe, the seat | `cli.rs::line_art_to_inlay_end_to_end`; `dreamcarve/tests/quick_start.rs::line_art_chapter_runs` | Covered. "The same pockets within 0.05 mm" compares bounding boxes, not outlines. |
| 61l Boundary replaces the margin | `inlay_margin_ignored`; the male's cuts inside the square; the end mill at the square less 1.5875 | `boundary.rs::boundary_replaces_margin_and_rounding`, `boundary_male_cuts_stay_inside_the_band`, `boundary_edge_pass_sweeps_to_the_boundary`, `boundary_v_wall_without_clearing`, `boundary_female_is_unchanged`; `validate::margin_with_a_boundary_warns_ignored`; `dc-cam/tests/acceptance/boundary.rs::boundary_edge_walls_are_vertical`, `boundary_corners_are_cut_with_the_end_mills_radius`, `boundary_open_edge_keeps_the_v_bit_its_cone_radius_inside` | Covered. The end mill's outermost path is 38.4125 mm out at every level it cuts; its centre runs into each inset corner, leaving `r(√2 − 1)` in the square's corner; no V-bit cut is within 2.887 mm of the square. |
| 61m Mirrored with the design | The frame mirrored about X = 55; the rooms swap sides | `boundary.rs::boundary_is_mirrored_with_the_design`; `cli.rs::preview_draws_the_boundary_on_the_male_board`; `dc-cam/tests/acceptance/boundary.rs::boundary_rooms_swap_sides_on_the_male`; `dreamcarve/tests/cli/acceptance_3.rs::moved_boundary_check_names_the_mirror_at_x_55` | Covered. The rooms are 5.3 and 11.3 mm as drawn (within 0.1) and the other way round on the male (within 0.02 of the drawn ones); `check` prints `inlay_male_mirrored` once, at X = 55 mm. |
| 61n Boundary checks | Six codes, each at the Inlay's `boundary`; no boundary is byte for byte as before | `validate::boundary_crossing_the_design_is_error_with_the_point`, `boundary_touching_the_design_is_error`, `boundary_open_profile_is_error`, `boundary_that_is_a_target_is_error`, `boundary_unknown_instance_is_error`, `boundary_with_no_design_inside_warns`, `boundary_hole_warns`; `dc-geometry/src/carve.rs::drawn_band_drops_holes`; `boundary.rs::no_boundary_is_byte_for_byte_unchanged`; `dc-cam/tests/acceptance/boundary.rs::boundary_smaller_than_the_star_crosses_the_design`, `band_is_cut_through_an_ignored_boundary_hole`; `dreamcarve/tests/cli/acceptance_3.rs::empty_boundary_warns_and_still_exports` | **Partial:** every clause is asserted. On the star: the smaller boundary (a tip 10 mm past it) and the ignored hole (the male cut move for move as without it). On the "DC" pair: `inlay_boundary_empty` warns in `check` and `export`, which writes the five programs. The open, target and unknown-id codes are asserted on a 10 mm square design, not the star. |
| 61o Several boundaries | Two squares make one band; one square alone is an error; one oval round both | `boundary.rs::several_boundaries_make_one_band`; `validate::several_boundaries_are_a_union`; `dc-cam/tests/acceptance/boundary.rs::two_boundaries_band_is_the_two_squares_less_the_stars` | Covered. Every V-bit cut's cone at the start plane is inside its square and outside both stars but for their circles, and both squares are cut. |
| 61p Cut-out and the board's edge | The cut-out 1.5875 inside the square; `inlay_cut_out_no_room` naming 5.484; clipped to the board | `boundary.rs::boundary_cut_out_hugs_the_edge`, `boundary_cut_out_no_room_names_5_484`, `boundary_clipped_to_the_board_warns_and_stays_on_it`; `validate::cut_out_room_reads_the_narrowest_band`; `dc-cam/tests/acceptance/boundary.rs::boundary_cut_out_frees_the_square_inset_by_the_end_mills_diameter` | Covered. Every point of the square inset by 3.175 mm is the end mill's radius from the cut-out's path, within 0.01 mm. |
| 61q Format 26 | A format 25 project migrates with `boundary: []` and exports the same; a boundary round-trips; Photo is 27 | `dc-project/src/migrate/tests.rs::every_older_sample_fixture_loads_as_the_sample_project` (row 25), `v25_to_v26_adds_an_empty_boundary`, `a_v25_project_exports_the_same_programs`, `version_26_fixtures_load`; `dc-project/src/serialize/tests.rs::inlay_boundary_round_trips`, `photo_v_carve_is_format_27`; `cli.rs::v25_project_exports_the_same_programs` | Covered. |
| 61r Safety re-derives | The old programs fail after the boundary moves; no generator is called | `dc-gcode/src/safety/tests/boundary.rs::moved_boundary_fails_the_old_program`, `moved_boundary_fails_one_males_programs_by_s17_and_s9`, `boundary_band_is_derived_from_the_instance`, `cut_out_off_the_boundary_ring_fails`, `boundary_that_no_longer_resolves_refuses_the_old_program`; `dc-gcode/src/safety/tests/s17.rs::s17_uses_no_generator` | Covered. A program is one tool's run and is held by that tool's rule, so no one program fails both; one male does, with a V wall and a cut-out: its V-bit program fails S17 and its cut-out program S9. The spec now says so. The fixture is two rectangles with a 90° bit. |
| 61s Bench stand-in | The compass prepared, inlaid in an oval, safe, seated, 3 mm to spare | `cli.rs::line_art_to_inlay_end_to_end`, `compass_inlay_exports_both_boards` | Covered by the first. The second starts from the committed `compass-regions.svg`. Cut end points are held 3 mm inside; arc interiors are not sampled. |

## Acceptance tests 61t to 61z (sub-phase 3.6)

| # | Asserts | Covering tests | Status and notes |
| --- | --- | --- | --- |
| 61t The tool and format 28 | The saved form; the derived values; the range checks; `tool_value_unset`; library version 3 | `dc-project/src/serialize/tests.rs::tapered_ball_nose_round_trips`, `tapered_ball_nose_is_format_28`; `dc-project/src/migrate/tests.rs::v27_file_with_a_tapered_ball_nose_fails`, `version_27_fixture_loads`; `dc-geometry/src/profile/tests.rs::bench_bits_derived_values`; `dc-project/src/tool.rs::cut_radius_is_capped_by_the_cutting_diameter`, `cutting_height_is_the_lower_of_flutes_and_cap`; `validate/tests.rs::tapered_ball_nose_taper_out_of_range_is_error`, `tapered_ball_nose_flute_must_pass_the_ball`, `cut_diameter_must_exceed_the_tip`, `unset_tapered_value_on_a_used_tool_is_tool_value_unset`; `dc-library/src/library.rs::version_2_library_loads_as_version_3`; `dreamcarve/tests/cli/tapered.rs::v27_project_exports_the_same_programs`, `check_lists_unset_tapered_values` | Covered. The unset value is located by the tool's id (`tools[t1]…`), as the spec now says. |
| 61u Depth by the profile | Z −3.500 and −0.146; 18.40 exports; 22.65 refused; S17 exact | `vcarve/tests/tapered.rs::tapered_bar_centreline_is_the_inverse_width`, `tapered_hairline_is_cut_by_the_ball`, `tapered_corners_are_reached_at_the_surface`, `tapered_flute_length_bounds_the_engagement`, `tapered_cut_diameter_bounds_the_engagement`; `dc-gcode/src/safety/tests/tapered.rs::generated_tapered_carves_pass_s17` | Covered. |
| 61v Floor, clearing and offset | The 27.497 × 7.497 floor; the fillet; a 0.116 ridge that `check` prints; offset rings | `vcarve/tests/tapered.rs::tapered_floor_inset_is_the_width_at_the_flat_depth`, `tapered_floor_has_the_ball_fillet`, `tapered_floor_ridge_is_the_ball_scallop`, `tapered_floor_covers_the_floor_region`, `tapered_clearing_pass_takes_the_floor_inset`, `tapered_offset_rings_follow_the_profile`; `dreamcarve/tests/cli/tapered.rs::check_prints_the_tapered_floor_ridge` | Covered. `check`'s line for the Fine bit's carve states the ridge the scallop formula gives, to its three places and within 0.005 of 0.116 mm. The offset square's `flat_depth_mm` 10 is in the spec now. |
| 61w Corners | `tapered_corners_softened` once, 4 corners, 0.378; the 0.156 and 0.469 setback; the outline at Z 0 | `vcarve/tests/tapered.rs::tapered_corners_softened_warns_once`, `tapered_corner_setback_matches_the_formula`; `dc-cam/tests/acceptance/tapered.rs::tapered_carve_cuts_the_drawn_outline_at_the_surface` | Covered. With both bits, what the program cuts at the stock top reaches every point of the square's outline, its corners too, within 0.01 mm and nowhere passes it. The setback is measured on a square's corner on the exact surface, as the spec now says. |
| 61x Inlay fit | Female equals a V-Carve; walls mate within 0.02; the seat within 0.05; the seat line; the two ball warnings | `inlay/tests/tapered.rs::tapered_female_equals_a_flat_depth_v_carve`, `tapered_walls_mate`, `tapered_plug_seats_flush`, `glue_gap_below_the_ball_warns`, `plug_depth_below_the_ball_warns`, `tapered_male_engagement_is_held_to_the_flute_length`; `dc-project/src/inlay/tapered/tests.rs::seat_line_reports_the_sensitivity`; `dc-gcode/src/safety/tests/tapered_inlay.rs::generated_tapered_inlays_pass_s17_and_s9`; `dc-gcode/tests/tapered_inlay.rs::rounded_star_seats_at_a_glue_gap_of_1_5`; `dreamcarve/tests/cli/tapered.rs::check_prints_the_seat_line` | **Partial:** on the spec's fixture (the star rounded 2 mm, `glue_gap_mm` 1.5, both bits, with and without a clearing pass and a cut-out) the pair seats within 0.05 mm, nothing is rounded for the bit, and every program passes the safety pass. The female-equals and walls-mate clauses are still asserted on the bit-rounded star at `glue_gap_mm` 1.0. The seat line's 0.40 and 0.76 are asserted on the function; `check` printing it is asserted for `depth_mm` 6 (0.55 and 0.76). |
| 61y Rounding for the bit | Tips to 0.731 on both boards; the warning; the two refusals; a V-bit inlay unchanged | `inlay/tests/tapered.rs::sharp_point_is_rounded_on_both_boards`, `narrow_notch_is_closed_on_both_boards`, `part_too_narrow_for_the_bit_is_an_error`, `gap_too_narrow_for_the_bit_is_an_error`, `v_bit_inlay_is_not_rounded`; `dc-project/src/inlay/tapered/tests.rs::rounded_design_is_open_and_closed`, `topology_changes_are_refused`; `dc-gcode/src/safety/tests/tapered_inlay.rs::tapered_inlay_regions_are_the_rounded_design` | **Partial:** "the same Inlay with the 60° V-bit" is asserted on the "DC" fixture, not the star. Everything else is asserted as written. |
| 61z Fusion and the refusals | A Fusion tapered mill imports; missing values unset; no shaft diameter skipped; Photo and countersink refuse; S17 between the old samples | `dc-library/src/fusion.rs::tapered_ball_mill_imports_as_tapered_ball_nose`, `tapered_mill_with_missing_fields_imports_them_unset`, `tapered_mill_without_a_shaft_diameter_is_skipped`, `tapered_mill_with_a_flat_or_bull_tip_is_skipped`; `validate/tests.rs::tapered_ball_nose_on_photo_is_v_carve_needs_v_bit`, `tapered_ball_nose_countersink_is_error`; `dreamcarve/tests/cli/tapered.rs::photo_with_a_tapered_ball_nose_is_refused`; `dc-gcode/src/safety/tests/tapered.rs::tapered_ball_zone_overlap_between_samples_is_refused` | Covered. |

## Clauses that had no assertion

When this audit was first written these 20 clauses had no assertion it
could find. Each has one now (worklog B5). Every test passes: none found a
defect. Two found a figure of the spec that the code does not give, marked
below and corrected in the spec.

Path shorthand: `acceptance/` is `dc-cam/tests/acceptance/`,
`acceptance_3.rs` is `dreamcarve/tests/cli/acceptance_3.rs`.

| # | Clause | Asserted by |
| --- | --- | --- |
| 51 | At a 0.4 step-over the boundary trace alone is 40 % of the unassisted floor cutting. | `acceptance/clearing.rs::boundary_trace_alone_is_over_a_third_of_the_floor_cutting_at_0_4`. **The figure is 36 %, not 40 %** (64 mm of 179 mm). What the clause is for holds: over a third, so "under a third" cannot hold at 0.4. |
| 55 | A countersink with `dwell_s: 0.5` writes a `G4` in the program. | `cli.rs::countersink_exports_as_plain_moves_with_canned_cycles` |
| 55 | With `start_depth_mm: 0` the mouth would open 3 mm above the pocket floor. | `acceptance_3.rs::countersink_from_the_stock_top_in_a_pocket_warns_and_cuts_above_its_floor`. **Not 3 mm:** the plunge ends at Z −3, so the mouth is at the stock top, 4 mm above the 4 mm Pocket's floor, and the tip 1 mm above it. |
| 61 | `check --preview` draws the sign's floor rings, serif lines, two countersinks and tabbed outline. | `acceptance_3.rs::sign_preview_draws_the_rings_the_serifs_the_countersinks_and_the_tabs` |
| 61d | Output piece order and ids follow position. | `dc-import/src/prep/write.rs::piece_order_and_ids_follow_position_not_document_order` |
| 61j | The prepared output is the same whatever the project it is later used in. | `acceptance_3.rs::image_prep_output_is_the_same_whatever_project_is_beside_it` |
| 61l | The end mill's walls on the boundary are vertical. | `acceptance/boundary.rs::boundary_edge_walls_are_vertical` |
| 61l | The square's corners are cut with the end mill's radius. | `acceptance/boundary.rs::boundary_corners_are_cut_with_the_end_mills_radius` |
| 61l | No V-bit cut comes within 2.887 mm of the square. | `acceptance/boundary.rs::boundary_open_edge_keeps_the_v_bit_its_cone_radius_inside` |
| 61m | The rooms of 5.3 and 11.3 mm swap sides on the male. | `acceptance/boundary.rs::boundary_rooms_swap_sides_on_the_male` |
| 61m | `check` prints `inlay_male_mirrored` at X = 55 mm for the moved frame. | `acceptance_3.rs::moved_boundary_check_names_the_mirror_at_x_55` |
| 61n | A boundary smaller than the star is `inlay_boundary_crosses_design`. | `acceptance/boundary.rs::boundary_smaller_than_the_star_crosses_the_design` |
| 61n | `inlay_boundary_empty` does not stop export. | `acceptance_3.rs::empty_boundary_warns_and_still_exports` |
| 61n | The band is cut through an ignored boundary hole. | `acceptance/boundary.rs::band_is_cut_through_an_ignored_boundary_hole` |
| 61o | The band of two boundaries is the two squares less the stars. | `acceptance/boundary.rs::two_boundaries_band_is_the_two_squares_less_the_stars` |
| 61p | The freed plug's outline is the square inset by 3.175 mm. | `acceptance/boundary.rs::boundary_cut_out_frees_the_square_inset_by_the_end_mills_diameter` |
| 61r | One male program fails both S17 and S9 after the boundary moves. | `dc-gcode/src/safety/tests/boundary.rs::moved_boundary_fails_one_males_programs_by_s17_and_s9`. One male, not one program: a program is one tool's run, held by S17 (a V-bit) or S9 (an end mill). The spec is reworded. |
| 61v | `check` prints the tapered ball nose's 0.116 mm floor ridge. | `dreamcarve/tests/cli/tapered.rs::check_prints_the_tapered_floor_ridge` |
| 61w | With a tapered ball nose the outline at Z 0 is the drawn one. | `acceptance/tapered.rs::tapered_carve_cuts_the_drawn_outline_at_the_surface` |
| 61x | The star rounded 2 mm seats at `glue_gap_mm` 1.5 (the spec's fixture was in no test). | `dc-gcode/tests/tapered_inlay.rs::rounded_star_seats_at_a_glue_gap_of_1_5` |

The tests of `acceptance/` read the generators through `dc-cam`'s public
functions, in new files, because `dc-cam`'s own test modules were being
moved while they were written.

### Spec-named tests that did not exist

The design spec's test plan named these when this audit was first
written, and no test had the name. The first two are written now; the
plan names the others as they were built.

| Name in the test plan | What exists |
| --- | --- |
| `order::clearing_pockets_are_not_containing_pockets` | Written, in `dc-cam/src/order.rs`. The name is from the first design, where a Pocket naming a carve cleared its floor and was not to count as the Pocket the carve starts in. Clearing passes are listed in the carve now, so the test asserts the same rule in today's terms: a carve's own clearing pass is no floor it starts from (the start plane with a pass is what it is without one: the stock top, uncleared, or the earlier Pocket's at the Pocket's depth), and an earlier carve's floor is the V-Carve's, pass or no pass. |
| `order::inlay_is_exempt` | Written, in `dc-cam/src/order.rs`: an Inlay inside a Pocket has no start plane, with its male board unset or measured, so neither `start_depth_mismatch` nor `start_depth_uncleared`, where the same square as a V-Carve is a mismatch. |
| `cli::sign_exports_four_programs_in_tool_run_order` | `cli.rs::sign_exports_in_tool_run_order`: the four programs, without the preview content the plan lists (acceptance 61). |
| `cli::check_prints_the_carve_summary_and_the_mirror_note` | Nearest: `cli.rs::inlay_exports_and_passes_safety` (the mirror note once) and `cli.rs::v_carve_exports_and_passes_safety`. |
| `inlay::mirror_negates_bulges_and_keeps_areas` | `dc-geometry/src/transform.rs::mirror_x_about_negates_bulges_and_keeps_areas`. |
| `inlay::male_shank_check_in_the_safety_pass_counts_from_the_stock_top` | `dc-gcode/src/safety/tests/inlay.rs::male_shank_check_counts_from_the_stock_top`. |
| `migrate::v19_to_v22_steps_bump_the_version_only` | One test a step: `migrate/tests.rs::v20_to_v21_bumps_the_version_only`, `v21_to_v22_bumps_the_version_only`. |
| `migrate::version_<n>_fixture_loads` for 19 to 25 | Rows 19 to 25 of `migrate/tests.rs::every_older_sample_fixture_loads_as_the_sample_project`, folded into a table this week (worklog F1). |
| `migrate::v26_with_bitmaps_already_present_fails` | Five rows of `migrate/tests.rs::a_file_holding_what_a_later_version_adds_fails_at_that_step`, each named after the old test in its `case`. |
| `fusion::unsupported_types_are_skipped` | `dc-library/src/fusion.rs::unsupported_types_skipped_by_name`. |

`cli::lettered_inlay_female_and_male_export`,
`validate::cut_out_on_a_female_is_an_error`,
`serialize::photo_v_carve_is_format_26` and
`validate::tapered_ball_nose_v_carve_is_refused_until_built` are old names
the plan itself replaces; their successors exist.

## Safety rules

The safety pass is `dc_gcode::safety::check_parsed`, run on every program
before `export` writes it; its `material::check` stage holds S9 and the
three rules this phase adds. S1 to S16 are phase 1's and phase 2's and were
audited in #255.

| Rule | Enforcing check | Tests | Status |
| --- | --- | --- | --- |
| S17 A V-bit cuts only its own material | `dc-gcode/src/material/cone.rs` (`Cone::check`, `least_room`) → `ExportError::ConeOffMaterial`, `ConeHeightExceeded`. Exact, not sampled: cuts and arcs are measured segment against segment, a sloping cut by a closed form. The generator checks too, before it returns: `dc_cam::vcarve::cone_in_material` → `CamError::ConeOffMaterial`. | Negative, `dc-gcode/src/safety/tests/s17.rs`: `cone_through_a_wall_fails_s17`, `tip_below_the_flat_depth_fails_s17`, `tip_below_the_cone_height_fails_s17`, `a_sideways_cut_above_the_start_plane_fails_s17`, `cone_test_is_exact_between_vertices`, `shank_engagement_from_the_stock_top_fails_s17`; `safety/tests/inlay.rs::male_cut_inside_the_plug_fails_s17`, `male_shank_check_counts_from_the_stock_top`. Positive: `generated_v_carves_pass_s17`, `s17_uses_no_generator`. Generator: `dc-cam/src/vcarve/tests.rs::generator_refuses_its_own_toolpath_on_a_cone_violation`. | Covered. The generator's copy measures wall arcs as chords within the flatten tolerance; the program's check measures them as arcs. |
| S18 A countersink's depth is derived | `dc-gcode/src/material/countersink.rs` (`Sink::check`) → `ExportError::CountersinkDepth`, `ConeHeightExceeded` | `dc-gcode/src/safety/tests/s18.rs::countersink_at_the_wrong_depth_fails_s18`, `countersink_written_as_g81_fails_s18`, `countersink_off_centre_fails_s18`, `uncleared_countersink_engagement_fails_s18`, `generated_countersinks_pass_s18` | **Partial:** depth, centre, plain moves and mouth are covered. No countersink-specific negative test for S1 or S15 on the parsed program was found; `dc-cam/src/drill.rs::countersink_below_the_stock_bottom_is_refused` holds S1 at the generator. |
| S19 Grooves stay in the picture, between-groove moves are fed | `dc-gcode/src/material/photo.rs` (`Picture::check`) → `ExportError::GrooveOffPicture`, `GrooveLinkRapid` | `dc-gcode/src/safety/tests/photo.rs::groove_outside_the_picture_fails_s19`, `groove_link_as_a_rapid_fails_s19`, `groove_below_max_depth_fails_s19`, `hand_written_grooves_pass_s19`, `generated_photos_pass_s19` | Covered. |
| S17 and S9 with a drawn boundary | `inlay_material` in `dc-gcode/src/material.rs` re-derives the band with `dc_project::male_band` | `dc-gcode/src/safety/tests/boundary.rs::boundary_band_is_derived_from_the_instance`, `moved_boundary_fails_the_old_program`, `cut_out_off_the_boundary_ring_fails`, `v_bit_across_a_drawn_open_edge_fails_s17`, `boundary_programs_are_held_to_the_drawn_band_not_the_margin`, `generated_boundary_males_pass_s17_and_s9` | Covered. |
| S17 with a tapered ball nose | The same check with the bit's profile; the shank limit is the cutting height; S7 reads the cut radius | `dc-gcode/src/safety/tests/tapered.rs::tapered_cut_touching_a_wall_passes`, `tapered_ball_zone_overlap_between_samples_is_refused`, `tapered_arc_wall_overlap_is_refused_exactly`, `tapered_virtual_v_bit_overlap_on_the_cone_is_refused`, `tapered_shank_is_the_cutting_height`, `tapered_stock_edge_reads_the_cut_radius`; `safety/tests/tapered_inlay.rs::tapered_male_open_edge_is_held_at_the_stock_top`, `tapered_inlay_regions_are_the_rounded_design` | Covered. |
| `image-prep` is not a safety layer | None: a property of the command and of the generators | `cli.rs::image_prep_help_has_no_machining_options`, `line_art_to_inlay_end_to_end`; unprepared designs, not ignored: `dc-gcode/tests/stress/main.rs::line_art_designs_carve_or_explain`, `lettering_designs_carve_or_explain`, `generator_never_refuses_its_own_move` | **Partial:** the sweeps the rule cites (#464, #604, #681) are `#[ignore]`d and run by hand; the tests that always run cover a handful of small generated designs. |
| S2, S6, S7 unchanged | The existing checks | S2: `vcarve/tests.rs::start_depth_lowers_every_cut_and_plunges_from_the_stock_top`. S7: `safety/tests/photo.rs::edge_groove_width_is_s7s_business`, `cli.rs::picture_past_the_stock_edge_fails_s7`. S6: `generated_photos_pass_s19` (positive only) | **Partial:** no groove-specific negative test for S6 was found. |

## Sub-phases and format versions

ADR-0002 and `docs/rust-guidelines.md` ask for a committed fixture of each
format a migration starts from. Every format from 20 to 28 has one,
`crates/dc-project/tests/fixtures/dcv/v<N>/sample.dcv`, loaded by
`dc-project/src/migrate/tests.rs`. The sample is the plain sample project,
so the last column says whether a committed file of that version also holds
what the format added. For 21, 22 and 25 that file was added on 2026-11-19.

| Sub-phase | Format | What it added | Committed fixture | Holds the new content? |
| --- | --- | --- | --- | --- |
| 3.1 V-Carve | 20 | `v_carve` | `dcv/v20/sample.dcv`; `tests/golden/vcarve-*/dreamcarve.dcv` (eight) | Yes, the goldens |
| 3.2 Countersink | 21 | the `countersink` Drill cycle | `dcv/v21/sample.dcv`, `dcv/v21/countersink.dcv` | Yes: the sample with the 90° V-bit, a 3.175 mm hole and a countersink Drill of it |
| 3.3 Inlay | 22 | `inlay` | `dcv/v22/sample.dcv`, `dcv/v22/inlay.dcv` | Yes: a female Inlay as format 22 wrote it, with `side` and without `cut_out`, `outline_rounding_mm`, `male_board` or `boundary` |
| 3.3 Inlay | 23 | `cut_out` | `dcv/v23/sample.dcv`; `dreamcarve/tests/fixtures/inlay-migration/star-female.dcv`, `star-male.dcv` | Yes |
| 3.3 Inlay | 24 | `outline_rounding_mm` | `dcv/v24/sample.dcv`; `inlay-migration/dc-female.dcv`, `dc-male.dcv` | Yes |
| 3.3 Inlay | 25 | `male_board`, one file for both boards | `dcv/v25/sample.dcv`, `dcv/v25/inlay-male-board.dcv` | Yes: an Inlay whose male board is measured, 10 mm thick, so the one file cuts both boards; no `boundary` |
| 3.4 Image Prep | 26 | `boundary` | `dcv/v26/sample.dcv`, `dcv/v26/inlay-boundary.dcv` | Yes |
| 3.5 Photo V-Carve | 27 | `bitmaps`, `photo_v_carve` | `dcv/v27/sample.dcv`, `dcv/v27/photo.dcv`; `tests/golden/photo-raster-*/dreamcarve.dcv` | Yes |
| 3.6 Tapered Ball Nose | 28 (current) | the `tapered_ball_nose` cutter | `dcv/v28/sample.dcv`, `dcv/v28/tapered.dcv`; `tests/fixtures/dc-tbn.dcv`, `tbn-coupon.dcv` and the other current projects | Yes |
| 3.7 Release | none | | | |

The rule is met for every format, and every format now has a committed
file that holds what it added. No current build can save an old format, so
the three new files were written by hand, each from its version's
`sample.dcv` and the step's description in `dc-project/src/migrate.rs`
(the Inlay's fields at format 22 were checked against the model as the
repository's history has it at that format). `migrate/tests.rs::feature_fixtures_load_holding_what_their_format_added`
proves each: the file says its version, holds the format's own words and
none of a later format's, loads at the current version as exactly the
project it should be (the sample plus the tool, shape and operation, built
from the fixture builders), with nothing for validation to report and
nothing noted, that project holds the feature, and it saves and loads
again unchanged.

## Documentation deliverables

Every file is under `docs/book/src/`. Presence was checked by reading the
summary and searching each chapter for the terms the spec promises.

| Deliverable | Where | Status |
| --- | --- | --- |
| V-Carve operation: medial lines, flat depth, clearing passes, start depth, one pass, the offset strategy, choosing a bit angle | `user-guide/v-carve.md` | Present |
| V-Carve updated for a tapered ball nose: depth law, fillet, ridge, soft corners, `tapered_corners_softened` | `user-guide/v-carve.md`, `user-guide/tapered-ball-nose.md` | Present |
| Inlay: the method, the four values, the two boards and the male board, the fit procedure, re-cutting the male, gluing and planing | `user-guide/inlay.md` | Present |
| Inlay updated for the drawn boundary: drawing, naming, mirrored, the cut-out's inset, several, clipping, replacing margin and rounding | `user-guide/inlay.md` | Present |
| Inlay updated for a tapered ball nose pair: the tangent height, the rounding and its radii, the seat line, the coupon fit | `user-guide/inlay.md`, `user-guide/tapered-ball-nose.md` | Present |
| Photo V-Carve: pictures and `image`, every setting, dark and light wood, picking a bit and depth, the image editor first | `user-guide/photo-v-carve.md`, `user-guide/images.md` | Present |
| Drill updated: countersink | `user-guide/drill.md` | Present |
| Preparing artwork (new) | `user-guide/preparing-artwork.md` | Present |
| Importing, and Shapes, holes and islands, updated with the checks | `user-guide/shapes-and-holes.md`, `user-guide/importing-dxf.md`, `user-guide/overview.md` | Present. The overview points to Preparing artwork and names no code itself. |
| Warnings: every code of capabilities 48, 49g and 49l | `user-guide/warnings.md` | Present. The spec names `vcarve_target_open` as the chapter and the code do. |
| The project file: the new kinds, the bitmap, formats 20 to 28, `boundary` | `user-guide/project-file.md` | Present |
| Tool library: V-bit angles, cone height; the tapered ball nose, its four numbers, the two plunge holes, Fusion's tapered mill | `user-guide/tool-library.md`, `user-guide/tapered-ball-nose.md` | Present |
| Safety: S17 to S19, the boundary, the tapered ball nose | `user-guide/safety.md` | Present. The chapter numbers its rules its own way (S17 is its rule 14) and names only S19 by the spec's number. |
| Quick start: "A V-carved sign from SVG outlines" | `quick-start/sign.md`; `quick_start.rs::v_carved_sign_chapter_runs` | Present, run as written by its test |
| Quick start: "An inlay pair", with "Preparing your own design" pointing to the chapter | `quick-start/inlay.md`; `quick_start.rs::inlay_pair_chapter_runs`, `inlay_boundary_chapter_runs` | Present |
| Quick start: "From line art", on `compass-art.svg` | `quick-start/line-art.md`; `quick_start.rs::line_art_chapter_runs` | Present, as a chapter of its own rather than a section of the inlay quick start |
| Quick start: "Carving and inlaying with a tapered ball nose" | `quick-start/tapered-ball-nose.md`; `quick_start.rs::tapered_ball_nose_chapter_runs` | Present |
| Quick start: "Carving a photo" (not in the spec's list) | `quick-start/photo.md`; `quick_start.rs::photo_quick_start_runs` | Present |
| Contributing: porting rules for medial lines and offset rings; the RasterCarve golden how-to; the crate map | `contributing/porting.md`, `contributing/golden-corpus.md`, `contributing/crate-map.md` | Present |
| `CONTEXT.md` terms | `CONTEXT.md` | Present |

### Capabilities and their chapters

`AGENTS.md`: a feature without a chapter is not done. All 24 capabilities
(38 to 49, 49a to 49g, 49h to 49l) have one.

| Capability | Chapter |
| --- | --- |
| 38 V-Carve, 39 Flat depth, 40 Clearing, 41 Start depth, 42 One pass, 43 Offset strategy | `user-guide/v-carve.md` |
| 44 Countersink | `user-guide/drill.md` |
| 45 Inlay | `user-guide/inlay.md`, `quick-start/inlay.md` |
| 46 Bitmap | `user-guide/images.md` |
| 47 Photo V-Carve | `user-guide/photo-v-carve.md`, `quick-start/photo.md` |
| 48 Warnings and errors | `user-guide/warnings.md` |
| 49 Golden parity | `contributing/golden-corpus.md` |
| 49a Import checks, 49b `image-prep`, 49c What it does, 49d Thresholds, 49e The report | `user-guide/preparing-artwork.md`, `quick-start/line-art.md` |
| 49f Drawn boundary | `user-guide/inlay.md` |
| 49g Warnings and errors of 3.4 | `user-guide/warnings.md` |
| 49h Tapered ball nose, 49l The library, Fusion import and the codes | `user-guide/tool-library.md`, `user-guide/tapered-ball-nose.md` |
| 49i Carving, 49j Corners | `user-guide/v-carve.md`, `user-guide/tapered-ball-nose.md` |
| 49k Inlay with a tapered ball nose | `user-guide/inlay.md`, `quick-start/tapered-ball-nose.md` |

Capabilities without a chapter: none.

## Open items that bear on the release

From `docs/alpha-worklog.md`, each checked against the code where it could
be.

| Item | What is open | Checked |
| --- | --- | --- |
| B1 Image-prep defaults (#599, open) | `--min-detail` 0.2 and `--smooth` 0.05 are calibrated on the synthetic fixtures only. The maintainer's run on their own art has not been done, so both are still proposals. | `tests/fixtures/prep/README.md`, "Still to confirm", gives the commands and what to look at. Open questions 16 and 17 of the functional spec say the same. Waits for the maintainer. |
| B4 `Narrowest gap` in the report | For the same letters the report gives 0.19 to 3.0 mm depending on `--smooth` (`dc-geometry`). Worklog status: todo. | The README records it and tells the reader not to judge by that line. No test would catch it: acceptance 61h pins the gap only at the default `--smooth` on clean strokes. It bears on the 3.4 bench check, whose success criterion compares the cut ridges with "the 2.5 mm the report gave". |
| A7 Generators fail loud (worklog status: doing) | Done in `dc-cam`. Not done inside `dc-geometry`: about 275 unclassified lookups, mostly in `medial.rs`, `merge.rs`, `simplify.rs`, `self_band.rs` and `polyline.rs`. | `dc_geometry::medial_axis` and `dc_geometry::boolean` still return a value, not a `Result` (`medial.rs` line 264, `boolean.rs` line 41). `GeometryError::Internal` exists and is raised in a few places. |
| A6, left over | Four tests in `dc-gcode` still assert wall-clock time: `safety/tests/s17.rs::s17_check_is_fast_on_the_sign` and `safety.rs::s9_check_is_fast_on_twenty_profile_parts`, `s9_check_is_fast_on_an_on_line_outline_of_many_chords`, `s9_check_is_fast_on_helix_pocket`. | Present. They can fail on a slow CI runner, which matters for the three-OS run the tag needs. |
| A5, left over | SVG ids and `.dcv` names are not cleaned of control characters before the CLI prints them; DXF text is. | `crates/dreamcarve/src/output.rs` has no such filter. |
| A4, noted | `drop_small_components` uses the same kind of step-relative threshold as the pruning that was fixed, and could drop a short channel open at both ends. Not hit, not changed. | `dc-geometry/src/medial.rs` line 1365. |
| GitHub issues #710 to #713 | Fixed on `alpha` (A1 to A4), still open on GitHub because the push is local-only. | They close when `alpha` merges. |

Found while auditing:

- **The three-OS criterion.** `alpha` has had no run on Linux or Windows,
  and `main`'s last run is marked failed (the macOS job was cancelled).
  The tag needs one green run of the merged branch.
- **The sign's full cut has no recorded result.** #429 was closed as
  completed on 2026-10-30 with one comment, the instructions for the cut.
  The spec's record says so; only the maintainer has the result. The
  lettered pair's record now holds what #438 says, which states no
  machine, bit, board or measurement.
- **`docs/plan.md` said four test parts.** Its "Exit" and "Real cut" now
  name five, with the tapered ball nose part.
- **Both phase 3 specs still say "Status: draft".**
- **Formats 21, 22 and 25** have their fixtures now (see "Sub-phases and
  format versions").
- **The V-Carve codes come in two spellings.** The generator's three
  warnings are `vcarve_target_open`, `vcarve_no_medial_axis` and
  `vcarve_sampling_budget`; validation's error is `v_carve_needs_v_bit`,
  after the file's `v_carve`. The spec was brought to the code. Making
  them one spelling would change codes users have seen, so it is the
  maintainer's to decide.
- **The release dry run** (`dist plan`, `dist build --artifacts=local`)
  was not run. The Release workflow last ran green on the `Code-Review`
  branch at `6cf39898` (2026-11-18), before this week's commits.

### Where the spec and the code disagree

None of these changes machine output. On 2026-11-19 each was settled the
same way where it could be: the code is what users have seen, so the spec
or the comment was brought to it. The last column says what was done.

| Where | The spec said | The code and tests say | Now |
| --- | --- | --- | --- |
| Capability 38 and 48, acceptance 52 | `v_carve_target_open` | `vcarve_target_open` (`dc-cam/src/warning.rs`, and the Warnings chapter) | Spec corrected, three places |
| Acceptance 51 and 57 | Runs named `op1 (clearing t2)` and `op1 (cut-out t2)` | Program comments carry the operation's name and no parentheses (`Background clearing t2`); the parenthesised form with the id is in the summary and the preview legend | Spec says where each form is |
| Acceptance 51 | At 0.4 the trace alone is 40 % of the unassisted floor cutting | 36 %: 64 mm of 179 mm (176 mm by the blocks) | Spec corrected. Found while closing the gaps |
| Acceptance 54 | The 6 mm square "matches PureCutCNC's `v_carve` golden" | The only offset golden is a 20 mm square, 60°, step-over 1 mm, as the design spec's corpus lists | Spec names that golden |
| Acceptance 55 | From the stock top "the mouth would open 3 mm above the floor" | The plunge ends at Z −3: the mouth at the stock top, 4 mm above the 4 mm Pocket's floor, the tip 1 mm above it | Spec corrected. Found while closing the gaps |
| Acceptance 61 | Each program passes S1 to S19 | The tests' comments said S1 to S18; the sign has no Photo V-Carve, so S19 has nothing to check | Comment corrected |
| Acceptance 61b | The output holds none of the clip path's content | The clip group's content is imported unclipped and is in the output | Spec corrected |
| Acceptance 61n | `inlay_boundary_crosses_design` names "the nearest point"; each check is at `operations[i].boundary` | The message names the worst point and how far it lies past the boundary; the location carries the operation's id and, per entry, an index (`operations[o7].boundary[1]`) | Spec corrected |
| Acceptance 61r | A male program "fails S17 and S9" | S17 fails a V-bit's program and S9 an end mill's; no one program fails both | Spec reworded; one male's two programs now fail one rule each in a test |
| Acceptance 61t | `tools[0].cutter.tapered_ball_nose.flute_length_mm` | Located by the tool's id, `tools[t1]…`, as capability 49l's own example is (`tools[t3]…`) | Spec corrected |
| Acceptance 61v | A 6 mm square in offset mode, no flat depth | The test adds `flat_depth_mm` 10; without it the carve would pass the flutes | Spec gives the flat depth |
| Acceptance 61w | A heightfield of a 90° wedge | A square's corner, measured on the exact surface | Spec corrected |
| Acceptance 61x | The star rounded 2 mm, `glue_gap_mm` 1.5 | The bit-rounded star at `glue_gap_mm` 1.0 | **Partly open.** The seat and the safety pass are now asserted on the spec's fixture; the female-equals and walls-mate clauses are not |
| Capability 49l, the design's "`check`'s V-bit floor line" | A tapered ball nose is named where it applies | `ExportError::ConeOffMaterial` and `ConeHeightExceeded` say "V-bit" and "cone", and `check`'s ridge line says "V-bit floor ridges", whatever the bit | **Open.** The messages are the ones to change, in `dc-gcode` and `dc-job`; the spec is left as it is |
| Design spec, test plan | The names under "Spec-named tests that did not exist" | Renamed or folded into tables; two had no test | Test plan corrected; the two tests written |
| Comments in tests | `cli.rs::line_art_to_inlay_end_to_end` said the spec's 61k names `ring-cross.svg`; `compass_inlay_exports_both_boards` said the stand-in is used "until that command is built"; `dc-gcode/tests/stress/main.rs` said the sweep "still fails" while #484 is open | The spec's 61k names `compass-art.svg`; the command is built; #484 is closed | Comments corrected |

## CHANGELOG `[Unreleased]`

This week's user-visible fixes, against the section as it stands after this
slice:

| Fix (worklog) | Entry |
| --- | --- |
| A1 An Inlay's schema error names its field (#712) | Added with this slice |
| A2 A clipped Inlay cut-out's tab count (#710) | Already there |
| A3 The safety pass fails closed (#711) | Added with this slice |
| A6 A cut outside every operation is refused | Added with this slice |
| A4 The plug proud in a narrow channel inside a drawn boundary (#713) | Added with this slice |
| A5 The DXF join budget, the control characters, the file-size cap | Added with this slice (two entries) |
| A5 `init` and `image-prep` never replace a file | Added with this slice |
| A7 A generator stops with an internal-defect error | Added with this slice; it says the geometry code is not done |
| A8 A smooth tab at an open path's end | Added with this slice |
| A9 A fitted arc into a dogbone notch | Already there |

The section has no release heading and no date: those come with the tag.

## Remaining steps

For the maintainer:

1. **Bench checks 3.4, 3.5 and 3.6** (#606, #649, #684), and the run of
   `image-prep` on their own art that confirms the defaults (#599). Record
   each cut in the functional spec's "Test parts", with the date, the
   machine and the outcome against each success item.
2. **Record the sign's full cut** (#429): the issue holds no result. The
   inlay pair's record holds what #438 says; add the machine, the bits and
   any measurement if they are to be on record.
3. **Decide the partial rows and the open items**: accept each, or name
   the ones to close before the tag. The `Narrowest gap` line (B4), the
   messages that say "V-bit" for a tapered ball nose, and the two
   spellings of the V-Carve codes are the ones this audit would look at
   first.
4. **Tag `v0.3.0`**, per `docs/plan.md`, "Versioning": set the workspace
   version to `0.3.0` (it is `0.2.1`), rename the changelog's
   `[Unreleased]` to `[0.3.0] - <date>` with a fresh empty section above
   it, tag the merge commit, check that the release has its binaries, then
   bump to `0.3.1`.

For an agent, before the tag, once `alpha` is merged or pushed for CI:

1. One green run on Ubuntu, macOS and Windows.
2. The release dry run: `dist plan`, then `dist build --artifacts=local`.
3. What is still open in "Where the spec and the code disagree": the two
   rows marked open.
