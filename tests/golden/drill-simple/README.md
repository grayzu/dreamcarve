# Golden case: drill-simple

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case drill-simple --post grblhal`.

- Source: `tools/parity/cases/drill-simple.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (drilling)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`drill_simple`) compares
  the holes in drilling order: each at the same XY within 0.01 mm plus
  rounding, with plunges ending at the same Zs, at the same feeds. Only
  motion is compared.
- DreamCarve project: `tests/golden/drill-simple/dreamcarve.dcv`.
  150 × 100 × 12 mm stock, origin at the bottom-left corner and the stock
  top; four 6 mm circles drawn at (20, 50), (95, 65), (130, 45) and
  (55, 30), Y-up. One Drill, `simple`, 8 mm deep, retract 1 mm, safe Z
  5 mm, with a 5 mm drill (feed 1500, plunge 300, 12000 RPM). In the
  `.camj` the circles are `subtract` features from Z 12 to 4 and the
  operation is `drilling` with `drillType: simple`, `retractHeight: 1`.
- Both programs rapid to safe Z over each hole, rapid down to the retract
  plane 1 mm above the stock, plunge at the plunge rate, and rapid back to
  safe Z (#138, decision 1: each hole ends at safe Z).
- Order: DreamCarve drills nearest-first from the first circle drawn
  (#138); PureCutCNC's walk starts elsewhere. The holes are placed so that
  both give (20, 50), (55, 30), (95, 65), (130, 45).
- Intentional differences: none in the motion.

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
