- Tolerance: `crates/dc-gcode/tests/golden.rs` (`order_full_depth_first`)
  compares **in order** (`Order::InOrder`): each run of horizontal cuts at
  one Z, in program order, against the other program's run at the same
  place in its sequence, within 0.01 mm plus rounding. So the order of
  shapes and passes must agree, as well as the moves at each level
  (`docs/specs/phase-2/functional.md`, acceptance 38).
- DreamCarve project: `tests/golden/order-full-depth-first/dreamcarve.dcv`.
  150 × 100 × 12 mm stock, origin at the bottom-left corner and the stock
  top; three 30 × 30 mm squares at x 10, 60 and 110 (y 35), each with a
  12 mm circle at its centre. `o1` cuts the circles on the inside, `o2`
  the squares on the outside, both through in 4 mm passes, climb, with
  `machining_order: full_depth_first`.
- PureCutCNC routes an inside edge only round `subtract` features and an
  outside edge only round `add` ones, so the `.camj` has the same two
  operations: `edge_route_inside` on the holes, then `edge_route_outside`
  on the squares, each with `machiningOrder: feature_first`. DreamCarve
  could cut each part's hole and outline in one operation, but in another
  order. The two-operation form is the one both can express.
- Both cut each hole to full depth, left to right, then each square to
  full depth, left to right.
- Intentional differences: none.
