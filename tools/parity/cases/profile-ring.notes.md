- Tolerance: `crates/dc-gcode/tests/golden.rs` (`profile_ring`) compares
  the cutting moves at each Z level within 0.01 mm, plus the rounding of
  each program's three decimal places, wherever each loop starts; feeds
  must be equal. Only motion is compared: header, tool-change and spindle
  lines differ by design.
- DreamCarve project: 150 × 100 × 12 mm stock, origin at the bottom-left
  corner (Y-up) and the stock top; a 60 mm and a 30 mm circle, both centred
  at (75, 50), targeted by one outside Profile. DreamCarve nests the small
  circle as a hole of the large one and cuts it on the inside. The `.camj`
  centres both at (75, 50) Y-down with the origin at (0, 100).
- Climb, 12 mm deep in 4 mm passes.
- PureCutCNC routes an outside edge only round `add` features and an
  inside edge only round `subtract` ones, so the same cuts take two
  operations there: `op1` inside the 30 mm `subtract` circle, then `op2`
  outside the 60 mm `add` circle. DreamCarve also cuts the hole first,
  within its one operation. The comparison treats each Z level as a set of
  moves, so the order of the contours is not checked.
- Intentional differences: none in the motion. Setting the case up differs:
  one operation and automatic nesting in DreamCarve, two operations in
  PureCutCNC.
