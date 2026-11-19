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

## Summary

- **Not ready to tag.** The code and the documents are there. What blocks
  `v0.3.0` is listed under "What blocks the tag" below: three bench checks,
  one run of the suite on all three operating systems, and the tag
  procedure, which has not started.
- **Exit criteria.** `docs/plan.md` gives six for phase 3. Three are met
  (parity with PureCutCNC's V-carve tests, the sign cut, the inlay pair cut
  and fitted). Three wait for the maintainer (the line-art inlay, the photo
  V-carve, the tag). The functional spec adds six of its own, checked in
  the second table: two are met, one is met in the repository and not yet
  published, one is not shown (green on all three OSes), and two wait for
  the maintainer. Its fifth test part, the tapered ball nose (#684), also
  waits.
- **Acceptance tests.** There are 40: 48 to 61, 61a to 61s and 61t to 61z.
  All 40 have tests behind them. 21 are covered clause for clause. 19 are
  partial: most clauses are asserted, and at least one is asserted on
  another fixture, at another layer, or not at all. None is without
  evidence, none waits for the maintainer, and no covering test is
  `#[ignore]`d.
- **Clauses with no assertion found: 20, in 14 of the partial rows.** They
  are listed under "Clauses with no evidence found". None is a safety
  rule. The other five partial rows (53, 54, 61b, 61h, 61y) assert every
  clause, some on another fixture or more weakly than written.
- **Test-plan names with no test: 2**,
  `order::clearing_pockets_are_not_containing_pockets` and
  `order::inlay_is_exempt`.
- **Safety rules.** Of the seven entries under "Safety requirements", four
  are covered (S17, S19, the drawn boundary, the tapered ball nose) and
  three are partial (S18, "`image-prep` is not a safety layer", "S2, S6,
  S7 unchanged").
- **Documentation.** Every capability has its chapter, and every promised
  chapter and quick start exists. One code is named differently in the
  spec and in the product (`v_carve_target_open` against
  `vcarve_target_open`).

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
slices: the 19 partial rows, the `Narrowest gap` inconsistency (worklog
B4), the unfinished part of A7 in `dc-geometry`, and the places where the
spec and the code disagree ("Open items").

## Evidence

- **Suite.** Not run for this audit. The worklog records the gate (`cargo
  fmt`, `clippy -D warnings`, `cargo nextest run --workspace`, the doc
  tests, `check-headers`) as run on macOS before each slice was marked
  done.
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
  | `dreamcarve-app/tests/review.rs` (three tests) | review pictures, run by hand (phase 4) | No |
  | `tools/third-party-licenses/tests/ci.rs` (two tests) | need cargo-about and dist | Yes, with `--ignored` |

  The worklog (A4, A7) records all three sweeps as passing with 0 failures
  on macOS this week, after the medial-axis fix, with identical totals
  after the generator changes.
- **Bench issues**, read with `gh issue view` on 2026-11-18:

  | Issue | State | Note |
  | --- | --- | --- |
  | #412 3.1 bench check | Closed 2026-10-29 | The sign without countersinks, cut on a LongMill MK3; recorded in the spec. |
  | #429 3.2 bench check | Closed 2026-10-30, completed | No result comment on the issue, and the spec's cut record has no line for the full cut. |
  | #438 3.3 bench check | Closed 2026-11-10 | The maintainer cut inlays of two designs of their own ("pacman", "shark") and signed off. The spec's cut record still says "not yet cut". |
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
| Test part cut: a lettered sign imported as SVG outlines | **Met; the record is incomplete** | #412 (closed): cut without the countersinks on 2026-10-29, recorded in the functional spec's "Sign" cut record. #429 (closed as completed 2026-10-30): the full cut with the countersinks. Neither the issue nor the spec records its result; the spec still says "The full cut, with the countersinks, is #429's." The maintainer has the result. |
| Test part cut: an inlay pair, female and male, cut and fitted | **Met; the record is stale** | #438 (closed 2026-11-10): the maintainer cut and fitted inlays of two of their own designs and signed off, as the spec's "a bench check may substitute its own design" allows. The spec's "Lettered inlay pair" cut record still says "not yet cut". |
| Test part cut: a line-art inlay prepared with `image-prep` inside a drawn boundary | **Waiting on the maintainer** | #606, open. The stand-in is tested: acceptance 61k and 61s. The defaults it would be prepared with are also unconfirmed (#599). |
| Test part cut: a photo V-carve whose toolpath matches RasterCarve's test cases within the tolerance the spec states | **Waiting on the maintainer** for the cut | #649, open. The match is tested: `dc-gcode/tests/golden.rs::photo_raster_inch` and `photo_raster_metric`, at 0.01 mm in XY and 0.05 mm in Z, the spec's tolerance, with the two departures the cases' READMEs declare (acceptance 60). |
| `v0.3.0` | **Waiting on the maintainer** | Not tagged (`git tag`: `v0.0.1`, `v0.1.0`, `v0.2.0`). The workspace version is `0.2.1`. |
| Real cut: yes, all four parts | **Two of four** | The sign and the inlay pair, as above. The line-art inlay and the photo wait for #606 and #649. |

The plan's exit names four parts. Its sub-phase list and the functional
spec name a fifth, the tapered ball nose carve and inlay (3.6, #684), which
is open and waits for the maintainer too.

### The functional spec's own list

| Criterion | Status | Evidence |
| --- | --- | --- |
| All acceptance tests and safety tests green on all three OSes | **Not met: not shown for `alpha`** | Every acceptance test and safety rule has tests (below). Whether they pass on `alpha` on Linux and Windows is not known: the branch has no CI run and the worklog says "Windows and Linux are not validated in this push". The last all-green run is `main` at `c8a83193`. This audit ran nothing. |
| Golden parity cases captured and passing: the PureCutCNC V-carve cases and the RasterCarve photo cases | **Met** (captured); passing as of the last green CI | Eight V-Carve cases and two photo cases (`photo-raster-inch`, `photo-raster-metric`) in `tests/golden/`, each with its README, compared in `dc-gcode/tests/golden.rs`. The hatch case of the draft was dropped (security review M5), as acceptance 60 says. |
| The five test parts cut on grblHAL and recorded above, the inlay pairs fitted | **Waiting on the maintainer** | Two cut (the sign, an inlay pair); three not (#606, #649, #684). Of the five cut records in the spec, one is filled in, and only for the partial cut. |
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
| 51 Clearing passes | Two runs; the end mill inside the floor; the V-bit's floor cutting under a third; a second pass; the four codes | `vcarve/tests/clearing_passes.rs::clearing_pass_takes_the_floor_region_and_depth`, `clearing_runs_come_before_the_v_bit_run_with_their_own_tools`, `floor_cutting_shrinks_to_what_the_end_mill_cannot_reach`, `floor_coverage_with_the_pass_still_reaches_the_floor`, `second_pass_clears_only_what_the_first_left`, `wider_second_pass_warns_not_descending_and_still_runs`, `clearing_without_a_flat_depth_is_an_error`, `clearing_pass_with_a_v_bit_is_an_error`, `removing_the_list_gives_the_v_bit_the_whole_floor`; `dc-gcode/src/emit.rs::clearing_runs_become_programs_of_their_own_tools` | **Partial:** no assertion found for "at 0.4 the trace alone is 40 % of it". "Export writes two runs" is asserted at the emit layer on the sample rectangle, not the 30 × 10 one. |
| 52 Cone and tool checks | `v_bit_cone_exceeded` naming 10 mm; an end mill refused; an open target skipped | `vcarve/tests.rs::cone_exceeded_is_an_error_naming_the_depth`, `flat_depth_within_the_cone_exports`, `end_mill_is_refused`, `open_path_is_skipped_with_warning`; `dc-project/src/validate/tests.rs::v_carve_with_end_mill_is_error` | Covered. The open-target code is `vcarve_target_open` in the code, not the spec's `v_carve_target_open`. |
| 53 Start depth | Every Z 4 mm lower; the mismatch and uncleared checks; one pass | `vcarve/tests.rs::start_depth_lowers_every_cut_and_plunges_from_the_stock_top`, `uncleared_start_depth_is_refused_and_acknowledged_counts_toward_the_cone`, `one_pass_whatever_the_depth`, `corner_tip_chain_plunges_to_the_surface_not_the_centreline`; `dc-cam/src/order.rs::start_depth_matching_an_earlier_pocket_is_quiet`, `start_depth_with_a_shallower_pocket_names_it`, `disabled_pocket_does_not_count`; `cli.rs::check_warns_start_depth_mismatch`, `check_refuses_an_uncleared_start_depth` | **Partial:** "plunges once per chain and never re-traces a line at a shallower Z" is asserted indirectly (the same toolpath with and without a step-down, fewer than nine plunges). "The rectangle outside it" is asserted only on `order.rs`'s 2 mm sample. The matching case is checked, never exported. The 90° refusal is asserted with the Pocket removed, not disabled. |
| 54 Offset strategy | Rings inset 0.5 to 2.5 at their own depth; both directions; the `v_carve` golden | `vcarve/tests/offset.rs::offset_rings_are_inset_by_their_depth`, `offset_climb_and_conventional_reverse_every_ring`; `golden.rs::vcarve_offset_square` | **Partial:** the rings are asserted on the 6 mm square. The reversal is asserted on a 16 mm square with a hole and a 60° bit. The golden is another fixture, the only offset case: a 20 mm square, 60°, step-over 1 mm, clamped at 4 mm, which is what the design spec's corpus lists. |
| 55 Countersink | Four plunges to Z −3; 82° to −3.451; three codes; a `G4`; the Pocket case to Z −7 | `dc-cam/src/drill.rs::acceptance_55_countersinks_four_holes`, `countersink_depth_is_mouth_over_two_tan_half_angle`, `countersink_needs_a_v_bit`, `countersink_dwell_is_at_the_bottom`, `countersink_depth_is_measured_from_the_start_plane`, `countersink_rapid_stops_above_the_stock_top_whatever_the_start_depth`, `uncleared_countersink_start_depth_is_refused`; `dc-cam/src/order.rs::countersink_start_depth_is_checked_too`; `cli.rs::countersink_exports_as_plain_moves_with_canned_cycles`; `dc-gcode/src/safety/tests/s18.rs::generated_countersinks_pass_s18` | **Partial:** no assertion found for the `G4` a countersink's dwell writes; the dwell is asserted as a move at the bottom, and the `G4` text only for a peck Drill. No assertion found for "the mouth would open 3 mm above the floor". The Pocket case uses one and two holes, and its quiet and mismatch cases are asserted in `order.rs` only, not through the CLI. |
| 56 Inlay female | The female equals a V-Carve with `flat_depth_mm: 4`; unset male board | `inlay/tests.rs::female_equals_a_flat_depth_v_carve`; `inlay/tests/boards.rs::male_runs_wait_for_the_board_thickness`; `cli.rs::inlay_exports_and_passes_safety` | Covered. |
| 57 Inlay male | The surround, the mate within 0.02, the cut-out, the open edge, rounding, clipping to the board | `inlay/tests/boards.rs::both_boards_from_one_operation`, `plug_base_thin_reads_the_male_board`; `inlay/tests.rs::male_carves_the_surround_and_the_holes`, `male_floor_is_at_depth_minus_gap_plus_plug`, `male_flank_passes_through_the_outline_at_the_start_plane`, `female_and_male_walls_mate`, `clearing_pass_clears_the_male_floor`, `cut_out_hugs_the_slope_foot_with_a_v_wall`, `cut_out_hugs_the_band_edge`, `male_with_clearing_has_no_v_wall_at_the_band_edge`; `inlay/tests/board.rs::band_is_clipped_to_the_stock`, `cut_out_runs_edge_to_edge_when_clipped`, `clipped_male_still_seats`, `design_too_near_the_edge_is_explained` | Covered: every sentence has an assertion. Weaker than written: the plug-base warning runs on a rectangle; "shrinks as in 51" is asserted as under half, not under a third; the end mill's outermost path equal to the edge less its radius is asserted on a band rounded by 5 mm. |
| 58 Bitmap and placement | `init` on a picture; `image add` and `image list`; a moved `.dcv`; S7 past the edge | `dreamcarve/tests/cli/picture.rs::init_on_a_picture_writes_a_bitmap_and_a_photo_v_carve`, `image_add_places_a_picture_and_image_list_shows_it`, `image_add_converts_colour_and_says_so`, `moved_dcv_exports_the_same_program`; `cli.rs::picture_past_the_stock_edge_fails_s7` | Covered. |
| 59 Photo grooves | Angle, spacing 1.179, depths by tone, invert, hatch, the fed lift, the overlap warning | `dc-cam/src/photo/tests.rs::every_cut_is_inside_the_footprint`, `grooves_run_at_the_line_angle_and_alternate`, `grooves_are_spaced_by_the_fraction_of_the_full_depth_width`, `one_groove_passes_through_the_top_left_corner`, `black_is_max_depth_white_is_the_surface_and_grey_is_between`, `invert_swaps_black_and_white`, `zero_angle_grooves_are_parallel_to_x`, `hatch_adds_a_second_family_at_ninety_degrees`, `between_grooves_the_bit_rises_to_the_retract_and_feeds_across`, `spacing_below_one_warns` | Covered, with the spec's numbers and tolerances. |
| 60 Photo parity | Groove for groove against RasterCarve, 0.01 mm XY, 0.05 mm Z | `golden.rs::photo_raster_inch`, `photo_raster_metric` (`golden/photo.rs::check`) | Covered at the spec's tolerances. Declared departures: `Resampled` on the inch case, `LastLineUncut` on the metric case (DreamCarve cuts 111 grooves to RasterCarve's 110). The comparison reads the generated toolpath, not the emitted program. |
| 61 Sign end to end | Four programs in tool-run order; what the preview draws; S1 to S19 | `cli.rs::sign_exports_in_tool_run_order`, `sign_countersinks_sit_on_the_end_mill_cleared_floor`, `sign_lettering_is_within_the_cone`; `dreamcarve/tests/quick_start.rs::v_carved_sign_chapter_runs` | **Partial:** no assertion found for what `check --preview` draws on the sign (the floor rings, the lines into every serif, the two countersinks, the tabbed outline). The sign's preview is only compared byte for byte with `dc-job`'s render (`dreamcarve/tests/pipeline_move.rs`). The safety clause is asserted as `export` succeeding, which runs the whole pass. |

## Acceptance tests 61a to 61s (sub-phase 3.4)

Path shorthand: `prep/` is `dc-import/src/prep/`, `boundary.rs` is
`dc-cam/src/inlay/tests/boundary.rs`, `validate` is
`dc-project/src/validate/tests.rs`.

| # | Asserts | Covering tests | Status and notes |
| --- | --- | --- | --- |
| 61a Import checks | One warning of each kind on `messy.svg`; the project unchanged; `check` repeats two | `cli.rs::init_prints_the_import_checks`, `init_project_is_unchanged_by_the_checks`, `check_repeats_overlap_and_size_findings`, `a_clean_drawing_prints_no_checks`; `prep/findings.rs::overlap_names_the_pair_and_where`, `feature_below_size_counts_and_names_the_smallest` | Covered. "Byte for byte what the build wrote before the checks" is equality with the committed `tests/fixtures/prep/messy.dcv`, a version 28 file, so it is a regression golden of today's build. |
| 61b Visible geometry only | Hidden, text, image and clip path reported; none in the output | `prep/run.rs::messy_svg_reports_what_it_skipped_and_leaves_it_out`; `prep/walk.rs::text_images_clip_paths_and_masks_are_counted_with_their_fix`; `cli.rs::image_prep_prints_warnings_then_the_report` | **Partial, and the spec and the test disagree:** the counts and the hidden square's absence are asserted, but the test pins the clip group's content in the output, imported unclipped (400 of the 920 mm²), where the spec says "the output holds none of them". |
| 61c Strokes become areas | 43.14, 40.00, 44.00, 80.00 and 79.79 mm²; dashes; a non-uniform scale | `prep/stroke.rs::round_caps_add_pi`, `butt_caps`, `square_caps`, `miter_join_at_a_right_angle`, `round_join_at_a_right_angle`, `dashes_give_one_piece_each`, `a_non_uniform_scale_widens_unevenly` | Covered, at the stroke-outline layer. |
| 61d Fills are resolved | 150 and 300 mm²; nonzero against even-odd; order by position | `prep/run.rs::overlapping_squares_are_one_piece`, `an_island_path_becomes_a_hole`, `fill_rule_is_per_path`, `output_is_independent_of_document_order` | **Partial:** no assertion found that piece order and ids follow position; they are asserted to be the same for both document orders. |
| 61e Regions mode | One piece of 584.3 mm² with four holes; four pieces of 2243 mm² in all; open ink | `prep/run.rs::shapes_mode_of_the_ring_and_cross_is_one_piece_with_four_holes`, `regions_mode_of_the_ring_and_cross`, `regions_mode_output_never_overlaps_ink`, `regions_mode_with_a_floating_cross_warns_open_ink`; `prep/report.rs::ridge_is_the_narrowest_gap_in_regions_mode` | Covered. |
| 61f Clean-up | Specks, holes, hairlines, the staircase, the range and the warning | `prep/run.rs::specks_and_hairlines_are_dropped`, `small_holes_are_filled`, `zero_min_detail_keeps_everything`, `smooth_removes_a_staircase_and_keeps_corners`, `smooth_zero_leaves_the_staircase`, `threshold_out_of_range_is_an_error`, `smooth_above_half_the_detail_warns`; `cli.rs::image_prep_threshold_range_exits_2` | Covered. |
| 61g The output file | Millimetres, filled paths only, no flip, never overwrites, a picture refused | `prep/write.rs::declares_millimetres_and_the_input_page`, `paths_are_filled_and_never_stroked`, `a_lone_rectangle_round_trips`, `y_is_flipped_back_so_an_f_is_an_f`; `cli.rs::image_prep_refuses_to_overwrite`, `image_prep_refuses_a_picture_and_names_inkscape`, `image_prep_malformed_svg_writes_nothing`, `image_prep_then_init_has_no_warnings` | Covered. |
| 61h The report | Height 60.00; gap 2.00 mm and its figures; thinnest piece; two warnings; counts | `prep/report.rs::design_height_and_counts`, `widths_scale_to_100_200_300`, `reaching_height_for_the_guide_width`, `thinnest_piece_is_the_least_median`; `dc-job/src/prep_report.rs::the_ring_and_cross_report_names_its_figures`; `cli.rs::image_prep_prints_the_report_of_the_ring_and_cross`; `prep/run.rs::regions_suggested_for_thin_ink_and_not_for_thick`, `background_shape_is_flagged` | **Partial:** the background test uses a rectangle over 100 % of the box, so the 98 % threshold is not pinned; the default-mode thinnest piece is asserted in the library only. The gap is pinned only at the default `--smooth` on clean strokes: no test computes it at two `--smooth` values, so none would catch the inconsistency of worklog B4. |
| 61i Determinism | Twice, reverse order, CRLF: the same bytes | `prep/write.rs::same_input_twice_reverse_order_and_crlf_give_identical_text`; `prep/run.rs::same_input_twice_is_identical`, `output_is_independent_of_document_order`, `crlf_input_gives_the_same_bytes` | Covered. |
| 61j No machining inputs | No machining option in `--help`; the output independent of the project | `cli.rs::image_prep_help_has_no_machining_options` | **Partial:** no assertion found for "the output is the same whatever the project it is later used in"; it follows from the command taking no project. |
| 61k Line art to inlay | Eight pieces, a 2.5 mm gap, one expected warning, both boards safe, the seat | `cli.rs::line_art_to_inlay_end_to_end`; `dreamcarve/tests/quick_start.rs::line_art_chapter_runs` | Covered. "The same pockets within 0.05 mm" compares bounding boxes, not outlines. |
| 61l Boundary replaces the margin | `inlay_margin_ignored`; the male's cuts inside the square; the end mill at the square less 1.5875 | `boundary.rs::boundary_replaces_margin_and_rounding`, `boundary_male_cuts_stay_inside_the_band`, `boundary_edge_pass_sweeps_to_the_boundary`, `boundary_v_wall_without_clearing`, `boundary_female_is_unchanged`; `validate::margin_with_a_boundary_warns_ignored` | **Partial:** no assertion found for "its walls vertical", for "the square's corners are cut with that radius", or for "no V-bit cut comes within 2.887 mm of the square". The end mill's path is asserted as its greatest reach, 38.4125 ± 0.01. |
| 61m Mirrored with the design | The frame mirrored about X = 55; the rooms swap sides | `boundary.rs::boundary_is_mirrored_with_the_design`; `cli.rs::preview_draws_the_boundary_on_the_male_board` | **Partial:** the axis and the band's mirrored bounds are asserted. No assertion found for the 5.3 and 11.3 mm rooms or their swap, nor that `check` prints `inlay_male_mirrored` at X = 55 for the moved frame. |
| 61n Boundary checks | Six codes, each at `operations[i].boundary`; no boundary is byte for byte as before | `validate::boundary_crossing_the_design_is_error_with_the_point`, `boundary_touching_the_design_is_error`, `boundary_open_profile_is_error`, `boundary_that_is_a_target_is_error`, `boundary_unknown_instance_is_error`, `boundary_with_no_design_inside_warns`, `boundary_hole_warns`; `dc-geometry/src/carve.rs::drawn_band_drops_holes`; `boundary.rs::no_boundary_is_byte_for_byte_unchanged` | **Partial:** every code, severity and location is asserted, on a 10 mm square design, not the star. No test found for a boundary wholly smaller than the design, for "a warning that does not stop export", or for the generator cutting through an ignored hole (the geometry layer has it). |
| 61o Several boundaries | Two squares make one band; one square alone is an error; one oval round both | `boundary.rs::several_boundaries_make_one_band`; `validate::several_boundaries_are_a_union` | **Partial:** "less the stars" is not asserted: the band is built with an empty design. |
| 61p Cut-out and the board's edge | The cut-out 1.5875 inside the square; `inlay_cut_out_no_room` naming 5.484; clipped to the board | `boundary.rs::boundary_cut_out_hugs_the_edge`, `boundary_cut_out_no_room_names_5_484`, `boundary_clipped_to_the_board_warns_and_stays_on_it`; `validate::cut_out_room_reads_the_narrowest_band` | **Partial:** no assertion found for "the freed plug's outline is the square inset by 3.175 mm"; it follows by arithmetic from the centre path. |
| 61q Format 26 | A format 25 project migrates with `boundary: []` and exports the same; a boundary round-trips; Photo is 27 | `dc-project/src/migrate/tests.rs::every_older_sample_fixture_loads_as_the_sample_project` (row 25), `v25_to_v26_adds_an_empty_boundary`, `a_v25_project_exports_the_same_programs`, `version_26_fixtures_load`; `dc-project/src/serialize/tests.rs::inlay_boundary_round_trips`, `photo_v_carve_is_format_27`; `cli.rs::v25_project_exports_the_same_programs` | Covered. |
| 61r Safety re-derives | The old program fails after the boundary moves; no generator is called | `dc-gcode/src/safety/tests/boundary.rs::moved_boundary_fails_the_old_program`, `boundary_band_is_derived_from_the_instance`, `cut_out_off_the_boundary_ring_fails`, `boundary_that_no_longer_resolves_refuses_the_old_program`; `dc-gcode/src/safety/tests/s17.rs::s17_uses_no_generator` | **Partial:** no single program fails both rules. S17 fails the V-wall male's V-bit run; S9 fails the open-edge male's clearing and cut-out runs, where the test requires no S17 failure. The fixture is two rectangles with a 90° bit. |
| 61s Bench stand-in | The compass prepared, inlaid in an oval, safe, seated, 3 mm to spare | `cli.rs::line_art_to_inlay_end_to_end`, `compass_inlay_exports_both_boards` | Covered by the first. The second starts from the committed `compass-regions.svg`. Cut end points are held 3 mm inside; arc interiors are not sampled. |

## Acceptance tests 61t to 61z (sub-phase 3.6)

| # | Asserts | Covering tests | Status and notes |
| --- | --- | --- | --- |
| 61t The tool and format 28 | The saved form; the derived values; the range checks; `tool_value_unset`; library version 3 | `dc-project/src/serialize/tests.rs::tapered_ball_nose_round_trips`, `tapered_ball_nose_is_format_28`; `dc-project/src/migrate/tests.rs::v27_file_with_a_tapered_ball_nose_fails`, `version_27_fixture_loads`; `dc-geometry/src/profile/tests.rs::bench_bits_derived_values`; `dc-project/src/tool.rs::cut_radius_is_capped_by_the_cutting_diameter`, `cutting_height_is_the_lower_of_flutes_and_cap`; `validate/tests.rs::tapered_ball_nose_taper_out_of_range_is_error`, `tapered_ball_nose_flute_must_pass_the_ball`, `cut_diameter_must_exceed_the_tip`, `unset_tapered_value_on_a_used_tool_is_tool_value_unset`; `dc-library/src/library.rs::version_2_library_loads_as_version_3`; `dreamcarve/tests/cli/tapered.rs::v27_project_exports_the_same_programs`, `check_lists_unset_tapered_values` | Covered. The unset value is located by the tool's id (`tools[t1]…`), not the spec's `tools[0]…`. |
| 61u Depth by the profile | Z −3.500 and −0.146; 18.40 exports; 22.65 refused; S17 exact | `vcarve/tests/tapered.rs::tapered_bar_centreline_is_the_inverse_width`, `tapered_hairline_is_cut_by_the_ball`, `tapered_corners_are_reached_at_the_surface`, `tapered_flute_length_bounds_the_engagement`, `tapered_cut_diameter_bounds_the_engagement`; `dc-gcode/src/safety/tests/tapered.rs::generated_tapered_carves_pass_s17` | Covered. |
| 61v Floor, clearing and offset | The 27.497 × 7.497 floor; the fillet; a 0.116 ridge that `check` prints; offset rings | `vcarve/tests/tapered.rs::tapered_floor_inset_is_the_width_at_the_flat_depth`, `tapered_floor_has_the_ball_fillet`, `tapered_floor_ridge_is_the_ball_scallop`, `tapered_floor_covers_the_floor_region`, `tapered_clearing_pass_takes_the_floor_inset`, `tapered_offset_rings_follow_the_profile` | **Partial:** no assertion found for "`check` says so": the 0.116 mm ridge is asserted on the generator's value, and `check`'s ridge line is tested for a V-bit only. The offset square is carved with an added `flat_depth_mm` 10 the spec does not give. |
| 61w Corners | `tapered_corners_softened` once, 4 corners, 0.378; the 0.156 and 0.469 setback | `vcarve/tests/tapered.rs::tapered_corners_softened_warns_once`, `tapered_corner_setback_matches_the_formula` | **Partial:** no assertion found for "the outline at Z 0 is the drawn one". The setback is measured on a square's corner through the exact surface function, not on a heightfield of a wedge. |
| 61x Inlay fit | Female equals a V-Carve; walls mate within 0.02; the seat within 0.05; the seat line; the two ball warnings | `inlay/tests/tapered.rs::tapered_female_equals_a_flat_depth_v_carve`, `tapered_walls_mate`, `tapered_plug_seats_flush`, `glue_gap_below_the_ball_warns`, `plug_depth_below_the_ball_warns`, `tapered_male_engagement_is_held_to_the_flute_length`; `dc-project/src/inlay/tapered/tests.rs::seat_line_reports_the_sensitivity`; `dc-gcode/src/safety/tests/tapered_inlay.rs::generated_tapered_inlays_pass_s17_and_s9`; `dreamcarve/tests/cli/tapered.rs::check_prints_the_seat_line` | **Partial:** every clause is asserted, none on the spec's fixture (the star rounded 2 mm), which no test uses. The seat is asserted at `glue_gap_mm` 1.0, not 1.5. The seat line's 0.40 and 0.76 are asserted on the function; `check` printing it is asserted for `depth_mm` 6 (0.55 and 0.76). |
| 61y Rounding for the bit | Tips to 0.731 on both boards; the warning; the two refusals; a V-bit inlay unchanged | `inlay/tests/tapered.rs::sharp_point_is_rounded_on_both_boards`, `narrow_notch_is_closed_on_both_boards`, `part_too_narrow_for_the_bit_is_an_error`, `gap_too_narrow_for_the_bit_is_an_error`, `v_bit_inlay_is_not_rounded`; `dc-project/src/inlay/tapered/tests.rs::rounded_design_is_open_and_closed`, `topology_changes_are_refused`; `dc-gcode/src/safety/tests/tapered_inlay.rs::tapered_inlay_regions_are_the_rounded_design` | **Partial:** "the same Inlay with the 60° V-bit" is asserted on the "DC" fixture, not the star. Everything else is asserted as written. |
| 61z Fusion and the refusals | A Fusion tapered mill imports; missing values unset; no shaft diameter skipped; Photo and countersink refuse; S17 between the old samples | `dc-library/src/fusion.rs::tapered_ball_mill_imports_as_tapered_ball_nose`, `tapered_mill_with_missing_fields_imports_them_unset`, `tapered_mill_without_a_shaft_diameter_is_skipped`, `tapered_mill_with_a_flat_or_bull_tip_is_skipped`; `validate/tests.rs::tapered_ball_nose_on_photo_is_v_carve_needs_v_bit`, `tapered_ball_nose_countersink_is_error`; `dreamcarve/tests/cli/tapered.rs::photo_with_a_tapered_ball_nose_is_refused`; `dc-gcode/src/safety/tests/tapered.rs::tapered_ball_zone_overlap_between_samples_is_refused` | Covered. |

## Clauses with no evidence found

No acceptance test is without tests. These clauses of the partial rows have
no assertion that this audit could find:

| # | Clause |
| --- | --- |
| 51 | At a 0.4 step-over the boundary trace alone is 40 % of the unassisted floor cutting. |
| 55 | A countersink with `dwell_s: 0.5` writes a `G4` in the program. |
| 55 | With `start_depth_mm: 0` the mouth would open 3 mm above the pocket floor. |
| 61 | `check --preview` draws the sign's floor rings, serif lines, two countersinks and tabbed outline. |
| 61d | Output piece order and ids follow position. |
| 61j | The prepared output is the same whatever the project it is later used in. |
| 61l | The end mill's walls on the boundary are vertical. |
| 61l | The square's corners are cut with the end mill's radius. |
| 61l | No V-bit cut comes within 2.887 mm of the square. |
| 61m | The rooms of 5.3 and 11.3 mm swap sides on the male. |
| 61m | `check` prints `inlay_male_mirrored` at X = 55 mm for the moved frame. |
| 61n | A boundary smaller than the star is `inlay_boundary_crosses_design`. |
| 61n | `inlay_boundary_empty` does not stop export. |
| 61n | The band is cut through an ignored boundary hole (asserted in `dc-geometry` only). |
| 61o | The band of two boundaries is the two squares less the stars. |
| 61p | The freed plug's outline is the square inset by 3.175 mm. |
| 61r | One male program fails both S17 and S9 after the boundary moves. |
| 61v | `check` prints the tapered ball nose's 0.116 mm floor ridge. |
| 61w | With a tapered ball nose the outline at Z 0 is the drawn one. |
| 61x | The star rounded 2 mm seats at `glue_gap_mm` 1.5 (the spec's fixture is in no test). |

### Spec-named tests that do not exist

The design spec's test plan names these; no test has the name.

| Name in the test plan | What exists |
| --- | --- |
| `order::clearing_pockets_are_not_containing_pockets` | **No test found.** |
| `order::inlay_is_exempt` | **No test found.** The rule is in comments at `dc-cam/src/order.rs` lines 194 and 216. |
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
what the format added.

| Sub-phase | Format | What it added | Committed fixture | Holds the new content? |
| --- | --- | --- | --- | --- |
| 3.1 V-Carve | 20 | `v_carve` | `dcv/v20/sample.dcv`; `tests/golden/vcarve-*/dreamcarve.dcv` (eight) | Yes, the goldens |
| 3.2 Countersink | 21 | the `countersink` Drill cycle | `dcv/v21/sample.dcv` | **No.** The sample has no countersink; `cli.rs::v21_project_exports_the_same_program` and `migrate/tests.rs::v21_to_v22_bumps_the_version_only` build one in the test. |
| 3.3 Inlay | 22 | `inlay` | `dcv/v22/sample.dcv` | **No.** The sample has no Inlay; `cli.rs::v22_project_exports_the_same_program` builds one in the test. |
| 3.3 Inlay | 23 | `cut_out` | `dcv/v23/sample.dcv`; `dreamcarve/tests/fixtures/inlay-migration/star-female.dcv`, `star-male.dcv` | Yes |
| 3.3 Inlay | 24 | `outline_rounding_mm` | `dcv/v24/sample.dcv`; `inlay-migration/dc-female.dcv`, `dc-male.dcv` | Yes |
| 3.3 Inlay | 25 | `male_board`, one file for both boards | `dcv/v25/sample.dcv` | **No.** The sample has no Inlay; `cli.rs::v25_project_exports_the_same_programs` and `migrate/tests.rs::a_v25_project_exports_the_same_programs` write version 25 text in the test. |
| 3.4 Image Prep | 26 | `boundary` | `dcv/v26/sample.dcv`, `dcv/v26/inlay-boundary.dcv` | Yes |
| 3.5 Photo V-Carve | 27 | `bitmaps`, `photo_v_carve` | `dcv/v27/sample.dcv`, `dcv/v27/photo.dcv`; `tests/golden/photo-raster-*/dreamcarve.dcv` | Yes |
| 3.6 Tapered Ball Nose | 28 (current) | the `tapered_ball_nose` cutter | `dcv/v28/sample.dcv`, `dcv/v28/tapered.dcv`; `tests/fixtures/dc-tbn.dcv`, `tbn-coupon.dcv` and the other current projects | Yes |
| 3.7 Release | none | | | |

The rule is met for every format. Formats 21, 22 and 25 have no committed
file that holds what they added; their migrations are tested on projects
built in the tests.

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
| Warnings: every code of capabilities 48, 49g and 49l | `user-guide/warnings.md` | Present, with one name that differs: the chapter and the code say `vcarve_target_open`, the spec `v_carve_target_open`. |
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
- **Cut records in the functional spec are behind the bench issues.** The
  sign's full cut (#429, closed) has no result line, and the lettered
  pair's record says "not yet cut" though #438 is closed. Only the
  maintainer has the results.
- **`docs/plan.md` says four test parts; the functional spec says five.**
  The plan's "Exit" and "Real cut" predate sub-phase 3.6 and do not name
  the tapered ball nose part, which its own sub-phase list does.
- **Both phase 3 specs still say "Status: draft".**
- **Formats 21, 22 and 25** have no committed fixture holding their new
  content (see "Sub-phases and format versions").
- **The release dry run** (`dist plan`, `dist build --artifacts=local`)
  was not run. The Release workflow last ran green on the `Code-Review`
  branch at `6cf39898` (2026-11-18), before this week's commits.

### Where the spec and the code disagree

None of these changes machine output. Each is a line of spec, a comment or
a message to bring into line.

| Where | The spec says | The code and tests say |
| --- | --- | --- |
| Capability 38 and 48, acceptance 52 | `v_carve_target_open` | `vcarve_target_open` (`dc-cam/src/warning.rs`, and the Warnings chapter) |
| Acceptance 51 and 57 | Runs named `op1 (clearing t2)` and `op1 (cut-out t2)` in programs | Program comments carry the operation's name and no parentheses (`Background clearing t2`); the parenthesised form with the id is in the summary and the preview legend |
| Acceptance 54 | The 6 mm square "matches PureCutCNC's `v_carve` golden" | The only offset golden is a 20 mm square, 60°, step-over 1 mm, as the design spec's corpus lists |
| Acceptance 61 | Each program passes S1 to S19 | The tests' comments say S1 to S18; the sign has no Photo V-Carve, so S19 has nothing to check |
| Acceptance 61b | The output holds none of the clip path's content | The clip group's content is imported unclipped and is in the output |
| Acceptance 61n | `inlay_boundary_crosses_design` names "the nearest point"; each check is at `operations[i].boundary` | The message names the worst point and how far it lies past the boundary; the location carries the operation's id and, per entry, an index (`operations[o7].boundary[1]`) |
| Acceptance 61r | A male program "fails S17 and S9" | S17 fails one male's V-bit run and S9 another's end-mill runs; no one program fails both |
| Acceptance 61t | `tools[0].cutter.tapered_ball_nose.flute_length_mm` | Located by the tool's id, `tools[t1]…`, as capability 49l's own example is (`tools[t3]…`) |
| Acceptance 61v | A 6 mm square in offset mode, no flat depth | The test adds `flat_depth_mm` 10; without it the carve would pass the flutes |
| Acceptance 61w | A heightfield of a 90° wedge | A square's corner, measured on the exact surface |
| Acceptance 61x | The star rounded 2 mm, `glue_gap_mm` 1.5 | The bit-rounded star at `glue_gap_mm` 1.0 |
| S17 with a tapered ball nose | A tapered ball nose is named where it applies | `ExportError::ConeOffMaterial` and `ConeHeightExceeded` say "V-bit" and "cone", and `check`'s ridge line says "V-bit floor ridges", whatever the bit |
| Design spec, test plan | The names under "Spec-named tests that do not exist" | Renamed or folded into tables; two have no test |
| Comments in tests | `cli.rs::line_art_to_inlay_end_to_end` says the spec's 61k names `ring-cross.svg`; `compass_inlay_exports_both_boards` says the stand-in is used "until that command is built"; `dc-gcode/tests/stress/main.rs` lines 45 to 48 say the sweep "still fails" while #484 is open | The spec's 61k names `compass-art.svg`; the command is built; #484 is closed |

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
2. **Fill in the two stale cut records**: the sign's full cut (#429) and
   the inlay pair (#438).
3. **Decide the partial rows and the open items**: accept each, or name
   the ones to close before the tag. The two test-plan names with no test
   (`order::clearing_pockets_are_not_containing_pockets`,
   `order::inlay_is_exempt`) and the `Narrowest gap` line (B4) are the
   ones this audit would look at first.
4. **Tag `v0.3.0`**, per `docs/plan.md`, "Versioning": set the workspace
   version to `0.3.0` (it is `0.2.1`), rename the changelog's
   `[Unreleased]` to `[0.3.0] - <date>` with a fresh empty section above
   it, tag the merge commit, check that the release has its binaries, then
   bump to `0.3.1`.

For an agent, before the tag, once `alpha` is merged or pushed for CI:

1. One green run on Ubuntu, macOS and Windows.
2. The release dry run: `dist plan`, then `dist build --artifacts=local`.
3. The spec lines in "Where the spec and the code disagree", in one
   commit.
