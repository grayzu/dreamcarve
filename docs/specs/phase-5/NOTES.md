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

## Graphite is the secondary reference for vector-editing tools

PureCutCNC stays the primary reference for drawing and editing. For the
vector-editing tools (pen, node and handle editing, snapping, transform
handles, measurement), also read
[Graphite](https://github.com/GraphiteEditor/Graphite) (MIT OR Apache-2.0,
Rust, `kurbo` 0.13), and say in the spec where its behaviour is taken over
PureCutCNC's and why.

- Port tool behaviour and geometry, not UI code: Graphite's UI is a Svelte
  web frontend, and its documents are a node graph rather than feature
  definitions and instances.
- Booleans stay on `i_overlay`, not Graphite's `linesweeper`.
- A file ported from Graphite takes the ported-file header from AGENTS.md
  with Graphite's copyright line, and Graphite is added to `NOTICE`.

Graphite sources: `editor/src/messages/tool/tool_messages/` (`pen_tool.rs`,
`path_tool.rs`, `shape_tool.rs`, `spline_tool.rs`, `select_tool.rs`,
`text_tool.rs`), `editor/src/messages/tool/common_functionality/`
(`snap_session.rs`, `transformation_cage.rs`, `measure.rs`, `pivot.rs`,
`shape_editor.rs`), `libraries/snapping`. The survey behind this choice,
including the CAD projects ruled out by licence, is
`docs/research/cad-reference-survey.md`.
