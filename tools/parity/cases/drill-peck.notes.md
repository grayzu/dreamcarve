- Tolerance: `crates/dc-gcode/tests/golden.rs` (`drill_peck`) compares the
  holes in drilling order: positions within 0.01 mm plus rounding, and
  depths. It does not compare peck depths; see below.
- DreamCarve project: `tests/golden/drill-peck/dreamcarve.dcv`. The
  `drill-simple` holes, 12 mm deep (through), `peck` with
  `peck_depth_mm: 4` and full retracts, retract 1 mm. In the `.camj` the
  circles go from Z 12 to 0 and the operation is `drillType: peck`,
  `peckDepth: 4`.
- **Intentional difference (#153, PR #190): pecks count from the retract
  plane.** DreamCarve's peck k ends at `max(bottom, R − k·peck)`, as
  `G83` does on every controller: with R at Z 1, the pecks end at Z −3,
  −7, −11 and −12. PureCutCNC counts from the stock top: −4, −8, −12. The
  test checks each program against its own rule instead of comparing them.
- Between pecks DreamCarve retracts to the retract plane and PureCutCNC to
  safe Z; both rapid back down to 0.5 mm above the last peck. Retracts are
  rapids and are not compared.
