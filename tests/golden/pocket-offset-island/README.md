# Golden case: pocket-offset-island

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case pocket-offset-island --post grblhal`.

- Source: `tools/parity/cases/pocket-offset-island.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (pocket)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`pocket_offset_island`)
  compares the cutting moves at each Z level within 0.01 mm, plus the
  rounding of each program's three decimal places, after start-point
  alignment: rings as sets, links between rings by their lengths. Feeds
  must be equal.
- DreamCarve project: `tests/golden/pocket-offset-island/dreamcarve.dcv`.
  150 × 100 × 12 mm stock, origin at the bottom-left corner and the stock
  top; an 80 × 60 mm rectangle at (20, 20) and a 20 mm circle at (60, 50),
  both targets of one offset Pocket, 6 mm deep in 3 mm passes, so the
  circle is an island. In the `.camj` the rectangle is a `subtract` and
  the island an `add` feature, both from Z 12 to 6.
- The `.camj` island is a 360-sided polygon on the circle: PureCutCNC
  samples a `circle` segment coarsely, and the offsets of that polygon sit
  up to 0.016 mm inside the true circle's. DreamCarve's island is a true
  circle, as a user would draw it.
- Pocket settings as `pocket-offset-rect`: `roundOutsideCorners`,
  `roundLinkCorners` and `cleanWallCorners` off, no slot-feed reduction.
- Known bug: #196. DreamCarve starts the first ring round the island at
  the circle's vertex nearest the previous ring, which on a two-vertex
  circle is 7.6 mm away; PureCutCNC's polygon ring starts at its nearest
  point, 2.54 mm away. The test is ignored until that is fixed; with the
  island drawn as a polygon in DreamCarve too, it passes.

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
