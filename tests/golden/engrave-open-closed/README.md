# Golden case: engrave-open-closed

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case engrave-open-closed --post grblhal`.

- Source: `tools/parity/cases/engrave-open-closed.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (follow_line)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`engrave_open_closed`)
  compares the cutting moves at each Z level within 0.01 mm, plus
  rounding: the open path and the closed loop as sets, each travelled the
  same way. Feeds must be equal.
- DreamCarve project: `tests/golden/engrave-open-closed/dreamcarve.dcv`.
  150 × 100 × 12 mm stock, origin at the bottom-left corner and the stock
  top; an open polyline through (20, 20), (50, 40), (80, 20) and
  (110, 40), and a closed 60 × 25 mm rectangle drawn (30, 80), (90, 80),
  (90, 55), (30, 55), Y-up. One Engrave, 3 mm deep in 1.5 mm passes,
  `alternate_direction: false`, full depth first. In the `.camj` both are
  `line` features with `kind: polygon`, the operation `follow_line` with
  `carveDepth: 3` and `stepdown: 1.5`.
- Engrave follows the path as drawn, so the DreamCarve rectangle is drawn
  in the same order as the `.camj` one: clockwise in the machine frame,
  from its top-left corner.
- Intentional differences: none in the motion. Between the closed loop's
  passes DreamCarve stays down and plunges to the next level; PureCutCNC
  retracts to safe Z and plunges from there. Both plunge at the plunge
  rate, and rapids are not compared.

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
