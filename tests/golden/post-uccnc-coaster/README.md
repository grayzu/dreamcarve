# Golden case: post-uccnc-coaster

The quick-start coaster (`fixtures/coaster.svg`: a 90 mm circle on 100 mm
stock, 11.8 mm thick, cut through in 4 mm passes at feed 1500, plunge 500,
18000 RPM) exported with the bundled `uccnc` post-processor, version 1.

- Recaptured in slice 10 (#165) when `init` began writing a 15° ramp
  entry (#119): each pass now starts with the ramp, the laps are
  unchanged.
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
