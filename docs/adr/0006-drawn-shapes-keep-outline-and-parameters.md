---
status: accepted
date: 2026-11-22
---

# A drawn shape keeps its outline, and beside it the sizes it was drawn with

Until phase 5 a feature definition was its outline and nothing else:
`profiles`, polylines of lines and circular arcs (ADR-0001, the phase 1
design spec). Every generator, every check and the CLI read that one
field. Phase 5 draws shapes in the app, and three of its requirements do
not fit an outline alone:

- a Slot, a rounded rectangle, a regular polygon, a gear and a line of
  text must stay editable by the sizes they were drawn with
  (`docs/specs/phase-5/functional.md`, capability 84): changing a slot's
  width keeps its centreline;
- a spline and an ellipse are curves, and a polyline holds none: the
  settled decision is "Béziers flattened at import to a project
  tolerance" (`docs/plan.md`);
- text needs a font to become an outline, and a library crate may not
  read one from disk (`docs/rust-guidelines.md`, "Crate boundaries").

PureCutCNC answers the first with its *canned shapes*: the definition
stores a kind and its parameters and the outline is generated every time
it is read (`src/cannedShapes/`, its `INDEX.md`, "Rules"). Its text works
the same way.

## Decision

A definition keeps `profiles`, and gains `shape`: `null` for a plain
outline, or the record of how the outline was made.

- **`profiles` is what is cut.** Generators, the artwork checks, the
  preview, the safety pass and the CLI read `profiles` and never read
  `shape`. A project made in the app exports from the command line on a
  machine that has neither the app nor the font.
- **`shape` is how the app edits it.** A rectangle's width, height and
  corner; a circle's diameter; an ellipse's two radii; a regular
  polygon's sides and radius; a slot's length and width; a gear's teeth
  and their form; a text's string, font and size; a path's nodes with
  their curves. A command that changes a parameter writes the new
  `shape` and the `profiles` generated from it, in one snapshot.
- **A curve lives in `shape` only.** A path with a Bézier segment is
  `shape: { "kind": "path", … }` holding the nodes and their handles;
  its `profiles` are that path flattened at the project's
  `flatten_tolerance_mm`, with its lines and arcs kept as they are. A
  path of lines and arcs alone needs no `shape`: its `profiles` say
  everything, as an imported drawing's do.
- **The two are checked against each other where that needs no font.**
  For every kind but text, validation generates the outline from `shape`
  and compares it with `profiles`; a difference past `EPS_MM` is the
  warning `shape_outline_differs`, since the cut is still what
  `profiles` says. A text's outline is not checked: `dc-project` has no
  fonts.

## Why not generate on read, as PureCutCNC does

- Every reader of an outline would need the generator, and for text a
  font: `dc-cam`, `dc-job`'s checks, the CLI. Fonts in `dc-project`
  would mean bundled font bytes under every command, and a user's font
  file read by a library crate.
- A project using a user's font would not export on a machine without
  that font. With the outline stored it exports anywhere, and only
  *editing the text* needs the font.
- The phase 4 rule stands unchanged: the engine is as phase 3 left it,
  and what the Viewport draws is what will be cut (S20).

## Consequences

- The file holds the outline twice for a parametric shape, once as sizes
  and once as vertices. A hand edit of one and not the other is the
  warning above; the app offers to regenerate.
- A text whose font is not on this computer still shows and cuts. Its
  Properties say the font is missing and keep the string; choosing a
  font that is here regenerates it.
- A parametric shape is edited by its sizes, not node by node, until the
  user converts it to a path, which drops `shape` (or leaves a `path`
  record when the outline has curves).
- Format 29 adds the field; a migrated definition has `shape: null`.
