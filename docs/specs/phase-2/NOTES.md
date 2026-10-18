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

## Multi-tool export

When a project's enabled operations use more than one tool, export writes
either **one program** with a tool change between tools, or **one program per
tool**. The user decides:

- A **default** lives in the user config directory (introduced in phase 2
  for the tool library), so it is set once, not on every export.
- Each export can **override** the default (for example
  `dreamcarve export --single-file` or `--file-per-tool`; final flag names
  are the spec's call).
- With no user default set, export uses one program per tool, because most
  hobby routers have no automatic tool changer and stop on a tool change
  differently per controller. The spec may revisit this built-in default.

One program per tool names files so their order is obvious (operation order,
tool number, tool name). Every program, either way, starts with its own
units command, work-coordinate select and safe-Z move, and stays within the
phase 1 safety rules S1–S6. A single program's tool change uses the
post-processor's `tool_change` block, retracting to safe Z and stopping the
spindle first.

## Cut order within and between operations

Research and sources: `docs/research/toolpath-ordering.md` (decisions D1–D10
in section 7, invariants O1–O10 in section 6). Terms: **Machining Order** and
**Link** in `CONTEXT.md`.

- **Machining order** is a per-operation setting on Profile, Pocket and
  Engrave: `full_depth_first` (default) or `pass_first`. Drill ignores it;
  V-Carve decides in phase 3. A missing field means `full_depth_first`, so
  phase 1 files load unchanged (a serde default, not a migration). There is
  no project, export or user-config default, so a `.dcv` cuts the same on
  every machine. The default matches PureCutCNC's default for new operations
  (`feature_first`).
- **Containment order** is a fixed rule with no setting: within an operation,
  a contour's freeing pass comes after the last pass of every contour it
  encloses, at every nesting depth. (Phase 1 fixes the island case.)
- **Nearest-neighbour order** of shapes, drill points and pocket regions,
  with no setting, constrained by containment order, starting from the first
  shape in drawing order, ties broken by drawing order. Output is
  deterministic. The changelog notes that multi-part projects change cut
  order (not moves).
- **Operation order** is the user's list. DreamCarve never reorders
  operations or groups them by tool. `dreamcarve check` and export warn
  `CutInsideFreedPart` when an operation cuts inside a shape an earlier
  operation freed, and hint when the same tool is used in non-adjacent
  operations.
- **Links** between shapes retract to Safe Z. A link stays down only where
  its whole path lies in material the operation has already cut at that
  depth. No lower retract height and no keep-tool-down option.
- **Architecture**: generators produce cut blocks (one contour, open path or
  drill point at one pass depth); one pure `dc_cam::sequence` step orders
  them and writes the links. `dc-gcode` never reorders (S6).
  `generate_profile` moves onto `sequence` early in the phase.
- Direction alternation on open paths is the Engrave spec's call, off by
  default. Merging the outlines of close shapes per level (PureCutCNC's
  `edgeLevelRuns`) is deferred; phase 1's overlap warning covers the risk.
- Golden cases: one multi-part case per machining order, compared in order,
  not only as per-level sets.

