# Golden case: pocket-raster-45deg

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case pocket-raster-45deg --post grblhal`.

- Source: `tools/parity/cases/pocket-raster-45deg.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (pocket)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`pocket_raster_45deg`)
  compares as `pocket-raster-0deg`: wall pass rings as sets, raster lines
  as sets without direction, within 0.01 mm plus rounding.
- DreamCarve project: `tests/golden/pocket-raster-45deg/dreamcarve.dcv`.
  The `pocket-offset-rect` rectangle, raster Pocket at 45° (lines along
  (1, 1) in the machine frame), 6 mm deep in 3 mm passes, wall pass first.
- **The `.camj` angle is −45.** PureCutCNC rotates the region by
  −`pocketAngle` in its Y-down frame. Flipping Y mirrors the angle, so
  PureCutCNC's +45° is DreamCarve's 135° and DreamCarve's 45° is
  PureCutCNC's −45° (#146, decision 2; `docs/specs/phase-2/design.md`,
  "Algorithms → Pocket", step 7).
- Intentional difference: as `pocket-raster-0deg` (#146, decision 5). Here
  PureCutCNC also links two lines across a corner where DreamCarve links
  along the wall; links are not compared.
- Known bug: #195, as `pocket-raster-0deg`. The lines are offset by
  0.77 mm across their direction. The test is ignored until that is
  fixed; with the scan started from the top, it passes.

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
