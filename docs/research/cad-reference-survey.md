# CAD reference survey for phases 4 and 5

Status: research note, 2026-10-24. The phase 5 specs are the authority;
`docs/specs/phase-5/NOTES.md` carries the decision below until they are
written. Licences were read from each repository's licence file on that
date.

## The question

> Are there permissively licensed CAD projects DreamCarve could use as a
> derivative base for its CAD and UI work, instead of starting from
> scratch?

Short answer: PureCutCNC is already that base, and it stays the primary
reference. Graphite is the one other project worth porting from, for
vector-editing tool behaviour. A few permissive Rust crates may help the
canvas. The well-known open source CAD packages are GPL or LGPL and stay
behaviour references at most.

## 1. PureCutCNC is already the CAD base

PureCutCNC (Apache-2.0) has a full drawing layer, about 30 000 lines of
TypeScript, and phase 5's "Read first" in `docs/plan.md` already points at
it:

- `src/sketch/`: constraint solver, driving dimensions, snapping, axis
  lock, feature distribution (array, distribute), gear profiles, text
  placement.
- `src/components/canvas/`: hit testing, manual dimension entry,
  measurements, dimension rendering, draft geometry, and the edit panels
  for sketches, constraints, driving dimensions and text layout.
- `src/text/`: font outlines and baselines.
- `src/commands/`: `creationShapes.ts` and the sketch commands.

Port it as the CAM side was ported: behaviour first, idiomatic Rust, with
the Y-down to Y-up re-derivation ADR-0001 requires.

## 2. Permissive projects to use alongside it

| Project | Licence | Use |
| --- | --- | --- |
| [Graphite](https://github.com/GraphiteEditor/Graphite) | MIT OR Apache-2.0 | Second reference for vector-editing tools, see below. |
| [lyon](https://github.com/nical/lyon) | MIT OR Apache-2.0 | Fill tessellation for wgpu, if egui's own painter is not enough for the canvas. |
| [vello](https://github.com/linebender/vello) | Apache-2.0 OR MIT | GPU 2D renderer from the `kurbo` authors; an option for the canvas, not a requirement. |
| [rerun](https://github.com/rerun-io/rerun) | MIT OR Apache-2.0 | A large egui + wgpu application: panel layout, a viewport inside egui, app structure. |

### Graphite

An actively developed 2D vector editor whose editor logic is Rust and which
uses `kurbo` 0.13, as DreamCarve does. The parts that match phase 5:

- Tools in `editor/src/messages/tool/tool_messages/`: `pen_tool.rs`,
  `path_tool.rs` (node and handle editing), `shape_tool.rs`,
  `spline_tool.rs`, `select_tool.rs`, `text_tool.rs`.
- Shared tool machinery in `editor/src/messages/tool/common_functionality/`:
  `snap_session.rs` and `libraries/snapping`, `transformation_cage.rs`,
  `measure.rs`, `pivot.rs`, `resize.rs`, `shape_editor.rs`.

Limits:

- The UI is a web frontend (Svelte), not egui. Port tool behaviour and
  geometry, not UI code.
- Its documents are a node graph; DreamCarve's are feature definitions and
  instances (`CONTEXT.md`). Do not carry the node-graph model across.
- Booleans go through `linesweeper` (`node-graph/nodes/path-bool`).
  DreamCarve's choice, `i_overlay`, stands (`docs/plan.md`, settled
  decisions).

A port from Graphite takes the ported-file header pattern in AGENTS.md with
Graphite's copyright line, and Graphite goes in `NOTICE`'s third-party
section. Graphite has no NOTICE file of its own to carry over.

## 3. Not usable as a code source

| Project | Licence | Why not |
| --- | --- | --- |
| LibreCAD | GPL-2.0 | GPL is not Apache-2.0-compatible (AGENTS.md). |
| QCAD | GPL-3.0 (optional exceptions) | As above. |
| SolveSpace | GPL-3.0 | As above. |
| Dune 3D | GPL-3.0 | As above. |
| Inkscape | GPL | As above. |
| OpenSCAD | GPL-2.0 | As above. |
| FreeCAD, Open CASCADE | LGPL-2.1 | LGPL needs a separate review (AGENTS.md); a Rust port of either is still under LGPL terms. |
| CADmium | Elastic License 2.0 | Source-available, not open source, and incompatible. Archived. |
| truck | Apache-2.0 | Permissive, but a B-rep solid kernel; ADR-0003 rules out solids. |
| Fornjot | 0BSD | As truck; archived, no longer developed. |
| Zoo Design Studio | MIT | Only the app is open; the geometry engine is a closed cloud service. Parametric 3D. |
| CadQuery | Apache-2.0 | Python scripting over Open CASCADE (LGPL). |

The GPL projects may be read for behaviour only, as the toolpath-ordering
research did with FreeCAD and phase 3 does with RasterCarve: describe what
they do, design from the description, and re-derive the tests. No code,
structure or tests are ported.

## 4. Decision

PureCutCNC stays the primary reference for phase 5. Graphite is the
secondary reference for vector-editing tools (pen, node editing, snapping,
transform handles, measurement), ported for behaviour, not UI. Recorded in
`docs/specs/phase-5/NOTES.md`.
