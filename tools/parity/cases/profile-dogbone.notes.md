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
- Intentional differences: none in the motion. The corners are visited in
  a different order (DreamCarve nearest-first from the last exit), which
  a set comparison does not see.
