# Phase 2 — requirements captured ahead of the spec

Items agreed before the phase 2 specs were written. Fold each into
`functional.md` and `design.md` when the phase starts, then delete this file.

## Pocket clearance strategies

The Pocket operation offers two clearance strategies, named as VCarve users
know them (see `CONTEXT.md`):

- **Offset**: concentric passes stepping inward from the boundary by the
  stepover, islands handled as holes, with the tool-centre ring set derived
  by repeated inward offset. Direction (climb/conventional) applies to every
  ring.
- **Raster**: parallel straight passes across the region at a user-set angle
  (degrees from the X axis, default 0), spaced by the stepover, clipped to the
  tool-centre region, linked in a zigzag where the link stays inside the
  region and by a retract otherwise.

Both end with a **wall finish pass** along the tool-centre boundary of the
region (and each island) so the raster's scallops and the offset's corner
cusps are cleaned. The finish pass is on by default and can be turned off.

Parameters shared by both: tool, depth, depth per pass, stepover as a
percentage of tool diameter (default 40%), direction, safe Z, through-cut
allowance (rare for pockets, kept for consistency), entry (plunge, ramp, or
helix; from the phase 2 `entry` port). Raster adds angle.

Rough pockets with a large tool followed by a finish pocket with a smaller
one must leave no uncut material reachable by the smaller tool; PureCutCNC's
`finishFloorCoverage` and its tests are the reference for the coverage check.

PureCutCNC sources: `src/engine/toolpaths/pocket.ts`, `pocketPatterns.ts`
(`OPERATION_PATTERN_SUPPORT`), the `pocketPattern` and `pocketAngle`
operation fields in `src/types/project.ts`, `finishFloorCoverage.ts`,
`clearingControls.ts`. Its `seeded_offset`, `trochoidal`, `waterline`, and
`constant_scallop` patterns are out of scope for phase 2 (the last two are
surface strategies for phase 7).

Golden corpus cases to capture: `pocket-offset-rect`, `pocket-offset-island`,
`pocket-raster-0deg`, `pocket-raster-45deg`, `pocket-rough-then-finish`.
