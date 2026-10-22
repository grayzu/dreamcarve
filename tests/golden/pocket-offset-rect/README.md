# Golden case: pocket-offset-rect

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case pocket-offset-rect --post grblhal`.

- Source: `tools/parity/cases/pocket-offset-rect.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (pocket)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`pocket_offset_rect`)
  compares the cutting moves at each Z level within 0.01 mm, plus the
  rounding of each program's three decimal places, after start-point
  alignment: the rings as sets, wherever each starts, and the one-move
  links between rings by their lengths, since they move with the rings'
  start corner. Feeds must be equal. Only motion is compared: header,
  tool-change and spindle lines differ by design.
- DreamCarve project: `tests/golden/pocket-offset-rect/dreamcarve.dcv`.
  150 × 100 × 12 mm stock, origin at the bottom-left corner (Y-up) and the
  stock top; a 60 × 40 mm rectangle at (30, 30), offset Pocket 6 mm deep
  in 3 mm passes, step-over 0.4, climb, wall pass last, plunge entry. The
  `.camj` rectangle is a `subtract` feature from Z 12 to 6, its corner at
  (30, 30) Y-down too, since the pocket is centred in Y.
- The `.camj` turns off what PureCutCNC adds by default and DreamCarve
  does not do: `roundOutsideCorners` (rounded inner ring corners),
  `roundLinkCorners` (S-curve links), `cleanWallCorners`, and the slot-feed
  reduction (`pocketSlotFeedPercent: 100`).
- Intentional differences: none in the motion. PureCutCNC starts the
  innermost ring at its lower-right corner, DreamCarve at its lower-left
  (#137, decision 4: each ring starts nearest the previous ring's end,
  from wherever the first starts).

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
