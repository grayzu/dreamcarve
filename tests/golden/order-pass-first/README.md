# Golden case: order-pass-first

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case order-pass-first --post grblhal`.

- Source: `tools/parity/cases/order-pass-first.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (edge_route_inside), `op2` (edge_route_outside)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`order_pass_first`)
  compares **in order**, pass by pass: the passes (consecutive runs of one
  operation at one Z) must come in the same order with the same number of
  shapes, and each pass's moves must match as a set, within 0.01 mm plus
  rounding.
- DreamCarve project: `tests/golden/order-pass-first/dreamcarve.dcv`. The
  `order-full-depth-first` parts and operations, with
  `machining_order: pass_first`. In the `.camj`:
  `machiningOrder: level_first`.
- Both programs cut every hole's first pass before any hole's second, and
  likewise for the squares: passes at Z −4, −8 and −12, three shapes each.
- **Intentional difference: the order of shapes within a pass.**
  DreamCarve walks each pass nearest-first from the previous pass's last
  exit, so it goes left to right, then right to left, then left to right
  (D5, `docs/specs/phase-2/design.md`, "Sequence", step 3). PureCutCNC's
  `level_first` routes the outside outlines in drawing order at every
  level. Its inside walk starts at the middle hole. The test checks
  DreamCarve's walk against D5: each shape of a pass starts nearer the
  previous shape's end than any shape still to come in that pass
  (`Difference::PassOrderWithinLayer`).

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
