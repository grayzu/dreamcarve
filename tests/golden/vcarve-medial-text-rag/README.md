# Golden case: vcarve-medial-text-rag

Captured from PureCutCNC commit `2ebab10e5e17c48281ff96805d5e936af135cb58` with
`tools/parity/capture.ts --case vcarve-medial-text-rag --post grblhal`.

- Source: `tools/parity/cases/vcarve-medial-text-rag.camj` (PureCutCNC project format v3.2, units mm)
- Post-processor: PureCutCNC builtin `grblhal`
- Operations: `op1` (v_carve_medial)
- Header date frozen to 2026-01-01 by the capture script.
- Tolerance: `crates/dc-gcode/tests/golden.rs` (`vcarve_medial_text_rag`) compares
  the V-Carve's cuts region by region, in the order both programs visit
  the regions, and the chains of each region as a set: every point of
  every cut that moves in XY, sampled every 0.05 mm, lies within 0.02 mm
  in XYZ (plus each program's rounding of three decimal places) of the
  other program's cuts in that region, both ways. A chain's direction is
  not compared: DreamCarve starts a region at its shallowest chain end
  (#404, decision 3), so it may cut a chain the other way, which cuts the
  same V groove. Neither program cuts below the flat depth, and the
  deepest cut above it is the same in both.
- DreamCarve project: `tests/golden/vcarve-medial-text-rag/dreamcarve.dcv`.
  80 × 35 × 12 mm stock, origin at the bottom-left corner (Y-up) and
  the stock top, so a point at PureCutCNC `y` is at DreamCarve
  `35 − y`.
  The outline text "Rag", 10 mm (Droid Sans Bold), as one shape whose
  profiles are the regions of `tests/fixtures/vcarve/rag.json` (PureCutCNC's
  resolved polygons, #402) placed at (10, 25); medial V-Carve with the
  6.35 mm 60° V-bit at feed 1000, plunge 300, 18000 RPM, step-over
  0.4 (of the floor's cut width), safe Z 5 mm above the stock top,
  `flat_depth_mm: 5`. In the `.camj` it is a `text` feature at
  (10, 10) Y-down, `maxCarveDepth` 5, so the resolver makes the same
  polygons. The twin lists the glyphs in reading order, R, a, g:
  DreamCarve visits shapes nearest-first from the first drawn, upstream
  from the origin, and both then cut R, a, g. No stroke is wider than
  the clamp: both programs cut 4.268 mm deep at most, so neither has
  cuts at the flat depth.
- Intentional difference, `ChainDepthFitted`: where the two programs'
  chains differ in depth at one XY (within 0.02 mm in XY), each
  program's point there is checked against the walls instead of
  against the other. Upstream simplifies each chain depth-aware
  (Douglas-Peucker) and runs straight between the kept points; near a
  reflex corner or a curved wall that takes the cone past the wall. In
  this program 164 cuts reach past a wall by more than 0.005 mm, the
  worst by 0.256 mm, on the stretch from the R's stem into its bowl,
  where upstream runs up to 0.44 mm deeper than DreamCarve. DreamCarve's
  depth fitting (#404, decision 2) splits such a stretch until its cone
  stays inside the walls, which only raises the carve. So DreamCarve's
  point must lie on the V surface (its cone touching a wall, within
  0.02 mm), and a golden point deeper than DreamCarve's must reach past
  a wall. At the foot of the R, DreamCarve is the deeper by up to
  0.074 mm over a few points: its junction is the exact meeting point
  of the two corner bisectors, upstream's 0.037 mm off it, and
  upstream's chord there stops short of the V surface. DreamCarve is
  right for safety in both; recorded on #410.
- Not compared, and why:
  - **Cuts at the flat depth.** DreamCarve's floor trace and V-bit floor
    (#405) are DreamCarve-only: PureCutCNC clamps the carve at
    `maxCarveDepth` but has no floor, and runs its clamped chains there
    instead. Neither program's cuts at the flat depth are compared point
    for point. Instead every golden cut at the clamp must lie within the
    V-bit's flat-depth radius, `F · tan(α/2)`, of a DreamCarve cut at
    that depth, so DreamCarve's floor clears what upstream's clamped
    chains cut.
  - **Links.** Upstream bridges two chain ends tool-down when the gap is
    within 0.9 of the carved radius; DreamCarve links only what passes
    its cone test and retracts otherwise (security review H2, #404
    decision 4). Where the two programs connect chains differently, a
    cut off the other program's path is accepted only as a link: one
    straight move between two points where the other program lifts and
    plunges instead.
  - **Plunges and lifts** (cuts that move only in Z) are not compared
    point for point: where they fall follows the chain order. Their feed
    is the plunge rate in both programs.
- Intentional difference, `DescentsHeldToPlunge`: DreamCarve holds every
  cut that descends while it moves in XY to the plunge rate (S10), at
  the fastest feed that does not fall faster than it; PureCutCNC runs
  them at the cutting feed, faster than the plunge rate (a 60° bit's
  corner arm at F1000 falls at 775 mm/min against the 300 mm/min plunge
  rate). These feeds are not compared; each program is checked against
  its own rule.
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

Files: `program.nc` is the emitted G-code; `toolpath.json` is the
key-sorted toolpath per operation in PureCutCNC's internal (Y-down) frame.
