- Tolerance: `crates/dc-gcode/tests/golden.rs` (`profile_inside_circle`)
  compares the cutting moves at each Z level within 0.01 mm, plus the
  rounding of each program's three decimal places, wherever each loop
  starts; feeds must be equal. Only motion is compared: header, tool-change
  and spindle lines differ by design.
- DreamCarve project: 150 × 100 × 12 mm stock, origin at the bottom-left
  corner (Y-up) and the stock top; a 60 mm circle centred at (70, 55). The
  `.camj` centres it at (70, 45) Y-down with the origin at (0, 100), so both
  programs share one machine frame.
- Inside, climb, 10 mm deep in 4 mm passes (Z 8, 4 and 2 above the stock
  bottom; the last pass is the shallow one). PureCutCNC cuts the inside of
  a `subtract` feature whose `z_bottom` is 2.
- PureCutCNC samples the circle every 10° and its post-processor fits arcs
  back through the samples; their radius is within 0.003 mm of the true
  tool-centre radius, 26.825 mm.
- Intentional differences: none in this case.
