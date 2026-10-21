# Golden case: post-uccnc-coaster

The quick-start coaster (`fixtures/coaster.svg`: a 90 mm circle on 100 mm
stock, 11.8 mm thick, cut through in 4 mm passes at feed 1500, plunge 500,
18000 RPM) exported with the bundled `uccnc` post-processor, version 3.

- Recaptured in #312 for version 3, whose header cancels scaling and
  rotation with `G50` and `G69`, each on a line of its own after the
  modal line: a `G51` or `G68` left set by MDI or an earlier program
  would scale, mirror or rotate every coordinate. Only those two lines
  were added. The version 2 program is kept as
  `crates/dreamcarve/tests/expected/coaster-uccnc-v2.txt`, which a
  version 2 project must still export
  (`cli::uccnc_v2_project_still_exports_byte_identically`).
- Recaptured in #286 for version 2, whose header has no `G49`
  (`G90 G17 G40 G80 G94`): a Z zero set with a tool length offset active
  may include it, so cancelling it could move every Z. Only that line
  changed. The version 1 program is kept as
  `crates/dreamcarve/tests/expected/coaster-uccnc-v1.txt`, which a version
  1 project must still export
  (`cli::mach3_and_uccnc_v1_projects_still_export_byte_identically`).
- Recaptured in slice 10 (#165) when `init` began writing a 15° ramp
  entry (#119): each pass now starts with the ramp, the laps are
  unchanged.
- Recaptured in #70 when `init` began importing the `<circle>` as two
  exact half-circle arcs instead of 160 flattened edges: each untabbed
  lap is two arcs on the true tool-centre circle (radius 48.175 mm), not
  seven fitted ones, and the cut starts at the circle's +X point.
- Captured from DreamCarve itself (slice 17, #164), with
  `dreamcarve export coaster.dcv --post uccnc --date 2026-10-15`, after
  `uccnc/v1.json` was checked line by line against the UCCNC user's manual
  1.0047 (`crates/dc-project/src/definitions/README.md`, "UCCNC"). It is
  not compared with PureCutCNC: the design spec ("Test plan → golden")
  compares these cases with the verified definitions, because DreamCarve
  changed PureCutCNC's UCCNC definition (no `%`, `O0001`, line numbers or
  `G21`, the dwell in milliseconds, no canned cycles, no `G43 H`, no `M6`,
  `M5` before `M30`).
- The program has no units word: UCCNC has none and reads it in its
  profile's units. The export warns `units_set_by_controller`.
- Test: `golden::post_uccnc_coaster` in `crates/dreamcarve/tests/cli.rs`.
  The export must equal `program.txt` byte for byte, and its motion, read
  back, must equal the grblHAL coaster's
  (`crates/dreamcarve/tests/expected/coaster-grblhal.nc`, functional spec
  acceptance 36).
- Regenerate only when a change to it is intended, and say why in the
  commit. A change to the definition is a new version file, not an edit.
