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
