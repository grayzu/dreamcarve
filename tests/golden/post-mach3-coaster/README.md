# Golden case: post-mach3-coaster

The quick-start coaster (`fixtures/coaster.svg`: a 90 mm circle on 100 mm
stock, 11.8 mm thick, cut through in 4 mm passes at feed 1500, plunge 500,
18000 RPM) exported with the bundled `mach3` post-processor, version 2.

- Recaptured in #286 for version 2, whose header has no `G49`
  (`N30 G90 G94 G17 G40 G80`): a Z zero set with a tool length offset
  active includes it, so cancelling it moved every Z. Only that line
  changed. The version 1 program is kept as
  `crates/dreamcarve/tests/expected/coaster-mach3-v1.tap`, which a version
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
  `dreamcarve export coaster.dcv --post mach3 --date 2026-10-15`, after
  `mach3/v1.json` was checked line by line against *Using Mach3Mill* 1.84
  and the Mach3 configuration guide
  (`crates/dc-project/src/definitions/README.md`, "Mach3"). It is not
  compared with PureCutCNC: the design spec ("Test plan → golden")
  compares these cases with the verified definitions, because DreamCarve
  changed PureCutCNC's Mach3 definition (no `O0001`, `%` without a line
  number, no `G43 H`, no `M6`, `G91.1`, `M5` before `M30`, the dwell).
- Test: `golden::post_mach3_coaster` in `crates/dreamcarve/tests/cli.rs`.
  The export must equal `program.tap` byte for byte, and its motion, read
  back, must equal the grblHAL coaster's
  (`crates/dreamcarve/tests/expected/coaster-grblhal.nc`, functional spec
  acceptance 36).
- Regenerate only when a change to it is intended, and say why in the
  commit. A change to the definition is a new version file, not an edit.
