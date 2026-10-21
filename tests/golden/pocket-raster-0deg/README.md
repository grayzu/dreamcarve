# Golden case: pocket-raster-0deg

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case pocket-raster-0deg --post grblhal`.

- Source: `tools/parity/cases/pocket-raster-0deg.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (pocket)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`pocket_raster_0deg`)
  compares the cutting moves at each Z level within 0.01 mm, plus the
  rounding of each program's three decimal places: the wall pass rings as
  sets, the raster lines as sets without their direction. Feeds must be
  equal.
- DreamCarve project: `tests/golden/pocket-raster-0deg/dreamcarve.dcv`.
  The `pocket-offset-rect` rectangle, raster Pocket at 0°, 6 mm deep in
  3 mm passes, step-over 0.4, wall pass first. In the `.camj`:
  `pocketPattern: parallel`, `pocketAngle: 0`.
- PureCutCNC cuts the boundary before the lines on a raster pocket, so the
  DreamCarve project asks for `wall_pass: first` (DreamCarve's default is
  last).
- Intentional difference (#146, decision 5): DreamCarve cuts the lines in
  scan order; PureCutCNC orders them greedily from the wall pass's end. The
  two programs therefore visit the lines in different orders and link
  different pairs of them. The links are not compared; the lines are,
  without direction, as the design spec says raster lines ignore it.
- Known bug: #195. DreamCarve scans from the bottom of the region in its
  Y-up frame; PureCutCNC scans from `minY` of its Y-down frame, the top in
  the machine frame. The lines sit 0.63 mm apart. The test is ignored
  until that is fixed; with the scan started from the top, it passes.

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
