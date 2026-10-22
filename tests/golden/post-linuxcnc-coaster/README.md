# Golden case: post-linuxcnc-coaster

The quick-start coaster (`fixtures/coaster.svg`: a 90 mm circle on 100 mm
stock, 11.8 mm thick, cut through in 4 mm passes at feed 1500, plunge 500,
18000 RPM) exported with the bundled `linuxcnc` post-processor, version 2.

- Recaptured in #286 for version 2, whose header has no `G49`
  (`G90 G17 G40 G80 G94`): a Z zero set with a tool length offset active
  includes it, so cancelling it moved every Z. Only that line changed. The
  version 1 program is kept as
  `crates/dreamcarve/tests/expected/coaster-linuxcnc-v1.ngc`, which a
  version 1 project must still export
  (`cli::linuxcnc_v1_project_still_exports_byte_identically`).
- Recaptured in slice 10 (#165) when `init` began writing a 15° ramp
  entry (#119): each pass now starts with the ramp, the laps are
  unchanged.
- Recaptured in #70 when `init` began importing the `<circle>` as two
  exact half-circle arcs instead of 160 flattened edges: each untabbed
  lap is two arcs on the true tool-centre circle (radius 48.175 mm), not
  seven fitted ones, and the cut starts at the circle's +X point.
- Captured from DreamCarve itself (slice 17, #164), with
  `dreamcarve export coaster.dcv --post linuxcnc --date 2026-10-15`, after
  `linuxcnc/v1.json` was checked line by line against the LinuxCNC
  documentation and source (`crates/dc-project/src/definitions/README.md`,
  "LinuxCNC"). It is not compared with PureCutCNC: the design spec ("Test
  plan → golden") compares these cases with the verified definitions,
  because DreamCarve changed PureCutCNC's LinuxCNC definition (no `G43 H`,
  no `M6`, `G91.1`, `M5` before `M2`, the dwell).
- Test: `golden::post_linuxcnc_coaster` in `crates/dreamcarve/tests/cli.rs`.
  The export must equal `program.ngc` byte for byte, and its motion, read
  back, must equal the grblHAL coaster's
  (`crates/dreamcarve/tests/expected/coaster-grblhal.nc`, functional spec
  acceptance 36).
- Regenerate only when a change to it is intended, and say why in the
  commit. A change to the definition is a new version file, not an edit.
