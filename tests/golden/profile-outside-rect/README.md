# Golden case: profile-outside-rect

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case profile-outside-rect --post grblhal`.

- Source: `tools/parity/cases/profile-outside-rect.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (edge_route_outside)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`profile_outside_rect`)
  compares the cutting moves at each Z level within 0.01 mm, plus the
  rounding of each program's three decimal places, wherever each loop
  starts; feeds must be equal. Only motion is compared: header, tool-change
  and spindle lines differ by design.
- DreamCarve project: 150 × 100 × 12 mm stock, origin at the bottom-left
  corner (Y-up) and the stock top; a 100 × 50 mm rectangle with its
  lower-left corner at (20, 30). The `.camj` places it at (20, 20) Y-down
  with the origin at (0, 100), so both programs share one machine frame.
- Outside, climb, 12 mm deep in 4 mm passes; 6.35 mm tool, feed 1500,
  plunge 500, safe Z 5 mm above the stock top.
- PureCutCNC enters partway round a corner and closes each lap with a
  straight chord over the last 9° of that corner's arc. Clipper spaces
  round-join points so that each chord sags exactly PureCutCNC's 0.01 mm
  arc tolerance; once the chord's ends are rounded to three decimals it
  sags 0.01003 mm, which is why the comparison allows for rounding.
- Intentional differences: none in this case.

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
