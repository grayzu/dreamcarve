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
- Intentional difference (#146, decision 5; #150): PureCutCNC orders the
  lines greedily from the wall pass's end; DreamCarve orders them
  nearest-first among the lines a link inside the region reaches, and
  starts its wall ring elsewhere. The two programs can therefore visit
  the lines in different orders and link different pairs of them. The
  links are not compared; the lines are, without direction, as the
  design spec says raster lines ignore it.
- The scan starts half a step-over below the top of the region: PureCutCNC
  scans from `minY` of its Y-down frame, the top in the machine frame
  (#195). Before that fix DreamCarve scanned from the bottom and its lines
  sat 0.63 mm from PureCutCNC's.

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
