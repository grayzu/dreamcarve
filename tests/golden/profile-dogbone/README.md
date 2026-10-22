# Golden case: profile-dogbone

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case profile-dogbone --post grblhal`.

- Source: `tools/parity/cases/profile-dogbone.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (edge_route_inside)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`profile_dogbone`)
  compares the cutting moves at each Z level within 0.01 mm plus rounding,
  wherever each loop starts. A notch is cut out and back along one line,
  so a move may match either of the two that lie there. Feeds must be
  equal.
- DreamCarve project: `tests/golden/profile-dogbone/dreamcarve.dcv`.
  150 × 100 × 12 mm stock, origin at the bottom-left corner and the stock
  top; a 40 × 30 mm rectangle at (55, 35), inside Profile 6 mm deep in
  3 mm passes, climb, `corner_relief: dogbone`. In the `.camj`: a
  `subtract` rectangle from Z 12 to 6, `edge_route_inside`,
  `cornerRelief: dogbone`.
- Both programs cut the four notches after the laps, along each corner's
  bisector to one tool radius past the corner (#181), at the tool's own
  step-down levels (4 mm: Z −4 and −6), not the operation's.
- Intentional differences: the corners are visited in a different order
  (DreamCarve nearest-first from the last exit), which a set comparison
  does not see. The notch at the lap's start and end is reached from the
  lap (#240): DreamCarve moves up from Z −6 to Z −4 at the plunge rate in
  the column the lap cut, where PureCutCNC retracts to safe Z and plunges
  (`NotchRisesFromLapEnd`).

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
