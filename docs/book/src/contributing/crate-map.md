# Crate map

DreamCarve is a Cargo workspace. Library crates carry a `dc-` prefix; the
command line binary is `dreamcarve`. Dependencies point one way, from the
bottom of this list to the top.

| Crate | Owns | Depends on |
| --- | --- | --- |
| `dreamcarve` | The command line (phase 1) and later the GUI (phase 4). All file, terminal, and exit-code handling. | everything below |
| `dc-import` | Importers: SVG (phase 1), DXF (phase 2), fonts and tool libraries later. Flips imported Y into the internal Y-up frame. | `dc-geometry`, `dc-project` |
| `dc-gcode` | Post-processor model and G-code emission. Checks every safety precondition before writing a move. | `dc-geometry`, `dc-project` |
| `dc-cam` | Toolpath generators: Profile (phase 1), Pocket, Drill, V-Carve, Engrave, surface strategies later. Pure functions from project and operation to toolpath plus warnings. | `dc-geometry`, `dc-project` |
| `dc-project` | The project data model, `.dcv` serialization, versioning and migrations, undo history. | `dc-geometry` |
| `dc-geometry` | Points, polylines with bulge arcs, transforms, offsets, flattening. Knows nothing about tools or stock. | nothing internal |

Outside the workspace crates:

- `tools/check-headers`: the license header checker CI runs.
- `tools/parity/`: a Node script that drives a local PureCutCNC checkout to
  capture golden outputs. Not needed to build or test DreamCarve.
- `tests/golden/`: the captured PureCutCNC outputs that `cargo test` compares
  against.
- `docs/book/`: this site.

The design spec of each phase is the authority on where new code goes. Rules
for what may live in each crate are in `docs/rust-guidelines.md`, "Crate
boundaries".
