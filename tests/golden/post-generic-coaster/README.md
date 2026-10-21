# Golden case: post-generic-coaster

The quick-start coaster (`fixtures/coaster.svg`: a 90 mm circle on 100 mm
stock, 11.8 mm thick, cut through in 4 mm passes at feed 1500, plunge 500,
18000 RPM) exported with the bundled `generic` post-processor, version 1.

- Recaptured in slice 10 (#165) when `init` began writing a 15° ramp
  entry (#119): each pass now starts with the ramp, the laps are
  unchanged.
- Recaptured in #70 when `init` began importing the `<circle>` as two
  exact half-circle arcs instead of 160 flattened edges: each untabbed
  lap is two arcs on the true tool-centre circle (radius 48.175 mm), not
  seven fitted ones, and the cut starts at the circle's +X point.
- Captured from DreamCarve itself (slice 17, #164), with
  `dreamcarve export coaster.dcv --post generic --date 2026-10-15`, after
  `generic/v1.json` was checked line by line against the NIST RS274/NGC
  interpreter report, version 3 (NISTIR 6556;
  `crates/dc-project/src/definitions/README.md`, "Generic RS274/NGC"). It
  is not compared with PureCutCNC: the design spec ("Test plan → golden")
  compares these cases with the verified definitions, because DreamCarve
  changed PureCutCNC's generic definition (`G94`, `M5` before `M30`, the
  dwell, a tool change that is never written).
- Numbers have no trailing zeros (`Z5`, `Y55`, `J0`), as acceptance 36 asks.
- Test: `golden::post_generic_coaster` in `crates/dreamcarve/tests/cli.rs`.
  The export must equal `program.nc` byte for byte, and its motion, read
  back, must equal the grblHAL coaster's
  (`crates/dreamcarve/tests/expected/coaster-grblhal.nc`, functional spec
  acceptance 36).
- Regenerate only when a change to it is intended, and say why in the
  commit. A change to the definition is a new version file, not an edit.
