# Golden case: profile-inside-circle

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case profile-inside-circle --post grblhal`.

- Source: `tools/parity/cases/profile-inside-circle.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (edge_route_inside)
- Header date frozen to 2026-01-01 by the capture script.
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

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
