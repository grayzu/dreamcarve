# Phase 5 — requirements captured ahead of the spec

Items agreed before the phase 5 specs were written. Fold each into
`functional.md` and `design.md` when the phase starts, then delete this file.

## Slot is a first-class drawing tool

Slots are fundamental to CNC work (joinery, adjustable fixtures, T-track,
cable passes), so the Slot tool sits in the drawing toolbar beside Rectangle
and Circle, not as a rounded-rectangle preset.

- **Input:** two centre points and a width. The user clicks the first
  centre, drags or clicks the second, and sets width by typing or by a
  third drag. Width and length are editable afterwards as properties, so a
  slot stays parametric: changing the width keeps the centreline.
- **Geometry:** a closed profile of line, semicircular arc, line,
  semicircular arc, with the arcs centred on the two centre points. Stored
  as a polyline with two bulge = ±1 vertices (exact semicircles), no
  flattening, so the arcs reach G2/G3 untouched.
- **Snapping:** the two centre points and the slot's overall centre are snap
  targets; the centreline is a construction reference for aligning a second
  slot.
- **Operations:** a slot is an ordinary closed shape to every operation.
  Inside Profile on a slot narrower than the tool must raise the
  `ToolTooLarge` warning from phase 1, since that is the classic slot
  mistake.
- **Dimension entry:** manual entry of length (centre to centre) and width,
  mirroring PureCutCNC's `manualEntry.ts` path for slots.

PureCutCNC sources: `slotProfile` in `src/types/project.ts`,
`src/commands/creationShapes.ts`, `useClickPlacement.ts`, `manualEntry.ts`,
`triggerDimensionEdit.ts`, `src/types/slot.test.ts`.

## Other parametric creation shapes from PureCutCNC

Carry these across in the same slice family as Slot, since they share the
click-placement and dimension-entry machinery: regular polygon (`ngon`),
gear, rounded rectangle, chamfered rectangle. Each keeps its defining
parameters editable after creation.
