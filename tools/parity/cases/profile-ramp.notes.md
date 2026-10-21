- Tolerance: `crates/dc-gcode/tests/golden.rs` (`profile_ramp`) compares
  the lap at each Z level within 0.01 mm plus rounding, wherever it
  starts. The ramp is checked, not compared (`Difference::EntryPlacement`).
- DreamCarve project: `tests/golden/profile-ramp/dreamcarve.dcv`.
  150 × 100 × 12 mm stock, origin at the bottom-left corner and the stock
  top; a 100 × 50 mm rectangle at (25, 25), outside Profile 12 mm deep in
  4 mm passes, climb, `entry: { ramp: { angle_deg: 15 } }`. In the
  `.camj`: `edge_route_outside`, `entryStrategy: ramp`,
  `entryRampAngle: 15`.
- **Intentional difference (#165, #174): the ramp is placed differently.**
  DreamCarve's zigzag has one end on the lap's entry point and runs into
  the removed material from there; on later passes it ramps down from the
  previous pass's exit without retracting. PureCutCNC centres its zigzag
  elsewhere in the removed material, ramps from safe Z on every pass, and
  joins the lap with a flat move. So the sloped moves are not compared,
  and neither are the flat moves that lie wholly within a tool radius of a
  ramp (its tail on the level). The rest of each level must lie on the
  other program's moves both ways, and the laps must have the same length.
  Instead, both programs' ramps must descend no steeper than 15°, and
  DreamCarve's must start on the path it enters.
