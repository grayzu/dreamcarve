# Golden case: profile-helix-inside

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case profile-helix-inside --post grblhal`.

- Source: `tools/parity/cases/profile-helix-inside.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (edge_route_inside)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`profile_helix_inside`)
  compares the lap at each Z level within 0.01 mm plus rounding, wherever
  it starts. The helix is checked, not compared
  (`Difference::EntryPlacement`).
- DreamCarve project: `tests/golden/profile-helix-inside/dreamcarve.dcv`.
  150 × 100 × 12 mm stock, origin at the bottom-left corner and the stock
  top; a 40 mm circle at (75, 50), inside Profile 12 mm deep in 4 mm
  passes, climb, `entry: { helix: { diameter_fraction: 0.5, angle_deg:
  10 } }`. In the `.camj`: a `subtract` circle from Z 12 to 0,
  `edge_route_inside`, `entryStrategy: helix`,
  `entryHelixDiameterPercent: 50`, `entryRampAngle: 10`.
- Both programs helix counter-clockwise (climb inside a wall, Y-up) on a
  3.175 mm circle at 10°, ending with one flat turn. The test checks this
  in both: the radius, the turning sense, and the angle.
- **Intentional difference (#147, #165, #174): the helix is placed
  differently.** DreamCarve places it at its lap's start, and on later
  passes rejoins the bore from the previous pass. PureCutCNC places it
  near the top of the circle and helixes from safe Z on every pass. So the
  sloped moves are not compared, and neither are the flat moves wholly
  within a tool radius of a helix: its flat turn and its links to and from
  the lap.

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
