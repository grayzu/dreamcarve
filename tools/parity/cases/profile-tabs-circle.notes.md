- Tolerance: `crates/dc-gcode/tests/golden.rs` (`profile_tabs_circle`)
  compares the cutting moves at each Z level within 0.01 mm, plus the
  rounding of each program's three decimal places, wherever each loop
  starts; feeds must be equal except as listed below. Only motion is
  compared: header, tool-change and spindle lines differ by design.
- DreamCarve project: the coaster from the phase 1 functional spec.
  120 × 120 × 12 mm stock, origin at the bottom-left corner (Y-up) and the
  stock top; a 90 mm circle centred at (60, 60), outside, climb, 12 mm deep
  in 4 mm passes, four rect tabs 8 mm wide and 3 mm tall (`Count(4)`). The
  circle is placed rotated 45°, so its tool-centre path starts on a
  diagonal and the tab centres, an eighth of the path from the start and a
  quarter apart, fall on the axes through the centre.
- PureCutCNC outline: a 360-sided polygon with its vertices on the 90 mm
  circle, centred at (60, 60) Y-down with the origin at (0, 120). A
  `circle` feature is sampled every 10°, and where the tabs split the path
  the split points sit up to 0.034 mm inside the true circle, between the
  arcs the post-processor refits. The polygon keeps them within 0.002 mm.
- PureCutCNC tabs: four world rectangles 7.947 mm along the path by 12 mm
  across it, centred on the 48.175 mm radius tool-centre path at its
  intersections with the axes through the centre, `z_top` 3.
- Intentional difference, tab placement: DreamCarve places tabs along the
  tool-centre path, raising `width + diameter` = 14.35 mm of path per tab.
  PureCutCNC raises the part of the path inside each rectangle grown by the
  tool radius, which on this curve is 0.054 mm longer than
  `width + diameter`. The 7.947 mm width is
  `2 × (48.175 × sin(7.175 / 48.175) − 3.175)`: the width whose raised span
  on this path is 14.35 mm, as DreamCarve's 8 mm tab raises.
- Intentional difference, tab step feeds: DreamCarve steps onto and off a
  rect tab with vertical moves at the plunge rate (500); PureCutCNC makes
  both steps at the cutting feed (1500). The harness checks each program
  against its own rule (`Difference::RectTabStepsAtPlungeRate`).
- PureCutCNC's lap starts inside a tab, so its final pass enters at the tab
  top. That is a different start point on the same moves.
