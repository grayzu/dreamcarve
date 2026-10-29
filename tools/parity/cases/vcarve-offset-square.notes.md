- Tolerance: `crates/dc-gcode/tests/golden.rs` (`vcarve_offset_square`) compares
  the V-Carve's rings level by level: the levels above the flat depth
  are the same, and at each, every point of every level cut, sampled
  every 0.05 mm, lies within 0.02 mm in XYZ (plus each program's rounding
  of three decimal places) of the other program's level cuts, both ways,
  and travels the same way as a cut there (climb). The rings are
  compared as sets, wherever each starts: upstream starts a level's ring
  nearest the previous ring's end with an 8 × stepover link budget,
  DreamCarve at its vertex nearest the previous ring's end (#407).
- DreamCarve project: `tests/golden/vcarve-offset-square/dreamcarve.dcv`.
  60 × 40 × 12 mm stock, origin at the bottom-left corner (Y-up) and
  the stock top, so a point at PureCutCNC `y` is at DreamCarve
  `40 − y`.
  A 20 × 20 mm square at (15, 12), offset V-Carve, `step_over_mm: 1`,
  climb, with the 6.35 mm 60° V-bit at feed 1000, plunge 300, 18000 RPM, step-over
  0.4 (of the floor's cut width), safe Z 5 mm above the stock top,
  `flat_depth_mm: 4`. In the `.camj` it is a `subtract` rectangle at
  (15, 8) Y-down, `v_carve`, `stepover` 1, `maxCarveDepth` 4.
- What it covers: offset rings 1 mm apart, each `inset / tan 30°` deep:
  1.732 and 3.464 mm in both programs. Upstream's next ring would be
  5.196 mm deep, past the clamp, so it stops, never reaching 4 mm:
  the 3.464 mm ring's flanks reach 2 mm either side, and the 12 mm
  square in the middle stays at the surface. DreamCarve's rings stop
  short of `F · tan 30°` = 2.309 mm in, and its floor cuts everything
  inside flat at 4 mm (#405).
- Not compared, and why:
  - **The flat depth.** DreamCarve's ring at the flat depth is its floor
    trace, and inside it the V-bit floor (#405, #407); PureCutCNC has no
    floor. They are DreamCarve-only. Every golden cut at the clamp (there
    are none here) would have to lie within the V-bit's flat-depth radius
    of a DreamCarve cut at that depth.
  - **Links** between rings, which descend from one level to the next:
    they follow each ring's start point, which differs (#407). Plunges
    and lifts are not compared point for point; their feed is the plunge
    rate in both programs.
- Intentional difference, `DescentsHeldToPlunge`: DreamCarve's ring links,
  which descend while they move in XY, are held to the plunge rate (S10);
  PureCutCNC's run at the cutting feed. Each program's feeds are checked
  against its own rule.
- The `.camj` V-bit is 6 mm across, PureCutCNC's test bit; DreamCarve's
  is 6.35 mm, the bench bit (`docs/specs/phase-3/design.md`, "Golden
  corpus"). A V-Carve's path does not depend on the diameter, only on the
  angle, and the clamp is within the 6.35 mm bit's cone height
  (5.5 mm at 60°, 3.175 mm at 90°), so the DreamCarve twin passes the
  cone check with `flat_depth_mm` equal to the clamp.
- The `.camj` turns off export arc fitting (`arcFittingEnabled: false`).
  PureCutCNC's export-stage fitter turns the 18 mm ring of
  `vcarve-offset-square` into four arcs through its corners, a circle
  that runs 2.73 mm past the square's wall (recorded on #410); V-Carve
  paths are straight moves in DreamCarve.
